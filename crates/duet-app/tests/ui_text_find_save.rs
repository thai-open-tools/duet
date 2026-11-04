//! Headless UI tests for the text view's find and replace and its saves (P9b Tasks 7 and 8):
//! whole word, regex, Replace all, the On save choices, encodings and Reformat.

mod text_support;

use std::path::Path;

use duet_app::palette::Palette;
use duet_app::text::prefs::{SaveEncoding, SaveEol};
use duet_app::text::view::TextView;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AnyWindowHandle, AppContext, Entity, TestAppContext};
use text_support::*;

// ---- Find: whole word, regex, replace (Task 7) ----

/// Opens the find bar and types `text` into its field.
fn find_for(cx: &mut TestAppContext, handle: AnyWindowHandle, text: &str) {
    press(cx, handle, "ctrl-f");
    cx.update_window(handle, |_, window, cx| window.input(text, cx))
        .unwrap();
}

#[gpui_kit::test]
async fn whole_word_and_regex_options_rerun_the_search(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "cat concat\n", "cat concat\n");
    wait_loaded(cx, handle, &view).await;
    find_for(cx, handle, "cat");
    assert_eq!(wait_found(cx, handle, &view).await, "4 matches");
    view.update(cx, |v, cx| v.toggle_whole_word(cx));
    assert_eq!(wait_found(cx, handle, &view).await, "2 matches");
    view.update(cx, |v, cx| v.toggle_whole_word(cx));
    view.update(cx, |v, cx| v.toggle_regex(cx));
    view.update(cx, |v, cx| v.set_find_text("c.t".into(), cx));
    assert_eq!(wait_found(cx, handle, &view).await, "4 matches");
    view.update(cx, |v, cx| v.set_find_text("(".into(), cx));
    assert_eq!(wait_found(cx, handle, &view).await, "Invalid pattern");
    assert!(view.read_with(cx, |v, _| v.find_error().is_some()));
    view.update(cx, |v, cx| v.set_find_text("c.t".into(), cx));
    wait_found(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.find_error().map(str::to_owned)),
        None
    );
    // The bar offers the controls.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for id in [
            "text-find-word",
            "text-find-regex",
            "text-find-replace",
            "text-replace-one",
            "text-replace-all",
        ] {
            assert!(window.try_find(id).is_some(), "{id}");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
async fn replace_one_edits_the_current_match_and_moves_on(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "z\n", "a=1\na=2\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_find_text("a=(\\d)".into(), cx));
    find_for(cx, handle, "");
    view.update(cx, |v, cx| {
        v.toggle_regex(cx);
        v.set_find_text("a=(\\d)".into(), cx);
    });
    wait_found(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_replace_text_for_test("b=$1", cx));
    press(cx, handle, "f3");
    assert_eq!(
        view.read_with(cx, |v, _| v.find_current_for_test()),
        Some((false, 0))
    );
    click(cx, handle, "text-replace-one");
    assert_eq!(right_text(cx, &view), "b=1\na=2\n");
    wait_found(cx, handle, &view).await;
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.find_current_for_test()),
        Some((false, 1)),
        "the search ran again and the next match is current"
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.find_status()).as_deref(),
        Some("1 of 1")
    );
    view.update(cx, |v, cx| v.set_caret_for_test(false, 0, cx));
    focus_view(cx, handle, &view);
    press(cx, handle, "ctrl-z");
    assert_eq!(right_text(cx, &view), "a=1\na=2\n");
}

#[gpui_kit::test]
async fn replace_one_says_so_when_the_text_moved(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "z\n", "a=1\na=2\n");
    wait_loaded(cx, handle, &view).await;
    find_for(cx, handle, "a=1");
    wait_found(cx, handle, &view).await;
    press(cx, handle, "f3");
    // The line changes under the match before Replace.
    view.update(cx, |v, cx| v.replace_on(false, 0..1, "q", cx));
    view.update(cx, |v, cx| v.set_replace_text_for_test("b", cx));
    click(cx, handle, "text-replace-one");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("The text here changed: the search ran again. Press Replace once more.")
    );
    assert_eq!(right_text(cx, &view), "q=1\na=2\n");
}

