//! Test workbooks, written from small specs. No binary workbook is checked in. `workbook` stores
//! its parts (not deflated) with the report's own zip writer, with text inline; `excel_workbook`
//! writes what Excel saves: every part deflated, text in `xl/sharedStrings.xml`.
//!
//! Hidden from the docs like `parse_str`: the app's tests use it too.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use chrono::NaiveDate;

use crate::folder::report::zip::stored_zip;

/// One cell's content. Dates and times are serial numbers under a date or time cell style.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Text(String),
    /// A text cell holding the empty string (not the same XML as an unwritten cell).
    EmptyText,
    Num(f64),
    Bool(bool),
    Err(String),
    /// Style `yyyy-mm-dd` (built-in 14).
    Date(f64),
    /// Style `dd/mm/yyyy` (custom).
    DateAlt(f64),
    /// Style `h:mm:ss` (built-in 21).
    Time(f64),
    /// Style `[h]:mm:ss` (built-in 46).
    Duration(f64),
    /// Style `0%`: the stored value is the fraction.
    Percent(f64),
    /// Style `#,##0.00`.
    Money(f64),
    /// A cell with a style and no value.
    StyledEmpty,
    /// Text in several runs, each its own `<r>`, with a phonetic reading that is not text.
    Rich(Vec<String>),
    /// A `t="d"` cell (strict ISO workbooks) holding this text.
    IsoDate(String),
    /// A formula with its saved result, if any.
    Formula {
        text: String,
        value: Option<Box<Cell>>,
    },
}

pub fn text(s: &str) -> Cell {
    Cell::Text(s.to_string())
}

#[derive(Debug, Clone, Default)]
pub struct SheetSpec {
    pub name: String,
    pub hidden: bool,
    /// A chartsheet (no cells): listed in the workbook, not a worksheet.
    pub chart: bool,
    /// An Excel 4 macro sheet (no cells), which the reader refuses.
    pub macro4: bool,
    /// `(row, column, cell)`, both 1-based.
    pub cells: Vec<(u32, u32, Cell)>,
    pub merges: Vec<String>,
    /// The workbook uses the 1904 date system (any sheet asking for it is enough).
    pub date1904: bool,
}

impl SheetSpec {
    pub fn new(name: &str) -> SheetSpec {
        SheetSpec {
            name: name.to_string(),
            ..SheetSpec::default()
        }
    }

    /// Rows of cells from A1, `None` leaving a gap.
    pub fn rows(name: &str, rows: &[&[Option<Cell>]]) -> SheetSpec {
        let mut spec = SheetSpec::new(name);
        for (r, row) in rows.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                if let Some(cell) = cell {
                    spec.cells.push((r as u32 + 1, c as u32 + 1, cell.clone()));
                }
            }
        }
        spec
    }

    /// Rows of plain text cells.
    pub fn text_rows(name: &str, rows: &[&[&str]]) -> SheetSpec {
        let mut spec = SheetSpec::new(name);
        for (r, row) in rows.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                spec.cells.push((r as u32 + 1, c as u32 + 1, text(cell)));
            }
        }
        spec
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

fn column_name(mut col: u32) -> String {
    let mut name = Vec::new();
    while col > 0 {
        let rem = ((col - 1) % 26) as u8;
        name.push(b'A' + rem);
        col = (col - 1) / 26;
    }
    name.reverse();
    String::from_utf8(name).unwrap()
}

fn style_of(cell: &Cell) -> u8 {
    match cell {
        Cell::Date(_) => 1,
        Cell::Time(_) => 2,
        Cell::Percent(_) => 3,
        Cell::Money(_) => 4,
        Cell::Duration(_) => 5,
        Cell::DateAlt(_) => 6,
        Cell::StyledEmpty => 1,
        Cell::Formula {
            value: Some(inner), ..
        } => style_of(inner),
        _ => 0,
    }
}

/// The shared string table a sheet's text goes into, as Excel writes it.
#[derive(Default)]
struct Strings {
    entries: Vec<Vec<String>>,
    references: usize,
}

impl Strings {
    fn index_of(&mut self, runs: Vec<String>) -> usize {
        self.references += 1;
        match self.entries.iter().position(|entry| *entry == runs) {
            Some(found) => found,
            None => {
                self.entries.push(runs);
                self.entries.len() - 1
            }
        }
    }

