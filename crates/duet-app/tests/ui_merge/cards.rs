//! The merge stage: the card list, folding, columns and navigation (Task 5).

use crate::common::*;

#[gpui_kit::test]
#[allow(clippy::single_range_in_vec_init)]
async fn a_card_shows_three_columns_with_marks_and_highlights(cx: &mut TestAppContext) {
    let (handle, view) = open_texts(
        cx,
        &owned(&["a", "one 2", "c"]),
        &owned(&["a", "one two", "c"]),
        &owned(&["a", "ONE two", "c"]),
    )
    .await;
    let c = card(cx, &view, 1);
    assert_eq!(
        (c.badge, c.state.as_str(), c.place.as_str()),
        ("CONFLICT", "Not resolved", "Base line 2")
    );
    assert_eq!(
        c.columns[0],
        ColumnText {
            head: "Left line 2".into(),
            rows: vec![row(2, "one 2", true, vec![4..5])],
            empty: None,
            more: None,
        }
    );
    assert_eq!(
        c.columns[1],
        ColumnText {
            head: "Base line 2".into(),
            rows: vec![row(2, "one two", true, Vec::new())],
            empty: None,
            more: None,
        }
    );
    assert_eq!(
        c.columns[2],
        ColumnText {
            head: "Right line 2".into(),
            rows: vec![row(2, "ONE two", true, vec![0..3])],
            empty: None,
            more: None,
        }
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.card_text(0)),
        None,
        "a stable run has no card"
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.items().to_vec()),
        [
            Item::Row {
                chunk: 0,
                line: 0,
                first: true,
                last: true
            },
            Item::Card(1),
            Item::Row {
                chunk: 2,
                line: 2,
                first: true,
                last: true
            },
        ]
    );
    for id in [
        "merge-card",
        "merge-card-head",
        "merge-col-left",
        "merge-col-base",
        "merge-col-right",
    ] {
        assert!(drawn(cx, handle, (id, 1usize)), "{id} is drawn");
    }
    let (l, b, r) = (
        bounds_of(cx, handle, ("merge-col-left", 1usize)),
        bounds_of(cx, handle, ("merge-col-base", 1usize)),
        bounds_of(cx, handle, ("merge-col-right", 1usize)),
    );
    assert!(
        l.origin.x < b.origin.x && b.origin.x < r.origin.x,
        "side by side"
    );
    assert_eq!(l.origin.y, r.origin.y);
    // Equal shares; the left and base columns also carry a 1 px divider, and the layout rounds
    // to half pixels.
    let widths = [l.size.width, b.size.width, r.size.width];
    let (narrow, wide) = (
        widths.into_iter().fold(widths[0], |a, w| a.min(w)),
        widths.into_iter().fold(widths[0], |a, w| a.max(w)),
    );
    assert!(wide - narrow <= px(2.), "three equal columns: {widths:?}");
}

#[gpui_kit::test]
async fn empty_columns_say_what_happened(cx: &mut TestAppContext) {
    let (_handle, view) = open_texts(
        cx,
        &owned(&["a", "b", "c"]),
        &owned(&["a", "b", "c"]),
        &owned(&["a", "c"]),
    )
    .await;
    let c = card(cx, &view, 1);
    assert_eq!(
        (c.badge, c.state.as_str(), c.place.as_str()),
        ("Right changed", "✓ Use right (automatic)", "Base line 2")
    );
    assert_eq!(
        c.columns[2],
        ColumnText {
            head: "Right no lines".into(),
            rows: Vec::new(),
            empty: Some("(right removed this part)".into()),
            more: None,
        }
    );
    assert_eq!(
        c.columns[0].rows,
        [row(2, "b", false, Vec::new())],
        "the left column of a right-side change is not marked"
    );

    let (_handle, view) = open_texts(
        cx,
        &owned(&["a", "X", "c"]),
        &owned(&["a", "c"]),
        &owned(&["a", "c"]),
    )
    .await;
    let c = card(cx, &view, 1);
    assert_eq!(
        (c.badge, c.state.as_str(), c.place.as_str()),
        (
            "Left changed",
            "✓ Use left (automatic)",
            "Inserted after base line 1"
        )
    );
    assert_eq!(c.columns[0].rows, [row(2, "X", true, Vec::new())]);
    assert_eq!(c.columns[1].head, "Base no lines");
    assert_eq!(
        c.columns[1].empty.as_deref(),
        Some("(not in the base: new lines)")
    );
    assert_eq!(
        c.columns[2].empty.as_deref(),
        Some("(no lines)"),
        "the base had nothing there either"
    );
}

