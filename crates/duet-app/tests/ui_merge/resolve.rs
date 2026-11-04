//! Resolving: choices, result fields, undo and redo, bulk resolve and the card menu
//! (Task 6).

use crate::common::*;

#[gpui_kit::test]
async fn the_choice_buttons_fill_the_result_and_mark_the_tab_dirty(cx: &mut TestAppContext) {
    let (_dir, l, b, r) = write_set("a\nX\nc\n", "a\nb\nc\n", "a\nY\nc\n");
    let (handle, workspace, view) = open_workspace(cx, paths(&l, &b, &r)).await;
    draw(cx, handle);
    let tab_dirty = |cx: &mut TestAppContext| workspace.read_with(cx, |w, cx| w.tab_dirty(0, cx));
    let count = |cx: &mut TestAppContext| label(cx, handle, ("merge-count", 1usize));
    assert_eq!(result_text(cx, &view, 1).as_deref(), Some(""));
    assert_eq!(count(cx).as_deref(), Some("Not chosen yet"));
    assert!(!tab_dirty(cx), "nothing chosen yet");
    for id in [
        "merge-use-left",
        "merge-use-right",
        "merge-use-left-right",
        "merge-use-right-left",
        "merge-use-base",
        "merge-reset",
        "merge-result",
    ] {
        assert!(drawn(cx, handle, (id, 1usize)), "{id} is drawn");
    }
    let field_height = |cx: &mut TestAppContext| {
        draw(cx, handle);
        bounds_of(cx, handle, ("merge-result", 1usize)).size.height
    };
    let open_height = field_height(cx);

    click(cx, handle, ("merge-use-left", 1usize));
    assert_eq!(result_text(cx, &view, 1).as_deref(), Some("X"));
    assert_eq!(count(cx).as_deref(), Some("1 line"));
    let one_line = field_height(cx);
    assert!(
        one_line < open_height,
        "an open conflict's field has two rows, one line has one: {one_line:?} {open_height:?}"
    );
    assert_eq!(card(cx, &view, 1).state, "✓ Use left");
    assert_eq!(view.read_with(cx, |v, _| v.badge()), Some("ok".to_owned()));
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        Some("ok".to_owned())
    );
    assert!(tab_dirty(cx), "a choice marks the tab");

    click(cx, handle, ("merge-use-left-right", 1usize));
    assert_eq!(result_text(cx, &view, 1).as_deref(), Some("X\nY"));
    assert_eq!(count(cx).as_deref(), Some("2 lines"));
    assert_eq!(field_height(cx), open_height, "two lines, two rows");
    click(cx, handle, ("merge-use-right-left", 1usize));
    assert_eq!(result_text(cx, &view, 1).as_deref(), Some("Y\nX"));
    click(cx, handle, ("merge-use-base", 1usize));
    assert_eq!(result_text(cx, &view, 1).as_deref(), Some("b"));
    assert_eq!(card(cx, &view, 1).state, "✓ Use base");

    click(cx, handle, ("merge-reset", 1usize));
    assert_eq!(result_text(cx, &view, 1).as_deref(), Some(""));
    assert_eq!(card(cx, &view, 1).state, "Not resolved");
    assert_eq!(count(cx).as_deref(), Some("Not chosen yet"));
    assert_eq!(view.read_with(cx, |v, _| v.badge()), Some("1".to_owned()));
    assert!(tab_dirty(cx), "a reset is a change too");
}

