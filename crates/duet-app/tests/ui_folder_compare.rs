//! Headless UI tests for the folder compare's criteria, verdicts and remembered choices (P10a).

mod folder_support;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use duet_app::app::layout_bindings;
use duet_app::folder::model::{FolderFilter, SortKey};
use duet_app::folder::prefs::Layout;
use duet_app::folder::view::options::{
    CriterionSize, ToggleConfirmCopy, ToggleEnterFolders, ToggleOddScan, criterion_menu,
    options_menu,
};
use duet_app::folder::view::{FolderEvent, FolderView};
use duet_app::launch::Launch;
use duet_app::tabs::TabKey;
use duet_app::text::view::TextView;
use duet_app::ui::menu::outline;
use duet_app::workspace::{TabItem, Workspace};
use duet_core::folder::classify::Warning;
use duet_core::folder::run::{CompareMode, FolderOptions};
use duet_core::folder::tree::Status;
use duet_core::text::diff::WhitespaceMode;
use folder_support::{
    assert_no_prompt_appears, fixture, open_folder, open_folder_with, open_test_window, press, put,
    put_at, rels, saved_setting, select, use_settings, wait_idle,
};
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{AnyWindowHandle, AppContext, Entity, Focusable, Keystroke, TestAppContext};

#[gpui_kit::test]
async fn a_new_folder_tab_starts_with_the_saved_filter(cx: &mut TestAppContext) {
    let data = use_settings(cx);
    std::fs::write(
        data.path().join("settings.json"),
        r#"{"folder.filter": "left"}"#,
    )
    .unwrap();
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().filter()),
        FolderFilter::LeftOnly,
        "the first model takes the saved filter"
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.prefs().filter),
        FolderFilter::LeftOnly
    );

    view.update(cx, |v, cx| v.set_filter(FolderFilter::All, cx));
    assert_eq!(saved_setting(&data, "folder.filter"), "all");
    view.update(cx, |v, cx| v.refresh(cx));
    wait_idle(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().filter()),
        FolderFilter::All,
        "a later model carries the filter over"
    );
}

#[gpui_kit::test]
async fn the_workspace_opens_folders_with_the_saved_criterion(cx: &mut TestAppContext) {
    let data = use_settings(cx);
    std::fs::write(
        data.path().join("settings.json"),
        r#"{"folder.criterion": "size"}"#,
    )
    .unwrap();
    let (l, r) = fixture();
    let (left, right) = (l.path().to_path_buf(), r.path().to_path_buf());
    let key = TabKey::Folders(left.clone(), right.clone());
    let (handle, workspace) = open_test_window(cx, move |window, cx| {
        cx.new(|cx| Workspace::new(Launch::Folders { left, right }, window, cx))
    });
    let view: Entity<FolderView> = workspace.read_with(cx, |w, _| {
        let index = w.tabs().find(&key).expect("the folder tab exists");
        match w.tabs().iter().nth(index) {
            Some((_, _, TabItem::Folder(view))) => view.clone(),
            _ => panic!("expected a folder tab"),
        }
    });
    assert_eq!(
        view.read_with(cx, |v, _| v.options().mode),
        CompareMode::Size
    );
    wait_idle(cx, handle, &view).await;
}

// ---- Task 4: the options row ----

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

/// Replaces the tolerance field's text and presses Enter in it.
fn type_tolerance(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<FolderView>,
    text: &str,
) {
    cx.update_window(handle, |_, window, cx| {
        let input = view.read(cx).tolerance_input().clone();
        input.update(cx, |state, cx| state.set_value("", window, cx));
        window.focus(&input.focus_handle(cx), cx);
        window.input(text, cx);
        window.press("enter", cx);
    })
    .unwrap();
}

fn tolerance_secs(view: &Entity<FolderView>, cx: &mut TestAppContext) -> u64 {
    view.read_with(cx, |v, _| v.options().quick.tolerance.as_secs())
}

fn present(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
    .unwrap()
}

#[gpui_kit::test]
async fn the_criterion_menu_lists_four_criteria_with_the_current_one_checked(
    cx: &mut TestAppContext,
) {
    let (l, r) = fixture();
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), FolderOptions::default());
    wait_idle(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| outline(&criterion_menu(v))),
        [
            "Content",
            "● Quick: size and modified time (reads no files)",
            "Size only",
            "Content by the text compare rules"
        ]
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.mode_button_label()),
        "Quick (size + time)"
    );
    view.update(cx, |v, cx| v.set_mode(CompareMode::Rules, cx));
    assert_eq!(
        view.read_with(cx, |v, _| v.mode_button_label()),
        "Text rules"
    );
    wait_idle(cx, handle, &view).await;
}

