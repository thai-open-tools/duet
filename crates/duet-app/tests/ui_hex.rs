//! Headless UI tests for the hex view and its tab.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use duet_app::hex::view::{ByteMark, HexEvent, HexView};
use duet_app::launch::Launch;
use duet_app::workspace::{TabItem, Workspace};
use duet_core::hex::HexSource;
use duet_core::hex::format::hex_line;
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, ClipboardItem, Entity, Focusable, InputEvent as _,
    Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, Render, ScrollDelta,
    ScrollWheelEvent, TestAppContext, Window, WindowBounds, WindowOptions, point, px, size,
};

fn open_test_window<V: Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        duet_app::app::configure_theme(gpui_kit::component::ThemeMode::Light, cx);
        cx.bind_keys(duet_app::app::key_bindings());
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1200.), px(800.)),
        };
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            build,
        )
        .expect("open test window")
    })
}

/// 64 bytes of zeros; `marks` are set to 0xFF.
fn bytes_with(marks: &[usize]) -> Vec<u8> {
    let mut bytes = vec![0u8; 64];
    for &at in marks {
        bytes[at] = 0xFF;
    }
    bytes
}

/// The two files differ at offsets 3 and 40: eight or more bytes apart, so two blocks.
fn write_pair(dir: &std::path::Path) -> (PathBuf, PathBuf) {
    let (a, b) = (dir.join("a.bin"), dir.join("b.bin"));
    std::fs::write(&a, bytes_with(&[])).unwrap();
    std::fs::write(&b, bytes_with(&[3, 40])).unwrap();
    (a, b)
}

fn open_hex(
    cx: &mut TestAppContext,
    left: &[u8],
    right: &[u8],
) -> (tempfile::TempDir, AnyWindowHandle, Entity<HexView>) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.bin"), dir.path().join("b.bin"));
    std::fs::write(&a, left).unwrap();
    std::fs::write(&b, right).unwrap();
    let (handle, view) = open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| HexView::new(a, b, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    (dir, handle, view)
}

/// A hex view over two given sources, such as synthetic ones of any length.
fn open_hex_sources(
    cx: &mut TestAppContext,
    left: HexSource,
    right: HexSource,
) -> (AnyWindowHandle, Entity<HexView>) {
    open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| HexView::with_sources_for_test(left, right, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    })
}

async fn wait_hex(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<HexView>) {
    let view = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !view.read(cx).is_busy()
    })
    .await;
}

fn press(cx: &mut TestAppContext, handle: AnyWindowHandle, key: &str) {
    cx.update_window(handle, |_, window, cx| window.press(key, cx))
        .unwrap();
}

#[gpui_kit::test]
async fn the_view_compares_a_pair_in_the_background(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_hex(cx, &bytes_with(&[]), &bytes_with(&[3, 40]));
    assert!(
        view.read_with(cx, |v, _| v.is_busy()),
        "mapping and diffing take a job"
    );
    wait_hex(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let diff = v.diff().expect("compared");
        assert_eq!(diff.ranges, [3..4, 40..41]);
        assert_eq!(v.message(), None);
    });
}

#[gpui_kit::test]
async fn a_hex_tab_opens_compares_and_badges(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = write_pair(dir.path());
    let (handle, workspace) = open_test_window(cx, move |window, cx| {
        cx.new(|cx| Workspace::new(Launch::Hex { left: a, right: b }, window, cx))
    });
    let ws = workspace.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        ws.read(cx).active_idle(cx)
    })
    .await;
    let view = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Hex(view)) => view.clone(),
        _ => panic!("expected a hex tab"),
    });
    assert_eq!(view.read_with(cx, |v, _| v.diff().unwrap().ranges.len()), 2);
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        Some("2".to_owned())
    );
}

#[gpui_kit::test]
async fn an_identical_pair_badges_ok(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.bin"), dir.path().join("b.bin"));
    std::fs::write(&a, bytes_with(&[5])).unwrap();
    std::fs::write(&b, bytes_with(&[5])).unwrap();
    let (handle, workspace) = open_test_window(cx, move |window, cx| {
        cx.new(|cx| Workspace::new(Launch::Hex { left: a, right: b }, window, cx))
    });
    let ws = workspace.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        ws.read(cx).active_idle(cx)
    })
    .await;
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        Some("ok".to_owned())
    );
}

#[gpui_kit::test]
async fn a_vanished_file_shows_the_open_error(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.bin"), dir.path().join("gone.bin"));
    std::fs::write(&a, bytes_with(&[])).unwrap();
    let missing = b.clone();
    let (handle, view) = open_test_window(cx, move |window, cx| {
        cx.new(|cx| HexView::new(a, b, window, cx))
    });
    wait_hex(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let message = v.message().expect("an error message");
        assert!(
            message.starts_with(&format!("Can't open {}: ", missing.display())),
            "{message}"
        );
        assert!(message.ends_with('.'), "{message}");
        assert_eq!(v.message_hint(), Some("Press F5 to try again."));
        assert!(v.diff().is_none());
    });
}

#[gpui_kit::test]
async fn reload_after_the_file_shrank_shows_the_new_size(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_hex(cx, &bytes_with(&[]), &bytes_with(&[3, 40]));
    wait_hex(cx, handle, &view).await;
    let right = dir.path().join("b.bin");
    view.update(cx, |v, cx| {
        // Nothing is locked: the file shrinks under the open tab.
        std::fs::write(&right, &bytes_with(&[3])[..20]).unwrap();
        v.reload(cx);
    });
    wait_hex(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let diff = v.diff().expect("compared again");
        assert_eq!(
            diff.ranges,
            [3..4, 20..64],
            "the longer left side adds a tail"
        );
        assert_eq!(diff.diff_bytes, 45);
    });
}

#[gpui_kit::test]
async fn escape_asks_to_go_back(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_hex(cx, &bytes_with(&[]), &bytes_with(&[3]));
    wait_hex(cx, handle, &view).await;
    let log = Rc::new(RefCell::new(Vec::new()));
    let seen = log.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &HexEvent, _| {
            seen.borrow_mut().push(matches!(event, HexEvent::Back));
        })
        .detach();
    });
    press(cx, handle, "escape");
    assert_eq!(*log.borrow(), [true]);
}

#[gpui_kit::test]
async fn the_badge_waits_for_the_compare(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = write_pair(dir.path());
    let (handle, workspace) = open_test_window(cx, move |window, cx| {
        cx.new(|cx| Workspace::new(Launch::Hex { left: a, right: b }, window, cx))
    });
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        None,
        "nothing is shown while the files are mapped and compared"
    );
    let ws = workspace.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        ws.read(cx).active_idle(cx)
    })
    .await;
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        Some("2".to_owned())
    );
}

fn top_row(cx: &mut TestAppContext, view: &Entity<HexView>) -> u64 {
    view.read_with(cx, |v, _| v.top_row())
}

/// (row count, rows that fit the body) after a fresh frame.
fn page(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<HexView>) -> (u64, u64) {
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    view.read_with(cx, |v, _| (v.row_count(), v.visible_rows()))
}

fn wheel(cx: &mut TestAppContext, handle: AnyWindowHandle, lines: f32) {
    cx.update_window(handle, |_, window, cx| {
        window.scroll("hex-rows", ScrollDelta::Lines(point(0., lines)), cx)
    })
    .unwrap();
}

#[gpui_kit::test]
async fn viewport_reaches_the_last_row_of_a_4_gib_pair(cx: &mut TestAppContext) {
    let (handle, view) = open_hex_sources(
        cx,
        HexSource::synthetic(4 << 30, 0),
        HexSource::synthetic(4 << 30, 0),
    );
    let last = view.read_with(cx, |v, _| v.row_count() - 1);
    assert_eq!(last, (4u64 << 30) / 16 - 1, "16 bytes per row by default");
    view.update(cx, |v, cx| v.scroll_to_row(u64::MAX, cx));
    let (top, visible) = view.read_with(cx, |v, _| (v.top_row(), v.visible_rows()));
    assert_eq!(
        top + visible - 1,
        last,
        "the last page is full and ends on the last row"
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("hex-row", 0usize)).is_some());
    })
    .unwrap();
    // Once a frame has measured the body, the last page still ends on the last row.
    let (rows, visible) = page(cx, handle, &view);
    assert!(visible > 10, "a page is many rows, got {visible}");
    view.update(cx, |v, cx| v.scroll_to_row(u64::MAX, cx));
    assert_eq!(top_row(cx, &view) + visible, rows);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let id = ("hex-row", visible as usize - 1);
        assert!(window.try_find(id).is_some(), "the last row is drawn");
        assert!(
            window.try_find(("hex-row", visible as usize)).is_none(),
            "nothing past the last row"
        );
    })
    .unwrap();
    // Close the tab: the update's flush releases the view, whose job slot cancels the 8 GiB
    // background compare before the harness would run it at teardown.
    drop(view);
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
}

#[gpui_kit::test]
async fn differing_bytes_and_the_short_files_tail_are_marked(cx: &mut TestAppContext) {
    // left "abcd", right "abXdEF": byte 2 differs, bytes 4..6 exist only on the right
    let (_dir, handle, view) = open_hex(cx, b"abcd", b"abXdEF");
    wait_hex(cx, handle, &view).await;
    let marks = view.read_with(cx, |v, _| v.row_marks_for_test(0));
    assert_eq!(marks.left[2], ByteMark::Differs);
    assert_eq!(marks.left[4], ByteMark::Missing);
    assert_eq!(marks.right[5], ByteMark::Differs);
    assert_eq!(marks.left[0], ByteMark::Same);
    assert_eq!(marks.right[2], ByteMark::Differs);
    assert_eq!(
        (marks.left.len(), marks.right.len()),
        (6, 6),
        "a row stops at the longer file's end"
    );
}

