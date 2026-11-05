//! YAML: the event parser `granit-parser` gives scalars with their text and style; they are kept
//! as written (a plain `no` stays plain, a quoted one stays quoted), aliases are expanded by
//! sharing the finished node, mappings are sorted by their keys' text, and everything is printed
//! in block style with two-space indents.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use granit_parser::{ErrorKind, Event, Parser, ScalarStyle, Tag};

use super::{MAX_DEPTH, MAX_NODES, MAX_OUTPUT, SortError, SortKind, SortNote, Sorted, message};
use crate::cancel::CancelToken;

/// Comments in a row that the parser may hold while it decides what an entry is. Past this the side
/// is refused: the parser's own default is 96, and `usize::MAX` let a file of nothing but comments
/// fill memory.
const MAX_BUFFERED_COMMENTS: usize = 10_000;

enum Kind {
    /// `quoted` is false only for a plain scalar.
    Scalar {
        text: String,
        plain: bool,
    },
    Seq(Vec<Rc<Node>>),
    Map(Vec<Pair>),
}

struct Pair {
    key: Rc<Node>,
    line: usize,
    value: Rc<Node>,
}

struct Node {
    kind: Kind,
    tag: Option<String>,
    /// Nodes in this subtree, counting a shared node once per use.
    size: usize,
    /// Bytes of text in this subtree, the same way.
    bytes: usize,
    /// Levels of collections below and including this node.
    height: usize,
}

enum Frame {
    Seq {
        items: Vec<Rc<Node>>,
        anchor: usize,
        tag: Option<String>,
    },
    Map {
        pairs: Vec<Pair>,
        key: Option<(Rc<Node>, usize)>,
        anchor: usize,
        tag: Option<String>,
    },
}

fn tag_text(tag: Option<&Tag>) -> Option<String> {
    let tag = tag?;
    let suffix = escaped_tag(tag.suffix());
    Some(match tag.handle() {
        "tag:yaml.org,2002:" => format!("!!{suffix}"),
        "!" => format!("!{suffix}"),
        other => format!("!<{other}{suffix}>"),
    })
}

/// The parser hands over a tag suffix with its `%20` escapes decoded; written back as it was it
/// could end the tag early (a space) or read as something else.
fn escaped_tag(suffix: &str) -> String {
    let mut out = String::with_capacity(suffix.len());
    for byte in suffix.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~*'()/:;@&=+$#?".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn scan_error(error: &granit_parser::ScanError) -> SortError {
    if matches!(error.kind(), ErrorKind::TooManyComments) {
        return SortError::TooBig;
    }
    SortError::Syntax {
        line: error.marker().line(),
        column: error.marker().col() + 1,
        message: error.info(),
    }
}

fn syntax_at(line: usize, column: usize, message: &str) -> SortError {
    SortError::Syntax {
        line,
        column,
        message: message.to_owned(),
    }
}

