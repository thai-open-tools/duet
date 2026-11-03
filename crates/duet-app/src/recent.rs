//! The recent compares list: newest first, at most 40 entries, one per line as
//! `F|T|H|C|P <tab> left <tab> right` (Windows paths cannot contain tabs), kept in
//! `%LOCALAPPDATA%\duet\recent.txt`. A merge is `M <tab> left <tab> right <tab> base`.

use std::io;
use std::path::{Path, PathBuf};

pub const MAX_RECENT: usize = 40;

/// What a recent pair opens as: tags `F` (folders), `T` (text files), `H` (hex), `C` (table),
/// `P` (picture) and `M` (3-way merge).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecentKind {
    Folders,
    Files,
    Hex,
    Table,
    Picture,
    Merge,
}

impl RecentKind {
    fn tag(self) -> &'static str {
        match self {
            RecentKind::Folders => "F",
            RecentKind::Files => "T",
            RecentKind::Hex => "H",
            RecentKind::Table => "C",
            RecentKind::Picture => "P",
            RecentKind::Merge => "M",
        }
    }

    fn from_tag(tag: &str) -> Option<RecentKind> {
        match tag {
            "F" => Some(RecentKind::Folders),
            "T" => Some(RecentKind::Files),
            "H" => Some(RecentKind::Hex),
            "C" => Some(RecentKind::Table),
            "P" => Some(RecentKind::Picture),
            "M" => Some(RecentKind::Merge),
            _ => None,
        }
    }

    /// The word the Start screen shows.
    pub fn label(self) -> &'static str {
        match self {
            RecentKind::Folders => "Folders",
            RecentKind::Files => "Files",
            RecentKind::Hex => "Hex",
            RecentKind::Table => "Table",
            RecentKind::Picture => "Picture",
            RecentKind::Merge => "Merge",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recent {
    pub kind: RecentKind,
    pub left: PathBuf,
    pub right: PathBuf,
    /// The merge base; None for every other kind.
    pub base: Option<PathBuf>,
}

/// A recent entry as the Start tab's one-row line shows it (U13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryLine {
    /// "left.txt ↔ right.txt"; a merge adds " · base base.txt".
    pub names: String,
    /// The folder every side lives in, else each side's folder once, in the order left, right
    /// and base, joined by " · ". Empty when no side has one (a drive root).
    pub place: String,
}

impl Recent {
    /// The paths in the line's order: left, right, then a merge's base.
    fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        [&self.left, &self.right].into_iter().chain(&self.base)
    }

    pub fn line(&self) -> EntryLine {
        // A drive or share root has no name of its own: it shows whole.
        let name = |path: &Path| {
            path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            )
        };
        let mut names = format!("{} ↔ {}", name(&self.left), name(&self.right));
        if let Some(base) = &self.base {
            names.push_str(&format!(" · base {}", name(base)));
        }
        // Windows paths compare without regard to case.
        let mut places: Vec<String> = Vec::new();
        for parent in self.paths().filter_map(|path| path.parent()) {
            let parent = parent.display().to_string();
            if !parent.is_empty() && !places.iter().any(|p| p.eq_ignore_ascii_case(&parent)) {
                places.push(parent);
            }
        }
        EntryLine {
            names,
            place: places.join(" · "),
        }
    }

    /// The full paths, one per line, in the line's order: left, right, then the base.
    pub fn tooltip(&self) -> String {
        self.paths()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Broken lines are skipped; at most 40 entries are kept.
pub fn parse(text: &str) -> Vec<Recent> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let kind = RecentKind::from_tag(parts.next()?)?;
            let (left, right) = (parts.next()?, parts.next()?);
            // Only a merge names a third path, and it must.
            let base = (kind == RecentKind::Merge).then(|| parts.next()).flatten();
            if parts.next().is_some()
                || left.is_empty()
                || right.is_empty()
                || (kind == RecentKind::Merge && base.is_none_or(str::is_empty))
            {
                return None;
            }
            Some(Recent {
                kind,
                left: left.into(),
                right: right.into(),
                base: base.map(PathBuf::from),
            })
        })
        .take(MAX_RECENT)
        .collect()
}