#[gpui_kit::test]
async fn long_stable_runs_fold_and_expand_on_click(cx: &mut TestAppContext) {
    let base = numbered(13);
    let mut left = base.clone();
    left[12] = "X".to_owned();
    let (handle, view) = open_texts(cx, &left, &base, &base).await;
    assert_eq!(
        label(cx, handle, ("merge-fold", 0usize)).as_deref(),
        Some("⋯ 6 same lines: click to expand")
    );
    assert_eq!(view.read_with(cx, |v, _| v.items().len()), 8);

    click(cx, handle, ("merge-fold", 0usize));
    assert_eq!(
        label(cx, handle, ("merge-collapse", 0usize)).as_deref(),
        Some("▴ Collapse 12 same lines")
    );
    assert!(!drawn(cx, handle, ("merge-fold", 0usize)));
    assert_eq!(view.read_with(cx, |v, _| v.items().len()), 14);
    assert!(
        drawn(cx, handle, ("merge-card", 1usize)),
        "the card follows the rows"
    );

    click(cx, handle, ("merge-collapse", 0usize));
    assert_eq!(
        label(cx, handle, ("merge-fold", 0usize)).as_deref(),
        Some("⋯ 6 same lines: click to expand")
    );
    assert!(!drawn(cx, handle, ("merge-collapse", 0usize)));
    assert_eq!(view.read_with(cx, |v, _| v.items().len()), 8);
}

#[gpui_kit::test]
async fn a_column_over_400_lines_shows_more_on_click(cx: &mut TestAppContext) {
    let mut left = owned(&["a"]);
    left.extend((0..=400).map(|i| format!("x{i}")));
    left.push("c".to_owned());
    let (handle, view) = open_texts(
        cx,
        &left,
        &owned(&["a", "b", "c"]),
        &owned(&["a", "Y", "c"]),
    )
    .await;
    let c = card(cx, &view, 1);
    assert_eq!(c.columns[0].head, "Left 2–402");
    assert_eq!(c.columns[0].rows.len(), 400);
    assert_eq!(
        c.columns[0].rows.last(),
        Some(&row(401, "x399", true, Vec::new()))
    );
    assert_eq!(
        c.columns[0].more.as_deref(),
        Some("… 1 more line: click to show all")
    );
    assert_eq!(c.columns[2].more, None);
    assert_eq!(
        label(cx, handle, ("merge-more", 1usize)).as_deref(),
        Some("… 1 more line: click to show all")
    );

    click(cx, handle, ("merge-more", 1usize));
    let c = card(cx, &view, 1);
    assert_eq!(c.columns[0].rows.len(), 401);
    assert_eq!(c.columns[0].more, None);
    assert!(!drawn(cx, handle, ("merge-more", 1usize)));
}