#[gpui_kit::test]
async fn choosing_a_criterion_compares_again_and_is_saved(cx: &mut TestAppContext) {
    let data = use_settings(cx);
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let before = view.read_with(cx, |v, _| v.job_generation());
    dispatch(cx, handle, &view, CriterionSize);
    assert_eq!(
        view.read_with(cx, |v, _| v.options().mode),
        CompareMode::Size
    );
    assert!(view.read_with(cx, |v, _| v.job_generation()) > before);
    assert_eq!(saved_setting(&data, "folder.criterion"), "size");
    wait_idle(cx, handle, &view).await;
}

#[gpui_kit::test]
async fn the_tolerance_field_takes_whole_seconds(cx: &mut TestAppContext) {
    let data = use_settings(cx);
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let opened = Rc::new(Cell::new(0usize));
    let seen = opened.clone();
    let _watch = cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &FolderEvent, _| {
            if matches!(event, FolderEvent::OpenFiles { .. }) {
                seen.set(seen.get() + 1);
            }
        })
    });
    select(&view, cx, "b.txt");

    type_tolerance(cx, handle, &view, "7");
    assert_eq!(tolerance_secs(&view, cx), 7);
    assert_eq!(saved_setting(&data, "folder.time_tolerance"), 7);
    wait_idle(cx, handle, &view).await;
    type_tolerance(cx, handle, &view, "90000");
    assert_eq!(tolerance_secs(&view, cx), 86_400);
    assert_eq!(
        view.read_with(cx, |v, _| v.prefs().tolerance_secs),
        86_400,
        "the saved choice clamps too"
    );
    wait_idle(cx, handle, &view).await;
    type_tolerance(cx, handle, &view, "abc");
    assert_eq!(tolerance_secs(&view, cx), 0);
    wait_idle(cx, handle, &view).await;

    // Delete and Backspace edit the field; they do not reach the rows.
    cx.update_window(handle, |_, window, cx| {
        let input = view.read(cx).tolerance_input().clone();
        input.update(cx, |state, cx| state.set_value("12", window, cx));
        window.focus(&input.focus_handle(cx), cx);
        window.press("backspace", cx);
        window.press("home", cx);
        window.press("delete", cx);
    })
    .unwrap();
    assert_no_prompt_appears(cx).await;
    let text = view.read_with(cx, |v, cx| v.tolerance_input().read(cx).value().to_string());
    assert_eq!(text, "", "Backspace took the 2 and Delete the 1");
    assert_eq!(opened.get(), 0, "Enter in the field opened no tab");

    view.update(cx, |v, cx| v.set_mode(CompareMode::Content, cx));
    wait_idle(cx, handle, &view).await;
    assert!(!present(cx, handle, "folder-tolerance"));
    assert!(!present(cx, handle, "folder-dst"));
}

#[gpui_kit::test]
async fn an_unchanged_tolerance_does_not_compare_again(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let now = tolerance_secs(&view, cx);
    let before = view.read_with(cx, |v, _| v.job_generation());
    view.update(cx, |v, cx| v.set_tolerance(now, cx));
    assert_eq!(view.read_with(cx, |v, _| v.job_generation()), before);
}

#[gpui_kit::test]
async fn each_criterion_shows_its_own_controls(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    for (mode, quick, format) in [
        (CompareMode::Quick, true, false),
        (CompareMode::Content, false, true),
        (CompareMode::Size, false, false),
        (CompareMode::Rules, false, true),
    ] {
        view.update(cx, |v, cx| v.set_mode(mode, cx));
        wait_idle(cx, handle, &view).await;
        for (id, shown) in [
            ("folder-tolerance", quick),
            ("folder-tolerance-label", quick),
            ("folder-dst", quick),
            ("folder-format-important", format),
        ] {
            assert_eq!(present(cx, handle, id), shown, "{id} in {mode:?}");
        }
    }
}

#[gpui_kit::test]
async fn count_line_endings_flips_similar_files_without_reading_again(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("crlf.txt"), "a\r\nb\r\n");
    put(&r.path().join("crlf.txt"), "a\nb\n");
    let options = FolderOptions {
        mode: CompareMode::Content,
        ..FolderOptions::default()
    };
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), options);
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_filter(FolderFilter::All, cx));
    let status = |cx: &mut TestAppContext| {
        view.read_with(cx, |v, _| {
            let m = v.model().unwrap();
            (0..m.visible_len())
                .map(|row| m.node_at(row))
                .find(|n| n.rel == "crlf.txt")
                .map(|n| n.status.clone())
                .unwrap()
        })
    };
    assert!(matches!(status(cx), Status::Similar { .. }));
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().counts().similar),
        1
    );

    let generation = view.read_with(cx, |v, _| v.job_generation());
    view.update(cx, |v, cx| v.set_format_important(true, cx));
    assert!(matches!(status(cx), Status::Different { .. }));
    assert_eq!(view.read_with(cx, |v, _| v.job_generation()), generation);
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().counts().similar),
        0
    );

    view.update(cx, |v, cx| v.set_format_important(false, cx));
    assert!(matches!(status(cx), Status::Similar { .. }));
    assert_eq!(view.read_with(cx, |v, _| v.job_generation()), generation);
}

