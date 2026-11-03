//! The merge tab's slots, load and merge jobs, title, badge and recent entry, and the
//! input stage (Tasks 3 and 4).

use crate::common::*;

#[gpui_kit::test]
async fn a_merge_tab_reads_three_files_and_merges(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (_dir, l, b, r) = write_set("a\nB\nc\nd\ne\n", "a\nb\nc\nd\ne\n", "a\nb\nc\nD\ne\n");
    let (_handle, workspace, view) = open_workspace(cx, paths(&l, &b, &r)).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.stage(), Stage::Merge);
        let session = v.session().expect("a session");
        assert_eq!(session.changes().len(), 2);
        assert!(session.conflicts().is_empty());
        assert_eq!(
            v.notice(),
            Some("Merged 2 changes automatically: no conflicts.")
        );
        assert_eq!(v.slot(LEFT).unwrap().name, "left.txt");
        assert_eq!(v.slot(BASE).unwrap().name, "base.txt");
        assert_eq!(v.slot(RIGHT).unwrap().name, "right.txt");
    });
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().iter().next().unwrap().1.title()),
        "Merge: left.txt ↔ right.txt"
    );
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        Some("ok".to_owned())
    );
    let recent = std::fs::read_to_string(data.path().join("recent.txt")).unwrap();
    assert_eq!(
        recent.lines().next(),
        Some(format!("M\t{}\t{}\t{}", l.display(), r.display(), b.display()).as_str()),
        "the recent entry runs left, right, base, as on the command line"
    );
}

#[gpui_kit::test]
async fn a_conflict_badges_its_count_and_becomes_current(cx: &mut TestAppContext) {
    let (_dir, l, b, r) = write_set("a\nX\nc\n", "a\nb\nc\n", "a\nY\nc\n");
    let (_handle, workspace, view) = open_workspace(cx, paths(&l, &b, &r)).await;
    view.read_with(cx, |v, _| {
        let session = v.session().expect("a session");
        assert_eq!(session.current(), Some(1));
        assert_eq!(v.badge(), Some("1".to_owned()));
        assert_eq!(
            v.notice(),
            Some("Merged 0 changes automatically; 1 conflict needs a choice.")
        );
    });
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        Some("1".to_owned())
    );

    let (_dir2, l2, b2, r2) = write_set("same\n", "same\n", "same\n");
    let (_handle, _workspace, same) = open_workspace(cx, paths(&l2, &b2, &r2)).await;
    same.read_with(cx, |v, _| {
        assert_eq!(v.badge(), Some("ok".to_owned()));
        assert_eq!(
            v.notice(),
            Some("The three files are the same: nothing to merge.")
        );
    });
}

#[gpui_kit::test]
async fn a_missing_or_binary_slot_says_why(cx: &mut TestAppContext) {
    let (_dir, l, b, r) = write_set("a\n", "a\n", "a\n");
    std::fs::remove_file(&b).unwrap();
    let (_handle, workspace, view) = open_workspace(cx, paths(&l, &b, &r)).await;
    view.read_with(cx, |v, _| {
        assert!(
            v.slot_message(BASE)
                .is_some_and(|m| m.starts_with("Can't open base.txt: ")),
            "{:?}",
            v.slot_message(BASE)
        );
        assert!(v.slot(BASE).is_none() && v.session().is_none());
        assert_eq!(v.badge(), None);
        assert!(!v.is_busy());
        assert!(v.slot(LEFT).is_some(), "the other slots still load");
    });
    assert_eq!(workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)), None);

    let (dir2, l2, b2, _r2) = write_set("a\n", "a\n", "a\n");
    let bin = dir2.path().join("r.bin");
    std::fs::write(&bin, [b'a', 0]).unwrap();
    let (_handle, _workspace, view) = open_workspace(cx, paths(&l2, &b2, &bin)).await;
    view.read_with(cx, |v, _| {
        assert_eq!(
            v.slot_message(RIGHT),
            Some("r.bin is a binary file: only text files can be merged.")
        );
        assert!(v.session().is_none() && !v.is_busy());
    });
}

