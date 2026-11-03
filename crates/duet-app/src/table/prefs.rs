//! The options of a table compare, remembered in `settings.json` (R9).

use std::io;
use std::path::Path;

use duet_core::table::Delimiter;
use duet_core::table::canon::CellRules;
use duet_core::table::compare::CompareOptions;
use duet_core::table::nav::Filter;

use crate::settings;

const DELIMITER: &str = "table.delimiter";
const HEADER: &str = "table.header";
const KEYS: &str = "table.keys";
const IGNORE_CASE: &str = "table.ignore_case";
const TRIM: &str = "table.trim";
const NUMBERS: &str = "table.numbers";
const FILTER: &str = "table.filter";
const AUTO: &str = "auto";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TablePrefs {
    /// None detects the delimiter of each file.
    pub delimiter: Option<Delimiter>,
    pub header: bool,
    /// Comma-separated key columns.
    pub keys: String,
    pub ignore_case: bool,
    pub trim: bool,
    pub numbers: bool,
    pub filter: Filter,
}

impl Default for TablePrefs {
    fn default() -> Self {
        Self {
            delimiter: None,
            header: true,
            keys: String::new(),
            ignore_case: false,
            trim: false,
            numbers: false,
            filter: Filter::All,
        }
    }
}

impl TablePrefs {
    /// The saved options; anything missing or unreadable is the default. None (tests) is all
    /// defaults.
    pub fn load(path: Option<&Path>) -> TablePrefs {
        let default = TablePrefs::default();
        let Some(path) = path else {
            return default;
        };
        TablePrefs {
            delimiter: match settings::get_string(path, DELIMITER).as_deref() {
                Some(key) => Delimiter::from_key(key),
                None => default.delimiter,
            },
            header: settings::get_bool(path, HEADER).unwrap_or(default.header),
            keys: settings::get_string(path, KEYS).unwrap_or(default.keys),
            ignore_case: settings::get_bool(path, IGNORE_CASE).unwrap_or(default.ignore_case),
            trim: settings::get_bool(path, TRIM).unwrap_or(default.trim),
            numbers: settings::get_bool(path, NUMBERS).unwrap_or(default.numbers),
            filter: settings::get_string(path, FILTER)
                .and_then(|key| Filter::from_key(&key))
                .unwrap_or(default.filter),
        }
    }

    /// Writes every option; None (tests) writes nothing.
    pub fn save(&self, path: Option<&Path>) -> io::Result<()> {
        let Some(path) = path else {
            return Ok(());
        };
        settings::set_string(path, DELIMITER, self.delimiter.map_or(AUTO, Delimiter::key))?;
        settings::set_bool(path, HEADER, self.header)?;
        settings::set_string(path, KEYS, &self.keys)?;
        settings::set_bool(path, IGNORE_CASE, self.ignore_case)?;
        settings::set_bool(path, TRIM, self.trim)?;
        settings::set_bool(path, NUMBERS, self.numbers)?;
        settings::set_string(path, FILTER, self.filter.key())
    }

    pub fn compare_options(&self) -> CompareOptions {
        CompareOptions {
            header: self.header,
            keys: self.keys.clone(),
            rules: CellRules {
                ignore_case: self.ignore_case,
                trim: self.trim,
                numbers: self.numbers,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_prefs_round_trip_and_default() {
        let default = TablePrefs::load(None);
        assert_eq!(default, TablePrefs::default());
        assert_eq!(default.delimiter, None);
        assert!(default.header);
        assert_eq!(default.keys, "");
        assert!(!default.ignore_case && !default.trim && !default.numbers);
        assert_eq!(default.filter, Filter::All);
        assert!(default.save(None).is_ok(), "nowhere to write is fine");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(TablePrefs::load(Some(&path)), default, "a missing file");
        let prefs = TablePrefs {
            delimiter: Some(Delimiter::Tab),
            header: false,
            keys: "id, 2".into(),
            ignore_case: true,
            trim: true,
            numbers: true,
            filter: Filter::Same,
        };
        prefs.save(Some(&path)).unwrap();
        assert_eq!(TablePrefs::load(Some(&path)), prefs);
        let auto = TablePrefs {
            delimiter: None,
            ..prefs
        };
        auto.save(Some(&path)).unwrap();
        assert_eq!(TablePrefs::load(Some(&path)).delimiter, None);
    }

    #[test]
    fn compare_options_carry_the_prefs() {
        let prefs = TablePrefs {
            header: false,
            keys: "a".into(),
            trim: true,
            ..TablePrefs::default()
        };
        let options = prefs.compare_options();
        assert!(!options.header);
        assert_eq!(options.keys, "a");
        assert!(options.rules.trim && !options.rules.ignore_case && !options.rules.numbers);
    }
}