#[gpui_kit::test]
async fn the_options_menu_holds_three_settings_and_saves_them(cx: &mut TestAppContext) {
    let data = use_settings(cx);
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| outline(&options_menu(v))),
        [
            "● Look for invisible characters after a compare",
            "● Ask before copying",
            "Double-click a folder to compare inside it"
        ]
    );
    dispatch(cx, handle, &view, ToggleOddScan);
    assert_eq!(saved_setting(&data, "folder.odd_scan"), false);
    assert_eq!(
        view.read_with(cx, |v, _| outline(&options_menu(v))[0].clone()),
        "Look for invisible characters after a compare"
    );
    dispatch(cx, handle, &view, ToggleConfirmCopy);
    assert_eq!(saved_setting(&data, "folder.confirm_copy"), false);
    dispatch(cx, handle, &view, ToggleEnterFolders);
    assert_eq!(saved_setting(&data, "folder.double_click"), "enter");
}

#[gpui_kit::test]
async fn the_other_choices_are_saved_when_they_change(cx: &mut TestAppContext) {
    let data = use_settings(cx);
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_ignore_dst(true, cx));
    assert_eq!(saved_setting(&data, "folder.ignore_dst"), true);
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_format_important(true, cx));
    assert_eq!(saved_setting(&data, "folder.format_important"), true);
}

/// G5: a rule set in a text tab applies to the next Rules-mode folder compare.
#[gpui_kit::test]
async fn a_rules_compare_picks_up_the_rules_a_text_tab_set(cx: &mut TestAppContext) {
    let _data = use_settings(cx);
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("w.txt"), "x  y\n");
    put(&r.path().join("w.txt"), "x y\n");
    let options = FolderOptions {
        mode: CompareMode::Rules,
        ..FolderOptions::default()
    };
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), options);
    wait_idle(cx, handle, &view).await;
    let status = |cx: &mut TestAppContext| {
        view.update(cx, |v, cx| v.set_filter(FolderFilter::All, cx));
        view.read_with(cx, |v, _| {
            let m = v.model().unwrap();
            (0..m.visible_len())
                .map(|row| m.node_at(row))
                .find(|n| n.rel == "w.txt")
                .map(|n| n.status.clone())
                .unwrap()
        })
    };
    assert!(
        matches!(status(cx), Status::Different { .. }),
        "no rules yet"
    );

    let (a, b) = (l.path().join("w.txt"), r.path().join("w.txt"));
    let text = cx.new(|cx| TextView::new(a, b, cx));
    text.update(cx, |t, cx| t.set_whitespace(WhitespaceMode::Amount, cx));
    view.update(cx, |v, cx| v.refresh(cx));
    wait_idle(cx, handle, &view).await;
    assert!(
        matches!(status(cx), Status::Similar { .. }),
        "the rules hide the extra space"
    );
}

// ---- Task 5: how the new verdicts look ----

#[gpui_kit::test]
async fn case_only_names_show_both_spellings_and_a_warning(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("Config.xml"), "<a/>");
    put(&r.path().join("config.xml"), "<a/>");
    let options = FolderOptions {
        case_insensitive: true,
        ..FolderOptions::default()
    };
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), options);
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find(("folder-name-left", 0usize)).label(),
            Some("Config.xml")
        );
        assert_eq!(
            window.find(("folder-name-right", 0usize)).label(),
            Some("config.xml")
        );
    })
    .unwrap();
    let (warning, status) = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (m.node_at(0).warning(), v.status().to_owned())
    });
    assert_eq!(warning, Some(Warning::CaseOnly));
    assert!(status.contains("⚠ names differ in case 1"), "{status}");
}

#[gpui_kit::test]
async fn a_same_file_with_another_time_counts_as_other_time(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(&l.path().join("t.txt"), "same", 1_700_000_000);
    put_at(&r.path().join("t.txt"), "same", 1_700_007_200);
    let options = FolderOptions {
        mode: CompareMode::Content,
        ..FolderOptions::default()
    };
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), options);
    wait_idle(cx, handle, &view).await;
    let (status, counts, text) = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        let n = m
            .comparison()
            .nodes
            .iter()
            .position(|n| n.rel == "t.txt")
            .unwrap();
        (
            m.comparison().nodes[n].status.clone(),
            m.counts(),
            v.status().to_owned(),
        )
    });
    assert_eq!(status, Status::Same);
    assert_eq!(counts.time_only, 1);
    assert!(text.contains("Same contents, other time 1"), "{text}");
}

