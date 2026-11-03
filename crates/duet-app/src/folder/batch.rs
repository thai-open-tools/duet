//! Batch work on the folder view's selection: the copy prompt, the background run with its
//! progress line, and the cancel and finish steps that delete and touch share (Task 6).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use duet_core::cancel::{CancelToken, Cancelled};
use duet_core::folder::actions::{ActionError, Conflict, copy_conflicts};
use duet_core::folder::plan::{
    BatchReport, CopyBatch, TrashPlan, copy_batch, expand, preflight_batch, run_copy, run_touch,
    run_trash_below, touch_pairs, trash_plan,
};
use duet_core::folder::sync::{SyncMode, SyncPlan, sync_plan, sync_plan_with_cancel};
use duet_core::folder::tree::{FolderComparison, Side};
use gpui_kit::{BackgroundExecutor, Context, PromptLevel, Task, Window};

use crate::folder::model::FolderModel;
use crate::folder::ops::Direction;
use crate::folder::view::FolderView;
use crate::format::count_of;

/// Paths a copy or delete prompt lists before "…and N more".
const LISTED: usize = 6;

/// Failures the finished-batch prompt lists before "…and N more" (R20).
const FAILURES_LISTED: usize = 200;

/// Moves one path to the Recycle Bin; the view holds it so tests can swap in a fake.
pub type TrashFn = Arc<dyn Fn(&Path) -> Result<(), ActionError> + Send + Sync>;

/// What a Delete command asks about.
#[derive(Clone, Copy)]
pub(super) enum DeleteAsk {
    /// Del: a choice prompt over the sides that have something.
    Choose,
    /// Shift+Del and the menu items: one confirmation for these sides.
    Sides(DeleteSides),
}

#[derive(Clone, Copy)]
pub(super) enum DeleteSides {
    One(Side),
    Both,
}

/// The batch that is running: its cancel switch, how many items were attempted, and the timer
/// that shows it in the status line. Dropping it cancels the work and stops the timer.
pub struct RunningBatch {
    pub cancel: CancelToken,
    pub done: Arc<AtomicUsize>,
    pub total: usize,
    verb: &'static str,
    _ticker: Task<()>,
}

impl RunningBatch {
    /// "Copying 3 of 7… Press Esc to cancel."
    pub fn progress_text(&self) -> String {
        let at = self.done.load(Ordering::Relaxed).min(self.total);
        format!("{} {at} of {}… Press Esc to cancel.", self.verb, self.total)
    }
}

