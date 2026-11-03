//! Column matching, key resolution, row alignment and cell comparison.

use std::borrow::Cow;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use imara_diff::{Algorithm, Diff, InternedInput, Interner, Token};

use crate::cancel::{CancelToken, Cancelled};
use crate::table::Table;
use crate::table::canon::CellRules;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Same,
    Differs,
    LeftOnly,
    RightOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub left_name: String,
    pub right_name: String,
    pub left: Option<usize>,
    pub right: Option<usize>,
}

impl Column {
    pub fn only_left(&self) -> bool {
        self.right.is_none()
    }
    pub fn only_right(&self) -> bool {
        self.left.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompareOptions {
    pub header: bool,
    pub keys: String,
    pub rules: CellRules,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairedRow {
    pub left: Option<u32>,
    pub right: Option<u32>,
    pub state: RowState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub same: usize,
    pub differs: usize,
    pub left_only: usize,
    pub right_only: usize,
    pub cells: usize,
}

#[derive(Debug, Clone)]
pub struct TableDiff {
    pub columns: Vec<Column>,
    pub matched: Vec<usize>,
    pub keys: Vec<usize>,
    pub bad_keys: Vec<String>,
    pub rows: Vec<PairedRow>,
    pub counts: Counts,
    pub column_diffs: Vec<u32>,
    pub duplicates: usize,
    pub left_only_columns: usize,
    pub right_only_columns: usize,
    pub both: bool,
    /// Differing column indices of every `Differs` row, back to back.
    diff_cells: Vec<u32>,
    /// `diff_starts[row]..diff_starts[row + 1]` is the row's range in `diff_cells`.
    diff_starts: Vec<u32>,
}

impl TableDiff {
    /// The column indices (into `columns`) where a `Differs` row's cells differ; empty otherwise.
    pub fn diff_columns(&self, row: usize) -> &[u32] {
        &self.diff_cells[self.diff_starts[row] as usize..self.diff_starts[row + 1] as usize]
    }

    /// No row is other than `Same` and no column is one-sided.
    pub fn is_identical(&self) -> bool {
        self.counts.differs == 0
            && self.counts.left_only == 0
            && self.counts.right_only == 0
            && self.left_only_columns == 0
            && self.right_only_columns == 0
    }
}

/// 1 when `header` and the table has a record, else 0: data row i is record i + offset.
pub fn data_offset(table: &Table, header: bool) -> usize {
    usize::from(header && !table.is_empty())
}

/// How often the long loops look at the cancel token.
const CANCEL_EVERY: usize = 4096;

/// One side as the comparison sees it: records from `offset` on are data rows.
#[derive(Clone, Copy)]
struct Side<'a> {
    table: Option<&'a Table>,
    offset: usize,
    rows: usize,
    width: usize,
}

impl<'a> Side<'a> {
    fn new(table: Option<&'a Table>, header: bool) -> Side<'a> {
        match table {
            None => Side {
                table: None,
                offset: 0,
                rows: 0,
                width: 0,
            },
            Some(t) => {
                let offset = data_offset(t, header);
                Side {
                    table,
                    offset,
                    rows: t.len() - offset,
                    width: t.width(),
                }
            }
        }
    }

    /// The trimmed header cell, or `#N` when it is missing or blank.
    fn name(&self, column: usize) -> String {
        let cell = self
            .table
            .filter(|_| self.offset == 1)
            .and_then(|t| t.record(0).get(column))
            .map(str::trim)
            .unwrap_or("");
        if cell.is_empty() {
            format!("#{}", column + 1)
        } else {
            cell.to_string()
        }
    }

    /// Cell of data row `row`; "" past the record's end.
    fn cell(&self, row: usize, column: usize) -> &'a str {
        self.table
            .and_then(|t| t.record(row + self.offset).get(column))
            .unwrap_or("")
    }
}

fn build_columns(a: &Side, b: &Side, header: bool) -> Vec<Column> {
    if !header {
        return (0..a.width.max(b.width))
            .map(|i| {
                let name = format!("#{}", i + 1);
                Column {
                    left_name: if i < a.width {
                        name.clone()
                    } else {
                        String::new()
                    },
                    right_name: if i < b.width {
                        name.clone()
                    } else {
                        String::new()
                    },
                    name,
                    left: (i < a.width).then_some(i),
                    right: (i < b.width).then_some(i),
                }
            })
            .collect();
    }
    // Right columns by lowercased name, each a queue of indices still unused.
    let mut by_name: HashMap<String, (Vec<usize>, usize)> = HashMap::new();
    for j in 0..b.width {
        by_name
            .entry(b.name(j).to_lowercase())
            .or_default()
            .0
            .push(j);
    }
    let mut used = vec![false; b.width];
    let mut cols: Vec<Column> = Vec::with_capacity(a.width.max(b.width));
    for i in 0..a.width {
        let name = a.name(i);
        let right = by_name
            .get_mut(&name.to_lowercase())
            .and_then(|(list, next)| {
                let j = list.get(*next).copied();
                *next += 1;
                j
            });
        if let Some(j) = right {
            used[j] = true;
        }
        cols.push(Column {
            left_name: name.clone(),
            right_name: right.map(|j| b.name(j)).unwrap_or_default(),
            name,
            left: Some(i),
            right,
        });
    }
    // Right-only columns go right after the column holding the previous right column.
    let mut position_of_right = vec![usize::MAX; b.width];
    for (p, col) in cols.iter().enumerate() {
        if let Some(j) = col.right {
            position_of_right[j] = p;
        }
    }
    let mut head: Vec<Column> = Vec::new();
    let mut after: Vec<Vec<Column>> = vec![Vec::new(); cols.len()];
    let mut last: Option<usize> = None;
    for j in 0..b.width {
        if used[j] {
            last = Some(position_of_right[j]);
            continue;
        }
        let name = b.name(j);
        let col = Column {
            left_name: String::new(),
            right_name: name.clone(),
            name,
            left: None,
            right: Some(j),
        };
        match last {
            None => head.push(col),
            Some(p) => after[p].push(col),
        }
    }
    let mut out = head;
    for (col, extra) in cols.into_iter().zip(after) {
        out.push(col);
        out.extend(extra);
    }
    out
}

/// "id, 3, #5" to indices into `cols` (a key column must exist on both sides).
fn resolve_keys(spec: &str, cols: &[Column], header: bool) -> (Vec<usize>, Vec<String>) {
    let mut keys: Vec<usize> = Vec::new();
    let mut bad: Vec<String> = Vec::new();
    for token in spec.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        let mut found = None;
        if header {
            let lower = token.to_lowercase();
            found = cols.iter().position(|c| {
                c.left.is_some()
                    && c.right.is_some()
                    && (c.left_name.to_lowercase() == lower || c.right_name.to_lowercase() == lower)
            });
        }
        if found.is_none() {
            let digits = token.strip_prefix('#').unwrap_or(token);
            if !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()) {
                // Numbers start at 1; an overflowing or zero number matches nothing.
                if let Some(n) = digits.parse::<usize>().ok().and_then(|n| n.checked_sub(1)) {
                    let both = |c: &Column| c.left.is_some() && c.right.is_some();
                    found = cols
                        .iter()
                        .position(|c| c.left == Some(n) && both(c))
                        .or_else(|| cols.iter().position(|c| c.right == Some(n) && both(c)));
                }
            }
        }
        match found {
            None => bad.push(token.to_string()),
            Some(f) if !keys.contains(&f) => keys.push(f),
            Some(_) => {}
        }
    }
    (keys, bad)
}

