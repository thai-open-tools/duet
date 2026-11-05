//! Headless UI tests for the text view's rules (P9b Tasks 3 to 6): whitespace, blank lines and
//! comments, the rules bar, the Type menu and lexer colours, and manual alignment.

mod text_support;

use std::path::Path;
use std::time::Duration;

use duet_app::palette::Palette;
use duet_app::text::job::IgnoreChoice;
use duet_app::text::model::TextFilter;
use duet_app::text::prefs::{SaveEncoding, SaveEol};
use duet_app::text::view::TextView;
use duet_app::text::view::rows::HalfFill;
use duet_core::text::diff::WhitespaceMode;
use duet_core::text::lexer::Language;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AnyWindowHandle, AppContext, Entity, Focusable as _, TestAppContext};
use text_support::*;

#[gpui_kit::test]
async fn blank_lines_show_purple_and_do_not_count(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n\nb\n", "a\nb\n");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(blocks(cx, &view), 1);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("copy-right", 0usize)).is_some());
    })
    .unwrap();
    view.update(cx, |v, cx| v.set_ignore_blank(true, cx));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(blocks(cx, &view), 0);
    assert!(view.read_with(cx, |v, _| v.rules_active()));
    assert_eq!(
        status(cx, &view),
        "Same under the rules · 1 unimportant line · 3 lines · showing All"
    );
    let p = Palette::light();
    let (left, right) = view.read_with(cx, |v, _| {
        (
            v.half_look_for_test(1, true).unwrap(),
            v.half_look_for_test(1, false).unwrap(),
        )
    });
    assert_eq!(left.fill, HalfFill::Tint(p.bg_unimp));
    assert_eq!((left.number, left.edge), (p.unimp, Some(p.unimp)));
    assert_eq!(right.fill, HalfFill::Hatch);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("text-row", 1usize)).is_some());
        assert!(
            window.try_find(("copy-right", 0usize)).is_none(),
            "no copy arrows"
        );
        assert!(window.try_find(("copy-left", 0usize)).is_none());
    })
    .unwrap();
    view.read_with(cx, |v, _| {
        let marks = duet_app::text::overview::marks(v.model().unwrap(), 0.);
        assert!(marks.is_empty(), "no overview mark: {marks:?}");
    });
    press(cx, handle, "f8");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("No differences to go to.")
    );
    view.update(cx, |v, cx| v.set_ignore_blank(false, cx));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(blocks(cx, &view), 1, "off again");

    // E6: rows and no blocks; a scroll of the user's and Differences never panic.
    let left: String = (0..300).map(|i| format!("line {i}\n\n")).collect();
    let right: String = (0..300).map(|i| format!("line {i}\n")).collect();
    for list in [false, true] {
        let (_dir, handle, view) = open_text(cx, &left, &right);
        wait_loaded(cx, handle, &view).await;
        view.update(cx, |v, cx| v.set_ignore_blank(true, cx));
        wait_loaded(cx, handle, &view).await;
        assert_eq!(blocks(cx, &view), 0);
        view.update(cx, |v, cx| v.set_list_rows_for_test(list, cx));
        render(cx, handle);
        view.update(cx, |v, cx| v.scroll_like_the_user_for_test(200, cx));
        render(cx, handle);
        render(cx, handle);
        assert_eq!(current_block(cx, &view), None, "wrapped list {list}");
        press(cx, handle, "ctrl-2");
        for _ in 0..3 {
            render(cx, handle);
        }
        assert_eq!(current_block(cx, &view), None, "wrapped list {list}");
        let position = view.read_with(cx, |v, _| duet_app::text::view::position_text(v.model()));
        assert_eq!(position, "– / 0");
    }
}

#[gpui_kit::test]
async fn the_whitespace_menu_offers_six_modes(cx: &mut TestAppContext) {
    for (mode, left, right) in [
        (WhitespaceMode::Trailing, "a \n", "a\n"),
        (WhitespaceMode::Leading, "  a\n", "a\n"),
        (WhitespaceMode::Amount, "a  b\n", "a b\n"),
    ] {
        let (_dir, handle, view) = open_text(cx, left, right);
        wait_loaded(cx, handle, &view).await;
        assert_eq!(blocks(cx, &view), 1, "{mode:?}: compared under None");
        view.update(cx, |v, cx| v.set_whitespace(mode, cx));
        wait_loaded(cx, handle, &view).await;
        assert_eq!(
            view.read_with(cx, |v, _| v.request().options.whitespace),
            mode
        );
        assert_eq!(blocks(cx, &view), 0, "{mode:?}");
        view.update(cx, |v, cx| v.set_whitespace(WhitespaceMode::None, cx));
        wait_loaded(cx, handle, &view).await;
        assert_eq!(blocks(cx, &view), 1, "{mode:?}, then None again");
    }
}

