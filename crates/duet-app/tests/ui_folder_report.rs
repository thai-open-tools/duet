//! Headless UI tests for the folder reports (P10b Task 6): the Report ▾ menu, the export panel
//! and the export job, on LocalDiff's report pair (`rr:43-56`).

mod folder_support;

use std::path::{Path, PathBuf};
use std::time::Duration;

use duet_app::folder::export_panel::{ExportChoices, report_menu};
use duet_app::folder::model::FolderFilter;
use duet_app::folder::view::FolderView;
use duet_core::folder::report::{ReportFormat, ReportScope};
use folder_support::{
    open_report_fixture, press, report_fixture, saved_setting, select, use_settings, wait_idle,
};
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, ClipboardItem, Entity, Focusable, ScrollDelta,
    TestAppContext, Window, point, px,
};

const MENU: [&str; 5] = [
    "HTML report: summary and per-file details…",
    "Excel (.xlsx): track the review as a team…",
    "-",
    "Text report (.txt)…",
    "CSV: a table for other programs…",
];

fn click(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// Presses Create as a user would: scrolled into view first. The panel is taller than the room
/// under the toolbar rows of the 1200 x 800 test window, so its last row lies under the status
/// bar until the overlay scrolls; since U17 put "Filter paths" in the criterion row, that row
/// wraps at 1200 px and the panel starts 30 px lower.
fn press_create(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.scroll(
            "folder-export-panel",
            ScrollDelta::Pixels(point(px(0.), px(-1000.))),
            cx,
        );
        window.render_frame(cx);
        window.click("folder-export-create", cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn label(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> Option<String> {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find(id).label().map(str::to_owned)
    })
    .unwrap()
}

fn selected(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> Option<bool> {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find(id).selected()
    })
    .unwrap()
}

/// A tab stop (`Some`) is an enabled control; a disabled one is no tab stop (`None`).
fn enabled(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find(id).focused().is_some()
    })
    .unwrap()
}

fn notice(cx: &mut TestAppContext, view: &Entity<FolderView>) -> Option<String> {
    view.read_with(cx, |v, _| v.notice().map(str::to_owned))
}

fn choices(cx: &mut TestAppContext, view: &Entity<FolderView>) -> ExportChoices {
    view.read_with(cx, |v, _| v.export_choices().cloned())
        .expect("the export panel is open")
}

fn open_export(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<FolderView>,
    format: ReportFormat,
) {
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.open_export(format, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
}

fn set_choices(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<FolderView>,
    change: impl FnOnce(&mut ExportChoices),
) {
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.set_export_choices_for_test(change, window, cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
}

fn names(a: &str, b: &str) -> [String; 2] {
    [a.to_owned(), b.to_owned()]
}

/// Answers the save dialog with `out`; returns the folder the dialog started in (as
/// `ui_text_watch_report.rs` does, G14).
fn pick_path(cx: &mut TestAppContext, out: &Path) -> PathBuf {
    assert!(
        cx.did_prompt_for_new_path(),
        "the export asks where to save"
    );
    let mut start = PathBuf::new();
    cx.simulate_new_path_selection(|dir| {
        start = dir.to_path_buf();
        Some(out.to_path_buf())
    });
    start
}

/// Waits for the export job to end; returns its notice.
async fn wait_export(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<FolderView>,
) -> String {
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        let v = v.read(cx);
        !v.export_running() && v.notice().is_some()
    })
    .await;
    notice(cx, view).unwrap()
}

/// Opens the panel for `format`, changes the choices, presses Create, answers the save dialog
/// with `out` and waits for the notice.
async fn export_to(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<FolderView>,
    format: ReportFormat,
    change: impl FnOnce(&mut ExportChoices),
    out: &Path,
) -> String {
    open_export(cx, handle, view, format);
    set_choices(cx, handle, view, change);
    press_create(cx, handle);
    pick_path(cx, out);
    wait_export(cx, handle, view).await
}

fn rows_focused(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<FolderView>,
) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        view.read(cx).focus_handle(cx).is_focused(window)
    })
    .unwrap()
}

/// A left click with Ctrl held at the centre of `id` (the harness's own click has none).
fn ctrl_click(window: &mut Window, cx: &mut App, id: impl Into<gpui_kit::ElementId>) {
    use gpui_kit::{InputEvent as _, MouseButton, MouseDownEvent, MouseUpEvent};
    window.render_frame(cx);
    let position = window.find(id).bounds().center();
    let modifiers = gpui_kit::Modifiers {
        control: true,
        ..Default::default()
    };
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
}

