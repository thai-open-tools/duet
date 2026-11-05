//! The line-details pane (R8): the whole line of the current row on both sides, spaces and
//! tabs shown, wrapped at the pane's width, with the inline highlights of a changed row.

use std::borrow::Cow;
use std::ops::Range;

use duet_core::text::rows::Row;
use gpui_kit::{Hsla, StyledText, relative};

use super::*;
use crate::text::display::{DisplayLine, MAX_WRAP_SEGMENTS, display_line, wrap_prefix};
use crate::text::prefs::DETAILS;

/// The label column's width in px.
const LABEL_WIDTH: f32 = 96.;
/// Left and right padding of the pane, and the gap after a label, in px.
const PANE_PAD: f32 = 12.;
const LABEL_GAP: f32 = 8.;
const LABEL_SIZE: f32 = 11.;

/// What the pane shows for one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailsText {
    pub row: u32,
    /// "Left 12", or "Left —" when that side has no line here.
    pub left_label: String,
    pub right_label: String,
    /// The line as shown: spaces as `·`, tabs as `→` and padding.
    pub left: Option<String>,
    pub right: Option<String>,
}

impl TextView {
    pub fn details(&self) -> bool {
        self.prefs.details
    }

    /// Shows or hides the pane and remembers the choice.
    pub fn set_details(&mut self, on: bool, cx: &mut Context<Self>) {
        self.prefs.details = on;
        self.remember(DETAILS, cx);
        cx.notify();
    }

    /// The row the pane describes: the row selection's head, else the caret's line, else the
    /// current difference's first row.
    fn details_row(&self) -> Option<u32> {
        let model = self.model.as_ref()?;
        if let Some(sel) = self.row_sel
            && model.aligned_row(sel.head).is_some()
        {
            return Some(sel.head);
        }
        if let Some(caret) = self.caret
            && let Some(buffer) = model.buffer(caret.left)
            && let Some(row) = model.row_of_line(caret.left, buffer.line_of(caret.head) as u32)
        {
            return Some(row);
        }
        model
            .current_block()
            .and_then(|block| model.block_rows(block))
            .map(|rows| rows.start)
    }

    /// Spaces and tabs always shown, no scroll.
    fn details_options(&self) -> DisplayOptions {
        DisplayOptions {
            tab_width: self.display.tab_width,
            show_whitespace: true,
            skip_columns: 0,
            view_columns: 0,
        }
    }

    /// Test hook: the bytes of each side's line the pane wrapped in the last frame.
    #[doc(hidden)]
    pub fn details_wrapped_for_test(&self) -> [usize; 2] {
        self.details_wrapped
    }

    pub fn details_text(&self) -> Option<DetailsText> {
        if !self.prefs.details {
            return None;
        }
        let row = self.details_row()?;
        let model = self.model.as_ref()?;
        let Row { left, right, .. } = model.aligned_row(row)?;
        let opts = self.details_options();
        let side = |is_left: bool, line: Option<u32>| {
            let label = side_label(is_left, line);
            let text = line.map(|l| display_line(&model.line_text(is_left, l as usize), opts).text);
            (label, text)
        };
        let ((left_label, left), (right_label, right)) = (side(true, left), side(false, right));
        Some(DetailsText {
            row,
            left_label,
            right_label,
            left,
            right,
        })
    }

