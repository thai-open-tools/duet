//! Folder sync (R14, R16): what each of the five modes copies, moves to the Recycle Bin or
//! skips, and the run that carries a plan out. Planning reads the disk only in mirror modes:
//! `trash_plan`, and the times of the folders it moves and of the folders inside them.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::time::SystemTime;

use crate::cancel::{CancelToken, Cancelled};
use crate::folder::actions::{ActionError, ensure_no_links_below, roots_overlap};
use crate::folder::classify::Warning;
use crate::folder::entry::{Entry, EntryKind, join_rel};
use crate::folder::plan::{
    BatchReport, CopyBatch, CopyItem, Expanded, children, copy_batch, dir_like, entry_on, expand,
    other, outermost_picks, root_of, run_copy_checked, run_trash, trash_plan_with_cancel,
};
use crate::folder::run::CompareMode;
use crate::folder::tree::{FolderComparison, Node, Side, Status};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    UpdateRight,
    UpdateLeft,
    UpdateBoth,
    MirrorRight,
    MirrorLeft,
}

impl SyncMode {
    pub const ALL: [SyncMode; 5] = [
        SyncMode::UpdateRight,
        SyncMode::UpdateLeft,
        SyncMode::UpdateBoth,
        SyncMode::MirrorRight,
        SyncMode::MirrorLeft,
    ];

    /// Whether items only `side` holds are copied to the other side.
    fn copies_from(self, side: Side) -> bool {
        match self {
            SyncMode::UpdateBoth => true,
            SyncMode::UpdateRight | SyncMode::MirrorRight => side == Side::Left,
            SyncMode::UpdateLeft | SyncMode::MirrorLeft => side == Side::Right,
        }
    }

    /// The side a mirror makes like the other: what only it holds goes to the Recycle Bin.
    fn trash_side(self) -> Option<Side> {
        match self {
            SyncMode::MirrorRight => Some(Side::Right),
            SyncMode::MirrorLeft => Some(Side::Left),
            _ => None,
        }
    }

    /// A pair that differs: the side to copy from, or why it stays. An update never replaces
    /// the newer side, and copies nothing when the times cannot tell which is newer.
    fn copy_for_difference(self, newer: Option<Side>) -> Result<Side, SkipReason> {
        match self {
            SyncMode::MirrorRight => Ok(Side::Left),
            SyncMode::MirrorLeft => Ok(Side::Right),
            _ => match newer {
                None => Err(SkipReason::SameTime),
                Some(side) if self.copies_from(side) => Ok(side),
                Some(side) => Err(SkipReason::Newer(side)),
            },
        }
    }
}

/// Why a sync leaves an item alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The update would replace this newer side.
    Newer(Side),
    SameTime,
    CaseOnly,
    KeptFolder,
    Changed,
    Unreadable,
    Kind,
    Link,
}