#[gpui_kit::test]
async fn replace_all_is_one_undo_step_per_side(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "x x\nx\n", "x x\nx\n");
    wait_loaded(cx, handle, &view).await;
    find_for(cx, handle, "x");
    wait_found(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_replace_text_for_test("y", cx));
    click(cx, handle, "text-replace-all");
    wait_notice(cx, handle, &view, "Replaced 6 matches.").await;
    assert_eq!(left_text(cx, &view), "y y\ny\n");
    assert_eq!(right_text(cx, &view), "y y\ny\n");
    view.update(cx, |v, cx| v.set_caret_for_test(false, 0, cx));
    focus_view(cx, handle, &view);
    press(cx, handle, "ctrl-z");
    let restored = [left_text(cx, &view), right_text(cx, &view)]
        .iter()
        .filter(|t| *t == "x x\nx\n")
        .count();
    assert_eq!(restored, 1, "one undo restores one side");
    press(cx, handle, "ctrl-z");
    assert_eq!(left_text(cx, &view), "x x\nx\n");
    assert_eq!(right_text(cx, &view), "x x\nx\n");
    // With the scope on Left only the left changes.
    view.update(cx, |v, cx| {
        v.set_find_scope(duet_app::text::find::FindScope::Left, cx)
    });
    view.update(cx, |v, cx| {
        assert!(v.replace_all("y", cx));
    });
    wait_notice(cx, handle, &view, "Replaced 3 matches.").await;
    assert_eq!(left_text(cx, &view), "y y\ny\n");
    assert_eq!(right_text(cx, &view), "x x\nx\n");
}

#[gpui_kit::test]
async fn replace_all_says_when_nothing_matches_or_changes(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "x\n", "x\n");
    wait_loaded(cx, handle, &view).await;
    find_for(cx, handle, "q");
    wait_found(cx, handle, &view).await;
    view.update(cx, |v, cx| v.replace_all("z", cx));
    wait_notice(cx, handle, &view, "No matches to replace.").await;
    view.update(cx, |v, cx| v.set_find_text("x".into(), cx));
    wait_found(cx, handle, &view).await;
    view.update(cx, |v, cx| v.replace_all("x", cx));
    wait_notice(
        cx,
        handle,
        &view,
        "2 matches, but the replacement changes nothing.",
    )
    .await;
    assert_eq!(left_text_opt(cx, &view), None);
}

#[gpui_kit::test]
async fn replace_spares_a_read_only_side_and_says_why(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_named(cx, "x.json", "{\"a\":1}", "{\"a\":2}");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_format_for_test(true, cx));
    wait_loaded(cx, handle, &view).await;
    find_for(cx, handle, "a");
    wait_found(cx, handle, &view).await;
    view.update(cx, |v, cx| v.replace_all("b", cx));
    cx.run_until_parked();
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("The formatted view is read-only. Turn Format off to edit.")
    );
    assert_eq!(left_text_opt(cx, &view), None);
    assert_eq!(right_text_opt(cx, &view), None);
}

#[gpui_kit::test]
async fn replace_all_keeps_the_caret_and_the_scroll_position(cx: &mut TestAppContext) {
    let (left, right) = pair_300();
    let (_dir, handle, view) = open_text(cx, &left, &right);
    wait_loaded(cx, handle, &view).await;
    find_for(cx, handle, "9");
    view.update(cx, |v, cx| {
        v.set_find_scope(duet_app::text::find::FindScope::Right, cx)
    });
    wait_found(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(false, 3, cx));
    view.update(cx, |v, cx| v.scroll_like_the_user_for_test(100, cx));
    let top = top_row(cx, handle, &view);
    view.update(cx, |v, cx| v.replace_all("nine", cx));
    wait_notice(cx, handle, &view, "Replaced 60 matches.").await;
    assert!(right_text(cx, &view).contains("line nine\n"));
    assert_eq!(left_text_opt(cx, &view), None, "the left side stays");
    let caret = view.read_with(cx, |v, _| v.caret().unwrap());
    assert_eq!((caret.left, caret.head, caret.anchor), (false, 3, 3));
    assert_eq!(top_row(cx, handle, &view), top);
}

