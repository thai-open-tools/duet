use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::fixture::{Cell, SheetSpec, text, workbook, workbook_with_parts};
use super::*;

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn open(dir: &Path, name: &str, sheets: &[SheetSpec]) -> Workbook {
    Workbook::open(&write(dir, name, &workbook(sheets))).unwrap()
}

fn read(book: &mut Workbook, index: usize) -> SheetRead {
    book.read_sheet(index, &CancelToken::new(), &AtomicU64::new(0))
        .unwrap()
}

fn rows(t: &Table) -> Vec<Vec<String>> {
    (0..t.len())
        .map(|i| t.record(i).iter().map(str::to_string).collect())
        .collect()
}

/// The first record of one sheet holding `cells` on row 1.
fn row_of(cells: Vec<Cell>, date1904: bool) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    sheet.date1904 = date1904;
    for (i, c) in cells.into_iter().enumerate() {
        sheet.cells.push((1, i as u32 + 1, c));
    }
    let mut book = open(dir.path(), "a.xlsx", &[sheet]);
    rows(&read(&mut book, 0).table).remove(0)
}

fn num(v: f64) -> Cell {
    Cell::Num(v)
}

#[test]
fn cell_values_read_as_their_stored_text() {
    let got = row_of(
        vec![
            text("a b"),
            num(10.0),
            num(0.25),
            Cell::Percent(0.25),
            Cell::Money(1234.5),
            Cell::Bool(true),
            Cell::Err("#DIV/0!".into()),
            text("007"),
        ],
        false,
    );
    assert_eq!(
        got,
        [
            "a b", "10", "0.25", "0.25", "1234.5", "TRUE", "#DIV/0!", "007"
        ]
    );
}

#[test]
fn a_stored_number_ignores_its_format() {
    // 0.25 is 0.25 shown as 25% or as 0.25; neither side may look different for it.
    let got = row_of(
        vec![
            num(0.25),
            Cell::Percent(0.25),
            Cell::Money(0.25),
            Cell::Percent(1.0),
            num(1.0),
            num(-0.0),
            num(1e21),
            num(0.1),
        ],
        false,
    );
    assert_eq!(
        got,
        [
            "0.25",
            "0.25",
            "0.25",
            "1",
            "1",
            "0",
            "1000000000000000000000",
            "0.1"
        ]
    );
}

#[test]
fn text_and_other_kinds_with_the_same_characters_stay_different() {
    let got = row_of(
        vec![
            text("1"),
            num(1.0),
            text("TRUE"),
            Cell::Bool(true),
            text("#N/A"),
            Cell::Err("#N/A".into()),
            text("2024-03-05"),
            Cell::Date(45356.0),
            text("13:30:00"),
            Cell::Time(0.5625),
            text("1.5"),
            text("1.50"),
            text("'1"),
            text("''1"),
            text("'x"),
            text(" 1"),
        ],
        false,
    );
    assert_eq!(
        got,
        [
            "'1",
            "1",
            "'TRUE",
            "TRUE",
            "'#N/A",
            "#N/A",
            "'2024-03-05",
            "2024-03-05",
            "'13:30:00",
            "13:30:00",
            "'1.5",
            "1.50",
            "''1",
            "'''1",
            "'x",
            " 1"
        ]
    );
    // No two different cells above read alike.
    let mut seen = std::collections::HashSet::new();
    assert!(got.iter().all(|t| seen.insert(t.clone())));
}

#[test]
fn one_and_one_point_zero_read_the_same() {
    // The file holds a float either way; this is Excel's own value, so both read `1`.
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    sheet.cells.push((1, 1, num(1.0)));
    let xml = fixture::sheet_xml(&sheet);
    assert!(xml.contains("<v>1</v>"));
    let mut got = Vec::new();
    for (i, v) in ["1", "1.0", "1.000", "+1", "1E0", "0.1e1"]
        .iter()
        .enumerate()
    {
        let cut = xml.replace("<v>1</v>", &format!("<v>{v}</v>"));
        let bytes = workbook_with_parts(
            std::slice::from_ref(&sheet),
            &[("xl/worksheets/sheet1.xml", cut.as_bytes())],
        );
        let mut book = Workbook::open(&write(dir.path(), &format!("f{i}.xlsx"), &bytes)).unwrap();
        got.push(rows(&read(&mut book, 0).table));
    }
    assert!(got.iter().all(|r| r == &[["1"]]), "{got:?}");
}

