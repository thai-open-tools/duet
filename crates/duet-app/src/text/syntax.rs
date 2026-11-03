//! Syntax colors from tree-sitter (gpui-component's highlighter). Each side of a text tab owns
//! one highlighter over a rope copy of its text; the first parse and any parse that overruns
//! its budget run on the background executor.
//!
//! The highlighter's timeout bounds only the parse: the injection pass after it (one sub-parse
//! per Rust macro call, say) has none. So an edit times the whole re-parse, and a side whose
//! re-parse overran, or would overrun by a trial made while it parsed, only shifts its old
//! styles on the UI thread and leaves re-parsing to the background.

use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Once, Weak};
use std::time::{Duration, Instant};

use duet_core::cancel::CancelToken;
use duet_core::text::buffer::Change;
use duet_core::text::lexer::{Language, LexState, Token, TokenClass, detect_language, lex_line};
use gpui_component::highlighter::{
    GrammarConfig, HighlightTheme, LanguageRegistry, SyntaxHighlighter,
};
use gpui_kit::component::Rope;
use gpui_kit::{HighlightStyle, Hsla};
use tree_sitter::{InputEdit, Point};

use crate::palette::Palette;

/// Sides larger than this stay plain (spec 6: highlighting for files up to 10 MiB).
pub const MAX_SYNTAX_BYTES: usize = 10 * 1024 * 1024;

/// How long an edit may re-parse on the UI thread before the rest moves to the background.
pub const EDIT_BUDGET: Duration = Duration::from_millis(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxState {
    /// Not highlighted, and why ("plain text", "over 10 MiB", "turned off").
    Off(&'static str),
    Parsing,
    Ready,
}

pub fn language_for(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "py" | "pyw" => "python",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "cs" => "csharp",
        "json" | "jsonc" => "json",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "html" | "htm" => "html",
        "css" => "css",
        "sql" => "sql",
        "md" | "markdown" => "markdown",
        "sh" | "bash" => "bash",
        "ps1" | "psm1" | "psd1" => "powershell",
        _ => return None,
    })
}

/// What colours a side (R10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxChoice {
    Tree(&'static str),
    Lex(Language),
    Plain,
}

impl SyntaxChoice {
    /// The name the Syntax tooltip shows; None for a plain side.
    pub fn label(self) -> Option<&'static str> {
        match self {
            SyntaxChoice::Tree(name) => Some(name),
            SyntaxChoice::Lex(language) => Some(language.label()),
            SyntaxChoice::Plain => None,
        }
    }
}

/// How a file is coloured (R10). Auto keeps tree-sitter for the extensions `language_for`
/// knows, else the lexer for a detected language, else plain. An override uses tree-sitter
/// where duet has the grammar, the lexer otherwise; Plain text is plain.
pub fn syntax_choice(path: &Path, language: Option<Language>) -> SyntaxChoice {
    match language {
        Some(Language::Text) => SyntaxChoice::Plain,
        Some(language) => {
            tree_sitter_name(language).map_or(SyntaxChoice::Lex(language), SyntaxChoice::Tree)
        }
        None => {
            if let Some(name) = language_for(path) {
                return SyntaxChoice::Tree(name);
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            match detect_language(name) {
                Language::Text => SyntaxChoice::Plain,
                detected => SyntaxChoice::Lex(detected),
            }
        }
    }
}

/// The tree-sitter grammar for a language chosen in the Type menu, where duet has one.
pub fn tree_sitter_name(language: Language) -> Option<&'static str> {
    Some(match language {
        Language::Js => "javascript",
        Language::Java => "java",
        Language::Go => "go",
        Language::Python => "python",
        Language::Shell => "bash",
        Language::PowerShell => "powershell",
        Language::Sql => "sql",
        Language::Css => "css",
        Language::Json => "json",
        Language::Yaml => "yaml",
        _ => return None,
    })
}

