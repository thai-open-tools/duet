//! Headless UI tests for the folder view (gpui-kit test harness).

use std::cell::RefCell;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, UNIX_EPOCH};

use duet_app::folder::model::FolderFilter;
use duet_app::folder::view::{FolderEvent, FolderView};
use duet_core::folder::run::FolderOptions;
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, Focusable, Point, Render, TestAppContext,
    Window, WindowBounds, WindowOptions, px, size,
};

fn open_test_window<V: Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        duet_app::app::configure_theme(gpui_kit::component::ThemeMode::Light, cx);
        cx.bind_keys(duet_app::folder::view::key_bindings());
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

fn put(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        .unwrap();
}

/// left: a.txt (same), b.txt (different), sub/c.txt (left only); right: a.txt, b.txt, new.txt.
fn fixture() -> (tempfile::TempDir, tempfile::TempDir) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("a.txt"), "same");
    put(&r.path().join("a.txt"), "same");
    put(&l.path().join("b.txt"), "left");
    put(&r.path().join("b.txt"), "right side");
    put(&l.path().join("sub/c.txt"), "only");
    put(&r.path().join("new.txt"), "new");
    (l, r)
}

fn open_folder(
    cx: &mut TestAppContext,
    l: &Path,
    r: &Path,
) -> (AnyWindowHandle, Entity<FolderView>) {
    let (left, right) = (l.to_path_buf(), r.to_path_buf());
    open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| FolderView::new(left, right, FolderOptions::default(), window, cx));
        window.focus(&view.focus_handle(cx), cx);
        // No test may reach the real Recycle Bin: the default stand-in deletes in place.
        view.update(cx, |v, _| v.set_trash_for_test(remove_in_place));
        view
    })
}

fn remove_in_place(path: &Path) -> Result<(), duet_core::folder::actions::ActionError> {
    let gone = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    gone.map_err(|e| duet_core::folder::actions::ActionError::Trash {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

async fn wait_idle(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<FolderView>) {
    let view = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        let v = view.read(cx);
        v.model().is_some() && !v.is_busy()
    })
    .await;
}

fn rels(view: &Entity<FolderView>, cx: &mut TestAppContext) -> Vec<String> {
    view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (0..m.visible_len())
            .map(|row| m.node_at(row).rel.clone())
            .collect()
    })
}

#[gpui_kit::test]
async fn folder_view_loads_and_shows_mismatches(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    assert_eq!(rels(&view, cx), ["sub", "b.txt", "new.txt"]);
    let status = view.read_with(cx, |v, _| v.status().to_owned());
    assert!(
        status.contains("Different 1") && status.contains("Showing: Mismatches"),
        "{status}"
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("folder-row", 0usize)).is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
async fn keys_navigate_differences_and_enter_opens_the_pair(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let opened: Rc<RefCell<Vec<(PathBuf, PathBuf)>>> = Rc::default();
    let sink = opened.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &FolderEvent, _| {
            if let FolderEvent::OpenFiles { left, right, .. } = event {
                sink.borrow_mut().push((left.clone(), right.clone()));
            }
        })
        .detach()
    });
    let selected = |cx: &mut TestAppContext| {
        view.read_with(cx, |v, _| {
            v.model().unwrap().selected_node().map(|n| n.rel.clone())
        })
    };
    cx.update_window(handle, |_, window, cx| window.press("f8", cx))
        .unwrap();
    assert_eq!(
        selected(cx).as_deref(),
        Some("sub"),
        "the orphan folder is the first stop"
    );
    cx.update_window(handle, |_, window, cx| window.press("f8", cx))
        .unwrap();
    assert_eq!(selected(cx).as_deref(), Some("b.txt"));
    cx.update_window(handle, |_, window, cx| window.press("enter", cx))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        opened.borrow().as_slice(),
        &[(l.path().join("b.txt"), r.path().join("b.txt"))]
    );
    cx.update_window(handle, |_, window, cx| window.press("alt-up", cx))
        .unwrap();
    assert_eq!(selected(cx).as_deref(), Some("sub"));
}

#[gpui_kit::test]
async fn a_late_kind_check_is_dropped_after_anything_newer(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for (name, left, right) in [("b.txt", "left", "right side"), ("d.txt", "one", "two!")] {
        put(&l.path().join(name), left);
        put(&r.path().join(name), right);
    }
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let opened: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
    let sink = opened.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &FolderEvent, _| {
            if let FolderEvent::OpenFiles { left, .. } = event {
                sink.borrow_mut().push(left.clone());
            }
        })
        .detach()
    });
    let row = |name: &str, cx: &mut TestAppContext| {
        rels(&view, cx).iter().position(|rel| rel == name).unwrap()
    };
    let enter_on = |name: &str, cx: &mut TestAppContext| {
        let at = row(name, cx);
        view.update(cx, |v, cx| v.select_row_for_test(at, cx));
        cx.update_window(handle, |_, window, cx| window.press("enter", cx))
            .unwrap();
    };
    let notice = |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.notice().map(str::to_owned));

    // While the check runs the status says so; the answer opens the pair.
    enter_on("b.txt", cx);
    assert_eq!(notice(cx).as_deref(), Some("Checking the file type…"));
    cx.run_until_parked();
    assert_eq!(opened.take(), [l.path().join("b.txt")]);
    assert_eq!(notice(cx), None);

    // Another row selected before the answer: dropped, and the notice goes.
    enter_on("b.txt", cx);
    let d_row = row("d.txt", cx);
    view.update(cx, |v, cx| v.select_row_for_test(d_row, cx));
    cx.run_until_parked();
    assert!(opened.borrow().is_empty(), "a changed selection drops it");
    assert_eq!(notice(cx), None);

    // F5, F9 and new roots before the answer: dropped.
    for key in ["f5", "f9", "f9"] {
        enter_on("b.txt", cx);
        cx.update_window(handle, |_, window, cx| window.press(key, cx))
            .unwrap();
        cx.run_until_parked();
        wait_idle(cx, handle, &view).await;
        assert!(opened.borrow().is_empty(), "{key} drops it");
        assert_eq!(notice(cx), None, "{key}");
    }
    enter_on("b.txt", cx);
    let (left, right) = (l.path().to_path_buf(), r.path().to_path_buf());
    view.update(cx, |v, cx| v.set_roots(left, right, cx));
    cx.run_until_parked();
    wait_idle(cx, handle, &view).await;
    assert!(opened.borrow().is_empty(), "new roots drop it");

    // A newer open replaces an older one.
    enter_on("b.txt", cx);
    enter_on("d.txt", cx);
    cx.run_until_parked();
    assert_eq!(opened.take(), [l.path().join("d.txt")]);
}

/// Presses Tab until `id` has the focus (at most 40 stops).
fn tab_to(window: &mut Window, cx: &mut App, id: impl Into<gpui_kit::ElementId> + Clone) {
    for _ in 0..40 {
        if window.find(id.clone()).focused() == Some(true) {
            return;
        }
        window.press("tab", cx);
    }
    panic!("Tab never reached the element");
}

#[gpui_kit::test]
async fn enter_on_a_focused_chip_clicks_it_instead_of_opening_the_row(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let opened: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
    let sink = opened.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &FolderEvent, _| {
            if let FolderEvent::OpenFiles { left, .. } = event {
                sink.borrow_mut().push(left.clone());
            }
        })
        .detach()
    });
    let b = rels(&view, cx)
        .iter()
        .position(|rel| rel == "b.txt")
        .unwrap();
    view.update(cx, |v, cx| v.select_row_for_test(b, cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        tab_to(window, cx, "folder-chip-all");
        window.press("enter", cx);
    })
    .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().filter()),
        FolderFilter::All,
        "Enter clicked the chip"
    );
    assert!(opened.borrow().is_empty(), "and opened no row");
    // With the rows focused, Enter still opens the selected file.
    let b = rels(&view, cx)
        .iter()
        .position(|rel| rel == "b.txt")
        .unwrap();
    view.update(cx, |v, cx| v.select_row_for_test(b, cx));
    cx.update_window(handle, |_, window, cx| {
        window.focus(&view.focus_handle(cx), cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(opened.borrow().as_slice(), [l.path().join("b.txt")]);
}

#[gpui_kit::test]
async fn shift_tab_and_escape_bring_the_keys_back_to_the_rows(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select_row_for_test(0, cx));
    let selected =
        |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.model().unwrap().selected_row());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let rows = view.read(cx).focus_handle(cx);
        tab_to(window, cx, "folder-chip-all");
        for _ in 0..40 {
            if rows.is_focused(window) {
                break;
            }
            window.press("shift-tab", cx);
        }
        assert!(rows.is_focused(window), "Shift+Tab reached the rows");
        window.press("down", cx);
    })
    .unwrap();
    assert_eq!(selected(cx), Some(1), "Down moved the selection");
    cx.update_window(handle, |_, window, cx| {
        let rows = view.read(cx).focus_handle(cx);
        tab_to(window, cx, "folder-chip-all");
        window.press("escape", cx);
        assert!(
            rows.is_focused(window),
            "Esc with nothing to cancel focuses the rows"
        );
        window.press("down", cx);
    })
    .unwrap();
    assert_eq!(selected(cx), Some(2));
}

#[gpui_kit::test]
async fn row_keys_move_expand_page_and_ask_before_deleting(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    assert_eq!(rels(&view, cx), ["sub", "b.txt", "new.txt"]);
    let selected =
        |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.model().unwrap().selected_row());
    view.update(cx, |v, cx| v.select_row_for_test(0, cx));
    press(cx, handle, "down");
    assert_eq!(selected(cx), Some(1));
    press(cx, handle, "up");
    assert_eq!(selected(cx), Some(0));
    press(cx, handle, "right");
    assert_eq!(
        rels(&view, cx),
        ["sub", "sub/c.txt", "b.txt", "new.txt"],
        "Right expands"
    );
    press(cx, handle, "left");
    assert_eq!(
        rels(&view, cx),
        ["sub", "b.txt", "new.txt"],
        "Left collapses"
    );
    press(cx, handle, "pagedown");
    assert_eq!(selected(cx), Some(2), "Page Down stops at the last row");
    press(cx, handle, "pageup");
    assert_eq!(selected(cx), Some(0), "Page Up stops at the first row");
    // Delete asks first; Cancel deletes nothing.
    press(cx, handle, "end");
    press(cx, handle, "delete");
    wait_for_prompt(cx, &view).await;
    let (message, _) = cx.pending_prompt().expect("a prompt");
    assert_eq!(message, "Move to the Recycle Bin?");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(r.path().join("new.txt").exists(), "Cancel kept the file");
}

fn press(cx: &mut TestAppContext, handle: AnyWindowHandle, key: &str) {
    cx.update_window(handle, |_, window, cx| window.press(key, cx))
        .unwrap();
}

