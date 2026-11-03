//! What changed inside a pair of changed lines, at grapheme-cluster granularity so Thai
//! vowels and tone marks are never split from their base consonant.

// Results are lists of byte ranges, so a one-element `vec![a..b]` is intended.
#![allow(clippy::single_range_in_vec_init)]

use std::ops::Range;

use imara_diff::{Algorithm, Diff, InternedInput, Interner, Token};
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

/// Above this many graphemes on either side of what differs (the common start and end cut
/// off), the whole middle is marked changed.
pub const INLINE_MAX_GRAPHEMES: usize = 10_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InlineDiff {
    pub left: Vec<Range<usize>>,
    pub right: Vec<Range<usize>>,
}

pub fn refine(left: &str, right: &str) -> InlineDiff {
    // Check the cap before collecting: collecting first costs 24 bytes per grapheme.
    if over_cap(left) || over_cap(right) {
        return refine_long(left, right);
    }
    refine_graphemes(left, right)
}

/// A line over the cap (T-43): the common start and end are cut off at a grapheme boundary of
/// both lines, without collecting graphemes, and only the middle is refined, or marked whole
/// when it is still over the cap.
fn refine_long(left: &str, right: &str) -> InlineDiff {
    let start = common_start(left, right);
    let end = common_end(left, right, start);
    let (left_mid, right_mid) = (
        &left[start..left.len() - end],
        &right[start..right.len() - end],
    );
    let middle = if over_cap(left_mid) || over_cap(right_mid) {
        InlineDiff {
            left: whole(left_mid),
            right: whole(right_mid),
        }
    } else {
        refine_graphemes(left_mid, right_mid)
    };
    let shift = |ranges: Vec<Range<usize>>| -> Vec<Range<usize>> {
        ranges
            .into_iter()
            .map(|r| r.start + start..r.end + start)
            .collect()
    };
    InlineDiff {
        left: shift(middle.left),
        right: shift(middle.right),
    }
}

/// How many grapheme steps the common start or end may move to reach a boundary of both
/// lines; past that (a long run of regional indicators, say) it is given up.
const BOUNDARY_STEPS: usize = 32;

// The whole line is the chunk, so the cursor never asks for more context.
fn is_grapheme_boundary(line: &str, at: usize) -> bool {
    GraphemeCursor::new(at, line.len(), true)
        .is_boundary(line, 0)
        .unwrap_or(false)
}

fn prev_grapheme_boundary(line: &str, at: usize) -> usize {
    GraphemeCursor::new(at, line.len(), true)
        .prev_boundary(line, 0)
        .ok()
        .flatten()
        .unwrap_or(0)
}

fn next_grapheme_boundary(line: &str, at: usize) -> usize {
    GraphemeCursor::new(at, line.len(), true)
        .next_boundary(line, 0)
        .ok()
        .flatten()
        .unwrap_or(line.len())
}

/// The length in bytes of the longest common start of both lines that ends on a grapheme
/// boundary of both.
fn common_start(left: &str, right: &str) -> usize {
    let (l, r) = (left.as_bytes(), right.as_bytes());
    let mut at = l.iter().zip(r).take_while(|(a, b)| a == b).count();
    // Both lines hold the same bytes before `at`, so it starts a character in both or in neither.
    while !left.is_char_boundary(at) {
        at -= 1;
    }
    for _ in 0..BOUNDARY_STEPS {
        if is_grapheme_boundary(left, at) && is_grapheme_boundary(right, at) {
            return at;
        }
        at = prev_grapheme_boundary(left, at);
    }
    0
}

