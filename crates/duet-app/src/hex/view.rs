//! The hex compare tab (R3, R4): both files side by side, byte by byte, read-only. The files
//! are opened off the UI thread and their rows show as soon as they are; the diff ranges
//! follow from the same background job. The viewport is a `u64` top row, so a 4 GiB pair
//! scrolls to its last row exactly; rows wider than the body scroll sideways by px. `rows`
//! draws the rows, the ruler, both scrollbars and the overview strip.

use std::cell::Cell;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

use duet_core::cancel::CancelToken;
use duet_core::hex::diff::{DiffRows, HexDiff, blocks, diff_ranges};
use duet_core::hex::format::parse_offset;
use duet_core::hex::{HexPair, HexSource};
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{
    App, Bounds, Context, Entity, EventEmitter, ExternalPaths, FocusHandle, Focusable, KeyBinding,
    Pixels, ScrollDelta, ScrollWheelEvent, SharedString, Subscription, Task, Window, actions, div,
    prelude::*, px,
};

use crate::job::JobSlot;
use crate::palette::Palette;
use crate::pathbar::ActiveEdit;
use crate::probe::ScrollProbe;
use crate::settings;
use crate::ui::fonts::{Fonts, UI_SIZE};
use select::Step;

// The find bar lives beside this folder, as `hex/find.rs`, but is a child module so it can
// reach the view's private state.
#[path = "find.rs"]
mod find;
mod open;
mod rows;
#[macro_use]
mod select;
mod status;
mod toolbar;

pub use rows::{ByteMark, RowMarks};

actions!(
    hex,
    [
        HexEscape,
        HexReload,
        HexSwap,
        HexNext,
        HexPrev,
        HexFirst,
        HexLast,
        HexShowAll,
        HexOnlyDiff,
        HexGoTo,
        HexRowUp,
        HexRowDown,
        HexPageUp,
        HexPageDown,
        HexTop,
        HexBottom,
        HexLeft,
        HexRight,
        HexSelLeft,
        HexSelRight,
        HexSelUp,
        HexSelDown,
        HexSelPageUp,
        HexSelPageDown,
        HexSelHome,
        HexSelEnd,
        HexFind,
        HexFindNext,
        HexFindPrev,
        HexCopy,
        HexCopyOffset,
        HexOpenLeft,
        HexOpenRight
    ]
);

pub const CONTEXT: &str = "HexView";
pub const ROW_HEIGHT: f32 = 20.0;
/// The bytes per row offered (R12).
pub const BYTES_PER_ROW: [u64; 3] = [8, 16, 32];
/// The key in `settings.json` that remembers the bytes per row.
pub const BYTES_PER_ROW_KEY: &str = "hex.bytes_per_row";
const DEFAULT_BYTES_PER_ROW: u64 = 16;

const OPEN_HINT: &str = "Press F5 to try again.";
const NO_MORE: &str = "No more differences.";
/// A cursor and a selection (side, offset or range) that F9 keeps across the reopen.
type Carry = (Option<(bool, u64)>, Option<(bool, Range<u64>)>);

const STILL_COMPARING: &str = "Still comparing…";
const CANCELLED: &str = "Cancelled: the result is incomplete.";
const SHOWING_ALL: &str = "Showing all rows to reach that offset.";
const BAD_OFFSET: &str = "Type an offset such as 0x1F40, 1F40h or 8000.";
const PAST_END: &str = "Past the end: went to the last byte.";
const TRUNCATED_NOTICE: &str =
    "More than 200,000 differences: navigation covers the first 200,000.";
/// How often the status line's progress is redrawn while comparing.
const PROGRESS_TICK: Duration = Duration::from_millis(250);

/// Where a navigation key goes.
#[derive(Clone, Copy)]
enum Nav {
    First,
    Prev,
    Next,
    Last,
}