#[gpui_kit::test]
async fn a_new_file_while_loading_replaces_the_pending_load(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        let path = dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        path
    };
    let (a, b) = (write("a.txt", "one\nx\n"), write("b.txt", "two\nx\n"));
    let (base, right) = (write("base.txt", "x\n"), write("right.txt", "x\n"));
    let (handle, view) = open_view(cx);
    view.update(cx, |v, cx| {
        v.open_slot_path(LEFT, a, cx);
        // Before the executor runs: the second file replaces the pending load.
        v.open_slot_path(LEFT, b.clone(), cx);
        v.open_slot_path(BASE, base, cx);
        v.open_slot_path(RIGHT, right, cx);
    });
    wait_idle(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.slot(LEFT).unwrap().name, "b.txt");
        let session = v.session().expect("the three files merged");
        assert_eq!(session.left().text(), "two\nx\n", "the merge used b's text");
    });

    // R13: a file that fails to open leaves no older file behind.
    view.update(cx, |v, cx| {
        v.open_slot_path(LEFT, dir.path().join("missing.txt"), cx)
    });
    wait_idle(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert!(v.slot(LEFT).is_none());
        assert!(
            v.slot_message(LEFT)
                .is_some_and(|m| m.starts_with("Can't open ")),
            "{:?}",
            v.slot_message(LEFT)
        );
        assert!(!v.is_busy());
    });
}

#[gpui_kit::test]
async fn escape_cancels_a_running_merge_first(cx: &mut TestAppContext) {
    let (_dir, l, b, r) = write_set("a\nX\nc\n", "a\nb\nc\n", "a\nY\nc\n");
    let (handle, view) = open_view(cx);
    view.update(cx, |v, cx| {
        v.open_slot_path(LEFT, l, cx);
        v.open_slot_path(BASE, b, cx);
        v.open_slot_path(RIGHT, r, cx);
    });
    assert!(view.read_with(cx, |v, _| v.is_busy()), "the loads wait");
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    view.read_with(cx, |v, _| {
        assert_eq!(v.notice(), Some(CANCELLED));
        assert!(!v.is_busy());
    });
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        assert!(v.session().is_none(), "a cancelled load never lands");
        assert!(v.slot(LEFT).is_none() && !v.is_busy());
    });
}

#[gpui_kit::test]
async fn opening_the_three_slots_merges_on_its_own(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (dir, l, b, r) = write_set("a\nL\nc\n", "a\nb\nc\n", "a\nb\nc\n");
    let (handle, workspace, view) = open_workspace(cx, MergePaths::default()).await;
    assert_eq!(view.read_with(cx, |v, _| v.stage()), Stage::Input);

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("merge-open-base", cx);
    })
    .unwrap();
    assert!(cx.did_prompt_for_paths(), "Open… asks for a file");
    let picked = b.clone();
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.files && !options.directories && !options.multiple);
        Some(vec![picked])
    });
    cx.run_until_parked();
    wait_idle(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.slot(BASE).unwrap().name, "base.txt");
        assert_eq!(v.stage(), Stage::Input, "two slots are still empty");
        assert!(v.session().is_none());
    });

    view.update(cx, |v, cx| {
        v.open_slot_path(LEFT, l.clone(), cx);
        v.open_slot_path(RIGHT, r.clone(), cx);
    });
    wait_idle(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.stage(), Stage::Merge);
        assert!(v.session().is_some());
    });
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().iter().next().unwrap().1.title()),
        "Merge: left.txt ↔ right.txt"
    );

    // A fresh file in a slot of a clean merge merges again.
    let fresh = dir.path().join("fresh.txt");
    std::fs::write(&fresh, "a\nZ\nc\n").unwrap();
    view.update(cx, |v, cx| v.open_slot_path(LEFT, fresh, cx));
    wait_idle(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.session().unwrap().left().text(), "a\nZ\nc\n");
    });
}

