//! The sweep (task 14): every kind of tab opens and draws in Thai without one missing key, and so
//! does every Help topic, whose chips are drawn from the same table.

use std::path::Path;
use std::time::Duration;

use duet_app::help::HelpView;
use duet_app::help::topics::TOPICS;
use duet_app::i18n;
use duet_app::launch::{Launch, MergePaths};
use duet_app::workspace::TabItem;
use duet_core::picture::Picture;
use duet_core::picture::export::encode_png;
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{AppContext as _, Entity, TestAppContext};

use super::{open, thai_ui};

fn png(marked: bool) -> Vec<u8> {
    let mut picture = Picture::filled(4, 4, [100, 100, 100, 255]);
    if marked {
        picture.set_pixel(1, 1, [100, 100, 200, 255]);
    }
    encode_png(&picture).unwrap()
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// One launch of every kind, over files in `dir`.
fn launches(dir: &Path) -> Vec<(&'static str, Launch)> {
    let (left_dir, right_dir) = (dir.join("left"), dir.join("right"));
    std::fs::create_dir_all(left_dir.join("sub")).unwrap();
    std::fs::create_dir_all(right_dir.join("sub")).unwrap();
    write(&left_dir, "same.txt", b"one\n");
    write(&right_dir, "same.txt", b"one\n");
    write(&left_dir, "changed.txt", b"one\ntwo\n");
    write(&right_dir, "changed.txt", b"one\n2\n");
    write(&left_dir, "only-left.txt", b"x\n");
    write(&right_dir.join("sub"), "only-right.txt", b"y\n");
    let merge = |name: &str, text: &[u8]| write(dir, name, text);
    vec![
        ("start", Launch::Start),
        (
            "folders",
            Launch::Folders {
                left: left_dir,
                right: right_dir,
            },
        ),
        (
            "text",
            Launch::Files {
                left: write(dir, "a.txt", b"alpha\nbeta\n"),
                right: write(dir, "b.txt", b"alpha\ngamma\n"),
            },
        ),
        (
            "hex",
            Launch::Hex {
                left: write(dir, "a.bin", &[0, 1, 2, 3, 4, 5, 6, 7]),
                right: write(dir, "b.bin", &[0, 1, 2, 9, 4, 5, 6, 7]),
            },
        ),
        (
            "table",
            Launch::Table {
                left: write(dir, "a.csv", b"id,name\n1,ann\n2,bob\n"),
                right: write(dir, "b.csv", b"id,name\n1,ann\n2,rob\n"),
            },
        ),
        (
            "picture",
            Launch::Picture {
                left: write(dir, "a.png", &png(false)),
                right: write(dir, "b.png", &png(true)),
            },
        ),
        (
            "merge",
            Launch::Merge(MergePaths {
                left: Some(merge("left.txt", b"a\nX\nc\n")),
                base: Some(merge("base.txt", b"a\nb\nc\n")),
                right: Some(merge("right.txt", b"a\nY\nc\n")),
                output: None,
            }),
        ),
    ]
}

#[gpui_kit::test]
async fn every_view_opens_in_thai_with_no_missing_key(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let dir = tempfile::tempdir().unwrap();
    for (name, launch) in launches(dir.path()) {
        let (handle, workspace) = open(cx, launch);
        let ws = workspace.clone();
        cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
            ws.read(cx).active_idle(cx)
        })
        .await;
        for _ in 0..2 {
            cx.update_window(handle, |_, window, cx| window.render_frame(cx))
                .unwrap();
        }
        assert_eq!(
            i18n::missing_keys_for_test(),
            Vec::<&str>::new(),
            "the {name} view drew a label with no Thai entry"
        );
    }
}

#[gpui_kit::test]
async fn every_help_topic_draws_its_chips_in_thai_with_no_missing_key(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let (handle, workspace) = open(cx, Launch::Start);
    cx.update_window(handle, |_, window, cx| window.click("header-help", cx))
        .unwrap();
    let view: Entity<HelpView> = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Help(view)) => view.clone(),
        _ => panic!("expected the help tab"),
    });
    for topic in &TOPICS {
        view.update(cx, |v, cx| assert!(v.go_to(topic.id, cx), "{}", topic.id));
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
    }
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}