pub(super) fn sort(src: &str, cancel: &CancelToken) -> Result<Sorted, SortError> {
    let options = granit_parser::options! {
        emit_comments: true,
        max_buffered_comment_events: MAX_BUFFERED_COMMENTS,
        flow_nesting_limit: 1024,
        block_nesting_limit: 1024,
    };
    let parser = Parser::new_from_str_with_options(src, options);
    let mut stack: Vec<Frame> = Vec::new();
    let mut root: Option<Rc<Node>> = None;
    let mut docs: Vec<Rc<Node>> = Vec::new();
    let mut anchors: HashMap<usize, Rc<Node>> = HashMap::new();
    let mut open_anchors: HashSet<usize> = HashSet::new();
    let (mut comments, mut aliased) = (false, false);
    let mut duplicates: Vec<(usize, String)> = Vec::new();
    let mut versions: Vec<(u32, u32)> = Vec::new();
    for (n, item) in parser.enumerate() {
        if n % 4096 == 0 {
            cancel.check()?;
        }
        let (event, span) = item.map_err(|error| scan_error(&error))?;
        let line = span.start.line();
        let column = span.start.col() + 1;
        let node = match event {
            // The crate is pinned, so no other event exists; `Event` is non_exhaustive.
            Event::StreamStart | Event::StreamEnd => continue,
            Event::DocumentStart(_, version) => {
                if let Some(version) = version {
                    let version = (version.major, version.minor);
                    if !versions.contains(&version) {
                        versions.push(version);
                    }
                }
                anchors.clear();
                open_anchors.clear();
                continue;
            }
            Event::DocumentEnd => {
                docs.push(root.take().unwrap_or_else(|| {
                    Rc::new(Node {
                        kind: Kind::Scalar {
                            text: String::new(),
                            plain: true,
                        },
                        tag: None,
                        size: 1,
                        bytes: 0,
                        height: 0,
                    })
                }));
                continue;
            }
            Event::Comment(..) => {
                // A run of comments is read inside one parser step: look often.
                cancel.check()?;
                comments = true;
                continue;
            }
            Event::Scalar(text, style, anchor, tag) => {
                // The parser writes an empty value as a plain `~` that takes no room in the text.
                let empty = matches!(style, ScalarStyle::Plain)
                    && text == "~"
                    && span.start.index() == span.end.index();
                let text = if empty { "".into() } else { text };
                let node = Rc::new(Node {
                    size: 1,
                    bytes: text.len(),
                    height: 0,
                    tag: tag_text(tag.as_deref()),
                    kind: Kind::Scalar {
                        plain: matches!(style, ScalarStyle::Plain),
                        text: text.into_owned(),
                    },
                });
                if anchor != 0 {
                    aliased = true;
                    anchors.insert(anchor, node.clone());
                }
                node
            }
            Event::Alias(id) => {
                aliased = true;
                match anchors.get(&id) {
                    Some(node) => {
                        if stack.len() + node.height > MAX_DEPTH {
                            return Err(SortError::TooDeep);
                        }
                        node.clone()
                    }
                    None if open_anchors.contains(&id) => {
                        return Err(syntax_at(line, column, message::RECURSIVE_ALIAS));
                    }
                    None => return Err(syntax_at(line, column, message::UNKNOWN_ALIAS)),
                }
            }
            Event::SequenceStart(_, anchor, tag) => {
                if stack.len() >= MAX_DEPTH {
                    return Err(SortError::TooDeep);
                }
                if anchor != 0 {
                    open_anchors.insert(anchor);
                }
                stack.push(Frame::Seq {
                    items: Vec::new(),
                    anchor,
                    tag: tag_text(tag.as_deref()),
                });
                continue;
            }
            Event::MappingStart(_, anchor, tag) => {
                if stack.len() >= MAX_DEPTH {
                    return Err(SortError::TooDeep);
                }
                if anchor != 0 {
                    open_anchors.insert(anchor);
                }
                stack.push(Frame::Map {
                    pairs: Vec::new(),
                    key: None,
                    anchor,
                    tag: tag_text(tag.as_deref()),
                });
                continue;
            }
            Event::SequenceEnd => {
                let Some(Frame::Seq { items, anchor, tag }) = stack.pop() else {
                    return Err(syntax_at(line, column, message::UNEXPECTED_END));
                };
                let node = collection(Kind::Seq(items), tag)?;
                if anchor != 0 {
                    open_anchors.remove(&anchor);
                    anchors.insert(anchor, node.clone());
                }
                node
            }
            Event::MappingEnd => {
                let Some(Frame::Map {
                    mut pairs,
                    anchor,
                    tag,
                    ..
                }) = stack.pop()
                else {
                    return Err(syntax_at(line, column, message::UNEXPECTED_END));
                };
                pairs.sort_by(key_order);
                for two in pairs.windows(2) {
                    if key_order(&two[0], &two[1]).is_eq() {
                        duplicates.push((two[1].line, key_text(&two[1].key).to_owned()));
                    }
                }
                let node = collection(Kind::Map(pairs), tag)?;
                if anchor != 0 {
                    open_anchors.remove(&anchor);
                    anchors.insert(anchor, node.clone());
                }
                node
            }
            _ => continue,
        };
        match stack.last_mut() {
            None => root = Some(node),
            Some(Frame::Seq { items, .. }) => items.push(node),
            Some(Frame::Map { pairs, key, .. }) => match key.take() {
                None => {
                    if !matches!(node.kind, Kind::Scalar { .. }) {
                        return Err(SortError::Unsupported(message::COMPLEX_KEYS.to_owned()));
                    }
                    *key = Some((node, line));
                }
                Some((key, key_line)) => pairs.push(Pair {
                    key,
                    line: key_line,
                    value: node,
                }),
            },
        }
    }
    cancel.check()?;
    let mut out = String::new();
    for (i, doc) in docs.iter().enumerate() {
        if i > 0 {
            out.push_str("---\n");
        }
        print_root(doc, &mut out)?;
    }
    if !src.ends_with(['\n', '\r']) && out.ends_with('\n') {
        out.pop();
    }
    duplicates.sort_by_key(|(line, _)| *line);
    let mut notes: Vec<SortNote> = duplicates
        .into_iter()
        .map(|(line, key)| SortNote::DuplicateKey { key, line })
        .collect();
    if comments {
        notes.push(SortNote::CommentsDropped);
    }
    notes.extend(
        versions
            .into_iter()
            .map(|(major, minor)| SortNote::YamlVersion(major, minor)),
    );
    if aliased {
        notes.push(SortNote::AnchorsExpanded);
    }
    if docs.len() > 1 {
        notes.push(SortNote::Documents(docs.len()));
    }
    Ok(Sorted {
        kind: SortKind::Yaml,
        text: out,
        notes,
    })
}

