//! Text compare tab: aligned side-by-side rows in one virtualized list (read-only in Plan 2).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Weak};
use std::time::Duration;

use duet_core::kind::CompareKind;
use duet_core::text::diff::{DiffOptions, WhitespaceMode};
use duet_core::text::document::{ReadOnlyReason, TextDocument};
use duet_core::text::lexer::{Language, detect_language};
use duet_core::text::rows::RowKind;
use encoding_rs::Encoding;
use gpui_component::highlighter::HighlightTheme;
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Bounds, ClickEvent, Context, Div, Entity, EventEmitter, ExternalPaths,
    FocusHandle, Focusable, HighlightStyle, KeyBinding, Pixels, ScrollStrategy, SharedString,
    Subscription, Task, Window, actions, div, prelude::*, px, uniform_list,
};

use crate::format::{count_of, format_size};
use crate::job::JobSlot;
use crate::palette::Palette;
use crate::probe::ScrollProbe;
use crate::settings;
use crate::text::align::AlignStart;
use crate::text::caret::{Caret, Motion};
use crate::text::display::{DisplayOptions, columns};
use crate::text::editing::{RediffTimes, WAIT_NOTICE, carried_caret};
use crate::text::find::{FindQuery, FindResult, FindScope, find, find_ticks};
use crate::text::goto::parse_line;
use crate::text::job::{
    ENCODINGS, IgnoreChoice, TextOutcome, TextRequest, apply_ignore, encoding_text, next_encoding,
    run,
};
use crate::text::menu::{
    AlignWithOtherSide, ClearAlignments, TreatAsUnimportant, path_actions, row_menu,
};
use crate::text::model::{
    DisplayRow, Landing, TextFilter, TextModel, doc_info, status_parts, status_text,
};
use crate::text::moving::MoveDrag;
use crate::text::overview::{display_at, marks};
use crate::text::prefs::TextPrefs;
use crate::text::report::{CopyPatch, ReportHtml, ReportPatch, ReportText};
use crate::text::rowsel::{RowDrag, RowSel};
use crate::text::sides::SideEdit;
use crate::text::syntax::{
    EDIT_BUDGET, MAX_SYNTAX_BYTES, SyntaxChoice, SyntaxSide, SyntaxState, register_extra_languages,
    syntax_choice,
};
use crate::text::undo::UndoStack;
use crate::text::watch::Watch;
use crate::ui::drag::follow_drag;
use crate::ui::fonts::{Fonts, UI_SIZE, code_advance_at};
use crate::ui::menu::fill as fill_menu;
use crate::ui::style::{
    LEGEND_MIN_WIDTH, LdExt as _, SCROLLBAR_WIDTH, ld_button, ld_gutter_button, legend, status_bar,
    status_parts_row,
};
use gpui_kit::{
    BorderStyle, ElementInputHandler, MouseButton, MouseDownEvent, MouseMoveEvent, Point, canvas,
    fill, point, quad, size, transparent_black,
};
use rows::{HalfText, RowFrame, half};
use stage::{
    CENTRE_GUTTER, OVERVIEW_WIDTH, hscroll_skip, hscroll_thumb, stage_columns, view_columns,
};

actions!(
    text,
    [
        NextChange,
        PrevChange,
        NextChangeKey,
        PrevChangeKey,
        FirstChange,
        LastChange,
        FilterAll,
        FilterDifferences,
        FilterContext,
        FilterSame,
        ToggleWhitespace,
        ScrollLeft,
        ScrollRight,
        ScrollHome,
        Reload,
        Escape,
        RowUp,
        RowDown,
        PageUp,
        PageDown,
        OpenFind,
        FindNext,
        FindPrev,
        OpenGoTo,
        SwapSides,
        LineEnd,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectHome,
        SelectEnd,
        SelectPageUp,
        SelectPageDown,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        Backspace,
        DeleteForward,
        Newline,
        InsertTab,
        Undo,
        Redo,
        CopyText,
        CutText,
        PasteText,
        SelectAll,
        Save,
        CopyBlockRight,
        CopyBlockLeft,
        CopyAllRight,
        CopyAllLeft,
        CopyBlockHereRight,
        CopyBlockHereLeft,
        EditRow,
        DeleteRows,
        OpenLeftFile,
        OpenRightFile,
        PasteSideLeft,
        PasteSideRight,
        SaveLeft,
        SaveRight,
        SaveLeftAs,
        SaveRightAs,
        SaveAs,
        CopyLeftPath,
        CopyRightPath,
        RevealLeftFile,
        RevealRightFile,
        OpenLeftExternally,
        OpenRightExternally
    ]
);

mod bars;
mod body;
mod compare;
mod details;
mod list;
mod nav;
mod odd;
mod prefs;
pub mod rows;
pub mod rules_bar;
mod scrollbars;
pub mod stage;
mod status;
mod syntax_jobs;
mod toolbar;
mod untitled;
mod wrap;

use bars::{FindBar, GoToBar};
pub use details::DetailsText;
use list::RowList;
pub use toolbar::position_text;

