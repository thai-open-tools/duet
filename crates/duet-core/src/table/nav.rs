//! Row filters, find and go-to-line over a `TableDiff`.

use crate::table::Table;
use crate::table::compare::{Counts, RowState, TableDiff, data_offset};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Differences,
    Same,
    LeftOnly,
    RightOnly,
}

impl Filter {
    pub const ALL: [Filter; 5] = [
        Filter::All,
        Filter::Differences,
        Filter::Same,
        Filter::LeftOnly,
        Filter::RightOnly,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Filter::All => "all",
            Filter::Differences => "diff",
            Filter::Same => "same",
            Filter::LeftOnly => "left",
            Filter::RightOnly => "right",
        }
    }

    pub fn from_key(key: &str) -> Option<Filter> {
        Filter::ALL.into_iter().find(|f| f.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => crate::i18n::tr_key("All"),
            Filter::Differences => crate::i18n::tr_key("Differences"),
            Filter::Same => crate::i18n::tr_key("Same"),
            Filter::LeftOnly => crate::i18n::tr_key("Left only"),
            Filter::RightOnly => crate::i18n::tr_key("Right only"),
        }
    }

    pub fn count(self, counts: &Counts) -> usize {
        match self {
            Filter::All => counts.same + counts.differs + counts.left_only + counts.right_only,
            Filter::Differences => counts.differs + counts.left_only + counts.right_only,
            Filter::Same => counts.same,
            Filter::LeftOnly => counts.left_only,
            Filter::RightOnly => counts.right_only,
        }
    }

    fn shows(self, state: RowState) -> bool {
        match self {
            Filter::All => true,
            Filter::Differences => state != RowState::Same,
            Filter::Same => state == RowState::Same,
            Filter::LeftOnly => state == RowState::LeftOnly,
            Filter::RightOnly => state == RowState::RightOnly,
        }
    }
}

/// The rows a filter shows: `rows[pos]` is an index into `TableDiff::rows`; `stops` are the
/// positions of non-same rows; `position(row)` maps back (None when filtered out).
const NOT_SHOWN: u32 = u32::MAX;

#[derive(Default)]
pub struct View {
    pub rows: Vec<u32>,
    pub stops: Vec<u32>,
    pos_of: Vec<u32>,
}

impl View {
    pub fn position(&self, row: usize) -> Option<usize> {
        self.pos_of
            .get(row)
            .filter(|&&p| p != NOT_SHOWN)
            .map(|&p| p as usize)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

pub fn filtered(diff: &TableDiff, filter: Filter) -> View {
    let mut view = View {
        rows: Vec::new(),
        stops: Vec::new(),
        pos_of: vec![NOT_SHOWN; diff.rows.len()],
    };
    for (index, row) in diff.rows.iter().enumerate() {
        if !filter.shows(row.state) {
            continue;
        }
        let pos = view.rows.len() as u32;
        view.pos_of[index] = pos;
        if row.state != RowState::Same {
            view.stops.push(pos);
        }
        view.rows.push(index as u32);
    }
    view
}

/// The most cells one find call looks at (both sides count), so a miss on a 160 MB table
/// never holds the UI for long; the next call goes on from where it stopped.
pub const FIND_CELLS: usize = 2_000_000;

/// Where a find that ran out of cells stopped: the rows it visited and the next column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FindResume {
    rows: usize,
    column: usize,
}

/// What a find call came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// A match: its display position and column.
    At(usize, usize),
    /// Every row was visited once without a match.
    Nothing,
    /// The cells ran out first: another call with this resume goes on.
    Paused(FindResume),
}

/// One find call: the text, the direction, where it starts and how many cells it may look at.
pub struct Search<'a> {
    pub needle: &'a str,
    pub backward: bool,
    /// The display position the search starts after: None starts at the top (the bottom when
    /// backward).
    pub from: Option<usize>,
    /// Where the last call with the same text, direction and start stopped, else the default.
    pub resume: FindResume,
    pub cells: usize,
}

/// The lowercased needle. A cell that is all ASCII is searched without allocating.
struct Needle(String);

