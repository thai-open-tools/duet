//! Editing in the text view: the caret, typing and the IME, undo, the clipboard and saving.
//! An `impl TextView` split out of view.rs so the view itself stays readable.

use std::ops::Range;
use std::time::{Duration, Instant};

use duet_core::text::buffer::{Change, TextBuffer};
use gpui_kit::{
    Bounds, ClipboardItem, Context, EntityInputHandler, Pixels, Point, ScrollStrategy,
    UTF16Selection, Window, px, size,
};

use crate::text::caret::{Caret, Motion, moved};
use crate::text::display::columns;
use crate::text::job::{TextOutcome, rediff};
use crate::text::model::TextModel;
use crate::text::syntax::SyntaxState;
use crate::text::view::rows::HalfText;
use crate::text::view::{
    Backspace, CopyText, CutText, DeleteForward, InsertTab, LineEnd, Newline, PasteText, Redo,
    SelectAll, SelectDown, SelectEnd, SelectHome, SelectLeft, SelectPageDown, SelectPageUp,
    SelectRight, SelectUp, SelectWordLeft, SelectWordRight, TextView, Undo, WordLeft, WordRight,
};

use crate::i18n::{tr, tr_key};

/// How long the re-diff waits after the last edit.
const REDIFF_DELAY: Duration = Duration::from_millis(150);

/// Why an edit did nothing while a compare runs.
pub(crate) const WAIT_NOTICE: &str = tr_key("Wait for the compare to finish.");

/// Per-keystroke UI-thread times from the typing probe, in milliseconds.
#[derive(Debug, Default)]
pub struct TypingStats {
    /// Edit plus the frame after it.
    pub samples: Vec<f64>,
    /// The `edit` call alone, one per sample.
    pub edits: Vec<f64>,
}

impl TypingStats {
    pub fn mean(&self) -> f64 {
        if self.samples.is_empty() {
            0.0
        } else {
            self.samples.iter().sum::<f64>() / self.samples.len() as f64
        }
    }

    pub fn max(&self) -> f64 {
        self.samples.iter().copied().fold(0.0, f64::max)
    }

    /// The first keystroke's edit alone.
    pub fn first_edit(&self) -> f64 {
        self.edits.first().copied().unwrap_or(0.0)
    }

    /// The frame after the first keystroke (its sample less its edit).
    pub fn first_frame(&self) -> f64 {
        self.samples.first().map_or(0.0, |s| s - self.first_edit())
    }

    /// Mean of the edit call alone over every keystroke after the first.
    pub fn steady_edit_mean(&self) -> f64 {
        let rest = self.edits.get(1..).unwrap_or_default();
        if rest.is_empty() {
            0.0
        } else {
            rest.iter().sum::<f64>() / rest.len() as f64
        }
    }

    pub fn steady_edit_max(&self) -> f64 {
        self.edits
            .get(1..)
            .unwrap_or_default()
            .iter()
            .copied()
            .fold(0.0, f64::max)
    }

    /// Index of the slowest sample (the first one on a tie).
    pub fn max_at(&self) -> usize {
        let mut at = 0;
        for (i, s) in self.samples.iter().enumerate() {
            if *s > self.samples[at] {
                at = i;
            }
        }
        at
    }

    /// Mean over every sample after the first: the first frame lays out the screen the
    /// caret jumped to, which is not a keystroke cost.
    pub fn steady_mean(&self) -> f64 {
        let rest = self.samples.get(1..).unwrap_or_default();
        if rest.is_empty() {
            0.0
        } else {
            rest.iter().sum::<f64>() / rest.len() as f64
        }
    }

    pub fn steady_max(&self) -> f64 {
        self.samples
            .get(1..)
            .unwrap_or_default()
            .iter()
            .copied()
            .fold(0.0, f64::max)
    }
}

impl TextView {
    /// `DUET_TYPE_PROBE`: types `count` characters into the middle of the right side (with
    /// `DUET_TYPE_PROBE_AT=end`, at the end of its last line; with `start`, at the start of its
    /// first), one per frame, timing each edit and the frame after it; logs the result and
    /// quits.
    pub(crate) fn run_type_probe(
        &mut self,
        count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(model) = &mut self.model else { return };
        if model.begin_edit(false).is_err() {
            tracing::warn!("typing probe: right side is read-only");
            return cx.quit();
        }
        let buffer = model.buffer(false).expect("begun");
        let at = match std::env::var("DUET_TYPE_PROBE_AT").as_deref() {
            Ok("end") => buffer.line_end(buffer.line_count().saturating_sub(1)),
            Ok("start") => 0,
            _ => buffer.line_start(buffer.line_count() / 2),
        };
        self.caret = Some(Caret::at(false, at));
        self.probe_step(count, TypingStats::default(), window, cx);
    }

