//! Core's error sentences and worded labels in the UI's language (task 13).
//!
//! The `thiserror` attributes stay in English: `duet-cli` and the logs read them. Here each
//! variant is said again in the language in force, from the same data, so the path and Windows' own
//! text (`io::Error`'s, the regex crate's) arrive as they are. In English every function gives
//! exactly what `err.to_string()` gives, which `tests/ui_lang/errors.rs` checks variant by variant.
//!
//! Run these on the UI thread, or inside a `LiveText` closure, never on a job's thread: a job has no
//! language of its own.

use std::fmt::Display;
use std::path::Path;

use duet_core::folder::actions::ActionError;
use duet_core::folder::classify::TextFormat;
use duet_core::folder::content::ContentError;
use duet_core::folder::entry::ScanError;
use duet_core::folder::sync::SkipReason;
use duet_core::picture::decode::DecodeError;
use duet_core::text::diff::DiffError;
use duet_core::text::document::LoadError;
use duet_core::text::encoding::EncodeError;
use duet_core::text::merge::ReadError;
use duet_core::text::rules::RuleError;
use duet_core::text::save::SaveError;

use super::{tr, tr_key, tr_str, trf};

fn shown(path: &Path) -> impl Display {
    path.display()
}

/// "operation cancelled", which core's `Cancelled` says.
pub fn cancelled() -> String {
    tr("operation cancelled").to_owned()
}

/// A copy, delete or sync step that failed.
pub fn action_error(err: &ActionError) -> String {
    match err {
        ActionError::WouldOverwrite(path) => trf("{} already exists", &[&shown(path)]),
        ActionError::Io { path, source } => {
            // `plan` writes one fixed sentence into its `io::Error`.
            let source = source.to_string();
            trf("{}: {}", &[&shown(path), &tr_str(&source)])
        }
        ActionError::Trash { path, message } => trf(
            "cannot move {} to the Recycle Bin: {}",
            &[&shown(path), &trash_reason(message)],
        ),
        ActionError::DestinationIsLink(path) => trf(
            "{} is a link or junction; duet never writes through links",
            &[&shown(path)],
        ),
        ActionError::IntoItself { src, dst } => trf(
            "cannot copy {} into itself ({})",
            &[&shown(src), &shown(dst)],
        ),
        ActionError::Changed(_) => tr("Changed since the compare.").to_owned(),
    }
}

/// What the trash crate says when Windows refuses, in its own debug words ("Error during a
/// `trash` operation: Unknown { .. }"). A person reads "Windows would not allow it"; the raw
/// text is in the log (`folder::view` logs it when the move fails). A readable system sentence
/// stays as it is.
fn trash_reason(message: &str) -> String {
    if message.starts_with("Error during a `trash` operation") {
        tr("Windows would not allow it").to_owned()
    } else {
        message.to_owned()
    }
}

/// The text of an `ActionError` that a report already holds as a `String` (`BatchReport::failed`),
/// said in the language in force. A text of no known shape, which is Windows' own, stays as it is.
pub fn action_text(text: &str) -> String {
    if text == tr_key("Changed since the compare.") {
        return tr("Changed since the compare.").to_owned();
    }
    if text == tr_key("one folder is inside the other, so nothing was synced") {
        return tr("one folder is inside the other, so nothing was synced").to_owned();
    }
    if let Some(path) = text.strip_suffix(" already exists") {
        return trf("{} already exists", &[&path]);
    }
    if let Some(path) = text.strip_suffix(" is a link or junction; duet never writes through links")
    {
        return trf(
            "{} is a link or junction; duet never writes through links",
            &[&path],
        );
    }
    if let Some(rest) = text.strip_prefix("cannot move ")
        && let Some((path, message)) = rest.split_once(" to the Recycle Bin: ")
    {
        return trf(
            "cannot move {} to the Recycle Bin: {}",
            &[&path, &trash_reason(message)],
        );
    }
    if let Some(rest) = text.strip_prefix("cannot copy ")
        && let Some((src, dst)) = rest.split_once(" into itself (")
        && let Some(dst) = dst.strip_suffix(')')
    {
        return trf("cannot copy {} into itself ({})", &[&src, &dst]);
    }
    if let Some((path, source)) = text.split_once(": ")
        && let Some(known) = fixed_source(source)
    {
        return trf("{}: {}", &[&path, &known]);
    }
    text.to_owned()
}

/// The one sentence core writes into an `ActionError::Io`.
fn fixed_source(source: &str) -> Option<String> {
    (source == tr_key("outside the compared folder")).then(|| tr_str(source).into_owned())
}

