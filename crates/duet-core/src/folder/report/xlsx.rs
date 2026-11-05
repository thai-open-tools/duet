//! The folder Excel workbook (LocalDiff's `buildFolderXlsx`, `sheet:148-411`), written by hand
//! as SpreadsheetML parts in a stored ZIP (R1): a Summary sheet, a Files sheet with filters,
//! frozen panes, status colours, reviewer columns and links, and a Changed lines sheet.

use std::collections::HashMap;
use std::fmt::Write as _;

use chrono::NaiveDateTime;

use super::csv::{RowData, csv_columns, row_data};
use super::lines::{LineDetails, SHEET_ROWS, SheetKind};
use super::zip::{TooLarge, stored_zip};
use super::{
    Attention, ItemState, NOT_SCANNED_HINT, ReportContext, ReportItem, ReportOptions, attention,
    count_text, report_counts, sort_items, top_groups,
};
use crate::i18n::{tr, tr_key, trf};
use crate::text::odd::{OddKind, is_odd_after, odd_info};

#[cfg(test)]
mod tests;

/// The sheets' names are keys: `tr` turns each into the report's language where it is used.
const SHEET_SUMMARY: &str = tr_key("Summary");
const SHEET_FILES: &str = tr_key("Files");
const SHEET_LINES: &str = tr_key("Changed lines");
/// The choices of the Result column (`RPT_REVIEW`), as keys.
const REVIEW: [&str; 4] = [
    tr_key("Passed"),
    tr_key("Needs a fix"),
    tr_key("To review"),
    tr_key("Not relevant"),
];
/// Characters a cell keeps; Excel's own limit is 32,767.
const CELL_MAX: usize = 32_000;
/// The Files sheet's column widths, in `csv_columns` order (`rptSheetCols`).
const FILE_WIDTHS: [u32; 21] = [
    7, 44, 34, 10, 16, 30, 11, 13, 13, 10, 46, 14, 14, 14, 19, 19, 60, 60, 14, 14, 34,
];
/// The Result column.
const RESULT_COLUMN: usize = 19;

/// The cell formats of `STYLES` (`XS`).
mod style {
    pub const DEF: u8 = 0;
    pub const HEAD: u8 = 1;
    pub const DATE: u8 = 2;
    pub const INT: u8 = 3;
    pub const ST_DIFF: u8 = 4;
    pub const ST_LEFT: u8 = 5;
    pub const ST_RIGHT: u8 = 6;
    pub const ST_SAME: u8 = 7;
    pub const WARN: u8 = 8;
    pub const TITLE: u8 = 9;
    pub const BOLD: u8 = 10;
    pub const MONO: u8 = 11;
    pub const PCT: u8 = 12;
    pub const LINK: u8 = 13;
    pub const INPUT: u8 = 14;
    pub const M_MOD: u8 = 15;
    pub const M_DEL: u8 = 16;
    pub const M_INS: u8 = 17;
    pub const M_SOFT: u8 = 18;
    pub const WRAP: u8 = 19;
    pub const MUTED: u8 = 20;
}