#[test]
fn dates_agree_across_epochs_and_formats() {
    let cells = |serial_offset: f64| {
        vec![
            Cell::Date(45356.0 - serial_offset),
            Cell::DateAlt(45356.0 - serial_offset),
            Cell::Time(0.5625),
            Cell::Date(45356.25 - serial_offset),
        ]
    };
    let nineteen = row_of(cells(0.0), false);
    let nineteen04 = row_of(cells(1462.0), true);
    assert_eq!(
        nineteen,
        [
            "2024-03-05",
            "2024-03-05",
            "13:30:00",
            "2024-03-05 06:00:00"
        ]
    );
    assert_eq!(nineteen, nineteen04, "1900 and 1904 workbooks agree");
    // A serial in a General cell is the number, and differs from the date.
    assert_eq!(row_of(vec![num(45356.0)], false), ["45356"]);
    // Milliseconds show; a leap day Excel invented stays.
    assert_eq!(
        row_of(
            vec![Cell::Date(45356.0 + 1.0 / 86_400_000.0 * 250.0)],
            false
        ),
        ["2024-03-05 00:00:00.250"]
    );
    assert_eq!(row_of(vec![Cell::Date(60.0)], false), ["1900-02-29"]);
}

#[test]
fn a_date_outside_excels_range_stays_the_number() {
    assert_eq!(
        row_of(vec![Cell::Date(-5.0), Cell::Date(3_000_000.0)], false),
        ["-5", "3000000"]
    );
}

#[test]
fn a_duration_reads_with_total_hours() {
    assert_eq!(
        row_of(
            vec![Cell::Duration(26.5 / 24.0), Cell::Duration(0.0)],
            false
        ),
        ["26:30:00", "0:00:00"]
    );
}

#[test]
fn formulas_compare_by_saved_value() {
    let f = |value| Cell::Formula {
        text: "A1+1".into(),
        value,
    };
    assert_eq!(
        row_of(
            vec![
                f(Some(Box::new(num(3.0)))),
                f(Some(Box::new(text("x")))),
                f(Some(Box::new(Cell::Date(45356.0)))),
                f(Some(Box::new(Cell::Err("#REF!".into())))),
            ],
            false
        ),
        ["3", "x", "2024-03-05", "#REF!"]
    );
}

#[test]
fn formulas_without_a_saved_result_are_counted() {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    for col in 1..=2 {
        sheet.cells.push((
            1,
            col,
            Cell::Formula {
                text: "1+1".into(),
                value: None,
            },
        ));
    }
    sheet.cells.push((1, 3, text("x")));
    let mut book = open(dir.path(), "a.xlsx", &[sheet]);
    let read = read(&mut book, 0);
    assert_eq!(read.formulas_without_result, 2);
    assert_eq!(rows(&read.table), [["", "", "x"]]);
}

#[test]
fn empty_cells_and_empty_strings_read_alike() {
    // Excel shows them alike; both are "" and neither makes a row.
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    sheet.cells.push((1, 1, text("a")));
    sheet.cells.push((1, 2, Cell::EmptyText));
    sheet.cells.push((1, 4, text("d")));
    sheet.cells.push((1, 5, Cell::EmptyText));
    sheet.cells.push((2, 1, Cell::EmptyText));
    sheet.cells.push((3, 1, Cell::StyledEmpty));
    sheet.cells.push((4, 2, text("z")));
    let mut book = open(dir.path(), "a.xlsx", &[sheet]);
    let read = read(&mut book, 0);
    assert_eq!(rows(&read.table), [vec!["a", "", "", "d"], vec!["", "z"]]);
    assert_eq!((read.table.line(0), read.table.line(1)), (1, 4));
}

#[test]
fn empty_rows_are_skipped_and_lines_keep_sheet_rows() {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    for r in [1, 2, 5] {
        sheet.cells.push((r, 1, text("x")));
    }
    let mut book = open(dir.path(), "a.xlsx", &[sheet]);
    let t = read(&mut book, 0).table;
    assert_eq!(t.len(), 3);
    assert_eq!((0..3).map(|i| t.line(i)).collect::<Vec<_>>(), [1, 2, 5]);
    assert_eq!(t.record_at_line(5), Some(2));
}

#[test]
fn leading_and_inner_gaps_pad_and_trailing_are_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    sheet.cells.push((1, 3, text("x")));
    sheet.cells.push((1, 5, text("y")));
    sheet.cells.push((1, 7, Cell::StyledEmpty));
    let mut book = open(dir.path(), "a.xlsx", &[sheet]);
    assert_eq!(rows(&read(&mut book, 0).table), [["", "", "x", "", "y"]]);
}

#[test]
fn merged_cells_hold_their_value_in_the_corner_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    sheet.cells.push((1, 1, text("m")));
    sheet.cells.push((3, 1, text("below")));
    sheet.merges.push("A1:C2".into());
    let mut book = open(dir.path(), "a.xlsx", &[sheet]);
    assert_eq!(rows(&read(&mut book, 0).table), [vec!["m"], vec!["below"]]);
}