#[gpui_kit::test]
async fn comments_are_ignored_by_language(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_files(
        cx,
        (Path::new("a.rs"), b"let a = 1; // x\nlet b = 2;\n"),
        (Path::new("b.rs"), b"let a = 1;\nlet b = 2;\n"),
    );
    wait_loaded(cx, handle, &view).await;
    assert_eq!(blocks(cx, &view), 1);
    view.update(cx, |v, cx| v.set_ignore_comments(true, cx));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(blocks(cx, &view), 0);
    assert!(
        status(cx, &view).contains("1 unimportant line"),
        "{}",
        status(cx, &view)
    );
    assert_eq!(notice(cx, &view), None, "Rust is a known type");
    // What the rule hid shows as unimportant on the pair it matched.
    let p = Palette::light();
    let look = view.read_with(cx, |v, _| v.half_look_for_test(0, true).unwrap());
    assert_eq!(
        (look.edge, look.highlight),
        (Some(p.unimp), Some(p.hl_unimp))
    );
    view.update(cx, |v, cx| v.set_ignore_comments(false, cx));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(blocks(cx, &view), 1);
}

#[gpui_kit::test]
async fn a_url_in_a_string_is_not_a_comment(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_files(
        cx,
        (Path::new("a.rs"), b"s = \"http://x\";\n"),
        (Path::new("b.rs"), b"s = \"http://y\";\n"),
    );
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_ignore_comments(true, cx));
    wait_loaded(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.rules_active()));
    assert_eq!(blocks(cx, &view), 1);
}

#[gpui_kit::test]
async fn comments_on_an_unknown_type_say_so(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "x = 1 # c\n", "x = 1\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_ignore_comments(true, cx));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("This file's type is unknown: pick one under Type so comments can be found.")
    );
    assert_eq!(blocks(cx, &view), 1, "plain text has no comments");
    view.update(cx, |v, cx| v.set_language(Some(Language::Python), cx));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(blocks(cx, &view), 0, "Python's # starts a comment");
    view.update(cx, |v, cx| v.set_language(None, cx));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(blocks(cx, &view), 1);
}

#[gpui_kit::test]
async fn comments_on_a_type_without_comments_say_nothing_is_ignored(cx: &mut TestAppContext) {
    // X-73: JSON, a log and a patch have no comment syntax, so the notice says so too.
    let cases = [
        ("data.json", "{\"a\": 1}\n", "JSON"),
        ("run.log", "INFO a\n", "Log"),
        ("fix.patch", "+a\n", "Diff/Patch"),
    ];
    for (name, text, label) in cases {
        let (_dir, handle, view) = open_named(cx, name, text, text);
        wait_loaded(cx, handle, &view).await;
        view.update(cx, |v, cx| v.set_ignore_comments(true, cx));
        wait_loaded(cx, handle, &view).await;
        let want = format!("{label} files have no comments, so nothing is ignored.");
        assert_eq!(notice(cx, &view), Some(want), "{name}");
        assert!(
            view.read_with(cx, |v, _| v.prefs().ignore_comments),
            "the button stays lit"
        );
    }
    // A type with comments gives no notice.
    let (_dir, handle, view) = open_named(cx, "a.py", "x = 1 # c\n", "x = 1\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_ignore_comments(true, cx));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(notice(cx, &view), None);
    assert_eq!(blocks(cx, &view), 0);
}

#[gpui_kit::test]
async fn compare_choices_are_saved_and_a_new_tab_uses_them(cx: &mut TestAppContext) {
    let (data, _dir, handle, view) = open_text_with_settings(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_whitespace(WhitespaceMode::Amount, cx));
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.toggle_ignore(IgnoreChoice::Case, cx));
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_ignore_blank(true, cx));
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_ignore_comments(true, cx));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(saved_setting(&data, "text.whitespace"), "amount");
    assert_eq!(saved_setting(&data, "text.ignore_case"), true);
    assert_eq!(saved_setting(&data, "text.ignore_blank"), true);
    assert_eq!(saved_setting(&data, "text.ignore_comments"), true);
    assert!(
        saved_setting(&data, "text.ignore_eol").is_null(),
        "one key per change (PF9)"
    );

    let (left, right) = view.read_with(cx, |v, _| {
        (v.request().left.clone(), v.request().right.clone())
    });
    let second = cx
        .update_window(handle, |_, _, cx| {
            cx.new(|cx| TextView::new(left, right, cx))
        })
        .unwrap();
    wait_loaded(cx, handle, &second).await;
    second.read_with(cx, |v, _| {
        let request = v.request();
        assert_eq!(request.options.whitespace, WhitespaceMode::Amount);
        assert!(request.options.ignore_case && !request.options.ignore_eol);
        assert!(request.rules.ignore_blank);
        assert_eq!(request.rules.comments, Some(Language::Text));
        assert!(v.rules_active(), "blank lines");
    });
}

