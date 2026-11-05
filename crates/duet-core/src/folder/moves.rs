//! Moved or renamed files: a left-only file and a right-only file with the same bytes.
//!
//! Size first (only over one-sided files), then a CRC-32 of the whole file for the sizes that
//! have both sides and more than one file, then a byte compare of the one candidate pair, so a
//! checksum collision can never tag two different files. One to one only.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use rayon::prelude::*;

use crate::cancel::{CancelToken, Cancelled};
use crate::folder::content::{CHUNK_LEN, ContentError, READ_THREADS, files_equal_with, read_full};
use crate::folder::entry::{EntryKind, ScanError, join_rel};
use crate::folder::sync::unreadable;
use crate::folder::tree::{FolderComparison, Status};

#[derive(Debug, Default)]
pub struct MoveProgress {
    pub total: AtomicU64,
    pub done: AtomicU64,
}

/// What the pass will read, decided without touching the disk.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reads {
    /// One left and one right of one size: straight to the byte compare.
    pub direct: Vec<(u32, u32)>,
    /// Sizes with both sides and more than one file on a side: every file is hashed.
    pub hashed: Vec<Vec<u32>>,
}

/// A file of a hashed group and its CRC, if it could be read.
type Sum = (usize, u32, Option<u32>);
/// Per CRC: the left and the right files that have it.
type BySum = BTreeMap<u32, (Vec<u32>, Vec<u32>)>;

/// Per node: a folder above it could not be read, so its other side only looks one-sided and
/// the file may still be there (the rule a sync applies). Parents precede their children.
fn below_unreadable(cmp: &FolderComparison) -> Vec<bool> {
    let mut out = vec![false; cmp.nodes.len()];
    for i in 0..cmp.nodes.len() {
        if let Some(p) = cmp.nodes[i].parent {
            out[i] = out[p as usize] || unreadable(&cmp.nodes[p as usize]);
        }
    }
    out
}

fn candidate(cmp: &FolderComparison, index: usize, blocked: &[bool]) -> Option<(bool, u64)> {
    let node = &cmp.nodes[index];
    if node.is_dir || blocked[index] {
        return None;
    }
    let (entry, is_left) = match node.status {
        Status::LeftOnly => (node.left.as_ref()?, true),
        Status::RightOnly => (node.right.as_ref()?, false),
        _ => return None,
    };
    (entry.kind == EntryKind::File && entry.error.is_none() && entry.len > 0)
        .then_some((is_left, entry.len))
}

/// Windows file attributes of a file whose content is not on this disk, so reading it would
/// download it (OneDrive and the like): OFFLINE, RECALL_ON_OPEN, RECALL_ON_DATA_ACCESS.
pub fn is_cloud_placeholder(attributes: u32) -> bool {
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_OPEN: u32 = 0x4_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
    attributes & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
}

/// Whether the file is a cloud placeholder, from its metadata, which does not recall it.
#[cfg(windows)]
fn is_placeholder_at(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(path).is_ok_and(|meta| is_cloud_placeholder(meta.file_attributes()))
}

#[cfg(not(windows))]
fn is_placeholder_at(_path: &Path) -> bool {
    false
}

pub fn plan_reads(cmp: &FolderComparison) -> Reads {
    plan_reads_with(cmp, |_| false)
}

