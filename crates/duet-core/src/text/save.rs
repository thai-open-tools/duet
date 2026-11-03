//! Saving edited text: original encoding/BOM/line endings, atomic replace, conflict check.

use std::borrow::Cow;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use memchr::memchr2;

use crate::text::document::FileStamp;
use crate::text::encoding::{EncodeError, TextEncoding, encode};
use crate::text::lines::EolStyle;

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("{path} changed on disk since it was loaded")]
    ChangedOnDisk { path: PathBuf },
    #[error(transparent)]
    Encode(#[from] EncodeError),
    #[error("cannot save {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct SaveRequest<'a> {
    pub path: &'a Path,
    pub text: &'a str,
    pub encoding: TextEncoding,
    /// Line endings to write. `None` and `Mixed` keep the text's own terminators.
    pub eol: EolStyle,
    /// Stamp taken at load time; `None` skips the external-change check (new file).
    pub expected: Option<FileStamp>,
}

pub fn save(req: &SaveRequest<'_>) -> Result<FileStamp, SaveError> {
    let io_err = |source| SaveError::Io {
        path: req.path.to_path_buf(),
        source,
    };
    if let Some(expected) = req.expected {
        match fs::metadata(req.path) {
            Ok(meta) if FileStamp::of(&meta) == expected => {}
            Ok(_) => {
                return Err(SaveError::ChangedOnDisk {
                    path: req.path.to_path_buf(),
                });
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(SaveError::ChangedOnDisk {
                    path: req.path.to_path_buf(),
                });
            }
            Err(e) => return Err(io_err(e)),
        }
    }
    let bytes = encode(&apply_eol(req.text, req.eol), req.encoding)?;
    let target = resolve_target(req.path).map_err(io_err)?;
    let dir = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(io_err)?;
    tmp.write_all(&bytes).map_err(io_err)?;
    tmp.as_file().sync_all().map_err(io_err)?;
    tmp.persist(&target).map_err(|e| io_err(e.error))?;
    let meta = fs::metadata(&target).map_err(io_err)?;
    Ok(FileStamp::of(&meta))
}

/// Writes through a symlink to its target instead of replacing the link with a file.
fn resolve_target(path: &Path) -> io::Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => fs::canonicalize(path),
        _ => Ok(path.to_path_buf()),
    }
}

pub fn apply_eol(text: &str, eol: EolStyle) -> Cow<'_, str> {
    let target = match eol {
        EolStyle::Lf => "\n",
        EolStyle::CrLf => "\r\n",
        EolStyle::Cr => "\r",
        EolStyle::None | EolStyle::Mixed => return Cow::Borrowed(text),
    };
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + text.len() / 32);
    let (mut last, mut pos) = (0, 0);
    while let Some(offset) = memchr2(b'\n', b'\r', &bytes[pos..]) {
        let at = pos + offset;
        out.push_str(&text[last..at]);
        out.push_str(target);
        pos = if bytes[at] == b'\r' && bytes.get(at + 1) == Some(&b'\n') {
            at + 2
        } else {
            at + 1
        };
        last = pos;
    }
    out.push_str(&text[last..]);
    if out == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(out)
    }
}