impl Needle {
    fn is_in(&self, cell: &str) -> bool {
        let needle = self.0.as_bytes();
        if cell.is_ascii() {
            // An ASCII cell lowercases to ASCII, so only an ASCII needle can be in it.
            self.0.is_ascii()
                && cell
                    .as_bytes()
                    .windows(needle.len())
                    .any(|w| w.eq_ignore_ascii_case(needle))
        } else {
            cell.to_lowercase().contains(&self.0)
        }
    }
}

/// Case-insensitive substring search over every cell of both sides, row by row in view order
/// from just after `from` (or the top / bottom), wrapping once. A call looks at most at
/// `cells` cells and then pauses; the rows visited count across the calls, so the search
/// still sees each row once before it finds nothing.
pub fn find(
    diff: &TableDiff,
    left: Option<&Table>,
    right: Option<&Table>,
    header: bool,
    view: &View,
    search: &Search,
) -> Found {
    let n = view.len();
    if n == 0 || search.needle.is_empty() {
        return Found::Nothing;
    }
    let needle = Needle(search.needle.to_lowercase());
    let start = match search.from {
        Some(pos) => pos as isize,
        None if search.backward => n as isize,
        None => -1,
    };
    // Each column looks at a cell on each side; a call always moves at least one column.
    let mut budget = (search.cells / 2).max(1);
    let FindResume {
        mut rows,
        mut column,
    } = search.resume;
    while rows < n {
        let step = rows as isize + 1;
        let pos = (start + if search.backward { -step } else { step }).rem_euclid(n as isize);
        let pos = pos as usize;
        let row = view.rows[pos] as usize;
        while column < diff.columns.len() {
            if budget == 0 {
                return Found::Paused(FindResume { rows, column });
            }
            budget -= 1;
            let hit = |t, left| {
                side_value(diff, t, header, row, column, left).is_some_and(|v| needle.is_in(v))
            };
            if hit(left, true) || hit(right, false) {
                return Found::At(pos, column);
            }
            column += 1;
        }
        column = 0;
        rows += 1;
    }
    Found::Nothing
}

/// The paired row holding file line `line`: the left side first, then the right (R14 go-to).
pub fn row_for_line(
    diff: &TableDiff,
    left: Option<&Table>,
    right: Option<&Table>,
    header: bool,
    line: u32,
) -> Option<usize> {
    // A header line is no data row, so it counts as "not on this side".
    let data_row = |table: Option<&Table>| {
        let table = table?;
        let offset = data_offset(table, header);
        Some(table.record_at_line(line)?.checked_sub(offset)? as u32)
    };
    match data_row(left) {
        Some(data) => diff.rows.iter().position(|p| p.left == Some(data)),
        None => {
            let data = data_row(right)?;
            diff.rows.iter().position(|p| p.right == Some(data))
        }
    }
}