#[gpui_kit::test]
async fn the_fields_are_named(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let field = |input: &Entity<gpui_kit::component::input::InputState>| {
            gpui_kit::ElementId::from(("input", input.entity_id()))
        };
        let v = view.read(cx);
        let (include, exclude, names) = (
            field(v.include_input()),
            field(v.exclude_input()),
            field(v.name_filter_input()),
        );
        assert_eq!(window.find(include).label(), Some("Include masks"));
        assert_eq!(window.find(exclude).label(), Some("Exclude masks"));
        assert_eq!(window.find(names).label(), Some("Filter paths"));
    })
    .unwrap();
}

#[gpui_kit::test]
async fn filters_switch_the_visible_rows(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_filter(FolderFilter::All, cx));
    assert_eq!(rels(&view, cx), ["sub", "a.txt", "b.txt", "new.txt"]);
    view.update(cx, |v, cx| v.set_filter(FolderFilter::Same, cx));
    assert_eq!(rels(&view, cx), ["a.txt"]);
}

#[gpui_kit::test]
async fn refresh_twice_keeps_only_the_latest_result(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    put(&r.path().join("later.txt"), "added");
    view.update(cx, |v, cx| {
        v.refresh(cx);
        v.refresh(cx);
    });
    wait_idle(cx, handle, &view).await;
    assert!(rels(&view, cx).contains(&"later.txt".to_owned()));
    let status = view.read_with(cx, |v, _| v.status().to_owned());
    assert!(
        !status.contains("Error") && !status.contains("Comparing"),
        "{status}"
    );
}

fn select(view: &Entity<FolderView>, cx: &mut TestAppContext, rel: &str) {
    view.update(cx, |v, cx| {
        v.set_filter(FolderFilter::All, cx);
        let row = {
            let m = v.model().unwrap();
            (0..m.visible_len())
                .find(|&r| m.node_at(r).rel == rel)
                .unwrap()
        };
        v.select_row_for_test(row, cx);
    });
}

#[gpui_kit::test]
async fn copying_a_right_only_file_to_the_left_creates_it_and_refreshes(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "new.txt");
    cx.update_window(handle, |_, window, cx| window.press("alt-left", cx))
        .unwrap();
    wait_for_prompt(cx, &view).await;
    let (message, _) = cx.pending_prompt().expect("a prompt");
    assert!(message.starts_with("Copy 1 file"), "{message}");
    cx.simulate_prompt_answer("Copy");
    let target = l.path().join("new.txt");
    let check = target.clone();
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        check.exists() && !v.read(cx).is_busy()
    })
    .await;
    assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    let status = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (0..m.visible_len())
            .map(|r| m.node_at(r))
            .find(|n| n.rel == "new.txt")
            .unwrap()
            .status
            .clone()
    });
    assert_eq!(status, duet_core::folder::tree::Status::Same);
}

#[gpui_kit::test]
async fn copy_from_a_vanished_source_reports_an_error(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "new.txt");
    fs::remove_file(r.path().join("new.txt")).unwrap();
    cx.update_window(handle, |_, window, cx| window.press("alt-left", cx))
        .unwrap();
    wait_for_prompt(cx, &view).await;
    cx.simulate_prompt_answer("Copy");
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx)
            .notice()
            .is_some_and(|n| n.starts_with("Copied 0 files; 1 failed"))
    })
    .await;
    assert!(!l.path().join("new.txt").exists());
    wait_for_prompt(cx, &view).await;
    assert_eq!(cx.pending_prompt().unwrap().0, "1 item failed.");
    cx.simulate_prompt_answer("OK");
}

/// Waits for a native prompt; the test platform queues it instead of showing a window.
async fn wait_for_prompt(cx: &mut TestAppContext, view: &Entity<FolderView>) {
    for _ in 0..2000 {
        let status = view.read_with(cx, |v, _| v.status().to_owned());
        assert!(!status.starts_with("Copy failed"), "{status}");
        if cx.has_pending_prompt() {
            return;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    panic!("no prompt appeared");
}

fn put_at(path: &Path, body: &str, secs: u64) {
    put(path, body);
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

#[gpui_kit::test]
async fn a_folder_copy_asks_once_naming_every_overwrite(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(&l.path().join("cfg/a.txt"), "new a", 1_700_000_000);
    put_at(&r.path().join("cfg/a.txt"), "old a", 1_600_000_000);
    put_at(&l.path().join("cfg/app.json"), "old json", 1_600_000_000);
    put_at(&r.path().join("cfg/app.json"), "hand edited", 1_700_000_000);
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "cfg");
    for answer in ["Cancel", "Copy"] {
        cx.update_window(handle, |_, window, cx| window.press("alt-right", cx))
            .unwrap();
        wait_for_prompt(cx, &view).await;
        let (message, detail) = cx.pending_prompt().expect("a prompt");
        assert!(message.starts_with("Copy 2 files"), "{message}");
        assert!(
            detail.contains("Overwrites 2 files on the right."),
            "{detail}"
        );
        assert!(detail.contains("1 of them is newer"), "{detail}");
        assert!(
            detail.contains("a.txt") && detail.contains("app.json"),
            "{detail}"
        );
        cx.simulate_prompt_answer(answer);
        if answer == "Cancel" {
            cx.run_until_parked();
            let kept = fs::read_to_string(r.path().join("cfg/app.json")).unwrap();
            assert_eq!(kept, "hand edited");
        }
    }
    let target = r.path().join("cfg/app.json");
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        fs::read_to_string(&target).is_ok_and(|t| t == "old json") && !v.read(cx).is_busy()
    })
    .await;
    assert_eq!(
        fs::read_to_string(r.path().join("cfg/a.txt")).unwrap(),
        "new a"
    );
}

#[gpui_kit::test]
async fn a_multi_copy_asks_once_with_a_summary_and_copies_each_file_once(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    put(&l.path().join("only/x.txt"), "x");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    press(cx, handle, "ctrl-a");
    press(cx, handle, "alt-right");
    wait_for_prompt(cx, &view).await;
    let prompt = cx.pending_prompt().expect("one prompt");
    assert!(prompt.0.starts_with("Copy 7 files"), "{prompt:?}");
    assert!(
        prompt.1.contains("Overwrites 6 files on the right."),
        "{prompt:?}"
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(
        !r.path().join("only/x.txt").exists(),
        "Cancel copies nothing"
    );
    press(cx, handle, "alt-right");
    wait_for_prompt(cx, &view).await;
    cx.simulate_prompt_answer("Copy");
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx).notice() == Some("Copied 7 files.")
    })
    .await;
    assert!(!cx.has_pending_prompt(), "asked once per press");
    assert_eq!(
        fs::read_to_string(r.path().join("only/x.txt")).unwrap(),
        "x"
    );
    assert_eq!(fs::read_to_string(r.path().join("f3.txt")).unwrap(), "left");
}

#[gpui_kit::test]
async fn the_copy_notice_counts_files_and_folders_apart(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for i in 0..5 {
        put(&l.path().join(format!("f{i}.txt")), "x");
    }
    for dir in ["e1", "e2"] {
        fs::create_dir_all(l.path().join(dir)).unwrap();
    }
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    press(cx, handle, "ctrl-a");
    press(cx, handle, "alt-right");
    wait_for_prompt(cx, &view).await;
    cx.simulate_prompt_answer("Copy");
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx).notice() == Some("Copied 5 files and created 2 folders.")
    })
    .await;
    assert!(r.path().join("e2").is_dir());
}

#[gpui_kit::test]
async fn copy_with_the_mismatch_chip_copies_only_shown_files(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("d/same.txt"), "s");
    put(&r.path().join("d/same.txt"), "s");
    put(&l.path().join("d/new.txt"), "n");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "d");
    view.update(cx, |v, cx| v.set_filter(FolderFilter::Mismatch, cx));
    press(cx, handle, "alt-right");
    wait_for_prompt(cx, &view).await;
    let prompt = cx.pending_prompt().unwrap();
    assert!(
        prompt.0.starts_with("Copy 1 file"),
        "same.txt is hidden by Mismatches: {prompt:?}"
    );
    cx.simulate_prompt_answer("Copy");
    let target = r.path().join("d/new.txt");
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        target.exists() && v.read(cx).notice().is_some()
    })
    .await;
    // same.txt exists on both sides and is hidden by Mismatches: only new.txt counts.
    assert_eq!(
        fs::read_to_string(r.path().join("d/same.txt")).unwrap(),
        "s"
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("Copied 1 file.")
    );
}

#[gpui_kit::test]
async fn esc_cancels_a_running_copy_and_new_work_waits_for_it(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    press(cx, handle, "ctrl-a");
    view.update(cx, |v, cx| v.hold_batch_for_test("Copying", 6, cx));
    assert!(view.read_with(cx, |v, _| v.batch_running()));
    press(cx, handle, "alt-right");
    assert_eq!(
        view.read_with(cx, |v, _| v.status_line().to_string()),
        "Wait for the running copy or delete to finish."
    );
    assert!(!cx.has_pending_prompt(), "no prompt while a batch runs");
    press(cx, handle, "escape");
    assert!(
        view.read_with(cx, |v, _| v.batch_cancelled_for_test()),
        "Esc cancels the running batch"
    );
}

#[gpui_kit::test]
async fn a_copy_confirmed_after_the_folders_changed_runs_nothing(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "new.txt");
    press(cx, handle, "alt-left");
    wait_for_prompt(cx, &view).await;
    view.update(cx, |v, cx| v.refresh(cx));
    cx.simulate_prompt_answer("Copy");
    cx.run_until_parked();
    assert!(!l.path().join("new.txt").exists());
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("The folders changed while the question was open. Copy again to use the new view.")
    );
}

#[gpui_kit::test]
async fn a_file_that_appears_while_the_prompt_is_open_is_not_overwritten(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "new.txt");
    press(cx, handle, "alt-left");
    wait_for_prompt(cx, &view).await;
    put(&l.path().join("new.txt"), "mine");
    cx.simulate_prompt_answer("Copy");
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx)
            .notice()
            .is_some_and(|n| n.starts_with("Copied 0 files; 1 failed"))
    })
    .await;
    assert_eq!(
        fs::read_to_string(l.path().join("new.txt")).unwrap(),
        "mine"
    );
    let notice = view.read_with(cx, |v, _| v.notice().unwrap().to_owned());
    assert!(notice.contains("new.txt"), "{notice}");
}

#[gpui_kit::test]
async fn delete_waits_for_a_running_batch(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "new.txt");
    view.update(cx, |v, cx| v.hold_batch_for_test("Copying", 1, cx));
    press(cx, handle, "delete");
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert_eq!(
        view.read_with(cx, |v, _| v.status_line().to_string()),
        "Wait for the running copy or delete to finish."
    );
}

#[cfg(windows)]
fn mklink_junction(link: &Path, target: &Path) {
    let out = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(out.status.success());
}