/// The length in bytes of the longest common end of both lines that starts on a grapheme
/// boundary of both and does not reach into the first `start` bytes.
fn common_end(left: &str, right: &str, start: usize) -> usize {
    let (l, r) = (left.as_bytes(), right.as_bytes());
    let room = l.len().min(r.len()) - start;
    let mut end = l
        .iter()
        .rev()
        .zip(r.iter().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    // Both lines end with the same `end` bytes: a character starts there in both or in neither.
    while !left.is_char_boundary(l.len() - end) {
        end -= 1;
    }
    for _ in 0..BOUNDARY_STEPS {
        let (at_left, at_right) = (l.len() - end, r.len() - end);
        if is_grapheme_boundary(left, at_left) && is_grapheme_boundary(right, at_right) {
            return end;
        }
        end = l.len() - next_grapheme_boundary(left, at_left);
    }
    0
}

fn refine_graphemes(left: &str, right: &str) -> InlineDiff {
    let lg: Vec<(usize, &str)> = left.grapheme_indices(true).collect();
    let rg: Vec<(usize, &str)> = right.grapheme_indices(true).collect();
    let mut interner = Interner::new(lg.len() + rg.len());
    let before: Vec<Token> = lg.iter().map(|&(_, g)| interner.intern(g)).collect();
    let after: Vec<Token> = rg.iter().map(|&(_, g)| interner.intern(g)).collect();
    let input = InternedInput {
        before,
        after,
        interner,
    };
    // Myers: Histogram's heuristic is poor for small alphabets such as characters.
    let mut diff = Diff::compute(Algorithm::Myers, &input);
    diff.postprocess_no_heuristic(&input);
    let mut out = InlineDiff::default();
    for hunk in diff.hunks() {
        push_merged(&mut out.left, span(&lg, hunk.before));
        push_merged(&mut out.right, span(&rg, hunk.after));
    }
    out
}

fn over_cap(line: &str) -> bool {
    // A grapheme is at least one byte, so short lines never need counting.
    line.len() > INLINE_MAX_GRAPHEMES
        && line.graphemes(true).take(INLINE_MAX_GRAPHEMES + 1).count() > INLINE_MAX_GRAPHEMES
}

fn whole(line: &str) -> Vec<Range<usize>> {
    if line.is_empty() {
        Vec::new()
    } else {
        vec![0..line.len()]
    }
}

fn span(graphemes: &[(usize, &str)], tokens: Range<u32>) -> Option<Range<usize>> {
    if tokens.is_empty() {
        return None;
    }
    let (first, _) = graphemes[tokens.start as usize];
    let (last, text) = graphemes[tokens.end as usize - 1];
    Some(first..last + text.len())
}

fn push_merged(ranges: &mut Vec<Range<usize>>, next: Option<Range<usize>>) {
    let Some(next) = next else { return };
    match ranges.last_mut() {
        Some(prev) if prev.end == next.start => prev.end = next.end,
        _ => ranges.push(next),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_ascii_change() {
        assert_eq!(
            refine("abc", "aXc"),
            InlineDiff {
                left: vec![1..2],
                right: vec![1..2]
            }
        );
    }

    #[test]
    fn identical_lines_have_no_ranges() {
        assert_eq!(refine("same", "same"), InlineDiff::default());
    }

    #[test]
    fn thai_tone_mark_stays_with_its_consonant() {
        // "ก่" is one grapheme (6 bytes); "ก" is 3 bytes.
        assert_eq!(
            refine("กา", "ก่า"),
            InlineDiff {
                left: vec![0..3],
                right: vec![0..6]
            }
        );
    }

    #[test]
    fn thai_suffix_change() {
        assert_eq!(
            refine("น้ำใส", "น้ำขุ่น"),
            InlineDiff {
                left: vec![9..15],
                right: vec![9..21]
            }
        );
    }

    #[test]
    fn ranges_fall_on_char_boundaries() {
        let (l, r) = ("ผู้ใหญ่ฟุ้งซ่าน", "ผู้น้อยฟุ้งซ่าน");
        let d = refine(l, r);
        for range in &d.left {
            assert!(l.is_char_boundary(range.start) && l.is_char_boundary(range.end));
        }
        for range in &d.right {
            assert!(r.is_char_boundary(range.start) && r.is_char_boundary(range.end));
        }
        assert!(!d.left.is_empty() && !d.right.is_empty());
    }

    #[test]
    fn empty_versus_text() {
        assert_eq!(
            refine("", "abc"),
            InlineDiff {
                left: vec![],
                right: vec![0..3]
            }
        );
    }

    #[test]
    fn a_long_line_still_marks_the_one_changed_character() {
        // T-43: a 100 KB cell whose one change sits near the end (LocalDiff marks it).
        let (head, tail) = ("x".repeat(99_991), "y".repeat(8));
        let (l, r) = (format!("{head}a{tail}"), format!("{head}b{tail}"));
        assert_eq!(
            refine(&l, &r),
            InlineDiff {
                left: vec![99_991..99_992],
                right: vec![99_991..99_992]
            }
        );
        // The cut falls on a grapheme boundary of both lines: "ก่" stays whole.
        let head = "x".repeat(10_001);
        let (l, r) = (format!("{head}กา"), format!("{head}ก่า"));
        assert_eq!(
            refine(&l, &r),
            InlineDiff {
                left: vec![10_001..10_004],
                right: vec![10_001..10_007]
            }
        );
        // A middle still over the cap is marked whole, the common ends are not.
        let (l, r) = (
            format!("{head}{}{head}", "a".repeat(10_001)),
            format!("{head}{}{head}", "b".repeat(10_002)),
        );
        assert_eq!(
            refine(&l, &r),
            InlineDiff {
                left: vec![10_001..20_002],
                right: vec![10_001..20_003]
            }
        );
    }

    #[test]
    fn very_long_lines_are_marked_whole() {
        let (l, r) = ("a".repeat(20_000), "b".repeat(20_000));
        assert_eq!(
            refine(&l, &r),
            InlineDiff {
                left: vec![0..20_000],
                right: vec![0..20_000]
            }
        );
    }
}
