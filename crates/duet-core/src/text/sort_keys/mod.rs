//! Sort keys (Plan D): a JSON or YAML text turned into the same data with its keys sorted at
//! every level and one indentation, so two files that differ only in key order compare equal.
//! Nothing a parser would change is changed: JSON numbers and strings are copied as written, and a
//! YAML scalar keeps its text and its style (`no` stays different from `"no"`).

use std::path::Path;

use crate::cancel::{CancelToken, Cancelled};
use crate::i18n::tr_key;
use crate::text::lexer::{Language, detect_language};

mod json;
mod yaml;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod yaml_tests;

/// A side over this size is left as it is: the sort builds a tree of about 50 bytes a node.
pub const SORT_LIMIT: usize = 16 * 1024 * 1024;
/// More nodes than this (an alias bomb, a huge file) is refused.
pub const MAX_NODES: usize = 2_000_000;
/// Deeper nesting is refused, as Format leaves it alone.
pub(crate) const MAX_DEPTH: usize = super::format::MAX_DEPTH;
/// Sorted text past this many bytes (a deep file with many items prints a long indent for each) is
/// refused while it is printed.
pub(crate) const MAX_OUTPUT: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKind {
    Json,
    Yaml,
}

impl SortKind {
    pub fn label(self) -> &'static str {
        match self {
            SortKind::Json => "JSON",
            SortKind::Yaml => "YAML",
        }
    }
}

/// JSON or YAML by the file's extension; else JSON when the text starts with `{` or `[` (after a
/// BOM and white space), as Format's probe does. None otherwise.
pub fn sort_kind_for(path: &Path, text: &str) -> Option<SortKind> {
    match detect_language(&path.to_string_lossy()) {
        Language::Json => return Some(SortKind::Json),
        Language::Yaml => return Some(SortKind::Yaml),
        _ => {}
    }
    let probe = text.trim_start_matches('\u{feff}').trim_start();
    matches!(probe.chars().next()?, '{' | '[').then_some(SortKind::Json)
}

/// What the sort noticed; the app turns each into a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortNote {
    /// The same key twice in one mapping: `key` as the second one is written (JSON keeps its
    /// quotes), `line` the line of the file it is on. Both entries are kept, in source order.
    DuplicateKey {
        key: String,
        line: usize,
    },
    CommentsDropped,
    AnchorsExpanded,
    /// A YAML file with this many documents (more than one), each sorted.
    Documents(usize),
    /// A `%YAML major.minor` directive, which the sorted text does not carry.
    YamlVersion(u32, u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sorted {
    pub kind: SortKind,
    pub text: String,
    pub notes: Vec<SortNote>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortError {
    /// Over `SORT_LIMIT`.
    TooLarge,
    /// The text does not parse. `line` and `column` start at 1. `message` is the parser's
    /// sentence: for JSON one of this module's keys (the app translates it), for YAML the
    /// parser's own English.
    Syntax {
        line: usize,
        column: usize,
        message: String,
    },
    /// YAML that has no sorted form here (a key that is not a scalar).
    Unsupported(String),
    TooDeep,
    /// More than `MAX_NODES` nodes, an output over `MAX_OUTPUT` bytes, or more comments in a row
    /// than the YAML parser buffers.
    TooBig,
    Cancelled,
}

impl From<Cancelled> for SortError {
    fn from(_: Cancelled) -> Self {
        SortError::Cancelled
    }
}

pub fn sort_keys(text: &str, kind: SortKind, cancel: &CancelToken) -> Result<Sorted, SortError> {
    if text.len() > SORT_LIMIT {
        return Err(SortError::TooLarge);
    }
    cancel.check()?;
    match kind {
        SortKind::Json => json::sort(text, cancel),
        SortKind::Yaml => yaml::sort(text, cancel),
    }
}

/// The sentences of the JSON scan (keys of the Thai table).
pub(crate) mod message {
    use super::tr_key;
    pub const EXPECTED_VALUE: &str = tr_key("expected a value");
    pub const EXPECTED_KEY: &str = tr_key("expected a key");
    pub const EXPECTED_COLON: &str = tr_key("expected a colon");
    pub const EXPECTED_COMMA: &str = tr_key("expected a comma or a closing bracket");
    pub const TRAILING_BRACE: &str = tr_key("unexpected comma before closing brace");
    pub const TRAILING_BRACKET: &str = tr_key("unexpected comma before closing bracket");
    pub const MISMATCHED: &str = tr_key("mismatched closing bracket");
    pub const UNTERMINATED: &str = tr_key("unterminated string");
    pub const UNEXPECTED_CHAR: &str = tr_key("unexpected character");
    pub const EXTRA: &str = tr_key("extra data after the root value");
    pub const UNEXPECTED_END: &str = tr_key("unexpected end of the text");
    pub const RECURSIVE_ALIAS: &str =
        tr_key("an alias refers to a collection that is not finished");
    pub const UNKNOWN_ALIAS: &str = tr_key("an alias refers to an anchor that does not exist");
    pub const COMPLEX_KEYS: &str = tr_key("complex mapping keys");
}

/// Line breaks before byte offsets that only grow: a CRLF or a lone CR or LF counts once.
pub(crate) struct LineCounter<'a> {
    bytes: &'a [u8],
    at: usize,
    line: usize,
}

impl<'a> LineCounter<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            bytes: text.as_bytes(),
            at: 0,
            line: 1,
        }
    }

    /// The 1-based line of the byte `offset`, which must not be before the last one asked.
    pub fn line_at(&mut self, offset: usize) -> usize {
        let end = offset.min(self.bytes.len());
        while self.at < end {
            match self.bytes[self.at] {
                b'\n' => self.line += 1,
                b'\r' if self.bytes.get(self.at + 1) != Some(&b'\n') => self.line += 1,
                _ => {}
            }
            self.at += 1;
        }
        self.line
    }
}
