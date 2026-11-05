//! Replacing one side of a text compare (rules R8, R10, R11): Ctrl+O, the path box, files
//! dropped on a half, and the clipboard pasted as a whole side.
//!
//! Every way in ends at `open_pair`, which asks about unsaved edits and then tells the
//! workspace (`TextEvent::Open`) to re-key the tab; the workspace calls `load_paths` back.

use std::ops::Range;
use std::path::PathBuf;

use gpui_kit::{App, Context, ExternalPaths, PathPromptOptions, ScrollStrategy, Window};

use super::view::stage::{OVERVIEW_WIDTH, stage_columns};
use super::view::{TextEvent, TextView};
use crate::i18n::{tr, tr_key};
use crate::pathbar::{self, ActiveEdit, dropped_files, typed_file};
use crate::text::caret::Caret;
use crate::text::editing::{WAIT_NOTICE, normalize_breaks};
use crate::text::job::TextRequest;

const NO_CLIPBOARD_TEXT: &str = tr_key("The clipboard has no text.");

/// A path box being typed into, with what ends it.
pub(crate) type SideEdit = ActiveEdit;

/// True when `x` (px from the view's left edge, in a view `width` wide) is over the left half.
fn over_left_half(x: f32, width: f32) -> bool {
    let columns = stage_columns(width);
    pathbar::over_left_half(x, (columns.gutter.start + columns.gutter.end) / 2.)
}

impl TextView {
    /// The side has no file yet: its request path is empty (R15).
    pub fn is_untitled(&self, left: bool) -> bool {
        let request = &self.request;
        (if left { &request.left } else { &request.right })
            .as_os_str()
            .is_empty()
    }

    /// A click on an empty half of an untitled pair: the caret goes to the start of that side.
    pub fn click_empty_side(&mut self, left: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(model) = &mut self.model else {
            return;
        };
        if let Err(reason) = model.begin_edit(left) {
            self.notice = Some(reason.into());
            cx.notify();
            return;
        }
        self.place_caret(Caret::at(left, 0), cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The side whose path box is being edited.
    #[doc(hidden)]
    pub fn path_edit_for_test(&self) -> Option<bool> {
        self.path_edit.as_ref().map(|e| e.edit.left)
    }

    /// The edited text and the selected byte range.
    #[doc(hidden)]
    pub fn path_edit_state_for_test(&self, cx: &App) -> Option<(String, Range<usize>)> {
        let edit = &self.path_edit.as_ref()?.edit;
        Some((edit.text(cx), edit.input.read(cx).selected_range()))
    }

    #[doc(hidden)]
    pub fn set_path_edit_text_for_test(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(edit) = &self.path_edit {
            edit.set_text(text, cx);
        }
    }

    /// Turns a side's path into an input holding it, all selected.
    pub(crate) fn begin_path_edit(
        &mut self,
        left: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.path_edit.as_ref().is_some_and(|e| e.edit.left == left) {
            return;
        }
        let path = if left {
            &self.request.left
        } else {
            &self.request.right
        };
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
    pub(crate) fn cancel_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.path_edit = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Enter: a file ends the edit and replaces the side; anything else keeps the input.
    fn apply_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(SideEdit { edit, .. }) = &self.path_edit else {
            return;
        };
        let left = edit.left;
        match typed_file(&edit.text(cx)) {
            Ok(path) => {
                self.cancel_path_edit(window, cx);
                self.open_path(left, path, window, cx);
            }
            Err(notice) => {
                self.notice = Some(notice.into());
                cx.notify();
            }
        }
    }

    /// Ctrl+O / Ctrl+Shift+O: pick a file for one side.
    pub(crate) fn pick_file(&mut self, left: bool, window: &mut Window, cx: &mut Context<Self>) {
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
            let _ = this.update_in(cx, |view, window, cx| {
                view.open_path(left, path, window, cx)
            });
        })
        .detach();
    }

    /// Replaces one side by `path`, after the unsaved edits are dealt with. The other side is
    /// read once they are: a Save there may have given an untitled side its file (I2).
    pub fn open_path(
        &mut self,
        left: bool,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_with(window, cx, move |request| {
            if left {
                (path.clone(), request.right.clone())
            } else {
                (request.left.clone(), path.clone())
            }
        });
    }

    fn open_pair(
        &mut self,
        left: PathBuf,
        right: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_with(window, cx, move |_| (left.clone(), right.clone()));
    }

