//! Headless UI tests for the text view.

use std::path::Path;
use std::time::Duration;

use duet_app::palette::Palette;
use duet_app::text::job::{IgnoreChoice, TextOutcome};
use duet_app::text::model::TextFilter;
use duet_app::text::syntax::{SyntaxSide, SyntaxState};
use duet_app::text::view::TextView;
use duet_app::text::view::rows::{HalfFill, HalfLook};
use duet_app::text::view::stage::{CENTRE_GUTTER, NUMBER_CELL, OVERVIEW_WIDTH, TEXT_INSET};
use duet_app::ui::style::SCROLLBAR_WIDTH;
use duet_core::text::rows::RowKind;
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, Focusable, Pixels, Point, Render,
    TestAppContext, Window, WindowBounds, WindowOptions, point, px, size,
};

fn open_test_window<V: Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        duet_app::app::configure_theme(gpui_kit::component::ThemeMode::Light, cx);
        cx.bind_keys(duet_app::text::view::key_bindings());
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

fn open_text(
    cx: &mut TestAppContext,
    left: &str,
    right: &str,
) -> (tempfile::TempDir, AnyWindowHandle, Entity<TextView>) {
    open_text_bytes(cx, left.as_bytes(), right.as_bytes())
}

fn open_text_bytes(
    cx: &mut TestAppContext,
    left: &[u8],
    right: &[u8],
) -> (tempfile::TempDir, AnyWindowHandle, Entity<TextView>) {
    open_files(cx, (Path::new("a.txt"), left), (Path::new("b.txt"), right))
}

/// `open_text` with a chosen file name, the same on both sides, in two folders.
fn open_named(
    cx: &mut TestAppContext,
    name: &str,
    left: &str,
    right: &str,
) -> (tempfile::TempDir, AnyWindowHandle, Entity<TextView>) {
    let (a, b) = (Path::new("left").join(name), Path::new("right").join(name));
    open_files(cx, (&a, left.as_bytes()), (&b, right.as_bytes()))
}

/// Writes both files under a new temp folder (paths relative to it) and opens them.
fn open_files(
    cx: &mut TestAppContext,
    (left_path, left): (&Path, &[u8]),
    (right_path, right): (&Path, &[u8]),
) -> (tempfile::TempDir, AnyWindowHandle, Entity<TextView>) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join(left_path), dir.path().join(right_path));
    for (path, bytes) in [(&a, left), (&b, right)] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let (handle, view) = open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| TextView::new(a, b, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    (dir, handle, view)
}

async fn wait_loaded(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<TextView>) {
    let view = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !view.read(cx).is_busy()
    })
    .await;
}

fn press(cx: &mut TestAppContext, handle: AnyWindowHandle, key: &str) {
    cx.update_window(handle, |_, window, cx| window.press(key, cx))
        .unwrap();
}

#[gpui_kit::test]
async fn the_legend_shows_on_wide_windows_only(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("text-status").is_some());
        assert!(
            window.try_find("text-legend").is_none(),
            "1200 px is under 1250"
        );
    })
    .unwrap();
    cx.simulate_window_resize(handle, size(px(1400.), px(800.)));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let legend = window.find("text-legend").bounds();
        let status = window.find("text-status").bounds();
        assert!(
            legend.right() <= status.right() && legend.left() > status.center().x,
            "right-aligned"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
async fn text_view_loads_rows_and_status(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\n", "a\nB\nc\nd\n");
    wait_loaded(cx, handle, &view).await;
    let (rows, status) = view.read_with(cx, |v, _| {
        (
            v.model().unwrap().row_count(),
            v.message().map(str::to_owned),
        )
    });
    assert_eq!(rows, 4);
    assert_eq!(status, None);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("text-row", 1usize)).is_some());
        assert!(window.try_find("text-status").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
async fn keys_move_between_changes_and_switch_filters(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\nd\ne\nf\ng\n", "a\nB\nc\nd\ne\nf\nG\nh\n");
    wait_loaded(cx, handle, &view).await;
    let current =
        |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.model().unwrap().current_block());
    press(cx, handle, "f8");
    assert_eq!(current(cx), Some(0));
    press(cx, handle, "alt-down");
    assert_eq!(current(cx), Some(1));
    press(cx, handle, "f7");
    assert_eq!(current(cx), Some(0));
    press(cx, handle, "ctrl-2");
    let (filter, rows) = view.read_with(cx, |v, _| {
        (v.model().unwrap().filter(), v.model().unwrap().row_count())
    });
    assert_eq!((filter, rows), (TextFilter::Differences, 3));
    press(cx, handle, "ctrl-1");
    let kind = view.read_with(cx, |v, _| v.model().unwrap().row(7).unwrap().kind);
    assert_eq!(kind, RowKind::RightOnly);
}

#[gpui_kit::test]
async fn arrows_scroll_horizontally_and_home_resets(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "abcdefghijklmnop\n", "abcdefghijklmnoq\n");
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "right");
    press(cx, handle, "right");
    assert_eq!(
        view.read_with(cx, |v, _| v.display_options().skip_columns),
        16
    );
    press(cx, handle, "left");
    assert_eq!(
        view.read_with(cx, |v, _| v.display_options().skip_columns),
        8
    );
    press(cx, handle, "home");
    assert_eq!(
        view.read_with(cx, |v, _| v.display_options().skip_columns),
        0
    );
}

#[gpui_kit::test]
async fn binary_and_missing_files_show_a_message(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\0b", "a\0c");
    wait_loaded(cx, handle, &view).await;
    let message = view.read_with(cx, |v, _| v.message().map(str::to_owned));
    assert_eq!(message.as_deref(), Some("Binary files differ."));
    std::fs::remove_file(dir.path().join("b.txt")).unwrap();
    view.update(cx, |v, cx| v.reload(cx));
    wait_loaded(cx, handle, &view).await;
    let message = view
        .read_with(cx, |v, _| v.message().map(str::to_owned))
        .unwrap();
    assert!(message.contains("b.txt"), "{message}");
}

#[gpui_kit::test]
async fn ignore_options_rerun_the_diff(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a  b\n", "a b\n");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().block_count()),
        1
    );
    view.update(cx, |v, cx| {
        let mut options = v.request().options;
        options.whitespace = duet_core::text::diff::WhitespaceMode::All;
        v.set_diff_options(options, cx);
    });
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().block_count()),
        0
    );
}

#[test]
fn paths_used_by_the_fixtures_exist() {
    assert!(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").exists());
}

#[gpui_kit::test]
async fn overview_bar_is_drawn_and_clicks_scroll(cx: &mut TestAppContext) {
    let left: String = (0..2_000).map(|i| format!("line {i}\n")).collect();
    let right = left.replace("line 1500\n", "changed\n");
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("text-overview").is_some());
    })
    .unwrap();
    view.update(cx, |v, cx| v.scroll_to_fraction(0.75, cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find(("text-row", 1_500usize)).is_some(),
            "row 1500 is on screen"
        );
    })
    .unwrap();
}

fn top_row(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<TextView>) -> usize {
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    view.read_with(cx, |v, _| v.top_row())
}

fn long_pair() -> (String, String) {
    let left: String = (0..2_000).map(|i| format!("line {i}\n")).collect();
    let right = left.replace("line 1500\n", "changed\n");
    (left, right)
}

/// The message state is a centred 15 px title over a muted hint, as two elements.
fn assert_message_look(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let frame = window.find("text-message").bounds();
        let t = window.find("text-message-title").bounds();
        let h = window.find("text-message-hint").bounds();
        assert!(t.bottom() <= h.top(), "the hint sits under the title");
        let centre = |b: Bounds<Pixels>| (b.center().x - frame.center().x).abs();
        assert!(centre(t) < px(2.) && centre(h) < px(2.), "centred");
    })
    .unwrap();
}

#[gpui_kit::test]
async fn message_states_show_a_title_and_instructions(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\0b", "a\0c");
    wait_loaded(cx, handle, &view).await;
    let (title, hint) = view.read_with(cx, |v, _| {
        (
            v.message().map(str::to_owned),
            v.message_hint().map(str::to_owned),
        )
    });
    assert_eq!(title.as_deref(), Some("Binary files differ."));
    assert!(hint.unwrap().ends_with("press F5 to compare again."));
    assert_message_look(cx, handle);

    let (left, right) = long_pair();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    press(cx, handle, "escape");
    let (title, hint) = view.read_with(cx, |v, _| {
        (
            v.message().map(str::to_owned),
            v.message_hint().map(str::to_owned),
        )
    });
    assert_eq!(title.as_deref(), Some("Compare cancelled."));
    assert_eq!(hint.as_deref(), Some("Press F5 to compare again."));
    assert_message_look(cx, handle);
    cx.run_until_parked();
}

#[gpui_kit::test]
async fn escape_cancels_a_running_compare(cx: &mut TestAppContext) {
    let (left, right) = long_pair();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    press(cx, handle, "escape");
    let (busy, message) = view.read_with(cx, |v, _| (v.is_busy(), v.message().map(str::to_owned)));
    assert!(!busy);
    assert_eq!(message.as_deref(), Some("Compare cancelled."));
    cx.run_until_parked();
    assert!(
        view.read_with(cx, |v, _| v.model().is_none()),
        "the cancelled result is dropped"
    );
}

#[gpui_kit::test]
async fn page_and_arrow_keys_scroll_the_rows(cx: &mut TestAppContext) {
    let (left, right) = long_pair();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    assert_eq!(top_row(cx, handle, &view), 0);
    press(cx, handle, "pagedown");
    let page = top_row(cx, handle, &view);
    assert!(page > 10, "a page is many rows, got {page}");
    press(cx, handle, "down");
    assert_eq!(top_row(cx, handle, &view), page + 1);
    press(cx, handle, "up");
    press(cx, handle, "pageup");
    assert_eq!(top_row(cx, handle, &view), 0);
}

#[gpui_kit::test]
async fn dragging_the_overview_scrolls(cx: &mut TestAppContext) {
    let (left, right) = long_pair();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let bar = window.find("text-overview").bounds();
        let x = bar.left() + bar.size.width / 2.;
        let from = Point::new(x, bar.top() + px(2.));
        let to = Point::new(x, bar.top() + bar.size.height * 0.75);
        window.drag(from, to, cx);
    })
    .unwrap();
    let top = top_row(cx, handle, &view);
    assert!(
        (1_400..1_500).contains(&top),
        "row 1500 is centred, top row {top}"
    );
}

#[gpui_kit::test]
async fn the_scrollbar_drags_the_rows(cx: &mut TestAppContext) {
    let (left, right) = long_pair();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let list = window.find("text-list").bounds();
        let x = list.right() - px(8.);
        let (from, to) = (
            Point::new(x, list.top() + px(6.)),
            Point::new(x, list.bottom() - px(2.)),
        );
        window.drag(from, to, cx);
    })
    .unwrap();
    let top = top_row(cx, handle, &view);
    assert!(
        top > 1_500,
        "dragging the thumb to the bottom reaches the end, top row {top}"
    );
}

#[gpui_kit::test]
async fn the_vertical_scrollbar_is_eleven_pixels_wide(cx: &mut TestAppContext) {
    let (left, right) = long_pair();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    let drag_at = |cx: &mut TestAppContext, inset: f32| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let list = window.find("text-list").bounds();
            let x = list.right() - px(inset);
            window.drag(
                Point::new(x, list.top() + px(6.)),
                Point::new(x, list.bottom() - px(2.)),
                cx,
            );
        })
        .unwrap();
    };
    drag_at(cx, 14.);
    assert_eq!(
        top_row(cx, handle, &view),
        0,
        "14 px in is text, not the scrollbar"
    );
    drag_at(cx, 5.);
    assert!(
        top_row(cx, handle, &view) > 1_500,
        "5 px in is the scrollbar's thumb"
    );
    assert_eq!(duet_app::ui::style::SCROLLBAR_WIDTH, 11.);
}

#[gpui_kit::test]
async fn stale_results_never_replace_the_current_view(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nB\n");
    wait_loaded(cx, handle, &view).await;
    let first = view.read_with(cx, |v, _| v.job_generation());
    view.update(cx, |v, cx| {
        v.reload(cx);
        v.apply_for_test(first, TextOutcome::Failed("stale".into()), cx);
    });
    assert_eq!(
        view.read_with(cx, |v, _| v.message().map(str::to_owned)),
        None
    );
    wait_loaded(cx, handle, &view).await;
    let second = view.read_with(cx, |v, _| v.job_generation());
    view.update(cx, |v, cx| {
        v.apply_for_test(second, TextOutcome::Failed("late".into()), cx)
    });
    let (message, blocks) = view.read_with(cx, |v, _| {
        (
            v.message().map(str::to_owned),
            v.model().map(|m| m.block_count()),
        )
    });
    assert_eq!(message, None, "a finished job's late result is dropped too");
    assert_eq!(blocks, Some(1));
}

#[gpui_kit::test]
async fn reloading_a_large_pair_releases_the_old_model_first(cx: &mut TestAppContext) {
    // 66,560 lines of 1 KiB: just over the 64 MiB edit limit, so both sides are mapped.
    let line = format!("{}\n", "x".repeat(1023));
    let mut left = line.repeat(66_560);
    let right = left.clone();
    left.replace_range(0..5, "LEFT!");
    let (_dir, handle, view) = open_text(cx, &left, &right);
    drop((left, right));
    wait_loaded(cx, handle, &view).await;
    let reason = view.read_with(cx, |v, _| v.model().unwrap().left().read_only_reason());
    assert_eq!(
        reason,
        Some(duet_core::text::document::ReadOnlyReason::TooLarge)
    );
    view.update(cx, |v, cx| {
        v.set_filter(TextFilter::Differences, cx);
        v.reload(cx);
    });
    assert!(
        view.read_with(cx, |v, _| v.model().is_none() && v.is_busy()),
        "the old model is released while the new compare runs"
    );
    wait_loaded(cx, handle, &view).await;
    let filter = view.read_with(cx, |v, _| v.model().map(|m| m.filter()));
    assert_eq!(
        filter,
        Some(TextFilter::Differences),
        "the filter survives the reload"
    );
    // Forcing an encoding on a mapped side would decode the whole file into memory.
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.cycle_encoding(true, window, cx))
    })
    .unwrap();
    let (encoding, busy, notice) = view.read_with(cx, |v, _| {
        (
            v.request().left_encoding,
            v.is_busy(),
            v.notice().map(str::to_owned),
        )
    });
    assert_eq!(encoding, None);
    assert!(!busy);
    assert!(notice.is_some_and(|n| n.contains("64 MiB")));
}

#[gpui_kit::test]
async fn the_copy_button_copies_the_full_path(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("text-left-path").is_some());
        window.click("text-right-path-copy", cx);
    })
    .unwrap();
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied, Some(dir.path().join("b.txt").display().to_string()));
}

#[gpui_kit::test]
async fn the_toolbar_fits_one_row(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    // At 1400 px, the width U16 plans the rows for: "On save" and the font size joined this
    // row. `ui_drawn` checks the same width with the real fonts.
    cx.simulate_window_resize(handle, size(px(1400.), px(800.)));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let height = window.find("text-toolbar").bounds().size.height;
        assert!(height < px(48.), "one row of controls, got {height:?}");
    })
    .unwrap();
}

#[gpui_kit::test]
async fn choosing_an_encoding_and_an_ignore_option_reruns_the_compare(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "A b\n", "a  b\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.toggle_ignore(duet_app::text::job::IgnoreChoice::Case, cx)
    });
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.toggle_ignore(duet_app::text::job::IgnoreChoice::WhitespaceAll, cx)
    });
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().block_count()),
        0
    );
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.set_encoding(true, Some(encoding_rs::WINDOWS_1252), window, cx)
        })
    })
    .unwrap();
    wait_loaded(cx, handle, &view).await;
    let chosen = view.read_with(cx, |v, _| v.request().left_encoding);
    assert_eq!(chosen, Some(encoding_rs::WINDOWS_1252));
}

#[gpui_kit::test]
async fn f8_past_the_last_difference_wraps_with_a_notice(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\n", "A\nb\nC\n");
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "f8");
    press(cx, handle, "f8");
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned)),
        None
    );
    press(cx, handle, "f8");
    let (block, notice) = view.read_with(cx, |v, _| {
        (
            v.model().unwrap().current_block(),
            v.notice().map(str::to_owned),
        )
    });
    assert_eq!(block, Some(0));
    assert_eq!(notice.as_deref(), Some("Wrapped to the first difference."));
}

#[gpui_kit::test]
async fn clicking_a_fold_row_shows_the_hidden_lines(cx: &mut TestAppContext) {
    let left: String = (0..40).map(|i| format!("line {i}\n")).collect();
    let right = left
        .replace("line 0\n", "LINE 0\n")
        .replace("line 39\n", "LINE 39\n");
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "ctrl-3");
    // rows 0-3, a fold for rows 4-35 at display row 4, rows 36-39
    assert_eq!(view.read_with(cx, |v, _| v.model().unwrap().row_count()), 9);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("text-fold-more", 4usize), cx);
    })
    .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().row_count()),
        9 + 20
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("text-fold-all", 24usize), cx);
    })
    .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().row_count()),
        40
    );
}

async fn wait_found(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
) -> String {
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx).find_status().is_some_and(|s| s != "Searching…")
    })
    .await;
    view.read_with(cx, |v, _| v.find_status().unwrap())
}

