//! The merge tab in Thai: the toolbar, the cards and the input stage, and what a switch does to
//! them. A switch must keep what the person made: a typed result, an unsaved choice, the place
//! in the list and the keys of a focused field.

use std::path::{Path, PathBuf};
use std::time::Duration;

use duet_app::i18n::{self, tr, trf};
use duet_app::launch::{Launch, MergePaths};
use duet_app::merge::view::{InputMode, MergeView};
use duet_app::workspace::{TabItem, Workspace};
use duet_core::text::merge::session::{How, Nav};
use gpui_kit::component::input::TextareaState;
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, Focusable as _, TestAppContext};

use super::{open, thai_ui};

const THAI: (&str, usize) = ("header-lang", 0);
const ENGLISH: (&str, usize) = ("header-lang", 1);

fn write_set(dir: &Path, names: [&str; 3], texts: [&str; 3]) -> (PathBuf, PathBuf, PathBuf) {
    let [l, b, r] = names.map(|name| dir.join(name));
    for (path, text) in [&l, &b, &r].into_iter().zip(texts) {
        std::fs::write(path, text).unwrap();
    }
    (l, b, r)
}

fn set(l: PathBuf, b: PathBuf, r: PathBuf) -> MergePaths {
    MergePaths {
        left: Some(l),
        base: Some(b),
        right: Some(r),
        output: None,
    }
}

/// A merge tab on these files, loaded and merged.
pub(super) async fn open_merge(
    cx: &mut TestAppContext,
    paths: MergePaths,
) -> (AnyWindowHandle, Entity<Workspace>, Entity<MergeView>) {
    let (handle, workspace) = open(cx, Launch::Merge(paths));
    let ws = workspace.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        ws.read(cx).active_idle(cx)
    })
    .await;
    let view = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Merge(view)) => view.clone(),
        _ => panic!("expected a merge tab"),
    });
    (handle, workspace, view)
}

pub(super) fn conflict_set(dir: &Path) -> MergePaths {
    let (l, b, r) = write_set(
        dir,
        ["left.txt", "base.txt", "right.txt"],
        ["a\nX\nc\n", "a\nb\nc\n", "a\nY\nc\n"],
    );
    set(l, b, r)
}

/// Five conflicts a long folded run apart, so the list scrolls.
fn long_set(dir: &Path) -> MergePaths {
    let lines = |change: Option<&str>| {
        (1..=100)
            .map(|n| match change {
                Some(side) if n % 20 == 11 => format!("{side}{n}\n"),
                _ => format!("L{n}\n"),
            })
            .collect::<String>()
    };
    let (l, b, r) = write_set(
        dir,
        ["left.txt", "base.txt", "right.txt"],
        [&lines(Some("X")), &lines(None), &lines(Some("Y"))],
    );
    set(l, b, r)
}

pub(super) fn draw(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

/// Clicks with no draw after it: a dirty window draws itself when effects flush, as the real
/// window does. An explicit draw is a second frame the real window never gets.
pub(super) fn click(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    id: impl Into<gpui_kit::ElementId>,
) {
    let id = id.into();
    cx.update_window(handle, |_, window, cx| window.click(id, cx))
        .unwrap();
}

pub(super) fn label(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    id: impl Into<gpui_kit::ElementId>,
) -> Option<String> {
    let id = id.into();
    cx.update_window(handle, |_, window, _| {
        window
            .try_find(id)
            .and_then(|e| e.label().map(str::to_owned))
    })
    .unwrap()
}

fn placeholder(cx: &mut TestAppContext, field: &Entity<TextareaState>) -> String {
    field.read_with(cx, |f, _| f.presentation().placeholder().to_string())
}

fn result_placeholder(cx: &mut TestAppContext, view: &Entity<MergeView>, chunk: usize) -> String {
    let field = field_of(cx, view, chunk);
    placeholder(cx, &field)
}

fn paste_placeholder(cx: &mut TestAppContext, view: &Entity<MergeView>, slot: usize) -> String {
    let field = view.read_with(cx, |v, _| v.paste_input_for_test(slot));
    placeholder(cx, &field)
}

fn field_of(
    cx: &mut TestAppContext,
    view: &Entity<MergeView>,
    chunk: usize,
) -> Entity<TextareaState> {
    view.read_with(cx, |v, _| v.field_for_test(chunk))
        .unwrap_or_else(|| panic!("no result field for chunk {chunk}"))
}

fn focus_and_type(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    field: &Entity<TextareaState>,
    text: &str,
) {
    cx.update_window(handle, |_, window, cx| {
        window.focus(&field.focus_handle(cx), cx);
        window.input(text, cx);
    })
    .unwrap();
}

fn in_view<R>(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<MergeView>,
    f: impl FnOnce(&mut MergeView, &mut gpui_kit::Window, &mut gpui_kit::Context<MergeView>) -> R,
) -> R {
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| f(v, window, cx))
    })
    .unwrap()
}

