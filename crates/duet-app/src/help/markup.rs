//! The help's inline markup (U8). `[Label]` names a control as duet shows it and is drawn as a
//! chip; `{Keys}` names keys, such as `{Ctrl+S}`, and is drawn as a key cap. `[[`, `]]`, `{{`
//! and `}}` stand for the characters themselves, for literal text such as `[[OUTPUT]]`.
//!
//! The page draws a text as pieces (H6): it breaks after each space outside a chip or a key, so
//! a chip or a key never splits, and the punctuation written right after one stays with it.

/// One run of a piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span<'a> {
    Text(&'a str),
    Chip(&'a str),
    Key(&'a str),
}

impl<'a> Span<'a> {
    /// The text the reader sees: a chip's or a key's without its brackets.
    pub fn text(self) -> &'a str {
        match self {
            Span::Text(t) | Span::Chip(t) | Span::Key(t) => t,
        }
    }
}

/// The parser's place in a text.
#[derive(Clone, Copy, PartialEq, Eq)]
enum In {
    Text,
    Chip,
    Key,
}

/// The spans of `text`, or what is wrong with its markup: a bracket or brace that is not
/// closed, closes nothing, or sits inside a chip or a key, or an empty chip or key.
pub fn parse(text: &str) -> Result<Vec<Span<'_>>, String> {
    let mut spans = Vec::new();
    let mut state = In::Text;
    // Where the run being read starts.
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let doubled = chars.peek().is_some_and(|&(_, next)| next == c);
        match (state, c) {
            (In::Text, '[' | ']' | '{' | '}') if doubled => {
                // The character itself: the run so far, then one of the pair.
                if start < i {
                    spans.push(Span::Text(&text[start..i]));
                }
                spans.push(Span::Text(&text[i..i + 1]));
                chars.next();
                start = i + 2;
            }
            (In::Text, '[' | '{') => {
                if start < i {
                    spans.push(Span::Text(&text[start..i]));
                }
                state = if c == '[' { In::Chip } else { In::Key };
                start = i + 1;
            }
            (In::Text, ']' | '}') => return Err(format!("{c:?} at {i} closes nothing")),
            (In::Chip, ']') | (In::Key, '}') => {
                if start == i {
                    return Err(format!("an empty chip or key at {i}"));
                }
                let inner = &text[start..i];
                spans.push(if state == In::Chip {
                    Span::Chip(inner)
                } else {
                    Span::Key(inner)
                });
                state = In::Text;
                start = i + 1;
            }
            (In::Chip | In::Key, '[' | ']' | '{' | '}') => {
                return Err(format!("{c:?} at {i} inside a chip or a key"));
            }
            _ => {}
        }
    }
    if state != In::Text {
        return Err("a chip or a key is not closed".to_owned());
    }
    if start < text.len() {
        spans.push(Span::Text(&text[start..]));
    }
    Ok(spans)
}

/// The spans as the page draws them. Text whose markup is broken, which the data tests rule
/// out, is drawn as it stands.
pub fn spans(text: &str) -> Vec<Span<'_>> {
    parse(text).unwrap_or_else(|_| vec![Span::Text(text)])
}

/// The text the reader sees, without the markup.
pub fn visible(text: &str) -> String {
    spans(text).into_iter().map(Span::text).collect()
}

/// The chips and keys of `text`, in order.
pub fn marks(text: &str) -> Vec<Span<'_>> {
    spans(text)
        .into_iter()
        .filter(|span| !matches!(span, Span::Text(_)))
        .collect()
}

