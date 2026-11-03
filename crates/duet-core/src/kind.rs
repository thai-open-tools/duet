//! Which kind of comparison a pair of paths calls for.

use std::io::Read;
use std::path::Path;

use crate::picture::is_picture_name;
use crate::text::encoding::{BINARY_SNIFF_LEN, is_binary};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompareKind {
    Text,
    Folder,
    Hex,
    Table,
    Picture,
    Merge,
}

impl CompareKind {
    pub fn label(self) -> &'static str {
        match self {
            CompareKind::Text => "Text",
            CompareKind::Folder => "Folders",
            CompareKind::Hex => "Hex",
            CompareKind::Table => "Table",
            CompareKind::Picture => "Picture",
            CompareKind::Merge => "Merge",
        }
    }
}

/// Never returns `Table` or `Merge`: a table compare is always asked for (`--table`, the Start card, a menu).
/// Folders give `Folder`; two files that both have picture extensions give `Picture` without
/// being read (R2); other file pairs give `Hex` when either starts (first 8 KiB) with a NUL,
/// else `Text`. Size alone never routes to hex.
pub fn detect(left: &Path, right: &Path) -> Result<CompareKind, String> {
    let is_dir = |p: &Path| {
        std::fs::metadata(p)
            .map(|m| m.is_dir())
            .map_err(|err| format!("{}: {err}", p.display()))
    };
    match (is_dir(left)?, is_dir(right)?) {
        (true, true) => Ok(CompareKind::Folder),
        (false, false) => {
            if is_picture_name(left) && is_picture_name(right) {
                Ok(CompareKind::Picture)
            } else if sniff_binary(left)? || sniff_binary(right)? {
                Ok(CompareKind::Hex)
            } else {
                Ok(CompareKind::Text)
            }
        }
        _ => Err("Cannot compare a file with a folder.".into()),
    }
}

fn sniff_binary(path: &Path) -> Result<bool, String> {
    let read = || -> std::io::Result<Vec<u8>> {
        let mut head = Vec::with_capacity(BINARY_SNIFF_LEN);
        std::fs::File::open(path)?
            .take(BINARY_SNIFF_LEN as u64)
            .read_to_end(&mut head)?;
        Ok(head)
    };
    read()
        .map(|head| is_binary(&head))
        .map_err(|err| format!("{}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_routes_to_hex_when_either_side_is_binary() {
        let dir = tempfile::tempdir().unwrap();
        let (t, b) = (dir.path().join("a.txt"), dir.path().join("b.bin"));
        std::fs::write(&t, "plain\n").unwrap();
        std::fs::write(&b, b"MZ\0\0\x90").unwrap();
        assert_eq!(detect(&t, &t), Ok(CompareKind::Text));
        assert_eq!(detect(&t, &b), Ok(CompareKind::Hex));
        assert_eq!(detect(&b, &t), Ok(CompareKind::Hex));
        assert_eq!(detect(dir.path(), dir.path()), Ok(CompareKind::Folder));
        assert!(detect(&t, dir.path()).unwrap_err().contains("folder"));
        assert!(detect(&t, &dir.path().join("nope")).is_err());
    }

    #[test]
    fn detect_routes_two_pictures_to_picture() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.PNG"), dir.path().join("b.jpg"));
        std::fs::write(&a, b"\x89PNG\0\0").unwrap();
        std::fs::write(&b, b"\xff\xd8\xff\0").unwrap();
        // The extensions decide, before the NUL sniff.
        assert_eq!(detect(&a, &b), Ok(CompareKind::Picture));
        let t = dir.path().join("t.txt");
        std::fs::write(&t, "plain\n").unwrap();
        assert_eq!(
            detect(&a, &t),
            Ok(CompareKind::Hex),
            "one picture is not enough"
        );
        let (x, y) = (dir.path().join("x.svg"), dir.path().join("y.SVG"));
        std::fs::write(&x, "<svg/>").unwrap();
        std::fs::write(&y, "<svg/>").unwrap();
        assert_eq!(detect(&x, &y), Ok(CompareKind::Picture));
        assert_eq!(detect(dir.path(), dir.path()), Ok(CompareKind::Folder));
        assert!(detect(&a, dir.path()).unwrap_err().contains("folder"));
        assert!(detect(&a, &dir.path().join("nope.png")).is_err());
    }

    #[test]
    fn a_big_text_file_stays_text() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.log");
        std::fs::write(&p, "x".repeat(300 * 1024)).unwrap(); // size alone never routes to hex (R2)
        assert_eq!(detect(&p, &p), Ok(CompareKind::Text));
    }

    #[test]
    fn labels_name_each_kind() {
        assert_eq!(CompareKind::Text.label(), "Text");
        assert_eq!(CompareKind::Folder.label(), "Folders");
        assert_eq!(CompareKind::Hex.label(), "Hex");
        assert_eq!(CompareKind::Table.label(), "Table");
        assert_eq!(CompareKind::Picture.label(), "Picture");
        assert_eq!(CompareKind::Merge.label(), "Merge");
    }
}
