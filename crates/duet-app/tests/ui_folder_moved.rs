//! Headless UI tests for moved or renamed files in the folder compare (Plan C): the tag on both
//! rows, the Moved chip, the jump, and the first rows that wait for nothing.

mod folder_support;

use std::cell::RefCell;
use std::fs;
use std::rc::Rc;
use std::time::Duration;

use duet_app::folder::job::shallow;
use duet_app::folder::model::FolderFilter;
use duet_app::folder::prefs::Layout;
use duet_app::folder::view::{FolderEvent, FolderView};
use duet_core::cancel::CancelToken;
use duet_core::folder::run::{CompareMode, FolderOptions, FolderProgress, compare_folders};
use folder_support::{open_folder_with, put, rels, saved_setting, use_settings, wait_idle};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, InputEvent as _, TestAppContext};

fn content() -> FolderOptions {
    FolderOptions {
        mode: CompareMode::Content,
        find_moves: true,
        ..FolderOptions::default()
    }
}

/// left a.txt, right b.txt (the same bytes), and left only.txt.
fn renamed() -> (tempfile::TempDir, tempfile::TempDir) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("a.txt"), "the same bytes");
    put(&r.path().join("b.txt"), "the same bytes");
    put(&l.path().join("only.txt"), "nobody else has these");
    (l, r)
}

fn row_of(view: &Entity<FolderView>, cx: &mut TestAppContext, rel: &str) -> usize {
    view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (0..m.visible_len())
            .find(|&row| m.node_at(row).rel == rel)
            .unwrap_or_else(|| panic!("{rel} is not a row"))
    })
}

/// The tag of `row` as the last frame drew it: its label, or None when there is none.
fn tag_label(cx: &mut TestAppContext, handle: AnyWindowHandle, row: usize) -> Option<String> {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window
            .try_find(("folder-moved-tag", row))
            .map(|tag| tag.label().unwrap_or_default().to_owned())
    })
    .unwrap()
}

#[gpui_kit::test]
async fn a_renamed_file_shows_a_tag_on_both_rows_in_the_flat_list_and_the_tree(
    cx: &mut TestAppContext,
) {
    let (l, r) = renamed();
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), content());
    wait_idle(cx, handle, &view).await;
    for layout in [Layout::Tree, Layout::Flat] {
        view.update(cx, |v, cx| v.set_layout(layout, cx));
        let (a, b, only) = (
            row_of(&view, cx, "a.txt"),
            row_of(&view, cx, "b.txt"),
            row_of(&view, cx, "only.txt"),
        );
        assert_eq!(
            tag_label(cx, handle, a).as_deref(),
            Some("Moved ↔ b.txt"),
            "{layout:?}"
        );
        assert_eq!(
            tag_label(cx, handle, b).as_deref(),
            Some("Moved ↔ a.txt"),
            "{layout:?}"
        );
        assert_eq!(tag_label(cx, handle, only), None, "{layout:?}");
    }
    // The status pill and the totals still call them left only and right only.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let status = window.find(("folder-flat-status", 0usize));
        let label = status.label();
        assert!(
            matches!(label, Some("Left only" | "Right only")),
            "{label:?}"
        );
    })
    .unwrap();
    let counts = view.read_with(cx, |v, _| v.model().unwrap().counts());
    assert_eq!(
        (counts.left_only, counts.right_only, counts.moved),
        (2, 1, 1)
    );
}

#[gpui_kit::test]
async fn clicking_the_tag_selects_the_other_row_and_opens_its_folder(cx: &mut TestAppContext) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("a.txt"), "carried over");
    put(&r.path().join("deep/er/b.txt"), "carried over");
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), content());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_filter(FolderFilter::Moved, cx));
    assert_eq!(rels(&view, cx), ["deep", "a.txt"], "deep is collapsed");
    let a = row_of(&view, cx, "a.txt");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("folder-moved-tag", a), cx);
    })
    .unwrap();
    // The state the click made; no second frame is drawn for it.
    let (selected, open) = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        let deep = m
            .comparison()
            .nodes
            .iter()
            .position(|n| n.rel == "deep")
            .unwrap() as u32;
        (
            m.selected_node().map(|n| n.rel.clone()),
            m.is_expanded(deep),
        )
    });
    assert_eq!(selected.as_deref(), Some("deep/er/b.txt"));
    assert!(open, "the folder above the partner opened");
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned)),
        None
    );
}