    fn xml(&self) -> String {
        let mut xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<sst xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" count=\"{}\" uniqueCount=\"{}\">",
            self.references,
            self.entries.len()
        );
        for runs in &self.entries {
            xml.push_str("<si>");
            xml.push_str(&runs_xml(runs));
            xml.push_str("</si>");
        }
        xml.push_str("</sst>");
        xml
    }
}

/// One `<t>`, or a `<r>` for each run and a phonetic reading when there are several.
fn runs_xml(runs: &[String]) -> String {
    let piece = |s: &String| format!("<t xml:space=\"preserve\">{}</t>", escape(s));
    match runs {
        [one] => piece(one),
        _ => {
            let mut xml = String::new();
            for (i, run) in runs.iter().enumerate() {
                let font = if i == 0 { "<rPr><b/></rPr>" } else { "" };
                xml.push_str(&format!("<r>{font}{}</r>", piece(run)));
            }
            xml.push_str("<rPh sb=\"0\" eb=\"1\"><t>phonetic</t></rPh>");
            xml
        }
    }
}

/// `(t attribute, content of <v> or <is>)` for a value; text goes into `strings` when there is a
/// table to hold it.
fn value_parts(
    cell: &Cell,
    in_formula: bool,
    strings: &mut Option<&mut Strings>,
) -> (Option<&'static str>, String) {
    let number = |v: f64| (None, format!("<v>{v}</v>"));
    match cell {
        Cell::Text(s) if in_formula => (Some("str"), format!("<v>{}</v>", escape(s))),
        Cell::Text(s) => match strings {
            Some(strings) => (
                Some("s"),
                format!("<v>{}</v>", strings.index_of(vec![s.clone()])),
            ),
            None => (
                Some("inlineStr"),
                format!("<is>{}</is>", runs_xml(std::slice::from_ref(s))),
            ),
        },
        Cell::Rich(runs) => match strings {
            Some(strings) => (
                Some("s"),
                format!("<v>{}</v>", strings.index_of(runs.clone())),
            ),
            None => (Some("inlineStr"), format!("<is>{}</is>", runs_xml(runs))),
        },
        Cell::IsoDate(s) => (Some("d"), format!("<v>{}</v>", escape(s))),
        Cell::EmptyText => (Some("inlineStr"), "<is><t></t></is>".to_string()),
        Cell::Bool(b) => (Some("b"), format!("<v>{}</v>", u8::from(*b))),
        Cell::Err(e) => (Some("e"), format!("<v>{}</v>", escape(e))),
        Cell::Num(v)
        | Cell::Date(v)
        | Cell::DateAlt(v)
        | Cell::Time(v)
        | Cell::Duration(v)
        | Cell::Percent(v)
        | Cell::Money(v) => number(*v),
        Cell::StyledEmpty | Cell::Formula { .. } => (None, String::new()),
    }
}

fn cell_xml(row: u32, col: u32, cell: &Cell, strings: &mut Option<&mut Strings>) -> String {
    let at = format!("{}{row}", column_name(col));
    let style = style_of(cell);
    match cell {
        Cell::Formula { text, value } => {
            let (t, v) = match value {
                Some(inner) => value_parts(inner, true, strings),
                None => (None, String::new()),
            };
            let t = t.map(|t| format!(" t=\"{t}\"")).unwrap_or_default();
            format!(
                "<c r=\"{at}\" s=\"{style}\"{t}><f>{}</f>{v}</c>",
                escape(text)
            )
        }
        _ => {
            let (t, v) = value_parts(cell, false, strings);
            let t = t.map(|t| format!(" t=\"{t}\"")).unwrap_or_default();
            format!("<c r=\"{at}\" s=\"{style}\"{t}>{v}</c>")
        }
    }
}

pub fn sheet_xml(spec: &SheetSpec) -> String {
    sheet_xml_with(spec, &mut None)
}

