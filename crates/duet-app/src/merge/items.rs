//! The merge stage's list (LocalDiff `renderMerge`, `stableHTML`, `js:350-482`): which items
//! show and in what order, and the words of a card that need no drawing.

use std::collections::HashSet;
use std::ops::Range;

use duet_core::text::merge::diff3::RegionKind;
use duet_core::text::merge::session::{MergeSession, Resolution};

use crate::format::count_of;
use crate::i18n::{tr, tr_key, trf};

/// A stable run longer than this folds to its first and last `FOLD_CONTEXT` lines.
pub const FOLD_MIN: u32 = 9;
pub const FOLD_CONTEXT: u32 = 3;
/// Lines a card column shows before "show all".
pub const COLUMN_MAX: u32 = 400;
/// Rows a result field grows to before it scrolls (`TA_MAX_ROWS`).
pub const FIELD_ROWS_MAX: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Banner {
    Same,
    TrailNote,
    Empty,
}

/// One entry of the card list. A stable run is a box of `Row`s, with a `Fold` bar in its
/// middle while folded or a `Collapse` bar on top while open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Banner(Banner),
    /// `line` is the left's (stable parts show the left text, R3). `first` and `last`: the
    /// row opens or closes its box, so it draws the box's top or bottom edge.
    Row {
        chunk: usize,
        line: u32,
        first: bool,
        last: bool,
    },
    Fold {
        chunk: usize,
        hidden: u32,
    },
    Collapse {
        chunk: usize,
        lines: u32,
    },
    Card(usize),
}

impl Item {
    /// The chunk the item belongs to; None for a banner.
    pub fn chunk(self) -> Option<usize> {
        match self {
            Item::Banner(_) => None,
            Item::Row { chunk, .. }
            | Item::Fold { chunk, .. }
            | Item::Collapse { chunk, .. }
            | Item::Card(chunk) => Some(chunk),
        }
    }
}

/// The list in order: a banner when there is nothing to merge (or the option's note), every
/// chunk, and a banner when the three texts are empty (`renderMerge`).
pub fn layout_items(session: &MergeSession, expanded: &HashSet<usize>) -> Vec<Item> {
    let mut items = Vec::new();
    if session.changes().is_empty() {
        items.push(Item::Banner(Banner::Same));
    } else if session.trail() {
        items.push(Item::Banner(Banner::TrailNote));
    }
    for chunk in 0..session.regions().len() {
        items.extend(chunk_items(session, chunk, expanded.contains(&chunk)));
    }
    if session.regions().is_empty() {
        items.push(Item::Banner(Banner::Empty));
    }
    items
}

/// One chunk's items: its card, or its stable box (`stableHTML`).
pub fn chunk_items(session: &MergeSession, chunk: usize, open: bool) -> Vec<Item> {
    let Some(region) = session.regions().get(chunk) else {
        return Vec::new();
    };
    if region.kind != RegionKind::Stable {
        return vec![Item::Card(chunk)];
    }
    let (start, end) = (region.left.start, region.left.end);
    let n = end - start;
    // A bar on top opens the box itself, so the first row then draws no top edge.
    let rows = |range: Range<u32>, top: bool| {
        range.map(move |line| Item::Row {
            chunk,
            line,
            first: top && line == start,
            last: line + 1 == end,
        })
    };
    if n <= FOLD_MIN {
        return rows(start..end, true).collect();
    }
    if open {
        let mut items = vec![Item::Collapse { chunk, lines: n }];
        items.extend(rows(start..end, false));
        return items;
    }
    let mut items: Vec<Item> = rows(start..start + FOLD_CONTEXT, true).collect();
    items.push(Item::Fold {
        chunk,
        hidden: n - 2 * FOLD_CONTEXT,
    });
    items.extend(rows(end - FOLD_CONTEXT..end, true));
    items
}

/// "no lines", "line 5" or "3–7", in 1-based lines (`rangeText`).
pub fn range_text(range: Range<u32>) -> String {
    match range.end.saturating_sub(range.start) {
        0 => tr("no lines").to_owned(),
        1 => trf("line {}", &[&range.end]),
        _ => format!("{}–{}", range.start + 1, range.end),
    }
}

