//! Parallel folder scan (ripgrep's walker) and a shallow first-level listing.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::{WalkBuilder, WalkState};

use crate::cancel::CancelToken;
use crate::folder::entry::{Entry, EntryKind, Filters, ScanError, ScanProgress};

#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    pub filters: Filters,
    /// Worker threads; 0 lets the walker choose.
    pub threads: usize,
}

/// Patterns as globs: a plain one is tried on the name and on the path; one that starts with
/// `/` is rooted, tried on the path only, so it names an item at the top level and nothing
/// deeper.
struct PatternSet {
    any: GlobSet,
    rooted: GlobSet,
}

impl PatternSet {
    fn new(patterns: &[String]) -> Result<Self, globset::Error> {
        let (rooted, any): (Vec<&String>, Vec<&String>) =
            patterns.iter().partition(|p| p.starts_with('/'));
        let rooted: Vec<String> = rooted.iter().map(|p| p[1..].to_owned()).collect();
        let any: Vec<String> = any.into_iter().cloned().collect();
        Ok(Self {
            any: build_set(&any)?,
            rooted: build_set(&rooted)?,
        })
    }

    fn is_match(&self, rel: &str, name: &str) -> bool {
        self.any.is_match(name) || self.any.is_match(rel) || self.rooted.is_match(rel)
    }
}

struct CompiledFilters {
    include: Option<PatternSet>,
    exclude: PatternSet,
}

impl CompiledFilters {
    fn new(filters: &Filters) -> Result<Self, globset::Error> {
        let include = if filters.include.is_empty() {
            None
        } else {
            Some(PatternSet::new(&filters.include)?)
        };
        Ok(Self {
            include,
            exclude: PatternSet::new(&filters.exclude)?,
        })
    }

    fn excluded(&self, rel: &str, name: &str) -> bool {
        self.exclude.is_match(rel, name)
    }

    fn included(&self, rel: &str, name: &str) -> bool {
        self.include
            .as_ref()
            .is_none_or(|set| set.is_match(rel, name))
    }
}

/// Checks the include and exclude patterns exactly as `scan` compiles them, so a bad one is
/// reported before any work starts.
pub fn validate_filters(filters: &Filters) -> Result<(), ScanError> {
    PatternSet::new(&filters.include)?;
    PatternSet::new(&filters.exclude)?;
    Ok(())
}

fn build_set(patterns: &[String]) -> Result<GlobSet, globset::Error> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            GlobBuilder::new(pattern)
                .case_insensitive(cfg!(windows))
                .build()?,
        );
    }
    builder.build()
}

enum Item {
    Entry(Entry),
    Error { rel: String, message: String },
}

fn check_root(root: &Path) -> Result<(), ScanError> {
    let meta = fs::metadata(root).map_err(|source| ScanError::Root {
        path: root.to_path_buf(),
        source,
    })?;
    if meta.is_dir() {
        Ok(())
    } else {
        Err(ScanError::NotADirectory(root.to_path_buf()))
    }
}

