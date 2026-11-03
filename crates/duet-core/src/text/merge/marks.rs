//! Which lines and characters of a change card differ from the base (LocalDiff `sideMarks`,
//! `js:385-409`, and the base column's marks, `js:431-448`).

// Highlights are lists of byte ranges, so a one-element `vec![a..b]` is intended.
#![allow(clippy::single_range_in_vec_init)]

use std::ops::Range;

use imara_diff::{Algorithm, Diff, InternedInput, Interner};

use super::MergeText;
use super::diff3::{Region, RegionKind, line_key};
use crate::text::inline::refine;

/// At most this many changed line pairs per side get character highlights.
pub const HIGHLIGHT_PAIRS_MAX: usize = 300;

/// One entry per line of the column.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnMarks {
    pub changed: Vec<bool>,
    pub highlights: Vec<Vec<Range<usize>>>,
}

impl ColumnMarks {
    fn unmarked(len: usize) -> ColumnMarks {
        ColumnMarks {
            changed: vec![false; len],
            highlights: vec![Vec::new(); len],
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CardMarks {
    pub left: ColumnMarks,
    pub base: ColumnMarks,
    pub right: ColumnMarks,
}

/// A side's marks against the base, and which base lines that side changed.
struct SideMarks {
    side: ColumnMarks,
    base: Vec<bool>,
}

fn side_marks(
    base: &MergeText,
    base_range: &Range<u32>,
    side: &MergeText,
    side_range: &Range<u32>,
    trail: bool,
) -> SideMarks {
    let (base_len, side_len) = (base_range.len(), side_range.len());
    if base_len == 0 || side_len == 0 {
        return SideMarks {
            side: ColumnMarks {
                changed: vec![true; side_len],
                highlights: vec![Vec::new(); side_len],
            },
            base: vec![true; base_len],
        };
    }
    let mut interner = Interner::new(base_len + side_len);
    let before = base_range
        .clone()
        .map(|n| interner.intern(line_key(base.line(n), trail)))
        .collect();
    let after = side_range
        .clone()
        .map(|n| interner.intern(line_key(side.line(n), trail)))
        .collect();
    let input = InternedInput {
        before,
        after,
        interner,
    };
    let mut diff = Diff::compute(Algorithm::Myers, &input);
    diff.postprocess_no_heuristic(&input);

    let mut out = SideMarks {
        side: ColumnMarks::unmarked(side_len),
        base: vec![false; base_len],
    };
    let mut pairs = 0;
    for hunk in diff.hunks() {
        for b in hunk.before.clone() {
            out.base[b as usize] = true;
        }
        for s in hunk.after.clone() {
            out.side.changed[s as usize] = true;
        }
        let both = hunk.before.len().min(hunk.after.len());
        for k in 0..both {
            if pairs == HIGHLIGHT_PAIRS_MAX {
                break;
            }
            pairs += 1;
            let (b, s) = (hunk.before.start + k as u32, hunk.after.start + k as u32);
            let marks = refine(
                base.line(base_range.start + b),
                side.line(side_range.start + s),
            );
            out.side.highlights[s as usize] = marks.right;
        }
    }
    out
}

/// The marks of one change card. The left column is marked unless the kind is `Right`, the
/// right column unless it is `Left`; a base line is marked when either shown side marks it.
pub fn card_marks(
    left: &MergeText,
    base: &MergeText,
    right: &MergeText,
    region: &Region,
    trail: bool,
) -> CardMarks {
    let mut marks = CardMarks {
        left: ColumnMarks::unmarked(region.left.len()),
        base: ColumnMarks::unmarked(region.base.len()),
        right: ColumnMarks::unmarked(region.right.len()),
    };
    if region.kind == RegionKind::Stable {
        return marks;
    }
    if region.kind != RegionKind::Right {
        let l = side_marks(base, &region.base, left, &region.left, trail);
        marks.left = l.side;
        for (m, c) in marks.base.changed.iter_mut().zip(l.base) {
            *m |= c;
        }
    }
    if region.kind != RegionKind::Left {
        let r = side_marks(base, &region.base, right, &region.right, trail);
        marks.right = r.side;
        for (m, c) in marks.base.changed.iter_mut().zip(r.base) {
            *m |= c;
        }
    }
    marks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[&str]) -> MergeText {
        MergeText::typed(lines.iter().map(|l| format!("{l}\n")).collect())
    }

    fn none() -> Vec<Range<usize>> {
        Vec::new()
    }

    fn region(kind: RegionKind, base: Range<u32>, left: Range<u32>, right: Range<u32>) -> Region {
        Region {
            kind,
            base,
            left,
            right,
        }
    }

    #[test]
    fn card_marks_flag_changed_lines_and_characters() {
        let base = text(&["one two", "same"]);
        let left = text(&["one 2", "same"]);
        let right = text(&["ONE two", "same", "extra"]);
        let conflict = region(RegionKind::Conflict, 0..2, 0..2, 0..3);
        let m = card_marks(&left, &base, &right, &conflict, false);
        assert_eq!(m.left.changed, [true, false]);
        assert_eq!(m.left.highlights, [vec![4..5], none()]);
        assert_eq!(m.right.changed, [true, false, true]);
        assert_eq!(m.right.highlights, [vec![0..3], none(), none()]);
        assert_eq!(m.base.changed, [true, false]);
        assert_eq!(m.base.highlights, [none(), none()]);

        let (b, l, r) = (text(&["x"]), text(&["y"]), text(&["x"]));
        let m = card_marks(
            &l,
            &b,
            &r,
            &region(RegionKind::Left, 0..1, 0..1, 0..1),
            false,
        );
        assert_eq!(m.left.changed, [true]);
        assert_eq!(m.left.highlights, [vec![0..1]]);
        assert_eq!(m.right.changed, [false]);
        assert_eq!(m.right.highlights, [none()]);
        assert_eq!(m.base.changed, [true]);

        let (b, l, r) = (text(&["x"]), text(&["x"]), text(&[]));
        let m = card_marks(
            &l,
            &b,
            &r,
            &region(RegionKind::Right, 0..1, 0..1, 1..1),
            false,
        );
        assert_eq!(m.right.changed, Vec::<bool>::new());
        assert_eq!(m.base.changed, [true]);
        assert_eq!(m.left.changed, [false]);
    }

    #[test]
    fn highlights_stop_after_300_pairs_but_every_line_is_marked() {
        let names = |p: &str| (0..301).map(|i| format!("{p}{i}")).collect::<Vec<_>>();
        let (b, l) = (names("b"), names("l"));
        let as_text = |v: &[String]| text(&v.iter().map(String::as_str).collect::<Vec<_>>());
        let (base, left) = (as_text(&b), as_text(&l));
        let right = as_text(&b);
        let r = region(RegionKind::Conflict, 0..301, 0..301, 0..301);
        let m = card_marks(&left, &base, &right, &r, false);
        assert_eq!(m.left.highlights[299], [0..1]);
        assert!(m.left.highlights[300].is_empty());
        assert!(m.left.changed[300]);
    }

    #[test]
    fn trailing_whitespace_is_not_a_change_when_ignored() {
        let (base, left, right) = (text(&["a"]), text(&["a  "]), text(&["b"]));
        let r = region(RegionKind::Conflict, 0..1, 0..1, 0..1);
        assert_eq!(
            card_marks(&left, &base, &right, &r, true).left.changed,
            [false]
        );
        assert_eq!(
            card_marks(&left, &base, &right, &r, false).left.changed,
            [true]
        );
    }
}
