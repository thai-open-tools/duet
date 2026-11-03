use super::*;
use duet_core::cancel::CancelToken;
use duet_core::text::diff::{DiffInput, DiffOptions, LineHunk, diff_lines};
use duet_core::text::document::FileStamp;

pub(super) fn model_for(left: &str, right: &str) -> (tempfile::TempDir, TextModel) {
    model_for_bytes(left.as_bytes(), right.as_bytes())
}

fn model_for_bytes(left: &[u8], right: &[u8]) -> (tempfile::TempDir, TextModel) {
    model_with(left, right, DiffOptions::default())
}

fn model_with(left: &[u8], right: &[u8], options: DiffOptions) -> (tempfile::TempDir, TextModel) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, left).unwrap();
    std::fs::write(&b, right).unwrap();
    let cancel = CancelToken::new();
    let (l, r) = (
        TextDocument::open(&a, &cancel).unwrap(),
        TextDocument::open(&b, &cancel).unwrap(),
    );
    let diff = diff_lines(
        DiffInput {
            bytes: l.bytes(),
            lines: l.lines(),
        },
        DiffInput {
            bytes: r.bytes(),
            lines: r.lines(),
        },
        options,
        &cancel,
    )
    .unwrap();
    (dir, TextModel::new(l, r, diff))
}

/// rows: 0 a | 1 b→B (changed) | 2-5 same | 6 g→G (changed) | 7 h (right only)
fn sample() -> (tempfile::TempDir, TextModel) {
    model_for("a\nb\nc\nd\ne\nf\ng\n", "a\nB\nc\nd\ne\nf\nG\nh\n")
}

#[test]
fn a_side_made_editable_has_room_to_grow_without_a_copy() {
    let line = format!("{}\n", "x".repeat(99));
    let big = line.repeat(20_000);
    for (left, right) in [("a\n", "a\n"), (big.as_str(), big.as_str())] {
        let (_dir, mut model) = model_for(left, right);
        model.begin_edit(false).unwrap();
        let buffer = model.buffer(false).unwrap();
        let room = (buffer.len() / 16).max(64 * 1024);
        assert!(
            buffer.capacity() >= buffer.len() + room,
            "capacity {} for {} bytes",
            buffer.capacity(),
            buffer.len()
        );
    }
}

#[test]
fn edits_change_rows_at_once_and_mark_the_side_dirty() {
    let (_dir, mut model) = sample();
    assert!(!model.is_dirty(false));
    model.begin_edit(false).unwrap();
    assert!(!model.is_dirty(false), "a buffer alone is not an edit");
    let start = model.buffer(false).unwrap().line_start(3);
    let change = model.replace(false, start..start + 1, "D").unwrap();
    assert_eq!((change.line, change.old_lines, change.new_lines), (3, 1, 1));
    assert!(model.is_dirty(false) && !model.is_dirty(true));
    assert_eq!(model.line_text(false, 3), "D");
    let row = model.row_of_line(false, 3).unwrap();
    let display = model.visible().display_of(row).unwrap();
    assert_eq!(
        model.row(display).unwrap().kind,
        RowKind::Changed,
        "provisional hunk"
    );
}

fn stale_request() -> crate::text::job::TextRequest {
    crate::text::job::TextRequest::new("a".into(), "b".into())
}

/// Replaces the first byte of right line `line` with `text`.
fn edit_right(model: &mut TextModel, line: usize, text: &str) {
    let start = {
        model.begin_edit(false).unwrap();
        model.buffer(false).unwrap().line_start(line)
    };
    model.replace(false, start..start + 1, text).unwrap();
}

#[test]
fn inline_highlights_follow_the_buffer_after_a_line_is_inserted() {
    let (_dir, mut model) = sample();
    edit_right(&mut model, 6, "GG");
    let at = model.buffer(false).unwrap().line_start(4);
    model.replace(false, at..at, "new\n").unwrap();
    // right line 7 is the old "G" (was 6), still paired with left line 6 "g"
    let row = model.row_of_line(false, 7).unwrap();
    let diff = model.inline_at(row).expect("a changed pair");
    let (l, r) = (model.line_text(true, 6), model.line_text(false, 7));
    assert_eq!((&*l, &*r), ("g", "GG"));
    let cover = |text: &str, ranges: &[Range<usize>]| {
        ranges
            .iter()
            .map(|range| {
                assert!(range.end <= text.len() && text.is_char_boundary(range.start));
                assert!(text.is_char_boundary(range.end));
                text[range.clone()].to_owned()
            })
            .collect::<String>()
    };
    assert_eq!(cover(&l, &diff.left), "g");
    assert_eq!(cover(&r, &diff.right), "GG");
}

