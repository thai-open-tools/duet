//! The table view's grid (A4, R7, R8): two halves side by side under a sticky header, each a
//! row-number gutter and a clipped cell area, with one vertical scrollbar for both halves and
//! one horizontal offset for both halves and the header.
//!
//! Only the rows on screen are built and, in each row, only the columns in view plus one each
//! side. A row is a few elements whatever its width: each half is its row number and one
//! canvas that paints the half's cells at their columns' offsets, one shaped line each, so a
//! frame costs about the same for 5 columns or 5,000. Clicks are hit-tested from the column
//! offsets; cells carry no ids. The viewport and the selection live in `view.rs`.

use std::borrow::Cow;
use std::ops::Range;

use duet_core::table::compare::{RowState, data_offset};
use duet_core::table::{Record, Table};
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu};
use gpui_kit::{
    AnyElement, Background, BorderStyle, ContentMask, Div, Font, FontWeight, Hsla, MouseButton,
    MouseDownEvent, Point, ScrollWheelEvent, ShapedLine, TextAlign, TextRun, TruncateFrom, canvas,
    fill, point, quad, size, transparent_black,
};

use super::*;
use crate::table::menu::{head_menu, row_menu};
use crate::ui::drag::follow_drag;
use crate::ui::fonts::{CODE_SIZE, Fonts, NUMBER_SIZE, SMALL_SIZE, code_advance};
use crate::ui::menu::fill_or_hand_back;
use crate::ui::scroll::{scroll_for_thumb, thumb};
use crate::ui::style::{SCROLLBAR_WIDTH, hatch, ld_pill, ld_tip};

/// The rule between the halves, in px.
const DIVIDER: f32 = 6.;
const HEAD_HEIGHT: f32 = 26.;
/// Space each side of a cell's text, in px.
const CELL_PAD: f32 = 7.;
/// What a column's width adds to its text (R8).
const WIDTH_PAD: f32 = 16.;
const COL_MIN: f32 = 44.;
const COL_MAX: f32 = 320.;
/// Only this many chars of a cell count toward its column's width (R8).
const MEASURED_CHARS: usize = 48;
/// Display text is cut after this many chars (R8).
const SHOWN_CHARS: usize = 300;
/// The rows measured at the top of a side, and how many more are spread over the rest (R8).
const SAMPLE: usize = 250;
/// LocalDiff's `scrollColIntoView` counts at least this much room for the cells.
const MIN_INTO_VIEW: f32 = 200.;
const NO_ROWS: &str = "No rows match this filter.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellMark {
    /// Equal to the other side's cell, or in a row only this side has.
    Same,
    /// A cell of a differing row that differs from the other side's.
    Differs,
    /// The column exists only on this side: shown, never compared.
    OneSidedColumn,
    /// The record is shorter than the column, or this side has no such column: hatched.
    Missing,
    /// This side has no row here: the half is hatched.
    NoRow,
}

/// The columns a cell counts for its column's width (R8): Thai marks above and below count 0,
/// wide chars (from U+2E80) count 2, and only the first 48 chars count.
pub fn text_width(s: &str) -> usize {
    s.chars()
        .take(MEASURED_CHARS)
        .map(|c| match u32::from(c) {
            0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E => 0,
            0x2E80.. => 2,
            _ => 1,
        })
        .sum()
}

/// A cell as the grid shows it (R8): cut after 300 chars with "…", line breaks as ⏎ and tabs
/// as ⇥.
pub fn display_text(s: &str) -> Cow<'_, str> {
    let cut = s.char_indices().nth(SHOWN_CHARS).map(|(at, _)| at);
    let shown = cut.map_or(s, |at| &s[..at]);
    if cut.is_none() && !shown.contains(['\r', '\n', '\t']) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(shown.len() + 3);
    let mut chars = shown.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                chars.next_if_eq(&'\n');
                out.push('⏎');
            }
            '\n' => out.push('⏎'),
            '\t' => out.push('⇥'),
            c => out.push(c),
        }
    }
    if cut.is_some() {
        out.push('…');
    }
    Cow::Owned(out)
}

/// The data rows a side's widths are measured on: the first 250, and 250 more spread over the
/// rest when it has more than 500 (LocalDiff `sampleWidths`).
fn sample_rows(rows: usize) -> impl Iterator<Item = usize> {
    let spread = (rows > 2 * SAMPLE).then(|| {
        let step = (rows - SAMPLE) as f64 / SAMPLE as f64;
        (0..SAMPLE).map(move |i| SAMPLE + (i as f64 * step) as usize)
    });
    (0..rows.min(SAMPLE)).chain(spread.into_iter().flatten())
}

/// A header cell's tooltip: the name, whether it is a key, which side alone has it
/// (`Some(true)` for the left) and how many of its cells differ.
fn head_tip(name: &str, key: bool, only: Option<bool>, diffs: u32) -> String {
    let mut tip = name.to_owned();
    if key {
        tip.push_str(" (key column)");
    }
    match only {
        Some(true) => tip.push_str("\nOnly on the left: not compared."),
        Some(false) => tip.push_str("\nOnly on the right: not compared."),
        None => {}
    }
    match diffs {
        0 => {}
        1 => tip.push_str("\n1 cell differs"),
        n => tip.push_str(&format!("\n{} cells differ", format_size(u64::from(n)))),
    }
    tip
}

/// The record one side shows in paired row `row`, and its index in the table, if that side
/// has the row.
fn side_record<'a>(
    diff: &TableDiff,
    table: Option<&'a Table>,
    header: bool,
    row: usize,
    left: bool,
) -> Option<(usize, Record<'a>)> {
    let table = table?;
    let paired = diff.rows.get(row)?;
    let index =
        (if left { paired.left } else { paired.right })? as usize + data_offset(table, header);
    Some((index, table.record(index)))
}

