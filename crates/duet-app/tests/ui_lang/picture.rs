//! The picture tab in Thai: the toolbar, the heads and the status line, and what a switch does
//! to the message a failed side keeps.

use std::time::Duration;

use duet_app::i18n::{self, tr, trf};
use duet_app::launch::Launch;
use duet_app::picture::view::PictureView;
use duet_app::workspace::{TabItem, Workspace};
use duet_core::picture::Picture;
use duet_core::picture::export::encode_png;
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::{open, thai_ui};

fn png(marked: Option<(u32, u32)>) -> Vec<u8> {
    let mut picture = Picture::filled(4, 4, [100, 100, 100, 255]);
    if let Some((x, y)) = marked {
        picture.set_pixel(x, y, [100, 100, 200, 255]);
    }
    encode_png(&picture).unwrap()
}

async fn open_picture(
    cx: &mut TestAppContext,
    dir: &std::path::Path,
    right: Vec<u8>,
) -> (AnyWindowHandle, Entity<Workspace>, Entity<PictureView>) {
    let (a, b) = (dir.join("a.png"), dir.join("b.png"));
    std::fs::write(&a, png(None)).unwrap();
    std::fs::write(&b, right).unwrap();
    let (handle, workspace) = open(cx, Launch::Picture { left: a, right: b });
    let ws = workspace.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        ws.read(cx).active_idle(cx)
    })
    .await;
    let view = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Picture(view)) => view.clone(),
        _ => panic!("expected a picture tab"),
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
async fn the_picture_view_reads_in_thai(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let dir = tempfile::tempdir().unwrap();
    let (handle, _ws, view) = open_picture(cx, dir.path(), png(Some((1, 1)))).await;
    draw(cx, handle);
    for (id, key) in [
        ("picture-mode-side", "Side by side"),
        ("picture-mode-diff", "Difference"),
        ("picture-mode-slider", "Slider"),
        ("picture-mode-blink", "Blink"),
        ("picture-zoom-fit", "Fit"),
        ("picture-prev", "Previous difference group (F7)"),
        ("picture-next", "Next difference group (F8)"),
        ("picture-export", "Export difference image"),
        ("picture-open-left", "Open left…"),
        ("picture-open-right", "Open right…"),
    ] {
        assert_eq!(label(cx, handle, id).as_deref(), Some(tr(key)), "{id}");
        assert_ne!(tr(key), key, "{key} has Thai");
    }
    let tolerance = view.read_with(cx, |v, _| v.tolerance_text());
    assert_eq!(
        tolerance,
        trf("Tolerance {} · {}", &[&0, &tr("Side by side")])
    );
    let status = view.read_with(cx, |v, _| duet_app::status::join_parts(&v.status_parts()));
    assert!(status.contains(tr("pixels differ")), "{status}");
    assert!(!status.contains("differ"), "{status}");
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

#[gpui_kit::test]
async fn a_failed_picture_side_redraws_on_a_switch(cx: &mut TestAppContext) {
    // English first: the right file is not a picture. The click is the switch.
    let dir = tempfile::tempdir().unwrap();
    let broken = b"\x89PNG\r\n\x1a\nthis is not a png".to_vec();
    let (handle, _ws, view) = open_picture(cx, dir.path(), broken).await;
    draw(cx, handle);
    let english = view
        .read_with(cx, |v, _| v.message(false).map(str::to_owned))
        .expect("the right side failed");
    assert!(english.starts_with("Can't open b.png: "), "{english}");
    assert_eq!(
        label(cx, handle, "picture-right-placeholder").as_deref(),
        Some(english.as_str())
    );

    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 0usize), cx)
    })
    .unwrap();
    // No draw: the frame the click caused already reads Thai.
    let thai = view
        .read_with(cx, |v, _| v.message(false).map(str::to_owned))
        .expect("the message stays");
    assert!(thai.starts_with("เปิด b.png ไม่ได้: "), "{thai}");
    assert_eq!(
        label(cx, handle, "picture-right-placeholder").as_deref(),
        Some(thai.as_str())
    );

    cx.update_window(handle, |_, window, cx| {
        window.click(("header-lang", 1usize), cx)
    })
    .unwrap();
    let back = view.read_with(cx, |v, _| v.message(false).map(str::to_owned));
    assert_eq!(back.as_deref(), Some(english.as_str()));
}
