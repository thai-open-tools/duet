//! Headless UI tests for the table view and its tab.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use duet_app::launch::Launch;
use duet_app::table::view::{CellMark, TableEvent, TableView};
use duet_app::ui::fonts::{Fonts, code_advance};
use duet_app::workspace::{TabItem, Workspace};
use duet_core::table::Delimiter;
use duet_core::table::nav::Filter;
use duet_core::table::xlsx::fixture::{Cell, SheetSpec, text, workbook};
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

/// Two files named a.csv and b.csv in a temporary folder.
fn write_pair(left: &[u8], right: &[u8]) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.csv"), dir.path().join("b.csv"));
    std::fs::write(&a, left).unwrap();
    std::fs::write(&b, right).unwrap();
    (dir, a, b)
}

fn open_table(
    cx: &mut TestAppContext,
    left: &[u8],
    right: &[u8],
) -> (tempfile::TempDir, AnyWindowHandle, Entity<TableView>) {
    let (dir, a, b) = write_pair(left, right);
    let (handle, view) = open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| TableView::new(a, b, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    (dir, handle, view)
}

async fn wait_table(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<TableView>) {
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

fn record_events(cx: &mut TestAppContext, view: &Entity<TableView>) -> Rc<RefCell<Vec<String>>> {
    let log = Rc::new(RefCell::new(Vec::new()));
    let seen = log.clone();
    cx.update(|cx| {
        cx.subscribe(view, move |_, event: &TableEvent, _| {
            seen.borrow_mut().push(match event {
                TableEvent::Back => "back".to_owned(),
                TableEvent::Swapped { left, right } => {
                    format!("swapped {} {}", name(left), name(right))
                }
                TableEvent::Open { .. } => "open".to_owned(),
            });
        })
        .detach();
    });
    log
}

fn name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

async fn open_workspace(
    cx: &mut TestAppContext,
    a: PathBuf,
    b: PathBuf,
) -> (AnyWindowHandle, Entity<Workspace>, Entity<TableView>) {
    let (handle, workspace) = open_test_window(cx, move |window, cx| {
        cx.new(|cx| Workspace::new(Launch::Table { left: a, right: b }, window, cx))
    });
    let ws = workspace.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        ws.read(cx).active_idle(cx)
    })
    .await;
    let view = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Table(view)) => view.clone(),
        _ => panic!("expected a table tab"),
    });
    (handle, workspace, view)
}

#[gpui_kit::test]
async fn a_table_tab_opens_compares_and_badges(cx: &mut TestAppContext) {
    let (_dir, a, b) = write_pair(b"id,v\n1,a\n2,b\n", b"id,v\n1,a\n2,c\n");
    let (handle, workspace) = open_test_window(cx, move |window, cx| {
        cx.new(|cx| Workspace::new(Launch::Table { left: a, right: b }, window, cx))
    });
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        None,
        "no badge while the files load"
    );
    let ws = workspace.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        ws.read(cx).active_idle(cx)
    })
    .await;
    let view = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Table(view)) => view.clone(),
        _ => panic!("expected a table tab"),
    });
    view.read_with(cx, |v, _| {
        assert_eq!(v.diff().expect("compared").counts.differs, 1);
        assert_eq!(v.message(true), None);
    });
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        Some("1".to_owned())
    );
    let title = workspace.read_with(cx, |w, _| w.tabs().iter().next().unwrap().1.title());
    assert_eq!(title, "a.csv ↔ b.csv");

    let (_dir, a, b) = write_pair(b"id,v\n1,a\n", b"id,v\n1,a\n");
    let (_handle, workspace, _view) = open_workspace(cx, a, b).await;
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        Some("ok".to_owned())
    );
}

#[gpui_kit::test]
async fn a_cp874_thai_csv_decodes(cx: &mut TestAppContext) {
    let (bytes, _, unmappable) = encoding_rs::WINDOWS_874.encode("ชื่อ,ราคา\nกา,5\n");
    assert!(!unmappable);
    let (_dir, handle, view) = open_table(cx, &bytes, &bytes);
    wait_table(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let diff = v.diff().expect("compared");
        let names: Vec<&str> = diff.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["ชื่อ", "ราคา"]);
        assert_eq!(
            v.side(true).unwrap().encoding.encoding,
            encoding_rs::WINDOWS_874
        );
        assert!(diff.is_identical());
    });
}

#[gpui_kit::test]
async fn a_vanished_file_shows_the_open_error(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_table(cx, b"id,v\n1,a\n", b"id,v\n1,a\n");
    wait_table(cx, handle, &view).await;
    std::fs::remove_file(dir.path().join("b.csv")).unwrap();
    view.update(cx, |v, cx| v.reload(cx));
    wait_table(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let message = v.message(false).expect("the right side did not open");
        assert!(message.starts_with("Can't open b.csv:"), "{message}");
        assert_eq!(v.message(true), None);
        assert!(v.side(true).is_some(), "the left side stays loaded");
        assert!(v.side(false).is_none());
        assert!(v.diff().is_none());
        assert_eq!(v.badge().as_deref(), Some("!"));
    });
}

#[gpui_kit::test]
#[ignore = "writes 161 MiB; run once in the final round"]
async fn a_file_over_160_mb_is_refused(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_table(cx, b"id\n1\n", b"id\n1\n");
    wait_table(cx, handle, &view).await;
    let big = dir.path().join("b.csv");
    std::fs::write(&big, vec![b'a'; 161 * 1024 * 1024]).unwrap();
    view.update(cx, |v, cx| v.reload(cx));
    wait_table(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(
            v.message(false),
            Some("Can't open b.csv: the file is larger than 160 MB.")
        );
        assert!(v.side(true).is_some());
    });
}

#[gpui_kit::test]
async fn swap_and_reload_keep_working(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_table(cx, b"id,v\n1,a\n", b"id,v\n1,b\n2,x\n");
    wait_table(cx, handle, &view).await;
    let log = record_events(cx, &view);
    press(cx, handle, "f9");
    assert_eq!(*log.borrow(), ["swapped b.csv a.csv"]);
    wait_table(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let (l, r) = v.paths();
        assert_eq!((name(l), name(r)), ("b.csv".to_owned(), "a.csv".to_owned()));
        assert_eq!(v.side(true).unwrap().name, "b.csv");
        let counts = v.diff().unwrap().counts;
        assert_eq!((counts.differs, counts.left_only), (1, 1), "now b is left");
    });
    std::fs::write(dir.path().join("a.csv"), b"id,v\n1,b\n2,x\n").unwrap();
    press(cx, handle, "f5");
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    wait_table(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.diff().unwrap().is_identical()));
}

#[gpui_kit::test]
async fn f5_then_escape_keeps_the_shown_result(cx: &mut TestAppContext) {
    // T-59: the reload is cancelled, and the result from before it stays usable.
    let (_dir, handle, view) = open_table(cx, b"id,v\n1,a\n2,b\n", b"id,v\n1,a\n2,X\n");
    wait_table(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.badge()).as_deref(), Some("1"));
    press(cx, handle, "f5");
    view.read_with(cx, |v, _| {
        assert!(v.is_busy());
        assert_eq!(v.badge(), None, "the tab is marked busy while it reloads");
        assert!(
            v.diff().is_some(),
            "the shown result stays while the files load"
        );
    });
    press(cx, handle, "escape");
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        assert!(!v.is_busy());
        assert_eq!(v.notice(), Some("Cancelled: the result is incomplete."));
        assert_eq!(v.diff().map(|d| d.counts.differs), Some(1));
        assert_eq!(v.badge().as_deref(), Some("1"));
        assert!(v.can_export());
    });
    press(cx, handle, "f8");
    assert_eq!(selection(cx, &view), Some((1, Some(1))));
}

#[gpui_kit::test]
async fn escape_goes_back_after_cancelling(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_table(cx, b"id,v\n1,a\n", b"id,v\n1,b\n");
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    let log = record_events(cx, &view);
    press(cx, handle, "escape");
    assert!(log.borrow().is_empty(), "the first Esc only cancels");
    view.read_with(cx, |v, _| {
        assert!(!v.is_busy());
        assert_eq!(v.notice(), Some("Cancelled: the result is incomplete."));
    });
    press(cx, handle, "escape");
    assert_eq!(*log.borrow(), ["back"]);
}

#[gpui_kit::test]
async fn options_reparse_or_recompare_and_pasted_sides_compare(cx: &mut TestAppContext) {
    let (handle, view) = open_test_window(cx, |window, cx| {
        let view =
            cx.new(|cx| TableView::with_text_for_test("a;b\nX;1\n", "a;b\nx;1\n", window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    wait_table(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let left = v.side(true).unwrap();
        assert_eq!(left.name, "Pasted text (left)");
        assert!(left.path.is_none());
        assert_eq!(
            (left.delimiter, left.auto, left.columns),
            (Delimiter::Semicolon, true, 2)
        );
        assert_eq!(v.diff().unwrap().counts.differs, 1);
    });
    view.update(cx, |v, cx| v.set_prefs(|p| p.ignore_case = true, cx));
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    wait_table(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.diff().unwrap().is_identical()));
    view.update(cx, |v, cx| {
        v.set_prefs(|p| p.delimiter = Some(Delimiter::Comma), cx)
    });
    wait_table(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let left = v.side(true).unwrap();
        assert_eq!(
            (left.delimiter, left.auto, left.columns),
            (Delimiter::Comma, false, 1)
        );
        assert!(v.prefs().ignore_case);
        assert_eq!(v.paths(), (Path::new(""), Path::new("")));
    });
}

/// A view over two pasted texts, focused.
fn open_text(
    cx: &mut TestAppContext,
    left: &str,
    right: &str,
) -> (AnyWindowHandle, Entity<TableView>) {
    let (left, right) = (left.to_owned(), right.to_owned());
    open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| TableView::with_text_for_test(&left, &right, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    })
}

