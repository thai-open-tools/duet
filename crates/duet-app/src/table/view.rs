//! The table compare tab (R3, R9, R11): both CSV/TSV files are read, decoded, parsed and
//! compared off the UI thread, then shown side by side in `grid`. The decoded text of each side
//! is kept, so a delimiter change parses again without reading the file.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use duet_core::cancel::{CancelToken, Cancelled};
use duet_core::table::compare::{CompareOptions, TableDiff, compare};
use duet_core::table::nav::{FIND_CELLS, Filter, View, filtered};
use duet_core::table::parse::{detect_delimiter, parse};
use duet_core::table::{Delimiter, Table};
use duet_core::text::encoding::{self, TextEncoding, is_binary};
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::v_flex;
use gpui_kit::{
    App, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding, Pixels,
    ScrollDelta, ScrollWheelEvent, SharedString, Subscription, Task, Window, actions, div,
    prelude::*, px,
};

use crate::format::format_size;
use crate::job::JobSlot;
use crate::palette::Palette;
use crate::pathbar::ActiveEdit;
use crate::probe::ScrollProbe;
use crate::settings;
use crate::table::prefs::TablePrefs;
use crate::tabs::badge_text;
use crate::ui::fonts::{Fonts, UI_SIZE};

mod actions;
mod detail;
mod export;
mod find;
mod grid;
mod open;
mod status;
mod toolbar;

pub use actions::{MenuColumn, MenuTarget, key_token};
pub use detail::{DETAIL_HINT, DETAIL_WINDOW_CHARS, Detail, DetailSide};
pub use grid::{CellMark, display_text, text_width};

/// A listener that moves the selection unless a field has the keys.
macro_rules! move_key {
    ($cx:expr, $action:ty, $step:expr) => {
        $cx.listener(|this, _: &$action, w, cx| {
            if this.takes_key(w, cx) {
                this.move_selection($step, cx)
            }
        })
    };
}

/// A listener that sets the row filter unless a field has the keys.
macro_rules! filter_key {
    ($cx:expr, $action:ty, $filter:expr) => {
        $cx.listener(|this, _: &$action, w, cx| {
            if this.takes_key(w, cx) {
                this.set_prefs(|prefs| prefs.filter = $filter, cx)
            }
        })
    };
}

/// A listener that jumps to a differing row unless a field has the keys.
macro_rules! jump_key {
    ($cx:expr, $action:ty, $jump:expr) => {
        $cx.listener(|this, _: &$action, w, cx| {
            if this.takes_key(w, cx) {
                this.jump($jump, cx)
            }
        })
    };
}

/// Where a navigation key takes the selection among the differing rows (LocalDiff `goDiff`).
#[derive(Clone, Copy)]
enum Jump {
    First,
    Prev,
    Next,
    Last,
}

/// Where a movement key takes the selection (LocalDiff `moveSel`).
#[derive(Clone, Copy)]
enum Move {
    Up,
    Down,
    PageUp,
    PageDown,
    Left,
    Right,
}

actions!(
    table,
    [
        TableEscape,
        TableReload,
        TableSwap,
        TableNext,
        TablePrev,
        TableFirst,
        TableLast,
        TableUp,
        TableDown,
        TableLeft,
        TableRight,
        TablePageUp,
        TablePageDown,
        TableShowAll,
        TableShowDiffs,
        TableShowSame,
        TableFind,
        TableFindNext,
        TableFindPrev,
        TableGoTo,
        TableOpenLeft,
        TableOpenRight
    ]
);

pub const CONTEXT: &str = "TableView";
pub const ROW_HEIGHT: f32 = 22.0;
/// A side larger than this is refused (R3).
pub const MAX_FILE_BYTES: u64 = 160 * 1024 * 1024;

/// The key box applies this long after the last change.
const KEYS_DELAY: Duration = Duration::from_millis(600);
const CANCELLED: &str = "Cancelled: the result is incomplete.";
/// How often the "Parsing…" status is redrawn while a side parses.
const PROGRESS_TICK: Duration = Duration::from_millis(250);

pub fn key_bindings() -> Vec<KeyBinding> {
    let c = Some(CONTEXT);
    vec![
        KeyBinding::new("escape", TableEscape, c),
        KeyBinding::new("f5", TableReload, c),
        KeyBinding::new("f9", TableSwap, c),
        KeyBinding::new("f8", TableNext, c),
        KeyBinding::new("alt-down", TableNext, c),
        KeyBinding::new("n", TableNext, c),
        KeyBinding::new("f7", TablePrev, c),
        KeyBinding::new("alt-up", TablePrev, c),
        KeyBinding::new("p", TablePrev, c),
        KeyBinding::new("ctrl-home", TableFirst, c),
        KeyBinding::new("ctrl-end", TableLast, c),
        KeyBinding::new("up", TableUp, c),
        KeyBinding::new("down", TableDown, c),
        KeyBinding::new("left", TableLeft, c),
        KeyBinding::new("right", TableRight, c),
        KeyBinding::new("pageup", TablePageUp, c),
        KeyBinding::new("pagedown", TablePageDown, c),
        KeyBinding::new("ctrl-1", TableShowAll, c),
        KeyBinding::new("ctrl-2", TableShowDiffs, c),
        KeyBinding::new("ctrl-3", TableShowSame, c),
        KeyBinding::new("ctrl-f", TableFind, c),
        KeyBinding::new("f3", TableFindNext, c),
        KeyBinding::new("shift-f3", TableFindPrev, c),
        KeyBinding::new("ctrl-g", TableGoTo, c),
        KeyBinding::new("ctrl-o", TableOpenLeft, c),
        KeyBinding::new("ctrl-shift-o", TableOpenRight, c),
    ]
}