/// How one side's cell of paired row `row` is marked, and its text ("" when it has none).
fn cell<'a>(
    diff: &TableDiff,
    record: Option<Record<'a>>,
    row: usize,
    column: usize,
    left: bool,
) -> (CellMark, &'a str) {
    let Some(record) = record else {
        return (CellMark::NoRow, "");
    };
    let col = &diff.columns[column];
    let (index, only_here) = if left {
        (col.left, col.only_left())
    } else {
        (col.right, col.only_right())
    };
    // No such column on this side, or a record shorter than it, in any column (LocalDiff
    // `tb-miss`).
    let Some(value) = index.and_then(|index| record.get(index)) else {
        return (CellMark::Missing, "");
    };
    if only_here {
        return (CellMark::OneSidedColumn, value);
    }
    let differs = diff.rows[row].state == RowState::Differs
        && diff
            .diff_columns(row)
            .binary_search(&(column as u32))
            .is_ok();
    let mark = if differs {
        CellMark::Differs
    } else {
        CellMark::Same
    };
    (mark, value)
}

/// Column widths and left edges for one result, in px, and the row-number gutter's width.
pub(super) struct Layout {
    widths: Vec<f32>,
    /// One more than the columns: the last is the total width.
    offsets: Vec<f32>,
    gutter: f32,
}

impl Layout {
    /// Measures the header names (3 chars more for a key column) and the sampled cells of both
    /// sides (R8), with `advance` px per char.
    pub(super) fn new(
        diff: &TableDiff,
        tables: [Option<&Table>; 2],
        header: bool,
        advance: f32,
    ) -> Layout {
        let mut chars: Vec<usize> = diff
            .columns
            .iter()
            .enumerate()
            .map(|(c, col)| {
                let key = if diff.keys.contains(&c) { 3 } else { 0 };
                text_width(&col.left_name).max(text_width(&col.right_name)) + key
            })
            .collect();
        for (left, table) in [true, false].into_iter().zip(tables) {
            let Some(table) = table else {
                continue;
            };
            let offset = data_offset(table, header);
            // (column, field of this side) for the columns this side has.
            let fields: Vec<(usize, usize)> = diff
                .columns
                .iter()
                .enumerate()
                .filter_map(|(c, col)| (if left { col.left } else { col.right }).map(|i| (c, i)))
                .collect();
            for row in sample_rows(table.len() - offset) {
                let record = table.record(row + offset);
                for &(c, field) in &fields {
                    if let Some(value) = record.get(field).filter(|v| !v.is_empty()) {
                        chars[c] = chars[c].max(text_width(value));
                    }
                }
            }
        }
        let widths = chars
            .into_iter()
            .map(|n| ((n as f32 * advance).round() + WIDTH_PAD).clamp(COL_MIN, COL_MAX))
            .collect();
        let last_line = tables
            .iter()
            .flatten()
            .map(|t| t.last_line())
            .max()
            .unwrap_or(0);
        let digits = last_line.to_string().len();
        let gutter = (18. + digits as f32 * advance).round().max(40.);
        Layout::from_widths(widths, gutter)
    }

    fn from_widths(widths: Vec<f32>, gutter: f32) -> Layout {
        let mut offsets = Vec::with_capacity(widths.len() + 1);
        let mut x = 0.;
        offsets.push(x);
        for width in &widths {
            x += width;
            offsets.push(x);
        }
        Layout {
            widths,
            offsets,
            gutter,
        }
    }

    fn total(&self) -> f32 {
        self.offsets[self.widths.len()]
    }

    /// The columns that show in `[x, x + width)` px of the cells, plus one each side.
    fn visible(&self, x: f32, width: f32) -> Range<usize> {
        let n = self.widths.len();
        let first = self.offsets[1..].partition_point(|&end| end <= x);
        let end = (self.offsets[..n].partition_point(|&start| start < x + width) + 1).min(n);
        first.saturating_sub(1).min(end)..end
    }

    /// The column under `x` px of the cells, if any.
    fn column_at(&self, x: f32) -> Option<usize> {
        if x < 0. {
            return None;
        }
        let column = self.offsets[1..].partition_point(|&end| end <= x);
        (column < self.widths.len()).then_some(column)
    }
}

/// What every row and header cell of one frame draws with.
struct Frame<'a> {
    diff: &'a TableDiff,
    layout: &'a Layout,
    tables: [Option<&'a Table>; 2],
    header: bool,
    /// The columns built in each row.
    columns: Range<usize>,
    scroll_x: f32,
    selection: Option<(usize, Option<usize>)>,
    p: &'a Palette,
    fonts: &'a Fonts,
    hatch: Background,
}

/// The rows a frame built, and the cells in them.
struct Built {
    rows: Vec<AnyElement>,
    cells: usize,
    /// The columns built on each half: empty for a half that built no cells.
    columns: [Range<usize>; 2],
}

/// One body cell, as its half-row's canvas paints it.
struct CellPaint {
    /// The left edge (scrolled) and the width, in px from the cell area's left edge.
    x: f32,
    width: f32,
    background: Option<Background>,
    color: Hsla,
    text: SharedString,
    selected: bool,
}

