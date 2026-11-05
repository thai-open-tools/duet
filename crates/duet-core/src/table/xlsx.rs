//! Excel workbooks (`.xlsx`, `.xlsm`) as tables, read-only, through `calamine`.
//!
//! A workbook is opened once (sheet names cost the open: calamine reads the shared strings
//! and styles then), and one sheet at a time is streamed cell by cell into a [`Table`]. The
//! sheet is never built as a dense range: a stray cell at `XFD1048576` would allocate
//! gigabytes. Macros in an `.xlsm` are never run (calamine does not run VBA) and nothing
//! else of the package is read. Before calamine sees a file, `guard` holds its package to the
//! limits a read can survive.
//!
//! # What a cell becomes
//!
//! The compare must neither hide a real difference nor invent one, so a cell is the text of
//! its **stored** value, never of what Excel's number format shows:
//!
//! * A number is its stored value, whatever its format: `0.25` stays `0.25` whether the cell
//!   shows `25%` or `0.25`; `1234.5` stays so under `#,##0.00`. `1` and `1.0` are one value
//!   (Excel's own: the file holds a float, not the digits typed) and read the same. It reads at
//!   15 significant digits, Excel's own precision: `=0.1+0.2` is `0.30000000000000004` in the
//!   file, Excel shows `0.3` and calls it equal to a typed `0.3`, and so does duet. That hides
//!   only what Excel cannot show; a difference within 15 digits stays one.
//! * A date or time (a number whose format is a date format) is ISO text, through the
//!   workbook's 1900 or 1904 epoch: `2024-03-05`, `2024-03-05 13:30:00`, time only
//!   `13:30:00`. Precision is the millisecond, as Excel's. A value outside Excel's date
//!   range stays the stored number (a clamped date would hide it). A date serial in a
//!   General cell stays the number. A `[h]:mm:ss` duration is `26:30:00`. Calamine does not know
//!   Excel's Thai built-in date formats (ids 71-81), so such a cell reads as its number.
//! * A boolean is `TRUE` / `FALSE`; an error is its text (`#DIV/0!`).
//! * A formula is its last saved result; formula text is not compared. A formula with no
//!   saved result (a file written by a library) is empty and counted in
//!   [`SheetRead::formulas_without_result`].
//! * **Text cells that could be mistaken for another type get a leading `'`**, Excel's own
//!   notation for text: text `1` is `'1`, text `TRUE` is `'TRUE`, text `#N/A` is `'#N/A`,
//!   text `2024-03-05` is `'2024-03-05`. Only a text that equals the rendering of a number,
//!   boolean, error, date or time gets it (text `007` or `abc` is shown as it is: no other
//!   cell renders that), and text that starts with `'` and would then be ambiguous gets a
//!   second one, so the mapping from cells to text never merges two different cells. The
//!   marker is for a workbook against a workbook. Beside a CSV file the app reads the workbook
//!   with it off ([`Workbook::read_sheet_with`]): a CSV has no types, and `'1` against `1`
//!   would be a difference nobody wrote. "Compare numbers by value" sees through the marker on a
//!   number, in the cells and in the key columns: `'1` then equals `1`. Marked `TRUE`, errors and
//!   dates stay text, since that option is about numbers.
//! * An empty cell and a text cell holding the empty string both read `""`: Excel shows
//!   them alike and formulas returning `""` are the common source. Leading and inner empty
//!   cells keep their column; trailing ones are not stored; a row with no value is skipped,
//!   as a blank line is in CSV, and [`Table::line`] keeps the sheet's row number.
//! * Merged cells: the value sits in the top-left cell only, as Excel stores it.
//! * Hidden rows and columns are read. Hidden sheets are listed.

use std::error::Error;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use calamine::{
    DataRef, ExcelDateTime, ExcelDateTimeType, Reader, SheetType, SheetVisible, Xlsx, XlsxError,
};

use super::{Delimiter, Table, TableBuilder};
use crate::cancel::CancelToken;
use crate::i18n::tr_key;
pub use guard::TooLarge;

#[doc(hidden)]
pub mod fixture;
mod guard;
#[cfg(test)]
mod tests;

