//! The merge tab (P8): the three files are read and merged off the UI thread. This holds the
//! three slots, the load and merge jobs, the actions and keys; the input stage, the card list,
//! resolving and the toolbar are in the child modules.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};

use duet_core::text::diff::DiffError;
use duet_core::text::merge::diff3::{Region, diff3};
use duet_core::text::merge::session::{MergeSession, Nav};
use duet_core::text::merge::{MAX_MERGE_BYTES, MergeText, ReadError, read_side};
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::v_flex;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, EventEmitter, ExternalPaths,
    FocusHandle, Focusable, KeyBinding, PromptLevel, SharedString, Subscription, Task, Window,
    actions, div, prelude::*, px,
};

use crate::format::count_of;
use crate::i18n::errors::{diff_error, merge_read_error};
use crate::i18n::{LiveText, answers, tr, tr_key, trf};
use crate::job::JobSlot;
use crate::launch::MergePaths;
use crate::palette::Palette;
use crate::settings;
use crate::tabs::badge_text;
use crate::ui::fonts::{Fonts, UI_SIZE};

actions!(
    merge,
    [
        MergeEscape,
        MergeNext,
        MergePrev,
        MergeFirst,
        MergeLast,
        MergeUseLeft,
        MergeUseRight,
        MergeUseLeftRight,
        MergeUseRightLeft,
        MergeUseBase,
        MergeReset,
        MergeUndo,
        MergeRedo,
        MergeSave,
        MergeRun,
        MergeSwap,
        MergeReload,
        MergeOpenLeft,
        MergeOpenRight
    ]
);

mod cards;
mod input;
mod resolve;
mod save;
mod status;
mod toolbar;

pub use cards::{CardText, ColumnText, RowText};
pub use input::{InputMode, SlotText};
pub use status::PREVIEW_DELAY;

pub const CONTEXT: &str = "MergeView";
/// The key context of every multi-line field of the view (PF3): Ctrl+Enter is bound again
/// inside it, as the field would otherwise take it for a newline.
pub const TEXT_CONTEXT: &str = "MergeText";
/// The slots, in the order they are drawn.
pub const LEFT: usize = 0;
pub const BASE: usize = 1;
pub const RIGHT: usize = 2;
pub const CANCELLED: &str = tr_key("Cancelled: the result is incomplete.");
/// Merge ▸ over choices that are not saved asks first (`js:209-212`).
const MERGE_AGAIN: &str = tr_key("Merging again drops the conflict choices you have not saved.");
const MERGE_AGAIN_ANSWERS: [&str; 2] = [tr_key("Cancel"), tr_key("Merge again")];
/// A file that lands over such choices only says so (`maybeAutoRun`).
const FILES_CHOSEN: &str = tr_key("Files chosen: press Merge to merge again.");
/// A choice or typing while a merge or a reload replaces the session would be lost (I3).
const WAIT_MERGE: &str = tr_key("Merging…: wait for the new result.");
/// A launch of an open set with another output (M8).
const OUTPUT_KEPT: &str = tr_key("This merge is open already; its output is unchanged.");
/// Why a merge did not take the key of its set: another tab already shows it.
pub const MERGED_ELSEWHERE: &str = tr_key("These files are already merged in another tab.");
/// The setting behind the "ignore trailing whitespace" option (R3).
pub(super) const TRAIL_SETTING: &str = "merge.trail";
/// Lower-case slot names for the notices.
const SLOT_NAMES: [&str; 3] = [tr_key("left"), tr_key("base"), tr_key("right")];

pub fn key_bindings() -> Vec<KeyBinding> {
    let c = Some(CONTEXT);
    vec![
        KeyBinding::new("escape", MergeEscape, c),
        KeyBinding::new("f8", MergeNext, c),
        KeyBinding::new("f7", MergePrev, c),
        KeyBinding::new("ctrl-home", MergeFirst, c),
        KeyBinding::new("ctrl-end", MergeLast, c),
        KeyBinding::new("alt-left", MergeUseLeft, c),
        KeyBinding::new("alt-right", MergeUseRight, c),
        KeyBinding::new("ctrl-z", MergeUndo, c),
        KeyBinding::new("ctrl-y", MergeRedo, c),
        KeyBinding::new("ctrl-shift-z", MergeRedo, c),
        KeyBinding::new("ctrl-s", MergeSave, c),
        KeyBinding::new("ctrl-enter", MergeRun, c),
        // A multi-line field takes Ctrl+Enter for a newline; inside one of ours it runs the
        // merge (PF3).
        KeyBinding::new("ctrl-enter", MergeRun, Some("MergeText > Input")),
        KeyBinding::new("f9", MergeSwap, c),
        KeyBinding::new("f5", MergeReload, c),
        KeyBinding::new("ctrl-o", MergeOpenLeft, c),
        KeyBinding::new("ctrl-shift-o", MergeOpenRight, c),
    ]
}