#[gpui_kit::test]
async fn the_moved_chip_counts_pairs_and_lists_both_rows(cx: &mut TestAppContext) {
    let (l, r) = renamed();
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), content());
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("folder-chip-moved").label(), Some("Moved 1"));
        window.click("folder-chip-moved", cx);
    })
    .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().filter()),
        FolderFilter::Moved
    );
    assert_eq!(rels(&view, cx), ["a.txt", "b.txt"], "two rows for one pair");
    let status = view.read_with(cx, |v, _| v.status().to_owned());
    assert!(status.contains("Moved 1"), "{status}");
}

#[gpui_kit::test]
async fn quick_shows_no_tags_and_the_chip_tooltip_says_why(cx: &mut TestAppContext) {
    let (l, r) = renamed();
    let options = FolderOptions {
        mode: CompareMode::Quick,
        find_moves: true,
        ..FolderOptions::default()
    };
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), options);
    wait_idle(cx, handle, &view).await;
    let a = row_of(&view, cx, "a.txt");
    assert_eq!(tag_label(cx, handle, a), None);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("folder-chip-moved").label(), Some("Moved 0"));
        window.hover("folder-chip-moved", cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("tooltip-text").label(),
            Some("Moved files are found by reading contents. Choose Content or Text rules.")
        );
    })
    .unwrap();
}

#[gpui_kit::test]
async fn changing_the_criterion_to_content_tags_after_the_compare(cx: &mut TestAppContext) {
    let _data = use_settings(cx);
    let (l, r) = renamed();
    let options = FolderOptions {
        mode: CompareMode::Quick,
        find_moves: true,
        ..FolderOptions::default()
    };
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), options);
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_mode(CompareMode::Content, cx));
    wait_idle(cx, handle, &view).await;
    let a = row_of(&view, cx, "a.txt");
    assert_eq!(tag_label(cx, handle, a).as_deref(), Some("Moved ↔ b.txt"));
}

#[gpui_kit::test]
async fn first_rows_carry_no_tags_and_the_result_does(cx: &mut TestAppContext) {
    let (l, r) = renamed();
    let opts = content();
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), opts.clone());
    let generation = view.read_with(cx, |v, _| v.job_generation());
    let first = shallow(l.path(), r.path(), &opts);
    view.update(cx, |v, cx| v.apply_for_test(generation, first, false, cx));
    let a = row_of(&view, cx, "a.txt");
    assert_eq!(tag_label(cx, handle, a), None, "first rows have no tags");
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().counts().moved),
        0
    );
    let full = compare_folders(
        l.path(),
        r.path(),
        &opts,
        &FolderProgress::default(),
        &CancelToken::new(),
    );
    view.update(cx, |v, cx| v.apply_for_test(generation, full, true, cx));
    let a = row_of(&view, cx, "a.txt");
    assert_eq!(tag_label(cx, handle, a).as_deref(), Some("Moved ↔ b.txt"));
}

#[gpui_kit::test]
async fn the_selection_note_names_the_other_path(cx: &mut TestAppContext) {
    let (l, r) = renamed();
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), content());
    wait_idle(cx, handle, &view).await;
    let a = row_of(&view, cx, "a.txt");
    view.update(cx, |v, cx| v.select_row_for_test(a, cx));
    let line = view.read_with(cx, |v, _| v.status_line().to_string());
    assert!(
        line.contains(
            "only on the left; the same contents are at b.txt on the right (moved or renamed)"
        ),
        "{line}"
    );
}

#[gpui_kit::test]
async fn after_renaming_back_and_f5_the_tags_are_gone(cx: &mut TestAppContext) {
    let (l, r) = renamed();
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), content());
    wait_idle(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().counts().moved),
        1
    );
    fs::rename(r.path().join("b.txt"), r.path().join("a.txt")).unwrap();
    view.update(cx, |v, cx| v.refresh(cx));
    wait_idle(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().counts().moved),
        0
    );
    let only = row_of(&view, cx, "only.txt");
    assert_eq!(tag_label(cx, handle, only), None);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("folder-chip-moved").label(), Some("Moved 0"));
    })
    .unwrap();
}

#[gpui_kit::test]
async fn a_long_other_path_is_cut_inside_the_row(cx: &mut TestAppContext) {
    let long = [
        "a".repeat(50),
        "b".repeat(50),
        "c".repeat(50),
        "d".repeat(50),
    ]
    .join("/");
    long_other_path(cx, long).await;
}

