//! Headless UI tests for the folder sync panel (P10a Task 10): the preview, the questions
//! Start asks, and the guards that keep a sync from acting on a stale or nested compare.

mod folder_support;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use duet_app::folder::job::shallow;
use duet_app::folder::sync_panel::{REPLACE_ANSWERS, TRASH_ANSWERS, sync_lines, sync_summary};
use duet_app::folder::view::{FolderEvent, FolderView};
use duet_core::folder::run::{FolderOptions, compare_folders};
use duet_core::folder::sync::SyncMode;
use duet_core::folder::tree::Status;
use folder_support::{
    T0, assert_no_prompt_appears, fixture, menu_outline, open_folder, press, put_at, rels,
    remove_in_place, run_menu_item, select, sync_fixture, use_settings, wait_for_prompt, wait_idle,
};
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{AnyWindowHandle, App, AppContext, Entity, Focusable, TestAppContext, Window};

const COMPARED_AGAIN: &str = "The folders were compared again, so the sync was closed.";

fn click(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn present(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
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

fn label(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> Option<String> {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find(id).label().map(str::to_owned)
    })
    .unwrap()
}

fn tab_to(window: &mut Window, cx: &mut App, id: &'static str) {
    for _ in 0..40 {
        if window.find(id).focused() == Some(true) {
            return;
        }
        window.press("tab", cx);
    }
    panic!("Tab never reached {id}");
}

async fn wait_planned(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<FolderView>) {
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx).sync_plan_shown().is_some()
    })
    .await;
}

/// Opens the panel from the toolbar and waits for its plan.
async fn open_panel(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<FolderView>) {
    click(cx, handle, "folder-sync");
    assert!(view.read_with(cx, |v, _| v.sync_open()), "the panel opened");
    wait_planned(cx, handle, view).await;
}

/// Picks a mode card and waits for its plan.
async fn pick_mode(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<FolderView>,
    id: &'static str,
) {
    click(cx, handle, id);
    wait_planned(cx, handle, view).await;
}

fn summary(cx: &mut TestAppContext, view: &Entity<FolderView>) -> String {
    view.read_with(cx, |v, _| sync_summary(v.sync_plan_shown().unwrap()))
}

fn lines(cx: &mut TestAppContext, view: &Entity<FolderView>, l: &Path, r: &Path) -> Vec<String> {
    view.read_with(cx, |v, _| {
        sync_lines(v.sync_plan_shown().unwrap(), l, r, v.options().mode)
    })
}

fn notice(cx: &mut TestAppContext, view: &Entity<FolderView>) -> Option<String> {
    view.read_with(cx, |v, _| v.notice().map(str::to_owned))
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
    notice(cx, view).unwrap()
}

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(path).unwrap()
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

fn panel_focused(cx: &mut TestAppContext, handle: AnyWindowHandle) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find("folder-sync-panel").focused() == Some(true)
    })
    .unwrap()
}

/// A Recycle Bin stand-in that removes in place and logs every call.
fn fake_trash(cx: &mut TestAppContext, view: &Entity<FolderView>) -> Arc<Mutex<Vec<PathBuf>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    view.update(cx, |v, _| {
        v.set_trash_for_test(move |path: &Path| {
            log.lock().unwrap().push(path.to_path_buf());
            remove_in_place(path)
        })
    });
    seen
}

#[gpui_kit::test]
async fn sync_opens_with_a_live_preview_and_recomputes_on_mode_change(cx: &mut TestAppContext) {
    let (l, r) = sync_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    assert!(!present(cx, handle, "folder-sync-panel"));
    click(cx, handle, "folder-sync");
    assert!(view.read_with(cx, |v, _| v.sync_open()));
    assert_eq!(
        view.read_with(cx, |v, _| v.sync_mode()),
        Some(SyncMode::UpdateRight)
    );
    assert!(present(cx, handle, "folder-sync-panel"));
    wait_planned(cx, handle, &view).await;
    let update_right = "Copy 2 files (8 bytes; 1 replaces an existing file) · Move 0 items to the Recycle Bin · Skip 2";
    assert_eq!(summary(cx, &view), update_right);
    assert_eq!(
        label(cx, handle, "folder-sync-summary").as_deref(),
        Some(update_right),
        "the panel shows the summary"
    );
    assert_eq!(
        lines(cx, &view, l.path(), r.path()),
        [
            "→ newer.txt (replaces)",
            "→ new.txt",
            "· older.txt — the right is newer",
            "· same-time.txt — the modified times are equal or near: can't tell which is newer",
        ]
    );
    assert_eq!(
        selected(cx, handle, "folder-sync-mode-update-right"),
        Some(true)
    );
    assert_eq!(
        selected(cx, handle, "folder-sync-mode-mirror-right"),
        Some(false)
    );
    click(cx, handle, "folder-sync-mode-mirror-right");
    assert_eq!(
        view.read_with(cx, |v, _| v.sync_mode()),
        Some(SyncMode::MirrorRight)
    );
    assert!(
        view.read_with(cx, |v, _| v.sync_plan_shown().is_none()),
        "the plan is worked out again"
    );
    assert_eq!(
        label(cx, handle, "folder-sync-summary").as_deref(),
        Some("Working out what to do…")
    );
    wait_planned(cx, handle, &view).await;
    assert_eq!(
        summary(cx, &view),
        "Copy 4 files (10 bytes; 3 replace existing files) · Move 1 item to the Recycle Bin"
    );
    assert_eq!(
        lines(cx, &view, l.path(), r.path())
            .last()
            .map(String::as_str),
        Some("✕ extra.txt")
    );
    assert_eq!(
        selected(cx, handle, "folder-sync-mode-mirror-right"),
        Some(true)
    );
    for id in [
        "folder-sync-mode-update-left",
        "folder-sync-mode-update-both",
        "folder-sync-mode-mirror-left",
        "folder-sync-list",
        "folder-sync-cancel",
        "folder-sync-start",
    ] {
        assert!(present(cx, handle, id), "{id}");
    }
}

