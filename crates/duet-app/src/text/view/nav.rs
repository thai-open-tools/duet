//! Moving through the text view: landing on a difference, the filters, and the scrolling keys.

use super::*;

impl TextView {
    pub(super) fn land(&mut self, landing: Option<Landing>, first: bool, cx: &mut Context<Self>) {
        self.notice = match landing {
            Some(Landing { wrapped: true, .. }) => Some(
                if first {
                    "Wrapped to the first difference."
                } else {
                    "Wrapped to the last difference."
                }
                .into(),
            ),
            Some(_) => None,
            None => Some("No differences to go to.".into()),
        };
        if let Some(landing) = landing {
            self.scroll_to_follow_line(landing.display);
        }
        cx.notify();
    }

    pub(super) fn jump(
        &mut self,
        cx: &mut Context<Self>,
        go: impl FnOnce(&mut TextModel) -> Option<usize>,
    ) {
        if let Some(display) = self.model.as_mut().and_then(go) {
            self.scroll_to_follow_line(display);
        }
        cx.notify();
    }

    pub(super) fn next_change(&mut self, _: &NextChange, _: &mut Window, cx: &mut Context<Self>) {
        let landing = self
            .model
            .as_mut()
            .and_then(TextModel::next_change_wrapping);
        self.land(landing, true, cx);
    }

    pub(super) fn prev_change(&mut self, _: &PrevChange, _: &mut Window, cx: &mut Context<Self>) {
        let landing = self
            .model
            .as_mut()
            .and_then(TextModel::prev_change_wrapping);
        self.land(landing, false, cx);
    }

    /// Bare `n` and `p` (R10): a placed caret or a field with the focus keeps the letter.
    pub(super) fn next_change_key(
        &mut self,
        _: &NextChangeKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.caret.is_some() || self.field_has_keys(window) {
            cx.propagate();
        } else {
            self.next_change(&NextChange, window, cx);
        }
    }

    pub(super) fn prev_change_key(
        &mut self,
        _: &PrevChangeKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.caret.is_some() || self.field_has_keys(window) {
            cx.propagate();
        } else {
            self.prev_change(&PrevChange, window, cx);
        }
    }

    /// Notes a scroll duet made itself: the next two frames do not move the current difference
    /// (R9, instead of LocalDiff's 250 ms timer).
    pub(crate) fn note_programmatic_scroll(&mut self) {
        self.follow_pause = FOLLOW_PAUSE;
    }

    /// Once a frame, before the rows are built: after a scroll the user made, the current
    /// difference becomes the last one at or above the 30% line (R9). A wrapped row scrolls a
    /// long way before the top row changes, so the scroll inside it counts too (X-38).
    pub(super) fn follow_scroll(&mut self, window: &mut Window) {
        let top = self.rows.scroll_mark(px(self.row_height()));
        let before = self.last_top.replace(top);
        if self.follow_pause > 0 {
            self.follow_pause -= 1;
            window.request_animation_frame();
            return;
        }
        if before.is_none_or(|b| b == top) {
            return;
        }
        let row_height = px(self.row_height());
        let line = self
            .rows
            .row_at_share(FOLLOW_LINE, row_height, &|row| self.wrapped_height(row));
        if let Some(model) = self.model.as_mut()
            && model.filter() != TextFilter::Same
            && model.row_count() > 0
            && let Some(block) = model.block_at_display(line)
        {
            model.set_current_block(Some(block));
        }
    }

    pub(super) fn first_change(&mut self, _: &FirstChange, _: &mut Window, cx: &mut Context<Self>) {
        self.jump(cx, TextModel::first_change);
    }

    pub(super) fn last_change(&mut self, _: &LastChange, _: &mut Window, cx: &mut Context<Self>) {
        self.jump(cx, TextModel::last_change);
    }

    pub(super) fn filter_all(&mut self, _: &FilterAll, _: &mut Window, cx: &mut Context<Self>) {
        self.set_filter(TextFilter::All, cx);
    }

    pub(super) fn filter_differences(
        &mut self,
        _: &FilterDifferences,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_filter(TextFilter::Differences, cx);
    }

    pub(super) fn filter_context(
        &mut self,
        _: &FilterContext,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_filter(TextFilter::Context(self.context_lines()), cx);
    }

    pub(super) fn filter_same(&mut self, _: &FilterSame, _: &mut Window, cx: &mut Context<Self>) {
        self.set_filter(TextFilter::Same, cx);
    }

    pub(super) fn toggle_whitespace(
        &mut self,
        _: &ToggleWhitespace,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_show_spaces(!self.display.show_whitespace, cx);
    }

    // Without a caret these scroll sideways, which a wrapped line never does (R3).

    pub(super) fn scroll_right(&mut self, _: &ScrollRight, _: &mut Window, cx: &mut Context<Self>) {
        if !self.move_caret(Motion::Right, false, cx) && !self.wrap {
            self.display.skip_columns += SCROLL_STEP;
            cx.notify();
        }
    }