#[gpui_kit::test]
#[ignore = "writes a 65 MiB file; run once in the final round"]
async fn rules_are_noted_when_a_file_is_too_large(cx: &mut TestAppContext) {
    let line = format!("{}\n", "x".repeat(99));
    let big = line.repeat(65 * 1024 * 1024 / line.len() + 1);
    let small = line.repeat(10);
    let (_dir, handle, view) = open_text(cx, &big, &small);
    drop(big);
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_ignore_blank(true, cx));
    wait_loaded(cx, handle, &view).await;
    let note = " · Rules not applied: a file is over 64 MiB.";
    let status_now = status(cx, &view);
    assert!(status_now.ends_with(note), "{status_now}");
    // The small side can be edited; the re-diff says it too (PF14).
    view.update(cx, |v, cx| v.set_caret_for_test(false, 0, cx));
    press(cx, handle, "enter");
    wait_loaded(cx, handle, &view).await;
    let status_now = status(cx, &view);
    assert!(status_now.ends_with(note), "after an edit: {status_now}");
}

#[gpui_kit::test]
async fn ini_and_log_files_get_colours(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_named(cx, "a.ini", "[sec]\nkey=1\n", "[sec]\nkey=1\n");
    wait_loaded(cx, handle, &view).await;
    wait_syntax(cx, &view, true).await;
    wait_syntax(cx, &view, false).await;
    view.read_with(cx, |v, _| {
        assert!(!v.line_styles_for_test(true, 0).is_empty());
        assert!(!v.line_styles_for_test(false, 1).is_empty());
        assert_eq!(v.syntax_tooltip_for_test(), "Left: INI. Right: INI.");
    });
    let (_dir, handle, view) = open_named(
        cx,
        "a.log",
        "2026-01-02 10:00:00 ERROR boom\n",
        "2026-01-02 10:00:00 ERROR boom\n",
    );
    wait_loaded(cx, handle, &view).await;
    wait_syntax(cx, &view, true).await;
    view.read_with(cx, |v, _| {
        assert!(!v.line_styles_for_test(true, 0).is_empty());
        assert_eq!(v.syntax_tooltip_for_test(), "Left: Log. Right: Log.");
    });
}

#[gpui_kit::test]
async fn the_type_menu_overrides_detection_per_tab(cx: &mut TestAppContext) {
    let text = "def f():\n  pass\n";
    let (_dir, handle, view) = open_text(cx, text, text);
    wait_loaded(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.language(), None);
        assert_eq!(v.detected_language(), Language::Text);
        assert_eq!(v.comment_language(), Language::Text);
        assert!(v.line_styles_for_test(true, 0).is_empty(), "plain");
    });
    view.update(cx, |v, cx| v.set_language(Some(Language::Python), cx));
    wait_syntax(cx, &view, true).await;
    wait_syntax(cx, &view, false).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.language(), Some(Language::Python));
        assert_eq!(v.comment_language(), Language::Python);
        assert_eq!(v.detected_language(), Language::Text);
        assert_eq!(v.syntax_label_for_test(true), Some("python"));
        assert_eq!(v.syntax_label_for_test(false), Some("python"));
        assert!(!v.line_styles_for_test(true, 0).is_empty());
    });
    view.update(cx, |v, cx| v.set_language(None, cx));
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        assert_eq!(v.language(), None);
        assert!(v.line_styles_for_test(true, 0).is_empty(), "plain again");
        assert_eq!(v.syntax_label_for_test(true), None);
    });
    // A second tab over the same pair starts on Auto.
    let (_dir2, handle2, view2) = open_text(cx, text, text);
    wait_loaded(cx, handle2, &view2).await;
    assert_eq!(view2.read_with(cx, |v, _| v.language()), None);
}

fn open_rules(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<TextView>) {
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.open_rules(window, cx))
    })
    .unwrap();
    render(cx, handle);
}

fn set_fields(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
    (text, lines, context): (&str, &str, &str),
) {
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.set_rules_fields_for_test(text, lines, context, window, cx)
        })
    })
    .unwrap();
}

fn apply_rules(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<TextView>) -> bool {
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.apply_rules(window, cx))
    })
    .unwrap()
}

fn preset(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<TextView>, index: usize) {
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.add_preset(index, window, cx))
    })
    .unwrap();
}

fn set_menu_row(cx: &mut TestAppContext, view: &Entity<TextView>, row: u32, left: bool) {
    view.update(cx, |v, _| v.set_menu_row_for_test(row, left));
}

