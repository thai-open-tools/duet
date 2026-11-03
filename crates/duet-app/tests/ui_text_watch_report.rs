//! Headless UI tests for the text view's warnings, changed-on-disk notice, invisible characters
//! and reports (P9b Tasks 9 to 11).

mod text_support;

use std::path::Path;

use duet_app::palette::Palette;
use duet_app::text::view::TextView;
use duet_app::text::watch::WATCH_INTERVAL;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AnyWindowHandle, AppContext, Entity, Focusable as _, TestAppContext};
use text_support::*;

// Task 9: warnings and the changed-on-disk notice.

fn drawn(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
    .unwrap()
}

/// Both polls (the second confirms) and the work they start.
fn check_disk(cx: &mut TestAppContext, view: &Entity<TextView>) {
    view.update(cx, |v, cx| v.check_disk_for_test(cx));
    cx.run_until_parked();
}

fn disk_notice(cx: &mut TestAppContext, view: &Entity<TextView>) -> Option<String> {
    view.read_with(cx, |v, _| v.disk_notice())
}

#[gpui_kit::test]
async fn the_status_bar_shows_warnings(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text_bytes(cx, b"a\nb\n", b"a\r\nb\r\n");
    wait_loaded(cx, handle, &view).await;
    assert!(drawn(cx, handle, "text-warnings"));
    assert_eq!(
        view.read_with(cx, |v, _| v.warnings()).as_deref(),
        Some("⚠ Line endings differ (LF / CRLF)")
    );
    let (_dir, handle, view) = open_text_bytes(cx, b"a\nb\n", b"a\nb\n");
    wait_loaded(cx, handle, &view).await;
    assert!(!drawn(cx, handle, "text-warnings"));
}

#[gpui_kit::test]
async fn a_file_changed_by_another_program_shows_reload_and_ignore(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let path = dir.path().join("b.txt");
    assert_eq!(disk_notice(cx, &view), None);
    std::fs::write(&path, "changed\n").unwrap();
    touch(&path, 1_000_000_000);
    check_disk(cx, &view);
    assert_eq!(
        disk_notice(cx, &view).as_deref(),
        Some("⚠ b.txt (right) was changed by another program.")
    );
    assert!(drawn(cx, handle, "text-disk-notice"));

    // Ignore: no bar, and none at the next check.
    cx.update_window(handle, |_, window, cx| window.click("text-disk-ignore", cx))
        .unwrap();
    assert!(!drawn(cx, handle, "text-disk-notice"));
    check_disk(cx, &view);
    assert_eq!(disk_notice(cx, &view), None);
    // Changed again: the bar is back.
    std::fs::write(&path, "changed again\n").unwrap();
    touch(&path, 1_000_000_100);
    check_disk(cx, &view);
    assert!(disk_notice(cx, &view).is_some());

    // Reload shows the new text and the bar goes.
    cx.update_window(handle, |_, window, cx| window.click("text-disk-reload", cx))
        .unwrap();
    wait_loaded(cx, handle, &view).await;
    let shown = view.read_with(cx, |v, _| {
        v.model().unwrap().right().text().map(str::to_owned)
    });
    assert_eq!(shown.as_deref(), Some("changed again\n"));
    assert_eq!(disk_notice(cx, &view), None);
    check_disk(cx, &view);
    assert_eq!(disk_notice(cx, &view), None);
    assert!(!drawn(cx, handle, "text-disk-notice"));
}

#[gpui_kit::test]
async fn one_poll_alone_shows_nothing_yet(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let path = dir.path().join("b.txt");
    std::fs::write(&path, "changed\n").unwrap();
    touch(&path, 1_000_000_000);
    // The timer's polls: the first sees the change, the next one confirms it (PF7).
    cx.executor().advance_clock(WATCH_INTERVAL);
    cx.run_until_parked();
    assert_eq!(disk_notice(cx, &view), None, "one poll alone shows nothing");
    // No frame is drawn here: the first tick asked for one, so the timer goes on (M13).
    cx.executor().advance_clock(WATCH_INTERVAL);
    cx.run_until_parked();
    assert!(disk_notice(cx, &view).is_some());
}