    /// Emits `Open` for the pair `pair` makes of the request, once the unsaved edits are dealt
    /// with; nothing when that is the pair shown.
    fn open_with(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        pair: impl Fn(&TextRequest) -> (PathBuf, PathBuf) + 'static,
    ) {
        let (left, right) = pair(&self.request);
        if left == self.request.left && right == self.request.right {
            return;
        }
        let ok = self.confirm_discard(window, cx);
        cx.spawn(async move |this, cx| {
            if ok.await {
                let _ = this.update(cx, |view, cx| {
                    let (left, right) = pair(&view.request);
                    cx.emit(TextEvent::Open { left, right })
                });
            }
        })
        .detach();
    }

    /// Files dropped on the view: one lands on the half under the pointer, two replace both.
    pub(crate) fn on_files_dropped(
        &mut self,
        dropped: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let list = self.list_bounds.get();
        let width = f32::from(list.size.width) + OVERVIEW_WIDTH;
        let x = f32::from(window.mouse_position().x - list.origin.x);
        self.drop_paths(dropped.paths(), over_left_half(x, width), window, cx);
    }

    pub fn drop_paths(
        &mut self,
        paths: &[PathBuf],
        pointer_left: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let used = match dropped_files(paths) {
            Ok(used) => used,
            Err(notice) => {
                self.notice = Some(notice.into());
                cx.notify();
                return;
            }
        };
        match used {
            [one] => self.open_path(pointer_left, one.clone(), window, cx),
            [a, b] => self.open_pair(a.clone(), b.clone(), window, cx),
            _ => {}
        }
    }

    /// Replaces one side's whole text by the clipboard's, with the side's own line breaks, as
    /// one undo step. False (with a notice) when nothing changed.
    pub fn replace_side_with_clipboard(&mut self, left: bool, cx: &mut Context<Self>) -> bool {
        let Some(text) = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .filter(|text| !text.is_empty())
        else {
            self.notice = Some(tr(NO_CLIPBOARD_TEXT).into());
            cx.notify();
            return false;
        };
        if self.job.is_running() {
            self.notice = Some(tr(WAIT_NOTICE).into());
            cx.notify();
            return false;
        }
        let Some(model) = self.model.as_mut() else {
            return false;
        };
        if let Err(reason) = model.begin_edit(left) {
            self.notice = Some(reason.into());
            cx.notify();
            return false;
        }
        let buffer = model.buffer(left).expect("begin_edit made the buffer");
        let len = buffer.len();
        let text = normalize_breaks(&text, buffer.terminator(0));
        self.undo.seal();
        let done = self.replace_on(left, 0..len, &text, cx);
        self.undo.seal();
        done
    }

    /// Compares `left` and `right` instead, once the caller has dealt with unsaved edits.
    pub fn load_paths(&mut self, left: PathBuf, right: PathBuf, cx: &mut Context<Self>) {
        let request = &mut self.request;
        if request.left != left {
            request.left_encoding = None;
        }
        if request.right != right {
            request.right_encoding = None;
        }
        request.left = left;
        request.right = right;
        request.force_text = false;
        // Pins and a waiting alignment belong to the old pair.
        request.rules.anchors.clear();
        self.align = None;
        self.watch.new_pair();
        // The rows, folds and edits belong to the old files.
        if let Some(model) = self.model.take() {
            self.pending_filter = Some(model.filter());
        }
        self.caret = None;
        self.row_sel = None;
        self.row_drag = None;
        self.menu_row = None;
        self.path_edit = None;
        self.undo.clear();
        self.scroll_to_display(0, ScrollStrategy::Top);
        self.reload(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pathbar::not_a_file;

    #[test]
    fn the_halves_split_at_the_middle_of_the_centre_gutter() {
        let columns = stage_columns(1200.);
        let middle = (columns.gutter.start + columns.gutter.end) / 2.;
        assert!(over_left_half(10., 1200.));
        assert!(over_left_half(middle - 1., 1200.));
        assert!(!over_left_half(middle + 1., 1200.));
        assert!(!over_left_half(1100., 1200.));
    }

    #[test]
    fn the_not_a_file_notice_names_the_path_as_typed() {
        assert_eq!(not_a_file(r"C:\x\y"), r"Not a file: C:\x\y.");
    }
}