impl SkipReason {
    pub fn text(&self) -> &'static str {
        match self {
            SkipReason::Newer(Side::Right) => crate::i18n::tr_key("the right is newer"),
            SkipReason::Newer(Side::Left) => crate::i18n::tr_key("the left is newer"),
            SkipReason::SameTime => crate::i18n::tr_key(
                "the modified times are equal or near: can't tell which is newer",
            ),
            SkipReason::CaseOnly => crate::i18n::tr_key(
                "the names differ only in case and the contents match: rename it by hand",
            ),
            SkipReason::KeptFolder => crate::i18n::tr_key(
                "holds files that are not in the compare (filtered out, links or unreadable): the folder stays",
            ),
            SkipReason::Changed => crate::i18n::tr_key("it changed on disk since the compare"),
            SkipReason::Unreadable => crate::i18n::tr_key("it could not be read"),
            SkipReason::Kind => {
                crate::i18n::tr_key("a file on one side and a folder or link on the other")
            }
            SkipReason::Link => crate::i18n::tr_key("links are never copied"),
        }
    }

    /// [`Self::text`] for a compare made in `mode`: Quick and Size only never read a file, so
    /// a case-only pair is said to match in what they compared, not in its contents (PF13).
    pub fn text_in(&self, mode: CompareMode) -> &'static str {
        match (self, mode) {
            (SkipReason::CaseOnly, CompareMode::Quick) => crate::i18n::tr_key(
                "the names differ only in case and the size and modified time match: rename it by hand",
            ),
            (SkipReason::CaseOnly, CompareMode::Size) => crate::i18n::tr_key(
                "the names differ only in case and the size matches: rename it by hand",
            ),
            _ => self.text(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncSkip {
    /// Relative, '/'-separated; a folder's ends with "/".
    pub rel: String,
    pub why: SkipReason,
}

/// What a plan saw at a path it replaces or moves to the Recycle Bin, so the run can tell
/// whether the path changed since: its kind, and a file's size and modified time as the
/// compare recorded them. A folder's time is read when the plan is made (after `trash_plan`
/// matched its listing to the compare), so the run compares two reads of the same kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    pub kind: EntryKind,
    pub len: u64,
    pub modified: Option<SystemTime>,
}

impl Seen {
    fn of(entry: &Entry) -> Self {
        Self {
            kind: entry.kind,
            len: entry.len,
            modified: entry.modified,
        }
    }

    /// A folder as it is on disk now.
    fn folder_at(path: &Path) -> Self {
        Self {
            kind: EntryKind::Dir,
            len: 0,
            modified: fs::symlink_metadata(path).and_then(|m| m.modified()).ok(),
        }
    }

    /// Whether `path` is still what was seen: the same kind, and for a file the same size and
    /// time (for a folder, the same time). A path that cannot be read is not.
    fn still_at(&self, path: &Path) -> bool {
        let Ok(meta) = fs::symlink_metadata(path) else {
            return false;
        };
        let file_type = meta.file_type();
        let same_time = self
            .modified
            .is_none_or(|seen| meta.modified().is_ok_and(|now| now == seen));
        match self.kind {
            EntryKind::File => file_type.is_file() && meta.len() == self.len && same_time,
            EntryKind::Dir => file_type.is_dir() && same_time,
            EntryKind::Symlink => file_type.is_symlink(),
            EntryKind::Unknown => !file_type.is_dir() && !file_type.is_symlink(),
        }
    }
}

/// Everything a sync will do. Every overwrite is a `CopyItem` with `overwrites` set and every
/// delete is in `trash`; nothing else is written or moved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncPlan {
    pub to_right: CopyBatch,
    pub to_left: CopyBatch,
    pub trash: Vec<PathBuf>,
    /// The side `trash` is on (mirror modes).
    pub trash_side: Option<Side>,
    pub skips: Vec<SyncSkip>,
    /// What the plan saw at the destination of every overwrite and at every path in `trash`.
    /// `run_sync` leaves a path that no longer matches ("Changed since the compare.").
    pub seen: BTreeMap<PathBuf, Seen>,
    /// What the plan saw below each folder of `trash`, which goes whole: every file as the
    /// compare recorded it, and every folder's time when the plan was made. `run_sync` moves
    /// such a folder only while all of it still matches.
    pub inside: BTreeMap<PathBuf, Seen>,
    /// The items of the scope left alone because they changed on disk since the compare (each
    /// is also a Changed skip).
    pub changed: Vec<PathBuf>,
    /// Pairs the plan treats as a move: the `rel` of a file it copies to the `rel` of its twin
    /// it sends to the Recycle Bin, and the reverse. For the preview's wording only.
    pub moves: BTreeMap<String, String>,
}

impl SyncPlan {
    fn batches(&self) -> [&CopyBatch; 2] {
        [&self.to_right, &self.to_left]
    }

    /// Nothing to copy, make or move.
    pub fn is_empty(&self) -> bool {
        self.to_right.is_empty() && self.to_left.is_empty() && self.trash.is_empty()
    }

    pub fn files(&self) -> usize {
        self.batches().iter().map(|b| b.files.len()).sum()
    }

