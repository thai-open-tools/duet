//! The text view's chrome in Thai: toolbars, messages and what a switch does to them.

use std::time::Duration;

use duet_app::i18n::{self, tr, trf};
use duet_app::launch::Launch;
use duet_app::text::view::TextView;
use duet_app::workspace::{TabItem, Workspace};
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::{open, thai_ui};

pub(super) fn text_of(workspace: &Entity<Workspace>, cx: &mut TestAppContext) -> Entity<TextView> {
    workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Text(view)) => view.clone(),
        _ => panic!("expected a text tab"),
    })
}

pub(super) async fn wait_loaded(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
) {
    let view = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        let v = view.read(cx);
        !v.is_busy() && (v.model().is_some() || v.message().is_some())
    })
    .await;
}

pub(super) fn open_text(
    cx: &mut TestAppContext,
    left: &str,
    right: &str,
) -> (
    tempfile::TempDir,
    AnyWindowHandle,
    Entity<Workspace>,
    Entity<TextView>,
) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, left).unwrap();
    std::fs::write(&b, right).unwrap();
    let (handle, workspace) = open(cx, Launch::Files { left: a, right: b });
    let view = text_of(&workspace, cx);
    (dir, handle, workspace, view)
}

fn label(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> Option<String> {
    cx.update_window(handle, |_, window, _| {
        window.find(id).label().map(str::to_owned)
    })
    .unwrap()
}

fn click(cx: &mut TestAppContext, handle: AnyWindowHandle, id: (&'static str, usize)) {
    cx.update_window(handle, |_, window, cx| window.click(id, cx))
        .unwrap();
}

#[gpui_kit::test]
async fn the_text_view_reads_in_thai(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let (_dir, handle, _workspace, view) = open_text(cx, "a\nsame\n", "b\nsame\n");
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    for (id, key) in [
        ("text-reload", "Reload (F5)"),
        ("text-prev", "Previous difference (F7)"),
        ("text-next", "Next difference (F8)"),
        ("text-undo", "Undo (Ctrl+Z)"),
        ("text-redo", "Redo (Ctrl+Y)"),
        ("text-save-left", "Save left"),
        ("text-save-right", "Save right"),
        ("text-rules-open", "Rules…"),
        ("text-ignore-case", "Case"),
        ("text-ignore-eol", "Line endings"),
        ("text-moves", "Moves"),
        ("text-syntax", "Syntax"),
        ("text-wrap", "Wrap"),
        ("text-details", "Details"),
        ("text-report", "Report"),
        ("text-ignore-ws", "Whitespace: compare"),
        ("text-save-options", "On save: keep"),
        ("text-filter-diffs", "Differences"),
        ("text-filter-same", "Same"),
    ] {
        let drawn = label(cx, handle, id);
        assert_eq!(drawn.as_deref(), Some(tr(key)), "{id}");
        assert_ne!(tr(key), key, "{key} has Thai");
    }
    assert_eq!(
        label(cx, handle, "text-language").as_deref(),
        Some(trf("Type: {}", &[&tr("Auto")]).as_str())
    );
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

#[gpui_kit::test]
async fn a_binary_message_redraws_on_a_switch(cx: &mut TestAppContext) {
    // English first; the click is the switch, and its frame must already read Thai.
    let (_dir, handle, _workspace, view) = open_text(cx, "a\0b", "a\0c");
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let message = view.read_with(cx, |v, _| v.message().map(str::to_owned));
    assert_eq!(message.as_deref(), Some("Binary files differ."));
    click(cx, handle, ("header-lang", 0));
    let (title, hint) = view.read_with(cx, |v, _| {
        (
            v.message().map(str::to_owned),
            v.message_hint().map(str::to_owned),
        )
    });
    assert_eq!(title.as_deref(), Some(tr("Binary files differ.")));
    assert_eq!(
        hint.as_deref(),
        Some(tr(
            "These files are not text. Open a text pair, or press F5 to compare again."
        ))
    );
    // The hint is still the binary one, so the other tabs are still offered.
    let drawn = label(cx, handle, "text-open-hex");
    assert_eq!(drawn.as_deref(), Some(tr("Open in hex")));
    click(cx, handle, ("header-lang", 1));
    let title = view.read_with(cx, |v, _| v.message().map(str::to_owned));
    assert_eq!(title.as_deref(), Some("Binary files differ."));
}
