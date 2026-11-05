//! Excel workbooks in the table view: opening them, choosing a sheet per side, reading it, the
//! sheet picker and its keys. Core reads (`duet_core::table::xlsx`); this file is the load flow
//! (open both workbooks, choose the sheets, read them), the wording of what cannot be opened and
//! the picker.

use duet_core::table::xlsx::{
    OpenError, ReadFail, SheetEntry, TooLarge, UNREADABLE, Workbook, default_sheet, default_sheets,
    is_workbook_name,
};
use gpui_kit::component::Disableable as _;
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::{BackgroundExecutor, Task};

use std::collections::HashSet;

use super::*;
use crate::format::format_size;
use crate::i18n::{Lang, current, tr, tr_key, tr_str, trf};
use crate::ui::style::ld_dropdown;

actions!(
    table_sheet,
    [
        TableSheetPrevLeft,
        TableSheetNextLeft,
        TableSheetPrevRight,
        TableSheetNextRight
    ]
);

/// What is known about the workbook a side was read from.
#[derive(Debug, Clone)]
pub struct BookInfo {
    /// The worksheets in workbook order.
    pub sheets: Vec<SheetEntry>,
    /// The index into `sheets` of the sheet shown.
    pub shown: usize,
    /// The sheet row where the read stopped at the cell budget.
    pub cut_at_row: Option<u32>,
    pub formulas_without_result: u32,
    /// A text cell that reads like another kind of cell carries a leading `'`: on when both
    /// sides are workbooks, off beside a CSV, which has no cell types.
    pub marked: bool,
    /// The text cells of this read that carry the marker.
    pub marked_cells: u32,
    /// The sheet shown could not be read: there is no table, and the picker is the way on.
    pub failed: bool,
    /// What the read has to say, said again when the language changes.
    notes: Vec<LiveText>,
}

impl BookInfo {
    /// Says the notes again in the language now in force.
    pub(super) fn relabel(&mut self) {
        for note in &mut self.notes {
            note.relabel();
        }
    }
}

/// A workbook side: where to read it from and what the person asked for.
pub(super) struct BookSource {
    pub path: PathBuf,
    /// The sheet asked for by name (a pick, or the shown one when only the reading changes).
    pub wish: Option<String>,
    /// The text marker is on (the other side is a workbook too).
    pub mark_text: bool,
}

/// A workbook that was opened, and the file's size.
struct Opened {
    book: Workbook,
    size: u64,
}

/// Whether a path is read as a workbook (a pasted side never is).
pub(super) fn reads_as_book(path: &Path, pasted: bool) -> bool {
    !pasted && is_workbook_name(path)
}

/// A sentence of core's in the language in force. The reader's own words have no entry, and stay.
fn core_sentence(message: &str) -> String {
    match message.strip_prefix(UNREADABLE) {
        Some(detail) => format!("{}{detail}", tr(UNREADABLE)),
        None => tr_str(message).into_owned(),
    }
}

/// The reason a damaged file gives. English keeps core's detail. Thai keeps a sentence duet has
/// a Thai wording for, and for the library's own words (a zip or XML reader's) says only that the
/// file is damaged; the raw detail is in the log, where `open_book` and the sheet read put it.
fn damaged_reason(message: &str, whole: &'static str, with_detail: &'static str) -> String {
    let known = message.starts_with(UNREADABLE) || tr_str(message) != message;
    if current() == Lang::Th && !known {
        tr(whole).to_owned()
    } else {
        trf(with_detail, &[&core_sentence(message)])
    }
}

/// What the system said about a file it would not read.
fn io_reason(err: &std::io::Error) -> String {
    if matches!(err.raw_os_error(), Some(32 | 33)) {
        tr("is open in another program that does not let it be read. Close it or save a copy.")
            .to_owned()
    } else {
        format!("{err}.")
    }
}

/// "Can't open a.xlsx: …" for a file that is not a workbook, worded from the reason.
pub(super) fn refusal_text(name: &str, error: &OpenError) -> String {
    let reason = match error {
        OpenError::Password => tr(
            "is password protected. duet cannot open protected workbooks: remove the password in Excel and save a copy.",
        )
        .to_owned(),
        OpenError::OldXls => tr("is an old Excel file (.xls) saved under an .xlsx name.").to_owned(),
        OpenError::NotAZip => tr("is not an Excel workbook (it is not a zip file).").to_owned(),
        OpenError::NotAWorkbook => tr("is a zip file but not an Excel workbook.").to_owned(),
        OpenError::NoSheets => tr("has no worksheets.").to_owned(),
        OpenError::MacroSheet => {
            tr("holds an Excel 4 macro sheet this version cannot open.").to_owned()
        }
        OpenError::TooLarge(limit) => tr(match limit {
            TooLarge::Total => "unpacks to more than 2 GB, more than duet reads.",
            TooLarge::Part => "holds a part that unpacks to more than 1 GB, more than duet reads.",
            TooLarge::Packed => {
                "is packed more tightly than a real workbook (over 1000 to 1). It may be damaged or built to crash a reader."
            }
            TooLarge::Strings => {
                "says it holds more text than its size allows. It may be damaged or built to crash a reader."
            }
        })
        .to_owned(),
        OpenError::Damaged(message) => {
            damaged_reason(
                message,
                tr_key("is damaged and can't be read."),
                tr_key("is damaged: {}."),
            )
        }
        OpenError::Io(err) => io_reason(err),
        OpenError::Cancelled => tr(CANCELLED).to_owned(),
    };
    trf("Can't open {}: {}", &[&name, &reason])
}