#[gpui_kit::test]
async fn a_crlf_script_is_a_warned_difference(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("run.sh"), "echo a\n");
    put(&r.path().join("run.sh"), "echo a\r\n");
    let options = FolderOptions {
        mode: CompareMode::Content,
        ..FolderOptions::default()
    };
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), options);
    wait_idle(cx, handle, &view).await;
    assert_eq!(rels(&view, cx), ["run.sh"], "listed under Mismatched");
    select(&view, cx, "run.sh");
    let line = view.read_with(cx, |v, _| v.status_line().to_string());
    assert!(line.ends_with("— a script with CRLF or a BOM"), "{line}");
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().counts().script),
        1
    );
}

// ---- Task 6: the invisible-character scan ----

fn odd_fixture() -> (tempfile::TempDir, tempfile::TempDir) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("a.properties"), "k =\u{a0}v\n");
    put(&r.path().join("a.properties"), "k = v\n");
    (l, r)
}

async fn wait_odd_scan(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<FolderView>,
) {
    let view = view.clone();
    cx.wait_for(handle, std::time::Duration::from_secs(20), move |_, cx| {
        !view.read(cx).odd_scan_running()
    })
    .await;
}

fn has_element(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    id: (&'static str, usize),
) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
    .unwrap()
}

#[gpui_kit::test]
async fn invisible_characters_get_a_chip_and_a_count(cx: &mut TestAppContext) {
    let _data = use_settings(cx);
    let (l, r) = odd_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    wait_odd_scan(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.odd_marks().is_some()));
    assert!(has_element(cx, handle, ("folder-odd-left", 0)));
    assert!(!has_element(cx, handle, ("folder-odd-right", 0)));
    let status = view.read_with(cx, |v, _| v.status().to_owned());
    assert!(
        status.contains("⚠ invisible characters in 1 file"),
        "{status}"
    );
    assert!(!status.contains("Looking for"), "{status}");
}

#[gpui_kit::test]
async fn the_scan_can_be_turned_off(cx: &mut TestAppContext) {
    let _data = use_settings(cx);
    let (l, r) = odd_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    wait_odd_scan(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_odd_scan(false, cx));
    assert!(view.read_with(cx, |v, _| !v.odd_scan_running() && v.odd_marks().is_none()));
    press(cx, handle, "f5");
    wait_idle(cx, handle, &view).await;
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| !v.odd_scan_running() && v.odd_marks().is_none()));
    assert!(!has_element(cx, handle, ("folder-odd-left", 0)));

    // Turning it on again scans the result on show; turning it off during the scan stops it.
    view.update(cx, |v, cx| v.set_odd_scan(true, cx));
    assert!(view.read_with(cx, |v, _| v.odd_scan_running()));
    let status = view.read_with(cx, |v, _| v.status().to_owned());
    assert!(
        status.ends_with("Looking for invisible characters…"),
        "{status}"
    );
    view.update(cx, |v, cx| v.set_odd_scan(false, cx));
    assert!(view.read_with(cx, |v, _| !v.odd_scan_running()));
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.odd_marks().is_none()));
}

#[gpui_kit::test]
async fn swapping_keeps_the_marks_with_their_side(cx: &mut TestAppContext) {
    let _data = use_settings(cx);
    let (l, r) = odd_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    wait_odd_scan(cx, handle, &view).await;
    press(cx, handle, "f9");
    assert!(view.read_with(cx, |v, _| v.odd_marks().is_some()));
    assert!(has_element(cx, handle, ("folder-odd-right", 0)));
    assert!(!has_element(cx, handle, ("folder-odd-left", 0)));

    // A swap during a scan starts it again on the swapped sides (PF12).
    press(cx, handle, "f5");
    wait_idle(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.odd_scan_running() || v.odd_marks().is_some()));
    press(cx, handle, "f9");
    wait_odd_scan(cx, handle, &view).await;
    assert!(has_element(cx, handle, ("folder-odd-left", 0)));
    assert!(!has_element(cx, handle, ("folder-odd-right", 0)));
}

#[gpui_kit::test]
async fn a_new_compare_drops_the_old_marks(cx: &mut TestAppContext) {
    let _data = use_settings(cx);
    let (l, r) = odd_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    wait_odd_scan(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.odd_marks().is_some()));
    press(cx, handle, "f5");
    assert!(view.read_with(cx, |v, _| v.odd_marks().is_none()));
    wait_idle(cx, handle, &view).await;
    wait_odd_scan(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.odd_marks().is_some()));
}