#[gpui_kit::test]
async fn differences_only_maps_rows_through_the_ranges(cx: &mut TestAppContext) {
    // Differences at 3 and 40: rows 0 and 2 of 4.
    let (_dir, handle, view) = open_hex(cx, &bytes_with(&[]), &bytes_with(&[3, 40]));
    wait_hex(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.row_count()), 4);
    view.update(cx, |v, cx| v.set_only_differences(true, cx));
    assert_eq!(view.read_with(cx, |v, _| v.row_count()), 2);
    let marks = view.read_with(cx, |v, _| v.row_marks_for_test(1));
    assert_eq!(marks.right[8], ByteMark::Differs, "display row 1 is row 2");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("hex-row", 1usize)).is_some());
        assert!(window.try_find(("hex-row", 2usize)).is_none());
    })
    .unwrap();
    view.update(cx, |v, cx| v.set_only_differences(false, cx));
    assert_eq!(view.read_with(cx, |v, _| v.row_count()), 4);
}

#[gpui_kit::test]
async fn wheel_and_the_scrollbar_move_the_rows(cx: &mut TestAppContext) {
    let (handle, view) = open_hex_sources(
        cx,
        HexSource::synthetic(1 << 20, 0),
        HexSource::synthetic(1 << 20, 0),
    );
    wait_hex(cx, handle, &view).await;
    let (rows, visible) = page(cx, handle, &view);
    assert!(visible > 10, "a page is many rows, got {visible}");
    wheel(cx, handle, -3.);
    assert_eq!(top_row(cx, &view), 3, "three lines down");
    wheel(cx, handle, 1.);
    assert_eq!(top_row(cx, &view), 2, "one line up");
    wheel(cx, handle, -0.2);
    assert_eq!(top_row(cx, &view), 3, "a small notch still moves one row");
    wheel(cx, handle, 5.);
    assert_eq!(top_row(cx, &view), 0, "no further up");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let bar = window.find("hex-vscroll").bounds();
        let x = bar.center().x;
        window.drag(
            point(x, bar.top() + px(6.)),
            point(x, bar.bottom() - px(2.)),
            cx,
        );
    })
    .unwrap();
    assert_eq!(
        top_row(cx, &view),
        rows - visible,
        "dragging the thumb to the bottom reaches the last page"
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let bar = window.find("hex-vscroll").bounds();
        let x = bar.center().x;
        window.drag(
            point(x, bar.bottom() - px(6.)),
            point(x, bar.top() + bar.size.height / 2.),
            cx,
        );
    })
    .unwrap();
    let top = top_row(cx, &view);
    assert!(
        (rows * 2 / 5..rows * 3 / 5).contains(&top),
        "the thumb half way down the track shows the middle rows, got {top} of {rows}"
    );
}

#[gpui_kit::test]
async fn clicking_the_overview_jumps_there(cx: &mut TestAppContext) {
    let (handle, view) = open_hex_sources(
        cx,
        HexSource::synthetic(1 << 20, 0),
        HexSource::synthetic(1 << 20, 0),
    );
    wait_hex(cx, handle, &view).await;
    let (rows, visible) = page(cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let bar = window.find("hex-overview").bounds();
        let at = point(bar.size.width / 2., bar.size.height / 2.);
        window.click_at("hex-overview", at, cx);
    })
    .unwrap();
    let top = top_row(cx, &view);
    assert!(
        top.abs_diff(rows / 2) <= visible,
        "half way down the strip is half way down the rows, got {top} of {rows}"
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let bar = window.find("hex-overview").bounds();
        let x = bar.center().x;
        window.drag(
            point(x, bar.center().y),
            point(x, bar.bottom() + px(20.)),
            cx,
        );
    })
    .unwrap();
    assert_eq!(
        top_row(cx, &view),
        rows - visible,
        "dragging past the strip's end reaches the last page"
    );
}

#[gpui_kit::test]
async fn bytes_per_row_changes_keep_the_top_byte_and_are_remembered(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let data_dir = data.path().to_path_buf();
    cx.update(|cx| cx.set_global(duet_app::app::AppPaths { data_dir }));
    let (handle, view) = open_hex_sources(
        cx,
        HexSource::synthetic(1 << 20, 0),
        HexSource::synthetic(1 << 20, 0),
    );
    wait_hex(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.bytes_per_row()), 16);
    view.update(cx, |v, cx| v.scroll_to_row(0x400 / 16, cx));
    view.update(cx, |v, cx| v.set_bytes_per_row(32, cx));
    let (top, bpr, rows) =
        view.read_with(cx, |v, _| (v.top_row(), v.bytes_per_row(), v.row_count()));
    assert_eq!((bpr, rows), (32, (1 << 20) / 32));
    assert!(
        top * 32 <= 0x400 && 0x400 < (top + 1) * 32,
        "byte 0x400 stays on the top row, got row {top}"
    );
    let settings = data.path().join("settings.json");
    assert_eq!(
        duet_app::settings::get_u64(&settings, "hex.bytes_per_row"),
        Some(32)
    );
    view.update(cx, |v, cx| v.set_bytes_per_row(12, cx));
    assert_eq!(
        view.read_with(cx, |v, _| v.bytes_per_row()),
        32,
        "only 8, 16 and 32 are offered"
    );
    view.update(cx, |v, cx| v.set_bytes_per_row(8, cx));
    let top = top_row(cx, &view);
    assert!(top * 8 <= 0x400 && 0x400 < (top + 1) * 8, "row {top}");
    view.update(cx, |v, cx| v.set_bytes_per_row(32, cx));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let again = cx
        .update_window(handle, |_, window, cx| {
            cx.new(|cx| {
                HexView::with_sources_for_test(
                    HexSource::synthetic(64, 0),
                    HexSource::synthetic(64, 0),
                    window,
                    cx,
                )
            })
        })
        .unwrap();
    assert_eq!(
        again.read_with(cx, |v, _| v.bytes_per_row()),
        32,
        "the next hex view starts with the remembered width"
    );
}

/// The rows area, the first row and the ruler's cells after a fresh frame.
fn row_bounds(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
) -> (Bounds<Pixels>, Bounds<Pixels>, Option<Bounds<Pixels>>) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        (
            window.find("hex-rows").bounds(),
            window.find(("hex-row", 0usize)).bounds(),
            window.try_find("hex-ruler-cells").map(|r| r.bounds()),
        )
    })
    .unwrap()
}

#[gpui_kit::test]
async fn the_right_side_scrolls_into_view_when_rows_are_wider_than_the_window(
    cx: &mut TestAppContext,
) {
    let (handle, view) = open_hex_sources(
        cx,
        HexSource::synthetic(1 << 20, 0),
        HexSource::synthetic(1 << 20, 1),
    );
    wait_hex(cx, handle, &view).await;
    let (rows, row, _) = row_bounds(cx, handle);
    assert!(row.right() <= rows.right(), "16 bytes per row fit 1200 px");
    cx.update_window(handle, |_, window, _| {
        assert!(
            window.try_find("hex-hscroll").is_none(),
            "nothing to scroll"
        );
    })
    .unwrap();

    view.update(cx, |v, cx| v.set_bytes_per_row(32, cx));
    let (rows, row, _) = row_bounds(cx, handle);
    assert!(
        row.right() > rows.right() + px(100.),
        "32 bytes per row are wider than the window: {row:?} in {rows:?}"
    );
    assert_eq!(row.left(), rows.left());

    // A sideways wheel scrolls right, and the ruler moves with the rows.
    cx.update_window(handle, |_, window, cx| {
        window.scroll("hex-rows", ScrollDelta::Lines(point(-3., 0.)), cx)
    })
    .unwrap();
    let (rows, row, ruler) = row_bounds(cx, handle);
    let moved = rows.left() - row.left();
    assert!(moved > px(0.), "the rows moved left");
    assert_eq!(
        ruler.map(|r| r.left()),
        Some(row.left()),
        "the ruler follows"
    );

    // So does Shift with the vertical wheel.
    cx.update_window(handle, |_, window, cx| {
        window.hover("hex-rows", cx);
        let position = window.find("hex-rows").bounds().center();
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        window.dispatch_event(
            ScrollWheelEvent {
                position,
                delta: ScrollDelta::Lines(point(0., -3.)),
                modifiers: shift,
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
    })
    .unwrap();
    let (rows, row, _) = row_bounds(cx, handle);
    assert!(
        rows.left() - row.left() > moved,
        "Shift+wheel scrolls sideways"
    );
    assert_eq!(top_row(cx, &view), 0, "and not down");

    // Dragging the horizontal scrollbar to its end brings the right ASCII column in.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let bar = window.find("hex-hscroll").bounds();
        let y = bar.center().y;
        window.drag(
            point(bar.right() - px(2.), y),
            point(bar.right() + px(40.), y),
            cx,
        );
    })
    .unwrap();
    let (rows, row, ruler) = row_bounds(cx, handle);
    assert!(
        row.right() <= rows.right() && row.right() > rows.right() - px(20.),
        "the row's end is just inside the rows area: {row:?} in {rows:?}"
    );
    assert_eq!(ruler.map(|r| r.left()), Some(row.left()));

    // Fewer bytes per row fit again: the offset clamps back.
    view.update(cx, |v, cx| v.set_bytes_per_row(16, cx));
    let (rows, row, ruler) = row_bounds(cx, handle);
    assert_eq!(row.left(), rows.left());
    assert_eq!(ruler.map(|r| r.left()), Some(rows.left()));
}

