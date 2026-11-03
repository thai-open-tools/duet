//! Light or dark (spec section 5). The mode comes from DUET_THEME, else the choice remembered
//! next to the recent list, else Windows. The header's ◐ button switches it and remembers it.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use gpui_component::highlighter::{HighlightTheme, HighlightThemeStyle, SyntaxColors, ThemeStyle};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Global, Hsla};

use crate::app::AppPaths;
use crate::palette::Palette;

pub const THEME_FILE: &str = "theme.txt";

pub fn theme_file(cx: &App) -> Option<PathBuf> {
    cx.try_global::<AppPaths>()
        .map(|paths| paths.data_dir.join(THEME_FILE))
}

/// "light" or "dark", in any case, with surrounding space or a BOM.
pub fn parse_mode(text: &str) -> Option<ThemeMode> {
    match text
        .trim_start_matches('\u{feff}')
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "light" => Some(ThemeMode::Light),
        "dark" => Some(ThemeMode::Dark),
        _ => None,
    }
}

/// A missing or unreadable file is no choice.
pub fn load(path: &Path) -> Option<ThemeMode> {
    let bytes = std::fs::read(path).ok()?;
    parse_mode(&String::from_utf8_lossy(&bytes))
}

/// Writes a temp file next to the choice, then renames it over the old one.
pub fn save(path: &Path, mode: ThemeMode) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("txt.tmp");
    std::fs::write(&temp, if mode.is_dark() { "dark\n" } else { "light\n" })?;
    std::fs::rename(&temp, path)
}

pub fn initial_mode(env: Option<&str>, saved: Option<ThemeMode>, system_dark: bool) -> ThemeMode {
    env.and_then(parse_mode)
        .or(saved)
        .unwrap_or(if system_dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        })
}

/// The mode to take when Windows switches between light and dark: None while DUET_THEME, a
/// remembered choice or a toggle this session decides, since an explicit choice wins. The
/// toggle counts even when its choice could not be saved.
pub fn follow_system(
    env: Option<&str>,
    saved: Option<ThemeMode>,
    chosen_this_session: bool,
    system_dark: bool,
) -> Option<ThemeMode> {
    if env.and_then(parse_mode).is_some() || saved.is_some() || chosen_this_session {
        return None;
    }
    Some(initial_mode(None, None, system_dark))
}

/// Set by the header's toggle: the user chose a mode in this session.
struct ChosenThisSession;

impl Global for ChosenThisSession {}

/// Windows switched between light and dark: follow it, unless the user chose a mode. The
/// saved choice is read again each time, since the header's toggle may have made one since
/// start.
pub fn system_changed(system_dark: bool, cx: &mut App) {
    let saved = theme_file(cx).and_then(|path| load(&path));
    let env = std::env::var("DUET_THEME").ok();
    let chosen = cx.has_global::<ChosenThisSession>();
    if let Some(mode) = follow_system(env.as_deref(), saved, chosen, system_dark)
        && mode != Theme::global(cx).mode
    {
        crate::app::configure_theme(mode, cx);
    }
}

/// Switches light and dark everywhere and remembers the choice. A failed write is logged:
/// the switch itself worked, and it still beats Windows for the rest of the session.
pub fn toggle(cx: &mut App) -> ThemeMode {
    let mode = if Theme::global(cx).mode.is_dark() {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    };
    crate::app::configure_theme(mode, cx);
    cx.set_global(ChosenThisSession);
    if let Some(path) = theme_file(cx)
        && let Err(err) = save(&path, mode)
    {
        tracing::warn!(%err, "could not remember the theme");
    }
    mode
}

static SYNTAX: LazyLock<[Arc<HighlightTheme>; 2]> = LazyLock::new(|| {
    [
        build_syntax(&Palette::light()),
        build_syntax(&Palette::dark()),
    ]
});

/// The tree-sitter colours for one mode, from the palette's `sx` set; built once per mode.
pub fn syntax_theme(dark: bool) -> Arc<HighlightTheme> {
    SYNTAX[usize::from(dark)].clone()
}

