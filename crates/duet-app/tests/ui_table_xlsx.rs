//! Headless UI tests for Excel workbooks in the table view: the load flow, the sheet picks, the
//! refusals, the wording and the picker. Every workbook is written by the test fixture.

use std::path::{Path, PathBuf};
use std::time::Duration;

use duet_app::table::view::TableView;
use duet_core::table::Delimiter;
use duet_core::table::nav::Filter;
use duet_core::table::xlsx::fixture::{Cell, SheetSpec, protected_bytes, text, workbook};
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, ClipboardItem, Entity, Focusable, Point, Render,
    TestAppContext, Window, WindowBounds, WindowOptions, px, size,
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
            size: size(px(1400.), px(800.)),
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

async fn wait_table(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<TableView>) {
    let view = view.clone();
    cx.wait_for(handle, Duration::from_secs(30), move |_, cx| {
        !view.read(cx).is_busy()
    })
    .await;
}

fn press(cx: &mut TestAppContext, handle: AnyWindowHandle, key: &str) {
    cx.update_window(handle, |_, window, cx| window.press(key, cx))
        .unwrap();
}

fn draw(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn has_id(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> bool {
    cx.update_window(handle, |_, window, _| window.try_find(id).is_some())
        .unwrap()
}

/// A sheet of rows from A1: numbers where the text reads as one, text otherwise.
fn grid(name: &str, rows: &[&[&str]]) -> SheetSpec {
    let mut spec = SheetSpec::new(name);
    for (r, row) in rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let cell = cell.parse::<f64>().map_or_else(|_| text(cell), Cell::Num);
            spec.cells.push((r as u32 + 1, c as u32 + 1, cell));
        }
    }
    spec
}

fn write_book(dir: &Path, name: &str, sheets: &[SheetSpec]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, workbook(sheets)).unwrap();
    path
}

fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn open_view(
    cx: &mut TestAppContext,
    left: PathBuf,
    right: PathBuf,
) -> (AnyWindowHandle, Entity<TableView>) {
    open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| TableView::new(left, right, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    })
}

async fn open_loaded(
    cx: &mut TestAppContext,
    left: PathBuf,
    right: PathBuf,
) -> (AnyWindowHandle, Entity<TableView>) {
    let (handle, view) = open_view(cx, left, right);
    wait_table(cx, handle, &view).await;
    (handle, view)
}

fn notice(cx: &mut TestAppContext, view: &Entity<TableView>) -> Option<String> {
    view.read_with(cx, |v, _| v.notice().map(str::to_owned))
}

fn differing_rows(cx: &mut TestAppContext, view: &Entity<TableView>) -> usize {
    view.read_with(cx, |v, _| {
        let counts = v.diff().expect("a result").counts;
        counts.differs + counts.left_only + counts.right_only
    })
}

fn ptr(cx: &mut TestAppContext, view: &Entity<TableView>, left: bool) -> Option<usize> {
    view.read_with(cx, |v, _| v.table_ptr_for_test(left))
}

fn shown(cx: &mut TestAppContext, view: &Entity<TableView>, left: bool) -> Option<usize> {
    view.read_with(cx, |v, _| v.shown_sheet(left))
}

fn q_book(dir: &Path, name: &str, sheets: &[(&str, &[&[&str]])]) -> PathBuf {
    let specs: Vec<SheetSpec> = sheets.iter().map(|(n, rows)| grid(n, rows)).collect();
    write_book(dir, name, &specs)
}

#[gpui_kit::test]
async fn two_workbooks_open_and_compare_cell_by_cell(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = q_book(
        dir.path(),
        "a.xlsx",
        &[("Data", &[&["id", "qty"], &["1", "10"], &["2", "20"]])],
    );
    let b = q_book(
        dir.path(),
        "b.xlsx",
        &[("Data", &[&["id", "qty"], &["1", "10"], &["2", "25"]])],
    );
    let (_handle, view) = open_loaded(cx, a, b).await;
    assert_eq!(differing_rows(cx, &view), 1);
    view.read_with(cx, |v, _| {
        let book = v.side(true).unwrap().book.as_ref().unwrap();
        assert_eq!(book.sheets.len(), 1);
        assert_eq!(book.sheets[0].name, "Data");
        assert_eq!(v.badge().as_deref(), Some("1"));
        assert_eq!(v.diff().unwrap().counts.same, 1);
    });
}

#[gpui_kit::test]
async fn the_default_sheets_share_a_name(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = q_book(
        dir.path(),
        "a.xlsx",
        &[
            ("Notes", &[&["n"], &["x"]]),
            ("Q1", &[&["id", "v"], &["1", "5"]]),
            ("Q2", &[&["id", "v"], &["1", "9"]]),
        ],
    );
    let b = q_book(
        dir.path(),
        "b.xlsx",
        &[
            ("Q1", &[&["id", "v"], &["1", "6"]]),
            ("Q2", &[&["id", "v"], &["1", "9"]]),
        ],
    );
    let (_handle, view) = open_loaded(cx, a, b).await;
    assert_eq!(shown(cx, &view, true), Some(1));
    assert_eq!(shown(cx, &view, false), Some(0));
    assert_eq!(differing_rows(cx, &view), 1, "Q1 against Q1");
}

fn three_sheets() -> [(&'static str, &'static [&'static [&'static str]]); 3] {
    [
        ("Q1", &[&["id", "v"], &["1", "1"]]),
        ("Q2", &[&["id", "v"], &["1", "2"]]),
        ("Q3", &[&["id", "v"], &["1", "3"]]),
    ]
}