#[gpui_kit::test]
async fn the_slots_name_their_files_and_formats(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, bytes: &[u8]| {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    };
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(b"a\r\nb");
    let left = write("left.txt", &bom);
    let base = write("base.txt", b"only\n");
    let right = write("right.txt", b"a\r\nb\nc\n");
    let empty = write("empty.txt", b"");
    let (handle, view) = open_view(cx);
    view.update(cx, |v, cx| v.open_slot_path(LEFT, left.clone(), cx));
    assert_eq!(
        slot_text(cx, &view, LEFT),
        SlotText {
            name: "left.txt".into(),
            meta: "Reading…".into(),
            filled: false,
            edited: false,
            failed: false,
        },
        "a slot being read says so"
    );
    view.update(cx, |v, cx| {
        v.open_slot_path(BASE, base, cx);
        v.open_slot_path(RIGHT, right, cx);
    });
    wait_idle(cx, handle, &view).await;
    assert_eq!(
        slot_text(cx, &view, LEFT),
        SlotText {
            name: "left.txt".into(),
            meta: format!(
                "2 lines · {} · UTF-8 (BOM) · CRLF · no newline at the end",
                human_size(7)
            ),
            filled: true,
            edited: false,
            failed: false,
        }
    );
    assert_eq!(
        slot_text(cx, &view, BASE).meta,
        format!("1 line · {} · UTF-8 · LF", human_size(5))
    );
    assert_eq!(
        slot_text(cx, &view, RIGHT).meta,
        format!("3 lines · {} · UTF-8 · LF (mixed)", human_size(7))
    );
    view.update(cx, |v, cx| v.open_slot_path(BASE, empty, cx));
    wait_idle(cx, handle, &view).await;
    assert_eq!(
        slot_text(cx, &view, BASE).meta,
        format!("0 lines · {} · UTF-8 · LF", human_size(0)),
        "an empty file has no missing newline"
    );

    // Emptied slots, and a file that cannot be read.
    view.update(cx, |v, cx| {
        v.open_slot_path(RIGHT, dir.path().join("missing.txt"), cx)
    });
    wait_idle(cx, handle, &view).await;
    let failed = slot_text(cx, &view, RIGHT);
    assert!(failed.failed && !failed.filled && !failed.edited);
    assert_eq!(failed.name, "missing.txt");
    assert_eq!(
        Some(failed.meta.as_str()),
        view.read_with(cx, |v, _| v.slot_message(RIGHT).map(str::to_owned))
            .as_deref(),
        "the meta is the message"
    );
    in_view(cx, handle, &view, |v, window, cx| {
        v.clear_slot(LEFT, window, cx)
    });
    assert_eq!(
        slot_text(cx, &view, LEFT),
        SlotText {
            name: "No file chosen".into(),
            meta: "Press “Open…” or drop a file here".into(),
            filled: false,
            edited: false,
            failed: false,
        }
    );
}

#[gpui_kit::test]
async fn paste_mode_merges_typed_text_and_edited_files_keep_their_format(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        let path = dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        path
    };
    let (left, base) = (
        write("left.txt", "one\ntwo\n"),
        write("base.txt", "one\ntwo\n"),
    );
    let (handle, view) = open_view(cx);
    view.update(cx, |v, cx| {
        v.open_slot_path(LEFT, left, cx);
        v.open_slot_path(BASE, base, cx);
    });
    wait_idle(cx, handle, &view).await;
    set_mode(cx, handle, &view, InputMode::Paste);
    assert_eq!(view.read_with(cx, |v, _| v.mode()), InputMode::Paste);
    assert_eq!(
        field_text(cx, &view, LEFT),
        "one\ntwo\n",
        "a file's text fills its field"
    );
    assert_eq!(field_text(cx, &view, BASE), "one\ntwo\n");
    assert_eq!(field_text(cx, &view, RIGHT), "");
    assert_eq!(
        view.read_with(cx, |v, _| texts(&v.status_parts())),
        ["Paste mode: type or paste the text of all three sides, then press Merge."]
    );
    assert!(
        !slot_text(cx, &view, LEFT).edited,
        "filling a field is not an edit"
    );

    focus_field(cx, handle, &view, LEFT);
    type_lines(cx, handle, &["X"]);
    assert_eq!(
        slot_text(cx, &view, LEFT),
        SlotText {
            name: "left.txt (edited)".into(),
            meta: "Uses the text in the box below · keeps UTF-8 · LF".into(),
            filled: true,
            edited: true,
            failed: false,
        }
    );
    assert!(
        view.read_with(cx, |v, _| v.can_run()),
        "paste mode runs with an empty side"
    );
    assert!(field_text(cx, &view, LEFT).starts_with('X'));
}

