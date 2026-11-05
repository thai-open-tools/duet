use super::*;
use crate::folder::entry::{Entry, EntryKind, Filters};
use crate::folder::run::{FolderOptions, FolderProgress, compare_folders};
use crate::folder::tree::build_tree;
use std::fs::{self, File};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

const T0: u64 = 1_700_000_000;

fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn put_at(root: &Path, rel: &str, body: &str, secs: u64) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, body).unwrap();
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(at(secs))
        .unwrap();
}

fn compare_with(l: &Path, r: &Path, opts: &FolderOptions) -> FolderComparison {
    compare_folders(l, r, opts, &FolderProgress::default(), &CancelToken::new()).unwrap()
}

/// Quick compare, 2 s, DST off (`FolderOptions::default()`).
fn quick(l: &Path, r: &Path) -> FolderComparison {
    compare_with(l, r, &FolderOptions::default())
}

/// The brief's `sync_fixture()`.
fn sync_fixture() -> (TempDir, TempDir, FolderComparison) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(l.path(), "eq.txt", "e", T0);
    put_at(r.path(), "eq.txt", "e", T0);
    put_at(r.path(), "extra.txt", "x", T0);
    put_at(l.path(), "new.txt", "n", T0);
    put_at(l.path(), "newer.txt", "L-newer", T0 + 3600);
    put_at(r.path(), "newer.txt", "R", T0);
    put_at(l.path(), "older.txt", "L", T0);
    put_at(r.path(), "older.txt", "R-older", T0 + 3600);
    put_at(l.path(), "same-time.txt", "L", T0);
    put_at(r.path(), "same-time.txt", "R!", T0);
    let cmp = quick(l.path(), r.path());
    (l, r, cmp)
}

/// Scope "all": every top-level node, everything shown.
fn plan_all(cmp: &FolderComparison, mode: SyncMode, l: &Path, r: &Path) -> SyncPlan {
    let top: Vec<u32> = (0..cmp.nodes.len() as u32)
        .filter(|&n| cmp.nodes[n as usize].parent.is_none())
        .collect();
    sync_plan(cmp, &top, &|_| true, mode, l, r)
}

fn index(cmp: &FolderComparison, rel: &str) -> u32 {
    cmp.nodes.iter().position(|n| n.rel == rel).unwrap() as u32
}

fn dsts(batch: &CopyBatch) -> Vec<PathBuf> {
    batch.files.iter().map(|f| f.dst.clone()).collect()
}

fn skips(plan: &SyncPlan) -> Vec<(String, SkipReason)> {
    plan.skips.iter().map(|s| (s.rel.clone(), s.why)).collect()
}

fn skip(rel: &str, why: SkipReason) -> (String, SkipReason) {
    (rel.to_owned(), why)
}

fn file(rel: &str, len: u64) -> Entry {
    Entry {
        rel: rel.into(),
        kind: EntryKind::File,
        len,
        modified: Some(at(T0)),
        error: None,
        link_target: None,
    }
}

fn dir(rel: &str) -> Entry {
    Entry {
        kind: EntryKind::Dir,
        len: 0,
        ..file(rel, 0)
    }
}

fn no_trash(_: &Path) -> Result<(), ActionError> {
    panic!("the Recycle Bin must not be reached")
}

#[test]
fn update_right_copies_new_and_newer_files_and_skips_the_rest() {
    let (l, r, cmp) = sync_fixture();
    let plan = plan_all(&cmp, SyncMode::UpdateRight, l.path(), r.path());
    assert_eq!(
        plan.to_right.files,
        [
            CopyItem {
                src: l.path().join("new.txt"),
                dst: r.path().join("new.txt"),
                len: 1,
                overwrites: false,
            },
            CopyItem {
                src: l.path().join("newer.txt"),
                dst: r.path().join("newer.txt"),
                len: 7,
                overwrites: true,
            },
        ]
    );
    assert_eq!(
        (
            plan.files(),
            plan.bytes(),
            plan.overwrites(),
            plan.folders()
        ),
        (2, 8, 1, 0)
    );
    assert_eq!(
        skips(&plan),
        [
            skip("older.txt", SkipReason::Newer(Side::Right)),
            skip("same-time.txt", SkipReason::SameTime),
        ]
    );
    assert!(plan.to_left.is_empty());
    assert!(plan.trash.is_empty());
    assert_eq!(plan.trash_side, None);
    assert!(!plan.is_empty());
}

#[test]
fn update_left_and_update_both() {
    let (l, r, cmp) = sync_fixture();
    let left = plan_all(&cmp, SyncMode::UpdateLeft, l.path(), r.path());
    assert_eq!(
        dsts(&left.to_left),
        [l.path().join("extra.txt"), l.path().join("older.txt")]
    );
    assert!(left.to_right.is_empty() && left.trash.is_empty());
    assert_eq!(
        skips(&left),
        [
            skip("newer.txt", SkipReason::Newer(Side::Left)),
            skip("same-time.txt", SkipReason::SameTime),
        ]
    );
    let both = plan_all(&cmp, SyncMode::UpdateBoth, l.path(), r.path());
    assert_eq!(
        dsts(&both.to_right),
        [r.path().join("new.txt"), r.path().join("newer.txt")]
    );
    assert_eq!(
        dsts(&both.to_left),
        [l.path().join("extra.txt"), l.path().join("older.txt")]
    );
    assert_eq!((both.files(), both.bytes(), both.overwrites()), (4, 16, 2));
    assert_eq!(skips(&both), [skip("same-time.txt", SkipReason::SameTime)]);
    assert!(both.trash.is_empty());
    assert_eq!(both.trash_side, None);
}