#[gpui_kit::test]
async fn the_top_byte_waits_for_the_ranges_in_differences_only(cx: &mut TestAppContext) {
    // 64 KiB a side, one difference every KiB: 64 of the 4,096 rows differ.
    let left = vec![0u8; 64 << 10];
    let mut right = left.clone();
    for at in (0..right.len()).step_by(1024) {
        right[at] = 1;
    }
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    page(cx, handle, &view);
    view.update(cx, |v, cx| {
        v.set_only_differences(true, cx);
        v.scroll_to_row(8, cx);
    });
    assert_eq!(top_row(cx, &view), 8, "the row of byte 0x2000");

    // F5: until the ranges are back, the body says so instead of an empty page at the top.
    press(cx, handle, "f5");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("hex-placeholder").is_some());
        assert!(window.try_find(("hex-row", 0usize)).is_none());
    })
    .unwrap();
    wait_hex(cx, handle, &view).await;
    assert_eq!(top_row(cx, &view), 8, "byte 0x2000 is back at the top");

    // "Differences only" turned on before the ranges arrive keeps the top byte too.
    view.update(cx, |v, cx| v.set_only_differences(false, cx));
    assert_eq!(top_row(cx, &view), 0x200);
    press(cx, handle, "f5");
    view.update(cx, |v, cx| v.set_only_differences(true, cx));
    wait_hex(cx, handle, &view).await;
    assert_eq!(top_row(cx, &view), 8);
}

/// 0x5100 bytes; the right file has 0xFF at each offset in `marks`, the left is all zeros.
fn blocks_pair(marks: &[usize]) -> (Vec<u8>, Vec<u8>) {
    let left = vec![0u8; 0x5100];
    let mut right = left.clone();
    for &at in marks {
        right[at] = 0xFF;
    }
    (left, right)
}

fn cursor(cx: &mut TestAppContext, view: &Entity<HexView>) -> Option<(bool, u64)> {
    view.read_with(cx, |v, _| v.cursor())
}

fn notice(cx: &mut TestAppContext, view: &Entity<HexView>) -> Option<String> {
    view.read_with(cx, |v, _| v.notice().map(str::to_owned))
}

#[gpui_kit::test]
async fn f8_f7_and_ctrl_home_end_walk_the_blocks(cx: &mut TestAppContext) {
    let (left, right) = blocks_pair(&[0x10, 0x200, 0x5000]);
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.position_text()), "– / 3");
    press(cx, handle, "f8");
    press(cx, handle, "f8");
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x200));
    assert_eq!(view.read_with(cx, |v, _| v.position_text()), "2 / 3");
    press(cx, handle, "ctrl-end");
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x5000));
    assert_eq!(view.read_with(cx, |v, _| v.position_text()), "3 / 3");
    press(cx, handle, "f8");
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x5000), "no wrap");
    assert_eq!(notice(cx, &view).as_deref(), Some("No more differences."));
    press(cx, handle, "f7");
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x200));
    assert_eq!(notice(cx, &view), None);
    press(cx, handle, "ctrl-home");
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x10));
    assert_eq!(view.read_with(cx, |v, _| v.position_text()), "1 / 3");
    // The target row is in view, not at the edge.
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    press(cx, handle, "ctrl-end");
    let (top, visible) = view.read_with(cx, |v, _| (v.top_row(), v.visible_rows()));
    let row = 0x5000 / 16;
    assert!(top <= row && row < top + visible, "{top} {visible}");
}

#[gpui_kit::test]
async fn differences_only_shows_only_rows_with_changes(cx: &mut TestAppContext) {
    let (left, right) = blocks_pair(&[0x10, 0x200, 0x5000]);
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    let all = view.read_with(cx, |v, _| v.row_count());
    assert_eq!(all, 0x5100 / 16);
    press(cx, handle, "ctrl-2");
    let (rows, expected) = view.read_with(cx, |v, _| {
        let ranges = &v.diff().unwrap().ranges;
        (
            v.row_count(),
            duet_core::hex::diff::DiffRows::new(ranges, 16).len(),
        )
    });
    assert_eq!(rows, expected);
    assert_eq!(rows, 3);
    for i in 0..rows {
        assert!(view.read_with(cx, |v, _| v.row_marks_for_test(i).differs()));
    }
    press(cx, handle, "ctrl-1");
    assert_eq!(view.read_with(cx, |v, _| v.row_count()), all);
}

#[gpui_kit::test]
async fn goto_moves_to_the_offset_and_bad_input_says_why(cx: &mut TestAppContext) {
    let (left, right) = blocks_pair(&[0x10]);
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let type_in = |cx: &mut TestAppContext, text: &str| {
        press(cx, handle, "ctrl-g");
        cx.update_window(handle, |_, window, cx| {
            window.press("ctrl-a", cx);
            window.input(text, cx);
            window.press("enter", cx);
        })
        .unwrap();
    };
    type_in(cx, "1F40h");
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x1F40));
    let (top, visible) = view.read_with(cx, |v, _| (v.top_row(), v.visible_rows()));
    let row = 0x1F40 / 16;
    assert!(top <= row && row < top + visible, "{top} {visible}");
    type_in(cx, "zz");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Type an offset such as 0x1F40, 1F40h or 8000.")
    );
    assert_eq!(
        cursor(cx, &view).map(|c| c.1),
        Some(0x1F40),
        "nothing moved"
    );
    type_in(cx, "0xFFFFFF");
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x50FF));
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Past the end: went to the last byte.")
    );
}

#[gpui_kit::test]
async fn the_status_names_sizes_counts_and_the_cursor(cx: &mut TestAppContext) {
    // left 100 bytes, right 90 bytes; bytes 4 and 40 differ, then the tail 90..100.
    let left = vec![0u8; 100];
    let mut right = vec![0u8; 90];
    right[4] = 0xAA;
    right[40] = 0xBB;
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.status_text()),
        "Left: 100 B · Right: 90 B · Sizes differ by 10 bytes (left is longer) · \
         Different bytes: 12 · Differences: 3 (3 runs) · Offset 0x00000000 (0) · left 00 right 00"
    );
    press(cx, handle, "f8");
    assert_eq!(
        view.read_with(cx, |v, _| v.status_text()),
        "Left: 100 B · Right: 90 B · Sizes differ by 10 bytes (left is longer) · \
         Different bytes: 12 · Differences: 3 (3 runs) · Offset 0x00000004 (4) · left 00 right AA"
    );
    // Identical files.
    let (_d2, h2, v2) = open_hex(cx, &[1, 2, 3], &[1, 2, 3]);
    wait_hex(cx, h2, &v2).await;
    assert_eq!(
        v2.read_with(cx, |v, _| v.status_text()),
        "Left: 3 B · Right: 3 B · Identical byte for byte. · Offset 0x00000000 (0) · left 01 right 01"
    );
}

#[gpui_kit::test]
async fn swap_and_reload_keep_working(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_hex(cx, &bytes_with(&[]), &bytes_with(&[3, 40]));
    wait_hex(cx, handle, &view).await;
    let log = Rc::new(RefCell::new(Vec::new()));
    let seen = log.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &HexEvent, _| {
            if let HexEvent::Swapped { left, right } = event {
                seen.borrow_mut().push((left.clone(), right.clone()));
            }
        })
        .detach();
    });
    let (a, b) = (dir.path().join("a.bin"), dir.path().join("b.bin"));
    press(cx, handle, "f9");
    assert_eq!(*log.borrow(), [(b.clone(), a.clone())]);
    view.read_with(cx, |v, _| assert_eq!(v.paths(), (b.as_path(), a.as_path())));
    wait_hex(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.block_count()), 2);
    std::fs::write(&b, bytes_with(&[7])).unwrap();
    press(cx, handle, "f5");
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    wait_hex(cx, handle, &view).await;
    let ranges = view.read_with(cx, |v, _| v.diff().unwrap().ranges.clone());
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0], 7..8);
}

#[gpui_kit::test]
async fn escape_cancels_a_running_compare_then_goes_back(cx: &mut TestAppContext) {
    let (handle, view) = open_hex_sources(
        cx,
        HexSource::synthetic(4 << 30, 0),
        HexSource::synthetic(4 << 30, 0),
    );
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    let log = Rc::new(RefCell::new(Vec::new()));
    let seen = log.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &HexEvent, _| {
            seen.borrow_mut().push(matches!(event, HexEvent::Back));
        })
        .detach();
    });
    press(cx, handle, "escape");
    assert!(log.borrow().is_empty(), "the first Esc only cancels");
    assert!(!view.read_with(cx, |v, _| v.is_busy()));
    assert!(
        view.read_with(cx, |v, _| v.status_text())
            .contains("Cancelled: the result is incomplete.")
    );
    press(cx, handle, "escape");
    assert_eq!(*log.borrow(), [true]);
    drop(view);
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
}

#[gpui_kit::test]
async fn the_toolbar_file_bar_and_status_are_drawn_and_the_goto_field_keeps_its_keys(
    cx: &mut TestAppContext,
) {
    let (left, right) = blocks_pair(&[0x10, 0x200]);
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for id in [
            "hex-toolbar",
            "hex-first",
            "hex-prev",
            "hex-position",
            "hex-next",
            "hex-last",
            "hex-swap",
            "hex-reload",
            "hex-bpr",
            "hex-filter",
            "hex-filter-all",
            "hex-filter-diff",
            "hex-goto",
            "hex-goto-go",
            "hex-find",
            "hex-copy",
            "hex-files",
            "hex-left-path",
            "hex-right-path",
            "hex-left-size",
            "hex-right-size",
            "hex-status",
        ] {
            assert!(window.try_find(id).is_some(), "{id} is drawn");
        }
    })
    .unwrap();
    // Ctrl+2 in the go-to field is the field's: the filter stays.
    press(cx, handle, "ctrl-g");
    press(cx, handle, "ctrl-2");
    assert_eq!(view.read_with(cx, |v, _| v.row_count()), 0x5100 / 16);
    // Esc gives the keys back to the rows, and the next Ctrl+2 filters.
    press(cx, handle, "escape");
    press(cx, handle, "ctrl-2");
    assert_eq!(view.read_with(cx, |v, _| v.row_count()), 2);
}