/// The alignment key of every data row: the chosen cells, normalised, joined by U+001F.
fn row_keys<'a>(
    side: &Side<'a>,
    columns: &[usize],
    rules: &CellRules,
    cancel: &CancelToken,
) -> Result<Vec<Cow<'a, str>>, Cancelled> {
    let mut keys = Vec::with_capacity(side.rows);
    for row in 0..side.rows {
        if row % CANCEL_EVERY == 0 {
            cancel.check()?;
        }
        let key = match columns {
            [only] => rules.normalize(side.cell(row, *only)),
            _ => {
                let mut joined = String::new();
                for (n, &c) in columns.iter().enumerate() {
                    if n > 0 {
                        joined.push('\u{1f}');
                    }
                    joined.push_str(&rules.normalize(side.cell(row, c)));
                }
                Cow::Owned(joined)
            }
        };
        keys.push(key);
    }
    Ok(keys)
}

/// Aligned data-row pairs, `None` marking the missing side.
type Pairs = Vec<(Option<u32>, Option<u32>)>;

fn align_by_order(
    a: &Side,
    b: &Side,
    left_cols: &[usize],
    right_cols: &[usize],
    rules: &CellRules,
    cancel: &CancelToken,
) -> Result<Pairs, Cancelled> {
    let mut pairs: Pairs = Vec::with_capacity(a.rows.max(b.rows));
    if a.rows == 0 || b.rows == 0 {
        pairs.extend((0..a.rows).map(|i| (Some(i as u32), None)));
        pairs.extend((0..b.rows).map(|j| (None, Some(j as u32))));
        return Ok(pairs);
    }
    let ka = row_keys(a, left_cols, rules, cancel)?;
    let kb = row_keys(b, right_cols, rules, cancel)?;
    cancel.check()?;
    let mut interner = Interner::new(ka.len() + kb.len());
    let before: Vec<Token> = ka.iter().map(|k| interner.intern(&**k)).collect();
    let after: Vec<Token> = kb.iter().map(|k| interner.intern(&**k)).collect();
    let input = InternedInput {
        before,
        after,
        interner,
    };
    let diff = Diff::compute(Algorithm::Histogram, &input);
    cancel.check()?;
    // Equal rows pair up; inside each changed hunk the first min(dels, inss) rows pair in order.
    let (mut i, mut j) = (0u32, 0u32);
    for hunk in diff.hunks() {
        while i < hunk.before.start {
            pairs.push((Some(i), Some(j)));
            i += 1;
            j += 1;
        }
        let dels = hunk.before.end - hunk.before.start;
        let inss = hunk.after.end - hunk.after.start;
        let both = dels.min(inss);
        for q in 0..both {
            pairs.push((Some(hunk.before.start + q), Some(hunk.after.start + q)));
        }
        for q in both..dels {
            pairs.push((Some(hunk.before.start + q), None));
        }
        for q in both..inss {
            pairs.push((None, Some(hunk.after.start + q)));
        }
        i = hunk.before.end;
        j = hunk.after.end;
    }
    while (i as usize) < a.rows {
        pairs.push((Some(i), Some(j)));
        i += 1;
        j += 1;
    }
    Ok(pairs)
}