fn message(text: impl Fn() -> String + Send + Sync + 'static) -> SideError {
    SideError::Message(LiveText::new(text))
}

/// "Can't read sheet “Q2” in a.xlsx: …" for one sheet of a workbook that opened.
fn sheet_refusal_text(name: &str, sheet: &str, fail: &ReadFail) -> String {
    let reason = match fail {
        ReadFail::CellError(value) => trf(
            "this sheet holds a cell error this version cannot read ({}).",
            &[value],
        ),
        ReadFail::Damaged(message) => damaged_reason(
            message,
            tr_key("the sheet is damaged and can't be read."),
            tr_key("the sheet is damaged: {}."),
        ),
        ReadFail::Io(err) => io_reason(err),
        ReadFail::Cancelled => tr(CANCELLED).to_owned(),
    };
    trf("Can't read sheet “{}” in {}: {}", &[&sheet, &name, &reason])
}

/// Opens the workbook at `path`: the size limit first, then the file's own checks.
fn open_book(name: &str, path: &Path, cancel: &CancelToken) -> Result<Opened, SideError> {
    cancel.check().map_err(|Cancelled| SideError::Cancelled)?;
    let name = name.to_owned();
    let size = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(err) => {
            let err = OpenError::Io(err);
            return Err(message(move || refusal_text(&name, &err)));
        }
    };
    if size > MAX_FILE_BYTES {
        return Err(message(move || {
            trf(
                "Can't open {}: {}",
                &[&name, &tr("the file is larger than 160 MB.")],
            )
        }));
    }
    match Workbook::open_with(path, cancel) {
        Ok(book) => Ok(Opened { book, size }),
        Err(OpenError::Cancelled) => Err(SideError::Cancelled),
        Err(err) => {
            tracing::warn!(file = %name, ?err, "workbook refused");
            Err(message(move || refusal_text(&name, &err)))
        }
    }
}

/// The sheet index each side opens on: its wish when the workbook still has that sheet, else the
/// default rule. Also the wish that is gone, for the notice.
fn choose(
    books: [Option<&[SheetEntry]>; 2],
    wishes: [Option<&str>; 2],
) -> [Option<(usize, Option<String>)>; 2] {
    let defaults = match books {
        [Some(left), Some(right)] => {
            let (l, r) = default_sheets(left, right);
            [l, r]
        }
        _ => [0, 1].map(|i| books[i].map_or(0, default_sheet)),
    };
    [0, 1].map(|i| {
        let sheets = books[i]?;
        let Some(wish) = wishes[i] else {
            return Some((defaults[i], None));
        };
        let wanted = wish.to_lowercase();
        let exact = sheets.iter().position(|s| s.name == wish);
        match exact.or_else(|| sheets.iter().position(|s| s.name.to_lowercase() == wanted)) {
            Some(found) => Some((found, None)),
            None => Some((defaults[i], Some(wish.to_owned()))),
        }
    })
}

/// What a read has to say, kept as data so a switch of language builds it again. The note on
/// formulas with no result is last: `book_notes` leaves it out when both sides have one.
fn notes(
    file: &str,
    sheet: &str,
    gone: Option<String>,
    cut_at_row: Option<u32>,
    formulas: u32,
) -> Vec<LiveText> {
    let mut notes = Vec::new();
    if let Some(gone) = gone {
        let (file, sheet) = (file.to_owned(), sheet.to_owned());
        notes.push(LiveText::new(move || {
            trf(
                "Sheet “{}” is no longer in {}. Showing “{}”.",
                &[&gone, &file, &sheet],
            )
        }));
    }
    if let Some(row) = cut_at_row {
        let (file, sheet) = (file.to_owned(), sheet.to_owned());
        notes.push(LiveText::new(move || {
            trf(
                "Only the first {} rows of “{}” in {} were read (the sheet is larger than duet reads).",
                &[&format_size(u64::from(row.saturating_sub(1))), &sheet, &file],
            )
        }));
    }
    if formulas == 1 {
        notes.push(LiveText::new(|| {
            tr("1 formula cell has no saved result, so it shows empty.").to_owned()
        }));
    } else if formulas > 1 {
        notes.push(LiveText::new(move || {
            trf(
                "{} formula cells have no saved result, so they show empty.",
                &[&format_size(u64::from(formulas))],
            )
        }));
    }
    notes
}