pub const CONTEXT: &str = "TextView";
/// The find field's tooltip: what Enter and Shift+Enter do there.
pub const FIND_TOOLTIP: &str = "Find text (Enter: next match, Shift+Enter: previous match)";
/// The code font's sizes the Font dropdown offers (R4); a saved size is clamped to their range.
pub const FONT_SIZES: [f32; 13] = [
    9., 10., 11., 12., 12.5, 13., 14., 15., 16., 18., 20., 22., 24.,
];
pub const DEFAULT_FONT_SIZE: f32 = 12.5;
/// The row height at the default font size.
pub const DEFAULT_ROW_HEIGHT: f32 = 20.0;
/// The most aligned rows a pair may have to wrap (R2): Task 2's spike measured gpui's `list`
/// to here.
pub const WRAP_MAX_ROWS: u32 = 200_000;

/// A row's height at code font size `font_size`: 1.6 lines, to the nearest pixel (LocalDiff's
/// `TV.rowH`).
pub fn row_height_for(font_size: f32) -> f32 {
    (font_size * 1.6).round()
}
/// R9: the current difference follows the scroll to the row this share of a screen below the
/// top (LocalDiff's 30% line).
pub const FOLLOW_LINE: f32 = 0.3;
/// Frames after a scroll duet made itself in which the current difference does not follow.
const FOLLOW_PAUSE: u8 = 2;
const SCROLL_STEP: usize = 8;
const FOLD_STEP: u32 = 20;

pub fn key_bindings() -> Vec<KeyBinding> {
    let c = Some(CONTEXT);
    vec![
        KeyBinding::new("f8", NextChange, c),
        KeyBinding::new("alt-down", NextChange, c),
        KeyBinding::new("ctrl-n", NextChange, c),
        KeyBinding::new("f7", PrevChange, c),
        KeyBinding::new("alt-up", PrevChange, c),
        KeyBinding::new("ctrl-p", PrevChange, c),
        KeyBinding::new("ctrl-home", FirstChange, c),
        KeyBinding::new("ctrl-end", LastChange, c),
        KeyBinding::new("ctrl-1", FilterAll, c),
        KeyBinding::new("ctrl-2", FilterDifferences, c),
        KeyBinding::new("ctrl-3", FilterContext, c),
        KeyBinding::new("ctrl-4", FilterSame, c),
        KeyBinding::new("right", ScrollRight, c),
        KeyBinding::new("left", ScrollLeft, c),
        KeyBinding::new("home", ScrollHome, c),
        KeyBinding::new("f5", Reload, c),
        KeyBinding::new("escape", Escape, c),
        KeyBinding::new("up", RowUp, c),
        KeyBinding::new("down", RowDown, c),
        KeyBinding::new("pageup", PageUp, c),
        KeyBinding::new("pagedown", PageDown, c),
        KeyBinding::new("ctrl-f", OpenFind, c),
        KeyBinding::new("f3", FindNext, c),
        KeyBinding::new("shift-f3", FindPrev, c),
        KeyBinding::new("ctrl-g", OpenGoTo, c),
        KeyBinding::new("f9", SwapSides, c),
        KeyBinding::new("end", LineEnd, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("shift-home", SelectHome, c),
        KeyBinding::new("shift-end", SelectEnd, c),
        KeyBinding::new("shift-pageup", SelectPageUp, c),
        KeyBinding::new("shift-pagedown", SelectPageDown, c),
        KeyBinding::new("ctrl-left", WordLeft, c),
        KeyBinding::new("ctrl-right", WordRight, c),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, c),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, c),
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", DeleteForward, c),
        KeyBinding::new("enter", Newline, c),
        KeyBinding::new("tab", InsertTab, c),
        KeyBinding::new("ctrl-z", Undo, c),
        KeyBinding::new("ctrl-y", Redo, c),
        KeyBinding::new("ctrl-shift-z", Redo, c),
        KeyBinding::new("ctrl-c", CopyText, c),
        KeyBinding::new("ctrl-x", CutText, c),
        KeyBinding::new("ctrl-v", PasteText, c),
        KeyBinding::new("ctrl-a", SelectAll, c),
        KeyBinding::new("ctrl-s", Save, c),
        KeyBinding::new("ctrl-shift-s", SaveAs, c),
        KeyBinding::new("alt-right", CopyBlockRight, c),
        KeyBinding::new("alt-left", CopyBlockLeft, c),
        KeyBinding::new("ctrl-r", CopyBlockRight, c),
        KeyBinding::new("ctrl-l", CopyBlockLeft, c),
        KeyBinding::new("n", NextChangeKey, c),
        KeyBinding::new("p", PrevChangeKey, c),
        KeyBinding::new("f2", EditRow, c),
        KeyBinding::new("ctrl-o", OpenLeftFile, c),
        KeyBinding::new("ctrl-shift-o", OpenRightFile, c),
    ]
}

