//! Folders the app starts from.

use std::path::PathBuf;

/// The user's home folder (`USERPROFILE`, else `HOME`): where a save dialog starts when
/// the file it saves next to has no folder. Empty when neither is set.
pub fn home_dir() -> PathBuf {
    ["USERPROFILE", "HOME"]
        .into_iter()
        .find_map(|var| std::env::var_os(var).filter(|value| !value.is_empty()))
        .map(PathBuf::from)
        .unwrap_or_default()
}