#[gpui_kit::test]
async fn a_big_folder_waits_for_a_click_before_it_scans(cx: &mut TestAppContext) {
    let _data = use_settings(cx);
    let (l, r) = odd_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    wait_odd_scan(cx, handle, &view).await;
    view.update(cx, |v, _| v.set_odd_auto_limit_for_test(0));
    press(cx, handle, "f5");
    wait_idle(cx, handle, &view).await;
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| !v.odd_scan_running() && v.odd_marks().is_none()));
    let status = view.read_with(cx, |v, _| v.status().to_owned());
    assert!(
        status.contains("Not looking for invisible characters in 1 file"),
        "{status}"
    );
    // M-5: the forced scan shares the model's comparison; the UI thread copies nothing.
    let holders = |cx: &mut TestAppContext| {
        view.read_with(cx, |v, _| {
            Arc::strong_count(&v.model().unwrap().shared_comparison())
        })
    };
    let before = holders(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("folder-odd-scan", cx);
    })
    .unwrap();
    assert!(view.read_with(cx, |v, _| v.odd_scan_running()));
    assert_eq!(
        holders(cx),
        before + 1,
        "the scan holds the model's own comparison"
    );
    wait_odd_scan(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.odd_marks().is_some()));
}

/// M-3: a copy, delete or sync stops the scan: on Windows a file the scan holds open could
/// not go to the Recycle Bin. The compare after the batch scans again.
#[gpui_kit::test]
async fn a_batch_stops_the_invisible_character_scan(cx: &mut TestAppContext) {
    let _data = use_settings(cx);
    let (l, r) = odd_fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    wait_odd_scan(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_odd_scan(true, cx));
    assert!(view.read_with(cx, |v, _| v.odd_scan_running()));
    view.update(cx, |v, cx| v.hold_batch_for_test("Copying", 1, cx));
    assert!(view.read_with(cx, |v, _| !v.odd_scan_running() && v.odd_marks().is_none()));
}

// ---- Task 7: pills, folder totals, the path filter, Ctrl+F and the legend ----

#[gpui_kit::test]
async fn ctrl_f_focuses_the_path_filter(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_filter(FolderFilter::All, cx));
    press(cx, handle, "ctrl-f");
    let focused = cx
        .update_window(handle, |_, window, cx| {
            let input = view.read(cx).name_filter_input().clone();
            input.focus_handle(cx).is_focused(window)
        })
        .unwrap();
    assert!(focused, "Ctrl+F put the focus in the path filter");
    cx.update_window(handle, |_, window, cx| window.input("c.t", cx))
        .unwrap();
    assert_eq!(rels(&view, cx), ["sub", "sub/c.txt"]);
}

/// F-54: Ctrl+F, and Ctrl+ด on the Thai Kedmanee layout, selects the text already in the path
/// filter, so typing replaces it.
#[gpui_kit::test]
async fn ctrl_f_selects_the_text_in_the_path_filter(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_filter(FolderFilter::All, cx));
    // Thai Kedmanee names the "f" key "ด"; bind the layout's names as `bind_shortcuts` does.
    cx.update(|cx| {
        let thai = |mut keystroke: Keystroke| {
            if keystroke.key == "f" {
                keystroke.key = "ด".into();
            }
            keystroke
        };
        cx.bind_keys(layout_bindings(
            &duet_app::folder::view::key_bindings(),
            thai,
        ));
    });
    let input = view.read_with(cx, |v, _| v.name_filter_input().clone());
    press(cx, handle, "ctrl-f");
    cx.update_window(handle, |_, window, cx| window.input("sub/", cx))
        .unwrap();
    let mut selected = Vec::new();
    for key in ["ctrl-f", "ctrl-ด"] {
        // The caret at the end of "sub/", nothing selected, and the keys on the rows.
        input.update(cx, |i, cx| i.set_selected_range(4..4, cx));
        cx.update_window(handle, |_, window, cx| {
            let rows = view.read(cx).focus_handle(cx);
            window.focus(&rows, cx);
        })
        .unwrap();
        press(cx, handle, key);
        let focused = cx
            .update_window(handle, |_, window, cx| {
                input.focus_handle(cx).is_focused(window)
            })
            .unwrap();
        assert!(focused, "{key} put the focus in the path filter");
        selected.push(input.read_with(cx, |i, _| i.selected_range()));
    }
    assert_eq!(selected, [0..4, 0..4], "both select the filter's text");
    // Typing replaces the selection; appending would give "sub/c.t".
    cx.update_window(handle, |_, window, cx| window.input("c.t", cx))
        .unwrap();
    assert_eq!(input.read_with(cx, |i, _| i.value().to_string()), "c.t");
    assert_eq!(rels(&view, cx), ["sub", "sub/c.txt"]);
}

