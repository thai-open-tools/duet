//! Reads Rust source well enough to list its string literals and the calls around each one, for
//! the tests that hold the sources to rules about their text. Comments, char literals, lifetimes,
//! byte strings and attributes hold no literal it reports, nor do items under `#[cfg(test)]` or
//! `#[test]`. Included by `#[path]` in each crate's tests.

/// A string literal in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lit {
    /// Its text, unescaped.
    pub text: String,
    /// The 1-based line it starts on.
    pub line: u32,
    /// The calls around it, innermost first, as written: `tr`, `format!`, `StatusPart::new`.
    pub calls: Vec<String>,
    /// Which argument of the innermost call it sits in. An array or a struct is one argument.
    pub arg: usize,
}

/// Every string literal of `code` outside what the module's comment lists.
pub fn scan(code: &str) -> Vec<Lit> {
    let mut scanner = Scanner {
        chars: code.chars().collect(),
        at: 0,
        line: 1,
        frames: Vec::new(),
        name: None,
        after_colons: false,
        hide_next_item: false,
        hidden_at: None,
        out: Vec::new(),
    };
    scanner.run();
    scanner.out
}

/// An open bracket: the call it belongs to, if any, and how many commas that call has seen.
struct Frame {
    call: Option<String>,
    arg: usize,
}

struct Scanner {
    chars: Vec<char>,
    at: usize,
    line: u32,
    frames: Vec<Frame>,
    /// The path just read, which a following `(` calls.
    name: Option<String>,
    /// The last thing read was `::`, so the next word extends `name`.
    after_colons: bool,
    /// A test attribute was just read: the next item is hidden.
    hide_next_item: bool,
    /// The bracket depth a hidden item started at, while inside it.
    hidden_at: Option<usize>,
    out: Vec<Lit>,
}