#[gpui_kit::test]
async fn typing_during_replace_all_is_not_overwritten(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "x x\nx\n", "x x\nx\n");
    wait_loaded(cx, handle, &view).await;
    find_for(cx, handle, "x");
    wait_found(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        assert!(v.replace_all("y", cx));
        // A keystroke lands before the background build does.
        assert!(v.replace_on(false, 0..0, "Z", cx));
    });
    cx.run_until_parked();
    assert_eq!(right_text(cx, &view), "Zx x\nx\n", "the typed text stays");
    assert_eq!(left_text_opt(cx, &view), None, "nothing was replaced");
    let said = notice(cx, &view);
    assert!(
        said.as_deref()
            .is_some_and(|n| n.contains("changed while replacing")),
        "{said:?}"
    );
}

#[gpui_kit::test]
async fn the_find_box_starts_with_the_selected_text(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "hello abc world\nsecond\n", "hello\nother\n");
    wait_loaded(cx, handle, &view).await;
    let field = |cx: &mut TestAppContext, view: &Entity<TextView>| {
        let input = view.read_with(cx, |v, _| v.find_input_for_test()).unwrap();
        input.read_with(cx, |i, _| i.value().to_string())
    };
    // A caret selection of "abc".
    view.update(cx, |v, cx| v.set_caret_for_test(true, 6, cx));
    focus_view(cx, handle, &view);
    for _ in 0..3 {
        press(cx, handle, "shift-right");
    }
    press(cx, handle, "ctrl-f");
    assert_eq!(field(cx, &view), "abc");
    press(cx, handle, "escape");
    // A selection across a line break leaves the box empty.
    view.update(cx, |v, cx| v.set_caret_for_test(true, 6, cx));
    focus_view(cx, handle, &view);
    for _ in 0..12 {
        press(cx, handle, "shift-right");
    }
    press(cx, handle, "ctrl-f");
    assert_eq!(field(cx, &view), "");
    press(cx, handle, "escape");
    // A one-row row selection gives its line.
    view.update(cx, |v, cx| v.select_rows_for_test(false, 0, 0, cx));
    focus_view(cx, handle, &view);
    press(cx, handle, "ctrl-f");
    assert_eq!(field(cx, &view), "hello");
}

#[gpui_kit::test]
async fn the_prefilled_find_text_is_selected(cx: &mut TestAppContext) {
    // X-103: the prefill is selected, so typing replaces it.
    let (_dir, handle, view) = open_text(cx, "tail line\nx\n", "tail line\ny\n");
    wait_loaded(cx, handle, &view).await;
    let input =
        |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.find_input_for_test()).unwrap();
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    focus_view(cx, handle, &view);
    for _ in 0..9 {
        press(cx, handle, "shift-right");
    }
    press(cx, handle, "ctrl-f");
    let field = input(cx);
    assert_eq!(
        field.read_with(cx, |i, _| i.value().to_string()),
        "tail line"
    );
    assert_eq!(
        field.read_with(cx, |i, _| i.selected_range()),
        0..9,
        "the prefill is selected"
    );
    press(cx, handle, "x");
    assert_eq!(
        field.read_with(cx, |i, _| i.value().to_string()),
        "x",
        "typing replaces it"
    );
    // Ctrl+F again with a one-row selection selects the new prefill in the open bar.
    press(cx, handle, "escape");
    view.update(cx, |v, cx| v.select_rows_for_test(false, 1, 1, cx));
    focus_view(cx, handle, &view);
    press(cx, handle, "ctrl-f");
    let field = input(cx);
    assert_eq!(field.read_with(cx, |i, _| i.value().to_string()), "y");
    assert_eq!(field.read_with(cx, |i, _| i.selected_range()), 0..1);
}

// ---- Task 8: save conversions, encodings, the UTF-8 fallback, Reformat ----

fn info(cx: &mut TestAppContext, view: &Entity<TextView>, left: bool) -> String {
    view.read_with(cx, |v, _| {
        let model = v.model().unwrap();
        duet_app::text::model::doc_info(if left { model.left() } else { model.right() })
    })
}