pub fn render(list: &[Recent]) -> String {
    list.iter()
        .map(|r| {
            let base = r
                .base
                .as_ref()
                .map_or_else(String::new, |base| format!("\t{}", base.display()));
            format!(
                "{}\t{}\t{}{base}\n",
                r.kind.tag(),
                r.left.display(),
                r.right.display()
            )
        })
        .collect()
}

/// Windows paths compare without regard to case.
fn same_pair(a: &Recent, b: &Recent) -> bool {
    let eq = |x: &Path, y: &Path| {
        x.to_string_lossy()
            .eq_ignore_ascii_case(&y.to_string_lossy())
    };
    let same_base = match (&a.base, &b.base) {
        (Some(x), Some(y)) => eq(x, y),
        (None, None) => true,
        _ => false,
    };
    a.kind == b.kind && eq(&a.left, &b.left) && eq(&a.right, &b.right) && same_base
}

/// Puts `entry` first, drops an older copy of the same pair, keeps at most 40.
pub fn push(list: &mut Vec<Recent>, entry: Recent) {
    list.retain(|r| !same_pair(r, &entry));
    list.insert(0, entry);
    list.truncate(MAX_RECENT);
}

/// A missing or unreadable file is an empty list; bytes that are not UTF-8 (a file saved as
/// ANSI, say) spoil only their own line.
pub fn load(path: &Path) -> Vec<Recent> {
    std::fs::read(path)
        .map(|bytes| parse(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_default()
}

/// Writes a temp file in the same folder, then renames it over the list.
pub fn save(path: &Path, list: &[Recent]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("txt.tmp");
    std::fs::write(&temp, render(list))?;
    std::fs::rename(&temp, path)
}

/// Drops one pair (compared as `push` compares them) and saves the rest.
pub fn remove(path: &Path, entry: &Recent) -> io::Result<()> {
    let mut list = load(path);
    list.retain(|r| !same_pair(r, entry));
    save(path, &list)
}

pub fn clear(path: &Path) -> io::Result<()> {
    save(path, &[])
}

pub fn record(path: &Path, entry: Recent) -> io::Result<()> {
    let mut list = load(path);
    push(&mut list, entry);
    save(path, &list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(left: &str, right: &str) -> Recent {
        Recent {
            kind: RecentKind::Files,
            left: left.into(),
            right: right.into(),
            base: None,
        }
    }

    #[test]
    fn broken_lines_are_skipped_and_the_list_is_capped() {
        let text = "F\tC:\\a\tD:\\a\ngarbage\nT\tonly-one-path\nX\tC:\\x\tC:\\y\nT\tC:\\a.txt\tC:\\b.txt\n";
        let list = parse(text);
        assert_eq!(list.len(), 2);
        assert_eq!(
            (list[0].kind, list[1].kind),
            (RecentKind::Folders, RecentKind::Files)
        );
        let many: String = (0..60).map(|i| format!("T\tC:\\{i}\tC:\\x\n")).collect();
        assert_eq!(parse(&many).len(), MAX_RECENT);
    }

    #[test]
    fn hex_entries_round_trip_and_old_tags_still_parse() {
        let text = "H\tC:\\a.bin\tC:\\b.bin\nF\tC:\\x\tC:\\y\nT\tC:\\a.txt\tC:\\b.txt\n";
        let list = parse(text);
        let kinds: Vec<RecentKind> = list.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            [RecentKind::Hex, RecentKind::Folders, RecentKind::Files]
        );
        assert_eq!(render(&list), text);
        let mut list = list;
        let same_paths = Recent {
            kind: RecentKind::Files,
            left: "C:\\a.bin".into(),
            right: "C:\\b.bin".into(),
            base: None,
        };
        push(&mut list, same_paths);
        assert_eq!(
            list.len(),
            4,
            "the same paths as another kind are another entry"
        );
    }

    #[test]
    fn table_entries_round_trip_and_old_tags_still_parse() {
        let text = "C\tC:\\a.csv\tC:\\b.csv\nH\tC:\\a.bin\tC:\\b.bin\nT\tC:\\a.txt\tC:\\b.txt\n";
        let list = parse(text);
        let kinds: Vec<RecentKind> = list.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            [RecentKind::Table, RecentKind::Hex, RecentKind::Files]
        );
        assert_eq!(render(&list), text);
        assert_eq!(RecentKind::Table.label(), "Table");
    }

    #[test]
    fn picture_entries_round_trip_and_old_tags_still_parse() {
        let text = "P\tC:\\a.png\tC:\\b.png\nC\tC:\\a.csv\tC:\\b.csv\n";
        let list = parse(text);
        let kinds: Vec<RecentKind> = list.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, [RecentKind::Picture, RecentKind::Table]);
        assert_eq!(render(&list), text);
        assert_eq!(RecentKind::Picture.label(), "Picture");
    }

    #[test]
    fn merge_entries_keep_three_paths_and_old_tags_still_parse() {
        let text = "M\tC:\\l.txt\tC:\\r.txt\tC:\\b.txt\nP\tC:\\a.png\tC:\\b.png\n";
        let list = parse(text);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].kind, RecentKind::Merge);
        assert_eq!(list[0].base, Some(PathBuf::from("C:\\b.txt")));
        assert_eq!(
            (list[1].kind, list[1].base.clone()),
            (RecentKind::Picture, None)
        );
        assert_eq!(render(&list), text);
        assert!(
            parse("M\tC:\\l\tC:\\r\n").is_empty(),
            "a merge entry without a base is skipped"
        );
        assert!(
            parse("T\ta\tb\tc\n").is_empty(),
            "a base on any other kind is skipped"
        );
        assert_eq!(RecentKind::Merge.label(), "Merge");

        let merge = |base: &str| Recent {
            kind: RecentKind::Merge,
            left: "C:\\l.txt".into(),
            right: "C:\\r.txt".into(),
            base: Some(base.into()),
        };
        let mut list = Vec::new();
        push(&mut list, merge("C:\\b.txt"));
        push(&mut list, merge("c:\\B.txt"));
        assert_eq!(list.len(), 1, "the same triple twice is one entry");
        push(&mut list, merge("C:\\other.txt"));
        assert_eq!(list.len(), 2, "another base is another entry");
    }

    #[test]
    fn pushing_moves_a_pair_to_the_top_once() {
        let mut list = Vec::new();
        push(&mut list, files("C:\\A.txt", "C:\\b.txt"));
        push(
            &mut list,
            Recent {
                kind: RecentKind::Folders,
                left: "C:\\x".into(),
                right: "C:\\y".into(),
                base: None,
            },
        );
        push(&mut list, files("c:\\a.txt", "C:\\B.txt"));
        assert_eq!(list.len(), 2, "the same pair in another case is one entry");
        assert_eq!(list[0], files("c:\\a.txt", "C:\\B.txt"));
    }

    #[test]
    fn removing_drops_one_pair_and_clear_empties_the_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recent.txt");
        record(&path, files("C:\\1", "C:\\2")).unwrap();
        record(&path, files("C:\\3", "C:\\4")).unwrap();
        remove(&path, &files("c:\\1", "C:\\2")).unwrap();
        assert_eq!(
            load(&path),
            [files("C:\\3", "C:\\4")],
            "the same pair in another case goes"
        );
        clear(&path).unwrap();
        assert!(load(&path).is_empty());
    }

    #[test]
    fn a_line_that_is_not_utf8_loses_only_itself() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recent.txt");
        let mut bytes = b"T\tC:\\1\tC:\\2\n".to_vec();
        bytes.extend_from_slice(b"T\tC:\\\xff\tC:\\x\n");
        bytes.extend_from_slice(b"F\tC:\\3\tC:\\4\n");
        std::fs::write(&path, bytes).unwrap();
        let list = load(&path);
        assert_eq!(list.first(), Some(&files("C:\\1", "C:\\2")));
        assert_eq!(
            list.len(),
            3,
            "the broken path is kept as lossy text; nothing else is lost"
        );
        assert_eq!(list[2].kind, RecentKind::Folders);
    }

    fn line(kind: RecentKind, left: &str, right: &str, base: Option<&str>) -> (String, String) {
        let line = Recent {
            kind,
            left: left.into(),
            right: right.into(),
            base: base.map(PathBuf::from),
        }
        .line();
        (line.names, line.place)
    }

    fn pair(names: &str, place: &str) -> (String, String) {
        (names.to_owned(), place.to_owned())
    }

    #[test]
    fn an_entry_line_names_the_pair_and_the_folder_it_lives_in() {
        use RecentKind::*;
        assert_eq!(
            line(Files, r"C:\work\a\left.txt", r"C:\work\a\right.txt", None),
            pair("left.txt ↔ right.txt", r"C:\work\a"),
            "the same parent"
        );
        assert_eq!(
            line(Files, r"C:\Work\a.txt", r"c:\work\b.txt", None),
            pair("a.txt ↔ b.txt", r"C:\Work"),
            "the same parent in another case, as Windows sees it"
        );
        assert_eq!(
            line(Hex, r"C:\one\x.bin", r"D:\two\x.bin", None),
            pair("x.bin ↔ x.bin", r"C:\one · D:\two"),
            "different parents"
        );
        assert_eq!(
            line(Files, r"C:\a.txt", r"C:\b.txt", None),
            pair("a.txt ↔ b.txt", r"C:\"),
            "a drive root"
        );
        assert_eq!(
            line(Folders, r"C:\", r"D:\", None),
            pair(r"C:\ ↔ D:\", ""),
            "two drive roots have no parent"
        );
        assert_eq!(
            line(
                Table,
                r"\\srv\share\dir\a.csv",
                r"\\srv\share\dir\b.csv",
                None
            ),
            pair("a.csv ↔ b.csv", r"\\srv\share\dir"),
            "UNC"
        );
        assert_eq!(
            line(Picture, r"\\srv\share\a.png", r"C:\pics\a.png", None),
            pair("a.png ↔ a.png", r"\\srv\share\ · C:\pics"),
            "a share's root"
        );
        assert_eq!(
            line(Folders, r"C:\proj\v1\", r"C:\proj\v2", None),
            pair("v1 ↔ v2", r"C:\proj"),
            "folders, one written with a trailing separator"
        );
        assert_eq!(
            line(
                Merge,
                r"C:\m\left.txt",
                r"C:\m\right.txt",
                Some(r"C:\m\base.txt")
            ),
            pair("left.txt ↔ right.txt · base base.txt", r"C:\m"),
            "a merge"
        );
        assert_eq!(
            line(
                Merge,
                r"C:\m\left.txt",
                r"C:\m\right.txt",
                Some(r"D:\orig\base.txt")
            ),
            pair("left.txt ↔ right.txt · base base.txt", r"C:\m · D:\orig"),
            "a merge whose base lives elsewhere"
        );
    }

    #[test]
    fn an_entry_tooltip_holds_the_full_paths_one_per_line() {
        assert_eq!(
            files(r"C:\a\left.txt", r"D:\b\right.txt").tooltip(),
            "C:\\a\\left.txt\nD:\\b\\right.txt"
        );
        let merge = Recent {
            kind: RecentKind::Merge,
            left: r"C:\m\l.txt".into(),
            right: r"C:\m\r.txt".into(),
            base: Some(r"C:\m\b.txt".into()),
        };
        assert_eq!(merge.tooltip(), "C:\\m\\l.txt\nC:\\m\\r.txt\nC:\\m\\b.txt");
    }

    #[test]
    fn save_and_load_round_trip_into_a_missing_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("recent.txt");
        assert!(load(&path).is_empty(), "a missing file is an empty list");
        record(&path, files("C:\\1", "C:\\2")).unwrap();
        record(&path, files("C:\\3", "C:\\4")).unwrap();
        assert_eq!(
            load(&path),
            [files("C:\\3", "C:\\4"), files("C:\\1", "C:\\2")]
        );
    }
}
