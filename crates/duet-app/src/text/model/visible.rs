//! The display rows of a text view under a filter: ranges of aligned rows with fold rows between.

use super::*;

/// What one display row shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayRow {
    /// An aligned row of the RowMap.
    Row(u32),
    /// Stands in for the hidden rows `start..end` (Context filter); a click shows them.
    Fold { start: u32, end: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Item {
    Rows(Range<u32>),
    Fold(Range<u32>),
}

impl Item {
    fn rows(&self) -> &Range<u32> {
        match self {
            Item::Rows(range) | Item::Fold(range) => range,
        }
    }

    fn display_len(&self) -> usize {
        match self {
            Item::Rows(range) => (range.end - range.start) as usize,
            Item::Fold(_) => 1,
        }
    }
}

/// Display rows: sorted, non-overlapping RowMap row ranges, with one fold row per hidden gap
/// under the Context filter. O(blocks) memory however many rows there are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Visible {
    items: Vec<Item>,
    starts: Vec<usize>,
    len: usize,
}

impl Visible {
    pub(super) fn new(items: Vec<Item>) -> Self {
        let mut starts = Vec::with_capacity(items.len());
        let mut len = 0usize;
        for item in &items {
            starts.push(len);
            len += item.display_len();
        }
        Self { items, starts, len }
    }

    pub(super) fn from_ranges(ranges: Vec<Range<u32>>) -> Self {
        Self::new(ranges.into_iter().map(Item::Rows).collect())
    }

    /// `ranges` with a fold row in every gap of `0..total`. A gap of one row shows that row:
    /// a fold row would take the same space and hide it.
    pub(super) fn folded(ranges: Vec<Range<u32>>, total: u32) -> Self {
        let gap = |range: Range<u32>| {
            if range.end - range.start == 1 {
                Item::Rows(range)
            } else {
                Item::Fold(range)
            }
        };
        let mut items = Vec::with_capacity(ranges.len() * 2 + 1);
        let mut at = 0;
        for range in ranges {
            if at < range.start {
                items.push(gap(at..range.start));
            }
            at = range.end;
            items.push(Item::Rows(range));
        }
        if at < total {
            items.push(gap(at..total));
        }
        Self::new(items)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn range_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| matches!(i, Item::Rows(_)))
            .count()
    }

    pub fn fold_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| matches!(i, Item::Fold(_)))
            .count()
    }

    /// What display row `display` shows. Nothing shown has no entry: callers check `is_empty`.
    pub fn entry(&self, display: usize) -> DisplayRow {
        let i = self.starts.partition_point(|&start| start <= display) - 1;
        match &self.items[i] {
            Item::Rows(range) => DisplayRow::Row(range.start + (display - self.starts[i]) as u32),
            Item::Fold(range) => DisplayRow::Fold {
                start: range.start,
                end: range.end,
            },
        }
    }

    /// The aligned row at `display`; None on a fold row.
    pub fn row(&self, display: usize) -> Option<u32> {
        match self.entry(display) {
            DisplayRow::Row(row) => Some(row),
            DisplayRow::Fold { .. } => None,
        }
    }

    pub(super) fn item_at(&self, row: u32) -> Option<usize> {
        let i = self.items.partition_point(|item| item.rows().end <= row);
        (i < self.items.len() && self.items[i].rows().start <= row).then_some(i)
    }

    /// The display row that shows `row`; None when it is filtered out or folded.
    pub fn display_of(&self, row: u32) -> Option<usize> {
        let i = self.item_at(row)?;
        match &self.items[i] {
            Item::Rows(range) => Some(self.starts[i] + (row - range.start) as usize),
            Item::Fold(_) => None,
        }
    }

    /// The hidden rows of the fold that covers `row`.
    pub fn fold_of(&self, row: u32) -> Option<Range<u32>> {
        match &self.items[self.item_at(row)?] {
            Item::Fold(range) => Some(range.clone()),
            Item::Rows(_) => None,
        }
    }
}

/// Sorts and joins overlapping or touching ranges, dropping empty ones.
pub(super) fn merge(mut ranges: Vec<Range<u32>>) -> Vec<Range<u32>> {
    ranges.sort_unstable_by_key(|r| r.start);
    let mut merged: Vec<Range<u32>> = Vec::with_capacity(ranges.len());
    for range in ranges.into_iter().filter(|r| r.start < r.end) {
        match merged.last_mut() {
            Some(last) if last.end >= range.start => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

pub(super) fn context_ranges(blocks: &[Range<u32>], total: u32, n: u32) -> Vec<Range<u32>> {
    blocks
        .iter()
        .map(|b| b.start.saturating_sub(n)..b.end.saturating_add(n).min(total))
        .collect()
}

pub fn visible_rows(blocks: &[Range<u32>], total: u32, filter: TextFilter) -> Visible {
    let ranges = match filter {
        TextFilter::All if total > 0 => std::iter::once(0..total).collect(),
        TextFilter::All => Vec::new(),
        TextFilter::Differences => blocks.to_vec(),
        TextFilter::Context(n) => merge(context_ranges(blocks, total, n)),
        TextFilter::Same => {
            let mut gaps = Vec::with_capacity(blocks.len() + 1);
            let mut at = 0;
            for block in blocks {
                if at < block.start {
                    gaps.push(at..block.start);
                }
                at = block.end;
            }
            if at < total {
                gaps.push(at..total);
            }
            gaps
        }
    };
    Visible::from_ranges(ranges)
}

/// The Context filter's rows plus the ones the user unfolded, with fold rows in the gaps.
pub fn context_rows(blocks: &[Range<u32>], total: u32, n: u32, extra: &[Range<u32>]) -> Visible {
    let mut ranges = context_ranges(blocks, total, n);
    ranges.extend(extra.iter().cloned());
    Visible::folded(merge(ranges), total)
}