/// How far below the list's top the card of `chunk` is drawn.
fn card_offset(cx: &mut TestAppContext, handle: AnyWindowHandle, chunk: usize) -> f32 {
    cx.update_window(handle, |_, window, _| {
        let card = window.find(("merge-card", chunk)).bounds();
        let list = window.find("merge-list").bounds();
        f32::from(card.origin.y - list.origin.y)
    })
    .unwrap()
}

#[gpui_kit::test]
async fn the_merge_view_reads_in_thai(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let dir = tempfile::tempdir().unwrap();
    let (handle, _workspace, view) = open_merge(cx, conflict_set(dir.path())).await;
    draw(cx, handle);
    // The merge stage.
    for (id, key) in [
        ("merge-stage-input", "Input"),
        ("merge-stage-merge", "Merge result"),
        ("merge-all-left", "Resolve all with left"),
        ("merge-all-right", "Resolve all with right"),
        ("merge-trail", "Ignore trailing whitespace"),
        ("merge-show-result", "Show the whole result"),
        ("merge-copy", "Copy result"),
        ("merge-export", "Export with conflict markers"),
        ("merge-save", "Save result"),
        ("merge-save-as", "Save result to…"),
    ] {
        let drawn = label(cx, handle, id);
        assert_eq!(drawn.as_deref(), Some(tr(key)), "{id}");
        assert_ne!(tr(key), key, "{key} has Thai");
    }
    assert_eq!(
        label(cx, handle, ("merge-count", 1usize)).as_deref(),
        Some(tr("Not chosen yet"))
    );
    let card = view.read_with(cx, |v, _| v.card_text(1)).unwrap();
    assert_eq!(card.state, tr("Not resolved"));
    assert_eq!(tr(card.badge), tr("CONFLICT"));
    assert_eq!(
        card.columns[0].head,
        format!("{} {}", tr("Left"), trf("line {}", &[&2]))
    );
    let parts: Vec<String> = view.read_with(cx, |v, _| {
        v.status_parts().into_iter().map(|p| p.text).collect()
    });
    assert!(
        parts.contains(&trf("Conflicts left: {} / {}", &[&1, &1])),
        "{parts:?}"
    );
    let notice = view.read_with(cx, |v, _| v.notice().map(str::to_owned));
    assert_eq!(
        notice.as_deref(),
        Some(
            trf(
                "Merged {} automatically; {} a choice.",
                &[
                    &format!("0 {}", tr("changes")),
                    &format!("1 {}", tr("conflicts need"))
                ],
            )
            .as_str()
        )
    );
    // The input stage.
    click(cx, handle, "merge-stage-input");
    draw(cx, handle);
    for (id, key) in [
        ("merge-mode-files", "Files"),
        ("merge-mode-paste", "Paste text"),
        ("merge-clear", "Clear all"),
        ("merge-run", "Merge"),
        ("merge-open-left", "Open…"),
    ] {
        let drawn = label(cx, handle, id);
        assert_eq!(drawn.as_deref(), Some(tr(key)), "{id}");
    }
    let output = view
        .read_with(cx, |v, _| v.output_input_for_test())
        .read_with(cx, |i, _| i.presentation().placeholder().to_string());
    assert_eq!(
        output,
        tr("(optional) output file: leave empty to choose when saving")
    );
    let slot = view.read_with(cx, |v, _| v.slot_text(0));
    assert!(
        slot.meta.starts_with(&format!("3 {}", tr("lines"))),
        "{slot:?}"
    );
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

#[gpui_kit::test]
async fn a_binary_slot_message_and_the_typed_names_follow_a_switch(cx: &mut TestAppContext) {
    // English first; each click is the switch, and its frame must already read the other way.
    let dir = tempfile::tempdir().unwrap();
    let (l, b, r) = write_set(
        dir.path(),
        ["left.txt", "base.txt", "right.txt"],
        ["a\0b", "a\nb\n", "a\nc\n"],
    );
    let (handle, _workspace, view) = open_merge(cx, set(l, b, r)).await;
    let message =
        |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.slot_message(0).map(str::to_owned));
    assert_eq!(
        message(cx).as_deref(),
        Some("left.txt is a binary file: only text files can be merged.")
    );
    click(cx, handle, THAI);
    let thai = trf(
        "{} is a binary file: only text files can be merged.",
        &[&"left.txt"],
    );
    assert_eq!(message(cx), Some(thai));
    click(cx, handle, ENGLISH);
    assert_eq!(
        message(cx).as_deref(),
        Some("left.txt is a binary file: only text files can be merged.")
    );

    // Typed text names itself by its slot, said in whatever language is in force; a file that
    // happens to be called "(empty)" is shown as it is.
    let (handle, _workspace, view) = {
        let (l, b, r) = write_set(
            dir.path(),
            ["(empty)", "base.txt", "right.txt"],
            ["a\nX\n", "a\nb\n", "a\nY\n"],
        );
        open_merge(cx, set(l, b, r)).await
    };
    let left = |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.info_parts().remove(0));
    assert!(left(cx).starts_with("Left: (empty) "), "{}", left(cx));
    click(cx, handle, THAI);
    assert!(
        left(cx).starts_with(&format!("{} (empty) ", tr("Left:"))),
        "{}",
        left(cx)
    );
    click(cx, handle, ENGLISH);
    assert!(left(cx).starts_with("Left: (empty) "), "{}", left(cx));
}

