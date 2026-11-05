//! Headless UI tests for tabs, launch handling and the start screen.

use std::path::PathBuf;
use std::time::Duration;

use duet_app::folder::view::{FolderEvent, FolderView};
use duet_app::hex::view::HexEvent;
use duet_app::launch::{Launch, MergePaths};
use duet_app::merge::view::{InputMode, LEFT, MergeView, RIGHT, Stage};
use duet_app::palette::Palette;
use duet_app::tabs::{Mode, TabKey};
use duet_app::text::view::TextEvent;
use duet_app::workspace::{TabItem, Workspace};
use gpui_kit::component::ThemeMode;
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, Focusable, Point, Render, TestAppContext,
    Window, WindowBounds, WindowOptions, px, size,
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

fn pair_of_dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let (l, r) = (dir.path().join("l"), dir.path().join("r"));
    std::fs::create_dir_all(&l).unwrap();
    std::fs::create_dir_all(&r).unwrap();
    std::fs::write(l.join("a.txt"), "1\n").unwrap();
    std::fs::write(r.join("a.txt"), "2\n").unwrap();
    (dir, l, r)
}

#[gpui_kit::test]
fn no_arguments_open_the_start_screen(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx, Launch::Start);
    assert_eq!(kinds(&workspace, cx), ["start"]);
    assert!(
        workspace.read_with(cx, |w, cx| w.active_idle(cx)),
        "Start is ready at once"
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("start-error").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
async fn the_workspace_is_ready_once_the_compare_lands(cx: &mut TestAppContext) {
    let (_dir, l, r) = pair_of_dirs();
    let (_handle, workspace) = open(
        cx,
        Launch::Files {
            left: l.join("a.txt"),
            right: r.join("a.txt"),
        },
    );
    assert!(
        !workspace.read_with(cx, |w, cx| w.active_idle(cx)),
        "the compare is still running"
    );
    wait_text_loaded(cx, &workspace).await;
    assert!(workspace.read_with(cx, |w, cx| w.active_idle(cx)));
}

#[gpui_kit::test]
fn an_invalid_launch_explains_itself_on_the_start_screen(cx: &mut TestAppContext) {
    let (handle, workspace) = open(
        cx,
        Launch::Invalid("Cannot compare a file with a folder.".into()),
    );
    let error = workspace.read_with(cx, |w, cx| match w.tabs().active() {
        Some(TabItem::Start(start)) => start.read(cx).error().map(str::to_owned),
        _ => None,
    });
    assert_eq!(
        error.as_deref(),
        Some("Cannot compare a file with a folder.")
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("start-error").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
async fn a_folder_pair_opens_its_files_in_a_text_tab_once(cx: &mut TestAppContext) {
    let (_dir, l, r) = pair_of_dirs();
    let (_handle, workspace) = open(
        cx,
        Launch::Folders {
            left: l.clone(),
            right: r.clone(),
        },
    );
    assert_eq!(kinds(&workspace, cx), ["folder"]);
    let folder = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Folder(view)) => view.clone(),
        _ => panic!("expected the folder tab"),
    });
    let event = || FolderEvent::OpenFiles {
        left: l.join("a.txt"),
        right: r.join("a.txt"),
        kind: None,
    };
    folder.update(cx, |_, cx| cx.emit(event()));
    assert_eq!(kinds(&workspace, cx), ["folder", "text"]);
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(1)
    );
    workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
    folder.update(cx, |_, cx| cx.emit(event()));
    assert_eq!(
        kinds(&workspace, cx),
        ["folder", "text"],
        "the open pair is reused"
    );
}

#[gpui_kit::test]
async fn ctrl_w_closes_tabs_and_the_last_close_leaves_a_start_tab(cx: &mut TestAppContext) {
    let (_dir, l, r) = pair_of_dirs();
    let (handle, workspace) = open(cx, Launch::Folders { left: l, right: r });
    cx.update_window(handle, |_, window, cx| window.press("ctrl-t", cx))
        .unwrap();
    assert_eq!(kinds(&workspace, cx), ["folder", "start"]);
    cx.update_window(handle, |_, window, cx| window.press("ctrl-w", cx))
        .unwrap();
    assert_eq!(kinds(&workspace, cx), ["folder"]);
    cx.update_window(handle, |_, window, cx| window.press("ctrl-w", cx))
        .unwrap();
    assert_eq!(kinds(&workspace, cx), ["start"]);
}

#[gpui_kit::test]
async fn the_start_screen_compares_two_files(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "x\n").unwrap();
    std::fs::write(&b, "y\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Start);
    let start = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    });
    let (a_text, b_text) = (a.display().to_string(), b.display().to_string());
    cx.update_window(handle, |_, window, cx| {
        start.update(cx, |view, cx| {
            view.set_paths(&a_text, &b_text, window, cx);
            view.compare(cx);
        })
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start", "text"]);
    let text = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected the text tab"),
    });
    let t = text.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !t.read(cx).is_busy()
    })
    .await;
    assert_eq!(
        text.read_with(cx, |v, _| v.model().unwrap().block_count()),
        1
    );
}

#[gpui_kit::test]
async fn escape_in_a_text_tab_returns_to_its_folder_tab(cx: &mut TestAppContext) {
    let (_dir, l, r) = pair_of_dirs();
    let (handle, workspace) = open(
        cx,
        Launch::Folders {
            left: l.clone(),
            right: r.clone(),
        },
    );
    let folder = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Folder(view)) => view.clone(),
        _ => panic!("expected the folder tab"),
    });
    folder.update(cx, |_, cx| {
        cx.emit(FolderEvent::OpenFiles {
            left: l.join("a.txt"),
            right: r.join("a.txt"),
            kind: None,
        })
    });
    let text = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected the text tab"),
    });
    let t = text.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !t.read(cx).is_busy()
    })
    .await;
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(0)
    );
}

#[gpui_kit::test]
async fn swapping_a_text_tab_renames_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "x\n").unwrap();
    std::fs::write(&b, "y\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Files { left: a, right: b });
    let text = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected the text tab"),
    });
    let t = text.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !t.read(cx).is_busy()
    })
    .await;
    cx.update_window(handle, |_, window, cx| window.press("f9", cx))
        .unwrap();
    let title = workspace.read_with(cx, |w, _| w.tabs().iter().next().unwrap().1.title());
    assert_eq!(title, "b.txt ↔ a.txt");
}

#[gpui_kit::test]
async fn swapping_onto_an_open_pair_with_unsaved_edits_is_refused_and_closes_nothing(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "x\n").unwrap();
    std::fs::write(&b, "y\n").unwrap();
    let (handle, workspace) = open(
        cx,
        Launch::Files {
            left: a.clone(),
            right: b.clone(),
        },
    );
    wait_text_loaded(cx, &workspace).await;
    type_into_active_text(cx, handle, &workspace, "z");
    let edited = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected the text tab"),
    });
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_files(b.clone(), a.clone(), None, window, cx)
        });
    })
    .unwrap();
    assert_eq!(kinds(&workspace, cx), ["text", "text"]);
    // Twice: a refused swap must not leave a stale key that finds the other tab.
    for _ in 0..2 {
        edited.update(cx, |v, cx| v.swap_sides(cx));
        assert_eq!(kinds(&workspace, cx), ["text", "text"], "no tab closed");
        let (left, right) = edited.read_with(cx, |v, _| {
            (v.request().left.clone(), v.request().right.clone())
        });
        assert_eq!((left, right), (a.clone(), b.clone()), "the swap was undone");
        assert_eq!(
            edited
                .read_with(cx, |v, _| v.notice().map(str::to_owned))
                .as_deref(),
            Some("That pair is already open in another tab.")
        );
        let keys: Vec<TabKey> = workspace.read_with(cx, |w, _| {
            w.tabs().iter().map(|(_, k, _)| k.clone()).collect()
        });
        assert_eq!(
            keys,
            [
                TabKey::Files(a.clone(), b.clone()),
                TabKey::Files(b.clone(), a.clone())
            ],
            "each tab's key matches what it shows"
        );
    }
    assert!(workspace.read_with(cx, |w, cx| w.tab_dirty(0, cx)));
}

#[gpui_kit::test]
async fn tabs_show_badges_and_middle_click_closes_them(cx: &mut TestAppContext) {
    let (_dir, l, r) = pair_of_dirs();
    // A different size, so the quick compare sees the difference.
    std::fs::write(r.join("a.txt"), "22\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Folders { left: l, right: r });
    let folder = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Folder(view)) => view.clone(),
        _ => panic!("expected the folder tab"),
    });
    let f = folder.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !f.read(cx).is_busy()
    })
    .await;
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        Some("1".into())
    );
    cx.update_window(handle, |_, window, cx| window.press("ctrl-t", cx))
        .unwrap();
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(1, cx)),
        None,
        "a start tab has no badge"
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let position = window.find(("tab", 1usize)).bounds().center();
        window.dispatch_event(
            gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                button: gpui_kit::MouseButton::Middle,
                position,
                modifiers: gpui_kit::Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
    })
    .unwrap();
    assert_eq!(kinds(&workspace, cx), ["folder"]);
}

#[gpui_kit::test]
async fn an_identical_pair_gets_an_ok_badge(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "same\n").unwrap();
    std::fs::write(&b, "same\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Files { left: a, right: b });
    let text = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected the text tab"),
    });
    let t = text.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !t.read(cx).is_busy()
    })
    .await;
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        Some("ok".into())
    );
}

fn with_data_dir(cx: &mut TestAppContext, dir: &std::path::Path) {
    let data_dir = dir.to_path_buf();
    cx.update(|cx| cx.set_global(duet_app::app::AppPaths { data_dir }));
}

#[gpui_kit::test]
async fn compares_of_temp_files_stay_out_of_the_recent_list(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (dir, l, r) = pair_of_dirs();
    // The pair lives under the "temp folder", as git's files and `no file` do.
    let root = dir.path().to_path_buf();
    cx.update(|cx| cx.set_global(duet_app::app::TempRoot(root)));
    let (handle, workspace) = open(cx, Launch::Folders { left: l, right: r });
    cx.update_window(handle, |_, window, cx| window.press("ctrl-t", cx))
        .unwrap();
    let start = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    });
    cx.run_until_parked();
    assert!(start.read_with(cx, |s, _| s.recent().is_empty()));
    assert!(!data.path().join("recent.txt").exists());
}

#[gpui_kit::test]
async fn compares_land_in_the_recent_list_and_reopen_from_it(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (_dir, l, r) = pair_of_dirs();
    let (handle, workspace) = open(
        cx,
        Launch::Folders {
            left: l.clone(),
            right: r.clone(),
        },
    );
    cx.update_window(handle, |_, window, cx| window.press("ctrl-t", cx))
        .unwrap();
    let start = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    });
    cx.run_until_parked();
    let recent = start.read_with(cx, |s, _| s.recent().to_vec());
    assert_eq!(recent.len(), 1);
    assert_eq!((recent[0].0.left.clone(), recent[0].1), (l, Some(true)));
    workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(1));
    start.update(cx, |s, cx| s.open_recent(0, cx));
    cx.run_until_parked();
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(0),
        "the open pair's tab"
    );
}

#[gpui_kit::test]
async fn dropping_files_fills_the_boxes_and_two_files_compare(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "x\n").unwrap();
    std::fs::write(&b, "y\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Start);
    let drop_on = |cx: &mut TestAppContext, target: &'static str, paths: Vec<PathBuf>| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let position = window.find(target).bounds().center();
            let mut external = gpui_kit::ExternalPaths::default();
            external.0.extend(paths);
            window.dispatch_event(
                gpui_kit::PlatformInput::FileDrop(gpui_kit::FileDropEvent::Entered {
                    position,
                    paths: external,
                }),
                cx,
            );
            window.dispatch_event(
                gpui_kit::PlatformInput::FileDrop(gpui_kit::FileDropEvent::Submit { position }),
                cx,
            );
        })
        .unwrap();
    };
    drop_on(cx, "start-right-row", vec![b.clone()]);
    let start = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    });
    let right = start.read_with(cx, |s, cx| s.right_input().read(cx).value().to_string());
    assert_eq!(right, b.display().to_string());
    drop_on(cx, "start-view", vec![a, b]);
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start", "text"]);
}

#[gpui_kit::test]
fn the_start_screen_focuses_the_left_box(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx, Launch::Start);
    let start = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    });
    cx.update_window(handle, |_, window, cx| {
        let focused = start
            .read(cx)
            .left_input()
            .focus_handle(cx)
            .is_focused(window);
        assert!(focused);
    })
    .unwrap();
}

#[gpui_kit::test]
async fn files_opened_from_a_folder_compare_stay_out_of_the_recent_list(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (_dir, l, r) = pair_of_dirs();
    let (handle, workspace) = open(
        cx,
        Launch::Folders {
            left: l.clone(),
            right: r.clone(),
        },
    );
    let folder = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Folder(view)) => view.clone(),
        _ => panic!("expected the folder tab"),
    });
    folder.update(cx, |_, cx| {
        cx.emit(FolderEvent::OpenFiles {
            left: l.join("a.txt"),
            right: r.join("a.txt"),
            kind: None,
        })
    });
    cx.update_window(handle, |_, window, cx| window.press("ctrl-t", cx))
        .unwrap();
    let start = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    });
    let recent = start.read_with(cx, |s, _| s.recent().to_vec());
    assert_eq!(recent.len(), 1, "only the folder pair the user opened");
    assert_eq!(recent[0].0.kind, duet_app::recent::RecentKind::Folders);
}

#[gpui_kit::test]
async fn recent_paths_are_checked_off_the_ui_thread(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let gone = data.path().join("gone");
    let line = format!(
        "F\t{}\t{}\n",
        gone.join("left").display(),
        gone.join("right").display()
    );
    std::fs::write(data.path().join("recent.txt"), line).unwrap();
    with_data_dir(cx, data.path());
    let (_handle, workspace) = open(cx, Launch::Start);
    let start = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    });
    assert_eq!(
        start.read_with(cx, |s, _| s.recent()[0].1),
        None,
        "the screen opens before any path is checked"
    );
    cx.run_until_parked();
    assert_eq!(start.read_with(cx, |s, _| s.recent()[0].1), Some(false));
}

async fn wait_text_loaded(cx: &mut TestAppContext, workspace: &Entity<Workspace>) {
    for _ in 0..2000 {
        let busy = workspace.read_with(cx, |w, cx| match w.tabs().active() {
            Some(TabItem::Text(view)) => view.read(cx).is_busy(),
            _ => true,
        });
        if !busy {
            return;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    panic!("the text tab never loaded");
}

fn type_into_active_text(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    workspace: &Entity<Workspace>,
    text: &str,
) {
    let view = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected a text tab"),
    });
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    cx.update_window(handle, |_, window, cx| window.input(text, cx))
        .unwrap();
}

#[gpui_kit::test]
async fn closing_a_tab_with_unsaved_edits_asks_and_cancel_keeps_it(cx: &mut TestAppContext) {
    let (dir, l, r) = pair_of_dirs();
    let (handle, workspace) = open(
        cx,
        Launch::Files {
            left: l.join("a.txt"),
            right: r.join("a.txt"),
        },
    );
    wait_text_loaded(cx, &workspace).await;
    type_into_active_text(cx, handle, &workspace, "z");
    assert!(workspace.read_with(cx, |w, cx| w.tab_dirty(0, cx)));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find(("tab-dirty", 0usize)).is_some(),
            "the dot shows the edits"
        );
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| window.press("ctrl-w", cx))
        .unwrap();
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["text"]);
    cx.update_window(handle, |_, window, cx| window.press("ctrl-w", cx))
        .unwrap();
    cx.simulate_prompt_answer("Discard");
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start"]);
    let original = std::fs::read_to_string(l.join("a.txt")).unwrap();
    assert!(!original.starts_with('z'), "Discard leaves the file alone");
    drop(dir);
}

