//! Headless UI tests for the text view's layout, display and navigation (P9a).

mod text_support;

use std::cell::Cell;
use std::rc::Rc;

use duet_app::text::display::MAX_WRAP_SEGMENTS;
use duet_app::text::view::{
    DEFAULT_FONT_SIZE, DEFAULT_ROW_HEIGHT, TextEvent, TextView, row_height_for,
};
use gpui_kit::test::{TestAppContextExt as _, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, EntityInputHandler as _, Focusable as _,
    TestAppContext, point, px,
};
use text_support::*;

/// Where line `line` (0-based) of `text` starts, in bytes.
fn line_start(text: &str, line: usize) -> usize {
    text.split_inclusive('\n').take(line).map(str::len).sum()
}

fn current_block(cx: &mut TestAppContext, view: &Entity<TextView>) -> Option<usize> {
    view.read_with(cx, |v, _| v.model().unwrap().current_block())
}

fn wrapped(cx: &mut TestAppContext, view: &Entity<TextView>) -> bool {
    view.read_with(cx, |v, _| v.rows_wrapped())
}

/// The rows a screen of the list holds at the view's row height.
fn page(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<TextView>) -> usize {
    let row_height = view.read_with(cx, |v, _| v.row_height());
    cx.update_window(handle, |_, window, _| {
        (f32::from(window.find("text-list").bounds().size.height) / row_height).floor() as usize
    })
    .unwrap()
}

/// After two frames (a uniform list applies a scroll when it lays out), row `display` is wholly
/// inside the list, and `top_row` agrees: the row is within a page (and a part row) of it.
fn assert_on_screen(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
    display: usize,
    what: &str,
) {
    render(cx, handle);
    let top = top_row(cx, handle, view);
    let page = page(cx, handle, view);
    assert!(
        (top..=top + page).contains(&display),
        "{what}: row {display} with top {top} and {page} rows a page"
    );
    cx.update_window(handle, |_, window, _| {
        let list = window.find("text-list").bounds();
        let row = window.try_find(("text-row", display)).map(|r| r.bounds());
        assert!(
            row.is_some_and(
                |r| r.top() >= list.top() - px(0.5) && r.bottom() <= list.bottom() + px(0.5)
            ),
            "{what}: row {display} at {row:?}, the list at {list:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
async fn both_row_lists_navigate_the_same(cx: &mut TestAppContext) {
    for list in [false, true] {
        let (left, right) = pair_300();
        let (_dir, handle, view) = open_text(cx, &left, &right);
        wait_loaded(cx, handle, &view).await;
        view.update(cx, |v, cx| v.set_list_rows_for_test(list, cx));
        render(cx, handle);
        assert_eq!(wrapped(cx, &view), list);

        press(cx, handle, "f8");
        assert_eq!(current_block(cx, &view), Some(0), "wrapped list {list}");
        assert_on_screen(cx, handle, &view, 40, "F8");
        press(cx, handle, "f8");
        assert_eq!(current_block(cx, &view), Some(1), "wrapped list {list}");
        assert_on_screen(cx, handle, &view, 150, "F8 again");

        press(cx, handle, "ctrl-g");
        cx.update_window(handle, |_, window, cx| {
            window.input("251", cx);
            window.press("enter", cx);
        })
        .unwrap();
        assert_eq!(view.read_with(cx, |v, _| v.marked_row()), Some(250));
        assert_on_screen(cx, handle, &view, 250, "Ctrl+G 251");

        view.update(cx, |v, cx| v.scroll_rows(-1000, cx));
        assert_eq!(top_row(cx, handle, &view), 0, "wrapped list {list}");
        press(cx, handle, "pagedown");
        render(cx, handle);
        assert!(
            top_row(cx, handle, &view) > 0,
            "PageDown, wrapped list {list}"
        );

        view.update(cx, |v, cx| v.scroll_to_fraction(0.5, cx));
        render(cx, handle);
        let (top, page) = (top_row(cx, handle, &view), page(cx, handle, &view));
        assert!(
            top.abs_diff(150) <= page,
            "half way down is row 150: top {top}, page {page}, wrapped list {list}"
        );
        assert_eq!(wrapped(cx, &view), list, "the list kind holds throughout");
    }
}

#[gpui_kit::test]
async fn font_size_sets_the_row_height_and_is_clamped(cx: &mut TestAppContext) {
    // round(size x 1.6), LocalDiff's TV.rowH.
    for (size, height) in [
        (9., 14.),
        (10., 16.),
        (11., 18.),
        (12., 19.),
        (12.5, 20.),
        (13., 21.),
        (14., 22.),
        (15., 24.),
        (16., 26.),
        (18., 29.),
        (20., 32.),
        (22., 35.),
        (24., 38.),
    ] {
        assert_eq!(row_height_for(size), height, "font size {size}");
    }
    assert_eq!(row_height_for(DEFAULT_FONT_SIZE), DEFAULT_ROW_HEIGHT);

    let (left, right) = pair_300();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    let size_and_height =
        |cx: &mut TestAppContext| view.read_with(cx, |v, _| (v.font_size(), v.row_height()));
    assert_eq!(size_and_height(cx), (12.5, 20.));

    view.update(cx, |v, cx| v.set_font_size(24., cx));
    render(cx, handle);
    render(cx, handle);
    let height = cx
        .update_window(handle, |_, window, _| {
            window.find(("text-row", 0usize)).bounds().size.height
        })
        .unwrap();
    assert_eq!(height, px(38.), "a row at 24 px text");

    view.update(cx, |v, cx| v.set_font_size(30., cx));
    assert_eq!(
        size_and_height(cx),
        (24., 38.),
        "clamped to the largest size"
    );
    view.update(cx, |v, cx| v.set_font_size(5., cx));
    assert_eq!(
        size_and_height(cx),
        (9., 14.),
        "clamped to the smallest size"
    );
}

#[gpui_kit::test]
async fn the_columns_follow_the_font_size(cx: &mut TestAppContext) {
    let (left, right) = pair_300();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    let columns = |cx: &mut TestAppContext| {
        render(cx, handle);
        render(cx, handle);
        view.read_with(cx, |v, _| v.display_options().view_columns)
    };
    let small = columns(cx);
    view.update(cx, |v, cx| v.set_font_size(24., cx));
    let large = columns(cx);
    assert!(
        large < small,
        "{large} columns at 24 px, {small} at 12.5 px"
    );
    assert!(
        large as f32 >= small as f32 * 12.5 / 24. - 2.,
        "{large} columns at 24 px, {small} at 12.5 px"
    );
}

#[gpui_kit::test]
async fn wrap_off_keeps_the_uniform_list(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nB\n");
    wait_loaded(cx, handle, &view).await;
    assert!(!wrapped(cx, &view), "a new view");
    view.update(cx, |v, cx| v.set_font_size(16., cx));
    render(cx, handle);
    assert!(!wrapped(cx, &view), "after a font change");
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    assert!(!wrapped(cx, &view), "after a reload");
    press(cx, handle, "f9");
    wait_loaded(cx, handle, &view).await;
    assert!(!wrapped(cx, &view), "after F9");
}

#[gpui_kit::test]
async fn a_wrapped_list_keeps_its_top_row_through_a_row_change(cx: &mut TestAppContext) {
    let (left, right) = pair_300();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_list_rows_for_test(true, cx));
    render(cx, handle);
    assert!(wrapped(cx, &view));
    view.update(cx, |v, cx| v.scroll_rows(120, cx));
    assert_eq!(top_row(cx, handle, &view), 120);

    // PF2: the caret sits on a line on screen, since every edit scrolls to the caret.
    view.update(cx, |v, cx| {
        v.set_caret_for_test(false, line_start(&right, 130), cx)
    });
    press(cx, handle, "enter");
    wait_loaded(cx, handle, &view).await;
    render(cx, handle);
    let top = top_row(cx, handle, &view);
    assert!(
        top.abs_diff(120) <= 1,
        "a row added below the top row leaves it in place: top {top}"
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().row_count()),
        301
    );

    // A caret off screen: the edit scrolls to it.
    let text = right_text(cx, &view);
    view.update(cx, |v, cx| {
        v.set_caret_for_test(false, line_start(&text, 250), cx)
    });
    press(cx, handle, "enter");
    wait_loaded(cx, handle, &view).await;
    // Right line 251 is row 251: one right-only row came before it.
    assert_on_screen(cx, handle, &view, 251, "the caret's row after Enter");
}

/// PF1: only rows the list painted keep their text for the IME. Row 0 is the uniform list's
/// sizing row in every frame; row 95 is in the wrapped list's leading overdraw (measured, never
/// painted) in the first frame at top row 100. Neither may reach the IME's queries.
#[gpui_kit::test]
async fn a_caret_off_screen_does_not_panic_the_ime_queries(cx: &mut TestAppContext) {
    for list in [false, true] {
        let (left, right) = pair_300();
        let (_dir, handle, view) = open_text(cx, &left, &right);
        wait_loaded(cx, handle, &view).await;
        view.update(cx, |v, cx| v.set_list_rows_for_test(list, cx));
        render(cx, handle);
        render(cx, handle);
        assert_eq!(wrapped(cx, &view), list);
        let ime = |cx: &mut TestAppContext| {
            cx.update_window(handle, |_, window, cx| {
                view.update(cx, |v, cx| {
                    (
                        v.bounds_for_range(0..0, Bounds::default(), window, cx),
                        v.character_index_for_point(point(px(1.), px(1.)), window, cx),
                    )
                })
            })
            .unwrap()
        };
        for line in [95, 0] {
            view.update(cx, |v, cx| v.scroll_rows(-1000, cx));
            render(cx, handle);
            render(cx, handle);
            // gpui's test mode draws a notified window as the update ends: that frame is the
            // first at row 100, the one whose overdraw rows are measured and not painted.
            view.update(cx, |v, cx| {
                v.set_caret_for_test(true, line_start(&left, line), cx);
                v.scroll_rows(100, cx);
            });
            assert_eq!(
                ime(cx),
                (None, None),
                "the caret's line {line} is off screen, wrapped list {list}"
            );
        }
        view.update(cx, |v, cx| {
            v.set_caret_for_test(true, line_start(&left, 105), cx)
        });
        render(cx, handle);
        let (bounds, index) = ime(cx);
        assert_eq!(
            bounds.map(|b| b.size.height),
            Some(px(20.)),
            "on screen the IME gets the caret's box, one row tall; wrapped list {list}"
        );
        assert!(index.is_some(), "wrapped list {list}");
    }
}

#[gpui_kit::test]
async fn the_split_view_still_opens_compares_and_navigates(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nB\n");
    wait_loaded(cx, handle, &view).await;
    let swapped = Rc::new(Cell::new(false));
    let sink = swapped.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, e: &TextEvent, _| {
            if matches!(e, TextEvent::Swapped { .. }) {
                sink.set(true);
            }
        })
        .detach()
    });

    press(cx, handle, "f8");
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().current_block()),
        Some(0)
    );
    let status = view.read_with(cx, |v, _| v.status_line().to_string());
    assert!(status.starts_with("1 difference"), "{status}");

    press(cx, handle, "f9");
    assert!(swapped.get(), "F9 emits Swapped");
}