/// Cells (padding included) one sheet may hold before the read stops at that row.
pub const MAX_CELLS: usize = 8_000_000;
/// Bytes of cell text one sheet may hold.
pub const MAX_TEXT_BYTES: usize = 512 * 1024 * 1024;
/// Rows times the widest row one sheet may hold. Cells past a row's end cost nothing to store, but
/// the compare walks every row across the full width and joins its key with a separator per column,
/// so a stray value at `XFD1` over 500,000 rows would take 8 GB per side. This allows 64 MB of them.
pub const MAX_GRID_CELLS: usize = 64_000_000;
/// Cells between cancel checks.
const CHECK_EVERY: u64 = 4096;

const OLE_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// `.xlsx` or `.xlsm`, by extension, case-insensitive.
pub fn is_workbook_name(path: &Path) -> bool {
    has_extension(path, &["xlsx", "xlsm"])
}

/// Whether the file starts as a workbook does: a zip, or an OLE file (an old `.xls`, or a password
/// protected workbook).
pub fn has_workbook_magic(path: &Path) -> io::Result<bool> {
    let head = FileHead::read(&mut File::open(path)?)?;
    Ok(head.ole || head.zip)
}

/// What a file's first bytes say it is.
struct FileHead {
    ole: bool,
    zip: bool,
}

impl FileHead {
    fn read(file: &mut File) -> io::Result<FileHead> {
        let mut head = [0u8; 8];
        let mut got = 0;
        while got < head.len() {
            match file.read(&mut head[got..])? {
                0 => break,
                n => got += n,
            }
        }
        Ok(FileHead {
            ole: got == 8 && head == OLE_SIGNATURE,
            zip: got >= 2 && &head[..2] == b"PK",
        })
    }
}

/// `.csv`, `.tsv` or `.tab`, by extension, case-insensitive.
pub fn is_csv_name(path: &Path) -> bool {
    has_extension(path, &["csv", "tsv", "tab"])
}

fn has_extension(path: &Path, wanted: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| wanted.iter().any(|w| e.eq_ignore_ascii_case(w)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetEntry {
    pub name: String,
    /// Hidden or very hidden.
    pub hidden: bool,
}

/// Why a file is not opened as a workbook. The wording is the app's.
#[derive(Debug)]
pub enum OpenError {
    /// Opening or reading the file failed (a sharing violation arrives here).
    Io(std::io::Error),
    /// An OLE file holding an encrypted package: a password-protected workbook.
    Password,
    /// An OLE file that is not encrypted: an old `.xls`.
    OldXls,
    /// Not a zip file.
    NotAZip,
    /// A zip file without a workbook in it.
    NotAWorkbook,
    /// Looks like a workbook but cannot be read (or the reader failed unexpectedly).
    Damaged(String),
    /// A workbook with no worksheet.
    NoSheets,
    /// An Excel 4 macro sheet: the reader refuses the whole workbook for it.
    MacroSheet,
    /// The package is larger, unpacked, than duet reads ([`guard`]).
    TooLarge(TooLarge),
    /// The cancel token was set between two steps of the open.
    Cancelled,
}

/// Why a sheet could not be read.
#[derive(Debug)]
pub enum ReadFail {
    Cancelled,
    Damaged(String),
    /// An error value this reader does not know (`#SPILL!`, `#CALC!`): the whole sheet stops at it.
    CellError(String),
    /// Reading the file failed.
    Io(io::Error),
}

#[derive(Debug)]
pub struct SheetRead {
    pub table: Table,
    /// The 1-based sheet row where the read stopped at the cell or text budget; that row and
    /// the ones after it are not in the table.
    pub cut_at_row: Option<u32>,
    /// Formula cells that have no saved result and read as empty.
    pub formulas_without_result: u32,
    /// Text cells that carry the leading `'`.
    pub marked_cells: u32,
}

/// An open workbook: its worksheets and the reader to stream one of them.
pub struct Workbook {
    xlsx: Xlsx<BufReader<File>>,
    sheets: Vec<SheetEntry>,
}

/// A reader that stopped for a reason of its own: the panic's text follows in brackets. The app
/// finds this sentence and says it in the language in force.
pub const UNREADABLE: &str = tr_key("the file's content could not be read");

/// Runs `f`, turning a panic inside the reader (a malformed file) into a message. The project
/// does not build with `panic = "abort"`, so unwinding works.
fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        let what = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        if what.is_empty() {
            UNREADABLE.to_string()
        } else {
            format!("{UNREADABLE} ({what})")
        }
    })
}

