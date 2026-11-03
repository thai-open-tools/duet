//! The invisible-character scan that follows a folder compare (R9): every file on each side up
//! to `ODD_FILE_LIMIT` is decoded and run through P9b's `find_odd`. The cap is deliberate: the
//! scan is a hint beside the rows, not a replacement for opening the file (the text tab scans
//! up to 64 MiB), and it must stay cheap after every compare.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use rayon::prelude::*;

use crate::cancel::{CancelToken, Cancelled};
use crate::folder::entry::EntryKind;
use crate::folder::tree::FolderComparison;
use crate::text::encoding::{decode, is_binary};
use crate::text::lexer::detect_language;
use crate::text::odd::{OddScan, find_odd};

/// Files above this size are not scanned.
pub const ODD_FILE_LIMIT: u64 = 4 * 1024 * 1024;
/// Hits kept per file (the counts cover all of them).
pub const ODD_HITS: usize = 3;

/// The scans of the files that have a hit, by node and side. Sparse: a file without a hit is
/// not in it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OddMarks {
    by_node: HashMap<u32, [Option<OddScan>; 2]>,
}

impl OddMarks {
    pub fn get(&self, node: u32, left: bool) -> Option<&OddScan> {
        self.by_node.get(&node)?[usize::from(!left)].as_ref()
    }

    /// (files with a hit in the text or values, files whose hits are only in comments or
    /// indentation). A pair counts once, whichever side holds the hit.
    pub fn files(&self) -> (usize, usize) {
        let mut files = (0, 0);
        for sides in self.by_node.values() {
            let warn: usize = sides.iter().flatten().map(|s| s.warn).sum();
            let info: usize = sides.iter().flatten().map(|s| s.info).sum();
            if warn > 0 {
                files.0 += 1;
            } else if info > 0 {
                files.1 += 1;
            }
        }
        files
    }

    /// The same marks seen from the other side (F9).
    pub fn swapped(mut self) -> OddMarks {
        for sides in self.by_node.values_mut() {
            sides.swap(0, 1);
        }
        self
    }
}

#[derive(Debug, Default)]
pub struct OddProgress {
    pub total: AtomicU64,
    pub done: AtomicU64,
}

/// A file with a hit: its node, whether it is the left side, and what was found.
type Found = (u32, bool, OddScan);

/// What to read for one side of one file.
struct Job<'a> {
    node: u32,
    left: bool,
    path: std::path::PathBuf,
    name: &'a str,
}

/// Reads and scans every file of both sides on `threads` threads. A file that cannot be read,
/// is empty, over `ODD_FILE_LIMIT` or binary has no marks. Each file is opened once and closed
/// before its scan starts.
pub fn scan_odd(
    cmp: &FolderComparison,
    left_root: &Path,
    right_root: &Path,
    threads: usize,
    progress: &OddProgress,
    cancel: &CancelToken,
) -> Result<OddMarks, Cancelled> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .build()
        .ok();
    scan_odd_in(pool.as_ref(), cmp, left_root, right_root, progress, cancel)
}

