//! The text view's row list (R2): `uniform_list` for every tab with wrap off, so the 1M-line
//! scroll and the 1 GB pairs keep today's code, and gpui's `list`, whose rows have their own
//! heights, only while wrap is on. `RowList` gives both one scrolling API in display rows.

use std::cell::Cell;
use std::ops::Range;

use gpui_kit::{
    ListAlignment, ListOffset, ListState, Pixels, ScrollStrategy, UniformListScrollHandle, px,
};

use super::*;

/// How far past the screen the wrapped list measures rows: what the spike measured (Task 2).
const OVERDRAW: f32 = 200.;

pub(crate) enum RowList {
    Uniform(UniformListScrollHandle),
    Wrapped(WrappedRows),
}

/// The wrapped list's state and what `RowList::sync` compares against.
pub(crate) struct WrappedRows {
    pub(crate) state: ListState,
    /// The model revision the row heights were measured for; None before the first sync.
    revision: Cell<Option<u64>>,
    /// The list width the height hints were last set at: `list` drops every hint when its
    /// width changes, its first layout included.
    hinted_width: Cell<Pixels>,
    /// One line's height, as the last sync was given it.
    row_height: Cell<Pixels>,
    /// Passes over every row this list has made (a reset, or new height hints): what a resize
    /// drag must not repeat a frame (X-18).
    passes: Cell<usize>,
}

impl RowList {
    pub fn uniform() -> Self {
        Self::Uniform(UniformListScrollHandle::new())
    }

    /// A list of `rows` rows, each assumed `row_height` tall until it is drawn.
    pub fn wrapped(rows: usize, row_height: Pixels) -> Self {
        Self::Wrapped(WrappedRows {
            state: ListState::new(rows, ListAlignment::Top, px(OVERDRAW))
                .with_uniform_item_height(row_height),
            revision: Cell::new(None),
            hinted_width: Cell::new(px(0.)),
            row_height: Cell::new(row_height),
            passes: Cell::new(0),
        })
    }

    /// The wrapped list's passes over every row so far; 0 for the uniform list.
    pub fn passes(&self) -> usize {
        match self {
            Self::Uniform(_) => 0,
            Self::Wrapped(w) => w.passes.get(),
        }
    }

    pub fn is_wrapped(&self) -> bool {
        matches!(self, Self::Wrapped(_))
    }

    /// The first row on screen, or the row a pending Top scroll puts there.
    pub fn top_row(&self, row_height: Pixels) -> usize {
        match self {
            Self::Uniform(handle) => {
                let state = handle.0.borrow();
                if let Some(deferred) = &state.deferred_scroll_to_item
                    && deferred.strategy == ScrollStrategy::Top
                {
                    return deferred.item_index;
                }
                (-state.base_handle.offset().y / row_height).round().max(0.) as usize
            }
            // As the uniform list rounds: a row more than half scrolled off is not the top. A
            // scroll past the end puts the top at the row count until the next layout.
            Self::Wrapped(w) => {
                let top = w.state.logical_scroll_top();
                let height = w
                    .state
                    .bounds_for_item(top.item_ix)
                    .map_or(w.row_height.get(), |row| row.size.height);
                let past = top.offset_in_item * 2. >= height && top.offset_in_item > px(0.);
                (top.item_ix + usize::from(past)).min(w.state.item_count().saturating_sub(1))
            }
        }
    }

    /// Where the list is scrolled to: the top row, and for the wrapped list also how far into
    /// it, since a tall row scrolls a long way before the top row changes (X-38).
    pub fn scroll_mark(&self, row_height: Pixels) -> (usize, Pixels) {
        match self {
            Self::Uniform(_) => (self.top_row(row_height), px(0.)),
            Self::Wrapped(w) => {
                let top = w.state.logical_scroll_top();
                (top.item_ix, top.offset_in_item)
            }
        }
    }

    /// Scrolls `display` into view by `strategy`, and not at all when it is already wholly on
    /// screen (the uniform list's `scroll_to_item`). `height` gives a row's height as drawn:
    /// the wrapped list knows only the rows it laid out, so a jump walks the rest (I1).
    pub fn scroll_to(
        &self,
        display: usize,
        strategy: ScrollStrategy,
        height: &dyn Fn(usize) -> Pixels,
    ) {
        match self {
            Self::Uniform(handle) => handle.scroll_to_item(display, strategy),
            Self::Wrapped(w) => w.scroll_to(display, strategy, height),
        }
    }