fn sheet_xml_with(spec: &SheetSpec, strings: &mut Option<&mut Strings>) -> String {
    let mut rows: BTreeMap<u32, BTreeMap<u32, &Cell>> = BTreeMap::new();
    for (r, c, cell) in &spec.cells {
        rows.entry(*r).or_default().insert(*c, cell);
    }
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>",
    );
    for (r, cells) in &rows {
        write!(xml, "<row r=\"{r}\">").unwrap();
        for (c, cell) in cells {
            xml.push_str(&cell_xml(*r, *c, cell, strings));
        }
        xml.push_str("</row>");
    }
    xml.push_str("</sheetData>");
    if !spec.merges.is_empty() {
        write!(xml, "<mergeCells count=\"{}\">", spec.merges.len()).unwrap();
        for m in &spec.merges {
            write!(xml, "<mergeCell ref=\"{m}\"/>").unwrap();
        }
        xml.push_str("</mergeCells>");
    }
    xml.push_str("</worksheet>");
    xml
}

const STYLES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\
<numFmts count=\"2\"><numFmt numFmtId=\"164\" formatCode=\"#,##0.00\"/>\
<numFmt numFmtId=\"165\" formatCode=\"dd/mm/yyyy\"/></numFmts>\
<cellXfs count=\"7\">\
<xf numFmtId=\"0\"/><xf numFmtId=\"14\"/><xf numFmtId=\"21\"/><xf numFmtId=\"9\"/>\
<xf numFmtId=\"164\"/><xf numFmtId=\"46\"/><xf numFmtId=\"165\"/>\
</cellXfs></styleSheet>";

/// A workbook with the given sheets.
pub fn workbook(sheets: &[SheetSpec]) -> Vec<u8> {
    workbook_with_parts(sheets, &[])
}

/// A workbook with extra parts in the zip (`xl/vbaProject.bin` for a macro workbook); a part
/// named like a generated one replaces it (to cut a sheet short).
pub fn workbook_with_parts(sheets: &[SheetSpec], extra: &[(&str, &[u8])]) -> Vec<u8> {
    zip(&workbook_files(sheets, extra, None))
}

/// A workbook as Excel saves one: every part deflated, text through `xl/sharedStrings.xml`, the
/// date formats through `xl/styles.xml`.
pub fn excel_workbook(sheets: &[SheetSpec]) -> Vec<u8> {
    let mut strings = Strings::default();
    deflated_zip(&workbook_files(sheets, &[], Some(&mut strings)))
}

/// The parts of a workbook, each deflated, with its text inline.
pub fn deflated_workbook(sheets: &[SheetSpec]) -> Vec<u8> {
    deflated_zip(&workbook_files(sheets, &[], None))
}

fn workbook_files(
    sheets: &[SheetSpec],
    extra: &[(&str, &[u8])],
    mut strings: Option<&mut Strings>,
) -> Vec<(String, Vec<u8>)> {
    const NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let date1904 = sheets.iter().any(|s| s.date1904);
    let mut workbook = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<workbook xmlns=\"{NS}\" xmlns:r=\"{REL}\">"
    );
    if date1904 {
        workbook.push_str("<workbookPr date1904=\"1\"/>");
    }
    workbook.push_str("<sheets>");
    let mut rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdStyles\" Type=\"{REL}/styles\" Target=\"styles.xml\"/>"
    );
    if strings.is_some() {
        write!(
            rels,
            "<Relationship Id=\"rIdStrings\" Type=\"{REL}/sharedStrings\" Target=\"sharedStrings.xml\"/>"
        )
        .unwrap();
    }
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    for (i, s) in sheets.iter().enumerate() {
        let n = i + 1;
        let state = if s.hidden { " state=\"hidden\"" } else { "" };
        write!(
            workbook,
            "<sheet name=\"{}\" sheetId=\"{n}\"{state} r:id=\"rId{n}\"/>",
            escape(&s.name)
        )
        .unwrap();
        if s.chart {
            write!(
                rels,
                "<Relationship Id=\"rId{n}\" Type=\"{REL}/chartsheet\" Target=\"chartsheets/sheet{n}.xml\"/>"
            )
            .unwrap();
            parts.push((
                format!("xl/chartsheets/sheet{n}.xml"),
                format!("<chartsheet xmlns=\"{NS}\"/>").into_bytes(),
            ));
        } else if s.macro4 {
            write!(
                rels,
                "<Relationship Id=\"rId{n}\" Type=\"http://schemas.microsoft.com/office/2006/relationships/xlMacrosheet\" Target=\"macrosheets/sheet{n}.xml\"/>"
            )
            .unwrap();
            parts.push((
                format!("xl/macrosheets/sheet{n}.xml"),
                format!("<xm:macrosheet xmlns:xm=\"{NS}\"/>").into_bytes(),
            ));
        } else {
            write!(
                rels,
                "<Relationship Id=\"rId{n}\" Type=\"{REL}/worksheet\" Target=\"worksheets/sheet{n}.xml\"/>"
            )
            .unwrap();
            parts.push((
                format!("xl/worksheets/sheet{n}.xml"),
                sheet_xml_with(s, &mut strings).into_bytes(),
            ));
        }
    }
    workbook.push_str("</sheets></workbook>");
    rels.push_str("</Relationships>");
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>";
    let root_rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"{REL}/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>"
    );
    let mut files: Vec<(String, Vec<u8>)> = vec![
        (
            "[Content_Types].xml".into(),
            content_types.as_bytes().to_vec(),
        ),
        ("_rels/.rels".into(), root_rels.into_bytes()),
        ("xl/workbook.xml".into(), workbook.into_bytes()),
        ("xl/_rels/workbook.xml.rels".into(), rels.into_bytes()),
        ("xl/styles.xml".into(), STYLES.as_bytes().to_vec()),
    ];
    files.extend(parts);
    if let Some(strings) = strings {
        files.push(("xl/sharedStrings.xml".into(), strings.xml().into_bytes()));
    }
    // An extra part with the name of a generated one replaces it.
    files.retain(|(n, _)| !extra.iter().any(|(e, _)| e == n));
    files.extend(extra.iter().map(|(n, d)| (n.to_string(), d.to_vec())));
    files
}