#[gpui_kit::test]
async fn shift_jis_and_gbk_can_be_chosen_to_read_a_file(cx: &mut TestAppContext) {
    for (bytes, encoding, line) in [
        (&b"\x82\xa0=1\n"[..], encoding_rs::SHIFT_JIS, "あ=1"),
        (&b"\xc4\xe3\n"[..], encoding_rs::GBK, "你"),
    ] {
        let (_dir, handle, view) = open_text_bytes(cx, bytes, bytes);
        wait_loaded(cx, handle, &view).await;
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |v, cx| v.set_encoding(true, Some(encoding), window, cx))
        })
        .unwrap();
        wait_loaded(cx, handle, &view).await;
        let got = view.read_with(cx, |v, _| v.model().unwrap().left().line(0).into_owned());
        assert_eq!(got, line, "{}", encoding.name());
    }
    assert_eq!(
        duet_app::text::job::encoding_label(Some(encoding_rs::GBK)),
        "Chinese (GBK)"
    );
    assert_eq!(
        duet_app::text::job::encoding_label(Some(encoding_rs::SHIFT_JIS)),
        "Japanese (Shift_JIS)"
    );
}

#[gpui_kit::test]
async fn on_save_line_endings_convert_the_written_file(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\r\nb\r\n", "a\r\nb\r\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_save_eol(SaveEol::Lf, cx));
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("c", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    wait_clean(cx, &view).await;
    let path = dir.path().join("a.txt");
    assert_eq!(std::fs::read(&path).unwrap(), b"ac\nb\n");
    assert!(
        info(cx, &view, true).contains("LF"),
        "PF5: the document takes the new style"
    );

    // A later save that keeps the style does not convert back.
    view.update(cx, |v, cx| v.set_save_eol(SaveEol::Keep, cx));
    view.update(cx, |v, cx| v.set_caret_for_test(true, 2, cx));
    cx.update_window(handle, |_, window, cx| window.input("d", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    wait_clean(cx, &view).await;
    assert_eq!(std::fs::read(&path).unwrap(), b"acd\nb\n");

    // F5 reads the new style, and the undo history made in the old text is gone.
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    assert!(info(cx, &view, true).contains("LF"));
    assert!(!info(cx, &view, true).contains("CRLF"));
    press(cx, handle, "ctrl-z");
    assert!(
        !view.read_with(cx, |v, _| v.is_dirty()),
        "Ctrl+Z after F5 changes nothing"
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"acd\nb\n");
}

#[gpui_kit::test]
async fn on_save_encoding_writes_utf8_with_a_bom(cx: &mut TestAppContext) {
    let thai: &[u8] = &[0xE4, 0xB7, 0xC2, 0x0A]; // "ไทย\n" in windows-874
    let (dir, handle, view) = open_text_bytes(cx, thai, thai);
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.set_encoding(true, Some(encoding_rs::WINDOWS_874), window, cx)
        })
    })
    .unwrap();
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_save_encoding(SaveEncoding::Utf8Bom, cx));
    view.update(cx, |v, cx| v.set_caret_for_test(true, "ไทย".len(), cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    wait_clean(cx, &view).await;
    let bytes = std::fs::read(dir.path().join("a.txt")).unwrap();
    assert_eq!(&bytes[..3], [0xEF, 0xBB, 0xBF]);
    assert_eq!(std::str::from_utf8(&bytes[3..]).unwrap(), "ไทยx\n");
    let left = info(cx, &view, true);
    assert!(left.contains("UTF-8") && left.contains("BOM"), "{left}");
}

#[gpui_kit::test]
async fn on_save_utf16le_always_writes_a_bom(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_save_encoding(SaveEncoding::Utf16Le, cx));
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("b", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    wait_clean(cx, &view).await;
    assert_eq!(
        std::fs::read(dir.path().join("a.txt")).unwrap(),
        [0xFF, 0xFE, b'a', 0, b'b', 0, b'\n', 0]
    );
}

#[gpui_kit::test]
async fn a_character_the_encoding_cannot_hold_offers_utf8(cx: &mut TestAppContext) {
    let thai: &[u8] = &[0xA1, 0x0A]; // "ก\n" in windows-874
    let (dir, handle, view) = open_text_bytes(cx, thai, thai);
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
    let path = dir.path().join("a.txt");

    press(cx, handle, "ctrl-s");
    wait_for_prompt(cx).await;
    let (message, detail) = cx.pending_prompt().unwrap();
    assert_eq!(message, "a.txt can't be saved as windows-874.");
    assert_eq!(
        detail,
        "It has a character windows-874 can't hold. Save it as UTF-8 instead? The file's encoding changes."
    );
    cx.simulate_prompt_answer("Cancel");
    let said = wait_new_notice(cx, &view, None).await;
    assert_eq!(
        said,
        "Not saved: a.txt has a character windows-874 can't hold."
    );
    assert_eq!(std::fs::read(&path).unwrap(), thai, "the file is unchanged");
    assert!(view.read_with(cx, |v, _| v.is_dirty()));

    press(cx, handle, "ctrl-s");
    wait_for_prompt(cx).await;
    cx.simulate_prompt_answer("Save as UTF-8");
    wait_clean(cx, &view).await;
    assert_eq!(std::fs::read(&path).unwrap(), "ก😀\n".as_bytes());
    let left = info(cx, &view, true);
    assert!(left.contains("UTF-8") && !left.contains("BOM"), "{left}");
}

#[gpui_kit::test]
async fn reformat_tidies_both_sides_as_edits(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_files(
        cx,
        (Path::new("x.json"), b"{\"a\":1}\r\n"),
        (Path::new("y.txt"), b"x  \r\ny\r\n"),
    );
    wait_loaded(cx, handle, &view).await;
    let json = duet_core::text::format::reformat("{\"a\":1}\r\n")
        .unwrap()
        .text;
    cx.update_window(handle, |_, window, cx| window.click("text-reformat", cx))
        .unwrap();
    let said = wait_new_notice(cx, &view, None).await;
    assert_eq!(
        said,
        "Left: re-indented JSON · Right: trimmed trailing spaces."
    );
    assert_eq!(left_text(cx, &view), json);
    assert_eq!(right_text(cx, &view), "x\r\ny\r\n");
    assert!(view.read_with(cx, |v, _| v.is_dirty()));

    // The keys edit once a caret is placed, as a click in the text does.
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    focus_view(cx, handle, &view);
    for _ in 0..2 {
        wait_loaded(cx, handle, &view).await;
        press(cx, handle, "ctrl-z");
    }
    assert_eq!(left_text(cx, &view), "{\"a\":1}\r\n");
    assert_eq!(right_text(cx, &view), "x  \r\ny\r\n");
    for _ in 0..2 {
        wait_loaded(cx, handle, &view).await;
        press(cx, handle, "ctrl-y");
    }
    assert_eq!(left_text(cx, &view), json);
    assert_eq!(right_text(cx, &view), "x\r\ny\r\n");

    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.click("text-reformat", cx))
        .unwrap();
    let said = wait_new_notice(cx, &view, Some(said)).await;
    assert_eq!(said, "Nothing to reformat: both sides are already tidy.");
}