#[gpui_kit::test]
async fn a_pick_changes_only_that_side(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = q_book(dir.path(), "a.xlsx", &three_sheets());
    let b = q_book(dir.path(), "b.xlsx", &three_sheets());
    let (handle, view) = open_loaded(cx, a, b).await;
    assert_eq!(differing_rows(cx, &view), 0);
    let right_before = ptr(cx, &view, false);
    let left_before = ptr(cx, &view, true);
    view.update(cx, |v, cx| v.pick_sheet(true, 2, cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(shown(cx, &view, true), Some(2));
    assert_eq!(shown(cx, &view, false), Some(0));
    assert_eq!(
        ptr(cx, &view, false),
        right_before,
        "the right side was kept"
    );
    assert_ne!(ptr(cx, &view, true), left_before, "the left side was read");
    assert_eq!(differing_rows(cx, &view), 1, "Q3 against Q1");
    // The sheet already shown, and one that is not there, change nothing.
    view.update(cx, |v, cx| v.pick_sheet(true, 2, cx));
    view.update(cx, |v, cx| v.pick_sheet(true, 9, cx));
    assert!(!view.read_with(cx, |v, _| v.is_busy()));
}

#[gpui_kit::test]
async fn f5_keeps_a_pick_and_swap_moves_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = q_book(dir.path(), "a.xlsx", &three_sheets());
    let b = q_book(dir.path(), "b.xlsx", &three_sheets());
    let (handle, view) = open_loaded(cx, a, b).await;
    view.update(cx, |v, cx| v.pick_sheet(true, 2, cx));
    wait_table(cx, handle, &view).await;
    view.update(cx, |v, cx| v.reload(cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(shown(cx, &view, true), Some(2), "F5 keeps the pick");
    view.update(cx, |v, cx| v.swap_sides(cx));
    assert_eq!(
        shown(cx, &view, false),
        Some(2),
        "the pick moved with its side, at once"
    );
    assert_eq!(shown(cx, &view, true), Some(0));
    cx.run_until_parked();
    wait_table(cx, handle, &view).await;
    // The swap only paired the rows again: both tables are the ones read before.
    view.update(cx, |v, cx| v.reload(cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(shown(cx, &view, false), Some(2), "and F5 after it finds it");
}

#[gpui_kit::test]
async fn a_vanished_pick_falls_back_with_a_notice(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let two = [
        ("Q1", &[&["id"][..], &["1"]][..]),
        ("Q2", &[&["id"][..], &["2"]][..]),
    ];
    let a = q_book(dir.path(), "a.xlsx", &two);
    let b = q_book(dir.path(), "b.xlsx", &two);
    let (handle, view) = open_loaded(cx, a.clone(), b).await;
    view.update(cx, |v, cx| v.pick_sheet(true, 1, cx));
    wait_table(cx, handle, &view).await;
    q_book(dir.path(), "a.xlsx", &two[..1]);
    view.update(cx, |v, cx| v.reload(cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(shown(cx, &view, true), Some(0));
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Sheet “Q2” is no longer in a.xlsx. Showing “Q1”.")
    );
    view.update(cx, |v, cx| v.reload(cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(notice(cx, &view), None, "said once");
}

#[gpui_kit::test]
async fn opening_another_file_clears_the_pick(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = q_book(dir.path(), "a.xlsx", &three_sheets());
    let b = q_book(dir.path(), "b.xlsx", &three_sheets());
    let c = q_book(dir.path(), "c.xlsx", &three_sheets());
    let (handle, view) = open_loaded(cx, a, b.clone()).await;
    view.update(cx, |v, cx| v.pick_sheet(true, 2, cx));
    wait_table(cx, handle, &view).await;
    view.update(cx, |v, cx| v.load_paths(c, b, cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(
        shown(cx, &view, true),
        Some(0),
        "the new file starts on its default"
    );
}

#[gpui_kit::test]
async fn a_workbook_against_a_csv_compares(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = q_book(
        dir.path(),
        "a.xlsx",
        &[("S", &[&["id", "name"], &["1", "Ann"]])],
    );
    let same = write_file(dir.path(), "b.csv", b"id,name\n1,Ann\n");
    let (handle, view) = open_loaded(cx, a.clone(), same).await;
    assert!(view.read_with(cx, |v, _| v.diff().unwrap().is_identical()));
    assert_eq!(view.read_with(cx, |v, _| v.badge()).as_deref(), Some("ok"));
    assert!(view.read_with(cx, |v, _| v.side(false).unwrap().book.is_none()));
    let other = write_file(dir.path(), "c.csv", b"id,name\n1,Bob\n");
    view.update(cx, |v, cx| v.load_paths(a, other, cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(differing_rows(cx, &view), 1);
}

/// The text marker: a cell that reads like another kind of cell carries `'` between two
/// workbooks and does not beside a CSV file, which has no cell types.
#[gpui_kit::test]
async fn the_text_marker_is_for_two_workbooks_only(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut text_one = SheetSpec::new("S");
    text_one.cells.push((1, 1, text("v")));
    text_one.cells.push((2, 1, text("1")));
    let mut number_one = SheetSpec::new("S");
    number_one.cells.push((1, 1, text("v")));
    number_one.cells.push((2, 1, Cell::Num(1.0)));
    let text_book = write_book(dir.path(), "text.xlsx", &[text_one]);
    let number_book = write_book(dir.path(), "number.xlsx", &[number_one]);
    let csv = write_file(dir.path(), "one.csv", b"v\n1\n");

    // Workbook against workbook: text 1 and number 1 differ.
    let (handle, view) = open_loaded(cx, text_book.clone(), number_book.clone()).await;
    assert_eq!(differing_rows(cx, &view), 1, "text 1 against number 1");
    assert_eq!(
        view.read_with(cx, |v, _| v.marks_text_for_test(true)),
        Some(true)
    );
    // "Compare numbers by value" sees through the marker: '1 is the number 1.
    view.update(cx, |v, cx| v.set_prefs(|p| p.numbers = true, cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(differing_rows(cx, &view), 0, "'1 is 1 by value");
    view.update(cx, |v, cx| v.set_prefs(|p| p.numbers = false, cx));
    wait_table(cx, handle, &view).await;

    // Text 1 in a workbook against 1 in a CSV: the same. Opening the CSV re-reads the
    // workbook without the marker.
    view.update(cx, |v, cx| v.load_paths(text_book.clone(), csv.clone(), cx));
    wait_table(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.diff().unwrap().is_identical()));
    assert_eq!(
        view.read_with(cx, |v, _| v.marks_text_for_test(true)),
        Some(false)
    );
    // Number 1 against the CSV's 1: the same.
    view.update(cx, |v, cx| {
        v.load_paths(number_book.clone(), csv.clone(), cx)
    });
    wait_table(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.diff().unwrap().is_identical()));
    // F9 keeps the pair mixed and the reading as it is.
    let before = ptr(cx, &view, false);
    view.update(cx, |v, cx| v.swap_sides(cx));
    wait_table(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.diff().unwrap().is_identical()));
    assert_eq!(ptr(cx, &view, true), before);
    assert_eq!(
        view.read_with(cx, |v, _| v.marks_text_for_test(false)),
        Some(false)
    );
    // Open left: a workbook on the CSV side makes the pair two workbooks again, and both
    // sides read with the marker.
    view.update(cx, |v, cx| v.load_paths(csv.clone(), text_book.clone(), cx));
    wait_table(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.load_paths(number_book.clone(), text_book.clone(), cx)
    });
    wait_table(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.marks_text_for_test(true)),
        Some(true)
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.marks_text_for_test(false)),
        Some(true)
    );
    assert_eq!(differing_rows(cx, &view), 1);
}

#[gpui_kit::test]
async fn pasting_on_one_side_of_two_workbooks_reads_the_other_without_the_marker(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    sheet.cells.push((1, 1, text("v")));
    sheet.cells.push((2, 1, text("1")));
    let a = write_book(dir.path(), "a.xlsx", &[sheet.clone()]);
    let b = write_book(dir.path(), "b.xlsx", &[sheet]);
    let (handle, view) = open_loaded(cx, a, b).await;
    draw(cx, handle);
    assert_eq!(
        view.read_with(cx, |v, _| v.marks_text_for_test(false)),
        Some(true)
    );
    let kept = ptr(cx, &view, true);
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("v\n1\n".into())));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("table-paste-left", cx);
    })
    .unwrap();
    wait_table(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert!(
            v.side(true).unwrap().book.is_none(),
            "the pasted side is CSV"
        );
        assert_eq!(
            v.marks_text_for_test(false),
            Some(false),
            "the workbook was read again"
        );
        assert!(v.diff().unwrap().is_identical(), "pasted 1 against text 1");
    });
    assert_ne!(ptr(cx, &view, true), kept);
    assert!(
        !has_id(cx, handle, "table-sheet-left"),
        "no picker on a pasted side"
    );
}

#[gpui_kit::test]
async fn a_delimiter_change_does_not_read_a_workbook_again(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = q_book(dir.path(), "a.xlsx", &[("S", &[&["id", "v"], &["1", "a"]])]);
    let b = write_file(dir.path(), "b.csv", b"id;v\n1;a\n");
    let (handle, view) = open_loaded(cx, a, b).await;
    let (book, csv) = (ptr(cx, &view, true), ptr(cx, &view, false));
    view.update(cx, |v, cx| {
        v.set_prefs(|p| p.delimiter = Some(Delimiter::Semicolon), cx)
    });
    wait_table(cx, handle, &view).await;
    assert_eq!(ptr(cx, &view, true), book, "the workbook side was kept");
    assert_ne!(ptr(cx, &view, false), csv, "the CSV side was parsed again");
    assert!(view.read_with(cx, |v, _| v.diff().is_some()));
}

#[gpui_kit::test]
async fn a_protected_file_names_the_file_and_the_other_side_loads(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let locked = write_file(dir.path(), "locked.xlsx", &protected_bytes());
    let good = q_book(dir.path(), "good.xlsx", &[("S", &[&["id"], &["1"]])]);
    let (_handle, view) = open_loaded(cx, locked, good).await;
    view.read_with(cx, |v, _| {
        let message = v.message(true).expect("a message");
        assert!(
            message.starts_with("Can't open locked.xlsx: is password protected."),
            "{message}"
        );
        assert!(v.side(false).is_some(), "the good side stays loaded");
        assert_eq!(v.badge().as_deref(), Some("!"));
    });
}

#[gpui_kit::test]
async fn other_refusals_name_the_file(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let good = q_book(dir.path(), "good.xlsx", &[("S", &[&["id"], &["1"]])]);
    let zip = duet_core::table::xlsx::fixture::zip(&[("readme.txt".to_owned(), b"hi".to_vec())]);
    let cases: [(&str, Vec<u8>, &str); 3] = [
        (
            "plain.xlsx",
            b"id,name\n1,Ann\n".to_vec(),
            "Can't open plain.xlsx: is not an Excel workbook (it is not a zip file).",
        ),
        (
            "book.xlsx",
            zip,
            "Can't open book.xlsx: is a zip file but not an Excel workbook.",
        ),
        (
            "empty.xlsx",
            workbook(&[]),
            "Can't open empty.xlsx: has no worksheets.",
        ),
    ];
    for (name, bytes, expected) in cases {
        let bad = write_file(dir.path(), name, &bytes);
        let (_handle, view) = open_loaded(cx, bad, good.clone()).await;
        view.read_with(cx, |v, _| {
            assert_eq!(v.message(true), Some(expected));
            assert!(v.side(false).is_some());
        });
    }
}

#[gpui_kit::test]
async fn a_huge_sheet_is_cut_with_a_notice(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut wide = SheetSpec::new("Data");
    for row in 1..=1000 {
        wide.cells.push((row, 1, text("x")));
        wide.cells.push((row, 16_384, text("y")));
    }
    let a = write_book(dir.path(), "a.xlsx", &[wide]);
    let b = q_book(dir.path(), "b.xlsx", &[("Data", &[&["x"]])]);
    let (_handle, view) = open_loaded(cx, a, b).await;
    let note = notice(cx, &view).expect("a notice");
    assert!(note.starts_with("Only the first "), "{note}");
    assert!(note.contains("Data") && note.contains("a.xlsx"), "{note}");
    assert!(view.read_with(cx, |v, _| v.diff().is_some()));
}

#[gpui_kit::test]
async fn formulas_without_results_say_so(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let formula = |t: &str| Cell::Formula {
        text: t.to_owned(),
        value: None,
    };
    let mut sheet = SheetSpec::new("S");
    sheet.cells.push((1, 1, text("v")));
    sheet.cells.push((2, 1, formula("1+1")));
    sheet.cells.push((3, 1, formula("2+2")));
    let a = write_book(dir.path(), "a.xlsx", &[sheet]);
    let b = q_book(dir.path(), "b.xlsx", &[("S", &[&["v"], &["2"]])]);
    let (_handle, view) = open_loaded(cx, a, b).await;
    let note = notice(cx, &view).expect("a notice");
    assert!(
        note.contains("2 formula cells have no saved result"),
        "{note}"
    );
}

#[gpui_kit::test]
async fn formulas_without_results_on_both_sides_are_said_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let formula = |t: &str| Cell::Formula {
        text: t.to_owned(),
        value: None,
    };
    let mut left = SheetSpec::new("S");
    left.cells.push((1, 1, text("v")));
    left.cells.push((2, 1, formula("1+1")));
    left.cells.push((3, 1, formula("2+2")));
    let mut right = SheetSpec::new("S");
    right.cells.push((1, 1, text("v")));
    right.cells.push((2, 1, formula("3+3")));
    let a = write_book(dir.path(), "a.xlsx", &[left]);
    let b = write_book(dir.path(), "b.xlsx", &[right]);
    let (_handle, view) = open_loaded(cx, a, b).await;
    let note = notice(cx, &view).expect("a notice");
    assert_eq!(
        note.matches("no saved result").count(),
        1,
        "once, not once per side: {note}"
    );
    assert!(
        note.contains("left 2") && note.contains("right 1"),
        "it names the sides: {note}"
    );
}

#[gpui_kit::test]
async fn esc_cancels_a_workbook_read(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    for row in 1..=2000 {
        sheet.cells.push((row, 1, text("x")));
    }
    let a = write_book(dir.path(), "a.xlsx", &[sheet.clone()]);
    let b = write_book(dir.path(), "b.xlsx", &[sheet]);
    let (handle, view) = open_view(cx, a, b);
    // The read runs on the background executor: the view is busy, and Esc stops it.
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    press(cx, handle, "escape");
    assert!(!view.read_with(cx, |v, _| v.is_busy()));
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Cancelled: the result is incomplete.")
    );
    cx.run_until_parked();
    assert!(
        view.read_with(cx, |v, _| v.diff().is_none()),
        "the late result is dropped"
    );
}

#[gpui_kit::test]
async fn keys_filters_find_and_goto_work_on_a_sheet(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    // Header on row 1, data on rows 2 and 5 (3 and 4 empty): the lines are the sheet's rows.
    let mut left = SheetSpec::new("S");
    let mut right = SheetSpec::new("S");
    for (sheet, rows) in [
        (&mut left, [("id", "name"), ("1", "Ann"), ("2", "Bob")]),
        (&mut right, [("id", "name"), ("2", "Bobby"), ("1", "Ann")]),
    ] {
        for (i, (id, name)) in rows.into_iter().enumerate() {
            let r = [1, 2, 5][i];
            sheet.cells.push((r, 1, text(id)));
            sheet.cells.push((r, 2, text(name)));
        }
    }
    let a = write_book(dir.path(), "a.xlsx", &[left]);
    let b = write_book(dir.path(), "b.xlsx", &[right]);
    let (handle, view) = open_loaded(cx, a, b).await;
    assert_eq!(differing_rows(cx, &view), 2, "by order every row differs");
    view.update(cx, |v, cx| v.set_prefs(|p| p.keys = "id".to_owned(), cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(
        differing_rows(cx, &view),
        1,
        "paired by id, only Bob differs"
    );
    view.update(cx, |v, cx| {
        v.set_prefs(|p| p.filter = Filter::Differences, cx)
    });
    assert_eq!(view.read_with(cx, |v, _| v.view().len()), 1);
    view.update(cx, |v, cx| v.set_prefs(|p| p.filter = Filter::All, cx));

    // Find "Bobby" selects the cell.
    draw(cx, handle);
    press(cx, handle, "ctrl-f");
    cx.update_window(handle, |_, window, cx| {
        window.press("ctrl-a", cx);
        window.input("Bobby", cx);
        window.press("enter", cx);
    })
    .unwrap();
    let selected = view
        .read_with(cx, |v, _| v.selection())
        .expect("a cell is selected");
    assert_eq!(selected.1, Some(1), "the name column");
    // Go to the sheet's row 5; the rows 3 and 4 hold nothing.
    press(cx, handle, "escape");
    press(cx, handle, "ctrl-g");
    cx.update_window(handle, |_, window, cx| {
        window.press("ctrl-a", cx);
        window.input("3", cx);
        window.press("enter", cx);
    })
    .unwrap();
    assert_eq!(notice(cx, &view).as_deref(), Some("No row at line 3."));
    cx.update_window(handle, |_, window, cx| {
        window.press("ctrl-a", cx);
        window.input("5", cx);
        window.press("enter", cx);
    })
    .unwrap();
    let row = view.read_with(cx, |v, _| v.selection()).unwrap().0;
    let state = view.read_with(cx, |v, _| {
        let pair = v.diff().unwrap().rows[row];
        assert!(pair.left.is_some() && pair.right.is_some());
        pair.state
    });
    assert_eq!(
        format!("{state:?}"),
        "Differs",
        "sheet row 5 is id 2, the row that differs"
    );
}

#[gpui_kit::test]
async fn the_csv_export_of_a_sheet_diff_is_the_csv_files_export(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let rows_a: &[&[&str]] = &[&["id", "v"], &["1", "a"], &["2", "b"]];
    let rows_b: &[&[&str]] = &[&["id", "v"], &["1", "a"], &["2", "c"]];
    let a = q_book(dir.path(), "a.xlsx", &[("S", rows_a)]);
    let b = q_book(dir.path(), "b.xlsx", &[("S", rows_b)]);
    let ca = write_file(dir.path(), "a.csv", b"id,v\n1,a\n2,b\n");
    let cb = write_file(dir.path(), "b.csv", b"id,v\n1,a\n2,c\n");
    let mut exported = Vec::new();
    for (name, left, right) in [("book.csv", a, b), ("plain.csv", ca, cb)] {
        let (handle, view) = open_loaded(cx, left, right).await;
        let entries = view.read_with(cx, |v, _| duet_app::table::menu::export_menu(v));
        let target = dir.path().join(name);
        let want = target.clone();
        let focus_view = view.clone();
        let ran = cx
            .update_window(handle, |_, window, cx| {
                let focus = focus_view.focus_handle(cx);
                duet_app::ui::menu::activate(entries, "Differences (CSV)", Some(&focus), window, cx)
            })
            .unwrap();
        assert!(ran);
        assert!(cx.did_prompt_for_new_path());
        cx.simulate_new_path_selection(move |_| Some(want));
        let seen = view.clone();
        cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
            seen.read(cx)
                .notice()
                .is_some_and(|n| n.starts_with("Exported: "))
        })
        .await;
        exported.push(std::fs::read(&target).unwrap());
    }
    assert_eq!(
        exported[0], exported[1],
        "the same CSV, whatever the files were"
    );
}

// ---- The picker, its keys and the wording ----

#[gpui_kit::test]
async fn the_picker_appears_for_a_workbook_side_only(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = q_book(dir.path(), "a.xlsx", &three_sheets());
    let csv = write_file(dir.path(), "b.csv", b"id,v\n1,1\n");
    let (handle, view) = open_loaded(cx, a.clone(), csv).await;
    draw(cx, handle);
    assert!(has_id(cx, handle, "table-sheet-left"));
    assert!(!has_id(cx, handle, "table-sheet-right"));
    // A workbook of one sheet still shows its picker (disabled).
    let one = q_book(dir.path(), "one.xlsx", &[("Only", &[&["id"], &["1"]])]);
    view.update(cx, |v, cx| v.load_paths(one.clone(), one, cx));
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    assert!(has_id(cx, handle, "table-sheet-left"));
    assert!(has_id(cx, handle, "table-sheet-right"));
    assert_eq!(
        view.read_with(cx, |v, _| v.sheet_names(true)),
        Some(vec!["Only".to_owned()])
    );
}

#[gpui_kit::test]
async fn the_picker_menu_lists_sheets_and_marks_hidden_ones(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut old = grid("Old", &[&["id"], &["1"]]);
    old.hidden = true;
    let a = write_book(
        dir.path(),
        "a.xlsx",
        &[
            grid("Q1", &[&["id"], &["1"]]),
            grid("Q2", &[&["id"], &["1"]]),
            old,
        ],
    );
    let (_handle, view) = open_loaded(cx, a.clone(), a).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.sheet_menu_entries_for_test(true)),
        [
            ("Q1".to_owned(), true),
            ("Q2".to_owned(), false),
            ("Old (hidden)".to_owned(), false)
        ]
    );
}

