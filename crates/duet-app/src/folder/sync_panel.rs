//! The sync panel (R14–R17): five modes, the scope, a preview worked out on the background
//! executor, and Start. Start checks the roots and every destination off the UI thread, asks
//! before it replaces any file (PF1) and before anything goes to the Recycle Bin ("Cancel"
//! first in both), checks that the compare is still the one the plan was made from, closes the
//! panel and runs the sync as a batch. Inside the batch the deletes are planned again; they
//! may only shrink to what the question named.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use duet_core::cancel::{CancelToken, Cancelled};
use duet_core::folder::actions::{ActionError, Conflict, roots_overlap};
use duet_core::folder::entry::EntryKind;
use duet_core::folder::plan::{BatchReport, TrashPlan};
use duet_core::folder::run::CompareMode;
use duet_core::folder::sync::{SkipReason, SyncMode, SyncPlan, SyncReport, rel_under, run_sync};
use duet_core::folder::tree::{Side, Status};
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{
    AnyElement, BackgroundExecutor, Context, FocusHandle, FontWeight, InteractiveElement,
    PromptLevel, SharedString, Task, Window, actions, div, prelude::*, px,
};

use crate::folder::batch::{
    Snapshot, TrashFn, check, overwrite_lines, trash_prompt, within_prompt,
};
use crate::folder::view::FolderView;
use crate::format::{count_of, format_size};
use crate::job::JobSlot;
use crate::palette::Palette;
use crate::ui::fonts::Fonts;
use crate::ui::style::{LdExt as _, ld_button, ld_card, ld_primary, ld_segment, ld_segmented};

/// Which rows a sync covers (R15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncScope {
    /// Every row, whatever the chips and the path filter show.
    All,
    /// The selection under the filters (P4 R2).
    Selected,
}

actions!(folder, [OpenSync, SyncSelected, SyncConfirm]);

/// The panel's key context, where Enter is `SyncConfirm`.
pub const CONTEXT: &str = "FolderSync";

/// The answers of Start's two questions. "Cancel" is first, so Enter never replaces or
/// deletes anything.
pub const REPLACE_ANSWERS: [&str; 2] = ["Cancel", "Replace"];
pub const TRASH_ANSWERS: [&str; 2] = ["Cancel", "Move to Recycle Bin"];

const COMPARED_AGAIN: &str = "The folders were compared again, so the sync was closed.";
const STALE: &str =
    "The folders changed while the question was open. Sync again to use the new view.";
const NESTED: &str =
    "One folder is inside the other, so sync is off: it could overwrite or delete its own source.";
const HELP: &str = "Deleted files go to the Recycle Bin, where they can be restored. Copies keep their modified times. The current include and exclude masks apply.";

/// Preview lines of each kind before "…" (LocalDiff `js:1115-1119`). The files a sync replaces
/// get `COPY_LINES` of their own, ahead of the plain copies.
const COPY_LINES: usize = 400;
const TRASH_LINES: usize = 200;
const SKIP_LINES: usize = 100;

/// Replaced files Start's question names per side before "…and N more" (R20's cap; a copy's
/// question names six).
const REPLACED_LISTED: usize = 200;

/// Each mode's card: its id, title and what it does. A mirror leaves "≈" files and pairs the
/// text rules call the same as they are (PF18).
const MODES: [(SyncMode, &str, &str, &str); 5] = [
    (
        SyncMode::UpdateRight,
        "folder-sync-mode-update-right",
        "Update right",
        "Copies new files, and files newer on the left, to the right.",
    ),
    (
        SyncMode::UpdateLeft,
        "folder-sync-mode-update-left",
        "Update left",
        "Copies new files, and files newer on the right, to the left.",
    ),
    (
        SyncMode::UpdateBoth,
        "folder-sync-mode-update-both",
        "Update both",
        "The newer file wins, and files missing on either side are filled in.",
    ),
    (
        SyncMode::MirrorRight,
        "folder-sync-mode-mirror-right",
        "Mirror to right",
        "Makes the right match the left (files that differ only in line endings or unimportant parts are left alone); extra files on the right go to the Recycle Bin.",
    ),
    (
        SyncMode::MirrorLeft,
        "folder-sync-mode-mirror-left",
        "Mirror to left",
        "Makes the left match the right (files that differ only in line endings or unimportant parts are left alone); extra files on the left go to the Recycle Bin.",
    ),
];

/// The copy lines of both ways: the files they replace (`replaces`), or the new files and
/// folders.
fn copy_lines<'a>(
    plan: &'a SyncPlan,
    left_root: &'a Path,
    right_root: &'a Path,
    replaces: bool,
) -> impl Iterator<Item = String> + 'a {
    let ways = [
        ("→", right_root, &plan.to_right),
        ("←", left_root, &plan.to_left),
    ];
    ways.into_iter().flat_map(move |(arrow, root, batch)| {
        let files = batch
            .files
            .iter()
            .filter(move |f| f.overwrites == replaces)
            .map(move |f| {
                let rel = rel_under(root, &f.dst);
                if replaces {
                    format!("{arrow} {rel} (replaces)")
                } else {
                    format!("{arrow} {rel}")
                }
            });
        let folders = batch
            .mkdirs
            .iter()
            .filter(move |_| !replaces)
            .map(move |d| format!("{arrow} {}/ (new folder)", rel_under(root, d)));
        files.chain(folders)
    })
}

/// The preview as drawn: the files a copy replaces first (an overwrite cannot be undone),
/// then the new files and folders, Recycle Bin lines and skips, each capped (R17), then "…"
/// when more were cut. `mode` is the compare's criterion, which the skip reasons follow.
pub fn sync_lines(
    plan: &SyncPlan,
    left_root: &Path,
    right_root: &Path,
    mode: CompareMode,
) -> Vec<String> {
    let replaced = copy_lines(plan, left_root, right_root, true);
    let added = copy_lines(plan, left_root, right_root, false);
    let trash_root = match plan.trash_side {
        Some(Side::Left) => left_root,
        _ => right_root,
    };
    let trash = plan.trash.iter().map(|path| {
        let folder = plan
            .seen
            .get(path)
            .is_some_and(|s| s.kind == EntryKind::Dir);
        let slash = if folder { "/" } else { "" };
        format!("✕ {}{slash}", rel_under(trash_root, path))
    });
    let skips = plan
        .skips
        .iter()
        .map(|s| format!("· {} — {}", s.rel, s.why.text_in(mode)));
    let mut lines: Vec<String> = replaced
        .take(COPY_LINES)
        .chain(added.take(COPY_LINES))
        .chain(trash.take(TRASH_LINES))
        .chain(skips.take(SKIP_LINES))
        .collect();
    let overwrites = plan.overwrites();
    let cut = overwrites > COPY_LINES
        || plan.files() - overwrites + plan.folders() > COPY_LINES
        || plan.trash.len() > TRASH_LINES
        || plan.skips.len() > SKIP_LINES;
    if cut {
        lines.push("…".into());
    }
    lines
}

