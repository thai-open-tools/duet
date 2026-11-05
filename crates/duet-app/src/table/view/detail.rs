//! The table view's detail panel: the selected row's head line and both sides' full values,
//! with the characters that changed marked (LocalDiff `renderDetail`).

use std::ops::Range;

use duet_core::table::compare::{RowState, data_offset};
use duet_core::table::nav::side_value;
use duet_core::table::report::record_csv;
use duet_core::text::inline::refine;
use gpui_kit::component::h_flex;
use gpui_kit::{AnyElement, FontWeight, HighlightStyle, Hsla, StyledText};
use unicode_segmentation::GraphemeCursor;

use super::*;
use crate::format::count_of;
use crate::i18n::{tr, tr_key, trf};
use crate::ui::fonts::{CODE_SIZE, SMALL_SIZE};
use crate::ui::style::ld_pill;

pub const DETAIL_HINT: &str = tr_key(
    "Click a cell to see both values in full · N / P or ◀ ▶ move \
    between differing rows · Right-click a column header to use it as the key.",
);
/// How many differing column names the head lists.
const HEAD_COLUMNS: usize = 10;
const DETAIL_HEIGHT: f32 = 92.0;
/// A value longer than this many characters is shown as a window around its first change
/// (T-43). DirectWrite lays out nothing for one line whose style changes after about 32,000
/// characters of one run (gpui-pre-windows 0.3.9 drops the error), and the 92 px strip could
/// not show a change deep in a long value anyway.
const DETAIL_FULL_CHARS: usize = 4_000;
/// The window's length in characters, and how many of them come before the first change.
pub const DETAIL_WINDOW_CHARS: usize = 80;
const DETAIL_WINDOW_BEFORE: usize = 30;

/// One side's value as the panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct DetailSide {
    /// The value in full, or "(none)" / "(empty)".
    pub text: String,
    /// A placeholder: it is drawn muted.
    pub muted: bool,
    /// Byte ranges of `text` that differ from the other side's value (whole graphemes).
    pub marks: Vec<Range<usize>>,
    /// For a value shown as a window: which characters it shows, as "characters
    /// 99,921–100,000 of 100,000".
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    pub state: RowState,
    /// "Same", "Differs", "Left only" or "Right only".
    pub pill: &'static str,
    /// The line after the pill: row numbers, key, column and the differing cells.
    pub head: String,
    pub left: DetailSide,
    pub right: DetailSide,
}

fn pill_text(state: RowState) -> &'static str {
    match state {
        RowState::Same => tr_key("Same"),
        RowState::Differs => tr_key("Differs"),
        RowState::LeftOnly => tr_key("Left only"),
        RowState::RightOnly => tr_key("Right only"),
    }
}

/// "(none)" for a side without a value, "(empty)" for "", else the value.
fn side_of(value: Option<String>) -> DetailSide {
    let (text, muted) = match value {
        None => (tr("(none)").to_owned(), true),
        Some(value) if value.is_empty() => (tr("(empty)").to_owned(), true),
        Some(value) => (value, false),
    };
    DetailSide {
        text,
        muted,
        marks: Vec::new(),
        note: None,
    }
}

// The whole value is the chunk, so the cursor never asks for more context.
fn grapheme_floor(text: &str, at: usize) -> usize {
    let mut cursor = GraphemeCursor::new(at, text.len(), true);
    if cursor.is_boundary(text, 0).unwrap_or(true) {
        return at;
    }
    cursor.prev_boundary(text, 0).ok().flatten().unwrap_or(0)
}

fn grapheme_ceil(text: &str, at: usize) -> usize {
    let mut cursor = GraphemeCursor::new(at, text.len(), true);
    if cursor.is_boundary(text, 0).unwrap_or(true) {
        return at;
    }
    cursor
        .next_boundary(text, 0)
        .ok()
        .flatten()
        .unwrap_or(text.len())
}

/// The byte offset of character `n` of `text`, or its length past the end.
fn char_offset(text: &str, n: usize) -> usize {
    text.char_indices().nth(n).map_or(text.len(), |(at, _)| at)
}

