//! diff3: base against left and right, grouped into stable, one-sided, equal and conflicting
//! regions (LocalDiff `ldDiff3`, `de:986-1040`).

use std::ops::Range;

use super::MergeText;
use crate::cancel::CancelToken;
use crate::text::diff::{DiffError, DiffInput, DiffOptions, LineHunk, diff_lines};
use crate::text::lines::LineIndex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    Stable,
    Left,
    Right,
    Both,
    Conflict,
}

/// Line ranges of one region in the base, the left and the right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub kind: RegionKind,
    pub base: Range<u32>,
    pub left: Range<u32>,
    pub right: Range<u32>,
}

/// What "ignore trailing whitespace" strips from the end of a line.
pub const TRAILING_WHITESPACE: [char; 4] = [' ', '\t', '\u{a0}', '\u{3000}'];

/// The comparison key of a line: itself, or without trailing whitespace when `trail`.
pub fn line_key(line: &str, trail: bool) -> &str {
    if trail {
        line.trim_end_matches(TRAILING_WHITESPACE)
    } else {
        line
    }
}

/// One side's hunk against the base, in base and side lines.
struct Hunk {
    /// 0 = left, 1 = right: the tie-break of equal base starts.
    side: u8,
    base: Range<i64>,
    side_range: Range<i64>,
}

/// A text of key lines (each key + "\n") for comparing with trailing whitespace ignored.
fn key_text(side: &MergeText) -> String {
    let mut out = String::with_capacity(side.text().len() + side.line_count() as usize);
    for n in 0..side.line_count() {
        out.push_str(line_key(side.line(n), true));
        out.push('\n');
    }
    out
}

fn hunks_of(
    base: DiffInput<'_>,
    side: DiffInput<'_>,
    which: u8,
    cancel: &CancelToken,
) -> Result<Vec<Hunk>, DiffError> {
    let opts = DiffOptions {
        ignore_eol: true,
        ..Default::default()
    };
    // `base` goes first, so a hunk's `left` is the base range and `right` the side's.
    let diff = diff_lines(base, side, opts, cancel)?;
    Ok(diff
        .hunks
        .into_iter()
        .map(|LineHunk { left, right }| Hunk {
            side: which,
            base: i64::from(left.start)..i64::from(left.end),
            side_range: i64::from(right.start)..i64::from(right.end),
        })
        .collect())
}

