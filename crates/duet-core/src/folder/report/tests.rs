use super::*;
use crate::cancel::CancelToken;
use crate::folder::classify::TextFormat;
use crate::folder::odd_scan::{OddProgress, scan_odd};
use crate::folder::run::{FolderOptions, FolderProgress, compare_folders};
use crate::folder::tree::Side;
use std::fs;

fn facts(rel: &str, len: u64) -> SideFacts {
    SideFacts {
        rel: rel.into(),
        path: PathBuf::from(rel),
        kind: EntryKind::File,
        len,
        modified: None,
    }
}

fn item(rel: &str, state: ItemState) -> ReportItem {
    ReportItem {
        node: 0,
        rel: rel.into(),
        state,
        left: (state != ItemState::RightOnly).then(|| facts(rel, 10)),
        right: (state != ItemState::LeftOnly).then(|| facts(rel, 20)),
        newer: None,
        error: None,
        note: None,
        similar: false,
        odd_text: [false; 2],
        whole_folder: None,
        moved_to: None,
    }
}

fn names() -> [String; 2] {
    ["IWT".to_owned(), "PRD".to_owned()]
}

fn fmt(
    eol: EolStyle,
    final_newline: bool,
    bom: bool,
    enc: &'static encoding_rs::Encoding,
) -> TextFormat {
    TextFormat {
        eol,
        final_newline,
        bom,
        encoding: enc,
    }
}

fn utf8(eol: EolStyle) -> TextFormat {
    fmt(eol, true, false, encoding_rs::UTF_8)
}

fn with_note(mut it: ReportItem, note: Note) -> ReportItem {
    it.note = Some(note);
    it
}

#[test]
fn report_items_take_files_and_their_verdicts() {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for root in [l.path(), r.path()] {
        fs::write(root.join("same.txt"), b"same\n").unwrap();
        fs::write(root.join("sim.txt"), b"abc\n").unwrap();
        fs::write(root.join("bad.txt"), b"abc\n").unwrap();
    }
    fs::write(l.path().join("odd.txt"), "x\u{a0}y\n").unwrap();
    fs::write(r.path().join("odd.txt"), "xy\n").unwrap();
    fs::create_dir(l.path().join("legacy")).unwrap();
    fs::write(l.path().join("legacy/old.txt"), b"old\n").unwrap();
    fs::write(l.path().join("top.txt"), b"top\n").unwrap();
    let mut cmp = compare_folders(
        l.path(),
        r.path(),
        &FolderOptions::default(),
        &FolderProgress::default(),
        &CancelToken::new(),
    )
    .unwrap();
    let at = |cmp: &FolderComparison, rel: &str| {
        cmp.nodes.iter().position(|n| n.rel == rel).unwrap() as u32
    };
    let (sim, bad) = (at(&cmp, "sim.txt") as usize, at(&cmp, "bad.txt") as usize);
    cmp.nodes[sim].status = Status::Similar {
        newer: Some(Side::Left),
    };
    cmp.nodes[sim].note = Some(Box::new(Note {
        soft: true,
        ..Note::default()
    }));
    cmp.nodes[bad].status = Status::Error("denied".into());
    let marks = scan_odd(
        &cmp,
        l.path(),
        r.path(),
        1,
        &OddProgress::default(),
        &CancelToken::new(),
    )
    .unwrap();

    let all: Vec<u32> = (0..cmp.nodes.len() as u32).collect();
    let items = report_items(&cmp, &all, l.path(), r.path(), Some(&marks));
    assert_eq!(items.len(), 6, "folders are not items");
    let get = |rel: &str| items.iter().find(|i| i.rel == rel).unwrap();

    let same = get("same.txt");
    assert_eq!(same.state, ItemState::Same);
    assert!(!same.unimportant());
    let similar = get("sim.txt");
    assert_eq!(similar.state, ItemState::Same);
    assert!(similar.similar && similar.unimportant());
    assert_eq!(similar.newer, Some(Side::Left));
    assert!(similar.note.as_ref().unwrap().soft);
    let broken = get("bad.txt");
    assert_eq!(broken.state, ItemState::Different);
    assert_eq!(broken.error.as_deref(), Some("denied"));

    let legacy = get("legacy/old.txt");
    assert_eq!(legacy.state, ItemState::LeftOnly);
    assert_eq!(legacy.whole_folder.as_deref(), Some("legacy"));
    assert!(legacy.right.is_none());
    assert_eq!(
        legacy.left.as_ref().unwrap().path,
        join_rel(l.path(), "legacy/old.txt")
    );
    assert_eq!(legacy.left.as_ref().unwrap().len, 4);
    assert!(legacy.left.as_ref().unwrap().modified.is_some());
    let top = get("top.txt");
    assert_eq!(top.state, ItemState::LeftOnly);
    assert_eq!(top.whole_folder, None);

    assert_eq!(get("odd.txt").odd_text, [true, false]);
    assert_eq!(same.odd_text, [false, false]);
    let without = report_items(&cmp, &all, l.path(), r.path(), None);
    assert!(without.iter().all(|i| i.odd_text == [false, false]));

    let one = report_items(&cmp, &[at(&cmp, "top.txt"), 9999], l.path(), r.path(), None);
    assert_eq!(one.len(), 1, "an unknown node is skipped");
}

