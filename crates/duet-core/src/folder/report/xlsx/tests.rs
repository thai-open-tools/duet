use super::*;
use crate::cancel::CancelToken;
use crate::folder::classify::{Note, Warning};
use crate::folder::report::lines::{LineInfo, LineOptions, SHEET_ROWS, SheetKind, SheetRow};
use crate::folder::report::test_data::{context, item, options, read_zip};
use crate::folder::report::{Attention, ItemState, report_items};
use crate::folder::run::{CompareMode, FolderOptions, FolderProgress, FolderTextRules};
use chrono::NaiveDate;
use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicUsize;

const PARTS: [&str; 10] = [
    "[Content_Types].xml",
    "_rels/.rels",
    "docProps/core.xml",
    "docProps/app.xml",
    "xl/workbook.xml",
    "xl/_rels/workbook.xml.rels",
    "xl/styles.xml",
    "xl/worksheets/sheet1.xml",
    "xl/worksheets/sheet2.xml",
    "xl/worksheets/sheet3.xml",
];

/// The workbook's bytes: every test input is far below the 4 GiB limit.
fn workbook_bytes(
    items: &[ReportItem],
    lines: Option<&LineDetails>,
    options: &ReportOptions,
    context: &ReportContext,
) -> Vec<u8> {
    folder_xlsx(items, lines, options, context).unwrap()
}

fn at(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(y, m, d)
        .unwrap()
        .and_hms_opt(h, min, s)
        .unwrap()
}

/// Six files compared for real (LocalDiff's `rr:56-75` cases): two differing pairs with lines
/// (a password the masking hides, a control character and an NBSP), one file on each side
/// only, one that differs only in its line endings and one that is the same.
struct Sample {
    items: Vec<ReportItem>,
    lines: LineDetails,
    _dirs: [tempfile::TempDir; 2],
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn sample() -> Sample {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let files: [(&str, Option<&str>, Option<&str>); 6] = [
        (
            "app/db.properties",
            Some("url=jdbc:x\nuser=svc\npassword=Secr3t-A\ntimeout=30\n"),
            Some("url=jdbc:x\nuser=svc\npassword=Secr3t-B\ntimeout=60\n"),
        ),
        (
            "app/ctrl.txt",
            Some("a\u{7}b\nk = v\n"),
            Some("a\u{8}b\nk =\u{a0}v\n"),
        ),
        ("legacy/old0.cfg", Some("x0"), None),
        ("app/new-prd.txt", None, Some("n")),
        ("app/readme.txt", Some("a\r\nb\r\n"), Some("a\nb\n")),
        ("app/same.cfg", Some("s"), Some("s")),
    ];
    for (rel, left, right) in files {
        if let Some(text) = left {
            write(l.path(), rel, text);
        }
        if let Some(text) = right {
            write(r.path(), rel, text);
        }
    }
    let cmp = crate::folder::run::compare_folders(
        l.path(),
        r.path(),
        &FolderOptions {
            mode: CompareMode::Content,
            ..FolderOptions::default()
        },
        &FolderProgress::default(),
        &CancelToken::new(),
    )
    .unwrap();
    let nodes: Vec<u32> = (0..cmp.nodes.len() as u32)
        .filter(|&n| !cmp.nodes[n as usize].is_dir)
        .collect();
    let items = report_items(&cmp, &nodes, l.path(), r.path(), None);
    assert_eq!(items.len(), 6);
    let lines = crate::folder::report::lines::line_details(
        &items,
        &FolderTextRules::default(),
        &LineOptions {
            rows: false,
            sheet: true,
            ..LineOptions::default()
        },
        &AtomicUsize::new(0),
        &CancelToken::new(),
    );
    Sample {
        items,
        lines,
        _dirs: [l, r],
    }
}

fn book_options() -> ReportOptions {
    ReportOptions {
        title: "Check before deploy".to_owned(),
        ..options()
    }
}

/// Every part as text, in the archive's order.
fn parts(book: &[u8]) -> Vec<(String, String)> {
    read_zip(book)
        .into_iter()
        .map(|(name, bytes)| (name, String::from_utf8(bytes).unwrap()))
        .collect()
}

fn part<'a>(parts: &'a [(String, String)], name: &str) -> &'a str {
    &parts
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("no part {name}"))
        .1
}

fn sample_book(options: &ReportOptions) -> Vec<(String, String)> {
    let sample = sample();
    parts(&workbook_bytes(
        &sample.items,
        Some(&sample.lines),
        options,
        &context(),
    ))
}

fn column_index(letters: &str) -> usize {
    letters
        .bytes()
        .fold(0, |n, b| n * 26 + usize::from(b - b'A') + 1)
        - 1
}