#[test]
fn hidden_sheets_are_listed_and_charts_are_not() {
    let dir = tempfile::tempdir().unwrap();
    let mut hidden = SheetSpec::new("Hidden");
    hidden.hidden = true;
    let mut chart = SheetSpec::new("Chart");
    chart.chart = true;
    let book = open(
        dir.path(),
        "a.xlsx",
        &[
            SheetSpec::new("Sheet1"),
            hidden,
            chart,
            SheetSpec::new("Data"),
        ],
    );
    let listed: Vec<_> = book
        .sheets()
        .iter()
        .map(|s| (s.name.as_str(), s.hidden))
        .collect();
    assert_eq!(
        listed,
        [("Sheet1", false), ("Hidden", true), ("Data", false)]
    );
}

fn entries(names: &[(&str, bool)]) -> Vec<SheetEntry> {
    names
        .iter()
        .map(|(n, h)| SheetEntry {
            name: (*n).into(),
            hidden: *h,
        })
        .collect()
}

#[test]
fn default_sheets_prefer_a_shared_name() {
    let abc = entries(&[("A", false), ("B", false), ("C", false)]);
    let xbc = entries(&[("X", false), ("B", false), ("C", false)]);
    assert_eq!(default_sheets(&abc, &xbc), (1, 1));
    assert_eq!(default_sheets(&abc, &entries(&[("Q", false)])), (0, 0));
    let hidden_first = entries(&[("A", true), ("B", false)]);
    assert_eq!(default_sheets(&hidden_first, &hidden_first), (1, 1));
    assert_eq!(default_sheet(&entries(&[("A", true), ("B", true)])), 0);
    assert_eq!(default_sheet(&hidden_first), 1);
    // A hidden sheet of the same name is not a match.
    let right = entries(&[("A", false), ("Z", false)]);
    assert_eq!(
        default_sheets(&entries(&[("Z", true), ("A", false)]), &right),
        (1, 0)
    );
}

#[test]
fn default_sheets_match_names_case_insensitively() {
    let left = entries(&[("Sales", false)]);
    let right = entries(&[("x", false), ("SALES", false)]);
    assert_eq!(default_sheets(&left, &right), (0, 1));
}

#[test]
fn a_far_corner_cell_is_read_sparse_not_allocated() {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    sheet.cells.push((1, 1, text("a")));
    sheet.cells.push((1_048_576, 16_384, text("z")));
    let mut book = open(dir.path(), "a.xlsx", &[sheet]);
    let read = read(&mut book, 0);
    assert_eq!(read.table.len(), 2);
    assert_eq!(read.table.line(1), 1_048_576);
    assert_eq!(read.table.width(), 16_384);
    assert_eq!(read.cut_at_row, None);
}

#[test]
fn a_far_corner_cell_on_many_rows_is_cut_by_the_budget() {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    for r in 1..=1000 {
        sheet.cells.push((r, 16_384, text("z")));
    }
    let mut book = open(dir.path(), "a.xlsx", &[sheet]);
    let read = read(&mut book, 0);
    let cut = read.cut_at_row.expect("the budget cuts the sheet");
    assert!(read.table.len() < 1000);
    assert_eq!(read.table.len() as u32 + 1, cut, "rows 1.. up to the cut");
    let cells: usize = (0..read.table.len())
        .map(|i| read.table.record(i).len())
        .sum();
    assert!(cells <= MAX_CELLS);
}

#[test]
fn a_cancelled_read_stops() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = open(
        dir.path(),
        "a.xlsx",
        &[SheetSpec::text_rows("S", &[&["a"]])],
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    let got = book.read_sheet(0, &cancel, &AtomicU64::new(0));
    assert!(matches!(got, Err(ReadFail::Cancelled)));
}

#[test]
fn progress_counts_rows() {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    for r in 1..=10_000 {
        sheet.cells.push((r, 1, text("x")));
    }
    let mut book = open(dir.path(), "a.xlsx", &[sheet]);
    let done = AtomicU64::new(0);
    let read = book.read_sheet(0, &CancelToken::new(), &done).unwrap();
    assert_eq!(read.table.len(), 10_000);
    assert_eq!(done.load(Ordering::Relaxed), 10_000);
}

use super::fixture::protected_bytes;

fn open_err(dir: &Path, name: &str, bytes: &[u8]) -> OpenError {
    match Workbook::open(&write(dir, name, bytes)) {
        Err(e) => e,
        Ok(_) => panic!("{name} should be refused"),
    }
}

#[test]
fn a_protected_file_is_named_by_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        open_err(dir.path(), "locked.xlsx", &protected_bytes()),
        OpenError::Password
    ));
    // The signature without an encrypted package: an old .xls.
    let mut old = vec![0u8; 4096];
    old[..8].copy_from_slice(&OLE_SIGNATURE);
    assert!(matches!(
        open_err(dir.path(), "old.xlsx", &old),
        OpenError::OldXls
    ));
}

