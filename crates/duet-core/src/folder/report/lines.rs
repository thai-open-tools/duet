//! Line details of a folder report (R4, `rep:160-273`, `sheet:65-79`): for each differing pair,
//! the counts of changed lines, the rows the report shows around them and the rows of the
//! Changed lines sheet. Reads run on the caller's thread, one pair at a time, and stop at the
//! cancel token.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::cancel::CancelToken;
use crate::folder::actions::ensure_no_links_below;
use crate::folder::classify::Warning;
use crate::folder::entry::EntryKind;
use crate::folder::report::mask::mask_lines;
use crate::folder::report::{ReportItem, SideFacts};
use crate::folder::run::FolderTextRules;
use crate::i18n::{tr, tr_key, trf};
use crate::text::diff::{DiffError, DiffInput, DiffOptions};
use crate::text::encoding::{decode, is_binary};
use crate::text::inline;
use crate::text::lines::LineIndex;
use crate::text::rows::{RowKind, RowMap};
use crate::text::rules::{TextRules, diff_with_rules};

#[cfg(test)]
mod tests;

/// Files over this size a side are not read for lines (`RPT_FILE_MAX`).
pub const FILE_MAX: u64 = 8 * 1024 * 1024;
/// Rows a report shows for one file (`RPT_ROW_CAP`).
pub const FILE_ROWS: usize = 300;
/// Rows a report shows in all (`RPT_TOTAL_ROWS`).
pub const TOTAL_ROWS: usize = 40_000;
/// Rows of the Changed lines sheet in all (`RPT_SHEET_ROWS`).
pub const SHEET_ROWS: usize = 100_000;

/// LocalDiff's row types (`LD_TYPE_NAMES`), used as CSS classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowClass {
    Same,
    Soft,
    Mod,
    Del,
    Ins,
    UDel,
    UIns,
}

impl RowClass {
    /// A real difference: changed, only on the left or only on the right.
    fn is_diff(self) -> bool {
        matches!(self, RowClass::Mod | RowClass::Del | RowClass::Ins)
    }

    fn is_soft(self) -> bool {
        matches!(self, RowClass::Soft | RowClass::UDel | RowClass::UIns)
    }
}