// ---- Fix wave: plans that outlive their text, converting saves ----

const REPLACE_STOPPED: &str =
    "The text changed while replacing, so nothing was replaced. Press Replace all again.";
const REFORMAT_STOPPED: &str =
    "The text changed while reformatting, so nothing was changed. Press Reformat again.";

fn forced(cx: &mut TestAppContext, view: &Entity<TextView>, left: bool) -> Option<&'static str> {
    view.read_with(cx, |v, _| {
        let request = v.request();
        let encoding = if left {
            request.left_encoding
        } else {
            request.right_encoding
        };
        encoding.map(|e| e.name())
    })
}

#[gpui_kit::test]
async fn replace_all_and_reformat_are_dropped_by_a_swap_or_a_reload(cx: &mut TestAppContext) {
    // Replace all, then F9 before its plan lands: the plan was made from the other file.
    let (dir, handle, view) = open_text(cx, "x x\n", "abc\n");
    wait_loaded(cx, handle, &view).await;
    find_for(cx, handle, "x");
    wait_found(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        assert!(v.replace_all("y", cx));
        v.swap_sides(cx);
    });
    cx.run_until_parked();
    assert_eq!(left_text_opt(cx, &view), None, "F9");
    assert_eq!(right_text_opt(cx, &view), None, "F9");
    assert_eq!(notice(cx, &view).as_deref(), Some(REPLACE_STOPPED));
    // Replace all, then F5.
    view.update(cx, |v, cx| {
        assert!(v.replace_all("y", cx));
        v.reload(cx);
    });
    wait_loaded(cx, handle, &view).await;
    cx.run_until_parked();
    assert_eq!(left_text_opt(cx, &view), None, "F5");
    assert_eq!(right_text_opt(cx, &view), None, "F5");
    assert_eq!(notice(cx, &view).as_deref(), Some(REPLACE_STOPPED));
    for name in ["a.txt", "b.txt"] {
        let text = std::fs::read_to_string(dir.path().join(name)).unwrap();
        assert!(
            text == "x x\n" || text == "abc\n",
            "{name} unchanged: {text:?}"
        );
    }
    // Reformat, then F9.
    let (_dir, handle, view) = open_text(cx, "a  \n", "bcd\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.reformat_sides(cx);
        v.swap_sides(cx);
    });
    cx.run_until_parked();
    assert_eq!(left_text_opt(cx, &view), None, "reformat, F9");
    assert_eq!(right_text_opt(cx, &view), None, "reformat, F9");
    assert_eq!(notice(cx, &view).as_deref(), Some(REFORMAT_STOPPED));
}