/// [`plan_reads`], where `skip(index)` says a candidate must not be read. A size group with a
/// skipped file is dropped whole: that file might be a second copy, and tagging the others
/// would then be a guess. `skip` is asked only about files that would be read.
pub fn plan_reads_with(cmp: &FolderComparison, skip: impl Fn(u32) -> bool) -> Reads {
    let blocked = below_unreadable(cmp);
    let mut by_size: BTreeMap<u64, (Vec<u32>, Vec<u32>)> = BTreeMap::new();
    for index in 0..cmp.nodes.len() {
        if let Some((is_left, len)) = candidate(cmp, index, &blocked) {
            let group = by_size.entry(len).or_default();
            if is_left {
                group.0.push(index as u32);
            } else {
                group.1.push(index as u32);
            }
        }
    }
    let mut reads = Reads::default();
    for (lefts, rights) in by_size.into_values() {
        if lefts.is_empty() || rights.is_empty() {
            continue;
        }
        if lefts.iter().chain(&rights).any(|&index| skip(index)) {
            continue;
        }
        match (lefts.len(), rights.len()) {
            (1, 1) => reads.direct.push((lefts[0], rights[0])),
            _ => reads.hashed.push(lefts.into_iter().chain(rights).collect()),
        }
    }
    reads
}

fn same_name_ignoring_case(cmp: &FolderComparison, a: u32, b: u32) -> bool {
    cmp.nodes[a as usize].rel.to_lowercase() == cmp.nodes[b as usize].rel.to_lowercase()
}

fn path_of(
    cmp: &FolderComparison,
    index: u32,
    left_root: &Path,
    right_root: &Path,
) -> std::path::PathBuf {
    let node = &cmp.nodes[index as usize];
    match (&node.left, &node.right, &node.status) {
        (Some(entry), _, &Status::LeftOnly) => join_rel(left_root, &entry.rel),
        (_, Some(entry), _) => join_rel(right_root, &entry.rel),
        (Some(entry), _, _) => join_rel(left_root, &entry.rel),
        _ => join_rel(left_root, &node.rel),
    }
}

/// A read that fails drops the candidate; only a cancel stops the pass.
fn settle<T>(result: Result<T, ContentError>) -> Result<Option<T>, Cancelled> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(ContentError::Io(_)) => Ok(None),
        Err(ContentError::Cancelled(c)) => Err(c),
    }
}

fn crc_of(path: &Path, buffer: &mut [u8], cancel: &CancelToken) -> Result<Option<u32>, Cancelled> {
    let Ok(mut file) = File::open(path) else {
        return Ok(None);
    };
    let mut hasher = crc32fast::Hasher::new();
    loop {
        cancel.check()?;
        match read_full(&mut file, buffer) {
            Ok(0) => return Ok(Some(hasher.finalize())),
            Ok(n) => hasher.update(&buffer[..n]),
            Err(_) => return Ok(None),
        }
    }
}

enum SearchError {
    Cancelled,
    Pool(rayon::ThreadPoolBuildError),
}

impl From<Cancelled> for SearchError {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}

/// Tags moved or renamed pairs in `cmp.moved`. A cancel leaves `cmp` as it was, with no tags,
/// and returns `Ok`: the comparison made before it is finished work worth keeping.
pub fn find_moves(
    cmp: &mut FolderComparison,
    left_root: &Path,
    right_root: &Path,
    max_parallel_io: usize,
    progress: &MoveProgress,
    cancel: &CancelToken,
) -> Result<(), ScanError> {
    let pairs = match search(
        cmp,
        left_root,
        right_root,
        max_parallel_io,
        progress,
        cancel,
    ) {
        Ok(pairs) => pairs,
        Err(SearchError::Cancelled) => return Ok(()),
        Err(SearchError::Pool(err)) => return Err(err.into()),
    };
    for (l, r) in pairs {
        cmp.moved.insert(l, r);
        cmp.moved.insert(r, l);
    }
    Ok(())
}