/// One half-row's cells: one canvas paints them all, each clipped to its box, with its text
/// cut to an ellipsis when it is wider. An element per cell would cost a layout node and a
/// text element each, several times a hex row's frame time on a wide table.
fn cells_canvas(cells: Vec<CellPaint>, font: Font, border: Hsla, accent: Hsla) -> impl IntoElement {
    canvas(
        move |_, window, cx| {
            let size = px(CODE_SIZE);
            let mut wrapper = None;
            let lines: Vec<Option<ShapedLine>> = cells
                .iter()
                .map(|cell| {
                    if cell.text.is_empty() {
                        return None;
                    }
                    let run = |len| TextRun {
                        len,
                        font: font.clone(),
                        color: cell.color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let text = &cell.text;
                    let line = window.text_system().shape_line(
                        text.clone(),
                        size,
                        &[run(text.len())],
                        None,
                    );
                    let room = px(cell.width - 2. * CELL_PAD);
                    if line.width() <= room {
                        return Some(line);
                    }
                    let wrapper = wrapper
                        .get_or_insert_with(|| cx.text_system().line_wrapper(font.clone(), size));
                    let (cut, _) = wrapper.truncate_line(
                        text.clone(),
                        room,
                        "…",
                        &[run(text.len())],
                        TruncateFrom::End,
                    );
                    let len = cut.len();
                    Some(
                        window
                            .text_system()
                            .shape_line(cut, size, &[run(len)], None),
                    )
                })
                .collect();
            (cells, lines)
        },
        move |bounds, (cells, lines), window, cx| {
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                for (cell, line) in cells.iter().zip(lines) {
                    let at = Bounds::new(
                        point(bounds.left() + px(cell.x), bounds.top()),
                        size(px(cell.width), bounds.size.height),
                    );
                    if let Some(background) = cell.background {
                        window.paint_quad(fill(at, background));
                    }
                    if let Some(line) = line {
                        window.with_content_mask(Some(ContentMask { bounds: at }), |window| {
                            let origin = point(at.left() + px(CELL_PAD), at.top());
                            let _ = line.paint(
                                origin,
                                at.size.height,
                                TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                        });
                    }
                    let edge = Bounds::new(
                        point(at.right() - px(1.), at.top()),
                        size(px(1.), at.size.height),
                    );
                    window.paint_quad(fill(edge, border));
                    if cell.selected {
                        window.paint_quad(quad(
                            at,
                            px(0.),
                            transparent_black(),
                            px(2.),
                            accent,
                            BorderStyle::Solid,
                        ));
                    }
                }
            });
        },
    )
    .flex_1()
    .min_w_0()
    .h_full()
}

impl Frame<'_> {
    /// The header cells of one half sit in a clipped area right of the row numbers.
    fn head_area(&self, cells: Vec<AnyElement>) -> Div {
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .children(cells)
    }

    /// An absolute box at `column`'s offset, scrolled.
    fn column_box(&self, column: usize) -> Div {
        div()
            .absolute()
            .top_0()
            .left(px(self.layout.offsets[column] - self.scroll_x))
            .w(px(self.layout.widths[column]))
            .h_full()
            .px(px(CELL_PAD))
    }

    fn gutter(&self) -> Div {
        div()
            .flex_none()
            .w(px(self.layout.gutter))
            .h_full()
            .px(px(6.))
            .text_right()
            .text_size(px(NUMBER_SIZE))
            .border_r_1()
            .border_color(self.p.line)
    }

    /// One half of paired row `row`: its file line and the cells in view, or a hatched half
    /// when this side has no row. Returns the half and how many cells it built.
    fn half(&self, row: usize, left: bool) -> (Div, usize) {
        let p = self.p;
        let selected = self.selection.filter(|s| s.0 == row);
        let side = side_record(
            self.diff,
            self.tables[side_index(left)],
            self.header,
            row,
            left,
        );
        let Some((index, record)) = side else {
            let gutter = self.gutter().bg(if selected.is_some() {
                p.accent
            } else {
                p.panel
            });
            let half = h_flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .bg(self.hatch)
                .child(gutter);
            return (half, 0);
        };
        let (bg, number, number_bg) = match self.diff.rows[row].state {
            RowState::Same => (None, p.dim, p.panel),
            RowState::Differs => (Some(p.bg_mod), p.modified, p.ln_mod),
            RowState::LeftOnly => (Some(p.bg_del), p.del, p.ln_del),
            RowState::RightOnly => (Some(p.bg_ins), p.ins, p.ln_ins),
        };
        let (bg, number, number_bg) = match selected {
            Some(_) => (
                Some(bg.map_or(p.sel, |bg| bg.blend(p.sel))),
                p.accent_ink,
                p.accent,
            ),
            None => (bg, number, number_bg),
        };
        let line = self.tables[side_index(left)].map_or(0, |t| t.line(index));
        let selected_column = selected.and_then(|s| s.1);
        let cells: Vec<CellPaint> = self
            .columns
            .clone()
            .map(|column| {
                let (mark, text) = cell(self.diff, Some(record), row, column, left);
                self.body_cell(column, mark, text, left, selected_column == Some(column))
            })
            .collect();
        let built = cells.len();
        let half = h_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .when_some(bg, |d, bg| d.bg(bg))
            .child(
                self.gutter()
                    .bg(number_bg)
                    .text_color(number)
                    .child(line.to_string()),
            )
            .child(cells_canvas(
                cells,
                self.fonts.code.clone(),
                p.line2,
                p.accent,
            ));
        (half, built)
    }

    /// One cell: differing cells on `hl_del` / `hl_ins` behind the text, a one-sided column's
    /// text in its side's colour, a missing cell hatched, the selected cell in an accent box.
    fn body_cell(
        &self,
        column: usize,
        mark: CellMark,
        text: &str,
        left: bool,
        selected: bool,
    ) -> CellPaint {
        let p = self.p;
        let (background, color) = match mark {
            CellMark::Same | CellMark::NoRow => (None, p.text),
            CellMark::Differs => (Some(if left { p.hl_del } else { p.hl_ins }.into()), p.text),
            CellMark::OneSidedColumn => (None, if left { p.del } else { p.ins }),
            CellMark::Missing => (Some(self.hatch), p.text),
        };
        CellPaint {
            x: self.layout.offsets[column] - self.scroll_x,
            width: self.layout.widths[column],
            background,
            color,
            text: display_text(text).into_owned().into(),
            selected,
        }
    }

    /// One half of the header: "#" over the row numbers, then the column names in view.
    fn head(&self, left: bool) -> Div {
        let p = self.p;
        let cells: Vec<AnyElement> = self
            .columns
            .clone()
            .map(|column| self.head_cell(column, left))
            .collect();
        h_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                self.gutter()
                    .font(self.fonts.code.clone())
                    .text_color(p.dim)
                    .bg(p.panel3)
                    .child("#"),
            )
            .child(self.head_area(cells))
    }

    /// A column's name: a key gets a "Key" pill, a one-sided column's name is in its side's
    /// colour, a column with differing cells is bold in `modified`. A column this side lacks
    /// is an empty hatched cell.
    fn head_cell(&self, column: usize, left: bool) -> AnyElement {
        let p = self.p;
        let col = &self.diff.columns[column];
        let cell = self.column_box(column).border_r_1().border_color(p.line);
        let (present, name) = if left {
            (col.left.is_some(), &col.left_name)
        } else {
            (col.right.is_some(), &col.right_name)
        };
        if !present {
            return cell.bg(self.hatch).into_any_element();
        }
        let key = self.diff.keys.contains(&column);
        let only = if col.only_left() {
            Some(true)
        } else if col.only_right() {
            Some(false)
        } else {
            None
        };
        let diffs = self.diff.column_diffs[column];
        let color = match only {
            Some(true) => p.del,
            Some(false) => p.ins,
            None if diffs > 0 => p.modified,
            None => p.text,
        };
        let tip: SharedString = head_tip(name, key, only, diffs).into();
        cell.id((
            if left {
                "table-head-left"
            } else {
                "table-head-right"
            },
            column,
        ))
        .flex()
        .items_center()
        .text_color(color)
        .when(diffs > 0, |d| d.font_weight(FontWeight::BOLD))
        .when(key, |d| {
            d.child(ld_pill("Key", p.accent_ink, p.accent).mr(px(5.)))
        })
        .child(div().min_w_0().truncate().child(name.clone()))
        .tooltip(move |window, cx| ld_tip(tip.clone(), window, cx))
        .into_any_element()
    }
}

