//! Text compare reports (`ops:472-561`, `de:1048-1119`): the unified diff, the text report
//! and the side-by-side HTML page, as strings the app writes off the UI thread.

use std::fmt::Write as _;
use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use crate::i18n::{scoped_lang, tr, trf};
use crate::text::diff::LineDiff;
use crate::text::document::TextDocument;
use crate::text::inline::{InlineDiff, refine};
use crate::text::lines::EolStyle;
use crate::text::odd::{OddKind, is_odd_after, odd_info};
use crate::text::rows::{RowKind, RowMap};

/// `ldUnifiedDiff`'s `nameA` / `nameB` without the `a/` and `b/` the headers add.
pub struct PatchOptions<'a> {
    pub left_name: &'a str,
    pub right_name: &'a str,
    pub context: u32,
}

/// One hunk of the patch: line ranges as signed numbers, so the context arithmetic can dip
/// below zero as `ldUnifiedDiff` does.
#[derive(Debug, Clone, Copy)]
struct PatchHunk {
    a_start: i64,
    a_end: i64,
    b_start: i64,
    b_end: i64,
}

const NO_NEWLINE: &str = "\n\\ No newline at end of file\n";

/// Whether the last line of `doc` ends with a line break (`hadNL`).
fn ends_with_newline(doc: &TextDocument) -> bool {
    let n = doc.line_count();
    n == 0 || !doc.lines().eol_bytes(doc.bytes(), n - 1).is_empty()
}

/// `ldEqPartner`: the line of the other side that line `idx` is equal to, or -1.
fn eq_partner(hunks: &[PatchHunk], idx: i64, side_a: bool, n_other: i64) -> i64 {
    let mut off = 0;
    for h in hunks {
        let (s, e) = if side_a {
            (h.a_start, h.a_end)
        } else {
            (h.b_start, h.b_end)
        };
        if s <= idx && idx < e {
            return -1;
        }
        if e <= idx {
            off = if side_a {
                h.b_end - h.a_end
            } else {
                h.a_end - h.b_end
            };
        } else {
            break;
        }
    }
    let p = idx + off;
    if (0..n_other).contains(&p) { p } else { -1 }
}

/// `ldPatchEndHunks`: a last line without a newline is context only against an equal last
/// line that lacks one too; otherwise the pair becomes a `-`/`+` hunk.
fn patch_end_hunks(
    hunks: Vec<PatchHunk>,
    n_a: i64,
    n_b: i64,
    a_nl: bool,
    b_nl: bool,
) -> Vec<PatchHunk> {
    if a_nl && b_nl {
        return hunks;
    }
    let mut add: Vec<(i64, i64)> = Vec::new();
    if n_a > 0 {
        let p = eq_partner(&hunks, n_a - 1, true, n_b);
        if p >= 0 && (if p == n_b - 1 { a_nl != b_nl } else { !a_nl }) {
            add.push((n_a - 1, p));
        }
    }
    if n_b > 0 {
        let p = eq_partner(&hunks, n_b - 1, false, n_a);
        if p >= 0
            && (if p == n_a - 1 { a_nl != b_nl } else { !b_nl })
            && add.first().is_none_or(|first| first.0 != p)
        {
            add.push((p, n_b - 1));
        }
    }
    if add.is_empty() {
        return hunks;
    }
    let mut all = hunks;
    all.extend(add.into_iter().map(|(a, b)| PatchHunk {
        a_start: a,
        a_end: a + 1,
        b_start: b,
        b_end: b + 1,
    }));
    all.sort_by_key(|h| (h.a_start, h.b_start));
    let mut merged: Vec<PatchHunk> = vec![all[0]];
    for h in &all[1..] {
        let last = merged.last_mut().expect("merged starts with one hunk");
        if h.a_start <= last.a_end && h.b_start <= last.b_end {
            last.a_end = last.a_end.max(h.a_end);
            last.b_end = last.b_end.max(h.b_end);
        } else {
            merged.push(*h);
        }
    }
    merged
}

