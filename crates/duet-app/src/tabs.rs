//! Open tabs and which one is active; reopening a pair returns to its tab.

use std::path::{Path, PathBuf};

use crate::format::format_size;
use crate::i18n::{tr, trf};

/// A tab's badge: "ok" for an identical pair, otherwise how many differences.
pub fn badge_text(count: usize) -> String {
    if count == 0 {
        tr("ok").to_owned()
    } else {
        format_size(count as u64)
    }
}

/// Whether `badge` says the pair is identical, which draws it green: the text `badge_text(0)`
/// gives in the language in force.
pub fn is_ok_badge(badge: &str) -> bool {
    badge == badge_text(0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabKey {
    Start(u32),
    Folders(PathBuf, PathBuf),
    Files(PathBuf, PathBuf),
    Hex(PathBuf, PathBuf),
    Table(PathBuf, PathBuf),
    Picture(PathBuf, PathBuf),
    /// (left, base, right, draft); an empty path is a slot with no file. A set with such a
    /// slot has a draft number of its own, so two tabs of typed or edited texts never share a
    /// key (C1); a set of three files has draft 0.
    Merge(PathBuf, PathBuf, PathBuf, u32),
    /// The one Help tab of a window (H1).
    Help,
}

impl TabKey {
    /// The key of a merge set; `draft` counts only when a slot has no file.
    pub fn merge(
        left: Option<&Path>,
        base: Option<&Path>,
        right: Option<&Path>,
        draft: u32,
    ) -> TabKey {
        let files = left.is_some() && base.is_some() && right.is_some();
        let own = |p: Option<&Path>| p.map(Path::to_path_buf).unwrap_or_default();
        TabKey::Merge(
            own(left),
            own(base),
            own(right),
            if files { 0 } else { draft },
        )
    }

    pub fn title(&self) -> String {
        let name = |path: &PathBuf| {
            if path.as_os_str().is_empty() {
                return tr("untitled").to_owned();
            }
            path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        };
        match self {
            TabKey::Start(_) => tr("Start").into(),
            TabKey::Help => tr("Help").into(),
            TabKey::Folders(l, r)
            | TabKey::Files(l, r)
            | TabKey::Hex(l, r)
            | TabKey::Table(l, r)
            | TabKey::Picture(l, r) => {
                format!("{} ↔ {}", name(l), name(r))
            }
            TabKey::Merge(l, b, r, _) => {
                if [l, b, r].iter().all(|p| p.as_os_str().is_empty()) {
                    return tr("3-way merge").into();
                }
                let side = |path: &PathBuf| {
                    if path.as_os_str().is_empty() {
                        "?".to_owned()
                    } else {
                        name(path)
                    }
                };
                trf("Merge: {} ↔ {}", &[&side(l), &side(r)])
            }
        }
    }
}

/// What a tab compares: the header's bar goes to the latest tab of each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Start,
    Text,
    Folders,
    Merge,
    Hex,
    Picture,
    Table,
}

impl Mode {
    /// In the order of the bar.
    pub const ALL: [Mode; 7] = [
        Mode::Start,
        Mode::Text,
        Mode::Folders,
        Mode::Merge,
        Mode::Hex,
        Mode::Picture,
        Mode::Table,
    ];

    /// The mode of a tab; the Help tab has none.
    pub fn of(key: &TabKey) -> Option<Mode> {
        match key {
            TabKey::Start(_) => Some(Mode::Start),
            TabKey::Files(..) => Some(Mode::Text),
            TabKey::Folders(..) => Some(Mode::Folders),
            TabKey::Merge(..) => Some(Mode::Merge),
            TabKey::Hex(..) => Some(Mode::Hex),
            TabKey::Picture(..) => Some(Mode::Picture),
            TabKey::Table(..) => Some(Mode::Table),
            TabKey::Help => None,
        }
    }

    pub fn label(self) -> &'static str {
        tr(match self {
            Mode::Start => "Start",
            Mode::Text => "Text",
            Mode::Folders => "Folders",
            Mode::Merge => "3-way merge",
            Mode::Hex => "Hex",
            Mode::Picture => "Picture",
            Mode::Table => "Table",
        })
    }
}

pub struct TabSet<T> {
    tabs: Vec<(TabKey, T)>,
    active: Option<usize>,
    /// The open tabs by when they were last active, latest last.
    used: Vec<TabKey>,
}

impl<T> Default for TabSet<T> {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active: None,
            used: Vec::new(),
        }
    }
}

impl<T> TabSet<T> {
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn active_index(&self) -> Option<usize> {
        self.active
    }

    pub fn active(&self) -> Option<&T> {
        self.active.map(|i| &self.tabs[i].1)
    }