#[gpui_kit::test]
async fn start_sync_copies_files_and_refreshes(cx: &mut TestAppContext) {
    let (l, r) = sync_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    open_panel(cx, handle, &view).await;
    click(cx, handle, "folder-sync-start");
    // PF1: the overwrite of newer.txt is asked about first, "Cancel" first.
    wait_for_prompt(cx, &view).await;
    let (message, detail) = cx.pending_prompt().unwrap();
    assert_eq!(message, "Replace 1 file?");
    assert!(
        detail.starts_with("Overwrites 1 file on the right."),
        "{detail}"
    );
    assert!(
        detail.contains(&r.path().join("newer.txt").display().to_string()),
        "{detail}"
    );
    assert_eq!(REPLACE_ANSWERS, ["Cancel", "Replace"]);
    cx.simulate_prompt_answer("Replace");
    assert_eq!(
        wait_notice(cx, handle, &view).await,
        "Synced: copied 2 files."
    );
    assert_eq!(read(r.path().join("new.txt")), "n");
    assert_eq!(read(r.path().join("newer.txt")), "L-newer");
    assert_eq!(read(r.path().join("older.txt")), "R-older");
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
    let status = view.read_with(cx, |v, _| {
        let cmp = v.model().unwrap().comparison();
        let node = cmp.nodes.iter().find(|n| n.rel == "new.txt").unwrap();
        (node.right.is_some(), node.status.clone())
    });
    assert_eq!(status, (true, Status::Same), "the tree was compared again");

    // A sync that replaces nothing asks nothing.
    put_at(&l.path().join("added.txt"), "a", T0);
    view.update(cx, |v, cx| v.refresh(cx));
    wait_idle(cx, handle, &view).await;
    open_panel(cx, handle, &view).await;
    assert_eq!(
        summary(cx, &view),
        "Copy 1 file (1 byte) · Move 0 items to the Recycle Bin · Skip 2"
    );
    click(cx, handle, "folder-sync-start");
    assert_no_prompt_appears(cx).await;
    assert_eq!(
        wait_notice(cx, handle, &view).await,
        "Synced: copied 1 file."
    );
    assert_eq!(read(r.path().join("added.txt")), "a");
}

#[gpui_kit::test]
async fn mirror_asks_before_the_recycle_bin(cx: &mut TestAppContext) {
    let (l, r) = sync_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let trashed = fake_trash(cx, &view);
    open_panel(cx, handle, &view).await;
    pick_mode(cx, handle, &view, "folder-sync-mode-mirror-right").await;
    click(cx, handle, "folder-sync-start");
    wait_for_prompt(cx, &view).await;
    assert_eq!(cx.pending_prompt().unwrap().0, "Replace 3 files?");
    cx.simulate_prompt_answer("Replace");
    wait_for_prompt(cx, &view).await;
    let (message, detail) = cx.pending_prompt().unwrap();
    assert_eq!(message, "Move 1 item on the right to the Recycle Bin?");
    assert_eq!(detail, r.path().join("extra.txt").display().to_string());
    assert_eq!(TRASH_ANSWERS, ["Cancel", "Move to Recycle Bin"]);
    cx.simulate_prompt_answer("Move to Recycle Bin");
    assert_eq!(
        wait_notice(cx, handle, &view).await,
        "Synced: copied 4 files and moved 1 item to the Recycle Bin."
    );
    assert!(!r.path().join("extra.txt").exists());
    assert_eq!(
        trashed.lock().unwrap().as_slice(),
        [r.path().join("extra.txt")]
    );
    assert_eq!(read(r.path().join("older.txt")), "L");
    assert_eq!(read(r.path().join("same-time.txt")), "L");
}

/// What the sync fixture's right side holds before any sync.
fn right_is_untouched(r: &Path) {
    assert_eq!(read(r.join("extra.txt")), "x");
    assert_eq!(read(r.join("newer.txt")), "R");
    assert_eq!(read(r.join("older.txt")), "R-older");
    assert_eq!(read(r.join("same-time.txt")), "R!");
    assert!(!r.join("new.txt").exists());
}