#[gpui_kit::test]
async fn ctrl_pagedown_moves_the_left_sheet_and_stops_at_the_end(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = q_book(dir.path(), "a.xlsx", &three_sheets());
    let b = q_book(dir.path(), "b.xlsx", &three_sheets());
    let (handle, view) = open_loaded(cx, a, b).await;
    press(cx, handle, "ctrl-pagedown");
    wait_table(cx, handle, &view).await;
    press(cx, handle, "ctrl-pagedown");
    wait_table(cx, handle, &view).await;
    assert_eq!(shown(cx, &view, true), Some(2));
    press(cx, handle, "ctrl-pagedown");
    assert!(!view.read_with(cx, |v, _| v.is_busy()), "the end stops");
    assert_eq!(shown(cx, &view, true), Some(2));
    press(cx, handle, "ctrl-pageup");
    wait_table(cx, handle, &view).await;
    assert_eq!(shown(cx, &view, true), Some(1));
    // With Shift the right side moves, and only it.
    press(cx, handle, "ctrl-shift-pagedown");
    wait_table(cx, handle, &view).await;
    assert_eq!(shown(cx, &view, false), Some(1));
    assert_eq!(shown(cx, &view, true), Some(1));
    press(cx, handle, "ctrl-shift-pageup");
    wait_table(cx, handle, &view).await;
    press(cx, handle, "ctrl-shift-pageup");
    assert!(!view.read_with(cx, |v, _| v.is_busy()), "the start stops");
    assert_eq!(shown(cx, &view, false), Some(0));
}