#[gpui_kit::test]
async fn the_rules_panel_applies_valid_patterns_and_names_a_bad_one(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "log 2024-01-01 ok\n", "log 2025-02-03 ok\n");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(blocks(cx, &view), 1);
    open_rules(cx, handle, &view);
    assert!(view.read_with(cx, |v, _| v.rules_open()));
    cx.update_window(handle, |_, window, _| {
        for id in [
            "text-rules",
            "text-rules-text",
            "text-rules-lines",
            "text-rules-context",
            "text-rules-apply",
            "text-rules-cancel",
        ] {
            assert!(window.try_find(id).is_some(), "{id}");
        }
        for i in 0..4usize {
            let id = gpui_kit::SharedString::from(format!("text-rules-preset-{i}"));
            assert!(window.try_find(id).is_some(), "{i}");
        }
        assert!(window.try_find("text-rules-error").is_none());
    })
    .unwrap();
    set_fields(cx, handle, &view, (r"\d+", "", "3"));
    assert!(apply_rules(cx, handle, &view));
    wait_loaded(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| !v.rules_open()));
    assert_eq!(blocks(cx, &view), 0);
    assert!(view.read_with(cx, |v, _| v.rules_active()));

    open_rules(cx, handle, &view);
    set_fields(cx, handle, &view, ("(oops", "", "3"));
    assert!(!apply_rules(cx, handle, &view));
    view.read_with(cx, |v, _| {
        assert!(v.rules_open(), "the panel stays open");
        let error = v.rules_error().unwrap();
        assert!(error.starts_with("Invalid pattern: (oops: "), "{error}");
    });
    render(cx, handle);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("text-rules-error").is_some());
    })
    .unwrap();
    assert_eq!(blocks(cx, &view), 0, "the previous rules still apply");
    assert_eq!(
        view.read_with(cx, |v, _| v.prefs().rules_text.clone()),
        r"\d+"
    );
}

#[gpui_kit::test]
async fn presets_add_their_pattern_on_a_new_line(cx: &mut TestAppContext) {
    use duet_app::text::view::rules_bar::PRESETS;
    let (_dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    open_rules(cx, handle, &view);
    set_fields(cx, handle, &view, ("", "", "3"));
    preset(cx, handle, &view, 2);
    preset(cx, handle, &view, 3);
    let text = view.read_with(cx, |v, cx| v.rules_text_for_test(cx).unwrap());
    assert_eq!(text, "\\d+\n\\d+\\.\\d+(\\.\\d+)*");
    assert_eq!(PRESETS[2].1, r"\d+");
    assert_eq!(PRESETS[0].0, "+ Date and time");
    // A preset after text with trailing blanks starts its own line (ops:600-605).
    set_fields(cx, handle, &view, ("x  \n\n", "", "3"));
    preset(cx, handle, &view, 2);
    let text = view.read_with(cx, |v, cx| v.rules_text_for_test(cx).unwrap());
    assert_eq!(text, "x\n\\d+");
}

#[gpui_kit::test]
async fn context_lines_set_the_context_filter(cx: &mut TestAppContext) {
    let (data, _dir, handle, view) = open_text_with_settings(cx, "a\nb\nc\n", "a\nB\nc\n");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.context_lines()), 3);
    open_rules(cx, handle, &view);
    set_fields(cx, handle, &view, ("", "", "1"));
    assert!(apply_rules(cx, handle, &view));
    wait_loaded(cx, handle, &view).await;
    press(cx, handle, "ctrl-3");
    view.read_with(cx, |v, _| {
        let filter = v.model().unwrap().filter();
        assert_eq!(filter, TextFilter::Context(1));
        assert_eq!(filter.label(), "Differences + 1 line");
    });
    assert_eq!(saved_setting(&data, "text.context_lines"), 1);
    for (typed, saved) in [("99", 50u32), ("x", 3), ("0", 0)] {
        open_rules(cx, handle, &view);
        set_fields(cx, handle, &view, ("", "", typed));
        assert!(apply_rules(cx, handle, &view));
        wait_loaded(cx, handle, &view).await;
        assert_eq!(saved_setting(&data, "text.context_lines"), saved, "{typed}");
        assert_eq!(view.read_with(cx, |v, _| v.context_lines()), saved);
        // While the filter is Context it follows the setting.
        assert_eq!(
            view.read_with(cx, |v, _| v.model().unwrap().filter()),
            TextFilter::Context(saved)
        );
    }
}

#[gpui_kit::test]
async fn the_rules_button_is_lit_while_rules_are_set(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| !v.rules_active() && !v.rules_lit()));
    open_rules(cx, handle, &view);
    set_fields(cx, handle, &view, ("", "^#", "3"));
    assert!(apply_rules(cx, handle, &view));
    wait_loaded(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.rules_active() && v.rules_lit()));
    open_rules(cx, handle, &view);
    set_fields(cx, handle, &view, (" \n", "\n", "3"));
    assert!(apply_rules(cx, handle, &view));
    wait_loaded(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| !v.rules_active() && !v.rules_lit()));
}

#[gpui_kit::test]
async fn cancel_throws_the_fields_away(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    open_rules(cx, handle, &view);
    set_fields(cx, handle, &view, ("zzz", "yyy", "9"));
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.cancel_rules(window, cx))
    })
    .unwrap();
    assert!(view.read_with(cx, |v, _| !v.rules_open()));
    assert_eq!(view.read_with(cx, |v, _| v.prefs().rules_text.clone()), "");
    open_rules(cx, handle, &view);
    let text = view.read_with(cx, |v, cx| v.rules_text_for_test(cx).unwrap());
    assert_eq!(text, "", "reopened from the saved rules");
}