fn set_wrap(cx: &mut TestAppContext, view: &Entity<TextView>, on: bool) -> bool {
    view.update(cx, |v, cx| v.set_wrap(on, cx))
}

fn wrap_on(cx: &mut TestAppContext, view: &Entity<TextView>) -> bool {
    view.read_with(cx, |v, _| v.wrap())
}

/// The text columns a half shows, after two frames (the first measures the list).
fn columns(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<TextView>) -> usize {
    render(cx, handle);
    render(cx, handle);
    view.read_with(cx, |v, _| v.display_options().view_columns)
}

/// What each line of one half showed in the last frame: one per segment when wrapped.
fn segments(
    cx: &mut TestAppContext,
    view: &Entity<TextView>,
    display: usize,
    left: bool,
) -> Vec<String> {
    view.read_with(cx, |v, _| v.segment_texts_for_test(display, left))
}

#[gpui_kit::test]
async fn wrap_grows_a_long_rows_height_and_drops_the_horizontal_bar(cx: &mut TestAppContext) {
    let line = "x".repeat(300);
    let (_dir, handle, view) = open_text(cx, &format!("{line}\n"), "y\n");
    wait_loaded(cx, handle, &view).await;
    render(cx, handle);
    render(cx, handle);
    cx.update_window(handle, |_, window, _| {
        assert!(
            window.try_find("text-hscroll").is_some(),
            "a 300-column line scrolls sideways"
        );
    })
    .unwrap();
    press(cx, handle, "right");
    assert!(view.read_with(cx, |v, _| v.display_options().skip_columns) > 0);

    assert!(set_wrap(cx, &view, true));
    assert!(wrap_on(cx, &view) && wrapped(cx, &view));
    let c = columns(cx, handle, &view);
    let k = 300usize.div_ceil(c).min(MAX_WRAP_SEGMENTS);
    assert!(k > 1, "{c} columns");
    cx.update_window(handle, |_, window, _| {
        let row = window.find(("text-row", 0usize)).bounds();
        let want = k as f32 * DEFAULT_ROW_HEIGHT;
        assert!(
            (f32::from(row.size.height) - want).abs() <= 0.5,
            "{k} segments at {c} columns: row {row:?}"
        );
        assert!(
            window.try_find("text-hscroll").is_none(),
            "a wrapped view has no horizontal bar"
        );
    })
    .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.display_options().skip_columns),
        0
    );
    let shown = segments(cx, &view, 0, true);
    assert_eq!(shown.len(), k, "{shown:?}");
    assert_eq!(shown.concat(), line);
    assert_eq!(segments(cx, &view, 0, false), ["y"]);
}