/// The junction appears after the scan, so the plan still thinks `lib` is absent on the right:
/// the preflight must refuse the whole copy before any prompt or write.
#[cfg(windows)]
#[gpui_kit::test]
async fn copy_never_writes_through_a_junction_on_the_destination(cx: &mut TestAppContext) {
    let (l, r, outside) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    put(&l.path().join("lib/new.txt"), "new");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    mklink_junction(&r.path().join("lib"), outside.path());
    select(&view, cx, "lib");
    cx.update_window(handle, |_, window, cx| window.press("alt-right", cx))
        .unwrap();
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx).status().starts_with("Copy failed")
    })
    .await;
    let status = view.read_with(cx, |v, _| v.status().to_owned());
    assert!(status.contains("link or junction"), "{status}");
    assert!(!cx.has_pending_prompt());
    assert!(!outside.path().join("new.txt").exists());
}

/// A junction already there at scan time is a destination of another kind: the plan leaves
/// it out, and nothing is written through it.
#[cfg(windows)]
#[gpui_kit::test]
async fn a_destination_junction_known_at_scan_time_is_left_alone(cx: &mut TestAppContext) {
    let (l, r, outside) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    put(&l.path().join("lib/new.txt"), "new");
    mklink_junction(&r.path().join("lib"), outside.path());
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "lib");
    press(cx, handle, "alt-right");
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert!(
        view.read_with(cx, |v, _| v.status().starts_with("Nothing to copy")),
        "the plan leaves the junction out"
    );
    assert!(!outside.path().join("new.txt").exists());
}

#[gpui_kit::test]
async fn escape_cancels_a_running_scan(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    let expected = "Cancelled. Press F5 to compare again.";
    let (busy, status) = view.read_with(cx, |v, _| (v.is_busy(), v.status().to_owned()));
    assert!(!busy);
    assert_eq!(status, expected);
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| v.status().to_owned()), expected);
}

#[gpui_kit::test]
async fn a_filter_changed_during_a_refresh_survives_it(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.refresh(cx);
        v.set_filter(FolderFilter::All, cx);
    });
    wait_idle(cx, handle, &view).await;
    let filter = view.read_with(cx, |v, _| v.model().unwrap().filter());
    assert_eq!(filter, FolderFilter::All);
}

#[gpui_kit::test]
async fn stale_results_never_replace_the_current_tree(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let before = rels(&view, cx);
    let other = tempfile::tempdir().unwrap();
    put(&other.path().join("elsewhere.txt"), "x");
    let stale = duet_core::folder::run::compare_folders(
        other.path(),
        r.path(),
        &FolderOptions::default(),
        &Default::default(),
        &duet_core::cancel::CancelToken::new(),
    );
    let first = view.read_with(cx, |v, _| v.job_generation());
    view.update(cx, |v, cx| {
        v.refresh(cx);
        assert!(!v.apply_for_test(first, stale, true, cx));
    });
    assert_eq!(rels(&view, cx), before, "the older job's result is dropped");
    wait_idle(cx, handle, &view).await;
    assert_eq!(rels(&view, cx), before);
}

#[gpui_kit::test]
async fn content_mode_and_masks_rerun_the_compare(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    // Same size and time, different bytes: only a content compare sees it.
    put(&l.path().join("x.txt"), "aaaa");
    put(&r.path().join("x.txt"), "bbbb");
    put(&l.path().join("notes.md"), "same");
    put(&r.path().join("notes.md"), "same");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let status_of = |cx: &mut TestAppContext, rel: &str| {
        view.read_with(cx, |v, _| {
            let m = v.model().unwrap();
            (0..m.visible_len())
                .map(|r| m.node_at(r))
                .find(|n| n.rel == rel)
                .map(|n| n.status.clone())
        })
    };
    view.update(cx, |v, cx| v.set_filter(FolderFilter::All, cx));
    assert_eq!(
        status_of(cx, "x.txt"),
        Some(duet_core::folder::tree::Status::Same)
    );
    view.update(cx, |v, cx| {
        v.set_mode(duet_core::folder::run::CompareMode::Content, cx)
    });
    wait_idle(cx, handle, &view).await;
    assert!(matches!(
        status_of(cx, "x.txt"),
        Some(duet_core::folder::tree::Status::Different { .. })
    ));
    let status = view.read_with(cx, |v, _| v.status().to_owned());
    assert!(status.contains("Content"), "{status}");
    // Typing a mask and pressing Enter reruns the compare with it.
    cx.update_window(handle, |_, window, cx| {
        let input = view.read(cx).include_input().clone();
        window.focus(&input.focus_handle(cx), cx);
        window.input("*.md", cx);
        window.press("enter", cx);
    })
    .unwrap();
    wait_idle(cx, handle, &view).await;
    assert_eq!(rels(&view, cx), ["notes.md"]);
    let include = view.read_with(cx, |v, _| v.options().filters.include.clone());
    assert_eq!(include, ["*.md"]);
}

#[gpui_kit::test]
async fn the_folder_scrollbar_drags_the_rows(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for i in 0..300 {
        put(&l.path().join(format!("f{i:03}.txt")), "x");
        put(&r.path().join(format!("f{i:03}.txt")), "x");
    }
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_filter(FolderFilter::All, cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let list = window.find("folder-list").bounds();
        let x = list.right() - px(8.);
        window.drag(
            Point::new(x, list.top() + px(6.)),
            Point::new(x, list.bottom() - px(2.)),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    let top = view.read_with(cx, |v, _| v.top_row());
    assert!(
        top > 200,
        "the thumb dragged to the bottom shows the last rows, top row {top}"
    );
}

#[gpui_kit::test]
async fn folder_path_bars_copy_their_root(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("folder-left-path-copy", cx);
    })
    .unwrap();
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied, Some(l.path().display().to_string()));
}

#[gpui_kit::test]
async fn folder_f8_wraps_with_a_notice(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    // Three stops (b.txt, new.txt, sub): within six presses one of them must wrap.
    let mut notice = None;
    for _ in 0..6 {
        cx.update_window(handle, |_, window, cx| window.press("f8", cx))
            .unwrap();
        notice = view.read_with(cx, |v, _| v.notice().map(str::to_owned));
        if notice.is_some() {
            break;
        }
    }
    assert_eq!(notice.as_deref(), Some("Wrapped to the first difference."));
}

#[gpui_kit::test]
async fn selecting_a_row_explains_why_it_differs(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let row = rels(&view, cx)
        .iter()
        .position(|rel| rel == "b.txt")
        .unwrap();
    view.update(cx, |v, cx| v.select_row_for_test(row, cx));
    let line = view.read_with(cx, |v, _| v.status_line().to_string());
    assert!(
        line.ends_with("Selected: b.txt — size 4 → 10 bytes"),
        "{line}"
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("folder-mid", row)).is_some());
        assert!(window.try_find("folder-selection").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
async fn content_mode_marks_line_ending_only_files_as_text_equal(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("x.txt"), "a\r\nb\r\n");
    put(&r.path().join("x.txt"), "a\nb\n");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.set_mode(duet_core::folder::run::CompareMode::Content, cx)
    });
    wait_idle(cx, handle, &view).await;
    let similar = view.read_with(cx, |v, _| v.model().unwrap().counts().similar);
    assert_eq!(similar, 1);
    assert_eq!(
        rels(&view, cx),
        Vec::<String>::new(),
        "the Mismatches filter leaves a file that is only text-equal out"
    );
    view.update(cx, |v, cx| v.set_filter(FolderFilter::Same, cx));
    assert_eq!(rels(&view, cx), ["x.txt"], "the Same filter lists it");
}

#[gpui_kit::test]
async fn name_filter_home_end_and_mask_errors(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.set_filter(FolderFilter::All, cx);
        v.set_name_filter("c.t", cx);
    });
    assert_eq!(rels(&view, cx), ["sub", "sub/c.txt"]);
    view.update(cx, |v, cx| v.set_name_filter("", cx));
    cx.update_window(handle, |_, window, cx| window.press("end", cx))
        .unwrap();
    let (selected, len) = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (m.selected_row(), m.visible_len())
    });
    assert_eq!(selected, Some(len - 1));
    cx.update_window(handle, |_, window, cx| window.press("home", cx))
        .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().selected_row()),
        Some(0)
    );
    cx.update_window(handle, |_, window, cx| {
        let input = view.read(cx).include_input().clone();
        window.focus(&input.focus_handle(cx), cx);
        window.input("src/[", cx);
        window.press("enter", cx);
    })
    .unwrap();
    let (error, include) = view.read_with(cx, |v, _| {
        (
            v.mask_error().map(str::to_owned),
            v.options().filters.include.clone(),
        )
    });
    assert!(error.is_some_and(|e| e.contains("invalid filter pattern")));
    assert!(include.is_empty(), "a bad mask does not start a compare");
}

#[gpui_kit::test]
async fn folder_f9_swaps_roots_and_one_sided_rows(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let before = view.read_with(cx, |v, _| v.model().unwrap().counts());
    cx.update_window(handle, |_, window, cx| window.press("f9", cx))
        .unwrap();
    let (after, left) = view.read_with(cx, |v, _| {
        (v.model().unwrap().counts(), v.paths().0.clone())
    });
    assert_eq!(left, r.path().to_path_buf());
    assert_eq!(
        (after.left_only, after.right_only),
        (before.right_only, before.left_only)
    );
}

#[gpui_kit::test]
async fn swapping_during_a_refresh_then_escape_keeps_rows_and_roots_together(
    cx: &mut TestAppContext,
) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.refresh(cx);
        assert!(v.is_busy());
        v.swap_sides(cx);
    });
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    let status = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        m.comparison()
            .nodes
            .iter()
            .find(|n| n.rel == "sub")
            .map(|n| n.status.clone())
    });
    assert_eq!(
        status,
        Some(duet_core::folder::tree::Status::RightOnly),
        "sub/ exists only under the new right root"
    );
}

#[gpui_kit::test]
async fn folder_chips_filter_as_before(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("folder-chip-mismatch").selected(),
            Some(true),
            "Mismatched is the default"
        );
        window.click("folder-chip-left-only", cx);
    })
    .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().filter()),
        FolderFilter::LeftOnly
    );
    let rows = rels(&view, cx);
    assert_eq!(
        rows.first().map(String::as_str),
        Some("sub"),
        "the left-only folder"
    );
    assert!(
        !rows.iter().any(|rel| rel == "b.txt" || rel == "new.txt"),
        "{rows:?}"
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("folder-chip-left-only").selected(), Some(true));
        assert_eq!(window.find("folder-chip-mismatch").selected(), Some(false));
        assert_eq!(window.find("folder-expand-all").label(), Some("Expand all"));
        // The tab list holds the chips and nothing else.
        let tabs = gpui_kit::ElementId::from("folder-chip-tabs");
        assert_eq!(
            window.find(tabs.clone()).role(),
            Some(gpui_kit::Role::TabList)
        );
        assert_ne!(
            window.find("folder-chips").role(),
            Some(gpui_kit::Role::TabList)
        );
        assert!(window.find("folder-chip-all").path().contains(&tabs));
        for other in [
            "folder-expand-all",
            "folder-copy-right",
            "folder-name-filter",
        ] {
            assert!(!window.find(other).path().contains(&tabs), "{other}");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
async fn the_pickers_are_named_by_their_side(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        // The folder icon is drawn, so it adds nothing to the name.
        assert_eq!(window.find("folder-pick-left").label(), Some("Left"));
        assert_eq!(window.find("folder-pick-right").label(), Some("Right"));
    })
    .unwrap();
}