/// The system's error inside a reader's error, when that is what stopped the reader. Inflate also
/// reports corrupt data through `io::Error`, with no code from the system: that stays the file's.
fn system_error(error: &XlsxError) -> Option<io::Error> {
    let mut at: Option<&(dyn Error + 'static)> = Some(error);
    while let Some(cause) = at {
        let io = cause
            .downcast_ref::<io::Error>()
            .or_else(|| cause.downcast_ref::<Arc<io::Error>>().map(|io| &**io));
        if let Some(code) = io.and_then(io::Error::raw_os_error) {
            return Some(io::Error::from_raw_os_error(code));
        }
        at = cause.source();
    }
    None
}

fn open_error(error: XlsxError) -> OpenError {
    if let Some(io) = system_error(&error) {
        return OpenError::Io(io);
    }
    match error {
        XlsxError::Io(e) => OpenError::Io(e),
        XlsxError::Password => OpenError::Password,
        XlsxError::FileNotFound(_)
        | XlsxError::RelationshipNotFound
        | XlsxError::NotAWorksheet(_) => OpenError::NotAWorkbook,
        XlsxError::Unrecognized {
            typ: "sheet:type", ..
        } => OpenError::MacroSheet,
        other => OpenError::Damaged(other.to_string()),
    }
}

impl Workbook {
    /// Sorts the file by its first bytes (zip, OLE, other) before calamine sees it, then opens
    /// it. Nothing is written.
    pub fn open(path: &Path) -> Result<Workbook, OpenError> {
        Self::open_with(path, &CancelToken::new())
    }

    /// [`Workbook::open`], stopping between its steps once `cancel` is set. Calamine's own open
    /// cannot be stopped; the check before it can.
    pub fn open_with(path: &Path, cancel: &CancelToken) -> Result<Workbook, OpenError> {
        let stop = || {
            if cancel.is_cancelled() {
                Err(OpenError::Cancelled)
            } else {
                Ok(())
            }
        };
        stop()?;
        let mut file = File::open(path).map_err(OpenError::Io)?;
        let FileHead { ole, zip } = FileHead::read(&mut file).map_err(OpenError::Io)?;
        if !ole && !zip {
            return Err(OpenError::NotAZip);
        }
        if zip {
            guarded(|| guard::check(BufReader::new(&file), cancel))
                .map_err(OpenError::Damaged)?
                .map_err(|refused| match refused {
                    guard::Refused::TooLarge(limit) => OpenError::TooLarge(limit),
                    guard::Refused::Cancelled => OpenError::Cancelled,
                })?;
        }
        stop()?;
        file.seek(SeekFrom::Start(0)).map_err(OpenError::Io)?;
        let opened = guarded(|| Xlsx::new(BufReader::new(file))).map_err(OpenError::Damaged)?;
        stop()?;
        let xlsx = match opened {
            Ok(xlsx) => xlsx,
            Err(XlsxError::Password) => return Err(OpenError::Password),
            Err(_) if ole => return Err(OpenError::OldXls),
            Err(error) => return Err(open_error(error)),
        };
        let sheets: Vec<SheetEntry> = xlsx
            .sheets_metadata()
            .iter()
            .filter(|s| s.typ == SheetType::WorkSheet)
            .map(|s| SheetEntry {
                name: s.name.clone(),
                hidden: s.visible != SheetVisible::Visible,
            })
            .collect();
        if sheets.is_empty() {
            return Err(OpenError::NoSheets);
        }
        Ok(Workbook { xlsx, sheets })
    }

    /// The worksheets in workbook order (chart sheets are not listed).
    pub fn sheets(&self) -> &[SheetEntry] {
        &self.sheets
    }

    /// Streams sheet `index` into a table. Checks `cancel` every 4,096 cells and keeps
    /// `rows_done` at the rows read so far. Stops at [`MAX_CELLS`] or [`MAX_TEXT_BYTES`] with
    /// `cut_at_row` set. A panic inside calamine is an error, not a crash.
    pub fn read_sheet(
        &mut self,
        index: usize,
        cancel: &CancelToken,
        rows_done: &AtomicU64,
    ) -> Result<SheetRead, ReadFail> {
        self.read_sheet_with(index, cancel, rows_done, true)
    }

    /// [`Workbook::read_sheet`] with the text marker chosen. `mark_text` is on for a workbook
    /// compared with a workbook; off beside a CSV file, which has no cell types, so that text
    /// `1` reads `1` and does not invent a difference from the CSV's `1`.
    pub fn read_sheet_with(
        &mut self,
        index: usize,
        cancel: &CancelToken,
        rows_done: &AtomicU64,
        mark_text: bool,
    ) -> Result<SheetRead, ReadFail> {
        let Some(entry) = self.sheets.get(index) else {
            return Err(ReadFail::Damaged(
                tr_key("there is no such sheet").to_string(),
            ));
        };
        let name = entry.name.clone();
        let is_1904 = self.xlsx.has_1904_epoch();
        guarded(|| read_cells(&mut self.xlsx, &name, is_1904, mark_text, cancel, rows_done))
            .map_err(ReadFail::Damaged)?
    }
}

fn read_fail(error: XlsxError) -> ReadFail {
    if let Some(io) = system_error(&error) {
        return ReadFail::Io(io);
    }
    match error {
        XlsxError::CellError(value) => ReadFail::CellError(value),
        other => ReadFail::Damaged(other.to_string()),
    }
}

fn read_cells(
    xlsx: &mut Xlsx<BufReader<File>>,
    name: &str,
    is_1904: bool,
    mark_text: bool,
    cancel: &CancelToken,
    rows_done: &AtomicU64,
) -> Result<SheetRead, ReadFail> {
    if cancel.is_cancelled() {
        return Err(ReadFail::Cancelled);
    }
    let mut reader = xlsx.worksheet_cells_reader(name).map_err(read_fail)?;
    let mut builder = TableBuilder::new(Delimiter::Comma, true);
    builder.mark_text(mark_text);
    let mut open_row: Option<u32> = None;
    let mut next_col = 0u32;
    let mut cut_at_row = None;
    let mut formulas_without_result = 0u32;
    let mut marked_cells = 0u32;
    let mut widest = 0usize;
    let mut records = 0u64;
    let mut seen = 0u64;
    while let Some(cell) = reader
        .next_cell_with_formula_metadata()
        .map_err(read_fail)?
    {
        seen += 1;
        if seen.is_multiple_of(CHECK_EVERY) {
            if cancel.is_cancelled() {
                return Err(ReadFail::Cancelled);
            }
            rows_done.store(records, Ordering::Relaxed);
        }
        let Some(text) = cell_text(&cell.value, is_1904, mark_text) else {
            if cell.formula.is_some() && cell.value == DataRef::Empty {
                formulas_without_result += 1;
            }
            continue;
        };
        let (row, col) = cell.pos;
        if open_row != Some(row) {
            if open_row.is_some_and(|open| row < open) {
                return Err(ReadFail::Damaged(
                    tr_key("the rows are out of order").to_string(),
                ));
            }
            if open_row.is_some() {
                builder.end_record(open_row.unwrap_or(0) + 1);
                records += 1;
            }
            open_row = Some(row);
            next_col = 0;
        }
        if col < next_col {
            return Err(ReadFail::Damaged(
                tr_key("the cells of a row are out of order").to_string(),
            ));
        }
        let pad = (col - next_col) as usize;
        widest = widest.max(col as usize + 1);
        if builder.cells() + pad + 1 > MAX_CELLS
            || builder.text_len() + text.len() > MAX_TEXT_BYTES
            || (records as usize + 1).saturating_mul(widest) > MAX_GRID_CELLS
        {
            cut_at_row = Some(row + 1);
            break;
        }
        if mark_text && unmarked(&text).len() != text.len() {
            marked_cells += 1;
        }
        for _ in 0..pad {
            builder.push_cell("");
        }
        builder.push_cell(&text);
        next_col = col + 1;
    }
    if cut_at_row.is_none()
        && let Some(open) = open_row
    {
        builder.end_record(open + 1);
        records += 1;
    }
    rows_done.store(records, Ordering::Relaxed);
    Ok(SheetRead {
        table: builder.finish(),
        cut_at_row,
        formulas_without_result,
        marked_cells,
    })
}

/// The first visible sheet; the first sheet when all are hidden.
pub fn default_sheet(sheets: &[SheetEntry]) -> usize {
    sheets.iter().position(|s| !s.hidden).unwrap_or(0)
}

/// The sheets two workbooks open on: the first visible sheet of the left whose name (case
/// does not matter, as in Excel) is also a visible sheet of the right, for both; otherwise the
/// first visible sheet of each.
pub fn default_sheets(left: &[SheetEntry], right: &[SheetEntry]) -> (usize, usize) {
    for (li, l) in left.iter().enumerate().filter(|(_, s)| !s.hidden) {
        let wanted = l.name.to_lowercase();
        if let Some(ri) = right
            .iter()
            .position(|r| !r.hidden && r.name.to_lowercase() == wanted)
        {
            return (li, ri);
        }
    }
    (default_sheet(left), default_sheet(right))
}

// ---- Cell text ---------------------------------------------------------------------------

const ERROR_TEXTS: [&str; 8] = [
    "#DIV/0!", "#N/A", "#NAME?", "#NULL!", "#NUM!", "#REF!", "#VALUE!", "#DATA!",
];

/// How many digits a number's text has from its first to its last that is not a zero.
fn significant_digits(text: &str) -> usize {
    let (mut count, mut zeros) = (0, 0);
    for digit in text.bytes().filter(u8::is_ascii_digit) {
        if digit != b'0' {
            count += zeros + 1;
            zeros = 0;
        } else if count > 0 {
            zeros += 1;
        }
    }
    count
}

/// The text of a stored number at 15 significant digits, Excel's own precision: the shortest text
/// that reads back, integral values without a point, no exponent. Negative zero is `0`.
fn number_text(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let shortest = v.to_string();
    if !v.is_finite() || significant_digits(&shortest) <= 15 {
        return shortest;
    }
    format!("{v:.14e}")
        .parse::<f64>()
        .map_or(shortest, |rounded| rounded.to_string())
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `h+:mm:ss` with an optional `.mmm`, what a time or a duration reads as.
fn is_clock(s: &str) -> bool {
    let main = match s.split_once('.') {
        Some((main, frac)) if frac.len() == 3 && all_digits(frac) => main,
        Some(_) => return false,
        None => s,
    };
    let mut parts = main.split(':');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(h), Some(m), Some(sec), None) => {
            all_digits(h) && m.len() == 2 && all_digits(m) && sec.len() == 2 && all_digits(sec)
        }
        _ => false,
    }
}