#[gpui_kit::test]
async fn find_marks_hits_on_both_sides_and_f3_cycles(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "alpha\nbeta\nalpha beta\n", "beta\nalpha\ngamma\n");
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "ctrl-f");
    cx.update_window(handle, |_, window, cx| window.input("alpha", cx))
        .unwrap();
    assert_eq!(wait_found(cx, handle, &view).await, "3 matches");
    press(cx, handle, "f3");
    assert_eq!(
        view.read_with(cx, |v, _| v.find_status()),
        Some("1 of 3".into())
    );
    press(cx, handle, "shift-f3");
    assert_eq!(
        view.read_with(cx, |v, _| v.find_status()),
        Some("3 of 3".into()),
        "back over the top wraps"
    );
    view.update(cx, |v, cx| {
        v.set_find_scope(duet_app::text::find::FindScope::Left, cx)
    });
    assert_eq!(wait_found(cx, handle, &view).await, "2 matches");
    press(cx, handle, "escape");
    assert!(
        !view.read_with(cx, |v, _| v.find_open()),
        "Esc closes the find bar first"
    );
}

#[gpui_kit::test]
async fn a_match_hidden_by_the_filter_is_revealed(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "same\nx\n", "same\ny\n");
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "ctrl-2");
    press(cx, handle, "ctrl-f");
    cx.update_window(handle, |_, window, cx| window.input("same", cx))
        .unwrap();
    wait_found(cx, handle, &view).await;
    press(cx, handle, "f3");
    let filter = view.read_with(cx, |v, _| v.model().unwrap().filter());
    assert_eq!(filter, TextFilter::All, "the row was hidden by Differences");
}

#[gpui_kit::test]
async fn find_runs_off_the_ui_thread_and_cancels(cx: &mut TestAppContext) {
    let (left, right) = long_pair();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "ctrl-f");
    view.update(cx, |v, cx| {
        v.set_find_text("line 1".into(), cx);
        assert_eq!(
            v.find_status().as_deref(),
            Some("Searching…"),
            "the search is not done inline"
        );
        v.set_find_text("no such text".into(), cx);
    });
    assert_eq!(
        wait_found(cx, handle, &view).await,
        "No matches",
        "only the newest search lands"
    );
}

#[gpui_kit::test]
async fn ctrl_g_jumps_to_a_line_on_the_chosen_side(cx: &mut TestAppContext) {
    let left: String = (0..60).map(|i| format!("left {i}\n")).collect();
    let right: String = (0..60).map(|i| format!("right {i}\n")).collect();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "ctrl-g");
    assert!(view.read_with(cx, |v, _| v.goto_open()));
    cx.update_window(handle, |_, window, cx| {
        window.input("abc", cx);
        window.press("enter", cx);
    })
    .unwrap();
    let error = view.read_with(cx, |v, _| v.goto_error().map(str::to_owned));
    assert_eq!(error.as_deref(), Some("Enter a line number from 1 to 60."));
    let ok = view.update(cx, |v, cx| {
        v.set_goto_side(false, cx);
        v.go_to_line(false, "45", cx)
    });
    assert!(ok);
    let (marked, expected) = view.read_with(cx, |v, _| {
        (v.marked_row(), v.model().unwrap().row_of_line(false, 44))
    });
    assert_eq!(marked, expected);
    assert!(expected.is_some());
    // The marked row's accent edge takes no width: its gutter lines up with the names bar's.
    let display = view.read_with(cx, |v, _| {
        v.model()
            .unwrap()
            .visible()
            .display_of(expected.unwrap())
            .unwrap()
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let gutter = window.find(("text-gutter", display)).bounds();
        let names_gutter = window.find("text-names-gutter").bounds();
        assert!(
            near(gutter.left(), names_gutter.left()),
            "{gutter:?} vs {names_gutter:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
async fn ctrl_g_past_the_end_after_deleting_lines_goes_to_the_last_line(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\nd\n", "a\nb\nc\nd\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    for _ in 0..3 {
        press(cx, handle, "shift-down");
    }
    press(cx, handle, "delete");
    assert_eq!(line_of(&view, cx, true, 0), "d");
    // Before the re-diff lands, "999" clamps to the one line the buffer has left.
    let ok = view.update(cx, |v, cx| v.go_to_line(true, "999", cx));
    assert!(ok);
    let (marked, last) = view.read_with(cx, |v, _| {
        (v.marked_row(), v.model().unwrap().row_of_line(true, 0))
    });
    assert_eq!(marked, last);
}

#[gpui_kit::test]
async fn moved_lines_are_marked_and_counted(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(
        cx,
        "a\nfn one()\nfn two()\n}\nb\nc\nd\ne\nf\n",
        "a\nb\nc\nd\ne\nf\nfn one()\nfn two()\n}\n",
    );
    wait_loaded(cx, handle, &view).await;
    let (status, partner) = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        let display = m
            .visible()
            .display_of(m.row_of_line(true, 1).unwrap())
            .unwrap();
        (
            duet_app::text::model::status_text(m),
            m.moved_partner(display, true),
        )
    });
    assert!(status.contains("2 moved lines"), "{status}");
    assert_eq!(partner, Some(6));
    view.update(cx, |v, cx| v.set_detect_moves(false, cx));
    wait_loaded(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.model().unwrap().moves().is_empty()));
}

#[gpui_kit::test]
async fn the_format_button_compares_pretty_printed_json(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "{\"a\":1}", "{\n  \"a\": 1\n}");
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("text-format", cx);
    })
    .unwrap();
    wait_loaded(cx, handle, &view).await;
    let (blocks, status) = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (m.block_count(), duet_app::text::model::status_text(m))
    });
    assert_eq!(blocks, 0);
    assert!(status.contains("formatted JSON"), "{status}");
}

#[gpui_kit::test]
async fn f9_swaps_the_sides_in_place(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\nb\nc\n", "a\nB\nc\nd\n");
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "f8");
    press(cx, handle, "f9");
    let (left, block, kind) = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (
            v.request().left.clone(),
            m.current_block(),
            m.row(3).unwrap().kind,
        )
    });
    assert_eq!(left, dir.path().join("b.txt"));
    assert_eq!(block, Some(0), "the current difference stays");
    assert_eq!(kind, RowKind::LeftOnly);
    assert!(
        !view.read_with(cx, |v, _| v.is_busy()),
        "a finished compare flips without re-diffing"
    );
}

#[gpui_kit::test]
async fn swapping_while_busy_restarts_with_the_sides_swapped(cx: &mut TestAppContext) {
    let (left, right) = long_pair();
    let (dir, handle, view) = open_text(cx, &left, &right);
    view.update(cx, |v, cx| v.swap_sides(cx));
    wait_loaded(cx, handle, &view).await;
    let shown = view.read_with(cx, |v, _| v.model().unwrap().left().path().to_path_buf());
    assert_eq!(shown, dir.path().join("b.txt"));
}

#[gpui_kit::test]
async fn swapping_during_a_reload_then_escape_keeps_labels_and_rows_together(
    cx: &mut TestAppContext,
) {
    let (dir, handle, view) = open_text(cx, "a\nb\n", "a\nc\nd\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.toggle_ignore(duet_app::text::job::IgnoreChoice::Case, cx);
        assert!(v.is_busy());
        v.swap_sides(cx);
    });
    press(cx, handle, "escape");
    let (label, shown) = view.read_with(cx, |v, _| {
        (
            v.request().left.clone(),
            v.model().unwrap().left().path().to_path_buf(),
        )
    });
    assert_eq!(label, dir.path().join("b.txt"));
    assert_eq!(
        shown, label,
        "the rows on the left are the file named on the left"
    );
}

#[gpui_kit::test]
async fn long_lines_show_a_cut_marker_and_scroll_with_the_bar(cx: &mut TestAppContext) {
    let long = format!("{}\nshort\n", "word ".repeat(80));
    let (_dir, handle, view) = open_text(cx, &long, &long.replace("short", "brief"));
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(
            window.try_find(("text-cut-left", 0usize)).is_some(),
            "row 0 is wider than the pane"
        );
        assert!(window.try_find(("text-cut-left", 1usize)).is_none());
        // The left text area runs from the number cell and inset to the marker's right edge;
        // at the code font's advance it holds exactly `view_columns`.
        let row = window.find(("text-row", 0usize)).bounds();
        let inset = px(NUMBER_CELL + TEXT_INSET);
        let area = window.find(("text-cut-left", 0usize)).bounds().right() - row.left() - inset;
        let fonts = duet_app::ui::fonts::Fonts::current(cx);
        let advance = duet_app::ui::fonts::code_advance(window, &fonts.code);
        let columns = view.read(cx).display_options().view_columns;
        assert_eq!((area / advance).floor() as usize, columns, "area {area:?}");
        let list = window.find("text-list").bounds();
        assert!(
            window.find(("text-cut-right", 0usize)).bounds().right()
                <= list.right() - px(SCROLLBAR_WIDTH),
            "the right marker stays clear of the vertical scrollbar"
        );
        let bar = window.find("text-hscroll").bounds();
        let y = bar.top() + px(5.);
        window.drag(
            Point::new(bar.left() + px(4.), y),
            Point::new(bar.right() - px(4.), y),
            cx,
        );
    })
    .unwrap();
    assert!(view.read_with(cx, |v, _| v.display_options().skip_columns) > 100);
}

/// Sends one pointer event to the window, then draws a frame, as the harness's own `drag` does.
fn pointer(window: &mut Window, event: gpui_kit::PlatformInput, cx: &mut App) {
    window.dispatch_event(event, cx);
    window.render_frame(cx);
}

fn move_to(position: Point<Pixels>, held: bool) -> gpui_kit::PlatformInput {
    gpui_kit::PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
        position,
        pressed_button: held.then_some(gpui_kit::MouseButton::Left),
        modifiers: Default::default(),
    })
}

fn button(position: Point<Pixels>, down: bool) -> gpui_kit::PlatformInput {
    let button = gpui_kit::MouseButton::Left;
    if down {
        gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
            button,
            position,
            modifiers: Default::default(),
            click_count: 1,
            first_mouse: false,
        })
    } else {
        gpui_kit::PlatformInput::MouseUp(gpui_kit::MouseUpEvent {
            button,
            position,
            modifiers: Default::default(),
            click_count: 1,
        })
    }
}

/// One minified line on each side, far longer than the 4,096 bytes a row shows at once; the
/// right one is 20,001 columns.
fn minified_pair() -> (String, String) {
    let line = "x".repeat(20_000);
    (format!("{line}\nsame\n"), format!("{line}y\nsame\n"))
}

#[gpui_kit::test]
async fn one_drag_along_the_bar_reaches_the_end_of_a_long_line(cx: &mut TestAppContext) {
    let (left, right) = minified_pair();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let bar = window.find("text-hscroll").bounds();
        let y = bar.top() + px(5.);
        // To the middle first: the bar maps the whole line, not what the rows have shown.
        window.drag(point(bar.left() + px(4.), y), point(bar.center().x, y), cx);
        let middle = view.read(cx).display_options().skip_columns;
        assert!(
            (9_000..=11_000).contains(&middle),
            "the middle of the bar is the middle of the line, got {middle}"
        );
        window.drag(point(bar.center().x, y), point(bar.right() - px(1.), y), cx);
        let shown = view.read(cx).display_options();
        assert_eq!(
            shown.skip_columns + shown.view_columns,
            20_001,
            "the end of the bar shows the end of the line"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
async fn an_edited_long_lines_width_catches_up_after_the_pause(cx: &mut TestAppContext) {
    let (left, right) = minified_pair();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    let widest = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        view.read_with(cx, |v, _| v.widest_for_test())
    };
    assert_eq!(
        widest(cx),
        20_001,
        "the whole line, counted off the UI thread"
    );
    view.update(cx, |v, cx| v.set_caret_for_test(false, 0, cx));
    cx.update_window(handle, |_, window, cx| {
        window.input("abcde", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.widest_for_test()),
        20_001,
        "the frame after a key keeps the stale count: it walks no whole line"
    );
    // The pause: the re-diff lands, then the edited line is counted again.
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        widest(cx),
        20_006,
        "the count catches up once typing pauses"
    );
}

#[gpui_kit::test]
async fn the_thumb_stays_on_the_bar_with_the_caret_at_the_line_end(cx: &mut TestAppContext) {
    let (left, right) = minified_pair();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    view.update(cx, |v, cx| v.set_caret_for_test(false, 0, cx));
    press(cx, handle, "end");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let (x, w, bar) = view
            .read(cx)
            .hscroll_thumb_for_test()
            .expect("the bar shows with the caret at the end");
        assert!(bar > 0.);
        assert!(
            x >= 0. && x + w <= bar + 0.5 && w >= 16.,
            "the thumb lies on the bar: x {x}, w {w}, bar {bar}"
        );
        assert!(
            (x + w - bar).abs() < 1.,
            "the caret is at the line's end, so the thumb is at the bar's end: x {x}, w {w}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
async fn the_horizontal_bar_follows_a_drag_that_leaves_it(cx: &mut TestAppContext) {
    let long = format!("{}\nshort\n", "word ".repeat(400));
    let (_dir, handle, view) = open_text(cx, &long, &long.replace("short", "brief"));
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let skip = |cx: &App| view.read(cx).display_options().skip_columns;
        let bar = window.find("text-hscroll").bounds();
        let start = point(bar.left() + px(4.), bar.top() + px(5.));
        pointer(window, move_to(start, false), cx);
        pointer(window, button(start, true), cx);
        // Well above the bar, as a hand drifts while it drags sideways.
        let above = bar.top() - px(60.);
        pointer(window, move_to(point(start.x, above), true), cx);
        let mut seen = vec![skip(cx)];
        for step in 1..=5 {
            let x = start.x + px(100. * step as f32);
            pointer(window, move_to(point(x, above), true), cx);
            seen.push(skip(cx));
        }
        assert!(
            seen.windows(2).all(|w| w[1] > w[0]),
            "every move right scrolls further: {seen:?}"
        );
        // Released off the bar: the drag ends there.
        let end = point(start.x + px(500.), above);
        pointer(window, button(end, false), cx);
        let released = skip(cx);
        pointer(window, move_to(point(start.x + px(800.), above), false), cx);
        assert_eq!(
            skip(cx),
            released,
            "after the release the pointer moves alone"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
async fn keys_move_the_caret_once_one_is_placed(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "one\ntwo\nthree\n", "one\ntwo\nthree!\n");
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "down");
    assert!(
        view.read_with(cx, |v, _| v.caret()).is_none(),
        "without a caret, Down scrolls"
    );
    view.update(cx, |v, cx| v.set_caret_for_test(false, 0, cx));
    press(cx, handle, "down");
    press(cx, handle, "end");
    let caret = view.read_with(cx, |v, _| v.caret().unwrap());
    assert_eq!((caret.left, caret.head), (false, "one\ntwo".len()));
    press(cx, handle, "shift-home");
    let caret = view.read_with(cx, |v, _| v.caret().unwrap());
    assert_eq!(caret.selection(), Some(4..7));
    press(cx, handle, "escape");
    assert!(
        view.read_with(cx, |v, _| v.caret()).is_none(),
        "Esc clears the caret first"
    );
}

#[gpui_kit::test]
async fn clicking_text_places_the_caret_on_that_line(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "alpha\nbeta\n", "alpha\ngamma\n");
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click_at(("text-row", 1usize), point(px(110.), px(10.)), cx);
    })
    .unwrap();
    let caret = view.read_with(cx, |v, _| v.caret()).expect("a caret");
    assert!(caret.left);
    assert!(
        (6..=10).contains(&caret.head),
        "on the left's line 1 (\"beta\"), at {}",
        caret.head
    );
}

#[gpui_kit::test]
async fn a_click_laid_out_before_an_edit_never_lands_inside_a_character(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "ก\n", "ก\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    let row = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.find(("text-row", 0usize)).bounds()
        })
        .unwrap();
    // An edit lands between that frame and the click, which still sees "ก" alone on the
    // line; past the old line's end is its byte 3, inside "ก" now.
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.replace_on(true, 0..0, "a", cx);
            let at = row.origin + point(px(300.), px(10.));
            v.click_text(0, true, at, false, window, cx)
        })
    })
    .unwrap();
    let (head, text) = view.read_with(cx, |v, _| {
        let text = v.model().unwrap().buffer(true).unwrap().text().to_owned();
        (v.caret().unwrap().head, text)
    });
    assert!(text.is_char_boundary(head), "caret at {head} in {text:?}");
    press(cx, handle, "shift-right");
    press(cx, handle, "ctrl-c");
}

#[gpui_kit::test]
async fn clicking_a_read_only_side_explains_why(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "{\"a\":1}", "{\"a\":2}");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_format_for_test(true, cx));
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click_at(("text-row", 0usize), point(px(110.), px(10.)), cx);
    })
    .unwrap();
    let (caret, notice) = view.read_with(cx, |v, _| (v.caret(), v.notice().map(str::to_owned)));
    assert!(caret.is_none());
    assert!(notice.is_some_and(|n| n.contains("formatted view is read-only")));
}

#[gpui_kit::test]
async fn dragging_over_text_selects_across_lines(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "alpha\nbeta\n", "alpha\ngamma\n");
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let (first, second) = (
            window.find(("text-row", 0usize)).bounds(),
            window.find(("text-row", 1usize)).bounds(),
        );
        window.drag(
            first.origin + point(px(NUMBER_CELL + TEXT_INSET + 4.), px(10.)),
            second.origin + point(px(110.), px(10.)),
            cx,
        );
    })
    .unwrap();
    let caret = view.read_with(cx, |v, _| v.caret()).expect("a caret");
    let selection = caret.selection().expect("a selection");
    assert!(caret.left);
    assert!(
        selection.start <= 1,
        "from the start of \"alpha\": {selection:?}"
    );
    assert!(selection.end >= 6, "into \"beta\": {selection:?}");
}

