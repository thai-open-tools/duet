//! The table tab in Thai: the toolbars, the status and the body, and what a switch does to the
//! text a failed side keeps.

use std::time::Duration;

use duet_app::i18n::{self, tr, trf};
use duet_app::launch::Launch;
use duet_app::table::view::TableView;
use duet_app::workspace::{TabItem, Workspace};
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::{open, thai_ui};

pub(super) async fn open_table(
    cx: &mut TestAppContext,
    dir: &std::path::Path,
    left: &str,
    right: &str,
) -> (AnyWindowHandle, Entity<Workspace>, Entity<TableView>) {
    let (a, b) = (dir.join("a.csv"), dir.join("b.csv"));
    if !left.is_empty() {
        std::fs::write(&a, left).unwrap();
    }
    if !right.is_empty() {
        std::fs::write(&b, right).unwrap();
    }
    let (handle, workspace) = open(cx, Launch::Table { left: a, right: b });
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

fn label(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> Option<String> {
    cx.update_window(handle, |_, window, _| {
        window
            .try_find(id)
            .and_then(|e| e.label().map(str::to_owned))
    })
    .unwrap()
}

fn draw(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

#[gpui_kit::test]
async fn the_table_view_reads_in_thai(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let dir = tempfile::tempdir().unwrap();
    let (handle, _ws, view) =
        open_table(cx, dir.path(), "id,v\n1,a\n2,b\n", "id,v\n1,a\n2,c\n").await;
    draw(cx, handle);
    for (id, key) in [
        ("table-open-left", "Open left…"),
        ("table-open-right", "Open right…"),
        ("table-first", "First differing row (Ctrl+Home)"),
        ("table-prev", "Previous differing row (F7)"),
        ("table-next", "Next differing row (F8)"),
        ("table-last", "Last differing row (Ctrl+End)"),
        ("table-swap", "Swap sides (F9)"),
        ("table-reload", "Reload (F5)"),
        ("table-find", "Find…"),
        ("table-export", "Export"),
        ("table-header", "First row is a header"),
        ("table-ignore-case", "Ignore case"),
        ("table-trim", "Trim spaces"),
        ("table-numbers", "Compare numbers by value"),
    ] {
        assert_eq!(label(cx, handle, id).as_deref(), Some(tr(key)), "{id}");
        assert_ne!(tr(key), key, "{key} has Thai");
    }
    assert_eq!(
        label(cx, handle, "table-delimiter").as_deref(),
        Some(trf("Delimiter: {}", &[&tr("Auto")]).as_str())
    );
    // The status line and the detail strip.
    let status = view.read_with(cx, |v, _| v.status_text());
    assert!(status.contains(&trf("Differ {}", &[&1])), "{status}");
    assert!(status.contains(tr("Aligned by row order")), "{status}");
    assert!(!status.contains("Differ 1"), "{status}");
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

#[gpui_kit::test]
async fn a_failed_table_side_redraws_on_a_switch(cx: &mut TestAppContext) {
    // English first: the left file does not exist. The click is the switch.
    let dir = tempfile::tempdir().unwrap();
    let (handle, _ws, view) = open_table(cx, dir.path(), "", "id\n1\n").await;
    draw(cx, handle);
    let english = view.read_with(cx, |v, _| v.message(true).map(str::to_owned));
    let english = english.expect("the left side failed");
    assert!(english.starts_with("Can't open a.csv: "), "{english}");
    assert_eq!(
        label(cx, handle, "table-placeholder").as_deref(),
        Some(english.as_str())
    );

    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 0usize), cx)
    })
    .unwrap();
    // No draw: the frame the click caused already reads Thai.
    let thai = view.read_with(cx, |v, _| v.message(true).map(str::to_owned));
    let thai = thai.expect("the message stays");
    assert!(thai.starts_with("เปิด a.csv ไม่ได้: "), "{thai}");
    assert_eq!(
        label(cx, handle, "table-placeholder").as_deref(),
        Some(thai.as_str())
    );
    // The reason after the colon is the system's and stays as it was.
    assert_eq!(
        thai.trim_start_matches("เปิด a.csv ไม่ได้: "),
        english.trim_start_matches("Can't open a.csv: ")
    );

    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 1usize), cx)
    })
    .unwrap();
    let back = view.read_with(cx, |v, _| v.message(true).map(str::to_owned));
    assert_eq!(back.as_deref(), Some(english.as_str()));
}

#[gpui_kit::test]
async fn the_table_status_follows_a_switch(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, _ws, view) = open_table(cx, dir.path(), "id\n1\n", "id\n1\n").await;
    let before = view.read_with(cx, |v, _| v.status_text());
    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 0usize), cx)
    })
    .unwrap();
    let after = view.read_with(cx, |v, _| v.status_text());
    assert_ne!(before, after);
    assert!(after.contains(tr("Left")), "{after}");
}

