//! Row selection on the line numbers (P4 task 9, rulings R5 and R6): a range of aligned rows on
//! one side, made by a click, a Shift+click or a drag on a line-number cell. Alt+arrow then
//! copies just those rows, Ctrl+C copies their text and Delete removes their lines.
//! An `impl TextView` split out of view.rs so the view itself stays readable.

use std::ops::Range;

use gpui_kit::{
    ClipboardItem, Context, MouseDownEvent, Pixels, Point, ScrollStrategy, Window, canvas,
    prelude::*, px,
};

use crate::text::model::DisplayRow;
use crate::text::view::TextView;
use crate::ui::drag::follow_drag;

/// Rows `anchor` and `head` (aligned-row indices, both ends included) of one side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowSel {
    pub left: bool,
    pub anchor: u32,
    pub head: u32,
}

impl RowSel {
    /// The selected rows, whichever end is first. Rows folded away between the ends count.
    pub fn rows(&self) -> Range<u32> {
        self.anchor.min(self.head)..self.anchor.max(self.head) + 1
    }
}

/// A drag that began on a line number. Only the pointer moving at least `JITTER` from the press
/// (or leaving the list) starts it, so a press on a row at the list's edge selects that row and
/// a 1 px twitch neither scrolls nor extends.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RowDrag {
    press: Point<Pixels>,
    at: Point<Pixels>,
    moved: bool,
}

/// How close to the list's top or bottom edge, in px, the pointer must be to scroll it.
const EDGE_BAND: f32 = 6.0;
/// How far, in px, the pointer must travel from the press before it counts as a drag.
const JITTER: f32 = 4.0;
/// Rows per animation frame the list scrolls at the most while the pointer is past an edge.
const MAX_STEP: isize = 3;

impl TextView {
    pub fn row_selection(&self) -> Option<RowSel> {
        self.row_sel
    }

    /// Test hook: selects display rows `from_display..=to_display` on one side, as a drag does.
    #[doc(hidden)]
    pub fn select_rows_for_test(
        &mut self,
        left: bool,
        from_display: usize,
        to_display: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(anchor) = self.row_at_display(from_display, 0) else {
            return;
        };
        let Some(head) = self.row_at_display(to_display, anchor) else {
            return;
        };
        self.row_sel = Some(RowSel { left, anchor, head });
        self.menu_row = None;
        self.caret = None;
        self.composing = None;
        cx.notify();
    }

    /// The aligned row shown at `display`. A fold stands for its hidden rows: the one nearest
    /// `toward`, which is the row a range reaching across the fold ends on.
    fn row_at_display(&self, display: usize, toward: u32) -> Option<u32> {
        match self.model.as_ref()?.display_row(display) {
            DisplayRow::Row(index) => Some(index),
            DisplayRow::Fold { start, end } => Some(if start >= toward { end - 1 } else { start }),
        }
    }