#[gpui_kit::test]
async fn goto_a_hidden_row_in_differences_only_shows_all_rows(cx: &mut TestAppContext) {
    let (left, right) = blocks_pair(&[0x10, 0x200, 0x5000]);
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    press(cx, handle, "ctrl-2");
    assert_eq!(view.read_with(cx, |v, _| v.row_count()), 3);
    assert!(view.update(cx, |v, cx| v.go_to("0x3000", cx)));
    assert_eq!(
        view.read_with(cx, |v, _| v.row_count()),
        0x5100 / 16,
        "the filter went back to All"
    );
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x3000));
    let (top, visible) = view.read_with(cx, |v, _| (v.top_row(), v.visible_rows()));
    let row = 0x3000 / 16;
    assert!(top <= row && row < top + visible, "{top} {visible}");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Showing all rows to reach that offset.")
    );
}

#[gpui_kit::test]
async fn goto_while_comparing_waits_for_the_ranges(cx: &mut TestAppContext) {
    // Identical 64 MiB sources are open at once, and their compare is still to run.
    let (handle, view) = open_hex_sources(
        cx,
        HexSource::synthetic(64 << 20, 0),
        HexSource::synthetic(64 << 20, 0),
    );
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    view.update(cx, |v, cx| {
        v.set_only_differences(true, cx);
        assert!(v.go_to("0x3000", cx));
    });
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x3000));
    assert_eq!(top_row(cx, &view), 0, "no jump to the end");
    wait_hex(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let (rows, top, visible) =
        view.read_with(cx, |v, _| (v.row_count(), v.top_row(), v.visible_rows()));
    assert_eq!(
        rows,
        (64 << 20) / 16,
        "All rows, so the target can be shown"
    );
    let row = 0x3000 / 16;
    assert!(top <= row && row < top + visible, "{top} {visible}");
}

#[gpui_kit::test]
async fn navigation_keys_say_still_comparing_and_do_not_move(cx: &mut TestAppContext) {
    let (handle, view) = open_hex_sources(
        cx,
        HexSource::synthetic(4 << 30, 0),
        HexSource::synthetic(4 << 30, 0),
    );
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    for key in ["f8", "f7", "ctrl-home", "ctrl-end"] {
        press(cx, handle, key);
        assert_eq!(
            notice(cx, &view).as_deref(),
            Some("Still comparing…"),
            "{key}"
        );
        assert_eq!(cursor(cx, &view), None);
    }
    press(cx, handle, "escape");
    press(cx, handle, "f8");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Cancelled: the result is incomplete.")
    );
    drop(view);
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
}

#[gpui_kit::test]
async fn a_notice_clears_on_the_next_scroll_filter_or_bytes_per_row(cx: &mut TestAppContext) {
    let (left, right) = blocks_pair(&[0x10]);
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    for action in 0..3 {
        press(cx, handle, "f8");
        press(cx, handle, "f8");
        assert!(notice(cx, &view).is_some());
        match action {
            0 => press(cx, handle, "down"),
            1 => view.update(cx, |v, cx| v.set_only_differences(true, cx)),
            _ => view.update(cx, |v, cx| v.set_bytes_per_row(32, cx)),
        }
        assert_eq!(notice(cx, &view), None, "action {action}");
    }
}

// ---- Task 6: selection, copy and find ----

/// 0x200 bytes each. The left file has `4D 5A 90 00` at 0x12; the right has "MZ" at 0x20,
/// 0x100 and 0x1F0 and "PE" at 0x80.
fn mz_files() -> (Vec<u8>, Vec<u8>) {
    let mut left = vec![0u8; 0x200];
    left[0x12..0x16].copy_from_slice(&[0x4D, 0x5A, 0x90, 0x00]);
    let mut right = vec![0u8; 0x200];
    for at in [0x20, 0x100, 0x1F0] {
        right[at..at + 2].copy_from_slice(b"MZ");
    }
    right[0x80..0x82].copy_from_slice(b"PE");
    (left, right)
}

/// A mouse press on byte `idx` of the hex cells of visible row `row`, with Shift or without.
fn click_byte(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<HexView>,
    row: usize,
    left: bool,
    idx: usize,
    shift: bool,
) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let x = view
            .read(cx)
            .cell_x_for_test(left, idx)
            .expect("the columns are known after a frame");
        let at = point(px(x), px(10.));
        if shift {
            let position = window.find(("hex-row", row)).bounds().origin + at;
            let modifiers = Modifiers::shift();
            window.dispatch_event(
                MouseDownEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers,
                    click_count: 1,
                    first_mouse: false,
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                MouseUpEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers,
                    click_count: 1,
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
        } else {
            window.click_at(("hex-row", row), at, cx);
        }
    })
    .unwrap();
}

fn selection(
    cx: &mut TestAppContext,
    view: &Entity<HexView>,
) -> Option<(bool, std::ops::Range<u64>)> {
    view.read_with(cx, |v, _| v.selection())
}

fn clipboard(cx: &mut TestAppContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

async fn wait_find(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<HexView>) {
    let view = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !view.read(cx).is_finding()
    })
    .await;
}

fn click_id(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) {
    cx.update_window(handle, |_, window, cx| window.click(id, cx))
        .unwrap();
}

/// Types `text` into the focused find field and presses Enter.
async fn find_text(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<HexView>,
    text: &str,
) {
    press(cx, handle, "ctrl-f");
    cx.update_window(handle, |_, window, cx| {
        window.press("ctrl-a", cx);
        window.input(text, cx);
        window.press("enter", cx);
    })
    .unwrap();
    wait_find(cx, handle, view).await;
}

#[gpui_kit::test]
async fn clicking_and_shift_clicking_bytes_selects_a_range(cx: &mut TestAppContext) {
    let (left, right) = mz_files();
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    click_byte(cx, handle, &view, 1, true, 2, false);
    assert_eq!(cursor(cx, &view), Some((true, 0x12)));
    assert_eq!(selection(cx, &view), None);
    click_byte(cx, handle, &view, 1, true, 5, true);
    assert_eq!(selection(cx, &view), Some((true, 0x12..0x16)));
    assert_eq!(cursor(cx, &view), Some((true, 0x12)), "the anchor stays");
    assert!(
        view.read_with(cx, |v, _| v.status_text())
            .contains("4 bytes selected")
    );
    // A click on the other side starts over there; Shift on another side does not extend.
    click_byte(cx, handle, &view, 2, false, 0, false);
    assert_eq!(cursor(cx, &view), Some((false, 0x20)));
    assert_eq!(selection(cx, &view), None);
    click_byte(cx, handle, &view, 2, true, 3, true);
    assert_eq!(cursor(cx, &view), Some((true, 0x23)));
    assert_eq!(selection(cx, &view), None);
}

#[gpui_kit::test]
async fn clicking_a_hatched_cell_does_nothing(cx: &mut TestAppContext) {
    let (left, _) = mz_files();
    let (_dir, handle, view) = open_hex(cx, &left, &left[..0x18]);
    wait_hex(cx, handle, &view).await;
    // Row 1 holds 0x10..0x20; the right file ends at 0x18, so its byte 0x1A is past the end.
    click_byte(cx, handle, &view, 1, false, 0xA, false);
    assert_eq!(cursor(cx, &view), None);
    click_byte(cx, handle, &view, 1, false, 0x2, false);
    assert_eq!(cursor(cx, &view), Some((false, 0x12)));
}

#[gpui_kit::test]
async fn copy_gives_hex_of_the_selection_or_the_cursor_row(cx: &mut TestAppContext) {
    let (left, right) = mz_files();
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    // Nothing selected and no cursor: the left side's top row, as LocalDiff does.
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("before".into())));
    press(cx, handle, "ctrl-c");
    assert_eq!(
        clipboard(cx).as_deref(),
        Some(hex_line(0, &left[0..16], 16, 0x1FF).as_str())
    );
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("before".into())));
    assert!(run_item(cx, handle, &view, "Copy this row"));
    assert_eq!(
        clipboard(cx).as_deref(),
        Some(hex_line(0, &left[0..16], 16, 0x1FF).as_str())
    );

    click_byte(cx, handle, &view, 1, true, 2, false);
    click_byte(cx, handle, &view, 1, true, 5, true);
    press(cx, handle, "ctrl-c");
    assert_eq!(clipboard(cx).as_deref(), Some("4D 5A 90 00"));

    // The cursor row, as the hex dump line.
    assert!(view.update(cx, |v, cx| v.go_to("0x10", cx)));
    press(cx, handle, "ctrl-c");
    assert_eq!(
        clipboard(cx).as_deref(),
        Some(hex_line(0x10, &left[0x10..0x20], 16, 0x1FF).as_str())
    );
    // The right side's row when the cursor is there.
    click_byte(cx, handle, &view, 2, false, 0, false);
    press(cx, handle, "ctrl-c");
    assert_eq!(
        clipboard(cx).as_deref(),
        Some(hex_line(0x20, &right[0x20..0x30], 16, 0x1FF).as_str())
    );
    // The toolbar button copies the same way.
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("x".into())));
    click_id(cx, handle, "hex-copy");
    assert_eq!(
        clipboard(cx).as_deref(),
        Some(hex_line(0x20, &right[0x20..0x30], 16, 0x1FF).as_str())
    );
}

#[gpui_kit::test]
async fn copying_two_mib_stops_at_one_mib_and_says_so(cx: &mut TestAppContext) {
    let big = vec![0xABu8; 2 << 20];
    let (_dir, handle, view) = open_hex(cx, &big, &big);
    wait_hex(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select_for_test(true, 0..(2 << 20), cx));
    press(cx, handle, "ctrl-c");
    let text = clipboard(cx).expect("copied");
    assert_eq!(
        text.len(),
        3 * (1 << 20) - 1,
        "1 MiB of bytes, as `AB AB …`"
    );
    assert!(text.starts_with("AB AB") && text.ends_with("AB"));
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Copied the first 1 MiB of the selection.")
    );
    // Exactly 1 MiB needs no cap notice.
    view.update(cx, |v, cx| v.select_for_test(true, 0..(1 << 20), cx));
    press(cx, handle, "ctrl-c");
    assert_ne!(
        notice(cx, &view).as_deref(),
        Some("Copied the first 1 MiB of the selection.")
    );
}