/// Two frames: the first measures the body, the second lays the rows out in it.
fn draw(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// `rows` data rows under a header of `columns` names; cell (r, c) is "r.c" and `pad`.
fn grid_text(rows: usize, columns: usize, pad: &str) -> String {
    let line = |cell: &dyn Fn(usize) -> String| {
        (0..columns).map(cell).collect::<Vec<_>>().join(",") + "\n"
    };
    let mut text = line(&|c| format!("c{c}"));
    for r in 0..rows {
        text.push_str(&line(&|c| format!("{r}.{c}{pad}")));
    }
    text
}

fn top_row(cx: &mut TestAppContext, view: &Entity<TableView>) -> usize {
    view.read_with(cx, |v, _| v.top_row())
}

fn selection(cx: &mut TestAppContext, view: &Entity<TableView>) -> Option<(usize, Option<usize>)> {
    view.read_with(cx, |v, _| v.selection())
}

fn wheel(cx: &mut TestAppContext, handle: AnyWindowHandle, lines: f32) {
    cx.update_window(handle, |_, window, cx| {
        window.scroll("table-rows", ScrollDelta::Lines(point(0., lines)), cx)
    })
    .unwrap();
}

#[gpui_kit::test]
async fn a_500_column_table_builds_only_visible_cells(cx: &mut TestAppContext) {
    let text = grid_text(50, 500, "");
    let (handle, view) = open_text(cx, &text, &text);
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    let (visible, built) = view.read_with(cx, |v, _| (v.visible_rows(), v.built_cells_for_test()));
    assert!(visible > 10, "a page is many rows, got {visible}");
    assert!(built > 0, "the cells on screen are built");
    let bound = 2 * 40 * (visible + 1);
    assert!(built < bound, "{built} cells for {visible} rows");
    view.read_with(cx, |v, _| {
        let [left, right] = v.built_columns_for_test();
        assert!(
            left.contains(&0) && right.contains(&0),
            "{left:?} {right:?}"
        );
        assert!(!left.contains(&499));
    });

    // Dragging the horizontal bar to its end brings the last column in, on both halves.
    cx.update_window(handle, |_, window, cx| {
        let bar = window.find("table-hscroll").bounds();
        let y = bar.center().y;
        window.drag(
            point(bar.right() - px(2.), y),
            point(bar.right() + px(40.), y),
            cx,
        );
    })
    .unwrap();
    draw(cx, handle);
    view.read_with(cx, |v, _| {
        assert!(v.scroll_x() > 0., "scrolled right");
        let [left, right] = v.built_columns_for_test();
        assert!(left.contains(&499), "left half: {left:?}");
        assert!(right.contains(&499), "right half: {right:?}");
        assert!(
            !left.contains(&0),
            "the first column is off screen: {left:?}"
        );
        assert!(v.built_cells_for_test() < bound);
    });
}

#[gpui_kit::test]
async fn columns_size_from_header_and_samples(cx: &mut TestAppContext) {
    let long = "x".repeat(2000);
    let text = format!("id,note\n1234567890,{long}\n");
    let (handle, view) = open_text(cx, &text, &text);
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    let advance = cx
        .update_window(handle, |_, window, cx| {
            f32::from(code_advance(window, &Fonts::current(cx).code))
        })
        .unwrap();
    assert!(
        48. * advance + 16. > 320.,
        "48 chars are wider than the cap"
    );
    view.read_with(cx, |v, _| {
        assert_eq!(v.column_width_for_test(0), (10. * advance).round() + 16.);
        assert_eq!(v.column_width_for_test(1), 320., "a 2,000-char cell clamps");
    });
}

#[gpui_kit::test]
async fn wheel_keys_and_scrollbars_move_the_rows(cx: &mut TestAppContext) {
    // 200 rows of 12 wide columns: taller than the window and wider than a half.
    let text = grid_text(200, 12, "-wide-cell");
    let (handle, view) = open_text(cx, &text, &text);
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    let (rows, visible) = view.read_with(cx, |v, _| (v.view().len(), v.visible_rows()));
    assert_eq!(rows, 200);
    assert!(visible > 10, "a page is many rows, got {visible}");

    wheel(cx, handle, -3.);
    assert_eq!(top_row(cx, &view), 3, "three lines down");
    wheel(cx, handle, 1.);
    assert_eq!(top_row(cx, &view), 2, "one line up");
    wheel(cx, handle, -0.2);
    assert_eq!(top_row(cx, &view), 3, "a small notch still moves one row");
    wheel(cx, handle, 5.);
    assert_eq!(top_row(cx, &view), 0, "no further up");

    // Shift with the vertical wheel scrolls sideways, and not down.
    cx.update_window(handle, |_, window, cx| {
        window.hover("table-rows", cx);
        let position = window.find("table-rows").bounds().center();
        window.dispatch_event(
            ScrollWheelEvent {
                position,
                delta: ScrollDelta::Lines(point(0., -3.)),
                modifiers: Modifiers::shift(),
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
    })
    .unwrap();
    assert!(view.read_with(cx, |v, _| v.scroll_x()) > 0., "Shift+wheel");
    assert_eq!(top_row(cx, &view), 0, "and not down");

    // Dragging the thumb to the bottom reaches the last page.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let bar = window.find("table-vscroll").bounds();
        let x = bar.center().x;
        window.drag(
            point(x, bar.top() + px(6.)),
            point(x, bar.bottom() - px(2.)),
            cx,
        );
    })
    .unwrap();
    assert_eq!(top_row(cx, &view), rows - visible);

    // Down with nothing selected takes the first row; Up and Down move one row, the page
    // keys a page less one.
    press(cx, handle, "down");
    draw(cx, handle);
    assert_eq!(selection(cx, &view), Some((0, None)));
    press(cx, handle, "down");
    assert_eq!(selection(cx, &view), Some((1, None)));
    press(cx, handle, "up");
    assert_eq!(selection(cx, &view), Some((0, None)));
    press(cx, handle, "pagedown");
    assert_eq!(selection(cx, &view), Some((visible - 1, None)));
    let top = top_row(cx, &view);
    assert!(
        (top..top + visible).contains(&(visible - 1)),
        "the selected row is in view"
    );
    press(cx, handle, "pageup");
    assert_eq!(selection(cx, &view), Some((0, None)));

    // Left and Right walk the columns, clamped, and scroll the column into view.
    press(cx, handle, "right");
    assert_eq!(selection(cx, &view), Some((0, Some(0))));
    for _ in 0..13 {
        press(cx, handle, "right");
    }
    assert_eq!(
        selection(cx, &view),
        Some((0, Some(11))),
        "clamped to the last column"
    );
    draw(cx, handle);
    cx.update_window(handle, |_, window, cx| {
        let row = window.find(("table-row", 0usize)).bounds();
        let half = f32::from(row.size.width - px(6.)) / 2.;
        let v = view.read(cx);
        assert!(v.scroll_x() > 0.);
        let centre = v.cell_x_for_test(true, 11).unwrap();
        let right_edge = centre + v.column_width_for_test(11) / 2.;
        assert!(
            right_edge <= half + 0.5,
            "the last column ends inside the left half: {right_edge} > {half}"
        );
    })
    .unwrap();
    for _ in 0..13 {
        press(cx, handle, "left");
    }
    assert_eq!(selection(cx, &view), Some((0, Some(0))));
    assert_eq!(view.read_with(cx, |v, _| v.scroll_x()), 0.);
}

#[gpui_kit::test]
async fn missing_cells_and_absent_rows_draw_differently(cx: &mut TestAppContext) {
    // Left row 1 has no "c"; the right has a column "z" of its own and a second row.
    let (handle, view) = open_text(cx, "a,b,c\n1,2\n", "a,b,c,z\n1,2,3,9\n5,6,7,8\n");
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    view.read_with(cx, |v, _| {
        let diff = v.diff().expect("compared");
        let names: Vec<&str> = diff.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c", "z"]);
        let mark = |pos, column, left| v.cell_mark_for_test(pos, column, left);
        assert_eq!(
            mark(0, 2, true),
            CellMark::Missing,
            "the left record is short"
        );
        assert_eq!(mark(0, 2, false), CellMark::Differs);
        assert_eq!(mark(0, 0, true), CellMark::Same);
        assert_eq!(mark(0, 0, false), CellMark::Same);
        assert_eq!(mark(0, 3, false), CellMark::OneSidedColumn);
        assert_eq!(
            mark(0, 3, true),
            CellMark::Missing,
            "no such column on the left"
        );
        assert_eq!(mark(1, 0, true), CellMark::NoRow, "a right-only row");
        assert_eq!(mark(1, 0, false), CellMark::Same);
        assert!(v.built_cells_for_test() > 0);
    });

    // An empty cell is a value, not a missing one: "1,2," equals the short "1,2", but only
    // the short record's cell is hatched.
    let (handle2, view2) = open_text(cx, "a,b,c\n1,2,\n", "a,b,c\n1,2\n");
    wait_table(cx, handle2, &view2).await;
    view2.read_with(cx, |v, _| {
        assert!(v.diff().unwrap().is_identical());
        assert_eq!(v.cell_mark_for_test(0, 2, true), CellMark::Same, "empty");
        assert_eq!(v.cell_mark_for_test(0, 2, false), CellMark::Missing);
    });
    // A one-sided column's cell past a short record's end is hatched like any missing cell.
    let (handle3, view3) = open_text(cx, "a,b,z\n1,2\n3,4,5\n", "a,b\n1,2\n3,4\n");
    wait_table(cx, handle3, &view3).await;
    view3.read_with(cx, |v, _| {
        assert_eq!(v.cell_mark_for_test(0, 2, true), CellMark::Missing);
        assert_eq!(v.cell_mark_for_test(1, 2, true), CellMark::OneSidedColumn);
    });

    // A filter with no rows says so.
    view.update(cx, |v, cx| v.set_prefs(|p| p.filter = Filter::Same, cx));
    draw(cx, handle);
    assert!(view.read_with(cx, |v, _| v.view().is_empty()));
    cx.update_window(handle, |_, window, _| {
        assert_eq!(
            window.find("table-empty").label(),
            Some("No rows match this filter.")
        );
    })
    .unwrap();
}

/// A left click on the cell of `column` in visible row `row`, on one half.
fn click_cell(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TableView>,
    row: usize,
    left: bool,
    column: usize,
) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let x = view
            .read(cx)
            .cell_x_for_test(left, column)
            .expect("the columns are laid out after a frame");
        window.click_at(("table-row", row), point(px(x), px(10.)), cx);
    })
    .unwrap();
}

/// (differing, left-only, right-only) rows of the shown result.
fn counts(cx: &mut TestAppContext, view: &Entity<TableView>) -> (usize, usize, usize) {
    view.read_with(cx, |v, _| {
        let c = v.diff().expect("compared").counts;
        (c.differs, c.left_only, c.right_only)
    })
}

#[gpui_kit::test]
async fn an_old_result_draws_from_its_own_tables_while_the_new_one_pairs(cx: &mut TestAppContext) {
    // 2 data rows (one short) against 6: once F9 swaps the tables, the old result's last
    // rows point past the new right table's records.
    let (handle, view) = open_text(
        cx,
        "a,b,c\n1,2\n3,4,5\n",
        "a,b,c\n1,2,3\n2,x,y\n3,4,5\n6,7,8\n9,9,9\n10,1,1\n",
    );
    wait_table(cx, handle, &view).await;
    assert_eq!(counts(cx, &view), (1, 0, 4));
    click_cell(cx, handle, &view, 5, false, 2);
    assert_eq!(
        selection(cx, &view),
        Some((5, Some(2))),
        "the last right-only row"
    );

    press(cx, handle, "f9");
    assert!(
        view.read_with(cx, |v, _| v.is_busy()),
        "the swapped pair pairs"
    );
    // The old result, drawn while the swapped tables pair.
    draw(cx, handle);
    assert!(
        view.read_with(cx, |v, _| v.is_busy()),
        "the frame was drawn before the new result"
    );
    assert_eq!(selection(cx, &view), Some((5, Some(2))));
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    assert_eq!(counts(cx, &view), (1, 4, 0), "the sides changed places");
    assert_eq!(
        selection(cx, &view),
        None,
        "a new result clears the selection"
    );
    view.read_with(cx, |v, _| {
        assert_eq!(v.cell_mark_for_test(0, 2, true), CellMark::Differs);
        assert_eq!(
            v.cell_mark_for_test(0, 2, false),
            CellMark::Missing,
            "the short record is on the right now"
        );
    });

    // Header off, then on: while the second compare runs, the shown result still counts the
    // header line as a data row.
    view.update(cx, |v, cx| v.set_prefs(|p| p.header = false, cx));
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    view.update(cx, |v, cx| v.set_prefs(|p| p.header = true, cx));
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    draw(cx, handle);
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    assert_eq!(counts(cx, &view), (1, 4, 0));
}

