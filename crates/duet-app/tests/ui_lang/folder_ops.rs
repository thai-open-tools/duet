//! The folder view's copy, delete and sync prompts, status line and panels in Thai.

use std::fs;
use std::time::Duration;

use duet_app::folder::model::FolderFilter;
use duet_app::folder::sync_panel::{SyncScope, sync_summary};
use duet_app::folder::view::FolderView;
use duet_app::format::count_of;
use duet_app::i18n::{self, Lang, scoped, tr, trf};
use duet_core::folder::report::ReportFormat;
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::folder::{click_thai, fixture, label, open_folders, wait_idle};
use super::{thai_ui, with_data_dir};

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

fn press(cx: &mut TestAppContext, handle: AnyWindowHandle, key: &str) {
    cx.update_window(handle, |_, window, cx| window.press(key, cx))
        .unwrap();
}

async fn wait_for_prompt(cx: &mut TestAppContext) {
    for _ in 0..2000 {
        if cx.has_pending_prompt() {
            return;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    panic!("no prompt appeared");
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
    view.read_with(cx, |v, _| v.notice().map(str::to_owned))
        .unwrap()
}

fn render(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

#[gpui_kit::test]
async fn the_status_line_and_the_selection_note_read_in_thai(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (l, r) = fixture(&[]);
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "a.txt");
    render(cx, handle);
    let status = view.read_with(cx, |v, _| v.status_line().to_string());
    for expected in [
        trf("Left only {}", &[&1]),
        trf("Different {}", &[&1]),
        trf("Showing: {}", &[&tr("All")]),
    ] {
        assert!(status.contains(&expected), "{expected:?} in {status:?}");
    }
    assert!(!status.contains("Left only"), "{status}");
    let quick = tr("Quick (±{} s)").split("{}").next().unwrap().to_owned();
    assert!(status.contains(&quick), "{quick:?} in {status:?}");
    assert!(status.contains("ที่เลือก: a.txt — "), "{status}");
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

#[gpui_kit::test]
async fn thai_copy_prompt_reads_in_thai_and_copies(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (l, r) = fixture(&[]);
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "only.txt");
    press(cx, handle, "alt-right");
    wait_for_prompt(cx).await;
    let what = format!(
        "{} ({})",
        count_of(1, "file", "files"),
        count_of(1, "byte", "bytes")
    );
    let (message, _) = cx.pending_prompt().expect("a prompt");
    assert_eq!(
        message,
        trf(
            "Copy {} from {} to {}?",
            &[&what, &tr("left"), &tr("right")]
        )
    );
    assert!(
        message.contains("ฝั่งซ้าย") && message.contains("ฝั่งขวา"),
        "{message}"
    );
    cx.simulate_prompt_answer(tr("Copy"));
    let target = r.path().join("only.txt");
    let v = view.clone();
    let check = target.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        check.exists() && !v.read(cx).is_busy()
    })
    .await;
    assert_eq!(fs::read_to_string(&target).unwrap(), "x");
    let notice = wait_notice(cx, handle, &view).await;
    assert_eq!(
        notice,
        trf(
            "{}.",
            &[&trf("Copied {}", &[&count_of(1, "file", "files")])]
        )
    );
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

#[gpui_kit::test]
async fn a_thai_overwrite_prompt_names_the_file_and_cancel_changes_nothing(
    cx: &mut TestAppContext,
) {
    let _th = thai_ui();
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (l, r) = fixture(&[]);
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    select(&view, cx, "a.txt");
    press(cx, handle, "alt-right");
    wait_for_prompt(cx).await;
    let (_, detail) = cx.pending_prompt().expect("a prompt");
    let named = r.path().join("a.txt").display().to_string();
    assert!(detail.contains(&named), "{detail}");
    assert!(
        detail.contains(&trf(
            "Overwrites {} on the {}.",
            &[&count_of(1, "file", "files"), &tr("right")]
        )),
        "{detail}"
    );
    cx.simulate_prompt_answer(tr("Cancel"));
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&named).unwrap(), "right side");
    assert!(!cx.has_pending_prompt());
}

#[gpui_kit::test]
async fn thai_delete_prompt_offers_the_sides_in_thai(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (l, r) = fixture(&[]);
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, _| v.set_trash_for_test(|_| Ok(())));
    select(&view, cx, "a.txt");
    press(cx, handle, "delete");
    wait_for_prompt(cx).await;
    assert_eq!(
        cx.pending_prompt().unwrap().0,
        tr("Move to the Recycle Bin?")
    );
    cx.simulate_prompt_answer(tr("Delete left"));
    let notice = wait_notice(cx, handle, &view).await;
    assert_eq!(
        notice,
        trf(
            "Moved {} to the Recycle Bin.",
            &[&count_of(1, "item", "items")]
        )
    );
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