pub fn diff3(
    left: &MergeText,
    base: &MergeText,
    right: &MergeText,
    trail: bool,
    cancel: &CancelToken,
) -> Result<Vec<Region>, DiffError> {
    let keyed = trail.then(|| (key_text(left), key_text(base), key_text(right)));
    let indexed = keyed.as_ref().map(|(l, b, r)| {
        (
            LineIndex::new(l.as_bytes()),
            LineIndex::new(b.as_bytes()),
            LineIndex::new(r.as_bytes()),
        )
    });
    let (l_in, b_in, r_in) = match (&keyed, &indexed) {
        (Some((l, b, r)), Some((li, bi, ri))) => (
            DiffInput {
                bytes: l.as_bytes(),
                lines: li,
            },
            DiffInput {
                bytes: b.as_bytes(),
                lines: bi,
            },
            DiffInput {
                bytes: r.as_bytes(),
                lines: ri,
            },
        ),
        _ => (
            DiffInput {
                bytes: left.text().as_bytes(),
                lines: left.line_index(),
            },
            DiffInput {
                bytes: base.text().as_bytes(),
                lines: base.line_index(),
            },
            DiffInput {
                bytes: right.text().as_bytes(),
                lines: right.line_index(),
            },
        ),
    };
    let mut all = hunks_of(b_in, l_in, 0, cancel)?;
    cancel.check()?;
    all.extend(hunks_of(b_in, r_in, 1, cancel)?);
    all.sort_by_key(|h| (h.base.start, h.side));

    let same_keys = |l: &Range<i64>, r: &Range<i64>| {
        l.end - l.start == r.end - r.start
            && (0..l.end - l.start).all(|k| {
                line_key(left.line((l.start + k) as u32), trail)
                    == line_key(right.line((r.start + k) as u32), trail)
            })
    };

    let base_len = i64::from(base.line_count());
    let mut regions = Vec::new();
    let (mut cur, mut d_left, mut d_right) = (0i64, 0i64, 0i64);
    let range = |r: Range<i64>| r.start as u32..r.end as u32;
    let stable = |regions: &mut Vec<Region>, end: i64, cur: &mut i64, dl: i64, dr: i64| {
        if end > *cur {
            regions.push(Region {
                kind: RegionKind::Stable,
                base: range(*cur..end),
                left: range(*cur + dl..end + dl),
                right: range(*cur + dr..end + dr),
            });
            *cur = end;
        }
    };
    let mut i = 0;
    while i < all.len() {
        let start = all[i].base.start;
        let mut end = all[i].base.end;
        let first = i;
        i += 1;
        while i < all.len() && all[i].base.start <= end {
            end = end.max(all[i].base.end);
            i += 1;
        }
        let group = &all[first..i];
        stable(&mut regions, start, &mut cur, d_left, d_right);
        // A side's range runs from its first hunk to its last, widened to the group's span.
        let extent = |side: u8, delta: i64| -> (Option<Range<i64>>, i64) {
            let mine: Vec<&Hunk> = group.iter().filter(|h| h.side == side).collect();
            let (Some(a), Some(z)) = (mine.first(), mine.last()) else {
                return (None, delta);
            };
            let range =
                a.side_range.start - (a.base.start - start)..z.side_range.end + (end - z.base.end);
            let shift: i64 = mine
                .iter()
                .map(|h| (h.side_range.end - h.side_range.start) - (h.base.end - h.base.start))
                .sum();
            (Some(range), delta + shift)
        };
        let (l_range, next_left) = extent(0, d_left);
        let (r_range, next_right) = extent(1, d_right);
        let kind = match (&l_range, &r_range) {
            (Some(_), None) => RegionKind::Left,
            (None, Some(_)) => RegionKind::Right,
            (Some(l), Some(r)) if same_keys(l, r) => RegionKind::Both,
            _ => RegionKind::Conflict,
        };
        let l_range = l_range.unwrap_or(start + d_left..end + d_left);
        let r_range = r_range.unwrap_or(start + d_right..end + d_right);
        regions.push(Region {
            kind,
            base: range(start..end),
            left: range(l_range),
            right: range(r_range),
        });
        d_left = next_left;
        d_right = next_right;
        cur = end;
    }
    stable(&mut regions, base_len, &mut cur, d_left, d_right);
    Ok(regions)
}

