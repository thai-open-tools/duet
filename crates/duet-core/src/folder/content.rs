//! Byte-for-byte comparison of same-size file pairs with early exit on the first mismatch.
//! No hashing: hashing must read both files fully, a direct compare can stop early.

use std::borrow::Cow;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rayon::prelude::*;

use crate::cancel::{CancelToken, Cancelled};
use crate::folder::classify::{LineRules, Note, classify, derive_status};
use crate::folder::entry::{EntryKind, ScanError, join_rel};
use crate::folder::run::{CompareMode, FolderOptions};
use crate::folder::tree::{FolderComparison, Side, Status, newer_side, rollup};
use crate::text::encoding::{decode, is_binary};

pub const CHUNK_LEN: usize = 1024 * 1024;

/// Files above this size are not read whole for the "≈" check.
pub const SIMILAR_LIMIT: u64 = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ContentError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Cancelled(#[from] Cancelled),
}

#[derive(Debug, Default)]
pub struct ContentProgress {
    pub total: AtomicU64,
    pub done: AtomicU64,
}

pub fn files_equal(a: &Path, b: &Path, cancel: &CancelToken) -> Result<bool, ContentError> {
    let mut buffers = (vec![0u8; CHUNK_LEN], vec![0u8; CHUNK_LEN]);
    files_equal_with(a, b, &mut buffers, cancel)
}

fn files_equal_with(
    a: &Path,
    b: &Path,
    buffers: &mut (Vec<u8>, Vec<u8>),
    cancel: &CancelToken,
) -> Result<bool, ContentError> {
    let mut fa = File::open(a)?;
    let mut fb = File::open(b)?;
    if fa.metadata()?.len() != fb.metadata()?.len() {
        return Ok(false);
    }
    loop {
        cancel.check()?;
        let na = read_full(&mut fa, &mut buffers.0)?;
        let nb = read_full(&mut fb, &mut buffers.1)?;
        if na != nb || buffers.0[..na] != buffers.1[..nb] {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
    }
}

fn read_full(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Bytes read from the start of each file to spot binary files and UTF-16 before streaming.
const SNIFF_LEN: u64 = 8 * 1024;

/// True when both files hold the same text once a BOM is dropped and CRLF, CR and LF count as
/// one. Streams with an early exit and checks `cancel` as it goes; UTF-16 files are decoded
/// whole (the caller bounds their size). Binary files are never text-equal.
pub fn text_equal(a: &Path, b: &Path, cancel: &CancelToken) -> Result<bool, ContentError> {
    cancel.check()?;
    let (mut fa, mut fb) = (File::open(a)?, File::open(b)?);
    let (head_a, head_b) = (sniff(&mut fa)?, sniff(&mut fb)?);
    let utf16 = |head: &[u8]| head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF]);
    if utf16(&head_a) || utf16(&head_b) {
        return decoded_equal(a, b);
    }
    if is_binary(&head_a) || is_binary(&head_b) {
        return Ok(false);
    }
    let mut x = Normalized::new(head_a, fa);
    let mut y = Normalized::new(head_b, fb);
    let mut seen = 0usize;
    loop {
        if seen.is_multiple_of(CHUNK_LEN) {
            cancel.check()?;
        }
        match (x.next()?, y.next()?) {
            (None, None) => return Ok(true),
            (l, r) if l != r => return Ok(false),
            _ => seen += 1,
        }
    }
}

fn sniff(file: &mut File) -> io::Result<Vec<u8>> {
    let mut head = Vec::with_capacity(SNIFF_LEN as usize);
    file.by_ref().take(SNIFF_LEN).read_to_end(&mut head)?;
    Ok(head)
}

/// The decoded-text comparison, for UTF-16 files.
fn decoded_equal(a: &Path, b: &Path) -> Result<bool, ContentError> {
    let (x, y) = (std::fs::read(a)?, std::fs::read(b)?);
    let (dx, dy) = (decode(&x), decode(&y));
    if dx.lossy || dy.lossy {
        return Ok(false);
    }
    Ok(normalize_eol(&dx.text) == normalize_eol(&dy.text))
}

fn normalize_eol(text: &str) -> Cow<'_, str> {
    if text.contains('\r') {
        Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(text)
    }
}

/// A file's bytes with a leading UTF-8 BOM dropped and every CRLF or CR read as LF.
struct Normalized {
    bytes: io::Bytes<BufReader<io::Chain<io::Cursor<Vec<u8>>, File>>>,
    peeked: Option<u8>,
}

impl Normalized {
    fn new(mut head: Vec<u8>, rest: File) -> Self {
        if head.starts_with(&[0xEF, 0xBB, 0xBF]) {
            head.drain(..3);
        }
        let reader = BufReader::with_capacity(64 * 1024, io::Cursor::new(head).chain(rest));
        Self {
            bytes: reader.bytes(),
            peeked: None,
        }
    }