/// The counts above the preview. A plan with nothing to do and nothing skipped says so.
pub fn sync_summary(plan: &SyncPlan) -> String {
    if plan.is_empty() && plan.skips.is_empty() {
        return "Nothing to do: the two sides already match for this mode.".into();
    }
    let replaces = match plan.overwrites() {
        0 => String::new(),
        1 => "; 1 replaces an existing file".into(),
        n => format!("; {} replace existing files", format_size(n as u64)),
    };
    let mut text = format!(
        "Copy {} ({}{replaces})",
        count_of(plan.files() as u64, "file", "files"),
        count_of(plan.bytes(), "byte", "bytes")
    );
    if plan.folders() > 0 {
        let folders = count_of(plan.folders() as u64, "folder", "folders");
        text.push_str(&format!(" · Create {folders}"));
    }
    let moves = count_of(plan.trash.len() as u64, "item", "items");
    text.push_str(&format!(" · Move {moves} to the Recycle Bin"));
    if !plan.skips.is_empty() {
        text.push_str(&format!(" · Skip {}", format_size(plan.skips.len() as u64)));
    }
    text
}

/// "a", "a and b", "a, b and c".
fn join_list(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The notice a finished sync leaves behind; `plan` is the one that ran.
pub fn sync_result(report: &SyncReport, plan: &SyncPlan) -> String {
    let moved = report.trash.as_ref().map_or(0, |t| t.done);
    if report.cancelled() {
        let tried: usize = [&report.to_right, &report.to_left]
            .into_iter()
            .chain(report.trash.as_ref())
            .map(|r| r.done + r.failed.len())
            .sum();
        let total = plan.files() + plan.folders() + plan.trash.len();
        let mut text = format!(
            "Sync cancelled after {} of {}.",
            format_size(tried as u64),
            count_of(total as u64, "item", "items")
        );
        if !plan.trash.is_empty() && moved == 0 {
            text.push_str(" Nothing went to the Recycle Bin.");
        }
        return text;
    }
    let folders = report.to_right.folders + report.to_left.folders;
    let files = report.to_right.done.saturating_sub(report.to_right.folders)
        + report.to_left.done.saturating_sub(report.to_left.folders);
    // A Recycle Bin step: the run's plan may move nothing while what the question named
    // failed there (F-100).
    let to_bin = !plan.trash.is_empty() || report.trash.is_some();
    let mut parts = Vec::new();
    if plan.files() > 0 || (plan.folders() == 0 && !to_bin) {
        parts.push(format!(
            "copied {}",
            count_of(files as u64, "file", "files")
        ));
    }
    if plan.folders() > 0 {
        let made = count_of(folders as u64, "folder", "folders");
        parts.push(format!("created {made}"));
    }
    if to_bin {
        let items = count_of(moved as u64, "item", "items");
        parts.push(format!("moved {items} to the Recycle Bin"));
    }
    let head = format!("Synced: {}", join_list(&parts));
    let failed = report.failed();
    match failed.first() {
        None => format!("{head}."),
        Some((path, err)) => format!(
            "{head}; {} failed: {}: {}.",
            format_size(failed.len() as u64),
            path.display(),
            err.trim_end_matches('.')
        ),
    }
}

/// PF1: the question before a sync replaces any file, as P4's copy asks it. Per side, how
/// many files it replaces, how many of them are newer than the copy (`newer`, read off the
/// disk at Start), and up to `REPLACED_LISTED` paths, the newer ones first: an overwrite is
/// the one thing a sync cannot undo, so the question names them all but for a huge plan.
pub fn sync_overwrite_prompt(plan: &SyncPlan, newer: &HashSet<PathBuf>) -> (String, String) {
    let message = format!(
        "Replace {}?",
        count_of(plan.overwrites() as u64, "file", "files")
    );
    let detail = [(&plan.to_right, "right"), (&plan.to_left, "left")]
        .into_iter()
        .filter_map(|(batch, side)| {
            let conflicts: Vec<Conflict> = batch
                .files
                .iter()
                .filter(|f| f.overwrites)
                .map(|f| Conflict {
                    path: f.dst.clone(),
                    destination_newer: newer.contains(&f.dst),
                })
                .collect();
            (!conflicts.is_empty())
                .then(|| overwrite_lines(conflicts.len(), &conflicts, side, REPLACED_LISTED))
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    (message, detail)
}

/// The Recycle Bin question: how many items on which side, the first few paths, and P4's
/// note on the folders that stay.
pub fn sync_trash_prompt(plan: &SyncPlan) -> (String, String) {
    let side = plan.trash_side.unwrap_or(Side::Right);
    let name = if side == Side::Left { "left" } else { "right" };
    let message = format!(
        "Move {} on the {name} to the Recycle Bin?",
        count_of(plan.trash.len() as u64, "item", "items")
    );
    let kept_folders = plan
        .skips
        .iter()
        .filter(|s| s.why == SkipReason::KeptFolder)
        .count();
    let listed = TrashPlan {
        paths: plan.trash.clone(),
        kept_folders,
        ..TrashPlan::default()
    };
    let (_, detail) = trash_prompt(&[(side, listed)]);
    (message, detail)
}

/// A plan on show, what it was made from and how it reads.
struct Shown {
    plan: Arc<SyncPlan>,
    snap: Arc<Snapshot>,
    summary: SharedString,
    lines: Vec<String>,
}

/// The open panel.
struct SyncPanel {
    mode: SyncMode,
    scope: SyncScope,
    /// None while the plan of `mode` and `scope` is worked out.
    shown: Option<Shown>,
    job: JobSlot,
    task: Option<Task<()>>,
    focus: FocusHandle,
    /// Which opening of the panel this is: an answer to a question it asked runs nothing once
    /// it is closed.
    id: u64,
    /// `plan_epoch` when the panel opened; any compare since makes its plans stale.
    epoch: u64,
    /// How many rows the selection picks, worked out when the panel opens and when the
    /// selection changes, never per frame (the label of "Selected items only").
    picked: usize,
}

/// The folder view's sync state.
#[derive(Default)]
pub(super) struct SyncState {
    panel: Option<SyncPanel>,
    opened: u64,
    /// The focus of a panel closed without a window at hand: the next frame gives the rows
    /// the focus if the panel held it.
    closed_focus: Option<FocusHandle>,
    /// Whether one root is inside the other, worked out beside the last compare, off the UI
    /// thread (resolving a path on a dropped share stalls).
    nested: bool,
    /// Whether the rows on show are the last compare started, finished with no Pending row (a
    /// refresh that Esc stopped or that failed leaves the previous result on show).
    live: bool,
}

/// What a Start the user said yes to runs.
struct SyncRun {
    id: u64,
    epoch: u64,
    /// The plan the questions were about.
    asked: Arc<SyncPlan>,
    snap: Arc<Snapshot>,
    mode: SyncMode,
    left: PathBuf,
    right: PathBuf,
    trash: TrashFn,
}

/// Inside the batch: the deletes are planned again, as the disk may have moved on while the
/// question was open. None (nothing runs) unless they stay within what the question named and
/// the copies are the ones it counted. Esc during that walk stops it: nothing runs and the
/// result counts the question's plan.
fn replan_and_run(
    run: &SyncRun,
    cancel: &CancelToken,
    done: &AtomicUsize,
) -> Option<(SyncReport, Arc<SyncPlan>)> {
    let asked = &run.asked;
    let plan = if asked.trash.is_empty() {
        asked.clone()
    } else {
        match run.snap.sync_plan_with_cancel(run.mode, cancel) {
            Ok(mut plan) => {
                plan.leave_folders_grown_since(asked, &run.left, &run.right);
                Arc::new(plan)
            }
            Err(Cancelled) => {
                let stopped = BatchReport {
                    cancelled: true,
                    ..BatchReport::default()
                };
                let report = SyncReport {
                    to_right: stopped,
                    ..SyncReport::default()
                };
                return Some((report, asked.clone()));
            }
        }
    };
    let same_copies = plan.to_right == asked.to_right && plan.to_left == asked.to_left;
    if !same_copies || !within_prompt(&plan.trash, &asked.trash) {
        return None;
    }
    let mut report = run_sync(&plan, &run.left, &run.right, &*run.trash, cancel, done);
    if !report.cancelled() {
        let left_alone = left_alone(asked, &plan);
        if !left_alone.is_empty() {
            let trash = report.trash.get_or_insert_with(BatchReport::default);
            trash.failed.extend(left_alone);
        }
    }
    Some((report, plan))
}

/// What the question named that the new plan leaves alone, as "Changed since the compare."
/// failures: each named item the new plan does not take at all, and each item inside what the
/// question named that changed since the compare. A named folder the new plan only splits
/// (some of it still goes) is no failure itself; what changed inside it is named instead.
fn left_alone(asked: &SyncPlan, plan: &SyncPlan) -> Vec<(PathBuf, String)> {
    let named: HashSet<&Path> = asked.trash.iter().map(PathBuf::as_path).collect();
    let still: HashSet<&Path> = plan.trash.iter().map(PathBuf::as_path).collect();
    let split: HashSet<&Path> = plan
        .trash
        .iter()
        .filter_map(|p| p.ancestors().skip(1).find(|a| named.contains(a)))
        .collect();
    let untaken = asked
        .trash
        .iter()
        .filter(|p| !still.contains(p.as_path()) && !split.contains(p.as_path()));
    let changed_inside = plan
        .changed
        .iter()
        .filter(|p| p.ancestors().any(|a| named.contains(a)));
    let mut listed = HashSet::new();
    untaken
        .chain(changed_inside)
        .filter(|p| listed.insert(p.as_path()))
        .map(|p| (p.clone(), ActionError::Changed(p.clone()).to_string()))
        .collect()
}

/// Registers the actions that open the panel on the view's element.
pub(super) fn sync_actions<E: InteractiveElement>(el: E, cx: &mut Context<FolderView>) -> E {
    el.on_action(
        cx.listener(|this, _: &OpenSync, window, cx| this.open_sync(SyncScope::All, window, cx)),
    )
    .on_action(cx.listener(|this, _: &SyncSelected, window, cx| {
        this.open_sync(SyncScope::Selected, window, cx)
    }))
}

impl FolderView {
    pub fn sync_open(&self) -> bool {
        self.sync.panel.is_some()
    }

    pub fn sync_mode(&self) -> Option<SyncMode> {
        self.sync.panel.as_ref().map(|p| p.mode)
    }

    /// The plan on show; None while it is worked out or the panel is closed.
    pub fn sync_plan_shown(&self) -> Option<&SyncPlan> {
        let shown = self.sync.panel.as_ref()?.shown.as_ref()?;
        Some(&shown.plan)
    }

    fn picked_count(&self) -> usize {
        self.model.as_ref().map_or(0, |m| m.picked().len())
    }

    /// A compare started (`false`) or finished with its result on show (`true`). Whether that
    /// result has a row still being compared is decided here, once, not on each frame (M-1).
    pub(super) fn set_result_live(&mut self, live: bool) {
        self.sync.live = live
            && self.model.as_ref().is_some_and(|m| {
                !m.comparison()
                    .nodes
                    .iter()
                    .any(|n| matches!(n.status, Status::Pending))
            });
    }

    /// What the last compare found about the roots: whether one is inside the other.
    pub(super) fn set_roots_nested(&mut self, nested: bool) {
        self.sync.nested = nested;
    }

    /// The selection may have changed: an open panel counts it again.
    pub(super) fn sync_selection_changed(&mut self) {
        if self.sync.panel.is_none() {
            return;
        }
        let picked = self.picked_count();
        if let Some(panel) = &mut self.sync.panel {
            panel.picked = picked;
        }
    }

    /// The last compare started finished and its result is on show, with no row still being
    /// compared (P10b PF4): what a sync or an export may work on.
    pub(super) fn result_ready(&self) -> bool {
        self.sync.live
    }

    /// R16: true, with a notice, when a sync cannot start now: a batch, an export or a
    /// compare runs, there is no finished result, or one root is inside the other (as the
    /// last compare found; Start checks it again off the UI thread).
    fn refuse_sync(&mut self, cx: &mut Context<Self>) -> bool {
        if self.refuse_while_working(cx) {
            return true;
        }
        let why = if self.is_busy() {
            Some("Wait for the compare to finish.")
        } else if !self.result_ready() {
            Some("Compare the folders first.")
        } else if self.sync.nested {
            Some(NESTED)
        } else {
            None
        };
        if let Some(why) = why {
            self.notice = Some(why.into());
            cx.notify();
        }
        why.is_some()
    }

    /// Opens the panel (or switches an open one) to `scope`: the toolbar asks for the whole
    /// folder, the row menu for the selection (R15). The panel takes the keys.
    pub fn open_sync(&mut self, scope: SyncScope, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_sync(cx) {
            return;
        }
        self.notice = None;
        self.close_export(cx);
        self.sync.closed_focus = None;
        let picked = self.picked_count();
        let scope = if picked == 0 { SyncScope::All } else { scope };
        let opened = self.sync.opened + 1;
        let epoch = self.plan_epoch;
        let panel = self.sync.panel.get_or_insert_with(|| SyncPanel {
            mode: SyncMode::UpdateRight,
            scope,
            shown: None,
            job: JobSlot::default(),
            task: None,
            focus: cx.focus_handle().tab_stop(true),
            id: opened,
            epoch,
            picked,
        });
        self.sync.opened = panel.id;
        panel.scope = scope;
        panel.picked = picked;
        window.focus(&panel.focus, cx);
        self.plan_sync(cx);
    }

    pub fn set_sync_mode(&mut self, mode: SyncMode, cx: &mut Context<Self>) {
        let Some(panel) = &mut self.sync.panel else {
            return;
        };
        if panel.mode != mode {
            panel.mode = mode;
            self.plan_sync(cx);
        }
    }

    /// "Selected items only" needs a selection.
    pub fn set_sync_scope(&mut self, scope: SyncScope, cx: &mut Context<Self>) {
        let Some(panel) = &mut self.sync.panel else {
            return;
        };
        if scope == SyncScope::Selected && panel.picked == 0 {
            return;
        }
        if panel.scope != scope {
            panel.scope = scope;
            self.plan_sync(cx);
        }
    }

    /// Works the plan of the panel's mode and scope out on the background executor, from a
    /// snapshot of the model (R16). A newer request drops an older answer.
    fn plan_sync(&mut self, cx: &mut Context<Self>) {
        let criterion = self.options().mode;
        let (Some(model), Some(panel)) = (&self.model, &mut self.sync.panel) else {
            return;
        };
        let snap = Arc::new(match panel.scope {
            SyncScope::All => Snapshot::whole(model, &self.left, &self.right),
            SyncScope::Selected => Snapshot::new(model, &self.left, &self.right),
        });
        panel.shown = None;
        let (generation, _) = panel.job.start();
        let mode = panel.mode;
        let (left, right) = (self.left.clone(), self.right.clone());
        let background = cx.background_executor().clone();
        panel.task = Some(cx.spawn(async move |this, cx| {
            let s = snap.clone();
            let (plan, summary, lines) = background
                .spawn(async move {
                    let plan = s.sync_plan(mode);
                    let summary = sync_summary(&plan);
                    let lines = sync_lines(&plan, &left, &right, criterion);
                    (plan, summary, lines)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if let Some(panel) = &mut view.sync.panel
                    && panel.job.is_current(generation)
                {
                    panel.job.finish(generation);
                    panel.shown = Some(Shown {
                        plan: Arc::new(plan),
                        snap,
                        summary: summary.into(),
                        lines,
                    });
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    /// Closes the panel. Without a window at hand, the next frame gives the rows the focus if
    /// the panel held it.
    pub fn close_sync(&mut self, cx: &mut Context<Self>) {
        if let Some(panel) = self.sync.panel.take() {
            self.sync.closed_focus = Some(panel.focus);
            cx.notify();
        }
    }

    /// Closes the panel; the rows take the focus if it was in the panel.
    pub(super) fn close_sync_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.sync.panel.take() {
            if panel.focus.contains_focused(window, cx) {
                window.focus(&self.focus, cx);
            }
            cx.notify();
        }
    }

    /// Any compare closes the panel (R17): its plan describes the result being replaced.
    pub(super) fn close_sync_for_compare(&mut self, cx: &mut Context<Self>) {
        if self.sync.panel.is_some() {
            self.close_sync(cx);
            self.notice = Some(COMPARED_AGAIN.into());
        }
    }

    /// Enter in the panel (R17, PF11): only on the panel itself, so a focused button takes its
    /// own Enter, and only for a sync that adds files and folders.
    fn sync_confirm(&mut self, _: &SyncConfirm, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = &self.sync.panel else {
            cx.propagate();
            return;
        };
        if !panel.focus.is_focused(window) {
            cx.propagate();
            return;
        }
        let adds_only = panel.shown.as_ref().is_some_and(|s| {
            !s.plan.is_empty() && s.plan.overwrites() == 0 && s.plan.trash.is_empty()
        });
        if adds_only {
            self.start_sync(window, cx);
        }
    }

    /// "Start sync" (R16): nested roots and every destination are checked first, on the
    /// background executor (links, outside the root, into its own source), then the
    /// questions, then `run_sync_batch`.
    pub fn start_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = &self.sync.panel else {
            return;
        };
        let Some(shown) = panel.shown.as_ref().filter(|s| !s.plan.is_empty()) else {
            return;
        };
        let (plan, snap, mode, id) = (shown.plan.clone(), shown.snap.clone(), panel.mode, panel.id);
        if panel.epoch != self.plan_epoch {
            self.close_sync_in(window, cx);
            self.notice = Some(COMPARED_AGAIN.into());
            return;
        }
        if self.refuse_sync(cx) {
            return;
        }
        self.notice = None;
        let run = SyncRun {
            id,
            epoch: self.plan_epoch,
            asked: plan.clone(),
            snap,
            mode,
            left: self.left.clone(),
            right: self.right.clone(),
            trash: self.trash.clone(),
        };
        let background = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let (p, l, r) = (plan.clone(), run.left.clone(), run.right.clone());
            let checked = background
                .spawn(async move {
                    // Again here, off the UI thread: a root may have become a link into the
                    // other since the compare.
                    if roots_overlap(&l, &r) {
                        return Err(NESTED.to_owned());
                    }
                    let checks = check(&r, &p.to_right).and_then(|to_right| {
                        check(&l, &p.to_left).map(|to_left| (to_right, to_left))
                    });
                    checks.map_err(|err| format!("Nothing was synced: {err}."))
                })
                .await;
            let newer: HashSet<PathBuf> = match checked {
                Ok((to_right, to_left)) => to_right
                    .into_iter()
                    .chain(to_left)
                    .filter(|c| c.destination_newer)
                    .map(|c| c.path)
                    .collect(),
                Err(notice) => {
                    let _ = this.update(cx, |view, cx| {
                        view.notice = Some(notice.into());
                        cx.notify();
                    });
                    return;
                }
            };
            let mut questions = Vec::new();
            if plan.overwrites() > 0 {
                questions.push((sync_overwrite_prompt(&plan, &newer), REPLACE_ANSWERS));
            }
            if !plan.trash.is_empty() {
                questions.push((sync_trash_prompt(&plan), TRASH_ANSWERS));
            }
            for ((message, detail), answers) in questions {
                let Ok(answer) = this.update_in(cx, |_, window, cx| {
                    window.prompt(PromptLevel::Warning, &message, Some(&detail), &answers, cx)
                }) else {
                    return;
                };
                if !matches!(answer.await, Ok(1)) {
                    return;
                }
            }
            let _ = this.update_in(cx, |view, window, cx| {
                view.run_sync_batch(run, background, window, cx)
            });
        })
        .detach();
    }

    /// After the answers: the compare must be the one the plan was made from and the panel the
    /// one that asked. The panel then closes, so Esc reaches the batch (PF5), and the sync
    /// runs with its progress line. The notice is `sync_result`.
    fn run_sync_batch(
        &mut self,
        run: SyncRun,
        background: BackgroundExecutor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.plan_epoch != run.epoch {
            self.close_sync_in(window, cx);
            self.notice = Some(COMPARED_AGAIN.into());
            cx.notify();
            return;
        }
        // Closed while the question was open (Esc, a chip): the answer runs nothing.
        if self.sync.panel.as_ref().map(|p| p.id) != Some(run.id) {
            return;
        }
        if self.refuse_while_working(cx) {
            return;
        }
        self.close_sync_in(window, cx);
        let total = run.asked.files() + run.asked.folders() + run.asked.trash.len();
        let (cancel, done) = self.start_batch("Syncing", total, cx);
        cx.spawn_in(window, async move |this, cx| {
            let outcome = background
                .spawn(async move { replan_and_run(&run, &cancel, &done) })
                .await;
            let _ = this.update_in(cx, |view, window, cx| {
                let (notice, failed) = match &outcome {
                    Some((report, plan)) => (sync_result(report, plan), report.failed()),
                    None => (STALE.to_owned(), Vec::new()),
                };
                view.finish_batch(notice, &failed, window, cx);
            });
        })
        .detach();
    }

    /// The panel over the row list (R17), or None while it is closed. A panel closed without
    /// a window at hand gives the rows the focus here, if it held it.
    pub(super) fn render_sync_panel(
        &mut self,
        p: &Palette,
        fonts: &Fonts,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if let Some(old) = self.sync.closed_focus.take()
            && (window.focused(cx).is_none() || old.contains_focused(window, cx))
        {
            window.focus(&self.focus, cx);
        }
        let panel = self.sync.panel.as_ref()?;
        let picked = panel.picked;
        let shown = panel.shown.as_ref();
        let summary = shown.map_or_else(
            || SharedString::from("Working out what to do…"),
            |s| s.summary.clone(),
        );
        let can_start = shown.is_some_and(|s| !s.plan.is_empty());
        let cards = MODES
            .iter()
            .map(|&(mode, id, title, text)| {
                let on = panel.mode == mode;
                ld_card(id, title, p)
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(190.))
                    .px(px(9.))
                    .py(px(7.))
                    .gap(px(2.))
                    .rounded(px(8.))
                    .when(on, |card| card.border_color(p.accent).bg(p.panel3))
                    .aria_selected(on)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(div().text_size(px(11.)).text_color(p.dim).child(text))
                    .ld_click(
                        true,
                        cx.listener(move |this, _, _, cx| this.set_sync_mode(mode, cx)),
                    )
            })
            .collect::<Vec<_>>();
        let scope = ld_segmented(
            "folder-sync-scope",
            [
                ld_segment(
                    "folder-sync-scope-all",
                    "Whole folder",
                    panel.scope == SyncScope::All,
                    false,
                    p,
                )
                .ld_click(
                    true,
                    cx.listener(|this, _, _, cx| this.set_sync_scope(SyncScope::All, cx)),
                ),
                ld_segment(
                    "folder-sync-scope-selected",
                    format!("Selected items only ({})", format_size(picked as u64)),
                    panel.scope == SyncScope::Selected,
                    true,
                    p,
                )
                .ld_click(
                    picked > 0,
                    cx.listener(|this, _, _, cx| this.set_sync_scope(SyncScope::Selected, cx)),
                ),
            ],
            p,
        );
        let list = shown.filter(|s| !s.lines.is_empty()).map(|s| {
            div()
                .id("folder-sync-list")
                .test_support()
                .max_h(px(300.))
                .min_h(px(72.))
                .overflow_y_scroll()
                .px(px(8.))
                .py(px(4.))
                .rounded(px(8.))
                .border_1()
                .border_color(p.line)
                .bg(p.panel2)
                .font(fonts.code.clone())
                .text_size(px(11.5))
                .children(s.lines.iter().map(|line| {
                    let color = match line.chars().next() {
                        Some('→' | '←') => p.ins,
                        Some('✕') => p.del,
                        _ => p.dim,
                    };
                    div()
                        .text_color(color)
                        .whitespace_nowrap()
                        .child(line.clone())
                }))
        });
        let body = v_flex()
            .id("folder-sync-panel")
            .test_support()
            .track_focus(&panel.focus)
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::sync_confirm))
            .w_full()
            .max_w(px(760.))
            // No taller than the window: the list gives up its height first, then the rest
            // scrolls, so Start stays on screen.
            .max_h_full()
            .p(px(14.))
            .gap(px(8.))
            .rounded(px(10.))
            .border_1()
            .border_color(p.line)
            .bg(p.panel)
            .text_size(px(12.5))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(14.))
                    .child("Sync folders"),
            )
            .child(
                v_flex()
                    .id("folder-sync-fields")
                    .test_support()
                    .min_h(px(72.))
                    .overflow_y_scroll()
                    .gap(px(8.))
                    .child(h_flex().flex_wrap().gap(px(6.)).children(cards))
                    .child(scope)
                    .child(
                        div()
                            .id("folder-sync-summary")
                            .test_support()
                            .aria_label(summary.clone())
                            .child(summary),
                    )
                    .children(list)
                    .child(div().text_size(px(11.5)).text_color(p.dim).child(HELP)),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(ld_button("folder-sync-cancel", "Cancel", p).ld_click(
                        true,
                        cx.listener(|this, _, window, cx| this.close_sync_in(window, cx)),
                    ))
                    .child(ld_primary("folder-sync-start", "Start sync", p).ld_click(
                        can_start,
                        cx.listener(|this, _, window, cx| this.start_sync(window, cx)),
                    )),
            );
        Some(
            div()
                .id("folder-sync-overlay")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .overflow_y_scroll()
                .flex()
                .justify_center()
                .items_start()
                .p(px(12.))
                .bg(p.bg)
                .child(body)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use duet_core::folder::entry::EntryKind;
    use duet_core::folder::plan::{BatchReport, CopyBatch, CopyItem};
    use duet_core::folder::sync::{Seen, SkipReason, SyncSkip};
    use duet_core::folder::tree::Side;

    fn item(rel: &str, len: u64, overwrites: bool, to_right: bool) -> CopyItem {
        let (from, to) = if to_right { ("L", "R") } else { ("R", "L") };
        CopyItem {
            src: Path::new(from).join(rel),
            dst: Path::new(to).join(rel),
            len,
            overwrites,
        }
    }

    fn batch(files: Vec<CopyItem>, mkdirs: Vec<PathBuf>) -> CopyBatch {
        let bytes = files.iter().map(|f| f.len).sum();
        CopyBatch {
            files,
            mkdirs,
            bytes,
        }
    }

    fn skip(rel: &str, why: SkipReason) -> SyncSkip {
        SyncSkip {
            rel: rel.into(),
            why,
        }
    }

    #[test]
    fn sync_lines_name_each_action_relative_to_its_root() {
        let plan = SyncPlan {
            to_right: batch(
                vec![
                    item("a/x.txt", 1, false, true),
                    item("y.txt", 2, true, true),
                ],
                vec![Path::new("R").join("empty")],
            ),
            to_left: batch(vec![item("b.txt", 3, true, false)], vec![]),
            trash: vec![Path::new("R").join("old"), Path::new("R").join("z.log")],
            trash_side: Some(Side::Right),
            skips: vec![skip("same-time.txt", SkipReason::SameTime)],
            seen: [(
                Path::new("R").join("old"),
                Seen {
                    kind: EntryKind::Dir,
                    len: 0,
                    modified: None,
                },
            )]
            .into_iter()
            .collect(),
            ..SyncPlan::default()
        };
        assert_eq!(
            sync_lines(&plan, Path::new("L"), Path::new("R"), CompareMode::Content),
            [
                "→ y.txt (replaces)",
                "← b.txt (replaces)",
                "→ a/x.txt",
                "→ empty/ (new folder)",
                "✕ old/",
                "✕ z.log",
                "· same-time.txt — the modified times are equal or near: can't tell which is newer",
            ]
        );
    }

    /// I-1: every file a sync replaces is named before the plain copies, under a cap of its
    /// own, so a long list of new files never hides one.
    #[test]
    fn sync_lines_list_replacements_first() {
        let mut files: Vec<CopyItem> = (0..450)
            .map(|i| item(&format!("a{i}"), 1, false, true))
            .collect();
        files.extend((0..3).map(|i| item(&format!("r{i}"), 1, true, true)));
        let plan = SyncPlan {
            to_right: batch(files, vec![]),
            ..SyncPlan::default()
        };
        let (l, r) = (Path::new("L"), Path::new("R"));
        let lines = sync_lines(&plan, l, r, CompareMode::Content);
        assert_eq!(
            lines[..4],
            [
                "→ r0 (replaces)",
                "→ r1 (replaces)",
                "→ r2 (replaces)",
                "→ a0"
            ]
        );
        assert_eq!(lines.len(), 3 + 400 + 1);
        assert_eq!(lines.last().map(String::as_str), Some("…"));
        // 401 replacements fill their own 400 lines; the plain copy still shows.
        let mut files: Vec<CopyItem> = (0..401)
            .map(|i| item(&format!("r{i}"), 1, true, true))
            .collect();
        files.push(item("a", 1, false, true));
        let plan = SyncPlan {
            to_right: batch(files, vec![]),
            to_left: batch(vec![item("b", 1, true, false)], vec![]),
            ..SyncPlan::default()
        };
        let lines = sync_lines(&plan, l, r, CompareMode::Content);
        assert_eq!(lines.len(), 400 + 1 + 1);
        assert_eq!(lines[399], "→ r399 (replaces)");
        assert_eq!(lines[400], "→ a");
        assert_eq!(lines[401], "…");
    }

    /// M-10 (PF13): in Quick and Size only a case-only skip does not claim the contents.
    #[test]
    fn a_case_only_skip_reads_by_the_criterion() {
        let plan = SyncPlan {
            skips: vec![skip("Config.xml", SkipReason::CaseOnly)],
            ..SyncPlan::default()
        };
        let line = |mode| sync_lines(&plan, Path::new("L"), Path::new("R"), mode);
        assert_eq!(
            line(CompareMode::Quick),
            [
                "· Config.xml — the names differ only in case and the size and modified time match: rename it by hand"
            ]
        );
        assert_eq!(
            line(CompareMode::Size),
            [
                "· Config.xml — the names differ only in case and the size matches: rename it by hand"
            ]
        );
        assert_eq!(
            line(CompareMode::Content),
            [
                "· Config.xml — the names differ only in case and the contents match: rename it by hand"
            ]
        );
    }

    #[test]
    fn sync_lines_cap_each_kind_and_mark_the_cut() {
        let files = (0..401)
            .map(|i| item(&format!("f{i}"), 1, false, true))
            .collect();
        let plan = SyncPlan {
            to_right: batch(files, vec![]),
            trash: (0..200)
                .map(|i| Path::new("R").join(format!("t{i}")))
                .collect(),
            trash_side: Some(Side::Right),
            skips: (0..100)
                .map(|i| skip(&format!("s{i}"), SkipReason::Link))
                .collect(),
            ..SyncPlan::default()
        };
        let lines = sync_lines(&plan, Path::new("L"), Path::new("R"), CompareMode::Content);
        assert_eq!(lines.len(), 400 + 200 + 100 + 1);
        assert_eq!(lines[399], "→ f399");
        assert_eq!(lines[400], "✕ t0");
        assert_eq!(lines[600], "· s0 — links are never copied");
        assert_eq!(lines.last().map(String::as_str), Some("…"));
        let exact = SyncPlan {
            to_right: batch(
                (0..400)
                    .map(|i| item(&format!("f{i}"), 1, false, true))
                    .collect(),
                vec![],
            ),
            ..SyncPlan::default()
        };
        let lines = sync_lines(&exact, Path::new("L"), Path::new("R"), CompareMode::Content);
        assert_eq!(lines.len(), 400, "nothing cut, no ellipsis");
        let left = SyncPlan {
            trash: (0..201)
                .map(|i| Path::new("L").join(format!("t{i}")))
                .collect(),
            trash_side: Some(Side::Left),
            ..SyncPlan::default()
        };
        let lines = sync_lines(&left, Path::new("L"), Path::new("R"), CompareMode::Content);
        assert_eq!((lines.len(), lines[0].as_str()), (201, "✕ t0"));
        assert_eq!(lines.last().map(String::as_str), Some("…"));
    }

    #[test]
    fn sync_summary_counts_each_part_and_says_when_there_is_nothing_to_do() {
        assert_eq!(
            sync_summary(&SyncPlan::default()),
            "Nothing to do: the two sides already match for this mode."
        );
        let plan = SyncPlan {
            to_right: batch(
                vec![item("a", 1500, true, true), item("b", 2, true, true)],
                vec![Path::new("R").join("e1"), Path::new("R").join("e2")],
            ),
            to_left: batch(vec![item("c", 1, false, false)], vec![]),
            trash: vec![Path::new("R").join("x")],
            trash_side: Some(Side::Right),
            skips: vec![skip("k", SkipReason::Kind)],
            ..SyncPlan::default()
        };
        assert_eq!(
            sync_summary(&plan),
            "Copy 3 files (1,503 bytes; 2 replace existing files) · Create 2 folders · Move 1 item to the Recycle Bin · Skip 1"
        );
        let skips_only = SyncPlan {
            skips: vec![skip("a", SkipReason::CaseOnly)],
            ..SyncPlan::default()
        };
        assert_eq!(
            sync_summary(&skips_only),
            "Copy 0 files (0 bytes) · Move 0 items to the Recycle Bin · Skip 1",
            "skips are something to read, so not \"nothing to do\""
        );
    }

    fn done(files: usize, folders: usize) -> BatchReport {
        BatchReport {
            done: files + folders,
            folders,
            ..BatchReport::default()
        }
    }

    #[test]
    fn sync_result_names_success_failure_and_cancel() {
        let copies = SyncPlan {
            to_right: batch(
                vec![item("a", 1, false, true), item("b", 1, true, true)],
                vec![],
            ),
            ..SyncPlan::default()
        };
        let ok = SyncReport {
            to_right: done(2, 0),
            ..SyncReport::default()
        };
        assert_eq!(sync_result(&ok, &copies), "Synced: copied 2 files.");
        let mirror = SyncPlan {
            to_right: batch(
                vec![item("a", 1, false, true)],
                vec![Path::new("R").join("e")],
            ),
            trash: vec![Path::new("R").join("x")],
            trash_side: Some(Side::Right),
            ..SyncPlan::default()
        };
        let all = SyncReport {
            to_right: done(1, 1),
            trash: Some(done(1, 0)),
            ..SyncReport::default()
        };
        assert_eq!(
            sync_result(&all, &mirror),
            "Synced: copied 1 file, created 1 folder and moved 1 item to the Recycle Bin."
        );
        let changed = SyncReport {
            to_right: done(1, 1),
            trash: Some(BatchReport {
                failed: vec![(
                    Path::new("R").join("x"),
                    "Changed since the compare.".into(),
                )],
                ..BatchReport::default()
            }),
            ..SyncReport::default()
        };
        assert_eq!(
            sync_result(&changed, &mirror),
            format!(
                "Synced: copied 1 file, created 1 folder and moved 0 items to the Recycle Bin; 1 failed: {}: Changed since the compare.",
                Path::new("R").join("x").display()
            )
        );
        let only_trash = SyncPlan {
            trash: vec![Path::new("R").join("x")],
            trash_side: Some(Side::Right),
            ..SyncPlan::default()
        };
        let moved = SyncReport {
            trash: Some(done(1, 0)),
            ..SyncReport::default()
        };
        assert_eq!(
            sync_result(&moved, &only_trash),
            "Synced: moved 1 item to the Recycle Bin."
        );
        let cancelled = SyncReport {
            to_right: BatchReport {
                done: 1,
                cancelled: true,
                ..BatchReport::default()
            },
            ..SyncReport::default()
        };
        assert_eq!(
            sync_result(&cancelled, &mirror),
            "Sync cancelled after 1 of 3 items. Nothing went to the Recycle Bin."
        );
        assert_eq!(
            sync_result(&cancelled, &copies),
            "Sync cancelled after 1 of 2 items."
        );
        let mid_trash = SyncReport {
            to_right: done(1, 1),
            trash: Some(BatchReport {
                done: 1,
                cancelled: true,
                ..BatchReport::default()
            }),
            ..SyncReport::default()
        };
        let two = SyncPlan {
            trash: vec![Path::new("R").join("x"), Path::new("R").join("y")],
            ..mirror.clone()
        };
        assert_eq!(
            sync_result(&mid_trash, &two),
            "Sync cancelled after 3 of 4 items.",
            "something went to the Recycle Bin, so it does not say nothing did"
        );
    }

    #[test]
    fn the_questions_name_every_overwrite_and_the_recycle_bin_side() {
        let plan = SyncPlan {
            to_right: batch(
                (0..7)
                    .map(|i| item(&format!("f{i}"), 1, true, true))
                    .collect(),
                vec![],
            ),
            to_left: batch(
                vec![item("g", 1, true, false), item("h", 1, false, false)],
                vec![],
            ),
            ..SyncPlan::default()
        };
        let newer: HashSet<PathBuf> = [Path::new("R").join("f6")].into_iter().collect();
        let (message, detail) = sync_overwrite_prompt(&plan, &newer);
        assert_eq!(message, "Replace 8 files?");
        let path = |side: &str, name: &str| Path::new(side).join(name).display().to_string();
        let lines: Vec<&str> = detail.lines().collect();
        assert_eq!(lines[0], "Overwrites 7 files on the right.", "{detail}");
        assert_eq!(
            lines[1], "1 of them is newer than the copy that replaces it.",
            "{detail}"
        );
        // I-1: every replaced file is named, the newer one first.
        let right: Vec<String> = ["f6", "f0", "f1", "f2", "f3", "f4", "f5"]
            .map(|n| path("R", n))
            .into();
        assert_eq!(lines[3..10], right, "{detail}");
        assert!(!detail.contains("more"), "{detail}");
        assert!(
            detail.contains("Overwrites 1 file on the left."),
            "{detail}"
        );
        assert!(detail.ends_with(&path("L", "g")), "{detail}");

        // Up to 200 a side, the newer ones first, then the count of the rest.
        let many = SyncPlan {
            to_right: batch(
                (0..210)
                    .map(|i| item(&format!("f{i}"), 1, true, true))
                    .collect(),
                vec![],
            ),
            ..SyncPlan::default()
        };
        let newer: HashSet<PathBuf> = (205..210)
            .map(|i| Path::new("R").join(format!("f{i}")))
            .collect();
        let (message, detail) = sync_overwrite_prompt(&many, &newer);
        assert_eq!(message, "Replace 210 files?");
        let lines: Vec<&str> = detail.lines().collect();
        assert_eq!(
            lines[1],
            "5 of them are newer than the copies that replace them."
        );
        assert_eq!(lines.len(), 3 + 200 + 1, "{detail}");
        assert_eq!(lines[3], path("R", "f205"));
        assert_eq!(lines[7], path("R", "f209"));
        assert_eq!(lines[8], path("R", "f0"));
        assert_eq!(lines[202], path("R", "f194"));
        assert_eq!(lines[203], "…and 10 more");

        let mirror = SyncPlan {
            trash: (0..8)
                .map(|i| Path::new("L").join(format!("t{i}")))
                .collect(),
            trash_side: Some(Side::Left),
            skips: vec![skip("logs/", SkipReason::KeptFolder)],
            ..SyncPlan::default()
        };
        let (message, detail) = sync_trash_prompt(&mirror);
        assert_eq!(message, "Move 8 items on the left to the Recycle Bin?");
        assert!(detail.contains("…and 2 more"), "{detail}");
        assert!(
            detail.ends_with(
                "1 folder still holds files that are not shown, so it stays; only the files you see go."
            ),
            "{detail}"
        );
    }

    fn put(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        let t0 = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(t0)
            .unwrap();
    }

    /// A Mirror to right of `l` and `r` as Start asks about it, with a logging Recycle Bin.
    fn mirror_run(l: &Path, r: &Path) -> (SyncRun, Arc<std::sync::Mutex<Vec<PathBuf>>>) {
        use duet_core::folder::run::{FolderOptions, FolderProgress, compare_folders};
        let progress = FolderProgress::default();
        let options = FolderOptions::default();
        let cmp = compare_folders(l, r, &options, &progress, &CancelToken::new()).unwrap();
        let model = crate::folder::model::FolderModel::new(cmp);
        let snap = Arc::new(Snapshot::whole(&model, l, r));
        let mode = SyncMode::MirrorRight;
        let calls: Arc<std::sync::Mutex<Vec<PathBuf>>> = Arc::default();
        let log = calls.clone();
        let run = SyncRun {
            id: 1,
            epoch: 1,
            asked: Arc::new(snap.sync_plan(mode)),
            snap,
            mode,
            left: l.to_path_buf(),
            right: r.to_path_buf(),
            trash: Arc::new(move |p: &Path| -> Result<(), ActionError> {
                log.lock().unwrap().push(p.to_path_buf());
                Ok(())
            }),
        };
        (run, calls)
    }

    /// A two-file right-only folder `old`, planned to go whole.
    fn mirror_fixture() -> (tempfile::TempDir, tempfile::TempDir) {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(l.path(), "keep.txt", "k\n");
        put(r.path(), "keep.txt", "k\n");
        put(r.path(), "old/a.txt", "a\n");
        put(r.path(), "old/b.txt", "b\n");
        (l, r)
    }

    /// M-8: the question named `old`; after it, `old/b.txt` was edited, so only `old/a.txt`
    /// goes. The folder is not a failure; the file that changed is named.
    #[test]
    fn a_folder_the_new_plan_splits_names_the_file_that_changed() {
        let (l, r) = mirror_fixture();
        let (run, calls) = mirror_run(l.path(), r.path());
        let old = r.path().join("old");
        assert_eq!(run.asked.trash, std::slice::from_ref(&old));
        put(r.path(), "old/b.txt", "edited after the question\n");
        let (report, plan) =
            replan_and_run(&run, &CancelToken::new(), &AtomicUsize::new(0)).unwrap();
        assert_eq!(plan.trash, [old.join("a.txt")]);
        assert_eq!(calls.lock().unwrap().as_slice(), [old.join("a.txt")]);
        assert_eq!(
            report.failed(),
            [(old.join("b.txt"), "Changed since the compare.".to_owned())]
        );
        // A folder left whole alone is still named, once.
        let (l, r) = mirror_fixture();
        let (run, calls) = mirror_run(l.path(), r.path());
        std::fs::remove_dir_all(r.path().join("old")).unwrap();
        let (report, _) = replan_and_run(&run, &CancelToken::new(), &AtomicUsize::new(0)).unwrap();
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(
            report.failed(),
            [(
                r.path().join("old"),
                "Changed since the compare.".to_owned()
            )]
        );
    }

    /// M-12: a cancel that comes before the batch plans the deletes again stops that walk; the
    /// result counts the question's plan and nothing goes to the Recycle Bin.
    #[test]
    fn a_cancel_before_the_new_plan_runs_nothing() {
        let (l, r) = mirror_fixture();
        let (run, calls) = mirror_run(l.path(), r.path());
        let cancel = CancelToken::new();
        cancel.cancel();
        let (report, plan) = replan_and_run(&run, &cancel, &AtomicUsize::new(0)).unwrap();
        assert!(report.cancelled());
        assert!(Arc::ptr_eq(&plan, &run.asked), "no new plan was made");
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(
            sync_result(&report, &plan),
            "Sync cancelled after 0 of 1 item. Nothing went to the Recycle Bin."
        );
    }
}
