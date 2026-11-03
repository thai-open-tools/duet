//! Helpers shared by the text view's UI test binaries (copied from `ui_text.rs`, which keeps its own).
#![allow(dead_code)]

use std::path::Path;
use std::time::Duration;

use duet_app::text::model::status_text;
use duet_app::text::syntax::SyntaxState;
use duet_app::text::view::TextView;
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, Focusable, InputEvent as _, Pixels, Point,
    Render, ScrollDelta, ScrollWheelEvent, TestAppContext, Window, WindowBounds, WindowOptions,
    point, px, size,
};

pub fn open_test_window<V: Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        duet_app::app::configure_theme(gpui_kit::component::ThemeMode::Light, cx);
        cx.bind_keys(duet_app::text::view::key_bindings());
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

pub fn open_text(
    cx: &mut TestAppContext,
    left: &str,
    right: &str,
) -> (tempfile::TempDir, AnyWindowHandle, Entity<TextView>) {
    open_text_bytes(cx, left.as_bytes(), right.as_bytes())
}

pub fn open_text_bytes(
    cx: &mut TestAppContext,
    left: &[u8],
    right: &[u8],
) -> (tempfile::TempDir, AnyWindowHandle, Entity<TextView>) {
    open_files(cx, (Path::new("a.txt"), left), (Path::new("b.txt"), right))
}

/// `open_text` with a chosen file name, the same on both sides, in two folders.
pub fn open_named(
    cx: &mut TestAppContext,
    name: &str,
    left: &str,
    right: &str,
) -> (tempfile::TempDir, AnyWindowHandle, Entity<TextView>) {
    let (a, b) = (Path::new("left").join(name), Path::new("right").join(name));
    open_files(cx, (&a, left.as_bytes()), (&b, right.as_bytes()))
}

/// Writes both files under a new temp folder (paths relative to it) and opens them.
pub fn open_files(
    cx: &mut TestAppContext,
    (left_path, left): (&Path, &[u8]),
    (right_path, right): (&Path, &[u8]),
) -> (tempfile::TempDir, AnyWindowHandle, Entity<TextView>) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join(left_path), dir.path().join(right_path));
    for (path, bytes) in [(&a, left), (&b, right)] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let (handle, view) = open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| TextView::new(a, b, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    (dir, handle, view)
}

pub async fn wait_loaded(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
) {
    let view = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !view.read(cx).is_busy()
    })
    .await;
}

/// Waits for the find bar's search to finish and returns its status.
pub async fn wait_found(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
) -> String {
    let v = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        v.read(cx).find_status().is_some_and(|s| s != "Searching…")
    })
    .await;
    view.read_with(cx, |v, _| v.find_status().unwrap())
}

pub fn press(cx: &mut TestAppContext, handle: AnyWindowHandle, key: &str) {
    cx.update_window(handle, |_, window, cx| window.press(key, cx))
        .unwrap();
}

pub fn top_row(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<TextView>) -> usize {
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    view.read_with(cx, |v, _| v.top_row())
}

/// Sends one pointer event to the window, then draws a frame, as the harness's own `drag` does.
pub fn pointer(window: &mut Window, event: gpui_kit::PlatformInput, cx: &mut App) {
    window.dispatch_event(event, cx);
    window.render_frame(cx);
}

pub fn move_to(position: Point<Pixels>, held: bool) -> gpui_kit::PlatformInput {
    gpui_kit::PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
        position,
        pressed_button: held.then_some(gpui_kit::MouseButton::Left),
        modifiers: Default::default(),
    })
}

pub fn button(position: Point<Pixels>, down: bool) -> gpui_kit::PlatformInput {
    let button = gpui_kit::MouseButton::Left;
    if down {
        gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
            button,
            position,
            modifiers: Default::default(),
            click_count: 1,
            first_mouse: false,
        })
    } else {
        gpui_kit::PlatformInput::MouseUp(gpui_kit::MouseUpEvent {
            button,
            position,
            modifiers: Default::default(),
            click_count: 1,
        })
    }
}

pub fn right_button(position: Point<Pixels>, down: bool) -> gpui_kit::PlatformInput {
    use gpui_kit::{MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput};
    if down {
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Right,
            position,
            modifiers: Default::default(),
            click_count: 1,
            first_mouse: false,
        })
    } else {
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Right,
            position,
            modifiers: Default::default(),
            click_count: 1,
        })
    }
}

pub fn right_click_row(cx: &mut TestAppContext, handle: AnyWindowHandle, display: usize) {
    cx.update_window(handle, |_, window, cx| {
        window.right_click(("text-row", display), cx)
    })
    .unwrap();
    cx.run_until_parked();
}

/// Runs the menu item `label` the way the popup does (pre-flight ruling P1).
pub fn run_item(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
    label: &str,
) -> bool {
    let entries = view.read_with(cx, |v, _| duet_app::text::menu::row_menu(v));
    let view = view.clone();
    cx.update_window(handle, |_, window, cx| {
        let focus = view.focus_handle(cx);
        duet_app::ui::menu::activate(entries, label, Some(&focus), window, cx)
    })
    .unwrap()
}

pub fn row_items(cx: &mut TestAppContext, view: &Entity<TextView>) -> Vec<String> {
    view.read_with(cx, |v, _| {
        duet_app::ui::menu::outline(&duet_app::text::menu::row_menu(v))
    })
}

pub fn left_text(cx: &mut TestAppContext, view: &Entity<TextView>) -> String {
    view.read_with(cx, |v, _| {
        v.model().unwrap().buffer(true).unwrap().text().to_owned()
    })
}