#[gpui_kit::test]
async fn typing_letters_in_a_rules_field_never_navigates(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\n", "a\nB\nc\n");
    wait_loaded(cx, handle, &view).await;
    open_rules(cx, handle, &view);
    for key in ["n", "p", "r", "l"] {
        press(cx, handle, key);
    }
    assert_eq!(
        current_block(cx, &view),
        None,
        "n and p stayed in the field"
    );
    let text = view.read_with(cx, |v, cx| v.rules_text_for_test(cx).unwrap());
    assert_eq!(text, "nprl");
    press(cx, handle, "ctrl-r");
    assert!(
        view.read_with(cx, |v, _| v.model().unwrap().buffer(false).is_none()),
        "Ctrl+R did not copy a block"
    );
}

#[gpui_kit::test]
async fn treat_lines_like_this_as_unimportant_adds_an_exact_line_rule(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "x\n", "x\na.b (x)\n");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(blocks(cx, &view), 1);
    right_click_row(cx, handle, 1);
    let items = row_items(cx, &view);
    assert_eq!(
        items.last().map(String::as_str),
        Some("Treat lines like this as unimportant"),
        "{items:?}"
    );
    assert!(run_item(
        cx,
        handle,
        &view,
        "Treat lines like this as unimportant"
    ));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.prefs().rules_lines.clone()),
        r"^a\.b \(x\)$"
    );
    assert_eq!(blocks(cx, &view), 0, "one block fewer");
    assert!(view.read_with(cx, |v, _| v.rules_active() && v.rules_lit()));
    // A second one goes on a new line.
    right_click_row(cx, handle, 0);
    set_menu_row(cx, &view, 0, true);
    assert!(run_item(
        cx,
        handle,
        &view,
        "Treat lines like this as unimportant"
    ));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.prefs().rules_lines.clone()),
        "^a\\.b \\(x\\)$\n^x$"
    );
    // On a placeholder half it is disabled.
    view.update(cx, |v, cx| v.set_rules_for_test("", "", cx));
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 1);
    set_menu_row(cx, &view, 1, true);
    let items = row_items(cx, &view);
    assert_eq!(
        items.last().map(String::as_str),
        Some("(Treat lines like this as unimportant)"),
        "{items:?}"
    );
}

#[gpui_kit::test]
async fn escape_clears_the_selection_then_closes_the_rules_panel(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\nc\n", "a\nB\nc\n");
    wait_loaded(cx, handle, &view).await;
    open_rules(cx, handle, &view);
    view.update(cx, |v, cx| v.select_rows_for_test(true, 0, 1, cx));
    assert!(selected_rows(cx, &view).is_some());
    press(cx, handle, "escape");
    assert!(
        selected_rows(cx, &view).is_none(),
        "the selection goes first"
    );
    assert!(view.read_with(cx, |v, _| v.rules_open()), "the panel stays");
    press(cx, handle, "escape");
    assert!(view.read_with(cx, |v, _| !v.rules_open()), "then it closes");
    cx.update_window(handle, |_, window, cx| {
        let rows = view.read(cx).focus_handle(cx);
        assert!(rows.is_focused(window), "and the rows have the keys");
        window.press("tab", cx);
        assert!(!rows.is_focused(window));
        window.press("escape", cx);
        assert!(rows.is_focused(window), "Esc gives the rows the focus");
    })
    .unwrap();
}

#[gpui_kit::test]
async fn a_second_click_on_rules_closes_the_panel(cx: &mut TestAppContext) {
    // X-74: the "Rules…" button toggles the panel.
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nB\n");
    wait_loaded(cx, handle, &view).await;
    render(cx, handle);
    click(cx, handle, "text-rules-open");
    render(cx, handle);
    assert!(
        view.read_with(cx, |v, _| v.rules_open()),
        "the first click opens it"
    );
    click(cx, handle, "text-rules-open");
    render(cx, handle);
    assert!(
        view.read_with(cx, |v, _| !v.rules_open()),
        "the second closes it"
    );
    cx.update_window(handle, |_, window, cx| {
        assert!(
            view.read(cx).focus_handle(cx).is_focused(window),
            "the rows have the keys"
        );
    })
    .unwrap();
    click(cx, handle, "text-rules-open");
    assert!(
        view.read_with(cx, |v, _| v.rules_open()),
        "and a third opens it again"
    );
}

const ALIGN_ITEM: &str = "Align this line with a line on the other side…";

/// "a b c" against "c a b" opened with the aligning started from the menu on the half of
/// display row `row` (row 0 is right "c" against a hatch, row 3 is left "c" against a hatch).
async fn start_aligning_at(
    cx: &mut TestAppContext,
    row: u32,
    left: bool,
) -> (tempfile::TempDir, AnyWindowHandle, Entity<TextView>) {
    let (dir, handle, view) = open_text(cx, "a\nb\nc\n", "c\na\nb\n");
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, row as usize);
    set_menu_row(cx, &view, row, left);
    assert!(run_item(cx, handle, &view, ALIGN_ITEM));
    (dir, handle, view)
}

