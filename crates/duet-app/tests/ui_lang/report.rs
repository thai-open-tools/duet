//! The folder export panel's report language: where it starts, that it is the report's own, and
//! that a job keeps the language it began with.

use std::path::Path;
use std::time::Duration;

use duet_app::folder::view::FolderView;
use duet_app::i18n::{self, Lang, tr};
use duet_core::folder::report::ReportFormat;
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, ScrollDelta, TestAppContext, point, px};

use super::folder::{fixture, label, open_folders, wait_idle};
use super::table::open_table;
use super::text::{open_text, wait_loaded};
use super::{thai_ui, with_data_dir};
use duet_app::table::view::TableView;
use duet_app::text::view::TextView;
use gpui_kit::Focusable as _;

const LANG_TH: (&str, usize) = ("folder-export-lang", 0);
const LANG_EN: (&str, usize) = ("folder-export-lang", 1);
const UI_TH: (&str, usize) = ("header-lang", 0);
const UI_EN: (&str, usize) = ("header-lang", 1);

fn click(cx: &mut TestAppContext, handle: AnyWindowHandle, id: (&'static str, usize)) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        // The row may lie below the fields' fold; the header's own control never does.
        if id.0 == "folder-export-lang" {
            window.scroll(
                "folder-export-fields",
                ScrollDelta::Pixels(point(px(0.), px(-1000.))),
                cx,
            );
            window.render_frame(cx);
        }
        window.click(id, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn label_of(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    id: (&'static str, usize),
) -> Option<String> {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find(id).label().map(str::to_owned)
    })
    .unwrap()
}

fn selected(cx: &mut TestAppContext, handle: AnyWindowHandle, id: (&'static str, usize)) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find(id).selected()
    })
    .unwrap()
    .expect("a segment is selected or not")
}

fn open_panel(
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

fn report_lang(cx: &mut TestAppContext, view: &Entity<FolderView>) -> Lang {
    view.read_with(cx, |v, _| v.export_choices().unwrap().options.lang)
}

/// Create, scrolled into view first, then the save dialog answered with `out`.
fn create_into(cx: &mut TestAppContext, handle: AnyWindowHandle, out: &Path) {
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
    assert!(
        cx.did_prompt_for_new_path(),
        "the export asks where to save"
    );
    cx.simulate_new_path_selection(|_| Some(out.to_path_buf()));
}

async fn wait_export(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<FolderView>) {
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        let v = v.read(cx);
        !v.export_running() && v.notice().is_some()
    })
    .await;
}

fn header_of(csv: &Path) -> String {
    let text = std::fs::read_to_string(csv).unwrap();
    let text = text.strip_prefix('\u{feff}').expect("the BOM stays");
    text.lines().next().unwrap().to_owned()
}

#[gpui_kit::test]
async fn the_export_panel_starts_at_the_ui_language(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (l, r) = fixture(&[]);
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;

    open_panel(cx, handle, &view, ReportFormat::Csv);
    assert_eq!(report_lang(cx, &view), Lang::En, "an English window");
    assert!(selected(cx, handle, LANG_EN));
    assert!(!selected(cx, handle, LANG_TH));
    assert_eq!(label(cx, handle, "folder-export-help-lang").unwrap(), {
        "The language of the whole report, column names and status words included. Choose English if a script reads the file."
    });

    click(cx, handle, LANG_TH);
    assert_eq!(report_lang(cx, &view), Lang::Th);
    assert!(selected(cx, handle, LANG_TH));
    // The window did not move: the choice is the report's own.
    assert_eq!(i18n::current(), Lang::En);
}