    fn next(&mut self) -> io::Result<Option<u8>> {
        let byte = match self.peeked.take() {
            Some(byte) => Some(byte),
            None => self.bytes.next().transpose()?,
        };
        if byte == Some(b'\r') {
            match self.bytes.next().transpose()? {
                Some(b'\n') | None => {}
                Some(other) => self.peeked = Some(other),
            }
            return Ok(Some(b'\n'));
        }
        Ok(byte)
    }
}

/// Threads that read and decode whole files at once (PF9, R5): at most about 4 x 128 MiB.
const READ_THREADS: usize = 4;

/// One file pair that needs a verdict.
struct Job {
    index: usize,
    left: PathBuf,
    right: PathBuf,
    name: String,
    rel: String,
    same_size: bool,
    /// Small enough to be read whole.
    small: bool,
    newer: Option<Side>,
}

/// What the byte compare of one pair decided.
enum Bytes {
    Equal,
    /// Different, or not compared because the sizes differ: the text check decides.
    Unequal,
}

type Outcome = (usize, Status, Option<Box<Note>>);

/// R5 over every file pair on both sides that needs a read, with `opts.mode` Content or Rules.
pub fn classify_contents(
    cmp: &mut FolderComparison,
    left_root: &Path,
    right_root: &Path,
    opts: &FolderOptions,
    progress: &ContentProgress,
    cancel: &CancelToken,
) -> Result<(), ScanError> {
    let limit = if opts.mode == CompareMode::Rules {
        opts.rules_limit
    } else {
        opts.similar_limit
    };
    let jobs: Vec<Job> = cmp
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            let (l, r) = (node.left.as_ref()?, node.right.as_ref()?);
            let files = l.kind == EntryKind::File
                && r.kind == EntryKind::File
                && l.error.is_none()
                && r.error.is_none();
            let same_size = l.len == r.len;
            let small = l.len <= limit && r.len <= limit;
            (files && (same_size || small)).then(|| Job {
                index,
                left: join_rel(left_root, &l.rel),
                right: join_rel(right_root, &r.rel),
                name: node.name.clone(),
                rel: node.rel.clone(),
                same_size,
                small,
                newer: newer_side(l, r, opts.quick),
            })
        })
        .collect();
    for job in &jobs {
        cmp.nodes[job.index].status = Status::Pending;
    }
    progress.total.store(jobs.len() as u64, Ordering::Relaxed);
    let done = || {
        progress.done.fetch_add(1, Ordering::Relaxed);
    };

    // Pass 1: equal bytes (streamed, early exit) on as many threads as the caller allows.
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(opts.max_parallel_io.max(1))
        .build()?;
    let bytes: Vec<Result<Bytes, ContentError>> = pool.install(|| {
        jobs.par_iter()
            .map_init(
                || (vec![0u8; CHUNK_LEN], vec![0u8; CHUNK_LEN]),
                |buffers, job| {
                    if job.same_size {
                        match files_equal_with(&job.left, &job.right, buffers, cancel) {
                            Ok(true) => {
                                done();
                                return Ok(Bytes::Equal);
                            }
                            Ok(false) => {}
                            Err(e) => {
                                done();
                                return Err(e);
                            }
                        }
                    }
                    if !job.small {
                        // Too big to read whole: a pair that differs in bytes differs.
                        done();
                    }
                    Ok(Bytes::Unequal)
                },
            )
            .collect()
    });
    cancel.check()?;

    let mut outcomes: Vec<Outcome> = Vec::with_capacity(jobs.len());
    let mut to_classify: Vec<&Job> = Vec::new();
    for (job, result) in jobs.iter().zip(bytes) {
        match result {
            Ok(Bytes::Equal) => outcomes.push((job.index, Status::Same, None)),
            Ok(Bytes::Unequal) if job.small => to_classify.push(job),
            Ok(Bytes::Unequal) => {
                let status = Status::Different { newer: job.newer };
                outcomes.push((job.index, status, None));
            }
            Err(ContentError::Io(err)) => {
                outcomes.push((job.index, Status::Error(err.to_string()), None));
            }
            Err(ContentError::Cancelled(c)) => return Err(c.into()),
        }
    }

    // Pass 2: whole-file reads, decodes and diffs on at most READ_THREADS threads.
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(opts.max_parallel_io.clamp(1, READ_THREADS))
        .build()?;
    let classified: Vec<Result<Outcome, Cancelled>> = pool.install(|| {
        to_classify
            .par_iter()
            .map(|job| {
                let outcome = classify_pair(job, opts, cancel);
                done();
                outcome
            })
            .collect()
    });
    for outcome in classified {
        outcomes.push(outcome?);
    }
    cancel.check()?;
    for (index, status, note) in outcomes {
        let node = &mut cmp.nodes[index];
        node.status = status;
        node.note = note;
    }
    rollup(cmp);
    Ok(())
}

