//! Folder compare tab: an aligned two-pane tree in one virtualized list.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use duet_core::folder::entry::{EntryKind, Filters, ScanError, join_rel};
use duet_core::folder::run::{CompareMode, FolderOptions, FolderProgress, compare_folders};
use duet_core::folder::scan::validate_filters;
use duet_core::folder::tree::{FolderComparison, Side};
use duet_core::kind::CompareKind;
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::v_flex;
use gpui_kit::{
    App, Context, Entity, EventEmitter, ExternalPaths, FocusHandle, Focusable, KeyBinding,
    Modifiers, MouseButton, MouseDownEvent, PathPromptOptions, ScrollStrategy, SharedString,
    Subscription, Task, TextLayout, UniformListScrollHandle, Window, actions, div, prelude::*, px,
    uniform_list,
};

use crate::folder::batch::RunningBatch;
use crate::folder::batch::{DeleteAsk, DeleteSides, TrashFn};
use crate::folder::export_panel::{self, ExportConfirm, ExportState, export_actions};
use crate::folder::job::shallow;
use crate::folder::menu::{list_menu, menu_actions};
use crate::folder::model::{FolderFilter, FolderModel, SortKey};
use crate::folder::odd::OddRun;
use crate::folder::ops::Direction;
use crate::folder::ops::{empty_state, parse_masks, progress_text};
use crate::folder::prefs::{self, FolderPrefs, Layout};
use crate::folder::select::{DragSelect, drag_listeners};
use crate::folder::sync_panel::{self, SyncConfirm, SyncState, sync_actions};
use crate::job::JobSlot;
use crate::palette::Palette;
use crate::pathbar::ActiveEdit;
use crate::settings;
use crate::status::StatusPart;
use crate::ui::fonts::Fonts;
use duet_core::folder::actions::{move_to_trash, roots_overlap};
use options::{
    CriterionContent, CriterionQuick, CriterionRules, CriterionSize, ToggleConfirmCopy,
    ToggleEnterFolders, ToggleOddScan,
};

mod flat;
pub mod legend;
pub mod options;
mod rows;
mod status;
mod toolbar;

actions!(
    folder,
    [
        SelectDown,
        SelectUp,
        PageDown,
        PageUp,
        ExpandRow,
        CollapseRow,
        OpenRow,
        NextDifference,
        PrevDifference,
        ExpandAll,
        CollapseAll,
        Refresh,
        CopyToRight,
        CopyToLeft,
        DeleteSelected,
        DeleteLeft,
        DeleteRight,
        DeleteBoth,
        TouchRight,
        TouchLeft,
        RevealSelected,
        CancelJob,
        SelectFirst,
        SelectLast,
        SwapSides,
        SelectAllRows,
        ExtendUp,
        ExtendDown,
        ExtendPageUp,
        ExtendPageDown,
        FocusFilter
    ]
);

pub const CONTEXT: &str = "FolderView";
pub(super) const ROW_HEIGHT: f32 = 24.0;
const PAGE_ROWS: isize = 20;

pub fn key_bindings() -> Vec<KeyBinding> {
    let c = Some(CONTEXT);
    vec![
        KeyBinding::new("down", SelectDown, c),
        KeyBinding::new("up", SelectUp, c),
        KeyBinding::new("pagedown", PageDown, c),
        KeyBinding::new("pageup", PageUp, c),
        KeyBinding::new("right", ExpandRow, c),
        KeyBinding::new("left", CollapseRow, c),
        KeyBinding::new("enter", OpenRow, c),
        KeyBinding::new("f8", NextDifference, c),
        KeyBinding::new("alt-down", NextDifference, c),
        KeyBinding::new("ctrl-n", NextDifference, c),
        KeyBinding::new("f7", PrevDifference, c),
        KeyBinding::new("alt-up", PrevDifference, c),
        KeyBinding::new("ctrl-p", PrevDifference, c),
        KeyBinding::new("f5", Refresh, c),
        KeyBinding::new("alt-right", CopyToRight, c),
        KeyBinding::new("alt-left", CopyToLeft, c),
        KeyBinding::new("ctrl-r", CopyToRight, c),
        KeyBinding::new("ctrl-l", CopyToLeft, c),
        KeyBinding::new("delete", DeleteSelected, c),
        KeyBinding::new("shift-delete", DeleteBoth, c),
        KeyBinding::new("escape", CancelJob, c),
        KeyBinding::new("home", SelectFirst, c),
        KeyBinding::new("end", SelectLast, c),
        KeyBinding::new("f9", SwapSides, c),
        KeyBinding::new("ctrl-a", SelectAllRows, c),
        KeyBinding::new("ctrl-f", FocusFilter, c),
        KeyBinding::new("shift-up", ExtendUp, c),
        KeyBinding::new("shift-down", ExtendDown, c),
        KeyBinding::new("shift-pageup", ExtendPageUp, c),
        KeyBinding::new("shift-pagedown", ExtendPageDown, c),
        KeyBinding::new("enter", SyncConfirm, Some(sync_panel::CONTEXT)),
        KeyBinding::new("enter", ExportConfirm, Some(export_panel::CONTEXT)),
    ]
}