    /// The mode of the active tab; None with no tab or on the Help tab.
    pub fn active_mode(&self) -> Option<Mode> {
        self.active.and_then(|i| Mode::of(&self.tabs[i].0))
    }

    /// The tab of `mode` that was active last.
    pub fn latest(&self, mode: Mode) -> Option<usize> {
        let key = self.used.iter().rev().find(|k| Mode::of(k) == Some(mode))?;
        self.find(key)
    }

    /// Notes that the active tab is now the one used last.
    fn note_used(&mut self) {
        if let Some(index) = self.active {
            let key = self.tabs[index].0.clone();
            self.used.retain(|k| *k != key);
            self.used.push(key);
        }
    }

    pub fn find(&self, key: &TabKey) -> Option<usize> {
        self.tabs.iter().position(|(k, _)| k == key)
    }

    /// Activates the tab for `key`, creating it with `make` only when it is not open yet.
    pub fn open(&mut self, key: TabKey, make: impl FnOnce() -> T) -> usize {
        let index = self.find(&key).unwrap_or_else(|| {
            self.tabs.push((key, make()));
            self.tabs.len() - 1
        });
        self.active = Some(index);
        self.note_used();
        index
    }

    pub fn activate(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = Some(index);
            self.note_used();
        }
    }

    /// Removes a tab; the active tab stays the same, or becomes the neighbour that took the
    /// closed tab's place.
    pub fn close(&mut self, index: usize) -> Option<T> {
        if index >= self.tabs.len() {
            return None;
        }
        let (key, item) = self.tabs.remove(index);
        self.used.retain(|k| *k != key);
        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(active) if active > index => Some(active - 1),
            Some(active) => Some(active.min(self.tabs.len() - 1)),
            None => None,
        };
        self.note_used();
        Some(item)
    }

    /// Gives tab `index` a new key (after a swap); its item and position stay.
    pub fn rekey(&mut self, index: usize, key: TabKey) {
        if let Some(tab) = self.tabs.get_mut(index) {
            if let Some(used) = self.used.iter_mut().find(|k| **k == tab.0) {
                *used = key.clone();
            }
            tab.0 = key;
        }
    }

    pub fn next(&mut self) {
        if !self.tabs.is_empty() {
            self.active = Some(self.active.map_or(0, |i| (i + 1) % self.tabs.len()));
            self.note_used();
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, &TabKey, &T)> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, (key, item))| (i, key, item))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(a: &str, b: &str) -> TabKey {
        TabKey::Files(PathBuf::from(a), PathBuf::from(b))
    }

    #[test]
    fn the_latest_tab_of_a_mode_is_the_one_used_last() {
        let mut tabs = TabSet::default();
        tabs.open(files("a", "b"), || 0);
        tabs.open(files("c", "d"), || 1);
        tabs.open(TabKey::Start(0), || 2);
        assert_eq!(tabs.latest(Mode::Text), Some(1));
        tabs.activate(0);
        assert_eq!(tabs.latest(Mode::Text), Some(0));
        assert_eq!(tabs.latest(Mode::Hex), None);
        assert_eq!(tabs.active_mode(), Some(Mode::Text));

        tabs.activate(1);
        tabs.close(1);
        assert_eq!(
            tabs.latest(Mode::Text),
            Some(0),
            "a closed tab is forgotten"
        );
        assert_eq!(tabs.latest(Mode::Start), Some(1), "and the rest renumber");

        tabs.rekey(0, files("x", "y"));
        assert_eq!(tabs.latest(Mode::Text), Some(0), "a swap keeps its place");
        tabs.next();
        assert_eq!(
            tabs.active_mode(),
            Some(Mode::Text),
            "next wraps to the first tab"
        );
    }

    #[test]
    fn each_tab_has_its_mode_and_help_has_none() {
        let path = || PathBuf::from("a");
        assert_eq!(Mode::of(&TabKey::Start(3)), Some(Mode::Start));
        assert_eq!(Mode::of(&TabKey::Files(path(), path())), Some(Mode::Text));
        assert_eq!(
            Mode::of(&TabKey::Folders(path(), path())),
            Some(Mode::Folders)
        );
        assert_eq!(Mode::of(&TabKey::Hex(path(), path())), Some(Mode::Hex));
        assert_eq!(Mode::of(&TabKey::Table(path(), path())), Some(Mode::Table));
        assert_eq!(
            Mode::of(&TabKey::Picture(path(), path())),
            Some(Mode::Picture)
        );
        assert_eq!(
            Mode::of(&TabKey::Merge(path(), path(), path(), 0)),
            Some(Mode::Merge)
        );
        assert_eq!(Mode::of(&TabKey::Help), None);
    }

    #[test]
    fn the_modes_are_named_in_the_chosen_language() {
        let _thai = crate::i18n::scoped(crate::i18n::Lang::Th);
        assert_eq!(
            Mode::ALL.map(Mode::label),
            [
                "เริ่มต้น",
                "ข้อความ",
                "โฟลเดอร์",
                "รวมสามทาง",
                "Hex",
                "รูปภาพ",
                "ตาราง"
            ]
        );
    }

    #[test]
    fn an_empty_path_is_titled_untitled() {
        assert_eq!(files("", "").title(), "untitled ↔ untitled");
        assert_eq!(files("C:/dir/x.txt", "").title(), "x.txt ↔ untitled");
    }

    #[test]
    fn reopening_a_pair_activates_its_tab() {
        let mut tabs = TabSet::default();
        assert_eq!(tabs.open(files("C:/a.txt", "D:/a.txt"), || 1), 0);
        assert_eq!(tabs.open(TabKey::Start(0), || 2), 1);
        let mut made = false;
        assert_eq!(
            tabs.open(files("C:/a.txt", "D:/a.txt"), || {
                made = true;
                3
            }),
            0
        );
        assert!(!made, "an open pair is not created twice");
        assert_eq!(
            (tabs.len(), tabs.active(), tabs.active_index()),
            (2, Some(&1), Some(0))
        );
    }

    #[test]
    fn closing_keeps_a_sensible_active_tab() {
        let mut tabs = TabSet::default();
        for i in 0..3 {
            tabs.open(TabKey::Start(i), || i);
        }
        tabs.activate(1);
        assert_eq!(tabs.close(1), Some(1));
        assert_eq!(
            tabs.active(),
            Some(&2),
            "the right neighbour takes its place"
        );
        tabs.close(1);
        assert_eq!(tabs.active(), Some(&0));
        tabs.close(0);
        assert!(tabs.is_empty() && tabs.active().is_none());
        assert_eq!(tabs.close(5), None);
    }

    #[test]
    fn next_cycles_and_titles_name_both_sides() {
        let mut tabs = TabSet::default();
        tabs.open(TabKey::Start(0), || 'a');
        tabs.open(TabKey::Start(1), || 'b');
        tabs.next();
        assert_eq!(tabs.active(), Some(&'a'));
        assert_eq!(
            files("C:/x/old.rs", "C:/y/new.rs").title(),
            "old.rs ↔ new.rs"
        );
        assert_eq!(TabKey::Start(3).title(), "Start");
        assert_eq!(
            TabKey::Picture("C:/a.png".into(), "C:/b.png".into()).title(),
            "a.png ↔ b.png"
        );
    }

    #[test]
    fn rekeying_renames_a_tab_in_place() {
        let mut tabs = TabSet::default();
        tabs.open(files("C:/a.txt", "C:/b.txt"), || 1);
        tabs.rekey(0, files("C:/b.txt", "C:/a.txt"));
        assert_eq!(tabs.find(&files("C:/b.txt", "C:/a.txt")), Some(0));
        assert_eq!(tabs.find(&files("C:/a.txt", "C:/b.txt")), None);
        assert_eq!(tabs.iter().next().unwrap().1.title(), "b.txt ↔ a.txt");
    }

    #[test]
    fn merge_tab_titles_name_the_outer_sides() {
        fn some(p: &str) -> Option<&Path> {
            Some(Path::new(p))
        }
        assert_eq!(
            TabKey::merge(
                some("C:/x/left.txt"),
                some("C:/x/base.txt"),
                some("C:/y/r.txt"),
                7
            )
            .title(),
            "Merge: left.txt ↔ r.txt"
        );
        assert_eq!(TabKey::merge(None, None, None, 1).title(), "3-way merge");
        assert_eq!(
            TabKey::merge(some("C:/x/l.txt"), None, None, 1).title(),
            "Merge: l.txt ↔ ?"
        );
        assert_eq!(
            TabKey::merge(None, some("C:/b.txt"), some("C:/r.txt"), 1).title(),
            "Merge: ? ↔ r.txt"
        );
        assert_eq!(
            TabKey::merge(some("C:/l"), None, None, 2),
            TabKey::Merge("C:/l".into(), PathBuf::new(), PathBuf::new(), 2),
            "a slot with no file is an empty path, and the set keeps its draft number"
        );
        assert_ne!(
            TabKey::merge(None, None, None, 1),
            TabKey::merge(None, None, None, 2),
            "two sets of no files are two keys (C1)"
        );
        assert_eq!(
            TabKey::merge(some("C:/l"), some("C:/b"), some("C:/r"), 5),
            TabKey::Merge("C:/l".into(), "C:/b".into(), "C:/r".into(), 0),
            "three files are one set, whatever the draft"
        );
    }

    #[test]
    fn badges_say_ok_or_count() {
        assert_eq!(badge_text(0), "ok");
        assert_eq!(badge_text(3), "3");
        assert_eq!(badge_text(12_345), "12,345");
    }
}