#[gpui_kit::test]
async fn the_delimiter_menu_hides_for_two_workbooks(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = q_book(dir.path(), "a.xlsx", &three_sheets());
    let b = q_book(dir.path(), "b.xlsx", &three_sheets());
    let csv = write_file(dir.path(), "c.csv", b"id,v\n1,1\n");
    let (handle, view) = open_loaded(cx, a.clone(), b).await;
    draw(cx, handle);
    assert!(!has_id(cx, handle, "table-delimiter"));
    view.update(cx, |v, cx| v.load_paths(a, csv, cx));
    wait_table(cx, handle, &view).await;
    draw(cx, handle);
    assert!(has_id(cx, handle, "table-delimiter"));
}

#[gpui_kit::test]
async fn the_status_and_tooltip_name_the_sheet(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = q_book(
        dir.path(),
        "a.xlsx",
        &[("Q1", &[&["id"], &["1"]]), ("Q2", &[&["id"], &["1"]])],
    );
    let csv = write_file(dir.path(), "b.csv", b"id\n1\n");
    let (_handle, view) = open_loaded(cx, a, csv).await;
    view.read_with(cx, |v, _| {
        assert!(
            v.status_text().contains("Left: sheet “Q1” (1 of 2)"),
            "{}",
            v.status_text()
        );
        let tip = v.name_tooltip(true).unwrap();
        assert!(tip.contains("sheet “Q1” (1 of 2)"), "{tip}");
        assert!(!tip.contains("utf-8") && !tip.contains("UTF-8"), "{tip}");
        let csv_tip = v.name_tooltip(false).unwrap();
        assert!(csv_tip.contains("Delimiter:"), "{csv_tip}");
        assert!(
            v.status_text().contains("Right: UTF-8"),
            "{}",
            v.status_text()
        );
    });
}