    /// Puts the top of `display` `share` of a screen below the top of the list, also when it is
    /// on screen: F7 and F8 land on the 30% line, as LocalDiff lands a hunk at 32% (I4).
    pub fn scroll_to_share(
        &self,
        display: usize,
        share: f32,
        row_height: Pixels,
        height: &dyn Fn(usize) -> Pixels,
    ) {
        match self {
            Self::Uniform(handle) => {
                let page = (self.viewport_height() / row_height).floor().max(0.);
                let above = (share * page).floor() as usize;
                handle.scroll_to_item_strict(display.saturating_sub(above), ScrollStrategy::Top);
            }
            Self::Wrapped(w) => {
                let screen = w.state.viewport_bounds().size.height;
                w.state
                    .scroll_to(w.offset_for(display, (screen * share).floor(), height));
            }
        }
    }

    /// Puts `display` at the top, even when it is on screen.
    pub fn scroll_to_top_strict(&self, display: usize) {
        match self {
            Self::Uniform(handle) => handle.scroll_to_item_strict(display, ScrollStrategy::Top),
            Self::Wrapped(w) => w.top_at(display),
        }
    }

    /// Scrolls by `dy`, positive down, inside the list (PF16: the uniform offset grows negative
    /// down, `ListState` positive down).
    pub fn scroll_by(&self, dy: Pixels) {
        match self {
            Self::Uniform(handle) => {
                let state = handle.0.borrow();
                let base = &state.base_handle;
                let mut offset = base.offset();
                offset.y = (offset.y - dy).min(px(0.)).max(-base.max_offset().y);
                base.set_offset(offset);
            }
            // The list clamps at the top; its next layout clamps at the end.
            Self::Wrapped(w) => w.state.scroll_by(dy),
        }
    }

    /// The width the wrapped list was last laid out at; None for the uniform list.
    pub fn laid_out_width(&self) -> Option<Pixels> {
        match self {
            Self::Uniform(_) => None,
            Self::Wrapped(w) => Some(w.state.viewport_bounds().size.width),
        }
    }

    pub fn viewport_height(&self) -> Pixels {
        match self {
            Self::Uniform(handle) => handle.0.borrow().base_handle.bounds().size.height,
            Self::Wrapped(w) => w.state.viewport_bounds().size.height,
        }
    }

    /// The overview box as fractions of the whole list; None while it has no rows. The wrapped
    /// list counts rows, as the overview's marks do, not pixels: each row an equal share, and
    /// within a row by how far into its height the screen's edge is, so the box follows a scroll
    /// inside a tall row (X-12). `height` gives a row's height where it is not laid out.
    pub fn viewport(
        &self,
        rows: usize,
        row_height: Pixels,
        height: &dyn Fn(usize) -> Pixels,
    ) -> Option<(f32, f32)> {
        match self {
            Self::Uniform(handle) => {
                let state = handle.0.borrow();
                let base = &state.base_handle;
                let total = row_height * rows as f32;
                (rows > 0 && total > px(0.)).then(|| {
                    let top = (-base.offset().y / total).clamp(0.0, 1.0);
                    let height = (base.bounds().size.height / total).clamp(0.0, 1.0);
                    (top, (top + height).min(1.0))
                })
            }
            Self::Wrapped(w) => (rows > 0).then(|| {
                let n = rows as f32;
                let row_px = |row: usize| w.drawn_height(row).unwrap_or_else(|| height(row));
                let into = |row: usize, px_in: Pixels| {
                    let h = row_px(row);
                    let part = if h > px(0.) { px_in / h } else { 0. };
                    row as f32 + part.clamp(0.0, 1.0)
                };
                let last = rows - 1;
                let top = w.state.logical_scroll_top();
                let first = top.item_ix.min(last);
                // The screen's bottom edge: a screen's height on from the top edge.
                let (mut row, mut below) = (
                    first,
                    w.state.viewport_bounds().size.height + top.offset_in_item,
                );
                while row < last {
                    let h = row_px(row);
                    if h > below {
                        break;
                    }
                    below -= h;
                    row += 1;
                }
                let start = (into(first, top.offset_in_item) / n).clamp(0.0, 1.0);
                (start, (into(row, below) / n).clamp(start, 1.0))
            }),
        }
    }