fn search(
    cmp: &FolderComparison,
    left_root: &Path,
    right_root: &Path,
    max_parallel_io: usize,
    progress: &MoveProgress,
    cancel: &CancelToken,
) -> Result<Vec<(u32, u32)>, SearchError> {
    cancel.check()?;
    let path = |index: u32| path_of(cmp, index, left_root, right_root);
    let mut reads = plan_reads_with(cmp, |index| is_placeholder_at(&path(index)));
    reads
        .direct
        .retain(|&(l, r)| !same_name_ignoring_case(cmp, l, r));
    let hashed_files: usize = reads.hashed.iter().map(Vec::len).sum();
    progress.total.store(
        (reads.direct.len() + hashed_files) as u64,
        Ordering::Relaxed,
    );
    progress.done.store(0, Ordering::Relaxed);
    if reads.direct.is_empty() && reads.hashed.is_empty() {
        return Ok(Vec::new());
    }
    // The same limit as the content pass: this pass opens files that pass never does.
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(max_parallel_io.clamp(1, READ_THREADS))
        .build()
        .map_err(SearchError::Pool)?;
    let buffers = || (vec![0u8; CHUNK_LEN], vec![0u8; CHUNK_LEN]);

    // Each closure checks the token before it opens anything, and collecting into a `Result`
    // stops handing out work at the first cancel.
    let direct: Vec<Option<(u32, u32)>> = pool.install(|| {
        reads
            .direct
            .par_iter()
            .map_init(buffers, |bufs, &(l, r)| {
                cancel.check()?;
                let equal = settle(files_equal_with(&path(l), &path(r), bufs, cancel));
                progress.done.fetch_add(1, Ordering::Relaxed);
                Ok(equal?.filter(|&equal| equal).map(|_| (l, r)))
            })
            .collect::<Result<_, Cancelled>>()
    })?;
    cancel.check()?;

    let files: Vec<(usize, u32)> = reads
        .hashed
        .iter()
        .enumerate()
        .flat_map(|(group, members)| members.iter().map(move |&index| (group, index)))
        .collect();
    let sums: Vec<Sum> = pool.install(|| {
        files
            .par_iter()
            .map_init(
                || vec![0u8; CHUNK_LEN],
                |buffer, &(group, index)| {
                    cancel.check()?;
                    let sum = crc_of(&path(index), buffer, cancel);
                    progress.done.fetch_add(1, Ordering::Relaxed);
                    Ok((group, index, sum?))
                },
            )
            .collect::<Result<_, Cancelled>>()
    })?;
    cancel.check()?;

    // Per group, a CRC with exactly one left and one right file is a candidate pair. A file
    // that could not be read might have been a second copy: its whole group tags nothing.
    let mut by_sum: Vec<BySum> = vec![BTreeMap::new(); reads.hashed.len()];
    let mut unread = vec![false; reads.hashed.len()];
    for (group, index, crc) in sums {
        let Some(crc) = crc else {
            unread[group] = true;
            continue;
        };
        let slot = by_sum[group].entry(crc).or_default();
        if cmp.nodes[index as usize].status == Status::LeftOnly {
            slot.0.push(index);
        } else {
            slot.1.push(index);
        }
    }
    let candidates: Vec<(u32, u32)> = by_sum
        .into_iter()
        .zip(unread)
        .filter(|&(_, unread)| !unread)
        .flat_map(|(group, _)| group)
        .filter_map(
            |(_, (lefts, rights))| match (lefts.as_slice(), rights.as_slice()) {
                (&[l], &[r]) if !same_name_ignoring_case(cmp, l, r) => Some((l, r)),
                _ => None,
            },
        )
        .collect();
    progress
        .total
        .fetch_add(candidates.len() as u64, Ordering::Relaxed);
    let confirmed: Vec<Option<(u32, u32)>> = pool.install(|| {
        candidates
            .par_iter()
            .map_init(buffers, |bufs, &(l, r)| {
                cancel.check()?;
                let equal = settle(files_equal_with(&path(l), &path(r), bufs, cancel));
                progress.done.fetch_add(1, Ordering::Relaxed);
                Ok(equal?.filter(|&equal| equal).map(|_| (l, r)))
            })
            .collect::<Result<_, Cancelled>>()
    })?;
    cancel.check()?;
    Ok(direct.into_iter().chain(confirmed).flatten().collect())
}

#[cfg(test)]
mod tests;