#[gpui_kit::test]
async fn the_export_panel_of_a_thai_window_starts_in_thai(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (l, r) = fixture(&[]);
    let _th = thai_ui();
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    open_panel(cx, handle, &view, ReportFormat::Csv);
    assert_eq!(report_lang(cx, &view), Lang::Th, "a Thai window");
    assert!(selected(cx, handle, LANG_TH));
    assert_eq!(label_of(cx, handle, LANG_TH).as_deref(), Some("ไทย"));
    assert_eq!(label_of(cx, handle, LANG_EN).as_deref(), Some("English"));
    assert_eq!(
        label(cx, handle, "folder-export-help-lang").unwrap(),
        tr(
            "The language of the whole report, column names and status words included. Choose English if a script reads the file."
        )
    );
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

fn click_ui(cx: &mut TestAppContext, handle: AnyWindowHandle, id: (&'static str, usize)) {
    click(cx, handle, id);
}

#[gpui_kit::test]
async fn a_thai_ui_exports_an_english_report_and_the_reverse(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (l, r) = fixture(&[]);
    let out_dir = tempfile::tempdir().unwrap();
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;

    // A Thai window, an English report.
    {
        let _th = thai_ui();
        click_ui(cx, handle, UI_TH);
        open_panel(cx, handle, &view, ReportFormat::Csv);
        assert_eq!(report_lang(cx, &view), Lang::Th);
        click(cx, handle, LANG_EN);
        let out = out_dir.path().join("english.csv");
        create_into(cx, handle, &out);
        wait_export(cx, handle, &view).await;
        assert!(
            header_of(&out).starts_with("No.,Folder,File name,Type,Status,Needs a look"),
            "{}",
            header_of(&out)
        );
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(
            text.contains(",Different,") && text.contains(",Only in "),
            "{text}"
        );
    }

    // An English window, a Thai report.
    click_ui(cx, handle, UI_EN);
    assert_eq!(i18n::current(), Lang::En);
    open_panel(cx, handle, &view, ReportFormat::Csv);
    assert_eq!(report_lang(cx, &view), Lang::En);
    click(cx, handle, LANG_TH);
    let out = out_dir.path().join("thai.csv");
    create_into(cx, handle, &out);
    wait_export(cx, handle, &view).await;
    assert!(
        header_of(&out).starts_with("ลำดับ,โฟลเดอร์,ชื่อไฟล์,ชนิด,สถานะ,ควรตรวจดู"),
        "{}",
        header_of(&out)
    );
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains(",ต่างกัน,"), "{text}");
    assert!(!text.contains("Different"));
}

#[gpui_kit::test]
async fn switching_the_ui_during_an_export_leaves_the_report_alone(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (l, r) = fixture(&[]);
    let out_dir = tempfile::tempdir().unwrap();
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;

    // The report is Thai from an English window; the job waits, the window turns Thai and back.
    view.update(cx, |v, _| v.hold_export_for_test(true));
    open_panel(cx, handle, &view, ReportFormat::Text);
    click(cx, handle, LANG_TH);
    let out = out_dir.path().join("report.txt");
    create_into(cx, handle, &out);
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.export_running()));

    click_ui(cx, handle, UI_TH);
    click_ui(cx, handle, UI_EN);
    view.update(cx, |v, _| v.hold_export_for_test(false));
    wait_export(cx, handle, &view).await;

    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.starts_with("\u{feff}รายงานเทียบโฟลเดอร์"), "{text}");
    assert!(!text.contains("Folder compare report"));
    // And the other way round: an English report while the window is Thai.
    click_ui(cx, handle, UI_TH);
    let _th = thai_ui();
    view.update(cx, |v, _| v.hold_export_for_test(true));
    open_panel(cx, handle, &view, ReportFormat::Text);
    click(cx, handle, LANG_EN);
    let out = out_dir.path().join("english.txt");
    create_into(cx, handle, &out);
    cx.run_until_parked();
    click_ui(cx, handle, UI_EN);
    click_ui(cx, handle, UI_TH);
    view.update(cx, |v, _| v.hold_export_for_test(false));
    wait_export(cx, handle, &view).await;
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.starts_with("\u{feff}Folder compare report"), "{text}");
}

// ---- The text and table tabs: a report language of their own ----

