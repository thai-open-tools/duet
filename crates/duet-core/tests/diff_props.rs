use duet_core::cancel::CancelToken;
use duet_core::text::diff::{DiffInput, DiffOptions, LineDiff, WhitespaceMode, diff_lines};
use duet_core::text::lines::LineIndex;
use proptest::prelude::*;

/// Every line is terminated so LineIndex sees exactly one line per element.
fn to_text(lines: &[String]) -> String {
    lines.iter().map(|l| format!("{l}\n")).collect()
}

fn compute(left: &str, right: &str, opts: DiffOptions) -> LineDiff {
    let (li, ri) = (
        LineIndex::new(left.as_bytes()),
        LineIndex::new(right.as_bytes()),
    );
    diff_lines(
        DiffInput {
            bytes: left.as_bytes(),
            lines: &li,
        },
        DiffInput {
            bytes: right.as_bytes(),
            lines: &ri,
        },
        opts,
        &CancelToken::new(),
    )
    .unwrap()
}

/// Rebuilds the right side from unchanged left lines plus the right side of each hunk.
fn reconstruct<'a>(left: &'a [String], right: &'a [String], diff: &LineDiff) -> Vec<&'a String> {
    let mut out = Vec::new();
    let mut l = 0usize;
    for h in &diff.hunks {
        out.extend(&left[l..h.left.start as usize]);
        out.extend(&right[h.right.start as usize..h.right.end as usize]);
        l = h.left.end as usize;
    }
    out.extend(&left[l..]);
    out
}

proptest! {
    #[test]
    fn unchanged_lines_rebuild_the_right_side(
        left in prop::collection::vec("[ab ]{0,3}", 0..40),
        right in prop::collection::vec("[ab ]{0,3}", 0..40),
    ) {
        let d = compute(&to_text(&left), &to_text(&right), DiffOptions::default());
        prop_assert_eq!(reconstruct(&left, &right, &d), right.iter().collect::<Vec<_>>());
    }

    #[test]
    fn hunks_are_ordered_and_in_bounds(
        left in prop::collection::vec("[ab]{0,2}", 0..40),
        right in prop::collection::vec("[ab]{0,2}", 0..40),
    ) {
        let d = compute(&to_text(&left), &to_text(&right), DiffOptions::default());
        for pair in d.hunks.windows(2) {
            prop_assert!(pair[0].left.end <= pair[1].left.start);
            prop_assert!(pair[0].right.end <= pair[1].right.start);
        }
        if let Some(last) = d.hunks.last() {
            prop_assert!(last.left.end as usize <= left.len());
            prop_assert!(last.right.end as usize <= right.len());
        }
    }

    #[test]
    fn whitespace_only_edits_vanish_under_ignore_all(
        left in prop::collection::vec("[ab]{0,4}", 0..30),
    ) {
        let spaced: Vec<String> =
            left.iter().map(|l| l.chars().flat_map(|c| [c, ' ']).collect()).collect();
        let opts = DiffOptions { whitespace: WhitespaceMode::All, ..DiffOptions::default() };
        prop_assert!(compute(&to_text(&left), &to_text(&spaced), opts).is_identical());
    }
}

use duet_core::text::rows::RowMap;

proptest! {
    #[test]
    fn every_line_appears_once_in_order(
        left in prop::collection::vec("[ab]{0,2}", 0..40),
        right in prop::collection::vec("[ab]{0,2}", 0..40),
    ) {
        let d = compute(&to_text(&left), &to_text(&right), DiffOptions::default());
        let map = RowMap::new(&d);
        let rows: Vec<_> = (0..map.row_count()).map(|r| map.row(r)).collect();
        let lefts: Vec<u32> = rows.iter().filter_map(|r| r.left).collect();
        let rights: Vec<u32> = rows.iter().filter_map(|r| r.right).collect();
        prop_assert_eq!(lefts, (0..left.len() as u32).collect::<Vec<_>>());
        prop_assert_eq!(rights, (0..right.len() as u32).collect::<Vec<_>>());
    }
}