#[gpui_kit::test]
async fn the_caret_survives_a_reload_and_follows_a_swap(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "{\"a\":1}\n", "{\"a\":2}\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(false, 3, cx));
    let at =
        |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.caret().map(|c| (c.left, c.head)));
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        at(cx),
        Some((false, 3)),
        "a reload of the same files keeps it"
    );
    press(cx, handle, "f9");
    assert_eq!(
        at(cx),
        Some((true, 3)),
        "it moves with its text to the other side"
    );
    view.update(cx, |v, cx| v.set_format_for_test(true, cx));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(at(cx), None, "a side that turned read-only drops it");
}

fn line_of(view: &Entity<TextView>, cx: &mut TestAppContext, left: bool, line: usize) -> String {
    view.read_with(cx, |v, _| {
        v.model().unwrap().line_text(left, line).into_owned()
    })
}

#[gpui_kit::test]
async fn typing_edits_the_line_at_once_and_rediffs_after_a_pause(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "same\nold\n", "same\nold\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.set_caret_for_test(false, "same\nold".len(), cx)
    });
    cx.update_window(handle, |_, window, cx| window.input("er", cx))
        .unwrap();
    assert_eq!(line_of(&view, cx, false, 1), "older");
    let kind = view.read_with(cx, |v, _| v.model().unwrap().row(1).unwrap().kind);
    assert_eq!(
        kind,
        RowKind::Changed,
        "the row follows the edit before the re-diff"
    );
    wait_loaded(cx, handle, &view).await;
    let (blocks, dirty) = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (m.block_count(), m.is_dirty(false))
    });
    assert_eq!((blocks, dirty), (1, true));
    assert_eq!(
        line_of(&view, cx, false, 1),
        "older",
        "the re-diff kept the edit"
    );
}

#[gpui_kit::test]
async fn thai_typing_and_backspace_work_by_code_point(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "x\n", "x\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("ก่", cx))
        .unwrap();
    assert_eq!(line_of(&view, cx, true, 0), "xก่");
    press(cx, handle, "backspace");
    assert_eq!(
        line_of(&view, cx, true, 0),
        "xก",
        "only the tone mark is removed"
    );
    press(cx, handle, "left");
    press(cx, handle, "delete");
    assert_eq!(line_of(&view, cx, true, 0), "x");
}

#[gpui_kit::test]
async fn enter_uses_the_files_own_line_break(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "ab\r\ncd\r\n", "ab\r\ncd\r\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    press(cx, handle, "enter");
    let text = view.read_with(cx, |v, _| {
        v.model().unwrap().buffer(true).unwrap().text().to_owned()
    });
    assert_eq!(text, "a\r\nb\r\ncd\r\n");
}

#[gpui_kit::test]
async fn a_landed_rediff_replaces_the_provisional_rows(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nc\n");
    wait_loaded(cx, handle, &view).await;
    // Make the changed line equal again: the spliced rows still mark it changed.
    view.update(cx, |v, cx| v.set_caret_for_test(false, 2, cx));
    press(cx, handle, "shift-right");
    cx.update_window(handle, |_, window, cx| window.input("b", cx))
        .unwrap();
    assert_eq!(line_of(&view, cx, false, 1), "b");
    let blocks =
        |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.model().unwrap().block_count());
    assert_eq!(blocks(cx), 1, "provisional");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(blocks(cx), 0, "the re-diff's rows");
}

#[gpui_kit::test]
async fn typing_during_a_rediff_keeps_the_newest_text(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("1", cx))
        .unwrap();
    // A result computed from this state...
    let (generation, (l, r, versions)) = view.read_with(cx, |v, _| {
        (v.rediff_generation(), v.model().unwrap().snapshot())
    });
    let request = view.read_with(cx, |v, _| v.request().clone());
    let stale = duet_app::text::job::rediff(l, r, &request, &duet_core::cancel::CancelToken::new());
    // ...arrives after the next keystroke.
    cx.update_window(handle, |_, window, cx| window.input("2", cx))
        .unwrap();
    view.update(cx, |v, cx| {
        v.apply_rediff_for_test(generation, stale, versions, cx)
    });
    assert_eq!(
        line_of(&view, cx, true, 0),
        "a12",
        "the stale result is dropped"
    );
    wait_loaded(cx, handle, &view).await;
    assert_eq!(line_of(&view, cx, true, 0), "a12");
}

#[gpui_kit::test]
async fn composition_commits_and_clears_cleanly(cx: &mut TestAppContext) {
    use gpui_kit::EntityInputHandler as _;
    let (_dir, handle, view) = open_text(cx, "x\n", "x\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.replace_and_mark_text_in_range(None, "か", Some(1..1), window, cx);
            assert!(v.marked_text_range(window, cx).is_some());
            v.replace_text_in_range(None, "火", window, cx);
            assert!(v.marked_text_range(window, cx).is_none());
        })
    })
    .unwrap();
    assert_eq!(line_of(&view, cx, true, 0), "x火");
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.replace_and_mark_text_in_range(None, "ka", Some(2..2), window, cx)
        })
    })
    .unwrap();
    press(cx, handle, "escape");
    assert_eq!(
        line_of(&view, cx, true, 0),
        "x火",
        "Esc drops the composition"
    );
    assert!(
        view.read_with(cx, |v, _| v.caret()).is_some(),
        "and keeps the caret"
    );
}

#[gpui_kit::test]
async fn an_edit_outside_the_ime_commits_the_composition(cx: &mut TestAppContext) {
    use gpui_kit::EntityInputHandler as _;
    let (_dir, handle, view) = open_text(cx, "x\n", "x\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.replace_and_mark_text_in_range(None, "ka", None, window, cx)
        })
    })
    .unwrap();
    // An edit at the caret, with no caret move first.
    press(cx, handle, "backspace");
    assert_eq!(line_of(&view, cx, true, 0), "xk");
    press(cx, handle, "escape");
    assert_eq!(
        line_of(&view, cx, true, 0),
        "xk",
        "the backspace committed the composed text, so Esc has none to remove"
    );
}

#[gpui_kit::test]
async fn the_composition_follows_a_caret_placed_elsewhere(cx: &mut TestAppContext) {
    use gpui_kit::EntityInputHandler as _;
    let (_dir, handle, view) = open_text(cx, "xyz\n", "xyz\n");
    wait_loaded(cx, handle, &view).await;
    let ime = |cx: &mut TestAppContext, text: &str, mark: bool| {
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |v, cx| {
                if mark {
                    v.replace_and_mark_text_in_range(None, text, None, window, cx);
                } else {
                    v.replace_text_in_range(None, text, window, cx);
                }
                v.marked_text_range(window, cx)
            })
        })
        .unwrap()
    };
    let lines =
        |cx: &mut TestAppContext| (line_of(&view, cx, true, 0), line_of(&view, cx, false, 0));
    let marked = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |v, cx| v.marked_text_range(window, cx))
        })
        .unwrap()
    };
    // The IME still holds "ka" after the caret moves: it sends it again, then commits it.
    let resend_and_commit = |cx: &mut TestAppContext| {
        assert!(ime(cx, "ka", true).is_some());
        assert!(ime(cx, "か", false).is_none());
    };

    // A key move on the same side, to past the composition's end: the offset shifts left.
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    assert!(ime(cx, "ka", true).is_some());
    assert_eq!(lines(cx).0, "xkayz");
    press(cx, handle, "end");
    assert_eq!(marked(cx), None);
    assert_eq!(
        lines(cx),
        ("xyz".into(), "xyz".into()),
        "the old place is back to its own text"
    );
    resend_and_commit(cx);
    assert_eq!(
        lines(cx),
        ("xyzか".into(), "xyz".into()),
        "once, at the line end the caret went to"
    );

    // Composing on the left, a click past the end of the right's "xyz": that offset is past
    // the composition's end too, but on the other side, so it stays.
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    assert!(ime(cx, "ka", true).is_some());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let row = window.find(("text-row", 0usize)).bounds();
        window.click_at(
            ("text-row", 0usize),
            point(row.size.width - px(40.), px(10.)),
            cx,
        );
    })
    .unwrap();
    assert_eq!(marked(cx), None);
    resend_and_commit(cx);
    assert_eq!(lines(cx), ("xyzか".into(), "xyzか".into()));

    // A caret placed inside the composition lands at its start.
    view.update(cx, |v, cx| v.set_caret_for_test(false, 1, cx));
    assert!(ime(cx, "ka", true).is_some());
    view.update(cx, |v, cx| v.set_caret_for_test(false, 2, cx));
    assert_eq!(lines(cx).1, "xyzか");
    resend_and_commit(cx);
    assert_eq!(lines(cx), ("xyzか".into(), "xかyzか".into()));
}

#[gpui_kit::test]
async fn an_empty_ime_update_changes_nothing(cx: &mut TestAppContext) {
    use gpui_kit::EntityInputHandler as _;
    let (_dir, handle, view) = open_text(cx, "x\n", "x\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            // What a Windows IME sends with no composition, and after each commit.
            v.replace_text_in_range(None, "", window, cx);
            v.replace_and_mark_text_in_range(None, "", None, window, cx);
        })
    })
    .unwrap();
    let (dirty, busy) = view.read_with(cx, |v, _| (v.model().unwrap().is_dirty(true), v.is_busy()));
    assert_eq!(
        (dirty, busy),
        (false, false),
        "not modified, and no re-diff"
    );
}

#[gpui_kit::test]
async fn a_reload_drops_a_pending_rediff_and_the_composition(cx: &mut TestAppContext) {
    use gpui_kit::EntityInputHandler as _;
    let (_dir, handle, view) = open_text(cx, "abc\n", "abc\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.replace_and_mark_text_in_range(None, "k", None, window, cx)
        })
    })
    .unwrap();
    let (generation, (l, r, versions)) = view.read_with(cx, |v, _| {
        (v.rediff_generation(), v.model().unwrap().snapshot())
    });
    let request = view.read_with(cx, |v, _| v.request().clone());
    let stale = duet_app::text::job::rediff(l, r, &request, &duet_core::cancel::CancelToken::new());
    view.update(cx, |v, cx| v.reload(cx));
    // Composing goes on while the reload runs, into the buffer the reload replaces.
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.replace_and_mark_text_in_range(None, "z", None, window, cx)
        })
    })
    .unwrap();
    // Lands the reload without reaching the 150 ms re-diff timer.
    cx.run_until_parked();
    assert!(
        !view.read_with(cx, |v, _| v.is_busy()),
        "the reload landed and no re-diff of the old text is waiting"
    );
    view.update(cx, |v, cx| {
        v.apply_rediff_for_test(generation, stale, versions, cx)
    });
    assert_eq!(
        line_of(&view, cx, true, 0),
        "abc",
        "the reloaded text is untouched"
    );
    press(cx, handle, "escape");
    assert_eq!(
        line_of(&view, cx, true, 0),
        "abc",
        "no composition from the old text is left for Esc to remove"
    );
}

#[gpui_kit::test]
async fn a_swap_reschedules_a_pending_rediff(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(false, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("2", cx))
        .unwrap();
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("1", cx))
        .unwrap();
    let (generation, (l, r, versions)) = view.read_with(cx, |v, _| {
        (v.rediff_generation(), v.model().unwrap().snapshot())
    });
    assert_eq!(
        versions[0], versions[1],
        "both sides at one version: only the generation tells a pre-swap result apart"
    );
    let request = view.read_with(cx, |v, _| v.request().clone());
    let stale = duet_app::text::job::rediff(l, r, &request, &duet_core::cancel::CancelToken::new());
    press(cx, handle, "f9");
    view.update(cx, |v, cx| {
        v.apply_rediff_for_test(generation, stale, versions, cx)
    });
    let shown = view.read_with(cx, |v, _| v.model().unwrap().left().path().to_path_buf());
    assert_eq!(
        shown,
        dir.path().join("b.txt"),
        "the pre-swap result is dropped"
    );
    wait_loaded(cx, handle, &view).await;
    assert_eq!(line_of(&view, cx, true, 0), "b2");
    assert_eq!(line_of(&view, cx, false, 0), "a1");
}

#[gpui_kit::test]
async fn tab_reaches_the_rows_and_moves_on_from_them_without_a_caret(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "ab\n", "ab\n");
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let rows = view.read(cx).focus_handle(cx);
        window.press("tab", cx);
        assert!(
            !rows.is_focused(window),
            "no caret: Tab moves on from the rows"
        );
        assert_eq!(window.find("text-swap").focused(), Some(true));
        for _ in 0..60 {
            if rows.is_focused(window) {
                break;
            }
            window.press("tab", cx);
        }
        assert!(
            rows.is_focused(window),
            "Tab through the toolbar comes back to the rows"
        );
        window.press("tab", cx);
        window.press("escape", cx);
        assert!(
            rows.is_focused(window),
            "Esc with no bar or job focuses the rows"
        );
    })
    .unwrap();
    let text = view.read_with(cx, |v, _| {
        v.model().unwrap().buffer(true).map(|b| b.text().to_owned())
    });
    assert_eq!(text, None, "nothing was typed");
}

#[gpui_kit::test]
async fn keys_a_find_field_passes_up_never_edit_the_text(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "ab\n", "ab\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    press(cx, handle, "ctrl-f");
    // A one-line field lets Enter, Tab and Backspace at its start bubble up to the view.
    for key in ["enter", "tab", "backspace"] {
        press(cx, handle, key);
    }
    let text = view.read_with(cx, |v, _| {
        v.model().unwrap().buffer(true).unwrap().text().to_owned()
    });
    assert_eq!(text, "ab\n");
}

#[gpui_kit::test]
async fn undo_takes_back_typing_in_one_step_and_redo_restores_it(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "one\n", "one\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(false, 3, cx));
    cx.update_window(handle, |_, window, cx| window.input("abc", cx))
        .unwrap();
    press(cx, handle, "ctrl-z");
    assert_eq!(line_of(&view, cx, false, 0), "one");
    press(cx, handle, "ctrl-y");
    assert_eq!(line_of(&view, cx, false, 0), "oneabc");
    press(cx, handle, "left");
    cx.update_window(handle, |_, window, cx| window.input("X", cx))
        .unwrap();
    press(cx, handle, "ctrl-z");
    assert_eq!(
        line_of(&view, cx, false, 0),
        "oneabc",
        "the caret move started a new step"
    );
}

#[gpui_kit::test]
async fn cut_copy_and_paste_use_the_clipboard_and_the_files_line_breaks(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "ab\r\ncd\r\n", "ab\r\ncd\r\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    press(cx, handle, "shift-end");
    press(cx, handle, "ctrl-c");
    assert_eq!(
        cx.read_from_clipboard().and_then(|i| i.text()),
        Some("ab".into())
    );
    press(cx, handle, "ctrl-x");
    assert_eq!(line_of(&view, cx, true, 0), "");
    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("x\ny".into()));
    press(cx, handle, "ctrl-v");
    let text = view.read_with(cx, |v, _| {
        v.model().unwrap().buffer(true).unwrap().text().to_owned()
    });
    assert_eq!(text, "x\r\ny\r\ncd\r\n");
    press(cx, handle, "ctrl-a");
    let caret = view.read_with(cx, |v, _| v.caret().unwrap());
    assert_eq!(caret.selection(), Some(0..text.len()));
}

#[gpui_kit::test]
async fn undo_after_f9_edits_the_side_the_text_moved_to(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("L", cx))
        .unwrap();
    press(cx, handle, "f9");
    assert_eq!(
        line_of(&view, cx, false, 0),
        "aL",
        "the edited text moved right"
    );
    press(cx, handle, "ctrl-z");
    assert_eq!(line_of(&view, cx, false, 0), "a");
    assert_eq!(
        view.read_with(cx, |v, _| v.caret().map(|c| c.left)),
        Some(false)
    );
}

#[gpui_kit::test]
async fn undo_after_the_file_changed_on_disk_clears_the_history_safely(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "abcdef\n", "abcdef\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 6, cx));
    cx.update_window(handle, |_, window, cx| window.input("XYZ", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    wait_clean(cx, &view).await;
    // The file shrinks on disk; the reload brings in text the history no longer fits, so the
    // reload itself drops the history.
    std::fs::write(dir.path().join("a.txt"), "q\n").unwrap();
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    press(cx, handle, "ctrl-z");
    assert_eq!(line_of(&view, cx, true, 0), "q", "the text is untouched");
    let notice = view.read_with(cx, |v, _| v.notice().map(str::to_owned));
    assert_eq!(notice, None, "nothing was left to undo");
    press(cx, handle, "ctrl-y");
    assert_eq!(line_of(&view, cx, true, 0), "q", "redo has nothing left");
}

fn touch(path: &Path, secs: u64) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

/// Backspaces left byte `at` of a pair of `text`, saves, and waits for the save.
async fn backspace_and_save(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
    at: usize,
) {
    view.update(cx, |v, cx| v.set_caret_for_test(true, at, cx));
    press(cx, handle, "backspace");
    press(cx, handle, "ctrl-s");
    wait_clean(cx, view).await;
}

#[gpui_kit::test]
async fn undo_after_a_clean_f5_never_edits_a_file_it_never_saw(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "abc\n", "abc\n");
    wait_loaded(cx, handle, &view).await;
    backspace_and_save(cx, handle, &view, 2).await;
    assert_eq!(line_of(&view, cx, true, 0), "ac");
    // Someone else rewrites the file; F5 has nothing unsaved to ask about.
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "zzzzzz\n").unwrap();
    touch(&path, 1_800_000_000);
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(line_of(&view, cx, true, 0), "zzzzzz");
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    press(cx, handle, "ctrl-z");
    assert_eq!(line_of(&view, cx, true, 0), "zzzzzz");
}