/// What a side keeps of a workbook, whether or not its sheet could be read.
fn kept_book(
    name: String,
    path: PathBuf,
    size: u64,
    (rows, columns): (usize, usize),
    book: BookInfo,
) -> Kept {
    Kept {
        info: SideInfo {
            name,
            path: Some(path),
            size,
            encoding: TextEncoding {
                encoding: encoding_rs::UTF_8,
                bom: false,
            },
            delimiter: Delimiter::Comma,
            auto: true,
            rows,
            columns,
            book: Some(book),
        },
        text: Arc::from(""),
        binary: false,
    }
}

/// Reads sheet `index` of an open workbook. Runs on the background executor.
fn read_book(
    opened: Opened,
    name: String,
    source: BookSource,
    index: usize,
    gone: Option<String>,
    cancel: &CancelToken,
    progress: &Progress,
) -> Result<Loaded, SideError> {
    let Opened { mut book, size } = opened;
    let sheets = book.sheets().to_vec();
    progress.bytes_per_row.store(1, Ordering::Relaxed);
    let mark_text = source.mark_text;
    let read = book.read_sheet_with(index, cancel, &progress.done, mark_text);
    progress.bytes_per_row.store(0, Ordering::Relaxed);
    let read = match read {
        Ok(read) => read,
        Err(ReadFail::Cancelled) => return Err(SideError::Cancelled),
        Err(fail) => {
            // The file opened: its other sheets stay within reach through the picker.
            let (file, sheet) = (name.clone(), sheets[index].name.clone());
            tracing::warn!(%file, %sheet, ?fail, "sheet refused");
            let kept = kept_book(
                name,
                source.path,
                size,
                (0, 0),
                BookInfo {
                    sheets,
                    shown: index,
                    cut_at_row: None,
                    formulas_without_result: 0,
                    marked: mark_text,
                    marked_cells: 0,
                    failed: true,
                    notes: Vec::new(),
                },
            );
            return Err(SideError::Sheet {
                kept: Box::new(kept),
                message: LiveText::new(move || sheet_refusal_text(&file, &sheet, &fail)),
            });
        }
    };
    let pick_gone = gone.is_some();
    let notes = notes(
        &name,
        &sheets[index].name,
        gone,
        read.cut_at_row,
        read.formulas_without_result,
    );
    let table = read.table;
    let kept = kept_book(
        name,
        source.path,
        size,
        (table.len(), table.width()),
        BookInfo {
            sheets,
            shown: index,
            cut_at_row: read.cut_at_row,
            formulas_without_result: read.formulas_without_result,
            marked: mark_text,
            marked_cells: read.marked_cells,
            failed: false,
            notes,
        },
    );
    Ok(Loaded {
        kept,
        table: Arc::new(table),
        pick_gone,
    })
}

/// Runs both sides' jobs off the UI thread, all of it. A newer load drops the future holding this;
/// what the work holds then (a finished workbook can take a second to free) goes with its own task.
pub(super) async fn run_jobs(
    background: &BackgroundExecutor,
    jobs: [Option<SideJob>; 2],
    cancel: &CancelToken,
    progress: &[Arc<Progress>; 2],
) -> [Option<Result<Loaded, SideError>>; 2] {
    let (inner, cancel, progress) = (background.clone(), cancel.clone(), progress.clone());
    background
        .spawn(async move { run_side_jobs(&inner, jobs, &cancel, &progress).await })
        .await
}

/// The workbooks are opened first (both at once), their sheets chosen with the two lists in hand,
/// then every side is read (both at once).
async fn run_side_jobs(
    background: &BackgroundExecutor,
    jobs: [Option<SideJob>; 2],
    cancel: &CancelToken,
    progress: &[Arc<Progress>; 2],
) -> [Option<Result<Loaded, SideError>>; 2] {
    // Stage 1: open each workbook.
    let mut opens: [Option<Task<Result<Opened, SideError>>>; 2] = [None, None];
    for (i, job) in jobs.iter().enumerate() {
        if let Some(SideJob {
            name,
            source: Source::Book(book),
            ..
        }) = job
        {
            let (name, path, cancel) = (name.clone(), book.path.clone(), cancel.clone());
            opens[i] = Some(background.spawn(async move { open_book(&name, &path, &cancel) }));
        }
    }
    let [left_open, right_open] = opens;
    let mut opened: [Option<Result<Opened, SideError>>; 2] = [None, None];
    opened[LEFT] = match left_open {
        Some(task) => Some(task.await),
        None => None,
    };
    opened[RIGHT] = match right_open {
        Some(task) => Some(task.await),
        None => None,
    };
    // The sheets: each side's wish, else the default rule over both lists.
    let lists: [Option<&[SheetEntry]>; 2] = [0, 1].map(|i| match &opened[i] {
        Some(Ok(o)) => Some(o.book.sheets()),
        _ => None,
    });
    let wishes: [Option<&str>; 2] = [0, 1].map(|i| match &jobs[i] {
        Some(SideJob {
            source: Source::Book(book),
            ..
        }) => book.wish.as_deref(),
        _ => None,
    });
    let chosen = choose(lists, wishes);
    // Stage 2: read every side.
    let mut reads: [Option<Task<Result<Loaded, SideError>>>; 2] = [None, None];
    let mut done: [Option<Result<Loaded, SideError>>; 2] = [None, None];
    for (i, job) in jobs.into_iter().enumerate() {
        let Some(job) = job else { continue };
        let cancel = cancel.clone();
        let progress = progress[i].clone();
        match job.source {
            Source::Book(book) => match opened[i].take() {
                Some(Ok(open)) => {
                    let (index, gone) = chosen[i].clone().unwrap_or((0, None));
                    let name = job.name;
                    reads[i] = Some(background.spawn(async move {
                        read_book(open, name, book, index, gone, &cancel, &progress)
                    }));
                }
                Some(Err(err)) => done[i] = Some(Err(err)),
                None => {}
            },
            source => {
                let job = SideJob {
                    name: job.name,
                    source,
                    forced: job.forced,
                };
                reads[i] =
                    Some(background.spawn(async move { load_side(job, &cancel, &progress) }));
            }
        }
    }
    let [left_read, right_read] = reads;
    if let Some(task) = left_read {
        done[LEFT] = Some(task.await);
    }
    if let Some(task) = right_read {
        done[RIGHT] = Some(task.await);
    }
    done
}

