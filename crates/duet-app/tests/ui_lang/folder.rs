//! The folder view in Thai.

use std::fs;
use std::path::Path;
use std::time::Duration;

use duet_app::folder::view::FolderView;
use duet_app::i18n::{self, Lang, scoped, tr};
use duet_app::launch::Launch;
use duet_app::workspace::{TabItem, Workspace};
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{AnyWindowHandle, AppContext, Entity, Focusable as _, TestAppContext};

use super::{open, thai_ui};

/// Two folders with one differing file and one file only on the left. Each of `extra` is made on
/// the left too: a name ending in `/` is a folder with a file in it.
pub(super) fn fixture(extra: &[&str]) -> (tempfile::TempDir, tempfile::TempDir) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    fs::write(l.path().join("a.txt"), "left").unwrap();
    fs::write(r.path().join("a.txt"), "right side").unwrap();
    fs::write(l.path().join("only.txt"), "x").unwrap();
    for name in extra {
        if let Some(dir) = name.strip_suffix('/') {
            fs::create_dir_all(l.path().join(dir)).unwrap();
            fs::write(l.path().join(dir).join("inside.txt"), "x").unwrap();
        } else {
            fs::write(l.path().join(name), "x").unwrap();
        }
    }
    (l, r)
}

pub(super) fn folder_of(
    workspace: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> Entity<FolderView> {
    workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Folder(view)) => view.clone(),
        _ => panic!("expected a folder tab"),
    })
}

pub(super) async fn wait_idle(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<FolderView>,
) {
    let view = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        let v = view.read(cx);
        v.model().is_some() && !v.is_busy()
    })
    .await;
}

pub(super) fn open_folders(
    cx: &mut TestAppContext,
    l: &Path,
    r: &Path,
) -> (AnyWindowHandle, Entity<Workspace>, Entity<FolderView>) {
    let (handle, workspace) = open(
        cx,
        Launch::Folders {
            left: l.to_path_buf(),
            right: r.to_path_buf(),
        },
    );
    let view = folder_of(&workspace, cx);
    (handle, workspace, view)
}

pub(super) fn label(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    id: &'static str,
) -> Option<String> {
    cx.update_window(handle, |_, window, _| {
        window.find(id).label().map(str::to_owned)
    })
    .unwrap()
}

fn placeholders(view: &Entity<FolderView>, cx: &mut TestAppContext) -> [String; 3] {
    view.read_with(cx, |v, cx| {
        [v.include_input(), v.exclude_input(), v.name_filter_input()]
            .map(|input| input.read(cx).presentation().placeholder().to_string())
    })
}

pub(super) fn click_thai(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 0usize), cx)
    })
    .unwrap();
}

#[gpui_kit::test]
async fn the_folder_view_reads_in_thai(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let (l, r) = fixture(&[]);
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    for (id, key) in [
        ("folder-pick-left", "Left"),
        ("folder-pick-right", "Right"),
        ("folder-swap", "Swap sides (F9)"),
        ("folder-up", "Up one folder on both sides"),
        ("folder-refresh", "Compare"),
        ("folder-options-menu", "Options"),
        ("folder-layout", "View: tree"),
        ("folder-expand-all", "Expand all"),
        ("folder-collapse-all", "Collapse all"),
        ("folder-legend", "Legend"),
        ("folder-delete", "Delete…"),
        ("folder-reveal", "Show in Explorer"),
        ("folder-sync", "Sync…"),
        ("folder-report", "Report"),
    ] {
        let drawn = label(cx, handle, id);
        assert_eq!(drawn.as_deref(), Some(tr(key)), "{id}");
        assert_ne!(tr(key), key, "{key} has Thai");
    }
    // The three fields name themselves for a screen reader.
    let named = view.read_with(cx, |v, _| {
        [v.include_input(), v.exclude_input(), v.name_filter_input()]
            .map(|input| gpui_kit::ElementId::from(("input", input.entity_id())))
    });
    for (id, key) in named
        .into_iter()
        .zip(["Include masks", "Exclude masks", "Filter paths"])
    {
        let drawn = cx
            .update_window(handle, |_, window, _| {
                window.find(id).label().map(str::to_owned)
            })
            .unwrap();
        assert_eq!(drawn.as_deref(), Some(tr(key)), "{key}");
    }
    let mode = label(cx, handle, "folder-mode");
    assert_eq!(mode.as_deref(), Some(tr("Quick (size + time)")));
    let [include, exclude, filter] = placeholders(&view, cx);
    assert_eq!(include, tr("e.g. *.rs; *.toml"));
    assert_eq!(exclude, tr("e.g. target; *.log"));
    assert_eq!(filter, tr("Filter paths"));
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

#[gpui_kit::test]
async fn a_file_named_like_a_label_stays_as_it_is(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let (l, r) = fixture(&["Compare", "Left/"]);
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let names: Vec<String> = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (0..m.visible_len())
            .map(|row| m.node_at(row).name.clone())
            .collect()
    });
    assert!(names.contains(&"Compare".to_owned()), "{names:?}");
    assert!(names.contains(&"Left".to_owned()), "{names:?}");
    for (row, name) in names.iter().enumerate() {
        let drawn = cx
            .update_window(handle, |_, window, _| {
                window
                    .find(("folder-name-left", row))
                    .label()
                    .map(str::to_owned)
            })
            .unwrap();
        assert_eq!(drawn.as_deref(), Some(name.as_str()), "row {row}");
    }
    // The button of the same word is the Thai one.
    assert_eq!(
        label(cx, handle, "folder-pick-left").as_deref(),
        Some(tr("Left"))
    );
}