#[gpui_kit::test]
async fn the_legend_opens_lists_every_symbol_and_closes_on_escape(cx: &mut TestAppContext) {
    use duet_app::folder::view::legend::LEGEND;
    assert_eq!(LEGEND.len(), 13);
    assert_eq!(LEGEND[0], ("≠", "Different: the contents differ"));
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    assert!(!present(cx, handle, "folder-legend-panel"));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("folder-legend", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(view.read_with(cx, |v, _| v.legend_open()));
    assert!(present(cx, handle, "folder-legend-panel"));
    for i in 0..LEGEND.len() {
        assert!(present_row(cx, handle, i), "legend row {i}");
    }
    cx.update_window(handle, |_, window, cx| {
        let input = view.read(cx).name_filter_input().clone();
        window.focus(&input.focus_handle(cx), cx);
    })
    .unwrap();
    press(cx, handle, "escape");
    assert!(!present(cx, handle, "folder-legend-panel"), "Esc closes it");
    let rows_focused = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            view.read(cx).focus_handle(cx).is_focused(window)
        })
        .unwrap()
    };
    assert!(!rows_focused(cx), "the first Esc only closed the legend");
    press(cx, handle, "escape");
    assert!(rows_focused(cx), "a second Esc gives the rows the focus");
    cx.update_window(handle, |_, window, cx| {
        window.click("folder-legend", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(present(cx, handle, "folder-legend-panel"));
    cx.update_window(handle, |_, window, cx| {
        window.click("folder-legend", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(
        !present(cx, handle, "folder-legend-panel"),
        "the button toggles it"
    );
}

fn present_row(cx: &mut TestAppContext, handle: AnyWindowHandle, index: usize) -> bool {
    cx.update_window(handle, |_, window, _| {
        window.try_find(("folder-legend-row", index)).is_some()
    })
    .unwrap()
}

#[gpui_kit::test]
async fn folder_rows_show_their_size_and_newest_time(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(&l.path().join("d/a.txt"), "abc", 100);
    put_at(&l.path().join("d/b.txt"), "abcd", 200);
    put(&r.path().join("other.txt"), "x");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let d = rels(&view, cx).iter().position(|rel| rel == "d").unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(("folder-size-left", d)).label(), Some("7"));
        assert!(window.try_find(("folder-size-right", d)).is_none());
    })
    .unwrap();
}

// ---- Task 8: the flat list ----

fn click(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
}

fn label(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> Option<String> {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window
            .try_find(id)
            .and_then(|e| e.label().map(str::to_owned))
    })
    .unwrap()
}

fn sort_of(view: &Entity<FolderView>, cx: &mut TestAppContext) -> (SortKey, bool) {
    view.read_with(cx, |v, _| v.model().unwrap().sort())
}

fn layout_of(view: &Entity<FolderView>, cx: &mut TestAppContext) -> Layout {
    view.read_with(cx, |v, _| v.model().unwrap().layout())
}

fn go_flat(view: &Entity<FolderView>, cx: &mut TestAppContext) {
    view.update(cx, |v, cx| {
        v.set_filter(FolderFilter::All, cx);
        v.set_layout(Layout::Flat, cx);
    });
}

#[gpui_kit::test]
async fn the_view_switch_shows_a_flat_sortable_list_and_is_saved(cx: &mut TestAppContext) {
    let data = use_settings(cx);
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    assert_eq!(
        label(cx, handle, "folder-layout").as_deref(),
        Some("View: tree")
    );
    assert!(!present(cx, handle, "folder-flat-header"));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.find("folder-expand-all").focused().is_some(),
            "a tab stop"
        );
    })
    .unwrap();

    click(cx, handle, "folder-layout");
    assert_eq!(layout_of(&view, cx), Layout::Flat);
    assert_eq!(saved_setting(&data, "folder.view"), "flat");
    assert!(present(cx, handle, "folder-flat-header"));
    assert!(!present(cx, handle, "folder-header"));
    assert_eq!(
        label(cx, handle, "folder-layout").as_deref(),
        Some("View: flat list")
    );
    assert_eq!(
        label(cx, handle, "folder-sort-path").as_deref(),
        Some("Path ▲")
    );
    assert_eq!(
        label(cx, handle, "folder-sort-left-size").as_deref(),
        Some("Size (left)")
    );
    for id in [
        "folder-sort-status",
        "folder-sort-right-size",
        "folder-sort-left-time",
        "folder-sort-right-time",
    ] {
        assert!(present(cx, handle, id), "{id}");
    }
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        // A disabled control is no tab stop (see ui_style.rs).
        assert_eq!(window.find("folder-expand-all").focused(), None);
        assert_eq!(window.find("folder-collapse-all").focused(), None);
    })
    .unwrap();
    view.update(cx, |v, cx| v.set_filter(FolderFilter::All, cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("folder-flat-row", 0usize)).is_some());
        assert!(window.try_find(("folder-row", 0usize)).is_none());
    })
    .unwrap();

    click(cx, handle, "folder-sort-left-size");
    assert_eq!(sort_of(&view, cx), (SortKey::LeftSize, false));
    assert_eq!(
        label(cx, handle, "folder-sort-left-size").as_deref(),
        Some("Size (left) ▲")
    );
    click(cx, handle, "folder-sort-left-size");
    assert_eq!(sort_of(&view, cx), (SortKey::LeftSize, true));
    assert_eq!(
        label(cx, handle, "folder-sort-left-size").as_deref(),
        Some("Size (left) ▼")
    );
    assert_eq!(
        label(cx, handle, "folder-sort-path").as_deref(),
        Some("Path")
    );

    // A refresh keeps the layout and the sort.
    view.update(cx, |v, cx| v.refresh(cx));
    wait_idle(cx, handle, &view).await;
    assert_eq!(layout_of(&view, cx), Layout::Flat);
    assert_eq!(sort_of(&view, cx), (SortKey::LeftSize, true));

    click(cx, handle, "folder-layout");
    assert_eq!(layout_of(&view, cx), Layout::Tree);
    assert_eq!(saved_setting(&data, "folder.view"), "tree");
    assert!(present(cx, handle, "folder-header"));
}

