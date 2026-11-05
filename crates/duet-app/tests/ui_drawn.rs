//! UI tests that lay text out with the real DirectWrite text system, as the window does. The
//! other UI tests run on gpui's no-op text system, which gives every character the same width
//! and never fails, so a value DirectWrite draws as nothing passed them (T-43, wave 2).
#![cfg(windows)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use duet_app::i18n::tr;
use duet_app::table::view::{DETAIL_WINDOW_CHARS, DetailSide, TableView};
use duet_app::ui::fonts::{CODE_SIZE, Fonts};
use gpui_kit::test::{TestAppContextExt as _, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, Focusable as _, ForegroundExecutor,
    Pixels, Point, Render, TestAppContext, TestDispatcher, TextRun, Window, WindowBounds,
    WindowOptions, hsla, px, size,
};

/// Help's language follows the UI's, and English is the default under test.
fn thai_ui() -> duet_app::i18n::LangScope {
    duet_app::i18n::scoped(duet_app::i18n::Lang::Th)
}

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
    let _th = thai_ui();
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
    let _th = thai_ui();
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
            // A chip draws the label in the language in force (D7); a key cap is the key itself.
            let shown = if kind == "help-chip" { tr(text) } else { text };
            let width = shaped_width(&mut cx, shown, font, size);
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
    dropdown_and_toggle_labels();
}

/// The same in Thai, where the toggle and the dropdown both read Thai.
#[test]
fn a_thai_dropdown_label_is_as_tall_as_its_toggle_neighbour() {
    let _th = thai_ui();
    dropdown_and_toggle_labels();
}

fn dropdown_and_toggle_labels() {
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

/// The rules panel's two help paragraphs in Thai, at a width that makes them wrap: they break only
/// between phrases (every piece stays on one line) and stay inside their own box.
#[test]
fn the_rules_help_wraps_between_phrases_in_thai() {
    use duet_app::ui::prose::PIECE_ID;
    let _th = thai_ui();
    let (mut cx, exec) = directwrite_cx();
    let (_dir, handle, view) = open_text_pair(&mut cx, &exec, 700.);
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.open_rules(window, cx));
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    for (help, key) in [
        (
            "text-rules-text-help",
            "Text that matches is left out before comparing. Lines that differ only there show purple and do not count.",
        ),
        (
            "text-rules-lines-help",
            "Lines that match are paired as unimportant and do not count.",
        ),
    ] {
        let thai = duet_app::i18n::tr(key);
        assert_ne!(thai, key, "{help} has Thai");
        let (boxed, pieces) = cx
            .update_window(handle, |_, window, _| {
                let boxed = window.find(help).bounds();
                let scoped = window.within(help);
                let pieces: Vec<Bounds<Pixels>> = (0usize..)
                    .map_while(|i| scoped.try_find((PIECE_ID, i)).map(|f| f.bounds()))
                    .collect();
                (boxed, pieces)
            })
            .unwrap();
        assert_eq!(
            pieces.len(),
            thai.split_inclusive(' ').count(),
            "{help}: one piece after each space"
        );
        let line = pieces.iter().map(|p| p.size.height).min().unwrap();
        for (i, piece) in pieces.iter().enumerate() {
            assert_eq!(piece.size.height, line, "{help}: piece {i} on one line");
            assert!(
                piece.left() >= boxed.left() && piece.right() <= boxed.right(),
                "{help}: piece {i} {piece:?} runs past {boxed:?}"
            );
        }
    }
    finish(cx);
}

// ---- U15: the Thai welcome card ----