/// Keys order by their text, then by how they are written: `"1"` and `1` are two keys to a
/// loader, so they are neither swapped from file to file nor reported as one key twice.
fn key_order(a: &Pair, b: &Pair) -> std::cmp::Ordering {
    key_text(&a.key)
        .cmp(key_text(&b.key))
        .then_with(|| shown_key(&a.key).cmp(&shown_key(&b.key)))
}

fn shown_key(node: &Node) -> String {
    let mut out = String::new();
    key_scalar(node, &mut out);
    out
}

fn key_text(node: &Node) -> &str {
    match &node.kind {
        Kind::Scalar { text, .. } => text,
        _ => "",
    }
}

/// A finished collection, refused when it (with its aliases expanded) is too big.
fn collection(kind: Kind, tag: Option<String>) -> Result<Rc<Node>, SortError> {
    let (mut size, mut bytes, mut height) = (1usize, 0usize, 0usize);
    let mut add = |node: &Node| {
        size = size.saturating_add(node.size);
        bytes = bytes.saturating_add(node.bytes);
        height = height.max(node.height);
    };
    match &kind {
        Kind::Seq(items) => items.iter().for_each(|node| add(node)),
        Kind::Map(pairs) => pairs.iter().for_each(|pair| {
            add(&pair.key);
            add(&pair.value);
        }),
        Kind::Scalar { .. } => {}
    }
    if size > MAX_NODES || bytes > MAX_OUTPUT {
        return Err(SortError::TooBig);
    }
    Ok(Rc::new(Node {
        kind,
        tag,
        size,
        bytes,
        height: height + 1,
    }))
}