fn run_entries<V: gpui_kit::Focusable + 'static>(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<V>,
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

fn text_language_items(cx: &mut TestAppContext, view: &Entity<TextView>) -> Vec<String> {
    view.read_with(cx, |v, _| {
        duet_app::ui::menu::outline(&duet_app::text::report::report_language_menu(v))
    })
}

fn pick_text_language(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
    label: &str,
) {
    let entries = view.read_with(cx, |v, _| duet_app::text::report::report_language_menu(v));
    assert!(run_entries(cx, handle, view, entries, label), "{label}");
}

fn run_text_report(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
    label: &str,
) {
    let entries = view.read_with(cx, |v, _| duet_app::text::report::report_menu(v));
    assert!(run_entries(cx, handle, view, entries, label), "{label}");
}

/// Answers the save dialog with `out` and waits for the report to land.
async fn save_text_report(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
    out: &Path,
) {
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| Some(out.to_path_buf()));
    let seen = view.clone();
    let named = out.display().to_string();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        seen.read(cx).notice().is_some_and(|n| n.contains(&named))
    })
    .await;
}

#[gpui_kit::test]
async fn the_text_report_menu_offers_the_language(cx: &mut TestAppContext) {
    let (_dir, handle, _ws, view) = open_text(cx, "a\nb\n", "a\nc\n");
    wait_loaded(cx, handle, &view).await;
    // An English window starts on English, and the choice is the report's own.
    assert_eq!(text_language_items(cx, &view), ["-", "ไทย", "● English"]);
    pick_text_language(cx, handle, &view, "ไทย");
    assert_eq!(text_language_items(cx, &view), ["-", "● ไทย", "English"]);
    assert_eq!(i18n::current(), Lang::En, "the window did not move");
    // The report items themselves are as they were.
    assert_eq!(
        view.read_with(cx, |v, _| duet_app::ui::menu::outline(
            &duet_app::text::report::report_menu(v)
        ))
        .len(),
        5
    );
    // A Thai window starts on Thai.
    let _th = thai_ui();
    let (_dir, handle, _ws, thai) = open_text(cx, "a\nb\n", "a\nc\n");
    wait_loaded(cx, handle, &thai).await;
    assert_eq!(text_language_items(cx, &thai), ["-", "● ไทย", "English"]);
}

#[gpui_kit::test]
async fn a_thai_ui_writes_an_english_text_report(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let (dir, handle, _ws, view) = open_text(cx, "a\nb\nc\n", "a\nB\nc\n");
    wait_loaded(cx, handle, &view).await;

    // Untouched, the report follows the window: Thai.
    let thai = dir.path().join("th.txt");
    run_text_report(cx, handle, &view, tr("Text report…"));
    save_text_report(cx, handle, &view, &thai).await;
    let written = std::fs::read_to_string(&thai).unwrap();
    assert!(
        written.starts_with("\u{feff}duet: รายงานเทียบข้อความ\r\nฝั่งซ้าย: a.txt\r\nฝั่งขวา: b.txt\r\n"),
        "{written:?}"
    );
    assert!(written.contains("=== จุดต่างที่ 1 · "), "{written:?}");

    // English picked: the report is English, the window and its notice stay Thai.
    pick_text_language(cx, handle, &view, "English");
    let english = dir.path().join("en.txt");
    run_text_report(cx, handle, &view, tr("Text report…"));
    save_text_report(cx, handle, &view, &english).await;
    let written = std::fs::read_to_string(&english).unwrap();
    assert!(
        written.starts_with(
            "\u{feff}duet: text compare report\r\nLeft : a.txt\r\nRight: b.txt\r\nDate : "
        ),
        "{written:?}"
    );
    assert!(written.contains("=== Difference 1 · Left 2-2 · Right 2-2 ==="));
    assert!(
        !written
            .chars()
            .any(|c| ('\u{0E00}'..='\u{0E7F}').contains(&c)),
        "no Thai in an English report: {written:?}"
    );
    assert_eq!(i18n::current(), Lang::Th);

    // The HTML page carries its language, and its status line was built in it.
    run_text_report(cx, handle, &view, tr("HTML report (side by side)…"));
    cx.simulate_prompt_answer(tr("All lines"));
    cx.run_until_parked();
    let page_en = dir.path().join("en.html");
    save_text_report(cx, handle, &view, &page_en).await;
    let page = std::fs::read_to_string(&page_en).unwrap();
    assert!(
        page.starts_with("<!DOCTYPE html><html lang=\"en\">"),
        "{page}"
    );
    assert!(page.contains("<h1>Text compare report</h1>"));

    pick_text_language(cx, handle, &view, "ไทย");
    run_text_report(cx, handle, &view, tr("HTML report (side by side)…"));
    cx.simulate_prompt_answer(tr("All lines"));
    cx.run_until_parked();
    let page_th = dir.path().join("th.html");
    save_text_report(cx, handle, &view, &page_th).await;
    let page = std::fs::read_to_string(&page_th).unwrap();
    assert!(
        page.starts_with("<!DOCTYPE html><html lang=\"th\">"),
        "{page}"
    );
    assert!(page.contains("<h1>รายงานเทียบข้อความ</h1>"));
    assert!(page.contains("ฝั่งซ้าย: <b>a.txt</b>"));
}

