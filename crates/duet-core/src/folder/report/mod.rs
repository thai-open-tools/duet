//! Folder compare reports: the items a report lists, their counts, groups and "needs a closer
//! look" kinds, the names of the two sides and the secret masking (LocalDiff's `31-folder-report.js`,
//! `rep:N`). Writers for each format build on these.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime};

use crate::cancel::CancelToken;
use crate::folder::classify::{Note, Warning};
use crate::folder::entry::{Entry, EntryKind, join_rel};
use crate::folder::odd_scan::OddMarks;
use crate::folder::run::{CompareMode, FolderTextRules};
use crate::folder::tree::{FolderComparison, Node, Side, Status, newer_side};
use crate::text::diff::WhitespaceMode;
use crate::text::lines::EolStyle;

pub mod csv;
pub mod html;
pub mod lines;
pub mod mask;
pub mod text;
pub mod xlsx;
pub mod zip;

#[cfg(test)]
mod test_data;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemState {
    Different,
    LeftOnly,
    RightOnly,
    Same,
}

/// What one side holds for an item: its own spelling of the path, where it is on disk and how
/// big and how old it is. `kind` lets a reader refuse a link or a special file (PF3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideFacts {
    pub rel: String,
    pub path: PathBuf,
    pub kind: EntryKind,
    pub len: u64,
    pub modified: Option<NaiveDateTime>,
}

/// One file row of the compare (R2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportItem {
    pub node: u32,
    pub rel: String,
    pub state: ItemState,
    pub left: Option<SideFacts>,
    pub right: Option<SideFacts>,
    pub newer: Option<Side>,
    pub error: Option<String>,
    pub note: Option<Note>,
    pub similar: bool,
    /// The odd scan found a hit in the text on that side.
    pub odd_text: [bool; 2],
    /// The outermost folder above it that exists on its side only.
    pub whole_folder: Option<String>,
}

impl ReportItem {
    /// `rptIsUnimp`: counted as the same, but not byte for byte.
    pub fn unimportant(&self) -> bool {
        self.state == ItemState::Same && self.similar
    }

    /// R4: a differing pair whose lines are worth reading.
    pub fn needs_lines(&self) -> bool {
        self.state == ItemState::Different
            && self.error.is_none()
            && self.left.is_some()
            && self.right.is_some()
            && !self.case_only()
            && !self.format_only()
    }

    /// Different with formats, not soft, and no OddSpace or CaseOnly warning: only the file
    /// format (line endings, BOM, encoding) differs.
    pub fn format_only(&self) -> bool {
        self.state == ItemState::Different
            && self.note.as_ref().is_some_and(|note| {
                note.formats.is_some()
                    && !note.soft
                    && !matches!(note.warning, Some(Warning::OddSpace | Warning::CaseOnly))
            })
    }

    /// The note says the names differ only in case and the contents are the same (PF10).
    pub fn case_only(&self) -> bool {
        self.note
            .as_ref()
            .is_some_and(|note| note.warning == Some(Warning::CaseOnly))
    }

    /// Both sides exist and spell the path differently (`caseDiff`, PF10).
    pub fn case_names_differ(&self) -> bool {
        matches!((&self.left, &self.right), (Some(l), Some(r)) if l.rel != r.rel)
    }
}

/// The one node predicate of every scope (PF13): some side holds the row as something other
/// than a folder, so a folder against a file is an item, as in `plan::expand`.
pub fn is_report_file(node: &Node) -> bool {
    crate::folder::plan::file_like(node)
}