#[gpui_kit::test]
async fn typed_text_merges_without_a_file_or_a_recent_entry(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    with_data_dir(cx, data.path());
    let (handle, workspace, view) = open_workspace(cx, MergePaths::default()).await;
    let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let log = events.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &MergeEvent, _| {
            log.borrow_mut().push(event.clone());
        })
    });
    set_mode(cx, handle, &view, InputMode::Paste);
    for (slot, lines) in [
        (LEFT, ["a", "L", "c"]),
        (BASE, ["a", "b", "c"]),
        (RIGHT, ["a", "b", "c"]),
    ] {
        focus_field(cx, handle, &view, slot);
        type_lines(cx, handle, &lines);
    }
    assert_eq!(
        slot_text(cx, &view, LEFT),
        SlotText {
            name: "Typed or pasted text".into(),
            meta: "Uses the text in the box below".into(),
            filled: true,
            edited: true,
            failed: false,
        }
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).can_run(), "the three fields are filled");
        window.click("merge-run", cx);
    })
    .unwrap();
    wait_idle(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.stage(), Stage::Merge);
        let left = v.session().expect("a session").left();
        assert_eq!(left.text(), "a\nL\nc", "the typed text, as typed");
        assert!(left.path.is_none() && !left.encoding.bom);
        assert_eq!(left.encoding.encoding.name(), "UTF-8");
    });
    assert!(matches!(
        events.borrow().as_slice(),
        [MergeEvent::Merged {
            left: None,
            base: None,
            right: None
        }]
    ));
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().iter().next().unwrap().1.title()),
        "3-way merge"
    );
    assert!(
        !data.path().join("recent.txt").exists(),
        "a set without files stays out of the recent list"
    );
}

#[gpui_kit::test]
async fn drops_fill_the_slots_in_localdiffs_order(cx: &mut TestAppContext) {
    let (dir, l, b, r) = write_set("a\nL\nc\n", "a\nb\nc\n", "a\nb\nc\n");
    let other = |name: &str| {
        let path = dir.path().join(name);
        std::fs::write(&path, "a\nb\nc\n").unwrap();
        path
    };
    let (x, y) = (other("x.txt"), other("y.txt"));
    let (handle, view) = open_view(cx);

    // Three files: left, base, right, in files mode, and merged.
    set_mode(cx, handle, &view, InputMode::Paste);
    drop_files(cx, handle, &view, &[l.clone(), b.clone(), r.clone()], None);
    wait_idle(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(
            v.mode(),
            InputMode::Files,
            "three files switch to files mode"
        );
        assert_eq!(v.slot(LEFT).unwrap().name, "left.txt");
        assert_eq!(v.slot(BASE).unwrap().name, "base.txt");
        assert_eq!(v.slot(RIGHT).unwrap().name, "right.txt");
        assert_eq!(v.stage(), Stage::Merge);
    });

    // A drop on the merge stage returns to the input stage; with no pointer slot and all
    // slots filled the file goes to the left.
    drop_files(cx, handle, &view, std::slice::from_ref(&x), None);
    assert_eq!(view.read_with(cx, |v, _| v.stage()), Stage::Input);
    wait_idle(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.slot(LEFT).unwrap().name.clone()),
        "x.txt"
    );

    // One file on a slot.
    in_view(cx, handle, &view, |v, window, cx| v.clear_all(window, cx));
    drop_files(cx, handle, &view, std::slice::from_ref(&b), Some(BASE));
    wait_idle(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.slot(BASE).unwrap().name, "base.txt");
        assert!(v.slot(LEFT).is_none() && v.slot(RIGHT).is_none());
    });

    // One file and no pointer slot: the first empty slot (left is empty, base is filled).
    drop_files(cx, handle, &view, std::slice::from_ref(&y), None);
    wait_idle(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.slot(LEFT).unwrap().name.clone()),
        "y.txt"
    );
    // And with left filled: the next empty one is right.
    drop_files(cx, handle, &view, std::slice::from_ref(&r), None);
    wait_idle(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.slot(RIGHT).unwrap().name.clone()),
        "right.txt"
    );

    // Two files starting at the right slot wrap round to the left.
    in_view(cx, handle, &view, |v, window, cx| v.clear_all(window, cx));
    drop_files(cx, handle, &view, &[x.clone(), y.clone()], Some(RIGHT));
    wait_idle(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.slot(RIGHT).unwrap().name, "x.txt");
        assert_eq!(v.slot(LEFT).unwrap().name, "y.txt");
        assert!(v.slot(BASE).is_none());
    });

    // A folder is refused with the folder notice.
    in_view(cx, handle, &view, |v, window, cx| v.clear_all(window, cx));
    drop_files(cx, handle, &view, &[dir.path().to_path_buf()], None);
    view.read_with(cx, |v, _| {
        assert_eq!(v.notice(), Some(FOLDER_DROP_NOTICE));
        assert!(!v.is_busy() && v.slot(LEFT).is_none());
    });
}

