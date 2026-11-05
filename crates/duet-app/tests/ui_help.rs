//! Headless UI tests for the Help tab: F1, the language switch, search, Escape and moving
//! between topics.

use duet_app::help::HelpView;
use duet_app::help::topics::{self, GROUPS, HelpLang, TOPICS};
use duet_app::launch::Launch;
use duet_app::tabs::TabKey;
use duet_app::workspace::{TabItem, Workspace};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, Focusable, Point, Render, ScrollDelta,
    TestAppContext, Window, WindowBounds, WindowOptions, point, px, size,
};

/// Help's language follows the UI's, and English is the default under test.
fn thai_ui() -> duet_app::i18n::LangScope {
    duet_app::i18n::scoped(duet_app::i18n::Lang::Th)
}

fn open_test_window<V: Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
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

fn open(cx: &mut TestAppContext, launch: Launch) -> (AnyWindowHandle, Entity<Workspace>) {
    open_test_window(cx, move |window, cx| {
        cx.new(|cx| Workspace::new(launch, window, cx))
    })
}

fn with_data_dir(cx: &mut TestAppContext, dir: &std::path::Path) {
    let data_dir = dir.to_path_buf();
    cx.update(|cx| cx.set_global(duet_app::app::AppPaths { data_dir }));
}

fn kinds(workspace: &Entity<Workspace>, cx: &mut TestAppContext) -> Vec<&'static str> {
    workspace.read_with(cx, |w, _| {
        w.tabs()
            .iter()
            .map(|(_, _, item)| match item {
                TabItem::Start(_) => "start",
                TabItem::Folder(_) => "folder",
                TabItem::Text(_) => "text",
                TabItem::Hex(_) => "hex",
                TabItem::Table(_) => "table",
                TabItem::Picture(_) => "picture",
                TabItem::Merge(_) => "merge",
                TabItem::Help(_) => "help",
            })
            .collect()
    })
}

fn active_help(workspace: &Entity<Workspace>, cx: &mut TestAppContext) -> Entity<HelpView> {
    workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Help(view)) => view.clone(),
        _ => panic!("expected the help tab"),
    })
}

fn press(cx: &mut TestAppContext, handle: AnyWindowHandle, key: &str) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(key, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn click(cx: &mut TestAppContext, handle: AnyWindowHandle, id: impl Into<gpui_kit::ElementId>) {
    let id = id.into();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// Clicks topic-list entry `i`, scrolled into the list's view first: with its group labels the
/// list is taller than the test window (U10).
fn click_toc(cx: &mut TestAppContext, handle: AnyWindowHandle, i: usize) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let list = window.find("help-toc").bounds();
        let entry = window.find(("help-toc", i)).bounds();
        let dy = if entry.bottom() > list.bottom() {
            list.bottom() - entry.bottom()
        } else if entry.top() < list.top() {
            list.top() - entry.top()
        } else {
            px(0.)
        };
        if dy != px(0.) {
            window.scroll("help-toc", ScrollDelta::Pixels(point(px(0.), dy)), cx);
            window.render_frame(cx);
        }
        window.click(("help-toc", i), cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// Previous and Next sit under the topic: scroll down to them, then click.
fn click_nav(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let delta = ScrollDelta::Pixels(point(px(0.), px(-100_000.)));
        window.scroll("help-topic", delta, cx);
        window.render_frame(cx);
        window.click(id, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn label(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    id: impl Into<gpui_kit::ElementId>,
) -> String {
    let id = id.into();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window
            .find(id)
            .label()
            .map(str::to_owned)
            .unwrap_or_default()
    })
    .unwrap()
}

fn topic(id: &str) -> &'static topics::Topic {
    TOPICS.iter().find(|t| t.id == id).unwrap()
}

fn index_of(id: &str) -> usize {
    TOPICS.iter().position(|t| t.id == id).unwrap()
}

/// Every string the page shows in `lang`: the topic list, the title, Previous and Next, and
/// the search box's placeholder.
fn assert_page_reads(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    help: &Entity<HelpView>,
    lang: HelpLang,
    current: &str,
) {
    for (i, t) in TOPICS.iter().enumerate() {
        assert_eq!(label(cx, handle, ("help-toc", i)), t.text(lang).title);
    }
    assert_eq!(
        label(cx, handle, "help-title"),
        topic(current).text(lang).title
    );
    let copy = topics::page_copy(lang);
    assert_eq!(label(cx, handle, "help-prev"), copy.prev);
    assert_eq!(label(cx, handle, "help-next"), copy.next);
    let placeholder = help.read_with(cx, |h, cx| {
        h.search_input()
            .read(cx)
            .presentation()
            .placeholder()
            .to_string()
    });
    assert_eq!(placeholder, copy.search);
    // The switch's own words never change (H3).
    assert_eq!(label(cx, handle, ("help-lang", 0usize)), "ไทย");
    assert_eq!(label(cx, handle, ("help-lang", 1usize)), "English");
}

#[gpui_kit::test]
fn f1_opens_the_help_tab_once_also_from_a_start_box(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx, Launch::Start);
    let start = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let left = start.read(cx).left_input().focus_handle(cx);
        assert!(left.is_focused(window), "the caret is in the left box");
    })
    .unwrap();
    press(cx, handle, "f1");
    assert_eq!(kinds(&workspace, cx), ["start", "help"]);
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(1)
    );
    let title = workspace.read_with(cx, |w, _| {
        w.tabs()
            .iter()
            .map(|(_, key, _)| key.title())
            .collect::<Vec<_>>()
    });
    assert_eq!(title, ["Start", "Help"]);
    // F1 on the Help tab keeps one tab.
    press(cx, handle, "f1");
    assert_eq!(kinds(&workspace, cx), ["start", "help"]);
    // Back on the Start tab with the caret in a box, F1 finds the same Help tab.
    click(cx, handle, ("tab", 0usize));
    cx.update_window(handle, |_, window, cx| {
        let left = start.read(cx).left_input().focus_handle(cx);
        assert!(left.is_focused(window), "the Start tab has the caret again");
    })
    .unwrap();
    press(cx, handle, "f1");
    assert_eq!(kinds(&workspace, cx), ["start", "help"]);
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(1)
    );
    assert_eq!(TabKey::Help.title(), "Help");
}