#[test]
fn mirror_right_copies_everything_left_and_trashes_right_only_items() {
    let (l, r, cmp) = sync_fixture();
    let plan = plan_all(&cmp, SyncMode::MirrorRight, l.path(), r.path());
    assert_eq!(
        dsts(&plan.to_right),
        ["new.txt", "newer.txt", "older.txt", "same-time.txt"].map(|n| r.path().join(n))
    );
    assert_eq!((plan.bytes(), plan.overwrites()), (10, 3));
    assert_eq!(plan.trash, [r.path().join("extra.txt")]);
    assert_eq!(plan.trash_side, Some(Side::Right));
    assert!(plan.to_left.is_empty());
    assert_eq!(skips(&plan), []);
    let back = plan_all(&cmp, SyncMode::MirrorLeft, l.path(), r.path());
    assert_eq!(
        dsts(&back.to_left),
        ["extra.txt", "newer.txt", "older.txt", "same-time.txt"].map(|n| l.path().join(n))
    );
    assert_eq!((back.bytes(), back.overwrites()), (11, 3));
    assert_eq!(back.trash, [l.path().join("new.txt")]);
    assert_eq!(back.trash_side, Some(Side::Left));
    assert!(back.to_right.is_empty());
    assert_eq!(skips(&back), []);
}

#[test]
fn case_only_pairs_are_skipped_and_never_trashed() {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(l.path(), "Config.xml", "LEFT\n", T0);
    put_at(r.path(), "config.xml", "RIGHT\n", T0 + 7200);
    put_at(l.path(), "Src/a.java", "same\n", T0);
    put_at(r.path(), "src/a.java", "same\n", T0);
    let opts = FolderOptions {
        case_insensitive: true,
        ..FolderOptions::default()
    };
    let cmp = compare_with(l.path(), r.path(), &opts);
    let update = plan_all(&cmp, SyncMode::UpdateRight, l.path(), r.path());
    assert!(update.is_empty(), "{update:?}");
    // Node order puts folders first; a node's rel is its left spelling.
    assert_eq!(
        skips(&update),
        [
            skip("Src/a.java", SkipReason::CaseOnly),
            skip("Config.xml", SkipReason::Newer(Side::Right)),
        ]
    );
    let mirror = plan_all(&cmp, SyncMode::MirrorRight, l.path(), r.path());
    assert_eq!(
        dsts(&mirror.to_right),
        [r.path().join("config.xml")],
        "the destination keeps its spelling"
    );
    assert!(mirror.trash.is_empty(), "{:?}", mirror.trash);
    assert_eq!(skips(&mirror), [skip("Src/a.java", SkipReason::CaseOnly)]);
    // Run it: the right keeps one config.xml, with the left's contents, and src/a.java.
    let report = run_sync(
        &mirror,
        l.path(),
        r.path(),
        &no_trash,
        &CancelToken::new(),
        &AtomicUsize::new(0),
    );
    assert!(report.failed().is_empty(), "{:?}", report.failed());
    assert_eq!(
        fs::read_to_string(r.path().join("config.xml")).unwrap(),
        "LEFT\n"
    );
    let names: Vec<String> = fs::read_dir(r.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"config.xml".to_owned()), "{names:?}");
    assert!(r.path().join("src").join("a.java").is_file());
}

#[test]
fn mirror_keeps_folders_whose_files_are_filtered_out() {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(l.path(), "app.properties", "k=v\n", T0);
    put_at(r.path(), "app.properties", "k=v\n", T0);
    put_at(r.path(), "logs/a.log", "audit\n", T0);
    fs::create_dir_all(r.path().join("old/deeper")).unwrap();
    put_at(r.path(), "extra/x.txt", "x\n", T0);
    put_at(r.path(), "extra/sub/y.txt", "y\n", T0);
    let opts = FolderOptions {
        filters: Filters {
            exclude: vec!["*.log".into()],
            ..Filters::default()
        },
        ..FolderOptions::default()
    };
    let cmp = compare_with(l.path(), r.path(), &opts);
    let plan = plan_all(&cmp, SyncMode::MirrorRight, l.path(), r.path());
    assert_eq!(
        plan.trash,
        [r.path().join("extra"), r.path().join("old")],
        "each whole, nested folders once"
    );
    assert_eq!(skips(&plan), [skip("logs/", SkipReason::KeptFolder)]);
    assert!(plan.to_right.is_empty() && plan.to_left.is_empty());
    // Run it with a fake that really removes: two calls, no "not found", the log stays.
    let calls = Mutex::new(Vec::new());
    let remove = |p: &Path| -> Result<(), ActionError> {
        calls.lock().unwrap().push(p.to_path_buf());
        fs::remove_dir_all(p).map_err(|source| ActionError::Io {
            path: p.to_path_buf(),
            source,
        })
    };
    let report = run_sync(
        &plan,
        l.path(),
        r.path(),
        &remove,
        &CancelToken::new(),
        &AtomicUsize::new(0),
    );
    assert!(report.failed().is_empty(), "{:?}", report.failed());
    assert_eq!(calls.into_inner().unwrap(), plan.trash);
    assert!(r.path().join("logs").join("a.log").is_file());
    assert!(!r.path().join("extra").exists() && !r.path().join("old").exists());
}