#[gpui_kit::test]
async fn clear_and_the_output_box(cx: &mut TestAppContext) {
    let (_dir, l, b, r) = write_set("a\nL\nc\n", "a\nb\nc\n", "a\nb\nc\n");
    let (handle, view) = open_view(cx);
    view.update(cx, |v, cx| {
        v.open_slot_path(LEFT, l, cx);
        v.open_slot_path(BASE, b, cx);
        v.open_slot_path(RIGHT, r, cx);
    });
    wait_idle(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.can_run() && v.session().is_some()));

    // The slot's × empties it.
    in_view(cx, handle, &view, |v, _, cx| v.set_stage(Stage::Input, cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("merge-clear-left", cx);
    })
    .unwrap();
    view.read_with(cx, |v, _| {
        assert!(v.slot(LEFT).is_none() && v.slot(BASE).is_some());
        assert!(!v.can_run(), "a slot is empty");
    });

    // The output box: quotes allowed, empty means none.
    let out: Entity<InputState> = view.read_with(cx, |v, _| v.output_input_for_test());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.focus(&out.focus_handle(cx), cx);
        window.input("\"C:\\out.txt\"", cx);
    })
    .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.output().map(Path::to_path_buf)),
        Some(PathBuf::from("C:\\out.txt"))
    );

    // Clear all: slots and session go, the output stays.
    in_view(cx, handle, &view, |v, window, cx| v.clear_all(window, cx));
    view.read_with(cx, |v, _| {
        assert!((LEFT..=RIGHT).all(|s| v.slot(s).is_none()));
        assert!(v.session().is_none());
        assert_eq!(v.stage(), Stage::Input);
        assert_eq!(v.output(), Some(Path::new("C:\\out.txt")));
        assert_eq!(texts(&v.status_parts()), ["0 of 3 files chosen"]);
    });

    cx.update_window(handle, |_, window, cx| {
        window.press("ctrl-a", cx);
        window.press("backspace", cx);
    })
    .unwrap();
    assert_eq!(
        view.read_with(cx, |v, _| v.output().map(Path::to_path_buf)),
        None
    );
}