#[gpui_kit::test]
async fn our_own_save_never_shows_the_notice(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(false, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    wait_clean(cx, &view).await;
    check_disk(cx, &view);
    assert_eq!(disk_notice(cx, &view), None);
}

#[gpui_kit::test]
async fn a_deleted_file_says_so(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    std::fs::remove_file(dir.path().join("b.txt")).unwrap();
    check_disk(cx, &view);
    assert_eq!(
        disk_notice(cx, &view).as_deref(),
        Some("⚠ b.txt (right) was deleted or moved.")
    );
}

#[gpui_kit::test]
async fn f9_moves_the_notice_to_the_other_side(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let path = dir.path().join("b.txt");
    std::fs::write(&path, "changed\n").unwrap();
    touch(&path, 1_000_000_000);
    check_disk(cx, &view);
    assert!(disk_notice(cx, &view).unwrap().contains("(right)"));
    press(cx, handle, "f9");
    assert_eq!(
        disk_notice(cx, &view).as_deref(),
        Some("⚠ b.txt (left) was changed by another program.")
    );
    check_disk(cx, &view);
    assert!(disk_notice(cx, &view).unwrap().contains("(left)"));
}

#[gpui_kit::test]
async fn untitled_sides_are_not_polled(cx: &mut TestAppContext) {
    let (handle, view) = open_test_window(cx, |window, cx| {
        let view = cx.new(|cx| TextView::new("".into(), "".into(), cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    wait_loaded(cx, handle, &view).await;
    check_disk(cx, &view);
    assert_eq!(disk_notice(cx, &view), None);
}

/// A window that shows the text view as a tab, or another tab in its place.
struct Host {
    text: Entity<TextView>,
    shown: bool,
}

impl gpui_kit::Render for Host {
    fn render(
        &mut self,
        _: &mut gpui_kit::Window,
        _: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        use gpui_kit::{ParentElement as _, Styled as _};
        gpui_kit::div()
            .size_full()
            .children(self.shown.then(|| self.text.clone()))
    }
}

#[gpui_kit::test]
async fn polling_stops_when_the_tab_is_not_drawn(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "a\n").unwrap();
    std::fs::write(&b, "b\n").unwrap();
    let (handle, host) = open_test_window(cx, move |_, cx| {
        let text = cx.new(|cx| TextView::new(a, b, cx));
        cx.new(|_| Host { text, shown: true })
    });
    let view = host.read_with(cx, |host, _| host.text.clone());
    wait_loaded(cx, handle, &view).await;
    render(cx, handle);
    assert!(view.read_with(cx, |v, _| v.is_watching_for_test()));
    // Another tab comes to the front: the window draws no frame of this one, not even the one
    // its tick asks for, so the second tick ends the timer.
    let show = |cx: &mut TestAppContext, shown: bool| {
        host.update(cx, |host, cx| {
            host.shown = shown;
            cx.notify();
        })
    };
    show(cx, false);
    render(cx, handle);
    cx.executor().advance_clock(WATCH_INTERVAL);
    cx.run_until_parked();
    cx.executor().advance_clock(WATCH_INTERVAL);
    cx.run_until_parked();
    assert!(!view.read_with(cx, |v, _| v.is_watching_for_test()));
    // Shown again, its first frame starts it.
    show(cx, true);
    render(cx, handle);
    assert!(view.read_with(cx, |v, _| v.is_watching_for_test()));
}

#[gpui_kit::test]
async fn a_drawn_tab_asks_for_a_frame_at_each_tick(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    render(cx, handle);
    let notified = std::rc::Rc::new(std::cell::Cell::new(0));
    let _observed = cx.update(|cx| {
        let notified = notified.clone();
        cx.observe(&view, move |_, _| notified.set(notified.get() + 1))
    });
    cx.executor().advance_clock(WATCH_INTERVAL);
    cx.run_until_parked();
    assert!(
        notified.get() > 0,
        "the tick asks for a frame, so a shown tab nothing redraws keeps polling (M13)"
    );
    // The window drew that frame, so the next tick polls again.
    cx.executor().advance_clock(WATCH_INTERVAL);
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.is_watching_for_test()));
}

// Task 10: invisible characters.

/// Lets the background scans of invisible characters land.
fn settle(cx: &mut TestAppContext) {
    cx.run_until_parked();
}

fn odd_text(cx: &mut TestAppContext, view: &Entity<TextView>) -> Option<String> {
    view.read_with(cx, |v, _| v.odd_button_text())
}

/// A click on `text-odd` with Shift held.
fn shift_click_odd(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let position = window.find("text-odd").bounds().center();
        pointer(window, move_to(position, false), cx);
        let modifiers = gpui_kit::Modifiers::shift();
        let down = gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
            button: gpui_kit::MouseButton::Left,
            position,
            modifiers,
            click_count: 1,
            first_mouse: false,
        });
        let up = gpui_kit::PlatformInput::MouseUp(gpui_kit::MouseUpEvent {
            button: gpui_kit::MouseButton::Left,
            position,
            modifiers,
            click_count: 1,
        });
        pointer(window, down, cx);
        pointer(window, up, cx);
    })
    .unwrap();
}