#[gpui_kit::test]
async fn a_click_on_a_later_segment_places_the_caret_there(cx: &mut TestAppContext) {
    let line = "x".repeat(300);
    let (_dir, handle, view) = open_text(cx, &format!("{line}\n"), "y\n");
    wait_loaded(cx, handle, &view).await;
    render(cx, handle);
    assert!(set_wrap(cx, &view, true));
    let c = columns(cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        // Segment 1 of display row 0.
        let at = window.find(("text-seg-left", 1usize)).bounds().origin + point(px(1.), px(10.));
        pointer(window, move_to(at, false), cx);
        pointer(window, button(at, true), cx);
        pointer(window, button(at, false), cx);
    })
    .unwrap();
    let caret = view
        .read_with(cx, |v, _| v.caret())
        .expect("a caret on the left");
    assert_eq!(
        (caret.left, caret.head),
        (true, c),
        "ASCII: segment 1 starts at byte {c}"
    );

    cx.update_window(handle, |_, window, cx| window.input("Z", cx))
        .unwrap();
    assert_eq!(&left_text(cx, &view)[c - 1..c + 2], "xZx");
    render(cx, handle);
    render(cx, handle);
    let shown = segments(cx, &view, 0, true);
    assert_eq!(shown.len(), 301usize.div_ceil(c), "{shown:?}");
    assert!(shown[1].starts_with('Z'), "{shown:?}");
}

#[gpui_kit::test]
async fn thai_text_wraps_between_clusters(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let c = columns(cx, handle, &view);
    // The brief's 50 clusters fit on one line at 66 columns; 150 wrap. Row 1 puts a leading
    // vowel in the last column, where a break would cut it off its consonant (PF14).
    let nam = "น้ำ".repeat(150);
    let thai = format!("{}ไทย{}", "ก".repeat(c - 1), "ก".repeat(20));
    std::fs::write(dir.path().join("b.txt"), format!("{nam}\n{thai}\n")).unwrap();
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    assert!(set_wrap(cx, &view, true));
    render(cx, handle);
    render(cx, handle);

    let shown = segments(cx, &view, 0, false);
    assert_eq!(shown.len(), 150usize.div_ceil(c), "{c} columns: {shown:?}");
    for segment in &shown {
        assert!(segment.starts_with('น'), "{segment:?}");
        assert!(
            !segment.starts_with(['\u{E49}', '\u{E33}']),
            "a mark left its consonant: {segment:?}"
        );
    }
    assert_eq!(shown.concat(), nam);

    let shown = segments(cx, &view, 1, false);
    let before = "ก".repeat(c - 1);
    assert_eq!(shown.first(), Some(&before), "ไ moves down with ท");
    assert!(shown[1].starts_with("ไท"), "{shown:?}");
    assert_eq!(shown.concat(), thai);
}

#[gpui_kit::test]
async fn a_find_hit_on_a_later_segment_is_drawn_there(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, &format!("{}\n", "x".repeat(300)), "y\n");
    wait_loaded(cx, handle, &view).await;
    let c = columns(cx, handle, &view);
    let line = format!("{}FIND{}", "x".repeat(c + 2), "x".repeat(300 - c - 6));
    std::fs::write(dir.path().join("a.txt"), format!("{line}\n")).unwrap();
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    assert!(set_wrap(cx, &view, true));
    render(cx, handle);

    press(cx, handle, "ctrl-f");
    cx.update_window(handle, |_, window, cx| window.input("FIND", cx))
        .unwrap();
    wait_found(cx, handle, &view).await;
    press(cx, handle, "f3");
    assert_eq!(
        view.read_with(cx, |v, _| v.find_status()),
        Some("1 of 1".into())
    );
    render(cx, handle);
    render(cx, handle);
    assert!(
        view.read_with(cx, |v, _| v.half_look_for_test(0, true))
            .is_some_and(|look| look.highlight.is_some())
    );
    let shown = segments(cx, &view, 0, true);
    let layers = view.update(cx, |v, _| v.segment_highlights_for_test(0, true));
    assert_eq!(
        layers.len(),
        shown.len(),
        "one list of highlights a segment"
    );
    // A find hit is the only highlight with an underline.
    let hits = |k: usize| -> Vec<&str> {
        layers[k]
            .iter()
            .filter(|(_, style)| style.underline.is_some())
            .map(|(r, _)| &shown[k][r.clone()])
            .collect()
    };
    assert!(hits(0).is_empty(), "segment 0 holds no hit");
    assert_eq!(hits(1), ["FIND"], "the hit is drawn on segment 1");
}

#[gpui_kit::test]
async fn wrap_is_refused_above_the_row_limit(cx: &mut TestAppContext) {
    let five: String = (1..=5).map(|i| format!("{i}\n")).collect();
    let (dir, handle, view) = open_text(cx, &five, &five);
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, _| v.set_wrap_limit_for_test(2));
    assert!(!set_wrap(cx, &view, true));
    assert!(!wrap_on(cx, &view));
    assert!(!wrapped(cx, &view), "the rows stay in the uniform list");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Wrap works up to 2 rows; this compare has 5.")
    );

    view.update(cx, |v, _| v.set_wrap_limit_for_test(10));
    assert!(set_wrap(cx, &view, true));
    assert!(wrap_on(cx, &view) && wrapped(cx, &view));
    // The left file's 5 lines and 15 more: 20 aligned rows.
    let twenty: String = (1..=20).map(|i| format!("{i}\n")).collect();
    std::fs::write(dir.path().join("b.txt"), twenty).unwrap();
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().aligned_count()),
        20
    );
    assert!(!wrap_on(cx, &view), "the reload grew past the limit");
    assert!(!wrapped(cx, &view));
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Wrap works up to 10 rows; this compare has 20.")
    );
}