impl Drop for RunningBatch {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

fn sides(direction: Direction) -> (&'static str, &'static str) {
    match direction {
        Direction::LeftToRight => ("left", "right"),
        Direction::RightToLeft => ("right", "left"),
    }
}

/// The copy prompt as `(message, detail, answers, confirm index)`. With an overwrite "Cancel"
/// comes first, so Enter never replaces a file; otherwise "Copy" does.
pub fn copy_prompt(
    batch: &CopyBatch,
    direction: Direction,
) -> (String, String, Vec<&'static str>, usize) {
    let conflicts: Vec<Conflict> = batch
        .files
        .iter()
        .filter(|f| f.overwrites)
        .map(|f| Conflict {
            path: f.dst.clone(),
            destination_newer: false,
        })
        .collect();
    copy_prompt_with(batch, direction, &conflicts)
}

/// [`copy_prompt`] with the overwritten files as the disk reports them, so the newer ones
/// are counted and listed first.
pub fn copy_prompt_with(
    batch: &CopyBatch,
    direction: Direction,
    conflicts: &[Conflict],
) -> (String, String, Vec<&'static str>, usize) {
    let (from, to) = sides(direction);
    let what = if batch.files.is_empty() {
        count_of(batch.mkdirs.len() as u64, "empty folder", "empty folders")
    } else {
        format!(
            "{} ({})",
            count_of(batch.files.len() as u64, "file", "files"),
            count_of(batch.bytes, "byte", "bytes")
        )
    };
    let message = format!("Copy {what} from {from} to {to}?");
    let mut sections = Vec::new();
    let overwrites = batch.overwrites();
    if overwrites > 0 {
        sections.push(overwrite_lines(overwrites, conflicts, to, LISTED));
    }
    if !batch.files.is_empty() && !batch.mkdirs.is_empty() {
        let folders = count_of(batch.mkdirs.len() as u64, "empty folder", "empty folders");
        sections.push(format!("Creates {folders}."));
    }
    let detail = sections.join("\n\n");
    if overwrites > 0 {
        (message, detail, vec!["Cancel", "Copy"], 1)
    } else {
        (message, detail, vec!["Copy", "Cancel"], 0)
    }
}

/// "Overwrites 3 files on the right.", how many are newer, and the first `listed` paths, the
/// newer ones first.
pub(super) fn overwrite_lines(
    count: usize,
    conflicts: &[Conflict],
    side: &str,
    listed: usize,
) -> String {
    let files = count_of(count as u64, "file", "files");
    let mut lines = vec![format!("Overwrites {files} on the {side}.")];
    let newer = conflicts.iter().filter(|c| c.destination_newer).count();
    match (count, newer) {
        (_, 0) => {}
        (1, _) => lines.push("It is newer than the copy that replaces it.".to_owned()),
        (_, 1) => lines.push("1 of them is newer than the copy that replaces it.".to_owned()),
        (_, n) => lines.push(format!(
            "{n} of them are newer than the copies that replace them."
        )),
    }
    lines.push(String::new());
    let mut paths: Vec<&Conflict> = conflicts.iter().collect();
    paths.sort_by_key(|c| !c.destination_newer);
    lines.extend(
        paths
            .iter()
            .take(listed)
            .map(|c| c.path.display().to_string()),
    );
    if count > listed {
        lines.push(format!("…and {} more", count - listed));
    }
    lines.join("\n")
}

/// The notice a finished copy leaves behind.
/// `files` and `folders` are how many of each the batch held (files run first).
pub fn copy_result(report: &BatchReport, files: usize, folders: usize) -> String {
    if report.cancelled {
        let tried = report.done + report.failed.len();
        if folders == 0 {
            return format!("Copy cancelled after {tried} of {files} files.");
        }
        let tried_files = tried.min(files);
        return format!(
            "Copy cancelled after {tried_files} of {files} files and {} of {folders} folders.",
            tried - tried_files
        );
    }
    let made = count_of(report.folders as u64, "folder", "folders");
    let copied = if files == 0 && folders > 0 {
        format!("Created {made}")
    } else {
        let copied = count_of(
            report.done.saturating_sub(report.folders) as u64,
            "file",
            "files",
        );
        match report.folders {
            0 => format!("Copied {copied}"),
            _ => format!("Copied {copied} and created {made}"),
        }
    };
    match report.failed.first() {
        None => format!("{copied}."),
        Some((path, err)) => format!(
            "{copied}; {} failed: {}: {err}.",
            report.failed.len(),
            path.display()
        ),
    }
}

/// The delete confirmation as `(message, detail)` for the plans of the sides in play, empty
/// ones included: the item count, per-side counts when both sides take part, the first few
/// paths, and what stays or was left alone (rule R4).
pub fn trash_prompt(plans: &[(Side, TrashPlan)]) -> (String, String) {
    let paths: Vec<&PathBuf> = plans.iter().flat_map(|(_, p)| &p.paths).collect();
    let kept: usize = plans.iter().map(|(_, p)| p.kept_folders).sum();
    let changed: usize = plans.iter().map(|(_, p)| p.changed).sum();
    let message = format!(
        "Move {} to the Recycle Bin?",
        count_of(paths.len() as u64, "item", "items")
    );
    let mut lines = Vec::new();
    if plans.iter().filter(|(_, p)| !p.paths.is_empty()).count() > 1 {
        let per_side: Vec<String> = plans
            .iter()
            .map(|(side, p)| {
                let name = if *side == Side::Left { "Left" } else { "Right" };
                format!(
                    "{name}: {}.",
                    count_of(p.paths.len() as u64, "item", "items")
                )
            })
            .collect();
        lines.push(per_side.join(" "));
        lines.push(String::new());
    }
    lines.extend(paths.iter().take(LISTED).map(|p| p.display().to_string()));
    if paths.len() > LISTED {
        lines.push(format!("…and {} more", paths.len() - LISTED));
    }
    let mut notes = Vec::new();
    match kept {
        0 => {}
        1 => notes.push(
            "1 folder still holds files that are not shown, so it stays; only the files you see go."
                .to_owned(),
        ),
        n => notes.push(format!(
            "{n} folders still hold files that are not shown, so they stay; only the files you see go."
        )),
    }
    match changed {
        0 => {}
        1 => notes.push("1 item changed on disk since the compare and is left alone.".to_owned()),
        n => notes.push(format!(
            "{n} items changed on disk since the compare and are left alone."
        )),
    }
    if !notes.is_empty() {
        lines.push(String::new());
        lines.extend(notes);
    }
    (message, lines.join("\n"))
}

/// The answers of the Del choice prompt for the sides that have something to take, and what
/// each answer after "Cancel" takes. "Cancel" is first, so Enter never deletes.
pub fn delete_answers(have: &[Side]) -> (Vec<&'static str>, Vec<Vec<Side>>) {
    let mut labels = vec!["Cancel"];
    let mut picks = Vec::new();
    for (label, side) in [("Delete left", Side::Left), ("Delete right", Side::Right)] {
        if have.contains(&side) {
            labels.push(label);
            picks.push(vec![side]);
        }
    }
    if have.contains(&Side::Left) && have.contains(&Side::Right) {
        labels.push("Delete both");
        picks.push(vec![Side::Left, Side::Right]);
    }
    (labels, picks)
}

/// The notice when every side in play has nothing to take.
fn nothing_to_delete(plans: &[(Side, TrashPlan)], one_side: bool) -> &'static str {
    if plans.iter().any(|(_, p)| p.kept_folders > 0) {
        "The selected folders hold only files that are not shown, so nothing was deleted."
    } else if plans.iter().any(|(_, p)| p.changed > 0) {
        "The selected items changed on disk since the compare. Press F5 to compare again."
    } else if one_side {
        "Nothing to delete on that side."
    } else {
        "Nothing to delete."
    }
}