fn menu(view: &Entity<FolderView>, cx: &mut TestAppContext) -> Vec<String> {
    view.read_with(cx, |v, _| duet_app::ui::menu::outline(&report_menu(v)))
}

#[gpui_kit::test]
async fn the_report_menu_lists_four_formats_and_waits_for_a_result(cx: &mut TestAppContext) {
    let (_dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    assert_eq!(menu(&view, cx), MENU);
    // The snapshot sets no disabled flag; a disabled button is no tab stop.
    let button_disabled = |cx: &mut TestAppContext| !enabled(cx, handle, "folder-report");
    assert!(!button_disabled(cx));
    assert_eq!(
        label(cx, handle, "folder-report").as_deref(),
        Some("Report")
    );
    view.update(cx, |v, cx| v.refresh(cx));
    let waiting: Vec<String> = MENU
        .iter()
        .map(|item| match *item {
            "-" => "-".to_owned(),
            item => format!("({item})"),
        })
        .collect();
    assert_eq!(menu(&view, cx), waiting, "the items wait for the compare");
    assert!(button_disabled(cx), "and so does the button");
    wait_idle(cx, handle, &view).await;
    assert_eq!(menu(&view, cx), MENU);
    assert!(!button_disabled(cx));
}

#[gpui_kit::test]
async fn the_export_panel_starts_with_default_names_and_saved_choices(cx: &mut TestAppContext) {
    let _data = use_settings(cx);
    let (_dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    open_export(cx, handle, &view, ReportFormat::Html);
    assert!(view.read_with(cx, |v, _| v.export_open()));
    let start = choices(cx, &view);
    assert_eq!(start.format, ReportFormat::Html);
    let o = &start.options;
    assert_eq!(o.names, names("IWT_Config", "PRD_Config"));
    assert_eq!(o.scope, ReportScope::All);
    assert!(o.details && o.mask && o.unimportant && !o.same, "{o:?}");
    assert_eq!(o.context, 2);
    assert_eq!((o.title.as_str(), o.note.as_str()), ("", ""));
    assert_eq!(
        selected(cx, handle, "folder-export-format-html"),
        Some(true)
    );
    assert_eq!(
        label(cx, handle, "folder-export-scope-all").as_deref(),
        Some("All items (16)")
    );
    // PF13: nothing is selected, so "Selected" has no files and is off.
    assert_eq!(
        label(cx, handle, "folder-export-scope-selected").as_deref(),
        Some("Selected items only (0 files)")
    );
    assert!(!enabled(cx, handle, "folder-export-scope-selected"));
    // The default Mismatched chip filters, so "As filtered now" is on...
    assert!(enabled(cx, handle, "folder-export-scope-filtered"));
    click(cx, handle, "folder-export-scope-filtered");
    assert_eq!(choices(cx, &view).options.scope, ReportScope::Filtered);
    // ...and with every row shown it has nothing to filter (counted at render, PF14).
    view.update(cx, |v, cx| v.set_filter(FolderFilter::All, cx));
    assert!(!enabled(cx, handle, "folder-export-scope-filtered"));
    assert_eq!(choices(cx, &view).options.scope, ReportScope::All);
    // Another format from the menu switches the open panel.
    open_export(cx, handle, &view, ReportFormat::Xlsx);
    assert_eq!(choices(cx, &view).format, ReportFormat::Xlsx);
    assert_eq!(
        label(cx, handle, "folder-export-details").as_deref(),
        Some("Add a “Changed lines” sheet (one row per line)")
    );
}

#[gpui_kit::test]
async fn an_html_report_is_saved_where_asked(cx: &mut TestAppContext) {
    let (dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    open_export(cx, handle, &view, ReportFormat::Html);
    set_choices(cx, handle, &view, |c| {
        c.options.names = names("IWT", "PRD");
        c.options.title = "Check before deploy".to_owned();
        c.options.details = true;
        c.options.mask = true;
    });
    press_create(cx, handle);
    assert!(
        !view.read_with(cx, |v, _| v.export_open()),
        "Create closes the panel"
    );
    // PF1: the test platform drops the suggested name, so the view records it.
    let name = view
        .read_with(cx, |v, _| v.last_export_name_for_test().map(str::to_owned))
        .unwrap();
    assert!(name.starts_with("folder-compare_IWT_vs_PRD_"), "{name}");
    assert!(name.ends_with(".html"), "{name}");
    let out = dir.path().join("report.html");
    assert_eq!(
        pick_path(cx, &out),
        dir.path(),
        "starts beside the left folder"
    );
    let done = wait_export(cx, handle, &view).await;
    assert_eq!(done, format!("Exported: {}.", out.display()));
    let page = std::fs::read_to_string(&out).unwrap();
    assert!(page.contains("<h1>Check before deploy</h1>"), "{page}");
    for secret in ["Secr3t-A", "Secr3t-B", "Zz9-same"] {
        assert!(!page.contains(secret), "{secret} leaked");
    }
    assert!(page.contains("(hidden #"));
    assert!(page.contains("db.properties"));
    assert!(!page.contains("<script>alert"), "file contents are escaped");
}

#[gpui_kit::test]
async fn the_names_are_remembered_for_this_folder_pair(cx: &mut TestAppContext) {
    let data = use_settings(cx);
    let (dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    let out = dir.path().join("report.html");
    let done = export_to(
        cx,
        handle,
        &view,
        ReportFormat::Html,
        |c| c.options.names = names("IWT", "PRD"),
        &out,
    )
    .await;
    assert!(done.starts_with("Exported: "), "{done}");
    open_export(cx, handle, &view, ReportFormat::Html);
    assert_eq!(choices(cx, &view).options.names, names("IWT", "PRD"));
    let pair = format!("{}|{}", a.display(), b.display());
    assert_eq!(
        saved_setting(&data, "folder.report_names"),
        serde_json::json!([{ "pair": pair, "names": ["IWT", "PRD"] }])
    );
    // The HTML report's choices are remembered too.
    assert_eq!(saved_setting(&data, "folder.report_lines"), true);
    assert_eq!(saved_setting(&data, "folder.report_context"), 2);
    assert_eq!(saved_setting(&data, "folder.report_mask"), true);
    assert_eq!(saved_setting(&data, "folder.report_unimportant"), true);
    assert_eq!(saved_setting(&data, "folder.report_same"), false);
}

#[gpui_kit::test]
async fn a_text_report_of_the_selection_has_a_bom(cx: &mut TestAppContext) {
    let (dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "app/db.properties");
    let legacy = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (0..m.visible_len())
            .find(|&row| m.node_at(row).rel == "legacy")
            .unwrap()
    });
    cx.update_window(handle, |_, window, cx| {
        ctrl_click(window, cx, ("folder-row", legacy))
    })
    .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.chosen_rels_for_test()),
        ["app/db.properties", "legacy"]
    );
    open_export(cx, handle, &view, ReportFormat::Text);
    assert_eq!(
        label(cx, handle, "folder-export-scope-selected").as_deref(),
        Some("Selected items only (5 files)")
    );
    click(cx, handle, "folder-export-scope-selected");
    assert_eq!(
        selected(cx, handle, "folder-export-scope-selected"),
        Some(true)
    );
    set_choices(cx, handle, &view, |c| c.options.names = names("IWT", "PRD"));
    press_create(cx, handle);
    let out = dir.path().join("report.txt");
    pick_path(cx, &out);
    let done = wait_export(cx, handle, &view).await;
    assert_eq!(done, format!("Exported: {}.", out.display()));
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.starts_with('\u{feff}'), "{text:?}");
    assert!(text.contains("A = IWT"), "{text}");
    assert!(text.contains("  All files        5"), "{text}");
    assert!(text.contains("~ A 3: password=•••••• (hidden #"), "{text}");
    assert!(
        !text.contains("Secr3t") && !text.contains("run.sh"),
        "{text}"
    );
}

