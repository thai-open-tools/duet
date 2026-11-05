//! Moved lines (R12): the "↷ N" tag jumps to the line's partner on a click and moves the line
//! on a drag. An `impl TextView` split out of view.rs.

use gpui_kit::{
    Context, MouseDownEvent, Pixels, Point, ScrollStrategy, Window, canvas, div, prelude::*, px,
};

use gpui_kit::TestSupportExt as _;

use crate::i18n::{tr, tr_key};
use crate::palette::Palette;
use crate::text::model::DisplayRow;
use crate::text::view::TextView;
use crate::text::view::stage::{OVERVIEW_WIDTH, stage_columns};
use crate::ui::drag::follow_drag;

/// A press on a moved line's tag, while the button is held. Only the pointer moving at least
/// `JITTER` from the press makes it a drag; a release before that is a click.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MoveDrag {
    pub left: bool,
    pub line: u32,
    pub press: Point<Pixels>,
    pub at: Point<Pixels>,
    pub moved: bool,
}

/// How far, in px, the pointer must travel from the press before it counts as a drag.
const JITTER: f32 = 4.0;
const OWN_SIDE: &str = tr_key("Drop the line on its own side.");

/// Where a release at a point would drop the line: the side and the display row under the
/// pointer, whether the pointer is in the row's lower half, and the window y of the boundary the
/// line would land on.
struct DropAt {
    left: bool,
    display: usize,
    lower: bool,
    y: Pixels,
}

impl TextView {
    /// Scrolls the partner of `display`'s line on `left` to the centre and marks its row. False
    /// when the line has no partner.
    pub fn jump_to_partner(&mut self, display: usize, left: bool, cx: &mut Context<Self>) -> bool {
        let Some(model) = self.model.as_mut() else {
            return false;
        };
        let Some(partner) = model.moved_partner(display, left) else {
            return false;
        };
        let Some(row) = model.row_of_line(!left, partner) else {
            return false;
        };
        if let Some(display) = model.reveal_row(row) {
            self.scroll_to_display(display, ScrollStrategy::Center);
        }
        self.marked = Some(row);
        cx.notify();
        true
    }

    /// Moves line `from` of one side to just before line `to` as one undo step, marking the
    /// side modified like typing. False (with a notice) when it cannot.
    pub fn move_line(&mut self, left: bool, from: u32, to: u32, cx: &mut Context<Self>) -> bool {
        let edit = self
            .model
            .as_mut()
            .map(|m| m.move_line_edit(left, from, to));
        self.apply_copy(edit, left, cx)
    }

    /// Test hook: the side and line of the move drag in progress.
    #[doc(hidden)]
    pub fn move_drag_for_test(&self) -> Option<(bool, u32)> {
        self.move_drag.map(|d| (d.left, d.line))
    }

    /// A press on the tag of `display`'s line on `left`: starts a move drag, and places no caret.
    pub(crate) fn press_moved_tag(
        &mut self,
        display: usize,
        left: bool,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        let Some(model) = self.model.as_ref() else {
            return;
        };
        let DisplayRow::Row(index) = model.display_row(display) else {
            return;
        };
        let row = model.row_at(index);
        let Some(line) = (if left { row.left } else { row.right }) else {
            return;
        };
        self.move_drag = Some(MoveDrag {
            left,
            line,
            press: event.position,
            at: event.position,
            moved: false,
        });
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The pointer moved to `at` with the button held.
    fn drag_move(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(drag) = self.move_drag.as_mut() else {
            return;
        };
        drag.at = at;
        let (dx, dy) = (at.x - drag.press.x, at.y - drag.press.y);
        drag.moved |= dx.abs().max(dy.abs()) >= px(JITTER);
        if drag.moved {
            cx.notify();
        }
    }

    /// The button came up: a click jumps to the partner, a drag drops the line.
    fn end_move_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.move_drag.take() else {
            return;
        };
        if drag.moved {
            self.drop_moved(drag, cx);
        } else {
            let display = self.model.as_ref().and_then(|m| {
                let row = m.row_of_line(drag.left, drag.line)?;
                m.visible().display_of(row)
            });
            if let Some(display) = display {
                self.jump_to_partner(display, drag.left, cx);
            }
        }
        cx.notify();
    }

    /// Moves the dragged line to where the pointer was released, or says why it stays.
    fn drop_moved(&mut self, drag: MoveDrag, cx: &mut Context<Self>) {
        let Some(drop) = self.drop_at(drag.at) else {
            return;
        };
        if drop.left != drag.left {
            self.notice = Some(tr(OWN_SIDE).into());
            return;
        }
        let Some(model) = self.model.as_ref() else {
            return;
        };
        let lines = match model.display_row(drop.display) {
            DisplayRow::Row(row) => model.side_lines(row..row + 1, drop.left),
            DisplayRow::Fold { start, end } => model.side_lines(start..end, drop.left),
        };
        let to = if drop.lower { lines.end } else { lines.start };
        self.move_line(drag.left, drag.line, to, cx);
    }

    /// The row under window point `at` and which half of it, None outside the rows.
    fn drop_at(&self, at: Point<Pixels>) -> Option<DropAt> {
        let rows = self.model.as_ref()?.row_count();
        let list = self.list_bounds.get();
        if !list.contains(&at) {
            return None;
        }
        let row_height = px(self.row_height());
        let display = self.rows.row_at_y(at.y, list.top(), row_height, rows)?;
        let (top, height) = self.rows.row_span(display, list.top(), row_height)?;
        let lower = at.y >= top + height / 2.;
        let columns = stage_columns(f32::from(list.size.width) + OVERVIEW_WIDTH);
        let centre = list.left() + px((columns.gutter.start + columns.gutter.end) / 2.);
        Some(DropAt {
            left: at.x < centre,
            display,
            lower,
            y: if lower { top + height } else { top },
        })
    }

    /// The canvas that follows the pointer anywhere in the window while a tag drag is held.
    pub(crate) fn move_drag_follower(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        self.move_drag?;
        let view = cx.entity().downgrade();
        Some(
            canvas(
                |_, _, _| {},
                move |_, (), window, _| {
                    follow_drag(view, window, Self::drag_move, Self::end_move_drag)
                },
            )
            .absolute()
            .size_full(),
        )
    }

    /// The 2 px accent line where the dragged line would land: on the dragged line's own half,
    /// above or below the row under the pointer.
    pub(crate) fn move_marker(&self, p: &Palette) -> Option<impl IntoElement> {
        let drag = self.move_drag.filter(|d| d.moved)?;
        let drop = self.drop_at(drag.at).filter(|d| d.left == drag.left)?;
        let list = self.list_bounds.get();
        let columns = stage_columns(f32::from(list.size.width) + OVERVIEW_WIDTH);
        let half = if drop.left {
            &columns.left
        } else {
            &columns.right
        };
        Some(
            div()
                .id("text-move-marker")
                .test_support()
                .absolute()
                .left(px(half.start))
                .w(px(half.end - half.start))
                .top(drop.y - list.top() - px(1.))
                .h(px(2.))
                .bg(p.accent),
        )
    }
}