/// The cells of each row, by column, as text: a number's `<v>` or an inline string's `<t>`.
fn rows(sheet: &str) -> Vec<Vec<String>> {
    let doc = roxmltree::Document::parse(sheet).unwrap();
    doc.descendants()
        .filter(|n| n.has_tag_name("row"))
        .map(|row| {
            let mut cells: Vec<String> = Vec::new();
            for c in row.children().filter(|n| n.has_tag_name("c")) {
                let r = c.attribute("r").unwrap();
                let col = column_index(r.trim_end_matches(|ch: char| ch.is_ascii_digit()));
                let text = c
                    .descendants()
                    .find(|n| n.has_tag_name("t") || n.has_tag_name("v"))
                    .and_then(|n| n.text())
                    .unwrap_or_default();
                if cells.len() <= col {
                    cells.resize(col + 1, String::new());
                }
                cells[col] = text.to_owned();
            }
            cells
        })
        .collect()
}

/// The first cell of a row ("" for a blank row).
fn first(row: &[String]) -> &str {
    row.first().map_or("", String::as_str)
}

fn row_starting<'a>(rows: &'a [Vec<String>], start: &str) -> &'a [String] {
    rows.iter()
        .find(|row| first(row) == start)
        .unwrap_or_else(|| panic!("no row starting {start:?}"))
}

#[test]
fn columns_are_named_like_excel() {
    for (index, name) in [
        (0, "A"),
        (2, "C"),
        (20, "U"),
        (25, "Z"),
        (26, "AA"),
        (27, "AB"),
        (51, "AZ"),
        (52, "BA"),
        (701, "ZZ"),
        (702, "AAA"),
    ] {
        assert_eq!(xl_col(index), name, "{index}");
    }
}

#[test]
fn excel_serials_count_local_days() {
    assert_eq!(excel_serial(at(2026, 10, 23, 12, 0, 0)), 46318.5);
    assert_eq!(excel_serial(at(1970, 1, 1, 0, 0, 0)), 25569.0);
    assert_eq!(
        format!("{:.6}", excel_serial(at(2026, 10, 23, 12, 0, 0))),
        "46318.500000"
    );
}

#[test]
fn xl_text_escapes_and_labels_bad_characters() {
    assert_eq!(xl_text("a<&>\""), "a&lt;&amp;&gt;&quot;");
    assert_eq!(xl_text("\u{7}"), "[U+0007]");
    assert_eq!(xl_text("\u{fffe}"), "[U+FFFE]");
    assert_eq!(
        xl_text("x\u{0}\u{b}\u{c}\u{1f}\u{ffff}y"),
        "x[U+0000][U+000B][U+000C][U+001F][U+FFFF]y"
    );
    // Tab, LF and CR are allowed in XML and stay as they are; a lone surrogate cannot occur in a &str.
    assert_eq!(xl_text("a\tb\nc\rd"), "a\tb\nc\rd");
    assert_eq!(xl_text("ไทย 'q'"), "ไทย 'q'");
    let long = "x".repeat(32_001);
    let cut = xl_text(&long);
    assert!(cut.starts_with(&"x".repeat(32_000)), "{}", cut.len());
    assert_eq!(&cut[32_000..], " …(cut to 32,000 characters)");
    assert_eq!(xl_text(&"x".repeat(32_000)), "x".repeat(32_000));
    // The labels come first, so a text of controls still stops at the cut.
    let bells = xl_text(&"\u{7}".repeat(5_000));
    assert_eq!(bells.len(), 32_000 + " …(cut to 32,000 characters)".len());
    assert!(bells.starts_with("[U+0007][U+0007]"));
    // Excel counts UTF-16 units: 16,001 emoji are 32,002 of them, and a pair is never split.
    let faces = xl_text(&"😀".repeat(16_001));
    assert_eq!(faces.matches('😀').count(), 16_000);
    assert!(faces.ends_with("😀 …(cut to 32,000 characters)"));
}

#[test]
fn odd_characters_become_visible_labels() {
    assert_eq!(xl_show_odd("k =\u{a0}v"), "k =[NBSP]v");
    assert_eq!(xl_show_odd("\u{200e}"), "[LRM]");
    // A space kind shows its code.
    assert_eq!(xl_show_odd("\u{2003}"), "[U+2003]");
    assert_eq!(xl_show_odd("a\u{7}b"), "a[U+0007]b");
    assert_eq!(xl_show_odd("plain text"), "plain text");
    assert_eq!(xl_show_odd("\u{feff}x\u{200b}"), "[BOM]x[ZWSP]");
}