// ---- The view's side of it ----------------------------------------------------------------

/// The longest sheet name the picker button spells out; the menu and the tooltip hold the rest.
const LABEL_CHARS: usize = 14;

fn shortened(name: &str) -> String {
    if name.chars().count() <= LABEL_CHARS {
        name.to_owned()
    } else {
        let head: String = name.chars().take(LABEL_CHARS - 1).collect();
        format!("{head}…")
    }
}

impl TableView {
    fn book(&self, left: bool) -> Option<&BookInfo> {
        self.side(left)?.book.as_ref()
    }

    /// The sheets of a side read from a workbook, in workbook order.
    pub fn sheet_names(&self, left: bool) -> Option<Vec<String>> {
        Some(
            self.book(left)?
                .sheets
                .iter()
                .map(|s| s.name.clone())
                .collect(),
        )
    }

    /// The index of the sheet a side shows.
    pub fn shown_sheet(&self, left: bool) -> Option<usize> {
        Some(self.book(left)?.shown)
    }

    /// Whether the side's text cells carry the `'` marker (workbook against workbook).
    #[doc(hidden)]
    pub fn marks_text_for_test(&self, left: bool) -> Option<bool> {
        Some(self.book(left)?.marked)
    }

    /// The menu's entries as `(label, checked)`.
    #[doc(hidden)]
    pub fn sheet_menu_entries_for_test(&self, left: bool) -> Vec<(String, bool)> {
        self.sheet_entries(left)
    }

    /// The table of a side by address, to tell a table that was kept from one read again.
    #[doc(hidden)]
    pub fn table_ptr_for_test(&self, left: bool) -> Option<usize> {
        self.tables[side_index(left)]
            .as_ref()
            .map(|t| Arc::as_ptr(t) as usize)
    }

    fn sheet_entries(&self, left: bool) -> Vec<(String, bool)> {
        let Some(book) = self.book(left) else {
            return Vec::new();
        };
        book.sheets
            .iter()
            .enumerate()
            .map(|(i, sheet)| {
                let label = if sheet.hidden {
                    trf("{} (hidden)", &[&sheet.name])
                } else {
                    sheet.name.clone()
                };
                (label, i == book.shown)
            })
            .collect()
    }

    /// Shows sheet `index` on a side; only that side is read again.
    pub fn pick_sheet(&mut self, left: bool, index: usize, cx: &mut Context<Self>) {
        let name = self
            .book(left)
            .and_then(|book| book.sheets.get(index))
            .map(|sheet| sheet.name.clone());
        if let Some(name) = name {
            self.pick_sheet_named(left, &name, cx);
        }
    }

    /// Shows the sheet of that name on a side. A name, not a position: a menu built before a read
    /// may meet a workbook whose sheets have moved. While another file loads on that side, the book
    /// in hand is the old file's, and its sheets are not this file's to pick from.
    pub fn pick_sheet_named(&mut self, left: bool, name: &str, cx: &mut Context<Self>) {
        let side = side_index(left);
        let same_file = self.side(left).and_then(|info| info.path.as_deref())
            == Some(self.paths[side].as_path());
        let Some(index) = self
            .book(left)
            .filter(|_| same_file)
            .and_then(|book| book.sheets.iter().position(|sheet| sheet.name == name))
        else {
            return;
        };
        // Asking again for the sheet on its way changes nothing; a side Esc left empty reads it.
        let settled = self.is_busy() || self.tables[side].is_some();
        if settled && Some(index) == self.current_sheet(left) {
            return;
        }
        self.sheet_picks[side] = Some(name.to_owned());
        self.start(Mode::Resheet(side), cx);
    }

