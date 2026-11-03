//! Small user settings kept in `settings.json` (a JSON object) next to the recent list.

use std::io;
use std::path::{Path, PathBuf};

use gpui_kit::App;

pub const SETTINGS_FILE: &str = "settings.json";

/// `settings.json` in duet's data folder; None when the app has no data folder (tests).
pub fn settings_file(cx: &App) -> Option<PathBuf> {
    cx.try_global::<crate::app::AppPaths>()
        .map(|paths| paths.data_dir.join(SETTINGS_FILE))
}

/// The whole file as a JSON object. A missing or garbled file is an empty object; any other
/// read error is returned, so a transient failure never wipes the keys already saved.
fn read_map(path: &Path) -> io::Result<serde_json::Map<String, serde_json::Value>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_default()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Default::default()),
        Err(err) => Err(err),
    }
}

/// Every key of the file, for a caller that needs several of them: one read and one parse. A
/// missing, garbled or unreadable file gives none.
pub fn read_all(path: &Path) -> serde_json::Map<String, serde_json::Value> {
    read_map(path).unwrap_or_default()
}

pub fn get_u64(path: &Path, key: &str) -> Option<u64> {
    read_map(path).ok()?.get(key)?.as_u64()
}

/// A number: an integer value reads as the same float.
pub fn get_f64(path: &Path, key: &str) -> Option<f64> {
    read_map(path).ok()?.get(key)?.as_f64()
}

pub fn get_bool(path: &Path, key: &str) -> Option<bool> {
    read_map(path).ok()?.get(key)?.as_bool()
}

pub fn get_string(path: &Path, key: &str) -> Option<String> {
    read_map(path).ok()?.get(key)?.as_str().map(str::to_owned)
}

/// Any value, as the file holds it: a list or an object a caller reads itself.
pub fn get_json(path: &Path, key: &str) -> Option<serde_json::Value> {
    read_map(path).ok()?.remove(key)
}

pub fn set_json(path: &Path, key: &str, value: serde_json::Value) -> io::Result<()> {
    set_value(path, key, value)
}

pub fn set_u64(path: &Path, key: &str, value: u64) -> io::Result<()> {
    set_value(path, key, value.into())
}

pub fn set_f64(path: &Path, key: &str, value: f64) -> io::Result<()> {
    // JSON has no NaN or infinity.
    let value = serde_json::Number::from_f64(value)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a finite number"))?;
    set_value(path, key, value.into())
}

pub fn set_bool(path: &Path, key: &str, value: bool) -> io::Result<()> {
    set_value(path, key, value.into())
}

pub fn set_string(path: &Path, key: &str, value: &str) -> io::Result<()> {
    set_value(path, key, value.into())
}

