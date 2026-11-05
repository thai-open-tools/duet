use super::*;
use crate::folder::report::ItemState;
use crate::folder::report::test_data::{counted, db_item, item};
use chrono::NaiveDate;

fn names() -> [String; 2] {
    ["IWT".to_owned(), "PRD".to_owned()]
}

#[test]
fn csv_columns_name_the_sides() {
    let cols = csv_columns(&names());
    assert_eq!(cols[0], "No.");
    assert_eq!(cols[6], "Changed (lines)");
    assert!(cols.contains(&"Only in IWT (lines)".to_owned()));
    assert!(cols.contains(&"Only in PRD (lines)".to_owned()));
    assert!(cols.contains(&"Size IWT (bytes)".to_owned()));
    assert!(cols.contains(&"Modified PRD".to_owned()));
    assert!(cols.contains(&"Path IWT".to_owned()));
    assert_eq!(&cols[18..], ["Reviewer", "Result", "Reviewer note"]);
}

#[test]
fn csv_neutralises_formulas_and_quotes() {
    for (input, want) in [
        ("=SUM(1).txt", "'=SUM(1).txt"),
        ("-5", "-5"),
        ("-x", "'-x"),
        ("+1", "'+1"),
        ("@a", "'@a"),
        ("a,b", "\"a,b\""),
        ("say \"hi\"", "\"say \"\"hi\"\"\""),
        ("12.5", "12.5"),
        ("-12.50", "-12.50"),
        ("\tx", "'\tx"),
        ("line\nbreak", "\"line\nbreak\""),
        ("plain", "plain"),
    ] {
        assert_eq!(csv_cell_text(input), want, "{input:?}");
    }
}

#[test]
fn local_times_have_seconds() {
    let time = NaiveDate::from_ymd_opt(2026, 10, 23)
        .unwrap()
        .and_hms_opt(9, 5, 7)
        .unwrap();
    assert_eq!(local_time_text(time), "2026-10-23 09:05:07");
}

#[test]
fn the_csv_has_one_row_per_file_and_local_times() {
    use crate::folder::classify::{Note, Warning};
    let (mut db, mut lines, _dirs) = db_item();
    let when = NaiveDate::from_ymd_opt(2026, 10, 23)
        .unwrap()
        .and_hms_opt(9, 5, 7)
        .unwrap();
    db.left.as_mut().unwrap().modified = Some(when);
    let mut script = item("bin/run.sh", ItemState::Different);
    script.note = Some(Note {
        formats: None,
        shebang: true,
        soft: false,
        warning: Some(Warning::Script),
    });
    let mut case = item("Docs/Readme.md", ItemState::Same);
    case.right.as_mut().unwrap().rel = "docs/readme.md".into();
    let items = vec![
        db,
        item("=SUM(1).txt", ItemState::LeftOnly),
        script,
        case,
        item("zeta.txt", ItemState::RightOnly),
        item("same.txt", ItemState::Same),
    ];
    lines.files.insert("bin/run.sh".into(), counted(1));
    let csv = folder_csv(&items, Some(&lines), &names());
    assert!(csv.starts_with("\u{feff}No.,Folder,File name,"), "{csv}");
    assert!(csv.ends_with("\r\n"));
    let parts: Vec<&str> = csv.split("\r\n").collect();
    assert_eq!(parts.len(), 1 + 6 + 1, "{csv}");
    assert_eq!(parts[7], "");
    let db_row = parts
        .iter()
        .find(|row| row.contains("db.properties"))
        .unwrap();
    assert!(db_row.contains(",Different,,2,0,0,1,"), "{db_row}");
    assert!(db_row.contains(&local_time_text(when)), "{db_row}");
    assert!(csv.contains("'=SUM(1).txt"));
    assert!(csv.contains("Linux scripts with CRLF or a BOM"));
    assert!(csv.contains("File names that differ only in case"));
    assert!(csv.contains(",Only in IWT,"));
    assert!(csv.contains(",(top level),"));
}

#[test]
fn row_data_follows_localdiff() {
    let readme = item("docs/README.TXT", ItemState::Same);
    let d = row_data(&readme, 0, &names(), None);
    assert_eq!(
        (d.no, d.folder.as_str(), d.name.as_str(), d.ext.as_str()),
        (1, "docs", "README.TXT", "txt")
    );
    assert_eq!(d.state, "Same");
    assert_eq!(d.changed, None);
    assert_eq!(d.left_size, Some(10));
    let make = row_data(&item("Makefile", ItemState::Same), 4, &names(), None);
    assert_eq!(
        (make.no, make.folder.as_str(), make.ext.as_str()),
        (5, "(top level)", "")
    );
    let dot = row_data(&item(".gitignore", ItemState::Same), 0, &names(), None);
    assert_eq!(dot.ext, "");
    let mut unimportant = item("a.txt", ItemState::Same);
    unimportant.similar = true;
    let d = row_data(&unimportant, 0, &names(), None);
    assert_eq!(
        d.note,
        "Differs only in unimportant parts by the text compare rules"
    );
    // Lines looked at but not counted, and no reason given.
    let mut lines = crate::folder::report::lines::LineDetails::default();
    lines.files.insert("x.txt".into(), Default::default());
    let d = row_data(
        &item("x.txt", ItemState::Different),
        0,
        &names(),
        Some(&lines),
    );
    assert_eq!(d.note, "Contents not read");
    // A Same, not unimportant item with a newer side.
    let mut newer = item("n.txt", ItemState::Same);
    newer.newer = Some(crate::folder::tree::Side::Left);
    let d = row_data(&newer, 0, &names(), None);
    assert_eq!(d.note, "Same contents but different modified times");
    assert_eq!(d.newer, "IWT");
    let only = row_data(&item("o.txt", ItemState::LeftOnly), 0, &names(), None);
    assert_eq!(only.state, "Only in IWT");
    assert_eq!(only.right_path, "");
    assert_eq!(only.right_size, None);
}

fn moved_item(state: ItemState, rel: &str, other: &str) -> ReportItem {
    let mut it = item(rel, state);
    it.moved_to = Some(other.to_owned());
    it
}

#[test]
fn csv_note_names_the_other_path() {
    let left = moved_item(ItemState::LeftOnly, "a.txt", "new/a.txt");
    let right = moved_item(ItemState::RightOnly, "new/a.txt", "a.txt");
    assert_eq!(
        row_data(&left, 0, &names(), None).note,
        "Same contents as new/a.txt in PRD (moved or renamed)"
    );
    assert_eq!(
        row_data(&right, 1, &names(), None).note,
        "Same contents as a.txt in IWT (moved or renamed)"
    );
    let plain = item("a.txt", ItemState::LeftOnly);
    assert_eq!(row_data(&plain, 0, &names(), None).note, "");
}

#[test]
fn the_note_reads_in_thai_under_a_thai_report_scope() {
    let left = moved_item(ItemState::LeftOnly, "a.txt", "new/a.txt");
    let _th = crate::i18n::scoped(crate::i18n::Lang::Th);
    let note = row_data(&left, 0, &names(), None).note;
    assert_eq!(note, "เนื้อหาเดียวกับ new/a.txt ใน PRD (ย้ายหรือเปลี่ยนชื่อ)");
}