#[gpui_kit::test]
async fn the_input_stage_draws_its_parts_and_the_mode_buttons_switch(cx: &mut TestAppContext) {
    let (handle, view) = open_view(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for id in [
            "merge-mode-files",
            "merge-mode-paste",
            "merge-output",
            "merge-swap",
            "merge-clear",
            "merge-run",
            "merge-slot-left",
            "merge-slot-base",
            "merge-slot-right",
            "merge-name-left",
            "merge-meta-base",
            "merge-open-right",
            "merge-clear-right",
            "merge-hint",
            "merge-status",
        ] {
            assert!(window.try_find(id).is_some(), "{id} is drawn");
        }
        assert!(
            window.try_find("merge-paste-left").is_none(),
            "no fields in files mode"
        );
        assert!(!view.read(cx).can_run(), "no files yet");
        window.click("merge-mode-paste", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("merge-hint").is_none(),
            "the hint goes in paste mode"
        );
        for id in ["merge-paste-left", "merge-paste-base", "merge-paste-right"] {
            assert!(window.try_find(id).is_some(), "{id} is drawn");
        }
        assert!(view.read(cx).can_run(), "paste mode merges empty sides too");
        for id in ["merge-paste-left", "merge-paste-base", "merge-paste-right"] {
            assert!(
                window.find(id).bounds().size.height > px(150.),
                "{id} has room to type"
            );
        }
        let (a, b, c) = (
            window.find("merge-slot-left").bounds(),
            window.find("merge-slot-base").bounds(),
            window.find("merge-slot-right").bounds(),
        );
        assert!(
            a.origin.x < b.origin.x && b.origin.x < c.origin.x,
            "side by side"
        );
        assert_eq!(a.origin.y, c.origin.y);
        window.click("merge-mode-files", cx);
        window.render_frame(cx);
        assert!(window.try_find("merge-hint").is_some());
    })
    .unwrap();
    assert_eq!(view.read_with(cx, |v, _| v.mode()), InputMode::Files);
}

#[gpui_kit::test]
async fn at_600_px_the_slots_clip_nothing(cx: &mut TestAppContext) {
    // M-21: in a 600 px window each slot's Open… and × stay whole inside it, in both modes.
    let (handle, view) = cx.update(|cx| {
        gpui_kit::init(cx);
        duet_app::app::configure_theme(gpui_kit::component::ThemeMode::Light, cx);
        cx.bind_keys(duet_app::app::key_bindings());
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(600.), px(700.)),
        };
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| MergeView::new(MergePaths::default(), window, cx)),
        )
        .expect("open test window")
    });
    let slots = [
        ("merge-slot-left", "merge-open-left", "merge-clear-left"),
        ("merge-slot-base", "merge-open-base", "merge-clear-base"),
        ("merge-slot-right", "merge-open-right", "merge-clear-right"),
    ];
    for mode in ["merge-mode-files", "merge-mode-paste"] {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click(mode, cx);
            window.render_frame(cx);
            let width = window.viewport_size().width;
            let mut previous_right = px(0.);
            for (slot, open, clear) in slots {
                let (s, o, c) = (
                    window.find(slot).bounds(),
                    window.find(open).bounds(),
                    window.find(clear).bounds(),
                );
                assert!(
                    s.left() >= previous_right,
                    "{mode}: {slot} overlaps the one before"
                );
                assert!(s.right() <= width, "{mode}: {slot} runs past the window");
                assert!(
                    o.left() >= s.left() && o.right() <= c.left(),
                    "{mode}: {open}"
                );
                assert!(
                    c.right() <= s.right(),
                    "{mode}: {clear} sticks out of {slot}"
                );
                previous_right = s.right();
            }
        })
        .unwrap();
    }
    assert_eq!(view.read_with(cx, |v, _| v.mode()), InputMode::Paste);
}

