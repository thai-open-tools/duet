//! The merge toolbar (`html:2-21`): the stage switch, the walk through the conflicts (R16)
//! bulk resolve and the options, plus swap, reload and the trailing-whitespace option, then on
//! the right copy, export and save.

use duet_core::text::merge::session::How;
use gpui_kit::MouseDownEvent;
use gpui_kit::component::h_flex;

use super::*;
use crate::ui::fonts::{Fonts, SMALL_SIZE};

/// F5 over choices that are not saved (`js:681`).
const RELOAD: &str = "Reloading drops the conflict choices you have not saved.";
const SWAPPED: &str = "Swapped left and right.";
const SWAPPED_SESSION: &str =
    "Swapped left and right. The result now uses the new left file's encoding and line endings.";
/// The option over choices that are not saved (`js:691`).
const TRAIL_AGAIN: &str =
    "Changing this option merges again and drops the conflict choices you have not saved.";
use crate::ui::icons;
use crate::ui::style::{
    LdExt as _, ld_button, ld_icon_button, ld_primary, ld_segment, ld_segmented, ld_separator,
    ld_toggle,
};

impl MergeView {
    /// F7, F8, Ctrl+Home, Ctrl+End and ◀ ▶ (R16): the conflicts when there are any, else the
    /// changes, with no wrap; the target's card scrolls to the top of the list. From the
    /// input stage this shows the merge. A field with the keys gives them back first, which
    /// ends its edit (R7).
    pub fn go(&mut self, nav: Nav, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let target = session.nav(nav);
        let word = session.nav_word().to_lowercase();
        let nothing = session.changes().is_empty();
        self.leave_input(window, cx);
        self.stage = Stage::Merge;
        match target {
            Some(chunk) => {
                self.notice = None;
                self.scroll_to_chunk(chunk);
            }
            None => {
                let text = if nothing {
                    "No differences.".to_owned()
                } else if nav == Nav::Prev {
                    format!("No previous {word}.")
                } else {
                    format!("No next {word}.")
                };
                self.notice = Some(text.into());
            }
        }
        cx.notify();
    }

    /// F8 and F7 work from a field too (free keys).
    pub(super) fn on_next(&mut self, _: &MergeNext, window: &mut Window, cx: &mut Context<Self>) {
        self.go(Nav::Next, window, cx);
    }

    pub(super) fn on_prev(&mut self, _: &MergePrev, window: &mut Window, cx: &mut Context<Self>) {
        self.go(Nav::Prev, window, cx);
    }

    /// Ctrl+Home and Ctrl+End stay with a field that has the keys (guarded).
    pub(super) fn on_first(&mut self, _: &MergeFirst, window: &mut Window, cx: &mut Context<Self>) {
        if self.takes_key(window, cx) {
            self.go(Nav::First, window, cx);
        }
    }

    pub(super) fn on_last(&mut self, _: &MergeLast, window: &mut Window, cx: &mut Context<Self>) {
        if self.takes_key(window, cx) {
            self.go(Nav::Last, window, cx);
        }
    }

    /// F9 and the input stage's button (`swapLR`): the two sides trade places at once (I1).
    /// Slots, edit flags, field texts, loads still pending (R13) and the session move together:
    /// the session takes its regions mirrored (diff3 is symmetric) with the choices flipped. A
    /// merge still running starts again from the swapped slots, as its result would land the
    /// wrong way round. A field with the keys gives them back first, which ends its edit (R7).
    pub fn swap_sides(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.swap_now(window, cx);
        let Some(session) = self.session.as_ref() else {
            self.notice = Some(SWAPPED.into());
            cx.notify();
            return;
        };
        let [left, base, right] =
            [session.left(), session.base(), session.right()].map(|text| text.path.clone());
        self.notice = Some(SWAPPED_SESSION.into());
        cx.emit(MergeEvent::Swapped { left, base, right });
        cx.notify();
    }

    /// The workspace refused the swapped set (another tab shows it, and this one holds choices
    /// that are not saved): the sides trade back, and `notice` says why (M6).
    pub fn undo_swap(&mut self, notice: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.swap_now(window, cx);
        self.notice = Some(notice.to_owned().into());
        cx.notify();
    }

