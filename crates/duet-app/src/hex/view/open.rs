//! Replacing one side of a hex compare: Ctrl+O / Ctrl+Shift+O, the path box and files dropped
//! on the view. Every way in ends at `open_pair`, which tells the workspace
//! (`HexEvent::Open`) to re-key the tab; the workspace calls `load_paths` back.

use std::ops::Range;

use gpui_kit::PathPromptOptions;

use super::*;
use crate::pathbar::{dropped_files, over_left_half, typed_file};

impl HexView {
    /// The side whose path box is being edited.
    #[doc(hidden)]
    pub fn path_edit_for_test(&self) -> Option<bool> {
        self.path_edit.as_ref().map(ActiveEdit::left)
    }

    /// The edited text and the selected byte range.
    #[doc(hidden)]
    pub fn path_edit_state_for_test(&self, cx: &App) -> Option<(String, Range<usize>)> {
        let edit = self.path_edit.as_ref()?.edit();
        Some((edit.text(cx), edit.input.read(cx).selected_range()))
    }

    #[doc(hidden)]
    pub fn set_path_edit_text_for_test(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(edit) = &self.path_edit {
            edit.set_text(text, cx);
        }
    }

    /// Whether a path box has the keys.
    pub(super) fn path_focused(&self, window: &Window, cx: &App) -> bool {
        self.path_edit
            .as_ref()
            .is_some_and(|edit| edit.edit().input.focus_handle(cx).is_focused(window))
    }

    /// Turns a side's path into an input holding it, all selected.
    pub(super) fn begin_path_edit(
        &mut self,
        left: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.path_edit.as_ref().is_some_and(|e| e.left() == left) {
            return;
        }
        let path = if left { &self.left } else { &self.right };
        self.path_edit = Some(ActiveEdit::start(
            left,
            path,
            window,
            cx,
            Self::apply_path_edit,
            // Focus went elsewhere: the bar comes back and nothing was applied.
            |this, _, cx| {
                this.path_edit = None;
                cx.notify();
            },
        ));
        cx.notify();
    }

    /// Esc: the bar comes back, the keys go back to the rows.
    pub(super) fn cancel_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.path_edit = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Enter: an absolute path to a file ends the edit and replaces the side; anything else
    /// keeps the input and says why.
    fn apply_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(active) = &self.path_edit else {
            return;
        };
        let left = active.left();
        match typed_file(&active.edit().text(cx)) {
            Ok(path) => {
                self.cancel_path_edit(window, cx);
                self.open_side(left, path, cx);
            }
            Err(notice) => {
                self.notice = Some(notice.into());
                cx.notify();
            }
        }
    }

    /// Ctrl+O / Ctrl+Shift+O: pick a file for one side.
    pub(super) fn pick_file(&mut self, left: bool, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |view, cx| view.open_side(left, path, cx));
        })
        .detach();
    }

    /// Replaces one side by `path`.
    fn open_side(&mut self, left: bool, path: PathBuf, cx: &mut Context<Self>) {
        let (l, r) = if left {
            (path, self.right.clone())
        } else {
            (self.left.clone(), path)
        };
        self.open_pair(l, r, cx);
    }

    fn open_pair(&mut self, left: PathBuf, right: PathBuf, cx: &mut Context<Self>) {
        if left != self.left || right != self.right {
            cx.emit(HexEvent::Open { left, right });
        }
    }

    /// Files dropped on the view: one lands on the half under the pointer, two replace both.
    pub(super) fn on_files_dropped(
        &mut self,
        dropped: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let body = self.body_bounds.get();
        let x = f32::from(window.mouse_position().x - body.origin.x) + self.skip_x;
        // The split is the gutter's middle, wherever the rows are scrolled to.
        let split = self
            .cols
            .map_or(f32::from(body.size.width) / 2., |c| c.gutter_center());
        self.drop_paths(dropped.paths(), over_left_half(x, split), cx);
    }

    pub fn drop_paths(&mut self, paths: &[PathBuf], pointer_left: bool, cx: &mut Context<Self>) {
        let used = match dropped_files(paths) {
            Ok(used) => used,
            Err(notice) => {
                self.notice = Some(notice.into());
                cx.notify();
                return;
            }
        };
        match used {
            [one] => self.open_side(pointer_left, one.clone(), cx),
            [a, b] => self.open_pair(a.clone(), b.clone(), cx),
            _ => {}
        }
    }
}