#[gpui_kit::test]
async fn a_picker_changes_one_root_and_compares_again(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let other = tempfile::tempdir().unwrap();
    put(&other.path().join("b.txt"), "right side");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let events = Rc::new(RefCell::new(Vec::new()));
    let seen = events.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &FolderEvent, _| {
            if let FolderEvent::Rooted { right, .. } = event {
                seen.borrow_mut().push(right.clone());
            }
        })
        .detach()
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("folder-pick-right", cx);
    })
    .unwrap();
    let chosen = other.path().to_path_buf();
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.directories && !options.files);
        Some(vec![chosen])
    });
    cx.run_until_parked();
    wait_idle(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.paths().1.clone()), other.path());
    assert_eq!(events.borrow().as_slice(), [other.path().to_path_buf()]);
    assert!(
        rels(&view, cx).contains(&"a.txt".to_owned()),
        "a.txt is now left only"
    );
}

#[gpui_kit::test]
async fn up_moves_both_roots_to_their_parents(cx: &mut TestAppContext) {
    let base = tempfile::tempdir().unwrap();
    let (l, r) = (
        base.path().join("x").join("l"),
        base.path().join("y").join("r"),
    );
    put(&l.join("a.txt"), "1");
    put(&r.join("a.txt"), "22");
    let (handle, view) = open_folder(cx, &l, &r);
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("folder-up", cx);
    })
    .unwrap();
    wait_idle(cx, handle, &view).await;
    let (left, right) = view.read_with(cx, |v, _| (v.paths().0.clone(), v.paths().1.clone()));
    assert_eq!(
        (left, right),
        (base.path().join("x"), base.path().join("y"))
    );
}

#[gpui_kit::test]
async fn folder_rows_show_columns_hatches_and_counts(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let rows = rels(&view, cx);
    let sub = rows.iter().position(|rel| rel == "sub").unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("folder-header").is_some());
        assert_eq!(
            window.find(("folder-mid", sub)).bounds().size.width,
            px(26.)
        );
        assert!(
            window.try_find(("folder-missing-right", sub)).is_some(),
            "sub is left only"
        );
        assert!(window.try_find(("folder-missing-left", sub)).is_none());
        assert!(
            window.try_find(("folder-count-left", sub)).is_some(),
            "one differing file below sub"
        );
        assert!(window.try_find(("folder-count-right", sub)).is_none());
        let name = window.find(("folder-name-left", sub)).bounds();
        let pill = window.find(("folder-count-left", sub)).bounds();
        assert!(
            (pill.left() - name.right()).abs() < px(16.),
            "the pill follows the name: name ends {:?}, pill starts {:?}",
            name.right(),
            pill.left()
        );
    })
    .unwrap();
    view.update(cx, |v, cx| v.select_row_for_test(sub, cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let bar = window.find(("folder-selected-bar", sub)).bounds();
        assert_eq!(bar.size.width, px(3.));
    })
    .unwrap();
}

#[gpui_kit::test]
async fn an_empty_list_explains_itself(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_name_filter("no such name", cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("folder-empty").is_some());
    })
    .unwrap();
}

/// The empty list's title and hint as drawn.
fn empty_text(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
) -> (Option<String>, Option<String>) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let text = |id: &'static str| {
            window
                .try_find(id)
                .and_then(|e| e.label().map(str::to_owned))
        };
        (text("folder-empty-title"), text("folder-empty-hint"))
    })
    .unwrap()
}

#[gpui_kit::test]
async fn empty_lists_say_why(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("a.txt"), "same");
    put(&r.path().join("a.txt"), "same");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let choose_all = Some("Choose All to see every file.".to_owned());
    assert_eq!(
        empty_text(cx, handle),
        (
            Some("The folders are identical.".into()),
            choose_all.clone()
        )
    );
    view.update(cx, |v, cx| v.set_filter(FolderFilter::Different, cx));
    assert_eq!(
        empty_text(cx, handle),
        (Some("No rows match this filter.".into()), choose_all)
    );
    view.update(cx, |v, cx| {
        v.set_filter(FolderFilter::All, cx);
        v.set_name_filter("zzz", cx);
    });
    assert_eq!(
        empty_text(cx, handle),
        (
            Some("No names match the filter.".into()),
            Some("Clear the name filter to see every file.".into())
        )
    );
    let (el, er) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    view.update(cx, |v, cx| {
        v.set_roots(el.path().to_path_buf(), er.path().to_path_buf(), cx)
    });
    wait_idle(cx, handle, &view).await;
    assert_eq!(
        empty_text(cx, handle),
        (Some("Both folders are empty.".into()), None)
    );
}

#[gpui_kit::test]
async fn thai_names_show_in_the_folder_tree(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("ไฟล์.txt"), "ก");
    put(&r.path().join("ไฟล์.txt"), "กข");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    assert_eq!(rels(&view, cx), ["ไฟล์.txt"]);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find(("folder-row", 0usize)).visible());
        for side in ["folder-name-left", "folder-name-right"] {
            let name = window.find((side, 0usize));
            assert_eq!(
                name.label(),
                Some("ไฟล์.txt"),
                "{side} renders the Thai name"
            );
            assert!(name.bounds().size.width > px(0.));
        }
    })
    .unwrap();
}

/// A differing file four folders down, so every folder on the way has a count pill.
fn deep_fixture() -> (tempfile::TempDir, tempfile::TempDir) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let deep = Path::new("alpha")
        .join("bravo")
        .join("charlie")
        .join("util");
    put(&l.path().join(&deep).join("settings.json"), "left");
    put(&r.path().join(&deep).join("settings.json"), "right side");
    put(&l.path().join("gone.txt"), "only");
    put(&r.path().join("new.txt"), "only");
    (l, r)
}

#[gpui_kit::test]
async fn a_narrow_folder_view_keeps_names_and_label_pairs(cx: &mut TestAppContext) {
    let (l, r) = deep_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.simulate_window_resize(handle, size(px(700.), px(800.)));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("folder-expand-all", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        let fonts = duet_app::ui::fonts::Fonts::current(cx);
        let advance = duet_app::ui::fonts::code_advance(window, &fonts.code) * (12. / 12.5);
        let rows = view.read(cx).model().unwrap().visible_len();
        let util = (0..rows)
            .find(|&row| view.read(cx).model().unwrap().node_at(row).name == "util")
            .expect("the deep folder is listed");
        for row in 0..rows {
            let node = view.read(cx).model().unwrap().node_at(row).clone();
            for side in ["folder-name-left", "folder-name-right"] {
                let Some(name) = window.try_find((side, row)) else {
                    continue;
                };
                let width = name.bounds().size.width;
                assert!(
                    width >= advance * node.name.chars().count().min(5) as f32 - px(0.5),
                    "{} at depth {} keeps a few characters: {width:?}",
                    node.name,
                    node.depth
                );
            }
        }
        assert!(
            window.find(("folder-name-left", util)).bounds().size.width >= advance * 4.,
            "the deep folder's name shows"
        );
        // Each options label stays on its field's line.
        for (label, field) in [
            ("folder-criterion-label", "folder-mode"),
            ("folder-include-label", "folder-include"),
            ("folder-exclude-label", "folder-exclude"),
        ] {
            let (a, b) = (window.find(label).bounds(), window.find(field).bounds());
            assert!(
                (a.center().y - b.center().y).abs() < px(2.),
                "{label} sits beside {field}: {a:?} {b:?}"
            );
        }
    })
    .unwrap();
}

#[gpui_kit::test]
async fn a_narrow_status_bar_ends_in_an_ellipsis_not_mid_word(cx: &mut TestAppContext) {
    let (l, r) = deep_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.simulate_window_resize(handle, size(px(700.), px(800.)));
    // A selected row adds its note on the right, as in the self-test.
    view.update(cx, |v, cx| v.select_row_for_test(0, cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let full = view.read(cx).status().to_owned();
        let shown = view
            .read(cx)
            .status_shown_for_test()
            .expect("the counts are drawn as one line");
        assert!(
            shown == full || shown.ends_with('…'),
            "the status is cut with an ellipsis: {shown:?} of {full:?}"
        );
        assert!(
            shown != full,
            "700 px is too narrow for {full:?} beside the note"
        );
        assert!(full.starts_with(shown.trim_end_matches('…')));
        let text = window.find("folder-status").bounds();
        let note = window.find("folder-selection").bounds();
        assert!(note.right() <= text.right(), "the note stays in the bar");
    })
    .unwrap();
}

#[gpui_kit::test]
async fn a_folder_notice_shows_its_full_text_on_hover(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    for _ in 0..6 {
        cx.update_window(handle, |_, window, cx| window.press("f8", cx))
            .unwrap();
        if view.read_with(cx, |v, _| v.notice().is_some()) {
            break;
        }
    }
    assert!(view.read_with(cx, |v, _| v.notice().is_some()));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.hover("folder-status-text", cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("tooltip-text").is_some(),
            "a notice cut short by a narrow bar can still be read"
        );
    })
    .unwrap();
}

/// A left click with modifiers at the centre of `id` (or `offset` from its origin): the
/// harness's own click has none.
fn click_mod(
    window: &mut Window,
    cx: &mut App,
    id: impl Into<gpui_kit::ElementId>,
    offset: Option<Point<gpui_kit::Pixels>>,
    modifiers: gpui_kit::Modifiers,
) {
    use gpui_kit::{InputEvent as _, MouseButton, MouseDownEvent, MouseUpEvent};
    window.render_frame(cx);
    let bounds = window.find(id).bounds();
    let position = offset.map_or(bounds.center(), |o| bounds.origin + o);
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

fn ctrl() -> gpui_kit::Modifiers {
    gpui_kit::Modifiers {
        control: true,
        ..Default::default()
    }
}

fn shift() -> gpui_kit::Modifiers {
    gpui_kit::Modifiers {
        shift: true,
        ..Default::default()
    }
}

fn wide_fixture() -> (tempfile::TempDir, tempfile::TempDir) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for i in 0..6 {
        put(&l.path().join(format!("f{i}.txt")), "left");
        put(&r.path().join(format!("f{i}.txt")), "right!");
    }
    (l, r)
}

