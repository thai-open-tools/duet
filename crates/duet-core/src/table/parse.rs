//! LocalDiff-compatible CSV parsing and delimiter detection (`js/70-table.js` parseRows, detectDelim).

use std::sync::atomic::{AtomicU64, Ordering};

use memchr::{memchr, memchr3};

use super::{Delimiter, Table};
use crate::cancel::{CancelToken, Cancelled};

/// Bytes between progress updates and cancel checks.
const CHECK_EVERY: usize = 1 << 20;

/// Guesses the delimiter: counts each candidate outside quotes per line over the first 60
/// non-empty lines (at most 65,536 chars) and prefers the most consistent per-line count.
pub fn detect_delimiter(text: &str, file_name: &str) -> Delimiter {
    let mut in_quote = false;
    let mut lines: Vec<[u32; 4]> = Vec::new();
    let mut counts = [0u32; 4];
    let mut any = false;
    let mut chars = text.chars();
    let mut consumed = 0usize;
    while lines.len() < 60 && consumed < 65_536 {
        let Some(c) = chars.next() else {
            break;
        };
        consumed += 1;
        if c == '"' {
            in_quote = !in_quote;
            any = true;
            continue;
        }
        if in_quote {
            continue;
        }
        if c == '\n' || c == '\r' {
            if any {
                lines.push(counts);
            }
            counts = [0; 4];
            any = false;
            continue;
        }
        any = true;
        match c {
            ',' => counts[0] += 1,
            '\t' => counts[1] += 1,
            ';' => counts[2] += 1,
            '|' => counts[3] += 1,
            _ => {}
        }
    }
    // Like LocalDiff, a trailing partial line counts only when the whole text was read.
    let at_end = chars.next().is_none();
    if any && (at_end || lines.is_empty()) {
        lines.push(counts);
    }
    let lower = file_name.to_ascii_lowercase();
    let is_tsv = lower.ends_with(".tsv") || lower.ends_with(".tab");
    let mut best: Option<Delimiter> = None;
    let mut best_score = -1i64;
    for (d, delimiter) in Delimiter::ALL.into_iter().enumerate() {
        let mut freq = std::collections::HashMap::<u32, u32>::new();
        let (mut m, mut mf) = (0u32, 0u32);
        for line in &lines {
            let v = line[d];
            if v == 0 {
                continue;
            }
            let f = freq.entry(v).or_insert(0);
            *f += 1;
            if *f > mf || (*f == mf && v > m) {
                mf = *f;
                m = v;
            }
        }
        if m == 0 {
            continue;
        }
        let pct = (f64::from(mf) / lines.len() as f64 * 100.0).round() as i64;
        let mut score = pct * 1000 + i64::from(m.min(400));
        if delimiter == Delimiter::Tab {
            score += 500 + if is_tsv { 1_000_000 } else { 0 };
        }
        if score > best_score {
            best_score = score;
            best = Some(delimiter);
        }
    }
    best.unwrap_or(if is_tsv {
        Delimiter::Tab
    } else {
        Delimiter::Comma
    })
}

/// Parses all of `text`. `done` gets the bytes parsed so far, updated and cancel-checked
/// about every 1 MiB.
pub fn parse(
    text: &str,
    delimiter: Delimiter,
    auto: bool,
    cancel: &CancelToken,
    done: &AtomicU64,
) -> Result<Table, Cancelled> {
    run(text, delimiter, auto, Some((cancel, done)))
}

/// Parses without cancellation or progress (tests).
#[doc(hidden)]
pub fn parse_str(text: &str, delimiter: Delimiter) -> Table {
    run(text, delimiter, false, None).expect("no cancel token")
}

fn count_newlines(s: &str) -> u32 {
    let b = s.as_bytes();
    let mut k = 0;
    for (i, &c) in b.iter().enumerate() {
        if c == b'\n' || (c == b'\r' && b.get(i + 1) != Some(&b'\n')) {
            k += 1;
        }
    }
    k
}

