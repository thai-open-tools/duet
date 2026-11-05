use super::*;
use crate::folder::entry::{Entry, Filters};
use crate::folder::run::{CompareMode, FolderOptions, FolderProgress, compare_folders};
use crate::folder::tree::build_tree;
use std::fs;
use std::path::Path;

fn put(root: &Path, rel: &str, body: &[u8]) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn options() -> FolderOptions {
    FolderOptions {
        mode: CompareMode::Content,
        find_moves: true,
        ..FolderOptions::default()
    }
}

fn compare(l: &Path, r: &Path, opts: &FolderOptions) -> (FolderComparison, FolderProgress) {
    let progress = FolderProgress::default();
    let cmp = compare_folders(l, r, opts, &progress, &CancelToken::new()).unwrap();
    (cmp, progress)
}

fn index(cmp: &FolderComparison, rel: &str) -> u32 {
    cmp.nodes.iter().position(|n| n.rel == rel).unwrap() as u32
}

fn dirs() -> (tempfile::TempDir, tempfile::TempDir) {
    (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap())
}

fn file(rel: &str, len: u64) -> Entry {
    Entry {
        rel: rel.to_owned(),
        kind: EntryKind::File,
        len,
        modified: None,
        error: None,
        link_target: None,
    }
}

#[test]
fn a_renamed_file_is_tagged_on_both_rows() {
    let (l, r) = dirs();
    put(l.path(), "a.txt", b"same body");
    put(r.path(), "b.txt", b"same body");
    let (cmp, _) = compare(l.path(), r.path(), &options());
    let (a, b) = (index(&cmp, "a.txt"), index(&cmp, "b.txt"));
    assert_eq!(cmp.moved_partner(a), Some(b));
    assert_eq!(cmp.moved_partner(b), Some(a));
    assert_eq!(cmp.moved_files(), 1);
    assert_eq!(cmp.nodes[a as usize].status, Status::LeftOnly);
    assert_eq!(cmp.nodes[b as usize].status, Status::RightOnly);
}

#[test]
fn a_file_moved_to_another_folder_is_tagged() {
    let (l, r) = dirs();
    put(l.path(), "old/x.bin", b"\x01\x02\x03");
    put(r.path(), "new/deep/x.bin", b"\x01\x02\x03");
    let (cmp, _) = compare(l.path(), r.path(), &options());
    assert_eq!(cmp.moved_files(), 1);
    assert_eq!(
        cmp.moved_partner(index(&cmp, "old/x.bin")),
        Some(index(&cmp, "new/deep/x.bin"))
    );
}

#[test]
fn a_renamed_folder_tags_each_file_and_not_the_folder() {
    let (l, r) = dirs();
    for name in ["1", "2", "3"] {
        let body = format!("body {name}");
        put(l.path(), &format!("d/{name}.txt"), body.as_bytes());
        put(r.path(), &format!("e/{name}.txt"), body.as_bytes());
    }
    let (cmp, _) = compare(l.path(), r.path(), &options());
    assert_eq!(cmp.moved_files(), 3);
    assert_eq!(cmp.moved_partner(index(&cmp, "d")), None);
    assert_eq!(cmp.moved_partner(index(&cmp, "e")), None);
}

#[test]
fn same_size_different_bytes_is_not_tagged() {
    let (l, r) = dirs();
    put(l.path(), "a.txt", b"abcd");
    put(r.path(), "b.txt", b"abce");
    let (cmp, _) = compare(l.path(), r.path(), &options());
    assert!(cmp.moved.is_empty());
}

#[test]
fn empty_files_never_match() {
    let (l, r) = dirs();
    put(l.path(), "a.txt", b"");
    put(r.path(), "b.txt", b"");
    let (cmp, progress) = compare(l.path(), r.path(), &options());
    assert!(cmp.moved.is_empty());
    assert_eq!(progress.moves.total.load(Ordering::Relaxed), 0);
}