#[gpui_kit::test]
async fn save_on_close_writes_nothing_when_one_file_has_different_edits_on_each_side(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.txt");
    std::fs::write(&file, "1\n").unwrap();
    let (handle, workspace) = open(
        cx,
        Launch::Files {
            left: file.clone(),
            right: file.clone(),
        },
    );
    wait_text_loaded(cx, &workspace).await;
    let view = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected a text tab"),
    });
    for (left, typed) in [(true, "x"), (false, "y")] {
        view.update(cx, |v, cx| v.set_caret_for_test(left, 0, cx));
        cx.update_window(handle, |_, window, cx| window.input(typed, cx))
            .unwrap();
    }
    cx.update_window(handle, |_, window, cx| window.press("ctrl-w", cx))
        .unwrap();
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Save");
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["text"], "the tab stays open");
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("Both sides are the same file with different edits. Save one side.")
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "1\n",
        "nothing is written"
    );
    assert!(view.read_with(cx, |v, _| {
        let model = v.model().unwrap();
        model.is_dirty(true) && model.is_dirty(false)
    }));
    // Ctrl+S still saves the caret's side.
    cx.update_window(handle, |_, window, cx| window.press("ctrl-s", cx))
        .unwrap();
    cx.run_until_parked();
    for _ in 0..200 {
        if std::fs::read_to_string(&file).unwrap() == "y1\n" {
            break;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "y1\n");
}

#[gpui_kit::test]
async fn closing_the_window_with_unsaved_edits_asks_and_save_writes(cx: &mut TestAppContext) {
    let (_dir, l, r) = pair_of_dirs();
    let (handle, workspace) = open(
        cx,
        Launch::Files {
            left: l.join("a.txt"),
            right: r.join("a.txt"),
        },
    );
    wait_text_loaded(cx, &workspace).await;
    type_into_active_text(cx, handle, &workspace, "z");
    let mut window = gpui_kit::VisualTestContext::from_window(handle, cx);
    assert!(!window.simulate_close(), "the close waits for an answer");
    assert!(window.has_pending_prompt());
    window.simulate_prompt_answer("Save");
    window.run_until_parked();
    let (mut saved, mut open) = (false, true);
    for _ in 0..200 {
        saved = std::fs::read_to_string(l.join("a.txt"))
            .unwrap()
            .starts_with('z');
        open = window.windows().contains(&handle);
        if saved && !open {
            return;
        }
        window.executor().timer(Duration::from_millis(10)).await;
    }
    panic!("after Save: file saved {saved}, window still open {open}");
}

#[gpui_kit::test]
fn the_theme_toggle_flips_the_palette_and_is_remembered(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (_handle, _workspace) = open(cx, Launch::Start);
    cx.update(|cx| duet_app::app::configure_theme(ThemeMode::Light, cx));
    assert_eq!(cx.update(duet_app::theme::toggle), ThemeMode::Dark);
    cx.update(|cx| {
        let dark = Palette::dark();
        assert_eq!(Palette::current(cx), dark);
        let theme = gpui_kit::component::Theme::global(cx);
        assert_eq!(
            theme.primary, dark.accent,
            "gpui-kit's primary buttons use the accent"
        );
        assert_eq!(theme.border, dark.line);
        assert_eq!(theme.popover, dark.panel, "menus sit on the panel colour");
        assert_eq!(theme.radius, px(7.));
        assert_eq!(theme.button, dark.panel2);
        assert_eq!(
            theme.button_hover, dark.panel3,
            "gpui-kit's dropdown buttons answer the pointer"
        );
    });
    let file = data.path().join(duet_app::theme::THEME_FILE);
    assert_eq!(duet_app::theme::load(&file), Some(ThemeMode::Dark));
    assert_eq!(cx.update(duet_app::theme::toggle), ThemeMode::Light);
    assert_eq!(duet_app::theme::load(&file), Some(ThemeMode::Light));
    cx.update(|cx| assert_eq!(Palette::current(cx), Palette::light()));
}

#[gpui_kit::test]
fn windows_light_and_dark_are_followed_until_a_mode_is_chosen(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (_handle, _workspace) = open(cx, Launch::Start);
    cx.update(|cx| duet_app::app::configure_theme(ThemeMode::Light, cx));
    cx.update(|cx| duet_app::theme::system_changed(true, cx));
    cx.update(|cx| assert_eq!(Palette::current(cx), Palette::dark()));
    cx.update(|cx| duet_app::theme::system_changed(false, cx));
    cx.update(|cx| assert_eq!(Palette::current(cx), Palette::light()));
    // The header's toggle remembers a choice, and a remembered choice wins.
    assert_eq!(cx.update(duet_app::theme::toggle), ThemeMode::Dark);
    cx.update(|cx| duet_app::theme::system_changed(false, cx));
    cx.update(|cx| assert_eq!(Palette::current(cx), Palette::dark()));
}

#[gpui_kit::test]
fn an_unwritable_data_folder_still_toggles_the_theme(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let blocked = data.path().join("not-a-folder");
    std::fs::write(&blocked, "x").unwrap();
    with_data_dir(cx, &blocked);
    let (_handle, _workspace) = open(cx, Launch::Start);
    cx.update(|cx| duet_app::app::configure_theme(ThemeMode::Light, cx));
    assert_eq!(
        cx.update(duet_app::theme::toggle),
        ThemeMode::Dark,
        "the write fails, the switch does not"
    );
    cx.update(|cx| assert_eq!(Palette::current(cx), Palette::dark()));
    // Nothing was saved, but the choice still beats Windows for the rest of the session.
    cx.update(|cx| duet_app::theme::system_changed(false, cx));
    cx.update(|cx| assert_eq!(Palette::current(cx), Palette::dark()));
}

#[gpui_kit::test]
fn the_home_path_fields_are_named(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx, Launch::Start);
    let start = active_start(&workspace, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let s = start.read(cx);
        let field = |input: &Entity<gpui_kit::component::input::InputState>| {
            gpui_kit::ElementId::from(("input", input.entity_id()))
        };
        let (left, right) = (field(s.left_input()), field(s.right_input()));
        assert_eq!(window.find(left).label(), Some("Left path"));
        assert_eq!(window.find(right).label(), Some("Right path"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_header_toggles_the_theme_and_remembers_it(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (handle, _workspace) = open(cx, Launch::Start);
    cx.update(|cx| duet_app::app::configure_theme(ThemeMode::Light, cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("header-theme").label(),
            Some("Switch between light and dark")
        );
        window.click("header-theme", cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(Palette::current(cx), Palette::dark()));
    assert_eq!(
        duet_app::theme::load(&data.path().join(duet_app::theme::THEME_FILE)),
        Some(ThemeMode::Dark)
    );
}

#[gpui_kit::test]
fn help_opens_the_help_tab_once(cx: &mut TestAppContext) {
    let (_dir, l, r) = pair_of_dirs();
    let (handle, workspace) = open(cx, Launch::Folders { left: l, right: r });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("header-help").label(), Some("Help (F1)"));
    })
    .unwrap();
    for _ in 0..2 {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("header-help", cx);
        })
        .unwrap();
    }
    assert_eq!(
        kinds(&workspace, cx),
        ["folder", "help"],
        "the second press finds the first Help tab"
    );
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(1)
    );
    workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("header-help", cx);
    })
    .unwrap();
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(1)
    );
}

#[gpui_kit::test]
fn the_offline_pill_hides_on_a_narrow_window(cx: &mut TestAppContext) {
    let (handle, _workspace) = open(cx, Launch::Start);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("header-offline").is_some(),
            "1200 px shows it"
        );
    })
    .unwrap();
    cx.simulate_window_resize(handle, size(px(1000.), px(800.)));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("header-offline").is_none());
        assert!(
            window.try_find("header-theme").is_some(),
            "the buttons stay"
        );
    })
    .unwrap();
    assert!(duet_app::workspace::header_pill_visible(px(1100.)));
    assert!(!duet_app::workspace::header_pill_visible(px(1099.)));
    assert!(duet_app::workspace::header_modes_visible(px(800.)));
    assert!(!duet_app::workspace::header_modes_visible(px(799.)));
}

#[gpui_kit::test]
async fn long_tab_titles_ellipsize_and_keep_their_close_button(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let long = "a_very_long_file_name_that_goes_on_and_on_and_on";
    let (a, b) = (
        dir.path().join(format!("{long}_left.txt")),
        dir.path().join(format!("{long}_right.txt")),
    );
    std::fs::write(&a, "x\n").unwrap();
    std::fs::write(&b, "y\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Files { left: a, right: b });
    wait_text_loaded(cx, &workspace).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let tab = window.find(("tab", 0usize)).bounds();
        assert!(tab.size.width <= px(280.), "tabs stop at 280 px: {tab:?}");
        let close = window.find(("tab-close", 0usize)).bounds();
        assert!(
            close.left() >= tab.left() && close.right() <= tab.right(),
            "x stays inside the tab"
        );
        window.click(("tab-close", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start"]);
}

fn active_start(
    workspace: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> Entity<duet_app::start::StartView> {
    workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    })
}

#[gpui_kit::test]
fn a_mode_card_opens_that_modes_picker_for_the_left_side(cx: &mut TestAppContext) {
    let (_dir, l, _r) = pair_of_dirs();
    let (handle, workspace) = open(cx, Launch::Start);
    let start = active_start(&workspace, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("start-shortcuts").is_some());
        window.click("start-mode-folders", cx);
    })
    .unwrap();
    assert!(cx.did_prompt_for_paths());
    let chosen = l.clone();
    cx.simulate_path_prompt_response(move |options| {
        assert!(
            options.directories && !options.files,
            "Compare folders picks a folder"
        );
        Some(vec![chosen])
    });
    cx.run_until_parked();
    let left = start.read_with(cx, |s, cx| s.left_input().read(cx).value().to_string());
    assert_eq!(left, l.display().to_string());
    cx.update_window(handle, |_, window, cx| {
        assert!(
            start
                .read(cx)
                .left_input()
                .focus_handle(cx)
                .is_focused(window)
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn recent_entries_can_be_removed_and_cleared(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let deep = data
        .path()
        .join("a_rather_long_folder_name_for_the_list".repeat(3));
    let line = format!(
        "T\t{}\t{}\nF\tC:\\x\tC:\\y\n",
        deep.join("left.txt").display(),
        deep.join("right.txt").display()
    );
    std::fs::write(data.path().join("recent.txt"), line).unwrap();
    with_data_dir(cx, data.path());
    let (handle, workspace) = open(cx, Launch::Start);
    let start = active_start(&workspace, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let item = window.find(("start-recent", 0usize)).bounds();
        let remove = window.find(("start-recent-remove", 0usize)).bounds();
        assert!(
            remove.right() <= item.right(),
            "a long path never pushes × out"
        );
        window.click(("start-recent-remove", 0usize), cx);
    })
    .unwrap();
    assert_eq!(kinds(&workspace, cx), ["start"], "removing opens nothing");
    assert_eq!(
        start.read_with(cx, |s, _| s.error().map(str::to_owned)),
        None,
        "the click never reached the card under the ×"
    );
    let kept = duet_app::recent::Recent {
        kind: duet_app::recent::RecentKind::Folders,
        left: "C:\\x".into(),
        right: "C:\\y".into(),
        base: None,
    };
    assert_eq!(
        start.read_with(cx, |s, _| s
            .recent()
            .iter()
            .map(|(r, _)| r.clone())
            .collect::<Vec<_>>()),
        std::slice::from_ref(&kept),
        "the first pair went, the second stays"
    );
    assert_eq!(
        duet_app::recent::load(&data.path().join("recent.txt")),
        std::slice::from_ref(&kept)
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("start-recent-clear", cx);
    })
    .unwrap();
    assert!(start.read_with(cx, |s, _| s.recent().is_empty()));
    assert!(duet_app::recent::load(&data.path().join("recent.txt")).is_empty());
}

/// Presses Tab until `id` has the focus (at most 30 stops).
fn tab_to(window: &mut Window, cx: &mut App, id: impl Into<gpui_kit::ElementId> + Clone) {
    for _ in 0..30 {
        if window.find(id.clone()).focused() == Some(true) {
            return;
        }
        window.press("tab", cx);
    }
    panic!("Tab never reached the element");
}

#[gpui_kit::test]
fn a_mode_card_is_a_tab_stop_and_enter_starts_it(cx: &mut TestAppContext) {
    let (handle, _workspace) = open(cx, Launch::Start);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        tab_to(window, cx, "start-mode-files");
        window.press("enter", cx);
    })
    .unwrap();
    assert!(
        cx.did_prompt_for_paths(),
        "Enter on the card opens the picker"
    );
    cx.simulate_path_prompt_response(|options| {
        assert!(options.files && !options.directories);
        None
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn a_recent_entry_is_a_tab_stop_and_enter_opens_it_while_its_x_is_separate(
    cx: &mut TestAppContext,
) {
    let data = tempfile::tempdir().unwrap();
    let gone = data.path().join("gone");
    let line = format!("T\t{0}\\a.txt\t{0}\\b.txt\n", gone.display());
    std::fs::write(data.path().join("recent.txt"), line).unwrap();
    with_data_dir(cx, data.path());
    let (handle, workspace) = open(cx, Launch::Start);
    let start = active_start(&workspace, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        tab_to(window, cx, ("start-recent", 0usize));
        let remove = window.find(("start-recent-remove", 0usize));
        assert_eq!(remove.focused(), Some(false), "the x is its own stop");
        assert!(
            !remove
                .path()
                .contains(&gpui_kit::ElementId::from(("start-recent", 0usize))),
            "and no button inside the entry's button: {:?}",
            remove.path()
        );
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        start.read_with(cx, |s, _| s.error().is_some()),
        "Enter opened the pair, and a missing path explains itself"
    );
    assert_eq!(start.read_with(cx, |s, _| s.recent().len()), 1);
}

#[gpui_kit::test]
async fn a_folder_tab_follows_its_new_roots(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (_dir, l, r) = pair_of_dirs();
    let right = r.clone();
    let (_handle, workspace) = open(
        cx,
        Launch::Folders {
            left: l.clone(),
            right: r.clone(),
        },
    );
    let folder = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Folder(view)) => view.clone(),
        _ => panic!("expected the folder tab"),
    });
    let parent = l.parent().unwrap().to_path_buf();
    folder.update(cx, |v, cx| v.set_roots(parent.clone(), r.clone(), cx));
    let key = duet_app::tabs::TabKey::Folders(parent, r);
    assert!(
        workspace
            .read_with(cx, |w, _| w.tabs().find(&key))
            .is_some(),
        "the tab is re-keyed"
    );
    assert_eq!(kinds(&workspace, cx), ["folder"]);
    let recent = duet_app::recent::load(&data.path().join("recent.txt"));
    assert_eq!(
        (recent[0].left.clone(), recent[0].right.clone()),
        (l.parent().unwrap().to_path_buf(), right),
        "the new pair is recorded as recent"
    );
}

fn folder_of(
    workspace: &Entity<Workspace>,
    cx: &mut TestAppContext,
    key: &TabKey,
) -> Entity<FolderView> {
    workspace.read_with(cx, |w, _| {
        let index = w.tabs().find(key).expect("the tab exists");
        match w.tabs().iter().nth(index) {
            Some((_, _, TabItem::Folder(view))) => view.clone(),
            _ => panic!("expected a folder tab"),
        }
    })
}

#[gpui_kit::test]
async fn re_rooting_onto_an_open_pair_activates_it_and_closes_the_mover(cx: &mut TestAppContext) {
    let (_dir, l, r) = pair_of_dirs();
    let (handle, workspace) = open(
        cx,
        Launch::Folders {
            left: l.clone(),
            right: r.clone(),
        },
    );
    let parent = l.parent().unwrap().to_path_buf();
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_folders(parent.clone(), r.clone(), window, cx)
        });
    })
    .unwrap();
    assert_eq!(kinds(&workspace, cx), ["folder", "folder"]);
    let first = TabKey::Folders(l.clone(), r.clone());
    let mover = folder_of(&workspace, cx, &first);
    mover.update(cx, |v, cx| v.set_roots(parent.clone(), r.clone(), cx));
    assert_eq!(kinds(&workspace, cx), ["folder"], "the mover closed");
    let key = TabKey::Folders(parent, r);
    assert_eq!(
        workspace.read_with(cx, |w, _| (w.tabs().find(&key), w.tabs().active_index())),
        (Some(0), Some(0)),
        "the existing tab is the only one and is active"
    );
}

#[gpui_kit::test]
async fn swapping_onto_an_open_pair_closes_the_swapped_tab(cx: &mut TestAppContext) {
    let (_dir, l, r) = pair_of_dirs();
    let (handle, workspace) = open(
        cx,
        Launch::Folders {
            left: l.clone(),
            right: r.clone(),
        },
    );
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| w.open_folders(r.clone(), l.clone(), window, cx));
    })
    .unwrap();
    let first = TabKey::Folders(l.clone(), r.clone());
    folder_of(&workspace, cx, &first).update(cx, |v, cx| v.swap_sides(cx));
    assert_eq!(kinds(&workspace, cx), ["folder"]);
    let key = TabKey::Folders(r, l);
    assert_eq!(
        workspace.read_with(cx, |w, _| (w.tabs().find(&key), w.tabs().active_index())),
        (Some(0), Some(0))
    );
}