#[gpui_kit::test]
async fn clicking_a_cell_selects_it_and_escape_clears(cx: &mut TestAppContext) {
    let (handle, view) = open_text(
        cx,
        "a,b,c\n1,2,3\n4,5,6\n7,8,9\n",
        "a,b,c\n1,X,3\n4,5,6\n7,8,9\n",
    );
    wait_table(cx, handle, &view).await;
    let log = record_events(cx, &view);
    click_cell(cx, handle, &view, 1, true, 2);
    assert_eq!(selection(cx, &view), Some((1, Some(2))));
    click_cell(cx, handle, &view, 2, false, 0);
    assert_eq!(selection(cx, &view), Some((2, Some(0))));
    // The row number selects the row at its first differing column.
    cx.update_window(handle, |_, window, cx| {
        window.click_at(("table-row", 0usize), point(px(4.), px(10.)), cx);
    })
    .unwrap();
    assert_eq!(selection(cx, &view), Some((0, Some(1))));

    press(cx, handle, "escape");
    assert_eq!(selection(cx, &view), None);
    assert!(
        log.borrow().is_empty(),
        "the first Esc only clears the selection"
    );
    press(cx, handle, "escape");
    assert_eq!(*log.borrow(), ["back"]);
}

// ---- Task 6: toolbar, options, chips, navigation and status ----

use duet_app::status::{StatusPart, Tone};
use gpui_kit::component::input::InputState;

fn texts(parts: &[StatusPart]) -> Vec<String> {
    parts.iter().map(|p| p.text.clone()).collect()
}

fn status(cx: &mut TestAppContext, view: &Entity<TableView>) -> Vec<String> {
    view.read_with(cx, |v, _| texts(&v.status_parts()))
}

fn notice(cx: &mut TestAppContext, view: &Entity<TableView>) -> Option<String> {
    view.read_with(cx, |v, _| v.notice().map(str::to_owned))
}

/// Gives the key box the keys, replaces its text with `text` and presses `key`, if any.
fn type_keys(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TableView>,
    text: &str,
    key: Option<&str>,
) {
    let input: Entity<InputState> = view.read_with(cx, |v, _| v.keys_input_for_test());
    cx.update_window(handle, |_, window, cx| {
        window.focus(&input.focus_handle(cx), cx);
        window.press("ctrl-a", cx);
        window.input(text, cx);
        if let Some(key) = key {
            window.press(key, cx);
        }
    })
    .unwrap();
}

fn click(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
}

#[gpui_kit::test]
async fn options_change_the_compare_and_are_saved(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let data_dir = data.path().to_path_buf();
    cx.update(|cx| cx.set_global(duet_app::app::AppPaths { data_dir }));
    let (handle, view) = open_text(cx, "id,v\n1,A\n2,b\n", "id,v\n1,a\n2,b\n");
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    for id in [
        "table-options",
        "table-delimiter",
        "table-header",
        "table-ignore-case",
        "table-trim",
        "table-numbers",
        "table-keys",
    ] {
        cx.update_window(handle, |_, window, _| {
            assert!(window.try_find(id).is_some(), "{id} is drawn")
        })
        .unwrap();
    }
    assert_eq!(counts(cx, &view), (1, 0, 0));

    // Ignore case: the case-only difference is the same, and the choice is saved.
    click(cx, handle, "table-ignore-case");
    assert!(view.read_with(cx, |v, _| v.prefs().ignore_case));
    wait_table(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.diff().unwrap().is_identical()));
    let saved = std::fs::read_to_string(data.path().join("settings.json")).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(saved["table.ignore_case"], serde_json::Value::Bool(true));

    // A key pairs the rows; the box applies on Enter.
    type_keys(cx, handle, &view, "id", Some("enter"));
    assert_eq!(view.read_with(cx, |v, _| v.prefs().keys.clone()), "id");
    wait_table(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.diff().unwrap().keys.clone()),
        [0]
    );
    assert_eq!(notice(cx, &view), None);

    // A bad token among good ones says so, without the alignment sentence.
    type_keys(cx, handle, &view, "id, zzz", Some("enter"));
    wait_table(cx, handle, &view).await;
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Key column not found on both sides: zzz.")
    );
    // The same key box value warns once: another option change is quiet.
    view.update(cx, |v, cx| v.set_prefs(|p| p.trim = true, cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(notice(cx, &view), None);
    // Only bad tokens: the rows fall back to order.
    type_keys(cx, handle, &view, "zzz", Some("enter"));
    wait_table(cx, handle, &view).await;
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Key column not found on both sides: zzz. Rows are aligned by order.")
    );

    // The delimiter re-parses both sides.
    view.update(cx, |v, cx| {
        v.set_prefs(|p| p.delimiter = Some(Delimiter::Semicolon), cx)
    });
    wait_table(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        for left in [true, false] {
            let side = v.side(left).unwrap();
            assert_eq!((side.delimiter, side.auto), (Delimiter::Semicolon, false));
        }
    });
}

#[gpui_kit::test]
async fn the_key_box_applies_on_blur_and_after_a_pause(cx: &mut TestAppContext) {
    let (handle, view) = open_text(cx, "id,v\n1,a\n", "id,v\n1,a\n");
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    // Typing alone changes nothing at once...
    type_keys(cx, handle, &view, "id", None);
    assert_eq!(view.read_with(cx, |v, _| v.prefs().keys.clone()), "");
    // ...600 ms later it applies.
    cx.executor().advance_clock(Duration::from_millis(700));
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| v.prefs().keys.clone()), "id");
    wait_table(cx, handle, &view).await;
    // Leaving the box applies it at once. The headless window never takes focus away, so the
    // box's own blur event is sent by hand.
    type_keys(cx, handle, &view, "v", None);
    let input = view.read_with(cx, |v, _| v.keys_input_for_test());
    input.update(cx, |_, cx| {
        cx.emit(gpui_kit::component::input::InputEvent::Blur)
    });
    assert_eq!(view.read_with(cx, |v, _| v.prefs().keys.clone()), "v");
}

#[gpui_kit::test]
async fn chips_filter_rows_and_show_counts(cx: &mut TestAppContext) {
    let (handle, view) = open_text(
        cx,
        "id,v\n1,a\n2,b\n3,c\n4,d\n",
        "id,v\n1,a\n2,X\n4,d\n5,e\n",
    );
    wait_table(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_prefs(|p| p.keys = "id".into(), cx));
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    let label = |cx: &mut TestAppContext, id: &'static str| {
        cx.update_window(handle, |_, window, _| {
            window.find(id).label().unwrap().to_owned()
        })
        .unwrap()
    };
    assert_eq!(label(cx, "table-chip-all"), "All 5");
    assert_eq!(label(cx, "table-chip-diff"), "Differences 3");
    assert_eq!(label(cx, "table-chip-same"), "Same 2");
    assert_eq!(label(cx, "table-chip-left"), "Left only 1");
    assert_eq!(label(cx, "table-chip-right"), "Right only 1");

    click(cx, handle, "table-chip-left");
    assert_eq!(
        view.read_with(cx, |v, _| v.prefs().filter),
        Filter::LeftOnly
    );
    assert_eq!(view.read_with(cx, |v, _| v.view().len()), 1);

    // Ctrl+1/2/3 pick All, Differences and Same.
    for (key, filter, rows) in [
        ("ctrl-2", Filter::Differences, 3),
        ("ctrl-3", Filter::Same, 2),
        ("ctrl-1", Filter::All, 5),
    ] {
        press(cx, handle, key);
        assert_eq!(view.read_with(cx, |v, _| v.prefs().filter), filter);
        assert_eq!(view.read_with(cx, |v, _| v.view().len()), rows);
    }
}

#[gpui_kit::test]
async fn the_filter_digits_stay_in_the_key_box(cx: &mut TestAppContext) {
    let (handle, view) = open_text(cx, "id,v\n1,a\n2,b\n", "id,v\n1,a\n2,c\n");
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    type_keys(cx, handle, &view, "", None);
    press(cx, handle, "ctrl-2");
    press(cx, handle, "n");
    assert_eq!(view.read_with(cx, |v, _| v.prefs().filter), Filter::All);
    assert_eq!(selection(cx, &view), None, "n typed an n, it did not jump");
}

/// Five data rows; id 2 differs in column w, id 4 in column v.
fn walk_fixture() -> (&'static str, &'static str) {
    (
        "id,v,w\n1,a,p\n2,b,q\n3,c,r\n4,d,s\n5,e,t\n",
        "id,v,w\n1,a,p\n2,b,Q\n3,c,r\n4,D,s\n5,e,t\n",
    )
}

#[gpui_kit::test]
async fn f8_f7_and_ctrl_home_end_walk_the_differing_rows(cx: &mut TestAppContext) {
    let (left, right) = walk_fixture();
    let (handle, view) = open_text(cx, left, right);
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    let position = |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.position_text());
    assert_eq!(position(cx), "– / 2");

    press(cx, handle, "f8");
    assert_eq!(
        selection(cx, &view),
        Some((1, Some(2))),
        "first differing column"
    );
    assert_eq!(position(cx), "1 / 2");
    press(cx, handle, "f8");
    assert_eq!(selection(cx, &view), Some((3, Some(1))));
    assert_eq!(position(cx), "2 / 2");
    press(cx, handle, "f8");
    assert_eq!(notice(cx, &view).as_deref(), Some("No more differences."));
    assert_eq!(selection(cx, &view), Some((3, Some(1))), "no wrap");

    press(cx, handle, "f7");
    assert_eq!(selection(cx, &view), Some((1, Some(2))));
    press(cx, handle, "f7");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("No earlier differences.")
    );
    assert_eq!(selection(cx, &view), Some((1, Some(2))));

    press(cx, handle, "ctrl-end");
    assert_eq!(selection(cx, &view).map(|s| s.0), Some(3));
    press(cx, handle, "ctrl-home");
    assert_eq!(selection(cx, &view).map(|s| s.0), Some(1));
    press(cx, handle, "n");
    assert_eq!(selection(cx, &view).map(|s| s.0), Some(3));
    press(cx, handle, "p");
    assert_eq!(selection(cx, &view).map(|s| s.0), Some(1));
    press(cx, handle, "alt-down");
    assert_eq!(selection(cx, &view).map(|s| s.0), Some(3));
    press(cx, handle, "alt-up");
    assert_eq!(selection(cx, &view).map(|s| s.0), Some(1));

    // On a row between the stops the position is a dash, and Next/Prev step around it.
    view.update(cx, |v, cx| v.select(2, None, false, cx));
    assert_eq!(position(cx), "– / 2");
    press(cx, handle, "f8");
    assert_eq!(selection(cx, &view).map(|s| s.0), Some(3));
    view.update(cx, |v, cx| v.select(2, None, false, cx));
    press(cx, handle, "f7");
    assert_eq!(selection(cx, &view).map(|s| s.0), Some(1));
}

#[gpui_kit::test]
async fn navigation_buttons_and_notices_without_stops(cx: &mut TestAppContext) {
    let (handle, view) = open_text(cx, "id,v\n1,a\n", "id,v\n1,a\n");
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    assert_eq!(view.read_with(cx, |v, _| v.position_text()), "– / 0");
    press(cx, handle, "f8");
    assert_eq!(notice(cx, &view).as_deref(), Some("No differing rows."));
    view.update(cx, |v, cx| v.set_prefs(|p| p.filter = Filter::Same, cx));
    press(cx, handle, "f8");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("The “Same” filter has no differing rows.")
    );

    // The buttons walk too.
    let (left, right) = walk_fixture();
    let (handle, view) = open_text(cx, left, right);
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    click(cx, handle, "table-next");
    assert_eq!(selection(cx, &view).map(|s| s.0), Some(1));
    click(cx, handle, "table-last");
    assert_eq!(selection(cx, &view).map(|s| s.0), Some(3));
    click(cx, handle, "table-prev");
    assert_eq!(selection(cx, &view).map(|s| s.0), Some(1));
    click(cx, handle, "table-first");
    assert_eq!(selection(cx, &view).map(|s| s.0), Some(1));
}