#[gpui_kit::test]
async fn a_csv_has_one_row_per_file(cx: &mut TestAppContext) {
    let data = use_settings(cx);
    let (dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    let out = dir.path().join("files.csv");
    let done = export_to(cx, handle, &view, ReportFormat::Csv, |_| {}, &out).await;
    assert_eq!(done, format!("Exported: {}.", out.display()));
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(
        text.starts_with('\u{feff}') && text.ends_with("\r\n"),
        "{text:?}"
    );
    let rows: Vec<&str> = text.trim_end_matches("\r\n").split("\r\n").collect();
    assert_eq!(
        rows.len(),
        1 + 16,
        "the header and one row per file: {text}"
    );
    assert!(text.contains(",'=SUM(1).txt,"), "{text}");
    assert!(
        rows.iter()
            .any(|row| row.contains(",db.properties,") && row.contains(",Different,,2,0,0,1,")),
        "the counts are read for a CSV too (PF5): {text}"
    );
    // A CSV keeps neither the lines nor the masking choice (LocalDiff `rep:735-738`).
    assert_eq!(
        saved_setting(&data, "folder.report_lines"),
        serde_json::Value::Null
    );
    assert_eq!(
        saved_setting(&data, "folder.report_mask"),
        serde_json::Value::Null
    );
    assert_eq!(
        saved_setting(&data, "folder.report_context"),
        serde_json::Value::Null
    );
}

#[gpui_kit::test]
async fn an_xlsx_is_a_zip(cx: &mut TestAppContext) {
    let (dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    let out = dir.path().join("review.xlsx");
    let done = export_to(cx, handle, &view, ReportFormat::Xlsx, |_| {}, &out).await;
    assert_eq!(done, format!("Exported: {}.", out.display()));
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[..2], b"PK");
}

#[gpui_kit::test]
async fn escape_stops_an_export_and_writes_nothing(cx: &mut TestAppContext) {
    let (_dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, _| v.hold_export_for_test(true));
    open_export(cx, handle, &view, ReportFormat::Html);
    press_create(cx, handle);
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("report.html");
    pick_path(cx, &out);
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.export_running()));
    press(cx, handle, "escape");
    let done = wait_export(cx, handle, &view).await;
    assert_eq!(done, "Export cancelled.");
    assert!(!view.read_with(cx, |v, _| v.export_running()));
    assert!(!out.exists());
    assert_eq!(
        std::fs::read_dir(out_dir.path()).unwrap().count(),
        0,
        "no temp file is left either"
    );
}

