//! The header control, `ui.lang` and what a switch reaches.

use std::path::{Path, PathBuf};
use std::time::Duration;

use duet_app::app::APP_NAME;
use duet_app::folder::view::FolderView;
use duet_app::help::HelpView;
use duet_app::help::topics::{self, HelpLang, TOPICS};
use duet_app::i18n::{self, Lang};
use duet_app::launch::Launch;
use duet_app::text::view::TextView;
use duet_app::workspace::{TabItem, Workspace};
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{AnyWindowHandle, AppContext, Entity, TestAppContext};

use super::{open, thai_ui, with_data_dir};

const THAI: (&str, usize) = ("header-lang", 0);
const ENGLISH: (&str, usize) = ("header-lang", 1);

fn active_help(workspace: &Entity<Workspace>, cx: &mut TestAppContext) -> Entity<HelpView> {
    workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Help(view)) => view.clone(),
        _ => panic!("expected the help tab"),
    })
}

fn active_text(workspace: &Entity<Workspace>, cx: &mut TestAppContext) -> Entity<TextView> {
    workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected a text tab"),
    })
}

fn active_folder(workspace: &Entity<Workspace>, cx: &mut TestAppContext) -> Entity<FolderView> {
    workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Folder(view)) => view.clone(),
        _ => panic!("expected a folder tab"),
    })
}