#[gpui_kit::test]
async fn a_file_too_large_to_edit_keeps_files_mode(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    // Over 50,000 lines, and over 2 MiB in one line.
    let many = dir.path().join("many.txt");
    std::fs::write(&many, "x\n".repeat(60_000)).unwrap();
    let wide = dir.path().join("wide.txt");
    std::fs::write(&wide, "y".repeat(2 * 1024 * 1024 + 1)).unwrap();
    let small = dir.path().join("small.txt");
    std::fs::write(&small, "ok\n").unwrap();
    let (handle, view) = open_view(cx);
    for big in [many, wide] {
        view.update(cx, |v, cx| {
            v.open_slot_path(LEFT, big, cx);
            v.open_slot_path(BASE, small.clone(), cx);
        });
        wait_idle(cx, handle, &view).await;
        set_mode(cx, handle, &view, InputMode::Paste);
        view.read_with(cx, |v, _| {
            assert_eq!(v.mode(), InputMode::Files);
            assert_eq!(
                v.notice(),
                Some("This file is too large to edit here. Use files mode.")
            );
        });
        assert_eq!(
            field_text(cx, &view, LEFT),
            "",
            "nothing was copied into a field"
        );
    }
    // Without the big file the switch works and copies the small one.
    in_view(cx, handle, &view, |v, window, cx| {
        v.clear_slot(LEFT, window, cx)
    });
    set_mode(cx, handle, &view, InputMode::Paste);
    assert_eq!(view.read_with(cx, |v, _| v.mode()), InputMode::Paste);
    assert_eq!(field_text(cx, &view, BASE), "ok\n");
}

#[gpui_kit::test]
async fn escape_in_the_output_box_or_a_paste_field_returns_focus_to_the_view(
    cx: &mut TestAppContext,
) {
    let (_dir, l, _b, _r) = write_set("a\n", "a\n", "a\n");
    let (handle, view) = open_view(cx);
    view.update(cx, |v, cx| v.open_slot_path(LEFT, l, cx));
    assert!(view.read_with(cx, |v, _| v.is_busy()), "a load is running");

    let out: Entity<InputState> = view.read_with(cx, |v, _| v.output_input_for_test());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.focus(&out.focus_handle(cx), cx);
    })
    .unwrap();
    assert!(!view_has_the_keys(cx, handle, &view));
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    assert!(
        view_has_the_keys(cx, handle, &view),
        "Esc leaves the output box"
    );
    view.read_with(cx, |v, _| {
        assert!(v.is_busy(), "and cancels nothing");
        assert_ne!(v.notice(), Some(CANCELLED));
    });

    set_mode(cx, handle, &view, InputMode::Paste);
    focus_field(cx, handle, &view, BASE);
    assert!(!view_has_the_keys(cx, handle, &view));
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    assert!(
        view_has_the_keys(cx, handle, &view),
        "Esc leaves a paste field"
    );
    assert!(
        view.read_with(cx, |v, _| v.is_busy()),
        "and cancels nothing"
    );

    // With the view focused the next Esc cancels the work.
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    view.read_with(cx, |v, _| {
        assert_eq!(v.notice(), Some(CANCELLED));
        assert!(!v.is_busy());
    });
}

#[gpui_kit::test]
async fn ctrl_enter_runs_the_merge_from_a_paste_field(cx: &mut TestAppContext) {
    let (handle, view) = open_view(cx);
    set_mode(cx, handle, &view, InputMode::Paste);
    for (slot, text) in [(LEFT, "L"), (BASE, "b"), (RIGHT, "b")] {
        focus_field(cx, handle, &view, slot);
        type_lines(cx, handle, &[text]);
    }
    cx.update_window(handle, |_, window, cx| window.press("ctrl-enter", cx))
        .unwrap();
    assert_eq!(
        field_text(cx, &view, RIGHT),
        "b",
        "Ctrl+Enter types no newline in the field"
    );
    wait_idle(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.stage(), Stage::Merge, "it ran the merge");
        assert_eq!(v.session().unwrap().changes().len(), 1);
    });
}

#[gpui_kit::test]
async fn ctrl_o_picks_a_slot_unless_a_box_has_the_keys(cx: &mut TestAppContext) {
    let (handle, view) = open_view(cx);
    let out: Entity<InputState> = view.read_with(cx, |v, _| v.output_input_for_test());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.focus(&out.focus_handle(cx), cx);
        window.press("ctrl-o", cx);
    })
    .unwrap();
    assert!(!cx.did_prompt_for_paths(), "a box keeps the keys");

    let view_focus = view.read_with(cx, |v, cx| v.focus_handle(cx));
    for key in ["ctrl-o", "ctrl-shift-o"] {
        cx.update_window(handle, |_, window, cx| {
            window.focus(&view_focus, cx);
            window.press(key, cx);
        })
        .unwrap();
        assert!(cx.did_prompt_for_paths(), "{key} asks for a file");
        cx.simulate_path_prompt_response(|_| None);
    }
}