/// Words that a `(` or a `[` can follow without calling anything.
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "fn", "for", "if",
    "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "static",
    "struct", "trait", "type", "unsafe", "use", "where", "while",
];

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl Scanner {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.at + ahead).copied()
    }

    /// Moves on one character, counting lines.
    fn bump(&mut self) -> Option<char> {
        let c = self.peek(0)?;
        self.at += 1;
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn run(&mut self) {
        while let Some(c) = self.peek(0) {
            match c {
                '/' if self.peek(1) == Some('/') => self.skip_line_comment(),
                '/' if self.peek(1) == Some('*') => self.skip_block_comment(),
                c if c.is_whitespace() => {
                    self.bump();
                }
                '#' if self.peek(1) == Some('[')
                    || (self.peek(1) == Some('!') && self.peek(2) == Some('[')) =>
                {
                    self.read_attribute();
                }
                _ => {
                    if self.hide_next_item {
                        self.hide_next_item = false;
                        self.hidden_at = Some(self.frames.len());
                    }
                    self.read_token(c);
                }
            }
        }
    }

    fn read_token(&mut self, c: char) {
        match c {
            '"' => {
                self.forget_name();
                let line = self.line;
                let text = self.read_string();
                self.report(text, line);
            }
            '\'' => {
                self.forget_name();
                self.read_quote();
            }
            ':' if self.peek(1) == Some(':') => {
                self.bump();
                self.bump();
                self.after_colons = true;
                if self.peek(0) == Some('<') {
                    self.skip_generics();
                }
            }
            '!' if self.name.is_some() && self.next_opens_a_bracket() => {
                self.bump();
                if let Some(name) = &mut self.name {
                    name.push('!');
                }
            }
            '(' | '[' | '{' => {
                self.bump();
                self.open(c);
            }
            ')' | ']' | '}' => {
                self.bump();
                self.close(c);
            }
            ',' => {
                self.bump();
                self.forget_name();
                if let Some(frame) = self.frames.last_mut().filter(|frame| frame.call.is_some()) {
                    frame.arg += 1;
                }
            }
            ';' => {
                self.bump();
                self.forget_name();
                if self.hidden_at == Some(self.frames.len()) {
                    self.hidden_at = None;
                }
            }
            c if c.is_ascii_digit() => {
                self.forget_name();
                while self.peek(0).is_some_and(is_word_char) {
                    self.bump();
                }
            }
            c if is_word_char(c) => self.read_word(),
            _ => {
                self.bump();
                self.forget_name();
            }
        }
    }

    fn forget_name(&mut self) {
        self.name = None;
        self.after_colons = false;
    }

    fn report(&mut self, text: String, line: u32) {
        if self.hidden_at.is_some() {
            return;
        }
        let calls = self
            .frames
            .iter()
            .rev()
            .filter_map(|f| f.call.clone())
            .collect();
        let arg = self
            .frames
            .iter()
            .rev()
            .find(|frame| frame.call.is_some())
            .map_or(0, |frame| frame.arg);
        self.out.push(Lit {
            text,
            line,
            calls,
            arg,
        });
    }

    fn open(&mut self, bracket: char) {
        let name = self.name.take().filter(|name| {
            // `f(`, and a macro with any bracket; `x[` indexes and `S {` builds.
            (bracket == '(' || name.ends_with('!')) && !KEYWORDS.contains(&name.as_str())
        });
        self.after_colons = false;
        self.frames.push(Frame { call: name, arg: 0 });
    }

    fn close(&mut self, bracket: char) {
        self.frames.pop();
        self.forget_name();
        if let Some(depth) = self.hidden_at {
            let done = if bracket == '}' {
                self.frames.len() <= depth
            } else {
                // An item that was a field: the bracket around it closed.
                self.frames.len() < depth
            };
            if done {
                self.hidden_at = None;
            }
        }
    }

    /// A word, or the prefix of a raw, byte or C string.
    fn read_word(&mut self) {
        let start = self.at;
        while self.peek(0).is_some_and(is_word_char) {
            self.bump();
        }
        let word: String = self.chars[start..self.at].iter().collect();
        let next = self.peek(0);
        match (word.as_str(), next) {
            ("r" | "br" | "cr", Some('"')) => {
                let line = self.line;
                let text = self.read_raw_string();
                self.forget_name();
                if word == "r" {
                    self.report(text, line);
                }
            }
            ("r" | "br" | "cr", Some('#')) => {
                let after_hashes = (1..)
                    .map(|ahead| self.peek(ahead))
                    .find(|c| *c != Some('#'))
                    .flatten();
                if after_hashes == Some('"') {
                    let line = self.line;
                    let text = self.read_raw_string();
                    self.forget_name();
                    if word == "r" {
                        self.report(text, line);
                    }
                } else {
                    // r#type: a name, as written, and never a keyword.
                    self.bump();
                    self.forget_name();
                    self.read_word();
                    if let Some(name) = &mut self.name {
                        name.insert_str(0, "r#");
                    }
                }
            }
            ("b" | "c", Some('"')) => {
                self.read_string();
                self.forget_name();
            }
            ("b", Some('\'')) => {
                self.read_quote();
                self.forget_name();
            }
            _ => {
                self.name = Some(match self.name.take().filter(|_| self.after_colons) {
                    Some(path) => format!("{path}::{word}"),
                    None => word,
                });
                self.after_colons = false;
            }
        }
    }

    fn next_opens_a_bracket(&self) -> bool {
        let next = (1..)
            .map(|ahead| self.peek(ahead))
            .find(|c| !c.is_some_and(char::is_whitespace))
            .flatten();
        matches!(next, Some('(' | '[' | '{'))
    }

    /// `"…"`, with the opening quote still ahead, unescaped.
    fn read_string(&mut self) -> String {
        self.bump();
        let mut text = String::new();
        while let Some(c) = self.bump() {
            match c {
                '"' => break,
                '\\' => self.read_escape(&mut text),
                c => text.push(c),
            }
        }
        text
    }

    fn read_escape(&mut self, text: &mut String) {
        match self.bump() {
            Some('n') => text.push('\n'),
            Some('r') => text.push('\r'),
            Some('t') => text.push('\t'),
            Some('0') => text.push('\0'),
            Some('x') => {
                let hex: String = [self.bump(), self.bump()].iter().flatten().collect();
                if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    text.push(c);
                }
            }
            Some('u') => {
                let mut hex = String::new();
                while let Some(c) = self.bump() {
                    match c {
                        '{' | '_' => {}
                        '}' => break,
                        c => hex.push(c),
                    }
                }
                if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    text.push(c);
                }
            }
            // A line continuation: the line break and the next line's indent are not text.
            Some('\n' | '\r') => {
                while self.peek(0).is_some_and(char::is_whitespace) {
                    self.bump();
                }
            }
            Some(c) => text.push(c),
            None => {}
        }
    }

    /// `"…"` or `#"…"#`, with the first of those still ahead.
    fn read_raw_string(&mut self) -> String {
        let mut hashes = 0;
        while self.peek(0) == Some('#') {
            self.bump();
            hashes += 1;
        }
        self.bump();
        let mut text = String::new();
        while let Some(c) = self.bump() {
            if c == '"' && (0..hashes).all(|ahead| self.peek(ahead) == Some('#')) {
                for _ in 0..hashes {
                    self.bump();
                }
                break;
            }
            text.push(c);
        }
        text
    }

    /// A char literal or a lifetime, with its quote still ahead.
    fn read_quote(&mut self) {
        self.bump();
        if self.peek(0) == Some('\\') {
            self.bump();
            self.bump();
            while let Some(c) = self.bump() {
                if c == '\'' {
                    break;
                }
            }
        } else if self.peek(1) == Some('\'') {
            self.bump();
            self.bump();
        } else {
            while self.peek(0).is_some_and(is_word_char) {
                self.bump();
            }
        }
    }

    fn skip_line_comment(&mut self) {
        while self.peek(0).is_some_and(|c| c != '\n') {
            self.bump();
        }
    }

    fn skip_block_comment(&mut self) {
        let mut depth = 0;
        while self.peek(0).is_some() {
            if self.peek(0) == Some('/') && self.peek(1) == Some('*') {
                depth += 1;
                self.bump();
            } else if self.peek(0) == Some('*') && self.peek(1) == Some('/') {
                depth -= 1;
                self.bump();
                if depth == 0 {
                    self.bump();
                    return;
                }
            }
            self.bump();
        }
    }

    /// `::<…>`, with its `<` still ahead. `->` and `=>` inside close nothing.
    fn skip_generics(&mut self) {
        let mut depth = 0;
        let mut previous = ' ';
        while let Some(c) = self.bump() {
            match c {
                '<' => depth += 1,
                '>' if previous != '-' && previous != '=' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            previous = c;
        }
        self.after_colons = false;
    }

    /// `#[…]` or `#![…]`, with its `#` still ahead. Nothing in it is reported, and a test
    /// attribute hides the item after it.
    fn read_attribute(&mut self) {
        while self.peek(0).is_some_and(|c| c != '[') {
            self.bump();
        }
        self.bump();
        let mut depth = 1;
        let mut words = String::new();
        while depth > 0 {
            match self.peek(0) {
                None => return,
                Some('"') => {
                    self.read_string();
                }
                Some('[') => {
                    self.bump();
                    depth += 1;
                }
                Some(']') => {
                    self.bump();
                    depth -= 1;
                }
                Some(c) => {
                    self.bump();
                    if !c.is_whitespace() {
                        words.push(c);
                    }
                }
            }
        }
        self.forget_name();
        if words == "cfg(test)" || words == "test" || words.ends_with("::test") {
            self.hide_next_item = true;
        }
    }
}