#[gpui_kit::test]
async fn an_automatic_part_opens_with_a_row_for_each_of_its_lines(cx: &mut TestAppContext) {
    let base = "a\nb\nm\nc\nd\nz\n";
    let (_dir, l, b, r) = write_set("a\nX\nm\nP\nQ\nz\n", base, base);
    let set = paths(&l, &b, &r);
    let (handle, workspace) = open_test_window(cx, move |window, cx| {
        cx.new(|cx| Workspace::new(Launch::Merge(set), window, cx))
    });
    let view = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Merge(view)) => view.clone(),
        _ => panic!("expected a merge tab"),
    });
    // No `draw` here: under test a dirty window draws by itself when effects flush, and the
    // frame that made the fields is the one to read. An explicit draw is a second frame, which
    // the real window never gets: a notify during paint schedules nothing.
    for _ in 0..2_000 {
        if view.read_with(cx, |v, _| v.field_for_test(3).is_some()) {
            break;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    assert_eq!(result_text(cx, &view, 1).as_deref(), Some("X"));
    assert_eq!(result_text(cx, &view, 3).as_deref(), Some("P\nQ"));
    let height = |cx: &mut TestAppContext, chunk: usize| {
        bounds_of(cx, handle, ("merge-result", chunk)).size.height
    };
    let (one_line, two_lines) = (height(cx, 1), height(cx, 3));
    assert!(
        two_lines > one_line,
        "a part of two lines must not open clipped to one row: {two_lines:?} {one_line:?}"
    );
    assert!(!dirty(cx, &view), "opening the set is not an edit");
}

#[gpui_kit::test]
async fn typing_in_a_result_field_is_an_edit_and_one_undo_step(cx: &mut TestAppContext) {
    let [l, b, r] = one_conflict();
    let (handle, view) = open_texts(cx, &l, &b, &r).await;
    focus_result(cx, handle, &view, 1);
    type_text(cx, handle, "mine");
    assert_eq!(card(cx, &view, 1).state, "✓ Edited");
    assert_eq!(lines_of(cx, &view, 1), Some(owned(&["mine"])));
    assert_eq!(
        label(cx, handle, ("merge-count", 1usize)).as_deref(),
        Some("1 line")
    );
    assert!(dirty(cx, &view));

    press(cx, handle, "escape");
    assert!(
        view_has_the_keys(cx, handle, &view),
        "Esc leaves the field and ends the edit"
    );

    press(cx, handle, "ctrl-z");
    assert_eq!(result_text(cx, &view, 1).as_deref(), Some(""));
    assert_eq!(card(cx, &view, 1).state, "Not resolved");
    assert_eq!(lines_of(cx, &view, 1), None, "the typing was one step");
    press(cx, handle, "ctrl-y");
    assert_eq!(result_text(cx, &view, 1).as_deref(), Some("mine"));
    assert_eq!(card(cx, &view, 1).state, "✓ Edited");

    press(cx, handle, "ctrl-z");
    assert_eq!(
        notice_of(cx, &view),
        None,
        "an undo that works says nothing"
    );
    press(cx, handle, "ctrl-z");
    assert_eq!(notice_of(cx, &view).as_deref(), Some("Nothing to undo."));
    press(cx, handle, "ctrl-y");
    assert_eq!(result_text(cx, &view, 1).as_deref(), Some("mine"));
    press(cx, handle, "ctrl-y");
    assert_eq!(notice_of(cx, &view).as_deref(), Some("Nothing to redo."));
    press(cx, handle, "ctrl-z");
    press(cx, handle, "ctrl-shift-z");
    assert_eq!(
        result_text(cx, &view, 1).as_deref(),
        Some("mine"),
        "Ctrl+Shift+Z redoes too"
    );
}

#[gpui_kit::test]
async fn alt_arrows_and_the_menu_resolve_the_current_card(cx: &mut TestAppContext) {
    let [l, b, r] = f2();
    let (handle, view) = open_texts(cx, &l, &b, &r).await;
    press(cx, handle, "f8");
    assert_eq!(current(cx, &view), Some(3));
    press(cx, handle, "alt-left");
    assert_eq!(how_of(cx, &view, 3), Some(How::Left));
    assert_eq!(result_text(cx, &view, 3).as_deref(), Some("X2"));
    press(cx, handle, "alt-right");
    assert_eq!(how_of(cx, &view, 3), Some(How::Right));
    assert_eq!(result_text(cx, &view, 3).as_deref(), Some("Y2"));
    assert_eq!(how_of(cx, &view, 1), None, "only the current card");

    assert!(
        view.read_with(cx, |v, _| card_menu(v).is_empty()),
        "no menu before a right press"
    );
    cx.update_window(handle, |_, window, cx| {
        window.right_click(("merge-card-head", 1usize), cx)
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| duet_app::ui::menu::outline(&card_menu(v))),
        [
            "Use left",
            "Use right",
            "Left + right",
            "Right + left",
            "Use base",
            "-",
            "Reset to default"
        ]
    );
    assert_eq!(view.read_with(cx, |v, _| v.menu_target_for_test()), Some(1));
    assert_eq!(
        current(cx, &view),
        Some(1),
        "the right press makes it current"
    );

    let focus = view.read_with(cx, |v, cx| v.focus_handle(cx));
    let activate = |cx: &mut TestAppContext, item: &str| {
        cx.update_window(handle, |_, window, cx| {
            let entries = card_menu(view.read(cx));
            duet_app::ui::menu::activate(entries, item, Some(&focus), window, cx)
        })
        .unwrap()
    };
    assert!(activate(cx, "Use base"));
    assert_eq!(how_of(cx, &view, 1), Some(How::Base));
    assert_eq!(result_text(cx, &view, 1).as_deref(), Some("b"));
    assert!(activate(cx, "Left + right"));
    assert_eq!(lines_of(cx, &view, 1), Some(owned(&["X1", "Y1"])));
    assert!(activate(cx, "Reset to default"));
    assert_eq!(how_of(cx, &view, 1), None);
    assert_eq!(
        how_of(cx, &view, 3),
        Some(How::Right),
        "the other card is kept"
    );

    // The columns carry the menu too; the field keeps its own text menu.
    cx.update_window(handle, |_, window, cx| {
        window.right_click(("merge-col-base", 3usize), cx)
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| v.menu_target_for_test()), Some(3));
    cx.update_window(handle, |_, window, cx| {
        window.right_click(("merge-result", 1usize), cx)
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.menu_target_for_test()),
        None,
        "a right press elsewhere in the list forgets the old target"
    );
    assert!(view.read_with(cx, |v, _| card_menu(v).is_empty()));

    // No change is current: the keys say how to pick one.
    let same = owned(&["a"]);
    let (handle, view) = open_texts(cx, &same, &same, &same).await;
    press(cx, handle, "alt-left");
    assert_eq!(
        notice_of(cx, &view).as_deref(),
        Some("Choose a part first: press F8 to go to the next conflict.")
    );
}

#[gpui_kit::test]
async fn keys_stay_in_a_focused_result_field(cx: &mut TestAppContext) {
    let [l, b, r] = f2();
    let (handle, view) = open_texts(cx, &l, &b, &r).await;
    assert_eq!(current(cx, &view), Some(1));
    let field = result_field(cx, &view, 1);
    focus_result(cx, handle, &view, 1);
    assert!(field_has_the_keys(cx, handle, &field));
    view.update(cx, |v, cx| v.show_notice("Before the keys.", cx));

    press(cx, handle, "alt-left");
    assert_eq!(
        how_of(cx, &view, 1),
        None,
        "Alt+← does not resolve from a field"
    );
    assert!(field_has_the_keys(cx, handle, &field));
    press(cx, handle, "ctrl-z");
    assert_eq!(
        notice_of(cx, &view).as_deref(),
        Some("Before the keys."),
        "the field had Ctrl+Z: the merge would have said there is nothing to undo"
    );
    press(cx, handle, "ctrl-shift-z");
    assert_eq!(notice_of(cx, &view).as_deref(), Some("Before the keys."));
    press(cx, handle, "ctrl-home");
    assert!(
        field_has_the_keys(cx, handle, &field),
        "Ctrl+Home is the field's"
    );
    press(cx, handle, "ctrl-enter");
    assert_eq!(
        result_text(cx, &view, 1).as_deref(),
        Some(""),
        "Ctrl+Enter types no newline in a result field (PF3)"
    );
    assert!(
        !view.read_with(cx, |v, _| v.is_busy()),
        "and merges nothing here"
    );

    press(cx, handle, "f8");
    assert_eq!(
        current(cx, &view),
        Some(3),
        "a free key acts from the field"
    );
    assert!(view_has_the_keys(cx, handle, &view), "and ends the edit");
    assert!(!dirty(cx, &view), "nothing was typed");
}

#[gpui_kit::test]
async fn a_focused_result_field_keeps_the_keys_when_scrolled_away(cx: &mut TestAppContext) {
    let [left, base, right] = thirty_conflicts();
    let (handle, view) = open_texts(cx, &left, &base, &right).await;
    activate(cx, handle);
    let first = view.read_with(cx, |v, _| v.session().unwrap().conflicts()[0]);
    let field = result_field(cx, &view, first);
    focus_result(cx, handle, &view, first);
    type_text(cx, handle, "a");
    cx.update_window(handle, |_, window, cx| {
        window.scroll(
            "merge-list",
            ScrollDelta::Pixels(point(px(0.), px(-4000.))),
            cx,
        )
    })
    .unwrap();
    draw(cx, handle);
    assert!(
        !view.read_with(cx, |v, _| v.card_visible_for_test(first)),
        "the card is scrolled out of view"
    );
    assert!(
        field_has_the_keys(cx, handle, &field),
        "its field keeps the keys"
    );
    type_text(cx, handle, "b");
    assert_eq!(lines_of(cx, &view, first), Some(owned(&["ab"])));
}

#[gpui_kit::test]
async fn focusing_a_result_field_leaves_the_list_where_it_is(cx: &mut TestAppContext) {
    let [left, base, right] = thirty_conflicts();
    let (handle, view) = open_texts(cx, &left, &base, &right).await;
    activate(cx, handle);
    let first = view.read_with(cx, |v, _| v.session().unwrap().conflicts()[0]);
    // A little down: the first card is the item at the top, partly scrolled past.
    cx.update_window(handle, |_, window, cx| {
        window.scroll(
            "merge-list",
            ScrollDelta::Pixels(point(px(0.), px(-60.))),
            cx,
        )
    })
    .unwrap();
    draw(cx, handle);
    let before = card_offset(cx, handle, first);
    assert!(
        before < px(0.),
        "the card's top is above the list: {before:?}"
    );
    focus_result(cx, handle, &view, first);
    draw(cx, handle);
    assert_eq!(
        card_offset(cx, handle, first),
        before,
        "keeping the field drawn does not move the list"
    );
}

#[gpui_kit::test]
async fn a_result_field_takes_the_current_part_and_its_blur_ends_the_edit(cx: &mut TestAppContext) {
    let [l, b, r] = f2();
    let (handle, view) = open_texts(cx, &l, &b, &r).await;
    activate(cx, handle);
    assert_eq!(current(cx, &view), Some(1));
    focus_result(cx, handle, &view, 3);
    assert_eq!(
        current(cx, &view),
        Some(3),
        "a focused field makes its part current"
    );
    type_text(cx, handle, "a");

    // The keys go elsewhere (not by Esc): the blur ends the typing session.
    let view_focus = view.read_with(cx, |v, cx| v.focus_handle(cx));
    cx.update_window(handle, |_, window, cx| {
        window.focus(&view_focus, cx);
        window.render_frame(cx);
    })
    .unwrap();
    focus_result(cx, handle, &view, 3);
    type_text(cx, handle, "b");
    assert_eq!(lines_of(cx, &view, 3), Some(owned(&["ab"])));
    in_view(cx, handle, &view, |v, window, cx| v.undo(window, cx));
    assert_eq!(
        lines_of(cx, &view, 3),
        Some(owned(&["a"])),
        "the second typing was its own step"
    );
    assert_eq!(result_text(cx, &view, 3).as_deref(), Some("a"));
}

#[gpui_kit::test]
async fn a_toolbar_click_ends_the_typing_session(cx: &mut TestAppContext) {
    // M-49: a toolbar button ends the edit as a blur does, though the field keeps the keys.
    let [l, b, r] = f2();
    let (handle, view) = open_texts(cx, &l, &b, &r).await;
    activate(cx, handle);
    focus_result(cx, handle, &view, 3);
    type_text(cx, handle, "a");
    click(cx, handle, "merge-copy");
    let field = result_field(cx, &view, 3);
    assert!(field_has_the_keys(cx, handle, &field));
    type_text(cx, handle, "b");
    assert_eq!(lines_of(cx, &view, 3), Some(owned(&["ab"])));
    in_view(cx, handle, &view, |v, window, cx| v.undo(window, cx));
    assert_eq!(
        lines_of(cx, &view, 3),
        Some(owned(&["a"])),
        "the typing after the click was its own step"
    );
}

#[gpui_kit::test]
async fn ctrl_z_in_the_field_after_a_toolbar_press_undoes_only_the_later_typing(
    cx: &mut TestAppContext,
) {
    // M-49 as the user meets it: with the field keeping the keys, Ctrl+Z is the field's own
    // undo (R5), so the press must split that history too, not only the session's.
    let [l, b, r] = f2();
    let (handle, view) = open_texts(cx, &l, &b, &r).await;
    activate(cx, handle);
    focus_result(cx, handle, &view, 3);
    for key in ["x", "x"] {
        type_text(cx, handle, key);
    }
    // A field click splits the steps (the control the re-run used).
    click(cx, handle, ("merge-result", 3usize));
    for key in ["p", "p"] {
        type_text(cx, handle, key);
    }
    click(cx, handle, "merge-copy");
    let field = result_field(cx, &view, 3);
    assert!(
        field_has_the_keys(cx, handle, &field),
        "the press keeps the keys in the field"
    );
    for key in ["q", "q"] {
        type_text(cx, handle, key);
    }
    draw(cx, handle);
    assert_eq!(result_text(cx, &view, 3).as_deref(), Some("xxppqq"));
    press(cx, handle, "ctrl-z");
    draw(cx, handle);
    assert_eq!(
        result_text(cx, &view, 3).as_deref(),
        Some("xxpp"),
        "Ctrl+Z undoes only what was typed after the toolbar press"
    );
    press(cx, handle, "ctrl-z");
    draw(cx, handle);
    assert_eq!(result_text(cx, &view, 3).as_deref(), Some("xx"));
}

#[gpui_kit::test]
async fn bulk_resolve_fills_only_open_conflicts(cx: &mut TestAppContext) {
    let [l, b, r] = f2();
    let (handle, view) = open_texts(cx, &l, &b, &r).await;
    assert!(enabled(cx, handle, "merge-all-left") && enabled(cx, handle, "merge-all-right"));
    click(cx, handle, ("merge-use-right", 3usize));
    click(cx, handle, "merge-all-left");
    assert_eq!(lines_of(cx, &view, 1), Some(owned(&["X1"])));
    assert_eq!(lines_of(cx, &view, 3), Some(owned(&["Y2"])));
    assert_eq!(
        how_of(cx, &view, 3),
        Some(How::Right),
        "a chosen conflict is kept"
    );
    assert_eq!(result_text(cx, &view, 1).as_deref(), Some("X1"));
    assert_eq!(
        notice_of(cx, &view).as_deref(),
        Some("Resolved 1 conflict with the left side.")
    );
    click(cx, handle, "merge-all-left");
    assert_eq!(
        notice_of(cx, &view).as_deref(),
        Some("No unresolved conflicts left.")
    );

    press(cx, handle, "ctrl-z");
    assert_eq!(how_of(cx, &view, 1), None, "the bulk step is undone as one");
    assert_eq!(how_of(cx, &view, 3), Some(How::Right));
    assert_eq!(result_text(cx, &view, 1).as_deref(), Some(""));
    press(cx, handle, "ctrl-z");
    assert_eq!(how_of(cx, &view, 3), None);
    click(cx, handle, "merge-all-right");
    assert_eq!(
        notice_of(cx, &view).as_deref(),
        Some("Resolved 2 conflicts with the right side.")
    );
    assert_eq!(result_text(cx, &view, 1).as_deref(), Some("Y1"));
    assert_eq!(result_text(cx, &view, 3).as_deref(), Some("Y2"));

    // Without conflicts the buttons wait.
    let (handle, view) = open_texts(
        cx,
        &owned(&["a", "L", "c"]),
        &owned(&["a", "b", "c"]),
        &owned(&["a", "b", "c"]),
    )
    .await;
    assert!(!enabled(cx, handle, "merge-all-left") && !enabled(cx, handle, "merge-all-right"));
    let resolved = in_view(cx, handle, &view, |v, window, cx| {
        v.resolve_all(How::Left, window, cx)
    });
    assert_eq!(resolved, 0);
    assert_eq!(notice_of(cx, &view).as_deref(), Some("No conflicts."));
}

#[gpui_kit::test]
async fn merging_again_or_clearing_over_unsaved_choices_asks(cx: &mut TestAppContext) {
    let (dir, l, b, r) = write_set("a\nX\nc\n", "a\nb\nc\n", "a\nY\nc\n");
    let (handle, view) = open_view(cx);
    let open_all = |cx: &mut TestAppContext| {
        view.update(cx, |v, cx| {
            v.open_slot_path(LEFT, l.clone(), cx);
            v.open_slot_path(BASE, b.clone(), cx);
            v.open_slot_path(RIGHT, r.clone(), cx);
        })
    };
    open_all(cx);
    wait_idle(cx, handle, &view).await;
    draw(cx, handle);
    click(cx, handle, ("merge-use-left", 1usize));
    assert!(dirty(cx, &view));

    // Merge ▸ again asks first; Cancel keeps the choice.
    click(cx, handle, "merge-stage-input");
    press(cx, handle, "ctrl-enter");
    let (message, _) = cx.pending_prompt().expect("a prompt");
    assert_eq!(
        message,
        "Merging again drops the conflict choices you have not saved."
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(how_of(cx, &view, 1), Some(How::Left));
    assert!(dirty(cx, &view) && !view.read_with(cx, |v, _| v.is_busy()));
    press(cx, handle, "ctrl-enter");
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Merge again");
    cx.run_until_parked();
    wait_idle(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.stage()), Stage::Merge);
    assert_eq!(how_of(cx, &view, 1), None, "a fresh session");
    assert!(!dirty(cx, &view));

    // Clear all asks too.
    draw(cx, handle);
    click(cx, handle, ("merge-use-left", 1usize));
    click(cx, handle, "merge-stage-input");
    click(cx, handle, "merge-clear");
    let (message, _) = cx.pending_prompt().expect("a prompt");
    assert_eq!(
        message,
        "Clear all three slots and the merge result you have not saved?"
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        assert!(v.slot(LEFT).is_some() && v.session().is_some());
    });
    click(cx, handle, "merge-clear");
    cx.simulate_prompt_answer("Clear");
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        assert!((LEFT..=RIGHT).all(|s| v.slot(s).is_none()));
        assert!(v.session().is_none() && !v.is_dirty());
    });

    // A file opened over unsaved choices waits for Merge ▸.
    open_all(cx);
    wait_idle(cx, handle, &view).await;
    draw(cx, handle);
    click(cx, handle, ("merge-use-left", 1usize));
    let other = dir.path().join("other.txt");
    std::fs::write(&other, "a\nZ\nc\n").unwrap();
    view.update(cx, |v, cx| v.open_slot_path(LEFT, other, cx));
    wait_idle(cx, handle, &view).await;
    assert!(!cx.has_pending_prompt(), "no prompt for a file");
    view.read_with(cx, |v, _| {
        assert_eq!(
            v.notice(),
            Some("Files chosen: press Merge to merge again.")
        );
        assert_eq!(v.slot(LEFT).unwrap().name, "other.txt");
        let session = v.session().unwrap();
        assert_eq!(session.left().text(), "a\nX\nc\n", "no merge ran");
        assert!(session.resolution(1).is_some());
    });
}