pub enum TextEvent {
    /// Esc with nothing to cancel or close: go back to where the pair was opened from.
    Back,
    /// F9 swapped the sides; the tab's key follows the new order.
    Swapped { left: PathBuf, right: PathBuf },
    /// A side was replaced (open, typed path, drop): the tab should show this pair. The view
    /// has asked about unsaved edits already; the workspace calls `load_paths` when it agrees.
    Open { left: PathBuf, right: PathBuf },
    /// Save As moved a side to a new file; the tab's key follows.
    Renamed { left: PathBuf, right: PathBuf },
    /// Open this pair in another kind of tab ("Open in hex"); this tab stays.
    OpenAs { kind: CompareKind },
}

impl EventEmitter<TextEvent> for TextView {}

pub struct TextView {
    pub(crate) request: TextRequest,
    pub(crate) model: Option<TextModel>,
    pub(crate) message: Option<SharedString>,
    /// What to do about `message`, shown under it in muted text.
    message_hint: Option<SharedString>,
    pub(crate) job: JobSlot,
    pub(crate) task: Option<Task<()>>,
    /// The row list and its scroll position: uniform unless wrapped (R2).
    pub(crate) rows: RowList,
    /// Where the last frame was scrolled to (`RowList::scroll_mark`), for following the
    /// scroll (R9).
    pub(crate) last_top: Option<(usize, Pixels)>,
    /// Frames left in which a scroll duet made itself does not move the current difference.
    pub(crate) follow_pause: u8,
    /// The code font's size in the rows (R4).
    pub(crate) font_size: f32,
    /// The remembered display choices (R7); each setter saves its own key.
    pub(crate) prefs: TextPrefs,
    /// Word wrap is on (R3): the rows are then in the wrapped list.
    pub(crate) wrap: bool,
    /// The most aligned rows wrap takes: `WRAP_MAX_ROWS`, lower in tests.
    pub(crate) wrap_limit: u32,
    /// The columns and tab width the wrapped rows were last measured at.
    pub(crate) wrap_measure: (usize, usize),
    /// The row list's width in the last frame, to tell a resize drag (X-18).
    pub(crate) list_width: Pixels,
    /// Lets the wrapped rows take the list's width once a resize drag has been still a moment
    /// (X-18).
    pub(crate) wrap_settle: Option<Task<()>>,
    /// While a resize drag goes on with wrap on: the width the rows keep until it settles.
    pub(crate) wrap_frozen: Option<Pixels>,
    /// Rows the lists have built so far, for the benches (X-18).
    pub(crate) rows_built: usize,
    pub(crate) focus: FocusHandle,
    pub(crate) display: DisplayOptions,
    /// Bounds of the overview bar from the last frame, for click-to-scroll (Task 13).
    pub(crate) overview_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// The left button went down on the overview bar and is still held.
    pub(crate) dragging_overview: bool,
    /// Bounds of the row list from the last frame, for measuring the columns a half shows.
    pub(crate) list_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// The most columns any drawn line reached since the model arrived; sizes the
    /// horizontal scrollbar.
    pub(crate) widest: Rc<Cell<usize>>,
    /// Full widths of the lines too long to show whole, for `widest`.
    pub(crate) line_widths: Rc<RefCell<rows::LineWidths>>,
    /// Counts those widths off the UI thread.
    pub(crate) width_job: JobSlot,
    /// Bounds of the horizontal scrollbar from the last frame.
    pub(crate) hscroll_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// The left button went down on the horizontal scrollbar and is still held.
    pub(crate) dragging_hscroll: bool,
    /// Where the held thumb was grabbed: px from its left edge.
    pub(crate) hscroll_grab: f32,
    /// A one-line note in the status bar, e.g. why a button did nothing.
    pub(crate) notice: Option<SharedString>,
    /// The tab was opened from a folder compare: the toolbar shows Back (R13).
    has_origin: bool,
    /// The filter to restore when a large pair's model was released for a reload.
    pub(crate) pending_filter: Option<TextFilter>,
    /// Which sides were mapped from disk (over 64 MiB) in the last result.
    pub(crate) mapped: [bool; 2],
    pub(crate) find: Option<FindBar>,
    pub(crate) goto: Option<GoToBar>,
    /// The rules panel while it is open (R8).
    pub(crate) rules: Option<rules_bar::RulesBar>,
    /// The line "Align this line…" started from, while the click on the other side is awaited.
    pub(crate) align: Option<AlignStart>,
    /// The row Ctrl+G went to, outlined until the next go-to or reload.
    pub(crate) marked: Option<u32>,
    pub(crate) probe: Option<ScrollProbe>,
    /// `DUET_TYPE_PROBE=n`: characters to type once the first compare lands.
    pub(crate) type_probe: Option<usize>,
    /// The caret and selection; None until a click or a test places one.
    pub(crate) caret: Option<Caret>,
    /// The rows picked on the line numbers (see `rowsel`); excludes the caret.
    pub(crate) row_sel: Option<RowSel>,
    /// A drag begun on a line number, while the button is held.
    pub(crate) row_drag: Option<RowDrag>,
    /// A press on a moved line's tag, while the button is held.
    pub(crate) move_drag: Option<MoveDrag>,
    /// The row (aligned index, side) last right-clicked: what its menu acts on.
    pub(crate) menu_row: Option<(u32, bool)>,
    /// A side's path box turned into an input (see `sides`).
    pub(crate) path_edit: Option<SideEdit>,
    /// The text every half drew this frame, by (display row, left), for hit-testing.
    pub(crate) layouts: Rc<RefCell<HashMap<(usize, bool), HalfText>>>,
    /// The IME's uncommitted text, in bytes of the caret's buffer.
    pub(crate) composing: Option<Range<usize>>,
    /// The background re-diff that follows edits, debounced.
    pub(crate) rediff_job: JobSlot,
    pub(crate) rediff_task: Option<Task<()>>,
    /// Undo and redo for the text on screen.
    pub(crate) undo: UndoStack,
    /// Syntax colors per side, indexed by `usize::from(!left)`; None until a parse lands.
    pub(crate) syntax: [Option<SyntaxSide>; 2],
    /// The Syntax button.
    pub(crate) syntax_on: bool,
    pub(crate) syntax_state: [SyntaxState; 2],
    pub(crate) syntax_job: [JobSlot; 2],
    /// F9 presses so far: a parse or a save that lands after an odd number of them was of the
    /// text that is now on the other side.
    pub(crate) syntax_swaps: u64,
    pub(crate) syntax_theme: Arc<HighlightTheme>,
    /// How long an edit may re-parse on the UI thread, given to every parsed side.
    pub(crate) syntax_budget: Duration,
    /// The Type menu's choice for this tab; None is Auto (R10). Not remembered.
    pub(crate) language: Option<Language>,
    /// Files whose save is still running; keyed by path, so F9 and reloads never confuse them.
    pub(crate) saving: Vec<PathBuf>,
    /// What the last re-diff cost, for the typing probe.
    pub(crate) last_rediff: Option<RediffTimes>,
    /// Bytes of each side's line the details pane wrapped in the last frame (I3).
    pub(crate) details_wrapped: [usize; 2],
    /// The details pane's scroll, which its labels follow (X-32, X-64).
    pub(crate) details_scroll: gpui_kit::ScrollHandle,
    /// The changed-on-disk poll (R18).
    pub(crate) watch: Watch,
    /// The invisible-character scan of each side, indexed by `usize::from(!left)`.
    pub(crate) odd: [odd::OddSide; 2],
    /// The hit the status button last went to, in the order it walks them.
    pub(crate) odd_pos: Option<usize>,
    pub(crate) odd_order: odd::OddOrder,
    /// Moves on with every new compare and reload: a plan made before it is of other text.
    pub(crate) model_epoch: u64,
    /// The background build of a Reformat, until it lands.
    pub(crate) reformatting: Option<Task<()>>,
    /// A lexed side's next lex, waiting for a pause in typing; indexed by `usize::from(!left)`.
    pub(crate) relex: [Option<Task<()>>; 2],
    /// Stops the reports still building when the tab closes.
    pub(crate) report_cancel: crate::text::report::ReportCancel,
}

