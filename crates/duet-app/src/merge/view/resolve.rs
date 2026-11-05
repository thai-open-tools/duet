//! Resolving the changes (R4, R5; LocalDiff `js:449-460`, `js:512-597`): a card's choice
//! buttons, reset and result field, Alt+arrows and the card menu, bulk resolve, and the
//! per-part undo and redo.

use duet_core::text::merge::session::How;
use gpui_kit::component::h_flex;
use gpui_kit::{AnyElement, ClickEvent, Div, FontWeight};

use super::*;
use crate::merge::items::{count_text, field_placeholder, field_rows, field_value, state_text};
use crate::ui::edit_menu::ld_textarea;
use crate::ui::fonts::{CODE_SIZE, Fonts};
use crate::ui::icons;
use crate::ui::style::{LdExt as _, ld_button, ld_small_icon_button};

const CHOOSE_FIRST: &str = tr_key("Choose a part first: press F8 to go to the next conflict.");
const NOTHING_TO_UNDO: &str = tr_key("Nothing to undo.");
const NOTHING_TO_REDO: &str = tr_key("Nothing to redo.");

/// The choice buttons' ids, in `How::CHOICES` order.
const CHOICE_IDS: [&str; 5] = [
    "merge-use-left",
    "merge-use-right",
    "merge-use-left-right",
    "merge-use-right-left",
    "merge-use-base",
];

/// A field's rows and placeholder follow whether its part is resolved (R5).
fn set_field_look(
    field: &mut TextareaState,
    resolved: bool,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let (rows, rows_max) = field_rows(resolved);
    field.set_auto_grow(rows, rows_max, cx);
    field.set_placeholder(field_placeholder(resolved), window, cx);
}

impl MergeView {
    /// A choice for `chunk`, as one history step; it becomes the current part. False for a
    /// stable chunk, for `Manual`, which is typed, and while the session is being replaced.
    pub fn resolve(
        &mut self,
        chunk: usize,
        how: How,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.session.is_none() || !self.may_edit(cx) {
            return false;
        }
        let done = self.session.as_mut().is_some_and(|s| s.resolve(chunk, how));
        if done {
            self.chunks_changed(&[chunk], window, cx);
        }
        done
    }

    /// Back to the part's default: its automatic side, or unresolved for a conflict.
    pub fn reset(&mut self, chunk: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.is_none() || !self.may_edit(cx) {
            return;
        }
        if self.session.as_mut().is_some_and(|s| s.reset(chunk)) {
            self.chunks_changed(&[chunk], window, cx);
        }
    }

    /// "Resolve all with left/right": every conflict not resolved yet takes `how`, as one
    /// history step (`resolveAll`). Returns how many did; the notice says so.
    pub fn resolve_all(&mut self, how: How, window: &mut Window, cx: &mut Context<Self>) -> usize {
        if self.session.is_none() || !self.may_edit(cx) {
            return 0;
        }
        let Some(session) = self.session.as_mut() else {
            return 0;
        };
        let open: Vec<usize> = session
            .conflicts()
            .iter()
            .copied()
            .filter(|&chunk| session.resolution(chunk).is_none())
            .collect();
        let no_conflicts = session.conflicts().is_empty();
        let resolved = session.resolve_all(how);
        let notice = if resolved > 0 {
            let count = count_of(resolved as u64, "conflict", "conflicts");
            match how {
                How::Left => trf("Resolved {} with the left side.", &[&count]),
                How::Right => trf("Resolved {} with the right side.", &[&count]),
                _ => trf("Resolved {}.", &[&count]),
            }
        } else if no_conflicts {
            tr("No conflicts.").to_owned()
        } else {
            tr("No unresolved conflicts left.").to_owned()
        };
        self.notice = Some(notice.into());
        if resolved > 0 {
            self.chunks_changed(&open, window, cx);
        }
        cx.notify();
        resolved
    }

