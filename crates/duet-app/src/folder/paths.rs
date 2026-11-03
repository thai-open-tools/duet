//! Replacing the folders of a folder compare (rules R10, R11): the path boxes take typing
//! and the view takes dropped folders. The text view's twin is `text/sides.rs`.

use std::path::PathBuf;

use gpui_kit::{AnyElement, ClickEvent, Context, ExternalPaths, SharedString, Window, prelude::*};

use super::view::FolderView;
use crate::palette::Palette;
use crate::pathbar::{ActiveEdit, clean_typed, path_bar_click, path_input};

pub const FILE_DROP_NOTICE: &str = "Drop folders here; files open in a text compare.";

const NOT_FULL_PATH: &str = "Type a full path, such as C:\\folder.";

fn not_a_folder(path: &str) -> SharedString {
    format!("Not a folder: {path}.").into()
}

impl FolderView {
    /// The side whose path box is being edited.
    #[doc(hidden)]
    pub fn path_edit_for_test(&self) -> Option<bool> {
        self.path_edit.as_ref().map(ActiveEdit::left)
    }

    #[doc(hidden)]
    pub fn set_path_edit_text_for_test(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(edit) = &self.path_edit {
            edit.set_text(text, cx);
        }
    }

    /// A side's path box: the bar (a click turns it into an input) or the input itself.
    pub(super) fn path_box_content(
        &self,
        left: bool,
        id: &'static str,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.path_edit.as_ref().filter(|e| e.left() == left) {
            Some(edit) => path_input(id, edit.edit(), p),
            None => {
                let path = if left { &self.left } else { &self.right };
                path_bar_click(
                    id,
                    path,
                    p,
                    cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.begin_path_edit(left, window, cx)
                    }),
                )
                .into_any_element()
            }
        }
    }

    fn begin_path_edit(&mut self, left: bool, window: &mut Window, cx: &mut Context<Self>) {
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

    /// Enter: a folder ends the edit and replaces that root; anything else keeps the input.
    fn apply_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &self.path_edit else {
            return;
        };
        let left = edit.left();
        let typed = edit.edit().text(cx);
        let typed = clean_typed(&typed);
        let path = PathBuf::from(typed);
        if !typed.is_empty() && !path.is_absolute() {
            self.notice = Some(NOT_FULL_PATH.into());
            cx.notify();
            return;
        }
        // Typed spellings such as C:/x/y or a path with ".." become the plain absolute path.
        let path = std::path::absolute(&path).unwrap_or(path);
        if self.refuse_while_batch(cx) {
            return;
        }
        if typed.is_empty() || !path.is_dir() {
            self.notice = Some(not_a_folder(typed));
            cx.notify();
            return;
        }
        self.cancel_path_edit(window, cx);
        self.replace_roots(left.then_some(path.clone()), (!left).then_some(path), cx);
    }

    fn replace_roots(
        &mut self,
        left: Option<PathBuf>,
        right: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let left = left.unwrap_or_else(|| self.left.clone());
        let right = right.unwrap_or_else(|| self.right.clone());
        self.set_roots(left, right, cx);
    }

    /// Folders dropped on the view: one lands on the half under the pointer, two replace both.
    pub(super) fn on_folders_dropped(
        &mut self,
        dropped: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let left_half = window.mouse_position().x < window.viewport_size().width / 2.;
        self.drop_paths(dropped.paths(), left_half, cx);
    }

    pub fn drop_paths(&mut self, paths: &[PathBuf], pointer_left: bool, cx: &mut Context<Self>) {
        let used = &paths[..paths.len().min(2)];
        if used.is_empty() || self.refuse_while_batch(cx) {
            return;
        }
        if used.iter().any(|p| !p.is_dir()) {
            self.notice = Some(FILE_DROP_NOTICE.into());
            cx.notify();
            return;
        }
        self.path_edit = None;
        match used {
            [one] if pointer_left => self.replace_roots(Some(one.clone()), None, cx),
            [one] => self.replace_roots(None, Some(one.clone()), cx),
            [a, b] => self.replace_roots(Some(a.clone()), Some(b.clone()), cx),
            _ => {}
        }
    }
}