    fn probe_step(
        &mut self,
        left: usize,
        mut stats: TypingStats,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if left == 0 {
            tracing::info!(
                n = stats.samples.len(),
                mean_ms = stats.mean(),
                max_ms = stats.max(),
                max_at = stats.max_at(),
                first_ms = stats.samples.first().copied().unwrap_or(0.0),
                steady_mean_ms = stats.steady_mean(),
                steady_max_ms = stats.steady_max(),
                first_edit_ms = stats.first_edit(),
                first_frame_ms = stats.first_frame(),
                steady_edit_mean_ms = stats.steady_edit_mean(),
                steady_edit_max_ms = stats.steady_edit_max(),
                "typing"
            );
            return self.probe_rediff(cx);
        }
        let started = std::time::Instant::now();
        if let Some(caret) = self.caret {
            self.edit(caret.range(), "x", cx);
        }
        stats.edits.push(started.elapsed().as_secs_f64() * 1000.0);
        let entity = cx.entity();
        window.on_next_frame(move |window, cx| {
            stats.samples.push(started.elapsed().as_secs_f64() * 1000.0);
            entity.update(cx, |view, cx| view.probe_step(left - 1, stats, window, cx));
        });
    }

    /// After the probe's last key: waits for the re-diff that the pause starts, logs what it
    /// cost and what the find bar's copy of the edited text would cost (the probe's find bar is
    /// closed), and quits.
    fn probe_rediff(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(5))
                    .await;
                let landed = this.update(cx, |view, cx| {
                    if view.rediff_job.is_running() {
                        return false;
                    }
                    let times = view.last_rediff.unwrap_or_default();
                    let find_copy_ms = view.model.as_ref().map_or(0.0, |model| {
                        let started = Instant::now();
                        drop(model.sources());
                        ms(started.elapsed())
                    });
                    tracing::info!(
                        snapshot_ms = times.snapshot_ms,
                        diff_ms = times.diff_ms,
                        land_ms = times.land_ms,
                        find_copy_ms,
                        "rediff"
                    );
                    cx.quit();
                    true
                });
                if !matches!(landed, Ok(false)) {
                    break;
                }
            }
        })
        .detach();
    }
}

/// What the last re-diff cost, for the typing probe: the snapshot (copying the edited text) and
/// the landing run on the UI thread; the diff, after indexing the copied text's lines, in the
/// background.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RediffTimes {
    pub snapshot_ms: f64,
    pub diff_ms: f64,
    pub land_ms: f64,
}

fn ms(elapsed: Duration) -> f64 {
    elapsed.as_secs_f64() * 1000.0
}

/// Bytes of `s` up to UTF-16 offset `u16`, clamped to `s`.
pub(crate) fn utf16_to_byte(s: &str, u16: usize) -> usize {
    let mut units = 0;
    for (i, ch) in s.char_indices() {
        if units >= u16 {
            return i;
        }
        units += ch.len_utf16();
    }
    s.len()
}

/// The UTF-16 offset of byte `byte` of `s`, clamped to `s`; a byte inside a character counts
/// that whole character, so no offset can panic.
pub(crate) fn byte_to_utf16(s: &str, byte: usize) -> usize {
    s.char_indices()
        .take_while(|&(i, _)| i < byte)
        .map(|(_, ch)| ch.len_utf16())
        .sum()
}

/// The caret to keep after a new compare result: on the same side if that side can still be
/// edited and the caret still falls on character boundaries inside its text.
pub(crate) fn carried_caret(model: &mut TextModel, caret: Caret) -> Option<Caret> {
    model.begin_edit(caret.left).ok()?;
    let text = model.buffer(caret.left)?.text();
    // False past the end, too.
    (text.is_char_boundary(caret.head) && text.is_char_boundary(caret.anchor)).then_some(caret)
}

impl TextView {
    pub fn caret(&self) -> Option<Caret> {
        self.caret
    }