pub enum TableEvent {
    /// Escape with nothing to cancel: back to the folder tab this one was opened from.
    Back,
    Swapped {
        left: PathBuf,
        right: PathBuf,
    },
    Open {
        left: PathBuf,
        right: PathBuf,
    },
}

/// What is known about one loaded side. `rows` counts records, the header included.
#[derive(Debug, Clone)]
pub struct SideInfo {
    pub name: String,
    /// None for pasted text.
    pub path: Option<PathBuf>,
    pub size: u64,
    pub encoding: TextEncoding,
    pub delimiter: Delimiter,
    /// The delimiter was detected, not forced.
    pub auto: bool,
    pub rows: usize,
    pub columns: usize,
}

/// A side's decoded text, kept so a delimiter change parses it again without reading the file.
#[derive(Clone)]
struct Kept {
    info: SideInfo,
    text: Arc<str>,
    /// The first 8 KiB held a NUL: it gets a notice.
    binary: bool,
}

/// Bytes parsed so far and the average bytes per record (0 while the side is not parsing),
/// which the status turns into "N rows".
#[derive(Default)]
struct Progress {
    done: AtomicU64,
    bytes_per_row: AtomicU64,
}

enum Source {
    File(PathBuf),
    Kept(Kept),
}

struct SideJob {
    name: String,
    source: Source,
    forced: Option<Delimiter>,
}

enum SideError {
    Cancelled,
    Message(String),
}

struct Loaded {
    kept: Kept,
    table: Arc<Table>,
}

/// A finished compare and what it was made from.
struct Compared {
    diff: TableDiff,
    tables: [Option<Arc<Table>>; 2],
    options: CompareOptions,
}

/// How much of the pipeline a start redoes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Read the files again (new paths).
    Reload,
    /// Read the files again and keep the result shown until the new one lands (F5, T-59).
    Refresh,
    /// Parse the kept text again (a delimiter change).
    Reparse,
    /// Only pair the rows again (any other option, a swap).
    Recompare,
}

const LEFT: usize = 0;
const RIGHT: usize = 1;