#[gpui_kit::test]
async fn page_down_on_wrapped_rows_moves_by_the_rows_on_screen(cx: &mut TestAppContext) {
    let long: String = (0..40).map(|_| format!("{}\n", "x".repeat(300))).collect();
    let (_dir, handle, view) = open_text(cx, &long, &long);
    wait_loaded(cx, handle, &view).await;
    render(cx, handle);
    assert!(set_wrap(cx, &view, true));
    let k = 300usize.div_ceil(columns(cx, handle, &view));
    assert!(k > 1);
    // A screen of one-line rows holds this many: a page step must not skip rows unseen.
    let lines = page(cx, handle, &view);
    press(cx, handle, "pagedown");
    render(cx, handle);
    let top = top_row(cx, handle, &view);
    assert!(
        (1..=lines / k).contains(&top),
        "{k}-line rows, {lines} lines a screen: top {top}"
    );
}

/// PF3: Ctrl+Home is FirstChange and Ctrl+End LastChange.
#[gpui_kit::test]
async fn wrapped_rows_scroll_with_the_overview_and_keys(cx: &mut TestAppContext) {
    let (left, right) = pair_300();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    render(cx, handle);
    assert!(set_wrap(cx, &view, true));
    render(cx, handle);
    view.update(cx, |v, cx| v.scroll_to_fraction(1.0, cx));
    assert_on_screen(cx, handle, &view, 299, "the overview's end");
    press(cx, handle, "ctrl-home");
    assert_eq!(current_block(cx, &view), Some(0));
    assert_on_screen(cx, handle, &view, 40, "Ctrl+Home");
    press(cx, handle, "ctrl-end");
    assert_eq!(current_block(cx, &view), Some(2));
    assert_on_screen(cx, handle, &view, 250, "Ctrl+End");
    assert!(wrapped(cx, &view), "still the wrapped list");
}

/// I1 and M3: wrapped jumps and the 30% line count the rows' real heights. Every row is three
/// lines tall, and rows 40, 150 and 250 differ.
#[gpui_kit::test]
async fn wrapped_jumps_land_on_screen_with_tall_rows(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let line = "x".repeat(3 * columns(cx, handle, &view) - 5);
    let right: String = (0..300)
        .map(|i| match i {
            40 | 150 | 250 => format!("y{}\n", &line[1..]),
            _ => format!("{line}\n"),
        })
        .collect();
    std::fs::write(dir.path().join("a.txt"), format!("{line}\n").repeat(300)).unwrap();
    std::fs::write(dir.path().join("b.txt"), right).unwrap();
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    assert!(set_wrap(cx, &view, true));
    render(cx, handle);
    render(cx, handle);
    assert_eq!(segments(cx, &view, 0, true).len(), 3);

    press(cx, handle, "f8");
    assert_on_screen(cx, handle, &view, 40, "F8");
    press(cx, handle, "f8");
    assert_on_screen(cx, handle, &view, 150, "F8 again");
    press(cx, handle, "ctrl-g");
    cx.update_window(handle, |_, window, cx| {
        window.input("251", cx);
        window.press("enter", cx);
    })
    .unwrap();
    assert_on_screen(cx, handle, &view, 250, "Ctrl+G 251");
    view.update(cx, |v, cx| v.scroll_to_fraction(1.0, cx));
    assert_on_screen(cx, handle, &view, 299, "the overview's end");

    // M3: the 30% line is `k` three-line rows below the top.
    let list = cx
        .update_window(handle, |_, window, _| window.find("text-list").bounds())
        .unwrap();
    let k = (0.3 * f32::from(list.size.height) / (3. * DEFAULT_ROW_HEIGHT)) as usize;
    for (top, block) in [(149 - k, 0), (150 - k, 1)] {
        view.update(cx, |v, cx| v.scroll_like_the_user_for_test(top, cx));
        render(cx, handle);
        render(cx, handle);
        assert_eq!(current_block(cx, &view), Some(block), "top {top}, k {k}");
    }
}

// ---- Display controls and remembered choices (Task 5) ----

use duet_app::text::model::TextFilter;

fn use_settings(cx: &mut TestAppContext) -> tempfile::TempDir {
    let data = tempfile::tempdir().unwrap();
    let data_dir = data.path().to_path_buf();
    cx.update(|cx| cx.set_global(duet_app::app::AppPaths { data_dir }));
    data
}

fn saved_setting(data: &tempfile::TempDir, key: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(data.path().join("settings.json")).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    saved[key].clone()
}

#[gpui_kit::test]
async fn display_choices_are_saved_and_a_new_tab_starts_with_them(cx: &mut TestAppContext) {
    let data = use_settings(cx);
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nc\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.set_tab_width(2, cx);
        v.set_font_size(16., cx);
        v.set_show_spaces(true, cx);
        assert!(v.set_wrap(true, cx));
        v.set_filter(TextFilter::Differences, cx);
    });
    assert_eq!(saved_setting(&data, "text.tab_width"), 2);
    assert_eq!(saved_setting(&data, "text.font_size"), 16.0);
    assert_eq!(saved_setting(&data, "text.show_spaces"), true);
    assert_eq!(saved_setting(&data, "text.wrap"), true);
    assert_eq!(saved_setting(&data, "text.filter"), "differences");
    assert!(
        saved_setting(&data, "text.syntax").is_null(),
        "one key per setter (PF9)"
    );

    let (left, right) = view.read_with(cx, |v, _| {
        (v.request().left.clone(), v.request().right.clone())
    });
    let second = cx
        .update_window(handle, |_, _, cx| {
            cx.new(|cx| TextView::new(left, right, cx))
        })
        .unwrap();
    wait_loaded(cx, handle, &second).await;
    second.read_with(cx, |v, _| {
        assert_eq!(v.display_options().tab_width, 2);
        assert_eq!(v.font_size(), 16.);
        assert!(v.display_options().show_whitespace);
        assert!(v.wrap());
        assert_eq!(v.model().unwrap().filter(), TextFilter::Differences);
        assert_eq!(v.prefs().tab_width, 2);
    });
}

#[gpui_kit::test]
async fn the_tab_width_takes_any_value_from_1_to_16(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "\tx\n", "y\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_tab_width(13, cx));
    let options = view.read_with(cx, |v, _| v.display_options());
    assert_eq!(options.tab_width, 13);
    let line = duet_app::text::display::display_line("\tx", options);
    assert_eq!(line.end_column, 14, "the x is at column 13");
    for (asked, got) in [(0, 1), (40, 16), (1, 1), (16, 16)] {
        view.update(cx, |v, cx| v.set_tab_width(asked, cx));
        assert_eq!(
            view.read_with(cx, |v, _| v.display_options().tab_width),
            got
        );
    }
}

