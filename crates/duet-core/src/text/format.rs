//! Pretty-printing for the Format button: JSON and XML with two-space indents. Only
//! insignificant whitespace changes, the same promise as LocalDiff's formatter: two files that
//! differ never become equal by formatting.

/// Deeper nesting is left unformatted: the indents of N levels cost O(N²) bytes, so a small
/// hostile file could otherwise need gigabytes.
pub const MAX_DEPTH: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatKind {
    Json,
    Xml,
}

impl FormatKind {
    pub fn label(self) -> &'static str {
        match self {
            FormatKind::Json => "JSON",
            FormatKind::Xml => "XML",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formatted {
    pub kind: FormatKind,
    pub text: String,
}

/// JSON when the text starts with `{` or `[`, XML when it starts with `<`; None otherwise or
/// when formatting is not safe.
pub fn auto_format(text: &str) -> Option<Formatted> {
    let probe = text.trim_start_matches('\u{feff}').trim_start();
    match probe.chars().next()? {
        '{' | '[' => format_json(text).map(|text| Formatted {
            kind: FormatKind::Json,
            text,
        }),
        '<' => format_xml(text).map(|text| Formatted {
            kind: FormatKind::Xml,
            text,
        }),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReformatKind {
    Json,
    Xml,
    Trimmed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reformatted {
    pub text: String,
    pub kind: ReformatKind,
}

/// LocalDiff `ldAutoFormat` as an edit (R16): JSON or XML re-indented, else the spaces and tabs
/// at the ends of lines trimmed. Trimming keeps every line's own break; a re-indented text
/// takes the break most of its lines end with (CRLF, LF or CR, R16, M10). Whether the text ends
/// with a break stays as it was. None when nothing changes.
pub fn reformat(text: &str) -> Option<Reformatted> {
    // The run of line breaks that ends the text is kept as it is.
    let (body, tail) = text.split_at(text.trim_end_matches(['\r', '\n']).len());
    let normalized;
    let probe = if body.contains('\r') {
        normalized = body.replace("\r\n", "\n").replace('\r', "\n");
        normalized.as_str()
    } else {
        body
    };
    if let Some(formatted) = auto_format(probe) {
        let kind = match formatted.kind {
            FormatKind::Json => ReformatKind::Json,
            FormatKind::Xml => ReformatKind::Xml,
        };
        let mut out = match dominant_break(text) {
            "\n" => formatted.text,
            other => formatted.text.replace('\n', other),
        };
        out.push_str(tail);
        return (out != text).then_some(Reformatted { text: out, kind });
    }
    let trimmed = trim_line_ends(text);
    (trimmed != text).then_some(Reformatted {
        text: trimmed,
        kind: ReformatKind::Trimmed,
    })
}

/// The line break most of `text`'s lines end with; a tie goes to CRLF, then LF. LF for a text
/// with none.
fn dominant_break(text: &str) -> &'static str {
    let crlf = text.matches("\r\n").count();
    let cr = text.matches('\r').count() - crlf;
    let lf = text.matches('\n').count() - crlf;
    if crlf > 0 && crlf >= lf && crlf >= cr {
        "\r\n"
    } else if cr > lf {
        "\r"
    } else {
        "\n"
    }
}

/// `text` without the spaces and tabs before each line break and at its end.
fn trim_line_ends(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    // A CRLF is two pieces here: the "\r" one, then the "\n" one.
    for piece in text.split_inclusive(['\n', '\r']) {
        let (content, brk) = piece.split_at(piece.trim_end_matches(['\n', '\r']).len());
        out.push_str(content.trim_end_matches([' ', '\t']));
        out.push_str(brk);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok<'a> {
    Open(u8),
    Close(u8),
    Comma,
    Colon,
    Scalar(&'a str),
}

fn json_tokens(src: &str) -> Option<Vec<Tok<'_>>> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            c @ (b'{' | b'[') => {
                out.push(Tok::Open(c));
                i += 1;
            }
            c @ (b'}' | b']') => {
                out.push(Tok::Close(c));
                i += 1;
            }
            b',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            b':' => {
                out.push(Tok::Colon);
                i += 1;
            }
            b'"' => {
                let start = i;
                i += 1;
                loop {
                    match *bytes.get(i)? {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        c if c < 0x20 => return None,
                        _ => i += 1,
                    }
                }
                out.push(Tok::Scalar(&src[start..i]));
            }
            _ => {
                let start = i;
                while i < bytes.len() && !b" \t\n\r{}[],:\"".contains(&bytes[i]) {
                    i += 1;
                }
                let literal = &src[start..i];
                if !matches!(literal, "true" | "false" | "null") && !valid_number(literal) {
                    return None;
                }
                out.push(Tok::Scalar(literal));
            }
        }
    }
    Some(out)
}

fn valid_number(s: &str) -> bool {
    let b = s.as_bytes();
    let digits = |mut i: usize| {
        let start = i;
        while b.get(i).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
        }
        (i > start).then_some(i)
    };
    let mut i = usize::from(b.first() == Some(&b'-'));
    i = match b.get(i) {
        Some(b'0') => i + 1,
        Some(c) if c.is_ascii_digit() => match digits(i) {
            Some(end) => end,
            None => return false,
        },
        _ => return false,
    };
    if b.get(i) == Some(&b'.') {
        match digits(i + 1) {
            Some(end) => i = end,
            None => return false,
        }
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        match digits(i) {
            Some(end) => i = end,
            None => return false,
        }
    }
    i == b.len()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect {
    Value,
    ValueOrClose,
    Key,
    KeyOrClose,
    Colon,
    CommaOrClose,
}

/// A token-level grammar check with an explicit stack (deep nesting cannot overflow).
fn json_valid(tokens: &[Tok<'_>]) -> bool {
    let mut stack: Vec<u8> = Vec::new();
    let mut expect = Expect::Value;
    let mut started = false;
    for &tok in tokens {
        if started && stack.is_empty() {
            return false; // something after the root value
        }
        started = true;
        expect = match (expect, tok) {
            (Expect::Value | Expect::ValueOrClose, Tok::Open(_)) if stack.len() == MAX_DEPTH => {
                return false;
            }
            (Expect::Value | Expect::ValueOrClose, Tok::Open(b'{')) => {
                stack.push(b'{');
                Expect::KeyOrClose
            }
            (Expect::Value | Expect::ValueOrClose, Tok::Open(_)) => {
                stack.push(b'[');
                Expect::ValueOrClose
            }
            (Expect::Value | Expect::ValueOrClose, Tok::Scalar(_)) => Expect::CommaOrClose,
            (Expect::Key | Expect::KeyOrClose, Tok::Scalar(s)) if s.starts_with('"') => {
                Expect::Colon
            }
            (Expect::Colon, Tok::Colon) => Expect::Value,
            (Expect::CommaOrClose, Tok::Comma) => {
                if stack.last() == Some(&b'{') {
                    Expect::Key
                } else {
                    Expect::Value
                }
            }
            (Expect::KeyOrClose, Tok::Close(b'}'))
            | (Expect::ValueOrClose, Tok::Close(b']'))
            | (Expect::CommaOrClose, Tok::Close(_)) => {
                let Tok::Close(close) = tok else {
                    unreachable!()
                };
                let open = if close == b'}' { b'{' } else { b'[' };
                if stack.pop() != Some(open) {
                    return false;
                }
                Expect::CommaOrClose
            }
            _ => return false,
        };
    }
    started && stack.is_empty() && expect == Expect::CommaOrClose
}

/// Re-indents valid JSON; scalars are copied as written. None for anything invalid.
pub fn format_json(src: &str) -> Option<String> {
    let tokens = json_tokens(src)?;
    if !json_valid(&tokens) {
        return None;
    }
    let mut out = String::with_capacity(src.len() + src.len() / 4);
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        out.extend(std::iter::repeat_n("  ", depth));
    };
    let mut depth = 0usize;
    let mut i = 0;
    while i < tokens.len() {
        match tokens[i] {
            Tok::Open(open) => {
                let close = if open == b'{' { b'}' } else { b']' };
                if tokens.get(i + 1) == Some(&Tok::Close(close)) {
                    out.push(open as char);
                    out.push(close as char);
                    i += 2;
                    continue;
                }
                depth += 1;
                out.push(open as char);
                newline(&mut out, depth);
            }
            Tok::Close(close) => {
                depth -= 1;
                newline(&mut out, depth);
                out.push(close as char);
            }
            Tok::Comma => {
                out.push(',');
                newline(&mut out, depth);
            }
            Tok::Colon => out.push_str(": "),
            Tok::Scalar(s) => out.push_str(s),
        }
        i += 1;
    }
    if src.ends_with('\n') {
        out.push('\n');
    }
    Some(out)
}

fn xml_tokens(src: &str) -> Option<Vec<&str>> {
    let mut out = Vec::new();
    let mut rest = src;
    while !rest.is_empty() {
        let len = if rest.starts_with("<!--") {
            rest.find("-->")? + 3
        } else if rest.starts_with("<![CDATA[") {
            rest.find("]]>")? + 3
        } else if rest.starts_with('<') {
            rest.find('>')? + 1
        } else {
            rest.find('<').unwrap_or(rest.len())
        };
        out.push(&rest[..len]);
        rest = &rest[len..];
    }
    Some(out)
}

fn is_open(tag: &str) -> bool {
    tag.starts_with('<') && !tag[1..].starts_with(['/', '!', '?']) && !tag.ends_with("/>")
}

fn is_close(tag: &str) -> bool {
    tag.starts_with("</")
}

fn without_whitespace(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Re-indents XML one element per line; `<a>text</a>` stays on one line. None when moving
/// text would change it (mixed content, or whitespace that is part of a value).
pub fn format_xml(src: &str) -> Option<String> {
    let tokens = xml_tokens(src)?;
    let indent = |depth: usize| "  ".repeat(depth);
    let mut lines: Vec<String> = Vec::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];
        if !token.starts_with('<') {
            let collapsed = token.split_whitespace().collect::<Vec<_>>().join(" ");
            let wrapped =
                i > 0 && is_open(tokens[i - 1]) && tokens.get(i + 1).is_some_and(|t| is_close(t));
            if collapsed.is_empty() {
                if wrapped {
                    return None; // <a> </a>: the whitespace is the value
                }
                i += 1;
                continue; // indentation between tags
            }
            if collapsed != token || !wrapped {
                return None;
            }
            let last = lines.last_mut()?;
            last.push_str(token);
            last.push_str(tokens[i + 1]);
            depth = depth.saturating_sub(1);
            i += 2;
            continue;
        }
        if is_close(token) {
            depth = depth.saturating_sub(1);
            lines.push(format!("{}{token}", indent(depth)));
        } else if is_open(token) {
            if tokens.get(i + 1).is_some_and(|t| is_close(t)) {
                lines.push(format!("{}{token}{}", indent(depth), tokens[i + 1]));
                i += 2;
                continue;
            }
            lines.push(format!("{}{token}", indent(depth)));
            depth += 1;
            if depth > MAX_DEPTH {
                return None;
            }
        } else {
            lines.push(format!("{}{token}", indent(depth)));
        }
        i += 1;
    }
    let result = lines.join("\n");
    (without_whitespace(src) == without_whitespace(&result)).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_indented_without_touching_scalars() {
        let src = r#"{"a":1.50,"b":[1,2,{}],"c":"x,y:{","d":[],"e":"a\"b\\"}"#;
        let expected = "{\n  \"a\": 1.50,\n  \"b\": [\n    1,\n    2,\n    {}\n  ],\n  \"c\": \"x,y:{\",\n  \"d\": [],\n  \"e\": \"a\\\"b\\\\\"\n}";
        assert_eq!(format_json(src).as_deref(), Some(expected));
        assert_eq!(
            format_json("[\"ไทย\", -0, 1e10]\n").as_deref(),
            Some("[\n  \"ไทย\",\n  -0,\n  1e10\n]\n")
        );
    }

    #[test]
    fn invalid_json_is_refused() {
        for bad in [
            r#"{"a":}"#,
            "[1,]",
            r#"{"a":1} x"#,
            "{a:1}",
            "[01]",
            r#"["open]"#,
            "[}",
            "{\"a\" 1}",
        ] {
            assert_eq!(format_json(bad), None, "{bad}");
        }
    }

    #[test]
    fn xml_is_indented_and_short_elements_stay_on_one_line() {
        let src = r#"<?xml version="1.0"?><!-- c --><a><b>x y</b><c/><d></d></a>"#;
        let expected =
            "<?xml version=\"1.0\"?>\n<!-- c -->\n<a>\n  <b>x y</b>\n  <c/>\n  <d></d>\n</a>";
        assert_eq!(format_xml(src).as_deref(), Some(expected));
        assert_eq!(
            format_xml(expected).as_deref(),
            Some(expected),
            "formatting is stable"
        );
    }

    #[test]
    fn xml_whose_text_would_change_is_refused() {
        assert_eq!(format_xml("<p>Hello <b>you</b></p>"), None, "mixed content");
        assert_eq!(format_xml("<a> </a>"), None, "whitespace is the value");
        assert_eq!(
            format_xml("<a>x  y</a>"),
            None,
            "inner spacing is part of the value"
        );
    }

    #[test]
    fn nesting_deeper_than_the_limit_is_left_alone() {
        let deep = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
        assert!(format_json(&deep(MAX_DEPTH)).is_some());
        assert_eq!(
            format_json(&deep(MAX_DEPTH + 1)),
            None,
            "indenting 4,000 levels would write about 32 MB per 8 KB"
        );
        let xml = |n: usize| format!("{}x{}", "<a>".repeat(n), "</a>".repeat(n));
        assert!(format_xml(&xml(MAX_DEPTH)).is_some());
        assert_eq!(format_xml(&xml(MAX_DEPTH + 1)), None);
    }

    #[test]
    fn reformat_keeps_line_endings_and_the_final_newline() {
        let got = reformat("{\"a\":1}\r\n").unwrap();
        assert_eq!(got.kind, ReformatKind::Json);
        assert_eq!(
            got.text,
            format_json("{\"a\":1}").unwrap().replace('\n', "\r\n") + "\r\n"
        );
        let got = reformat("x  \r\ny\t\r\n").unwrap();
        assert_eq!(
            (got.kind, got.text.as_str()),
            (ReformatKind::Trimmed, "x\r\ny\r\n")
        );
        let got = reformat("x \ny").unwrap();
        assert_eq!(
            (got.kind, got.text.as_str()),
            (ReformatKind::Trimmed, "x\ny")
        );
        assert_eq!(reformat("x\ny\n"), None);
        assert_eq!(reformat(""), None);
        let got = reformat("<a><b>x</b></a>").unwrap();
        assert_eq!(got.kind, ReformatKind::Xml);
        assert_eq!(got.text, format_xml("<a><b>x</b></a>").unwrap());
        // Already tidy JSON is left alone; so is a mixed text's own break style when trimming.
        assert_eq!(reformat(&format_json("{\"a\":1}").unwrap()), None);
        let got = reformat("a \r\nb \nc \r").unwrap();
        assert_eq!(got.text, "a\r\nb\nc\r");
        // Invalid JSON falls back to trimming.
        let got = reformat("{\"a\": 1,} \n").unwrap();
        assert_eq!(
            (got.kind, got.text.as_str()),
            (ReformatKind::Trimmed, "{\"a\": 1,}\n")
        );
    }

    #[test]
    fn a_reindent_writes_the_files_own_line_break() {
        let pretty = |text: &str, brk: &str| format_json(text).unwrap().replace('\n', brk);
        // A CR-only file comes back with CR breaks.
        let got = reformat("{\"a\":1,\r\"b\":2}\r").unwrap();
        assert_eq!(got.text, pretty("{\"a\":1,\"b\":2}", "\r") + "\r");
        // A mixed file takes the break most of its lines end with.
        let got = reformat("{\"a\":1,\n\"b\":2,\n\"c\":3,\r\n\"d\":4}\n").unwrap();
        assert_eq!(
            got.text,
            pretty("{\"a\":1,\"b\":2,\"c\":3,\"d\":4}", "\n") + "\n"
        );
        let got = reformat("{\"a\":1,\r\n\"b\":2,\r\n\"c\":3,\n\"d\":4}\r\n").unwrap();
        assert_eq!(
            got.text,
            pretty("{\"a\":1,\"b\":2,\"c\":3,\"d\":4}", "\r\n") + "\r\n"
        );
        let got = reformat("<a>\r<b>x</b></a>").unwrap();
        assert_eq!(
            got.text,
            format_xml("<a><b>x</b></a>").unwrap().replace('\n', "\r")
        );
    }

    #[test]
    fn auto_format_picks_by_the_first_character() {
        assert_eq!(
            auto_format("  {\"a\":1}").map(|f| f.kind),
            Some(FormatKind::Json)
        );
        assert_eq!(auto_format("<a/>").map(|f| f.kind), Some(FormatKind::Xml));
        assert_eq!(auto_format("plain text"), None);
        assert_eq!(auto_format("\"just a string\""), None);
    }
}
