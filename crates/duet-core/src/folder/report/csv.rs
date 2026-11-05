//! The folder CSV (LocalDiff's `buildFolderCsv`, `sheet:94-102`) and the per-file row data it
//! shares with the workbook (`rptRowData`, `sheet:48-64`).

use chrono::NaiveDateTime;

use super::lines::{LineDetails, LineInfo};
use super::{ItemState, ReportItem, attention, moved_note, sort_items, unimportant_reason};
use crate::folder::tree::Side;
use crate::i18n::{tr, trf};
use crate::table::report::csv_cell;

/// The 21 column titles; `names` are the two sides.
pub fn csv_columns(names: &[String; 2]) -> [String; 21] {
    let [a, b] = names;
    [
        tr("No.").to_owned(),
        tr("Folder").to_owned(),
        tr("File name").to_owned(),
        tr("Type").to_owned(),
        tr("Status").to_owned(),
        tr("Needs a look").to_owned(),
        tr("Changed (lines)").to_owned(),
        trf("Only in {} (lines)", &[a]),
        trf("Only in {} (lines)", &[b]),
        tr("Places").to_owned(),
        tr("Note").to_owned(),
        tr("Modified later").to_owned(),
        trf("Size {} (bytes)", &[a]),
        trf("Size {} (bytes)", &[b]),
        trf("Modified {}", &[a]),
        trf("Modified {}", &[b]),
        trf("Path {}", &[a]),
        trf("Path {}", &[b]),
        tr("Reviewer").to_owned(),
        tr("Result").to_owned(),
        tr("Reviewer note").to_owned(),
    ]
}

/// `rptCsvCell` for text: a cell that a spreadsheet would run as a formula (it starts with
/// `= + - @`, a tab or a CR, and is not a plain number) gets a `'` in front; then it is quoted
/// when it holds `,` `"` CR or LF.
pub fn csv_cell_text(value: &str) -> String {
    let guarded = value.starts_with(['=', '+', '-', '@', '\t', '\r']) && !is_plain_number(value);
    if guarded {
        csv_cell(&format!("'{value}"), ',').into_owned()
    } else {
        csv_cell(value, ',').into_owned()
    }
}

/// `^-?\d+(\.\d+)?$`.
fn is_plain_number(value: &str) -> bool {
    let value = value.strip_prefix('-').unwrap_or(value);
    let (whole, fraction) = match value.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (value, None),
    };
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    digits(whole) && fraction.is_none_or(digits)
}