/// Where a change sits in the base (`whereText`).
pub fn where_text(base: Range<u32>) -> String {
    if base.end > base.start {
        trf("Base {}", &[&range_text(base)])
    } else if base.start > 0 {
        trf("Inserted after base line {}", &[&base.start])
    } else {
        tr("Inserted at the start of the file").to_owned()
    }
}

pub fn kind_badge(kind: RegionKind) -> &'static str {
    match kind {
        RegionKind::Stable => "",
        RegionKind::Left => tr_key("Left changed"),
        RegionKind::Right => tr_key("Right changed"),
        RegionKind::Both => tr_key("Same change on both sides"),
        RegionKind::Conflict => tr_key("CONFLICT"),
    }
}

/// "Not resolved", or the choice with "(automatic)" when nobody made it (`stateHTML`).
pub fn state_text(resolution: Option<&Resolution>) -> String {
    match resolution {
        None => tr("Not resolved").to_owned(),
        Some(r) if r.auto => trf("✓ {} (automatic)", &[&tr(r.how.label())]),
        Some(r) => format!("✓ {}", tr(r.how.label())),
    }
}

/// The size of a part's result, beside its choice buttons (`resCountText`).
pub fn count_text(resolution: Option<&Resolution>) -> String {
    match resolution {
        None => tr("Not chosen yet").to_owned(),
        Some(r) if r.lines.is_empty() => tr("Empty: no lines in the result").to_owned(),
        Some(r) => count_of(r.lines.len() as u64, "line", "lines"),
    }
}

/// A part's result as its field holds it: the lines joined by LF, empty when not resolved.
pub fn field_value(resolution: Option<&Resolution>) -> String {
    resolution.map(|r| r.lines.join("\n")).unwrap_or_default()
}

/// What an empty result field says (`taPlaceholder`).
pub fn field_placeholder(resolved: bool) -> &'static str {
    if resolved {
        tr("(empty: this part has no lines in the result)")
    } else {
        tr("Not resolved: press a button above, or type the result here")
    }
}