impl TableView {
    /// The width of one half, from the last frame.
    fn half_width(&self) -> f32 {
        ((f32::from(self.body_bounds.get().size.width) - DIVIDER) / 2.).max(0.)
    }

    /// Where a drop goes from the left half to the right one, in px from the body's left edge.
    pub(super) fn drop_split(&self) -> f32 {
        self.half_width() + DIVIDER / 2.
    }

    /// The width of one half's cell area, right of the row numbers.
    fn cells_width(&self) -> f32 {
        let gutter = self.layout.as_ref().map_or(0., |l| l.gutter);
        (self.half_width() - gutter).max(0.)
    }

    /// The most the cells can scroll right: what a half is too narrow to show.
    fn max_scroll_x(&self) -> f32 {
        let total = self.layout.as_ref().map_or(0., Layout::total);
        (total - self.cells_width()).max(0.)
    }

    pub(super) fn scroll_x_to(&mut self, x: f32, cx: &mut Context<Self>) {
        let x = x.clamp(0., self.max_scroll_x());
        if x != self.scroll_x {
            self.scroll_x = x;
            cx.notify();
        }
    }

    /// Scrolls sideways just enough that `column` shows whole (LocalDiff `scrollColIntoView`).
    pub(super) fn scroll_column_into_view(&mut self, column: usize, cx: &mut Context<Self>) {
        let Some(layout) = &self.layout else {
            return;
        };
        let (Some(&x), Some(&width)) = (layout.offsets.get(column), layout.widths.get(column))
        else {
            return;
        };
        let room = self.cells_width().max(MIN_INTO_VIEW);
        if x < self.scroll_x {
            self.scroll_x_to(x, cx);
        } else if x + width > self.scroll_x + room {
            self.scroll_x_to(x + width - room, cx);
        }
    }

    /// Which half a window x falls in and the x within it, in px; the rule between the halves
    /// and the scrollbar are in neither. The header's halves line up with the body's.
    fn half_at(&self, x: Pixels) -> Option<(bool, f32)> {
        let x = f32::from(x - self.body_bounds.get().left());
        let half = self.half_width();
        if (0. ..half).contains(&x) {
            Some((true, x))
        } else if (half + DIVIDER..2. * half + DIVIDER).contains(&x) {
            Some((false, x - half - DIVIDER))
        } else {
            None
        }
    }

