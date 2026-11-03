//! The choices of a folder compare that stay between sessions, kept in `settings.json` (R3).

use std::io;
use std::path::Path;
use std::time::Duration;

use duet_core::folder::run::{CompareMode, FolderOptions, FolderTextRules};
use duet_core::folder::tree::QuickRule;
use duet_core::text::rules::compile_list;
use gpui_kit::App;

use crate::folder::model::FolderFilter;
use crate::settings;
use crate::text::prefs::{TextPrefs, pull_shared};

pub const CRITERION: &str = "folder.criterion";
pub const TIME_TOLERANCE: &str = "folder.time_tolerance";
pub const IGNORE_DST: &str = "folder.ignore_dst";
pub const FORMAT_IMPORTANT: &str = "folder.format_important";
pub const FILTER: &str = "folder.filter";
pub const VIEW: &str = "folder.view";
pub const ODD_SCAN: &str = "folder.odd_scan";
pub const CONFIRM_COPY: &str = "folder.confirm_copy";
pub const DOUBLE_CLICK: &str = "folder.double_click";

/// The largest time tolerance a setting may hold: one day, in seconds.
pub const MAX_TOLERANCE_SECS: u64 = 86_400;

const KEYS: [&str; 9] = [
    CRITERION,
    TIME_TOLERANCE,
    IGNORE_DST,
    FORMAT_IMPORTANT,
    FILTER,
    VIEW,
    ODD_SCAN,
    CONFIRM_COPY,
    DOUBLE_CLICK,
];

/// Whether the rows are a tree or one flat list of files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    #[default]
    Tree,
    Flat,
}

impl Layout {
    pub fn key(self) -> &'static str {
        match self {
            Layout::Tree => "tree",
            Layout::Flat => "flat",
        }
    }

    pub fn from_key(key: &str) -> Option<Layout> {
        [Layout::Tree, Layout::Flat]
            .into_iter()
            .find(|layout| layout.key() == key)
    }
}

pub fn mode_key(mode: CompareMode) -> &'static str {
    match mode {
        CompareMode::Quick => "quick",
        CompareMode::Content => "content",
        CompareMode::Size => "size",
        CompareMode::Rules => "rules",
    }
}