#[gpui_kit::test]
async fn the_open_sync_panel_relabels_on_a_switch(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (l, r) = fixture(&[]);
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    // The sync button waits for a finished result on show.
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.open_sync(SyncScope::All, window, cx))
    })
    .unwrap();
    assert!(view.read_with(cx, |v, _| v.sync_open()));
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx).sync_plan_shown().is_some()
    })
    .await;
    render(cx, handle);
    let english = view.read_with(cx, |v, _| sync_summary(v.sync_plan_shown().unwrap()));
    assert_eq!(
        label(cx, handle, "folder-sync-summary").as_deref(),
        Some(english.as_str())
    );

    click_thai(cx, handle);

    let _th = scoped(Lang::Th);
    assert!(view.read_with(cx, |v, _| v.sync_open()), "the panel stays");
    let thai = view.read_with(cx, |v, _| sync_summary(v.sync_plan_shown().unwrap()));
    assert_ne!(thai, english);
    render(cx, handle);
    assert_eq!(
        label(cx, handle, "folder-sync-summary").as_deref(),
        Some(thai.as_str())
    );
    for (id, key) in [
        ("folder-sync-mode-update-right", "Update right"),
        ("folder-sync-mode-mirror-left", "Mirror to left"),
        ("folder-sync-scope-all", "Whole folder"),
        ("folder-sync-cancel", "Cancel"),
        ("folder-sync-start", "Start sync"),
    ] {
        assert_eq!(label(cx, handle, id).as_deref(), Some(tr(key)), "{id}");
    }
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

#[gpui_kit::test]
async fn the_export_panel_reads_in_thai_and_its_placeholders_follow_a_switch(
    cx: &mut TestAppContext,
) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (l, r) = fixture(&[]);
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.open_export(ReportFormat::Xlsx, window, cx))
    })
    .unwrap();
    assert!(view.read_with(cx, |v, _| v.export_open()));
    render(cx, handle);
    let english = view.read_with(cx, |v, cx| v.export_placeholders_for_test(cx).unwrap());
    assert_eq!(english[1], "Optional");

    click_thai(cx, handle);

    let _th = scoped(Lang::Th);
    assert!(
        view.read_with(cx, |v, _| v.export_open()),
        "the panel stays"
    );
    let thai = view.read_with(cx, |v, cx| v.export_placeholders_for_test(cx).unwrap());
    assert_eq!(
        thai[0],
        tr("For example: check the config before deploying (optional)")
    );
    assert_eq!(thai[1], tr("Optional"));
    render(cx, handle);
    for (id, key) in [
        ("folder-export-cancel", "Cancel"),
        ("folder-export-create", "Create"),
        ("folder-export-scope-filtered", "As filtered now"),
    ] {
        assert_eq!(label(cx, handle, id).as_deref(), Some(tr(key)), "{id}");
    }
    let card = label(cx, handle, "folder-export-format-xlsx").unwrap();
    assert_eq!(
        card,
        format!(
            "{}: {}",
            tr("Excel (.xlsx)"),
            tr("team tracking: filters, status colours, columns for reviewers")
        )
    );
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

#[gpui_kit::test]
async fn a_cancelling_status_follows_a_switch(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (l, r) = fixture(&[]);
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.hold_batch_for_test("Copying", 3, cx));
    press(cx, handle, "escape");
    assert!(view.read_with(cx, |v, _| v.batch_cancelled_for_test()));
    let key = "Cancelling…";
    assert_eq!(view.read_with(cx, |v, _| v.status().to_owned()), key);
    click_thai(cx, handle);
    let _th = scoped(Lang::Th);
    assert_ne!(tr(key), key, "the sentence has Thai");
    assert_eq!(view.read_with(cx, |v, _| v.status().to_owned()), tr(key));
}

#[gpui_kit::test]
async fn the_sync_overwrite_prompt_says_overwrite_on_its_button(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (l, r) = fixture(&[]);
    let _th = thai_ui();
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.open_sync(SyncScope::All, window, cx))
    })
    .unwrap();
    let planned = |view: &Entity<FolderView>| {
        let v = view.clone();
        move |_: &mut _, cx: &mut gpui_kit::App| v.read(cx).sync_plan_shown().is_some()
    };
    cx.wait_for(handle, Duration::from_secs(20), planned(&view))
        .await;
    // Mirror to the right replaces a.txt, which differs, so the question is asked.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("folder-sync-mode-mirror-right", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx)
            .sync_plan_shown()
            .is_some_and(|plan| plan.overwrites() > 0)
    })
    .await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("folder-sync-start", cx);
    })
    .unwrap();
    wait_for_prompt(cx).await;
    let (_, detail) = cx.pending_prompt().unwrap();
    assert!(detail.contains("เขียนทับ"), "{detail}");
    assert_ne!(tr("Replace"), "เขียนทับ", "find-replace keeps its own word");
    // The button says what the detail says, and answering it replaces the file.
    cx.simulate_prompt_answer("เขียนทับ");
    wait_notice(cx, handle, &view).await;
    assert_eq!(fs::read_to_string(r.path().join("a.txt")).unwrap(), "left");
}