    /// Puts the caret where the IME did not. A composition in progress comes out of the text
    /// first, as Esc does: the IME still holds it and sends it again at the new caret, so it
    /// shows once, there (gpui gives no way to make the IME commit it in place). On the
    /// composition's side, an offset past it shifts left by its length and one inside it
    /// lands at its start; the other side's offsets stay.
    pub(crate) fn place_caret(&mut self, mut caret: Caret, cx: &mut Context<Self>) {
        self.row_sel = None;
        self.menu_row = None;
        if let (Some(composing), Some(old)) = (self.composing.take(), self.caret)
            && self.edit(composing.clone(), "", cx)
        {
            if old.left == caret.left {
                let shift = |offset: usize| {
                    if offset >= composing.end {
                        offset - (composing.end - composing.start)
                    } else {
                        offset.min(composing.start)
                    }
                };
                caret.head = shift(caret.head);
                caret.anchor = shift(caret.anchor);
            }
            self.caret = Some(caret);
            // The removal scrolled to where the composition was.
            self.ensure_caret_visible(cx);
        } else {
            self.caret = Some(caret);
        }
        // After the composition's removal, so that folds into the typing step it ends.
        self.undo.seal();
    }

    #[doc(hidden)]
    pub fn set_caret_for_test(&mut self, left: bool, offset: usize, cx: &mut Context<Self>) {
        if let Some(model) = &mut self.model
            && model.begin_edit(left).is_ok()
        {
            self.place_caret(Caret::at(left, offset), cx);
            cx.notify();
        }
    }

    /// A click (or a drag, with `extend`) at `position` in the half for `left` of `display`.
    /// On a side that cannot be edited, the notice says why and no caret is placed.
    pub fn click_text(
        &mut self,
        display: usize,
        left: bool,
        position: Point<Pixels>,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hit = self
            .layouts
            .borrow()
            .get(&(display, left))
            .and_then(|half| {
                let (t, position) = half.under(position)?;
                let ix = t
                    .layout
                    .index_for_position(position)
                    .unwrap_or_else(|nearest| nearest);
                Some((t.line, t.shown.source_at(ix)))
            });
        let Some((line, byte)) = hit else { return };
        let Some(model) = &mut self.model else { return };
        if let Err(reason) = model.begin_edit(left) {
            self.notice = Some(reason.into());
            cx.notify();
            return;
        }
        let buffer = model.buffer(left).expect("begun");
        // The hit is from the last frame's layout: an edit since can put it inside a character.
        let mut offset =
            (buffer.line_start(line as usize) + byte).min(buffer.line_end(line as usize));
        while !buffer.text().is_char_boundary(offset) {
            offset -= 1;
        }
        let caret = match self.caret {
            Some(c) if extend && c.left == left => Caret {
                head: offset,
                goal: None,
                ..c
            },
            _ => Caret::at(left, offset),
        };
        self.place_caret(caret, cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Moves the caret; returns false when there is none (the key then scrolls instead).
    pub fn move_caret(&mut self, motion: Motion, extend: bool, cx: &mut Context<Self>) -> bool {
        let (Some(caret), Some(model)) = (self.caret, self.model.as_ref()) else {
            return false;
        };
        let Some(buffer) = model.buffer(caret.left) else {
            return false;
        };
        let caret = moved(buffer, caret, motion, extend);
        self.place_caret(caret, cx);
        self.ensure_caret_visible(cx);
        cx.notify();
        true
    }

    /// Scrolls so the caret's row and column are on screen.
    pub fn ensure_caret_visible(&mut self, cx: &mut Context<Self>) {
        let (Some(caret), Some(model)) = (self.caret, self.model.as_mut()) else {
            return;
        };
        let Some(buffer) = model.buffer(caret.left) else {
            return;
        };
        let line = buffer.line_of(caret.head);
        let start = buffer.line_start(line);
        let column = columns(
            &buffer.text()[start..caret.head.max(start)],
            self.display.tab_width,
        );
        // The empty line after a final break has no row; show the last line's.
        let line = line.min(model.line_count(caret.left).saturating_sub(1)) as u32;
        if let Some(display) = model
            .row_of_line(caret.left, line)
            .and_then(|row| model.reveal_row(row))
        {
            self.scroll_to_display(display, ScrollStrategy::Nearest);
        }
        // Before the first frame the columns are unknown; leave the scroll alone. A wrapped
        // line never scrolls sideways (R3).
        let shown = self.display.view_columns;
        if shown > 0 && !self.wrap {
            let margin = 4.min(shown / 2);
            let skip = &mut self.display.skip_columns;
            if column < *skip {
                *skip = column.saturating_sub(margin);
            } else if column >= *skip + shown {
                *skip = column + margin - shown;
            }
        }
        cx.notify();
    }

    // Motions that only mean something with a caret (without one, they do nothing).

    pub(crate) fn line_end(&mut self, _: &LineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_caret(Motion::End, false, cx);
    }

    pub(crate) fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_caret(Motion::Left, true, cx);
    }

    pub(crate) fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_caret(Motion::Right, true, cx);
    }