#[gpui_kit::test]
async fn the_wrap_toggle_turns_wrapping_on_and_off(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    for expected in [true, false] {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("text-wrap", cx);
        })
        .unwrap();
        assert_eq!(wrap_on(cx, &view), expected);
    }
}

// ---- The line-details pane (Task 6) ----

fn details_text(
    cx: &mut TestAppContext,
    view: &Entity<TextView>,
) -> Option<duet_app::text::view::DetailsText> {
    view.read_with(cx, |v, _| v.details_text())
}

#[gpui_kit::test]
async fn details_follow_the_selection_then_the_caret_then_the_current_difference(
    cx: &mut TestAppContext,
) {
    let (_dir, handle, view) = open_text(cx, "a\nb c\nd\n", "a\nb\tc\nD\n");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(details_text(cx, &view), None, "off until it is turned on");
    view.update(cx, |v, cx| v.set_details(true, cx));
    assert_eq!(details_text(cx, &view), None, "no row yet");
    press(cx, handle, "f8");
    let d = details_text(cx, &view).expect("the current difference");
    assert_eq!(d.row, 1);
    assert_eq!(d.left_label, "Left 2");
    assert_eq!(d.left.as_deref(), Some("b·c"));
    assert_eq!(d.right_label, "Right 2");
    assert_eq!(d.right.as_deref(), Some("b→  c"));

    view.update(cx, |v, cx| v.select_rows_for_test(false, 2, 2, cx));
    let d = details_text(cx, &view).expect("the selected row");
    assert_eq!(
        (d.row, d.left.as_deref(), d.right.as_deref()),
        (2, Some("d"), Some("D"))
    );

    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    let d = details_text(cx, &view).expect("the caret's row");
    assert_eq!(
        (d.row, d.left.as_deref(), d.right.as_deref()),
        (0, Some("a"), Some("a"))
    );
    assert_eq!(d.left_label, "Left 1");
}

#[gpui_kit::test]
async fn a_side_with_no_line_there_shows_a_dash(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "a\nb\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.set_details(true, cx);
        v.select_rows_for_test(false, 1, 1, cx);
    });
    let d = details_text(cx, &view).expect("row 1");
    assert_eq!(d.left_label, "Left —");
    assert_eq!(d.left, None);
    assert_eq!(d.right_label, "Right 2");
    assert_eq!(d.right.as_deref(), Some("b"));
}

#[gpui_kit::test]
async fn details_off_hides_the_pane_and_the_choice_is_saved(cx: &mut TestAppContext) {
    let data = use_settings(cx);
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nc\n");
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "f8");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("text-details-pane").is_none(), "off");
    })
    .unwrap();
    assert!(!view.read_with(cx, |v, _| v.details()));
    cx.update_window(handle, |_, window, cx| window.click("text-details", cx))
        .unwrap();
    assert!(
        view.read_with(cx, |v, _| v.details()),
        "the toggle turns it on"
    );
    assert_eq!(saved_setting(&data, "text.details"), true);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let pane = window.find("text-details-pane").bounds();
        assert!(pane.size.height > px(0.));
        assert!(
            pane.size.height <= window.viewport_size().height * 0.3 + px(0.5),
            "at most 30% of the view: {pane:?}"
        );
        assert!(window.try_find("text-details-left").is_some());
        assert!(window.try_find("text-details-right").is_some());
    })
    .unwrap();
    view.update(cx, |v, cx| v.set_details(false, cx));
    assert_eq!(saved_setting(&data, "text.details"), false);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("text-details-pane").is_none(), "off again");
    })
    .unwrap();
}

#[gpui_kit::test]
async fn a_long_line_in_the_details_is_shown_whole(cx: &mut TestAppContext) {
    let long = "x".repeat(500);
    let (_dir, handle, view) = open_text(cx, &format!("{long}\n"), "y\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.set_details(true, cx);
        v.select_rows_for_test(true, 0, 0, cx);
    });
    let d = details_text(cx, &view).expect("row 0");
    assert_eq!(d.left.as_deref().map(|t| t.chars().count()), Some(500));
    let row_height = view.read_with(cx, |v, _| v.row_height());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let left = window.find("text-details-left").bounds();
        let right = window.find("text-details-right").bounds();
        assert!(
            f32::from(left.size.height) > row_height,
            "wrapped over several lines: {left:?}"
        );
        assert!(
            left.size.height > right.size.height,
            "{left:?} against {right:?}"
        );
    })
    .unwrap();
}

/// I3: a frame wraps a bounded prefix of a huge line, about (200 + 1) segments x columns x 4
/// bytes, never a copy of the whole 2 MB line; the pane still shows at most 200 segments.
#[gpui_kit::test]
async fn the_details_pane_draws_a_huge_line_from_a_bounded_prefix(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, &format!("{}\n", "x".repeat(2_000_000)), "");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_details(true, cx));
    press(cx, handle, "f8");
    render(cx, handle);
    render(cx, handle);
    let [left, right] = view.read_with(cx, |v, _| v.details_wrapped_for_test());
    assert!((1..=200_000).contains(&left), "left: {left} bytes");
    assert_eq!(right, 0, "the right side has no line there");
    cx.update_window(handle, |_, window, _| {
        let block = window.find("text-details-left").bounds();
        let most = MAX_WRAP_SEGMENTS as f32 * DEFAULT_ROW_HEIGHT;
        assert!(f32::from(block.size.height) <= most + 0.5, "{block:?}");
    })
    .unwrap();
}

#[gpui_kit::test]
async fn n_and_p_move_between_differences_like_f8_and_f7(cx: &mut TestAppContext) {
    let (left, right) = pair_300();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "n");
    assert_eq!(current_block(cx, &view), Some(0));
    press(cx, handle, "n");
    assert_eq!(current_block(cx, &view), Some(1));
    press(cx, handle, "p");
    assert_eq!(current_block(cx, &view), Some(0));
    press(cx, handle, "p");
    assert_eq!(current_block(cx, &view), Some(2), "p from the first wraps");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Wrapped to the last difference.")
    );
}

