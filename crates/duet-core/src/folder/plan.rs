//! What a copy, delete or touch of a folder-view selection covers under the current filters,
//! and the batch runners that carry it out. Planning is pure except `trash_plan`, which reads
//! the disk (rule R4).

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::cancel::{CancelToken, Cancelled};
use crate::folder::actions::{self, ActionError};
use crate::folder::entry::{Entry, EntryKind, join_rel};
use crate::folder::tree::{FolderComparison, Side};

/// Child node indexes of every node, in tree order.
pub fn children(cmp: &FolderComparison) -> Vec<Vec<u32>> {
    let mut out = vec![Vec::new(); cmp.nodes.len()];
    for (i, node) in cmp.nodes.iter().enumerate() {
        if let Some(parent) = node.parent {
            out[parent as usize].push(i as u32);
        }
    }
    out
}

/// The files (and file-less folders) a pick covers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Expanded {
    pub files: Vec<u32>,
    pub empty_dirs: Vec<u32>,
}

/// `picked` without repeats, out-of-range rows, or rows inside another picked folder, so
/// nothing is done twice.
pub(crate) fn outermost_picks(cmp: &FolderComparison, picked: &[u32]) -> Vec<u32> {
    let pick_set: HashSet<u32> = picked.iter().copied().collect();
    let inside_a_pick = |n: u32| {
        let mut at = cmp.nodes[n as usize].parent;
        while let Some(p) = at {
            if pick_set.contains(&p) {
                return true;
            }
            at = cmp.nodes[p as usize].parent;
        }
        false
    };
    let mut seen = HashSet::new();
    picked
        .iter()
        .copied()
        .filter(|&n| (n as usize) < cmp.nodes.len() && seen.insert(n) && !inside_a_pick(n))
        .collect()
}

fn entries(node: &crate::folder::tree::Node) -> impl Iterator<Item = &Entry> {
    node.left.iter().chain(node.right.iter())
}

/// Some side holds this row as something other than a folder (a file, a link).
pub(crate) fn file_like(node: &crate::folder::tree::Node) -> bool {
    entries(node).any(|e| e.kind != EntryKind::Dir)
}

/// Some side holds this row as a folder.
pub(crate) fn dir_like(node: &crate::folder::tree::Node) -> bool {
    entries(node).any(|e| e.kind == EntryKind::Dir)
}

