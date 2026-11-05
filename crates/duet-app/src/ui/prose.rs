//! Prose that wraps between phrases in Thai (D5).
//!
//! gpui's line wrapper treats every Thai letter as a place to break, so a plain `.child(text)`
//! can split a word, even leave a vowel or a tone mark at the start of a line. `.prose(text)`
//! draws the same text as pieces, split after each space, and a piece that holds Thai never
//! breaks inside. English is exactly `.child(text)`: the same element tree as before.

use gpui_kit::TestSupportExt as _;
use gpui_kit::component::h_flex;
use gpui_kit::{
    AnyElement, FontWeight, HighlightStyle, Hsla, IntoElement, ParentElement, SharedString, Styled,
    StyledText, div, prelude::*,
};

use crate::i18n::{self, Lang};

/// The longest piece with Thai in it that is drawn without a break. A longer one (a file name in
/// Thai) breaks where it must, rather than run out of its box. `thai_values_are_ready_to_draw`
/// holds every table value to the same length.
const NO_BREAK_CHARS: usize = crate::help::topics::MAX_PIECE_CHARS;

/// The id of each piece Thai prose is drawn in, for the drawn tests.
pub const PIECE_ID: &str = "prose-piece";

pub trait ProseExt: ParentElement + Styled + Sized {
    /// `text` as a wrapping paragraph: `.child(text)` in English, pieces in Thai.
    fn prose(self, text: impl Into<SharedString>) -> Self;
}

impl<T: ParentElement + Styled + Sized> ProseExt for T {
    fn prose(self, text: impl Into<SharedString>) -> Self {
        let text = text.into();
        if i18n::current() == Lang::En {
            return self.child(text);
        }
        self.child(pieces_row(&[(&text, None)]))
    }
}

/// Text with parts in a colour and bold, as one wrapping run: a `StyledText` in English, pieces in
/// Thai. A part with a colour is bold.
pub fn rich_prose(parts: &[(&str, Option<Hsla>)]) -> AnyElement {
    if i18n::current() == Lang::En {
        let mut text = String::new();
        let mut marks = Vec::new();
        for (piece, color) in parts {
            let start = text.len();
            text.push_str(piece);
            if let Some(color) = color {
                marks.push((
                    start..text.len(),
                    HighlightStyle {
                        color: Some(*color),
                        font_weight: Some(FontWeight::BOLD),
                        ..Default::default()
                    },
                ));
            }
        }
        return StyledText::new(text)
            .with_highlights(marks)
            .into_any_element();
    }
    pieces_row(parts).into_any_element()
}

/// Whether `c` is in the Thai block.
fn is_thai(c: char) -> bool {
    ('\u{0E00}'..='\u{0E7F}').contains(&c)
}

/// A wrapping row of the pieces of `parts`, each piece the text up to and including a space. A
/// piece with Thai in it stays whole; any other (a path, a file name) wraps inside the row's width.
fn pieces_row(parts: &[(&str, Option<Hsla>)]) -> impl IntoElement {
    let mut row = h_flex().flex_wrap().items_start().max_w_full();
    let mut index = 0usize;
    for (part, color) in parts {
        // A line break ends the row's line: gpui would otherwise make the piece holding it a
        // two-line cell, and the pieces after it would flow to its right.
        for (n, line) in part.split('\n').enumerate() {
            if n > 0 {
                row = row.child(div().w_full().h_0());
            }
            for piece in line.split_inclusive(' ') {
                let whole = piece.chars().any(is_thai) && piece.chars().count() <= NO_BREAK_CHARS;
                let mut cell = div()
                    .id((PIECE_ID, index))
                    .test_support()
                    .child(SharedString::from(piece.to_owned()));
                index += 1;
                cell = if whole {
                    cell.whitespace_nowrap()
                } else {
                    cell.max_w_full()
                };
                if let Some(color) = color {
                    cell = cell.text_color(*color).font_weight(FontWeight::BOLD);
                }
                row = row.child(cell);
            }
        }
    }
    row
}
