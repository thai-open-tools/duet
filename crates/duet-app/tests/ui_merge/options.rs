//! Toolbar options, status, info, preview, swap and reload (Task 7).

use crate::common::*;

#[gpui_kit::test]
async fn the_status_line_counts_and_names_formats(cx: &mut TestAppContext) {
    let [l, b, r] = f2();
    let (handle, view) = open_texts(cx, &l, &b, &r).await;
    let formats = "Left UTF-8 · LF | Base UTF-8 · LF | Right UTF-8 · LF";
    assert_eq!(
        status_of(cx, &view),
        [
            "0 changes merged automatically",
            "Conflicts left: 2 / 2",
            "Result: 17 lines · UTF-8 · LF",
            formats,
        ]
    );
    // The notice sits in the status bar, under the list, so the list does not jump.
    assert!(drawn(cx, handle, "merge-notice"));
    let status = bounds_of(cx, handle, "merge-status");
    let notice = bounds_of(cx, handle, "merge-notice");
    let list = bounds_of(cx, handle, "merge-list");
    assert!(notice.origin.y >= status.origin.y && notice.bottom() <= status.bottom());
    assert!(
        list.bottom() <= status.origin.y,
        "the list ends where the bar starts"
    );
    assert_eq!(list.origin.y, bounds_of(cx, handle, "merge-info").bottom());

    click(cx, handle, ("merge-use-left", 1usize));
    click(cx, handle, ("merge-use-right", 3usize));
    assert_eq!(
        status_of(cx, &view),
        [
            "0 changes merged automatically",
            "Conflicts left: 0 / 2",
            "Result: 5 lines · UTF-8 · LF",
            formats,
            "● Not saved",
        ]
    );

    // A base with other line endings is named; three equal files are ready at once.
    use duet_core::text::merge::MergeText;
    let crlf = MergeText::typed("a\r\nb\r\nc\r\nd\r\ne\r\n".to_owned());
    let lf = |lines: &[String]| MergeText::typed(lines.iter().map(|l| format!("{l}\n")).collect());
    let (handle2, view2) = open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| MergeView::with_texts_for_test([lf(&l), crlf, lf(&r)], window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    wait_idle(cx, handle2, &view2).await;
    draw(cx, handle2);
    let parts = status_of(cx, &view2);
    assert_eq!(
        parts[3],
        "Left UTF-8 · LF | Base UTF-8 · CRLF | Right UTF-8 · LF"
    );
    assert_eq!(parts[4], "Line endings differ");
    assert_eq!(
        parts.len(),
        5,
        "open conflicts and nothing chosen: no ready or dirty mark"
    );

    let same = owned(&["a", "b"]);
    let (_h, view3) = open_texts(cx, &same, &same, &same).await;
    assert_eq!(
        status_of(cx, &view3),
        [
            "0 changes merged automatically",
            "Conflicts left: 0 / 0",
            "Result: 2 lines · UTF-8 · LF",
            "Left UTF-8 · LF | Base UTF-8 · LF | Right UTF-8 · LF",
            "✓ Ready to save",
        ]
    );
}

#[gpui_kit::test]
async fn the_info_bar_names_the_sides_and_the_output(cx: &mut TestAppContext) {
    let (dir, l, b, r) = write_set("a\nX\nc\n", "a\nb\nc\n", "a\nY\nc\n");
    let (handle, _ws, view) = open_workspace(cx, paths(&l, &b, &r)).await;
    draw(cx, handle);
    assert_eq!(
        view.read_with(cx, |v, _| v.info_parts()),
        [
            "Left: left.txt UTF-8 · LF",
            "Base: base.txt UTF-8 · LF",
            "Right: right.txt UTF-8 · LF",
            "→ Result: chosen when saving",
        ]
    );
    for id in ["merge-info", "merge-save-as"] {
        assert!(drawn(cx, handle, id), "{id} is drawn");
    }
    let out = dir.path().join("out.txt");
    let shown = out.clone();
    in_view(cx, handle, &view, move |v, window, cx| {
        v.set_output(Some(shown), window, cx)
    });
    assert_eq!(
        view.read_with(cx, |v, _| v.info_parts()).last().cloned(),
        Some(format!("→ Result: {}", out.display()))
    );
}

#[gpui_kit::test]
async fn the_whole_result_preview_follows_changes_after_120_ms(cx: &mut TestAppContext) {
    let [l, b, r] = f2();
    let (handle, view) = open_texts(cx, &l, &b, &r).await;
    assert!(!drawn(cx, handle, "merge-preview"));
    assert!(view.read_with(cx, |v, _| v.preview_lines().is_none()));
    click(cx, handle, "merge-show-result");
    draw(cx, handle);
    assert!(view.read_with(cx, |v, _| v.show_result()));
    let all = result_strings(cx, &view);
    assert_eq!(all.len(), 17);
    assert_eq!(
        view.read_with(cx, |v, _| v.preview_lines().map(<[String]>::to_vec)),
        Some(all)
    );
    assert!(drawn(cx, handle, "merge-preview"));
    assert_eq!(
        label(cx, handle, "merge-preview-count").as_deref(),
        Some("17 lines · 2 conflicts as markers")
    );

    click(cx, handle, ("merge-use-left", 1usize));
    draw(cx, handle);
    assert_eq!(
        view.read_with(cx, |v, _| v.preview_lines().map(<[String]>::len)),
        Some(17),
        "the preview waits for the timer"
    );
    cx.executor().advance_clock(PREVIEW_DELAY);
    cx.run_until_parked();
    draw(cx, handle);
    assert_eq!(
        view.read_with(cx, |v, _| v.preview_lines().map(<[String]>::len)),
        Some(11)
    );
    assert_eq!(
        label(cx, handle, "merge-preview-count").as_deref(),
        Some("11 lines · 1 conflict as markers")
    );

    // Each change restarts the timer: the preview shows the last state.
    click(cx, handle, ("merge-use-right", 3usize));
    cx.executor().advance_clock(PREVIEW_DELAY / 2);
    click(cx, handle, ("merge-use-left-right", 3usize));
    cx.executor().advance_clock(PREVIEW_DELAY / 2);
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.preview_lines().map(<[String]>::len)),
        Some(11),
        "half a delay after the last change: not rebuilt yet"
    );
    cx.executor().advance_clock(PREVIEW_DELAY);
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.preview_lines().map(<[String]>::len)),
        Some(6)
    );
    draw(cx, handle);
    assert_eq!(
        label(cx, handle, "merge-preview-count").as_deref(),
        Some("6 lines")
    );

    click(cx, handle, "merge-preview-close");
    draw(cx, handle);
    assert!(!view.read_with(cx, |v, _| v.show_result()));
    assert!(!drawn(cx, handle, "merge-preview"));
    assert!(view.read_with(cx, |v, _| v.preview_lines().is_none()));
}