    pub fn bytes(&self) -> u64 {
        self.batches().iter().map(|b| b.bytes).sum()
    }

    /// Files the copies replace, both ways (the panel asks before any, PF1).
    pub fn overwrites(&self) -> usize {
        self.batches().iter().map(|b| b.overwrites()).sum()
    }

    /// Empty folders made, both ways.
    pub fn folders(&self) -> usize {
        self.batches().iter().map(|b| b.mkdirs.len()).sum()
    }

    /// F-100: `self` is the plan made again at the run, `asked` the one the question named. A
    /// folder `asked` took whole, or one inside it, that `self` keeps because it now holds
    /// something the compare never saw (or can no longer be listed) changed after the
    /// question. It is left alone whole, as `run_sync` leaves a folder whose time changed
    /// (I-2): nothing below it goes, its skip says it changed (its "/" kept), and it joins
    /// `changed`, so the run names it.
    pub fn leave_folders_grown_since(
        &mut self,
        asked: &SyncPlan,
        left_root: &Path,
        right_root: &Path,
    ) {
        let Some(side) = self.trash_side else {
            return;
        };
        let root = root_of(side, left_root, right_root);
        let named: HashSet<&Path> = asked.trash.iter().map(PathBuf::as_path).collect();
        let mut grown = Vec::new();
        for skip in &mut self.skips {
            if skip.why != SkipReason::KeptFolder {
                continue;
            }
            let path = join_rel(root, &skip.rel);
            if path.ancestors().any(|a| named.contains(a)) {
                skip.why = SkipReason::Changed;
                grown.push(path);
            }
        }
        self.trash
            .retain(|p| !grown.iter().any(|folder| p.starts_with(folder)));
        self.changed.extend(grown);
    }
}

fn entries(node: &Node) -> impl Iterator<Item = &Entry> {
    node.left.iter().chain(node.right.iter())
}

/// Some side could not be read: an error row, an entry with an error, or one of unknown kind.
pub(super) fn unreadable(node: &Node) -> bool {
    matches!(node.status, Status::Error(_))
        || entries(node).any(|e| e.error.is_some() || e.kind == EntryKind::Unknown)
}

/// Both sides hold it, as different kinds: a file against a folder or a link.
fn kind_clash(node: &Node) -> bool {
    matches!((&node.left, &node.right), (Some(l), Some(r)) if l.kind != r.kind)
}

/// Per node, what an ancestor stops it with: nothing below a folder that could not be read
/// (its other side only looks one-sided) or below a kind clash is synced. Parents precede
/// their children, so one forward pass sees every ancestor first.
fn stopped_above(cmp: &FolderComparison) -> Vec<Option<SkipReason>> {
    let mut out: Vec<Option<SkipReason>> = vec![None; cmp.nodes.len()];
    for i in 0..cmp.nodes.len() {
        if let Some(p) = cmp.nodes[i].parent {
            let parent = &cmp.nodes[p as usize];
            out[i] = out[p as usize].or(if unreadable(parent) {
                Some(SkipReason::Unreadable)
            } else if kind_clash(parent) {
                Some(SkipReason::Kind)
            } else {
                None
            });
        }
    }
    out
}

/// The rel a skip names: a folder's ends with "/".
fn skip_of(node: &Node, why: SkipReason) -> SyncSkip {
    let folder = entries(node).all(|e| e.kind == EntryKind::Dir);
    SyncSkip {
        rel: if folder {
            format!("{}/", node.rel)
        } else {
            node.rel.clone()
        },
        why,
    }
}

/// What a mode does with one row of the scope.
enum Act {
    Nothing,
    Copy { from: Side },
    Skip(SkipReason),
}

