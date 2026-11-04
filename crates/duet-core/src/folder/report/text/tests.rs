use super::*;
use crate::folder::report::ItemState;
use crate::folder::report::test_data::{context, db_item, item, options};

#[test]
fn the_text_report_follows_localdiff() {
    let (db, lines, _dirs) = db_item();
    let mut items = vec![db];
    for n in 0..4 {
        items.push(item(&format!("legacy/old{n}.cfg"), ItemState::LeftOnly));
    }
    let text = folder_text(&items, Some(&lines), &options(), &context());
    assert!(
        text.starts_with("Folder compare report \u{2014} Check\r\n"),
        "{text}"
    );
    assert!(text.contains("A = IWT : C:\\a\\iwt\r\n"));
    assert!(text.contains("B = PRD : C:\\a\\prd\r\n"));
    assert!(text.contains("  All files        5   (mismatched 5)\r\n"));
    assert!(text.contains("  \u{2260} Different      1\r\n"));
    assert!(text.contains("  < Only in A      4   (IWT)\r\n"));
    assert!(text.contains("  > Only in B      0   (PRD)\r\n"));
    assert!(text.contains("  = Same           0\r\n"));
    assert!(text.contains("Values that look like passwords or keys are hidden"));
    assert!(
        text.contains("      ~ A 3: password=\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022} (hidden #d397)\r\n"),
        "{text}"
    );
    assert!(!text.contains("Secr3t"));
    assert!(text.contains("[app]\r\n"));
    assert!(
        text.contains("  \u{2260} db.properties   \u{2014} Lines: 2 changed"),
        "{text}"
    );
    assert!(text.contains("[legacy]\r\n"));
    assert!(
        text.contains("  < old0.cfg   \u{2014} only in IWT\r\n"),
        "{text}"
    );
}

#[test]
fn the_text_report_covers_every_section_and_row_kind() {
    use crate::folder::classify::{Note, TextFormat, Warning};
    use crate::folder::report::lines::{DetailRow, LineDetails, LineInfo, RowClass};
    use crate::text::lines::EolStyle;
    let fmt = |eol| TextFormat {
        eol,
        final_newline: true,
        bom: false,
        encoding: encoding_rs::UTF_8,
    };
    let mut fmt_only = item("conf/run.sh", ItemState::Different);
    fmt_only.note = Some(Note {
        formats: Some([fmt(EolStyle::Lf), fmt(EolStyle::CrLf)]),
        shebang: true,
        soft: false,
        warning: Some(Warning::Script),
    });
    fmt_only.newer = Some(crate::folder::tree::Side::Right);
    let mut similar = item("docs/a.txt", ItemState::Same);
    similar.similar = true;
    similar.note = Some(Note {
        formats: None,
        shebang: false,
        soft: true,
        warning: None,
    });
    let plain_same = item("docs/b.txt", ItemState::Same);
    let top = item("Makefile", ItemState::RightOnly);
    let rows = vec![
        DetailRow::Line {
            class: RowClass::Soft,
            left: Some((1, "a ".into())),
            right: Some((1, "a".into())),
            marks: None,
        },
        DetailRow::Line {
            class: RowClass::Same,
            left: Some((2, "same".into())),
            right: Some((2, "same".into())),
            marks: None,
        },
        DetailRow::Line {
            class: RowClass::Del,
            left: Some((3, "gone".into())),
            right: None,
            marks: None,
        },
        DetailRow::Line {
            class: RowClass::Ins,
            left: None,
            right: Some((3, "new".into())),
            marks: None,
        },
        DetailRow::Skip(4),
        DetailRow::More(7),
    ];
    let mut lines = LineDetails::default();
    lines.files.insert(
        "conf/run.sh".into(),
        LineInfo {
            counted: true,
            rows,
            ..LineInfo::default()
        },
    );
    let items = vec![plain_same, similar, fmt_only, top];
    let mut o = options();
    o.same = true;
    let text = folder_text(&items, Some(&lines), &o, &context());
    for want in [
        "   (including 1 that differs only in unimportant parts)",
        "Needs a closer look\r\n  [Linux scripts with CRLF or a BOM] conf/run.sh\r\n",
        "[(top level)]\r\n  > Makefile   \u{2014} only in PRD\r\n",
        "PRD newer \u{b7} only the file format differs (LF (Linux), UTF-8 / CRLF (Windows), UTF-8)",
        "      \u{2248} A 1: a \r\n      \u{2248} B 1: a\r\n",
        "        A 2 | B 2: same\r\n",
        "      - A 3: gone\r\n",
        "      + B 3: new\r\n",
        "        \u{22ef} 4 same lines\r\n",
        "        \u{2026} 7 more rows\r\n",
        "  = b.txt\r\n",
        "Differs only in unimportant parts (counted as Same)\r\n  \u{2248} docs/a.txt   \u{2014} Differs only in unimportant parts by the text compare rules",
    ] {
        assert!(text.contains(want), "missing {want:?} in\n{text}");
    }
    assert!(
        !text.contains("  = a.txt"),
        "unimportant files are not listed as plain Same"
    );
    // Without details no line rows; without lines no mask note.
    o.details = false;
    let bare = folder_text(&items, None, &o, &context());
    assert!(!bare.contains("A 3: gone") && !bare.contains("are hidden"));
}

/// I-1 (PF12): the text report says when the invisible-character scan did not run, as the
/// HTML report and the workbook do.
#[test]
fn the_text_report_says_when_invisible_characters_were_not_scanned() {
    let items = [item("a.txt", ItemState::Different)];
    let mut ctx = context();
    ctx.invisible_scan = false;
    let text = folder_text(&items, None, &options(), &ctx);
    assert!(
        text.contains(
            "\r\nInvisible characters: not scanned (run the scan in the folder tab first)\r\n"
        ),
        "{text}"
    );
    let text = folder_text(&items, None, &options(), &context());
    assert!(!text.contains("Invisible characters:"), "{text}");
}

/// M-3: a note of several lines keeps the report's CRLF line ends.
#[test]
fn a_note_on_several_lines_keeps_crlf() {
    let mut o = options();
    o.note = "first\nsecond\r\nthird".to_owned();
    let text = folder_text(&[item("a.txt", ItemState::Different)], None, &o, &context());
    assert!(
        text.contains("\r\nfirst\r\nsecond\r\nthird\r\nA = IWT"),
        "{text:?}"
    );
    let bytes = text.as_bytes();
    for (at, _) in text.match_indices('\n') {
        assert!(
            at > 0 && bytes[at - 1] == b'\r',
            "a bare LF at {at} in {text:?}"
        );
    }
}