#[gpui_kit::test]
fn the_active_tab_and_the_new_tab_button_stay_in_view(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx, Launch::Start);
    for _ in 0..16 {
        cx.update_window(handle, |_, window, cx| window.press("ctrl-t", cx))
            .unwrap();
    }
    assert_eq!(workspace.read_with(cx, |w, _| w.tabs().len()), 17);
    let shown = |window: &mut Window, active: usize| {
        let strip = window.find("tab-strip").bounds();
        let new = window.find("tab-new").bounds();
        let tab = window.find(("tab", active)).bounds();
        assert!(
            new.left() >= strip.left() && new.right() <= strip.right() + px(0.5),
            "the + is in the strip: {new:?} of {strip:?}"
        );
        assert!(
            tab.left() >= strip.left() - px(0.5) && tab.right() <= new.left() + px(0.5),
            "tab {active} shows whole, clear of the +: {tab:?}, + at {new:?}"
        );
    };
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        shown(window, 16);
        // Ctrl+Tab from the last tab wraps to the first.
        window.press("ctrl-tab", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        shown(window, 0);
    })
    .unwrap();
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(0)
    );
}

#[gpui_kit::test]
async fn an_open_start_tab_lists_the_pairs_compared_since_it_opened(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (_dir, l, r) = pair_of_dirs();
    let (handle, workspace) = open(cx, Launch::Start);
    let start = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    });
    assert!(start.read_with(cx, |s, _| s.recent().is_empty()));
    cx.update_window(handle, |_, window, cx| {
        start.update(cx, |s, cx| {
            s.set_paths(
                &l.display().to_string(),
                &r.display().to_string(),
                window,
                cx,
            );
            s.compare(cx);
        })
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start", "folder"]);
    cx.run_until_parked();
    let listed = start.read_with(cx, |s, _| s.recent().to_vec());
    assert_eq!(listed.len(), 1, "the compare it started is listed at once");
    assert_eq!(
        (listed[0].0.left.clone(), listed[0].1),
        (l.clone(), Some(true))
    );
    // Another window records a pair: switching back to the Start tab reads the list again.
    let (_other, l2, r2) = pair_of_dirs();
    duet_app::recent::record(
        &data.path().join("recent.txt"),
        duet_app::recent::Recent {
            kind: duet_app::recent::RecentKind::Folders,
            left: l2.clone(),
            right: r2,
            base: None,
        },
    )
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("tab", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let listed = start.read_with(cx, |s, _| s.recent().to_vec());
    assert_eq!(listed.len(), 2, "activating the tab reloads the list");
    assert_eq!((listed[0].0.left.clone(), listed[0].1), (l2, Some(true)));
    assert_eq!(
        listed[1].1,
        Some(true),
        "the pair already checked keeps its mark"
    );
}

#[gpui_kit::test]
fn a_narrower_window_brings_the_active_tab_back_into_view(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx, Launch::Start);
    for _ in 0..16 {
        cx.update_window(handle, |_, window, cx| window.press("ctrl-t", cx))
            .unwrap();
    }
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(16)
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.simulate_window_resize(handle, size(px(700.), px(800.)));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let strip = window.find("tab-strip").bounds();
        let new = window.find("tab-new").bounds();
        let tab = window.find(("tab", 16usize)).bounds();
        assert!(
            new.right() <= strip.right() + px(0.5),
            "{new:?} of {strip:?}"
        );
        assert!(
            tab.left() >= strip.left() - px(0.5) && tab.right() <= new.left() + px(0.5),
            "the active tab shows whole at 700 px: {tab:?}, + at {new:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_recent_entry_has_open_and_remove_on_right_click(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let line = "F\tC:\\x\tC:\\y\nF\tC:\\p\tC:\\q\n";
    std::fs::write(data.path().join("recent.txt"), line).unwrap();
    with_data_dir(cx, data.path());
    let (handle, workspace) = open(cx, Launch::Start);
    let start = active_start(&workspace, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.right_click(("start-recent", 1usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        start.read_with(cx, |s, _| s.menu_target_for_test()),
        Some(1)
    );
    let items = start.read_with(cx, |s, _| duet_app::ui::menu::outline(&s.recent_menu()));
    assert_eq!(items, ["Open", "Remove from list"]);
    // "Remove from list" runs through the same path the popup uses.
    cx.update_window(handle, |_, window, cx| {
        let entries = start.read(cx).recent_menu();
        assert!(duet_app::ui::menu::activate(
            entries,
            "Remove from list",
            None,
            window,
            cx
        ));
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(start.read_with(cx, |s, _| s.recent().len()), 1);
    // The entry that was right-clicked went; the other one stayed.
    assert_eq!(
        start.read_with(cx, |s, _| s.recent()[0].0.left.clone()),
        PathBuf::from("C:\\x")
    );
}

#[gpui_kit::test]
fn a_right_click_on_blank_list_space_leaves_no_menu_target(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let line = "F\tC:\\x\tC:\\y\nF\tC:\\p\tC:\\q\n";
    std::fs::write(data.path().join("recent.txt"), line).unwrap();
    with_data_dir(cx, data.path());
    let (handle, workspace) = open(cx, Launch::Start);
    let start = active_start(&workspace, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.right_click(("start-recent", 1usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        start.read_with(cx, |s, _| s.menu_target_for_test()),
        Some(1)
    );
    // The gap between card 0 and card 1 is inside the list but on no card.
    cx.update_window(handle, |_, window, cx| {
        let card = window.find(("start-recent", 0usize)).bounds();
        let position = Point {
            x: card.center().x,
            y: card.bottom() + px(3.),
        };
        for press in [true, false] {
            let event = if press {
                gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                    button: gpui_kit::MouseButton::Right,
                    position,
                    modifiers: gpui_kit::Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                })
            } else {
                gpui_kit::PlatformInput::MouseUp(gpui_kit::MouseUpEvent {
                    button: gpui_kit::MouseButton::Right,
                    position,
                    modifiers: gpui_kit::Modifiers::default(),
                    click_count: 1,
                })
            };
            window.dispatch_event(event, cx);
            window.render_frame(cx);
        }
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(start.read_with(cx, |s, _| s.menu_target_for_test()), None);
    assert!(start.read_with(cx, |s, _| s.recent_menu().is_empty()));
}

fn active_text(
    workspace: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> Entity<duet_app::text::view::TextView> {
    workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected a text tab"),
    })
}

fn active_key(workspace: &Entity<Workspace>, cx: &mut TestAppContext) -> TabKey {
    workspace.read_with(cx, |w, _| {
        let index = w.tabs().active_index().expect("an active tab");
        w.tabs()
            .iter()
            .nth(index)
            .map(|(_, key, _)| key.clone())
            .unwrap()
    })
}

#[gpui_kit::test]
async fn opening_another_file_into_a_side_rekeys_the_tab_or_goes_to_the_open_one(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b, c) = (
        dir.path().join("a.txt"),
        dir.path().join("b.txt"),
        dir.path().join("c.txt"),
    );
    for (path, text) in [(&a, "a\n"), (&b, "b\n"), (&c, "c\n")] {
        std::fs::write(path, text).unwrap();
    }
    let (handle, workspace) = open(
        cx,
        Launch::Files {
            left: a.clone(),
            right: b.clone(),
        },
    );
    wait_text_loaded(cx, &workspace).await;
    let view = active_text(&workspace, cx);
    view.update(cx, |_, cx| {
        cx.emit(TextEvent::Open {
            left: a.clone(),
            right: c.clone(),
        })
    });
    cx.run_until_parked();
    assert_eq!(
        active_key(&workspace, cx),
        TabKey::Files(a.clone(), c.clone())
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.request().right.clone()),
        c,
        "the view reloads with the new path"
    );
    // A second tab with a/b: asking tab 2 for a/c goes to tab 1 and changes nothing.
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_files(a.clone(), b.clone(), None, window, cx)
        })
    })
    .unwrap();
    let view2 = active_text(&workspace, cx);
    view2.update(cx, |_, cx| {
        cx.emit(TextEvent::Open {
            left: a.clone(),
            right: c.clone(),
        })
    });
    cx.run_until_parked();
    assert_eq!(
        active_key(&workspace, cx),
        TabKey::Files(a.clone(), c.clone())
    );
    assert_eq!(workspace.read_with(cx, |w, _| w.tabs().len()), 2);
    assert_eq!(
        view2.read_with(cx, |v, _| v.request().right.clone()),
        b,
        "the asking tab is left as it was"
    );
}

#[gpui_kit::test]
async fn save_as_renames_the_text_tab(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "x\n").unwrap();
    std::fs::write(&b, "y\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Files { left: a, right: b });
    let text = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected the text tab"),
    });
    let t = text.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !t.read(cx).is_busy()
    })
    .await;
    let target = dir.path().join("c.txt");
    let done = cx
        .update_window(handle, |_, window, cx| {
            text.update(cx, |v, cx| v.save_side_as(true, target, window, cx))
        })
        .unwrap();
    assert!(done.await);
    let title = workspace.read_with(cx, |w, _| w.tabs().iter().next().unwrap().1.title());
    assert_eq!(title, "c.txt ↔ b.txt");
}

#[gpui_kit::test]
async fn a_refused_save_as_rekey_tells_the_view(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let p = |n: &str| dir.path().join(n);
    for n in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(p(n), "x\n").unwrap();
    }
    let (handle, workspace) = open(
        cx,
        Launch::Files {
            left: p("a.txt"),
            right: p("b.txt"),
        },
    );
    wait_text_loaded(cx, &workspace).await;
    let first = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected the text tab"),
    });
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_files(p("c.txt"), p("b.txt"), None, window, cx)
        });
    })
    .unwrap();
    // The first tab holds an unsaved edit on the other side, so it cannot close.
    first.update(cx, |v, cx| v.set_caret_for_test(false, 0, cx));
    first.update(cx, |v, cx| {
        v.edit(0..0, "z", cx);
    });
    let target = p("c.txt");
    let done = cx
        .update_window(handle, |_, window, cx| {
            first.update(cx, |v, cx| v.save_side_as(true, target, window, cx))
        })
        .unwrap();
    assert!(done.await);
    assert_eq!(kinds(&workspace, cx), ["text", "text"]);
    assert_eq!(
        first
            .read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("Saved. This pair is already open in another tab.")
    );
}

#[gpui_kit::test]
async fn recent_hex_entries_round_trip_and_reopen_as_hex(cx: &mut TestAppContext) {
    use duet_app::recent::{self, Recent, RecentKind};
    let text = "H\tC:\\a.bin\tC:\\b.bin\nF\tC:\\x\tC:\\y\nT\tC:\\a.txt\tC:\\b.txt\n";
    let list = recent::parse(text);
    assert_eq!(list[0].kind, RecentKind::Hex);
    assert_eq!(recent::render(&list), text);

    // A hex entry reopens as hex even when the files look like text now.
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let files = tempfile::tempdir().unwrap();
    let (a, b) = (files.path().join("a.txt"), files.path().join("b.txt"));
    std::fs::write(&a, "one\n").unwrap();
    std::fs::write(&b, "two\n").unwrap();
    recent::record(
        &data.path().join("recent.txt"),
        Recent {
            kind: RecentKind::Hex,
            left: a,
            right: b,
            base: None,
        },
    )
    .unwrap();
    let (_handle, workspace) = open(cx, Launch::Start);
    cx.run_until_parked();
    let start = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    });
    assert_eq!(
        start.read_with(cx, |s, _| s.recent()[0].0.kind),
        RecentKind::Hex
    );
    start.update(cx, |s, cx| s.open_recent(0, cx));
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start", "hex"]);
}

#[gpui_kit::test]
async fn recent_table_entries_round_trip_and_reopen_as_table(cx: &mut TestAppContext) {
    use duet_app::recent::{self, Recent, RecentKind};
    let text = "C\tC:\\a.csv\tC:\\b.csv\nH\tC:\\a.bin\tC:\\b.bin\nT\tC:\\a.txt\tC:\\b.txt\n";
    let list = recent::parse(text);
    assert_eq!(list[0].kind, RecentKind::Table);
    assert_eq!(recent::render(&list), text);

    // A table entry reopens as a table even though the files look like text.
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let files = tempfile::tempdir().unwrap();
    let (a, b) = (files.path().join("a.csv"), files.path().join("b.csv"));
    std::fs::write(&a, "id,v\n1,a\n").unwrap();
    std::fs::write(&b, "id,v\n1,b\n").unwrap();
    recent::record(
        &data.path().join("recent.txt"),
        Recent {
            kind: RecentKind::Table,
            left: a,
            right: b,
            base: None,
        },
    )
    .unwrap();
    let (_handle, workspace) = open(cx, Launch::Start);
    cx.run_until_parked();
    let start = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    });
    assert_eq!(
        start.read_with(cx, |s, _| s.recent()[0].0.kind),
        RecentKind::Table
    );
    start.update(cx, |s, cx| s.open_recent(0, cx));
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start", "table"]);
}

#[gpui_kit::test]
async fn a_hex_tab_goes_back_to_its_folder_and_follows_a_swap(cx: &mut TestAppContext) {
    let (_dir, l, r) = pair_of_dirs();
    let (handle, workspace) = open(
        cx,
        Launch::Folders {
            left: l.clone(),
            right: r.clone(),
        },
    );
    let (a, b) = (l.join("a.txt"), r.join("a.txt"));
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            let origin = w.tabs().iter().next().map(|(_, key, _)| key.clone());
            w.open_hex(a.clone(), b.clone(), origin, window, cx);
        })
    })
    .unwrap();
    assert_eq!(kinds(&workspace, cx), ["folder", "hex"]);
    let hex = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Hex(view)) => view.clone(),
        _ => panic!("expected the hex tab"),
    });
    hex.update(cx, |_, cx| {
        cx.emit(HexEvent::Swapped {
            left: b.clone(),
            right: a.clone(),
        })
    });
    let key = workspace.read_with(cx, |w, _| w.tabs().iter().nth(1).unwrap().1.clone());
    assert_eq!(key, TabKey::Hex(b, a));
    // A running compare takes the first Esc (it cancels), so let it finish.
    let ws = workspace.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        ws.read(cx).active_idle(cx)
    })
    .await;
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(0),
        "Esc returns to the folder tab, even after the swap"
    );
}

fn binary_pair(dir: &std::path::Path) -> (PathBuf, PathBuf) {
    let (a, b) = (dir.join("a.bin"), dir.join("b.bin"));
    std::fs::write(&a, [1u8, 0, 2, 0]).unwrap();
    std::fs::write(&b, [1u8, 0, 3, 0]).unwrap();
    (a, b)
}

#[gpui_kit::test]
async fn a_text_tab_and_a_hex_tab_of_one_pair_coexist_and_open_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = binary_pair(dir.path());
    let (handle, workspace) = open(
        cx,
        Launch::Files {
            left: a.clone(),
            right: b.clone(),
        },
    );
    for _ in 0..2 {
        cx.update_window(handle, |_, window, cx| {
            workspace.update(cx, |w, cx| {
                w.open_hex(a.clone(), b.clone(), None, window, cx)
            })
        })
        .unwrap();
    }
    assert_eq!(
        kinds(&workspace, cx),
        ["text", "hex"],
        "one hex tab, beside the text tab"
    );
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(1)
    );
}