fn side_index(left: bool) -> usize {
    if left { LEFT } else { RIGHT }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn pasted_name(left: bool) -> String {
    format!("Pasted text ({})", if left { "left" } else { "right" })
}

/// The kept text of a pasted side (R10): UTF-8, no path, named for its side.
fn pasted_kept(left: bool, text: &str) -> Kept {
    Kept {
        info: SideInfo {
            name: pasted_name(left),
            path: None,
            size: text.len() as u64,
            encoding: TextEncoding {
                encoding: encoding_rs::UTF_8,
                bom: false,
            },
            delimiter: Delimiter::Comma,
            auto: true,
            rows: 0,
            columns: 0,
        },
        text: Arc::from(text),
        binary: is_binary(text.as_bytes()),
    }
}

/// Reads, decodes and parses one side (R3). Runs on the background executor.
fn load_side(job: SideJob, cancel: &CancelToken, progress: &Progress) -> Result<Loaded, SideError> {
    let SideJob {
        name,
        source,
        forced,
    } = job;
    let (mut kept, auto_name) = match source {
        Source::Kept(kept) => (kept, name),
        Source::File(path) => {
            let open_error = |err: String| SideError::Message(format!("Can't open {name}: {err}"));
            let meta = std::fs::metadata(&path).map_err(|err| open_error(format!("{err}.")))?;
            if meta.len() > MAX_FILE_BYTES {
                return Err(open_error("the file is larger than 160 MB.".into()));
            }
            let bytes = std::fs::read(&path).map_err(|err| open_error(format!("{err}.")))?;
            cancel.check().map_err(|Cancelled| SideError::Cancelled)?;
            let binary = is_binary(&bytes);
            let decoded = encoding::decode(&bytes);
            let text: Arc<str> = Arc::from(&*decoded.text);
            let kept = Kept {
                info: SideInfo {
                    name: name.clone(),
                    path: Some(path),
                    size: bytes.len() as u64,
                    encoding: decoded.encoding,
                    delimiter: Delimiter::Comma,
                    auto: true,
                    rows: 0,
                    columns: 0,
                },
                text,
                binary,
            };
            (kept, name)
        }
    };
    let (delimiter, auto) = match forced {
        Some(delimiter) => (delimiter, false),
        None => (detect_delimiter(&kept.text, &auto_name), true),
    };
    progress
        .bytes_per_row
        .store(bytes_per_row(&kept.text), Ordering::Relaxed);
    let parsed = parse(&kept.text, delimiter, auto, cancel, &progress.done);
    progress.bytes_per_row.store(0, Ordering::Relaxed);
    let table = parsed.map_err(|Cancelled| SideError::Cancelled)?;
    kept.info.delimiter = table.delimiter;
    kept.info.auto = table.auto;
    kept.info.rows = table.len();
    kept.info.columns = table.width();
    Ok(Loaded {
        kept,
        table: Arc::new(table),
    })
}

/// The average bytes per record in the first 64 KiB (at least 1): the row count of a parse in
/// progress is estimated from the bytes done, since the parser reports only bytes.
fn bytes_per_row(text: &str) -> u64 {
    let mut end = text.len().min(64 * 1024);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let sample = &text.as_bytes()[..end];
    let lines = sample.iter().filter(|&&b| b == b'\n').count().max(1);
    (sample.len() / lines).max(1) as u64
}

pub struct TableView {
    paths: [PathBuf; 2],
    pasted: [bool; 2],
    /// The sides the last `TableEvent::Open` named: `load_paths` replaces their pasted text.
    opened: [bool; 2],
    kept: [Option<Kept>; 2],
    tables: [Option<Arc<Table>>; 2],
    /// Why a side could not be opened.
    messages: [Option<SharedString>; 2],
    diff: Option<TableDiff>,
    /// The tables and the options `diff` was made from. Its rows index these, so while a swap
    /// or an option change recompares, the old result still draws from its own tables.
    compared: [Option<Arc<Table>>; 2],
    compared_options: CompareOptions,
    /// The display rows (R7): the rows of `diff` that the filter shows.
    view: View,
    /// Column widths for `diff`, measured on its first frame (R8).
    layout: Option<grid::Layout>,
    /// The selected row (an index into `diff.rows`) and column.
    selection: Option<(usize, Option<usize>)>,
    /// Where the last right press landed: a row, a column name or neither.
    menu_press: actions::MenuPress,
    /// The display row at the top of the body.
    top: usize,
    /// How far both halves and the header are scrolled right, in px.
    scroll_x: f32,
    /// Touchpad scrolling not yet a whole row, in px.
    wheel_rest: f32,
    /// The rows area and the scrollbars as the last frame drew them.
    body_bounds: Rc<Cell<Bounds<Pixels>>>,
    vscroll_bounds: Rc<Cell<Bounds<Pixels>>>,
    hscroll_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// The left button went down on a scrollbar and is still held, and where its thumb was
    /// grabbed: px from the thumb's start.
    dragging_vscroll: bool,
    vscroll_grab: f32,
    dragging_hscroll: bool,
    hscroll_grab: f32,
    /// What the last frame built, for the tests: cells in all, and each half's columns.
    built_cells: usize,
    built_columns: [std::ops::Range<usize>; 2],
    /// When the last load started, for the "first rows" log line.
    load_started: Instant,
    first_rows_logged: bool,
    probe: Option<ScrollProbe>,
    prefs: TablePrefs,
    /// The key columns box, the key spec it last applied or showed, and the pause before a
    /// change applies by itself.
    keys_input: Entity<InputState>,
    keys_shown: String,
    keys_task: Option<Task<()>>,
    _keys_subscription: Subscription,
    /// The key spec the bad-key notice was last shown for (LocalDiff `warnedKeys`).
    warned_keys: Option<String>,
    /// The side whose name is being edited as a path.
    path_edit: Option<ActiveEdit>,
    job: JobSlot,
    task: Option<Task<()>>,
    /// Redraws the progress status while the job runs.
    ticker: Option<Task<()>>,
    progress: [Arc<Progress>; 2],
    progress_text: Option<String>,
    notice: Option<SharedString>,
    /// Notice for a side that looks binary: it follows the loaded side, not the last compare.
    binary_notice: Option<SharedString>,
    /// The find bar and the go-to bar (at most one is open), and the last text found.
    find: Option<find::Bar>,
    goto: Option<find::Bar>,
    last_find: String,
    /// A find that ran out of cells, which the next F3 in its direction goes on with, and the
    /// cells one find call may look at.
    find_paused: Option<find::Paused>,
    find_cells: usize,
    /// Esc stopped the load: what is shown is incomplete.
    cancelled: bool,
    focus: FocusHandle,
}

impl EventEmitter<TableEvent> for TableView {}

impl Focusable for TableView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TableView {
    pub fn new(left: PathBuf, right: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::blank(left, right, window, cx);
        view.start(Mode::Reload, cx);
        view
    }

    /// A view whose two sides are pasted text.
    #[doc(hidden)]
    pub fn with_text_for_test(
        left: &str,
        right: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::blank(PathBuf::new(), PathBuf::new(), window, cx);
        view.pasted = [true, true];
        for (index, text) in [left, right].into_iter().enumerate() {
            view.kept[index] = Some(pasted_kept(index == LEFT, text));
        }
        view.start(Mode::Reload, cx);
        view
    }

    fn blank(left: PathBuf, right: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prefs = TablePrefs::load(settings::settings_file(cx).as_deref());
        let keys_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder(toolbar::KEYS_PLACEHOLDER);
            state.set_value(prefs.keys.clone(), window, cx);
            state
        });
        let _keys_subscription = cx.subscribe_in(
            &keys_input,
            window,
            |this: &mut Self, _, event: &InputEvent, _, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => this.apply_keys(cx),
                InputEvent::Change => this.keys_changed(cx),
                _ => {}
            },
        );
        Self {
            paths: [left, right],
            pasted: [false, false],
            opened: [false, false],
            kept: [None, None],
            tables: [None, None],
            messages: [None, None],
            diff: None,
            compared: [None, None],
            compared_options: CompareOptions::default(),
            view: View::default(),
            layout: None,
            selection: None,
            menu_press: actions::MenuPress::Nothing,
            top: 0,
            scroll_x: 0.,
            wheel_rest: 0.,
            body_bounds: Rc::default(),
            vscroll_bounds: Rc::default(),
            hscroll_bounds: Rc::default(),
            dragging_vscroll: false,
            vscroll_grab: 0.,
            dragging_hscroll: false,
            hscroll_grab: 0.,
            built_cells: 0,
            built_columns: [0..0, 0..0],
            load_started: Instant::now(),
            first_rows_logged: false,
            probe: ScrollProbe::from_env(),
            keys_shown: prefs.keys.clone(),
            prefs,
            keys_input,
            keys_task: None,
            _keys_subscription,
            warned_keys: None,
            path_edit: None,
            job: JobSlot::default(),
            task: None,
            ticker: None,
            progress: [Arc::default(), Arc::default()],
            progress_text: None,
            notice: None,
            binary_notice: None,
            find: None,
            goto: None,
            last_find: String::new(),
            find_paused: None,
            find_cells: FIND_CELLS,
            cancelled: false,
            // A tab stop, so Tab comes back to the view after a button or field.
            focus: cx.focus_handle().tab_stop(true),
        }
    }

    /// Reading, parsing or comparing.
    pub fn is_busy(&self) -> bool {
        self.job.is_running()
    }

    pub fn diff(&self) -> Option<&TableDiff> {
        self.diff.as_ref()
    }

    pub fn side(&self, left: bool) -> Option<&SideInfo> {
        self.kept[side_index(left)].as_ref().map(|k| &k.info)
    }

    /// Why that side could not be opened.
    pub fn message(&self, left: bool) -> Option<&str> {
        self.messages[side_index(left)].as_deref()
    }

    /// The status note: the parse progress while a side parses, else the last notice, else the
    /// note on a binary-looking side.
    pub fn notice(&self) -> Option<&str> {
        self.progress_text
            .as_deref()
            .or(self.notice.as_deref())
            .or(self.binary_notice.as_deref())
    }

    /// The key box, for the tests to give it the keys.
    #[doc(hidden)]
    pub fn keys_input_for_test(&self) -> Entity<InputState> {
        self.keys_input.clone()
    }

    /// The key box changed: it applies after a pause, unless it changes again first.
    fn keys_changed(&mut self, cx: &mut Context<Self>) {
        self.keys_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(KEYS_DELAY).await;
            let _ = this.update(cx, |view, cx| view.apply_keys(cx));
        }));
    }

    /// Makes the key box's text the key columns, when it differs from the current ones.
    fn apply_keys(&mut self, cx: &mut Context<Self>) {
        self.keys_task = None;
        let keys = self.keys_input.read(cx).value().trim().to_owned();
        if keys == self.prefs.keys {
            return;
        }
        self.keys_shown.clone_from(&keys);
        self.set_prefs(|prefs| prefs.keys = keys, cx);
    }

    /// The box shows the keys in the options when they changed some other way.
    fn sync_keys_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.prefs.keys == self.keys_shown {
            return;
        }
        self.keys_shown.clone_from(&self.prefs.keys);
        let keys = self.prefs.keys.clone();
        self.keys_input
            .update(cx, |state, cx| state.set_value(keys, window, cx));
    }

    /// "“a.csv” looks like a binary file, not CSV or TSV." for each side that does.
    fn binary_note(&self) -> Option<SharedString> {
        let notes: Vec<String> = self
            .kept
            .iter()
            .flatten()
            .filter(|kept| kept.binary)
            .map(|kept| {
                format!(
                    "“{}” looks like a binary file, not CSV or TSV.",
                    kept.info.name
                )
            })
            .collect();
        (!notes.is_empty()).then(|| notes.join(" ").into())
    }

    /// The two paths; empty for a pasted side.
    pub fn paths(&self) -> (&Path, &Path) {
        (&self.paths[LEFT], &self.paths[RIGHT])
    }

    pub fn prefs(&self) -> &TablePrefs {
        &self.prefs
    }

    /// The tab's badge (R11): "ok" for identical tables, otherwise the rows that are not the
    /// same; "!" when a side did not open; None while busy or without a result.
    pub fn badge(&self) -> Option<String> {
        if self.is_busy() {
            return None;
        }
        if self.messages.iter().any(Option::is_some) {
            return Some("!".to_owned());
        }
        let diff = self.diff.as_ref()?;
        if diff.is_identical() {
            return Some(badge_text(0));
        }
        let counts = &diff.counts;
        match counts.differs + counts.left_only + counts.right_only {
            // Only columns differ: no row count to show.
            0 => Some("≠".to_owned()),
            n => Some(badge_text(n)),
        }
    }

    /// Changes the options. A new delimiter parses both sides again; any other option pairs
    /// the rows again; the filter only changes what is shown. The options are saved.
    pub fn set_prefs(&mut self, change: impl FnOnce(&mut TablePrefs), cx: &mut Context<Self>) {
        let before = self.prefs.clone();
        change(&mut self.prefs);
        if self.prefs == before {
            return;
        }
        if let Err(err) = self.prefs.save(settings::settings_file(cx).as_deref()) {
            tracing::warn!(%err, "could not remember the table options");
        }
        if self.prefs.filter != before.filter {
            self.refilter(cx);
        }
        if self.prefs.delimiter != before.delimiter {
            self.start(Mode::Reparse, cx);
        } else if self.prefs.compare_options() != before.compare_options() {
            self.start(Mode::Recompare, cx);
        } else {
            cx.notify();
        }
    }

    /// Shows other files in this tab (the workspace re-keys the tab first).
    pub fn load_paths(&mut self, left: PathBuf, right: PathBuf, cx: &mut Context<Self>) {
        // Pasted text stays on a side the open did not name, if its path did not change.
        let named = std::mem::take(&mut self.opened);
        let paths = [left, right];
        self.pasted =
            [LEFT, RIGHT].map(|i| self.pasted[i] && !named[i] && self.paths[i] == paths[i]);
        self.paths = paths;
        self.path_edit = None;
        self.top = 0;
        self.scroll_x = 0.;
        self.start(Mode::Reload, cx);
    }

    /// Reads the files again; a pasted side keeps its text. The result stays shown until the
    /// new one lands, and Esc keeps it (T-59).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.start(Mode::Refresh, cx);
    }

    /// F9: the sides change places, in this tab and in its key.
    pub fn swap_sides(&mut self, cx: &mut Context<Self>) {
        self.paths.swap(LEFT, RIGHT);
        self.path_edit = None;
        self.pasted.swap(LEFT, RIGHT);
        self.kept.swap(LEFT, RIGHT);
        self.tables.swap(LEFT, RIGHT);
        self.messages.swap(LEFT, RIGHT);
        for index in [LEFT, RIGHT] {
            if self.pasted[index]
                && let Some(kept) = &mut self.kept[index]
            {
                kept.info.name = pasted_name(index == LEFT);
            }
        }
        self.binary_notice = self.binary_note();
        cx.emit(TableEvent::Swapped {
            left: self.paths[LEFT].clone(),
            right: self.paths[RIGHT].clone(),
        });
        self.start(Mode::Recompare, cx);
    }

    fn side_name(&self, index: usize) -> String {
        self.kept[index]
            .as_ref()
            .map_or_else(|| file_name(&self.paths[index]), |k| k.info.name.clone())
    }

    fn side_job(&self, index: usize, mode: Mode) -> Option<SideJob> {
        if mode == Mode::Recompare {
            return None;
        }
        // A file side's kept text is parsed again only when its table finished loading: while
        // a newer file (or F5) is still loading, the kept text is the old file's. `start` asks
        // before it clears the tables.
        let kept = self.kept[index].clone();
        let source = match kept {
            Some(kept)
                if self.pasted[index]
                    || (mode == Mode::Reparse && self.tables[index].is_some()) =>
            {
                Source::Kept(kept)
            }
            _ => Source::File(self.paths[index].clone()),
        };
        // A file is named for its path: a re-keyed tab must not keep the old file's name.
        let name = match &source {
            Source::File(path) => file_name(path),
            Source::Kept(kept) => kept.info.name.clone(),
        };
        Some(SideJob {
            name,
            source,
            forced: self.prefs.delimiter,
        })
    }

    /// Starts the pipeline: read and parse each side (both at once), then pair the rows, all
    /// off the UI thread. A newer start supersedes this one.
    fn start(&mut self, mode: Mode, cx: &mut Context<Self>) {
        // Without both tables there is nothing to only pair again.
        let mode = if mode == Mode::Recompare && self.tables.iter().any(Option::is_none) {
            Mode::Reparse
        } else {
            mode
        };
        let jobs: [Option<SideJob>; 2] = std::array::from_fn(|i| self.side_job(i, mode));
        let (generation, cancel) = self.job.start();
        self.cancelled = false;
        self.notice = None;
        self.progress_text = None;
        self.progress = [Arc::default(), Arc::default()];
        self.ticker = Some(Self::ticker(cx));
        self.load_started = Instant::now();
        self.first_rows_logged = false;
        if mode != Mode::Recompare {
            if mode != Mode::Refresh {
                self.clear_diff();
            }
            self.tables = [None, None];
            self.messages = [None, None];
        }
        let options = self.prefs.compare_options();
        let made_with = options.clone();
        let progress = self.progress.clone();
        let background = cx.background_executor().clone();
        self.task = Some(cx.spawn(async move |this, cx| {
            let [left_job, right_job] = jobs;
            let spawn_side = |job: Option<SideJob>, progress: Arc<Progress>| {
                let cancel = cancel.clone();
                job.map(|job| background.spawn(async move { load_side(job, &cancel, &progress) }))
            };
            let left = spawn_side(left_job, progress[LEFT].clone());
            let right = spawn_side(right_job, progress[RIGHT].clone());
            let left = match left {
                Some(task) => Some(task.await),
                None => None,
            };
            let right = match right {
                Some(task) => Some(task.await),
                None => None,
            };
            let Ok(Some((a, b))) =
                this.update(cx, |view, cx| view.loaded(generation, [left, right], cx))
            else {
                return;
            };
            let tables = [a.clone(), b.clone()];
            let outcome = background
                .spawn(async move { compare(a.as_deref(), b.as_deref(), &options, &cancel) })
                .await;
            let compared = outcome.ok().map(|diff| Compared {
                diff,
                tables,
                options: made_with,
            });
            let _ = this.update(cx, |view, cx| view.apply(generation, compared, cx));
        }));
        cx.notify();
    }

    /// Redraws the status line every `PROGRESS_TICK` while the job runs, so the row count of a
    /// long parse moves.
    fn ticker(cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PROGRESS_TICK).await;
                let going = this.update(cx, |view, cx| {
                    let going = view.is_busy();
                    view.progress_text = going.then(|| view.parsing_text()).flatten();
                    if going {
                        cx.notify();
                    }
                    going
                });
                if !going.unwrap_or(false) {
                    break;
                }
            }
        })
    }

    /// "Parsing b.csv… 12,000 rows" for the first side that is parsing.
    fn parsing_text(&self) -> Option<String> {
        (0..2).find_map(|index| {
            let per_row = self.progress[index].bytes_per_row.load(Ordering::Relaxed);
            (per_row != 0).then(|| {
                let rows = self.progress[index].done.load(Ordering::Relaxed) / per_row;
                format!(
                    "Parsing {}… {} rows",
                    self.side_name(index),
                    format_size(rows)
                )
            })
        })
    }

    /// Both sides of load `generation` are read and parsed (or failed). Returns the tables to
    /// pair while that load is still the current one and both sides opened.
    #[allow(clippy::type_complexity)]
    fn loaded(
        &mut self,
        generation: u64,
        results: [Option<Result<Loaded, SideError>>; 2],
        cx: &mut Context<Self>,
    ) -> Option<(Option<Arc<Table>>, Option<Arc<Table>>)> {
        if !self.job.is_current(generation) {
            return None;
        }
        for (index, result) in results.into_iter().enumerate() {
            match result {
                None => {}
                Some(Ok(loaded)) => {
                    self.tables[index] = Some(loaded.table);
                    self.kept[index] = Some(loaded.kept);
                    self.messages[index] = None;
                }
                Some(Err(SideError::Message(message))) => {
                    self.tables[index] = None;
                    self.kept[index] = None;
                    self.messages[index] = Some(message.into());
                }
                Some(Err(SideError::Cancelled)) => {
                    self.job.finish(generation);
                    cx.notify();
                    return None;
                }
            }
        }
        self.binary_notice = self.binary_note();
        cx.notify();
        if self.messages.iter().any(Option::is_some) {
            self.job.finish(generation);
            self.clear_diff();
            // No compare follows to clear the last "Parsing…" tick.
            self.progress_text = None;
            return None;
        }
        Some((self.tables[LEFT].clone(), self.tables[RIGHT].clone()))
    }

    /// A new result replaces the shown one: the filter's rows are rebuilt, the widths measured
    /// again on the next frame and the selection cleared; the scroll position stays (R9).
    fn apply(&mut self, generation: u64, outcome: Option<Compared>, cx: &mut Context<Self>) {
        if !self.job.is_current(generation) {
            return;
        }
        self.job.finish(generation);
        if let Some(Compared {
            diff,
            tables,
            options,
        }) = outcome
        {
            self.view = filtered(&diff, self.prefs.filter);
            self.find_paused = None;
            self.diff = Some(diff);
            self.compared = tables;
            self.compared_options = options;
            self.layout = None;
            self.selection = None;
            // The binary note stays visible beside what the compare found.
            self.notice = self
                .compare_notice()
                .map(|found| match &self.binary_notice {
                    Some(binary) => format!("{binary} {found}").into(),
                    None => found,
                });
        }
        self.progress_text = None;
        cx.notify();
    }

    /// What is wrong with the options a compare ran with: key columns that match nothing (once
    /// per key box value, LocalDiff `warnedKeys`) and a header that names no common column.
    fn compare_notice(&mut self) -> Option<SharedString> {
        let diff = self.diff.as_ref().filter(|diff| diff.both)?;
        let mut notices = Vec::new();
        let spec = &self.prefs.keys;
        if diff.bad_keys.is_empty() {
            self.warned_keys = None;
        } else {
            if self.warned_keys.as_ref() != Some(spec) {
                let mut notice = format!(
                    "Key column not found on both sides: {}.",
                    diff.bad_keys.join(", ")
                );
                if diff.keys.is_empty() {
                    notice.push_str(" Rows are aligned by order.");
                }
                notices.push(notice);
            }
            self.warned_keys = Some(spec.clone());
        }
        if self.compared_options.header && diff.matched.is_empty() && !diff.columns.is_empty() {
            notices.push(
                "No column names match. Turn off “First row is a header” to compare by position."
                    .to_owned(),
            );
        }
        (!notices.is_empty()).then(|| notices.join(" ").into())
    }

    /// No result is shown (a side is loading again or failed).
    fn clear_diff(&mut self) {
        self.diff = None;
        self.compared = [None, None];
        self.view = View::default();
        self.find_paused = None;
        self.layout = None;
        self.selection = None;
    }

    /// The filter changed: its rows are rebuilt. A selected row it still shows comes to the
    /// middle; otherwise the rows start at the top (LocalDiff `setFilter`).
    fn refilter(&mut self, cx: &mut Context<Self>) {
        self.view = self
            .diff
            .as_ref()
            .map(|diff| filtered(diff, self.prefs.filter))
            .unwrap_or_default();
        self.find_paused = None;
        match self.selection {
            Some((row, column)) if self.view.position(row).is_some() => {
                self.select(row, column, true, cx)
            }
            _ => self.top = 0,
        }
        cx.notify();
    }

    /// Stops the load. What was shown stays, and the status says it is incomplete.
    fn cancel_load(&mut self, cx: &mut Context<Self>) {
        self.job.cancel();
        self.task = None;
        self.ticker = None;
        self.cancelled = true;
        // An F5 stopped before its files landed: the tables of the result still shown are
        // the current ones again, so Export and a recompare work on them.
        if self.diff.is_some() && self.tables.iter().all(Option::is_none) {
            self.tables = self.compared.clone();
        }
        self.progress_text = None;
        self.notice = Some(CANCELLED.into());
        cx.notify();
    }

    pub fn top_row(&self) -> usize {
        self.top
    }

    /// Whole rows that fit the body, as the last frame measured it; at least one.
    pub fn visible_rows(&self) -> usize {
        let height = f32::from(self.body_bounds.get().size.height);
        ((height / ROW_HEIGHT).floor() as usize).max(1)
    }

    /// The top row of the last page, which ends on the last row.
    fn max_top(&self) -> usize {
        self.view.len().saturating_sub(self.visible_rows())
    }

    /// Puts display row `pos` at the top, clamped so the last page stays full.
    pub fn scroll_to_row(&mut self, pos: usize, cx: &mut Context<Self>) {
        let pos = pos.min(self.max_top());
        if pos != self.top {
            self.top = pos;
            cx.notify();
        }
    }

    fn scroll_by(&mut self, rows: isize, cx: &mut Context<Self>) {
        self.scroll_to_row(self.top.saturating_add_signed(rows), cx);
    }

    pub fn scroll_x(&self) -> f32 {
        self.scroll_x
    }

    /// The selected row (an index into `diff.rows`) and column.
    pub fn selection(&self) -> Option<(usize, Option<usize>)> {
        self.selection
    }

    /// The display rows: the rows the filter shows.
    pub fn view(&self) -> &View {
        &self.view
    }

    /// Selects paired row `row` and, optionally, a column. With `scroll`, the row comes to the
    /// middle of the page and the column into view (LocalDiff `scrollToRow(p, 'center')`).
    pub fn select(
        &mut self,
        row: usize,
        column: Option<usize>,
        scroll: bool,
        cx: &mut Context<Self>,
    ) {
        self.selection = Some((row, column));
        if scroll {
            if let Some(pos) = self.view.position(row) {
                self.scroll_to_row(pos.saturating_sub(self.visible_rows() / 2), cx);
            }
            if let Some(column) = column {
                self.scroll_column_into_view(column, cx);
            }
        }
        cx.notify();
    }

    /// An arrow or page key. Up and Down move one display row, the page keys a page less one;
    /// without a selection (or with it filtered out) the first row is taken. Left and Right
    /// move the column within a selected row, clamped to the columns (LocalDiff `moveSel`).
    fn move_selection(&mut self, step: Move, cx: &mut Context<Self>) {
        let Some(columns) = self.diff.as_ref().map(|diff| diff.columns.len()) else {
            return;
        };
        let len = self.view.len();
        if len == 0 {
            return;
        }
        let page = self.visible_rows().saturating_sub(1).max(1) as isize;
        let (rows, cols) = match step {
            Move::Up => (-1, 0),
            Move::Down => (1, 0),
            Move::PageUp => (-page, 0),
            Move::PageDown => (page, 0),
            Move::Left => (0, -1),
            Move::Right => (0, 1),
        };
        if cols != 0 && self.selection.is_none() {
            return;
        }
        let pos = match self.selection.and_then(|(row, _)| self.view.position(row)) {
            Some(pos) => pos.saturating_add_signed(rows).min(len - 1),
            None => 0,
        };
        let column = self.selection.and_then(|(_, column)| column);
        let column = if cols != 0 && columns > 0 {
            Some(
                column
                    .map_or(0, |c| c.saturating_add_signed(cols))
                    .min(columns - 1),
            )
        } else {
            column
        };
        let row = self.view.rows[pos] as usize;
        self.select(row, column, true, cx);
    }

    /// A wheel notch moves at least one row; a touchpad's small steps add up to rows. A
    /// sideways delta, or Shift with the wheel, scrolls both halves sideways by px.
    fn on_wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(ROW_HEIGHT));
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        // Windows already sends Shift+wheel as `x`; elsewhere it may still arrive as `y`.
        let (dx, dy) = if event.modifiers.shift && dx == 0. {
            (dy, 0.)
        } else {
            (dx, dy)
        };
        if dx != 0. {
            self.scroll_x_to(self.scroll_x - dx, cx);
        }
        if dy == 0. {
            return;
        }
        let rows = match event.delta {
            ScrollDelta::Lines(_) => {
                let rows = (-dy / ROW_HEIGHT).round();
                if rows == 0. { -dy.signum() } else { rows }
            }
            ScrollDelta::Pixels(_) => {
                self.wheel_rest -= dy;
                let rows = (self.wheel_rest / ROW_HEIGHT).trunc();
                self.wheel_rest -= rows * ROW_HEIGHT;
                rows
            }
        };
        self.scroll_by(rows as isize, cx);
    }

    /// `DUET_AUTOSCROLL_SECS`: about 1,000 rows further each frame, then the stats and quit.
    fn autoscroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view.is_empty() {
            return;
        }
        let Some(probe) = self.probe.as_mut() else {
            return;
        };
        let done = probe.frame();
        self.top = (self.top + 997) % self.view.len();
        window.request_animation_frame();
        if done {
            cx.quit();
        }
    }

    /// Esc cancels a running load, else closes the find or go-to bar, else clears the selection,
    /// else goes back to the folder tab (R15, built up per PF3). The bars' fields close
    /// themselves on Esc; the key box keeps it.
    fn on_escape(&mut self, _: &TableEscape, window: &mut Window, cx: &mut Context<Self>) {
        if self.path_edit.is_some() {
            self.cancel_path_edit(window, cx);
            return;
        }
        if !self.bar_open() && !self.takes_key(window, cx) {
            return;
        }
        if self.job.is_running() {
            self.cancel_load(cx);
        } else if self.bar_open() {
            self.close_bars(window, cx);
        } else if self.selection.is_some() {
            self.selection = None;
            cx.notify();
        } else {
            cx.emit(TableEvent::Back);
        }
    }

    /// Keys that mean something in a field stay with it: the table's keys run only when no
    /// input of this view has the keys, else they propagate to the input.
    fn takes_key(&self, window: &Window, cx: &mut Context<Self>) -> bool {
        let free = !self
            .inputs(cx)
            .iter()
            .any(|input| input.contains_focused(window, cx));
        if !free {
            cx.propagate();
        }
        free
    }

    /// The focus handles of the view's inputs: the key box and the open bar's field.
    fn inputs(&self, cx: &App) -> Vec<FocusHandle> {
        let mut inputs = vec![self.keys_input.focus_handle(cx)];
        inputs.extend(self.bar_inputs(cx));
        inputs.extend(self.path_input(cx));
        inputs
    }

    /// F7, F8, Ctrl+Home, Ctrl+End: selects a differing row of the filter's rows and its first
    /// differing column. There is no wrap (R14).
    fn jump(&mut self, jump: Jump, cx: &mut Context<Self>) {
        let Some(diff) = self.diff.as_ref() else {
            return;
        };
        let stops = &self.view.stops;
        if stops.is_empty() {
            let note = if self.prefs.filter == Filter::Same {
                "The “Same” filter has no differing rows."
            } else {
                "No differing rows."
            };
            self.notice = Some(note.into());
            cx.notify();
            return;
        }
        // The selected row's display position, if it is shown.
        let at = self.selection.and_then(|(row, _)| self.view.position(row));
        let target = match jump {
            Jump::First => Some(stops[0]),
            Jump::Last => stops.last().copied(),
            Jump::Next => {
                let next = at.map_or(0, |at| stops.partition_point(|&s| s as usize <= at));
                stops.get(next).copied()
            }
            // With nothing selected, Previous starts from the last one.
            Jump::Prev => match at {
                None => stops.last().copied(),
                Some(at) => stops
                    .partition_point(|&s| (s as usize) < at)
                    .checked_sub(1)
                    .map(|i| stops[i]),
            },
        };
        let Some(pos) = target else {
            let note = if matches!(jump, Jump::Next) {
                "No more differences."
            } else {
                "No earlier differences."
            };
            self.notice = Some(note.into());
            cx.notify();
            return;
        };
        let row = self.view.rows[pos as usize] as usize;
        let column = diff.diff_columns(row).first().map(|&c| c as usize);
        self.notice = None;
        self.select(row, column, true, cx);
    }

    /// What the body says while there is no result to draw.
    fn summary(&self) -> String {
        if let Some(message) = self.messages.iter().flatten().next() {
            message.to_string()
        } else if self.is_busy() {
            "Comparing…".to_owned()
        } else if self.cancelled {
            CANCELLED.to_owned()
        } else {
            String::new()
        }
    }
}

