//! The hex view's body: rows of offset | left hex | left ASCII | gutter | right hex | right
//! ASCII under a header ruler, with a vertical scrollbar and the overview strip on the right
//! (R4), and a horizontal scrollbar under the rows when they are wider than the body. Only the
//! rows on screen are built, as plain elements.
//!
//! A row costs a constant handful of elements whatever its width: each column's bytes are one
//! `StyledText` whose highlights mark the differing bytes, plus at most one hatched box over
//! the cells past that side's end. A frame reads one span per side for all the rows it shows.

use std::ops::Range;
use std::rc::Rc;

use duet_core::hex::format::format_offset;
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu};
use gpui_kit::{
    AnyElement, Background, BorderStyle, Div, HighlightStyle, Hsla, MouseButton, MouseDownEvent,
    Point, StyledText, canvas, fill, point, quad, size, transparent_black,
};

use super::*;
use crate::hex::menu::row_menu;
use crate::i18n::tr;
use crate::ui::drag::follow_drag;
use crate::ui::fonts::{CODE_SIZE, Fonts, code_advance};
use crate::ui::menu::fill as fill_menu;
use crate::ui::scroll::{scroll_for_thumb, thumb};
use crate::ui::style::{OVERVIEW_WIDTH, SCROLLBAR_WIDTH, hatch};

/// Space before the offsets, in px.
const EDGE: f32 = 10.;
/// Advances between the offset, hex and ASCII columns.
const GAP: usize = 2;
/// The centre gutter between the sides, which holds ≠ on rows that differ, in px.
const GUTTER: f32 = 30.;
const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteMark {
    Same,
    /// The byte differs from the other side's, or the other side has ended.
    Differs,
    /// Past this side's end, within the longer file: drawn hatched.
    Missing,
}

/// How each byte position of one row is marked on each side.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowMarks {
    pub left: Vec<ByteMark>,
    pub right: Vec<ByteMark>,
}

impl RowMarks {
    /// The row shows a difference: the gutter's ≠.
    pub fn differs(&self) -> bool {
        self.left
            .iter()
            .chain(&self.right)
            .any(|mark| *mark != ByteMark::Same)
    }
}

/// Marks the `width` positions of one row from the bytes each side has there; a side that ends
/// inside the row has fewer. The bytes on screen decide, so the marks hold even past the
/// 200,000 ranges a compare keeps (R6) and before the ranges arrive.
pub fn row_marks(left: &[u8], right: &[u8], width: usize) -> RowMarks {
    let mark = |mine: Option<&u8>, theirs: Option<&u8>| match (mine, theirs) {
        (None, _) => ByteMark::Missing,
        (Some(a), Some(b)) if a == b => ByteMark::Same,
        _ => ByteMark::Differs,
    };
    RowMarks {
        left: (0..width)
            .map(|i| mark(left.get(i), right.get(i)))
            .collect(),
        right: (0..width)
            .map(|i| mark(right.get(i), left.get(i)))
            .collect(),
    }
}

/// The bytes of file row `row`: `bpr` of them, fewer on the longer file's last row.
pub(super) fn row_span(row: u64, bpr: u64, max_len: u64) -> Range<u64> {
    let start = row.saturating_mul(bpr).min(max_len);
    start..start.saturating_add(bpr).min(max_len)
}

/// `len` bytes of `bytes` from `from`, or as many as there are.
fn part(bytes: &[u8], from: usize, len: usize) -> &[u8] {
    let start = from.min(bytes.len());
    &bytes[start..from.saturating_add(len).min(bytes.len())]
}

/// The bytes of one side's row that are selected or under the cursor, as positions in the row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Picked {
    pub(super) range: Option<Range<usize>>,
    pub(super) cursor: Option<usize>,
}

impl Picked {
    fn has(&self, i: usize) -> bool {
        self.cursor == Some(i) || self.range.as_ref().is_some_and(|r| r.contains(&i))
    }
}

/// Column widths from the code font's advance; the rows and the ruler share them.
#[derive(Clone, Copy)]
pub(super) struct Columns {
    advance: f32,
    offset_chars: usize,
    bpr: usize,
}

impl Columns {
    fn new(advance: f32, bpr: u64, max_len: u64) -> Self {
        Self {
            advance,
            offset_chars: format_offset(0, max_len.saturating_sub(1)).len(),
            bpr: bpr as usize,
        }
    }

    fn chars(&self, n: usize) -> Pixels {
        px(n as f32 * self.advance)
    }

