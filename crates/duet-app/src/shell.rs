//! The path chosen with "Select as left" in Explorer, kept in a file until "Compare to selected
//! left" asks for it.

use std::io;
use std::path::{Path, PathBuf};

use crate::i18n::trf;
use crate::launch::{Launch, classify};

/// Where `--select-left` keeps its path, in the data folder beside `settings.json`.
pub const STORED_LEFT_FILE: &str = "select-left.txt";

/// Keeps `path` (made absolute) for the next `--compare-to`. Creates `data_dir` when it is missing.
pub fn store_left(data_dir: &Path, path: &Path) -> io::Result<()> {
    let path = std::path::absolute(path)?;
    let text = path.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "the path is not valid Unicode")
    })?;
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(data_dir.join(STORED_LEFT_FILE), text)
}

/// The stored path, if any. Once the file has been read it is deleted (a failed delete is
/// ignored), whatever it held; a file that cannot be read stays, and a missing, empty or garbled
/// one is "nothing stored".
pub fn take_left(data_dir: &Path) -> Option<PathBuf> {
    let file = data_dir.join(STORED_LEFT_FILE);
    let bytes = std::fs::read(&file).ok()?;
    let _ = std::fs::remove_file(&file);
    let text = String::from_utf8(bytes).ok()?;
    let text = text.strip_suffix('\n').unwrap_or(&text);
    let text = text.strip_suffix('\r').unwrap_or(text);
    (!text.is_empty()).then(|| PathBuf::from(text))
}

/// What `duet --compare-to RIGHT` opens. The stored left path is always cleared. Nothing stored,
/// or a stored path that is gone, opens the Start tab with RIGHT as the left side.
pub fn compare_to(right: &Path, data_dir: &Path) -> Launch {
    let right_abs = std::path::absolute(right).unwrap_or_else(|_| right.to_path_buf());
    match take_left(data_dir) {
        // A path that cannot be checked (access denied, an offline share) goes to `classify`,
        // which names the real error.
        Some(left) if left.try_exists().unwrap_or(true) => classify(&left, right),
        Some(left) => Launch::StartWithLeft {
            left: right_abs,
            notice: Some(trf("{}: no longer exists", &[&left.display()])),
        },
        None => Launch::StartWithLeft {
            left: right_abs,
            notice: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn a_stored_left_round_trips_with_thai_spaces_and_symbols() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ไทย & (x) file.txt");
        std::fs::write(&file, "x").unwrap();
        store_left(dir.path(), &file).unwrap();
        assert_eq!(take_left(dir.path()), Some(file));
    }

    #[test]
    fn taking_clears_the_stored_path() {
        let dir = tempfile::tempdir().unwrap();
        store_left(dir.path(), Path::new("a.txt")).unwrap();
        assert!(take_left(dir.path()).is_some());
        assert_eq!(take_left(dir.path()), None);
        assert!(!dir.path().join(STORED_LEFT_FILE).exists());
    }

    #[test]
    fn an_empty_or_missing_file_is_nothing_stored() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(take_left(dir.path()), None, "missing");
        std::fs::write(dir.path().join(STORED_LEFT_FILE), "").unwrap();
        assert_eq!(take_left(dir.path()), None, "empty");
        std::fs::write(dir.path().join(STORED_LEFT_FILE), "\n").unwrap();
        assert_eq!(take_left(dir.path()), None, "only a newline");
        std::fs::write(dir.path().join(STORED_LEFT_FILE), [0xffu8, 0xfe, 0x80]).unwrap();
        assert_eq!(take_left(dir.path()), None, "garbled");
        assert!(
            !dir.path().join(STORED_LEFT_FILE).exists(),
            "a bad file is cleared too"
        );
    }

    #[test]
    fn store_left_creates_the_data_folder() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("new").join("sub");
        store_left(&data, Path::new("a.txt")).unwrap();
        assert!(take_left(&data).is_some());
    }

    #[test]
    fn an_unwritable_data_folder_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let not_a_folder = dir.path().join("file");
        std::fs::write(&not_a_folder, "x").unwrap();
        assert!(store_left(&not_a_folder, Path::new("a.txt")).is_err());
    }
}
