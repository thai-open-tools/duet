//! Helpers shared by the folder compare and sync UI test binaries (copied from `ui_folder.rs`, which keeps its own).
#![allow(dead_code)]

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use duet_app::folder::model::FolderFilter;
use duet_app::folder::view::FolderView;
use duet_core::folder::run::FolderOptions;
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, Focusable, Point, Render, TestAppContext,
    Window, WindowBounds, WindowOptions, px, size,
};

pub fn open_test_window<V: Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        duet_app::app::configure_theme(gpui_kit::component::ThemeMode::Light, cx);
        cx.bind_keys(duet_app::folder::view::key_bindings());
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1200.), px(800.)),
        };
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            build,
        )
        .expect("open test window")
    })
}

pub fn put(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        .unwrap();
}

pub fn put_at(path: &Path, body: &str, secs: u64) {
    put(path, body);
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

/// left: a.txt (same), b.txt (different), sub/c.txt (left only); right: a.txt, b.txt, new.txt.
pub fn fixture() -> (tempfile::TempDir, tempfile::TempDir) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("a.txt"), "same");
    put(&r.path().join("a.txt"), "same");
    put(&l.path().join("b.txt"), "left");
    put(&r.path().join("b.txt"), "right side");
    put(&l.path().join("sub/c.txt"), "only");
    put(&r.path().join("new.txt"), "new");
    (l, r)
}

/// The modified time most fixture files get (`put`).
pub const T0: u64 = 1_700_000_000;

/// The sync fixture (as duet-core's): eq.txt the same; extra.txt right only; new.txt left
/// only; newer.txt newer on the left; older.txt newer on the right; same-time.txt different
/// contents with the same time.
pub fn sync_fixture() -> (tempfile::TempDir, tempfile::TempDir) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(&l.path().join("eq.txt"), "e", T0);
    put_at(&r.path().join("eq.txt"), "e", T0);
    put_at(&r.path().join("extra.txt"), "x", T0);
    put_at(&l.path().join("new.txt"), "n", T0);
    put_at(&l.path().join("newer.txt"), "L-newer", T0 + 3600);
    put_at(&r.path().join("newer.txt"), "R", T0);
    put_at(&l.path().join("older.txt"), "L", T0);
    put_at(&r.path().join("older.txt"), "R-older", T0 + 3600);
    put_at(&l.path().join("same-time.txt"), "L", T0);
    put_at(&r.path().join("same-time.txt"), "R!", T0);
    (l, r)
}

/// LocalDiff's report pair (`rr:43-56`): `IWT_Config` and `PRD_Config` side by side in one temp
/// folder, which is returned with the two roots. db.properties is newer on the right.
pub fn report_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("IWT_Config"), dir.path().join("PRD_Config"));
    let both = |rel: &str, left: &str, right: &str| {
        put(&a.join(rel), left);
        put(&b.join(rel), right);
    };
    put_at(
        &a.join("app/db.properties"),
        "url=jdbc:x\nuser=svc\npassword=Secr3t-A\ntimeout=30\n",
        T0,
    );
    put_at(
        &b.join("app/db.properties"),
        "url=jdbc:x\nuser=svc\npassword=Secr3t-B\ntimeout=60\n",
        T0 + 5 * 3600,
    );
    both(
        "app/ws.xml",
        "<cfg>\n  <PASS>Zz9-same</PASS>\n  <host>a</host>\n</cfg>\n",
        "<cfg>\n  <PASS>Zz9-same</PASS>\n  <host>b</host>\n</cfg>\n",
    );
    both(
        "bin/run.sh",
        "#!/bin/bash\necho hi\n",
        "#!/bin/bash\r\necho hi\r\n",
    );
    both("app/limit.properties", "max = 10\n", "max =\u{a0}10\n");
    put(&a.join("app/Config.xml"), "<a/>\n");
    put(&b.join("app/config.xml"), "<a/>\n");
    for i in 0..4 {
        put(&a.join(format!("legacy/old{i}.cfg")), &format!("x{i}"));
    }
    put(&a.join("app/extra-iwt.txt"), "e");
    put(&b.join("app/new-prd.txt"), "n");
    both("app/readme.txt", "a\r\nb\r\n", "a\nb\n");
    both("app/same.cfg", "s", "s");
    both(
        "app/x.html",
        "<script>alert(1)</script>\n",
        "<script>alert(2)</script>\n",
    );
    put(&b.join("app/=SUM(1).txt"), "f");
    both("app/ctrl.txt", "a\u{7}b\nk = v\n", "a\u{8}b\nk =\u{a0}v\n");
    (dir, a, b)
}

/// The report pair compared by contents, as LocalDiff compares it.
pub fn open_report_fixture(
    cx: &mut TestAppContext,
    a: &Path,
    b: &Path,
) -> (AnyWindowHandle, Entity<FolderView>) {
    let options = FolderOptions {
        mode: duet_core::folder::run::CompareMode::Content,
        ..FolderOptions::default()
    };
    open_folder_with(cx, a, b, options)
}