pub fn key_bindings() -> Vec<KeyBinding> {
    let c = Some(CONTEXT);
    vec![
        KeyBinding::new("escape", HexEscape, c),
        KeyBinding::new("f5", HexReload, c),
        KeyBinding::new("f9", HexSwap, c),
        KeyBinding::new("f8", HexNext, c),
        KeyBinding::new("alt-down", HexNext, c),
        KeyBinding::new("ctrl-n", HexNext, c),
        KeyBinding::new("f7", HexPrev, c),
        KeyBinding::new("alt-up", HexPrev, c),
        KeyBinding::new("ctrl-p", HexPrev, c),
        KeyBinding::new("ctrl-home", HexFirst, c),
        KeyBinding::new("ctrl-end", HexLast, c),
        KeyBinding::new("ctrl-1", HexShowAll, c),
        KeyBinding::new("ctrl-2", HexOnlyDiff, c),
        KeyBinding::new("ctrl-g", HexGoTo, c),
        KeyBinding::new("up", HexRowUp, c),
        KeyBinding::new("down", HexRowDown, c),
        KeyBinding::new("pageup", HexPageUp, c),
        KeyBinding::new("pagedown", HexPageDown, c),
        KeyBinding::new("home", HexTop, c),
        KeyBinding::new("end", HexBottom, c),
        KeyBinding::new("left", HexLeft, c),
        KeyBinding::new("right", HexRight, c),
        KeyBinding::new("shift-left", HexSelLeft, c),
        KeyBinding::new("shift-right", HexSelRight, c),
        KeyBinding::new("shift-up", HexSelUp, c),
        KeyBinding::new("shift-down", HexSelDown, c),
        KeyBinding::new("shift-pageup", HexSelPageUp, c),
        KeyBinding::new("shift-pagedown", HexSelPageDown, c),
        KeyBinding::new("shift-home", HexSelHome, c),
        KeyBinding::new("shift-end", HexSelEnd, c),
        KeyBinding::new("ctrl-f", HexFind, c),
        KeyBinding::new("f3", HexFindNext, c),
        KeyBinding::new("shift-f3", HexFindPrev, c),
        KeyBinding::new("ctrl-c", HexCopy, c),
        KeyBinding::new("ctrl-o", HexOpenLeft, c),
        KeyBinding::new("ctrl-shift-o", HexOpenRight, c),
    ]
}

pub enum HexEvent {
    /// Escape: back to the folder tab this one was opened from.
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

/// What a load starts from.
enum Load {
    Paths(PathBuf, PathBuf),
    /// Sources that are open already (tests): their rows show at once.
    Opened(Arc<HexPair>),
}

/// Opens both files. An open error names the side that failed.
fn open(left: &Path, right: &Path) -> Result<HexPair, String> {
    let open = |path: &Path| {
        HexSource::open(path).map_err(|err| format!("Can't open {}: {err}.", path.display()))
    };
    Ok(HexPair {
        left: open(left)?,
        right: open(right)?,
    })
}

/// The ranges and the navigation blocks they merge into (R6); None when cancelled.
fn compare(
    pair: &HexPair,
    cancel: &CancelToken,
    done: &AtomicU64,
) -> Option<(HexDiff, Vec<Range<u64>>)> {
    let diff = diff_ranges(pair, cancel, done);
    if diff.cancelled {
        return None;
    }
    let blocks = blocks(&diff.ranges);
    Some((diff, blocks))
}

pub struct HexView {
    left: PathBuf,
    right: PathBuf,
    pair: Option<Arc<HexPair>>,
    diff: Option<HexDiff>,
    /// Navigation stops: the ranges with close ones merged (R6).
    blocks: Vec<Range<u64>>,
    message: Option<SharedString>,
    message_hint: Option<SharedString>,
    job: JobSlot,
    task: Option<Task<()>>,
    /// Redraws the status line's percentage while comparing.
    ticker: Option<Task<()>>,
    /// Bytes compared so far, for the percentage.
    progress: Arc<AtomicU64>,
    /// Esc stopped the compare: what is shown is incomplete.
    cancelled: bool,
    /// The byte the navigation keys and "Go to" work from: (left side?, offset).
    cursor: Option<(bool, u64)>,
    /// The selected bytes: (left side?, range).
    selection: Option<(bool, Range<u64>)>,
    /// The fixed and the moving end of the selection; only valid while it spans them.
    sel_ends: Option<(u64, u64)>,
    /// The find bar, while it is open.
    find: Option<find::FindBar>,
    /// The running find, if any: Esc stops it before anything else.
    find_job: JobSlot,
    find_task: Option<Task<()>>,
    /// Redraws the status line's percentage while a find runs.
    find_ticker: Option<Task<()>>,
    /// Bytes the running find has looked at, and how many it may look at, for the percentage.
    find_progress: Arc<AtomicU64>,
    find_total: u64,
    /// The column layout of the last frame, for hit-testing clicks.
    cols: Option<rows::Columns>,
    /// The block the last jump landed on.
    current_block: Option<usize>,
    /// A one-line note that replaces the status line until the next action.
    notice: Option<SharedString>,
    /// F9: the cursor and selection (already on their new side) that wait for the reopened
    /// files, to be clamped to the new lengths. Any other reopen drops them.
    carry: Option<Carry>,
    /// The path box being typed into, if any.
    path_edit: Option<ActiveEdit>,
    /// The byte the last right press landed on; the menu's "Copy offset" is about it.
    menu_byte: Option<(bool, u64)>,
    /// A go-to made before the ranges were ready: finished when they arrive.
    pending_goto: Option<u64>,
    goto: Entity<InputState>,
    _goto_subscription: Subscription,
    focus: FocusHandle,
    /// The display row at the top of the body (R4).
    top: u64,
    bytes_per_row: u64,
    /// "Differences only": the display rows are the rows that touch a range (R7).
    only_diff: bool,
    /// The display rows of "differences only", built while it is on and the ranges are known.
    diff_rows: Option<DiffRows>,
    /// The byte to bring back to the top once "differences only" has the ranges to map it.
    pending_top: Option<u64>,
    /// Touchpad scrolling not yet a whole row, in px.
    wheel_rest: f32,
    /// How far the rows are scrolled right, in px; the ruler moves with them.
    skip_x: f32,
    /// The width of a row and of the ruler, from the last frame's code advance.
    content_width: f32,
    /// The rows area, the scrollbars and the overview strip as the last frame drew them.
    body_bounds: Rc<Cell<Bounds<Pixels>>>,
    vscroll_bounds: Rc<Cell<Bounds<Pixels>>>,
    hscroll_bounds: Rc<Cell<Bounds<Pixels>>>,
    overview_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// The left button went down on the vertical scrollbar and is still held.
    dragging_vscroll: bool,
    /// Where the held thumb was grabbed: px from its top edge.
    vscroll_grab: f32,
    /// The left button went down on the horizontal scrollbar and is still held.
    dragging_hscroll: bool,
    /// Where the held horizontal thumb was grabbed: px from its left edge.
    hscroll_grab: f32,
    /// The left button went down on the overview strip and is still held.
    dragging_overview: bool,
    /// The strip's block marks for one strip height and row mapping.
    overview_marks: Option<rows::StripMarks>,
    /// When the last load started, for the "first rows" log line.
    load_started: Instant,
    first_rows_logged: bool,
    probe: Option<ScrollProbe>,
}

impl EventEmitter<HexEvent> for HexView {}

impl Focusable for HexView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl HexView {
    pub fn new(left: PathBuf, right: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::blank(left, right, window, cx);
        view.reload(cx);
        view
    }