// ---- What "identical" may say ----

fn status(cx: &mut TestAppContext, view: &Entity<TableView>) -> String {
    view.read_with(cx, |v, _| v.status_text())
}

fn badge(cx: &mut TestAppContext, view: &Entity<TableView>) -> Option<String> {
    view.read_with(cx, |v, _| v.badge())
}

fn message(cx: &mut TestAppContext, view: &Entity<TableView>, left: bool) -> Option<String> {
    view.read_with(cx, |v, _| v.message(left).map(str::to_owned))
}

const PARTIAL: &str = "≈";

#[gpui_kit::test]
async fn identical_speaks_for_the_sheet_shown_and_no_more(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let left = three_sheets();
    let mut right = three_sheets();
    right[1] = ("Q2", &[&["id", "v"], &["1", "changed"]]);
    let a = q_book(dir.path(), "a.xlsx", &left);
    let b = q_book(dir.path(), "b.xlsx", &right);
    let (handle, view) = open_loaded(cx, a, b).await;
    assert_eq!(differing_rows(cx, &view), 0, "Q1 against Q1");
    assert_eq!(badge(cx, &view).as_deref(), Some(PARTIAL), "not ok");
    let said = status(cx, &view);
    assert!(
        said.contains("Sheet “Q1” is identical cell for cell."),
        "{said}"
    );
    assert!(said.contains("2 other sheets were not compared."), "{said}");
    assert!(!said.contains("The tables are identical"), "{said}");
    // Different names on the two sides are named, and a difference needs no such words.
    view.update(cx, |v, cx| v.pick_sheet(false, 2, cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(differing_rows(cx, &view), 1);
    assert_ne!(badge(cx, &view).as_deref(), Some(PARTIAL));
    let said = status(cx, &view);
    assert!(!said.contains("identical"), "{said}");
    assert!(said.contains("other sheets were not compared"), "{said}");
}

#[gpui_kit::test]
async fn sheets_on_one_side_only_are_named(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let same: &[&[&str]] = &[&["id"], &["1"]];
    let a = q_book(dir.path(), "a.xlsx", &[("Q1", same), ("Extra", same)]);
    let b = q_book(
        dir.path(),
        "b.xlsx",
        &[("q1", same), ("Other", same), ("Q9", same)],
    );
    let (handle, view) = open_loaded(cx, a, b).await;
    let said = status(cx, &view);
    assert!(said.contains("Only in a.xlsx: Extra."), "{said}");
    assert!(said.contains("Only in b.xlsx: Other, Q9."), "{said}");
    assert!(said.contains("3 other sheets were not compared."), "{said}");
    // The sheets shown differ in name: both are named.
    view.update(cx, |v, cx| v.pick_sheet(true, 1, cx));
    wait_table(cx, handle, &view).await;
    let said = status(cx, &view);
    assert!(
        said.contains("“Extra” and “q1” are identical cell for cell."),
        "{said}"
    );
}

#[gpui_kit::test]
async fn one_sheet_each_is_still_simply_identical(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let same: &[&[&str]] = &[&["id"], &["1"]];
    let a = q_book(dir.path(), "a.xlsx", &[("S", same)]);
    let b = q_book(dir.path(), "b.xlsx", &[("S", same)]);
    let (_handle, view) = open_loaded(cx, a, b).await;
    assert_eq!(badge(cx, &view).as_deref(), Some("ok"));
    let said = status(cx, &view);
    assert!(
        said.contains("The tables are identical cell for cell."),
        "{said}"
    );
    assert!(!said.contains("other sheet"), "{said}");
}

#[gpui_kit::test]
async fn a_workbook_of_two_sheets_against_a_csv_is_not_all_compared(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let same: &[&[&str]] = &[&["id"], &["1"]];
    let a = q_book(dir.path(), "a.xlsx", &[("S", same), ("T", same)]);
    let csv = write_file(dir.path(), "b.csv", b"id\n1\n");
    let (_handle, view) = open_loaded(cx, a, csv).await;
    assert_eq!(badge(cx, &view).as_deref(), Some(PARTIAL));
    let said = status(cx, &view);
    assert!(
        said.contains("Sheet “S” is identical cell for cell."),
        "{said}"
    );
    assert!(said.contains("1 other sheet was not compared."), "{said}");
}

#[gpui_kit::test]
async fn a_cut_sheet_is_never_identical_outright(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut wide = SheetSpec::new("Data");
    for row in 1..=1000 {
        wide.cells.push((row, 1, text("x")));
        wide.cells.push((row, 16_384, text("y")));
    }
    let a = write_book(dir.path(), "a.xlsx", std::slice::from_ref(&wide));
    let b = write_book(dir.path(), "b.xlsx", &[wide]);
    let (_handle, view) = open_loaded(cx, a, b).await;
    assert_eq!(differing_rows(cx, &view), 0);
    assert_eq!(badge(cx, &view).as_deref(), Some(PARTIAL));
    let said = status(cx, &view);
    assert!(
        said.contains("The rows read are identical. The rest of the sheet was not read."),
        "{said}"
    );
    assert!(!said.contains("The tables are identical"), "{said}");
}

#[gpui_kit::test]
async fn formulas_with_no_saved_result_are_not_vouched_for(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let formula = |t: &str| Cell::Formula {
        text: t.to_owned(),
        value: None,
    };
    let sheet = |t: &str| {
        let mut sheet = SheetSpec::new("S");
        sheet.cells.push((1, 1, text("v")));
        sheet.cells.push((2, 1, text("x")));
        sheet.cells.push((2, 2, formula(t)));
        sheet
    };
    // Two different formulas, neither with a result: nothing in the file tells them apart.
    let a = write_book(dir.path(), "a.xlsx", &[sheet("1+1")]);
    let b = write_book(dir.path(), "b.xlsx", &[sheet("2+2")]);
    let (_handle, view) = open_loaded(cx, a, b).await;
    assert_eq!(differing_rows(cx, &view), 0);
    assert_eq!(badge(cx, &view).as_deref(), Some(PARTIAL));
    let said = status(cx, &view);
    assert!(said.contains("The values read are identical."), "{said}");
}

// ---- What a report says ----

/// Runs an item of the Export menu and returns the file it wrote to `name`.
async fn export(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TableView>,
    item: &'static str,
    target: &Path,
) -> String {
    let entries = view.read_with(cx, |v, _| duet_app::table::menu::export_menu(v));
    let focus_view = view.clone();
    let ran = cx
        .update_window(handle, |_, window, cx| {
            let focus = focus_view.focus_handle(cx);
            duet_app::ui::menu::activate(entries, item, Some(&focus), window, cx)
        })
        .unwrap();
    assert!(ran, "{item}");
    let want = target.to_owned();
    cx.simulate_new_path_selection(move |_| Some(want));
    let (seen, shown) = (view.clone(), target.display().to_string());
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        seen.read(cx).notice().is_some_and(|n| n.contains(&shown))
    })
    .await;
    std::fs::read_to_string(target).unwrap()
}