#[gpui_kit::test]
async fn ctrl_and_shift_clicks_build_a_selection(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.click(("folder-row", 0usize), cx);
        click_mod(window, cx, ("folder-row", 2usize), None, ctrl());
        click_mod(window, cx, ("folder-row", 4usize), None, shift());
    })
    .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.chosen_rels_for_test()),
        ["f2.txt", "f3.txt", "f4.txt"],
        "shift extends from the ctrl-clicked anchor"
    );
    press(cx, handle, "ctrl-a");
    assert_eq!(view.read_with(cx, |v, _| v.chosen_rels_for_test()).len(), 6);
    press(cx, handle, "down");
    assert_eq!(
        view.read_with(cx, |v, _| v.chosen_rels_for_test()).len(),
        1,
        "a plain move resets"
    );
    // The cursor is on the last row by now, so the range grows upwards.
    press(cx, handle, "shift-up");
    press(cx, handle, "shift-up");
    assert_eq!(view.read_with(cx, |v, _| v.chosen_rels_for_test()).len(), 3);
    press(cx, handle, "shift-down");
    assert_eq!(
        view.read_with(cx, |v, _| v.chosen_rels_for_test()).len(),
        2,
        "shift in the other direction shrinks it"
    );
}

#[gpui_kit::test]
async fn dragging_down_the_rows_selects_the_range(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let from = window.find(("folder-row", 1usize)).bounds().center();
        let to = window.find(("folder-row", 3usize)).bounds().center();
        window.drag(from, to, cx);
    })
    .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.chosen_rels_for_test()),
        ["f1.txt", "f2.txt", "f3.txt"]
    );
}

#[gpui_kit::test]
async fn chosen_rows_are_tinted_and_the_status_counts_them(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    press(cx, handle, "ctrl-a");
    let status = view.read_with(cx, |v, _| v.status_line().to_string());
    assert!(status.contains("6 selected"), "{status}");
    // The tint is `palette.sel` on every row the model calls chosen.
    let all = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (0..m.visible_len()).all(|row| m.is_chosen(row))
    });
    assert!(all, "every row is chosen, so every row is tinted");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        // Only the cursor row carries the 3 px bar; every chosen row is tinted (`palette.sel`).
        assert!(window.try_find(("folder-selected-bar", 0usize)).is_some());
        assert!(window.try_find(("folder-selected-bar", 3usize)).is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
async fn clicking_the_expander_toggles_the_folder_without_selecting_it(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("sub/c.txt"), "x");
    put(&r.path().join("sub/c.txt"), "y!");
    put(&l.path().join("z.txt"), "x");
    put(&r.path().join("z.txt"), "y!");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let before = rels(&view, cx);
    select(&view, cx, "z.txt");
    cx.update_window(handle, |_, window, cx| {
        // The expander sits just inside the cell's 8 px padding and the row's 1 px border.
        let at = gpui_kit::point(px(14.), px(12.));
        click_mod(
            window,
            cx,
            ("folder-row", 0usize),
            Some(at),
            Default::default(),
        );
    })
    .unwrap();
    let after = rels(&view, cx);
    assert_ne!(
        before, after,
        "the folder opened or closed: {before:?} -> {after:?}"
    );
    let chosen = view.read_with(cx, |v, _| v.chosen_rels_for_test());
    assert!(
        chosen == ["z.txt"],
        "the expander does not select: {chosen:?}"
    );
}

#[gpui_kit::test]
async fn holding_the_pointer_below_the_list_scrolls_and_keeps_selecting(cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent as _, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for i in 0..80 {
        put(&l.path().join(format!("f{i:02}.txt")), "left");
        put(&r.path().join(format!("f{i:02}.txt")), "right!");
    }
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let from = window.find(("folder-row", 1usize)).bounds().center();
        let list = window.find("folder-list").bounds();
        let below = gpui_kit::point(from.x, list.bottom() + px(30.));
        let none = gpui_kit::Modifiers::default();
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Left,
                position: from,
                modifiers: none,
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
        window.dispatch_event(
            MouseMoveEvent {
                position: below,
                pressed_button: Some(MouseButton::Left),
                modifiers: none,
            }
            .to_platform_input(),
            cx,
        );
        for _ in 0..8 {
            window.render_frame(cx);
            window.simulate_next_frame(cx);
        }
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Left,
                position: below,
                modifiers: none,
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
    })
    .unwrap();
    assert!(
        view.read_with(cx, |v, _| v.top_row()) > 0,
        "the list scrolled"
    );
    assert!(
        view.read_with(cx, |v, _| v.chosen_rels_for_test()).len() > 20,
        "the range followed the scroll"
    );
}

/// Left-button helpers for the drag tests below: a press, a held move and a release.
fn mouse(
    window: &mut Window,
    cx: &mut App,
    kind: &str,
    at: Point<gpui_kit::Pixels>,
    modifiers: gpui_kit::Modifiers,
) {
    use gpui_kit::{InputEvent as _, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
    window.render_frame(cx);
    let event = match kind {
        "down" => MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }
        .to_platform_input(),
        "move" => MouseMoveEvent {
            position: at,
            pressed_button: Some(MouseButton::Left),
            modifiers,
        }
        .to_platform_input(),
        _ => MouseUpEvent {
            button: MouseButton::Left,
            position: at,
            modifiers,
            click_count: 1,
        }
        .to_platform_input(),
    };
    window.dispatch_event(event, cx);
}

#[gpui_kit::test]
async fn ctrl_dragging_down_and_back_up_shrinks_the_range(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let at =
            |window: &mut Window, row: usize| window.find(("folder-row", row)).bounds().center();
        let none = gpui_kit::Modifiers::default();
        window.click(("folder-row", 0usize), cx);
        let (p1, p4, p3) = (at(window, 1), at(window, 4), at(window, 3));
        mouse(window, cx, "down", p1, ctrl());
        mouse(window, cx, "move", p4, ctrl());
        mouse(window, cx, "move", p3, ctrl());
        mouse(window, cx, "up", p3, none);
    })
    .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.chosen_rels_for_test()),
        ["f0.txt", "f1.txt", "f2.txt", "f3.txt"],
        "row 0 stays (base); the sweep went to row 4 and came back to row 3"
    );
}

#[gpui_kit::test]
async fn pressing_an_edge_row_without_moving_selects_just_that_row(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for i in 0..80 {
        put(&l.path().join(format!("f{i:02}.txt")), "left");
        put(&r.path().join(format!("f{i:02}.txt")), "right!");
    }
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        // The last row whose centre is still inside the list: it sits in the bottom band.
        let list = window.find("folder-list").bounds();
        let mut row = 0usize;
        while window
            .try_find(("folder-row", row + 1))
            .is_some_and(|b| b.bounds().center().y < list.bottom())
        {
            row += 1;
        }
        let at = window.find(("folder-row", row)).bounds().center();
        let none = gpui_kit::Modifiers::default();
        mouse(window, cx, "down", at, none);
        for _ in 0..8 {
            window.render_frame(cx);
            window.simulate_next_frame(cx);
        }
        mouse(window, cx, "up", at, none);
    })
    .unwrap();
    assert_eq!(view.read_with(cx, |v, _| v.chosen_rels_for_test()).len(), 1);
    assert_eq!(view.read_with(cx, |v, _| v.top_row()), 0, "no scroll");
}

/// Installs a stand-in for the Recycle Bin that deletes inside the temp dir and records paths.
fn fake_trash(
    cx: &mut TestAppContext,
    view: &Entity<FolderView>,
) -> std::sync::Arc<std::sync::Mutex<Vec<PathBuf>>> {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    view.update(cx, |v, _| {
        v.set_trash_for_test(move |path: &Path| {
            log.lock().unwrap().push(path.to_path_buf());
            remove_in_place(path)
        })
    });
    seen
}

fn dispatch(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<FolderView>,
    action: impl gpui_kit::Action,
) {
    cx.update_window(handle, |_, window, cx| {
        window.focus(&view.focus_handle(cx), cx);
        window.dispatch_action(Box::new(action), cx);
    })
    .unwrap();
}

async fn wait_notice(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<FolderView>,
) -> String {
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        let v = v.read(cx);
        v.notice().is_some() && !v.batch_running() && !v.is_busy()
    })
    .await;
    view.read_with(cx, |v, _| v.notice().unwrap().to_owned())
}

#[gpui_kit::test]
async fn deleting_a_filtered_folder_keeps_hidden_files(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("d/same.txt"), "s");
    put(&r.path().join("d/same.txt"), "s");
    put(&l.path().join("d/gone.txt"), "g");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let trashed = fake_trash(cx, &view);
    select(&view, cx, "d");
    view.update(cx, |v, cx| v.set_filter(FolderFilter::Mismatch, cx));
    dispatch(cx, handle, &view, duet_app::folder::view::DeleteLeft);
    wait_for_prompt(cx, &view).await;
    let prompt = cx.pending_prompt().unwrap();
    assert_eq!(prompt.0, "Move 1 item to the Recycle Bin?");
    assert!(
        prompt
            .1
            .contains("1 folder still holds files that are not shown"),
        "{}",
        prompt.1
    );
    cx.simulate_prompt_answer("Move to Recycle Bin");
    let notice = wait_notice(cx, handle, &view).await;
    assert_eq!(notice, "Moved 1 item to the Recycle Bin.");
    assert!(!l.path().join("d/gone.txt").exists());
    assert!(
        l.path().join("d/same.txt").exists(),
        "hidden by Mismatches, so kept"
    );
    assert_eq!(*trashed.lock().unwrap(), [l.path().join("d/gone.txt")]);
}

#[gpui_kit::test]
async fn shift_delete_offers_both_sides_for_the_whole_selection(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let trashed = fake_trash(cx, &view);
    press(cx, handle, "ctrl-a");
    press(cx, handle, "shift-delete");
    wait_for_prompt(cx, &view).await;
    assert_eq!(
        cx.pending_prompt().unwrap().0,
        "Move 12 items to the Recycle Bin?"
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(l.path().join("f0.txt").exists() && r.path().join("f0.txt").exists());
    assert!(trashed.lock().unwrap().is_empty());
}

#[gpui_kit::test]
async fn del_asks_which_side_for_the_selection_and_runs_the_choice(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let trashed = fake_trash(cx, &view);
    press(cx, handle, "ctrl-a");
    press(cx, handle, "delete");
    wait_for_prompt(cx, &view).await;
    assert_eq!(cx.pending_prompt().unwrap().0, "Move to the Recycle Bin?");
    cx.simulate_prompt_answer("Delete left");
    let notice = wait_notice(cx, handle, &view).await;
    assert_eq!(notice, "Moved 6 items to the Recycle Bin.");
    assert!(!l.path().join("f0.txt").exists() && r.path().join("f0.txt").exists());
    assert_eq!(trashed.lock().unwrap().len(), 6);
}

#[gpui_kit::test]
async fn a_delete_prompt_goes_stale_when_the_folders_change(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let trashed = fake_trash(cx, &view);
    select(&view, cx, "f0.txt");
    dispatch(cx, handle, &view, duet_app::folder::view::DeleteLeft);
    wait_for_prompt(cx, &view).await;
    view.update(cx, |v, cx| v.refresh(cx));
    cx.simulate_prompt_answer("Move to Recycle Bin");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("The folders changed while the question was open. Delete again to use the new view.")
    );
    assert!(l.path().join("f0.txt").exists() && trashed.lock().unwrap().is_empty());
}

