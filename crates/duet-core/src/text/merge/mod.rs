//! 3-way text merge: the three texts, reading them, diff3 regions and card marks.

pub mod diff3;
pub mod marks;
pub mod output;
pub mod session;

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::cancel::CancelToken;
use crate::text::encoding::{self, TextEncoding};
use crate::text::lines::LineIndex;

/// Largest file a merge reads (LocalDiff `MAX_SIZE`).
pub const MAX_MERGE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    Lf,
    CrLf,
    Cr,
}

impl Eol {
    pub fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::CrLf => "\r\n",
            Eol::Cr => "\r",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Eol::Lf => "LF",
            Eol::CrLf => "CRLF",
            Eol::Cr => "CR",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextFormat {
    pub eol: Eol,
    pub final_newline: bool,
    pub mixed: bool,
}

/// The majority line ending, final newline and mix of a text (LocalDiff `ldSplitLines`).
pub fn text_format(text: &str) -> TextFormat {
    let bytes = text.as_bytes();
    let (mut crlf, mut lf, mut cr) = (0usize, 0usize, 0usize);
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                crlf += 1;
                i += 1;
            }
            b'\r' => cr += 1,
            b'\n' => lf += 1,
            _ => {}
        }
        i += 1;
    }
    let eol = if crlf >= lf && crlf >= cr && crlf > 0 {
        Eol::CrLf
    } else if cr > lf {
        Eol::Cr
    } else {
        Eol::Lf
    };
    let kinds = usize::from(crlf > 0) + usize::from(lf > 0) + usize::from(cr > 0);
    TextFormat {
        eol,
        final_newline: matches!(bytes.last(), Some(b'\r' | b'\n')),
        mixed: kinds > 1,
    }
}

/// One side of a merge: its text split into lines, and where it came from.
#[derive(Debug, Clone)]
pub struct MergeText {
    text: String,
    lines: LineIndex,
    pub encoding: TextEncoding,
    pub format: TextFormat,
    pub lossy: bool,
    pub name: String,
    pub path: Option<PathBuf>,
    pub size: u64,
}

impl MergeText {
    pub fn new(text: String, encoding: TextEncoding) -> MergeText {
        let lines = LineIndex::new(text.as_bytes());
        let format = text_format(&text);
        let size = text.len() as u64;
        MergeText {
            text,
            lines,
            encoding,
            format,
            lossy: false,
            name: String::new(),
            path: None,
            size,
        }
    }

    /// Typed or pasted text: UTF-8 without a BOM.
    pub fn typed(text: String) -> MergeText {
        MergeText::new(
            text,
            TextEncoding {
                encoding: encoding_rs::UTF_8,
                bom: false,
            },
        )
    }