#[gpui_kit::test]
async fn the_preview_follows_typing_and_undo(cx: &mut TestAppContext) {
    let [l, b, r] = one_conflict();
    let (handle, view) = open_texts(cx, &l, &b, &r).await;
    activate(cx, handle);
    click(cx, handle, "merge-show-result");
    draw(cx, handle);
    focus_result(cx, handle, &view, 1);
    type_text(cx, handle, "typed");
    cx.executor().advance_clock(PREVIEW_DELAY);
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.preview_lines().map(<[String]>::to_vec)),
        Some(owned(&["a", "typed", "c"]))
    );
    press(cx, handle, "escape");
    press(cx, handle, "ctrl-z");
    cx.executor().advance_clock(PREVIEW_DELAY);
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.preview_lines().map(<[String]>::len)),
        Some(9),
        "the unresolved conflict is back as markers"
    );
}

#[gpui_kit::test]
async fn f9_swaps_the_sides_and_flips_the_choices(cx: &mut TestAppContext) {
    let [l, b, r] = f2();
    let (_dir, lp, bp, rp) = write_set(
        &(l.join("\n") + "\n"),
        &(b.join("\n") + "\n"),
        &(r.join("\n") + "\n"),
    );
    let (handle, workspace, view) = open_workspace(cx, paths(&lp, &bp, &rp)).await;
    draw(cx, handle);
    focus_view(cx, handle, &view);
    click(cx, handle, ("merge-use-left-right", 1usize));
    assert_eq!(lines_of(cx, &view, 1), Some(owned(&["X1", "Y1"])));

    press(cx, handle, "f9");
    wait_idle(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.slot(LEFT).unwrap().name, "right.txt");
        assert_eq!(v.slot(RIGHT).unwrap().name, "left.txt");
        assert_eq!(v.session().unwrap().left().name, "right.txt");
        assert!(v.is_dirty(), "still dirty");
        assert_eq!(
            v.notice(),
            Some("Swapped left and right. The result now uses the new left file's encoding and line endings.")
        );
    });
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().iter().next().unwrap().1.title()),
        "Merge: right.txt ↔ left.txt"
    );
    assert_eq!(how_of(cx, &view, 1), Some(How::RightThenLeft));
    assert_eq!(lines_of(cx, &view, 1), Some(owned(&["X1", "Y1"])));
    assert_eq!(
        how_of(cx, &view, 3),
        None,
        "the other conflict is still open"
    );
    assert_eq!(current(cx, &view), Some(1), "the current part is kept");

    // Pressing it again undoes the swap, and the choice flips back.
    press(cx, handle, "f9");
    wait_idle(cx, handle, &view).await;
    assert_eq!(how_of(cx, &view, 1), Some(How::LeftThenRight));
    assert_eq!(
        view.read_with(cx, |v, _| v.slot(LEFT).unwrap().name.clone()),
        "left.txt"
    );

    // Without a session only the slots swap.
    let (handle, view) = open_view(cx);
    let (l, b) = (lp.clone(), bp.clone());
    view.update(cx, |v, cx| {
        v.open_slot_path(LEFT, l, cx);
        v.open_slot_path(BASE, b, cx);
    });
    wait_idle(cx, handle, &view).await;
    press(cx, handle, "f9");
    view.read_with(cx, |v, _| {
        assert!(v.slot(LEFT).is_none());
        assert_eq!(v.slot(RIGHT).unwrap().name, "left.txt");
        assert_eq!(v.slot(BASE).unwrap().name, "base.txt");
        assert_eq!(v.notice(), Some("Swapped left and right."));
        assert!(v.session().is_none());
    });
    click(cx, handle, "merge-swap");
    view.read_with(cx, |v, _| {
        assert_eq!(v.slot(LEFT).unwrap().name, "left.txt");
        assert!(v.slot(RIGHT).is_none());
    });
}

