//! Position-based byte comparison of a [`HexPair`]: ranges, blocks and "differences only" rows.

use crate::cancel::CancelToken;
use crate::hex::HexPair;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

/// Ranges kept before the compare stops and reports `truncated`.
pub const MAX_RANGES: usize = 200_000;
/// Ranges closer than this many bytes share one navigation block.
pub const MERGE_GAP: u64 = 8;
const CHUNK: u64 = 1 << 20;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HexDiff {
    pub ranges: Vec<Range<u64>>,
    pub diff_bytes: u64,
    pub truncated: bool,
    pub cancelled: bool,
}

/// Compares by position in 1 MiB chunks (equal chunks skipped with one slice compare), adds the
/// tail range for the longer file, stops at `MAX_RANGES` (truncated) or on cancel. `done` counts
/// compared bytes for progress.
pub fn diff_ranges(pair: &HexPair, cancel: &CancelToken, done: &AtomicU64) -> HexDiff {
    let common = pair.common_len();
    let max = pair.max_len();
    let mut out = HexDiff::default();
    let mut open: Option<u64> = None;
    let mut pos = 0u64;
    while pos < common {
        if cancel.is_cancelled() {
            out.cancelled = true;
            return out;
        }
        let want = pos..(pos + CHUNK).min(common);
        let a = pair.left.bytes(want.clone());
        let b = pair.right.bytes(want);
        let n = a.len().min(b.len());
        if n == 0 {
            break;
        }
        let (a, b) = (&a[..n], &b[..n]);
        if a != b {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                let at = pos + i as u64;
                match (x != y, open) {
                    (true, None) => open = Some(at),
                    (false, Some(start)) => {
                        open = None;
                        if !push(&mut out, start..at) {
                            return out;
                        }
                    }
                    _ => {}
                }
            }
        } else if let Some(start) = open.take()
            && !push(&mut out, start..pos)
        {
            return out;
        }
        pos += n as u64;
        done.fetch_add(n as u64, Ordering::Relaxed);
    }
    // A run still open at the end of the common length joins the tail range of the longer file.
    let tail_start = open.take().unwrap_or(common);
    if tail_start < max {
        push(&mut out, tail_start..max);
    }
    out
}

/// Keeps `range` unless the cap is reached; returns false once the compare must stop.
fn push(out: &mut HexDiff, range: Range<u64>) -> bool {
    if out.ranges.len() >= MAX_RANGES {
        out.truncated = true;
        return false;
    }
    out.diff_bytes += range.end - range.start;
    out.ranges.push(range);
    true
}

/// Merges ranges whose gap is under `MERGE_GAP` bytes. `ranges` must be sorted.
pub fn blocks(ranges: &[Range<u64>]) -> Vec<Range<u64>> {
    let mut out: Vec<Range<u64>> = Vec::new();
    for r in ranges {
        match out.last_mut() {
            Some(last) if r.start.saturating_sub(last.end) < MERGE_GAP => {
                last.end = last.end.max(r.end);
            }
            _ => out.push(r.clone()),
        }
    }
    out
}

/// The index of the range holding `offset`. `ranges` must be sorted and disjoint.
pub fn range_index_at(ranges: &[Range<u64>], offset: u64) -> Option<usize> {
    let after = ranges.partition_point(|r| r.end <= offset);
    ranges
        .get(after)
        .filter(|r| r.start <= offset)
        .map(|_| after)
}

/// Rows touching any range, merged; display index <-> row for "differences only".
pub struct DiffRows {
    /// Disjoint, ascending runs of file rows.
    segments: Vec<Range<u64>>,
    /// `prefix[i]` is the display index of the first row of `segments[i]`.
    prefix: Vec<u64>,
    total: u64,
}

impl DiffRows {
    pub fn new(ranges: &[Range<u64>], bytes_per_row: u64) -> DiffRows {
        let bpr = bytes_per_row.max(1);
        let mut segments: Vec<Range<u64>> = Vec::new();
        for r in ranges.iter().filter(|r| r.start < r.end) {
            let rows = r.start / bpr..(r.end - 1) / bpr + 1;
            match segments.last_mut() {
                Some(last) if rows.start <= last.end => last.end = last.end.max(rows.end),
                _ => segments.push(rows),
            }
        }
        let mut prefix = Vec::with_capacity(segments.len());
        let mut total = 0;
        for s in &segments {
            prefix.push(total);
            total += s.end - s.start;
        }
        DiffRows {
            segments,
            prefix,
            total,
        }
    }