    fn hex(&self) -> Pixels {
        self.chars(self.bpr * 3 - 1)
    }

    fn ascii(&self) -> Pixels {
        self.chars(self.bpr)
    }

    /// A whole row as `line` lays it out, with the same space after the right ASCII as before
    /// the offsets.
    fn total(&self) -> f32 {
        let chars = self.offset_chars + 3 * GAP + 2 * (self.bpr * 3 - 1) + 2 * self.bpr;
        2. * EDGE + GUTTER + f32::from(self.chars(chars))
    }

    /// Left edge and width, in px from the row's left edge, of the left hex, left ASCII, right
    /// hex and right ASCII columns, as `line` lays them out.
    fn byte_columns(&self) -> [(f32, f32); 4] {
        let gap = f32::from(self.chars(GAP));
        let (hex, ascii) = (f32::from(self.hex()), f32::from(self.ascii()));
        let left_hex = EDGE + f32::from(self.chars(self.offset_chars)) + gap;
        let left_ascii = left_hex + hex + gap;
        let right_hex = left_ascii + ascii + GUTTER;
        let right_ascii = right_hex + hex + gap;
        [
            (left_hex, hex),
            (left_ascii, ascii),
            (right_hex, hex),
            (right_ascii, ascii),
        ]
    }

    /// The byte under `x` px from the row's left edge: (left side?, position in the row). A
    /// hex cell owns the space after it; the offsets and the gutter hit nothing.
    pub(super) fn hit(&self, x: f32) -> Option<(bool, usize)> {
        for (i, (start, width)) in self.byte_columns().into_iter().enumerate() {
            if x >= start && x < start + width {
                let cell = self.advance * if i % 2 == 0 { 3. } else { 1. };
                let at = (((x - start) / cell) as usize).min(self.bpr - 1);
                return Some((i < 2, at));
            }
        }
        None
    }

    /// The middle of the gutter between the sides, in px from the row's left edge.
    pub(super) fn gutter_center(&self) -> f32 {
        let cols = self.byte_columns();
        (cols[1].0 + cols[1].1 + cols[2].0) / 2.
    }

    /// The middle of byte `at`'s hex cell, in px from the row's left edge.
    pub(super) fn hex_center(&self, left: bool, at: usize) -> f32 {
        let (start, _) = self.byte_columns()[if left { 0 } else { 2 }];
        start + self.advance * (at * 3 + 1) as f32
    }
}

/// The six cells of a row or of the ruler, sized and spaced alike so they line up.
fn line(
    cols: Columns,
    [offset, left_hex, left_ascii, gutter, right_hex, right_ascii]: [Div; 6],
) -> Div {
    let gap = cols.chars(GAP);
    h_flex()
        .flex_none()
        .h(px(ROW_HEIGHT))
        .whitespace_nowrap()
        .child(
            offset
                .flex_none()
                .ml(px(EDGE))
                .w(cols.chars(cols.offset_chars)),
        )
        .child(left_hex.flex_none().ml(gap).w(cols.hex()))
        .child(left_ascii.flex_none().ml(gap).w(cols.ascii()))
        .child(gutter.flex_none().w(px(GUTTER)))
        .child(right_hex.flex_none().w(cols.hex()))
        .child(right_ascii.flex_none().ml(gap).w(cols.ascii()))
}

/// The header ruler: the column offsets over the hex cells and their low digit over the ASCII,
/// scrolled sideways with the rows and clipped to their width.
fn ruler(cols: Columns, skip: Pixels, p: &Palette) -> impl IntoElement {
    let hex = (0..cols.bpr)
        .map(|i| format!("{i:02X}"))
        .collect::<Vec<_>>()
        .join(" ");
    let ascii: String = (0..cols.bpr)
        .map(|i| char::from(HEX_DIGITS[i % 16]))
        .collect();
    let cells = line(
        cols,
        [
            div().child(tr("Offset")),
            div().child(hex.clone()),
            div().child(ascii.clone()),
            div(),
            div().child(hex),
            div().child(ascii),
        ],
    );
    div()
        .id("hex-ruler")
        .test_support()
        .relative()
        .flex_1()
        .min_w_0()
        .h(px(ROW_HEIGHT))
        .overflow_hidden()
        .text_color(p.dim)
        .child(
            cells
                .id("hex-ruler-cells")
                .test_support()
                .absolute()
                .top_0()
                .left(-skip)
                .w(px(cols.total())),
        )
}