enum Slot {
    One(usize),
    Many(Vec<usize>, usize),
    Spent,
}

/// Pairs rows by their key tuple (duplicates in order of appearance), keeping left order.
/// Each right-only row goes right after the left row paired with the previous right row.
fn align_by_key(
    a: &Side,
    b: &Side,
    left_cols: &[usize],
    right_cols: &[usize],
    rules: &CellRules,
    cancel: &CancelToken,
) -> Result<(Pairs, usize), Cancelled> {
    let ka = row_keys(a, left_cols, rules, cancel)?;
    let kb = row_keys(b, right_cols, rules, cancel)?;
    let mut duplicates = 0;
    let mut slots: HashMap<&str, Slot> = HashMap::with_capacity(kb.len());
    for (j, key) in kb.iter().enumerate() {
        if j % CANCEL_EVERY == 0 {
            cancel.check()?;
        }
        match slots.entry(&**key) {
            Entry::Vacant(v) => {
                v.insert(Slot::One(j));
            }
            Entry::Occupied(mut o) => {
                duplicates += 1;
                match o.get_mut() {
                    Slot::Many(list, _) => list.push(j),
                    slot => {
                        let Slot::One(first) = *slot else {
                            unreachable!("slots start as One")
                        };
                        *slot = Slot::Many(vec![first, j], 0);
                    }
                }
            }
        }
    }
    let mut left_partner: Vec<Option<usize>> = vec![None; a.rows];
    let mut right_partner: Vec<Option<usize>> = vec![None; b.rows];
    let mut seen: HashSet<&str> = HashSet::with_capacity(ka.len());
    for (i, key) in ka.iter().enumerate() {
        if i % CANCEL_EVERY == 0 {
            cancel.check()?;
        }
        if !seen.insert(&**key) {
            duplicates += 1;
        }
        let Some(slot) = slots.get_mut(&**key) else {
            continue;
        };
        let partner = match slot {
            Slot::One(j) => {
                let j = *j;
                *slot = Slot::Spent;
                Some(j)
            }
            Slot::Many(list, next) => {
                let j = list.get(*next).copied();
                *next += 1;
                j
            }
            Slot::Spent => None,
        };
        if let Some(j) = partner {
            left_partner[i] = Some(j);
            right_partner[j] = Some(i);
        }
    }
    let mut head: Vec<usize> = Vec::new();
    let mut after: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut last: Option<usize> = None;
    for (j, partner) in right_partner.iter().enumerate() {
        match (partner, last) {
            (Some(i), _) => last = Some(*i),
            (None, None) => head.push(j),
            (None, Some(i)) => after.entry(i).or_default().push(j),
        }
    }
    let mut pairs: Pairs = Vec::with_capacity(a.rows.max(b.rows));
    pairs.extend(head.iter().map(|&j| (None, Some(j as u32))));
    for (i, partner) in left_partner.iter().enumerate() {
        pairs.push((Some(i as u32), partner.map(|j| j as u32)));
        if let Some(extra) = after.get(&i) {
            pairs.extend(extra.iter().map(|&j| (None, Some(j as u32))));
        }
    }
    Ok((pairs, duplicates))
}