    fn swap_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_input(window, cx);
        self.slots.swap(LEFT, RIGHT);
        self.messages.swap(LEFT, RIGHT);
        self.failed.swap(LEFT, RIGHT);
        self.loads.swap(LEFT, RIGHT);
        self.load_ids.swap(LEFT, RIGHT);
        self.load_tasks.swap(LEFT, RIGHT);
        self.names.swap(LEFT, RIGHT);
        self.edited.swap(LEFT, RIGHT);
        let [left, right] =
            [LEFT, RIGHT].map(|slot| self.pastes[slot].read(cx).value().to_string());
        self.pastes[LEFT].update(cx, |field, cx| field.set_value(right, window, cx));
        self.pastes[RIGHT].update(cx, |field, cx| field.set_value(left, window, cx));
        if let Some(old) = self.session.as_ref() {
            let mut session = old.swapped();
            // The current part stays (`setCur(oldCur)`), else the first target.
            let target = session.current().or_else(|| session.nav(Nav::First));
            self.session = Some(session);
            self.utf8_agreed = false;
            self.rebuild_cards();
            if let Some(chunk) = target {
                self.scroll_to_chunk(chunk);
            }
            self.refresh_preview(cx);
        }
        if self.merge_job.is_running() {
            self.merge_job.cancel();
            self.merge_task = None;
            self.run_with(true, true, window, cx);
        }
    }

    /// F5 (`reload`): every slot with a file and no edit is read again, and so is a slot that
    /// failed to read; with a session the merge runs again once they are in (PF8). Choices
    /// that are not saved are asked about first.
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_input(window, cx);
        if !self.is_dirty() {
            self.reload_files(cx);
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            RELOAD,
            None,
            &["Cancel", "Reload"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(1) {
                let _ = this.update(cx, |view, cx| view.reload_files(cx));
            }
        })
        .detach();
    }

    /// Reads again every unedited slot's file, and the file a slot failed to read (`S.ent`).
    /// The notice counts the files read, once they are in (M1).
    fn reload_files(&mut self, cx: &mut Context<Self>) {
        let files: Vec<(usize, PathBuf)> = (LEFT..=RIGHT)
            .filter(|&slot| !self.edited[slot])
            .filter_map(|slot| {
                let file = self.slots[slot].as_ref().and_then(|text| text.path.clone());
                Some((slot, file.or_else(|| self.failed[slot].clone())?))
            })
            .collect();
        if files.is_empty() {
            self.notice = Some("No files to reload.".into());
            cx.notify();
            return;
        }
        if self.session.is_some() {
            self.force_next_run = true;
            self.quiet_next_run = true;
        }
        for (slot, path) in files {
            self.start_load(slot, path, cx);
        }
        self.reloaded = Some(0);
        cx.notify();
    }

    /// "Ignore trailing whitespace" (R3).
    pub fn trail(&self) -> bool {
        self.trail
    }

    /// Changes the option (`setTrail`). Over choices that are not saved it asks first; Cancel
    /// leaves the option as it was. The setting is saved, and a session merges again from its
    /// own texts.
    pub fn set_trail(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        // A merge running now would land with the old option (I3).
        if on == self.trail || !self.may_edit(cx) {
            return;
        }
        if !self.is_dirty() {
            self.apply_trail(on, window, cx);
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            TRAIL_AGAIN,
            None,
            &["Cancel", "Merge again"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(1) {
                let _ = this.update_in(cx, |view, window, cx| view.apply_trail(on, window, cx));
            }
        })
        .detach();
    }

    fn apply_trail(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.trail = on;
        if let Some(file) = settings::settings_file(cx) {
            let _ = settings::set_bool(&file, TRAIL_SETTING, on);
        }
        if let Some(session) = self.session.as_ref() {
            let texts = [
                session.left().clone(),
                session.base().clone(),
                session.right().clone(),
            ];
            self.leave_input(window, cx);
            self.start_merge(texts, false, cx);
        }
        cx.notify();
    }

    pub(super) fn on_swap(&mut self, _: &MergeSwap, window: &mut Window, cx: &mut Context<Self>) {
        self.swap_sides(window, cx);
    }

    pub(super) fn on_reload(
        &mut self,
        _: &MergeReload,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reload(window, cx);
    }

    /// The bar over both stages: the stage switch, then ◀, the position and ▶.
    pub(super) fn render_toolbar(
        &self,
        p: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let stage_segment =
            |id: &'static str, label: &'static str, tip: &'static str, stage: Stage, last: bool| {
                let enabled = stage == Stage::Input || self.session.is_some();
                ld_segment(id, label, self.stage == stage, last, p)
                    .ld_tooltip(tip)
                    .ld_click(
                        enabled,
                        cx.listener(move |this, _, _, cx| this.set_stage(stage, cx)),
                    )
            };
        let segments = [
            stage_segment(
                "merge-stage-input",
                "Input",
                "Choose or paste the three sides",
                Stage::Input,
                false,
            ),
            stage_segment(
                "merge-stage-merge",
                "Merge result",
                "Resolve the conflicts one by one",
                Stage::Merge,
                true,
            ),
        ];
        let session = self.session.as_ref();
        let can_go = |forward| session.is_some_and(|s| s.can_go(forward));
        let position: SharedString = session
            .map_or_else(|| "Conflict 0 / 0".to_owned(), MergeSession::position_text)
            .into();
        // The walk is over the conflicts, else the changes (M10).
        let word = session.map_or("conflict".to_owned(), |s| s.nav_word().to_lowercase());
        // Bulk resolve waits for conflicts (`updateToolbar`).
        let has_session = session.is_some();
        let has_conflicts = session.is_some_and(|s| !s.conflicts().is_empty());
        let bulk = |id: &'static str, label: &'static str, tip: &'static str, how: How| {
            ld_button(id, label, p).ld_tooltip(tip).ld_click(
                has_conflicts,
                cx.listener(move |this, _, window, cx| {
                    this.resolve_all(how, window, cx);
                }),
            )
        };
        h_flex()
            .id("merge-toolbar")
            .test_support()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap(px(5.))
            .px(px(10.))
            .py(px(5.))
            .bg(p.panel)
            .border_b_1()
            .border_color(p.line)
            // The buttons take no keys, so a result field keeps them: a press here ends its
            // typing as a blur would, and the next typing is a step of its own (M-49).
            .capture_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                this.end_typing(window, cx);
            }))
            .child(ld_segmented("merge-stage-seg", segments, p))
            .child(ld_separator(p))
            .child(
                ld_icon_button("merge-prev", icons::PREV, format!("Previous {word} (F7)"), p).ld_click(
                    can_go(false),
                    cx.listener(|this, _, window, cx| this.go(Nav::Prev, window, cx)),
                ),
            )
            .child(
                h_flex()
                    .id("merge-pos")
                    .test_support()
                    .aria_label(position.clone())
                    .min_w(px(112.))
                    .justify_center()
                    .font(fonts.code.clone())
                    .text_size(px(SMALL_SIZE))
                    .text_color(p.muted)
                    .whitespace_nowrap()
                    .child(position),
            )
            .child(
                ld_icon_button("merge-next", icons::NEXT, format!("Next {word} (F8)"), p).ld_click(
                    can_go(true),
                    cx.listener(|this, _, window, cx| this.go(Nav::Next, window, cx)),
                ),
            )
            .child(ld_separator(p))
            .child(bulk(
                "merge-all-left",
                "Resolve all with left",
                "Use the left side for every conflict not resolved yet",
                How::Left,
            ))
            .child(bulk(
                "merge-all-right",
                "Resolve all with right",
                "Use the right side for every conflict not resolved yet",
                How::Right,
            ))
            .child(ld_separator(p))
            .child(
                ld_toggle("merge-trail", "Ignore trailing whitespace", self.trail, p)
                    .ld_tooltip(
                        "Lines that differ only in spaces or tabs at the end count as the same (merges again)",
                    )
                    .ld_click(
                        true,
                        cx.listener(|this, _, window, cx| {
                            let on = !this.trail;
                            this.set_trail(on, window, cx)
                        }),
                    ),
            )
            .child(
                ld_toggle(
                    "merge-show-result",
                    "Show the whole result",
                    self.show_result,
                    p,
                )
                .ld_tooltip("Show the assembled result beside the changes")
                .ld_click(
                    self.stage == Stage::Merge && session.is_some(),
                    cx.listener(|this, _, _, cx| {
                        let on = !this.show_result;
                        this.set_show_result(on, cx)
                    }),
                ),
            )
            .child(div().flex_1())
            .child(
                ld_button("merge-copy", "Copy result", p)
                    .ld_tooltip("Copy the whole result to the clipboard")
                    .ld_click(
                        has_session,
                        cx.listener(|this, _, _, cx| this.copy_result(cx)),
                    ),
            )
            .child(
                ld_button("merge-export", "Export with conflict markers", p)
                    .ld_tooltip(
                        "Save as a new file, writing unresolved conflicts as <<<<<<< left / ||||||| base / ======= / >>>>>>> right",
                    )
                    .ld_click(
                        has_session,
                        cx.listener(|this, _, window, cx| this.export_markers(window, cx).detach()),
                    ),
            )
            .child(
                ld_primary("merge-save", "Save result", p)
                    .ld_tooltip("Save the result (Ctrl+S)")
                    .ld_click(
                        has_session,
                        cx.listener(|this, _, window, cx| this.save(false, window, cx).detach()),
                    ),
            )
    }
}
