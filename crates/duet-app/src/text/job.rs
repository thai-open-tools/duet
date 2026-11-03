//! Loads both files and diffs them off the UI thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use duet_core::cancel::CancelToken;
use duet_core::folder::content::files_equal;
use duet_core::text::diff::{DiffInput, DiffOptions, WhitespaceMode, count_soft_pairs_checked};
use duet_core::text::document::{LoadError, TextDocument};
use duet_core::text::format::auto_format;
use duet_core::text::moves::{MOVE_LIMIT, Moves, detect_moves};
use duet_core::text::rules::{RULES_MAX_BYTES, TextRules, diff_with_rules};
use encoding_rs::{Encoding, GBK, SHIFT_JIS, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_874, WINDOWS_1252};

use crate::text::model::TextModel;

#[derive(Debug, Clone)]
pub struct TextRequest {
    pub left: PathBuf,
    pub right: PathBuf,
    pub left_encoding: Option<&'static Encoding>,
    pub right_encoding: Option<&'static Encoding>,
    pub options: DiffOptions,
    /// Blank lines, comments, text and line rules (R4); the view rebuilds them before every
    /// compare.
    pub rules: TextRules,
    /// Mark lines that moved (LocalDiff's default: on).
    pub detect_moves: bool,
    /// Pretty-print JSON or XML on both sides before comparing (read-only).
    pub format: bool,
    /// Decode a file with NUL bytes as text too (Open as text); it lasts until new paths load.
    pub force_text: bool,
}

impl TextRequest {
    pub fn new(left: PathBuf, right: PathBuf) -> Self {
        Self {
            left,
            right,
            left_encoding: None,
            right_encoding: None,
            options: DiffOptions::default(),
            rules: TextRules::default(),
            detect_moves: true,
            format: false,
            force_text: false,
        }
    }

    /// An ignore option or a rule is on, so a row the diff matched may still differ.
    pub fn ignoring(&self) -> bool {
        self.ignoring_options() || self.rules.is_active()
    }

    /// Whitespace, case or line endings are ignored.
    fn ignoring_options(&self) -> bool {
        let o = self.options;
        o.whitespace != WhitespaceMode::None || o.ignore_case || o.ignore_eol
    }
}

/// The status note when a side is too large for the rules (R4).
pub const RULES_SKIPPED: &str = "Rules not applied: a file is over 64 MiB.";

pub enum TextOutcome {
    Ready(Box<TextModel>),
    Binary {
        identical: bool,
    },
    /// Open as text was asked for a binary file over `EDIT_LIMIT`.
    TooLargeAsText,
    Failed(String),
}