/// LocalDiff's `XL_STYLES` (`sheet:179-219`), unchanged.
const STYLES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
    r##"<numFmts count="2"><numFmt numFmtId="164" formatCode="yyyy-mm-dd hh:mm:ss"/><numFmt numFmtId="165" formatCode="#,##0"/></numFmts>"##,
    r#"<fonts count="7">"#,
    r#"<font><sz val="10"/><name val="Tahoma"/><family val="2"/></font>"#,
    r#"<font><b/><sz val="10"/><name val="Tahoma"/><family val="2"/></font>"#,
    r#"<font><b/><sz val="14"/><name val="Tahoma"/><family val="2"/></font>"#,
    r#"<font><sz val="10"/><name val="Consolas"/><family val="3"/></font>"#,
    r#"<font><sz val="10"/><color rgb="FFB0432B"/><name val="Tahoma"/><family val="2"/></font>"#,
    r#"<font><u/><sz val="10"/><color rgb="FF2C5D9E"/><name val="Consolas"/><family val="3"/></font>"#,
    r#"<font><sz val="10"/><color rgb="FF7A8391"/><name val="Tahoma"/><family val="2"/></font></fonts>"#,
    r#"<fills count="9"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill>"#,
    r#"<fill><patternFill patternType="solid"><fgColor rgb="FFE9ECF1"/><bgColor indexed="64"/></patternFill></fill>"#,
    r#"<fill><patternFill patternType="solid"><fgColor rgb="FFFBF3DF"/><bgColor indexed="64"/></patternFill></fill>"#,
    r#"<fill><patternFill patternType="solid"><fgColor rgb="FFFBE9EC"/><bgColor indexed="64"/></patternFill></fill>"#,
    r#"<fill><patternFill patternType="solid"><fgColor rgb="FFE3F3EA"/><bgColor indexed="64"/></patternFill></fill>"#,
    r#"<fill><patternFill patternType="solid"><fgColor rgb="FFFFF8DB"/><bgColor indexed="64"/></patternFill></fill>"#,
    r#"<fill><patternFill patternType="solid"><fgColor rgb="FFFDF0EA"/><bgColor indexed="64"/></patternFill></fill>"#,
    r#"<fill><patternFill patternType="solid"><fgColor rgb="FFF1EFFA"/><bgColor indexed="64"/></patternFill></fill></fills>"#,
    r#"<borders count="2"><border><left/><right/><top/><bottom/><diagonal/></border>"#,
    r#"<border><left/><right/><top/><bottom style="thin"><color rgb="FFC9CFD8"/></bottom><diagonal/></border></borders>"#,
    r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#,
    r#"<cellXfs count="21">"#,
    r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>"#,
    r#"<xf numFmtId="0" fontId="1" fillId="2" borderId="1" xfId="0" applyFont="1" applyFill="1" applyBorder="1" applyAlignment="1"><alignment vertical="center" wrapText="1"/></xf>"#,
    r#"<xf numFmtId="164" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>"#,
    r#"<xf numFmtId="165" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>"#,
    r#"<xf numFmtId="0" fontId="0" fillId="3" borderId="0" xfId="0" applyFill="1"/>"#,
    r#"<xf numFmtId="0" fontId="0" fillId="4" borderId="0" xfId="0" applyFill="1"/>"#,
    r#"<xf numFmtId="0" fontId="0" fillId="5" borderId="0" xfId="0" applyFill="1"/>"#,
    r#"<xf numFmtId="0" fontId="6" fillId="0" borderId="0" xfId="0" applyFont="1"/>"#,
    r#"<xf numFmtId="0" fontId="4" fillId="7" borderId="0" xfId="0" applyFont="1" applyFill="1"/>"#,
    r#"<xf numFmtId="0" fontId="2" fillId="0" borderId="0" xfId="0" applyFont="1"/>"#,
    r#"<xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/>"#,
    r#"<xf numFmtId="0" fontId="3" fillId="0" borderId="0" xfId="0" applyFont="1"/>"#,
    r#"<xf numFmtId="9" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>"#,
    r#"<xf numFmtId="0" fontId="5" fillId="0" borderId="0" xfId="0" applyFont="1"/>"#,
    r#"<xf numFmtId="0" fontId="0" fillId="6" borderId="0" xfId="0" applyFill="1"/>"#,
    r#"<xf numFmtId="0" fontId="3" fillId="3" borderId="0" xfId="0" applyFont="1" applyFill="1"/>"#,
    r#"<xf numFmtId="0" fontId="3" fillId="4" borderId="0" xfId="0" applyFont="1" applyFill="1"/>"#,
    r#"<xf numFmtId="0" fontId="3" fillId="5" borderId="0" xfId="0" applyFont="1" applyFill="1"/>"#,
    r#"<xf numFmtId="0" fontId="3" fillId="8" borderId="0" xfId="0" applyFont="1" applyFill="1"/>"#,
    r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyAlignment="1"><alignment vertical="top" wrapText="1"/></xf>"#,
    r#"<xf numFmtId="0" fontId="6" fillId="0" borderId="0" xfId="0" applyFont="1"/>"#,
    r#"</cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#,
);

const XML_HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_PACKAGE_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// 0 → "A", 25 → "Z", 26 → "AA".
pub fn xl_col(index: usize) -> String {
    let mut letters = Vec::new();
    let mut n = index + 1;
    while n > 0 {
        letters.push(b'A' + ((n - 1) % 26) as u8);
        n = (n - 1) / 26;
    }
    letters.iter().rev().map(|&b| char::from(b)).collect()
}

/// Days since 1899-12-30 00:00, Excel's date serial of a local time (`xlSerial`).
pub fn excel_serial(time: NaiveDateTime) -> f64 {
    let epoch = chrono::NaiveDate::from_ymd_opt(1899, 12, 30)
        .and_then(|day| day.and_hms_opt(0, 0, 0))
        .unwrap_or_default();
    time.signed_duration_since(epoch).num_milliseconds() as f64 / 86_400_000.
}

/// A character XML 1.0 does not allow (a lone surrogate cannot occur in a `str`).
fn not_xml(ch: char) -> bool {
    matches!(
        ch,
        '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}'
    )
}