    pub(crate) fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        if !self.move_caret(Motion::Up, true, cx) {
            self.move_row_selection(-1, true, cx);
        }
    }

    pub(crate) fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        if !self.move_caret(Motion::Down, true, cx) {
            self.move_row_selection(1, true, cx);
        }
    }

    pub(crate) fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.move_caret(Motion::Home, true, cx);
    }

    pub(crate) fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_caret(Motion::End, true, cx);
    }

    pub(crate) fn select_page_up(
        &mut self,
        _: &SelectPageUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_caret(Motion::PageUp(self.page_rows() as u32), true, cx);
    }

    pub(crate) fn select_page_down(
        &mut self,
        _: &SelectPageDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_caret(Motion::PageDown(self.page_rows() as u32), true, cx);
    }

    pub(crate) fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_caret(Motion::WordLeft, false, cx);
    }

    pub(crate) fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_caret(Motion::WordRight, false, cx);
    }

    pub(crate) fn select_word_left(
        &mut self,
        _: &SelectWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_caret(Motion::WordLeft, true, cx);
    }

    pub(crate) fn select_word_right(
        &mut self,
        _: &SelectWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_caret(Motion::WordRight, true, cx);
    }
}

// Typing, deleting and the re-diff that follows.
impl TextView {
    /// Changes one side's buffer: the provisional rows, the syntax re-parse, then the re-diff
    /// schedule. Every buffer change (typing, undo, redo) goes through here, and it ends an IME
    /// composition (the IME marks its text again after its own edits).
    pub(crate) fn apply_change(
        &mut self,
        left: bool,
        range: Range<usize>,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Result<Change, String> {
        let model = self
            .model
            .as_mut()
            .ok_or_else(|| String::from(tr("Nothing is loaded.")))?;
        let change = model.replace(left, range, text)?;
        self.line_widths.borrow_mut().edited(left, &change);
        self.drop_stale_anchors();
        // Past the budget the side only shifts its styles and a full parse runs off the thread.
        // One already running sees a newer version when it lands and starts again, so typing
        // starts one parse per burst. A lexed side waits for a pause in typing (PF10).
        let index = usize::from(!left);
        if let Some(side) = &mut self.syntax[index]
            && !side.edit(&change, text)
        {
            if side.is_lexed() {
                self.schedule_relex(left, cx);
            } else if self.syntax_state[index] != SyntaxState::Parsing {
                self.parse_syntax(left, cx);
            }
        }
        self.composing = None;
        self.schedule_rediff(cx);
        Ok(change)
    }

    /// A compare that is running replaces the buffers when it lands, so an edit made now would
    /// vanish: it is refused, with a notice.
    pub(crate) fn refused_while_loading(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.job.is_running() {
            return false;
        }
        self.notice = Some(tr(WAIT_NOTICE).into());
        cx.notify();
        true
    }

    /// Copies change block `block` onto the target side (`to_left`) as one undo step.
    pub fn copy_block(&mut self, block: usize, to_left: bool, cx: &mut Context<Self>) -> bool {
        let edit = self
            .model
            .as_mut()
            .map(|m| m.copy_block_edit(block, to_left));
        self.apply_copy(edit, to_left, cx)
    }

    /// Replaces the whole target side with the other side, as one undo step.
    pub fn copy_all(&mut self, to_left: bool, cx: &mut Context<Self>) -> bool {
        let edit = self.model.as_mut().map(|m| m.copy_all_edit(to_left));
        self.apply_copy(edit, to_left, cx)
    }

    pub(crate) fn apply_copy(
        &mut self,
        edit: Option<Result<(Range<usize>, String), String>>,
        to_left: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        match edit {
            None => false,
            Some(Err(reason)) => {
                self.notice = Some(reason.into());
                cx.notify();
                false
            }
            Some(Ok((range, text))) => {
                self.undo.seal();
                let done = self.replace_on(to_left, range, &text, cx);
                self.undo.seal();
                done
            }
        }
    }

    /// Alt+arrow: copies the current block, or says there is none.
    pub(crate) fn copy_current(&mut self, to_left: bool, cx: &mut Context<Self>) {
        if let Some(sel) = self.row_sel {
            self.copy_rows(sel.rows(), to_left, cx);
            return;
        }
        match self.model.as_ref().and_then(TextModel::current_block) {
            Some(block) => {
                self.copy_block(block, to_left, cx);
            }
            None => {
                self.notice = Some(tr("Pick a difference first (F7/F8).").into());
                cx.notify();
            }
        }
    }

    /// A find, go-to, rules or path field is open and has the focus, not the rows.
    pub(crate) fn field_has_keys(&self, window: &Window) -> bool {
        (self.find.is_some()
            || self.goto.is_some()
            || self.rules.is_some()
            || self.path_edit.is_some())
            && !self.focus.is_focused(window)
    }

    pub(crate) fn on_copy_block(&mut self, to_left: bool, window: &Window, cx: &mut Context<Self>) {
        // A find, go-to or path field that has the focus keeps its own keys.
        if self.field_has_keys(window) {
            cx.propagate();
        } else {
            self.menu_row = None;
            self.copy_current(to_left, cx);
        }
    }

    #[doc(hidden)]
    pub fn clear_current_block_for_test(&mut self, cx: &mut Context<Self>) {
        if let Some(model) = self.model.as_mut() {
            model.clear_current_block();
        }
        cx.notify();
    }

    /// Replaces `range` of the caret's side with `text`.
    pub fn edit(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) -> bool {
        match self.caret {
            Some(caret) => self.replace_on(caret.left, range, text, cx),
            None => {
                self.composing = None;
                false
            }
        }
    }

    pub(crate) fn buffer_and_caret(&self) -> Option<(&TextBuffer, Caret)> {
        let caret = self.caret?;
        Some((self.model.as_ref()?.buffer(caret.left)?, caret))
    }

    /// The selection, else one code point back: a Thai tone mark goes without its consonant.
    pub(crate) fn backspace(&mut self, cx: &mut Context<Self>) {
        let Some((buffer, caret)) = self.buffer_and_caret() else {
            return;
        };
        let range = caret
            .selection()
            .unwrap_or_else(|| buffer.prev_char(caret.head)..caret.head);
        if !range.is_empty() {
            self.edit(range, "", cx);
        }
    }

    /// The selection, else the whole grapheme after the caret.
    pub(crate) fn delete_forward(&mut self, cx: &mut Context<Self>) {
        let Some((buffer, caret)) = self.buffer_and_caret() else {
            return;
        };
        let range = caret
            .selection()
            .unwrap_or_else(|| caret.head..buffer.next_grapheme(caret.head).min(buffer.len()));
        if !range.is_empty() {
            self.edit(range, "", cx);
        }
    }

    /// A line break like the one the caret's line already has, so a CRLF file stays CRLF.
    pub(crate) fn newline(&mut self, cx: &mut Context<Self>) {
        let Some((buffer, caret)) = self.buffer_and_caret() else {
            return;
        };
        let terminator = buffer.terminator(buffer.line_of(caret.head));
        self.edit(caret.range(), terminator, cx);
    }

    pub(crate) fn insert_tab(&mut self, cx: &mut Context<Self>) {
        if let Some(caret) = self.caret {
            self.edit(caret.range(), "\t", cx);
        }
    }

    /// Whether an editing key is for the text: a caret is placed and the rows have the focus.
    /// A find or go-to field passes Enter, Tab and Backspace at its start up to the view;
    /// those keys go on up as before and never edit the text.
    fn keys_edit_text(&self, window: &Window) -> bool {
        self.caret.is_some() && self.focus.is_focused(window)
    }

    pub(crate) fn on_backspace(
        &mut self,
        _: &Backspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.rows_have_keys(window) {
            self.delete_selected_rows(cx);
        } else if self.keys_edit_text(window) {
            self.backspace(cx);
        } else {
            cx.propagate();
        }
    }

    pub(crate) fn on_delete_forward(
        &mut self,
        _: &DeleteForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.rows_have_keys(window) {
            self.delete_selected_rows(cx);
        } else if self.keys_edit_text(window) {
            self.delete_forward(cx);
        } else {
            cx.propagate();
        }
    }

    pub(crate) fn on_newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        if self.keys_edit_text(window) {
            self.newline(cx);
        } else {
            cx.propagate();
        }
    }