#[gpui_kit::test]
async fn ctrl_enter_runs_the_merge_from_the_output_box(cx: &mut TestAppContext) {
    let (handle, view) = open_view(cx);
    set_mode(cx, handle, &view, InputMode::Paste);
    for (slot, text) in [(LEFT, "L"), (BASE, "b"), (RIGHT, "b")] {
        focus_field(cx, handle, &view, slot);
        type_lines(cx, handle, &[text]);
    }
    let out: Entity<InputState> = view.read_with(cx, |v, _| v.output_input_for_test());
    cx.update_window(handle, |_, window, cx| {
        window.focus(&out.focus_handle(cx), cx);
        window.press("ctrl-enter", cx);
    })
    .unwrap();
    wait_idle(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.stage()), Stage::Merge);
}

#[gpui_kit::test]
async fn an_edited_file_keeps_its_encoding_bom_and_line_ending(cx: &mut TestAppContext) {
    use duet_core::text::merge::Eol;
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"a\r\nb\r\nc\r\n");
    let left = dir.path().join("left.txt");
    std::fs::write(&left, bytes).unwrap();
    let plain = dir.path().join("plain.txt");
    std::fs::write(&plain, "a\nb\nc\n").unwrap();
    let (handle, view) = open_view(cx);
    view.update(cx, |v, cx| {
        v.open_slot_path(LEFT, left, cx);
        v.open_slot_path(BASE, plain.clone(), cx);
        v.open_slot_path(RIGHT, plain, cx);
    });
    wait_idle(cx, handle, &view).await;
    // The three files merged on their own; the input stage is where the fields are.
    in_view(cx, handle, &view, |v, _, cx| v.set_stage(Stage::Input, cx));
    set_mode(cx, handle, &view, InputMode::Paste);
    assert_eq!(
        field_text(cx, &view, LEFT),
        "a\nb\nc\n",
        "the field holds the lines, LF-separated"
    );
    focus_field(cx, handle, &view, LEFT);
    type_lines(cx, handle, &["X"]);
    in_view(cx, handle, &view, |v, window, cx| v.run(false, window, cx));
    wait_idle(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let left = v.session().expect("a session").left();
        assert_eq!(left.text(), "Xa\nb\nc\n");
        assert!(left.encoding.bom, "the file's BOM is kept");
        assert_eq!(left.encoding.encoding.name(), "UTF-8");
        assert_eq!(left.format.eol, Eol::CrLf, "and its line ending");
        assert_eq!(left.name, "left.txt");
        assert!(left.path.is_none(), "but the edited text has no path");
    });
}

#[gpui_kit::test]
async fn a_cli_output_path_fills_the_output_box(cx: &mut TestAppContext) {
    let out = PathBuf::from(r"C:\merged\out.txt");
    let given = out.clone();
    let (_handle, view) = open_test_window(cx, move |window, cx| {
        cx.new(|cx| {
            MergeView::new(
                MergePaths {
                    output: Some(given),
                    ..MergePaths::default()
                },
                window,
                cx,
            )
        })
    });
    assert_eq!(
        view.read_with(cx, |v, _| v.output().map(Path::to_path_buf)),
        Some(out.clone())
    );
    let input: Entity<InputState> = view.read_with(cx, |v, _| v.output_input_for_test());
    assert_eq!(
        input.read_with(cx, |i, _| i.value().to_string()),
        out.display().to_string()
    );
}

#[gpui_kit::test]
fn an_out_of_range_slot_reads_as_blank(cx: &mut TestAppContext) {
    let (_handle, view) = open_view(cx);
    assert_eq!(slot_text(cx, &view, 3), SlotText::default());
}