/// Cuts a value over `DETAIL_FULL_CHARS` characters to `DETAIL_WINDOW_CHARS` of them around
/// `anchor` (a byte offset, normally its first mark): from `DETAIL_WINDOW_BEFORE` characters
/// before it, cut at grapheme boundaries, with "…" at each cut end and a note of the range.
/// Marks move with the text; marks outside the window go.
fn window_side(side: &mut DetailSide, anchor: usize) {
    let text = &side.text;
    let total = text.chars().count();
    if side.muted || total <= DETAIL_FULL_CHARS {
        return;
    }
    let mut anchor = anchor.min(text.len());
    while !text.is_char_boundary(anchor) {
        anchor -= 1;
    }
    let anchor_char = text[..anchor].chars().count();
    let first = anchor_char
        .saturating_sub(DETAIL_WINDOW_BEFORE)
        .min(total - DETAIL_WINDOW_CHARS);
    let start = grapheme_floor(text, char_offset(text, first));
    let end = grapheme_ceil(text, char_offset(text, first + DETAIL_WINDOW_CHARS));
    let (head, tail) = (
        if start > 0 { "…" } else { "" },
        if end < text.len() { "…" } else { "" },
    );
    let shown_from = text[..start].chars().count() + 1;
    let shown_to = shown_from - 1 + text[start..end].chars().count();
    let marks = side
        .marks
        .iter()
        .filter(|r| r.start < end && r.end > start)
        .map(|r| r.start.max(start) - start + head.len()..r.end.min(end) - start + head.len())
        .collect();
    side.note = Some(trf(
        "characters {}–{} of {}",
        &[
            &format_size(shown_from as u64),
            &format_size(shown_to as u64),
            &format_size(total as u64),
        ],
    ));
    side.text = format!("{head}{}{tail}", &text[start..end]);
    side.marks = marks;
}

impl TableView {
    /// The panel's content for the selection; None without one.
    fn detail(&self) -> Option<Detail> {
        let diff = self.diff.as_ref()?;
        let (row, column) = self.selection?;
        let paired = diff.rows.get(row)?;
        let header = self.compared_options.header;
        let tables = [
            self.compared[LEFT].as_deref(),
            self.compared[RIGHT].as_deref(),
        ];
        // The file line of a side's row, as LocalDiff's row numbers are.
        let line = |side: usize| {
            let data = if side == LEFT {
                paired.left
            } else {
                paired.right
            }?;
            let table = tables[side]?;
            Some(table.line(data as usize + data_offset(table, header)))
        };
        let number = |side: usize| line(side).map_or("—".to_owned(), |n| n.to_string());
        let mut head = trf(
            "Left row {} · Right row {}",
            &[&number(LEFT), &number(RIGHT)],
        );
        if !diff.keys.is_empty() {
            let key: Vec<&str> = diff
                .keys
                .iter()
                .map(|&c| {
                    let (table, left) = if paired.left.is_some() {
                        (tables[LEFT], true)
                    } else {
                        (tables[RIGHT], false)
                    };
                    side_value(diff, table, header, row, c, left).unwrap_or("")
                })
                .collect();
            head.push_str(&trf(" · Key {}", &[&key.join(" | ")]));
        }
        if let Some(column) = column.and_then(|c| diff.columns.get(c)) {
            head.push_str(&trf(" · Column {}", &[&column.name]));
            if column.only_left() {
                head.push_str(tr(" (left only)"));
            } else if column.only_right() {
                head.push_str(tr(" (right only)"));
            }
        }
        if paired.state == RowState::Differs {
            let differing = diff.diff_columns(row);
            let names: Vec<&str> = differing
                .iter()
                .take(HEAD_COLUMNS)
                .map(|&c| diff.columns[c as usize].name.as_str())
                .collect();
            head.push_str(&format!(
                " · {}: {}{}",
                count_of(differing.len() as u64, "cell differs", "cells differ"),
                names.join(", "),
                if differing.len() > HEAD_COLUMNS {
                    " …"
                } else {
                    ""
                }
            ));
        }
        let value = |side: usize| -> Option<String> {
            let left = side == LEFT;
            match column.filter(|&c| c < diff.columns.len()) {
                Some(c) => side_value(diff, tables[side], header, row, c, left).map(str::to_owned),
                None => {
                    let data = if left { paired.left } else { paired.right }?;
                    let table = tables[side]?;
                    let record = table.record(data as usize + data_offset(table, header));
                    Some(record_csv(record, table.delimiter.char()))
                }
            }
        };
        let (left, right) = (value(LEFT), value(RIGHT));
        let marks = match (&left, &right) {
            (Some(a), Some(b)) if a != b => Some(refine(a, b)),
            _ => None,
        };
        let (mut left, mut right) = (side_of(left), side_of(right));
        if let Some(marks) = marks {
            if !left.muted {
                left.marks = marks.left;
            }
            if !right.muted {
                right.marks = marks.right;
            }
        }
        // A long value shows the part around its first change; a side without a mark (a pure
        // insertion on the other side) shows the same place.
        let firsts = [&left, &right].map(|side| side.marks.first().map(|r| r.start));
        window_side(&mut left, firsts[0].or(firsts[1]).unwrap_or(0));
        window_side(&mut right, firsts[1].or(firsts[0]).unwrap_or(0));
        Some(Detail {
            state: paired.state,
            pill: pill_text(paired.state),
            head,
            left,
            right,
        })
    }