#[test]
fn items_know_what_they_need() {
    let different = item("a.txt", ItemState::Different);
    assert!(different.needs_lines() && !different.format_only() && !different.case_only());
    assert!(!item("a.txt", ItemState::Same).needs_lines());
    assert!(!item("a.txt", ItemState::LeftOnly).needs_lines());
    let mut broken = item("a.txt", ItemState::Different);
    broken.error = Some("denied".into());
    assert!(!broken.needs_lines());

    let format = with_note(
        item("a.txt", ItemState::Different),
        Note {
            formats: Some([utf8(EolStyle::CrLf), utf8(EolStyle::Lf)]),
            ..Note::default()
        },
    );
    assert!(format.format_only() && !format.needs_lines());
    let soft = with_note(
        item("a.txt", ItemState::Different),
        Note {
            formats: Some([utf8(EolStyle::CrLf), utf8(EolStyle::Lf)]),
            soft: true,
            ..Note::default()
        },
    );
    assert!(!soft.format_only() && soft.needs_lines());
    let odd_space = with_note(
        item("a.txt", ItemState::Different),
        Note {
            formats: Some([utf8(EolStyle::Lf), utf8(EolStyle::Lf)]),
            warning: Some(Warning::OddSpace),
            ..Note::default()
        },
    );
    assert!(!odd_space.format_only() && odd_space.needs_lines());

    let case = with_note(
        item("a.txt", ItemState::Different),
        Note {
            warning: Some(Warning::CaseOnly),
            ..Note::default()
        },
    );
    assert!(case.case_only() && !case.needs_lines());
    assert!(!case.case_names_differ(), "same spelling on both sides");
    let mut renamed = item("app/Config.xml", ItemState::Different);
    renamed.right.as_mut().unwrap().rel = "app/config.xml".into();
    assert!(renamed.case_names_differ() && !renamed.case_only());
    assert!(
        renamed.needs_lines(),
        "a content difference in a renamed pair is still read"
    );
}

fn state_item(rel: &str, state: ItemState) -> ReportItem {
    item(rel, state)
}

