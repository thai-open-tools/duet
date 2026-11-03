//! Replacing one side of a table compare: Ctrl+O / Ctrl+Shift+O, the name label turned into a
//! path box, files dropped on the view and the clipboard (R10). Every way to a file ends at
//! `open_pair`, which tells the workspace (`TableEvent::Open`) to re-key the tab; the workspace
//! calls `load_paths` back. Pasted text stays in this tab until a file is opened on its side.

use std::ops::Range;

use gpui_kit::{ExternalPaths, PathPromptOptions};

use super::*;
use crate::pathbar::{ActiveEdit, dropped_files, over_left_half, typed_file};

const NO_CLIPBOARD_TEXT: &str = "The clipboard has no text.";

impl TableView {
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

    /// The focus handle of the open path box, if any.
    pub(super) fn path_input(&self, cx: &App) -> Option<FocusHandle> {
        let edit = self.path_edit.as_ref()?;
        Some(edit.edit().input.focus_handle(cx))
    }

    /// Turns a side's name into an input holding its path, all selected.
    pub(super) fn begin_path_edit(
        &mut self,
        left: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.path_edit.as_ref().is_some_and(|e| e.left() == left) {
            return;
        }
        let index = side_index(left);
        let path = self.kept[index]
            .as_ref()
            .and_then(|kept| kept.info.path.clone())
            .unwrap_or_else(|| self.paths[index].clone());
        self.path_edit = Some(ActiveEdit::start(
            left,
            &path,
            window,
            cx,
            Self::apply_path_edit,
            // Focus went elsewhere: the name comes back and nothing was applied.
            |this, _, cx| {
                this.path_edit = None;
                cx.notify();
            },
        ));
        cx.notify();
    }

    /// Esc: the name comes back, the keys go back to the rows.
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

    /// Ctrl+O / Ctrl+Shift+O and the Open buttons: pick a file for one side.
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

    /// Replaces one side by the file at `path`; the other side stays as it is.
    fn open_side(&mut self, left: bool, path: PathBuf, cx: &mut Context<Self>) {
        let index = side_index(left);
        let mut paths = self.paths.clone();
        paths[index] = path;
        let mut named = [false, false];
        named[index] = true;
        self.open_pair(paths, named, cx);
    }

    /// Shows `paths`. The `named` sides were asked for by an open, a drop or a typed path:
    /// their files replace any pasted text, which only F5 and a re-parse keep.
    fn open_pair(&mut self, paths: [PathBuf; 2], named: [bool; 2], cx: &mut Context<Self>) {
        if paths == self.paths {
            // The tab already names these files; pasted text on a named side is what they
            // replace.
            let replaced = [LEFT, RIGHT].map(|i| named[i] && self.pasted[i]);
            if replaced.contains(&true) {
                self.pasted = [LEFT, RIGHT].map(|i| self.pasted[i] && !replaced[i]);
                self.start(Mode::Reload, cx);
            }
            return;
        }
        self.opened = named;
        let [left, right] = paths;
        cx.emit(TableEvent::Open { left, right });
    }

    /// Files dropped on the view: one lands on the half under the pointer, two replace both.
    pub(super) fn on_files_dropped(
        &mut self,
        dropped: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let x = f32::from(window.mouse_position().x - self.body_bounds.get().origin.x);
        self.drop_paths(dropped.paths(), over_left_half(x, self.drop_split()), cx);
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
            [a, b] => self.open_pair([a.clone(), b.clone()], [true, true], cx),
            _ => {}
        }
    }

    /// "Paste left/right" (R10): the clipboard's text becomes the side, with no dialog.
    pub(super) fn paste_side(&mut self, left: bool, cx: &mut Context<Self>) {
        let Some(text) = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .filter(|text| !text.is_empty())
        else {
            self.notice = Some(NO_CLIPBOARD_TEXT.into());
            cx.notify();
            return;
        };
        let index = side_index(left);
        self.kept[index] = Some(pasted_kept(left, &text));
        self.pasted[index] = true;
        self.start(Mode::Reparse, cx);
    }
}