#[test]
fn update_never_overwrites_when_times_are_equal_or_near() {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for (name, right_at) in [
        ("same-time.cfg", T0),
        ("near.cfg", T0 + 1),
        ("dst.cfg", T0 + 3600),
    ] {
        put_at(l.path(), name, "L\n", T0);
        put_at(r.path(), name, "R!\n", right_at);
    }
    let mut opts = FolderOptions::default();
    opts.quick.ignore_dst = true;
    let cmp = compare_with(l.path(), r.path(), &opts);
    for mode in [
        SyncMode::UpdateRight,
        SyncMode::UpdateLeft,
        SyncMode::UpdateBoth,
    ] {
        let plan = plan_all(&cmp, mode, l.path(), r.path());
        assert!(plan.is_empty(), "{mode:?}: {plan:?}");
        assert_eq!(
            skips(&plan),
            ["dst.cfg", "near.cfg", "same-time.cfg"].map(|n| skip(n, SkipReason::SameTime)),
            "{mode:?}"
        );
    }
}

#[test]
fn the_selected_scope_follows_the_filters() {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for name in ["conf/same0.xml", "conf/same1.xml"] {
        put_at(l.path(), name, "s\n", T0);
        put_at(r.path(), name, "s\n", T0);
    }
    put_at(l.path(), "conf/onlyL.xml", "l\n", T0);
    let cmp = quick(l.path(), r.path());
    let conf = index(&cmp, "conf");
    let only = index(&cmp, "conf/onlyL.xml");
    let not_same = |n: u32| cmp.nodes[n as usize].status != crate::folder::tree::Status::Same;
    let plan = sync_plan(
        &cmp,
        &[conf],
        &not_same,
        SyncMode::UpdateRight,
        l.path(),
        r.path(),
    );
    assert_eq!(
        dsts(&plan.to_right),
        [r.path().join("conf").join("onlyL.xml")]
    );
    // A file the filters hide is left out when reached through a folder...
    let hide_only = |n: u32| n != only;
    let plan = sync_plan(
        &cmp,
        &[conf],
        &hide_only,
        SyncMode::UpdateRight,
        l.path(),
        r.path(),
    );
    assert!(plan.is_empty(), "{plan:?}");
    // ...and counts when picked itself.
    let plan = sync_plan(
        &cmp,
        &[only],
        &|_| false,
        SyncMode::UpdateRight,
        l.path(),
        r.path(),
    );
    assert_eq!(
        dsts(&plan.to_right),
        [r.path().join("conf").join("onlyL.xml")]
    );
    // The Recycle Bin follows the same rule.
    let plan = sync_plan(
        &cmp,
        &[conf],
        &hide_only,
        SyncMode::MirrorLeft,
        l.path(),
        r.path(),
    );
    assert!(plan.trash.is_empty(), "{:?}", plan.trash);
    let plan = sync_plan(
        &cmp,
        &[conf],
        &not_same,
        SyncMode::MirrorLeft,
        l.path(),
        r.path(),
    );
    assert_eq!(plan.trash, [l.path().join("conf").join("onlyL.xml")]);
}

#[test]
fn odd_rows_are_skipped_with_their_reasons() {
    let mut unreadable = file("err", 1);
    unreadable.error = Some("access denied".into());
    let link = Entry {
        kind: EntryKind::Symlink,
        link_target: Some("target".into()),
        ..file("ln", 0)
    };
    // `a` is a file on the left and a folder (holding a/x) on the right.
    let left = vec![file("a", 1), unreadable, link];
    let right = vec![dir("a"), file("a/x", 1), file("err", 1)];
    let cmp = build_tree(left, right, true);
    let (l, r) = (Path::new("L"), Path::new("R"));
    let update = plan_all(&cmp, SyncMode::UpdateRight, l, r);
    assert!(update.is_empty(), "{update:?}");
    assert_eq!(
        skips(&update),
        [
            skip("a", SkipReason::Kind),
            skip("err", SkipReason::Unreadable),
            skip("ln", SkipReason::Link),
        ]
    );
    let mirror = plan_all(&cmp, SyncMode::MirrorRight, l, r);
    assert!(mirror.is_empty(), "{mirror:?}");
    assert_eq!(
        skips(&mirror),
        [
            skip("a", SkipReason::Kind),
            skip("a/x", SkipReason::Kind),
            skip("err", SkipReason::Unreadable),
            skip("ln", SkipReason::Link),
        ]
    );
    // The other way round: a/y under a left folder that is a file on the right.
    let cmp = build_tree(vec![dir("a"), file("a/y", 1)], vec![file("a", 1)], true);
    let update = plan_all(&cmp, SyncMode::UpdateRight, l, r);
    assert!(update.is_empty(), "{update:?}");
    assert_eq!(
        skips(&update),
        [skip("a", SkipReason::Kind), skip("a/y", SkipReason::Kind)]
    );
}