/// The fewest and most rows a result field shows (`taRows`, `TA_MAX_ROWS`): an open conflict's
/// field has room for two lines (R5).
pub fn field_rows(resolved: bool) -> (usize, usize) {
    (if resolved { 1 } else { 2 }, FIELD_ROWS_MAX)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use duet_core::cancel::CancelToken;
    use duet_core::text::merge::MergeText;
    use duet_core::text::merge::diff3::diff3;
    use duet_core::text::merge::session::How;

    use super::*;

    fn text(lines: &[String]) -> Arc<MergeText> {
        Arc::new(MergeText::typed(
            lines.iter().map(|l| format!("{l}\n")).collect(),
        ))
    }

    fn session(left: &[String], base: &[String], right: &[String], trail: bool) -> MergeSession {
        let (l, b, r) = (text(left), text(base), text(right));
        let regions = diff3(&l, &b, &r, trail, &CancelToken::new()).unwrap();
        MergeSession::new(l, b, r, regions, trail)
    }

    fn strings(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| (*l).to_owned()).collect()
    }

    /// Lines "1" to "n", and the same with the last line changed: n-1 stable lines, then a
    /// change on the left.
    fn long_run(n: usize) -> MergeSession {
        let base: Vec<String> = (1..=n).map(|k| k.to_string()).collect();
        let mut left = base.clone();
        left[n - 1] = "X".to_owned();
        session(&left, &base, &base, false)
    }

    fn row(line: u32, first: bool, last: bool) -> Item {
        Item::Row {
            chunk: 0,
            line,
            first,
            last,
        }
    }

    #[test]
    fn layout_items_fold_long_stable_runs() {
        let none = HashSet::new();
        let twelve = long_run(13);
        assert_eq!(
            layout_items(&twelve, &none),
            [
                row(0, true, false),
                row(1, false, false),
                row(2, false, false),
                Item::Fold {
                    chunk: 0,
                    hidden: 6
                },
                row(9, false, false),
                row(10, false, false),
                row(11, false, true),
                Item::Card(1),
            ]
        );

        // Open, the bar comes first and draws the box's top; then every line.
        let mut expected = vec![Item::Collapse {
            chunk: 0,
            lines: 12,
        }];
        expected.extend((0..12).map(|line| row(line, false, line == 11)));
        expected.push(Item::Card(1));
        assert_eq!(layout_items(&twelve, &HashSet::from([0])), expected);

        // Ten lines fold; nine do not.
        assert_eq!(
            layout_items(&long_run(11), &none)[3],
            Item::Fold {
                chunk: 0,
                hidden: 4
            }
        );
        let mut nine: Vec<Item> = (0..9).map(|line| row(line, line == 0, line == 8)).collect();
        nine.push(Item::Card(1));
        assert_eq!(layout_items(&long_run(10), &none), nine);

        // The banners.
        let same = strings(&["a", "b"]);
        assert_eq!(
            layout_items(&session(&same, &same, &same, false), &none),
            [
                Item::Banner(Banner::Same),
                row(0, true, false),
                row(1, false, true)
            ]
        );
        let empty = session(&[], &[], &[], false);
        assert_eq!(
            layout_items(&empty, &none),
            [Item::Banner(Banner::Same), Item::Banner(Banner::Empty)]
        );
        let (left, base) = (strings(&["a", "X"]), strings(&["a", "b"]));
        assert_eq!(
            layout_items(&session(&left, &base, &base, true), &none),
            [
                Item::Banner(Banner::TrailNote),
                row(0, true, true),
                Item::Card(1)
            ]
        );
        assert_eq!(
            layout_items(&session(&same, &same, &same, true), &none)[0],
            Item::Banner(Banner::Same),
            "nothing to merge says so, with or without the option"
        );
    }

    #[test]
    fn a_change_between_stable_runs_keeps_each_box_whole() {
        let base = strings(&["a", "b", "c"]);
        let left = strings(&["a", "B", "c"]);
        let s = session(&left, &base, &base, false);
        assert_eq!(
            layout_items(&s, &HashSet::new()),
            [
                row(0, true, true),
                Item::Card(1),
                Item::Row {
                    chunk: 2,
                    line: 2,
                    first: true,
                    last: true
                },
            ]
        );
    }

    #[test]
    fn range_and_where_texts_follow_localdiff() {
        assert_eq!(range_text(2..2), "no lines");
        assert_eq!(range_text(4..5), "line 5");
        assert_eq!(range_text(2..7), "3–7");
        assert_eq!(where_text(4..5), "Base line 5");
        assert_eq!(where_text(2..7), "Base 3–7");
        assert_eq!(where_text(4..4), "Inserted after base line 4");
        assert_eq!(where_text(0..0), "Inserted at the start of the file");
    }

    #[test]
    fn badges_and_states_read_as_in_localdiff() {
        assert_eq!(kind_badge(RegionKind::Left), "Left changed");
        assert_eq!(kind_badge(RegionKind::Right), "Right changed");
        assert_eq!(kind_badge(RegionKind::Both), "Same change on both sides");
        assert_eq!(kind_badge(RegionKind::Conflict), "CONFLICT");
        let res = |how, auto| Resolution {
            how,
            lines: Vec::new(),
            auto,
        };
        assert_eq!(state_text(None), "Not resolved");
        assert_eq!(
            state_text(Some(&res(How::Left, true))),
            "✓ Use left (automatic)"
        );
        assert_eq!(state_text(Some(&res(How::Right, false))), "✓ Use right");
        assert_eq!(state_text(Some(&res(How::Manual, false))), "✓ Edited");
    }

    #[test]
    fn result_counts_and_placeholders_read_as_in_localdiff() {
        let res = |lines: &[&str]| Resolution {
            how: How::Manual,
            lines: strings(lines),
            auto: false,
        };
        assert_eq!(count_text(None), "Not chosen yet");
        assert_eq!(count_text(Some(&res(&["x"]))), "1 line");
        assert_eq!(count_text(Some(&res(&["x", ""]))), "2 lines");
        assert_eq!(count_text(Some(&res(&[]))), "Empty: no lines in the result");
        assert_eq!(field_value(None), "");
        assert_eq!(field_value(Some(&res(&["x", "y", ""]))), "x\ny\n");
        assert_eq!(field_value(Some(&res(&[]))), "");
        assert_eq!(
            field_placeholder(true),
            "(empty: this part has no lines in the result)"
        );
        assert_eq!(
            field_placeholder(false),
            "Not resolved: press a button above, or type the result here"
        );
        assert_eq!(field_rows(true), (1, 30));
        assert_eq!(
            field_rows(false),
            (2, 30),
            "an open conflict's field shows two rows"
        );
    }
}