/// `text` split after each space outside a chip or a key, so every piece is a word, a Thai
/// phrase, a chip or a key with what is written against it, and its trailing space.
pub fn pieces(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let doubled = chars.peek().is_some_and(|&(_, next)| next == c);
        match c {
            '[' | ']' | '{' | '}' if depth == 0 && doubled => {
                chars.next();
            }
            '[' | '{' => depth += 1,
            ']' | '}' => depth = depth.saturating_sub(1),
            ' ' if depth == 0 => {
                out.push(&text[start..i + 1]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// The keys a `{Keys}` may name, joined by `+`: the modifiers and named keys duet binds, F1 to
/// F12, the arrows, and one letter or digit.
pub fn is_known_key(token: &str) -> bool {
    const NAMED: [&str; 12] = [
        "Ctrl", "Alt", "Shift", "Esc", "Enter", "Tab", "Del", "Home", "End", "Space", "PgUp",
        "PgDn",
    ];
    const ARROWS: [&str; 4] = ["↑", "↓", "←", "→"];
    let function = token
        .strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=12).contains(&n));
    let single = {
        let mut chars = token.chars();
        matches!((chars.next(), chars.next()), (Some(c), None) if c.is_ascii_uppercase() || c.is_ascii_digit())
    };
    NAMED.contains(&token) || ARROWS.contains(&token) || function || single
}

/// Whether every key a `{Keys}` names is known.
pub fn keys_are_known(keys: &str) -> bool {
    keys.split('+').all(is_known_key)
}

#[cfg(test)]
mod tests {
    use super::Span::{Chip, Key, Text};
    use super::*;

    #[test]
    fn chips_keys_and_doubled_brackets_parse_into_spans() {
        assert_eq!(
            parse("press [Compare] or {Enter}.").unwrap(),
            [
                Text("press "),
                Chip("Compare"),
                Text(" or "),
                Key("Enter"),
                Text(".")
            ]
        );
        assert_eq!(
            parse("BASE [[OUTPUT]] and {{x}}").unwrap(),
            [
                Text("BASE "),
                Text("["),
                Text("OUTPUT"),
                Text("]"),
                Text(" and "),
                Text("{"),
                Text("x"),
                Text("}")
            ]
        );
        assert_eq!(
            parse("[Remove from list]").unwrap(),
            [Chip("Remove from list")]
        );
        assert_eq!(parse("").unwrap(), []);
        for broken in [
            "[Open", "Open]", "{Ctrl+S", "a } b", "[]", "{}", "[a{b}]", "[a [b]]",
        ] {
            assert!(parse(broken).is_err(), "{broken:?} is not balanced");
        }
        assert_eq!(visible("({Ctrl+F}) and [[x]]"), "(Ctrl+F) and [x]");
        assert_eq!(
            marks("[A], {F1} and [[B]]"),
            [Chip("A"), Key("F1")],
            "escaped brackets are text"
        );
        assert_eq!(spans("broken ["), [Text("broken [")], "drawn as it stands");
    }

    #[test]
    fn pieces_break_only_at_spaces_outside_chips_and_keys() {
        assert_eq!(
            pieces("press [Remove from list], then {Ctrl+S}."),
            ["press ", "[Remove from list], ", "then ", "{Ctrl+S}."]
        );
        assert_eq!(
            pieces("กด [Compare] หรือ {Enter}"),
            ["กด ", "[Compare] ", "หรือ ", "{Enter}"]
        );
        assert_eq!(pieces("BASE [[OUTPUT]] x"), ["BASE ", "[[OUTPUT]] ", "x"]);
        assert_eq!(pieces("ab cd  "), ["ab ", "cd ", " "]);
        assert_eq!(pieces("เทียบ โฟลเดอร์"), ["เทียบ ", "โฟลเดอร์"]);
    }

    #[test]
    fn keys_use_the_known_names_only() {
        for keys in [
            "Ctrl+S",
            "Alt+→",
            "Shift",
            "F1",
            "F12",
            "Esc",
            "Enter",
            "Ctrl+Home",
            "Del",
            "Ctrl+1",
            "N",
            "Ctrl+Tab",
        ] {
            assert!(keys_are_known(keys), "{keys}");
        }
        for keys in [
            "ctrl+s",
            "F13",
            "F0",
            "Ctrl+",
            "Strg+S",
            "Ctrl+click",
            "n",
            "Ctrl+wheel",
        ] {
            assert!(!keys_are_known(keys), "{keys}");
        }
    }
}
