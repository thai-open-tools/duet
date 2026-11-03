//! Headless UI tests for the final-round fixes to wrapped rows, the overview strip and the
//! details pane (X-12, X-18 / X-66, X-32 / X-64, X-38).

mod text_support;

use duet_app::text::view::TextView;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Bounds, Entity, Pixels, TestAppContext, px};
use text_support::*;

fn set_wrap(cx: &mut TestAppContext, view: &Entity<TextView>, on: bool) -> bool {
    view.update(cx, |v, cx| v.set_wrap(on, cx))
}

/// The text columns a half shows, after two frames (the first measures the list).
fn columns(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<TextView>) -> usize {
    render(cx, handle);
    render(cx, handle);
    view.read_with(cx, |v, _| v.display_options().view_columns)
}

/// A line of `segments` wrapped segments at `columns` columns, no spaces.
fn tall_line(ch: char, columns: usize, segments: usize) -> String {
    ch.to_string().repeat(segments * columns - 5)
}

fn bounds_of(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> Bounds<Pixels> {
    cx.update_window(handle, |_, window, _| window.find(id).bounds())
        .unwrap()
}

/// Writes `left` and `right` over the open pair, compares again, turns wrap on and draws.
async fn reopen_wrapped(
    cx: &mut TestAppContext,
    dir: &tempfile::TempDir,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
    (left, right): (String, String),
) {
    std::fs::write(dir.path().join("a.txt"), left).unwrap();
    std::fs::write(dir.path().join("b.txt"), right).unwrap();
    press(cx, handle, "f5");
    wait_loaded(cx, handle, view).await;
    assert!(set_wrap(cx, view, true));
    render(cx, handle);
    render(cx, handle);
}

// ---- X-38: the current difference follows a tall row's top ----

#[gpui_kit::test]
async fn a_tall_difference_is_current_once_its_top_reaches_the_30_percent_line(
    cx: &mut TestAppContext,
) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let columns = columns(cx, handle, &view);
    // Every row three lines tall; rows 10 and 30 differ, and row 30 is ten lines tall.
    let three = tall_line('x', columns, 3);
    let ten = tall_line('y', columns, 10);
    let left: String = (0..60)
        .map(|i| match i {
            30 => format!("{ten}\n"),
            _ => format!("{three}\n"),
        })
        .collect();
    let right: String = (0..60)
        .map(|i| match i {
            10 => format!("z{}\n", &three[1..]),
            30 => format!("z{}\n", &ten[1..]),
            _ => format!("{three}\n"),
        })
        .collect();
    reopen_wrapped(cx, &dir, handle, &view, (left, right)).await;
    view.update(cx, |v, cx| v.scroll_like_the_user_for_test(25, cx));
    render(cx, handle);
    render(cx, handle);
    assert_eq!(
        current_block(cx, &view),
        Some(0),
        "row 30 is below the line"
    );
    let list = bounds_of(cx, handle, "text-list");
    let line = list.top() + list.size.height * 0.3;
    let mut reached = false;
    for step in 0..120 {
        wheel(cx, handle, list.center(), -5.);
        render(cx, handle);
        let top = cx
            .update_window(handle, |_, window, _| {
                window
                    .try_find(("text-row", 30usize))
                    .map(|r| r.bounds().top())
            })
            .unwrap();
        let Some(top) = top else { continue };
        let want = if top <= line - px(0.5) {
            Some(1)
        } else if top > line + px(0.5) {
            Some(0)
        } else {
            continue;
        };
        assert_eq!(
            current_block(cx, &view),
            want,
            "step {step}: row 30's top at {top:?}, the 30% line at {line:?}"
        );
        if want == Some(1) {
            reached = true;
            break;
        }
    }
    assert!(reached, "row 30's top never reached the line");
}

// ---- X-12: the overview strip with wrap on ----

fn thumb(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<TextView>) -> (f32, f32) {
    render(cx, handle);
    view.read_with(cx, |v, _| v.overview_thumb_for_test())
        .expect("the strip has a thumb")
}

