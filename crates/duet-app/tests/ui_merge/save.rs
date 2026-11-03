//! Save, save to, export with markers, copy (Task 8).

use crate::common::*;

#[gpui_kit::test]
async fn save_writes_the_result_in_the_left_files_format(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, bytes: &[u8]| {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    };
    let l = write("left.txt", b"\xEF\xBB\xBFa\r\nb\r\nc\r\n");
    let b = write("base.txt", b"a\nb\nc\n");
    let r = write("right.txt", b"a\nB\nc\n");
    let out = dir.path().join("out.txt");
    let (handle, view) = open_files(cx, [&l, &b, &r], Some(out.clone())).await;
    click(cx, handle, ("merge-use-right", 1usize));
    assert!(dirty(cx, &view), "a choice is unsaved");
    // The result field has the keys: Ctrl+S still saves (R7).
    activate(cx, handle);
    focus_result(cx, handle, &view, 1);
    let field = result_field(cx, &view, 1);
    assert!(field_has_the_keys(cx, handle, &field));
    press(cx, handle, "ctrl-s");
    wait_notice(cx, handle, &view, "Saved the result").await;
    assert_eq!(
        std::fs::read(&out).unwrap(),
        b"\xEF\xBB\xBFa\r\nB\r\nc\r\n",
        "the left's BOM, line ending and final newline"
    );
    assert!(!dirty(cx, &view));
    assert_eq!(
        notice_of(cx, &view),
        Some(format!("Saved the result: {}.", out.display()))
    );
}

#[gpui_kit::test]
async fn save_without_an_output_asks_where_and_remembers(cx: &mut TestAppContext) {
    let (dir, l, b, r) = write_set("a\nX\nc\n", "a\nb\nc\n", "a\nb\nc\n");
    let (handle, view) = open_files(cx, [&l, &b, &r], None).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.output().map(Path::to_path_buf)),
        None
    );
    press(cx, handle, "ctrl-s");
    assert!(cx.did_prompt_for_new_path(), "no output: Save asks where");
    let chosen = dir.path().join("merged-here.txt");
    let (target, seen_dir) = (
        chosen.clone(),
        std::sync::Arc::new(std::sync::Mutex::new(None)),
    );
    let seen = seen_dir.clone();
    cx.simulate_new_path_selection(move |folder| {
        *seen.lock().unwrap() = Some(folder.to_path_buf());
        Some(target)
    });
    wait_notice(cx, handle, &view, "Saved the result").await;
    assert_eq!(seen_dir.lock().unwrap().as_deref(), Some(dir.path()));
    assert_eq!(std::fs::read_to_string(&chosen).unwrap(), "a\nX\nc\n");
    assert_eq!(
        view.read_with(cx, |v, _| v.output().map(Path::to_path_buf)),
        Some(chosen.clone())
    );

    // The second save writes to the same file with no dialog.
    std::fs::write(&chosen, "old").unwrap();
    press(cx, handle, "ctrl-s");
    assert!(!cx.did_prompt_for_new_path());
    let again = chosen.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, _| {
        std::fs::read_to_string(&again).is_ok_and(|text| text == "a\nX\nc\n")
    })
    .await;

    // Save result to... always asks, and a cancelled dialog writes nothing.
    std::fs::write(&chosen, "old").unwrap();
    click(cx, handle, "merge-save-as");
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&chosen).unwrap(), "old");
    click(cx, handle, "merge-save-as");
    let other = dir.path().join("other.txt");
    let target = other.clone();
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(move |_| Some(target));
    let seen = other.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, _| seen.exists())
        .await;
    wait_notice(cx, handle, &view, "Saved the result").await;
    assert_eq!(
        view.read_with(cx, |v, _| v.output().map(Path::to_path_buf)),
        Some(other),
        "a saved path becomes the output"
    );
}

#[gpui_kit::test]
async fn unresolved_conflicts_ask_before_markers_are_saved(cx: &mut TestAppContext) {
    let (dir, l, b, r) = f2_files();
    let out = dir.path().join("out.txt");
    let (handle, view) = open_files(cx, [&l, &b, &r], Some(out.clone())).await;
    press(cx, handle, "ctrl-s");
    let (message, detail) = cx.pending_prompt().expect("a prompt");
    assert_eq!(message, "2 conflicts are not resolved.");
    assert_eq!(
        Some(detail.as_str()),
        Some(
            "Saving now writes them as conflict markers: <<<<<<< left … ||||||| base … ======= … >>>>>>> right"
        )
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(!out.exists(), "Cancel writes nothing");
    assert!(!cx.did_prompt_for_new_path());

    press(cx, handle, "ctrl-s");
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Save with conflict markers");
    wait_notice(cx, handle, &view, "Saved the result").await;
    let expected = view.read_with(cx, |v, _| v.session().unwrap().result_text());
    assert!(expected.contains("<<<<<<< left"));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), expected);
    let notice = notice_of(cx, &view).unwrap();
    assert!(
        notice.ends_with("(2 conflicts written as markers)."),
        "{notice}"
    );
}