/// R8's cleaning and XML escaping of a cell's text (`xlText`): a character XML cannot hold
/// becomes "[U+XXXX]", then the text is cut at 32,000 characters (UTF-16 units, as Excel counts
/// them) with a note. The labels come before the cut, so a cell never grows past it.
pub fn xl_text(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    for ch in text.chars() {
        if not_xml(ch) {
            let _ = write!(clean, "[U+{:04X}]", ch as u32);
        } else {
            clean.push(ch);
        }
    }
    let mut units = 0;
    let cut = clean.char_indices().find_map(|(at, ch)| {
        units += ch.len_utf16();
        (units > CELL_MAX).then_some(at)
    });
    if let Some(at) = cut {
        clean.truncate(at);
        clean.push_str(tr(" …(cut to 32,000 characters)"));
    }
    let mut out = String::with_capacity(clean.len() + 16);
    for ch in clean.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// `xlShowOdd` (`sheet:160-173`): invisible characters as plain labels, since Excel fonts
/// rarely have "⍽". NBSP is "[NBSP]"; another space kind, or a label longer than 6, is its code
/// "[U+XXXX]"; anything else is its label ("[LRM]"). A joiner that is part of the writing stays,
/// as in the HTML report.
pub fn xl_show_odd(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev = None;
    for ch in text.chars() {
        if is_odd_after(prev, ch) {
            let (label, _, kind) = odd_info(ch);
            if ch == '\u{a0}' {
                out.push_str("[NBSP]");
            } else if kind == OddKind::Space || label.chars().count() > 6 {
                let _ = write!(out, "[U+{:04X}]", ch as u32);
            } else {
                let _ = write!(out, "[{label}]");
            }
        } else {
            out.push(ch);
        }
        prev = Some(ch);
    }
    out
}

enum Value {
    Text(String),
    Int(u64),
    Share(f64),
    Date(NaiveDateTime),
}

/// One cell: an empty text with a style is a filled cell without a value.
struct Cell {
    value: Value,
    style: u8,
    link: Option<String>,
}

fn text(value: impl Into<String>, style: u8) -> Option<Cell> {
    Some(Cell {
        value: Value::Text(value.into()),
        style,
        link: None,
    })
}

fn int(value: u64, style: u8) -> Option<Cell> {
    Some(Cell {
        value: Value::Int(value),
        style,
        link: None,
    })
}

/// A row of column titles.
fn heads<T: Into<String>>(titles: impl IntoIterator<Item = T>) -> Vec<Option<Cell>> {
    titles
        .into_iter()
        .map(|title| text(title, style::HEAD))
        .collect()
}

fn count(value: usize) -> Option<Cell> {
    int(value as u64, style::DEF)
}

/// `n / total` as a percentage cell (0 when there is nothing).
fn share(n: usize, total: usize) -> Option<Cell> {
    Some(Cell {
        value: Value::Share(if total == 0 {
            0.
        } else {
            n as f64 / total as f64
        }),
        style: style::PCT,
        link: None,
    })
}

/// LocalDiff's `XlSheet`: rows are added one at a time, then `xml` writes the part.
struct Sheet {
    name: &'static str,
    widths: &'static [u32],
    /// Frozen columns and rows.
    freeze: Option<(usize, u32)>,
    data: String,
    rows: u32,
    /// (cell, location) of each link inside the workbook.
    links: Vec<(String, String)>,
    /// The auto-filter's last column and last row.
    filter: Option<(usize, u32)>,
    validations: Vec<String>,
}

impl Sheet {
    fn new(name: &'static str, widths: &'static [u32]) -> Self {
        Self {
            name,
            widths,
            freeze: None,
            data: String::new(),
            rows: 0,
            links: Vec::new(),
            filter: None,
            validations: Vec::new(),
        }
    }

    /// Adds a row and returns its 1-based number. `None` cells are left out.
    fn row(&mut self, cells: &[Option<Cell>], height: Option<u32>) -> u32 {
        self.rows += 1;
        let r = self.rows;
        let out = &mut self.data;
        let _ = write!(out, "<row r=\"{r}\"");
        if let Some(height) = height {
            let _ = write!(out, " ht=\"{height}\" customHeight=\"1\"");
        }
        out.push('>');
        for (column, cell) in cells.iter().enumerate() {
            let Some(cell) = cell else {
                continue;
            };
            let at = format!("{}{r}", xl_col(column));
            if let Some(link) = &cell.link {
                self.links.push((at.clone(), link.clone()));
            }
            let style = cell.style;
            let attr = |style: u8| {
                if style > 0 {
                    format!(" s=\"{style}\"")
                } else {
                    String::new()
                }
            };
            match &cell.value {
                Value::Text(value) if value.is_empty() => {
                    if style > 0 {
                        let _ = write!(out, "<c r=\"{at}\" s=\"{style}\"/>");
                    }
                }
                Value::Text(value) => {
                    let _ = write!(
                        out,
                        "<c r=\"{at}\"{} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                        attr(style),
                        xl_text(value)
                    );
                }
                Value::Int(value) => {
                    let _ = write!(out, "<c r=\"{at}\"{}><v>{value}</v></c>", attr(style));
                }
                Value::Share(value) => {
                    let _ = write!(out, "<c r=\"{at}\"{}><v>{value}</v></c>", attr(style));
                }
                Value::Date(time) => {
                    let style = if style > 0 { style } else { style::DATE };
                    let _ = write!(
                        out,
                        "<c r=\"{at}\" s=\"{style}\"><v>{:.6}</v></c>",
                        excel_serial(*time)
                    );
                }
            }
        }
        out.push_str("</row>");
        r
    }

    fn filter_ref(&self) -> Option<String> {
        self.filter
            .map(|(column, row)| format!("A1:{}{row}", xl_col(column)))
    }

    fn xml(&self, selected: bool) -> String {
        let mut out = String::with_capacity(self.data.len() + 1024);
        let _ = write!(
            out,
            "{XML_HEAD}<worksheet xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_REL}\"><sheetViews><sheetView workbookViewId=\"0\"{}>",
            if selected { " tabSelected=\"1\"" } else { "" }
        );
        if let Some((x, y)) = self.freeze {
            let pane = match (x > 0, y > 0) {
                (true, true) => "bottomRight",
                (false, true) => "bottomLeft",
                _ => "topRight",
            };
            let corner = format!("{}{}", xl_col(x), y + 1);
            out.push_str("<pane");
            if x > 0 {
                let _ = write!(out, " xSplit=\"{x}\"");
            }
            if y > 0 {
                let _ = write!(out, " ySplit=\"{y}\"");
            }
            let _ = write!(
                out,
                " topLeftCell=\"{corner}\" activePane=\"{pane}\" state=\"frozen\"/><selection pane=\"{pane}\" activeCell=\"{corner}\" sqref=\"{corner}\"/>"
            );
        }
        out.push_str("</sheetView></sheetViews><sheetFormatPr defaultRowHeight=\"15\"/>");
        if !self.widths.is_empty() {
            out.push_str("<cols>");
            for (at, width) in self.widths.iter().enumerate() {
                let n = at + 1;
                let _ = write!(
                    out,
                    "<col min=\"{n}\" max=\"{n}\" width=\"{width}\" customWidth=\"1\"/>"
                );
            }
            out.push_str("</cols>");
        }
        let _ = write!(out, "<sheetData>{}</sheetData>", self.data);
        if let Some(filter) = self.filter_ref() {
            let _ = write!(out, "<autoFilter ref=\"{filter}\"/>");
        }
        if !self.validations.is_empty() {
            let _ = write!(
                out,
                "<dataValidations count=\"{}\">{}</dataValidations>",
                self.validations.len(),
                self.validations.concat()
            );
        }
        if !self.links.is_empty() {
            out.push_str("<hyperlinks>");
            for (at, location) in &self.links {
                let _ = write!(
                    out,
                    "<hyperlink ref=\"{at}\" location=\"{}\" display=\"{}\"/>",
                    xl_text(location),
                    xl_text(tr("Go to the changed lines"))
                );
            }
            out.push_str("</hyperlinks>");
        }
        out.push_str("<pageMargins left=\"0.5\" right=\"0.5\" top=\"0.6\" bottom=\"0.6\" header=\"0.3\" footer=\"0.3\"/></worksheet>");
        out
    }
}

/// A sheet's name as a formula writes it between quotes: a quote inside is doubled.
fn quoted(name: &str) -> String {
    name.replace('\'', "''")
}

/// The choices of the Result column in the report's language.
fn review_choices() -> Vec<&'static str> {
    REVIEW.iter().copied().map(tr).collect()
}