#[gpui_kit::test]
async fn f9_from_a_result_field_hands_the_keys_back(cx: &mut TestAppContext) {
    let [l, b, r] = one_conflict();
    let (handle, view) = open_texts(cx, &l, &b, &r).await;
    activate(cx, handle);
    focus_result(cx, handle, &view, 1);
    type_text(cx, handle, "q");
    let field = result_field(cx, &view, 1);
    assert!(field_has_the_keys(cx, handle, &field));
    press(cx, handle, "f9");
    wait_idle(cx, handle, &view).await;
    draw(cx, handle);
    assert!(
        view_has_the_keys(cx, handle, &view),
        "the view has the keys"
    );
    assert_eq!(how_of(cx, &view, 1), Some(How::Manual));
    assert_eq!(lines_of(cx, &view, 1), Some(owned(&["q"])));
    assert_eq!(
        view.read_with(cx, |v, _| v.session().unwrap().left().text().to_owned()),
        "a\nY\nc\n",
        "the sides traded places"
    );
}

#[gpui_kit::test]
async fn f9_during_a_load_moves_the_pending_slot(cx: &mut TestAppContext) {
    let (_dir, l, b, r) = write_set("a\nX\nc\n", "a\nb\nc\n", "a\nY\nc\n");
    let launch = paths(&l, &b, &r);
    let (handle, workspace) = open_test_window(cx, move |window, cx| {
        cx.new(|cx| Workspace::new(Launch::Merge(launch), window, cx))
    });
    let view = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Merge(view)) => view.clone(),
        _ => panic!("expected a merge tab"),
    });
    assert!(
        view.read_with(cx, |v, _| v.is_busy()),
        "the loads are pending"
    );
    focus_view(cx, handle, &view);
    press(cx, handle, "f9");
    let ws = workspace.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        ws.read(cx).active_idle(cx)
    })
    .await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.slot(LEFT).unwrap().name, "right.txt");
        assert_eq!(v.slot(RIGHT).unwrap().name, "left.txt");
        let session = v.session().expect("the merge was computed");
        assert_eq!(session.left().name, "right.txt");
        assert_eq!(session.right().name, "left.txt");
        assert_eq!(session.conflicts().len(), 1);
    });
}