#[gpui_kit::test]
async fn escape_leaves_a_result_field_then_cancels(cx: &mut TestAppContext) {
    let [l, b, r] = one_conflict();
    let (handle, view) = open_texts(cx, &l, &b, &r).await;
    focus_result(cx, handle, &view, 1);
    type_text(cx, handle, "a");
    press(cx, handle, "escape");
    assert!(view_has_the_keys(cx, handle, &view), "Esc leaves the field");
    assert_ne!(notice_of(cx, &view).as_deref(), Some(CANCELLED));

    // The next typing is a new undo step.
    focus_result(cx, handle, &view, 1);
    type_text(cx, handle, "b");
    assert_eq!(lines_of(cx, &view, 1), Some(owned(&["ab"])));
    press(cx, handle, "escape");
    press(cx, handle, "ctrl-z");
    assert_eq!(lines_of(cx, &view, 1), Some(owned(&["a"])));
    assert_eq!(result_text(cx, &view, 1).as_deref(), Some("a"));

    // With the view focused, Esc cancels a running merge.
    in_view(cx, handle, &view, |v, window, cx| v.run(true, window, cx));
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    press(cx, handle, "escape");
    view.read_with(cx, |v, _| {
        assert_eq!(v.notice(), Some(CANCELLED));
        assert!(!v.is_busy());
    });
    cx.run_until_parked();
    assert_eq!(
        lines_of(cx, &view, 1),
        Some(owned(&["a"])),
        "the cancelled merge never landed"
    );
}