pub enum FolderEvent {
    /// `kind` is how to open the pair; `None` leaves it to the workspace (text).
    OpenFiles {
        left: PathBuf,
        right: PathBuf,
        kind: Option<CompareKind>,
    },
    /// F9 swapped the roots; the tab's key follows the new order.
    Swapped { left: PathBuf, right: PathBuf },
    /// A picker or Up changed the roots; the tab's key follows them.
    Rooted { left: PathBuf, right: PathBuf },
}

pub struct FolderView {
    pub(crate) left: PathBuf,
    pub(crate) right: PathBuf,
    options: FolderOptions,
    /// The remembered choices (filter, layout, ...) this tab started from and keeps writing.
    pub(super) prefs: FolderPrefs,
    pub(super) model: Option<FolderModel>,
    pub(crate) status: SharedString,
    /// The same line as coloured parts while a compare result is on show; empty for progress,
    /// cancel and error texts.
    status_parts: Vec<StatusPart>,
    /// When the running compare started, and how long the last finished one took.
    compare_started: Option<std::time::Instant>,
    took: Option<Duration>,
    /// A one-line note shown instead of the status, e.g. that F8 wrapped around.
    pub(super) notice: Option<SharedString>,
    /// The path box being typed into (see `paths.rs`).
    pub(super) path_edit: Option<ActiveEdit>,
    pub(super) job: JobSlot,
    /// The invisible-character scan that follows a finished compare (see `odd.rs`).
    pub(super) odd: OddRun,
    /// The copy, delete or touch that runs in the background (see `batch.rs`).
    pub(super) batch: Option<RunningBatch>,
    /// Bumped whenever the roots, options or tree are replaced; a prompt that was asked
    /// before that no longer describes what is on screen.
    pub(super) plan_epoch: u64,
    /// The kind check of the last Enter or double-click on a file (see `menu.rs`).
    pub(super) detect: JobSlot,
    /// Moves a path to the Recycle Bin; tests swap it for a fake.
    pub(super) trash: TrashFn,
    task: Option<Task<()>>,
    /// Updates the status line with scan counts while a compare runs.
    progress_task: Option<Task<()>>,
    include: Entity<InputState>,
    exclude: Entity<InputState>,
    /// The Quick time tolerance, in whole seconds.
    tolerance: Entity<InputState>,
    name_filter: Entity<InputState>,
    /// Whether the legend panel is open (see `view/legend.rs`).
    legend_open: bool,
    /// The sync panel (see `sync_panel.rs`).
    pub(super) sync: SyncState,
    /// The export panel and job (see `export_panel.rs`).
    pub(super) export: ExportState,
    /// Why the include/exclude masks were not applied.
    mask_error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
    pub(super) scroll: UniformListScrollHandle,
    pub(super) focus: FocusHandle,
    /// The press that may become a drag-select (see `select.rs`).
    pub(super) drag: DragSelect,
    /// Whether a row was pressed with the right button since the last such press began: the
    /// menu opens only then (see `menu.rs`).
    pub(super) menu_row_pressed: bool,
    /// When the first load started; the first-rows budget is measured from here.
    scan_started: Option<std::time::Instant>,
    /// The status parts' line as the last frame laid it out, for tests.
    status_shown: Option<TextLayout>,
}

impl EventEmitter<FolderEvent> for FolderView {}