#[gpui_kit::test]
async fn the_toolbar_names_the_sides(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_table(cx, b"id,v\n1,a\n2,b\n", "id,v\n1,a\n".as_bytes());
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    for id in [
        "table-toolbar",
        "table-open-left",
        "table-open-right",
        "table-paste-left",
        "table-paste-right",
        "table-left-name",
        "table-right-name",
        "table-first",
        "table-prev",
        "table-position",
        "table-next",
        "table-last",
        "table-swap",
        "table-reload",
        "table-find",
        "table-export",
        "table-status",
    ] {
        cx.update_window(handle, |_, window, _| {
            assert!(window.try_find(id).is_some(), "{id} is drawn")
        })
        .unwrap();
    }
    view.read_with(cx, |v, _| {
        assert_eq!(v.name_text(true), "a.csv  2 × 2");
        assert_eq!(v.name_text(false), "b.csv  1 × 2");
        let tip = v.name_tooltip(true).unwrap();
        assert!(tip.contains("a.csv"), "{tip}");
        assert!(tip.contains("Delimiter: comma (,) (auto)"), "{tip}");
        assert!(tip.contains("2 rows × 2 columns"), "{tip}");
        assert!(tip.contains("UTF-8"), "{tip}");
    });
    // Without a header the first record is a row.
    view.update(cx, |v, cx| v.set_prefs(|p| p.header = false, cx));
    assert_eq!(view.read_with(cx, |v, _| v.name_text(true)), "a.csv  3 × 2");
    // Pasted text names itself and has no path.
    let (handle, view) = open_text(cx, "a\n1\n", "a\n1\n");
    wait_table(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.name_text(true)),
        "Pasted text (left)  1 × 1"
    );
}