#[gpui_kit::test]
async fn undo_after_reading_the_file_in_another_encoding_changes_nothing(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "éb\n", "éb\n");
    wait_loaded(cx, handle, &view).await;
    backspace_and_save(cx, handle, &view, "éb".len()).await;
    // The same bytes read as windows-1252 are other text at other offsets.
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.set_encoding(true, Some(encoding_rs::WINDOWS_1252), window, cx)
        })
    })
    .unwrap();
    wait_loaded(cx, handle, &view).await;
    let read = line_of(&view, cx, true, 0);
    assert_eq!(read, "Ã©");
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    press(cx, handle, "ctrl-z");
    assert_eq!(line_of(&view, cx, true, 0), read);
}

#[gpui_kit::test]
async fn undo_survives_a_reload_of_unchanged_files(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "abc\n", "abc\n");
    wait_loaded(cx, handle, &view).await;
    backspace_and_save(cx, handle, &view, 2).await;
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    press(cx, handle, "ctrl-z");
    assert_eq!(line_of(&view, cx, true, 0), "abc");
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

async fn wait_clean(cx: &mut TestAppContext, view: &Entity<TextView>) {
    for _ in 0..2000 {
        if !view.read_with(cx, |v, _| v.is_dirty()) {
            return;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    panic!("never saved");
}

#[gpui_kit::test]
async fn ctrl_s_saves_keeping_the_bom_and_line_breaks(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "\u{feff}a\r\nb\r\n", "a\r\nb\r\n");
    wait_loaded(cx, handle, &view).await;
    // The decoded text holds no BOM, so offset 0 is before "a".
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    cx.update_window(handle, |_, window, cx| window.input("ก", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    wait_clean(cx, &view).await;
    let bytes = std::fs::read(dir.path().join("a.txt")).unwrap();
    assert_eq!(bytes, "\u{feff}กa\r\nb\r\n".as_bytes());
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    assert!(
        cx.update_window(handle, |_, w, _| w.try_find("text-dirty-left").is_none())
            .unwrap()
    );
}

#[gpui_kit::test]
async fn a_character_the_encoding_cannot_hold_blocks_the_save(cx: &mut TestAppContext) {
    let thai_874: &[u8] = &[0xA1, 0x0A]; // "ก\n" in windows-874
    let (dir, handle, view) = open_text_bytes(cx, thai_874, thai_874);
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.set_encoding(true, Some(encoding_rs::WINDOWS_874), window, cx)
        })
    })
    .unwrap();
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, "ก".len(), cx));
    cx.update_window(handle, |_, window, cx| window.input("😀", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    wait_for_prompt(cx).await;
    let (message, _) = cx.pending_prompt().unwrap();
    assert_eq!(message, "a.txt can't be saved as windows-874.");
    cx.simulate_prompt_answer("Cancel");
    for _ in 0..200 {
        cx.executor().timer(Duration::from_millis(10)).await;
        if view.read_with(cx, |v, _| v.notice().is_some()) {
            break;
        }
    }
    let notice = view
        .read_with(cx, |v, _| v.notice().map(str::to_owned))
        .unwrap_or_default();
    assert_eq!(
        notice,
        "Not saved: a.txt has a character windows-874 can't hold."
    );
    assert_eq!(
        std::fs::read(dir.path().join("a.txt")).unwrap(),
        thai_874,
        "the file is untouched"
    );
    assert!(view.read_with(cx, |v, _| v.is_dirty()));
}

#[gpui_kit::test]
async fn a_file_changed_on_disk_asks_before_overwriting(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(false, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("!", cx))
        .unwrap();
    let path = dir.path().join("b.txt");
    std::fs::write(&path, "someone else\n").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_800_000_000))
        .unwrap();
    for (answer, expected) in [("Cancel", "someone else\n"), ("Overwrite", "a!\n")] {
        press(cx, handle, "ctrl-s");
        wait_for_prompt(cx).await;
        let (message, _) = cx.pending_prompt().unwrap();
        assert!(message.contains("changed on disk"), "{message}");
        cx.simulate_prompt_answer(answer);
        cx.run_until_parked();
        for _ in 0..200 {
            if std::fs::read_to_string(&path).unwrap() == expected {
                break;
            }
            cx.executor().timer(Duration::from_millis(10)).await;
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
    }
    wait_clean(cx, &view).await;
}

#[gpui_kit::test]
async fn typing_while_a_save_runs_leaves_the_side_dirty_without_a_false_conflict(
    cx: &mut TestAppContext,
) {
    let (dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    // Start the save and type again before it lands.
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.save_side(true, window, cx).detach())
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| window.input("y", cx))
        .unwrap();
    let path = dir.path().join("a.txt");
    for _ in 0..200 {
        if std::fs::read_to_string(&path).unwrap() == "ax\n" {
            break;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "ax\n");
    assert!(
        view.read_with(cx, |v, _| v.is_dirty()),
        "the newer text is unsaved"
    );
    press(cx, handle, "ctrl-s");
    wait_clean(cx, &view).await;
    assert!(!cx.has_pending_prompt(), "our own write is not a conflict");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "axy\n");
}

/// Waits until the view is clean or a prompt asks something.
async fn wait_clean_or_prompt(cx: &mut TestAppContext, view: &Entity<TextView>) {
    for _ in 0..300 {
        if cx.has_pending_prompt() || !view.read_with(cx, |v, _| v.is_dirty()) {
            return;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    panic!("neither saved nor asked");
}

#[gpui_kit::test]
async fn a_rediff_that_lands_after_a_save_keeps_the_saved_stamp(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    // A re-diff of this text, computed now and delivered after the save lands.
    let (generation, (l, r, versions)) = view.read_with(cx, |v, _| {
        (v.rediff_generation(), v.model().unwrap().snapshot())
    });
    let request = view.read_with(cx, |v, _| v.request().clone());
    let late = duet_app::text::job::rediff(l, r, &request, &duet_core::cancel::CancelToken::new());
    press(cx, handle, "ctrl-s");
    wait_clean(cx, &view).await;
    assert!(
        view.read_with(cx, |v, _| v.is_busy()),
        "the re-diff is still pending"
    );
    view.update(cx, |v, cx| {
        v.apply_rediff_for_test(generation, late, versions, cx)
    });
    cx.update_window(handle, |_, window, cx| window.input("y", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    wait_clean_or_prompt(cx, &view).await;
    assert!(
        !cx.has_pending_prompt(),
        "our own write is not a change on disk: {:?}",
        cx.pending_prompt()
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "axy\n"
    );
}

#[gpui_kit::test]
async fn a_second_ctrl_s_while_the_save_runs_is_skipped(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    press(cx, handle, "ctrl-s");
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("Still saving a.txt."),
        "the skipped save says why"
    );
    wait_clean_or_prompt(cx, &view).await;
    cx.run_until_parked();
    assert!(
        !cx.has_pending_prompt(),
        "the first save's write is not a conflict for a second one: {:?}",
        cx.pending_prompt()
    );
    assert!(!view.read_with(cx, |v, _| v.is_dirty()));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "ax\n"
    );
}

#[gpui_kit::test]
async fn a_save_that_lands_after_f9_cleans_the_side_its_file_moved_to(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    press(cx, handle, "f9");
    let path = dir.path().join("a.txt");
    for _ in 0..200 {
        if std::fs::read_to_string(&path).unwrap() == "ax\n" {
            break;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    cx.run_until_parked();
    let dirty = view.read_with(cx, |v, _| {
        let model = v.model().unwrap();
        (model.is_dirty(true), model.is_dirty(false))
    });
    assert_eq!(dirty, (false, false), "a.txt, now on the right, is saved");
    // The next save checks the disk against that write.
    view.update(cx, |v, cx| v.set_caret_for_test(false, 2, cx));
    cx.update_window(handle, |_, window, cx| window.input("y", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    wait_clean_or_prompt(cx, &view).await;
    assert!(
        !cx.has_pending_prompt(),
        "our own write is not a change on disk: {:?}",
        cx.pending_prompt()
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "axy\n");
}

#[gpui_kit::test]
async fn saving_everything_writes_a_file_open_on_both_sides_once(cx: &mut TestAppContext) {
    let name = Path::new("a.txt");
    let (dir, handle, view) = open_files(cx, (name, b"a\n"), (name, b"a\n"));
    wait_loaded(cx, handle, &view).await;
    // The same edit on both sides; different edits save nothing (see ui_workspace.rs).
    for left in [true, false] {
        view.update(cx, |v, cx| v.set_caret_for_test(left, 1, cx));
        cx.update_window(handle, |_, window, cx| window.input("x", cx))
            .unwrap();
    }
    let saved = cx
        .update_window(handle, |_, window, cx| {
            view.update(cx, |v, cx| v.save_all(window, cx))
        })
        .unwrap();
    assert!(
        saved.await,
        "its own save of the file is no reason to fail: {:?}",
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join(name)).unwrap(),
        "ax\n"
    );
    let (left, right) = view.read_with(cx, |v, _| {
        let model = v.model().unwrap();
        (model.is_dirty(true), model.is_dirty(false))
    });
    assert_eq!(
        (left, right),
        (false, true),
        "written once, from the left side"
    );
}

#[gpui_kit::test]
async fn ctrl_s_saves_without_a_caret(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    press(cx, handle, "escape");
    assert!(view.read_with(cx, |v, _| v.caret().is_none()));
    press(cx, handle, "ctrl-s");
    wait_clean(cx, &view).await;
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "ax\n"
    );
}

#[gpui_kit::test]
async fn ctrl_s_saves_while_the_find_field_has_focus(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    press(cx, handle, "escape");
    press(cx, handle, "ctrl-f");
    assert!(view.read_with(cx, |v, _| v.find_open()));
    press(cx, handle, "ctrl-s");
    wait_clean(cx, &view).await;
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "ax\n"
    );
    assert!(
        view.read_with(cx, |v, _| v.find_open()),
        "the find bar stays open"
    );
}

#[gpui_kit::test]
async fn reloading_with_unsaved_edits_asks_first(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    press(cx, handle, "f5");
    wait_for_prompt(cx).await;
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(line_of(&view, cx, true, 0), "ax", "Cancel keeps the edit");
    press(cx, handle, "f5");
    wait_for_prompt(cx).await;
    cx.simulate_prompt_answer("Discard");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(line_of(&view, cx, true, 0), "a");
}

/// Runs ready tasks one at a time until `done` holds: unlike `run_until_parked`, a reload
/// started on the way is still running when this returns.
fn tick_until(cx: &mut TestAppContext, mut done: impl FnMut(&mut TestAppContext) -> bool) {
    for _ in 0..1_000 {
        if done(cx) {
            return;
        }
        cx.executor().tick();
    }
    panic!("the condition never held");
}

/// Adds a line "b" after the left's "a" and answers Discard to the F5 prompt; returns once
/// the answer is applied and the reload it starts is still running.
async fn add_a_line_then_discard_on_f5(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
    wait_for_the_rediff: bool,
) {
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    press(cx, handle, "enter");
    cx.update_window(handle, |_, window, cx| window.input("b", cx))
        .unwrap();
    assert_eq!(line_of(view, cx, true, 1), "b");
    if wait_for_the_rediff {
        wait_loaded(cx, handle, view).await;
    }
    press(cx, handle, "f5");
    wait_for_prompt(cx).await;
    cx.simulate_prompt_answer("Discard");
    tick_until(cx, |cx| !view.read_with(cx, |v, _| v.is_dirty()));
    assert!(view.read_with(cx, |v, _| v.is_busy()), "the reload runs");
}

#[gpui_kit::test]
async fn discarding_before_the_rediff_never_draws_rows_for_lines_that_are_gone(
    cx: &mut TestAppContext,
) {
    let (_dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    add_a_line_then_discard_on_f5(cx, handle, &view, false).await;
    // A frame while the reload runs: the rows must not point at the discarded line.
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    wait_loaded(cx, handle, &view).await;
    let lines = view.read_with(cx, |v, _| v.model().unwrap().line_count(true));
    assert_eq!((line_of(&view, cx, true, 0), lines), ("a".into(), 1));
}

#[gpui_kit::test]
async fn esc_after_discard_never_shows_the_discarded_text_as_the_file(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    // The re-diff lands first, so the documents behind the rows hold the edited text.
    add_a_line_then_discard_on_f5(cx, handle, &view, true).await;
    press(cx, handle, "escape");
    let (shown, message) = view.read_with(cx, |v, _| {
        (
            v.model().and_then(|m| m.left().text().map(str::to_owned)),
            v.message().map(str::to_owned),
        )
    });
    assert_eq!(shown, None, "nothing claims to be the file");
    assert_eq!(message.as_deref(), Some("Compare cancelled."));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "a\n"
    );
}

#[gpui_kit::test]
async fn keys_typed_while_a_reload_runs_are_refused_with_a_notice(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    press(cx, handle, "f5");
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    cx.update_window(handle, |_, window, cx| window.input("z", cx))
        .unwrap();
    // The reload would replace the buffer, so the key must not seem to land.
    let (typed, status) = view.read_with(cx, |v, _| {
        (
            v.model().unwrap().line_text(true, 0).into_owned(),
            v.status_line(),
        )
    });
    assert_eq!(typed, "a");
    assert_eq!(status, "Wait for the compare to finish.");
    wait_loaded(cx, handle, &view).await;
    let (line, dirty, notice) = view.read_with(cx, |v, _| {
        (
            v.model().unwrap().line_text(true, 0).into_owned(),
            v.is_dirty(),
            v.notice().map(str::to_owned),
        )
    });
    assert_eq!((line.as_str(), dirty), ("a", false));
    assert_eq!(notice, None, "the notice goes once the compare lands");
}

#[gpui_kit::test]
async fn esc_after_a_clean_f5_redoes_the_rediff_the_reload_cancelled(cx: &mut TestAppContext) {
    // The edit makes the lines equal: only the re-diff can tell, the provisional rows can't.
    let (_dir, handle, view) = open_text(cx, "a\n", "ab\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("b", cx))
        .unwrap();
    let blocks =
        |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.model().unwrap().block_count());
    assert_eq!(blocks(cx), 1, "the provisional rows");
    press(cx, handle, "ctrl-s");
    wait_clean(cx, &view).await;
    assert!(
        view.read_with(cx, |v, _| v.is_busy()),
        "the re-diff is still pending"
    );
    // Nothing is unsaved, so F5 reloads at once and drops the pending re-diff; Esc keeps the
    // edited model.
    press(cx, handle, "f5");
    press(cx, handle, "escape");
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("Reload cancelled; showing the previous result.")
    );
    wait_loaded(cx, handle, &view).await;
    assert_eq!(blocks(cx), 0, "the kept model's rows are re-diffed");
}

#[gpui_kit::test]
async fn esc_that_cancels_a_compare_with_nothing_loaded_clears_the_wait_notice(
    cx: &mut TestAppContext,
) {
    // The left side is over 64 MiB, so F5 releases the model while the compare runs; the caret
    // on the small right side stays.
    let big = format!("{}\n", "x".repeat(1023)).repeat(66_560);
    let (_dir, handle, view) = open_text(cx, &big, "a\n");
    drop(big);
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(false, 1, cx));
    press(cx, handle, "f5");
    assert!(view.read_with(cx, |v, _| v.model().is_none() && v.is_busy()));
    press(cx, handle, "ctrl-z");
    let notice = |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.notice().map(str::to_owned));
    assert_eq!(
        notice(cx).as_deref(),
        Some("Wait for the compare to finish.")
    );
    press(cx, handle, "escape");
    let (message, busy) = view.read_with(cx, |v, _| (v.message().map(str::to_owned), v.is_busy()));
    assert_eq!(
        (message.as_deref(), busy),
        (Some("Compare cancelled."), false)
    );
    assert_eq!(
        notice(cx),
        None,
        "nothing runs, so there is nothing to wait for"
    );
}

#[gpui_kit::test]
async fn undo_while_a_reload_runs_waits_for_it(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    wait_clean(cx, &view).await;
    press(cx, handle, "f5");
    press(cx, handle, "ctrl-z");
    assert_eq!(line_of(&view, cx, true, 0), "ax", "refused while comparing");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 2, cx));
    press(cx, handle, "ctrl-z");
    assert_eq!(
        line_of(&view, cx, true, 0),
        "a",
        "the step is still there once the unchanged file is back"
    );
}

#[gpui_kit::test]
async fn the_gutter_arrows_copy_a_block_as_one_undo_step(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\n", "a\nX\nY\nc\n");
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("copy-right", 0usize), cx);
    })
    .unwrap();
    let right = view.read_with(cx, |v, _| {
        v.model().unwrap().buffer(false).unwrap().text().to_owned()
    });
    assert_eq!(right, "a\nb\nc\n");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().block_count()),
        0
    );
    press(cx, handle, "ctrl-z");
    assert_eq!(
        line_of(&view, cx, false, 2),
        "Y",
        "one undo brings the whole block back"
    );
}

#[gpui_kit::test]
async fn alt_arrows_copy_the_current_block_and_explain_when_there_is_none(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nc\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.clear_current_block_for_test(cx));
    press(cx, handle, "alt-left");
    assert!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .is_some_and(|n| n.contains("F7/F8"))
    );
    press(cx, handle, "f8");
    press(cx, handle, "alt-left");
    assert_eq!(line_of(&view, cx, true, 1), "c", "right to left");
}

