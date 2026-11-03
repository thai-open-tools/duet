use super::*;
use crate::folder::report::report_items;
use crate::folder::run::{CompareMode, FolderOptions, FolderProgress, compare_folders};
use crate::text::diff::WhitespaceMode;
use std::fs;
use std::path::Path;
use std::sync::atomic::Ordering;

struct Fixture {
    left: tempfile::TempDir,
    right: tempfile::TempDir,
    items: Vec<ReportItem>,
}

fn write(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// Items of every file under two temp folders, compared by content.
fn fixture(files: &[(&str, &[u8], &[u8])], mode: CompareMode) -> Fixture {
    let (left, right) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for (rel, l, r) in files {
        write(left.path(), rel, l);
        write(right.path(), rel, r);
    }
    let opts = FolderOptions {
        mode,
        ..FolderOptions::default()
    };
    let cmp = compare_folders(
        left.path(),
        right.path(),
        &opts,
        &FolderProgress::default(),
        &CancelToken::new(),
    )
    .unwrap();
    let nodes: Vec<u32> = (0..cmp.nodes.len() as u32)
        .filter(|&n| !cmp.nodes[n as usize].is_dir)
        .collect();
    let items = report_items(&cmp, &nodes, left.path(), right.path(), None);
    Fixture { left, right, items }
}

fn text(files: &[(&str, &str, &str)]) -> Fixture {
    let raw: Vec<(&str, &[u8], &[u8])> = files
        .iter()
        .map(|(n, l, r)| (*n, l.as_bytes(), r.as_bytes()))
        .collect();
    fixture(&raw, CompareMode::Content)
}

fn opts() -> LineOptions {
    LineOptions {
        rows: true,
        context: 2,
        mask: false,
        sheet: false,
        file_rows: FILE_ROWS,
        total_rows: TOTAL_ROWS,
        sheet_rows: SHEET_ROWS,
    }
}

fn run(fx: &Fixture, rules: &FolderTextRules, opts: &LineOptions) -> LineDetails {
    line_details(
        &fx.items,
        rules,
        opts,
        &AtomicUsize::new(0),
        &CancelToken::new(),
    )
}

type Shape = (RowClass, Option<(u32, String)>, Option<(u32, String)>);

fn shape(rows: &[DetailRow]) -> Vec<Result<Shape, String>> {
    rows.iter()
        .map(|row| match row {
            DetailRow::Line {
                class, left, right, ..
            } => Ok((*class, left.clone(), right.clone())),
            DetailRow::Skip(n) => Err(format!("skip {n}")),
            DetailRow::More(n) => Err(format!("more {n}")),
        })
        .collect()
}

fn ln(n: u32, s: &str) -> Option<(u32, String)> {
    Some((n, s.to_owned()))
}

fn numbered(prefix: &str, count: usize) -> String {
    (1..=count).map(|i| format!("{prefix}{i}\n")).collect()
}

#[test]
fn line_details_count_changed_lines_and_mask_secrets() {
    let fx = text(&[(
        "app/db.properties",
        "url=jdbc:x\nuser=svc\npassword=Secr3t-A\ntimeout=30\n",
        "url=jdbc:x\nuser=svc\npassword=Secr3t-B\ntimeout=60\n",
    )]);
    let progress = AtomicUsize::new(0);
    let details = line_details(
        &fx.items,
        &FolderTextRules::default(),
        &LineOptions {
            mask: true,
            ..opts()
        },
        &progress,
        &CancelToken::new(),
    );
    let info = &details.files["app/db.properties"];
    assert!(info.counted);
    assert_eq!(
        (info.changed, info.left_only, info.right_only, info.places),
        (2, 0, 0, 1)
    );
    assert_eq!(details.masked_lines, 2);
    assert_eq!(
        shape(&info.rows),
        vec![
            Ok((RowClass::Same, ln(1, "url=jdbc:x"), ln(1, "url=jdbc:x"))),
            Ok((RowClass::Same, ln(2, "user=svc"), ln(2, "user=svc"))),
            Ok((
                RowClass::Mod,
                ln(3, "password=•••••• (hidden #d397)"),
                ln(3, "password=•••••• (hidden #d097)")
            )),
            Ok((RowClass::Mod, ln(4, "timeout=30"), ln(4, "timeout=60"))),
        ]
    );
    // A paired changed row carries the character marks of both sides.
    assert!(matches!(
        &info.rows[3],
        DetailRow::Line { marks: Some((l, r)), .. } if !l.is_empty() && !r.is_empty()
    ));
    assert!(matches!(&info.rows[0], DetailRow::Line { marks: None, .. }));
    let names = ["IWT".to_owned(), "PRD".to_owned()];
    assert_eq!(line_summary(info, &names), "Lines: 2 changed");
    assert_eq!(progress.load(Ordering::Relaxed), 1, "one file per item");
}

#[test]
fn without_masking_secrets_stay_as_they_are() {
    let fx = text(&[("a.ini", "password=one\n", "password=two\n")]);
    let details = run(&fx, &FolderTextRules::default(), &opts());
    assert_eq!(details.masked_lines, 0);
    assert!(matches!(
        &details.files["a.ini"].rows[0],
        DetailRow::Line { left: Some((1, l)), .. } if l == "password=one"
    ));
}

#[test]
fn context_rows_and_skips_follow_localdiff() {
    let l = numbered("l", 10);
    let r = l.replace("l6\n", "l6x\n");
    let fx = text(&[("a.txt", &l, &r)]);
    let details = run(
        &fx,
        &FolderTextRules::default(),
        &LineOptions {
            context: 1,
            ..opts()
        },
    );
    assert_eq!(
        shape(&details.files["a.txt"].rows),
        vec![
            Ok((RowClass::Same, ln(5, "l5"), ln(5, "l5"))),
            Ok((RowClass::Mod, ln(6, "l6"), ln(6, "l6x"))),
            Ok((RowClass::Same, ln(7, "l7"), ln(7, "l7"))),
            Err("skip 3".to_owned()),
        ]
    );
}

#[test]
fn a_gap_between_two_places_is_one_skip_row() {
    let l = numbered("l", 12);
    let r = l.replace("l2\n", "x2\n").replace("l11\n", "x11\n");
    let fx = text(&[("a.txt", &l, &r)]);
    let details = run(
        &fx,
        &FolderTextRules::default(),
        &LineOptions {
            context: 1,
            ..opts()
        },
    );
    let info = &details.files["a.txt"];
    assert_eq!(info.places, 2);
    let shown = shape(&info.rows);
    assert_eq!(shown[0], Ok((RowClass::Same, ln(1, "l1"), ln(1, "l1"))));
    assert!(shown.contains(&Err("skip 6".to_owned())), "{shown:?}");
    assert_eq!(shown.last().unwrap().as_ref().unwrap().1, ln(12, "l12"));
}

#[test]
fn rows_stop_at_the_per_file_cap() {
    let fx = text(&[("a.txt", &numbered("a", 400), &numbered("b", 400))]);
    let details = run(
        &fx,
        &FolderTextRules::default(),
        &LineOptions {
            context: 0,
            ..opts()
        },
    );
    let info = &details.files["a.txt"];
    assert_eq!(info.rows.len(), 301);
    assert!(info.rows[..300].iter().all(|r| matches!(
        r,
        DetailRow::Line {
            class: RowClass::Mod,
            ..
        }
    )));
    assert_eq!(info.rows[300], DetailRow::More(100));
    assert_eq!(info.changed, 400, "counts cover every line, shown or not");
}

#[test]
fn the_report_stops_showing_lines_at_its_total() {
    let fx = text(&[
        ("a.txt", &numbered("a", 4), &numbered("b", 4)),
        ("b.txt", &numbered("a", 4), &numbered("b", 4)),
        ("c.txt", &numbered("a", 4), &numbered("b", 4)),
    ]);
    let details = run(
        &fx,
        &FolderTextRules::default(),
        &LineOptions {
            context: 0,
            total_rows: 5,
            ..opts()
        },
    );
    assert_eq!(details.files["a.txt"].rows.len(), 4);
    let second = &details.files["b.txt"].rows;
    assert_eq!(second.len(), 2);
    assert_eq!(second[1], DetailRow::More(3));
    let third = &details.files["c.txt"];
    assert!(third.rows.is_empty());
    assert_eq!(
        third.note.as_deref(),
        Some("The report reached its size limit: no lines are shown for this file.")
    );
    assert_eq!(third.changed, 4, "counts are still read");
}

#[test]
fn counts_are_read_even_when_no_rows_are_asked_for() {
    let fx = text(&[("a.txt", "a\nb\n", "a\nc\n")]);
    let details = run(
        &fx,
        &FolderTextRules::default(),
        &LineOptions {
            rows: false,
            ..opts()
        },
    );
    let info = &details.files["a.txt"];
    assert!(info.counted && info.rows.is_empty() && info.note.is_none());
    assert_eq!(info.changed, 1);
}

#[test]
fn big_binary_and_unreadable_files_get_a_note() {
    let big = vec![b'a'; FILE_MAX as usize + 1];
    let fx = fixture(
        &[
            ("big.txt", &big, b"small\n"),
            ("bin.dat", b"ab\0cd\n", b"ab\0ce\n"),
            ("gone.txt", b"one\n", b"two\n"),
        ],
        CompareMode::Content,
    );
    fs::remove_file(fx.right.path().join("gone.txt")).unwrap();
    let details = run(&fx, &FolderTextRules::default(), &opts());
    let note = |name: &str| {
        let info = &details.files[name];
        assert!(!info.counted && info.rows.is_empty(), "{name}");
        info.note.clone().unwrap()
    };
    assert_eq!(
        note("big.txt"),
        "The file is larger than 8 MiB: no lines are shown (open it in duet to see them)."
    );
    assert_eq!(
        note("bin.dat"),
        "Binary file: compare it in duet's hex view."
    );
    assert_eq!(note("gone.txt"), "Could not read the file.");
}

#[test]
fn a_pair_that_is_not_two_files_is_never_read() {
    let mut fx = text(&[("a.txt", "one\n", "two\n")]);
    fx.items[0].right.as_mut().unwrap().kind = crate::folder::entry::EntryKind::Symlink;
    let details = run(&fx, &FolderTextRules::default(), &opts());
    let info = &details.files["a.txt"];
    assert!(!info.counted);
    assert_eq!(
        info.note.as_deref(),
        Some("Only plain files are read: no lines are shown.")
    );
}

#[cfg(unix)]
#[test]
fn a_link_on_disk_is_not_followed() {
    let fx = text(&[("a.txt", "one\n", "two\n")]);
    let path = fx.right.path().join("a.txt");
    let target = fx.right.path().join("target.txt");
    fs::rename(&path, &target).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    let details = run(&fx, &FolderTextRules::default(), &opts());
    assert!(!details.files["a.txt"].counted);
}

#[test]
fn odd_space_pairs_compare_without_rules() {
    let fx = text(&[("m.conf", "max = 10\n", "max =\u{a0}10\n")]);
    assert!(
        fx.items[0]
            .note
            .as_ref()
            .is_some_and(|n| n.warning == Some(crate::folder::classify::Warning::OddSpace))
    );
    let rules = FolderTextRules {
        whitespace: WhitespaceMode::All,
        ..FolderTextRules::default()
    };
    let details = run(&fx, &rules, &opts());
    assert_eq!(details.files["m.conf"].changed, 1);
}

#[test]
fn soft_rows_show_when_nothing_else_differs() {
    let fx = text(&[("a.txt", "A\nb\n", "a\nb\n")]);
    let rules = FolderTextRules {
        ignore_case: true,
        ..FolderTextRules::default()
    };
    let details = run(&fx, &rules, &opts());
    let info = &details.files["a.txt"];
    assert_eq!(
        shape(&info.rows),
        vec![
            Ok((RowClass::Soft, ln(1, "A"), ln(1, "a"))),
            Ok((RowClass::Same, ln(2, "b"), ln(2, "b"))),
        ]
    );
    assert!(matches!(
        &info.rows[0],
        DetailRow::Line { marks: Some(_), .. }
    ));
    assert_eq!((info.changed, info.soft, info.places), (0, 1, 0));
    let names = ["IWT".to_owned(), "PRD".to_owned()];
    assert_eq!(
        line_summary(info, &names),
        "Differs only in unimportant parts"
    );
}

#[test]
fn a_blank_last_line_is_kept() {
    // PF15: "a\n\n" has a second, blank line that "a\n" lacks.
    let fx = text(&[("a.txt", "a\n\n", "a\n")]);
    let details = run(&fx, &FolderTextRules::default(), &opts());
    let info = &details.files["a.txt"];
    assert_eq!((info.left_only, info.right_only, info.changed), (1, 0, 0));
    assert!(matches!(
        info.rows.last().unwrap(),
        DetailRow::Line { class: RowClass::Del, left: Some((2, l)), right: None, .. } if l.is_empty()
    ));
}

#[test]
fn sheet_rows_list_each_differing_line_once() {
    let fx = text(&[(
        "a.txt",
        "l1\nl2\nl3\nl4\nl5\nl6\n",
        "l1\nx2\nl3\nl4\nx5\nl6\nl7\n",
    )]);
    let details = run(
        &fx,
        &FolderTextRules::default(),
        &LineOptions {
            sheet: true,
            ..opts()
        },
    );
    let sheet: Vec<_> = details.files["a.txt"]
        .sheet
        .iter()
        .map(|r| {
            (
                r.kind,
                r.place,
                r.left.as_ref().map(|l| l.0),
                r.right.as_ref().map(|l| l.0),
            )
        })
        .collect();
    assert_eq!(
        sheet,
        vec![
            (SheetKind::Changed, 1, Some(2), Some(2)),
            (SheetKind::Changed, 2, Some(5), Some(5)),
            (SheetKind::RightOnly, 3, None, Some(7)),
        ]
    );
}

#[test]
fn sheet_rows_use_soft_lines_when_nothing_else_differs() {
    let fx = text(&[("a.txt", "A\nb\n", "a\nb\n")]);
    let rules = FolderTextRules {
        ignore_case: true,
        ..FolderTextRules::default()
    };
    let details = run(
        &fx,
        &rules,
        &LineOptions {
            sheet: true,
            ..opts()
        },
    );
    let sheet = &details.files["a.txt"].sheet;
    assert_eq!(sheet.len(), 1);
    assert_eq!(sheet[0].kind, SheetKind::Soft);
}

#[test]
fn sheet_rows_stop_at_their_cap() {
    let fx = text(&[
        ("a.txt", "a1\na2\n", "b1\nb2\n"),
        ("b.txt", "a1\na2\n", "b1\nb2\n"),
    ]);
    let details = run(
        &fx,
        &FolderTextRules::default(),
        &LineOptions {
            sheet: true,
            sheet_rows: 2,
            ..opts()
        },
    );
    assert_eq!(details.files["a.txt"].sheet.len(), 2);
    assert!(details.files["b.txt"].sheet.is_empty());
    assert_eq!(details.sheet_cut_files, 1);
    assert!(details.files["b.txt"].counted);
}

#[test]
fn cancel_stops_reading() {
    let fx = text(&[("a.txt", "a\n", "b\n"), ("b.txt", "a\n", "b\n")]);
    let cancel = CancelToken::new();
    cancel.cancel();
    let details = line_details(
        &fx.items,
        &FolderTextRules::default(),
        &opts(),
        &AtomicUsize::new(0),
        &cancel,
    );
    assert!(details.files.is_empty());
    assert_eq!(details.cancelled_files, 2);
}

#[test]
fn items_that_need_no_lines_are_skipped() {
    let fx = text(&[("same.txt", "x\n", "x\n"), ("diff.txt", "a\n", "b\n")]);
    let details = run(&fx, &FolderTextRules::default(), &opts());
    assert_eq!(details.files.len(), 1);
    assert!(details.files.contains_key("diff.txt"));
    let _ = (&fx.left, &fx.right);
}

#[test]
fn line_summaries_name_the_sides() {
    let names = ["IWT".to_owned(), "PRD".to_owned()];
    let info = LineInfo {
        counted: true,
        changed: 2,
        left_only: 1,
        right_only: 3,
        places: 2,
        ..LineInfo::default()
    };
    assert_eq!(
        line_summary(&info, &names),
        "Lines: 2 changed · 1 only in IWT · 3 only in PRD (2 places)"
    );
    let one = LineInfo {
        counted: true,
        left_only: 1,
        places: 1,
        ..LineInfo::default()
    };
    assert_eq!(line_summary(&one, &names), "Lines: 1 only in IWT");
    assert_eq!(line_summary(&LineInfo::default(), &names), "");
    let counted_nothing = LineInfo {
        counted: true,
        ..LineInfo::default()
    };
    assert_eq!(line_summary(&counted_nothing, &names), "");
}

/// Every text a line row or a Changed lines sheet row carries, both sides.
fn row_texts(info: &LineInfo) -> Vec<String> {
    let mut out = Vec::new();
    for row in &info.rows {
        if let DetailRow::Line { left, right, .. } = row {
            out.extend([left, right].into_iter().flatten().map(|(_, t)| t.clone()));
        }
    }
    for row in &info.sheet {
        out.extend(
            [&row.left, &row.right]
                .into_iter()
                .flatten()
                .map(|(_, t)| t.clone()),
        );
    }
    out
}

/// C-1 (K3): a private key's body never reaches the rows or the sheet, and the compare still
/// sees the one line that changed.
#[test]
fn a_private_key_block_is_hidden_in_rows_and_sheet() {
    let body = [
        "MIIEowIBAAKCAQEA0Z3VS5JJcds3xfn",
        "q5x/AbC+defLeftBody0001==",
        "k9Jz3mN8pQ7rT2vW6yX1aB4c",
        "q5x/AbC+defRightBody002==",
    ];
    let key = |second: &str| {
        format!(
            "-----BEGIN RSA PRIVATE KEY-----\n{}\n{second}\n{}\n-----END RSA PRIVATE KEY-----\n",
            body[0], body[2]
        )
    };
    let (left, right) = (key(body[1]), key(body[3]));
    let fx = text(&[("key.pem", left.as_str(), right.as_str())]);
    let masked = LineOptions {
        mask: true,
        sheet: true,
        context: 20,
        ..opts()
    };
    let details = run(&fx, &FolderTextRules::default(), &masked);
    let info = &details.files["key.pem"];
    assert_eq!((info.changed, info.left_only, info.right_only), (1, 0, 0));
    assert_eq!(details.masked_lines, 6, "three body lines a side");
    let texts = row_texts(info);
    assert!(texts.iter().any(|t| t == "-----BEGIN RSA PRIVATE KEY-----"));
    assert!(texts.iter().any(|t| t.contains("(hidden #")));
    for line in body {
        assert!(
            !texts.iter().any(|t| t.contains(line)),
            "{line} in {texts:?}"
        );
    }
    // The control: without masking the body shows.
    let plain = run(
        &fx,
        &FolderTextRules::default(),
        &LineOptions {
            mask: false,
            ..masked
        },
    );
    let texts = row_texts(&plain.files["key.pem"]);
    assert!(texts.iter().any(|t| t == body[3]), "{texts:?}");
    assert_eq!(plain.masked_lines, 0);
}

#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    let out = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
}

/// M-2: a folder above the file that became a junction after the compare is never followed:
/// the pair gets the plain-files note, and the file outside the tree is not read.
#[cfg(windows)]
#[test]
fn a_junction_above_the_file_is_never_followed() {
    let fx = text(&[(
        "app/db.properties",
        "url=jdbc:x\nnote=one\n",
        "url=jdbc:x\nnote=two\n",
    )]);
    let outside = tempfile::tempdir().unwrap();
    write(
        outside.path(),
        "db.properties",
        b"url=jdbc:x\nnote=outside-secret\n",
    );
    fs::rename(fx.left.path().join("app"), fx.left.path().join("app-moved")).unwrap();
    junction(&fx.left.path().join("app"), outside.path());
    let details = run(&fx, &FolderTextRules::default(), &opts());
    let info = &details.files["app/db.properties"];
    assert!(!info.counted);
    assert_eq!(
        info.note.as_deref(),
        Some("Only plain files are read: no lines are shown.")
    );
    assert!(!row_texts(info).iter().any(|t| t.contains("outside-secret")));
}