#[gpui_kit::test]
async fn reports_say_what_the_compare_did_not_cover(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let formula = Cell::Formula {
        text: "1+1".to_owned(),
        value: None,
    };
    let mut wide = SheetSpec::new("Data");
    for row in 1..=1000 {
        wide.cells.push((row, 1, text("x")));
        wide.cells.push((row, 16_384, text("y")));
    }
    wide.cells.push((1, 2, formula.clone()));
    let spare = grid("Spare", &[&["id"], &["1"]]);
    let a = write_book(dir.path(), "a.xlsx", &[wide.clone(), spare]);
    let b = write_book(dir.path(), "b.xlsx", &[wide]);
    let (handle, view) = open_loaded(cx, a, b).await;

    let csv = export(
        cx,
        handle,
        &view,
        "Differences (CSV)",
        &dir.path().join("r.csv"),
    )
    .await;
    assert!(
        csv.contains("Note,,,,,1 other sheet was not compared.,"),
        "{csv}"
    );
    assert!(csv.contains("Note,,,,,Only in a.xlsx: Spare.,"), "{csv}");
    assert!(
        csv.contains("Only the first 488 rows of “Data” in a.xlsx were read"),
        "{csv}"
    );
    assert!(
        csv.contains("Note,,,,,\"1 formula cell has no saved result"),
        "{csv}"
    );
    assert!(
        csv.contains("Note,,,,,,Only the first 488 rows of “Data” in b.xlsx were read"),
        "the right side's note goes under the right side: {csv}"
    );

    let html = export(cx, handle, &view, "HTML report", &dir.path().join("r.html")).await;
    assert!(
        html.contains("a.xlsx (sheet “Data”) 1 other sheet was not compared."),
        "{html}"
    );
    assert!(html.contains("Only in a.xlsx: Spare."), "{html}");
    assert!(
        html.contains("Only the first 488 rows of “Data” in b.xlsx were read"),
        "{html}"
    );
    assert!(
        html.contains("1 formula cell has no saved result"),
        "{html}"
    );
    // The verdict the status gives: what was read is identical, not the whole sheet.
    const VERDICT: &str = "The rows read are identical. The rest of the sheet was not read.";
    assert!(csv.contains(VERDICT), "{csv}");
    assert!(html.contains(VERDICT), "{html}");
    assert!(!html.contains("No differing rows."), "{html}");

    // In the language of the report, whatever the window's.
    let entries = view.read_with(cx, |v, _| duet_app::table::menu::export_language_menu(v));
    let focus_view = view.clone();
    cx.update_window(handle, |_, window, cx| {
        let focus = focus_view.focus_handle(cx);
        assert!(duet_app::ui::menu::activate(
            entries,
            "ไทย",
            Some(&focus),
            window,
            cx
        ));
    })
    .unwrap();
    cx.run_until_parked();
    let csv = export(
        cx,
        handle,
        &view,
        "Differences (CSV)",
        &dir.path().join("t.csv"),
    )
    .await;
    assert!(csv.contains("อีก 1 ชีตไม่ได้เทียบ"), "{csv}");
    assert!(csv.contains("มีเฉพาะใน a.xlsx: Spare"), "{csv}");
    assert!(csv.contains("อ่านเฉพาะ 488 แถวแรก"), "{csv}");
    assert!(!csv.contains("other sheet"), "{csv}");
    let html = export(cx, handle, &view, "HTML report", &dir.path().join("t.html")).await;
    assert!(html.contains("อีก 1 ชีตไม่ได้เทียบ"), "{html}");
    assert!(html.contains("มี 1 เซลล์สูตร"), "{html}");
}

#[gpui_kit::test]
async fn a_marked_cell_in_a_report_comes_with_its_legend(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    const LEGEND: &str =
        "A leading ' marks text that reads like a number, date or other value, as in Excel.";
    let sheet = |v: Cell| {
        let mut sheet = SheetSpec::new("S");
        sheet.cells.push((1, 1, text("v")));
        sheet.cells.push((2, 1, v));
        sheet
    };
    let a = write_book(dir.path(), "a.xlsx", &[sheet(text("1"))]);
    let b = write_book(dir.path(), "b.xlsx", &[sheet(Cell::Num(1.0))]);
    let (handle, view) = open_loaded(cx, a, b).await;
    // The real export holds the marked cell, and says once what it means, on screen too.
    let on_screen = notice(cx, &view).expect("a notice");
    assert_eq!(on_screen.matches(LEGEND).count(), 1, "{on_screen}");
    let csv = export(
        cx,
        handle,
        &view,
        "Differences (CSV)",
        &dir.path().join("m.csv"),
    )
    .await;
    assert!(csv.contains(",v,'1,1\r\n"), "the marked cell: {csv}");
    assert_eq!(csv.matches(LEGEND).count(), 1, "{csv}");
    let html = export(cx, handle, &view, "HTML report", &dir.path().join("m.html")).await;
    assert!(html.contains("<del>'1</del>"), "{html}");
    assert_eq!(html.matches(LEGEND).count(), 1, "{html}");

    // With nothing marked there is nothing to explain.
    let c = write_book(dir.path(), "c.xlsx", &[sheet(Cell::Num(2.0))]);
    let d = write_book(dir.path(), "d.xlsx", &[sheet(Cell::Num(3.0))]);
    view.update(cx, |v, cx| v.load_paths(c, d, cx));
    wait_table(cx, handle, &view).await;
    assert!(notice(cx, &view).is_none_or(|note| !note.contains(LEGEND)));
    let csv = export(
        cx,
        handle,
        &view,
        "Differences (CSV)",
        &dir.path().join("n.csv"),
    )
    .await;
    assert!(!csv.contains(LEGEND), "{csv}");
}