/// R14 for a file row, or a one-sided empty folder. Items only the target of a mirror holds
/// are left to the Recycle Bin walk. The guards outrank the table: what stops the row above,
/// then unreadable, case only, a kind clash and links.
fn decide(node: &Node, mode: SyncMode, above: Option<SkipReason>) -> Act {
    let wanted = match (&node.left, &node.right) {
        (Some(_), None) => mode.copies_from(Side::Left).then_some(Ok(Side::Left)),
        (None, Some(_)) => mode.copies_from(Side::Right).then_some(Ok(Side::Right)),
        (Some(_), Some(_)) => match node.status {
            Status::Different { newer } => Some(mode.copy_for_difference(newer)),
            Status::Error(_) => Some(Err(SkipReason::Unreadable)),
            // Same and ≈ need nothing; nor does Pending (sync is refused while rows are).
            _ => None,
        },
        (None, None) => None,
    };
    let Some(wanted) = wanted else {
        return Act::Nothing;
    };
    let guard = above
        .or_else(|| unreadable(node).then_some(SkipReason::Unreadable))
        .or_else(|| (node.warning() == Some(Warning::CaseOnly)).then_some(SkipReason::CaseOnly))
        .or_else(|| kind_clash(node).then_some(SkipReason::Kind))
        .or_else(|| {
            entries(node)
                .any(|e| e.kind == EntryKind::Symlink)
                .then_some(SkipReason::Link)
        });
    match (guard, wanted) {
        (Some(why), _) | (None, Err(why)) => Act::Skip(why),
        (None, Ok(from)) => Act::Copy { from },
    }
}

/// `copy_batch` over the files and folders chosen to copy from `from`. It keeps their order
/// and drops only what it cannot write (another kind on the way); each of those becomes a
/// Kind skip, so nothing chosen vanishes from the plan. What the compare saw at each file it
/// replaces goes into `seen`.
#[allow(clippy::too_many_arguments)]
fn batch_for(
    cmp: &FolderComparison,
    files: Vec<u32>,
    dirs: Vec<u32>,
    from: Side,
    left_root: &Path,
    right_root: &Path,
    skipped: &mut Vec<(u32, SyncSkip)>,
    seen: &mut BTreeMap<PathBuf, Seen>,
) -> CopyBatch {
    let ex = Expanded {
        files,
        empty_dirs: dirs,
    };
    let batch = copy_batch(cmp, &ex, from, left_root, right_root);
    let src_root = root_of(from, left_root, right_root);
    let dst_root = root_of(other(from), left_root, right_root);
    let mut at = 0;
    for &n in &ex.files {
        let item = entry_on(cmp, n, from).and_then(|e| {
            batch
                .files
                .get(at)
                .filter(|f| f.src == join_rel(src_root, &e.rel))
        });
        match item {
            Some(item) => {
                if item.overwrites
                    && let Some(dst) = entry_on(cmp, n, other(from))
                {
                    seen.insert(item.dst.clone(), Seen::of(dst));
                }
                at += 1;
            }
            None => skipped.push((n, skip_of(&cmp.nodes[n as usize], SkipReason::Kind))),
        }
    }
    let mut at = 0;
    for &n in &ex.empty_dirs {
        let listed = entry_on(cmp, n, from).is_some_and(|e| {
            batch
                .mkdirs
                .get(at)
                .is_some_and(|d| *d == join_rel(dst_root, &e.rel))
        });
        if listed {
            at += 1;
        } else {
            skipped.push((n, skip_of(&cmp.nodes[n as usize], SkipReason::Kind)));
        }
    }
    batch
}