#[derive(Debug, Clone)]
pub enum MergeEvent {
    /// A merge ran; the paths are the session's, None for a slot with no file (R12).
    Merged {
        left: Option<PathBuf>,
        base: Option<PathBuf>,
        right: Option<PathBuf>,
    },
    /// F9 traded the session's sides; these are its paths now. The workspace may take the
    /// swap back with `undo_swap` (M6).
    Swapped {
        left: Option<PathBuf>,
        base: Option<PathBuf>,
        right: Option<PathBuf>,
    },
}

/// What a merge job was started with: the option as it stood, and whether the merge leaves
/// the notice alone.
#[derive(Debug, Clone, Copy)]
struct JobOptions {
    trail: bool,
    quiet: bool,
}

/// Whether a tab other than this one merges the set of these three files (the workspace's
/// answer; a slot of no file never matches).
pub type ElsewhereCheck = Box<dyn Fn([Option<&Path>; 3], &App) -> bool>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stage {
    #[default]
    Input,
    Merge,
}

pub struct MergeView {
    /// For the job results that need the window when they land.
    window: AnyWindowHandle,
    slots: [Option<Arc<MergeText>>; 3],
    /// Why a slot could not be read, and the file it could not read, so F5 tries it again.
    messages: [Option<LiveText>; 3],
    failed: [Option<PathBuf>; 3],
    /// One load per slot, and the id of the load each slot waits for (0 for none). A result
    /// lands on the slot that holds its id, so a pending load moves with its slot (R13).
    loads: [JobSlot; 3],
    load_ids: [u64; 3],
    load_tasks: [Option<Task<()>>; 3],
    next_load_id: u64,
    /// The merge itself; a newer run supersedes an older one.
    merge_job: JobSlot,
    merge_task: Option<Task<()>>,
    session: Option<MergeSession>,
    /// The merge stage's list of cards for that session.
    cards: cards::Cards,
    stage: Stage,
    notice: Option<SharedString>,
    output: Option<PathBuf>,
    mode: InputMode,
    /// The paste fields (files mode keeps them hidden) and whether the user changed each: an
    /// edited slot merges the field's text instead of its file's.
    pastes: [Entity<TextareaState>; 3],
    edited: [bool; 3],
    /// The file name a slot is reading or failed to read (it has no text to name it by).
    names: [SharedString; 3],
    output_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
    /// Set by Reload and Swap once the user agreed to discard choices: the loads they start
    /// then merge even over unsaved choices (PF8).
    force_next_run: bool,
    /// Set with `force_next_run` by Reload: that merge leaves the notice alone (`quiet`).
    quiet_next_run: bool,
    /// While a Reload's reads run: how many have read their file so far (`js:685`).
    reloaded: Option<usize>,
    /// "Ignore trailing whitespace" (R3), read from the setting when the view is made.
    trail: bool,
    /// The whole-result preview: whether it shows, what it holds, and the timer that
    /// rebuilds it 120 ms after the last change (R15).
    show_result: bool,
    preview: Option<status::Preview>,
    preview_task: Option<Task<()>>,
    /// UTF-8 was agreed for a result its output encoding cannot hold (R8, set by Save).
    utf8_agreed: bool,
    /// Set by the workspace: whether another tab merges a set (M-68).
    elsewhere: Option<ElsewhereCheck>,
    focus: FocusHandle,
}

impl EventEmitter<MergeEvent> for MergeView {}

impl Focusable for MergeView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// What a slot says when its file could not be used.
fn read_message(name: &str, err: &ReadError) -> LiveText {
    let name = name.to_owned();
    match err {
        ReadError::Binary => LiveText::new(move || {
            trf(
                "{} is a binary file: only text files can be merged.",
                &[&name],
            )
        }),
        err => {
            let err = err.clone();
            LiveText::new(move || trf("Can't open {}: {}", &[&name, &merge_read_error(&err)]))
        }
    }
}