impl Focusable for TextView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TextView {
    /// The Type menu's choice; None is Auto.
    pub fn language(&self) -> Option<Language> {
        self.language
    }

    /// The language the file names suggest: the left name decides, else the right (R10). The
    /// request's paths, so the first compare's rules know it before any result.
    pub fn detected_language(&self) -> Language {
        [&self.request.left, &self.request.right]
            .into_iter()
            .find_map(|path| path.file_name()?.to_str())
            .map_or(Language::Text, detect_language)
    }

    /// The language comment rules use: the Type menu's choice, else the detected one.
    pub fn comment_language(&self) -> Language {
        self.language.unwrap_or_else(|| self.detected_language())
    }

    /// Picks the language for this tab and colours both sides again; while Comments is on it
    /// compares again with that language's comments.
    pub fn set_language(&mut self, language: Option<Language>, cx: &mut Context<Self>) {
        if self.language == language {
            return;
        }
        self.language = language;
        self.start_syntax(true, cx);
        self.start_syntax(false, cx);
        // Comments are told by the language, and an invisible character in one is only a note.
        self.start_odd(true, true, cx);
        self.start_odd(false, true, cx);
        if self.prefs.ignore_comments {
            self.recompare(cx);
        }
    }

    pub fn new(left: PathBuf, right: PathBuf, cx: &mut Context<Self>) -> Self {
        register_extra_languages();
        let syntax_theme = crate::theme::syntax_theme(Palette::current(cx).dark);
        let mut prefs = TextPrefs::load(settings::settings_file(cx).as_deref());
        crate::text::prefs::join_shared(&mut prefs, cx);
        // Wrap stays within the row limit: the first result checks it. The env var is for the
        // wrapped-scroll bench row (Task 11).
        let wrap = prefs.wrap || std::env::var("DUET_TEXT_WRAP").is_ok_and(|v| v == "1");
        let font_size = prefs.font_size;
        let mut view = Self {
            request: TextRequest::new(left, right),
            model: None,
            message: None,
            message_hint: None,
            job: JobSlot::default(),
            task: None,
            rows: if wrap {
                RowList::wrapped(0, px(row_height_for(font_size)))
            } else {
                RowList::uniform()
            },
            last_top: None,
            follow_pause: 0,
            font_size,
            wrap,
            wrap_limit: WRAP_MAX_ROWS,
            wrap_measure: (0, 0),
            list_width: px(0.),
            wrap_settle: None,
            wrap_frozen: None,
            rows_built: 0,
            // A tab stop, first in the view's order, so Tab and Shift+Tab come back to the
            // rows after leaving them for a button or field.
            focus: cx.focus_handle().tab_stop(true),
            display: DisplayOptions {
                tab_width: prefs.tab_width.into(),
                show_whitespace: prefs.show_spaces,
                ..DisplayOptions::default()
            },
            overview_bounds: Rc::default(),
            dragging_overview: false,
            list_bounds: Rc::default(),
            widest: Rc::default(),
            line_widths: Rc::default(),
            width_job: JobSlot::default(),
            hscroll_bounds: Rc::default(),
            dragging_hscroll: false,
            hscroll_grab: 0.,
            notice: None,
            has_origin: false,
            pending_filter: Some(prefs.filter.filter(prefs.context_lines)),
            mapped: [false; 2],
            find: None,
            goto: None,
            rules: None,
            align: None,
            marked: None,
            probe: ScrollProbe::from_env(),
            type_probe: std::env::var("DUET_TYPE_PROBE")
                .ok()
                .and_then(|v| v.parse().ok()),
            caret: None,
            row_sel: None,
            row_drag: None,
            move_drag: None,
            menu_row: None,
            path_edit: None,
            layouts: Rc::default(),
            composing: None,
            rediff_job: JobSlot::default(),
            rediff_task: None,
            undo: UndoStack::default(),
            syntax: [None, None],
            syntax_on: prefs.syntax,
            syntax_state: [SyntaxState::Off("plain text"); 2],
            syntax_job: Default::default(),
            syntax_swaps: 0,
            syntax_theme,
            syntax_budget: EDIT_BUDGET,
            language: None,
            saving: Vec::new(),
            last_rediff: None,
            details_wrapped: [0; 2],
            details_scroll: gpui_kit::ScrollHandle::new(),
            watch: Watch::default(),
            odd: Default::default(),
            odd_pos: None,
            odd_order: Default::default(),
            model_epoch: 0,
            reformatting: None,
            relex: [None, None],
            report_cancel: Default::default(),
            prefs,
        };
        view.request.detect_moves = view.prefs.moves;
        let p = &view.prefs;
        view.request.options = DiffOptions {
            whitespace: p.whitespace,
            ignore_case: p.ignore_case,
            ignore_eol: p.ignore_eol,
        };
        view.reload(cx);
        view
    }