#[test]
fn two_equal_copies_on_one_side_tag_nothing() {
    let (l, r) = dirs();
    put(l.path(), "a.txt", b"twin");
    put(l.path(), "b.txt", b"twin");
    put(r.path(), "c.txt", b"twin");
    let (cmp, _) = compare(l.path(), r.path(), &options());
    assert!(cmp.moved.is_empty());
}

#[test]
fn an_equal_pair_among_same_size_decoys_is_tagged() {
    let (l, r) = dirs();
    put(l.path(), "a.txt", b"aaaa");
    put(l.path(), "b.txt", b"bbbb");
    put(l.path(), "c.txt", b"cccc");
    put(r.path(), "z.txt", b"bbbb");
    let (cmp, progress) = compare(l.path(), r.path(), &options());
    assert_eq!(cmp.moved_files(), 1);
    assert_eq!(
        cmp.moved_partner(index(&cmp, "b.txt")),
        Some(index(&cmp, "z.txt"))
    );
    // Four files hashed, then the one candidate pair compared.
    assert_eq!(progress.moves.total.load(Ordering::Relaxed), 5);
    assert_eq!(progress.moves.done.load(Ordering::Relaxed), 5);
}

#[test]
fn a_size_with_one_left_and_one_right_is_compared_not_hashed() {
    let cmp = build_tree(vec![file("a", 7)], vec![file("b", 7)], true);
    let reads = plan_reads(&cmp);
    assert_eq!(reads.direct.len(), 1);
    assert!(reads.hashed.is_empty());
}

#[test]
fn ten_thousand_one_sided_files_of_one_size_plan_no_reads() {
    let left: Vec<Entry> = (0..10_000).map(|i| file(&format!("f{i}"), 5)).collect();
    let reads = plan_reads(&build_tree(left, Vec::new(), true));
    assert!(reads.direct.is_empty() && reads.hashed.is_empty());
}

#[test]
fn ten_thousand_per_side_of_one_size_plan_one_hash_each_and_no_pairwise_compare() {
    let left: Vec<Entry> = (0..10_000).map(|i| file(&format!("l{i}"), 5)).collect();
    let right: Vec<Entry> = (0..10_000).map(|i| file(&format!("r{i}"), 5)).collect();
    let reads = plan_reads(&build_tree(left, right, true));
    assert!(reads.direct.is_empty());
    assert_eq!(reads.hashed.len(), 1);
    assert_eq!(reads.hashed[0].len(), 20_000);
}

#[test]
fn case_only_names_are_not_tagged_when_case_is_sensitive() {
    let (l, r) = dirs();
    put(l.path(), "Config.xml", b"<a/>");
    put(r.path(), "config.xml", b"<a/>");
    let opts = FolderOptions {
        case_insensitive: false,
        ..options()
    };
    let (cmp, _) = compare(l.path(), r.path(), &opts);
    assert_eq!(cmp.nodes.len(), 2);
    assert!(cmp.moved.is_empty());
}

#[cfg(windows)]
#[test]
fn an_unreadable_candidate_is_skipped() {
    use std::os::windows::fs::OpenOptionsExt;
    let (l, r) = dirs();
    put(l.path(), "a.txt", b"locked body");
    put(r.path(), "b.txt", b"locked body");
    let _lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(r.path().join("b.txt"))
        .unwrap();
    let progress = FolderProgress::default();
    let cmp = compare_folders(
        l.path(),
        r.path(),
        &options(),
        &progress,
        &CancelToken::new(),
    );
    assert!(cmp.unwrap().moved.is_empty());
}

#[cfg(windows)]
#[test]
fn a_one_sided_link_is_never_read_or_tagged() {
    let (l, r) = dirs();
    put(l.path(), "real.txt", b"linked body");
    put(r.path(), "real.txt", b"linked body");
    // The twin makes this bite: a left link beside a right-only file of the same bytes.
    put(r.path(), "twin.txt", b"linked body");
    if std::os::windows::fs::symlink_file(r.path().join("real.txt"), l.path().join("link.txt"))
        .is_err()
    {
        return;
    }
    let (cmp, _) = compare(l.path(), r.path(), &options());
    let (link, twin) = (index(&cmp, "link.txt"), index(&cmp, "twin.txt"));
    assert_eq!(cmp.nodes[link as usize].status, Status::LeftOnly);
    assert_eq!(cmp.nodes[twin as usize].status, Status::RightOnly);
    assert!(cmp.moved.is_empty());
}