#[gpui_kit::test]
async fn a_long_thai_other_path_is_cut_inside_the_row(cx: &mut TestAppContext) {
    let long = vec![
        "โฟลเดอร์งานเอกสารประจำปี".repeat(3);
        3
    ]
    .join("/");
    long_other_path(cx, long).await;
}

async fn long_other_path(cx: &mut TestAppContext, long: String) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("short.txt"), "same on both sides");
    put(
        &r.path().join(format!("{long}/long-name.txt")),
        "same on both sides",
    );
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), content());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_filter(FolderFilter::Moved, cx));
    view.update(cx, |v, cx| v.set_layout(Layout::Flat, cx));
    let row = row_of(&view, cx, "short.txt");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let tag = window.find(("folder-moved-tag", row)).bounds();
        let cell = window.find(("folder-flat-path", row)).bounds();
        let line = window.find(("folder-flat-row", row)).bounds();
        assert!(
            tag.left() >= cell.left() && tag.right() <= cell.right(),
            "the tag {tag:?} stays inside its cell {cell:?}"
        );
        assert!(tag.right() <= line.right(), "{tag:?} inside {line:?}");
        assert!(tag.size.width <= cell.size.width * 0.46, "{tag:?} {cell:?}");
    })
    .unwrap();
    view.update(cx, |v, cx| v.set_layout(Layout::Tree, cx));
    let row = row_of(&view, cx, "short.txt");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let tag = window.find(("folder-moved-tag", row)).bounds();
        let mid = window.find(("folder-mid", row)).bounds();
        assert!(
            tag.right() <= mid.left(),
            "the left cell holds it: {tag:?} {mid:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
async fn the_moved_tag_chip_and_note_read_in_thai(cx: &mut TestAppContext) {
    let _th = duet_app::i18n::scoped(duet_app::i18n::Lang::Th);
    let (l, r) = renamed();
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), content());
    wait_idle(cx, handle, &view).await;
    let a = row_of(&view, cx, "a.txt");
    assert_eq!(tag_label(cx, handle, a).as_deref(), Some("ย้าย ↔ b.txt"));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("folder-chip-moved").label(), Some("ย้าย 1"));
    })
    .unwrap();
    view.update(cx, |v, cx| v.select_row_for_test(a, cx));
    let line = view.read_with(cx, |v, _| v.status_line().to_string());
    assert!(line.contains("ย้าย 1"), "{line}");
    assert!(
        line.contains("เนื้อหาเดียวกันอยู่ที่ b.txt ฝั่งขวา (ย้ายหรือเปลี่ยนชื่อ)"),
        "{line}"
    );
    assert!(duet_app::i18n::missing_keys_for_test().is_empty());
}

#[gpui_kit::test]
async fn a_jump_the_chips_hide_switches_to_moved_and_says_so(cx: &mut TestAppContext) {
    let (l, r) = renamed();
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), content());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_filter(FolderFilter::LeftOnly, cx));
    let a = row_of(&view, cx, "a.txt");
    let node = view.read_with(cx, |v, _| v.model().unwrap().node_index(a));
    view.update(cx, |v, cx| v.jump_to_partner(node, cx));
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().filter()),
        FolderFilter::Moved
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("Showing Moved so both files are visible.")
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.prefs().filter),
        FolderFilter::LeftOnly,
        "the saved choice is not touched"
    );
}

#[gpui_kit::test]
async fn a_jump_the_path_filter_hides_names_the_path(cx: &mut TestAppContext) {
    let (l, r) = renamed();
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), content());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_filter(FolderFilter::All, cx));
    view.update(cx, |v, cx| v.set_name_filter("a.txt", cx));
    let a = row_of(&view, cx, "a.txt");
    let node = view.read_with(cx, |v, _| v.model().unwrap().node_index(a));
    let before = view.read_with(cx, |v, _| {
        v.model().unwrap().selected_node().map(|n| n.rel.clone())
    });
    view.update(cx, |v, cx| v.jump_to_partner(node, cx));
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("The other file is hidden by the path filter: b.txt")
    );
    let after = view.read_with(cx, |v, _| {
        v.model().unwrap().selected_node().map(|n| n.rel.clone())
    });
    assert_eq!(after, before, "nothing moved");
}

#[gpui_kit::test]
async fn swap_sides_keeps_the_tags_on_their_rows(cx: &mut TestAppContext) {
    let (l, r) = renamed();
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), content());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.swap_sides(cx));
    wait_idle(cx, handle, &view).await;
    let a = row_of(&view, cx, "a.txt");
    assert_eq!(tag_label(cx, handle, a).as_deref(), Some("Moved ↔ b.txt"));
}