/// One side's cells in one row.
struct Cells {
    hex: String,
    ascii: String,
    hex_marks: Vec<(Range<usize>, HighlightStyle)>,
    ascii_marks: Vec<(Range<usize>, HighlightStyle)>,
    /// The bytes this side has in the row; the cells after them, up to `width`, are hatched.
    have: usize,
    width: usize,
    /// A byte of this side differs from the other (not merely picked).
    differs: bool,
    /// The cursor byte's position in the row: it gets a border over its highlight.
    cursor: Option<usize>,
}

/// Both sides' cells for one row of `width` positions: differing bytes on `hl_del` on the left
/// and `hl_ins` on the right, behind the text colour, as text inline highlights are.
fn row_cells(
    left: &[u8],
    right: &[u8],
    width: usize,
    picked: [&Picked; 2],
    p: &Palette,
) -> (Cells, Cells) {
    let marks = row_marks(left, right, width);
    let style = |background| HighlightStyle {
        color: Some(p.text),
        background_color: Some(background),
        ..Default::default()
    };
    (
        cells(left, &marks.left, style(p.hl_del), picked[0], p.sel),
        cells(right, &marks.right, style(p.hl_ins), picked[1], p.sel),
    )
}

/// The text of one side's bytes, with each run of differing bytes highlighted in `style` and
/// the picked bytes in `sel`: over a difference, `sel` is laid over its colour so both show.
fn cells(
    bytes: &[u8],
    marks: &[ByteMark],
    style: HighlightStyle,
    picked: &Picked,
    sel: Hsla,
) -> Cells {
    let have = bytes.len().min(marks.len());
    let mut hex = String::with_capacity(have * 3);
    let mut ascii = String::with_capacity(have);
    for (i, &b) in bytes[..have].iter().enumerate() {
        if i > 0 {
            hex.push(' ');
        }
        hex.push(char::from(HEX_DIGITS[usize::from(b >> 4)]));
        hex.push(char::from(HEX_DIGITS[usize::from(b & 0xF)]));
        ascii.push(if (0x20..=0x7E).contains(&b) {
            char::from(b)
        } else {
            '.'
        });
    }
    let picked_style = |background_color| HighlightStyle {
        background_color: Some(background_color),
        ..style
    };
    // Bit 0: differs, bit 1: picked. One highlight per run of equal bytes, so a row costs a
    // handful of ranges however wide it is.
    let looks = [
        None,
        Some(style),
        Some(picked_style(sel)),
        Some(picked_style(
            style.background_color.map_or(sel, |b| b.blend(sel)),
        )),
    ];
    let look =
        |i: usize| usize::from(marks[i] == ByteMark::Differs) | usize::from(picked.has(i)) << 1;
    let (mut hex_marks, mut ascii_marks) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < have {
        let (start, kind) = (i, look(i));
        while i < have && look(i) == kind {
            i += 1;
        }
        if let Some(run) = looks[kind] {
            // A run reads as one highlight, the spaces inside it included.
            hex_marks.push((start * 3..i * 3 - 1, run));
            ascii_marks.push((start..i, run));
        }
    }
    Cells {
        hex,
        ascii,
        hex_marks,
        ascii_marks,
        have,
        width: marks.len(),
        differs: marks[..have].contains(&ByteMark::Differs),
        cursor: picked.cursor.filter(|&at| at < have),
    }
}