#[gpui_kit::test]
async fn invisible_characters_are_counted_and_visited_in_order(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_named(cx, "b.rs", "x\n", "a\u{a0}b\n// \u{200b}\n");
    wait_loaded(cx, handle, &view).await;
    settle(cx);
    assert_eq!(
        odd_text(cx, &view).as_deref(),
        Some("⚠ 1 invisible character in the text")
    );
    assert_eq!(view.read_with(cx, |v, _| v.odd_counts()), Some((1, 1)));
    assert!(drawn(cx, handle, "text-odd"));
    let tip = view.read_with(cx, |v, _| v.odd_tooltip()).unwrap();
    assert!(
        tip.starts_with("Left 0 · Right 2. Click for the next one (Shift+click: previous).\n"),
        "{tip}"
    );
    assert!(tip.contains("Right line 1: U+00A0 no-break space (in the text)"));
    assert!(tip.contains("Right line 2: U+200B zero-width space (in a comment)"));
    assert!(
        tip.ends_with("Invisible characters inside values can make equal-looking text differ.")
    );

    cx.update_window(handle, |_, window, cx| window.click("text-odd", cx))
        .unwrap();
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("1 / 2 · Right line 1: U+00A0 no-break space (in the text)")
    );
    let row = view.read_with(cx, |v, _| v.model().unwrap().row_of_line(false, 0).unwrap());
    assert_eq!(selected_rows(cx, &view), Some(row..row + 1));
    assert!(!view.read_with(cx, |v, _| v.row_selection().unwrap().left));

    cx.update_window(handle, |_, window, cx| window.click("text-odd", cx))
        .unwrap();
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("2 / 2 · Right line 2: U+200B zero-width space (in a comment)")
    );
    let row = view.read_with(cx, |v, _| v.model().unwrap().row_of_line(false, 1).unwrap());
    assert_eq!(selected_rows(cx, &view), Some(row..row + 1));

    shift_click_odd(cx, handle);
    assert!(
        notice(cx, &view)
            .unwrap()
            .starts_with("1 / 2 · Right line 1:")
    );
    // Wraps round both ways.
    assert!(view.update(cx, |v, cx| v.next_odd(false, cx)));
    assert!(notice(cx, &view).unwrap().starts_with("2 / 2"));
    assert!(view.update(cx, |v, cx| v.next_odd(true, cx)));
    assert!(notice(cx, &view).unwrap().starts_with("1 / 2"));
}

#[gpui_kit::test]
async fn comment_and_indent_only_hits_read_as_info(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_named(cx, "b.rs", "x\n", "// \u{200b}\n");
    wait_loaded(cx, handle, &view).await;
    settle(cx);
    assert_eq!(
        odd_text(cx, &view).as_deref(),
        Some("1 invisible character in comments or indentation")
    );
    assert_eq!(view.read_with(cx, |v, _| v.odd_counts()), Some((0, 1)));
    let tip = view.read_with(cx, |v, _| v.odd_tooltip()).unwrap();
    assert!(!tip.contains("equal-looking"), "{tip}");
}

