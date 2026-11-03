//! Line index over raw bytes. A line ends with "\n", "\r\n" or "\r". A terminator at the
//! end of input does not start an extra empty line: "a\n" has one line, "" has none.

use std::ops::Range;

use memchr::memchr2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EolStyle {
    None,
    Lf,
    CrLf,
    Cr,
    Mixed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineIndex {
    starts: Vec<usize>,
    total: usize,
}

impl LineIndex {
    pub fn new(bytes: &[u8]) -> Self {
        let mut starts = Vec::new();
        if !bytes.is_empty() {
            starts.push(0);
        }
        let mut pos = 0;
        while let Some(offset) = memchr2(b'\n', b'\r', &bytes[pos..]) {
            let next = terminator_end(bytes, pos + offset);
            if next < bytes.len() {
                starts.push(next);
            }
            pos = next;
        }
        Self {
            starts,
            total: bytes.len(),
        }
    }

    pub fn len(&self) -> usize {
        self.starts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }

    /// The line holding `offset`, clamped to the last line (0 for an empty index).
    pub fn line_of(&self, offset: usize) -> usize {
        self.starts
            .partition_point(|&s| s <= offset)
            .saturating_sub(1)
    }

    /// Updates the index after `range` of the old text was replaced by `inserted` bytes,
    /// `bytes_after` being the new text. Rescans only the edited lines, so a keystroke in a
    /// large file costs O(lines) pointer shifts instead of a full scan.
    pub fn replace(&mut self, bytes_after: &[u8], range: Range<usize>, inserted: usize) {
        let delta = inserted as isize - (range.end - range.start) as isize;
        // A CR before the edit may now pair with an LF it inserted (or lost one), so rescan
        // from the start of the line before the edit's line.
        let first = self
            .starts
            .partition_point(|&s| s <= range.start)
            .saturating_sub(2);
        let rescan_from = self.starts.get(first).copied().unwrap_or(0);
        // Old starts after the edit (plus one byte, for a CRLF split at its end) shift by delta.
        let keep_from = self.starts.partition_point(|&s| s <= range.end + 1);
        let tail: Vec<usize> = self.starts[keep_from..]
            .iter()
            .map(|&s| (s as isize + delta) as usize)
            .collect();
        let rescan_to = tail.first().copied().unwrap_or(bytes_after.len());
        self.starts.truncate(first);
        let mut pos = rescan_from;
        if pos < bytes_after.len() {
            self.starts.push(pos);
        }
        while pos < rescan_to {
            let Some(offset) = memchr2(b'\n', b'\r', &bytes_after[pos..rescan_to]) else {
                break;
            };
            let next = terminator_end(bytes_after, pos + offset);
            if next < bytes_after.len() {
                self.starts.push(next);
            }
            pos = next;
        }
        // A break found at the end of the rescan may repeat the first shifted start.
        self.starts.extend(tail);
        self.starts.dedup();
        self.total = bytes_after.len();
    }

    pub fn span_with_eol(&self, line: usize) -> Range<usize> {
        self.starts[line]..self.starts.get(line + 1).copied().unwrap_or(self.total)
    }

    pub fn line_bytes_with_eol<'a>(&self, bytes: &'a [u8], line: usize) -> &'a [u8] {
        &bytes[self.span_with_eol(line)]
    }

    pub fn line_bytes<'a>(&self, bytes: &'a [u8], line: usize) -> &'a [u8] {
        let full = self.line_bytes_with_eol(bytes, line);
        &full[..full.len() - eol_len(full)]
    }

    pub fn eol_bytes<'a>(&self, bytes: &'a [u8], line: usize) -> &'a [u8] {
        let full = self.line_bytes_with_eol(bytes, line);
        &full[full.len() - eol_len(full)..]
    }
}

fn terminator_end(bytes: &[u8], at: usize) -> usize {
    if bytes[at] == b'\r' && bytes.get(at + 1) == Some(&b'\n') {
        at + 2
    } else {
        at + 1
    }
}

fn eol_len(line: &[u8]) -> usize {
    match line {
        [.., b'\r', b'\n'] => 2,
        [.., b'\n'] | [.., b'\r'] => 1,
        _ => 0,
    }
}

pub fn detect_eol(bytes: &[u8]) -> EolStyle {
    let (mut lf, mut crlf, mut cr) = (false, false, false);
    let mut pos = 0;
    while let Some(offset) = memchr2(b'\n', b'\r', &bytes[pos..]) {
        let at = pos + offset;
        let next = terminator_end(bytes, at);
        if next - at == 2 {
            crlf = true;
        } else if bytes[at] == b'\n' {
            lf = true;
        } else {
            cr = true;
        }
        pos = next;
    }
    match (lf, crlf, cr) {
        (false, false, false) => EolStyle::None,
        (true, false, false) => EolStyle::Lf,
        (false, true, false) => EolStyle::CrLf,
        (false, false, true) => EolStyle::Cr,
        _ => EolStyle::Mixed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_lines() {
        for (text, expected) in [
            ("", 0),
            ("a", 1),
            ("a\n", 1),
            ("a\nb", 2),
            ("a\r\nb\r\n", 2),
            ("a\rb", 2),
            ("\n\n", 2),
            ("ก\nข", 2),
        ] {
            assert_eq!(LineIndex::new(text.as_bytes()).len(), expected, "{text:?}");
        }
    }

    #[test]
    fn splits_content_and_terminator() {
        let text = "a\r\nขค\nlast".as_bytes();
        let index = LineIndex::new(text);
        assert_eq!(index.line_bytes(text, 0), b"a");
        assert_eq!(index.eol_bytes(text, 0), b"\r\n");
        assert_eq!(index.line_bytes_with_eol(text, 0), b"a\r\n");
        assert_eq!(index.line_bytes(text, 1), "ขค".as_bytes());
        assert_eq!(index.eol_bytes(text, 1), b"\n");
        assert_eq!(index.line_bytes(text, 2), b"last");
        assert_eq!(index.eol_bytes(text, 2), b"");
        assert_eq!(index.span_with_eol(2), 10..text.len());
    }

    #[test]
    fn empty_input_is_empty() {
        let index = LineIndex::new(b"");
        assert!(index.is_empty());
    }

    #[test]
    fn line_of_finds_the_containing_line() {
        let index = LineIndex::new(b"a\r\nbb\ncc");
        assert_eq!(index.line_of(0), 0);
        assert_eq!(index.line_of(2), 0);
        assert_eq!(index.line_of(3), 1);
        assert_eq!(index.line_of(7), 2);
        assert_eq!(LineIndex::new(b"").line_of(0), 0);
    }

    #[test]
    fn replace_rejoins_a_split_crlf() {
        let mut bytes = b"a\rb\nc".to_vec();
        let mut index = LineIndex::new(&bytes);
        bytes.splice(2..2, *b"\n");
        index.replace(&bytes, 2..2, 1);
        assert_eq!(index, LineIndex::new(&bytes));
        assert_eq!(index.len(), 3);
        bytes.remove(2);
        index.replace(&bytes, 2..3, 0);
        assert_eq!(index, LineIndex::new(&bytes));
    }

    #[test]
    fn detects_line_ending_styles() {
        assert_eq!(detect_eol(b"abc"), EolStyle::None);
        assert_eq!(detect_eol(b"a\nb\n"), EolStyle::Lf);
        assert_eq!(detect_eol(b"a\r\nb\r\n"), EolStyle::CrLf);
        assert_eq!(detect_eol(b"a\rb"), EolStyle::Cr);
        assert_eq!(detect_eol(b"a\r\nb\n"), EolStyle::Mixed);
    }
}
