//! What the status line says and which way a copy goes. No I/O here.

use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;

use duet_core::folder::classify::Warning;
use duet_core::folder::entry::{Entry, EntryKind};
use duet_core::folder::run::{CompareMode, FolderOptions, FolderProgress};
use duet_core::folder::tree::{Node, Side, Status};

use crate::folder::model::{Breakdown, Counts, FolderFilter};
use crate::format::{count_of, format_size};
use crate::status::{StatusPart, Tone};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    LeftToRight,
    RightToLeft,
}

/// Why a row differs, in one word for the middle column. Folders get a count instead.
pub fn reason(node: &Node, mode: CompareMode) -> Option<&'static str> {
    match &node.status {
        Status::Error(_) => Some("error"),
        Status::Similar { .. } if !node.is_dir => {
            Some(if node.note.as_ref().is_some_and(|n| n.soft) {
                "unimportant"
            } else {
                "EOL/BOM"
            })
        }
        Status::Different { .. } if !node.is_dir => {
            let (l, r) = (node.left.as_ref()?, node.right.as_ref()?);
            let note = node.note.as_deref();
            Some(if let Some(warning) = node.warning() {
                match warning {
                    Warning::CaseOnly => "case",
                    Warning::OddSpace => "whitespace",
                    Warning::Script => "script",
                }
            } else if note.is_some_and(|n| n.encoding_differs()) {
                "encoding"
            } else if note.is_some_and(|n| n.format_differs()) {
                "line endings or BOM"
            } else if l.kind != r.kind {
                "type"
            } else if l.kind == EntryKind::Symlink {
                "link"
            } else if l.len != r.len {
                "size"
            } else if mode.reads_contents() {
                "content"
            } else {
                "time"
            })
        }
        _ => None,
    }
}

/// What a case-only pair matches in under `mode`: Quick and Size only never read a file, so
/// they never claim the same contents (PF13).
fn matched_by(mode: CompareMode) -> &'static str {
    match mode {
        CompareMode::Content | CompareMode::Rules => "the contents match",
        CompareMode::Quick => "the size and modified time match",
        CompareMode::Size => "the size matches",
    }
}

/// The status line's note about the selected row.
pub fn selection_text(node: &Node, differing_below: u32, mode: CompareMode) -> String {
    format!(
        "Selected: {} — {}",
        node.rel,
        detail(node, differing_below, mode)
    )
}

/// Why a row differs, or what it is, in the selection note's words: the middle tooltip of a
/// plain Different row says the same (F-04).
fn detail(node: &Node, differing_below: u32, mode: CompareMode) -> String {
    let size = |e: &Option<Entry>| e.as_ref().map_or(0, |e| e.len);
    match (&node.status, reason(node, mode)) {
        (Status::Error(message), _) => message.clone(),
        (_, Some("size")) => format!(
            "size {} → {}",
            format_size(size(&node.left)),
            count_of(size(&node.right), "byte", "bytes")
        ),
        (
            Status::Different {
                newer: Some(Side::Left),
            },
            Some("time"),
        ) => "same size, left is newer".into(),
        (
            Status::Different {
                newer: Some(Side::Right),
            },
            Some("time"),
        ) => "same size, right is newer".into(),
        (_, Some("time")) => "same size, times differ".into(),
        (_, Some("content")) => "same size, contents differ".into(),
        (_, Some("EOL/BOM")) => {
            "same text; only the line endings, final newline or BOM differ".into()
        }
        (_, Some("unimportant")) => "differs only in unimportant parts".into(),
        (_, Some("case")) => format!("the names differ only in case; {}", matched_by(mode)),
        (_, Some("whitespace")) => {
            "only the kind of whitespace or invisible characters differ".into()
        }
        (_, Some("script")) => "a script with CRLF or a BOM".into(),
        (_, Some("encoding")) => "the same text in another encoding".into(),
        (_, Some("line endings or BOM")) => {
            "only the line endings, final newline or BOM differ".into()
        }
        (_, Some("type")) => "a file on one side, a folder or link on the other".into(),
        (_, Some("link")) => "the links point to different targets".into(),
        _ if node.is_dir && differing_below > 0 => count_of(
            u64::from(differing_below),
            "differing item inside",
            "differing items inside",
        ),
        (Status::LeftOnly, _) => "only on the left".into(),
        (Status::RightOnly, _) => "only on the right".into(),
        (Status::Pending, _) => "still comparing".into(),
        _ => "same".into(),
    }
}