#[gpui_kit::test]
async fn a_delete_skips_files_edited_or_removed_while_the_prompt_was_open(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let trashed = fake_trash(cx, &view);
    press(cx, handle, "ctrl-a");
    press(cx, handle, "delete");
    wait_for_prompt(cx, &view).await;
    put(&l.path().join("f0.txt"), "left, edited meanwhile");
    fs::remove_file(l.path().join("f1.txt")).unwrap();
    cx.simulate_prompt_answer("Delete left");
    let notice = wait_notice(cx, handle, &view).await;
    assert_eq!(
        notice,
        "Moved 4 items to the Recycle Bin. 2 items changed on disk and were left alone. Press F5 to compare again."
    );
    let mut got = trashed.lock().unwrap().clone();
    got.sort();
    let want: Vec<PathBuf> = (2..6).map(|i| l.path().join(format!("f{i}.txt"))).collect();
    assert_eq!(got, want);
    assert!(l.path().join("f0.txt").exists(), "the edited file stays");
}

#[gpui_kit::test]
async fn a_delete_plans_again_after_the_answer(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let trashed = fake_trash(cx, &view);
    select(&view, cx, "f0.txt");
    dispatch(cx, handle, &view, duet_app::folder::view::DeleteLeft);
    wait_for_prompt(cx, &view).await;
    fs::remove_file(l.path().join("f0.txt")).unwrap();
    cx.simulate_prompt_answer("Move to Recycle Bin");
    let notice = wait_notice(cx, handle, &view).await;
    assert_eq!(
        notice,
        "Nothing left to delete: the folders changed while the question was open."
    );
    assert!(trashed.lock().unwrap().is_empty());
}

#[gpui_kit::test]
async fn delete_with_nothing_to_take_says_why(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("d/same.txt"), "s");
    put(&r.path().join("d/same.txt"), "s");
    put(&l.path().join("d/gone.txt"), "g");
    put(&r.path().join("only.txt"), "o");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    fake_trash(cx, &view);
    select(&view, cx, "only.txt");
    dispatch(cx, handle, &view, duet_app::folder::view::DeleteLeft);
    cx.run_until_parked();
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx).notice() == Some("Nothing to delete on that side.")
    })
    .await;
    assert!(!cx.has_pending_prompt());
}

#[gpui_kit::test]
async fn touch_sets_the_other_sides_times_for_the_selection(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    put_at(&l.path().join("f0.txt"), "left", 1_650_000_000);
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "f0.txt");
    dispatch(cx, handle, &view, duet_app::folder::view::TouchRight);
    let notice = wait_notice(cx, handle, &view).await;
    assert_eq!(
        notice,
        "Set the modified time of 1 file on the right to match the left."
    );
    let t = |p: &Path| fs::metadata(p).unwrap().modified().unwrap();
    assert_eq!(t(&r.path().join("f0.txt")), t(&l.path().join("f0.txt")));
}

#[gpui_kit::test]
async fn touch_needs_files_on_both_sides(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "new.txt");
    dispatch(cx, handle, &view, duet_app::folder::view::TouchLeft);
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("Touch needs files that exist on both sides.")
    );
}

#[gpui_kit::test]
async fn a_folder_holding_only_hidden_files_says_so(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("d/mine.txt"), "m");
    put(&r.path().join("d/theirs.txt"), "t");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let trashed = fake_trash(cx, &view);
    select(&view, cx, "d");
    // d stays on show for its left-only file; on the right everything in it is hidden.
    view.update(cx, |v, cx| v.set_filter(FolderFilter::LeftOnly, cx));
    dispatch(cx, handle, &view, duet_app::folder::view::DeleteRight);
    let notice = wait_notice(cx, handle, &view).await;
    assert_eq!(
        notice,
        "The selected folders hold only files that are not shown, so nothing was deleted."
    );
    assert!(!cx.has_pending_prompt() && trashed.lock().unwrap().is_empty());
}

#[gpui_kit::test]
async fn an_item_gone_since_the_compare_says_to_compare_again(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    fake_trash(cx, &view);
    select(&view, cx, "f0.txt");
    fs::remove_file(l.path().join("f0.txt")).unwrap();
    dispatch(cx, handle, &view, duet_app::folder::view::DeleteLeft);
    let notice = wait_notice(cx, handle, &view).await;
    assert_eq!(
        notice,
        "The selected items changed on disk since the compare. Press F5 to compare again."
    );
}

#[gpui_kit::test]
async fn a_folder_that_becomes_takeable_while_asking_is_not_taken_whole(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("d/a.txt"), "a");
    put(&r.path().join("keep.txt"), "k");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let trashed = fake_trash(cx, &view);
    // A file the compare never saw keeps the folder from going whole.
    put(&l.path().join("d/late.txt"), "late");
    select(&view, cx, "d");
    dispatch(cx, handle, &view, duet_app::folder::view::DeleteLeft);
    wait_for_prompt(cx, &view).await;
    let prompt = cx.pending_prompt().unwrap();
    assert_eq!(prompt.0, "Move 1 item to the Recycle Bin?", "{prompt:?}");
    fs::remove_file(l.path().join("d/late.txt")).unwrap();
    cx.simulate_prompt_answer("Move to Recycle Bin");
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx).notice().is_some()
    })
    .await;
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("The folders changed while the question was open. Delete again to use the new view.")
    );
    assert!(trashed.lock().unwrap().is_empty());
    assert!(l.path().join("d/a.txt").exists());
}

#[gpui_kit::test]
async fn a_scan_result_landing_while_asking_makes_the_delete_stale(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let trashed = fake_trash(cx, &view);
    select(&view, cx, "f0.txt");
    // The prompt is planned at the epoch of this moment; the running scan's result bumps it.
    view.update(cx, |v, cx| v.refresh(cx));
    dispatch(cx, handle, &view, duet_app::folder::view::DeleteLeft);
    wait_for_prompt(cx, &view).await;
    wait_idle(cx, handle, &view).await;
    cx.simulate_prompt_answer("Move to Recycle Bin");
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx).notice().is_some()
    })
    .await;
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("The folders changed while the question was open. Delete again to use the new view.")
    );
    assert!(trashed.lock().unwrap().is_empty());
}

#[gpui_kit::test]
async fn delete_both_takes_both_sides_from_del_and_from_shift_delete(cx: &mut TestAppContext) {
    for via_shift in [false, true] {
        let (l, r) = wide_fixture();
        let (handle, view) = open_folder(cx, l.path(), r.path());
        wait_idle(cx, handle, &view).await;
        let trashed = fake_trash(cx, &view);
        select(&view, cx, "f1.txt");
        if via_shift {
            press(cx, handle, "shift-delete");
        } else {
            press(cx, handle, "delete");
        }
        wait_for_prompt(cx, &view).await;
        let answer = if via_shift {
            "Move to Recycle Bin"
        } else {
            "Delete both"
        };
        cx.simulate_prompt_answer(answer);
        let notice = wait_notice(cx, handle, &view).await;
        assert_eq!(notice, "Moved 2 items to the Recycle Bin.");
        let mut seen = trashed.lock().unwrap().clone();
        seen.sort();
        let mut want = vec![l.path().join("f1.txt"), r.path().join("f1.txt")];
        want.sort();
        assert_eq!(seen, want, "via_shift={via_shift}");
    }
}

#[gpui_kit::test]
async fn a_folder_whose_shown_file_vanished_says_to_compare_again(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("d/a.txt"), "a");
    put(&r.path().join("keep.txt"), "k");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    fake_trash(cx, &view);
    select(&view, cx, "d");
    fs::remove_file(l.path().join("d/a.txt")).unwrap();
    dispatch(cx, handle, &view, duet_app::folder::view::DeleteLeft);
    let notice = wait_notice(cx, handle, &view).await;
    assert_eq!(
        notice,
        "The selected items changed on disk since the compare. Press F5 to compare again."
    );
}

/// Runs the item called `label` of the row menu the way the popup would.
fn run_menu_item(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<FolderView>,
    label: &str,
) -> bool {
    cx.update_window(handle, |_, window, cx| {
        let entries = duet_app::folder::menu::row_menu(view.read(cx));
        let focus = view.focus_handle(cx);
        duet_app::ui::menu::activate(entries, label, Some(&focus), window, cx)
    })
    .unwrap()
}

fn menu_outline(view: &Entity<FolderView>, cx: &mut TestAppContext) -> Vec<String> {
    view.read_with(cx, |v, _| {
        duet_app::ui::menu::outline(&duet_app::folder::menu::row_menu(v))
    })
}

/// A right press and release at `position`. The open popup blocks events under it, so tests
/// press clear of it (left of the row centre, or near the list's bottom edge).
fn right_press(window: &mut Window, cx: &mut App, position: Point<gpui_kit::Pixels>) {
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
}

fn row_left(window: &mut Window, cx: &mut App, row: usize) -> Point<gpui_kit::Pixels> {
    window.render_frame(cx);
    let b = window.find(("folder-row", row)).bounds();
    Point {
        x: b.origin.x + px(40.),
        y: b.center().y,
    }
}

#[gpui_kit::test]
async fn right_click_outside_the_selection_selects_the_row_inside_keeps_it(
    cx: &mut TestAppContext,
) {
    let (l, r) = wide_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.click(("folder-row", 0usize), cx);
        click_mod(window, cx, ("folder-row", 1usize), None, ctrl());
        window.right_click(("folder-row", 1usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.chosen_rels_for_test()).len(),
        2,
        "inside: kept"
    );
    assert!(!view.read_with(cx, |v, _| v.menu_entries().is_empty()));
    cx.update_window(handle, |_, window, cx| {
        let at = row_left(window, cx, 4);
        right_press(window, cx, at);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.chosen_rels_for_test()),
        ["f4.txt"],
        "outside: replaced"
    );
}