/// `yyyy-mm-dd`, optionally followed by a space and a clock.
fn is_date(s: &str) -> bool {
    // Text of any language comes through here: the slices below are byte positions.
    if !s.is_ascii() {
        return false;
    }
    let b = s.as_bytes();
    if b.len() < 10
        || !(all_digits(&s[..4]) && b[4] == b'-' && all_digits(&s[5..7]) && b[7] == b'-')
        || !all_digits(&s[8..10])
    {
        return false;
    }
    s.len() == 10 || (b[10] == b' ' && is_clock(&s[11..]))
}

/// Whether a text cell reads the same as some other kind of cell would, so that it needs the
/// marker to stay different.
fn collides(s: &str) -> bool {
    s.parse::<f64>().is_ok_and(|v| number_text(v) == s)
        || s == "TRUE"
        || s == "FALSE"
        || ERROR_TEXTS.contains(&s)
        || is_date(s)
        || is_clock(s)
}

/// `collides` on the text under its apostrophes: `'1` needs a marker of its own, or marked `1` and
/// the text `'1` would read alike.
fn needs_marker(s: &str) -> bool {
    collides(s.trim_start_matches('\''))
}

/// A cell's text without the marker when it carries one; any other text as it is. The inverse of
/// what `cell_text` does, so a text that merely starts with `'` (`'x`, `'1.50`) keeps it.
pub(crate) fn unmarked(s: &str) -> &str {
    match s.strip_prefix('\'') {
        Some(rest) if needs_marker(rest) => rest,
        _ => s,
    }
}