#[gpui_kit::test]
fn the_help_opens_in_thai_without_a_saved_choice(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (handle, workspace) = open(cx, Launch::Start);
    press(cx, handle, "f1");
    let help = active_help(&workspace, cx);
    assert_eq!(help.read_with(cx, |h, _| h.lang()), HelpLang::Th);
    assert_eq!(help.read_with(cx, |h, _| h.current()), Some("start"));
    assert_page_reads(cx, handle, &help, HelpLang::Th, "start");
    assert!(
        !data.path().join("settings.json").exists(),
        "opening the help writes nothing"
    );
}

#[gpui_kit::test]
fn the_language_switch_flips_every_string_keeps_the_topic_and_is_saved(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (handle, workspace) = open(cx, Launch::Start);
    press(cx, handle, "f1");
    let help = active_help(&workspace, cx);
    click_toc(cx, handle, index_of("rules"));
    assert_eq!(help.read_with(cx, |h, _| h.current()), Some("rules"));
    assert_page_reads(cx, handle, &help, HelpLang::Th, "rules");
    // A query both languages match: the search runs again in the new language.
    cx.update_window(handle, |_, window, cx| {
        help.update(cx, |h, cx| h.set_query("regex", window, cx));
    })
    .unwrap();
    let thai_shown = help.read_with(cx, |h, _| h.shown());
    assert!(thai_shown.contains(&"rules"), "{thai_shown:?}");

    click(cx, handle, ("help-lang", 1usize));
    assert_eq!(help.read_with(cx, |h, _| h.lang()), HelpLang::En);
    assert_eq!(
        help.read_with(cx, |h, _| h.current()),
        Some("rules"),
        "the topic stays"
    );
    assert_eq!(
        help.read_with(cx, |h, cx| h.query(cx)),
        "regex",
        "the query stays"
    );
    let english_shown = help.read_with(cx, |h, _| h.shown());
    assert_eq!(
        english_shown,
        topics::matching(
            "regex",
            &topics::search_texts(HelpLang::En, &topics::sheet_keys())
        )
        .into_iter()
        .map(|i| TOPICS[i].id)
        .collect::<Vec<_>>()
    );
    // An empty query, so every topic-list entry is drawn.
    cx.update_window(handle, |_, window, cx| {
        help.update(cx, |h, cx| h.set_query("", window, cx));
    })
    .unwrap();
    assert_page_reads(cx, handle, &help, HelpLang::En, "rules");
    assert_eq!(
        duet_app::settings::get_string(&data.path().join("settings.json"), "ui.lang").as_deref(),
        Some("en")
    );
    // The empty message follows the language too.
    cx.update_window(handle, |_, window, cx| {
        help.update(cx, |h, cx| h.set_query("no such words xyz", window, cx));
    })
    .unwrap();
    assert_eq!(label(cx, handle, "help-empty"), "No topic matches.");
    click(cx, handle, ("help-lang", 0usize));
    assert_eq!(label(cx, handle, "help-empty"), "ไม่พบหัวข้อที่ตรงกัน");
    assert_eq!(
        duet_app::settings::get_string(&data.path().join("settings.json"), "ui.lang").as_deref(),
        Some("th")
    );
    click(cx, handle, ("help-lang", 1usize));

    // A new Help tab reads the choice back.
    press(cx, handle, "ctrl-w");
    assert_eq!(kinds(&workspace, cx), ["start"]);
    press(cx, handle, "f1");
    let again = active_help(&workspace, cx);
    assert_eq!(again.read_with(cx, |h, _| h.lang()), HelpLang::En);
    assert_page_reads(cx, handle, &again, HelpLang::En, "start");
}

