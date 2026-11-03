//! The display and compare choices of a text compare, remembered in `settings.json` (R7, PF9).

use std::io;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use duet_core::text::diff::WhitespaceMode;
use duet_core::text::encoding::TextEncoding;
use duet_core::text::lines::EolStyle;
use encoding_rs::{UTF_8, UTF_16LE, WINDOWS_874};
use gpui_kit::{App, Global};

use crate::settings;
use crate::text::model::TextFilter;
use crate::text::view::FONT_SIZES;

pub const WRAP: &str = "text.wrap";
pub const SHOW_SPACES: &str = "text.show_spaces";
pub const SYNTAX: &str = "text.syntax";
pub const MOVES: &str = "text.moves";
pub const TAB_WIDTH: &str = "text.tab_width";
pub const FONT_SIZE: &str = "text.font_size";
pub const DETAILS: &str = "text.details";
pub const FILTER: &str = "text.filter";
pub const WHITESPACE: &str = "text.whitespace";
pub const IGNORE_CASE: &str = "text.ignore_case";
pub const IGNORE_EOL: &str = "text.ignore_eol";
pub const IGNORE_BLANK: &str = "text.ignore_blank";
pub const IGNORE_COMMENTS: &str = "text.ignore_comments";
pub const RULES_TEXT: &str = "text.rules_text";
pub const RULES_LINES: &str = "text.rules_lines";
pub const CONTEXT_LINES: &str = "text.context_lines";
pub const SAVE_EOL: &str = "text.save_eol";
pub const SAVE_ENCODING: &str = "text.save_encoding";

/// What the line endings of a saved file become.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SaveEol {
    #[default]
    Keep,
    CrLf,
    Lf,
}

impl SaveEol {
    pub fn key(self) -> &'static str {
        match self {
            SaveEol::Keep => "keep",
            SaveEol::CrLf => "crlf",
            SaveEol::Lf => "lf",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        [SaveEol::Keep, SaveEol::CrLf, SaveEol::Lf]
            .into_iter()
            .find(|choice| choice.key() == key)
    }

    /// The style a file is written in, given the one it was read in.
    pub fn apply(self, eol: EolStyle) -> EolStyle {
        match self {
            SaveEol::Keep => eol,
            SaveEol::CrLf => EolStyle::CrLf,
            SaveEol::Lf => EolStyle::Lf,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SaveEol::Keep => "keep",
            SaveEol::CrLf => "CRLF",
            SaveEol::Lf => "LF",
        }
    }
}

/// What the encoding of a saved file becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SaveEncoding {
    #[default]
    Keep,
    Utf8,
    Utf8Bom,
    Windows874,
    Utf16Le,
}

impl SaveEncoding {
    pub fn key(self) -> &'static str {
        match self {
            SaveEncoding::Keep => "keep",
            SaveEncoding::Utf8 => "utf-8",
            SaveEncoding::Utf8Bom => "utf-8-bom",
            SaveEncoding::Windows874 => "windows-874",
            SaveEncoding::Utf16Le => "utf-16le",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        [
            SaveEncoding::Keep,
            SaveEncoding::Utf8,
            SaveEncoding::Utf8Bom,
            SaveEncoding::Windows874,
            SaveEncoding::Utf16Le,
        ]
        .into_iter()
        .find(|choice| choice.key() == key)
    }

    /// The encoding a file is written in, given the one it was read in. UTF-16LE always gets a
    /// BOM: without one, duet's binary check would refuse to open the file again (PF4).
    pub fn apply(self, encoding: TextEncoding) -> TextEncoding {
        let with = |encoding, bom| TextEncoding { encoding, bom };
        match self {
            SaveEncoding::Keep => encoding,
            SaveEncoding::Utf8 => with(UTF_8, false),
            SaveEncoding::Utf8Bom => with(UTF_8, true),
            SaveEncoding::Windows874 => with(WINDOWS_874, false),
            SaveEncoding::Utf16Le => with(UTF_16LE, true),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SaveEncoding::Keep => "keep",
            SaveEncoding::Utf8 => "UTF-8",
            SaveEncoding::Utf8Bom => "UTF-8 BOM",
            SaveEncoding::Windows874 => "windows-874",
            SaveEncoding::Utf16Le => "UTF-16LE",
        }
    }
}

