//! What `tr` costs (D11): a frame makes at most about 300 lookups, so a lookup allocates nothing
//! and the bounds below, loose as they are in a debug test build, fail only on a real mistake such
//! as a linear scan or a sort per call. The allocation count is per thread, so tests running side
//! by side do not add to it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::{Duration, Instant};

use duet_app::i18n::{Lang, scoped, tr, trc};

thread_local! {
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|count| count.set(count.get() + 1));
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// Keys the table holds, and one it does not, so a lookup is not always the same probe.
const KEYS: [&str; 3] = [
    "files",
    "lines",
    "A key with no Thai entry, on the path a miss takes",
];

/// The median of 20 rounds of 2,000 lookups.
fn median_of_rounds(mut lookup: impl FnMut(&'static str) -> &'static str) -> Duration {
    let mut rounds: Vec<Duration> = (0..20)
        .map(|_| {
            let start = Instant::now();
            for i in 0..2_000 {
                std::hint::black_box(lookup(KEYS[i % KEYS.len()]));
            }
            start.elapsed()
        })
        .collect();
    rounds.sort();
    rounds[rounds.len() / 2]
}

#[test]
fn tr_allocates_nothing_in_either_language() {
    let count = |run: &dyn Fn()| {
        let before = ALLOCS.with(Cell::get);
        for _ in 0..1_000 {
            run();
        }
        ALLOCS.with(Cell::get) - before
    };
    let lookups = || {
        std::hint::black_box(tr(KEYS[0]));
        std::hint::black_box(tr(KEYS[1]));
        std::hint::black_box(trc("a context", KEYS[1]));
    };
    assert_eq!(count(&lookups), 0, "English");
    let _th = scoped(Lang::Th);
    // The first Thai lookup builds the table, once.
    lookups();
    assert_eq!(count(&lookups), 0, "Thai");
}

#[test]
fn a_frame_of_lookups_stays_inside_the_budget() {
    let english = median_of_rounds(tr);
    assert!(english < Duration::from_millis(2), "English: {english:?}");
    let _th = scoped(Lang::Th);
    let thai = median_of_rounds(tr);
    assert!(thai < Duration::from_millis(16), "Thai: {thai:?}");
}

/// By hand: `cargo test --release -p duet-app --test i18n_cost -- --ignored --nocapture`.
#[test]
#[ignore = "prints release figures for a final-round row"]
fn bench_tr_by_hand() {
    let per_lookup = |round: Duration| round / 2_000;
    println!("English: {:?} a lookup", per_lookup(median_of_rounds(tr)));
    let _th = scoped(Lang::Th);
    println!("Thai: {:?} a lookup", per_lookup(median_of_rounds(tr)));
}