/// Whether every fresh path is a prompt-time path or lies under one (component-wise), so a
/// plan rebuilt after the answer never takes more than the prompt showed.
pub(super) fn within_prompt(fresh: &[PathBuf], asked: &[PathBuf]) -> bool {
    let asked: std::collections::HashSet<&Path> = asked.iter().map(PathBuf::as_path).collect();
    fresh
        .iter()
        .all(|p| p.ancestors().any(|a| asked.contains(a)))
}

/// R20: the prompt text for a finished batch's failures, or None without any.
pub fn failure_prompt(failed: &[(PathBuf, String)]) -> Option<(String, String)> {
    if failed.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = failed
        .iter()
        .take(FAILURES_LISTED)
        .map(|(path, err)| format!("{}: {err}", path.display()))
        .collect();
    if failed.len() > FAILURES_LISTED {
        lines.push(format!("…and {} more", failed.len() - FAILURES_LISTED));
    }
    let message = format!("{} failed.", count_of(failed.len() as u64, "item", "items"));
    Some((message, lines.join("\n")))
}

/// The first failure as `"path: error"`, for a result notice.
fn first_failure(report: &BatchReport) -> Option<String> {
    report
        .failed
        .first()
        .map(|(path, err)| format!("{}: {err}", path.display()))
}

/// The notice a finished delete leaves behind.
pub fn trash_result(report: &BatchReport, total: usize, skipped: usize) -> String {
    if report.cancelled {
        let tried = report.done + report.failed.len();
        return format!("Delete cancelled after {tried} of {total} items.");
    }
    let moved = count_of(report.done as u64, "item", "items");
    let head = match first_failure(report) {
        None => format!("Moved {moved} to the Recycle Bin."),
        Some(first) => format!("Moved {moved}; {} failed: {first}.", report.failed.len()),
    };
    if skipped == 0 {
        return head;
    }
    let items = count_of(skipped as u64, "item", "items");
    let verb = if skipped == 1 { "was" } else { "were" };
    format!("{head} {items} changed on disk and {verb} left alone. Press F5 to compare again.")
}

/// The notice a finished touch leaves behind; `to` is the side that took the times.
pub fn touch_result(report: &BatchReport, total: usize, to: Side) -> String {
    if report.cancelled {
        let tried = report.done + report.failed.len();
        return format!("Touch cancelled after {tried} of {total} files.");
    }
    let (to, from) = match to {
        Side::Left => ("left", "right"),
        Side::Right => ("right", "left"),
    };
    let files = count_of(report.done as u64, "file", "files");
    let head = format!("Set the modified time of {files} on the {to} to match the {from}");
    match first_failure(report) {
        None => format!("{head}."),
        Some(first) => format!("{head}; {} failed: {first}.", report.failed.len()),
    }
}

/// What a delete or a sync works from: the tree, the pick and what the filters show, copied
/// off the model so the disk is read away from the UI thread.
pub(super) struct Snapshot {
    cmp: Arc<FolderComparison>,
    picked: Vec<u32>,
    shown: Vec<bool>,
    left: PathBuf,
    right: PathBuf,
}

impl Snapshot {
    /// The selection under the filters.
    pub(super) fn new(model: &FolderModel, left: &Path, right: &Path) -> Self {
        let cmp = model.shared_comparison();
        let shown = (0..cmp.nodes.len() as u32)
            .map(|n| model.passes_filters(n))
            .collect();
        Self {
            cmp,
            picked: model.picked(),
            shown,
            left: left.to_path_buf(),
            right: right.to_path_buf(),
        }
    }

    /// The whole folder: every top-level row, the filters ignored (R15).
    pub(super) fn whole(model: &FolderModel, left: &Path, right: &Path) -> Self {
        let cmp = model.shared_comparison();
        let picked = (0..cmp.nodes.len() as u32)
            .filter(|&n| cmp.nodes[n as usize].parent.is_none())
            .collect();
        let shown = vec![true; cmp.nodes.len()];
        Self {
            cmp,
            picked,
            shown,
            left: left.to_path_buf(),
            right: right.to_path_buf(),
        }
    }

    /// R14 over the snapshot. Reads the disk in mirror modes.
    pub(super) fn sync_plan(&self, mode: SyncMode) -> SyncPlan {
        sync_plan(
            &self.cmp,
            &self.picked,
            &|n| self.shown[n as usize],
            mode,
            &self.left,
            &self.right,
        )
    }

    /// [`Self::sync_plan`], which stops once `cancel` is set (the re-plan inside the batch).
    pub(super) fn sync_plan_with_cancel(
        &self,
        mode: SyncMode,
        cancel: &CancelToken,
    ) -> Result<SyncPlan, Cancelled> {
        sync_plan_with_cancel(
            &self.cmp,
            &self.picked,
            &|n| self.shown[n as usize],
            mode,
            &self.left,
            &self.right,
            cancel,
        )
    }

    /// Reads the disk now.
    fn plan(&self, side: Side) -> TrashPlan {
        let root = match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        };
        trash_plan(
            &self.cmp,
            &self.picked,
            &|n| self.shown[n as usize],
            side,
            root,
        )
    }
}