pub fn right_text(cx: &mut TestAppContext, view: &Entity<TextView>) -> String {
    view.read_with(cx, |v, _| {
        v.model().unwrap().buffer(false).unwrap().text().to_owned()
    })
}

pub fn notice(cx: &mut TestAppContext, view: &Entity<TextView>) -> Option<String> {
    view.read_with(cx, |v, _| v.notice().map(str::to_owned))
}

/// One `render_frame`.
pub fn render(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

/// A pixel wheel notch of `dy_px` (negative scrolls down) at `position`.
pub fn wheel(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    position: Point<Pixels>,
    dy_px: f32,
) {
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(
            ScrollWheelEvent {
                position,
                delta: ScrollDelta::Pixels(point(px(0.), px(dy_px))),
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

/// 300 lines "line {i}" on both sides; lines 40, 150 and 250 differ on the right ("LINE {i}").
pub fn pair_300() -> (String, String) {
    let left: String = (0..300).map(|i| format!("line {i}\n")).collect();
    let right: String = (0..300)
        .map(|i| match i {
            40 | 150 | 250 => format!("LINE {i}\n"),
            _ => format!("line {i}\n"),
        })
        .collect();
    (left, right)
}

/// Sets a file's modified time, in seconds after the Unix epoch.
pub fn touch(path: &Path, secs: u64) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

pub async fn wait_clean(cx: &mut TestAppContext, view: &Entity<TextView>) {
    for _ in 0..2000 {
        if !view.read_with(cx, |v, _| v.is_dirty()) {
            return;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    panic!("never saved");
}

pub async fn wait_syntax(cx: &mut TestAppContext, view: &Entity<TextView>, left: bool) {
    for _ in 0..2000 {
        if view.read_with(cx, |v, _| v.syntax_state(left)) == SyntaxState::Ready {
            return;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    panic!("syntax never became ready");
}

pub fn selected_rows(
    cx: &mut TestAppContext,
    view: &Entity<TextView>,
) -> Option<std::ops::Range<u32>> {
    view.read_with(cx, |v, _| v.row_selection().map(|s| s.rows()))
}

/// Points the settings file at a new temp folder; keep the returned folder alive.
pub fn use_settings(cx: &mut TestAppContext) -> tempfile::TempDir {
    let data = tempfile::tempdir().unwrap();
    let data_dir = data.path().to_path_buf();
    cx.update(|cx| cx.set_global(duet_app::app::AppPaths { data_dir }));
    data
}

/// One key of the saved settings file.
pub fn saved_setting(data: &tempfile::TempDir, key: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(data.path().join("settings.json")).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    saved[key].clone()
}

/// `use_settings`, then `open_text`: the settings folder first, so the tab reads it.
pub fn open_text_with_settings(
    cx: &mut TestAppContext,
    left: &str,
    right: &str,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    AnyWindowHandle,
    Entity<TextView>,
) {
    let data = use_settings(cx);
    let (dir, handle, view) = open_text(cx, left, right);
    (data, dir, handle, view)
}

// Shared by the P9b binaries (split from `ui_text_rules.rs`).

pub fn blocks(cx: &mut TestAppContext, view: &Entity<TextView>) -> usize {
    view.read_with(cx, |v, _| v.model().unwrap().block_count())
}

/// The compare's status, without any notice over it.
pub fn status(cx: &mut TestAppContext, view: &Entity<TextView>) -> String {
    view.read_with(cx, |v, _| status_text(v.model().unwrap()))
}

pub fn current_block(cx: &mut TestAppContext, view: &Entity<TextView>) -> Option<usize> {
    view.read_with(cx, |v, _| v.model().unwrap().current_block())
}

pub fn focus_view(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<TextView>) {
    cx.update_window(handle, |_, window, cx| {
        window.focus(&view.focus_handle(cx), cx)
    })
    .unwrap();
}

pub async fn wait_notice(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<TextView>,
    want: &str,
) {
    let (v, want) = (view.clone(), want.to_owned());
    cx.wait_for(handle, std::time::Duration::from_secs(20), move |_, cx| {
        v.read(cx).notice() == Some(want.as_str())
    })
    .await;
}

pub fn click(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) {
    cx.update_window(handle, |_, window, cx| window.click(id, cx))
        .unwrap();
}

pub fn left_text_opt(cx: &mut TestAppContext, view: &Entity<TextView>) -> Option<String> {
    view.read_with(cx, |v, _| {
        v.model().unwrap().buffer(true).map(|b| b.text().to_owned())
    })
}

pub fn right_text_opt(cx: &mut TestAppContext, view: &Entity<TextView>) -> Option<String> {
    view.read_with(cx, |v, _| {
        v.model()
            .unwrap()
            .buffer(false)
            .map(|b| b.text().to_owned())
    })
}

pub async fn wait_for_prompt(cx: &mut TestAppContext) {
    for _ in 0..2000 {
        if cx.has_pending_prompt() {
            return;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    panic!("no prompt appeared");
}

/// Waits until the notice changes from `before`, and returns it.
pub async fn wait_new_notice(
    cx: &mut TestAppContext,
    view: &Entity<TextView>,
    before: Option<String>,
) -> String {
    for _ in 0..2000 {
        let now = notice(cx, view);
        if let Some(now) = now.filter(|now| Some(now) != before.as_ref()) {
            return now;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    panic!("no notice appeared");
}