/// Lines of context the Context filter shows: the default and the most the panel accepts
/// (LocalDiff's `ruCtx`, `ops:576`).
pub const DEFAULT_CONTEXT_LINES: u32 = 3;
pub const MAX_CONTEXT_LINES: u32 = 50;

/// Which rows the view shows, without the context size (that is a setting of its own).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterKey {
    #[default]
    All,
    Differences,
    Context,
    Same,
}

impl FilterKey {
    const ALL: [FilterKey; 4] = [
        FilterKey::All,
        FilterKey::Differences,
        FilterKey::Context,
        FilterKey::Same,
    ];

    pub fn key(self) -> &'static str {
        match self {
            FilterKey::All => "all",
            FilterKey::Differences => "differences",
            FilterKey::Context => "context",
            FilterKey::Same => "same",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|filter| filter.key() == key)
    }

    pub fn of(filter: TextFilter) -> Self {
        match filter {
            TextFilter::All => FilterKey::All,
            TextFilter::Differences => FilterKey::Differences,
            TextFilter::Context(_) => FilterKey::Context,
            TextFilter::Same => FilterKey::Same,
        }
    }

    /// The filter to show; `context` is the lines around a difference for "context".
    pub fn filter(self, context: u32) -> TextFilter {
        match self {
            FilterKey::All => TextFilter::All,
            FilterKey::Differences => TextFilter::Differences,
            FilterKey::Context => TextFilter::Context(context),
            FilterKey::Same => TextFilter::Same,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextPrefs {
    pub wrap: bool,
    pub show_spaces: bool,
    pub syntax: bool,
    pub moves: bool,
    pub tab_width: u8,
    pub font_size: f32,
    pub details: bool,
    pub filter: FilterKey,
    /// The compare options (R2, R4): every tab starts with the last ones chosen.
    pub whitespace: WhitespaceMode,
    pub ignore_case: bool,
    pub ignore_eol: bool,
    pub ignore_blank: bool,
    pub ignore_comments: bool,
    /// The rules panel's two lists, one regex per line (R8); global, so every tab uses them.
    pub rules_text: String,
    pub rules_lines: String,
    /// Lines around a difference in the Context filter, 0 to 50.
    pub context_lines: u32,
    /// What a save does to the file's line endings and encoding (R13).
    pub save_eol: SaveEol,
    pub save_encoding: SaveEncoding,
}

impl Default for TextPrefs {
    fn default() -> Self {
        Self {
            wrap: false,
            show_spaces: false,
            syntax: true,
            moves: true,
            tab_width: 4,
            font_size: 12.5,
            details: false,
            filter: FilterKey::All,
            whitespace: WhitespaceMode::None,
            ignore_case: false,
            ignore_eol: false,
            ignore_blank: false,
            ignore_comments: false,
            rules_text: String::new(),
            rules_lines: String::new(),
            context_lines: DEFAULT_CONTEXT_LINES,
            save_eol: SaveEol::Keep,
            save_encoding: SaveEncoding::Keep,
        }
    }
}

/// Tab widths a saved value is clamped to (R5).
pub const TAB_WIDTHS: RangeInclusive<u8> = 1..=16;

/// The choices every text tab shares (R9, R13): the rules panel's two lists and context size,
/// and the On save choices. A tab copies them in before it compares, saves or shows them, and
/// hands its change to every tab as it writes its key, so no tab compares with, shows or saves
/// an old copy, and none writes one back over a newer list (fix wave I2).
#[derive(Debug, Clone, PartialEq)]
pub struct SharedTextPrefs {
    /// The settings file the choices belong to; a tab of another file (a test) keeps its own.
    file: Option<PathBuf>,
    rules_text: String,
    rules_lines: String,
    context_lines: u32,
    save_eol: SaveEol,
    save_encoding: SaveEncoding,
}

impl Global for SharedTextPrefs {}

impl SharedTextPrefs {
    fn of(prefs: &TextPrefs, file: Option<PathBuf>) -> Self {
        Self {
            file,
            rules_text: prefs.rules_text.clone(),
            rules_lines: prefs.rules_lines.clone(),
            context_lines: prefs.context_lines,
            save_eol: prefs.save_eol,
            save_encoding: prefs.save_encoding,
        }
    }

    /// Into `prefs`; a list is copied only when it differs.
    fn give(&self, prefs: &mut TextPrefs) {
        if prefs.rules_text != self.rules_text {
            prefs.rules_text.clone_from(&self.rules_text);
        }
        if prefs.rules_lines != self.rules_lines {
            prefs.rules_lines.clone_from(&self.rules_lines);
        }
        prefs.context_lines = self.context_lines;
        prefs.save_eol = self.save_eol;
        prefs.save_encoding = self.save_encoding;
    }
}

/// A new tab's loaded choices: the first tab of a settings file shares them, a later one takes
/// the shared ones (this session's newest, even when a write to the file failed).
pub fn join_shared(prefs: &mut TextPrefs, cx: &mut App) {
    let file = settings::settings_file(cx);
    match cx.try_global::<SharedTextPrefs>() {
        Some(shared) if shared.file == file => shared.give(prefs),
        _ => cx.set_global(SharedTextPrefs::of(prefs, file)),
    }
}

/// The shared choices, into a tab's `prefs`.
pub fn pull_shared(prefs: &mut TextPrefs, cx: &App) {
    if let Some(shared) = cx.try_global::<SharedTextPrefs>()
        && shared.file == settings::settings_file(cx)
    {
        shared.give(prefs);
    }
}

/// A tab's choices, for every tab, after it changed one (pulled first, so the rest are the
/// shared ones).
pub fn push_shared(prefs: &TextPrefs, cx: &mut App) {
    let file = settings::settings_file(cx);
    cx.set_global(SharedTextPrefs::of(prefs, file));
}

impl TextPrefs {
    /// The saved choices; anything missing or unreadable is the default, numbers clamp. None
    /// (tests) is all defaults.
    pub fn load(path: Option<&Path>) -> TextPrefs {
        let default = TextPrefs::default();
        let Some(path) = path else {
            return default;
        };
        // One read of the file for every key (M4).
        let saved = settings::read_all(path);
        let flag = |key: &str, default: bool| {
            saved
                .get(key)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(default)
        };
        let text = |key: &str| {
            saved
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let (min, max) = (FONT_SIZES[0], FONT_SIZES[FONT_SIZES.len() - 1]);
        TextPrefs {
            wrap: flag(WRAP, default.wrap),
            show_spaces: flag(SHOW_SPACES, default.show_spaces),
            syntax: flag(SYNTAX, default.syntax),
            moves: flag(MOVES, default.moves),
            tab_width: saved
                .get(TAB_WIDTH)
                .and_then(serde_json::Value::as_u64)
                .map_or(default.tab_width, |value| {
                    value.clamp((*TAB_WIDTHS.start()).into(), (*TAB_WIDTHS.end()).into()) as u8
                }),
            font_size: saved
                .get(FONT_SIZE)
                .and_then(serde_json::Value::as_f64)
                .map_or(default.font_size, |value| (value as f32).clamp(min, max)),
            details: flag(DETAILS, default.details),
            filter: saved
                .get(FILTER)
                .and_then(serde_json::Value::as_str)
                .and_then(FilterKey::from_key)
                .unwrap_or(default.filter),
            whitespace: saved
                .get(WHITESPACE)
                .and_then(serde_json::Value::as_str)
                .and_then(WhitespaceMode::from_key)
                .unwrap_or(default.whitespace),
            ignore_case: flag(IGNORE_CASE, default.ignore_case),
            ignore_eol: flag(IGNORE_EOL, default.ignore_eol),
            ignore_blank: flag(IGNORE_BLANK, default.ignore_blank),
            ignore_comments: flag(IGNORE_COMMENTS, default.ignore_comments),
            rules_text: text(RULES_TEXT),
            rules_lines: text(RULES_LINES),
            context_lines: saved
                .get(CONTEXT_LINES)
                .and_then(serde_json::Value::as_u64)
                .map_or(default.context_lines, |value| {
                    value.min(MAX_CONTEXT_LINES.into()) as u32
                }),
            save_eol: saved
                .get(SAVE_EOL)
                .and_then(serde_json::Value::as_str)
                .and_then(SaveEol::from_key)
                .unwrap_or(default.save_eol),
            save_encoding: saved
                .get(SAVE_ENCODING)
                .and_then(serde_json::Value::as_str)
                .and_then(SaveEncoding::from_key)
                .unwrap_or(default.save_encoding),
        }
    }

    /// Writes every choice; None (tests) writes nothing. The view saves one key per change
    /// (`save_key`) so a second tab never overwrites newer choices (PF9).
    pub fn save(&self, path: Option<&Path>) -> io::Result<()> {
        for key in [
            WRAP,
            SHOW_SPACES,
            SYNTAX,
            MOVES,
            TAB_WIDTH,
            FONT_SIZE,
            DETAILS,
            FILTER,
            WHITESPACE,
            IGNORE_CASE,
            IGNORE_EOL,
            IGNORE_BLANK,
            IGNORE_COMMENTS,
            RULES_TEXT,
            RULES_LINES,
            CONTEXT_LINES,
            SAVE_EOL,
            SAVE_ENCODING,
        ] {
            self.save_key(key, path)?;
        }
        Ok(())
    }

    /// Writes the one choice named by `key` (one of the `text.*` constants).
    pub fn save_key(&self, key: &str, path: Option<&Path>) -> io::Result<()> {
        let Some(path) = path else {
            return Ok(());
        };
        match key {
            WRAP => settings::set_bool(path, key, self.wrap),
            SHOW_SPACES => settings::set_bool(path, key, self.show_spaces),
            SYNTAX => settings::set_bool(path, key, self.syntax),
            MOVES => settings::set_bool(path, key, self.moves),
            TAB_WIDTH => settings::set_u64(path, key, self.tab_width.into()),
            FONT_SIZE => settings::set_f64(path, key, self.font_size.into()),
            DETAILS => settings::set_bool(path, key, self.details),
            FILTER => settings::set_string(path, key, self.filter.key()),
            WHITESPACE => settings::set_string(path, key, self.whitespace.key()),
            IGNORE_CASE => settings::set_bool(path, key, self.ignore_case),
            IGNORE_EOL => settings::set_bool(path, key, self.ignore_eol),
            IGNORE_BLANK => settings::set_bool(path, key, self.ignore_blank),
            IGNORE_COMMENTS => settings::set_bool(path, key, self.ignore_comments),
            RULES_TEXT => settings::set_string(path, key, &self.rules_text),
            RULES_LINES => settings::set_string(path, key, &self.rules_lines),
            CONTEXT_LINES => settings::set_u64(path, key, self.context_lines.into()),
            SAVE_EOL => settings::set_string(path, key, self.save_eol.key()),
            SAVE_ENCODING => settings::set_string(path, key, self.save_encoding.key()),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_prefs_round_trip_and_clamp() {
        assert_eq!(TextPrefs::load(None), TextPrefs::default());
        assert!(TextPrefs::default().save(None).is_ok(), "nowhere to write");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(TextPrefs::load(Some(&path)), TextPrefs::default());
        let prefs = TextPrefs {
            wrap: true,
            show_spaces: true,
            syntax: false,
            moves: false,
            tab_width: 8,
            font_size: 16.,
            details: true,
            filter: FilterKey::Same,
            ..TextPrefs::default()
        };
        prefs.save(Some(&path)).unwrap();
        assert_eq!(TextPrefs::load(Some(&path)), prefs);

        let load = |json: &str| {
            std::fs::write(&path, json).unwrap();
            TextPrefs::load(Some(&path))
        };
        assert_eq!(load(r#"{"text.tab_width": 99}"#).tab_width, 16);
        assert_eq!(load(r#"{"text.tab_width": 0}"#).tab_width, 1);
        assert_eq!(load(r#"{"text.font_size": 3}"#).font_size, 9.0);
        assert_eq!(load(r#"{"text.font_size": 40}"#).font_size, 24.0);
        assert_eq!(load(r#"{"text.font_size": 12.5}"#).font_size, 12.5);
        assert_eq!(load(r#"{"text.filter": "bogus"}"#).filter, FilterKey::All);
        assert_eq!(
            load(r#"{"text.filter": "context"}"#).filter,
            FilterKey::Context
        );
        assert!(
            load(r#"{"text.syntax": "yes"}"#).syntax,
            "a bad bool is the default"
        );

        // One key per change (PF9): a stale copy never overwrites the others.
        std::fs::write(&path, r#"{"text.tab_width": 3, "text.wrap": true}"#).unwrap();
        TextPrefs::default()
            .save_key(TAB_WIDTH, Some(&path))
            .unwrap();
        let after = TextPrefs::load(Some(&path));
        assert_eq!((after.tab_width, after.wrap), (4, true));

        assert_eq!(FilterKey::of(TextFilter::Context(7)), FilterKey::Context);
        assert_eq!(FilterKey::Context.filter(3), TextFilter::Context(3));
        for key in ["all", "differences", "context", "same"] {
            assert_eq!(FilterKey::from_key(key).unwrap().key(), key);
        }
        assert_eq!(FilterKey::from_key("nope"), None);
    }

    #[test]
    fn save_choices_round_trip_and_apply() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let prefs = TextPrefs {
            save_eol: SaveEol::CrLf,
            save_encoding: SaveEncoding::Windows874,
            ..TextPrefs::default()
        };
        prefs.save(Some(&path)).unwrap();
        assert_eq!(TextPrefs::load(Some(&path)), prefs);
        let saved = settings::read_all(&path);
        assert_eq!(
            (&saved[SAVE_EOL], &saved[SAVE_ENCODING]),
            (&"crlf".into(), &"windows-874".into())
        );
        for choice in [
            SaveEncoding::Keep,
            SaveEncoding::Utf8,
            SaveEncoding::Utf8Bom,
            SaveEncoding::Windows874,
            SaveEncoding::Utf16Le,
        ] {
            assert_eq!(SaveEncoding::from_key(choice.key()), Some(choice));
        }
        for choice in [SaveEol::Keep, SaveEol::CrLf, SaveEol::Lf] {
            assert_eq!(SaveEol::from_key(choice.key()), Some(choice));
        }
        let bad = r#"{"text.save_eol": "cr", "text.save_encoding": 5}"#;
        std::fs::write(&path, bad).unwrap();
        let read = TextPrefs::load(Some(&path));
        assert_eq!(
            (read.save_eol, read.save_encoding),
            (SaveEol::Keep, SaveEncoding::Keep)
        );
        // One key per change.
        std::fs::write(&path, r#"{"text.save_encoding": "utf-8"}"#).unwrap();
        prefs.save_key(SAVE_EOL, Some(&path)).unwrap();
        let after = TextPrefs::load(Some(&path));
        assert_eq!(
            (after.save_eol, after.save_encoding),
            (SaveEol::CrLf, SaveEncoding::Utf8)
        );

        assert_eq!(SaveEol::Lf.apply(EolStyle::CrLf), EolStyle::Lf);
        assert_eq!(SaveEol::CrLf.apply(EolStyle::None), EolStyle::CrLf);
        assert_eq!(SaveEol::Keep.apply(EolStyle::Mixed), EolStyle::Mixed);
        let w874 = TextEncoding {
            encoding: encoding_rs::WINDOWS_874,
            bom: false,
        };
        let utf8 = |bom| TextEncoding {
            encoding: encoding_rs::UTF_8,
            bom,
        };
        assert_eq!(SaveEncoding::Utf8Bom.apply(w874), utf8(true));
        assert_eq!(SaveEncoding::Utf8.apply(utf8(true)), utf8(false));
        assert_eq!(SaveEncoding::Keep.apply(w874), w874);
        assert_eq!(SaveEncoding::Windows874.apply(utf8(true)), w874);
        let utf16 = SaveEncoding::Utf16Le.apply(w874);
        assert_eq!(
            (utf16.encoding, utf16.bom),
            (encoding_rs::UTF_16LE, true),
            "PF4"
        );
        assert_eq!(SaveEol::Lf.label(), "LF");
        assert_eq!(SaveEncoding::Utf8Bom.label(), "UTF-8 BOM");
    }

    #[test]
    fn rules_and_context_lines_round_trip_and_clamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let prefs = TextPrefs {
            rules_text: "\\d+\nx".into(),
            rules_lines: "^#".into(),
            context_lines: 7,
            ..TextPrefs::default()
        };
        prefs.save(Some(&path)).unwrap();
        assert_eq!(TextPrefs::load(Some(&path)), prefs);
        let load = |json: &str| {
            std::fs::write(&path, json).unwrap();
            TextPrefs::load(Some(&path))
        };
        assert_eq!(load(r#"{"text.context_lines": 99}"#).context_lines, 50);
        assert_eq!(load(r#"{"text.context_lines": 0}"#).context_lines, 0);
        assert_eq!(load(r#"{"text.context_lines": "9"}"#).context_lines, 3);
        let bad = load(r#"{"text.rules_text": 5, "text.rules_lines": null}"#);
        assert_eq!(
            (bad.rules_text.as_str(), bad.rules_lines.as_str()),
            ("", "")
        );
        // One key per change: saving the context leaves the lists alone.
        std::fs::write(&path, r#"{"text.rules_lines": "keep"}"#).unwrap();
        prefs.save_key(CONTEXT_LINES, Some(&path)).unwrap();
        let after = TextPrefs::load(Some(&path));
        assert_eq!(
            (after.rules_lines.as_str(), after.context_lines),
            ("keep", 7)
        );
    }

    #[test]
    fn compare_choices_round_trip_and_bad_values_are_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let prefs = TextPrefs {
            whitespace: WhitespaceMode::Amount,
            ignore_case: true,
            ignore_eol: true,
            ignore_blank: true,
            ignore_comments: true,
            ..TextPrefs::default()
        };
        prefs.save(Some(&path)).unwrap();
        assert_eq!(TextPrefs::load(Some(&path)), prefs);
        let saved = settings::read_all(&path);
        assert_eq!(saved[WHITESPACE], "amount");
        for key in [IGNORE_CASE, IGNORE_EOL, IGNORE_BLANK, IGNORE_COMMENTS] {
            assert_eq!(saved[key], true, "{key}");
        }

        let all = r#"{"text.whitespace": "trailing", "text.ignore_case": true,
            "text.ignore_eol": false, "text.ignore_blank": true, "text.ignore_comments": false}"#;
        std::fs::write(&path, all).unwrap();
        let read = TextPrefs::load(Some(&path));
        assert_eq!(
            (
                read.whitespace,
                read.ignore_case,
                read.ignore_eol,
                read.ignore_blank,
                read.ignore_comments
            ),
            (WhitespaceMode::Trailing, true, false, true, false)
        );
        let bad = r#"{"text.whitespace": "most", "text.ignore_blank": "yes",
            "text.ignore_comments": 1}"#;
        std::fs::write(&path, bad).unwrap();
        let read = TextPrefs::load(Some(&path));
        assert_eq!(
            read.whitespace,
            WhitespaceMode::None,
            "a bad mode is the default"
        );
        assert!(!read.ignore_blank && !read.ignore_comments);

        // One key per change: saving the whitespace mode leaves the flags alone.
        std::fs::write(&path, r#"{"text.ignore_case": true}"#).unwrap();
        prefs.save_key(WHITESPACE, Some(&path)).unwrap();
        let after = TextPrefs::load(Some(&path));
        assert_eq!(
            (after.whitespace, after.ignore_case),
            (WhitespaceMode::Amount, true)
        );
    }
}