/// Reads one pair whole and gives it its R5 verdict and R6 status. A pair that cannot be read
/// again stays Different: its bytes already differ.
fn classify_pair(
    job: &Job,
    opts: &FolderOptions,
    cancel: &CancelToken,
) -> Result<Outcome, Cancelled> {
    let different = (job.index, Status::Different { newer: job.newer }, None);
    cancel.check()?;
    let Ok(Some((left, right))) = read_pair(job) else {
        return Ok(different);
    };
    let (line_options, text_rules);
    let rules = if opts.mode == CompareMode::Rules {
        (line_options, text_rules) = opts.text_rules.for_file(&job.name);
        Some(LineRules {
            options: line_options,
            rules: &text_rules,
        })
    } else {
        None
    };
    let verdict = classify(&left, &right, rules.as_ref(), cancel)?;
    let (status, note) = derive_status(&verdict, &job.rel, job.newer, opts.format_important);
    Ok((job.index, status, note))
}

/// Both files whole, after a look at their heads; `None` when either is binary.
fn read_pair(job: &Job) -> io::Result<Option<(Vec<u8>, Vec<u8>)>> {
    let (mut fl, mut fr) = (File::open(&job.left)?, File::open(&job.right)?);
    if is_binary(&sniff(&mut fl)?) || is_binary(&sniff(&mut fr)?) {
        return Ok(None);
    }
    let (mut left, mut right) = (Vec::new(), Vec::new());
    fl.rewind()?;
    fr.rewind()?;
    fl.read_to_end(&mut left)?;
    fr.read_to_end(&mut right)?;
    Ok(Some((left, right)))
}

/// The same-size and under-the-limit file pairs of `cmp` in Content mode (the default rule).
pub fn compare_contents(
    cmp: &mut FolderComparison,
    left_root: &Path,
    right_root: &Path,
    max_parallel_io: usize,
    similar_limit: u64,
    progress: &ContentProgress,
    cancel: &CancelToken,
) -> Result<(), ScanError> {
    let opts = FolderOptions {
        mode: CompareMode::Content,
        max_parallel_io,
        similar_limit,
        quick: cmp.quick,
        ..FolderOptions::default()
    };
    classify_contents(cmp, left_root, right_root, &opts, progress, cancel)
}

#[cfg(test)]
mod similar_tests {
    use crate::cancel::CancelToken;
    use crate::folder::run::{CompareMode, FolderOptions, compare_folders};
    use crate::folder::tree::Status;

    fn statuses(opts: &FolderOptions, files: &[(&str, &[u8], &[u8])]) -> Vec<(String, Status)> {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        for (name, left, right) in files {
            std::fs::write(l.path().join(name), left).unwrap();
            std::fs::write(r.path().join(name), right).unwrap();
        }
        let cmp = compare_folders(
            l.path(),
            r.path(),
            opts,
            &Default::default(),
            &CancelToken::new(),
        )
        .unwrap();
        cmp.nodes.into_iter().map(|n| (n.rel, n.status)).collect()
    }

    fn notes(
        opts: &FolderOptions,
        files: &[(&str, &[u8], &[u8])],
    ) -> Vec<(String, crate::folder::classify::Note)> {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        for (name, left, right) in files {
            std::fs::write(l.path().join(name), left).unwrap();
            std::fs::write(r.path().join(name), right).unwrap();
        }
        let cmp = compare_folders(
            l.path(),
            r.path(),
            opts,
            &Default::default(),
            &CancelToken::new(),
        )
        .unwrap();
        cmp.nodes
            .into_iter()
            .map(|n| (n.rel, *n.note.expect("a note")))
            .collect()
    }

    #[test]
    fn line_endings_and_bom_only_are_similar() {
        let opts = FolderOptions {
            mode: CompareMode::Content,
            ..FolderOptions::default()
        };
        let got = statuses(
            &opts,
            &[
                ("bom.txt", b"\xEF\xBB\xBFx\n", b"x\n"),
                ("crlf.txt", b"a\r\nb\r\n", b"a\nb\n"),
                ("real.txt", b"a\n", b"b\n"),
                ("bin.dat", b"\0\r\n", b"\0\n"),
            ],
        );
        let status = |name: &str| got.iter().find(|(rel, _)| rel == name).unwrap().1.clone();
        assert!(matches!(status("crlf.txt"), Status::Similar { .. }));
        assert!(matches!(status("bom.txt"), Status::Similar { .. }));
        assert!(matches!(status("real.txt"), Status::Different { .. }));
        assert!(
            matches!(status("bin.dat"), Status::Different { .. }),
            "binary files are never text-equal"
        );
    }

