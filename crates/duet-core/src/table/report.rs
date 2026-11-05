//! The two table reports: a differences CSV and a self-contained HTML page.

use std::borrow::Cow;

use crate::i18n::{scoped_lang, tr, tr_key, trf};
use crate::table::compare::{CompareOptions, RowState, TableDiff, data_offset};
use crate::table::nav::side_value;
use crate::table::{Record, Table};

/// The most differing rows an HTML report lists.
pub const REPORT_MAX_ROWS: usize = 20_000;

/// Quoted when it holds the delimiter, `"`, CR or LF.
pub fn csv_cell(value: &str, delimiter: char) -> Cow<'_, str> {
    if value.contains([delimiter, '"', '\r', '\n']) {
        Cow::Owned(format!("\"{}\"", value.replace('"', "\"\"")))
    } else {
        Cow::Borrowed(value)
    }
}

pub fn record_csv(record: Record<'_>, delimiter: char) -> String {
    let mut out = String::new();
    for (i, cell) in record.iter().enumerate() {
        if i > 0 {
            out.push(delimiter);
        }
        out.push_str(&csv_cell(cell, delimiter));
    }
    out
}

fn csv_line(out: &mut String, cells: &[&str]) {
    for (i, cell) in cells.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&csv_cell(cell, ','));
    }
    out.push_str("\r\n");
}