/// The unified diff of two documents (`ldUnifiedDiff`, `de:1048-1119`): `diff` comes from a
/// plain compare with `ignore_eol` on. Content lines end with the left side's line ending
/// (CRLF when it is CRLF, else LF); headers, `@@` lines and the no-newline note end with LF.
pub fn unified_diff(
    left: &TextDocument,
    right: &TextDocument,
    diff: &LineDiff,
    o: &PatchOptions,
) -> String {
    let ctx = i64::from(o.context);
    let eol = if left.eol() == EolStyle::CrLf {
        "\r\n"
    } else {
        "\n"
    };
    let (n_a, n_b) = (left.line_count() as i64, right.line_count() as i64);
    let (a_nl, b_nl) = (ends_with_newline(left), ends_with_newline(right));
    let mut out = format!("--- a/{}\n+++ b/{}\n", o.left_name, o.right_name);
    let hunks: Vec<PatchHunk> = diff
        .hunks
        .iter()
        .map(|h| PatchHunk {
            a_start: h.left.start.into(),
            a_end: h.left.end.into(),
            b_start: h.right.start.into(),
            b_end: h.right.end.into(),
        })
        .collect();
    let hunks = patch_end_hunks(hunks, n_a, n_b, a_nl, b_nl);
    let line = |out: &mut String, doc: &TextDocument, i: i64, n: i64, nl: bool, pre: char| {
        out.push(pre);
        out.push_str(&doc.line(i as usize));
        out.push_str(if i == n - 1 && !nl { NO_NEWLINE } else { eol });
    };
    let mut h = 0;
    while h < hunks.len() {
        let (g0, mut g1) = (h, h);
        while g1 + 1 < hunks.len() && hunks[g1 + 1].a_start - hunks[g1].a_end <= 2 * ctx {
            g1 += 1;
        }
        let (first, last) = (hunks[g0], hunks[g1]);
        let a0 = (first.a_start - ctx).max(0);
        let a1 = (last.a_end + ctx).min(n_a);
        let b0 = (first.b_start - (first.a_start - a0)).max(0);
        let b1 = (last.b_end + (a1 - last.a_end)).min(n_b);
        let (la, lb) = (a1 - a0, b1 - b0);
        let range = |start: i64, len: i64| {
            let shown = if len > 0 { start + 1 } else { start };
            if len == 1 {
                shown.to_string()
            } else {
                format!("{shown},{len}")
            }
        };
        let _ = writeln!(out, "@@ -{} +{} @@", range(a0, la), range(b0, lb));
        let mut pa = a0;
        for hk in &hunks[g0..=g1] {
            for x in pa..hk.a_start {
                line(&mut out, left, x, n_a, a_nl, ' ');
            }
            for x in hk.a_start..hk.a_end {
                line(&mut out, left, x, n_a, a_nl, '-');
            }
            for x in hk.b_start..hk.b_end {
                line(&mut out, right, x, n_b, b_nl, '+');
            }
            pa = hk.a_end;
        }
        for y in pa..a1 {
            line(&mut out, left, y, n_a, a_nl, ' ');
        }
        h = g1 + 1;
    }
    out
}

pub struct ReportMeta<'a> {
    pub left: &'a str,
    pub right: &'a str,
    pub date: &'a str,
    pub status: &'a str,
    pub title: &'a str,
}