#[gpui_kit::test]
async fn replace_all_and_reformat_index_nothing_on_the_ui_thread(cx: &mut TestAppContext) {
    use duet_app::text::model::snapshots_finished_here;
    let (_dir, handle, view) = open_text(cx, "x  \n", "x  \n");
    wait_loaded(cx, handle, &view).await;
    find_for(cx, handle, "x");
    wait_found(cx, handle, &view).await;
    // An edited side: its text is copied, and its lines are indexed in the background (PF16).
    view.update(cx, |v, cx| assert!(v.replace_on(false, 0..0, "z", cx)));
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| assert!(v.replace_on(false, 0..0, "z", cx)));
    let before = snapshots_finished_here();
    view.update(cx, |v, cx| assert!(v.replace_all("y", cx)));
    assert_eq!(snapshots_finished_here(), before, "Replace all");
    wait_notice(cx, handle, &view, "Replaced 2 matches.").await;
    let before = snapshots_finished_here();
    view.update(cx, |v, cx| v.reformat_sides(cx));
    assert_eq!(snapshots_finished_here(), before, "Reformat");
    let said = wait_new_notice(cx, &view, Some("Replaced 2 matches.".into())).await;
    assert_eq!(
        said,
        "Left: trimmed trailing spaces · Right: trimmed trailing spaces."
    );
}

#[gpui_kit::test]
async fn a_side_that_cannot_take_its_edit_is_named(cx: &mut TestAppContext) {
    use duet_app::text::replace::ReplacePlan;
    let (_dir, handle, view) = open_text(cx, "x\n", "x\n");
    wait_loaded(cx, handle, &view).await;
    find_for(cx, handle, "x");
    wait_found(cx, handle, &view).await;
    let plan = |range: std::ops::Range<usize>, with: &str| ReplacePlan {
        matches: 1,
        edit: Some((range, with.to_owned())),
    };
    view.update(cx, |v, cx| {
        v.apply_replace_plans_for_test(vec![(true, plan(0..1, "y")), (false, plan(5..9, "z"))], cx)
    });
    assert_eq!(left_text(cx, &view), "y\n");
    assert_eq!(
        notice(cx, &view).as_deref(),
        Some("Replaced 1 match on the left side only; the right side could not be changed.")
    );
    assert_eq!(
        wait_found(cx, handle, &view).await,
        "1 match",
        "the search ran again"
    );
}

#[gpui_kit::test]
async fn an_invalid_find_pattern_turns_the_field_red(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    let border = |cx: &mut TestAppContext| view.read_with(cx, |v, cx| v.find_border_for_test(cx));
    find_for(cx, handle, "(");
    wait_found(cx, handle, &view).await;
    assert_eq!(border(cx), None, "a literal ( is a fine pattern");
    view.update(cx, |v, cx| v.toggle_regex(cx));
    assert_eq!(wait_found(cx, handle, &view).await, "Invalid pattern");
    assert_eq!(border(cx), Some(Palette::light().del));
    render(cx, handle);
    view.update(cx, |v, cx| v.set_find_text("a".into(), cx));
    wait_found(cx, handle, &view).await;
    assert_eq!(border(cx), None);
}