#[test]
fn inline_highlights_use_edited_thai_text_on_char_boundaries() {
    let (_dir, mut model) = sample();
    edit_right(&mut model, 1, "ข");
    let row = model.row_of_line(false, 1).unwrap();
    let diff = model.inline_at(row).expect("a changed pair");
    let r = model.line_text(false, 1);
    assert_eq!(&*r, "ข");
    for range in &diff.right {
        assert!(r.is_char_boundary(range.start) && r.is_char_boundary(range.end));
    }
    assert_eq!(diff.right.iter().map(|x| x.len()).sum::<usize>(), r.len());
}

#[test]
fn sources_return_the_edited_text_that_matches_the_rows() {
    let (_dir, mut model) = sample();
    model.begin_edit(false).unwrap();
    model.replace(false, 0..0, "new\n").unwrap();
    let (_, right, rows) = model.sources();
    assert_eq!(right.line(0), "new");
    assert_eq!(right.line_count(), 9);
    let row = rows.row(model.row_of_line(false, 8).unwrap());
    assert_eq!(row.right, Some(8));
    assert_eq!(right.line(8), "h");
}

/// A search copies an edited side into a document. Where the side's document already holds
/// the buffer's text (a buffer just made, a landed re-diff, a save), it is shared instead:
/// copying a 60 MiB side costs about 30 ms of the UI thread.
#[test]
fn documents_that_hold_the_buffers_text_are_shared_not_copied() {
    use crate::text::job::{TextOutcome, rediff};
    let shared = |doc: &Arc<TextDocument>, side: &TextDocument| std::ptr::eq(&**doc, side);
    let (_dir, mut model) = sample();
    model.begin_edit(true).unwrap();
    let (left, _, _) = model.sources();
    assert!(
        shared(&left, model.left()),
        "a buffer just made from its document"
    );
    edit_right(&mut model, 3, "D");
    let (l, r, snapshot) = model.snapshot();
    assert!(!shared(&r, model.right()), "an edited side is copied");
    let TextOutcome::Ready(mut fresh) = rediff(l, r, &stale_request(), &CancelToken::new()) else {
        panic!("expected a model");
    };
    assert!(fresh.take_edits_from(&mut model, snapshot));
    let (left, right, _) = fresh.sources();
    assert!(
        shared(&left, fresh.left()) && shared(&right, fresh.right()),
        "a landed re-diff's documents hold the buffers' text"
    );
    assert_eq!(right.line(3), "D");
    edit_right(&mut fresh, 4, "E");
    let (_, right, _) = fresh.sources();
    assert!(!shared(&right, fresh.right()), "an edit since is copied");
    assert_eq!(right.line(4), "E");
    let buffer = fresh.buffer(false).unwrap();
    let (text, version) = (buffer.text().to_owned(), buffer.version());
    let path = fresh.right().path().to_path_buf();
    fresh.mark_written(false, &path, &text, version, fresh.right().stamp());
    let (_, right, _) = fresh.sources();
    assert!(shared(&right, fresh.right()), "a saved side's document");
    fresh.swap_sides();
    let (left, _, _) = fresh.sources();
    assert!(shared(&left, fresh.left()), "the hold follows a swap");
    assert_eq!(left.line(4), "E");
}

/// The re-diff's snapshot comes in two steps: copying the edited text on the UI thread, then
/// indexing its lines in the background. Together they give the one-step snapshot.
#[test]
fn a_snapshot_copies_the_text_first_and_indexes_it_later() {
    let (_dir, mut model) = sample();
    edit_right(&mut model, 3, "D");
    let begun = model.begin_snapshot();
    assert_eq!(begun.versions(), [None, Some(1)]);
    edit_right(&mut model, 4, "E");
    let (left, right, versions) = begun.finish();
    assert!(
        std::ptr::eq(&*left, model.left()),
        "an unedited side is shared"
    );
    assert_eq!(versions, [None, Some(1)]);
    assert_eq!(
        (right.line(3), right.line(4)),
        ("D".into(), "e".into()),
        "the text as it was when the snapshot began"
    );
    assert_eq!(right.line_count(), 8);
    assert_eq!(right.read_only_reason(), None);
    assert_eq!(right.stamp(), model.right().stamp());
    assert_eq!(right.path(), model.right().path());
}