#[gpui_kit::test]
async fn cancel_at_the_recycle_bin_question_changes_nothing(cx: &mut TestAppContext) {
    let (l, r) = sync_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let trashed = fake_trash(cx, &view);
    open_panel(cx, handle, &view).await;
    pick_mode(cx, handle, &view, "folder-sync-mode-mirror-right").await;
    click(cx, handle, "folder-sync-start");
    wait_for_prompt(cx, &view).await;
    cx.simulate_prompt_answer("Replace");
    wait_for_prompt(cx, &view).await;
    assert!(cx.pending_prompt().unwrap().0.starts_with("Move 1 item"));
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    right_is_untouched(r.path());
    assert!(trashed.lock().unwrap().is_empty());
    assert!(view.read_with(cx, |v, _| v.sync_open() && !v.batch_running()));
    // "Cancel" at the first question stops it as well, before the second.
    click(cx, handle, "folder-sync-start");
    wait_for_prompt(cx, &view).await;
    cx.simulate_prompt_answer("Cancel");
    assert_no_prompt_appears(cx).await;
    right_is_untouched(r.path());
    assert!(view.read_with(cx, |v, _| v.sync_open()));
}

/// F-100: the question named the right-only folder `gone2`; a file appears in it before the
/// run. The folder stays whole, and the result and the failure list name it as changed since
/// the compare.
#[gpui_kit::test]
async fn a_folder_that_gains_a_file_after_the_question_stays_and_is_named(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(&l.path().join("keep.txt"), "k", T0);
    put_at(&r.path().join("keep.txt"), "k", T0);
    put_at(&r.path().join("gone2/h.txt"), "h", T0);
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let trashed = fake_trash(cx, &view);
    open_panel(cx, handle, &view).await;
    pick_mode(cx, handle, &view, "folder-sync-mode-mirror-right").await;
    assert_eq!(lines(cx, &view, l.path(), r.path()), ["✕ gone2/"]);
    click(cx, handle, "folder-sync-start");
    wait_for_prompt(cx, &view).await;
    assert_eq!(
        cx.pending_prompt().unwrap().0,
        "Move 1 item on the right to the Recycle Bin?"
    );
    let gone2 = r.path().join("gone2");
    put_at(&gone2.join("h2.txt"), "added after the question", T0);
    cx.simulate_prompt_answer("Move to Recycle Bin");
    let changed = format!("{}: Changed since the compare.", gone2.display());
    assert_eq!(
        wait_notice(cx, handle, &view).await,
        format!("Synced: moved 0 items to the Recycle Bin; 1 failed: {changed}")
    );
    assert!(trashed.lock().unwrap().is_empty(), "nothing in it went");
    assert_eq!(read(gone2.join("h.txt")), "h");
    wait_for_prompt(cx, &view).await;
    assert_eq!(
        cx.pending_prompt().unwrap(),
        ("1 item failed.".to_owned(), changed)
    );
    cx.simulate_prompt_answer("OK");
}

#[gpui_kit::test]
async fn a_compare_landing_while_the_panel_is_open_closes_it(cx: &mut TestAppContext) {
    let (l, r) = sync_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    open_panel(cx, handle, &view).await;
    press(cx, handle, "f5");
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
    wait_idle(cx, handle, &view).await;
    assert!(!present(cx, handle, "folder-sync-panel"));
    assert_eq!(notice(cx, &view).as_deref(), Some(COMPARED_AGAIN));
    assert!(
        rows_focused(cx, handle, &view),
        "the panel had the keys: the rows take them"
    );
    // The format toggle re-derives the verdicts without a compare, and closes it too.
    open_panel(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_format_important(true, cx));
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
    assert_eq!(notice(cx, &view).as_deref(), Some(COMPARED_AGAIN));
}

#[gpui_kit::test]
async fn f9_closes_the_sync_panel(cx: &mut TestAppContext) {
    let (l, r) = sync_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    open_panel(cx, handle, &view).await;
    press(cx, handle, "f9");
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
    assert_eq!(notice(cx, &view).as_deref(), Some(COMPARED_AGAIN));
    // A chip or the path filter changes what the rows show: the panel closes, quietly.
    open_panel(cx, handle, &view).await;
    click(cx, handle, "folder-chip-all");
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
    assert_eq!(notice(cx, &view), None);
    open_panel(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        let input = view.read(cx).name_filter_input().clone();
        window.focus(&input.focus_handle(cx), cx);
        window.input("new", cx);
    })
    .unwrap();
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
}

#[gpui_kit::test]
async fn start_refuses_a_plan_from_an_older_compare(cx: &mut TestAppContext) {
    let (l, r) = sync_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let compare = || {
        compare_folders(
            l.path(),
            r.path(),
            &FolderOptions::default(),
            &Default::default(),
            &duet_core::cancel::CancelToken::new(),
        )
    };
    let land = |v: &mut FolderView, cx: &mut gpui_kit::Context<FolderView>, result| {
        v.refresh(cx);
        let generation = v.job_generation();
        assert!(v.apply_for_test(generation, result, true, cx));
    };
    // A compare lands while Start's question is open: the answer runs nothing.
    open_panel(cx, handle, &view).await;
    click(cx, handle, "folder-sync-start");
    wait_for_prompt(cx, &view).await;
    let result = compare();
    view.update(cx, |v, cx| land(v, cx, result));
    cx.simulate_prompt_answer("Replace");
    cx.run_until_parked();
    right_is_untouched(r.path());
    assert_eq!(notice(cx, &view).as_deref(), Some(COMPARED_AGAIN));
    // With a plan on show, a compare lands, then Start: nothing is copied.
    wait_idle(cx, handle, &view).await;
    open_panel(cx, handle, &view).await;
    let result = compare();
    view.update(cx, |v, cx| land(v, cx, result));
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.start_sync(window, cx))
    })
    .unwrap();
    assert_no_prompt_appears(cx).await;
    right_is_untouched(r.path());
    assert_eq!(notice(cx, &view).as_deref(), Some(COMPARED_AGAIN));
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