/// A column's text over an optional hatched box and under an optional cursor border: an id,
/// the left edge, the width and the colour.
fn column(
    text: String,
    marks: Vec<(Range<usize>, HighlightStyle)>,
    hatch: Option<(Pixels, Pixels)>,
    cursor: Option<(&'static str, Pixels, Pixels, Hsla)>,
    bg: Background,
) -> Div {
    div()
        .relative()
        .h_full()
        .when_some(hatch, |d, (left, width)| {
            d.child(div().absolute().top_0().left(left).w(width).h_full().bg(bg))
        })
        .child(StyledText::new(text).with_highlights(marks))
        .when_some(cursor, |d, (id, left, width, accent)| {
            d.child(
                div()
                    .id(id)
                    .test_support()
                    .absolute()
                    .top_0()
                    .left(left)
                    .w(width)
                    .h_full()
                    .border_1()
                    .border_color(accent),
            )
        })
}

impl Cells {
    /// A byte of this side differs or is missing: the row's gutter shows ≠.
    fn shows_difference(&self) -> bool {
        self.differs || self.have < self.width
    }

    /// The hatched boxes (left edge and width) over the hex and the ASCII cells past this
    /// side's end, within the row.
    fn hatches(&self, cols: Columns) -> Option<((Pixels, Pixels), (Pixels, Pixels))> {
        let missing = self.width - self.have;
        (missing > 0).then(|| {
            (
                (cols.chars(self.have * 3), cols.chars(missing * 3 - 1)),
                (cols.chars(self.have), cols.chars(missing)),
            )
        })
    }

    /// The hex and the ASCII column of this side.
    fn columns(self, cols: Columns, bg: Background, accent: Hsla) -> (Div, Div) {
        let (hex_hatch, ascii_hatch) = self.hatches(cols).unzip();
        let hex_cursor = self
            .cursor
            .map(|i| ("hex-cursor-hex", cols.chars(i * 3), cols.chars(2), accent));
        let ascii_cursor = self
            .cursor
            .map(|i| ("hex-cursor-ascii", cols.chars(i), cols.chars(1), accent));
        (
            column(self.hex, self.hex_marks, hex_hatch, hex_cursor, bg),
            column(self.ascii, self.ascii_marks, ascii_hatch, ascii_cursor, bg),
        )
    }
}

/// The block marks of the overview strip for one strip height and row mapping.
pub(super) struct StripMarks {
    key: (u32, u64, bool),
    marks: Rc<Vec<(f32, f32)>>,
}

/// Merges spans of the strip (top and bottom as fractions) per pixel row of a strip `height`
/// px tall, so a strip gets at most one mark per pixel row however many blocks there are.
fn strip_marks(spans: impl Iterator<Item = (f64, f64)>, height: f32) -> Vec<(f32, f32)> {
    let h = f64::from(height);
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (top, bottom) in spans {
        let top = (top * h).floor();
        let bottom = (bottom * h).ceil().max(top + 1.);
        match out.last_mut() {
            Some(last) if top <= last.1 => last.1 = last.1.max(bottom),
            _ => out.push((top, bottom)),
        }
    }
    out.into_iter()
        .map(|(top, bottom)| ((top / h) as f32, (bottom / h) as f32))
        .collect()
}

impl HexView {
    /// The ruler over the rows, with the scrollbars and the overview strip beside them.
    pub(super) fn render_body(
        &mut self,
        pair: &HexPair,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        // The last frame measured the page; a taller window keeps the last page full.
        self.top = self.top.min(self.max_top());
        let fonts = Fonts::current(cx);
        let advance = f32::from(code_advance(window, &fonts.code));
        let cols = Columns::new(advance, self.bytes_per_row, pair.max_len());
        // A wider window or fewer bytes per row can leave less to scroll.
        self.content_width = cols.total();
        self.cols = Some(cols);
        self.skip_x = self.skip_x.clamp(0., self.max_skip_x());
        let skip = px(self.skip_x);
        let rows = self.frame_rows(pair, cols, p, window);
        if !rows.is_empty() && !self.first_rows_logged {
            self.log_first_rows(window);
        }
        let body = self.body_bounds.clone();
        v_flex()
            .flex_1()
            .min_h_0()
            .font(fonts.code)
            .text_size(px(CODE_SIZE))
            .line_height(px(ROW_HEIGHT))
            .child(
                h_flex()
                    .flex_none()
                    .bg(p.panel2)
                    .border_b_1()
                    .border_color(p.line2)
                    .child(ruler(cols, skip, p))
                    .child(
                        div()
                            .flex_none()
                            .w(px(SCROLLBAR_WIDTH + OVERVIEW_WIDTH))
                            .h(px(ROW_HEIGHT)),
                    ),
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
                                    .id("hex-rows")
                                    .test_support()
                                    .relative()
                                    .flex_1()
                                    .min_h_0()
                                    .w_full()
                                    .overflow_hidden()
                                    .bg(p.panel)
                                    // A right press first forgets the last menu target, then
                                    // takes the byte under it, before the popup is built.
                                    .capture_any_mouse_down(cx.listener(
                                        |this, e: &MouseDownEvent, window, cx| {
                                            if e.button == MouseButton::Right {
                                                this.press_right(e, window, cx);
                                            }
                                        },
                                    ))
                                    .on_scroll_wheel(cx.listener(
                                        |this, e: &ScrollWheelEvent, _, cx| this.on_wheel(e, cx),
                                    ))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, e: &MouseDownEvent, window, cx| {
                                            this.press_rows(e, window, cx);
                                        }),
                                    )
                                    // `visible_rows` and the horizontal room come from these
                                    // bounds, so a new size needs one more frame.
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
                                    .child(
                                        v_flex()
                                            .absolute()
                                            .top_0()
                                            .left(-skip)
                                            .w(px(cols.total()))
                                            .children(rows),
                                    )
                                    .context_menu({
                                        let (view, focus) =
                                            (cx.entity().downgrade(), self.focus.clone());
                                        move |menu, _, cx: &mut Context<PopupMenu>| {
                                            let entries = view
                                                .upgrade()
                                                .map(|v| row_menu(v.read(cx)))
                                                .unwrap_or_default();
                                            fill_menu(menu, entries, Some(focus.clone()))
                                        }
                                    }),
                            )
                            .children(self.render_hscroll(p, cx)),
                    )
                    .child(self.render_vscroll(p, cx))
                    .child(self.render_overview(p, cx)),
            )
    }

    /// The rows on screen: the page plus one partly shown below it.
    fn frame_rows(
        &self,
        pair: &HexPair,
        cols: Columns,
        p: &Palette,
        window: &Window,
    ) -> Vec<AnyElement> {
        let bpr = self.bytes_per_row;
        let max_len = pair.max_len();
        let end = self
            .top
            .saturating_add(self.visible_rows() + 1)
            .min(self.row_count());
        let shown: Vec<u64> = (self.top..end).filter_map(|d| self.file_row(d)).collect();
        let bg = hatch(p, window);
        let last_offset = max_len.saturating_sub(1);
        let mut out = Vec::with_capacity(shown.len());
        // One read per side for each run of consecutive rows: the whole page unless only the
        // rows that differ are shown.
        for run in shown.chunk_by(|a, b| *b == a + 1) {
            let (first, last) = (run[0], run[run.len() - 1]);
            let span = row_span(first, bpr, max_len).start..row_span(last, bpr, max_len).end;
            let left = pair.left.bytes(span.clone());
            let right = pair.right.bytes(span.clone());
            for &row in run {
                let at = row_span(row, bpr, max_len);
                let from = (at.start - span.start) as usize;
                let width = (at.end - at.start) as usize;
                let (l, r) = row_cells(
                    part(&left, from, width),
                    part(&right, from, width),
                    width,
                    [&self.picked_in(true, &at), &self.picked_in(false, &at)],
                    p,
                );
                let differs = l.shows_difference() || r.shows_difference();
                let (left_hex, left_ascii) = l.columns(cols, bg, p.accent);
                let (right_hex, right_ascii) = r.columns(cols, bg, p.accent);
                let gutter = div()
                    .flex()
                    .justify_center()
                    .text_color(p.modified)
                    .when(differs, |d| d.child("≠"));
                let offset = div()
                    .text_color(p.muted)
                    .child(format_offset(at.start, last_offset));
                out.push(
                    line(
                        cols,
                        [offset, left_hex, left_ascii, gutter, right_hex, right_ascii],
                    )
                    .id(("hex-row", out.len()))
                    .test_support()
                    .into_any_element(),
                );
            }
        }
        out
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

    /// The custom vertical scrollbar: the thumb maps the top row to a fraction of the rows.
    fn render_vscroll(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let (rows, visible, top) = (self.row_count(), self.visible_rows(), self.top);
        let cell = self.vscroll_bounds.clone();
        let (track, thumb_color) = (p.panel, p.line);
        let dragging = self.dragging_vscroll.then(|| cx.entity().downgrade());
        div()
            .id("hex-vscroll")
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

    /// The left button went down at `y` on the scrollbar. On the thumb, the thumb is held by
    /// that point and nothing moves yet; on the track, the thumb's middle jumps there.
    fn press_vscroll(&mut self, y: Pixels, cx: &mut Context<Self>) {
        let (rows, visible) = (self.row_count(), self.visible_rows());
        if rows <= visible {
            return;
        }
        let bounds = self.vscroll_bounds.get();
        self.dragging_vscroll = true;
        let at = f32::from(y - bounds.top());
        let (top, height) = thumb(f32::from(bounds.size.height), rows, visible, self.top);
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
        let row = scroll_for_thumb(
            f32::from(bounds.size.height),
            self.row_count(),
            self.visible_rows(),
            f32::from(y - bounds.top()) - self.vscroll_grab,
        );
        self.scroll_to_row(row, cx);
    }

    /// The px of a row and of the body the horizontal bar maps; None while a row fits.
    fn hscroll_range(&self) -> Option<(u64, u64)> {
        let body = f32::from(self.body_bounds.get().size.width);
        (body > 0. && self.content_width > body)
            .then(|| (self.content_width.ceil() as u64, body.floor() as u64))
    }

    /// The horizontal scrollbar under the rows, while a row is wider than the body.
    fn render_hscroll(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (total, shown) = self.hscroll_range()?;
        let at = self.skip_x.round() as u64;
        let cell = self.hscroll_bounds.clone();
        let (track, thumb_color) = (p.panel, p.line);
        let dragging = self.dragging_hscroll.then(|| cx.entity().downgrade());
        Some(
            div()
                .id("hex-hscroll")
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
            self.skip_x.round() as u64,
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

    /// The overview strip: the blocks of the whole file and a box around the rows on screen.
    fn render_overview(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let marks = self.strip_marks_for(f32::from(self.overview_bounds.get().size.height));
        let (rows, visible, top) = (self.row_count(), self.visible_rows(), self.top);
        let viewport = (rows > 0).then(|| {
            let fraction = |row: u64| (row.min(rows) as f64 / rows as f64) as f32;
            (fraction(top), fraction(top.saturating_add(visible)))
        });
        let (mark_color, box_fill, box_border) = (p.modified, p.viewport, p.accent);
        let cell = self.overview_bounds.clone();
        let dragging = self.dragging_overview.then(|| cx.entity().downgrade());
        div()
            .id("hex-overview")
            .test_support()
            .flex_none()
            .w(px(OVERVIEW_WIDTH))
            .h_full()
            .bg(p.panel2)
            .border_l_1()
            .border_color(p.line2)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if let Some(fraction) = this.overview_fraction(event.position.y) {
                        this.dragging_overview = true;
                        this.scroll_to_fraction(fraction, cx);
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| end_overview(this, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| end_overview(this, cx)),
            )
            .child(
                canvas(
                    move |bounds, _, _| cell.set(bounds),
                    move |bounds, (), window, _| {
                        let height = bounds.size.height;
                        for &(top, bottom) in marks.iter() {
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(bounds.left() + px(3.), bounds.top() + height * top),
                                    size(
                                        bounds.size.width - px(6.),
                                        (height * (bottom - top)).max(px(2.)),
                                    ),
                                ),
                                mark_color,
                            ));
                        }
                        if let Some((top, bottom)) = viewport {
                            window.paint_quad(quad(
                                Bounds::new(
                                    point(bounds.left() + px(1.), bounds.top() + height * top),
                                    size(
                                        bounds.size.width - px(2.),
                                        (height * (bottom - top)).max(px(4.)),
                                    ),
                                ),
                                px(2.),
                                box_fill,
                                px(1.),
                                box_border,
                                BorderStyle::Solid,
                            ));
                        }
                        if let Some(view) = dragging {
                            follow_drag(view, window, drag_overview, end_overview);
                        }
                    },
                )
                .size_full(),
            )
    }

    /// The strip's marks for a strip `height` px tall, kept until the height, the ranges or
    /// the row mapping change: a compare can hold 200,000 blocks.
    fn strip_marks_for(&mut self, height: f32) -> Rc<Vec<(f32, f32)>> {
        let key = (height as u32, self.bytes_per_row, self.only_diff);
        if let Some(cached) = &self.overview_marks
            && cached.key == key
        {
            return cached.marks.clone();
        }
        let rows = self.row_count();
        let marks = if rows == 0 || height < 1. {
            Vec::new()
        } else {
            let bpr = self.bytes_per_row;
            let fraction =
                |row: u64| self.display_row(row).unwrap_or(rows).min(rows) as f64 / rows as f64;
            let spans = self.blocks.iter().filter(|b| b.start < b.end).map(|b| {
                let last = (b.end - 1) / bpr;
                (fraction(b.start / bpr), fraction(last.saturating_add(1)))
            });
            strip_marks(spans, height)
        };
        let marks = Rc::new(marks);
        self.overview_marks = Some(StripMarks {
            key,
            marks: marks.clone(),
        });
        marks
    }

    fn overview_fraction(&self, y: Pixels) -> Option<f32> {
        let bounds = self.overview_bounds.get();
        (bounds.size.height > px(0.)).then(|| (y - bounds.top()) / bounds.size.height)
    }

    /// Scrolls so the row under `fraction` of the overview strip is in the middle of the page.
    fn scroll_to_fraction(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let rows = self.row_count();
        if rows == 0 {
            return;
        }
        let row = ((f64::from(fraction.clamp(0., 1.)) * rows as f64) as u64).min(rows - 1);
        self.scroll_to_row(row.saturating_sub(self.visible_rows() / 2), cx);
    }
}