/// dirCountTitle (js:549-554) in English.
pub fn pill_tip(filter: FolderFilter, searching: bool, b: Breakdown) -> String {
    let n = |count: u32| format_size(u64::from(count));
    let total = u64::from(b.different) + u64::from(b.left_only) + u64::from(b.right_only);
    format!(
        "Files inside that match the “{}” filter{} (files, not lines).\nAll {} in this folder: {} different · {} left only · {} right only · {} same",
        filter.chip_label(),
        if searching {
            " and the path filter"
        } else {
            ""
        },
        count_of(total + u64::from(b.same), "file", "files"),
        n(b.different),
        n(b.left_only),
        n(b.right_only),
        n(b.same),
    )
}

/// The middle glyph's tooltip (`stLabel`, `js:596-609`); None for a folder, whose tip stays
/// "N differing below". `time_differs`: the modified times differ beyond the rule.
pub fn row_tip(node: &Node, time_differs: bool, mode: CompareMode) -> Option<String> {
    if node.is_dir {
        return None;
    }
    if let Status::Error(message) = &node.status {
        return Some(message.clone());
    }
    let (left, right) = (node.left.as_ref(), node.right.as_ref());
    let spelling = |side: Option<&Entry>| side.map_or("", |e| e.rel.as_str()).to_owned();
    let note = node.note.as_deref();
    let formats = note.and_then(|n| n.formats);
    let sides = formats
        .map(|[a, b]| format!("\nLeft: {}\nRight: {}", a.describe(), b.describe()))
        .unwrap_or_default();
    let body = if note.is_some_and(|n| n.soft) {
        "differs only in unimportant parts and the file format"
    } else {
        "every line matches; only the file format differs"
    };
    let tip = match (&node.status, node.warning()) {
        (_, Some(Warning::CaseOnly)) => format!(
            "⚠ The names differ only in case (left: {} · right: {}) and {}.\nOn Windows they are one file; on Linux they are two, and a program that uses the old name may not find it.",
            spelling(left),
            spelling(right),
            matched_by(mode)
        ),
        (_, Some(Warning::OddSpace)) => "⚠ Only the kind of whitespace or invisible characters differ: the files look the same, but the values do not (for example NBSP for a space, LRM or zero-width). Open the pair to see where.".to_owned(),
        (_, Some(Warning::Script)) => format!(
            "⚠ A script with CRLF or a BOM may not run on Linux (for example \"/bin/bash^M: bad interpreter\") — {body}{sides}"
        ),
        (Status::Different { .. }, None) if note.is_some_and(|n| n.encoding_differs()) => format!(
            "The text matches but the encoding differs, so a program may read Thai wrongly — {body}{sides}"
        ),
        (Status::Similar { .. }, None) if note.is_some_and(|n| n.soft) => {
            format!("Differs only in unimportant parts{sides}")
        }
        (Status::Similar { .. }, None) => format!("Unimportant: {body}{sides}"),
        (Status::Different { .. }, None) if formats.is_some() => format!("{body}{sides}"),
        (status, None) => {
            let mut tip = match status {
                Status::Same if time_differs && mode.reads_contents() => {
                    "Same: the contents match, but the modified times differ".into()
                }
                Status::Same if time_differs => "Same size, but the modified times differ".into(),
                // F-04: the reason, as the selection note gives it, and the newer side unless
                // the reason already names it.
                Status::Different { newer } => {
                    let newer = match (newer, reason(node, mode)) {
                        (_, Some("time")) | (None, _) => "",
                        (Some(Side::Left), _) => " (left is newer)",
                        (Some(Side::Right), _) => " (right is newer)",
                    };
                    format!("Different{newer}: {}", detail(node, 0, mode))
                }
                Status::LeftOnly => "Left only".into(),
                Status::RightOnly => "Right only".into(),
                Status::Pending => "Still comparing".into(),
                Status::Same | Status::Similar { .. } | Status::Error(_) => "Same".into(),
            };
            if node.case_differs() {
                tip.push_str(&format!(
                    "\nThe names differ only in case: left {} · right {}",
                    spelling(left),
                    spelling(right)
                ));
            }
            tip
        }
    };
    Some(tip)
}

