//! The merge session: a result per change, choices, typing, history, navigation and swap
//! (LocalDiff `40-merge.js:512-679`).

use std::collections::VecDeque;
use std::sync::Arc;

use super::MergeText;
use super::diff3::{Region, RegionKind, mirrored};
use super::output::split_typed;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    Left,
    Right,
    LeftThenRight,
    RightThenLeft,
    Base,
    Manual,
}

impl How {
    pub const CHOICES: [How; 5] = [
        How::Left,
        How::Right,
        How::LeftThenRight,
        How::RightThenLeft,
        How::Base,
    ];

    pub fn label(self) -> &'static str {
        match self {
            How::Left => "Use left",
            How::Right => "Use right",
            How::LeftThenRight => "Left + right",
            How::RightThenLeft => "Right + left",
            How::Base => "Use base",
            How::Manual => "Edited",
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            How::Left => "Use the left text (Alt+\u{2190})",
            How::Right => "Use the right text (Alt+\u{2192})",
            How::LeftThenRight => "Use both: the left text, then the right",
            How::RightThenLeft => "Use both: the right text, then the left",
            How::Base => "Keep the base text and drop both sides' changes",
            How::Manual => "",
        }
    }

    /// The same choice after the two sides trade places.
    pub fn flipped(self) -> How {
        match self {
            How::Left => How::Right,
            How::Right => How::Left,
            How::LeftThenRight => How::RightThenLeft,
            How::RightThenLeft => How::LeftThenRight,
            other => other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub how: How,
    pub lines: Vec<String>,
    pub auto: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    First,
    Prev,
    Next,
    Last,
}

/// Most undo steps kept (LocalDiff `HIST_MAX`).
pub const HISTORY_MAX: usize = 400;

/// One chunk's change inside a history step.
struct Item {
    chunk: usize,
    before: Option<Resolution>,
    after: Option<Resolution>,
}

type Step = Vec<Item>;

pub struct MergeSession {
    left: Arc<MergeText>,
    base: Arc<MergeText>,
    right: Arc<MergeText>,
    regions: Vec<Region>,
    trail: bool,
    resolutions: Vec<Option<Resolution>>,
    changes: Vec<usize>,
    conflicts: Vec<usize>,
    current: Option<usize>,
    dirty: bool,
    history: VecDeque<Step>,
    future: Vec<Step>,
    /// The chunk whose typing session is open; its step is the newest in `history`.
    editing: Option<usize>,
}

impl MergeSession {
    pub fn new(
        left: Arc<MergeText>,
        base: Arc<MergeText>,
        right: Arc<MergeText>,
        regions: Vec<Region>,
        trail: bool,
    ) -> MergeSession {
        let mut session = MergeSession {
            left,
            base,
            right,
            regions,
            trail,
            resolutions: Vec::new(),
            changes: Vec::new(),
            conflicts: Vec::new(),
            current: None,
            dirty: false,
            history: VecDeque::new(),
            future: Vec::new(),
            editing: None,
        };
        for (i, region) in session.regions.iter().enumerate() {
            if region.kind != RegionKind::Stable {
                session.changes.push(i);
            }
            if region.kind == RegionKind::Conflict {
                session.conflicts.push(i);
            }
        }
        session.resolutions = (0..session.regions.len())
            .map(|i| session.default_resolution(i))
            .collect();
        session
    }

    pub fn left(&self) -> &Arc<MergeText> {
        &self.left
    }

    pub fn base(&self) -> &Arc<MergeText> {
        &self.base
    }

    pub fn right(&self) -> &Arc<MergeText> {
        &self.right
    }

    pub fn trail(&self) -> bool {
        self.trail
    }

    pub fn regions(&self) -> &[Region] {
        &self.regions
    }

    /// `None` for a stable chunk and for a conflict not yet resolved.
    pub fn resolution(&self, chunk: usize) -> Option<&Resolution> {
        self.resolutions.get(chunk).and_then(Option::as_ref)
    }

    pub fn changes(&self) -> &[usize] {
        &self.changes
    }

    pub fn conflicts(&self) -> &[usize] {
        &self.conflicts
    }

    pub fn unresolved(&self) -> usize {
        self.conflicts
            .iter()
            .filter(|&&c| self.resolutions[c].is_none())
            .count()
    }

    /// Changes that need no decision.
    pub fn automatic(&self) -> usize {
        self.changes.len() - self.conflicts.len()
    }

    pub fn current(&self) -> Option<usize> {
        self.current
    }

    pub fn set_current(&mut self, chunk: usize) {
        if self.is_change(chunk) {
            self.current = Some(chunk);
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_saved(&mut self) {
        self.end_edit();
        self.dirty = false;
    }

    fn is_change(&self, chunk: usize) -> bool {
        self.regions
            .get(chunk)
            .is_some_and(|r| r.kind != RegionKind::Stable)
    }

    fn default_resolution(&self, chunk: usize) -> Option<Resolution> {
        let how = match self.regions[chunk].kind {
            RegionKind::Stable | RegionKind::Conflict => return None,
            RegionKind::Right => How::Right,
            RegionKind::Left | RegionKind::Both => How::Left,
        };
        Some(Resolution {
            how,
            lines: self.choice_lines(chunk, how),
            auto: true,
        })
    }

    /// The lines a choice would put in the chunk (`resolveLines`). `Manual` gives the
    /// lines now there.
    pub fn choice_lines(&self, chunk: usize, how: How) -> Vec<String> {
        let Some(region) = self.regions.get(chunk) else {
            return Vec::new();
        };
        let left = || self.left.lines(region.left.clone());
        let right = || self.right.lines(region.right.clone());
        match how {
            How::Left => left(),
            How::Right => right(),
            How::LeftThenRight => [left(), right()].concat(),
            How::RightThenLeft => [right(), left()].concat(),
            How::Base => self.base.lines(region.base.clone()),
            How::Manual => self
                .resolution(chunk)
                .map(|r| r.lines.clone())
                .unwrap_or_default(),
        }
    }

    fn push_step(&mut self, step: Step) {
        self.history.push_back(step);
        if self.history.len() > HISTORY_MAX {
            self.history.pop_front();
        }
        self.future.clear();
    }

    fn apply(&mut self, chunk: usize, resolution: Option<Resolution>) {
        let before = std::mem::replace(&mut self.resolutions[chunk], resolution.clone());
        self.push_step(vec![Item {
            chunk,
            before,
            after: resolution,
        }]);
        self.dirty = true;
    }

    pub fn resolve(&mut self, chunk: usize, how: How) -> bool {
        if how == How::Manual || !self.is_change(chunk) {
            return false;
        }
        self.end_edit();
        let lines = self.choice_lines(chunk, how);
        self.apply(
            chunk,
            Some(Resolution {
                how,
                lines,
                auto: false,
            }),
        );
        self.current = Some(chunk);
        true
    }

    /// Back to the default: the automatic side, or unresolved for a conflict.
    pub fn reset(&mut self, chunk: usize) -> bool {
        if !self.is_change(chunk) {
            return false;
        }
        self.end_edit();
        let default = self.default_resolution(chunk);
        self.apply(chunk, default);
        true
    }

    /// A typed result. Typing in one chunk is one history step until `end_edit`.
    pub fn edit(&mut self, chunk: usize, text: &str) -> bool {
        if !self.is_change(chunk) {
            return false;
        }
        let after = Some(Resolution {
            how: How::Manual,
            lines: split_typed(text),
            auto: false,
        });
        if self.editing == Some(chunk) {
            self.resolutions[chunk] = after.clone();
            if let Some(item) = self.history.back_mut().and_then(|s| s.first_mut()) {
                item.after = after;
            }
            self.dirty = true;
        } else {
            self.end_edit();
            self.apply(chunk, after);
            self.editing = Some(chunk);
        }
        self.current = Some(chunk);
        true
    }

    pub fn end_edit(&mut self) {
        self.editing = None;
    }

    /// Resolves every open conflict with one side, as one history step. Returns how many.
    pub fn resolve_all(&mut self, how: How) -> usize {
        self.end_edit();
        if how == How::Manual {
            return 0;
        }
        let mut step = Step::new();
        for k in 0..self.conflicts.len() {
            let chunk = self.conflicts[k];
            if self.resolutions[chunk].is_some() {
                continue;
            }
            let after = Some(Resolution {
                how,
                lines: self.choice_lines(chunk, how),
                auto: false,
            });
            let before = std::mem::replace(&mut self.resolutions[chunk], after.clone());
            step.push(Item {
                chunk,
                before,
                after,
            });
        }
        let n = step.len();
        if n > 0 {
            self.push_step(step);
            self.dirty = true;
        }
        n
    }

    pub fn undo(&mut self) -> Option<Vec<usize>> {
        self.end_edit();
        let step = self.history.pop_back()?;
        for item in step.iter().rev() {
            self.resolutions[item.chunk] = item.before.clone();
        }
        let chunks = self.finish_move(&step);
        self.future.push(step);
        Some(chunks)
    }

    pub fn redo(&mut self) -> Option<Vec<usize>> {
        self.end_edit();
        let step = self.future.pop()?;
        for item in &step {
            self.resolutions[item.chunk] = item.after.clone();
        }
        let chunks = self.finish_move(&step);
        self.history.push_back(step);
        Some(chunks)
    }

    fn finish_move(&mut self, step: &Step) -> Vec<usize> {
        self.dirty = true;
        let chunks: Vec<usize> = step.iter().map(|i| i.chunk).collect();
        self.current = chunks.first().copied();
        chunks
    }

    /// Conflicts when there are any, else the changes.
    fn nav_list(&self) -> &[usize] {
        if self.conflicts.is_empty() {
            &self.changes
        } else {
            &self.conflicts
        }
    }

    /// Moves to the first, last, next or previous target, never wrapping.
    pub fn nav(&mut self, nav: Nav) -> Option<usize> {
        let list = self.nav_list();
        let cur = self.current;
        let target = match nav {
            Nav::First => list.first().copied(),
            Nav::Last => list.last().copied(),
            Nav::Next => list.iter().copied().find(|&c| cur.is_none_or(|n| c > n)),
            Nav::Prev => cur.and_then(|n| list.iter().rev().copied().find(|&c| c < n)),
        }?;
        self.current = Some(target);
        Some(target)
    }

    pub fn nav_word(&self) -> &'static str {
        if self.conflicts.is_empty() {
            "Change"
        } else {
            "Conflict"
        }
    }

    pub fn position_text(&self) -> String {
        let list = self.nav_list();
        let place = self
            .current
            .and_then(|c| list.iter().position(|&x| x == c))
            .map_or_else(|| "\u{2013}".to_owned(), |k| (k + 1).to_string());
        format!("{} {} / {}", self.nav_word(), place, list.len())
    }

    pub fn can_go(&self, forward: bool) -> bool {
        let list = self.nav_list();
        match (forward, self.current) {
            (true, None) => !list.is_empty(),
            (true, Some(n)) => list.last().is_some_and(|&l| l > n),
            (false, None) => false,
            (false, Some(n)) => list.first().is_some_and(|&f| f < n),
        }
    }

    /// The same files with left and right traded (LocalDiff `swapLR`). The regions are this
    /// session's mirrored, which is `diff3(right, base, left)`, so every chunk lines up and the
    /// user's choices are carried over, flipped; the current part stays.
    pub fn swapped(&self) -> MergeSession {
        let mut session = MergeSession::new(
            self.right.clone(),
            self.base.clone(),
            self.left.clone(),
            mirrored(&self.regions),
            self.trail,
        );
        for (chunk, old) in self.resolutions.iter().enumerate() {
            let Some(old) = old.as_ref().filter(|r| !r.auto) else {
                continue;
            };
            let how = old.how.flipped();
            let lines = if old.how == How::Manual {
                old.lines.clone()
            } else {
                session.choice_lines(chunk, how)
            };
            session.resolutions[chunk] = Some(Resolution {
                how,
                lines,
                auto: false,
            });
        }
        session.dirty = self.dirty;
        session.current = self.current;
        session
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;
    use crate::cancel::CancelToken;
    use crate::text::merge::diff3::diff3;

    pub fn text(lines: &[&str]) -> Arc<MergeText> {
        Arc::new(MergeText::typed(
            lines.iter().map(|l| format!("{l}\n")).collect(),
        ))
    }

    pub fn session_of(l: &[&str], b: &[&str], r: &[&str]) -> MergeSession {
        let (l, b, r) = (text(l), text(b), text(r));
        let regions = diff3(&l, &b, &r, false, &CancelToken::new()).unwrap();
        MergeSession::new(l, b, r, regions, false)
    }

    const BASE: [&str; 11] = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"];

    /// Chunks: Stable, Left, Stable, Right, Stable, Both, Stable, Conflict, Stable.
    pub fn f1() -> MergeSession {
        session_of(
            &["a", "L", "c", "d", "e", "S", "g", "h", "X", "j", "k"],
            &BASE,
            &["a", "b", "c", "R", "e", "S", "g", "h", "Y", "j", "k"],
        )
    }

    /// Chunks: Stable, Conflict, Stable, Conflict, Stable.
    pub fn f2() -> MergeSession {
        session_of(
            &["a", "X1", "c", "X2", "e"],
            &["a", "b", "c", "d", "e"],
            &["a", "Y1", "c", "Y2", "e"],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::cancel::CancelToken;
    use crate::text::merge::diff3::diff3;

    fn res(how: How, lines: &[&str], auto: bool) -> Resolution {
        Resolution {
            how,
            lines: lines.iter().map(|s| (*s).to_owned()).collect(),
            auto,
        }
    }

    #[test]
    fn new_sessions_resolve_one_sided_and_same_changes_on_their_own() {
        let s = f1();
        assert_eq!(s.changes(), [1, 3, 5, 7]);
        assert_eq!(s.conflicts(), [7]);
        assert_eq!(s.resolution(1), Some(&res(How::Left, &["L"], true)));
        assert_eq!(s.resolution(3), Some(&res(How::Right, &["R"], true)));
        assert_eq!(s.resolution(5), Some(&res(How::Left, &["S"], true)));
        assert_eq!(s.resolution(7), None);
        assert_eq!(s.resolution(0), None);
        assert_eq!(
            (s.unresolved(), s.automatic(), s.current(), s.is_dirty()),
            (1, 3, None, false)
        );
        assert_eq!(
            s.result_lines(),
            [
                "a",
                "L",
                "c",
                "R",
                "e",
                "S",
                "g",
                "h",
                "<<<<<<< left",
                "X",
                "||||||| base",
                "i",
                "=======",
                "Y",
                ">>>>>>> right",
                "j",
                "k"
            ]
        );
        assert_eq!(s.result_line_count(), 17);
    }

    #[test]
    fn choices_fill_a_conflict_and_reset_restores_the_default() {
        let mut s = f1();
        assert!(s.resolve(7, How::Left));
        assert_eq!(s.resolution(7), Some(&res(How::Left, &["X"], false)));
        assert_eq!(s.current(), Some(7));
        assert!(s.is_dirty());
        assert_eq!(s.unresolved(), 0);
        assert_eq!(s.choice_lines(7, How::Right), ["Y"]);
        assert_eq!(s.choice_lines(7, How::LeftThenRight), ["X", "Y"]);
        assert_eq!(s.choice_lines(7, How::RightThenLeft), ["Y", "X"]);
        assert_eq!(s.choice_lines(7, How::Base), ["i"]);
        assert!(!s.resolve(0, How::Left));
        assert!(!s.resolve(7, How::Manual));
        assert!(s.reset(7));
        assert_eq!(s.resolution(7), None);
        assert!(s.resolve(1, How::Right));
        assert!(s.reset(1));
        assert_eq!(s.resolution(1), Some(&res(How::Left, &["L"], true)));
    }

    #[test]
    fn typing_is_one_undo_step_until_the_edit_ends() {
        let mut s = f1();
        assert!(s.edit(7, "one\ntwo"));
        assert_eq!(
            s.resolution(7),
            Some(&res(How::Manual, &["one", "two"], false))
        );
        assert!(s.edit(7, "one\ntwo\n"));
        assert_eq!(
            s.resolution(7),
            Some(&res(How::Manual, &["one", "two", ""], false))
        );
        assert!(s.edit(7, ""));
        assert_eq!(s.resolution(7), Some(&res(How::Manual, &[], false)));
        assert_eq!(s.unresolved(), 0);

        assert_eq!(s.undo(), Some(vec![7]));
        assert_eq!(s.resolution(7), None);
        assert_eq!(s.redo(), Some(vec![7]));
        assert_eq!(s.resolution(7), Some(&res(How::Manual, &[], false)));

        assert!(s.edit(7, "z"));
        assert_eq!(s.undo(), Some(vec![7]));
        assert_eq!(s.resolution(7), Some(&res(How::Manual, &[], false)));
        assert_eq!(s.undo(), Some(vec![7]));
        assert_eq!(s.resolution(7), None);
        assert_eq!(s.undo(), None);

        assert!(s.edit(7, "a\r\nb\rc"));
        assert_eq!(
            s.resolution(7),
            Some(&res(How::Manual, &["a", "b", "c"], false))
        );
        assert!(!s.edit(0, "x"));
    }

    #[test]
    fn bulk_resolve_touches_only_open_conflicts_and_undoes_as_one_step() {
        let mut s = f2();
        s.resolve(3, How::Right);
        assert_eq!(s.resolve_all(How::Left), 1);
        assert_eq!(s.resolution(1), Some(&res(How::Left, &["X1"], false)));
        assert_eq!(s.resolution(3), Some(&res(How::Right, &["Y2"], false)));
        assert_eq!(s.resolve_all(How::Left), 0);
        assert_eq!(s.undo(), Some(vec![1]));
        assert_eq!(s.resolution(1), None);
        assert_eq!(s.resolution(3).map(|r| r.how), Some(How::Right));
        assert_eq!(s.undo(), Some(vec![3]));
        assert_eq!(s.resolution(3), None);
    }

    #[test]
    fn history_keeps_the_last_400_steps() {
        let mut s = f2();
        for i in 0..401 {
            s.resolve(1, if i % 2 == 0 { How::Left } else { How::Right });
        }
        for _ in 0..HISTORY_MAX {
            assert!(s.undo().is_some());
        }
        assert_eq!(s.undo(), None);
    }

    #[test]
    fn navigation_walks_conflicts_or_else_changes_without_wrapping() {
        let mut s = f2();
        assert_eq!(s.position_text(), "Conflict \u{2013} / 2");
        assert_eq!(s.nav(Nav::Next), Some(1));
        assert_eq!(s.position_text(), "Conflict 1 / 2");
        assert_eq!(s.nav(Nav::Next), Some(3));
        assert_eq!(s.nav(Nav::Next), None);
        assert_eq!(s.current(), Some(3));
        assert_eq!(s.nav(Nav::Prev), Some(1));
        assert_eq!(s.nav(Nav::Prev), None);
        assert_eq!(s.nav(Nav::Last), Some(3));
        assert_eq!(s.nav(Nav::First), Some(1));
        assert!(!s.can_go(false));
        assert!(s.can_go(true));
        s.set_current(2);
        assert_eq!(s.current(), Some(1));

        let mut c = session_of(&["a", "X", "c"], &["a", "b", "c"], &["a", "b", "c"]);
        assert_eq!(c.nav_word(), "Change");
        assert_eq!(c.position_text(), "Change \u{2013} / 1");
        assert_eq!(c.nav(Nav::Next), Some(1));

        let mut e = session_of(&["a"], &["a"], &["a"]);
        assert_eq!(e.nav(Nav::Next), None);
        assert_eq!(e.position_text(), "Change \u{2013} / 0");
    }

    #[test]
    fn swapping_flips_the_choices_over_the_mirrored_regions() {
        let mut s = f2();
        s.resolve(1, How::LeftThenRight);
        assert_eq!(s.resolution(1).unwrap().lines, ["X1", "Y1"]);
        s.edit(3, "mine");
        s.end_edit();
        let (l, b, r) = (s.left().clone(), s.base().clone(), s.right().clone());
        let mut n = s.swapped();
        assert_eq!(
            n.regions(),
            diff3(&r, &b, &l, false, &CancelToken::new())
                .unwrap()
                .as_slice(),
            "the regions of the merge with the sides traded"
        );
        assert_eq!(n.left().text(), r.text(), "the right is the new left");
        assert_eq!(
            n.resolution(1),
            Some(&res(How::RightThenLeft, &["X1", "Y1"], false))
        );
        assert_eq!(n.resolution(3), Some(&res(How::Manual, &["mine"], false)));
        assert!(n.is_dirty());
        assert_eq!(n.current(), Some(3));
        assert_eq!(n.undo(), None);
    }

    #[test]
    fn how_has_labels_tips_and_flips() {
        assert_eq!(How::CHOICES.len(), 5);
        assert_eq!(How::Left.label(), "Use left");
        assert_eq!(How::Manual.label(), "Edited");
        assert_eq!(How::Left.tip(), "Use the left text (Alt+\u{2190})");
        assert_eq!(How::Manual.tip(), "");
        assert_eq!(How::LeftThenRight.flipped(), How::RightThenLeft);
        assert_eq!(How::Base.flipped(), How::Base);
        assert_eq!(How::Manual.flipped(), How::Manual);
    }
}