pub fn open_folder(
    cx: &mut TestAppContext,
    l: &Path,
    r: &Path,
) -> (AnyWindowHandle, Entity<FolderView>) {
    open_folder_with(cx, l, r, FolderOptions::default())
}

/// Like `open_folder`, with the given compare options.
pub fn open_folder_with(
    cx: &mut TestAppContext,
    l: &Path,
    r: &Path,
    options: FolderOptions,
) -> (AnyWindowHandle, Entity<FolderView>) {
    let (left, right) = (l.to_path_buf(), r.to_path_buf());
    open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| FolderView::new(left, right, options, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        // No test may reach the real Recycle Bin: the default stand-in deletes in place.
        view.update(cx, |v, _| v.set_trash_for_test(remove_in_place));
        view
    })
}

pub fn remove_in_place(path: &Path) -> Result<(), duet_core::folder::actions::ActionError> {
    let gone = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    gone.map_err(|e| duet_core::folder::actions::ActionError::Trash {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

pub async fn wait_idle(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<FolderView>,
) {
    let view = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        let v = view.read(cx);
        v.model().is_some() && !v.is_busy()
    })
    .await;
}

pub fn rels(view: &Entity<FolderView>, cx: &mut TestAppContext) -> Vec<String> {
    view.read_with(cx, |v, _| {
        let m = v.model().unwrap();
        (0..m.visible_len())
            .map(|row| m.node_at(row).rel.clone())
            .collect()
    })
}

pub fn select(view: &Entity<FolderView>, cx: &mut TestAppContext, rel: &str) {
    view.update(cx, |v, cx| {
        v.set_filter(FolderFilter::All, cx);
        let row = {
            let m = v.model().unwrap();
            (0..m.visible_len())
                .find(|&r| m.node_at(r).rel == rel)
                .unwrap()
        };
        v.select_row_for_test(row, cx);
    });
}

/// Waits for a native prompt; the test platform queues it instead of showing a window.
pub async fn wait_for_prompt(cx: &mut TestAppContext, view: &Entity<FolderView>) {
    for _ in 0..2000 {
        let status = view.read_with(cx, |v, _| v.status().to_owned());
        assert!(!status.starts_with("Copy failed"), "{status}");
        if cx.has_pending_prompt() {
            return;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    panic!("no prompt appeared");
}

pub fn press(cx: &mut TestAppContext, handle: AnyWindowHandle, key: &str) {
    cx.update_window(handle, |_, window, cx| window.press(key, cx))
        .unwrap();
}

pub async fn assert_no_prompt_appears(cx: &mut TestAppContext) {
    for _ in 0..30 {
        assert!(!cx.has_pending_prompt(), "a prompt opened");
        cx.executor().timer(Duration::from_millis(10)).await;
    }
}

pub fn menu_outline(view: &Entity<FolderView>, cx: &mut TestAppContext) -> Vec<String> {
    view.read_with(cx, |v, _| {
        duet_app::ui::menu::outline(&duet_app::folder::menu::row_menu(v))
    })
}

/// Runs the item called `label` of the row menu the way the popup would.
pub fn run_menu_item(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<FolderView>,
    label: &str,
) -> bool {
    cx.update_window(handle, |_, window, cx| {
        let entries = duet_app::folder::menu::row_menu(view.read(cx));
        let focus = view.focus_handle(cx);
        duet_app::ui::menu::activate(entries, label, Some(&focus), window, cx)
    })
    .unwrap()
}

/// A right press and release at `position`. The open popup blocks events under it, so tests
/// press clear of it (left of the row centre, or near the list's bottom edge).
pub fn right_press(window: &mut Window, cx: &mut App, position: Point<gpui_kit::Pixels>) {
    for press in [true, false] {
        let event = if press {
            gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                button: gpui_kit::MouseButton::Right,
                position,
                modifiers: gpui_kit::Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            })
        } else {
            gpui_kit::PlatformInput::MouseUp(gpui_kit::MouseUpEvent {
                button: gpui_kit::MouseButton::Right,
                position,
                modifiers: gpui_kit::Modifiers::default(),
                click_count: 1,
            })
        };
        window.dispatch_event(event, cx);
        window.render_frame(cx);
    }
}

pub fn row_left(window: &mut Window, cx: &mut App, row: usize) -> Point<gpui_kit::Pixels> {
    window.render_frame(cx);
    let b = window.find(("folder-row", row)).bounds();
    Point {
        x: b.origin.x + px(40.),
        y: b.center().y,
    }
}

/// Polls `done` (a disk check) while the app runs.
pub async fn wait_until(cx: &mut TestAppContext, handle: AnyWindowHandle, done: impl Fn() -> bool) {
    cx.wait_for(handle, Duration::from_secs(20), move |_, _| done())
        .await;
}

pub fn use_settings(cx: &mut TestAppContext) -> tempfile::TempDir {
    let data = tempfile::tempdir().unwrap();
    let data_dir = data.path().to_path_buf();
    cx.update(|cx| cx.set_global(duet_app::app::AppPaths { data_dir }));
    data
}

pub fn saved_setting(data: &tempfile::TempDir, key: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(data.path().join("settings.json")).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    saved[key].clone()
}
