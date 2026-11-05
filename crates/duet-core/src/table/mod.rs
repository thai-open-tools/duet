//! Table (CSV/TSV) compare: parsing, and later pairing, navigation and reports.

pub mod canon;
pub mod compare;
pub mod nav;
pub mod parse;
pub mod report;
pub mod xlsx;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delimiter {
    Comma,
    Tab,
    Semicolon,
    Pipe,
}

impl Delimiter {
    pub const ALL: [Delimiter; 4] = [
        Delimiter::Comma,
        Delimiter::Tab,
        Delimiter::Semicolon,
        Delimiter::Pipe,
    ];

    pub fn char(self) -> char {
        match self {
            Delimiter::Comma => ',',
            Delimiter::Tab => '\t',
            Delimiter::Semicolon => ';',
            Delimiter::Pipe => '|',
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Delimiter::Comma => ",",
            Delimiter::Tab => "tab",
            Delimiter::Semicolon => ";",
            Delimiter::Pipe => "|",
        }
    }

    pub fn from_key(key: &str) -> Option<Delimiter> {
        Delimiter::ALL.into_iter().find(|d| d.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Delimiter::Comma => crate::i18n::tr_key("Comma (,)"),
            Delimiter::Tab => crate::i18n::tr_key("Tab"),
            Delimiter::Semicolon => crate::i18n::tr_key("Semicolon (;)"),
            Delimiter::Pipe => crate::i18n::tr_key("Pipe (|)"),
        }
    }
}

/// Every record of one file. Cell text lives in one arena String with u32 end offsets, so a
/// 160 MB file is not millions of Strings.
#[derive(Debug, Clone)]
pub struct Table {
    pub delimiter: Delimiter,
    pub auto: bool,
    arena: String,
    /// End offset in `arena` of each cell, in file order.
    cell_ends: Vec<u32>,
    /// `record_cells[i]..record_cells[i + 1]` are record i's cells; one sentinel more than records.
    record_cells: Vec<u32>,
    /// 1-based file line where each record starts.
    lines: Vec<u32>,
    /// 1-based file line where each record ends (after the newlines inside its cells).
    end_lines: Vec<u32>,
    width: usize,
    /// A workbook's text cells that read like another kind of cell carry a leading `'` (`xlsx`).
    text_marked: bool,
}

impl Table {
    /// Records, header included.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The most fields in any record.
    pub fn width(&self) -> usize {
        self.width
    }

    /// A workbook read with the text marker on: a text cell that reads like another kind of cell
    /// carries a leading `'` (`xlsx`).
    pub fn text_marked(&self) -> bool {
        self.text_marked
    }

    pub fn record(&self, index: usize) -> Record<'_> {
        assert!(index < self.len(), "record index out of range");
        Record { table: self, index }
    }

    /// The 1-based file line where record `index` starts.
    pub fn line(&self, index: usize) -> u32 {
        self.lines[index]
    }

    /// Line of the last record, 0 when empty.
    pub fn last_line(&self) -> u32 {
        self.lines.last().copied().unwrap_or(0)
    }

    /// The record holding file line `line`, counting the lines inside multi-line cells.
    pub fn record_at_line(&self, line: u32) -> Option<usize> {
        let after = self.lines.partition_point(|&start| start <= line);
        let index = after.checked_sub(1)?;
        (line <= self.end_lines[index]).then_some(index)
    }

    fn cell(&self, cell_index: usize) -> &str {
        let start = if cell_index == 0 {
            0
        } else {
            self.cell_ends[cell_index - 1] as usize
        };
        &self.arena[start..self.cell_ends[cell_index] as usize]
    }
}

#[derive(Clone, Copy)]
pub struct Record<'a> {
    table: &'a Table,
    index: usize,
}

impl<'a> Record<'a> {
    fn cell_range(&self) -> std::ops::Range<usize> {
        self.table.record_cells[self.index] as usize
            ..self.table.record_cells[self.index + 1] as usize
    }

    pub fn len(&self) -> usize {
        self.cell_range().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `None` past the record's end.
    pub fn get(&self, column: usize) -> Option<&'a str> {
        let range = self.cell_range();
        (column < range.len()).then(|| self.table.cell(range.start + column))
    }

    pub fn iter(&self) -> impl Iterator<Item = &'a str> {
        let table = self.table;
        self.cell_range().map(move |i| table.cell(i))
    }
}

/// Builds a [`Table`] one cell at a time, for sources that are not parsed from text (a
/// workbook sheet). `parse` fills the same fields.
#[derive(Debug)]
pub struct TableBuilder {
    delimiter: Delimiter,
    auto: bool,
    arena: String,
    cell_ends: Vec<u32>,
    record_cells: Vec<u32>,
    lines: Vec<u32>,
    end_lines: Vec<u32>,
    width: usize,
    /// Start of the record being built, in `cell_ends`.
    open_from: usize,
    text_marked: bool,
}

