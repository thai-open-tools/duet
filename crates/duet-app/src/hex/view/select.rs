//! Selecting bytes with the mouse, copying them, and what the rows draw for the cursor and the
//! selection (A2, R9).

use duet_core::hex::format::{bytes_hex, format_offset, hex_line};
use gpui_kit::{ClipboardItem, MouseDownEvent, Point};

use super::rows::{Picked, row_span};
use super::*;
use crate::format::count_of;

/// The most source bytes one copy takes (R9).
const COPY_CAP: u64 = 1 << 20;
const COPIED_CAPPED: &str = "Copied the first 1 MiB of the selection.";
const COPIED_ROW: &str = "Copied the cursor row.";
const NOTHING_TO_COPY: &str = "Nothing to copy: click a byte or select a range first.";

/// A listener that moves the cursor (`extend`: Shift is held) unless a field has the keys.
macro_rules! step_key {
    ($cx:expr, $action:ty, $step:expr, $extend:expr) => {
        $cx.listener(|this, _: &$action, w, cx| {
            if this.takes_key(w, cx) {
                this.step_cursor($step, $extend, cx)
            }
        })
    };
}

/// Where a cursor key goes (LocalDiff's `onKey`).
#[derive(Clone, Copy)]
pub(super) enum Step {
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    /// The start of the cursor's row, not of the file.
    Home,
    End,
}

impl HexView {
    /// A press on the rows: a plain click puts the cursor on the byte; Shift extends a
    /// selection on the same side from its anchor (the cursor, or the selection's fixed end
    /// after Shift+arrows, which leave the cursor on the moving end); the cursor goes back to
    /// the anchor. A click past a file's end does nothing.
    pub(super) fn press_rows(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let Some((left, offset)) = self.hit_byte(event.position) else {
            return;
        };
        self.notice = None;
        match self.cursor {
            Some((side, at)) if event.modifiers.shift && side == left => {
                let anchor = self.selection_ends().map_or(at, |ends| ends.0);
                let (lo, hi) = (anchor.min(offset), anchor.max(offset));
                self.selection = Some((left, lo..hi + 1));
                self.sel_ends = Some((anchor, offset));
                self.cursor = Some((left, anchor));
                self.current_block = self.block_at(anchor);
            }
            _ => {
                self.cursor = Some((left, offset));
                self.selection = None;
                self.current_block = self.block_at(offset);
            }
        }
        cx.notify();
    }

    /// The byte under a window point: (left side?, offset), when it is on a byte the side has.
    fn hit_byte(&self, at: Point<Pixels>) -> Option<(bool, u64)> {
        let (pair, cols) = (self.pair.as_ref()?, self.cols?);
        let body = self.body_bounds.get();
        let (x, y) = (
            f32::from(at.x - body.left()) + self.skip_x,
            f32::from(at.y - body.top()),
        );
        if x < 0. || y < 0. {
            return None;
        }
        let row = self.file_row(self.top.checked_add((y / ROW_HEIGHT) as u64)?)?;
        let (left, position) = cols.hit(x)?;
        let offset = row
            .checked_mul(self.bytes_per_row)?
            .checked_add(position as u64)?;
        let len = if left {
            pair.left.len()
        } else {
            pair.right.len()
        };
        (offset < len).then_some((left, offset))
    }

    /// The selection's fixed and moving ends, while the selection still spans exactly them.
    fn selection_ends(&self) -> Option<(u64, u64)> {
        let (_, range) = self.selection.as_ref()?;
        self.sel_ends
            .filter(|&(a, b)| *range == (a.min(b)..a.max(b) + 1))
    }

    /// An arrow, page or Home/End key: the cursor moves and the view follows just enough to
    /// show it. With `extend` the selection grows from its fixed end; without, it collapses.
    pub(super) fn step_cursor(&mut self, step: Step, extend: bool, cx: &mut Context<Self>) {
        let Some(pair) = self.pair.clone() else {
            return;
        };
        let left = self.cursor.is_none_or(|c| c.0);
        let len = if left {
            pair.left.len()
        } else {
            pair.right.len()
        };
        if len == 0 {
            return;
        }
        self.notice = None;
        let ends = self.selection_ends();
        let from = ends
            .map(|e| e.1)
            .or(self.cursor.map(|c| c.1))
            .unwrap_or_else(|| self.top_byte())
            .min(len - 1);
        let bpr = self.bytes_per_row;
        let (row, col) = (from / bpr, from % bpr);
        let to = match step {
            Step::Left => from.saturating_sub(1),
            Step::Right => from + 1,
            Step::Home => row * bpr,
            Step::End => row * bpr + bpr - 1,
            Step::Up | Step::Down | Step::PageUp | Step::PageDown => {
                if self.only_diff && self.diff_rows.is_none() {
                    self.notice = Some(STILL_COMPARING.into());
                    cx.notify();
                    return;
                }
                let page = self.visible_rows().saturating_sub(1).max(1);
                let down = matches!(step, Step::Down | Step::PageDown);
                let mut n = if matches!(step, Step::Up | Step::Down) {
                    1
                } else {
                    page
                };
                // A hidden row's "display row" is the next one shown, which is already below.
                if down && !self.row_shown(row) {
                    n -= 1;
                }
                let last = self.row_count().saturating_sub(1);
                let at = self.display_row(row).unwrap_or(last);
                let target = if down {
                    at.saturating_add(n).min(last)
                } else {
                    at.saturating_sub(n)
                };
                let Some(file_row) = self.file_row(target) else {
                    return;
                };
                file_row * bpr + col
            }
        }
        .min(len - 1);
        if extend {
            let anchor = ends.map_or(from, |e| e.0);
            self.selection = Some((left, anchor.min(to)..anchor.max(to) + 1));
            self.sel_ends = Some((anchor, to));
        } else {
            self.selection = None;
            self.sel_ends = None;
        }
        self.cursor = Some((left, to));
        self.current_block = self.block_at(to);
        self.follow(to / bpr, cx);
        cx.notify();
    }