async fn wait_syntax(cx: &mut TestAppContext, view: &Entity<TextView>, left: bool) {
    for _ in 0..2000 {
        if view.read_with(cx, |v, _| v.syntax_state(left)) == SyntaxState::Ready {
            return;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    panic!("syntax never became ready");
}

#[gpui_kit::test]
async fn rust_files_get_syntax_colors_that_follow_edits(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_named(cx, "a.rs", "fn main() {}\n", "fn main() { }\n");
    wait_loaded(cx, handle, &view).await;
    wait_syntax(cx, &view, true).await;
    let styles = view.read_with(cx, |v, _| v.line_styles_for_test(true, 0));
    assert!(
        styles
            .iter()
            .any(|(r, s)| r.start == 0 && s.color.is_some()),
        "`fn` is colored: {styles:?}"
    );
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    cx.update_window(handle, |_, window, cx| window.input("// ", cx))
        .unwrap();
    wait_syntax(cx, &view, true).await;
    let styles = view.read_with(cx, |v, _| v.line_styles_for_test(true, 0));
    assert_eq!(
        styles.first().map(|(r, _)| r.clone()),
        Some(0.."// fn main() {}".len())
    );
    view.update(cx, |v, cx| v.set_syntax(false, cx));
    assert!(
        view.read_with(cx, |v, _| v.line_styles_for_test(true, 0))
            .is_empty()
    );
}

#[gpui_kit::test]
async fn large_or_unknown_files_stay_plain(cx: &mut TestAppContext) {
    let big = "let x = 1;\n".repeat(1_000_000); // 11 MB
    let (_dir, handle, view) = open_named(cx, "big.rs", &big, &big);
    wait_loaded(cx, handle, &view).await;
    assert!(matches!(
        view.read_with(cx, |v, _| v.syntax_state(true)),
        SyntaxState::Off(_)
    ));
    let (_dir2, handle2, view2) = open_named(cx, "notes.txt", "a\n", "b\n");
    wait_loaded(cx, handle2, &view2).await;
    assert_eq!(
        view2.read_with(cx, |v, _| v.syntax_state(true)),
        SyntaxState::Off("plain text")
    );
}

#[gpui_kit::test]
async fn a_parse_that_lands_after_f9_colors_the_side_it_parsed(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_named(cx, "a.rs", "// note\n", "fn main() {}\n");
    wait_loaded(cx, handle, &view).await;
    // Both sides start parsing again, and swap before either parse lands.
    view.update(cx, |v, cx| {
        v.set_syntax(true, cx);
        v.swap_sides(cx);
    });
    wait_syntax(cx, &view, true).await;
    wait_syntax(cx, &view, false).await;
    let left = view.read_with(cx, |v, _| v.line_styles_for_test(true, 0));
    let right = view.read_with(cx, |v, _| v.line_styles_for_test(false, 0));
    assert_eq!(
        left.first().map(|(r, _)| r.clone()),
        Some(0.."fn".len()),
        "the left side now holds `fn main() {{}}`: {left:?}"
    );
    assert_eq!(
        right.first().map(|(r, _)| r.clone()),
        Some(0.."// note".len()),
        "the right side now holds the comment: {right:?}"
    );
}

#[gpui_kit::test]
async fn after_an_overrun_edits_keep_shifted_colors_until_the_parse_lands(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_named(cx, "a.rs", "fn main() {}\n", "fn main() {}\n");
    wait_loaded(cx, handle, &view).await;
    wait_syntax(cx, &view, true).await;
    // A zero budget: every re-parse on the UI thread overruns.
    view.update(cx, |v, cx| {
        v.set_syntax_budget_for_test(Duration::ZERO);
        v.set_caret_for_test(true, 0, cx);
    });
    cx.update_window(handle, |_, window, cx| window.input(" ", cx))
        .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.syntax_state(true)),
        SyntaxState::Parsing
    );
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    cx.update_window(handle, |_, window, cx| window.input("//", cx))
        .unwrap();
    let (state, styles) = view.read_with(cx, |v, _| {
        (v.syntax_state(true), v.line_styles_for_test(true, 0))
    });
    assert_eq!(state, SyntaxState::Parsing);
    assert!(
        styles
            .iter()
            .any(|(r, s)| *r == (3.."// fn".len()) && s.color.is_some()),
        "the old colors stay on screen, moved by the edits: {styles:?}"
    );
    wait_syntax(cx, &view, true).await;
    let styles = view.read_with(cx, |v, _| v.line_styles_for_test(true, 0));
    assert_eq!(
        styles.first().map(|(r, _)| r.clone()),
        Some(0.."// fn main() {}".len()),
        "the landed parse sees the comment: {styles:?}"
    );
}

/// Freeing a large file's tree and rope takes tens of milliseconds, so no syntax colors are
/// dropped on the UI thread: each one this view lets go of is still alive when the call that let
/// go of it returns, and is gone once the background executor has run.
#[gpui_kit::test]
async fn syntax_colors_are_dropped_off_the_ui_thread(cx: &mut TestAppContext) {
    let text = "fn main() {}\n";
    let (_dir, handle, view) = open_named(cx, "a.rs", text, text);
    wait_loaded(cx, handle, &view).await;
    wait_syntax(cx, &view, true).await;
    let shown = |cx: &mut TestAppContext| {
        view.read_with(cx, |v, _| v.syntax_witness_for_test(true))
            .expect("the left side has colors")
    };

    // A parse that lands for the current text replaces the colors on screen.
    let replaced = shown(cx);
    view.update(cx, |v, cx| {
        v.land_syntax_for_test(true, true, 0, SyntaxSide::parse_now("rust", text), cx);
        assert!(
            replaced.upgrade().is_some(),
            "the replaced colors are dropped inside land_syntax"
        );
    });
    assert_eq!(
        view.read_with(cx, |v, _| v.syntax_state(true)),
        SyntaxState::Ready
    );
    cx.run_until_parked();
    assert!(
        replaced.upgrade().is_none(),
        "the replaced colors are freed"
    );

    // A parse of text that was edited since, and one whose job was cancelled, are let go.
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    cx.update_window(handle, |_, window, cx| window.input(" ", cx))
        .unwrap();
    for newest in [true, false] {
        let late = SyntaxSide::parse_now("rust", text);
        let witness = late.witness();
        view.update(cx, |v, cx| {
            v.land_syntax_for_test(true, newest, 0, late, cx);
            assert!(
                witness.upgrade().is_some(),
                "a stale parse (newest: {newest}) is dropped inside land_syntax"
            );
        });
        cx.run_until_parked();
        assert!(witness.upgrade().is_none(), "the stale parse is freed");
    }

    // Switching colors off lets go of them.
    wait_syntax(cx, &view, true).await;
    let shown = shown(cx);
    view.update(cx, |v, cx| {
        v.set_syntax(false, cx);
        assert!(
            shown.upgrade().is_some(),
            "colors switched off are dropped on the UI thread"
        );
    });
    cx.run_until_parked();
    assert!(
        shown.upgrade().is_none(),
        "the colors switched off are freed"
    );
}

#[gpui_kit::test]
async fn python_and_javascript_files_get_syntax_colors(cx: &mut TestAppContext) {
    for (name, text, keyword) in [
        ("a.py", "def f():\n    return 1\n", 0.."def".len()),
        ("a.js", "function f() { return 1; }\n", 0.."function".len()),
    ] {
        let (_dir, handle, view) = open_named(cx, name, text, text);
        wait_loaded(cx, handle, &view).await;
        wait_syntax(cx, &view, true).await;
        let styles = view.read_with(cx, |v, _| v.line_styles_for_test(true, 0));
        assert!(
            styles
                .iter()
                .any(|(r, s)| *r == keyword && s.color.is_some()),
            "{name}: the keyword is colored: {styles:?}"
        );
    }
}

#[test]
fn typing_stats_report_mean_and_max() {
    let stats = duet_app::text::editing::TypingStats {
        samples: vec![1.0, 3.0, 2.0],
        ..Default::default()
    };
    assert_eq!((stats.mean(), stats.max()), (2.0, 3.0));
}

#[test]
fn typing_stats_find_the_max_sample_and_the_steady_state() {
    let stats = duet_app::text::editing::TypingStats {
        samples: vec![30.0, 16.0, 18.0, 17.0],
        ..Default::default()
    };
    assert_eq!(stats.max_at(), 0);
    assert_eq!((stats.steady_mean(), stats.steady_max()), (17.0, 18.0));
    let empty = duet_app::text::editing::TypingStats::default();
    assert_eq!(
        (empty.max_at(), empty.steady_mean(), empty.steady_max()),
        (0, 0.0, 0.0)
    );
}

#[test]
fn typing_stats_split_the_first_sample_into_edit_and_frame() {
    let stats = duet_app::text::editing::TypingStats {
        samples: vec![40.0, 17.0],
        edits: vec![25.0, 0.5],
    };
    assert_eq!((stats.first_edit(), stats.first_frame()), (25.0, 15.0));
    let more = duet_app::text::editing::TypingStats {
        samples: vec![40.0, 17.0, 17.0],
        edits: vec![25.0, 4.0, 8.0],
    };
    assert_eq!(
        (more.steady_edit_mean(), more.steady_edit_max()),
        (6.0, 8.0)
    );
    let empty = duet_app::text::editing::TypingStats::default();
    assert_eq!((empty.first_edit(), empty.first_frame()), (0.0, 0.0));
}

fn same_colour(a: gpui_kit::Hsla, b: gpui_kit::Hsla) -> bool {
    let (a, b): (gpui_kit::Rgba, gpui_kit::Rgba) = (a.into(), b.into());
    [(a.r, b.r), (a.g, b.g), (a.b, b.b)]
        .iter()
        .all(|(x, y)| (x - y).abs() <= 1.5 / 255.)
}

#[gpui_kit::test]
async fn syntax_colours_follow_the_theme(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_named(cx, "a.rs", "fn main() {}\n", "fn main() { }\n");
    wait_loaded(cx, handle, &view).await;
    wait_syntax(cx, &view, true).await;
    let keyword = |cx: &mut TestAppContext| {
        view.read_with(cx, |v, _| v.line_styles_for_test(true, 0))
            .into_iter()
            .find(|(r, _)| r.start == 0)
            .and_then(|(_, s)| s.color)
            .expect("`fn` is coloured")
    };
    assert!(same_colour(keyword(cx), Palette::light().sx.keyword));
    cx.update(|cx| {
        duet_app::theme::toggle(cx);
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    assert!(
        same_colour(keyword(cx), Palette::dark().sx.keyword),
        "the next frame recolours"
    );
}

#[gpui_kit::test]
async fn the_save_buttons_follow_each_sides_dirty_state(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    assert!(!view.read_with(cx, |v, _| v.save_enabled(true) || v.save_enabled(false)));
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    cx.update_window(handle, |_, window, cx| window.input("z", cx))
        .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| (v.save_enabled(true), v.save_enabled(false))),
        (true, false)
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("text-save-right", cx);
    })
    .unwrap();
    assert!(
        view.read_with(cx, |v, _| v.is_dirty()),
        "Save right leaves the edited left alone"
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("text-save-left", cx);
    })
    .unwrap();
    wait_clean(cx, &view).await;
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "za\n"
    );
    assert!(!view.read_with(cx, |v, _| v.save_enabled(true)));
}

#[gpui_kit::test]
async fn the_undo_and_redo_buttons_act_like_their_keys(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "abc\n", "abc\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 3, cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("text-undo", cx);
    })
    .unwrap();
    assert_eq!(line_of(&view, cx, true, 0), "abc");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("text-redo", cx);
    })
    .unwrap();
    assert_eq!(line_of(&view, cx, true, 0), "abcx");
}

#[gpui_kit::test]
async fn the_navigation_buttons_jump_and_show_the_position(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\nd\n", "A\nb\nC\nD\n");
    wait_loaded(cx, handle, &view).await;
    let block =
        |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.model().unwrap().current_block());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("text-next").label(),
            Some("Next difference (F8)")
        );
        assert_eq!(
            window.find("text-first").label(),
            Some("First difference (Ctrl+Home)")
        );
        window.click("text-last", cx);
    })
    .unwrap();
    assert_eq!(block(cx), Some(1), "a, then c+d: two blocks");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("text-first", cx);
    })
    .unwrap();
    assert_eq!(block(cx), Some(0));
    let text = view.read_with(cx, |v, _| duet_app::text::view::position_text(v.model()));
    assert_eq!(text, "1 / 2");
}

#[gpui_kit::test]
async fn the_filter_segments_show_the_active_filter(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nB\n");
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "ctrl-2");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("text-filter-diffs").selected(), Some(true));
        assert_eq!(window.find("text-filter-all").selected(), Some(false));
        window.click("text-filter-same", cx);
    })
    .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().filter()),
        TextFilter::Same
    );
}

#[gpui_kit::test]
async fn the_whitespace_menu_sets_one_mode(cx: &mut TestAppContext) {
    use duet_core::text::diff::WhitespaceMode;
    let (_dir, handle, view) = open_text(cx, "a  b\n", "a b\n");
    wait_loaded(cx, handle, &view).await;
    for mode in [
        WhitespaceMode::All,
        WhitespaceMode::Edges,
        WhitespaceMode::None,
    ] {
        view.update(cx, |v, cx| v.set_whitespace(mode, cx));
        wait_loaded(cx, handle, &view).await;
        assert_eq!(
            view.read_with(cx, |v, _| v.request().options.whitespace),
            mode
        );
    }
}

#[gpui_kit::test]
async fn the_sub_toolbar_wraps_on_a_narrow_window(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    cx.simulate_window_resize(handle, size(px(700.), px(800.)));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let bar = window.find("text-subbar").bounds();
        assert!(bar.size.height > px(40.), "two or more rows: {bar:?}");
        for id in [
            "text-filter",
            "text-ignore-ws",
            "text-ignore-case",
            "text-ignore-eol",
            "text-ignore-blank",
            "text-ignore-comments",
            "text-moves",
            "text-syntax",
            "text-language",
            "text-show-ws",
            "text-wrap",
            "text-details",
            "text-tab",
            "text-font",
            "text-enc-left",
            "text-enc-right",
            "text-save-options",
            "text-find-open",
            "text-goto-open",
            "text-save-right",
        ] {
            let b = window.find(id).bounds();
            assert!(b.right() <= px(700.) + px(0.5), "{id} is clipped at {b:?}");
        }
    })
    .unwrap();
}

fn near(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() < px(0.5)
}

#[gpui_kit::test]
async fn the_names_bar_lines_up_with_the_stage(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nc\n");
    wait_loaded(cx, handle, &view).await;
    for width in [1200., 900.] {
        cx.simulate_window_resize(handle, size(px(width), px(800.)));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            let gutter = window.find(("text-gutter", 0usize)).bounds();
            let names_gutter = window.find("text-names-gutter").bounds();
            assert!(
                near(names_gutter.left(), gutter.left()),
                "{width}: {names_gutter:?} vs {gutter:?}"
            );
            assert!(
                near(gutter.size.width, px(CENTRE_GUTTER))
                    && near(names_gutter.size.width, px(CENTRE_GUTTER))
            );
            let overview = window.find("text-overview").bounds();
            let tail = window.find("text-names-tail").bounds();
            let list = window.find("text-list").bounds();
            assert!(near(overview.size.width, px(OVERVIEW_WIDTH)));
            assert!(near(tail.right(), overview.right()));
            assert!(near(tail.left(), list.right() - px(SCROLLBAR_WIDTH)));
            let left = window.find("text-names-left").bounds();
            let row = window.find(("text-row", 0usize)).bounds();
            assert!(near(left.left(), row.left()) && near(left.right(), gutter.left()));
        })
        .unwrap();
    }
}

#[gpui_kit::test]
async fn a_long_path_keeps_the_names_bar_info_visible(cx: &mut TestAppContext) {
    let deep = Path::new(&"a_deep_folder_name".repeat(6)).join("file.txt");
    let (_dir, handle, view) = open_files(cx, (&deep, b"x\n"), (Path::new("b.txt"), b"y\n"));
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    cx.update_window(handle, |_, window, cx| window.input("z", cx))
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let cell = window.find("text-names-left").bounds();
        for id in ["text-info-left", "text-dirty-left", "text-left-path-copy"] {
            let b = window.find(id).bounds();
            assert!(
                b.left() >= cell.left() && b.right() <= cell.right() + px(0.5),
                "{id} at {b:?} in {cell:?}"
            );
        }
    })
    .unwrap();
}

#[gpui_kit::test]
async fn a_thai_line_keeps_its_caret_at_the_code_size(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "ภาษาไทย ok\n", "ภาษาลาว ok\n");
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        // The column count follows the code font at the code size (12.5 px).
        let fonts = duet_app::ui::fonts::Fonts::current(cx);
        let advance = duet_app::ui::fonts::code_advance(window, &fonts.code);
        let list = window.find("text-list").bounds();
        assert_eq!(
            view.read(cx).display_options().view_columns,
            duet_app::text::view::stage::view_columns(list.size.width, advance),
            "columns at {advance:?} per column"
        );
        window.click_at(
            ("text-row", 0usize),
            point(px(NUMBER_CELL + TEXT_INSET + 20.), px(10.)),
            cx,
        );
    })
    .unwrap();
    let caret = view.read_with(cx, |v, _| v.caret()).expect("a caret");
    let line = "ภาษาไทย ok";
    assert!(caret.left);
    assert!(
        line.is_char_boundary(caret.head) && caret.head > 0 && caret.head < line.len(),
        "at {}",
        caret.head
    );
    assert!(view.read_with(cx, |v, _| v.display_options().view_columns) > 20);
}