/// Items for `nodes` (file leaves, in the order given). Local times through `chrono::Local`.
/// A folder on both sides is not an item; a folder against a file is, as in `plan::expand`.
pub fn report_items(
    cmp: &FolderComparison,
    nodes: &[u32],
    left_root: &Path,
    right_root: &Path,
    odd: Option<&OddMarks>,
) -> Vec<ReportItem> {
    let facts = |entry: &Entry, root: &Path| SideFacts {
        rel: entry.rel.clone(),
        path: join_rel(root, &entry.rel),
        kind: entry.kind,
        len: entry.len,
        modified: entry
            .modified
            .map(|time| DateTime::<Local>::from(time).naive_local()),
    };
    nodes
        .iter()
        .filter_map(|&index| {
            let node = cmp.nodes.get(index as usize)?;
            if !is_report_file(node) {
                return None;
            }
            let (state, mut newer, error, similar) = match &node.status {
                Status::Same => (ItemState::Same, None, None, false),
                Status::Similar { newer } => (ItemState::Same, *newer, None, true),
                Status::Different { newer } => (ItemState::Different, *newer, None, false),
                Status::LeftOnly => (ItemState::LeftOnly, None, None, false),
                Status::RightOnly => (ItemState::RightOnly, None, None, false),
                // An unfinished row is not a verdict; the export refuses an unfinished result
                // (PF4), so this only keeps the report honest if one slips through.
                Status::Pending => (
                    ItemState::Different,
                    None,
                    Some("Not compared yet.".to_owned()),
                    false,
                ),
                Status::Error(message) => {
                    (ItemState::Different, None, Some(message.clone()), false)
                }
            };
            if state == ItemState::Same
                && !similar
                && let (Some(l), Some(r)) = (&node.left, &node.right)
            {
                newer = newer_side(l, r, cmp.quick);
            }
            let mut whole_folder = None;
            let mut at = node.parent;
            while let Some(parent) = at {
                let parent = &cmp.nodes[parent as usize];
                if parent.is_dir && matches!(parent.status, Status::LeftOnly | Status::RightOnly) {
                    whole_folder = Some(parent.rel.clone());
                }
                at = parent.parent;
            }
            let odd_text = odd.map_or([false; 2], |marks| {
                [true, false].map(|left| marks.get(index, left).is_some_and(|scan| scan.warn > 0))
            });
            Some(ReportItem {
                node: index,
                rel: node.rel.clone(),
                state,
                left: node.left.as_ref().map(|e| facts(e, left_root)),
                right: node.right.as_ref().map(|e| facts(e, right_root)),
                newer,
                error,
                note: node.note.as_deref().cloned(),
                similar,
                odd_text,
                whole_folder,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReportCounts {
    pub total: usize,
    pub different: usize,
    pub left_only: usize,
    pub right_only: usize,
    pub same: usize,
    pub unimportant: usize,
    pub left_files: usize,
    pub right_files: usize,
    pub left_bytes: u64,
    pub right_bytes: u64,
}

impl ReportCounts {
    pub fn mismatched(&self) -> usize {
        self.different + self.left_only + self.right_only
    }
}

/// `rptCount`.
pub fn report_counts(items: &[ReportItem]) -> ReportCounts {
    let mut counts = ReportCounts {
        total: items.len(),
        ..ReportCounts::default()
    };
    for item in items {
        match item.state {
            ItemState::Different => counts.different += 1,
            ItemState::LeftOnly => counts.left_only += 1,
            ItemState::RightOnly => counts.right_only += 1,
            ItemState::Same => counts.same += 1,
        }
        if item.unimportant() {
            counts.unimportant += 1;
        }
        for (side, files, bytes) in [
            (&item.left, &mut counts.left_files, &mut counts.left_bytes),
            (
                &item.right,
                &mut counts.right_files,
                &mut counts.right_bytes,
            ),
        ] {
            if let Some(side) = side.as_ref().filter(|s| s.kind != EntryKind::Dir) {
                *files += 1;
                *bytes += side.len;
            }
        }
    }
    counts
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Group {
    pub folder: String,
    pub different: usize,
    pub left_only: usize,
    pub right_only: usize,
    pub same: usize,
}

/// `rptTopGroups` (`rep:96-112`): the depth (1 to 3) that gives at least three groups, but not
/// more than about 25.
pub fn top_groups(items: &[ReportItem]) -> Vec<Group> {
    let mut best: Vec<Group> = Vec::new();
    for depth in 1..=3 {
        let mut groups: Vec<Group> = Vec::new();
        let mut slot: HashMap<String, usize> = HashMap::new();
        for item in items {
            let dir = item.rel.rfind('/').map_or("", |at| &item.rel[..at]);
            let key = dir.split('/').take(depth).collect::<Vec<_>>().join("/");
            let at = *slot.entry(key.clone()).or_insert_with(|| {
                groups.push(Group {
                    folder: key,
                    ..Group::default()
                });
                groups.len() - 1
            });
            let group = &mut groups[at];
            match item.state {
                ItemState::Different => group.different += 1,
                ItemState::LeftOnly => group.left_only += 1,
                ItemState::RightOnly => group.right_only += 1,
                ItemState::Same => group.same += 1,
            }
        }
        if depth > 1 && groups.len() > 25 {
            break;
        }
        best = groups;
        if best.len() >= 3 {
            break;
        }
    }
    best
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attention {
    Script,
    Encoding,
    OddSpace,
    Invisible,
    CaseOnly,
    Unreadable,
}

impl Attention {
    pub const ALL: [Attention; 6] = [
        Attention::Script,
        Attention::Encoding,
        Attention::OddSpace,
        Attention::Invisible,
        Attention::CaseOnly,
        Attention::Unreadable,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Attention::Script => "Linux scripts with CRLF or a BOM",
            Attention::Encoding => "Encodings differ",
            Attention::OddSpace => "Differs only in the kind of whitespace or invisible characters",
            Attention::Invisible => "Invisible characters in the text or values",
            Attention::CaseOnly => "File names that differ only in case",
            Attention::Unreadable => "Files that could not be read",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Attention::Script => {
                "They may not run on Linux (for example \"/bin/bash^M: bad interpreter\"). Convert them to LF without a BOM."
            }
            Attention::Encoding => {
                "The text matches but the encodings differ: a program that reads it may show Thai wrongly."
            }
            Attention::OddSpace => {
                "They look the same but the values differ (for example NBSP for a space, LRM, zero-width): this often breaks a config value without anyone noticing."
            }
            Attention::Invisible => {
                "They may change a value without anyone noticing. Open the file in duet and use the button in the status bar to visit each one."
            }
            Attention::CaseOnly => {
                "On Windows they are one file, on Linux two: a program that uses the old name may not find it."
            }
            Attention::Unreadable => {
                "No permission, or the file is locked: the contents were not compared."
            }
        }
    }
}

/// `rptAttention`: what deserves a closer look about this item, in LocalDiff's order.
pub fn attention(item: &ReportItem, names: &[String; 2]) -> Vec<(Attention, Option<String>)> {
    let mut out = Vec::new();
    let note = item.note.as_ref();
    if note.is_some_and(|n| n.warning == Some(Warning::Script)) {
        out.push((Attention::Script, None));
    }
    if let Some([a, b]) = note
        .filter(|n| n.encoding_differs())
        .and_then(|n| n.formats)
    {
        out.push((
            Attention::Encoding,
            Some(format!("{} / {}", a.encoding.name(), b.encoding.name())),
        ));
    }
    if note.is_some_and(|n| n.warning == Some(Warning::OddSpace)) {
        out.push((Attention::OddSpace, None));
    } else {
        let sides: Vec<&str> = (0..2)
            .filter(|&side| item.odd_text[side])
            .map(|side| names[side].as_str())
            .collect();
        if !sides.is_empty() {
            out.push((Attention::Invisible, Some(sides.join(", "))));
        }
    }
    if item.case_names_differ()
        && let (Some(l), Some(r)) = (&item.left, &item.right)
    {
        let base = |rel: &str| rel.rsplit('/').next().unwrap_or(rel).to_owned();
        out.push((
            Attention::CaseOnly,
            Some(format!(
                "{}: {} · {}: {}",
                names[0],
                base(&l.rel),
                names[1],
                base(&r.rel)
            )),
        ));
    }
    if item.error.is_some() {
        out.push((Attention::Unreadable, None));
    }
    out
}

/// `rptUnimpReason`: why an unimportant item counts as the same.
pub fn unimportant_reason(item: &ReportItem) -> Option<String> {
    if !item.unimportant() {
        return None;
    }
    let note = item.note.as_ref();
    if !note.is_some_and(|n| n.soft)
        && let Some([a, b]) = note.and_then(|n| n.formats)
    {
        let mut parts = Vec::new();
        if a.eol != b.eol {
            parts.push(format!("EOL {} / {}", eol_label(a.eol), eol_label(b.eol)));
        }
        if a.final_newline != b.final_newline {
            parts.push("final newline".to_owned());
        }
        if a.bom != b.bom {
            parts.push("BOM".to_owned());
        }
        let what = if parts.is_empty() {
            String::new()
        } else {
            format!(" ({})", parts.join(", "))
        };
        return Some(format!(
            "Only the file format differs{what} — every line matches"
        ));
    }
    let format = note.is_some_and(|n| n.format_differs());
    Some(format!(
        "Differs only in unimportant parts by the text compare rules{}",
        if format { " and the file format" } else { "" }
    ))
}

fn eol_label(eol: EolStyle) -> &'static str {
    match eol {
        EolStyle::CrLf => "CRLF",
        EolStyle::Lf => "LF",
        EolStyle::Cr => "CR",
        EolStyle::Mixed => "mixed",
        EolStyle::None => "none",
    }
}

/// The two sides' names in the report, from the folders' own names (`rptDefaultNames`).
pub fn default_names(left_root: &Path, right_root: &Path) -> [String; 2] {
    fn base(path: &Path) -> String {
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned()
    }
    fn parent_base(path: &Path) -> String {
        path.parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
    let (a, b) = (base(left_root), base(right_root));
    if a.to_lowercase() != b.to_lowercase() {
        return [a, b];
    }
    // Two folders with the same name (config and config): the parent folder tells them apart.
    let (a2, b2) = (parent_base(left_root), parent_base(right_root));
    if !a2.is_empty() && !b2.is_empty() && a2 != b2 {
        [format!("{a2}/{a}"), format!("{b2}/{b}")]
    } else {
        [format!("{a} (left)"), format!("{b} (right)")]
    }
}

/// `folder-compare_{A}_vs_{B}_{YYYYMMDD}.{ext}` (R10). In each name a run of `\ / : * ? " < > |`
/// or whitespace becomes one `_`, and the name is cut to 40 characters.
pub fn report_file_name(names: &[String; 2], date: NaiveDate, ext: &str) -> String {
    fn safe(name: &str) -> String {
        let mut out = String::new();
        let mut gap = false;
        for c in name.chars() {
            if c.is_whitespace() || "\\/:*?\"<>|".contains(c) {
                gap = true;
            } else {
                if gap {
                    out.push('_');
                    gap = false;
                }
                out.push(c);
            }
        }
        if gap {
            out.push('_');
        }
        out.chars().take(40).collect()
    }
    format!(
        "folder-compare_{}_vs_{}_{}.{ext}",
        safe(&names[0]),
        safe(&names[1]),
        date.format("%Y%m%d")
    )
}

/// `rptPct`.
pub fn percent(n: usize, total: usize) -> String {
    if total == 0 {
        "0%".to_owned()
    } else if n > 0 && n * 100 < total {
        "<1%".to_owned()
    } else {
        // Rounded half up, as JavaScript's Math.round.
        format!("{}%", (n * 200 + total) / (2 * total))
    }
}

/// A count and its noun, with thousands separators (the app's `format::count_of`).
pub fn count_text(n: u64, one: &str, many: &str) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 8);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out.push(' ');
    out.push_str(if n == 1 { one } else { many });
    out
}

/// "64 B", "1.5 KiB", "4.0 GiB" (the app's `format::human_size`).
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.;
    let mut unit = 0;
    while (value * 10.).round() >= 10240. && unit + 1 < UNITS.len() {
        value /= 1024.;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Different, Left only, Right only, Same; then the lower-cased path.
pub fn sort_items(items: &mut [ReportItem]) {
    items.sort_by_cached_key(|item| {
        let rank = match item.state {
            ItemState::Different => 0,
            ItemState::LeftOnly => 1,
            ItemState::RightOnly => 2,
            ItemState::Same => 3,
        };
        (rank, item.rel.to_lowercase())
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    Html,
    Xlsx,
    Csv,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportScope {
    All,
    Filtered,
    Selected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportOptions {
    pub names: [String; 2],
    pub title: String,
    pub note: String,
    pub scope: ReportScope,
    pub details: bool,
    pub context: u32,
    pub mask: bool,
    pub unimportant: bool,
    pub same: bool,
}

/// What the page header and the Summary sheet say about the compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportContext {
    pub side_paths: [String; 2],
    pub criterion: String,
    pub scope: String,
    pub include: String,
    pub exclude: String,
    pub rules: Option<String>,
    pub generated: NaiveDateTime,
    pub generated_utc: String,
    pub version: String,
    pub reads_contents: bool,
    pub tab_width: usize,
    /// The invisible-character scan has run; when false the report says "not scanned" (PF12).
    pub invisible_scan: bool,
}

/// PF12: what every format adds to "not scanned" (I-1).
pub const NOT_SCANNED_HINT: &str = "run the scan in the folder tab first";

pub fn criterion_text(mode: CompareMode) -> &'static str {
    match mode {
        CompareMode::Content => "File contents (modified times not counted)",
        CompareMode::Rules => "Contents by the text compare rules",
        CompareMode::Size => "File size only (contents not read)",
        CompareMode::Quick => "Modified time and size (contents not read)",
    }
}

/// Writes `bytes` to `path` whole or not at all (PF9): a temp file in the target's folder,
/// written in pieces and synced, then renamed over the target, unless `cancel` was set by then.
/// A link is written through to its target, as a text save does. `Ok(false)` when cancelled;
/// the temp file is removed whenever the rename does not happen.
pub fn write_atomic(path: &Path, bytes: &[u8], cancel: &CancelToken) -> io::Result<bool> {
    const PIECE: usize = 1 << 20;
    let target = match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => fs::canonicalize(path)?,
        _ => path.to_path_buf(),
    };
    let dir = target
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    for piece in bytes.chunks(PIECE) {
        if cancel.is_cancelled() {
            return Ok(false);
        }
        temp.write_all(piece)?;
    }
    temp.as_file().sync_all()?;
    if cancel.is_cancelled() {
        return Ok(false);
    }
    temp.persist(&target).map_err(|err| err.error)?;
    Ok(true)
}

/// The text compare rules in words, for a Rules-mode report.
pub fn rules_text(rules: &FolderTextRules) -> String {
    let mut parts: Vec<String> = Vec::new();
    match rules.whitespace {
        WhitespaceMode::None => {}
        WhitespaceMode::Trailing => parts.push("Ignore trailing whitespace".into()),
        WhitespaceMode::Leading => parts.push("Ignore leading whitespace".into()),
        WhitespaceMode::Edges => parts.push("Ignore whitespace at both ends".into()),
        WhitespaceMode::Amount => parts.push("Ignore the amount of whitespace".into()),
        WhitespaceMode::All => parts.push("Ignore all whitespace".into()),
    }
    if rules.ignore_case {
        parts.push("Ignore case".into());
    }
    if rules.ignore_blank {
        parts.push("Ignore blank lines".into());
    }
    if rules.ignore_comments {
        parts.push("Ignore comments".into());
    }
    let patterns = (rules.text.len() + rules.lines.len()) as u64;
    if patterns > 0 {
        parts.push(count_text(
            patterns,
            "unimportant-text rule",
            "unimportant-text rules",
        ));
    }
    if parts.is_empty() {
        "Every character counts".to_owned()
    } else {
        parts.join(", ")
    }
}
