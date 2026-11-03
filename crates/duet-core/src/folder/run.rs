//! One-call folder comparison used by the CLI and the GUI.

use std::path::Path;

use crate::cancel::CancelToken;
use crate::folder::classify::apply_case;
use crate::folder::content::{ContentProgress, SIMILAR_LIMIT, classify_contents};
use crate::folder::entry::{Filters, ScanError, ScanProgress};
use crate::folder::scan::{ScanOptions, scan};
use crate::folder::tree::{FolderComparison, QuickRule, apply_size_rule, build_tree_with, rollup};
use crate::text::diff::{DiffOptions, WhitespaceMode};
use crate::text::lexer::detect_language;
use crate::text::rules::TextRules;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompareMode {
    #[default]
    Quick,
    Content,
    /// Lengths only: no times, no reads.
    Size,
    /// Content, with the text rules of the text tabs deciding "same text".
    Rules,
}

impl CompareMode {
    /// Whether this mode reads file contents (Content and Rules).
    pub fn reads_contents(self) -> bool {
        matches!(self, Self::Content | Self::Rules)
    }
}

/// Rules mode's compare rules (R8), kept as plain values so a compare can be built from them.
#[derive(Debug, Clone, Default)]
pub struct FolderTextRules {
    pub whitespace: WhitespaceMode,
    pub ignore_case: bool,
    pub ignore_blank: bool,
    pub ignore_comments: bool,
    pub text: Vec<regex::Regex>,
    pub lines: Vec<regex::Regex>,
}

impl FolderTextRules {
    /// R8: line endings are always ignored (the format steps handled them); comments are cut
    /// by `detect_language(name)` while `ignore_comments` is on.
    pub fn for_file(&self, name: &str) -> (DiffOptions, TextRules) {
        let options = DiffOptions {
            whitespace: self.whitespace,
            ignore_case: self.ignore_case,
            ignore_eol: true,
        };
        let rules = TextRules {
            ignore_blank: self.ignore_blank,
            text: self.text.clone(),
            lines: self.lines.clone(),
            comments: self.ignore_comments.then(|| detect_language(name)),
            anchors: Vec::new(),
        };
        (options, rules)
    }
}

/// Rules mode reads pairs up to this size a side (`fs:166`).
pub const RULES_LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct FolderOptions {
    pub mode: CompareMode,
    pub filters: Filters,
    pub case_insensitive: bool,
    pub scan_threads: usize,
    pub max_parallel_io: usize,
    pub quick: QuickRule,
    /// Content mode reads files up to this size whole to look for "≈" text-equal pairs.
    pub similar_limit: u64,
    pub text_rules: FolderTextRules,
    /// Count line-ending and BOM differences as "different" (R4).
    pub format_important: bool,
    /// Rules mode reads pairs up to this size a side.
    pub rules_limit: u64,
}

impl Default for FolderOptions {
    fn default() -> Self {
        Self {
            mode: CompareMode::Quick,
            filters: Filters::default(),
            case_insensitive: cfg!(windows),
            scan_threads: 0,
            max_parallel_io: std::thread::available_parallelism().map_or(4, |n| n.get()),
            quick: QuickRule::default(),
            similar_limit: SIMILAR_LIMIT,
            text_rules: FolderTextRules::default(),
            format_important: false,
            rules_limit: RULES_LIMIT,
        }
    }
}

#[derive(Debug, Default)]
pub struct FolderProgress {
    pub left: ScanProgress,
    pub right: ScanProgress,
    pub content: ContentProgress,
}