/// The plain-text report (`exportTextReport`): a BOM, CRLF line ends, one block per difference.
pub fn text_report(
    left: &TextDocument,
    right: &TextDocument,
    diff: &LineDiff,
    meta: &ReportMeta,
) -> String {
    let mut out = format!(
        "\u{feff}{}\r\n{}\r\n{}\r\n{}\r\n{}\r\n\r\n",
        tr("duet: text compare report"),
        trf("Left : {}", &[&meta.left]),
        trf("Right: {}", &[&meta.right]),
        trf("Date : {}", &[&meta.date]),
        meta.status
    );
    let place = |r: &Range<u32>| {
        if r.end > r.start {
            format!("{}-{}", r.start + 1, r.end)
        } else {
            trf("insert at {}", &[&(r.start + 1)])
        }
    };
    for (i, h) in diff.hunks.iter().enumerate() {
        let _ = write!(
            out,
            "{}\r\n",
            trf(
                "=== Difference {} · Left {} · Right {} ===",
                &[&(i + 1), &place(&h.left), &place(&h.right)]
            )
        );
        for a in h.left.clone() {
            let _ = write!(out, "< {}\r\n", left.line(a as usize));
        }
        if !h.left.is_empty() && !h.right.is_empty() {
            out.push_str("---\r\n");
        }
        for b in h.right.clone() {
            let _ = write!(out, "> {}\r\n", right.line(b as usize));
        }
        out.push_str("\r\n");
    }
    // LocalDiff joins its lines with CRLF: the last block has no blank line after it.
    if !diff.hunks.is_empty() {
        out.truncate(out.len() - 2);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportMode {
    /// The differences and this many lines around each.
    Context(u32),
    Differences,
    All,
}

/// LocalDiff has no cap; a page with more rows than this stops with a row saying so.
pub const HTML_MAX_ROWS: usize = 200_000;

/// `LD_DISPLAY_MAX`: a longer line is cut.
const LINE_MAX_CHARS: usize = 50_000;

const CSS: &str = "body{font:13px/1.5 \"Segoe UI\",sans-serif;margin:24px;color:#232830;background:#fff}\
h1{font-size:17px;margin:0 0 6px}.meta{color:#66707e;margin-bottom:14px}.meta b{color:#232830}\
table{border-collapse:collapse;width:100%;table-layout:fixed;font:12px/1.55 Consolas,\"Cascadia Mono\",monospace}\
td{vertical-align:top;padding:0 8px;white-space:pre-wrap;word-break:break-all}td.ln{width:56px;text-align:right;color:#98a1ae;background:#f5f7fa;border-right:1px solid #e2e7ee}\
tr.mod td.l,tr.mod td.r{background:#fbf3df}tr.del td.l{background:#fae6e9}tr.ins td.r{background:#e3f3ea}\
tr.soft td.l,tr.soft td.r,tr.udel td,tr.uins td{background:#f1effa}td.ph{background:repeating-linear-gradient(45deg,#fff 0 6px,#f3f4f7 6px 12px)}\
tr.mod td.l .ch,tr.del td.l .ch{background:#f2b8c1}tr.mod td.r .ch,tr.ins td.r .ch{background:#b7e3c9}.ch{border-radius:2px}\
tr.skip td{text-align:center;color:#98a1ae;background:#f5f7fa;font-family:\"Segoe UI\",sans-serif;padding:3px}\
td.l{border-right:2px solid #d5dbe4}\
.odd{color:#c0394b;background:#fae6e9;border-radius:2px}.odd.o-zero{font-size:10px}";

fn escape(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

/// One line as a report cell holds it (`ldDecorate`): escaped; `marks` (byte ranges) wrapped
/// in `<span class="ch">`; tabs expanded to spaces by grapheme columns; with `show_odd`, each
/// invisible character becomes a marker (`⍽` for a space-like one, its label otherwise).
pub fn html_line(text: &str, marks: &[Range<usize>], tab_width: usize, show_odd: bool) -> String {
    let (text, cut) = match text.char_indices().nth(LINE_MAX_CHARS) {
        Some((at, _)) => (&text[..at], true),
        None => (text, false),
    };
    let tab_width = tab_width.max(1);
    let mut out = String::with_capacity(text.len() + 16);
    let (mut column, mut next_mark, mut open) = (0usize, 0usize, false);
    let mut prev: Option<char> = None;
    for (at, grapheme) in text.grapheme_indices(true) {
        while next_mark < marks.len() && marks[next_mark].end <= at {
            next_mark += 1;
        }
        let marked = marks
            .get(next_mark)
            .is_some_and(|m| m.start <= at && at < m.end);
        if marked != open {
            out.push_str(if marked {
                "<span class=\"ch\">"
            } else {
                "</span>"
            });
            open = marked;
        }
        for ch in grapheme.chars() {
            match ch {
                '\t' => {
                    let width = tab_width - column % tab_width;
                    out.extend(std::iter::repeat_n(' ', width - 1));
                    out.push(' ');
                    column += width - 1;
                }
                _ if show_odd && is_odd_after(prev, ch) => {
                    let (label, name, kind) = odd_info(ch);
                    let class = if kind == OddKind::Space {
                        "o-space"
                    } else {
                        "o-zero"
                    };
                    let shown = if kind == OddKind::Space {
                        "⍽"
                    } else {
                        &label
                    };
                    let _ = write!(out, "<span class=\"odd {class}\" title=\"");
                    escape(&name, &mut out);
                    out.push_str("\">");
                    escape(shown, &mut out);
                    out.push_str("</span>");
                }
                _ => escape(ch.encode_utf8(&mut [0; 4]), &mut out),
            }
            prev = Some(ch);
        }
        column += 1;
    }
    if open {
        out.push_str("</span>");
    }
    if cut {
        out.push_str("<i class=\"cut\">…</i>");
    }
    out
}

/// 1234567 as "1,234,567".
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut shown = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            shown.push(',');
        }
        shown.push(c);
    }
    shown
}