#[gpui_kit::test]
fn search_filters_the_topics_in_both_languages(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let (handle, workspace) = open(cx, Launch::Start);
    press(cx, handle, "f1");
    let help = active_help(&workspace, cx);
    let set = |cx: &mut TestAppContext, query: &str| {
        let query = query.to_owned();
        cx.update_window(handle, |_, window, cx| {
            help.update(cx, |h, cx| h.set_query(&query, window, cx));
            window.render_frame(cx);
        })
        .unwrap();
        help.read_with(cx, |h, _| h.shown())
    };
    assert_eq!(
        set(cx, "").len(),
        TOPICS.len(),
        "an empty query shows every topic"
    );
    // Thai: a Thai word, and an English word the Thai keywords carry.
    let shown = set(cx, "ซิงก์");
    assert!(shown.contains(&"ops"), "{shown:?}");
    assert!(shown.len() < TOPICS.len());
    let shown = set(cx, "  SYNC ");
    assert!(
        shown.contains(&"ops"),
        "an English word in Thai mode: {shown:?}"
    );
    assert!(shown.len() < TOPICS.len());
    // The current topic stops matching: the first match is shown.
    set(cx, "");
    click_toc(cx, handle, index_of("faq"));
    let shown = set(cx, "mirror");
    assert!(
        shown.contains(&"ops") && !shown.contains(&"faq"),
        "{shown:?}"
    );
    assert_eq!(help.read_with(cx, |h, _| h.current()), Some(shown[0]));
    // Topics that do not match leave the list.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for (i, t) in TOPICS.iter().enumerate() {
            assert_eq!(
                window.try_find(("help-toc", i)).is_some(),
                shown.contains(&t.id),
                "{}",
                t.id
            );
        }
    })
    .unwrap();
    let shown = set(cx, "no such words xyz");
    assert!(shown.is_empty());
    assert_eq!(help.read_with(cx, |h, _| h.current()), None);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("help-empty").is_some());
        assert!(window.try_find("help-topic").is_none());
    })
    .unwrap();
    // English.
    click(cx, handle, ("help-lang", 1usize));
    let shown = set(cx, "Zero-Width");
    assert!(shown.contains(&"odd"), "{shown:?}");
    let shown = set(cx, "mirror");
    assert!(shown.contains(&"ops"), "{shown:?}");
    assert!(!shown.contains(&"hex"), "{shown:?}");
    let shown = set(cx, "ซิงก์");
    assert!(
        shown.is_empty(),
        "Thai words are not in the English text: {shown:?}"
    );
}

#[gpui_kit::test]
fn the_topic_list_shows_its_groups_and_a_search_hides_an_empty_one(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let (handle, workspace) = open(cx, Launch::Start);
    press(cx, handle, "f1");
    let help = active_help(&workspace, cx);
    let groups_shown = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            (0..GROUPS.len())
                .map(|g| window.try_find(("help-group", g)).is_some())
                .collect::<Vec<_>>()
        })
        .unwrap()
    };
    assert_eq!(groups_shown(cx), [true; 4]);
    for (g, group) in GROUPS.iter().enumerate() {
        assert_eq!(label(cx, handle, ("help-group", g)), group.th);
    }
    // Each group's label sits above its first topic.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for (g, group) in GROUPS.iter().enumerate() {
            let first = index_of(group.topics[0]);
            let label = window.find(("help-group", g)).bounds();
            let entry = window.find(("help-toc", first)).bounds();
            assert!(
                label.bottom() <= entry.top(),
                "{} above its topics",
                group.en
            );
        }
    })
    .unwrap();
    // "mirror" finds the sync topic only: the other groups leave the list.
    cx.update_window(handle, |_, window, cx| {
        help.update(cx, |h, cx| h.set_query("mirror", window, cx));
    })
    .unwrap();
    let shown = help.read_with(cx, |h, _| h.shown());
    assert_eq!(shown, ["ops"]);
    assert_eq!(groups_shown(cx), [false, true, false, false]);
    // The labels follow the language.
    cx.update_window(handle, |_, window, cx| {
        help.update(cx, |h, cx| h.set_query("", window, cx));
    })
    .unwrap();
    click(cx, handle, ("help-lang", 1usize));
    for (g, group) in GROUPS.iter().enumerate() {
        assert_eq!(label(cx, handle, ("help-group", g)), group.en);
    }
}