#[test]
fn line_text_and_count_follow_an_edit_that_adds_a_line() {
    let (_dir, mut model) = sample();
    assert_eq!(model.line_count(false), 8);
    model.begin_edit(false).unwrap();
    model.replace(false, 0..0, "new\n").unwrap();
    assert_eq!(model.line_count(false), 9);
    assert_eq!(model.line_count(true), 7);
    assert_eq!(model.line_text(false, 0), "new");
    assert_eq!(model.line_text(false, 8), "h");
    assert_eq!(model.row_of_line(false, 8).map(|_| ()), Some(()));
    assert!(model.row_of_line(false, 9).is_none());
}

#[test]
fn versions_report_each_sides_buffer() {
    let (_dir, mut model) = sample();
    assert_eq!(model.versions(), [None, None]);
    edit_right(&mut model, 3, "D");
    let [left, right] = model.versions();
    assert_eq!(left, None);
    assert_eq!(right, Some(model.buffer(false).unwrap().version()));
}

#[test]
fn a_rediffed_snapshot_takes_over_the_edits() {
    use crate::text::job::{TextOutcome, rediff};
    let (_dir, mut model) = sample();
    edit_right(&mut model, 3, "D");
    let (l, r, snapshot) = model.snapshot();
    let TextOutcome::Ready(mut fresh) = rediff(l, r, &stale_request(), &CancelToken::new()) else {
        panic!("expected a model");
    };
    assert!(!fresh.is_dirty(false) && fresh.buffer(false).is_none());
    assert!(fresh.take_edits_from(&mut model, snapshot));
    assert!(fresh.is_dirty(false) && !fresh.is_dirty(true));
    assert_eq!(fresh.versions(), snapshot);
    assert_eq!(fresh.line_text(false, 3), "D");
    assert!(model.buffer(false).is_none(), "the buffers moved over");
}

#[test]
fn a_rediff_keeps_the_current_change_within_its_blocks() {
    use crate::text::job::{TextOutcome, rediff};
    let (_dir, mut model) = sample();
    model.last_change();
    assert_eq!(model.current_block(), Some(1));
    // Line 1 matches again: the provisional rows still show a change there, the re-diff not.
    edit_right(&mut model, 1, "b");
    let (l, r, snapshot) = model.snapshot();
    let TextOutcome::Ready(mut fresh) = rediff(l, r, &stale_request(), &CancelToken::new()) else {
        panic!("expected a model");
    };
    assert_eq!((model.block_count(), fresh.block_count()), (2, 1));
    assert!(fresh.take_edits_from(&mut model, snapshot));
    assert_eq!(
        fresh.current_block(),
        Some(0),
        "kept, and clamped to the one block left"
    );
}

#[test]
fn a_stale_snapshot_takes_nothing() {
    use crate::text::job::{TextOutcome, rediff};
    let (_dir, mut model) = sample();
    edit_right(&mut model, 3, "D");
    let (l, r, snapshot) = model.snapshot();
    let TextOutcome::Ready(mut fresh) = rediff(l, r, &stale_request(), &CancelToken::new()) else {
        panic!("expected a model");
    };
    edit_right(&mut model, 4, "E");
    assert!(!fresh.take_edits_from(&mut model, snapshot));
    assert!(fresh.buffer(false).is_none() && !fresh.any_dirty());
    assert!(
        model.buffer(false).is_some(),
        "the old model keeps its edits"
    );
}

#[test]
fn saving_cleans_the_side_and_the_document_holds_the_saved_text() {
    let (_dir, mut model) = sample();
    edit_right(&mut model, 3, "D");
    let stamp = FileStamp {
        len: 1234,
        modified: None,
    };
    model.mark_saved(false, stamp);
    assert!(!model.is_dirty(false));
    assert_eq!(model.right().stamp(), stamp);
    assert!(model.right().text().unwrap().contains("\nD\n"));
    edit_right(&mut model, 4, "E");
    assert!(model.is_dirty(false), "a later edit dirties it again");
}