impl Focusable for FolderView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl FolderView {
    pub fn new(
        left: PathBuf,
        right: PathBuf,
        options: FolderOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut masks = |placeholder: &'static str, masks: &[String]| {
            let value = masks.join("; ");
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            })
        };
        // Short enough for the 220 px fields in the code font; the labels name the fields.
        let include = masks("e.g. *.rs; *.toml", &options.filters.include);
        let exclude = masks("e.g. target; *.log", &options.filters.exclude);
        let name_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter paths"));
        let tolerance = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(options.quick.tolerance.as_secs().to_string(), window, cx);
            state
        });
        let on_enter = |this: &mut Self,
                        _: &Entity<InputState>,
                        event: &InputEvent,
                        _: &mut Window,
                        cx: &mut Context<Self>| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.apply_masks(cx);
            }
        };
        let subscriptions = vec![
            cx.subscribe_in(&include, window, on_enter),
            cx.subscribe_in(&exclude, window, on_enter),
            cx.subscribe_in(
                &tolerance,
                window,
                |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        this.tolerance_entered(window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &name_filter,
                window,
                |this: &mut Self, input, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        let text = input.read(cx).value().to_string();
                        this.set_name_filter(&text, cx);
                    }
                },
            ),
        ];
        let mut prefs = FolderPrefs::load(settings::settings_file(cx).as_deref());
        prefs.take_options(&options);
        let mut view = Self {
            left,
            right,
            options,
            prefs,
            model: None,
            status: "Scanning…".into(),
            status_parts: Vec::new(),
            compare_started: None,
            took: None,
            notice: None,
            path_edit: None,
            job: JobSlot::default(),
            odd: OddRun::default(),
            batch: None,
            plan_epoch: 0,
            detect: JobSlot::default(),
            trash: std::sync::Arc::new(move_to_trash),
            task: None,
            progress_task: None,
            include,
            exclude,
            tolerance,
            name_filter,
            legend_open: false,
            sync: SyncState::default(),
            export: ExportState::default(),
            mask_error: None,
            _subscriptions: subscriptions,
            scroll: UniformListScrollHandle::new(),
            drag: DragSelect::default(),
            menu_row_pressed: false,
            // A tab stop, first in the view's order, so Tab and Shift+Tab come back to the
            // rows after leaving them for a button or field.
            focus: cx.focus_handle().tab_stop(true),
            scan_started: None,
            status_shown: None,
        };
        view.refresh(cx);
        view
    }

    pub fn prefs(&self) -> &FolderPrefs {
        &self.prefs
    }

    pub fn paths(&self) -> (&PathBuf, &PathBuf) {
        (&self.left, &self.right)
    }

    pub fn model(&self) -> Option<&FolderModel> {
        self.model.as_ref()
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// Shows plain text in the status bar (progress, cancel, errors).
    pub(super) fn set_status(&mut self, text: impl Into<SharedString>) {
        self.status = text.into();
        self.status_parts.clear();
    }

    /// Test hook: the status parts' line as the last frame drew it, cut to the bar's room.
    #[doc(hidden)]
    pub fn status_shown_for_test(&self) -> Option<String> {
        self.status_shown.as_ref().map(TextLayout::text)
    }

    pub fn is_busy(&self) -> bool {
        self.job.is_running()
    }

    pub fn options(&self) -> &FolderOptions {
        &self.options
    }

    /// New compare options; the tree is compared again with them.
    pub fn set_options(&mut self, options: FolderOptions, cx: &mut Context<Self>) {
        self.options = options;
        self.refresh(cx);
    }

    fn update_options(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut FolderOptions)) {
        let mut options = self.options.clone();
        change(&mut options);
        self.set_options(options, cx);
    }

    pub(super) fn apply_masks(&mut self, cx: &mut Context<Self>) {
        let filters = Filters {
            include: parse_masks(&self.include.read(cx).value()),
            exclude: parse_masks(&self.exclude.read(cx).value()),
        };
        if let Err(err) = validate_filters(&filters) {
            self.mask_error = Some(err.to_string().into());
            cx.notify();
            return;
        }
        self.mask_error = None;
        self.update_options(cx, |o| o.filters = filters);
    }

    /// New roots (a picker, or Up): the tab follows them and the tree is compared from scratch.
    pub fn set_roots(&mut self, left: PathBuf, right: PathBuf, cx: &mut Context<Self>) {
        if self.refuse_while_batch(cx) {
            return;
        }
        self.plan_epoch += 1;
        self.left = left;
        self.right = right;
        self.model = None;
        cx.emit(FolderEvent::Rooted {
            left: self.left.clone(),
            right: self.right.clone(),
        });
        self.refresh(cx);
    }

    /// Both roots move to their parents. False, with nothing changed, when a batch is running
    /// (it says so) or either root has no parent.
    pub fn go_up(&mut self, cx: &mut Context<Self>) -> bool {
        if self.refuse_while_batch(cx) {
            return false;
        }
        let (Some(left), Some(right)) = (self.left.parent(), self.right.parent()) else {
            return false;
        };
        let (left, right) = (left.to_path_buf(), right.to_path_buf());
        self.set_roots(left, right, cx);
        true
    }

    /// A folder picker for one side.
    pub fn pick_root(&mut self, left: bool, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |view, cx| {
                let (l, r) = if left {
                    (path, view.right.clone())
                } else {
                    (view.left.clone(), path)
                };
                view.set_roots(l, r, cx);
            });
        })
        .detach();
    }

    /// F9: exchanges the roots. The rows flip in place, keeping expansion and selection; a
    /// running compare also restarts with the sides swapped.
    pub fn swap_sides(&mut self, cx: &mut Context<Self>) {
        if self.refuse_while_batch(cx) {
            return;
        }
        self.plan_epoch += 1;
        self.close_sync_for_compare(cx);
        self.close_export_for_compare(cx);
        self.drop_detect(cx);
        std::mem::swap(&mut self.left, &mut self.right);
        let rescan = self.swap_odd();
        // Flip what is on screen at once, so rows and roots always agree (copy and delete join
        // rows to roots), even if a running compare is then cancelled.
        let running = self.job.is_running();
        if let Some(old) = self.model.take() {
            let mut cmp = old.comparison().clone();
            cmp.swap_sides();
            let mut model = FolderModel::new(cmp);
            model.carry_over(&old);
            model.set_filter(old.filter().mirrored());
            self.set_model_status(&model, !running);
            self.model = Some(model);
        }
        if running || self.model.is_none() {
            self.refresh(cx);
        } else if rescan {
            self.start_odd_scan(false, cx);
        }
        cx.emit(FolderEvent::Swapped {
            left: self.left.clone(),
            right: self.right.clone(),
        });
        cx.notify();
    }

    pub fn set_name_filter(&mut self, text: &str, cx: &mut Context<Self>) {
        self.close_sync(cx);
        self.with_model(cx, |m| m.set_name_filter(text));
    }

    pub fn legend_open(&self) -> bool {
        self.legend_open
    }

    pub fn toggle_legend(&mut self, cx: &mut Context<Self>) {
        self.legend_open = !self.legend_open;
        cx.notify();
    }

    /// Ctrl+F (Ctrl+ด on the Thai layout): the keys go to the path filter, its text selected
    /// so typing replaces it (F-54).
    fn focus_filter(&mut self, _: &FocusFilter, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.name_filter.focus_handle(cx), cx);
        self.name_filter
            .update(cx, |field, cx| field.select_all(window, cx));
    }

    pub fn mask_error(&self) -> Option<&str> {
        self.mask_error.as_deref()
    }

    /// Whether a plain key (Enter, an arrow, Home, End, Page Up, Page Down, Delete) is for the
    /// rows: only while they have the focus. On a focused button or field the key goes on to
    /// it, as in `TextView::keys_edit_text`, so Enter clicks a focused chip.
    fn rows_take_key(&self, window: &Window, cx: &mut Context<Self>) -> bool {
        let focused = self.focus.is_focused(window);
        if !focused {
            cx.propagate();
        }
        focused
    }

    fn select_first(&mut self, _: &SelectFirst, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows_take_key(window, cx) {
            self.with_model(cx, FolderModel::select_first);
        }
    }

    fn select_last(&mut self, _: &SelectLast, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows_take_key(window, cx) {
            self.with_model(cx, FolderModel::select_last);
        }
    }

    #[doc(hidden)]
    pub fn include_input(&self) -> &Entity<InputState> {
        &self.include
    }

    #[doc(hidden)]
    pub fn exclude_input(&self) -> &Entity<InputState> {
        &self.exclude
    }

    #[doc(hidden)]
    pub fn name_filter_input(&self) -> &Entity<InputState> {
        &self.name_filter
    }

    /// The first row on screen.
    pub fn top_row(&self) -> usize {
        let state = self.scroll.0.borrow();
        if let Some(deferred) = &state.deferred_scroll_to_item
            && deferred.strategy == ScrollStrategy::Top
        {
            return deferred.item_index;
        }
        (-state.base_handle.offset().y / px(ROW_HEIGHT))
            .round()
            .max(0.) as usize
    }

    #[doc(hidden)]
    pub fn job_generation(&self) -> u64 {
        self.job.generation()
    }

    /// Test hook: deliver a job result as if job `generation` had produced it.
    #[doc(hidden)]
    pub fn apply_for_test(
        &mut self,
        generation: u64,
        result: Result<FolderComparison, ScanError>,
        done: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        self.apply(generation, result, done, cx)
    }

    /// Esc stops, in order, a path edit, a running batch (first, so Esc always stops a sync:
    /// PF5), a running export, the legend, the sync or export panel and a running compare
    /// (P10b PF6); with none, it gives the keys back to the rows.
    fn cancel_job(&mut self, _: &CancelJob, window: &mut Window, cx: &mut Context<Self>) {
        if self.path_edit.is_some() {
            self.cancel_path_edit(window, cx);
            return;
        }
        if self.cancel_batch(cx) || self.cancel_export(cx) {
            return;
        }
        if self.legend_open {
            self.legend_open = false;
            cx.notify();
            return;
        }
        if self.sync_open() {
            self.close_sync_in(window, cx);
            return;
        }
        if self.export_open() {
            self.close_export_in(window, cx);
            return;
        }
        if !self.job.is_running() {
            if !self.focus.is_focused(window) {
                window.focus(&self.focus, cx);
                cx.notify();
            }
            return;
        }
        self.job.cancel();
        self.task = None;
        self.progress_task = None;
        self.set_status("Cancelled. Press F5 to compare again.");
        cx.notify();
    }

    /// Rescans both sides. The first load shows top-level rows at once (shallow pass), then
    /// the full result; later refreshes keep the old tree on screen until the new one is ready.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.plan_epoch += 1;
        self.detect.cancel();
        self.odd.drop_all();
        self.notice = None;
        self.close_sync_for_compare(cx);
        self.close_export_for_compare(cx);
        self.set_result_live(false);
        self.compare_started = Some(std::time::Instant::now());
        self.took = None;
        let (generation, cancel) = self.job.start();
        let first_load = self.model.is_none();
        if first_load {
            self.scan_started = Some(std::time::Instant::now());
        }
        let (left, right, mut options) =
            (self.left.clone(), self.right.clone(), self.options.clone());
        if options.mode == CompareMode::Rules {
            // The newest rules of the text tabs, read now so a change applies to this compare.
            options.text_rules = prefs::text_rules(cx);
        }
        self.set_status("Scanning… · Esc to cancel");
        let background = cx.background_executor().clone();
        let progress = Arc::new(FolderProgress::default());
        let ticker = progress.clone();
        self.progress_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let running = this
                    .update(cx, |view, cx| {
                        let running = view.job.is_current(generation);
                        if running {
                            view.set_status(progress_text(&ticker));
                            cx.notify();
                        }
                        running
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        }));
        // The first pass starts here, not when the task below first runs: on a first load that
        // is after the window's first draw, which the shallow scan now overlaps instead of
        // waiting behind. The full compare still waits for it: started at once, its scan
        // threads slowed the first draw itself.
        let rows = first_load.then(|| {
            let (l, r, o) = (left.clone(), right.clone(), options.clone());
            background.spawn(async move { shallow(&l, &r, &o) })
        });
        // Whether one root is inside the other resolves both paths, which can stall on a
        // dropped share: worked out here, beside the compare (sync reads it).
        let full = move || {
            background.spawn(async move {
                let nested = roots_overlap(&left, &right);
                (
                    compare_folders(&left, &right, &options, &progress, &cancel),
                    nested,
                )
            })
        };
        self.task = Some(cx.spawn(async move |this, cx| {
            if let Some(rows) = rows {
                let rows = rows.await;
                let go_on = this
                    .update(cx, |view, cx| view.apply(generation, rows, false, cx))
                    .unwrap_or(false);
                if !go_on {
                    return;
                }
            }
            let (full, nested) = full().await;
            let _ = this.update(cx, |view, cx| {
                if view.job.is_current(generation) {
                    view.set_roots_nested(nested);
                }
                view.apply(generation, full, true, cx)
            });
        }));
        cx.notify();
    }

    /// Applies a job result if it is still the newest job; returns whether to continue.
    fn apply(
        &mut self,
        generation: u64,
        result: Result<FolderComparison, ScanError>,
        done: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.job.is_current(generation) {
            return false;
        }
        match result {
            Ok(cmp) => {
                self.plan_epoch += 1;
                let mut model = FolderModel::new(cmp);
                // The live model, not a snapshot from when the refresh started: what the user
                // did while it ran (filter, expansion, selection) carries over.
                match &self.model {
                    Some(old) => model.carry_over(old),
                    None => {
                        model.set_layout(self.prefs.layout);
                        model.set_filter(self.prefs.filter);
                    }
                }
                if done {
                    self.job.finish(generation);
                    self.progress_task = None;
                }
                if done {
                    self.took = self.compare_started.map(|t| t.elapsed());
                }
                self.set_model_status(&model, done);
                self.model = Some(model);
                if done {
                    // Read from the new rows, so set after them (M-1).
                    self.set_result_live(true);
                    // The rows are on screen; the scan starts after them and writes nothing else.
                    self.start_odd_scan(false, cx);
                }
                if !done {
                    let ms = self
                        .scan_started
                        .map_or(0, |t| t.elapsed().as_millis() as u64);
                    tracing::info!(ms, "first rows");
                    if std::env::var_os("DUET_EXIT_AFTER_FIRST_ROWS").is_some() {
                        cx.quit();
                    }
                }
            }
            Err(ScanError::Cancelled(_)) => return false,
            Err(err) => {
                self.job.finish(generation);
                self.progress_task = None;
                self.set_status(format!("Error: {err}"));
                cx.notify();
                return false;
            }
        }
        cx.notify();
        true
    }

    pub fn set_filter(&mut self, filter: FolderFilter, cx: &mut Context<Self>) {
        self.close_sync(cx);
        self.prefs.filter = filter;
        self.remember(prefs::FILTER, cx);
        if let Some(mut model) = self.model.take() {
            model.set_filter(filter);
            let done = !self.job.is_running();
            self.set_model_status(&model, done);
            self.model = Some(model);
            cx.notify();
        }
    }

    /// The tree or the flat list; remembered.
    pub fn set_layout(&mut self, layout: Layout, cx: &mut Context<Self>) {
        self.prefs.layout = layout;
        self.remember(prefs::VIEW, cx);
        self.with_model(cx, |m| m.set_layout(layout));
    }

    /// A flat-list header click (the same column flips the order); not remembered.
    pub fn sort_by(&mut self, key: SortKey, cx: &mut Context<Self>) {
        self.with_model(cx, |m| m.sort_by(key));
    }

    /// Writes one remembered choice; a write error is logged, not shown.
    fn remember(&mut self, key: &'static str, cx: &mut Context<Self>) {
        let file = settings::settings_file(cx);
        if let Err(err) = self.prefs.save_key(key, file.as_deref()) {
            tracing::warn!(%err, key, "could not remember a folder view choice");
        }
    }

    fn with_model(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut FolderModel)) {
        self.notice = None;
        if let Some(model) = &mut self.model {
            change(model);
            if let Some(row) = model.selected_row() {
                self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
            }
            self.sync_selection_changed();
            cx.notify();
        }
    }

    fn select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows_take_key(window, cx) {
            self.with_model(cx, |m| m.move_selection(1));
        }
    }

    fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows_take_key(window, cx) {
            self.with_model(cx, |m| m.move_selection(-1));
        }
    }

    fn page_down(&mut self, _: &PageDown, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows_take_key(window, cx) {
            self.with_model(cx, |m| m.move_selection(PAGE_ROWS));
        }
    }

    fn page_up(&mut self, _: &PageUp, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows_take_key(window, cx) {
            self.with_model(cx, |m| m.move_selection(-PAGE_ROWS));
        }
    }

    fn expand_row(&mut self, _: &ExpandRow, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows_take_key(window, cx) {
            self.with_model(cx, FolderModel::expand_selected);
        }
    }

    fn collapse_row(&mut self, _: &CollapseRow, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows_take_key(window, cx) {
            self.with_model(cx, FolderModel::collapse_selected);
        }
    }

    fn expand_all(&mut self, _: &ExpandAll, _: &mut Window, cx: &mut Context<Self>) {
        self.with_model(cx, FolderModel::expand_all);
    }

    fn collapse_all(&mut self, _: &CollapseAll, _: &mut Window, cx: &mut Context<Self>) {
        self.with_model(cx, FolderModel::collapse_all);
    }

    fn next_difference(&mut self, _: &NextDifference, _: &mut Window, cx: &mut Context<Self>) {
        let mut wrapped = false;
        self.with_model(cx, |m| {
            wrapped = m.next_difference_wrapping().is_some_and(|step| step.1)
        });
        if wrapped {
            self.notice = Some("Wrapped to the first difference.".into());
        }
    }

    fn prev_difference(&mut self, _: &PrevDifference, _: &mut Window, cx: &mut Context<Self>) {
        let mut wrapped = false;
        self.with_model(cx, |m| {
            wrapped = m.prev_difference_wrapping().is_some_and(|step| step.1)
        });
        if wrapped {
            self.notice = Some("Wrapped to the last difference.".into());
        }
    }

    fn on_refresh(&mut self, _: &Refresh, _: &mut Window, cx: &mut Context<Self>) {
        self.refresh(cx);
    }

    fn open_row(&mut self, _: &OpenRow, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows_take_key(window, cx) {
            self.open_selected(cx);
        }
    }

    /// Enter / double-click: a folder toggles (or, with "Double-click a folder to compare
    /// inside it", becomes the base when it is on both sides); a link or junction on either
    /// side toggles too, as it is never followed (F-123); a file present on both sides opens
    /// as text or hex; a file on one side opens as text beside an untitled side (R21).
    fn open_selected(&mut self, cx: &mut Context<Self>) {
        let Some(model) = &self.model else {
            return;
        };
        let Some(row) = model.selected_row() else {
            return;
        };
        let node = model.node_at(row);
        let link = [&node.left, &node.right]
            .into_iter()
            .flatten()
            .any(|e| e.kind == EntryKind::Symlink);
        if node.is_dir || link {
            if self.prefs.enter_folders && self.selected_folder_is_enterable() {
                self.set_as_base(cx);
            } else if let Some(model) = &mut self.model {
                model.toggle_row(row);
                cx.notify();
            }
            return;
        }
        if let Some((left, right)) = self.selected_pair() {
            self.open_detected(left, right, cx);
        } else if let Some((left, right)) = self.selected_sides() {
            self.drop_detect(cx);
            cx.emit(FolderEvent::OpenFiles {
                left,
                right,
                kind: Some(CompareKind::Text),
            });
        }
    }

    /// A click on a row. Ctrl alone was done by the press (which began a drag); Shift extends
    /// the range from the anchor; a double click opens, but only without modifiers.
    fn click_row(
        &mut self,
        row: usize,
        clicks: usize,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        if modifiers.shift {
            if let Some(model) = &mut self.model {
                model.extend_to(row, modifiers.control);
            }
        } else if clicks >= 2 && !modifiers.control {
            self.open_selected(cx);
        }
        cx.notify();
    }

    /// Clicking the expander toggles the folder (it stops the press reaching the row).
    fn toggle_expander(&mut self, row: usize, cx: &mut Context<Self>) {
        if let Some(model) = &mut self.model {
            model.toggle_row(row);
            cx.notify();
        }
    }

    /// Ctrl+A and Shift+arrows: only while the rows have the focus.
    fn select_all_rows(&mut self, _: &SelectAllRows, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows_take_key(window, cx) {
            self.with_model(cx, FolderModel::select_all);
        }
    }

    fn extend(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows_take_key(window, cx) {
            self.with_model(cx, |m| m.extend_by(delta));
        }
    }

    /// Test hook: the chosen rows' paths, in display order.
    #[doc(hidden)]
    pub fn chosen_rels_for_test(&self) -> Vec<String> {
        let Some(m) = &self.model else {
            return Vec::new();
        };
        (0..m.visible_len())
            .filter(|&row| m.is_chosen(row))
            .map(|row| m.node_at(row).rel.clone())
            .collect()
    }

    /// Test hook: select a visible row as a click would, without hit-testing.
    pub fn select_row_for_test(&mut self, row: usize, cx: &mut Context<Self>) {
        if let Some(model) = &mut self.model {
            model.select_row(row);
            cx.notify();
        }
    }

    /// Ctrl+R / Ctrl+L and the menu: only while the rows have the focus, so the keys reach a
    /// focused field instead of opening a copy prompt over it. The toolbar calls `copy` itself.
    fn copy_to_right(&mut self, _: &CopyToRight, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows_take_key(window, cx) {
            self.copy(Direction::LeftToRight, window, cx);
        }
    }

    fn copy_to_left(&mut self, _: &CopyToLeft, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows_take_key(window, cx) {
            self.copy(Direction::RightToLeft, window, cx);
        }
    }

    fn delete_selected(&mut self, _: &DeleteSelected, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows_take_key(window, cx) {
            self.delete_selection(DeleteAsk::Choose, window, cx);
        }
    }

    fn reveal_selected(&mut self, _: &RevealSelected, _: &mut Window, cx: &mut Context<Self>) {
        let path = self
            .model
            .as_ref()
            .and_then(FolderModel::selected_node)
            .and_then(|n| {
                n.left
                    .as_ref()
                    .map(|e| join_rel(&self.left, &e.rel))
                    .or_else(|| n.right.as_ref().map(|e| join_rel(&self.right, &e.rel)))
            });
        if let Some(path) = path {
            cx.reveal_path(&path);
        }
    }
}