#[test]
fn other_files_are_refused_by_kind() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    assert!(matches!(
        open_err(p, "plain.xlsx", b"id,name\n1,Ann\n"),
        OpenError::NotAZip
    ));
    assert!(matches!(open_err(p, "empty.xlsx", b""), OpenError::NotAZip));
    let not_book = fixture::zip(&[("readme.txt".into(), b"hi".to_vec())]);
    assert!(matches!(
        open_err(p, "book.xlsx", &not_book),
        OpenError::NotAWorkbook
    ));
    let good = workbook(&[SheetSpec::text_rows("S", &[&["a"]])]);
    assert!(matches!(
        open_err(p, "cut.xlsx", &good[..good.len() / 2]),
        OpenError::Damaged(_)
    ));
    assert!(matches!(
        Workbook::open(&p.join("missing.xlsx")),
        Err(OpenError::Io(_))
    ));
    assert!(matches!(
        open_err(p, "none.xlsx", &workbook(&[])),
        OpenError::NoSheets
    ));
    let mut only_chart = SheetSpec::new("C");
    only_chart.chart = true;
    assert!(matches!(
        open_err(p, "chart.xlsx", &workbook(&[only_chart])),
        OpenError::NoSheets
    ));
}

#[test]
fn a_sheet_cut_in_half_is_damaged_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut spec = SheetSpec::new("S");
    for r in 1..=50 {
        spec.cells.push((r, 1, text("x")));
    }
    let xml = fixture::sheet_xml(&spec);
    let cut = &xml.as_bytes()[..xml.len() / 2];
    let bytes = workbook_with_parts(&[spec], &[("xl/worksheets/sheet1.xml", cut)]);
    let mut book = Workbook::open(&write(dir.path(), "a.xlsx", &bytes)).unwrap();
    let got = book.read_sheet(0, &CancelToken::new(), &AtomicU64::new(0));
    assert!(matches!(got, Err(ReadFail::Damaged(_))), "{got:?}");
}

#[test]
fn a_macro_workbook_opens_as_data() {
    let dir = tempfile::tempdir().unwrap();
    let sheet = SheetSpec::text_rows("S", &[&["a", "b"]]);
    let bytes = workbook_with_parts(&[sheet], &[("xl/vbaProject.bin", b"not really vba")]);
    let mut book = Workbook::open(&write(dir.path(), "m.xlsm", &bytes)).unwrap();
    assert_eq!(rows(&read(&mut book, 0).table), [["a", "b"]]);
    assert!(is_workbook_name(Path::new("m.xlsm")));
}

#[test]
fn a_panic_in_the_reader_becomes_a_message() {
    let got = guarded(|| -> u8 { panic!("boom") });
    assert!(got.unwrap_err().contains("boom"));
    assert_eq!(guarded(|| 7), Ok(7));
}

#[test]
fn names_are_matched_by_extension() {
    for yes in ["A.XLSX", "b.xlsm", "dir/c.Xlsx"] {
        assert!(is_workbook_name(Path::new(yes)), "{yes}");
    }
    for no in ["c.xls", "d.xlsb", "e.xlsx.bak", "xlsx"] {
        assert!(!is_workbook_name(Path::new(no)), "{no}");
    }
    assert!(is_csv_name(Path::new("x.TSV")));
    assert!(is_csv_name(Path::new("x.csv")));
    assert!(is_csv_name(Path::new("x.tab")));
    assert!(!is_csv_name(Path::new("x.txt")));
}

#[test]
fn a_real_sheet_reads_the_second_sheet_by_index() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = open(
        dir.path(),
        "a.xlsx",
        &[
            SheetSpec::text_rows("One", &[&["1"]]),
            SheetSpec::text_rows("Two", &[&["2", "x"]]),
        ],
    );
    assert_eq!(
        rows(&read(&mut book, 1).table),
        [["'2".to_string(), "x".to_string()]]
    );
    assert_eq!(rows(&read(&mut book, 0).table), [["'1"]]);
}

#[test]
fn the_text_marker_can_be_left_off_for_a_csv_partner() {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    sheet.cells.push((1, 1, text("1")));
    sheet.cells.push((1, 2, num(1.0)));
    sheet.cells.push((1, 3, text("TRUE")));
    sheet.cells.push((1, 4, text("abc")));
    let mut book = open(dir.path(), "a.xlsx", &[sheet]);
    let cancel = CancelToken::new();
    let done = AtomicU64::new(0);
    let marked = book.read_sheet_with(0, &cancel, &done, true).unwrap();
    assert_eq!(rows(&marked.table), [["'1", "1", "'TRUE", "abc"]]);
    let plain = book.read_sheet_with(0, &cancel, &done, false).unwrap();
    assert_eq!(rows(&plain.table), [["1", "1", "TRUE", "abc"]]);
    let default = read(&mut book, 0);
    assert_eq!(
        rows(&default.table),
        rows(&marked.table),
        "marked by default"
    );
}

// ---- Text that is not ASCII ----

#[test]
fn text_whose_fifth_byte_is_inside_a_character_reads_as_it_is() {
    // `is_date` looked at bytes 4 and 7 of every text cell of a workbook pair.
    let cells = [
        "สวัสดีครับ",
        "ชื่อลูกค้า",
        "René Dupont",
        "1234é-56-78",
        "١٢٣٤-٠٣-٠٥ 12",
    ];
    let got = row_of(cells.iter().map(|c| text(c)).collect(), false);
    assert_eq!(got, cells);
}