#[gpui_kit::test]
async fn find_hex_and_text_wraps_and_says_so(cx: &mut TestAppContext) {
    let (left, right) = mz_files();
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    press(cx, handle, "ctrl-f");
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("hex-find-bar").is_some());
        assert!(window.try_find("hex-find-input").is_some());
    })
    .unwrap();
    click_id(cx, handle, "hex-find-right");
    click_id(cx, handle, "hex-find-hex");
    // The first match from the top, on the right file only.
    find_text(cx, handle, &view, "4D 5A").await;
    assert_eq!(cursor(cx, &view), Some((false, 0x20)));
    assert_eq!(selection(cx, &view), Some((false, 0x20..0x22)));
    press(cx, handle, "f3");
    wait_find(cx, handle, &view).await;
    assert_eq!(cursor(cx, &view), Some((false, 0x100)));
    assert_eq!(notice(cx, &view), None);
    press(cx, handle, "f3");
    wait_find(cx, handle, &view).await;
    assert_eq!(cursor(cx, &view), Some((false, 0x1F0)));
    press(cx, handle, "f3");
    wait_find(cx, handle, &view).await;
    assert_eq!(cursor(cx, &view), Some((false, 0x20)));
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Wrapped to the start of the file.")
    );
    // Backwards from the first match wraps to the end.
    press(cx, handle, "shift-f3");
    wait_find(cx, handle, &view).await;
    assert_eq!(cursor(cx, &view), Some((false, 0x1F0)));
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Wrapped to the end of the file.")
    );
    press(cx, handle, "shift-f3");
    wait_find(cx, handle, &view).await;
    assert_eq!(cursor(cx, &view), Some((false, 0x100)));
    assert_eq!(notice(cx, &view), None);

    // Text.
    click_id(cx, handle, "hex-find-text");
    find_text(cx, handle, &view, "PE").await;
    assert_eq!(cursor(cx, &view), Some((false, 0x80)));
    // Not found, and a pattern that is not hex.
    find_text(cx, handle, &view, "QQ").await;
    assert_eq!(notice(cx, &view).as_deref(), Some("Not found: QQ."));
    assert_eq!(cursor(cx, &view), Some((false, 0x80)), "nothing moved");
    click_id(cx, handle, "hex-find-hex");
    find_text(cx, handle, &view, "4D 5").await;
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Type hex bytes, such as 4D 5A.")
    );
    // The toolbar's buttons step too, and the left side has its own match.
    click_id(cx, handle, "hex-find-left");
    find_text(cx, handle, &view, "4D5A").await;
    assert_eq!(cursor(cx, &view), Some((true, 0x12)));
    click_id(cx, handle, "hex-find-next");
    wait_find(cx, handle, &view).await;
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Wrapped to the start of the file."),
        "the lone match is found again after wrapping"
    );
    click_id(cx, handle, "hex-find-prev");
    wait_find(cx, handle, &view).await;
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Wrapped to the end of the file.")
    );
}

#[gpui_kit::test]
async fn hex_keys_stay_out_of_the_find_input(cx: &mut TestAppContext) {
    let (left, right) = mz_files();
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("sentinel".into())));
    view.update(cx, |v, cx| v.select_for_test(true, 0x12..0x16, cx));
    let rows = view.read_with(cx, |v, _| v.row_count());
    press(cx, handle, "ctrl-f");
    press(cx, handle, "ctrl-c");
    assert_eq!(clipboard(cx).as_deref(), Some("sentinel"), "no hex copy");
    press(cx, handle, "ctrl-2");
    assert_eq!(view.read_with(cx, |v, _| v.row_count()), rows, "no filter");
    press(cx, handle, "ctrl-1");
    assert_eq!(view.read_with(cx, |v, _| v.row_count()), rows);
    // Esc closes the bar and gives the keys back.
    press(cx, handle, "escape");
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("hex-find-bar").is_none());
    })
    .unwrap();
    press(cx, handle, "ctrl-c");
    assert_eq!(clipboard(cx).as_deref(), Some("4D 5A 90 00"));
    press(cx, handle, "ctrl-2");
    assert!(view.read_with(cx, |v, _| v.row_count()) < rows);
    press(cx, handle, "ctrl-1");

    // Enter in the field finds next, and so does F3 there.
    find_text(cx, handle, &view, "4D 5A").await;
    assert_eq!(cursor(cx, &view), Some((true, 0x12)));
    press(cx, handle, "f3");
    wait_find(cx, handle, &view).await;
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Wrapped to the start of the file.")
    );
}

#[gpui_kit::test]
async fn escape_cancels_a_running_find_before_anything_else(cx: &mut TestAppContext) {
    let (handle, view) = open_hex_sources(
        cx,
        HexSource::synthetic(4 << 30, 0),
        HexSource::synthetic(4 << 30, 0),
    );
    press(cx, handle, "ctrl-f");
    cx.update_window(handle, |_, window, cx| {
        window.input("AB", cx);
        window.press("enter", cx);
    })
    .unwrap();
    assert!(view.read_with(cx, |v, _| v.is_finding()));
    assert!(
        view.read_with(cx, |v, _| v.is_busy()),
        "the compare runs too"
    );
    press(cx, handle, "escape");
    assert!(!view.read_with(cx, |v, _| v.is_finding()));
    assert!(
        view.read_with(cx, |v, _| v.is_busy()),
        "the compare goes on"
    );
    assert_eq!(cursor(cx, &view), None);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("hex-find-bar").is_some(), "the bar stays");
    })
    .unwrap();
    // Next Esc: the bar closes. Then the compare is cancelled.
    press(cx, handle, "escape");
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("hex-find-bar").is_none());
    })
    .unwrap();
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    press(cx, handle, "escape");
    assert!(!view.read_with(cx, |v, _| v.is_busy()));
    drop(view);
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
}

#[gpui_kit::test]
async fn the_cursor_byte_is_drawn_on_both_of_its_cells(cx: &mut TestAppContext) {
    let (mut left, mut right) = mz_files();
    left.resize(0x2000, 0);
    right.resize(0x2000, 0);
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("hex-cursor-hex").is_none());
    })
    .unwrap();
    click_byte(cx, handle, &view, 1, true, 2, false);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let hex = window.find("hex-cursor-hex").bounds();
        let ascii = window.find("hex-cursor-ascii").bounds();
        let row = window.find(("hex-row", 1usize)).bounds();
        assert!(row.contains(&hex.center()) && row.contains(&ascii.center()));
        assert!(hex.size.width > ascii.size.width, "two digits vs one char");
    })
    .unwrap();
    // The cursor row scrolled out of view: nothing is drawn.
    view.update(cx, |v, cx| v.scroll_to_row(10, cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("hex-cursor-hex").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
async fn a_reload_or_a_filter_change_drops_a_go_to_made_while_comparing(cx: &mut TestAppContext) {
    let (left, right) = blocks_pair(&[0x10]);
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.bin"), dir.path().join("b.bin"));
    std::fs::write(&a, &left).unwrap();
    std::fs::write(&b, &right).unwrap();
    // Sources that are open already, so a go-to has a pair while the compare still runs.
    let open = || (HexSource::open(&a).unwrap(), HexSource::open(&b).unwrap());
    let (l, r) = open();
    let (handle, view) = open_hex_sources(cx, l, r);
    // Reload: the go-to typed before F5 must not jump the view after the reload.
    view.update(cx, |v, cx| {
        v.set_only_differences(true, cx);
        assert!(v.go_to("0x40", cx));
        v.reload(cx);
    });
    wait_hex(cx, handle, &view).await;
    assert_eq!(notice(cx, &view), None, "no late jump to a hidden row");
    assert_eq!(
        view.read_with(cx, |v, _| v.row_count()),
        1,
        "still filtered"
    );

    // A filter change drops it too: the byte clicked afterwards stays the cursor.
    let (l, r) = open();
    let (handle2, view2) = open_hex_sources(cx, l, r);
    view2.update(cx, |v, cx| {
        v.set_only_differences(true, cx);
        assert!(v.go_to("0x4000", cx));
        v.set_only_differences(false, cx);
        assert!(v.go_to("0x20", cx));
    });
    wait_hex(cx, handle2, &view2).await;
    assert_eq!(cursor(cx, &view2).map(|c| c.1), Some(0x20));
}

#[gpui_kit::test]
async fn scrollbar_and_overview_clicks_clear_the_notice(cx: &mut TestAppContext) {
    let (handle, view) = open_hex_sources(
        cx,
        HexSource::synthetic(1 << 20, 0),
        HexSource::synthetic(1 << 20, 0),
    );
    wait_hex(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    for target in ["hex-overview", "hex-vscroll"] {
        view.update(cx, |v, cx| {
            v.scroll_to_row(0, cx);
            assert!(!v.go_to("zz", cx));
        });
        assert!(notice(cx, &view).is_some());
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let bar = window.find(target).bounds();
            let at = point(bar.size.width / 2., bar.size.height * 0.75);
            window.click_at(target, at, cx);
        })
        .unwrap();
        assert!(top_row(cx, &view) > 0, "{target} scrolled");
        assert_eq!(notice(cx, &view), None, "{target}");
    }
}

#[gpui_kit::test]
async fn go_to_keeps_its_own_notice_through_its_scroll(cx: &mut TestAppContext) {
    let (handle, view) = open_hex_sources(
        cx,
        HexSource::synthetic(1 << 20, 0),
        HexSource::synthetic(1 << 20, 0),
    );
    wait_hex(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    assert!(view.update(cx, |v, cx| v.go_to("0xFFFFFFFF", cx)));
    assert!(top_row(cx, &view) > 0, "the view scrolled to the last byte");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Past the end: went to the last byte.")
    );
}

#[gpui_kit::test]
async fn the_top_byte_past_the_last_difference_lands_on_the_last_page(cx: &mut TestAppContext) {
    let (left, right) = blocks_pair(&[0x10]);
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    // A reload with the filter on remembers a top byte below the only difference.
    view.update(cx, |v, cx| {
        v.scroll_to_row(u64::MAX, cx);
        v.reload(cx);
        v.set_only_differences(true, cx);
    });
    while view.read_with(cx, |v, _| v.is_busy()) {
        cx.run_until_parked();
    }
    let (top, rows, visible) =
        view.read_with(cx, |v, _| (v.top_row(), v.row_count(), v.visible_rows()));
    assert_eq!(top, rows.saturating_sub(visible), "no u64::MAX sentinel");
}

#[gpui_kit::test]
async fn navigation_keys_and_buttons_share_one_rule(cx: &mut TestAppContext) {
    // Differences: both work. Identical: neither moves. While comparing: neither moves.
    let (left, right) = blocks_pair(&[0x10]);
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    assert!(!view.read_with(cx, |v, _| v.can_navigate()), "comparing");
    wait_hex(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.can_navigate()));
    click_id(cx, handle, "hex-next");
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x10));

    let (_dir2, handle2, same) = open_hex(cx, &left, &left);
    wait_hex(cx, handle2, &same).await;
    assert!(
        !same.read_with(cx, |v, _| v.can_navigate()),
        "no blocks: the buttons are off, and so is the key"
    );
    press(cx, handle2, "f8");
    assert_eq!(cursor(cx, &same), None);
    assert_eq!(notice(cx, &same).as_deref(), Some("No more differences."));
}