#[gpui_kit::test]
async fn a_workbook_beside_a_csv_says_it_compares_stored_values(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    const NOTE: &str = "The workbook side compares its stored values, not what Excel shows: a 25% cell is 0.25, and dates read 2024-03-05.";
    let mut sheet = SheetSpec::new("S");
    sheet.cells.push((1, 1, text("pct")));
    sheet.cells.push((2, 1, Cell::Percent(0.25)));
    let a = write_book(dir.path(), "a.xlsx", std::slice::from_ref(&sheet));
    let exported = write_file(dir.path(), "a.csv", b"pct\n25%\n");
    let (handle, view) = open_loaded(cx, a.clone(), exported).await;
    assert_eq!(differing_rows(cx, &view), 1, "0.25 against 25%, as stored");
    assert_eq!(notice(cx, &view).as_deref(), Some(NOTE));
    let csv = export(
        cx,
        handle,
        &view,
        "Differences (CSV)",
        &dir.path().join("r.csv"),
    )
    .await;
    assert!(csv.contains(NOTE), "{csv}");
    // Two workbooks have no such question.
    let b = write_book(dir.path(), "b.xlsx", &[sheet]);
    view.update(cx, |v, cx| v.load_paths(a, b, cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(notice(cx, &view), None);
}

// ---- A sheet that cannot be read ----

fn with_spill() -> [SheetSpec; 3] {
    let mut bad = SheetSpec::new("Q2");
    bad.cells.push((1, 1, text("id")));
    bad.cells.push((2, 1, Cell::Err("#SPILL!".to_owned())));
    [
        grid("Q1", &[&["id"], &["1"]]),
        bad,
        grid("Q3", &[&["id"], &["3"]]),
    ]
}

#[gpui_kit::test]
async fn a_sheet_that_cannot_be_read_leaves_the_picker_and_the_rest(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = write_book(dir.path(), "a.xlsx", &with_spill());
    let b = q_book(dir.path(), "b.xlsx", &three_sheets());
    let (handle, view) = open_loaded(cx, a, b).await;
    let names = view.read_with(cx, |v, _| v.sheet_names(true));
    let right = ptr(cx, &view, false);

    view.update(cx, |v, cx| v.pick_sheet(true, 1, cx));
    wait_table(cx, handle, &view).await;
    let said = message(cx, &view, true).expect("a message");
    assert_eq!(
        said,
        "Can't read sheet “Q2” in a.xlsx: this sheet holds a cell error this version cannot read (#SPILL!)."
    );
    assert_eq!(badge(cx, &view).as_deref(), Some("!"));
    assert_eq!(
        view.read_with(cx, |v, _| v.sheet_names(true)),
        names,
        "the book stays"
    );
    assert_eq!(shown(cx, &view, true), Some(1));
    assert_eq!(ptr(cx, &view, false), right, "the other side is kept");
    draw(cx, handle);
    assert!(has_id(cx, handle, "table-sheet-left"), "the picker stays");
    let said = status(cx, &view);
    assert!(said.contains("Choose another sheet."), "{said}");

    // The pick is gone, so F5 does not walk back into the same failure.
    view.update(cx, |v, cx| v.reload(cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(message(cx, &view, true), None);
    assert_eq!(shown(cx, &view, true), Some(0));

    // And the sheet after it is reachable, without opening the file again.
    view.update(cx, |v, cx| v.pick_sheet(true, 1, cx));
    wait_table(cx, handle, &view).await;
    assert!(message(cx, &view, true).is_some());
    press(cx, handle, "ctrl-pagedown");
    wait_table(cx, handle, &view).await;
    assert_eq!(message(cx, &view, true), None);
    assert_eq!(shown(cx, &view, true), Some(2));
    assert_eq!(differing_rows(cx, &view), 1, "Q3 against Q1");
}

#[gpui_kit::test]
async fn a_default_sheet_that_cannot_be_read_still_lists_the_others(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut sheets = with_spill().to_vec();
    sheets.remove(0);
    let a = write_book(dir.path(), "a.xlsx", &sheets);
    let b = q_book(dir.path(), "b.xlsx", &[("Q3", &[&["id"], &["3"]])]);
    // Both sides open on the sheet that cannot be read.
    let (handle, view) = open_loaded(cx, a.clone(), a.clone()).await;
    assert!(message(cx, &view, true).is_some());
    assert!(message(cx, &view, false).is_some());
    assert_eq!(
        view.read_with(cx, |v, _| v.sheet_names(true)),
        Some(vec!["Q2".to_owned(), "Q3".to_owned()])
    );
    press(cx, handle, "ctrl-pagedown");
    wait_table(cx, handle, &view).await;
    assert_eq!(shown(cx, &view, true), Some(1));
    assert_eq!(message(cx, &view, true), None);
    view.update(cx, |v, cx| v.load_paths(a, b, cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(
        shown(cx, &view, true),
        Some(1),
        "the pick stays on its file"
    );
}

#[gpui_kit::test]
async fn a_file_that_would_end_the_process_is_refused_and_named(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let sst = b"<sst uniqueCount=\"99999999999\"><si><t>a</t></si></sst>";
    let bytes = duet_core::table::xlsx::fixture::workbook_with_parts(
        &[grid("S", &[&["id"]])],
        &[("xl/sharedStrings.xml", sst)],
    );
    let bomb = write_file(dir.path(), "bomb.xlsx", &bytes);
    let good = q_book(dir.path(), "good.xlsx", &[("S", &[&["id"], &["1"]])]);
    let (_handle, view) = open_loaded(cx, bomb, good).await;
    let said = message(cx, &view, true).expect("a message");
    assert!(said.starts_with("Can't open bomb.xlsx: "), "{said}");
    assert!(said.contains("more text than its size allows"), "{said}");
    assert!(view.read_with(cx, |v, _| v.side(false).is_some()));
}

// ---- Picks made while something else is on its way ----

#[gpui_kit::test]
async fn a_pick_that_races_a_load_still_reads_the_other_side(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = q_book(dir.path(), "a.xlsx", &three_sheets());
    let b = q_book(dir.path(), "b.xlsx", &three_sheets());
    let (handle, view) = open_loaded(cx, a, b.clone()).await;
    // F5, then the key before anything landed: F5 emptied both tables.
    view.update(cx, |v, cx| v.reload(cx));
    press(cx, handle, "ctrl-pagedown");
    wait_table(cx, handle, &view).await;
    assert_eq!(shown(cx, &view, true), Some(1));
    assert_eq!(shown(cx, &view, false), Some(0));
    assert!(ptr(cx, &view, false).is_some(), "the other side loaded");
    assert!(view.read_with(cx, |v, _| v.diff().is_some_and(|d| d.both)));
    assert_eq!(differing_rows(cx, &view), 1, "Q2 against Q1");

    // Open left, then the right side's key: the left is still the file on its way.
    let c = q_book(dir.path(), "c.xlsx", &three_sheets());
    view.update(cx, |v, cx| v.load_paths(c, b, cx));
    press(cx, handle, "ctrl-shift-pagedown");
    wait_table(cx, handle, &view).await;
    assert_eq!(shown(cx, &view, false), Some(1));
    assert!(ptr(cx, &view, true).is_some(), "the new left file loaded");
    assert!(view.read_with(cx, |v, _| v.diff().is_some_and(|d| d.both)));
    assert_eq!(
        view.read_with(cx, |v, _| v.side(true).map(|s| s.name.clone()))
            .as_deref(),
        Some("c.xlsx")
    );
}

#[gpui_kit::test]
async fn f5_keeps_the_sheet_on_screen_even_when_no_one_picked_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let same: &[&[&str]] = &[&["id"], &["1"]];
    let a = q_book(dir.path(), "a.xlsx", &[("A", same), ("B", same)]);
    let b = q_book(dir.path(), "b.xlsx", &[("B", same), ("A", same)]);
    let (handle, view) = open_loaded(cx, a, b).await;
    let name = |cx: &mut TestAppContext, left: bool| {
        view.read_with(cx, |v, _| {
            let names = v.sheet_names(left).unwrap();
            names[v.shown_sheet(left).unwrap()].clone()
        })
    };
    assert_eq!(
        (name(cx, true), name(cx, false)),
        ("A".to_owned(), "A".to_owned())
    );
    // After F9 the default rule would now choose B, the left side's first shared name.
    view.update(cx, |v, cx| v.swap_sides(cx));
    wait_table(cx, handle, &view).await;
    view.update(cx, |v, cx| v.reload(cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(
        (name(cx, true), name(cx, false)),
        ("A".to_owned(), "A".to_owned()),
        "F5 reads what is shown again"
    );
    assert_eq!(notice(cx, &view), None, "nothing moved, so nothing to say");
}

#[gpui_kit::test]
async fn sheet_keys_follow_the_pick_on_its_way(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = q_book(dir.path(), "a.xlsx", &three_sheets());
    let b = q_book(dir.path(), "b.xlsx", &three_sheets());
    let (handle, view) = open_loaded(cx, a, b).await;
    // Twice without waiting: two steps.
    press(cx, handle, "ctrl-pagedown");
    press(cx, handle, "ctrl-pagedown");
    wait_table(cx, handle, &view).await;
    assert_eq!(shown(cx, &view, true), Some(2));
    // Forward and straight back: the sheet it started on, read again.
    press(cx, handle, "ctrl-pageup");
    press(cx, handle, "ctrl-pagedown");
    wait_table(cx, handle, &view).await;
    assert_eq!(shown(cx, &view, true), Some(2));
    // Going back to the shown sheet while a pick is on its way is not ignored.
    view.update(cx, |v, cx| v.pick_sheet(true, 0, cx));
    view.update(cx, |v, cx| v.pick_sheet(true, 2, cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(shown(cx, &view, true), Some(2));
}

#[gpui_kit::test]
async fn a_pick_goes_by_name_and_a_stopped_pick_can_be_made_again(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = q_book(dir.path(), "a.xlsx", &three_sheets());
    let b = q_book(dir.path(), "b.xlsx", &three_sheets());
    let (handle, view) = open_loaded(cx, a, b).await;
    view.update(cx, |v, cx| v.pick_sheet_named(true, "Q3", cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(shown(cx, &view, true), Some(2));
    view.update(cx, |v, cx| v.pick_sheet_named(true, "Gone", cx));
    assert!(!view.read_with(cx, |v, _| v.is_busy()), "no such sheet");
    // Esc stops a pick; the same pick can be made again.
    view.update(cx, |v, cx| v.pick_sheet_named(true, "Q2", cx));
    press(cx, handle, "escape");
    assert!(!view.read_with(cx, |v, _| v.is_busy()));
    view.update(cx, |v, cx| v.pick_sheet_named(true, "Q2", cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(shown(cx, &view, true), Some(1));
    // Esc leaves the side empty: the sheet it last showed can be asked for again.
    view.update(cx, |v, cx| v.pick_sheet_named(true, "Q3", cx));
    press(cx, handle, "escape");
    assert_eq!(ptr(cx, &view, true), None);
    view.update(cx, |v, cx| v.pick_sheet_named(true, "Q2", cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(shown(cx, &view, true), Some(1));
    assert!(ptr(cx, &view, true).is_some());
}

// ---- Numbers, keys and what Excel really saves ----

#[gpui_kit::test]
async fn by_value_pairs_a_text_key_with_a_number_key(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let sheet = |id: Cell| {
        let mut sheet = SheetSpec::new("S");
        sheet.cells.push((1, 1, text("id")));
        sheet.cells.push((2, 1, id));
        sheet
    };
    let a = write_book(dir.path(), "a.xlsx", &[sheet(Cell::Num(1001.0))]);
    let b = write_book(dir.path(), "b.xlsx", &[sheet(text("1001"))]);
    let (handle, view) = open_loaded(cx, a, b).await;
    view.update(cx, |v, cx| v.set_prefs(|p| p.keys = "id".to_owned(), cx));
    wait_table(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let counts = v.diff().unwrap().counts;
        assert_eq!(
            (counts.left_only, counts.right_only),
            (1, 1),
            "off: never paired"
        );
    });
    view.update(cx, |v, cx| v.set_prefs(|p| p.numbers = true, cx));
    wait_table(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let counts = v.diff().unwrap().counts;
        assert_eq!(
            (counts.same, counts.left_only, counts.right_only),
            (1, 0, 0)
        );
    });
}

#[gpui_kit::test]
async fn workbooks_as_excel_saves_them_compare_cell_by_cell(cx: &mut TestAppContext) {
    use duet_core::table::xlsx::fixture::excel_workbook;
    let dir = tempfile::tempdir().unwrap();
    let sheet = |name: &str, day: f64| {
        let mut sheet = SheetSpec::new("ยอดขาย");
        sheet.cells.push((1, 1, text("ชื่อลูกค้า")));
        sheet.cells.push((1, 2, text("วันที่")));
        sheet
            .cells
            .push((2, 1, Cell::Rich(vec!["คุณ".to_owned(), name.to_owned()])));
        sheet.cells.push((2, 2, Cell::Date(day)));
        sheet
    };
    let a = write_file(
        dir.path(),
        "a.xlsx",
        &excel_workbook(&[sheet("สมชาย", 45356.0)]),
    );
    let b = write_file(
        dir.path(),
        "b.xlsx",
        &excel_workbook(&[sheet("สมชาย", 45357.0)]),
    );
    let c = write_file(
        dir.path(),
        "c.xlsx",
        &excel_workbook(&[sheet("สมศรี", 45356.0)]),
    );
    let (handle, view) = open_loaded(cx, a.clone(), a.clone()).await;
    assert_eq!(message(cx, &view, true), None);
    assert_eq!(badge(cx, &view).as_deref(), Some("ok"));
    view.update(cx, |v, cx| v.load_paths(a.clone(), b, cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(
        differing_rows(cx, &view),
        1,
        "the dates differ, through styles.xml"
    );
    view.update(cx, |v, cx| v.load_paths(a, c, cx));
    wait_table(cx, handle, &view).await;
    assert_eq!(differing_rows(cx, &view), 1, "the rich text differs");
}