#[gpui_kit::test]
fn escape_clears_the_search_first(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx, Launch::Start);
    press(cx, handle, "f1");
    let help = active_help(&workspace, cx);
    let search_focused = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            let input = help.read(cx).search_input().focus_handle(cx);
            input.is_focused(window)
        })
        .unwrap()
    };
    assert!(!search_focused(cx), "the page has the keys, not the box");
    press(cx, handle, "ctrl-f");
    assert!(
        search_focused(cx),
        "Ctrl+F puts the caret in the search box"
    );
    cx.update_window(handle, |_, window, cx| window.input("sync", cx))
        .unwrap();
    assert_eq!(help.read_with(cx, |h, cx| h.query(cx)), "sync");
    assert!(help.read_with(cx, |h, _| h.shown().len()) < TOPICS.len());
    press(cx, handle, "escape");
    assert_eq!(help.read_with(cx, |h, cx| h.query(cx)), "");
    assert_eq!(help.read_with(cx, |h, _| h.shown().len()), TOPICS.len());
    assert_eq!(kinds(&workspace, cx), ["start", "help"], "the tab stays");
    // Escape on an empty search changes nothing.
    press(cx, handle, "escape");
    assert_eq!(help.read_with(cx, |h, _| h.shown().len()), TOPICS.len());
    // Ctrl+F from the page (a topic clicked) goes back to the box and selects its text.
    cx.update_window(handle, |_, window, cx| window.input("eol", cx))
        .unwrap();
    click_toc(cx, handle, index_of("eol"));
    cx.update_window(handle, |_, window, cx| {
        window.focus(&help.read(cx).focus_handle(cx), cx);
    })
    .unwrap();
    assert!(!search_focused(cx));
    press(cx, handle, "escape");
    assert_eq!(
        help.read_with(cx, |h, cx| h.query(cx)),
        "",
        "Escape clears a non-empty search from the page too"
    );
    press(cx, handle, "ctrl-f");
    assert!(search_focused(cx));
}

#[gpui_kit::test]
fn a_toc_click_and_previous_next_move_the_topic(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx, Launch::Start);
    press(cx, handle, "f1");
    let help = active_help(&workspace, cx);
    let current = |cx: &mut TestAppContext| help.read_with(cx, |h, _| h.current());
    assert_eq!(current(cx), Some("start"));
    click_nav(cx, handle, "help-prev");
    assert_eq!(current(cx), Some("start"), "nothing before the first topic");
    click_toc(cx, handle, index_of("merge"));
    assert_eq!(current(cx), Some("merge"));
    click_nav(cx, handle, "help-next");
    assert_eq!(current(cx), Some("hex"));
    click_nav(cx, handle, "help-prev");
    click_nav(cx, handle, "help-prev");
    assert_eq!(current(cx), Some("rules"));
    click_toc(cx, handle, TOPICS.len() - 1);
    assert_eq!(current(cx), Some("faq"));
    click_nav(cx, handle, "help-next");
    assert_eq!(current(cx), Some("faq"), "nothing after the last topic");
    // With a search, Previous and Next walk the topics the list shows.
    cx.update_window(handle, |_, window, cx| {
        help.update(cx, |h, cx| h.set_query("F8", window, cx));
    })
    .unwrap();
    let shown = help.read_with(cx, |h, _| h.shown());
    assert!(shown.len() >= 2, "{shown:?}");
    click_toc(cx, handle, index_of(shown[0]));
    click_nav(cx, handle, "help-next");
    assert_eq!(current(cx), Some(shown[1]));
    assert!(
        shown.contains(&"keys"),
        "the sheet's keys are searched: {shown:?}"
    );
    assert!(help.update(cx, |h, cx| h.go_to("keys", cx)));
    assert_eq!(current(cx), Some("keys"));
}