impl Render for TableView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.autoscroll(window, cx);
        self.sync_keys_input(window, cx);
        let palette = Palette::current(cx);
        let fonts = Fonts::current(cx);
        let body = if self.diff.is_some() {
            self.render_grid(&palette, window, cx).into_any_element()
        } else {
            self.built_cells = 0;
            self.built_columns = [0..0, 0..0];
            let summary = self.summary();
            v_flex()
                .id("table-placeholder")
                .test_support()
                .aria_label(summary.clone())
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(palette.muted)
                .child(summary)
                .into_any_element()
        };
        let root = v_flex()
            .id("table-view")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .bg(palette.bg)
            .text_color(palette.text)
            .text_size(px(UI_SIZE))
            .on_action(cx.listener(Self::on_escape))
            .on_action(cx.listener(|this, _: &TableReload, w, cx| {
                if this.takes_key(w, cx) {
                    this.reload(cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TableSwap, w, cx| {
                if this.takes_key(w, cx) {
                    this.swap_sides(cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TableFind, w, cx| this.open_find(w, cx)))
            .on_action(cx.listener(|this, _: &TableFindNext, w, cx| this.find_step(true, w, cx)))
            .on_action(cx.listener(|this, _: &TableFindPrev, w, cx| this.find_step(false, w, cx)))
            .on_action(cx.listener(|this, _: &TableGoTo, w, cx| this.open_goto(w, cx)))
            .on_action(move_key!(cx, TableUp, Move::Up))
            .on_action(move_key!(cx, TableDown, Move::Down))
            .on_action(move_key!(cx, TablePageUp, Move::PageUp))
            .on_action(move_key!(cx, TablePageDown, Move::PageDown))
            .on_action(move_key!(cx, TableLeft, Move::Left))
            .on_action(move_key!(cx, TableRight, Move::Right))
            .on_action(jump_key!(cx, TableNext, Jump::Next))
            .on_action(jump_key!(cx, TablePrev, Jump::Prev))
            .on_action(jump_key!(cx, TableFirst, Jump::First))
            .on_action(jump_key!(cx, TableLast, Jump::Last))
            .on_action(filter_key!(cx, TableShowAll, Filter::All))
            .on_action(filter_key!(cx, TableShowDiffs, Filter::Differences))
            .on_action(filter_key!(cx, TableShowSame, Filter::Same));
        Self::bind_actions(root, cx)
            .child(self.render_toolbar(&palette, &fonts, cx))
            .child(self.render_options(&palette, &fonts, cx))
            .children(self.render_find(&palette, cx))
            .children(self.render_goto(&palette, cx))
            .child(body)
            .child(self.render_detail(&palette, &fonts))
            .child(self.render_status(&palette))
    }
}