impl MergeView {
    /// Loads the given slots; the merge runs once all three have landed.
    pub fn new(paths: MergePaths, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let MergePaths {
            left,
            base,
            right,
            output,
        } = paths;
        let mut view = Self::blank(window, cx);
        view.set_output(output, window, cx);
        for (slot, path) in [(LEFT, left), (BASE, base), (RIGHT, right)] {
            if let Some(path) = path {
                view.start_load(slot, path, cx);
            }
        }
        view
    }

    /// Three typed slots (no files) holding these texts, merged at once: tests set exact
    /// encodings this way.
    #[doc(hidden)]
    pub fn with_texts_for_test(
        texts: [MergeText; 3],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::blank(window, cx);
        for (slot, text) in texts.into_iter().enumerate() {
            view.slots[slot] = Some(Arc::new(text));
        }
        view.run(false, window, cx);
        view
    }

    fn blank(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (pastes, output_input, _subscriptions) = Self::make_inputs(window, cx);
        Self {
            window: window.window_handle(),
            slots: [None, None, None],
            messages: [None, None, None],
            failed: [None, None, None],
            loads: [JobSlot::default(), JobSlot::default(), JobSlot::default()],
            load_ids: [0; 3],
            load_tasks: [None, None, None],
            next_load_id: 0,
            merge_job: JobSlot::default(),
            merge_task: None,
            session: None,
            cards: cards::Cards::new(),
            stage: Stage::default(),
            notice: None,
            output: None,
            mode: InputMode::default(),
            pastes,
            edited: [false; 3],
            names: [
                SharedString::default(),
                SharedString::default(),
                SharedString::default(),
            ],
            output_input,
            _subscriptions,
            force_next_run: false,
            quiet_next_run: false,
            reloaded: None,
            trail: settings::settings_file(cx)
                .and_then(|file| settings::get_bool(&file, TRAIL_SETTING))
                .unwrap_or(false),
            show_result: false,
            preview: None,
            preview_task: None,
            utf8_agreed: false,
            elsewhere: None,
            // A tab stop, so Tab comes back to the view after a button or field.
            focus: cx.focus_handle().tab_stop(true),
        }
    }

    /// A slot being read, or a merge running.
    pub fn is_busy(&self) -> bool {
        self.loads_running() || self.merge_job.is_running()
    }