/// "100,000".
fn grouped(n: usize) -> String {
    count_text(n as u64, "", "").trim_end().to_owned()
}

/// The workbook: Summary (selected), Files and, when the lines are shown and some file has
/// them, Changed lines. Its parts are dated with `context.generated` (the ZIP) and
/// `context.generated_utc` (the document properties). An error when it would pass 4 GiB.
pub fn folder_xlsx(
    items: &[ReportItem],
    lines: Option<&LineDetails>,
    options: &ReportOptions,
    context: &ReportContext,
) -> Result<Vec<u8>, TooLarge> {
    let _lang = crate::i18n::scoped(options.lang);
    let names = &options.names;
    let mut sorted = items.to_vec();
    sort_items(&mut sorted);
    let shown_lines = lines.filter(|lines| {
        options.details
            && sorted.iter().any(|item| {
                lines
                    .files
                    .get(&item.rel)
                    .is_some_and(|info| !info.sheet.is_empty())
            })
    });

    // Changed lines first, so the Files sheet knows the row each file's lines start on.
    let mut changed = Sheet::new(tr(SHEET_LINES), &[44, 8, 13, 9, 70, 9, 70]);
    let mut first_rows: HashMap<&str, u32> = HashMap::new();
    if let Some(lines) = shown_lines {
        changed_lines(&mut changed, &mut first_rows, &sorted, lines, names);
    }

    let mut files = Sheet::new(tr(SHEET_FILES), &FILE_WIDTHS);
    files.freeze = Some((3, 1));
    files.row(&heads(csv_columns(names)), Some(32));
    for (index, item) in sorted.iter().enumerate() {
        let data = row_data(item, index, names, lines);
        let link = first_rows.get(item.rel.as_str()).copied();
        files.row(&file_cells(&data, item.state, link), None);
    }
    let last = (sorted.len() as u32 + 1).max(2);
    files.filter = Some((FILE_WIDTHS.len() - 1, last));
    let result = xl_col(RESULT_COLUMN);
    files.validations.push(format!(
        "<dataValidation type=\"list\" allowBlank=\"1\" showInputMessage=\"1\" showErrorMessage=\"1\" promptTitle=\"{}\" prompt=\"{}\" sqref=\"{result}2:{result}{last}\"><formula1>\"{}\"</formula1></dataValidation>",
        xl_text(tr("Result")),
        xl_text(tr("Pick one from the list")),
        xl_text(&review_choices().join(","))
    ));

    let summary = summary_sheet(
        items,
        &sorted,
        lines,
        options,
        context,
        shown_lines.is_some(),
    );
    let mut sheets = vec![summary, files];
    if shown_lines.is_some() {
        sheets.push(changed);
    }
    workbook(&sheets, context)
}

