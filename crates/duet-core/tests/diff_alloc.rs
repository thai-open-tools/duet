//! With ignore options each comparison key must be built about once per line, not once per
//! hash, prediction check and probe. A single test: the counting allocator is global.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};

use duet_core::cancel::CancelToken;
use duet_core::text::diff::{DiffInput, DiffOptions, WhitespaceMode, diff_lines};
use duet_core::text::lines::LineIndex;

struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

#[test]
fn ignore_options_build_each_key_about_once() {
    let (mut left, mut right) = (String::new(), String::new());
    for i in 0..100_000u64 {
        let line = format!(
            "  {i:08}\t{:016x}  lorem ipsum \n",
            i.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        );
        left.push_str(&line);
        if i % 100 == 0 {
            let _ = writeln!(right, "{i:08} changed");
        } else {
            right.push_str(&line);
        }
    }
    let (li, ri) = (
        LineIndex::new(left.as_bytes()),
        LineIndex::new(right.as_bytes()),
    );
    let lines = li.len() + ri.len();
    let opts = DiffOptions {
        whitespace: WhitespaceMode::All,
        ignore_case: true,
        ignore_eol: false,
    };
    let before = ALLOCS.load(Ordering::Relaxed);
    let diff = diff_lines(
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
    .unwrap();
    let per_line = (ALLOCS.load(Ordering::Relaxed) - before) as f64 / lines as f64;
    assert_eq!(diff.hunks.len(), 1_000);
    assert!(per_line <= 2.0, "{per_line:.2} allocations per line");
}