#[gpui_kit::test]
async fn a_choice_during_a_reload_is_not_dropped_silently(cx: &mut TestAppContext) {
    const WAIT: &str = "Merging…: wait for the new result.";
    let (_dir, lp, bp, rp) = write_set("a\nX\nc\n", "a\nb\nc\n", "a\nY\nc\n");
    let (handle, _workspace, view) = open_workspace(cx, paths(&lp, &bp, &rp)).await;
    draw(cx, handle);
    focus_view(cx, handle, &view);
    press(cx, handle, "f5");
    assert!(
        view.read_with(cx, |v, _| v.is_busy()),
        "the files are read again"
    );
    click(cx, handle, ("merge-use-left", 1usize));
    assert_eq!(notice_of(cx, &view).as_deref(), Some(WAIT));
    assert_eq!(how_of(cx, &view, 1), None, "the choice is refused");
    assert!(!dirty(cx, &view));
    wait_idle(cx, handle, &view).await;
    assert!(!cx.has_pending_prompt());
    assert_eq!(how_of(cx, &view, 1), None);

    // Typing into a result field while a merge runs is refused too, and the field of the old
    // result gives the keys back when the new one lands.
    activate(cx, handle);
    in_view(cx, handle, &view, |v, window, cx| v.run(false, window, cx));
    assert!(view.read_with(cx, |v, _| v.is_busy()), "the merge runs");
    focus_result(cx, handle, &view, 1);
    type_text(cx, handle, "q");
    assert_eq!(how_of(cx, &view, 1), None);
    assert_eq!(result_text(cx, &view, 1).as_deref(), Some(""));
    assert_eq!(notice_of(cx, &view).as_deref(), Some(WAIT));
    wait_idle(cx, handle, &view).await;
    draw(cx, handle);
    assert!(view_has_the_keys(cx, handle, &view));
    assert!(!dirty(cx, &view));
}