/// The junction appears after the compare, so the plan still copies into `lib`: Start's
/// preflight refuses the whole sync before any question or write.
#[cfg(windows)]
#[gpui_kit::test]
async fn start_never_writes_through_a_junction_on_the_destination(cx: &mut TestAppContext) {
    let (l, r, outside) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    put_at(&l.path().join("lib/new.txt"), "new", T0);
    put_at(&l.path().join("top.txt"), "t", T0);
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    open_panel(cx, handle, &view).await;
    assert_eq!(
        lines(cx, &view, l.path(), r.path()),
        ["→ lib/new.txt", "→ top.txt"]
    );
    mklink_junction(&r.path().join("lib"), outside.path());
    click(cx, handle, "folder-sync-start");
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx).notice().is_some()
    })
    .await;
    let text = notice(cx, &view).unwrap();
    assert!(text.starts_with("Nothing was synced: "), "{text}");
    assert!(text.contains("link or junction"), "{text}");
    assert!(!cx.has_pending_prompt());
    assert!(!outside.path().join("new.txt").exists());
    assert!(
        !r.path().join("top.txt").exists(),
        "the whole sync is refused"
    );
    assert!(view.read_with(cx, |v, _| v.sync_open()));
}

#[gpui_kit::test]
async fn sync_is_refused_for_nested_roots(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (outer, inner) = (dir.path().join("l"), dir.path().join("l").join("backup"));
    put_at(&outer.join("app.cfg"), "current\n", T0);
    put_at(&inner.join("app.cfg"), "backup copy\n", T0 + 3600);
    let (handle, view) = open_folder(cx, &outer, &inner);
    wait_idle(cx, handle, &view).await;
    let nested = "One folder is inside the other, so sync is off: it could overwrite or delete its own source.";
    click(cx, handle, "folder-sync");
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
    assert!(!present(cx, handle, "folder-sync-panel"));
    assert_eq!(notice(cx, &view).as_deref(), Some(nested));
    select(&view, cx, "app.cfg");
    assert!(run_menu_item(cx, handle, &view, "Sync selected…"));
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
    assert_eq!(notice(cx, &view).as_deref(), Some(nested));
}

#[gpui_kit::test]
async fn sync_waits_for_a_running_compare_or_batch(cx: &mut TestAppContext) {
    let (l, r) = sync_fixture();
    put_at(&l.path().join("d/x.txt"), "d", T0);
    put_at(&r.path().join("d/x.txt"), "d", T0);
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    // A running compare.
    view.update(cx, |v, cx| v.refresh(cx));
    click(cx, handle, "folder-sync");
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Wait for the compare to finish.")
    );
    // Its first pass landed (d is Pending) and Esc stopped it: there is no result to sync.
    let first = shallow(l.path(), r.path(), &FolderOptions::default());
    view.update(cx, |v, cx| {
        let generation = v.job_generation();
        assert!(v.apply_for_test(generation, first, false, cx));
    });
    press(cx, handle, "escape");
    assert!(!view.read_with(cx, |v, _| v.is_busy()));
    click(cx, handle, "folder-sync");
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Compare the folders first.")
    );
    // M-6: Esc stopped a refresh of a finished result, so the rows on show (complete, with
    // nothing Pending) are the previous result: there is nothing to sync yet.
    view.update(cx, |v, cx| v.refresh(cx));
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.refresh(cx));
    press(cx, handle, "escape");
    assert!(!view.read_with(cx, |v, _| v.is_busy()));
    click(cx, handle, "folder-sync");
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Compare the folders first.")
    );
    // A compare that finishes makes it the result to sync again.
    view.update(cx, |v, cx| v.refresh(cx));
    wait_idle(cx, handle, &view).await;
    click(cx, handle, "folder-sync");
    assert!(view.read_with(cx, |v, _| v.sync_open()));
    click(cx, handle, "folder-sync-cancel");
    // A running batch.
    view.update(cx, |v, cx| v.refresh(cx));
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.hold_batch_for_test("Copying", 1, cx));
    click(cx, handle, "folder-sync");
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Wait for the running copy or delete to finish.")
    );
}

#[gpui_kit::test]
async fn sync_selected_from_the_row_menu_uses_the_selection(cx: &mut TestAppContext) {
    let (l, r) = sync_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "new.txt");
    let items = menu_outline(&view, cx);
    let copy_left = items
        .iter()
        .position(|i| i.ends_with("Copy to left)") || i == "Copy to left")
        .expect("Copy to left");
    assert_eq!(items[copy_left + 1], "Sync selected…", "{items:?}");
    assert!(run_menu_item(cx, handle, &view, "Sync selected…"));
    assert!(view.read_with(cx, |v, _| v.sync_open()));
    assert_eq!(
        selected(cx, handle, "folder-sync-scope-selected"),
        Some(true)
    );
    assert_eq!(
        label(cx, handle, "folder-sync-scope-selected").as_deref(),
        Some("Selected items only (1)")
    );
    wait_planned(cx, handle, &view).await;
    assert_eq!(
        summary(cx, &view),
        "Copy 1 file (1 byte) · Move 0 items to the Recycle Bin"
    );
}