    pub(crate) fn on_insert_tab(
        &mut self,
        _: &InsertTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.keys_edit_text(window) {
            self.insert_tab(cx);
        } else {
            cx.propagate();
        }
    }

    /// Whether the buffer holds `text` at `range`, so an undo entry still applies.
    fn buffer_holds(&mut self, left: bool, range: Range<usize>, text: &str) -> bool {
        self.model.as_mut().is_some_and(|model| {
            model.begin_edit(left).is_ok()
                && model
                    .buffer(left)
                    .and_then(|b| b.text().get(range))
                    .is_some_and(|held| held == text)
        })
    }

    /// The text no longer matches the history (a reload brought in a changed file).
    fn forget_history(&mut self, cx: &mut Context<Self>) {
        self.undo.clear();
        self.notice =
            Some(tr("The text changed since that edit, so the undo history was cleared.").into());
        cx.notify();
    }

    pub(crate) fn undo_edit(&mut self, cx: &mut Context<Self>) {
        if self.refused_while_loading(cx) {
            return;
        }
        let Some(edit) = self.undo.undo() else { return };
        if !self.buffer_holds(edit.left, edit.applied_range(), &edit.inserted) {
            self.forget_history(cx);
            return;
        }
        if self
            .apply_change(edit.left, edit.applied_range(), &edit.removed, cx)
            .is_ok()
        {
            self.caret = Some(Caret::at(edit.left, edit.caret_before));
            self.row_sel = None;
            self.menu_row = None;
            self.ensure_caret_visible(cx);
        }
        cx.notify();
    }

