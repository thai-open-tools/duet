//! The word-wrap spike (P9a Task 2): gpui's `list` against `uniform_list`, headless. A test
//! binary of its own, so its counting allocator measures only these tests (M8).

mod text_support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, IntoElement, ListAlignment, ListOffset, ListState,
    Render, ScrollStrategy, UniformListScrollHandle, Window, div, list, prelude::*, px,
    uniform_list,
};

use gpui_kit::test::TestWindowExt as _;

use text_support::*;

/// System allocator that counts live bytes and the peak since the last `reset_peak`.
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grow(by: usize) {
    let now = LIVE.fetch_add(by, Relaxed) + by;
    PEAK.fetch_max(now, Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size >= layout.size() {
            grow(new_size - layout.size());
        } else {
            LIVE.fetch_sub(layout.size() - new_size, Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

const TEXT: &str =
    "abcdefghijklmnopqrstuvwxyz abcdefghijklmnopqrstuvwxyz abcdefghijklmnopqrstuvwxyz 0123456";

struct SpikeRows {
    rows: usize,
    wrapped: bool,
    uniform: UniformListScrollHandle,
    list: ListState,
    seen: Rc<RefCell<Vec<usize>>>,
}

fn row(ix: usize, tall: bool) -> AnyElement {
    let number = div().w(px(52.)).h(px(20.)).child(format!("{ix}"));
    if tall {
        // Two 20 px text lines: a wrapped row.
        h_flex()
            .child(number)
            .child(
                v_flex()
                    .child(div().h(px(20.)).child(&TEXT[..40]))
                    .child(div().h(px(20.)).child(&TEXT[40..])),
            )
            .into_any_element()
    } else {
        h_flex()
            .h(px(20.))
            .child(number)
            .child(div().h(px(20.)).child(TEXT))
            .into_any_element()
    }
}

impl Render for SpikeRows {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows;
        let seen = self.seen.clone();
        if self.wrapped {
            div().size_full().child(
                list(self.list.clone(), move |ix, _, _| {
                    seen.borrow_mut().push(ix);
                    row(ix, ix % 4 == 3)
                })
                .size_full(),
            )
        } else {
            div().size_full().child(
                uniform_list(
                    "spike-rows",
                    rows,
                    cx.processor(move |_, range: Range<usize>, _, _| {
                        range
                            .map(|ix| {
                                seen.borrow_mut().push(ix);
                                row(ix, false)
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.uniform)
                .size_full(),
            )
        }
    }
}

struct Run {
    mode: &'static str,
    rows: usize,
    build_ms: f64,
    reset_ms: f64,
    bytes_per_item: f64,
    peak_per_item: f64,
    mean_ms: f64,
    p95_ms: f64,
    frames: usize,
}

fn open_spike(
    cx: &mut gpui_kit::TestAppContext,
    rows: usize,
    wrapped: bool,
    state: ListState,
) -> (
    gpui_kit::AnyWindowHandle,
    Entity<SpikeRows>,
    UniformListScrollHandle,
    Rc<RefCell<Vec<usize>>>,
) {
    let uniform = UniformListScrollHandle::new();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (handle, view) = {
        let (u, s) = (uniform.clone(), seen.clone());
        open_test_window(cx, move |_, cx| {
            cx.new(|_| SpikeRows {
                rows,
                wrapped,
                uniform: u,
                list: state,
                seen: s,
            })
        })
    };
    (handle, view, uniform, seen)
}

fn frame(
    cx: &mut gpui_kit::TestAppContext,
    handle: gpui_kit::AnyWindowHandle,
    view: &Entity<SpikeRows>,
) {
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |_, cx| cx.notify());
        window.render_frame(cx)
    })
    .unwrap();
}

fn run(cx: &mut gpui_kit::TestAppContext, rows: usize, wrapped: bool) -> Run {
    let (mut build_ms, mut reset_ms, mut bytes_per_item, mut peak_per_item) = (0.0, 0.0, 0.0, 0.0);
    let state;
    if wrapped {
        let before = LIVE.load(Relaxed);
        PEAK.store(before, Relaxed);
        let t = Instant::now();
        state =
            ListState::new(rows, ListAlignment::Top, px(200.)).with_uniform_item_height(px(20.));
        build_ms = t.elapsed().as_secs_f64() * 1e3;
        bytes_per_item = (LIVE.load(Relaxed) as f64 - before as f64) / rows as f64;
        peak_per_item = (PEAK.load(Relaxed) as f64 - before as f64) / rows as f64;
        let t = Instant::now();
        state.reset_with_uniform_height(rows, px(20.));
        reset_ms = t.elapsed().as_secs_f64() * 1e3;
    } else {
        state = ListState::new(0, ListAlignment::Top, px(200.));
    }
    let (handle, view, uniform, seen) = open_spike(cx, rows, wrapped, state.clone());
    let jump = |row: usize| {
        if wrapped {
            state.scroll_to(ListOffset {
                item_ix: row,
                offset_in_item: px(0.),
            });
        } else {
            uniform.scroll_to_item_strict(row, ScrollStrategy::Top);
        }
    };
    let mut row_ix = 0usize;
    for _ in 0..20 {
        row_ix = (row_ix + 997) % rows;
        jump(row_ix);
        frame(cx, handle, &view);
    }
    let mut times = Vec::new();
    for _ in 0..300 {
        row_ix = (row_ix + 997) % rows;
        jump(row_ix);
        let t = Instant::now();
        frame(cx, handle, &view);
        times.push(t.elapsed().as_secs_f64() * 1e3);
    }
    let _ = &seen;
    let frames = times.len();
    let mean_ms = times.iter().sum::<f64>() / frames as f64;
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p95_ms = times[(frames * 95 / 100).min(frames - 1)];
    if wrapped {
        state.reset(0);
    }
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
    Run {
        mode: if wrapped { "list" } else { "uniform" },
        rows,
        build_ms,
        reset_ms,
        bytes_per_item,
        peak_per_item,
        mean_ms,
        p95_ms,
        frames,
    }
}

fn print(r: &Run) {
    println!(
        "spike mode={} rows={} build_ms={:.1} reset_ms={:.1} bytes_per_item={:.1} peak_bytes_per_item={:.1} mean_ms={:.3} p95_ms={:.3}",
        r.mode,
        r.rows,
        r.build_ms,
        r.reset_ms,
        r.bytes_per_item,
        r.peak_per_item,
        r.mean_ms,
        r.p95_ms
    );
}

#[gpui_kit::test]
async fn wrap_spike_harness_draws_both_lists(cx: &mut gpui_kit::TestAppContext) {
    for wrapped in [false, true] {
        let rows = 1_000;
        let state =
            ListState::new(rows, ListAlignment::Top, px(200.)).with_uniform_item_height(px(20.));
        let (handle, view, uniform, seen) = open_spike(cx, rows, wrapped, state.clone());
        frame(cx, handle, &view);
        assert!(
            !seen.borrow().is_empty(),
            "wrapped={wrapped}: no rows drawn"
        );
        seen.borrow_mut().clear();
        if wrapped {
            state.scroll_to(ListOffset {
                item_ix: 997,
                offset_in_item: px(0.),
            });
        } else {
            uniform.scroll_to_item_strict(997, ScrollStrategy::Top);
        }
        frame(cx, handle, &view);
        assert!(
            seen.borrow().contains(&997),
            "wrapped={wrapped}: row 997 not drawn after the jump: {:?}",
            seen.borrow()
        );
    }
}

#[gpui_kit::test]
#[ignore = "P9a spike: run by hand, see plan Task 2"]
async fn wrap_spike_list_against_uniform_list(cx: &mut gpui_kit::TestAppContext) {
    let counts = [1_000_000, 500_000, 200_000, 100_000, 50_000, 20_000];
    let mut results: Vec<(Run, Run)> = Vec::new();
    for rows in counts {
        let uniform = run(cx, rows, false);
        print(&uniform);
        let wrapped = run(cx, rows, true);
        print(&wrapped);
        assert_eq!(uniform.frames, 300);
        assert_eq!(wrapped.frames, 300);
        results.push((uniform, wrapped));
    }
    // R2's rule, for the record (PF8: the limit is set from absolute numbers by t2-limit.py).
    let holds = |u: &Run, l: &Run| {
        l.mean_ms <= 1.25 * u.mean_ms && l.p95_ms <= 1.5 * u.p95_ms && l.reset_ms <= u.mean_ms
    };
    let limit = results
        .iter()
        .filter(|(u, l)| u.rows >= 100_000 && holds(u, l))
        .map(|(u, _)| u.rows)
        .max();
    match limit {
        Some(1_000_000) => println!("decision=pass (R2 ratios; PF8: no stop)"),
        Some(n) => println!("decision=fallback limit={n} (R2 ratios)"),
        None => println!("decision=fallback limit=50000 (R2 ratios)"),
    }
}