#[gpui_kit::test]
async fn a_hex_open_event_re_keys_the_tab_or_activates_the_open_pair(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = binary_pair(dir.path());
    let c = dir.path().join("c.bin");
    std::fs::write(&c, [9u8, 0]).unwrap();
    let (handle, workspace) = open(
        cx,
        Launch::Hex {
            left: a.clone(),
            right: b.clone(),
        },
    );
    let hex = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Hex(view)) => view.clone(),
        _ => panic!("expected the hex tab"),
    });
    cx.update_window(handle, |_, _, cx| {
        hex.update(cx, |_, cx| {
            cx.emit(HexEvent::Open {
                left: a.clone(),
                right: c.clone(),
            })
        })
    })
    .unwrap();
    let key = workspace.read_with(cx, |w, _| w.tabs().iter().next().unwrap().1.clone());
    assert_eq!(
        key,
        TabKey::Hex(a.clone(), c.clone()),
        "the tab names what it shows"
    );
    assert_eq!(
        hex.read_with(cx, |v, _| (
            v.paths().0.to_path_buf(),
            v.paths().1.to_path_buf()
        )),
        (a.clone(), c.clone())
    );
    // A second tab for the original pair; opening that pair from the first goes there.
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_hex(a.clone(), b.clone(), None, window, cx)
        })
    })
    .unwrap();
    workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
    cx.update_window(handle, |_, _, cx| {
        hex.update(cx, |_, cx| {
            cx.emit(HexEvent::Open {
                left: a.clone(),
                right: b.clone(),
            })
        })
    })
    .unwrap();
    assert_eq!(kinds(&workspace, cx), ["hex", "hex"], "neither tab closed");
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(1)
    );
}

#[gpui_kit::test]
async fn the_typed_form_opens_a_binary_pair_as_hex_after_the_background_check(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = binary_pair(dir.path());
    let (handle, workspace) = open(cx, Launch::Start);
    let start = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    });
    let (a_text, b_text) = (a.display().to_string(), b.display().to_string());
    cx.update_window(handle, |_, window, cx| {
        start.update(cx, |view, cx| {
            view.set_paths(&a_text, &b_text, window, cx);
            view.compare(cx);
        })
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start", "hex"]);
}

#[gpui_kit::test]
async fn a_check_the_user_has_moved_past_is_ignored(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = binary_pair(dir.path());
    let (handle, workspace) = open(cx, Launch::Start);
    let start = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    });
    let (a_text, b_text) = (a.display().to_string(), b.display().to_string());
    cx.update_window(handle, |_, window, cx| {
        start.update(cx, |view, cx| {
            view.set_paths(&a_text, &b_text, window, cx);
            view.compare(cx);
            // The form changes before the check comes back.
            view.set_paths("somewhere-else", &b_text, window, cx);
        })
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        kinds(&workspace, cx),
        ["start"],
        "the old answer opens nothing"
    );
}

/// A folder pair holding one binary pair (`a.bin`), one text pair (`t.txt`) and a file only
/// the left side has.
fn routing_dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let (l, r) = (dir.path().join("l"), dir.path().join("r"));
    std::fs::create_dir_all(&l).unwrap();
    std::fs::create_dir_all(&r).unwrap();
    std::fs::write(l.join("a.bin"), [1u8, 0, 2, 0]).unwrap();
    std::fs::write(r.join("a.bin"), [1u8, 0, 3, 0, 9]).unwrap();
    std::fs::write(l.join("t.txt"), "1\n").unwrap();
    std::fs::write(r.join("t.txt"), "22\n").unwrap();
    std::fs::write(l.join("only.txt"), "x\n").unwrap();
    (dir, l, r)
}

fn active_folder(workspace: &Entity<Workspace>, cx: &mut TestAppContext) -> Entity<FolderView> {
    workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Folder(view)) => view.clone(),
        _ => panic!("expected the folder tab"),
    })
}

async fn folder_ready(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    folder: &Entity<FolderView>,
) {
    let folder = folder.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        let v = folder.read(cx);
        v.model().is_some() && !v.is_busy()
    })
    .await;
}

fn row_of(folder: &Entity<FolderView>, rel: &str, cx: &mut TestAppContext) -> usize {
    folder.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (0..m.visible_len())
            .position(|row| m.node_at(row).rel == rel)
            .unwrap_or_else(|| panic!("no row {rel}"))
    })
}

#[gpui_kit::test]
async fn the_start_form_opens_a_binary_pair_as_hex(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = binary_pair(dir.path());
    let (handle, workspace) = open(cx, Launch::Start);
    let start = active_start(&workspace, cx);
    let (a_text, b_text) = (a.display().to_string(), b.display().to_string());
    cx.update_window(handle, |_, window, cx| {
        start.update(cx, |view, cx| {
            view.set_paths(&a_text, &b_text, window, cx);
            view.compare(cx);
        })
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start", "hex"]);
}

#[gpui_kit::test]
async fn the_hex_mode_card_opens_a_file_picker_and_forces_hex(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "one\n").unwrap();
    std::fs::write(&b, "two\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Start);
    let start = active_start(&workspace, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("start-mode-hex").is_some());
        window.click("start-mode-hex", cx);
    })
    .unwrap();
    assert!(cx.did_prompt_for_paths());
    let picked = a.clone();
    cx.simulate_path_prompt_response(move |options| {
        assert!(
            options.files && !options.directories,
            "the card picks files"
        );
        Some(vec![picked])
    });
    cx.run_until_parked();
    let (a_text, b_text) = (a.display().to_string(), b.display().to_string());
    let compare = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            start.update(cx, |view, cx| {
                view.set_paths(&a_text, &b_text, window, cx);
                view.compare(cx);
            })
        })
        .unwrap();
        cx.run_until_parked();
    };
    compare(cx);
    assert_eq!(
        kinds(&workspace, cx),
        ["start", "hex"],
        "two text files open as hex: the card forced the kind"
    );
    // The force ends with that compare: the next one is detected as usual.
    workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
    compare(cx);
    assert_eq!(kinds(&workspace, cx), ["start", "hex", "text"]);
}

#[gpui_kit::test]
async fn the_hex_cards_force_ends_on_a_cancelled_picker_or_a_typed_box(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "one\n").unwrap();
    std::fs::write(&b, "two\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Start);
    let start = active_start(&workspace, cx);
    let (a_text, b_text) = (a.display().to_string(), b.display().to_string());
    let card = |answer: Option<Vec<PathBuf>>, cx: &mut TestAppContext| {
        workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("start-mode-hex", cx);
        })
        .unwrap();
        cx.simulate_path_prompt_response(move |_| answer);
        cx.run_until_parked();
    };
    let compare = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            start.update(cx, |view, cx| {
                view.set_paths(&a_text, &b_text, window, cx);
                view.compare(cx);
            })
        })
        .unwrap();
        cx.run_until_parked();
    };

    // The card's picker cancelled: two text files are detected as text.
    card(None, cx);
    compare(cx);
    assert_eq!(kinds(&workspace, cx), ["start", "text"]);

    // A file picked, then either box typed in: the typed pair is detected too (the text tab
    // that is open already comes to the front, and no hex tab opens).
    for left in [true, false] {
        card(Some(vec![a.clone()]), cx);
        cx.update_window(handle, |_, window, cx| {
            let start = start.read(cx);
            let input = if left {
                start.left_input().clone()
            } else {
                start.right_input().clone()
            };
            input.focus_handle(cx).focus(window, cx);
            window.input("x", cx);
        })
        .unwrap();
        compare(cx);
        assert_eq!(kinds(&workspace, cx), ["start", "text"], "left box: {left}");
    }
}

#[gpui_kit::test]
async fn double_click_on_a_binary_pair_opens_a_hex_tab(cx: &mut TestAppContext) {
    let (_dir, l, r) = routing_dirs();
    let (handle, workspace) = open(cx, Launch::Folders { left: l, right: r });
    let folder = active_folder(&workspace, cx);
    folder_ready(cx, handle, &folder).await;
    let (bin, txt) = (row_of(&folder, "a.bin", cx), row_of(&folder, "t.txt", cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.double_click(("folder-row", bin), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["folder", "hex"]);
    workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.double_click(("folder-row", txt), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["folder", "hex", "text"]);
}

fn folder_menu(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    folder: &Entity<FolderView>,
    label: &str,
) -> bool {
    cx.update_window(handle, |_, window, cx| {
        let entries = duet_app::folder::menu::row_menu(folder.read(cx));
        let focus = folder.focus_handle(cx);
        duet_app::ui::menu::activate(entries, label, Some(&focus), window, cx)
    })
    .unwrap()
}

#[gpui_kit::test]
async fn folder_menu_offers_open_as_text_and_hex(cx: &mut TestAppContext) {
    let (_dir, l, r) = routing_dirs();
    let (handle, workspace) = open(cx, Launch::Folders { left: l, right: r });
    let folder = active_folder(&workspace, cx);
    folder_ready(cx, handle, &folder).await;
    let outline = |folder: &Entity<FolderView>, cx: &mut TestAppContext| {
        folder.read_with(cx, |v, _| {
            duet_app::ui::menu::outline(&duet_app::folder::menu::row_menu(v))
        })
    };
    let select = |rel: &str, cx: &mut TestAppContext| {
        let row = row_of(&folder, rel, cx);
        folder.update(cx, |v, cx| v.select_row_for_test(row, cx));
    };
    select("only.txt", cx);
    let items = outline(&folder, cx);
    assert!(items.contains(&"Open as text".to_owned()), "{items:?}");
    assert!(items.contains(&"(Open as hex)".to_owned()), "{items:?}");
    select("a.bin", cx);
    let items = outline(&folder, cx);
    assert!(items.contains(&"Open as text".to_owned()), "{items:?}");
    assert!(items.contains(&"Open as hex".to_owned()), "{items:?}");
    // Open as text on a binary pair is a text tab, which shows the binary message.
    assert!(folder_menu(cx, handle, &folder, "Open as text"));
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["folder", "text"]);
    workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
    select("t.txt", cx);
    assert!(folder_menu(cx, handle, &folder, "Open as hex"));
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["folder", "text", "hex"]);
}

#[gpui_kit::test]
async fn the_binary_message_offers_open_in_hex(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = binary_pair(dir.path());
    let (handle, workspace) = open(cx, Launch::Files { left: a, right: b });
    let text = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected the text tab"),
    });
    let t = text.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        t.read(cx).message().is_some()
    })
    .await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("text-open-hex", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["text", "hex"]);
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(1),
        "the hex tab is active"
    );
}

#[gpui_kit::test]
async fn the_text_path_menu_opens_in_hex(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "one\n").unwrap();
    std::fs::write(&b, "two\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Files { left: a, right: b });
    let text = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected the text tab"),
    });
    let t = text.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !t.read(cx).is_busy()
    })
    .await;
    let entries = text.read_with(cx, |v, _| duet_app::text::menu::path_menu(v, true));
    assert!(
        duet_app::ui::menu::outline(&entries).contains(&"Open in hex".to_owned()),
        "text files can open in hex too"
    );
    cx.update_window(handle, |_, window, cx| {
        let focus = text.focus_handle(cx);
        assert!(duet_app::ui::menu::activate(
            entries,
            "Open in hex",
            Some(&focus),
            window,
            cx
        ));
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["text", "hex"]);
}

#[gpui_kit::test]
async fn the_table_card_opens_a_picker_and_forces_table(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "k,v\n1,x\n").unwrap();
    std::fs::write(&b, "k,v\n1,y\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Start);
    let start = active_start(&workspace, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("start-mode-table").is_some());
        window.click("start-mode-table", cx);
    })
    .unwrap();
    assert!(cx.did_prompt_for_paths());
    let picked = a.clone();
    cx.simulate_path_prompt_response(move |options| {
        assert!(
            options.files && !options.directories,
            "the card picks files"
        );
        Some(vec![picked])
    });
    cx.run_until_parked();
    let (a_text, b_text) = (a.display().to_string(), b.display().to_string());
    let compare = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            start.update(cx, |view, cx| {
                view.set_paths(&a_text, &b_text, window, cx);
                view.compare(cx);
            })
        })
        .unwrap();
        cx.run_until_parked();
    };
    compare(cx);
    assert_eq!(
        kinds(&workspace, cx),
        ["start", "table"],
        "two text files open as a table: the card forced the kind"
    );
    // The force ends with that compare: the next one is detected as usual.
    workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
    compare(cx);
    assert_eq!(kinds(&workspace, cx), ["start", "table", "text"]);
}

#[gpui_kit::test]
async fn the_table_cards_force_ends_on_a_cancelled_picker_or_a_typed_box(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "one\n").unwrap();
    std::fs::write(&b, "two\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Start);
    let start = active_start(&workspace, cx);
    let (a_text, b_text) = (a.display().to_string(), b.display().to_string());
    let card = |answer: Option<Vec<PathBuf>>, cx: &mut TestAppContext| {
        workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("start-mode-table", cx);
        })
        .unwrap();
        cx.simulate_path_prompt_response(move |_| answer);
        cx.run_until_parked();
    };
    let compare = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            start.update(cx, |view, cx| {
                view.set_paths(&a_text, &b_text, window, cx);
                view.compare(cx);
            })
        })
        .unwrap();
        cx.run_until_parked();
    };
    card(None, cx);
    compare(cx);
    assert_eq!(kinds(&workspace, cx), ["start", "text"]);
    for left in [true, false] {
        card(Some(vec![a.clone()]), cx);
        cx.update_window(handle, |_, window, cx| {
            let start = start.read(cx);
            let input = if left {
                start.left_input().clone()
            } else {
                start.right_input().clone()
            };
            input.focus_handle(cx).focus(window, cx);
            window.input("x", cx);
        })
        .unwrap();
        compare(cx);
        assert_eq!(kinds(&workspace, cx), ["start", "text"], "left box: {left}");
    }
}

#[gpui_kit::test]
async fn the_picture_card_opens_a_picker_and_forces_picture(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "one\n").unwrap();
    std::fs::write(&b, "two\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Start);
    let start = active_start(&workspace, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("start-mode-picture").is_some());
        window.click("start-mode-picture", cx);
    })
    .unwrap();
    assert!(cx.did_prompt_for_paths());
    let picked = a.clone();
    cx.simulate_path_prompt_response(move |options| {
        assert!(
            options.files && !options.directories,
            "the card picks files"
        );
        Some(vec![picked])
    });
    cx.run_until_parked();
    let (a_text, b_text) = (a.display().to_string(), b.display().to_string());
    let compare = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            start.update(cx, |view, cx| {
                view.set_paths(&a_text, &b_text, window, cx);
                view.compare(cx);
            })
        })
        .unwrap();
        cx.run_until_parked();
    };
    compare(cx);
    assert_eq!(
        kinds(&workspace, cx),
        ["start", "picture"],
        "two text files open as a picture: the card forced the kind"
    );
    workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
    compare(cx);
    assert_eq!(kinds(&workspace, cx), ["start", "picture", "text"]);
}

#[gpui_kit::test]
async fn the_picture_cards_force_ends_on_a_cancelled_picker_or_a_typed_box(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "one\n").unwrap();
    std::fs::write(&b, "two\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Start);
    let start = active_start(&workspace, cx);
    let (a_text, b_text) = (a.display().to_string(), b.display().to_string());
    let card = |answer: Option<Vec<PathBuf>>, cx: &mut TestAppContext| {
        workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("start-mode-picture", cx);
        })
        .unwrap();
        cx.simulate_path_prompt_response(move |_| answer);
        cx.run_until_parked();
    };
    let compare = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            start.update(cx, |view, cx| {
                view.set_paths(&a_text, &b_text, window, cx);
                view.compare(cx);
            })
        })
        .unwrap();
        cx.run_until_parked();
    };
    card(None, cx);
    compare(cx);
    assert_eq!(kinds(&workspace, cx), ["start", "text"]);
    for left in [true, false] {
        card(Some(vec![a.clone()]), cx);
        cx.update_window(handle, |_, window, cx| {
            let start = start.read(cx);
            let input = if left {
                start.left_input().clone()
            } else {
                start.right_input().clone()
            };
            input.focus_handle(cx).focus(window, cx);
            window.input("x", cx);
        })
        .unwrap();
        compare(cx);
        assert_eq!(kinds(&workspace, cx), ["start", "text"], "left box: {left}");
    }
}

fn png_bytes(side: u32) -> Vec<u8> {
    let one = duet_core::picture::Picture::filled(side, side, [9, 9, 9, 255]);
    duet_core::picture::export::encode_png(&one).unwrap()
}

fn png_dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let (l, r) = (dir.path().join("l"), dir.path().join("r"));
    std::fs::create_dir_all(&l).unwrap();
    std::fs::create_dir_all(&r).unwrap();
    // The sides differ, so the rows show without a filter.
    for (side, shade) in [(&l, 1u32), (&r, 3u32)] {
        std::fs::write(side.join("a.png"), png_bytes(shade * 2)).unwrap();
        std::fs::write(side.join("n.txt"), "x".repeat(shade as usize)).unwrap();
    }
    std::fs::write(l.join("only.png"), png_bytes(1)).unwrap();
    (dir, l, r)
}

