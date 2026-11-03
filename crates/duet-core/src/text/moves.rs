//! Moved lines: a removed line whose text reappears among the added lines. The same rule as
//! LocalDiff: comparison keys (ignore options applied), at least 3 non-blank bytes, paired
//! first come first served. Only lines inside hunks are looked at, so the cost is O(changes).

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};

use crate::cancel::{CancelToken, Cancelled};
use crate::text::diff::{DiffInput, DiffOptions, LineDiff, line_key};

/// Above this many changed lines on either side, moves are not looked for.
pub const MOVE_LIMIT: usize = 1_000_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Moves {
    /// (left line, right line) pairs sorted by left line.
    by_left: Vec<(u32, u32)>,
    /// The same pairs sorted by right line.
    by_right: Vec<(u32, u32)>,
    /// True when there were too many changed lines to check.
    pub skipped: bool,
}

impl Moves {
    pub fn len(&self) -> usize {
        self.by_left.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_left.is_empty()
    }

    pub fn partner_of_left(&self, line: u32) -> Option<u32> {
        self.by_left
            .binary_search_by_key(&line, |&(l, _)| l)
            .ok()
            .map(|i| self.by_left[i].1)
    }

    pub fn partner_of_right(&self, line: u32) -> Option<u32> {
        self.by_right
            .binary_search_by_key(&line, |&(_, r)| r)
            .ok()
            .map(|i| self.by_right[i].0)
    }

    /// The same moves with the sides exchanged (F9).
    pub fn swapped(&self) -> Moves {
        Moves {
            by_left: self.by_right.iter().map(|&(l, r)| (r, l)).collect(),
            by_right: self.by_left.iter().map(|&(l, r)| (r, l)).collect(),
            skipped: self.skipped,
        }
    }
}

fn significant(key: &[u8]) -> bool {
    key.iter().filter(|b| !b.is_ascii_whitespace()).count() >= 3
}

pub fn detect_moves<'a>(
    left: DiffInput<'a>,
    right: DiffInput<'a>,
    diff: &LineDiff,
    opts: DiffOptions,
    limit: usize,
    cancel: &CancelToken,
) -> Result<Moves, Cancelled> {
    let removed: usize = diff
        .hunks
        .iter()
        .map(|h| (h.left.end - h.left.start) as usize)
        .sum();
    let added: usize = diff
        .hunks
        .iter()
        .map(|h| (h.right.end - h.right.start) as usize)
        .sum();
    if removed.max(added) > limit {
        return Ok(Moves {
            skipped: true,
            ..Moves::default()
        });
    }
    let mut pool: HashMap<Cow<'a, [u8]>, VecDeque<u32>> = HashMap::new();
    let mut seen = 0usize;
    for hunk in &diff.hunks {
        for line in hunk.left.clone() {
            seen += 1;
            if seen.is_multiple_of(65_536) {
                cancel.check()?;
            }
            let key = line_key(left, line as usize, opts);
            if significant(&key) {
                pool.entry(key).or_default().push_back(line);
            }
        }
    }
    let mut by_left = Vec::new();
    for hunk in &diff.hunks {
        for line in hunk.right.clone() {
            seen += 1;
            if seen.is_multiple_of(65_536) {
                cancel.check()?;
            }
            let key = line_key(right, line as usize, opts);
            if !significant(&key) {
                continue;
            }
            if let Some(from) = pool.get_mut(&key).and_then(VecDeque::pop_front) {
                by_left.push((from, line));
            }
        }
    }
    by_left.sort_unstable();
    let mut by_right = by_left.clone();
    by_right.sort_unstable_by_key(|&(l, r)| (r, l));
    Ok(Moves {
        by_left,
        by_right,
        skipped: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::diff::diff_lines;
    use crate::text::lines::LineIndex;

    fn moves_for(left: &str, right: &str, opts: DiffOptions, limit: usize) -> Moves {
        let (li, ri) = (
            LineIndex::new(left.as_bytes()),
            LineIndex::new(right.as_bytes()),
        );
        let (l, r) = (
            DiffInput {
                bytes: left.as_bytes(),
                lines: &li,
            },
            DiffInput {
                bytes: right.as_bytes(),
                lines: &ri,
            },
        );
        let cancel = CancelToken::new();
        let diff = diff_lines(l, r, opts, &cancel).unwrap();
        detect_moves(l, r, &diff, opts, limit, &cancel).unwrap()
    }

    const LEFT: &str = "a\nfn one()\nfn two()\n}\nb\nc\nd\ne\nf\n";
    const RIGHT: &str = "a\nb\nc\nd\ne\nf\nfn one()\nfn two()\n}\n";

    #[test]
    fn a_block_moved_down_pairs_its_lines() {
        let moves = moves_for(LEFT, RIGHT, DiffOptions::default(), MOVE_LIMIT);
        assert_eq!(moves.len(), 2, "\"}}\" is too short to count");
        assert_eq!(moves.partner_of_left(1), Some(6));
        assert_eq!(moves.partner_of_left(2), Some(7));
        assert_eq!(moves.partner_of_left(3), None);
        assert_eq!(moves.partner_of_right(7), Some(2));
    }

    #[test]
    fn ignore_options_apply_to_moves() {
        let right = RIGHT.replace("fn one()", "FN ONE()");
        assert_eq!(
            moves_for(LEFT, &right, DiffOptions::default(), MOVE_LIMIT).len(),
            1
        );
        let opts = DiffOptions {
            ignore_case: true,
            ..DiffOptions::default()
        };
        assert_eq!(moves_for(LEFT, &right, opts, MOVE_LIMIT).len(), 2);
    }

    #[test]
    fn moves_are_skipped_above_the_limit() {
        let moves = moves_for(LEFT, RIGHT, DiffOptions::default(), 2);
        assert!(moves.skipped && moves.is_empty());
    }

    #[test]
    fn swapped_moves_mirror_the_pairs() {
        let moves = moves_for(LEFT, RIGHT, DiffOptions::default(), MOVE_LIMIT).swapped();
        assert_eq!(moves.partner_of_left(6), Some(1));
        assert_eq!(moves.partner_of_right(2), Some(7));
    }
}