#[gpui_kit::test]
async fn the_toolbar_opens_the_whole_folder_even_with_a_selection(cx: &mut TestAppContext) {
    let (l, r) = sync_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "new.txt");
    open_panel(cx, handle, &view).await;
    assert_eq!(selected(cx, handle, "folder-sync-scope-all"), Some(true));
    assert_eq!(
        selected(cx, handle, "folder-sync-scope-selected"),
        Some(false)
    );
    assert_eq!(
        summary(cx, &view),
        "Copy 2 files (8 bytes; 1 replaces an existing file) · Move 0 items to the Recycle Bin · Skip 2"
    );
    // The panel switches to the selection, and back.
    click(cx, handle, "folder-sync-scope-selected");
    wait_planned(cx, handle, &view).await;
    assert_eq!(
        summary(cx, &view),
        "Copy 1 file (1 byte) · Move 0 items to the Recycle Bin"
    );
    click(cx, handle, "folder-sync-scope-all");
    wait_planned(cx, handle, &view).await;
    assert_eq!(selected(cx, handle, "folder-sync-scope-all"), Some(true));

    // Without a selection (nothing to show), "Selected items only" is off.
    let (same_l, same_r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(&same_l.path().join("eq.txt"), "e", T0);
    put_at(&same_r.path().join("eq.txt"), "e", T0);
    view.update(cx, |v, cx| {
        v.set_roots(same_l.path().to_path_buf(), same_r.path().to_path_buf(), cx)
    });
    wait_idle(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.model().unwrap().picked().is_empty()));
    open_panel(cx, handle, &view).await;
    assert_eq!(
        label(cx, handle, "folder-sync-scope-selected").as_deref(),
        Some("Selected items only (0)")
    );
    let tab_stop = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.find("folder-sync-scope-selected").focused()
        })
        .unwrap();
    assert_eq!(tab_stop, None, "a disabled segment is no tab stop");
    click(cx, handle, "folder-sync-scope-selected");
    assert_eq!(selected(cx, handle, "folder-sync-scope-all"), Some(true));
    assert_eq!(
        summary(cx, &view),
        "Nothing to do: the two sides already match for this mode."
    );
}

#[gpui_kit::test]
async fn row_keys_do_nothing_while_the_sync_panel_is_open(cx: &mut TestAppContext) {
    let (l, r) = sync_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let opened: Rc<RefCell<usize>> = Rc::default();
    let sink = opened.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &FolderEvent, _| {
            if matches!(event, FolderEvent::OpenFiles { .. }) {
                *sink.borrow_mut() += 1;
            }
        })
        .detach()
    });
    let trashed = fake_trash(cx, &view);
    select(&view, cx, "newer.txt");
    open_panel(cx, handle, &view).await;
    assert!(panel_focused(cx, handle));
    for key in ["delete", "alt-right", "alt-left", "enter"] {
        press(cx, handle, key);
        assert_no_prompt_appears(cx).await;
        assert!(view.read_with(cx, |v, _| v.sync_open()), "{key}");
    }
    cx.run_until_parked();
    assert_eq!(*opened.borrow(), 0, "no tab opened");
    assert!(trashed.lock().unwrap().is_empty());
    right_is_untouched(r.path());
    assert!(!view.read_with(cx, |v, _| v.batch_running()));
}

#[gpui_kit::test]
async fn enter_starts_only_a_sync_that_adds_files(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(&l.path().join("eq.txt"), "e", T0);
    put_at(&r.path().join("eq.txt"), "e", T0);
    put_at(&l.path().join("new.txt"), "n", T0);
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    open_panel(cx, handle, &view).await;
    assert_eq!(
        summary(cx, &view),
        "Copy 1 file (1 byte) · Move 0 items to the Recycle Bin"
    );
    // PF11: Enter on a focused button of the panel presses that button, never Start.
    cx.update_window(handle, |_, window, cx| {
        tab_to(window, cx, "folder-sync-cancel");
        window.press("enter", cx);
    })
    .unwrap();
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
    assert_no_prompt_appears(cx).await;
    assert!(!r.path().join("new.txt").exists());
    // Enter on the panel starts a sync that only adds files.
    open_panel(cx, handle, &view).await;
    assert!(panel_focused(cx, handle));
    press(cx, handle, "enter");
    assert_eq!(
        wait_notice(cx, handle, &view).await,
        "Synced: copied 1 file."
    );
    assert_eq!(read(r.path().join("new.txt")), "n");

    // One that replaces a file waits for a click on Start.
    let (l, r) = sync_fixture();
    view.update(cx, |v, cx| {
        v.set_roots(l.path().to_path_buf(), r.path().to_path_buf(), cx)
    });
    wait_idle(cx, handle, &view).await;
    open_panel(cx, handle, &view).await;
    press(cx, handle, "enter");
    assert_no_prompt_appears(cx).await;
    assert!(view.read_with(cx, |v, _| v.sync_open()));
    right_is_untouched(r.path());
    // So does a mirror, which moves files to the Recycle Bin.
    pick_mode(cx, handle, &view, "folder-sync-mode-mirror-right").await;
    assert!(
        panel_focused(cx, handle),
        "a click on a card leaves the focus"
    );
    press(cx, handle, "enter");
    assert_no_prompt_appears(cx).await;
    right_is_untouched(r.path());
}