#[gpui_kit::test]
async fn folder_rows_open_pictures_by_double_click_and_menu(cx: &mut TestAppContext) {
    let (_dir, l, r) = png_dirs();
    let (handle, workspace) = open(cx, Launch::Folders { left: l, right: r });
    let folder = active_folder(&workspace, cx);
    folder_ready(cx, handle, &folder).await;
    let row = row_of(&folder, "a.png", cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.double_click(("folder-row", row), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["folder", "picture"]);
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(0),
        "Esc returns to the folder tab"
    );
    let outline = |folder: &Entity<FolderView>, cx: &mut TestAppContext| {
        folder.read_with(cx, |v, _| {
            duet_app::ui::menu::outline(&duet_app::folder::menu::row_menu(v))
        })
    };
    let select = |rel: &str, cx: &mut TestAppContext| {
        let row = row_of(&folder, rel, cx);
        folder.update(cx, |v, cx| v.select_row_for_test(row, cx));
    };
    select("only.png", cx);
    let items = outline(&folder, cx);
    assert!(items.contains(&"(Open as picture)".to_owned()), "{items:?}");
    select("a.png", cx);
    let items = outline(&folder, cx);
    assert!(items.contains(&"Open as picture".to_owned()), "{items:?}");
    select("n.txt", cx);
    assert!(folder_menu(cx, handle, &folder, "Open as picture"));
    cx.run_until_parked();
    let picture = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Picture(view)) => view.clone(),
        _ => panic!("expected the picture tab"),
    });
    let p = picture.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        p.read(cx).message(true).is_some()
    })
    .await;
    let message = picture.read_with(cx, |v, _| v.message(true).unwrap().to_owned());
    assert!(message.starts_with("Can't open n.txt: "), "{message}");
}

#[gpui_kit::test]
async fn the_binary_notice_offers_open_as_picture_for_pictures_only(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.png"), dir.path().join("b.png"));
    std::fs::write(&a, png_bytes(1)).unwrap();
    std::fs::write(&b, png_bytes(1)).unwrap();
    let (handle, workspace) = open(cx, Launch::Files { left: a, right: b });
    let text = active_text(&workspace, cx);
    let t = text.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        t.read(cx).message().is_some()
    })
    .await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("text-open-picture", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["text", "picture"]);
}

#[gpui_kit::test]
async fn a_binary_pair_that_is_not_pictures_has_no_picture_button(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = binary_pair(dir.path());
    let (handle, workspace) = open(cx, Launch::Files { left: a, right: b });
    let text = active_text(&workspace, cx);
    let t = text.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        t.read(cx).message().is_some()
    })
    .await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("text-open-hex").is_some());
        assert!(window.try_find("text-open-picture").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
async fn the_text_path_menu_opens_as_picture(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "one\n").unwrap();
    std::fs::write(&b, "two\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Files { left: a, right: b });
    let text = active_text(&workspace, cx);
    let t = text.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !t.read(cx).is_busy()
    })
    .await;
    let entries = text.read_with(cx, |v, _| duet_app::text::menu::path_menu(v, true));
    assert!(duet_app::ui::menu::outline(&entries).contains(&"Open as picture".to_owned()));
    cx.update_window(handle, |_, window, cx| {
        let focus = text.focus_handle(cx);
        assert!(duet_app::ui::menu::activate(
            entries,
            "Open as picture",
            Some(&focus),
            window,
            cx
        ));
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["text", "picture"]);
}

fn csv_dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let (l, r) = (dir.path().join("l"), dir.path().join("r"));
    std::fs::create_dir_all(&l).unwrap();
    std::fs::create_dir_all(&r).unwrap();
    std::fs::write(l.join("a.csv"), "k,v\n1,x\n").unwrap();
    std::fs::write(r.join("a.csv"), "k,v\n1,yy\n").unwrap();
    std::fs::write(l.join("only.csv"), "k\n1\n").unwrap();
    (dir, l, r)
}

#[gpui_kit::test]
async fn folder_menu_offers_open_as_table(cx: &mut TestAppContext) {
    let (_dir, l, r) = csv_dirs();
    let (handle, workspace) = open(cx, Launch::Folders { left: l, right: r });
    let folder = active_folder(&workspace, cx);
    folder_ready(cx, handle, &folder).await;
    let outline = |folder: &Entity<FolderView>, cx: &mut TestAppContext| {
        folder.read_with(cx, |v, _| {
            duet_app::ui::menu::outline(&duet_app::folder::menu::row_menu(v))
        })
    };
    let select = |rel: &str, cx: &mut TestAppContext| {
        let row = row_of(&folder, rel, cx);
        folder.update(cx, |v, cx| v.select_row_for_test(row, cx));
    };
    select("only.csv", cx);
    let items = outline(&folder, cx);
    assert!(items.contains(&"(Open as table)".to_owned()), "{items:?}");
    select("a.csv", cx);
    let items = outline(&folder, cx);
    assert!(items.contains(&"Open as table".to_owned()), "{items:?}");
    assert!(folder_menu(cx, handle, &folder, "Open as table"));
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["folder", "table"]);
    // The folder tab is the origin: Esc goes back.
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(0),
        "Esc returns to the folder tab"
    );
}

#[gpui_kit::test]
async fn csv_still_opens_as_text_by_default(cx: &mut TestAppContext) {
    let (_dir, l, r) = csv_dirs();
    let (handle, workspace) = open(cx, Launch::Folders { left: l, right: r });
    let folder = active_folder(&workspace, cx);
    folder_ready(cx, handle, &folder).await;
    let row = row_of(&folder, "a.csv", cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.double_click(("folder-row", row), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["folder", "text"]);
}

#[gpui_kit::test]
async fn the_text_path_menu_opens_as_table(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.csv"), dir.path().join("b.csv"));
    std::fs::write(&a, "k,v\n1,x\n").unwrap();
    std::fs::write(&b, "k,v\n1,y\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Files { left: a, right: b });
    let text = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected the text tab"),
    });
    let t = text.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !t.read(cx).is_busy()
    })
    .await;
    let entries = text.read_with(cx, |v, _| duet_app::text::menu::path_menu(v, true));
    assert!(duet_app::ui::menu::outline(&entries).contains(&"Open as table".to_owned()));
    cx.update_window(handle, |_, window, cx| {
        let focus = text.focus_handle(cx);
        assert!(duet_app::ui::menu::activate(
            entries,
            "Open as table",
            Some(&focus),
            window,
            cx
        ));
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["text", "table"]);
}

#[gpui_kit::test]
async fn a_typed_path_and_a_drop_replace_a_side(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = binary_pair(dir.path());
    let (c, d) = (dir.path().join("c.bin"), dir.path().join("d.bin"));
    std::fs::write(&c, [9u8, 0]).unwrap();
    std::fs::write(&d, [8u8, 0]).unwrap();
    let (handle, workspace) = open(
        cx,
        Launch::Hex {
            left: a.clone(),
            right: b.clone(),
        },
    );
    let hex = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Hex(view)) => view.clone(),
        _ => panic!("expected the hex tab"),
    });
    let key = |cx: &mut TestAppContext| {
        workspace.read_with(cx, |w, _| w.tabs().iter().next().unwrap().1.clone())
    };
    // A typed path on the right: the tab is re-keyed and the view loads it.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("hex-right-path", cx)
    })
    .unwrap();
    hex.update(cx, |v, cx| {
        v.set_path_edit_text_for_test(c.to_str().unwrap(), cx)
    });
    cx.update_window(handle, |_, window, cx| window.press("enter", cx))
        .unwrap();
    assert_eq!(key(cx), TabKey::Hex(a.clone(), c.clone()));
    assert_eq!(
        hex.read_with(cx, |v, _| (
            v.paths().0.to_path_buf(),
            v.paths().1.to_path_buf()
        )),
        (a.clone(), c.clone())
    );
    // A drop of one file on the left half replaces the left side.
    hex.update(cx, |v, cx| v.drop_paths(std::slice::from_ref(&d), true, cx));
    assert_eq!(key(cx), TabKey::Hex(d.clone(), c.clone()));
    // Two files replace both.
    hex.update(cx, |v, cx| v.drop_paths(&[a.clone(), b.clone()], false, cx));
    assert_eq!(key(cx), TabKey::Hex(a.clone(), b.clone()));
    // A pair that another tab holds is activated, not duplicated.
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_hex(d.clone(), c.clone(), None, window, cx)
        })
    })
    .unwrap();
    workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
    hex.update(cx, |v, cx| v.drop_paths(&[d.clone(), c.clone()], true, cx));
    assert_eq!(kinds(&workspace, cx), ["hex", "hex"], "neither tab closed");
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(1)
    );
}

#[gpui_kit::test]
async fn forced_kinds_win_over_picture_detection(cx: &mut TestAppContext) {
    use duet_app::recent::{self, Recent, RecentKind};
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let files = tempfile::tempdir().unwrap();
    let (a, b) = (files.path().join("a.png"), files.path().join("b.png"));
    let (x, y) = (files.path().join("x.txt"), files.path().join("y.txt"));
    let one = duet_core::picture::Picture::filled(2, 2, [9, 9, 9, 255]);
    std::fs::write(&a, duet_core::picture::export::encode_png(&one).unwrap()).unwrap();
    std::fs::write(&b, duet_core::picture::export::encode_png(&one).unwrap()).unwrap();
    std::fs::write(&x, "one\n").unwrap();
    std::fs::write(&y, "two\n").unwrap();
    let file = data.path().join("recent.txt");
    for (kind, left, right) in [
        (RecentKind::Files, &a, &b),
        (RecentKind::Hex, &a, &b),
        (RecentKind::Table, &a, &b),
        (RecentKind::Picture, &x, &y),
    ] {
        recent::record(
            &file,
            Recent {
                kind,
                left: left.clone(),
                right: right.clone(),
                base: None,
            },
        )
        .unwrap();
    }
    let (_handle, workspace) = open(cx, Launch::Start);
    cx.run_until_parked();
    let start = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    });
    // Opening an entry moves it in the list (and a picture adds its own), so each one is found
    // by its kind and left path.
    for (kind, left) in [
        (RecentKind::Files, &a),
        (RecentKind::Hex, &a),
        (RecentKind::Table, &a),
        (RecentKind::Picture, &x),
    ] {
        start.update(cx, |s, cx| {
            let at = s
                .recent()
                .iter()
                .position(|(entry, _)| entry.kind == kind && &entry.left == left)
                .expect("the entry is listed");
            s.open_recent(at, cx)
        });
        cx.run_until_parked();
    }
    assert_eq!(
        kinds(&workspace, cx),
        ["start", "picture", "hex", "table", "picture"],
        "a T entry of two PNGs is a picture; H, C and P force their kind"
    );
}

fn merge_set() -> (tempfile::TempDir, MergePaths) {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        let path = dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        path
    };
    let paths = MergePaths {
        left: Some(write("left.txt", "a\nB\nc\n")),
        base: Some(write("base.txt", "a\nb\nc\n")),
        right: Some(write("right.txt", "a\nb\nc\n")),
        output: None,
    };
    (dir, paths)
}

#[gpui_kit::test]
async fn launch_merge_twice_activates_one_tab(cx: &mut TestAppContext) {
    let (_dir, paths) = merge_set();
    let (handle, workspace) = open(cx, Launch::Merge(paths.clone()));
    let ws = workspace.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        ws.read(cx).active_idle(cx)
    })
    .await;
    assert_eq!(kinds(&workspace, cx), ["merge"]);
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_launch(Launch::Start, window, cx);
            assert_eq!(w.tabs().active_index(), Some(1));
            w.open_launch(Launch::Merge(paths.clone()), window, cx);
        });
    })
    .unwrap();
    assert_eq!(
        kinds(&workspace, cx),
        ["merge", "start"],
        "the same set is one tab"
    );
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(0),
        "and the second launch activates it"
    );
}

#[gpui_kit::test]
async fn launch_merge_without_paths_opens_an_empty_merge_tab(cx: &mut TestAppContext) {
    use duet_app::merge::view::Stage;
    let (_handle, workspace) = open(cx, Launch::Merge(MergePaths::default()));
    assert_eq!(kinds(&workspace, cx), ["merge"]);
    let view = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Merge(view)) => view.clone(),
        _ => panic!("expected a merge tab"),
    });
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().iter().next().unwrap().1.title()),
        "3-way merge"
    );
    assert!(workspace.read_with(cx, |w, cx| w.active_idle(cx)));
    assert_eq!(view.read_with(cx, |v, _| v.stage()), Stage::Input);
    assert_eq!(workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)), None);
}

#[gpui_kit::test]
async fn a_recent_merge_entry_reopens_as_a_merge_tab(cx: &mut TestAppContext) {
    use duet_app::recent::{self, Recent, RecentKind};
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (_dir, paths) = merge_set();
    recent::record(
        &data.path().join("recent.txt"),
        Recent {
            kind: RecentKind::Merge,
            left: paths.left.clone().unwrap(),
            right: paths.right.clone().unwrap(),
            base: paths.base.clone(),
        },
    )
    .unwrap();
    let (_handle, workspace) = open(cx, Launch::Start);
    cx.run_until_parked();
    let start = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    });
    assert_eq!(
        start.read_with(cx, |s, _| s.recent()[0].0.kind),
        RecentKind::Merge
    );
    start.update(cx, |s, cx| s.open_recent(0, cx));
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start", "merge"]);
}

fn active_merge(workspace: &Entity<Workspace>, cx: &mut TestAppContext) -> Entity<MergeView> {
    workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Merge(view)) => view.clone(),
        _ => panic!("expected a merge tab"),
    })
}

#[gpui_kit::test]
async fn the_start_card_opens_an_empty_merge_tab_once(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx, Launch::Start);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("start-mode-merge").label(), Some("3-way merge"));
        window.click("start-mode-merge", cx);
    })
    .unwrap();
    assert_eq!(kinds(&workspace, cx), ["start", "merge"]);
    let view = active_merge(&workspace, cx);
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().iter().nth(1).unwrap().1.title()),
        "3-way merge"
    );
    view.read_with(cx, |v, _| {
        assert_eq!(v.stage(), Stage::Input);
        assert_eq!(v.mode(), InputMode::Files);
        let parts: Vec<String> = v.status_parts().into_iter().map(|p| p.text).collect();
        assert_eq!(parts, ["0 of 3 files chosen"]);
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("merge-run").is_some());
        assert!(!view.read(cx).can_run(), "no files yet");
    })
    .unwrap();

    // A new Start tab and the card again: the same merge tab, not a second one.
    cx.update_window(handle, |_, window, cx| window.press("ctrl-t", cx))
        .unwrap();
    assert_eq!(kinds(&workspace, cx), ["start", "merge", "start"]);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("start-mode-merge", cx);
    })
    .unwrap();
    assert_eq!(kinds(&workspace, cx), ["start", "merge", "start"]);
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(1),
        "the card activates the merge tab"
    );
}

#[gpui_kit::test]
async fn the_recent_merge_entry_names_its_three_files_and_reopens(cx: &mut TestAppContext) {
    use duet_app::recent::{self, Recent, RecentKind};
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (_dir, paths) = merge_set();
    let (l, b, r) = (
        paths.left.clone().unwrap(),
        paths.base.clone().unwrap(),
        paths.right.clone().unwrap(),
    );
    recent::record(
        &data.path().join("recent.txt"),
        Recent {
            kind: RecentKind::Merge,
            left: l.clone(),
            right: r.clone(),
            base: Some(b.clone()),
        },
    )
    .unwrap();
    let (handle, workspace) = open(cx, Launch::Start);
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find(("start-recent", 0usize)).label(),
            Some(
                format!(
                    "Merge: {}, {} and {}",
                    l.display(),
                    b.display(),
                    r.display()
                )
                .as_str()
            )
        );
        // One row (U13): the names with the base after them, then the folder they share.
        assert_eq!(
            window.find(("start-recent-names", 0usize)).label(),
            Some("left.txt ↔ right.txt · base base.txt")
        );
        assert_eq!(
            window.find(("start-recent-place", 0usize)).label(),
            l.parent().map(|p| p.display().to_string()).as_deref()
        );
        window.click(("start-recent", 0usize), cx);
    })
    .unwrap();
    let ws = workspace.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        ws.read(cx).active_idle(cx)
    })
    .await;
    assert_eq!(kinds(&workspace, cx), ["start", "merge"]);
    active_merge(&workspace, cx).read_with(cx, |v, _| {
        let name = |slot| v.slot(slot).map(|t| t.name.clone());
        assert_eq!(name(0).as_deref(), Some("left.txt"));
        assert_eq!(name(1).as_deref(), Some("base.txt"));
        assert_eq!(name(2).as_deref(), Some("right.txt"));
    });

    // An entry whose base is gone is dimmed: its check says the files are not all there.
    std::fs::remove_file(&b).unwrap();
    let (_handle, workspace) = open(cx, Launch::Start);
    cx.run_until_parked();
    let start = active_start(&workspace, cx);
    assert_eq!(start.read_with(cx, |s, _| s.recent()[0].1), Some(false));
}