    pub fn len(&self) -> u64 {
        self.total
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// The file row shown at display index `display`.
    pub fn row(&self, display: u64) -> Option<u64> {
        if display >= self.total {
            return None;
        }
        let i = self.prefix.partition_point(|&p| p <= display) - 1;
        Some(self.segments[i].start + (display - self.prefix[i]))
    }

    /// The display index of `row`, or of the next shown row; `None` past the last one.
    pub fn display_of(&self, row: u64) -> Option<u64> {
        let i = self.segments.partition_point(|s| s.end <= row);
        let seg = self.segments.get(i)?;
        Some(self.prefix[i] + row.saturating_sub(seg.start))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancelToken;
    use crate::hex::{HexPair, HexSource};
    use std::sync::atomic::AtomicU64;

    fn pair_of(a: &[u8], b: &[u8]) -> (tempfile::TempDir, HexPair) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), a).unwrap();
        std::fs::write(dir.path().join("b"), b).unwrap();
        let pair = HexPair::open(&dir.path().join("a"), &dir.path().join("b")).unwrap();
        (dir, pair)
    }

    #[test]
    fn ranges_are_runs_of_differing_bytes_plus_the_tail() {
        let (_d, pair) = pair_of(b"abcdefghij", b"abXdeYYhijKLM");
        let diff = diff_ranges(&pair, &CancelToken::new(), &AtomicU64::new(0));
        assert_eq!(diff.ranges, vec![2..3, 5..7, 10..13]);
        assert_eq!(diff.diff_bytes, 1 + 2 + 3);
        assert!(!diff.truncated && !diff.cancelled);
    }

    #[test]
    fn an_empty_side_is_all_hatch_and_the_tail_is_one_range() {
        let (_d, pair) = pair_of(b"", b"xyz");
        assert_eq!(pair.left.bytes(0..3), b"");
        let diff = diff_ranges(&pair, &CancelToken::new(), &AtomicU64::new(0));
        assert_eq!(diff.ranges, vec![0..3]);
    }

    #[test]
    fn blocks_merge_ranges_closer_than_eight_bytes() {
        assert_eq!(blocks(&[0..1, 5..6, 20..21, 28..30]), vec![0..6, 20..30]);
        assert_eq!(blocks(&[0..1, 9..10]), vec![0..1, 9..10]);
    }

    #[test]
    fn ranges_stop_at_the_cap_and_say_so() {
        let a = vec![0u8; 600_000];
        let b: Vec<u8> = (0..600_000)
            .map(|i| if i % 2 == 0 { 1 } else { 0 })
            .collect();
        let (_d, pair) = pair_of(&a, &b);
        let diff = diff_ranges(&pair, &CancelToken::new(), &AtomicU64::new(0));
        assert_eq!(diff.ranges.len(), MAX_RANGES);
        assert!(diff.truncated);
    }

    #[test]
    fn cancel_stops_the_compare() {
        let (_d, pair) = pair_of(&vec![0u8; 4 << 20], &vec![1u8; 4 << 20]);
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(diff_ranges(&pair, &cancel, &AtomicU64::new(0)).cancelled);
    }

    #[test]
    fn difference_rows_map_both_ways() {
        let rows = DiffRows::new(&[3..4, 40..70], 16); // rows 0, 2, 3, 4
        assert_eq!(rows.len(), 4);
        assert_eq!(
            (0..4).map(|d| rows.row(d).unwrap()).collect::<Vec<_>>(),
            [0, 2, 3, 4]
        );
        assert_eq!(rows.display_of(3), Some(2));
        assert_eq!(
            rows.display_of(1),
            Some(1),
            "a hidden row maps to the next shown one"
        );
    }

    #[test]
    fn a_synthetic_four_gib_source_serves_its_last_row() {
        let s = HexSource::synthetic(4 << 30, 0xAB);
        assert_eq!(s.bytes((4u64 << 30) - 16..(4u64 << 30) + 16).len(), 16);
    }

    #[test]
    fn bytes_is_safe_for_odd_ranges() {
        let s = HexSource::synthetic(10, 7);
        assert!(s.bytes(5..5).is_empty());
        #[allow(clippy::reversed_empty_ranges)]
        let inverted = 8..2;
        assert!(s.bytes(inverted).is_empty());
        assert!(s.bytes(10..20).is_empty());
        assert!(s.bytes(u64::MAX - 1..u64::MAX).is_empty());
        assert_eq!(s.bytes(0..u64::MAX).len(), 10);
        assert_eq!(s.bytes(3..6), &[7, 7, 7]);
    }

    #[test]
    fn range_index_finds_the_holder() {
        let r = [2..3, 5..7, 10..13];
        assert_eq!(range_index_at(&r, 2), Some(0));
        assert_eq!(range_index_at(&r, 6), Some(1));
        assert_eq!(range_index_at(&r, 7), None);
        assert_eq!(range_index_at(&r, 12), Some(2));
        assert_eq!(range_index_at(&r, 13), None);
        assert_eq!(range_index_at(&[], 0), None);
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn display_of_past_the_end_and_empty_rows() {
        let rows = DiffRows::new(&[3..4], 16);
        assert_eq!(rows.display_of(5), None);
        assert_eq!(rows.row(1), None);
        let none = DiffRows::new(&[], 16);
        assert_eq!(none.len(), 0);
        assert_eq!(none.display_of(0), None);
    }

    #[test]
    fn chunk_boundary_runs_stay_one_range() {
        let n = (1 << 20) + 100;
        let a = vec![0u8; n];
        let mut b = a.clone();
        for x in &mut b[(1 << 20) - 3..(1 << 20) + 3] {
            *x = 9;
        }
        let (_d, pair) = pair_of(&a, &b);
        let done = AtomicU64::new(0);
        let diff = diff_ranges(&pair, &CancelToken::new(), &done);
        assert_eq!(diff.ranges, vec![(1 << 20) - 3..(1 << 20) + 3]);
        assert_eq!(done.load(std::sync::atomic::Ordering::Relaxed), n as u64);
    }
}
