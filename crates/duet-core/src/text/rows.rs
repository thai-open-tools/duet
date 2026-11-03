//! Side-by-side row layout: filler gaps opposite insertions/deletions, stored per segment
//! (O(hunks) memory) so very large files stay cheap.

use std::ops::Range;

use crate::text::diff::LineDiff;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Same,
    Changed,
    LeftOnly,
    RightOnly,
    SoftLeft,
    SoftRight,
}

impl RowKind {
    /// A difference: `Changed`, `LeftOnly` or `RightOnly`.
    pub fn is_change(self) -> bool {
        matches!(
            self,
            RowKind::Changed | RowKind::LeftOnly | RowKind::RightOnly
        )
    }

    /// An unimportant row that only one side holds.
    pub fn is_soft(self) -> bool {
        matches!(self, RowKind::SoftLeft | RowKind::SoftRight)
    }

    /// Whether rows of this kind carry a line of that side.
    pub fn holds(self, left: bool) -> bool {
        match self {
            RowKind::Same | RowKind::Changed => true,
            RowKind::LeftOnly | RowKind::SoftLeft => left,
            RowKind::RightOnly | RowKind::SoftRight => !left,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub kind: RowKind,
    pub left: Option<u32>,
    pub right: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
struct Segment {
    row_start: u32,
    len: u32,
    kind: RowKind,
    left_start: u32,
    right_start: u32,
}

#[derive(Debug, Clone, Default)]
pub struct RowMap {
    segments: Vec<Segment>,
    change_starts: Vec<u32>,
    rows: u32,
    left_lines: u32,
    right_lines: u32,
}

impl RowMap {
    pub fn new(diff: &LineDiff) -> Self {
        let mut map = RowMap {
            left_lines: diff.left_lines,
            right_lines: diff.right_lines,
            ..RowMap::default()
        };
        let (mut left, mut right) = (0u32, 0u32);
        for (soft, hunk) in diff.ordered() {
            map.push(RowKind::Same, hunk.left.start - left, left, right);
            let removed = hunk.left.end - hunk.left.start;
            let added = hunk.right.end - hunk.right.start;
            let paired = removed.min(added);
            if soft {
                // Paired lines are plain rows; the longer side's extras are unimportant.
                map.push(RowKind::Same, paired, hunk.left.start, hunk.right.start);
                map.push(
                    RowKind::SoftLeft,
                    removed - paired,
                    hunk.left.start + paired,
                    hunk.right.end,
                );
                map.push(
                    RowKind::SoftRight,
                    added - paired,
                    hunk.left.end,
                    hunk.right.start + paired,
                );
                left = hunk.left.end;
                right = hunk.right.end;
                continue;
            }
            map.change_starts.push(map.rows);
            map.push(RowKind::Changed, paired, hunk.left.start, hunk.right.start);
            map.push(
                RowKind::LeftOnly,
                removed - paired,
                hunk.left.start + paired,
                hunk.right.end,
            );
            map.push(
                RowKind::RightOnly,
                added - paired,
                hunk.left.end,
                hunk.right.start + paired,
            );
            left = hunk.left.end;
            right = hunk.right.end;
        }
        map.push(RowKind::Same, diff.left_lines - left, left, right);
        map
    }

    fn push(&mut self, kind: RowKind, len: u32, left_start: u32, right_start: u32) {
        if len > 0 {
            self.segments.push(Segment {
                row_start: self.rows,
                len,
                kind,
                left_start,
                right_start,
            });
            self.rows += len;
        }
    }

    /// Row ranges of the change blocks (consecutive difference segments), in order. O(hunks):
    /// views filter and mark huge files from these instead of walking every row.
    pub fn change_blocks(&self) -> Vec<Range<u32>> {
        let mut blocks: Vec<Range<u32>> = Vec::new();
        for segment in self.segments.iter().filter(|s| s.kind.is_change()) {
            let range = segment.row_start..segment.row_start + segment.len;
            match blocks.last_mut() {
                Some(last) if last.end == range.start => last.end = range.end,
                _ => blocks.push(range),
            }
        }
        blocks
    }

    pub fn row_count(&self) -> u32 {
        self.rows
    }

    pub fn row(&self, row: u32) -> Row {
        assert!(row < self.rows, "row {row} out of range ({})", self.rows);
        let index = self
            .segments
            .partition_point(|s| s.row_start + s.len <= row);
        let segment = self.segments[index];
        let offset = row - segment.row_start;
        let (left, right) = match segment.kind {
            RowKind::Same | RowKind::Changed => (
                Some(segment.left_start + offset),
                Some(segment.right_start + offset),
            ),
            RowKind::LeftOnly | RowKind::SoftLeft => (Some(segment.left_start + offset), None),
            RowKind::RightOnly | RowKind::SoftRight => (None, Some(segment.right_start + offset)),
        };
        Row {
            kind: segment.kind,
            left,
            right,
        }
    }

    /// The row that shows `line` of the left (`left = true`) or right side. Every line has
    /// exactly one row. O(log segments).
    pub fn row_of(&self, left: bool, line: u32) -> u32 {
        let holds = |s: &Segment| s.kind.holds(left);
        let start = |s: &Segment| if left { s.left_start } else { s.right_start };
        // A side's line ends never decrease along the segments, so this is a binary search.
        let index = self
            .segments
            .partition_point(|s| start(s) + if holds(s) { s.len } else { 0 } <= line);
        let segment = self.segments[index];
        debug_assert!(
            holds(&segment) && start(&segment) <= line,
            "line {line} out of range"
        );
        segment.row_start + (line - start(&segment))
    }

    /// The lines of one side that `rows` covers, or an empty range at the insertion point when
    /// those rows hold none of that side's lines.
    pub fn line_range(&self, rows: Range<u32>, left: bool) -> Range<u32> {
        let mut first: Option<u32> = None;
        let mut last = 0u32;
        let mut insert_at: Option<u32> = None;
        let covered = self
            .segments
            .iter()
            .filter(|s| s.row_start < rows.end && s.row_start + s.len > rows.start);
        for s in covered {
            let holds = s.kind.holds(left);
            let start = if left { s.left_start } else { s.right_start };
            if holds {
                let from = rows.start.max(s.row_start) - s.row_start;
                let to = rows.end.min(s.row_start + s.len) - s.row_start;
                first.get_or_insert(start + from);
                last = start + to;
            } else {
                insert_at.get_or_insert(start);
            }
        }
        match first {
            Some(first) => first..last,
            None => {
                let at = insert_at.unwrap_or(if left {
                    self.left_lines
                } else {
                    self.right_lines
                });
                at..at
            }
        }
    }

    /// First row of the next change block strictly after `after` (or the first one if `None`).
    pub fn next_change(&self, after: Option<u32>) -> Option<u32> {
        let index = match after {
            None => 0,
            Some(row) => self.change_starts.partition_point(|&start| start <= row),
        };
        self.change_starts.get(index).copied()
    }

    /// First row of the closest change block that starts strictly before `before`.
    pub fn prev_change(&self, before: u32) -> Option<u32> {
        let index = self.change_starts.partition_point(|&start| start < before);
        index.checked_sub(1).map(|i| self.change_starts[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::diff::LineHunk;

    #[test]
    fn line_range_finds_lines_or_the_insertion_point() {
        let diff = LineDiff {
            hunks: vec![LineHunk {
                left: 1..3,
                right: 1..1,
            }],
            left_lines: 5,
            right_lines: 3,
            ..LineDiff::default()
        };
        let map = RowMap::new(&diff);
        // Rows 1-2 are left-only: lines 1-2 on the left, nothing on the right (insert at 1).
        assert_eq!(map.line_range(1..3, true), 1..3);
        assert_eq!(map.line_range(1..3, false), 1..1);
        assert_eq!(map.line_range(0..4, false), 0..2);
    }

    fn diff(hunks: Vec<(std::ops::Range<u32>, std::ops::Range<u32>)>, l: u32, r: u32) -> LineDiff {
        LineDiff {
            hunks: hunks
                .into_iter()
                .map(|(left, right)| LineHunk { left, right })
                .collect(),
            left_lines: l,
            right_lines: r,
            ..LineDiff::default()
        }
    }

    fn all_rows(map: &RowMap) -> Vec<Row> {
        (0..map.row_count()).map(|r| map.row(r)).collect()
    }

    #[test]
    fn modify_plus_insert_is_aligned() {
        let map = RowMap::new(&diff(vec![(1..2, 1..3)], 3, 4));
        assert_eq!(
            all_rows(&map),
            vec![
                Row {
                    kind: RowKind::Same,
                    left: Some(0),
                    right: Some(0)
                },
                Row {
                    kind: RowKind::Changed,
                    left: Some(1),
                    right: Some(1)
                },
                Row {
                    kind: RowKind::RightOnly,
                    left: None,
                    right: Some(2)
                },
                Row {
                    kind: RowKind::Same,
                    left: Some(2),
                    right: Some(3)
                },
            ]
        );
    }

    #[test]
    fn pure_deletion() {
        let map = RowMap::new(&diff(vec![(0..2, 0..0)], 3, 1));
        assert_eq!(
            all_rows(&map),
            vec![
                Row {
                    kind: RowKind::LeftOnly,
                    left: Some(0),
                    right: None
                },
                Row {
                    kind: RowKind::LeftOnly,
                    left: Some(1),
                    right: None
                },
                Row {
                    kind: RowKind::Same,
                    left: Some(2),
                    right: Some(0)
                },
            ]
        );
    }

    #[test]
    fn identical_and_empty() {
        assert_eq!(RowMap::new(&diff(vec![], 5, 5)).row_count(), 5);
        assert_eq!(RowMap::new(&diff(vec![], 0, 0)).row_count(), 0);
    }

    #[test]
    fn navigation_between_change_blocks() {
        // rows: 0 same, 1 changed, 2 same, 3 left-only, 4 same
        let map = RowMap::new(&diff(vec![(1..2, 1..2), (3..4, 3..3)], 5, 4));
        assert_eq!(map.next_change(None), Some(1));
        assert_eq!(map.next_change(Some(1)), Some(3));
        assert_eq!(map.next_change(Some(3)), None);
        assert_eq!(map.prev_change(5), Some(3));
        assert_eq!(map.prev_change(3), Some(1));
        assert_eq!(map.prev_change(1), None);
    }

    #[test]
    fn change_blocks_cover_each_hunk_once() {
        // rows: 0 same | 1 changed, 2 right-only | 3-4 same | 5-6 left-only | 7 same
        let map = RowMap::new(&diff(vec![(1..2, 1..3), (4..6, 5..5)], 7, 6));
        assert_eq!(map.change_blocks(), vec![1..3, 5..7]);
        let identical = RowMap::new(&diff(vec![], 3, 3));
        assert!(identical.change_blocks().is_empty());
    }

    #[test]
    fn soft_runs_pair_then_leave_one_sided_unimportant_rows() {
        let map = RowMap::new(&LineDiff {
            hunks: vec![LineHunk {
                left: 5..6,
                right: 4..5,
            }],
            soft: vec![LineHunk {
                left: 1..3,
                right: 1..2,
            }],
            left_lines: 7,
            right_lines: 6,
            ..LineDiff::default()
        });
        let row = |kind, left, right| Row { kind, left, right };
        assert_eq!(
            all_rows(&map),
            vec![
                row(RowKind::Same, Some(0), Some(0)),
                row(RowKind::Same, Some(1), Some(1)),
                row(RowKind::SoftLeft, Some(2), None),
                row(RowKind::Same, Some(3), Some(2)),
                row(RowKind::Same, Some(4), Some(3)),
                row(RowKind::Changed, Some(5), Some(4)),
                row(RowKind::Same, Some(6), Some(5)),
            ]
        );
        assert_eq!(map.change_blocks(), vec![5..6]);
        assert_eq!(map.row_of(true, 2), 2);
        assert_eq!(map.row_of(false, 2), 3);
        assert_eq!(map.line_range(2..3, false), 2..2);
        assert_eq!(map.next_change(None), Some(5), "soft rows are no change");
    }

    #[test]
    fn row_kinds_know_their_class_and_sides() {
        assert!(RowKind::Changed.is_change() && RowKind::LeftOnly.is_change());
        assert!(RowKind::RightOnly.is_change());
        assert!(!RowKind::Same.is_change() && !RowKind::SoftLeft.is_change());
        assert!(RowKind::SoftLeft.is_soft() && RowKind::SoftRight.is_soft());
        assert!(!RowKind::Changed.is_soft());
        assert!(RowKind::SoftLeft.holds(true) && !RowKind::SoftLeft.holds(false));
        assert!(RowKind::SoftRight.holds(false) && !RowKind::SoftRight.holds(true));
        assert!(RowKind::Same.holds(true) && RowKind::Changed.holds(false));
    }

    #[test]
    fn a_soft_run_with_one_empty_side_is_all_unimportant_rows() {
        let map = RowMap::new(&LineDiff {
            soft: vec![LineHunk {
                left: 1..1,
                right: 1..3,
            }],
            left_lines: 2,
            right_lines: 4,
            ..LineDiff::default()
        });
        let kinds: Vec<RowKind> = all_rows(&map).iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            [
                RowKind::Same,
                RowKind::SoftRight,
                RowKind::SoftRight,
                RowKind::Same
            ]
        );
        assert!(map.change_blocks().is_empty());
        assert_eq!(map.line_range(1..3, true), 1..1);
    }

    #[test]
    fn row_of_inverts_row_for_every_line() {
        let diff = LineDiff {
            hunks: vec![
                LineHunk {
                    left: 1..3,
                    right: 1..2,
                },
                LineHunk {
                    left: 5..5,
                    right: 4..7,
                },
            ],
            left_lines: 8,
            right_lines: 10,
            ..LineDiff::default()
        };
        let map = RowMap::new(&diff);
        for row in 0..map.row_count() {
            let r = map.row(row);
            if let Some(line) = r.left {
                assert_eq!(map.row_of(true, line), row, "left line {line}");
            }
            if let Some(line) = r.right {
                assert_eq!(map.row_of(false, line), row, "right line {line}");
            }
        }
    }
}