/// A merge set with one conflict, writing to `out.txt`.
fn conflict_set() -> (tempfile::TempDir, MergePaths) {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        let path = dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        path
    };
    let paths = MergePaths {
        left: Some(write("left.txt", "a\nX\nc\n")),
        base: Some(write("base.txt", "a\nb\nc\n")),
        right: Some(write("right.txt", "a\nY\nc\n")),
        output: Some(dir.path().join("out.txt")),
    };
    (dir, paths)
}

async fn wait_merge_idle(cx: &mut TestAppContext, handle: AnyWindowHandle, w: &Entity<Workspace>) {
    let ws = w.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        ws.read(cx).active_idle(cx)
    })
    .await;
}

/// Resolves the first conflict of the active merge tab with the left side.
fn choose_left(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("merge-use-left", 1usize), cx);
    })
    .unwrap();
}

fn press_key(cx: &mut TestAppContext, handle: AnyWindowHandle, key: &str) {
    cx.update_window(handle, |_, window, cx| window.press(key, cx))
        .unwrap();
}

#[gpui_kit::test]
async fn closing_a_dirty_merge_tab_or_window_asks_first(cx: &mut TestAppContext) {
    let (dir, paths) = conflict_set();
    let out = dir.path().join("out.txt");
    let (handle, workspace) = open(cx, Launch::Merge(paths.clone()));
    wait_merge_idle(cx, handle, &workspace).await;

    // A clean tab closes without a question.
    press_key(cx, handle, "ctrl-w");
    assert!(!cx.has_pending_prompt());
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start"]);

    // A choice makes it dirty: Cancel keeps the tab, Discard closes it.
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_launch(Launch::Merge(paths.clone()), window, cx)
        })
    })
    .unwrap();
    wait_merge_idle(cx, handle, &workspace).await;
    choose_left(cx, handle);
    assert!(workspace.read_with(cx, |w, cx| w.tab_dirty(1, cx)));
    press_key(cx, handle, "ctrl-w");
    let (message, detail) = cx.pending_prompt().expect("a prompt");
    assert_eq!(message, "Save the merge result?");
    assert_eq!(
        Some(detail.as_str()),
        Some("Your conflict choices are lost if you don't save them.")
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start", "merge"]);
    press_key(cx, handle, "ctrl-w");
    cx.simulate_prompt_answer("Discard");
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start"]);
    assert!(!out.exists(), "Discard writes nothing");

    // Save writes the output, then closes the tab.
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_launch(Launch::Merge(paths.clone()), window, cx)
        })
    })
    .unwrap();
    wait_merge_idle(cx, handle, &workspace).await;
    choose_left(cx, handle);
    press_key(cx, handle, "ctrl-w");
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Save");
    let ws = workspace.clone();
    let seen = out.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        seen.exists() && ws.read(cx).tabs().iter().count() == 1
    })
    .await;
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "a\nX\nc\n");
    assert_eq!(kinds(&workspace, cx), ["start"]);
}

#[gpui_kit::test]
async fn closing_the_window_asks_for_a_text_tab_and_a_merge_tab_in_turn(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (l, r) = (dir.path().join("l.txt"), dir.path().join("r.txt"));
    std::fs::write(&l, "1\n").unwrap();
    std::fs::write(&r, "2\n").unwrap();
    let (_set, paths) = conflict_set();
    let (handle, workspace) = open(cx, Launch::Files { left: l, right: r });
    wait_text_loaded(cx, &workspace).await;
    type_into_active_text(cx, handle, &workspace, "z");
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_launch(Launch::Merge(paths.clone()), window, cx)
        })
    })
    .unwrap();
    wait_merge_idle(cx, handle, &workspace).await;
    choose_left(cx, handle);

    let task = cx
        .update_window(handle, |_, window, cx| {
            workspace.update(cx, |w, cx| w.confirm_close_all(window, cx))
        })
        .unwrap();
    let (message, _) = cx.pending_prompt().expect("the text tab asks first");
    assert!(message.starts_with("Save changes to"), "{message}");
    cx.simulate_prompt_answer("Discard");
    cx.run_until_parked();
    let (message, _) = cx.pending_prompt().expect("then the merge tab");
    assert_eq!(message, "Save the merge result?");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(!task.await, "Cancel keeps the window open");
    assert_eq!(kinds(&workspace, cx), ["text", "merge"]);

    // Both answered Discard: the window may close.
    let task = cx
        .update_window(handle, |_, window, cx| {
            workspace.update(cx, |w, cx| w.confirm_close_all(window, cx))
        })
        .unwrap();
    cx.simulate_prompt_answer("Discard");
    cx.run_until_parked();
    cx.simulate_prompt_answer("Discard");
    cx.run_until_parked();
    assert!(task.await);
}

/// Types one line into each paste field of `view` (the active tab), then presses Merge.
fn type_a_merge(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<MergeView>,
    texts: [&str; 3],
) {
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.set_mode(InputMode::Paste, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    for (slot, text) in texts.into_iter().enumerate() {
        let field = view.read_with(cx, |v, _| v.paste_input_for_test(slot));
        cx.update_window(handle, |_, window, cx| {
            window.focus(&field.focus_handle(cx), cx);
            window.input(text, cx);
        })
        .unwrap();
    }
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("merge-run", cx);
    })
    .unwrap();
}

fn keys(workspace: &Entity<Workspace>, cx: &mut TestAppContext) -> Vec<TabKey> {
    workspace.read_with(cx, |w, _| {
        w.tabs().iter().map(|(_, key, _)| key.clone()).collect()
    })
}

fn open_empty_merge(cx: &mut TestAppContext, handle: AnyWindowHandle, w: &Entity<Workspace>) {
    cx.update_window(handle, |_, window, cx| {
        w.update(cx, |w, cx| {
            w.open_launch(Launch::Merge(MergePaths::default()), window, cx)
        })
    })
    .unwrap();
}

#[gpui_kit::test]
async fn a_typed_merge_never_closes_its_tab_over_the_empty_merge_tab(cx: &mut TestAppContext) {
    let (_dir, paths) = merge_set();
    let (handle, workspace) = open(cx, Launch::Merge(paths));
    wait_merge_idle(cx, handle, &workspace).await;
    let typed = active_merge(&workspace, cx);
    open_empty_merge(cx, handle, &workspace);
    assert_eq!(kinds(&workspace, cx), ["merge", "merge"]);
    workspace.update(cx, |w, cx| {
        w.tabs_mut_for_test().activate(0);
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        typed.update(cx, |v, cx| v.clear_all(window, cx));
    })
    .unwrap();
    type_a_merge(cx, handle, &typed, ["mine", "base", "base"]);
    wait_merge_idle(cx, handle, &workspace).await;
    assert_eq!(kinds(&workspace, cx), ["merge", "merge"], "both tabs stay");
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(0),
        "and the typed merge is still the active one"
    );
    typed.read_with(cx, |v, _| {
        let left = v.session().expect("a session").left();
        assert_eq!(left.text(), "mine");
        assert!(left.path.is_none());
    });
    let keys = keys(&workspace, cx);
    assert_ne!(
        keys[0], keys[1],
        "two sets of no files have keys of their own"
    );
    assert_eq!(keys[0].title(), "3-way merge");
}

#[gpui_kit::test]
async fn the_start_card_reuses_only_an_untouched_merge_tab(cx: &mut TestAppContext) {
    let (_dir, paths) = merge_set();
    let (handle, workspace) = open(cx, Launch::Start);
    open_empty_merge(cx, handle, &workspace);
    assert_eq!(kinds(&workspace, cx), ["start", "merge"]);
    let first = active_merge(&workspace, cx);
    // A file chosen in it: the tab is in use, so the card opens a new one.
    first.update(cx, |v, cx| v.open_slot_path(LEFT, paths.left.unwrap(), cx));
    cx.run_until_parked();
    open_empty_merge(cx, handle, &workspace);
    assert_eq!(kinds(&workspace, cx), ["start", "merge", "merge"]);
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(2)
    );
    // The new one is untouched: the card goes back to it.
    workspace.update(cx, |w, cx| {
        w.tabs_mut_for_test().activate(0);
        cx.notify();
    });
    open_empty_merge(cx, handle, &workspace);
    assert_eq!(kinds(&workspace, cx), ["start", "merge", "merge"]);
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(2)
    );
    let keys = keys(&workspace, cx);
    assert_ne!(keys[1], keys[2]);
}

#[gpui_kit::test]
async fn a_dirty_merge_swapped_onto_an_open_set_swaps_back(cx: &mut TestAppContext) {
    let (_dir, paths) = conflict_set();
    let swapped = MergePaths {
        left: paths.right.clone(),
        base: paths.base.clone(),
        right: paths.left.clone(),
        output: None,
    };
    let (handle, workspace) = open(cx, Launch::Merge(paths.clone()));
    wait_merge_idle(cx, handle, &workspace).await;
    let view = active_merge(&workspace, cx);
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_launch(Launch::Merge(swapped), window, cx)
        })
    })
    .unwrap();
    wait_merge_idle(cx, handle, &workspace).await;
    workspace.update(cx, |w, cx| {
        w.tabs_mut_for_test().activate(0);
        cx.notify();
    });
    choose_left(cx, handle);
    assert!(workspace.read_with(cx, |w, cx| w.tab_dirty(0, cx)));
    cx.update_window(handle, |_, window, cx| {
        window.focus(&view.focus_handle(cx), cx);
        window.press("f9", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["merge", "merge"]);
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().iter().next().unwrap().1.title()),
        "Merge: left.txt ↔ right.txt"
    );
    view.read_with(cx, |v, _| {
        assert_eq!(v.slot(LEFT).unwrap().name, "left.txt", "the swap went back");
        let session = v.session().unwrap();
        assert_eq!(session.left().name, "left.txt");
        assert_eq!(
            session.resolution(1).map(|r| r.how),
            Some(duet_core::text::merge::session::How::Left),
            "with the choice as it was"
        );
        assert!(v.is_dirty());
        assert_eq!(
            v.notice(),
            Some("These files are already merged in another tab.")
        );
    });
}

#[gpui_kit::test]
async fn a_dirty_merge_merged_again_onto_an_open_set_is_refused(cx: &mut TestAppContext) {
    // M-68: as a swap is, a Merge ▸ of other slots is refused before it drops the choices.
    let (_dir, paths) = conflict_set();
    let swapped = MergePaths {
        left: paths.right.clone(),
        base: paths.base.clone(),
        right: paths.left.clone(),
        output: None,
    };
    let (handle, workspace) = open(cx, Launch::Merge(paths.clone()));
    wait_merge_idle(cx, handle, &workspace).await;
    let view = active_merge(&workspace, cx);
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_launch(Launch::Merge(swapped.clone()), window, cx)
        })
    })
    .unwrap();
    wait_merge_idle(cx, handle, &workspace).await;
    workspace.update(cx, |w, cx| {
        w.tabs_mut_for_test().activate(0);
        cx.notify();
    });
    choose_left(cx, handle);
    assert!(workspace.read_with(cx, |w, cx| w.tab_dirty(0, cx)));
    // The slots take the other tab's files, then Merge ▸.
    view.update(cx, |v, cx| {
        v.open_slot_path(LEFT, swapped.left.clone().unwrap(), cx);
        v.open_slot_path(RIGHT, swapped.right.clone().unwrap(), cx);
    });
    wait_merge_idle(cx, handle, &workspace).await;
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.run(false, window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        !cx.has_pending_prompt(),
        "refused before asking to drop the choices"
    );
    assert_eq!(kinds(&workspace, cx), ["merge", "merge"]);
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().iter().next().unwrap().1.title()),
        "Merge: left.txt ↔ right.txt"
    );
    view.read_with(cx, |v, _| {
        let session = v.session().unwrap();
        assert_eq!(session.left().name, "left.txt");
        assert_eq!(
            session.resolution(1).map(|r| r.how),
            Some(duet_core::text::merge::session::How::Left),
            "the choice stays"
        );
        assert_eq!(
            v.notice(),
            Some("These files are already merged in another tab.")
        );
    });
}

#[gpui_kit::test]
async fn launching_an_open_merge_with_an_output_sets_it_once(cx: &mut TestAppContext) {
    let (dir, paths) = merge_set();
    let (handle, workspace) = open(cx, Launch::Merge(paths.clone()));
    wait_merge_idle(cx, handle, &workspace).await;
    let view = active_merge(&workspace, cx);
    let (one, two) = (dir.path().join("one.txt"), dir.path().join("two.txt"));
    let launch = |cx: &mut TestAppContext, output: &PathBuf| {
        let paths = MergePaths {
            output: Some(output.clone()),
            ..paths.clone()
        };
        cx.update_window(handle, |_, window, cx| {
            workspace.update(cx, |w, cx| w.open_launch(Launch::Merge(paths), window, cx))
        })
        .unwrap();
    };
    launch(cx, &one);
    assert_eq!(kinds(&workspace, cx), ["merge"]);
    assert_eq!(
        view.read_with(cx, |v, _| v.output().map(|p| p.to_path_buf())),
        Some(one.clone()),
        "a tab with no output takes the new one"
    );
    launch(cx, &two);
    view.read_with(cx, |v, _| {
        assert_eq!(v.output(), Some(one.as_path()), "the output it has stays");
        assert_eq!(
            v.notice(),
            Some("This merge is open already; its output is unchanged.")
        );
    });
}

/// A folder tab with its pair `a.txt` open from it in a text tab, and the text tab loaded.
async fn text_from_folder(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    workspace: &Entity<Workspace>,
    folder: &Entity<FolderView>,
    (l, r): &(PathBuf, PathBuf),
) {
    folder.update(cx, |_, cx| {
        cx.emit(FolderEvent::OpenFiles {
            left: l.join("a.txt"),
            right: r.join("a.txt"),
            kind: None,
        })
    });
    let text = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected the text tab"),
    });
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !text.read(cx).is_busy()
    })
    .await;
}

fn first_folder(workspace: &Entity<Workspace>, cx: &mut TestAppContext) -> Entity<FolderView> {
    workspace.read_with(cx, |w, _| match w.tabs().iter().next() {
        Some((_, _, TabItem::Folder(view))) => view.clone(),
        _ => panic!("expected the folder tab first"),
    })
}

fn active_index(workspace: &Entity<Workspace>, cx: &mut TestAppContext) -> Option<usize> {
    workspace.read_with(cx, |w, _| w.tabs().active_index())
}

#[gpui_kit::test]
async fn a_text_tab_from_a_folder_has_a_back_button_and_closing_returns_there(
    cx: &mut TestAppContext,
) {
    let (_dir, l, r) = pair_of_dirs();
    let (handle, workspace) = open(
        cx,
        Launch::Folders {
            left: l.clone(),
            right: r.clone(),
        },
    );
    let folder = first_folder(&workspace, cx);
    // A start tab between the folder and the text tab, so a neighbour differs from the origin.
    press_key(cx, handle, "ctrl-t");
    text_from_folder(cx, handle, &workspace, &folder, &(l.clone(), r.clone())).await;
    assert_eq!(kinds(&workspace, cx), ["folder", "start", "text"]);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("text-back").is_some());
        window.click("text-back", cx);
    })
    .unwrap();
    assert_eq!(
        active_index(&workspace, cx),
        Some(0),
        "Back shows the folder"
    );
    // Opened again from the folder: the same tab, and still with its origin.
    text_from_folder(cx, handle, &workspace, &folder, &(l, r)).await;
    assert_eq!(kinds(&workspace, cx), ["folder", "start", "text"]);
    assert_eq!(active_index(&workspace, cx), Some(2));
    press_key(cx, handle, "ctrl-w");
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["folder", "start"]);
    assert_eq!(
        active_index(&workspace, cx),
        Some(0),
        "closing the text tab returns to its folder, not to the neighbour"
    );
}