// ---- Numbers read at Excel's own precision ----

#[test]
fn float_noise_excel_does_not_show_is_not_a_difference() {
    // =0.1+0.2 against a typed 0.3: Excel shows 0.3 for both and =A1=B1 is TRUE.
    assert_eq!(
        row_of(vec![num(0.1 + 0.2), num(0.3)], false),
        ["0.3", "0.3"]
    );
    assert_eq!(
        row_of(
            vec![num(1.0 - 0.9), num(1.1 * 1.1), num(-(0.1 + 0.2))],
            false
        ),
        ["0.1", "1.21", "-0.3"]
    );
}

#[test]
fn a_difference_within_fifteen_digits_stays_different() {
    assert_eq!(
        row_of(
            vec![
                num(0.300000000000001),
                num(123456789012345.0),
                num(12345678901234.5),
                num(1e21),
                num(1.5e-7),
            ],
            false
        ),
        [
            "0.300000000000001",
            "123456789012345",
            "12345678901234.5",
            "1000000000000000000000",
            "0.00000015",
        ]
    );
    // Past fifteen digits a number reads as Excel shows it.
    assert_eq!(
        row_of(vec![num(1234567890123456789.0)], false),
        ["1234567890123460000"]
    );
}

// ---- Width by rows ----

#[test]
fn rows_by_width_are_budgeted() {
    // One stray cell at XFD1 makes every row of the sheet 16,384 columns wide to the compare.
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    sheet.cells.push((1, 16_384, text("stray")));
    for r in 2..=5000 {
        sheet.cells.push((r, 1, text("x")));
    }
    let mut book = open(dir.path(), "a.xlsx", &[sheet]);
    let read = read(&mut book, 0);
    let cut = read.cut_at_row.expect("the sheet is cut");
    assert_eq!(read.table.len() as u32 + 1, cut);
    assert!(read.table.len() * read.table.width() <= MAX_GRID_CELLS);
    assert!(read.table.len() + 1 > MAX_GRID_CELLS / 16_384);
}

#[test]
fn a_wide_row_late_in_the_sheet_counts_for_the_rows_before_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    for r in 1..=5000 {
        sheet.cells.push((r, 1, text("x")));
    }
    sheet.cells.push((5000, 16_384, text("stray")));
    let mut book = open(dir.path(), "a.xlsx", &[sheet]);
    let read = read(&mut book, 0);
    assert_eq!(
        read.cut_at_row,
        Some(5000),
        "the row that would pass the budget"
    );
    assert_eq!(read.table.len(), 4999);
    assert_eq!(read.table.width(), 1);
}

// ---- Dates at the edge of a day ----

#[test]
fn a_time_that_rounds_to_midnight_is_the_next_day() {
    assert_eq!(
        row_of(
            vec![
                Cell::Date(45356.99999999999),
                Cell::Date(45356.999999),
                Cell::Time(0.99999999999),
            ],
            false
        ),
        ["2024-03-06", "2024-03-05 23:59:59.914", "00:00:00"]
    );
    // In the 1904 system too, and the last day of Excel's range stays the number it was.
    assert_eq!(
        row_of(vec![Cell::Date(45356.99999999999 - 1462.0)], true),
        ["2024-03-06"]
    );
    assert_eq!(
        row_of(vec![Cell::Date(2958465.9999999995)], false),
        ["2958466"]
    );
}

#[test]
fn an_iso_date_cell_reads_like_a_serial_date() {
    assert_eq!(
        row_of(
            vec![
                Cell::IsoDate("2024-03-05T00:00:00".into()),
                Cell::IsoDate("2024-03-05T00:00:00.000".into()),
                Cell::IsoDate("2024-03-05T13:30:00".into()),
                Cell::IsoDate("2024-03-05T13:30:00.000".into()),
                Cell::IsoDate("2024-03-05T13:30:00.250".into()),
                Cell::IsoDate("2024-03-05".into()),
                Cell::Date(45356.0),
            ],
            false
        ),
        [
            "2024-03-05",
            "2024-03-05",
            "2024-03-05 13:30:00",
            "2024-03-05 13:30:00",
            "2024-03-05 13:30:00.250",
            "2024-03-05",
            "2024-03-05",
        ]
    );
}

// ---- The marker ----

#[test]
fn a_long_run_of_apostrophes_does_not_use_up_the_stack() {
    let long = format!("{}1", "'".repeat(300_000));
    let got = row_of(vec![text(&long), text(&"'".repeat(300_000))], false);
    assert_eq!(got[0], format!("'{long}"));
    assert_eq!(
        got[1],
        "'".repeat(300_000),
        "nothing it could be mistaken for"
    );
}