/// Registers the grammars gpui-component does not ship (PowerShell). Safe to call twice.
/// Only stores the grammar and its query text; queries compile when a highlighter is made.
pub fn register_extra_languages() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // tree-sitter-powershell 0.26.4 ships only a highlights query.
        LanguageRegistry::singleton().register(
            "powershell",
            &GrammarConfig::new(
                "powershell",
                tree_sitter_powershell::LANGUAGE.into(),
                vec![],
                tree_sitter_powershell::HIGHLIGHTS_QUERY,
                "",
                "",
            ),
        );
    });
}

/// One side's colours: a tree-sitter highlighter over a rope copy of its text, or the tokens
/// of the line lexer over the whole text.
pub struct SyntaxSide {
    engine: Engine,
    /// Bumped by every edit.
    pub version: u64,
    /// Lives exactly as long as this side, so tests can see where a side is dropped.
    witness: Arc<()>,
}

enum Engine {
    Tree(Box<TreeSide>),
    Lex(LexSide),
}

struct TreeSide {
    language: &'static str,
    highlighter: SyntaxHighlighter,
    rope: Rope,
    /// How long one edit may re-parse on the UI thread.
    budget: Duration,
    /// False once a re-parse overran the budget, or the trial said it would: edits then only
    /// shift the styles, until a background parse replaces this side (and tries again).
    reparse_on_edit: bool,
    /// Re-parses `edit` ran on the calling thread.
    reparses: u64,
}

/// The lexer's tokens over a whole side, in order, never overlapping, as absolute bytes. An edit
/// only moves the split: the tokens from `split` on lie `delta` bytes further on than they are
/// stored, so typing costs the tokens between two edits, not the whole side (I4).
struct LexSide {
    language: Language,
    tokens: Vec<Token>,
    split: usize,
    delta: isize,
    /// Tokens the last `shift` wrote, for the tests.
    #[cfg(test)]
    writes: usize,
}

impl SyntaxSide {
    /// Parses `text` to completion (run it on the background executor).
    pub fn parse_now(language: &'static str, text: &str) -> Self {
        Self::parse_with_budget(language, text, EDIT_BUDGET)
    }

    /// `parse_now` for edits allowed `budget` each on the UI thread. Also times one trial
    /// re-parse, so a side that cannot re-parse within the budget never makes a keystroke wait.
    pub fn parse_with_budget(language: &'static str, text: &str, budget: Duration) -> Self {
        let rope = Rope::from_str(text);
        let mut highlighter = SyntaxHighlighter::new(language);
        highlighter.update(None, &rope, None);
        let mut side = TreeSide {
            language,
            highlighter,
            rope,
            budget,
            reparse_on_edit: true,
            reparses: 0,
        };
        side.reparse_on_edit = side.trial_reparse_fits(text);
        Self::of(Engine::Tree(Box::new(side)))
    }

    /// Lexes every line of `text` (run it on the background executor). Block comments and
    /// strings carry from line to line as the lexer's state. None once `cancel` is set (it is
    /// checked every 4,096 lines): a newer lex took over.
    pub fn lex_now(language: Language, text: &str, cancel: &CancelToken) -> Option<Self> {
        let mut tokens = Vec::new();
        let mut state = LexState::default();
        let mut line_tokens = Vec::new();
        let bytes = text.as_bytes();
        let mut start = 0;
        let mut line = 0usize;
        while start < bytes.len() {
            if line.is_multiple_of(4_096) && cancel.is_cancelled() {
                return None;
            }
            line += 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'\n' && bytes[end] != b'\r' {
                end += 1;
            }
            line_tokens.clear();
            lex_line(&text[start..end], language, &mut state, &mut line_tokens);
            tokens.extend(line_tokens.iter().map(|t| Token {
                start: t.start + start,
                end: t.end + start,
                class: t.class,
            }));
            start = end;
            if bytes[start..].starts_with(b"\r\n") {
                start += 2;
            } else {
                start += 1;
            }
        }
        let split = tokens.len();
        Some(Self::of(Engine::Lex(LexSide {
            language,
            tokens,
            split,
            delta: 0,
            #[cfg(test)]
            writes: 0,
        })))
    }

