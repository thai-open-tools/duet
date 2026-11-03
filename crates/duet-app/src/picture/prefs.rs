//! The options of a picture compare, remembered in `settings.json` (R9).

use std::io;
use std::path::Path;

use crate::settings;

const MODE: &str = "picture.mode";
const TOLERANCE: &str = "picture.tolerance";

/// How the two pictures are shown (LocalDiff's four modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    SideBySide,
    Difference,
    Slider,
    Blink,
}

impl Mode {
    pub const ALL: [Mode; 4] = [
        Mode::SideBySide,
        Mode::Difference,
        Mode::Slider,
        Mode::Blink,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Mode::SideBySide => "side",
            Mode::Difference => "diff",
            Mode::Slider => "slider",
            Mode::Blink => "blink",
        }
    }

    pub fn from_key(key: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|mode| mode.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::SideBySide => "Side by side",
            Mode::Difference => "Difference",
            Mode::Slider => "Slider",
            Mode::Blink => "Blink",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PicturePrefs {
    pub mode: Mode,
    /// 0 to 255: a pixel whose largest channel difference is above this counts.
    pub tolerance: u8,
}

impl PicturePrefs {
    /// The saved options; anything missing or unreadable is the default, a tolerance above 255
    /// reads as 255. None (tests) is all defaults.
    pub fn load(path: Option<&Path>) -> PicturePrefs {
        let default = PicturePrefs::default();
        let Some(path) = path else {
            return default;
        };
        PicturePrefs {
            mode: settings::get_string(path, MODE)
                .and_then(|key| Mode::from_key(&key))
                .unwrap_or(default.mode),
            tolerance: settings::get_u64(path, TOLERANCE)
                .map_or(default.tolerance, |value| value.min(255) as u8),
        }
    }

    /// Writes every option; None (tests) writes nothing.
    pub fn save(&self, path: Option<&Path>) -> io::Result<()> {
        let Some(path) = path else {
            return Ok(());
        };
        settings::set_string(path, MODE, self.mode.key())?;
        settings::set_u64(path, TOLERANCE, u64::from(self.tolerance))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picture_prefs_round_trip_and_default() {
        let default = PicturePrefs::load(None);
        assert_eq!(
            default,
            PicturePrefs {
                mode: Mode::SideBySide,
                tolerance: 0
            }
        );
        assert!(default.save(None).is_ok(), "nowhere to write is fine");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(PicturePrefs::load(Some(&path)), default, "a missing file");
        let prefs = PicturePrefs {
            mode: Mode::Blink,
            tolerance: 37,
        };
        prefs.save(Some(&path)).unwrap();
        assert_eq!(PicturePrefs::load(Some(&path)), prefs);

        std::fs::write(
            &path,
            r#"{"picture.tolerance": 999, "picture.mode": "bogus"}"#,
        )
        .unwrap();
        assert_eq!(
            PicturePrefs::load(Some(&path)),
            PicturePrefs {
                mode: Mode::SideBySide,
                tolerance: 255
            }
        );
    }

    #[test]
    fn the_modes_have_keys_and_labels() {
        let keys: Vec<_> = Mode::ALL.iter().map(|m| m.key()).collect();
        assert_eq!(keys, ["side", "diff", "slider", "blink"]);
        let labels: Vec<_> = Mode::ALL.iter().map(|m| m.label()).collect();
        assert_eq!(labels, ["Side by side", "Difference", "Slider", "Blink"]);
        for mode in Mode::ALL {
            assert_eq!(Mode::from_key(mode.key()), Some(mode));
        }
        assert_eq!(Mode::from_key("nope"), None);
    }
}
