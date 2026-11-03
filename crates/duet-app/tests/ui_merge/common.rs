//! Helpers shared by every area of the merge tests.

pub use std::path::{Path, PathBuf};
pub use std::time::Duration;

pub use duet_app::format::human_size;
pub use duet_app::launch::{Launch, MergePaths};
pub use duet_app::merge::items::Item;
pub use duet_app::merge::menu::card_menu;
pub use duet_app::merge::view::{
    BASE, CANCELLED, CardText, ColumnText, InputMode, LEFT, MergeEvent, MergeView, PREVIEW_DELAY,
    RIGHT, RowText, SlotText, Stage,
};
pub use duet_app::pathbar::FOLDER_DROP_NOTICE;
pub use duet_app::status::StatusPart;
pub use duet_app::workspace::{TabItem, Workspace};
pub use duet_core::text::merge::session::How;
pub use gpui_kit::component::input::{InputState, TextareaState};
pub use gpui_kit::test::{TestAppContextExt, TestWindowExt};
pub use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Context, ElementId, Entity, Focusable, Point, Render,
    ScrollDelta, TestAppContext, Window, WindowBounds, WindowOptions, point, px, size,
};

pub fn open_test_window<V: Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        duet_app::app::configure_theme(gpui_kit::component::ThemeMode::Light, cx);
        cx.bind_keys(duet_app::app::key_bindings());
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

/// left.txt, base.txt and right.txt in a temporary folder.
pub fn write_set(
    left: &str,
    base: &str,
    right: &str,
) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        let path = dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        path
    };
    let (l, b, r) = (
        write("left.txt", left),
        write("base.txt", base),
        write("right.txt", right),
    );
    (dir, l, b, r)
}

pub fn paths(l: &Path, b: &Path, r: &Path) -> MergePaths {
    MergePaths {
        left: Some(l.to_path_buf()),
        base: Some(b.to_path_buf()),
        right: Some(r.to_path_buf()),
        output: None,
    }
}

pub async fn open_workspace(
    cx: &mut TestAppContext,
    paths: MergePaths,
) -> (AnyWindowHandle, Entity<Workspace>, Entity<MergeView>) {
    let (handle, workspace) = open_test_window(cx, move |window, cx| {
        cx.new(|cx| Workspace::new(Launch::Merge(paths), window, cx))
    });
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

/// A view with no files, focused, in a bare window.
pub fn open_view(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<MergeView>) {
    open_test_window(cx, |window, cx| {
        let view = cx.new(|cx| MergeView::new(MergePaths::default(), window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    })
}

pub async fn wait_idle(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<MergeView>) {
    let view = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !view.read(cx).is_busy()
    })
    .await;
}

pub fn with_data_dir(cx: &mut TestAppContext, dir: &std::path::Path) {
    let data_dir = dir.to_path_buf();
    cx.update(|cx| cx.set_global(duet_app::app::AppPaths { data_dir }));
}

pub fn texts(parts: &[StatusPart]) -> Vec<String> {
    parts.iter().map(|p| p.text.clone()).collect()
}

/// Runs `f` on the view inside its window.
pub fn in_view<R>(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<MergeView>,
    f: impl FnOnce(&mut MergeView, &mut Window, &mut Context<MergeView>) -> R,
) -> R {
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| f(v, window, cx))
    })
    .unwrap()
}

pub fn drop_files(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<MergeView>,
    paths: &[PathBuf],
    target: Option<usize>,
) {
    in_view(cx, handle, view, |v, window, cx| {
        v.drop_paths(paths, target, window, cx)
    });
}