/// Walks the scope as `expand` does. A folder pair that could not be read is named once; for
/// a mirror, the outermost items only `target` holds are the picks for `trash_plan`, unless
/// something above or the item itself stops them (then they are skips).
fn walk_scope(
    cmp: &FolderComparison,
    picked: &[u32],
    shown: &dyn Fn(u32) -> bool,
    target: Option<Side>,
    above: &[Option<SkipReason>],
) -> (Vec<u32>, Vec<(u32, SyncSkip)>) {
    let kids = children(cmp);
    let (mut picks, mut skipped) = (Vec::new(), Vec::new());
    for root in outermost_picks(cmp, picked) {
        let mut stack = vec![(root, true)];
        while let Some((n, top)) = stack.pop() {
            // The filters hide everything below a hidden row, as they show its folders.
            if !(top || shown(n)) {
                continue;
            }
            let node = &cmp.nodes[n as usize];
            let both = node.left.is_some() && node.right.is_some();
            if both && entries(node).all(|e| e.kind == EntryKind::Dir) && unreadable(node) {
                skipped.push((n, skip_of(node, SkipReason::Unreadable)));
            }
            if let Some(side) = target
                && entry_on(cmp, n, side).is_some()
                && entry_on(cmp, n, other(side)).is_none()
            {
                let stop = above[n as usize]
                    .or_else(|| unreadable(node).then_some(SkipReason::Unreadable));
                match stop {
                    Some(why) => skipped.push((n, skip_of(node, why))),
                    None => picks.push(n),
                }
                continue;
            }
            if dir_like(node) {
                stack.extend(kids[n as usize].iter().rev().map(|&c| (c, false)));
            }
        }
    }
    (picks, skipped)
}

/// What a folder that goes whole holds on `side`, for `SyncPlan::inside`: each file as the
/// compare recorded it and each folder's time now (I-2). A file added two levels down moves
/// only its own folder's time, and a file edited in place moves none.
fn record_inside(
    cmp: &FolderComparison,
    kids: &[Vec<u32>],
    folder: u32,
    side: Side,
    root: &Path,
    inside: &mut BTreeMap<PathBuf, Seen>,
    cancel: &CancelToken,
) -> Result<(), Cancelled> {
    let mut stack = kids[folder as usize].clone();
    while let Some(n) = stack.pop() {
        cancel.check()?;
        let Some(entry) = entry_on(cmp, n, side) else {
            continue;
        };
        let path = join_rel(root, &entry.rel);
        if entry.kind == EntryKind::Dir {
            stack.extend(&kids[n as usize]);
            let seen = Seen::folder_at(&path);
            inside.insert(path, seen);
        } else {
            inside.insert(path, Seen::of(entry));
        }
    }
    Ok(())
}

/// `path` below `root`, '/'-separated (the whole path when it is not below `root`).
pub fn rel_under(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rel) => rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => path.display().to_string(),
    }
}

/// R14 over `picked` (scope "all" is every node without a parent with `shown` always true).
/// A picked row always counts; anything reached through a folder counts only when `shown`.
/// Reads the disk only in mirror modes: `trash_plan`, and the times of the folders it moves
/// and of the folders inside them.
pub fn sync_plan(
    cmp: &FolderComparison,
    picked: &[u32],
    shown: &dyn Fn(u32) -> bool,
    mode: SyncMode,
    left_root: &Path,
    right_root: &Path,
) -> SyncPlan {
    let never = CancelToken::new();
    // A fresh token is never cancelled.
    sync_plan_with_cancel(cmp, picked, shown, mode, left_root, right_root, &never)
        .unwrap_or_default()
}