#[gpui_kit::test]
async fn f5_rereads_the_files_and_asks_when_dirty(cx: &mut TestAppContext) {
    let (_dir, l, b, r) = write_set("a\nX\nc\n", "a\nb\nc\n", "a\nY\nc\n");
    let (handle, _ws, view) = open_workspace(cx, paths(&l, &b, &r)).await;
    draw(cx, handle);
    focus_view(cx, handle, &view);
    std::fs::write(&r, "a\nZ\nc\n").unwrap();
    press(cx, handle, "f5");
    wait_idle(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.session().unwrap().right().text(), "a\nZ\nc\n");
        assert_eq!(v.notice(), Some("Reloaded 3 files."));
        assert_eq!(v.slot(RIGHT).unwrap().text(), "a\nZ\nc\n");
    });

    // With a choice made, F5 asks; Cancel changes nothing.
    draw(cx, handle);
    click(cx, handle, ("merge-use-left", 1usize));
    std::fs::write(&r, "a\nW\nc\n").unwrap();
    press(cx, handle, "f5");
    let (message, _) = cx.pending_prompt().expect("a prompt");
    assert_eq!(
        message,
        "Reloading drops the conflict choices you have not saved."
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |v, _| v.is_busy()), "nothing was read");
    assert_eq!(how_of(cx, &view, 1), Some(How::Left));
    assert_eq!(
        view.read_with(cx, |v, _| v.session().unwrap().right().text().to_owned()),
        "a\nZ\nc\n"
    );
    assert!(dirty(cx, &view));

    press(cx, handle, "f5");
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Reload");
    cx.run_until_parked();
    wait_idle(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.session().unwrap().right().text(), "a\nW\nc\n");
        assert!(!v.is_dirty(), "merged again");
        assert_eq!(v.notice(), Some("Reloaded 3 files."));
    });
    assert_eq!(how_of(cx, &view, 1), None);

    // Typed text has no file to read again.
    let [tl, tb, tr] = one_conflict();
    let (handle, typed) = open_texts(cx, &tl, &tb, &tr).await;
    press(cx, handle, "f5");
    assert!(!cx.has_pending_prompt());
    assert_eq!(
        notice_of(cx, &typed).as_deref(),
        Some("No files to reload.")
    );
}