    pub(super) fn scroll_left(&mut self, _: &ScrollLeft, _: &mut Window, cx: &mut Context<Self>) {
        if !self.move_caret(Motion::Left, false, cx) && !self.wrap {
            self.display.skip_columns = self.display.skip_columns.saturating_sub(SCROLL_STEP);
            cx.notify();
        }
    }

    pub(super) fn scroll_home(&mut self, _: &ScrollHome, _: &mut Window, cx: &mut Context<Self>) {
        if !self.move_caret(Motion::Home, false, cx) && !self.wrap {
            self.display.skip_columns = 0;
            cx.notify();
        }
    }

    pub(super) fn on_reload(&mut self, _: &Reload, window: &mut Window, cx: &mut Context<Self>) {
        self.reload_guarded(window, cx, |_| {});
    }

    pub(super) fn on_save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        self.save_shortcut(window, cx);
    }

    /// Ctrl+Shift+S: Save as… for the caret's side, the left one with no caret.
    pub(super) fn on_save_as(&mut self, _: &SaveAs, window: &mut Window, cx: &mut Context<Self>) {
        let left = self.caret.is_none_or(|c| c.left);
        if self.side_has_text(left) {
            self.save_as_dialog(left, window, cx);
        }
    }

    /// Esc clears a row selection, then cancels a running compare, then closes the bars, then gives the keys back to the
    /// rows from a toolbar control, then removes the IME's composed text, then drops the caret;
    /// with none of those it asks to go back to the folder tab.
    pub(super) fn on_escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        if self.path_edit.is_some() {
            self.cancel_path_edit(window, cx);
            return;
        }
        if self.move_drag.take().is_some() {
            cx.notify();
            return;
        }
        if self.align.take().is_some() {
            self.notice = None;
            cx.notify();
            return;
        }
        if self.row_sel.take().is_some() {
            cx.notify();
            return;
        }
        if self.job.is_running() {
            self.job.cancel();
            self.task = None;
            if self.model.is_none() {
                self.set_message("Compare cancelled.", CANCELLED_HINT);
                // A notice now is about a key pressed while the compare ran, such as the
                // wait notice; nothing runs any more.
                self.notice = None;
            } else {
                self.notice = Some("Reload cancelled; showing the previous result.".into());
                // The reload dropped any pending re-diff of the edited buffers, whose rows may
                // still be provisional.
                if self
                    .model
                    .as_ref()
                    .is_some_and(|m| m.buffer(true).is_some() || m.buffer(false).is_some())
                {
                    self.schedule_rediff(cx);
                }
            }
            cx.notify();
            return;
        }
        if self.find.is_some() || self.goto.is_some() || self.rules.is_some() {
            self.close_bars(window, cx);
            return;
        }
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        if let Some(composing) = self.composing.take() {
            self.edit(composing, "", cx);
            return;
        }
        if self.caret.is_some() {
            self.caret = None;
            cx.notify();
            return;
        }
        cx.emit(TextEvent::Back);
    }

    /// Scrolls by whole rows, keeping the result inside the list.
    pub fn scroll_rows(&mut self, delta: isize, cx: &mut Context<Self>) {
        let rows = self.model.as_ref().map_or(0, TextModel::row_count);
        if rows == 0 {
            return;
        }
        let top = self.top_row() as isize;
        let target = (top + delta).clamp(0, rows as isize - 1) as usize;
        self.rows.scroll_to_top_strict(target);
        cx.notify();
    }

    /// Rows in one screen, less one so a page step keeps a line of context. A wrapped row can
    /// be taller than a line, so there it is the rows wholly on screen.
    pub(crate) fn page_rows(&self) -> isize {
        let row_height = px(self.row_height());
        let rows = if self.wrap {
            self.rows.visible_rows(row_height).len() as isize
        } else {
            (self.rows.viewport_height() / row_height).floor() as isize
        };
        (rows - 1).max(1)
    }

    pub(super) fn row_up(&mut self, _: &RowUp, _: &mut Window, cx: &mut Context<Self>) {
        if !self.move_caret(Motion::Up, false, cx) && !self.move_row_selection(-1, false, cx) {
            self.scroll_rows(-1, cx);
        }
    }

    pub(super) fn row_down(&mut self, _: &RowDown, _: &mut Window, cx: &mut Context<Self>) {
        if !self.move_caret(Motion::Down, false, cx) && !self.move_row_selection(1, false, cx) {
            self.scroll_rows(1, cx);
        }
    }

    pub(super) fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        let page = self.page_rows();
        if !self.move_caret(Motion::PageUp(page as u32), false, cx) {
            self.scroll_rows(-page, cx);
        }
    }

    pub(super) fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        let page = self.page_rows();
        if !self.move_caret(Motion::PageDown(page as u32), false, cx) {
            self.scroll_rows(page, cx);
        }
    }

    /// Test hook: scrolls to `top` as the wheel or the scrollbar does, which does not go
    /// through `scroll_to_display`.
    #[doc(hidden)]
    pub fn scroll_like_the_user_for_test(&mut self, top: usize, cx: &mut Context<Self>) {
        self.rows.scroll_to_top_strict(top);
        cx.notify();
    }
}
