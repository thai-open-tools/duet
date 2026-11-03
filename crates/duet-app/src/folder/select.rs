//! Drag-select in the folder list: the press, the pointer-to-row maths and the auto-scroll.

use std::collections::BTreeSet;

use gpui_kit::{
    AnyElement, Context, DispatchPhase, Modifiers, MouseButton, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, WeakEntity, Window, canvas, point, prelude::*, px,
};

use crate::folder::view::{FolderView, ROW_HEIGHT};

/// The strip at the list's top and bottom edge where a held pointer scrolls it.
const EDGE_BAND: f32 = 28.;
/// The most a held pointer scrolls per frame.
const MAX_STEP: f32 = 80.;

/// A press on a row that has not been released yet.
#[derive(Default)]
pub struct DragSelect {
    pub(super) active: bool,
    /// The row pressed: one end of the swept range.
    anchor_row: usize,
    /// The chosen set at a Ctrl press (after its own toggle), empty for a plain one; the
    /// range is added to it on every move, so sweeping back shrinks.
    base: BTreeSet<u32>,
    /// A held move changed the row or left the list: only then does the drag act, so a
    /// press on an edge row does not scroll by itself.
    started: bool,
    at: Point<Pixels>,
    last_row: usize,
}

/// The row under pointer height `y`, for a list whose top is at `top` and which is scrolled
/// `scrolled` px down; clamped to the rows there are.
pub fn row_at(y: f32, top: f32, scrolled: f32, rows: usize) -> Option<usize> {
    let last = rows.checked_sub(1)?;
    let row = ((y - top + scrolled) / ROW_HEIGHT).floor().max(0.);
    Some((row as usize).min(last))
}

/// How far to scroll for a pointer at `y` over a list from `top` to `bottom`: negative is up,
/// zero in the middle. Half the depth into the band, so it speeds up outside the list.
pub fn edge_step(y: f32, top: f32, bottom: f32) -> f32 {
    let step = |depth: f32| (depth / 2.).clamp(1., MAX_STEP);
    if y < top + EDGE_BAND {
        -step(top + EDGE_BAND - y)
    } else if y > bottom - EDGE_BAND {
        step(y - (bottom - EDGE_BAND))
    } else {
        0.
    }
}

impl FolderView {
    /// A left press on a row: Ctrl toggles it, a plain press selects it, and either starts a
    /// drag. Shift is left to the click, which has no drag.
    pub(super) fn press_row(
        &mut self,
        row: usize,
        modifiers: Modifiers,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let Some(model) = &mut self.model else {
            return;
        };
        if modifiers.shift {
            return;
        }
        if modifiers.control {
            model.toggle_chosen(row);
        } else {
            model.select_row(row);
        }
        self.drag = DragSelect {
            active: true,
            anchor_row: row,
            base: if modifiers.control {
                model.chosen_snapshot()
            } else {
                BTreeSet::new()
            },
            started: false,
            at,
            last_row: row,
        };
        cx.notify();
    }

    /// Follows a held pointer: extends the range to the row under it, and scrolls near an edge.
    /// Runs every frame of a drag, so a still pointer at the edge keeps scrolling.
    pub(super) fn drag_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !(self.drag.active && self.drag.started) {
            return;
        }
        let (top, bottom, offset_y) = self.list_geometry_raw();
        let Some(model) = &mut self.model else {
            return;
        };
        let offset = point(px(0.), px(offset_y));
        let y = f32::from(self.drag.at.y);
        let rows = model.visible_len();
        let room = (rows as f32 * ROW_HEIGHT - (bottom - top)).max(0.);
        let scrolled = (-f32::from(offset.y) + edge_step(y, top, bottom)).clamp(0., room);
        let step = scrolled + f32::from(offset.y);
        if step != 0. {
            self.scroll
                .0
                .borrow()
                .base_handle
                .set_offset(point(offset.x, px(-scrolled)));
            let id = cx.entity_id();
            window.on_next_frame(move |_, cx| cx.notify(id));
        }
        if let Some(row) = row_at(y, top, scrolled, rows)
            && row != self.drag.last_row
        {
            self.drag.last_row = row;
            model.set_range_over(&self.drag.base, self.drag.anchor_row, row);
        }
    }

    /// The list's top, bottom and px scrolled down, as of the last frame.
    fn list_geometry(&self) -> (f32, f32, f32) {
        let (top, bottom, offset_y) = self.list_geometry_raw();
        (top, bottom, -offset_y)
    }

    /// As `list_geometry`, with the raw (negative) scroll offset.
    fn list_geometry_raw(&self) -> (f32, f32, f32) {
        let state = self.scroll.0.borrow();
        let (bounds, offset) = (state.base_handle.bounds(), state.base_handle.offset());
        (
            f32::from(bounds.top()),
            f32::from(bounds.bottom()),
            f32::from(offset.y),
        )
    }

    pub(super) fn drag_moved(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.drag.started {
            let (top, bottom, scrolled) = self.list_geometry();
            let rows = self.model.as_ref().map_or(0, |m| m.visible_len());
            let (y, outside) = (
                f32::from(at.y),
                f32::from(at.y) < top || f32::from(at.y) > bottom,
            );
            self.drag.started =
                outside || row_at(y, top, scrolled, rows).is_some_and(|r| r != self.drag.last_row);
        }
        self.drag.at = at;
        cx.notify();
    }

    pub(super) fn end_drag(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.drag.active) {
            cx.notify();
        }
    }
}

/// Window-level listeners for a drag, armed from the paint of every frame it lasts (a
/// listener lives one frame): held moves steer the drag, a release or a move with the button
/// up ends it. Held moves are kept from the elements under the pointer.
pub(super) fn drag_listeners(view: WeakEntity<FolderView>) -> AnyElement {
    canvas(
        |_, _, _| {},
        move |_, (), window, _| {
            let on_move = view.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let held = event.pressed_button == Some(MouseButton::Left);
                if held {
                    cx.stop_propagation();
                }
                let _ = on_move.update(cx, |this, cx| {
                    if held {
                        this.drag_moved(event.position, cx);
                    } else {
                        this.end_drag(cx);
                    }
                });
            });
            let on_up = view.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                    let _ = on_up.update(cx, |this, cx| this.end_drag(cx));
                }
            });
        },
    )
    .absolute()
    .size_0()
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_under_the_pointer_counts_the_scroll_and_clamps() {
        assert_eq!(row_at(100., 100., 0., 10), Some(0));
        assert_eq!(row_at(100. + 2.5 * ROW_HEIGHT, 100., 0., 10), Some(2));
        assert_eq!(
            row_at(100. + ROW_HEIGHT, 100., 3. * ROW_HEIGHT, 10),
            Some(4)
        );
        assert_eq!(row_at(10., 100., 0., 10), Some(0), "above the list");
        assert_eq!(row_at(9999., 100., 0., 10), Some(9), "below the last row");
        assert_eq!(row_at(100., 100., 0., 0), None);
    }

    #[test]
    fn the_edge_bands_scroll_faster_the_further_out_the_pointer_is() {
        let (top, bottom) = (100., 500.);
        assert_eq!(edge_step(300., top, bottom), 0., "the middle stays");
        assert_eq!(
            edge_step(top + 27., top, bottom),
            -1.,
            "slowest at the band"
        );
        assert_eq!(edge_step(top + 8., top, bottom), -10.);
        assert_eq!(edge_step(bottom - 8., top, bottom), 10.);
        assert_eq!(edge_step(bottom + 400., top, bottom), 80., "capped");
        assert_eq!(edge_step(top - 400., top, bottom), -80.);
    }
}