#[gpui_kit::test]
async fn a_stored_notice_clears_and_a_failed_mask_redraws_on_a_switch(cx: &mut TestAppContext) {
    let (l, r) = fixture(&[]);
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    // A notice in English: F8 wraps past the last difference within a few presses.
    for _ in 0..6 {
        cx.update_window(handle, |_, window, cx| window.press("f8", cx))
            .unwrap();
        if view.read_with(cx, |v, _| v.notice().is_some()) {
            break;
        }
    }
    let notice = view.read_with(cx, |v, _| v.notice().map(str::to_owned));
    assert_eq!(
        notice.as_deref(),
        Some("Wrapped to the first difference."),
        "F8 wrapped"
    );
    // And a mask that does not parse.
    cx.update_window(handle, |_, window, cx| {
        let input = view.read(cx).include_input().clone();
        window.focus(&input.focus_handle(cx), cx);
        window.input("src/[", cx);
        window.press("enter", cx);
    })
    .unwrap();
    assert!(view.read_with(cx, |v, _| v.mask_error().is_some()));

    click_thai(cx, handle);

    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned)),
        None
    );
    assert!(
        view.read_with(cx, |v, _| v.mask_error().is_some()),
        "the failed mask stays on show"
    );
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("folder-mask-error").is_some());
    })
    .unwrap();
    let thai = scoped(Lang::Th);
    let [include, ..] = placeholders(&view, cx);
    assert_eq!(include, tr("e.g. *.rs; *.toml"));
    drop(thai);
}

#[gpui_kit::test]
async fn a_cancelled_compare_says_so_in_the_new_language(cx: &mut TestAppContext) {
    let (l, r) = fixture(&[]);
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.refresh(cx));
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    let key = "Cancelled. Press F5 to compare again.";
    assert_eq!(view.read_with(cx, |v, _| v.status().to_owned()), key);
    click_thai(cx, handle);
    let thai = scoped(Lang::Th);
    assert_ne!(tr(key), key, "the sentence has Thai");
    assert_eq!(view.read_with(cx, |v, _| v.status().to_owned()), tr(key));
    drop(thai);
}

#[gpui_kit::test]
async fn the_seconds_unit_reads_in_thai_on_the_toolbar_and_the_status_bar(cx: &mut TestAppContext) {
    let (l, r) = fixture(&[]);
    // English keeps the Latin unit.
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let english = view.read_with(cx, |v, _| v.status().to_owned());
    assert!(english.contains(" s"), "{english}");
    drop(view);
    let _th = thai_ui();
    let (handle, _workspace, view) = open_folders(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let status = view.read_with(cx, |v, _| v.status().to_owned());
    assert!(status.contains(" วินาที"), "{status}");
    assert!(
        !status.ends_with(" s") && !status.contains(" s "),
        "{status}"
    );
    // The tolerance box's unit, drawn with `tr("s")`.
    assert_eq!(tr("s"), "วินาที");
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}