    /// Puts the spot `fraction` of the way down the overview strip on screen: the uniform list
    /// centres the row there; the wrapped list puts the spot itself, by the row's height, in
    /// the middle of the screen, also when that row is on screen already, so a click inside a
    /// tall row and a drag of the thumb go where the pointer is (X-12).
    pub fn scroll_to_fraction(&self, fraction: f32, rows: usize, height: &dyn Fn(usize) -> Pixels) {
        if rows == 0 {
            return;
        }
        match self {
            Self::Uniform(handle) => {
                handle.scroll_to_item(display_at(fraction, rows), ScrollStrategy::Center)
            }
            Self::Wrapped(w) => {
                let at = fraction.clamp(0.0, 1.0) * rows as f32;
                let row = (at as usize).min(rows - 1);
                let into = height(row) * (at - row as f32).clamp(0.0, 1.0);
                let middle = (w.state.viewport_bounds().size.height / 2.).floor();
                let offset = if into >= middle {
                    ListOffset {
                        item_ix: row,
                        offset_in_item: into - middle,
                    }
                } else {
                    w.offset_for(row, middle - into, height)
                };
                w.state.scroll_to(offset);
            }
        }
    }

    /// The display row `share` of a screen below the top: the 30% line (R9). The uniform list
    /// counts one-line rows; the wrapped list walks the rows' drawn heights from the top, which
    /// need not be measured yet (M3).
    pub fn row_at_share(
        &self,
        share: f32,
        row_height: Pixels,
        height: &dyn Fn(usize) -> Pixels,
    ) -> usize {
        match self {
            Self::Uniform(_) => {
                let page = (self.viewport_height() / row_height).floor().max(0.);
                self.top_row(row_height) + (share * page).floor() as usize
            }
            Self::Wrapped(w) => {
                let top = w.state.logical_scroll_top();
                let line = w.state.viewport_bounds().size.height * share;
                let last = w.state.item_count().saturating_sub(1);
                let (mut row, mut y) = (top.item_ix.min(last), -top.offset_in_item);
                while row < last && y + height(row) <= line {
                    y += height(row);
                    row += 1;
                }
                row
            }
        }
    }

    /// The rows wholly on screen from the top row, at least one. The uniform range is what a
    /// screen holds and may run past the last row.
    pub fn visible_rows(&self, row_height: Pixels) -> Range<usize> {
        let top = self.top_row(row_height);
        match self {
            Self::Uniform(_) => {
                let rows = ((self.viewport_height() / row_height).floor() as usize).max(1);
                top..top + rows
            }
            Self::Wrapped(w) => {
                let bottom = w.state.viewport_bounds().bottom();
                let mut end = top;
                while let Some(row) = w.state.bounds_for_item(end)
                    && row.bottom() <= bottom + px(0.5)
                {
                    end += 1;
                }
                top..end.max(top + 1)
            }
        }
    }

    /// The display row under window y `y`, of `rows`; `list_top` is the list's top edge.
    pub fn row_at_y(
        &self,
        y: Pixels,
        list_top: Pixels,
        row_height: Pixels,
        rows: usize,
    ) -> Option<usize> {
        if rows == 0 {
            return None;
        }
        let row = match self {
            Self::Uniform(handle) => {
                let offset = handle.0.borrow().base_handle.offset().y;
                ((y - list_top - offset) / row_height).floor().max(0.) as usize
            }
            // The drawn rows from the top, down to the one that holds `y`.
            Self::Wrapped(w) => {
                let mut row = w.state.logical_scroll_top().item_ix;
                while let Some(bounds) = w.state.bounds_for_item(row)
                    && bounds.bottom() <= y
                {
                    row += 1;
                }
                row
            }
        };
        Some(row.min(rows - 1))
    }

    /// Window y and height of display row `display`, when it is laid out; `list_top` is the
    /// list's top edge.
    pub fn row_span(
        &self,
        display: usize,
        list_top: Pixels,
        row_height: Pixels,
    ) -> Option<(Pixels, Pixels)> {
        match self {
            Self::Uniform(handle) => {
                let offset = handle.0.borrow().base_handle.offset().y;
                Some((list_top + offset + row_height * display as f32, row_height))
            }
            Self::Wrapped(w) => w
                .state
                .bounds_for_item(display)
                .map(|row| (row.top(), row.size.height)),
        }
    }

    /// Once a frame, before the list is drawn; nothing for the uniform list. A new row count
    /// resets the wrapped list and puts the old top row back; the same count with a new model
    /// revision re-measures the rows on screen and a page either side.
    pub fn sync(&self, rows: usize, row_height: Pixels, revision: u64) {
        let Self::Wrapped(w) = self else {
            return;
        };
        let state = &w.state;
        let width = state.viewport_bounds().size.width;
        w.row_height.set(row_height);
        if state.item_count() != rows {
            let top = state.logical_scroll_top();
            w.pass();
            state.reset_with_uniform_height(rows, row_height);
            state.scroll_to(top);
            w.hinted_width.set(width);
        } else if w.revision.get().is_some_and(|r| r != revision) {
            self.remeasure_near_top(row_height);
        }
        if width > px(0.) && width != w.hinted_width.get() {
            // `with_uniform_item_height` sets the hints on the state it shares with this clone;
            // rows already measured keep their heights as hints.
            w.pass();
            let _ = state.clone().with_uniform_item_height(row_height);
            w.hinted_width.set(width);
        }
        w.revision.set(Some(revision));
    }

