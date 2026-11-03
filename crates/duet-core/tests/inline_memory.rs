//! Inline refinement must not allocate per grapheme for lines above the cap (Review Focus 4).
//! A single test: the counting allocator is global to this test binary.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use duet_core::text::inline::refine;

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

#[test]
fn long_lines_are_marked_whole_without_per_grapheme_allocation() {
    let (left, right) = ("ก".repeat(2 << 20), "ข".repeat(2 << 20));
    let base = CURRENT.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let diff = refine(&left, &right);
    let extra = PEAK.load(Ordering::Relaxed) - base;
    assert_eq!(diff.left, vec![0..left.len()]);
    assert_eq!(diff.right, vec![0..right.len()]);
    assert!(
        extra < 1 << 20,
        "refine allocated {extra} bytes for two 2M-grapheme lines"
    );
}
