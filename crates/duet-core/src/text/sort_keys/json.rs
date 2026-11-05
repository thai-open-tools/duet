//! JSON: the token scan `format.rs` has, a tree of slices of the source, a stable sort of every
//! object's entries by the key's text, and a printer in `format_json`'s layout. Numbers and
//! strings never become a number or a string type, so nothing is rounded, unescaped or merged.

use super::{LineCounter, MAX_NODES, MAX_OUTPUT, SortError, SortKind, SortNote, Sorted, message};
use crate::cancel::CancelToken;
use crate::text::format::{JsonError, JsonErrorKind, Tok, json_scan_limited};

enum Node<'a> {
    Scalar(&'a str),
    Array(Vec<Node<'a>>),
    Object(Vec<Entry<'a>>),
}

struct Entry<'a> {
    /// The key as written, quotes included.
    key: &'a str,
    /// Where the key starts in the text.
    offset: usize,
    value: Node<'a>,
}

enum Frame<'a> {
    Array(Vec<Node<'a>>),
    Object {
        entries: Vec<Entry<'a>>,
        key: Option<(&'a str, usize)>,
    },
}

/// The key without its quotes: keys order by what they hold, not by the closing quote.
fn inner(key: &str) -> &str {
    key.get(1..key.len().saturating_sub(1)).unwrap_or(key)
}

fn syntax(text: &str, error: JsonError) -> SortError {
    let mut counter = LineCounter::new(text);
    let line = counter.line_at(error.offset);
    let start = text[..error.offset.min(text.len())]
        .rfind(['\n', '\r'])
        .map_or(0, |at| at + 1);
    let column = text[start..error.offset.min(text.len())].chars().count() + 1;
    let message = match error.kind {
        JsonErrorKind::ExpectedValue => message::EXPECTED_VALUE,
        JsonErrorKind::ExpectedKey => message::EXPECTED_KEY,
        JsonErrorKind::ExpectedColon => message::EXPECTED_COLON,
        JsonErrorKind::ExpectedCommaOrClose => message::EXPECTED_COMMA,
        JsonErrorKind::TrailingComma(true) => message::TRAILING_BRACE,
        JsonErrorKind::TrailingComma(false) => message::TRAILING_BRACKET,
        JsonErrorKind::Mismatched => message::MISMATCHED,
        JsonErrorKind::Unterminated => message::UNTERMINATED,
        JsonErrorKind::UnexpectedChar => message::UNEXPECTED_CHAR,
        JsonErrorKind::Extra => message::EXTRA,
        JsonErrorKind::UnexpectedEnd => message::UNEXPECTED_END,
        JsonErrorKind::TooDeep => return SortError::TooDeep,
        JsonErrorKind::TooBig => return SortError::TooBig,
    };
    SortError::Syntax {
        line,
        column,
        message: message.to_owned(),
    }
}

pub(super) fn sort(src: &str, cancel: &CancelToken) -> Result<Sorted, SortError> {
    let text = src.strip_prefix('\u{feff}').unwrap_or(src);
    let tokens = json_scan_limited(text, MAX_NODES).map_err(|error| syntax(text, error))?;
    let mut stack: Vec<Frame<'_>> = Vec::new();
    let mut root: Option<Node<'_>> = None;
    let mut duplicates: Vec<(usize, &str)> = Vec::new();
    let mut nodes = 0usize;
    for (n, (tok, offset)) in tokens.into_iter().enumerate() {
        if n % 4096 == 0 {
            cancel.check()?;
        }
        let node = match tok {
            Tok::Colon | Tok::Comma => continue,
            Tok::Open(open) => {
                nodes += 1;
                if nodes > MAX_NODES {
                    return Err(SortError::TooBig);
                }
                stack.push(if open == b'{' {
                    Frame::Object {
                        entries: Vec::new(),
                        key: None,
                    }
                } else {
                    Frame::Array(Vec::new())
                });
                continue;
            }
            Tok::Scalar(text) => {
                if let Some(Frame::Object {
                    key: key @ None, ..
                }) = stack.last_mut()
                {
                    *key = Some((text, offset));
                    continue;
                }
                nodes += 1;
                if nodes > MAX_NODES {
                    return Err(SortError::TooBig);
                }
                Node::Scalar(text)
            }
            Tok::Close(_) => match stack.pop() {
                Some(Frame::Array(items)) => Node::Array(items),
                Some(Frame::Object { mut entries, .. }) => {
                    entries.sort_by(|a, b| inner(a.key).cmp(inner(b.key)));
                    for pair in entries.windows(2) {
                        if pair[0].key == pair[1].key {
                            duplicates.push((pair[1].offset, pair[1].key));
                        }
                    }
                    Node::Object(entries)
                }
                None => unreachable!("the scan balanced the brackets"),
            },
        };
        match stack.last_mut() {
            None => root = Some(node),
            Some(Frame::Array(items)) => items.push(node),
            Some(Frame::Object { entries, key }) => {
                let (key, offset) = key.take().expect("the scan put a key before each value");
                entries.push(Entry {
                    key,
                    offset,
                    value: node,
                });
            }
        }
    }
    cancel.check()?;
    let root = root.expect("the scan found a root value");
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    print(&root, 0, &mut out)?;
    // A lone CR is a line break too, so a text that ends with one ends with a break.
    if text.ends_with(['\n', '\r']) {
        out.push('\n');
    }
    // The lines are counted once, in order, not once per note.
    duplicates.sort_by_key(|(offset, _)| *offset);
    let mut counter = LineCounter::new(text);
    let notes = duplicates
        .into_iter()
        .map(|(offset, key)| SortNote::DuplicateKey {
            key: key.to_owned(),
            line: counter.line_at(offset),
        })
        .collect();
    Ok(Sorted {
        kind: SortKind::Json,
        text: out,
        notes,
    })
}

fn newline(out: &mut String, depth: usize) {
    out.push('\n');
    out.extend(std::iter::repeat_n("  ", depth));
}

/// Two-space layout as `format_json`. The depth is at most `MAX_DEPTH`, which the scan checked,
/// so the recursion is bounded.
fn print(node: &Node<'_>, depth: usize, out: &mut String) -> Result<(), SortError> {
    match node {
        Node::Scalar(text) => out.push_str(text),
        Node::Array(items) if items.is_empty() => out.push_str("[]"),
        Node::Object(entries) if entries.is_empty() => out.push_str("{}"),
        Node::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                too_long(out)?;
                newline(out, depth + 1);
                print(item, depth + 1, out)?;
            }
            newline(out, depth);
            out.push(']');
        }
        Node::Object(entries) => {
            out.push('{');
            for (i, entry) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                too_long(out)?;
                newline(out, depth + 1);
                out.push_str(entry.key);
                out.push_str(": ");
                print(&entry.value, depth + 1, out)?;
            }
            newline(out, depth);
            out.push('}');
        }
    }
    Ok(())
}

/// A small file can print gigabytes (a long indent before each of millions of items), and a
/// failed allocation aborts the process: so the output is checked as it grows.
fn too_long(out: &str) -> Result<(), SortError> {
    if out.len() > MAX_OUTPUT {
        return Err(SortError::TooBig);
    }
    Ok(())
}
