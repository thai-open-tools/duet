//! Open tabs and which one is active; reopening a pair returns to its tab.

use std::path::{Path, PathBuf};

use crate::format::format_size;

/// A tab's badge: "ok" for an identical pair, otherwise how many differences.
pub fn badge_text(count: usize) -> String {
    if count == 0 {
        "ok".to_owned()
    } else {
        format_size(count as u64)
    }
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
                return "untitled".to_owned();
            }
            path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        };
        match self {
            TabKey::Start(_) => "Start".into(),
            TabKey::Help => "Help".into(),
            TabKey::Folders(l, r)
            | TabKey::Files(l, r)
            | TabKey::Hex(l, r)
            | TabKey::Table(l, r)
            | TabKey::Picture(l, r) => {
                format!("{} ↔ {}", name(l), name(r))
            }
            TabKey::Merge(l, b, r, _) => {
                if [l, b, r].iter().all(|p| p.as_os_str().is_empty()) {
                    return "3-way merge".into();
                }
                let side = |path: &PathBuf| {
                    if path.as_os_str().is_empty() {
                        "?".to_owned()
                    } else {
                        name(path)
                    }
                };
                format!("Merge: {} ↔ {}", side(l), side(r))
            }
        }
    }
}

pub struct TabSet<T> {
    tabs: Vec<(TabKey, T)>,
    active: Option<usize>,
}

impl<T> Default for TabSet<T> {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active: None,
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
        index
    }

    pub fn activate(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = Some(index);
        }
    }

    /// Removes a tab; the active tab stays the same, or becomes the neighbour that took the
    /// closed tab's place.
    pub fn close(&mut self, index: usize) -> Option<T> {
        if index >= self.tabs.len() {
            return None;
        }
        let (_, item) = self.tabs.remove(index);
        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(active) if active > index => Some(active - 1),
            Some(active) => Some(active.min(self.tabs.len() - 1)),
            None => None,
        };
        Some(item)
    }

    /// Gives tab `index` a new key (after a swap); its item and position stay.
    pub fn rekey(&mut self, index: usize, key: TabKey) {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.0 = key;
        }
    }

    pub fn next(&mut self) {
        if !self.tabs.is_empty() {
            self.active = Some(self.active.map_or(0, |i| (i + 1) % self.tabs.len()));
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