/// The Start tab's welcome card in Thai at 900 px is drawn as the help draws its prose (H6):
/// every piece of its title and lines is laid out with a width, inside the card, and F1 is a
/// key cap.
#[test]
fn the_thai_welcome_card_draws_every_piece_inside_it() {
    let _th = thai_ui();
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

// ---- Start tiles in Thai ----

/// Thai tiles at 1400 and 900 px: every tile's title and every piece of its line is laid out
/// by DirectWrite inside the tile, and the tile's text is not wider than the tile.
#[test]
fn the_thai_start_tiles_keep_their_text_inside_them() {
    let _th = thai_ui();
    use duet_app::start::StartView;
    use duet_app::ui::prose::PIECE_ID;
    const TILES: [&str; 6] = [
        "start-mode-files",
        "start-mode-folders",
        "start-mode-merge",
        "start-mode-hex",
        "start-mode-table",
        "start-mode-picture",
    ];
    for width in [1400., 900.] {
        let (mut cx, _exec) = directwrite_cx();
        let (handle, _view) = open_window(&mut cx, (width, 800.), |window, cx| {
            let view = cx.new(|cx| StartView::new(None, window, cx));
            window.focus(&view.focus_handle(cx), cx);
            view
        });
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        for id in TILES {
            let (tile, pieces) = cx
                .update_window(handle, |_, window, _| {
                    let tile = window.find(id).bounds();
                    let scoped = window.within(id);
                    let pieces: Vec<Bounds<Pixels>> = (0usize..)
                        .map_while(|i| scoped.try_find((PIECE_ID, i)).map(|f| f.bounds()))
                        .collect();
                    (tile, pieces)
                })
                .unwrap();
            assert!(tile.size.width > px(100.), "{id} at {width}: {tile:?}");
            assert!(
                pieces.len() >= 2,
                "{id} at {width} draws its line in pieces: {pieces:?}"
            );
            let line = pieces.iter().map(|p| p.size.height).min().unwrap();
            for (i, piece) in pieces.iter().enumerate() {
                assert_eq!(piece.size.height, line, "{id} piece {i} stays on one line");
                assert!(
                    piece.left() >= tile.left()
                        && piece.right() <= tile.right()
                        && piece.bottom() <= tile.bottom(),
                    "{id} piece {i} {piece:?} runs past its tile {tile:?} at {width} px"
                );
            }
        }
        finish(cx);
    }
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
    text_toolbar_rows();
}

/// The same rows with the UI in Thai: a label that grew must not push a control to a new line.
#[test]
fn the_text_view_has_two_toolbar_rows_at_1400_px_in_thai() {
    let _th = thai_ui();
    text_toolbar_rows();
}

fn text_toolbar_rows() {
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
            "text-sort-keys",
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
    folder_toolbar_rows();
}

/// The same rows with the UI in Thai: a label that grew must not push a control to a new line.
#[test]
fn the_folder_view_has_three_toolbar_rows_at_1400_px_in_thai() {
    let _th = thai_ui();
    folder_toolbar_rows();
}

fn folder_toolbar_rows() {
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
            "folder-chip-moved",
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

// ---- Plan B task 4: prose that wraps between phrases ----

/// A box of `width` px holding `.prose(text)`, and nothing else.
struct ProseBox {
    text: gpui_kit::SharedString,
    width: f32,
    mode: Mode,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// `.prose(text)`.
    Prose,
    /// `.child(text)`, what English `.prose` must equal.
    Plain,
    /// Drawn as a tooltip is, so its own width limit applies.
    Tooltip,
}

impl Render for ProseBox {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        use duet_app::ui::prose::ProseExt as _;
        use gpui_kit::prelude::*;
        use gpui_kit::{TestSupportExt as _, div};
        let inner = match self.mode {
            Mode::Tooltip => div()
                .child(duet_app::ui::style::ld_tip(self.text.clone(), window, cx))
                .into_any_element(),
            Mode::Prose => div()
                .id("prose-text")
                .test_support()
                .prose(self.text.clone())
                .into_any_element(),
            Mode::Plain => div()
                .id("prose-text")
                .test_support()
                .child(self.text.clone())
                .into_any_element(),
        };
        div()
            .id("prose-box")
            .test_support()
            .w(px(self.width))
            .child(inner)
    }
}

fn open_prose(cx: &mut TestAppContext, text: &str, width: f32, mode: Mode) -> AnyWindowHandle {
    let text: gpui_kit::SharedString = text.to_owned().into();
    let (handle, _view) = open_window(cx, (900., 700.), move |_, cx| {
        cx.new(|_| ProseBox { text, width, mode })
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    handle
}

/// The bounds of every piece the last frame drew, in order.
fn prose_pieces(cx: &mut TestAppContext, handle: AnyWindowHandle) -> Vec<Bounds<Pixels>> {
    cx.update_window(handle, |_, window, _| {
        (0usize..)
            .map_while(|i| {
                window
                    .try_find((duet_app::ui::prose::PIECE_ID, i))
                    .map(|found| found.bounds())
            })
            .collect()
    })
    .unwrap()
}

const THAI_SENTENCE: &str =
    "เปิดสองโฟลเดอร์จากแท็บเริ่มต้น แล้วเลือกจุดต่างที่ต้องการดู ปุ่มเทียบใหม่อยู่ด้านบนของหน้าต่าง ";

#[test]
fn thai_prose_breaks_only_between_pieces() {
    let _th = thai_ui();
    let (mut cx, _exec) = directwrite_cx();
    let text = THAI_SENTENCE.repeat(2);
    let handle = open_prose(&mut cx, &text, 420., Mode::Prose);
    let boxed = bounds_of(&mut cx, handle, "prose-box");
    let pieces = prose_pieces(&mut cx, handle);
    let expected = text.split_inclusive(' ').count();
    assert_eq!(pieces.len(), expected, "one piece after each space");
    let line = pieces.iter().map(|p| p.size.height).min().unwrap();
    for (i, piece) in pieces.iter().enumerate() {
        assert_eq!(piece.size.height, line, "piece {i} stays on one line");
        assert!(
            piece.left() >= boxed.left() && piece.right() <= boxed.right(),
            "piece {i} {piece:?} runs past the box {boxed:?}"
        );
    }
    let mut tops: Vec<_> = pieces.iter().map(|p| p.top()).collect();
    tops.dedup();
    assert!(tops.len() >= 3, "and the text wraps: {} lines", tops.len());
    finish(cx);
}

/// The sync panel's summary, as the panel would say it in Thai, drawn the way `.prose` draws it:
/// it breaks between phrases and keeps every piece inside its box.
#[test]
fn the_thai_sync_summary_wraps_between_phrases() {
    use duet_app::folder::sync_panel::{SyncScope, sync_summary};
    use duet_app::folder::view::FolderView;
    let _th = thai_ui();
    let (mut cx, exec) = directwrite_cx();
    let dir = tempfile::tempdir().unwrap();
    let (left, right) = (dir.path().join("l"), dir.path().join("r"));
    for side in [&left, &right] {
        std::fs::create_dir_all(side).unwrap();
    }
    std::fs::write(left.join("new.txt"), "new").unwrap();
    std::fs::write(right.join("newer.txt"), "old").unwrap();
    std::fs::write(left.join("newer.txt"), "newer than the right one").unwrap();
    let later = std::time::SystemTime::now() + Duration::from_secs(3600);
    std::fs::OpenOptions::new()
        .write(true)
        .open(left.join("newer.txt"))
        .unwrap()
        .set_modified(later)
        .unwrap();
    let (handle, view) = open_window(&mut cx, (900., 700.), move |window, cx| {
        let options = duet_core::folder::run::FolderOptions::default();
        cx.new(|cx| FolderView::new(left, right, options, window, cx))
    });
    let loaded = view.clone();
    exec.block_test(cx.wait_for(handle, Duration::from_secs(30), move |_, cx| {
        !loaded.read(cx).is_busy()
    }));
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.open_sync(SyncScope::All, window, cx))
    })
    .unwrap();
    let planned = view.clone();
    exec.block_test(cx.wait_for(handle, Duration::from_secs(30), move |_, cx| {
        planned.read(cx).sync_plan_shown().is_some()
    }));
    let text = view.read_with(&cx, |v, _| sync_summary(v.sync_plan_shown().unwrap()));
    assert!(!text.contains("Copy"), "in Thai: {text}");
    let probe = open_prose(&mut cx, &text, 360., Mode::Prose);
    let boxed = bounds_of(&mut cx, probe, "prose-box");
    let pieces = prose_pieces(&mut cx, probe);
    assert_eq!(
        pieces.len(),
        text.split_inclusive(' ').count(),
        "one piece after each space"
    );
    let line = pieces.iter().map(|p| p.size.height).min().unwrap();
    for (i, piece) in pieces.iter().enumerate() {
        assert_eq!(piece.size.height, line, "piece {i} stays on one line");
        assert!(
            piece.left() >= boxed.left() && piece.right() <= boxed.right(),
            "piece {i} {piece:?} runs past the box {boxed:?}"
        );
    }
    let mut tops: Vec<_> = pieces.iter().map(|p| p.top()).collect();
    tops.dedup();
    assert!(tops.len() >= 2, "and the text wraps: {} lines", tops.len());
    finish(cx);
}