#[test]
fn nothing_under_an_unreadable_folder_is_copied_or_trashed() {
    // The right `bad` could not be listed, so the left's files only look left-only.
    let mut bad = dir("bad");
    bad.error = Some("access denied".into());
    let cmp = build_tree(vec![dir("bad"), file("bad/f", 1)], vec![bad], true);
    let (l, r) = (Path::new("L"), Path::new("R"));
    let expected = [
        skip("bad/", SkipReason::Unreadable),
        skip("bad/f", SkipReason::Unreadable),
    ];
    let update = plan_all(&cmp, SyncMode::UpdateRight, l, r);
    assert!(update.is_empty(), "{update:?}");
    assert_eq!(skips(&update), expected);
    let mirror = plan_all(&cmp, SyncMode::MirrorLeft, l, r);
    assert!(mirror.is_empty(), "{mirror:?}");
    assert_eq!(skips(&mirror), expected);
}

#[test]
fn a_file_its_batch_drops_becomes_a_kind_skip() {
    // `copy_batch` refuses m/x: the right's m is a file. Nothing may vanish silently.
    let cmp = build_tree(vec![dir("m"), file("m/x", 1)], vec![file("m", 1)], true);
    let x = index(&cmp, "m/x");
    let mut skipped = Vec::new();
    let batch = batch_for(
        &cmp,
        vec![x],
        Vec::new(),
        Side::Left,
        Path::new("L"),
        Path::new("R"),
        &mut skipped,
        &mut BTreeMap::new(),
    );
    assert!(batch.is_empty());
    assert_eq!(
        skipped,
        [(
            x,
            SyncSkip {
                rel: "m/x".into(),
                why: SkipReason::Kind
            }
        )]
    );
}

#[test]
fn empty_folders_are_made_where_the_mode_copies() {
    let (l, r) = (Path::new("L"), Path::new("R"));
    let cmp = build_tree(vec![dir("e")], vec![], true);
    for mode in [
        SyncMode::UpdateRight,
        SyncMode::UpdateBoth,
        SyncMode::MirrorRight,
    ] {
        let plan = plan_all(&cmp, mode, l, r);
        assert_eq!(plan.to_right.mkdirs, [r.join("e")], "{mode:?}");
        assert_eq!((plan.folders(), plan.files()), (1, 0), "{mode:?}");
        assert!(!plan.is_empty(), "{mode:?}");
    }
    let plan = plan_all(&cmp, SyncMode::UpdateLeft, l, r);
    assert!(plan.is_empty() && plan.skips.is_empty(), "{plan:?}");
    let cmp = build_tree(vec![], vec![dir("f")], true);
    let plan = plan_all(&cmp, SyncMode::UpdateLeft, l, r);
    assert_eq!(plan.to_left.mkdirs, [l.join("f")]);
    assert!(plan_all(&cmp, SyncMode::UpdateRight, l, r).is_empty());
}

#[test]
fn a_cancelled_sync_never_reaches_the_recycle_bin() {
    let (l, r, cmp) = sync_fixture();
    let plan = plan_all(&cmp, SyncMode::MirrorRight, l.path(), r.path());
    assert_eq!(plan.trash, [r.path().join("extra.txt")]);
    let cancel = CancelToken::new();
    cancel.cancel();
    let report = run_sync(
        &plan,
        l.path(),
        r.path(),
        &no_trash,
        &cancel,
        &AtomicUsize::new(0),
    );
    assert!(report.cancelled());
    assert_eq!(report.trash, None);
    assert_eq!(report.to_right.done, 0);
    assert!(r.path().join("extra.txt").is_file());
    assert!(!r.path().join("new.txt").exists());
    assert_eq!(fs::read_to_string(r.path().join("newer.txt")).unwrap(), "R");
    // A plan with only Recycle Bin items stops before the first one, and says so.
    let only_trash = SyncPlan {
        trash: plan.trash.clone(),
        trash_side: Some(Side::Right),
        ..SyncPlan::default()
    };
    let report = run_sync(
        &only_trash,
        l.path(),
        r.path(),
        &no_trash,
        &cancel,
        &AtomicUsize::new(0),
    );
    assert!(report.cancelled());
    assert_eq!(report.trash.map(|t| t.done), Some(0));
}

/// Every planned destination holds its source's bytes and modified time.
fn assert_copied(plan: &SyncPlan) {
    for item in plan.to_right.files.iter().chain(&plan.to_left.files) {
        assert_eq!(
            fs::read(&item.dst).unwrap(),
            fs::read(&item.src).unwrap(),
            "{item:?}"
        );
        let mtime = |p: &Path| fs::metadata(p).unwrap().modified().unwrap();
        assert_eq!(mtime(&item.dst), mtime(&item.src), "{item:?}");
    }
}