/// A string in double quotes with JSON's escapes; other characters are written as they are.
fn quoted(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A scalar as shown: its tag, then its text. Empty plain text is nothing at all.
fn scalar(node: &Node, out: &mut String) {
    if let Some(tag) = &node.tag {
        out.push_str(tag);
        out.push(' ');
    }
    let Kind::Scalar { text, plain } = &node.kind else {
        return;
    };
    // A tagged scalar with no text is the tag alone: take back the space written after it, and
    // only that one (a space the caller wrote before the key is not ours).
    if text.is_empty() && *plain {
        if node.tag.is_some() {
            out.pop();
        }
        return;
    }
    if *plain && !text.contains(['\n', '\r']) {
        out.push_str(text);
    } else {
        quoted(text, out);
    }
}

/// A key as shown. A quoted key whose text a loader could not read as anything but that text
/// (`"a"`, so `{"a": 1}` matches `a: 1`) is written plain; `"no"`, `"1"` and the like keep their
/// quotes, since `no: 1` and `"no": 1` can be different keys.
fn key_scalar(node: &Node, out: &mut String) {
    if let Kind::Scalar { text, plain: false } = &node.kind {
        let first = text.chars().next();
        let word = text.to_ascii_lowercase();
        let safe = node.tag.is_none()
            && first.is_some_and(|c| c.is_alphabetic() || c == '_')
            && text
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
            && !matches!(
                word.as_str(),
                "true" | "false" | "yes" | "no" | "on" | "off" | "y" | "n" | "null"
            );
        if safe {
            out.push_str(text);
            return;
        }
    }
    scalar(node, out);
}

fn is_empty(node: &Node) -> bool {
    match &node.kind {
        Kind::Seq(items) => items.is_empty(),
        Kind::Map(pairs) => pairs.is_empty(),
        Kind::Scalar { .. } => false,
    }
}

/// A small file can print gigabytes (a long indent before each of millions of items), and a
/// failed allocation aborts the process: so the output is checked as it grows.
fn too_long(out: &str) -> Result<(), SortError> {
    if out.len() > MAX_OUTPUT {
        return Err(SortError::TooBig);
    }
    Ok(())
}

fn pad(out: &mut String, indent: usize) {
    out.extend(std::iter::repeat_n(' ', indent));
}

fn print_root(node: &Node, out: &mut String) -> Result<(), SortError> {
    match &node.kind {
        Kind::Scalar { .. } => {
            scalar(node, out);
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
        _ if is_empty(node) => {
            tag_then(node, out);
            out.push_str(if matches!(node.kind, Kind::Map(_)) {
                "{}\n"
            } else {
                "[]\n"
            });
        }
        _ => {
            if let Some(tag) = &node.tag {
                out.push_str(tag);
                out.push('\n');
            }
            entries(node, 0, false, out)?;
        }
    }
    Ok(())
}

fn tag_then(node: &Node, out: &mut String) {
    if let Some(tag) = &node.tag {
        out.push_str(tag);
        out.push(' ');
    }
}

/// The lines of a non-empty collection at `indent`. With `inline` its first line continues the
/// line the caller has begun (`- ` before it).
fn entries(node: &Node, indent: usize, inline: bool, out: &mut String) -> Result<(), SortError> {
    too_long(out)?;
    match &node.kind {
        Kind::Seq(items) => {
            for (i, item) in items.iter().enumerate() {
                too_long(out)?;
                if !(inline && i == 0) {
                    pad(out, indent);
                }
                out.push('-');
                value(item, indent, true, out)?;
            }
        }
        Kind::Map(pairs) => {
            for (i, pair) in pairs.iter().enumerate() {
                too_long(out)?;
                if !(inline && i == 0) {
                    pad(out, indent);
                }
                key_scalar(&pair.key, out);
                out.push(':');
                value(&pair.value, indent, false, out)?;
            }
        }
        Kind::Scalar { .. } => {}
    }
    Ok(())
}

/// What follows `-` or `key:` at `indent`, ending its line.
fn value(node: &Node, indent: usize, in_seq: bool, out: &mut String) -> Result<(), SortError> {
    match &node.kind {
        Kind::Scalar { .. } => {
            let mark = out.len();
            out.push(' ');
            scalar(node, out);
            if out.len() == mark + 1 {
                out.pop();
            }
            out.push('\n');
        }
        _ if is_empty(node) => {
            out.push(' ');
            tag_then(node, out);
            out.push_str(if matches!(node.kind, Kind::Map(_)) {
                "{}\n"
            } else {
                "[]\n"
            });
        }
        // `- key: v` and `- - a` continue on the dash's line; everything else starts a new one.
        _ if in_seq && node.tag.is_none() => {
            out.push(' ');
            entries(node, indent + 2, true, out)?;
        }
        _ => {
            if let Some(tag) = &node.tag {
                out.push(' ');
                out.push_str(tag);
            }
            out.push('\n');
            entries(node, indent + 2, false, out)?;
        }
    }
    Ok(())
}