/// Left "c" (line 2, display row 3).
async fn start_aligning(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, AnyWindowHandle, Entity<TextView>) {
    start_aligning_at(cx, 3, true).await
}

fn anchors(cx: &mut TestAppContext, view: &Entity<TextView>) -> Vec<(u32, u32)> {
    view.read_with(cx, |v, _| v.anchors().to_vec())
}

#[gpui_kit::test]
async fn aligning_a_line_with_the_other_side_pairs_them(cx: &mut TestAppContext) {
    let (_dir, handle, view) = start_aligning(cx).await;
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Click the line on the right side to align with left line 3 (Esc cancels).")
    );
    assert_eq!(view.read_with(cx, |v, _| v.aligning()), Some((true, 2)));
    assert_eq!(blocks(cx, &view), 2);
    view.update(cx, |v, cx| v.finish_align(0, false, cx));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(anchors(cx, &view), vec![(2, 0)]);
    assert_eq!(view.read_with(cx, |v, _| v.aligning()), None);
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Aligned left line 3 with right line 1.")
    );
    assert_eq!(blocks(cx, &view), 2);
    // c now sits on one row with c; the rest are one-sided.
    render(cx, handle);
    cx.update_window(handle, |_, window, _| {
        for side in ["left", "right"] {
            let id = if side == "left" {
                "text-anchor-left"
            } else {
                "text-anchor-right"
            };
            assert!(window.try_find((id, 2usize)).is_some(), "{id} on row 2");
            assert!(
                window.try_find((id, 0usize)).is_none(),
                "{id} only on the pair"
            );
        }
    })
    .unwrap();
    right_click_row(cx, handle, 2);
    assert!(
        row_items(cx, &view).contains(&"Clear manual alignments (1)".to_owned()),
        "{:?}",
        row_items(cx, &view)
    );
    assert!(run_item(cx, handle, &view, "Clear manual alignments (1)"));
    wait_loaded(cx, handle, &view).await;
    assert!(anchors(cx, &view).is_empty());
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Manual alignments cleared.")
    );
    assert_eq!(blocks(cx, &view), 2);
    render(cx, handle);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(("text-anchor-left", 2usize)).is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
async fn escape_cancels_aligning_first(cx: &mut TestAppContext) {
    let (_dir, handle, view) = start_aligning(cx).await;
    view.update(cx, |v, cx| v.select_rows_for_test(true, 1, 2, cx));
    press(cx, handle, "escape");
    assert_eq!(view.read_with(cx, |v, _| v.aligning()), None);
    assert!(selected_rows(cx, &view).is_some(), "the selection stays");
    press(cx, handle, "escape");
    assert!(selected_rows(cx, &view).is_none(), "the next Esc clears it");
}

#[gpui_kit::test]
async fn a_click_on_the_same_side_cancels_with_a_notice(cx: &mut TestAppContext) {
    let (_dir, handle, view) = start_aligning(cx).await;
    // A real press on the left half of row 0 (a hatched filler).
    cx.update_window(handle, |_, window, cx| {
        window.click(("text-filler-left", 0usize), cx)
    })
    .unwrap();
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Click a line on the other side.")
    );
    assert_eq!(view.read_with(cx, |v, _| v.aligning()), None);
    assert!(anchors(cx, &view).is_empty());
}

#[gpui_kit::test]
async fn a_click_on_the_other_sides_line_number_finishes_aligning(cx: &mut TestAppContext) {
    // X-91: the number cell finishes the alignment exactly like a click on the text.
    let (_dir, handle, view) = start_aligning(cx).await;
    let picked = selected_rows(cx, &view);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        // Right "c" (line 1) on display row 0: its number cell starts after the centre gutter.
        let gutter = window.find(("text-gutter", 0usize)).bounds();
        let at = gpui_kit::point(
            gutter.right() + gpui_kit::px(6.),
            gutter.top() + gpui_kit::px(8.),
        );
        pointer(window, button(at, true), cx);
        pointer(window, button(at, false), cx);
    })
    .unwrap();
    assert_eq!(view.read_with(cx, |v, _| v.aligning()), None);
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Aligned left line 3 with right line 1.")
    );
    assert_eq!(selected_rows(cx, &view), picked, "the press picks no rows");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(anchors(cx, &view), vec![(2, 0)]);
}