/// The key columns' cells joined by " | ": the left row's when there is one, else the right's.
fn key_text(diff: &TableDiff, left: &Table, right: &Table, header: bool, row: usize) -> String {
    let has_left = diff.rows[row].left.is_some();
    diff.keys
        .iter()
        .map(|&c| {
            let (table, is_left) = if has_left {
                (left, true)
            } else {
                (right, false)
            };
            side_value(diff, Some(table), header, row, c, is_left).unwrap_or("")
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// File line of a paired row's side, as text; empty without a row.
fn line_text(table: &Table, header: bool, data: Option<u32>) -> String {
    data.map(|d| {
        table
            .line(d as usize + data_offset(table, header))
            .to_string()
    })
    .unwrap_or_default()
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub struct ReportSide<'a> {
    pub table: &'a Table,
    pub name: &'a str,
    /// e.g. `C:\a.csv (UTF-8, comma (,) auto)`
    pub describe: String,
}

/// What a report says beside its rows, in the report's language.
#[derive(Debug, Clone, Default)]
pub struct ReportNotes {
    /// Of the left side (for a CSV: its "Left value" column).
    pub left: Vec<String>,
    /// Of the right side (for a CSV: its "Right value" column).
    pub right: Vec<String>,
    /// Of the pair.
    pub both: Vec<String>,
    /// What the status says of a result with no differences that is not the whole story (the
    /// rows read are identical, the rest was not read). Said where the report would say "No
    /// differing rows."
    pub verdict: Option<String>,
}

pub fn differences_csv(diff: &TableDiff, left: &Table, right: &Table, header: bool) -> String {
    differences_csv_with(diff, left, right, header, &ReportNotes::default())
}

/// [`differences_csv`] with a `Note` row for each note, after the header row.
pub fn differences_csv_with(
    diff: &TableDiff,
    left: &Table,
    right: &Table,
    header: bool,
    notes: &ReportNotes,
) -> String {
    let mut out = String::from('\u{feff}');
    csv_line(
        &mut out,
        &[
            tr("Status"),
            tr("Left row"),
            tr("Right row"),
            tr("Key"),
            tr("Column"),
            tr("Left value"),
            tr("Right value"),
        ],
    );
    let note = |out: &mut String, left: &str, right: &str| {
        csv_line(out, &[tr("Note"), "", "", "", "", left, right]);
    };
    if let Some(verdict) = &notes.verdict {
        note(&mut out, verdict, "");
    }
    for text in &notes.left {
        note(&mut out, text, "");
    }
    for text in &notes.right {
        note(&mut out, "", text);
    }
    for text in &notes.both {
        note(&mut out, text, "");
    }
    for column in &diff.columns {
        if column.only_left() {
            csv_line(
                &mut out,
                &[tr("Left-only column"), "", "", "", &column.name, "", ""],
            );
        } else if column.only_right() {
            csv_line(
                &mut out,
                &[tr("Right-only column"), "", "", "", &column.name, "", ""],
            );
        }
    }
    for (index, row) in diff.rows.iter().enumerate() {
        if row.state == RowState::Same {
            continue;
        }
        let ln = line_text(left, header, row.left);
        let rn = line_text(right, header, row.right);
        let key = key_text(diff, left, right, header, index);
        match row.state {
            RowState::Differs => {
                for &c in diff.diff_columns(index) {
                    let c = c as usize;
                    let a = side_value(diff, Some(left), header, index, c, true).unwrap_or("");
                    let b = side_value(diff, Some(right), header, index, c, false).unwrap_or("");
                    csv_line(
                        &mut out,
                        &[tr("Differs"), &ln, &rn, &key, &diff.columns[c].name, a, b],
                    );
                }
            }
            RowState::LeftOnly => {
                let whole = record_csv(
                    left.record(row.left.unwrap() as usize + data_offset(left, header)),
                    ',',
                );
                csv_line(
                    &mut out,
                    &[tr("Left only"), &ln, "", &key, tr(WHOLE_ROW), &whole, ""],
                );
            }
            RowState::RightOnly => {
                let whole = record_csv(
                    right.record(row.right.unwrap() as usize + data_offset(right, header)),
                    ',',
                );
                csv_line(
                    &mut out,
                    &[tr("Right only"), "", &rn, &key, tr(WHOLE_ROW), "", &whole],
                );
            }
            RowState::Same => {}
        }
    }
    out
}

const WHOLE_ROW: &str = tr_key("(whole row)");

const HTML_STYLE: &str = "body{font:13px/1.5 \"Segoe UI\",\"Leelawadee UI\",\"Noto Sans Thai\",sans-serif;color:#232830;background:#fff;margin:24px}\
h1{font-size:18px;margin:0 0 10px}table{border-collapse:collapse}.meta td,.meta th{padding:2px 10px 2px 0;text-align:left;vertical-align:top}\
.meta th{color:#66707e;font-weight:600}.sum{margin:12px 0}.sum span{margin-right:16px}\
.g td,.g th{border:1px solid #d5dbe4;padding:2px 6px;vertical-align:top;white-space:pre-wrap;max-width:360px;font-family:Consolas,\"Leelawadee UI\",monospace;font-size:12px}\
.g th{background:#eceff4;position:sticky;top:0}.g td.n{color:#98a1ae;text-align:right}\
tr.m td{background:#fbf6ea}tr.l td{background:#fbeef0}tr.r td{background:#eaf5ef}td.x{background:#f3e3b8 !important}\
del{color:#c23d52;text-decoration:line-through}ins{color:#1e8a5c;text-decoration:none;font-weight:600}.ph{color:#98a1ae}\
.oL{color:#c23d52}.oR{color:#1e8a5c}";

pub fn html_report(
    diff: &TableDiff,
    left: &ReportSide,
    right: &ReportSide,
    opts: &CompareOptions,
    generated: &str,
) -> String {
    html_report_with(diff, left, right, opts, generated, &[])
}

/// [`html_report`] with a line for each note of the pair; a side's own notes are in its `describe`.
pub fn html_report_with(
    diff: &TableDiff,
    left: &ReportSide,
    right: &ReportSide,
    opts: &CompareOptions,
    generated: &str,
    notes: &[String],
) -> String {
    html_report_full(diff, left, right, opts, generated, notes, None)
}

/// [`html_report_with`] and the `verdict` to say instead of "No differing rows." when no row
/// differs but the result is not the whole story.
pub fn html_report_full(
    diff: &TableDiff,
    left: &ReportSide,
    right: &ReportSide,
    opts: &CompareOptions,
    generated: &str,
    notes: &[String],
    verdict: Option<&str>,
) -> String {
    let header = opts.header;
    let (lt, rt) = (left.table, right.table);
    let mut parts: Vec<String> = Vec::new();
    if diff.keys.is_empty() {
        parts.push(tr("Aligned by row order").into());
    } else {
        let names: Vec<&str> = diff
            .keys
            .iter()
            .map(|&c| diff.columns[c].name.as_str())
            .collect();
        parts.push(trf("Key: {}", &[&names.join(", ")]));
    }
    if header {
        parts.push(tr("First row is a header").into());
    }
    if opts.rules.ignore_case {
        parts.push(tr("Ignore case").into());
    }
    if opts.rules.trim {
        parts.push(tr("Trim spaces").into());
    }
    if opts.rules.numbers {
        parts.push(tr("Compare numbers by value").into());
    }

    let c = &diff.counts;
    let mut out = String::new();
    let (lang, title) = (
        scoped_lang().unwrap_or_default().code(),
        tr("Table compare report"),
    );
    out.push_str(&format!(
        "<!DOCTYPE html><html lang=\"{lang}\"><head><meta charset=\"utf-8\"><title>{title} - {} \u{2194} {}</title><style>{HTML_STYLE}</style></head><body>",
        escape(left.name),
        escape(right.name)
    ));
    out.push_str(&format!(
        "<h1>{title}</h1><table class=\"meta\"><tr><th>{}</th><td>{}</td></tr><tr><th>{}</th><td>{}</td></tr><tr><th>{}</th><td>{}</td></tr><tr><th>{}</th><td>{}</td></tr>",
        tr("Left"),
        escape(&left.describe),
        tr("Right"),
        escape(&right.describe),
        tr("Options"),
        escape(&parts.join(" \u{b7} ")),
        tr("Generated"),
        escape(generated)
    ));
    for note in notes {
        out.push_str(&format!(
            "<tr><th>{}</th><td>{}</td></tr>",
            tr("Note"),
            escape(note)
        ));
    }
    out.push_str("</table>");
    out.push_str(&format!(
        "<div class=\"sum\"><span>{} <b>{}</b></span><span>{} <b>{}</b></span><span>{} <b>{}</b></span><span>{} <b>{}</b></span><span>{} <b>{}</b></span><span>{} <b>{}</b></span><span>{} <b>{}</b></span></div>",
        tr("Same"),
        c.same,
        tr("Differs"),
        c.differs,
        tr("Left only"),
        c.left_only,
        tr("Right only"),
        c.right_only,
        tr("Cells differ"),
        c.cells,
        tr("Left-only columns"),
        diff.left_only_columns,
        tr("Right-only columns"),
        diff.right_only_columns
    ));
    out.push_str(&format!(
        "<table class=\"g\"><thead><tr><th>{}</th><th>{}</th><th>{}</th>",
        tr("Status"),
        tr("Left row"),
        tr("Right row")
    ));
    for column in &diff.columns {
        let class = if column.only_left() {
            format!(" class=\"oL\" title=\"{}\"", tr("Left only"))
        } else if column.only_right() {
            format!(" class=\"oR\" title=\"{}\"", tr("Right only"))
        } else {
            String::new()
        };
        out.push_str(&format!("<th{class}>{}</th>", escape(&column.name)));
    }
    out.push_str("</tr></thead><tbody>");

    let total = c.differs + c.left_only + c.right_only;
    let mut shown = 0;
    for (index, row) in diff.rows.iter().enumerate() {
        if shown >= REPORT_MAX_ROWS {
            break;
        }
        let (class, status) = match row.state {
            RowState::Same => continue,
            RowState::Differs => ("m", tr("Differs")),
            RowState::LeftOnly => ("l", tr("Left only")),
            RowState::RightOnly => ("r", tr("Right only")),
        };
        shown += 1;
        out.push_str(&format!(
            "<tr class=\"{class}\"><td>{status}</td><td class=\"n\">{}</td><td class=\"n\">{}</td>",
            line_text(lt, header, row.left),
            line_text(rt, header, row.right)
        ));
        let changed = diff.diff_columns(index);
        for column in 0..diff.columns.len() {
            let a = side_value(diff, Some(lt), header, index, column, true);
            let b = side_value(diff, Some(rt), header, index, column, false);
            if row.state == RowState::Differs && changed.contains(&(column as u32)) {
                out.push_str(&format!(
                    "<td class=\"x\"><del>{}</del>\n<ins>{}</ins></td>",
                    escape(a.unwrap_or("")),
                    escape(b.unwrap_or(""))
                ));
            } else {
                let value = if row.state == RowState::RightOnly {
                    b
                } else {
                    a.or(b)
                };
                match value {
                    Some(v) => out.push_str(&format!("<td>{}</td>", escape(v))),
                    None => out.push_str("<td class=\"ph\"></td>"),
                }
            }
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
    if total == 0 {
        let said = verdict.map_or_else(|| tr("No differing rows.").to_owned(), escape);
        out.push_str(&format!("<p>{said}</p>"));
    } else if total > shown {
        out.push_str(&format!(
            "<p>{}</p>",
            trf(
                "Showing {} of {} differing rows.",
                &[&thousands(shown), &thousands(total)]
            )
        ));
    }
    out.push_str("</body></html>");
    out
}

/// `table-diff-{left stem}-vs-{right stem}.csv|html`, safe as a file name.
pub fn export_name(left_name: &str, right_name: &str, html: bool) -> String {
    format!(
        "table-diff-{}-vs-{}.{}",
        stem(left_name),
        stem(right_name),
        if html { "html" } else { "csv" }
    )
}

fn stem(name: &str) -> String {
    let base = match name.rfind('.') {
        Some(dot) if !name[dot..].contains(['\\', '/']) => &name[..dot],
        _ => name,
    };
    let mut out = String::new();
    let mut in_run = false;
    for c in base.chars() {
        let unsafe_char = c.is_whitespace() || "\\/:*?\"<>|".contains(c);
        if unsafe_char {
            if !in_run {
                out.push('_');
            }
        } else {
            out.push(c);
        }
        in_run = unsafe_char;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancelToken;
    use crate::table::Delimiter;
    use crate::table::compare::compare;
    use crate::table::parse::parse_str;
    use proptest::prelude::*;

    fn csv(text: &str) -> Table {
        parse_str(text, Delimiter::Comma)
    }

    fn keyed() -> CompareOptions {
        CompareOptions {
            header: true,
            keys: "id".into(),
            ..Default::default()
        }
    }

    fn run(l: &Table, r: &Table, opts: &CompareOptions) -> TableDiff {
        compare(Some(l), Some(r), opts, &CancelToken::new()).unwrap()
    }

    fn side<'a>(table: &'a Table, name: &'a str) -> ReportSide<'a> {
        ReportSide {
            table,
            name,
            describe: format!("C:\\{name} (UTF-8, comma (,) auto)"),
        }
    }

    const L: &str = "id,v\n1,a\n2,b\n3,c\n3,x\n";
    const R: &str = "id,v\n3,c\n1,A\n4,d\n3,y\n";

    fn rows(t: &Table) -> Vec<Vec<String>> {
        (0..t.len())
            .map(|i| t.record(i).iter().map(str::to_string).collect())
            .collect()
    }

    fn record_csv_strs(cells: &[String], delimiter: char) -> String {
        cells
            .iter()
            .map(|c| csv_cell(c, delimiter).into_owned())
            .collect::<Vec<_>>()
            .join(&delimiter.to_string())
    }

    #[test]
    fn csv_cells_are_quoted_only_when_needed() {
        assert_eq!(csv_cell("a", ','), "a");
        assert_eq!(csv_cell("a,b", ','), "\"a,b\"");
        assert_eq!(csv_cell("say \"hi\"", ','), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_cell("x\ny", ','), "\"x\ny\"");
        assert_eq!(csv_cell("x\ry", ','), "\"x\ry\"");
        assert_eq!(csv_cell("a;b", ';'), "\"a;b\"");
        assert_eq!(csv_cell("a;b", ','), "a;b");
    }

    #[test]
    fn a_record_joins_its_cells_with_the_delimiter() {
        let t = csv("a,\"b,c\",d\n");
        assert_eq!(record_csv(t.record(0), ','), "a,\"b,c\",d");
        assert_eq!(record_csv(t.record(0), ';'), "a;b,c;d");
    }

    #[test]
    fn the_differences_csv_matches_localdiff() {
        let (l, r) = (csv(L), csv(R));
        let d = run(&l, &r, &keyed());
        assert_eq!(
            differences_csv(&d, &l, &r, true),
            "\u{feff}Status,Left row,Right row,Key,Column,Left value,Right value\r\n\
             Differs,2,3,1,v,a,A\r\n\
             Right only,,4,4,(whole row),,\"4,d\"\r\n\
             Left only,3,,2,(whole row),\"2,b\",\r\n\
             Differs,5,5,3,v,x,y\r\n"
        );
    }

    #[test]
    fn one_sided_columns_are_listed_in_column_order_before_rows() {
        let (l, r) = (csv("a,b\n1,2\n"), csv("a,c\n1,2\n"));
        let d = run(
            &l,
            &r,
            &CompareOptions {
                header: true,
                ..Default::default()
            },
        );
        assert_eq!(
            differences_csv(&d, &l, &r, true),
            "\u{feff}Status,Left row,Right row,Key,Column,Left value,Right value\r\n\
             Right-only column,,,,c,,\r\n\
             Left-only column,,,,b,,\r\n"
        );
    }

    fn html(l: &str, r: &str, opts: &CompareOptions) -> String {
        let (lt, rt) = (csv(l), csv(r));
        let d = run(&lt, &rt, opts);
        html_report(
            &d,
            &side(&lt, "a.csv"),
            &side(&rt, "b.csv"),
            opts,
            "2026-10-17 10:00",
        )
    }

    #[test]
    fn the_html_report_escapes_and_caps() {
        let page = html(L, R, &keyed());
        assert!(page.contains("Table compare report"));
        assert!(page.contains("C:\\a.csv (UTF-8, comma (,) auto)"));
        assert!(page.contains("C:\\b.csv"));
        assert!(page.contains("Key: id · First row is a header"));
        assert!(page.contains("2026-10-17 10:00"));
        assert!(page.contains("<del>a</del>\n<ins>A</ins>"));

        let page = html("id,v\n1,<b>\n", "id,v\n1,x\n", &keyed());
        assert!(page.contains("&lt;b&gt;"));
        assert!(!page.contains("<b></del>"));

        let n = REPORT_MAX_ROWS + 1;
        let left: String =
            "id\n".to_string() + &(0..n).map(|i| format!("{i}\n")).collect::<String>();
        let page = html(&left, "id\n", &keyed());
        assert_eq!(page.matches("<tr class=").count(), REPORT_MAX_ROWS);
        assert!(page.contains("Showing 20,000 of 20,001 differing rows."));

        let page = html("id\n1\n", "id\n1\n", &keyed());
        assert!(page.contains("No differing rows."));
        assert!(!page.contains("Showing"));
    }

    #[test]
    fn a_verdict_stands_where_no_differing_rows_would() {
        let (lt, rt) = (
            csv("id
1
"),
            csv("id
1
"),
        );
        let opts = keyed();
        let d = run(&lt, &rt, &opts);
        let (a, b) = (side(&lt, "a.csv"), side(&rt, "b.csv"));
        let said = "The rows read are identical. The rest of the sheet was not read.";
        let page = html_report_full(&d, &a, &b, &opts, "2026-10-17 10:00", &[], Some(said));
        assert!(
            page.contains(said) && !page.contains("No differing rows."),
            "{page}"
        );
        let page = html_report_full(&d, &a, &b, &opts, "2026-10-17 10:00", &[], None);
        assert!(page.contains("No differing rows."), "{page}");
        let notes = ReportNotes {
            verdict: Some(said.to_owned()),
            ..ReportNotes::default()
        };
        let text = differences_csv_with(&d, &lt, &rt, true, &notes);
        assert!(text.contains(&format!("Note,,,,,{said},")), "{text}");
    }

    #[test]
    fn export_names_are_safe() {
        assert_eq!(
            export_name("my data.csv", "b:c?.tsv", false),
            "table-diff-my_data-vs-b_c_.csv"
        );
        assert_eq!(
            export_name("a.csv", "b.csv", true),
            "table-diff-a-vs-b.html"
        );
        assert_eq!(
            export_name("dir.x/a.b.csv", "noext", false),
            "table-diff-dir.x_a.b-vs-noext.csv"
        );
    }

    fn notes() -> ReportNotes {
        ReportNotes {
            left: vec!["Left note, with a comma.".to_owned()],
            right: vec!["Right note.".to_owned()],
            both: vec!["A note on the pair.".to_owned()],
            verdict: None,
        }
    }

    #[test]
    fn a_csv_says_its_notes_after_its_header() {
        let (l, r) = (csv("id,v\n1,a\n"), csv("id,v\n1,b\n"));
        let d = run(&l, &r, &keyed());
        assert_eq!(
            differences_csv_with(&d, &l, &r, true, &notes()),
            "\u{feff}Status,Left row,Right row,Key,Column,Left value,Right value\r\n\
             Note,,,,,\"Left note, with a comma.\",\r\n\
             Note,,,,,,Right note.\r\n\
             Note,,,,,A note on the pair.,\r\n\
             Differs,2,2,1,v,a,b\r\n"
        );
        assert_eq!(
            differences_csv_with(&d, &l, &r, true, &ReportNotes::default()),
            differences_csv(&d, &l, &r, true),
            "no notes, no rows"
        );
    }

    #[test]
    fn an_html_report_says_the_notes_of_the_pair() {
        let (l, r) = (csv("id\n1\n"), csv("id\n1\n"));
        let d = run(&l, &r, &keyed());
        let page = html_report_with(
            &d,
            &side(&l, "a.csv"),
            &side(&r, "b.csv"),
            &keyed(),
            "now",
            &["A <note> on the pair.".to_owned()],
        );
        assert!(
            page.contains("<tr><th>Note</th><td>A &lt;note&gt; on the pair.</td></tr>"),
            "{page}"
        );
        let plain = html_report(&d, &side(&l, "a.csv"), &side(&r, "b.csv"), &keyed(), "now");
        assert!(!plain.contains("<th>Note</th>"), "no notes, no line");
    }

    proptest! {
        /// Records written with `report::csv_cell` read back the same (spec §8 "CSV round-trips").
        #[test]
        fn csv_round_trips(records in prop::collection::vec(prop::collection::vec("[a-c ,;\"\r\n\tก-ฮ]{0,6}", 1..5), 1..20)) {
            let records: Vec<Vec<String>> = records.into_iter()
                .filter(|r| !(r.len() == 1 && r[0].is_empty()))         // a blank line is skipped (R1)
                .collect();
            let text: String = records.iter().map(|r| record_csv_strs(r, ',') + "\n").collect();
            prop_assert_eq!(rows(&parse_str(&text, Delimiter::Comma)), records);
        }
    }
}