#[gpui_kit::test]
async fn escape_closes_the_panel_then_gives_the_rows_the_focus(cx: &mut TestAppContext) {
    let (l, r) = sync_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    open_panel(cx, handle, &view).await;
    assert!(panel_focused(cx, handle));
    assert!(!rows_focused(cx, handle, &view));
    press(cx, handle, "escape");
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
    assert!(rows_focused(cx, handle, &view), "the rows took the focus");
    // With the keys in the path filter, the first Esc only closes the panel.
    open_panel(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        let input = view.read(cx).name_filter_input().clone();
        window.focus(&input.focus_handle(cx), cx);
    })
    .unwrap();
    press(cx, handle, "escape");
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
    assert!(!rows_focused(cx, handle, &view));
    press(cx, handle, "escape");
    assert!(rows_focused(cx, handle, &view));
}

#[gpui_kit::test]
async fn esc_cancels_a_running_sync_and_nothing_goes_to_the_recycle_bin(cx: &mut TestAppContext) {
    let (l, r) = sync_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let trashed = fake_trash(cx, &view);
    open_panel(cx, handle, &view).await;
    pick_mode(cx, handle, &view, "folder-sync-mode-mirror-right").await;
    // Esc as soon as the answers are in: the sync never starts.
    click(cx, handle, "folder-sync-start");
    wait_for_prompt(cx, &view).await;
    cx.simulate_prompt_answer("Replace");
    wait_for_prompt(cx, &view).await;
    cx.simulate_prompt_answer("Move to Recycle Bin");
    press(cx, handle, "escape");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |v, _| v.sync_open() || v.batch_running()));
    right_is_untouched(r.path());
    assert!(trashed.lock().unwrap().is_empty());
    // PF5: Esc reaches a running batch before the panel.
    open_panel(cx, handle, &view).await;
    view.update(cx, |v, cx| v.hold_batch_for_test("Syncing", 5, cx));
    press(cx, handle, "escape");
    assert!(view.read_with(cx, |v, _| v.batch_cancelled_for_test()));
    assert!(
        view.read_with(cx, |v, _| v.sync_open()),
        "the panel waits until the batch is over"
    );
    click(cx, handle, "folder-sync-cancel");
    assert!(!view.read_with(cx, |v, _| v.sync_open()));
}

// ---- Task 11: one-sided files, "Ask before copying", the failure list, entering folders ----

type Opened = Rc<RefCell<Vec<(PathBuf, PathBuf, Option<duet_core::kind::CompareKind>)>>>;

fn record_opens(cx: &mut TestAppContext, view: &Entity<FolderView>) -> Opened {
    let opened: Opened = Rc::default();
    let sink = opened.clone();
    cx.update(|cx| {
        cx.subscribe(view, move |_, event: &FolderEvent, _| {
            if let FolderEvent::OpenFiles { left, right, kind } = event {
                sink.borrow_mut().push((left.clone(), right.clone(), *kind));
            }
        })
        .detach()
    });
    opened
}

#[gpui_kit::test]
async fn a_one_sided_file_opens_as_text_beside_an_untitled_side(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let opened = record_opens(cx, &view);
    select(&view, cx, "sub");
    press(cx, handle, "enter");
    select(&view, cx, "sub/c.txt");
    press(cx, handle, "enter");
    cx.run_until_parked();
    assert_eq!(
        opened.borrow().as_slice(),
        [(
            l.path().join("sub").join("c.txt"),
            PathBuf::new(),
            Some(duet_core::kind::CompareKind::Text)
        )]
    );
    let items = menu_outline(&view, cx);
    for want in [
        "Open compare",
        "Open as text",
        "(Open as hex)",
        "(Open as table)",
        "(Open as picture)",
    ] {
        assert!(items.iter().any(|i| i == want), "{want}: {items:?}");
    }
    // The menu item opens it too, and a right-only file puts the empty path on the left.
    select(&view, cx, "new.txt");
    assert!(run_menu_item(cx, handle, &view, "Open as text"));
    cx.run_until_parked();
    assert_eq!(
        opened.borrow().last().unwrap(),
        &(
            PathBuf::new(),
            r.path().join("new.txt"),
            Some(duet_core::kind::CompareKind::Text)
        )
    );
}