    /// Wrapped: the rows on screen and a page either side are measured again, after an edit.
    pub fn remeasure_near_top(&self, row_height: Pixels) {
        if let Self::Wrapped(w) = self {
            let rows = w.state.item_count();
            let top = w.state.logical_scroll_top().item_ix;
            let page = ((self.viewport_height() / row_height).ceil() as usize).max(1);
            w.state
                .remeasure_items(top.saturating_sub(page)..(top + 2 * page).min(rows));
        }
    }

    /// Wrapped: the top row stays on top, at its start, while the list lays its rows out at a
    /// new width (it drops their heights itself then; X-18).
    pub fn keep_top(&self) {
        if let Self::Wrapped(w) = self {
            w.top_at(w.state.logical_scroll_top().item_ix);
        }
    }

    /// Wrapped: every row is `row_height` tall again until drawn, keeping the top row; after a
    /// change of the font, or of the columns or tab width a wrapped line is cut at.
    pub fn remeasure_all(&self, row_height: Pixels) {
        if let Self::Wrapped(w) = self {
            let top = w.state.logical_scroll_top().item_ix;
            w.pass();
            w.state
                .reset_with_uniform_height(w.state.item_count(), row_height);
            w.top_at(top);
            w.hinted_width.set(w.state.viewport_bounds().size.width);
            w.row_height.set(row_height);
        }
    }
}

impl WrappedRows {
    fn pass(&self) {
        self.passes.set(self.passes.get() + 1);
    }

    /// Row `display`'s height as laid out, when it is.
    fn drawn_height(&self, display: usize) -> Option<Pixels> {
        self.state
            .bounds_for_item(display)
            .map(|row| row.size.height)
    }

    fn top_at(&self, display: usize) {
        self.state.scroll_to(ListOffset {
            item_ix: display,
            offset_in_item: px(0.),
        });
    }

    fn scroll_to(
        &self,
        display: usize,
        strategy: ScrollStrategy,
        height: &dyn Fn(usize) -> Pixels,
    ) {
        let viewport = self.state.viewport_bounds();
        if self
            .state
            .bounds_for_item(display)
            .is_some_and(|row| row.top() >= viewport.top() && row.bottom() <= viewport.bottom())
        {
            return;
        }
        // The rows above the target take their drawn heights (I1): a screen of one-line rows
        // would put a target below tall rows off the screen.
        let screen = viewport.size.height;
        let bottom = || (screen - height(display)).max(px(0.));
        let top = self.state.logical_scroll_top().item_ix;
        match strategy {
            ScrollStrategy::Top => self.top_at(display),
            // The row's top at the middle, where a screen of one-line rows put it.
            ScrollStrategy::Center => {
                self.state
                    .scroll_to(self.offset_for(display, (screen / 2.).floor(), height))
            }
            ScrollStrategy::Bottom => {
                self.state
                    .scroll_to(self.offset_for(display, bottom(), height))
            }
            // Above the screen it goes to the top. Below it, to the bottom, and only ever down,
            // as `scroll_to_reveal_item` does: a row whose height is not measured yet may be
            // on screen (PF16: the check above keeps a measured row on screen where it is).
            ScrollStrategy::Nearest if display <= top => self.top_at(display),
            ScrollStrategy::Nearest => {
                let goal = self.offset_for(display, bottom(), height);
                if goal.item_ix >= top {
                    self.state.scroll_to(goal);
                }
            }
        }
    }

    /// The top that puts the top of `display` `above` px below the top of the screen: the rows
    /// over it, walked up by their drawn heights, need not be measured.
    fn offset_for(
        &self,
        display: usize,
        above: Pixels,
        height: &dyn Fn(usize) -> Pixels,
    ) -> ListOffset {
        let (mut row, mut gap) = (display, above);
        while row > 0 && gap > px(0.) {
            let h = height(row - 1);
            if h > gap {
                // The row above shows its last `gap` px.
                return ListOffset {
                    item_ix: row - 1,
                    offset_in_item: h - gap,
                };
            }
            gap -= h;
            row -= 1;
        }
        ListOffset {
            item_ix: row,
            offset_in_item: px(0.),
        }
    }
}