#[test]
fn the_workbook_has_its_parts_and_they_are_well_formed() {
    let sample = sample();
    let book = workbook_bytes(
        &sample.items,
        Some(&sample.lines),
        &book_options(),
        &context(),
    );
    assert_eq!(
        book,
        workbook_bytes(
            &sample.items,
            Some(&sample.lines),
            &book_options(),
            &context()
        ),
        "the same input gives the same bytes"
    );
    let parts = parts(&book);
    let names: Vec<&str> = parts.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, PARTS);
    for (name, xml) in &parts {
        roxmltree::Document::parse(xml).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    let workbook = roxmltree::Document::parse(part(&parts, "xl/workbook.xml")).unwrap();
    let sheets: Vec<&str> = workbook
        .descendants()
        .filter(|n| n.has_tag_name("sheet"))
        .map(|n| n.attribute("name").unwrap())
        .collect();
    assert_eq!(sheets, ["Summary", "Files", "Changed lines"]);

    let types = part(&parts, "[Content_Types].xml");
    for n in 1..=3 {
        assert!(
            types.contains(&format!("PartName=\"/xl/worksheets/sheet{n}.xml\"")),
            "{types}"
        );
    }
    let rels = part(&parts, "xl/_rels/workbook.xml.rels");
    assert!(rels.contains("Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet3.xml\""), "{rels}");
    assert!(rels.contains("Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\""), "{rels}");
    // The export's clock, passed in: the document times and the ZIP times are fixed.
    let core = part(&parts, "docProps/core.xml");
    assert!(core.contains("<dc:creator>duet</dc:creator>"), "{core}");
    assert!(
        core.contains(
            "<dcterms:created xsi:type=\"dcterms:W3CDTF\">2026-10-24T02:30:00Z</dcterms:created>"
        ),
        "{core}"
    );
    assert!(
        core.contains(
            "<dcterms:modified xsi:type=\"dcterms:W3CDTF\">2026-10-24T02:30:00Z</dcterms:modified>"
        ),
        "{core}"
    );
    assert!(part(&parts, "docProps/app.xml").contains("<Application>duet</Application>"));
    assert_eq!(
        u16::from_le_bytes([book[10], book[11]]),
        (9 << 11) | (30 << 5)
    );
    // The first sheet is the selected one.
    assert!(part(&parts, "xl/worksheets/sheet1.xml").contains("tabSelected=\"1\""));
    assert!(!part(&parts, "xl/worksheets/sheet2.xml").contains("tabSelected"));
    // LocalDiff's styles, unchanged.
    let styles = part(&parts, "xl/styles.xml");
    assert!(styles.contains("<cellXfs count=\"21\">"), "{styles}");
    assert!(styles.contains("<fgColor rgb=\"FFFFF8DB\"/>"), "{styles}");
}

#[test]
fn the_files_sheet_filters_freezes_and_offers_review_results() {
    let parts = sample_book(&book_options());
    let files = part(&parts, "xl/worksheets/sheet2.xml");
    assert!(files.contains("<autoFilter ref=\"A1:U7\"/>"), "{files}");
    assert!(files.contains("xSplit=\"3\" ySplit=\"1\""), "{files}");
    assert!(files.contains("state=\"frozen\""), "{files}");
    let doc = roxmltree::Document::parse(files).unwrap();
    let check = doc
        .descendants()
        .find(|n| n.has_tag_name("dataValidation"))
        .unwrap();
    assert_eq!(check.attribute("type"), Some("list"));
    assert_eq!(check.attribute("sqref"), Some("T2:T7"));
    let list = check
        .children()
        .find(|n| n.has_tag_name("formula1"))
        .unwrap();
    assert_eq!(
        list.text(),
        Some("\"Passed,Needs a fix,To review,Not relevant\"")
    );
    // app/ctrl.txt sorts first and its lines start on row 2 of the Changed lines sheet.
    assert!(
        files.contains("<hyperlink ref=\"C2\" location=\"'Changed lines'!A2\" display=\"Go to the changed lines\"/>"),
        "{files}"
    );
    assert!(
        files.contains("<hyperlink ref=\"C3\" location=\"'Changed lines'!A4\""),
        "{files}"
    );
    assert_eq!(files.matches("<hyperlink ").count(), 2, "{files}");
    assert_eq!(files.matches("<row ").count(), 7);
    let table = rows(files);
    assert_eq!(table[0].len(), 21);
    assert_eq!(table[0][19], "Result");
    assert_eq!(&table[1][..5], ["1", "app", "ctrl.txt", "txt", "Different"]);
    let lines = rows(part(&parts, "xl/worksheets/sheet3.xml"));
    assert_eq!(lines[1][0], "app/ctrl.txt");
    assert_eq!(lines[3][0], "app/db.properties");
    // Reviewer columns are filled cells; a missing side has no size cell.
    assert!(
        files.contains("<c r=\"S2\" s=\"14\"/><c r=\"T2\" s=\"14\"/><c r=\"U2\" s=\"14\"/>"),
        "{files}"
    );
    let new_prd = row_starting(&table, "4");
    assert_eq!(new_prd[2], "new-prd.txt");
    assert_eq!((new_prd[12].as_str(), new_prd[13].as_str()), ("", "1"));
}