impl Render for FolderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.drag_tick(window, cx);
        let palette = Palette::current(cx);
        let rows = self.model.as_ref().map_or(0, FolderModel::visible_len);
        let fonts = Fonts::current(cx);
        let empty = self
            .model
            .as_ref()
            .filter(|_| !self.is_busy() && rows == 0)
            .map(|m| {
                let filters = &self.options.filters;
                let masks = !filters.include.is_empty() || !filters.exclude.is_empty();
                empty_state(&m.counts(), m.filter(), m.name_filter().is_some(), masks)
            });
        let sync_panel = self.render_sync_panel(&palette, &fonts, window, cx);
        let export_panel = self.render_export_panel(&palette, &fonts, window, cx);
        export_actions(sync_actions(menu_actions(v_flex(), cx), cx), cx)
            .id("folder-view")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.on_folders_dropped(paths, window, cx)
            }))
            .size_full()
            .bg(palette.bg)
            .text_color(palette.text)
            .text_size(px(crate::ui::fonts::UI_SIZE))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::expand_row))
            .on_action(cx.listener(Self::collapse_row))
            .on_action(cx.listener(Self::open_row))
            .on_action(cx.listener(Self::next_difference))
            .on_action(cx.listener(Self::prev_difference))
            .on_action(cx.listener(Self::expand_all))
            .on_action(cx.listener(Self::collapse_all))
            .on_action(cx.listener(Self::on_refresh))
            .on_action(cx.listener(Self::copy_to_right))
            .on_action(cx.listener(Self::copy_to_left))
            .on_action(cx.listener(Self::delete_selected))
            .on_action(cx.listener(|this, _: &DeleteLeft, w, cx| {
                this.delete_selection(DeleteAsk::Sides(DeleteSides::One(Side::Left)), w, cx)
            }))
            .on_action(cx.listener(|this, _: &DeleteRight, w, cx| {
                this.delete_selection(DeleteAsk::Sides(DeleteSides::One(Side::Right)), w, cx)
            }))
            .on_action(cx.listener(|this, _: &DeleteBoth, w, cx| {
                if this.rows_take_key(w, cx) {
                    this.delete_selection(DeleteAsk::Sides(DeleteSides::Both), w, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TouchRight, w, cx| {
                if this.rows_take_key(w, cx) {
                    this.touch(Side::Left, w, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TouchLeft, w, cx| {
                if this.rows_take_key(w, cx) {
                    this.touch(Side::Right, w, cx)
                }
            }))
            .on_action(cx.listener(Self::reveal_selected))
            .on_action(cx.listener(Self::cancel_job))
            .on_action(cx.listener(Self::select_first))
            .on_action(cx.listener(Self::select_last))
            .on_action(cx.listener(Self::select_all_rows))
            .on_action(cx.listener(Self::focus_filter))
            .on_action(cx.listener(|this, _: &ExtendUp, w, cx| this.extend(-1, w, cx)))
            .on_action(cx.listener(|this, _: &ExtendDown, w, cx| this.extend(1, w, cx)))
            .on_action(cx.listener(|this, _: &ExtendPageUp, w, cx| this.extend(-PAGE_ROWS, w, cx)))
            .on_action(cx.listener(|this, _: &ExtendPageDown, w, cx| this.extend(PAGE_ROWS, w, cx)))
            .on_action(cx.listener(|this, _: &SwapSides, _, cx| this.swap_sides(cx)))
            .on_action(cx.listener(|this, _: &CriterionContent, _, cx| {
                this.set_mode(CompareMode::Content, cx)
            }))
            .on_action(
                cx.listener(|this, _: &CriterionQuick, _, cx| {
                    this.set_mode(CompareMode::Quick, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &CriterionSize, _, cx| this.set_mode(CompareMode::Size, cx)),
            )
            .on_action(
                cx.listener(|this, _: &CriterionRules, _, cx| {
                    this.set_mode(CompareMode::Rules, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ToggleOddScan, _, cx| {
                this.set_odd_scan(!this.prefs.odd_scan, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleConfirmCopy, _, cx| {
                this.set_confirm_copy(!this.prefs.confirm_copy, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleEnterFolders, _, cx| {
                this.set_enter_folders(!this.prefs.enter_folders, cx)
            }))
            .child(self.render_paths(&palette, &fonts, cx))
            .child(self.render_options(&palette, &fonts, cx))
            .children(self.mask_error.clone().map(|error| {
                div()
                    .id("folder-mask-error")
                    .test_support()
                    .px_2()
                    .pb_1()
                    .text_color(palette.del)
                    .child(error)
            }))
            .child(self.render_chips(&palette, cx))
            .child(if self.prefs.layout == Layout::Flat {
                self.render_flat_header(&palette, cx).into_any_element()
            } else {
                self.render_header(&palette).into_any_element()
            })
            .child(
                div()
                    .id("folder-list")
                    .test_support()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    // A right press first forgets the row pressed before (capture runs ahead of
                    // the rows), so blank space below the rows opens no menu.
                    .capture_any_mouse_down(cx.listener(|this, e: &MouseDownEvent, _, _| {
                        if e.button == MouseButton::Right {
                            this.menu_row_pressed = false;
                        }
                    }))
                    .child(
                        uniform_list("folder-rows", rows, cx.processor(Self::render_rows))
                            .track_scroll(&self.scroll)
                            .size_full(),
                    )
                    .when(self.drag.active, |d| {
                        d.child(drag_listeners(cx.entity().downgrade()))
                    })
                    .vertical_scrollbar(&self.scroll)
                    .when_some(empty, |d, (title, hint)| {
                        d.child(
                            v_flex()
                                .id("folder-empty")
                                .test_support()
                                .absolute()
                                .size_full()
                                .bg(palette.bg)
                                .items_center()
                                .justify_center()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .id("folder-empty-title")
                                        .test_support()
                                        .aria_label(title)
                                        .text_size(px(15.))
                                        .text_color(palette.text)
                                        .child(title),
                                )
                                .children(hint.map(|hint| {
                                    div()
                                        .id("folder-empty-hint")
                                        .test_support()
                                        .aria_label(hint)
                                        .text_color(palette.muted)
                                        .child(hint)
                                })),
                        )
                    })
                    .children(sync_panel)
                    .children(export_panel)
                    .context_menu(list_menu(cx.entity().downgrade())),
            )
            .child(self.render_status(&palette, cx))
    }
}
