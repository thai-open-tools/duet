//! Word wrap (R2, R3): the toggle, its row limit, and test hooks for the wrapped rows.

use super::*;
use crate::text::display::{MAX_WRAP_SEGMENTS, wrap_starts};

/// How long the row list's width must stay the same before the wrapped rows take it after a
/// resize (X-18).
const WRAP_SETTLE: Duration = Duration::from_millis(150);

impl TextView {
    pub fn wrap(&self) -> bool {
        self.wrap
    }

    /// Turns word wrap on or off; the top row stays on top. False, with R2's notice, when the
    /// pair has more aligned rows than the limit: wrap then stays off.
    pub fn set_wrap(&mut self, on: bool, cx: &mut Context<Self>) -> bool {
        if on && let Some(rows) = self.rows_over_wrap_limit() {
            self.notice = Some(self.wrap_refused(rows));
            cx.notify();
            return false;
        }
        if on != self.wrap {
            self.wrap = on;
            // A wrapped line never scrolls sideways (R3).
            self.display.skip_columns = 0;
            self.switch_rows(on);
            self.remember_wrap(on, cx);
            cx.notify();
        }
        true
    }

    /// After a new result: a pair that grew past the limit stops wrapping, and says why.
    pub(crate) fn check_wrap_limit(&mut self) {
        if self.wrap
            && let Some(rows) = self.rows_over_wrap_limit()
        {
            self.wrap = false;
            self.switch_rows(false);
            self.notice = Some(self.wrap_refused(rows));
        }
    }

    fn rows_over_wrap_limit(&self) -> Option<u32> {
        self.model
            .as_ref()
            .map(TextModel::aligned_count)
            .filter(|&rows| rows > self.wrap_limit)
    }

    fn wrap_refused(&self, rows: u32) -> SharedString {
        trf(
            "Wrap works up to {} rows; this compare has {}.",
            &[
                &format_size(self.wrap_limit.into()),
                &format_size(rows.into()),
            ],
        )
        .into()
    }