#[gpui_kit::test]
async fn no_invisible_characters_no_button(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nc\n");
    wait_loaded(cx, handle, &view).await;
    settle(cx);
    assert_eq!(odd_text(cx, &view), None);
    assert_eq!(view.read_with(cx, |v, _| v.odd_counts()), None);
    assert!(!drawn(cx, handle, "text-odd"));
    assert!(!view.update(cx, |v, cx| v.next_odd(true, cx)));
}

#[gpui_kit::test]
async fn an_emoji_joiner_is_not_an_invisible_character(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "x\n", "family 👨\u{200d}👩\u{200d}👧\n");
    wait_loaded(cx, handle, &view).await;
    settle(cx);
    assert_eq!(odd_text(cx, &view), None);
}

#[gpui_kit::test]
async fn an_edit_rescans(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "ab\n", "ab\n");
    wait_loaded(cx, handle, &view).await;
    settle(cx);
    assert_eq!(odd_text(cx, &view), None);
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("\u{a0}", cx))
        .unwrap();
    wait_loaded(cx, handle, &view).await;
    settle(cx);
    assert_eq!(
        odd_text(cx, &view).as_deref(),
        Some("⚠ 1 invisible character in the text")
    );
    // Only the side whose version moved is read again.
    assert_eq!(view.read_with(cx, |v, _| v.odd_scans_for_test()), [2, 1]);
    // Removing it again clears the button.
    press(cx, handle, "ctrl-z");
    wait_loaded(cx, handle, &view).await;
    settle(cx);
    assert_eq!(odd_text(cx, &view), None);
}

#[gpui_kit::test]
async fn the_rescan_after_an_edit_copies_no_text(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\u{200b}b\n", "ab\n");
    wait_loaded(cx, handle, &view).await;
    settle(cx);
    let scans = view.read_with(cx, |v, _| v.odd_scans_for_test());
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    wait_loaded(cx, handle, &view).await;
    settle(cx);
    let now = view.read_with(cx, |v, _| v.odd_scans_for_test());
    assert_eq!(
        now,
        [scans[0] + 1, scans[1]],
        "the edited side is read again"
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.odd_copies_for_test()),
        [0, 0],
        "from the document the re-diff left, not a copy of the buffer (I3)"
    );
    assert!(odd_text(cx, &view).is_some());
}

#[gpui_kit::test]
async fn the_hits_are_put_in_order_once_not_every_frame(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "\u{a0}x\na\u{200b}b\n", "ab\n");
    wait_loaded(cx, handle, &view).await;
    settle(cx);
    render(cx, handle);
    let sorts = view.read_with(cx, |v, _| v.odd_sorts_for_test());
    for _ in 0..3 {
        render(cx, handle);
    }
    assert_eq!(
        view.read_with(cx, |v, _| v.odd_sorts_for_test()),
        sorts,
        "frames reuse the order (M4)"
    );
    // The walk still goes text hits first.
    assert!(view.update(cx, |v, cx| v.next_odd(true, cx)));
    let said = notice(cx, &view).unwrap();
    assert!(said.starts_with("1 / 2 · Left line 2: "), "{said}");
    assert!(view.update(cx, |v, cx| v.next_odd(true, cx)));
    let said = notice(cx, &view).unwrap();
    assert!(said.starts_with("2 / 2 · Left line 1: "), "{said}");
}

#[gpui_kit::test]
async fn swapping_sides_keeps_each_scan_with_its_text(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "x\n", "a\u{a0}b\n");
    wait_loaded(cx, handle, &view).await;
    settle(cx);
    view.update(cx, |v, cx| v.swap_sides(cx));
    wait_loaded(cx, handle, &view).await;
    settle(cx);
    cx.update_window(handle, |_, window, cx| window.click("text-odd", cx))
        .unwrap();
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("1 / 1 · Left line 1: U+00A0 no-break space (in the text)")
    );
}