    #[test]
    fn cr_only_text_is_similar_and_an_encoding_change_is_a_difference() {
        let opts = FolderOptions {
            mode: CompareMode::Content,
            ..FolderOptions::default()
        };
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("x\r\ny\r\n".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        let notes = notes(&opts, &[("wide.txt", &utf16, b"x\ny\n")]);
        let note = |name: &str| notes.iter().find(|(rel, _)| rel == name).unwrap().1.clone();
        let got = statuses(
            &opts,
            &[
                ("cr.txt", b"a\rb\r", b"a\nb\n"),
                ("wide.txt", &utf16, b"x\ny\n"),
                ("late.txt", b"same\nsame\nX\n", b"same\r\nsame\r\nY\r\n"),
            ],
        );
        let status = |name: &str| got.iter().find(|(rel, _)| rel == name).unwrap().1.clone();
        assert!(matches!(status("cr.txt"), Status::Similar { .. }));
        // R6 (C10): the same text in another encoding is a difference, and the note says why.
        assert!(matches!(status("wide.txt"), Status::Different { .. }));
        assert!(note("wide.txt").encoding_differs());
        assert!(matches!(status("late.txt"), Status::Different { .. }));
    }

    #[test]
    fn the_text_check_stops_when_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::write(&a, "x\r\n").unwrap();
        std::fs::write(&b, "x\n").unwrap();
        let cancel = CancelToken::new();
        assert!(super::text_equal(&a, &b, &cancel).unwrap());
        cancel.cancel();
        assert!(matches!(
            super::text_equal(&a, &b, &cancel),
            Err(super::ContentError::Cancelled(_))
        ));
    }

    #[test]
    fn an_unreadable_pair_of_different_sizes_stays_different() {
        use crate::folder::entry::{Entry, EntryKind};
        use crate::folder::tree::build_tree;
        let file = |len| Entry {
            rel: "gone.txt".into(),
            kind: EntryKind::File,
            len,
            modified: None,
            error: None,
            link_target: None,
        };
        // Neither file exists on disk, as if both were locked or deleted after the scan.
        let mut cmp = build_tree(vec![file(3)], vec![file(5)], true);
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        super::compare_contents(
            &mut cmp,
            l.path(),
            r.path(),
            2,
            super::SIMILAR_LIMIT,
            &Default::default(),
            &CancelToken::new(),
        )
        .unwrap();
        assert!(
            matches!(cmp.nodes[0].status, Status::Different { .. }),
            "{:?}",
            cmp.nodes[0].status
        );
    }

    #[test]
    fn files_over_the_limit_are_not_checked() {
        let opts = FolderOptions {
            mode: CompareMode::Content,
            similar_limit: 4,
            ..FolderOptions::default()
        };
        let got = statuses(&opts, &[("crlf.txt", b"a\r\nb\r\n", b"a\nb\n")]);
        assert!(matches!(got[0].1, Status::Different { .. }));
    }

    #[test]
    fn newer_follows_the_configured_tolerance() {
        use crate::folder::tree::{QuickRule, Side};
        use std::time::{Duration, UNIX_EPOCH};
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        for (root, body, secs) in [(&l, "x\r\n", 1_000), (&r, "x\n", 1_003)] {
            let path = root.path().join("a.txt");
            std::fs::write(&path, body).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(UNIX_EPOCH + Duration::from_secs(secs))
                .unwrap();
        }
        let status = |secs: u64| {
            let opts = FolderOptions {
                mode: CompareMode::Content,
                quick: QuickRule {
                    tolerance: Duration::from_secs(secs),
                    ignore_dst: false,
                },
                ..FolderOptions::default()
            };
            let cmp = compare_folders(
                l.path(),
                r.path(),
                &opts,
                &Default::default(),
                &CancelToken::new(),
            )
            .unwrap();
            cmp.nodes[0].status.clone()
        };
        assert_eq!(status(5), Status::Similar { newer: None });
        assert_eq!(
            status(2),
            Status::Similar {
                newer: Some(Side::Right)
            }
        );
    }

    #[test]
    fn a_missing_final_newline_is_similar() {
        let opts = FolderOptions {
            mode: CompareMode::Content,
            ..FolderOptions::default()
        };
        let got = statuses(&opts, &[("nl.txt", b"a\nb", b"a\nb\n")]);
        assert!(matches!(got[0].1, Status::Similar { .. }), "{:?}", got[0].1);
    }
}