fn selected_rel(view: &Entity<FolderView>, cx: &mut TestAppContext) -> Option<String> {
    view.read_with(cx, |v, _| {
        v.model().unwrap().selected_node().map(|n| n.rel.clone())
    })
}

fn tooltip_of(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    id: impl Into<gpui_kit::ElementId>,
) -> Option<String> {
    let id = id.into();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.hover(id.clone(), cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window
            .try_find("tooltip-text")
            .and_then(|tip| tip.label().map(str::to_owned))
    })
    .unwrap()
}

#[gpui_kit::test]
async fn turning_find_moved_files_off_drops_the_tags_and_disables_the_chip(
    cx: &mut TestAppContext,
) {
    let data = use_settings(cx);
    let (l, r) = renamed();
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), content());
    wait_idle(cx, handle, &view).await;
    let a = row_of(&view, cx, "a.txt");
    assert!(tag_label(cx, handle, a).is_some());
    let generation = view.read_with(cx, |v, _| v.job_generation());
    view.update(cx, |v, cx| v.set_find_moves(false, cx));
    assert_eq!(
        view.read_with(cx, |v, _| v.job_generation()),
        generation,
        "no read is needed to drop the tags"
    );
    assert_eq!(saved_setting(&data, "folder.find_moves"), false);
    assert_eq!(tag_label(cx, handle, a), None);
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().counts().moved),
        0
    );
    // The chip is disabled: a click changes nothing, and its tooltip says how to turn it on.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("folder-chip-moved", cx);
    })
    .unwrap();
    assert_ne!(
        view.read_with(cx, |v, _| v.model().unwrap().filter()),
        FolderFilter::Moved
    );
    assert_eq!(
        tooltip_of(cx, handle, "folder-chip-moved").as_deref(),
        Some("Finding moved files is off. Turn on Find moved files in Options.")
    );
    // On again: the folders are compared again and the tags come back.
    view.update(cx, |v, cx| v.set_find_moves(true, cx));
    wait_idle(cx, handle, &view).await;
    assert_eq!(saved_setting(&data, "folder.find_moves"), true);
    let a = row_of(&view, cx, "a.txt");
    assert_eq!(tag_label(cx, handle, a).as_deref(), Some("Moved ↔ b.txt"));
}

#[gpui_kit::test]
async fn the_chip_is_disabled_under_quick_and_says_why(cx: &mut TestAppContext) {
    let (l, r) = renamed();
    let options = FolderOptions {
        mode: CompareMode::Quick,
        find_moves: true,
        ..FolderOptions::default()
    };
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), options);
    wait_idle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("folder-chip-moved", cx);
    })
    .unwrap();
    assert_ne!(
        view.read_with(cx, |v, _| v.model().unwrap().filter()),
        FolderFilter::Moved
    );
}

#[gpui_kit::test]
async fn escape_in_the_moved_pass_keeps_the_comparison_without_tags(cx: &mut TestAppContext) {
    let (l, r) = renamed();
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), content());
    wait_idle(cx, handle, &view).await;
    // A compare that has finished everything but the search for moved files.
    let generation = view.update(cx, |v, _| v.begin_moves_pass_for_test());
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    assert!(
        view.read_with(cx, |v, _| v.is_busy()),
        "the job is stopped, not dropped: its result still arrives"
    );
    let finished = compare_folders(
        l.path(),
        r.path(),
        &FolderOptions {
            find_moves: false,
            ..content()
        },
        &FolderProgress::default(),
        &CancelToken::new(),
    );
    assert!(view.update(cx, |v, cx| v.apply_for_test(generation, finished, true, cx)));
    let (rows, moved, status) = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (m.counts().all, m.counts().moved, v.status().to_owned())
    });
    assert_eq!((rows, moved), (3, 0), "the comparison is kept, untagged");
    assert!(
        status.contains("Stopped looking for moved files"),
        "{status}"
    );
    assert!(!status.contains("Cancelled"), "{status}");
}