/// The row ranges `mode` keeps. O(hunks): the differences come from the map's change blocks.
#[allow(clippy::single_range_in_vec_init)]
fn kept_ranges(rows: &RowMap, mode: ReportMode) -> Vec<Range<u32>> {
    let total = rows.row_count();
    let context = match mode {
        ReportMode::All => return vec![0..total],
        ReportMode::Differences => 0,
        ReportMode::Context(c) => c,
    };
    let mut kept: Vec<Range<u32>> = Vec::new();
    for block in rows.change_blocks() {
        let range =
            block.start.saturating_sub(context)..block.end.saturating_add(context).min(total);
        match kept.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => kept.push(range),
        }
    }
    kept
}

fn cell(
    body: &mut String,
    number: Option<u32>,
    text: Option<&str>,
    side: &str,
    marks: &[Range<usize>],
    tab_width: usize,
) {
    match (number, text) {
        (Some(number), Some(text)) => {
            let _ = write!(
                body,
                "<td class=\"ln\">{}</td><td class=\"{side}\">{}</td>",
                number + 1,
                html_line(text, marks, tab_width, true)
            );
        }
        _ => body.push_str("<td class=\"ln\"></td><td class=\"ph\"></td>"),
    }
}

fn skip_row(out: &mut String, text: &str) {
    let _ = write!(
        out,
        "<tr class=\"skip\"><td colspan=\"4\">⋯ {text} ⋯</td></tr>"
    );
}

/// The side-by-side page (`exportHtmlReport`): one self-contained file with no script and no
/// network. `ignoring` is true when an option or a rule is on, so a matched pair whose text
/// differs is a "soft" row. After `max_rows` rows the page stops with a row saying so.
#[allow(clippy::too_many_arguments)]
pub fn html_report(
    left: &TextDocument,
    right: &TextDocument,
    rows: &RowMap,
    mode: ReportMode,
    ignoring: bool,
    tab_width: usize,
    meta: &ReportMeta,
    max_rows: usize,
) -> String {
    let mut body = String::new();
    let mut shown = 0usize;
    let mut position = 0u32;
    let unchanged = |n: u32| {
        format!(
            "{} {}",
            thousands(n.into()),
            if n == 1 {
                tr("unchanged line")
            } else {
                tr("unchanged lines")
            }
        )
    };
    let mut stopped = false;
    'ranges: for range in kept_ranges(rows, mode) {
        let mut hidden = range.start - position;
        for index in range.clone() {
            if shown == max_rows {
                skip_row(
                    &mut body,
                    &trf(
                        "The report stops here: more than {} rows.",
                        &[&thousands(max_rows as u64)],
                    ),
                );
                stopped = true;
                break 'ranges;
            }
            if hidden > 0 {
                skip_row(&mut body, &unchanged(hidden));
                hidden = 0;
            }
            let row = rows.row(index);
            let (l, r) = (
                row.left.map(|n| left.line(n as usize)),
                row.right.map(|n| right.line(n as usize)),
            );
            let differs = row.kind == RowKind::Same && ignoring && l != r;
            let class = match row.kind {
                RowKind::Same if differs => "soft",
                RowKind::Same => "same",
                RowKind::Changed => "mod",
                RowKind::LeftOnly => "del",
                RowKind::RightOnly => "ins",
                RowKind::SoftLeft => "udel",
                RowKind::SoftRight => "uins",
            };
            let marks = match (&l, &r) {
                (Some(l), Some(r)) if row.kind == RowKind::Changed || differs => refine(l, r),
                _ => InlineDiff::default(),
            };
            let _ = write!(body, "<tr class=\"{class}\">");
            cell(
                &mut body,
                row.left,
                l.as_deref(),
                "l",
                &marks.left,
                tab_width,
            );
            cell(
                &mut body,
                row.right,
                r.as_deref(),
                "r",
                &marks.right,
                tab_width,
            );
            body.push_str("</tr>");
            shown += 1;
        }
        position = range.end;
    }
    if !stopped && position < rows.row_count() {
        skip_row(&mut body, &unchanged(rows.row_count() - position));
    }
    let mut head = String::new();
    escape(meta.title, &mut head);
    let (mut l, mut r, mut status, mut date) =
        (String::new(), String::new(), String::new(), String::new());
    escape(meta.left, &mut l);
    escape(meta.right, &mut r);
    escape(meta.status, &mut status);
    escape(meta.date, &mut date);
    let lang = scoped_lang().unwrap_or_default().code();
    let (title, left_label, right_label) = (tr("Text compare report"), tr("Left:"), tr("Right:"));
    let made = trf("Made {} with duet", &[&date]);
    format!(
        "<!DOCTYPE html><html lang=\"{lang}\"><head><meta charset=\"utf-8\"><title>duet — {head}</title><style>{CSS}</style></head><body>\
<h1>{title}</h1><div class=\"meta\">{left_label} <b>{l}</b><br>{right_label} <b>{r}</b><br>{status}<br>{made}</div>\
<table><colgroup><col style=\"width:56px\"><col><col style=\"width:56px\"><col></colgroup>{body}</table></body></html>"
    )
}