#[test]
fn run_sync_copies_both_ways_then_trashes() {
    let (l, r, cmp) = sync_fixture();
    let plan = plan_all(&cmp, SyncMode::UpdateBoth, l.path(), r.path());
    let done = AtomicUsize::new(0);
    let report = run_sync(
        &plan,
        l.path(),
        r.path(),
        &no_trash,
        &CancelToken::new(),
        &done,
    );
    assert!(
        !report.cancelled() && report.failed().is_empty(),
        "{report:?}"
    );
    assert_eq!((report.to_right.done, report.to_left.done), (2, 2));
    assert_eq!(done.load(std::sync::atomic::Ordering::Relaxed), 4);
    assert_copied(&plan);

    let (l, r, cmp) = sync_fixture();
    let plan = plan_all(&cmp, SyncMode::MirrorRight, l.path(), r.path());
    let calls = Mutex::new(Vec::new());
    let fake = |p: &Path| -> Result<(), ActionError> {
        // The copies are done before anything goes to the Recycle Bin.
        assert_eq!(
            fs::read_to_string(r.path().join("same-time.txt")).unwrap(),
            "L"
        );
        calls.lock().unwrap().push(p.to_path_buf());
        Ok(())
    };
    let report = run_sync(
        &plan,
        l.path(),
        r.path(),
        &fake,
        &CancelToken::new(),
        &AtomicUsize::new(0),
    );
    assert!(
        !report.cancelled() && report.failed().is_empty(),
        "{report:?}"
    );
    assert_eq!(calls.into_inner().unwrap(), [r.path().join("extra.txt")]);
    assert_eq!(report.trash.map(|t| t.done), Some(1));
    assert_copied(&plan);
}

#[test]
fn mirror_leaves_items_changed_since_the_compare() {
    let (l, r, cmp) = sync_fixture();
    fs::write(r.path().join("extra.txt"), "edited after the compare").unwrap();
    let plan = plan_all(&cmp, SyncMode::MirrorRight, l.path(), r.path());
    assert!(plan.trash.is_empty(), "{:?}", plan.trash);
    assert_eq!(skips(&plan), [skip("extra.txt", SkipReason::Changed)]);
    assert_eq!(plan.changed, [r.path().join("extra.txt")]);
}

/// M-8: the preview names each item that changed, a folder with its "/".
#[test]
fn mirror_names_the_items_that_changed_since_the_compare() {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(l.path(), "keep.txt", "k\n", T0);
    put_at(r.path(), "keep.txt", "k\n", T0);
    put_at(r.path(), "extra.txt", "x\n", T0);
    put_at(r.path(), "gone/f.txt", "f\n", T0);
    let cmp = quick(l.path(), r.path());
    put_at(r.path(), "extra.txt", "edited\n", T0);
    fs::remove_dir_all(r.path().join("gone")).unwrap();
    let plan = plan_all(&cmp, SyncMode::MirrorRight, l.path(), r.path());
    assert!(plan.trash.is_empty(), "{:?}", plan.trash);
    assert_eq!(
        skips(&plan),
        [
            skip("gone/", SkipReason::Changed),
            skip("extra.txt", SkipReason::Changed)
        ]
    );
    assert_eq!(
        plan.changed,
        [r.path().join("gone"), r.path().join("extra.txt")]
    );
}

/// F-100: the question took `gone2` whole; a file added two levels down before the run makes
/// the new plan keep it and take only its known files. It is left alone whole instead, and its
/// skip says it changed, with its "/". A folder already kept when the question was asked keeps
/// its own reason, and the files the question named in it still go.
#[test]
fn a_folder_that_grew_after_the_question_is_left_alone_whole() {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(l.path(), "keep.txt", "k\n", T0);
    put_at(r.path(), "keep.txt", "k\n", T0);
    put_at(r.path(), "gone2/h.txt", "h\n", T0);
    put_at(r.path(), "gone2/sub/i.txt", "i\n", T0);
    put_at(r.path(), "kept/a.txt", "a\n", T0);
    let cmp = quick(l.path(), r.path());
    put_at(r.path(), "kept/new.txt", "new before the question\n", T0);
    let asked = plan_all(&cmp, SyncMode::MirrorRight, l.path(), r.path());
    let (gone2, kept) = (r.path().join("gone2"), r.path().join("kept"));
    assert!(asked.trash.contains(&gone2), "{:?}", asked.trash);
    assert!(
        asked.trash.contains(&kept.join("a.txt")),
        "{:?}",
        asked.trash
    );
    put_at(r.path(), "gone2/sub/h2.txt", "new after the question\n", T0);
    let mut plan = plan_all(&cmp, SyncMode::MirrorRight, l.path(), r.path());
    assert!(
        plan.trash.contains(&gone2.join("h.txt")),
        "the new plan alone splits it"
    );
    plan.leave_folders_grown_since(&asked, l.path(), r.path());
    assert_eq!(plan.trash, [kept.join("a.txt")]);
    assert_eq!(
        skips(&plan),
        [
            skip("gone2/", SkipReason::Changed),
            skip("kept/", SkipReason::KeptFolder)
        ]
    );
    assert_eq!(plan.changed, [gone2]);
}