#[gpui_kit::test]
async fn show_all_on_a_long_conflict_builds_only_the_rows_in_view(cx: &mut TestAppContext) {
    // M-82: a frame after "show all" builds the rows its columns show, not all 3 × 3,000.
    let side = |tag: &str| {
        let mut lines = owned(&["a"]);
        lines.extend((0..3_000).map(|i| format!("{tag} {i}")));
        lines.push("c".to_owned());
        lines
    };
    let (handle, view) = open_texts(cx, &side("left"), &side("base"), &side("right")).await;
    assert!(card(cx, &view, 1).columns[0].more.is_some());
    // Each column has its link (the same id): the one the click runs.
    view.update(cx, |v, cx| v.show_all(1, cx));
    draw(cx, handle);
    let before = view.read_with(cx, |v, _| v.code_rows_built_for_test());
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let built = view.read_with(cx, |v, _| v.code_rows_built_for_test()) - before;
    assert!(built <= 300, "one frame built {built} code rows");
    assert!(!drawn(cx, handle, ("merge-more", 1usize)));
    let c = card(cx, &view, 1);
    assert_eq!(
        c.columns[0].rows.len(),
        3_000,
        "every line is there to scroll to"
    );
    assert_eq!(c.columns[2].rows.last().map(|r| r.number), Some(3_001));
}

#[gpui_kit::test]
async fn f8_f7_walk_conflicts_and_bring_the_card_into_view(cx: &mut TestAppContext) {
    let (mut left, mut base, mut right) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..30 {
        for side in [&mut left, &mut base, &mut right] {
            side.push(format!("same {i}"));
        }
        left.push(format!("left {i}"));
        base.push(format!("base {i}"));
        right.push(format!("right {i}"));
    }
    for side in [&mut left, &mut base, &mut right] {
        side.push("end".to_owned());
    }
    let (handle, view) = open_texts(cx, &left, &base, &right).await;
    let conflicts = view.read_with(cx, |v, _| v.session().unwrap().conflicts().to_vec());
    assert_eq!(conflicts.len(), 30);
    let (first, second, last) = (conflicts[0], conflicts[1], conflicts[29]);
    let visible =
        |cx: &mut TestAppContext, chunk| view.read_with(cx, |v, _| v.card_visible_for_test(chunk));
    assert_eq!(position(cx, handle), "Conflict 1 / 30");
    assert!(visible(cx, first), "the merge shows its first conflict");
    assert!(!visible(cx, last), "the list is taller than the window");
    let landed = card_offset(cx, handle, first);

    press(cx, handle, "f8");
    assert_eq!(position(cx, handle), "Conflict 2 / 30");
    assert_eq!(current(cx, &view), Some(second));
    assert!(visible(cx, second));

    press(cx, handle, "ctrl-end");
    assert_eq!(position(cx, handle), "Conflict 30 / 30");
    assert!(visible(cx, last), "the last card is brought into view");
    assert!(!visible(cx, first));
    press(cx, handle, "f8");
    assert_eq!(notice_of(cx, &view).as_deref(), Some("No next conflict."));
    assert_eq!(current(cx, &view), Some(last), "no wrap");

    press(cx, handle, "ctrl-home");
    assert_eq!(position(cx, handle), "Conflict 1 / 30");
    assert!(visible(cx, first) && !visible(cx, last));
    assert!(
        card_offset(cx, handle, first) < landed,
        "a jump puts the card at the top; the merge left the stable line above it in view"
    );
    press(cx, handle, "f7");
    assert_eq!(
        notice_of(cx, &view).as_deref(),
        Some("No previous conflict.")
    );
    assert_eq!(position(cx, handle), "Conflict 1 / 30");
    assert!(!enabled(cx, handle, "merge-prev") && enabled(cx, handle, "merge-next"));

    // A disabled ◀ does nothing; ▶ walks on.
    view.update(cx, |v, cx| v.show_notice("Before the click.", cx));
    click(cx, handle, "merge-prev");
    assert_eq!(notice_of(cx, &view).as_deref(), Some("Before the click."));
    click(cx, handle, "merge-next");
    assert_eq!(position(cx, handle), "Conflict 2 / 30");
    assert!(enabled(cx, handle, "merge-prev"));

    // No conflicts: the changes, with their own word.
    let (handle, view) = open_texts(
        cx,
        &owned(&["a", "L", "c", "d", "e"]),
        &owned(&["a", "b", "c", "d", "e"]),
        &owned(&["a", "b", "c", "R", "e"]),
    )
    .await;
    assert_eq!(position(cx, handle), "Change 1 / 2");
    press(cx, handle, "f8");
    assert_eq!(position(cx, handle), "Change 2 / 2");
    press(cx, handle, "f8");
    assert_eq!(notice_of(cx, &view).as_deref(), Some("No next change."));

    let same = owned(&["a"]);
    let (handle, view) = open_texts(cx, &same, &same, &same).await;
    press(cx, handle, "f8");
    assert_eq!(notice_of(cx, &view).as_deref(), Some("No differences."));
    assert_eq!(position(cx, handle), "Change – / 0");
    assert!(!enabled(cx, handle, "merge-prev") && !enabled(cx, handle, "merge-next"));
}