#[test]
fn modified_times_are_real_excel_dates() {
    let mut one = item("a.txt", ItemState::Different);
    one.left.as_mut().unwrap().modified = Some(at(2026, 10, 23, 12, 0, 0));
    let parts = parts(&workbook_bytes(&[one], None, &book_options(), &context()));
    let files = part(&parts, "xl/worksheets/sheet2.xml");
    assert!(
        files.contains("<c r=\"O2\" s=\"2\"><v>46318.500000</v></c>"),
        "{files}"
    );
    assert!(!files.contains("r=\"P2\""), "no time, no cell: {files}");
    // Sizes are numbers with the thousands format.
    assert!(
        files.contains("<c r=\"M2\" s=\"3\"><v>10</v></c><c r=\"N2\" s=\"3\"><v>20</v></c>"),
        "{files}"
    );
}

#[test]
fn the_summary_names_title_sides_and_counts() {
    let parts = sample_book(&book_options());
    let summary = part(&parts, "xl/worksheets/sheet1.xml");
    assert!(summary.contains("Check before deploy"));
    assert!(summary.contains("IWT"));
    assert!(summary.contains("Different"));
    let table = rows(summary);
    assert_eq!(table[0], ["Check before deploy"]);
    assert_eq!(
        table[1],
        ["Folder compare report — made with duet 9.9.9 on 2026-10-24 09:30"]
    );
    // Five files on each side: db 49, ctrl 10 / 11 (the NBSP is two bytes), old0 2, new-prd 1,
    // readme 6 / 4, same 1.
    assert_eq!(
        row_starting(&table, "A"),
        ["A", "IWT", "5", "68", "", "", "C:\\a\\iwt"]
    );
    assert_eq!(row_starting(&table, "B")[..4], ["B", "PRD", "5", "66"]);
    assert_eq!(row_starting(&table, "Scope")[1], "Selected files");
    assert_eq!(row_starting(&table, "Include / exclude")[1], "- / -");
    assert_eq!(
        row_starting(&table, "Line compare")[1],
        "Every character counts"
    );
    assert_eq!(
        row_starting(&table, "Hide passwords and keys")[1],
        "Hidden (•••••• + fingerprint: the same fingerprint means the same value) — 2 lines"
    );
    assert_eq!(
        row_starting(&table, "Different")[..3],
        ["Different", "2", "0.3333333333333333"]
    );
    assert_eq!(
        row_starting(&table, "Only in IWT")[..2],
        ["Only in IWT", "1"]
    );
    assert_eq!(
        row_starting(&table, "Only in PRD")[..2],
        ["Only in PRD", "1"]
    );
    assert_eq!(
        row_starting(&table, "Same")[..3],
        ["Same", "2", "0.3333333333333333"]
    );
    assert_eq!(row_starting(&table, "Total"), ["Total", "6", "1"]);
    assert_eq!(
        row_starting(&table, "Mismatched (different + one side only)"),
        [
            "Mismatched (different + one side only)",
            "4",
            "0.6666666666666666"
        ]
    );
    assert_eq!(
        row_starting(
            &table,
            "Of these, differ only in unimportant parts (counted as Same)"
        )[1],
        "1"
    );
    assert_eq!(
        row_starting(&table, "Differing lines in total (2 files read)"),
        ["Differing lines in total (2 files read)", "Lines"]
    );
    assert_eq!(row_starting(&table, "Changed"), ["Changed", "4"]);
    // The groups, the most mismatched first.
    assert_eq!(row_starting(&table, "Folder")[5], "Total");
    assert_eq!(
        row_starting(&table, "app"),
        ["app", "2", "0", "1", "2", "5"]
    );
    assert_eq!(
        row_starting(&table, "legacy"),
        ["legacy", "0", "1", "0", "0", "1"]
    );
    let how = table
        .iter()
        .position(|row| first(row) == "How to use")
        .unwrap();
    assert_eq!(table.len(), how + 6, "five bullets end the sheet");
    assert!(
        table[how + 3][0].starts_with("• A blue file name"),
        "{:?}",
        table[how + 3]
    );
    assert!(
        !table
            .iter()
            .any(|row| first(row) == "⚠ Needs a closer look")
    );
    assert!(!summary.contains("Invisible characters"));
}