    /// The column whose name is under a window point on the header, if any (not the "#").
    pub(super) fn head_column(&self, at: Point<Pixels>) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let (_, x) = self.half_at(at.x)?;
        (x >= layout.gutter)
            .then(|| layout.column_at(x - layout.gutter + self.scroll_x))
            .flatten()
    }

    /// The cell under a window point: (paired row, left half?, column). The column is None on
    /// the row numbers and on a half without the row; the rule between the halves hits nothing.
    pub(super) fn hit(&self, at: Point<Pixels>) -> Option<(usize, bool, Option<usize>)> {
        let (diff, layout) = (self.diff.as_ref()?, self.layout.as_ref()?);
        let y = f32::from(at.y - self.body_bounds.get().top());
        if y < 0. {
            return None;
        }
        let row = *self.view.rows.get(self.top + (y / ROW_HEIGHT) as usize)? as usize;
        let (left, x) = self.half_at(at.x)?;
        let paired = diff.rows[row];
        let has_row = if left {
            paired.left.is_some()
        } else {
            paired.right.is_some()
        };
        let column = (has_row && x >= layout.gutter)
            .then(|| layout.column_at(x - layout.gutter + self.scroll_x))
            .flatten();
        Some((row, left, column))
    }

    /// A left press on the rows selects the cell under it; on the row numbers or a half
    /// without the row, the row's first differing column (LocalDiff `onGridClick`).
    pub(super) fn press_rows(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let Some((row, _, column)) = self.hit(event.position) else {
            return;
        };
        let column = column.or_else(|| {
            let diff = self.diff.as_ref()?;
            diff.diff_columns(row).first().map(|&c| c as usize)
        });
        self.select(row, column, false, cx);
    }

    /// The header over the rows, both halves and their scrollbars.
    pub(super) fn render_grid(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let fonts = Fonts::current(cx);
        if self.layout.is_none()
            && let Some(diff) = &self.diff
        {
            let advance = f32::from(code_advance(window, &fonts.code));
            let tables = [
                self.compared[LEFT].as_deref(),
                self.compared[RIGHT].as_deref(),
            ];
            self.layout = Some(Layout::new(
                diff,
                tables,
                self.compared_options.header,
                advance,
            ));
        }
        // The last frame measured the body; a taller or wider window keeps the last page full
        // and leaves less to scroll.
        self.top = self.top.min(self.max_top());
        self.scroll_x = self.scroll_x.clamp(0., self.max_scroll_x());
        let Some((head, built)) = self.frame(p, &fonts, window).map(|frame| {
            let head = [frame.head(true), frame.head(false)];
            (head, self.frame_rows(&frame))
        }) else {
            return div();
        };
        let [head_left, head_right] = head;
        self.built_cells = built.cells;
        self.built_columns = built.columns;
        if !built.rows.is_empty() && !self.first_rows_logged {
            self.log_first_rows(window);
        }
        let body = self.body_bounds.clone();
        v_flex()
            .flex_1()
            .min_h_0()
            .font(fonts.code.clone())
            .text_size(px(CODE_SIZE))
            .line_height(px(ROW_HEIGHT - 1.))
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| this.on_wheel(e, cx)))
            .child(
                h_flex()
                    .id("table-head")
                    .test_support()
                    .flex_none()
                    .h(px(HEAD_HEIGHT))
                    .bg(p.panel2)
                    .border_b_1()
                    .border_color(p.line)
                    .font(fonts.ui.clone())
                    .text_size(px(SMALL_SIZE))
                    .line_height(px(HEAD_HEIGHT - 1.))
                    .child(head_left)
                    .child(div().flex_none().w(px(DIVIDER)).h_full().bg(p.line))
                    .child(head_right)
                    .child(div().flex_none().w(px(SCROLLBAR_WIDTH)).h_full())
                    // A right press on a name sets the key columns from it (LocalDiff).
                    .capture_any_mouse_down(cx.listener(|this, e: &MouseDownEvent, window, cx| {
                        this.press_head(e, window, cx)
                    }))
                    .context_menu({
                        let (view, focus) = (cx.entity().downgrade(), self.focus.clone());
                        move |menu, window, cx: &mut Context<PopupMenu>| {
                            let entries = view
                                .upgrade()
                                .map(|v| head_menu(v.read(cx)))
                                .unwrap_or_default();
                            fill_or_hand_back(menu, entries, focus.clone(), window, cx)
                        }
                    }),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(
                                div()
                                    .id("table-rows")
                                    .test_support()
                                    .relative()
                                    .flex_1()
                                    .min_h_0()
                                    .w_full()
                                    .overflow_hidden()
                                    .bg(p.bg)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, e: &MouseDownEvent, window, cx| {
                                            this.press_rows(e, window, cx);
                                        }),
                                    )
                                    // A right press takes the cell under it before the popup
                                    // is built, so the menu is about that cell.
                                    .capture_any_mouse_down(cx.listener(
                                        |this, e: &MouseDownEvent, window, cx| {
                                            this.press_right(e, window, cx)
                                        },
                                    ))
                                    .context_menu({
                                        let (view, focus) =
                                            (cx.entity().downgrade(), self.focus.clone());
                                        move |menu, window, cx: &mut Context<PopupMenu>| {
                                            let entries = view
                                                .upgrade()
                                                .map(|v| row_menu(v.read(cx)))
                                                .unwrap_or_default();
                                            fill_or_hand_back(
                                                menu,
                                                entries,
                                                focus.clone(),
                                                window,
                                                cx,
                                            )
                                        }
                                    })
                                    // `visible_rows` and the room for the cells come from
                                    // these bounds, so a new size needs one more frame.
                                    .child(
                                        canvas(
                                            move |bounds, window, _| {
                                                if body.replace(bounds).size != bounds.size {
                                                    window.request_animation_frame();
                                                }
                                            },
                                            |_, _, _, _| {},
                                        )
                                        .absolute()
                                        .size_full(),
                                    )
                                    .child(v_flex().w_full().children(built.rows))
                                    // The rule between the halves runs the whole height.
                                    .child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .left_0()
                                            .size_full()
                                            .flex()
                                            .justify_center()
                                            .child(
                                                div()
                                                    .flex_none()
                                                    .w(px(DIVIDER))
                                                    .h_full()
                                                    .bg(p.line),
                                            ),
                                    )
                                    .when(self.view.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .id("table-empty")
                                                .test_support()
                                                .aria_label(NO_ROWS)
                                                .absolute()
                                                .top_0()
                                                .left_0()
                                                .px(px(16.))
                                                .py(px(18.))
                                                .font(fonts.ui.clone())
                                                .text_size(px(UI_SIZE))
                                                .text_color(p.muted)
                                                .child(NO_ROWS),
                                        )
                                    }),
                            )
                            .children(self.render_hscroll(p, cx)),
                    )
                    .child(self.render_vscroll(p, cx)),
            )
    }

    /// What this frame draws with, once a result and its widths are known.
    fn frame<'a>(&'a self, p: &'a Palette, fonts: &'a Fonts, window: &Window) -> Option<Frame<'a>> {
        let (diff, layout) = (self.diff.as_ref()?, self.layout.as_ref()?);
        Some(Frame {
            diff,
            layout,
            tables: [
                self.compared[LEFT].as_deref(),
                self.compared[RIGHT].as_deref(),
            ],
            header: self.compared_options.header,
            columns: layout.visible(self.scroll_x, self.cells_width()),
            scroll_x: self.scroll_x,
            selection: self.selection,
            p,
            fonts,
            hatch: hatch(p, window),
        })
    }

    /// The rows on screen: the page plus one partly shown below it.
    fn frame_rows(&self, frame: &Frame) -> Built {
        let end = (self.top + self.visible_rows() + 1).min(self.view.len());
        let mut built = Built {
            rows: Vec::with_capacity(end.saturating_sub(self.top)),
            cells: 0,
            columns: [0..0, 0..0],
        };
        let mut sides = [false, false];
        for (i, pos) in (self.top..end).enumerate() {
            let row = self.view.rows[pos] as usize;
            let (left, left_cells) = frame.half(row, true);
            let (right, right_cells) = frame.half(row, false);
            built.cells += left_cells + right_cells;
            sides[LEFT] |= left_cells > 0;
            sides[RIGHT] |= right_cells > 0;
            built.rows.push(
                h_flex()
                    .id(("table-row", i))
                    .test_support()
                    .flex_none()
                    .w_full()
                    .h(px(ROW_HEIGHT))
                    .border_b_1()
                    .border_color(frame.p.line2)
                    .child(left)
                    .child(div().flex_none().w(px(DIVIDER)).h_full())
                    .child(right)
                    .into_any_element(),
            );
        }
        for side in [LEFT, RIGHT] {
            if sides[side] {
                built.columns[side] = frame.columns.clone();
            }
        }
        built
    }

    /// `DUET_EXIT_AFTER_FIRST_ROWS`: logs how long the load took to show a full page of rows,
    /// once that frame is drawn, then quits. The first frame only knows the page's height.
    fn log_first_rows(&mut self, window: &mut Window) {
        if self.body_bounds.get().size.height <= px(0.) {
            return;
        }
        self.first_rows_logged = true;
        let started = self.load_started;
        window.on_next_frame(move |_, cx| {
            tracing::info!(ms = started.elapsed().as_millis() as u64, "first rows");
            if std::env::var_os("DUET_EXIT_AFTER_FIRST_ROWS").is_some() {
                cx.quit();
            }
        });
    }

    /// The vertical scrollbar beside both halves: the thumb maps the top row to a fraction of
    /// the display rows.
    fn render_vscroll(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let (rows, visible, top) = (
            self.view.len() as u64,
            self.visible_rows() as u64,
            self.top as u64,
        );
        let cell = self.vscroll_bounds.clone();
        let (track, thumb_color) = (p.panel, p.line);
        let dragging = self.dragging_vscroll.then(|| cx.entity().downgrade());
        div()
            .id("table-vscroll")
            .test_support()
            .flex_none()
            .w(px(SCROLLBAR_WIDTH))
            .h_full()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.press_vscroll(event.position.y, cx);
                }),
            )
            // A quick click's release ends the drag here, before a frame has drawn the
            // window-wide listeners.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| end_vscroll(this, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| end_vscroll(this, cx)),
            )
            .child(
                canvas(
                    move |bounds, _, _| cell.set(bounds),
                    move |bounds, (), window, _| {
                        window.paint_quad(fill(bounds, track));
                        if rows > visible {
                            let (y, height) =
                                thumb(f32::from(bounds.size.height), rows, visible, top);
                            window.paint_quad(quad(
                                Bounds::new(
                                    point(bounds.left() + px(3.), bounds.top() + px(y)),
                                    size(px(5.), px(height)),
                                ),
                                px(3.),
                                thumb_color,
                                px(0.),
                                transparent_black(),
                                BorderStyle::Solid,
                            ));
                        }
                        if let Some(view) = dragging {
                            follow_drag(view, window, drag_vscroll, end_vscroll);
                        }
                    },
                )
                .size_full(),
            )
    }

    /// The left button went down at `y` on the vertical bar. On the thumb, the thumb is held
    /// by that point and nothing moves yet; on the track, the thumb's middle jumps there.
    fn press_vscroll(&mut self, y: Pixels, cx: &mut Context<Self>) {
        let (rows, visible) = (self.view.len() as u64, self.visible_rows() as u64);
        if rows <= visible {
            return;
        }
        let bounds = self.vscroll_bounds.get();
        self.dragging_vscroll = true;
        let at = f32::from(y - bounds.top());
        let (top, height) = thumb(
            f32::from(bounds.size.height),
            rows,
            visible,
            self.top as u64,
        );
        if (top..=top + height).contains(&at) {
            self.vscroll_grab = at - top;
        } else {
            self.vscroll_grab = height / 2.;
            self.vscroll_to(y, cx);
        }
    }

    /// Scrolls so the held thumb's grabbed point is under `y`.
    fn vscroll_to(&mut self, y: Pixels, cx: &mut Context<Self>) {
        let bounds = self.vscroll_bounds.get();
        let pos = scroll_for_thumb(
            f32::from(bounds.size.height),
            self.view.len() as u64,
            self.visible_rows() as u64,
            f32::from(y - bounds.top()) - self.vscroll_grab,
        );
        self.scroll_to_row(pos as usize, cx);
    }

    /// The px of the cells and of one half's cell area the horizontal bar maps; None while
    /// the cells fit.
    fn hscroll_range(&self) -> Option<(u64, u64)> {
        let total = self.layout.as_ref()?.total();
        let shown = self.cells_width();
        (shown > 0. && total > shown).then(|| (total.ceil() as u64, shown.floor() as u64))
    }

    /// The horizontal scrollbar under both halves, while the cells are wider than a half.
    fn render_hscroll(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (total, shown) = self.hscroll_range()?;
        let at = self.scroll_x.round() as u64;
        let cell = self.hscroll_bounds.clone();
        let (track, thumb_color) = (p.panel, p.line);
        let dragging = self.dragging_hscroll.then(|| cx.entity().downgrade());
        Some(
            div()
                .id("table-hscroll")
                .test_support()
                .flex_none()
                .h(px(SCROLLBAR_WIDTH))
                .w_full()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        this.press_hscroll(event.position.x, cx);
                    }),
                )
                // As on the vertical bar: a quick click's release ends the drag here.
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| end_hscroll(this, cx)),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| end_hscroll(this, cx)),
                )
                .child(
                    canvas(
                        move |bounds, _, _| cell.set(bounds),
                        move |bounds, (), window, _| {
                            window.paint_quad(fill(bounds, track));
                            let (x, width) = thumb(f32::from(bounds.size.width), total, shown, at);
                            window.paint_quad(quad(
                                Bounds::new(
                                    point(bounds.left() + px(x), bounds.top() + px(3.)),
                                    size(px(width), px(5.)),
                                ),
                                px(3.),
                                thumb_color,
                                px(0.),
                                transparent_black(),
                                BorderStyle::Solid,
                            ));
                            if let Some(view) = dragging {
                                follow_drag(view, window, drag_hscroll, end_hscroll);
                            }
                        },
                    )
                    .size_full(),
                )
                .into_any_element(),
        )
    }

    /// The left button went down at `x` on the horizontal bar: on the thumb, it is held by
    /// that point; on the track, the thumb's middle jumps there.
    fn press_hscroll(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some((total, shown)) = self.hscroll_range() else {
            return;
        };
        let bounds = self.hscroll_bounds.get();
        self.dragging_hscroll = true;
        let at = f32::from(x - bounds.left());
        let (left, width) = thumb(
            f32::from(bounds.size.width),
            total,
            shown,
            self.scroll_x.round() as u64,
        );
        if (left..=left + width).contains(&at) {
            self.hscroll_grab = at - left;
        } else {
            self.hscroll_grab = width / 2.;
            self.hscroll_to(x, cx);
        }
    }

    /// Scrolls so the held thumb's grabbed point is under `x`.
    fn hscroll_to(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some((total, shown)) = self.hscroll_range() else {
            return;
        };
        let bounds = self.hscroll_bounds.get();
        let skip = scroll_for_thumb(
            f32::from(bounds.size.width),
            total,
            shown,
            f32::from(x - bounds.left()) - self.hscroll_grab,
        );
        self.scroll_x_to(skip as f32, cx);
    }

    /// Test hook: the cells built in the last frame, both halves together.
    #[doc(hidden)]
    pub fn built_cells_for_test(&self) -> usize {
        self.built_cells
    }

    /// Test hook: the columns each half built in the last frame (empty for a half that built
    /// no cells).
    #[doc(hidden)]
    pub fn built_columns_for_test(&self) -> [Range<usize>; 2] {
        self.built_columns.clone()
    }

    /// Test hook: how one side's cell of display row `pos` is marked.
    #[doc(hidden)]
    pub fn cell_mark_for_test(&self, pos: usize, column: usize, left: bool) -> CellMark {
        let (Some(diff), Some(&row)) = (&self.diff, self.view.rows.get(pos)) else {
            return CellMark::NoRow;
        };
        let row = row as usize;
        let table = self.compared[side_index(left)].as_deref();
        let record = side_record(diff, table, self.compared_options.header, row, left).map(|r| r.1);
        cell(diff, record, row, column, left).0
    }

    /// Test hook: the width of `column`, in px, once a frame has measured it (else 0).
    #[doc(hidden)]
    pub fn column_width_for_test(&self, column: usize) -> f32 {
        self.layout
            .as_ref()
            .and_then(|l| l.widths.get(column).copied())
            .unwrap_or(0.)
    }

    /// Test hook: where `column`'s cell on one half is centred, in px from a row's left edge.
    #[doc(hidden)]
    pub fn cell_x_for_test(&self, left: bool, column: usize) -> Option<f32> {
        let layout = self.layout.as_ref()?;
        let half = if left {
            0.
        } else {
            self.half_width() + DIVIDER
        };
        let start = layout.offsets.get(column)?;
        Some(half + layout.gutter + start - self.scroll_x + layout.widths[column] / 2.)
    }
}