#[gpui_kit::test]
async fn a_character_the_left_encoding_cannot_hold_asks_for_utf8(cx: &mut TestAppContext) {
    use duet_core::text::encoding::TextEncoding;
    use duet_core::text::merge::MergeText;
    let latin = TextEncoding {
        encoding: encoding_rs::WINDOWS_1252,
        bom: false,
    };
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.txt");
    let open = |cx: &mut TestAppContext| {
        let texts = [
            MergeText::new("caf\u{e9}\n".to_owned(), latin),
            MergeText::new("caf\u{e9}\n".to_owned(), latin),
            MergeText::new("caf\u{e9} \u{e01}\n".to_owned(), latin),
        ];
        let path = out.clone();
        let (handle, view) = open_test_window(cx, move |window, cx| {
            let view = cx.new(|cx| MergeView::with_texts_for_test(texts, window, cx));
            view.update(cx, |v, cx| v.set_output(Some(path), window, cx));
            window.focus(&view.focus_handle(cx), cx);
            view
        });
        (handle, view)
    };
    let (handle, view) = open(cx);
    wait_idle(cx, handle, &view).await;
    draw(cx, handle);
    press(cx, handle, "ctrl-s");
    wait_prompt(cx).await;
    let (message, detail) = cx.pending_prompt().expect("a prompt");
    assert_eq!(
        message,
        "The result has characters windows-1252 can't hold."
    );
    assert_eq!(Some(detail.as_str()), Some("Save it as UTF-8 instead?"));
    cx.simulate_prompt_answer("Save as UTF-8");
    wait_notice(cx, handle, &view, "Saved the result").await;
    assert_eq!(
        std::fs::read(&out).unwrap(),
        "caf\u{e9} \u{e01}\n".as_bytes()
    );
    assert!(notice_of(cx, &view).unwrap().ends_with(" (as UTF-8)."));
    assert!(
        status_of(cx, &view)
            .iter()
            .any(|part| part.contains("UTF-8 (instead of windows-1252)")),
        "{:?}",
        status_of(cx, &view)
    );

    // The answer holds for this result: no second question.
    std::fs::remove_file(&out).unwrap();
    press(cx, handle, "ctrl-s");
    let seen = out.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, _| seen.exists())
        .await;
    assert!(!cx.has_pending_prompt());

    // Cancel on a fresh view writes nothing.
    std::fs::remove_file(&out).unwrap();
    let (handle, view) = open(cx);
    wait_idle(cx, handle, &view).await;
    draw(cx, handle);
    press(cx, handle, "ctrl-s");
    wait_prompt(cx).await;
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(!out.exists());
    assert!(
        !status_of(cx, &view)
            .iter()
            .any(|p| p.contains("instead of"))
    );
}

#[gpui_kit::test]
async fn an_empty_left_saves_in_the_bases_encoding(cx: &mut TestAppContext) {
    // M-62: with no left lines the result takes the base's format, its encoding too.
    use duet_core::text::encoding::TextEncoding;
    use duet_core::text::merge::MergeText;
    let (utf8, latin) = (
        TextEncoding {
            encoding: encoding_rs::UTF_8,
            bom: false,
        },
        TextEncoding {
            encoding: encoding_rs::WINDOWS_1252,
            bom: false,
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.txt");
    let texts = [
        MergeText::new(String::new(), utf8),
        MergeText::new("caf\u{e9}\nbase two\n".to_owned(), latin),
        MergeText::new("caf\u{e9}\nright two\n".to_owned(), utf8),
    ];
    let path = out.clone();
    let (handle, view) = open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| MergeView::with_texts_for_test(texts, window, cx));
        view.update(cx, |v, cx| v.set_output(Some(path), window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    wait_idle(cx, handle, &view).await;
    draw(cx, handle);
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(duet_app::merge::view::MergeUseBase), cx)
    })
    .unwrap();
    let status = status_of(cx, &view);
    assert!(
        status
            .iter()
            .any(|part| part.starts_with("Result: ") && part.contains("windows-1252")),
        "{status:?}"
    );
    press(cx, handle, "ctrl-s");
    wait_notice(cx, handle, &view, "Saved the result").await;
    assert_eq!(std::fs::read(&out).unwrap(), b"caf\xe9\nbase two\n");
}