#[test]
fn counts_and_groups_follow_localdiff() {
    use ItemState::{Different as D, LeftOnly as L, RightOnly as R, Same as S};
    let g = |folder: &str, d, l, r, s| Group {
        folder: folder.into(),
        different: d,
        left_only: l,
        right_only: r,
        same: s,
    };
    let items = |rows: &[(&str, ItemState)]| -> Vec<ReportItem> {
        rows.iter().map(|(rel, st)| state_item(rel, *st)).collect()
    };

    let first = items(&[("a/x", D), ("a/y", S), ("b/z", L), ("top", R)]);
    assert_eq!(
        top_groups(&first),
        vec![g("a", 1, 0, 0, 1), g("b", 0, 1, 0, 0), g("", 0, 0, 1, 0)]
    );
    assert_eq!(
        top_groups(&items(&[("app/a", D), ("app/b", S)])),
        vec![g("app", 1, 0, 0, 1)]
    );
    assert_eq!(
        top_groups(&items(&[("app/x/a", D), ("app/y/b", S), ("app/z/c", L)])),
        vec![
            g("app/x", 1, 0, 0, 0),
            g("app/y", 0, 0, 0, 1),
            g("app/z", 0, 1, 0, 0)
        ]
    );
    assert_eq!(
        top_groups(&items(&[("x", S), ("y", D)])),
        vec![g("", 1, 0, 0, 1)]
    );
    assert!(top_groups(&[]).is_empty());

    let counts = report_counts(&first);
    assert_eq!(
        (
            counts.total,
            counts.different,
            counts.left_only,
            counts.right_only,
            counts.same
        ),
        (4, 1, 1, 1, 1)
    );
    assert_eq!(counts.mismatched(), 3);
    // a/x and a/y have both sides, b/z the left only, top the right only
    assert_eq!((counts.left_files, counts.right_files), (3, 3));
    assert_eq!((counts.left_bytes, counts.right_bytes), (30, 60));
    assert_eq!(counts.unimportant, 0);

    let mut unimportant = state_item("u", S);
    unimportant.similar = true;
    assert_eq!(
        report_counts(&[unimportant, state_item("v", S)]).unimportant,
        1
    );
}

#[test]
fn percentages_follow_localdiff() {
    for (n, total, want) in [
        (1, 300, "<1%"),
        (0, 0, "0%"),
        (1, 3, "33%"),
        (2, 3, "67%"),
        (0, 5, "0%"),
        (5, 5, "100%"),
        (1, 200, "<1%"),
        (1, 100, "1%"),
        (7, 9, "78%"),
    ] {
        assert_eq!(percent(n, total), want, "{n}/{total}");
    }
}

#[test]
fn attention_lists_each_kind() {
    let n = names();
    let mut script = item("run.sh", ItemState::Different);
    script.note = Some(Note {
        warning: Some(Warning::Script),
        ..Note::default()
    });
    assert_eq!(attention(&script, &n), vec![(Attention::Script, None)]);

    let enc = with_note(
        item("t.txt", ItemState::Different),
        Note {
            formats: Some([
                fmt(EolStyle::Lf, true, false, encoding_rs::WINDOWS_874),
                fmt(EolStyle::Lf, true, false, encoding_rs::UTF_8),
            ]),
            ..Note::default()
        },
    );
    assert_eq!(
        attention(&enc, &n),
        vec![(Attention::Encoding, Some("windows-874 / UTF-8".to_owned()))]
    );

    let mut odd_space = with_note(
        item("c.cfg", ItemState::Different),
        Note {
            warning: Some(Warning::OddSpace),
            ..Note::default()
        },
    );
    odd_space.odd_text = [true, true];
    assert_eq!(attention(&odd_space, &n), vec![(Attention::OddSpace, None)]);

    let mut invisible = item("c.cfg", ItemState::Different);
    invisible.odd_text = [true, false];
    assert_eq!(
        attention(&invisible, &n),
        vec![(Attention::Invisible, Some("IWT".to_owned()))]
    );
    invisible.odd_text = [true, true];
    assert_eq!(
        attention(&invisible, &n),
        vec![(Attention::Invisible, Some("IWT, PRD".to_owned()))]
    );

    let mut case = item("app/Config.xml", ItemState::Different);
    case.right.as_mut().unwrap().rel = "app/config.xml".into();
    case.left.as_mut().unwrap().rel = "app/Config.xml".into();
    assert_eq!(
        attention(&case, &n),
        vec![(
            Attention::CaseOnly,
            Some("IWT: Config.xml · PRD: config.xml".to_owned())
        )]
    );

    let mut broken = item("x", ItemState::Different);
    broken.error = Some("denied".into());
    assert_eq!(attention(&broken, &n), vec![(Attention::Unreadable, None)]);

    assert!(attention(&item("fine", ItemState::Same), &n).is_empty());
    assert_eq!(Attention::ALL.len(), 6);
    assert_eq!(
        Attention::Script.title(),
        "Linux scripts with CRLF or a BOM"
    );
    assert_eq!(
        Attention::Script.help(),
        "They may not run on Linux (for example \"/bin/bash^M: bad interpreter\"). Convert them to LF without a BOM."
    );
    assert_eq!(Attention::Encoding.title(), "Encodings differ");
    assert_eq!(
        Attention::Encoding.help(),
        "The text matches but the encodings differ: a program that reads it may show Thai wrongly."
    );
    assert_eq!(
        Attention::OddSpace.title(),
        "Differs only in the kind of whitespace or invisible characters"
    );
    assert_eq!(
        Attention::OddSpace.help(),
        "They look the same but the values differ (for example NBSP for a space, LRM, zero-width): this often breaks a config value without anyone noticing."
    );
    assert_eq!(
        Attention::Invisible.title(),
        "Invisible characters in the text or values"
    );
    assert_eq!(
        Attention::Invisible.help(),
        "They may change a value without anyone noticing. Open the file in duet and use the button in the status bar to visit each one."
    );
    assert_eq!(
        Attention::CaseOnly.title(),
        "File names that differ only in case"
    );
    assert_eq!(
        Attention::CaseOnly.help(),
        "On Windows they are one file, on Linux two: a program that uses the old name may not find it."
    );
    assert_eq!(
        Attention::Unreadable.title(),
        "Files that could not be read"
    );
    assert_eq!(
        Attention::Unreadable.help(),
        "No permission, or the file is locked: the contents were not compared."
    );
}