pub fn compare_folders(
    left: &Path,
    right: &Path,
    opts: &FolderOptions,
    progress: &FolderProgress,
    cancel: &CancelToken,
) -> Result<FolderComparison, ScanError> {
    let scan_opts = ScanOptions {
        filters: opts.filters.clone(),
        threads: opts.scan_threads,
    };
    let (l, r) = rayon::join(
        || scan(left, &scan_opts, &progress.left, cancel),
        || scan(right, &scan_opts, &progress.right, cancel),
    );
    let mut cmp = build_tree_with(l?, r?, opts.case_insensitive, opts.quick);
    match opts.mode {
        CompareMode::Quick => {}
        CompareMode::Size => apply_size_rule(&mut cmp),
        CompareMode::Content | CompareMode::Rules => {
            classify_contents(&mut cmp, left, right, opts, &progress.content, cancel)?;
        }
    }
    apply_case(&mut cmp);
    rollup(&mut cmp);
    Ok(cmp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder::classify::Warning;
    use crate::folder::content::compare_contents;
    use crate::folder::tree::{Side, Status};
    use std::fs::{self, File};
    use std::sync::atomic::Ordering;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn put(root: &Path, rel: &str, body: &[u8], mtime: SystemTime) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    }

    fn time(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn opts(mode: CompareMode) -> FolderOptions {
        FolderOptions {
            mode,
            ..FolderOptions::default()
        }
    }

    fn run(l: &Path, r: &Path, mode: CompareMode) -> FolderComparison {
        compare_folders(
            l,
            r,
            &opts(mode),
            &FolderProgress::default(),
            &CancelToken::new(),
        )
        .unwrap()
    }

    #[test]
    fn same_bytes_with_different_times_is_same_only_in_content_mode() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(l.path(), "sub/a.txt", b"hello", time(1_000_000));
        put(r.path(), "sub/a.txt", b"hello", time(2_000_000));
        let quick = run(l.path(), r.path(), CompareMode::Quick);
        assert_eq!(
            quick.nodes[1].status,
            Status::Different {
                newer: Some(Side::Right)
            }
        );
        let content = run(l.path(), r.path(), CompareMode::Content);
        assert!(content.is_identical(), "{:?}", content.nodes);
    }

    #[test]
    fn last_byte_difference_across_chunks_is_found() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut body = vec![7u8; crate::folder::content::CHUNK_LEN * 2 + 1];
        put(l.path(), "big.bin", &body, time(5));
        *body.last_mut().unwrap() = 8;
        put(r.path(), "big.bin", &body, time(5));
        let cmp = run(l.path(), r.path(), CompareMode::Content);
        assert_eq!(cmp.nodes[0].status, Status::Different { newer: None });
    }

    #[test]
    fn vanished_file_becomes_an_error_row() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(l.path(), "a.txt", b"x", time(5));
        put(r.path(), "a.txt", b"x", time(5));
        let mut cmp = run(l.path(), r.path(), CompareMode::Quick);
        fs::remove_file(r.path().join("a.txt")).unwrap();
        let progress = ContentProgress::default();
        compare_contents(
            &mut cmp,
            l.path(),
            r.path(),
            2,
            SIMILAR_LIMIT,
            &progress,
            &CancelToken::new(),
        )
        .unwrap();
        assert!(
            matches!(cmp.nodes[0].status, Status::Error(_)),
            "{:?}",
            cmp.nodes[0].status
        );
    }

    #[cfg(windows)]
    #[test]
    fn exclusively_locked_file_becomes_an_error_row() {
        use std::os::windows::fs::OpenOptionsExt;
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(l.path(), "a.txt", b"x", time(5));
        put(r.path(), "a.txt", b"x", time(5));
        let _lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(r.path().join("a.txt"))
            .unwrap();
        let cmp = run(l.path(), r.path(), CompareMode::Content);
        assert!(
            matches!(cmp.nodes[0].status, Status::Error(_)),
            "{:?}",
            cmp.nodes[0].status
        );
    }

    #[test]
    fn cancelled_compare_returns_error() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(l.path(), "a.txt", b"x", time(5));
        put(r.path(), "a.txt", b"x", time(5));
        let cancel = CancelToken::new();
        cancel.cancel();
        let progress = FolderProgress::default();
        let result = compare_folders(
            l.path(),
            r.path(),
            &opts(CompareMode::Content),
            &progress,
            &cancel,
        );
        assert!(matches!(result, Err(ScanError::Cancelled(_))));
    }

    #[test]
    fn progress_reports_content_work() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        for name in ["a", "b", "c"] {
            put(l.path(), name, b"same", time(5));
            put(r.path(), name, b"same", time(9));
        }
        let progress = FolderProgress::default();
        let cancel = CancelToken::new();
        compare_folders(
            l.path(),
            r.path(),
            &opts(CompareMode::Content),
            &progress,
            &cancel,
        )
        .unwrap();
        assert_eq!(progress.content.total.load(Ordering::Relaxed), 3);
        assert_eq!(progress.content.done.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn files_equal_direct() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "a", b"abc", time(1));
        put(dir.path(), "b", b"abc", time(1));
        put(dir.path(), "c", b"abd", time(1));
        let cancel = CancelToken::new();
        let p = |n: &str| dir.path().join(n);
        assert!(crate::folder::content::files_equal(&p("a"), &p("b"), &cancel).unwrap());
        assert!(!crate::folder::content::files_equal(&p("a"), &p("c"), &cancel).unwrap());
    }

    fn node<'a>(cmp: &'a FolderComparison, rel: &str) -> &'a crate::folder::tree::Node {
        cmp.nodes.iter().find(|n| n.rel == rel).unwrap()
    }

    fn run_with(l: &Path, r: &Path, opts: &FolderOptions) -> FolderComparison {
        compare_folders(l, r, opts, &FolderProgress::default(), &CancelToken::new()).unwrap()
    }

    #[test]
    fn size_only_ignores_times_and_contents() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(l.path(), "a.txt", b"abc", time(1_000));
        put(r.path(), "a.txt", b"xyz", time(1_100));
        put(l.path(), "b.txt", b"ab", time(1_000));
        put(r.path(), "b.txt", b"abc", time(1_100));
        let cmp = run(l.path(), r.path(), CompareMode::Size);
        assert_eq!(node(&cmp, "a.txt").status, Status::Same);
        assert_eq!(
            node(&cmp, "b.txt").status,
            Status::Different {
                newer: Some(Side::Right)
            }
        );
    }

    #[test]
    fn rules_mode_ignores_what_the_text_rules_hide() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(l.path(), "a.txt", b"x  y\n", time(5));
        put(r.path(), "a.txt", b"x y\n", time(5));
        put(l.path(), "b.txt", b"a\n", time(5));
        put(r.path(), "b.txt", b"b\n", time(5));
        put(l.path(), "run.sh", b"#!/bin/bash\necho  hi\n", time(5));
        put(r.path(), "run.sh", b"#!/bin/bash\r\necho hi\r\n", time(5));
        let opts = FolderOptions {
            mode: CompareMode::Rules,
            text_rules: FolderTextRules {
                whitespace: WhitespaceMode::All,
                ..FolderTextRules::default()
            },
            ..FolderOptions::default()
        };
        let cmp = run_with(l.path(), r.path(), &opts);
        let a = node(&cmp, "a.txt");
        assert!(matches!(a.status, Status::Similar { .. }), "{:?}", a.status);
        assert!(a.note.as_ref().unwrap().soft);
        assert!(matches!(
            node(&cmp, "b.txt").status,
            Status::Different { .. }
        ));
        let sh = node(&cmp, "run.sh");
        assert!(
            matches!(sh.status, Status::Different { .. }),
            "{:?}",
            sh.status
        );
        assert_eq!(sh.warning(), Some(Warning::Script));
        // Plain Content mode sees the whitespace as a real difference.
        let content = run(l.path(), r.path(), CompareMode::Content);
        assert!(matches!(
            node(&content, "a.txt").status,
            Status::Different { .. }
        ));
    }

    #[test]
    fn rules_mode_reads_only_up_to_its_limit() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(l.path(), "a.txt", b"x  y\n", time(5));
        put(r.path(), "a.txt", b"x y\n", time(5));
        let mut opts = FolderOptions {
            mode: CompareMode::Rules,
            text_rules: FolderTextRules {
                whitespace: WhitespaceMode::All,
                ..FolderTextRules::default()
            },
            rules_limit: 4,
            ..FolderOptions::default()
        };
        let cmp = run_with(l.path(), r.path(), &opts);
        assert!(matches!(
            node(&cmp, "a.txt").status,
            Status::Different { .. }
        ));
        opts.rules_limit = 64;
        let cmp = run_with(l.path(), r.path(), &opts);
        assert!(matches!(node(&cmp, "a.txt").status, Status::Similar { .. }));
    }

    #[test]
    fn the_format_rule_marks_similar_files_different() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(l.path(), "crlf.txt", b"a\r\nb\r\n", time(5));
        put(r.path(), "crlf.txt", b"a\nb\n", time(5));
        let mut opts = opts(CompareMode::Content);
        let cmp = run_with(l.path(), r.path(), &opts);
        assert!(matches!(
            node(&cmp, "crlf.txt").status,
            Status::Similar { .. }
        ));
        opts.format_important = true;
        let cmp = run_with(l.path(), r.path(), &opts);
        let n = node(&cmp, "crlf.txt");
        assert!(
            matches!(n.status, Status::Different { .. }),
            "{:?}",
            n.status
        );
        assert!(n.note.as_ref().unwrap().formats.is_some());
        assert!(!cmp.is_identical());
    }

    #[test]
    fn case_only_names_are_flagged_in_every_mode() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(l.path(), "Config.xml", b"same\n", time(5));
        put(r.path(), "config.xml", b"same\n", time(5));
        for mode in [CompareMode::Quick, CompareMode::Content, CompareMode::Size] {
            let opts = FolderOptions {
                mode,
                case_insensitive: true,
                ..FolderOptions::default()
            };
            let cmp = run_with(l.path(), r.path(), &opts);
            assert_eq!(cmp.nodes.len(), 1, "{mode:?}");
            let n = &cmp.nodes[0];
            assert!(matches!(n.status, Status::Different { .. }), "{mode:?}");
            assert_eq!(n.warning(), Some(Warning::CaseOnly), "{mode:?}");
        }
    }

    #[test]
    fn case_only_names_make_the_folder_different() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(l.path(), "d/Config.xml", b"same\n", time(5));
        put(r.path(), "d/config.xml", b"same\n", time(5));
        let opts = FolderOptions {
            case_insensitive: true,
            ..FolderOptions::default()
        };
        let cmp = run_with(l.path(), r.path(), &opts);
        assert!(matches!(cmp.nodes[0].status, Status::Different { .. }));
    }
}
