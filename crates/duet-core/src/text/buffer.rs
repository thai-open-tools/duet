//! An editable text: the decoded string with its line index, kept in step by `replace`, which
//! reports what changed in line terms (for the provisional diff) and in byte/point terms (for
//! the incremental re-parse).

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use crate::text::lines::LineIndex;

/// What one `replace` did. Lines `line..line + old_lines` became `line..line + new_lines`;
/// points are (row, byte column) as tree-sitter takes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub line: u32,
    pub old_lines: u32,
    pub new_lines: u32,
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    pub start_point: (usize, usize),
    pub old_end_point: (usize, usize),
    pub new_end_point: (usize, usize),
}

#[derive(Debug, Clone)]
pub struct TextBuffer {
    text: String,
    lines: LineIndex,
    version: u64,
}

impl TextBuffer {
    pub fn new(text: String) -> Self {
        let lines = LineIndex::new(text.as_bytes());
        Self {
            text,
            lines,
            version: 0,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn lines(&self) -> &LineIndex {
        &self.lines
    }

    /// Bumped by every `replace`.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Bytes the text can grow to before it must be copied to a larger allocation.
    pub fn capacity(&self) -> usize {
        self.text.capacity()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line_start(&self, line: usize) -> usize {
        if line < self.lines.len() {
            self.lines.span_with_eol(line).start
        } else {
            self.text.len()
        }
    }

    /// The byte before the line's terminator.
    pub fn line_end(&self, line: usize) -> usize {
        if line < self.lines.len() {
            let start = self.line_start(line);
            start + self.lines.line_bytes(self.text.as_bytes(), line).len()
        } else {
            self.text.len()
        }
    }

    /// The line without its terminator; `line_count()` is the empty line after a final break.
    pub fn line(&self, line: usize) -> &str {
        &self.text[self.line_start(line)..self.line_end(line)]
    }

    /// The line holding `offset`. The offset just past a final break is the virtual last line.
    pub fn line_of(&self, offset: usize) -> usize {
        let line = self.lines.line_of(offset);
        let end = self.line_end(line);
        if line + 1 == self.lines.len() && offset > end && end < self.text.len() {
            // Past the last line's terminator.
            if offset == self.text.len() {
                return self.lines.len();
            }
        }
        line
    }

    /// The break that ends `line`, or the first break in the text when it has none (the
    /// last line), or "\n" for a text without breaks.
    pub fn terminator(&self, line: usize) -> &'static str {
        let break_at = |l: usize| {
            let end = self.line_end(l);
            match &self.text[end..self.line_start(l + 1).max(end)] {
                "\r\n" => Some("\r\n"),
                "\r" => Some("\r"),
                "\n" => Some("\n"),
                _ => None,
            }
        };
        break_at(line)
            .or_else(|| (0..self.lines.len()).find_map(break_at))
            .unwrap_or("\n")
    }

    fn point(&self, offset: usize) -> (usize, usize) {
        let line = self.line_of(offset);
        (line, offset - self.line_start(line))
    }

    /// Replaces `range` (byte offsets on char boundaries) with `with`.
    pub fn replace(&mut self, range: Range<usize>, with: &str) -> Change {
        let before = self.lines.len();
        let start_point = self.point(range.start);
        let old_end_point = self.point(range.end);
        let mut first = if before == 0 {
            0
        } else {
            self.line_of(range.start).min(before - 1)
        };
        // A lone CR just before the edit joins an LF that now follows it, so its line's
        // terminator changes too.
        let bytes = self.text.as_bytes();
        let lf_follows = match with.as_bytes().first() {
            Some(&b) => b == b'\n',
            None => bytes.get(range.end) == Some(&b'\n'),
        };
        if first > 0
            && range.start == self.line_start(first)
            && bytes[range.start - 1] == b'\r'
            && lf_follows
        {
            first -= 1;
        }
        let last = if before == 0 {
            0
        } else {
            self.line_of(range.end).min(before - 1)
        };
        self.text.replace_range(range.clone(), with);
        self.lines
            .replace(self.text.as_bytes(), range.clone(), with.len());
        self.version += 1;
        let after = self.lines.len();
        let old_lines = if before == 0 { 0 } else { last - first + 1 };
        let new_lines = old_lines as isize + after as isize - before as isize;
        debug_assert!(new_lines >= 0, "line totals out of step");
        let new_lines = new_lines as usize;
        let new_end = range.start + with.len();
        Change {
            line: first as u32,
            old_lines: old_lines as u32,
            new_lines: new_lines as u32,
            start_byte: range.start,
            old_end_byte: range.end,
            new_end_byte: new_end,
            start_point,
            old_end_point,
            new_end_point: self.point(new_end),
        }
    }

    /// The end of the grapheme that starts at `offset` (CRLF and Thai clusters are one).
    pub fn next_grapheme(&self, offset: usize) -> usize {
        self.text[offset..]
            .grapheme_indices(true)
            .nth(1)
            .map_or(self.text.len(), |(i, _)| offset + i)
    }

    /// The start of the grapheme that ends at `offset`.
    pub fn prev_grapheme(&self, offset: usize) -> usize {
        // Look back from the start of the previous line at most, which bounds the scan.
        let from = self.line_start(self.line_of(offset.saturating_sub(1)).saturating_sub(1));
        self.text[from..offset]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| from + i)
    }

    /// The start of the code point before `offset`, with CRLF as one unit (Backspace).
    pub fn prev_char(&self, offset: usize) -> usize {
        if self.text[..offset].ends_with("\r\n") {
            return offset - 2;
        }
        self.text[..offset]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Each line's text with the break that actually follows it.
    fn rows(b: &TextBuffer) -> Vec<(String, String)> {
        (0..b.line_count())
            .map(|l| {
                let end = b.line_end(l);
                let next = b.line_start(l + 1);
                (b.line(l).to_string(), b.text()[end..next].to_string())
            })
            .collect()
    }

    #[test]
    fn lines_follow_the_line_index_convention() {
        let b = TextBuffer::new("a\r\nbb\ncc".into());
        assert_eq!(b.line_count(), 3);
        assert_eq!((b.line(0), b.line(1), b.line(2)), ("a", "bb", "cc"));
        assert_eq!((b.line_start(1), b.line_end(1)), (3, 5));
        assert_eq!(b.terminator(0), "\r\n");
        assert_eq!(b.line_of(4), 1);
        let ended = TextBuffer::new("x\n".into());
        assert_eq!(ended.line_count(), 1);
        assert_eq!(ended.line(1), "", "the virtual line after a final break");
        assert_eq!(ended.line_of(2), 1);
        assert_eq!(TextBuffer::new(String::new()).line_count(), 0);
    }

    #[test]
    fn replace_reports_lines_and_points() {
        let mut b = TextBuffer::new("one\ntwo\nthree\n".into());
        let change = b.replace(4..7, "2\n2b");
        assert_eq!(b.text(), "one\n2\n2b\nthree\n");
        assert_eq!((change.line, change.old_lines, change.new_lines), (1, 1, 2));
        assert_eq!(
            (change.start_byte, change.old_end_byte, change.new_end_byte),
            (4, 7, 8)
        );
        assert_eq!(change.start_point, (1, 0));
        assert_eq!(change.old_end_point, (1, 3));
        assert_eq!(change.new_end_point, (2, 2));
        assert_eq!(b.version(), 1);
        let joined = b.replace(5..6, "");
        assert_eq!(b.line(1), "22b");
        assert_eq!((joined.old_lines, joined.new_lines), (2, 1));
    }

    #[test]
    fn edits_at_the_ends_keep_totals_consistent() {
        let mut b = TextBuffer::new(String::new());
        let c = b.replace(0..0, "a");
        assert_eq!((c.line, c.old_lines, c.new_lines), (0, 0, 1));
        let c = b.replace(1..1, "\n");
        assert_eq!(b.line_count(), 1, "a final break opens no line");
        assert_eq!(1 - c.old_lines + c.new_lines, 1);
        let c = b.replace(2..2, "b");
        assert_eq!(b.line_count(), 2);
        assert_eq!(1 - c.old_lines + c.new_lines, 2);
        let c = b.replace(0..3, "");
        assert_eq!((b.line_count(), c.old_lines, c.new_lines), (0, 2, 0));
    }

    #[test]
    fn thai_clusters_and_crlf_are_single_steps() {
        let b = TextBuffer::new("ก่ข\r\nx".into());
        assert_eq!(
            b.next_grapheme(0),
            "ก่".len(),
            "the tone mark stays with its consonant"
        );
        assert_eq!(b.prev_grapheme("ก่".len()), 0);
        assert_eq!(
            b.prev_char("ก่".len()),
            "ก".len(),
            "backspace removes only the mark"
        );
        let crlf = "ก่ข".len();
        assert_eq!(b.next_grapheme(crlf), crlf + 2, "CRLF is one step");
        assert_eq!(b.prev_char(crlf + 2), crlf);
    }

    #[test]
    fn a_cr_joined_into_a_crlf_widens_the_reported_lines() {
        let mut b = TextBuffer::new("a\rx\nb".into());
        let c = b.replace(2..3, "");
        assert_eq!(b.text(), "a\r\nb");
        assert_eq!((c.line, c.old_lines, c.new_lines), (0, 2, 1));
        let mut b = TextBuffer::new("a\rb".into());
        let c = b.replace(2..2, "\nz");
        assert_eq!((c.line, c.old_lines, c.new_lines), (0, 2, 2));
    }

    proptest! {
        #[test]
        fn incremental_line_index_matches_a_full_rebuild(
            start in "[ab\r\n]{0,40}",
            edits in proptest::collection::vec((0usize..60, 0usize..8, "[xy\r\n]{0,6}"), 1..12),
        ) {
            let mut b = TextBuffer::new(start);
            for (at, len, text) in edits {
                let start = at.min(b.len());
                let end = (start + len).min(b.len());
                let before = b.line_count() as i64;
                let old_rows = rows(&b);
                let change = b.replace(start..end, &text);
                let new_rows = rows(&b);
                let (line, old_n, new_n) =
                    (change.line as usize, change.old_lines as usize, change.new_lines as usize);
                prop_assert_eq!(&old_rows[..line], &new_rows[..line]);
                prop_assert_eq!(&old_rows[line + old_n..], &new_rows[line + new_n..]);
                prop_assert_eq!(b.lines(), &LineIndex::new(b.text().as_bytes()));
                prop_assert_eq!(
                    before - change.old_lines as i64 + change.new_lines as i64,
                    b.line_count() as i64
                );
            }
        }
    }
}