#[gpui_kit::test]
async fn the_status_names_counts_keys_and_encodings(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_table(
        cx,
        b"\xEF\xBB\xBFid;name;only_l\n1;a;p\n2;b;q\n",
        b"id;name;only_r\n1;a;p\n2;B;q\n",
    );
    wait_table(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_prefs(|p| p.keys = "id, zzz".into(), cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(
        status(cx, &view),
        [
            "Same 1",
            "Differ 1",
            "Left only 0",
            "Right only 0",
            "Cells differ 1",
            "Columns matched 2 · left only 1 · right only 1",
            "Paired by key: id",
            "Key column not found: zzz",
            "Left: UTF-8 (BOM) · semicolon (;) (auto)",
            "Right: UTF-8 · semicolon (;) (auto)",
        ]
    );
    let tones = view.read_with(cx, |v, _| {
        v.status_parts().iter().map(|p| p.tone).collect::<Vec<_>>()
    });
    assert_eq!(tones[1], Tone::Changed);
    assert_eq!(tones[2], Tone::LeftOnly);
    assert_eq!(tones[3], Tone::RightOnly);

    // Without keys the rows align by order; a forced delimiter has no "(auto)".
    view.update(cx, |v, cx| {
        v.set_prefs(
            |p| {
                p.keys.clear();
                p.delimiter = Some(Delimiter::Semicolon);
            },
            cx,
        )
    });
    wait_table(cx, handle, &view).await;
    let parts = status(cx, &view);
    assert!(
        parts.contains(&"Aligned by row order".to_owned()),
        "{parts:?}"
    );
    assert!(
        parts.contains(&"Left: UTF-8 (BOM) · semicolon (;)".to_owned()),
        "{parts:?}"
    );
    // Thousands separators.
    let big: String = std::iter::once("id\n".to_owned())
        .chain((0..1500).map(|i| format!("{i}\n")))
        .collect();
    let (handle, view) = open_text(cx, &big, &big);
    wait_table(cx, handle, &view).await;
    let parts = status(cx, &view);
    assert_eq!(parts[0], "The tables are identical cell for cell.");
    assert_eq!(parts[1], "Same 1,500");
}

#[gpui_kit::test]
async fn no_matching_column_names_say_so(cx: &mut TestAppContext) {
    let (handle, view) = open_text(cx, "a,b\n1,2\n", "x,y\n1,2\n");
    wait_table(cx, handle, &view).await;
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("No column names match. Turn off “First row is a header” to compare by position.")
    );
    view.update(cx, |v, cx| v.set_prefs(|p| p.header = false, cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(notice(cx, &view), None);
}

#[gpui_kit::test]
async fn the_binary_notice_survives_a_recompare(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_table(cx, b"a,b\n1,\x00\n", b"a,b\n1,2\n");
    wait_table(cx, handle, &view).await;
    let expected = "“a.csv” looks like a binary file, not CSV or TSV.";
    assert_eq!(notice(cx, &view).as_deref(), Some(expected));
    view.update(cx, |v, cx| v.set_prefs(|p| p.trim = true, cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(notice(cx, &view).as_deref(), Some(expected));
    press(cx, handle, "f9");
    wait_table(cx, handle, &view).await;
    assert_eq!(notice(cx, &view).as_deref(), Some(expected));
}

#[gpui_kit::test]
async fn an_option_change_mid_compare_drops_the_stale_result(cx: &mut TestAppContext) {
    // 20,000 rows; row 7 differs only by a leading space.
    let side = |pad: &str| {
        let mut text = String::from("id,v\n");
        for i in 0..20_000 {
            let pad = if i == 7 { pad } else { "" };
            text.push_str(&format!("{i},{pad}x\n"));
        }
        text
    };
    let (left, right) = (side(""), side(" "));
    let (handle, view) = open_text(cx, &left, &right);
    // The first compare is under way; the option change supersedes it.
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    view.update(cx, |v, cx| v.set_prefs(|p| p.trim = true, cx));
    wait_table(cx, handle, &view).await;
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        assert!(v.prefs().trim);
        assert!(
            v.diff().unwrap().is_identical(),
            "the trim-off result must not land"
        );
    });
}

#[gpui_kit::test]
async fn the_binary_notice_shows_beside_the_compare_notice(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_table(cx, b"a,b\n1,\x00\n", b"x,y\n1,2\n");
    wait_table(cx, handle, &view).await;
    let shown = notice(cx, &view).unwrap();
    assert!(
        shown.contains("\u{201c}a.csv\u{201d} looks like a binary file, not CSV or TSV."),
        "{shown}"
    );
    assert!(shown.contains("No column names match."), "{shown}");
}

// ---- Task 7: detail panel, find and go to row ----

/// Types `text` into the focused field, replacing what it holds, and presses `key`, if any.
fn type_in_focus(cx: &mut TestAppContext, handle: AnyWindowHandle, text: &str, key: Option<&str>) {
    cx.update_window(handle, |_, window, cx| {
        window.press("ctrl-a", cx);
        window.input(text, cx);
        if let Some(key) = key {
            window.press(key, cx);
        }
    })
    .unwrap();
}

fn has_id(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> bool {
    cx.update_window(handle, |_, window, _| window.try_find(id).is_some())
        .unwrap()
}

#[gpui_kit::test]
async fn the_detail_panel_highlights_changed_characters(cx: &mut TestAppContext) {
    let (handle, view) = open_text(cx, "id,name\n1,บ้านปี\n2,x\n", "id,name\n1,บ้านปู\n2,x\n");
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    assert!(
        view.read_with(cx, |v, _| v.detail_for_test()).is_none(),
        "no selection: the hint"
    );
    assert!(has_id(cx, handle, "table-detail"));
    view.update(cx, |v, cx| v.select(0, Some(1), false, cx));
    draw(cx, handle);
    let detail = view.read_with(cx, |v, _| v.detail_for_test()).unwrap();
    assert_eq!(detail.pill, "Differs");
    assert_eq!(
        detail.head,
        "Left row 2 · Right row 2 · Column name · 1 cell differs: name"
    );
    assert_eq!(detail.left.text, "บ้านปี");
    assert_eq!(detail.right.text, "บ้านปู");
    let marked = |side: &duet_app::table::view::DetailSide| -> Vec<String> {
        side.marks
            .iter()
            .map(|r| side.text[r.clone()].to_owned())
            .collect()
    };
    assert_eq!(
        marked(&detail.left),
        ["ปี"],
        "whole graphemes, no lone vowel mark"
    );
    assert_eq!(marked(&detail.right), ["ปู"]);

    // Keys, a left-only column and a one-sided row.
    view.update(cx, |v, cx| v.set_prefs(|p| p.keys = "id".into(), cx));
    wait_table(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select(0, None, false, cx));
    let detail = view.read_with(cx, |v, _| v.detail_for_test()).unwrap();
    assert_eq!(
        detail.head,
        "Left row 2 · Right row 2 · Key 1 · 1 cell differs: name"
    );
    // No column: each side's whole row as CSV.
    assert_eq!(detail.left.text, "1,บ้านปี");
    assert_eq!(detail.right.text, "1,บ้านปู");
}

#[gpui_kit::test]
async fn the_detail_panel_names_missing_and_empty_values(cx: &mut TestAppContext) {
    let (handle, view) = open_text(cx, "id,a,l\n1,,x\n2,y,z\n", "id,a,r\n1,v,x\n");
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    let names: Vec<String> = view.read_with(cx, |v, _| {
        v.diff()
            .unwrap()
            .columns
            .iter()
            .map(|c| c.name.clone())
            .collect()
    });
    assert_eq!(names, ["id", "a", "r", "l"]);
    // Row 0 differs in a: empty on the left.
    view.update(cx, |v, cx| v.select(0, Some(1), false, cx));
    let detail = view.read_with(cx, |v, _| v.detail_for_test()).unwrap();
    assert_eq!(
        (detail.left.text.as_str(), detail.left.muted),
        ("(empty)", true)
    );
    assert_eq!(
        (detail.right.text.as_str(), detail.right.muted),
        ("v", false)
    );
    assert!(detail.left.marks.is_empty(), "marks only for two values");
    // A column only the left has: the right has no value.
    view.update(cx, |v, cx| v.select(0, Some(3), false, cx));
    let detail = view.read_with(cx, |v, _| v.detail_for_test()).unwrap();
    assert!(
        detail.head.contains("Column l (left only)"),
        "{}",
        detail.head
    );
    assert_eq!(detail.right.text, "(none)");
    // A left-only row.
    view.update(cx, |v, cx| v.select(1, Some(1), false, cx));
    let detail = view.read_with(cx, |v, _| v.detail_for_test()).unwrap();
    assert_eq!(detail.pill, "Left only");
    assert!(
        detail.head.starts_with("Left row 3 · Right row —"),
        "{}",
        detail.head
    );
    assert_eq!(detail.right.text, "(none)");
}

#[gpui_kit::test]
async fn the_detail_panel_shows_a_long_value_around_its_first_change(cx: &mut TestAppContext) {
    // Up to 4,000 characters a value is shown whole; past that, 80 characters around its
    // first change, which DirectWrite can draw and the strip can show (T-43).
    let big = format!("line1\nline2 {}", "x".repeat(100_000));
    let (handle, view) = open_text(cx, &format!("id,v\n1,\"{big}\"\n"), "id,v\n1,short\n");
    wait_table(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select(0, Some(1), false, cx));
    draw(cx, handle);
    let detail = view.read_with(cx, |v, _| v.detail_for_test()).unwrap();
    let shown = format!("line1\nline2 {}…", "x".repeat(68));
    assert_eq!(
        detail.left.text, shown,
        "from the first change, the line break kept"
    );
    assert_eq!(
        detail.left.note.as_deref(),
        Some("characters 1–80 of 100,012")
    );
    let marked: Vec<&str> = detail
        .left
        .marks
        .iter()
        .map(|r| &shown[r.clone()])
        .collect();
    assert_eq!(
        marked,
        [shown.trim_end_matches('…')],
        "marked whole up to the cut"
    );
    assert_eq!(
        (detail.right.text.as_str(), detail.right.note.as_deref()),
        ("short", None)
    );
    let medium = "y".repeat(4_000);
    let (handle, view) = open_text(cx, &format!("id,v\n1,{medium}\n"), "id,v\n1,z\n");
    wait_table(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select(0, Some(1), false, cx));
    let detail = view.read_with(cx, |v, _| v.detail_for_test()).unwrap();
    assert_eq!(
        (detail.left.text.len(), detail.left.note.as_deref()),
        (4_000, None)
    );
    assert!(!detail.left.text.contains('⏎'));
    // The panel is about three lines high and scrolls inside that.
    let height = cx
        .update_window(handle, |_, window, _| {
            f32::from(window.find("table-detail").bounds().size.height)
        })
        .unwrap();
    assert!(height < 140.0, "the panel stays small: {height}");
}

/// Four data rows: "foo" is in rows 0 and 2, "bar" only in the right side of row 1.
fn find_fixture() -> (&'static str, &'static str) {
    (
        "id,v\n1,Foo\n2,a\n3,foo\n4,d\n",
        "id,v\n1,Foo\n2,bar\n3,foo\n4,d\n",
    )
}

#[gpui_kit::test]
async fn find_walks_matches_and_wraps(cx: &mut TestAppContext) {
    let (left, right) = find_fixture();
    let (handle, view) = open_text(cx, left, right);
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    press(cx, handle, "ctrl-f");
    assert!(has_id(cx, handle, "table-find-input"));
    type_in_focus(cx, handle, "foo", Some("enter"));
    assert_eq!(
        selection(cx, &view),
        Some((0, Some(1))),
        "case-insensitive, first match"
    );
    // F3 inside the field still finds next.
    press(cx, handle, "f3");
    assert_eq!(selection(cx, &view), Some((2, Some(1))));
    press(cx, handle, "f3");
    assert_eq!(selection(cx, &view), Some((0, Some(1))), "wrapped");
    press(cx, handle, "shift-f3");
    assert_eq!(
        selection(cx, &view),
        Some((2, Some(1))),
        "backward wraps too"
    );
    // The right side is searched as well.
    type_in_focus(cx, handle, "BAR", Some("enter"));
    assert_eq!(selection(cx, &view), Some((1, Some(1))));
    // Esc in the field closes the bar; F3 then repeats the last query.
    press(cx, handle, "escape");
    assert!(!has_id(cx, handle, "table-find-bar"));
    assert!(selection(cx, &view).is_some(), "the selection stays");
    press(cx, handle, "f3");
    assert_eq!(
        selection(cx, &view),
        Some((1, Some(1))),
        "only one match, wrapped to itself"
    );
    assert!(!has_id(cx, handle, "table-find-bar"));
    // Not found.
    press(cx, handle, "ctrl-f");
    type_in_focus(cx, handle, "zz", Some("enter"));
    assert_eq!(notice(cx, &view).as_deref(), Some("Not found: zz."));
    assert_eq!(selection(cx, &view), Some((1, Some(1))), "nothing moved");
}

#[gpui_kit::test]
async fn f3_without_a_query_opens_the_bar_and_the_buttons_step(cx: &mut TestAppContext) {
    let (left, right) = find_fixture();
    let (handle, view) = open_text(cx, left, right);
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    press(cx, handle, "f3");
    assert!(
        has_id(cx, handle, "table-find-bar"),
        "no last query: F3 opens the bar"
    );
    assert_eq!(selection(cx, &view), None);
    type_in_focus(cx, handle, "foo", Some("enter"));
    assert_eq!(selection(cx, &view).map(|s| s.0), Some(0));
    click(cx, handle, "table-find-next");
    assert_eq!(selection(cx, &view).map(|s| s.0), Some(2));
    click(cx, handle, "table-find-prev");
    assert_eq!(selection(cx, &view).map(|s| s.0), Some(0));
    click(cx, handle, "table-find-close");
    assert!(!has_id(cx, handle, "table-find-bar"));
    // The toolbar button opens it again.
    click(cx, handle, "table-find");
    assert!(has_id(cx, handle, "table-find-bar"));
}

/// 200 data rows; the row at file line 150 (paired row 148) differs.
fn goto_fixture() -> (String, String) {
    let side = |changed: &str| {
        let mut text = String::from("id,v\n");
        for r in 0..200 {
            let v = if r == 148 { changed } else { "same" };
            text.push_str(&format!("{r},{v}\n"));
        }
        text
    };
    (side("a"), side("b"))
}

#[gpui_kit::test]
async fn go_to_line_selects_and_reveals(cx: &mut TestAppContext) {
    let (left, right) = goto_fixture();
    let (handle, view) = open_text(cx, &left, &right);
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    press(cx, handle, "ctrl-g");
    assert!(has_id(cx, handle, "table-goto"));
    type_in_focus(cx, handle, "4", Some("enter"));
    assert_eq!(
        selection(cx, &view),
        Some((2, None)),
        "line 4 is paired row 2"
    );
    assert!(
        !has_id(cx, handle, "table-goto"),
        "the bar closes after a jump"
    );
    // Far down: the row is revealed.
    press(cx, handle, "ctrl-g");
    type_in_focus(cx, handle, "150", Some("enter"));
    draw(cx, handle);
    assert_eq!(
        selection(cx, &view),
        Some((148, Some(1))),
        "first differing column"
    );
    let (top, visible, pos) = view.read_with(cx, |v, _| {
        (
            v.top_row(),
            v.visible_rows(),
            v.view().position(148).unwrap(),
        )
    });
    assert!((top..top + visible).contains(&pos), "{top} {visible} {pos}");
    // Under the Same filter the differing row is hidden: the filter goes to All.
    view.update(cx, |v, cx| v.set_prefs(|p| p.filter = Filter::Same, cx));
    view.update(cx, |v, cx| v.select(0, None, false, cx));
    assert_eq!(view.read_with(cx, |v, _| v.view().position(148)), None);
    press(cx, handle, "ctrl-g");
    type_in_focus(cx, handle, "150", Some("enter"));
    assert_eq!(view.read_with(cx, |v, _| v.prefs().filter), Filter::All);
    assert_eq!(selection(cx, &view), Some((148, Some(1))));
    // Bad input keeps the bar and says why.
    press(cx, handle, "ctrl-g");
    type_in_focus(cx, handle, "abc", Some("enter"));
    assert_eq!(notice(cx, &view).as_deref(), Some("Type a line number."));
    type_in_focus(cx, handle, "999", Some("enter"));
    assert_eq!(notice(cx, &view).as_deref(), Some("No row at line 999."));
    type_in_focus(cx, handle, "1", Some("enter"));
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("No row at line 1."),
        "the header is no row"
    );
    assert!(has_id(cx, handle, "table-goto"));
}

#[gpui_kit::test]
async fn table_keys_stay_out_of_inputs(cx: &mut TestAppContext) {
    let (left, right) = find_fixture();
    let (_dir, a, b) = write_pair(left.as_bytes(), right.as_bytes());
    let (handle, view) = open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| TableView::new(a, b, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    let log = record_events(cx, &view);
    let paths = view.read_with(cx, |v, _| {
        let (l, r) = v.paths();
        (l.to_owned(), r.to_owned())
    });

    for name in ["keys", "find", "goto"] {
        let input: Entity<InputState> = match name {
            "keys" => {
                type_keys(cx, handle, &view, "", None);
                view.read_with(cx, |v, _| v.keys_input_for_test())
            }
            "find" => {
                press(cx, handle, "ctrl-f");
                view.read_with(cx, |v, _| v.find_input_for_test().unwrap())
            }
            _ => {
                press(cx, handle, "ctrl-g");
                view.read_with(cx, |v, _| v.goto_input_for_test().unwrap())
            }
        };
        cx.update_window(handle, |_, window, cx| {
            window.press("ctrl-a", cx);
            window.press("backspace", cx);
        })
        .unwrap();
        for key in ["n", "p", "1", "ctrl-1", "f9"] {
            press(cx, handle, key);
        }
        let text = input.read_with(cx, |state, _| state.value().to_string());
        assert_eq!(
            text, "np1",
            "{name}: letters and digits typed, Ctrl+1 and F9 did nothing"
        );
        assert_eq!(selection(cx, &view), None, "{name}: no navigation");
        assert_eq!(
            view.read_with(cx, |v, _| v.prefs().filter),
            Filter::All,
            "{name}"
        );
        assert!(log.borrow().is_empty(), "{name}: no swap");
        view.read_with(cx, |v, _| {
            let (l, r) = v.paths();
            assert_eq!((l.to_owned(), r.to_owned()), paths, "{name}");
        });
    }

    // Enter in the key box applies the keys.
    type_keys(cx, handle, &view, "id", Some("enter"));
    assert_eq!(view.read_with(cx, |v, _| v.prefs().keys.clone()), "id");
}

#[gpui_kit::test]
async fn escape_closes_the_bar_then_clears_then_goes_back(cx: &mut TestAppContext) {
    let (left, right) = find_fixture();
    let (handle, view) = open_text(cx, left, right);
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    let log = record_events(cx, &view);
    click_cell(cx, handle, &view, 0, true, 1);
    assert!(selection(cx, &view).is_some());

    // The find bar closes first, from the field.
    press(cx, handle, "ctrl-f");
    press(cx, handle, "escape");
    assert!(!has_id(cx, handle, "table-find-bar"));
    assert!(
        selection(cx, &view).is_some(),
        "the bar went, the selection stayed"
    );
    // The go-to bar too.
    press(cx, handle, "ctrl-g");
    press(cx, handle, "escape");
    assert!(!has_id(cx, handle, "table-goto"));
    assert!(selection(cx, &view).is_some());
    // Keys are back on the rows: Esc clears the selection, then goes back.
    press(cx, handle, "escape");
    assert_eq!(selection(cx, &view), None);
    assert!(log.borrow().is_empty());
    press(cx, handle, "escape");
    assert_eq!(*log.borrow(), ["back"]);
}

#[gpui_kit::test]
async fn the_detail_head_pluralises_differing_cells(cx: &mut TestAppContext) {
    let (handle, view) = open_text(
        cx,
        "a,b,c
1,2,3
",
        "a,b,c
9,8,3
",
    );
    wait_table(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select(0, None, false, cx));
    let detail = view.read_with(cx, |v, _| v.detail_for_test()).unwrap();
    assert!(
        detail.head.ends_with(" · 2 cells differ: a, b"),
        "{}",
        detail.head
    );
}

// ---- Task 8: right-click menu, export, paste, open, typed paths and drop ----

type Opens = Rc<RefCell<Vec<(PathBuf, PathBuf)>>>;

fn collect_opens(cx: &mut TestAppContext, view: &Entity<TableView>) -> Opens {
    let asked: Opens = Default::default();
    let sink = asked.clone();
    cx.update(|cx| {
        cx.subscribe(view, move |_, e: &TableEvent, _| {
            if let TableEvent::Open { left, right } = e {
                sink.borrow_mut().push((left.clone(), right.clone()));
            }
        })
        .detach()
    });
    asked
}

fn clipboard(cx: &mut TestAppContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

/// A right press and release on `column` of one half of display row `pos`.
fn right_click_cell(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TableView>,
    pos: usize,
    left: bool,
    column: usize,
) {
    draw(cx, handle);
    let position = cx
        .update_window(handle, |_, window, cx| {
            let x = view
                .read(cx)
                .cell_x_for_test(left, column)
                .expect("the columns are known after a frame");
            let y = pos as f32 * duet_app::table::view::ROW_HEIGHT + 10.;
            window.find("table-rows").bounds().origin + point(px(x), px(y))
        })
        .unwrap();
    right_press_at(cx, handle, position);
    // The popup is open; Esc closes it, or it would swallow the next press.
    press(cx, handle, "escape");
}

/// A right press and release at a window point.
fn right_press_at(cx: &mut TestAppContext, handle: AnyWindowHandle, position: Point<Pixels>) {
    cx.update_window(handle, |_, window, cx| {
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

fn menu_items(cx: &mut TestAppContext, view: &Entity<TableView>) -> Vec<String> {
    view.read_with(cx, |v, _| {
        duet_app::ui::menu::outline(&duet_app::table::menu::row_menu(v))
    })
}

fn run_entries(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TableView>,
    entries: Vec<duet_app::ui::menu::MenuEntry>,
    label: &str,
) -> bool {
    let view = view.clone();
    let ran = cx
        .update_window(handle, |_, window, cx| {
            let focus = view.focus_handle(cx);
            duet_app::ui::menu::activate(entries, label, Some(&focus), window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    ran
}

fn run_item(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TableView>,
    label: &str,
) -> bool {
    let entries = view.read_with(cx, |v, _| duet_app::table::menu::row_menu(v));
    run_entries(cx, handle, view, entries, label)
}

const USE_V: &str = "Use “v” as the key column";
const CLEAR: &str = "Clear key columns (align by row order)";

#[gpui_kit::test]
async fn the_table_menu_matches_the_state(cx: &mut TestAppContext) {
    let (handle, view) = open_text(cx, "id,v\n1,a\n2,b\n3,c\n", "id,v\n1,a\n2,X\n");
    wait_table(cx, handle, &view).await;
    // A differing row, on the matched column "v".
    right_click_cell(cx, handle, &view, 1, true, 1);
    assert_eq!(selection(cx, &view), Some((1, Some(1))));
    assert_eq!(
        menu_items(cx, &view),
        [
            "Copy left value",
            "Copy right value",
            "Copy left row (CSV)",
            "Copy right row (CSV)",
            "-",
            USE_V,
            "(Add “v” to the key columns)",
            &format!("({CLEAR})"),
        ]
    );
    // A row only the left side has: the right items are disabled.
    right_click_cell(cx, handle, &view, 2, true, 1);
    assert_eq!(
        menu_items(cx, &view)[..4],
        [
            "Copy left value",
            "(Copy right value)",
            "Copy left row (CSV)",
            "(Copy right row (CSV))",
        ]
    );
    // "Use “v”" sets the key box and recompares.
    right_click_cell(cx, handle, &view, 1, true, 1);
    assert!(run_item(cx, handle, &view, USE_V));
    wait_table(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.prefs().keys.clone()), "v");
    let input = view.read_with(cx, |v, _| v.keys_input_for_test());
    draw(cx, handle);
    assert_eq!(input.read_with(cx, |i, _| i.value().to_string()), "v");
    assert!(
        status(cx, &view).contains(&"Paired by key: v".to_owned()),
        "{:?}",
        status(cx, &view)
    );
    // With keys set, "Add" is offered for another column and Clear is enabled.
    right_click_cell(cx, handle, &view, 0, true, 0);
    let items = menu_items(cx, &view);
    assert_eq!(items[5], "Use “id” as the key column");
    assert_eq!(items[6], "Add “id” to the key columns");
    assert_eq!(items[7], CLEAR);
    assert!(run_item(cx, handle, &view, "Add “id” to the key columns"));
    wait_table(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.prefs().keys.clone()), "v, id");
    // "v" is a key now, so it cannot be added again.
    right_click_cell(cx, handle, &view, 0, true, 1);
    assert_eq!(menu_items(cx, &view)[6], "(Add “v” to the key columns)");
    assert!(run_item(cx, handle, &view, CLEAR));
    wait_table(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.prefs().keys.clone()), "");
    draw(cx, handle);
    assert_eq!(input.read_with(cx, |i, _| i.value().to_string()), "");

    // A column only the left side has offers no key items.
    let (handle, view) = open_text(cx, "id,v,w\n1,a,x\n", "id,v\n1,b\n");
    wait_table(cx, handle, &view).await;
    right_click_cell(cx, handle, &view, 0, true, 2);
    assert_eq!(
        menu_items(cx, &view),
        [
            "Copy left value",
            "(Copy right value)",
            "Copy left row (CSV)",
            "Copy right row (CSV)",
            "-",
            &format!("({CLEAR})"),
        ]
    );
}

#[gpui_kit::test]
async fn a_key_token_falls_back_to_the_column_number(cx: &mut TestAppContext) {
    // Duplicate names (ignoring case), a comma and a number all need "#n".
    let (handle, view) = open_text(cx, "a,A,\"x,y\",7\n1,2,3,4\n", "a,A,\"x,y\",7\n1,2,3,5\n");
    wait_table(cx, handle, &view).await;
    for (column, expected) in [(0, "#1"), (1, "#2"), (2, "#3"), (3, "#4")] {
        right_click_cell(cx, handle, &view, 0, true, column);
        let entries = view.read_with(cx, |v, _| duet_app::table::menu::row_menu(v));
        let label = duet_app::ui::menu::outline(&entries)[5].clone();
        assert!(run_entries(cx, handle, &view, entries, &label));
        wait_table(cx, handle, &view).await;
        assert_eq!(
            view.read_with(cx, |v, _| v.prefs().keys.clone()),
            expected,
            "column {column}"
        );
    }
    // Without a header every column is a number.
    view.update(cx, |v, cx| v.set_prefs(|p| p.header = false, cx));
    wait_table(cx, handle, &view).await;
    right_click_cell(cx, handle, &view, 0, true, 1);
    let entries = view.read_with(cx, |v, _| duet_app::table::menu::row_menu(v));
    let label = duet_app::ui::menu::outline(&entries)[5].clone();
    assert!(run_entries(cx, handle, &view, entries, &label));
    wait_table(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.prefs().keys.clone()), "#2");
}

#[gpui_kit::test]
async fn copy_row_gives_csv_with_the_comma(cx: &mut TestAppContext) {
    let (handle, view) = open_text(cx, "k,v\nx,\"a,b\"\n", "k,v\nx,z\n");
    wait_table(cx, handle, &view).await;
    right_click_cell(cx, handle, &view, 0, true, 1);
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("before".into())));
    assert!(run_item(cx, handle, &view, "Copy left row (CSV)"));
    assert_eq!(clipboard(cx).as_deref(), Some("x,\"a,b\""));
    assert_eq!(notice(cx, &view).as_deref(), Some("Copied."));
    assert!(run_item(cx, handle, &view, "Copy left value"));
    assert_eq!(clipboard(cx).as_deref(), Some("a,b"));
    assert!(run_item(cx, handle, &view, "Copy right row (CSV)"));
    assert_eq!(clipboard(cx).as_deref(), Some("x,z"));
    assert!(run_item(cx, handle, &view, "Copy right value"));
    assert_eq!(clipboard(cx).as_deref(), Some("z"));

    // A tab-separated file still copies a comma-separated row.
    let (handle, view) = open_text(cx, "k\tv\nx\ty,z\n", "k\tv\nx\tq\n");
    wait_table(cx, handle, &view).await;
    right_click_cell(cx, handle, &view, 0, true, 1);
    assert!(run_item(cx, handle, &view, "Copy left row (CSV)"));
    assert_eq!(clipboard(cx).as_deref(), Some("x,\"y,z\""));
}

/// A two-sheet workbook whose first sheet holds `rows`: a cell that reads as a number is one.
fn write_workbook(dir: &Path, name: &str, rows: &[&[&str]]) -> PathBuf {
    let mut first = SheetSpec::new("S");
    for (r, row) in rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let cell = cell.parse::<f64>().map_or_else(|_| text(cell), Cell::Num);
            first.cells.push((r as u32 + 1, c as u32 + 1, cell));
        }
    }
    let mut second = SheetSpec::new("T");
    second.cells.push((1, 1, text("other")));
    let path = dir.join(name);
    std::fs::write(&path, workbook(&[first, second])).unwrap();
    path
}

#[gpui_kit::test]
async fn copy_row_on_a_workbook_gives_csv_of_what_the_sheet_shows(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let header: &[&str] = &["สินค้า", "n", "v"];
    let a = write_workbook(dir.path(), "a.xlsx", &[header, &["กาแฟ", "1.5", "a,b"]]);
    let b = write_workbook(dir.path(), "b.xlsx", &[header, &["กาแฟ", "2", "z"]]);
    let (handle, view) = open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| TableView::new(a, b, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    wait_table(cx, handle, &view).await;
    right_click_cell(cx, handle, &view, 0, true, 1);
    assert!(run_item(cx, handle, &view, "Copy left row (CSV)"));
    assert_eq!(clipboard(cx).as_deref(), Some("กาแฟ,1.5,\"a,b\""));
    assert!(run_item(cx, handle, &view, "Copy left value"));
    assert_eq!(clipboard(cx).as_deref(), Some("1.5"));
    assert!(run_item(cx, handle, &view, "Copy right row (CSV)"));
    assert_eq!(clipboard(cx).as_deref(), Some("กาแฟ,2,z"));
}

#[gpui_kit::test]
async fn paste_on_a_workbook_side_drops_its_picker_and_an_open_brings_it_back(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let rows: &[&[&str]] = &[&["id", "v"], &["1", "x"]];
    let a = write_workbook(dir.path(), "a.xlsx", rows);
    let b = write_workbook(dir.path(), "b.xlsx", rows);
    let c = write_workbook(dir.path(), "c.xlsx", rows);
    let (handle, _workspace, view) = open_workspace(cx, a.clone(), b).await;
    let paste = |cx: &mut TestAppContext| {
        cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("id,v\n1,x\n".into())));
        click(cx, handle, "table-paste-left");
    };
    let left = |cx: &mut TestAppContext| {
        view.read_with(cx, |v, _| {
            let side = v.side(true).unwrap();
            (side.name.clone(), side.book.is_some())
        })
    };
    draw(cx, handle);
    assert!(
        has_id(cx, handle, "table-sheet-left"),
        "a workbook's picker"
    );

    paste(cx);
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    assert_eq!(left(cx), ("Pasted text (left)".to_owned(), false));
    assert!(!has_id(cx, handle, "table-sheet-left"), "pasted: no picker");
    assert!(
        has_id(cx, handle, "table-sheet-right"),
        "the other side's stays"
    );
    assert!(view.read_with(cx, |v, _| v.side(false).unwrap().book.is_some()));

    // The Open button and a dropped file go the same way (`open_side`): first its own workbook...
    view.update(cx, |v, cx| v.drop_paths(std::slice::from_ref(&a), true, cx));
    cx.run_until_parked();
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    assert_eq!(left(cx), ("a.xlsx".to_owned(), true));
    assert!(has_id(cx, handle, "table-sheet-left"), "its own file: back");

    // ...then another one.
    paste(cx);
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    assert!(!has_id(cx, handle, "table-sheet-left"), "pasted again");
    view.update(cx, |v, cx| v.drop_paths(std::slice::from_ref(&c), true, cx));
    cx.run_until_parked();
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    assert_eq!(left(cx), ("c.xlsx".to_owned(), true));
    assert!(has_id(cx, handle, "table-sheet-left"), "another file: back");
}

async fn pick_export(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TableView>,
    label: &str,
    target: impl FnOnce(&Path) -> Option<PathBuf>,
) {
    // A fresh load clears the last export's notice, so the wait below sees this one.
    view.update(cx, |v, cx| v.reload(cx));
    wait_table(cx, handle, view).await;
    let entries = view.read_with(cx, |v, _| duet_app::table::menu::export_menu(v));
    assert!(run_entries(cx, handle, view, entries, label));
    assert!(cx.did_prompt_for_new_path(), "{label} asks where to save");
    cx.simulate_new_path_selection(target);
    let seen = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        seen.read(cx)
            .notice()
            .is_some_and(|n| n.starts_with("Exported: ") || n.starts_with("Can't export: "))
    })
    .await;
}