/// I-2: a file edited in place leaves its folder's listing and time as they were; the folder
/// does not go whole, the edited file stays, and the rest goes.
#[test]
fn mirror_leaves_a_folder_whose_file_was_edited_since_the_compare() {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(l.path(), "keep.txt", "k\n", T0);
    put_at(r.path(), "keep.txt", "k\n", T0);
    put_at(r.path(), "old/a.txt", "a\n", T0);
    put_at(r.path(), "old/b.txt", "b\n", T0);
    let cmp = quick(l.path(), r.path());
    let whole = plan_all(&cmp, SyncMode::MirrorRight, l.path(), r.path());
    assert_eq!(whole.trash, [r.path().join("old")]);
    put_at(r.path(), "old/a.txt", "edited\n", T0);
    let plan = plan_all(&cmp, SyncMode::MirrorRight, l.path(), r.path());
    assert_eq!(plan.trash, [r.path().join("old").join("b.txt")]);
    assert!(!plan.trash.contains(&r.path().join("old").join("a.txt")));
    assert_eq!(
        plan.skips.iter().map(|s| s.why).collect::<Vec<_>>(),
        [SkipReason::Changed]
    );
}

/// The Task 9 ruling: what the plan saw at each overwrite and each Recycle Bin path is
/// checked again just before the run acts on it.
#[test]
fn run_sync_leaves_targets_that_changed_between_plan_and_run() {
    let (l, r, cmp) = sync_fixture();
    let plan = plan_all(&cmp, SyncMode::MirrorRight, l.path(), r.path());
    // After the plan: newer.txt gets another size (same time), older.txt another time
    // (same bytes), and extra.txt, bound for the Recycle Bin, another time.
    put_at(r.path(), "newer.txt", "edited after the plan", T0);
    File::options()
        .write(true)
        .open(r.path().join("older.txt"))
        .unwrap()
        .set_modified(at(T0 + 7200))
        .unwrap();
    put_at(r.path(), "extra.txt", "y", T0 + 60);
    let calls = Mutex::new(Vec::new());
    let fake = |p: &Path| -> Result<(), ActionError> {
        calls.lock().unwrap().push(p.to_path_buf());
        Ok(())
    };
    let report = run_sync(
        &plan,
        l.path(),
        r.path(),
        &fake,
        &CancelToken::new(),
        &AtomicUsize::new(0),
    );
    let changed = |name: &str| (r.path().join(name), "Changed since the compare.".to_owned());
    assert_eq!(
        report.failed(),
        [
            changed("newer.txt"),
            changed("older.txt"),
            changed("extra.txt")
        ]
    );
    assert!(
        calls.into_inner().unwrap().is_empty(),
        "a changed item never reaches the Recycle Bin"
    );
    let read = |name: &str| fs::read_to_string(r.path().join(name)).unwrap();
    assert_eq!(read("newer.txt"), "edited after the plan");
    assert_eq!(read("older.txt"), "R-older");
    assert_eq!(read("extra.txt"), "y");
    // What did not change is synced as planned.
    assert_eq!(
        (read("new.txt"), read("same-time.txt")),
        ("n".into(), "L".into())
    );
    assert_eq!(report.to_right.done, 2);
    assert_eq!(report.trash.map(|t| t.done), Some(0));
    assert_eq!(
        plan.seen.keys().cloned().collect::<Vec<_>>(),
        ["extra.txt", "newer.txt", "older.txt", "same-time.txt"].map(|n| r.path().join(n)),
        "every overwrite and every Recycle Bin path is recorded"
    );
}

#[test]
fn run_sync_leaves_a_folder_that_changed_between_plan_and_run() {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(l.path(), "keep.txt", "k\n", T0);
    put_at(r.path(), "keep.txt", "k\n", T0);
    put_at(r.path(), "gone/b.txt", "b\n", T0);
    put_at(r.path(), "old/a.txt", "a\n", T0);
    put_at(r.path(), "deep/sub/b.txt", "b\n", T0);
    put_at(r.path(), "edit/c.txt", "c\n", T0);
    let cmp = quick(l.path(), r.path());
    let plan = plan_all(&cmp, SyncMode::MirrorRight, l.path(), r.path());
    let at = |name: &str| r.path().join(name);
    assert_eq!(plan.trash, [at("deep"), at("edit"), at("gone"), at("old")]);
    // A file lands in old/ after the plan; the folder's own time moves on (past one tick
    // of the system clock).
    std::thread::sleep(Duration::from_millis(50));
    put_at(r.path(), "old/new.txt", "added after the plan\n", T0);
    // I-2: one lands two levels down, where only deep/sub's time moves; another file is
    // edited in place, where no folder's time moves.
    put_at(r.path(), "deep/sub/new.txt", "added after the plan\n", T0);
    put_at(r.path(), "edit/c.txt", "edited after the plan\n", T0);
    let remove = |p: &Path| -> Result<(), ActionError> {
        fs::remove_dir_all(p).map_err(|source| ActionError::Io {
            path: p.to_path_buf(),
            source,
        })
    };
    let report = run_sync(
        &plan,
        l.path(),
        r.path(),
        &remove,
        &CancelToken::new(),
        &AtomicUsize::new(0),
    );
    let changed = |name: &str| (at(name), "Changed since the compare.".to_owned());
    assert_eq!(
        report.failed(),
        [changed("deep"), changed("edit"), changed("old")]
    );
    assert!(r.path().join("old").join("new.txt").is_file());
    assert!(r.path().join("old").join("a.txt").is_file());
    assert!(r.path().join("deep/sub/new.txt").is_file());
    assert!(r.path().join("deep/sub/b.txt").is_file());
    assert!(r.path().join("edit/c.txt").is_file());
    assert!(!r.path().join("gone").exists(), "the unchanged folder went");
}