    /// Builds the side a choice names; None for a plain side, or a lex `cancel` stopped.
    pub fn build(
        choice: SyntaxChoice,
        text: &str,
        budget: Duration,
        cancel: &CancelToken,
    ) -> Option<Self> {
        match choice {
            SyntaxChoice::Tree(name) => Some(Self::parse_with_budget(name, text, budget)),
            SyntaxChoice::Lex(language) => Self::lex_now(language, text, cancel),
            SyntaxChoice::Plain => None,
        }
    }

    fn of(engine: Engine) -> Self {
        Self {
            engine,
            version: 0,
            witness: Arc::new(()),
        }
    }

    /// "rust" for tree-sitter, the language's name for the lexer.
    pub fn label(&self) -> &'static str {
        match &self.engine {
            Engine::Tree(tree) => tree.language,
            Engine::Lex(lex) => lex.language.label(),
        }
    }

    /// The line lexer colours this side: an edit only shifts its tokens, and the view lexes
    /// it again after a pause in typing.
    pub fn is_lexed(&self) -> bool {
        matches!(self.engine, Engine::Lex(_))
    }

    /// For tests: a new budget for the edits that follow.
    pub fn set_budget(&mut self, budget: Duration) {
        if let Engine::Tree(tree) = &mut self.engine {
            tree.budget = budget;
        }
    }

    /// For tests: a handle that stops upgrading once this side is dropped.
    pub fn witness(&self) -> Weak<()> {
        Arc::downgrade(&self.witness)
    }

    /// For tests: how many edits re-parsed on the calling thread.
    pub fn reparses(&self) -> u64 {
        match &self.engine {
            Engine::Tree(tree) => tree.reparses,
            Engine::Lex(_) => 0,
        }
    }

    /// Applies a buffer change and re-parses within the edit budget. False when the budget ran
    /// out, now or before (or the change does not fit the rope): the styles are the old ones,
    /// shifted by the edit, until a background `parse_now` replaces this side. A lexed side
    /// always returns false: it shifts its tokens and is lexed again in the background.
    pub fn edit(&mut self, change: &Change, inserted: &str) -> bool {
        match &mut self.engine {
            Engine::Tree(tree) => match tree.edit(change, inserted) {
                None => false,
                Some(fits) => {
                    self.version += 1;
                    fits
                }
            },
            Engine::Lex(lex) => {
                lex.shift(change);
                self.version += 1;
                false
            }
        }
    }

    /// Styles for the bytes `line_start..line_end`, as ranges relative to `line_start`. Bytes
    /// no capture styles are left out.
    pub fn line_styles(
        &self,
        line_start: usize,
        line_end: usize,
        theme: &HighlightTheme,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        if line_start >= line_end {
            return Vec::new();
        }
        match &self.engine {
            Engine::Tree(tree) => tree.line_styles(line_start, line_end, theme),
            Engine::Lex(lex) => lex.line_styles(line_start, line_end, theme),
        }
    }
}

impl TreeSide {
    /// None when the change does not fit the rope.
    fn edit(&mut self, change: &Change, inserted: &str) -> Option<bool> {
        self.rope
            .try_remove(change.start_byte..change.old_end_byte)
            .and_then(|()| self.rope.try_insert(change.start_byte, inserted))
            .ok()?;
        let point = |(row, column): (usize, usize)| Point { row, column };
        let edit = InputEdit {
            start_byte: change.start_byte,
            old_end_byte: change.old_end_byte,
            new_end_byte: change.new_end_byte,
            start_position: point(change.start_point),
            old_end_position: point(change.old_end_point),
            new_end_position: point(change.new_end_point),
        };
        if !self.reparse_on_edit {
            self.highlighter.edit_tree(Some(edit), &self.rope);
            return Some(false);
        }
        self.reparses += 1;
        let started = Instant::now();
        let parsed = self
            .highlighter
            .update(Some(edit), &self.rope, Some(self.budget));
        // The parse's own timeout leaves the injection pass out, so the whole re-parse counts.
        if parsed && started.elapsed() < self.budget {
            return Some(true);
        }
        self.reparse_on_edit = false;
        Some(false)
    }