#[gpui_kit::test]
async fn the_patch_stays_english_in_a_thai_text_tab(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let (dir, handle, _ws, view) = open_text(cx, "a\nb\nc\n", "a\nB\nc\n");
    wait_loaded(cx, handle, &view).await;
    let patch = dir.path().join("out.patch");
    run_text_report(cx, handle, &view, tr("Patch file (unified diff)…"));
    save_text_report(cx, handle, &view, &patch).await;
    assert_eq!(
        std::fs::read(&patch).unwrap(),
        b"--- a/a.txt\n+++ b/b.txt\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n"
    );
}

/// Reloads, then runs an Export item and answers the save dialog with `out`.
async fn export_table(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TableView>,
    label: &str,
    out: &Path,
) {
    view.update(cx, |v, cx| v.reload(cx));
    let seen = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !seen.read(cx).is_busy()
    })
    .await;
    let entries = view.read_with(cx, |v, _| duet_app::table::menu::export_menu(v));
    assert!(run_entries(cx, handle, view, entries, label), "{label}");
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| Some(out.to_path_buf()));
    let seen = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        seen.read(cx).notice().is_some_and(|n| !n.is_empty())
    })
    .await;
}

#[gpui_kit::test]
async fn the_table_export_menu_offers_the_language_and_writes_it(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let dir = tempfile::tempdir().unwrap();
    let (handle, _ws, view) =
        open_table(cx, dir.path(), "id,v\n1,a\n2,b\n", "id,v\n1,a\n2,c\n").await;
    let items = |cx: &mut TestAppContext| {
        view.read_with(cx, |v, _| {
            duet_app::ui::menu::outline(&duet_app::table::menu::export_language_menu(v))
        })
    };
    assert_eq!(items(cx), ["-", "● ไทย", "English"], "a Thai window");

    // Thai, untouched: the window's language.
    let th = dir.path().join("th.html");
    export_table(cx, handle, &view, tr("HTML report"), &th).await;
    let page = std::fs::read_to_string(&th).unwrap();
    assert!(
        page.starts_with("<!DOCTYPE html><html lang=\"th\">"),
        "{page}"
    );
    assert!(page.contains("<h1>รายงานเทียบตาราง</h1>"));

    // English picked: the CSV and the page are English, the window stays Thai.
    let entries = view.read_with(cx, |v, _| duet_app::table::menu::export_language_menu(v));
    assert!(run_entries(cx, handle, &view, entries, "English"));
    assert_eq!(items(cx), ["-", "ไทย", "● English"]);
    assert_eq!(i18n::current(), Lang::Th);
    let csv = dir.path().join("en.csv");
    export_table(cx, handle, &view, tr("Differences (CSV)"), &csv).await;
    let text = std::fs::read_to_string(&csv).unwrap();
    assert!(
        text.starts_with("\u{feff}Status,Left row,Right row,Key,Column,Left value,Right value\r\n"),
        "{text:?}"
    );
    let en = dir.path().join("en.html");
    export_table(cx, handle, &view, tr("HTML report"), &en).await;
    let page = std::fs::read_to_string(&en).unwrap();
    assert!(
        page.starts_with("<!DOCTYPE html><html lang=\"en\">"),
        "{page}"
    );
    assert!(page.contains("<h1>Table compare report</h1>"));
    assert!(
        !page.chars().any(|c| ('\u{0E00}'..='\u{0E7F}').contains(&c)),
        "no Thai in an English report"
    );
}