    /// Once a frame, from the row list's width `list_width` (its last frame's): the width the
    /// rows are laid out at. That is the list's, except while it keeps changing with wrap on (a
    /// window edge dragged): the rows then keep the width they had as the drag began, until
    /// the width has been still for `WRAP_SETTLE`. gpui's list drops every row's height
    /// whenever its width changes (a pass over every row), and a new column count gives every
    /// row on screen a new wrap to shape, so a frame of the drag would cost far more than one
    /// with wrap off (X-18, X-66). Meanwhile the rows show their old wrap, clipped.
    pub(super) fn follow_list_width(
        &mut self,
        list_width: Pixels,
        cx: &mut Context<Self>,
    ) -> Pixels {
        let before = std::mem::replace(&mut self.list_width, list_width);
        if !self.wrap {
            self.wrap_frozen = None;
        } else if before > px(0.) && list_width != before {
            self.wrap_frozen.get_or_insert(before);
            // A new timer drops the last one: the settle waits for the drag to stop.
            self.wrap_settle = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(WRAP_SETTLE).await;
                let _ = this.update(cx, |view, cx| view.settle_wrap(cx));
            }));
        }
        self.rows_width()
    }

    /// The width the rows are laid out at: the row list's, or the one a resize drag keeps.
    pub(crate) fn rows_width(&self) -> Pixels {
        self.wrap_frozen.unwrap_or(self.list_width)
    }

    /// Once a frame, before the rows sync: a wrapped row's height follows the columns and the
    /// tab width (R3).
    pub(super) fn measure_wrapped(&mut self) {
        let measure = (self.display.view_columns, self.display.tab_width);
        if self.wrap && measure != self.wrap_measure {
            self.wrap_measure = measure;
            // Not the user's scroll: the current difference stays.
            self.note_programmatic_scroll();
            if self
                .rows
                .laid_out_width()
                .is_some_and(|width| width != self.rows_width())
            {
                // The list drops every row's height itself as it lays them out at the new
                // width; `sync` gives them their hints again once it has (one pass).
                self.rows.keep_top();
            } else {
                self.rows.remeasure_all(px(self.row_height()));
            }
        }
    }

    /// The width has been still: the rows take it in the next frame.
    fn settle_wrap(&mut self, cx: &mut Context<Self>) {
        // This runs inside the timer's task: it ends by itself.
        if let Some(task) = self.wrap_settle.take() {
            task.detach();
        }
        if self.wrap_frozen.take().is_some() {
            cx.notify();
        }
    }

    /// How tall display row `display` is drawn while wrapped: the taller half's segments, a
    /// line each, and one line for a fold (R3). The wrapped list knows only the heights of rows
    /// it laid out; a jump and the 30% line walk the others by this (I1, M3).
    pub(crate) fn wrapped_height(&self, display: usize) -> Pixels {
        let row_height = px(self.row_height());
        let Some(model) = self.model.as_ref().filter(|m| display < m.row_count()) else {
            return row_height;
        };
        let DisplayRow::Row(index) = model.display_row(display) else {
            return row_height;
        };
        let row = model.row_at(index);
        let (tab, width) = (self.display.tab_width, self.display.view_columns);
        let lines = |left: bool, line: Option<u32>| {
            line.map_or(1, |l| {
                let text = model.line_text(left, l as usize);
                wrap_starts(&text, tab, width, MAX_WRAP_SEGMENTS + 1)
                    .len()
                    .min(MAX_WRAP_SEGMENTS)
            })
        };
        row_height * lines(true, row.left).max(lines(false, row.right)) as f32
    }

    /// Test hook: the passes over every row the wrapped list has made (X-18).
    #[doc(hidden)]
    pub fn wrap_passes_for_test(&self) -> usize {
        self.rows.passes()
    }

    /// Bench hook: the rows the lists have built so far (X-18).
    #[doc(hidden)]
    pub fn rows_built_for_test(&self) -> usize {
        self.rows_built
    }

    /// Test hook: the width the wrapped list was last laid out at (X-18).
    #[doc(hidden)]
    pub fn wrapped_list_width_for_test(&self) -> Option<Pixels> {
        self.rows.laid_out_width()
    }

    /// Test hook: a row limit for wrap below `WRAP_MAX_ROWS`.
    #[doc(hidden)]
    pub fn set_wrap_limit_for_test(&mut self, rows: u32) {
        self.wrap_limit = rows;
    }

    /// Test hook: the text each line of half `left` of display row `display` showed in the
    /// last frame, one per segment when wrapped; empty when the row was not painted.
    #[doc(hidden)]
    pub fn segment_texts_for_test(&self, display: usize, left: bool) -> Vec<String> {
        self.layouts
            .borrow()
            .get(&(display, left))
            .map(|half| half.lines().iter().map(|t| t.shown.text.clone()).collect())
            .unwrap_or_default()
    }

    /// Test hook: the highlights each of those lines layers on its text, built by the calls
    /// the rows make (display bytes of each line's own text).
    #[doc(hidden)]
    pub fn segment_highlights_for_test(
        &mut self,
        display: usize,
        left: bool,
    ) -> Vec<Vec<(Range<usize>, HighlightStyle)>> {
        let metrics = self.metrics();
        let frame = RowFrame::new(
            &self.request,
            (self.caret, self.row_sel),
            self.find.as_ref(),
            &self.syntax,
            &self.syntax_theme,
            metrics,
        );
        let Some(model) = self.model.as_mut() else {
            return Vec::new();
        };
        let DisplayRow::Row(index) = model.display_row(display) else {
            return Vec::new();
        };
        let row = model.row_at(index);
        let inline = frame.inline(model, index, row.kind);
        let [l, r] = frame.halves(model, display, index, row, inline.as_deref());
        // `render` keeps the syntax theme on the current palette's mode.
        let palette = Palette::of(self.syntax_theme.appearance.is_dark());
        rows::half_highlights(if left { &l } else { &r }, self.display, &palette)
    }
}