#[gpui_kit::test]
async fn open_cards_redraw_on_a_switch_and_keep_their_scroll(cx: &mut TestAppContext) {
    // English first; the click is the switch. No draw follows it: the frame it causes is read.
    let dir = tempfile::tempdir().unwrap();
    let (handle, _workspace, view) = open_merge(cx, long_set(dir.path())).await;
    draw(cx, handle);
    // A typed result in the first conflict, an unsaved choice in the third, then the view walks to
    // the last conflict, which scrolls the list.
    let first = field_of(cx, &view, 1);
    focus_and_type(cx, handle, &first, "mine");
    assert!(in_view(cx, handle, &view, |v, w, cx| v.resolve(
        5,
        How::Right,
        w,
        cx
    )));
    in_view(cx, handle, &view, |v, w, cx| v.go(Nav::Last, w, cx));
    draw(cx, handle);
    let last = view
        .read_with(cx, |v, _| v.session().and_then(|s| s.current()))
        .expect("the walk made a conflict current");
    assert_eq!(last, 9);
    assert!(view.read_with(cx, |v, _| v.card_visible_for_test(last)));
    let place = card_offset(cx, handle, last);
    assert!(view.read_with(cx, |v, _| v.is_dirty()));

    click(cx, handle, THAI);
    assert_eq!(
        label(cx, handle, ("merge-count", last)).as_deref(),
        Some(tr("Not chosen yet")),
        "the card on screen reads Thai on the frame of the switch"
    );
    let card = view.read_with(cx, |v, _| v.card_text(last)).unwrap();
    assert_eq!(card.state, tr("Not resolved"));
    assert_eq!(
        result_placeholder(cx, &view, last),
        tr("Not resolved: press a button above, or type the result here")
    );
    assert_eq!(
        card_offset(cx, handle, last),
        place,
        "the list stays where it was"
    );
    assert!(view.read_with(cx, |v, _| v.card_visible_for_test(last)));
    // What the person made is untouched.
    assert_eq!(
        view.read_with(cx, |v, cx| v.field_text(1, cx)).as_deref(),
        Some("mine")
    );
    assert_eq!(
        view.read_with(cx, |v, _| {
            v.session()
                .and_then(|s| s.resolution(5).map(|r| (r.how, r.lines.clone())))
        }),
        Some((How::Right, vec!["Y51".to_owned()]))
    );
    assert_eq!(
        view.read_with(cx, |v, _| {
            v.session()
                .and_then(|s| s.resolution(1).map(|r| (r.how, r.lines.clone())))
        }),
        Some((How::Manual, vec!["mine".to_owned()]))
    );
    assert!(view.read_with(cx, |v, _| v.is_dirty()), "still unsaved");

    click(cx, handle, ENGLISH);
    assert_eq!(
        label(cx, handle, ("merge-count", last)).as_deref(),
        Some("Not chosen yet")
    );
    assert_eq!(
        result_placeholder(cx, &view, last),
        "Not resolved: press a button above, or type the result here"
    );
    assert_eq!(card_offset(cx, handle, last), place);
    assert_eq!(
        view.read_with(cx, |v, cx| v.field_text(1, cx)).as_deref(),
        Some("mine")
    );
    assert!(view.read_with(cx, |v, _| v.is_dirty()));
}