/// Byte ranges of the changed characters on the left and on the right.
pub type Marks = (Vec<Range<usize>>, Vec<Range<usize>>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailRow {
    /// Line numbers are 1-based; `marks` are byte ranges of the changed characters on each side.
    Line {
        class: RowClass,
        left: Option<(u32, String)>,
        right: Option<(u32, String)>,
        marks: Option<Marks>,
    },
    /// "⋯ N same lines ⋯".
    Skip(usize),
    /// "… N more rows".
    More(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetKind {
    Changed,
    LeftOnly,
    RightOnly,
    Soft,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetRow {
    pub kind: SheetKind,
    /// The change block the line belongs to, 1-based (for soft lines, the run of them).
    pub place: u32,
    pub left: Option<(u32, String)>,
    pub right: Option<(u32, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineInfo {
    /// The pair was read and compared, so the counts mean something.
    pub counted: bool,
    pub changed: u32,
    pub left_only: u32,
    pub right_only: u32,
    pub soft: u32,
    pub places: u32,
    pub rows: Vec<DetailRow>,
    pub sheet: Vec<SheetRow>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LineDetails {
    /// By the item's `rel`.
    pub files: HashMap<String, LineInfo>,
    /// Lines the secret masking changed, both sides.
    pub masked_lines: usize,
    /// Pairs not read because the job was cancelled.
    pub cancelled_files: usize,
    /// Files whose Changed lines sheet rows did not fit the workbook's cap.
    pub sheet_cut_files: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineOptions {
    /// Build the rows a report shows (counts are always read).
    pub rows: bool,
    pub context: u32,
    pub mask: bool,
    /// Build the Changed lines sheet rows.
    pub sheet: bool,
    pub file_rows: usize,
    pub total_rows: usize,
    pub sheet_rows: usize,
}

impl Default for LineOptions {
    fn default() -> Self {
        Self {
            rows: true,
            context: 2,
            mask: true,
            sheet: false,
            file_rows: FILE_ROWS,
            total_rows: TOTAL_ROWS,
            sheet_rows: SHEET_ROWS,
        }
    }
}

const NOTE_BIG: &str =
    tr_key("The file is larger than 8 MiB: no lines are shown (open it in duet to see them).");
const NOTE_BINARY: &str = tr_key("Binary file: compare it in duet's hex view.");
const NOTE_UNREADABLE: &str = tr_key("Could not read the file.");
const NOTE_NOT_PLAIN: &str = tr_key("Only plain files are read: no lines are shown.");
const NOTE_NO_LINES: &str = tr_key(
    "No differing lines under the current text compare rules (only parts set to be ignored differ).",
);
const NOTE_LIMIT: &str =
    tr_key("The report reached its size limit: no lines are shown for this file.");
const NOTE_COMPARE: &str = tr_key("Could not compare the file.");

enum ReadFail {
    NotPlain,
    Big,
    Binary,
    Unreadable,
}

impl ReadFail {
    fn note(&self) -> &'static str {
        match self {
            ReadFail::NotPlain => tr(NOTE_NOT_PLAIN),
            ReadFail::Big => tr(NOTE_BIG),
            ReadFail::Binary => tr(NOTE_BINARY),
            ReadFail::Unreadable => tr(NOTE_UNREADABLE),
        }
    }
}

/// One side's bytes: a plain file only (never through a link, PF3), at most `FILE_MAX` of them.
fn read_side(side: &SideFacts) -> Result<Vec<u8>, ReadFail> {
    if side.kind != EntryKind::File {
        return Err(ReadFail::NotPlain);
    }
    // A folder on the way that became a link or junction after the compare would lead the
    // read outside the compared tree (M-2). The root itself may be a link: the user chose it.
    let Some(root) = side.path.ancestors().nth(side.rel.split('/').count()) else {
        return Err(ReadFail::NotPlain);
    };
    if ensure_no_links_below(root, &side.path).is_err() {
        return Err(ReadFail::NotPlain);
    }
    read_plain(&side.path)
}

fn read_plain(path: &Path) -> Result<Vec<u8>, ReadFail> {
    let meta = fs::symlink_metadata(path).map_err(|_| ReadFail::Unreadable)?;
    if !meta.file_type().is_file() {
        return Err(if meta.file_type().is_dir() {
            ReadFail::Unreadable
        } else {
            ReadFail::NotPlain
        });
    }
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(FILE_MAX + 1).read_to_end(&mut bytes))
        .map_err(|_| ReadFail::Unreadable)?;
    if bytes.len() as u64 > FILE_MAX {
        return Err(ReadFail::Big);
    }
    if is_binary(&bytes) {
        return Err(ReadFail::Binary);
    }
    Ok(bytes)
}

/// The decoded lines of a file, masked first when asked (`masked` counts the lines changed).
/// This is the only place file text enters a report, so every output is masked from here.
fn split_lines(bytes: &[u8], mask: bool, masked: &mut usize) -> Vec<String> {
    let decoded = decode(bytes);
    let text = decoded.text.as_bytes();
    let index = LineIndex::new(text);
    let mut lines: Vec<String> = (0..index.len())
        .map(|n| String::from_utf8_lossy(index.line_bytes(text, n)).into_owned())
        .collect();
    if mask {
        *masked += mask_lines(&mut lines);
    }
    lines
}

/// Every line ends with "\n", so a trailing blank line survives (PF15).
fn join_lines(lines: &[String]) -> Vec<u8> {
    let mut out = Vec::with_capacity(lines.iter().map(|l| l.len() + 1).sum());
    for line in lines {
        out.extend_from_slice(line.as_bytes());
        out.push(b'\n');
    }
    out
}

enum Outcome {
    Info(LineInfo),
    Cancelled,
}

/// Reads and compares every item that needs lines. `progress` counts the items done.
pub fn line_details(
    items: &[ReportItem],
    rules: &FolderTextRules,
    opts: &LineOptions,
    progress: &AtomicUsize,
    cancel: &CancelToken,
) -> LineDetails {
    let list: Vec<&ReportItem> = items.iter().filter(|item| item.needs_lines()).collect();
    let mut out = LineDetails::default();
    let (mut total_left, mut sheet_left) = (opts.total_rows, opts.sheet_rows);
    for (at, item) in list.iter().enumerate() {
        if cancel.is_cancelled() {
            out.cancelled_files = list.len() - at;
            break;
        }
        let outcome = one_file(
            item,
            rules,
            opts,
            &mut total_left,
            &mut sheet_left,
            &mut out,
            cancel,
        );
        match outcome {
            Outcome::Info(info) => {
                out.files.insert(item.rel.clone(), info);
            }
            Outcome::Cancelled => {
                out.cancelled_files = list.len() - at;
                break;
            }
        }
        progress.fetch_add(1, Ordering::Relaxed);
    }
    out
}

fn note_only(note: &str) -> Outcome {
    Outcome::Info(LineInfo {
        note: Some(note.to_owned()),
        ..LineInfo::default()
    })
}

fn one_file(
    item: &ReportItem,
    rules: &FolderTextRules,
    opts: &LineOptions,
    total_left: &mut usize,
    sheet_left: &mut usize,
    out: &mut LineDetails,
    cancel: &CancelToken,
) -> Outcome {
    let (Some(left), Some(right)) = (&item.left, &item.right) else {
        return note_only(tr(NOTE_UNREADABLE));
    };
    let bytes = match (read_side(left), read_side(right)) {
        (Ok(l), Ok(r)) => [l, r],
        (Err(fail), _) | (_, Err(fail)) => return note_only(fail.note()),
    };
    let mut masked = 0usize;
    let lines = [
        split_lines(&bytes[0], opts.mask, &mut masked),
        split_lines(&bytes[1], opts.mask, &mut masked),
    ];
    out.masked_lines += masked;
    let joined = [join_lines(&lines[0]), join_lines(&lines[1])];
    let indexes = [LineIndex::new(&joined[0]), LineIndex::new(&joined[1])];

    // An OddSpace pair is compared plainly, so its look-alike-space lines show.
    let odd_space = item
        .note
        .as_ref()
        .is_some_and(|note| note.warning == Some(Warning::OddSpace));
    let (diff_options, text_rules) = if odd_space {
        (DiffOptions::default(), TextRules::default())
    } else {
        rules.for_file(item.rel.rsplit('/').next().unwrap_or(&item.rel))
    };
    let diff = match diff_with_rules(
        DiffInput {
            bytes: &joined[0],
            lines: &indexes[0],
        },
        DiffInput {
            bytes: &joined[1],
            lines: &indexes[1],
        },
        diff_options,
        &text_rules,
        cancel,
    ) {
        Ok((diff, _)) => diff,
        Err(DiffError::Cancelled(_)) => return Outcome::Cancelled,
        Err(DiffError::TooManyLines(_)) => return note_only(tr(NOTE_COMPARE)),
    };
    let map = RowMap::new(&diff);
    let classes = row_classes(&map, &lines);

    let mut info = LineInfo {
        counted: true,
        places: map.change_blocks().len() as u32,
        ..LineInfo::default()
    };
    for class in &classes {
        match class {
            RowClass::Mod => info.changed += 1,
            RowClass::Del => info.left_only += 1,
            RowClass::Ins => info.right_only += 1,
            RowClass::Soft | RowClass::UDel | RowClass::UIns => info.soft += 1,
            RowClass::Same => {}
        }
    }
    if opts.rows {
        if *total_left == 0 {
            info.note = Some(tr(NOTE_LIMIT).to_owned());
        } else {
            let cap = opts.file_rows.min(*total_left);
            let (rows, shown) = detail_rows(&map, &classes, &lines, opts.context, cap);
            *total_left -= shown;
            if shown == 0 {
                info.note = Some(tr(NOTE_NO_LINES).to_owned());
            } else {
                info.rows = rows;
            }
        }
    }
    if opts.sheet {
        if *sheet_left == 0 {
            out.sheet_cut_files += 1;
        } else {
            info.sheet = sheet_rows(&map, &classes, &lines, *sheet_left);
            *sheet_left -= info.sheet.len();
        }
    }
    Outcome::Info(info)
}

/// One class per row. A `Same` row whose two texts differ is soft: the compare rules hid it.
fn row_classes(map: &RowMap, lines: &[Vec<String>; 2]) -> Vec<RowClass> {
    (0..map.row_count())
        .map(|n| {
            let row = map.row(n);
            match row.kind {
                RowKind::Same => match (row.left, row.right) {
                    (Some(l), Some(r)) if lines[0][l as usize] != lines[1][r as usize] => {
                        RowClass::Soft
                    }
                    _ => RowClass::Same,
                },
                RowKind::Changed => RowClass::Mod,
                RowKind::LeftOnly => RowClass::Del,
                RowKind::RightOnly => RowClass::Ins,
                RowKind::SoftLeft => RowClass::UDel,
                RowKind::SoftRight => RowClass::UIns,
            }
        })
        .collect()
}

fn text_of(lines: &[Vec<String>; 2], side: usize, line: Option<u32>) -> Option<(u32, String)> {
    line.map(|n| (n + 1, lines[side][n as usize].clone()))
}

/// `rptDiffRows`: the differing rows (every non-same row when there are none), `context` rows
/// around each, a skip row for each gap after the first shown row, at most `cap` rows and then
/// one "more" row. Returns the rows and how many lines they show.
fn detail_rows(
    map: &RowMap,
    classes: &[RowClass],
    lines: &[Vec<String>; 2],
    context: u32,
    cap: usize,
) -> (Vec<DetailRow>, usize) {
    let any = classes.iter().any(|class| class.is_diff());
    let keep: Vec<bool> = classes
        .iter()
        .map(|&class| {
            if any {
                class.is_diff()
            } else {
                class != RowClass::Same
            }
        })
        .collect();
    let mut shown_rows = vec![false; keep.len()];
    let mut distance = u64::MAX / 2;
    for (i, &kept) in keep.iter().enumerate() {
        distance = if kept { 0 } else { distance + 1 };
        if distance <= u64::from(context) {
            shown_rows[i] = true;
        }
    }
    distance = u64::MAX / 2;
    for (i, &kept) in keep.iter().enumerate().rev() {
        distance = if kept { 0 } else { distance + 1 };
        if distance <= u64::from(context) {
            shown_rows[i] = true;
        }
    }
    let (mut rows, mut shown, mut skipped, mut more) = (Vec::new(), 0usize, 0usize, 0usize);
    for (i, &class) in classes.iter().enumerate() {
        if !shown_rows[i] {
            if shown > 0 {
                skipped += 1;
            }
            continue;
        }
        if shown >= cap {
            more += 1;
            continue;
        }
        if skipped > 0 {
            rows.push(DetailRow::Skip(skipped));
            skipped = 0;
        }
        let row = map.row(i as u32);
        let marks = match (class, row.left, row.right) {
            (RowClass::Mod | RowClass::Soft, Some(l), Some(r)) => {
                let refined = inline::refine(&lines[0][l as usize], &lines[1][r as usize]);
                Some((refined.left, refined.right))
            }
            _ => None,
        };
        rows.push(DetailRow::Line {
            class,
            left: text_of(lines, 0, row.left),
            right: text_of(lines, 1, row.right),
            marks,
        });
        shown += 1;
    }
    if more > 0 {
        rows.push(DetailRow::More(more));
    } else if skipped > 0 {
        rows.push(DetailRow::Skip(skipped));
    }
    (rows, shown)
}

/// `rptLineRows`: one row per differing line, without context; the soft lines when nothing
/// else differs.
fn sheet_rows(
    map: &RowMap,
    classes: &[RowClass],
    lines: &[Vec<String>; 2],
    cap: usize,
) -> Vec<SheetRow> {
    let any = classes.iter().any(|class| class.is_diff());
    let blocks = map.change_blocks();
    let (mut block, mut soft_runs, mut in_soft) = (0usize, 0u32, false);
    let mut out = Vec::new();
    for (i, &class) in classes.iter().enumerate() {
        if out.len() >= cap {
            break;
        }
        let i = i as u32;
        while block < blocks.len() && blocks[block].end <= i {
            block += 1;
        }
        let soft = !any && class.is_soft();
        if soft && !in_soft {
            soft_runs += 1;
        }
        in_soft = soft;
        let (kind, place) = match class {
            RowClass::Mod => (SheetKind::Changed, block as u32 + 1),
            RowClass::Del => (SheetKind::LeftOnly, block as u32 + 1),
            RowClass::Ins => (SheetKind::RightOnly, block as u32 + 1),
            _ if soft => (SheetKind::Soft, soft_runs),
            _ => continue,
        };
        let row = map.row(i);
        out.push(SheetRow {
            kind,
            place,
            left: text_of(lines, 0, row.left),
            right: text_of(lines, 1, row.right),
        });
    }
    out
}

fn number(n: u32) -> String {
    crate::folder::report::count_text(u64::from(n), "", "")
        .trim_end()
        .to_owned()
}

/// `rptLineSummary` (`rep:263-271`): "Lines: 2 changed · 1 only in IWT · 3 only in PRD (2
/// places)", "Differs only in unimportant parts", or "" when nothing was counted.
pub fn line_summary(info: &LineInfo, names: &[String; 2]) -> String {
    if !info.counted {
        return String::new();
    }
    let mut parts = Vec::new();
    if info.changed > 0 {
        parts.push(trf("{} changed", &[&number(info.changed)]));
    }
    if info.left_only > 0 {
        parts.push(trf("{} only in {}", &[&number(info.left_only), &names[0]]));
    }
    if info.right_only > 0 {
        parts.push(trf("{} only in {}", &[&number(info.right_only), &names[1]]));
    }
    if parts.is_empty() {
        return if info.soft > 0 {
            tr("Differs only in unimportant parts").to_owned()
        } else {
            String::new()
        };
    }
    let mut text = trf("Lines: {}", &[&parts.join(" · ")]);
    if info.places > 1 {
        text.push_str(&format!(
            " ({})",
            crate::folder::report::count_text(u64::from(info.places), "place", "places")
        ));
    }
    text
}