pub fn set_mode(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<MergeView>,
    mode: InputMode,
) {
    in_view(cx, handle, view, |v, window, cx| {
        v.set_mode(mode, window, cx)
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

pub fn slot_text(cx: &mut TestAppContext, view: &Entity<MergeView>, slot: usize) -> SlotText {
    view.read_with(cx, |v, _| v.slot_text(slot))
}

pub fn field(
    cx: &mut TestAppContext,
    view: &Entity<MergeView>,
    slot: usize,
) -> Entity<TextareaState> {
    view.read_with(cx, |v, _| v.paste_input_for_test(slot))
}

pub fn field_text(cx: &mut TestAppContext, view: &Entity<MergeView>, slot: usize) -> String {
    let field = field(cx, view, slot);
    field.read_with(cx, |f, _| f.value().to_string())
}

/// Gives a paste field the keys (the view must be drawn in paste mode).
pub fn focus_field(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<MergeView>,
    slot: usize,
) {
    let field = field(cx, view, slot);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.focus(&field.focus_handle(cx), cx);
    })
    .unwrap();
}

/// Types lines into the focused field: the harness types one character at a time.
pub fn type_lines(cx: &mut TestAppContext, handle: AnyWindowHandle, lines: &[&str]) {
    cx.update_window(handle, |_, window, cx| {
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                window.press("enter", cx);
            }
            window.input(line, cx);
        }
    })
    .unwrap();
}

pub fn view_has_the_keys(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<MergeView>,
) -> bool {
    let focus = view.read_with(cx, |v, cx| v.focus_handle(cx));
    cx.update_window(handle, |_, window, _| focus.is_focused(window))
        .unwrap()
}

pub fn owned(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|l| (*l).to_owned()).collect()
}

/// "1" to "n".
pub fn numbered(n: usize) -> Vec<String> {
    (1..=n).map(|k| k.to_string()).collect()
}