#[test]
fn a_write_for_another_file_changes_nothing() {
    let (_dir, mut model) = sample();
    edit_right(&mut model, 3, "D");
    let before = model.right().stamp();
    let other = model.left().path().to_path_buf();
    let stamp = FileStamp {
        len: 99,
        modified: None,
    };
    let version = model.buffer(false).unwrap().version();
    model.mark_written(false, &other, "elsewhere\n", version, stamp);
    assert!(model.is_dirty(false), "the buffer is not marked clean");
    assert_eq!(model.right().stamp(), before);
    assert!(!model.right().text().unwrap().contains("elsewhere"));
    let own = model.right().path().to_path_buf();
    model.mark_written(false, &own, "mine\n", version, stamp);
    assert!(!model.is_dirty(false));
    assert_eq!(model.right().stamp(), stamp);
}

#[test]
fn the_on_disk_stamp_survives_a_rediff_and_follows_a_swap() {
    use crate::text::job::{TextOutcome, rediff};
    let (_dir, mut model) = sample();
    edit_right(&mut model, 3, "D");
    let (l, r, snapshot) = model.snapshot();
    let TextOutcome::Ready(mut fresh) = rediff(l, r, &stale_request(), &CancelToken::new()) else {
        panic!("expected a model");
    };
    // A save lands while the re-diff runs; the re-diff's documents carry the load stamp.
    let written = FileStamp {
        len: 4321,
        modified: None,
    };
    let (path, text) = (
        model.right().path().to_path_buf(),
        model.buffer(false).unwrap().text().to_owned(),
    );
    let version = model.buffer(false).unwrap().version();
    model.mark_written(false, &path, &text, version, written);
    assert!(fresh.take_edits_from(&mut model, snapshot));
    assert_eq!(fresh.disk_stamp(false), written);
    let left = fresh.disk_stamp(true);
    fresh.swap_sides();
    assert_eq!(
        (fresh.disk_stamp(true), fresh.disk_stamp(false)),
        (written, left)
    );
}

#[test]
fn swapping_sides_carries_buffers_and_clean_state() {
    let (_dir, mut model) = sample();
    edit_right(&mut model, 3, "D");
    model.mark_saved(false, model.right().stamp());
    edit_right(&mut model, 4, "E");
    assert!(model.is_dirty(false));
    model.swap_sides();
    assert!(model.is_dirty(true) && !model.is_dirty(false));
    assert!(model.buffer(true).is_some() && model.buffer(false).is_none());
    assert_eq!(model.line_text(true, 3), "D");
    assert_eq!(model.line_text(true, 4), "E");
}

#[test]
fn read_only_sides_refuse_edits_with_a_reason() {
    let (_dir, model) = sample();
    let (left, right, _) = model.sources();
    let blocked = TextModel::new(
        Arc::new(left.formatted("x".into())),
        right,
        LineDiff {
            hunks: vec![],
            left_lines: 1,
            right_lines: 8,
            ..LineDiff::default()
        },
    );
    assert!(blocked.editable(true).unwrap_err().contains("formatted"));
    assert!(blocked.editable(false).is_ok());
}

#[test]
fn filters_select_rows_from_change_blocks() {
    let (_dir, mut model) = sample();
    assert_eq!(model.blocks(), &[1..2, 6..8]);
    assert_eq!(model.row_count(), 8);
    model.set_filter(TextFilter::Differences);
    assert_eq!(model.row_count(), 3);
    assert_eq!(model.row(2).unwrap().kind, RowKind::RightOnly);
    model.set_filter(TextFilter::Context(1));
    assert_eq!(
        model.row_count(),
        7,
        "rows 0-2, one fold for rows 3-4, rows 5-7"
    );
    model.set_filter(TextFilter::Same);
    assert_eq!(model.row_count(), 5);
    assert_eq!(model.row(1).unwrap().left, Some(2));
}

#[test]
fn navigation_moves_between_blocks_and_survives_a_filter_change() {
    let (_dir, mut model) = sample();
    assert_eq!(model.next_change(), Some(1));
    assert!(model.in_current_block(1) && !model.in_current_block(2));
    assert_eq!(model.next_change(), Some(6));
    assert_eq!(model.next_change(), None, "the plain step stops at the end");
    assert_eq!(model.prev_change(), Some(1));
    assert_eq!(model.last_change(), Some(6));
    model.set_filter(TextFilter::Differences);
    assert_eq!(
        model.current_display(),
        Some(1),
        "block 2 starts at display row 1"
    );
    assert_eq!(model.first_change(), Some(0));
    model.set_filter(TextFilter::Same);
    assert_eq!(
        model.next_change(),
        None,
        "changes are hidden by the Same filter"
    );
}