/// The preflight and the overwritten files, read off the UI thread.
pub(super) fn check(dst_root: &Path, batch: &CopyBatch) -> Result<Vec<Conflict>, ActionError> {
    preflight_batch(dst_root, batch)?;
    Ok(batch
        .files
        .iter()
        .filter(|f| f.overwrites)
        .filter_map(|f| copy_conflicts(&f.src, &f.dst).ok())
        .flatten()
        .collect())
}

impl FolderView {
    pub fn batch_running(&self) -> bool {
        self.batch.is_some()
    }

    /// True (with a notice) while a batch runs: copy, delete and touch wait for it.
    pub(super) fn refuse_while_batch(&mut self, cx: &mut Context<Self>) -> bool {
        if self.batch.is_none() {
            return false;
        }
        self.notice = Some("Wait for the running copy or delete to finish.".into());
        cx.notify();
        true
    }

    /// Registers a running batch of `total` items and shows its progress every 100 ms. The
    /// invisible-character scan stops: on Windows a file it holds open could not go to the
    /// Recycle Bin. The compare after the batch scans again.
    pub(super) fn start_batch(
        &mut self,
        verb: &'static str,
        total: usize,
        cx: &mut Context<Self>,
    ) -> (CancelToken, Arc<AtomicUsize>) {
        self.odd.drop_all();
        let (cancel, done) = (CancelToken::new(), Arc::new(AtomicUsize::new(0)));
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let running = this
                    .update(cx, |view, cx| {
                        let text = view.batch.as_ref().map(RunningBatch::progress_text);
                        if let Some(text) = &text {
                            view.set_status(text.clone());
                            cx.notify();
                        }
                        text.is_some()
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        });
        self.batch = Some(RunningBatch {
            cancel: cancel.clone(),
            done: done.clone(),
            total,
            verb,
            _ticker: ticker,
        });
        self.set_status(
            self.batch
                .as_ref()
                .map(RunningBatch::progress_text)
                .unwrap_or_default(),
        );
        cx.notify();
        (cancel, done)
    }

    /// Esc: stops a running batch after the item in hand. False when none runs.
    pub(super) fn cancel_batch(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(batch) = &self.batch else {
            return false;
        };
        batch.cancel.cancel();
        self.set_status("Cancelling…");
        cx.notify();
        true
    }