/// Presses the left button at `fraction` of the overview strip, moves it to each of `then`
/// with the button held and releases it there; the thumb after each step.
fn press_strip(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
    fraction: f32,
    then: &[f32],
) -> Vec<(f32, f32)> {
    let strip = bounds_of(cx, handle, "text-overview");
    let at = |f: f32| gpui_kit::point(strip.center().x, strip.top() + strip.size.height * f);
    let mut thumbs = Vec::new();
    cx.update_window(handle, |_, window, cx| {
        pointer(window, button(at(fraction), true), cx)
    })
    .unwrap();
    thumbs.push(thumb(cx, handle, view));
    for &f in then {
        cx.update_window(handle, |_, window, cx| {
            pointer(window, move_to(at(f), true), cx)
        })
        .unwrap();
        thumbs.push(thumb(cx, handle, view));
    }
    let last = then.last().copied().unwrap_or(fraction);
    cx.update_window(handle, |_, window, cx| {
        pointer(window, button(at(last), false), cx)
    })
    .unwrap();
    thumbs
}

#[gpui_kit::test]
async fn the_overview_follows_wrapped_rows_and_scrolls_to_the_spot(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let columns = columns(cx, handle, &view);
    // 20 rows four lines tall, but row 10 is sixty; rows 2 and 15 differ.
    let four = tall_line('x', columns, 4);
    let sixty = tall_line('y', columns, 60);
    let row = |i: usize, changed: bool| {
        let text = if i == 10 { &sixty } else { &four };
        if changed {
            format!("z{}\n", &text[1..])
        } else {
            format!("{text}\n")
        }
    };
    let left: String = (0..20).map(|i| row(i, false)).collect();
    let right: String = (0..20).map(|i| row(i, i == 2 || i == 15)).collect();
    reopen_wrapped(cx, &dir, handle, &view, (left, right)).await;
    assert_eq!(thumb(cx, handle, &view).0, 0.0);

    // The wheel moves the thumb on every notch, also inside the sixty-line row.
    let list = bounds_of(cx, handle, "text-list");
    let mut last = thumb(cx, handle, &view);
    for notch in 0..18 {
        wheel(cx, handle, list.center(), -100.);
        let now = thumb(cx, handle, &view);
        assert!(now.0 > last.0, "notch {notch}: {last:?} then {now:?}");
        last = now;
    }

    // Ctrl+Home and Ctrl+End: the thumb takes in the difference's row.
    for (key, row) in [("ctrl-home", 2.), ("ctrl-end", 15.)] {
        press(cx, handle, key);
        let (top, bottom) = thumb(cx, handle, &view);
        let at = row / 20.;
        assert!(
            top <= at && at <= bottom,
            "{key}: {top}..{bottom}, row {row}"
        );
    }

    // A click puts that spot of the strip on screen, inside the sixty-line row too.
    for f in [0.525, 0.3, 0.55] {
        let (top, bottom) = press_strip(cx, handle, &view, f, &[])[0];
        assert!(top < f && f < bottom, "click at {f}: thumb {top}..{bottom}");
    }
    // Dragging the thumb scrolls with it.
    let thumbs = press_strip(cx, handle, &view, 0.525, &[0.45, 0.4, 0.6]);
    for (thumb, f) in thumbs.iter().zip([0.525, 0.45, 0.4, 0.6]) {
        assert!(thumb.0 < f && f < thumb.1, "drag at {f}: thumb {thumb:?}");
    }
    assert!(
        thumbs.windows(2).all(|w| w[0] != w[1]),
        "every step scrolled: {thumbs:?}"
    );
}

// ---- X-32 / X-64: the details pane scrolls inside; its blocks never overlap ----