#[test]
fn wrapping_navigation_starts_over_and_says_so() {
    let (_dir, mut model) = sample();
    assert_eq!(
        model.prev_change_wrapping(),
        Some(Landing {
            display: 6,
            wrapped: false
        }),
        "with nothing current, F7 goes to the last block without calling it a wrap"
    );
    assert_eq!(
        model.next_change_wrapping(),
        Some(Landing {
            display: 1,
            wrapped: true
        })
    );
    assert_eq!(
        model.next_change_wrapping(),
        Some(Landing {
            display: 6,
            wrapped: false
        })
    );
    assert_eq!(
        model.prev_change_wrapping(),
        Some(Landing {
            display: 1,
            wrapped: false
        })
    );
    assert_eq!(
        model.prev_change_wrapping(),
        Some(Landing {
            display: 6,
            wrapped: true
        })
    );
    model.set_filter(TextFilter::Same);
    assert_eq!(
        model.next_change_wrapping(),
        None,
        "no visible block, nowhere to go"
    );
}

#[test]
fn stats_count_changed_and_one_sided_lines() {
    let (_dir, model) = sample();
    let stats = model.stats();
    assert_eq!(
        (
            stats.hunks,
            stats.changed,
            stats.left_only,
            stats.right_only
        ),
        (2, 2, 0, 1)
    );
    assert!(
        (stats.similarity - 10.0 / 15.0).abs() < 1e-6,
        "{}",
        stats.similarity
    );
    assert!(
        status_text(&model).starts_with("2 differences"),
        "{}",
        status_text(&model)
    );
}

/// A model of `left` and `right` compared under `rules` (blank lines etc.).
fn model_with_rules(
    left: &str,
    right: &str,
    rules: &duet_core::text::rules::TextRules,
) -> (tempfile::TempDir, TextModel) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, left).unwrap();
    std::fs::write(&b, right).unwrap();
    let cancel = CancelToken::new();
    let (l, r) = (
        TextDocument::open(&a, &cancel).unwrap(),
        TextDocument::open(&b, &cancel).unwrap(),
    );
    let (diff, applied) = duet_core::text::rules::diff_with_rules(
        DiffInput {
            bytes: l.bytes(),
            lines: l.lines(),
        },
        DiffInput {
            bytes: r.bytes(),
            lines: r.lines(),
        },
        DiffOptions::default(),
        rules,
        &cancel,
    )
    .unwrap();
    assert!(applied);
    (dir, TextModel::new(l, r, diff))
}

#[test]
fn unimportant_lines_count_and_similarity_follows_localdiff() {
    let blank = duet_core::text::rules::TextRules {
        ignore_blank: true,
        ..Default::default()
    };
    let (_dir, model) = model_with_rules("a\n\nb\n", "a\nb\n", &blank);
    let stats = model.stats();
    assert_eq!((stats.hunks, stats.unimportant), (0, 1));
    assert!(
        (stats.similarity - 0.8).abs() < 1e-6,
        "2 × 2 paired rows / 5 lines: {}",
        stats.similarity
    );
    assert_eq!(
        status_text(&model),
        "Same under the rules · 1 unimportant line · 3 lines · showing All"
    );
    let (_dir, model) = model_with_rules("a\n\n\nb\n", "a\nb\n", &blank);
    assert_eq!(
        status_text(&model),
        "Same under the rules · 2 unimportant lines · 4 lines · showing All"
    );
    // A paired blank line whose spaces differ is unimportant too (soft_pairs).
    let (_dir, model) = model_with_rules("a\n\nb\n", "a\n  \nb\n", &blank);
    let stats = model.stats();
    assert_eq!((stats.unimportant, stats.similarity), (1, 1.0));

    // With a difference, the count goes after "right only" (LocalDiff: 2 × 2 / 7).
    let (_dir, model) = model_with_rules("a\n\nb\nc\n", "a\nb\nd\n", &blank);
    assert_eq!(
        status_text(&model),
        "1 difference · 1 changed · 0 left only · 0 right only · 1 unimportant · 57.1% same \
         · showing All"
    );
    let parts = status_parts(&model);
    let tone = parts
        .iter()
        .find(|p| p.text == "1 unimportant")
        .map(|p| p.tone);
    assert_eq!(tone, Some(Tone::Unimportant));
    let (_dir, plain) = model_for("a\nb\n", "a\nc\n");
    assert_eq!(plain.stats().unimportant, 0);
    assert!(!status_text(&plain).contains("unimportant"));
}