    /// A press on the line-number cell of `display`: a click selects its row, Shift extends from
    /// the old anchor on the same side, and the press starts a drag; while an alignment waits it
    /// finishes the alignment instead.
    pub(crate) fn press_number(
        &mut self,
        display: usize,
        left: bool,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        // While an alignment waits, the number picks the line as a click on the text does (X-91).
        if self.aligning().is_some() {
            self.finish_align(display, left, cx);
            return;
        }
        let Some(model) = self.model.as_ref() else {
            return;
        };
        let DisplayRow::Row(row) = model.display_row(display) else {
            return;
        };
        self.row_sel = Some(match self.row_sel {
            Some(sel) if event.modifiers.shift && sel.left == left => RowSel { head: row, ..sel },
            _ => RowSel {
                left,
                anchor: row,
                head: row,
            },
        });
        self.caret = None;
        self.menu_row = None;
        self.composing = None;
        self.row_drag = Some(RowDrag {
            press: event.position,
            at: event.position,
            moved: false,
        });
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The display row that shows aligned row `row`; a row inside a fold has the fold's.
    fn display_of_row(&self, row: u32) -> Option<usize> {
        let visible = self.model.as_ref()?.visible();
        visible.display_of(row).or_else(|| {
            let fold = visible.fold_of(row)?;
            Some(fold.start.checked_sub(1).map_or(0, |above| {
                visible.display_of(above).map_or(0, |display| display + 1)
            }))
        })
    }

    /// Up and Down with a row selection (R11): the selection, or with `extend` only its head,
    /// moves `delta` display rows. The head stays on screen, and a difference it lands in
    /// becomes current. False without a row selection.
    pub(crate) fn move_row_selection(
        &mut self,
        delta: isize,
        extend: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let (Some(sel), Some(model)) = (self.row_sel, self.model.as_ref()) else {
            return false;
        };
        let count = model.row_count();
        let Some(from) = self.display_of_row(sel.head) else {
            return true;
        };
        let display = from
            .saturating_add_signed(delta)
            .min(count.saturating_sub(1));
        let Some(head) = self.row_at_display(display, sel.anchor) else {
            return true;
        };
        let anchor = if extend { sel.anchor } else { head };
        self.row_sel = Some(RowSel {
            anchor,
            head,
            ..sel
        });
        self.menu_row = None;
        if let Some(model) = self.model.as_mut()
            && let Some(block) = model.block_containing(head)
        {
            model.set_current_block(Some(block));
        }
        self.scroll_to_display(display, ScrollStrategy::Nearest);
        cx.notify();
        true
    }

    /// The display row under window y `y`, clamped to the rows that exist.
    fn display_at_y(&self, y: Pixels) -> Option<usize> {
        let rows = self.model.as_ref()?.row_count();
        let list = self.list_bounds.get();
        let into_list = y.clamp(list.top(), (list.bottom() - px(1.)).max(list.top()));
        self.rows
            .row_at_y(into_list, list.top(), px(self.row_height()), rows)
    }

    fn set_head(&mut self, display: usize) {
        let Some(sel) = self.row_sel else { return };
        if let Some(head) = self.row_at_display(display, sel.anchor) {
            self.row_sel = Some(RowSel { head, ..sel });
        }
    }

    /// The drag's pointer moved to `at`. The head is recomputed from the anchor every time, so
    /// dragging back shrinks the range.
    fn drag_rows(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(drag) = self.row_drag.as_mut() else {
            return;
        };
        drag.at = at;
        let (dx, dy) = (at.x - drag.press.x, at.y - drag.press.y);
        drag.moved |= dx.abs().max(dy.abs()) >= px(JITTER);
        if drag.moved
            && let Some(display) = self.display_at_y(at.y)
        {
            self.set_head(display);
            cx.notify();
        }
    }

    fn end_row_drag(&mut self, cx: &mut Context<Self>) {
        if self.row_drag.take().is_some() {
            cx.notify();
        }
    }

    /// While the pointer is past (or in the band at) the list's top or bottom edge, scrolls up
    /// to three rows a frame and carries the head with it. Runs at the start of every render;
    /// does nothing until the pointer has moved (or left the list) since the press.
    pub(crate) fn drag_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.row_drag else {
            return;
        };
        let list = self.list_bounds.get();
        if !drag.moved && drag.at.y >= list.top() && drag.at.y <= list.bottom() {
            return;
        }
        let band = px(EDGE_BAND);
        let row_height = px(self.row_height());
        let rows = |distance: Pixels| ((distance / row_height).ceil() as isize).clamp(1, MAX_STEP);
        let step = if drag.at.y < list.top() + band {
            -rows(list.top() + band - drag.at.y)
        } else if drag.at.y > list.bottom() - band {
            rows(drag.at.y - (list.bottom() - band))
        } else {
            return;
        };
        let count = self.model.as_ref().map_or(0, |m| m.row_count());
        if count == 0 {
            return;
        }
        self.scroll_rows(step, cx);
        let visible = self.rows.visible_rows(row_height);
        let edge = if step < 0 {
            visible.start
        } else {
            visible.end - 1
        };
        self.set_head(edge.min(count - 1));
        window.request_animation_frame();
    }

    /// The canvas that follows the pointer anywhere in the window while a number drag is held.
    pub(crate) fn row_drag_follower(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        self.row_drag?;
        let view = cx.entity().downgrade();
        Some(
            canvas(
                |_, _, _| {},
                move |_, (), window, _| {
                    follow_drag(view, window, Self::drag_rows, Self::end_row_drag)
                },
            )
            .absolute()
            .size_full(),
        )
    }

    /// Whether Delete, Backspace and Ctrl+C are for the selected rows: there are some, and the
    /// rows (not a find or go-to field) have the focus.
    pub(crate) fn rows_have_keys(&self, window: &Window) -> bool {
        self.row_sel.is_some() && self.focus.is_focused(window)
    }

    /// Copies aligned rows `rows` onto the target side as one undo step.
    pub fn copy_rows(&mut self, rows: Range<u32>, to_left: bool, cx: &mut Context<Self>) -> bool {
        let edit = self.model.as_mut().map(|m| m.copy_rows_edit(rows, to_left));
        self.apply_copy(edit, to_left, cx)
    }

    /// Delete and Backspace with a row selection: removes those lines as one undo step.
    pub(crate) fn delete_selected_rows(&mut self, cx: &mut Context<Self>) {
        let Some(sel) = self.row_sel else { return };
        let Some(model) = self.model.as_mut() else {
            return;
        };
        match model.delete_rows_edit(sel.rows(), sel.left) {
            Err(reason) => {
                self.notice = Some(reason.into());
                cx.notify();
            }
            Ok(range) if range.is_empty() => {
                self.notice = Some("There are no lines on that side to delete.".into());
                cx.notify();
            }
            Ok(range) => {
                self.undo.seal();
                // False while a compare runs, or when the side refuses the edit.
                if self.replace_on(sel.left, range, "", cx) {
                    self.row_sel = None;
                }
                self.undo.seal();
            }
        }
    }

    /// Ctrl+A with a row selection: every row on the same side.
    pub(crate) fn select_all_rows(&mut self, cx: &mut Context<Self>) {
        let (Some(sel), Some(model)) = (self.row_sel, self.model.as_ref()) else {
            return;
        };
        let count = model.aligned_count();
        self.row_sel = Some(RowSel {
            left: sel.left,
            anchor: 0,
            head: count.saturating_sub(1),
        });
        self.menu_row = None;
        cx.notify();
    }

    /// Ctrl+C with a row selection: the rows' text, joined with the side's line break.
    pub(crate) fn copy_selected_rows(&self, cx: &mut Context<Self>) {
        if let (Some(sel), Some(model)) = (self.row_sel, self.model.as_ref()) {
            cx.write_to_clipboard(ClipboardItem::new_string(
                model.rows_text(sel.rows(), sel.left),
            ));
        }
    }
}