/// The value a side shows in a column of a paired row: None when the side has no row or no
/// such column, Some("") for a cell past a short record's end.
pub fn side_value<'a>(
    diff: &TableDiff,
    table: Option<&'a Table>,
    header: bool,
    row: usize,
    column: usize,
    left: bool,
) -> Option<&'a str> {
    let table = table?;
    let paired = diff.rows.get(row)?;
    let data = (if left { paired.left } else { paired.right })? as usize;
    let col = &diff.columns[column];
    let index = if left { col.left } else { col.right }?;
    Some(
        table
            .record(data + data_offset(table, header))
            .get(index)
            .unwrap_or(""),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancelToken;
    use crate::table::Delimiter;
    use crate::table::compare::{CompareOptions, compare};
    use crate::table::parse::parse_str;

    fn csv(text: &str) -> Table {
        parse_str(text, Delimiter::Comma)
    }

    const L: &str = "id,v\n1,a\n2,b\n3,c\n3,x\n";
    const R: &str = "id,v\n3,c\n1,A\n4,d\n3,y\n";

    fn opts() -> CompareOptions {
        CompareOptions {
            header: true,
            keys: "id".into(),
            ..Default::default()
        }
    }

    fn diff(l: &Table, r: &Table) -> TableDiff {
        compare(Some(l), Some(r), &opts(), &CancelToken::new()).unwrap()
    }

    #[test]
    fn filters_list_rows_and_stops() {
        let (l, r) = (csv(L), csv(R));
        let d = diff(&l, &r);
        let all = filtered(&d, Filter::All);
        assert_eq!(all.rows, [0, 1, 2, 3, 4]);
        assert_eq!(all.stops, [0, 1, 2, 4]);
        assert_eq!(filtered(&d, Filter::Differences).len(), 4);
        let same = filtered(&d, Filter::Same);
        assert_eq!((same.rows.clone(), same.stops.len()), (vec![3], 0));
        assert_eq!(filtered(&d, Filter::LeftOnly).rows, [2]);
        assert_eq!(filtered(&d, Filter::RightOnly).rows, [1]);
        assert_eq!(same.position(3), Some(0));
        assert_eq!(same.position(0), None);
        let diffs = filtered(&d, Filter::Differences);
        assert_eq!(diffs.position(4), Some(3));
        assert_eq!(diffs.stops, [0, 1, 2, 3]);
        assert!(!diffs.is_empty());
    }

    #[test]
    fn filter_keys_labels_and_counts() {
        for f in Filter::ALL {
            assert_eq!(Filter::from_key(f.key()), Some(f));
        }
        assert_eq!(Filter::from_key("nope"), None);
        assert_eq!(Filter::Differences.label(), "Differences");
        assert_eq!(Filter::LeftOnly.label(), "Left only");
        let (l, r) = (csv(L), csv(R));
        let d = diff(&l, &r);
        let counts: Vec<usize> = Filter::ALL.iter().map(|f| f.count(&d.counts)).collect();
        assert_eq!(counts, [5, 4, 1, 1, 1]);
    }

    /// An uncapped find: Some((position, column)) or None.
    fn find_all(
        d: &TableDiff,
        l: &Table,
        r: &Table,
        v: &View,
        from: Option<usize>,
        needle: &str,
        backward: bool,
    ) -> Option<(usize, usize)> {
        let search = Search {
            needle,
            backward,
            from,
            resume: FindResume::default(),
            cells: usize::MAX,
        };
        match find(d, Some(l), Some(r), true, v, &search) {
            Found::At(pos, column) => Some((pos, column)),
            Found::Nothing => None,
            Found::Paused(_) => panic!("an uncapped find never pauses"),
        }
    }

    #[test]
    fn find_is_case_insensitive_wraps_once_and_searches_both_sides() {
        let (l, r) = (csv(L), csv(R));
        let d = diff(&l, &r);
        let v = filtered(&d, Filter::All);
        let f = |from, needle, back| find_all(&d, &l, &r, &v, from, needle, back);
        assert_eq!(f(None, "a", false), Some((0, 1)));
        assert_eq!(f(Some(0), "C", false), Some((3, 1)));
        assert_eq!(f(Some(0), "y", false), Some((4, 1)));
        assert_eq!(f(None, "zz", false), None);
        assert_eq!(f(Some(0), "x", true), Some((4, 1)), "backward wraps");
        assert_eq!(f(Some(4), "", false), None, "an empty needle finds nothing");
        assert_eq!(f(None, "d", true), Some((1, 1)), "right-only row, backward");
    }

    #[test]
    fn find_matches_ascii_and_thai_text_ignoring_case() {
        let l = csv("k,v\n1,xFOOy\n2,ที่บ้านปู่\n3,ÉtéABC\n4,\u{212A}elvin\n");
        let r = csv("k,v\n1,q\n2,q\n3,q\n4,q\n");
        let d = diff_by_order(&l, &r);
        let v = filtered(&d, Filter::All);
        let f = |needle| find_all(&d, &l, &r, &v, None, needle, false);
        assert_eq!(f("foo"), Some((0, 1)), "an ASCII cell, ASCII needle");
        assert_eq!(f("FoO"), Some((0, 1)));
        assert_eq!(f("บ้าน"), Some((1, 1)), "a Thai needle");
        assert_eq!(
            f("abc"),
            Some((2, 1)),
            "an ASCII needle in a non-ASCII cell"
        );
        assert_eq!(f("ÉTÉ"), Some((2, 1)), "a non-ASCII needle, any case");
        assert_eq!(f("kelvin"), Some((3, 1)), "the Kelvin sign lowercases to k");
        assert_eq!(f("é"), Some((2, 1)));
        assert_eq!(f("ü"), None, "a non-ASCII needle is in no ASCII cell");
    }

    #[test]
    fn find_stops_at_its_cell_budget_and_goes_on_from_there() {
        // 30 rows of two columns; only row 22 holds "needle", in column 1.
        let mut text = String::from("k,v\n");
        for i in 0..30 {
            let v = if i == 22 { "a needle" } else { "hay" };
            text.push_str(&format!("{i},{v}\n"));
        }
        let (l, r) = (csv(&text), csv(&text));
        let d = diff_by_order(&l, &r);
        let v = filtered(&d, Filter::All);
        // 20 cells: ten columns of both sides, so five rows a call.
        let run = |needle: &str| {
            let mut resume = FindResume::default();
            let mut pauses = 0;
            loop {
                let search = Search {
                    needle,
                    backward: false,
                    from: None,
                    resume,
                    cells: 20,
                };
                match find(&d, Some(&l), Some(&r), true, &v, &search) {
                    Found::Paused(at) => {
                        assert!(pauses < 100, "a paused find makes progress");
                        pauses += 1;
                        resume = at;
                    }
                    found => return (found, pauses),
                }
            }
        };
        assert_eq!(run("needle"), (Found::At(22, 1), 4));
        assert_eq!(
            run("zz"),
            (Found::Nothing, 5),
            "a miss visits each of the 60 columns once, ten a call"
        );
        // A budget of one cell still moves one column a call.
        let search = Search {
            needle: "zz",
            backward: false,
            from: None,
            resume: FindResume::default(),
            cells: 1,
        };
        assert!(matches!(
            find(&d, Some(&l), Some(&r), true, &v, &search),
            Found::Paused(at) if at != FindResume::default()
        ));
    }

    #[test]
    fn go_to_line_prefers_the_left_side() {
        let (l, r) = (csv(L), csv(R));
        let d = diff(&l, &r);
        let go =
            |l: &Table, r: &Table, d: &TableDiff, n| row_for_line(d, Some(l), Some(r), true, n);
        assert_eq!(go(&l, &r, &d, 3), Some(2), "left line 3 is `2,b`");
        assert_eq!(go(&l, &r, &d, 99), None);
        assert_eq!(go(&l, &r, &d, 1), None, "the header is no row");
        let r2 = csv("id,v\n3,c\n1,A\n4,d\n3,y\n5,e\n");
        let d2 = diff(&l, &r2);
        let row = go(&l, &r2, &d2, 6).unwrap();
        assert_eq!(d2.rows[row].state, RowState::RightOnly);
        assert_eq!(d2.rows[row].right, Some(4));
    }

    #[test]
    fn a_line_inside_the_left_header_falls_through_to_the_right() {
        let l = csv("\"id
x\",v
1,a
");
        let r = csv("id,v
9,z
1,a
");
        let d = diff_by_order(&l, &r);
        let row = row_for_line(&d, Some(&l), Some(&r), true, 2).unwrap();
        assert_eq!(d.rows[row].right, Some(0), "line 2 is right data row 0");
    }

    fn diff_by_order(l: &Table, r: &Table) -> TableDiff {
        let opts = CompareOptions {
            header: true,
            ..Default::default()
        };
        compare(Some(l), Some(r), &opts, &CancelToken::new()).unwrap()
    }

    #[test]
    fn side_values_distinguish_missing_row_column_and_cell() {
        let (l, r) = (csv("a,b\n1\n"), csv("a\n1\n"));
        let d = compare(
            Some(&l),
            Some(&r),
            &CompareOptions {
                header: true,
                ..Default::default()
            },
            &CancelToken::new(),
        )
        .unwrap();
        let v = |t, c, left| side_value(&d, t, true, 0, c, left);
        assert_eq!(v(Some(&l), 0, true), Some("1"));
        assert_eq!(v(Some(&l), 1, true), Some(""), "past a short record");
        assert_eq!(v(Some(&r), 1, false), None, "no such column on the right");
        assert_eq!(v(None, 0, true), None);
    }
}