/// Clicks with no draw after it: a dirty window draws itself when effects flush, as the real
/// window does. An explicit draw is a second frame the real window never gets (ui_merge/resolve.rs).
fn click(cx: &mut TestAppContext, handle: AnyWindowHandle, id: (&'static str, usize)) {
    cx.update_window(handle, |_, window, cx| window.click(id, cx))
        .unwrap();
}

fn press(cx: &mut TestAppContext, handle: AnyWindowHandle, key: &str) {
    cx.update_window(handle, |_, window, cx| window.press(key, cx))
        .unwrap();
}

/// What the last frame drew for `id`.
fn drawn_label(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    id: impl Into<gpui_kit::ElementId>,
) -> Option<String> {
    let id = id.into();
    cx.update_window(handle, |_, window, _| {
        window.find(id).label().map(str::to_owned)
    })
    .unwrap()
}

fn drawn_selected(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    id: (&'static str, usize),
) -> Option<bool> {
    cx.update_window(handle, |_, window, _| window.find(id).selected())
        .unwrap()
}

fn settings_of(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

/// Two folders holding one differing `a.txt`, and the pair's two files.
fn pair() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let (l, r) = (dir.path().join("l"), dir.path().join("r"));
    std::fs::create_dir_all(&l).unwrap();
    std::fs::create_dir_all(&r).unwrap();
    std::fs::write(l.join("a.txt"), "1\n").unwrap();
    std::fs::write(r.join("a.txt"), "2\n").unwrap();
    (dir, l, r)
}

async fn wait_text_loaded(cx: &mut TestAppContext, view: &Entity<TextView>) {
    for _ in 0..2000 {
        if !view.read_with(cx, |v, _| v.is_busy()) {
            return;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    panic!("the text tab never loaded");
}

#[gpui_kit::test]
fn the_header_switch_shows_on_the_frame_it_causes(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx, Launch::Start);
    press(cx, handle, "f1");
    let help = active_help(&workspace, cx);
    assert_eq!(
        drawn_label(cx, handle, "help-title").as_deref(),
        Some(TOPICS[0].en.title)
    );

    click(cx, handle, THAI);

    // The Help page is the one surface already Thai at this task.
    assert_eq!(
        drawn_label(cx, handle, "help-title").as_deref(),
        Some(TOPICS[0].th.title)
    );
    assert_eq!(
        drawn_label(cx, handle, ("help-toc", 0usize)).as_deref(),
        Some(TOPICS[0].th.title)
    );
    assert_eq!(drawn_selected(cx, handle, THAI), Some(true));
    assert_eq!(drawn_selected(cx, handle, ENGLISH), Some(false));
    assert_eq!(help.read_with(cx, |h, _| h.lang()), HelpLang::Th);
    let placeholder = help.read_with(cx, |h, cx| {
        h.search_input()
            .read(cx)
            .presentation()
            .placeholder()
            .to_string()
    });
    assert_eq!(placeholder, topics::page_copy(HelpLang::Th).search);

    click(cx, handle, ENGLISH);
    assert_eq!(
        drawn_label(cx, handle, "help-title").as_deref(),
        Some(TOPICS[0].en.title),
        "and back"
    );
}

#[gpui_kit::test]
fn the_header_control_marks_the_language_and_saves_ui_lang(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let settings = settings_of(data.path());
    let (handle, _workspace) = open(cx, Launch::Start);
    assert_eq!(drawn_label(cx, handle, THAI).as_deref(), Some("ไทย"));
    assert_eq!(drawn_label(cx, handle, ENGLISH).as_deref(), Some("EN"));
    assert_eq!(drawn_selected(cx, handle, THAI), Some(false));
    assert_eq!(drawn_selected(cx, handle, ENGLISH), Some(true));
    // It sits in the header, right before the theme button.
    cx.update_window(handle, |_, window, _| {
        let header = gpui_kit::ElementId::from("app-header");
        let control = window.find("header-lang");
        assert!(control.path().contains(&header));
        let (control, theme) = (control.bounds(), window.find("header-theme").bounds());
        assert!(
            control.right() <= theme.left() && theme.left() - control.right() < gpui_kit::px(20.),
            "{control:?} beside {theme:?}"
        );
    })
    .unwrap();

    // The language already chosen: nothing happens, and nothing is saved.
    click(cx, handle, ENGLISH);
    assert!(!settings.exists(), "no choice was made");

    click(cx, handle, THAI);
    assert_eq!(i18n::current(), Lang::Th);
    assert_eq!(drawn_selected(cx, handle, THAI), Some(true));
    assert_eq!(drawn_selected(cx, handle, ENGLISH), Some(false));
    assert_eq!(
        duet_app::settings::get_string(&settings, i18n::LANG_KEY).as_deref(),
        Some("th")
    );
    assert_eq!(i18n::LANG_KEY, "ui.lang");

    click(cx, handle, ENGLISH);
    assert_eq!(i18n::current(), Lang::En);
    assert_eq!(
        duet_app::settings::get_string(&settings, "ui.lang").as_deref(),
        Some("en")
    );
    assert_eq!(
        duet_app::settings::get_string(&settings, "help.lang"),
        None,
        "the old key is only ever read"
    );
}

#[gpui_kit::test]
fn a_window_opened_in_thai_marks_thai_and_english_takes_it_back(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let (handle, _workspace) = open(cx, Launch::Start);
    assert_eq!(drawn_selected(cx, handle, THAI), Some(true));
    assert_eq!(drawn_selected(cx, handle, ENGLISH), Some(false));
    click(cx, handle, ENGLISH);
    assert_eq!(i18n::current(), Lang::En);
    assert_eq!(drawn_selected(cx, handle, ENGLISH), Some(true));
}

#[gpui_kit::test]
fn a_help_lang_setting_is_the_starting_value(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let settings = settings_of(data.path());
    let write = |text: &str| std::fs::write(&settings, text).unwrap();

    assert_eq!(i18n::starting_lang(data.path(), true), Lang::Th, "no file");
    assert_eq!(i18n::starting_lang(data.path(), false), Lang::En, "no file");
    write(r#"{ "help.lang": "th" }"#);
    assert_eq!(
        i18n::starting_lang(data.path(), false),
        Lang::Th,
        "on English Windows too"
    );
    write(r#"{ "help.lang": "en" }"#);
    assert_eq!(
        i18n::starting_lang(data.path(), true),
        Lang::En,
        "on Thai Windows too"
    );
    write(r#"{ "ui.lang": "en", "help.lang": "th" }"#);
    assert_eq!(
        i18n::starting_lang(data.path(), true),
        Lang::En,
        "ui.lang wins"
    );
    write("not json {{");
    assert_eq!(i18n::starting_lang(data.path(), true), Lang::Th, "garbage");

    // The app started on that value: every surface reads it, and nothing was written.
    write(r#"{ "help.lang": "th" }"#);
    with_data_dir(cx, data.path());
    i18n::set(i18n::starting_lang(data.path(), false));
    let (handle, workspace) = open(cx, Launch::Start);
    press(cx, handle, "f1");
    assert_eq!(
        active_help(&workspace, cx).read_with(cx, |h, _| h.lang()),
        HelpLang::Th
    );
    assert_eq!(drawn_selected(cx, handle, THAI), Some(true));
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap(),
        r#"{ "help.lang": "th" }"#,
        "nothing is saved until the user chooses"
    );
}

#[gpui_kit::test]
async fn the_help_switch_moves_every_tab(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (_dir, l, r) = pair();
    let (handle, workspace) = open(
        cx,
        Launch::Files {
            left: l.join("a.txt"),
            right: r.join("a.txt"),
        },
    );
    let text = active_text(&workspace, cx);
    wait_text_loaded(cx, &text).await;
    text.update(cx, |v, cx| v.show_notice("A note in English", cx));
    press(cx, handle, "f1");
    let help = active_help(&workspace, cx);
    assert_eq!(drawn_selected(cx, handle, ENGLISH), Some(true));

    click(cx, handle, ("help-lang", 0));

    assert_eq!(i18n::current(), Lang::Th);
    assert_eq!(help.read_with(cx, |h, _| h.lang()), HelpLang::Th);
    assert_eq!(
        drawn_label(cx, handle, "help-title").as_deref(),
        Some(TOPICS[0].th.title),
        "the Help page, on this frame"
    );
    assert_eq!(
        drawn_selected(cx, handle, THAI),
        Some(true),
        "the header too, on this frame"
    );
    assert_eq!(
        text.read_with(cx, |v, _| v.notice().map(str::to_owned)),
        None,
        "the text tab, not active, was relabelled"
    );
    assert_eq!(
        duet_app::settings::get_string(&settings_of(data.path()), "ui.lang").as_deref(),
        Some("th")
    );

    // A second click on the language in force changes nothing.
    text.update(cx, |v, cx| v.show_notice("Another note", cx));
    click(cx, handle, ("help-lang", 0));
    assert_eq!(
        text.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("Another note")
    );

    // And the header moves the Help tab's own switch.
    click(cx, handle, ENGLISH);
    assert_eq!(help.read_with(cx, |h, _| h.lang()), HelpLang::En);
    assert_eq!(drawn_selected(cx, handle, ("help-lang", 1)), Some(true));
}

#[gpui_kit::test]
async fn switching_language_keeps_edits_and_starts_no_job(cx: &mut TestAppContext) {
    let (_dir, l, r) = pair();
    let (handle, workspace) = open(
        cx,
        Launch::Files {
            left: l.join("a.txt"),
            right: r.join("a.txt"),
        },
    );
    // A dirty text tab.
    let text = active_text(&workspace, cx);
    wait_text_loaded(cx, &text).await;
    text.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    cx.update_window(handle, |_, window, cx| window.input("z", cx))
        .unwrap();
    assert!(text.read_with(cx, |v, _| v.is_dirty()));
    // The edit's own re-diff ends first, so what runs after the click is the click's.
    wait_text_loaded(cx, &text).await;
    let text_job = text.read_with(cx, |v, _| v.job_generation());

    // A folder tab with its sync panel open and a batch running.
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| w.open_folders(l.clone(), r.clone(), window, cx))
    })
    .unwrap();
    let folder = active_folder(&workspace, cx);
    let waited = folder.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        let v = waited.read(cx);
        v.model().is_some() && !v.is_busy()
    })
    .await;
    cx.update_window(handle, |_, window, cx| window.click("folder-sync", cx))
        .unwrap();
    let waited = folder.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        waited.read(cx).sync_plan_shown().is_some()
    })
    .await;
    folder.update(cx, |v, cx| v.hold_batch_for_test("Copying", 3, cx));
    let folder_job = folder.read_with(cx, |v, _| v.job_generation());
    let tabs_before = workspace.read_with(cx, |w, _| (w.tabs().len(), w.tabs().active_index()));

    let check = |cx: &mut TestAppContext| {
        assert!(text.read_with(cx, |v, _| v.is_dirty()), "the edit stays");
        assert_eq!(text.read_with(cx, |v, _| v.job_generation()), text_job);
        assert!(
            !text.read_with(cx, |v, _| v.is_busy()),
            "no compare started"
        );
        assert_eq!(folder.read_with(cx, |v, _| v.job_generation()), folder_job);
        assert!(
            folder.read_with(cx, |v, _| v.batch_running()),
            "the batch runs on"
        );
        assert!(
            folder.read_with(cx, |v, _| v.sync_open()),
            "the panel stays"
        );
        assert!(
            cx.update_window(handle, |_, window, _| {
                window.try_find("folder-sync-panel").is_some()
            })
            .unwrap(),
            "and is drawn"
        );
        assert!(!cx.has_pending_prompt(), "nothing asks");
        assert_eq!(
            workspace.read_with(cx, |w, _| (w.tabs().len(), w.tabs().active_index())),
            tabs_before
        );
    };

    click(cx, handle, THAI);
    assert_eq!(i18n::current(), Lang::Th);
    check(cx);
    click(cx, handle, ENGLISH);
    assert_eq!(i18n::current(), Lang::En);
    check(cx);
}