fn changed_lines<'a>(
    sheet: &mut Sheet,
    first_rows: &mut HashMap<&'a str, u32>,
    sorted: &'a [ReportItem],
    lines: &LineDetails,
    names: &[String; 2],
) {
    sheet.freeze = Some((1, 1));
    let header = heads([
        tr("File").to_owned(),
        tr("Place").to_owned(),
        tr("Kind").to_owned(),
        trf("Line {}", &[&names[0]]),
        trf("Text {}", &[&names[0]]),
        trf("Line {}", &[&names[1]]),
        trf("Text {}", &[&names[1]]),
    ]);
    sheet.row(&header, Some(30));
    for item in sorted {
        let Some(info) = lines.files.get(&item.rel) else {
            continue;
        };
        for (at, row) in info.sheet.iter().enumerate() {
            let (kind, style) = match row.kind {
                SheetKind::Changed => (tr("Changed").to_owned(), style::M_MOD),
                SheetKind::LeftOnly => (trf("Only in {}", &[&names[0]]), style::M_DEL),
                SheetKind::RightOnly => (trf("Only in {}", &[&names[1]]), style::M_INS),
                SheetKind::Soft => (tr("Unimportant").to_owned(), style::M_SOFT),
            };
            let number = |side: &Option<(u32, String)>| {
                side.as_ref()
                    .and_then(|(n, _)| int(u64::from(*n), style::DEF))
            };
            let line = |side: &Option<(u32, String)>| {
                side.as_ref()
                    .and_then(|(_, line)| text(xl_show_odd(line), style))
            };
            let r = sheet.row(
                &[
                    text(&item.rel, style::MONO),
                    int(u64::from(row.place), style::DEF),
                    text(kind, style::DEF),
                    number(&row.left),
                    line(&row.left),
                    number(&row.right),
                    line(&row.right),
                ],
                None,
            );
            if at == 0 {
                first_rows.insert(&item.rel, r);
            }
        }
    }
    if lines.sheet_cut_files > 0 {
        sheet.row(
            &[text(
                trf(
                    "… {} no lines here (over {} rows)",
                    &[
                        &count_text(
                            lines.sheet_cut_files as u64,
                            "more file has",
                            "more files have",
                        ),
                        &grouped(SHEET_ROWS),
                    ],
                ),
                style::MUTED,
            )],
            None,
        );
    }
    sheet.filter = Some((6, sheet.rows.max(2)));
}