#[gpui_kit::test]
async fn n_and_p_type_into_the_text_with_a_caret_and_into_fields(cx: &mut TestAppContext) {
    let (left, right) = pair_300();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "f8");
    let before = current_block(cx, &view);
    view.update(cx, |v, cx| v.set_caret_for_test(false, 0, cx));
    press(cx, handle, "n");
    assert!(
        right_text(cx, &view).starts_with("nline 0\n"),
        "n types with a caret"
    );
    assert_eq!(current_block(cx, &view), before, "and moves nothing");
    press(cx, handle, "p");
    assert!(right_text(cx, &view).starts_with("npline 0\n"));
    assert_eq!(current_block(cx, &view), before);

    // A find field keeps its letters as well.
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "ctrl-f");
    press(cx, handle, "p");
    let input = view.read_with(cx, |v, _| v.find_input_for_test().unwrap());
    assert_eq!(input.read_with(cx, |i, _| i.value().to_string()), "p");
    assert_eq!(current_block(cx, &view), None, "no change of block");
}

#[gpui_kit::test]
async fn ctrl_r_and_ctrl_l_copy_the_current_block(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nB\n");
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "f8");
    press(cx, handle, "ctrl-r");
    assert_eq!(right_text(cx, &view), "a\nb\n");
    press(cx, handle, "ctrl-z");
    assert_eq!(right_text(cx, &view), "a\nB\n");
    press(cx, handle, "ctrl-l");
    assert_eq!(left_text(cx, &view), "a\nB\n");
}

#[gpui_kit::test]
async fn the_current_difference_follows_scrolling(cx: &mut TestAppContext) {
    let (left, right) = pair_300();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    for _ in 0..3 {
        render(cx, handle);
    }
    assert_eq!(position(cx, &view), "– / 3", "nothing follows on load");

    for (top, block) in [(150, 1), (260, 2), (0, 0)] {
        view.update(cx, |v, cx| v.scroll_like_the_user_for_test(top, cx));
        render(cx, handle);
        render(cx, handle);
        assert_eq!(current_block(cx, &view), Some(block), "top {top}");
    }

    press(cx, handle, "f8");
    for _ in 0..4 {
        render(cx, handle);
    }
    assert_eq!(
        current_block(cx, &view),
        Some(1),
        "the F8 scroll does not move the current difference back"
    );
    assert_eq!(position(cx, &view), "2 / 3");

    // I4: F8 lands the difference on the 30% line, so reading on keeps it current.
    let (top, page) = (top_row(cx, handle, &view), page(cx, handle, &view));
    for (to, block, what) in [
        (top + 2, 1, "two rows down after F8"),
        (top + 1 - (0.3 * page as f32) as usize, 0, "back up past it"),
    ] {
        view.update(cx, |v, cx| v.scroll_like_the_user_for_test(to, cx));
        render(cx, handle);
        render(cx, handle);
        assert_eq!(current_block(cx, &view), Some(block), "{what}: top {to}");
    }
}

fn position(cx: &mut TestAppContext, view: &Entity<TextView>) -> String {
    view.read_with(cx, |v, _| duet_app::text::view::position_text(v.model()))
}

#[gpui_kit::test]
async fn nothing_follows_under_the_same_filter(cx: &mut TestAppContext) {
    let (left, right) = pair_300();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "f8");
    press(cx, handle, "ctrl-4");
    for _ in 0..4 {
        render(cx, handle);
    }
    let before = current_block(cx, &view);
    view.update(cx, |v, cx| v.scroll_like_the_user_for_test(260, cx));
    render(cx, handle);
    render(cx, handle);
    assert_eq!(current_block(cx, &view), before);
}

/// C1: Differences on an identical pair shows no rows; the list then clamps its top to 0,
/// which must not count as a scroll to follow.
#[gpui_kit::test]
async fn following_survives_a_filter_with_no_rows(cx: &mut TestAppContext) {
    let same: String = (0..300).map(|i| format!("line {i}\n")).collect();
    for list in [false, true] {
        let (_dir, handle, view) = open_text(cx, &same, &same);
        wait_loaded(cx, handle, &view).await;
        view.update(cx, |v, cx| v.set_list_rows_for_test(list, cx));
        render(cx, handle);
        view.update(cx, |v, cx| v.scroll_like_the_user_for_test(100, cx));
        render(cx, handle);
        render(cx, handle);
        press(cx, handle, "ctrl-2");
        for _ in 0..3 {
            render(cx, handle);
        }
        assert_eq!(current_block(cx, &view), None, "wrapped list {list}");
        assert_eq!(position(cx, &view), "– / 0", "wrapped list {list}");
    }
}

fn rows_of(cx: &mut TestAppContext, view: &Entity<TextView>) -> Option<std::ops::Range<u32>> {
    view.read_with(cx, |v, _| v.row_selection().map(|s| s.rows()))
}

#[gpui_kit::test]
async fn up_and_down_move_a_row_selection_and_shift_extends_it(cx: &mut TestAppContext) {
    let (left, right) = pair_300();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select_rows_for_test(false, 2, 2, cx));
    press(cx, handle, "down");
    assert_eq!(rows_of(cx, &view), Some(3..4));
    press(cx, handle, "shift-down");
    assert_eq!(rows_of(cx, &view), Some(3..5));
    press(cx, handle, "up");
    assert_eq!(
        rows_of(cx, &view),
        Some(3..4),
        "Up collapses to the moved head"
    );

    view.update(cx, |v, cx| v.select_rows_for_test(false, 39, 39, cx));
    press(cx, handle, "down");
    assert_eq!(rows_of(cx, &view), Some(40..41));
    assert_eq!(
        current_block(cx, &view),
        Some(0),
        "row 40 is in the first difference"
    );

    press(cx, handle, "escape");
    assert_eq!(rows_of(cx, &view), None);
    let top = top_row(cx, handle, &view);
    press(cx, handle, "down");
    assert_eq!(top_row(cx, handle, &view), top + 1, "no selection: scrolls");
}

#[gpui_kit::test]
async fn the_wheel_over_the_overview_and_the_gutter_scrolls_the_rows(cx: &mut TestAppContext) {
    let (left, right) = pair_300();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    render(cx, handle);
    render(cx, handle);
    let centre = cx
        .update_window(handle, |_, window, _| {
            window.find("text-overview").bounds().center()
        })
        .unwrap();
    wheel(cx, handle, centre, -200.);
    let after_overview = top_row(cx, handle, &view);
    assert!(after_overview >= 9, "overview wheel: top {after_overview}");

    let gutter = cx
        .update_window(handle, |_, window, _| {
            window
                .find(("text-gutter", after_overview + 3))
                .bounds()
                .center()
        })
        .unwrap();
    wheel(cx, handle, gutter, -200.);
    let after_gutter = top_row(cx, handle, &view);
    assert!(
        after_gutter > after_overview,
        "gutter wheel: top {after_gutter}"
    );
}