/// [`scan_odd`] on `pool`, or on the calling thread without one.
fn scan_odd_in(
    pool: Option<&rayon::ThreadPool>,
    cmp: &FolderComparison,
    left_root: &Path,
    right_root: &Path,
    progress: &OddProgress,
    cancel: &CancelToken,
) -> Result<OddMarks, Cancelled> {
    let mut jobs = Vec::new();
    for (index, node) in cmp.nodes.iter().enumerate() {
        for (left, entry, root) in [
            (true, &node.left, left_root),
            (false, &node.right, right_root),
        ] {
            let Some(entry) = entry.as_ref().filter(|e| e.kind == EntryKind::File) else {
                continue;
            };
            jobs.push(Job {
                node: index as u32,
                left,
                path: root.join(&entry.rel),
                name: &node.name,
            });
        }
    }
    progress.total.store(jobs.len() as u64, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    let one = |job: &Job| {
        cancel.check()?;
        let scan = scan_file(job, cancel);
        progress.done.fetch_add(1, Ordering::Relaxed);
        Ok(scan?.map(|scan| (job.node, job.left, scan)))
    };
    let found: Vec<Result<Option<Found>, Cancelled>> = match pool {
        Some(pool) => pool.install(|| jobs.par_iter().map(one).collect()),
        // No thread could be made: the caller is a background thread, so the scan runs there
        // (never on rayon's global pool, which would fail the same way).
        None => jobs.iter().map(one).collect(),
    };
    cancel.check()?;
    let mut marks = OddMarks::default();
    for hit in found {
        if let Some((node, left, scan)) = hit? {
            marks.by_node.entry(node).or_default()[usize::from(!left)] = Some(scan);
        }
    }
    Ok(marks)
}

fn scan_file(job: &Job, cancel: &CancelToken) -> Result<Option<OddScan>, Cancelled> {
    let Some(bytes) = read_small(&job.path) else {
        return Ok(None);
    };
    if bytes.is_empty() || is_binary(&bytes) {
        return Ok(None);
    }
    let decoded = decode(&bytes);
    let scan = find_odd(&decoded.text, detect_language(job.name), ODD_HITS, cancel)?;
    Ok((scan.count > 0).then_some(scan))
}

/// The file's bytes, or None when it is over the limit or cannot be read. The handle is
/// closed on return.
fn read_small(path: &Path) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len > ODD_FILE_LIMIT {
        return None;
    }
    let mut bytes = Vec::with_capacity(len as usize);
    // The file may have grown since the metadata: never read past the limit.
    file.by_ref()
        .take(ODD_FILE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= ODD_FILE_LIMIT).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder::run::{FolderOptions, FolderProgress, compare_folders};
    use std::fs;

    fn put(root: &Path, rel: &str, body: &[u8]) {
        fs::write(root.join(rel), body).unwrap();
    }

    fn compare(l: &Path, r: &Path) -> FolderComparison {
        compare_folders(
            l,
            r,
            &FolderOptions::default(),
            &FolderProgress::default(),
            &CancelToken::new(),
        )
        .unwrap()
    }

    fn index(cmp: &FolderComparison, rel: &str) -> u32 {
        cmp.nodes.iter().position(|n| n.rel == rel).unwrap() as u32
    }

    #[test]
    fn the_scan_finds_invisible_characters_per_side() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(l.path(), "a.properties", "k =\u{a0}v\n".as_bytes());
        put(r.path(), "a.properties", b"k = v\n");
        put(l.path(), "c.js", "// \u{200b}\n".as_bytes());
        let mut big = "\u{a0}".as_bytes().to_vec();
        big.resize(ODD_FILE_LIMIT as usize + 1, b'x');
        put(r.path(), "big.txt", &big);
        put(l.path(), "bin.dat", "\0\u{a0}".as_bytes());
        let cmp = compare(l.path(), r.path());
        let progress = OddProgress::default();
        let marks = scan_odd(&cmp, l.path(), r.path(), 2, &progress, &CancelToken::new()).unwrap();

        let a = index(&cmp, "a.properties");
        let left = marks.get(a, true).unwrap();
        assert_eq!((left.count, left.warn), (1, 1));
        assert!(marks.get(a, false).is_none());
        let c = index(&cmp, "c.js");
        let comment = marks.get(c, true).unwrap();
        assert_eq!((comment.info, comment.warn), (1, 0));
        assert_eq!(comment.hits[0].place, crate::text::odd::OddPlace::Comment);
        assert!(marks.get(index(&cmp, "big.txt"), false).is_none());
        assert!(marks.get(index(&cmp, "bin.dat"), true).is_none());
        assert_eq!(marks.files(), (1, 1));
        assert_eq!(
            progress.done.load(Ordering::Relaxed),
            progress.total.load(Ordering::Relaxed)
        );
    }

    /// M-4: when no thread can be made the scan does not panic: it runs on the calling (a
    /// background) thread and finds what the pool finds.
    #[test]
    fn without_a_thread_pool_the_scan_runs_on_the_calling_thread() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(l.path(), "a.txt", "x\u{a0}\n".as_bytes());
        put(r.path(), "a.txt", b"x\n");
        put(l.path(), "c.js", "// \u{200b}\n".as_bytes());
        let cmp = compare(l.path(), r.path());
        let cancel = CancelToken::new();
        let pooled = scan_odd(
            &cmp,
            l.path(),
            r.path(),
            2,
            &OddProgress::default(),
            &cancel,
        );
        let progress = OddProgress::default();
        let alone = scan_odd_in(None, &cmp, l.path(), r.path(), &progress, &cancel);
        assert_eq!(alone, pooled);
        assert_eq!(alone.unwrap().files(), (1, 1));
        assert_eq!(progress.done.load(Ordering::Relaxed), 3);
        let stopped = CancelToken::new();
        stopped.cancel();
        let got = scan_odd_in(None, &cmp, l.path(), r.path(), &progress, &stopped);
        assert_eq!(got, Err(Cancelled));
    }

    #[test]
    fn the_scan_stops_when_cancelled() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(l.path(), "a.txt", "x\u{a0}\n".as_bytes());
        let cmp = compare(l.path(), r.path());
        let cancel = CancelToken::new();
        cancel.cancel();
        let got = scan_odd(
            &cmp,
            l.path(),
            r.path(),
            2,
            &OddProgress::default(),
            &cancel,
        );
        assert_eq!(got, Err(Cancelled));
    }

    #[test]
    fn swapped_marks_change_sides() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(l.path(), "a.txt", "x\u{a0}\n".as_bytes());
        put(r.path(), "a.txt", b"x\n");
        let cmp = compare(l.path(), r.path());
        let marks = scan_odd(
            &cmp,
            l.path(),
            r.path(),
            1,
            &OddProgress::default(),
            &CancelToken::new(),
        )
        .unwrap();
        let old = marks.get(0, true).cloned().unwrap();
        let swapped = marks.swapped();
        assert_eq!(swapped.get(0, false), Some(&old));
        assert!(swapped.get(0, true).is_none());
    }
}