    /// A file's own line ending for its edited text (the text may now have none, or a mix).
    pub fn keeping_eol(mut self, eol: Eol) -> MergeText {
        self.format = TextFormat {
            eol,
            final_newline: self.format.final_newline,
            mixed: false,
        };
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn line_count(&self) -> u32 {
        self.lines.len() as u32
    }

    /// Line `line` without its terminator.
    pub fn line(&self, line: u32) -> &str {
        let span = self.lines.span_with_eol(line as usize);
        // A line holds no CR or LF, so what trails it is the terminator.
        self.text[span].trim_end_matches(['\r', '\n'])
    }

    pub fn lines(&self, range: Range<u32>) -> Vec<String> {
        range.map(|n| self.line(n).to_owned()).collect()
    }

    pub(crate) fn line_index(&self) -> &LineIndex {
        &self.lines
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ReadError {
    #[error("the file is larger than {0} MB.")]
    FileTooLarge(u64),
    #[error("it is a binary file")]
    Binary,
    #[error("{0}.")]
    Io(String),
    #[error("operation cancelled")]
    Cancelled,
}

impl From<crate::cancel::Cancelled> for ReadError {
    fn from(_: crate::cancel::Cancelled) -> Self {
        ReadError::Cancelled
    }
}

/// Reads one side. Refuses by metadata before reading; sets the name, path and size.
pub fn read_side(
    path: &Path,
    max_bytes: u64,
    cancel: &CancelToken,
) -> Result<MergeText, ReadError> {
    let io = |e: std::io::Error| ReadError::Io(e.to_string());
    let meta = fs::metadata(path).map_err(io)?;
    if meta.len() > max_bytes {
        return Err(ReadError::FileTooLarge(max_bytes / (1024 * 1024)));
    }
    let bytes = fs::read(path).map_err(io)?;
    cancel.check()?;
    if encoding::is_binary(&bytes) {
        return Err(ReadError::Binary);
    }
    let decoded = encoding::decode(&bytes);
    cancel.check()?;
    let mut side = MergeText::new(decoded.text.into_owned(), decoded.encoding);
    side.lossy = decoded.lossy;
    side.name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    side.path = Some(path.to_path_buf());
    side.size = bytes.len() as u64;
    Ok(side)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(eol: Eol, final_newline: bool, mixed: bool) -> TextFormat {
        TextFormat {
            eol,
            final_newline,
            mixed,
        }
    }

    #[test]
    fn text_formats_follow_localdiff() {
        assert_eq!(text_format(""), fmt(Eol::Lf, false, false));
        assert_eq!(text_format("a\r\nb\r\n"), fmt(Eol::CrLf, true, false));
        assert_eq!(text_format("a\nb"), fmt(Eol::Lf, false, false));
        assert_eq!(text_format("a\rb\r"), fmt(Eol::Cr, true, false));
        assert_eq!(text_format("a\r\nb\nc\n"), fmt(Eol::Lf, true, true));
        assert_eq!(text_format("a\r\nb\n"), fmt(Eol::CrLf, true, true));
        assert_eq!(text_format("a\rb\n"), fmt(Eol::Lf, true, true));
        assert_eq!(text_format("a\r\rb\n"), fmt(Eol::Cr, true, true));

        let typed = MergeText::typed("a\r\nb".into());
        assert_eq!(typed.line_count(), 2);
        assert_eq!(typed.line(0), "a");
        assert_eq!(typed.lines(0..2), ["a", "b"]);
        assert_eq!(
            typed.keeping_eol(Eol::Cr).format,
            fmt(Eol::Cr, false, false)
        );
        assert_eq!(Eol::CrLf.as_str(), "\r\n");
        assert_eq!(Eol::Cr.label(), "CR");
    }

    #[test]
    fn read_side_decodes_and_refuses_with_a_reason() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"\xEF\xBB\xBFx\r\ny\r\n").unwrap();
        let cancel = CancelToken::new();
        let side = read_side(&file, MAX_MERGE_BYTES, &cancel).unwrap();
        assert!(side.encoding.bom);
        assert_eq!(side.encoding.encoding, encoding_rs::UTF_8);
        assert_eq!(side.format, fmt(Eol::CrLf, true, false));
        assert_eq!(side.line(1), "y");
        assert_eq!(side.name, "a.txt");
        assert_eq!(side.path.as_deref(), Some(file.as_path()));
        assert_eq!(side.size, 9);
        assert!(!side.lossy);

        assert!(matches!(
            read_side(&file, 8, &cancel),
            Err(ReadError::FileTooLarge(_))
        ));
        assert_eq!(
            ReadError::FileTooLarge(256).to_string(),
            "the file is larger than 256 MB."
        );

        let bin = dir.path().join("b.bin");
        std::fs::write(&bin, [b'a', 0, b'b']).unwrap();
        assert!(matches!(
            read_side(&bin, MAX_MERGE_BYTES, &cancel),
            Err(ReadError::Binary)
        ));
        assert!(matches!(
            read_side(&dir.path().join("missing"), MAX_MERGE_BYTES, &cancel),
            Err(ReadError::Io(_))
        ));
        let stopped = CancelToken::new();
        stopped.cancel();
        assert!(matches!(
            read_side(&file, MAX_MERGE_BYTES, &stopped),
            Err(ReadError::Cancelled)
        ));
    }
}