fn drag_vscroll(view: &mut TableView, at: Point<Pixels>, cx: &mut Context<TableView>) {
    view.vscroll_to(at.y, cx);
}

fn end_vscroll(view: &mut TableView, cx: &mut Context<TableView>) {
    view.dragging_vscroll = false;
    cx.notify();
}

fn drag_hscroll(view: &mut TableView, at: Point<Pixels>, cx: &mut Context<TableView>) {
    view.hscroll_to(at.x, cx);
}

fn end_hscroll(view: &mut TableView, cx: &mut Context<TableView>) {
    view.dragging_hscroll = false;
    cx.notify();
}

#[cfg(test)]
mod tests {
    use duet_core::cancel::CancelToken;
    use duet_core::table::Delimiter;
    use duet_core::table::compare::{CompareOptions, compare};
    use duet_core::table::parse::parse_str;

    use super::*;

    #[test]
    fn text_width_skips_thai_marks_and_doubles_wide_chars() {
        // Two consonants, each under two marks above: R8 counts 2 (the plan's 4 miscounted).
        assert_eq!(text_width("ที่นี่"), 2);
        assert_eq!(text_width("บ้านปู่"), 4, "marks above and below count 0");
        assert_eq!(text_width("日本"), 4);
        assert_eq!(text_width("abc"), 3);
        assert_eq!(
            text_width(&"a".repeat(100)),
            48,
            "only the first 48 chars count"
        );
        assert_eq!(text_width(&"日".repeat(100)), 96);
        assert_eq!(text_width(""), 0);
    }