/// Sets one key and keeps the others. Writes a temp file in the same folder, then renames it
/// over the settings.
fn set_value(path: &Path, key: &str, value: serde_json::Value) -> io::Result<()> {
    let mut map = read_map(path)?;
    map.insert(key.to_owned(), value);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(&map).map_err(io::Error::other)?;
    std::fs::write(&temp, text + "\n")?;
    std::fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_keep_unknown_keys_and_survive_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("settings.json");
        assert_eq!(get_u64(&path, "hex.bytes_per_row"), None, "missing file");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"x":1}"#).unwrap();
        set_u64(&path, "hex.bytes_per_row", 32).unwrap();
        assert_eq!(get_u64(&path, "hex.bytes_per_row"), Some(32));
        assert_eq!(get_u64(&path, "x"), Some(1), "unknown keys are kept");
        assert_eq!(get_u64(&path, "y"), None);
        std::fs::write(&path, "not json {{").unwrap();
        assert_eq!(
            get_u64(&path, "hex.bytes_per_row"),
            None,
            "garbage reads as none"
        );
        set_u64(&path, "hex.bytes_per_row", 8).unwrap();
        assert_eq!(
            get_u64(&path, "hex.bytes_per_row"),
            Some(8),
            "writing over garbage works"
        );
        std::fs::write(&path, "[1,2]").unwrap();
        assert_eq!(
            get_u64(&path, "hex.bytes_per_row"),
            None,
            "a non-object reads as none"
        );
        set_u64(&path, "hex.bytes_per_row", 16).unwrap();
        assert_eq!(get_u64(&path, "hex.bytes_per_row"), Some(16));
        assert!(
            !path.with_extension("json.tmp").exists(),
            "no temp file is left"
        );
    }

    #[test]
    fn an_unreadable_settings_file_is_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        // A folder where the file should be: reading it fails, and it is not "missing".
        let path = dir.path().join("settings.json");
        std::fs::create_dir(&path).unwrap();
        assert_eq!(get_u64(&path, "hex.bytes_per_row"), None);
        assert!(set_u64(&path, "hex.bytes_per_row", 32).is_err());
        assert!(path.is_dir(), "nothing replaced it");
        assert!(
            !path.with_extension("json.tmp").exists(),
            "and no temp file was left behind"
        );
    }

    #[test]
    fn settings_hold_bools_and_strings_beside_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        set_bool(&path, "table.header", false).unwrap();
        set_string(&path, "table.keys", "id, name").unwrap();
        set_u64(&path, "hex.bytes_per_row", 8).unwrap();
        assert_eq!(get_bool(&path, "table.header"), Some(false));
        assert_eq!(get_string(&path, "table.keys").as_deref(), Some("id, name"));
        assert_eq!(get_u64(&path, "hex.bytes_per_row"), Some(8));
        assert_eq!(
            get_bool(&path, "table.keys"),
            None,
            "a string is not a bool"
        );
        assert_eq!(
            get_string(&path, "table.header"),
            None,
            "a bool is not a string"
        );
        assert_eq!(get_u64(&path, "table.header"), None);
        assert_eq!(get_bool(&path, "missing"), None);
        assert_eq!(get_string(&path, "missing"), None);
    }

    #[test]
    fn read_all_gives_every_key_in_one_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert!(read_all(&path).is_empty(), "missing file");
        set_bool(&path, "text.wrap", true).unwrap();
        set_u64(&path, "text.tab_width", 8).unwrap();
        let all = read_all(&path);
        assert_eq!(all.len(), 2);
        assert_eq!(all["text.wrap"].as_bool(), Some(true));
        assert_eq!(all["text.tab_width"].as_u64(), Some(8));
        std::fs::write(&path, "not json {{").unwrap();
        assert!(read_all(&path).is_empty(), "garbage");
        let folder = dir.path().join("folder");
        std::fs::create_dir(&folder).unwrap();
        assert!(read_all(&folder).is_empty(), "unreadable");
    }

    #[test]
    fn settings_hold_json_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(get_json(&path, "folder.report_names"), None, "missing file");
        let names = serde_json::json!([{ "pair": "a|b", "names": ["A", "B"] }]);
        set_json(&path, "folder.report_names", names.clone()).unwrap();
        set_bool(&path, "folder.report_mask", false).unwrap();
        assert_eq!(get_json(&path, "folder.report_names"), Some(names));
        assert_eq!(
            get_json(&path, "folder.report_mask"),
            Some(serde_json::Value::Bool(false)),
            "any value reads as JSON"
        );
        assert_eq!(get_json(&path, "missing"), None);
        std::fs::write(&path, "not json {{").unwrap();
        assert_eq!(get_json(&path, "folder.report_names"), None, "garbage");
    }

    #[test]
    fn settings_hold_floats() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        set_f64(&path, "text.font_size", 12.5).unwrap();
        assert_eq!(get_f64(&path, "text.font_size"), Some(12.5));
        set_u64(&path, "text.whole", 16).unwrap();
        assert_eq!(get_f64(&path, "text.whole"), Some(16.0), "an integer reads");
        set_string(&path, "text.word", "big").unwrap();
        assert_eq!(
            get_f64(&path, "text.word"),
            None,
            "a string is not a number"
        );
        assert_eq!(get_f64(&path, "missing"), None);
        assert!(set_f64(&path, "text.bad", f64::NAN).is_err());
    }
}