fn drag_vscroll(view: &mut HexView, at: Point<Pixels>, cx: &mut Context<HexView>) {
    view.vscroll_to(at.y, cx);
}

fn end_vscroll(view: &mut HexView, cx: &mut Context<HexView>) {
    view.dragging_vscroll = false;
    cx.notify();
}

fn drag_hscroll(view: &mut HexView, at: Point<Pixels>, cx: &mut Context<HexView>) {
    view.hscroll_to(at.x, cx);
}

fn end_hscroll(view: &mut HexView, cx: &mut Context<HexView>) {
    view.dragging_hscroll = false;
    cx.notify();
}

fn drag_overview(view: &mut HexView, at: Point<Pixels>, cx: &mut Context<HexView>) {
    if let Some(fraction) = view.overview_fraction(at.y) {
        view.scroll_to_fraction(fraction, cx);
    }
}

fn end_overview(view: &mut HexView, cx: &mut Context<HexView>) {
    view.dragging_overview = false;
    cx.notify();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_compare_the_bytes_and_hatch_past_a_sides_end() {
        let marks = row_marks(b"abcd", b"abXdEF", 6);
        use ByteMark::{Differs as D, Missing as M, Same as S};
        assert_eq!(marks.left, [S, S, D, S, M, M]);
        assert_eq!(marks.right, [S, S, D, S, D, D]);
        assert!(marks.differs());
        assert!(!row_marks(b"ab", b"ab", 2).differs());
    }

    #[test]
    fn cells_highlight_runs_and_leave_the_missing_tail_to_the_hatch() {
        let marks = row_marks(b"\x00\x41\x42\x7F", b"\x00\x00\x00\x7F\x10", 5);
        let style = HighlightStyle::default();
        let left = cells(
            b"\x00\x41\x42\x7F",
            &marks.left,
            style,
            &Picked::default(),
            Palette::dark().sel,
        );
        assert_eq!(left.hex, "00 41 42 7F");
        assert_eq!(left.ascii, ".AB.");
        assert_eq!(
            left.hex_marks,
            [(3..8, style)],
            "one run, its inner space included"
        );
        assert_eq!(left.ascii_marks, [(1..3, style)]);
        assert_eq!((left.have, left.width), (4, 5));
    }

    #[test]
    fn each_side_highlights_in_its_colour_and_hatches_past_its_end() {
        let p = Palette::dark();
        let none = Picked::default();
        let (left, right) = row_cells(b"ab", b"aXc", 3, [&none, &none], &p);
        assert_eq!(left.hex_marks[0].1.background_color, Some(p.hl_del));
        assert_eq!(left.ascii_marks[0].1.background_color, Some(p.hl_del));
        assert_eq!(right.hex_marks[0].1.background_color, Some(p.hl_ins));
        assert_eq!(right.ascii_marks[0].1.background_color, Some(p.hl_ins));
        assert_eq!(left.hex_marks[0].1.color, Some(p.text));
        let cols = Columns {
            advance: 10.,
            offset_chars: 8,
            bpr: 16,
        };
        // The left side ends after two bytes: its third cell is hatched, "XX" and one char.
        assert_eq!(
            left.hatches(cols),
            Some(((px(60.), px(20.)), (px(20.), px(10.))))
        );
        assert_eq!(right.hatches(cols), None);
        // A side with no bytes in the row is hatched across it.
        let (gone, _) = row_cells(b"", b"abcd", 4, [&none, &none], &p);
        assert_eq!(
            gone.hatches(cols),
            Some(((px(0.), px(110.)), (px(0.), px(40.))))
        );
    }

    #[test]
    fn picked_bytes_never_make_an_equal_row_show_a_difference() {
        let p = Palette::dark();
        let same = row_marks(b"abcdef", b"abcdef", 6);
        let picked = Picked {
            range: Some(1..4),
            cursor: Some(2),
        };
        let left = cells(
            b"abcdef",
            &same.left,
            HighlightStyle::default(),
            &picked,
            p.sel,
        );
        assert!(!left.hex_marks.is_empty(), "the selection is drawn");
        assert!(!left.shows_difference());
        let other = row_marks(b"abcdef", b"abXdef", 6);
        let hit = cells(
            b"abcdef",
            &other.left,
            HighlightStyle::default(),
            &picked,
            p.sel,
        );
        assert!(hit.shows_difference());
    }

    #[test]
    fn picked_bytes_take_the_selection_colour_and_keep_a_difference_visible() {
        let p = Palette::dark();
        let marks = row_marks(b"abcdef", b"abXdef", 6);
        let style = HighlightStyle {
            color: Some(p.text),
            background_color: Some(p.hl_del),
            ..Default::default()
        };
        // Bytes 1..4 are selected and byte 2 is also the cursor; byte 2 differs.
        let picked = Picked {
            range: Some(1..4),
            cursor: Some(2),
        };
        let left = cells(b"abcdef", &marks.left, style, &picked, p.sel);
        let ranges: Vec<_> = left.hex_marks.iter().map(|m| m.0.clone()).collect();
        assert_eq!(
            ranges,
            [3..5, 6..8, 9..11],
            "selected, differing and selected, selected: one range each"
        );
        let bg = |i: usize| left.hex_marks[i].1.background_color;
        assert_eq!(bg(0), Some(p.sel));
        assert_eq!(bg(1), Some(p.hl_del.blend(p.sel)));
        assert_eq!(bg(2), Some(p.sel));
        assert_eq!(left.cursor, Some(2));
        // A cursor with nothing selected is one range of its own, and a missing byte has none.
        let same = row_marks(b"ab", b"ab", 2).left;
        let only = Picked {
            range: None,
            cursor: Some(0),
        };
        let one = cells(b"ab", &same, style, &only, p.sel);
        assert_eq!(one.hex_marks.len(), 1);
        assert_eq!(one.hex_marks[0].0, 0..2);
        let past = Picked {
            range: None,
            cursor: Some(5),
        };
        assert_eq!(cells(b"ab", &same, style, &past, p.sel).cursor, None);
    }

    #[test]
    fn a_click_finds_its_byte_in_the_hex_and_ascii_columns() {
        let cols = Columns {
            advance: 10.,
            offset_chars: 8,
            bpr: 16,
        };
        // Left hex starts at 10 + 80 + 20 = 110; a cell is 3 advances, the last has no space.
        assert_eq!(cols.hit(115.), Some((true, 0)));
        assert_eq!(cols.hit(110. + 25.), Some((true, 0)), "space after a cell");
        assert_eq!(cols.hit(110. + 30.), Some((true, 1)));
        assert_eq!(cols.hit(110. + 469.), Some((true, 15)));
        assert_eq!(cols.hit(109.), None, "between the offsets and the hex");
        // Left ASCII: 110 + 470 + 20 = 600, one advance a byte; the gutter hits nothing.
        assert_eq!(cols.hit(600. + 25.), Some((true, 2)));
        assert_eq!(cols.hit(600. + 165.), None);
        // Right hex: 600 + 160 + 30 = 790; right ASCII: 790 + 470 + 20 = 1280.
        assert_eq!(cols.hit(790. + 31.), Some((false, 1)));
        assert_eq!(cols.hit(1280. + 159.), Some((false, 15)));
        assert_eq!(cols.hit(1280. + 160.), None);
        assert_eq!(cols.hit(-1.), None);
        assert_eq!(cols.hex_center(true, 2), 110. + 70.);
        assert_eq!(cols.hit(cols.hex_center(false, 7)), Some((false, 7)));
    }

    #[test]
    fn row_spans_stop_at_the_longer_files_end_without_overflow() {
        assert_eq!(row_span(0, 16, 6), 0..6);
        assert_eq!(row_span(2, 16, 100), 32..48);
        assert_eq!(row_span(u64::MAX, 32, u64::MAX), u64::MAX..u64::MAX);
        assert_eq!(part(b"abc", 2, 5), b"c");
        assert_eq!(part(b"abc", 9, 5), b"");
    }

    #[test]
    fn a_row_is_as_wide_as_its_cells() {
        let cols = Columns {
            advance: 10.,
            offset_chars: 8,
            bpr: 16,
        };
        // 10 + 80 + 20 + 470 + 20 + 160 + 30 + 470 + 20 + 160 + 10
        assert_eq!(cols.total(), 1450.);
    }

    #[test]
    fn strip_marks_merge_per_pixel_row() {
        let spans = [(0.0, 0.001), (0.002, 0.003), (0.5, 0.6)].into_iter();
        let marks = strip_marks(spans, 100.);
        assert_eq!(
            marks,
            [(0.0, 0.01), (0.5, 0.6)],
            "the first two share pixel row 0"
        );
    }
}