/// Include/exclude masks as typed: separated by semicolons or commas.
pub fn parse_masks(text: &str) -> Vec<String> {
    text.split([';', ','])
        .map(str::trim)
        .filter(|mask| !mask.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The status line while a compare runs.
pub fn progress_text(progress: &FolderProgress) -> String {
    let total = progress.content.total.load(Relaxed);
    if total > 0 {
        let done = progress.content.done.load(Relaxed);
        format!(
            "Comparing contents… {} of {} · Esc to cancel",
            format_size(done),
            count_of(total, "file", "files")
        )
    } else {
        let items = progress.left.entries.load(Relaxed) + progress.right.entries.load(Relaxed);
        format!(
            "Scanning… {} · Esc to cancel",
            count_of(items, "item", "items")
        )
    }
}

/// What the invisible-character scan adds to the status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OddStatus {
    /// Switched off, or no result to scan yet.
    Off,
    Running,
    /// Files with a hit in the text or values, files with hits only in comments or indentation.
    Done {
        warn: usize,
        info: usize,
    },
    /// A compare of this many files, too many to scan by itself.
    HeldBack(usize),
}

/// The status line as coloured parts: the counts, the active filter and the compare options, so
/// an empty list explains itself. A finished compare ends with the time it took.
pub fn status_parts(
    counts: &Counts,
    filter: FolderFilter,
    opts: &FolderOptions,
    done: bool,
    took: Option<Duration>,
    odd: OddStatus,
) -> Vec<StatusPart> {
    let muted = |text: String| StatusPart::new(text, Tone::Muted);
    let mut parts = vec![
        muted(format!(
            "Left: {}, {}",
            count_of(counts.left_files as u64, "file", "files"),
            count_of(counts.left_bytes, "byte", "bytes")
        )),
        muted(format!(
            "Right: {}, {}",
            count_of(counts.right_files as u64, "file", "files"),
            count_of(counts.right_bytes, "byte", "bytes")
        )),
        StatusPart::new(format!("Different {}", counts.different), Tone::Changed),
    ];
    parts.extend([
        StatusPart::new(format!("Left only {}", counts.left_only), Tone::LeftOnly),
        StatusPart::new(format!("Right only {}", counts.right_only), Tone::RightOnly),
        muted(format!("Same {}", counts.same)),
    ]);
    let time_label = if opts.mode.reads_contents() {
        "Same contents, other time"
    } else {
        "Same size, other time"
    };
    for (count, text, tone) in [
        (counts.soft, "≈ unimportant", Tone::Unimportant),
        (
            counts.format_only,
            "≈ line endings or BOM only",
            Tone::Unimportant,
        ),
        (counts.time_only, time_label, Tone::Moved),
        (counts.script, "⚠ CRLF or BOM in scripts", Tone::LeftOnly),
        (counts.odd_space, "⚠ whitespace kind only", Tone::LeftOnly),
        (counts.case_names, "⚠ names differ in case", Tone::LeftOnly),
    ] {
        if count > 0 {
            parts.push(StatusPart::new(format!("{text} {count}"), tone));
        }
    }
    if let OddStatus::Done { warn, info } = odd {
        if warn > 0 {
            parts.push(StatusPart::new(
                format!(
                    "⚠ invisible characters in {}",
                    count_of(warn as u64, "file", "files")
                ),
                Tone::LeftOnly,
            ));
        }
        if info > 0 {
            parts.push(muted(format!(
                "invisible characters in comments or indentation in {}",
                count_of(info as u64, "file", "files")
            )));
        }
    }
    if counts.errors > 0 {
        parts.push(StatusPart::new(
            format!("Errors {}", counts.errors),
            Tone::LeftOnly,
        ));
    }
    if !done || counts.pending > 0 {
        parts.push(muted("Comparing…".to_owned()));
    }
    parts.push(muted(format!("Showing: {}", filter.label())));
    let mode = match opts.mode {
        CompareMode::Quick => {
            let mut text = format!("Quick (±{} s", opts.quick.tolerance.as_secs());
            if opts.quick.ignore_dst {
                text.push_str(", ignore 1 h DST");
            }
            text.push(')');
            text
        }
        CompareMode::Content => "Content".to_owned(),
        CompareMode::Size => "Size only".to_owned(),
        CompareMode::Rules => "Content with rules".to_owned(),
    };
    parts.push(muted(mode));
    if !opts.filters.include.is_empty() {
        parts.push(muted(format!(
            "Include {}",
            opts.filters.include.join("; ")
        )));
    }
    if !opts.filters.exclude.is_empty() {
        parts.push(muted(format!(
            "Exclude {}",
            opts.filters.exclude.join("; ")
        )));
    }
    if let (true, Some(took)) = (done, took) {
        parts.push(muted(format!("{:.2} s", took.as_secs_f64())));
    }
    match odd {
        OddStatus::Running => parts.push(muted("Looking for invisible characters…".to_owned())),
        OddStatus::HeldBack(files) => parts.push(muted(format!(
            "Not looking for invisible characters in {}",
            count_of(files as u64, "file", "files")
        ))),
        OddStatus::Off | OddStatus::Done { .. } => {}
    }
    parts
}

/// What an empty folder list says: why it is empty, and what to do about it when anything
/// helps. `name_filter` is whether the name filter is on; `masks` whether include or exclude
/// masks are set.
pub fn empty_state(
    counts: &Counts,
    filter: FolderFilter,
    name_filter: bool,
    masks: bool,
) -> (&'static str, Option<&'static str>) {
    const CHOOSE_ALL: Option<&str> = Some("Choose All to see every file.");
    if counts.all == 0 {
        if masks {
            (
                "No files match the include and exclude masks.",
                Some("Clear the masks to see every file."),
            )
        } else {
            ("Both folders are empty.", None)
        }
    } else if name_filter {
        (
            "No names match the filter.",
            Some("Clear the name filter to see every file."),
        )
    } else if filter == FolderFilter::Mismatch
        && FolderFilter::Mismatch.count(counts) == 0
        && counts.similar > 0
    {
        (
            "The folders are identical except for unimportant differences (≈).",
            Some(
                "Choose All to see every file, or turn on \"Count line-ending and BOM differences\" to list them as changes.",
            ),
        )
    } else if filter == FolderFilter::Mismatch && FolderFilter::Mismatch.count(counts) == 0 {
        ("The folders are identical.", CHOOSE_ALL)
    } else {
        ("No rows match this filter.", CHOOSE_ALL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::join_parts;
    use duet_core::folder::classify::{Note, TextFormat, Warning};
    use duet_core::folder::entry::{Entry, EntryKind, Filters};
    use duet_core::folder::tree::{QuickRule, Status};
    use duet_core::text::lines::EolStyle;
    use std::time::Duration;

    #[test]
    fn pill_tips_give_the_breakdown() {
        let b = Breakdown {
            different: 1,
            left_only: 1,
            right_only: 0,
            same: 2,
        };
        assert_eq!(
            pill_tip(FolderFilter::Mismatch, false, b),
            "Files inside that match the “Mismatched” filter (files, not lines).\nAll 4 files in this folder: 1 different · 1 left only · 0 right only · 2 same"
        );
        assert_eq!(
            pill_tip(FolderFilter::Same, true, b),
            "Files inside that match the “Same” filter and the path filter (files, not lines).\nAll 4 files in this folder: 1 different · 1 left only · 0 right only · 2 same"
        );
        let one = Breakdown {
            same: 1,
            ..Breakdown::default()
        };
        assert!(pill_tip(FolderFilter::All, false, one).contains("All 1 file in this folder"));
    }

    /// The status parts as the one line the view shows.
    fn status_text(
        counts: &Counts,
        filter: FolderFilter,
        opts: &FolderOptions,
        done: bool,
        took: Option<Duration>,
    ) -> String {
        join_parts(&status_parts(
            counts,
            filter,
            opts,
            done,
            took,
            OddStatus::Off,
        ))
    }

    fn entry(rel: &str) -> Entry {
        Entry {
            rel: rel.into(),
            kind: EntryKind::File,
            len: 1,
            modified: None,
            error: None,
            link_target: None,
        }
    }

    fn node(left: Option<&str>, right: Option<&str>) -> Node {
        Node {
            name: "x".into(),
            rel: left.or(right).unwrap().into(),
            depth: 1,
            parent: None,
            is_dir: false,
            left: left.map(entry),
            right: right.map(entry),
            status: Status::Same,
            note: None,
        }
    }

    #[test]
    fn masks_split_on_semicolons_and_commas() {
        assert_eq!(
            parse_masks(" *.rs; *.toml ,target;; "),
            ["*.rs", "*.toml", "target"]
        );
        assert!(parse_masks("  ").is_empty());
    }

    #[test]
    fn progress_text_counts_scanned_items_then_content_checks() {
        use std::sync::atomic::Ordering::Relaxed;
        let progress = FolderProgress::default();
        progress.left.entries.store(1_200, Relaxed);
        progress.right.entries.store(34, Relaxed);
        assert_eq!(
            progress_text(&progress),
            "Scanning… 1,234 items · Esc to cancel"
        );
        progress.content.total.store(20, Relaxed);
        progress.content.done.store(5, Relaxed);
        assert_eq!(
            progress_text(&progress),
            "Comparing contents… 5 of 20 files · Esc to cancel"
        );
    }

    #[test]
    fn counts_of_one_read_in_the_singular() {
        let one = Counts {
            all: 1,
            different: 1,
            left_files: 1,
            right_files: 0,
            left_bytes: 1,
            right_bytes: 0,
            ..Counts::default()
        };
        let text = status_text(
            &one,
            FolderFilter::All,
            &FolderOptions::default(),
            true,
            None,
        );
        assert!(
            text.starts_with("Left: 1 file, 1 byte · Right: 0 files, 0 bytes ·"),
            "{text}"
        );
        let progress = FolderProgress::default();
        progress
            .left
            .entries
            .store(1, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(progress_text(&progress), "Scanning… 1 item · Esc to cancel");
        progress
            .content
            .total
            .store(1, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            progress_text(&progress),
            "Comparing contents… 0 of 1 file · Esc to cancel"
        );
        let mut n = node(Some("a.txt"), Some("a.txt"));
        n.status = Status::Different { newer: None };
        n.left.as_mut().unwrap().len = 5;
        assert_eq!(
            selection_text(&n, 0, CompareMode::Quick),
            "Selected: a.txt — size 5 → 1 byte"
        );
    }

    #[test]
    fn reasons_name_what_differs() {
        let mut n = node(Some("a.txt"), Some("a.txt"));
        n.status = Status::Different { newer: None };
        assert_eq!(reason(&n, CompareMode::Quick), Some("time"));
        assert_eq!(reason(&n, CompareMode::Content), Some("content"));
        n.right.as_mut().unwrap().len = 5;
        assert_eq!(reason(&n, CompareMode::Quick), Some("size"));
        assert_eq!(
            selection_text(&n, 0, CompareMode::Quick),
            "Selected: a.txt — size 1 → 5 bytes"
        );
        n.status = Status::Error("Access is denied. (os error 5)".into());
        assert_eq!(reason(&n, CompareMode::Quick), Some("error"));
        assert_eq!(
            selection_text(&n, 0, CompareMode::Quick),
            "Selected: a.txt — Access is denied. (os error 5)"
        );
        let mut dir = node(Some("src"), Some("src"));
        dir.is_dir = true;
        dir.status = Status::Different { newer: None };
        assert_eq!(
            reason(&dir, CompareMode::Quick),
            None,
            "folders show a count, not a reason"
        );
        assert_eq!(
            selection_text(&dir, 1, CompareMode::Quick),
            "Selected: src — 1 differing item inside"
        );
    }

    #[test]
    fn an_empty_list_says_why_and_what_to_do() {
        let same = Counts {
            all: 2,
            same: 2,
            ..Counts::default()
        };
        let none = Counts::default();
        let choose_all = Some("Choose All to see every file.");
        assert_eq!(
            empty_state(&same, FolderFilter::Mismatch, false, false),
            ("The folders are identical.", choose_all)
        );
        assert_eq!(
            empty_state(&same, FolderFilter::Different, false, false),
            ("No rows match this filter.", choose_all)
        );
        assert_eq!(
            empty_state(&same, FolderFilter::Mismatch, true, false),
            (
                "No names match the filter.",
                Some("Clear the name filter to see every file.")
            ),
            "the name filter hid everything"
        );
        assert_eq!(
            empty_state(&none, FolderFilter::All, false, false),
            ("Both folders are empty.", None)
        );
        assert_eq!(
            empty_state(&none, FolderFilter::Mismatch, false, false),
            ("Both folders are empty.", None),
            "whatever the filter"
        );
        assert_eq!(
            empty_state(&none, FolderFilter::All, false, true),
            (
                "No files match the include and exclude masks.",
                Some("Clear the masks to see every file.")
            ),
            "masks can leave nothing, which does not make the folders empty"
        );
    }

    #[test]
    fn status_text_names_filter_mode_and_masks() {
        let counts = Counts {
            all: 4,
            different: 1,
            left_only: 1,
            right_only: 0,
            same: 2,
            errors: 0,
            pending: 0,
            left_files: 4,
            right_files: 3,
            left_bytes: 1_234,
            right_bytes: 99,
            ..Counts::default()
        };
        let opts = FolderOptions {
            quick: QuickRule {
                tolerance: Duration::from_secs(2),
                ignore_dst: true,
            },
            filters: Filters {
                include: vec!["*.rs".into()],
                exclude: vec!["target".into()],
            },
            ..FolderOptions::default()
        };
        assert_eq!(
            status_text(&counts, FolderFilter::Mismatch, &opts, true, None),
            "Left: 4 files, 1,234 bytes · Right: 3 files, 99 bytes · Different 1 · Left only 1 · Right only 0 · Same 2 · Showing: Mismatches · Quick (±2 s, ignore 1 h DST) · Include *.rs · Exclude target"
        );
        assert!(
            status_text(
                &counts,
                FolderFilter::All,
                &FolderOptions::default(),
                false,
                None
            )
            .contains("Comparing…")
        );
    }

    #[test]
    fn a_finished_compare_ends_with_the_time_it_took() {
        let counts = Counts {
            different: 1,
            left_only: 2,
            ..Counts::default()
        };
        let opts = FolderOptions::default();
        let text = status_text(
            &counts,
            FolderFilter::All,
            &opts,
            true,
            Some(Duration::from_millis(20)),
        );
        assert!(text.ends_with(" · 0.02 s"), "{text}");
        let parts = status_parts(
            &counts,
            FolderFilter::All,
            &opts,
            true,
            None,
            OddStatus::Off,
        );
        let tone = |text: &str| parts.iter().find(|p| p.text == text).map(|p| p.tone);
        assert_eq!(tone("Different 1"), Some(Tone::Changed));
        assert_eq!(tone("Left only 2"), Some(Tone::LeftOnly));
        assert_eq!(tone("Right only 0"), Some(Tone::RightOnly));
        let running = status_text(
            &counts,
            FolderFilter::All,
            &opts,
            false,
            Some(Duration::from_millis(20)),
        );
        assert!(
            !running.ends_with("0.02 s"),
            "a running compare has no time yet: {running}"
        );
    }

    fn fmt(eol: EolStyle, bom: bool) -> TextFormat {
        TextFormat {
            eol,
            final_newline: true,
            bom,
            encoding: encoding_rs::UTF_8,
        }
    }

    /// A file pair with a note: its status, the two formats, the soft flag and the warning.
    fn noted(
        status: Status,
        formats: Option<[TextFormat; 2]>,
        soft: bool,
        warning: Option<Warning>,
    ) -> Node {
        let mut n = node(Some("a.txt"), Some("a.txt"));
        n.status = status;
        n.note = Some(Box::new(Note {
            formats,
            shebang: false,
            soft,
            warning,
        }));
        n
    }

    trait With {
        fn with(self, status: Status) -> Node;
    }

    impl With for Node {
        fn with(mut self, status: Status) -> Node {
            self.status = status;
            self
        }
    }

    const CONTENT: CompareMode = CompareMode::Content;

    #[test]
    fn row_tips_explain_each_case() {
        let (lf, crlf) = (fmt(EolStyle::Lf, false), fmt(EolStyle::CrLf, false));
        let both = Some([lf, crlf]);
        let (a, b) = (lf.describe(), crlf.describe());
        let diff = Status::Different { newer: None };
        let similar = Status::Similar { newer: None };
        let body = "every line matches; only the file format differs";
        let soft_body = "differs only in unimportant parts and the file format";
        let tip = |n: &Node| row_tip(n, false, CONTENT).unwrap();

        let mut case = noted(diff.clone(), None, false, Some(Warning::CaseOnly));
        case.left.as_mut().unwrap().rel = "Config.xml".into();
        case.right.as_mut().unwrap().rel = "config.xml".into();
        assert_eq!(
            tip(&case),
            "⚠ The names differ only in case (left: Config.xml · right: config.xml) and the contents match.\nOn Windows they are one file; on Linux they are two, and a program that uses the old name may not find it."
        );
        assert_eq!(
            tip(&noted(diff.clone(), both, false, Some(Warning::OddSpace))),
            "⚠ Only the kind of whitespace or invisible characters differ: the files look the same, but the values do not (for example NBSP for a space, LRM or zero-width). Open the pair to see where."
        );
        let script = noted(diff.clone(), both, false, Some(Warning::Script));
        assert_eq!(
            tip(&script),
            format!(
                "⚠ A script with CRLF or a BOM may not run on Linux (for example \"/bin/bash^M: bad interpreter\") — {body}\nLeft: {a}\nRight: {b}"
            )
        );
        let mut soft_script = script.clone();
        soft_script.note.as_mut().unwrap().soft = true;
        assert!(tip(&soft_script).contains(&format!("— {soft_body}\nLeft: {a}")));

        let other = TextFormat {
            encoding: encoding_rs::WINDOWS_874,
            ..lf
        };
        let encoded = noted(diff.clone(), Some([lf, other]), false, None);
        assert_eq!(
            tip(&encoded),
            format!(
                "The text matches but the encoding differs, so a program may read Thai wrongly — {body}\nLeft: {}\nRight: {}",
                lf.describe(),
                other.describe()
            )
        );
        assert_eq!(
            tip(&noted(similar.clone(), both, false, None)),
            format!("Unimportant: {body}\nLeft: {a}\nRight: {b}")
        );
        assert_eq!(
            tip(&noted(similar, both, true, None)),
            format!("Differs only in unimportant parts\nLeft: {a}\nRight: {b}")
        );
        assert_eq!(
            tip(&noted(diff.clone(), both, false, None)),
            format!("{body}\nLeft: {a}\nRight: {b}"),
            "a difference the format toggle made important"
        );

        let mut same = node(Some("a.txt"), Some("a.txt"));
        assert_eq!(tip(&same), "Same");
        assert_eq!(
            row_tip(&same, true, CONTENT).unwrap(),
            "Same: the contents match, but the modified times differ"
        );
        assert_eq!(
            row_tip(&same, true, CompareMode::Size).unwrap(),
            "Same size, but the modified times differ",
            "Size only and Quick never claim equal contents"
        );
        same.status = Status::Different {
            newer: Some(Side::Left),
        };
        assert_eq!(
            tip(&same),
            "Different (left is newer): same size, contents differ"
        );
        same.status = Status::Different {
            newer: Some(Side::Right),
        };
        assert_eq!(
            tip(&same),
            "Different (right is newer): same size, contents differ"
        );
        same.status = diff;
        assert_eq!(tip(&same), "Different: same size, contents differ");
        same.left.as_mut().unwrap().rel = "A.txt".into();
        assert_eq!(
            tip(&same),
            "Different: same size, contents differ\nThe names differ only in case: left A.txt · right a.txt"
        );
        assert_eq!(
            tip(&node(Some("x"), None).with(Status::LeftOnly)),
            "Left only"
        );
        assert_eq!(
            tip(&node(None, Some("x")).with(Status::RightOnly)),
            "Right only"
        );
        assert_eq!(
            tip(&node(Some("x"), Some("x")).with(Status::Error("Access is denied.".into()))),
            "Access is denied."
        );
        let mut dir = node(Some("src"), Some("src"));
        dir.is_dir = true;
        assert_eq!(
            row_tip(&dir, false, CONTENT),
            None,
            "a folder keeps its count tip"
        );
    }

    /// F-04: the middle tooltip of a plain Different row carries its reason in the selection
    /// note's words; a known newer side is named, unless the reason already names it.
    #[test]
    fn a_different_rows_tip_carries_its_reason() {
        let newer = |side| Status::Different { newer: side };
        let mut n = node(Some("a.txt"), Some("a.txt")).with(newer(None));
        let both = |n: &Node, mode| (row_tip(n, false, mode).unwrap(), selection_text(n, 0, mode));
        assert_eq!(
            both(&n, CONTENT),
            (
                "Different: same size, contents differ".to_owned(),
                "Selected: a.txt — same size, contents differ".to_owned()
            )
        );
        assert_eq!(
            row_tip(&n, false, CompareMode::Quick).unwrap(),
            "Different: same size, times differ"
        );
        n.status = newer(Some(Side::Left));
        assert_eq!(
            row_tip(&n, false, CompareMode::Quick).unwrap(),
            "Different: same size, left is newer",
            "the reason already names the newer side"
        );
        n.status = newer(Some(Side::Right));
        assert_eq!(
            row_tip(&n, false, CompareMode::Quick).unwrap(),
            "Different: same size, right is newer"
        );
        n.right.as_mut().unwrap().len = 5;
        assert_eq!(
            both(&n, CONTENT),
            (
                "Different (right is newer): size 1 → 5 bytes".to_owned(),
                "Selected: a.txt — size 1 → 5 bytes".to_owned()
            )
        );
        n.right.as_mut().unwrap().kind = EntryKind::Dir;
        assert_eq!(
            row_tip(&n, false, CONTENT).unwrap(),
            "Different (right is newer): a file on one side, a folder or link on the other"
        );
        let mut link = node(Some("j"), Some("j")).with(newer(None));
        for side in [&mut link.left, &mut link.right] {
            side.as_mut().unwrap().kind = EntryKind::Symlink;
        }
        assert_eq!(
            both(&link, CONTENT),
            (
                "Different: the links point to different targets".to_owned(),
                "Selected: j — the links point to different targets".to_owned()
            )
        );
    }

    #[test]
    fn the_new_verdicts_have_reasons_and_selection_text() {
        let (lf, crlf) = (fmt(EolStyle::Lf, false), fmt(EolStyle::CrLf, false));
        let both = Some([lf, crlf]);
        let diff = Status::Different { newer: None };
        let similar = Status::Similar { newer: None };
        let other = TextFormat {
            encoding: encoding_rs::WINDOWS_874,
            ..lf
        };
        let cases = [
            (
                noted(similar.clone(), both, false, None),
                "EOL/BOM",
                "same text; only the line endings, final newline or BOM differ",
            ),
            (
                noted(similar, both, true, None),
                "unimportant",
                "differs only in unimportant parts",
            ),
            (
                noted(diff.clone(), None, false, Some(Warning::CaseOnly)),
                "case",
                "the names differ only in case; the contents match",
            ),
            (
                noted(diff.clone(), both, false, Some(Warning::OddSpace)),
                "whitespace",
                "only the kind of whitespace or invisible characters differ",
            ),
            (
                noted(diff.clone(), both, false, Some(Warning::Script)),
                "script",
                "a script with CRLF or a BOM",
            ),
            (
                noted(diff.clone(), Some([lf, other]), false, None),
                "encoding",
                "the same text in another encoding",
            ),
            (
                noted(diff, both, false, None),
                "line endings or BOM",
                "only the line endings, final newline or BOM differ",
            ),
        ];
        for (n, word, text) in cases {
            assert_eq!(reason(&n, CONTENT), Some(word));
            assert_eq!(
                selection_text(&n, 0, CONTENT),
                format!("Selected: a.txt — {text}")
            );
        }
    }

    /// M-10 (PF13): Quick and Size only never read a file, so a case-only pair is said to match
    /// in what they compared.
    #[test]
    fn case_only_texts_never_claim_the_contents_in_quick_or_size_only() {
        let diff = Status::Different { newer: None };
        let mut case = noted(diff, None, false, Some(Warning::CaseOnly));
        case.left.as_mut().unwrap().rel = "Config.xml".into();
        case.right.as_mut().unwrap().rel = "config.xml".into();
        let linux = "\nOn Windows they are one file; on Linux they are two, and a program that uses the old name may not find it.";
        for (mode, matched) in [
            (CompareMode::Content, "the contents match"),
            (CompareMode::Rules, "the contents match"),
            (CompareMode::Quick, "the size and modified time match"),
            (CompareMode::Size, "the size matches"),
        ] {
            assert_eq!(
                row_tip(&case, false, mode).unwrap(),
                format!(
                    "⚠ The names differ only in case (left: Config.xml · right: config.xml) and {matched}.{linux}"
                ),
                "{mode:?}"
            );
            assert_eq!(
                selection_text(&case, 0, mode),
                format!("Selected: a.txt — the names differ only in case; {matched}"),
                "{mode:?}"
            );
        }
    }

    #[test]
    fn the_status_line_counts_each_kind_of_difference() {
        let counts = Counts {
            all: 10,
            same: 4,
            soft: 1,
            format_only: 2,
            time_only: 3,
            script: 1,
            odd_space: 1,
            case_names: 2,
            ..Counts::default()
        };
        let content = FolderOptions {
            mode: CompareMode::Content,
            ..FolderOptions::default()
        };
        let parts = status_parts(
            &counts,
            FolderFilter::All,
            &content,
            true,
            None,
            OddStatus::Off,
        );
        let at = |text: &str| parts.iter().position(|p| p.text == text).unwrap();
        let wanted = [
            ("Same 4", Tone::Muted),
            ("≈ unimportant 1", Tone::Unimportant),
            ("≈ line endings or BOM only 2", Tone::Unimportant),
            ("Same contents, other time 3", Tone::Moved),
            ("⚠ CRLF or BOM in scripts 1", Tone::LeftOnly),
            ("⚠ whitespace kind only 1", Tone::LeftOnly),
            ("⚠ names differ in case 2", Tone::LeftOnly),
        ];
        for (i, (text, tone)) in wanted.iter().enumerate() {
            assert_eq!(parts[at(wanted[0].0) + i].text, *text);
            assert_eq!(parts[at(text)].tone, *tone, "{text}");
        }
        assert_eq!(
            parts[at("⚠ names differ in case 2") + 1].text,
            "Showing: All"
        );
        let text = status_text(&Counts::default(), FolderFilter::All, &content, true, None);
        for gone in ["≈", "other time", "⚠", "text-equal"] {
            assert!(
                !text.contains(gone),
                "{gone} appears with zero counts: {text}"
            );
        }
        let quick = status_text(
            &counts,
            FolderFilter::All,
            &FolderOptions::default(),
            true,
            None,
        );
        assert!(quick.contains("Same size, other time 3"), "{quick}");
        assert!(!quick.contains("Same contents"), "{quick}");
    }

    #[test]
    fn an_empty_list_of_only_similar_files_points_at_the_format_toggle() {
        let counts = Counts {
            all: 2,
            same: 1,
            similar: 1,
            format_only: 1,
            ..Counts::default()
        };
        let (title, hint) = empty_state(&counts, FolderFilter::Mismatch, false, false);
        assert_eq!(
            title,
            "The folders are identical except for unimportant differences (≈)."
        );
        assert!(
            hint.is_some_and(|h| h.contains("line-ending and BOM")),
            "{hint:?}"
        );
    }
}