#[gpui_kit::test]
async fn export_writes_name_conflicts_ext_and_copy_fills_the_clipboard(cx: &mut TestAppContext) {
    let (dir, l, b, r) = f2_files();
    let (handle, view) = open_files(cx, [&l, &b, &r], None).await;
    let expected = view.read_with(cx, |v, _| v.session().unwrap().result_text());
    click(cx, handle, "merge-export");
    assert!(cx.did_prompt_for_new_path());
    let chosen = dir.path().join("left.conflicts.txt");
    let (target, seen_dir) = (
        chosen.clone(),
        std::sync::Arc::new(std::sync::Mutex::new(None)),
    );
    let seen = seen_dir.clone();
    cx.simulate_new_path_selection(move |folder| {
        *seen.lock().unwrap() = Some(folder.to_path_buf());
        Some(target)
    });
    wait_notice(cx, handle, &view, "Exported").await;
    assert_eq!(seen_dir.lock().unwrap().as_deref(), Some(dir.path()));
    assert_eq!(std::fs::read_to_string(&chosen).unwrap(), expected);
    assert_eq!(
        notice_of(cx, &view),
        Some(format!(
            "Exported: {} (2 conflicts as markers).",
            chosen.display()
        ))
    );
    assert!(!dirty(cx, &view), "Export does not touch the dirty flag");
    assert_eq!(
        view.read_with(cx, |v, _| v.output().map(Path::to_path_buf)),
        None,
        "and does not become the output"
    );

    click(cx, handle, "merge-copy");
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some(expected.as_str()));
    assert_eq!(
        notice_of(cx, &view).as_deref(),
        Some("Copied the result: 17 lines, 2 conflicts as markers.")
    );
}

#[gpui_kit::test]
async fn a_typed_output_must_be_a_full_file_path(cx: &mut TestAppContext) {
    let (dir, l, b, r) = write_set("a\nL\nc\n", "a\nb\nc\n", "a\nb\nc\n");
    let (handle, view) = open_files(cx, [&l, &b, &r], None).await;
    let target = dir.path().join("out.txt");
    let relative = relative_to_cwd(&target);
    assert!(relative.is_relative());
    let out: Entity<InputState> = view.read_with(cx, |v, _| v.output_input_for_test());
    for (typed, notice) in [
        (
            relative.display().to_string(),
            "Type a full path for the output file, such as C:\\folder\\merged.txt.",
        ),
        (
            dir.path().display().to_string(),
            "The output must be a file, not a folder.",
        ),
    ] {
        in_view(cx, handle, &view, |v, _, cx| v.set_stage(Stage::Input, cx));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.focus(&out.focus_handle(cx), cx);
            window.press("ctrl-a", cx);
            window.input(&typed, cx);
        })
        .unwrap();
        // The box's change lands at the end of that update; then Ctrl+S.
        press(cx, handle, "ctrl-s");
        cx.run_until_parked();
        assert_eq!(notice_of(cx, &view).as_deref(), Some(notice), "{typed}");
        assert!(!cx.did_prompt_for_new_path(), "and no dialog");
    }
    assert!(!target.exists(), "nothing was written");
    assert!(!dirty(cx, &view));
}

#[gpui_kit::test]
async fn the_dialogs_suggest_the_outputs_name_and_folder(cx: &mut TestAppContext) {
    let (dir, l, b, r) = f2_files();
    let (handle, view) = open_files(cx, [&l, &b, &r], None).await;
    let dialog = |cx: &mut TestAppContext, export: bool| {
        view.read_with(cx, |v, _| v.dialog_for_test(export))
    };
    let folder = dir.path().to_path_buf();
    // No output: the left's folder and name.
    assert_eq!(dialog(cx, false), (folder.clone(), "left.txt".to_owned()));
    assert_eq!(
        dialog(cx, true),
        (folder.clone(), "left.conflicts.txt".to_owned())
    );
    // An output: Save result to… starts at it; Export names its file after it, in the left's
    // folder (LocalDiff `suggestName`, `exportMarkers`).
    let out = dir.path().join("out").join("result.md");
    in_view(cx, handle, &view, |v, window, cx| {
        v.set_output(Some(out), window, cx)
    });
    assert_eq!(
        dialog(cx, false),
        (dir.path().join("out"), "result.md".to_owned())
    );
    assert_eq!(dialog(cx, true), (folder, "result.conflicts.md".to_owned()));
}