#[gpui_kit::test]
async fn the_trailing_whitespace_option_merges_again_and_is_saved(cx: &mut TestAppContext) {
    use duet_core::text::merge::diff3::RegionKind;
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (_dir, l, b, r) = write_set("a  \nb\n", "a\nb\n", "a\nB\n");
    let (handle, _ws, view) = open_workspace(cx, paths(&l, &b, &r)).await;
    draw(cx, handle);
    let kinds = |cx: &mut TestAppContext, view: &Entity<MergeView>| {
        view.read_with(cx, |v, _| {
            v.session()
                .unwrap()
                .regions()
                .iter()
                .map(|r| r.kind)
                .collect::<Vec<_>>()
        })
    };
    assert!(!view.read_with(cx, |v, _| v.trail()));
    assert_eq!(
        kinds(cx, &view),
        [RegionKind::Conflict],
        "touching edits join"
    );
    assert!(!drawn(cx, handle, "merge-banner-trail"));

    click(cx, handle, "merge-trail");
    wait_idle(cx, handle, &view).await;
    draw(cx, handle);
    assert!(view.read_with(cx, |v, _| v.trail()));
    assert_eq!(kinds(cx, &view), [RegionKind::Stable, RegionKind::Right]);
    assert!(drawn(cx, handle, "merge-banner-trail"));
    assert!(status_of(cx, &view).contains(&"Ignoring trailing whitespace".to_owned()));
    let file = cx
        .update(|cx| duet_app::settings::settings_file(cx))
        .unwrap();
    assert_eq!(
        duet_app::settings::get_bool(&file, "merge.trail"),
        Some(true)
    );

    // A new view on the set starts with the option on.
    let (handle2, _ws2, view2) = open_workspace(cx, paths(&l, &b, &r)).await;
    assert!(view2.read_with(cx, |v, _| v.trail()));
    assert_eq!(kinds(cx, &view2), [RegionKind::Stable, RegionKind::Right]);

    // Over a choice that is not saved, the option asks first.
    draw(cx, handle2);
    click(cx, handle2, ("merge-use-left", 1usize));
    assert!(dirty(cx, &view2));
    click(cx, handle2, "merge-trail");
    let (message, _) = cx.pending_prompt().expect("a prompt");
    assert_eq!(
        message,
        "Changing this option merges again and drops the conflict choices you have not saved."
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(
        view2.read_with(cx, |v, _| v.trail()),
        "Cancel keeps the option"
    );
    assert_eq!(kinds(cx, &view2), [RegionKind::Stable, RegionKind::Right]);
    assert!(dirty(cx, &view2));
    assert_eq!(
        duet_app::settings::get_bool(&file, "merge.trail"),
        Some(true)
    );

    click(cx, handle2, "merge-trail");
    cx.simulate_prompt_answer("Merge again");
    cx.run_until_parked();
    wait_idle(cx, handle2, &view2).await;
    assert!(!view2.read_with(cx, |v, _| v.trail()));
    assert_eq!(kinds(cx, &view2), [RegionKind::Conflict]);
    assert!(!dirty(cx, &view2), "a fresh session");
    assert_eq!(
        current(cx, &view2),
        Some(0),
        "the first conflict is current"
    );
    assert_eq!(
        duet_app::settings::get_bool(&file, "merge.trail"),
        Some(false)
    );
}

#[gpui_kit::test]
async fn swapping_in_paste_mode_trades_the_texts_and_edit_flags(cx: &mut TestAppContext) {
    let (handle, view) = open_view(cx);
    set_mode(cx, handle, &view, InputMode::Paste);
    focus_field(cx, handle, &view, LEFT);
    type_lines(cx, handle, &["one", "two"]);
    assert!(slot_text(cx, &view, LEFT).edited);
    press(cx, handle, "f9");
    assert_eq!(field_text(cx, &view, RIGHT), "one\ntwo");
    assert_eq!(field_text(cx, &view, LEFT), "");
    assert!(slot_text(cx, &view, RIGHT).edited && !slot_text(cx, &view, LEFT).edited);
    assert_eq!(
        notice_of(cx, &view).as_deref(),
        Some("Swapped left and right.")
    );
}

#[gpui_kit::test]
async fn f9_twice_or_during_a_merge_keeps_the_slots_and_the_session_together(
    cx: &mut TestAppContext,
) {
    let (_dir, lp, bp, rp) = write_set("a\nX\nc\n", "a\nb\nc\n", "a\nY\nc\n");
    let names = |cx: &mut TestAppContext, view: &Entity<MergeView>| {
        view.read_with(cx, |v, _| {
            let session = v.session().expect("a session");
            (
                v.slot(LEFT).unwrap().name.clone(),
                session.left().name.clone(),
            )
        })
    };
    let swapped = || ("right.txt".to_owned(), "right.txt".to_owned());

    // F9 twice with nothing run between: the session swaps with the slots, at once.
    let (handle, _workspace, view) = open_workspace(cx, paths(&lp, &bp, &rp)).await;
    draw(cx, handle);
    focus_view(cx, handle, &view);
    press(cx, handle, "f9");
    assert_eq!(names(cx, &view), swapped(), "the session swaps at once");
    press(cx, handle, "f9");
    wait_idle(cx, handle, &view).await;
    assert_eq!(
        names(cx, &view),
        ("left.txt".to_owned(), "left.txt".to_owned())
    );

    // F9, then Escape: no swap is left running to cancel, so the two still agree.
    press(cx, handle, "f9");
    press(cx, handle, "escape");
    cx.run_until_parked();
    assert_eq!(names(cx, &view), swapped());

    // F9 while the first merge runs: it merges again from the swapped slots.
    let (handle, view) = open_view(cx);
    view.update(cx, |v, cx| {
        v.open_slot_path(LEFT, lp.clone(), cx);
        v.open_slot_path(BASE, bp.clone(), cx);
        v.open_slot_path(RIGHT, rp.clone(), cx);
    });
    let loading = view.clone();
    step_until(cx, move |cx| {
        !(LEFT..=RIGHT).any(|slot| loading.read(cx).loading(slot))
    });
    assert!(
        view.read_with(cx, |v, _| v.is_busy() && v.session().is_none()),
        "the files are in and the merge runs"
    );
    draw(cx, handle);
    press(cx, handle, "f9");
    wait_idle(cx, handle, &view).await;
    assert_eq!(names(cx, &view), swapped());
}

#[gpui_kit::test]
async fn f5_counts_the_files_it_read_and_retries_a_failed_slot(cx: &mut TestAppContext) {
    let (_dir, l, b, r) = write_set("a\nL\nc\n", "a\nb\nc\n", "a\nb\nc\n");
    std::fs::remove_file(&l).unwrap();
    let (handle, view) = open_view(cx);
    view.update(cx, |v, cx| {
        v.open_slot_path(LEFT, l.clone(), cx);
        v.open_slot_path(BASE, b.clone(), cx);
        v.open_slot_path(RIGHT, r.clone(), cx);
    });
    wait_idle(cx, handle, &view).await;
    assert!(
        view.read_with(cx, |v, _| v.slot(LEFT).is_none()
            && v.slot_message(LEFT).is_some()),
        "the left is missing"
    );

    // The left is back and the other two are gone: F5 reads all three again, the failed
    // left too, and counts the files it read.
    std::fs::write(&l, "a\nL\nc\n").unwrap();
    std::fs::remove_file(&b).unwrap();
    std::fs::remove_file(&r).unwrap();
    draw(cx, handle);
    press(cx, handle, "f5");
    wait_idle(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(
            v.slot(LEFT).map(|text| text.name.as_str()),
            Some("left.txt"),
            "the failed slot was read again"
        );
        assert!(v.slot(BASE).is_none() && v.slot(RIGHT).is_none());
        assert_eq!(v.notice(), Some("Reloaded 1 file."));
    });
}