/// The per-side "read as" choices; `None` means detect automatically.
pub const ENCODINGS: [(&str, Option<&'static Encoding>); 8] = [
    ("Auto", None),
    ("UTF-8", Some(UTF_8)),
    ("Thai (windows-874)", Some(WINDOWS_874)),
    ("UTF-16LE", Some(UTF_16LE)),
    ("UTF-16BE", Some(UTF_16BE)),
    ("Western (windows-1252)", Some(WINDOWS_1252)),
    ("Japanese (Shift_JIS)", Some(SHIFT_JIS)),
    ("Chinese (GBK)", Some(GBK)),
];

pub fn next_encoding(current: Option<&'static Encoding>) -> Option<&'static Encoding> {
    let at = ENCODINGS
        .iter()
        .position(|(_, e)| *e == current)
        .unwrap_or(0);
    ENCODINGS[(at + 1) % ENCODINGS.len()].1
}

pub fn encoding_label(current: Option<&'static Encoding>) -> &'static str {
    ENCODINGS
        .iter()
        .find(|(_, e)| *e == current)
        .map_or("Auto", |(label, _)| *label)
}

/// The encoding button's text: the chosen encoding, or what auto-detection found.
pub fn encoding_text(
    chosen: Option<&'static Encoding>,
    detected: Option<&'static Encoding>,
) -> String {
    match (chosen, detected) {
        (Some(chosen), _) => encoding_label(Some(chosen)).to_owned(),
        (None, Some(found)) => format!("{} (auto)", found.name()),
        (None, None) => "Auto".to_owned(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoreChoice {
    WhitespaceEdges,
    WhitespaceAll,
    Case,
    LineEndings,
}

/// Toggles one ignore option; the two whitespace modes exclude each other.
pub fn apply_ignore(mut options: DiffOptions, choice: IgnoreChoice) -> DiffOptions {
    let whitespace = |current, wanted| {
        if current == wanted {
            WhitespaceMode::None
        } else {
            wanted
        }
    };
    match choice {
        IgnoreChoice::WhitespaceEdges => {
            options.whitespace = whitespace(options.whitespace, WhitespaceMode::Edges)
        }
        IgnoreChoice::WhitespaceAll => {
            options.whitespace = whitespace(options.whitespace, WhitespaceMode::All)
        }
        IgnoreChoice::Case => options.ignore_case = !options.ignore_case,
        IgnoreChoice::LineEndings => options.ignore_eol = !options.ignore_eol,
    }
    options
}

/// One side for the Format button: pretty-printed when it is JSON or XML, as is otherwise.
fn format_side(doc: TextDocument, side: &str, notes: &mut Vec<String>) -> TextDocument {
    let Some(text) = doc.text() else {
        notes.push(format!("{side}: over 64 MiB, shown as is"));
        return doc;
    };
    match auto_format(text) {
        Some(formatted) => {
            notes.push(format!("{side}: formatted {}", formatted.kind.label()));
            doc.formatted(formatted.text)
        }
        None => {
            notes.push(format!("{side}: not JSON or XML, shown as is"));
            doc
        }
    }
}

fn open(
    path: &Path,
    encoding: Option<&'static Encoding>,
    force_text: bool,
    cancel: &CancelToken,
) -> Result<TextDocument, LoadError> {
    // An empty path is an untitled side (R15): nothing to read.
    if path.as_os_str().is_empty() {
        return Ok(TextDocument::untitled());
    }
    match (force_text, encoding) {
        (true, _) => TextDocument::open_as_text(path, encoding, cancel),
        (false, Some(encoding)) => TextDocument::open_with_encoding(path, encoding, cancel),
        (false, None) => TextDocument::open(path, cancel),
    }
}

pub fn run(request: &TextRequest, cancel: &CancelToken) -> TextOutcome {
    // Both sides load at once: loading is mostly page-ins and line indexing.
    let (left, right) = std::thread::scope(|scope| {
        let force = request.force_text;
        let right =
            scope.spawn(move || open(&request.right, request.right_encoding, force, cancel));
        let left = open(&request.left, request.left_encoding, force, cancel);
        let right = right
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        (left, right)
    });
    let (left, right) = match (left, right) {
        (Ok(l), Ok(r)) => (l, r),
        // A missing or unreadable side explains itself (with its path) before any binary fallback.
        (Err(err @ LoadError::Io { .. }), _) | (_, Err(err @ LoadError::Io { .. })) => {
            return TextOutcome::Failed(err.to_string());
        }
        (Err(LoadError::TooLargeAsText { .. }), _) | (_, Err(LoadError::TooLargeAsText { .. })) => {
            return TextOutcome::TooLargeAsText;
        }
        (Err(LoadError::Binary { .. }), _) | (_, Err(LoadError::Binary { .. })) => {
            return match files_equal(&request.left, &request.right, cancel) {
                Ok(identical) => TextOutcome::Binary { identical },
                Err(err) => TextOutcome::Failed(err.to_string()),
            };
        }
        (Err(err), _) | (_, Err(err)) => return TextOutcome::Failed(err.to_string()),
    };
    let mut notes = Vec::new();
    let (left, right) = if request.format {
        (
            format_side(left, "Left", &mut notes),
            format_side(right, "Right", &mut notes),
        )
    } else {
        (left, right)
    };
    match rediff(Arc::new(left), Arc::new(right), request, cancel) {
        TextOutcome::Ready(model) => {
            // The format notes first, then the compare's own (PF14).
            notes.extend_from_slice(model.notes());
            TextOutcome::Ready(Box::new(model.with_notes(notes)))
        }
        other => other,
    }
}

/// Diffs two documents that are already loaded (an edit's snapshot) and finds moves. With no
/// rule set this is `diff_lines` (R4); a side too large for the rules gets a note instead.
pub fn rediff(
    left: Arc<TextDocument>,
    right: Arc<TextDocument>,
    request: &TextRequest,
    cancel: &CancelToken,
) -> TextOutcome {
    let diff = diff_with_rules(
        DiffInput {
            bytes: left.bytes(),
            lines: left.lines(),
        },
        DiffInput {
            bytes: right.bytes(),
            lines: right.lines(),
        },
        request.options,
        &request.rules,
        cancel,
    );
    let (mut diff, applied) = match diff {
        Ok(diff) => diff,
        Err(err) => return TextOutcome::Failed(err.to_string()),
    };
    // A re-diff says it too, so the note outlives the first edit (PF14).
    let notes = if applied {
        Vec::new()
    } else {
        vec![RULES_SKIPPED.to_owned()]
    };
    // Pairs the ignore options hid (PF6): counted here, never inside `diff_lines`, and only for
    // sides small enough to scan twice. Rules count their own.
    let small = left.bytes().len() <= RULES_MAX_BYTES && right.bytes().len() <= RULES_MAX_BYTES;
    if !request.rules.is_active() && request.ignoring_options() && small {
        let counted = count_soft_pairs_checked(
            DiffInput {
                bytes: left.bytes(),
                lines: left.lines(),
            },
            DiffInput {
                bytes: right.bytes(),
                lines: right.lines(),
            },
            &diff,
            cancel,
        );
        match counted {
            Ok(pairs) => diff.soft_pairs = pairs,
            Err(_) => return TextOutcome::Failed("Compare cancelled.".into()),
        }
    }
    let moves = if request.detect_moves {
        let found = detect_moves(
            DiffInput {
                bytes: left.bytes(),
                lines: left.lines(),
            },
            DiffInput {
                bytes: right.bytes(),
                lines: right.lines(),
            },
            &diff,
            request.options,
            MOVE_LIMIT,
            cancel,
        );
        match found {
            Ok(moves) => moves,
            Err(_) => return TextOutcome::Failed("Compare cancelled.".into()),
        }
    } else {
        Moves::default()
    };
    TextOutcome::Ready(Box::new(
        TextModel::new(left, right, diff)
            .with_moves(moves)
            .with_notes(notes),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(left: &[u8], right: &[u8]) -> (tempfile::TempDir, TextRequest) {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
        std::fs::write(&a, left).unwrap();
        std::fs::write(&b, right).unwrap();
        (dir, TextRequest::new(a, b))
    }

    #[test]
    fn text_pairs_become_a_model() {
        let (_dir, request) = pair(b"a\nb\n", b"a\nc\n");
        let TextOutcome::Ready(model) = run(&request, &CancelToken::new()) else {
            panic!("expected a model");
        };
        assert_eq!(model.block_count(), 1);
    }

    #[test]
    fn rediff_compares_loaded_documents_without_touching_disk() {
        let (dir, request) = pair(b"a\nb\n", b"a\nc\n");
        let TextOutcome::Ready(model) = run(&request, &CancelToken::new()) else {
            panic!()
        };
        let (left, right, _) = model.snapshot();
        std::fs::remove_file(dir.path().join("a.txt")).unwrap();
        let edited = Arc::new(right.edited("a\nb\n".into(), right.stamp()));
        let TextOutcome::Ready(again) = rediff(left, edited, &request, &CancelToken::new()) else {
            panic!()
        };
        assert_eq!(again.block_count(), 0);
    }

    #[test]
    fn binary_files_report_byte_equality() {
        let (_dir, request) = pair(b"a\0b", b"a\0c");
        assert!(matches!(
            run(&request, &CancelToken::new()),
            TextOutcome::Binary { identical: false }
        ));
    }

    #[test]
    fn forcing_text_loads_a_binary_pair_as_a_model() {
        let (_dir, mut request) = pair(b"a\0b\nc\n", b"a\0c\nc\n");
        request.force_text = true;
        let TextOutcome::Ready(model) = run(&request, &CancelToken::new()) else {
            panic!("expected a model");
        };
        assert_eq!(model.block_count(), 1);
        assert_eq!(model.left().line(0), "a\0b");
    }

    #[test]
    fn missing_files_fail_with_the_path() {
        let (dir, mut request) = pair(b"a", b"a");
        request.right = dir.path().join("nope.txt");
        let TextOutcome::Failed(message) = run(&request, &CancelToken::new()) else {
            panic!("expected a failure");
        };
        assert!(message.contains("nope.txt"), "{message}");
    }

    #[test]
    fn a_missing_side_is_reported_even_when_the_other_is_binary() {
        let (dir, mut request) = pair(b"a\0b", b"a\0c");
        request.right = dir.path().join("gone.bin");
        let TextOutcome::Failed(message) = run(&request, &CancelToken::new()) else {
            panic!("expected a failure");
        };
        assert!(message.contains("gone.bin"), "{message}");
    }

    #[test]
    fn a_forced_encoding_decodes_thai_bytes() {
        // "ไทย" in windows-874, which auto-detection might read as windows-1252.
        let (_dir, mut request) = pair(b"\xe4\xb7\xc2\n", b"\xe4\xb7\xc2\n");
        request.left_encoding = Some(WINDOWS_874);
        request.right_encoding = Some(WINDOWS_874);
        let TextOutcome::Ready(model) = run(&request, &CancelToken::new()) else {
            panic!("expected a model");
        };
        assert_eq!(model.left().line(0), "ไทย");
    }

    #[test]
    fn encoding_buttons_name_what_was_detected() {
        assert_eq!(encoding_text(None, Some(UTF_8)), "UTF-8 (auto)");
        assert_eq!(encoding_text(None, None), "Auto");
        assert_eq!(
            encoding_text(Some(WINDOWS_874), Some(WINDOWS_874)),
            "Thai (windows-874)"
        );
    }

    #[test]
    fn ignore_choices_toggle_and_count() {
        use duet_core::text::diff::WhitespaceMode;
        let none = DiffOptions::default();
        let edges = apply_ignore(none, IgnoreChoice::WhitespaceEdges);
        assert_eq!(edges.whitespace, WhitespaceMode::Edges);
        let all = apply_ignore(edges, IgnoreChoice::WhitespaceAll);
        assert_eq!(
            all.whitespace,
            WhitespaceMode::All,
            "the two whitespace modes exclude each other"
        );
        assert_eq!(
            apply_ignore(all, IgnoreChoice::WhitespaceAll).whitespace,
            WhitespaceMode::None
        );
        let two = apply_ignore(
            apply_ignore(edges, IgnoreChoice::Case),
            IgnoreChoice::LineEndings,
        );
        assert!(two.ignore_case && two.ignore_eol);
    }

    #[test]
    fn formatting_hides_layout_only_json_differences() {
        let (_dir, mut request) = pair(
            b"{\"a\":1,\"b\":[1,2]}",
            b"{\n  \"a\": 1,\n  \"b\": [1, 2]\n}",
        );
        let TextOutcome::Ready(plain) = run(&request, &CancelToken::new()) else {
            panic!("expected a model");
        };
        assert!(plain.block_count() > 0);
        request.format = true;
        let TextOutcome::Ready(model) = run(&request, &CancelToken::new()) else {
            panic!("expected a model");
        };
        assert_eq!(model.block_count(), 0);
        assert_eq!(
            model.notes(),
            ["Left: formatted JSON", "Right: formatted JSON"]
        );
    }

    #[test]
    fn encoding_choices_cycle() {
        assert_eq!(next_encoding(None), Some(UTF_8));
        assert_eq!(
            next_encoding(Some(WINDOWS_1252)),
            Some(encoding_rs::SHIFT_JIS)
        );
        assert_eq!(next_encoding(Some(encoding_rs::GBK)), None);
        assert_eq!(ENCODINGS.len(), 8);
        assert_eq!(encoding_label(Some(WINDOWS_874)), "Thai (windows-874)");
    }
}
