//! The hex tab in Thai: the toolbar, the status line and the body, and what a switch does to the
//! message a failed open keeps.

use std::time::Duration;

use duet_app::hex::view::HexView;
use duet_app::i18n::{self, tr, trf};
use duet_app::launch::Launch;
use duet_app::workspace::{TabItem, Workspace};
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::{open, thai_ui};

async fn open_hex(
    cx: &mut TestAppContext,
    dir: &std::path::Path,
    left: &[u8],
    right: Option<&[u8]>,
) -> (AnyWindowHandle, Entity<Workspace>, Entity<HexView>) {
    let (a, b) = (dir.join("a.bin"), dir.join("b.bin"));
    std::fs::write(&a, left).unwrap();
    if let Some(right) = right {
        std::fs::write(&b, right).unwrap();
    }
    let (handle, workspace) = open(cx, Launch::Hex { left: a, right: b });
    let ws = workspace.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        ws.read(cx).active_idle(cx)
    })
    .await;
    let view = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Hex(view)) => view.clone(),
        _ => panic!("expected a hex tab"),
    });
    (handle, workspace, view)
}

fn label(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> Option<String> {
    cx.update_window(handle, |_, window, _| {
        window
            .try_find(id)
            .and_then(|e| e.label().map(str::to_owned))
    })
    .unwrap()
}

fn draw(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

#[gpui_kit::test]
async fn the_hex_view_reads_in_thai(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let dir = tempfile::tempdir().unwrap();
    let (handle, _ws, view) = open_hex(cx, dir.path(), &[0; 64], Some(&[1; 64])).await;
    draw(cx, handle);
    for (id, key) in [
        ("hex-first", "First difference (Ctrl+Home)"),
        ("hex-prev", "Previous difference (F7)"),
        ("hex-next", "Next difference (F8)"),
        ("hex-last", "Last difference (Ctrl+End)"),
        ("hex-swap", "Swap sides (F9)"),
        ("hex-reload", "Reload (F5)"),
        ("hex-filter-all", "All"),
        ("hex-filter-diff", "Differences only"),
        ("hex-goto-go", "Go to"),
        ("hex-find", "Find…"),
        ("hex-copy", "Copy hex"),
    ] {
        assert_eq!(label(cx, handle, id).as_deref(), Some(tr(key)), "{id}");
        assert_ne!(tr(key), key, "{key} has Thai");
    }
    assert_eq!(
        label(cx, handle, "hex-bpr").as_deref(),
        Some(trf("Bytes per row: {}", &[&16]).as_str())
    );
    let status = view.read_with(cx, |v, _| v.status_text());
    assert!(status.contains(&trf("Left: {}", &[&"64 B"])), "{status}");
    assert!(!status.contains("Different bytes"), "{status}");
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

#[gpui_kit::test]
async fn a_failed_hex_open_redraws_on_a_switch(cx: &mut TestAppContext) {
    // English first: the right file does not exist. The click is the switch.
    let dir = tempfile::tempdir().unwrap();
    let (handle, _ws, view) = open_hex(cx, dir.path(), &[0; 8], None).await;
    draw(cx, handle);
    let hint = view.read_with(cx, |v, _| v.message_hint().map(str::to_owned));
    assert_eq!(hint.as_deref(), Some("Press F5 to try again."));
    let english = view
        .read_with(cx, |v, _| v.message().map(str::to_owned))
        .expect("the open failed");
    assert!(english.starts_with("Can't open "), "{english}");

    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 0usize), cx)
    })
    .unwrap();
    // No draw: the frame the click caused already reads Thai.
    let thai = view
        .read_with(cx, |v, _| v.message().map(str::to_owned))
        .expect("the message stays");
    assert!(thai.starts_with("เปิด "), "{thai}");
    assert_eq!(
        view.read_with(cx, |v, _| v.message_hint().map(str::to_owned))
            .as_deref(),
        Some(tr("Press F5 to try again."))
    );

    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 1usize), cx)
    })
    .unwrap();
    let back = view.read_with(cx, |v, _| v.message().map(str::to_owned));
    assert_eq!(back.as_deref(), Some(english.as_str()));
}