    pub fn model(&self) -> Option<&TextModel> {
        self.model.as_ref()
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Shows or hides the toolbar's Back button: the workspace knows where the tab came from.
    pub fn set_has_origin(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.has_origin != on {
            self.has_origin = on;
            cx.notify();
        }
    }

    pub fn has_origin(&self) -> bool {
        self.has_origin
    }

    /// The binary notice's "Open as text": reloads with both files decoded as text (R14).
    pub fn open_as_text(&mut self, cx: &mut Context<Self>) {
        self.request.force_text = true;
        self.reload(cx);
    }

    pub fn message_hint(&self) -> Option<&str> {
        self.message_hint.as_deref()
    }

    fn set_message(&mut self, title: impl Into<SharedString>, hint: &'static str) {
        self.message = Some(title.into());
        self.message_hint = Some(hint.into());
    }

    /// A compare runs or a re-diff is pending.
    pub fn is_busy(&self) -> bool {
        self.job.is_running() || self.rediff_job.is_running()
    }

    pub fn display_options(&self) -> DisplayOptions {
        self.display
    }

    pub fn request(&self) -> &TextRequest {
        &self.request
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// What the status bar says. A compare clears the notice when it starts, so a notice while
    /// it runs explains a key pressed meanwhile and shows instead of the progress line.
    pub fn status_line(&self) -> SharedString {
        if let Some(notice) = &self.notice {
            notice.clone()
        } else if self.job.is_running() {
            "Comparing… Press Esc to cancel.".into()
        } else {
            self.model
                .as_ref()
                .map(status_text)
                .unwrap_or_default()
                .into()
        }
    }

    /// The first row on screen (or the row a pending Top scroll will put there).
    pub fn top_row(&self) -> usize {
        self.rows.top_row(px(self.row_height()))
    }

    #[doc(hidden)]
    pub fn job_generation(&self) -> u64 {
        self.job.generation()
    }

    /// Test hook: deliver a job result as if job `generation` had finished.
    #[doc(hidden)]
    pub fn apply_for_test(
        &mut self,
        generation: u64,
        outcome: TextOutcome,
        cx: &mut Context<Self>,
    ) {
        self.apply(generation, outcome, cx);
    }

    /// Shows a one-line note in the status bar.
    pub fn show_notice(&mut self, notice: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.notice = Some(notice.into());
        cx.notify();
    }

    pub fn set_filter(&mut self, filter: TextFilter, cx: &mut Context<Self>) {
        if let Some(model) = &mut self.model {
            model.set_filter(filter);
            let display = model.current_display();
            // The list clamps its top to the new rows: not a scroll to follow (C1).
            self.note_programmatic_scroll();
            if let Some(display) = display {
                self.scroll_to_display(display, ScrollStrategy::Center);
            }
            self.remember_filter(filter, cx);
            cx.notify();
        }
    }

    pub fn expand_fold(&mut self, display: usize, rows: Option<u32>, cx: &mut Context<Self>) {
        if let Some(model) = &mut self.model {
            model.expand_fold(display, rows);
            cx.notify();
        }
    }

    /// Whether `left`'s side holds unsaved edits: the toolbar's Save buttons follow it.
    #[doc(hidden)]
    pub fn save_enabled(&self, left: bool) -> bool {
        self.model.as_ref().is_some_and(|m| m.is_dirty(left))
    }
}

/// Drops syntax colors on the background executor: freeing a large file's tree and rope takes
/// tens of milliseconds, which on the UI thread is a dropped frame.
fn drop_syntax(side: Option<SyntaxSide>, cx: &mut Context<TextView>) {
    if let Some(side) = side {
        cx.background_executor()
            .spawn(async move { drop(side) })
            .detach();
    }
}

/// Whether a reload brought back the text the undo history was made in: both files unchanged
/// on disk, read in the same encodings and line endings, and as long. Undo entries are byte
/// offsets into that text. A save that converted the line endings left the edited text in the
/// old ones while its document names the new ones (PF5), so such a side never counts as the
/// same text, even when the length did not change (CR to LF, M5).
fn same_text(old: &TextModel, new: &TextModel) -> bool {
    fn doc(model: &TextModel, left: bool) -> &TextDocument {
        if left { model.left() } else { model.right() }
    }
    [true, false].into_iter().all(|left| {
        let (old_doc, new_doc) = (doc(old, left), doc(new, left));
        !old.converted(left)
            && old.disk_stamp(left) == new.disk_stamp(left)
            && old_doc.encoding() == new_doc.encoding()
            && old_doc.eol() == new_doc.eol()
            && old_doc.bytes().len() == new_doc.bytes().len()
    })
}

/// A half that places the caret where it is clicked and extends the selection while the
/// left button drags over the caret's side.
fn clickable(half: Div, display: usize, left: bool, cx: &mut Context<TextView>) -> Div {
    half.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
            // While a manual alignment is awaited, the press picks the line to pair.
            if this.aligning().is_some() {
                cx.stop_propagation();
                this.finish_align(display, left, cx);
                return;
            }
            this.click_text(display, left, e.position, e.modifiers.shift, window, cx)
        }),
    )
    .on_mouse_down(
        MouseButton::Right,
        cx.listener(move |this, _, window, cx| this.press_right(display, Some(left), window, cx)),
    )
    .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, window, cx| {
        if e.pressed_button == Some(MouseButton::Left) && this.caret.is_some_and(|c| c.left == left)
        {
            this.click_text(display, left, e.position, true, window, cx)
        }
    }))
}