#[gpui_kit::test]
async fn the_current_difference_and_fillers_look_like_localdiff(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\n", "a\nc\n");
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "f8");
    let p = Palette::light();
    let look = |cx: &mut TestAppContext, row: usize, left: bool| -> HalfLook {
        view.read_with(cx, |v, _| v.half_look_for_test(row, left))
            .unwrap()
    };
    let (left, right) = (look(cx, 1, true), look(cx, 1, false));
    assert_eq!(left.fill, HalfFill::Tint(p.bg_del));
    assert_eq!(
        (left.number_bg, left.number, left.bar),
        (Some(p.ln_del), p.accent, Some(p.accent))
    );
    assert_eq!(right.fill, HalfFill::Hatch);
    assert_eq!(
        look(cx, 0, true).bar,
        None,
        "row 0 is not in the current block"
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("text-filler-right", 1usize)).is_some());
        assert!(window.try_find(("text-filler-left", 1usize)).is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
async fn an_ignored_difference_shows_as_unimportant(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "A b\nsame\n", "a  b\nsame\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.toggle_ignore(IgnoreChoice::Case, cx));
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.toggle_ignore(IgnoreChoice::WhitespaceAll, cx));
    wait_loaded(cx, handle, &view).await;
    let p = Palette::light();
    let (hidden, same) = view.read_with(cx, |v, _| {
        (
            v.half_look_for_test(0, true).unwrap(),
            v.half_look_for_test(1, true).unwrap(),
        )
    });
    assert_eq!((hidden.edge, hidden.number), (Some(p.unimp), p.unimp));
    // Ruling (Task 8): the ≈ row is tinted bg_unimp and what the option hid is hl_unimp.
    assert_eq!(
        (hidden.fill, hidden.highlight),
        (HalfFill::Tint(p.bg_unimp), Some(p.hl_unimp))
    );
    assert_eq!(same.edge, None);
}

#[gpui_kit::test]
async fn a_moved_line_says_so_after_its_text(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(
        cx,
        "a\nfn one()\nfn two()\n}\nb\nc\nd\ne\nf\n",
        "a\nb\nc\nd\ne\nf\nfn one()\nfn two()\n}\n",
    );
    wait_loaded(cx, handle, &view).await;
    let display = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        m.visible()
            .display_of(m.row_of_line(true, 1).unwrap())
            .unwrap()
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let row = window.find(("text-row", display)).bounds();
        let tag = window.find(("text-moved-left", display)).bounds();
        assert!(
            tag.left() > row.left() + px(NUMBER_CELL + TEXT_INSET),
            "after the text: {tag:?}"
        );
        assert!(tag.right() < window.find(("text-gutter", display)).bounds().left());
    })
    .unwrap();
}

#[gpui_kit::test]
async fn the_gutter_buttons_are_small_bordered_and_named(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\nd\n", "A\nb\nC\nd\n");
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "f8");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let right = window.find(("copy-right", 0usize));
        assert_eq!(
            (right.bounds().size.width, right.bounds().size.height),
            (px(23.), px(18.))
        );
        assert_eq!(right.label(), Some("Copy block to the right (Alt+→)"));
        assert_eq!(
            window.find(("copy-left", 0usize)).label(),
            Some("Copy block to the left (Alt+←)")
        );
        assert_eq!(
            right.selected(),
            Some(true),
            "block 0 is current: accent border"
        );
        assert_eq!(window.find(("copy-right", 1usize)).selected(), Some(false));
        let gutter = window.find(("text-gutter", 0usize)).bounds();
        assert!(right.bounds().left() >= gutter.left() && right.bounds().right() <= gutter.right());
    })
    .unwrap();
}

#[gpui_kit::test]
async fn fold_rows_are_one_row_high_and_keep_their_buttons(cx: &mut TestAppContext) {
    let left: String = (0..40).map(|i| format!("line {i}\n")).collect();
    let right = left
        .replace("line 0\n", "LINE 0\n")
        .replace("line 39\n", "LINE 39\n");
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "ctrl-3");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let fold = window.find(("text-fold", 4usize)).bounds();
        assert_eq!(fold.size.height, px(20.));
        let more = window.find(("text-fold-more", 4usize)).bounds();
        assert!(
            more.top() >= fold.top() && more.bottom() <= fold.bottom(),
            "the buttons sit in the row"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
async fn a_long_paths_tooltip_wraps_within_its_width(cx: &mut TestAppContext) {
    let deep: std::path::PathBuf = (0..6)
        .map(|i| format!("component_{i}_of_a_deep_path"))
        .collect();
    let left = Path::new("left").join(deep).join("a.txt");
    let (_dir, handle, view) = open_files(
        cx,
        (&left, b"a\n".as_slice()),
        (Path::new("b.txt"), b"b\n".as_slice()),
    );
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.hover("text-left-path", cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let tip = window.find("tooltip-text").bounds();
        let max = px(duet_app::ui::style::TOOLTIP_MAX_WIDTH);
        assert!(tip.size.width <= max + px(0.5), "{tip:?}");
        assert!(
            tip.size.height > px(30.),
            "the path wraps onto more lines: {tip:?}"
        );
        assert!(tip.right() <= window.viewport_size().width, "{tip:?}");
    })
    .unwrap();
}

#[gpui_kit::test]
async fn the_find_field_is_short_and_its_tooltip_names_the_keys(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "ctrl-f");
    let placeholder = view.read_with(cx, |v, cx| {
        let input = v.find_input_for_test().expect("the find bar is open");
        input.read(cx).presentation().placeholder().to_string()
    });
    assert_eq!(
        placeholder, "Find text",
        "short enough for the 220 px field"
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.hover("text-find-field", cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("tooltip-text").is_some(),
            "hovering the field shows its tooltip"
        );
    })
    .unwrap();
    assert_eq!(
        duet_app::text::view::FIND_TOOLTIP,
        "Find text (Enter: next match, Shift+Enter: previous match)"
    );
}

#[gpui_kit::test]
async fn a_read_only_format_view_offers_no_copy_arrows(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "{\"a\":1,\"b\":2}", "{\"a\":1,\"b\":3}");
    wait_loaded(cx, handle, &view).await;
    let arrows = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            [("copy-right", 0usize), ("copy-left", 0usize)].map(|id| window.try_find(id).is_some())
        })
        .unwrap()
    };
    assert_eq!(
        arrows(cx),
        [true, true],
        "an editable pair offers both copies"
    );
    view.update(cx, |v, cx| v.set_format_for_test(true, cx));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().block_count()),
        1
    );
    assert_eq!(
        arrows(cx),
        [false, false],
        "neither side of the formatted view can take a copy"
    );
    // The copy-all buttons are off: a click does nothing, not even a notice.
    cx.update_window(handle, |_, window, cx| {
        window.click("text-copy-all-right", cx);
        window.click("text-copy-all-left", cx);
    })
    .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned)),
        None
    );
    // The keys still say why.
    press(cx, handle, "f8");
    press(cx, handle, "alt-right");
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("The formatted view is read-only. Turn Format off to edit.")
    );
}

#[gpui_kit::test]
async fn a_quick_click_on_the_horizontal_bar_leaves_no_drag_behind(cx: &mut TestAppContext) {
    let long = format!("{}\nshort\n", "word ".repeat(400));
    let (_dir, handle, view) = open_text(cx, &long, &long.replace("short", "brief"));
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let skip = |cx: &App| view.read(cx).display_options().skip_columns;
        let bar = window.find("text-hscroll").bounds();
        let at = point(bar.left() + px(4.), bar.top() + px(5.));
        // Press and release land before the next frame, as a fast click's can.
        window.dispatch_event(button(at, true), cx);
        window.dispatch_event(button(at, false), cx);
        window.render_frame(cx);
        let clicked = skip(cx);
        let far = point(bar.right() - px(4.), bar.top() - px(60.));
        pointer(window, move_to(far, true), cx);
        assert_eq!(skip(cx), clicked, "no drag runs after the release");
        window.hover("text-swap", cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("tooltip-text").is_some(),
            "a plain move reaches the toolbar: its tooltip shows"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
async fn the_overview_drag_follows_the_pointer_off_the_strip(cx: &mut TestAppContext) {
    let (left, right) = long_pair();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    let mut tops = Vec::new();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let bar = window.find("text-overview").bounds();
        let start = point(bar.center().x, bar.top() + px(2.));
        pointer(window, move_to(start, false), cx);
        pointer(window, button(start, true), cx);
        // Down the bar while drifting 30 px to its left, off the 18 px strip.
        for step in 1..=5 {
            let y = bar.top() + bar.size.height * (0.15 * step as f32);
            pointer(window, move_to(point(bar.left() - px(30.), y), true), cx);
            tops.push(view.read(cx).top_row());
        }
        pointer(
            window,
            button(point(bar.left() - px(30.), bar.bottom()), false),
            cx,
        );
    })
    .unwrap();
    assert!(
        tops.windows(2).all(|w| w[1] > w[0]),
        "the rows keep following the drag: {tops:?}"
    );
}

#[gpui_kit::test]
async fn a_press_on_the_thumb_grabs_it_and_a_press_on_the_track_centres_it(
    cx: &mut TestAppContext,
) {
    // About four halves wide, so the thumb is about a quarter of the bar.
    let line = "word ".repeat(56);
    let text = format!("{line}\nshort\n");
    let (_dir, handle, view) = open_text(cx, &text, &text.replace("short", "brief"));
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    for _ in 0..4 {
        press(cx, handle, "right");
    }
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let skip = |cx: &App| view.read(cx).display_options().skip_columns;
        let thumb = |cx: &App| view.read(cx).hscroll_thumb_for_test().unwrap();
        let bar = window.find("text-hscroll").bounds();
        let y = bar.top() + px(5.);
        let (x, w, width) = thumb(cx);
        assert!(
            x > 20. && w < width / 2.,
            "a thumb away from the start: {x} {w}"
        );
        let before = skip(cx);
        // Pressed 10 px inside its left edge, then moved 40 px right.
        let grab = point(bar.left() + px(x + 10.), y);
        pointer(window, move_to(grab, false), cx);
        pointer(window, button(grab, true), cx);
        assert_eq!(skip(cx), before, "a press on the thumb does not jump it");
        pointer(window, move_to(point(grab.x + px(40.), y), true), cx);
        let (moved, _, _) = thumb(cx);
        assert!(
            (moved - x - 40.).abs() <= 5.,
            "the thumb keeps the point it was grabbed by: {x} -> {moved}"
        );
        pointer(window, button(point(grab.x + px(40.), y), false), cx);
        // On the track, 200 px past the thumb: the thumb's middle comes under the pointer.
        let (x, w, _) = thumb(cx);
        let track = x + w + 200.;
        let at = point(bar.left() + px(track), y);
        pointer(window, move_to(at, false), cx);
        pointer(window, button(at, true), cx);
        pointer(window, button(at, false), cx);
        let (x, w, _) = thumb(cx);
        assert!(
            (x + w / 2. - track).abs() <= 5.,
            "a press on the track centres the thumb: {} vs {track}",
            x + w / 2.
        );
    })
    .unwrap();
}

// ---- Row selection on the line numbers (P4 task 9) ----

/// The left number cell of display row `i`, a few px in from its left edge.
fn left_number(i: usize) -> (gpui_kit::ElementId, Point<Pixels>) {
    (("text-row", i).into(), point(px(6.), px(10.)))
}

fn click_mod(
    window: &mut Window,
    cx: &mut App,
    id: impl Into<gpui_kit::ElementId>,
    offset: Option<Point<Pixels>>,
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

fn selected_rows(cx: &mut TestAppContext, view: &Entity<TextView>) -> Option<std::ops::Range<u32>> {
    view.read_with(cx, |v, _| v.row_selection().map(|s| s.rows()))
}

fn right_text(cx: &mut TestAppContext, view: &Entity<TextView>) -> String {
    view.read_with(cx, |v, _| {
        v.model().unwrap().buffer(false).unwrap().text().to_owned()
    })
}

#[gpui_kit::test]
async fn clicking_and_shift_clicking_line_numbers_selects_rows(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\nd\n", "a\nB\nC\nd\n");
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        let (id, at) = left_number(1);
        window.click_at(id, at, cx);
    })
    .unwrap();
    assert_eq!(selected_rows(cx, &view), Some(1..2));
    assert!(
        view.read_with(cx, |v, _| v.caret().is_none()),
        "a row selection is not a caret"
    );
    cx.update_window(handle, |_, window, cx| {
        let (id, at) = left_number(2);
        click_mod(
            window,
            cx,
            id,
            Some(at),
            gpui_kit::Modifiers {
                shift: true,
                ..Default::default()
            },
        );
    })
    .unwrap();
    assert_eq!(selected_rows(cx, &view), Some(1..3));
    let look = view
        .read_with(cx, |v, _| v.half_look_for_test(1, true))
        .unwrap();
    let palette = Palette::light();
    assert_eq!(look.number_bg, Some(palette.sel));
    assert_eq!(look.number, palette.text);
    press(cx, handle, "escape");
    assert_eq!(selected_rows(cx, &view), None, "Esc clears it first");
}

#[gpui_kit::test]
async fn placing_a_caret_drops_the_row_selection(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nB\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select_rows_for_test(true, 0, 1, cx));
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    assert_eq!(selected_rows(cx, &view), None, "a caret clears the rows");
}

#[gpui_kit::test]
async fn alt_right_copies_only_the_selected_rows(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\nd\n", "a\nB\nC\nd\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select_rows_for_test(true, 1, 1, cx));
    press(cx, handle, "alt-right");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        right_text(cx, &view),
        "a\nb\nC\nd\n",
        "only row 1 went across"
    );
}

#[gpui_kit::test]
async fn ctrl_c_and_delete_work_on_selected_rows(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\r\nb\r\nc\r\n", "a\r\nb\r\nc\r\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select_rows_for_test(false, 0, 1, cx));
    press(cx, handle, "ctrl-c");
    let clip = cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
    assert_eq!(clip.as_deref(), Some("a\r\nb"));
    press(cx, handle, "delete");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(right_text(cx, &view), "c\r\n");
    assert_eq!(selected_rows(cx, &view), None);
    press(cx, handle, "ctrl-z");
    assert_eq!(right_text(cx, &view), "a\r\nb\r\nc\r\n", "one undo step");
}

#[gpui_kit::test]
async fn dragging_down_the_line_numbers_selects_the_range(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "1\n2\n3\n4\n5\n", "1\n2\n3\n4\n5\n");
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let from = window.find(("text-row", 0usize)).bounds().origin + point(px(6.), px(10.));
        let to = window.find(("text-row", 3usize)).bounds().origin + point(px(6.), px(10.));
        window.drag(from, to, cx);
    })
    .unwrap();
    assert_eq!(selected_rows(cx, &view), Some(0..4));
}

#[gpui_kit::test]
async fn dragging_back_up_shrinks_the_range(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "1\n2\n3\n4\n5\n6\n", "1\n2\n3\n4\n5\n6\n");
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let at = |window: &Window, i: usize| {
            window.find(("text-row", i)).bounds().origin + point(px(6.), px(10.))
        };
        let (from, far, back) = (at(window, 1), at(window, 5), at(window, 3));
        pointer(window, move_to(from, false), cx);
        pointer(window, button(from, true), cx);
        pointer(window, move_to(far, true), cx);
        pointer(window, move_to(back, true), cx);
        pointer(window, button(back, false), cx);
    })
    .unwrap();
    assert_eq!(
        selected_rows(cx, &view),
        Some(1..4),
        "shrunk from the anchor"
    );
}

#[gpui_kit::test]
async fn a_press_on_the_bottom_edge_row_does_not_scroll_or_extend(cx: &mut TestAppContext) {
    let (left, right) = long_pair();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let list = window.find("text-list").bounds();
        let at = point(list.left() + px(6.), list.bottom() - px(2.));
        pointer(window, move_to(at, false), cx);
        pointer(window, button(at, true), cx);
        for _ in 0..5 {
            window.render_frame(cx);
        }
    })
    .unwrap();
    assert_eq!(
        top_row(cx, handle, &view),
        0,
        "no auto-scroll before a move"
    );
    let rows = selected_rows(cx, &view).unwrap();
    assert_eq!(rows.end - rows.start, 1, "still one row: {rows:?}");
    cx.update_window(handle, |_, window, cx| {
        let list = window.find("text-list").bounds();
        let at = point(list.left() + px(6.), list.bottom() - px(2.));
        pointer(window, button(at, false), cx);
    })
    .unwrap();
}

#[gpui_kit::test]
async fn dragging_past_the_bottom_scrolls_and_extends(cx: &mut TestAppContext) {
    let (left, right) = long_pair();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let list = window.find("text-list").bounds();
        let from = window.find(("text-row", 0usize)).bounds().origin + point(px(6.), px(10.));
        pointer(window, move_to(from, false), cx);
        pointer(window, button(from, true), cx);
        let below = point(list.left() + px(6.), list.bottom() + px(30.));
        pointer(window, move_to(below, true), cx);
        for _ in 0..10 {
            window.render_frame(cx);
        }
        pointer(window, button(below, false), cx);
    })
    .unwrap();
    assert!(top_row(cx, handle, &view) > 0, "the list scrolled");
    let rows = selected_rows(cx, &view).unwrap();
    assert!(rows.end > 40, "the range follows the scroll: {rows:?}");
}

#[gpui_kit::test]
async fn row_selection_on_a_read_only_side_explains_why_it_cannot_change(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_named(cx, "x.json", "{\"a\":1}", "{\"a\":2}");
    wait_loaded(cx, handle, &view).await;
    // Substitution (P7): the existing test setter for the Format toggle.
    view.update(cx, |v, cx| v.set_format_for_test(true, cx));
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select_rows_for_test(true, 0, 0, cx));
    press(cx, handle, "alt-right");
    let notice = view
        .read_with(cx, |v, _| v.notice().map(str::to_owned))
        .unwrap();
    assert!(!notice.is_empty(), "the formatted view says why: {notice}");
}

#[gpui_kit::test]
async fn a_one_pixel_jitter_on_the_bottom_edge_row_neither_scrolls_nor_extends(
    cx: &mut TestAppContext,
) {
    let (left, right) = long_pair();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let list = window.find("text-list").bounds();
        let at = point(list.left() + px(6.), list.bottom() - px(2.));
        pointer(window, move_to(at, false), cx);
        pointer(window, button(at, true), cx);
        pointer(window, move_to(at + point(px(0.), px(1.)), true), cx);
        for _ in 0..5 {
            window.render_frame(cx);
        }
    })
    .unwrap();
    assert_eq!(top_row(cx, handle, &view), 0, "1 px does not scroll");
    let rows = selected_rows(cx, &view).unwrap();
    assert_eq!(rows.end - rows.start, 1, "still one row: {rows:?}");
    cx.update_window(handle, |_, window, cx| {
        let list = window.find("text-list").bounds();
        let at = point(list.left() + px(6.), list.bottom() - px(1.));
        pointer(window, button(at, false), cx);
    })
    .unwrap();
}