#[gpui_kit::test]
async fn with_ask_before_copying_off_a_safe_copy_runs_at_once(cx: &mut TestAppContext) {
    let _data = use_settings(cx);
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_confirm_copy(false, cx));
    select(&view, cx, "new.txt");
    press(cx, handle, "alt-left");
    assert_no_prompt_appears(cx).await;
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx)
            .notice()
            .is_some_and(|n| n.starts_with("Copied 1 file"))
    })
    .await;
    assert!(l.path().join("new.txt").exists());
    // R19: an overwrite still asks.
    select(&view, cx, "b.txt");
    press(cx, handle, "alt-right");
    wait_for_prompt(&mut *cx, &view).await;
    let (message, detail) = cx.pending_prompt().unwrap();
    assert!(message.starts_with("Copy 1 file"), "{message}");
    assert!(
        detail.contains("Overwrites 1 file on the right."),
        "{detail}"
    );
    cx.simulate_prompt_answer("Cancel");
    assert_eq!(read(r.path().join("b.txt")), "right side");
}

#[gpui_kit::test]
async fn failures_are_listed_in_one_prompt(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "new.txt");
    std::fs::remove_file(r.path().join("new.txt")).unwrap();
    press(cx, handle, "alt-left");
    wait_for_prompt(cx, &view).await;
    cx.simulate_prompt_answer("Copy");
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx)
            .notice()
            .is_some_and(|n| n.starts_with("Copied 0 files; 1 failed"))
    })
    .await;
    wait_for_prompt(cx, &view).await;
    let (message, detail) = cx.pending_prompt().unwrap();
    assert_eq!(message, "1 item failed.");
    // The report names the destination; the error names the vanished source.
    let first = l.path().join("new.txt").display().to_string();
    assert!(detail.starts_with(&first), "{detail}");
    assert!(detail[first.len()..].starts_with(": "), "{detail}");
    assert!(
        detail.contains(&r.path().join("new.txt").display().to_string()),
        "{detail}"
    );
    cx.simulate_prompt_answer("OK");
    assert_no_prompt_appears(cx).await;
}

#[gpui_kit::test]
async fn with_enter_folders_on_enter_goes_into_a_folder_on_both_sides(cx: &mut TestAppContext) {
    let _data = use_settings(cx);
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(&l.path().join("d/x.txt"), "l", T0);
    put_at(&r.path().join("d/x.txt"), "r!", T0);
    put_at(&l.path().join("only/y.txt"), "y", T0);
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let roots: Rc<RefCell<Vec<(PathBuf, PathBuf)>>> = Rc::default();
    let sink = roots.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &FolderEvent, _| {
            if let FolderEvent::Rooted { left, right } = event {
                sink.borrow_mut().push((left.clone(), right.clone()));
            }
        })
        .detach()
    });
    // Off: Enter toggles.
    select(&view, cx, "d");
    let before = rels(&view, cx);
    press(cx, handle, "enter");
    assert_ne!(rels(&view, cx), before, "the folder toggled");
    assert!(roots.borrow().is_empty());
    press(cx, handle, "enter");
    assert_eq!(rels(&view, cx), before);
    // On: a folder on both sides becomes the base; a one-sided folder still toggles.
    view.update(cx, |v, cx| v.set_enter_folders(true, cx));
    select(&view, cx, "only");
    let before = rels(&view, cx);
    press(cx, handle, "enter");
    assert_ne!(rels(&view, cx), before);
    assert!(roots.borrow().is_empty());
    select(&view, cx, "d");
    press(cx, handle, "enter");
    cx.run_until_parked();
    assert_eq!(
        roots.borrow().as_slice(),
        [(l.path().join("d"), r.path().join("d"))]
    );
}

// ---- Final-review fix wave ----

/// M-1: a file on one side against a folder on the other is no folder to go into: Enter
/// toggles the row and never roots a side on a file, and "Set this folder as base" is off.
#[gpui_kit::test]
async fn enter_never_goes_into_a_file_against_a_folder(cx: &mut TestAppContext) {
    let _data = use_settings(cx);
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(&l.path().join("item"), "a file", T0);
    put_at(&r.path().join("item/x.txt"), "x", T0);
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let roots: Rc<RefCell<usize>> = Rc::default();
    let sink = roots.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &FolderEvent, _| {
            if matches!(event, FolderEvent::Rooted { .. }) {
                *sink.borrow_mut() += 1;
            }
        })
        .detach()
    });
    view.update(cx, |v, cx| v.set_enter_folders(true, cx));
    select(&view, cx, "item");
    let before = rels(&view, cx);
    press(cx, handle, "enter");
    cx.run_until_parked();
    assert_eq!(*roots.borrow(), 0, "no side is rooted on a file");
    assert_ne!(rels(&view, cx), before, "the row toggled");
    let items = menu_outline(&view, cx);
    assert!(
        items.iter().any(|i| i == "(Set this folder as base)"),
        "{items:?}"
    );
    assert!(!run_menu_item(cx, handle, &view, "Set this folder as base"));
    // The action itself, past the disabled item, does not go in either.
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(duet_app::folder::menu::SetAsBase), cx)
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(*roots.borrow(), 0);
    assert_eq!(
        view.read_with(cx, |v, _| v.paths().0.clone()),
        l.path().to_path_buf()
    );
}