#[gpui_kit::test]
#[ignore = "writes a 65 MiB file; run once in the final round"]
async fn a_pin_with_a_side_over_64_mib_says_rules_are_not_applied(cx: &mut TestAppContext) {
    // X-96: a pin counts as a rule, so a side over 64 MiB gives the note at once.
    let line = format!("{}\n", "x".repeat(99));
    let big = line.repeat(65 * 1024 * 1024 / line.len() + 1);
    let small = line.repeat(10);
    let (_dir, handle, view) = open_text(cx, &big, &small);
    drop(big);
    wait_loaded(cx, handle, &view).await;
    right_click_row(cx, handle, 2);
    set_menu_row(cx, &view, 2, true);
    assert!(run_item(cx, handle, &view, ALIGN_ITEM));
    view.update(cx, |v, cx| v.finish_align(5, false, cx));
    let note = "Rules not applied: a file is over 64 MiB.";
    assert_eq!(notice(cx, &view).as_deref(), Some(note), "at once");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(anchors(cx, &view), vec![(2, 5)]);
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some(note),
        "the compare landed"
    );
    let status_now = status(cx, &view);
    assert!(status_now.ends_with(&format!(" · {note}")), "{status_now}");
}

#[gpui_kit::test]
async fn a_click_on_a_placeholder_half_cancels_with_a_notice(cx: &mut TestAppContext) {
    // Started on right line 0 (display row 0), whose left half is a filler.
    let (_dir, handle, view) = start_aligning_at(cx, 0, false).await;
    cx.update_window(handle, |_, window, cx| {
        window.click(("text-filler-left", 0usize), cx)
    })
    .unwrap();
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("That side has no line here.")
    );
    assert_eq!(view.read_with(cx, |v, _| v.aligning()), None);
    assert!(anchors(cx, &view).is_empty());
}

#[gpui_kit::test]
async fn f9_swaps_the_anchors_and_f5_keeps_them(cx: &mut TestAppContext) {
    let (_dir, handle, view) = start_aligning(cx).await;
    view.update(cx, |v, cx| v.finish_align(0, false, cx));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(anchors(cx, &view), vec![(2, 0)]);
    press(cx, handle, "f9");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(anchors(cx, &view), vec![(0, 2)]);
    assert_eq!(blocks(cx, &view), 2);
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(anchors(cx, &view), vec![(0, 2)]);
    assert_eq!(blocks(cx, &view), 2, "still aligned after the reload");
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("x.txt"), dir.path().join("y.txt"));
    std::fs::write(&a, "1\n").unwrap();
    std::fs::write(&b, "2\n").unwrap();
    view.update(cx, |v, cx| v.load_paths(a, b, cx));
    wait_loaded(cx, handle, &view).await;
    assert!(anchors(cx, &view).is_empty(), "new paths start clean");
}

#[gpui_kit::test]
async fn an_edit_drops_the_pins_past_the_end(cx: &mut TestAppContext) {
    let (_dir, handle, view) = start_aligning(cx).await;
    view.update(cx, |v, cx| v.finish_align(0, false, cx));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(anchors(cx, &view), vec![(2, 0)]);
    // The left side loses its last line, and with it the pin on that line (R11).
    let len = view.read_with(cx, |v, _| v.model().unwrap().left().bytes().len());
    view.update(cx, |v, cx| assert!(v.replace_on(true, 0..len, "a\n", cx)));
    wait_loaded(cx, handle, &view).await;
    assert!(anchors(cx, &view).is_empty(), "{:?}", anchors(cx, &view));
}

// ---- Fix wave: rules shared by every tab, invalid lists, lexing while typing ----

#[gpui_kit::test]
async fn rules_are_shared_by_open_tabs(cx: &mut TestAppContext) {
    let data = use_settings(cx);
    let (_a_dir, a_handle, a) = open_text(cx, "x\n", "x\n");
    let (_b_dir, b_handle, b) = open_text(cx, "# c\nx\n", "x\n");
    wait_loaded(cx, a_handle, &a).await;
    wait_loaded(cx, b_handle, &b).await;
    assert_eq!(blocks(cx, &b), 1);
    a.update(cx, |v, cx| v.set_rules_for_test("", "^#", cx));
    wait_loaded(cx, a_handle, &a).await;
    // The other tab's panel shows the list the first one saved.
    open_rules(cx, b_handle, &b);
    let lines = b.read_with(cx, |v, cx| v.rules_lines_for_test(cx).unwrap());
    assert_eq!(lines, "^#");
    cx.update_window(b_handle, |_, window, cx| {
        b.update(cx, |v, cx| v.cancel_rules(window, cx))
    })
    .unwrap();
    // A line rule added there joins that list instead of replacing it with an old copy.
    right_click_row(cx, b_handle, 1);
    set_menu_row(cx, &b, 1, true);
    assert!(run_item(
        cx,
        b_handle,
        &b,
        "Treat lines like this as unimportant"
    ));
    wait_loaded(cx, b_handle, &b).await;
    assert_eq!(saved_setting(&data, "text.rules_lines"), "^#\n^x$");
    assert_eq!(blocks(cx, &b), 0, "its compare uses both rules");
    open_rules(cx, a_handle, &a);
    let lines = a.read_with(cx, |v, cx| v.rules_lines_for_test(cx).unwrap());
    assert_eq!(lines, "^#\n^x$");
}