/// [`sync_plan`], which stops once `cancel` is set: the batch plans the deletes again, and
/// Esc must not wait for the whole disk walk of a large mirror.
pub fn sync_plan_with_cancel(
    cmp: &FolderComparison,
    picked: &[u32],
    shown: &dyn Fn(u32) -> bool,
    mode: SyncMode,
    left_root: &Path,
    right_root: &Path,
    cancel: &CancelToken,
) -> Result<SyncPlan, Cancelled> {
    cancel.check()?;
    let above = stopped_above(cmp);
    let ex = expand(cmp, picked, shown);
    let mut skipped: Vec<(u32, SyncSkip)> = Vec::new();
    // (files, folders) to copy from the left, and from the right.
    let (mut rightward, mut leftward) = ((Vec::new(), Vec::new()), (Vec::new(), Vec::new()));
    let one_sided_dirs = ex.empty_dirs.iter().copied().filter(|&n| {
        let node = &cmp.nodes[n as usize];
        node.left.is_some() != node.right.is_some()
    });
    let rows = ex.files.iter().map(|&n| (n, false));
    for (n, is_dir) in rows.chain(one_sided_dirs.map(|n| (n, true))) {
        let node = &cmp.nodes[n as usize];
        match decide(node, mode, above[n as usize]) {
            Act::Nothing => {}
            Act::Skip(why) => skipped.push((n, skip_of(node, why))),
            Act::Copy { from } => {
                let (files, dirs) = match from {
                    Side::Left => &mut rightward,
                    Side::Right => &mut leftward,
                };
                if is_dir { dirs } else { files }.push(n);
            }
        }
    }
    let mut seen = BTreeMap::new();
    let (files, dirs) = rightward;
    let to_right = batch_for(
        cmp,
        files,
        dirs,
        Side::Left,
        left_root,
        right_root,
        &mut skipped,
        &mut seen,
    );
    let (files, dirs) = leftward;
    let to_left = batch_for(
        cmp,
        files,
        dirs,
        Side::Right,
        left_root,
        right_root,
        &mut skipped,
        &mut seen,
    );
    let target = mode.trash_side();
    let (picks, walked) = walk_scope(cmp, picked, shown, target, &above);
    skipped.extend(walked);
    skipped.sort_by_key(|(n, _)| *n);
    let mut plan = SyncPlan {
        to_right,
        to_left,
        skips: skipped.into_iter().map(|(_, s)| s).collect(),
        seen,
        ..SyncPlan::default()
    };
    if let Some(side) = target {
        let root = root_of(side, left_root, right_root);
        let found = trash_plan_with_cancel(cmp, &picks, shown, side, root, cancel)?;
        plan.skips.extend(found.kept_paths.iter().map(|p| SyncSkip {
            rel: format!("{}/", rel_under(root, p)),
            why: SkipReason::KeptFolder,
        }));
        for &n in &found.changed_nodes {
            let Some(e) = entry_on(cmp, n, side) else {
                continue;
            };
            let slash = if e.kind == EntryKind::Dir { "/" } else { "" };
            plan.skips.push(SyncSkip {
                rel: format!("{}{slash}", e.rel),
                why: SkipReason::Changed,
            });
            plan.changed.push(join_rel(root, &e.rel));
        }
        let kids = children(cmp);
        for (path, &n) in found.paths.iter().zip(&found.nodes) {
            match entry_on(cmp, n, side) {
                Some(e) if e.kind != EntryKind::Dir => {
                    plan.seen.insert(path.clone(), Seen::of(e));
                }
                _ => {
                    plan.seen.insert(path.clone(), Seen::folder_at(path));
                    record_inside(cmp, &kids, n, side, root, &mut plan.inside, cancel)?;
                }
            }
        }
        plan.trash = found.paths;
        plan.trash_side = Some(side);
        plan.moves = planned_moves(cmp, &plan, side, left_root, right_root);
    }
    Ok(plan)
}

/// The moved pairs of `cmp` whose one file the plan copies and whose twin it trashes.
fn planned_moves(
    cmp: &FolderComparison,
    plan: &SyncPlan,
    trash_side: Side,
    left_root: &Path,
    right_root: &Path,
) -> BTreeMap<String, String> {
    let mut moves = BTreeMap::new();
    if cmp.moved.is_empty() {
        return moves;
    }
    let copied: HashSet<&Path> = plan
        .batches()
        .iter()
        .flat_map(|b| b.files.iter().map(|item| item.dst.as_path()))
        .collect();
    let trashed: HashSet<&Path> = plan.trash.iter().map(PathBuf::as_path).collect();
    let copy_side = match trash_side {
        Side::Left => Side::Right,
        Side::Right => Side::Left,
    };
    // The copy lands on the side whose twin goes to the Recycle Bin.
    let trash_root = root_of(trash_side, left_root, right_root);
    for (&a, &b) in &cmp.moved {
        let (Some(from), Some(twin)) = (entry_on(cmp, a, copy_side), entry_on(cmp, b, trash_side))
        else {
            continue;
        };
        let landed = join_rel(trash_root, &from.rel);
        let gone = join_rel(trash_root, &twin.rel);
        if copied.contains(landed.as_path()) && gone.ancestors().any(|p| trashed.contains(p)) {
            moves.insert(from.rel.clone(), twin.rel.clone());
            moves.insert(twin.rel.clone(), from.rel.clone());
        }
    }
    moves
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub to_right: BatchReport,
    pub to_left: BatchReport,
    /// `None` when the Recycle Bin step did not run: a copy was cancelled, or the plan had
    /// nothing for it.
    pub trash: Option<BatchReport>,
}