#[gpui_kit::test]
async fn export_writes_csv_and_html(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_table(cx, b"id,v\n1,a\n2,b\n", b"id,v\n1,a\n2,c\n");
    wait_table(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| duet_app::ui::menu::outline(
            &duet_app::table::menu::export_menu(v)
        )),
        ["Differences (CSV)", "HTML report"]
    );
    let folder = dir.path().to_path_buf();
    let csv = folder.join("out.csv");
    let target = csv.clone();
    pick_export(cx, handle, &view, "Differences (CSV)", move |start| {
        assert_eq!(start, folder, "the prompt starts in the left file's folder");
        Some(target)
    })
    .await;
    assert_eq!(
        notice(cx, &view),
        Some(format!("Exported: {}.", csv.display()))
    );
    let bytes = std::fs::read(&csv).unwrap();
    assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "UTF-8 BOM");
    assert!(String::from_utf8(bytes).unwrap().contains("\r\n"));

    let html = dir.path().join("out.html");
    let target = html.clone();
    pick_export(cx, handle, &view, "HTML report", move |_| Some(target)).await;
    let text = std::fs::read_to_string(&html).unwrap();
    assert!(text.contains("Table compare report"), "{text}");
    assert!(
        text.contains("a.csv (UTF-8, comma (,) (auto))"),
        "the report names each side with its encoding and delimiter"
    );

    // A folder that is not there is reported, not swallowed.
    let missing = dir.path().join("none").join("x.csv");
    pick_export(cx, handle, &view, "Differences (CSV)", move |_| {
        Some(missing)
    })
    .await;
    assert!(
        notice(cx, &view).is_some_and(|n| n.starts_with("Can't export: ") && n.ends_with('.')),
        "{:?}",
        notice(cx, &view)
    );
}