#[test]
fn the_identical_status_counts_the_edited_lines() {
    let (_dir, mut model) = model_for("a\nx\n", "a\n");
    model.begin_edit(true).unwrap();
    let start = model.buffer(true).unwrap().line_start(1);
    // Deleting the left-only line leaves no hunk before the re-diff lands.
    model.replace(true, start..start + 2, "").unwrap();
    assert_eq!(status_text(&model), "Identical · 1 line · showing All");
}

#[test]
fn one_difference_reads_in_the_singular() {
    let (_dir, model) = model_for(
        "a
", "b
",
    );
    assert!(
        status_text(&model).starts_with("1 difference · 1 changed"),
        "{}",
        status_text(&model)
    );
    let (_dir, model) = model_for(
        "a
b
", "x
y
z
",
    );
    assert!(status_text(&model).starts_with("1 difference ·"));
    let (_dir, model) = model_for(
        "a
b
c
", "x
b
y
",
    );
    assert!(status_text(&model).starts_with("2 differences ·"));
}

#[test]
fn the_status_counts_blocks_then_lines_with_units() {
    let (_dir, model) = sample();
    assert_eq!(
        status_text(&model),
        "2 differences · 2 changed · 0 left only · 1 right only · 66.7% same · showing All"
    );
}

#[test]
fn status_parts_colour_the_line_counts() {
    let (_dir, model) = sample();
    let parts = status_parts(&model);
    let tone = |text: &str| parts.iter().find(|p| p.text == text).map(|p| p.tone);
    assert_eq!(tone("2 changed"), Some(Tone::Changed));
    assert_eq!(tone("0 left only"), Some(Tone::LeftOnly));
    assert_eq!(tone("1 right only"), Some(Tone::RightOnly));
    assert_eq!(tone("2 differences"), Some(Tone::Muted));
}

#[test]
fn inline_highlights_only_for_changed_rows() {
    let (_dir, mut model) = model_for("ภาษา ไทย\nsame\n", "ภาษา ลาว\nsame\n");
    let inline = model.inline(0).expect("row 0 changed");
    assert_eq!(&"ภาษา ไทย"[inline.left[0].clone()], "ไทย");
    assert!(model.inline(1).is_none());
}

#[test]
fn hidden_differences_are_refined_only_where_matched_text_differs() {
    let options = DiffOptions {
        ignore_case: true,
        ..DiffOptions::default()
    };
    let (_dir, mut model) = model_with(b"Hello\nsame\nx\n", b"hello\nsame\ny\n", options);
    let hidden = model.hidden_inline_at(0).expect("row 0 differs in case");
    assert_eq!(&"Hello"[hidden.left[0].clone()], "H");
    assert!(model.inline_at(0).is_none(), "row 0 is not a changed pair");
    assert!(
        model.hidden_inline_at(1).is_none(),
        "the same text hides nothing"
    );
    assert!(model.hidden_inline_at(2).is_none(), "row 2 is changed");
    assert!(model.inline_at(2).is_some());
}

#[test]
fn context_filter_over_ten_million_rows_stores_ranges_not_rows() {
    let hunks: Vec<LineHunk> = (0..1_000u32)
        .map(|i| LineHunk {
            left: i * 10_000..i * 10_000 + 1,
            right: i * 10_000..i * 10_000 + 1,
        })
        .collect();
    let diff = LineDiff {
        hunks,
        left_lines: 10_000_000,
        right_lines: 10_000_000,
        ..LineDiff::default()
    };
    let rows = RowMap::new(&diff);
    let visible = visible_rows(
        &rows.change_blocks(),
        rows.row_count(),
        TextFilter::Context(3),
    );
    assert_eq!(visible.range_count(), 1_000);
    assert_eq!(
        visible.len(),
        1_000 * 7 - 3,
        "the first block has no rows above it"
    );
    // The first block (row 0) shows rows 0-3, so the second one starts at display row 4.
    assert_eq!(visible.row(4), Some(10_000 - 3));
    assert_eq!(visible.display_of(10_000), Some(4 + 3));
    assert_eq!(visible.display_of(5_000), None);
    let all = visible_rows(&rows.change_blocks(), rows.row_count(), TextFilter::All);
    assert_eq!((all.range_count(), all.len()), (1, 10_000_000));
}