#[gpui_kit::test]
async fn a_saved_flat_view_opens_flat(cx: &mut TestAppContext) {
    let data = use_settings(cx);
    std::fs::write(
        data.path().join("settings.json"),
        r#"{"folder.view": "flat"}"#,
    )
    .unwrap();
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    assert_eq!(layout_of(&view, cx), Layout::Flat);
    assert!(present(cx, handle, "folder-flat-header"));
}

#[gpui_kit::test]
async fn a_flat_row_shows_the_path_status_sizes_and_times(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(&l.path().join("d/b.txt"), "left", 100);
    put_at(&r.path().join("d/b.txt"), "right side", 200);
    put_at(&l.path().join("same.txt"), "same", 100);
    put_at(&r.path().join("same.txt"), "same", 100);
    put(&r.path().join("only.txt"), "x");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    go_flat(&view, cx);
    assert_eq!(rels(&view, cx), ["d/b.txt", "only.txt", "same.txt"]);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find(("folder-flat-path", 0usize)).label(),
            Some("d/b.txt")
        );
        assert_eq!(
            window.find(("folder-flat-status", 0usize)).label(),
            Some("Different")
        );
        assert_eq!(
            window.find(("folder-flat-status", 1usize)).label(),
            Some("Right only")
        );
        assert_eq!(
            window.find(("folder-flat-status", 2usize)).label(),
            Some("Same")
        );
        assert_eq!(
            window.find(("folder-flat-size-left", 0usize)).label(),
            Some("4")
        );
        assert_eq!(
            window.find(("folder-flat-size-right", 0usize)).label(),
            Some("10")
        );
        let right = window.find(("folder-flat-time-right", 0usize));
        assert!(
            right.label().is_some_and(|t| t.starts_with("▲ ")),
            "{:?}",
            right.label()
        );
        let left = window.find(("folder-flat-time-left", 0usize));
        assert!(left.label().is_some_and(|t| !t.starts_with('▲')));
    })
    .unwrap();
}

#[gpui_kit::test]
async fn copy_and_delete_work_on_flat_rows(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    go_flat(&view, cx);
    select(&view, cx, "new.txt");
    press(cx, handle, "alt-left");
    folder_support::wait_for_prompt(cx, &view).await;
    let (message, _) = cx.pending_prompt().expect("a prompt");
    assert!(message.starts_with("Copy 1 file"), "{message}");
    cx.simulate_prompt_answer("Copy");
    let target = l.path().join("new.txt");
    let seen = target.clone();
    folder_support::wait_until(cx, handle, move || seen.exists()).await;
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
    wait_idle(cx, handle, &view).await;
    assert_eq!(
        layout_of(&view, cx),
        Layout::Flat,
        "the refresh after the copy keeps the flat list"
    );

    // M-9: re-sorted by Status, high to low, a Delete acts on the file its row shows.
    let trashed: Arc<Mutex<Vec<std::path::PathBuf>>> = Arc::default();
    let log = trashed.clone();
    view.update(cx, |v, cx| {
        v.set_trash_for_test(move |p: &std::path::Path| {
            let gone = folder_support::remove_in_place(p);
            log.lock().unwrap().push(p.to_path_buf());
            gone
        });
        v.set_filter(FolderFilter::All, cx);
        v.sort_by(SortKey::Status, cx);
        v.sort_by(SortKey::Status, cx);
    });
    assert_eq!(sort_of(&view, cx), (SortKey::Status, true));
    assert_eq!(
        rels(&view, cx),
        ["a.txt", "new.txt", "sub/c.txt", "b.txt"],
        "same, then left only, then different"
    );
    select(&view, cx, "sub/c.txt");
    assert!(folder_support::run_menu_item(
        cx,
        handle,
        &view,
        "Delete left (to Recycle Bin)…"
    ));
    folder_support::wait_for_prompt(cx, &view).await;
    let (message, _) = cx.pending_prompt().expect("a prompt");
    assert_eq!(message, "Move 1 item to the Recycle Bin?");
    cx.simulate_prompt_answer("Move to Recycle Bin");
    let moved = trashed.clone();
    folder_support::wait_until(cx, handle, move || !moved.lock().unwrap().is_empty()).await;
    let gone = l.path().join("sub").join("c.txt");
    assert_eq!(
        trashed.lock().unwrap().as_slice(),
        std::slice::from_ref(&gone)
    );
    assert!(!gone.exists());
    for name in ["a.txt", "b.txt", "new.txt"] {
        assert!(l.path().join(name).is_file(), "{name}");
        assert!(r.path().join(name).is_file(), "{name}");
    }
}