/// Expands `picked` rows into the files under them. A picked row always counts; anything
/// reached through a folder counts only when `shown`. A pick inside another picked folder is
/// dropped, so nothing is done twice. Folders with no file anywhere below them are listed in
/// `empty_dirs`, nested ones included.
pub fn expand(cmp: &FolderComparison, picked: &[u32], shown: &dyn Fn(u32) -> bool) -> Expanded {
    let kids = children(cmp);
    // Parents precede their children, so one reverse pass rolls "holds a file" upwards.
    let mut has_file = vec![false; cmp.nodes.len()];
    for i in (0..cmp.nodes.len()).rev() {
        if file_like(&cmp.nodes[i]) {
            has_file[i] = true;
        }
        if has_file[i]
            && let Some(parent) = cmp.nodes[i].parent
        {
            has_file[parent as usize] = true;
        }
    }
    let mut out = Expanded::default();
    for root in outermost_picks(cmp, picked) {
        let mut stack = vec![(root, true)];
        while let Some((n, top)) = stack.pop() {
            let wanted = top || shown(n);
            let node = &cmp.nodes[n as usize];
            if file_like(node) && wanted {
                out.files.push(n);
            }
            if dir_like(node) {
                if !has_file[n as usize] && wanted {
                    out.empty_dirs.push(n);
                }
                stack.extend(kids[n as usize].iter().rev().map(|&c| (c, false)));
            }
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyItem {
    pub src: PathBuf,
    pub dst: PathBuf,
    pub len: u64,
    pub overwrites: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopyBatch {
    pub files: Vec<CopyItem>,
    pub mkdirs: Vec<PathBuf>,
    pub bytes: u64,
}

impl CopyBatch {
    pub fn overwrites(&self) -> usize {
        self.files.iter().filter(|f| f.overwrites).count()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.mkdirs.is_empty()
    }
}

pub(crate) fn entry_on(cmp: &FolderComparison, n: u32, side: Side) -> Option<&Entry> {
    let node = &cmp.nodes[n as usize];
    match side {
        Side::Left => node.left.as_ref(),
        Side::Right => node.right.as_ref(),
    }
}

pub(crate) fn other(side: Side) -> Side {
    match side {
        Side::Left => Side::Right,
        Side::Right => Side::Left,
    }
}

pub(crate) fn root_of<'a>(side: Side, left_root: &'a Path, right_root: &'a Path) -> &'a Path {
    match side {
        Side::Left => left_root,
        Side::Right => right_root,
    }
}

/// Copies from side `from` to the other side. A destination that already exists keeps its
/// own spelling, so a case-only difference overwrites instead of adding a twin. Only regular
/// files are copied; links are never counted.
pub fn copy_batch(
    cmp: &FolderComparison,
    ex: &Expanded,
    from: Side,
    left_root: &Path,
    right_root: &Path,
) -> CopyBatch {
    let to = other(from);
    let (src_root, dst_root) = (
        root_of(from, left_root, right_root),
        root_of(to, left_root, right_root),
    );
    // A destination of another kind, or under a file, would only fail at run time.
    let blocked = |n: u32| {
        let mut at = cmp.nodes[n as usize].parent;
        while let Some(p) = at {
            if entry_on(cmp, p, to).is_some_and(|e| e.kind != EntryKind::Dir) {
                return true;
            }
            at = cmp.nodes[p as usize].parent;
        }
        false
    };
    let mut batch = CopyBatch::default();
    for &n in &ex.files {
        let Some(src) = entry_on(cmp, n, from).filter(|e| e.kind == EntryKind::File) else {
            continue;
        };
        let dst = entry_on(cmp, n, to);
        if blocked(n) || dst.is_some_and(|e| e.kind != EntryKind::File) {
            continue;
        }
        batch.bytes += src.len;
        batch.files.push(CopyItem {
            src: join_rel(src_root, &src.rel),
            dst: join_rel(dst_root, &dst.unwrap_or(src).rel),
            len: src.len,
            overwrites: dst.is_some(),
        });
    }
    for &n in &ex.empty_dirs {
        if let (Some(src), None) = (entry_on(cmp, n, from), entry_on(cmp, n, to))
            && src.kind == EntryKind::Dir
            && !blocked(n)
        {
            batch.mkdirs.push(join_rel(dst_root, &src.rel));
        }
    }
    batch
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrashPlan {
    pub paths: Vec<PathBuf>,
    /// Outermost folders left in place because something under them is hidden by the
    /// filters, unknown to the compare, or unreadable. A picked folder whose wanted contents
    /// are all hidden counts, even when nothing below it is taken. A folder kept only because
    /// a known entry vanished or changed kind does not count (the entry counts in `changed`),
    /// and folders nested in a counted one are not counted again.
    pub kept_folders: usize,
    /// The outermost kept folders, in the order `kept_folders` counts them.
    pub kept_paths: Vec<PathBuf>,
    /// The node of each of `paths`, in the same order.
    pub nodes: Vec<u32>,
    /// Picked or wanted items that are missing on disk now or are of another kind than the
    /// compare recorded, or (files) have another size or modified time. They and everything
    /// under them are left alone.
    pub changed: usize,
    /// The node of each item `changed` counts, in the same order, so a preview can name it.
    pub changed_nodes: Vec<u32>,
}

/// What the disk can tell apart about an entry: folder, link, or anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DiskKind {
    Dir,
    Link,
    Other,
}

fn disk_kind(kind: EntryKind) -> DiskKind {
    match kind {
        EntryKind::Dir => DiskKind::Dir,
        EntryKind::Symlink => DiskKind::Link,
        EntryKind::File | EntryKind::Unknown => DiskKind::Other,
    }
}

fn normalized(name: &str) -> String {
    if cfg!(windows) {
        name.to_lowercase()
    } else {
        name.to_owned()
    }
}

/// The folder's children on disk as a sorted multiset of (name, kind), or `None` when it
/// cannot be read completely.
fn disk_listing(folder: &Path) -> Option<Vec<(String, DiskKind)>> {
    let mut listed = Vec::new();
    for item in fs::read_dir(folder).ok()? {
        let item = item.ok()?;
        let file_type = item.file_type().ok()?;
        let kind = if file_type.is_symlink() {
            DiskKind::Link
        } else if file_type.is_dir() {
            DiskKind::Dir
        } else {
            DiskKind::Other
        };
        listed.push((normalized(&item.file_name().to_string_lossy()), kind));
    }
    listed.sort();
    Some(listed)
}

/// Why a folder may not go whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stays {
    /// Something under it is hidden by the filters, unknown to the compare, or unreadable.
    Hidden,
    /// A known entry under it is missing on disk or of another kind now.
    Changed,
}

/// Whether the sorted `expected` multiset is contained in the sorted `disk` one.
fn contains_all(disk: &[(String, DiskKind)], expected: &[(String, DiskKind)]) -> bool {
    let mut at = disk.iter();
    expected.iter().all(|want| at.any(|have| have == want))
}

/// Whether `folder` may go to the Recycle Bin whole: everything below it on `side` is wanted,
/// the disk holds exactly the nodes the comparison knows there, name and kind, and every file
/// below it is still what the compare saw (`still_is`). `Some` says why not; a hidden or
/// unknown entry outranks a changed one.
fn takeable_whole(
    cmp: &FolderComparison,
    kids: &[Vec<u32>],
    folder: u32,
    shown: &dyn Fn(u32) -> bool,
    side: Side,
    root: &Path,
    cancel: &CancelToken,
) -> Result<Option<Stays>, Cancelled> {
    let mut changed = false;
    let mut stack = vec![folder];
    while let Some(n) = stack.pop() {
        cancel.check()?;
        let Some(entry) = entry_on(cmp, n, side) else {
            continue;
        };
        if n != folder && !shown(n) {
            return Ok(Some(Stays::Hidden));
        }
        if entry.kind != EntryKind::Dir {
            // Edited in place: its folder's listing and time stay as they were (I-2).
            if !still_is(&join_rel(root, &entry.rel), entry) {
                changed = true;
            }
            continue;
        }
        let known: Vec<(u32, &Entry)> = kids[n as usize]
            .iter()
            .filter_map(|&c| entry_on(cmp, c, side).map(|e| (c, e)))
            .collect();
        let mut expected: Vec<(String, DiskKind)> = known
            .iter()
            .map(|(_, e)| (normalized(e.name()), disk_kind(e.kind)))
            .collect();
        expected.sort();
        match disk_listing(&join_rel(root, &entry.rel)) {
            None => return Ok(Some(Stays::Hidden)),
            Some(disk) if disk == expected => {}
            Some(disk) if contains_all(&disk, &expected) => return Ok(Some(Stays::Hidden)),
            Some(_) => changed = true,
        }
        stack.extend(known.iter().map(|(c, _)| *c));
    }
    Ok(changed.then_some(Stays::Changed))
}

/// Whether `path` is still what the comparison recorded: a regular file with the size and
/// modified time the scan saw (size only when none was recorded), a real folder (not a
/// link), or a link. A path that cannot be read is not.
fn still_is(path: &Path, entry: &Entry) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    let file_type = meta.file_type();
    match entry.kind {
        EntryKind::File => {
            file_type.is_file()
                && meta.len() == entry.len
                && entry
                    .modified
                    .is_none_or(|seen| meta.modified().is_ok_and(|now| now == seen))
        }
        EntryKind::Dir => file_type.is_dir(),
        EntryKind::Symlink => file_type.is_symlink(),
        // Unknown means "Other", as in `disk_kind`: a folder here may hold files the scan never saw.
        EntryKind::Unknown => !file_type.is_dir() && !file_type.is_symlink(),
    }
}

/// What to move to the Recycle Bin on `side` (rule R4). Reads the disk at call time.
pub fn trash_plan(
    cmp: &FolderComparison,
    picked: &[u32],
    shown: &dyn Fn(u32) -> bool,
    side: Side,
    root: &Path,
) -> TrashPlan {
    // A fresh token is never cancelled.
    trash_plan_with_cancel(cmp, picked, shown, side, root, &CancelToken::new()).unwrap_or_default()
}

/// [`trash_plan`], which stops at the next item once `cancel` is set: a sync plans its deletes
/// again inside its batch, where Esc must not wait for the whole disk walk.
pub fn trash_plan_with_cancel(
    cmp: &FolderComparison,
    picked: &[u32],
    shown: &dyn Fn(u32) -> bool,
    side: Side,
    root: &Path,
    cancel: &CancelToken,
) -> Result<TrashPlan, Cancelled> {
    let kids = children(cmp);
    let mut plan = TrashPlan::default();
    for pick in outermost_picks(cmp, picked) {
        // (node, top, inside a folder that is kept)
        let mut stack: Vec<(u32, bool, bool)> = vec![(pick, true, false)];
        while let Some((n, top, inside)) = stack.pop() {
            cancel.check()?;
            let Some(entry) = entry_on(cmp, n, side) else {
                continue;
            };
            if !(top || shown(n)) {
                continue;
            }
            let path = join_rel(root, &entry.rel);
            if !still_is(&path, entry) {
                // Changed since the scan: leave it and everything under it alone.
                plan.changed += 1;
                plan.changed_nodes.push(n);
                continue;
            }
            let stays = if entry.kind == EntryKind::Dir {
                takeable_whole(cmp, &kids, n, shown, side, root, cancel)?
            } else {
                None
            };
            match stays {
                None => {
                    plan.paths.push(path);
                    plan.nodes.push(n);
                }
                Some(why) => {
                    // A folder kept for a vanished entry is not "kept": the entry counts itself.
                    if why == Stays::Hidden && !inside {
                        plan.kept_folders += 1;
                        plan.kept_paths.push(path);
                    }
                    let inside = inside || why == Stays::Hidden;
                    stack.extend(kids[n as usize].iter().rev().map(|&c| (c, false, inside)));
                }
            }
        }
    }
    Ok(plan)
}

/// `(from_path, other_path)` for every covered file that is a regular file on both sides.
/// Links are left out: a touch would land on their target.
pub fn touch_pairs(
    cmp: &FolderComparison,
    ex: &Expanded,
    from: Side,
    left_root: &Path,
    right_root: &Path,
) -> Vec<(PathBuf, PathBuf)> {
    let to = other(from);
    let (src_root, dst_root) = (
        root_of(from, left_root, right_root),
        root_of(to, left_root, right_root),
    );
    ex.files
        .iter()
        .filter_map(|&n| {
            let src = entry_on(cmp, n, from).filter(|e| e.kind == EntryKind::File)?;
            let dst = entry_on(cmp, n, to).filter(|e| e.kind == EntryKind::File)?;
            Some((join_rel(src_root, &src.rel), join_rel(dst_root, &dst.rel)))
        })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchReport {
    /// Items that succeeded.
    pub done: usize,
    /// Of `done`, the created folders (a copy only); the rest are files.
    pub folders: usize,
    pub failed: Vec<(PathBuf, String)>,
    pub cancelled: bool,
}

/// Runs `step` over `items`, checking `cancel` before each. `progress` counts every item
/// attempted, `done` in the report only the ones that worked.
fn run_items<T>(
    items: impl IntoIterator<Item = T>,
    cancel: &CancelToken,
    progress: &AtomicUsize,
    mut step: impl FnMut(T) -> Result<(), (PathBuf, ActionError)>,
) -> BatchReport {
    let mut report = BatchReport::default();
    for item in items {
        if cancel.is_cancelled() {
            report.cancelled = true;
            break;
        }
        match step(item) {
            Ok(()) => report.done += 1,
            Err((path, err)) => report.failed.push((path, err.to_string())),
        }
        progress.fetch_add(1, Ordering::Relaxed);
    }
    report
}

/// Refuses a destination that is not under `dst_root` (the link check below would pass it
/// unexamined) or that has a link or junction on the way.
fn guard_destination(dst_root: &Path, dst: &Path) -> Result<(), ActionError> {
    if !dst.starts_with(dst_root) {
        return Err(ActionError::Io {
            path: dst.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                crate::i18n::tr_key("outside the compared folder"),
            ),
        });
    }
    actions::ensure_no_links_below(dst_root, dst)
}

/// Checks every destination of `batch` before anything is written: inside `dst_root`, no link
/// or junction on the way, and no copy into its own source. The first problem refuses the lot.
pub fn preflight_batch(dst_root: &Path, batch: &CopyBatch) -> Result<(), ActionError> {
    for item in &batch.files {
        guard_destination(dst_root, &item.dst)?;
        actions::refuse_nested(&item.src, &item.dst)?;
    }
    for dir in &batch.mkdirs {
        guard_destination(dst_root, dir)?;
    }
    Ok(())
}

pub fn run_copy(
    dst_root: &Path,
    batch: &CopyBatch,
    cancel: &CancelToken,
    done: &AtomicUsize,
) -> BatchReport {
    run_copy_checked(dst_root, batch, cancel, done, &|_| Ok(()))
}

/// [`run_copy`], asking `check` about each file just before it is written: an `Err` leaves
/// the file alone and is its failure.
pub(crate) fn run_copy_checked(
    dst_root: &Path,
    batch: &CopyBatch,
    cancel: &CancelToken,
    done: &AtomicUsize,
    check: &dyn Fn(&CopyItem) -> Result<(), ActionError>,
) -> BatchReport {
    enum Job<'a> {
        File(&'a CopyItem),
        Dir(&'a PathBuf),
    }
    let jobs = batch
        .files
        .iter()
        .map(Job::File)
        .chain(batch.mkdirs.iter().map(Job::Dir));
    let mut folders = 0;
    let mut report = run_items(jobs, cancel, done, |job| match job {
        Job::File(item) => guard_destination(dst_root, &item.dst)
            .and_then(|()| check(item))
            .and_then(|()| actions::copy_into(dst_root, &item.src, &item.dst, item.overwrites))
            .map(|_| ())
            .map_err(|e| (item.dst.clone(), e)),
        Job::Dir(path) => guard_destination(dst_root, path)
            .and_then(|()| {
                fs::create_dir_all(path).map_err(|source| ActionError::Io {
                    path: path.clone(),
                    source,
                })
            })
            .map(|()| folders += 1)
            .map_err(|e| (path.clone(), e)),
    });
    report.folders = folders;
    report
}

/// `trash` is `&actions::move_to_trash` in production; tests pass a fake.
pub fn run_trash(
    paths: &[PathBuf],
    trash: &(dyn Fn(&Path) -> Result<(), ActionError> + Sync),
    cancel: &CancelToken,
    done: &AtomicUsize,
) -> BatchReport {
    run_items(paths, cancel, done, |path| {
        trash(path).map_err(|e| (path.clone(), e))
    })
}

/// P4's Delete: [`run_trash`] over paths on their sides (K2). Just before each move, no folder
/// between the side's root and the path may be a link or junction: one made after the compare
/// would lead the move outside the compared tree, so the path fails and is never followed.
/// The path itself was checked by its kind when the plan was made.
pub fn run_trash_below(
    left_root: &Path,
    right_root: &Path,
    paths: &[(Side, PathBuf)],
    trash: &(dyn Fn(&Path) -> Result<(), ActionError> + Sync),
    cancel: &CancelToken,
    done: &AtomicUsize,
) -> BatchReport {
    run_items(paths, cancel, done, |(side, path)| {
        let root = root_of(*side, left_root, right_root);
        path.parent()
            .map_or(Ok(()), |parent| {
                actions::ensure_no_links_below(root, parent)
            })
            .and_then(|()| trash(path))
            .map_err(|e| (path.clone(), e))
    })
}

/// `dst_root` is the root of the side the pairs' second paths live on.
pub fn run_touch(
    dst_root: &Path,
    pairs: &[(PathBuf, PathBuf)],
    cancel: &CancelToken,
    done: &AtomicUsize,
) -> BatchReport {
    run_items(pairs, cancel, done, |(src, dst)| {
        guard_destination(dst_root, dst)
            .and_then(|()| actions::touch_like(src, dst))
            .map_err(|e| (dst.clone(), e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancelToken;
    use crate::folder::actions::ActionError;
    use crate::folder::entry::{Entry, EntryKind};
    use crate::folder::tree::build_tree;
    use std::fs;
    use std::sync::Mutex;

    fn file(rel: &str, len: u64) -> Entry {
        Entry {
            rel: rel.into(),
            kind: EntryKind::File,
            len,
            modified: None,
            error: None,
            link_target: None,
        }
    }
    fn dir(rel: &str) -> Entry {
        Entry {
            rel: rel.into(),
            kind: EntryKind::Dir,
            len: 0,
            modified: None,
            error: None,
            link_target: None,
        }
    }
    fn index(cmp: &FolderComparison, rel: &str) -> u32 {
        cmp.nodes.iter().position(|n| n.rel == rel).unwrap() as u32
    }

    #[test]
    fn expand_drops_children_of_picked_folders_and_skips_hidden_files() {
        let left = vec![
            dir("d"),
            file("d/a", 1),
            file("d/b", 2),
            dir("d/e"),
            file("x", 3),
        ];
        let right = vec![dir("d"), file("d/a", 9)];
        let cmp = build_tree(left, right, true);
        let picked = [index(&cmp, "d"), index(&cmp, "x")];
        let hidden = index(&cmp, "d/b");
        let ex = expand(&cmp, &picked, &|n| n != hidden);
        let rels: Vec<&str> = ex
            .files
            .iter()
            .map(|&n| cmp.nodes[n as usize].rel.as_str())
            .collect();
        assert_eq!(
            rels,
            ["d/a", "x"],
            "d/b is hidden by the filter; x is picked itself"
        );
        let empty: Vec<&str> = ex
            .empty_dirs
            .iter()
            .map(|&n| cmp.nodes[n as usize].rel.as_str())
            .collect();
        assert_eq!(empty, ["d/e"]);
    }

    #[test]
    fn a_picked_row_counts_even_when_the_filter_hides_it() {
        let cmp = build_tree(vec![file("same", 1)], vec![file("same", 1)], true);
        let n = index(&cmp, "same");
        assert_eq!(expand(&cmp, &[n], &|_| false).files, [n]);
    }

    #[test]
    fn a_pick_inside_a_picked_folder_is_not_done_twice() {
        let cmp = build_tree(vec![dir("d"), file("d/a", 1)], vec![], true);
        let (d, a) = (index(&cmp, "d"), index(&cmp, "d/a"));
        assert_eq!(expand(&cmp, &[d, a, a], &|_| true).files, [a]);
    }

    #[test]
    fn copy_batch_counts_bytes_overwrites_and_keeps_the_destination_spelling() {
        let left = vec![file("A.txt", 5), file("new.txt", 7)];
        let right = vec![file("a.txt", 1)];
        let cmp = build_tree(left, right, true);
        let ex = expand(
            &cmp,
            &[index(&cmp, "A.txt"), index(&cmp, "new.txt")],
            &|_| true,
        );
        let batch = copy_batch(&cmp, &ex, Side::Left, Path::new("L"), Path::new("R"));
        assert_eq!(batch.bytes, 12);
        assert_eq!(batch.overwrites(), 1);
        let dsts: Vec<_> = batch.files.iter().map(|f| f.dst.clone()).collect();
        assert_eq!(
            dsts,
            [Path::new("R").join("a.txt"), Path::new("R").join("new.txt")]
        );
    }

    #[test]
    fn copy_batch_makes_empty_folders_the_other_side_lacks() {
        let cmp = build_tree(vec![dir("e")], vec![], true);
        let ex = expand(&cmp, &[index(&cmp, "e")], &|_| true);
        let batch = copy_batch(&cmp, &ex, Side::Left, Path::new("L"), Path::new("R"));
        assert_eq!(batch.mkdirs, [Path::new("R").join("e")]);
        assert!(!batch.is_empty());
    }

    #[test]
    fn trash_keeps_a_folder_that_holds_files_the_compare_never_saw() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("d")).unwrap();
        fs::write(root.path().join("d/seen.txt"), "x").unwrap();
        fs::write(root.path().join("d/masked.log"), "x").unwrap(); // excluded from the scan
        let cmp = build_tree(vec![dir("d"), file("d/seen.txt", 1)], vec![], true);
        let plan = trash_plan(
            &cmp,
            &[index(&cmp, "d")],
            &|_| true,
            Side::Left,
            root.path(),
        );
        assert_eq!(plan.paths, [root.path().join("d").join("seen.txt")]);
        assert_eq!(plan.kept_folders, 1);
        assert_eq!(plan.changed, 0);
    }

    #[test]
    fn trash_takes_a_folder_whole_when_everything_in_it_is_wanted() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("d/e")).unwrap();
        fs::write(root.path().join("d/a"), "x").unwrap();
        fs::write(root.path().join("d/e/b"), "x").unwrap();
        let cmp = build_tree(
            vec![dir("d"), file("d/a", 1), dir("d/e"), file("d/e/b", 1)],
            vec![],
            true,
        );
        let plan = trash_plan(
            &cmp,
            &[index(&cmp, "d")],
            &|_| true,
            Side::Left,
            root.path(),
        );
        assert_eq!(plan.paths, [root.path().join("d")]);
        assert_eq!(plan.kept_folders, 0);
        assert_eq!(plan.changed, 0);
    }

    #[test]
    fn trash_counts_a_file_edited_after_the_scan_as_changed() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.txt");
        fs::write(&path, "before").unwrap();
        let meta = fs::symlink_metadata(&path).unwrap();
        let mut entry = file("a.txt", meta.len());
        entry.modified = meta.modified().ok();
        let cmp = build_tree(vec![entry], vec![], true);
        let pick = [index(&cmp, "a.txt")];
        let same = trash_plan(&cmp, &pick, &|_| true, Side::Left, root.path());
        assert_eq!((same.paths, same.changed), (vec![path.clone()], 0));
        fs::write(&path, "before and after").unwrap();
        let plan = trash_plan(&cmp, &pick, &|_| true, Side::Left, root.path());
        assert!(plan.paths.is_empty());
        assert_eq!(plan.changed, 1);
    }

    /// A file of `d` as the scan records it: its size and modified time read off the disk.
    fn scanned(root: &Path, rel: &str) -> Entry {
        let meta = fs::symlink_metadata(root.join(rel)).unwrap();
        Entry {
            modified: meta.modified().ok(),
            ..file(rel, meta.len())
        }
    }

    /// I-2: a file edited in place keeps its folder's listing and time; the folder still may
    /// not go whole, and the edited file stays.
    #[test]
    fn trash_checks_the_files_of_a_folder_it_would_take_whole() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("d/e")).unwrap();
        for rel in ["d/a", "d/b", "d/e/c"] {
            fs::write(root.path().join(rel), "x").unwrap();
        }
        let entries = [dir("d"), dir("d/e")]
            .into_iter()
            .chain(["d/a", "d/b", "d/e/c"].map(|rel| scanned(root.path(), rel)))
            .collect();
        let cmp = build_tree(entries, vec![], true);
        let pick = [index(&cmp, "d")];
        let whole = trash_plan(&cmp, &pick, &|_| true, Side::Left, root.path());
        assert_eq!(whole.paths, [root.path().join("d")]);
        fs::write(root.path().join("d/e/c"), "edited").unwrap();
        let plan = trash_plan(&cmp, &pick, &|_| true, Side::Left, root.path());
        let d = root.path().join("d");
        assert_eq!(plan.paths, [d.join("a"), d.join("b")]);
        assert_eq!((plan.changed, plan.kept_folders), (1, 0));
    }

    /// M-8: every item `changed` counts is named, a missing folder as well as an edited file.
    #[test]
    fn trash_names_what_changed() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("d")).unwrap();
        fs::write(root.path().join("d/a"), "x").unwrap();
        fs::write(root.path().join("f"), "x").unwrap();
        let entries = vec![
            dir("d"),
            scanned(root.path(), "d/a"),
            dir("gone"),
            scanned(root.path(), "f"),
        ];
        let cmp = build_tree(entries, vec![], true);
        fs::write(root.path().join("d/a"), "edited").unwrap();
        fs::write(root.path().join("f"), "edited").unwrap();
        let picks = [index(&cmp, "d"), index(&cmp, "gone"), index(&cmp, "f")];
        let plan = trash_plan(&cmp, &picks, &|_| true, Side::Left, root.path());
        assert!(plan.paths.is_empty(), "{:?}", plan.paths);
        assert_eq!(plan.changed, 3);
        assert_eq!(
            plan.changed_nodes,
            [index(&cmp, "d/a"), index(&cmp, "gone"), index(&cmp, "f")]
        );
    }

    /// M-12: a cancelled walk stops; a live token plans as `trash_plan` does.
    #[test]
    fn trash_plan_stops_when_cancelled() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("d")).unwrap();
        fs::write(root.path().join("d/a"), "x").unwrap();
        let cmp = build_tree(vec![dir("d"), file("d/a", 1)], vec![], true);
        let pick = [index(&cmp, "d")];
        let cancel = CancelToken::new();
        let live = trash_plan_with_cancel(&cmp, &pick, &|_| true, Side::Left, root.path(), &cancel);
        assert_eq!(
            live,
            Ok(trash_plan(&cmp, &pick, &|_| true, Side::Left, root.path()))
        );
        cancel.cancel();
        let stopped =
            trash_plan_with_cancel(&cmp, &pick, &|_| true, Side::Left, root.path(), &cancel);
        assert_eq!(stopped, Err(Cancelled));
    }

    #[test]
    fn trash_counts_a_vanished_shown_file_as_changed_not_as_a_kept_folder() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("d")).unwrap();
        let cmp = build_tree(vec![dir("d"), file("d/a", 1)], vec![], true);
        let plan = trash_plan(
            &cmp,
            &[index(&cmp, "d")],
            &|_| true,
            Side::Left,
            root.path(),
        );
        assert!(plan.paths.is_empty());
        assert_eq!((plan.kept_folders, plan.changed), (0, 1));
    }

    #[test]
    fn trash_counts_a_picked_folder_whose_contents_are_all_hidden() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("d")).unwrap();
        fs::write(root.path().join("d/a"), "x").unwrap();
        let cmp = build_tree(vec![dir("d"), file("d/a", 1)], vec![], true);
        let a = index(&cmp, "d/a");
        let plan = trash_plan(
            &cmp,
            &[index(&cmp, "d")],
            &|n| n != a,
            Side::Left,
            root.path(),
        );
        assert!(plan.paths.is_empty());
        assert_eq!((plan.kept_folders, plan.changed), (1, 0));
    }

    #[test]
    fn trash_keeps_a_folder_with_a_filtered_file_and_takes_the_shown_ones() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("d")).unwrap();
        fs::write(root.path().join("d/a"), "x").unwrap();
        fs::write(root.path().join("d/b"), "x").unwrap();
        let cmp = build_tree(vec![dir("d"), file("d/a", 1), file("d/b", 1)], vec![], true);
        let b = index(&cmp, "d/b");
        let plan = trash_plan(
            &cmp,
            &[index(&cmp, "d")],
            &|n| n != b,
            Side::Left,
            root.path(),
        );
        assert_eq!(plan.paths, [root.path().join("d").join("a")]);
        assert_eq!(plan.kept_folders, 1);
        assert_eq!(plan.changed, 0);
    }

    #[test]
    fn trash_lists_the_folders_it_keeps() {
        // The fixture of trash_keeps_a_folder_with_a_filtered_file_and_takes_the_shown_ones.
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("d")).unwrap();
        fs::write(root.path().join("d/a"), "x").unwrap();
        fs::write(root.path().join("d/b"), "x").unwrap();
        let cmp = build_tree(vec![dir("d"), file("d/a", 1), file("d/b", 1)], vec![], true);
        let b = index(&cmp, "d/b");
        let plan = trash_plan(
            &cmp,
            &[index(&cmp, "d")],
            &|n| n != b,
            Side::Left,
            root.path(),
        );
        assert_eq!(plan.kept_paths, [root.path().join("d")]);
        assert_eq!(plan.kept_paths.len(), plan.kept_folders);
        // Only the outermost kept folder is listed, as it is counted.
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("d/e")).unwrap();
        fs::write(root.path().join("d/a"), "x").unwrap();
        fs::write(root.path().join("d/e/b"), "x").unwrap();
        fs::write(root.path().join("d/e/masked"), "x").unwrap();
        let cmp = build_tree(
            vec![dir("d"), file("d/a", 1), dir("d/e"), file("d/e/b", 1)],
            vec![],
            true,
        );
        let plan = trash_plan(
            &cmp,
            &[index(&cmp, "d")],
            &|_| true,
            Side::Left,
            root.path(),
        );
        assert_eq!(plan.kept_paths, [root.path().join("d")]);
        assert_eq!(plan.kept_paths.len(), plan.kept_folders);
    }

    #[test]
    fn trash_counts_only_the_outermost_kept_folder_and_unreadable_means_kept() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("d/e")).unwrap();
        fs::write(root.path().join("d/a"), "x").unwrap();
        fs::write(root.path().join("d/e/b"), "x").unwrap();
        fs::write(root.path().join("d/e/masked"), "x").unwrap();
        let cmp = build_tree(
            vec![dir("d"), file("d/a", 1), dir("d/e"), file("d/e/b", 1)],
            vec![],
            true,
        );
        let plan = trash_plan(
            &cmp,
            &[index(&cmp, "d")],
            &|_| true,
            Side::Left,
            root.path(),
        );
        assert_eq!(
            plan.paths,
            [
                root.path().join("d").join("e").join("b"),
                root.path().join("d").join("a")
            ]
        );
        assert_eq!(plan.kept_folders, 1);
        assert_eq!(plan.changed, 0);
        // a folder missing on disk cannot be trashed at all
        let gone = tempfile::tempdir().unwrap();
        let plan = trash_plan(
            &cmp,
            &[index(&cmp, "d")],
            &|_| true,
            Side::Left,
            gone.path(),
        );
        assert!(plan.paths.is_empty(), "nothing on disk to trash");
        assert_eq!(plan.kept_folders, 0);
        assert_eq!(plan.changed, 1);
    }

    #[test]
    fn copy_and_touch_runs_do_every_item_and_stop_on_cancel() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        fs::write(l.path().join("a"), "aa").unwrap();
        fs::write(l.path().join("b"), "bb").unwrap();
        let cmp = build_tree(vec![file("a", 2), file("b", 2)], vec![], true);
        let ex = expand(&cmp, &[index(&cmp, "a"), index(&cmp, "b")], &|_| true);
        let batch = copy_batch(&cmp, &ex, Side::Left, l.path(), r.path());
        let done = AtomicUsize::new(0);
        let report = run_copy(r.path(), &batch, &CancelToken::new(), &done);
        assert_eq!(
            (report.done, report.failed.len(), report.cancelled),
            (2, 0, false)
        );
        assert_eq!(done.load(Ordering::Relaxed), 2);
        assert_eq!(fs::read_to_string(r.path().join("b")).unwrap(), "bb");
        let cancel = CancelToken::new();
        cancel.cancel();
        let report = run_copy(r.path(), &batch, &cancel, &AtomicUsize::new(0));
        assert!(report.cancelled && report.done == 0);
        // touch: right gets left's times
        let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        fs::File::options()
            .write(true)
            .open(l.path().join("a"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        let cmp = build_tree(vec![file("a", 2)], vec![file("a", 2)], true);
        let ex = expand(&cmp, &[index(&cmp, "a")], &|_| true);
        let pairs = touch_pairs(&cmp, &ex, Side::Left, l.path(), r.path());
        let report = run_touch(r.path(), &pairs, &CancelToken::new(), &AtomicUsize::new(0));
        assert_eq!(report.done, 1);
        assert_eq!(
            fs::metadata(r.path().join("a"))
                .unwrap()
                .modified()
                .unwrap(),
            old
        );
    }

    #[test]
    fn run_copy_creates_empty_folders() {
        let r = tempfile::tempdir().unwrap();
        let batch = CopyBatch {
            files: vec![],
            mkdirs: vec![r.path().join("x/y")],
            bytes: 0,
        };
        let report = run_copy(r.path(), &batch, &CancelToken::new(), &AtomicUsize::new(0));
        assert_eq!(report.done, 1);
        assert!(r.path().join("x/y").is_dir());
    }

    #[test]
    fn run_trash_passes_every_path_in_order_records_failures_and_stops_on_cancel() {
        let paths: Vec<PathBuf> = ["p1", "p2", "p3"].iter().map(PathBuf::from).collect();
        let seen = Mutex::new(Vec::new());
        let fake = |p: &Path| -> Result<(), ActionError> {
            seen.lock().unwrap().push(p.to_path_buf());
            if p == Path::new("p2") {
                Err(ActionError::Trash {
                    path: p.to_path_buf(),
                    message: "locked".into(),
                })
            } else {
                Ok(())
            }
        };
        let done = AtomicUsize::new(0);
        let report = run_trash(&paths, &fake, &CancelToken::new(), &done);
        assert_eq!(*seen.lock().unwrap(), paths);
        assert_eq!((report.done, report.cancelled), (2, false));
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].0, PathBuf::from("p2"));
        assert!(report.failed[0].1.contains("locked"));
        assert_eq!(done.load(Ordering::Relaxed), 3);

        seen.lock().unwrap().clear();
        let cancel = CancelToken::new();
        let stop_after_first = |p: &Path| -> Result<(), ActionError> {
            seen.lock().unwrap().push(p.to_path_buf());
            cancel.cancel();
            Ok(())
        };
        let report = run_trash(&paths, &stop_after_first, &cancel, &AtomicUsize::new(0));
        assert_eq!(*seen.lock().unwrap(), [PathBuf::from("p1")]);
        assert!(report.cancelled);
        assert_eq!(report.done, 1);
    }

    fn kinded(rel: &str, kind: EntryKind) -> Entry {
        Entry {
            kind,
            ..file(rel, 3)
        }
    }

    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) {
        let out = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[cfg(windows)]
    #[test]
    fn trash_keeps_a_folder_holding_two_names_that_fold_together() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("d")).unwrap();
        fs::write(root.path().join("d/k.txt"), "x").unwrap();
        fs::write(root.path().join("d/\u{212A}.txt"), "x").unwrap();
        let cmp = build_tree(vec![dir("d"), file("d/k.txt", 1)], vec![], true);
        let plan = trash_plan(
            &cmp,
            &[index(&cmp, "d")],
            &|_| true,
            Side::Left,
            root.path(),
        );
        assert_eq!(plan.paths, [root.path().join("d").join("k.txt")]);
        assert_eq!(plan.kept_folders, 1);
        assert_eq!(plan.changed, 0);
    }

    #[test]
    fn trash_keeps_a_folder_whose_known_file_became_a_folder() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("d/f")).unwrap();
        fs::write(root.path().join("d/f/secret"), "x").unwrap();
        let cmp = build_tree(vec![dir("d"), file("d/f", 1)], vec![], true);
        let plan = trash_plan(
            &cmp,
            &[index(&cmp, "d")],
            &|_| true,
            Side::Left,
            root.path(),
        );
        assert_eq!(plan.kept_folders, 0);
        assert_eq!(plan.changed, 1);
        assert!(!plan.paths.contains(&root.path().join("d")));
        assert!(!plan.paths.contains(&root.path().join("d").join("f")));
    }

    #[test]
    fn nested_empty_folders_are_all_created() {
        let cmp = build_tree(vec![dir("e"), dir("e/f")], vec![], true);
        let ex = expand(&cmp, &[index(&cmp, "e")], &|_| true);
        let b = copy_batch(&cmp, &ex, Side::Left, Path::new("L"), Path::new("R"));
        assert!(b.mkdirs.contains(&Path::new("R").join("e").join("f")));
        let cmp = build_tree(vec![dir("e"), dir("e/f")], vec![dir("e")], true);
        let ex = expand(&cmp, &[index(&cmp, "e")], &|_| true);
        let b = copy_batch(&cmp, &ex, Side::Left, Path::new("L"), Path::new("R"));
        assert_eq!(b.mkdirs, [Path::new("R").join("e").join("f")]);
    }

    #[test]
    fn a_folder_on_one_side_and_a_file_on_the_other_is_decided_per_side() {
        let left = vec![dir("m"), file("m/x", 1)];
        let right = vec![file("m", 4)];
        let cmp = build_tree(left, right, true);
        let m = index(&cmp, "m");
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("m"), "four").unwrap();
        let plan = trash_plan(&cmp, &[m], &|_| true, Side::Right, root.path());
        assert_eq!(plan.paths, [root.path().join("m")]);
    }

    #[test]
    fn copy_skips_items_whose_destination_is_of_another_kind() {
        let cmp = build_tree(vec![dir("m"), file("m/x", 1)], vec![file("m", 4)], true);
        let ex = expand(&cmp, &[index(&cmp, "m")], &|_| true);
        for from in [Side::Left, Side::Right] {
            let b = copy_batch(&cmp, &ex, from, Path::new("L"), Path::new("R"));
            assert!(b.is_empty(), "{from:?}: {b:?}");
            assert_eq!(b.overwrites(), 0);
        }
    }

    #[test]
    fn trash_skips_a_picked_file_that_became_a_folder() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("f")).unwrap();
        fs::write(root.path().join("f/secret"), "x").unwrap();
        let cmp = build_tree(vec![file("f", 1)], vec![], true);
        let plan = trash_plan(
            &cmp,
            &[index(&cmp, "f")],
            &|_| true,
            Side::Left,
            root.path(),
        );
        assert!(plan.paths.is_empty());
        assert_eq!(plan.kept_folders, 0);
        assert_eq!(plan.changed, 1);
    }

    #[test]
    fn links_are_neither_copied_nor_touched() {
        let left = vec![kinded("ln", EntryKind::Symlink), file("f", 2)];
        let right = vec![kinded("ln", EntryKind::Symlink), file("f", 2)];
        let cmp = build_tree(left, right, true);
        let ex = expand(&cmp, &[index(&cmp, "ln"), index(&cmp, "f")], &|_| true);
        assert_eq!(ex.files.len(), 2);
        let b = copy_batch(&cmp, &ex, Side::Left, Path::new("L"), Path::new("R"));
        assert_eq!((b.files.len(), b.bytes), (1, 2));
        let pairs = touch_pairs(&cmp, &ex, Side::Left, Path::new("L"), Path::new("R"));
        assert_eq!(pairs.len(), 1);
    }

    #[test]
    fn runs_refuse_a_destination_outside_the_root() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        fs::write(root.path().join("s"), "x").unwrap();
        let batch = CopyBatch {
            files: vec![CopyItem {
                src: root.path().join("s"),
                dst: elsewhere.path().join("s"),
                len: 1,
                overwrites: false,
            }],
            ..CopyBatch::default()
        };
        let r = run_copy(
            root.path(),
            &batch,
            &CancelToken::new(),
            &AtomicUsize::new(0),
        );
        assert_eq!((r.done, r.failed.len()), (0, 1));
        assert!(!elsewhere.path().join("s").exists());
        let pairs = [(root.path().join("s"), elsewhere.path().join("s"))];
        let r = run_touch(
            root.path(),
            &pairs,
            &CancelToken::new(),
            &AtomicUsize::new(0),
        );
        assert_eq!((r.done, r.failed.len()), (0, 1));
    }

    #[test]
    fn run_copy_does_not_overwrite_a_file_the_plan_did_not_list() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("s"), "new").unwrap();
        fs::write(root.path().join("t"), "mine").unwrap();
        let batch = CopyBatch {
            files: vec![CopyItem {
                src: root.path().join("s"),
                dst: root.path().join("t"),
                len: 3,
                overwrites: false,
            }],
            ..CopyBatch::default()
        };
        let r = run_copy(
            root.path(),
            &batch,
            &CancelToken::new(),
            &AtomicUsize::new(0),
        );
        assert_eq!((r.done, r.failed.len()), (0, 1));
        assert_eq!(fs::read_to_string(root.path().join("t")).unwrap(), "mine");
    }

    #[cfg(windows)]
    #[test]
    fn preflight_batch_refuses_a_destination_junction_before_anything_is_written() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        junction(&root.path().join("j"), outside.path());
        fs::write(root.path().join("s"), "x").unwrap();
        let item = |dst: PathBuf| CopyItem {
            src: root.path().join("s"),
            dst,
            len: 1,
            overwrites: false,
        };
        let ok = CopyBatch {
            files: vec![item(root.path().join("fresh"))],
            ..CopyBatch::default()
        };
        assert!(preflight_batch(root.path(), &ok).is_ok());
        let bad = CopyBatch {
            files: vec![
                item(root.path().join("fresh")),
                item(root.path().join("j").join("t")),
            ],
            ..CopyBatch::default()
        };
        assert!(matches!(
            preflight_batch(root.path(), &bad),
            Err(ActionError::DestinationIsLink(_))
        ));
        let dirs = CopyBatch {
            mkdirs: vec![root.path().join("j").join("sub")],
            ..CopyBatch::default()
        };
        assert!(preflight_batch(root.path(), &dirs).is_err());
    }

    #[test]
    fn preflight_batch_refuses_a_destination_outside_its_root_or_inside_its_source() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("dir")).unwrap();
        let one = |src: PathBuf, dst: PathBuf| CopyBatch {
            files: vec![CopyItem {
                src,
                dst,
                len: 0,
                overwrites: false,
            }],
            ..CopyBatch::default()
        };
        let outside = one(root.path().join("s"), other.path().join("t"));
        assert!(preflight_batch(root.path(), &outside).is_err());
        let nested = one(root.path().join("dir"), root.path().join("dir").join("x"));
        assert!(matches!(
            preflight_batch(root.path(), &nested),
            Err(ActionError::IntoItself { .. })
        ));
    }

    #[cfg(windows)]
    #[test]
    fn run_copy_and_touch_refuse_a_destination_junction() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        junction(&root.path().join("j"), outside.path());
        fs::write(root.path().join("s"), "x").unwrap();
        fs::write(outside.path().join("t"), "old").unwrap();
        let dst = root.path().join("j").join("t");
        let batch = CopyBatch {
            files: vec![CopyItem {
                src: root.path().join("s"),
                dst: dst.clone(),
                len: 1,
                overwrites: true,
            }],
            ..CopyBatch::default()
        };
        let r = run_copy(
            root.path(),
            &batch,
            &CancelToken::new(),
            &AtomicUsize::new(0),
        );
        assert_eq!((r.done, r.failed.len()), (0, 1));
        assert_eq!(fs::read_to_string(outside.path().join("t")).unwrap(), "old");
        let pairs = [(root.path().join("s"), dst)];
        let r = run_touch(
            root.path(),
            &pairs,
            &CancelToken::new(),
            &AtomicUsize::new(0),
        );
        assert_eq!((r.done, r.failed.len()), (0, 1));
        // touch_like itself refuses a junction as the destination
        let err = actions::touch_like(&root.path().join("s"), &root.path().join("j"));
        assert!(matches!(err, Err(ActionError::DestinationIsLink(_))));
    }

    #[test]
    fn trash_skips_an_unknown_row_that_is_a_folder_on_disk() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("d/u")).unwrap();
        fs::write(root.path().join("d/u/unseen"), "x").unwrap();
        let cmp = build_tree(
            vec![dir("d"), kinded("d/u", EntryKind::Unknown)],
            vec![],
            true,
        );
        let plan = trash_plan(
            &cmp,
            &[index(&cmp, "d")],
            &|_| true,
            Side::Left,
            root.path(),
        );
        assert!(plan.paths.is_empty(), "{:?}", plan.paths);
        assert_eq!(plan.kept_folders, 0);
        assert_eq!(plan.changed, 1);
        let plan = trash_plan(
            &cmp,
            &[index(&cmp, "d/u")],
            &|_| true,
            Side::Left,
            root.path(),
        );
        assert!(plan.paths.is_empty());
        assert_eq!(plan.kept_folders, 0);
        assert_eq!(plan.changed, 1);
    }

    #[test]
    fn copy_plans_no_mkdir_under_a_destination_file() {
        let cmp = build_tree(vec![dir("m"), dir("m/e")], vec![file("m", 4)], true);
        let ex = expand(&cmp, &[index(&cmp, "m")], &|_| true);
        assert!(ex.empty_dirs.contains(&index(&cmp, "m/e")));
        let b = copy_batch(&cmp, &ex, Side::Left, Path::new("L"), Path::new("R"));
        assert!(b.mkdirs.is_empty(), "{:?}", b.mkdirs);
    }

    #[cfg(windows)]
    #[test]
    fn trash_skips_a_picked_folder_replaced_by_a_junction() {
        let root = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        junction(&root.path().join("d"), target.path());
        let cmp = build_tree(vec![dir("d")], vec![], true);
        let plan = trash_plan(
            &cmp,
            &[index(&cmp, "d")],
            &|_| true,
            Side::Left,
            root.path(),
        );
        assert!(plan.paths.is_empty());
        assert_eq!(plan.kept_folders, 0);
        assert_eq!(plan.changed, 1);
    }
}