#[gpui_kit::test]
async fn the_details_pane_scrolls_inside_and_its_blocks_never_overlap(cx: &mut TestAppContext) {
    // Line 5 as in `wrap-a.txt`: 14,000 characters, cut at 200 segments.
    let long = "abcdefghij".repeat(1_400);
    let left = format!("a\nb\nc\nd\n{long}\nshort\n");
    let right = format!("a\nb\nc\nd\n{long}x\nshort\n");
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.set_details(true, cx);
        v.select_rows_for_test(true, 4, 4, cx);
    });
    render(cx, handle);
    render(cx, handle);
    let blocks = |cx: &mut TestAppContext| {
        render(cx, handle);
        (
            bounds_of(cx, handle, "text-details-pane"),
            bounds_of(cx, handle, "text-details-left"),
            bounds_of(cx, handle, "text-details-right"),
        )
    };
    let (pane, left_block, right_block) = blocks(cx);
    let window = cx
        .update_window(handle, |_, window, _| window.viewport_size().height)
        .unwrap();
    assert!(
        pane.size.height <= window * 0.3 + px(0.5),
        "the cap: {pane:?}"
    );
    assert!(
        left_block.bottom() <= right_block.top() + px(0.5),
        "the blocks overlap: {left_block:?} and {right_block:?}"
    );
    assert!(right_block.bottom() > pane.bottom(), "taller than the pane");
    // The wheel over the pane scrolls it: the left block's last segment comes into view, then
    // the right block's.
    let mut left_end_seen = false;
    for _ in 0..100 {
        wheel(cx, handle, pane.center(), -200.);
        let (pane, left_block, right_block) = blocks(cx);
        assert!(left_block.bottom() <= right_block.top() + px(0.5));
        left_end_seen |= left_block.bottom() > pane.top() && left_block.bottom() <= pane.bottom();
        if right_block.bottom() <= pane.bottom() + px(0.5) {
            break;
        }
    }
    let (pane, _, right_block) = blocks(cx);
    assert!(left_end_seen, "the left block's end was never on screen");
    assert!(
        right_block.bottom() <= pane.bottom() + px(0.5),
        "the right block's end is reachable: {right_block:?} in {pane:?}"
    );
    // The rows keep their place above the pane.
    let rows = bounds_of(cx, handle, "text-list");
    assert!(
        rows.bottom() <= pane.top() + px(0.5),
        "{rows:?} above {pane:?}"
    );
}