#[gpui_kit::test]
async fn arrow_keys_do_not_expand_in_the_flat_list(cx: &mut TestAppContext) {
    let (l, r) = fixture();
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    go_flat(&view, cx);
    select(&view, cx, "sub/c.txt");
    let before = rels(&view, cx);
    let row = view.read_with(cx, |v, _| v.model().unwrap().selected_row());
    press(cx, handle, "right");
    press(cx, handle, "left");
    assert_eq!(rels(&view, cx), before);
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().selected_row()),
        row,
        "Left does not move the cursor to the parent folder"
    );
}

#[cfg(windows)]
fn mklink_junction(link: &std::path::Path, target: &std::path::Path) {
    let out = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
}

/// K2 (P10b fix wave, ruling C22): a folder above the picked file that became a junction
/// after the compare is never followed by Delete. The file there looks unchanged (it is the
/// same file, moved), so the plans keep it; the run-time check refuses it, lists it as a
/// failure and never calls the Recycle Bin.
#[cfg(windows)]
#[gpui_kit::test]
async fn delete_never_follows_a_junction_made_after_the_compare(cx: &mut TestAppContext) {
    let (l, r, outside) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    put(&l.path().join("keep/a.txt"), "a");
    put(&r.path().join("keep/a.txt"), "b");
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    let moved = outside.path().join("keep");
    std::fs::rename(l.path().join("keep"), &moved).unwrap();
    mklink_junction(&l.path().join("keep"), &moved);
    let trashed: Arc<Mutex<Vec<std::path::PathBuf>>> = Arc::default();
    let log = trashed.clone();
    view.update(cx, |v, _| {
        v.set_trash_for_test(move |p: &std::path::Path| {
            log.lock().unwrap().push(p.to_path_buf());
            Ok(())
        })
    });
    select(&view, cx, "keep/a.txt");
    assert!(folder_support::run_menu_item(
        cx,
        handle,
        &view,
        "Delete left (to Recycle Bin)…"
    ));
    folder_support::wait_for_prompt(cx, &view).await;
    assert_eq!(
        cx.pending_prompt().unwrap().0,
        "Move 1 item to the Recycle Bin?"
    );
    cx.simulate_prompt_answer("Move to Recycle Bin");
    let v = view.clone();
    cx.wait_for(handle, std::time::Duration::from_secs(20), move |_, cx| {
        v.read(cx).notice().is_some_and(|n| n.starts_with("Moved "))
    })
    .await;
    assert!(
        trashed.lock().unwrap().is_empty(),
        "the Recycle Bin is never reached"
    );
    let notice = view.read_with(cx, |v, _| v.notice().unwrap_or_default().to_owned());
    assert!(notice.starts_with("Moved 0 items; 1 failed"), "{notice}");
    assert!(notice.contains("link or junction"), "{notice}");
    assert!(moved.join("a.txt").is_file());
    folder_support::wait_for_prompt(cx, &view).await;
    assert_eq!(cx.pending_prompt().unwrap().0, "1 item failed.");
    cx.simulate_prompt_answer("OK");
}

/// A press in the legend closes it (F-56) and stops there: the row under the panel stays
/// unselected.
#[gpui_kit::test]
async fn a_press_in_the_legend_leaves_the_rows_under_it_alone(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for i in 0..40 {
        let name = format!("file{i:03}.txt");
        std::fs::write(l.path().join(&name), "left").unwrap();
        std::fs::write(r.path().join(&name), "right!").unwrap();
    }
    let (handle, view) = open_folder(cx, l.path(), r.path());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.toggle_legend(cx));
    let selected =
        |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.model().unwrap().selected_row());
    let before = selected(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("folder-legend-row", 12usize), cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(view.read_with(cx, |v, _| !v.legend_open()), "it closes");
    assert_eq!(selected(cx), before, "the press does not reach the list");
}