#[test]
fn a_thai_message_with_a_long_path_stays_inside_its_box() {
    let _th = thai_ui();
    let (mut cx, _exec) = directwrite_cx();
    let path = ["C:\\", &"folder\\".repeat(30), "file.txt"].concat();
    assert!(path.len() > 200);
    let text = format!("เปิดไฟล์ {path} ไม่ได้ ไฟล์อาจถูกย้ายหรือลบแล้ว");
    let handle = open_prose(&mut cx, &text, 300., Mode::Prose);
    let boxed = bounds_of(&mut cx, handle, "prose-box");
    let pieces = prose_pieces(&mut cx, handle);
    assert!(pieces.len() >= 4);
    for (i, piece) in pieces.iter().enumerate() {
        assert!(
            piece.left() >= boxed.left() && piece.right() <= boxed.right(),
            "piece {i} {piece:?} runs past the box {boxed:?}"
        );
    }
    let text_box = bounds_of(&mut cx, handle, "prose-text");
    assert!(
        text_box.right() <= boxed.right(),
        "{text_box:?} in {boxed:?}"
    );
    finish(cx);
}

#[test]
fn a_thai_tooltip_stays_inside_its_width() {
    let _th = thai_ui();
    let (mut cx, _exec) = directwrite_cx();
    let path = ["C:\\", &"folder\\".repeat(40), "file.txt"].concat();
    let text = format!("{THAI_SENTENCE}{path} {THAI_SENTENCE}");
    let handle = open_prose(&mut cx, &text, 900., Mode::Tooltip);
    let tip = bounds_of(&mut cx, handle, "tooltip-text");
    assert!(
        tip.size.width <= px(duet_app::ui::style::TOOLTIP_MAX_WIDTH) + px(0.5),
        "the tooltip is {:?} wide",
        tip.size.width
    );
    let pieces = prose_pieces(&mut cx, handle);
    assert!(pieces.len() > 5);
    for (i, piece) in pieces.iter().enumerate() {
        assert!(
            piece.left() >= tip.left() && piece.right() <= tip.right() + px(0.5),
            "piece {i} {piece:?} runs past the tooltip {tip:?}"
        );
    }
    // A line break in the text: the second line starts a row of its own, in order, below the
    // first, and no piece is a two-line cell.
    let two_lines = "แถวแรก สั้น\nแถวสอง ต่อมา\nแถวสาม";
    let handle = open_prose(&mut cx, two_lines, 900., Mode::Tooltip);
    let pieces = prose_pieces(&mut cx, handle);
    assert_eq!(pieces.len(), 5, "one piece per word, none holding a break");
    let line = pieces.iter().map(|p| p.size.height).min().unwrap();
    for (i, piece) in pieces.iter().enumerate() {
        assert_eq!(piece.size.height, line, "piece {i} stays on one line");
    }
    assert_eq!(pieces[0].top(), pieces[1].top(), "line one");
    assert!(pieces[0].left() < pieces[1].left());
    assert!(pieces[2].top() >= pieces[1].bottom(), "line two is below");
    assert_eq!(pieces[2].top(), pieces[3].top(), "line two");
    assert!(pieces[2].left() < pieces[3].left());
    assert!(
        pieces[2].left() <= pieces[0].left() + px(0.5),
        "from the left"
    );
    assert!(pieces[4].top() >= pieces[3].bottom(), "line three is below");
    finish(cx);
}

