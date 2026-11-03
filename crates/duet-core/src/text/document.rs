//! Loading a text file for comparison: binary check, encoding, line index, edit limits.
//!
//! Files above EDIT_LIMIT are memory-mapped and view-only. Their content is only exposed as
//! bytes or through checked/lossy UTF-8 conversion, never as an unchecked &str.

use std::borrow::Cow;
use std::fs::{File, Metadata};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use encoding_rs::{Encoding, UTF_8};
use memmap2::Mmap;

use crate::cancel::{CancelToken, Cancelled};
use crate::text::encoding::{TextEncoding, decode_as, detect, is_binary};
use crate::text::lines::{EolStyle, LineIndex, detect_eol};

/// Files larger than this are memory-mapped and opened read-only.
pub const EDIT_LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    pub len: u64,
    pub modified: Option<SystemTime>,
}

impl FileStamp {
    pub fn of(meta: &Metadata) -> Self {
        Self {
            len: meta.len(),
            modified: meta.modified().ok(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOnlyReason {
    TooLarge,
    LossyDecode,
    /// A pretty-printed view (Format button); the file itself is untouched.
    Formatted,
    /// The file has the read-only attribute (C23): clear it to edit the file in duet.
    Attribute,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path} is a binary file")]
    Binary { path: PathBuf },
    #[error("{path} is binary and over 64 MiB, too large to open as text.")]
    TooLargeAsText { path: PathBuf },
    #[error(transparent)]
    Cancelled(#[from] Cancelled),
}

#[derive(Debug, Clone)]
enum Storage {
    Owned(String),
    Mapped { map: Arc<Mmap>, start: usize },
}

#[derive(Debug)]
pub struct TextDocument {
    path: PathBuf,
    storage: Storage,
    lines: LineIndex,
    encoding: TextEncoding,
    eol: EolStyle,
    lossy: bool,
    stamp: FileStamp,
    read_only: Option<ReadOnlyReason>,
}

impl TextDocument {
    pub fn open(path: &Path, cancel: &CancelToken) -> Result<Self, LoadError> {
        Self::load(path, None, false, cancel)
    }

    pub fn open_with_encoding(
        path: &Path,
        encoding: &'static Encoding,
        cancel: &CancelToken,
    ) -> Result<Self, LoadError> {
        Self::load(path, Some(encoding), false, cancel)
    }

    /// Opens a file that looks binary (NUL bytes) as text anyway: decoded as `encoding`, or as
    /// detected, and editable. Above `EDIT_LIMIT` a binary file is refused (`TooLargeAsText`).
    pub fn open_as_text(
        path: &Path,
        encoding: Option<&'static Encoding>,
        cancel: &CancelToken,
    ) -> Result<Self, LoadError> {
        Self::load(path, encoding, true, cancel)
    }

    fn load(
        path: &Path,
        forced: Option<&'static Encoding>,
        as_text: bool,
        cancel: &CancelToken,
    ) -> Result<Self, LoadError> {
        let io_err = |source| LoadError::Io {
            path: path.to_path_buf(),
            source,
        };
        cancel.check()?;
        let mut file = File::open(path).map_err(io_err)?;
        let meta = file.metadata().map_err(io_err)?;
        let stamp = FileStamp::of(&meta);
        if meta.len() > EDIT_LIMIT {
            // SAFETY: memmap2 requires that the file is not modified while mapped. duet maps
            // only files it views read-only, and Windows refuses to truncate a mapped file.
            // In-place writes by another process are an accepted risk for >64 MiB view-only
            // files; every &str view of the bytes is produced by checked or lossy conversion.
            let map = unsafe { Mmap::map(&file) }.map_err(io_err)?;
            return Self::from_mapped(path, map, forced, as_text, stamp, cancel);
        }
        let mut bytes = Vec::with_capacity(meta.len() as usize);
        file.read_to_end(&mut bytes).map_err(io_err)?;
        let mut doc = Self::from_bytes(path, &bytes, forced, as_text, stamp, cancel)?;
        // The read-only attribute makes a read-only side as well (C23).
        if meta.permissions().readonly() && doc.read_only.is_none() {
            doc.read_only = Some(ReadOnlyReason::Attribute);
        }
        Ok(doc)
    }

    fn from_mapped(
        path: &Path,
        map: Mmap,
        forced: Option<&'static Encoding>,
        as_text: bool,
        stamp: FileStamp,
        cancel: &CancelToken,
    ) -> Result<Self, LoadError> {
        if is_binary(&map) {
            // A mapped file is view-only, and a binary one would be shown as garbage: no copy.
            return Err(if as_text {
                LoadError::TooLargeAsText {
                    path: path.to_path_buf(),
                }
            } else {
                LoadError::Binary {
                    path: path.to_path_buf(),
                }
            });
        }
        let encoding = forced.unwrap_or_else(|| detect(&map));
        cancel.check()?;
        if encoding == UTF_8 {
            let start = if map.starts_with(&[0xEF, 0xBB, 0xBF]) {
                3
            } else {
                0
            };
            if std::str::from_utf8(&map[start..]).is_ok() {
                let lines = LineIndex::new(&map[start..]);
                cancel.check()?;
                let eol = detect_eol(&map[start..]);
                return Ok(Self {
                    path: path.to_path_buf(),
                    storage: Storage::Mapped {
                        map: Arc::new(map),
                        start,
                    },
                    lines,
                    encoding: TextEncoding {
                        encoding,
                        bom: start == 3,
                    },
                    eol,
                    lossy: false,
                    stamp,
                    read_only: Some(ReadOnlyReason::TooLarge),
                });
            }
        }
        let mut doc = Self::from_bytes(path, &map, Some(encoding), as_text, stamp, cancel)?;
        doc.read_only = Some(ReadOnlyReason::TooLarge);
        Ok(doc)
    }

    fn from_bytes(
        path: &Path,
        bytes: &[u8],
        forced: Option<&'static Encoding>,
        as_text: bool,
        stamp: FileStamp,
        cancel: &CancelToken,
    ) -> Result<Self, LoadError> {
        if is_binary(bytes) && !as_text {
            return Err(LoadError::Binary {
                path: path.to_path_buf(),
            });
        }
        let decoded = decode_as(bytes, forced.unwrap_or_else(|| detect(bytes)));
        cancel.check()?;
        let text = decoded.text.into_owned();
        let lines = LineIndex::new(text.as_bytes());
        let eol = detect_eol(text.as_bytes());
        Ok(Self {
            path: path.to_path_buf(),
            storage: Storage::Owned(text),
            lines,
            encoding: decoded.encoding,
            eol,
            lossy: decoded.lossy,
            stamp,
            read_only: decoded.lossy.then_some(ReadOnlyReason::LossyDecode),
        })
    }

    /// A read-only copy that shows `text`, a pretty-printed form of this document, in its
    /// place. Path, encoding and file stamp stay the same.
    pub fn formatted(&self, text: String) -> TextDocument {
        let lines = LineIndex::new(text.as_bytes());
        let eol = detect_eol(text.as_bytes());
        TextDocument {
            path: self.path.clone(),
            storage: Storage::Owned(text),
            lines,
            encoding: self.encoding,
            eol,
            lossy: false,
            stamp: self.stamp,
            read_only: Some(ReadOnlyReason::Formatted),
        }
    }

    /// A writable copy holding `text`, the edited form of this document: same path, encoding
    /// and line-ending style, stamped `stamp` (the load stamp until it is saved).
    pub fn edited(&self, text: String, stamp: FileStamp) -> TextDocument {
        let lines = LineIndex::new(text.as_bytes());
        TextDocument {
            path: self.path.clone(),
            storage: Storage::Owned(text),
            lines,
            encoding: self.encoding,
            eol: self.eol,
            lossy: false,
            stamp,
            read_only: None,
        }
    }

    /// An empty, editable document with no file (a side of an untitled compare).
    pub fn untitled() -> TextDocument {
        TextDocument {
            path: PathBuf::new(),
            storage: Storage::Owned(String::new()),
            lines: LineIndex::new(b""),
            encoding: TextEncoding {
                encoding: UTF_8,
                bom: false,
            },
            eol: EolStyle::None,
            lossy: false,
            stamp: FileStamp {
                len: 0,
                modified: None,
            },
            read_only: None,
        }
    }

    /// The same document under another path (Save As): text, encoding, line endings, stamp
    /// and read-only state carry over, except the read-only attribute, which stays with the
    /// old file.
    pub fn with_path(&self, path: PathBuf) -> TextDocument {
        let read_only = match self.read_only {
            Some(ReadOnlyReason::Attribute) if path != self.path => None,
            other => other,
        };
        TextDocument {
            path,
            storage: self.storage.clone(),
            lines: self.lines.clone(),
            encoding: self.encoding,
            eol: self.eol,
            lossy: self.lossy,
            stamp: self.stamp,
            read_only,
        }
    }

    /// The same document read as `encoding`: what the next plain save writes.
    pub fn with_encoding(&self, encoding: TextEncoding) -> TextDocument {
        TextDocument {
            encoding,
            ..self.with_path(self.path.clone())
        }
    }

    /// The same document with `eol` as its line-ending style: what the next plain save writes.
    /// The text is not touched.
    pub fn with_eol(&self, eol: EolStyle) -> TextDocument {
        TextDocument {
            eol,
            ..self.with_path(self.path.clone())
        }
    }

    /// The same document with `stamp` as its disk stamp.
    pub fn with_stamp(mut self, stamp: FileStamp) -> TextDocument {
        self.stamp = stamp;
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn bytes(&self) -> &[u8] {
        match &self.storage {
            Storage::Owned(text) => text.as_bytes(),
            Storage::Mapped { map, start } => &map[*start..],
        }
    }

    /// The full decoded text; `None` for memory-mapped (view-only) documents.
    pub fn text(&self) -> Option<&str> {
        match &self.storage {
            Storage::Owned(text) => Some(text),
            Storage::Mapped { .. } => None,
        }
    }

    pub fn lines(&self) -> &LineIndex {
        &self.lines
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, line: usize) -> Cow<'_, str> {
        String::from_utf8_lossy(self.lines.line_bytes(self.bytes(), line))
    }

    pub fn encoding(&self) -> TextEncoding {
        self.encoding
    }

    pub fn eol(&self) -> EolStyle {
        self.eol
    }

    pub fn is_lossy(&self) -> bool {
        self.lossy
    }

    pub fn read_only_reason(&self) -> Option<ReadOnlyReason> {
        self.read_only
    }

    pub fn stamp(&self) -> FileStamp {
        self.stamp
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufWriter, Write};

    fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn with_path_keeps_text_encoding_and_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        std::fs::write(&p, "x\r\n").unwrap();
        let doc = TextDocument::open(&p, &CancelToken::new()).unwrap();
        let moved = doc.with_path(dir.path().join("b.txt"));
        assert_eq!(moved.path(), dir.path().join("b.txt"));
        assert_eq!(
            (moved.text(), moved.eol(), moved.encoding(), moved.stamp()),
            (doc.text(), doc.eol(), doc.encoding(), doc.stamp())
        );
    }

    #[test]
    fn with_encoding_changes_only_the_encoding() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        std::fs::write(&p, "x\r\n").unwrap();
        let doc = TextDocument::open(&p, &CancelToken::new()).unwrap();
        let wanted = TextEncoding {
            encoding: encoding_rs::WINDOWS_874,
            bom: false,
        };
        let moved = doc.with_encoding(wanted);
        assert_eq!(moved.encoding(), wanted);
        assert_eq!(
            (moved.path(), moved.text(), moved.eol(), moved.stamp()),
            (doc.path(), doc.text(), doc.eol(), doc.stamp())
        );
    }

    #[test]
    fn small_utf8_file_is_editable() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "a.txt", "a\r\nข\n".as_bytes());
        let doc = TextDocument::open(&path, &CancelToken::new()).unwrap();
        assert_eq!(doc.line_count(), 2);
        assert_eq!(doc.line(1), "ข");
        assert_eq!(doc.eol(), EolStyle::Mixed);
        assert_eq!(doc.text(), Some("a\r\nข\n"));
        assert_eq!(
            doc.encoding(),
            TextEncoding {
                encoding: UTF_8,
                bom: false
            }
        );
        assert_eq!(doc.read_only_reason(), None);
        assert_eq!(doc.stamp().len, 7);
    }

