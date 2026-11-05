//! The Start tab in Thai.

use duet_app::i18n::{self, tr};
use duet_app::launch::Launch;
use duet_app::start::StartView;
use duet_app::workspace::{TabItem, Workspace};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext, Entity, TestAppContext};

use super::{open, thai_ui};

fn start_of(workspace: &Entity<Workspace>, cx: &mut TestAppContext) -> Entity<StartView> {
    workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    })
}

fn placeholder(start: &Entity<StartView>, left: bool, cx: &mut TestAppContext) -> String {
    start.read_with(cx, |v, cx| {
        let input = if left {
            v.left_input()
        } else {
            v.right_input()
        };
        input.read(cx).presentation().placeholder().to_string()
    })
}

#[gpui_kit::test]
fn the_start_tab_reads_in_thai(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let (handle, workspace) = open(cx, Launch::Start);
    let start = start_of(&workspace, cx);
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let label = |cx: &mut TestAppContext, id: &'static str| {
        cx.update_window(handle, |_, window, _| {
            window.find(id).label().map(str::to_owned)
        })
        .unwrap()
    };
    for (id, key) in [
        ("start-mode-files", "Compare files"),
        ("start-mode-folders", "Compare folders"),
        ("start-mode-merge", "3-way merge"),
        ("start-mode-hex", "Compare binary files"),
        ("start-mode-table", "Compare tables"),
        ("start-mode-picture", "Compare pictures"),
        ("start-mode-text", "Paste text…"),
        ("start-compare", "Compare"),
        ("start-recent-empty", "Compared pairs show up here."),
        ("start-shortcuts-help", "All shortcuts in Help (F1)"),
    ] {
        let thai = label(cx, id);
        assert_eq!(thai.as_deref(), Some(tr(key)), "{id}");
        assert_ne!(tr(key), key, "{key} has Thai");
    }
    assert_eq!(placeholder(&start, true, cx), tr("Left file or folder"));
    assert_eq!(placeholder(&start, false, cx), tr("Right file or folder"));
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

#[gpui_kit::test]
fn a_switch_relabels_the_placeholders_and_clears_the_error(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx, Launch::Start);
    let start = start_of(&workspace, cx);
    start.update(cx, |v, cx| v.compare(cx));
    assert!(start.read_with(cx, |v, _| v.error().is_some()));
    assert_eq!(placeholder(&start, true, cx), "Left file or folder");
    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 0usize), cx)
    })
    .unwrap();
    assert_eq!(
        placeholder(&start, true, cx),
        tr_in_thai("Left file or folder")
    );
    assert_eq!(
        placeholder(&start, false, cx),
        tr_in_thai("Right file or folder")
    );
    assert_eq!(
        start.read_with(cx, |v, _| v.error().map(str::to_owned)),
        None
    );
}

fn tr_in_thai(key: &'static str) -> String {
    let _th = thai_ui();
    tr(key).to_owned()
}

#[gpui_kit::test]
fn an_empty_compare_reads_its_error_in_thai(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let (_handle, workspace) = open(cx, Launch::Start);
    let start = start_of(&workspace, cx);
    start.update(cx, |v, cx| v.compare(cx));
    assert_eq!(
        start.read_with(cx, |v, _| v.error().map(str::to_owned)),
        Some(tr("Choose two files or two folders.").to_owned())
    );
}