#[gpui_kit::test]
async fn right_click_inside_the_selection_acts_on_the_pressed_row(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let opened: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
    let sink = opened.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &FolderEvent, _| {
            if let FolderEvent::OpenFiles { left, .. } = event {
                sink.borrow_mut().push(left.clone());
            }
        })
        .detach()
    });
    cx.update_window(handle, |_, window, cx| {
        window.click(("folder-row", 0usize), cx);
        click_mod(window, cx, ("folder-row", 1usize), None, ctrl());
        // Toggling row 0 off leaves the cursor on it while only row 1 stays chosen.
        click_mod(window, cx, ("folder-row", 0usize), None, ctrl());
        window.right_click(("folder-row", 1usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.chosen_rels_for_test()),
        ["f1.txt"]
    );
    assert!(run_menu_item(cx, handle, &view, "Open compare"));
    cx.run_until_parked();
    assert_eq!(*opened.borrow(), [l.path().join("f1.txt")]);
}

#[gpui_kit::test]
async fn right_click_on_blank_space_opens_no_menu_and_keeps_the_selection(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.right_click(("folder-row", 2usize), cx);
    })
    .unwrap();
    assert!(!view.read_with(cx, |v, _| v.menu_entries().is_empty()));
    // Six rows fill 144 px of a much taller list. The open popup covers the middle of it, so
    // press at the very bottom edge, left of the popup (it opens at the pressed point).
    cx.update_window(handle, |_, window, cx| {
        let b = window.find("folder-list").bounds();
        let at = Point {
            x: b.left() + px(4.),
            y: b.bottom() - px(4.),
        };
        right_press(window, cx, at);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        view.read_with(cx, |v, _| v.menu_entries().is_empty()),
        "a stale target would open a menu for row 2"
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.chosen_rels_for_test()),
        ["f2.txt"]
    );
    // T-62: no popup takes the keys, so Down still moves the selection.
    press(cx, handle, "down");
    assert_eq!(
        view.read_with(cx, |v, _| v.chosen_rels_for_test()),
        ["f3.txt"]
    );
}

#[gpui_kit::test]
async fn the_row_menu_matches_the_selection(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "b.txt");
    assert_eq!(
        menu_outline(&view, cx),
        [
            "Open compare",
            "Open as text",
            "Open as hex",
            "Open as table",
            "Open as picture",
            "-",
            "Copy to right",
            "Copy to left",
            "Sync selected…",
            "-",
            "Delete left (to Recycle Bin)…",
            "Delete right (to Recycle Bin)…",
            "Delete both (to Recycle Bin)…",
            "-",
            "Set right modified time = left",
            "Set left modified time = right",
            "-",
            "Show left in Explorer",
            "Show right in Explorer",
            "Open left with default app",
            "Open right with default app",
            "Copy path",
            "-",
            "Exclude this item",
        ]
    );
    select(&view, cx, "new.txt");
    let items = menu_outline(&view, cx);
    for want in [
        "Open compare",
        "Open as text",
        "(Open as hex)",
        "(Open as table)",
        "(Open as picture)",
        "(Copy to right)",
        "Copy to left",
        "Sync selected…",
        "(Delete left (to Recycle Bin)…)",
        "(Delete both (to Recycle Bin)…)",
        "(Set right modified time = left)",
        "(Show left in Explorer)",
        "Show right in Explorer",
        "(Open left with default app)",
        "Open right with default app",
        "Copy path",
        "Exclude this item",
    ] {
        assert!(items.iter().any(|i| i == want), "{want}: {items:?}");
    }
    select(&view, cx, "sub");
    let items = menu_outline(&view, cx);
    assert!(
        items.contains(&"Collapse folder".to_owned())
            || items.contains(&"Expand folder".to_owned())
    );
    assert!(
        !items
            .iter()
            .any(|i| i.contains("Open compare") || i.contains("default app"))
    );
    assert!(
        items.contains(&"(Set this folder as base)".to_owned()),
        "left-only folder"
    );
    assert!(items.contains(&"(Copy to left)".to_owned()));
    let copy_left = items.iter().position(|i| i == "(Copy to left)").unwrap();
    assert_eq!(items[copy_left + 1], "Sync selected…", "{items:?}");
}

#[gpui_kit::test]
async fn the_row_menu_is_off_while_a_batch_runs(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "b.txt");
    view.update(cx, |v, cx| v.hold_batch_for_test("copy", 1, cx));
    let items = menu_outline(&view, cx);
    for want in [
        "(Copy to right)",
        "(Delete both (to Recycle Bin)…)",
        "(Set right modified time = left)",
    ] {
        assert!(items.iter().any(|i| i == want), "{want}: {items:?}");
    }
    assert!(!run_menu_item(cx, handle, &view, "Copy to right"));
}

#[gpui_kit::test]
async fn menu_items_copy_delete_and_toggle(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    fake_trash(cx, &view);
    select(&view, cx, "b.txt");
    assert!(run_menu_item(cx, handle, &view, "Copy to right"));
    wait_for_prompt(cx, &view).await;
    assert!(cx.pending_prompt().unwrap().0.starts_with("Copy 1 file"));
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(run_menu_item(
        cx,
        handle,
        &view,
        "Delete left (to Recycle Bin)…"
    ));
    wait_for_prompt(cx, &view).await;
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(l.path().join("b.txt").exists(), "Cancel kept the file");
    select(&view, cx, "sub");
    let before = rels(&view, cx);
    let label = if menu_outline(&view, cx).contains(&"Collapse folder".to_owned()) {
        "Collapse folder"
    } else {
        "Expand folder"
    };
    assert!(run_menu_item(cx, handle, &view, label));
    assert_ne!(rels(&view, cx), before, "{label} changed the rows");
}

#[gpui_kit::test]
async fn menu_copy_path_gives_the_left_path_else_the_right(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let clip =
        |cx: &mut TestAppContext| cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
    select(&view, cx, "b.txt");
    assert!(run_menu_item(cx, handle, &view, "Copy path"));
    assert_eq!(
        clip(cx).as_deref(),
        Some(l.path().join("b.txt").to_str().unwrap())
    );
    select(&view, cx, "new.txt");
    assert!(run_menu_item(cx, handle, &view, "Copy path"));
    assert_eq!(
        clip(cx).as_deref(),
        Some(r.path().join("new.txt").to_str().unwrap())
    );
    cx.update_window(handle, |_, window, cx| {
        click_mod(window, cx, ("folder-row", 0usize), None, Default::default());
        click_mod(window, cx, ("folder-row", 2usize), None, shift());
    })
    .unwrap();
    assert!(run_menu_item(cx, handle, &view, "Copy path"));
    let text = clip(cx).unwrap();
    assert_eq!(
        text.matches("\r\n").count(),
        2,
        "one line per item: {text:?}"
    );
}

fn nested_fixture() -> (tempfile::TempDir, tempfile::TempDir) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("d/x.txt"), "1");
    put(&r.path().join("d/x.txt"), "2");
    put(&l.path().join("keep.txt"), "1");
    put(&r.path().join("keep.txt"), "2");
    (l, r)
}

#[gpui_kit::test]
async fn menu_set_as_base_goes_into_the_folder_on_both_sides(cx: &mut TestAppContext) {
    let (l, r) = nested_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "d");
    assert!(run_menu_item(cx, handle, &view, "Set this folder as base"));
    wait_idle(cx, handle, &view).await;
    let (left, right) = view.read_with(cx, |v, _| {
        let (a, b) = v.paths();
        (a.clone(), b.clone())
    });
    assert_eq!((left, right), (l.path().join("d"), r.path().join("d")));
    view.update(cx, |v, cx| v.set_filter(FolderFilter::All, cx));
    assert_eq!(rels(&view, cx), ["x.txt"]);
}

#[gpui_kit::test]
async fn menu_exclude_adds_the_item_to_the_mask_and_compares_again(cx: &mut TestAppContext) {
    let (l, r) = nested_fixture();
    put(&l.path().join("sub/d/y.txt"), "1");
    put(&r.path().join("sub/d/y.txt"), "2");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "d");
    assert!(run_menu_item(cx, handle, &view, "Exclude this item"));
    wait_idle(cx, handle, &view).await;
    let shown = rels(&view, cx);
    assert!(!shown.iter().any(|r| r == "d"), "d is excluded: {shown:?}");
    assert!(shown.iter().any(|r| r == "keep.txt"));
    assert!(
        shown.iter().any(|r| r == "sub/d"),
        "a top-level mask keeps the same name deeper down: {shown:?}"
    );
    let mask = view.read_with(cx, |v, cx| v.exclude_input().read(cx).value().to_string());
    assert_eq!(mask, "/d");
    select(&view, cx, "keep.txt");
    assert!(run_menu_item(cx, handle, &view, "Exclude this item"));
    wait_idle(cx, handle, &view).await;
    let mask = view.read_with(cx, |v, cx| v.exclude_input().read(cx).value().to_string());
    assert_eq!(mask, "/d, /keep.txt");
    select(&view, cx, "sub/d");
    assert!(run_menu_item(cx, handle, &view, "Exclude this item"));
    wait_idle(cx, handle, &view).await;
    let mask = view.read_with(cx, |v, cx| v.exclude_input().read(cx).value().to_string());
    assert_eq!(mask, "/d, /keep.txt, sub/d");
}

#[gpui_kit::test]
async fn menu_touch_and_delete_items_run_on_the_selection(cx: &mut TestAppContext) {
    let (l, r) = wide_fixture();
    put_at(&l.path().join("f0.txt"), "left", 1_650_000_000);
    put_at(&r.path().join("f1.txt"), "right!", 1_650_000_000);
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let trashed = fake_trash(cx, &view);
    let t = |p: &Path| fs::metadata(p).unwrap().modified().unwrap();
    select(&view, cx, "f0.txt");
    assert!(run_menu_item(
        cx,
        handle,
        &view,
        "Set right modified time = left"
    ));
    wait_until(cx, handle, || {
        t(&r.path().join("f0.txt")) == t(&l.path().join("f0.txt"))
    })
    .await;
    select(&view, cx, "f1.txt");
    assert!(run_menu_item(
        cx,
        handle,
        &view,
        "Set left modified time = right"
    ));
    wait_until(cx, handle, || {
        t(&l.path().join("f1.txt")) == t(&r.path().join("f1.txt"))
    })
    .await;
    select(&view, cx, "f2.txt");
    assert!(run_menu_item(
        cx,
        handle,
        &view,
        "Delete right (to Recycle Bin)…"
    ));
    wait_for_prompt(cx, &view).await;
    cx.simulate_prompt_answer("Move to Recycle Bin");
    wait_until(cx, handle, || !r.path().join("f2.txt").exists()).await;
    assert!(l.path().join("f2.txt").exists());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "f3.txt");
    assert!(run_menu_item(
        cx,
        handle,
        &view,
        "Delete both (to Recycle Bin)…"
    ));
    wait_for_prompt(cx, &view).await;
    cx.simulate_prompt_answer("Move to Recycle Bin");
    wait_until(cx, handle, || {
        !l.path().join("f3.txt").exists() && !r.path().join("f3.txt").exists()
    })
    .await;
    assert_eq!(trashed.lock().unwrap().len(), 3);
}

