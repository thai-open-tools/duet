//! Table (CSV/TSV) compare: parsing, and later pairing, navigation and reports.

pub mod canon;
pub mod compare;
pub mod nav;
pub mod parse;
pub mod report;

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
            Delimiter::Comma => "Comma (,)",
            Delimiter::Tab => "Tab",
            Delimiter::Semicolon => "Semicolon (;)",
            Delimiter::Pipe => "Pipe (|)",
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