fn run(
    text: &str,
    delimiter: Delimiter,
    auto: bool,
    progress: Option<(&CancelToken, &AtomicU64)>,
) -> Result<Table, Cancelled> {
    let b = text.as_bytes();
    let n = b.len();
    let dc = delimiter.char() as u8;
    let mut arena = String::with_capacity(n);
    let mut cell_ends: Vec<u32> = Vec::new();
    let mut record_cells: Vec<u32> = vec![0];
    let mut lines: Vec<u32> = Vec::new();
    let mut end_lines: Vec<u32> = Vec::new();
    let mut width = 0usize;
    let mut pos = if text.starts_with('\u{feff}') { 3 } else { 0 };
    let mut checked = pos;
    let mut ln = 1u32;
    while pos < n {
        if pos - checked >= CHECK_EVERY {
            checked = pos;
            if let Some((cancel, done)) = progress {
                cancel.check()?;
                done.store(pos as u64, Ordering::Relaxed);
            }
        }
        let l0 = ln;
        let first_cell = cell_ends.len();
        let mut terminated = false;
        loop {
            let mut c = b[pos];
            if c == b' ' && dc != b' ' {
                let mut e = pos;
                while e < n && b[e] == b' ' {
                    e += 1;
                }
                if e < n && b[e] == b'"' {
                    pos = e;
                    c = b'"';
                }
            }
            if c == b'"' {
                let cell_start = arena.len();
                let mut e = pos + 1;
                loop {
                    match memchr(b'"', &b[e..]) {
                        None => {
                            arena.push_str(&text[e..]);
                            pos = n;
                            break;
                        }
                        Some(k) => {
                            let q = e + k;
                            arena.push_str(&text[e..q]);
                            if b.get(q + 1) == Some(&b'"') {
                                arena.push('"');
                                e = q + 2;
                            } else {
                                pos = q + 1;
                                break;
                            }
                        }
                    }
                }
                // Text after the closing quote, up to the delimiter, is kept unless blank.
                let e = pos + memchr3(dc, b'\n', b'\r', &b[pos..]).unwrap_or(n - pos);
                if e > pos {
                    let junk = &text[pos..e];
                    if !junk.trim().is_empty() {
                        arena.push_str(junk);
                    }
                }
                ln += count_newlines(&arena[cell_start..]);
                cell_ends.push(arena.len() as u32);
                pos = e;
            } else {
                let e = pos + memchr3(dc, b'\n', b'\r', &b[pos..]).unwrap_or(n - pos);
                arena.push_str(&text[pos..e]);
                cell_ends.push(arena.len() as u32);
                pos = e;
            }
            if pos >= n {
                break;
            }
            c = b[pos];
            if c == dc {
                pos += 1;
                if pos >= n {
                    cell_ends.push(arena.len() as u32);
                    break;
                }
                continue;
            }
            pos += if c == b'\r' && b.get(pos + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
            ln += 1;
            terminated = true;
            break;
        }
        let cells = cell_ends.len() - first_cell;
        let blank = cells == 1 && {
            let start = if first_cell == 0 {
                0
            } else {
                cell_ends[first_cell - 1]
            };
            cell_ends[first_cell] == start
        };
        if blank {
            cell_ends.truncate(first_cell);
        } else {
            width = width.max(cells);
            record_cells.push(cell_ends.len() as u32);
            lines.push(l0);
            end_lines.push(if terminated { ln - 1 } else { ln });
        }
    }
    if let Some((_, done)) = progress {
        done.store(n as u64, Ordering::Relaxed);
    }
    Ok(Table {
        delimiter,
        auto,
        arena,
        cell_ends,
        record_cells,
        lines,
        end_lines,
        width,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(t: &Table) -> Vec<Vec<String>> {
        (0..t.len())
            .map(|i| t.record(i).iter().map(str::to_owned).collect())
            .collect()
    }

    #[test]
    fn parse_follows_localdiff_on_odd_input() {
        let t = parse_str(
            "\u{feff}a,b\r\n\"x,1\",\"he said \"\"hi\"\"\"\n\n3,\"multi\nline\"\r4,5,",
            Delimiter::Comma,
        );
        assert_eq!(
            rows(&t),
            [
                vec!["a", "b"],
                vec!["x,1", "he said \"hi\""],
                vec!["3", "multi\nline"],
                vec!["4", "5", ""]
            ]
        );
        assert_eq!((0..4).map(|i| t.line(i)).collect::<Vec<_>>(), [1, 2, 4, 6]);
        assert_eq!(t.record_at_line(5), Some(2), "inside the multi-line cell");
        assert_eq!(t.record_at_line(3), None, "the blank line");
        assert_eq!(t.width(), 3);
        assert_eq!(t.last_line(), 6);
        assert_eq!(
            rows(&parse_str("a, \"b,c\"\n", Delimiter::Comma)),
            [vec!["a", "b,c"]]
        );
        assert_eq!(
            rows(&parse_str("\"ab\"x ,c\n\"ab\"  ,d", Delimiter::Comma)),
            [vec!["abx ", "c"], vec!["ab", "d"]]
        );
        assert_eq!(
            rows(&parse_str("\"abc\nd", Delimiter::Comma)),
            [vec!["abc\nd"]],
            "an unterminated quote takes the rest"
        );
        assert_eq!(rows(&parse_str("", Delimiter::Comma)).len(), 0);
    }

    #[test]
    fn empty_table_and_record_access() {
        let t = parse_str("", Delimiter::Comma);
        assert!(t.is_empty());
        assert_eq!(t.last_line(), 0);
        assert_eq!(t.width(), 0);
        assert_eq!(t.record_at_line(1), None);
        let t = parse_str("a,b\nc", Delimiter::Comma);
        assert_eq!(t.record(1).get(0), Some("c"));
        assert_eq!(t.record(1).get(1), None);
        assert_eq!(t.record(0).len(), 2);
    }

    #[test]
    fn delimiter_keys_round_trip() {
        for d in Delimiter::ALL {
            assert_eq!(Delimiter::from_key(d.key()), Some(d));
        }
        assert_eq!(Delimiter::from_key("x"), None);
        assert_eq!(Delimiter::Tab.char(), '\t');
        assert_eq!(Delimiter::Comma.label(), "Comma (,)");
        assert_eq!(Delimiter::Tab.label(), "Tab");
        assert_eq!(Delimiter::Semicolon.label(), "Semicolon (;)");
        assert_eq!(Delimiter::Pipe.label(), "Pipe (|)");
    }

    #[test]
    fn delimiters_are_detected_like_localdiff() {
        assert_eq!(
            detect_delimiter("a;b;c\n1;2,5;3\n", "x.csv"),
            Delimiter::Semicolon
        );
        assert_eq!(detect_delimiter("a\tb\n1\t2\n", "x.csv"), Delimiter::Tab);
        assert_eq!(detect_delimiter("a|b|c\n1|2|3", "x.txt"), Delimiter::Pipe);
        assert_eq!(
            detect_delimiter("\"a,b,c\";d\n\"e,f\";g\n", "x.csv"),
            Delimiter::Semicolon,
            "quoted commas do not count"
        );
        assert_eq!(
            detect_delimiter("single\ncolumn\n", "x.tsv"),
            Delimiter::Tab
        );
        assert_eq!(
            detect_delimiter("single\ncolumn\n", "x.csv"),
            Delimiter::Comma
        );
    }

    #[test]
    fn parse_reports_progress_and_stops_on_cancel() {
        let text = "a,b\n".repeat(600_000); // 2.4 MB
        let done = AtomicU64::new(0);
        let t = parse(&text, Delimiter::Comma, false, &CancelToken::new(), &done).unwrap();
        assert_eq!(t.len(), 600_000);
        assert_eq!(done.load(Ordering::Relaxed), text.len() as u64);
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(parse(&text, Delimiter::Comma, false, &cancel, &AtomicU64::new(0)).is_err());
    }

    #[test]
    fn detection_ignores_a_line_cut_by_the_scan_cap() {
        // Two complete lines (one holds a `;`), then one line the 65,536-char cap cuts
        // mid-way. LocalDiff drops that partial line, so `;` wins; counting it makes `,` win.
        let text = format!("a;b\nc\n{}", ",".repeat(70_000));
        assert_eq!(detect_delimiter(&text, "x.csv"), Delimiter::Semicolon);
    }
}
