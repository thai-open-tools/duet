//! Loads both files and diffs them off the UI thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use duet_core::cancel::CancelToken;
use duet_core::folder::content::files_equal;
use duet_core::text::diff::{DiffInput, DiffOptions, WhitespaceMode, count_soft_pairs_checked};
use duet_core::text::document::{LoadError, TextDocument};
use duet_core::text::format::auto_format;
use duet_core::text::lines::EolStyle;
use duet_core::text::moves::{MOVE_LIMIT, Moves, detect_moves};
use duet_core::text::rules::{RULES_MAX_BYTES, TextRules, diff_with_rules};
use duet_core::text::sort_keys::{SortError, SortNote, Sorted, sort_keys, sort_kind_for};
use encoding_rs::{Encoding, GBK, SHIFT_JIS, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_874, WINDOWS_1252};

use crate::i18n::errors::{content_error, diff_error, read_error};
use crate::i18n::{LiveText, live, tr, tr_key, tr_str, trf};
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
    /// Show JSON or YAML with its keys sorted on both sides before comparing (read-only). It wins
    /// over `format` for a side it can sort.
    pub sort_keys: bool,
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
            sort_keys: false,
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
pub const RULES_SKIPPED: &str = tr_key("Rules not applied: a file is over 64 MiB.");

pub enum TextOutcome {
    Ready(Box<TextModel>),
    Binary {
        identical: bool,
    },
    /// Open as text was asked for a binary file over `EDIT_LIMIT`.
    TooLargeAsText,
    Failed(LiveText),
}

/// The per-side "read as" choices; `None` means detect automatically.
pub const ENCODINGS: [(&str, Option<&'static Encoding>); 8] = [
    (tr_key("Auto"), None),
    ("UTF-8", Some(UTF_8)),
    (tr_key("Thai (windows-874)"), Some(WINDOWS_874)),
    ("UTF-16LE", Some(UTF_16LE)),
    ("UTF-16BE", Some(UTF_16BE)),
    (tr_key("Western (windows-1252)"), Some(WINDOWS_1252)),
    (tr_key("Japanese (Shift_JIS)"), Some(SHIFT_JIS)),
    (tr_key("Chinese (GBK)"), Some(GBK)),
];

/// The rows of the "read as" menu, each named in the chosen language.
pub fn encoding_choices() -> impl Iterator<Item = (&'static str, Option<&'static Encoding>)> {
    ENCODINGS
        .iter()
        .map(|(label, encoding)| (tr(label), *encoding))
}

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
        .map_or(tr_key("Auto"), |(label, _)| *label)
}

