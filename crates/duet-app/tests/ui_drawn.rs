//! UI tests that lay text out with the real DirectWrite text system, as the window does. The
//! other UI tests run on gpui's no-op text system, which gives every character the same width
//! and never fails, so a value DirectWrite draws as nothing passed them (T-43, wave 2).
#![cfg(windows)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use duet_app::table::view::{DETAIL_WINDOW_CHARS, DetailSide, TableView};
use duet_app::ui::fonts::{CODE_SIZE, Fonts};
use gpui_kit::test::{TestAppContextExt as _, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, Focusable as _, ForegroundExecutor,
    Pixels, Point, Render, TestAppContext, TestDispatcher, TextRun, Window, WindowBounds,
    WindowOptions, hsla, px, size,
};

/// A test context whose windows shape text with DirectWrite. `current_platform(true)` would
/// give the no-op text system; `false` builds the real one without opening a window.
fn directwrite_cx() -> (TestAppContext, ForegroundExecutor) {
    let text_system = gpui_kit::platform::current_platform(false).text_system();
    let dispatcher = TestDispatcher::new(0);
    let cx = TestAppContext::build_with_text_system(dispatcher.clone(), None, text_system);
    (cx, ForegroundExecutor::new(Arc::new(dispatcher)))
}

fn finish(cx: TestAppContext) {
    cx.run_until_parked();
    cx.update(|cx| cx.quit());
    cx.run_until_parked();
}

fn open_window<V: Render>(
    cx: &mut TestAppContext,
    (width, height): (f32, f32),
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        duet_app::app::configure_theme(gpui_kit::component::ThemeMode::Light, cx);
        cx.bind_keys(duet_app::app::key_bindings());
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(width), px(height)),
                })),
                ..Default::default()
            },
            cx,
            build,
        )
        .expect("open test window")
    })
}