#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    let out = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
}

/// M-7: a folder on the way to a Recycle Bin path became a junction after the plan, and the
/// file it now leads to has the size and time the compare saw. Nothing is moved.
#[cfg(windows)]
#[test]
fn run_sync_never_trashes_through_a_junction_made_after_the_plan() {
    let (l, r, outside) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    put_at(l.path(), "keep/same.txt", "s\n", T0);
    put_at(r.path(), "keep/same.txt", "s\n", T0);
    put_at(r.path(), "keep/extra.txt", "x\n", T0);
    let cmp = quick(l.path(), r.path());
    let plan = plan_all(&cmp, SyncMode::MirrorRight, l.path(), r.path());
    let target = r.path().join("keep").join("extra.txt");
    assert_eq!(plan.trash, std::slice::from_ref(&target));
    fs::rename(r.path().join("keep"), r.path().join("keep-moved")).unwrap();
    put_at(outside.path(), "extra.txt", "x\n", T0);
    junction(&r.path().join("keep"), outside.path());
    let calls = Mutex::new(Vec::new());
    let fake = |p: &Path| -> Result<(), ActionError> {
        calls.lock().unwrap().push(p.to_path_buf());
        Ok(())
    };
    let report = run_sync(
        &plan,
        l.path(),
        r.path(),
        &fake,
        &CancelToken::new(),
        &AtomicUsize::new(0),
    );
    assert!(
        calls.into_inner().unwrap().is_empty(),
        "the bin is never reached"
    );
    let failed = report.failed();
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert_eq!(failed[0].0, target);
    assert!(failed[0].1.contains("link or junction"), "{failed:?}");
    assert!(outside.path().join("extra.txt").is_file());
}

/// M-12: a cancelled plan stops; a live token plans as `sync_plan` does.
#[test]
fn sync_plan_stops_when_cancelled() {
    let (l, r, cmp) = sync_fixture();
    let top: Vec<u32> = (0..cmp.nodes.len() as u32)
        .filter(|&n| cmp.nodes[n as usize].parent.is_none())
        .collect();
    let mode = SyncMode::MirrorRight;
    let cancel = CancelToken::new();
    let plan = |cancel: &CancelToken| {
        sync_plan_with_cancel(&cmp, &top, &|_| true, mode, l.path(), r.path(), cancel)
    };
    assert_eq!(plan(&cancel), Ok(plan_all(&cmp, mode, l.path(), r.path())));
    cancel.cancel();
    assert_eq!(plan(&cancel), Err(Cancelled));
}

/// M-10 (PF13): Quick and Size only never read a file, so a case-only skip does not say the
/// contents match.
#[test]
fn a_case_only_skip_names_what_the_criterion_compared() {
    let case = SkipReason::CaseOnly;
    for mode in [CompareMode::Content, CompareMode::Rules] {
        assert_eq!(case.text_in(mode), case.text(), "{mode:?}");
    }
    assert_eq!(
        case.text_in(CompareMode::Quick),
        "the names differ only in case and the size and modified time match: rename it by hand"
    );
    assert_eq!(
        case.text_in(CompareMode::Size),
        "the names differ only in case and the size matches: rename it by hand"
    );
    assert_eq!(
        SkipReason::Link.text_in(CompareMode::Quick),
        SkipReason::Link.text()
    );
}

#[test]
fn run_sync_refuses_nested_roots() {
    // The right root lives inside the left: Mirror to left would take the whole right
    // root as a left-only folder, and Mirror to right would copy into its own source.
    let dir = tempfile::tempdir().unwrap();
    let (outer, inner) = (dir.path().join("deploy"), dir.path().join("deploy/backup"));
    put_at(&outer, "app.cfg", "current\n", T0);
    put_at(&inner, "app.cfg", "backup copy\n", T0 + 3600);
    let cmp = quick(&outer, &inner);
    let back = plan_all(&cmp, SyncMode::MirrorLeft, &outer, &inner);
    assert!(back.trash.contains(&inner), "{:?}", back.trash);
    for plan in [back, plan_all(&cmp, SyncMode::MirrorRight, &outer, &inner)] {
        let report = run_sync(
            &plan,
            &outer,
            &inner,
            &no_trash,
            &CancelToken::new(),
            &AtomicUsize::new(0),
        );
        assert!(!report.failed().is_empty(), "{report:?}");
        assert_eq!(report.trash, None);
        assert_eq!(report.to_right.done + report.to_left.done, 0);
    }
    assert_eq!(
        fs::read_to_string(outer.join("app.cfg")).unwrap(),
        "current\n"
    );
    assert_eq!(
        fs::read_to_string(inner.join("app.cfg")).unwrap(),
        "backup copy\n"
    );
    assert!(!inner.join("backup").exists());
}