pub fn mode_from_key(key: &str) -> Option<CompareMode> {
    [
        CompareMode::Quick,
        CompareMode::Content,
        CompareMode::Size,
        CompareMode::Rules,
    ]
    .into_iter()
    .find(|mode| mode_key(*mode) == key)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderPrefs {
    pub mode: CompareMode,
    pub tolerance_secs: u64,
    pub ignore_dst: bool,
    pub format_important: bool,
    pub filter: FolderFilter,
    pub layout: Layout,
    pub odd_scan: bool,
    pub confirm_copy: bool,
    /// A double-click enters a folder instead of opening or closing it.
    pub enter_folders: bool,
}

impl Default for FolderPrefs {
    /// R2 with ruling C8: LocalDiff's choices, except that the criterion stays Quick, the
    /// default duet users have had since the first folder compare.
    fn default() -> Self {
        Self {
            mode: CompareMode::Quick,
            tolerance_secs: 2,
            ignore_dst: true,
            format_important: false,
            filter: FolderFilter::Mismatch,
            layout: Layout::Tree,
            odd_scan: true,
            confirm_copy: true,
            enter_folders: false,
        }
    }
}

impl FolderPrefs {
    /// The saved choices; a missing or unreadable key is the default and the tolerance clamps.
    /// None (tests) is all defaults.
    pub fn load(path: Option<&Path>) -> FolderPrefs {
        let default = FolderPrefs::default();
        let Some(path) = path else {
            return default;
        };
        // One read of the file for every key.
        let saved = settings::read_all(path);
        let text = |key: &str| saved.get(key).and_then(serde_json::Value::as_str);
        let flag = |key: &str, default: bool| {
            saved
                .get(key)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(default)
        };
        FolderPrefs {
            mode: text(CRITERION)
                .and_then(mode_from_key)
                .unwrap_or(default.mode),
            tolerance_secs: saved
                .get(TIME_TOLERANCE)
                .and_then(serde_json::Value::as_u64)
                .map_or(default.tolerance_secs, |secs| secs.min(MAX_TOLERANCE_SECS)),
            ignore_dst: flag(IGNORE_DST, default.ignore_dst),
            format_important: flag(FORMAT_IMPORTANT, default.format_important),
            filter: text(FILTER)
                .and_then(FolderFilter::from_key)
                .unwrap_or(default.filter),
            layout: text(VIEW)
                .and_then(Layout::from_key)
                .unwrap_or(default.layout),
            odd_scan: flag(ODD_SCAN, default.odd_scan),
            confirm_copy: flag(CONFIRM_COPY, default.confirm_copy),
            enter_folders: text(DOUBLE_CLICK)
                .map_or(default.enter_folders, |value| value == "enter"),
        }
    }

    /// Every key. None writes nothing (tests without a data folder).
    pub fn save(&self, path: Option<&Path>) -> io::Result<()> {
        for key in KEYS {
            self.save_key(key, path)?;
        }
        Ok(())
    }

    /// One key, after the choice behind it changed. None writes nothing.
    pub fn save_key(&self, key: &str, path: Option<&Path>) -> io::Result<()> {
        let Some(path) = path else {
            return Ok(());
        };
        match key {
            CRITERION => settings::set_string(path, key, mode_key(self.mode)),
            TIME_TOLERANCE => settings::set_u64(path, key, self.tolerance_secs),
            IGNORE_DST => settings::set_bool(path, key, self.ignore_dst),
            FORMAT_IMPORTANT => settings::set_bool(path, key, self.format_important),
            FILTER => settings::set_string(path, key, self.filter.key()),
            VIEW => settings::set_string(path, key, self.layout.key()),
            ODD_SCAN => settings::set_bool(path, key, self.odd_scan),
            CONFIRM_COPY => settings::set_bool(path, key, self.confirm_copy),
            DOUBLE_CLICK => settings::set_string(
                path,
                key,
                if self.enter_folders {
                    "enter"
                } else {
                    "toggle"
                },
            ),
            _ => Ok(()),
        }
    }

    /// The compare options these choices make; the masks, rules and limits stay at their
    /// defaults.
    pub fn options(&self) -> FolderOptions {
        FolderOptions {
            mode: self.mode,
            quick: QuickRule {
                tolerance: Duration::from_secs(self.tolerance_secs),
                ignore_dst: self.ignore_dst,
            },
            format_important: self.format_important,
            ..FolderOptions::default()
        }
    }

    /// The reverse of `options`, for a view built from given options.
    pub fn take_options(&mut self, options: &FolderOptions) {
        self.mode = options.mode;
        self.tolerance_secs = options.quick.tolerance.as_secs();
        self.ignore_dst = options.quick.ignore_dst;
        self.format_important = options.format_important;
    }
}

/// The text rules of a Rules-mode compare, from a text tab's choices (R8). A list that does
/// not compile counts as empty, as in the text view.
pub fn text_rules_of(prefs: &TextPrefs) -> FolderTextRules {
    FolderTextRules {
        whitespace: prefs.whitespace,
        ignore_case: prefs.ignore_case,
        ignore_blank: prefs.ignore_blank,
        ignore_comments: prefs.ignore_comments,
        text: compile_list(&prefs.rules_text).unwrap_or_default(),
        lines: compile_list(&prefs.rules_lines).unwrap_or_default(),
    }
}

/// The text rules of the next compare: the saved choices with the lists the open text tabs
/// share, which are newer when a write to the file failed (PF22/G1).
pub fn text_rules(cx: &App) -> FolderTextRules {
    let mut prefs = TextPrefs::load(settings::settings_file(cx).as_deref());
    pull_shared(&mut prefs, cx);
    text_rules_of(&prefs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use duet_core::text::diff::WhitespaceMode;

    #[test]
    fn folder_prefs_round_trip_and_default() {
        let none = FolderPrefs::load(None);
        assert_eq!(none, FolderPrefs::default());
        assert_eq!(none.mode, CompareMode::Quick, "C8: Quick stays the default");
        assert_eq!(none.tolerance_secs, 2);
        assert!(none.ignore_dst && !none.format_important && none.odd_scan && none.confirm_copy);
        assert_eq!(none.filter, FolderFilter::Mismatch);
        assert_eq!(none.layout, Layout::Tree);
        assert!(!none.enter_folders);
        none.save(None).unwrap();

        let defaults = FolderPrefs::default().options();
        assert_eq!(defaults.mode, CompareMode::Quick);
        assert_eq!(defaults.quick.tolerance, Duration::from_secs(2));
        assert!(defaults.quick.ignore_dst);

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let every_field = FolderPrefs {
            mode: CompareMode::Rules,
            tolerance_secs: 90,
            ignore_dst: false,
            format_important: true,
            filter: FolderFilter::RightOnly,
            layout: Layout::Flat,
            odd_scan: false,
            confirm_copy: false,
            enter_folders: true,
        };
        every_field.save(Some(&file)).unwrap();
        assert_eq!(FolderPrefs::load(Some(&file)), every_field);
        let options = every_field.options();
        assert_eq!(options.mode, CompareMode::Rules);
        assert_eq!(options.quick.tolerance, Duration::from_secs(90));
        assert!(!options.quick.ignore_dst && options.format_important);
        let mut taken = FolderPrefs::default();
        taken.take_options(&options);
        assert_eq!(
            (taken.mode, taken.tolerance_secs, taken.ignore_dst),
            (CompareMode::Rules, 90, false)
        );
        assert!(taken.format_important);

        std::fs::write(
            &file,
            r#"{"folder.time_tolerance": 90000, "folder.criterion": "bogus", "x": 1}"#,
        )
        .unwrap();
        let loaded = FolderPrefs::load(Some(&file));
        assert_eq!(loaded.tolerance_secs, 86_400, "a long tolerance clamps");
        assert_eq!(
            loaded.mode,
            FolderPrefs::default().mode,
            "a bad key is the default"
        );

        let one = FolderPrefs {
            filter: FolderFilter::Same,
            ..FolderPrefs::default()
        };
        one.save_key(FILTER, Some(&file)).unwrap();
        assert_eq!(settings::get_string(&file, FILTER).as_deref(), Some("same"));
        assert_eq!(
            settings::get_u64(&file, "x"),
            Some(1),
            "other keys are kept"
        );
        assert_eq!(
            settings::get_bool(&file, ODD_SCAN),
            None,
            "only that key was written"
        );
    }

    #[test]
    fn modes_and_layouts_have_keys() {
        for mode in [
            CompareMode::Quick,
            CompareMode::Content,
            CompareMode::Size,
            CompareMode::Rules,
        ] {
            assert_eq!(mode_from_key(mode_key(mode)), Some(mode));
        }
        assert_eq!(mode_from_key("x"), None);
        for layout in [Layout::Tree, Layout::Flat] {
            assert_eq!(Layout::from_key(layout.key()), Some(layout));
        }
        assert_eq!(Layout::from_key("x"), None);
    }

    #[test]
    fn text_rules_follow_the_text_choices_and_skip_bad_lists() {
        let prefs = TextPrefs {
            whitespace: WhitespaceMode::All,
            ignore_case: true,
            ignore_blank: true,
            ignore_comments: true,
            rules_text: "foo\n\n  bar  \n".into(),
            rules_lines: "(".into(),
            ..TextPrefs::default()
        };
        let rules = text_rules_of(&prefs);
        assert_eq!(rules.whitespace, WhitespaceMode::All);
        assert!(rules.ignore_case && rules.ignore_blank && rules.ignore_comments);
        assert_eq!(rules.text.len(), 2);
        assert!(
            rules.lines.is_empty(),
            "a list that does not compile counts as empty"
        );
        let none = text_rules_of(&TextPrefs::default());
        assert!(none.text.is_empty() && none.lines.is_empty());
    }
}
