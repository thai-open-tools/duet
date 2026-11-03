//! Copy and Recycle Bin delete for folder-compare rows. Never deletes permanently.

use std::fs::{self, File, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    #[error("{0} already exists")]
    WouldOverwrite(PathBuf),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot move {path} to the Recycle Bin: {message}")]
    Trash { path: PathBuf, message: String },
    #[error("{0} is a link or junction; duet never writes through links")]
    DestinationIsLink(PathBuf),
    #[error("cannot copy {src} into itself ({dst})")]
    IntoItself { src: PathBuf, dst: PathBuf },
    /// The path is no longer what the compare saw, so a sync leaves it alone.
    #[error("Changed since the compare.")]
    Changed(PathBuf),
}

/// A destination file a copy would overwrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub path: PathBuf,
    /// The destination copy was modified later than the source (beyond the 2 s tolerance).
    pub destination_newer: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CopyReport {
    pub files: u64,
    pub skipped_symlinks: u64,
}

fn io_at(path: &Path) -> impl Fn(io::Error) -> ActionError + '_ {
    move |source| ActionError::Io {
        path: path.to_path_buf(),
        source,
    }
}

pub fn copy_entry(src: &Path, dst: &Path, overwrite: bool) -> Result<CopyReport, ActionError> {
    let meta = fs::symlink_metadata(src).map_err(io_at(src))?;
    refuse_link(dst)?;
    refuse_nested(src, dst)?;
    if !overwrite && let Some(conflict) = copy_conflicts(src, dst)?.into_iter().next() {
        return Err(ActionError::WouldOverwrite(conflict.path));
    }
    let mut report = CopyReport::default();
    if meta.file_type().is_symlink() {
        report.skipped_symlinks += 1;
    } else if meta.is_dir() {
        copy_dir(src, dst, overwrite, &mut report)?;
    } else {
        copy_file(src, dst, overwrite)?;
        report.files += 1;
    }
    Ok(report)
}

/// Checks a copy into the compared tree rooted at `dst_root` before anything is written: no link
/// or junction on the way, no copy into its own subtree, and every file it would overwrite.
pub fn preflight_copy(
    dst_root: &Path,
    src: &Path,
    dst: &Path,
) -> Result<Vec<Conflict>, ActionError> {
    ensure_no_links_below(dst_root, dst)?;
    refuse_nested(src, dst)?;
    copy_conflicts(src, dst)
}

/// [`copy_entry`] for a destination inside the compared tree rooted at `dst_root`.
pub fn copy_into(
    dst_root: &Path,
    src: &Path,
    dst: &Path,
    overwrite: bool,
) -> Result<CopyReport, ActionError> {
    ensure_no_links_below(dst_root, dst)?;
    copy_entry(src, dst, overwrite)
}