#[test]
fn unimportant_reasons_name_what_differs() {
    let unimportant = |note: Note| {
        let mut it = with_note(item("a.txt", ItemState::Same), note);
        it.similar = true;
        it
    };
    let formats = |a: TextFormat, b: TextFormat| Note {
        formats: Some([a, b]),
        ..Note::default()
    };
    assert_eq!(
        unimportant_reason(&unimportant(formats(
            utf8(EolStyle::CrLf),
            utf8(EolStyle::Lf)
        )))
        .as_deref(),
        Some("Only the file format differs (EOL CRLF / LF) — every line matches")
    );
    assert_eq!(
        unimportant_reason(&unimportant(formats(
            fmt(EolStyle::Lf, false, false, encoding_rs::UTF_8),
            utf8(EolStyle::Lf)
        )))
        .as_deref(),
        Some("Only the file format differs (final newline) — every line matches")
    );
    assert_eq!(
        unimportant_reason(&unimportant(formats(
            fmt(EolStyle::Lf, true, true, encoding_rs::UTF_8),
            utf8(EolStyle::Lf)
        )))
        .as_deref(),
        Some("Only the file format differs (BOM) — every line matches")
    );
    let soft_equal = Note {
        soft: true,
        ..formats(utf8(EolStyle::Lf), utf8(EolStyle::Lf))
    };
    assert_eq!(
        unimportant_reason(&unimportant(soft_equal)).as_deref(),
        Some("Differs only in unimportant parts by the text compare rules")
    );
    let soft_format = Note {
        soft: true,
        ..formats(utf8(EolStyle::CrLf), utf8(EolStyle::Lf))
    };
    assert_eq!(
        unimportant_reason(&unimportant(soft_format)).as_deref(),
        Some("Differs only in unimportant parts by the text compare rules and the file format")
    );
    assert_eq!(
        unimportant_reason(&item("a.txt", ItemState::Different)),
        None
    );
    assert_eq!(unimportant_reason(&item("a.txt", ItemState::Same)), None);
    for (eol, label) in [
        (EolStyle::CrLf, "CRLF"),
        (EolStyle::Lf, "LF"),
        (EolStyle::Cr, "CR"),
        (EolStyle::Mixed, "mixed"),
        (EolStyle::None, "none"),
    ] {
        let note = formats(
            utf8(eol),
            utf8(if eol == EolStyle::Lf {
                EolStyle::Cr
            } else {
                EolStyle::Lf
            }),
        );
        let reason = unimportant_reason(&unimportant(note)).unwrap();
        assert!(reason.contains(&format!("EOL {label} / ")), "{reason}");
    }
}