impl SyncReport {
    pub fn cancelled(&self) -> bool {
        self.to_right.cancelled
            || self.to_left.cancelled
            || self.trash.as_ref().is_some_and(|t| t.cancelled)
    }

    pub fn failed(&self) -> Vec<(PathBuf, String)> {
        self.to_right
            .failed
            .iter()
            .chain(&self.to_left.failed)
            .chain(self.trash.iter().flat_map(|t| &t.failed))
            .cloned()
            .collect()
    }
}

/// R16's run order: copies to the right, copies to the left, then the Recycle Bin, only when
/// no copy was cancelled. Each copy writes with `copy_into(.., overwrites)`, so a file that
/// appeared after planning is never replaced. Just before a file is replaced or an item goes
/// to the Recycle Bin, it is compared with what the plan saw (`seen`, and `inside` for a
/// folder that goes whole); one that changed is left alone and fails with "Changed since the
/// compare.". Nested roots write nothing at all (the view refuses them first).
pub fn run_sync(
    plan: &SyncPlan,
    left_root: &Path,
    right_root: &Path,
    trash: &(dyn Fn(&Path) -> Result<(), ActionError> + Sync),
    cancel: &CancelToken,
    done: &AtomicUsize,
) -> SyncReport {
    if roots_overlap(left_root, right_root) {
        let refused = BatchReport {
            failed: vec![(
                right_root.to_path_buf(),
                crate::i18n::tr_key("one folder is inside the other, so nothing was synced").into(),
            )],
            ..BatchReport::default()
        };
        return SyncReport {
            to_right: refused,
            ..SyncReport::default()
        };
    }
    let unchanged = |path: &Path| match plan.seen.get(path) {
        Some(seen) if seen.still_at(path) => Ok(()),
        _ => Err(ActionError::Changed(path.to_path_buf())),
    };
    let replaced = |item: &CopyItem| {
        if item.overwrites {
            unchanged(&item.dst)
        } else {
            Ok(())
        }
    };
    let to_right = run_copy_checked(right_root, &plan.to_right, cancel, done, &replaced);
    let to_left = run_copy_checked(left_root, &plan.to_left, cancel, done, &replaced);
    let trash_root = plan.trash_side.map(|s| root_of(s, left_root, right_root));
    let checked_trash = |path: &Path| {
        // A folder on the way that became a link or junction after the plan would lead the
        // checks below, and the move, outside the compared tree. The item itself is checked
        // by its kind in `seen`.
        if let (Some(root), Some(parent)) = (trash_root, path.parent()) {
            ensure_no_links_below(root, parent)?;
        }
        unchanged(path)?;
        // A folder goes whole only while everything the plan saw below it still matches (I-2).
        let below = (Bound::Excluded(path), Bound::Unbounded);
        let changed_below = plan
            .inside
            .range::<Path, _>(below)
            .take_while(|(p, _)| p.starts_with(path))
            .any(|(p, seen)| !seen.still_at(p));
        if changed_below {
            return Err(ActionError::Changed(path.to_path_buf()));
        }
        trash(path)
    };
    let trash = (!to_right.cancelled && !to_left.cancelled && !plan.trash.is_empty())
        .then(|| run_trash(&plan.trash, &checked_trash, cancel, done));
    SyncReport {
        to_right,
        to_left,
        trash,
    }
}

#[cfg(test)]
mod tests;