#[gpui_kit::test]
async fn export_waits_for_a_running_compare(cx: &mut TestAppContext) {
    let (_dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.refresh(cx));
    open_export(cx, handle, &view, ReportFormat::Html);
    assert!(!view.read_with(cx, |v, _| v.export_open()));
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Wait for the compare to finish.")
    );
    wait_idle(cx, handle, &view).await;
}

#[gpui_kit::test]
async fn equal_names_get_a_and_b(cx: &mut TestAppContext) {
    let (dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    let out = dir.path().join("report.txt");
    export_to(
        cx,
        handle,
        &view,
        ReportFormat::Text,
        |c| c.options.names = names("X", "X"),
        &out,
    )
    .await;
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains("A = X (A) : "), "{text}");
    assert!(text.contains("B = X (B) : "), "{text}");
}

#[gpui_kit::test]
async fn escape_closes_the_export_panel(cx: &mut TestAppContext) {
    let (_dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    open_export(cx, handle, &view, ReportFormat::Csv);
    assert!(!rows_focused(cx, handle, &view), "the panel takes the keys");
    press(cx, handle, "escape");
    assert!(!view.read_with(cx, |v, _| v.export_open()));
    assert!(rows_focused(cx, handle, &view));
    assert!(!cx.did_prompt_for_new_path());
}

#[gpui_kit::test]
async fn a_compare_landing_while_the_panel_is_open_closes_it(cx: &mut TestAppContext) {
    let (_dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    open_export(cx, handle, &view, ReportFormat::Html);
    view.update(cx, |v, cx| v.refresh(cx));
    assert!(!view.read_with(cx, |v, _| v.export_open()));
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("The folders were compared again, so the export was closed.")
    );
    wait_idle(cx, handle, &view).await;
}

/// PF2: Enter in a field commits the field; Enter on the panel itself creates.
#[gpui_kit::test]
async fn enter_in_a_field_stays_and_enter_on_the_panel_creates(cx: &mut TestAppContext) {
    let (_dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    open_export(cx, handle, &view, ReportFormat::Html);
    // PF8: what is typed goes straight into the choices.
    set_choices(cx, handle, &view, |c| c.options.names[0] = String::new());
    click(cx, handle, "folder-export-name-a");
    cx.update_window(handle, |_, window, cx| window.input("typed", cx))
        .unwrap();
    assert_eq!(choices(cx, &view).options.names[0], "typed");
    press(cx, handle, "enter");
    assert!(view.read_with(cx, |v, _| v.export_open()));
    assert!(
        !cx.did_prompt_for_new_path(),
        "Enter in a field asks nothing"
    );
    open_export(cx, handle, &view, ReportFormat::Html);
    press(cx, handle, "enter");
    assert!(cx.did_prompt_for_new_path(), "Enter on the panel creates");
    let name = view
        .read_with(cx, |v, _| v.last_export_name_for_test().map(str::to_owned))
        .unwrap();
    assert!(
        name.starts_with("folder-compare_typed_vs_PRD_Config_"),
        "{name}"
    );
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    assert!(!view.read_with(cx, |v, _| v.export_running()));
}

/// PF14: the export and sync panels never show together.
#[gpui_kit::test]
async fn the_export_and_sync_panels_are_exclusive(cx: &mut TestAppContext) {
    let (_dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    click(cx, handle, "folder-sync");
    assert!(view.read_with(cx, |v, _| v.sync_open()));
    open_export(cx, handle, &view, ReportFormat::Html);
    assert!(view.read_with(cx, |v, _| v.export_open() && !v.sync_open()));
    click(cx, handle, "folder-sync");
    assert!(view.read_with(cx, |v, _| v.sync_open() && !v.export_open()));
}

/// PF14: a copy, delete or sync waits while an export reads the files.
#[gpui_kit::test]
async fn a_batch_waits_for_a_running_export(cx: &mut TestAppContext) {
    let (dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "app/extra-iwt.txt");
    view.update(cx, |v, _| v.hold_export_for_test(true));
    open_export(cx, handle, &view, ReportFormat::Text);
    press_create(cx, handle);
    let out = dir.path().join("report.txt");
    pick_path(cx, &out);
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.export_running()));
    click(cx, handle, "folder-copy-right");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Wait for the export to finish.")
    );
    assert!(!cx.has_pending_prompt(), "no copy question");
    assert!(!view.read_with(cx, |v, _| v.batch_running()));
    click(cx, handle, "folder-sync");
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
    view.update(cx, |v, _| v.hold_export_for_test(false));
    let done = wait_export(cx, handle, &view).await;
    assert_eq!(done, format!("Exported: {}.", out.display()));
    assert!(!b.join("app/extra-iwt.txt").exists(), "nothing was copied");
}