#[test]
fn english_prose_is_one_text_child() {
    let (mut cx, _exec) = directwrite_cx();
    let text = "Open two folders from the Start tab, then pick the difference you want to see";
    let prose = open_prose(&mut cx, text, 200., Mode::Prose);
    let plain = open_prose(&mut cx, text, 200., Mode::Plain);
    assert!(
        prose_pieces(&mut cx, prose).is_empty(),
        "English draws no pieces"
    );
    let (prose, plain) = (
        bounds_of(&mut cx, prose, "prose-text"),
        bounds_of(&mut cx, plain, "prose-text"),
    );
    assert_eq!(prose, plain, "the same text laid out the same way");
    assert!(prose.size.height > px(30.), "and it wraps: {prose:?}");
    finish(cx);
}

// ---- The merge tab in Thai ----

/// A merge view over typed texts (one conflict) at `width`, loaded and drawn.
fn open_merge(
    cx: &mut TestAppContext,
    exec: &ForegroundExecutor,
    width: f32,
    merged: bool,
) -> (AnyWindowHandle, Entity<duet_app::merge::view::MergeView>) {
    use duet_app::launch::MergePaths;
    use duet_app::merge::view::MergeView;
    use duet_core::text::merge::MergeText;
    let (handle, view) = open_window(cx, (width, 800.), move |window, cx| {
        let view = cx.new(|cx| {
            if merged {
                let typed = |text: &str| MergeText::typed(text.to_owned());
                MergeView::with_texts_for_test(
                    [
                        typed(
                            "a
X
c
",
                        ),
                        typed(
                            "a
b
c
",
                        ),
                        typed(
                            "a
Y
c
",
                        ),
                    ],
                    window,
                    cx,
                )
            } else {
                MergeView::new(MergePaths::default(), window, cx)
            }
        });
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    let loaded = view.clone();
    exec.block_test(cx.wait_for(handle, Duration::from_secs(30), move |_, cx| {
        !loaded.read(cx).is_busy()
    }));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    (handle, view)
}

/// How many lines the controls `ids` sit on: their vertical centres, within a pixel.
fn rows_of(cx: &mut TestAppContext, handle: AnyWindowHandle, ids: &[&'static str]) -> usize {
    let mut centres: Vec<Pixels> = cx
        .update_window(handle, |_, window, _| {
            ids.iter()
                .map(|id| window.find(*id).bounds().center().y)
                .collect()
        })
        .unwrap();
    centres.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut rows = 0;
    let mut last: Option<Pixels> = None;
    for y in centres {
        if last.is_none_or(|l| (y - l).abs() > px(1.)) {
            rows += 1;
        }
        last = Some(y);
    }
    rows
}

/// The merge toolbar, the input stage's bar and the info bar, at 1400 px: the rows each takes.
fn merge_rows() -> [usize; 3] {
    use duet_app::merge::view::Stage;
    let (mut cx, exec) = directwrite_cx();
    let (handle, view) = open_merge(&mut cx, &exec, 1400., true);
    let toolbar = rows_of(
        &mut cx,
        handle,
        &[
            "merge-stage-seg",
            "merge-prev",
            "merge-pos",
            "merge-next",
            "merge-all-left",
            "merge-all-right",
            "merge-trail",
            "merge-show-result",
            "merge-copy",
            "merge-export",
            "merge-save",
        ],
    );
    let info = rows_of(&mut cx, handle, &["merge-info", "merge-save-as"]);
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.set_stage(Stage::Input, cx));
        window.render_frame(cx);
    })
    .unwrap();
    let input = rows_of(
        &mut cx,
        handle,
        &[
            "merge-mode",
            "merge-output",
            "merge-swap",
            "merge-clear",
            "merge-run",
        ],
    );
    finish(cx);
    [toolbar, info, input]
}

#[test]
fn thai_merge_toolbar_takes_no_more_rows_than_english() {
    let english = merge_rows();
    let thai = {
        let _th = thai_ui();
        merge_rows()
    };
    assert_eq!(english[0], 1, "the English toolbar is one line at 1400 px");
    for (i, (en, th)) in english.iter().zip(thai).enumerate() {
        assert!(
            th <= *en,
            "bar {i} takes {th} rows in Thai, {en} in English"
        );
    }
}