    /// A view over sources that are open already, such as synthetic ones of any length.
    #[doc(hidden)]
    pub fn with_sources_for_test(
        left: HexSource,
        right: HexSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::blank(
            left.path().to_path_buf(),
            right.path().to_path_buf(),
            window,
            cx,
        );
        view.start(Load::Opened(Arc::new(HexPair { left, right })), cx);
        view
    }

    fn blank(left: PathBuf, right: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let bytes_per_row = settings::settings_file(cx)
            .and_then(|path| settings::get_u64(&path, BYTES_PER_ROW_KEY))
            .filter(|bpr| BYTES_PER_ROW.contains(bpr))
            .unwrap_or(DEFAULT_BYTES_PER_ROW);
        let goto = cx.new(|cx| InputState::new(window, cx).placeholder("Offset, e.g. 0x1F40"));
        let _goto_subscription = cx.subscribe_in(
            &goto,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_goto(window, cx);
                }
            },
        );
        Self {
            left,
            right,
            pair: None,
            diff: None,
            blocks: Vec::new(),
            message: None,
            message_hint: None,
            job: JobSlot::default(),
            task: None,
            ticker: None,
            progress: Arc::default(),
            cancelled: false,
            cursor: None,
            selection: None,
            sel_ends: None,
            find: None,
            find_job: JobSlot::default(),
            find_task: None,
            find_ticker: None,
            find_progress: Arc::default(),
            find_total: 0,
            cols: None,
            current_block: None,
            notice: None,
            carry: None,
            path_edit: None,
            menu_byte: None,
            pending_goto: None,
            goto,
            _goto_subscription,
            // A tab stop, so Tab comes back to the view after a button or field.
            focus: cx.focus_handle().tab_stop(true),
            top: 0,
            bytes_per_row,
            only_diff: false,
            diff_rows: None,
            pending_top: None,
            wheel_rest: 0.,
            skip_x: 0.,
            content_width: 0.,
            body_bounds: Rc::default(),
            vscroll_bounds: Rc::default(),
            hscroll_bounds: Rc::default(),
            overview_bounds: Rc::default(),
            dragging_vscroll: false,
            vscroll_grab: 0.,
            dragging_hscroll: false,
            hscroll_grab: 0.,
            dragging_overview: false,
            overview_marks: None,
            load_started: Instant::now(),
            first_rows_logged: false,
            probe: ScrollProbe::from_env(),
        }
    }

    /// Opening or diffing.
    pub fn is_busy(&self) -> bool {
        self.job.is_running()
    }

    pub fn diff(&self) -> Option<&HexDiff> {
        self.diff.as_ref()
    }

    pub fn pair(&self) -> Option<&Arc<HexPair>> {
        self.pair.as_ref()
    }

    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn message_hint(&self) -> Option<&str> {
        self.message_hint.as_deref()
    }

    pub fn paths(&self) -> (&Path, &Path) {
        (&self.left, &self.right)
    }

    /// Shows other files in this tab (the workspace re-keys the tab first).
    pub fn load_paths(&mut self, left: PathBuf, right: PathBuf, cx: &mut Context<Self>) {
        self.left = left;
        self.right = right;
        self.top = 0;
        self.skip_x = 0.;
        self.pending_top = self.only_diff.then_some(0);
        self.reopen(cx);
    }

    /// Opens both files again and rebuilds the ranges; the top byte stays at the top.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        // "Differences only" can map that byte to its row only once the new ranges are in.
        if self.only_diff {
            self.pending_top = Some(self.top_byte());
        }
        self.reopen(cx);
    }

    /// Drops the sources and loads them again. The old ones (and their cached blocks) go first,
    /// so a changed file never shows stale bytes.
    fn reopen(&mut self, cx: &mut Context<Self>) {
        self.path_edit = None;
        self.menu_byte = None;
        self.pair = None;
        self.diff = None;
        self.blocks = Vec::new();
        self.message = None;
        self.message_hint = None;
        self.notice = None;
        self.pending_goto = None;
        // The new files may be shorter: no byte of the old ones is still under the cursor.
        self.cursor = None;
        self.selection = None;
        self.carry = None;
        self.current_block = None;
        self.cancel_find();
        self.cancelled = false;
        self.mapping_changed();
        self.start(Load::Paths(self.left.clone(), self.right.clone()), cx);
    }

    /// Starts a load: opens the files unless they are open already, shows their rows, then
    /// builds the ranges, all off the UI thread.
    fn start(&mut self, load: Load, cx: &mut Context<Self>) {
        let (generation, cancel) = self.job.start();
        self.cancelled = false;
        let progress: Arc<AtomicU64> = Arc::default();
        self.progress = progress.clone();
        self.ticker = Some(Self::ticker(Self::is_busy, cx));
        self.load_started = Instant::now();
        self.first_rows_logged = false;
        if let Load::Opened(pair) = &load {
            self.pair = Some(pair.clone());
        }
        let background = cx.background_executor().clone();
        self.task = Some(cx.spawn(async move |this, cx| {
            let pair = match load {
                Load::Opened(pair) => pair,
                Load::Paths(left, right) => {
                    let opened = background
                        .spawn(async move { open(&left, &right).map(Arc::new) })
                        .await;
                    match this.update(cx, |view, cx| view.opened(generation, opened, cx)) {
                        Ok(Some(pair)) => pair,
                        _ => return,
                    }
                }
            };
            let outcome = background
                .spawn(async move { compare(&pair, &cancel, &progress) })
                .await;
            let _ = this.update(cx, |view, cx| view.apply(generation, outcome, cx));
        }));
        cx.notify();
    }

    /// Redraws the status line every `PROGRESS_TICK` while `running` (the compare or a find)
    /// goes on, so its percentage moves.
    fn ticker(running: fn(&Self) -> bool, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PROGRESS_TICK).await;
                let going = this.update(cx, |view, cx| {
                    let going = running(view);
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

    /// The files of load `generation` are open (or failed to open). Returns the pair to compare
    /// while that load is still the current one.
    fn opened(
        &mut self,
        generation: u64,
        opened: Result<Arc<HexPair>, String>,
        cx: &mut Context<Self>,
    ) -> Option<Arc<HexPair>> {
        if !self.job.is_current(generation) {
            return None;
        }
        cx.notify();
        match opened {
            Ok(pair) => {
                self.pair = Some(pair.clone());
                self.restore_carry(&pair);
                Some(pair)
            }
            Err(message) => {
                self.carry = None;
                self.job.finish(generation);
                self.message = Some(message.into());
                self.message_hint = Some(OPEN_HINT.into());
                None
            }
        }
    }

    fn apply(
        &mut self,
        generation: u64,
        outcome: Option<(HexDiff, Vec<Range<u64>>)>,
        cx: &mut Context<Self>,
    ) {
        if !self.job.is_current(generation) {
            return;
        }
        self.job.finish(generation);
        // None: cancelled, and a newer job replaced this one, so `is_current` turned it away.
        if let Some((diff, blocks)) = outcome {
            self.diff = Some(diff);
            self.blocks = blocks;
            self.current_block = self.cursor.and_then(|(_, at)| self.block_at(at));
            self.mapping_changed();
            // The byte remembered while "differences only" waited goes back to the top; the
            // next frame clamps it to the last page.
            if self.diff_rows.is_some()
                && let Some(byte) = self.pending_top.take()
            {
                self.top = self
                    .display_row(byte / self.bytes_per_row)
                    .unwrap_or_else(|| self.max_top());
            }
            if let Some(offset) = self.pending_goto.take() {
                // A find made while the ranges loaded selected its match: keep that.
                let selection = self.selection.clone();
                self.move_cursor(offset, cx);
                self.selection = selection;
            }
        }
        cx.notify();
    }

    /// "Differences only" waits for the ranges to know its rows.
    fn awaiting_ranges(&self) -> bool {
        self.only_diff && self.diff_rows.is_none() && self.is_busy()
    }

    /// "Differences only" has no rows because the compare was cancelled before the ranges.
    fn ranges_cancelled(&self) -> bool {
        self.only_diff && self.diff_rows.is_none() && self.cancelled
    }

    /// The display rows or the ranges changed: rebuild what is derived from them.
    fn mapping_changed(&mut self) {
        self.diff_rows = match (&self.diff, self.only_diff) {
            (Some(diff), true) => Some(DiffRows::new(&diff.ranges, self.bytes_per_row)),
            _ => None,
        };
        self.overview_marks = None;
    }

    pub fn top_row(&self) -> u64 {
        self.top
    }

    /// Display rows: every row of the longer file, or the rows that differ (R7).
    pub fn row_count(&self) -> u64 {
        if self.only_diff {
            self.diff_rows.as_ref().map_or(0, DiffRows::len)
        } else {
            self.pair
                .as_ref()
                .map_or(0, |pair| pair.max_len().div_ceil(self.bytes_per_row))
        }
    }

    /// Whole rows that fit the body, as the last frame measured it; at least one.
    pub fn visible_rows(&self) -> u64 {
        let height = f32::from(self.body_bounds.get().size.height);
        ((height / ROW_HEIGHT).floor() as u64).max(1)
    }

    /// The top row of the last page, which ends on the last row.
    fn max_top(&self) -> u64 {
        self.row_count().saturating_sub(self.visible_rows())
    }

    /// Puts display row `row` at the top, clamped so the last page stays full.
    pub fn scroll_to_row(&mut self, row: u64, cx: &mut Context<Self>) {
        let row = row.min(self.max_top());
        if row != self.top {
            self.top = row;
            self.notice = None;
            cx.notify();
        }
    }

    fn scroll_by(&mut self, rows: i64, cx: &mut Context<Self>) {
        self.notice = None;
        self.scroll_to_row(self.top.saturating_add_signed(rows), cx);
    }

    pub fn bytes_per_row(&self) -> u64 {
        self.bytes_per_row
    }

    /// Shows 8, 16 or 32 bytes per row (other widths are ignored), keeps the top byte in view
    /// and remembers the choice (R12).
    pub fn set_bytes_per_row(&mut self, bpr: u64, cx: &mut Context<Self>) {
        if bpr == self.bytes_per_row || !BYTES_PER_ROW.contains(&bpr) {
            return;
        }
        self.notice = None;
        self.remap(|view| view.bytes_per_row = bpr, cx);
        if let Some(path) = settings::settings_file(cx)
            && let Err(err) = settings::set_u64(&path, BYTES_PER_ROW_KEY, bpr)
        {
            tracing::warn!(%err, "could not remember the bytes per row");
        }
    }

    /// Shows only the rows that differ, or every row again; the top byte stays in view.
    pub fn set_only_differences(&mut self, on: bool, cx: &mut Context<Self>) {
        if on != self.only_diff {
            self.notice = None;
            self.pending_goto = None;
            self.remap(|view| view.only_diff = on, cx);
        }
    }

    /// Changes how display rows map to bytes, keeping the byte at the top in view.
    fn remap(&mut self, change: impl FnOnce(&mut Self), cx: &mut Context<Self>) {
        let top_byte = self.top_byte();
        change(self);
        self.mapping_changed();
        if self.only_diff && self.diff_rows.is_none() {
            // The ranges are still loading: `apply` brings the byte back.
            self.pending_top = Some(top_byte);
        } else {
            self.pending_top = None;
            let row = self
                .display_row(top_byte / self.bytes_per_row)
                .unwrap_or_else(|| self.max_top());
            // Without the files (a reload) the rows are not known yet; the next frame clamps.
            self.top = if self.pair.is_some() {
                row.min(self.max_top())
            } else {
                row
            };
        }
        cx.notify();
    }

    /// The byte at the top: the one remembered while the ranges load, else the top row's first.
    fn top_byte(&self) -> u64 {
        if let Some(byte) = self.pending_top {
            return byte;
        }
        let row = if self.only_diff {
            self.diff_rows
                .as_ref()
                .and_then(|rows| rows.row(self.top))
                .unwrap_or(0)
        } else {
            self.top
        };
        row.saturating_mul(self.bytes_per_row)
    }

    /// The file row shown at display row `display`.
    fn file_row(&self, display: u64) -> Option<u64> {
        if self.only_diff {
            self.diff_rows.as_ref()?.row(display)
        } else {
            (display < self.row_count()).then_some(display)
        }
    }

    /// The display row of file row `row`, or of the next row shown. None past the last row
    /// shown, and while "differences only" has no rows yet: never scroll to such a row.
    fn display_row(&self, row: u64) -> Option<u64> {
        if self.only_diff {
            self.diff_rows.as_ref()?.display_of(row)
        } else {
            Some(row)
        }
    }

    /// Whether file row `row` is one of the display rows.
    fn row_shown(&self, row: u64) -> bool {
        match (&self.diff_rows, self.only_diff) {
            (_, false) => true,
            (Some(rows), true) => rows
                .display_of(row)
                .is_some_and(|display| rows.row(display) == Some(row)),
            (None, true) => false,
        }
    }

    /// Test hook: how each byte of display row `display` is marked on each side.
    #[doc(hidden)]
    pub fn row_marks_for_test(&self, display: u64) -> RowMarks {
        let (Some(pair), Some(row)) = (&self.pair, self.file_row(display)) else {
            return RowMarks::default();
        };
        let span = rows::row_span(row, self.bytes_per_row, pair.max_len());
        let width = (span.end - span.start) as usize;
        rows::row_marks(
            &pair.left.bytes(span.clone()),
            &pair.right.bytes(span),
            width,
        )
    }

    /// A wheel notch moves at least one row; a touchpad's small steps add up to rows. A
    /// sideways delta, or Shift with the wheel, scrolls the rows right or left by px.
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
            self.scroll_x_to(self.skip_x - dx, cx);
        }
        if dy == 0. {
            return;
        }
        let rows = match event.delta {
            ScrollDelta::Lines(_) => {
                let rows = (-dy / ROW_HEIGHT).round();
                if rows == 0. && dy != 0. {
                    -dy.signum()
                } else {
                    rows
                }
            }
            ScrollDelta::Pixels(_) => {
                self.wheel_rest -= dy;
                let rows = (self.wheel_rest / ROW_HEIGHT).trunc();
                self.wheel_rest -= rows * ROW_HEIGHT;
                rows
            }
        };
        self.scroll_by(rows as i64, cx);
    }

    /// The most the rows can scroll right: what the body is too narrow to show.
    fn max_skip_x(&self) -> f32 {
        (self.content_width - f32::from(self.body_bounds.get().size.width)).max(0.)
    }

    /// Scrolls the rows and the ruler so `x` px of a row are off the left edge.
    fn scroll_x_to(&mut self, x: f32, cx: &mut Context<Self>) {
        let x = x.clamp(0., self.max_skip_x());
        if x != self.skip_x {
            self.skip_x = x;
            cx.notify();
        }
    }

    /// `DUET_AUTOSCROLL_SECS`: about 1,000 rows further each frame, then the stats and quit.
    fn autoscroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pair.is_none() {
            return;
        }
        let Some(probe) = self.probe.as_mut() else {
            return;
        };
        let done = probe.frame();
        self.top = (self.top + 997) % self.row_count().max(1);
        window.request_animation_frame();
        if done {
            cx.quit();
        }
    }

    /// Esc stops a running find, then gives the keys back from the go-to field, then closes the
    /// find bar, then cancels a running compare, then asks to go back to the folder tab.
    fn on_escape(&mut self, _: &HexEscape, window: &mut Window, cx: &mut Context<Self>) {
        if self.path_edit.is_some() {
            self.cancel_path_edit(window, cx);
        } else if self.find_job.is_running() {
            self.cancel_find();
            cx.notify();
        } else if self.goto_focused(window, cx) {
            window.focus(&self.focus, cx);
            cx.notify();
        } else if self.find.is_some() {
            self.close_find(window, cx);
        } else if self.job.is_running() {
            self.cancel_compare(cx);
        } else {
            cx.emit(HexEvent::Back);
        }
    }

    /// Stops the compare. What was shown stays, and the status says it is incomplete.
    fn cancel_compare(&mut self, cx: &mut Context<Self>) {
        self.job.cancel();
        self.task = None;
        self.pending_goto = None;
        self.cancelled = true;
        if self.notice.as_deref() == Some(STILL_COMPARING) {
            self.notice = Some(CANCELLED.into());
        }
        if self.pair.is_none() {
            self.message = Some("Cancelled: the files were not opened.".into());
            self.message_hint = Some(OPEN_HINT.into());
        }
        cx.notify();
    }

    fn on_reload(&mut self, _: &HexReload, _: &mut Window, cx: &mut Context<Self>) {
        self.reload(cx);
    }

    /// F9: the sides change places, in this tab and in its key.
    pub fn swap_sides(&mut self, cx: &mut Context<Self>) {
        std::mem::swap(&mut self.left, &mut self.right);
        let cursor = self.cursor.map(|(left, at)| (!left, at));
        let selection = self.selection.take().map(|(left, r)| (!left, r));
        cx.emit(HexEvent::Swapped {
            left: self.left.clone(),
            right: self.right.clone(),
        });
        self.reload(cx);
        self.carry = Some((cursor, selection));
    }

    /// Puts the cursor and selection kept by F9 back, cut to the length of the side they sit on.
    fn restore_carry(&mut self, pair: &HexPair) {
        let Some((cursor, selection)) = self.carry.take() else {
            return;
        };
        let len = |left: bool| {
            if left {
                pair.left.len()
            } else {
                pair.right.len()
            }
        };
        self.cursor = cursor.filter(|&(left, at)| at < len(left));
        self.selection = selection.and_then(|(left, r)| {
            let end = r.end.min(len(left));
            (r.start < end).then_some((left, r.start..end))
        });
    }

    /// Whether the go-to field has the keys.
    fn goto_focused(&self, window: &Window, cx: &App) -> bool {
        self.goto.focus_handle(cx).is_focused(window)
    }

    /// Keys that mean something in a field (arrows, Home, End, Ctrl+C, digits) stay with it:
    /// the hex actions run only when no field has the keys: not the go-to field, the find
    /// field or a path box.
    fn takes_key(&self, window: &Window, cx: &mut Context<Self>) -> bool {
        let free = !self.goto_focused(window, cx)
            && !self.find_focused(window, cx)
            && !self.path_focused(window, cx);
        if !free {
            cx.propagate();
        }
        free
    }

    pub fn cursor(&self) -> Option<(bool, u64)> {
        self.cursor
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn selection(&self) -> Option<(bool, Range<u64>)> {
        self.selection.clone()
    }

    /// Whether the navigation keys and buttons have blocks to walk to.
    pub fn can_navigate(&self) -> bool {
        !self.blocks.is_empty()
    }

    /// "3 / 6" at a block, "– / 6" before the first jump, "" before the compare is done.
    pub fn position_text(&self) -> String {
        if self.diff.is_none() {
            return String::new();
        }
        let at = self
            .current_block
            .map_or("–".to_owned(), |b| (b + 1).to_string());
        format!("{at} / {}", self.blocks.len())
    }

    /// The block that holds `offset`.
    fn block_at(&self, offset: u64) -> Option<usize> {
        let after = self.blocks.partition_point(|b| b.start <= offset);
        after
            .checked_sub(1)
            .filter(|&i| self.blocks[i].end > offset)
    }

    /// Puts the cursor on `offset` of its side and its row a third of the way down the page.
    fn move_cursor(&mut self, offset: u64, cx: &mut Context<Self>) {
        let side = self.cursor.is_none_or(|c| c.0);
        self.place_cursor(side, offset, cx);
    }

    /// Puts the cursor on `offset` of the given side and its row a third of the way down the
    /// page. A notice set before the call stays; scrolling would clear it, so it is put back.
    fn place_cursor(&mut self, side: bool, offset: u64, cx: &mut Context<Self>) {
        self.cursor = Some((side, offset));
        self.selection = None;
        self.current_block = self.block_at(offset);
        let file_row = offset / self.bytes_per_row;
        if self.only_diff && self.diff_rows.is_none() {
            // The ranges are still loading: `apply` lands the top row, then finishes this.
            self.pending_top = Some(offset);
            self.pending_goto = Some(offset);
            cx.notify();
            return;
        }
        let mut notice = self.notice.take();
        if !self.row_shown(file_row) {
            self.set_only_differences(false, cx);
            notice = Some(match notice {
                Some(earlier) => format!("{earlier} {SHOWING_ALL}").into(),
                None => SHOWING_ALL.into(),
            });
        }
        let row = self.display_row(file_row).unwrap_or(0);
        self.scroll_to_row(row.saturating_sub(self.visible_rows() / 3), cx);
        self.notice = notice;
        cx.notify();
    }

    fn navigate(&mut self, to: Nav, cx: &mut Context<Self>) {
        self.notice = None;
        if !self.can_navigate() {
            // Nothing to walk to: the compare is running or was cancelled, the files did not
            // open, or they have no differences.
            if self.is_busy() {
                self.notice = Some(STILL_COMPARING.into());
            } else if self.cancelled {
                self.notice = Some(CANCELLED.into());
            } else if self.diff.is_some() {
                self.notice = Some(NO_MORE.into());
            }
            cx.notify();
            return;
        }
        // From the cursor, else from the top row.
        let from = self.cursor.map_or_else(|| self.top_byte(), |c| c.1);
        let index = match to {
            Nav::First => (!self.blocks.is_empty()).then_some(0),
            Nav::Last => self.blocks.len().checked_sub(1),
            Nav::Next => {
                let strict = self.cursor.is_some();
                let after = self.blocks.partition_point(|b| {
                    if strict {
                        b.start <= from
                    } else {
                        b.start < from
                    }
                });
                (after < self.blocks.len()).then_some(after)
            }
            Nav::Prev => self
                .blocks
                .partition_point(|b| b.start < from)
                .checked_sub(1),
        };
        match index {
            Some(i) => {
                let start = self.blocks[i].start;
                self.move_cursor(start, cx);
                self.current_block = Some(i);
            }
            None => self.notice = Some(NO_MORE.into()),
        }
        cx.notify();
    }

    /// Enter in the go-to field, or the Go to button.
    fn submit_goto(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.goto.read(cx).value().to_string();
        if self.go_to(&text, cx) {
            window.focus(&self.focus, cx);
        }
    }

    /// Moves the cursor to `text`, an offset; says why when it can't.
    pub fn go_to(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(max_len) = self.pair.as_ref().map(|pair| pair.max_len()) else {
            return false;
        };
        let Some(offset) = parse_offset(text) else {
            self.notice = Some(BAD_OFFSET.into());
            cx.notify();
            return false;
        };
        if max_len == 0 {
            self.notice = Some("There are no bytes to go to.".into());
            cx.notify();
            return false;
        }
        let clamped = offset.min(max_len - 1);
        self.notice = (clamped != offset).then(|| PAST_END.into());
        self.move_cursor(clamped, cx);
        true
    }
}