#[gpui_kit::test]
async fn a_converting_save_moves_the_forced_read_as(cx: &mut TestAppContext) {
    // Forced windows-874, then the UTF-8 fallback: F5 reads the UTF-8 that was written (K1).
    let thai: &[u8] = &[0xA1, 0x0A]; // "ก\n" in windows-874
    let (dir, handle, view) = open_text_bytes(cx, thai, b"z\n");
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
    cx.simulate_prompt_answer("Save as UTF-8");
    wait_clean(cx, &view).await;
    assert_eq!(
        std::fs::read(dir.path().join("a.txt")).unwrap(),
        "ก😀\n".as_bytes()
    );
    assert_eq!(forced(cx, &view, true), Some("UTF-8"));
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let model = v.model().unwrap();
        assert_eq!(model.left().text(), Some("ก😀\n"));
        assert!(model.editable(true).is_ok(), "decoded cleanly");
    });
    press(cx, handle, "f9");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        forced(cx, &view, false),
        Some("UTF-8"),
        "the choice moves with its side"
    );
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v
            .model()
            .unwrap()
            .right()
            .text()
            .map(str::to_owned)),
        Some("ก😀\n".to_owned())
    );

    // Forced windows-874, On save UTF-8 with BOM.
    let thai: &[u8] = &[0xE4, 0xB7, 0xC2, 0x0A]; // "ไทย\n" in windows-874
    let (_dir, handle, view) = open_text_bytes(cx, thai, b"z\n");
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.set_encoding(true, Some(encoding_rs::WINDOWS_874), window, cx)
        })
    })
    .unwrap();
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_save_encoding(SaveEncoding::Utf8Bom, cx));
    view.update(cx, |v, cx| v.set_caret_for_test(true, "ไทย".len(), cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    wait_clean(cx, &view).await;
    assert_eq!(forced(cx, &view, true), Some("UTF-8"));
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    let shown = view.read_with(cx, |v, _| {
        v.model().unwrap().left().text().map(str::to_owned)
    });
    assert_eq!(shown.as_deref(), Some("ไทยx\n"));
    assert!(
        info(cx, &view, true).contains("BOM"),
        "{}",
        info(cx, &view, true)
    );

    // Auto, On save Thai: an encoding detection cannot tell is forced, so F5 reads it back.
    let (dir, handle, view) = open_text(cx, "ก\n", "z\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| {
        v.set_save_encoding(SaveEncoding::Windows874, cx)
    });
    view.update(cx, |v, cx| v.set_caret_for_test(true, "ก".len(), cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
    press(cx, handle, "ctrl-s");
    wait_clean(cx, &view).await;
    assert_eq!(
        std::fs::read(dir.path().join("a.txt")).unwrap(),
        [0xA1, b'x', 0x0A]
    );
    assert_eq!(forced(cx, &view, true), Some("windows-874"));
    assert_eq!(
        forced(cx, &view, false),
        None,
        "the other side stays on Auto"
    );
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    let shown = view.read_with(cx, |v, _| {
        v.model().unwrap().left().text().map(str::to_owned)
    });
    assert_eq!(shown.as_deref(), Some("กx\n"));
}

#[gpui_kit::test]
async fn a_cr_to_lf_save_clears_the_undo_history_on_f5(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "a\rb\r", "a\rb\r");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_save_eol(SaveEol::Lf, cx));
    view.update(cx, |v, cx| v.set_caret_for_test(true, 2, cx));
    focus_view(cx, handle, &view);
    press(cx, handle, "backspace");
    assert_eq!(left_text(cx, &view), "ab\r");
    press(cx, handle, "ctrl-s");
    wait_clean(cx, &view).await;
    let path = dir.path().join("a.txt");
    assert_eq!(std::fs::read(&path).unwrap(), b"ab\n");
    // The same length, but other breaks: the history made in the old text goes (M5).
    press(cx, handle, "f5");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(true, 0, cx));
    focus_view(cx, handle, &view);
    press(cx, handle, "ctrl-z");
    assert!(
        !view.read_with(cx, |v, _| v.is_dirty()),
        "Ctrl+Z after F5 changes nothing: {:?}",
        left_text_opt(cx, &view)
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"ab\n");
}