/// The input stage's explanation in Thai, narrow enough to wrap: every paragraph breaks only
/// between phrases and stays inside its own box, the coloured words among them.
#[test]
fn a_thai_merge_intro_wraps_between_phrases() {
    use duet_app::ui::prose::PIECE_ID;
    let _th = thai_ui();
    let (mut cx, exec) = directwrite_cx();
    let (handle, _view) = open_merge(&mut cx, &exec, 400., false);
    let tr = duet_app::i18n::tr;
    for (id, parts) in [
        (
            "merge-hint-title",
            vec!["3-way merge: combine the edits of two sides."],
        ),
        (
            "merge-hint-choose",
            vec![
                "Choose ",
                "Left",
                " (your version), ",
                "Base",
                " (the common original both sides started from) and ",
                "Right",
                " (their version).",
            ],
        ),
        (
            "merge-hint-parts",
            vec![
                "Parts changed on one side only, or changed the same way on both, merge on their own. Parts both sides changed differently are ",
                "conflicts",
                ": choose left, right, both, base or type the result, then save.",
            ],
        ),
        (
            "merge-hint-drop",
            vec![
                "Drop 3 files at once for left, base and right in that order, drop 1 file on its slot, or switch to “Paste text”.",
            ],
        ),
    ] {
        let want: usize = parts
            .iter()
            .map(|key| {
                assert_ne!(tr(key), *key, "{key} has Thai");
                tr(key).split_inclusive(' ').count()
            })
            .sum();
        let (boxed, pieces) = cx
            .update_window(handle, |_, window, _| {
                let boxed = window.find(id).bounds();
                let scoped = window.within(id);
                let pieces: Vec<Bounds<Pixels>> = (0usize..)
                    .map_while(|i| scoped.try_find((PIECE_ID, i)).map(|f| f.bounds()))
                    .collect();
                (boxed, pieces)
            })
            .unwrap();
        assert_eq!(pieces.len(), want, "{id}: one piece after each space");
        let line = pieces.iter().map(|p| p.size.height).min().unwrap();
        for (i, piece) in pieces.iter().enumerate() {
            assert_eq!(piece.size.height, line, "{id}: piece {i} on one line");
            assert!(
                piece.left() >= boxed.left() && piece.right() <= boxed.right(),
                "{id}: piece {i} {piece:?} runs past {boxed:?}"
            );
        }
        assert!(
            id == "merge-hint-title" || boxed.size.height > line,
            "{id}: at 400 px the paragraph wraps ({boxed:?})"
        );
    }
    finish(cx);
}

// ---- The table, hex and picture toolbars in Thai ----

/// A thai test for a bar: it takes no more rows in Thai than in English at 1400 px.
fn no_more_rows_in_thai(english: &[usize], thai: &[usize]) {
    assert_eq!(english.len(), thai.len());
    for (i, (en, th)) in english.iter().zip(thai).enumerate() {
        assert!(th <= en, "bar {i} takes {th} rows in Thai, {en} in English");
    }
}

