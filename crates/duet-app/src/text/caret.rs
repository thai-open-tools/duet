//! The caret and selection in one side's buffer, and how keys move them. Offsets are bytes;
//! horizontal moves go by grapheme cluster, so Thai marks never split from their consonant.

use std::ops::Range;

use duet_core::text::buffer::TextBuffer;
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

use crate::text::display::{columns, walk_columns};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caret {
    pub left: bool,
    pub head: usize,
    pub anchor: usize,
    /// The column (in graphemes) Up/Down aim for.
    pub goal: Option<usize>,
}

impl Caret {
    pub fn at(left: bool, offset: usize) -> Self {
        Self {
            left,
            head: offset,
            anchor: offset,
            goal: None,
        }
    }

    pub fn range(&self) -> Range<usize> {
        self.head.min(self.anchor)..self.head.max(self.anchor)
    }

    pub fn selection(&self) -> Option<Range<usize>> {
        (self.head != self.anchor).then(|| self.range())
    }

    pub fn collapsed(&self, offset: usize) -> Self {
        Self::at(self.left, offset)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    WordLeft,
    WordRight,
    PageUp(u32),
    PageDown(u32),
    DocStart,
    DocEnd,
}

/// The caret's column in graphemes (a tab counts one).
fn column(buffer: &TextBuffer, offset: usize) -> usize {
    let line = buffer.line_of(offset);
    columns(&buffer.text()[buffer.line_start(line)..offset], 1)
}

/// Where grapheme `column` of `line` starts, or the line's end when it is shorter.
fn offset_at_column(buffer: &TextBuffer, line: usize, column: usize) -> usize {
    buffer.line_start(line) + walk_columns(buffer.line(line), 1, column).0
}

fn vertical(buffer: &TextBuffer, caret: Caret, lines: i64) -> (usize, Option<usize>) {
    let goal = caret.goal.unwrap_or_else(|| column(buffer, caret.head));
    let last =
        buffer.line_count().max(1) as i64 - 1 + i64::from(buffer.text().ends_with(['\n', '\r']));
    let line = (buffer.line_of(caret.head) as i64 + lines).clamp(0, last.max(0)) as usize;
    (offset_at_column(buffer, line, goal), Some(goal))
}

/// How far a word move looks from the caret, each way. A word longer than this (a base64 blob
/// in a minified file) is crossed in steps of about this size.
const WORD_WINDOW: usize = 4096;

/// The grapheme boundary at or before byte `at` of `text`.
fn grapheme_floor(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let mut cursor = GraphemeCursor::new(at, text.len(), true);
    match cursor.is_boundary(text, 0) {
        Ok(true) => at,
        _ => cursor.prev_boundary(text, 0).ok().flatten().unwrap_or(0),
    }
}

/// The grapheme boundary at or after byte `at` of `text`.
fn grapheme_ceil(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at += 1;
    }
    let mut cursor = GraphemeCursor::new(at, text.len(), true);
    match cursor.is_boundary(text, 0) {
        Ok(true) => at,
        _ => cursor
            .next_boundary(text, 0)
            .ok()
            .flatten()
            .unwrap_or(text.len()),
    }
}

fn word(buffer: &TextBuffer, offset: usize, forward: bool) -> usize {
    word_within(buffer, offset, forward, WORD_WINDOW)
}

/// `word`, splitting only the `window` bytes each side of the caret into words, so a move on
/// a minified line costs the same as on a short one.
fn word_within(buffer: &TextBuffer, offset: usize, forward: bool, window: usize) -> usize {
    let line = buffer.line_of(offset);
    let (line_start, end) = (buffer.line_start(line), buffer.line_end(line));
    if forward && offset >= end {
        return buffer.next_grapheme(offset);
    }
    if !forward && offset <= line_start {
        return buffer.prev_grapheme(offset);
    }
    let full = buffer.line(line);
    let at = offset - line_start;
    let low = grapheme_floor(full, at.saturating_sub(window));
    let high = grapheme_ceil(full, at.saturating_add(window).min(full.len()));
    let text = &full[low..high];
    let (start, end, at) = (line_start + low, line_start + high, at - low);
    // UAX#29 has no Thai dictionary, so every Thai cluster is its own segment; join a run.
    let is_thai = |s: &str| s.chars().all(|c| (0x0E00..=0x0E7F).contains(&(c as u32)));
    let mut bounds: Vec<(usize, &str)> = Vec::new();
    for (i, s) in text.split_word_bound_indices() {
        match bounds.last_mut() {
            Some((j, prev)) if is_thai(prev) && is_thai(s) => *prev = &text[*j..i + s.len()],
            _ => bounds.push((i, s)),
        }
    }
    let is_word = |s: &str| s.chars().any(char::is_alphanumeric);
    if forward {
        bounds
            .iter()
            .filter(|(i, s)| *i + s.len() > at && is_word(s))
            .map(|(i, s)| start + i + s.len())
            .next()
            .unwrap_or(end)
    } else {
        bounds
            .iter()
            .rev()
            .filter(|(i, s)| *i < at && is_word(s))
            .map(|(i, _)| start + i)
            .next()
            .unwrap_or(start)
    }
}

