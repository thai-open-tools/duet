//! Path bars: the file or folder name first, then its folder cut from the start, with the
//! full path on hover and a copy button, so deep paths never hide the name (review H1).

use std::path::{Path, PathBuf};

use gpui_kit::TestSupportExt as _;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, ClickEvent, ClipboardItem, Context, Div, Entity, FontWeight,
    SharedString, Subscription, Window, div, prelude::*, relative,
};

use crate::palette::Palette;
use crate::ui::icons;
use crate::ui::style::{LdExt as _, ld_small_icon_button, ld_tip};

/// The last component and the folder that holds it. A root such as `C:\` has no name of its
/// own, so the whole path is the name.
pub fn split_path(path: &Path) -> (String, String) {
    match (path.file_name(), path.parent()) {
        (Some(name), Some(parent)) if !parent.as_os_str().is_empty() => (
            name.to_string_lossy().into_owned(),
            parent.display().to_string(),
        ),
        _ => (path.display().to_string(), String::new()),
    }
}

pub const NOT_FULL_PATH: &str = "Type a full path, such as C:\\folder\\file.txt.";
pub const FOLDER_DROP_NOTICE: &str = "Drop files here; folders open in a folder compare.";

pub fn not_a_file(path: &str) -> String {
    format!("Not a file: {path}.")
}

/// A typed path box's text as an existing file: cleaned (`clean_typed`), absolute, and made
/// plain (`C:/x/y` or a path with ".." become the usual spelling). The error is the notice.
pub fn typed_file(text: &str) -> Result<PathBuf, String> {
    let typed = clean_typed(text);
    let path = PathBuf::from(typed);
    if !typed.is_empty() && !path.is_absolute() {
        return Err(NOT_FULL_PATH.to_owned());
    }
    let path = std::path::absolute(&path).unwrap_or(path);
    if typed.is_empty() || !path.is_file() {
        return Err(not_a_file(typed));
    }
    Ok(path)
}

/// The files of a drop: the first two, or the notice when one is a folder or missing.
pub fn dropped_files(paths: &[PathBuf]) -> Result<&[PathBuf], String> {
    dropped_files_up_to(paths, 2)
}

/// The first `n` files of a drop, or the notice when one is a folder or missing.
pub fn dropped_files_up_to(paths: &[PathBuf], n: usize) -> Result<&[PathBuf], String> {
    let used = &paths[..paths.len().min(n)];
    match used.iter().find(|p| !p.is_file()) {
        Some(bad) if bad.is_dir() => Err(FOLDER_DROP_NOTICE.to_owned()),
        Some(bad) => Err(not_a_file(&bad.display().to_string())),
        None => Ok(used),
    }
}

/// Whether a drop at `x` lands on the left half, when the halves split at `split_x` (both in
/// px from the same edge). Each view passes the split of its own two halves.
pub fn over_left_half(x: f32, split_x: f32) -> bool {
    x < split_x
}

/// A path box being edited: an input holding the path, in place of the bar (rule R10).
pub struct PathEdit {
    /// Which side or pane the box belongs to.
    pub left: bool,
    pub input: Entity<InputState>,
}

impl PathEdit {
    /// An input holding `path`, all selected, focused.
    pub fn begin(left: bool, path: &Path, window: &mut Window, cx: &mut App) -> Self {
        let text = path.display().to_string();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(text));
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        Self { left, input }
    }

    pub fn text(&self, cx: &App) -> String {
        self.input.read(cx).value().to_string()
    }
}

/// A `PathEdit` on show in a view, with the window it lives in and the subscription that
/// ends it: Enter runs `on_enter`, losing focus runs `on_blur`.
pub struct ActiveEdit {
    pub edit: PathEdit,
    window: AnyWindowHandle,
    _subscription: Subscription,
}