#[test]
fn the_summary_lists_what_needs_a_closer_look() {
    let mut script = item("bin/run.sh", ItemState::Different);
    script.note = Some(Note {
        warning: Some(Warning::Script),
        ..Note::default()
    });
    let mut case = item("Docs/Readme.md", ItemState::Same);
    case.right.as_mut().unwrap().rel = "docs/readme.md".into();
    let mut locked = item("db.lock", ItemState::Different);
    locked.error = Some("Access is denied.".into());
    let items = [script, case, locked, item("z.txt", ItemState::Same)];
    let options = ReportOptions {
        mask: false,
        ..book_options()
    };
    let context = ReportContext {
        invisible_scan: false,
        ..context()
    };
    let parts = parts(&workbook_bytes(&items, None, &options, &context));
    let table = rows(part(&parts, "xl/worksheets/sheet1.xml"));
    assert_eq!(
        row_starting(&table, "⚠ Needs a closer look"),
        ["⚠ Needs a closer look", "Files", "Meaning"]
    );
    for kind in [
        Attention::Script,
        Attention::CaseOnly,
        Attention::Unreadable,
    ] {
        assert_eq!(
            row_starting(&table, kind.title()),
            [kind.title(), "1", kind.help()]
        );
    }
    assert!(
        !table
            .iter()
            .any(|row| first(row) == Attention::Encoding.title())
    );
    let hint = "Filter the “Needs a look” column on the “Files” sheet to see the names";
    assert_eq!(row_starting(&table, hint), [hint]);
    assert_eq!(row_starting(&table, "Line compare")[1], "Contents not read");
    assert_eq!(
        row_starting(&table, "Hide passwords and keys")[1],
        "Not hidden"
    );
    assert_eq!(
        row_starting(&table, "Invisible characters")[1],
        "Not scanned (run the scan in the folder tab first)"
    );
    // No lines read: no line totals, and three groups at the top level and below.
    assert!(
        !table
            .iter()
            .any(|row| first(row).starts_with("Differing lines"))
    );
    assert_eq!(
        row_starting(&table, "(top level)")[..3],
        ["(top level)", "1", "0"]
    );
    let how = table
        .iter()
        .position(|row| first(row) == "How to use")
        .unwrap();
    assert_eq!(
        table[how + 3][0],
        "• No “Changed lines” sheet was added (you can add it when you export)"
    );
}

#[test]
fn the_changed_lines_sheet_masks_and_marks() {
    let parts = sample_book(&book_options());
    let every: String = parts.iter().map(|(_, xml)| xml.as_str()).collect();
    assert!(!every.contains("Secr3t"));
    let sheet = part(&parts, "xl/worksheets/sheet3.xml");
    assert!(sheet.contains("(hidden #"), "{sheet}");
    assert!(sheet.contains("[U+0007]"), "{sheet}");
    assert!(sheet.contains("[NBSP]v"), "{sheet}");
    assert!(sheet.contains("<autoFilter ref=\"A1:G5\"/>"), "{sheet}");
    assert!(
        sheet.contains("xSplit=\"1\" ySplit=\"1\" topLeftCell=\"B2\""),
        "{sheet}"
    );
    let table = rows(sheet);
    assert_eq!(
        table[0],
        [
            "File", "Place", "Kind", "Line IWT", "Text IWT", "Line PRD", "Text PRD"
        ]
    );
    assert_eq!(
        table[1],
        [
            "app/ctrl.txt",
            "1",
            "Changed",
            "1",
            "a[U+0007]b",
            "1",
            "a[U+0008]b"
        ]
    );
    assert_eq!(table[2][4..], ["k = v", "2", "k =[NBSP]v"]);
    assert!(
        table[3][4].starts_with("password=•••••• (hidden #"),
        "{:?}",
        table[3]
    );
    assert_eq!(table[4][4], "timeout=30");
    assert_eq!(table.len(), 5);
    assert!(
        sheet.contains("<c r=\"E2\" s=\"15\" t=\"inlineStr\">"),
        "{sheet}"
    );
}