    pub(crate) fn redo_edit(&mut self, cx: &mut Context<Self>) {
        if self.refused_while_loading(cx) {
            return;
        }
        let Some(edit) = self.undo.redo() else { return };
        let range = edit.at..edit.at + edit.removed.len();
        if !self.buffer_holds(edit.left, range.clone(), &edit.removed) {
            self.forget_history(cx);
            return;
        }
        if self
            .apply_change(edit.left, range, &edit.inserted, cx)
            .is_ok()
        {
            self.caret = Some(Caret::at(edit.left, edit.at + edit.inserted.len()));
            self.row_sel = None;
            self.menu_row = None;
            self.ensure_caret_visible(cx);
        }
        cx.notify();
    }

    pub(crate) fn selected_text(&self) -> Option<String> {
        let (buffer, caret) = self.buffer_and_caret()?;
        caret
            .selection()
            .and_then(|r| buffer.text().get(r))
            .map(str::to_owned)
    }

    pub(crate) fn copy_text(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub(crate) fn cut_text(&mut self, cx: &mut Context<Self>) {
        let Some(range) = self.caret.and_then(|c| c.selection()) else {
            return;
        };
        self.copy_text(cx);
        self.undo.seal();
        self.edit(range, "", cx);
        self.undo.seal();
    }

    pub(crate) fn paste_text(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let Some((buffer, caret)) = self.buffer_and_caret() else {
            return;
        };
        let terminator = buffer.terminator(buffer.line_of(caret.head));
        let text = normalize_breaks(&text, terminator);
        self.undo.seal();
        self.edit(caret.range(), &text, cx);
        self.undo.seal();
    }

    pub(crate) fn select_all(&mut self, cx: &mut Context<Self>) {
        self.menu_row = None;
        self.row_sel = None;
        let Some((buffer, caret)) = self.buffer_and_caret() else {
            return;
        };
        self.caret = Some(Caret {
            head: buffer.len(),
            anchor: 0,
            goal: None,
            ..caret
        });
        cx.notify();
    }

    pub(crate) fn on_undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        if self.keys_edit_text(window) {
            self.undo_edit(cx);
        } else {
            cx.propagate();
        }
    }