/// A focused view merging these three texts (typed, LF), drawn once the merge landed.
pub async fn open_texts(
    cx: &mut TestAppContext,
    left: &[String],
    base: &[String],
    right: &[String],
) -> (AnyWindowHandle, Entity<MergeView>) {
    use duet_core::text::merge::MergeText;
    let texts = [left, base, right]
        .map(|lines| MergeText::typed(lines.iter().map(|l| format!("{l}\n")).collect()));
    let (handle, view) = open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| MergeView::with_texts_for_test(texts, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    wait_idle(cx, handle, &view).await;
    draw(cx, handle);
    (handle, view)
}

pub fn draw(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

pub fn press(cx: &mut TestAppContext, handle: AnyWindowHandle, key: &str) {
    cx.update_window(handle, |_, window, cx| window.press(key, cx))
        .unwrap();
}

pub fn click(cx: &mut TestAppContext, handle: AnyWindowHandle, id: impl Into<ElementId>) {
    let id = id.into();
    cx.update_window(handle, |_, window, cx| window.click(id, cx))
        .unwrap();
}

pub fn drawn(cx: &mut TestAppContext, handle: AnyWindowHandle, id: impl Into<ElementId>) -> bool {
    let id = id.into();
    cx.update_window(handle, |_, window, _| window.try_find(id).is_some())
        .unwrap()
}

/// What an element reads (its accessible name).
pub fn label(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    id: impl Into<ElementId>,
) -> Option<String> {
    let id = id.into();
    cx.update_window(handle, |_, window, _| {
        window
            .try_find(id)
            .and_then(|e| e.label().map(str::to_owned))
    })
    .unwrap()
}

pub fn bounds_of(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    id: impl Into<ElementId>,
) -> Bounds<gpui_kit::Pixels> {
    let id = id.into();
    cx.update_window(handle, |_, window, _| window.find(id).bounds())
        .unwrap()
}

/// How far below the list's top the card of `chunk` is drawn (the notice line above the list
/// comes and goes).
pub fn card_offset(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    chunk: usize,
) -> gpui_kit::Pixels {
    bounds_of(cx, handle, ("merge-card", chunk)).origin.y
        - bounds_of(cx, handle, "merge-list").origin.y
}

/// An enabled control has a focus binding; a disabled one is no tab stop and has none.
pub fn enabled(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> bool {
    cx.update_window(handle, |_, window, _| window.find(id).focused().is_some())
        .unwrap()
}

pub fn position(cx: &mut TestAppContext, handle: AnyWindowHandle) -> String {
    label(cx, handle, "merge-pos").expect("the position label")
}

pub fn card(cx: &mut TestAppContext, view: &Entity<MergeView>, chunk: usize) -> CardText {
    view.read_with(cx, |v, _| v.card_text(chunk))
        .unwrap_or_else(|| panic!("no card for chunk {chunk}"))
}

pub fn current(cx: &mut TestAppContext, view: &Entity<MergeView>) -> Option<usize> {
    view.read_with(cx, |v, _| v.session().and_then(|s| s.current()))
}

pub fn notice_of(cx: &mut TestAppContext, view: &Entity<MergeView>) -> Option<String> {
    view.read_with(cx, |v, _| v.notice().map(str::to_owned))
}

pub fn row(number: u32, text: &str, changed: bool, marks: Vec<std::ops::Range<usize>>) -> RowText {
    RowText {
        number,
        text: text.to_owned(),
        changed,
        marks,
    }
}

/// Task 3's set: one conflict, chunk 1 (X / b / Y).
pub fn one_conflict() -> [Vec<String>; 3] {
    [
        owned(&["a", "X", "c"]),
        owned(&["a", "b", "c"]),
        owned(&["a", "Y", "c"]),
    ]
}

/// F2 of Task 2: conflicts at chunks 1 (X1 / b / Y1) and 3 (X2 / d / Y2).
pub fn f2() -> [Vec<String>; 3] {
    [
        owned(&["a", "X1", "c", "X2", "e"]),
        owned(&["a", "b", "c", "d", "e"]),
        owned(&["a", "Y1", "c", "Y2", "e"]),
    ]
}

/// Thirty conflicts, each after one stable line: the list is taller than the window.
pub fn thirty_conflicts() -> [Vec<String>; 3] {
    let (mut left, mut base, mut right) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..30 {
        for side in [&mut left, &mut base, &mut right] {
            side.push(format!("same {i}"));
        }
        left.push(format!("left {i}"));
        base.push(format!("base {i}"));
        right.push(format!("right {i}"));
    }
    [left, base, right]
}

pub fn result_field(
    cx: &mut TestAppContext,
    view: &Entity<MergeView>,
    chunk: usize,
) -> Entity<TextareaState> {
    view.read_with(cx, |v, _| v.field_for_test(chunk))
        .unwrap_or_else(|| panic!("no result field for chunk {chunk}"))
}

/// The text in the result field of `chunk`.
pub fn result_text(
    cx: &mut TestAppContext,
    view: &Entity<MergeView>,
    chunk: usize,
) -> Option<String> {
    view.read_with(cx, |v, cx| v.field_text(chunk, cx))
}

/// Gives the result field of `chunk` the keys, then draws, so its focus is seen.
pub fn focus_result(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<MergeView>,
    chunk: usize,
) {
    let field = result_field(cx, view, chunk);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.focus(&field.focus_handle(cx), cx);
        window.render_frame(cx);
    })
    .unwrap();
}

pub fn field_has_the_keys(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    field: &Entity<TextareaState>,
) -> bool {
    let focus = field.read_with(cx, |f, cx| f.focus_handle(cx));
    cx.update_window(handle, |_, window, _| focus.is_focused(window))
        .unwrap()
}

/// Makes the window the active one. A headless window starts inactive, and gpui sends focus
/// and blur events only for an active window.
pub fn activate(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    assert!(
        cx.update_window(handle, |_, window, _| window.is_window_active())
            .unwrap()
    );
}

pub fn type_text(cx: &mut TestAppContext, handle: AnyWindowHandle, text: &str) {
    cx.update_window(handle, |_, window, cx| window.input(text, cx))
        .unwrap();
}

pub fn how_of(cx: &mut TestAppContext, view: &Entity<MergeView>, chunk: usize) -> Option<How> {
    view.read_with(cx, |v, _| {
        v.session().unwrap().resolution(chunk).map(|r| r.how)
    })
}

pub fn lines_of(
    cx: &mut TestAppContext,
    view: &Entity<MergeView>,
    chunk: usize,
) -> Option<Vec<String>> {
    view.read_with(cx, |v, _| {
        v.session()
            .unwrap()
            .resolution(chunk)
            .map(|r| r.lines.clone())
    })
}

pub fn dirty(cx: &mut TestAppContext, view: &Entity<MergeView>) -> bool {
    view.read_with(cx, |v, _| v.is_dirty())
}

pub fn status_of(cx: &mut TestAppContext, view: &Entity<MergeView>) -> Vec<String> {
    view.read_with(cx, |v, _| texts(&v.status_parts()))
}

/// The view's keys, for a test that presses a key in a workspace window.
pub fn focus_view(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<MergeView>) {
    in_view(cx, handle, view, |v, window, cx| {
        window.focus(&v.focus_handle(cx), cx)
    });
}

pub fn result_strings(cx: &mut TestAppContext, view: &Entity<MergeView>) -> Vec<String> {
    view.read_with(cx, |v, _| {
        v.session()
            .unwrap()
            .result_lines()
            .into_iter()
            .map(str::to_owned)
            .collect()
    })
}

/// A focused view that read these files (in a bare window), the output box set, drawn once
/// the merge landed.
pub async fn open_files(
    cx: &mut TestAppContext,
    files: [&Path; 3],
    output: Option<PathBuf>,
) -> (AnyWindowHandle, Entity<MergeView>) {
    let (handle, view) = open_view(cx);
    view.update(cx, |v, cx| {
        v.open_slot_path(LEFT, files[0].to_path_buf(), cx);
        v.open_slot_path(BASE, files[1].to_path_buf(), cx);
        v.open_slot_path(RIGHT, files[2].to_path_buf(), cx);
    });
    wait_idle(cx, handle, &view).await;
    if output.is_some() {
        in_view(cx, handle, &view, |v, window, cx| {
            v.set_output(output, window, cx)
        });
    }
    draw(cx, handle);
    (handle, view)
}

/// The three files of `f2()` as LF text files.
pub fn f2_files() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let [l, b, r] = f2().map(|lines| lines.iter().map(|l| format!("{l}\n")).collect::<String>());
    write_set(&l, &b, &r)
}