/// `reportName`: `diff-{left stem}-vs-{right stem}{ext}`. A side's stem is its file name
/// without the last extension, `left` / `right` when it has none; each run of `\ / : * ? " < > |`
/// becomes `_` and the name is cut to 100 characters before the extension.
pub fn report_name(left: &str, right: &str, ext: &str) -> String {
    let stem = |name: &str, fallback: &str| {
        let base = name.rsplit(['\\', '/']).next().unwrap_or("");
        if base.is_empty() {
            return fallback.to_owned();
        }
        match base.rfind('.') {
            Some(dot) if dot > 0 && dot + 1 < base.len() => base[..dot].to_owned(),
            _ => base.to_owned(),
        }
    };
    let joined = format!("diff-{}-vs-{}", stem(left, "left"), stem(right, "right"));
    let mut name = String::with_capacity(joined.len());
    let mut in_run = false;
    for ch in joined.chars() {
        let bad = matches!(ch, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|');
        if bad {
            if !in_run {
                name.push('_');
            }
        } else {
            name.push(ch);
        }
        in_run = bad;
    }
    name.chars().take(100).chain(ext.chars()).collect()
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;
    use crate::cancel::CancelToken;
    use crate::text::diff::{DiffInput, DiffOptions, diff_lines};

    struct Pair {
        _dir: tempfile::TempDir,
        left: TextDocument,
        right: TextDocument,
        diff: LineDiff,
    }

    fn pair_bytes(a: &[u8], b: &[u8]) -> Pair {
        pair_with_bytes(
            a,
            b,
            DiffOptions {
                ignore_eol: true,
                ..DiffOptions::default()
            },
        )
    }

    fn pair_with(a: &str, b: &str, options: DiffOptions) -> Pair {
        pair_with_bytes(a.as_bytes(), b.as_bytes(), options)
    }

    fn pair_with_bytes(a: &[u8], b: &[u8], options: DiffOptions) -> Pair {
        let dir = tempfile::tempdir().unwrap();
        let (pa, pb) = (dir.path().join("l.txt"), dir.path().join("r.txt"));
        std::fs::write(&pa, a).unwrap();
        std::fs::write(&pb, b).unwrap();
        let cancel = CancelToken::new();
        let left = TextDocument::open(&pa, &cancel).unwrap();
        let right = TextDocument::open(&pb, &cancel).unwrap();
        let diff = diff_lines(
            DiffInput {
                bytes: left.bytes(),
                lines: left.lines(),
            },
            DiffInput {
                bytes: right.bytes(),
                lines: right.lines(),
            },
            options,
            &cancel,
        )
        .unwrap();
        Pair {
            _dir: dir,
            left,
            right,
            diff,
        }
    }

    fn pair(a: &str, b: &str) -> Pair {
        pair_bytes(a.as_bytes(), b.as_bytes())
    }

    fn patch(a: &str, b: &str) -> String {
        let p = pair(a, b);
        unified_diff(
            &p.left,
            &p.right,
            &p.diff,
            &PatchOptions {
                left_name: "l.txt",
                right_name: "r.txt",
                context: 3,
            },
        )
    }

    #[test]
    fn the_unified_diff_ports_localdiff() {
        assert_eq!(
            patch("a\nb\nc\n", "a\nB\nc\n"),
            "--- a/l.txt\n+++ b/r.txt\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n"
        );
        assert_eq!(
            patch("a\nb", "a\nc"),
            "--- a/l.txt\n+++ b/r.txt\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n"
        );
        assert_eq!(
            patch("a\nb\n", "a\nb"),
            "--- a/l.txt\n+++ b/r.txt\n@@ -1,2 +1,2 @@\n a\n-b\n+b\n\\ No newline at end of file\n"
        );
        assert_eq!(
            patch("a\r\nb\r\n", "a\r\nc\r\n"),
            "--- a/l.txt\n+++ b/r.txt\n@@ -1,2 +1,2 @@\n a\r\n-b\r\n+c\r\n"
        );
        assert_eq!(
            patch("", "x\n"),
            "--- a/l.txt\n+++ b/r.txt\n@@ -0,0 +1 @@\n+x\n"
        );
    }

    #[test]
    fn hunks_within_twice_the_context_share_a_group() {
        let lines = |mid: usize, changed: &str| {
            let mut v = vec![changed.to_owned()];
            v.extend((0..mid).map(|i| format!("m{i}")));
            v.push(changed.to_owned());
            v.join("\n") + "\n"
        };
        let groups = |mid: usize| {
            patch(&lines(mid, "x"), &lines(mid, "y"))
                .matches("@@ -")
                .count()
        };
        assert_eq!(groups(6), 1);
        assert_eq!(groups(7), 2);
    }

    #[test]
    fn identical_files_give_only_the_headers() {
        assert_eq!(patch("a\n", "a\n"), "--- a/l.txt\n+++ b/r.txt\n");
    }

    fn meta<'a>(status: &'a str) -> ReportMeta<'a> {
        ReportMeta {
            left: "l.txt",
            right: "r.txt",
            date: "2026-10-23 10:00",
            status,
            title: "l.txt vs r.txt",
        }
    }

    #[test]
    fn the_text_report_starts_with_a_bom_and_uses_crlf() {
        let p = pair("a\nb\n", "a\nc\nd\n");
        assert_eq!(
            text_report(&p.left, &p.right, &p.diff, &meta("S")),
            "\u{feff}duet: text compare report\r\nLeft : l.txt\r\nRight: r.txt\r\nDate : 2026-10-23 10:00\r\nS\r\n\r\n=== Difference 1 · Left 2-2 · Right 2-3 ===\r\n< b\r\n---\r\n> c\r\n> d\r\n"
        );
        let p = pair("a\nc\n", "a\nb\nc\n");
        let text = text_report(&p.left, &p.right, &p.diff, &meta("S"));
        assert!(
            text.contains("=== Difference 1 · Left insert at 2 · Right 2-2 ===\r\n> b\r\n"),
            "{text:?}"
        );
        assert!(!text.contains("---"), "{text:?}");
    }

    fn html(a: &str, b: &str, mode: ReportMode, max: usize) -> String {
        let p = pair(a, b);
        let rows = RowMap::new(&p.diff);
        html_report(&p.left, &p.right, &rows, mode, false, 4, &meta("S"), max)
    }

    const SKIP1: &str = "<tr class=\"skip\"><td colspan=\"4\">⋯ 1 unchanged line ⋯</td></tr>";

    #[test]
    fn the_html_report_escapes_skips_and_caps() {
        let (a, b) = ("a\nb\nc\nd\ne\n", "a\nb\nC\nd\ne\n");
        let page = html(a, b, ReportMode::Context(1), HTML_MAX_ROWS);
        assert_eq!(page.matches(SKIP1).count(), 2, "{page}");
        assert!(page.contains(
            "<tr class=\"mod\"><td class=\"ln\">3</td><td class=\"l\"><span class=\"ch\">c</span></td><td class=\"ln\">3</td><td class=\"r\"><span class=\"ch\">C</span></td></tr>"
        ));
        let all = html(a, b, ReportMode::All, HTML_MAX_ROWS);
        assert!(!all.contains("class=\"skip\""));
        let only = html(a, b, ReportMode::Differences, HTML_MAX_ROWS);
        assert_eq!(only.matches("class=\"skip\"").count(), 2);
        assert_eq!(only.matches("⋯ 2 unchanged lines ⋯").count(), 2);
        assert!(!only.contains("<td class=\"l\">a</td>"));
        let esc = html("<b>&\n", "<b>&\n", ReportMode::All, HTML_MAX_ROWS);
        assert!(esc.contains("&lt;b&gt;&amp;"));
        assert!(!esc.contains("<script"));
        let capped = html(a, b, ReportMode::All, 2);
        assert!(
            capped.contains(
                "<tr class=\"skip\"><td colspan=\"4\">⋯ The report stops here: more than 2 rows. ⋯</td></tr></table>"
            ),
            "{capped}"
        );
        assert_eq!(capped.matches("<tr class=\"same\"").count(), 2);
    }

    #[test]
    fn the_html_page_is_self_contained() {
        let page = html("a\n", "b\n", ReportMode::All, HTML_MAX_ROWS);
        assert!(page.starts_with("<!DOCTYPE html><html lang=\"en\">"));
        assert!(page.contains("<title>duet — l.txt vs r.txt</title>"));
        assert!(page.contains("<h1>Text compare report</h1>"));
        assert!(page.contains("Left: <b>l.txt</b>"));
        assert!(page.contains("Made 2026-10-23 10:00 with duet"));
        assert!(!page.contains("http"), "no external URL");
    }

    #[test]
    fn row_classes_follow_the_kinds() {
        let page = html("a\nb\n", "a\nb\nc\n", ReportMode::All, HTML_MAX_ROWS);
        assert!(page.contains("<tr class=\"ins\"><td class=\"ln\"></td><td class=\"ph\"></td><td class=\"ln\">3</td><td class=\"r\">c</td></tr>"));
        let page = html("a\nb\nc\n", "a\nb\n", ReportMode::All, HTML_MAX_ROWS);
        assert!(page.contains("<tr class=\"del\"><td class=\"ln\">3</td><td class=\"l\">c</td><td class=\"ln\"></td><td class=\"ph\"></td></tr>"));
        // Ignoring: a pair the diff matched whose text differs is a "soft" row, with highlights.
        let p = pair_with(
            "a
x
",
            "a
X
",
            DiffOptions {
                ignore_case: true,
                ..DiffOptions::default()
            },
        );
        let rows = RowMap::new(&p.diff);
        let page = html_report(
            &p.left,
            &p.right,
            &rows,
            ReportMode::All,
            true,
            4,
            &meta("S"),
            100,
        );
        assert!(page.contains("<tr class=\"soft\">"), "{page}");
        assert!(page.contains("<span class=\"ch\">x</span>"), "{page}");
        assert!(page.contains("<span class=\"ch\">X</span>"), "{page}");
    }

    #[test]
    fn html_line_marks_escapes_and_expands_tabs() {
        assert_eq!(
            html_line("a<b\tc", &[0..1], 4, false),
            "<span class=\"ch\">a</span>&lt;b c"
        );
        assert_eq!(
            html_line("x\u{a0}y", &[], 4, true),
            "x<span class=\"odd o-space\" title=\"U+00A0 no-break space\">⍽</span>y"
        );
        assert_eq!(html_line("x\u{a0}y", &[], 4, false), "x\u{a0}y");
        assert_eq!(
            html_line("x\u{200b}", &[], 4, true),
            "x<span class=\"odd o-zero\" title=\"U+200B zero-width space\">ZWSP</span>"
        );
        assert_eq!(html_line("\"q\"", &[], 4, false), "&quot;q&quot;");
        // Tabs count grapheme columns: a Thai base with its vowel mark is one column.
        assert_eq!(html_line("กิ\tx", &[], 4, false), "กิ   x");
        assert_eq!(html_line("", &[], 4, true), "");
    }

    #[test]
    fn report_names_follow_localdiff() {
        assert_eq!(
            report_name("C:\\x\\left.txt", "right.txt", ".patch"),
            "diff-left-vs-right.patch"
        );
        assert_eq!(
            report_name("a:b?.c", "", ".html"),
            "diff-a_b_-vs-right.html"
        );
        assert_eq!(report_name("", "", ".txt"), "diff-left-vs-right.txt");
        let long = "x".repeat(120);
        let name = report_name(&format!("{long}.txt"), "r.txt", ".html");
        assert_eq!(name.strip_suffix(".html").unwrap().chars().count(), 100);
    }
}