/// The regions of the same merge with left and right traded: `diff3(right, base, left)`.
/// diff3 is symmetric, as each side is diffed against the base on its own and hunks with equal
/// base starts always join one group, so a swap needs no new merge.
pub fn mirrored(regions: &[Region]) -> Vec<Region> {
    regions
        .iter()
        .map(|region| Region {
            kind: match region.kind {
                RegionKind::Left => RegionKind::Right,
                RegionKind::Right => RegionKind::Left,
                kind => kind,
            },
            base: region.base.clone(),
            left: region.right.clone(),
            right: region.left.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn text(lines: &[&str]) -> MergeText {
        MergeText::typed(lines.iter().map(|l| format!("{l}\n")).collect())
    }

    fn rg(kind: RegionKind, base: Range<u32>, left: Range<u32>, right: Range<u32>) -> Region {
        Region {
            kind,
            base,
            left,
            right,
        }
    }

    fn run(left: &MergeText, base: &MergeText, right: &MergeText, trail: bool) -> Vec<Region> {
        diff3(left, base, right, trail, &CancelToken::new()).unwrap()
    }

    #[test]
    fn regions_split_into_stable_left_right_both_and_conflict() {
        use RegionKind::*;
        let base = text(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"]);
        let left = text(&["a", "L", "c", "d", "e", "S", "g", "h", "X", "j", "k"]);
        let right = text(&["a", "b", "c", "R", "e", "S", "g", "h", "Y", "j", "k"]);
        let same = |kind, s: u32, e: u32| rg(kind, s..e, s..e, s..e);
        assert_eq!(
            run(&left, &base, &right, false),
            [
                same(Stable, 0, 1),
                same(Left, 1, 2),
                same(Stable, 2, 3),
                same(Right, 3, 4),
                same(Stable, 4, 5),
                same(Both, 5, 6),
                same(Stable, 6, 8),
                same(Conflict, 8, 9),
                same(Stable, 9, 11),
            ]
        );
    }

    #[test]
    fn insertions_and_deletions_shift_the_side_ranges() {
        use RegionKind::*;
        let base = text(&["a", "b", "c", "d", "e"]);
        let left = text(&["a", "N1", "N2", "b", "c", "d", "e"]);
        let right = text(&["a", "b", "c", "e"]);
        assert_eq!(
            run(&left, &base, &right, false),
            [
                rg(Stable, 0..1, 0..1, 0..1),
                rg(Left, 1..1, 1..3, 1..1),
                rg(Stable, 1..3, 3..5, 1..3),
                rg(Right, 3..4, 5..6, 3..3),
                rg(Stable, 4..5, 6..7, 3..4),
            ]
        );
    }

    #[test]
    fn touching_changes_and_same_point_inserts_join() {
        use RegionKind::*;
        let base = text(&["a", "b", "c", "d"]);
        let left = text(&["a", "X", "c", "d"]);
        let right = text(&["a", "b", "Y", "d"]);
        assert_eq!(
            run(&left, &base, &right, false),
            [
                rg(Stable, 0..1, 0..1, 0..1),
                rg(Conflict, 1..3, 1..3, 1..3),
                rg(Stable, 3..4, 3..4, 3..4),
            ]
        );

        let base = text(&["a", "b"]);
        let left = text(&["a", "X", "b"]);
        let right = text(&["a", "Y", "b"]);
        assert_eq!(
            run(&left, &base, &right, false),
            [
                rg(Stable, 0..1, 0..1, 0..1),
                rg(Conflict, 1..1, 1..2, 1..2),
                rg(Stable, 1..2, 2..3, 2..3),
            ]
        );
        let right = text(&["a", "X", "b"]);
        assert_eq!(
            run(&left, &base, &right, false),
            [
                rg(Stable, 0..1, 0..1, 0..1),
                rg(Both, 1..1, 1..2, 1..2),
                rg(Stable, 1..2, 2..3, 2..3),
            ]
        );
    }

    #[test]
    fn empty_and_one_sided_inputs_merge() {
        use RegionKind::*;
        let none = MergeText::typed(String::new());
        assert_eq!(run(&none, &none, &none, false), []);
        assert_eq!(
            run(&text(&["a"]), &none, &none, false),
            [rg(Left, 0..0, 0..1, 0..0)]
        );
        assert_eq!(
            run(&none, &text(&["a", "b"]), &text(&["a", "b"]), false),
            [rg(Left, 0..2, 0..0, 0..2)]
        );
        assert_eq!(
            run(&text(&["b"]), &text(&["a"]), &text(&["b"]), false),
            [rg(Both, 0..1, 0..1, 0..1)]
        );
    }

    #[test]
    fn line_endings_never_make_a_change() {
        let base = MergeText::typed("a\r\nb\r\n".into());
        let left = MergeText::typed("a\nb\n".into());
        let right = MergeText::typed("a\r\nb".into());
        assert_eq!(
            run(&left, &base, &right, false),
            [rg(RegionKind::Stable, 0..2, 0..2, 0..2)]
        );
    }

    #[test]
    fn trailing_whitespace_can_be_ignored() {
        use RegionKind::*;
        let base = text(&["a", "b"]);
        let left = text(&["a  ", "b"]);
        let right = text(&["a", "c"]);
        assert_eq!(
            run(&left, &base, &right, false),
            [rg(Conflict, 0..2, 0..2, 0..2)]
        );
        assert_eq!(
            run(&left, &base, &right, true),
            [rg(Stable, 0..1, 0..1, 0..1), rg(Right, 1..2, 1..2, 1..2)]
        );

        let base = text(&["b"]);
        let left = text(&["c "]);
        let right = text(&["c"]);
        assert_eq!(
            run(&left, &base, &right, true),
            [rg(Both, 0..1, 0..1, 0..1)]
        );
        assert_eq!(
            run(&left, &base, &right, false),
            [rg(Conflict, 0..1, 0..1, 0..1)]
        );

        assert_eq!(line_key("x \t\u{a0}\u{3000}", true), "x");
        assert_eq!(line_key(" x ", true), " x");
        assert_eq!(line_key("x ", false), "x ");
    }

    fn side() -> impl Strategy<Value = Vec<&'static str>> {
        proptest::collection::vec(
            prop_oneof![Just("a"), Just("b"), Just("c"), Just("d")],
            0..12,
        )
    }

    /// Lines that differ from others only in trailing whitespace, for the option.
    fn spaced_side() -> impl Strategy<Value = Vec<&'static str>> {
        proptest::collection::vec(
            prop_oneof![Just("a"), Just("a "), Just("b"), Just("b\t"), Just("c")],
            0..12,
        )
    }

    proptest! {
        /// diff3 is symmetric: trading the sides mirrors the regions, so a swap needs no new
        /// merge (I1).
        #[test]
        fn mirrored_regions_are_the_merge_with_the_sides_traded(
            l in spaced_side(),
            b in spaced_side(),
            r in spaced_side(),
            trail in any::<bool>(),
        ) {
            let (left, base, right) = (text(&l), text(&b), text(&r));
            prop_assert_eq!(
                mirrored(&run(&left, &base, &right, trail)),
                run(&right, &base, &left, trail)
            );
        }

        #[test]
        fn regions_rebuild_every_input(l in side(), b in side(), r in side()) {
            let (left, base, right) = (text(&l), text(&b), text(&r));
            let regions = run(&left, &base, &right, false);
            let (mut pb, mut pl, mut pr) = (0, 0, 0);
            for g in &regions {
                prop_assert_eq!(g.base.start, pb);
                prop_assert_eq!(g.left.start, pl);
                prop_assert_eq!(g.right.start, pr);
                (pb, pl, pr) = (g.base.end, g.left.end, g.right.end);
                if g.kind == RegionKind::Stable {
                    prop_assert!(!g.base.is_empty());
                    let lines = |t: &MergeText, r: &Range<u32>| t.lines(r.clone());
                    prop_assert_eq!(lines(&left, &g.left), lines(&base, &g.base));
                    prop_assert_eq!(lines(&right, &g.right), lines(&base, &g.base));
                }
            }
            prop_assert_eq!(pb, base.line_count());
            prop_assert_eq!(pl, left.line_count());
            prop_assert_eq!(pr, right.line_count());

            // With one side unchanged, only the other side's regions remain.
            if l == b {
                let regions = run(&left, &base, &right, false);
                prop_assert!(regions.iter().all(|g| matches!(g.kind, RegionKind::Stable | RegionKind::Right)));
                let mut out = Vec::new();
                for g in &regions {
                    if g.kind == RegionKind::Right {
                        out.extend(right.lines(g.right.clone()));
                    } else {
                        out.extend(left.lines(g.left.clone()));
                    }
                }
                prop_assert_eq!(out, right.lines(0..right.line_count()));
            }
            if r == b {
                let regions = run(&left, &base, &right, false);
                prop_assert!(regions.iter().all(|g| matches!(g.kind, RegionKind::Stable | RegionKind::Left)));
                let mut out = Vec::new();
                for g in &regions {
                    out.extend(left.lines(g.left.clone()));
                }
                prop_assert_eq!(out, left.lines(0..left.line_count()));
            }
        }
    }
}