#[test]
fn the_kinds_have_their_names_and_colours_and_a_cut_is_said() {
    let row = |kind, left: Option<(u32, &str)>, right: Option<(u32, &str)>| SheetRow {
        kind,
        place: 1,
        left: left.map(|(n, s)| (n, s.to_owned())),
        right: right.map(|(n, s)| (n, s.to_owned())),
    };
    let mut lines = LineDetails::default();
    lines.files.insert(
        "a.txt".into(),
        LineInfo {
            counted: true,
            changed: 1,
            sheet: vec![
                row(SheetKind::LeftOnly, Some((3, "gone")), None),
                row(SheetKind::RightOnly, None, Some((4, ""))),
                row(SheetKind::Soft, Some((5, "x ")), Some((5, "x"))),
            ],
            ..LineInfo::default()
        },
    );
    lines.sheet_cut_files = 3;
    let items = [item("a.txt", ItemState::Different)];
    let parts = parts(&workbook_bytes(
        &items,
        Some(&lines),
        &book_options(),
        &context(),
    ));
    let sheet = part(&parts, "xl/worksheets/sheet3.xml");
    let table = rows(sheet);
    assert_eq!(table[1][..4], ["a.txt", "1", "Only in IWT", "3"]);
    assert_eq!(table[2][..3], ["a.txt", "1", "Only in PRD"]);
    assert_eq!(table[3][2], "Unimportant");
    assert!(
        sheet.contains("<c r=\"E2\" s=\"16\" t=\"inlineStr\">"),
        "{sheet}"
    );
    assert!(!sheet.contains("r=\"F2\""), "no line on the right: {sheet}");
    assert!(
        sheet.contains("<c r=\"F3\"><v>4</v></c><c r=\"G3\" s=\"17\"/>"),
        "an empty line keeps its colour: {sheet}"
    );
    assert!(
        sheet.contains("<c r=\"G4\" s=\"18\" t=\"inlineStr\">"),
        "{sheet}"
    );
    assert_eq!(
        table[4],
        ["… 3 more files have no lines here (over 100,000 rows)"]
    );
    assert!(
        sheet.contains("<c r=\"A5\" s=\"20\" t=\"inlineStr\">"),
        "{sheet}"
    );
    assert!(sheet.contains("<autoFilter ref=\"A1:G5\"/>"), "{sheet}");
    lines.sheet_cut_files = 1;
    let parts = self::parts(&workbook_bytes(
        &items,
        Some(&lines),
        &book_options(),
        &context(),
    ));
    let table = rows(part(&parts, "xl/worksheets/sheet3.xml"));
    assert_eq!(
        table[4],
        ["… 1 more file has no lines here (over 100,000 rows)"]
    );
    assert_eq!(SHEET_ROWS, 100_000);
}

#[test]
fn the_filters_are_named_for_excel() {
    let parts = sample_book(&book_options());
    let workbook = part(&parts, "xl/workbook.xml");
    assert!(workbook.contains("<definedNames><definedName name=\"_xlnm._FilterDatabase\" localSheetId=\"1\" hidden=\"1\">'Files'!$A$1:$U$7</definedName><definedName name=\"_xlnm._FilterDatabase\" localSheetId=\"2\" hidden=\"1\">'Changed lines'!$A$1:$G$5</definedName></definedNames>"), "{workbook}");
}

#[test]
fn no_lines_means_two_sheets() {
    let options = ReportOptions {
        details: false,
        ..book_options()
    };
    let parts = sample_book(&options);
    let names: Vec<&str> = parts.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, PARTS[..9]);
    let workbook = part(&parts, "xl/workbook.xml");
    assert!(workbook.contains("name=\"Files\""));
    assert!(!workbook.contains("Changed lines"), "{workbook}");
    assert!(!part(&parts, "[Content_Types].xml").contains("sheet3"));
    let files = part(&parts, "xl/worksheets/sheet2.xml");
    assert!(!files.contains("<hyperlink"), "{files}");
    // The line counts stay (PF5).
    assert_eq!(rows(files)[1][6], "2");
    let summary = rows(part(&parts, "xl/worksheets/sheet1.xml"));
    assert_eq!(row_starting(&summary, "Changed"), ["Changed", "4"]);
}

