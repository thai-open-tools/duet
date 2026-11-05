//! The fast first pass of a folder compare: top-level entries only, so rows appear within the
//! 200 ms budget while the full compare runs.

use std::path::Path;

use duet_core::folder::entry::ScanError;
use duet_core::folder::run::{CompareMode, FolderOptions};
use duet_core::folder::scan::scan_shallow;
use duet_core::folder::tree::{FolderComparison, Status, apply_size_rule, build_tree_with};

/// Paired folders (and, in the modes that read contents, same-looking files) stay `Pending` until the full
/// compare replaces this result.
pub fn shallow(
    left: &Path,
    right: &Path,
    opts: &FolderOptions,
) -> Result<FolderComparison, ScanError> {
    let (l, r) = (
        scan_shallow(left, &opts.filters)?,
        scan_shallow(right, &opts.filters)?,
    );
    let mut cmp = build_tree_with(l, r, opts.case_insensitive, opts.quick);
    if opts.mode == CompareMode::Size {
        apply_size_rule(&mut cmp);
    }
    for node in &mut cmp.nodes {
        let paired = node.left.is_some() && node.right.is_some();
        let unknown = node.is_dir || opts.mode.reads_contents();
        if paired && unknown && node.status == Status::Same {
            node.status = Status::Pending;
        }
    }
    Ok(cmp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::time::{Duration, UNIX_EPOCH};

    fn put(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
            .unwrap();
    }

    #[test]
    fn shallow_lists_top_level_and_marks_folders_pending() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(&l.path().join("sub/deep.txt"), "x");
        put(&r.path().join("sub/deep.txt"), "y");
        put(&l.path().join("a.txt"), "same");
        put(&r.path().join("a.txt"), "same");
        put(&r.path().join("b.txt"), "new");
        let cmp = shallow(l.path(), r.path(), &FolderOptions::default()).unwrap();
        let rows: Vec<(&str, &Status)> = cmp
            .nodes
            .iter()
            .map(|n| (n.rel.as_str(), &n.status))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("sub", &Status::Pending),
                ("a.txt", &Status::Same),
                ("b.txt", &Status::RightOnly)
            ]
        );
    }

    #[test]
    fn content_mode_leaves_same_looking_files_pending() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(&l.path().join("a.txt"), "1234");
        put(&r.path().join("a.txt"), "abcd");
        let opts = FolderOptions {
            mode: CompareMode::Content,
            ..FolderOptions::default()
        };
        assert_eq!(
            shallow(l.path(), r.path(), &opts).unwrap().nodes[0].status,
            Status::Pending
        );
    }

    #[test]
    fn the_first_pass_marks_rules_mode_files_pending() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(&l.path().join("a.txt"), "1234");
        put(&r.path().join("a.txt"), "abcd");
        let status = |mode| {
            let opts = FolderOptions {
                mode,
                ..FolderOptions::default()
            };
            shallow(l.path(), r.path(), &opts).unwrap().nodes[0]
                .status
                .clone()
        };
        assert_eq!(status(CompareMode::Rules), Status::Pending);
        assert_eq!(status(CompareMode::Content), Status::Pending);
        assert_eq!(status(CompareMode::Size), Status::Same);
        File::options()
            .write(true)
            .open(r.path().join("a.txt"))
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_500))
            .unwrap();
        // Size ignores the times: the same length is Same (Quick calls it different).
        assert_eq!(status(CompareMode::Size), Status::Same);
        assert!(matches!(
            status(CompareMode::Quick),
            Status::Different { .. }
        ));
    }

    #[test]
    fn the_first_pass_finds_no_moves() {
        let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        put(&l.path().join("a.txt"), "same bytes");
        put(&r.path().join("b.txt"), "same bytes");
        let opts = FolderOptions {
            mode: CompareMode::Content,
            find_moves: true,
            ..FolderOptions::default()
        };
        let first = shallow(l.path(), r.path(), &opts).unwrap();
        assert!(first.moved.is_empty());
        let full = duet_core::folder::run::compare_folders(
            l.path(),
            r.path(),
            &opts,
            &duet_core::folder::run::FolderProgress::default(),
            &duet_core::cancel::CancelToken::new(),
        )
        .unwrap();
        assert_eq!(full.moved_files(), 1, "only the full compare looks");
    }
}