/// A mouse wheel notch over `position`: three lines, as Windows sends it.
fn wheel_notch(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    position: gpui_kit::Point<Pixels>,
    down: bool,
) {
    use gpui_kit::InputEvent as _;
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(
            gpui_kit::ScrollWheelEvent {
                position,
                delta: gpui_kit::ScrollDelta::Lines(gpui_kit::point(
                    0.,
                    if down { -3. } else { 3. },
                )),
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
async fn every_row_of_the_details_pane_shows_which_side_it_is(cx: &mut TestAppContext) {
    // RR3: line 5 of wrap-a/wrap-b, 14,000 characters, the same on both sides, so the two
    // blocks are the same 26-letter run and only their labels tell them apart. Wave 1's pin
    // checked the blocks' bounds; the re-run scrolled to the end, saw a block end and no
    // "Right 5" label (81 rows up), and took the right block for the left.
    let line: String = (0..14_000)
        .map(|i| (b'a' + (i % 26) as u8) as char)
        .collect();
    let text = format!("a\nb\nc\nd\n{line}\nshort\n");
    let (_dir, handle, view) = open_text(cx, &text, &text.replacen("a\n", "A\n", 1));
    wait_loaded(cx, handle, &view).await;
    assert!(set_wrap(cx, &view, true));
    view.update(cx, |v, cx| {
        v.set_details(true, cx);
        v.select_rows_for_test(true, 4, 4, cx);
    });
    render(cx, handle);
    render(cx, handle);
    let pane = bounds_of(cx, handle, "text-details-pane");
    // Inside the pane's top border and padding: where its rows show.
    let rows_top = pane.top() + px(5.);
    let mut right_seen = false;
    for notch in 0..400 {
        let (left, right, left_label, right_label) = cx
            .update_window(handle, |_, window, _| {
                let snap = |id: &'static str| {
                    let found = window.find(id);
                    (found.bounds(), found.visible())
                };
                (
                    snap("text-details-left").0,
                    snap("text-details-right").0,
                    snap("text-details-left-label"),
                    snap("text-details-right-label"),
                )
            })
            .unwrap();
        // The block whose rows are at the top of the pane names its side there.
        let (name, (label, visible)) = if left.bottom() > rows_top + px(1.) {
            ("Left", left_label)
        } else {
            ("Right", right_label)
        };
        assert!(
            visible
                && label.top() >= rows_top - px(0.5)
                && label.bottom() <= pane.bottom() + px(0.5),
            "notch {notch}: the {name} block fills the top of the pane, its label is at {label:?}, the pane {pane:?}"
        );
        right_seen |= name == "Right";
        if right.bottom() <= pane.bottom() + px(0.5) {
            break;
        }
        wheel_notch(cx, handle, pane.center(), true);
        render(cx, handle);
    }
    assert!(right_seen, "the wheel reaches the right block");
    // The rows are cut inside the pane's padding, so a row half scrolled out is never drawn
    // against the list's last row at the pane's top edge.
    let rows = bounds_of(cx, handle, "text-details-scroll");
    assert!(
        rows.top() >= pane.top() + px(4.),
        "the rows' clip {rows:?} starts inside the pane {pane:?}"
    );
}

// ---- X-18 / X-66: a resize drag with wrap on ----

/// `rows` rows where every tenth is a line of three segments at `columns` columns; one
/// difference, at row 5.
fn every_tenth_tall(columns: usize, rows: usize) -> (String, String) {
    let long = tall_line('x', columns, 3);
    let left: String = (0..rows)
        .map(|i| {
            if i % 10 == 0 {
                format!("{long}\n")
            } else {
                format!("line {i}\n")
            }
        })
        .collect();
    let right = left.replacen("line 5\n", "LINE 5\n", 1);
    (left, right)
}

/// Drags the window's right edge from 1200 px to the left, `px_a_step` px a step for `steps`
/// steps. A step draws two frames, as the window does: the list's new width shows in the first,
/// and its canvas asks for the second. The time each frame took.
fn drag_the_edge(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    px_a_step: f32,
    steps: usize,
) -> Vec<std::time::Duration> {
    let mut times = Vec::with_capacity(2 * steps);
    for step in 1..=steps {
        let width = 1200. - px_a_step * step as f32;
        cx.simulate_window_resize(handle, gpui_kit::size(px(width), px(800.)));
        for _ in 0..2 {
            let start = std::time::Instant::now();
            render(cx, handle);
            times.push(start.elapsed());
        }
    }
    times
}

#[gpui_kit::test]
async fn a_resize_drag_keeps_the_wrapped_rows_until_the_width_settles(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let columns = columns(cx, handle, &view);
    reopen_wrapped(cx, &dir, handle, &view, every_tenth_tall(columns, 2_000)).await;
    let passes = |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.wrap_passes_for_test());
    let list_width = |cx: &mut TestAppContext| {
        view.read_with(cx, |v, _| v.wrapped_list_width_for_test())
            .expect("the wrapped list")
    };
    let row_0 = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, _| {
            window.find(("text-row", 0usize)).bounds().size.height
        })
        .unwrap()
    };
    let row_height = px(view.read_with(cx, |v, _| v.row_height()));
    let (before, width, tall) = (passes(cx), list_width(cx), row_0(cx));
    assert_eq!(
        tall,
        row_height * 3.,
        "row 0 is three lines at {columns} columns"
    );
    // While the edge moves, the list keeps the width it had: gpui's list drops every row's
    // height whenever its width changes, a pass over every row a frame that duet's own
    // counter never saw (wave 1's pin counted only duet's passes).
    for step in 1..=60 {
        cx.simulate_window_resize(
            handle,
            gpui_kit::size(px(1200. - 2. * step as f32), px(800.)),
        );
        for frame in 0..2 {
            render(cx, handle);
            assert_eq!(
                list_width(cx),
                width,
                "step {step}, frame {frame}: the list keeps its width while the edge moves"
            );
        }
    }
    assert_eq!(
        passes(cx),
        before,
        "no pass over every row while the width keeps changing"
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.display_options().view_columns),
        columns,
        "the rows keep their wrap"
    );
    assert_eq!(row_0(cx), tall);
    let area = bounds_of(cx, handle, "text-list");
    assert!(
        area.size.width < width,
        "the area is narrower: {area:?}, the list {width:?}"
    );
    // Once the width is still, the list takes it and every row is measured again, once.
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    render(cx, handle);
    render(cx, handle);
    assert_eq!(list_width(cx), area.size.width, "the last frame is exact");
    let now = view.read_with(cx, |v, _| v.display_options().view_columns);
    assert!(now < columns, "{now} columns, {columns} before");
    let long = 3 * columns - 5;
    assert_eq!(
        row_0(cx),
        row_height * long.div_ceil(now) as f32,
        "row 0 at {now} columns"
    );
    assert_eq!(passes(cx), before + 1, "one pass once it settled");
    view.update(cx, |v, cx| v.scroll_to_fraction(1.0, cx));
    render(cx, handle);
    render(cx, handle);
    let last = cx
        .update_window(handle, |_, window, _| {
            let list = window.find("text-list").bounds();
            window
                .try_find(("text-row", 1_999usize))
                .is_some_and(|row| row.bounds().bottom() <= list.bottom() + px(0.5))
        })
        .unwrap();
    assert!(last, "the end of the list is reachable at the new heights");
}