/// The path as the file system resolves it, so two spellings of one folder compare equal.
/// A tail that does not exist yet is appended as written.
fn resolved(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut tail = Vec::new();
    loop {
        if let Ok(mut real) = fs::canonicalize(existing) {
            real.extend(tail.iter().rev());
            return real;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name);
                existing = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// The two roots are the same folder or one is inside the other, comparing resolved paths.
pub fn roots_overlap(left: &Path, right: &Path) -> bool {
    let (a, b) = (comparable(left), comparable(right));
    a.starts_with(&b) || b.starts_with(&a)
}

/// [`resolved`], case-folded where the file system folds case.
fn comparable(path: &Path) -> PathBuf {
    let real = resolved(path);
    if cfg!(windows) {
        PathBuf::from(real.to_string_lossy().to_lowercase())
    } else {
        real
    }
}

/// A folder copied into its own subtree (one compare root nested in the other) would descend
/// into its own output until the disk fills.
pub(crate) fn refuse_nested(src: &Path, dst: &Path) -> Result<(), ActionError> {
    if resolved(dst).starts_with(resolved(src)) {
        Err(ActionError::IntoItself {
            src: src.to_path_buf(),
            dst: dst.to_path_buf(),
        })
    } else {
        Ok(())
    }
}

fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// Scans never follow links, so copies must not write through them either: a link or junction
/// on the destination side would land the write outside the compared tree.
fn refuse_link(path: &Path) -> Result<(), ActionError> {
    if is_link(path) {
        Err(ActionError::DestinationIsLink(path.to_path_buf()))
    } else {
        Ok(())
    }
}

/// Refuses when any path between `root` (exclusive) and `path` (inclusive) is a link or
/// junction. The root itself may be reached through a link: the user chose it.
pub fn ensure_no_links_below(root: &Path, path: &Path) -> Result<(), ActionError> {
    let Ok(rel) = path.strip_prefix(root) else {
        return Ok(());
    };
    let mut at = root.to_path_buf();
    for part in rel.components() {
        at.push(part);
        refuse_link(&at)?;
    }
    Ok(())
}

fn destination_newer(destination: &Metadata, source: &Metadata) -> bool {
    match (destination.modified(), source.modified()) {
        (Ok(d), Ok(s)) => d > s + Duration::from_secs(2),
        _ => false,
    }
}

/// Every destination file a copy of `src` to `dst` would overwrite; existing folders merge and
/// are not conflicts. Errors if a destination path on the way is a link or junction.
pub fn copy_conflicts(src: &Path, dst: &Path) -> Result<Vec<Conflict>, ActionError> {
    let meta = fs::symlink_metadata(src).map_err(io_at(src))?;
    refuse_link(dst)?;
    let mut conflicts = Vec::new();
    if meta.file_type().is_symlink() {
        return Ok(conflicts);
    }
    if !meta.is_dir() {
        if let Ok(existing) = fs::symlink_metadata(dst) {
            conflicts.push(Conflict {
                path: dst.to_path_buf(),
                destination_newer: destination_newer(&existing, &meta),
            });
        }
        return Ok(conflicts);
    }
    // Explicit work stack: deep trees (thousands of levels) must not overflow the stack.
    let mut pending = vec![(src.to_path_buf(), dst.to_path_buf())];
    while let Some((src, dst)) = pending.pop() {
        if fs::symlink_metadata(&dst).is_err() {
            continue;
        }
        refuse_link(&dst)?;
        for item in fs::read_dir(&src).map_err(io_at(&src))? {
            let item = item.map_err(io_at(&src))?;
            let from = item.path();
            let to = dst.join(item.file_name());
            let child = fs::symlink_metadata(&from).map_err(io_at(&from))?;
            if child.file_type().is_symlink() {
                continue;
            }
            refuse_link(&to)?;
            if child.is_dir() {
                pending.push((from, to));
            } else if let Ok(existing) = fs::symlink_metadata(&to) {
                conflicts.push(Conflict {
                    path: to,
                    destination_newer: destination_newer(&existing, &child),
                });
            }
        }
    }
    Ok(conflicts)
}

/// `overwrite` is the caller's confirmation that `dst` may be replaced (R18).
fn copy_file(src: &Path, dst: &Path, overwrite: bool) -> Result<(), ActionError> {
    refuse_link(dst)?;
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(io_at(parent))?;
    }
    if overwrite {
        make_writable(dst)?;
    }
    fs::copy(src, dst).map_err(io_at(dst))?;
    let modified = fs::metadata(src)
        .and_then(|m| m.modified())
        .map_err(io_at(src))?;
    open_for_times(dst)
        .and_then(|f| f.set_modified(modified))
        .map_err(io_at(dst))?;
    Ok(())
}

/// R18: a read-only file a confirmed overwrite replaces loses the attribute first, as LocalDiff
/// chmods it (`fs:316`). Anything else at `path` is left for the copy itself to report.
fn make_writable(path: &Path) -> Result<(), ActionError> {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if !meta.is_file() || !meta.permissions().readonly() {
        return Ok(());
    }
    let mut perms = meta.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(perms.mode() | 0o200);
    }
    #[cfg(not(unix))]
    {
        // Clears the Windows read-only attribute and nothing else.
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
    }
    fs::set_permissions(path, perms).map_err(io_at(path))
}

/// Opens a file only to change its times. On Windows this asks for FILE_WRITE_ATTRIBUTES alone,
/// which succeeds on read-only files (the copy keeps the source's read-only attribute).
fn open_for_times(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
        File::options()
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .open(path)
    }
    #[cfg(not(windows))]
    {
        File::options().write(true).open(path)
    }
}

/// Gives `dst` the modified time of `src`. Works on read-only files; refuses a link or
/// junction as `dst`, whose target may lie outside the compared tree.
pub fn touch_like(src: &Path, dst: &Path) -> Result<(), ActionError> {
    refuse_link(dst)?;
    let modified = fs::metadata(src)
        .and_then(|m| m.modified())
        .map_err(io_at(src))?;
    open_for_times(dst)
        .and_then(|f| f.set_modified(modified))
        .map_err(io_at(dst))
}