    pub fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.step_history(false, window, cx);
    }

    pub fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.step_history(true, window, cx);
    }

    /// Undo or redo one step (`undoRedo`): its parts go back or forward, the first of them
    /// becomes current and its card comes into view.
    fn step_history(&mut self, redo: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.is_none() || !self.may_edit(cx) {
            return;
        }
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let moved = if redo { session.redo() } else { session.undo() };
        match moved {
            None => {
                let nothing = if redo {
                    NOTHING_TO_REDO
                } else {
                    NOTHING_TO_UNDO
                };
                self.notice = Some(tr(nothing).into());
            }
            Some(chunks) => {
                self.notice = None;
                self.chunks_changed(&chunks, window, cx);
                if let Some(&first) = chunks.first() {
                    self.scroll_to_chunk(first);
                }
            }
        }
        cx.notify();
    }

    /// Parts whose result changed other than by typing (`updateChunk(i, true)`): each card is
    /// drawn and measured again, and each field shows the new result.
    fn chunks_changed(&mut self, chunks: &[usize], window: &mut Window, cx: &mut Context<Self>) {
        for &chunk in chunks {
            self.refresh_card(chunk);
            let Some(field) = self.field(chunk).cloned() else {
                continue;
            };
            let resolution = self.session.as_ref().and_then(|s| s.resolution(chunk));
            let (text, resolved) = (field_value(resolution), resolution.is_some());
            field.update(cx, |field, cx| {
                field.set_value(text, window, cx);
                set_field_look(field, resolved, window, cx);
            });
        }
        self.schedule_preview(cx);
        cx.notify();
    }

    /// Alt+arrows and the card menu: the current part takes a choice, or its default with
    /// `None` (`copyL`, `copyR`).
    fn choose(&mut self, how: Option<How>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let Some(chunk) = session.current() else {
            self.notice = Some(tr(CHOOSE_FIRST).into());
            cx.notify();
            return;
        };
        match how {
            Some(how) => {
                self.resolve(chunk, how, window, cx);
            }
            None => self.reset(chunk, window, cx),
        }
    }

    /// Alt+← and Alt+→ are guarded: a field with the keys keeps them.
    pub(super) fn on_use_left(
        &mut self,
        _: &MergeUseLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.takes_key(window, cx) {
            self.choose(Some(How::Left), window, cx);
        }
    }

    pub(super) fn on_use_right(
        &mut self,
        _: &MergeUseRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.takes_key(window, cx) {
            self.choose(Some(How::Right), window, cx);
        }
    }

    /// The card menu's other items have no keys.
    pub(super) fn on_use_left_right(
        &mut self,
        _: &MergeUseLeftRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.choose(Some(How::LeftThenRight), window, cx);
    }

    pub(super) fn on_use_right_left(
        &mut self,
        _: &MergeUseRightLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.choose(Some(How::RightThenLeft), window, cx);
    }

    pub(super) fn on_use_base(
        &mut self,
        _: &MergeUseBase,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.choose(Some(How::Base), window, cx);
    }

    pub(super) fn on_reset(&mut self, _: &MergeReset, window: &mut Window, cx: &mut Context<Self>) {
        self.choose(None, window, cx);
    }

    /// Ctrl+Z, Ctrl+Y and Ctrl+Shift+Z are guarded: a field with the keys has its own undo
    /// (R5). The per-part history acts on the merge stage (`js:973`).
    pub(super) fn on_undo(&mut self, _: &MergeUndo, window: &mut Window, cx: &mut Context<Self>) {
        if self.takes_key(window, cx) && self.stage == Stage::Merge {
            self.undo(window, cx);
        }
    }

    pub(super) fn on_redo(&mut self, _: &MergeRedo, window: &mut Window, cx: &mut Context<Self>) {
        if self.takes_key(window, cx) && self.stage == Stage::Merge {
            self.redo(window, cx);
        }
    }

    /// A press that takes no keys (the toolbar, M-49) ends the typing as a blur would. Both
    /// histories end their step: the session's, and the focused field's own, which is what
    /// Ctrl+Z undoes while the field keeps the keys (R5).
    pub(super) fn end_typing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = self.session.as_mut() {
            session.end_edit();
        }
        let focused = self
            .result_fields()
            .find(|field| field.focus_handle(cx).contains_focused(window, cx))
            .cloned();
        if let Some(field) = focused {
            // Setting the selection the field has is its one public undo boundary (a click
            // in the field and its blur set one the same way).
            field.update(cx, |field, cx| {
                let range = field.selected_range();
                field.set_selected_range(range, cx);
            });
        }
    }

    /// A result field's events (R5): typing is `Manual`, one history step until the edit
    /// ends; focus makes the part current and keeps its card drawn; blur ends the edit.
    pub(super) fn on_field_event(
        &mut self,
        chunk: usize,
        field: &Entity<TextareaState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A field of an older session has nothing to say.
        if self.field(chunk) != Some(field) {
            return;
        }
        match event {
            InputEvent::Change => self.typed(chunk, field, window, cx),
            InputEvent::Focus => {
                if let Some(session) = self.session.as_mut() {
                    session.set_current(chunk);
                }
                self.keep_field_drawn(chunk, Some(field.focus_handle(cx)));
                cx.notify();
            }
            InputEvent::Blur => {
                if let Some(session) = self.session.as_mut() {
                    session.end_edit();
                }
                self.keep_field_drawn(chunk, None);
                cx.notify();
            }
            InputEvent::PressEnter { .. } => {}
        }
    }

    /// Typing in a field (`onTaInput`). The card is drawn again only when its state changes
    /// (to "Edited"), so a keystroke does not rebuild the card's marks. While the session is
    /// being replaced the field goes back to the part's result (I3).
    fn typed(
        &mut self,
        chunk: usize,
        field: &Entity<TextareaState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.may_edit(cx) {
            let resolution = self.session.as_ref().and_then(|s| s.resolution(chunk));
            let text = field_value(resolution);
            field.update(cx, |field, cx| field.set_value(text, window, cx));
            return;
        }
        let text = field.read(cx).value();
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let before = state_text(session.resolution(chunk));
        if !session.edit(chunk, &text) {
            return;
        }
        if state_text(session.resolution(chunk)) != before {
            self.refresh_card(chunk);
            field.update(cx, |field, cx| set_field_look(field, true, window, cx));
        }
        self.schedule_preview(cx);
        cx.notify();
    }

    /// The placeholder of every result field made so far, in the current language. The text in a
    /// field is the person's, and stays.
    pub(super) fn relabel_result_fields(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        for (chunk, field) in self.fields_made() {
            let resolved = session.resolution(chunk).is_some();
            field.update(cx, |field, cx| {
                field.set_placeholder(field_placeholder(resolved), window, cx)
            });
        }
    }

    /// The text in the result field of `chunk`, once its card was drawn.
    pub fn field_text(&self, chunk: usize, cx: &App) -> Option<String> {
        self.field(chunk)
            .map(|field| field.read(cx).value().to_string())
    }

    #[doc(hidden)]
    pub fn field_for_test(&self, chunk: usize) -> Option<Entity<TextareaState>> {
        self.field(chunk).cloned()
    }

    #[doc(hidden)]
    pub fn menu_target_for_test(&self) -> Option<usize> {
        self.menu_target()
    }

    /// The bar under a card's columns (`js:449-458`): "Result", the five choices (the one in
    /// use is on), the reset, the result's size and the hint.
    pub(super) fn render_result_bar(
        &self,
        chunk: usize,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let resolution = self.session.as_ref().and_then(|s| s.resolution(chunk));
        let how = resolution.map(|r| r.how);
        let count = count_text(resolution);
        let choices = How::CHOICES
            .into_iter()
            .zip(CHOICE_IDS)
            .map(|(choice, id)| {
                ld_button((id, chunk), tr(choice.label()), p)
                    .ld_small()
                    .ld_on(how == Some(choice), p)
                    .ld_tooltip(tr(choice.tip()))
                    .ld_click(
                        true,
                        cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.resolve(chunk, choice, window, cx);
                        }),
                    )
            });
        h_flex()
            .flex_wrap()
            .items_center()
            .gap(px(5.))
            .px(px(10.))
            .pt(px(6.))
            .pb(px(5.))
            .border_t_1()
            .border_color(p.line)
            .child(
                div()
                    .mr(px(4.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(tr("Result")),
            )
            .children(choices)
            .child(
                ld_small_icon_button(
                    ("merge-reset", chunk),
                    icons::RESET,
                    tr("Reset this part to its default"),
                    p,
                )
                .ld_click(
                    true,
                    cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.reset(chunk, window, cx)
                    }),
                ),
            )
            .child(
                div()
                    .id(("merge-count", chunk))
                    .test_support()
                    .aria_label(count.clone())
                    .ml(px(6.))
                    .text_size(px(11.5))
                    .text_color(p.muted)
                    .child(count),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(p.dim)
                    .child(tr("Typing here = edited")),
            )
    }

    /// The result field under the bar, in the code font (`css:142-150`): an open conflict's
    /// field is dashed in the deleted colour, a focused one has the accent border.
    pub(super) fn render_result_field(
        &self,
        chunk: usize,
        field: &Entity<TextareaState>,
        p: &Palette,
        fonts: &Fonts,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        let resolved = self
            .session
            .as_ref()
            .is_some_and(|s| s.resolution(chunk).is_some());
        let focused = field.focus_handle(cx).is_focused(window);
        div()
            .px(px(10.))
            .pb(px(8.))
            .child(
                div()
                    .id(("merge-result", chunk))
                    .test_support()
                    .key_context(TEXT_CONTEXT)
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .border_1()
                    .font(fonts.code.clone())
                    .when(resolved, |d| {
                        d.bg(p.bg)
                            .border_color(if focused { p.accent } else { p.line })
                    })
                    .when(!resolved, |d| {
                        d.bg(p.bg_del).border_color(p.del).border_dashed()
                    })
                    .child(
                        ld_textarea(field)
                            .appearance(false)
                            .text_size(px(CODE_SIZE)),
                    ),
            )
            .into_any_element()
    }
}