    /// Times one re-parse as an edit runs it: a space added at the end, which is then taken
    /// back by a re-parse without a time limit.
    fn trial_reparse_fits(&mut self, text: &str) -> bool {
        let end = text.len();
        let row = text.bytes().filter(|&b| b == b'\n').count();
        let column = end - text.rfind('\n').map_or(0, |i| i + 1);
        let (at, past) = (
            Point { row, column },
            Point {
                row,
                column: column + 1,
            },
        );
        let mut trial = self.rope.clone();
        trial.insert(end, " ");
        let started = Instant::now();
        let parsed = self.highlighter.update(
            Some(InputEdit {
                start_byte: end,
                old_end_byte: end,
                new_end_byte: end + 1,
                start_position: at,
                old_end_position: at,
                new_end_position: past,
            }),
            &trial,
            Some(self.budget),
        );
        let fits = parsed && started.elapsed() < self.budget;
        self.highlighter.update(
            Some(InputEdit {
                start_byte: end,
                old_end_byte: end + 1,
                new_end_byte: end,
                start_position: at,
                old_end_position: past,
                new_end_position: at,
            }),
            &self.rope,
            None,
        );
        fits
    }

    fn line_styles(
        &self,
        line_start: usize,
        line_end: usize,
        theme: &HighlightTheme,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        self.highlighter
            .styles(&(line_start..line_end), theme)
            .into_iter()
            .filter(|(r, s)| {
                r.end > line_start && r.start < line_end && *s != HighlightStyle::default()
            })
            .map(|(r, s)| {
                (
                    r.start.max(line_start) - line_start..r.end.min(line_end) - line_start,
                    s,
                )
            })
            .collect()
    }
}

impl LexSide {
    /// Token `at`'s true place.
    fn place(&self, at: usize) -> (usize, usize) {
        let token = &self.tokens[at];
        let by = if at >= self.split { self.delta } else { 0 };
        (
            token.start.wrapping_add_signed(by),
            token.end.wrapping_add_signed(by),
        )
    }

    /// The first token at or after `from` for which `before(start, end)` is false, by true
    /// places; `before` must hold for a run of tokens and then never again.
    fn first_not(&self, from: usize, before: impl Fn(usize, usize) -> bool) -> usize {
        let split = self.split.max(from);
        let (head, tail) = self.tokens.split_at(split);
        let at = from + head[from..].partition_point(|t| before(t.start, t.end));
        if at < split {
            return at;
        }
        let by = self.delta;
        split
            + tail.partition_point(|t| {
                before(
                    t.start.wrapping_add_signed(by),
                    t.end.wrapping_add_signed(by),
                )
            })
    }

    /// Moves the tokens the change leaves whole; the ones it touches are dropped (left empty)
    /// until the next lexing. Only the tokens between the last edit and this one are written.
    fn shift(&mut self, change: &Change) {
        let (start, old_end) = (change.start_byte, change.old_end_byte);
        let delta = change.new_end_byte as isize - old_end as isize;
        let first = self.first_not(0, |_, end| end <= start);
        let last = self.first_not(first, |begin, _| begin < old_end);
        // Every token before `last` takes its true place, every one after it keeps the old
        // shift stored, and the split moves to `last`.
        let (from, to, by) = if self.split < last {
            (self.split, last, self.delta)
        } else {
            (last, self.split, -self.delta)
        };
        if by != 0 {
            for token in &mut self.tokens[from..to] {
                token.start = token.start.wrapping_add_signed(by);
                token.end = token.end.wrapping_add_signed(by);
            }
        }
        for token in &mut self.tokens[first..last] {
            (token.start, token.end) = (start, start);
        }
        #[cfg(test)]
        {
            let moved = if by != 0 { to - from } else { 0 };
            self.writes = moved + (last - first);
        }
        self.split = last;
        self.delta += delta;
    }

    fn line_styles(
        &self,
        line_start: usize,
        line_end: usize,
        theme: &HighlightTheme,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let first = self.first_not(0, |_, end| end <= line_start);
        let palette = Palette::of(theme.appearance.is_dark());
        (first..self.tokens.len())
            .map(|at| (at, self.place(at)))
            .take_while(|&(_, (start, _))| start < line_end)
            .filter(|&(_, (start, end))| start < end)
            .filter_map(|(at, (start, end))| {
                let style = lex_style(self.language, self.tokens[at].class, theme, &palette)?;
                Some((
                    start.max(line_start) - line_start..end.min(line_end) - line_start,
                    style,
                ))
            })
            .collect()
    }
}