#[test]
fn context_folds_hidden_rows_and_expands_them() {
    let (_dir, mut model) = sample();
    model.set_filter(TextFilter::Context(1));
    assert_eq!(model.display_row(3), DisplayRow::Fold { start: 3, end: 5 });
    assert_eq!(model.row(3), None);
    assert_eq!(model.visible().display_of(6), Some(5));
    assert_eq!(
        model.visible().display_of(4),
        None,
        "a folded row has no display row"
    );
    model.expand_fold(3, Some(1));
    assert_eq!(model.display_row(3), DisplayRow::Row(3));
    assert_eq!(
        model.display_row(4),
        DisplayRow::Row(4),
        "the one row left over shows itself"
    );
    assert_eq!((model.row_count(), model.visible().fold_count()), (8, 0));
    model.set_filter(TextFilter::Differences);
    assert_eq!(
        model.row_count(),
        3,
        "folds belong to the Context filter only"
    );
    model.set_filter(TextFilter::Context(1));
    assert_eq!(model.row_count(), 8, "unfolded rows stay unfolded");
}

#[test]
fn a_single_hidden_line_shows_instead_of_folding() {
    // rows: 0 a→A | 1-3 same | 4 e→E; Context(1) leaves only row 2 hidden.
    let (_dir, mut model) = model_for("a\nb\nc\nd\ne\n", "A\nb\nc\nd\nE\n");
    model.set_filter(TextFilter::Context(1));
    assert_eq!(model.visible().fold_count(), 0);
    assert_eq!(model.row_count(), 5);
}

#[test]
fn expanded_rows_survive_a_new_model_clamped_to_its_rows() {
    let (_dir, mut model) = sample();
    model.set_expanded(vec![3..5, 7..40]);
    model.set_filter(TextFilter::Context(1));
    assert_eq!(model.expanded(), &[3..5, 7..8]);
    assert_eq!(model.visible().fold_count(), 0);
}

#[test]
fn swapping_keeps_rows_and_flips_sides() {
    let (_dir, mut model) = sample();
    model.set_filter(TextFilter::Context(1));
    model.next_change();
    let before: Vec<_> = (0..model.row_count())
        .map(|d| model.display_row(d))
        .collect();
    model.swap_sides();
    let after: Vec<_> = (0..model.row_count())
        .map(|d| model.display_row(d))
        .collect();
    assert_eq!(after, before);
    assert_eq!(model.current_block(), Some(0));
    assert_eq!(model.left().line(1), "B");
    model.set_filter(TextFilter::All);
    assert_eq!(
        model.row(7).unwrap().kind,
        RowKind::LeftOnly,
        "the right-only \"h\" is now on the left"
    );
    let inline = model.inline(1).unwrap();
    assert_eq!(inline.left.len(), 1);
}

#[test]
fn doc_info_names_encoding_and_line_endings() {
    let (_dir, model) = model_for("a\r\nb\r\n", "a\nb\n");
    assert_eq!(doc_info(model.left()), "UTF-8 · CRLF");
    assert_eq!(doc_info(model.right()), "UTF-8 · LF");
}

fn apply_copy(model: &mut TextModel, to_left: bool, (range, text): (Range<usize>, String)) {
    model.replace(to_left, range, &text).unwrap();
}

#[test]
fn copying_every_block_makes_the_sides_equal() {
    let cases = [
        ("a\nb\nc\n", "a\nX\nc\nd\n"),
        ("only left\n", ""),
        ("x\r\ny\r\n", "x\r\nz\r\nextra\r\n"),
        ("same\nend", "same\nother end"),
        ("same\nother end", "same\nend"),
        ("a\nb", "a"),
        ("a", "a\nb"),
    ];
    for (left, right) in cases {
        let (_dir, mut model) = model_for(left, right);
        // Last block first, so earlier block numbers stay valid.
        for block in (0..model.block_count()).rev() {
            let edit = model.copy_block_edit(block, false).unwrap();
            apply_copy(&mut model, false, edit);
        }
        let right_text = model
            .buffer(false)
            .map_or_else(|| right.to_owned(), |b| b.text().to_owned());
        assert_eq!(
            right_text.replace("\r\n", "\n"),
            left.replace("\r\n", "\n"),
            "{left:?} -> {right:?}"
        );
    }
}