/// A stored zip of the named parts.
pub fn zip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, d)| (n.as_str(), d.as_slice()))
        .collect();
    let when = NaiveDate::from_ymd_opt(2026, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    stored_zip(&refs, when).expect("a fixture is small")
}

/// A zip of the named parts, each deflated, as Excel writes one.
pub fn deflated_zip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    use std::io::Write as _;
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in files {
        writer
            .start_file(name.as_str(), options)
            .expect("a fixture is small");
        writer.write_all(data).expect("a fixture is small");
    }
    writer.finish().expect("a fixture is small").into_inner()
}

/// Makes the central directory of `bytes` declare `size` unpacked bytes for the part `name`; the
/// part itself is left as it is. A zip's sizes are only what its directory says.
pub fn declare_size(bytes: &mut [u8], name: &str, size: u32) {
    const SIGNATURE: &[u8] = b"PK\x01\x02";
    let at = (0..bytes.len().saturating_sub(46))
        .find(|&at| {
            bytes[at..].starts_with(SIGNATURE) && bytes[at + 46..].starts_with(name.as_bytes())
        })
        .expect("the part is in the directory");
    bytes[at + 24..at + 28].copy_from_slice(&size.to_le_bytes());
}

/// A 512-byte-sector compound file with a root entry and an `EncryptedPackage` entry: what
/// Excel writes for a password-protected workbook, without the encrypted data.
pub fn protected_bytes() -> Vec<u8> {
    let mut b = vec![0u8; 512 * 3];
    b[..8].copy_from_slice(&super::OLE_SIGNATURE);
    b[26..28].copy_from_slice(&3u16.to_le_bytes());
    b[30..32].copy_from_slice(&9u16.to_le_bytes());
    b[32..34].copy_from_slice(&6u16.to_le_bytes());
    b[44..48].copy_from_slice(&1u32.to_le_bytes()); // one FAT sector
    b[48..52].copy_from_slice(&1u32.to_le_bytes()); // directory at sector 1
    b[60..64].copy_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
    b[68..72].copy_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
    b[76..80].copy_from_slice(&0u32.to_le_bytes()); // FAT is sector 0
    for i in 1..109 {
        b[76 + 4 * i..80 + 4 * i].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    }
    let fat = 512;
    b[fat..fat + 4].copy_from_slice(&0xFFFF_FFFDu32.to_le_bytes());
    b[fat + 4..fat + 8].copy_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
    let dir = 1024;
    for (slot, name) in ["Root Entry", "EncryptedPackage"].iter().enumerate() {
        let at = dir + slot * 128;
        for (i, unit) in name.encode_utf16().enumerate() {
            b[at + 2 * i..at + 2 * i + 2].copy_from_slice(&unit.to_le_bytes());
        }
    }
    b
}