fn date_text(dt: &ExcelDateTime, is_1904: bool) -> String {
    let v = dt.as_f64();
    if !v.is_finite() || v < 0.0 {
        return number_text(v);
    }
    if dt.is_duration() {
        let total_ms = (v * 86_400_000.0).round();
        if total_ms >= 9.0e15 {
            return number_text(v);
        }
        let total_ms = total_ms as u64;
        let (secs, ms) = (total_ms / 1000, total_ms % 1000);
        let mut out = format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60);
        if ms != 0 {
            out.push_str(&format!(".{ms:03}"));
        }
        return out;
    }
    // The 1900 serial the instant would have, so both epochs decide alike.
    let serial = if is_1904 { v + 1462.0 } else { v };
    if serial >= 2_958_466.0 {
        return number_text(v);
    }
    let (y, mo, d, h, mi, s, ms) = dt.to_ymd_hms_milli();
    if h >= 24 {
        // A hair before midnight, rounded to the millisecond: it reads as that midnight.
        if v < 1.0 {
            return "00:00:00".to_string();
        }
        let next = v.floor() + 1.0;
        if serial.floor() + 1.0 >= 2_958_466.0 {
            return number_text(v);
        }
        let (y, mo, d, ..) =
            ExcelDateTime::new(next, ExcelDateTimeType::DateTime, is_1904).to_ymd_hms_milli();
        return format!("{y:04}-{mo:02}-{d:02}");
    }
    let clock = if ms == 0 {
        format!("{h:02}:{mi:02}:{s:02}")
    } else {
        format!("{h:02}:{mi:02}:{s:02}.{ms:03}")
    };
    // Under one day is a time of day, in either epoch (Excel stores a typed time that way).
    if v < 1.0 {
        clock
    } else if (h, mi, s, ms) == (0, 0, 0, 0) {
        format!("{y:04}-{mo:02}-{d:02}")
    } else {
        format!("{y:04}-{mo:02}-{d:02} {clock}")
    }
}