    /// The pane, or None while it is off, no row applies or the body shows a message.
    /// `width` is the view's width in px.
    pub(super) fn render_details(
        &mut self,
        p: &Palette,
        fonts: &Fonts,
        width: f32,
        advance: Pixels,
    ) -> Option<impl IntoElement> {
        if !self.prefs.details || self.message.is_some() {
            return None;
        }
        let row = self.details_row()?;
        let ignoring = self.request.ignoring();
        let opts = self.details_options();
        let (font_size, row_height) = (self.font_size, self.row_height());
        let model = self.model.as_mut()?;
        let Row { left, right, kind } = model.aligned_row(row)?;
        let inline = match kind {
            RowKind::Changed => model.inline_at(row),
            RowKind::Same if ignoring => model.hidden_inline_at(row),
            _ => None,
        };
        // The text column: the pane's width less its padding, the label and the gap.
        let columns = if width > 0. {
            let text_width = width - 2. * PANE_PAD - LABEL_WIDTH - LABEL_GAP;
            ((text_width / f32::from(advance) + 1e-3).floor() as usize).max(1)
        } else {
            10_000
        };
        // A frame reads only what 200 segments can show, borrowed from a buffer (I3).
        let (model, most) = (&*model, details_cap(columns));
        let text = |is_left: bool, line: Option<u32>| {
            line.map(|l| line_prefix(model, is_left, l as usize, most))
        };
        let (left_text, right_text) = (text(true, left), text(false, right));
        let len = |text: &Option<(Cow<'_, str>, bool)>| text.as_ref().map_or(0, |(t, _)| t.len());
        self.details_wrapped = [len(&left_text), len(&right_text)];
        // Lines that differ only past the prefix show no highlight in it either way.
        let soft = rows::unimportant(
            kind,
            ignoring,
            left_text.as_ref().map(|(t, _)| t.as_ref()),
            right_text.as_ref().map(|(t, _)| t.as_ref()),
        );
        let colour = |is_left: bool| {
            if soft {
                Some(p.hl_unimp)
            } else if kind == RowKind::Changed {
                Some(if is_left { p.hl_del } else { p.hl_ins })
            } else {
                None
            }
        };
        let wrapped = |text: Option<(Cow<'_, str>, bool)>| {
            text.map(|(text, more)| wrap_prefix(&text, more, opts, columns))
        };
        let (left_lines, right_lines) = (wrapped(left_text), wrapped(right_text));
        // Each block's label keeps to the top of the rows while the block's rows are there, so
        // every row on screen says which side it is: the two blocks of a long line that is the
        // same on both sides are the same text (X-32, X-64). A block is its lines' height.
        let height = |lines: &Option<Vec<DisplayLine>>| {
            lines.as_ref().map_or(1, Vec::len).max(1) as f32 * row_height
        };
        let (left_height, right_height) = (height(&left_lines), height(&right_lines));
        let shown = f32::from(self.details_scroll.bounds().size.height);
        let most = (left_height + right_height - shown).max(0.);
        let scrolled = f32::from(-self.details_scroll.offset().y).clamp(0., most);
        let drop =
            |top: f32, height: f32| (scrolled - top).clamp(0., (height - row_height).max(0.));
        let drops = [drop(0., left_height), drop(left_height, right_height)];
        let block = |is_left: bool, line: Option<u32>, lines: Option<Vec<DisplayLine>>| {
            let label = side_label(is_left, line);
            let ranges = inline
                .as_ref()
                .map(|d| if is_left { &d.left } else { &d.right });
            let lines = lines.map(|lines| {
                lines
                    .into_iter()
                    .map(|shown| detail_line(shown, ranges, colour(is_left), row_height))
                    .collect::<Vec<_>>()
            });
            h_flex()
                .id(if is_left {
                    "text-details-left"
                } else {
                    "text-details-right"
                })
                .test_support()
                // As tall as its lines, so a pane over its cap scrolls instead of squeezing
                // the blocks; each clips its own text, so the two never draw over each other
                // (X-32, X-64).
                .flex_none()
                .overflow_hidden()
                .items_start()
                .gap(px(LABEL_GAP))
                .min_h(px(row_height))
                .child(
                    div()
                        .id(if is_left {
                            "text-details-left-label"
                        } else {
                            "text-details-right-label"
                        })
                        .test_support()
                        .flex_none()
                        .mt(px(drops[usize::from(!is_left)]))
                        .w(px(LABEL_WIDTH))
                        .text_size(px(LABEL_SIZE))
                        .line_height(px(row_height))
                        .text_color(p.dim)
                        .child(label),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .font(fonts.code.clone())
                        .text_size(px(font_size))
                        .line_height(px(row_height))
                        .children(lines.into_iter().flatten()),
                )
        };
        Some(
            v_flex()
                .id("text-details-pane")
                .test_support()
                .flex_none()
                .max_h(relative(0.3))
                .px(px(PANE_PAD))
                .py(px(4.))
                .bg(p.panel2)
                .border_t_1()
                .border_color(p.line)
                .child(
                    // The rows scroll inside the padding: a row half scrolled out is cut there,
                    // not at the pane's edge against the list's last row.
                    v_flex()
                        .id("text-details-scroll")
                        .test_support()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.details_scroll)
                        .child(block(true, left, left_lines))
                        .child(block(false, right, right_lines)),
                ),
        )
    }
}