fn row_items(cx: &mut TestAppContext, view: &Entity<TextView>) -> Vec<String> {
    view.read_with(cx, |v, _| {
        duet_app::ui::menu::outline(&duet_app::text::menu::row_menu(v))
    })
}

fn right_click_row(cx: &mut TestAppContext, handle: AnyWindowHandle, display: usize) {
    cx.update_window(handle, |_, window, cx| {
        window.right_click(("text-row", display), cx)
    })
    .unwrap();
    cx.run_until_parked();
}

/// Runs the menu item `label` the way the popup does (pre-flight ruling P1).
fn run_item(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
    label: &str,
) -> bool {
    let entries = view.read_with(cx, |v, _| duet_app::text::menu::row_menu(v));
    let view = view.clone();
    cx.update_window(handle, |_, window, cx| {
        let focus = view.focus_handle(cx);
        duet_app::ui::menu::activate(entries, label, Some(&focus), window, cx)
    })
    .unwrap()
}

fn left_text(cx: &mut TestAppContext, view: &Entity<TextView>) -> String {
    view.read_with(cx, |v, _| {
        v.model().unwrap().buffer(true).unwrap().text().to_owned()
    })
}

#[gpui_kit::test]
async fn f9_forgets_the_menu_target_so_f2_does_not_use_the_old_side(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\n", "a\nB\nc\n");
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 1);
    // The first Esc closes the popup, the second clears the row selection.
    press(cx, handle, "escape");
    press(cx, handle, "escape");
    assert_eq!(selected_rows(cx, &view), None);
    press(cx, handle, "f9");
    assert_eq!(view.read_with(cx, |v, _| v.menu_row_for_test()), None);
    press(cx, handle, "f2");
    assert!(view.read_with(cx, |v, _| v.caret()).is_none());
}

#[gpui_kit::test]
async fn a_rediff_result_drops_the_row_selection(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\n", "a\nB\nc\n");
    wait_loaded(cx, handle, &view).await;
    // An edit schedules the re-diff whose result is delivered below.
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("1", cx))
        .unwrap();
    right_click_row(cx, handle, 1);
    assert!(selected_rows(cx, &view).is_some());
    let (generation, (l, r, versions)) = view.read_with(cx, |v, _| {
        (v.rediff_generation(), v.model().unwrap().snapshot())
    });
    let request = view.read_with(cx, |v, _| v.request().clone());
    let fresh = duet_app::text::job::rediff(l, r, &request, &duet_core::cancel::CancelToken::new());
    view.update(cx, |v, cx| {
        v.apply_rediff_for_test(generation, fresh, versions, cx)
    });
    assert_eq!(selected_rows(cx, &view), None);
}

#[gpui_kit::test]
async fn right_click_on_a_change_row_offers_the_row_menu(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\n", "a\nB\nc\n");
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 1);
    assert_eq!(selected_rows(cx, &view), Some(1..2));
    assert_eq!(
        row_items(cx, &view),
        [
            "Copy selected rows to the right",
            "Copy selected rows to the left",
            "Copy this difference to the right",
            "Copy this difference to the left",
            "-",
            "Edit this line",
            "Delete selected lines",
            "Copy text",
            "-",
            "Align this line with a line on the other side…",
            "(Clear manual alignments (0))",
            "Treat lines like this as unimportant",
        ]
    );
}

#[gpui_kit::test]
async fn right_click_on_an_unchanged_row_has_no_difference_items(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\n", "a\nB\nc\n");
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 0);
    let items = row_items(cx, &view);
    assert!(!items.iter().any(|i| i.contains("difference")), "{items:?}");
    assert!(items.contains(&"Edit this line".to_owned()));
}

#[gpui_kit::test]
async fn right_click_on_a_placeholder_half_offers_insert(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 1);
    // The centre click lands in the gutter, which picks the right side: a placeholder there.
    let items = row_items(cx, &view);
    assert!(
        items.contains(&"Insert a line here".to_owned()),
        "{items:?}"
    );
    assert!(items.contains(&"(Delete selected lines)".to_owned()));
}

#[gpui_kit::test]
async fn right_click_on_a_fold_row_has_no_menu(cx: &mut TestAppContext) {
    let (left, right) = long_pair();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "ctrl-3");
    let fold = view
        .read_with(cx, |v, _| {
            let m = v.model().unwrap();
            (0..m.row_count()).find(|&i| {
                matches!(
                    m.display_row(i),
                    duet_app::text::model::DisplayRow::Fold { .. }
                )
            })
        })
        .unwrap();
    // A row first, so a stale target would show.
    right_click_row(cx, handle, fold + 1);
    assert!(!row_items(cx, &view).is_empty());
    cx.update_window(handle, |_, window, cx| {
        window.right_click(("text-fold", fold), cx)
    })
    .unwrap();
    cx.run_until_parked();
    assert!(row_items(cx, &view).is_empty());
    assert_eq!(view.read_with(cx, |v, _| v.menu_row_for_test()), None);
}

fn right_button(position: Point<Pixels>, down: bool) -> gpui_kit::PlatformInput {
    use gpui_kit::{MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput};
    if down {
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Right,
            position,
            modifiers: Default::default(),
            click_count: 1,
            first_mouse: false,
        })
    } else {
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Right,
            position,
            modifiers: Default::default(),
            click_count: 1,
        })
    }
}

#[gpui_kit::test]
async fn right_click_on_blank_space_clears_the_target(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nB\n");
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 1);
    assert!(view.read_with(cx, |v, _| v.menu_row_for_test()).is_some());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let list = window.find("text-list").bounds();
        let below = point(list.left() + px(20.), list.bottom() - px(3.));
        pointer(window, right_button(below, true), cx);
        pointer(window, right_button(below, false), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| v.menu_row_for_test()), None);
    assert!(row_items(cx, &view).is_empty());
}

#[gpui_kit::test]
async fn copy_this_difference_item_copies_that_block(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\nd\n", "a\nB\nc\nD\n");
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 1);
    assert!(run_item(
        cx,
        handle,
        &view,
        "Copy this difference to the left"
    ));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        left_text(cx, &view),
        "a\nB\nc\nd\n",
        "only the clicked block went across"
    );
}

#[gpui_kit::test]
async fn copy_selected_rows_item_copies_the_selection(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\nd\n", "a\nB\nC\nd\n");
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 2);
    assert!(run_item(
        cx,
        handle,
        &view,
        "Copy selected rows to the left"
    ));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(left_text(cx, &view), "a\nb\nC\nd\n");
}

#[gpui_kit::test]
async fn delete_selected_lines_item_removes_them(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\n", "a\nB\nc\n");
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 1);
    assert!(run_item(cx, handle, &view, "Delete selected lines"));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(right_text(cx, &view), "a\nc\n");
    assert_eq!(selected_rows(cx, &view), None);
}

#[gpui_kit::test]
async fn copy_text_item_copies_the_clicked_rows(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\n", "a\nB\nc\n");
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 1);
    assert!(run_item(cx, handle, &view, "Copy text"));
    let clip = cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
    assert_eq!(clip.as_deref(), Some("B"));
}

#[gpui_kit::test]
async fn edit_this_line_item_puts_the_caret_there(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nbb\nc\n", "a\nBB\nc\n");
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 1);
    assert!(run_item(cx, handle, &view, "Edit this line"));
    let caret = view.read_with(cx, |v, _| v.caret()).unwrap();
    assert_eq!((caret.left, caret.head), (false, 2));
    assert_eq!(selected_rows(cx, &view), None);
}

#[gpui_kit::test]
async fn edit_row_puts_the_caret_on_that_line(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nbb\nc\n", "a\nBB\nc\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select_rows_for_test(false, 1, 1, cx));
    press(cx, handle, "f2");
    let caret = view.read_with(cx, |v, _| v.caret()).unwrap();
    assert_eq!(
        (caret.left, caret.head),
        (false, 2),
        "start of line 1 on the right"
    );
}

#[gpui_kit::test]
async fn f2_on_a_placeholder_row_inserts_one_empty_line_and_undo_removes_it(
    cx: &mut TestAppContext,
) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\n", "a\nc\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select_rows_for_test(false, 1, 1, cx));
    press(cx, handle, "f2");
    assert_eq!(right_text(cx, &view), "a\n\nc\n");
    let caret = view.read_with(cx, |v, _| v.caret()).unwrap();
    assert_eq!((caret.left, caret.head, caret.anchor), (false, 2, 2));
    press(cx, handle, "ctrl-z");
    assert_eq!(right_text(cx, &view), "a\nc\n", "one undo step");
}

#[gpui_kit::test]
async fn insert_a_line_here_item_inserts_with_the_sides_own_terminator(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\r\nb\r\nc\r\n", "a\r\nc\r\n");
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 1);
    assert!(run_item(cx, handle, &view, "Insert a line here"));
    assert_eq!(right_text(cx, &view), "a\r\n\r\nc\r\n");
    let caret = view.read_with(cx, |v, _| v.caret()).unwrap();
    assert_eq!(
        (caret.left, caret.head),
        (false, 3),
        "on the new empty line"
    );
    press(cx, handle, "ctrl-z");
    assert_eq!(right_text(cx, &view), "a\r\nc\r\n");
}

#[gpui_kit::test]
async fn f2_on_a_placeholder_after_an_unterminated_last_line_adds_the_break_too(
    cx: &mut TestAppContext,
) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc", "a\nb");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select_rows_for_test(false, 2, 2, cx));
    press(cx, handle, "f2");
    assert_eq!(right_text(cx, &view), "a\nb\n\n");
    let caret = view.read_with(cx, |v, _| v.caret()).unwrap();
    assert_eq!(caret.head, 4, "on the new empty line");
}

#[gpui_kit::test]
async fn row_menu_disables_edits_on_a_formatted_view(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_named(cx, "x.json", "{\"a\":1}", "{\"a\":2}");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_format_for_test(true, cx));
    wait_loaded(cx, handle, &view).await;
    // The menu needs a target, so a real right-click follows the selection (it keeps it).
    view.update(cx, |v, cx| v.select_rows_for_test(true, 0, 0, cx));
    right_click_row(cx, handle, 0);
    let items = row_items(cx, &view);
    assert!(
        items.contains(&"(Copy selected rows to the right)".to_owned()),
        "{items:?}"
    );
    assert!(items.contains(&"(Delete selected lines)".to_owned()));
    assert!(items.contains(&"Copy text".to_owned()));
}

#[gpui_kit::test]
async fn deleting_rows_with_no_lines_on_that_side_says_so_and_keeps_the_selection(
    cx: &mut TestAppContext,
) {
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select_rows_for_test(false, 1, 1, cx));
    press(cx, handle, "delete");
    let notice = view.read_with(cx, |v, _| v.notice().map(str::to_owned));
    assert_eq!(
        notice.as_deref(),
        Some("There are no lines on that side to delete.")
    );
    assert_eq!(right_text(cx, &view), "a\n");
    assert_eq!(selected_rows(cx, &view), Some(1..2), "kept");
}

#[gpui_kit::test]
async fn f2_after_selecting_another_row_edits_that_row_not_the_old_menu_row(
    cx: &mut TestAppContext,
) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\nd\n", "a\nB\nC\nd\n");
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 1);
    view.update(cx, |v, cx| v.select_rows_for_test(false, 2, 2, cx));
    press(cx, handle, "f2");
    let caret = view.read_with(cx, |v, _| v.caret()).unwrap();
    assert_eq!((caret.left, caret.head), (false, 4), "line 2, not line 1");
}

#[gpui_kit::test]
async fn f2_after_a_caret_click_edits_nothing_stale(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\nd\n", "a\nB\nC\nd\n");
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 1);
    view.update(cx, |v, cx| v.set_caret_for_test(false, 0, cx));
    assert_eq!(view.read_with(cx, |v, _| v.menu_row_for_test()), None);
    press(cx, handle, "f2");
    let caret = view.read_with(cx, |v, _| v.caret()).unwrap();
    assert_eq!(caret.head, 0, "F2 did not jump to the old menu row");
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned)),
        None
    );
}

#[gpui_kit::test]
async fn f2_from_a_caret_in_the_text_moves_it_to_the_start_of_that_line(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nbbbb\nc\n", "a\nBBBB\nc\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(false, 4, cx));
    press(cx, handle, "f2");
    let caret = view.read_with(cx, |v, _| v.caret()).unwrap();
    assert_eq!((caret.left, caret.head, caret.anchor), (false, 2, 2));
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned)),
        None
    );
    assert_eq!(right_text(cx, &view), "a\nBBBB\nc\n", "nothing inserted");
}

#[gpui_kit::test]
async fn select_all_drops_the_menu_target(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\n", "a\nB\nc\n");
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 1);
    cx.update_window(handle, |_, window, cx| {
        let focus = view.focus_handle(cx);
        focus.focus(window, cx)
    })
    .unwrap();
    press(cx, handle, "ctrl-a");
    assert_eq!(view.read_with(cx, |v, _| v.menu_row_for_test()), None);
    assert_eq!(
        selected_rows(cx, &view),
        Some(0..3),
        "Ctrl+A took every row"
    );
}

#[gpui_kit::test]
async fn f2_with_a_menu_row_past_the_end_falls_back_to_the_selection(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nbb\nc\n", "a\nBB\nc\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.select_rows_for_test(false, 1, 1, cx);
        v.set_menu_row_for_test(99, false);
    });
    press(cx, handle, "f2");
    let caret = view.read_with(cx, |v, _| v.caret()).unwrap();
    assert_eq!((caret.left, caret.head), (false, 2), "the selection's line");
}

#[gpui_kit::test]
async fn f2_with_a_menu_row_past_the_end_and_no_selection_says_so(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nB\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, _| v.set_menu_row_for_test(99, false));
    press(cx, handle, "f2");
    assert_eq!(view.read_with(cx, |v, _| v.caret()), None);
    let notice = view.read_with(cx, |v, _| v.notice().map(str::to_owned));
    assert_eq!(notice.as_deref(), Some("Pick a line first."));
}

#[gpui_kit::test]
async fn f2_after_the_row_map_shrank_does_not_panic(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "a\nx\ny\nz\n");
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 3);
    view.update(cx, |v, cx| v.select_rows_for_test(false, 1, 3, cx));
    // The popup took the focus; the menu item would have given it back.
    cx.update_window(handle, |_, window, cx| {
        let focus = view.focus_handle(cx);
        focus.focus(window, cx)
    })
    .unwrap();
    press(cx, handle, "delete");
    wait_loaded(cx, handle, &view).await;
    cx.run_until_parked();
    press(cx, handle, "f2");
    let caret = view.read_with(cx, |v, _| v.caret()).unwrap();
    assert_eq!(caret.head, 2, "F2 left the caret at its own line start");
}

#[gpui_kit::test]
async fn ctrl_a_with_a_row_selection_selects_every_row_on_that_side(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\nd\n", "a\nB\nC\nd\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select_rows_for_test(false, 1, 2, cx));
    cx.update_window(handle, |_, window, cx| {
        let focus = view.focus_handle(cx);
        focus.focus(window, cx)
    })
    .unwrap();
    press(cx, handle, "ctrl-a");
    let sel = view.read_with(cx, |v, _| v.row_selection()).unwrap();
    assert!(!sel.left, "same side");
    assert_eq!(sel.rows(), 0..4);
    assert_eq!(view.read_with(cx, |v, _| v.menu_row_for_test()), None);
    press(cx, handle, "ctrl-c");
    let clip = cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
    assert_eq!(clip.as_deref(), Some("a\nB\nC\nd"));
}

type Opens = std::rc::Rc<std::cell::RefCell<Vec<(std::path::PathBuf, std::path::PathBuf)>>>;

/// Records every `TextEvent::Open` the view emits.
fn collect_opens(cx: &mut TestAppContext, view: &Entity<TextView>) -> Opens {
    let asked: Opens = Default::default();
    let sink = asked.clone();
    cx.update(|cx| {
        cx.subscribe(view, move |_, e: &duet_app::text::view::TextEvent, _| {
            if let duet_app::text::view::TextEvent::Open { left, right } = e {
                sink.borrow_mut().push((left.clone(), right.clone()));
            }
        })
        .detach()
    });
    asked
}

#[gpui_kit::test]
async fn pasting_the_clipboard_replaces_one_side_as_one_undo_step(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\r\nb\r\n", "a\r\nb\r\n");
    wait_loaded(cx, handle, &view).await;
    cx.update(|cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("x\ny\n".into())));
    view.update(cx, |v, cx| {
        assert!(v.replace_side_with_clipboard(false, cx))
    });
    wait_loaded(cx, handle, &view).await;
    let right = view.read_with(cx, |v, _| {
        v.model().unwrap().buffer(false).unwrap().text().to_owned()
    });
    assert_eq!(right, "x\r\ny\r\n", "the side's own line breaks");
    press(cx, handle, "ctrl-z");
    let right = view.read_with(cx, |v, _| {
        v.model().unwrap().buffer(false).unwrap().text().to_owned()
    });
    assert_eq!(right, "a\r\nb\r\n");
}

#[gpui_kit::test]
async fn pasting_an_empty_clipboard_changes_nothing_and_says_so(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    cx.update(|cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(String::new())));
    view.update(cx, |v, cx| {
        assert!(!v.replace_side_with_clipboard(true, cx))
    });
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("The clipboard has no text.")
    );
    assert!(!view.read_with(cx, |v, _| v.is_dirty()));
}