    /// Scrolls just far enough that file row `row` is among the visible ones.
    fn follow(&mut self, row: u64, cx: &mut Context<Self>) {
        let Some(shown) = self.display_row(row) else {
            return;
        };
        let visible = self.visible_rows();
        if shown < self.top {
            self.scroll_to_row(shown, cx);
        } else if shown >= self.top.saturating_add(visible) {
            self.scroll_to_row(shown + 1 - visible, cx);
        }
    }

    /// The part of the cursor and the selection that falls in the file bytes `at`, on one side.
    pub(super) fn picked_in(&self, left: bool, at: &Range<u64>) -> Picked {
        let cursor = self
            .cursor
            .filter(|&(side, offset)| side == left && at.contains(&offset))
            .map(|(_, offset)| (offset - at.start) as usize);
        let range = self
            .selection
            .as_ref()
            .filter(|(side, _)| *side == left)
            .and_then(|(_, range)| {
                let (lo, hi) = (range.start.max(at.start), range.end.min(at.end));
                (lo < hi).then(|| (lo - at.start) as usize..(hi - at.start) as usize)
            });
        Picked { range, cursor }
    }

    /// Ctrl+C: the selected bytes as uppercase hex (at most 1 MiB of them), else the cursor's
    /// row as a hex dump line.
    pub(super) fn copy_hex(&mut self, cx: &mut Context<Self>) {
        let Some(pair) = self.pair.clone() else {
            return;
        };
        let side = |left: bool| if left { &pair.left } else { &pair.right };
        let (text, notice) = if let Some((left, range)) = self.selection.clone() {
            // Only the bytes the side has: a reload may have left the selection past its end.
            let have = range.end.min(side(left).len());
            if range.start >= have {
                self.notice = Some(
                    format!(
                        "The selection is past the end of the {} file.",
                        if left { "left" } else { "right" }
                    )
                    .into(),
                );
                cx.notify();
                return;
            }
            let end = have.min(range.start.saturating_add(COPY_CAP));
            let text = bytes_hex(&side(left).bytes(range.start..end));
            let notice = if end < have {
                COPIED_CAPPED.to_owned()
            } else {
                format!("Copied {}.", count_of(end - range.start, "byte", "bytes"))
            };
            (text, notice)
        } else if let Some((left, at)) = self.cursor.or_else(|| {
            // Neither selection nor cursor: the left side's top row (the right's when the
            // left is empty), as LocalDiff copies it.
            let left = !pair.left.is_empty() || pair.right.is_empty();
            (pair.max_len() > 0).then(|| (left, self.top_byte()))
        }) {
            let bpr = self.bytes_per_row;
            let span = row_span(at / bpr, bpr, pair.max_len());
            let line = hex_line(
                span.start,
                &side(left).bytes(span),
                bpr as usize,
                pair.max_len().saturating_sub(1),
            );
            (line, COPIED_ROW.to_owned())
        } else {
            self.notice = Some(NOTHING_TO_COPY.into());
            cx.notify();
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.notice = Some(notice.into());
        cx.notify();
    }

    /// Test hook: where byte `at`'s hex cell is centred, in px from a row's left edge.
    #[doc(hidden)]
    pub fn cell_x_for_test(&self, left: bool, at: usize) -> Option<f32> {
        Some(self.cols?.hex_center(left, at))
    }

    /// Test hook: the middle of the gutter, in px from a row's left edge.
    #[doc(hidden)]
    pub fn gutter_x_for_test(&self) -> Option<f32> {
        Some(self.cols?.gutter_center())
    }

    /// Test hook: selects `range` of one side without the mouse.
    #[doc(hidden)]
    pub fn select_for_test(&mut self, left: bool, range: Range<u64>, cx: &mut Context<Self>) {
        self.selection = Some((left, range));
        cx.notify();
    }

    /// A right press on the rows: it becomes the menu's target, and a byte outside the
    /// selection gets the cursor first (a byte inside it keeps the selection).
    pub(super) fn press_right(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.menu_byte = self.hit_byte(event.position);
        let Some((left, offset)) = self.menu_byte else {
            return;
        };
        let inside = self
            .selection
            .as_ref()
            .is_some_and(|(side, range)| *side == left && range.contains(&offset));
        if !inside {
            self.notice = None;
            self.cursor = Some((left, offset));
            self.selection = None;
            self.current_block = self.block_at(offset);
        }
        cx.notify();
    }

    /// "Copy offset": the pressed byte's offset (else the cursor's, else the top row's) as
    /// `0x` and the status line's digits.
    pub(super) fn copy_offset(&mut self, cx: &mut Context<Self>) {
        let Some(pair) = self.pair.clone() else {
            return;
        };
        let offset = self
            .menu_byte
            .or(self.cursor)
            .map_or_else(|| self.top_byte(), |c| c.1);
        let text = format!(
            "0x{}",
            format_offset(offset, pair.max_len().saturating_sub(1))
        );
        self.notice = Some(format!("Copied {text}.").into());
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        cx.notify();
    }
}