/// Where `motion` takes the caret. Without `extend`, a horizontal move over a selection
/// collapses it to the edge in that direction.
pub fn moved(buffer: &TextBuffer, caret: Caret, motion: Motion, extend: bool) -> Caret {
    if !extend
        && let Some(range) = caret.selection()
        && matches!(motion, Motion::Left | Motion::Right)
    {
        let to = if motion == Motion::Left {
            range.start
        } else {
            range.end
        };
        return caret.collapsed(to);
    }
    let line = buffer.line_of(caret.head);
    let (head, goal) = match motion {
        Motion::Left => (buffer.prev_grapheme(caret.head), None),
        Motion::Right => (buffer.next_grapheme(caret.head).min(buffer.len()), None),
        Motion::Up => vertical(buffer, caret, -1),
        Motion::Down => vertical(buffer, caret, 1),
        Motion::PageUp(n) => vertical(buffer, caret, -(n as i64)),
        Motion::PageDown(n) => vertical(buffer, caret, n as i64),
        Motion::Home => (buffer.line_start(line), None),
        Motion::End => (buffer.line_end(line), None),
        Motion::WordLeft => (word(buffer, caret.head, false), None),
        Motion::WordRight => (word(buffer, caret.head, true), None),
        Motion::DocStart => (0, None),
        Motion::DocEnd => (buffer.len(), None),
    };
    let anchor = if extend { caret.anchor } else { head };
    Caret {
        left: caret.left,
        head,
        anchor,
        goal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(text: &str) -> TextBuffer {
        TextBuffer::new(text.into())
    }

    #[test]
    fn thai_clusters_move_as_one() {
        let b = buf("ก่ข\n");
        let c = moved(&b, Caret::at(true, 0), Motion::Right, false);
        assert_eq!(c.head, "ก่".len());
        let back = moved(&b, c, Motion::Left, false);
        assert_eq!(back.head, 0);
    }

    #[test]
    fn up_and_down_keep_the_goal_column_across_short_lines() {
        let b = buf("abcdef\nab\nabcdef\n");
        let start = Caret::at(true, 5);
        let down = moved(&b, start, Motion::Down, false);
        assert_eq!(down.head, b.line_start(1) + 2, "clamped to the short line");
        let down2 = moved(&b, down, Motion::Down, false);
        assert_eq!(
            down2.head,
            b.line_start(2) + 5,
            "the goal column comes back"
        );
    }

    #[test]
    fn shift_extends_and_a_plain_move_collapses_the_selection() {
        let b = buf("hello world\n");
        let sel = moved(&b, Caret::at(true, 0), Motion::WordRight, true);
        assert_eq!(sel.selection(), Some(0..5));
        let left = moved(&b, sel, Motion::Left, false);
        assert_eq!(
            (left.head, left.selection()),
            (0, None),
            "Left collapses to the start"
        );
        let end = moved(&b, Caret::at(true, 3), Motion::End, false);
        assert_eq!(end.head, 11, "End stops before the line break");
    }

    #[test]
    fn horizontal_moves_cross_line_breaks_and_crlf() {
        let b = buf("ab\r\ncd");
        let at_end = Caret::at(false, 2);
        assert_eq!(
            moved(&b, at_end, Motion::Right, false).head,
            4,
            "CRLF is one step"
        );
        assert_eq!(moved(&b, Caret::at(false, 4), Motion::Left, false).head, 2);
        assert_eq!(
            moved(&b, Caret::at(false, 0), Motion::DocEnd, false).head,
            6
        );
    }

    #[test]
    fn word_moves_cover_a_whole_thai_run() {
        let b = buf("สวัสดีครับ ab");
        let run = "สวัสดีครับ".len();
        assert_eq!(
            moved(&b, Caret::at(true, 0), Motion::WordRight, false).head,
            run
        );
        assert_eq!(
            moved(&b, Caret::at(true, run), Motion::WordLeft, false).head,
            0
        );
        let m = buf("abcไทย def");
        let first = moved(&m, Caret::at(true, 0), Motion::WordRight, false);
        assert_eq!(first.head, 3);
        assert_eq!(
            moved(&m, first, Motion::WordRight, false).head,
            3 + "ไทย".len()
        );
    }

    /// `word` over the whole line, as it was before it looked at a window around the caret.
    fn reference_word(buffer: &TextBuffer, offset: usize, forward: bool) -> usize {
        let line = buffer.line_of(offset);
        let (start, end) = (buffer.line_start(line), buffer.line_end(line));
        if forward && offset >= end {
            return buffer.next_grapheme(offset);
        }
        if !forward && offset <= start {
            return buffer.prev_grapheme(offset);
        }
        let text = buffer.line(line);
        let at = offset - start;
        let is_thai = |s: &str| s.chars().all(|c| (0x0E00..=0x0E7F).contains(&(c as u32)));
        let mut bounds: Vec<(usize, &str)> = Vec::new();
        for (i, s) in text.split_word_bound_indices() {
            match bounds.last_mut() {
                Some((j, prev)) if is_thai(prev) && is_thai(s) => *prev = &text[*j..i + s.len()],
                _ => bounds.push((i, s)),
            }
        }
        let is_word = |s: &str| s.chars().any(char::is_alphanumeric);
        if forward {
            bounds
                .iter()
                .filter(|(i, s)| *i + s.len() > at && is_word(s))
                .map(|(i, s)| start + i + s.len())
                .next()
                .unwrap_or(end)
        } else {
            bounds
                .iter()
                .rev()
                .filter(|(i, s)| *i < at && is_word(s))
                .map(|(i, _)| start + i)
                .next()
                .unwrap_or(start)
        }
    }

    /// A 64-byte window on a line many windows long: every word is shorter than the window,
    /// so looking only at the window must find what the whole line finds.
    #[test]
    fn word_moves_in_a_window_match_a_whole_line_scan() {
        let line: String = (0..120)
            .map(|i| match i % 4 {
                0 => "word ",
                1 => "สวัสดีครับ, ",
                2 => "ก่ข \t",
                _ => "e\u{301}x👍 ",
            })
            .collect();
        let b = buf(&format!("{line}\nnext line\n"));
        let starts = line.grapheme_indices(true).map(|(i, _)| i);
        for offset in starts.chain([line.len(), line.len() + 1]) {
            for forward in [true, false] {
                assert_eq!(
                    word_within(&b, offset, forward, 64),
                    reference_word(&b, offset, forward),
                    "at {offset}, forward {forward}"
                );
            }
        }
    }

    #[test]
    fn moves_at_the_end_of_a_minified_line_never_walk_the_whole_line() {
        let b = buf(&format!("{} end", "x".repeat(8 << 20)));
        let end = Caret::at(true, b.len());
        let started = std::time::Instant::now();
        let left = moved(&b, end, Motion::WordLeft, false);
        let right = moved(&b, Caret::at(true, b.len() - 2), Motion::WordRight, false);
        let up = moved(&b, end, Motion::Up, false);
        let down = moved(&b, end, Motion::Down, false);
        let took = started.elapsed();
        assert_eq!(left.head, b.len() - 3);
        assert_eq!(
            (right.head, up.head, down.head),
            (b.len(), b.len(), b.len())
        );
        assert!(took < std::time::Duration::from_secs(1), "{took:?}");
    }

    #[test]
    fn word_left_skips_punctuation() {
        let b = buf("foo, bar");
        let first = moved(&b, Caret::at(true, 8), Motion::WordLeft, false);
        assert_eq!(first.head, 5);
        assert_eq!(moved(&b, first, Motion::WordLeft, false).head, 0);
    }
}