// ---- C23: the read-only attribute (X-46, X-101, X-112) ----

/// Sets or clears a file's read-only attribute.
fn set_read_only(path: &Path, on: bool) {
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_readonly(on);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[gpui_kit::test]
async fn a_side_with_the_read_only_attribute_is_read_only(cx: &mut TestAppContext) {
    // As a side over 64 MiB: typing, a moved line, Reformat and Replace skip it with its
    // reason, and nothing is saved.
    const REASON: &str = "This file is read-only (Windows attribute).";
    let dir = tempfile::tempdir().unwrap();
    let left = dir.path().join("data.json");
    let right = dir.path().join("ro").join("data.json");
    std::fs::create_dir_all(right.parent().unwrap()).unwrap();
    std::fs::write(&left, "{\"a\":1}\n").unwrap();
    let right_bytes = b"{\"a\":2}\n{\"a\":3}\n";
    std::fs::write(&right, right_bytes).unwrap();
    set_read_only(&right, true);
    let (a, b) = (left.clone(), right.clone());
    let (handle, view) = open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| TextView::new(a, b, cx));
        window.focus(&gpui_kit::Focusable::focus_handle(&view, cx), cx);
        view
    });
    wait_loaded(cx, handle, &view).await;
    let editable = view.read_with(cx, |v, _| v.model().unwrap().editable(false));
    assert_eq!(editable, Err(REASON.to_owned()));
    // Typing and a moved line.
    view.update(cx, |v, cx| assert!(!v.replace_on(false, 0..0, "x", cx)));
    assert_eq!(notice(cx, &view).as_deref(), Some(REASON), "typing");
    view.update(cx, |v, cx| v.show_notice("-", cx));
    view.update(cx, |v, cx| assert!(!v.move_line(false, 1, 0, cx)));
    assert_eq!(notice(cx, &view).as_deref(), Some(REASON), "a moved line");
    // Reformat re-indents the left side only.
    view.update(cx, |v, cx| v.reformat_sides(cx));
    let said = wait_new_notice(cx, &view, Some(REASON.to_owned())).await;
    assert_eq!(
        said,
        format!("Left: re-indented JSON. The right side was skipped: {REASON}")
    );
    // Replace all on both sides replaces the left only; Replace one on the right says why.
    find_for(cx, handle, "a");
    wait_found(cx, handle, &view).await;
    view.update(cx, |v, cx| assert!(v.replace_all("b", cx)));
    let said = wait_new_notice(cx, &view, Some(said)).await;
    assert_eq!(
        said,
        format!("Replaced 1 match. The right side was skipped: {REASON}")
    );
    assert!(left_text(cx, &view).contains("\"b\""));
    view.update(cx, |v, cx| {
        v.set_find_scope(duet_app::text::find::FindScope::Right, cx)
    });
    wait_found(cx, handle, &view).await;
    view.update(cx, |v, cx| assert!(!v.replace_one("b", cx)));
    assert_eq!(notice(cx, &view).as_deref(), Some(REASON), "Replace one");
    // The right side never changed, so there is nothing to save there.
    assert_eq!(right_text_opt(cx, &view), None);
    assert!(view.read_with(cx, |v, _| !v.save_enabled(false)));
    set_read_only(&right, false);
    assert_eq!(std::fs::read(&right).unwrap(), right_bytes);
}

#[gpui_kit::test]
async fn ctrl_shift_s_saves_the_caret_side_under_a_new_name(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_text(cx, "left\n", "right\n");
    wait_loaded(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_caret_for_test(false, 0, cx));
    press(cx, handle, "ctrl-shift-s");
    assert!(cx.did_prompt_for_new_path(), "it asks where to save");
    let out = dir.path().join("copy.txt");
    cx.simulate_new_path_selection(|_| Some(out.clone()));
    for _ in 0..2000 {
        if out.exists() {
            break;
        }
        cx.executor()
            .timer(std::time::Duration::from_millis(10))
            .await;
    }
    assert_eq!(std::fs::read(&out).unwrap(), b"right\n");
}