#[gpui_kit::test]
async fn export_is_disabled_without_a_comparison(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_table(cx, b"id\n1\n", b"id\n1\n");
    wait_table(cx, handle, &view).await;
    std::fs::remove_file(dir.path().join("b.csv")).unwrap();
    view.update(cx, |v, cx| v.reload(cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| duet_app::ui::menu::outline(
            &duet_app::table::menu::export_menu(v)
        )),
        ["(Differences (CSV))", "(HTML report)"]
    );
    // The failed side says what to do next, and keeps its file name.
    assert!(
        status(cx, &view)
            .contains(&"b.csv could not be read. Open another file to start comparing.".to_owned()),
        "{:?}",
        status(cx, &view)
    );
    assert_eq!(view.read_with(cx, |v, _| v.name_text(false)), "b.csv");
}

#[gpui_kit::test]
async fn a_cancelled_open_leaves_the_label_with_the_tab_name(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_table(
        cx, b"id
1
", b"id
1
",
    );
    wait_table(cx, handle, &view).await;
    let (c, b) = (dir.path().join("c.csv"), dir.path().join("b.csv"));
    std::fs::write(
        &c, b"id
1
",
    )
    .unwrap();
    view.update(cx, |v, cx| v.load_paths(c, b, cx));
    press(cx, handle, "escape");
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        assert!(!v.is_busy());
        assert_eq!(v.paths().0.file_name().unwrap(), "c.csv");
        assert_eq!(
            v.name_text(true),
            "c.csv",
            "the label names the file the tab does"
        );
    });
}

#[gpui_kit::test]
async fn paste_replaces_a_side_and_reload_keeps_it(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_table(cx, b"id,v\n1,a\n", b"id,v\n1,a\n");
    wait_table(cx, handle, &view).await;
    let left_path = view.read_with(cx, |v, _| v.paths().0.to_path_buf());
    draw(cx, handle);
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(String::new())));
    click(cx, handle, "table-paste-left");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("The clipboard has no text.")
    );
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("id\n1\n".into())));
    click(cx, handle, "table-paste-left");
    wait_table(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let side = v.side(true).unwrap();
        assert_eq!(side.name, "Pasted text (left)");
        assert_eq!(side.path, None);
        assert_eq!(
            v.paths().0,
            left_path,
            "the tab keeps its path for the side"
        );
        assert!(!v.diff().unwrap().is_identical());
    });
    // F5 re-reads the file side only.
    std::fs::write(dir.path().join("b.csv"), b"id\n1\n").unwrap();
    press(cx, handle, "f5");
    wait_table(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.side(true).unwrap().name, "Pasted text (left)");
        assert!(
            v.diff().unwrap().is_identical(),
            "the right file was read again"
        );
    });
    // The right side can be pasted too.
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("id\n2\n".into())));
    click(cx, handle, "table-paste-right");
    wait_table(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.side(false).unwrap().name, "Pasted text (right)");
        assert!(!v.diff().unwrap().is_identical());
    });
}

#[gpui_kit::test]
async fn ctrl_o_and_ctrl_shift_o_ask_for_a_file_for_that_side(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_table(cx, b"id\n1\n", b"id\n2\n");
    wait_table(cx, handle, &view).await;
    let other = dir.path().join("c.csv");
    std::fs::write(&other, b"id\n3\n").unwrap();
    let asked = collect_opens(cx, &view);
    let (l, r) = view.read_with(cx, |v, _| {
        (v.paths().0.to_path_buf(), v.paths().1.to_path_buf())
    });
    // A field with the keys keeps Ctrl+O.
    type_keys(cx, handle, &view, "id", None);
    press(cx, handle, "ctrl-o");
    assert!(!cx.did_prompt_for_paths(), "the key box keeps the key");
    cx.update_window(handle, |_, window, cx| {
        window.focus(&view.focus_handle(cx), cx)
    })
    .unwrap();
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
    // The toolbar buttons ask the same.
    draw(cx, handle);
    click(cx, handle, "table-open-left");
    assert!(cx.did_prompt_for_paths());
}

#[gpui_kit::test]
async fn a_typed_path_replaces_a_side_and_bad_paths_say_why(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_table(cx, b"id\n1\n", b"id\n2\n");
    wait_table(cx, handle, &view).await;
    let other = dir.path().join("c.csv");
    std::fs::write(&other, b"id\n3\n").unwrap();
    let asked = collect_opens(cx, &view);
    draw(cx, handle);
    click(cx, handle, "table-right-name");
    assert_eq!(
        view.read_with(cx, |v, _| v.path_edit_for_test()),
        Some(false)
    );
    let missing = dir.path().join("none.csv");
    for (typed, expected) in [
        (
            "rel.csv".to_owned(),
            "Type a full path, such as C:\\folder\\file.txt.".to_owned(),
        ),
        (String::new(), "Not a file: .".to_owned()),
        (
            missing.display().to_string(),
            format!("Not a file: {}.", missing.display()),
        ),
    ] {
        view.update(cx, |v, cx| v.set_path_edit_text_for_test(&typed, cx));
        press(cx, handle, "enter");
        assert_eq!(notice(cx, &view), Some(expected));
    }
    assert!(asked.borrow().is_empty());
    view.update(cx, |v, cx| {
        v.set_path_edit_text_for_test(other.to_str().unwrap(), cx)
    });
    press(cx, handle, "enter");
    cx.run_until_parked();
    let l = view.read_with(cx, |v, _| v.paths().0.to_path_buf());
    assert_eq!(asked.borrow().as_slice(), [(l, other)]);
    assert_eq!(view.read_with(cx, |v, _| v.path_edit_for_test()), None);

    // Esc leaves the box without opening or going back.
    let backs = record_events(cx, &view);
    draw(cx, handle);
    click(cx, handle, "table-left-name");
    assert_eq!(
        view.read_with(cx, |v, _| v.path_edit_for_test()),
        Some(true)
    );
    press(cx, handle, "escape");
    assert_eq!(view.read_with(cx, |v, _| v.path_edit_for_test()), None);
    assert_eq!(asked.borrow().len(), 1);
    assert!(
        backs.borrow().iter().all(|e| e != "back"),
        "Esc only left the box"
    );
}

#[gpui_kit::test]
async fn a_typed_path_and_a_drop_replace_a_side(cx: &mut TestAppContext) {
    let (dir, a, b) = write_pair(b"id\n1\n", b"id\n2\n");
    let (c, d) = (dir.path().join("c.csv"), dir.path().join("d.csv"));
    std::fs::write(&c, b"id\n3\n").unwrap();
    std::fs::write(&d, b"id\n4\n").unwrap();
    let (handle, workspace, view) = open_workspace(cx, a.clone(), b.clone()).await;
    let paths =
        |cx: &mut TestAppContext| view.read_with(cx, |v, _| (name(v.paths().0), name(v.paths().1)));
    let pair = |l: &str, r: &str| (l.to_owned(), r.to_owned());

    // A typed path re-keys the tab.
    draw(cx, handle);
    click(cx, handle, "table-right-name");
    view.update(cx, |v, cx| {
        v.set_path_edit_text_for_test(c.to_str().unwrap(), cx)
    });
    press(cx, handle, "enter");
    cx.run_until_parked();
    wait_table(cx, handle, &view).await;
    assert_eq!(paths(cx), pair("a.csv", "c.csv"));
    assert_eq!(
        view.read_with(cx, |v, _| v.side(false).unwrap().name.clone()),
        "c.csv"
    );
    assert_eq!(workspace.read_with(cx, |w, _| w.tabs().len()), 1);

    // One dropped file lands on the half under the pointer.
    view.update(cx, |v, cx| v.drop_paths(std::slice::from_ref(&d), true, cx));
    cx.run_until_parked();
    wait_table(cx, handle, &view).await;
    assert_eq!(paths(cx), pair("d.csv", "c.csv"));
    // Two replace both.
    view.update(cx, |v, cx| v.drop_paths(&[a.clone(), b.clone()], false, cx));
    cx.run_until_parked();
    wait_table(cx, handle, &view).await;
    assert_eq!(paths(cx), pair("a.csv", "b.csv"));

    // A pair that is open in another tab is not opened twice.
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_table(c.clone(), d.clone(), None, window, cx)
        })
    })
    .unwrap();
    assert_eq!(workspace.read_with(cx, |w, _| w.tabs().len()), 2);
    view.update(cx, |v, cx| v.drop_paths(&[c.clone(), d.clone()], true, cx));
    cx.run_until_parked();
    assert_eq!(workspace.read_with(cx, |w, _| w.tabs().len()), 2);
    assert_eq!(paths(cx), pair("a.csv", "b.csv"));

    // A folder opens nothing and says so.
    view.update(cx, |v, cx| {
        v.drop_paths(&[dir.path().to_path_buf()], true, cx)
    });
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Drop files here; folders open in a folder compare.")
    );
}

// ---- Final review fix wave ----

#[gpui_kit::test]
async fn an_option_change_during_a_new_load_reads_the_new_file(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_table(cx, b"id,v\n1,a\n", b"id,v\n1,a\n");
    wait_table(cx, handle, &view).await;
    let a = dir.path().join("a.csv");
    let c = dir.path().join("c.csv");
    std::fs::write(&c, b"id,v\n1,a\n2,c\n").unwrap();
    // The option change lands while c.csv is still loading: it must not parse b.csv's text.
    view.update(cx, |v, cx| {
        v.load_paths(a, c, cx);
        v.set_prefs(|p| p.trim = true, cx);
    });
    wait_table(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.side(false).unwrap().name, "c.csv");
        assert_eq!(
            v.diff().unwrap().counts.right_only,
            1,
            "c.csv has a row more"
        );
    });
}