/// One Files row, in `csv_columns` order (`sheet:330-343`).
fn file_cells(data: &RowData, state: ItemState, link: Option<u32>) -> Vec<Option<Cell>> {
    let mono = |value: &str| text(value, style::MONO).filter(|_| !value.is_empty());
    let plain = |value: &str| text(value, style::DEF);
    let lines = |n: Option<u32>| n.and_then(|n| int(u64::from(n), style::DEF));
    let size = |n: Option<u64>| n.and_then(|n| int(n, style::INT));
    let time = |time: Option<NaiveDateTime>| {
        time.map(|time| Cell {
            value: Value::Date(time),
            style: style::DEF,
            link: None,
        })
    };
    let state_style = match state {
        ItemState::Different => style::ST_DIFF,
        ItemState::LeftOnly => style::ST_LEFT,
        ItemState::RightOnly => style::ST_RIGHT,
        ItemState::Same => style::ST_SAME,
    };
    vec![
        count(data.no),
        mono(&data.folder),
        Some(Cell {
            value: Value::Text(data.name.clone()),
            style: if link.is_some() {
                style::LINK
            } else {
                style::MONO
            },
            link: link.map(|row| format!("'{}'!A{row}", quoted(tr(SHEET_LINES)))),
        }),
        plain(&data.ext),
        text(&data.state, state_style),
        text(&data.attention, style::WARN).filter(|_| !data.attention.is_empty()),
        lines(data.changed),
        lines(data.left_only),
        lines(data.right_only),
        lines(data.places),
        plain(&data.note),
        plain(&data.newer),
        size(data.left_size),
        size(data.right_size),
        time(data.left_time),
        time(data.right_time),
        mono(&data.left_path),
        mono(&data.right_path),
        text("", style::INPUT),
        text("", style::INPUT),
        text("", style::INPUT),
    ]
}