    pub(crate) fn on_redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        if self.keys_edit_text(window) {
            self.redo_edit(cx);
        } else {
            cx.propagate();
        }
    }

    pub(crate) fn on_copy(&mut self, _: &CopyText, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows_have_keys(window) {
            self.menu_row = None;
            self.copy_selected_rows(cx);
        } else if self.keys_edit_text(window) {
            self.menu_row = None;
            self.copy_text(cx);
        } else {
            cx.propagate();
        }
    }

    pub(crate) fn on_cut(&mut self, _: &CutText, window: &mut Window, cx: &mut Context<Self>) {
        if self.keys_edit_text(window) {
            self.cut_text(cx);
        } else {
            cx.propagate();
        }
    }

    pub(crate) fn on_paste(&mut self, _: &PasteText, window: &mut Window, cx: &mut Context<Self>) {
        // Two empty sides show only the hint: a paste goes to the left one.
        if self.caret.is_none() && self.is_empty_pair() {
            self.click_empty_side(true, window, cx);
        }
        if self.keys_edit_text(window) {
            self.paste_text(cx);
        } else {
            cx.propagate();
        }
    }

    pub(crate) fn on_select_all(
        &mut self,
        _: &SelectAll,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.keys_edit_text(window) {
            self.select_all(cx);
        } else if self.rows_have_keys(window) {
            self.select_all_rows(cx);
        } else {
            cx.propagate();
        }
    }

    /// Re-diffs the buffers `REDIFF_DELAY` after the last call; a newer call or edit wins.
    pub(crate) fn schedule_rediff(&mut self, cx: &mut Context<Self>) {
        let (generation, cancel) = self.rediff_job.start();
        let request = self.request.clone();
        self.rediff_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REDIFF_DELAY).await;
            let started = Instant::now();
            let Ok(Some(snapshot)) = this.update(cx, |view, _| {
                view.model.as_ref().map(TextModel::begin_snapshot)
            }) else {
                return;
            };
            let versions = snapshot.versions();
            let snapshot_ms = ms(started.elapsed());
            let (outcome, diff_ms) = cx
                .background_executor()
                .spawn(async move {
                    let started = Instant::now();
                    let (left, right, _) = snapshot.finish();
                    let outcome = rediff(left, right, &request, &cancel);
                    (outcome, ms(started.elapsed()))
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                let started = Instant::now();
                view.apply_rediff(generation, outcome, versions, cx);
                view.last_rediff = Some(RediffTimes {
                    snapshot_ms,
                    diff_ms,
                    land_ms: ms(started.elapsed()),
                });
            });
        }));
    }

    fn apply_rediff(
        &mut self,
        generation: u64,
        outcome: TextOutcome,
        versions: [Option<u64>; 2],
        cx: &mut Context<Self>,
    ) {
        if !self.rediff_job.is_current(generation) {
            return;
        }
        self.rediff_job.finish(generation);
        cx.notify();
        let TextOutcome::Ready(fresh) = outcome else {
            return;
        };
        let mut fresh = *fresh;
        let Some(old) = self.model.as_mut() else {
            return;
        };
        // False when the buffers changed since the snapshot: the provisional rows stay.
        if fresh.take_edits_from(old, versions) {
            self.model = Some(fresh);
            // Fewer rows clamp the list's top: not a scroll to follow (C1, M12).
            self.note_programmatic_scroll();
            self.menu_row = None;
            self.row_sel = None;
            self.check_wrap_limit();
            self.run_find(cx);
            self.rescan_stale_odd(cx);
        }
    }

    /// Forgets what points into the buffers a new compare replaces: the pending re-diff, the
    /// IME composition and a Replace all or Reformat still being planned (C1).
    pub(crate) fn drop_buffer_state(&mut self) {
        self.rediff_job.cancel();
        self.rediff_task = None;
        self.composing = None;
        self.model_epoch += 1;
        self.stop_plans();
    }

    #[doc(hidden)]
    pub fn rediff_generation(&self) -> u64 {
        self.rediff_job.generation()
    }

    /// Test hook: deliver a re-diff result as if re-diff `generation` had finished.
    #[doc(hidden)]
    pub fn apply_rediff_for_test(
        &mut self,
        generation: u64,
        outcome: TextOutcome,
        versions: [Option<u64>; 2],
        cx: &mut Context<Self>,
    ) {
        self.apply_rediff(generation, outcome, versions, cx);
    }
}

// The IME sees the caret's line as its text field; offsets it passes are UTF-16 within it.
impl TextView {
    /// The caret's line without its break, and where it starts in the buffer.
    fn caret_line(&self) -> Option<(String, usize)> {
        let (buffer, caret) = self.buffer_and_caret()?;
        let line = buffer.line_of(caret.head);
        Some((buffer.line(line).to_owned(), buffer.line_start(line)))
    }