/// The most bytes of a line the pane wraps at `columns` columns: one segment past
/// `MAX_WRAP_SEGMENTS`, at up to 4 bytes a column.
fn details_cap(columns: usize) -> usize {
    (MAX_WRAP_SEGMENTS + 1) * columns * 4
}

/// Where `bytes` (UTF-8) end when cut to at most `max` bytes at a character boundary.
fn cut_at(bytes: &[u8], max: usize) -> usize {
    if bytes.len() <= max {
        return bytes.len();
    }
    let mut end = max;
    while end > 0 && (bytes[end] & 0xC0) == 0x80 {
        end -= 1;
    }
    end
}

/// At most `max` bytes of `line` of one side, cut at a character boundary, and whether more of
/// the line follows. A buffer's line is borrowed; a file side decodes only the prefix, as
/// `TextDocument::line` decodes the whole line (I3).
fn line_prefix(model: &TextModel, left: bool, line: usize, max: usize) -> (Cow<'_, str>, bool) {
    if let Some(buffer) = model.buffer(left) {
        let text = buffer.line(line);
        let end = cut_at(text.as_bytes(), max);
        return (Cow::Borrowed(&text[..end]), end < text.len());
    }
    let doc = if left { model.left() } else { model.right() };
    let bytes = doc.lines().line_bytes(doc.bytes(), line);
    let end = cut_at(bytes, max);
    (String::from_utf8_lossy(&bytes[..end]), end < bytes.len())
}

/// "Left 12", or "Left —" when that side has no line here.
fn side_label(left: bool, line: Option<u32>) -> String {
    let name = tr(if left { "Left" } else { "Right" });
    line.map_or(format!("{name} —"), |l| format!("{name} {}", l + 1))
}

/// One wrapped segment of a details line, its inline highlights laid on it.
fn detail_line(
    shown: DisplayLine,
    ranges: Option<&Vec<Range<usize>>>,
    colour: Option<Hsla>,
    row_height: f32,
) -> impl IntoElement {
    let highlights = colour
        .map(|color| {
            ranges
                .into_iter()
                .flatten()
                .filter_map(|r| shown.map_range(r.clone()))
                .map(|r| {
                    (
                        r,
                        HighlightStyle {
                            background_color: Some(color),
                            ..Default::default()
                        },
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    h_flex()
        .flex_none()
        .h(px(row_height))
        .child(StyledText::new(shown.text.clone()).with_highlights(highlights))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_huge_line_is_cut_at_a_character_boundary() {
        // Each of these characters is 3 bytes.
        let thai = "น้ำ".repeat(1_000);
        for max in [0, 1, 2, 3, 4, 100, 8_999] {
            let end = cut_at(thai.as_bytes(), max);
            assert!(end <= max && thai.is_char_boundary(end), "max {max}: {end}");
            assert!(
                max - end < 3,
                "at most one character back: max {max}, {end}"
            );
        }
        assert_eq!(cut_at(b"short", 100), 5, "a short line is whole");
        assert_eq!(cut_at(b"short", 5), 5);
        assert_eq!(details_cap(10), 201 * 40);
    }
}