#[test]
fn names_and_notes_are_escaped_in_every_part() {
    let mut odd = item("app/<script>&\"x\".txt", ItemState::Different);
    odd.right.as_mut().unwrap().path = "C:\\b\\\u{7}bell.txt".into();
    let options = ReportOptions {
        names: ["A&B".to_owned(), "<PRD>".to_owned()],
        title: "\"T\" & <t>".to_owned(),
        note: "line one\nline two \u{1}".to_owned(),
        ..book_options()
    };
    let parts = parts(&workbook_bytes(&[odd], None, &options, &context()));
    for (name, xml) in &parts {
        roxmltree::Document::parse(xml).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    let files = part(&parts, "xl/worksheets/sheet2.xml");
    assert!(
        files.contains("&lt;script&gt;&amp;&quot;x&quot;.txt"),
        "{files}"
    );
    assert!(files.contains("C:\\b\\[U+0007]bell.txt"), "{files}");
    assert!(files.contains("Only in &lt;PRD&gt; (lines)"), "{files}");
    let summary = rows(part(&parts, "xl/worksheets/sheet1.xml"));
    assert_eq!(summary[0], ["\"T\" & <t>"]);
    assert_eq!(summary[2], ["line one\nline two [U+0001]"]);
    assert_eq!(row_starting(&summary, "A")[1], "A&B");
}

// ---- The workbook in Thai ----

#[path = "../../../../tests/support/report_text.rs"]
mod report_text;

const THAI_SHEETS: [&str; 3] = ["สรุป", "ไฟล์", "บรรทัดที่เปลี่ยน"];
const THAI_RESULTS: [&str; 4] = ["ผ่าน", "ต้องแก้", "รอตรวจ", "ไม่เกี่ยวข้อง"];

fn thai_options() -> ReportOptions {
    ReportOptions {
        lang: crate::i18n::Lang::Th,
        ..book_options()
    }
}

/// The context the app builds for a Thai report: its words are Thai too.
fn thai_context() -> ReportContext {
    let _th = crate::i18n::scoped(crate::i18n::Lang::Th);
    ReportContext {
        criterion: crate::folder::report::criterion_text(CompareMode::Content).to_owned(),
        scope: "ไฟล์ที่เลือก".to_owned(),
        ..context()
    }
}

/// A sample compared and read under Thai, with the workbook written from it, and the words of
/// the compare itself (names and paths) that the Thai report keeps.
fn thai_book() -> (Vec<(String, String)>, Vec<String>) {
    let _th = crate::i18n::scoped(crate::i18n::Lang::Th);
    let sample = sample();
    let book = parts(&workbook_bytes(
        &sample.items,
        Some(&sample.lines),
        &thai_options(),
        &thai_context(),
    ));
    let mut words: Vec<String> = [
        "IWT",
        "PRD",
        "Check before deploy",
        "9.9.9",
        "C:\\a\\iwt",
        "C:\\a\\prd",
        "url",
        "jdbc",
        "user",
        "svc",
        "Secr",
        "timeout",
    ]
    .map(str::to_owned)
    .to_vec();
    for item in &sample.items {
        words.push(item.rel.clone());
        words.extend(item.rel.split('/').map(str::to_owned));
        for side in [&item.left, &item.right].into_iter().flatten() {
            words.push(side.rel.clone());
            words.push(side.path.to_string_lossy().into_owned());
        }
    }
    (book, words)
}

fn utf16(text: &str) -> usize {
    text.encode_utf16().count()
}

fn sheet_names(parts: &[(String, String)]) -> Vec<String> {
    let workbook = roxmltree::Document::parse(part(parts, "xl/workbook.xml")).unwrap();
    workbook
        .descendants()
        .filter(|n| n.has_tag_name("sheet"))
        .map(|n| n.attribute("name").unwrap().to_owned())
        .collect()
}

#[test]
fn thai_sheet_names_fit_excel() {
    let (book, _) = thai_book();
    let names = sheet_names(&book);
    assert_eq!(names, THAI_SHEETS);
    for name in &names {
        assert!(utf16(name) <= 31, "{name}: Excel allows 31 characters");
        assert!(
            !name.contains(['[', ']', ':', '*', '?', '/', '\\', '\'']),
            "{name}: a character Excel refuses in a sheet name"
        );
        assert!(!name.is_empty());
    }
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "three different names");
}

#[test]
fn thai_links_and_the_filter_name_their_sheet() {
    let (book, _) = thai_book();
    let names = sheet_names(&book);
    assert_eq!(names, THAI_SHEETS, "the names the links must match");
    let files = roxmltree::Document::parse(part(&book, "xl/worksheets/sheet2.xml")).unwrap();
    let links: Vec<&str> = files
        .descendants()
        .filter(|n| n.has_tag_name("hyperlink"))
        .map(|n| n.attribute("location").unwrap())
        .collect();
    assert!(!links.is_empty());
    let lines_rows = rows(part(&book, "xl/worksheets/sheet3.xml")).len();
    for location in links {
        // The name is quoted as Excel requires, and it is a sheet that exists.
        let rest = location
            .strip_prefix(&format!("'{}'!A", names[2]))
            .unwrap_or_else(|| panic!("{location} names no sheet of {names:?}"));
        let row: usize = rest.parse().unwrap();
        assert!((2..=lines_rows).contains(&row), "{location}");
    }
    let workbook = roxmltree::Document::parse(part(&book, "xl/workbook.xml")).unwrap();
    let hidden: Vec<&str> = workbook
        .descendants()
        .filter(|n| n.has_tag_name("definedName"))
        .map(|n| n.text().unwrap())
        .collect();
    assert_eq!(hidden.len(), 2);
    for (text, name) in hidden.iter().zip(&names[1..]) {
        assert!(text.starts_with(&format!("'{name}'!$A$1:$")), "{text}");
    }
}

fn validation(book: &[(String, String)]) -> (String, String, String) {
    let files = roxmltree::Document::parse(part(book, "xl/worksheets/sheet2.xml")).unwrap();
    let one = files
        .descendants()
        .find(|n| n.has_tag_name("dataValidation"))
        .unwrap();
    let formula = one
        .children()
        .find(|n| n.has_tag_name("formula1"))
        .unwrap()
        .text()
        .unwrap()
        .to_owned();
    (
        formula,
        one.attribute("promptTitle").unwrap().to_owned(),
        one.attribute("prompt").unwrap().to_owned(),
    )
}