// ---- Task 6 fix round 1 ----

#[gpui_kit::test]
async fn arrows_move_the_cursor_and_home_end_go_to_the_row_ends(cx: &mut TestAppContext) {
    let (left, right) = mz_files();
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    // No cursor yet: the move starts from the top row's first byte, on the left side.
    press(cx, handle, "down");
    assert_eq!(cursor(cx, &view), Some((true, 0x10)));
    click_byte(cx, handle, &view, 1, true, 2, false);
    assert_eq!(cursor(cx, &view), Some((true, 0x12)));
    for (key, at) in [
        ("right", 0x13),
        ("left", 0x12),
        ("down", 0x22),
        ("up", 0x12),
        ("end", 0x1F),
        ("home", 0x10),
    ] {
        press(cx, handle, key);
        assert_eq!(cursor(cx, &view), Some((true, at)), "{key}");
    }
    assert_eq!(top_row(cx, &view), 0, "the cursor stayed in view");
    // Clamped to the side.
    assert!(view.update(cx, |v, cx| v.go_to("0", cx)));
    press(cx, handle, "left");
    press(cx, handle, "up");
    assert_eq!(cursor(cx, &view), Some((true, 0)));
    assert!(view.update(cx, |v, cx| v.go_to("0x1FF", cx)));
    press(cx, handle, "right");
    press(cx, handle, "down");
    assert_eq!(cursor(cx, &view), Some((true, 0x1FF)));
    press(cx, handle, "end");
    assert_eq!(cursor(cx, &view), Some((true, 0x1FF)));
}

#[gpui_kit::test]
async fn shift_with_the_arrows_extends_the_selection(cx: &mut TestAppContext) {
    let (left, right) = mz_files();
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    click_byte(cx, handle, &view, 1, true, 2, false);
    for _ in 0..3 {
        press(cx, handle, "shift-right");
    }
    assert_eq!(selection(cx, &view), Some((true, 0x12..0x16)));
    assert!(
        view.read_with(cx, |v, _| v.status_text())
            .contains("4 bytes selected")
    );
    press(cx, handle, "shift-left");
    assert_eq!(selection(cx, &view), Some((true, 0x12..0x15)));
    press(cx, handle, "shift-down");
    assert_eq!(selection(cx, &view), Some((true, 0x12..0x25)));
    // Without Shift the selection collapses to the cursor.
    press(cx, handle, "right");
    assert_eq!(selection(cx, &view), None);
    assert_eq!(cursor(cx, &view), Some((true, 0x25)));
}

#[gpui_kit::test]
async fn down_under_differences_only_skips_the_hidden_rows(cx: &mut TestAppContext) {
    let (left, right) = blocks_pair(&[0x10, 0x200, 0x5000]);
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    press(cx, handle, "ctrl-2");
    press(cx, handle, "f8");
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x10));
    press(cx, handle, "down");
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x200));
    press(cx, handle, "down");
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x5000));
    press(cx, handle, "up");
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x200));
    assert_eq!(
        view.read_with(cx, |v, _| v.row_count()),
        3,
        "still filtered"
    );
}

#[gpui_kit::test]
async fn page_down_moves_the_cursor_a_page_and_keeps_its_row_visible(cx: &mut TestAppContext) {
    let (left, right) = blocks_pair(&[0x10]);
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    assert!(view.update(cx, |v, cx| v.go_to("0", cx)));
    let visible = view.read_with(cx, |v, _| v.visible_rows());
    press(cx, handle, "pagedown");
    assert_eq!(
        cursor(cx, &view).map(|c| c.1),
        Some((visible - 1) * 16),
        "a page less one row"
    );
    assert_eq!(top_row(cx, &view), 0, "still in view: no scroll");
    press(cx, handle, "pagedown");
    let (top, at) = (top_row(cx, &view), cursor(cx, &view).unwrap().1);
    let row = at / 16;
    assert_eq!(row, 2 * (visible - 1));
    assert!(top > 0 && top <= row && row < top + visible, "{top} {row}");
    assert_eq!(row, top + visible - 1, "scrolled just enough");
    press(cx, handle, "pageup");
    assert_eq!(cursor(cx, &view).map(|c| c.1 / 16), Some(visible - 1));
}

#[gpui_kit::test]
async fn the_wheel_and_scrollbar_never_move_the_cursor(cx: &mut TestAppContext) {
    let (left, right) = blocks_pair(&[0x10]);
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    click_byte(cx, handle, &view, 1, true, 2, false);
    wheel(cx, handle, -3.);
    assert!(top_row(cx, &view) > 0);
    assert_eq!(cursor(cx, &view), Some((true, 0x12)));
}

#[gpui_kit::test]
async fn a_reopen_drops_the_cursor_and_a_stale_selection_never_copies(cx: &mut TestAppContext) {
    let big = vec![7u8; 0x2000];
    let (dir, handle, view) = open_hex(cx, &big, &big);
    wait_hex(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select_for_test(true, 0x1000..0x1800, cx));
    assert!(view.update(cx, |v, cx| v.go_to("0x1000", cx)));
    view.update(cx, |v, cx| v.select_for_test(true, 0x1000..0x1800, cx));
    for name in ["a.bin", "b.bin"] {
        std::fs::write(dir.path().join(name), vec![7u8; 0x100]).unwrap();
    }
    press(cx, handle, "f5");
    wait_hex(cx, handle, &view).await;
    assert_eq!(selection(cx, &view), None);
    assert_eq!(cursor(cx, &view), None);
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("before".into())));
    press(cx, handle, "ctrl-c");
    // No cursor and no selection: the left side's top row.
    assert_eq!(
        clipboard(cx).as_deref(),
        Some(hex_line(0, &[7u8; 16], 16, 0xFF).as_str())
    );
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("before".into())));
    // A selection wholly past the new end says so, and one that crosses it counts what exists.
    view.update(cx, |v, cx| v.select_for_test(true, 0x1000..0x1800, cx));
    press(cx, handle, "ctrl-c");
    assert_eq!(clipboard(cx).as_deref(), Some("before"));
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("The selection is past the end of the left file.")
    );
    view.update(cx, |v, cx| v.select_for_test(false, 0xF8..0x200, cx));
    press(cx, handle, "ctrl-c");
    assert_eq!(notice(cx, &view).as_deref(), Some("Copied 8 bytes."));
    assert_eq!(clipboard(cx).as_deref(), Some("07 07 07 07 07 07 07 07"));
    view.update(cx, |v, cx| v.select_for_test(false, 0x1000..0x1800, cx));
    press(cx, handle, "ctrl-c");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("The selection is past the end of the right file.")
    );
}

#[gpui_kit::test]
async fn find_keeps_the_show_all_notice_and_the_wrap_notice_together(cx: &mut TestAppContext) {
    let mut left = vec![0u8; 0x100];
    left[0x40..0x42].copy_from_slice(b"QQ");
    let mut right = left.clone();
    right[0x10] = 0xFF;
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    press(cx, handle, "ctrl-2");
    press(cx, handle, "f8");
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x10));
    press(cx, handle, "ctrl-f");
    click_id(cx, handle, "hex-find-text");
    press(cx, handle, "ctrl-f");
    cx.update_window(handle, |_, window, cx| window.input("QQ", cx))
        .unwrap();
    // Backwards from 0x10 there is nothing: it wraps to 0x40, a row "Differences only" hides.
    press(cx, handle, "shift-f3");
    wait_find(cx, handle, &view).await;
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x40));
    let said = notice(cx, &view).unwrap_or_default();
    assert!(
        said.contains("Showing all rows to reach that offset.")
            && said.contains("Wrapped to the end of the file."),
        "{said}"
    );
}

#[gpui_kit::test]
async fn a_find_made_while_the_ranges_load_keeps_its_selection(cx: &mut TestAppContext) {
    let (handle, view) = open_hex_sources(
        cx,
        HexSource::synthetic(64 << 20, 0),
        HexSource::synthetic(64 << 20, 0),
    );
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    // A match arrives while "Differences only" still waits for the ranges.
    view.update(cx, |v, cx| {
        v.set_only_differences(true, cx);
        v.found_for_test(true, 0x3000, 2, cx);
    });
    let expected = Some((true, 0x3000..0x3002));
    assert_eq!(selection(cx, &view), expected);
    wait_hex(cx, handle, &view).await;
    assert_eq!(cursor(cx, &view), Some((true, 0x3000)));
    assert_eq!(
        selection(cx, &view),
        expected,
        "the ranges arriving keep it"
    );
}