#[test]
fn copied_lines_take_the_targets_line_breaks() {
    let (_dir, mut model) = model_for("a\r\nnew\n", "a\r\n");
    let (range, text) = model.copy_block_edit(0, false).unwrap();
    assert_eq!(text, "new\r\n");
    assert_eq!(range, 3..3, "inserted after the right's line 0");
}

#[test]
fn a_side_that_did_not_decode_cleanly_is_never_copied_from() {
    // Odd-length UTF-16: decoded with a replacement character, so lossy.
    let (_dir, mut model) = model_for_bytes(&[0xFF, 0xFE, 0x41, 0x00, 0x42], b"x\n");
    assert!(model.left().is_lossy() && model.block_count() > 0);
    let refused = "This file did not decode cleanly, so it can't be copied from.";
    assert_eq!(model.copy_all_edit(false), Err(refused.to_owned()));
    assert_eq!(model.copy_block_edit(0, false), Err(refused.to_owned()));
}

#[test]
fn copy_all_replaces_the_whole_target() {
    let (_dir, mut model) = model_for("1\n2\n", "x\r\n");
    let (range, text) = model.copy_all_edit(false).unwrap();
    assert_eq!((range, text.as_str()), (0..3, "1\r\n2\r\n"));
}

#[test]
fn copy_rows_edit_takes_part_of_a_block() {
    // left a b c d, right a B C d: rows 1..2 is the b/B pair
    let (_dir, mut model) = model_for("a\nb\nc\nd\n", "a\nB\nC\nd\n");
    let (range, text) = model.copy_rows_edit(1..2, false).unwrap();
    assert_eq!(text, "b\n");
    assert_eq!(range, 2..4);
}

#[test]
fn delete_rows_edit_takes_whole_lines_and_the_break_before_an_open_end() {
    let (_dir, mut model) = model_for("a\r\nb\r\nc\r\n", "a\r\nb\r\nc\r\n");
    assert_eq!(model.delete_rows_edit(0..2, false).unwrap(), 0..6);
    let (_dir, mut model) = model_for("a\nb\nc", "a\nb\nc");
    // The last line has no break, so the one before it goes too.
    assert_eq!(model.delete_rows_edit(1..3, false).unwrap(), 1..5);
    assert_eq!(model.delete_rows_edit(0..3, false).unwrap(), 0..5);
}

#[test]
fn rows_text_joins_lines_with_the_sides_break_and_keeps_no_final_one() {
    let (_dir, model) = model_for("a\r\nb\r\nc\r\n", "x\ny\n");
    assert_eq!(model.rows_text(0..2, true), "a\r\nb");
    assert_eq!(model.rows_text(1..2, false), "y");
}

#[test]
fn the_revision_moves_with_every_change_to_the_rows() {
    let (_a, mut model) = sample();
    let (_b, other) = sample();
    assert_ne!(
        model.revision(),
        other.revision(),
        "a new model has its own"
    );
    let mut last = model.revision();
    let mut moved = |model: &TextModel| {
        let moved = model.revision() != last;
        last = model.revision();
        moved
    };
    model.set_filter(TextFilter::Context(1));
    assert!(moved(&model), "a filter");
    assert_eq!(model.display_row(3), DisplayRow::Fold { start: 3, end: 5 });
    model.expand_fold(3, None);
    assert!(moved(&model), "a fold opened");
    model.replace(true, 0..0, "new\n").unwrap();
    assert!(moved(&model), "an edit");
    model.swap_sides();
    assert!(moved(&model), "F9");
    assert!(!moved(&model), "reading it changes nothing");
}

#[test]
fn block_at_display_is_none_when_nothing_is_shown() {
    let same: String = (0..300).map(|i| format!("line {i}\n")).collect();
    let (_a, mut model) = model_for(&same, &same);
    model.set_filter(TextFilter::Differences);
    assert_eq!((model.row_count(), model.block_count()), (0, 0));
    let (_b, empty) = model_for("", "");
    assert_eq!(
        empty.row_count(),
        0,
        "two 0-line sides show no row under All"
    );
    for display in [0, 50] {
        assert_eq!(
            model.block_at_display(display),
            None,
            "identical, display {display}"
        );
        assert_eq!(
            empty.block_at_display(display),
            None,
            "empty, display {display}"
        );
    }
    let (_c, two) = sample();
    assert_eq!(
        two.block_at_display(1_000),
        Some(1),
        "past the end: the last block"
    );
}