#[gpui_kit::test]
async fn a_text_tab_from_the_start_screen_has_no_back_button(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "x\n").unwrap();
    std::fs::write(&b, "y\n").unwrap();
    let (handle, workspace) = open(cx, Launch::Files { left: a, right: b });
    let text = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected the text tab"),
    });
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !text.read(cx).is_busy()
    })
    .await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("text-back").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
async fn closing_an_inactive_tab_from_a_folder_keeps_the_active_tab(cx: &mut TestAppContext) {
    let (_dir, l, r) = pair_of_dirs();
    let (handle, workspace) = open(
        cx,
        Launch::Folders {
            left: l.clone(),
            right: r.clone(),
        },
    );
    let folder = first_folder(&workspace, cx);
    text_from_folder(cx, handle, &workspace, &folder, &(l, r)).await;
    press_key(cx, handle, "ctrl-t");
    assert_eq!(kinds(&workspace, cx), ["folder", "text", "start"]);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let position = window.find(("tab", 1usize)).bounds().center();
        window.dispatch_event(
            gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                button: gpui_kit::MouseButton::Middle,
                position,
                modifiers: gpui_kit::Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["folder", "start"]);
    assert_eq!(
        active_index(&workspace, cx),
        Some(1),
        "the start tab stays active; the folder is not pulled forward"
    );
}

fn click_compare_text_card(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("start-mode-text").is_some());
        window.click("start-mode-text", cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
async fn the_compare_text_card_opens_one_untitled_tab(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (handle, workspace) = open(cx, Launch::Start);
    let start = active_start(&workspace, cx);
    click_compare_text_card(cx, handle);
    assert_eq!(kinds(&workspace, cx), ["start", "text"]);
    let title = |cx: &mut TestAppContext| {
        workspace.read_with(cx, |w, _| w.tabs().iter().nth(1).unwrap().1.title())
    };
    assert_eq!(title(cx), "untitled ↔ untitled");
    assert!(
        start.read_with(cx, |s, _| s.recent().to_vec()).is_empty(),
        "an untitled pair never enters the recent list"
    );
    // A second click comes back to the same tab.
    workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
    click_compare_text_card(cx, handle);
    assert_eq!(kinds(&workspace, cx), ["start", "text"]);
    assert_eq!(active_index(&workspace, cx), Some(1));
    // A file opened into the left side re-keys the tab.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("x.txt");
    std::fs::write(&file, "x\n").unwrap();
    wait_text_loaded(cx, &workspace).await;
    let view = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected the text tab"),
    });
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.open_path(true, file.clone(), window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(title(cx), "x.txt ↔ untitled");
    assert!(
        start.read_with(cx, |s, _| s.recent().to_vec()).is_empty(),
        "a pair with an untitled side stays out of the recent list"
    );
}

#[gpui_kit::test]
async fn closing_an_untitled_tab_with_typed_text_asks_first(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx, Launch::Start);
    click_compare_text_card(cx, handle);
    wait_text_loaded(cx, &workspace).await;
    type_into_active_text(cx, handle, &workspace, "z");
    press_key(cx, handle, "ctrl-w");
    let (message, _) = cx.pending_prompt().expect("the untitled tab asks first");
    assert_eq!(message, "Save changes to untitled?");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start", "text"]);
    press_key(cx, handle, "ctrl-w");
    cx.simulate_prompt_answer("Discard");
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start"]);
}

/// I2: Save in the open prompt gives the untitled side a file; the pair then opens with it.
#[gpui_kit::test]
async fn opening_a_file_into_an_untitled_pair_after_save_keeps_the_saved_side(
    cx: &mut TestAppContext,
) {
    let (handle, workspace) = open(cx, Launch::Start);
    click_compare_text_card(cx, handle);
    wait_text_loaded(cx, &workspace).await;
    type_into_active_text(cx, handle, &workspace, "x");
    let view = active_text(&workspace, cx);
    let dir = tempfile::tempdir().unwrap();
    let (saved, file) = (dir.path().join("x.txt"), dir.path().join("file.txt"));
    std::fs::write(&file, "y\n").unwrap();
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.open_path(false, file.clone(), window, cx))
    })
    .unwrap();
    cx.simulate_prompt_answer("Save");
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path(), "the untitled side asks where");
    let chosen = saved.clone();
    cx.simulate_new_path_selection(move |_| Some(chosen));
    let (v, seen, right) = (view.clone(), saved.clone(), file.clone());
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        let v = v.read(cx);
        seen.exists() && v.request().right == right && !v.is_busy()
    })
    .await;
    assert_eq!(active_key(&workspace, cx).title(), "x.txt ↔ file.txt");
    assert_eq!(std::fs::read_to_string(&saved).unwrap(), "x");
    view.read_with(cx, |v, _| {
        assert!(!v.is_untitled(true), "the saved side shows its file");
        assert_eq!(v.request().left, saved);
        let model = v.model().expect("the pair compares");
        assert_eq!(model.line_text(true, 0), "x");
        assert_eq!(model.line_text(false, 0), "y");
    });
}

/// M6: a text tab whose folder tab closed has no Back button left.
#[gpui_kit::test]
async fn closing_the_folder_tab_takes_back_from_its_text_tabs(cx: &mut TestAppContext) {
    let (_dir, l, r) = pair_of_dirs();
    let (handle, workspace) = open(
        cx,
        Launch::Folders {
            left: l.clone(),
            right: r.clone(),
        },
    );
    let folder = first_folder(&workspace, cx);
    text_from_folder(cx, handle, &workspace, &folder, &(l, r)).await;
    let text = active_text(&workspace, cx);
    assert!(text.read_with(cx, |v, _| v.has_origin()));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("tab-close", 0usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["text"]);
    assert!(!text.read_with(cx, |v, _| v.has_origin()));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("text-back").is_none());
    })
    .unwrap();
}

/// D6 (final review M10): closing the window asks a dirty untitled tab first, whose Save asks
/// where, and then a dirty merge tab.
#[gpui_kit::test]
async fn closing_the_window_asks_for_an_untitled_tab_and_a_merge_tab_in_turn(
    cx: &mut TestAppContext,
) {
    let (set, paths) = conflict_set();
    let (handle, workspace) = open(cx, Launch::Start);
    click_compare_text_card(cx, handle);
    wait_text_loaded(cx, &workspace).await;
    type_into_active_text(cx, handle, &workspace, "z");
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_launch(Launch::Merge(paths.clone()), window, cx)
        })
    })
    .unwrap();
    wait_merge_idle(cx, handle, &workspace).await;
    choose_left(cx, handle);
    let close = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            workspace.update(cx, |w, cx| w.confirm_close_all(window, cx))
        })
        .unwrap()
    };

    let task = close(cx);
    let (message, _) = cx.pending_prompt().expect("the untitled tab asks first");
    assert_eq!(message, "Save changes to untitled?");
    cx.simulate_prompt_answer("Save");
    cx.run_until_parked();
    assert!(
        cx.did_prompt_for_new_path(),
        "Save asks where the side goes"
    );
    let saved = set.path().join("typed.txt");
    let chosen = saved.clone();
    cx.simulate_new_path_selection(move |_| Some(chosen));
    cx.run_until_parked();
    let (message, _) = cx.pending_prompt().expect("then the merge tab");
    assert_eq!(message, "Save the merge result?");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(!task.await, "Cancel keeps the window open");
    assert_eq!(std::fs::read_to_string(&saved).unwrap(), "z");
    assert_eq!(kinds(&workspace, cx), ["start", "text", "merge"]);

    let task = close(cx);
    let (message, _) = cx
        .pending_prompt()
        .expect("only the merge tab is left to ask");
    assert_eq!(message, "Save the merge result?");
    cx.simulate_prompt_answer("Discard");
    cx.run_until_parked();
    assert!(task.await);
}

// ---- U12–U15: the Start page ----

/// A data folder whose settings.json holds `settings`, a JSON object.
fn data_with_settings(cx: &mut TestAppContext, settings: &str) -> tempfile::TempDir {
    let data = tempfile::tempdir().unwrap();
    std::fs::write(data.path().join("settings.json"), settings).unwrap();
    with_data_dir(cx, data.path());
    data
}

fn start_views(
    workspace: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> Vec<Entity<duet_app::start::StartView>> {
    workspace.read_with(cx, |w, _| {
        w.tabs()
            .iter()
            .filter_map(|(_, _, item)| match item {
                TabItem::Start(view) => Some(view.clone()),
                _ => None,
            })
            .collect()
    })
}

/// Draws the window, then whether it shows `id`.
fn shows(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
    .unwrap()
}

fn label_of(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    id: impl Into<gpui_kit::ElementId>,
) -> Option<String> {
    let id = id.into();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find(id).label().map(str::to_owned)
    })
    .unwrap()
}

#[gpui_kit::test]
fn the_welcome_card_shows_until_dismissed_and_then_on_no_start_tab(cx: &mut TestAppContext) {
    let _th = thai_ui();
    use duet_app::help::topics::HelpLang;
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let settings = data.path().join("settings.json");
    let (handle, workspace) = open(cx, Launch::Start);
    press_key(cx, handle, "ctrl-t");
    assert_eq!(kinds(&workspace, cx), ["start", "start"]);
    assert!(shows(cx, handle, "start-welcome"), "no key: it shows");
    for start in start_views(&workspace, cx) {
        assert_eq!(
            start.read_with(cx, |s, _| s.welcome()),
            Some(HelpLang::Th),
            "in Thai, the help's language with no setting"
        );
    }
    // The welcome leads the left column, above the form.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let (card, form) = (
            window.find("start-welcome").bounds(),
            window.find("start-form").bounds(),
        );
        assert!(card.bottom() <= form.top(), "{card:?} above {form:?}");
        window.click("start-welcome-close", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        duet_app::settings::get_u64(&settings, "start.welcome"),
        Some(1),
        "the choice is kept"
    );
    for start in start_views(&workspace, cx) {
        assert_eq!(start.read_with(cx, |s, _| s.welcome()), None);
    }
    assert!(!shows(cx, handle, "start-welcome"));
    workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
    assert!(
        !shows(cx, handle, "start-welcome"),
        "the other Start tab hid it too"
    );
    press_key(cx, handle, "ctrl-t");
    assert!(!shows(cx, handle, "start-welcome"), "and a new one");
}

#[gpui_kit::test]
fn the_welcome_card_stays_hidden_once_seen(cx: &mut TestAppContext) {
    let _data = data_with_settings(cx, r#"{ "start.welcome": 1 }"#);
    let (handle, workspace) = open(cx, Launch::Start);
    assert!(!shows(cx, handle, "start-welcome"));
    assert_eq!(
        active_start(&workspace, cx).read_with(cx, |s, _| s.welcome()),
        None
    );
    assert!(shows(cx, handle, "start-form"), "the form leads instead");
}

#[gpui_kit::test]
fn the_welcome_card_speaks_the_help_language(cx: &mut TestAppContext) {
    use duet_app::help::topics::HelpLang;
    let _data = data_with_settings(cx, r#"{ "help.lang": "en" }"#);
    let (handle, workspace) = open(cx, Launch::Start);
    assert_eq!(
        label_of(cx, handle, "start-welcome-title").as_deref(),
        Some("Welcome to duet")
    );
    assert_eq!(
        label_of(cx, handle, "start-welcome-help").as_deref(),
        Some("Open the guide")
    );
    assert_eq!(
        label_of(cx, handle, "start-welcome-close").as_deref(),
        Some("Don't show again")
    );
    // Thai in the Help tab: the Start tab follows when it is shown again.
    press_key(cx, handle, "f1");
    assert_eq!(kinds(&workspace, cx), ["start", "help"]);
    let help = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Help(view)) => view.clone(),
        _ => panic!("expected the help tab"),
    });
    cx.update_window(handle, |_, window, cx| {
        help.update(cx, |h, cx| h.set_lang(HelpLang::Th, window, cx))
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("tab", 0usize), cx);
    })
    .unwrap();
    assert_eq!(
        label_of(cx, handle, "start-welcome-title").as_deref(),
        Some("ยินดีต้อนรับสู่ duet")
    );
    assert_eq!(
        label_of(cx, handle, "start-welcome-help").as_deref(),
        Some("เปิดคู่มือ")
    );
    assert_eq!(
        label_of(cx, handle, "start-welcome-close").as_deref(),
        Some("ไม่ต้องแสดงอีก")
    );
}

#[gpui_kit::test]
fn the_welcome_guide_button_opens_the_help_and_hides_the_card(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (handle, workspace) = open(cx, Launch::Start);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("start-welcome-help", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start", "help"]);
    assert_eq!(active_index(&workspace, cx), Some(1));
    assert_eq!(
        duet_app::settings::get_u64(&data.path().join("settings.json"), "start.welcome"),
        Some(1)
    );
    workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
    assert!(!shows(cx, handle, "start-welcome"));
}

#[gpui_kit::test]
fn the_start_sheet_has_eight_rows_and_links_to_the_keys_topic(cx: &mut TestAppContext) {
    let _data = data_with_settings(cx, r#"{ "start.welcome": 1 }"#);
    let (handle, workspace) = open(cx, Launch::Start);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for row in 0..8usize {
            assert!(
                window.try_find(("start-shortcut", row)).is_some(),
                "row {row}"
            );
        }
        assert!(window.try_find(("start-shortcut", 8usize)).is_none());
        assert_eq!(
            window.find("start-shortcuts-help").label(),
            Some("All shortcuts in Help (F1)")
        );
        window.click("start-shortcuts-help", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start", "help"]);
    let topic = workspace.read_with(cx, |w, cx| match w.tabs().active() {
        Some(TabItem::Help(view)) => view.read(cx).current(),
        _ => panic!("expected the help tab"),
    });
    assert_eq!(topic, Some("keys"));
}

#[gpui_kit::test]
fn paste_text_sits_in_the_form_and_six_tiles_line_up(cx: &mut TestAppContext) {
    let _data = data_with_settings(cx, r#"{ "start.welcome": 1 }"#);
    let (handle, _workspace) = open(cx, Launch::Start);
    let form = gpui_kit::ElementId::from("start-form");
    let tiles = gpui_kit::ElementId::from("start-tiles");
    let bounds = |window: &mut Window, id: &'static str| window.find(id).bounds();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let paste = window.find("start-mode-text");
        assert_eq!(paste.label(), Some("Paste text…"));
        assert!(
            paste.path().contains(&form),
            "in the form: {:?}",
            paste.path()
        );
        let (paste, compare) = (paste.bounds(), bounds(window, "start-compare"));
        assert!(
            paste.right() <= compare.left()
                && (paste.center().y - compare.center().y).abs() < px(1.),
            "Paste text… before Compare: {paste:?} {compare:?}"
        );
        assert!(window.try_find("start-drop-hint").is_some());
        for id in [
            "start-mode-files",
            "start-mode-folders",
            "start-mode-merge",
            "start-mode-hex",
            "start-mode-table",
            "start-mode-picture",
        ] {
            assert!(window.find(id).path().contains(&tiles), "{id} is a tile");
        }
        // 1200 px: a 612 px left column, so two columns of tiles, each row stretched.
        let (files, folders, merge) = (
            bounds(window, "start-mode-files"),
            bounds(window, "start-mode-folders"),
            bounds(window, "start-mode-merge"),
        );
        assert_eq!(
            (files.top(), files.size.height),
            (folders.top(), folders.size.height)
        );
        assert!(merge.top() > files.bottom(), "the third tile starts row 2");
    })
    .unwrap();
    cx.simulate_window_resize(handle, size(px(1400.), px(800.)));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let row = |a: &'static str, b: &'static str, c: &'static str| {
            let [a, b, c] = [a, b, c].map(|id| window.find(id).bounds());
            assert!(
                a.top() == b.top() && b.top() == c.top(),
                "one row: {a:?} {b:?} {c:?}"
            );
            assert!(
                a.size.height == b.size.height && b.size.height == c.size.height,
                "equal heights, not centred: {a:?} {b:?} {c:?}"
            );
        };
        row("start-mode-files", "start-mode-folders", "start-mode-merge");
        row("start-mode-hex", "start-mode-table", "start-mode-picture");
    })
    .unwrap();
}