#[gpui_kit::test]
async fn a_merge_brings_its_first_conflict_into_view(cx: &mut TestAppContext) {
    // Thirty changes on the left only, then one conflict: the list is taller than the window.
    let (mut left, mut base, mut right) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..30 {
        for side in [&mut left, &mut base, &mut right] {
            side.push(format!("same {i}"));
        }
        left.push(format!("left {i}"));
        base.push(format!("base {i}"));
        right.push(format!("base {i}"));
    }
    for (side, line) in [(&mut left, "L"), (&mut base, "B"), (&mut right, "R")] {
        side.push("same".to_owned());
        side.push(line.to_owned());
    }
    let (handle, view) = open_texts(cx, &left, &base, &right).await;
    let (changes, conflicts) = view.read_with(cx, |v, _| {
        let s = v.session().unwrap();
        (s.changes().to_vec(), s.conflicts().to_vec())
    });
    assert_eq!((changes.len(), conflicts.len()), (31, 1));
    assert_eq!(position(cx, handle), "Conflict 1 / 1");
    let visible =
        |cx: &mut TestAppContext, chunk| view.read_with(cx, |v, _| v.card_visible_for_test(chunk));
    assert!(visible(cx, conflicts[0]), "the conflict is scrolled to");
    assert!(!visible(cx, changes[0]), "past the changes above it");
}

#[gpui_kit::test]
async fn stage_switch_and_navigation_from_the_input_stage(cx: &mut TestAppContext) {
    let (handle, view) = open_texts(
        cx,
        &owned(&["a", "X1", "c", "X2", "e"]),
        &owned(&["a", "b", "c", "d", "e"]),
        &owned(&["a", "Y1", "c", "Y2", "e"]),
    )
    .await;
    assert_eq!(view.read_with(cx, |v, _| v.stage()), Stage::Merge);
    assert!(drawn(cx, handle, "merge-list"));
    assert!(enabled(cx, handle, "merge-stage-merge"));
    click(cx, handle, "merge-stage-input");
    assert_eq!(view.read_with(cx, |v, _| v.stage()), Stage::Input);
    assert!(drawn(cx, handle, "merge-input-bar") && drawn(cx, handle, "merge-toolbar"));
    press(cx, handle, "f8");
    assert_eq!(
        view.read_with(cx, |v, _| v.stage()),
        Stage::Merge,
        "navigation shows the merge"
    );
    assert_eq!(position(cx, handle), "Conflict 2 / 2");
    click(cx, handle, "merge-stage-input");
    click(cx, handle, "merge-stage-merge");
    assert_eq!(view.read_with(cx, |v, _| v.stage()), Stage::Merge);

    // From a box: Ctrl+End stays with it (guarded); F7 acts and takes the keys back (free).
    click(cx, handle, "merge-stage-input");
    let out: Entity<InputState> = view.read_with(cx, |v, _| v.output_input_for_test());
    cx.update_window(handle, |_, window, cx| {
        window.focus(&out.focus_handle(cx), cx)
    })
    .unwrap();
    press(cx, handle, "ctrl-end");
    assert_eq!(view.read_with(cx, |v, _| v.stage()), Stage::Input);
    assert!(!view_has_the_keys(cx, handle, &view));
    press(cx, handle, "f7");
    assert_eq!(view.read_with(cx, |v, _| v.stage()), Stage::Merge);
    assert_eq!(position(cx, handle), "Conflict 1 / 2");
    assert!(
        view_has_the_keys(cx, handle, &view),
        "the box gave the keys back"
    );

    // An empty tab: nothing to show or walk.
    let (handle, view) = open_view(cx);
    draw(cx, handle);
    assert_eq!(position(cx, handle), "Conflict 0 / 0");
    assert!(!enabled(cx, handle, "merge-stage-merge"));
    assert!(!enabled(cx, handle, "merge-prev") && !enabled(cx, handle, "merge-next"));
    click(cx, handle, "merge-stage-merge");
    press(cx, handle, "f8");
    view.read_with(cx, |v, _| {
        assert_eq!(v.stage(), Stage::Input);
        assert_eq!(v.notice(), None);
    });
}