#[cfg(windows)]
#[test]
fn a_one_sided_junction_is_not_followed_into_a_twin() {
    let (l, r) = dirs();
    let target = tempfile::tempdir().unwrap();
    put(target.path(), "inner.txt", b"junction body");
    put(r.path(), "twin.txt", b"junction body");
    let made = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(l.path().join("jd"))
        .arg(target.path())
        .output();
    if !made.is_ok_and(|out| out.status.success()) {
        return;
    }
    let (cmp, _) = compare(l.path(), r.path(), &options());
    assert!(cmp.nodes.iter().any(|n| n.rel == "jd"));
    assert!(cmp.nodes.iter().all(|n| n.rel != "jd/inner.txt"));
    assert!(cmp.moved.is_empty());
}

#[test]
fn files_below_an_excluded_folder_are_not_candidates() {
    let (l, r) = dirs();
    put(l.path(), "a.txt", b"hidden twin");
    put(r.path(), "skip/b.txt", b"hidden twin");
    let opts = FolderOptions {
        filters: Filters {
            exclude: vec!["skip".to_owned()],
            ..Filters::default()
        },
        ..options()
    };
    let (cmp, _) = compare(l.path(), r.path(), &opts);
    assert!(cmp.moved.is_empty());
}

#[test]
fn a_cancelled_pass_keeps_the_comparison_and_tags_nothing() {
    let mut cmp = build_tree(vec![file("a", 3)], vec![file("b", 3)], true);
    let cancel = CancelToken::new();
    cancel.cancel();
    let result = find_moves(
        &mut cmp,
        Path::new("."),
        Path::new("."),
        2,
        &MoveProgress::default(),
        &cancel,
    );
    assert!(result.is_ok());
    assert!(cmp.moved.is_empty());
    assert_eq!(cmp.nodes.len(), 2);
}

/// Runs the pass on one thread over files that are not on disk (every open fails fast), and
/// cancels from a watcher as soon as the first one is counted.
fn cancel_after_first(left: Vec<Entry>, right: Vec<Entry>) -> (MoveProgress, FolderComparison) {
    let mut cmp = build_tree(left, right, true);
    let nowhere = tempfile::tempdir().unwrap();
    let progress = MoveProgress::default();
    let cancel = CancelToken::new();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            while progress.done.load(Ordering::Relaxed) == 0 {
                std::thread::yield_now();
            }
            cancel.cancel();
        });
        let result = find_moves(
            &mut cmp,
            nowhere.path(),
            nowhere.path(),
            1,
            &progress,
            &cancel,
        );
        assert!(result.is_ok());
    });
    (progress, cmp)
}

fn assert_stopped_early(progress: &MoveProgress, cmp: &FolderComparison) {
    let (done, total) = (
        progress.done.load(Ordering::Relaxed),
        progress.total.load(Ordering::Relaxed),
    );
    assert!(
        done < total,
        "{done} of {total} files were opened, the cancel stopped nothing"
    );
    assert!(cmp.moved.is_empty());
}

#[test]
fn a_cancel_mid_pass_stops_the_direct_compares() {
    let left: Vec<Entry> = (0..20_000)
        .map(|i| file(&format!("l{i}"), 10 + i))
        .collect();
    let right: Vec<Entry> = (0..20_000)
        .map(|i| file(&format!("r{i}"), 10 + i))
        .collect();
    let (progress, cmp) = cancel_after_first(left, right);
    assert_stopped_early(&progress, &cmp);
}