    /// Ends the batch, rescans, and leaves `notice` as the result line. Failures then get one
    /// prompt of their own (R20).
    pub(super) fn finish_batch(
        &mut self,
        notice: String,
        failed: &[(PathBuf, String)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.batch = None;
        self.refresh(cx);
        self.notice = Some(notice.into());
        cx.notify();
        if let Some((message, detail)) = failure_prompt(failed) {
            cx.spawn_in(window, async move |this, cx| {
                let Ok(answer) = this.update_in(cx, |_, window, cx| {
                    window.prompt(PromptLevel::Warning, &message, Some(&detail), &["OK"], cx)
                }) else {
                    return;
                };
                let _ = answer.await;
            })
            .detach();
        }
    }

    /// Test hook: pretend a batch of `total` items is running, without any work behind it.
    #[doc(hidden)]
    pub fn hold_batch_for_test(
        &mut self,
        verb: &'static str,
        total: usize,
        cx: &mut Context<Self>,
    ) {
        self.start_batch(verb, total, cx);
    }

    /// Test hook: whether the running batch has been told to stop.
    #[doc(hidden)]
    pub fn batch_cancelled_for_test(&self) -> bool {
        self.batch.as_ref().is_some_and(|b| b.cancel.is_cancelled())
    }

    /// Alt+Right / Alt+Left: copies what the selection covers under the filters, after one
    /// summary prompt, and shows its progress.
    pub(super) fn copy(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.refuse_while_working(cx) {
            return;
        }
        self.notice = None;
        let Some(model) = &self.model else {
            return;
        };
        let (from, to_root) = match direction {
            Direction::LeftToRight => (Side::Left, self.right.clone()),
            Direction::RightToLeft => (Side::Right, self.left.clone()),
        };
        let cmp = model.comparison();
        let ex = expand(cmp, &model.picked(), &|n| model.passes_filters(n));
        let batch = copy_batch(cmp, &ex, from, &self.left, &self.right);
        if batch.is_empty() {
            let (from_name, _) = sides(direction);
            self.set_status(format!(
                "Nothing to copy: the selection has no files on the {from_name}."
            ));
            cx.notify();
            return;
        }
        let background = cx.background_executor().clone();
        let epoch = self.plan_epoch;
        let ask = self.prefs.confirm_copy;
        cx.spawn_in(window, async move |this, cx| {
            let (b, root) = (batch.clone(), to_root.clone());
            let checked = background.spawn(async move { check(&root, &b) }).await;
            let conflicts = match checked {
                Ok(conflicts) => conflicts,
                Err(err) => {
                    let _ = this.update(cx, |view, cx| {
                        view.set_status(format!("Copy failed: {err}"));
                        cx.notify();
                    });
                    return;
                }
            };
            // R19: with "Ask before copying" off only a copy that overwrites nothing skips the
            // question; an overwrite always asks.
            if ask || batch.overwrites() > 0 {
                let (message, detail, answers, confirm) =
                    copy_prompt_with(&batch, direction, &conflicts);
                let level = if batch.overwrites() > 0 {
                    PromptLevel::Warning
                } else {
                    PromptLevel::Info
                };
                let Ok(answer) = this.update_in(cx, |_, window, cx| {
                    let detail = (!detail.is_empty()).then_some(detail.as_str());
                    window.prompt(level, &message, detail, &answers, cx)
                }) else {
                    return;
                };
                if !matches!(answer.await, Ok(i) if i == confirm) {
                    return;
                }
            }
            let _ = this.update_in(cx, |view, window, cx| {
                if view.plan_epoch != epoch {
                    view.notice = Some(
                        "The folders changed while the question was open. Copy again to use the new view."
                            .into(),
                    );
                    cx.notify();
                } else {
                    view.run_copy_batch(batch, to_root, background, window, cx)
                }
            });
        })
        .detach();
    }

    /// Del, Shift+Del and the Delete items: plans off the UI thread, asks, plans again after
    /// the answer, and moves the files to the Recycle Bin with progress.
    pub(super) fn delete_selection(
        &mut self,
        ask: DeleteAsk,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.refuse_while_working(cx) {
            return;
        }
        self.notice = None;
        let Some(model) = &self.model else {
            return;
        };
        let snap = Arc::new(Snapshot::new(model, &self.left, &self.right));
        let sides: Vec<Side> = match ask {
            DeleteAsk::Sides(DeleteSides::One(side)) => vec![side],
            _ => vec![Side::Left, Side::Right],
        };
        let one_side = matches!(ask, DeleteAsk::Sides(DeleteSides::One(_)));
        let (epoch, trash) = (self.plan_epoch, self.trash.clone());
        let background = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let (s, asked) = (snap.clone(), sides.clone());
            let plans: Vec<(Side, TrashPlan)> = background
                .spawn(async move { asked.into_iter().map(|side| (side, s.plan(side))).collect() })
                .await;
            let have: Vec<Side> = plans
                .iter()
                .filter(|(_, p)| !p.paths.is_empty())
                .map(|(side, _)| *side)
                .collect();
            if have.is_empty() {
                let text = nothing_to_delete(&plans, one_side);
                let _ = this.update(cx, |view, cx| {
                    view.notice = Some(text.into());
                    cx.notify();
                });
                return;
            }
            let (message, detail) = trash_prompt(&plans);
            // Each answer after "Cancel" is the sides it takes.
            let (message, labels, picks) = match ask {
                DeleteAsk::Choose => {
                    let (labels, picks) = delete_answers(&have);
                    ("Move to the Recycle Bin?".to_owned(), labels, picks)
                }
                DeleteAsk::Sides(_) => (message, vec!["Cancel", "Move to Recycle Bin"], vec![have]),
            };
            let Ok(answer) = this.update_in(cx, |_, window, cx| {
                window.prompt(PromptLevel::Warning, &message, Some(&detail), &labels, cx)
            }) else {
                return;
            };
            let Some(chosen) = answer
                .await
                .ok()
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| picks.into_iter().nth(i))
            else {
                return;
            };
            let asked: Vec<PathBuf> = plans
                .iter()
                .filter(|(side, _)| chosen.contains(side))
                .flat_map(|(_, p)| p.paths.clone())
                .collect();
            // Plan again on the worker: the disk may have moved on while the question was open.
            let stale = "The folders changed while the question was open. Delete again to use the new view.";
            let still_current = this.update(cx, |view, _| view.plan_epoch == epoch);
            if !matches!(still_current, Ok(true)) {
                let _ = this.update(cx, |view, cx| {
                    view.notice = Some(stale.into());
                    cx.notify();
                });
                return;
            }
            let (fresh, changed, inside_prompt) = background
                .spawn(async move {
                    let plans: Vec<(Side, TrashPlan)> =
                        chosen.iter().map(|&side| (side, snap.plan(side))).collect();
                    let changed: usize = plans.iter().map(|(_, p)| p.changed).sum();
                    let inside_prompt = plans.iter().all(|(_, p)| within_prompt(&p.paths, &asked));
                    // Each path keeps its side, so the run can check below that side's root.
                    let fresh: Vec<(Side, PathBuf)> = plans
                        .into_iter()
                        .flat_map(|(side, p)| p.paths.into_iter().map(move |path| (side, path)))
                        .collect();
                    (fresh, changed, inside_prompt)
                })
                .await;
            let _ = this.update_in(cx, |view, window, cx| {
                let notice = if view.plan_epoch != epoch || !inside_prompt {
                    Some(stale)
                } else if fresh.is_empty() {
                    Some("Nothing left to delete: the folders changed while the question was open.")
                } else {
                    None
                };
                match notice {
                    Some(text) => {
                        view.notice = Some(text.into());
                        cx.notify();
                    }
                    None => {
                        view.run_trash_batch(fresh, changed, trash, background, window, cx)
                    }
                }
            });
        })
        .detach();
    }

    /// The delete itself, over the plan made after the answer. The roots are the plan's: the
    /// caller checked that no compare or root change came in between (`plan_epoch`).
    fn run_trash_batch(
        &mut self,
        paths: Vec<(Side, PathBuf)>,
        skipped: usize,
        trash: TrashFn,
        background: BackgroundExecutor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.refuse_while_working(cx) {
            return;
        }
        let total = paths.len();
        let (left, right) = (self.left.clone(), self.right.clone());
        let (cancel, done) = self.start_batch("Deleting", total, cx);
        cx.spawn_in(window, async move |this, cx| {
            let report = background
                .spawn(
                    async move { run_trash_below(&left, &right, &paths, &*trash, &cancel, &done) },
                )
                .await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.finish_batch(
                    trash_result(&report, total, skipped),
                    &report.failed,
                    window,
                    cx,
                )
            });
        })
        .detach();
    }

    /// Sets the modified time of the selection's files on the other side to match `from`'s.
    pub(super) fn touch(&mut self, from: Side, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_while_working(cx) {
            return;
        }
        self.notice = None;
        let Some(model) = &self.model else {
            return;
        };
        let cmp = model.comparison();
        let ex = expand(cmp, &model.picked(), &|n| model.passes_filters(n));
        let pairs = touch_pairs(cmp, &ex, from, &self.left, &self.right);
        if pairs.is_empty() {
            self.notice = Some("Touch needs files that exist on both sides.".into());
            cx.notify();
            return;
        }
        let (to, dst_root) = match from {
            Side::Left => (Side::Right, self.right.clone()),
            Side::Right => (Side::Left, self.left.clone()),
        };
        let total = pairs.len();
        let (cancel, done) = self.start_batch("Touching", total, cx);
        let background = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let report = background
                .spawn(async move { run_touch(&dst_root, &pairs, &cancel, &done) })
                .await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.finish_batch(touch_result(&report, total, to), &report.failed, window, cx)
            });
        })
        .detach();
    }

    /// Test hook: replaces the Recycle Bin call (tests must never use the real one).
    #[doc(hidden)]
    pub fn set_trash_for_test(
        &mut self,
        f: impl Fn(&Path) -> Result<(), ActionError> + Send + Sync + 'static,
    ) {
        self.trash = Arc::new(f);
    }

    fn run_copy_batch(
        &mut self,
        batch: CopyBatch,
        dst_root: PathBuf,
        background: gpui_kit::BackgroundExecutor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.refuse_while_working(cx) {
            return;
        }
        let (files, folders) = (batch.files.len(), batch.mkdirs.len());
        let total = files + folders;
        let (cancel, done) = self.start_batch("Copying", total, cx);
        cx.spawn_in(window, async move |this, cx| {
            let report = background
                .spawn(async move { run_copy(&dst_root, &batch, &cancel, &done) })
                .await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.finish_batch(
                    copy_result(&report, files, folders),
                    &report.failed,
                    window,
                    cx,
                )
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use duet_core::folder::plan::CopyItem;

    fn item(name: &str, len: u64, overwrites: bool) -> CopyItem {
        CopyItem {
            src: PathBuf::from("L").join(name),
            dst: PathBuf::from("R").join(name),
            len,
            overwrites,
        }
    }

    #[test]
    fn copy_prompt_says_files_bytes_and_overwrites_and_picks_the_default() {
        let batch = CopyBatch {
            files: vec![item("a", 1024, true), item("b", 2048, false)],
            mkdirs: vec!["R/empty".into()],
            bytes: 3072,
        };
        let (message, detail, answers, confirm) = copy_prompt(&batch, Direction::LeftToRight);
        assert_eq!(message, "Copy 2 files (3,072 bytes) from left to right?");
        assert!(
            detail.contains("Overwrites 1 file on the right."),
            "{detail}"
        );
        assert!(detail.contains("Creates 1 empty folder."), "{detail}");
        assert_eq!(answers, ["Cancel", "Copy"]);
        assert_eq!(confirm, 1, "an overwrite makes Cancel the default");
        let safe = CopyBatch {
            files: vec![batch.files[1].clone()],
            mkdirs: vec![],
            bytes: 2048,
        };
        let (_, _, answers, confirm) = copy_prompt(&safe, Direction::LeftToRight);
        assert_eq!((answers, confirm), (vec!["Copy", "Cancel"], 0));
    }

    #[test]
    fn the_prompt_counts_newer_overwrites_and_caps_the_list() {
        let conflict = |name: &str, destination_newer| Conflict {
            path: PathBuf::from("D:/R").join(name),
            destination_newer,
        };
        let many: Vec<_> = (0..8)
            .map(|i| conflict(&format!("f{i}.txt"), i == 7))
            .collect();
        let batch = CopyBatch {
            files: (0..8)
                .map(|i| item(&format!("f{i}.txt"), 1, true))
                .collect(),
            mkdirs: vec![],
            bytes: 8,
        };
        let (message, detail, _, _) = copy_prompt_with(&batch, Direction::RightToLeft, &many);
        assert_eq!(message, "Copy 8 files (8 bytes) from right to left?");
        assert!(
            detail.starts_with("Overwrites 8 files on the left."),
            "{detail}"
        );
        assert!(detail.contains("1 of them is newer"), "{detail}");
        assert!(
            detail.contains("f7.txt"),
            "the newer one is listed first: {detail}"
        );
        assert!(!detail.contains("f6.txt"), "{detail}");
        assert!(detail.ends_with("…and 2 more"), "{detail}");
        let only_folders = CopyBatch {
            mkdirs: vec!["R/a".into(), "R/b".into()],
            ..CopyBatch::default()
        };
        let (message, detail, _, _) = copy_prompt(&only_folders, Direction::LeftToRight);
        assert_eq!(message, "Copy 2 empty folders from left to right?");
        assert_eq!(detail, "");
    }

    #[test]
    fn trash_prompt_counts_lists_six_and_mentions_kept_folders() {
        let plan = TrashPlan {
            paths: (0..8).map(|i| PathBuf::from(format!("L/f{i}"))).collect(),
            kept_folders: 2,
            changed: 0,
            ..Default::default()
        };
        let (message, detail) = trash_prompt(&[(Side::Left, plan)]);
        assert_eq!(message, "Move 8 items to the Recycle Bin?");
        assert!(
            detail.contains("L/f5") && !detail.contains("L/f6") && detail.contains("…and 2 more"),
            "{detail}"
        );
        assert!(
            detail.contains(
                "2 folders still hold files that are not shown, so they stay; only the files you see go."
            ),
            "{detail}"
        );
        let one = TrashPlan {
            paths: vec!["R/a".into()],
            kept_folders: 1,
            changed: 0,
            ..Default::default()
        };
        let (message, detail) = trash_prompt(&[(Side::Right, one)]);
        assert_eq!(message, "Move 1 item to the Recycle Bin?");
        assert!(
            detail.contains(
                "1 folder still holds files that are not shown, so it stays; only the files you see go."
            ),
            "{detail}"
        );
    }

    #[test]
    fn delete_answers_start_with_cancel_and_offer_only_sides_that_have_paths() {
        let (labels, picks) = delete_answers(&[Side::Left, Side::Right]);
        assert_eq!(
            labels,
            ["Cancel", "Delete left", "Delete right", "Delete both"]
        );
        assert_eq!(
            picks,
            [
                vec![Side::Left],
                vec![Side::Right],
                vec![Side::Left, Side::Right]
            ]
        );
        let (labels, picks) = delete_answers(&[Side::Right]);
        assert_eq!(labels, ["Cancel", "Delete right"]);
        assert_eq!(picks, [vec![Side::Right]]);
        let (labels, picks) = delete_answers(&[Side::Left]);
        assert_eq!(
            (labels, picks),
            (vec!["Cancel", "Delete left"], vec![vec![Side::Left]])
        );
        assert_eq!(delete_answers(&[]).0, ["Cancel"]);
    }

    #[test]
    fn the_prompt_counts_each_side_and_names_changed_items() {
        let side = |n: usize, kept_folders, changed| TrashPlan {
            paths: (0..n).map(|i| PathBuf::from(format!("p{i}"))).collect(),
            kept_folders,
            changed,
            ..Default::default()
        };
        let (_, detail) =
            trash_prompt(&[(Side::Left, side(5, 0, 0)), (Side::Right, side(3, 1, 2))]);
        assert!(
            detail.starts_with("Left: 5 items. Right: 3 items."),
            "{detail}"
        );
        assert!(detail.contains("1 folder still holds files"), "{detail}");
        assert!(
            detail.contains("2 items changed on disk since the compare and are left alone."),
            "{detail}"
        );
        let (_, detail) =
            trash_prompt(&[(Side::Left, side(2, 0, 0)), (Side::Right, side(0, 3, 0))]);
        assert!(
            !detail.contains("Left:"),
            "one side only, no per-side line: {detail}"
        );
        assert!(
            detail.contains("3 folders still hold files"),
            "kept counts survive an empty side: {detail}"
        );
    }

    #[test]
    fn checking_a_big_plan_against_the_prompt_stays_linear() {
        let asked: Vec<PathBuf> = (0..20_000)
            .map(|i| PathBuf::from(format!("L/dir{}/file{i}.txt", i % 50)))
            .collect();
        let fresh = asked.clone();
        let start = std::time::Instant::now();
        assert!(within_prompt(&fresh, &asked));
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "took {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn a_fresh_plan_may_only_shrink_to_what_the_prompt_listed() {
        let asked = [PathBuf::from("L/d/a.txt"), PathBuf::from("L/b")];
        assert!(within_prompt(&["L/d/a.txt".into()], &asked));
        assert!(
            within_prompt(&["L/b/inner".into()], &asked),
            "under a listed folder"
        );
        assert!(
            !within_prompt(&["L/d".into()], &asked),
            "a parent of a listed file"
        );
        assert!(
            !within_prompt(&["L/bb".into()], &asked),
            "component-wise, not a text prefix"
        );
        assert!(within_prompt(&[], &asked));
    }

    #[test]
    fn the_trash_and_touch_notices_name_success_failure_and_cancel() {
        let ok = BatchReport {
            done: 12,
            ..BatchReport::default()
        };
        assert_eq!(
            trash_result(&ok, 12, 0),
            "Moved 12 items to the Recycle Bin."
        );
        let failed = BatchReport {
            done: 2,
            failed: vec![("L/x".into(), "denied".into())],
            ..BatchReport::default()
        };
        let text = trash_result(&failed, 3, 0);
        assert!(text.starts_with("Moved 2 items; 1 failed: "), "{text}");
        let cancelled = BatchReport {
            done: 1,
            cancelled: true,
            ..BatchReport::default()
        };
        assert_eq!(
            trash_result(&cancelled, 5, 0),
            "Delete cancelled after 1 of 5 items."
        );
        assert_eq!(
            touch_result(&ok, 12, Side::Right),
            "Set the modified time of 12 files on the right to match the left."
        );
        assert_eq!(
            touch_result(
                &BatchReport {
                    done: 1,
                    ..BatchReport::default()
                },
                1,
                Side::Left
            ),
            "Set the modified time of 1 file on the left to match the right."
        );
    }

    #[test]
    fn the_result_notice_names_success_failure_and_cancel() {
        let ok = BatchReport {
            done: 7,
            ..BatchReport::default()
        };
        assert_eq!(copy_result(&ok, 7, 0), "Copied 7 files.");
        let failed = BatchReport {
            done: 5,
            failed: vec![
                ("R/x".into(), "denied".into()),
                ("R/y".into(), "full".into()),
            ],
            ..BatchReport::default()
        };
        let text = copy_result(&failed, 7, 0);
        assert!(text.starts_with("Copied 5 files; 2 failed: "), "{text}");
        assert!(text.ends_with(": denied."), "{text}");
        let cancelled = BatchReport {
            done: 3,
            cancelled: true,
            ..BatchReport::default()
        };
        assert_eq!(
            copy_result(&cancelled, 7, 0),
            "Copy cancelled after 3 of 7 files."
        );
    }

    #[test]
    fn a_copy_of_only_empty_folders_says_created_n_folders() {
        let ok = BatchReport {
            done: 2,
            folders: 2,
            ..BatchReport::default()
        };
        assert_eq!(copy_result(&ok, 0, 2), "Created 2 folders.");
        let one = BatchReport {
            done: 1,
            folders: 1,
            ..BatchReport::default()
        };
        assert_eq!(copy_result(&one, 0, 1), "Created 1 folder.");
        let failed = BatchReport {
            done: 1,
            folders: 1,
            failed: vec![("R/x".into(), "denied".into())],
            ..BatchReport::default()
        };
        let text = copy_result(&failed, 0, 2);
        assert!(text.starts_with("Created 1 folder; 1 failed: "), "{text}");
        let all_failed = BatchReport {
            failed: vec![
                ("R/x".into(), "denied".into()),
                ("R/y".into(), "denied".into()),
            ],
            ..BatchReport::default()
        };
        let text = copy_result(&all_failed, 0, 2);
        assert!(text.starts_with("Created 0 folders; 2 failed: "), "{text}");
    }

    #[test]
    fn the_copy_notice_splits_files_from_folders() {
        let ok = BatchReport {
            done: 7,
            folders: 2,
            ..BatchReport::default()
        };
        assert_eq!(
            copy_result(&ok, 5, 2),
            "Copied 5 files and created 2 folders."
        );
        let one = BatchReport {
            done: 2,
            folders: 1,
            ..BatchReport::default()
        };
        assert_eq!(
            copy_result(&one, 1, 1),
            "Copied 1 file and created 1 folder."
        );
        let failed = BatchReport {
            done: 6,
            folders: 2,
            failed: vec![("R/x".into(), "denied".into())],
            ..BatchReport::default()
        };
        let text = copy_result(&failed, 5, 3);
        assert!(
            text.starts_with("Copied 4 files and created 2 folders; 1 failed: "),
            "{text}"
        );
        let cancelled = BatchReport {
            done: 6,
            cancelled: true,
            ..BatchReport::default()
        };
        assert_eq!(
            copy_result(&cancelled, 5, 3),
            "Copy cancelled after 5 of 5 files and 1 of 3 folders."
        );
    }

    #[test]
    fn failure_prompts_cap_at_200_lines() {
        assert_eq!(failure_prompt(&[]), None);
        let many: Vec<(PathBuf, String)> = (0..205)
            .map(|i| (PathBuf::from(format!("R/f{i}")), "denied".to_owned()))
            .collect();
        let (message, detail) = failure_prompt(&many).unwrap();
        assert_eq!(message, "205 items failed.");
        let lines: Vec<&str> = detail.lines().collect();
        assert_eq!(lines.len(), 201, "{detail}");
        assert_eq!(
            lines[0],
            format!("{}: denied", PathBuf::from("R/f0").display())
        );
        assert_eq!(
            lines[199],
            format!("{}: denied", PathBuf::from("R/f199").display())
        );
        assert_eq!(lines[200], "…and 5 more");
        let (message, detail) = failure_prompt(&many[..1]).unwrap();
        assert_eq!(message, "1 item failed.");
        assert!(!detail.contains("more"), "{detail}");
    }
}