/// The frame times of a slow edge drag at the wrap limit (X-18's `limit-ok`: 200,000 rows), as
/// the debug build draws them, with wrap off, wrap on, and wrap on with the details pane of a
/// tall row. Run by hand: `cargo test -p duet-app --test ui_text_wrap -- --ignored --nocapture`.
#[gpui_kit::test]
#[ignore = "a timing bench: run by hand"]
async fn bench_a_resize_drag_at_the_wrap_limit(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let columns = columns(cx, handle, &view);
    reopen_wrapped(cx, &dir, handle, &view, every_tenth_tall(columns, 200_000)).await;
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.;
    let drag = |cx: &mut TestAppContext, what: &str| {
        let counts = |cx: &mut TestAppContext| {
            view.read_with(cx, |v, _| {
                (v.wrap_passes_for_test(), v.rows_built_for_test())
            })
        };
        let (passes, built) = counts(cx);
        let mut times = drag_the_edge(cx, handle, 3., 100);
        let (passes, built) = (counts(cx).0 - passes, counts(cx).1 - built);
        times.sort();
        println!(
            "{what}: 200 frames of a 300 px drag (debug build): median {:.1} ms, p95 {:.1} ms, max {:.1} ms; {passes} passes over every row, {:.1} rows built a frame",
            ms(times[times.len() / 2]),
            ms(times[times.len() * 95 / 100]),
            ms(times[times.len() - 1]),
            built as f64 / times.len() as f64,
        );
        // Back to the start, settled.
        cx.simulate_window_resize(handle, gpui_kit::size(px(1200.), px(800.)));
        render(cx, handle);
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        let start = std::time::Instant::now();
        render(cx, handle);
        render(cx, handle);
        println!(
            "{what}: the settle frames took {:.1} ms",
            ms(start.elapsed())
        );
    };
    drag(cx, "wrap on");
    view.update(cx, |v, cx| {
        v.set_details(true, cx);
        v.select_rows_for_test(true, 0, 0, cx);
    });
    render(cx, handle);
    drag(cx, "wrap on, details on");
    view.update(cx, |v, cx| v.set_details(false, cx));
    assert!(set_wrap(cx, &view, false));
    render(cx, handle);
    render(cx, handle);
    drag(cx, "wrap off");
}