/// A lexer class's colour: the syntax theme's name for it (R10), or the palette's for the
/// log levels and a patch's added lines (green, as its removed lines are red: X-87).
fn lex_style(
    language: Language,
    class: TokenClass,
    theme: &HighlightTheme,
    palette: &Palette,
) -> Option<HighlightStyle> {
    let name = match class {
        TokenClass::Key if language == Language::Diff => return Some(colour(palette.ins)),
        TokenClass::Comment => "comment",
        TokenClass::String => "string",
        TokenClass::Number | TokenClass::DateTime => "number",
        TokenClass::Keyword => "keyword",
        TokenClass::Key => "property",
        TokenClass::Tag => "tag",
        TokenClass::Attr => "attribute",
        TokenClass::Error => return Some(colour(palette.del)),
        TokenClass::Warn => return Some(colour(palette.modified)),
        TokenClass::Info => return Some(colour(palette.accent)),
        TokenClass::Muted => return Some(colour(palette.dim)),
    };
    theme.style.syntax.style(name)
}

fn colour(color: Hsla) -> HighlightStyle {
    HighlightStyle {
        color: Some(color),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use duet_core::text::buffer::TextBuffer;
    use std::path::Path;

    #[test]
    fn languages_come_from_the_extension() {
        let cases = [
            ("a.rs", Some("rust")),
            ("a.ts", Some("typescript")),
            ("a.tsx", Some("tsx")),
            ("a.mjs", Some("javascript")),
            ("a.py", Some("python")),
            ("a.go", Some("go")),
            ("A.java", Some("java")),
            ("a.h", Some("c")),
            ("a.hpp", Some("cpp")),
            ("a.cs", Some("csharp")),
            ("a.json", Some("json")),
            ("a.yml", Some("yaml")),
            ("Cargo.toml", Some("toml")),
            ("a.htm", Some("html")),
            ("a.css", Some("css")),
            ("a.sql", Some("sql")),
            ("README.md", Some("markdown")),
            ("a.sh", Some("bash")),
            ("a.PS1", Some("powershell")),
            ("notes.txt", None),
            ("Makefile", None),
        ];
        for (name, want) in cases {
            assert_eq!(language_for(Path::new(name)), want, "{name}");
        }
    }

    #[test]
    fn the_highlighter_can_move_to_a_background_thread() {
        fn send<T: Send>() {}
        send::<SyntaxHighlighter>();
    }

    #[test]
    fn powershell_is_registered_and_colors_a_variable() {
        register_extra_languages();
        register_extra_languages();
        let text = "$name = 'x'\n";
        let side = SyntaxSide::parse_now("powershell", text);
        let styles = side.line_styles(0, text.len() - 1, &HighlightTheme::default_light());
        assert!(
            styles.iter().any(|(r, _)| r.start == 0),
            "the variable is styled: {styles:?}"
        );
    }

    #[test]
    fn an_edit_reparses_incrementally_and_shifts_styles() {
        let text = "fn a() {}\n";
        // No time limit: on a busy machine even a small re-parse can overrun 5 ms.
        let mut side = SyntaxSide::parse_with_budget("rust", text, Duration::MAX);
        let mut buffer = duet_core::text::buffer::TextBuffer::new(text.into());
        let change = buffer.replace(0..0, "// ");
        assert!(side.edit(&change, "// "), "the edit re-parses in place");
        let styles = side.line_styles(0, buffer.line_end(0), &HighlightTheme::default_light());
        let comment = styles
            .iter()
            .find(|(r, _)| r.start == 0)
            .map(|(r, _)| r.clone());
        assert_eq!(
            comment,
            Some(0..buffer.line_end(0)),
            "the whole line is now a comment"
        );
    }

    #[test]
    fn bash_css_go_javascript_and_python_color_a_keyword_line() {
        let cases = [
            ("bash", "if true; then echo hi; fi\n", "if"),
            ("css", "a { color: red; }\n", "color"),
            ("go", "func main() {}\n", "func"),
            ("javascript", "function f() { return 1; }\n", "function"),
            ("python", "def f():\n    return 1\n", "def"),
        ];
        for (language, text, word) in cases {
            let side = SyntaxSide::parse_now(language, text);
            let line_end = text.find('\n').unwrap();
            let styles = side.line_styles(0, line_end, &HighlightTheme::default_light());
            let at = text.find(word).unwrap();
            assert!(
                styles
                    .iter()
                    .any(|(r, s)| r.start == at && s.color.is_some()),
                "{language}: `{word}` is colored: {styles:?}"
            );
        }
    }

    fn styles_of(side: &SyntaxSide, buffer: &TextBuffer) -> Vec<(Range<usize>, HighlightStyle)> {
        side.line_styles(0, buffer.line_end(0), &HighlightTheme::default_light())
    }

    #[test]
    fn after_an_overrun_edits_only_shift_the_styles() {
        let text = "fn a() {}\n";
        // No limit while parsing, so the trial re-parse fits and edits start out re-parsing.
        let mut side = SyntaxSide::parse_with_budget("rust", text, Duration::MAX);
        side.set_budget(Duration::ZERO);
        let mut buffer = TextBuffer::new(text.into());
        let change = buffer.replace(0..0, " ");
        assert!(
            !side.edit(&change, " "),
            "nothing fits a zero budget: a background parse follows"
        );
        assert_eq!(side.reparses(), 1, "the first edit tried to re-parse");
        let change = buffer.replace(0..0, "  ");
        assert!(!side.edit(&change, "  "));
        assert_eq!(side.reparses(), 1, "after the overrun, edits only shift");
        let styles = styles_of(&side, &buffer);
        assert!(
            styles
                .iter()
                .any(|(r, s)| *r == (3.."   fn".len()) && s.color.is_some()),
            "`fn` keeps its color, moved by the edits: {styles:?}"
        );
        let fresh = SyntaxSide::parse_now("rust", buffer.text());
        assert_eq!(
            styles,
            styles_of(&fresh, &buffer),
            "shifting by spaces gives what a fresh parse gives"
        );
    }

    #[test]
    fn a_side_whose_trial_reparse_overruns_starts_out_shifting() {
        let text = "fn a() {}\n";
        let mut side = SyntaxSide::parse_with_budget("rust", text, Duration::ZERO);
        let mut buffer = TextBuffer::new(text.into());
        let change = buffer.replace(0..0, "// ");
        assert!(!side.edit(&change, "// "));
        assert_eq!(side.reparses(), 0, "no keystroke waits for a re-parse");
        let styles = styles_of(&side, &buffer);
        assert!(
            styles
                .iter()
                .any(|(r, s)| *r == (3.."// fn".len()) && s.color.is_some()),
            "until the background parse lands, `fn` is still a keyword: {styles:?}"
        );
    }

    #[test]
    fn syntax_choices_follow_r10() {
        let choice = |name: &str, language| syntax_choice(Path::new(name), language);
        assert_eq!(choice("a.rs", None), SyntaxChoice::Tree("rust"));
        assert_eq!(choice("a.ini", None), SyntaxChoice::Lex(Language::Ini));
        assert_eq!(choice("a.txt", None), SyntaxChoice::Plain);
        assert_eq!(
            choice("a.txt", Some(Language::Python)),
            SyntaxChoice::Tree("python")
        );
        assert_eq!(
            choice("a.rs", Some(Language::Lua)),
            SyntaxChoice::Lex(Language::Lua)
        );
        assert_eq!(choice("a.rs", Some(Language::Text)), SyntaxChoice::Plain);
        assert_eq!(choice("a.xml", None), SyntaxChoice::Lex(Language::Markup));
    }

    #[test]
    fn a_lexed_side_styles_its_lines_and_shifts_on_edit() {
        let text = "[s]
k=1
";
        let theme = HighlightTheme::default_light();
        let mut side = SyntaxSide::lex_now(Language::Ini, text, &CancelToken::new()).unwrap();
        assert_eq!(side.label(), "INI");
        let head = side.line_styles(0, 3, &theme);
        assert_eq!(head.len(), 1, "{head:?}");
        assert_eq!(head[0].0, 0..3);
        let key = side.line_styles(4, 7, &theme);
        assert_eq!(key.len(), 1, "{key:?}");
        assert_eq!(key[0].0, 0..1);
        let mut buffer = TextBuffer::new(text.into());
        let change = buffer.replace(0..0, "x");
        assert!(
            !side.edit(&change, "x"),
            "a lexed side re-lexes in the background"
        );
        let moved = side.line_styles(0, 4, &theme);
        assert_eq!(moved[0].0.start, 1, "{moved:?}");
    }

    #[test]
    fn patch_lines_take_the_add_and_remove_colours() {
        // X-87: in a patch `+` lines are green and `-` lines red, unlike `@@`, in both themes.
        let text = "@@ -1 +1 @@\n+new\n-old\n";
        let side = SyntaxSide::lex_now(Language::Diff, text, &CancelToken::new()).unwrap();
        for dark in [false, true] {
            let theme = crate::theme::syntax_theme(dark);
            let palette = Palette::of(dark);
            let color = |start, end| side.line_styles(start, end, &theme)[0].1.color;
            assert_eq!(color(12, 16), Some(palette.ins), "+ line, dark {dark}");
            assert_eq!(color(17, 21), Some(palette.del), "- line, dark {dark}");
            assert_ne!(color(0, 11), Some(palette.ins), "@@ line, dark {dark}");
        }
    }

    #[test]
    fn a_cancelled_lex_gives_up() {
        let cancel = CancelToken::new();
        cancel.cancel();
        let text = "k=1\n".repeat(10);
        assert!(SyntaxSide::lex_now(Language::Ini, &text, &cancel).is_none());
        assert!(SyntaxSide::lex_now(Language::Ini, &text, &CancelToken::new()).is_some());
    }

    fn all_styles(side: &SyntaxSide, buffer: &TextBuffer) -> Vec<Vec<Range<usize>>> {
        let theme = HighlightTheme::default_light();
        (0..buffer.line_count())
            .map(|line| {
                side.line_styles(buffer.line_start(line), buffer.line_end(line), &theme)
                    .into_iter()
                    .map(|(range, _)| range)
                    .collect()
            })
            .collect()
    }

    fn lex_writes(side: &SyntaxSide) -> usize {
        match &side.engine {
            Engine::Lex(lex) => lex.writes,
            Engine::Tree(_) => 0,
        }
    }

    #[test]
    fn a_lexed_edit_touches_only_the_tokens_near_it() {
        let text: String = (0..2_000).map(|i| format!("key{i}=1\n")).collect();
        let mut side = SyntaxSide::lex_now(Language::Ini, &text, &CancelToken::new()).unwrap();
        let mut buffer = TextBuffer::new(text);
        // Typing inside a key in the middle, then above it, then below it, then a new line.
        let edits: [(usize, usize, &str, usize); 5] = [
            (1_000, 2, "x", 0),
            (1_000, 3, "y", 0),
            (10, 0, "zz", 0),
            (1_500, 0, "", 3),
            (1_999, 0, "\nnew=2", 0),
        ];
        for (line, column, inserted, removed) in edits {
            let at = buffer.line_start(line) + column;
            let change = buffer.replace(at..at + removed, inserted);
            side.edit(&change, inserted);
            if line == 1_000 && inserted == "y" {
                assert!(
                    lex_writes(&side) < 16,
                    "a second keystroke at the same place writes {} tokens",
                    lex_writes(&side)
                );
            }
        }
        // Every line the edits did not touch is styled as a fresh lex styles it.
        let fresh = SyntaxSide::lex_now(Language::Ini, buffer.text(), &CancelToken::new()).unwrap();
        let (got, want) = (all_styles(&side, &buffer), all_styles(&fresh, &buffer));
        for (line, (got, want)) in got.iter().zip(&want).enumerate() {
            if ![10, 1_000, 1_500, 1_999, 2_000].contains(&line) {
                assert_eq!(got, want, "line {line}");
            }
        }
        assert!(
            got[1_000].len() < want[1_000].len(),
            "the typed key waits for the lex"
        );
    }
}