/// A folder scan that failed.
pub fn folder_error(err: &ScanError) -> String {
    match err {
        ScanError::Filter(err) => trf("invalid filter pattern: {}", &[err]),
        ScanError::Root { path, source } => {
            trf("{} is not a readable folder: {}", &[&shown(path), source])
        }
        ScanError::NotADirectory(path) => trf("{} is not a folder", &[&shown(path)]),
        ScanError::Cancelled(_) => cancelled(),
        ScanError::ThreadPool(err) => trf("cannot start worker threads: {}", &[err]),
    }
}

/// A picture that could not be read or decoded.
pub fn decode_error(err: &DecodeError) -> String {
    match err {
        DecodeError::FileTooLarge(megabytes) => trf("the file is larger than {} MB.", &[megabytes]),
        DecodeError::Io(text) => trf("{}.", &[text]),
        DecodeError::Unsupported(what) => trf("{} is not supported.", &[what]),
        DecodeError::Corrupt => {
            tr("the picture can't be decoded: the format is not supported, or the file is damaged.")
                .to_owned()
        }
        DecodeError::Empty => tr("the picture has no size.").to_owned(),
        DecodeError::TooLarge(width, height) => trf(
            "the picture is too large to open ({}×{}).",
            &[width, height],
        ),
        DecodeError::Cancelled => cancelled(),
    }
}

/// A file the merge could not read.
pub fn merge_read_error(err: &ReadError) -> String {
    match err {
        ReadError::FileTooLarge(megabytes) => trf("the file is larger than {} MB.", &[megabytes]),
        ReadError::Binary => tr("it is a binary file").to_owned(),
        ReadError::Io(text) => trf("{}.", &[text]),
        ReadError::Cancelled => cancelled(),
    }
}

/// A text file that could not be loaded.
pub fn read_error(err: &LoadError) -> String {
    match err {
        LoadError::Io { path, source } => trf("cannot read {}: {}", &[&shown(path), source]),
        LoadError::Binary { path } => trf("{} is a binary file", &[&shown(path)]),
        LoadError::TooLargeAsText { path } => trf(
            "{} is binary and over 64 MiB, too large to open as text.",
            &[&shown(path)],
        ),
        LoadError::Cancelled(_) => cancelled(),
    }
}

/// A save that failed.
pub fn save_error(err: &SaveError) -> String {
    match err {
        SaveError::ChangedOnDisk { path } => {
            trf("{} changed on disk since it was loaded", &[&shown(path)])
        }
        SaveError::Encode(err) => encode_error(err),
        SaveError::Io { path, source } => trf("cannot save {}: {}", &[&shown(path), source]),
    }
}

/// Text an encoding cannot hold.
pub fn encode_error(err: &EncodeError) -> String {
    match err {
        EncodeError::Unrepresentable { encoding, position } => trf(
            "text contains a character that {} cannot represent (byte offset {})",
            &[encoding, position],
        ),
    }
}

/// A file compare that could not finish: Windows' own text, or a cancel.
pub fn content_error(err: &ContentError) -> String {
    match err {
        ContentError::Io(err) => err.to_string(),
        ContentError::Cancelled(_) => cancelled(),
    }
}

/// A diff that could not be made.
pub fn diff_error(err: &DiffError) -> String {
    match err {
        DiffError::Cancelled(_) => cancelled(),
        DiffError::TooManyLines(lines) => trf("too many lines to diff ({})", &[lines]),
    }
}

/// A pattern the rules could not compile: the pattern, then why. The reason is the regex
/// crate's own sentence, which stays as it is, except the fallback core writes itself.
pub fn rule_error(err: &RuleError) -> String {
    trf("{}: {}", &[&err.pattern, &tr_str(&err.reason)])
}

/// `TextFormat::describe` in the language in force: the line endings, then what else is unusual,
/// then the encoding's name, which stays as it is.
pub fn describe_format(format: &TextFormat) -> String {
    let mut out = tr(format.eol_text()).to_owned();
    if !format.final_newline {
        out.push_str(tr(TextFormat::NO_FINAL_NEWLINE));
    }
    if format.bom {
        out.push_str(tr(TextFormat::WITH_BOM));
    }
    out.push_str(", ");
    out.push_str(format.encoding.name());
    out
}

/// Why a sync leaves an item alone.
pub fn skip_text(reason: &SkipReason, mode: duet_core::folder::run::CompareMode) -> &'static str {
    tr(reason.text_in(mode))
}
