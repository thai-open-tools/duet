//! The plain-text folder report (LocalDiff's `buildFolderReportText`, `rep:612-660`).

use std::collections::BTreeMap;

use super::lines::{DetailRow, LineDetails, RowClass, line_summary};
use super::{
    ItemState, NOT_SCANNED_HINT, ReportContext, ReportItem, ReportOptions, attention, count_text,
    report_counts, sort_items, unimportant_reason,
};
use crate::folder::tree::Side;

fn num(n: usize) -> String {
    count_text(n as u64, "", "").trim_end().to_owned()
}

/// Without the BOM; the writer adds "\u{feff}". Lines joined by "\r\n".
pub fn folder_text(
    items: &[ReportItem],
    lines: Option<&LineDetails>,
    options: &ReportOptions,
    context: &ReportContext,
) -> String {
    let names = &options.names;
    let counts = report_counts(items);
    let mut sorted = items.to_vec();
    sort_items(&mut sorted);
    let mut out: Vec<String> = Vec::new();
    out.push(if options.title.is_empty() {
        "Folder compare report".to_owned()
    } else {
        format!("Folder compare report \u{2014} {}", options.title)
    });
    // A note typed on several lines keeps the report's CRLF (M-3).
    out.extend(options.note.lines().map(str::to_owned));
    out.push(format!("A = {} : {}", names[0], context.side_paths[0]));
    out.push(format!("B = {} : {}", names[1], context.side_paths[1]));
    out.push(format!(
        "Made {} \u{b7} Criterion: {}{}",
        context.generated.format("%Y-%m-%d %H:%M"),
        context.criterion,
        if lines.is_some() && options.mask && options.details {
            " \u{b7} Values that look like passwords or keys are hidden"
        } else {
            ""
        }
    ));
    if !context.invisible_scan {
        out.push(format!(
            "Invisible characters: not scanned ({NOT_SCANNED_HINT})"
        ));
    }
    out.push(String::new());
    out.push("Summary".to_owned());
    out.push(format!(
        "  All files        {}   (mismatched {})",
        num(counts.total),
        num(counts.mismatched())
    ));
    out.push(format!(
        "  \u{2260} Different      {}",
        num(counts.different)
    ));
    out.push(format!(
        "  < Only in A      {}   ({})",
        num(counts.left_only),
        names[0]
    ));
    out.push(format!(
        "  > Only in B      {}   ({})",
        num(counts.right_only),
        names[1]
    ));
    out.push(format!(
        "  = Same           {}{}",
        num(counts.same),
        if counts.unimportant > 0 {
            format!(
                "   (including {} that differ only in unimportant parts)",
                num(counts.unimportant)
            )
        } else {
            String::new()
        }
    ));
    let mut watch: Vec<String> = Vec::new();
    for item in &sorted {
        for (kind, extra) in attention(item, names) {
            watch.push(format!(
                "  [{}] {}{}",
                kind.title(),
                item.rel,
                extra.map_or(String::new(), |extra| format!(" ({extra})"))
            ));
        }
    }
    if !watch.is_empty() {
        out.push(String::new());
        out.push("Needs a closer look".to_owned());
        out.extend(watch);
    }
    let detail: Vec<&ReportItem> = sorted
        .iter()
        .filter(|item| item.state != ItemState::Same || (options.same && !item.unimportant()))
        .collect();
    if !detail.is_empty() {
        out.push(String::new());
        out.push("Details by folder".to_owned());
        // Items are already in state order, then by lower-cased path; a folder keeps that.
        let mut by_dir: BTreeMap<&str, Vec<&ReportItem>> = BTreeMap::new();
        for item in detail {
            let dir = item.rel.rfind('/').map_or("", |at| &item.rel[..at]);
            by_dir.entry(dir).or_default().push(item);
        }
        for (dir, mut group) in by_dir {
            out.push(String::new());
            out.push(format!(
                "[{}]",
                if dir.is_empty() { "(top level)" } else { dir }
            ));
            group.sort_by(|x, y| (rank(x), &x.rel).cmp(&(rank(y), &y.rel)));
            for item in group {
                file_lines(&mut out, item, lines, options);
            }
        }
    }
    if options.unimportant {
        let unimportant: Vec<&ReportItem> =
            sorted.iter().filter(|item| item.unimportant()).collect();
        if !unimportant.is_empty() {
            out.push(String::new());
            out.push("Differs only in unimportant parts (counted as Same)".to_owned());
            for item in unimportant {
                out.push(format!(
                    "  \u{2248} {}   \u{2014} {}",
                    item.rel,
                    unimportant_reason(item).unwrap_or_default()
                ));
            }
        }
    }
    out.join("\r\n")
}