#[gpui_kit::test]
async fn clearing_every_pending_reload_load_drops_the_reload(cx: &mut TestAppContext) {
    let (_dir, l, b, r) = write_set("a\nX\nc\n", "a\nb\nc\n", "a\nY\nc\n");
    let (handle, view) = open_files(cx, [&l, &b, &r], None).await;
    press(cx, handle, "f5");
    assert!(
        view.read_with(cx, |v, _| v.is_busy()),
        "the files are read again"
    );
    in_view(cx, handle, &view, |v, window, cx| {
        for slot in LEFT..=RIGHT {
            v.clear_slot(slot, window, cx);
        }
    });
    cx.run_until_parked();
    draw(cx, handle);
    click(cx, handle, ("merge-use-left", 1usize));
    assert!(dirty(cx, &view));

    // Files chosen later land over that choice as over any other: it stays.
    view.update(cx, |v, cx| {
        v.open_slot_path(LEFT, l.clone(), cx);
        v.open_slot_path(BASE, b.clone(), cx);
        v.open_slot_path(RIGHT, r.clone(), cx);
    });
    wait_idle(cx, handle, &view).await;
    assert!(dirty(cx, &view), "the choice is still there");
    assert_eq!(how_of(cx, &view, 1), Some(How::Left));
    assert_eq!(
        notice_of(cx, &view).as_deref(),
        Some("Files chosen: press Merge to merge again.")
    );
}