#[gpui_kit::test]
async fn an_option_change_right_after_f5_keeps_the_reload(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_table(cx, b"id,v\n1,a\n", b"id,v\n1,a\n");
    wait_table(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.diff().unwrap().is_identical()));
    std::fs::write(dir.path().join("b.csv"), b"id,v\n1,z\n").unwrap();
    press(cx, handle, "f5");
    view.update(cx, |v, cx| v.set_prefs(|p| p.ignore_case = true, cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(
        counts(cx, &view),
        (1, 0, 0),
        "the new b.csv content is compared"
    );
}

#[gpui_kit::test]
async fn find_pauses_at_its_cells_and_f3_or_enter_goes_on(cx: &mut TestAppContext) {
    // 30 rows of two columns; only row 22 holds the needle, in column 1.
    let mut text = String::from("k,v\n");
    for i in 0..30 {
        let v = if i == 22 { "a needle" } else { "hay" };
        text.push_str(&format!("{i},{v}\n"));
    }
    let (handle, view) = open_text(cx, &text, &text);
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    // 20 cells a call: five rows of both sides.
    view.update(cx, |v, _| v.set_find_cells_for_test(20));
    press(cx, handle, "ctrl-f");
    type_in_focus(cx, handle, "NEEDLE", Some("enter"));
    let still = Some("Still searching: press F3 to go on.");
    assert_eq!(notice(cx, &view).as_deref(), still);
    assert_eq!(
        selection(cx, &view),
        None,
        "nothing moves while it searches"
    );
    let mut calls = 1;
    while notice(cx, &view).as_deref() == still {
        assert!(calls < 20, "the search goes on");
        press(cx, handle, "f3");
        calls += 1;
    }
    assert_eq!(calls, 5, "F3 goes on from where the last call stopped");
    assert_eq!(selection(cx, &view), Some((22, Some(1))));
    assert_eq!(notice(cx, &view), None);

    // A miss visits each row once across the calls (Enter goes on too), then says so.
    type_in_focus(cx, handle, "zz", Some("enter"));
    let mut calls = 1;
    while notice(cx, &view).as_deref() == still {
        assert!(calls < 20, "the search ends");
        press(cx, handle, "enter");
        calls += 1;
    }
    assert_eq!(calls, 6, "60 columns, ten a call");
    assert_eq!(notice(cx, &view).as_deref(), Some("Not found: zz."));
    assert_eq!(selection(cx, &view), Some((22, Some(1))), "nothing moved");
}

#[gpui_kit::test]
async fn a_right_click_below_the_rows_opens_no_menu_and_keeps_the_selection(
    cx: &mut TestAppContext,
) {
    let (handle, view) = open_text(cx, "id,v\n1,a\n2,b\n", "id,v\n1,a\n2,X\n");
    wait_table(cx, handle, &view).await;
    right_click_cell(cx, handle, &view, 1, true, 1);
    assert_eq!(selection(cx, &view), Some((1, Some(1))));
    assert!(!menu_items(cx, &view).is_empty());
    // The two rows fill 44 px of a much taller body: press near its bottom, on the left half.
    let below = cx
        .update_window(handle, |_, window, _| {
            let b = window.find("table-rows").bounds();
            point(b.left() + px(120.), b.bottom() - px(4.))
        })
        .unwrap();
    right_press_at(cx, handle, below);
    assert!(
        menu_items(cx, &view).is_empty(),
        "a stale target would open a menu for row 1"
    );
    assert_eq!(selection(cx, &view), Some((1, Some(1))));
    // T-62: no popup takes the keys, so Up and Down still move the selection.
    press(cx, handle, "up");
    assert_eq!(selection(cx, &view), Some((0, Some(1))));
    // The same after a right press on the "#" over the row numbers, which names no column.
    let gutter = cx
        .update_window(handle, |_, window, _| {
            window.find("table-head").bounds().origin + point(px(4.), px(10.))
        })
        .unwrap();
    right_press_at(cx, handle, gutter);
    press(cx, handle, "down");
    assert_eq!(selection(cx, &view), Some((1, Some(1))));
}

/// The window point over `column`'s name on one half of the header.
fn head_point(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TableView>,
    left: bool,
    column: usize,
) -> Point<Pixels> {
    draw(cx, handle);
    cx.update_window(handle, |_, window, cx| {
        let x = view
            .read(cx)
            .cell_x_for_test(left, column)
            .expect("the columns are known after a frame");
        window.find("table-head").bounds().origin + point(px(x), px(10.))
    })
    .unwrap()
}

fn head_items(cx: &mut TestAppContext, view: &Entity<TableView>) -> Vec<String> {
    view.read_with(cx, |v, _| {
        duet_app::ui::menu::outline(&duet_app::table::menu::head_menu(v))
    })
}

#[gpui_kit::test]
async fn a_right_click_on_a_column_name_offers_the_key_items(cx: &mut TestAppContext) {
    // "w" is only on the left.
    let (handle, view) = open_text(cx, "id,v,w\n1,a,x\n2,b,y\n", "id,v\n1,a\n2,X\n");
    wait_table(cx, handle, &view).await;
    let at = head_point(cx, handle, &view, true, 1);
    right_press_at(cx, handle, at);
    press(cx, handle, "escape");
    assert_eq!(
        head_items(cx, &view),
        [USE_V, "(Add “v” to the key columns)", &format!("({CLEAR})")]
    );
    assert_eq!(selection(cx, &view), None, "a column name selects no cell");
    assert!(
        menu_items(cx, &view).is_empty(),
        "the row menu is not this one"
    );
    let entries = view.read_with(cx, |v, _| duet_app::table::menu::head_menu(v));
    assert!(run_entries(cx, handle, &view, entries, USE_V));
    wait_table(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.prefs().keys.clone()), "v");

    // The right half's name, with a key and nothing selected: Add and Clear are on (T-51).
    assert_eq!(selection(cx, &view), None);
    let at = head_point(cx, handle, &view, false, 0);
    right_press_at(cx, handle, at);
    press(cx, handle, "escape");
    assert_eq!(
        head_items(cx, &view),
        [
            "Use “id” as the key column",
            "Add “id” to the key columns",
            CLEAR
        ]
    );
    let entries = view.read_with(cx, |v, _| duet_app::table::menu::head_menu(v));
    assert!(run_entries(
        cx,
        handle,
        &view,
        entries,
        "Add “id” to the key columns"
    ));
    wait_table(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.prefs().keys.clone()), "v, id");

    // A one-sided column offers only Clear, on its name and on the other half's gap.
    for left in [true, false] {
        let at = head_point(cx, handle, &view, left, 2);
        right_press_at(cx, handle, at);
        press(cx, handle, "escape");
        assert_eq!(head_items(cx, &view), [CLEAR], "left half: {left}");
    }
    // The "#" over the row numbers names no column.
    let at = cx
        .update_window(handle, |_, window, _| {
            window.find("table-head").bounds().origin + point(px(4.), px(10.))
        })
        .unwrap();
    right_press_at(cx, handle, at);
    assert!(head_items(cx, &view).is_empty());
    assert!(
        duet_app::table::view::DETAIL_HINT
            .ends_with(" · Right-click a column header to use it as the key."),
        "the hint says so"
    );
}

fn export_items(cx: &mut TestAppContext, view: &Entity<TableView>) -> Vec<String> {
    view.read_with(cx, |v, _| {
        duet_app::ui::menu::outline(&duet_app::table::menu::export_menu(v))
    })
}

#[gpui_kit::test]
async fn export_waits_for_the_result_of_the_shown_sides_and_options(cx: &mut TestAppContext) {
    let (handle, view) = open_text(cx, "id,v\n1,a\n", "id,v\n1,b\n2,c\n");
    wait_table(cx, handle, &view).await;
    let ready = ["Differences (CSV)", "HTML report"];
    let off = ["(Differences (CSV))", "(HTML report)"];
    assert_eq!(export_items(cx, &view), ready);
    // Esc stops the swapped compare: the old result shows, but the sides changed places.
    press(cx, handle, "f9");
    press(cx, handle, "escape");
    assert!(!view.read_with(cx, |v, _| v.is_busy()));
    assert!(
        view.read_with(cx, |v, _| v.diff().is_some()),
        "the old result stays"
    );
    assert_eq!(export_items(cx, &view), off);
    press(cx, handle, "f5");
    wait_table(cx, handle, &view).await;
    assert_eq!(export_items(cx, &view), ready);
    // The same for an option change.
    view.update(cx, |v, cx| v.set_prefs(|p| p.trim = true, cx));
    press(cx, handle, "escape");
    assert_eq!(export_items(cx, &view), off);
}

#[gpui_kit::test]
async fn a_side_that_fails_to_open_leaves_no_parsing_text(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_table(cx, b"id\n1\n", b"id\n1\n");
    wait_table(cx, handle, &view).await;
    std::fs::remove_file(dir.path().join("b.csv")).unwrap();
    view.update(cx, |v, cx| {
        v.reload(cx);
        v.show_progress_for_test("Parsing a.csv… 5 rows");
    });
    wait_table(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.message(false).is_some()));
    assert_eq!(notice(cx, &view), None);
}

#[gpui_kit::test]
async fn an_open_or_drop_replaces_the_pasted_sides_it_names(cx: &mut TestAppContext) {
    let (dir, a, b) = write_pair(b"id\n1\n", b"id\n2\n");
    let c = dir.path().join("c.csv");
    std::fs::write(&c, b"id\n3\n").unwrap();
    let (handle, _workspace, view) = open_workspace(cx, a.clone(), b.clone()).await;
    let names = |cx: &mut TestAppContext| {
        view.read_with(cx, |v, _| {
            (
                v.side(true).unwrap().name.clone(),
                v.side(false).unwrap().name.clone(),
            )
        })
    };
    let pair = |l: &str, r: &str| (l.to_owned(), r.to_owned());
    let paste_left = |cx: &mut TestAppContext| {
        cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("id\n9\n".into())));
        click(cx, handle, "table-paste-left");
    };
    draw(cx, handle);
    paste_left(cx);
    wait_table(cx, handle, &view).await;
    assert_eq!(names(cx), pair("Pasted text (left)", "b.csv"));

    // Two dropped files, one the pasted side's own path: both sides are files again.
    view.update(cx, |v, cx| v.drop_paths(&[a.clone(), c.clone()], true, cx));
    cx.run_until_parked();
    wait_table(cx, handle, &view).await;
    assert_eq!(names(cx), pair("a.csv", "c.csv"));

    // A file for one side leaves the other side's paste, even when it is that side's file.
    paste_left(cx);
    wait_table(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.drop_paths(std::slice::from_ref(&c), false, cx)
    });
    cx.run_until_parked();
    wait_table(cx, handle, &view).await;
    assert_eq!(names(cx), pair("Pasted text (left)", "c.csv"));
    view.update(cx, |v, cx| {
        v.drop_paths(std::slice::from_ref(&b), false, cx)
    });
    cx.run_until_parked();
    wait_table(cx, handle, &view).await;
    assert_eq!(names(cx), pair("Pasted text (left)", "b.csv"));
    // The pasted side's own file, opened on that side, replaces the paste.
    view.update(cx, |v, cx| v.drop_paths(std::slice::from_ref(&a), true, cx));
    cx.run_until_parked();
    wait_table(cx, handle, &view).await;
    assert_eq!(names(cx), pair("a.csv", "b.csv"));
}