/// A left-button double click on a folder row, as a real one arrives (two presses).
#[cfg(windows)]
fn double_click_row(cx: &mut TestAppContext, handle: AnyWindowHandle, row: usize) {
    use gpui_kit::{InputEvent as _, MouseButton, MouseDownEvent, MouseUpEvent};
    cx.update_window(handle, |_, window, cx| {
        let position = folder_support::row_left(window, cx, row);
        for click_count in [1, 2] {
            window.dispatch_event(
                MouseDownEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers: Default::default(),
                    click_count,
                    first_mouse: false,
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                MouseUpEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers: Default::default(),
                    click_count,
                }
                .to_platform_input(),
                cx,
            );
        }
        window.render_frame(cx);
    })
    .unwrap();
}

/// F-123: Enter and a double click on a junction row, on both sides or one, with "Double-click
/// a folder to compare inside it" off and on, toggle it as a link: no tab opens, no side is
/// rooted, and no file-type check starts.
#[cfg(windows)]
#[gpui_kit::test]
async fn enter_and_double_click_on_a_junction_never_open_it(cx: &mut TestAppContext) {
    let _data = use_settings(cx);
    let (l, r, target) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    put_at(&target.path().join("t.txt"), "t", T0);
    put_at(&l.path().join("a.txt"), "a", T0);
    put_at(&r.path().join("a.txt"), "a!", T0);
    mklink_junction(&l.path().join("jdir"), target.path());
    mklink_junction(&r.path().join("jdir"), target.path());
    mklink_junction(&l.path().join("jonly"), target.path());
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let opened = record_opens(cx, &view);
    let roots: Rc<RefCell<usize>> = Rc::default();
    let sink = roots.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &FolderEvent, _| {
            if matches!(event, FolderEvent::Rooted { .. }) {
                *sink.borrow_mut() += 1;
            }
        })
        .detach()
    });
    for on in [false, true] {
        view.update(cx, |v, cx| v.set_enter_folders(on, cx));
        for rel in ["jdir", "jonly"] {
            select(&view, cx, rel);
            let before = rels(&view, cx);
            let row = before.iter().position(|r| r == rel).unwrap();
            press(cx, handle, "enter");
            cx.run_until_parked();
            double_click_row(cx, handle, row);
            cx.run_until_parked();
            assert_eq!(rels(&view, cx), before, "{rel}, setting {on}");
            assert_ne!(
                notice(cx, &view).as_deref(),
                Some("Checking the file type…"),
                "{rel}, setting {on}"
            );
        }
    }
    assert!(opened.borrow().is_empty(), "{:?}", opened.borrow());
    assert_eq!(*roots.borrow(), 0);
    // The control: Enter on a file still opens it.
    select(&view, cx, "a.txt");
    press(cx, handle, "enter");
    cx.run_until_parked();
    assert_eq!(opened.borrow().len(), 1);
}

/// M-2: whether one root is inside the other is worked out off the UI thread, with the
/// compare and again in Start's check. The right root became a junction into the left after
/// the compare: the panel opens (the window resolved no path), and Start refuses before any
/// question or write.
#[cfg(windows)]
#[gpui_kit::test]
async fn start_checks_for_nested_roots_off_the_ui_thread(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (left, right) = (dir.path().join("l"), dir.path().join("r"));
    put_at(&left.join("sub/x.txt"), "x", T0);
    put_at(&left.join("new.txt"), "n", T0);
    put_at(&right.join("other.txt"), "o", T0);
    let (handle, view) = open_folder(cx, &left, &right);
    wait_idle(cx, handle, &view).await;
    std::fs::remove_dir_all(&right).unwrap();
    mklink_junction(&right, &left.join("sub"));
    click(cx, handle, "folder-sync");
    assert!(view.read_with(cx, |v, _| v.sync_open()));
    wait_planned(cx, handle, &view).await;
    click(cx, handle, "folder-sync-start");
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx).notice().is_some()
    })
    .await;
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some(
            "One folder is inside the other, so sync is off: it could overwrite or delete its own source."
        )
    );
    assert_no_prompt_appears(cx).await;
    assert!(!view.read_with(cx, |v, _| v.batch_running()));
    assert!(!left.join("sub").join("new.txt").exists());
}

/// M-11: "Selected items only (N)" is worked out when the panel opens and when the selection
/// changes through the view, not on every frame.
#[gpui_kit::test]
async fn the_selected_count_is_worked_out_once_per_selection_change(cx: &mut TestAppContext) {
    let (l, r) = sync_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "eq.txt");
    press(cx, handle, "ctrl-a");
    let all = view.read_with(cx, |v, _| v.model().unwrap().picked().len());
    assert!(all > 1, "{all}");
    assert!(run_menu_item(cx, handle, &view, "Sync selected…"));
    let count = |cx: &mut TestAppContext| label(cx, handle, "folder-sync-scope-selected");
    let all_text = format!("Selected items only ({all})");
    assert_eq!(count(cx).as_deref(), Some(all_text.as_str()));
    // Behind the view's back (a test hook), frames do not count again...
    view.update(cx, |v, cx| v.select_row_for_test(0, cx));
    assert_eq!(count(cx).as_deref(), Some(all_text.as_str()));
    // ...and a selection change through the view does: F8 still moves it while the panel is
    // open.
    press(cx, handle, "f8");
    assert_eq!(count(cx).as_deref(), Some("Selected items only (1)"));
}