#[gpui_kit::test]
async fn a_switch_with_a_result_field_focused_keeps_its_keys_and_text(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, workspace, view) = open_merge(cx, conflict_set(dir.path())).await;
    draw(cx, handle);
    let field = field_of(cx, &view, 1);
    // An untouched field with the keys: its placeholder changes under the cursor, and the switch
    // is not an edit.
    cx.update_window(handle, |_, window, cx| {
        window.focus(&field.focus_handle(cx), cx)
    })
    .unwrap();
    let focused = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            field.focus_handle(cx).is_focused(window)
        })
        .unwrap()
    };
    assert!(focused(cx));
    assert!(!view.read_with(cx, |v, _| v.is_dirty()));
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| w.set_lang(i18n::Lang::Th, window, cx))
    })
    .unwrap();
    assert!(focused(cx), "the field keeps the keys");
    assert_eq!(
        placeholder(cx, &field),
        tr("Not resolved: press a button above, or type the result here")
    );
    assert!(
        !view.read_with(cx, |v, _| v.is_dirty()),
        "a switch is not an edit"
    );
    // Typing goes on in it, and it is one edit.
    cx.update_window(handle, |_, window, cx| window.input("mine", cx))
        .unwrap();
    assert_eq!(
        view.read_with(cx, |v, cx| v.field_text(1, cx)).as_deref(),
        Some("mine")
    );
    // A typed result with the keys survives the switch back, and goes on from where it was.
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| w.set_lang(i18n::Lang::En, window, cx))
    })
    .unwrap();
    assert!(focused(cx));
    cx.update_window(handle, |_, window, cx| window.input("!", cx))
        .unwrap();
    assert_eq!(
        view.read_with(cx, |v, cx| v.field_text(1, cx)).as_deref(),
        Some("mine!")
    );
    assert!(view.read_with(cx, |v, _| v.is_dirty()));
    assert_eq!(
        view.read_with(cx, |v, _| {
            v.session()
                .and_then(|s| s.resolution(1).map(|r| (r.how, r.lines.clone())))
        }),
        Some((How::Manual, vec!["mine!".to_owned()]))
    );
}

#[gpui_kit::test]
async fn pasted_text_survives_a_switch(cx: &mut TestAppContext) {
    let (handle, workspace) = open(cx, Launch::Merge(MergePaths::default()));
    let view = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Merge(view)) => view.clone(),
        _ => panic!("expected a merge tab"),
    });
    in_view(cx, handle, &view, |v, w, cx| {
        v.set_mode(InputMode::Paste, w, cx)
    });
    draw(cx, handle);
    let field = view.read_with(cx, |v, _| v.paste_input_for_test(0));
    focus_and_type(cx, handle, &field, "left side");
    assert_eq!(
        view.read_with(cx, |v, _| v.slot_text(0).name),
        "Typed or pasted text"
    );
    click(cx, handle, THAI);
    assert_eq!(
        field.read_with(cx, |f, _| f.value().to_string()),
        "left side"
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.slot_text(0).name),
        tr("Typed or pasted text")
    );
    assert_eq!(
        paste_placeholder(cx, &view, 1),
        tr("Type or paste the common original (base) text here")
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.status_parts()[0].text.clone()),
        tr("Paste mode: type or paste the text of all three sides, then press Merge.")
    );
}