#[test]
fn a_read_counts_the_cells_it_marked() {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    sheet.cells.push((1, 1, text("1")));
    sheet.cells.push((1, 2, text("'1")));
    sheet.cells.push((1, 3, text("'x")));
    sheet.cells.push((1, 4, num(1.0)));
    sheet.cells.push((1, 5, text("TRUE")));
    let mut book = open(dir.path(), "a.xlsx", &[sheet]);
    let (cancel, done) = (CancelToken::new(), AtomicU64::new(0));
    let marked = book.read_sheet_with(0, &cancel, &done, true).unwrap();
    assert_eq!(rows(&marked.table), [["'1", "''1", "'x", "1", "'TRUE"]]);
    assert_eq!(marked.marked_cells, 3);
    assert!(marked.table.text_marked());
    let plain = book.read_sheet_with(0, &cancel, &done, false).unwrap();
    assert_eq!(plain.marked_cells, 0);
    assert!(!plain.table.text_marked());
}

// ---- Compare numbers by value sees through the marker ----

fn sheet_table(cells: Vec<(u32, u32, Cell)>) -> Table {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    sheet.cells = cells;
    let mut book = open(dir.path(), "a.xlsx", &[sheet]);
    read(&mut book, 0).table
}

fn by_value(numbers: bool, keys: &str) -> crate::table::compare::CompareOptions {
    crate::table::compare::CompareOptions {
        header: true,
        keys: keys.to_owned(),
        rules: crate::table::canon::CellRules {
            numbers,
            ..Default::default()
        },
    }
}

#[test]
fn text_and_number_pair_up_by_value_and_only_by_value() {
    use crate::table::compare::compare;
    let left = sheet_table(vec![
        (1, 1, text("id")),
        (1, 2, text("v")),
        (2, 1, num(1001.0)),
        (2, 2, num(1.5)),
        (3, 1, num(1002.0)),
        (3, 2, text("1.50")),
    ]);
    let right = sheet_table(vec![
        (1, 1, text("id")),
        (1, 2, text("v")),
        (2, 1, text("1002")),
        (2, 2, num(1.5)),
        (3, 1, text("1001")),
        (3, 2, text("1.5")),
    ]);
    let cancel = CancelToken::new();
    let off = compare(Some(&left), Some(&right), &by_value(false, "id"), &cancel).unwrap();
    assert_eq!(off.counts.same, 0);
    assert_eq!(
        off.counts.left_only + off.counts.right_only,
        4,
        "text is not a number"
    );
    let on = compare(Some(&left), Some(&right), &by_value(true, "id"), &cancel).unwrap();
    assert_eq!(
        on.counts.left_only + on.counts.right_only,
        0,
        "the keys pair"
    );
    assert_eq!(
        on.counts.differs, 0,
        "1.5 and text 1.5 and text 1.50 are one value"
    );
    assert_eq!(on.counts.same, 2);
    // By row order the rows align on the same values.
    let left = sheet_table(vec![(1, 1, text("id")), (2, 1, num(1001.0))]);
    let right = sheet_table(vec![(1, 1, text("id")), (2, 1, text("1001"))]);
    let on = compare(Some(&left), Some(&right), &by_value(true, ""), &cancel).unwrap();
    assert_eq!(on.counts.same, 1);
}

#[test]
fn by_value_sees_through_a_marker_and_nothing_else() {
    use crate::table::compare::compare;
    let row = |a: Cell, b: Cell| (sheet_table(vec![(1, 1, a)]), sheet_table(vec![(1, 1, b)]));
    let cancel = CancelToken::new();
    let opts = crate::table::compare::CompareOptions {
        header: false,
        keys: String::new(),
        rules: crate::table::canon::CellRules {
            numbers: true,
            ..Default::default()
        },
    };
    for (a, b, same) in [
        (text("1"), num(1.0), true),
        (text("1.0"), num(1.0), true),
        (text("'1"), num(1.0), false),
        (text("'1.50"), num(1.5), false),
        (text("'x"), text("x"), false),
        (text("TRUE"), Cell::Bool(true), false),
        (text("2024-03-05"), Cell::Date(45356.0), false),
        (text("#N/A"), Cell::Err("#N/A".into()), false),
    ] {
        let label = format!("{a:?} against {b:?}");
        let (l, r) = row(a, b);
        let diff = compare(Some(&l), Some(&r), &opts, &cancel).unwrap();
        assert_eq!(diff.counts.same == 1, same, "{label}");
    }
    // A table that carries no marker keeps what its cells say: a CSV's '1 is the text '1.
    let csv = crate::table::parse::parse_str("'1\n", Delimiter::Comma);
    let (one, _) = row(num(1.0), num(1.0));
    let diff = compare(Some(&csv), Some(&one), &opts, &cancel).unwrap();
    assert_eq!(diff.counts.same, 0);
}

// ---- What Excel really writes ----

