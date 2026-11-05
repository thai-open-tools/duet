//! The merge tab's plumbing in the workspace (PF5): opening, re-keying after each merge and the
//! recent entry.

use super::*;

use gpui_kit::EntityId;

use crate::launch::MergePaths;
use crate::merge::view::{MERGED_ELSEWHERE, MergeEvent};

/// A tab that can hold unsaved work and asks before it is closed: the text flow stays as it
/// was, the merge tab asks about its conflict choices (Task 8).
pub(super) enum Guarded {
    Text(Entity<TextView>),
    Merge(Entity<MergeView>),
}

impl Guarded {
    /// The guard of a tab, if it holds unsaved work.
    pub(super) fn of(item: &TabItem, cx: &App) -> Option<Self> {
        match item {
            TabItem::Text(view) if view.read(cx).is_dirty() => Some(Self::Text(view.clone())),
            TabItem::Merge(view) if view.read(cx).is_dirty() => Some(Self::Merge(view.clone())),
            _ => None,
        }
    }

    /// The guard of any text or merge tab, dirty or not (the tab may be clean by now).
    pub(super) fn any(item: &TabItem) -> Option<Self> {
        match item {
            TabItem::Text(view) => Some(Self::Text(view.clone())),
            TabItem::Merge(view) => Some(Self::Merge(view.clone())),
            _ => None,
        }
    }

    pub(super) fn is(&self, item: &TabItem) -> bool {
        match (self, item) {
            (Self::Text(a), TabItem::Text(b)) => a == b,
            (Self::Merge(a), TabItem::Merge(b)) => a == b,
            _ => false,
        }
    }

    /// Asks (or not, when clean); true when the tab may go.
    pub(super) fn confirm(&self, window: &mut Window, cx: &mut App) -> Task<bool> {
        match self {
            Self::Text(view) => view.update(cx, |v, cx| v.confirm_discard(window, cx)),
            Self::Merge(view) => view.update(cx, |v, cx| v.confirm_discard(window, cx)),
        }
    }

    pub(super) fn confirm_async(&self, cx: &mut AsyncWindowContext) -> Option<Task<bool>> {
        match self {
            Self::Text(view) => view
                .update_in(cx, |v, window, cx| v.confirm_discard(window, cx))
                .ok(),
            Self::Merge(view) => view
                .update_in(cx, |v, window, cx| v.confirm_discard(window, cx))
                .ok(),
        }
    }
}

impl Workspace {
    /// Opens the merge tab for this set, or activates the tab that has it (R11). A set of no
    /// files (the Start card) brings back a merge tab nobody has used yet, else opens a new one
    /// with a draft number of its own (C1).
    pub fn open_merge(&mut self, paths: MergePaths, window: &mut Window, cx: &mut Context<Self>) {
        let files = paths.left.is_some() && paths.base.is_some() && paths.right.is_some();
        let open = if files {
            let key = TabKey::merge(
                paths.left.as_deref(),
                paths.base.as_deref(),
                paths.right.as_deref(),
                0,
            );
            self.tabs.find(&key)
        } else if paths == MergePaths::default() {
            self.index_of(|item| matches!(item, TabItem::Merge(v) if v.read(cx).is_untouched()))
        } else {
            None
        };
        match open {
            Some(index) => {
                self.tabs.activate(index);
                if let (Some(output), Some(TabItem::Merge(view))) =
                    (paths.output, self.tabs.active())
                {
                    let view = view.clone();
                    view.update(cx, |v, cx| v.offer_output(output, window, cx));
                }
            }
            None => {
                let key = TabKey::merge(
                    paths.left.as_deref(),
                    paths.base.as_deref(),
                    paths.right.as_deref(),
                    self.new_draft(),
                );
                let view = cx.new(|cx| MergeView::new(paths, window, cx));
                // Merge ▸ in a tab with unsaved choices asks here before it drops them (M-68).
                let (workspace, id) = (cx.entity().downgrade(), view.entity_id());
                view.update(cx, |view, _| {
                    view.set_elsewhere_check(Box::new(move |set: [Option<&Path>; 3], cx: &App| {
                        workspace
                            .upgrade()
                            .is_some_and(|w| w.read(cx).merged_elsewhere(id, set))
                    }))
                });
                self.subscriptions.push(cx.subscribe_in(
                    &view,
                    window,
                    |this, view, event: &MergeEvent, window, cx| {
                        this.on_merge_event(view, event, window, cx)
                    },
                ));
                self.subscriptions
                    .push(cx.observe(&view, |_, _, cx| cx.notify()));
                self.tabs.open(key, || TabItem::Merge(view));
            }
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Whether a merge tab other than the view `id` shows this set of three files.
    fn merged_elsewhere(&self, id: EntityId, [left, base, right]: [Option<&Path>; 3]) -> bool {
        if left.is_none() || base.is_none() || right.is_none() {
            return false;
        }
        let key = TabKey::merge(left, base, right, 0);
        self.tabs.iter().any(|(_, open, item)| {
            *open == key && !matches!(item, TabItem::Merge(view) if view.entity_id() == id)
        })
    }

    /// A draft number no open merge tab has.
    fn new_draft(&self) -> u32 {
        self.tabs
            .iter()
            .filter_map(|(_, key, _)| match key {
                TabKey::Merge(_, _, _, draft) => Some(*draft),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            + 1
    }

    /// The key for the set tab `index` now shows: a set with a slot of no file keeps the
    /// tab's own draft number, or takes a new one, so it never meets another tab's key (C1).
    fn merge_key(&self, index: usize, [left, base, right]: [&Option<PathBuf>; 3]) -> TabKey {
        let own = self
            .tabs
            .iter()
            .nth(index)
            .and_then(|(_, key, _)| match key {
                TabKey::Merge(_, _, _, draft) if *draft != 0 => Some(*draft),
                _ => None,
            });
        TabKey::merge(
            left.as_deref(),
            base.as_deref(),
            right.as_deref(),
            own.unwrap_or_else(|| self.new_draft()),
        )
    }

    fn on_merge_event(
        &mut self,
        view: &Entity<MergeView>,
        event: &MergeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.index_of(|item| matches!(item, TabItem::Merge(v) if v == view))
        else {
            return;
        };
        match event {
            MergeEvent::Merged { left, base, right } => {
                let new = self.merge_key(index, [left, base, right]);
                // Another tab may already show this set of files: this one closes, unless it
                // holds choices.
                if !self.rekey(index, new, window, cx) {
                    view.update(cx, |v, cx| v.show_notice(tr(MERGED_ELSEWHERE), cx));
                }
                if let (Some(left), Some(base), Some(right)) = (left, base, right) {
                    self.remember_entry(
                        Recent {
                            kind: RecentKind::Merge,
                            left: left.clone(),
                            right: right.clone(),
                            base: Some(base.clone()),
                        },
                        cx,
                    );
                }
            }
            MergeEvent::Swapped { left, base, right } => {
                let new = self.merge_key(index, [left, base, right]);
                // The swapped set is open in another tab and this one holds choices: the
                // sides go back, so the key still names what the tab shows (M6).
                if !self.rekey(index, new, window, cx) {
                    view.update(cx, |v, cx| v.undo_swap(tr(MERGED_ELSEWHERE), window, cx));
                }
            }
        }
        cx.notify();
    }
}