#[test]
fn the_thai_result_list_is_valid() {
    let (book, _) = thai_book();
    let (formula, _, _) = validation(&book);
    assert!(utf16(&formula) <= 255, "Excel's limit on a list formula");
    let inner = formula
        .strip_prefix('"')
        .and_then(|f| f.strip_suffix('"'))
        .expect("a quoted list");
    assert!(!inner.contains('"'));
    let values: Vec<&str> = inner.split(',').collect();
    assert_eq!(values, THAI_RESULTS, "four values, no comma inside one");
    assert!(values.iter().all(|v| !v.is_empty() && *v == v.trim()));
}

#[test]
fn thai_list_prompts_fit() {
    let (book, _) = thai_book();
    let (_, title, prompt) = validation(&book);
    assert!(utf16(&title) <= 32, "{title}");
    assert!(utf16(&prompt) <= 255, "{prompt}");
    assert_eq!(title, "ผลตรวจ", "the title is the column's name");
    assert_ne!(prompt, "Pick one from the list");
    let files = part(&book, "xl/worksheets/sheet2.xml");
    assert!(!files.contains("Go to the changed lines"), "{files}");
}

#[test]
fn the_thai_summary_names_its_own_sheet_and_column() {
    let (book, _) = thai_book();
    let summary = rows(part(&book, "xl/worksheets/sheet1.xml"));
    let text: Vec<&str> = summary.iter().flatten().map(String::as_str).collect();
    let all = text.join("\n");
    let column = "ควรตรวจดู";
    for name in [column, THAI_SHEETS[1], THAI_SHEETS[2]] {
        assert!(all.contains(name), "{name} in\n{all}");
    }
    // The note on the result column names the choices the team picks.
    let note = text
        .iter()
        .find(|cell| cell.contains("ผลตรวจ") && cell.contains("ผ่าน"))
        .expect("the note on the Result column");
    for value in THAI_RESULTS {
        assert!(note.contains(value), "{value} in {note}");
    }
    // The column it names is the Files sheet's own.
    let files = rows(part(&book, "xl/worksheets/sheet2.xml"));
    assert!(files[0].iter().any(|title| title == column));
    assert!(files[0].iter().any(|title| title == "ผลตรวจ"));
}

#[test]
fn thai_parts_are_well_formed_xml() {
    let (book, _) = thai_book();
    assert_eq!(book.len(), PARTS.len());
    for (name, xml) in &book {
        roxmltree::Document::parse(xml).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\""),
            "{name}"
        );
    }
    // Thai letters stay letters: they are not escaped as entities.
    let workbook = part(&book, "xl/workbook.xml");
    assert!(workbook.contains("name=\"บรรทัดที่เปลี่ยน\""), "{workbook}");
    assert_eq!(
        xl_text("ไฟล์ <a> & \"b\""),
        "ไฟล์ &lt;a&gt; &amp; &quot;b&quot;"
    );
}

#[test]
fn the_thai_workbook_carries_no_stray_english() {
    let (book, words) = thai_book();
    let strip: Vec<&str> = words.iter().map(String::as_str).collect();
    let allowed = [
        "duet",
        "CRLF",
        "LF",
        "CR",
        "BOM",
        "EOL",
        "NBSP",
        "LRM",
        "ZWSP",
        "Linux",
        "Windows",
        "config",
        "encoding",
        "path",
        "hex",
        "password",
        "conflict",
        "UTF",
        "MiB",
        "bash",
        "bad",
        "interpreter",
        "properties",
        "txt",
        "cfg",
        "env",
        "dat",
        "bin",
    ];
    for name in [
        "xl/workbook.xml",
        "xl/worksheets/sheet1.xml",
        "xl/worksheets/sheet2.xml",
        "xl/worksheets/sheet3.xml",
    ] {
        report_text::assert_no_stray_english(part(&book, name), &strip, &allowed);
    }
}

#[test]
fn a_workbook_without_a_language_is_english() {
    let parts = sample_book(&book_options());
    assert_eq!(sheet_names(&parts), ["Summary", "Files", "Changed lines"]);
    assert_eq!(crate::i18n::scoped_lang(), None);
}

#[test]
fn xlsx_note_cell_names_the_other_path() {
    let mut moved = item("a.txt", ItemState::LeftOnly);
    moved.moved_to = Some("new/a.txt".to_owned());
    let book = parts(&workbook_bytes(&[moved], None, &options(), &context()));
    let sheet = part(&book, "xl/worksheets/sheet2.xml");
    assert!(
        sheet.contains("Same contents as new/a.txt in PRD (moved or renamed)"),
        "{sheet}"
    );
}