#[test]
fn default_names_tell_same_named_folders_apart() {
    let names = |l: &str, r: &str| default_names(Path::new(l), Path::new(r));
    assert_eq!(
        names("C:/x/IWT_Config", "D:/y/PRD_Config"),
        ["IWT_Config".to_owned(), "PRD_Config".to_owned()]
    );
    assert_eq!(
        names("C:/old/config", "C:/new/config"),
        ["old/config".to_owned(), "new/config".to_owned()]
    );
    assert_eq!(
        names("C:/a/config", "D:/a/config"),
        ["config (left)".to_owned(), "config (right)".to_owned()]
    );
    assert_eq!(
        names("C:/a/Config", "D:/b/config"),
        ["a/Config".to_owned(), "b/config".to_owned()],
        "equal names are compared without case"
    );
}

#[test]
fn report_file_names_are_safe() {
    let date = NaiveDate::from_ymd_opt(2026, 10, 23).unwrap();
    assert_eq!(
        report_file_name(&["IWT Config".into(), "PRD:prod".into()], date, "html"),
        "folder-compare_IWT_Config_vs_PRD_prod_20261023.html"
    );
    let long = "x".repeat(50);
    let name = report_file_name(&[long.clone(), "b".into()], date, "csv");
    assert_eq!(
        name,
        format!("folder-compare_{}_vs_b_20261023.csv", "x".repeat(40))
    );
    assert_eq!(
        report_file_name(&["a \\ / ? * \" < > | b".into(), "c".into()], date, "txt"),
        "folder-compare_a_b_vs_c_20261023.txt"
    );
}

#[test]
fn items_sort_different_first_then_by_path() {
    use ItemState::{Different as D, LeftOnly as L, RightOnly as R, Same as S};
    let mut items: Vec<ReportItem> = [("b", S), ("A", D), ("c", L), ("a2", R), ("a1", D)]
        .iter()
        .map(|(rel, st)| state_item(rel, *st))
        .collect();
    sort_items(&mut items);
    let order: Vec<&str> = items.iter().map(|i| i.rel.as_str()).collect();
    assert_eq!(order, ["A", "a1", "c", "a2", "b"]);
}

#[test]
fn count_and_size_words_follow_the_app() {
    assert_eq!(count_text(1, "file", "files"), "1 file");
    assert_eq!(count_text(0, "file", "files"), "0 files");
    assert_eq!(count_text(1234, "file", "files"), "1,234 files");
    assert_eq!(human_bytes(64), "64 B");
    assert_eq!(human_bytes(1536), "1.5 KiB");
    assert_eq!(human_bytes(1_048_575), "1.0 MiB");
    assert_eq!(human_bytes(4 * 1024 * 1024 * 1024), "4.0 GiB");
}

#[test]
fn a_small_size_says_the_thai_unit_word_in_thai() {
    let _th = crate::i18n::scoped(crate::i18n::Lang::Th);
    assert_eq!(human_bytes(3), "3 ไบต์");
    assert_eq!(human_bytes(1536), "1.5 KiB");
}