/// The encoding button's text: the chosen encoding, or what auto-detection found.
pub fn encoding_text(
    chosen: Option<&'static Encoding>,
    detected: Option<&'static Encoding>,
) -> String {
    match (chosen, detected) {
        (Some(chosen), _) => tr(encoding_label(Some(chosen))).to_owned(),
        (None, Some(found)) => trf("{} (auto)", &[&found.name()]),
        (None, None) => tr("Auto").to_owned(),
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

/// Which side a note speaks of, kept apart from the note's words: F9 swaps the sides of a model
/// in place, and the notes it holds must follow without the compare running again. Every note
/// of a model shares one flip, and the model turns it over when it swaps its sides.
#[derive(Debug, Clone, Default)]
pub struct SideFlip(Arc<AtomicBool>);

impl SideFlip {
    pub fn toggle(&self) {
        self.0.fetch_xor(true, Ordering::SeqCst);
    }

    fn is_flipped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// One side as a note names it, now: "Left" or "Right" in the language in force, after any swap.
#[derive(Debug, Clone)]
struct SideName {
    /// The side the note was made for when the model was built.
    left: bool,
    flip: SideFlip,
}

impl SideName {
    fn new(left: bool, flip: &SideFlip) -> Self {
        Self {
            left,
            flip: flip.clone(),
        }
    }

    fn words(&self) -> String {
        let key = if self.left != self.flip.is_flipped() {
            tr_key("Left")
        } else {
            tr_key("Right")
        };
        tr(key).to_owned()
    }
}

/// A note about one side, worded again whenever the language or the side changes.
fn side_note(side: &SideName, make: impl Fn(&str) -> String + Send + Sync + 'static) -> LiveText {
    let side = side.clone();
    LiveText::new(move || make(&side.words()))
}

/// One side for the Format button: pretty-printed when it is JSON or XML, as is otherwise. A side
/// with bytes its encoding cannot read is kept as it is: the pretty text would hide them.
fn format_side(doc: TextDocument, side: &SideName, notes: &mut Vec<LiveText>) -> TextDocument {
    if doc.is_lossy() {
        notes.push(undecodable_note(side));
        return doc;
    }
    let Some(text) = doc.text() else {
        notes.push(side_note(side, |side| {
            trf("{}: over 64 MiB, shown as is", &[&side])
        }));
        return doc;
    };
    match auto_format(text) {
        Some(formatted) => {
            let kind = formatted.kind.label();
            notes.push(side_note(side, move |side| {
                trf("{}: formatted {}", &[&side, &kind])
            }));
            doc.formatted(formatted.text)
        }
        None => {
            notes.push(side_note(side, |side| {
                trf("{}: not JSON or XML, shown as is", &[&side])
            }));
            doc
        }
    }
}

fn undecodable_note(side: &SideName) -> LiveText {
    side_note(side, |side| {
        trf(
            "{}: has bytes the encoding cannot read, shown as is",
            &[&side],
        )
    })
}

/// Why a side was not sorted. It holds data only: the note is worded in the language in force
/// when it is read.
enum SortSkip {
    /// Not JSON or YAML (a reason Format may still take the side).
    NotSortable,
    /// Over the sort's size limit (Format has a larger one).
    TooLarge,
    /// Bytes the encoding could not read: the sorted text would not show them (Format may keep
    /// the side as it is too).
    Undecodable,
    TooDeep,
    TooBig,
    Complex,
    Syntax {
        line: usize,
        message: String,
    },
    Cancelled,
}

/// What the sort said of a side, apart from its text.
struct SortReport {
    kind: duet_core::text::sort_keys::SortKind,
    notes: Vec<SortNote>,
}

/// One side after Sort keys: the document with its keys sorted, or the document back with the
/// reason.
enum SideSort {
    /// The sorted document, what the sort noticed, and the line breaks of the file it came from.
    Sorted(TextDocument, SortReport, EolStyle),
    Kept(TextDocument, SortSkip),
}

/// One side for the Sort keys button. It parses on the calling thread, so the caller is off the
/// UI thread.
fn sort_side(doc: TextDocument, cancel: &CancelToken) -> SideSort {
    if doc.is_lossy() {
        return SideSort::Kept(doc, SortSkip::Undecodable);
    }
    let Some(text) = doc.text() else {
        return SideSort::Kept(doc, SortSkip::TooLarge);
    };
    let Some(kind) = sort_kind_for(doc.path(), text) else {
        return SideSort::Kept(doc, SortSkip::NotSortable);
    };
    match sort_keys(text, kind, cancel) {
        Ok(Sorted { kind, text, notes }) => {
            let eol = doc.eol();
            SideSort::Sorted(doc.sorted(text), SortReport { kind, notes }, eol)
        }
        Err(error) => {
            let skip = match error {
                SortError::TooLarge => SortSkip::TooLarge,
                SortError::TooDeep => SortSkip::TooDeep,
                SortError::TooBig => SortSkip::TooBig,
                SortError::Unsupported(_) => SortSkip::Complex,
                SortError::Cancelled => SortSkip::Cancelled,
                SortError::Syntax { line, message, .. } => SortSkip::Syntax { line, message },
            };
            SideSort::Kept(doc, skip)
        }
    }
}

/// How many duplicate keys a side names one by one; the rest are counted.
const DUPLICATES_NAMED: usize = 3;

/// A key in a note is cut here: a key can be as long as the file.
const KEY_SHOWN: usize = 80;

fn shortened(key: &str) -> String {
    match key.char_indices().nth(KEY_SHOWN) {
        Some((at, _)) => format!("{}…", &key[..at]),
        None => key.to_owned(),
    }
}

/// The notes of a side that was sorted: what it is, then what the sort left out or kept.
fn sorted_notes(side: &SideName, report: &SortReport, notes: &mut Vec<LiveText>) {
    let kind = report.kind.label();
    notes.push(side_note(side, move |side| {
        trf("{}: sorted {} keys", &[&side, &kind])
    }));
    let mut duplicates = 0usize;
    for note in &report.notes {
        match note {
            SortNote::CommentsDropped => notes.push(side_note(side, |side| {
                trf("{}: YAML comments are not shown", &[&side])
            })),
            SortNote::AnchorsExpanded => notes.push(side_note(side, |side| {
                trf("{}: YAML anchors are expanded", &[&side])
            })),
            SortNote::YamlVersion(major, minor) => {
                let (major, minor) = (*major, *minor);
                notes.push(side_note(side, move |side| {
                    trf(
                        "{}: the YAML version directive {}.{} is not shown",
                        &[&side, &major, &minor],
                    )
                }));
            }
            SortNote::Documents(count) => {
                let count = *count;
                notes.push(side_note(side, move |side| {
                    trf("{}: {} documents, each sorted", &[&side, &count])
                }));
            }
            SortNote::DuplicateKey { key, line } => {
                duplicates += 1;
                if duplicates <= DUPLICATES_NAMED {
                    let (key, line) = (shortened(key), *line);
                    notes.push(side_note(side, move |side| {
                        trf(
                            "{}: duplicate key {} at line {} of the file, kept in order",
                            &[&side, &key, &line],
                        )
                    }));
                }
            }
        }
    }
    if duplicates > DUPLICATES_NAMED {
        let more = duplicates - DUPLICATES_NAMED;
        notes.push(side_note(side, move |side| {
            trf("{}: and {} more duplicate keys", &[&side, &more])
        }));
    }
}

/// The note of a side that was left as it is.
fn skipped_note(side: &SideName, skip: SortSkip) -> Option<LiveText> {
    Some(match skip {
        SortSkip::Cancelled => return None,
        SortSkip::Undecodable => undecodable_note(side),
        SortSkip::NotSortable => side_note(side, |side| {
            trf("{}: not JSON or YAML, shown as is", &[&side])
        }),
        SortSkip::TooLarge => side_note(side, |side| trf("{}: over 16 MiB, shown as is", &[&side])),
        SortSkip::TooDeep => side_note(side, |side| {
            trf("{}: nested too deeply to sort, shown as is", &[&side])
        }),
        SortSkip::TooBig => side_note(side, |side| {
            trf("{}: too large to sort, shown as is", &[&side])
        }),
        SortSkip::Complex => side_note(side, |side| {
            trf(
                "{}: complex YAML keys are not supported, shown as is",
                &[&side],
            )
        }),
        SortSkip::Syntax { line, message } => side_note(side, move |side| {
            trf(
                "{}: {} (line {} of the file), shown as is",
                &[&side, &tr_str(&message), &line],
            )
        }),
    })
}

/// Both sides through Sort keys, each parsed on its own thread. A side that is neither JSON nor
/// YAML, or that is over the sort's limit, goes on to Format when that is on. None when the job
/// was cancelled.
fn sort_sides(
    left: TextDocument,
    right: TextDocument,
    format: bool,
    notes: &mut Vec<LiveText>,
    flip: &SideFlip,
    cancel: &CancelToken,
) -> Option<(TextDocument, TextDocument)> {
    // Nothing in the scope calls `tr`: the notes are built from the data after it.
    let (left, right) = std::thread::scope(|scope| {
        let right = scope.spawn(move || sort_side(right, cancel));
        let left = sort_side(left, cancel);
        let right = right
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        (left, right)
    });
    if cancel.is_cancelled() {
        return None;
    }
    let mut any = false;
    let mut eols = Vec::new();
    let mut finish = |result, side: SideName| match result {
        SideSort::Sorted(doc, report, eol) => {
            any = true;
            eols.push(eol);
            sorted_notes(&side, &report, notes);
            doc
        }
        SideSort::Kept(doc, SortSkip::NotSortable | SortSkip::TooLarge | SortSkip::Undecodable)
            if format =>
        {
            format_side(doc, &side, notes)
        }
        SideSort::Kept(doc, skip) => {
            notes.extend(skipped_note(&side, skip));
            doc
        }
    };
    let left = finish(left, SideName::new(true, flip));
    let right = finish(right, SideName::new(false, flip));
    // The sorted text always breaks its lines with LF, so the files' own breaks are not compared:
    // when they differ, say so, or a real difference would go unnoticed.
    if let [one, other] = eols[..]
        && one != other
    {
        notes.push(live(LINE_ENDINGS_DIFFER));
    }
    if any {
        notes.push(live("Line numbers are those of the sorted text."));
    }
    Some((left, right))
}

const LINE_ENDINGS_DIFFER: &str =
    tr_key("The two files end their lines differently; the sorted view does not show that");

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
            return TextOutcome::Failed(LiveText::new(move || read_error(&err)));
        }
        (Err(LoadError::TooLargeAsText { .. }), _) | (_, Err(LoadError::TooLargeAsText { .. })) => {
            return TextOutcome::TooLargeAsText;
        }
        (Err(LoadError::Binary { .. }), _) | (_, Err(LoadError::Binary { .. })) => {
            return match files_equal(&request.left, &request.right, cancel) {
                Ok(identical) => TextOutcome::Binary { identical },
                Err(err) => TextOutcome::Failed(LiveText::new(move || content_error(&err))),
            };
        }
        (Err(err), _) | (_, Err(err)) => {
            return TextOutcome::Failed(LiveText::new(move || read_error(&err)));
        }
    };
    let mut notes = Vec::new();
    let flip = SideFlip::default();
    let (left, right) = if request.sort_keys {
        match sort_sides(left, right, request.format, &mut notes, &flip, cancel) {
            Some(sides) => sides,
            None => return TextOutcome::Failed(live("Compare cancelled.")),
        }
    } else if request.format {
        (
            format_side(left, &SideName::new(true, &flip), &mut notes),
            format_side(right, &SideName::new(false, &flip), &mut notes),
        )
    } else {
        (left, right)
    };
    match rediff(Arc::new(left), Arc::new(right), request, cancel) {
        TextOutcome::Ready(model) => {
            // The format notes first, then the compare's own (PF14).
            notes.extend(model.live_notes().iter().cloned());
            TextOutcome::Ready(Box::new(model.with_notes(notes).with_note_flip(flip)))
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
        Err(err) => return TextOutcome::Failed(LiveText::new(move || diff_error(&err))),
    };
    // A re-diff says it too, so the note outlives the first edit (PF14).
    let notes = if applied {
        Vec::new()
    } else {
        vec![live(RULES_SKIPPED)]
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
            Err(_) => return TextOutcome::Failed(live("Compare cancelled.")),
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
            Err(_) => return TextOutcome::Failed(live("Compare cancelled.")),
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
    #[test]
    fn the_read_as_menu_is_in_the_chosen_language() {
        let labels = |lang| {
            let _scope = crate::i18n::scoped(lang);
            super::encoding_choices()
                .map(|(label, _)| label)
                .collect::<Vec<_>>()
        };
        let thai = labels(crate::i18n::Lang::Th);
        assert!(thai.contains(&"ไทย (windows-874)"), "{thai:?}");
        assert!(!thai.contains(&"Thai (windows-874)"), "{thai:?}");
        assert!(!thai.contains(&"Auto"), "{thai:?}");
        assert!(thai.contains(&"UTF-8"), "format names stay: {thai:?}");
        let english = labels(crate::i18n::Lang::En);
        assert!(english.contains(&"Thai (windows-874)"), "{english:?}");
    }

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

    fn sorted(request: &TextRequest) -> Box<TextModel> {
        let TextOutcome::Ready(model) = run(request, &CancelToken::new()) else {
            panic!("expected a model");
        };
        model
    }

    fn sorting(left: &[u8], right: &[u8]) -> (tempfile::TempDir, TextRequest) {
        let (dir, mut request) = pair(left, right);
        request.sort_keys = true;
        (dir, request)
    }

    #[test]
    fn sorting_hides_key_order_differences() {
        let (_dir, mut request) = sorting(b"{\"a\":1,\"b\":2}", b"{\"b\":2,\"a\":1}");
        let model = sorted(&request);
        assert_eq!(model.block_count(), 0);
        assert_eq!(
            model.notes(),
            [
                "Left: sorted JSON keys",
                "Right: sorted JSON keys",
                "Line numbers are those of the sorted text."
            ]
        );
        request.sort_keys = false;
        assert!(sorted(&request).block_count() > 0);
    }

    #[test]
    fn sorting_still_shows_a_real_difference() {
        let (_dir, request) = sorting(b"{\"a\":1.0}", b"{\"a\":1}");
        assert!(sorted(&request).block_count() > 0, "1.0 against 1");
        let (_dir, request) = sorting(b"{\"a\":1,\"b\":2}", b"{\"b\":3,\"a\":1}");
        assert!(sorted(&request).block_count() > 0, "a changed value");
        let (_dir, request) = sorting(b"{\"a\":[1,2]}", b"{\"a\":[2,1]}");
        assert!(sorted(&request).block_count() > 0, "array order counts");
    }

    #[test]
    fn a_side_that_does_not_parse_keeps_its_text_and_the_note_names_the_line() {
        let (_dir, request) = sorting(b"{\"b\":1,\"a\":2}", b"{\"a\": 1,}");
        let model = sorted(&request);
        assert_eq!(model.right().text(), Some("{\"a\": 1,}"));
        assert_eq!(model.left().line(1), "  \"a\": 2,");
        assert_eq!(
            model.notes(),
            [
                "Left: sorted JSON keys",
                "Right: unexpected comma before closing brace (line 1 of the file), shown as is",
                "Line numbers are those of the sorted text."
            ]
        );
    }

    #[test]
    fn a_yaml_pair_is_sorted_by_extension_and_a_txt_pair_that_is_not_json_is_left() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.yaml"), dir.path().join("b.yaml"));
        std::fs::write(&a, "b: 1\na: 2\n").unwrap();
        std::fs::write(&b, "a: 2\nb: 1\n").unwrap();
        let mut request = TextRequest::new(a, b);
        request.sort_keys = true;
        let model = sorted(&request);
        assert_eq!(model.block_count(), 0);
        assert_eq!(model.notes()[0], "Left: sorted YAML keys");
        let (_dir, request) = sorting(b"x: 1\n", b"x: 2\n");
        let model = sorted(&request);
        assert_eq!(
            model.notes(),
            [
                "Left: not JSON or YAML, shown as is",
                "Right: not JSON or YAML, shown as is"
            ]
        );
        assert!(model.block_count() > 0);
    }

    #[test]
    fn with_format_on_xml_goes_on_to_format() {
        let (_dir, mut request) = sorting(b"{\"b\":1,\"a\":2}", b"<a><b>x</b></a>");
        request.format = true;
        let model = sorted(&request);
        assert_eq!(
            model.notes(),
            [
                "Left: sorted JSON keys",
                "Right: formatted XML",
                "Line numbers are those of the sorted text."
            ]
        );
        assert_eq!(model.right().line(1), "  <b>x</b>");
    }

    #[test]
    fn a_duplicate_key_is_noted_and_kept() {
        let (_dir, request) = sorting(b"{\"a\":1,\n\"a\":2}", b"{}");
        let model = sorted(&request);
        assert_eq!(model.left().line_count(), 4);
        assert_eq!(
            model.notes()[1],
            "Left: duplicate key \"a\" at line 2 of the file, kept in order"
        );
    }

    #[test]
    fn more_than_three_duplicates_are_summarised() {
        let (_dir, request) = sorting(b"{\"a\":1,\"a\":2,\"a\":3,\"a\":4,\"a\":5,\"a\":6}", b"{}");
        let notes = sorted(&request).notes();
        let named = notes
            .iter()
            .filter(|n| n.contains("duplicate key \"a\""))
            .count();
        assert_eq!(named, 3);
        assert!(
            notes.contains(&"Left: and 2 more duplicate keys".to_owned()),
            "{notes:?}"
        );
    }

    #[test]
    fn a_side_over_16_mib_is_shown_as_is_with_a_note() {
        let big = format!("[{}0]", "0,".repeat(8 * 1024 * 1024 + 1));
        let (_dir, request) = sorting(big.as_bytes(), b"[0]");
        let model = sorted(&request);
        assert_eq!(model.notes()[0], "Left: over 16 MiB, shown as is");
        assert_eq!(model.left().text().map(str::len), Some(big.len()));
    }

    #[test]
    fn thai_notes_have_entries() {
        use crate::i18n::{Lang, missing_keys_for_test, scoped};
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.yaml"), dir.path().join("b.yaml"));
        std::fs::write(
            &a,
            "# c\nb: &x 1\na: *x\na: 2\na: 3\na: 4\na: 5\n---\nz: 1\n",
        )
        .unwrap();
        std::fs::write(&b, "a: [1\n").unwrap();
        let (c, d) = (dir.path().join("c.txt"), dir.path().join("d.json"));
        std::fs::write(&c, "x").unwrap();
        std::fs::write(&d, "{'a': 1}").unwrap();
        let (e, f) = (dir.path().join("e.yaml"), dir.path().join("f.json"));
        std::fs::write(&e, "? [a]\n: v\n").unwrap();
        std::fs::write(&f, "[[[[[[").unwrap();
        let _th = scoped(Lang::Th);
        for (left, right) in [(a, b), (c, d), (e, f)] {
            let mut request = TextRequest::new(left, right);
            request.sort_keys = true;
            let model = sorted(&request);
            assert!(model.notes().len() >= 2);
            for note in model.notes() {
                assert!(
                    !note.starts_with("Left:") && !note.starts_with("Right:"),
                    "{note}"
                );
            }
        }
        assert!(
            missing_keys_for_test().is_empty(),
            "{:?}",
            missing_keys_for_test()
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
    fn crlf_note() -> String {
        tr(LINE_ENDINGS_DIFFER).to_owned()
    }

    #[test]
    fn line_endings_do_not_invent_a_difference_but_the_note_tells_them() {
        // No line break at all against the same data pretty-printed with CRLF.
        let (_dir, request) = sorting(b"{\"b\":2,\"a\":1}", b"{\r\n  \"a\": 1,\r\n  \"b\": 2\r\n}");
        let model = sorted(&request);
        assert_eq!(model.block_count(), 0, "the same data");
        assert!(model.notes().contains(&crlf_note()), "{:?}", model.notes());
        // CRLF against LF.
        let (_dir, request) = sorting(
            b"{\r\n  \"b\": 2,\r\n  \"a\": 1\r\n}\r\n",
            b"{\n  \"a\": 1,\n  \"b\": 2\n}\n",
        );
        let model = sorted(&request);
        assert_eq!(model.block_count(), 0);
        assert!(model.notes().contains(&crlf_note()), "{:?}", model.notes());
        // The same break on both sides says nothing.
        let (_dir, request) = sorting(
            b"{\r\n  \"b\": 2,\r\n  \"a\": 1\r\n}\r\n",
            b"{\"a\":1,\"b\":2}\r\n",
        );
        let model = sorted(&request);
        assert!(!model.notes().contains(&crlf_note()), "{:?}", model.notes());
        // A real difference still shows, whatever the breaks.
        let (_dir, request) = sorting(b"{\"a\":1}", b"{\r\n  \"a\": 2\r\n}");
        assert!(sorted(&request).block_count() > 0);
    }

    #[test]
    fn a_side_that_did_not_decode_cleanly_is_kept_with_a_note() {
        let (_dir, mut request) = sorting(b"{\"a\":\"\xff\"}", b"{\"a\":\"\xfe\"}");
        request.left_encoding = Some(UTF_8);
        request.right_encoding = Some(UTF_8);
        let model = sorted(&request);
        assert!(model.left().is_lossy() && model.right().is_lossy());
        assert_eq!(
            model.notes(),
            [
                "Left: has bytes the encoding cannot read, shown as is",
                "Right: has bytes the encoding cannot read, shown as is"
            ]
        );
        // Format keeps such a side as it is too.
        request.sort_keys = false;
        request.format = true;
        let model = sorted(&request);
        assert!(model.left().is_lossy() && model.right().is_lossy());
        assert_eq!(
            model.notes(),
            [
                "Left: has bytes the encoding cannot read, shown as is",
                "Right: has bytes the encoding cannot read, shown as is"
            ]
        );
    }

    #[test]
    fn a_long_duplicate_key_is_cut_in_its_note() {
        let key = "k".repeat(300);
        let text = format!("{{\"{key}\":1,\"{key}\":2}}");
        let (_dir, request) = sorting(text.as_bytes(), b"{}");
        let notes = sorted(&request).notes();
        let note = notes.iter().find(|n| n.contains("duplicate key")).unwrap();
        assert!(note.contains("kkkk…"), "{note}");
        assert!(note.chars().count() < 200, "{}", note.chars().count());
    }

    #[test]
    fn a_json_side_over_16_mib_goes_on_to_format() {
        let big = format!("[\"{}\"]", "a".repeat(16 * 1024 * 1024 + 8));
        let (_dir, mut request) = sorting(big.as_bytes(), b"[0]");
        request.format = true;
        let model = sorted(&request);
        assert_eq!(
            model.notes(),
            [
                "Left: formatted JSON",
                "Right: sorted JSON keys",
                "Line numbers are those of the sorted text."
            ]
        );
        assert_eq!(model.left().line(1).len(), 16 * 1024 * 1024 + 8 + 4);
    }

    #[test]
    fn a_yaml_version_directive_is_noted() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.yaml"), dir.path().join("b.yaml"));
        std::fs::write(&a, "%YAML 1.1\n---\na: 1\n").unwrap();
        std::fs::write(&b, "%YAML 1.2\n---\na: 1\n").unwrap();
        let mut request = TextRequest::new(a, b);
        request.sort_keys = true;
        let notes = sorted(&request).notes();
        assert!(notes.contains(&"Left: the YAML version directive 1.1 is not shown".to_owned()));
        assert!(notes.contains(&"Right: the YAML version directive 1.2 is not shown".to_owned()));
    }
}