#[test]
fn a_deflated_workbook_reads_as_a_stored_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("Data");
    sheet.cells.push((1, 1, text("ชื่อลูกค้า")));
    sheet.cells.push((1, 2, num(0.25)));
    sheet.cells.push((1, 3, Cell::Date(45356.0)));
    let specs = [sheet];
    let stored = rows(&read(&mut open(dir.path(), "s.xlsx", &specs), 0).table);
    let bytes = fixture::deflated_workbook(&specs);
    let mut book = Workbook::open(&write(dir.path(), "d.xlsx", &bytes)).unwrap();
    assert_eq!(rows(&read(&mut book, 0).table), stored);
    assert_eq!(stored, [["ชื่อลูกค้า", "0.25", "2024-03-05"]]);
}

#[test]
fn shared_strings_rich_runs_and_styles_read_as_excel_saved_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("ยอดขาย");
    let cells = vec![
        text("ชื่อลูกค้า"),
        Cell::Rich(vec!["สวัสดี".into(), " ครับ".into()]),
        text("ชื่อลูกค้า"),
        text("1"),
        text("  spaced  "),
        Cell::Date(45356.0),
        Cell::DateAlt(45356.5),
        Cell::Time(0.5625),
        Cell::Duration(26.5 / 24.0),
        Cell::Percent(0.25),
        Cell::Money(1234.5),
        Cell::Formula {
            text: "A1".into(),
            value: Some(Box::new(text("ผล"))),
        },
    ];
    for (c, cell) in cells.into_iter().enumerate() {
        sheet.cells.push((1, c as u32 + 1, cell));
    }
    let bytes = fixture::excel_workbook(&[sheet]);
    let mut book = Workbook::open(&write(dir.path(), "x.xlsx", &bytes)).unwrap();
    assert_eq!(book.sheets()[0].name, "ยอดขาย");
    assert_eq!(
        rows(&read(&mut book, 0).table),
        [[
            "ชื่อลูกค้า",
            "สวัสดี ครับ",
            "ชื่อลูกค้า",
            "'1",
            "  spaced  ",
            "2024-03-05",
            "2024-03-05 12:00:00",
            "13:30:00",
            "26:30:00",
            "0.25",
            "1234.5",
            "ผล",
        ]]
    );
}

// ---- Reads that stop ----

#[test]
fn a_new_style_error_cell_stops_that_sheet_only_and_names_itself() {
    let dir = tempfile::tempdir().unwrap();
    for value in ["#SPILL!", "#CALC!"] {
        let mut bad = SheetSpec::new("Bad");
        bad.cells.push((1, 1, Cell::Err(value.into())));
        let mut book = open(
            dir.path(),
            "a.xlsx",
            &[SheetSpec::text_rows("Good", &[&["a"]]), bad],
        );
        let got = book.read_sheet(1, &CancelToken::new(), &AtomicU64::new(0));
        match got {
            Err(ReadFail::CellError(text)) => assert_eq!(text, value),
            other => panic!("{value}: {other:?}"),
        }
        assert_eq!(
            rows(&read(&mut book, 0).table),
            [["a"]],
            "the other sheet reads"
        );
    }
}

#[test]
fn a_read_cancelled_in_the_middle_stops_in_the_middle() {
    const ROWS: u64 = 400_000;
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    sheet.cells.reserve(ROWS as usize);
    for r in 1..=ROWS as u32 {
        sheet.cells.push((r, 1, text("x")));
    }
    let mut book = open(dir.path(), "a.xlsx", &[sheet]);
    let cancel = CancelToken::new();
    let done = AtomicU64::new(0);
    let got = std::thread::scope(|scope| {
        scope.spawn(|| {
            // The read has made progress, and is far from its end.
            while done.load(Ordering::Relaxed) == 0 {
                std::thread::yield_now();
            }
            cancel.cancel();
        });
        book.read_sheet(0, &cancel, &done)
    });
    assert!(matches!(got, Err(ReadFail::Cancelled)), "{got:?}");
    let reached = done.load(Ordering::Relaxed);
    assert!(reached > 0 && reached < ROWS, "stopped at {reached}");
}

#[test]
fn opening_stops_between_its_steps_when_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(
        dir.path(),
        "a.xlsx",
        &workbook(&[SheetSpec::text_rows("S", &[&["a"]])]),
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(matches!(
        Workbook::open_with(&path, &cancel),
        Err(OpenError::Cancelled)
    ));
    assert!(Workbook::open_with(&path, &CancelToken::new()).is_ok());
}

// ---- Files that must be refused, not crash ----

#[test]
fn a_tiny_file_claiming_billions_of_strings_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let sst = b"<sst uniqueCount=\"99999999999\" count=\"99999999999\"><si><t>a</t></si></sst>";
    let bytes = workbook_with_parts(
        &[SheetSpec::text_rows("S", &[&["a"]])],
        &[("xl/sharedStrings.xml", sst)],
    );
    assert!(bytes.len() < 4096, "{}", bytes.len());
    assert!(matches!(
        open_err(dir.path(), "bomb.xlsx", &bytes),
        OpenError::TooLarge(TooLarge::Strings)
    ));
}