/// Polls `done` (a disk check) while the app runs.
async fn wait_until(cx: &mut TestAppContext, handle: AnyWindowHandle, done: impl Fn() -> bool) {
    cx.wait_for(handle, Duration::from_secs(20), move |_, _| done())
        .await;
}

#[cfg(windows)]
#[gpui_kit::test]
async fn set_as_base_is_off_for_a_folder_that_is_a_junction(cx: &mut TestAppContext) {
    let (l, r, outside) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    put(&l.path().join("lib/a.txt"), "1");
    mklink_junction(&r.path().join("lib"), outside.path());
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "lib");
    let items = menu_outline(&view, cx);
    assert!(
        items.contains(&"(Set this folder as base)".to_owned()),
        "{items:?}"
    );
    assert!(!run_menu_item(cx, handle, &view, "Set this folder as base"));
}

#[gpui_kit::test]
async fn ctrl_r_and_ctrl_l_copy_to_the_right_and_left(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "b.txt");
    press(cx, handle, "ctrl-r");
    wait_for_prompt(cx, &view).await;
    assert!(cx.pending_prompt().unwrap().0.starts_with("Copy 1 file"));
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    press(cx, handle, "ctrl-l");
    wait_for_prompt(cx, &view).await;
    assert!(cx.pending_prompt().unwrap().0.starts_with("Copy 1 file"));
}

async fn assert_no_prompt_appears(cx: &mut TestAppContext) {
    for _ in 0..30 {
        assert!(!cx.has_pending_prompt(), "a prompt opened");
        cx.executor().timer(Duration::from_millis(10)).await;
    }
}

#[gpui_kit::test]
async fn copy_keys_in_the_name_filter_open_no_prompt(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "b.txt");
    cx.update_window(handle, |_, window, cx| {
        let input = view.read(cx).name_filter_input().clone();
        window.focus(&input.focus_handle(cx), cx);
        window.press("ctrl-l", cx);
    })
    .unwrap();
    assert_no_prompt_appears(cx).await;
}

#[gpui_kit::test]
async fn copy_keys_in_the_path_edit_open_no_prompt(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "b.txt");
    cx.update_window(handle, |_, window, cx| window.click("folder-left-path", cx))
        .unwrap();
    assert!(view.read_with(cx, |v, _| v.path_edit_for_test().is_some()));
    press(cx, handle, "ctrl-r");
    assert_no_prompt_appears(cx).await;
}

#[gpui_kit::test]
async fn running_a_menu_item_forgets_the_pressed_row(cx: &mut TestAppContext) {
    // A fresh window per item: an open popup covers the row of the next press.
    for label in ["Copy path", "Copy to right"] {
        let (l, r) = fixture();
        let (handle, view) = open_folder(cx, l.path(), r.path());
        wait_idle(cx, handle, &view).await;
        cx.update_window(handle, |_, window, cx| {
            let at = row_left(window, cx, 0);
            right_press(window, cx, at);
        })
        .unwrap();
        assert!(!view.read_with(cx, |v, _| v.menu_entries().is_empty()));
        assert!(run_menu_item(cx, handle, &view, label));
        assert!(
            view.read_with(cx, |v, _| v.menu_entries().is_empty()),
            "{label}"
        );
    }
}

#[gpui_kit::test]
async fn a_typed_folder_path_with_thai_reroots_and_esc_restores(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let thai = l.path().join("โฟลเดอร์ใหม่");
    put(&thai.join("z.txt"), "z");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.click("folder-left-path", cx))
        .unwrap();
    assert!(view.read_with(cx, |v, _| v.path_edit_for_test().is_some()));
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    assert!(
        view.read_with(cx, |v, _| v.path_edit_for_test().is_none()),
        "Esc restores the bar"
    );
    cx.update_window(handle, |_, window, cx| window.click("folder-left-path", cx))
        .unwrap();
    view.update(cx, |v, cx| {
        v.set_path_edit_text_for_test(&format!("\"{}\"", thai.to_str().unwrap()), cx)
    });
    cx.update_window(handle, |_, window, cx| window.press("enter", cx))
        .unwrap();
    wait_idle(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.paths().0.clone()), thai);
    assert!(view.read_with(cx, |v, _| v.path_edit_for_test().is_none()));
}

#[gpui_kit::test]
async fn a_typed_path_that_is_not_a_folder_keeps_the_input_open(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.click("folder-left-path", cx))
        .unwrap();
    let file = l.path().join("a.txt");
    view.update(cx, |v, cx| {
        v.set_path_edit_text_for_test(file.to_str().unwrap(), cx)
    });
    cx.update_window(handle, |_, window, cx| window.press("enter", cx))
        .unwrap();
    assert!(view.read_with(cx, |v, _| v.path_edit_for_test().is_some()));
    let notice = view
        .read_with(cx, |v, _| v.notice().map(str::to_owned))
        .unwrap();
    assert!(notice.starts_with("Not a folder:"), "{notice}");
}

#[gpui_kit::test]
async fn dropping_one_folder_replaces_the_half_under_it_and_two_replace_both(
    cx: &mut TestAppContext,
) {
    let (l, r) = fixture();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.drop_paths(&[a.path().to_path_buf()], false, cx)
    });
    wait_idle(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.paths().1.clone()), a.path());
    view.update(cx, |v, cx| {
        v.drop_paths(&[a.path().to_path_buf(), b.path().to_path_buf()], true, cx)
    });
    wait_idle(cx, handle, &view).await;
    let (left, right) = view.read_with(cx, |v, _| (v.paths().0.clone(), v.paths().1.clone()));
    assert_eq!((left.as_path(), right.as_path()), (a.path(), b.path()));
}

#[gpui_kit::test]
async fn typed_paths_f9_and_drops_wait_for_a_running_batch(cx: &mut TestAppContext) {
    // A fresh window per case, so each notice is its own.
    let mut wrong = Vec::new();
    for case in ["typed path", "F9", "drop"] {
        let (l, r) = fixture();
        let other = tempfile::tempdir().unwrap();
        let (handle, view) = open_folder(cx, l.path(), r.path());
        wait_idle(cx, handle, &view).await;
        view.update(cx, |v, cx| v.hold_batch_for_test("Copying", 1, cx));
        match case {
            "typed path" => {
                cx.update_window(handle, |_, window, cx| window.click("folder-left-path", cx))
                    .unwrap();
                view.update(cx, |v, cx| {
                    v.set_path_edit_text_for_test(other.path().to_str().unwrap(), cx)
                });
                cx.update_window(handle, |_, window, cx| window.press("enter", cx))
                    .unwrap();
            }
            "F9" => press(cx, handle, "f9"),
            _ => view.update(cx, |v, cx| {
                v.drop_paths(&[other.path().to_path_buf()], true, cx)
            }),
        }
        let (left, right) = view.read_with(cx, |v, _| (v.paths().0.clone(), v.paths().1.clone()));
        let notice = view.read_with(cx, |v, _| v.notice().map(str::to_owned));
        if (left.as_path(), right.as_path()) != (l.path(), r.path())
            || notice.as_deref() != Some("Wait for the running copy or delete to finish.")
        {
            wrong.push(format!("{case}: {left:?} {right:?} {notice:?}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[gpui_kit::test]
async fn a_relative_typed_folder_path_is_refused(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.click("folder-left-path", cx))
        .unwrap();
    // "." is a folder relative to the working directory, which a typed path must not use.
    view.update(cx, |v, cx| v.set_path_edit_text_for_test(".", cx));
    cx.update_window(handle, |_, window, cx| window.press("enter", cx))
        .unwrap();
    assert!(view.read_with(cx, |v, _| v.path_edit_for_test().is_some()));
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned)),
        Some("Type a full path, such as C:\\folder.".to_owned())
    );
    assert_eq!(view.read_with(cx, |v, _| v.paths().0.clone()), l.path());
}

#[gpui_kit::test]
async fn dropping_a_file_gets_a_notice_and_changes_nothing(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let file = l.path().join("a.txt");
    view.update(cx, |v, cx| v.drop_paths(&[file], true, cx));
    let notice = view
        .read_with(cx, |v, _| v.notice().map(str::to_owned))
        .unwrap();
    assert_eq!(notice, "Drop folders here; files open in a text compare.");
    assert_eq!(view.read_with(cx, |v, _| v.paths().0.clone()), l.path());
}

#[gpui_kit::test]
async fn a_copy_of_only_empty_folders_says_created_n_folders(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for dir in ["e1", "e2"] {
        fs::create_dir_all(l.path().join(dir)).unwrap();
    }
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    press(cx, handle, "ctrl-a");
    press(cx, handle, "alt-right");
    wait_for_prompt(cx, &view).await;
    cx.simulate_prompt_answer("Copy");
    let notice = wait_notice(cx, handle, &view).await;
    assert_eq!(notice, "Created 2 folders.");
    assert!(r.path().join("e2").is_dir());
}

#[gpui_kit::test]
async fn the_delete_skip_count_is_the_changed_count(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("f0.txt"), "left");
    fs::create_dir_all(l.path().join("d/e")).unwrap();
    put(&r.path().join("other.txt"), "o");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let trashed = fake_trash(cx, &view);
    press(cx, handle, "ctrl-a");
    dispatch(cx, handle, &view, duet_app::folder::view::DeleteLeft);
    wait_for_prompt(cx, &view).await;
    // A file the compare never saw lands in the empty folder: d stays whole, but no listed item
    // changed on disk, so the notice must not say so.
    put(&l.path().join("d/e/new.txt"), "n");
    cx.simulate_prompt_answer("Move to Recycle Bin");
    let notice = wait_notice(cx, handle, &view).await;
    assert_eq!(notice, "Moved 1 item to the Recycle Bin.");
    assert_eq!(trashed.lock().unwrap().len(), 1);
    assert!(l.path().join("d/e/new.txt").exists());
}

#[cfg(windows)]
#[gpui_kit::test]
async fn typed_paths_are_normalised(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let base = tempfile::tempdir().unwrap();
    for dir in ["a", "b", "sub"] {
        fs::create_dir_all(base.path().join(dir)).unwrap();
    }
    let root = base.path().to_str().unwrap().to_owned();
    for (typed, want) in [
        (
            format!("{}/sub", root.replace('\\', "/")),
            format!("{root}\\sub"),
        ),
        (format!("{root}\\a\\..\\b"), format!("{root}\\b")),
    ] {
        cx.update_window(handle, |_, window, cx| window.click("folder-left-path", cx))
            .unwrap();
        view.update(cx, |v, cx| v.set_path_edit_text_for_test(&typed, cx));
        cx.update_window(handle, |_, window, cx| window.press("enter", cx))
            .unwrap();
        wait_idle(cx, handle, &view).await;
        let shown = view.read_with(cx, |v, _| v.paths().0.to_str().unwrap().to_owned());
        assert_eq!(shown, want, "typed {typed}");
    }
}