const MOVED_LEFT: &str = "a\nfn one()\nfn two()\n}\nb\nc\nd\ne\nf\n";
const MOVED_RIGHT: &str = "a\nb\nc\nd\ne\nf\nfn one()\nfn two()\n}\n";

/// The display row that shows `line` of one side.
fn display_of_line(
    cx: &mut TestAppContext,
    view: &Entity<TextView>,
    left: bool,
    line: u32,
) -> usize {
    view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        m.visible()
            .display_of(m.row_of_line(left, line).unwrap())
            .unwrap()
    })
}

#[gpui_kit::test]
async fn a_moved_line_names_its_partner_and_a_click_goes_there(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, MOVED_LEFT, MOVED_RIGHT);
    wait_loaded(cx, handle, &view).await;
    let left_row = display_of_line(cx, &view, true, 1);
    let right_row = display_of_line(cx, &view, false, 6);
    render(cx, handle);
    cx.update_window(handle, |_, window, _| {
        assert_eq!(
            window.find(("text-moved-left", left_row)).label(),
            Some("↷ 7")
        );
        assert_eq!(
            window.find(("text-moved-right", right_row)).label(),
            Some("↶ 2")
        );
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click(("text-moved-left", left_row), cx)
    })
    .unwrap();
    render(cx, handle);
    render(cx, handle);
    let (marked, caret, row) = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (v.marked_row(), v.caret(), m.row_of_line(false, 6))
    });
    assert_eq!(marked, row, "the partner's row is marked");
    assert!(caret.is_none(), "the click placed no caret");
    cx.update_window(handle, |_, window, _| {
        let list = window.find("text-list").bounds();
        let shown = window.find(("text-row", right_row)).bounds();
        assert!(
            list.contains(&shown.center()),
            "the partner is on screen: {shown:?} in {list:?}"
        );
    })
    .unwrap();
}

/// Left `"moved line\na\nb\nc\n"`, right the same with the first line last: the left tag is on
/// display row 0, and left lines 1 to 3 are display rows 1 to 3.
const DRAG_LEFT: &str = "moved line\na\nb\nc\n";
const DRAG_RIGHT: &str = "a\nb\nc\nmoved line\n";

/// Presses the left tag of row 0, moves 30 px down (past the 4 px that make a drag), then
/// onto `(display, fraction down the row, x offset into the row)`, without releasing.
fn drag_tag_to(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    (display, fraction, x): (usize, f32, f32),
) -> gpui_kit::Point<gpui_kit::Pixels> {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let tag = window.find(("text-moved-left", 0usize)).bounds().center();
        pointer(window, move_to(tag, false), cx);
        pointer(window, button(tag, true), cx);
        pointer(window, move_to(tag + point(px(0.), px(30.)), true), cx);
        let row = window.find(("text-row", display)).bounds();
        let target = point(row.left() + px(x), row.top() + row.size.height * fraction);
        pointer(window, move_to(target, true), cx);
        target
    })
    .unwrap()
}

fn release(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    at: gpui_kit::Point<gpui_kit::Pixels>,
) {
    cx.update_window(handle, |_, window, cx| {
        pointer(window, button(at, false), cx)
    })
    .unwrap();
}

#[gpui_kit::test]
async fn dragging_a_moved_lines_tag_moves_the_line_as_one_undo_step(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, DRAG_LEFT, DRAG_RIGHT);
    wait_loaded(cx, handle, &view).await;
    let target = drag_tag_to(cx, handle, (3, 0.75, 200.));
    assert_eq!(
        view.read_with(cx, |v, _| v.move_drag_for_test()),
        Some((true, 0))
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let row = window.find(("text-row", 3usize)).bounds();
        let marker = window.find("text-move-marker").bounds();
        assert_eq!(marker.size.height, px(2.));
        assert!(
            (marker.center().y - row.bottom()).abs() <= px(1.),
            "the marker lies under the row: {marker:?} {row:?}"
        );
        assert!(marker.right() < row.center().x, "on the line's own half");
    })
    .unwrap();
    release(cx, handle, target);
    assert_eq!(left_text(cx, &view), "a\nb\nc\nmoved line\n");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().block_count()),
        0,
        "the two sides match after the re-diff"
    );
    assert!(view.read_with(cx, |v, _| v.model().unwrap().is_dirty(true)));
    press(cx, handle, "ctrl-z");
    assert_eq!(left_text(cx, &view), "moved line\na\nb\nc\n");
}

#[gpui_kit::test]
async fn a_drop_on_the_other_side_or_the_same_place_moves_nothing(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, DRAG_LEFT, DRAG_RIGHT);
    wait_loaded(cx, handle, &view).await;
    let untouched = |cx: &mut TestAppContext| {
        view.read_with(cx, |v, _| {
            let m = v.model().unwrap();
            !m.is_dirty(true) && !m.is_dirty(false)
        })
    };
    let target = drag_tag_to(cx, handle, (2, 0.75, 900.));
    release(cx, handle, target);
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Drop the line on its own side.")
    );
    assert!(untouched(cx));
    let target = drag_tag_to(cx, handle, (0, 0.25, 200.));
    release(cx, handle, target);
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("The line is there already.")
    );
    assert!(untouched(cx));
    assert_eq!(view.read_with(cx, |v, _| v.move_drag_for_test()), None);
}

#[gpui_kit::test]
async fn escape_cancels_a_move_drag(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, DRAG_LEFT, DRAG_RIGHT);
    wait_loaded(cx, handle, &view).await;
    let target = drag_tag_to(cx, handle, (3, 0.75, 200.));
    assert!(view.read_with(cx, |v, _| v.move_drag_for_test()).is_some());
    press(cx, handle, "escape");
    assert_eq!(view.read_with(cx, |v, _| v.move_drag_for_test()), None);
    release(cx, handle, target);
    let changed = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (m.is_dirty(true), m.is_dirty(false), v.marked_row())
    });
    assert_eq!(changed, (false, false, None), "the release did nothing");
    assert_eq!(notice(cx, &view), None);
}