#[gpui_kit::test]
async fn a_tag_click_gives_the_rows_the_keys_back(cx: &mut TestAppContext) {
    let (l, r) = renamed();
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), content());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_filter(FolderFilter::Moved, cx));
    // Typing in the path field: it holds the focus, so the arrow keys are its.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("ctrl-f", cx);
    })
    .unwrap();
    let b = row_of(&view, cx, "b.txt");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("folder-moved-tag", b), cx);
    })
    .unwrap();
    assert_eq!(selected_rel(&view, cx).as_deref(), Some("a.txt"));
    cx.update_window(handle, |_, window, cx| window.press("down", cx))
        .unwrap();
    assert_eq!(
        selected_rel(&view, cx).as_deref(),
        Some("b.txt"),
        "Down moves the selection after a jump"
    );
}

/// Presses the left button twice at `row`, as the second half of a double click arrives: one
/// press with `click_count` 2, and its release.
fn second_press_of_a_double_click(cx: &mut TestAppContext, handle: AnyWindowHandle, row: usize) {
    use gpui_kit::{MouseButton, MouseDownEvent, MouseUpEvent};
    cx.update_window(handle, |_, window, cx| {
        let position = folder_support::row_left(window, cx, row);
        for press in [true, false] {
            let event = if press {
                MouseDownEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers: Default::default(),
                    click_count: 2,
                    first_mouse: false,
                }
                .to_platform_input()
            } else {
                MouseUpEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers: Default::default(),
                    click_count: 2,
                }
                .to_platform_input()
            };
            window.dispatch_event(event, cx);
        }
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
async fn the_second_press_of_a_double_click_on_a_tag_opens_nothing(cx: &mut TestAppContext) {
    let (l, r) = renamed();
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), content());
    wait_idle(cx, handle, &view).await;
    let opened: Rc<RefCell<Vec<std::path::PathBuf>>> = Rc::default();
    let sink = opened.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &FolderEvent, _| {
            if let FolderEvent::OpenFiles { left, .. } = event {
                sink.borrow_mut().push(left.clone());
            }
        })
        .detach()
    });
    let a = row_of(&view, cx, "a.txt");
    let only = row_of(&view, cx, "only.txt");
    // First press of the double click: on the tag, which jumps.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("folder-moved-tag", a), cx);
    })
    .unwrap();
    // The second press lands on whatever the pointer is over now.
    second_press_of_a_double_click(cx, handle, only);
    assert!(opened.borrow().is_empty(), "{:?}", opened.borrow());
    // A double click of its own, later, still opens.
    cx.update_window(handle, |_, window, cx| {
        use gpui_kit::{MouseButton, MouseDownEvent, MouseUpEvent};
        let position = folder_support::row_left(window, cx, only);
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
    })
    .unwrap();
    assert_eq!(opened.borrow().len(), 1, "{:?}", opened.borrow());
}

#[gpui_kit::test]
async fn the_name_wins_the_room_and_the_tag_goes_when_it_has_none(cx: &mut TestAppContext) {
    let name = format!("{}.txt", "n".repeat(70));
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join(&name), "same on both sides");
    put(&r.path().join("short.txt"), "same on both sides");
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), content());
    wait_idle(cx, handle, &view).await;
    for layout in [Layout::Tree, Layout::Flat] {
        view.update(cx, |v, cx| v.set_layout(layout, cx));
        // Draw once so the list width is known, then read the frame the tags are chosen in.
        let long = row_of(&view, cx, &name);
        let short = row_of(&view, cx, "short.txt");
        tag_label(cx, handle, long);
        assert_eq!(
            tag_label(cx, handle, long),
            None,
            "{layout:?}: a name this long leaves no room for a tag"
        );
        assert!(
            tag_label(cx, handle, short).is_some(),
            "{layout:?}: a short name has room"
        );
    }
}

#[gpui_kit::test]
async fn the_tag_tooltip_holds_the_whole_other_path(cx: &mut TestAppContext) {
    let long = ["a".repeat(30), "b".repeat(30), "c".repeat(30)].join("/");
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("short.txt"), "same on both sides");
    put(
        &r.path().join(format!("{long}/x.txt")),
        "same on both sides",
    );
    let (handle, view) = open_folder_with(cx, l.path(), r.path(), content());
    wait_idle(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_filter(FolderFilter::Moved, cx));
    view.update(cx, |v, cx| v.set_layout(Layout::Flat, cx));
    let row = row_of(&view, cx, "short.txt");
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let tip = tooltip_of(cx, handle, ("folder-moved-tag", row));
    assert_eq!(
        tip.as_deref(),
        Some(
            format!("Moved or renamed: the same contents are at {long}/x.txt on the right. Click to go there.")
                .as_str()
        )
    );
}
