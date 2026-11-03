//! Peak memory of `diff_lines` per input line (spec section 8: 1 GB files in < 1 GB private RAM).
//! A single test: the counting allocator is global to this test binary.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};

use duet_core::cancel::CancelToken;
use duet_core::text::diff::{DiffInput, DiffOptions, diff_lines};
use duet_core::text::lines::LineIndex;

struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let now = CURRENT.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
        PEAK.fetch_max(now, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// The 1 GB budget leaves about 24 bytes per line for the diff once line offsets are counted.
const MAX_BYTES_PER_LINE: usize = 24;

#[test]
fn diff_memory_per_line_stays_within_the_budget() {
    let (mut left, mut right) = (String::new(), String::new());
    for i in 0..200_000u64 {
        let line = format!(
            "{i:010} {:016x} lorem ipsum\n",
            i.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        );
        left.push_str(&line);
        if i % 100 == 0 {
            let _ = writeln!(right, "{i:010} CHANGED");
        } else {
            right.push_str(&line);
        }
    }
    let (li, ri) = (
        LineIndex::new(left.as_bytes()),
        LineIndex::new(right.as_bytes()),
    );
    let lines = li.len() + ri.len();
    let base = CURRENT.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let diff = diff_lines(
        DiffInput {
            bytes: left.as_bytes(),
            lines: &li,
        },
        DiffInput {
            bytes: right.as_bytes(),
            lines: &ri,
        },
        DiffOptions::default(),
        &CancelToken::new(),
    )
    .unwrap();
    let per_line = (PEAK.load(Ordering::Relaxed) - base) / lines;
    assert_eq!(diff.hunks.len(), 2_000);
    assert!(
        per_line <= MAX_BYTES_PER_LINE,
        "diff_lines peaked at {per_line} bytes per line"
    );
}