/// The Summary sheet (`sheet:349-407`).
fn summary_sheet(
    items: &[ReportItem],
    sorted: &[ReportItem],
    lines: Option<&LineDetails>,
    options: &ReportOptions,
    context: &ReportContext,
    has_rows: bool,
) -> Sheet {
    let names = &options.names;
    let c = report_counts(items);
    let mismatched = c.mismatched();
    let mut s = Sheet::new(tr(SHEET_SUMMARY), &[40, 22, 16, 14, 12, 12, 60]);
    let title = if options.title.is_empty() {
        trf("{} compared with {}", &[&names[0], &names[1]])
    } else {
        options.title.clone()
    };
    s.row(&[text(title, style::TITLE)], Some(24));
    s.row(
        &[text(
            trf(
                "Folder compare report — made with duet {} on {}",
                &[
                    &context.version,
                    &context.generated.format("%Y-%m-%d %H:%M"),
                ],
            ),
            style::MUTED,
        )],
        None,
    );
    if !options.note.is_empty() {
        s.row(&[text(&options.note, style::WRAP)], None);
    }
    s.row(&[], None);
    s.row(
        &heads([
            tr("Side"),
            tr("Name"),
            tr("Files"),
            tr("Total size (bytes)"),
            "",
            "",
            tr("Path"),
        ]),
        None,
    );
    for (tag, name, files, bytes, path) in [
        (
            "A",
            &names[0],
            c.left_files,
            c.left_bytes,
            &context.side_paths[0],
        ),
        (
            "B",
            &names[1],
            c.right_files,
            c.right_bytes,
            &context.side_paths[1],
        ),
    ] {
        s.row(
            &[
                text(tag, style::DEF),
                text(name, style::BOLD),
                count(files),
                int(bytes, style::INT),
                None,
                None,
                text(path, style::MONO),
            ],
            None,
        );
    }
    s.row(&[], None);

    let or_dash = |value: &str| {
        if value.is_empty() {
            "-".to_owned()
        } else {
            value.to_owned()
        }
    };
    let line_compare = match (lines, &context.rules) {
        (None, _) => tr("Contents not read").to_owned(),
        (Some(_), Some(rules)) => rules.clone(),
        (Some(_), None) => tr("Every character counts").to_owned(),
    };
    let masking = if options.mask {
        let mut hidden =
            tr("Hidden (•••••• + fingerprint: the same fingerprint means the same value)")
                .to_owned();
        if let Some(lines) = lines.filter(|lines| lines.masked_lines > 0) {
            hidden.push_str(" — ");
            hidden.push_str(&count_text(lines.masked_lines as u64, "line", "lines"));
        }
        hidden
    } else {
        tr("Not hidden").to_owned()
    };
    let mut facts = vec![
        (tr("Criterion"), context.criterion.clone()),
        (tr("Scope"), context.scope.clone()),
        (
            tr("Include / exclude"),
            format!(
                "{} / {}",
                or_dash(&context.include),
                or_dash(&context.exclude)
            ),
        ),
        (tr("Line compare"), line_compare),
        (tr("Hide passwords and keys"), masking),
    ];
    if !context.invisible_scan {
        facts.push((
            tr("Invisible characters"),
            trf("Not scanned ({})", &[&tr(NOT_SCANNED_HINT)]),
        ));
    }
    for (label, value) in facts {
        s.row(&[text(label, style::BOLD), text(value, style::DEF)], None);
    }
    s.row(&[], None);

    s.row(&heads([tr("Status"), tr("Files"), tr("Share")]), None);
    for (label, n, look) in [
        (tr("Different").to_owned(), c.different, style::ST_DIFF),
        (trf("Only in {}", &[&names[0]]), c.left_only, style::ST_LEFT),
        (
            trf("Only in {}", &[&names[1]]),
            c.right_only,
            style::ST_RIGHT,
        ),
        (tr("Same").to_owned(), c.same, style::ST_SAME),
    ] {
        s.row(&[text(label, look), count(n), share(n, c.total)], None);
    }
    s.row(
        &[
            text(tr("Total"), style::BOLD),
            int(c.total as u64, style::BOLD),
            share(1, 1),
        ],
        None,
    );
    s.row(
        &[
            text(tr("Mismatched (different + one side only)"), style::MUTED),
            count(mismatched),
            share(mismatched, c.total),
        ],
        None,
    );
    if c.unimportant > 0 {
        s.row(
            &[
                text(
                    tr("Of these, differ only in unimportant parts (counted as Same)"),
                    style::MUTED,
                ),
                count(c.unimportant),
            ],
            None,
        );
    }

    if let Some(lines) = lines {
        let (mut changed, mut left, mut right, mut read) = (0u64, 0u64, 0u64, 0u64);
        for item in sorted {
            if let Some(info) = lines.files.get(&item.rel).filter(|info| info.counted) {
                changed += u64::from(info.changed);
                left += u64::from(info.left_only);
                right += u64::from(info.right_only);
                read += 1;
            }
        }
        if read > 0 {
            s.row(&[], None);
            let title = trf(
                "Differing lines in total ({} read)",
                &[&count_text(read, "file", "files")],
            );
            s.row(&heads([title, tr("Lines").to_owned()]), None);
            for (label, n) in [
                (tr("Changed").to_owned(), changed),
                (trf("Only in {}", &[&names[0]]), left),
                (trf("Only in {}", &[&names[1]]), right),
            ] {
                s.row(&[text(label, style::DEF), int(n, style::DEF)], None);
            }
        }
    }

    let mut kinds = [0usize; Attention::ALL.len()];
    for item in items {
        for (kind, _) in attention(item, names) {
            if let Some(at) = Attention::ALL.iter().position(|k| *k == kind) {
                kinds[at] += 1;
            }
        }
    }
    if kinds.iter().any(|&n| n > 0) {
        s.row(&[], None);
        s.row(
            &heads([
                format!("⚠ {}", tr("Needs a closer look")),
                tr("Files").to_owned(),
                tr("Meaning").to_owned(),
            ]),
            None,
        );
        for (kind, &n) in Attention::ALL.iter().zip(&kinds) {
            if n > 0 {
                s.row(
                    &[
                        text(kind.title(), style::WARN),
                        count(n),
                        text(kind.help(), style::DEF),
                    ],
                    None,
                );
            }
        }
        s.row(
            &[text(
                trf(
                    "Filter the “{}” column on the “{}” sheet to see the names",
                    &[&tr("Needs a look"), &tr(SHEET_FILES)],
                ),
                style::MUTED,
            )],
            None,
        );
    }

    let mut groups = top_groups(items);
    let mismatched_in = |g: &super::Group| g.different + g.left_only + g.right_only;
    groups.sort_by(|x, y| {
        mismatched_in(y)
            .cmp(&mismatched_in(x))
            .then_with(|| x.folder.cmp(&y.folder))
    });
    if groups.len() > 1 {
        s.row(&[], None);
        let header = heads([
            tr("Folder").to_owned(),
            tr("Different").to_owned(),
            trf("Only in {}", &[&names[0]]),
            trf("Only in {}", &[&names[1]]),
            tr("Same").to_owned(),
            tr("Total").to_owned(),
        ]);
        s.row(&header, None);
        for g in &groups {
            let folder = if g.folder.is_empty() {
                tr("(top level)")
            } else {
                &g.folder
            };
            s.row(
                &[
                    text(folder, style::MONO),
                    count(g.different),
                    count(g.left_only),
                    count(g.right_only),
                    count(g.same),
                    count(mismatched_in(g) + g.same),
                ],
                None,
            );
        }
    }

    s.row(&[], None);
    s.row(&heads([tr("How to use"), "", ""]), None);
    let tips = [
        trf(
            "The “{}” sheet has one row per file: use the filter buttons in the column titles, for example on “{}” or “{}”",
            &[&tr(SHEET_FILES), &tr("Status"), &tr("Needs a look")],
        ),
        trf(
            "The yellow columns ({}, {}, {}) are for the team to fill in; “{}” offers {}",
            &[
                &tr("Reviewer"),
                &tr("Result"),
                &tr("Reviewer note"),
                &tr("Result"),
                &review_choices().join(", "),
            ],
        ),
        if has_rows {
            trf(
                "A blue file name links to its changed lines: click it to go to the “{}” sheet",
                &[&tr(SHEET_LINES)],
            )
        } else {
            trf(
                "No “{}” sheet was added (you can add it when you export)",
                &[&tr(SHEET_LINES)],
            )
        },
        trf(
            "Changed = a line in the same place with different text · Only in {} / Only in {} = a line the other side does not have · [NBSP] / [LRM] = invisible characters",
            &[&names[0], &names[1]],
        ),
        trf(
            "Times are the clock of the computer that made the report · “{}” includes files that differ only in EOL or BOM, or in parts set as unimportant",
            &[&tr("Same")],
        ),
    ];
    for tip in tips {
        s.row(&[text(format!("• {tip}"), style::DEF)], None);
    }
    s
}