/// The report names its scope; "As filtered now" keeps what the chip and the path filter show.
#[gpui_kit::test]
async fn a_filtered_report_names_its_chip_and_path_filter(cx: &mut TestAppContext) {
    let (dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_name_filter("APP/", cx));
    open_export(cx, handle, &view, ReportFormat::Html);
    click(cx, handle, "folder-export-scope-filtered");
    let out = dir.path().join("filtered.html");
    press_create(cx, handle);
    pick_path(cx, &out);
    wait_export(cx, handle, &view).await;
    let page = std::fs::read_to_string(&out).unwrap();
    assert!(
        page.contains("Scope: <b>As filtered: “Mismatched” + path filter “app/”</b>"),
        "{page}"
    );
    assert!(page.contains("db.properties"));
    assert!(
        !page.contains("run.sh") && !page.contains("old0.cfg"),
        "{page}"
    );

    let all = dir.path().join("all.html");
    export_to(cx, handle, &view, ReportFormat::Html, |_| {}, &all).await;
    let page = std::fs::read_to_string(&all).unwrap();
    assert!(page.contains("Scope: <b>All items</b>"), "{page}");
    assert!(page.contains("run.sh"));
}

/// A write that fails says so in the table view's words, and leaves nothing behind.
#[gpui_kit::test]
async fn a_failed_write_says_why(cx: &mut TestAppContext) {
    let (dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    let out = dir.path().join("no-such-folder").join("report.csv");
    let done = export_to(cx, handle, &view, ReportFormat::Csv, |_| {}, &out).await;
    assert!(
        done.starts_with("Can't export: ") && done.ends_with('.'),
        "{done}"
    );
    assert!(!out.exists());
}

/// M-1: whether the result on show has a row still being compared is decided once, when the
/// result goes live, and not on each frame: a finished result with a Pending row keeps Report
/// off, and the next finished result that has none turns it on.
#[gpui_kit::test]
async fn report_is_decided_when_the_result_goes_live(cx: &mut TestAppContext) {
    use duet_app::folder::job::shallow;
    use duet_core::folder::run::{CompareMode, FolderOptions, compare_folders};
    let (_dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    assert!(enabled(cx, handle, "folder-report"));
    let options = FolderOptions {
        mode: CompareMode::Content,
        ..FolderOptions::default()
    };
    let land = |v: &mut FolderView, cx: &mut gpui_kit::Context<FolderView>, result| {
        v.refresh(cx);
        let generation = v.job_generation();
        assert!(v.apply_for_test(generation, result, true, cx));
    };
    // The top-level pass: its paired folders are still Pending.
    let pending = shallow(&a, &b, &options);
    view.update(cx, |v, cx| land(v, cx, pending));
    assert!(!view.read_with(cx, |v, _| v.is_busy()));
    assert!(!view.read_with(cx, |v, _| v.can_report()));
    assert!(!enabled(cx, handle, "folder-report"));
    let full = compare_folders(&a, &b, &options, &Default::default(), &Default::default());
    view.update(cx, |v, cx| land(v, cx, full));
    assert!(view.read_with(cx, |v, _| v.can_report()));
    assert!(enabled(cx, handle, "folder-report"));
}

/// F-154: the names, the title and the note stop taking text at 60, 160 and 2,000 characters,
/// typed one by one or pasted at once.
#[gpui_kit::test]
async fn the_report_fields_stop_at_their_limits_while_typing(cx: &mut TestAppContext) {
    let (_dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    open_export(cx, handle, &view, ReportFormat::Html);
    // Two characters short of the limit, so a few keys reach it (each key draws a frame).
    set_choices(cx, handle, &view, |c| {
        c.options.names = names(&"a".repeat(58), "");
        c.options.title = "t".repeat(158);
    });
    let fields = |cx: &mut TestAppContext| {
        view.read_with(cx, |v, cx| v.export_field_texts_for_test(cx))
            .expect("the export panel is open")
    };
    let chars = |text: &String| text.chars().count();
    // Typed: one key and one frame per character.
    let input = |cx: &mut TestAppContext, text: &str| {
        cx.update_window(handle, |_, window, cx| window.input(text, cx))
            .unwrap();
    };
    let paste = |cx: &mut TestAppContext, text: &str| {
        cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned())));
        press(cx, handle, "ctrl-v");
    };
    click(cx, handle, "folder-export-name-a");
    for _ in 0..5 {
        input(cx, "a");
    }
    click(cx, handle, "folder-export-name-b");
    paste(cx, &"b".repeat(70));
    click(cx, handle, "folder-export-title");
    for _ in 0..5 {
        input(cx, "t");
    }
    click(cx, handle, "folder-export-note");
    let note = "note ".repeat(500);
    paste(cx, &note);
    let texts = fields(cx);
    assert_eq!(texts.each_ref().map(chars), [60, 60, 160, 2000]);
    assert_eq!(texts[0], "a".repeat(60));
    assert_eq!(texts[1], "b".repeat(60), "a paste keeps what fits");
    assert_eq!(texts[3], note[..2000]);
    // A full field takes nothing more (`fit`'s unit test covers a caret mid-text).
    input(cx, "x");
    assert_eq!(fields(cx)[3], note[..2000]);
    let o = choices(cx, &view).options;
    assert_eq!(
        [&o.names[0], &o.names[1], &o.title, &o.note],
        texts.each_ref(),
        "the choices are what the fields show"
    );
}

/// The fields give up their height in a short window, so Create stays on screen.
#[gpui_kit::test]
async fn create_stays_on_screen_in_a_short_window(cx: &mut TestAppContext) {
    let (_dir, a, b) = report_fixture();
    let (handle, view) = open_report_fixture(cx, &a, &b);
    wait_idle(cx, handle, &view).await;
    open_export(cx, handle, &view, ReportFormat::Html);
    let height = 420.;
    cx.simulate_window_resize(
        handle,
        gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(height)),
    );
    let bottom = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.find("folder-export-create").bounds().bottom()
        })
        .unwrap();
    assert!(
        bottom <= gpui_kit::px(height),
        "Create ends at {bottom:?} in a window of {height} px"
    );
}