/// The table view's toolbar and options row, at 1400 px: the rows each takes.
fn table_rows() -> [usize; 2] {
    let (mut cx, exec) = directwrite_cx();
    let (handle, view) = open_window(&mut cx, (1400., 760.), |window, cx| {
        let view = cx.new(|cx| {
            TableView::with_text_for_test("id,v\n1,a\n2,b\n", "id,v\n1,a\n2,c\n", window, cx)
        });
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    let loaded = view.clone();
    exec.block_test(cx.wait_for(handle, Duration::from_secs(30), move |_, cx| {
        !loaded.read(cx).is_busy()
    }));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let toolbar = rows_of(
        &mut cx,
        handle,
        &[
            "table-open-left",
            "table-paste-left",
            "table-left-name",
            "table-open-right",
            "table-paste-right",
            "table-right-name",
            "table-first",
            "table-prev",
            "table-position",
            "table-next",
            "table-last",
            "table-swap",
            "table-reload",
            "table-find",
            "table-export",
        ],
    );
    let options = rows_of(
        &mut cx,
        handle,
        &[
            "table-delimiter",
            "table-header",
            "table-ignore-case",
            "table-trim",
            "table-numbers",
            "table-keys",
            "table-chip-all",
            "table-chip-diff",
            "table-chip-same",
            "table-chip-left",
            "table-chip-right",
        ],
    );
    finish(cx);
    [toolbar, options]
}

/// The same toolbars with two workbooks loaded, at 1400 px: the sheet pickers are in the toolbar
/// and the Delimiter menu is gone from the options row.
fn table_book_rows() -> [usize; 2] {
    use duet_core::table::xlsx::fixture::{SheetSpec, workbook};
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.xlsx"), dir.path().join("b.xlsx"));
    // Sheet names as long as a person gives them, so the picker is as wide as it gets.
    let sheets = [
        SheetSpec::text_rows("Quarterly sales summary", &[&["id", "v"], &["1", "a"]]),
        SheetSpec::text_rows("Notes", &[&["id"], &["1"]]),
    ];
    std::fs::write(&a, workbook(&sheets)).unwrap();
    std::fs::write(&b, workbook(&sheets)).unwrap();
    let (mut cx, exec) = directwrite_cx();
    let (handle, view) = open_window(&mut cx, (1400., 760.), move |window, cx| {
        let view = cx.new(|cx| TableView::new(a, b, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    let loaded = view.clone();
    exec.block_test(cx.wait_for(handle, Duration::from_secs(30), move |_, cx| {
        !loaded.read(cx).is_busy()
    }));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let toolbar = rows_of(
        &mut cx,
        handle,
        &[
            "table-open-left",
            "table-paste-left",
            "table-left-name",
            "table-sheet-left",
            "table-open-right",
            "table-paste-right",
            "table-right-name",
            "table-sheet-right",
            "table-first",
            "table-prev",
            "table-position",
            "table-next",
            "table-last",
            "table-swap",
            "table-reload",
            "table-find",
            "table-export",
        ],
    );
    let options = rows_of(
        &mut cx,
        handle,
        &[
            "table-header",
            "table-ignore-case",
            "table-trim",
            "table-numbers",
            "table-keys",
            "table-chip-all",
            "table-chip-diff",
            "table-chip-same",
            "table-chip-left",
            "table-chip-right",
        ],
    );
    finish(cx);
    [toolbar, options]
}

#[test]
fn the_sheet_pickers_add_no_row_to_the_table_toolbar_in_either_language() {
    let english = table_book_rows();
    assert_eq!(
        english[0], 1,
        "the English toolbar with pickers is one line at 1400 px"
    );
    let thai = {
        let _th = thai_ui();
        table_book_rows()
    };
    no_more_rows_in_thai(&english, &thai);
    assert_eq!(thai[0], 1, "and so is the Thai one");
}

#[test]
fn thai_table_toolbars_take_no_more_rows_than_english() {
    let english = table_rows();
    let thai = {
        let _th = thai_ui();
        table_rows()
    };
    assert_eq!(english[0], 1, "the English toolbar is one line at 1400 px");
    no_more_rows_in_thai(&english, &thai);
}

/// The hex view's toolbar at 1400 px: the rows it takes.
fn hex_rows() -> [usize; 1] {
    use duet_app::hex::view::HexView;
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.bin"), dir.path().join("b.bin"));
    std::fs::write(&a, [0u8; 64]).unwrap();
    std::fs::write(&b, [1u8; 64]).unwrap();
    let (mut cx, exec) = directwrite_cx();
    let (handle, view) = open_window(&mut cx, (1400., 760.), move |window, cx| {
        let view = cx.new(|cx| HexView::new(a, b, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    let loaded = view.clone();
    exec.block_test(cx.wait_for(handle, Duration::from_secs(30), move |_, cx| {
        !loaded.read(cx).is_busy()
    }));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let toolbar = rows_of(
        &mut cx,
        handle,
        &[
            "hex-first",
            "hex-prev",
            "hex-position",
            "hex-next",
            "hex-last",
            "hex-swap",
            "hex-reload",
            "hex-bpr",
            "hex-filter",
            "hex-goto",
            "hex-goto-go",
            "hex-find",
            "hex-copy",
        ],
    );
    finish(cx);
    [toolbar]
}

#[test]
fn thai_hex_toolbar_takes_no_more_rows_than_english() {
    let english = hex_rows();
    let thai = {
        let _th = thai_ui();
        hex_rows()
    };
    assert_eq!(english[0], 1, "the English toolbar is one line at 1400 px");
    no_more_rows_in_thai(&english, &thai);
}

/// The picture view's toolbar and heads at 1400 px: the rows each takes.
fn picture_rows() -> [usize; 2] {
    use duet_app::picture::view::PictureView;
    use duet_core::picture::Picture;
    use duet_core::picture::export::encode_png;
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.png"), dir.path().join("b.png"));
    let mut marked = Picture::filled(4, 4, [100, 100, 100, 255]);
    marked.set_pixel(1, 1, [100, 100, 200, 255]);
    std::fs::write(
        &a,
        encode_png(&Picture::filled(4, 4, [100, 100, 100, 255])).unwrap(),
    )
    .unwrap();
    std::fs::write(&b, encode_png(&marked).unwrap()).unwrap();
    let (mut cx, exec) = directwrite_cx();
    let (handle, view) = open_window(&mut cx, (1400., 760.), move |window, cx| {
        let view = cx.new(|cx| PictureView::new(a, b, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    let loaded = view.clone();
    exec.block_test(cx.wait_for(handle, Duration::from_secs(30), move |_, cx| {
        !loaded.read(cx).is_busy()
    }));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let toolbar = rows_of(
        &mut cx,
        handle,
        &[
            "picture-mode",
            "picture-zoom-fit",
            "picture-zoom-1",
            "picture-zoom-out",
            "picture-zoom-pct",
            "picture-zoom-in",
            "picture-prev",
            "picture-next",
            "picture-swap",
            "picture-reload",
            "picture-export",
            "picture-tolerance-group",
        ],
    );
    let heads = rows_of(
        &mut cx,
        handle,
        &[
            "picture-open-left",
            "picture-left-name",
            "picture-left-meta",
            "picture-open-right",
            "picture-right-name",
            "picture-right-meta",
        ],
    );
    finish(cx);
    [toolbar, heads]
}

#[test]
fn thai_picture_toolbar_takes_no_more_rows_than_english() {
    let english = picture_rows();
    let thai = {
        let _th = thai_ui();
        picture_rows()
    };
    no_more_rows_in_thai(&english, &thai);
}

/// The slider's knob hangs 8 px left of its track plus a 3 px hover ring; the label must end
/// clear of that, so the knob never covers its last letter.
fn tolerance_label_gap() -> f32 {
    use duet_app::picture::view::PictureView;
    use duet_core::picture::Picture;
    use duet_core::picture::export::encode_png;
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.png"), dir.path().join("b.png"));
    let png = encode_png(&Picture::filled(4, 4, [100, 100, 100, 255])).unwrap();
    std::fs::write(&a, &png).unwrap();
    std::fs::write(&b, &png).unwrap();
    let (mut cx, exec) = directwrite_cx();
    let (handle, view) = open_window(&mut cx, (1400., 760.), move |window, cx| {
        let view = cx.new(|cx| PictureView::new(a, b, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    let loaded = view.clone();
    exec.block_test(cx.wait_for(handle, Duration::from_secs(30), move |_, cx| {
        !loaded.read(cx).is_busy()
    }));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let label = bounds_of(&mut cx, handle, "picture-tolerance-label");
    let slider = bounds_of(&mut cx, handle, "picture-tolerance");
    finish(cx);
    f32::from(slider.left() - label.right())
}

#[test]
fn the_thai_tolerance_label_ends_clear_of_the_slider_knob() {
    let _th = thai_ui();
    let gap = tolerance_label_gap();
    assert!(
        gap >= 11.,
        "label to slider track is {gap} px, the knob needs 11"
    );
}

#[test]
fn the_english_tolerance_label_ends_clear_of_the_slider_knob() {
    let gap = tolerance_label_gap();
    assert!(
        gap >= 11.,
        "label to slider track is {gap} px, the knob needs 11"
    );
}

/// A long Thai status sentence, the read-only one of the sorted view and then some.
const LONG_THAI_STATUS: &str = "มุมมองที่เรียงคีย์แล้ว เป็นแบบอ่านอย่างเดียว ปิด “เรียงคีย์” ก่อน จึงจะแก้ได้ และข้อความนี้ยาวต่อไปอีกมากเพื่อให้เกินความกว้างของแถบสถานะแน่นอน";

/// With a long Thai notice at 900 px, the status bar keeps its height, the text is one line
/// and ends inside the bar.
fn assert_status_stays_one_line(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    bar: &'static str,
    text: &'static str,
    show: impl FnOnce(&mut TestAppContext),
) {
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let before = bounds_of(cx, handle, bar);
    show(cx);
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let after = bounds_of(cx, handle, bar);
    let line = bounds_of(cx, handle, text);
    assert_eq!(before.size.height, after.size.height, "{bar} grew");
    assert!(
        line.size.height <= px(17.),
        "{text} is {:?} tall",
        line.size.height
    );
    assert!(
        line.left() >= after.left() && line.right() <= after.right(),
        "{text} {line:?} is outside {after:?}"
    );
}

#[test]
fn a_long_thai_text_status_stays_one_line_in_the_bar_at_900_px() {
    let _th = thai_ui();
    let (mut cx, exec) = directwrite_cx();
    let (_dir, handle, view) = open_text_pair(&mut cx, &exec, 900.);
    let shown = view.clone();
    assert_status_stays_one_line(
        &mut cx,
        handle,
        "text-status",
        "text-status-text",
        move |cx| {
            cx.update(|cx| shown.update(cx, |v, cx| v.show_notice(LONG_THAI_STATUS, cx)));
        },
    );
    finish(cx);
}

#[test]
fn a_long_thai_table_status_stays_one_line_in_the_bar_at_900_px() {
    let _th = thai_ui();
    let (mut cx, exec) = directwrite_cx();
    let (handle, view) = open_window(&mut cx, (900., 760.), |window, cx| {
        let view = cx.new(|cx| {
            TableView::with_text_for_test(
                "id,v
1,a
2,b
",
                "id,v
1,a
2,c
",
                window,
                cx,
            )
        });
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    let loaded = view.clone();
    exec.block_test(cx.wait_for(handle, Duration::from_secs(30), move |_, cx| {
        !loaded.read(cx).is_busy()
    }));
    let shown = view.clone();
    assert_status_stays_one_line(&mut cx, handle, "table-status", "table-notice", move |cx| {
        cx.update(|cx| {
            shown.update(cx, |v, cx| {
                v.show_progress_for_test(LONG_THAI_STATUS);
                cx.notify();
            })
        });
    });
    finish(cx);
}

/// Opened from a folder compare the text view's toolbar also holds Back, and may wrap at
/// 1400 px; that is accepted, but no control is cut off: each lies inside the toolbar.
#[test]
fn the_text_toolbar_with_back_cuts_nothing_off_at_1400_px() {
    back_toolbar_cuts_nothing_off();
}

#[test]
fn the_text_toolbar_with_back_cuts_nothing_off_at_1400_px_in_thai() {
    let _th = thai_ui();
    back_toolbar_cuts_nothing_off();
}

fn back_toolbar_cuts_nothing_off() {
    let (mut cx, exec) = directwrite_cx();
    let (_dir, handle, view) = open_text_pair(&mut cx, &exec, 1400.);
    view.update(&mut cx, |v, cx| v.set_has_origin(true, cx));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.update_window(handle, |_, window, _| {
        let bar = window.find("text-toolbar").bounds();
        for id in [
            "text-back",
            "text-swap",
            "text-reload",
            "text-format",
            "text-sort-keys",
            "text-next",
            "text-report",
            "text-save-options",
            "text-font",
            "text-find-open",
            "text-goto-open",
        ] {
            let b = window.find(id).bounds();
            assert!(
                b.left() >= bar.left()
                    && b.right() <= bar.right()
                    && b.top() >= bar.top()
                    && b.bottom() <= bar.bottom(),
                "{id} at {b:?} sticks out of the toolbar at {bar:?}"
            );
        }
    })
    .unwrap();
    finish(cx);
}

// ---- The header's mode bar fits, in both languages ----

fn the_mode_bar_fits_at_each_width() {
    let (mut cx, _fg) = directwrite_cx();
    for width in [800., 1100., 1400.] {
        let (handle, _workspace) = open_window(&mut cx, (width, 850.), |window, cx| {
            cx.new(|cx| {
                duet_app::workspace::Workspace::new(duet_app::launch::Launch::Start, window, cx)
            })
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            let right = |b: Bounds<Pixels>| b.origin.x + b.size.width;
            let brand = window.find("header-brand").bounds();
            let modes = window.find("header-modes").bounds();
            assert!(
                right(brand) <= modes.origin.x,
                "{width}: brand {brand:?} modes {modes:?}"
            );
            let next = window
                .try_find("header-offline")
                .unwrap_or_else(|| window.find("header-help"))
                .bounds();
            assert!(
                right(modes) <= next.origin.x,
                "{width}: modes {modes:?} next {next:?}"
            );
            for index in 0..duet_app::tabs::Mode::ALL.len() {
                let segment = window.find(("header-mode", index)).bounds();
                assert!(
                    right(segment) <= right(modes) + px(1.),
                    "{width}: segment {index} {segment:?} cut in {modes:?}"
                );
            }
        })
        .unwrap();
    }
    finish(cx);
}

#[test]
fn the_mode_bar_fits_the_header_in_english() {
    the_mode_bar_fits_at_each_width();
}

#[test]
fn the_mode_bar_fits_the_header_in_thai() {
    let _thai = thai_ui();
    the_mode_bar_fits_at_each_width();
}

// ---- A Thai label stays on one line where it is drawn in a box of its own width ----

struct LabelBox {
    label: gpui_kit::SharedString,
    one_line: bool,
    layout: std::rc::Rc<std::cell::RefCell<Option<gpui_kit::TextLayout>>>,
}

impl Render for LabelBox {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        use gpui_kit::{IntoElement as _, ParentElement as _, Styled as _};
        let theme = gpui_kit::component::Theme::global(cx);
        window.set_rem_size(theme.font_size);
        let family = theme.font_family.clone();
        let text = gpui_kit::StyledText::new(self.label.clone());
        *self.layout.borrow_mut() = Some(text.layout().clone());
        // As a kit menu row lays out its label: a flex child with no width of its own.
        let label = if self.one_line {
            duet_app::ui::menu::one_line(text).into_any_element()
        } else {
            text.into_any_element()
        };
        gpui_kit::div()
            .absolute()
            .w(px(500.))
            .font_family(family)
            .text_sm()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(label)
    }
}

/// Which of `labels` draw on more than one line, at each display scale.
fn wrapped_labels(labels: &[&'static str], one_line: bool) -> Vec<(f32, &'static str)> {
    let (mut cx, _fg) = directwrite_cx();
    let layout = std::rc::Rc::new(std::cell::RefCell::new(None));
    let shared = layout.clone();
    let (handle, view) = open_window(&mut cx, (1400., 850.), move |_, cx| {
        cx.new(|_| LabelBox {
            label: "x".into(),
            one_line,
            layout: shared,
        })
    });
    let mut wrapped = Vec::new();
    for scale in [1.0_f32, 1.25, 1.5, 1.75, 2.0] {
        for label in labels {
            let wraps = cx
                .update_window(handle, |_, window, cx| {
                    window.set_scale_factor(scale);
                    view.update(cx, |v, cx| {
                        v.label = (*label).into();
                        cx.notify();
                    });
                    window.render_frame(cx);
                    window.render_frame(cx);
                    let drawn: Option<gpui_kit::TextLayout> = layout.borrow().clone();
                    drawn.expect("drawn").wrapped_text().contains('\n')
                })
                .unwrap();
            if wraps {
                wrapped.push((scale, *label));
            }
        }
    }
    finish(cx);
    wrapped
}

#[test]
fn a_bare_thai_label_can_wrap_and_one_line_stops_it() {
    let _thai = thai_ui();
    let labels: Vec<&'static str> = duet_app::i18n::th::table()
        .iter()
        .filter(|(_, thai)| thai.chars().count() <= 45 && !thai.contains('{'))
        .map(|(_, thai)| *thai)
        .collect();
    let bare = wrapped_labels(&labels, false);
    assert!(
        bare.iter().any(|(_, label)| *label == "ยุบโฟลเดอร์"),
        "the case this guards: gpui wraps it in a box of its own width. If this now passes, \
         gpui changed and `ui::menu::row` may be simpler: {bare:?}"
    );
    let held = wrapped_labels(&labels, true);
    assert!(held.is_empty(), "{held:?}");
}

#[test]
fn a_thai_tooltip_does_not_shrink_to_its_narrowest_piece() {
    use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _};
    let _thai = thai_ui();
    let (mut cx, _fg) = directwrite_cx();
    struct Tip;
    impl Render for Tip {
        fn render(
            &mut self,
            _: &mut Window,
            _: &mut gpui_kit::Context<Self>,
        ) -> impl gpui_kit::IntoElement {
            use gpui_kit::{Styled as _, TestSupportExt as _};
            gpui_kit::div()
                .id("tip-target")
                .test_support()
                .size(px(100.))
                .tooltip(|window, cx| {
                    duet_app::ui::style::ld_tip(
                        "1.0 = 1, 1,000 = 1000, 1e3 = 1000".into(),
                        window,
                        cx,
                    )
                })
        }
    }
    let (handle, _view) = open_window(&mut cx, (1400., 850.), |_, cx| cx.new(|_| Tip));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.hover("tip-target", cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let tip = window.find("tooltip-text").bounds();
        assert!(tip.size.width >= px(180.), "one word a line: {tip:?}");
        assert!(tip.size.height <= px(44.), "at most two lines: {tip:?}");
    })
    .unwrap();
    finish(cx);
}