#[test]
fn skip_reasons_read_as_the_brief_says() {
    assert_eq!(SkipReason::Newer(Side::Right).text(), "the right is newer");
    assert_eq!(SkipReason::Newer(Side::Left).text(), "the left is newer");
    assert_eq!(
        SkipReason::SameTime.text(),
        "the modified times are equal or near: can't tell which is newer"
    );
    assert_eq!(
        SkipReason::CaseOnly.text(),
        "the names differ only in case and the contents match: rename it by hand"
    );
    assert_eq!(
        SkipReason::KeptFolder.text(),
        "holds files that are not in the compare (filtered out, links or unreadable): the folder stays"
    );
    assert_eq!(
        SkipReason::Changed.text(),
        "it changed on disk since the compare"
    );
    assert_eq!(SkipReason::Unreadable.text(), "it could not be read");
    assert_eq!(
        SkipReason::Kind.text(),
        "a file on one side and a folder or link on the other"
    );
    assert_eq!(SkipReason::Link.text(), "links are never copied");
    assert_eq!(SyncMode::ALL.len(), 5);
}

fn moved_options() -> FolderOptions {
    FolderOptions {
        mode: CompareMode::Content,
        find_moves: true,
        ..FolderOptions::default()
    }
}

fn moved_pair(left: &str, right: &str) -> (TempDir, TempDir, FolderComparison) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put_at(l.path(), left, "moved body", T0);
    put_at(r.path(), right, "moved body", T0);
    let cmp = compare_with(l.path(), r.path(), &moved_options());
    (l, r, cmp)
}

#[test]
fn a_mirror_that_copies_one_file_and_trashes_its_twin_records_a_move() {
    let (l, r, cmp) = moved_pair("new/a.txt", "old/a.txt");
    assert_eq!(cmp.moved_files(), 1);
    let plan = plan_all(&cmp, SyncMode::MirrorRight, l.path(), r.path());
    assert_eq!(
        plan.moves.get("new/a.txt").map(String::as_str),
        Some("old/a.txt")
    );
    assert_eq!(
        plan.moves.get("old/a.txt").map(String::as_str),
        Some("new/a.txt")
    );
    let back = plan_all(&cmp, SyncMode::MirrorLeft, l.path(), r.path());
    assert_eq!(back.moves.len(), 2);
}

#[test]
fn a_twin_inside_a_trashed_folder_still_counts() {
    let (l, r, cmp) = moved_pair("new/a.txt", "old/a.txt");
    let plan = plan_all(&cmp, SyncMode::MirrorRight, l.path(), r.path());
    assert!(
        plan.trash.iter().any(|p| p.ends_with("old")),
        "{:?}",
        plan.trash
    );
    assert_eq!(plan.moves.len(), 2);
}

#[test]
fn a_scope_with_only_one_half_of_the_pair_records_no_move() {
    // The copy without the trash, and the trash without the copy: neither is a move.
    let (l, r, cmp) = moved_pair("new/a.txt", "old/a.txt");
    let (new, old) = (index(&cmp, "new"), index(&cmp, "old"));
    let both = sync_plan(
        &cmp,
        &[new, old],
        &|_| true,
        SyncMode::MirrorRight,
        l.path(),
        r.path(),
    );
    assert_eq!(both.moves.len(), 2, "the control: both halves in scope");
    for half in [new, old] {
        let plan = sync_plan(
            &cmp,
            &[half],
            &|_| true,
            SyncMode::MirrorRight,
            l.path(),
            r.path(),
        );
        assert!(
            !plan.to_right.files.is_empty() || !plan.trash.is_empty(),
            "the half still plans something"
        );
        assert!(plan.moves.is_empty(), "{:?}", plan.moves);
    }
}

#[test]
fn update_mode_records_no_moves() {
    let (l, r, cmp) = moved_pair("a.txt", "b.txt");
    for mode in [
        SyncMode::UpdateRight,
        SyncMode::UpdateLeft,
        SyncMode::UpdateBoth,
    ] {
        assert!(plan_all(&cmp, mode, l.path(), r.path()).moves.is_empty());
    }
}

#[test]
fn moves_do_not_change_the_plan() {
    let (l, r, cmp) = moved_pair("new/a.txt", "old/a.txt");
    let mut plain = cmp.clone();
    plain.moved.clear();
    for mode in SyncMode::ALL {
        let with = plan_all(&cmp, mode, l.path(), r.path());
        let without = plan_all(&plain, mode, l.path(), r.path());
        assert_eq!(with.to_right, without.to_right);
        assert_eq!(with.to_left, without.to_left);
        assert_eq!(with.trash, without.trash);
        assert_eq!(with.skips, without.skips);
        assert_eq!(with.seen, without.seen);
        assert_eq!(with.inside, without.inside);
        assert_eq!(with.changed, without.changed);
        assert!(without.moves.is_empty());
    }
}
