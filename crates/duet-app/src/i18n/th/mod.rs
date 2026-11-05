//! The UI's Thai, one file per area, in the order the UI reads. `table` merges and sorts them once,
//! at the first Thai lookup, so an area's task never touches another's lines and no file is sorted
//! by hand. The English path never builds it.

use std::sync::LazyLock;

use duet_core::i18n::merged;

pub mod chips;
pub mod common;
pub mod core_text;
pub mod folder;
pub mod hex;
pub mod merge;
pub mod picture;
pub mod shell;
pub mod start;
pub mod table;
pub mod text;

static TABLE: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    merged(&[
        common::ENTRIES,
        shell::ENTRIES,
        start::ENTRIES,
        folder::ENTRIES,
        merge::ENTRIES,
        text::ENTRIES,
        table::ENTRIES,
        hex::ENTRIES,
        picture::ENTRIES,
        core_text::ENTRIES,
        chips::ENTRIES,
    ])
});

/// Every entry of every area, sorted by key.
pub fn table() -> &'static [(&'static str, &'static str)] {
    &TABLE
}