impl ActiveEdit {
    pub fn start<V: 'static>(
        left: bool,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<V>,
        on_enter: fn(&mut V, &mut Window, &mut Context<V>),
        on_blur: fn(&mut V, &mut Window, &mut Context<V>),
    ) -> Self {
        let edit = PathEdit::begin(left, path, window, cx);
        let _subscription = cx.subscribe_in(
            &edit.input,
            window,
            move |this: &mut V, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => on_enter(this, window, cx),
                InputEvent::Blur => on_blur(this, window, cx),
                _ => {}
            },
        );
        Self {
            edit,
            window: window.window_handle(),
            _subscription,
        }
    }

    pub fn left(&self) -> bool {
        self.edit.left
    }

    pub fn edit(&self) -> &PathEdit {
        &self.edit
    }

    /// Replaces the typed text, as typing would (for tests).
    pub fn set_text(&self, text: &str, cx: &mut App) {
        let input = self.edit.input.clone();
        let _ = cx.update_window(self.window, |_, window, cx| {
            input.update(cx, |state, cx| state.set_value(text, window, cx))
        });
    }
}

/// What was typed, as a path: without the spaces around it and the pair of quotes that
/// Explorer's "Copy as path" puts on it.
pub fn clean_typed(text: &str) -> &str {
    let text = text.trim();
    text.strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .map_or(text, str::trim)
}

/// `render` helper: the input for an edit in progress, styled as the bar it replaces.
pub fn path_input(id: &str, edit: &PathEdit, palette: &Palette) -> AnyElement {
    h_flex()
        .id(SharedString::from(id.to_owned()))
        .test_support()
        .flex_1()
        .min_w_0()
        .text_color(palette.text)
        .child(Input::new(&edit.input).aria_label("Path"))
        .into_any_element()
}

/// `id` names the bar for tests and tooltips; the copy button is `{id}-copy`.
pub fn path_bar(id: &str, path: &Path, palette: &Palette) -> Div {
    path_bar_click(id, path, palette, |_, _, _| {})
}