pub fn compare(
    left: Option<&Table>,
    right: Option<&Table>,
    opts: &CompareOptions,
    cancel: &CancelToken,
) -> Result<TableDiff, Cancelled> {
    cancel.check()?;
    let a = Side::new(left, opts.header);
    let b = Side::new(right, opts.header);
    let columns = build_columns(&a, &b, opts.header);
    let matched: Vec<usize> = (0..columns.len())
        .filter(|&c| columns[c].left.is_some() && columns[c].right.is_some())
        .collect();
    let left_only_columns = columns.iter().filter(|c| c.only_left()).count();
    let right_only_columns = columns.iter().filter(|c| c.only_right()).count();
    let (keys, bad_keys) = resolve_keys(&opts.keys, &columns, opts.header);

    let left_of =
        |cols: &[usize]| -> Vec<usize> { cols.iter().filter_map(|&c| columns[c].left).collect() };
    let right_of =
        |cols: &[usize]| -> Vec<usize> { cols.iter().filter_map(|&c| columns[c].right).collect() };
    let rules = &opts.rules;
    let (pairs, duplicates) = if keys.is_empty() {
        let pairs = align_by_order(
            &a,
            &b,
            &left_of(&matched),
            &right_of(&matched),
            rules,
            cancel,
        )?;
        (pairs, 0)
    } else {
        align_by_key(&a, &b, &left_of(&keys), &right_of(&keys), rules, cancel)?
    };

    let matched_left = left_of(&matched);
    let matched_right = right_of(&matched);
    let mut rows = Vec::with_capacity(pairs.len());
    let mut counts = Counts::default();
    let mut column_diffs = vec![0u32; columns.len()];
    let mut diff_cells: Vec<u32> = Vec::new();
    let mut diff_starts: Vec<u32> = Vec::with_capacity(pairs.len() + 1);
    for (n, &(l, r)) in pairs.iter().enumerate() {
        if n % CANCEL_EVERY == 0 {
            cancel.check()?;
        }
        diff_starts.push(diff_cells.len() as u32);
        let state = match (l, r) {
            (None, _) => RowState::RightOnly,
            (_, None) => RowState::LeftOnly,
            (Some(li), Some(ri)) => {
                let before = diff_cells.len();
                for (q, &col) in matched.iter().enumerate() {
                    let x = a.cell(li as usize, matched_left[q]);
                    let y = b.cell(ri as usize, matched_right[q]);
                    if !rules.equal(x, y) {
                        diff_cells.push(col as u32);
                        column_diffs[col] += 1;
                    }
                }
                counts.cells += diff_cells.len() - before;
                if diff_cells.len() > before {
                    RowState::Differs
                } else {
                    RowState::Same
                }
            }
        };
        match state {
            RowState::Same => counts.same += 1,
            RowState::Differs => counts.differs += 1,
            RowState::LeftOnly => counts.left_only += 1,
            RowState::RightOnly => counts.right_only += 1,
        }
        rows.push(PairedRow {
            left: l,
            right: r,
            state,
        });
    }
    diff_starts.push(diff_cells.len() as u32);

    Ok(TableDiff {
        columns,
        matched,
        keys,
        bad_keys,
        rows,
        counts,
        column_diffs,
        duplicates,
        left_only_columns,
        right_only_columns,
        both: left.is_some() && right.is_some(),
        diff_cells,
        diff_starts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Delimiter;
    use crate::table::parse::parse_str;

    fn csv(text: &str) -> Table {
        parse_str(text, Delimiter::Comma)
    }

    fn run(l: &str, r: &str, opts: &CompareOptions) -> TableDiff {
        compare(Some(&csv(l)), Some(&csv(r)), opts, &CancelToken::new()).unwrap()
    }

    fn header() -> CompareOptions {
        CompareOptions {
            header: true,
            ..Default::default()
        }
    }

    fn states(d: &TableDiff) -> Vec<RowState> {
        d.rows.iter().map(|p| p.state).collect()
    }

    #[test]
    fn columns_match_by_name_in_any_order_and_one_sided_columns_sit_after_their_neighbour() {
        let l = csv("id,name,price\n1,a,5\n");
        let r = csv("name,ID,qty,price\na,1,9,5\n");
        let d = compare(Some(&l), Some(&r), &header(), &CancelToken::new()).unwrap();
        let names: Vec<&str> = d.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["id", "qty", "name", "price"]);
        assert_eq!(
            (
                d.columns[0].left,
                d.columns[0].right,
                d.columns[0].right_name.as_str()
            ),
            (Some(0), Some(1), "ID")
        );
        assert!(d.columns[1].only_right());
        assert_eq!(d.columns[1].left_name, "");
        assert_eq!(d.matched, [0, 2, 3]);
        assert_eq!((d.left_only_columns, d.right_only_columns), (0, 1));
        assert_eq!(
            d.rows,
            [PairedRow {
                left: Some(0),
                right: Some(0),
                state: RowState::Same
            }]
        );
        assert!(!d.is_identical(), "a one-sided column is not identical");
    }

    #[test]
    fn right_only_columns_before_any_match_come_first() {
        let d = run("a,b\n1,2\n", "z,a,b\n0,1,2\n", &header());
        let names: Vec<&str> = d.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["z", "a", "b"]);
    }

    #[test]
    fn duplicate_header_names_pair_in_order() {
        let d = run("a,a,b\n1,2,3\n", "b,a,a\n3,1,2\n", &header());
        let cols: Vec<(&str, Option<usize>, Option<usize>)> = d
            .columns
            .iter()
            .map(|c| (c.name.as_str(), c.left, c.right))
            .collect();
        assert_eq!(
            cols,
            [
                ("a", Some(0), Some(1)),
                ("a", Some(1), Some(2)),
                ("b", Some(2), Some(0))
            ]
        );
        assert_eq!(states(&d), [RowState::Same]);
    }

    #[test]
    fn blank_header_cells_are_numbered_and_names_are_trimmed() {
        let d = run(" a ,\n1,2\n", "a,\n1,2\n", &header());
        let names: Vec<&str> = d.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["a", "#2"]);
        assert!(d.is_identical());
    }

    #[test]
    fn without_a_header_columns_pair_by_position() {
        let d = run("1,2,3\n", "1,2\n", &CompareOptions::default());
        let names: Vec<&str> = d.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["#1", "#2", "#3"]);
        assert!(d.columns[2].only_left());
        assert_eq!(d.columns[2].left_name, "#3");
        assert_eq!(d.columns[2].right_name, "");
        assert_eq!(d.columns[0].right_name, "#1");
        assert_eq!((d.left_only_columns, d.right_only_columns), (1, 0));
        assert_eq!(states(&d), [RowState::Same], "#3 is never compared");
        assert_eq!(d.rows[0].left, Some(0), "no header: row 0 is record 0");
    }

    #[test]
    fn keyless_rows_align_by_order_and_runs_pair_up() {
        let l = "1,a\n2,b\n3,c\n4,d\n";
        let r = "1,a\n2,B\n9,z\n3,c\n4,d\n";
        use RowState::*;
        let d = run(l, r, &CompareOptions::default());
        assert_eq!(states(&d), [Same, Differs, RightOnly, Same, Same]);
        assert_eq!(d.rows[1].left, Some(1));
        assert_eq!(d.rows[1].right, Some(1));
        assert_eq!(d.rows[2].left, None);
        assert_eq!(d.rows[2].right, Some(2));
        assert_eq!(
            d.counts,
            Counts {
                same: 3,
                differs: 1,
                left_only: 0,
                right_only: 1,
                cells: 1
            }
        );
        let opts = CompareOptions {
            rules: CellRules {
                ignore_case: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let d = run(l, r, &opts);
        assert_eq!(states(&d), [Same, Same, RightOnly, Same, Same]);
    }

    #[test]
    fn key_columns_pair_rows_out_of_order_and_count_duplicates() {
        let l = csv("id,v\n1,a\n2,b\n3,c\n3,x\n");
        let r = csv("id,v\n3,c\n1,A\n4,d\n3,y\n");
        let opts = CompareOptions {
            header: true,
            keys: "id".into(),
            ..Default::default()
        };
        let d = compare(Some(&l), Some(&r), &opts, &CancelToken::new()).unwrap();
        let got: Vec<(Option<u32>, Option<u32>, RowState)> =
            d.rows.iter().map(|p| (p.left, p.right, p.state)).collect();
        use RowState::*;
        assert_eq!(
            got,
            [
                (Some(0), Some(1), Differs),
                (None, Some(2), RightOnly),
                (Some(1), None, LeftOnly),
                (Some(2), Some(0), Same),
                (Some(3), Some(3), Differs)
            ]
        );
        assert_eq!(d.duplicates, 2);
        assert_eq!(
            d.counts,
            Counts {
                same: 1,
                differs: 2,
                left_only: 1,
                right_only: 1,
                cells: 2
            }
        );
        assert_eq!(d.diff_columns(0), [1]);
        assert!(d.diff_columns(1).is_empty());
        assert!(d.diff_columns(3).is_empty());
        assert_eq!(d.diff_columns(4), [1]);
        assert_eq!(d.column_diffs, [0, 2]);
    }

    #[test]
    fn keys_resolve_by_name_then_number_and_report_missing() {
        let l = "id,name,price\n1,a,5\n2,b,6\n";
        let r = "id,name,price\n2,b,6\n1,a,5\n";
        let with = |keys: &str| {
            run(
                l,
                r,
                &CompareOptions {
                    header: true,
                    keys: keys.into(),
                    ..Default::default()
                },
            )
        };
        let d = with("ID, 2, nope");
        assert_eq!(d.keys, [0, 1]);
        assert_eq!(d.bad_keys, ["nope"]);
        assert!(states(&d).iter().all(|s| *s == RowState::Same));
        let d = with("#3");
        assert_eq!(d.keys, [2]);
        assert!(d.bad_keys.is_empty());
        let d = with("9");
        assert!(d.keys.is_empty());
        assert_eq!(d.bad_keys, ["9"]);
        assert_eq!(
            states(&d),
            [RowState::LeftOnly, RowState::Same, RowState::RightOnly],
            "rows aligned by order, not by key"
        );
        let d = with("0");
        assert_eq!(d.bad_keys, ["0"], "column numbers start at 1");
        let d = with("id,ID,#1");
        assert_eq!(d.keys, [0], "a column is listed once");
    }

    #[test]
    fn numeric_key_tokens_match_by_number_without_a_header() {
        let d = run(
            "1,a\n2,b\n",
            "2,b\n1,a\n",
            &CompareOptions {
                keys: "1".into(),
                ..Default::default()
            },
        );
        assert_eq!(d.keys, [0]);
        assert!(states(&d).iter().all(|s| *s == RowState::Same));
    }

    #[test]
    fn ragged_rows_compare_missing_as_empty() {
        let d = run("a,b,c\n1,2,\n", "a,b,c\n1,2\n", &header());
        assert_eq!(states(&d), [RowState::Same]);
        let d = run("a,b,c\n1,2,x\n", "a,b,c\n1,2\n", &header());
        assert_eq!(states(&d), [RowState::Differs]);
        assert_eq!(d.diff_columns(0), [2]);
    }

    #[test]
    fn an_empty_or_header_only_side_makes_rows_one_sided() {
        let r = csv("id,v\n1,a\n2,b\n");
        let d = compare(None, Some(&r), &header(), &CancelToken::new()).unwrap();
        assert!(!d.both);
        assert_eq!(states(&d), [RowState::RightOnly, RowState::RightOnly]);
        assert_eq!(d.counts.right_only, 2);
        assert_eq!(d.rows[0].right, Some(0));

        let l = csv("id,v\n");
        let r = csv("id,v\n1,a\n");
        let d = compare(Some(&l), Some(&r), &header(), &CancelToken::new()).unwrap();
        assert!(d.both);
        assert_eq!(states(&d), [RowState::RightOnly]);
        assert_eq!(d.rows[0].right, Some(0), "data row 0 is record 1");
    }

    #[test]
    fn data_offset_skips_the_header_only_when_there_is_one() {
        assert_eq!(data_offset(&csv("a\n1\n"), true), 1);
        assert_eq!(data_offset(&csv("a\n1\n"), false), 0);
        assert_eq!(data_offset(&csv(""), true), 0);
    }

    #[test]
    fn identical_tables_are_identical_and_cell_rules_apply() {
        let d = run("id,v\n1,a\n", "id,v\n1,a\n", &header());
        assert!(d.is_identical());
        let opts = CompareOptions {
            header: true,
            rules: CellRules {
                numbers: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(run("id,v\n1,1.0\n", "id,v\n1,1\n", &opts).is_identical());
        assert!(!run("id,v\n1,1.0\n", "id,v\n1,1\n", &header()).is_identical());
    }

    #[test]
    fn compare_stops_on_cancel() {
        let t = csv("a\n1\n2\n");
        let cancel = CancelToken::new();
        cancel.cancel();
        assert_eq!(
            compare(Some(&t), Some(&t), &header(), &cancel).err(),
            Some(Cancelled)
        );
    }
}