    /// The buffer range an IME call means: its own range, else the composition, else the
    /// selection or caret.
    fn input_range(&self, range: Option<Range<usize>>) -> Option<Range<usize>> {
        let caret = self.caret?;
        Some(match range {
            Some(r) => {
                let (line, start) = self.caret_line()?;
                let (a, b) = (utf16_to_byte(&line, r.start), utf16_to_byte(&line, r.end));
                start + a.min(b)..start + a.max(b)
            }
            None => self.composing.clone().unwrap_or_else(|| caret.range()),
        })
    }

    /// `f` on what the caret's line drew on the caret's side last frame; None off screen.
    fn with_caret_row<R>(&self, f: impl FnOnce(&HalfText) -> Option<R>) -> Option<R> {
        let (buffer, caret) = self.buffer_and_caret()?;
        let line = buffer.line_of(caret.head);
        let layouts = self.layouts.borrow();
        let (_, row) = layouts.iter().find(|((_, left), t)| {
            *left == caret.left && t.line().is_some_and(|l| l as usize == line)
        })?;
        f(row)
    }
}

impl EntityInputHandler for TextView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let (line, _) = self.caret_line()?;
        let (a, b) = (
            utf16_to_byte(&line, range.start),
            utf16_to_byte(&line, range.end),
        );
        *adjusted = Some(byte_to_utf16(&line, a)..byte_to_utf16(&line, b));
        line.get(a..b).map(str::to_owned)
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let caret = self.caret?;
        let (line, start) = self.caret_line()?;
        let clamp = |o: usize| o.clamp(start, start + line.len()) - start;
        let (anchor, head) = (clamp(caret.anchor), clamp(caret.head));
        Some(UTF16Selection {
            range: byte_to_utf16(&line, anchor.min(head))..byte_to_utf16(&line, anchor.max(head)),
            reversed: head < anchor,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let composing = self.composing.clone()?;
        let (line, start) = self.caret_line()?;
        let rel = |o: usize| byte_to_utf16(&line, o.saturating_sub(start));
        Some(rel(composing.start)..rel(composing.end))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.composing = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(target) = self.input_range(range) {
            self.edit(target, text, cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.input_range(range) else {
            return;
        };
        if !self.edit(target.clone(), text, cx) {
            return;
        }
        self.composing = (!text.is_empty()).then(|| target.start..target.start + text.len());
        if let (Some(sel), Some(caret)) = (selected, self.caret) {
            self.caret = Some(Caret {
                head: target.start + utf16_to_byte(text, sel.end),
                anchor: target.start + utf16_to_byte(text, sel.start),
                goal: None,
                ..caret
            });
        }
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let (line, _) = self.caret_line()?;
        let byte = utf16_to_byte(&line, range.start);
        let row_height = px(self.row_height());
        self.with_caret_row(|half| {
            let (row, at) = half.holding(byte)?;
            let at = row.layout.position_for_index(at)?;
            Some(Bounds::new(at, size(px(2.), row_height)))
        })
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let (line, _) = self.caret_line()?;
        self.with_caret_row(|half| {
            let (row, point) = half.under(point)?;
            let ix = row
                .layout
                .index_for_position(point)
                .unwrap_or_else(|nearest| nearest);
            Some(byte_to_utf16(&line, row.shown.source_at(ix)))
        })
    }
}

/// `text` with every line break (CRLF, CR, LF) turned into `terminator`.
pub(crate) fn normalize_breaks(text: &str, terminator: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str(terminator);
            }
            '\n' => out.push_str(terminator),
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_expand_tabs_and_count_thai_clusters_once() {
        assert_eq!(columns("", 4), 0);
        assert_eq!(columns("a\t", 4), 4);
        assert_eq!(columns("a\tก่b", 4), 6);
        assert_eq!(columns(&"x".repeat(9_000), 4), 9_000, "never cut");
    }

    #[test]
    fn ime_offsets_convert_through_thai_and_surrogate_pairs() {
        // UTF-16 units / bytes: a 1/1, ก 1/3, ่ 1/3, 😀 2/4, b 1/1.
        let s = "aก่😀b";
        for (units, bytes) in [(0, 0), (1, 1), (3, 7), (5, 11), (6, 12)] {
            assert_eq!(utf16_to_byte(s, units), bytes);
            assert_eq!(byte_to_utf16(s, bytes), units);
        }
        assert_eq!(utf16_to_byte(s, 99), s.len(), "clamped to the text");
        assert_eq!(byte_to_utf16(s, 99), 6, "clamped to the text");
        assert_eq!(
            byte_to_utf16(s, 8),
            5,
            "inside 😀: no panic, the emoji counts whole"
        );
    }
}