// ---- The names a report gives its two sides ----

/// Two folders called `config` under parents of the same name: the only pair whose default names
/// are words ("config (left)") and not just the folders' names.
fn twin_folders() -> (tempfile::TempDir, tempfile::TempDir) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for (side, text) in [(&l, "left"), (&r, "right side")] {
        let dir = side.path().join("p").join("config");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), text).unwrap();
    }
    (l, r)
}

fn names_of(cx: &mut TestAppContext, view: &Entity<FolderView>) -> [String; 2] {
    view.read_with(cx, |v, _| v.export_choices().unwrap().options.names.clone())
}

fn pair(a: &str, b: &str) -> [String; 2] {
    [a.to_owned(), b.to_owned()]
}

#[gpui_kit::test]
async fn a_report_language_change_rebuilds_the_names_still_at_their_default(
    cx: &mut TestAppContext,
) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (l, r) = twin_folders();
    let _th = thai_ui();
    let (handle, _workspace, view) = open_folders(
        cx,
        &l.path().join("p").join("config"),
        &r.path().join("p").join("config"),
    );
    wait_idle(cx, handle, &view).await;
    open_panel(cx, handle, &view, ReportFormat::Csv);
    let thai = names_of(cx, &view);
    assert_eq!(thai, pair("config (ซ้าย)", "config (ขวา)"));

    click(cx, handle, LANG_EN);
    assert_eq!(
        names_of(cx, &view),
        pair("config (left)", "config (right)"),
        "an untouched name follows the report language"
    );
    click(cx, handle, LANG_TH);
    assert_eq!(names_of(cx, &view), thai);

    // A name the person typed stays; the other one still follows.
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.set_export_choices_for_test(|c| c.options.names[0] = "mine".into(), window, cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
    click(cx, handle, LANG_EN);
    assert_eq!(names_of(cx, &view), pair("mine", "config (right)"));
    // An emptied name falls back to the default of the language it is created in.
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.set_export_choices_for_test(|c| c.options.names[0] = String::new(), window, cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
    click(cx, handle, LANG_TH);
    let finished = view.read_with(cx, |v, _| v.export_choices().unwrap().options.names.clone());
    assert_eq!(finished[1], "config (ขวา)");
}

#[gpui_kit::test]
async fn default_names_are_not_remembered_in_the_language_they_were_made_in(
    cx: &mut TestAppContext,
) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (l, r) = twin_folders();
    let (left, right) = (
        l.path().join("p").join("config"),
        r.path().join("p").join("config"),
    );
    let out_dir = tempfile::tempdir().unwrap();
    let (handle, _workspace, view) = open_folders(cx, &left, &right);
    wait_idle(cx, handle, &view).await;
    {
        let _th = thai_ui();
        click_ui(cx, handle, UI_TH);
        open_panel(cx, handle, &view, ReportFormat::Csv);
        assert_eq!(names_of(cx, &view)[0], "config (ซ้าย)");
        create_into(cx, handle, &out_dir.path().join("thai.csv"));
        wait_export(cx, handle, &view).await;
    }
    let saved = std::fs::read_to_string(data.path().join("settings.json")).unwrap();
    assert!(!saved.contains("ซ้าย"), "{saved}");
    // The pair opens again in English: its names are English, not the Thai that was shown.
    click_ui(cx, handle, UI_EN);
    open_panel(cx, handle, &view, ReportFormat::Csv);
    assert_eq!(
        names_of(cx, &view),
        pair("config (left)", "config (right)"),
        "{saved}"
    );
    // A name the person typed is remembered as typed.
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.set_export_choices_for_test(|c| c.options.names[0] = "mine".into(), window, cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
    create_into(cx, handle, &out_dir.path().join("mine.csv"));
    wait_export(cx, handle, &view).await;
    open_panel(cx, handle, &view, ReportFormat::Csv);
    assert_eq!(names_of(cx, &view), pair("mine", "config (right)"));
}

// ---- What a report builds from the view: the report's language, not the window's ----

#[gpui_kit::test]
async fn a_text_report_in_the_other_language_says_its_notes_and_title_in_it(
    cx: &mut TestAppContext,
) {
    let _th = thai_ui();
    let (dir, handle, _ws, view) = open_text(cx, "{\"a\":1}", "{\"a\":2}");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_format_for_test(true, cx));
    wait_loaded(cx, handle, &view).await;
    // The window's own status carries the note in Thai.
    let note = view.read_with(cx, |v, _| v.model().unwrap().notes());
    assert!(note[0].contains("ซ้าย"), "{note:?}");

    pick_text_language(cx, handle, &view, "English");
    run_text_report(cx, handle, &view, tr("HTML report (side by side)…"));
    cx.simulate_prompt_answer(tr("All lines"));
    cx.run_until_parked();
    let en = dir.path().join("en.html");
    save_text_report(cx, handle, &view, &en).await;
    let page = std::fs::read_to_string(&en).unwrap();
    assert!(page.contains("Left: formatted JSON"), "{page}");
    assert!(
        !page.chars().any(|c| ('\u{0E00}'..='\u{0E7F}').contains(&c)),
        "no Thai in an English report"
    );
    assert!(page.contains("<title>duet — a.txt vs b.txt</title>"));

    // And the title of a Thai page has no English joining word.
    pick_text_language(cx, handle, &view, "ไทย");
    run_text_report(cx, handle, &view, tr("HTML report (side by side)…"));
    cx.simulate_prompt_answer(tr("All lines"));
    cx.run_until_parked();
    let th = dir.path().join("th.html");
    save_text_report(cx, handle, &view, &th).await;
    let page = std::fs::read_to_string(&th).unwrap();
    assert!(
        page.contains("<title>duet — a.txt กับ b.txt</title>"),
        "{page}"
    );
    assert!(!page.contains(" vs "));
}

#[gpui_kit::test]
async fn a_pasted_side_is_named_in_the_report_language(cx: &mut TestAppContext) {
    use gpui_kit::{Bounds, WindowBounds, WindowOptions, size};
    let _th = thai_ui();
    let dir = tempfile::tempdir().unwrap();
    let (handle, view) = cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(0.), px(0.)),
                    size: size(px(1000.), px(700.)),
                })),
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| TableView::with_text_for_test("id,v\n1,a\n", "id,v\n1,b\n", window, cx))
            },
        )
        .expect("open test window")
    });
    let seen = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !seen.read(cx).is_busy()
    })
    .await;
    let entries = view.read_with(cx, |v, _| duet_app::table::menu::export_language_menu(v));
    assert!(run_entries(cx, handle, &view, entries, "English"));
    let en = dir.path().join("en.html");
    export_table(cx, handle, &view, tr("HTML report"), &en).await;
    let page = std::fs::read_to_string(&en).unwrap();
    assert!(page.contains("Pasted text (left)"), "{page}");
    assert!(
        !page.chars().any(|c| ('\u{0E00}'..='\u{0E7F}').contains(&c)),
        "no Thai in an English report"
    );
}