fn rank(item: &ReportItem) -> u8 {
    match item.state {
        ItemState::Different => 0,
        ItemState::LeftOnly => 1,
        ItemState::RightOnly => 2,
        ItemState::Same => 3,
    }
}

fn file_lines(
    out: &mut Vec<String>,
    item: &ReportItem,
    lines: Option<&LineDetails>,
    options: &ReportOptions,
) {
    let names = &options.names;
    let info = lines.and_then(|lines| lines.files.get(&item.rel));
    let mut why: Vec<String> = Vec::new();
    match item.state {
        ItemState::LeftOnly => why.push(format!("only in {}", names[0])),
        ItemState::RightOnly => why.push(format!("only in {}", names[1])),
        _ => {}
    }
    if let Some(info) = info {
        let summary = line_summary(info, names);
        if !summary.is_empty() {
            why.push(summary);
        }
    }
    if item.state == ItemState::Different
        && let Some(side) = item.newer
    {
        let name = if side == Side::Left {
            &names[0]
        } else {
            &names[1]
        };
        why.push(format!("{name} newer"));
    }
    if item.case_only() {
        why.push("names differ only in case".to_owned());
    } else if item.format_only()
        && let Some([a, b]) = item.note.as_ref().and_then(|note| note.formats)
    {
        why.push(format!(
            "only the file format differs ({} / {})",
            a.describe(),
            b.describe()
        ));
    }
    if let Some(note) = info.and_then(|info| info.note.as_ref()) {
        why.push(note.clone());
    }
    let symbol = match item.state {
        ItemState::Different => '\u{2260}',
        ItemState::LeftOnly => '<',
        ItemState::RightOnly => '>',
        ItemState::Same => '=',
    };
    let name = item.rel.rsplit('/').next().unwrap_or(&item.rel);
    out.push(format!(
        "  {symbol} {name}{}",
        if why.is_empty() {
            String::new()
        } else {
            format!("   \u{2014} {}", why.join(" \u{b7} "))
        }
    ));
    if !options.details {
        return;
    }
    for row in info.map_or(&[][..], |info| &info.rows[..]) {
        out.push(format!("    {}", row_text(row)));
    }
}

/// One line row as `rptDiffRows` writes it, before the 4 spaces of indent.
fn row_text(row: &DetailRow) -> String {
    match row {
        DetailRow::Skip(n) => format!("    \u{22ef} {n} same lines"),
        DetailRow::More(n) => format!("    \u{2026} {n} more rows"),
        DetailRow::Line {
            class, left, right, ..
        } => {
            let mark = match class {
                RowClass::Mod => '~',
                RowClass::Soft => '\u{2248}',
                RowClass::Del | RowClass::UDel => '-',
                RowClass::Ins | RowClass::UIns => '+',
                RowClass::Same => ' ',
            };
            match (left, right) {
                (Some((a, left)), Some((b, right)))
                    if matches!(class, RowClass::Mod | RowClass::Soft) =>
                {
                    format!("  {mark} A {a}: {left}\r\n      {mark} B {b}: {right}")
                }
                (Some((a, left)), Some((b, _))) => format!("    A {a} | B {b}: {left}"),
                (Some((a, left)), None) => format!("  {mark} A {a}: {left}"),
                (None, Some((b, right))) => format!("  {mark} B {b}: {right}"),
                (None, None) => String::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests;