/// The time on the user's clock, not UTC, in a form Excel reads as a date.
pub fn local_time_text(time: NaiveDateTime) -> String {
    time.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// "\u{feff}" + header + one row per item + a final CRLF. Items are sorted as the other reports do.
pub fn folder_csv(
    items: &[ReportItem],
    lines: Option<&LineDetails>,
    names: &[String; 2],
) -> String {
    let mut sorted = items.to_vec();
    sort_items(&mut sorted);
    let mut rows: Vec<String> = vec![
        csv_columns(names)
            .iter()
            .map(|title| csv_cell_text(title))
            .collect::<Vec<_>>()
            .join(","),
    ];
    for (index, item) in sorted.iter().enumerate() {
        let d = row_data(item, index, names, lines);
        let number = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
        let size = |n: Option<u64>| n.map(|n| n.to_string()).unwrap_or_default();
        let time = |t: Option<NaiveDateTime>| t.map(local_time_text).unwrap_or_default();
        let cells = [
            d.no.to_string(),
            csv_cell_text(&d.folder),
            csv_cell_text(&d.name),
            csv_cell_text(&d.ext),
            csv_cell_text(&d.state),
            csv_cell_text(&d.attention),
            number(d.changed),
            number(d.left_only),
            number(d.right_only),
            number(d.places),
            csv_cell_text(&d.note),
            csv_cell_text(&d.newer),
            size(d.left_size),
            size(d.right_size),
            csv_cell_text(&time(d.left_time)),
            csv_cell_text(&time(d.right_time)),
            csv_cell_text(&d.left_path),
            csv_cell_text(&d.right_path),
            String::new(),
            String::new(),
            String::new(),
        ];
        rows.push(cells.join(","));
    }
    format!("\u{feff}{}\r\n", rows.join("\r\n"))
}

/// One file's cells (`rptRowData`), shared with the workbook. `index` is 0-based.
pub struct RowData {
    pub no: usize,
    pub folder: String,
    pub name: String,
    pub ext: String,
    pub state: String,
    pub attention: String,
    pub changed: Option<u32>,
    pub left_only: Option<u32>,
    pub right_only: Option<u32>,
    pub places: Option<u32>,
    pub note: String,
    pub newer: String,
    pub left_size: Option<u64>,
    pub right_size: Option<u64>,
    pub left_time: Option<NaiveDateTime>,
    pub right_time: Option<NaiveDateTime>,
    pub left_path: String,
    pub right_path: String,
}

pub fn row_data(
    item: &ReportItem,
    index: usize,
    names: &[String; 2],
    lines: Option<&LineDetails>,
) -> RowData {
    let info: Option<&LineInfo> = lines.and_then(|lines| lines.files.get(&item.rel));
    let counted = info.filter(|info| info.counted);
    let attention = attention(item, names)
        .into_iter()
        .map(|(kind, extra)| match extra {
            Some(extra) => format!("{} ({extra})", kind.title()),
            None => kind.title().to_owned(),
        })
        .collect::<Vec<_>>()
        .join("; ");
    let mut notes: Vec<String> = Vec::new();
    notes.extend(unimportant_reason(item));
    if item.format_only()
        && let Some([a, b]) = item.note.as_ref().and_then(|note| note.formats)
    {
        notes.push(trf(
            "Every line matches; only the file format differs \u{2014} {}: {} / {}: {}",
            &[&names[0], &a.describe(), &names[1], &b.describe()],
        ));
    }
    if item.case_only() {
        notes.push(tr("Same contents; the names differ only in case").to_owned());
    }
    notes.extend(moved_note(item, names));
    if item.state == ItemState::Same && item.newer.is_some() && !item.unimportant() {
        notes.push(tr("Same contents but different modified times").to_owned());
    }
    let line_note = info.and_then(|info| info.note.clone());
    if let Some(note) = &line_note {
        notes.push(note.clone());
    } else if info.is_some_and(|info| !info.counted) {
        notes.push(tr("Contents not read").to_owned());
    }
    let base = item.rel.rsplit('/').next().unwrap_or(&item.rel);
    let ext = match base.rfind('.') {
        Some(dot) if dot > 0 => base[dot + 1..].to_lowercase(),
        _ => String::new(),
    };
    let folder = item
        .rel
        .rfind('/')
        .map_or(tr("(top level)"), |at| &item.rel[..at]);
    let state = match item.state {
        ItemState::Different => tr("Different").to_owned(),
        ItemState::LeftOnly => trf("Only in {}", &[&names[0]]),
        ItemState::RightOnly => trf("Only in {}", &[&names[1]]),
        ItemState::Same => tr("Same").to_owned(),
    };
    let path = |side: &Option<super::SideFacts>| {
        side.as_ref()
            .map(|side| side.path.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    RowData {
        no: index + 1,
        folder: folder.to_owned(),
        name: base.to_owned(),
        ext,
        state,
        attention,
        changed: counted.map(|info| info.changed),
        left_only: counted.map(|info| info.left_only),
        right_only: counted.map(|info| info.right_only),
        places: counted.map(|info| info.places),
        note: notes.join(" \u{b7} "),
        newer: match item.newer {
            Some(Side::Left) => names[0].clone(),
            Some(Side::Right) => names[1].clone(),
            None => String::new(),
        },
        left_size: item.left.as_ref().map(|side| side.len),
        right_size: item.right.as_ref().map(|side| side.len),
        left_time: item.left.as_ref().and_then(|side| side.modified),
        right_time: item.right.as_ref().and_then(|side| side.modified),
        left_path: path(&item.left),
        right_path: path(&item.right),
    }
}

#[cfg(test)]
mod tests;