impl Render for HexView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.autoscroll(window, cx);
        let palette = Palette::current(cx);
        let fonts = Fonts::current(cx);
        let body = match (self.message.clone(), self.pair.clone()) {
            (Some(title), _) => v_flex()
                .id("hex-message")
                .test_support()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .p_4()
                .text_center()
                .child(
                    div()
                        .id("hex-message-title")
                        .test_support()
                        .text_size(px(15.))
                        .child(title),
                )
                .children(self.message_hint.clone().map(|hint| {
                    div()
                        .id("hex-message-hint")
                        .test_support()
                        .text_color(palette.muted)
                        .child(hint)
                }))
                .into_any_element(),
            (None, Some(pair)) if !self.awaiting_ranges() && !self.ranges_cancelled() => self
                .render_body(&pair, &palette, window, cx)
                .into_any_element(),
            // Opening, or "differences only" before the ranges (or cancelled before them): no
            // rows to show, and the top row is not clamped, so the top byte comes back.
            (None, _) => {
                let text = if self.ranges_cancelled() {
                    CANCELLED
                } else {
                    "Comparing…"
                };
                v_flex()
                    .id("hex-placeholder")
                    .test_support()
                    .aria_label(text)
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_color(palette.muted)
                    .child(text)
                    .into_any_element()
            }
        };
        v_flex()
            .id("hex-view")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .bg(palette.bg)
            .text_color(palette.text)
            .text_size(px(UI_SIZE))
            .on_action(cx.listener(Self::on_escape))
            .on_action(cx.listener(Self::on_reload))
            .on_action(cx.listener(|this, _: &HexSwap, _, cx| this.swap_sides(cx)))
            .on_action(cx.listener(|this, _: &HexGoTo, window, cx| {
                window.focus(&this.goto.focus_handle(cx), cx);
            }))
            .on_action(cx.listener(|this, _: &HexNext, w, cx| {
                if this.takes_key(w, cx) {
                    this.navigate(Nav::Next, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &HexPrev, w, cx| {
                if this.takes_key(w, cx) {
                    this.navigate(Nav::Prev, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &HexFirst, w, cx| {
                if this.takes_key(w, cx) {
                    this.navigate(Nav::First, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &HexLast, w, cx| {
                if this.takes_key(w, cx) {
                    this.navigate(Nav::Last, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &HexShowAll, w, cx| {
                if this.takes_key(w, cx) {
                    this.set_only_differences(false, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &HexOnlyDiff, w, cx| {
                if this.takes_key(w, cx) {
                    this.set_only_differences(true, cx)
                }
            }))
            .on_action(step_key!(cx, HexLeft, Step::Left, false))
            .on_action(step_key!(cx, HexRight, Step::Right, false))
            .on_action(step_key!(cx, HexRowUp, Step::Up, false))
            .on_action(step_key!(cx, HexRowDown, Step::Down, false))
            .on_action(step_key!(cx, HexPageUp, Step::PageUp, false))
            .on_action(step_key!(cx, HexPageDown, Step::PageDown, false))
            .on_action(step_key!(cx, HexTop, Step::Home, false))
            .on_action(step_key!(cx, HexBottom, Step::End, false))
            .on_action(step_key!(cx, HexSelLeft, Step::Left, true))
            .on_action(step_key!(cx, HexSelRight, Step::Right, true))
            .on_action(step_key!(cx, HexSelUp, Step::Up, true))
            .on_action(step_key!(cx, HexSelDown, Step::Down, true))
            .on_action(step_key!(cx, HexSelPageUp, Step::PageUp, true))
            .on_action(step_key!(cx, HexSelPageDown, Step::PageDown, true))
            .on_action(step_key!(cx, HexSelHome, Step::Home, true))
            .on_action(step_key!(cx, HexSelEnd, Step::End, true))
            .on_action(cx.listener(|this, _: &HexFind, window, cx| this.open_find(window, cx)))
            .on_action(
                cx.listener(|this, _: &HexFindNext, window, cx| this.find_step(true, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &HexFindPrev, window, cx| this.find_step(false, window, cx)),
            )
            .on_action(cx.listener(|this, _: &HexCopyOffset, w, cx| {
                if this.takes_key(w, cx) {
                    this.copy_offset(cx)
                }
            }))
            .on_action(
                cx.listener(|this, _: &HexOpenLeft, window, cx| this.pick_file(true, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &HexOpenRight, window, cx| this.pick_file(false, window, cx)),
            )
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.on_files_dropped(paths, window, cx)
            }))
            .on_action(cx.listener(|this, _: &HexCopy, w, cx| {
                if this.takes_key(w, cx) {
                    this.copy_hex(cx)
                }
            }))
            .child(self.render_toolbar(&palette, &fonts, cx))
            .children(self.render_find(&palette, cx))
            .child(self.render_files(&palette, cx))
            .child(body)
            .child(self.render_status(&palette))
    }
}