#[gpui_kit::test]
async fn the_language_stays_on_its_own_test_thread(cx: &mut TestAppContext) {
    let (handle, _workspace) = open(cx, Launch::Start);
    click(cx, handle, THAI);
    assert_eq!(i18n::current(), Lang::Th);
    let other = std::thread::spawn(i18n::current).join().unwrap();
    assert_eq!(other, Lang::En, "another test's thread still reads English");
    // Under test the executors run on this thread, so a view's task sees its test's language.
    let task = cx.executor().spawn(async { i18n::current() }).await;
    assert_eq!(task, Lang::Th);
}

/// The window's title as the last frame left it.
fn window_title(cx: &mut TestAppContext, workspace: &Entity<Workspace>) -> String {
    workspace.read_with(cx, |w, _| w.window_title_for_test().to_owned())
}

#[gpui_kit::test]
fn the_header_and_tabs_read_in_thai_on_the_switch_frame(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx, Launch::Start);
    press(cx, handle, "f1");
    assert_eq!(
        drawn_label(cx, handle, "header-help").as_deref(),
        Some("Help (F1)")
    );
    assert_eq!(window_title(cx, &workspace), format!("Help — {APP_NAME}"));

    click(cx, handle, THAI);

    // No draw after the click: these are the frame the click caused.
    assert_eq!(
        drawn_label(cx, handle, "header-help").as_deref(),
        Some("คู่มือ (F1)")
    );
    assert_eq!(
        drawn_label(cx, handle, "header-theme").as_deref(),
        Some("สลับธีมสว่างและธีมมืด")
    );
    assert_eq!(
        drawn_label(cx, handle, "tab-new").as_deref(),
        Some("เทียบใหม่ (Ctrl+T)")
    );
    assert_eq!(
        drawn_label(cx, handle, ("tab-close", 1usize)).as_deref(),
        Some("ปิดแท็บ (Ctrl+W)")
    );
    assert_eq!(window_title(cx, &workspace), format!("คู่มือ — {APP_NAME}"));

    // The Start tab is not showing, and its boxes were built in English: their placeholders are
    // set by the switch itself, not by a later draw.
    let start = workspace.read_with(cx, |w, _| {
        w.tabs()
            .iter()
            .find_map(|(_, _, item)| match item {
                TabItem::Start(view) => Some(view.clone()),
                _ => None,
            })
            .expect("the start tab")
    });
    let placeholders = start.read_with(cx, |v, cx| {
        [v.left_input(), v.right_input()]
            .map(|input| input.read(cx).presentation().placeholder().to_string())
    });
    let thai = |key: &'static str| {
        let _th = i18n::scoped(Lang::Th);
        i18n::tr(key).to_owned()
    };
    assert_eq!(
        placeholders,
        [thai("Left file or folder"), thai("Right file or folder")]
    );

    // The tab that was not showing reads Thai as soon as it is.
    click(cx, handle, ("tab", 0usize));
    assert_eq!(window_title(cx, &workspace), format!("เริ่มต้น — {APP_NAME}"));
    assert_eq!(
        drawn_label(cx, handle, ("tab-close", 0usize)).as_deref(),
        Some("ปิดแท็บ (Ctrl+W)")
    );

    click(cx, handle, ENGLISH);
    assert_eq!(
        window_title(cx, &workspace),
        format!("Start — {APP_NAME}"),
        "and back"
    );
    assert_eq!(
        drawn_label(cx, handle, "header-help").as_deref(),
        Some("Help (F1)")
    );
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