fn build_syntax(p: &Palette) -> Arc<HighlightTheme> {
    // ThemeStyle's fields are private; it deserializes from the theme-file shape.
    let style = |color: Hsla, italic: bool| -> Option<ThemeStyle> {
        let mut value = serde_json::json!({ "color": color });
        if italic {
            value["font_style"] = "italic".into();
        }
        serde_json::from_value(value).ok()
    };
    let x = p.sx;
    let syntax = SyntaxColors {
        comment: style(x.comment, true),
        comment_doc: style(x.comment, true),
        string: style(x.string, false),
        string_escape: style(x.string, false),
        string_regex: style(x.string, false),
        string_special: style(x.string, false),
        string_special_symbol: style(x.string, false),
        text_literal: style(x.string, false),
        text_code_span: style(x.string, false),
        number: style(x.number, false),
        boolean: style(x.number, false),
        constant: style(x.number, false),
        variant: style(x.number, false),
        keyword: style(x.keyword, false),
        title: style(x.keyword, false),
        type_: style(x.type_, false),
        function: style(x.type_, false),
        constructor: style(x.type_, false),
        enum_: style(x.type_, false),
        tag: style(x.tag, false),
        tag_doctype: style(x.tag, false),
        link_text: style(x.tag, false),
        link_uri: style(x.tag, false),
        attribute: style(x.attr, false),
        property: style(x.attr, false),
        preproc: style(x.decorator, false),
        label: style(x.decorator, false),
        variable_special: style(x.decorator, false),
        embedded: style(x.decorator, false),
        ..Default::default()
    };
    Arc::new(HighlightTheme {
        name: if p.dark { "duet dark" } else { "duet light" }.into(),
        appearance: if p.dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        style: HighlightThemeStyle {
            syntax,
            ..Default::default()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::Rgba;

    fn close(a: Hsla, b: Hsla) -> bool {
        let (a, b): (Rgba, Rgba) = (a.into(), b.into());
        [(a.r, b.r), (a.g, b.g), (a.b, b.b), (a.a, b.a)]
            .iter()
            .all(|(x, y)| (x - y).abs() <= 1.5 / 255.)
    }

    #[test]
    fn the_environment_beats_the_saved_choice_which_beats_windows() {
        assert_eq!(
            initial_mode(Some("light"), Some(ThemeMode::Dark), true),
            ThemeMode::Light
        );
        assert_eq!(initial_mode(Some(" DARK "), None, false), ThemeMode::Dark);
        assert_eq!(
            initial_mode(None, Some(ThemeMode::Light), true),
            ThemeMode::Light
        );
        assert_eq!(
            initial_mode(Some("purple"), None, true),
            ThemeMode::Dark,
            "an unknown DUET_THEME is ignored"
        );
        assert_eq!(
            initial_mode(None, None, false),
            ThemeMode::Light,
            "Windows decides last"
        );
    }

    #[test]
    fn windows_is_followed_only_without_an_explicit_choice() {
        assert_eq!(
            follow_system(None, None, false, true),
            Some(ThemeMode::Dark)
        );
        assert_eq!(
            follow_system(None, None, false, false),
            Some(ThemeMode::Light)
        );
        assert_eq!(
            follow_system(None, Some(ThemeMode::Light), false, true),
            None,
            "a saved choice wins"
        );
        assert_eq!(
            follow_system(None, None, true, true),
            None,
            "so does a choice made this session that could not be saved"
        );
        assert_eq!(
            follow_system(Some("light"), None, false, true),
            None,
            "DUET_THEME wins"
        );
        assert_eq!(
            follow_system(Some("purple"), None, false, true),
            Some(ThemeMode::Dark),
            "an unknown DUET_THEME is no choice"
        );
    }

    #[test]
    fn the_choice_round_trips_and_garbage_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join(THEME_FILE);
        assert_eq!(load(&path), None, "a missing file is no choice");
        save(&path, ThemeMode::Dark).unwrap();
        assert_eq!(load(&path), Some(ThemeMode::Dark));
        save(&path, ThemeMode::Light).unwrap();
        assert_eq!(load(&path), Some(ThemeMode::Light));
        std::fs::write(&path, "\u{feff}dark\r\n").unwrap();
        assert_eq!(
            load(&path),
            Some(ThemeMode::Dark),
            "Notepad's BOM and CRLF are fine"
        );
        std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(load(&path), None);
        let blocked = dir.path().join("file");
        std::fs::write(&blocked, "x").unwrap();
        assert!(
            save(&blocked.join(THEME_FILE), ThemeMode::Dark).is_err(),
            "a file where the folder should be"
        );
    }

    #[test]
    fn syntax_colours_come_from_the_palette() {
        for dark in [false, true] {
            let p = Palette::of(dark);
            let theme = syntax_theme(dark);
            let style = |name: &str| theme.style.syntax.style(name);
            assert!(close(
                style("keyword").unwrap().color.unwrap(),
                p.sx.keyword
            ));
            assert!(close(style("string").unwrap().color.unwrap(), p.sx.string));
            assert!(close(style("type").unwrap().color.unwrap(), p.sx.type_));
            let comment = style("comment").unwrap();
            assert!(close(comment.color.unwrap(), p.sx.comment));
            assert_eq!(comment.font_style, Some(gpui_kit::FontStyle::Italic));
            assert!(
                style("keyword.function").is_some(),
                "dotted names fall back to their head"
            );
            assert!(
                style("punctuation.bracket").is_none(),
                "punctuation keeps the text colour"
            );
            assert!(
                Arc::ptr_eq(&theme, &syntax_theme(dark)),
                "built once per mode"
            );
        }
    }
}