fn bounds_of(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> Bounds<Pixels> {
    cx.update_window(handle, |_, window, _| window.find(id).bounds())
        .unwrap()
}

// ---- T-43: the detail panel on a 100 KB cell ----

/// The runs the panel draws a side with: the code font, a background on each mark.
fn detail_runs(side: &DetailSide, font: &gpui_kit::Font) -> Vec<TextRun> {
    let run = |len: usize, marked: bool| TextRun {
        len,
        font: font.clone(),
        color: hsla(0., 0., 0., 1.),
        background_color: marked.then(|| hsla(0., 0.8, 0.8, 1.)),
        underline: None,
        strikethrough: None,
    };
    let mut runs = Vec::new();
    let mut at = 0;
    for mark in &side.marks {
        if at < mark.start {
            runs.push(run(mark.start - at, false));
        }
        runs.push(run(mark.len(), true));
        at = mark.end;
    }
    if at < side.text.len() {
        runs.push(run(side.text.len() - at, false));
    }
    runs
}

#[test]
fn the_100_kb_cell_draws_its_changed_character_in_view() {
    // The bigcell fixture: one 100,000-character cell, character 99,991 changed.
    let (mut cx, exec) = directwrite_cx();
    let cell = "a".repeat(100_000);
    let mut changed = cell.clone();
    changed.replace_range(99_990..99_991, "b");
    let left = format!("id,big\r\n1,{cell}\r\n2,small\r\n");
    let right = format!("id,big\r\n1,{changed}\r\n2,small\r\n");
    let (handle, view) = open_window(&mut cx, (1400., 760.), move |window, cx| {
        let view = cx.new(|cx| TableView::with_text_for_test(&left, &right, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    let loaded = view.clone();
    exec.block_test(cx.wait_for(handle, Duration::from_secs(30), move |_, cx| {
        !loaded.read(cx).is_busy()
    }));
    let draw = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
    };
    draw(&mut cx);
    // F8, as the user goes to the difference: it selects the big cell.
    cx.update_window(handle, |_, window, cx| window.press("f8", cx))
        .unwrap();
    let started = Instant::now();
    draw(&mut cx);
    let first_frame = started.elapsed();
    draw(&mut cx);
    assert_eq!(
        view.read_with(&cx, |v, _| v.selection()),
        Some((0, Some(1)))
    );
    let detail = view.read_with(&cx, |v, _| v.detail_for_test()).unwrap();
    let font = cx.update(|cx| Fonts::current(cx).code);
    let panel = bounds_of(&mut cx, handle, "table-detail");
    for (side, id, want) in [
        (&detail.left, "table-detail-left", "a"),
        (&detail.right, "table-detail-right", "b"),
    ] {
        let text = bounds_of(&mut cx, handle, id);
        // What the panel draws, shaped as the panel shapes it, at the width it has.
        let lines = cx
            .update_window(handle, |_, window, _| {
                window.text_system().shape_text(
                    side.text.clone().into(),
                    px(CODE_SIZE),
                    &detail_runs(side, &font),
                    Some(text.size.width),
                    None,
                )
            })
            .unwrap()
            .unwrap();
        let glyphs: usize = lines
            .iter()
            .flat_map(|line| &line.unwrapped_layout.runs)
            .map(|run| run.glyphs.len())
            .sum();
        assert!(
            glyphs > 0,
            "{id}: DirectWrite lays out nothing for {} characters with marks {:?}",
            side.text.chars().count(),
            side.marks
        );
        let mark = side.marks.first().expect("the change is marked").clone();
        assert_eq!(
            &side.text[mark.clone()],
            want,
            "{id}: the changed character"
        );
        assert!(
            side.text.chars().count() <= DETAIL_WINDOW_CHARS + 2,
            "{id}: the panel shows a window, not {} characters",
            side.text.chars().count()
        );
        assert_eq!(
            side.note.as_deref(),
            Some("characters 99,921–100,000 of 100,000")
        );
        // The change is on screen without scrolling the panel.
        let rows: usize = lines.iter().map(|l| l.wrap_boundaries.len() + 1).sum();
        let line_height = text.size.height / rows as f32;
        let at = lines[0]
            .position_for_index(mark.start, line_height)
            .expect("the mark is laid out");
        let (x, y) = (text.left() + at.x, text.top() + at.y);
        assert!(
            y >= panel.top() && y + line_height <= panel.bottom() && x < panel.right(),
            "{id}: the change at ({x:?}, {y:?}) is outside the panel {panel:?}"
        );
    }
    eprintln!("the first frame with the big cell selected took {first_frame:?}");
    finish(cx);
}

// ---- H9: a Thai help topic in a narrow window ----

/// The help in Thai at 900 px: every piece of one long Thai topic is laid out by DirectWrite
/// with glyphs (the T-43 lesson) and sits inside the topic pane, so no row runs past it.
#[test]
fn a_thai_help_topic_draws_every_piece_inside_a_narrow_pane() {
    use duet_app::help::HelpView;
    use duet_app::help::topics::{self, HelpLang, TOPICS};
    let (mut cx, _exec) = directwrite_cx();
    let (handle, view) = open_window(&mut cx, (900., 700.), |window, cx| {
        let view = cx.new(|cx| HelpView::new(window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    assert_eq!(view.read_with(&cx, |v, _| v.lang()), HelpLang::Th);
    assert!(view.update(&mut cx, |v, cx| v.go_to("folder", cx)));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let pane = bounds_of(&mut cx, handle, "help-topic");
    assert!(
        pane.size.width < px(700.),
        "a narrow pane beside the topic list: {pane:?}"
    );
    let topic = TOPICS.iter().find(|t| t.id == "folder").unwrap();
    let pieces = topics::topic_pieces(topic, HelpLang::Th);
    assert!(pieces.len() > 50, "a long topic: {} pieces", pieces.len());
    let font = cx.update(|cx| Fonts::current(cx).ui);
    let run = |len: usize| TextRun {
        len,
        font: font.clone(),
        color: hsla(0., 0., 0., 1.),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    for (i, piece) in pieces.iter().enumerate() {
        let drawn = cx
            .update_window(handle, |_, window, _| {
                window.find(("help-piece", i)).bounds()
            })
            .unwrap();
        assert!(
            drawn.size.width > px(0.),
            "piece {i} {piece:?} has no width: {drawn:?}"
        );
        assert!(
            drawn.left() >= pane.left() && drawn.right() <= pane.right(),
            "piece {i} {piece:?} at {drawn:?} runs past the pane {pane:?}"
        );
        // What the reader sees: a chip's or a key's text without its brackets (U8).
        let seen = duet_app::help::markup::visible(piece);
        let layout = cx
            .update_window(handle, |_, window, _| {
                window.text_system().layout_line(
                    &seen,
                    px(duet_app::help::BODY_SIZE),
                    &[run(seen.len())],
                    None,
                )
            })
            .unwrap();
        let glyphs: usize = layout.runs.iter().map(|r| r.glyphs.len()).sum();
        assert!(glyphs > 0, "DirectWrite lays out nothing for {piece:?}");
        assert!(
            layout.width < pane.size.width,
            "{piece:?} is wider than the pane"
        );
    }
    let extra = cx
        .update_window(handle, |_, window, _| {
            window.try_find(("help-piece", pieces.len())).is_some()
        })
        .unwrap();
    assert!(!extra, "the page draws exactly the topic's pieces");
    finish(cx);
}

/// U11: in Thai at 900 px, every chip and key of a topic is drawn by DirectWrite with its text
/// inside it and stays inside the topic pane, and the language switch's two segments split
/// its box evenly.
#[test]
fn a_thai_topic_draws_its_chips_and_keys_and_the_switch_fills_its_box() {
    use duet_app::help::markup::{self, Span};
    use duet_app::help::topics::{self, HelpLang, TOPICS};
    use duet_app::help::{CHIP_SIZE, HelpView};
    let (mut cx, _exec) = directwrite_cx();
    let (handle, view) = open_window(&mut cx, (900., 700.), |window, cx| {
        let view = cx.new(|cx| HelpView::new(window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    assert_eq!(view.read_with(&cx, |v, _| v.lang()), HelpLang::Th);
    assert!(view.update(&mut cx, |v, cx| v.go_to("ops", cx)));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let pane = bounds_of(&mut cx, handle, "help-topic");
    let topic = TOPICS.iter().find(|t| t.id == "ops").unwrap();
    let marks: Vec<Span> = topics::topic_pieces(topic, HelpLang::Th)
        .into_iter()
        .flat_map(markup::marks)
        .collect();
    let chips: Vec<&str> = marks
        .iter()
        .filter_map(|m| match m {
            Span::Chip(label) => Some(*label),
            _ => None,
        })
        .collect();
    let keys: Vec<&str> = marks
        .iter()
        .filter_map(|m| match m {
            Span::Key(keys) => Some(*keys),
            _ => None,
        })
        .collect();
    assert!(
        chips.len() > 10 && keys.len() > 2,
        "{} chips, {} keys",
        chips.len(),
        keys.len()
    );
    let (ui, code) = cx.update(|cx| {
        let fonts = Fonts::current(cx);
        (fonts.ui, fonts.code)
    });
    let shaped_width = |cx: &mut TestAppContext, text: &str, font: &gpui_kit::Font, size: f32| {
        let run = TextRun {
            len: text.len(),
            font: font.clone(),
            color: hsla(0., 0., 0., 1.),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let layout = cx
            .update_window(handle, |_, window, _| {
                window
                    .text_system()
                    .layout_line(text, px(size), &[run], None)
            })
            .unwrap();
        let glyphs: usize = layout.runs.iter().map(|r| r.glyphs.len()).sum();
        assert!(glyphs > 0, "DirectWrite lays out nothing for {text:?}");
        layout.width
    };
    for (kind, texts, font, size) in [
        ("help-chip", &chips, &ui, CHIP_SIZE),
        ("help-key", &keys, &code, 11.),
    ] {
        for (i, text) in texts.iter().enumerate() {
            let drawn = cx
                .update_window(handle, |_, window, _| window.find((kind, i)).bounds())
                .unwrap();
            let width = shaped_width(&mut cx, text, font, size);
            assert!(
                drawn.size.width > width && width > px(0.),
                "{kind} {i} {text:?} is {drawn:?}, its text {width:?} wide"
            );
            assert!(
                drawn.left() >= pane.left() && drawn.right() <= pane.right(),
                "{kind} {i} {text:?} at {drawn:?} runs past the pane {pane:?}"
            );
        }
        let extra = cx
            .update_window(handle, |_, window, _| {
                window.try_find((kind, texts.len())).is_some()
            })
            .unwrap();
        assert!(!extra, "the page draws exactly the topic's {kind}s");
    }
    // U7: the two segments split the switch, inside its 1 px border.
    let (frame, thai, english) = cx
        .update_window(handle, |_, window, _| {
            (
                window.find("help-lang").bounds(),
                window.find(("help-lang", 0usize)).bounds(),
                window.find(("help-lang", 1usize)).bounds(),
            )
        })
        .unwrap();
    let near = |a: Pixels, b: Pixels, within: f32| (a - b).abs() <= px(within);
    assert!(
        near(thai.size.width, english.size.width, 1.),
        "equal segments: {thai:?} {english:?}"
    );
    assert!(
        near(thai.left(), frame.left(), 1.) && near(english.right(), frame.right(), 1.),
        "the segments fill the switch {frame:?}: {thai:?} {english:?}"
    );
    assert!(near(thai.right(), english.left(), 0.01), "side by side");
    finish(cx);
}

/// The topic list is one left-aligned column: a title that wraps in the 240 px list starts
/// where the one-row titles start, and its entry holds both rows (real-window check HV 4,
/// where the one-row titles sat centred beside it).
#[test]
fn the_topic_list_is_left_aligned_and_a_wrapped_entry_holds_its_rows() {
    use duet_app::help::HelpView;
    use duet_app::help::topics::{HelpLang, TOPICS};
    let (mut cx, _exec) = directwrite_cx();
    let (handle, view) = open_window(&mut cx, (900., 700.), |window, cx| {
        let view = cx.new(|cx| HelpView::new(window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.set_lang(HelpLang::En, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    let entry = |cx: &mut TestAppContext, id: &str| {
        let i = TOPICS.iter().position(|t| t.id == id).unwrap();
        cx.update_window(handle, |_, window, _| {
            (
                window.find(("help-toc", i)).bounds(),
                window.find(("help-toc-label", i)).bounds(),
            )
        })
        .unwrap()
    };
    let (one_row, one_row_label) = entry(&mut cx, "text");
    let (wrapped, wrapped_label) = entry(&mut cx, "rules");
    assert!(
        wrapped_label.size.height > one_row_label.size.height + px(10.),
        "\"Ignoring unimportant differences\" wraps to two rows: {wrapped_label:?}"
    );
    assert_eq!(
        one_row_label.left(),
        wrapped_label.left(),
        "every title starts at the list's left padding"
    );
    for (entry, label) in [(one_row, one_row_label), (wrapped, wrapped_label)] {
        assert!(
            label.top() >= entry.top() && label.bottom() <= entry.bottom(),
            "the title {label:?} sits inside its entry {entry:?}"
        );
    }
    finish(cx);
}

// ---- U4: a dropdown's label is the size of its row's ----

/// A text compare of two one-line files, loaded, at `width` x 760.
fn open_text_pair(
    cx: &mut TestAppContext,
    exec: &ForegroundExecutor,
    width: f32,
) -> (
    tempfile::TempDir,
    AnyWindowHandle,
    Entity<duet_app::text::view::TextView>,
) {
    use duet_app::text::view::TextView;
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "a\n").unwrap();
    std::fs::write(&b, "b\n").unwrap();
    let (handle, view) = open_window(cx, (width, 760.), move |window, cx| {
        let view = cx.new(|cx| TextView::new(a, b, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    let loaded = view.clone();
    exec.block_test(cx.wait_for(handle, Duration::from_secs(30), move |_, cx| {
        !loaded.read(cx).is_busy()
    }));
    (dir, handle, view)
}

/// In the text view's sub-bar, the whitespace dropdown's label is as tall as the Case toggle's
/// beside it: the same text size and line height. gpui-kit's small Button drew 14 px text in
/// the row's 12 px.
#[test]
fn a_dropdown_label_is_as_tall_as_its_toggle_neighbour() {
    use duet_app::ui::style::LABEL_ID;
    let (mut cx, exec) = directwrite_cx();
    let (_dir, handle, _view) = open_text_pair(&mut cx, &exec, 1400.);
    let label = |cx: &mut TestAppContext, control: &'static str| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.within(control).find(LABEL_ID).bounds()
        })
        .unwrap()
    };
    let dropdown = label(&mut cx, "text-ignore-ws");
    let toggle = label(&mut cx, "text-ignore-case");
    assert!(toggle.size.height > px(0.), "{toggle:?}");
    assert_eq!(
        dropdown.size.height, toggle.size.height,
        "the dropdown's label {dropdown:?} and the toggle's {toggle:?}"
    );
    finish(cx);
}

// ---- U15: the Thai welcome card ----

/// The Start tab's welcome card in Thai at 900 px is drawn as the help draws its prose (H6):
/// every piece of its title and lines is laid out with a width, inside the card, and F1 is a
/// key cap.
#[test]
fn the_thai_welcome_card_draws_every_piece_inside_it() {
    use duet_app::help::topics::{self, HelpLang};
    use duet_app::start::StartView;
    let (mut cx, _exec) = directwrite_cx();
    let (handle, view) = open_window(&mut cx, (900., 700.), |window, cx| {
        let view = cx.new(|cx| StartView::new(None, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    assert_eq!(view.read_with(&cx, |v, _| v.welcome()), Some(HelpLang::Th));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let card = bounds_of(&mut cx, handle, "start-welcome");
    let copy = topics::welcome_copy(HelpLang::Th);
    let pieces: Vec<&str> = [copy.title]
        .into_iter()
        .chain(copy.lines)
        .flat_map(topics::pieces)
        .collect();
    for (i, piece) in pieces.iter().enumerate() {
        let drawn = cx
            .update_window(handle, |_, window, _| {
                window.find(("help-piece", i)).bounds()
            })
            .unwrap();
        assert!(
            drawn.size.width > px(0.)
                && drawn.left() >= card.left()
                && drawn.right() <= card.right(),
            "piece {i} {piece:?} at {drawn:?} in the card {card:?}"
        );
    }
    let key = cx
        .update_window(handle, |_, window, _| {
            window.find(("help-key", 0usize)).bounds()
        })
        .unwrap();
    assert!(key.size.width > px(0.), "F1 is a key cap: {key:?}");
    finish(cx);
}

// ---- U16 / U17: the toolbar rows at 1400 px ----

/// Asserts that every control of `ids` is drawn inside `row` on one line: each one's middle is
/// within a pixel of the first one's.
fn on_one_line(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    row: &'static str,
    ids: &[&'static str],
) {
    cx.update_window(handle, |_, window, _| {
        let row_id = gpui_kit::ElementId::from(row);
        let first = window.find(ids[0]).bounds();
        for id in ids {
            let found = window.find(*id);
            assert!(
                found.path().contains(&row_id),
                "{id} is in {row}: {:?}",
                found.path()
            );
            let b = found.bounds();
            assert!(
                (b.center().y - first.center().y).abs() <= px(1.),
                "{id} at {b:?} is not on the line of {} at {first:?}",
                ids[0]
            );
        }
    })
    .unwrap();
}

/// U16: at 1400 px the text view's sub-bar is one line. Each side's encoding is its name
/// bar's info, and "On save" and the font size end the main row, before find and go to.
#[test]
fn the_text_view_has_two_toolbar_rows_at_1400_px() {
    let (mut cx, exec) = directwrite_cx();
    let (_dir, handle, _view) = open_text_pair(&mut cx, &exec, 1400.);
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    on_one_line(
        &mut cx,
        handle,
        "text-subbar",
        &[
            "text-filter",
            "text-ignore-ws",
            "text-ignore-case",
            "text-ignore-eol",
            "text-ignore-blank",
            "text-ignore-comments",
            "text-rules-open",
            "text-moves",
            "text-syntax",
            "text-language",
            "text-show-ws",
            "text-wrap",
            "text-details",
            "text-tab",
        ],
    );
    on_one_line(
        &mut cx,
        handle,
        "text-toolbar",
        &[
            "text-swap",
            "text-reload",
            "text-next",
            "text-report",
            "text-save-options",
            "text-font",
            "text-find-open",
            "text-goto-open",
        ],
    );
    let (save, font, find) = cx
        .update_window(handle, |_, window, _| {
            (
                window.find("text-save-options").bounds(),
                window.find("text-font").bounds(),
                window.find("text-find-open").bounds(),
            )
        })
        .unwrap();
    assert!(
        save.right() <= font.left() && font.right() <= find.left(),
        "On save, then the font, then find: {save:?} {font:?} {find:?}"
    );
    for (control, names) in [
        ("text-enc-left", "text-names-left"),
        ("text-enc-right", "text-names-right"),
    ] {
        on_one_line(&mut cx, handle, names, &[control]);
    }
    finish(cx);
}

/// U17: at 1400 px the folder view's toolbar is three rows: the paths, the criterion row with
/// "Filter paths" after Exclude and before Options, and the chips with the file actions.
#[test]
fn the_folder_view_has_three_toolbar_rows_at_1400_px() {
    use duet_app::folder::view::FolderView;
    let (mut cx, exec) = directwrite_cx();
    let dir = tempfile::tempdir().unwrap();
    let (left, right) = (dir.path().join("l"), dir.path().join("r"));
    for side in [&left, &right] {
        std::fs::create_dir_all(side).unwrap();
        std::fs::write(side.join("a.txt"), side.display().to_string()).unwrap();
    }
    let (handle, view) = open_window(&mut cx, (1400., 760.), move |window, cx| {
        let options = duet_core::folder::run::FolderOptions::default();
        let view = cx.new(|cx| FolderView::new(left, right, options, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    let loaded = view.clone();
    exec.block_test(cx.wait_for(handle, Duration::from_secs(30), move |_, cx| {
        !loaded.read(cx).is_busy()
    }));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    on_one_line(
        &mut cx,
        handle,
        "folder-options",
        &[
            "folder-mode",
            "folder-criterion-label",
            "folder-tolerance",
            "folder-dst",
            "folder-include",
            "folder-exclude",
            "folder-name-filter",
            "folder-options-menu",
        ],
    );
    on_one_line(
        &mut cx,
        handle,
        "folder-chips",
        &[
            "folder-chip-all",
            "folder-chip-same",
            "folder-layout",
            "folder-expand-all",
            "folder-legend",
            "folder-copy-right",
            "folder-delete",
            "folder-reveal",
            "folder-sync",
            "folder-report",
        ],
    );
    let (exclude, filter, options) = cx
        .update_window(handle, |_, window, _| {
            (
                window.find("folder-exclude").bounds(),
                window.find("folder-name-filter").bounds(),
                window.find("folder-options-menu").bounds(),
            )
        })
        .unwrap();
    assert!(
        exclude.right() <= filter.left() && filter.right() <= options.left(),
        "Exclude, then the filter, then Options: {exclude:?} {filter:?} {options:?}"
    );
    assert!(
        (filter.size.width - px(200.)).abs() <= px(1.),
        "about 200 px wide: {filter:?}"
    );
    finish(cx);
}

// ---- U6: an icon costs a frame no more than the glyph it replaced ----

/// Ten toolbar glyphs and the icons that replaced them.
const GLYPHS: [&str; 10] = ["⤒", "▲", "▼", "⤓", "↶", "↷", "⌕", "⧉", "⇄", "↻"];
const ICONS: [gpui_kit::assets::IconName; 10] = {
    use duet_app::ui::icons::*;
    [
        FIRST, PREV, NEXT, LAST, UNDO, REDO, FIND, COPY_PATH, SWAP, RELOAD,
    ]
};

/// Forty 28 px buttons: icon buttons, or buttons labelled with the glyphs.
struct ButtonRow {
    icons: bool,
}

impl Render for ButtonRow {
    fn render(
        &mut self,
        _: &mut Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        use duet_app::ui::style::{ld_button, ld_icon_button};
        use gpui_kit::{IntoElement as _, ParentElement as _, Styled as _};
        let p = duet_app::palette::Palette::current(cx);
        gpui_kit::div()
            .flex()
            .flex_wrap()
            .children((0..40usize).map(|i| {
                if self.icons {
                    ld_icon_button(("b", i), ICONS[i % 10], "Tip", &p).into_any_element()
                } else {
                    ld_button(("b", i), GLYPHS[i % 10], &p).into_any_element()
                }
            }))
    }
}

/// The frame times of forty icon buttons against forty glyph buttons, with DirectWrite. In
/// the window an icon is one sprite-atlas lookup a frame, keyed by its path and size: gpui
/// parses and rasterises the SVG only when that key is missing (`Window::paint_svg`,
/// `AtlasState::get_or_insert_with`), and the embedded bytes are borrowed, never copied. The
/// test app has no asset source, so here the lookup misses, the load finds nothing and no
/// sprite is queued: the times are the element, layout and lookup work an icon adds a frame.
/// Run by hand: `cargo test -p duet-app --test ui_drawn -- --ignored --nocapture bench_icon`.
#[test]
#[ignore = "a timing bench: run by hand"]
fn bench_icon_buttons_against_glyph_buttons() {
    let ms = |d: Duration| d.as_secs_f64() * 1000.;
    for icons in [false, true] {
        let (mut cx, _exec) = directwrite_cx();
        let (handle, _) = open_window(&mut cx, (1400., 200.), move |_, cx| {
            cx.new(|_| ButtonRow { icons })
        });
        let draw = |cx: &mut TestAppContext| {
            cx.update_window(handle, |_, window, cx| window.render_frame(cx))
                .unwrap();
        };
        let start = Instant::now();
        draw(&mut cx);
        let first = start.elapsed();
        let mut times: Vec<Duration> = (0..300)
            .map(|_| {
                let start = Instant::now();
                draw(&mut cx);
                start.elapsed()
            })
            .collect();
        times.sort();
        println!(
            "{}: first frame {:.2} ms; 300 frames median {:.3} ms, p95 {:.3} ms, max {:.3} ms",
            if icons {
                "40 icon buttons"
            } else {
                "40 glyph buttons"
            },
            ms(first),
            ms(times[times.len() / 2]),
            ms(times[times.len() * 95 / 100]),
            ms(times[times.len() - 1]),
        );
        finish(cx);
    }
}

// ---- X-18 / X-66: a resize drag at the wrap limit, shaped by DirectWrite ----

/// `rows` rows where every tenth is a line of three segments at `columns` columns.
fn every_tenth_tall(columns: usize, rows: usize) -> (String, String) {
    let long = "x".repeat(3 * columns - 5);
    let left: String = (0..rows)
        .map(|i| {
            if i % 10 == 0 {
                format!("{long}\n")
            } else {
                format!("line {i}\n")
            }
        })
        .collect();
    let right = left.replacen("line 5\n", "LINE 5\n", 1);
    (left, right)
}

/// The frame times of a 300 px edge drag at 200,000 rows, two frames a step as the window
/// draws them, with wrap on and off. Unlike the no-op text system, DirectWrite pays for every
/// row segment a new wrap makes. Run by hand: `cargo test -p duet-app --test ui_drawn --
/// --ignored --nocapture`.
#[test]
#[ignore = "a timing bench: run by hand"]
fn bench_a_resize_drag_with_directwrite() {
    use duet_app::text::view::TextView;
    let (mut cx, exec) = directwrite_cx();
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, "a\n").unwrap();
    std::fs::write(&b, "b\n").unwrap();
    let (paths_a, paths_b) = (a.clone(), b.clone());
    let (handle, view) = open_window(&mut cx, (1200., 800.), move |window, cx| {
        let view = cx.new(|cx| TextView::new(paths_a, paths_b, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    let draw = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
    };
    let wait = |cx: &mut TestAppContext| {
        let loaded = view.clone();
        exec.block_test(cx.wait_for(handle, Duration::from_secs(60), move |_, cx| {
            !loaded.read(cx).is_busy()
        }));
    };
    wait(&mut cx);
    draw(&mut cx);
    draw(&mut cx);
    let columns = view.read_with(&cx, |v, _| v.display_options().view_columns);
    let (left, right) = every_tenth_tall(columns, 200_000);
    std::fs::write(&a, left).unwrap();
    std::fs::write(&b, right).unwrap();
    cx.update_window(handle, |_, window, cx| window.press("f5", cx))
        .unwrap();
    wait(&mut cx);
    let ms = |d: Duration| d.as_secs_f64() * 1000.;
    for wrap in [true, false] {
        assert!(view.update(&mut cx, |v, cx| v.set_wrap(wrap, cx)));
        draw(&mut cx);
        draw(&mut cx);
        let built = view.read_with(&cx, |v, _| v.rows_built_for_test());
        let mut times = Vec::new();
        for step in 1..=100 {
            let width = 1200. - 3. * step as f32;
            cx.simulate_window_resize(handle, size(px(width), px(800.)));
            for _ in 0..2 {
                let start = Instant::now();
                draw(&mut cx);
                times.push(start.elapsed());
            }
        }
        let built = view.read_with(&cx, |v, _| v.rows_built_for_test()) - built;
        times.sort();
        println!(
            "wrap {}: 200 frames of a 300 px drag, DirectWrite (debug build): median {:.1} ms, p95 {:.1} ms, max {:.1} ms; {:.1} rows built a frame",
            if wrap { "on" } else { "off" },
            ms(times[times.len() / 2]),
            ms(times[times.len() * 95 / 100]),
            ms(times[times.len() - 1]),
            built as f64 / times.len() as f64,
        );
        cx.simulate_window_resize(handle, size(px(1200.), px(800.)));
        draw(&mut cx);
        cx.executor().advance_clock(Duration::from_millis(500));
        cx.run_until_parked();
        let start = Instant::now();
        draw(&mut cx);
        draw(&mut cx);
        println!(
            "wrap {wrap}: the settle frames took {:.1} ms",
            ms(start.elapsed())
        );
    }
    finish(cx);
}
