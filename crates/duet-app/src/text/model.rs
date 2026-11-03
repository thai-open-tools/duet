//! Text-view state without any GUI: which aligned rows show under a filter, the current change,
//! stats and a bounded cache of inline highlights. Everything works from change blocks, so a
//! 1 GB file with 30M rows costs O(hunks) here (Review Focus 1).

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use duet_core::text::buffer::{Change, TextBuffer};
use duet_core::text::diff::LineDiff;
use duet_core::text::document::{FileStamp, ReadOnlyReason, TextDocument};
use duet_core::text::encoding::TextEncoding;
use duet_core::text::inline::{InlineDiff, refine};
use duet_core::text::lines::{EolStyle, LineIndex};
use duet_core::text::moves::Moves;
use duet_core::text::rows::{Row, RowKind, RowMap};

use crate::format::{count_of, format_size};
use crate::status::{StatusPart, Tone, join_parts};

mod edits;
mod visible;

use visible::merge;
pub use visible::{DisplayRow, Visible, context_rows, visible_rows};

const INLINE_CACHE: usize = 4_096;

/// Revisions of every model in the process, so a new model never repeats an old one's.
static REVISION: AtomicU64 = AtomicU64::new(0);

fn next_revision() -> u64 {
    REVISION.fetch_add(1, Ordering::Relaxed) + 1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextFilter {
    #[default]
    All,
    Differences,
    Context(u32),
    Same,
}

impl TextFilter {
    pub fn label(self) -> String {
        match self {
            TextFilter::All => "All".into(),
            TextFilter::Differences => "Differences".into(),
            TextFilter::Context(n) => {
                format!("Differences + {}", count_of(u64::from(n), "line", "lines"))
            }
            TextFilter::Same => "Same".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    pub hunks: usize,
    pub changed: u32,
    pub left_only: u32,
    pub right_only: u32,
    /// Share of lines that match, 0.0–1.0 (1.0 for two empty files).
    pub similarity: f32,
    /// Lines the rules or options hid: one-sided soft rows and paired rows whose text differs.
    pub unimportant: u32,
}

/// Where a navigation step landed, and whether it went past an end to get there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Landing {
    pub display: usize,
    pub wrapped: bool,
}

/// What a re-diff takes from the model on the UI thread: each side's document, or an edited
/// side's text, copied but not yet indexed. Indexing a large side's lines is a full scan, so
/// `finish` belongs off the UI thread.
pub struct Snapshot {
    sides: [SnapshotSide; 2],
    versions: [Option<u64>; 2],
}

enum SnapshotSide {
    /// A document that already holds the side's text.
    Shared(Arc<TextDocument>),
    /// The buffer's text, to become an edited copy of `base`.
    Copied {
        base: Arc<TextDocument>,
        text: String,
    },
}

impl Snapshot {
    /// The buffer versions the snapshot holds.
    pub fn versions(&self) -> [Option<u64>; 2] {
        self.versions
    }

    /// Both sides as documents, and the versions they hold.
    pub fn finish(self) -> (Arc<TextDocument>, Arc<TextDocument>, [Option<u64>; 2]) {
        FINISHED.with(|finished| finished.set(finished.get() + 1));
        let document = |side: SnapshotSide| match side {
            SnapshotSide::Shared(doc) => doc,
            SnapshotSide::Copied { base, text } => Arc::new(base.edited(text, base.stamp())),
        };
        let [left, right] = self.sides;
        (document(left), document(right), self.versions)
    }
}

thread_local! {
    /// Snapshots finished on this thread, for the tests: a UI-thread caller that only begins
    /// one leaves the line indexing to the background.
    static FINISHED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Test hook: how many snapshots this thread has finished.
#[doc(hidden)]
pub fn snapshots_finished_here() -> usize {
    FINISHED.with(std::cell::Cell::get)
}

pub struct TextModel {
    left: Arc<TextDocument>,
    right: Arc<TextDocument>,
    diff: LineDiff,
    rows: Arc<RowMap>,
    blocks: Vec<Range<u32>>,
    filter: TextFilter,
    visible: Visible,
    current: Option<usize>,
    inline: HashMap<u32, Arc<InlineDiff>>,
    /// Rows the user unfolded under the Context filter.
    expanded: Vec<Range<u32>>,
    moves: Moves,
    /// Notes for the status line, e.g. "Left: formatted JSON".
    notes: Vec<String>,
    /// Editable text per side, indexed by `usize::from(!left)`; made on the first edit.
    buffers: [Option<TextBuffer>; 2],
    /// The buffer version that matches what is on disk, per side.
    clean: [u64; 2],
    /// Each file's stamp as last read or written, per side: what a save checks the disk
    /// against. The documents behind the rows may be a re-diff's copies with an older stamp.
    disk: [FileStamp; 2],
    /// The buffer version each side's document holds, if any: the buffer was made from it, a
    /// re-diff of a snapshot at that version landed, or that version was saved. While it matches
    /// the buffer, the document stands in for the buffer's text without a copy.
    doc_version: [Option<u64>; 2],
    /// A save wrote the side with other line breaks than its text holds (R13, PF5).
    converted: [bool; 2],
    /// Changes with the rows or the visible map (see `revision`).
    revision: u64,
}

impl TextModel {
    pub fn new(
        left: impl Into<Arc<TextDocument>>,
        right: impl Into<Arc<TextDocument>>,
        diff: LineDiff,
    ) -> Self {
        let rows = RowMap::new(&diff);
        let blocks = rows.change_blocks();
        let visible = visible_rows(&blocks, rows.row_count(), TextFilter::All);
        let (left, right) = (left.into(), right.into());
        Self {
            disk: [left.stamp(), right.stamp()],
            left,
            right,
            diff,
            rows: Arc::new(rows),
            blocks,
            filter: TextFilter::All,
            visible,
            current: None,
            inline: HashMap::new(),
            expanded: Vec::new(),
            moves: Moves::default(),
            notes: Vec::new(),
            buffers: [None, None],
            clean: [0, 0],
            doc_version: [None, None],
            converted: [false, false],
            revision: next_revision(),
        }
    }

    /// A process-wide counter that moves on with every change to the rows or the visible map:
    /// a new model, an edit, F9, a filter or a fold. Each of those ends in `set_filter`.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn with_notes(mut self, notes: Vec<String>) -> Self {
        self.notes = notes;
        self
    }

    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Exchanges the sides in place (F9). A hunk of `r` removed and `a` added lines takes
    /// max(r, a) rows either way, so row numbers, the filter, folds and the current block all
    /// stay; only the side-specific parts flip.
    pub fn swap_sides(&mut self) {
        std::mem::swap(&mut self.left, &mut self.right);
        self.buffers.swap(0, 1);
        self.clean.swap(0, 1);
        self.disk.swap(0, 1);
        self.doc_version.swap(0, 1);
        self.converted.swap(0, 1);
        self.diff = self.diff.swapped();
        self.rows = Arc::new(RowMap::new(&self.diff));
        self.blocks = self.rows.change_blocks();
        self.moves = self.moves.swapped();
        self.inline.clear();
        self.set_filter(self.filter);
    }

    pub fn with_moves(mut self, moves: Moves) -> Self {
        self.moves = moves;
        self
    }

    pub fn moves(&self) -> &Moves {
        &self.moves
    }

    /// The other side's line that `display`'s line on `left` moved to or from.
    pub fn moved_partner(&self, display: usize, left: bool) -> Option<u32> {
        self.moved_partner_of(self.row(display)?, left)
    }

    pub fn left(&self) -> &TextDocument {
        &self.left
    }

    pub fn right(&self) -> &TextDocument {
        &self.right
    }

    pub(crate) fn side(&self, left: bool) -> &Arc<TextDocument> {
        if left { &self.left } else { &self.right }
    }

    /// Why a side cannot be edited, or Ok.
    pub fn editable(&self, left: bool) -> Result<(), String> {
        let doc = self.side(left);
        match doc.read_only_reason() {
            Some(ReadOnlyReason::TooLarge) => {
                Err("This file is over 64 MiB, so it is read-only.".into())
            }
            Some(ReadOnlyReason::LossyDecode) => {
                Err("This file has bytes the encoding cannot read, so it is read-only.".into())
            }
            Some(ReadOnlyReason::Formatted) => {
                Err("The formatted view is read-only. Turn Format off to edit.".into())
            }
            Some(ReadOnlyReason::Attribute) => {
                Err("This file is read-only (Windows attribute).".into())
            }
            None if doc.text().is_none() => Err("This file is read-only.".into()),
            None => Ok(()),
        }
    }

    pub fn buffer(&self, left: bool) -> Option<&TextBuffer> {
        self.buffers[usize::from(!left)].as_ref()
    }

    /// The side's document while it holds the side's text: there is no buffer, or the buffer
    /// is at the version the document holds. None while the buffer is ahead of it.
    pub fn current_document(&self, left: bool) -> Option<&Arc<TextDocument>> {
        match self.buffer(left) {
            Some(buffer) if self.doc_version[usize::from(!left)] != Some(buffer.version()) => None,
            _ => Some(self.side(left)),
        }
    }

    /// A save wrote this side with other line breaks than its text holds, so an undo history
    /// made in the text does not fit what F5 reads.
    pub fn converted(&self, left: bool) -> bool {
        self.converted[usize::from(!left)]
    }

    /// Makes the side editable: copies its text into a buffer the first time. The copy keeps
    /// room to grow (a sixteenth, at least 64 KiB), so the first keystrokes into a large file
    /// never copy it again to a bigger allocation.
    pub fn begin_edit(&mut self, left: bool) -> Result<(), String> {
        self.editable(left)?;
        let index = usize::from(!left);
        if self.buffers[index].is_none() {
            let source = self.side(left).text().unwrap_or_default();
            let mut text = String::with_capacity(source.len() + (source.len() / 16).max(64 << 10));
            text.push_str(source);
            self.buffers[index] = Some(TextBuffer::new(text));
            self.clean[index] = 0;
            self.doc_version[index] = Some(0);
        }
        Ok(())
    }

    /// Replaces `range` of one side's buffer and splices the rows so they follow at once.
    pub fn replace(
        &mut self,
        left: bool,
        range: Range<usize>,
        text: &str,
    ) -> Result<Change, String> {
        self.begin_edit(left)?;
        let change = self.buffers[usize::from(!left)]
            .as_mut()
            .expect("begun")
            .replace(range, text);
        self.diff = self
            .diff
            .splice(left, change.line, change.old_lines, change.new_lines);
        self.rows = Arc::new(RowMap::new(&self.diff));
        self.blocks = self.rows.change_blocks();
        self.inline.clear();
        self.moves = Moves::default();
        self.current = self.current.filter(|&b| b < self.blocks.len());
        self.set_filter(self.filter);
        Ok(change)
    }

    pub fn line_text(&self, left: bool, line: usize) -> Cow<'_, str> {
        match self.buffer(left) {
            Some(buffer) => Cow::Borrowed(buffer.line(line)),
            None => self.side(left).line(line),
        }
    }

    pub fn line_count(&self, left: bool) -> usize {
        self.buffer(left)
            .map_or_else(|| self.side(left).line_count(), TextBuffer::line_count)
    }

    pub fn is_dirty(&self, left: bool) -> bool {
        let index = usize::from(!left);
        self.buffers[index]
            .as_ref()
            .is_some_and(|b| b.version() != self.clean[index])
    }

    pub fn any_dirty(&self) -> bool {
        self.is_dirty(true) || self.is_dirty(false)
    }

    pub fn versions(&self) -> [Option<u64>; 2] {
        [
            self.buffer(true).map(TextBuffer::version),
            self.buffer(false).map(TextBuffer::version),
        ]
    }

    /// Documents for a re-diff: edited sides become writable documents holding the buffer.
    pub fn snapshot(&self) -> (Arc<TextDocument>, Arc<TextDocument>, [Option<u64>; 2]) {
        self.begin_snapshot().finish()
    }

    /// The UI thread's part of `snapshot`: copies the text of each side whose document lags its
    /// buffer, and shares every other document. `Snapshot::finish` indexes the copies' lines.
    pub fn begin_snapshot(&self) -> Snapshot {
        let side = |left: bool| match self.buffer(left) {
            Some(buffer) if self.doc_version[usize::from(!left)] != Some(buffer.version()) => {
                SnapshotSide::Copied {
                    base: self.side(left).clone(),
                    text: buffer.text().to_owned(),
                }
            }
            _ => SnapshotSide::Shared(self.side(left).clone()),
        };
        Snapshot {
            sides: [side(true), side(false)],
            versions: self.versions(),
        }
    }

    /// After a re-diff of `old`'s snapshot: take over its buffers, saved versions and on-disk
    /// stamps (a save may have landed while the re-diff ran), filter, unfolded rows and current
    /// change (clamped to the new blocks, so F7/F8 go on from about where they were). Returns
    /// false (and takes nothing) when `old` was edited since. This model's documents are the
    /// snapshot's, so they hold the buffers' text at the snapshot's versions.
    pub fn take_edits_from(&mut self, old: &mut TextModel, snapshot: [Option<u64>; 2]) -> bool {
        if old.versions() != snapshot {
            return false;
        }
        self.buffers = std::mem::take(&mut old.buffers);
        self.doc_version = snapshot;
        self.clean = old.clean;
        self.disk = old.disk;
        self.converted = old.converted;
        self.set_expanded(old.expanded().to_vec());
        self.set_filter(old.filter());
        self.current = old
            .current
            .zip(self.blocks.len().checked_sub(1))
            .map(|(block, last)| block.min(last));
        true
    }

    /// What the side's file was stamped when it was last read or written.
    pub fn disk_stamp(&self, left: bool) -> FileStamp {
        self.disk[usize::from(!left)]
    }

    /// The side was written to disk with `stamp`: its buffer is clean again.
    pub fn mark_saved(&mut self, left: bool, stamp: FileStamp) {
        let index = usize::from(!left);
        if let Some(buffer) = &self.buffers[index] {
            self.clean[index] = buffer.version();
            self.disk[index] = stamp;
            self.doc_version[index] = Some(buffer.version());
            let doc = self.side(left).edited(buffer.text().to_owned(), stamp);
            if left {
                self.left = Arc::new(doc);
            } else {
                self.right = Arc::new(doc);
            }
        }
    }

    /// `text`, the buffer at `version`, was written to disk with `stamp`. The on-disk stamp
    /// (and the document) take it, so the next save checks against our own write; the side
    /// stays dirty when the buffer has moved on since. Does nothing when the side now holds
    /// another file (a reload while the save ran): the write belongs to `path` only. The caller
    /// follows F9 presses made while the save ran to the side the file moved to.
    pub fn mark_written(
        &mut self,
        left: bool,
        path: &std::path::Path,
        text: &str,
        version: u64,
        stamp: FileStamp,
    ) {
        let index = usize::from(!left);
        if self.buffers[index].is_some() && self.side(left).path() == path {
            self.clean[index] = version;
            self.disk[index] = stamp;
            self.doc_version[index] = Some(version);
            let doc = self.side(left).edited(text.to_owned(), stamp);
            if left {
                self.left = Arc::new(doc);
            } else {
                self.right = Arc::new(doc);
            }
        }
    }

    /// Save As wrote `text` to `path` with `stamp`: the side now refers to that file. With a
    /// buffer (`version` is its version when the write began) the side is clean at `version`
    /// and its document holds `text`; without one only the path changes.
    pub fn mark_saved_as(
        &mut self,
        left: bool,
        path: std::path::PathBuf,
        text: &str,
        version: Option<u64>,
        stamp: FileStamp,
    ) {
        let index = usize::from(!left);
        let doc = match version.filter(|_| self.buffers[index].is_some()) {
            Some(version) => {
                self.clean[index] = version;
                self.doc_version[index] = Some(version);
                self.side(left)
                    .edited(text.to_owned(), stamp)
                    .with_path(path)
            }
            None => self.side(left).with_path(path).with_stamp(stamp),
        };
        self.disk[index] = stamp;
        if left {
            self.left = Arc::new(doc);
        } else {
            self.right = Arc::new(doc);
        }
    }

    /// A save wrote the side's file in `encoding` and `eol`, which are not the document's (the
    /// on-save choices, or the UTF-8 fallback): the document takes them, so a later save that
    /// keeps them does not convert back (PF5). Does nothing when the side now holds another file.
    pub fn set_encoding_after_save(
        &mut self,
        left: bool,
        path: &std::path::Path,
        encoding: TextEncoding,
        eol: EolStyle,
    ) {
        let doc = self.side(left);
        if doc.path() != path || (doc.encoding(), doc.eol()) == (encoding, eol) {
            return;
        }
        let converted = doc.eol() != eol;
        let doc = Arc::new(doc.with_encoding(encoding).with_eol(eol));
        if converted {
            self.converted[usize::from(!left)] = true;
        }
        if left {
            self.left = doc;
        } else {
            self.right = doc;
        }
    }

    pub fn blocks(&self) -> &[Range<u32>] {
        &self.blocks
    }

    pub fn block_rows(&self, block: usize) -> Option<Range<u32>> {
        self.blocks.get(block).cloned()
    }

    /// The block whose first row is `row`.
    pub fn clear_current_block(&mut self) {
        self.current = None;
    }

    pub fn block_starting_at(&self, row: u32) -> Option<usize> {
        self.blocks.binary_search_by_key(&row, |b| b.start).ok()
    }

    /// One side's whole text (its buffer when edited) and line index.
    fn side_text(&self, left: bool) -> Option<(&str, &LineIndex)> {
        match self.buffer(left) {
            Some(buffer) => Some((buffer.text(), buffer.lines())),
            None => {
                let doc = self.side(left);
                doc.text().map(|text| (text, doc.lines()))
            }
        }
    }

    /// Where `line` starts on one side (its buffer when edited); past the last line, the end.
    pub(crate) fn line_offset(&self, left: bool, line: usize) -> usize {
        let (len, index) = match self.buffer(left) {
            Some(buffer) => (buffer.len(), buffer.lines()),
            None => {
                let doc = self.side(left);
                (doc.bytes().len(), doc.lines())
            }
        };
        if line < index.len() {
            index.span_with_eol(line).start
        } else {
            len
        }
    }

    /// Byte span of `lines` on one side, terminators included. The flag says the span ends
    /// the text without a final break.
    fn span(&self, left: bool, lines: &Range<u32>) -> (Range<usize>, bool) {
        let (text, _) = self.side_text(left).expect("a text side");
        let offset = |line: u32| self.line_offset(left, line as usize);
        let span = offset(lines.start)..offset(lines.end);
        let open = !span.is_empty() && span.end == text.len() && !text.ends_with(['\n', '\r']);
        (span, open)
    }

    /// The edit that makes block `block` of the target side (`to_left`) match the other side:
    /// the target byte range and the text for it, with the target's line breaks.
    pub fn copy_block_edit(
        &mut self,
        block: usize,
        to_left: bool,
    ) -> Result<(Range<usize>, String), String> {
        let rows = self
            .block_rows(block)
            .ok_or_else(|| String::from("No such difference."))?;
        self.copy_rows_edit(rows, to_left)
    }

    /// The edit that makes aligned rows `rows` of the target side (`to_left`) match the other
    /// side, with the target's line breaks.
    pub fn copy_rows_edit(
        &mut self,
        rows: Range<u32>,
        to_left: bool,
    ) -> Result<(Range<usize>, String), String> {
        self.begin_edit(to_left)?;
        self.copy_source(!to_left)?;
        let source = self.rows.line_range(rows.clone(), !to_left);
        let target = self.rows.line_range(rows, to_left);
        let (source_span, source_open) = self.span(!to_left, &source);
        let (target_span, target_open) = self.span(to_left, &target);
        let buffer = self.buffer(to_left).expect("begun");
        let terminator =
            buffer.terminator((target.start as usize).min(buffer.line_count().saturating_sub(1)));
        let (source_text, _) = self.side_text(!to_left).expect("checked");
        let mut text =
            crate::text::editing::normalize_breaks(&source_text[source_span], terminator);
        let target_text = buffer.text();
        let at_end = target_span.end == target_text.len();
        if target_span.is_empty() {
            // Inserting after an unterminated last line needs a break before the new lines.
            if !text.is_empty()
                && at_end
                && !target_text.is_empty()
                && !target_text.ends_with(['\n', '\r'])
            {
                text.insert_str(0, terminator);
            }
            // Inserting before more lines: the copied last line needs its break.
            if source_open && !at_end {
                text.push_str(terminator);
            }
        } else if source_open && !at_end {
            text.push_str(terminator);
        } else if target_open && !source_open && text.ends_with(terminator) && !text.is_empty() {
            // The target's last line had no final break; the copy brings none.
            text.truncate(text.len() - terminator.len());
        }
        Ok((target_span, text))
    }

    /// The edit that puts one empty line at the gap a placeholder row stands for on `left`'s
    /// side: the byte offset, the text (the side's own line break), and where the new empty
    /// line starts once it is in. The side must be editable already (`begin_edit`).
    pub fn insert_line_edit(&self, row: u32, left: bool) -> Result<(usize, String, usize), String> {
        let buffer = self.buffer(left).ok_or("That side has no edits begun.")?;
        let line = self.rows.line_range(row..row + 1, left).start as usize;
        let at = self.line_offset(left, line);
        let terminator = buffer.terminator(line.min(buffer.line_count().saturating_sub(1)));
        let text = buffer.text();
        // After an unterminated last line the new line needs a break before it as well.
        if at == text.len() && !text.is_empty() && !text.ends_with(['\n', '\r']) {
            return Ok((
                at,
                format!("{terminator}{terminator}"),
                at + terminator.len(),
            ));
        }
        Ok((at, terminator.to_owned(), at))
    }

    /// The byte range that deleting aligned rows `rows` of one side removes: their lines with
    /// terminators. When that runs to the end of a text with no final break, the break before it
    /// goes too, so no empty last line stays behind.
    pub fn delete_rows_edit(
        &mut self,
        rows: Range<u32>,
        left: bool,
    ) -> Result<Range<usize>, String> {
        self.begin_edit(left)?;
        let lines = self.rows.line_range(rows, left);
        let (span, open) = self.span(left, &lines);
        let before = &self.buffer(left).expect("begun").text()[..span.start];
        let break_len = match before {
            _ if !open => 0,
            b if b.ends_with("\r\n") => 2,
            b if b.ends_with(['\n', '\r']) => 1,
            _ => 0,
        };
        Ok(span.start - break_len..span.end)
    }

    /// The text of aligned rows `rows` of one side: their lines joined with the side's line
    /// break, with no final one.
    pub fn rows_text(&self, rows: Range<u32>, left: bool) -> String {
        let terminator = match self.buffer(left) {
            Some(buffer) => buffer.terminator(0),
            None => match self.side(left).eol() {
                EolStyle::CrLf => "\r\n",
                EolStyle::Cr => "\r",
                _ => "\n",
            },
        };
        self.rows
            .line_range(rows, left)
            .map(|line| self.line_text(left, line as usize))
            .collect::<Vec<_>>()
            .join(terminator)
    }

    /// The text a copy takes from one side, or why that side can't be copied from: a file over
    /// 64 MiB has no text, and one that did not decode cleanly would bring U+FFFD along.
    fn copy_source(&self, left: bool) -> Result<&str, String> {
        if self.side(left).is_lossy() {
            return Err("This file did not decode cleanly, so it can't be copied from.".into());
        }
        self.side_text(left)
            .map(|(text, _)| text)
            .ok_or_else(|| "This file is over 64 MiB, so it can't be copied from.".into())
    }

    /// The edit that replaces the whole target side with the other side's text.
    pub fn copy_all_edit(&mut self, to_left: bool) -> Result<(Range<usize>, String), String> {
        self.begin_edit(to_left)?;
        let source = self.copy_source(!to_left)?;
        let buffer = self.buffer(to_left).expect("begun");
        let terminator = buffer.terminator(0);
        Ok((
            0..buffer.len(),
            crate::text::editing::normalize_breaks(source, terminator),
        ))
    }

    pub fn block_kind(&self, block: usize) -> RowKind {
        self.rows.row(self.blocks[block].start).kind
    }

    pub fn visible(&self) -> &Visible {
        &self.visible
    }

    pub fn filter(&self) -> TextFilter {
        self.filter
    }

    pub fn set_filter(&mut self, filter: TextFilter) {
        self.filter = filter;
        let total = self.rows.row_count();
        self.visible = match filter {
            TextFilter::Context(n) => context_rows(&self.blocks, total, n, &self.expanded),
            other => visible_rows(&self.blocks, total, other),
        };
        self.revision = next_revision();
    }

    pub fn row_count(&self) -> usize {
        self.visible.len()
    }

    /// Shared handles for a background search: both documents and the row map.
    /// Edited sides are documents holding the buffer, so lines match the rows.
    pub fn sources(&self) -> (Arc<TextDocument>, Arc<TextDocument>, Arc<RowMap>) {
        let (left, right) = self.documents();
        (left, right, self.rows.clone())
    }

    /// The diff and rows on screen, for a report built from `begin_snapshot`'s documents:
    /// both are O(hunks) to copy, so a report takes them on the UI thread.
    pub fn report_parts(&self) -> (LineDiff, Arc<RowMap>) {
        (self.diff.clone(), self.rows.clone())
    }

    /// Both sides as documents: a side whose document lags its buffer is a writable copy holding
    /// the buffer's text; any other side is its own document, shared.
    fn documents(&self) -> (Arc<TextDocument>, Arc<TextDocument>) {
        let (left, right, _) = self.snapshot();
        (left, right)
    }

    /// The display row of `row`: a fold that hides it is opened, and outside Context the
    /// filter switches to All.
    pub fn reveal_row(&mut self, row: u32) -> Option<usize> {
        if row >= self.rows.row_count() {
            return None;
        }
        if let Some(display) = self.visible.display_of(row) {
            return Some(display);
        }
        match (self.filter, self.visible.fold_of(row)) {
            (TextFilter::Context(_), Some(fold)) => {
                self.expanded.push(fold);
                self.expanded = merge(std::mem::take(&mut self.expanded));
                self.set_filter(self.filter);
            }
            _ => self.set_filter(TextFilter::All),
        }
        self.visible.display_of(row)
    }

    /// The row that shows `line` (0-based) of one side; None past the end.
    pub fn row_of_line(&self, left: bool, line: u32) -> Option<u32> {
        ((line as usize) < self.line_count(left)).then(|| self.rows.row_of(left, line))
    }

    pub fn display_row(&self, display: usize) -> DisplayRow {
        self.visible.entry(display)
    }

    /// The aligned row at `display`; None on a fold row.
    pub fn row(&self, display: usize) -> Option<Row> {
        self.visible.row(display).map(|row| self.rows.row(row))
    }

    /// Shows `rows` more lines of the fold at `display` (from its top), or all of it.
    pub fn expand_fold(&mut self, display: usize, rows: Option<u32>) {
        if let DisplayRow::Fold { start, end } = self.visible.entry(display) {
            let end = rows.map_or(end, |n| start.saturating_add(n).min(end));
            self.expanded.push(start..end);
            self.expanded = merge(std::mem::take(&mut self.expanded));
            self.set_filter(self.filter);
        }
    }

    pub fn expanded(&self) -> &[Range<u32>] {
        &self.expanded
    }

    /// Unfolded rows carried over from an earlier model, clamped to this one.
    pub fn set_expanded(&mut self, ranges: Vec<Range<u32>>) {
        let total = self.rows.row_count();
        self.expanded = merge(
            ranges
                .into_iter()
                .map(|r| r.start.min(total)..r.end.min(total))
                .collect(),
        );
        self.set_filter(self.filter);
    }

    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    pub fn current_block(&self) -> Option<usize> {
        self.current
    }

    /// Makes `block` current without moving anywhere (the view follows the scroll with it).
    pub fn set_current_block(&mut self, block: Option<usize>) {
        self.current = block.filter(|&b| b < self.blocks.len());
    }

    /// R9: the last block whose first row is shown at or above `display`, else the first
    /// shown block; None when no block is shown (the Same filter, or no rows at all: C1).
    pub fn block_at_display(&self, display: usize) -> Option<usize> {
        if self.visible.is_empty() || self.blocks.is_empty() {
            return None;
        }
        let limit = match self.visible.entry(display.min(self.visible.len() - 1)) {
            DisplayRow::Row(row) => row.saturating_add(1),
            DisplayRow::Fold { start, .. } => start,
        };
        let shown = |b: usize| self.visible.display_of(self.blocks[b].start).is_some();
        let above = self.blocks.partition_point(|b| b.start < limit);
        (0..above)
            .rev()
            .find(|&b| shown(b))
            .or_else(|| (above..self.blocks.len()).find(|&b| shown(b)))
    }

    /// The block that holds aligned row `row`.
    pub fn block_containing(&self, row: u32) -> Option<usize> {
        let after = self.blocks.partition_point(|b| b.start <= row);
        after
            .checked_sub(1)
            .filter(|&b| self.blocks[b].contains(&row))
    }

    pub fn current_display(&self) -> Option<usize> {
        self.current
            .and_then(|b| self.visible.display_of(self.blocks[b].start))
    }

    pub fn in_current_block(&self, display: usize) -> bool {
        self.current.is_some_and(|b| {
            self.visible
                .row(display)
                .is_some_and(|row| self.blocks[b].contains(&row))
        })
    }

    /// Makes the first block in `order` whose start is visible current; returns its display row.
    fn go(&mut self, mut order: impl Iterator<Item = usize>) -> Option<usize> {
        let (block, display) = order.find_map(|b| {
            self.visible
                .display_of(self.blocks[b].start)
                .map(|d| (b, d))
        })?;
        self.current = Some(block);
        Some(display)
    }

    pub fn next_change(&mut self) -> Option<usize> {
        let start = self.current.map_or(0, |b| b + 1);
        self.go(start..self.blocks.len())
    }

    pub fn prev_change(&mut self) -> Option<usize> {
        let end = self.current.unwrap_or(0);
        self.go((0..end).rev())
    }

    /// F8: the next block, or the first one again after the last.
    pub fn next_change_wrapping(&mut self) -> Option<Landing> {
        let had_current = self.current.is_some();
        if let Some(display) = self.next_change() {
            return Some(Landing {
                display,
                wrapped: false,
            });
        }
        let display = self.first_change()?;
        Some(Landing {
            display,
            wrapped: had_current,
        })
    }

    /// F7: the previous block, or the last one again before the first.
    pub fn prev_change_wrapping(&mut self) -> Option<Landing> {
        let had_current = self.current.is_some();
        if let Some(display) = self.prev_change() {
            return Some(Landing {
                display,
                wrapped: false,
            });
        }
        let display = self.last_change()?;
        Some(Landing {
            display,
            wrapped: had_current,
        })
    }

    pub fn first_change(&mut self) -> Option<usize> {
        self.go(0..self.blocks.len())
    }

    pub fn last_change(&mut self) -> Option<usize> {
        self.go((0..self.blocks.len()).rev())
    }

    /// The counts of the status line. Similarity is LocalDiff's `2 × (same + soft) / total`
    /// (`de:850`): paired rows outside the hunks, the unimportant pairs included, over both
    /// sides' lines.
    pub fn stats(&self) -> Stats {
        let (mut changed, mut left_only, mut right_only, mut removed) = (0, 0, 0, 0);
        for hunk in &self.diff.hunks {
            let (l, r) = (
                hunk.left.end - hunk.left.start,
                hunk.right.end - hunk.right.start,
            );
            changed += l.min(r);
            left_only += l.saturating_sub(r);
            right_only += r.saturating_sub(l);
            removed += l;
        }
        // A soft run's extra lines are one-sided unimportant rows (R3).
        let (mut soft_left, mut soft_rows) = (0, 0);
        for run in &self.diff.soft {
            let (l, r) = (
                run.left.end - run.left.start,
                run.right.end - run.right.start,
            );
            soft_left += l.saturating_sub(r);
            soft_rows += l.abs_diff(r);
        }
        let total = self.diff.left_lines + self.diff.right_lines;
        let paired = self.diff.left_lines - removed - soft_left;
        Stats {
            hunks: self.diff.hunks.len(),
            changed,
            left_only,
            right_only,
            similarity: if total == 0 {
                1.0
            } else {
                2.0 * paired as f32 / total as f32
            },
            unimportant: soft_rows + self.diff.soft_pairs,
        }
    }

    /// Grapheme-level highlights for a changed row, computed on first use and cached.
    pub fn inline(&mut self, display: usize) -> Option<Arc<InlineDiff>> {
        let row = self.visible.row(display)?;
        self.inline_at(row)
    }

    /// The aligned row with RowMap index `row` (a display row resolved by `display_row`).
    pub fn row_at(&self, row: u32) -> Row {
        self.rows.row(row)
    }

    /// How many aligned rows there are, folded ones included.
    pub fn aligned_count(&self) -> u32 {
        self.rows.row_count()
    }

    /// The aligned row `row`, or None past the end (a menu target can outlive a reload).
    pub fn aligned_row(&self, row: u32) -> Option<Row> {
        (row < self.rows.row_count()).then(|| self.rows.row(row))
    }

    /// The lines of one side that aligned rows `rows` cover; empty at the insertion point when
    /// they hold none.
    pub fn side_lines(&self, rows: Range<u32>, left: bool) -> Range<u32> {
        self.rows.line_range(rows, left)
    }

    /// Whether RowMap row `row` is in the current change block.
    pub fn row_in_current_block(&self, row: u32) -> bool {
        self.current.is_some_and(|b| self.blocks[b].contains(&row))
    }

    /// The other side's line that one side of `row` moved to or from.
    pub fn moved_partner_of(&self, row: Row, left: bool) -> Option<u32> {
        if left {
            self.moves.partner_of_left(row.left?)
        } else {
            self.moves.partner_of_right(row.right?)
        }
    }

    /// Grapheme-level highlights for RowMap row `row` if it is a changed pair.
    pub fn inline_at(&mut self, row: u32) -> Option<Arc<InlineDiff>> {
        let Row {
            kind: RowKind::Changed,
            left: Some(l),
            right: Some(r),
        } = self.rows.row(row)
        else {
            return None;
        };
        Some(self.refined(row, l, r))
    }

    /// What an ignore option or a rule hid on RowMap row `row`, a pair the diff matched: the
    /// grapheme-level differences of its text; None when the two lines are the same.
    pub fn hidden_inline_at(&mut self, row: u32) -> Option<Arc<InlineDiff>> {
        let Row {
            kind: RowKind::Same,
            left: Some(l),
            right: Some(r),
        } = self.rows.row(row)
        else {
            return None;
        };
        let differs = self.inline.contains_key(&row)
            || self.line_text(true, l as usize) != self.line_text(false, r as usize);
        differs.then(|| self.refined(row, l, r))
    }

    /// The highlights of RowMap row `row` (left line `l`, right line `r`), computed on first
    /// use and cached.
    fn refined(&mut self, row: u32, l: u32, r: u32) -> Arc<InlineDiff> {
        if let Some(found) = self.inline.get(&row) {
            return found.clone();
        }
        if self.inline.len() >= INLINE_CACHE {
            self.inline.clear();
        }
        let diff = Arc::new(refine(
            &self.line_text(true, l as usize),
            &self.line_text(false, r as usize),
        ));
        self.inline.insert(row, diff.clone());
        diff
    }
}

/// "UTF-8 · CRLF", plus BOM, lossy-decode and read-only notes.
pub fn doc_info(doc: &TextDocument) -> String {
    let encoding = doc.encoding();
    let mut parts = vec![encoding.encoding.name().to_owned()];
    parts.push(
        match doc.eol() {
            EolStyle::None => "no line breaks",
            EolStyle::Lf => "LF",
            EolStyle::CrLf => "CRLF",
            EolStyle::Cr => "CR",
            EolStyle::Mixed => "mixed line endings",
        }
        .to_owned(),
    );
    if encoding.bom {
        parts.push("BOM".into());
    }
    match doc.read_only_reason() {
        Some(ReadOnlyReason::TooLarge) => parts.push("read-only (over 64 MiB)".into()),
        Some(ReadOnlyReason::LossyDecode) => parts.push("read-only (undecodable bytes)".into()),
        Some(ReadOnlyReason::Formatted) => parts.push("formatted, read-only".into()),
        Some(ReadOnlyReason::Attribute) => parts.push("read-only (Windows attribute)".into()),
        None => {}
    }
    parts.join(" · ")
}

/// The status line as coloured parts: the counts, the filter, moves and notes.
pub fn status_parts(model: &TextModel) -> Vec<StatusPart> {
    let stats = model.stats();
    let muted = |text: String| StatusPart::new(text, Tone::Muted);
    let mut parts = Vec::new();
    let unimportant = stats.unimportant;
    if stats.hunks == 0 {
        if unimportant > 0 {
            parts.push(muted("Same under the rules".into()));
            parts.push(StatusPart::new(
                count_of(unimportant.into(), "unimportant line", "unimportant lines"),
                Tone::Unimportant,
            ));
        } else {
            parts.push(muted("Identical".into()));
        }
        let lines = model.line_count(true) as u64;
        parts.push(muted(count_of(lines, "line", "lines")));
    } else {
        parts.extend([
            muted(count_of(stats.hunks as u64, "difference", "differences")),
            StatusPart::new(format!("{} changed", stats.changed), Tone::Changed),
            StatusPart::new(format!("{} left only", stats.left_only), Tone::LeftOnly),
            StatusPart::new(format!("{} right only", stats.right_only), Tone::RightOnly),
        ]);
        if unimportant > 0 {
            parts.push(StatusPart::new(
                format!("{} unimportant", format_size(unimportant.into())),
                Tone::Unimportant,
            ));
        }
        parts.push(muted(format!("{:.1}% same", stats.similarity * 100.0)));
    }
    parts.push(muted(format!("showing {}", model.filter().label())));
    if stats.hunks > 0 {
        match (model.moves().len(), model.moves().skipped) {
            (_, true) => parts.push(muted("moved lines not checked (too many changes)".into())),
            (0, false) => {}
            (n, false) => parts.push(StatusPart::new(
                count_of(n as u64, "moved line", "moved lines"),
                Tone::Moved,
            )),
        }
    }
    parts.extend(model.notes().iter().map(|note| muted(note.clone())));
    parts
}

pub fn status_text(model: &TextModel) -> String {
    join_parts(&status_parts(model))
}

#[cfg(test)]
mod tests;
