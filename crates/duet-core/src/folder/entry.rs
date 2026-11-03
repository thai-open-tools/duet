//! Scan results and shared folder types.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::time::SystemTime;

use crate::cancel::Cancelled;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Path relative to the scan root, '/'-separated, original case.
    pub rel: String,
    pub kind: EntryKind,
    pub len: u64,
    pub modified: Option<SystemTime>,
    pub error: Option<String>,
    /// Where a symlink or junction points, as stored in the link. Links are never followed.
    pub link_target: Option<PathBuf>,
}

impl Entry {
    pub fn name(&self) -> &str {
        self.rel.rsplit('/').next().unwrap_or(&self.rel)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filters {
    /// Glob patterns a file must match (name or relative path). Empty = all files.
    pub include: Vec<String>,
    /// Glob patterns that remove a file or a whole folder (name or relative path).
    pub exclude: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ScanProgress {
    pub entries: AtomicU64,
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("invalid filter pattern: {0}")]
    Filter(#[from] globset::Error),
    #[error("{path} is not a readable folder: {source}")]
    Root {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{0} is not a folder")]
    NotADirectory(PathBuf),
    #[error(transparent)]
    Cancelled(#[from] Cancelled),
    #[error("cannot start worker threads: {0}")]
    ThreadPool(#[from] rayon::ThreadPoolBuildError),
}

/// Joins a '/'-separated relative path component by component (safe for `\\?\` roots).
pub fn join_rel(root: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .filter(|part| !part.is_empty())
        .fold(root.to_path_buf(), |path, part| path.join(part))
}