    #[test]
    fn long_and_multiline_cells_are_cut_for_display() {
        assert_eq!(display_text("a\r\nb\tc"), "a⏎b⇥c");
        assert_eq!(display_text("a\rb\nc"), "a⏎b⏎c");
        assert!(matches!(display_text("plain"), Cow::Borrowed("plain")));
        let big = "x".repeat(100 * 1024);
        let shown = display_text(&big);
        assert_eq!(shown.chars().count(), 301, "300 chars and the ellipsis");
        assert!(shown.ends_with('…'));
        let thai = "ก".repeat(400);
        assert_eq!(
            display_text(&thai).chars().count(),
            301,
            "cut by chars, not bytes"
        );
    }

    #[test]
    fn sampled_rows_are_the_first_250_and_250_spread_over_the_rest() {
        assert_eq!(sample_rows(3).collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(sample_rows(0).count(), 0);
        assert_eq!(
            sample_rows(400).count(),
            250,
            "up to 500 rows: only the first 250"
        );
        let rows: Vec<usize> = sample_rows(1000).collect();
        assert_eq!(rows.len(), 500);
        assert_eq!(
            &rows[248..252],
            [248, 249, 250, 253],
            "a step of (1000 - 250) / 250"
        );
        assert_eq!(rows.last(), Some(&997));
    }

    fn diff_of(left: &str, right: &str, keys: &str) -> (TableDiff, Table, Table) {
        let (a, b) = (
            parse_str(left, Delimiter::Comma),
            parse_str(right, Delimiter::Comma),
        );
        let options = CompareOptions {
            header: true,
            keys: keys.into(),
            ..Default::default()
        };
        let diff = compare(Some(&a), Some(&b), &options, &CancelToken::new()).unwrap();
        (diff, a, b)
    }

    #[test]
    fn widths_measure_names_keys_and_sampled_cells() {
        let (diff, a, b) = diff_of(
            "id,name,k\n1,ab,1\n2,abcdefghij,2\n",
            "id,name,k,extra\n1,ab,1,日本語\n2,x,2,\n",
            "k",
        );
        let layout = Layout::new(&diff, [Some(&a), Some(&b)], true, 10.);
        // "id": 2 chars → 36, raised to the 44 minimum; "name": the left's 10-char cell; "k":
        // the key's 1 + 3 chars; "extra": the right's three wide chars (6) beat its name (5).
        assert_eq!(layout.widths, [44., 116., 56., 76.]);
        assert_eq!(layout.offsets, [0., 44., 160., 216., 292.]);
        assert_eq!(layout.total(), 292.);
        // The last line is 3 on both sides: one digit.
        assert_eq!(layout.gutter, 40.);
        let wide: String = (0..30).map(|i| format!("{i}\n")).collect();
        let (diff, a, b) = diff_of(&format!("n\n{wide}"), "n\n1\n", "");
        let layout = Layout::new(&diff, [Some(&a), Some(&b)], true, 10.);
        assert_eq!(layout.gutter, 40., "18 + 2 × 10 stays under 40");
        let layout = Layout::new(&diff, [Some(&a), Some(&b)], true, 15.);
        assert_eq!(layout.gutter, 48., "18 + 2 × 15");
    }

    #[test]
    fn the_visible_columns_come_from_the_offsets() {
        let layout = Layout::from_widths(vec![50., 50., 100., 50.], 40.);
        assert_eq!(layout.offsets, [0., 50., 100., 200., 250.]);
        assert_eq!(
            layout.visible(0., 120.),
            0..4,
            "0, 1 and 2 show, plus one after"
        );
        assert_eq!(
            layout.visible(110., 50.),
            1..4,
            "2 shows, plus one each side"
        );
        assert_eq!(layout.visible(200., 100.), 2..4);
        assert_eq!(layout.visible(0., 0.), 0..1);
        assert_eq!(layout.column_at(0.), Some(0));
        assert_eq!(layout.column_at(49.9), Some(0));
        assert_eq!(layout.column_at(50.), Some(1));
        assert_eq!(layout.column_at(249.), Some(3));
        assert_eq!(layout.column_at(250.), None);
        assert_eq!(layout.column_at(-1.), None);
        let empty = Layout::from_widths(Vec::new(), 40.);
        assert_eq!(empty.visible(0., 500.), 0..0);
        assert_eq!(empty.column_at(0.), None);
    }

    #[test]
    fn header_tips_name_the_key_the_side_and_the_count() {
        assert_eq!(head_tip("id", true, None, 0), "id (key column)");
        assert_eq!(
            head_tip("qty", false, Some(true), 0),
            "qty\nOnly on the left: not compared."
        );
        assert_eq!(
            head_tip("qty", false, Some(false), 0),
            "qty\nOnly on the right: not compared."
        );
        assert_eq!(head_tip("v", false, None, 1), "v\n1 cell differs");
        assert_eq!(
            head_tip("v", true, None, 1234),
            "v (key column)\n1,234 cells differ"
        );
    }
}