// Explicit work stacks below: deep trees (thousands of levels) must not overflow the stack.
fn copy_dir(
    src: &Path,
    dst: &Path,
    overwrite: bool,
    report: &mut CopyReport,
) -> Result<(), ActionError> {
    let mut pending = vec![(src.to_path_buf(), dst.to_path_buf())];
    while let Some((src, dst)) = pending.pop() {
        refuse_link(&dst)?;
        fs::create_dir_all(&dst).map_err(io_at(&dst))?;
        for item in fs::read_dir(&src).map_err(io_at(&src))? {
            let item = item.map_err(io_at(&src))?;
            let from = item.path();
            let to = dst.join(item.file_name());
            let file_type = item.file_type().map_err(io_at(&from))?;
            if file_type.is_symlink() {
                report.skipped_symlinks += 1;
            } else if file_type.is_dir() {
                pending.push((from, to));
            } else {
                copy_file(&from, &to, overwrite)?;
                report.files += 1;
            }
        }
    }
    Ok(())
}

pub fn move_to_trash(path: &Path) -> Result<(), ActionError> {
    trash::delete(path).map_err(|e| ActionError::Trash {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn put(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn mtime(path: &Path) -> SystemTime {
        fs::metadata(path).unwrap().modified().unwrap()
    }

    #[test]
    fn copies_a_file_into_missing_folders_and_keeps_its_time() {
        let dir = tempfile::tempdir().unwrap();
        let (src, dst) = (
            dir.path().join("src.txt"),
            dir.path().join("new/deep/dst.txt"),
        );
        put(&src, "ข้อมูล");
        let when = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        File::options()
            .write(true)
            .open(&src)
            .unwrap()
            .set_modified(when)
            .unwrap();
        let report = copy_entry(&src, &dst, false).unwrap();
        assert_eq!(
            report,
            CopyReport {
                files: 1,
                skipped_symlinks: 0
            }
        );
        assert_eq!(fs::read_to_string(&dst).unwrap(), "ข้อมูล");
        assert_eq!(mtime(&dst), when);
    }

    #[test]
    fn refuses_to_overwrite_without_permission() {
        let dir = tempfile::tempdir().unwrap();
        let (src, dst) = (dir.path().join("a"), dir.path().join("b"));
        put(&src, "new");
        put(&dst, "old");
        assert!(matches!(
            copy_entry(&src, &dst, false),
            Err(ActionError::WouldOverwrite(p)) if p == dst
        ));
        assert_eq!(fs::read_to_string(&dst).unwrap(), "old");
        copy_entry(&src, &dst, true).unwrap();
        assert_eq!(fs::read_to_string(&dst).unwrap(), "new");
    }

    #[test]
    fn copies_folders_recursively() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        put(&src.join("a.txt"), "1");
        put(&src.join("โฟลเดอร์/b.txt"), "2");
        put(&src.join("โฟลเดอร์/ลึก/c.txt"), "3");
        let dst = dir.path().join("dst");
        assert_eq!(copy_entry(&src, &dst, false).unwrap().files, 3);
        assert_eq!(
            fs::read_to_string(dst.join("โฟลเดอร์/ลึก/c.txt")).unwrap(),
            "3"
        );
    }

    #[test]
    fn folder_conflict_is_detected_before_any_write() {
        let dir = tempfile::tempdir().unwrap();
        let (src, dst) = (dir.path().join("src"), dir.path().join("dst"));
        put(&src.join("a.txt"), "new-a");
        put(&src.join("z.txt"), "new-z");
        put(&dst.join("z.txt"), "old-z");
        assert!(matches!(
            copy_entry(&src, &dst, false),
            Err(ActionError::WouldOverwrite(_))
        ));
        assert!(
            !dst.join("a.txt").exists(),
            "nothing may be copied when a conflict exists"
        );
    }

    #[test]
    fn missing_source_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = copy_entry(&dir.path().join("nope"), &dir.path().join("x"), false);
        assert!(matches!(result, Err(ActionError::Io { .. })));
    }

    fn set_readonly(path: &Path, readonly: bool) {
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_readonly(readonly);
        fs::set_permissions(path, perms).unwrap();
    }

    #[test]
    fn copies_a_read_only_file_and_keeps_its_time() {
        let dir = tempfile::tempdir().unwrap();
        let (src, dst) = (dir.path().join("ro.txt"), dir.path().join("out/ro.txt"));
        put(&src, "locked");
        let when = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        File::options()
            .write(true)
            .open(&src)
            .unwrap()
            .set_modified(when)
            .unwrap();
        set_readonly(&src, true);
        let result = copy_entry(&src, &dst, false);
        let copied_readonly = dst.exists() && fs::metadata(&dst).unwrap().permissions().readonly();
        let copied_time = dst.exists().then(|| mtime(&dst));
        set_readonly(&src, false);
        if dst.exists() {
            set_readonly(&dst, false);
        }
        assert_eq!(result.unwrap().files, 1);
        assert_eq!(copied_time, Some(when));
        assert!(copied_readonly, "the copy keeps the read-only attribute");
    }

    #[test]
    fn a_confirmed_overwrite_replaces_a_read_only_file() {
        let dir = tempfile::tempdir().unwrap();
        let (src, dst) = (dir.path().join("new.txt"), dir.path().join("old.txt"));
        put(&src, "new");
        put(&dst, "old");
        set_readonly(&dst, true);
        let refused = copy_entry(&src, &dst, false);
        let kept = fs::read_to_string(&dst).unwrap();
        let replaced = copy_entry(&src, &dst, true);
        let after = fs::read_to_string(&dst).unwrap();
        set_readonly(&dst, false);
        assert!(
            matches!(refused, Err(ActionError::WouldOverwrite(ref p)) if *p == dst),
            "{refused:?}"
        );
        assert_eq!(kept, "old", "an unconfirmed copy leaves the file alone");
        assert_eq!(replaced.unwrap().files, 1);
        assert_eq!(after, "new");
    }

    #[test]
    fn roots_overlap_finds_nested_and_equal_roots() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("proj");
        let inner = a.join("b");
        let sibling = dir.path().join("proj2");
        fs::create_dir_all(&inner).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        assert!(roots_overlap(&a, &a));
        assert!(roots_overlap(&a, &inner));
        assert!(roots_overlap(&inner, &a));
        assert!(
            !roots_overlap(&a, &sibling),
            "a shared name prefix is not nesting"
        );
        assert!(!roots_overlap(&sibling, &a));
        assert!(
            roots_overlap(&inner.join(".."), &a),
            "two spellings of one folder"
        );
        #[cfg(windows)]
        assert!(roots_overlap(&a, &dir.path().join("PROJ")));
    }

    #[test]
    fn folder_with_a_read_only_file_copies_completely() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        put(&src.join("a_ro.txt"), "1");
        put(&src.join("b.txt"), "2");
        set_readonly(&src.join("a_ro.txt"), true);
        let dst = dir.path().join("dst");
        let result = copy_entry(&src, &dst, false);
        set_readonly(&src.join("a_ro.txt"), false);
        if dst.join("a_ro.txt").exists() {
            set_readonly(&dst.join("a_ro.txt"), false);
        }
        assert_eq!(result.unwrap().files, 2);
        assert_eq!(fs::read_to_string(dst.join("b.txt")).unwrap(), "2");
    }

    #[test]
    fn very_deep_folders_copy_and_check_conflicts_without_recursion() {
        let dir = tempfile::tempdir().unwrap();
        let (src, dst) = (dir.path().join("s"), dir.path().join("d"));
        let mut leaf = src.clone();
        for _ in 0..1_000 {
            leaf.push("a");
        }
        put(&leaf.join("f.txt"), "deep");
        let (s2, d2) = (src.clone(), dst.clone());
        let result = std::thread::Builder::new()
            .stack_size(128 * 1024)
            .spawn(move || {
                let copied = copy_entry(&s2, &d2, false).map(|r| r.files);
                let conflict = copy_entry(&s2, &d2, false);
                (copied, conflict)
            })
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(result.0.unwrap(), 1);
        assert!(matches!(result.1, Err(ActionError::WouldOverwrite(_))));
        let copied_leaf = dst.join(leaf.strip_prefix(&src).unwrap()).join("f.txt");
        assert_eq!(fs::read_to_string(copied_leaf).unwrap(), "deep");
    }

    #[test]
    fn copy_conflicts_lists_every_overwrite_and_which_are_newer() {
        let dir = tempfile::tempdir().unwrap();
        let (src, dst) = (dir.path().join("src"), dir.path().join("dst"));
        put(&src.join("a.txt"), "new-a");
        put(&src.join("sub/b.txt"), "new-b");
        put(&src.join("c.txt"), "only-left");
        put(&dst.join("a.txt"), "old-a");
        put(&dst.join("sub/b.txt"), "edited-b");
        let old = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let newer = old + Duration::from_secs(3_600);
        for (path, when) in [
            (src.join("a.txt"), newer),
            (dst.join("a.txt"), old),
            (src.join("sub/b.txt"), old),
            (dst.join("sub/b.txt"), newer),
        ] {
            File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(when)
                .unwrap();
        }
        let mut conflicts = copy_conflicts(&src, &dst).unwrap();
        conflicts.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(
            conflicts,
            vec![
                Conflict {
                    path: dst.join("a.txt"),
                    destination_newer: false
                },
                Conflict {
                    path: dst.join("sub").join("b.txt"),
                    destination_newer: true
                },
            ]
        );
        assert!(
            copy_conflicts(&src.join("c.txt"), &dst.join("c.txt"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn copies_into_their_own_subtree_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        // Left root `a`, right root `a/sub`: the left-only row `sub` would copy into itself.
        let a = dir.path().join("a");
        put(&a.join("sub/f.txt"), "f");
        let (src, dst) = (a.join("sub"), a.join("sub").join("sub"));
        assert!(matches!(
            copy_entry(&src, &dst, false),
            Err(ActionError::IntoItself { .. })
        ));
        assert!(matches!(
            preflight_copy(&a.join("sub"), &src, &dst),
            Err(ActionError::IntoItself { .. })
        ));
        assert!(!dst.exists(), "nothing was created");
        // A sibling with a shared name prefix is not nested.
        put(&a.join("subway/g.txt"), "g");
        assert!(
            preflight_copy(&a, &a.join("subway"), &a.join("sub").join("subway"))
                .unwrap()
                .is_empty()
        );
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
    fn copies_never_write_through_a_destination_junction() {
        let dir = tempfile::tempdir().unwrap();
        let (left, right, outside) = (
            dir.path().join("left"),
            dir.path().join("right"),
            dir.path().join("outside"),
        );
        put(&left.join("lib/new.txt"), "new");
        fs::create_dir_all(&right).unwrap();
        fs::create_dir_all(&outside).unwrap();
        junction(&right.join("lib"), &outside);
        // A file whose destination folder is a junction.
        assert!(matches!(
            ensure_no_links_below(&right, &right.join("lib").join("new.txt")),
            Err(ActionError::DestinationIsLink(p)) if p == right.join("lib")
        ));
        // A folder copy onto the junction itself, with and without overwrite.
        assert!(matches!(
            copy_conflicts(&left.join("lib"), &right.join("lib")),
            Err(ActionError::DestinationIsLink(_))
        ));
        assert!(matches!(
            copy_entry(&left.join("lib"), &right.join("lib"), true),
            Err(ActionError::DestinationIsLink(_))
        ));
        assert!(
            !outside.join("new.txt").exists(),
            "nothing may land outside the compared roots"
        );
        // The pre-flight the GUI runs before every copy catches a junction anywhere on the way.
        let (from, to) = (
            left.join("lib").join("new.txt"),
            right.join("lib").join("new.txt"),
        );
        assert!(matches!(
            preflight_copy(&right, &from, &to),
            Err(ActionError::DestinationIsLink(_))
        ));
        assert!(matches!(
            copy_into(&right, &from, &to, true),
            Err(ActionError::DestinationIsLink(_))
        ));
        assert!(
            !outside.join("new.txt").exists(),
            "nothing may land outside the compared roots"
        );
        // The compare root itself may be reached through a link; only links below it are refused.
        assert!(
            ensure_no_links_below(&right.join("lib"), &right.join("lib").join("x.txt")).is_ok()
        );
    }

    #[test]
    #[ignore = "moves a real file into the user's Recycle Bin; run manually once"]
    fn moves_a_file_to_the_recycle_bin() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("duet-trash-test.txt");
        put(&path, "bye");
        move_to_trash(&path).unwrap();
        assert!(!path.exists());
    }
}