/// A right press and release on byte `idx` of the hex cells of visible row `row`.
fn right_click_byte(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<HexView>,
    row: usize,
    left: bool,
    idx: usize,
) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let x = view
            .read(cx)
            .cell_x_for_test(left, idx)
            .expect("the columns are known after a frame");
        let position = window.find(("hex-row", row)).bounds().origin + point(px(x), px(10.));
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Right,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Right,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn menu_items(cx: &mut TestAppContext, view: &Entity<HexView>) -> Vec<String> {
    view.read_with(cx, |v, _| {
        duet_app::ui::menu::outline(&duet_app::hex::menu::row_menu(v))
    })
}

/// Runs the menu item `label` the way the popup does.
fn run_item(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<HexView>,
    label: &str,
) -> bool {
    let entries = view.read_with(cx, |v, _| duet_app::hex::menu::row_menu(v));
    let view = view.clone();
    cx.update_window(handle, |_, window, cx| {
        let focus = view.focus_handle(cx);
        duet_app::ui::menu::activate(entries, label, Some(&focus), window, cx)
    })
    .unwrap()
}

#[gpui_kit::test]
async fn the_hex_menu_matches_the_state(cx: &mut TestAppContext) {
    let (left, right) = blocks_pair(&[0x10]);
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    assert_eq!(
        menu_items(cx, &view),
        [
            "Copy this row",
            "Copy offset",
            "-",
            "Previous difference",
            "Next difference",
            "-",
            "Go to offset…",
            "Find…",
        ]
    );
    view.update(cx, |v, cx| v.select_for_test(true, 0..4, cx));
    assert_eq!(menu_items(cx, &view)[0], "Copy bytes (hex)");

    // "Copy offset" puts the cursor's offset on the clipboard, padded like the status line.
    assert!(view.update(cx, |v, cx| v.go_to("0x1F40", cx)));
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("before".into())));
    assert!(run_item(cx, handle, &view, "Copy offset"));
    assert_eq!(clipboard(cx).as_deref(), Some("0x00001F40"));

    // The items run the same actions as the keys.
    assert!(run_item(cx, handle, &view, "Previous difference"));
    assert_eq!(cursor(cx, &view).map(|c| c.1), Some(0x10));
    view.update(cx, |v, cx| v.select_for_test(true, 0..4, cx));
    assert!(run_item(cx, handle, &view, "Copy bytes (hex)"));
    assert_eq!(clipboard(cx).as_deref(), Some("00 00 00 00"));
    assert!(run_item(cx, handle, &view, "Find…"));
    assert!(view.read_with(cx, |v, _| v.find_open()));
}

#[gpui_kit::test]
async fn the_difference_items_are_disabled_without_a_difference(cx: &mut TestAppContext) {
    let same = vec![7u8; 64];
    let (_dir, handle, view) = open_hex(cx, &same, &same);
    wait_hex(cx, handle, &view).await;
    let items = menu_items(cx, &view);
    assert_eq!(items[3], "(Previous difference)");
    assert_eq!(items[4], "(Next difference)");
    assert!(!run_item(cx, handle, &view, "Next difference"));
    assert!(run_item(cx, handle, &view, "Go to offset…"));
}

#[gpui_kit::test]
async fn right_click_on_an_unselected_byte_moves_the_cursor(cx: &mut TestAppContext) {
    let (left, right) = blocks_pair(&[0x10]);
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    right_click_byte(cx, handle, &view, 1, false, 4);
    assert_eq!(cursor(cx, &view), Some((false, 0x14)));
    assert_eq!(selection(cx, &view), None);
    // Esc closes the popup, which would swallow the next press.
    press(cx, handle, "escape");

    // Inside the selection nothing moves and nothing is dropped.
    view.update(cx, |v, cx| v.select_for_test(true, 0..8, cx));
    let before = cursor(cx, &view);
    right_click_byte(cx, handle, &view, 0, true, 3);
    assert_eq!(selection(cx, &view), Some((true, 0..8)));
    assert_eq!(cursor(cx, &view), before);
    assert_eq!(menu_items(cx, &view)[0], "Copy bytes (hex)");
    press(cx, handle, "escape");

    // Outside it, the cursor goes there and the selection is dropped.
    right_click_byte(cx, handle, &view, 0, true, 12);
    assert_eq!(cursor(cx, &view), Some((true, 12)));
    assert_eq!(selection(cx, &view), None);
    assert_eq!(menu_items(cx, &view)[0], "Copy this row");
    press(cx, handle, "escape");

    // "Copy offset" is about the pressed byte.
    right_click_byte(cx, handle, &view, 2, true, 5);
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("before".into())));
    assert!(run_item(cx, handle, &view, "Copy offset"));
    assert_eq!(clipboard(cx).as_deref(), Some("0x00000025"));
}

type Opens = Rc<RefCell<Vec<(PathBuf, PathBuf)>>>;

fn collect_opens(cx: &mut TestAppContext, view: &Entity<HexView>) -> Opens {
    let asked: Opens = Default::default();
    let sink = asked.clone();
    cx.update(|cx| {
        cx.subscribe(view, move |_, e: &HexEvent, _| {
            if let HexEvent::Open { left, right } = e {
                sink.borrow_mut().push((left.clone(), right.clone()));
            }
        })
        .detach()
    });
    asked
}

#[gpui_kit::test]
async fn a_typed_path_replaces_one_side_and_bad_paths_say_why(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_hex(cx, b"abc", b"abd");
    wait_hex(cx, handle, &view).await;
    let other = dir.path().join("c.bin");
    std::fs::write(&other, b"xyz").unwrap();
    let asked = collect_opens(cx, &view);
    cx.update_window(handle, |_, window, cx| window.click("hex-right-path", cx))
        .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.path_edit_for_test()),
        Some(false)
    );
    for (typed, expected) in [
        ("rel.bin", "Type a full path, such as C:\\folder\\file.txt."),
        ("", "Not a file: ."),
    ] {
        view.update(cx, |v, cx| v.set_path_edit_text_for_test(typed, cx));
        press(cx, handle, "enter");
        assert_eq!(notice(cx, &view).as_deref(), Some(expected));
    }
    let missing = dir.path().join("none.bin");
    view.update(cx, |v, cx| {
        v.set_path_edit_text_for_test(missing.to_str().unwrap(), cx)
    });
    press(cx, handle, "enter");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some(format!("Not a file: {}.", missing.display()).as_str())
    );
    assert!(asked.borrow().is_empty(), "nothing was opened");
    assert_eq!(
        view.read_with(cx, |v, _| v.path_edit_for_test()),
        Some(false),
        "the input stays for another try"
    );
    view.update(cx, |v, cx| {
        v.set_path_edit_text_for_test(other.to_str().unwrap(), cx)
    });
    press(cx, handle, "enter");
    cx.run_until_parked();
    let left = view.read_with(cx, |v, _| v.paths().0.to_path_buf());
    assert_eq!(asked.borrow().as_slice(), [(left, other)]);
    assert_eq!(view.read_with(cx, |v, _| v.path_edit_for_test()), None);
}

#[gpui_kit::test]
async fn escape_leaves_the_path_box_without_opening(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_hex(cx, b"abc", b"abd");
    wait_hex(cx, handle, &view).await;
    let asked = collect_opens(cx, &view);
    let backs = Rc::new(RefCell::new(0));
    let sink = backs.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, e: &HexEvent, _| {
            if matches!(e, HexEvent::Back) {
                *sink.borrow_mut() += 1;
            }
        })
        .detach()
    });
    cx.update_window(handle, |_, window, cx| window.click("hex-left-path", cx))
        .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.path_edit_for_test()),
        Some(true)
    );
    press(cx, handle, "escape");
    assert_eq!(view.read_with(cx, |v, _| v.path_edit_for_test()), None);
    assert!(asked.borrow().is_empty());
    assert_eq!(*backs.borrow(), 0, "Esc only left the box");
}

#[gpui_kit::test]
async fn dropped_files_replace_the_half_under_the_pointer_or_both(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_hex(cx, b"abc", b"abd");
    wait_hex(cx, handle, &view).await;
    let (c, d) = (dir.path().join("c.bin"), dir.path().join("d.bin"));
    std::fs::write(&c, b"1").unwrap();
    std::fs::write(&d, b"2").unwrap();
    let asked = collect_opens(cx, &view);
    let (l, r) = view.read_with(cx, |v, _| {
        (v.paths().0.to_path_buf(), v.paths().1.to_path_buf())
    });
    view.update(cx, |v, cx| v.drop_paths(std::slice::from_ref(&c), true, cx));
    view.update(cx, |v, cx| {
        v.drop_paths(std::slice::from_ref(&d), false, cx)
    });
    view.update(cx, |v, cx| v.drop_paths(&[c.clone(), d.clone()], false, cx));
    view.update(cx, |v, cx| {
        v.drop_paths(&[c.clone(), d.clone(), l.clone()], true, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        asked.borrow().as_slice(),
        [
            (c.clone(), r.clone()),
            (l.clone(), d.clone()),
            (c.clone(), d.clone()),
            (c.clone(), d.clone()),
        ]
    );
    // A folder or a missing path opens nothing and says so.
    view.update(cx, |v, cx| {
        v.drop_paths(&[dir.path().to_path_buf()], true, cx)
    });
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Drop files here; folders open in a folder compare.")
    );
    assert_eq!(asked.borrow().len(), 4);
}

#[gpui_kit::test]
async fn ctrl_o_and_ctrl_shift_o_ask_for_a_file_for_that_side(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_hex(cx, b"abc", b"abd");
    wait_hex(cx, handle, &view).await;
    let other = dir.path().join("c.bin");
    std::fs::write(&other, b"xyz").unwrap();
    let asked = collect_opens(cx, &view);
    let (l, r) = view.read_with(cx, |v, _| {
        (v.paths().0.to_path_buf(), v.paths().1.to_path_buf())
    });
    for (key, expected) in [
        ("ctrl-o", (other.clone(), r.clone())),
        ("ctrl-shift-o", (l.clone(), other.clone())),
    ] {
        press(cx, handle, key);
        assert!(cx.did_prompt_for_paths(), "{key} asks for a file");
        let picked = other.clone();
        cx.simulate_path_prompt_response(move |options| {
            assert!(options.files && !options.directories && !options.multiple);
            Some(vec![picked])
        });
        cx.run_until_parked();
        assert_eq!(asked.borrow().last(), Some(&expected));
    }
}

#[gpui_kit::test]
async fn copying_two_empty_files_says_there_is_nothing(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_hex(cx, b"", b"");
    wait_hex(cx, handle, &view).await;
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("before".into())));
    press(cx, handle, "ctrl-c");
    assert_eq!(clipboard(cx).as_deref(), Some("before"));
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Nothing to copy: click a byte or select a range first.")
    );
}