impl TextView {
    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    pub fn row_height(&self) -> f32 {
        row_height_for(self.font_size)
    }

    /// What each row is drawn with.
    pub(crate) fn metrics(&self) -> rows::RowMetrics {
        rows::RowMetrics {
            row_height: self.row_height(),
            font_size: self.font_size,
            wrap: self.wrap,
        }
    }

    /// The code font's size, clamped to the dropdown's range; the top row stays on top.
    pub fn set_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        let (min, max) = (FONT_SIZES[0], FONT_SIZES[FONT_SIZES.len() - 1]);
        let size = if size.is_finite() {
            size.clamp(min, max)
        } else {
            DEFAULT_FONT_SIZE
        };
        if size == self.font_size {
            return;
        }
        let top = self.top_row();
        self.font_size = size;
        let row_height = px(self.row_height());
        // Not the user's scroll: the current difference stays (X-38 follows the offset too).
        self.note_programmatic_scroll();
        self.rows.remeasure_all(row_height);
        self.rows.scroll_to_top_strict(top);
        self.remember_font_size(cx);
        cx.notify();
    }

    /// The rows are in the variable-height list.
    pub fn rows_wrapped(&self) -> bool {
        self.rows.is_wrapped()
    }

    /// Test hook: the list kind alone, no wrapping. The top row stays on top (PF16).
    #[doc(hidden)]
    pub fn set_list_rows_for_test(&mut self, wrapped: bool, cx: &mut Context<Self>) {
        self.switch_rows(wrapped);
        cx.notify();
    }

    /// Puts the rows in the wrapped or the uniform list; the top row stays on top (PF16).
    pub(crate) fn switch_rows(&mut self, wrapped: bool) {
        if self.rows.is_wrapped() == wrapped {
            return;
        }
        let top = self.top_row();
        let rows = self.model.as_ref().map_or(0, TextModel::row_count);
        self.rows = if wrapped {
            RowList::wrapped(rows, px(self.row_height()))
        } else {
            RowList::uniform()
        };
        self.rows.scroll_to_top_strict(top);
    }

    /// Every scroll duet makes itself goes through here (PF11), so the current difference can
    /// tell them from the user's. Key scrolls, the wheel and the overview are the user's and
    /// call `rows` directly.
    pub(crate) fn scroll_to_display(&mut self, display: usize, strategy: ScrollStrategy) {
        self.note_programmatic_scroll();
        self.rows
            .scroll_to(display, strategy, &|row| self.wrapped_height(row));
    }

    /// F7, F8 and the first and last difference: the block's first row on the 30% line, so
    /// following the scroll keeps it current while the user reads on (I4).
    pub(crate) fn scroll_to_follow_line(&mut self, display: usize) {
        self.note_programmatic_scroll();
        let row_height = px(self.row_height());
        self.rows
            .scroll_to_share(display, FOLLOW_LINE, row_height, &|row| {
                self.wrapped_height(row)
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROW: Pixels = px(20.);

    #[test]
    fn the_wrapped_list_scrolls_down_for_a_positive_delta() {
        let rows = RowList::wrapped(100, ROW);
        rows.scroll_by(px(45.));
        assert_eq!(
            rows.top_row(ROW),
            2,
            "45 px down: row 2 is mostly on screen"
        );
        rows.scroll_by(px(5.));
        assert_eq!(
            rows.top_row(ROW),
            3,
            "two rows and a half round to three, as in the uniform list"
        );
        rows.scroll_by(px(-1_000.));
        assert_eq!(rows.top_row(ROW), 0, "never above the first row");
    }

    #[test]
    fn a_pending_top_scroll_is_the_top_row_in_both_lists() {
        for rows in [RowList::uniform(), RowList::wrapped(300, ROW)] {
            assert_eq!(rows.top_row(ROW), 0);
            rows.scroll_to_top_strict(120);
            assert_eq!(rows.top_row(ROW), 120, "wrapped: {}", rows.is_wrapped());
        }
    }

    #[test]
    fn a_new_row_count_keeps_the_wrapped_top_row() {
        let rows = RowList::wrapped(300, ROW);
        rows.sync(300, ROW, 1);
        rows.scroll_to_top_strict(120);
        rows.sync(301, ROW, 2);
        assert_eq!(
            rows.top_row(ROW),
            120,
            "a row added below the top leaves it"
        );
        rows.sync(301, ROW, 3);
        assert_eq!(rows.top_row(ROW), 120, "a re-measure keeps it as well");
        rows.sync(50, ROW, 4);
        assert_eq!(
            rows.top_row(ROW),
            49,
            "a shorter list keeps the top inside it"
        );
    }
}