pub async fn wait_notice(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<MergeView>,
    start: &'static str,
) {
    let seen = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        seen.read(cx).notice().is_some_and(|n| n.starts_with(start))
    })
    .await;
}

/// The prompt a background write raises is not there at once.
pub async fn wait_prompt(cx: &mut TestAppContext) {
    for _ in 0..2000 {
        cx.run_until_parked();
        if cx.has_pending_prompt() {
            return;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    panic!("no prompt appeared");
}

/// Runs the executor one task at a time until `done` holds, so a test can act between a load
/// landing and the merge it starts.
pub fn step_until(cx: &mut TestAppContext, mut done: impl FnMut(&mut App) -> bool) {
    for _ in 0..100_000 {
        if cx.update(&mut done) {
            return;
        }
        if !cx.executor().tick() {
            break;
        }
    }
    assert!(cx.update(&mut done), "the condition never held");
}

/// `target` spelled relative to the working folder: `..` up to the drive's root, then down. A
/// relative output the view failed to refuse would still land on `target`, in a temp folder.
pub fn relative_to_cwd(target: &Path) -> PathBuf {
    use std::path::Component;
    let cwd = std::env::current_dir().unwrap();
    let drive = |path: &Path| {
        path.components()
            .next()
            .map(|c| c.as_os_str().to_ascii_uppercase())
    };
    assert_eq!(
        drive(&cwd),
        drive(target),
        "the temp folder is on the same drive"
    );
    let mut relative = PathBuf::new();
    for _ in cwd
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
    {
        relative.push("..");
    }
    for component in target.components() {
        if let Component::Normal(part) = component {
            relative.push(part);
        }
    }
    relative
}