    #[test]
    fn a_binary_file_opens_as_text_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "b.bin", b"a\0b\nc\n");
        let cancel = CancelToken::new();
        let err = TextDocument::open(&path, &cancel).unwrap_err();
        assert!(matches!(err, LoadError::Binary { .. }), "{err:?}");
        let doc = TextDocument::open_as_text(&path, None, &cancel).unwrap();
        assert_eq!(doc.line_count(), 2);
        assert_eq!(doc.line(0), "a\0b");
        assert_eq!(doc.read_only_reason(), None);
        // "ab\n" in UTF-16LE without a BOM, which only a forced encoding decodes.
        let wide = write(&dir, "w.bin", &[0x61, 0, 0x62, 0, 0x0a, 0]);
        let doc = TextDocument::open_as_text(&wide, Some(encoding_rs::UTF_16LE), &cancel).unwrap();
        assert_eq!(doc.line(0), "ab");
        assert_eq!(doc.read_only_reason(), None);
    }

    #[test]
    fn a_binary_file_above_the_edit_limit_is_not_opened_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.bin");
        let file = File::create(&path).unwrap();
        file.set_len(EDIT_LIMIT + 1).unwrap();
        drop(file);
        let err = TextDocument::open_as_text(&path, None, &CancelToken::new()).unwrap_err();
        assert!(matches!(err, LoadError::TooLargeAsText { .. }), "{err:?}");
        assert!(err.to_string().contains("64 MiB"), "{err}");
    }

    #[test]
    fn binary_file_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "b.bin", b"MZ\0\0\x90");
        let err = TextDocument::open(&path, &CancelToken::new()).unwrap_err();
        assert!(matches!(err, LoadError::Binary { .. }), "{err:?}");
    }

    #[test]
    fn odd_length_utf16_is_lossy_and_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "u.txt", &[0xFF, 0xFE, 0x41, 0x00, 0x42]);
        let doc = TextDocument::open(&path, &CancelToken::new()).unwrap();
        assert!(doc.is_lossy());
        assert_eq!(doc.read_only_reason(), Some(ReadOnlyReason::LossyDecode));
    }

    #[test]
    fn forced_wrong_encoding_is_lossy_and_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "l.txt", b"abc\xFFdef");
        let doc = TextDocument::open_with_encoding(&path, UTF_8, &CancelToken::new()).unwrap();
        assert!(doc.is_lossy());
        assert_eq!(doc.read_only_reason(), Some(ReadOnlyReason::LossyDecode));
    }

    #[test]
    fn large_file_is_mapped_and_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.txt");
        let lines = (EDIT_LIMIT + 1) / 5 + 1; // "line\n" is 5 bytes
        {
            let mut out = BufWriter::new(File::create(&path).unwrap());
            for _ in 0..lines {
                out.write_all(b"line\n").unwrap();
            }
        }
        let doc = TextDocument::open(&path, &CancelToken::new()).unwrap();
        assert_eq!(doc.read_only_reason(), Some(ReadOnlyReason::TooLarge));
        assert_eq!(doc.text(), None);
        assert_eq!(doc.line_count() as u64, lines);
        assert_eq!(doc.line(0), "line");
        assert_eq!(doc.bytes().len() as u64, lines * 5);
    }

    #[test]
    fn cancelled_load_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "c.txt", b"abc");
        let cancel = CancelToken::new();
        cancel.cancel();
        let err = TextDocument::open(&path, &cancel).unwrap_err();
        assert!(matches!(err, LoadError::Cancelled(_)), "{err:?}");
    }

    #[test]
    fn missing_file_is_io_error() {
        let err = TextDocument::open(Path::new("Z:/definitely/missing.txt"), &CancelToken::new())
            .unwrap_err();
        assert!(matches!(err, LoadError::Io { .. }), "{err:?}");
    }

    #[test]
    fn an_edited_copy_is_writable_and_keeps_the_encoding() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "\u{feff}one\r\ntwo\r\n").unwrap();
        let doc = TextDocument::open(&path, &CancelToken::new()).unwrap();
        let edited = doc.edited("one\r\n2\r\n".into(), doc.stamp());
        assert_eq!(edited.read_only_reason(), None);
        assert!(edited.encoding().bom);
        assert_eq!(edited.eol(), EolStyle::CrLf);
        assert_eq!(edited.line(1), "2");
    }

    #[test]
    fn a_formatted_copy_is_read_only_and_keeps_the_encoding() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        std::fs::write(&path, "\u{feff}{\"a\":1}").unwrap();
        let doc = TextDocument::open(&path, &CancelToken::new()).unwrap();
        let shown = doc.formatted("{\n  \"a\": 1\n}".into());
        assert_eq!(shown.line_count(), 3);
        assert_eq!(shown.read_only_reason(), Some(ReadOnlyReason::Formatted));
        assert!(shown.encoding().bom);
        assert_eq!(shown.path(), doc.path());
    }

    #[test]
    fn an_untitled_document_is_empty_and_editable() {
        let doc = TextDocument::untitled();
        assert_eq!(doc.path(), Path::new(""));
        assert_eq!(doc.line_count(), 0);
        assert_eq!(doc.text(), Some(""));
        assert_eq!(doc.read_only_reason(), None);
        assert_eq!(doc.encoding().encoding, UTF_8);
        assert!(!doc.encoding().bom);
        assert_eq!(doc.eol(), EolStyle::None);
        assert_eq!(
            doc.stamp(),
            FileStamp {
                len: 0,
                modified: None
            }
        );
        assert!(!doc.is_lossy());
    }

    fn set_read_only(path: &Path, on: bool) {
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_readonly(on);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn a_file_with_the_read_only_attribute_opens_read_only() {
        // C23: the attribute makes a read-only side, as a file over 64 MiB is one.
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "ro.txt", b"a\r\n");
        set_read_only(&path, true);
        let doc = TextDocument::open(&path, &CancelToken::new());
        set_read_only(&path, false);
        let doc = doc.unwrap();
        assert_eq!(doc.read_only_reason(), Some(ReadOnlyReason::Attribute));
        assert_eq!(doc.text(), Some("a\r\n"), "still read whole");
        // The same file read another way keeps it; Save As writes a new file, which has none.
        let western = TextEncoding {
            encoding: encoding_rs::WINDOWS_1252,
            bom: false,
        };
        assert_eq!(
            doc.with_encoding(western).read_only_reason(),
            Some(ReadOnlyReason::Attribute)
        );
        assert_eq!(
            doc.with_eol(EolStyle::Lf).read_only_reason(),
            Some(ReadOnlyReason::Attribute)
        );
        assert_eq!(
            doc.with_path(dir.path().join("b.txt")).read_only_reason(),
            None
        );
        // A writable file stays editable.
        let doc = TextDocument::open(&path, &CancelToken::new()).unwrap();
        assert_eq!(doc.read_only_reason(), None);
    }
}