/// True when both paths name the same file on disk, whatever their spelling (case, 8.3 names,
/// mapped drive or UNC, symlinked folders). Equal paths are the same file even when the file
/// does not exist yet. On Windows, when either path cannot be resolved (a file about to be
/// created), the absolute forms compare ignoring case; elsewhere an unresolvable path counts as
/// different. It touches the disk, so call it off the UI thread.
pub fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        #[cfg(windows)]
        _ => match (std::path::absolute(a), std::path::absolute(b)) {
            (Ok(a), Ok(b)) => {
                a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
            }
            _ => false,
        },
        #[cfg(not(windows))]
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::{UTF_8, UTF_16LE, WINDOWS_874};

    fn stamp(path: &Path) -> FileStamp {
        FileStamp::of(&fs::metadata(path).unwrap())
    }

    fn request<'a>(
        path: &'a Path,
        text: &'a str,
        encoding: TextEncoding,
        eol: EolStyle,
    ) -> SaveRequest<'a> {
        SaveRequest {
            path,
            text,
            encoding,
            eol,
            expected: Some(stamp(path)),
        }
    }

    #[test]
    fn keeps_utf8_bom_and_crlf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, b"old").unwrap();
        let enc = TextEncoding {
            encoding: UTF_8,
            bom: true,
        };
        let new_stamp = save(&request(&path, "ก\nข\n", enc, EolStyle::CrLf)).unwrap();
        let mut expected = vec![0xEF, 0xBB, 0xBF];
        expected.extend_from_slice("ก\r\nข\r\n".as_bytes());
        assert_eq!(fs::read(&path).unwrap(), expected);
        assert_eq!(new_stamp, stamp(&path));
    }

    #[test]
    fn writes_utf16le_and_windows_874() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.txt");
        fs::write(&path, b"old").unwrap();
        let utf16 = TextEncoding {
            encoding: UTF_16LE,
            bom: true,
        };
        save(&request(&path, "ก", utf16, EolStyle::None)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), vec![0xFF, 0xFE, 0x01, 0x0E]);
        let thai = TextEncoding {
            encoding: WINDOWS_874,
            bom: false,
        };
        save(&request(&path, "ก", thai, EolStyle::None)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), vec![0xA1]);
    }

    #[test]
    fn refuses_when_file_changed_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.txt");
        fs::write(&path, b"v1").unwrap();
        let utf8 = TextEncoding {
            encoding: UTF_8,
            bom: false,
        };
        let req = request(&path, "mine", utf8, EolStyle::None);
        fs::write(&path, b"someone else's longer content").unwrap();
        assert!(matches!(save(&req), Err(SaveError::ChangedOnDisk { .. })));
        assert_eq!(fs::read(&path).unwrap(), b"someone else's longer content");
    }

    #[test]
    fn refuses_when_file_was_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.txt");
        fs::write(&path, b"v1").unwrap();
        let utf8 = TextEncoding {
            encoding: UTF_8,
            bom: false,
        };
        let req = request(&path, "mine", utf8, EolStyle::None);
        fs::remove_file(&path).unwrap();
        assert!(matches!(save(&req), Err(SaveError::ChangedOnDisk { .. })));
    }

    #[test]
    fn unrepresentable_text_leaves_file_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("e.txt");
        fs::write(&path, b"original").unwrap();
        let thai = TextEncoding {
            encoding: WINDOWS_874,
            bom: false,
        };
        let req = request(&path, "emoji 😀", thai, EolStyle::None);
        assert!(matches!(save(&req), Err(SaveError::Encode(_))));
        assert_eq!(fs::read(&path).unwrap(), b"original");
    }

    #[test]
    fn apply_eol_rewrites_every_terminator() {
        let text = "a\r\nb\nc\rd";
        assert_eq!(apply_eol(text, EolStyle::Lf), "a\nb\nc\nd");
        assert_eq!(apply_eol(text, EolStyle::CrLf), "a\r\nb\r\nc\r\nd");
        assert!(matches!(apply_eol(text, EolStyle::Mixed), Cow::Borrowed(_)));
        assert!(matches!(apply_eol(text, EolStyle::None), Cow::Borrowed(_)));
        assert!(matches!(apply_eol("a\nb", EolStyle::Lf), Cow::Borrowed(_)));
    }

    #[cfg(windows)]
    #[test]
    #[allow(clippy::permissions_set_readonly_false)]
    fn read_only_target_reports_io_error_and_keeps_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ro.txt");
        fs::write(&path, b"keep").unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&path, perms.clone()).unwrap();
        let utf8 = TextEncoding {
            encoding: UTF_8,
            bom: false,
        };
        let result = save(&request(&path, "new", utf8, EolStyle::None));
        perms.set_readonly(false);
        fs::set_permissions(&path, perms).unwrap();
        assert!(matches!(result, Err(SaveError::Io { .. })), "{result:?}");
        assert_eq!(fs::read(&path).unwrap(), b"keep");
    }

    #[test]
    fn same_file_resolves_spelling_and_treats_unresolvable_as_different() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        fs::write(&a, b"x").unwrap();
        fs::write(dir.path().join("b.txt"), b"y").unwrap();
        assert!(same_file(&a, &dir.path().join(".").join("a.txt")));
        assert!(!same_file(&a, &dir.path().join("b.txt")));
        assert!(!same_file(&a, &dir.path().join("missing.txt")));
        #[cfg(windows)]
        assert!(same_file(&a, &dir.path().join("A.TXT")));
    }

    #[test]
    fn same_file_is_true_for_identical_paths_that_do_not_exist_yet() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("not-yet").join("new.txt");
        assert!(same_file(&p, &p));
        assert!(!same_file(&p, &dir.path().join("other.txt")));
    }

    #[cfg(windows)]
    #[test]
    fn same_file_ignores_case_for_existing_and_unresolvable_paths() {
        // The (Ok, Err) arm (one side resolves, the other does not) is not pinned here: no
        // deterministic fixture makes `canonicalize` fail for a path that is lexically the
        // same as an existing file (Windows resolves ".." lexically, and a no-share lock does
        // not make it fail). The (Err, Err) and (Ok, Ok) arms are what this checks.
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        fs::write(&a, b"x").unwrap();
        // Neither a missing file nor a missing folder resolves; the spelling still decides.
        let upper = dir.path().join("A.TXT");
        let missing = dir.path().join("new").join("File.txt");
        let missing_upper = dir.path().join("NEW").join("FILE.TXT");
        assert!(same_file(&a, &upper));
        assert!(same_file(&upper, &a));
        assert!(same_file(&missing, &missing_upper));
        assert!(!same_file(&a, &missing));
        assert!(!same_file(
            &missing,
            &dir.path().join("NEW").join("OTHER.TXT")
        ));
    }

    #[cfg(windows)]
    #[test]
    fn saving_through_a_symlink_keeps_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        let link = dir.path().join("link.txt");
        fs::write(&target, b"v1").unwrap();
        if std::os::windows::fs::symlink_file(&target, &link).is_err() {
            eprintln!("skipped: creating symlinks needs Developer Mode or admin");
            return;
        }
        let utf8 = TextEncoding {
            encoding: UTF_8,
            bom: false,
        };
        save(&request(&link, "v2", utf8, EolStyle::None)).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&target).unwrap(), b"v2");
    }
}