#[gpui_kit::test]
async fn on_save_choices_are_shared_by_open_tabs(cx: &mut TestAppContext) {
    let _data = use_settings(cx);
    let (a_dir, a_handle, a) = open_text(cx, "a\r\n", "a\r\n");
    let (b_dir, b_handle, b) = open_text(cx, "x\r\ny\r\n", "x\r\ny\r\n");
    wait_loaded(cx, a_handle, &a).await;
    wait_loaded(cx, b_handle, &b).await;
    a.update(cx, |v, cx| v.set_save_eol(SaveEol::Lf, cx));
    b.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(b_handle, |_, window, cx| window.input("z", cx))
        .unwrap();
    press(cx, b_handle, "ctrl-s");
    wait_clean(cx, &b).await;
    assert_eq!(
        std::fs::read(b_dir.path().join("a.txt")).unwrap(),
        b"xz\ny\n"
    );
    // A choice made in the second tab keeps the first tab's.
    b.update(cx, |v, cx| v.set_save_encoding(SaveEncoding::Utf8Bom, cx));
    a.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(a_handle, |_, window, cx| window.input("b", cx))
        .unwrap();
    press(cx, a_handle, "ctrl-s");
    wait_clean(cx, &a).await;
    assert_eq!(
        std::fs::read(a_dir.path().join("a.txt")).unwrap(),
        b"\xEF\xBB\xBFab\n"
    );
}

#[gpui_kit::test]
async fn an_invalid_list_turns_its_field_red(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    let p = Palette::light();
    let borders = |cx: &mut TestAppContext| {
        view.read_with(cx, |v, cx| {
            (
                v.rules_border_for_test(true, cx),
                v.rules_border_for_test(false, cx),
            )
        })
    };
    open_rules(cx, handle, &view);
    assert_eq!(borders(cx), (p.line, p.line));
    set_fields(cx, handle, &view, ("(oops", "", "3"));
    assert!(!apply_rules(cx, handle, &view));
    assert_eq!(borders(cx), (p.del, p.line), "the text list");
    set_fields(cx, handle, &view, ("", "[z", "3"));
    assert!(!apply_rules(cx, handle, &view));
    assert_eq!(borders(cx), (p.line, p.del), "the lines list");
    render(cx, handle);
}

#[gpui_kit::test]
async fn a_typing_burst_relexes_once_after_the_pause(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_named(cx, "a.ini", "[s]\nk=1\n", "[s]\nk=1\n");
    wait_loaded(cx, handle, &view).await;
    wait_syntax(cx, &view, true).await;
    let starts =
        |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.syntax_starts_for_test(true));
    let before = starts(cx);
    view.update(cx, |v, cx| v.set_caret_for_test(true, 5, cx));
    for _ in 0..10 {
        cx.update_window(handle, |_, window, cx| window.input("x", cx))
            .unwrap();
    }
    assert_eq!(
        starts(cx),
        before,
        "no lex starts while the keys come (PF10)"
    );
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    assert_eq!(starts(cx), before + 1, "one lex after the pause");
    wait_syntax(cx, &view, true).await;
    assert!(!view.read_with(cx, |v, _| v.line_styles_for_test(true, 1).is_empty()));
}

#[gpui_kit::test]
async fn saved_rules_open_with_a_row_for_each_line(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\nb\n", "a\nB\n");
    wait_loaded(cx, handle, &view).await;
    render(cx, handle);
    // No `render` after opening: the frame that made the field is the one to read.
    let open = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |v, cx| v.open_rules(window, cx))
        })
        .unwrap();
        cx.update_window(handle, |_, window, _| {
            window.find("text-rules-text").bounds().size.height
        })
        .unwrap()
    };
    let empty = open(cx);
    set_fields(cx, handle, &view, ("a\nb\nc\nd\ne\nf", "", "3"));
    assert!(apply_rules(cx, handle, &view), "the patterns are valid");
    assert!(
        view.read_with(cx, |v, _| !v.rules_open()),
        "Apply closes it"
    );
    render(cx, handle);
    let six_lines = open(cx);
    assert!(
        six_lines > empty,
        "six saved rules must not open clipped to the three rows: {six_lines:?} {empty:?}"
    );
}

#[gpui_kit::test]
async fn turning_sort_keys_on_drops_the_manual_alignments(cx: &mut TestAppContext) {
    let (_dir, handle, view) = start_aligning(cx).await;
    view.update(cx, |v, cx| v.finish_align(0, false, cx));
    wait_loaded(cx, handle, &view).await;
    assert_eq!(anchors(cx, &view), vec![(2, 0)]);
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.set_sort_keys(true, window, cx))
    })
    .unwrap();
    wait_loaded(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.request().sort_keys));
    assert!(
        anchors(cx, &view).is_empty(),
        "a pin names a line of the unsorted text"
    );
}