#[test]
fn a_real_string_table_with_a_large_reference_count_opens() {
    let dir = tempfile::tempdir().unwrap();
    let sst = b"<sst count=\"1000000\" uniqueCount=\"1\"><si><t>Yes</t></si></sst>";
    let sheet = "<worksheet><sheetData><row r=\"1\"><c r=\"A1\" t=\"s\"><v>0</v></c></row>\
</sheetData></worksheet>";
    let bytes = workbook_with_parts(
        &[SheetSpec::new("S")],
        &[
            ("xl/sharedStrings.xml", sst),
            ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
        ],
    );
    let mut book = Workbook::open(&write(dir.path(), "a.xlsx", &bytes)).unwrap();
    assert_eq!(rows(&read(&mut book, 0).table), [["Yes"]]);
}

#[test]
fn a_file_declaring_a_huge_part_is_refused_before_it_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = workbook(&[SheetSpec::text_rows("S", &[&["a"]])]);
    fixture::declare_size(&mut bytes, "xl/worksheets/sheet1.xml", 0x7000_0000);
    assert!(matches!(
        open_err(dir.path(), "big.xlsx", &bytes),
        OpenError::TooLarge(TooLarge::Part)
    ));
}

#[test]
fn a_sheet_larger_than_it_declares_stops_as_damaged() {
    let dir = tempfile::tempdir().unwrap();
    let mut sheet = SheetSpec::new("S");
    for r in 1..=5000 {
        sheet.cells.push((r, 1, text("some text to take up room")));
    }
    let mut bytes = fixture::deflated_workbook(&[sheet]);
    fixture::declare_size(&mut bytes, "xl/worksheets/sheet1.xml", 1000);
    let mut book = Workbook::open(&write(dir.path(), "lie.xlsx", &bytes)).unwrap();
    let got = book.read_sheet(0, &CancelToken::new(), &AtomicU64::new(0));
    assert!(matches!(got, Err(ReadFail::Damaged(_))), "{got:?}");
}

#[test]
fn an_excel_4_macro_sheet_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let mut macros = SheetSpec::new("Macro1");
    macros.macro4 = true;
    let bytes = workbook(&[SheetSpec::text_rows("S", &[&["a"]]), macros]);
    assert!(matches!(
        open_err(dir.path(), "m.xlsm", &bytes),
        OpenError::MacroSheet
    ));
}

// ---- Errors that are the machine's, not the file's ----

#[test]
fn an_io_error_inside_the_reader_is_an_io_error() {
    let sharing = || std::io::Error::from_raw_os_error(32);
    for error in [
        XlsxError::Io(sharing()),
        XlsxError::Zip(zip::result::ZipError::Io(sharing())),
        XlsxError::Xml(sharing().into()),
    ] {
        let label = format!("{error:?}");
        match open_error(error) {
            OpenError::Io(err) => assert_eq!(err.raw_os_error(), Some(32), "{label}"),
            other => panic!("{label}: {other:?}"),
        }
    }
    for error in [
        XlsxError::Zip(zip::result::ZipError::Io(sharing())),
        XlsxError::Xml(sharing().into()),
    ] {
        let label = format!("{error:?}");
        match read_fail(error) {
            ReadFail::Io(err) => assert_eq!(err.raw_os_error(), Some(32), "{label}"),
            other => panic!("{label}: {other:?}"),
        }
    }
}

#[test]
fn bad_data_surfacing_as_an_io_error_stays_damaged() {
    // Inflate reports a corrupt stream through `io::Error`, with no code from the system.
    let corrupt = || std::io::Error::new(std::io::ErrorKind::InvalidData, "corrupt deflate stream");
    assert!(matches!(
        read_fail(XlsxError::Xml(corrupt().into())),
        ReadFail::Damaged(_)
    ));
    assert!(matches!(
        open_error(XlsxError::Zip(zip::result::ZipError::Io(corrupt()))),
        OpenError::Damaged(_)
    ));
}

// ---- The first bytes ----

#[test]
fn a_file_starts_like_a_workbook_or_it_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    let real = workbook(&[SheetSpec::text_rows("S", &[&["a"]])]);
    assert!(has_workbook_magic(&write(p, "a.xlsx", &real)).unwrap());
    assert!(has_workbook_magic(&write(p, "b.xlsx", &protected_bytes())).unwrap());
    assert!(has_workbook_magic(&write(p, "c.xlsx", b"PK")).unwrap());
    assert!(!has_workbook_magic(&write(p, "d.xlsx", b"id,name\n")).unwrap());
    assert!(!has_workbook_magic(&write(p, "e.xlsx", b"")).unwrap());
    assert!(!has_workbook_magic(&write(p, "f.xlsx", b"P")).unwrap());
    assert!(has_workbook_magic(&p.join("missing.xlsx")).is_err());
}