    /// The sheet a side is on or on its way to: while a pick is being read, that pick, so a second
    /// key press moves on from it and going back is not going nowhere.
    fn current_sheet(&self, left: bool) -> Option<usize> {
        let book = self.book(left)?;
        let on_its_way = self.sheet_picks[side_index(left)]
            .as_deref()
            .filter(|_| self.is_busy())
            .and_then(|pick| book.sheets.iter().position(|sheet| sheet.name == pick));
        Some(on_its_way.unwrap_or(book.shown))
    }

    /// Ctrl+PageUp / Ctrl+PageDown: the previous or next sheet; the ends stop.
    fn step_sheet(&mut self, left: bool, forward: bool, cx: &mut Context<Self>) {
        let Some(current) = self.current_sheet(left) else {
            return;
        };
        let next = if forward {
            current + 1
        } else {
            let Some(previous) = current.checked_sub(1) else {
                return;
            };
            previous
        };
        self.pick_sheet(left, next, cx);
    }

    /// The listeners of the four sheet keys.
    pub(super) fn bind_sheet_keys<E: InteractiveElement>(element: E, cx: &mut Context<Self>) -> E {
        element
            .on_action(cx.listener(|this, _: &TableSheetPrevLeft, w, cx| {
                if this.takes_key(w, cx) {
                    this.step_sheet(true, false, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TableSheetNextLeft, w, cx| {
                if this.takes_key(w, cx) {
                    this.step_sheet(true, true, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TableSheetPrevRight, w, cx| {
                if this.takes_key(w, cx) {
                    this.step_sheet(false, false, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TableSheetNextRight, w, cx| {
                if this.takes_key(w, cx) {
                    this.step_sheet(false, true, cx)
                }
            }))
    }

    /// The sheet picker of a side read from a workbook: "Sheet: Q1", with the sheets in its menu.
    /// Disabled with one sheet.
    pub(super) fn sheet_picker(
        &self,
        left: bool,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let book = self.book(left)?;
        let shown = &book.sheets[book.shown];
        let id = if left {
            "table-sheet-left"
        } else {
            "table-sheet-right"
        };
        let entries = self.sheet_entries(left);
        let names: Vec<String> = book.sheets.iter().map(|sheet| sheet.name.clone()).collect();
        let this = cx.entity().downgrade();
        let focus = self.focus.clone();
        Some(
            ld_dropdown(id, trf("Sheet: {}", &[&shortened(&shown.name)]), UI_SIZE)
                .tooltip(tr(
                    "Choose the sheet (Ctrl+PageUp / Ctrl+PageDown; with Shift for the right side)",
                ))
                .disabled(book.sheets.len() < 2)
                .dropdown_menu(move |menu, _, _| {
                    let focus = focus.clone();
                    entries
                        .iter()
                        .zip(&names)
                        .fold(menu, |menu, ((label, checked), name)| {
                            let (this, focus, name) = (this.clone(), focus.clone(), name.clone());
                            menu.item(
                                crate::ui::menu::row(label.clone())
                                    .checked(*checked)
                                    .on_click(move |_, window, cx| {
                                        let _ = this.update(cx, |view, cx| {
                                            view.pick_sheet_named(left, &name, cx)
                                        });
                                        window.focus(&focus, cx);
                                    }),
                            )
                        })
                }),
        )
    }

    /// The Delimiter menu applies to a CSV side: with two workbooks there is none to set.
    pub(super) fn shows_delimiter(&self) -> bool {
        [true, false]
            .into_iter()
            .filter(|&left| self.book(left).is_some())
            .count()
            < 2
    }

    /// Exactly one side is a workbook: the Delimiter menu is for the other.
    pub(super) fn delimiter_csv_only(&self) -> bool {
        [true, false]
            .into_iter()
            .filter(|&left| self.book(left).is_some())
            .count()
            == 1
    }

    /// What a side read from a workbook says of itself, in place of "utf-8 · comma".
    pub(super) fn sheet_label(&self, left: bool) -> Option<String> {
        let book = self.book(left)?;
        Some(trf(
            "sheet “{}” ({} of {})",
            &[
                &book.sheets[book.shown].name,
                &(book.shown + 1),
                &book.sheets.len(),
            ],
        ))
    }

    /// The notes of the sides' reads (a sheet that was cut, formulas without a result, a pick
    /// that is gone), said in the language in force.
    pub(super) fn book_notes(&self) -> Vec<String> {
        let books: Vec<&BookInfo> = self
            .kept
            .iter()
            .flatten()
            .filter_map(|kept| kept.info.book.as_ref())
            .collect();
        // Formulas with no result on both sides are one sentence naming the sides, not the
        // same sentence twice. A read's formula note is its last (`notes`).
        let both = books
            .iter()
            .filter(|b| b.formulas_without_result > 0)
            .count()
            > 1;
        let mut out: Vec<String> = Vec::new();
        for book in &books {
            let keep = if both && book.formulas_without_result > 0 {
                book.notes.len().saturating_sub(1)
            } else {
                book.notes.len()
            };
            out.extend(book.notes[..keep].iter().map(ToString::to_string));
        }
        if both {
            let count = |left: bool| self.book(left).map_or(0, |b| b.formulas_without_result);
            out.push(trf(
                "Formula cells have no saved result, so they show empty: left {}, right {}.",
                &[
                    &format_size(u64::from(count(true))),
                    &format_size(u64::from(count(false))),
                ],
            ));
        }
        out
    }

    /// Says the notes again in the language now in force. Nothing is read.
    pub(super) fn relabel_books(&mut self) {
        for kept in self.kept.iter_mut().flatten() {
            if let Some(book) = &mut kept.info.book {
                book.relabel();
            }
        }
    }

    /// What the pair as a whole has to say, in the language in force: a workbook beside a CSV
    /// compares what is stored, and a `'` needs explaining where there is one to see.
    pub(super) fn pair_notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        let books = [self.book(true), self.book(false)];
        let (left, right) = (self.side(true), self.side(false));
        if left.is_some() && right.is_some() && books.iter().flatten().count() == 1 {
            notes.push(
                tr("The workbook side compares its stored values, not what Excel shows: a 25% cell is 0.25, and dates read 2024-03-05.")
                    .to_owned(),
            );
        }
        if books
            .iter()
            .flatten()
            .any(|book| book.marked && book.marked_cells > 0)
        {
            notes.push(
                tr("A leading ' marks text that reads like a number, date or other value, as in Excel.")
                    .to_owned(),
            );
        }
        notes
    }

    /// The sheets a workbook does not show, counted once per name across both, case aside as in
    /// Excel. A sheet shown on one side only is still one the other side did not compare.
    fn other_sheets(&self) -> usize {
        let mut names = HashSet::new();
        for book in [self.book(true), self.book(false)].into_iter().flatten() {
            for (index, sheet) in book.sheets.iter().enumerate() {
                if index != book.shown {
                    names.insert(sheet.name.to_lowercase());
                }
            }
        }
        names.len()
    }

    /// "Only in a.xlsx: Q3, Old." for a workbook's sheets the other workbook has no name for.
    fn only_here_note(&self, left: bool) -> Option<String> {
        const MOST: usize = 8;
        let (book, other) = (self.book(left)?, self.book(!left)?);
        let names: Vec<&str> = book
            .sheets
            .iter()
            .enumerate()
            .filter(|(index, sheet)| {
                let wanted = sheet.name.to_lowercase();
                *index != book.shown
                    && !other.sheets.iter().any(|o| o.name.to_lowercase() == wanted)
            })
            .map(|(_, sheet)| sheet.name.as_str())
            .collect();
        if names.is_empty() {
            return None;
        }
        let mut list = names[..names.len().min(MOST)].join(", ");
        if names.len() > MOST {
            list.push_str(", …");
        }
        Some(trf("Only in {}: {}.", &[&self.side(left)?.name, &list]))
    }

    /// "2 other sheets were not compared."
    fn others_note(count: usize) -> Option<String> {
        match count {
            0 => None,
            1 => Some(tr("1 other sheet was not compared.").to_owned()),
            many => Some(trf(
                "{} other sheets were not compared.",
                &[&format_size(many as u64)],
            )),
        }
    }

    /// What the status line adds to a result of a workbook pair: what else there was to compare.
    pub(super) fn unchecked_notes(&self) -> Vec<String> {
        let mut notes: Vec<String> = Self::others_note(self.other_sheets()).into_iter().collect();
        notes.extend(self.only_here_note(true));
        notes.extend(self.only_here_note(false));
        notes
    }

    /// What a report says of a side: the sheets it did not compare and what its read had to say, in
    /// the language in force, whatever the window's.
    pub(super) fn side_notes(&self, left: bool) -> Vec<String> {
        let Some(book) = self.book(left) else {
            return Vec::new();
        };
        let mut notes: Vec<String> = Self::others_note(book.sheets.len().saturating_sub(1))
            .into_iter()
            .collect();
        notes.extend(self.only_here_note(left));
        notes.extend(book.notes.iter().map(LiveText::said));
        notes
    }

    /// The result speaks for all that was asked: no other sheet waits, nothing was cut and no
    /// formula went without a result. Two library-written files whose formulas differ read alike.
    pub(super) fn vouches_for_all(&self) -> bool {
        self.other_sheets() == 0
            && [self.book(true), self.book(false)]
                .into_iter()
                .flatten()
                .all(|book| book.cut_at_row.is_none() && book.formulas_without_result == 0)
    }

    /// What the status says of a result with no differences.
    pub(super) fn identical_text(&self) -> String {
        let books: Vec<&BookInfo> = [self.book(true), self.book(false)]
            .into_iter()
            .flatten()
            .collect();
        if books.iter().any(|book| book.cut_at_row.is_some()) {
            return tr("The rows read are identical. The rest of the sheet was not read.")
                .to_owned();
        }
        if books.iter().any(|book| book.formulas_without_result > 0) {
            return tr(
                "The values read are identical. Formula cells with no saved result were not compared.",
            )
            .to_owned();
        }
        if self.other_sheets() == 0 {
            return tr("The tables are identical cell for cell.").to_owned();
        }
        let name = |book: &BookInfo| book.sheets[book.shown].name.clone();
        match books.as_slice() {
            [left, right] if name(left).to_lowercase() != name(right).to_lowercase() => trf(
                "“{}” and “{}” are identical cell for cell.",
                &[&name(left), &name(right)],
            ),
            [one, ..] => trf("Sheet “{}” is identical cell for cell.", &[&name(one)]),
            [] => tr("The tables are identical cell for cell.").to_owned(),
        }
    }

    /// A sheet could not be read and the workbook holds others to go to.
    pub(super) fn another_sheet_to_try(&self) -> bool {
        [self.book(true), self.book(false)]
            .into_iter()
            .flatten()
            .any(|book| book.failed && book.sheets.len() > 1)
    }

    /// The side job of a workbook side, or `None` when its table is kept as it is.
    pub(super) fn book_job(&self, index: usize, mode: Mode) -> Option<SideJob> {
        let path = self.paths[index].clone();
        let other = 1 - index;
        let mark_text = reads_as_book(&self.paths[other], self.pasted[other]);
        // The sheet this file shows. A side loading another file holds the old file's book, and a
        // sheet that could not be read is not one to walk back into.
        let shown = self.kept[index]
            .as_ref()
            .filter(|k| k.info.path.as_deref() == Some(path.as_path()))
            .and_then(|k| k.info.book.as_ref())
            .filter(|book| !book.failed)
            .map(|book| (book.sheets[book.shown].name.clone(), book.marked));
        // A delimiter change or a paste does not touch a workbook, unless the pair stopped
        // being (or became) two workbooks: then its cells are worded again.
        if mode == Mode::Reparse
            && self.tables[index].is_some()
            && let Some((_, marked)) = &shown
            && *marked == mark_text
        {
            return None;
        }
        // Reading the same file again keeps its sheet: the default rule is for a file just opened,
        // and would move the sheet with no word (after F9 it can choose another).
        let stays = matches!(mode, Mode::Reparse | Mode::Refresh | Mode::Resheet(_));
        let wish = self.sheet_picks[index]
            .clone()
            .or_else(|| shown.filter(|_| stays).map(|(name, _)| name));
        Some(SideJob {
            name: file_name(&path),
            source: Source::Book(BookSource {
                path,
                wish,
                mark_text,
            }),
            forced: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Lang, scoped};

    #[test]
    fn a_sharing_violation_is_worded() {
        for code in [32, 33] {
            let text = refusal_text(
                "a.xlsx",
                &OpenError::Io(std::io::Error::from_raw_os_error(code)),
            );
            assert!(text.starts_with("Can't open a.xlsx: "), "{text}");
            assert!(text.contains("is open in another program"), "{text}");
        }
        let denied = refusal_text(
            "a.xlsx",
            &OpenError::Io(std::io::Error::from_raw_os_error(5)),
        );
        assert!(!denied.contains("another program"), "{denied}");
        assert!(denied.ends_with('.'), "{denied}");
    }

    #[test]
    fn a_damaged_file_gives_the_readers_reason() {
        let text = refusal_text("a.xlsx", &OpenError::Damaged("bad xml".to_owned()));
        assert_eq!(text, "Can't open a.xlsx: is damaged: bad xml.");
    }

    #[test]
    fn a_file_past_the_limits_says_so_and_names_itself() {
        for (limit, words) in [
            (TooLarge::Total, "unpacks to more than 2 GB"),
            (TooLarge::Part, "unpacks to more than 1 GB"),
            (TooLarge::Packed, "packed more tightly than a real workbook"),
            (TooLarge::Strings, "more text than its size allows"),
        ] {
            let text = refusal_text("a.xlsx", &OpenError::TooLarge(limit));
            assert!(text.starts_with("Can't open a.xlsx: "), "{text}");
            assert!(text.contains(words), "{text}");
            assert!(!text.contains("damaged:"), "{text}");
        }
    }

    #[test]
    fn an_excel_4_macro_sheet_is_not_called_damage() {
        assert_eq!(
            refusal_text("m.xlsm", &OpenError::MacroSheet),
            "Can't open m.xlsm: holds an Excel 4 macro sheet this version cannot open."
        );
    }

    #[test]
    fn a_sheet_that_stops_is_worded_for_that_sheet() {
        let text = sheet_refusal_text("a.xlsx", "Q2", &ReadFail::CellError("#SPILL!".to_owned()));
        assert_eq!(
            text,
            "Can't read sheet “Q2” in a.xlsx: this sheet holds a cell error this version cannot read (#SPILL!)."
        );
        let text = sheet_refusal_text("a.xlsx", "Q2", &ReadFail::Damaged("bad xml".to_owned()));
        assert_eq!(
            text,
            "Can't read sheet “Q2” in a.xlsx: the sheet is damaged: bad xml."
        );
        for code in [32, 33] {
            let text = sheet_refusal_text(
                "a.xlsx",
                "Q2",
                &ReadFail::Io(std::io::Error::from_raw_os_error(code)),
            );
            assert!(text.contains("is open in another program"), "{text}");
            assert!(!text.contains("damaged"), "{text}");
        }
    }

    #[test]
    fn core_sentences_inside_a_refusal_are_said_in_thai() {
        let _thai = scoped(Lang::Th);
        for (sentence, thai) in [
            ("the rows are out of order", "แถวเรียงผิดลำดับ"),
            ("the cells of a row are out of order", "เซลล์ในแถวเรียงผิดลำดับ"),
            ("there is no such sheet", "ไม่มีชีตนี้"),
        ] {
            let text = refusal_text("a.xlsx", &OpenError::Damaged(sentence.to_owned()));
            assert_eq!(text, format!("เปิด a.xlsx ไม่ได้: ไฟล์เสียหาย: {thai}"));
        }
        let text = refusal_text("a.xlsx", &OpenError::Damaged(UNREADABLE.to_owned()));
        assert!(text.contains("อ่านเนื้อหาของไฟล์ไม่ได้"), "{text}");
        let text = refusal_text(
            "a.xlsx",
            &OpenError::Damaged(format!("{UNREADABLE} (boom)")),
        );
        assert!(text.contains("อ่านเนื้อหาของไฟล์ไม่ได้ (boom)"), "{text}");
        // The reader's own words are a library's, so Thai says only that the file is damaged.
        let text = refusal_text(
            "trunc.xlsx",
            &OpenError::Damaged("Zip error: invalid Zip archive: Could not find EOCD".to_owned()),
        );
        assert_eq!(text, "เปิด trunc.xlsx ไม่ได้: ไฟล์เสียหาย อ่านไม่ได้");
        let text = sheet_refusal_text(
            "a.xlsx",
            "Q2",
            &ReadFail::Damaged("Xml error: x".to_owned()),
        );
        assert_eq!(text, "อ่านชีต “Q2” ใน a.xlsx ไม่ได้: ชีตเสียหาย อ่านไม่ได้");
        // A sentence of duet's own keeps its Thai wording on a sheet too.
        let text = sheet_refusal_text(
            "a.xlsx",
            "Q2",
            &ReadFail::Damaged("the rows are out of order".to_owned()),
        );
        assert!(text.ends_with("ชีตเสียหาย: แถวเรียงผิดลำดับ"), "{text}");
    }

    #[test]
    fn a_read_made_in_one_language_is_said_in_the_other_on_receipt() {
        let mut said = notes("a.xlsx", "Q1", None, Some(6), 2);
        assert!(
            said[0].starts_with("Only the first 5 rows"),
            "{}",
            &*said[0]
        );
        let _thai = scoped(Lang::Th);
        for note in &mut said {
            note.relabel();
        }
        assert!(said[0].starts_with("อ่านเฉพาะ 5 แถวแรก"), "{}", &*said[0]);
        assert!(said[1].starts_with("มี 2 เซลล์สูตร"), "{}", &*said[1]);
    }

    #[test]
    fn an_exact_name_wins_over_one_that_differs_in_case() {
        let sheet = |name: &str| SheetEntry {
            name: name.to_owned(),
            hidden: false,
        };
        let book = [sheet("q1"), sheet("Q1"), sheet("Other")];
        assert_eq!(
            choose([Some(&book), None], [Some("Q1"), None])[0],
            Some((1, None))
        );
        assert_eq!(
            choose([Some(&book), None], [Some("q1"), None])[0],
            Some((0, None))
        );
        assert_eq!(
            choose([Some(&book), None], [Some("OTHER"), None])[0],
            Some((2, None))
        );
    }

    #[test]
    fn sheets_are_chosen_by_wish_then_default_and_a_lost_wish_is_reported() {
        let sheet = |name: &str, hidden| SheetEntry {
            name: name.to_owned(),
            hidden,
        };
        let left = [sheet("Notes", false), sheet("Q1", false)];
        let right = [sheet("q1", false)];
        let chosen = choose([Some(&left), Some(&right)], [None, None]);
        assert_eq!(chosen, [Some((1, None)), Some((0, None))]);
        let chosen = choose([Some(&left), Some(&right)], [Some("Q2"), Some("Q1")]);
        assert_eq!(chosen, [Some((1, Some("Q2".to_owned()))), Some((0, None))]);
        let chosen = choose([Some(&left), None], [Some("notes"), None]);
        assert_eq!(chosen, [Some((0, None)), None]);
    }
}