#[test]
fn a_cancel_mid_pass_stops_the_hashing() {
    let mut left = Vec::new();
    let mut right = Vec::new();
    for i in 0..6_000u64 {
        left.push(file(&format!("l{i}a"), 10 + i));
        left.push(file(&format!("l{i}b"), 10 + i));
        right.push(file(&format!("r{i}"), 10 + i));
    }
    let (progress, cmp) = cancel_after_first(left, right);
    assert_stopped_early(&progress, &cmp);
}

#[test]
fn an_unreadable_copy_in_a_hashed_group_drops_the_whole_group() {
    // a is not on disk; b and c are. b and c look like a pair, but a may be a copy of both.
    let (l, r) = dirs();
    put(l.path(), "b.bin", b"same bytes");
    put(r.path(), "c.bin", b"same bytes");
    let mut cmp = build_tree(
        vec![file("a.bin", 10), file("b.bin", 10)],
        vec![file("c.bin", 10)],
        true,
    );
    find_moves(
        &mut cmp,
        l.path(),
        r.path(),
        2,
        &MoveProgress::default(),
        &CancelToken::new(),
    )
    .unwrap();
    assert!(cmp.moved.is_empty(), "{:?}", cmp.moved);
}

#[test]
fn a_candidate_below_an_unreadable_folder_is_not_tagged() {
    let (l, r) = dirs();
    put(l.path(), "d/x.txt", b"maybe still there");
    put(r.path(), "y.txt", b"maybe still there");
    let dir = |error: Option<&str>| Entry {
        kind: EntryKind::Dir,
        error: error.map(str::to_owned),
        ..file("d", 0)
    };
    let build = |error| {
        build_tree(
            vec![dir(None), file("d/x.txt", 17)],
            vec![dir(error), file("y.txt", 17)],
            true,
        )
    };
    let run = |mut cmp: FolderComparison| {
        find_moves(
            &mut cmp,
            l.path(),
            r.path(),
            2,
            &MoveProgress::default(),
            &CancelToken::new(),
        )
        .unwrap();
        cmp.moved.len()
    };
    assert_eq!(run(build(None)), 2, "readable: the pair is tagged");
    assert_eq!(run(build(Some("Access is denied."))), 0);
}

#[test]
fn nested_roots_tag_nothing() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), "y/a.txt", b"one physical file");
    let (cmp, progress) = compare(dir.path(), &dir.path().join("y"), &options());
    assert!(cmp.moved.is_empty());
    assert_eq!(progress.moves.total.load(Ordering::Relaxed), 0);
}

#[test]
fn the_cloud_attribute_bits_are_offline_and_both_recalls() {
    assert!(is_cloud_placeholder(0x1000));
    assert!(is_cloud_placeholder(0x4_0000));
    assert!(is_cloud_placeholder(0x40_0000));
    assert!(is_cloud_placeholder(0x20 | 0x40_0000));
    assert!(!is_cloud_placeholder(0));
    assert!(!is_cloud_placeholder(0x20 | 0x2000 | 0x400 | 0x1));
}

#[test]
fn a_placeholder_drops_its_size_group_and_nothing_else() {
    let cmp = build_tree(
        vec![file("a", 7), file("p", 9)],
        vec![file("b", 7), file("q", 9)],
        true,
    );
    let p = cmp.nodes.iter().position(|n| n.rel == "p").unwrap() as u32;
    let reads = plan_reads_with(&cmp, |index| index == p);
    assert_eq!(reads.direct.len(), 1, "only the size-7 pair is left");
    assert!(reads.hashed.is_empty());
    assert_eq!(plan_reads(&cmp).direct.len(), 2);
}

#[test]
fn swap_sides_keeps_the_moves() {
    let (l, r) = dirs();
    put(l.path(), "a.txt", b"swap me");
    put(r.path(), "b.txt", b"swap me");
    let (mut cmp, _) = compare(l.path(), r.path(), &options());
    let before = cmp.moved.clone();
    cmp.swap_sides();
    assert_eq!(cmp.moved, before);
    for (a, b) in &cmp.moved {
        assert_eq!(cmp.moved.get(b), Some(a));
    }
}