pub(crate) const BINARY_HINT: &str =
    "These files are not text. Open a text pair, or press F5 to compare again.";
const TOO_LARGE_AS_TEXT: &str = "Binary files over 64 MiB can't be opened as text.";
const FAILED_HINT: &str =
    "Check that both files exist and can be read, then press F5 to compare again.";
const CANCELLED_HINT: &str = "Press F5 to compare again.";

impl Render for TextView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The rows of the last frame are gone; the rows this frame paints register again.
        self.layouts.borrow_mut().clear();
        self.watch_frame(cx);
        // Another tab may have changed the rules or the On save choices (I2).
        self.pull_shared(cx);
        if let (Some(probe), Some(model)) = (self.probe.as_mut(), self.model.as_ref()) {
            let done = probe.frame();
            let rows = model.row_count().max(1);
            probe.row = (probe.row + 997) % rows;
            let row = probe.row;
            self.scroll_to_display(row, ScrollStrategy::Top);
            window.request_animation_frame();
            if done {
                cx.quit();
            }
        }
        if self.model.is_some()
            && !self.is_busy()
            && let Some(count) = self.type_probe.take()
        {
            let entity = cx.entity();
            window.on_next_frame(move |window, cx| {
                entity.update(cx, |view, cx| view.run_type_probe(count, window, cx));
            });
        }
        self.drag_tick(window, cx);
        self.follow_scroll(window);
        let palette = Palette::current(cx);
        let fonts = Fonts::current(cx);
        self.syntax_theme = crate::theme::syntax_theme(palette.dark);
        let list = self.list_bounds.get();
        let details_advance = code_advance_at(window, &fonts.code, self.font_size);
        // A resize drag with wrap on keeps the rows' width until it settles (X-18).
        let rows_width = self.follow_list_width(list.size.width, cx);
        if rows_width > px(0.) {
            self.display.view_columns = view_columns(rows_width, details_advance);
        }
        let names_width = f32::from(list.size.width) + OVERVIEW_WIDTH;
        let rows = self.model.as_ref().map_or(0, TextModel::row_count);
        let row_height = self.row_height();
        let revision = self.model.as_ref().map_or(0, TextModel::revision);
        self.measure_wrapped();
        self.rows.sync(rows, px(row_height), revision);
        let body = match &self.message {
            Some(title) => v_flex()
                .id("text-message")
                .test_support()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .p_4()
                .text_center()
                .child(
                    div()
                        .id("text-message-title")
                        .test_support()
                        .text_size(px(15.))
                        .text_color(palette.text)
                        .child(title.clone()),
                )
                .children(self.message_hint.clone().map(|hint| {
                    div()
                        .id("text-message-hint")
                        .test_support()
                        .text_color(palette.muted)
                        .child(hint)
                }))
                .children(self.is_binary_message().then(|| {
                    ld_button("text-open-hex", "Open in hex", &palette).ld_click(
                        true,
                        cx.listener(|this, _, _, cx| this.open_as(CompareKind::Hex, cx)),
                    )
                }))
                .children(self.is_binary_message().then(|| {
                    ld_button("text-open-as-text", "Open as text", &palette)
                        .ld_click(true, cx.listener(|this, _, _, cx| this.open_as_text(cx)))
                }))
                .children((self.is_binary_message() && self.both_pictures()).then(|| {
                    ld_button("text-open-picture", "Open as picture", &palette).ld_click(
                        true,
                        cx.listener(|this, _, _, cx| this.open_as(CompareKind::Picture, cx)),
                    )
                }))
                .into_any_element(),
            None if self.is_empty_pair() => self.render_empty(&palette, &fonts, cx),
            None => v_flex()
                .flex_1()
                .min_h_0()
                .child(
                    h_flex()
                        .flex_1()
                        .min_h_0()
                        .child(
                            div()
                                .id("text-list")
                                .test_support()
                                .capture_any_mouse_down(cx.listener(
                                    |this, e: &MouseDownEvent, _, _| {
                                        if e.button == MouseButton::Right {
                                            this.menu_row = None;
                                        }
                                    },
                                ))
                                .relative()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                // A resize drag's rows keep their width: clipped here.
                                .overflow_hidden()
                                .bg(palette.panel)
                                .font(fonts.code.clone())
                                .text_size(px(self.font_size))
                                .line_height(px(row_height))
                                .child({
                                    // `render` measures the columns from these bounds, so a
                                    // new width needs one more frame.
                                    let cell = self.list_bounds.clone();
                                    canvas(
                                        move |bounds, window, _| {
                                            if cell.replace(bounds).size.width != bounds.size.width
                                            {
                                                window.request_animation_frame();
                                            }
                                        },
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .size_full()
                                })
                                .child(match &self.rows {
                                    RowList::Uniform(handle) => uniform_list(
                                        "text-rows",
                                        rows,
                                        cx.processor(Self::render_rows),
                                    )
                                    .track_scroll(handle)
                                    .size_full()
                                    .into_any_element(),
                                    // The list asks for one row at a time, outside `render`.
                                    RowList::Wrapped(wrapped) => {
                                        let view = cx.entity();
                                        gpui_kit::list(
                                            wrapped.state.clone(),
                                            move |ix, window, cx| {
                                                view.update(cx, |v, cx| {
                                                    v.render_row(ix, window, cx)
                                                })
                                            },
                                        )
                                        .h_full()
                                        .map(|list| {
                                            if rows_width > px(0.) {
                                                list.flex_none().w(rows_width)
                                            } else {
                                                list.w_full()
                                            }
                                        })
                                        .into_any_element()
                                    }
                                })
                                // Typing and the IME reach the caret through this handler.
                                // A canvas takes no mouse hitbox, so clicks still reach rows.
                                .children(self.row_drag_follower(cx))
                                .children(self.move_drag_follower(cx))
                                .children(self.move_marker(&palette))
                                .when(self.caret.is_some(), |d| {
                                    let entity = cx.entity();
                                    let focus = self.focus.clone();
                                    d.child(
                                        canvas(
                                            |_, _, _| {},
                                            move |bounds, (), window, cx| {
                                                window.handle_input(
                                                    &focus,
                                                    ElementInputHandler::new(bounds, entity),
                                                    cx,
                                                );
                                            },
                                        )
                                        .absolute()
                                        .size_full(),
                                    )
                                })
                                .map(|d| match &self.rows {
                                    RowList::Uniform(handle) => d.vertical_scrollbar(handle),
                                    RowList::Wrapped(wrapped) => {
                                        d.vertical_scrollbar(&wrapped.state)
                                    }
                                })
                                .context_menu({
                                    let (view, focus) =
                                        (cx.entity().downgrade(), self.focus.clone());
                                    move |menu, _, cx: &mut Context<PopupMenu>| {
                                        let entries = view
                                            .upgrade()
                                            .map(|v| row_menu(v.read(cx)))
                                            .unwrap_or_default();
                                        fill_menu(menu, entries, Some(focus.clone()))
                                    }
                                }),
                        )
                        .child(self.render_overview(&palette, cx)),
                )
                .children(self.render_hscroll(&palette, cx))
                .into_any_element(),
        };
        v_flex()
            .id("text-view")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .bg(palette.bg)
            .text_color(palette.text)
            .text_size(px(UI_SIZE))
            .on_action(cx.listener(Self::next_change))
            .on_action(cx.listener(Self::prev_change))
            .on_action(cx.listener(Self::next_change_key))
            .on_action(cx.listener(Self::prev_change_key))
            .on_action(cx.listener(Self::first_change))
            .on_action(cx.listener(Self::last_change))
            .on_action(cx.listener(Self::filter_all))
            .on_action(cx.listener(Self::filter_differences))
            .on_action(cx.listener(Self::filter_context))
            .on_action(cx.listener(Self::filter_same))
            .on_action(cx.listener(Self::toggle_whitespace))
            .on_action(cx.listener(Self::scroll_left))
            .on_action(cx.listener(Self::scroll_right))
            .on_action(cx.listener(Self::scroll_home))
            .on_action(cx.listener(Self::on_reload))
            .on_action(cx.listener(Self::on_escape))
            .on_action(cx.listener(Self::open_find))
            .on_action(cx.listener(Self::find_next))
            .on_action(cx.listener(Self::find_prev))
            .on_action(cx.listener(Self::open_goto))
            .on_action(cx.listener(|this, _: &SwapSides, _, cx| this.swap_sides(cx)))
            .on_action(cx.listener(Self::row_up))
            .on_action(cx.listener(Self::row_down))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::line_end))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::select_page_up))
            .on_action(cx.listener(Self::select_page_down))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::on_backspace))
            .on_action(cx.listener(Self::on_delete_forward))
            .on_action(cx.listener(Self::on_newline))
            .on_action(cx.listener(Self::on_insert_tab))
            .on_action(cx.listener(Self::on_undo))
            .on_action(cx.listener(Self::on_redo))
            .on_action(cx.listener(Self::on_copy))
            .on_action(cx.listener(Self::on_cut))
            .on_action(cx.listener(Self::on_paste))
            .on_action(cx.listener(Self::on_select_all))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_save_as))
            .on_action(
                cx.listener(|this, _: &OpenLeftFile, window, cx| this.pick_file(true, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &OpenRightFile, window, cx| {
                    this.pick_file(false, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &PasteSideLeft, _, cx| {
                this.replace_side_with_clipboard(true, cx);
            }))
            .on_action(cx.listener(|this, _: &PasteSideRight, _, cx| {
                this.replace_side_with_clipboard(false, cx);
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.on_files_dropped(paths, window, cx)
            }))
            .on_action(cx.listener(|this, _: &CopyBlockRight, window, cx| {
                this.on_copy_block(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &CopyBlockLeft, window, cx| {
                this.on_copy_block(true, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &CopyBlockHereRight, _, cx| this.copy_block_here(false, cx)),
            )
            .on_action(
                cx.listener(|this, _: &CopyBlockHereLeft, _, cx| this.copy_block_here(true, cx)),
            )
            .on_action(cx.listener(|this, _: &EditRow, window, cx| this.edit_row(window, cx)))
            .on_action(cx.listener(|this, _: &AlignWithOtherSide, _, cx| this.start_align(cx)))
            .on_action(cx.listener(|this, _: &ClearAlignments, _, cx| this.clear_anchors(cx)))
            .on_action(cx.listener(|this, _: &TreatAsUnimportant, window, cx| {
                this.treat_menu_row_as_unimportant(window, cx)
            }))
            .on_action(cx.listener(|this, _: &DeleteRows, _, cx| {
                this.menu_row = None;
                this.delete_selected_rows(cx)
            }))
            .on_action(cx.listener(|this, _: &ReportHtml, window, cx| this.report_html(window, cx)))
            .on_action(
                cx.listener(|this, _: &ReportPatch, window, cx| this.report_patch(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ReportText, window, cx| this.report_text(window, cx)))
            .on_action(cx.listener(|this, _: &CopyPatch, _, cx| this.copy_patch(cx)))
            .on_action(cx.listener(|this, _: &CopyAllRight, _, cx| {
                this.copy_all(false, cx);
            }))
            .on_action(cx.listener(|this, _: &CopyAllLeft, _, cx| {
                this.copy_all(true, cx);
            }))
            .map(|d| path_actions(d, cx))
            .child(self.render_toolbar(&palette, &fonts, cx))
            .child(self.render_subbar(&palette, cx))
            .children(self.render_rules(&palette, &fonts, cx))
            .children(self.render_find(&palette, cx))
            .children(self.render_goto(&palette, cx))
            .children(self.render_disk_notice(&palette, cx))
            .child(self.render_names(&palette, names_width, cx))
            .child(body)
            .children(self.render_details(
                &palette,
                &fonts,
                f32::from(rows_width) + OVERVIEW_WIDTH,
                details_advance,
            ))
            .child(self.render_status(&palette, window, cx))
    }
}