impl TableBuilder {
    pub fn new(delimiter: Delimiter, auto: bool) -> TableBuilder {
        TableBuilder {
            delimiter,
            auto,
            arena: String::new(),
            cell_ends: Vec::new(),
            record_cells: vec![0],
            lines: Vec::new(),
            end_lines: Vec::new(),
            width: 0,
            open_from: 0,
            text_marked: false,
        }
    }

    /// The table's text cells carry the marker.
    pub fn mark_text(&mut self, marked: bool) {
        self.text_marked = marked;
    }

    /// Adds a cell to the record being built.
    pub fn push_cell(&mut self, text: &str) {
        self.arena.push_str(text);
        let end =
            u32::try_from(self.arena.len()).expect("table text stays under 4 GiB (callers cap it)");
        self.cell_ends.push(end);
    }

    /// Closes the record of the cells pushed since the last call. `line` is its 1-based line
    /// (start and end alike: a cell here never spans lines).
    pub fn end_record(&mut self, line: u32) {
        debug_assert!(
            self.cell_ends.len() > self.open_from,
            "a record needs a cell"
        );
        self.width = self.width.max(self.cell_ends.len() - self.open_from);
        self.open_from = self.cell_ends.len();
        self.record_cells.push(self.open_from as u32);
        self.lines.push(line);
        self.end_lines.push(line);
    }

    /// Cells pushed so far, closed records and the open one together.
    pub fn cells(&self) -> usize {
        self.cell_ends.len()
    }

    /// Bytes of cell text pushed so far.
    pub fn text_len(&self) -> usize {
        self.arena.len()
    }

    /// The table of the closed records; cells pushed after the last `end_record` are dropped.
    pub fn finish(mut self) -> Table {
        self.cell_ends.truncate(self.open_from);
        self.arena
            .truncate(self.cell_ends.last().map_or(0, |&end| end as usize));
        Table {
            delimiter: self.delimiter,
            auto: self.auto,
            arena: self.arena,
            cell_ends: self.cell_ends,
            record_cells: self.record_cells,
            lines: self.lines,
            end_lines: self.end_lines,
            width: self.width,
            text_marked: self.text_marked,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::parse::parse_str;

    fn rows(t: &Table) -> Vec<Vec<&str>> {
        (0..t.len()).map(|i| t.record(i).iter().collect()).collect()
    }

    #[test]
    fn builder_makes_the_table_parse_would() {
        let mut b = TableBuilder::new(Delimiter::Comma, false);
        for (cells, line) in [(["id", "name"], 1), (["1", "Ann"], 2), (["3", "Cy"], 5)] {
            for c in cells {
                b.push_cell(c);
            }
            b.end_record(line);
        }
        let built = b.finish();
        let parsed = parse_str(
            "id,name
1,Ann


3,Cy
",
            Delimiter::Comma,
        );
        assert_eq!(built.len(), 3);
        assert_eq!(built.width(), 2);
        assert_eq!(rows(&built), rows(&parsed));
        assert_eq!((0..3).map(|i| built.line(i)).collect::<Vec<_>>(), [1, 2, 5]);
        assert_eq!(built.last_line(), 5);
        assert_eq!(built.record_at_line(4), None);
        assert_eq!(built.record_at_line(5), Some(2));
    }

    #[test]
    fn builder_keeps_ragged_records_and_empty_cells() {
        let mut b = TableBuilder::new(Delimiter::Comma, true);
        b.push_cell("a");
        b.end_record(1);
        for c in ["", "", "x"] {
            b.push_cell(c);
        }
        b.end_record(2);
        let t = b.finish();
        assert_eq!(t.width(), 3);
        assert!(t.auto);
        assert_eq!(t.record(1).get(0), Some(""));
        assert_eq!(t.record(0).get(1), None);
    }

    #[test]
    fn builder_reports_its_size() {
        let mut b = TableBuilder::new(Delimiter::Comma, false);
        b.push_cell("ab");
        b.push_cell("c");
        assert_eq!((b.cells(), b.text_len()), (2, 3));
    }

    #[test]
    fn finish_drops_an_unclosed_record() {
        let mut b = TableBuilder::new(Delimiter::Comma, false);
        b.push_cell("a");
        b.end_record(1);
        b.push_cell("half");
        let t = b.finish();
        assert_eq!((t.len(), rows(&t)), (1, vec![vec!["a"]]));
    }
}