#[gpui_kit::test]
async fn the_binary_message_offers_open_as_text(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\0b\nc\n", "a\0c\nc\n");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.message().map(str::to_owned))
            .as_deref(),
        Some("Binary files differ.")
    );
    render(cx, handle);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.try_find("text-open-as-text").is_some());
        window.click("text-open-as-text", cx);
    })
    .unwrap();
    wait_loaded(cx, handle, &view).await;
    let (shown, message, rows) = view.read_with(cx, |v, _| {
        (
            v.model().is_some(),
            v.message().map(str::to_owned),
            v.model().map_or(0, |m| m.row_count()),
        )
    });
    assert!(shown, "the pair is a text compare now");
    assert_eq!(message, None);
    assert_eq!(rows, 2);
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    assert!(
        view.read_with(cx, |v, _| v.model().is_some()),
        "a reload keeps the text view"
    );
}

#[gpui_kit::test]
async fn a_text_view_without_an_origin_has_no_back_button(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    render(cx, handle);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("text-back").is_none());
    })
    .unwrap();
    view.update(cx, |v, cx| v.set_has_origin(true, cx));
    render(cx, handle);
    assert!(view.read_with(cx, |v, _| v.has_origin()));
    let back = Rc::new(Cell::new(0));
    let seen = back.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &TextEvent, _| {
            if matches!(event, TextEvent::Back) {
                seen.set(seen.get() + 1);
            }
        })
        .detach()
    });
    cx.update_window(handle, |_, window, cx| window.click("text-back", cx))
        .unwrap();
    assert_eq!(back.get(), 1);
}

fn open_untitled(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<TextView>) {
    open_test_window(cx, |window, cx| {
        let view = cx.new(|cx| TextView::new("".into(), "".into(), cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    })
}

#[gpui_kit::test]
async fn an_untitled_pair_compares_what_is_typed_and_pasted(cx: &mut TestAppContext) {
    let (handle, view) = open_untitled(cx);
    wait_loaded(cx, handle, &view).await;
    render(cx, handle);
    assert!(view.read_with(cx, |v, _| v.model().is_some() && v.message().is_none()));
    assert_eq!(view.read_with(cx, |v, _| v.model().unwrap().row_count()), 0);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("text-empty-left").is_some());
        assert!(window.try_find("text-empty-right").is_some());
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| window.click("text-empty-left", cx))
        .unwrap();
    let caret = view.read_with(cx, |v, _| v.caret()).expect("a caret");
    assert!(caret.left && caret.head == 0 && caret.anchor == 0);
    cx.update_window(handle, |_, window, cx| window.input("a", cx))
        .unwrap();
    press(cx, handle, "enter");
    cx.update_window(handle, |_, window, cx| window.input("b", cx))
        .unwrap();
    assert_eq!(left_text(cx, &view), "a\nb");
    cx.update(|cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("a\nc\n".into())));
    view.update(cx, |v, cx| {
        assert!(v.replace_side_with_clipboard(false, cx))
    });
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().block_count()),
        1
    );
    assert!(view.read_with(cx, |v, _| v.is_untitled(true) && v.is_untitled(false)));
    assert!(view.read_with(cx, |v, _| v.is_dirty()));
}

#[gpui_kit::test]
async fn saving_an_untitled_side_asks_where_and_the_side_follows_the_file(cx: &mut TestAppContext) {
    let (handle, view) = open_untitled(cx);
    wait_loaded(cx, handle, &view).await;
    let renamed = Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = renamed.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &TextEvent, _| {
            if let TextEvent::Renamed { left, right } = event {
                seen.borrow_mut().push((left.clone(), right.clone()));
            }
        })
        .detach()
    });
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("x.txt");
    press(cx, handle, "ctrl-s");
    assert!(cx.did_prompt_for_new_path(), "an untitled side asks where");
    let chosen = target.clone();
    cx.simulate_new_path_selection(move |_| Some(chosen));
    let v = view.clone();
    cx.wait_for(handle, std::time::Duration::from_secs(20), move |_, cx| {
        !v.read(cx).is_untitled(true) && !v.read(cx).is_dirty()
    })
    .await;
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "x");
    assert_eq!(
        renamed.borrow().as_slice(),
        [(target, std::path::PathBuf::new())]
    );
    assert!(view.read_with(cx, |v, _| v.is_untitled(false)));
}

#[gpui_kit::test]
async fn ctrl_v_on_an_empty_pair_pastes_into_the_left_side(cx: &mut TestAppContext) {
    let (handle, view) = open_untitled(cx);
    wait_loaded(cx, handle, &view).await;
    render(cx, handle);
    assert!(view.read_with(cx, |v, _| v.caret().is_none()));
    cx.update(|cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("p\nq".into())));
    press(cx, handle, "ctrl-v");
    assert_eq!(left_text(cx, &view), "p\nq");
}

#[gpui_kit::test]
async fn saving_both_untitled_sides_asks_one_at_a_time(cx: &mut TestAppContext) {
    let (handle, view) = open_untitled(cx);
    wait_loaded(cx, handle, &view).await;
    for (left, text) in [(true, "l"), (false, "r")] {
        view.update(cx, |v, cx| v.set_caret_for_test(left, 0, cx));
        cx.update_window(handle, |_, window, cx| window.input(text, cx))
            .unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    let saved = cx
        .update_window(handle, |_, window, cx| {
            view.update(cx, |v, cx| v.save_all(window, cx))
        })
        .unwrap();
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path(), "the first side asks");
    let first = a.clone();
    cx.simulate_new_path_selection(move |_| Some(first));
    let v = view.clone();
    cx.wait_for(handle, std::time::Duration::from_secs(20), move |_, cx| {
        !v.read(cx).is_untitled(true)
    })
    .await;
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path(), "then the second side asks");
    let second = b.clone();
    cx.simulate_new_path_selection(move |_| Some(second));
    assert!(saved.await);
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "l");
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "r");
    assert!(!view.read_with(cx, |v, _| v.is_dirty()));
}

/// M11: two dirty untitled sides are named once in the prompt.
#[gpui_kit::test]
async fn the_prompt_names_two_untitled_sides_once(cx: &mut TestAppContext) {
    let (handle, view) = open_untitled(cx);
    wait_loaded(cx, handle, &view).await;
    for left in [true, false] {
        view.update(cx, |v, cx| v.set_caret_for_test(left, 0, cx));
        cx.update_window(handle, |_, window, cx| window.input("t", cx))
            .unwrap();
    }
    let answer = cx
        .update_window(handle, |_, window, cx| {
            view.update(cx, |v, cx| v.confirm_discard(window, cx))
        })
        .unwrap();
    let (message, _) = cx.pending_prompt().expect("a prompt");
    assert_eq!(message, "Save changes to both untitled sides?");
    cx.simulate_prompt_answer("Cancel");
    assert!(!answer.await);
}