/// `path_bar` whose name and folder run `on_click` when clicked (to start a `PathEdit`).
pub fn path_bar_click(
    id: &str,
    path: &Path,
    palette: &Palette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    let (name, parent) = split_path(path);
    let full: SharedString = path.display().to_string().into();
    let tip = full.clone();
    h_flex()
        .flex_1()
        .min_w_0()
        .gap_1()
        .child(
            h_flex()
                .id(SharedString::from(id.to_owned()))
                .test_support()
                .flex_1()
                .min_w_0()
                .gap_2()
                .cursor_text()
                .on_click(on_click)
                .tooltip(move |window, cx| ld_tip(tip.clone(), window, cx))
                .child(
                    div()
                        .flex_none()
                        .max_w(relative(0.7))
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_start()
                        .text_color(palette.muted)
                        .child(parent),
                ),
        )
        .child(
            ld_small_icon_button(
                SharedString::from(format!("{id}-copy")),
                icons::COPY_PATH,
                "Copy path",
                palette,
            )
            .ld_click(true, move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(full.to_string()))
            }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn explorers_quotes_and_stray_spaces_are_stripped() {
        assert_eq!(clean_typed(r#""C:\a b\c.txt""#), r"C:\a b\c.txt");
        assert_eq!(clean_typed(r#"  "C:\x.txt"  "#), r"C:\x.txt");
        assert_eq!(clean_typed(r#" " C:\x.txt " "#), r"C:\x.txt");
        assert_eq!(clean_typed(r"C:\x.txt"), r"C:\x.txt");
        assert_eq!(
            clean_typed(r#""C:\x.txt"#),
            r#""C:\x.txt"#,
            "a lone quote stays"
        );
        assert_eq!(clean_typed(r#""""#), "", "an empty pair is empty");
        assert_eq!(clean_typed("\""), "\"", "one quote alone stays");
    }

    #[test]
    fn a_typed_file_is_cleaned_absolute_and_existing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();
        let quoted = format!("  \"{}\"  ", file.display());
        assert_eq!(typed_file(&quoted), Ok(file.clone()));
        assert_eq!(typed_file("rel.txt"), Err(NOT_FULL_PATH.to_owned()));
        assert_eq!(typed_file(""), Err("Not a file: .".to_owned()));
        let missing = dir.path().join("none.txt");
        assert_eq!(
            typed_file(&missing.display().to_string()),
            Err(format!("Not a file: {}.", missing.display()))
        );
        let dotted = format!("{}/sub/../a.txt", dir.path().display());
        assert_eq!(typed_file(&dotted).map(|p| p.is_file()), Ok(true));
    }

    #[test]
    fn dropped_files_keep_two_and_name_folders_and_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, c) = (
            dir.path().join("a"),
            dir.path().join("b"),
            dir.path().join("c"),
        );
        for f in [&a, &b, &c] {
            std::fs::write(f, "x").unwrap();
        }
        assert_eq!(dropped_files(&[]), Ok(&[][..]));
        assert_eq!(
            dropped_files(std::slice::from_ref(&a)),
            Ok(std::slice::from_ref(&a))
        );
        assert_eq!(
            dropped_files(&[a.clone(), b.clone(), c.clone()]),
            Ok(&[a.clone(), b.clone()][..]),
            "a third file is ignored"
        );
        assert_eq!(
            dropped_files(&[dir.path().to_path_buf()]),
            Err(FOLDER_DROP_NOTICE.to_owned())
        );
        let gone = dir.path().join("gone");
        assert_eq!(
            dropped_files(&[a, gone.clone()]),
            Err(format!("Not a file: {}.", gone.display()))
        );
    }

    #[test]
    fn dropped_files_up_to_keeps_that_many_and_names_the_same_problems() {
        let dir = tempfile::tempdir().unwrap();
        let files: Vec<PathBuf> = ["a", "b", "c", "d"]
            .iter()
            .map(|name| {
                let path = dir.path().join(name);
                std::fs::write(&path, "x").unwrap();
                path
            })
            .collect();
        assert_eq!(dropped_files_up_to(&files, 3), Ok(&files[..3]));
        assert_eq!(dropped_files_up_to(&files[..1], 3), Ok(&files[..1]));
        assert_eq!(dropped_files_up_to(&files, 1), Ok(&files[..1]));
        assert_eq!(dropped_files_up_to(&[], 3), Ok(&[][..]));
        let folder = dir.path().to_path_buf();
        assert_eq!(
            dropped_files_up_to(&[files[0].clone(), files[1].clone(), folder], 3),
            Err(FOLDER_DROP_NOTICE.to_owned()),
            "the third path is checked too"
        );
        let mut four = files.clone();
        four[3] = dir.path().to_path_buf();
        assert_eq!(
            dropped_files_up_to(&four, 3),
            Ok(&four[..3]),
            "a path past the limit is ignored"
        );
        let gone = dir.path().join("gone");
        assert_eq!(
            dropped_files_up_to(&[files[0].clone(), files[1].clone(), gone.clone()], 3),
            Err(format!("Not a file: {}.", gone.display()))
        );
    }

    #[test]
    fn the_pointer_is_over_the_left_half_before_the_split() {
        assert!(over_left_half(99.9, 100.));
        assert!(!over_left_half(100., 100.));
        assert!(!over_left_half(300., 100.));
        assert!(over_left_half(-5., 100.));
    }

    #[test]
    fn the_name_comes_first_and_the_folder_second() {
        assert_eq!(
            split_path(Path::new(r"C:\work\deep\src\main.rs")),
            ("main.rs".to_owned(), r"C:\work\deep\src".to_owned())
        );
    }

    #[test]
    fn roots_and_unc_paths_keep_a_name() {
        assert_eq!(
            split_path(Path::new(r"C:\")),
            (r"C:\".to_owned(), String::new())
        );
        assert_eq!(
            split_path(Path::new(r"\\server\share\dir\f.txt")),
            ("f.txt".to_owned(), r"\\server\share\dir".to_owned())
        );
        assert_eq!(
            split_path(Path::new(r"\\server\share")),
            (r"\\server\share".to_owned(), String::new()),
            "a share root has no name of its own"
        );
        assert_eq!(
            split_path(Path::new("x.txt")),
            ("x.txt".to_owned(), String::new())
        );
    }
}