/// The package parts around the sheets (`xlWorkbook`, `sheet:260-296`), zipped.
fn workbook(sheets: &[Sheet], context: &ReportContext) -> Result<Vec<u8>, TooLarge> {
    let mut types = format!(
        "{XML_HEAD}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/><Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>"
    );
    for n in 1..=sheets.len() {
        let _ = write!(
            types,
            "<Override PartName=\"/xl/worksheets/sheet{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"
        );
    }
    types.push_str("</Types>");

    let rels = format!(
        "{XML_HEAD}<Relationships xmlns=\"{NS_PACKAGE_REL}\"><Relationship Id=\"rId1\" Type=\"{NS_REL}/officeDocument\" Target=\"xl/workbook.xml\"/><Relationship Id=\"rId2\" Type=\"{NS_PACKAGE_REL}/metadata/core-properties\" Target=\"docProps/core.xml\"/><Relationship Id=\"rId3\" Type=\"{NS_REL}/extended-properties\" Target=\"docProps/app.xml\"/></Relationships>"
    );
    let when = xl_text(&context.generated_utc);
    let core = format!(
        "{XML_HEAD}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:creator>duet</dc:creator><dcterms:created xsi:type=\"dcterms:W3CDTF\">{when}</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">{when}</dcterms:modified></cp:coreProperties>"
    );
    let app = format!(
        "{XML_HEAD}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>duet</Application></Properties>"
    );

    let mut book = format!(
        "{XML_HEAD}<workbook xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_REL}\"><bookViews><workbookView activeTab=\"0\"/></bookViews><sheets>"
    );
    for (at, sheet) in sheets.iter().enumerate() {
        let _ = write!(
            book,
            "<sheet name=\"{}\" sheetId=\"{}\" r:id=\"rId{}\"/>",
            xl_text(sheet.name),
            at + 1,
            at + 1
        );
    }
    book.push_str("</sheets>");
    // Excel keeps each auto-filter's range in a hidden name (PF19).
    let mut defined = String::new();
    for (at, sheet) in sheets.iter().enumerate() {
        if let Some((column, row)) = sheet.filter {
            let _ = write!(
                defined,
                "<definedName name=\"_xlnm._FilterDatabase\" localSheetId=\"{at}\" hidden=\"1\">'{}'!$A$1:${}${row}</definedName>",
                xl_text(&quoted(sheet.name)),
                xl_col(column)
            );
        }
    }
    if !defined.is_empty() {
        let _ = write!(book, "<definedNames>{defined}</definedNames>");
    }
    book.push_str("</workbook>");

    let mut book_rels = format!("{XML_HEAD}<Relationships xmlns=\"{NS_PACKAGE_REL}\">");
    for n in 1..=sheets.len() {
        let _ = write!(
            book_rels,
            "<Relationship Id=\"rId{n}\" Type=\"{NS_REL}/worksheet\" Target=\"worksheets/sheet{n}.xml\"/>"
        );
    }
    let _ = write!(
        book_rels,
        "<Relationship Id=\"rId{}\" Type=\"{NS_REL}/styles\" Target=\"styles.xml\"/></Relationships>",
        sheets.len() + 1
    );

    let mut parts: Vec<(String, String)> = vec![
        ("[Content_Types].xml".to_owned(), types),
        ("_rels/.rels".to_owned(), rels),
        ("docProps/core.xml".to_owned(), core),
        ("docProps/app.xml".to_owned(), app),
        ("xl/workbook.xml".to_owned(), book),
        ("xl/_rels/workbook.xml.rels".to_owned(), book_rels),
        ("xl/styles.xml".to_owned(), STYLES.to_owned()),
    ];
    for (at, sheet) in sheets.iter().enumerate() {
        parts.push((
            format!("xl/worksheets/sheet{}.xml", at + 1),
            sheet.xml(at == 0),
        ));
    }
    let files: Vec<(&str, &[u8])> = parts
        .iter()
        .map(|(name, xml)| (name.as_str(), xml.as_bytes()))
        .collect();
    stored_zip(&files, context.generated)
}