#[test]
fn criterion_and_rules_texts() {
    assert_eq!(
        criterion_text(CompareMode::Content),
        "File contents (modified times not counted)"
    );
    assert_eq!(
        criterion_text(CompareMode::Rules),
        "Contents by the text compare rules"
    );
    assert_eq!(
        criterion_text(CompareMode::Size),
        "File size only (contents not read)"
    );
    assert_eq!(
        criterion_text(CompareMode::Quick),
        "Modified time and size (contents not read)"
    );
    let rules = FolderTextRules {
        whitespace: WhitespaceMode::All,
        ignore_case: true,
        text: vec![regex::Regex::new("a").unwrap()],
        lines: vec![regex::Regex::new("b").unwrap()],
        ..FolderTextRules::default()
    };
    assert_eq!(
        rules_text(&rules),
        "Ignore all whitespace, Ignore case, 2 unimportant-text rules"
    );
    assert_eq!(
        rules_text(&FolderTextRules::default()),
        "Every character counts"
    );
    let more = FolderTextRules {
        whitespace: WhitespaceMode::Trailing,
        ignore_blank: true,
        ignore_comments: true,
        text: vec![regex::Regex::new("a").unwrap()],
        ..FolderTextRules::default()
    };
    assert_eq!(
        rules_text(&more),
        "Ignore trailing whitespace, Ignore blank lines, Ignore comments, 1 unimportant-text rule"
    );
    for (mode, text) in [
        (WhitespaceMode::Leading, "Ignore leading whitespace"),
        (WhitespaceMode::Edges, "Ignore whitespace at both ends"),
        (WhitespaceMode::Amount, "Ignore the amount of whitespace"),
    ] {
        let one = FolderTextRules {
            whitespace: mode,
            ..FolderTextRules::default()
        };
        assert_eq!(rules_text(&one), text);
    }
}

/// PF9: a report is written whole or not at all. A cancel before the rename leaves the folder
/// as it was (the old file and no temp file); otherwise the new bytes replace the old file.
#[test]
fn a_report_is_written_whole_or_not_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("report.html");
    fs::write(&path, "old").unwrap();
    let entries = || fs::read_dir(dir.path()).unwrap().count();
    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(!write_atomic(&path, b"new", &cancel).unwrap());
    assert_eq!(fs::read_to_string(&path).unwrap(), "old");
    assert_eq!(entries(), 1, "no temp file is left");
    assert!(write_atomic(&path, b"new", &CancelToken::new()).unwrap());
    assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    assert_eq!(entries(), 1);
    let fresh = dir.path().join("fresh.csv");
    assert!(write_atomic(&fresh, b"a,b", &CancelToken::new()).unwrap());
    assert_eq!(fs::read(&fresh).unwrap(), b"a,b");
    assert!(
        write_atomic(&dir.path().join("missing/x.txt"), b"x", &CancelToken::new()).is_err(),
        "a folder that is not there is an error"
    );
    assert_eq!(entries(), 2);
}

/// C-1 (K3): with masking on, a private key's body reaches no format: not the HTML line
/// cells, the text report's rows, the CSV or any part of the workbook.
#[test]
fn a_private_key_never_reaches_a_report() {
    use super::test_data::{PEM_BODY, context, options, pem_item, read_zip};
    let (item, lines, _dirs) = pem_item();
    assert_eq!(lines.files["key.pem"].changed, 1);
    let items = [item];
    let (o, c) = (options(), context());
    let book = xlsx::folder_xlsx(&items, Some(&lines), &o, &c).unwrap();
    let parts: String = read_zip(&book)
        .into_iter()
        .map(|(_, bytes)| String::from_utf8(bytes).unwrap())
        .collect();
    let outputs = [
        ("HTML", html::folder_html(&items, Some(&lines), &o, &c)),
        ("text", text::folder_text(&items, Some(&lines), &o, &c)),
        ("CSV", csv::folder_csv(&items, Some(&lines), &o.names)),
        ("Excel", parts),
    ];
    for (format, out) in &outputs {
        for body in PEM_BODY {
            assert!(!out.contains(body), "{format} holds {body}");
        }
        // The CSV carries no file text; the sheet only the changed line.
        if *format != "CSV" {
            assert!(out.contains("(hidden #"), "{format}");
        }
        if matches!(*format, "HTML" | "text") {
            assert!(out.contains("-----BEGIN RSA PRIVATE KEY-----"), "{format}");
        }
    }
}