/// A `t="d"` cell (strict ISO workbooks) as a serial date reads: a space for the `T`, no empty
/// milliseconds and no midnight. What is not a date in that shape stays as it is.
fn iso_date_text(s: &str) -> String {
    if s.as_bytes().get(10) != Some(&b'T') {
        return s.to_string();
    }
    let spaced = s.replacen('T', " ", 1);
    if !is_date(&spaced) {
        return s.to_string();
    }
    let whole = spaced.strip_suffix(".000").unwrap_or(&spaced);
    whole.strip_suffix(" 00:00:00").unwrap_or(whole).to_string()
}

/// The table text of a stored value; `None` for an empty cell and for the empty string.
fn cell_text(value: &DataRef<'_>, is_1904: bool, mark_text: bool) -> Option<String> {
    let text = |s: &str| -> Option<String> {
        if s.is_empty() {
            None
        } else if mark_text && needs_marker(s) {
            Some(format!("'{s}"))
        } else {
            Some(s.to_string())
        }
    };
    match value {
        DataRef::Empty => None,
        DataRef::String(s) => text(s),
        DataRef::SharedString(s) => text(s),
        DataRef::Int(i) => Some(i.to_string()),
        DataRef::Float(f) => Some(number_text(*f)),
        DataRef::Bool(b) => Some(if *b { "TRUE" } else { "FALSE" }.to_string()),
        DataRef::DateTime(dt) => Some(date_text(dt, is_1904)),
        DataRef::DateTimeIso(s) => Some(iso_date_text(s)),
        DataRef::DurationIso(s) => Some(s.clone()),
        DataRef::Error(e) => Some(e.to_string()),
    }
}