/// Two workbooks in a workspace tab: the left holds two formulas with no saved result.
async fn open_books(
    cx: &mut TestAppContext,
    dir: &std::path::Path,
    left: &[u8],
) -> (AnyWindowHandle, Entity<Workspace>, Entity<TableView>) {
    use duet_core::table::xlsx::fixture::{Cell, SheetSpec, text, workbook};
    let (a, b) = (dir.join("a.xlsx"), dir.join("b.xlsx"));
    std::fs::write(&a, left).unwrap();
    let mut sheet = SheetSpec::new("Q1");
    sheet.cells.push((1, 1, text("v")));
    sheet.cells.push((2, 1, Cell::Num(2.0)));
    std::fs::write(&b, workbook(&[sheet.clone(), SheetSpec::new("Q2")])).unwrap();
    let (handle, workspace) = open(cx, Launch::Table { left: a, right: b });
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
async fn workbook_notes_and_pickers_follow_a_switch_without_a_read(cx: &mut TestAppContext) {
    use duet_core::table::xlsx::fixture::{Cell, SheetSpec, text, workbook};
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("Q1");
    let formula = Cell::Formula {
        text: "1+1".to_owned(),
        value: None,
    };
    sheet.cells.push((1, 1, text("v")));
    sheet.cells.push((2, 1, formula.clone()));
    sheet.cells.push((3, 1, formula));
    let (handle, _ws, view) = open_books(cx, dir.path(), &workbook(&[sheet])).await;
    draw(cx, handle);
    let english = view.read_with(cx, |v, _| v.notice().map(str::to_owned));
    assert_eq!(
        english.as_deref(),
        Some("2 formula cells have no saved result, so they show empty.")
    );
    assert_eq!(
        label(cx, handle, "table-sheet-left").as_deref(),
        Some("Sheet: Q1")
    );
    let tables = view.read_with(cx, |v, _| {
        (v.table_ptr_for_test(true), v.table_ptr_for_test(false))
    });
    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 0usize), cx)
    })
    .unwrap();
    // No draw and no wait: the switch said it again, and read nothing.
    assert!(!view.read_with(cx, |v, _| v.is_busy()));
    let thai = view.read_with(cx, |v, _| v.notice().map(str::to_owned));
    assert_eq!(
        thai.as_deref(),
        Some("มี 2 เซลล์สูตรที่ไม่มีผลลัพธ์ที่บันทึกไว้ จึงแสดงว่าง")
    );
    draw(cx, handle);
    assert_eq!(
        label(cx, handle, "table-sheet-left").as_deref(),
        Some("ชีต: Q1")
    );
    let status = view.read_with(cx, |v, _| v.status_text());
    assert!(status.contains("ซ้าย: ชีต “Q1” (1 จาก 1)"), "{status}");
    assert_eq!(
        view.read_with(cx, |v, _| (
            v.table_ptr_for_test(true),
            v.table_ptr_for_test(false)
        )),
        tables,
        "the tables are the ones read before"
    );
    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 1usize), cx)
    })
    .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned)),
        english
    );
}

#[gpui_kit::test]
async fn a_refused_workbook_is_worded_again_on_a_switch(cx: &mut TestAppContext) {
    use duet_core::table::xlsx::fixture::protected_bytes;
    let dir = tempfile::tempdir().unwrap();
    let (handle, _ws, view) = open_books(cx, dir.path(), &protected_bytes()).await;
    let english = view
        .read_with(cx, |v, _| v.message(true).map(str::to_owned))
        .unwrap();
    assert!(
        english.starts_with("Can't open a.xlsx: is password protected."),
        "{english}"
    );
    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 0usize), cx)
    })
    .unwrap();
    let thai = view
        .read_with(cx, |v, _| v.message(true).map(str::to_owned))
        .unwrap();
    assert!(
        thai.starts_with("เปิด a.xlsx ไม่ได้: มีรหัสผ่านป้องกันอยู่"),
        "{thai}"
    );
    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 1usize), cx)
    })
    .unwrap();
}

#[gpui_kit::test]
async fn a_failed_sheet_is_worded_again_on_a_switch(cx: &mut TestAppContext) {
    use duet_core::table::xlsx::fixture::{Cell, SheetSpec, text, workbook};
    let dir = tempfile::tempdir().unwrap();
    let mut bad = SheetSpec::new("Q1");
    bad.cells.push((1, 1, text("v")));
    bad.cells.push((2, 1, Cell::Err("#SPILL!".to_owned())));
    let (handle, _ws, view) =
        open_books(cx, dir.path(), &workbook(&[bad, SheetSpec::new("Q2")])).await;
    let english = view
        .read_with(cx, |v, _| v.message(true).map(str::to_owned))
        .unwrap();
    assert!(
        english.starts_with("Can't read sheet “Q1” in a.xlsx: "),
        "{english}"
    );
    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 0usize), cx)
    })
    .unwrap();
    let thai = view
        .read_with(cx, |v, _| v.message(true).map(str::to_owned))
        .unwrap();
    assert!(thai.starts_with("อ่านชีต “Q1” ใน a.xlsx ไม่ได้: "), "{thai}");
    assert!(thai.contains("#SPILL!"), "{thai}");
    assert!(
        view.read_with(cx, |v, _| v.status_text())
            .contains("เลือกชีตอื่น"),
        "the status too"
    );
    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 1usize), cx)
    })
    .unwrap();
}

#[gpui_kit::test]
async fn what_identical_cannot_speak_for_reads_in_thai(cx: &mut TestAppContext) {
    use duet_core::table::xlsx::fixture::{Cell, SheetSpec, text, workbook};
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("Q1");
    sheet.cells.push((1, 1, text("v")));
    sheet.cells.push((2, 1, Cell::Num(2.0)));
    // Against `open_books`' right side, Q1 is the same and Q2 was not compared.
    let (handle, _ws, view) =
        open_books(cx, dir.path(), &workbook(&[sheet, SheetSpec::new("Q2")])).await;
    assert_eq!(view.read_with(cx, |v, _| v.badge()).as_deref(), Some("≈"));
    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 0usize), cx)
    })
    .unwrap();
    let status = view.read_with(cx, |v, _| v.status_text());
    assert!(status.contains("ชีต “Q1” เหมือนกันทุกเซลล์"), "{status}");
    assert!(status.contains("อีก 1 ชีตไม่ได้เทียบ"), "{status}");
    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 1usize), cx)
    })
    .unwrap();
}