    fn loads_running(&self) -> bool {
        self.loads.iter().any(JobSlot::is_running)
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// The slot's loaded file.
    pub fn slot(&self, slot: usize) -> Option<&Arc<MergeText>> {
        self.slots.get(slot)?.as_ref()
    }

    pub fn loading(&self, slot: usize) -> bool {
        self.loads.get(slot).is_some_and(JobSlot::is_running)
    }

    /// Why that slot could not be read.
    pub fn slot_message(&self, slot: usize) -> Option<&str> {
        self.messages.get(slot)?.as_deref()
    }

    pub fn session(&self) -> Option<&MergeSession> {
        self.session.as_ref()
    }

    /// The session holds choices that are not saved.
    pub fn is_dirty(&self) -> bool {
        self.session.as_ref().is_some_and(MergeSession::is_dirty)
    }

    /// Nothing chosen, typed, read or merged yet: the Start card may bring this tab back
    /// instead of opening another (C1).
    pub fn is_untouched(&self) -> bool {
        self.session.is_none()
            && !self.is_busy()
            && self.output.is_none()
            && (LEFT..=RIGHT).all(|slot| {
                self.slots[slot].is_none() && !self.edited[slot] && self.messages[slot].is_none()
            })
    }

    /// A merge, or the reads of a Reload, will replace the session: a choice or typing made
    /// now would be dropped when it lands (I3).
    fn replacing(&self) -> bool {
        self.merge_job.is_running() || (self.force_next_run && self.loads_running())
    }

    /// Refuses an edit while the session is being replaced, with the notice; true when the
    /// edit may go ahead.
    pub(super) fn may_edit(&mut self, cx: &mut Context<Self>) -> bool {
        if self.replacing() {
            self.notice = Some(tr(WAIT_MERGE).into());
            cx.notify();
            return false;
        }
        true
    }

    /// The language changed. A notice is one-shot feedback in the old language, so it goes and the
    /// status falls back to what it works out. What is stored is built again: a failed slot's
    /// message, each card's text (its height can change, so the list measures it again where it
    /// stands) and every placeholder. Nothing the person made is touched: the typed fields, the
    /// choices and the scroll place stay.
    pub fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.notice = None;
        for message in self.messages.iter_mut().flatten() {
            message.relabel();
        }
        self.cards.relabel();
        self.relabel_inputs(window, cx);
        self.relabel_result_fields(window, cx);
        cx.notify();
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn show_notice(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.notice = Some(text.into());
        cx.notify();
    }

    pub fn output(&self) -> Option<&Path> {
        self.output.as_deref()
    }

    /// Sets the output path and shows it in the output box.
    pub fn set_output(
        &mut self,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        self.output_input
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.output = path;
        cx.notify();
    }

    /// Another launch of this set names an output (M8): a tab with none takes it, else the
    /// notice says the output stays as it is.
    pub fn offer_output(&mut self, output: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        match self.output.as_deref() {
            None => self.set_output(Some(output), window, cx),
            Some(own) if own == output => {}
            Some(_) => self.show_notice(tr(OUTPUT_KEPT), cx),
        }
    }

    /// The tab's badge (R12): none before a merge or while busy, else the conflicts still
    /// unresolved, "ok" at none.
    pub fn badge(&self) -> Option<String> {
        if self.is_busy() {
            return None;
        }
        self.session.as_ref().map(|s| badge_text(s.unresolved()))
    }

    /// Reads a file into a slot. A file still being read for that slot is dropped; if this one
    /// cannot be read, the slot is left empty (R13).
    pub fn open_slot_path(&mut self, slot: usize, path: PathBuf, cx: &mut Context<Self>) {
        if slot < self.slots.len() {
            self.start_load(slot, path, cx);
        }
    }

    fn start_load(&mut self, slot: usize, path: PathBuf, cx: &mut Context<Self>) {
        // A merge of the old files is not wanted any more.
        self.merge_job.cancel();
        self.merge_task = None;
        let (generation, cancel) = self.loads[slot].start();
        self.next_load_id += 1;
        let id = self.next_load_id;
        self.load_ids[slot] = id;
        self.messages[slot] = None;
        self.failed[slot] = None;
        self.names[slot] = file_name(&path).into();
        let window = self.window;
        let background = cx.background_executor().clone();
        self.load_tasks[slot] = Some(cx.spawn(async move |this, cx| {
            let outcome = {
                let path = path.clone();
                background
                    .spawn(async move { read_side(&path, MAX_MERGE_BYTES, &cancel) })
                    .await
            };
            let Some(view) = this.upgrade() else {
                return;
            };
            let _ = cx.update_window(window, |_, window, cx| {
                view.update(cx, |view, cx| {
                    view.loaded(id, generation, &path, outcome, window, cx)
                })
            });
        }));
        cx.notify();
    }

    /// Load `id` finished. It lands on the slot that waits for it, if that is still so.
    fn loaded(
        &mut self,
        id: u64,
        generation: u64,
        path: &Path,
        outcome: Result<MergeText, ReadError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.load_ids.iter().position(|&waiting| waiting == id) else {
            return;
        };
        if !self.loads[slot].is_current(generation) {
            return;
        }
        self.loads[slot].finish(generation);
        self.load_ids[slot] = 0;
        match outcome {
            Ok(text) => {
                self.slots[slot] = Some(Arc::new(text));
                self.messages[slot] = None;
                self.edited[slot] = false;
                self.fill_field(slot, window, cx);
                if let Some(read) = self.reloaded.as_mut() {
                    *read += 1;
                }
            }
            Err(ReadError::Cancelled) => {}
            Err(err) => {
                self.slots[slot] = None;
                self.messages[slot] = Some(read_message(&file_name(path), &err));
                self.failed[slot] = Some(path.to_path_buf());
                self.edited[slot] = false;
                self.fill_field(slot, window, cx);
            }
        }
        cx.notify();
        // A Reload counts the files it read, once all are in (`js:685`). A merge that cannot
        // run after it says why over that.
        if !self.loads_running()
            && let Some(read) = self.reloaded.take()
            && read > 0
        {
            self.notice =
                Some(trf("Reloaded {}.", &[&count_of(read as u64, "file", "files")]).into());
        }
        // Files mode merges on its own once all three are in (LocalDiff `maybeAutoRun`), but
        // never over choices that are not saved, unless Reload or Swap was agreed to (PF8).
        // A Reload merges once its loads are in, whatever the mode (`run({force})`).
        let wanted = self.force_next_run
            || (self.mode == InputMode::Files && (LEFT..=RIGHT).all(|slot| self.filled(slot)));
        if wanted && !self.loads_running() {
            let force = std::mem::take(&mut self.force_next_run);
            let quiet = std::mem::take(&mut self.quiet_next_run);
            if !force && self.is_dirty() {
                self.notice = Some(tr(FILES_CHOSEN).into());
            } else {
                self.run_with(force, quiet, window, cx);
            }
        }
    }

    /// How the view asks whether another tab merges a set (the workspace sets it).
    pub fn set_elsewhere_check(&mut self, check: ElsewhereCheck) {
        self.elsewhere = Some(check);
    }

    /// Another tab merges the set of these texts' files.
    fn merged_elsewhere(&self, texts: &[Arc<MergeText>; 3], cx: &App) -> bool {
        let set = texts.each_ref().map(|text| text.path.as_deref());
        self.elsewhere.as_ref().is_some_and(|check| check(set, cx))
    }

    /// Merge ▸: diff3 over the three slots in the background. A newer run supersedes this
    /// one. Over choices that are not saved it asks first, unless `force`.
    pub fn run(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.run_with(force, false, window, cx);
    }

    /// `run`; a `quiet` merge leaves the notice as it is (Reload says its own).
    fn run_with(&mut self, force: bool, quiet: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(texts) = self.gather(cx) else {
            return;
        };
        if !force && self.is_dirty() {
            // The set another tab merges is refused at once, as a swap onto it is: the
            // choices stay, and the tab keeps the key of what it shows (M-68).
            if self.merged_elsewhere(&texts, cx) {
                self.notice = Some(tr(MERGED_ELSEWHERE).into());
                cx.notify();
                return;
            }
            let answer = window.prompt(
                PromptLevel::Warning,
                tr(MERGE_AGAIN),
                None,
                &answers(&MERGE_AGAIN_ANSWERS),
                cx,
            );
            cx.spawn_in(window, async move |this, cx| {
                if answer.await == Ok(1) {
                    let _ = this.update_in(cx, |view, window, cx| view.run(true, window, cx));
                }
            })
            .detach();
            return;
        }
        // A box or field with the keys goes with the stage or the session (LocalDiff ends
        // the edit and hides the input stage).
        self.leave_input(window, cx);
        self.start_merge(texts, quiet, cx);
    }

    /// The diff3 job over these texts, with the option as it stands.
    fn start_merge(&mut self, texts: [Arc<MergeText>; 3], quiet: bool, cx: &mut Context<Self>) {
        let trail = self.trail;
        let (generation, cancel) = self.merge_job.start();
        let started = Instant::now();
        let window = self.window;
        let background = cx.background_executor().clone();
        self.merge_task = Some(cx.spawn(async move |this, cx| {
            let outcome = {
                let [left, base, right] = texts.clone();
                background
                    .spawn(async move { diff3(&left, &base, &right, trail, &cancel) })
                    .await
            };
            let Some(view) = this.upgrade() else {
                return;
            };
            let _ = cx.update_window(window, |_, window, cx| {
                view.update(cx, |view, cx| {
                    view.merged(
                        generation,
                        texts,
                        JobOptions { trail, quiet },
                        started,
                        outcome,
                        window,
                        cx,
                    )
                })
            });
        }));
        cx.notify();
    }

    /// A finished merge replaces the session, unless a newer run superseded it. `started` is
    /// when its job started, for the bench hook.
    #[allow(clippy::too_many_arguments)]
    fn merged(
        &mut self,
        generation: u64,
        texts: [Arc<MergeText>; 3],
        JobOptions { trail, quiet }: JobOptions,
        started: Instant,
        outcome: Result<Vec<Region>, DiffError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.merge_job.is_current(generation) {
            return;
        }
        self.merge_job.finish(generation);
        let regions = match outcome {
            Ok(regions) => regions,
            Err(DiffError::Cancelled(_)) => {
                cx.notify();
                return;
            }
            Err(err) => {
                self.notice = Some(trf("Can't merge: {}.", &[&diff_error(&err)]).into());
                cx.notify();
                return;
            }
        };
        let paths = texts.clone().map(|text| text.path.clone());
        let [left, base, right] = texts;
        let mut session = MergeSession::new(left, base, right, regions, trail);
        let conflicts = session.conflicts().len();
        let auto = count_of(session.automatic() as u64, "change", "changes");
        let notice: SharedString = if session.changes().is_empty() {
            tr("The three files are the same: nothing to merge.").to_owned()
        } else if conflicts > 0 {
            let needs = count_of(conflicts as u64, "conflict needs", "conflicts need");
            trf("Merged {} automatically; {} a choice.", &[&auto, &needs])
        } else {
            trf("Merged {} automatically: no conflicts.", &[&auto])
        }
        .into();
        if !quiet {
            self.notice = Some(notice);
        }
        // The first conflict comes into view (LocalDiff `js:224`). With no card above it, it
        // is on the first screen already (a folded stable run is at most seven items), so the
        // list stays at its top, as LocalDiff's scroll does near the top (`js:613`). With no
        // conflicts, the first change is current where it is (`js:225`).
        let jump = if conflicts > 0 {
            session
                .nav(Nav::First)
                .filter(|&chunk| session.changes().first() != Some(&chunk))
        } else {
            if let Some(&first) = session.changes().first() {
                session.set_current(first);
            }
            None
        };
        // A field the keys went to while the job ran goes with the old session (I3).
        self.leave_input(window, cx);
        self.session = Some(session);
        self.utf8_agreed = false;
        self.stage = Stage::Merge;
        self.rebuild_cards();
        self.refresh_preview(cx);
        if let Some(chunk) = jump {
            self.scroll_to_chunk(chunk);
        }
        self.time_first_merge(started);
        let [left, base, right] = paths;
        cx.emit(MergeEvent::Merged { left, base, right });
        cx.notify();
    }

    /// Stops every load and the merge. The notice says the result is incomplete.
    fn cancel_work(&mut self, cx: &mut Context<Self>) {
        for slot in LEFT..=RIGHT {
            self.loads[slot].cancel();
            self.load_ids[slot] = 0;
            self.load_tasks[slot] = None;
        }
        self.merge_job.cancel();
        self.merge_task = None;
        self.force_next_run = false;
        self.quiet_next_run = false;
        self.reloaded = None;
        self.notice = Some(tr(CANCELLED).into());
        cx.notify();
    }

    /// Esc leaves the output box, a paste field or a result field first (which ends a typing
    /// session), else cancels running jobs, else does nothing: a merge tab has no folder tab
    /// to go back to (R7).
    fn on_escape(&mut self, _: &MergeEscape, window: &mut Window, cx: &mut Context<Self>) {
        if self.leave_input(window, cx) {
            return;
        }
        if self.is_busy() {
            self.cancel_work(cx);
        }
    }

    /// The merge stage's body without a session: why a slot is empty, or what is running.
    fn body_text(&self) -> String {
        if let Some(message) = self.messages.iter().flatten().next() {
            return message.to_string();
        }
        if self.loads_running() {
            return tr("Reading…").to_owned();
        }
        if self.merge_job.is_running() {
            return tr("Merging…").to_owned();
        }
        tr("Choose the three files to merge.").to_owned()
    }
}

impl Render for MergeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::current(cx);
        let fonts = Fonts::current(cx);
        let root = v_flex()
            .id("merge-view")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .bg(palette.bg)
            .text_color(palette.text)
            .text_size(px(UI_SIZE))
            .on_action(cx.listener(Self::on_escape))
            .on_action(cx.listener(Self::on_next))
            .on_action(cx.listener(Self::on_prev))
            .on_action(cx.listener(Self::on_first))
            .on_action(cx.listener(Self::on_last))
            .on_action(cx.listener(Self::on_use_left))
            .on_action(cx.listener(Self::on_use_right))
            .on_action(cx.listener(Self::on_use_left_right))
            .on_action(cx.listener(Self::on_use_right_left))
            .on_action(cx.listener(Self::on_use_base))
            .on_action(cx.listener(Self::on_reset))
            .on_action(cx.listener(Self::on_undo))
            .on_action(cx.listener(Self::on_redo))
            .on_action(cx.listener(Self::on_run))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_swap))
            .on_action(cx.listener(Self::on_reload))
            .on_action(cx.listener(Self::on_open_left))
            .on_action(cx.listener(Self::on_open_right))
            .on_drop(cx.listener(|this, dropped: &ExternalPaths, window, cx| {
                this.drop_paths(dropped.paths(), None, window, cx)
            }))
            .child(self.render_toolbar(&palette, &fonts, cx));
        match self.stage {
            Stage::Input => self.render_input_stage(root, &palette, cx),
            Stage::Merge => self.render_merge_stage(root, &palette, window, cx),
        }
    }
}