/// Two text files in a data folder of their own, with the given contents.
fn text_pair(left: &str, right: &str) -> (tempfile::TempDir, Launch) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, left).unwrap();
    std::fs::write(&b, right).unwrap();
    (dir, Launch::Files { left: a, right: b })
}

#[gpui_kit::test]
async fn the_tab_badge_keeps_its_colours_in_thai(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let (_dir, launch) = text_pair("same\n", "same\n");
    let (_handle, workspace) = open(cx, launch);
    let text = active_text(&workspace, cx);
    wait_text_loaded(cx, &text).await;
    let badge = workspace
        .read_with(cx, |w, cx| w.tab_badge(0, cx))
        .expect("an identical pair has a badge");
    assert_eq!(badge, "เหมือนกัน");
    assert!(duet_app::tabs::is_ok_badge(&badge), "drawn green");
    assert!(!duet_app::tabs::is_ok_badge("3"), "a count is not");
    assert!(
        !duet_app::tabs::is_ok_badge("ok"),
        "nor is the English word"
    );

    let (_dir, launch) = text_pair("one\n", "two\n");
    let (_handle, workspace) = open(cx, launch);
    let text = active_text(&workspace, cx);
    wait_text_loaded(cx, &text).await;
    let badge = workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)).unwrap();
    assert_eq!(badge, "1");
    assert!(!duet_app::tabs::is_ok_badge(&badge));
}