    #[doc(hidden)]
    pub fn detail_for_test(&self) -> Option<Detail> {
        self.detail()
    }

    /// The panel under the body: a hint without a selection, else the head and both values.
    pub(super) fn render_detail(&self, p: &Palette, fonts: &Fonts) -> AnyElement {
        let panel = div()
            .id("table-detail")
            .test_support()
            .flex_none()
            .h(px(DETAIL_HEIGHT))
            .overflow_y_scroll()
            .px(px(12.))
            .py(px(5.))
            .bg(p.panel)
            .border_t_1()
            .border_color(p.line)
            .text_size(px(SMALL_SIZE));
        let Some(detail) = self.detail().filter(|_| self.diff.is_some()) else {
            return panel
                .text_color(p.muted)
                .prose(tr(DETAIL_HINT))
                .into_any_element();
        };
        let (pill_fg, pill_bg) = match detail.state {
            RowState::Same => (p.muted, p.panel2),
            RowState::Differs => (p.modified, p.bg_mod),
            RowState::LeftOnly => (p.del, p.bg_del),
            RowState::RightOnly => (p.ins, p.bg_ins),
        };
        let value = |left: bool, color: Hsla, side: DetailSide, mark: Hsla| {
            let label = tr(if left { "Left" } else { "Right" });
            let marks = side
                .marks
                .into_iter()
                .map(|range| {
                    (
                        range,
                        HighlightStyle {
                            color: Some(p.text),
                            background_color: Some(mark),
                            ..Default::default()
                        },
                    )
                })
                .collect::<Vec<_>>();
            h_flex()
                .items_start()
                .gap(px(8.))
                .child(
                    div()
                        .flex_none()
                        .w(px(36.))
                        .text_color(color)
                        .font_weight(FontWeight::BOLD)
                        .child(label),
                )
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .items_start()
                        .gap(px(8.))
                        .child(
                            div()
                                .id(if left {
                                    "table-detail-left"
                                } else {
                                    "table-detail-right"
                                })
                                .test_support()
                                .min_w_0()
                                .font(fonts.code.clone())
                                .text_size(px(CODE_SIZE))
                                .when(side.muted, |d| d.text_color(p.muted))
                                .child(StyledText::new(side.text).with_highlights(marks)),
                        )
                        .children(side.note.map(|note| {
                            div().flex_none().pt(px(1.)).text_color(p.muted).prose(note)
                        })),
                )
        };
        panel
            .child(
                h_flex()
                    .items_center()
                    .gap(px(8.))
                    .pb(px(3.))
                    .child(ld_pill(tr(detail.pill), pill_fg, pill_bg))
                    .child(div().prose(detail.head)),
            )
            .child(value(true, p.del, detail.left, p.hl_del))
            .child(value(false, p.ins, detail.right, p.hl_ins))
            .into_any_element()
    }
}