#[gpui_kit::test]
async fn a_typed_file_path_replaces_one_side(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let other = dir.path().join("ไฟล์.txt");
    std::fs::write(&other, "c\n").unwrap();
    let asked = collect_opens(cx, &view);
    cx.update_window(handle, |_, window, cx| window.click("text-right-path", cx))
        .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.path_edit_for_test()),
        Some(false)
    );
    view.update(cx, |v, cx| {
        v.set_path_edit_text_for_test(other.to_str().unwrap(), cx)
    });
    press(cx, handle, "enter");
    cx.run_until_parked();
    assert_eq!(asked.borrow().last().map(|(_, r)| r.clone()), Some(other));
    assert_eq!(view.read_with(cx, |v, _| v.path_edit_for_test()), None);
}

#[gpui_kit::test]
async fn the_path_box_opens_with_the_whole_path_selected_and_escape_restores_it(
    cx: &mut TestAppContext,
) {
    let (_dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let asked = collect_opens(cx, &view);
    cx.update_window(handle, |_, window, cx| window.click("text-left-path", cx))
        .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.path_edit_for_test()),
        Some(true)
    );
    let (text, selected) = view.read_with(cx, |v, cx| v.path_edit_state_for_test(cx).unwrap());
    let shown = view.read_with(cx, |v, _| v.request().left.display().to_string());
    assert_eq!(text, shown);
    assert_eq!(selected, 0..shown.len(), "all of it selected");
    press(cx, handle, "escape");
    assert_eq!(view.read_with(cx, |v, _| v.path_edit_for_test()), None);
    assert!(asked.borrow().is_empty(), "Esc applies nothing");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("text-left-path").is_some(),
            "the bar is back"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
async fn a_typed_path_that_is_not_a_file_keeps_the_input_open_with_a_notice(
    cx: &mut TestAppContext,
) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let asked = collect_opens(cx, &view);
    cx.update_window(handle, |_, window, cx| window.click("text-left-path", cx))
        .unwrap();
    for bad in [dir.path().to_path_buf(), dir.path().join("missing.txt")] {
        let text = bad.display().to_string();
        view.update(cx, |v, cx| v.set_path_edit_text_for_test(&text, cx));
        press(cx, handle, "enter");
        cx.run_until_parked();
        assert_eq!(
            view.read_with(cx, |v, _| v.notice().map(str::to_owned)),
            Some(format!("Not a file: {text}."))
        );
        assert_eq!(
            view.read_with(cx, |v, _| v.path_edit_for_test()),
            Some(true)
        );
    }
    assert!(asked.borrow().is_empty());
}

#[gpui_kit::test]
async fn a_relative_typed_file_path_is_refused(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let asked = collect_opens(cx, &view);
    cx.update_window(handle, |_, window, cx| window.click("text-left-path", cx))
        .unwrap();
    // Cargo.toml exists next to the test's working directory, which a typed path must not use.
    view.update(cx, |v, cx| v.set_path_edit_text_for_test("Cargo.toml", cx));
    press(cx, handle, "enter");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned)),
        Some("Type a full path, such as C:\\folder\\file.txt.".to_owned())
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.path_edit_for_test()),
        Some(true)
    );
    assert!(asked.borrow().is_empty());
}

#[gpui_kit::test]
async fn a_path_pasted_from_explorer_loses_its_quotes(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let other = dir.path().join("c.txt");
    std::fs::write(&other, "c\n").unwrap();
    let asked = collect_opens(cx, &view);
    cx.update_window(handle, |_, window, cx| window.click("text-left-path", cx))
        .unwrap();
    let quoted = format!("\"{}\"", other.display());
    view.update(cx, |v, cx| v.set_path_edit_text_for_test(&quoted, cx));
    press(cx, handle, "enter");
    cx.run_until_parked();
    assert_eq!(asked.borrow().last().map(|(l, _)| l.clone()), Some(other));
}

#[gpui_kit::test]
async fn opening_over_unsaved_edits_asks_first_and_cancel_opens_nothing(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let other = dir.path().join("c.txt");
    std::fs::write(&other, "c\n").unwrap();
    let asked = collect_opens(cx, &view);
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    cx.update_window(handle, |_, window, cx| window.input("z", cx))
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.open_path(true, other.clone(), window, cx))
    })
    .unwrap();
    wait_for_prompt(cx).await;
    assert!(asked.borrow().is_empty(), "nothing opens before the answer");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(asked.borrow().is_empty());
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.open_path(true, other.clone(), window, cx))
    })
    .unwrap();
    wait_for_prompt(cx).await;
    cx.simulate_prompt_answer("Discard");
    cx.run_until_parked();
    assert_eq!(asked.borrow().len(), 1);
}

fn drop_files(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    target: &'static str,
    paths: Vec<std::path::PathBuf>,
) {
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
}

#[gpui_kit::test]
async fn dropping_files_lands_on_the_half_under_the_pointer(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let (c, d) = (dir.path().join("c.txt"), dir.path().join("d.txt"));
    std::fs::write(&c, "c\n").unwrap();
    std::fs::write(&d, "d\n").unwrap();
    let (a, b) = view.read_with(cx, |v, _| {
        (v.request().left.clone(), v.request().right.clone())
    });
    let asked = collect_opens(cx, &view);
    drop_files(cx, handle, "text-names-right", vec![c.clone()]);
    cx.run_until_parked();
    assert_eq!(asked.borrow().last(), Some(&(a.clone(), c.clone())));
    drop_files(cx, handle, "text-names-left", vec![d.clone()]);
    cx.run_until_parked();
    assert_eq!(asked.borrow().last(), Some(&(d.clone(), b)));
    drop_files(cx, handle, "text-names-left", vec![c.clone(), d.clone()]);
    cx.run_until_parked();
    assert_eq!(asked.borrow().last(), Some(&(c, d)));
    assert_eq!(asked.borrow().len(), 3);
}

#[gpui_kit::test]
async fn dropping_a_folder_says_where_folders_go_and_opens_nothing(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    let asked = collect_opens(cx, &view);
    drop_files(
        cx,
        handle,
        "text-names-left",
        vec![dir.path().to_path_buf()],
    );
    cx.run_until_parked();
    assert!(asked.borrow().is_empty());
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("Drop files here; folders open in a folder compare.")
    );
}

fn run_path_item(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
    left: bool,
    label: &str,
) -> bool {
    let entries = view.read_with(cx, |v, _| duet_app::text::menu::path_menu(v, left));
    let view = view.clone();
    cx.update_window(handle, |_, window, cx| {
        let focus = view.focus_handle(cx);
        duet_app::ui::menu::activate(entries, label, Some(&focus), window, cx)
    })
    .unwrap()
}

#[gpui_kit::test]
async fn save_as_writes_the_side_and_the_side_follows_the_new_file(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\r\n", "b\r\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    view.update(cx, |v, cx| {
        v.edit(0..0, "z", cx);
    });
    let target = dir.path().join("saved.txt");
    let done = cx
        .update_window(handle, |_, window, cx| {
            let target = target.clone();
            view.update(cx, |v, cx| v.save_side_as(true, target, window, cx))
        })
        .unwrap();
    assert!(done.await);
    assert_eq!(std::fs::read(&target).unwrap(), b"za\r\n");
    let (path, dirty, request) = view.read_with(cx, |v, _| {
        (
            v.model().unwrap().left().path().to_path_buf(),
            v.model().unwrap().is_dirty(true),
            v.request().left.clone(),
        )
    });
    assert_eq!((path, dirty, request), (target.clone(), false, target));
    assert_eq!(
        std::fs::read(dir.path().join("a.txt")).unwrap(),
        b"a\r\n",
        "the old file is untouched"
    );
}

#[gpui_kit::test]
async fn the_path_menu_follows_the_side(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    for left in [true, false] {
        let items = view.read_with(cx, |v, _| {
            duet_app::ui::menu::outline(&duet_app::text::menu::path_menu(v, left))
        });
        assert_eq!(
            items,
            [
                "Open file…",
                "Paste from clipboard (replace this side)",
                "-",
                "(Save)",
                "Save as…",
                "-",
                "Copy path",
                "Show in Explorer",
                "Open with default app",
                "-",
                "Open in hex",
                "Open as table",
                "Open as picture"
            ]
        );
    }
}

#[gpui_kit::test]
async fn the_path_menu_enables_save_for_an_edited_side_only(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    view.update(cx, |v, cx| {
        v.edit(0..0, "z", cx);
    });
    let outline = |cx: &mut TestAppContext, left| {
        view.read_with(cx, |v, _| {
            duet_app::ui::menu::outline(&duet_app::text::menu::path_menu(v, left))
        })
    };
    assert_eq!(outline(cx, true)[3], "Save");
    assert_eq!(outline(cx, false)[3], "(Save)");
}

#[gpui_kit::test]
async fn path_menu_copy_path_and_save_run_through_the_menu(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    assert!(run_path_item(cx, handle, &view, false, "Copy path"));
    let copied = cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(
        copied.as_deref(),
        Some(dir.path().join("b.txt").to_str().unwrap())
    );
    assert!(!run_path_item(cx, handle, &view, true, "Save"), "clean");
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    view.update(cx, |v, cx| {
        v.edit(0..0, "z", cx);
    });
    assert!(run_path_item(cx, handle, &view, true, "Save"));
    for _ in 0..200 {
        if std::fs::read_to_string(dir.path().join("a.txt")).unwrap() == "za\n" {
            break;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "za\n"
    );
}

#[gpui_kit::test]
async fn path_menu_paste_replaces_only_its_own_side(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("clip\n".into()));
    assert!(run_path_item(
        cx,
        handle,
        &view,
        false,
        "Paste from clipboard (replace this side)"
    ));
    let (l, r) = view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (
            m.buffer(true).is_some(),
            m.buffer(false).map(|b| b.text().to_owned()),
        )
    });
    assert!(!l);
    assert_eq!(r.as_deref(), Some("clip\n"));
}

#[gpui_kit::test]
async fn path_menu_reveal_and_open_are_disabled_for_a_missing_file(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    std::fs::remove_file(dir.path().join("a.txt")).unwrap();
    let items = view.read_with(cx, |v, _| {
        duet_app::ui::menu::outline(&duet_app::text::menu::path_menu(v, true))
    });
    assert_eq!(
        &items[6..],
        [
            "Copy path",
            "(Show in Explorer)",
            "(Open with default app)",
            "-",
            "(Open in hex)",
            "(Open as table)",
            "(Open as picture)"
        ]
    );
}

fn save_as(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
    left: bool,
    path: std::path::PathBuf,
) -> gpui_kit::Task<bool> {
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.save_side_as(left, path, window, cx))
    })
    .unwrap()
}

fn notice(cx: &mut TestAppContext, view: &Entity<TextView>) -> Option<String> {
    view.read_with(cx, |v, _| v.notice().map(str::to_owned))
}

fn dirty_left(cx: &mut TestAppContext, view: &Entity<TextView>) {
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    view.update(cx, |v, cx| {
        v.edit(0..0, "z", cx);
    });
}

#[gpui_kit::test]
async fn save_as_that_lands_after_the_side_changed_renames_nothing(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    dirty_left(cx, &view);
    let other = dir.path().join("other.txt");
    std::fs::write(&other, "o\n").unwrap();
    let target = dir.path().join("saved.txt");
    let done = save_as(cx, handle, &view, true, target.clone());
    let b = dir.path().join("b.txt");
    view.update(cx, |v, cx| v.load_paths(other.clone(), b, cx));
    assert!(!done.await);
    assert_eq!(view.read_with(cx, |v, _| v.request().left.clone()), other);
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Saved saved.txt, but this side now shows another file.")
    );
}

#[gpui_kit::test]
async fn save_as_onto_the_other_sides_file_is_refused(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    dirty_left(cx, &view);
    let b = dir.path().join("b.txt");
    assert!(!save_as(cx, handle, &view, true, b.clone()).await);
    assert_eq!(std::fs::read(&b).unwrap(), b"b\n");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("That file is open on the other side. Pick another name.")
    );
}

#[cfg(windows)]
#[gpui_kit::test]
async fn save_as_onto_the_other_sides_file_in_another_case_is_refused(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    dirty_left(cx, &view);
    let b = dir.path().join("b.txt");
    assert!(!save_as(cx, handle, &view, true, dir.path().join("B.TXT")).await);
    assert_eq!(std::fs::read(&b).unwrap(), b"b\n");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("That file is open on the other side. Pick another name.")
    );
}

#[gpui_kit::test]
async fn save_as_waits_for_a_running_compare_and_a_running_save(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    dirty_left(cx, &view);
    let target = dir.path().join("q.txt");
    let first = save_as(cx, handle, &view, true, target.clone());
    assert!(!save_as(cx, handle, &view, true, target.clone()).await);
    assert_eq!(notice(cx, &view).as_deref(), Some("Still saving q.txt."));
    assert!(first.await);
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    view.update(cx, |v, cx| v.load_paths(a, b, cx));
    assert!(!save_as(cx, handle, &view, true, dir.path().join("r.txt")).await);
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Wait for the compare to finish.")
    );
}

#[gpui_kit::test]
async fn a_lossy_side_refuses_save_as(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text_bytes(cx, b"abc\xFFdef\n", b"b\n");
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.set_encoding(true, Some(encoding_rs::UTF_8), window, cx)
        })
    })
    .unwrap();
    wait_loaded(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.model().unwrap().left().is_lossy()));
    let target = dir.path().join("q.txt");
    assert!(!save_as(cx, handle, &view, true, target.clone()).await);
    assert!(!target.exists());
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("This file did not decode cleanly, so it can't be saved as a new file.")
    );
}

#[gpui_kit::test]
async fn after_save_as_ctrl_s_writes_the_new_file_and_reload_reads_it(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    dirty_left(cx, &view);
    let q = dir.path().join("q.txt");
    assert!(save_as(cx, handle, &view, true, q.clone()).await);
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    cx.update_window(handle, |_, window, cx| window.input("y", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    for _ in 0..200 {
        if std::fs::read_to_string(&q).unwrap() == "yza\n" {
            break;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&q).unwrap(), "yza\n");
    assert_eq!(std::fs::read(dir.path().join("a.txt")).unwrap(), b"a\n");
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.model().unwrap().left().path().to_path_buf()),
        q
    );
    assert_eq!(
        view.read_with(cx, |v, _| v
            .model()
            .unwrap()
            .left()
            .text()
            .map(str::to_owned)),
        Some("yza\n".into())
    );
}

#[gpui_kit::test]
async fn undo_still_works_after_save_as(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    dirty_left(cx, &view);
    assert!(save_as(cx, handle, &view, true, dir.path().join("q.txt")).await);
    press(cx, handle, "ctrl-z");
    assert_eq!(left_text(cx, &view), "a\n");
}

#[cfg(windows)]
#[gpui_kit::test]
async fn typed_paths_are_normalised(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "b\n");
    wait_loaded(cx, handle, &view).await;
    std::fs::create_dir_all(dir.path().join("sub")).unwrap();
    std::fs::create_dir_all(dir.path().join("other")).unwrap();
    std::fs::write(dir.path().join("sub").join("c.txt"), "c\n").unwrap();
    std::fs::write(dir.path().join("c.txt"), "c\n").unwrap();
    let root = dir.path().to_str().unwrap().to_owned();
    let asked = collect_opens(cx, &view);
    for (typed, want) in [
        (
            format!("{}/sub/c.txt", root.replace('\\', "/")),
            format!("{root}\\sub\\c.txt"),
        ),
        (
            format!("{root}\\other\\..\\c.txt"),
            format!("{root}\\c.txt"),
        ),
    ] {
        cx.update_window(handle, |_, window, cx| window.click("text-left-path", cx))
            .unwrap();
        view.update(cx, |v, cx| v.set_path_edit_text_for_test(&typed, cx));
        press(cx, handle, "enter");
        cx.run_until_parked();
        let got = asked
            .borrow()
            .last()
            .map(|(l, _)| l.to_str().unwrap().to_owned());
        assert_eq!(got, Some(want), "typed {typed}");
    }
}

#[cfg(windows)]
#[gpui_kit::test]
async fn ctrl_s_during_a_save_as_of_the_same_file_spelled_differently_waits(
    cx: &mut TestAppContext,
) {
    // One file, opened on both sides under two spellings of its name.
    let (dir, handle, view) = open_files(
        cx,
        (Path::new("a.txt"), b"a\n"),
        (Path::new("A.TXT"), b"a\n"),
    );
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    view.update(cx, |v, cx| v.set_caret_for_test(false, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    // The UI thread only compares spellings; the disk-resolving check belongs to the job, so
    // the refusal cannot be there yet.
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned)),
        None,
        "the same-file check must not run on the UI thread"
    );
    for _ in 0..300 {
        if std::fs::read_to_string(dir.path().join("a.txt")).unwrap() == "ax\n"
            && view.read_with(cx, |v, _| v.notice().is_some())
        {
            break;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
            .as_deref(),
        Some("Still saving A.TXT."),
        "the other spelling of a file being written waits"
    );
    assert!(!cx.has_pending_prompt(), "{:?}", cx.pending_prompt());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "ax\n"
    );
}

#[gpui_kit::test]
async fn inserting_after_an_unterminated_last_line_undoes_exactly(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc", "a\nb");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.select_rows_for_test(false, 2, 2, cx));
    press(cx, handle, "f2");
    assert_eq!(right_text(cx, &view), "a\nb\n\n");
    press(cx, handle, "ctrl-z");
    assert_eq!(right_text(cx, &view).as_bytes(), b"a\nb");
}