#[gpui_kit::test]
async fn the_binary_badge_reads_in_thai(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let (_dir, launch) = text_pair("a\0b", "a\0b");
    let (_handle, workspace) = open(cx, launch);
    let text = active_text(&workspace, cx);
    wait_text_loaded(cx, &text).await;
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        Some("เหมือนกัน".to_owned()),
        "identical binary files"
    );

    let (_dir, launch) = text_pair("a\0b", "a\0c");
    let (_handle, workspace) = open(cx, launch);
    let text = active_text(&workspace, cx);
    wait_text_loaded(cx, &text).await;
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        Some("≠".to_owned()),
        "different binary files"
    );
}

#[test]
fn tab_titles_read_in_thai() {
    use duet_app::tabs::TabKey;
    let some = |p: &str| Some(Path::new(p).to_path_buf());
    let _th = thai_ui();
    assert_eq!(TabKey::Start(1).title(), "เริ่มต้น");
    assert_eq!(TabKey::Help.title(), "คู่มือ");
    assert_eq!(TabKey::merge(None, None, None, 1).title(), "รวมสามทาง");
    assert_eq!(
        TabKey::merge(some("C:/x/l.txt").as_deref(), None, None, 1).title(),
        "รวม: l.txt ↔ ?"
    );
    assert_eq!(
        TabKey::Files("".into(), "C:/x/new.rs".into()).title(),
        "ไม่มีชื่อ ↔ new.rs"
    );
    // A file named like a label stays as it is.
    assert_eq!(
        TabKey::Files("C:/x/Start".into(), "C:/y/Help".into()).title(),
        "Start ↔ Help"
    );
    assert!(i18n::missing_keys_for_test().is_empty());
}