fn rel_path(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn error_path(err: &ignore::Error) -> Option<&Path> {
    match err {
        ignore::Error::WithPath { path, .. } => Some(path),
        ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => {
            error_path(err)
        }
        ignore::Error::Loop { child, .. } => Some(child),
        _ => None,
    }
}

pub fn scan(
    root: &Path,
    opts: &ScanOptions,
    progress: &ScanProgress,
    cancel: &CancelToken,
) -> Result<Vec<Entry>, ScanError> {
    check_root(root)?;
    let filters = CompiledFilters::new(&opts.filters)?;
    cancel.check()?;
    let (tx, rx) = mpsc::channel::<Item>();
    let walker = WalkBuilder::new(root)
        .standard_filters(false)
        .follow_links(false)
        .threads(opts.threads)
        .build_parallel();
    walker.run(|| {
        let tx = tx.clone();
        let filters = &filters;
        Box::new(move |result| {
            if cancel.is_cancelled() {
                return WalkState::Quit;
            }
            match result {
                Ok(dent) => {
                    if dent.depth() == 0 {
                        return WalkState::Continue;
                    }
                    let rel = rel_path(root, dent.path());
                    let name = dent.file_name().to_string_lossy();
                    let kind = if dent.path_is_symlink() {
                        EntryKind::Symlink
                    } else if dent.file_type().is_some_and(|t| t.is_dir()) {
                        EntryKind::Dir
                    } else {
                        EntryKind::File
                    };
                    if filters.excluded(&rel, &name) {
                        return if kind == EntryKind::Dir {
                            WalkState::Skip
                        } else {
                            WalkState::Continue
                        };
                    }
                    if kind == EntryKind::File && !filters.included(&rel, &name) {
                        return WalkState::Continue;
                    }
                    let (len, modified, error) = match dent.metadata() {
                        Ok(meta) => (
                            if kind == EntryKind::Dir {
                                0
                            } else {
                                meta.len()
                            },
                            meta.modified().ok(),
                            None,
                        ),
                        Err(err) => (0, None, Some(err.to_string())),
                    };
                    let link_target = (kind == EntryKind::Symlink)
                        .then(|| fs::read_link(dent.path()).ok())
                        .flatten();
                    progress.entries.fetch_add(1, Ordering::Relaxed);
                    let _ = tx.send(Item::Entry(Entry {
                        rel,
                        kind,
                        len,
                        modified,
                        error,
                        link_target,
                    }));
                }
                Err(err) => {
                    if let Some(path) = error_path(&err) {
                        let rel = rel_path(root, path);
                        let _ = tx.send(Item::Error {
                            rel,
                            message: err.to_string(),
                        });
                    }
                }
            }
            WalkState::Continue
        })
    });
    drop(tx);
    cancel.check()?;
    merge(rx, root)
}

fn merge(rx: mpsc::Receiver<Item>, root: &Path) -> Result<Vec<Entry>, ScanError> {
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for item in rx {
        match item {
            Item::Entry(entry) => entries.push(entry),
            Item::Error { rel, message } => errors.push((rel, message)),
        }
    }
    entries.sort_by(|a, b| a.rel.cmp(&b.rel));
    for (rel, message) in errors {
        if rel.is_empty() {
            // The root itself could not be listed: reporting it as an empty folder would make
            // every file on the other side look like an orphan.
            return Err(ScanError::Root {
                path: root.to_path_buf(),
                source: io::Error::other(message),
            });
        }
        match entries.binary_search_by(|e| e.rel.as_str().cmp(rel.as_str())) {
            Ok(i) => entries[i].error = Some(message),
            Err(i) => entries.insert(
                i,
                Entry {
                    rel,
                    kind: EntryKind::Unknown,
                    len: 0,
                    modified: None,
                    error: Some(message),
                    link_target: None,
                },
            ),
        }
    }
    Ok(entries)
}

pub fn scan_shallow(root: &Path, filters: &Filters) -> Result<Vec<Entry>, ScanError> {
    check_root(root)?;
    let filters = CompiledFilters::new(filters)?;
    let listing = fs::read_dir(root).map_err(|source| ScanError::Root {
        path: root.to_path_buf(),
        source,
    })?;
    let mut entries = Vec::new();
    for item in listing.flatten() {
        let rel = item.file_name().to_string_lossy().into_owned();
        let kind = match item.file_type() {
            Ok(t) if t.is_symlink() => EntryKind::Symlink,
            Ok(t) if t.is_dir() => EntryKind::Dir,
            Ok(_) => EntryKind::File,
            Err(_) => EntryKind::Unknown,
        };
        if filters.excluded(&rel, &rel)
            || (kind == EntryKind::File && !filters.included(&rel, &rel))
        {
            continue;
        }
        let link_target = (kind == EntryKind::Symlink)
            .then(|| fs::read_link(item.path()).ok())
            .flatten();
        let entry = match item.metadata() {
            Ok(meta) => Entry {
                len: if kind == EntryKind::Dir {
                    0
                } else {
                    meta.len()
                },
                modified: meta.modified().ok(),
                error: None,
                rel,
                kind,
                link_target,
            },
            Err(err) => Entry {
                rel,
                kind,
                len: 0,
                modified: None,
                error: Some(err.to_string()),
                link_target,
            },
        };
        entries.push(entry);
    }
    entries.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn touch(root: &Path, rel: &str, body: &str) -> PathBuf {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }

    fn rels(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.rel.as_str()).collect()
    }

    fn run(root: &Path, filters: Filters) -> Vec<Entry> {
        let opts = ScanOptions {
            filters,
            threads: 2,
        };
        scan(root, &opts, &ScanProgress::default(), &CancelToken::new()).unwrap()
    }

    #[test]
    fn lists_files_and_dirs_with_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "a.txt", "12345");
        touch(dir.path(), "sub/b.txt", "");
        touch(dir.path(), "ไทย/ไฟล์.txt", "");
        let entries = run(dir.path(), Filters::default());
        assert_eq!(
            rels(&entries),
            vec!["a.txt", "sub", "sub/b.txt", "ไทย", "ไทย/ไฟล์.txt"]
        );
        assert_eq!(entries[0].kind, EntryKind::File);
        assert_eq!(entries[0].len, 5);
        assert!(entries[0].modified.is_some());
        assert_eq!(entries[1].kind, EntryKind::Dir);
        assert_eq!(entries[4].name(), "ไฟล์.txt");
    }

    #[test]
    fn exclude_skips_whole_subtree() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "node_modules/x/y.js", "");
        touch(dir.path(), "src/main.rs", "");
        let filters = Filters {
            exclude: vec!["node_modules".into()],
            ..Filters::default()
        };
        assert_eq!(rels(&run(dir.path(), filters)), vec!["src", "src/main.rs"]);
    }

    #[test]
    fn include_filters_files_only() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "src/main.rs", "");
        touch(dir.path(), "src/notes.txt", "");
        let filters = Filters {
            include: vec!["*.rs".into()],
            ..Filters::default()
        };
        assert_eq!(rels(&run(dir.path(), filters)), vec!["src", "src/main.rs"]);
    }

    #[test]
    fn hidden_and_gitignored_files_are_listed() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), ".gitignore", "*.log\n");
        touch(dir.path(), "app.log", "");
        assert_eq!(
            rels(&run(dir.path(), Filters::default())),
            vec![".gitignore", "app.log"]
        );
    }

    #[test]
    fn paths_longer_than_260_characters_are_scanned() {
        let dir = tempfile::tempdir().unwrap();
        let deep: Vec<String> = (0..6).map(|i| format!("{i}{}", "d".repeat(50))).collect();
        let rel = format!("{}/deep.txt", deep.join("/"));
        let path = touch(dir.path(), &rel, "x");
        assert!(path.as_os_str().len() > 300);
        let entries = run(dir.path(), Filters::default());
        assert!(
            entries
                .iter()
                .any(|e| e.rel == rel && e.kind == EntryKind::File)
        );
    }

    #[test]
    fn progress_counts_entries() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "a/b.txt", "");
        let progress = ScanProgress::default();
        let entries = scan(
            dir.path(),
            &ScanOptions::default(),
            &progress,
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(
            progress.entries.load(Ordering::Relaxed),
            entries.len() as u64
        );
    }

    #[test]
    fn cancelled_scan_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "a.txt", "");
        let cancel = CancelToken::new();
        cancel.cancel();
        let result = scan(
            dir.path(),
            &ScanOptions::default(),
            &ScanProgress::default(),
            &cancel,
        );
        assert!(matches!(result, Err(ScanError::Cancelled(_))));
    }

    #[test]
    fn root_must_be_an_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let file = touch(dir.path(), "f.txt", "");
        let opts = ScanOptions::default();
        let progress = ScanProgress::default();
        let cancel = CancelToken::new();
        assert!(matches!(
            scan(&file, &opts, &progress, &cancel),
            Err(ScanError::NotADirectory(_))
        ));
        let missing = dir.path().join("missing");
        assert!(matches!(
            scan(&missing, &opts, &progress, &cancel),
            Err(ScanError::Root { .. })
        ));
    }

    #[test]
    fn invalid_filter_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let opts = ScanOptions {
            filters: Filters {
                exclude: vec!["a[".into()],
                ..Filters::default()
            },
            threads: 1,
        };
        let result = scan(
            dir.path(),
            &opts,
            &ScanProgress::default(),
            &CancelToken::new(),
        );
        assert!(matches!(result, Err(ScanError::Filter(_))));
    }

    #[test]
    fn shallow_scan_lists_only_top_level() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "a.txt", "");
        touch(dir.path(), "sub/deep.txt", "");
        let entries = scan_shallow(dir.path(), &Filters::default()).unwrap();
        assert_eq!(rels(&entries), vec!["a.txt", "sub"]);
        assert_eq!(entries[1].kind, EntryKind::Dir);
    }

    #[cfg(windows)]
    #[test]
    fn symlinked_folders_are_listed_but_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "real/inner.txt", "");
        let (real, link) = (dir.path().join("real"), dir.path().join("link"));
        if std::os::windows::fs::symlink_dir(real, link).is_err() {
            eprintln!("skipped: creating symlinks needs Developer Mode or admin");
            return;
        }
        let entries = run(dir.path(), Filters::default());
        let link = entries.iter().find(|e| e.rel == "link").unwrap();
        assert_eq!(link.kind, EntryKind::Symlink);
        assert!(!entries.iter().any(|e| e.rel == "link/inner.txt"));
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
    fn junction_pairs_compare_by_target_not_by_link_time() {
        use crate::folder::tree::{Status, build_tree};
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "t1/x.txt", "one");
        touch(dir.path(), "t2/x.txt", "two");
        let (left, right) = (dir.path().join("left"), dir.path().join("right"));
        fs::create_dir_all(&left).unwrap();
        fs::create_dir_all(&right).unwrap();
        // Created within 2 s of each other, so link size (0) and link time always match.
        junction(&left.join("diff"), &dir.path().join("t1"));
        junction(&right.join("diff"), &dir.path().join("t2"));
        junction(&left.join("same"), &dir.path().join("t1"));
        junction(&right.join("same"), &dir.path().join("t1"));
        let tree = build_tree(
            run(&left, Filters::default()),
            run(&right, Filters::default()),
            true,
        );
        let status = |rel: &str| {
            tree.nodes
                .iter()
                .find(|n| n.rel == rel)
                .unwrap()
                .status
                .clone()
        };
        assert_eq!(status("diff"), Status::Different { newer: None });
        assert_eq!(status("same"), Status::Same);
    }

    #[test]
    fn walker_error_on_the_root_itself_fails_the_scan() {
        let (tx, rx) = mpsc::channel();
        tx.send(Item::Error {
            rel: String::new(),
            message: "Access is denied. (os error 5)".into(),
        })
        .unwrap();
        drop(tx);
        let root = Path::new("C:/denied");
        assert!(matches!(merge(rx, root), Err(ScanError::Root { .. })));
    }

    #[cfg(windows)]
    #[test]
    fn root_that_cannot_be_listed_is_an_error_not_an_empty_folder() {
        use std::process::Command;
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "a.txt", "x");
        let user = std::env::var("USERNAME").unwrap();
        let path = dir.path().to_str().unwrap().to_owned();
        let icacls = |args: &[&str]| {
            Command::new("icacls")
                .arg(&path)
                .args(args)
                .output()
                .unwrap()
        };
        assert!(icacls(&["/deny", &format!("{user}:(RD)")]).status.success());
        let result = scan(
            dir.path(),
            &ScanOptions::default(),
            &ScanProgress::default(),
            &CancelToken::new(),
        );
        icacls(&["/remove:d", &user]);
        assert!(matches!(result, Err(ScanError::Root { .. })), "{result:?}");
    }

    #[test]
    fn bad_patterns_are_reported_before_a_scan() {
        let ok = Filters {
            include: vec!["*.rs".into()],
            exclude: vec!["target".into()],
        };
        assert!(validate_filters(&ok).is_ok());
        let bad = Filters {
            include: vec!["src/[".into()],
            exclude: vec![],
        };
        assert!(matches!(validate_filters(&bad), Err(ScanError::Filter(_))));
    }

    #[test]
    fn a_rooted_exclude_matches_only_the_top_level_path() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "d/f.txt", "");
        touch(dir.path(), "sub/d/f.txt", "");
        touch(dir.path(), "sub/x/d.txt", "");
        let with = |pattern: &str| {
            let filters = Filters {
                include: Vec::new(),
                exclude: vec![pattern.into()],
            };
            assert!(validate_filters(&filters).is_ok());
            let mut seen: Vec<String> = run(dir.path(), filters)
                .into_iter()
                .map(|e| e.rel)
                .collect();
            seen.sort();
            seen
        };
        let rooted = with("/d");
        assert!(
            !rooted.iter().any(|r| r == "d" || r.starts_with("d/")),
            "{rooted:?}"
        );
        assert!(rooted.iter().any(|r| r == "sub/d"), "{rooted:?}");
        assert!(rooted.iter().any(|r| r == "sub/x/d.txt"), "{rooted:?}");
        let plain = with("d");
        assert!(!plain.iter().any(|r| r == "d" || r == "sub/d"), "{plain:?}");
        let shallow = scan_shallow(
            dir.path(),
            &Filters {
                include: Vec::new(),
                exclude: vec!["/sub".into()],
            },
        )
        .unwrap();
        assert_eq!(rels(&shallow), vec!["d"]);
    }
}