#[gpui_kit::test]
fn recent_entries_are_one_row_of_names_and_their_folder(cx: &mut TestAppContext) {
    let data = data_with_settings(cx, r#"{ "start.welcome": 1 }"#);
    let lines = "T\tC:\\work\\a\\left.txt\tC:\\work\\a\\right.txt\n\
                 F\tC:\\one\\x\tD:\\two\\y\n";
    std::fs::write(data.path().join("recent.txt"), lines).unwrap();
    let (handle, _workspace) = open(cx, Launch::Start);
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let entry = window.find(("start-recent", 0usize));
        assert_eq!(
            entry.label(),
            Some("Files: C:\\work\\a\\left.txt and C:\\work\\a\\right.txt"),
            "the accessible name is kept"
        );
        assert!(
            entry.bounds().size.height <= px(34.),
            "one row: {:?}",
            entry.bounds()
        );
        let names = window.find(("start-recent-names", 0usize));
        let place = window.find(("start-recent-place", 0usize));
        assert_eq!(names.label(), Some("left.txt ↔ right.txt"));
        assert_eq!(place.label(), Some("C:\\work\\a"));
        let (n, p) = (names.bounds(), place.bounds());
        assert!(
            p.left() >= n.right() && (p.center().y - n.center().y).abs() < px(1.),
            "the folder follows the names on their row: {n:?} {p:?}"
        );
        assert_eq!(
            window.find(("start-recent-place", 1usize)).label(),
            Some("C:\\one · D:\\two")
        );
        assert!(
            window.try_find(("start-recent-path", 0usize)).is_none(),
            "no two-line paths"
        );
        window.hover(("start-recent", 0usize), cx);
    })
    .unwrap();
    // The tooltip holds the full paths, one per line.
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let tip = window.find("tooltip-text").bounds();
        assert!(tip.size.height > px(30.), "two lines: {tip:?}");
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_recent_list_shows_eight_and_toggles_the_rest(cx: &mut TestAppContext) {
    let data = data_with_settings(cx, r#"{ "start.welcome": 1 }"#);
    let lines: String = (0..10)
        .map(|i| format!("F\tC:\\l{i}\tC:\\r{i}\n"))
        .collect();
    std::fs::write(data.path().join("recent.txt"), lines).unwrap();
    let (handle, workspace) = open(cx, Launch::Start);
    cx.run_until_parked();
    let listed = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            (0..12usize)
                .filter(|&i| window.try_find(("start-recent", i)).is_some())
                .count()
        })
        .unwrap()
    };
    assert_eq!(listed(cx), 8);
    assert_eq!(
        label_of(cx, handle, "start-recent-more").as_deref(),
        Some("Show all (10)")
    );
    cx.update_window(handle, |_, window, cx| {
        window.click("start-recent-more", cx)
    })
    .unwrap();
    assert_eq!(listed(cx), 10);
    assert_eq!(
        label_of(cx, handle, "start-recent-more").as_deref(),
        Some("Show fewer")
    );
    cx.update_window(handle, |_, window, cx| {
        window.click("start-recent-more", cx)
    })
    .unwrap();
    assert_eq!(listed(cx), 8);
    let start = active_start(&workspace, cx);
    start.update(cx, |s, cx| {
        s.remove_recent(0, cx);
        s.remove_recent(0, cx);
    });
    assert_eq!(listed(cx), 8);
    assert!(
        !shows(cx, handle, "start-recent-more"),
        "eight need no toggle"
    );
}

#[gpui_kit::test]
fn an_empty_recent_list_says_so_beside_a_clock(cx: &mut TestAppContext) {
    let (handle, _workspace) = open(cx, Launch::Start);
    assert_eq!(
        label_of(cx, handle, "start-recent-empty").as_deref(),
        Some("Compared pairs show up here.")
    );
}

/// The header draws the app's own icon, at a size that maps it 1:1 onto device pixels, not a
/// text glyph that a font decides the look of.
#[gpui_kit::test]
fn the_header_draws_the_app_icon(cx: &mut TestAppContext) {
    use duet_app::ui::brand::{brand_frame, brand_size};
    assert_eq!(
        [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0].map(brand_frame),
        [26, 32, 40, 46, 52, 64, 78]
    );
    assert_eq!(brand_size(1.25), px(25.6));
    let (handle, _workspace) = open(cx, Launch::Start);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("header-brand-icon").bounds().size.width,
            px(26.)
        );
    })
    .unwrap();
}

fn start_view(
    workspace: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> Entity<duet_app::start::StartView> {
    workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Start(view)) => view.clone(),
        _ => panic!("expected the start tab"),
    })
}

/// `--compare-to` with nothing stored: one Start tab, its left box already holding the path. The
/// boxes are read straight after opening, with no extra draw: the real window gets none either.
#[gpui_kit::test]
fn compare_to_without_a_stored_left_fills_the_left_box(cx: &mut TestAppContext) {
    let left = "C:\\ไทย folder\\a.txt";
    let (_handle, workspace) = open(
        cx,
        Launch::StartWithLeft {
            left: left.into(),
            notice: None,
        },
    );
    assert_eq!(kinds(&workspace, cx), ["start"]);
    let start = start_view(&workspace, cx);
    start.read_with(cx, |v, cx| {
        assert_eq!(v.left_input().read(cx).value().as_ref(), left);
        assert_eq!(v.right_input().read(cx).value().as_ref(), "");
        assert_eq!(v.error(), None);
    });
}

#[gpui_kit::test]
fn a_vanished_stored_left_shows_its_name_in_the_first_frame(cx: &mut TestAppContext) {
    let notice = "C:\\gone\\old.txt: no longer exists";
    let (handle, workspace) = open(
        cx,
        Launch::StartWithLeft {
            left: "C:\\now\\here.txt".into(),
            notice: Some(notice.into()),
        },
    );
    let start = start_view(&workspace, cx);
    start.read_with(cx, |v, cx| {
        assert!(v.error().is_some_and(|e| e.contains("C:\\gone\\old.txt")));
        assert_eq!(
            v.left_input().read(cx).value().as_ref(),
            "C:\\now\\here.txt"
        );
    });
    // No render_frame: this is the frame open_window itself painted.
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("start-error").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
async fn a_dirty_merge_swapped_onto_an_open_set_swaps_back_says_so_in_thai(
    cx: &mut TestAppContext,
) {
    let _th = duet_app::i18n::scoped(duet_app::i18n::Lang::Th);
    let (_dir, paths) = conflict_set();
    let swapped = MergePaths {
        left: paths.right.clone(),
        base: paths.base.clone(),
        right: paths.left.clone(),
        output: None,
    };
    let (handle, workspace) = open(cx, Launch::Merge(paths.clone()));
    wait_merge_idle(cx, handle, &workspace).await;
    let view = active_merge(&workspace, cx);
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_launch(Launch::Merge(swapped), window, cx)
        })
    })
    .unwrap();
    wait_merge_idle(cx, handle, &workspace).await;
    workspace.update(cx, |w, cx| {
        w.tabs_mut_for_test().activate(0);
        cx.notify();
    });
    choose_left(cx, handle);
    assert!(workspace.read_with(cx, |w, cx| w.tab_dirty(0, cx)));
    cx.update_window(handle, |_, window, cx| {
        window.focus(&view.focus_handle(cx), cx);
        window.press("f9", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["merge", "merge"]);
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().iter().next().unwrap().1.title()),
        format!("{}: left.txt ↔ right.txt", duet_app::i18n::tr("Merge"))
    );
    view.read_with(cx, |v, _| {
        assert_eq!(v.slot(LEFT).unwrap().name, "left.txt", "the swap went back");
        let session = v.session().unwrap();
        assert_eq!(session.left().name, "left.txt");
        assert_eq!(
            session.resolution(1).map(|r| r.how),
            Some(duet_core::text::merge::session::How::Left),
            "with the choice as it was"
        );
        assert!(v.is_dirty());
        assert_eq!(
            v.notice(),
            Some(duet_app::i18n::tr(
                "These files are already merged in another tab."
            ))
        );
    });
}

#[gpui_kit::test]
async fn a_dirty_merge_merged_again_onto_an_open_set_is_refused_says_so_in_thai(
    cx: &mut TestAppContext,
) {
    // M-68: as a swap is, a Merge ▸ of other slots is refused before it drops the choices.
    let (_dir, paths) = conflict_set();
    let swapped = MergePaths {
        left: paths.right.clone(),
        base: paths.base.clone(),
        right: paths.left.clone(),
        output: None,
    };
    let (handle, workspace) = open(cx, Launch::Merge(paths.clone()));
    wait_merge_idle(cx, handle, &workspace).await;
    let view = active_merge(&workspace, cx);
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_launch(Launch::Merge(swapped.clone()), window, cx)
        })
    })
    .unwrap();
    wait_merge_idle(cx, handle, &workspace).await;
    workspace.update(cx, |w, cx| {
        w.tabs_mut_for_test().activate(0);
        cx.notify();
    });
    choose_left(cx, handle);
    assert!(workspace.read_with(cx, |w, cx| w.tab_dirty(0, cx)));
    // The slots take the other tab's files, then Merge ▸.
    view.update(cx, |v, cx| {
        v.open_slot_path(LEFT, swapped.left.clone().unwrap(), cx);
        v.open_slot_path(RIGHT, swapped.right.clone().unwrap(), cx);
    });
    wait_merge_idle(cx, handle, &workspace).await;
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.run(false, window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        !cx.has_pending_prompt(),
        "refused before asking to drop the choices"
    );
    assert_eq!(kinds(&workspace, cx), ["merge", "merge"]);
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().iter().next().unwrap().1.title()),
        format!("{}: left.txt ↔ right.txt", duet_app::i18n::tr("Merge"))
    );
    view.read_with(cx, |v, _| {
        let session = v.session().unwrap();
        assert_eq!(session.left().name, "left.txt");
        assert_eq!(
            session.resolution(1).map(|r| r.how),
            Some(duet_core::text::merge::session::How::Left),
            "the choice stays"
        );
        assert_eq!(
            v.notice(),
            Some(duet_app::i18n::tr(
                "These files are already merged in another tab."
            ))
        );
    });
}

/// The binary-files card, and a recent entry of another kind, say what to open: two workbooks are
/// not a table then.
#[gpui_kit::test]
async fn a_forced_kind_beats_a_workbook_pair(cx: &mut TestAppContext) {
    use duet_core::table::xlsx::fixture::{SheetSpec, workbook};
    let files = tempfile::tempdir().unwrap();
    let (a, b) = (files.path().join("a.xlsx"), files.path().join("b.xlsx"));
    let book = workbook(&[SheetSpec::text_rows("S", &[&["id"], &["1"]])]);
    std::fs::write(&a, &book).unwrap();
    std::fs::write(&b, &book).unwrap();

    let (handle, workspace) = open(cx, Launch::Start);
    let start = active_start(&workspace, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("start-mode-hex", cx);
    })
    .unwrap();
    let picked = a.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![picked]));
    cx.run_until_parked();
    let (a_text, b_text) = (a.display().to_string(), b.display().to_string());
    cx.update_window(handle, |_, window, cx| {
        start.update(cx, |view, cx| {
            view.set_paths(&a_text, &b_text, window, cx);
            view.compare(cx);
        })
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["start", "hex"]);
}

#[gpui_kit::test]
async fn a_recent_entry_of_another_kind_keeps_its_kind_on_a_workbook_pair(cx: &mut TestAppContext) {
    use duet_app::recent::{self, Recent, RecentKind};
    use duet_core::table::xlsx::fixture::{SheetSpec, workbook};
    for (kind, want) in [(RecentKind::Hex, "hex"), (RecentKind::Picture, "picture")] {
        let data = tempfile::tempdir().unwrap();
        with_data_dir(cx, data.path());
        let files = tempfile::tempdir().unwrap();
        let (a, b) = (files.path().join("a.xlsx"), files.path().join("b.xlsx"));
        let book = workbook(&[SheetSpec::text_rows("S", &[&["id"], &["1"]])]);
        std::fs::write(&a, &book).unwrap();
        std::fs::write(&b, &book).unwrap();
        recent::record(
            &data.path().join("recent.txt"),
            Recent {
                kind,
                left: a,
                right: b,
                base: None,
            },
        )
        .unwrap();
        let (_handle, workspace) = open(cx, Launch::Start);
        cx.run_until_parked();
        let start = workspace.read_with(cx, |w, _| match w.tabs().active() {
            Some(TabItem::Start(view)) => view.clone(),
            _ => panic!("expected the start tab"),
        });
        start.update(cx, |s, cx| s.open_recent(0, cx));
        cx.run_until_parked();
        assert_eq!(kinds(&workspace, cx), ["start", want]);
    }
}

// ---- The mode bar in the header ----

fn click_mode(cx: &mut TestAppContext, handle: AnyWindowHandle, mode: Mode) {
    let index = Mode::ALL.iter().position(|m| *m == mode).unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("header-mode", index), cx);
    })
    .unwrap();
}

fn active_mode(workspace: &Entity<Workspace>, cx: &mut TestAppContext) -> Option<Mode> {
    workspace.read_with(cx, |w, _| w.tabs().active_mode())
}

#[gpui_kit::test]
fn the_mode_bar_goes_to_the_tab_of_its_kind_used_last(cx: &mut TestAppContext) {
    let (dir, l, r) = pair_of_dirs();
    let csv = |name: &str| {
        let path = dir.path().join(name);
        std::fs::write(&path, "a\n1\n").unwrap();
        path
    };
    let (a, b, c, d) = (csv("a.csv"), csv("b.csv"), csv("c.csv"), csv("d.csv"));
    let (handle, workspace) = open(cx, Launch::Folders { left: l, right: r });
    for (left, right) in [(a, b), (c, d)] {
        cx.update_window(handle, |_, window, cx| {
            workspace.update(cx, |w, cx| {
                w.open_launch(Launch::Table { left, right }, window, cx)
            });
        })
        .unwrap();
    }
    workspace.update(cx, |w, _| {
        w.tabs_mut_for_test().activate(1);
        w.tabs_mut_for_test().activate(0);
    });
    assert_eq!(active_mode(&workspace, cx), Some(Mode::Folders));

    click_mode(cx, handle, Mode::Table);
    assert_eq!(
        active_index(&workspace, cx),
        Some(1),
        "the table used last, not the last table in the strip"
    );
    assert_eq!(active_mode(&workspace, cx), Some(Mode::Table));

    click_mode(cx, handle, Mode::Folders);
    assert_eq!(active_index(&workspace, cx), Some(0));
    assert_eq!(
        kinds(&workspace, cx),
        ["folder", "table", "table"],
        "going to a tab opens none"
    );
}

#[gpui_kit::test]
fn the_mode_bar_starts_a_compare_of_a_kind_with_no_tab(cx: &mut TestAppContext) {
    let (_dir, l, r) = pair_of_dirs();
    let (handle, workspace) = open(cx, Launch::Folders { left: l, right: r });

    click_mode(cx, handle, Mode::Table);
    cx.run_until_parked();
    assert_eq!(kinds(&workspace, cx), ["folder", "start"]);
    assert_eq!(active_mode(&workspace, cx), Some(Mode::Start));
    assert!(cx.did_prompt_for_paths(), "the picker of its Start tile");
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();

    click_mode(cx, handle, Mode::Hex);
    cx.run_until_parked();
    assert_eq!(
        kinds(&workspace, cx),
        ["folder", "start"],
        "another kind uses the same Start tab"
    );
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();

    click_mode(cx, handle, Mode::Merge);
    cx.run_until_parked();
    assert_eq!(
        kinds(&workspace, cx),
        ["folder", "start", "merge"],
        "as its tile, a merge opens empty"
    );
    assert!(!cx.did_prompt_for_paths(), "and asks for nothing");

    click_mode(cx, handle, Mode::Start);
    cx.run_until_parked();
    assert_eq!(active_mode(&workspace, cx), Some(Mode::Start));
    assert!(!cx.did_prompt_for_paths(), "Start is only a place to go");
}

#[gpui_kit::test]
fn the_help_tab_has_no_mode_and_the_bar_leaves_it(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx, Launch::Start);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("header-help", cx);
    })
    .unwrap();
    assert_eq!(kinds(&workspace, cx), ["start", "help"]);
    assert_eq!(active_mode(&workspace, cx), None);
    click_mode(cx, handle, Mode::Start);
    assert_eq!(active_mode(&workspace, cx), Some(Mode::Start));
    assert_eq!(kinds(&workspace, cx), ["start", "help"]);
}