/// A right press and release at `x` px from the left edge of visible row `row`.
fn right_click_x(cx: &mut TestAppContext, handle: AnyWindowHandle, row: usize, x: f32) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let position = window.find(("hex-row", row)).bounds().origin + point(px(x), px(10.));
        for down in [true, false] {
            let event = if down {
                MouseDownEvent {
                    button: MouseButton::Right,
                    position,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                }
                .to_platform_input()
            } else {
                MouseUpEvent {
                    button: MouseButton::Right,
                    position,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                }
                .to_platform_input()
            };
            window.dispatch_event(event, cx);
        }
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
async fn a_right_click_that_misses_every_byte_changes_nothing_and_keeps_the_menu(
    cx: &mut TestAppContext,
) {
    // The left file ends at 20 bytes, so row 1 has hatching past its end.
    let (_dir, handle, view) = open_hex(cx, &[1u8; 20], &[2u8; 40]);
    wait_hex(cx, handle, &view).await;
    click_byte(cx, handle, &view, 0, true, 2, false);
    view.update(cx, |v, cx| v.select_for_test(true, 0..2, cx));
    let (cur, sel) = (cursor(cx, &view), selection(cx, &view));
    let items = menu_items(cx, &view);
    let (gutter, hatch) = view.read_with(cx, |v, _| {
        (
            v.gutter_x_for_test().unwrap(),
            v.cell_x_for_test(true, 10).unwrap(),
        )
    });
    assert!(
        !popup_open(cx, handle),
        "no menu before the first right press"
    );
    for (row, x) in [(0, 2.0), (0, gutter), (1, hatch)] {
        right_click_x(cx, handle, row, x);
        assert!(popup_open(cx, handle), "the menu opened: x={x} row={row}");
        assert_eq!(cursor(cx, &view), cur, "x={x} row={row}");
        assert_eq!(selection(cx, &view), sel, "x={x} row={row}");
        assert_eq!(menu_items(cx, &view), items);
        assert_eq!(items.len(), 8, "the menu still has its items");
        press(cx, handle, "escape");
        assert!(!popup_open(cx, handle), "Esc closed the menu");
    }
}

/// Whether a context menu popup is on screen (gpui-component's `popup-menu`).
fn popup_open(cx: &mut TestAppContext, handle: AnyWindowHandle) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.try_find("popup-menu").is_some()
    })
    .unwrap()
}

#[gpui_kit::test]
async fn a_running_find_says_how_far_it_got(cx: &mut TestAppContext) {
    let (left, right) = mz_files();
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    press(cx, handle, "ctrl-f");
    cx.update_window(handle, |_, window, cx| {
        window.input("AB CD", cx);
        window.press("enter", cx);
    })
    .unwrap();
    assert!(view.read_with(cx, |v, _| v.is_finding()));
    let status = view.read_with(cx, |v, _| v.status_text());
    assert!(status.contains("Searching… 0%"), "{status}");
    wait_find(cx, handle, &view).await;
    let status = view.read_with(cx, |v, _| v.status_text());
    assert!(!status.contains("Searching…"), "{status}");
    assert_eq!(notice(cx, &view).as_deref(), Some("Not found: AB CD."));
}

#[gpui_kit::test]
async fn hex_keys_stay_out_of_the_path_box(cx: &mut TestAppContext) {
    let (left, right) = blocks_pair(&[0x10, 0x200, 0x5000]);
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    let rows = view.read_with(cx, |v, _| v.row_count());
    cx.update_window(handle, |_, window, cx| window.click("hex-left-path", cx))
        .unwrap();
    for key in [
        "f8",
        "f7",
        "alt-down",
        "alt-up",
        "ctrl-n",
        "ctrl-p",
        "ctrl-end",
        "ctrl-2",
        "ctrl-1",
        "down",
        "up",
        "pagedown",
        "right",
        "shift-down",
    ] {
        press(cx, handle, key);
        assert_eq!(cursor(cx, &view), None, "{key} moved the cursor");
        assert_eq!(selection(cx, &view), None, "{key} selected");
        assert_eq!(
            view.read_with(cx, |v, _| v.row_count()),
            rows,
            "{key} changed the filter"
        );
        assert_eq!(top_row(cx, &view), 0, "{key} scrolled the rows");
        assert_eq!(
            view.read_with(cx, |v, _| v.path_edit_for_test()),
            Some(true),
            "{key}: the box stays"
        );
    }
}

#[gpui_kit::test]
async fn a_cancelled_compare_under_differences_only_says_so_in_the_body(cx: &mut TestAppContext) {
    let (handle, view) = open_hex_sources(
        cx,
        HexSource::synthetic(4 << 30, 0),
        HexSource::synthetic(4 << 30, 0),
    );
    press(cx, handle, "ctrl-2");
    press(cx, handle, "escape");
    assert!(!view.read_with(cx, |v, _| v.is_busy()));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("hex-placeholder").label(),
            Some("Cancelled: the result is incomplete.")
        );
    })
    .unwrap();
    // All rows are still there to read.
    press(cx, handle, "ctrl-1");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("hex-placeholder").is_none());
        assert!(window.try_find(("hex-row", 0usize)).is_some());
    })
    .unwrap();
    drop(view);
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
}

#[gpui_kit::test]
async fn shift_click_after_shift_arrows_extends_from_the_first_anchor(cx: &mut TestAppContext) {
    let (left, right) = mz_files();
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    click_byte(cx, handle, &view, 1, true, 2, false);
    for _ in 0..3 {
        press(cx, handle, "shift-right");
    }
    assert_eq!(selection(cx, &view), Some((true, 0x12..0x16)));
    assert_eq!(cursor(cx, &view), Some((true, 0x15)), "the moving end");
    click_byte(cx, handle, &view, 1, true, 8, true);
    assert_eq!(selection(cx, &view), Some((true, 0x12..0x19)));
    assert_eq!(cursor(cx, &view), Some((true, 0x12)), "back on the anchor");
    // Shift+arrows from there, then a Shift+click before the anchor: still from 0x12.
    press(cx, handle, "shift-left");
    assert_eq!(selection(cx, &view), Some((true, 0x12..0x18)));
    click_byte(cx, handle, &view, 1, true, 0, true);
    assert_eq!(selection(cx, &view), Some((true, 0x10..0x13)));
}

#[gpui_kit::test]
async fn f9_keeps_the_cursor_and_selection_on_the_other_side_and_f5_clears_them(
    cx: &mut TestAppContext,
) {
    let (left, right) = mz_files();
    let (_dir, handle, view) = open_hex(cx, &left, &right);
    wait_hex(cx, handle, &view).await;
    click_byte(cx, handle, &view, 1, true, 2, false);
    click_byte(cx, handle, &view, 1, true, 5, true);
    assert_eq!(selection(cx, &view), Some((true, 0x12..0x16)));
    press(cx, handle, "f9");
    wait_hex(cx, handle, &view).await;
    assert_eq!(selection(cx, &view), Some((false, 0x12..0x16)));
    assert_eq!(cursor(cx, &view), Some((false, 0x12)));
    assert!(
        view.read_with(cx, |v, _| v.status_text())
            .contains("4 bytes selected")
    );
    press(cx, handle, "f5");
    wait_hex(cx, handle, &view).await;
    assert_eq!(selection(cx, &view), None);
    assert_eq!(cursor(cx, &view), None);
}

#[gpui_kit::test]
async fn f9_cuts_the_kept_selection_to_the_side_it_lands_on(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_hex(cx, &[1u8; 0x40], &[1u8; 0x40]);
    wait_hex(cx, handle, &view).await;
    click_byte(cx, handle, &view, 1, true, 2, false);
    view.update(cx, |v, cx| v.select_for_test(true, 0x12..0x30, cx));
    // The file that moves to the right side is cut to 0x20 bytes before the reopen reads it.
    std::fs::write(dir.path().join("a.bin"), [1u8; 0x20]).unwrap();
    press(cx, handle, "f9");
    wait_hex(cx, handle, &view).await;
    assert_eq!(selection(cx, &view), Some((false, 0x12..0x20)));
    assert_eq!(cursor(cx, &view), Some((false, 0x12)));
}

#[gpui_kit::test]
async fn esc_replaces_the_still_comparing_notice_with_the_cancelled_text(cx: &mut TestAppContext) {
    let (handle, view) = open_hex_sources(
        cx,
        HexSource::synthetic(4 << 30, 0),
        HexSource::synthetic(4 << 30, 0),
    );
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    press(cx, handle, "f8");
    assert_eq!(notice(cx, &view).as_deref(), Some("Still comparing…"));
    press(cx, handle, "escape");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Cancelled: the result is incomplete.")
    );
    assert!(
        !view
            .read_with(cx, |v, _| v.status_text())
            .contains("Still comparing")
    );
    drop(view);
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
}
