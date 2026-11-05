//! Replacing a side of a picture compare: Ctrl+O / Ctrl+Shift+O and the Open buttons, a head
//! name turned into a path box, and files dropped on the body (R16). Every way to a file ends
//! at `open_pair`, which tells the workspace (`PictureEvent::Open`) to re-key the tab; the
//! workspace calls `load_paths` back.

use std::ops::Range;

use gpui_kit::component::h_flex;
use gpui_kit::{AnyElement, Div, ExternalPaths, PathPromptOptions, Stateful, div};

use super::*;
use crate::i18n::{tr, trf};
use crate::pathbar::{ActiveEdit, dropped_files, typed_file};
use crate::ui::fonts::SMALL_SIZE;
use crate::ui::prose::ProseExt as _;

/// The group of both drop zones: a file dragged over the body outlines each half.
const DROP_BODY: &str = "picture-drop-body";

impl PictureView {
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

    /// Turns a head's name into an input holding the path, all selected.
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
        let path = self.sides[index]
            .as_ref()
            .and_then(|side| side.info.path.clone())
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

    /// Esc: the name comes back, the keys go back to the view.
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
        let mut paths = self.paths.clone();
        paths[side_index(left)] = path;
        self.open_pair(paths, cx);
    }

    /// Shows `paths`, unless the tab names them already. True when the workspace was asked.
    fn open_pair(&mut self, paths: [PathBuf; 2], cx: &mut Context<Self>) -> bool {
        self.carried = None;
        if paths == self.paths {
            return false;
        }
        let [left, right] = paths;
        cx.emit(PictureEvent::Open { left, right });
        true
    }

    /// Files dropped on a half of the body: one replaces that half's side, two replace both,
    /// and more than two use the first two (R16).
    pub fn drop_paths(&mut self, paths: &[PathBuf], pointer_left: bool, cx: &mut Context<Self>) {
        let used = match dropped_files(paths) {
            Ok(used) => used,
            Err(notice) => {
                self.notice = Some(notice.into());
                cx.notify();
                return;
            }
        };
        let asked = match used {
            [one] => {
                let mut pair = self.paths.clone();
                pair[side_index(pointer_left)] = one.clone();
                self.open_pair(pair, cx)
            }
            [a, b] => self.open_pair([a.clone(), b.clone()], cx),
            _ => return,
        };
        if paths.len() > 2 {
            let notice: SharedString =
                trf("{} files dropped: the first two are used.", &[&paths.len()]).into();
            // The new pair clears the notice when it loads, so it is handed over.
            if asked {
                self.carried = Some(notice.clone());
            }
            self.notice = Some(notice);
            cx.notify();
        }
    }

    /// The two halves of the body, each taking the drop for its side (R16; LocalDiff `css:103`):
    /// while a file is dragged over the body both show a dashed `line` outline and their label,
    /// and the half under the pointer turns `accent` on `bg_mod`. gpui applies drag styles only
    /// to an element with a hitbox, so the zone (which has one for its drop) carries them all:
    /// it is see-through at rest, and its label takes its text colour.
    pub(super) fn drop_zones(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let zone = |left: bool| {
            let (id, text) = if left {
                ("picture-drop-left", tr("Drop as the left picture"))
            } else {
                ("picture-drop-right", tr("Drop as the right picture"))
            };
            let (line, accent, lit) = (p.line, p.accent, p.bg_mod);
            div()
                .id(id)
                .test_support()
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .opacity(0.)
                .text_color(p.muted)
                .group_drag_over::<ExternalPaths>(DROP_BODY, move |style| {
                    style
                        .opacity(1.)
                        .border_2()
                        .border_dashed()
                        .border_color(line)
                })
                .drag_over::<ExternalPaths>(move |style, _, _, _| {
                    style.bg(lit).border_color(accent).text_color(accent)
                })
                .on_drop(cx.listener(move |this, dropped: &ExternalPaths, _, cx| {
                    this.drop_paths(dropped.paths(), left, cx)
                }))
                .child(
                    div()
                        .px(px(12.))
                        .py(px(5.))
                        .rounded(px(8.))
                        .bg(p.panel3)
                        .text_size(px(SMALL_SIZE))
                        .prose(text),
                )
        };
        h_flex()
            .group(DROP_BODY)
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(zone(true))
            .child(zone(false))
            .into_any_element()
    }

    /// The Open buttons' keys, the menu's items and the export, bound to the view.
    pub(super) fn open_actions(root: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        root.on_action(cx.listener(|this, _: &PictureOpenLeft, w, cx| {
            if this.takes_key(w, cx) {
                this.pick_file(true, w, cx)
            }
        }))
        .on_action(cx.listener(|this, _: &PictureOpenRight, w, cx| {
            if this.takes_key(w, cx) {
                this.pick_file(false, w, cx)
            }
        }))
        .on_action(cx.listener(|this, _: &PictureExport, w, cx| {
            if this.takes_key(w, cx) {
                this.export(w, cx)
            }
        }))
    }
}