#[gpui_kit::test]
async fn clicking_a_card_makes_it_current(cx: &mut TestAppContext) {
    let (handle, view) = open_texts(
        cx,
        &owned(&["a", "X1", "c", "X2", "e"]),
        &owned(&["a", "b", "c", "d", "e"]),
        &owned(&["a", "Y1", "c", "Y2", "e"]),
    )
    .await;
    assert_eq!(current(cx, &view), Some(1));
    click(cx, handle, ("merge-card-head", 3usize));
    assert_eq!(current(cx, &view), Some(3));
    assert_eq!(position(cx, handle), "Conflict 2 / 2");
    click(cx, handle, ("merge-col-base", 1usize));
    assert_eq!(current(cx, &view), Some(1), "anywhere on the card");
}

#[gpui_kit::test]
async fn the_banners_say_when_there_is_little_to_show(cx: &mut TestAppContext) {
    let same = owned(&["a", "b"]);
    let (handle, _view) = open_texts(cx, &same, &same, &same).await;
    assert_eq!(
        label(cx, handle, "merge-banner-same").as_deref(),
        Some("✓ The three files are the same: nothing to merge.")
    );
    assert!(!drawn(cx, handle, "merge-banner-empty"));

    let (handle, _view) = open_texts(cx, &[], &[], &[]).await;
    assert!(drawn(cx, handle, "merge-banner-same"));
    assert_eq!(
        label(cx, handle, "merge-banner-empty").as_deref(),
        Some("The three files are empty.")
    );

    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let file = cx
        .update(|cx| duet_app::settings::settings_file(cx))
        .unwrap();
    duet_app::settings::set_bool(&file, "merge.trail", true).unwrap();
    let (handle, view) = open_texts(
        cx,
        &owned(&["a  ", "X"]),
        &owned(&["a", "b"]),
        &owned(&["a", "Y"]),
    )
    .await;
    assert!(view.read_with(cx, |v, _| v.session().unwrap().trail()));
    assert_eq!(current(cx, &view), Some(1), "the conflict is current");
    assert_eq!(
        label(cx, handle, "merge-banner-trail").as_deref(),
        Some("Ignoring trailing whitespace: the parts that match use the left text."),
        "the note heads the list"
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.items()[1]),
        Item::Row {
            chunk: 0,
            line: 0,
            first: true,
            last: true
        },
        "the matching line follows the note as one stable row"
    );
}

#[gpui_kit::test]
async fn the_walk_buttons_name_what_they_walk(cx: &mut TestAppContext) {
    let same = owned(&["a", "b", "c"]);
    let (handle, _view) = open_texts(cx, &owned(&["a", "X", "c"]), &same, &same).await;
    assert_eq!(
        label(cx, handle, "merge-prev").as_deref(),
        Some("Previous change (F7)")
    );
    assert_eq!(
        label(cx, handle, "merge-next").as_deref(),
        Some("Next change (F8)")
    );
    let [l, b, r] = one_conflict();
    let (handle, _view) = open_texts(cx, &l, &b, &r).await;
    assert_eq!(
        label(cx, handle, "merge-prev").as_deref(),
        Some("Previous conflict (F7)")
    );
    assert_eq!(
        label(cx, handle, "merge-next").as_deref(),
        Some("Next conflict (F8)")
    );
}