#[gpui_kit::test]
async fn markers_draw_red_and_indentation_markers_dim(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "\u{a0}a\u{a0}b\n", "\u{a0}a\u{a0}b\n");
    wait_loaded(cx, handle, &view).await;
    let p = Palette::light();
    let lines = view.update(cx, |v, _| v.segment_highlights_for_test(0, false));
    assert_eq!(lines.len(), 1);
    let colours: Vec<_> = lines[0]
        .iter()
        .map(|(range, style)| (range.clone(), style.color, style.background_color))
        .collect();
    assert_eq!(
        colours,
        vec![
            (0..3, Some(p.dim), None),
            (4..7, Some(p.del), Some(p.bg_del)),
        ]
    );
}

fn report_items(cx: &mut TestAppContext, view: &Entity<TextView>) -> Vec<String> {
    view.read_with(cx, |v, _| {
        duet_app::ui::menu::outline(&duet_app::text::report::report_menu(v))
    })
}

fn run_report(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
    label: &str,
) -> bool {
    let entries = view.read_with(cx, |v, _| duet_app::text::report::report_menu(v));
    let view = view.clone();
    let ran = cx
        .update_window(handle, |_, window, cx| {
            let focus = view.focus_handle(cx);
            duet_app::ui::menu::activate(entries, label, Some(&focus), window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    ran
}

/// Answers the save dialog with `out`; returns the folder the dialog started in.
fn pick_path(cx: &mut TestAppContext, out: &Path) -> std::path::PathBuf {
    assert!(
        cx.did_prompt_for_new_path(),
        "the report asks where to save"
    );
    let mut start = std::path::PathBuf::new();
    cx.simulate_new_path_selection(|dir| {
        start = dir.to_path_buf();
        Some(out.to_path_buf())
    });
    start
}

const REPORT_ITEMS: [&str; 5] = [
    "HTML report (side by side)…",
    "Patch file (unified diff)…",
    "Text report…",
    "-",
    "Copy patch to the clipboard",
];

#[gpui_kit::test]
async fn the_report_menu_lists_its_items_and_waits_for_a_result(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nc\n");
    let disabled = |items: &[&str]| -> Vec<String> {
        items
            .iter()
            .map(|item| {
                if *item == "-" {
                    (*item).to_owned()
                } else {
                    format!("({item})")
                }
            })
            .collect()
    };
    assert_eq!(
        report_items(cx, &view),
        disabled(&REPORT_ITEMS),
        "waits for the first result"
    );
    wait_loaded(cx, handle, &view).await;
    assert_eq!(report_items(cx, &view), REPORT_ITEMS);
    view.update(cx, |v, cx| v.reload(cx));
    assert_eq!(
        report_items(cx, &view),
        disabled(&REPORT_ITEMS),
        "waits while a reload runs"
    );
    wait_loaded(cx, handle, &view).await;
    assert_eq!(report_items(cx, &view), REPORT_ITEMS);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("text-report").is_some(),
            "the Report button"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
async fn copy_patch_puts_the_unified_diff_on_the_clipboard(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\n", "a\nB\nc\n");
    wait_loaded(cx, handle, &view).await;
    assert!(run_report(cx, handle, &view, "Copy patch to the clipboard"));
    wait_notice(cx, handle, &view, "Patch copied to the clipboard.").await;
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(
        copied.as_deref(),
        Some("--- a/a.txt\n+++ b/b.txt\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n")
    );
}

#[gpui_kit::test]
async fn the_patch_compares_plainly_with_line_endings_ignored(cx: &mut TestAppContext) {
    // The view compares line endings (a difference per line), the patch does not (R20).
    let (_dir, handle, view) = open_text(cx, "a\r\nb\r\nc\r\n", "a\nB\nc\n");
    wait_loaded(cx, handle, &view).await;
    assert!(run_report(cx, handle, &view, "Copy patch to the clipboard"));
    wait_notice(cx, handle, &view, "Patch copied to the clipboard.").await;
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(
        copied.as_deref(),
        Some("--- a/a.txt\n+++ b/b.txt\n@@ -1,3 +1,3 @@\n a\r\n-b\r\n+B\r\n c\r\n"),
        "content lines end like the left file"
    );
}

#[gpui_kit::test]
async fn the_patch_and_text_reports_save_where_asked(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\nb\nc\n", "a\nB\nc\n");
    wait_loaded(cx, handle, &view).await;

    let patch = dir.path().join("out.patch");
    assert!(run_report(cx, handle, &view, "Patch file (unified diff)…"));
    assert_eq!(
        pick_path(cx, &patch),
        dir.path(),
        "starts in the left file's folder"
    );
    wait_notice(
        cx,
        handle,
        &view,
        &format!("Exported: {}.", patch.display()),
    )
    .await;
    assert_eq!(
        std::fs::read(&patch).unwrap(),
        b"--- a/a.txt\n+++ b/b.txt\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n"
    );

    let text = dir.path().join("out.txt");
    assert!(run_report(cx, handle, &view, "Text report…"));
    assert_eq!(pick_path(cx, &text), dir.path());
    wait_notice(cx, handle, &view, &format!("Exported: {}.", text.display())).await;
    let written = String::from_utf8(std::fs::read(&text).unwrap()).unwrap();
    let status = status(cx, &view);
    assert!(
        written.starts_with(
            "\u{feff}duet: text compare report\r\nLeft : a.txt\r\nRight: b.txt\r\nDate : "
        ),
        "{written:?}"
    );
    assert!(
        written.ends_with(&format!(
            "\r\n{status}\r\n\r\n=== Difference 1 · Left 2-2 · Right 2-2 ===\r\n< b\r\n---\r\n> B\r\n"
        )),
        "{written:?}"
    );
}

#[gpui_kit::test]
async fn a_cancelled_save_dialog_writes_nothing(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    assert!(run_report(cx, handle, &view, "Patch file (unified diff)…"));
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    assert_eq!(notice(cx, &view), None);
    let files = std::fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(files, 2, "only the two inputs");
}

#[gpui_kit::test]
async fn the_html_report_asks_what_to_show_then_saves(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\nb\nc\nd\ne\n", "a\nb\nC\nd\ne\n");
    wait_loaded(cx, handle, &view).await;

    assert!(run_report(cx, handle, &view, "HTML report (side by side)…"));
    let (message, _) = cx.pending_prompt().unwrap();
    assert_eq!(message, "Which lines should the report show?");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(!cx.did_prompt_for_new_path(), "Cancel asks nothing more");
    assert_eq!(notice(cx, &view), None);

    assert!(run_report(cx, handle, &view, "HTML report (side by side)…"));
    cx.simulate_prompt_answer("Differences only");
    cx.run_until_parked();
    let out = dir.path().join("out.html");
    assert_eq!(pick_path(cx, &out), dir.path());
    wait_notice(cx, handle, &view, &format!("Exported: {}.", out.display())).await;
    let page = std::fs::read_to_string(&out).unwrap();
    assert!(page.starts_with("<!DOCTYPE html><html lang=\"en\">"));
    assert!(
        page.contains("<title>duet — a.txt vs b.txt</title>"),
        "{page}"
    );
    assert_eq!(page.matches("class=\"skip\"").count(), 2, "{page}");
    assert!(page.contains("<span class=\"ch\">c</span>"));
    assert!(
        !page.contains("<td class=\"l\">a</td>"),
        "same lines are hidden"
    );
    assert!(!page.contains("<script"));
}

#[gpui_kit::test]
async fn patch_and_text_reports_refuse_files_over_64_mib(cx: &mut TestAppContext) {
    const TOO_LARGE: &str = "Patch and text reports are not available for files over 64 MiB.";
    let (_dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    // As a side over 64 MiB is shown: read straight from disk.
    view.update(cx, |v, _| v.set_mapped_for_test([false, true]));
    for item in [
        "Copy patch to the clipboard",
        "Patch file (unified diff)…",
        "Text report…",
    ] {
        view.update(cx, |v, cx| v.show_notice("-", cx));
        assert!(run_report(cx, handle, &view, item));
        assert_eq!(notice(cx, &view).as_deref(), Some(TOO_LARGE), "{item}");
        assert!(!cx.did_prompt_for_new_path(), "{item} asks nothing");
    }
    assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()), None);
    // The HTML page stops at its row cap instead, so it is still offered.
    assert!(run_report(cx, handle, &view, "HTML report (side by side)…"));
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
}
