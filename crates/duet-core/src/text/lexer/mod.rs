//! LocalDiff's line lexer (`ldLex`, `ldLexMarkup`, `ldStripComments`, `de:287-631`): token
//! classes for colouring, and comment cutting for the compare rules. Offsets are bytes of the
//! line (without its line ending); identifier, number and keyword tests are ASCII, as in
//! LocalDiff, so every token starts and ends on a character boundary.

use std::borrow::Cow;
use std::sync::OnceLock;

use memchr::memmem;

mod keywords;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    CLike,
    Js,
    Java,
    Go,
    Python,
    Ruby,
    Shell,
    PowerShell,
    Batch,
    Sql,
    Markup,
    Css,
    Json,
    Yaml,
    Properties,
    Ini,
    Vb,
    Lua,
    Log,
    Diff,
    Text,
}

impl Language {
    /// LocalDiff's order (`LD_LANGS`).
    pub const ALL: [Language; 21] = [
        Language::CLike,
        Language::Js,
        Language::Java,
        Language::Go,
        Language::Python,
        Language::Ruby,
        Language::Shell,
        Language::PowerShell,
        Language::Batch,
        Language::Sql,
        Language::Markup,
        Language::Css,
        Language::Json,
        Language::Yaml,
        Language::Properties,
        Language::Ini,
        Language::Vb,
        Language::Lua,
        Language::Log,
        Language::Diff,
        Language::Text,
    ];

    /// LocalDiff's id.
    pub fn id(self) -> &'static str {
        match self {
            Language::CLike => "clike",
            Language::Js => "js",
            Language::Java => "java",
            Language::Go => "go",
            Language::Python => "py",
            Language::Ruby => "rb",
            Language::Shell => "sh",
            Language::PowerShell => "ps1",
            Language::Batch => "bat",
            Language::Sql => "sql",
            Language::Markup => "xml",
            Language::Css => "css",
            Language::Json => "json",
            Language::Yaml => "yaml",
            Language::Properties => "props",
            Language::Ini => "ini",
            Language::Vb => "vb",
            Language::Lua => "lua",
            Language::Log => "log",
            Language::Diff => "diff",
            Language::Text => "text",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|lang| lang.id() == id)
    }

    /// LocalDiff's name ("Plain text" for its `ข้อความ`).
    pub fn label(self) -> &'static str {
        match self {
            Language::CLike => "C-like",
            Language::Js => "JavaScript",
            Language::Java => "Java",
            Language::Go => "Go",
            Language::Python => "Python",
            Language::Ruby => "Ruby",
            Language::Shell => "Shell",
            Language::PowerShell => "PowerShell",
            Language::Batch => "Batch",
            Language::Sql => "SQL",
            Language::Markup => "XML/HTML",
            Language::Css => "CSS",
            Language::Json => "JSON",
            Language::Yaml => "YAML",
            Language::Properties => "Properties",
            Language::Ini => "INI",
            Language::Vb => "VB",
            Language::Lua => "Lua",
            Language::Log => "Log",
            Language::Diff => "Diff/Patch",
            Language::Text => "Plain text",
        }
    }

    /// Whether the language has comment syntax (`de:680`): JSON, Log, Diff/Patch and Plain
    /// text ignore nothing even with comments ignored.
    pub fn has_comments(self) -> bool {
        let def = self.def();
        !def.line.is_empty() || !def.block.is_empty() || !def.line_start.is_empty()
    }

    fn def(self) -> &'static Def {
        match self {
            Language::CLike => &CLIKE,
            Language::Js => &JS,
            Language::Java => &JAVA,
            Language::Go => &GO,
            Language::Python => &PYTHON,
            Language::Ruby => &RUBY,
            Language::Shell => &SHELL,
            Language::PowerShell => &POWERSHELL,
            Language::Batch => &BATCH,
            Language::Sql => &SQL,
            Language::Markup => &MARKUP,
            Language::Css => &CSS,
            Language::Json => &JSON,
            Language::Yaml => &YAML,
            Language::Properties => &PROPERTIES,
            Language::Ini => &INI,
            Language::Vb => &VB,
            Language::Lua => &LUA,
            Language::Log => &LOG,
            Language::Diff => &DIFF,
            Language::Text => &TEXT,
        }
    }
}

/// The language a file name suggests (`ldDetectLang` with `LD_EXT`, `de:335-365`).
pub fn detect_language(name: &str) -> Language {
    let base = name
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default()
        .to_lowercase();
    if base == "dockerfile" || base == "makefile" {
        return Language::Shell;
    }
    if is_log_name(&base) {
        return Language::Log;
    }
    match base.rfind('.') {
        Some(dot) => by_extension(&base[dot + 1..]),
        None => Language::Text,
    }
}

/// `\.log(\.\d+)?$`, or `\.log\.\d{4}-\d{2}-\d{2}` anywhere (rotated logs).
fn is_log_name(base: &str) -> bool {
    let numbered = base.trim_end_matches(|c: char| c.is_ascii_digit());
    if base.ends_with(".log") || (numbered.len() < base.len() && numbered.ends_with(".log.")) {
        return true;
    }
    base.match_indices(".log.").any(|(at, _)| {
        base.as_bytes().get(at + 5..at + 15).is_some_and(|date| {
            date.iter().enumerate().all(|(i, &b)| match i {
                4 | 7 => b == b'-',
                _ => b.is_ascii_digit(),
            })
        })
    })
}

/// `LD_EXT`.
fn by_extension(ext: &str) -> Language {
    match ext {
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" | "cs" | "rs" | "swift" | "dart"
        | "php" | "m" => Language::CLike,
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "jsonc" | "json5" => Language::Js,
        "java" | "kt" | "kts" | "scala" | "groovy" | "gradle" => Language::Java,
        "go" => Language::Go,
        "py" | "pyw" => Language::Python,
        "rb" => Language::Ruby,
        "pl" | "pm" | "sh" | "bash" | "zsh" | "ksh" | "env" | "dockerfile" | "mk" | "makefile" => {
            Language::Shell
        }
        "ps1" | "psm1" => Language::PowerShell,
        "bat" | "cmd" => Language::Batch,
        "sql" | "pks" | "pkb" | "prc" | "fnc" | "trg" | "ddl" => Language::Sql,
        "vue" | "jsp" | "xml" | "xsd" | "xsl" | "xslt" | "wsdl" | "xaml" | "html" | "htm"
        | "xhtml" | "svg" | "config" | "csproj" | "vbproj" | "props" | "targets" | "pom"
        | "plist" | "resx" | "nuspec" | "jrxml" | "fxml" => Language::Markup,
        "css" | "scss" | "less" | "sass" => Language::Css,
        "json" | "geojson" | "har" => Language::Json,
        "yml" | "yaml" => Language::Yaml,
        "properties" => Language::Properties,
        "ini" | "cfg" | "conf" | "toml" | "inf" | "reg" | "editorconfig" => Language::Ini,
        "vb" | "vbs" | "bas" => Language::Vb,
        "lua" => Language::Lua,
        "log" | "out" | "trc" => Language::Log,
        "diff" | "patch" => Language::Diff,
        _ => Language::Text,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenClass {
    Comment,
    String,
    Number,
    Keyword,
    Key,
    Tag,
    Attr,
    DateTime,
    Error,
    Warn,
    Info,
    Muted,
}

/// A token over bytes `start..end` of the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub class: TokenClass,
}

/// What a line leaves open for the next one (`ldLexState`). The default is "nothing open".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LexState {
    /// The open block comment, an index into the language's pairs (LocalDiff's `bc`).
    block: Option<u8>,
    /// The open string's quote (LocalDiff's `q`).
    quote: Option<Quote>,
    /// Inside a markup tag.
    tag: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quote {
    /// One quote byte (a JavaScript or Go backtick string).
    One(u8),
    /// The quote byte three times (`"""` or `'''`).
    Three(u8),
}

/// A language's lexer rules (`LD_LANGS`, `de:306-330`).
struct Def {
    /// Line comments (`lc`).
    line: &'static [&'static str],
    /// Block comments (`bc`).
    block: &'static [(&'static str, &'static str)],
    /// Comments that must start the line's text (`lcStart`), in lower case.
    line_start: &'static [&'static str],
    /// Quote bytes (`q`).
    quotes: &'static [u8],
    /// Quotes whose strings go on to the next line (`ml`).
    multi_line: &'static [u8],
    /// Triple quotes (`tq`).
    triple: &'static [&'static str],
    /// A `/` may start a regex literal (`rx`).
    regex: bool,
    /// `#word` at the start of the text is a keyword (`pp`).
    preprocessor: bool,
    /// A line comment needs the line start or a space or tab before it (`hashWs`).
    hash_ws: bool,
    /// Keywords and comment starts ignore case (`ci`).
    ignore_case: bool,
    /// A doubled quote is one quote and a backslash escapes nothing (`dq`).
    doubled: bool,
    markup: bool,
    /// A string followed by `:` is a key (`jsonKey`).
    json_key: bool,
    /// `key:` at the start of the line (`yamlKey`).
    yaml_key: bool,
    /// The text up to `=`, `:`, a space or a tab is a key (`propKey`).
    prop_key: bool,
    /// `[section]` lines (`iniSec`).
    ini_section: bool,
    log: bool,
    diff: bool,
    /// Sorted by bytes; empty for a language without keywords.
    keywords: &'static [&'static str],
}

const PLAIN: Def = Def {
    line: &[],
    block: &[],
    line_start: &[],
    quotes: b"",
    multi_line: b"",
    triple: &[],
    regex: false,
    preprocessor: false,
    hash_ws: false,
    ignore_case: false,
    doubled: false,
    markup: false,
    json_key: false,
    yaml_key: false,
    prop_key: false,
    ini_section: false,
    log: false,
    diff: false,
    keywords: &[],
};
const SLASHES: &[&str] = &["//"];
const SLASH_STAR: &[(&str, &str)] = &[("/*", "*/")];

static CLIKE: Def = Def {
    line: SLASHES,
    block: SLASH_STAR,
    quotes: b"\"'",
    keywords: keywords::CLIKE,
    preprocessor: true,
    ..PLAIN
};
static JS: Def = Def {
    line: SLASHES,
    block: SLASH_STAR,
    quotes: b"\"'`",
    multi_line: b"`",
    regex: true,
    keywords: keywords::CLIKE,
    ..PLAIN
};
static JAVA: Def = Def {
    line: SLASHES,
    block: SLASH_STAR,
    quotes: b"\"'",
    triple: &["\"\"\""],
    keywords: keywords::CLIKE,
    ..PLAIN
};
static GO: Def = Def {
    line: SLASHES,
    block: SLASH_STAR,
    quotes: b"\"'`",
    multi_line: b"`",
    keywords: keywords::CLIKE,
    ..PLAIN
};
static PYTHON: Def = Def {
    line: &["#"],
    quotes: b"\"'",
    triple: &["\"\"\"", "'''"],
    keywords: keywords::PYTHON,
    ..PLAIN
};
static RUBY: Def = Def {
    line: &["#"],
    quotes: b"\"'",
    keywords: keywords::CLIKE,
    ..PLAIN
};
static SHELL: Def = Def {
    line: &["#"],
    hash_ws: true,
    quotes: b"\"'",
    keywords: keywords::SHELL,
    ..PLAIN
};
static POWERSHELL: Def = Def {
    line: &["#"],
    block: &[("<#", "#>")],
    quotes: b"\"'",
    ignore_case: true,
    keywords: keywords::POWERSHELL,
    ..PLAIN
};
static BATCH: Def = Def {
    line_start: &["rem ", "::", "@rem "],
    ignore_case: true,
    quotes: b"\"",
    keywords: keywords::BATCH,
    ..PLAIN
};
static SQL: Def = Def {
    line: &["--"],
    block: SLASH_STAR,
    quotes: b"'\"",
    doubled: true,
    ignore_case: true,
    keywords: keywords::SQL,
    ..PLAIN
};
static MARKUP: Def = Def {
    markup: true,
    block: &[("<!--", "-->")],
    ..PLAIN
};
static CSS: Def = Def {
    block: SLASH_STAR,
    quotes: b"\"'",
    ..PLAIN
};
static JSON: Def = Def {
    quotes: b"\"",
    json_key: true,
    keywords: keywords::JSON,
    ..PLAIN
};
static YAML: Def = Def {
    line: &["#"],
    hash_ws: true,
    quotes: b"\"'",
    yaml_key: true,
    keywords: keywords::YAML,
    ..PLAIN
};
static PROPERTIES: Def = Def {
    line_start: &["#", "!"],
    prop_key: true,
    ..PLAIN
};
static INI: Def = Def {
    line_start: &[";", "#"],
    ini_section: true,
    prop_key: true,
    ..PLAIN
};
static VB: Def = Def {
    line: &["'"],
    quotes: b"\"",
    ignore_case: true,
    keywords: keywords::VB,
    ..PLAIN
};
static LUA: Def = Def {
    line: &["--"],
    quotes: b"\"'",
    keywords: keywords::LUA,
    ..PLAIN
};
static LOG: Def = Def { log: true, ..PLAIN };
static DIFF: Def = Def {
    diff: true,
    ..PLAIN
};
static TEXT: Def = PLAIN;

/// `LD_LOG_RE` with JavaScript's meaning: `\d` is ASCII, `\b` an ASCII word boundary, and `.`
/// stops at line separators.
const LOG_PATTERN: &str = concat!(
    r"([0-9]{4}[-/.][0-9]{2}[-/.][0-9]{2}[ T][0-9]{1,2}:[0-9]{2}:[0-9]{2}(?:[.,][0-9]+)?",
    r"(?:Z|[+-][0-9]{2}:?[0-9]{2})?|(?-u:\b)[0-9]{2}:[0-9]{2}:[0-9]{2}(?:[.,][0-9]+)?(?-u:\b))",
    r"|(?-u:\b)(ERROR|ERR|FATAL|SEVERE|CRITICAL|CRIT|EXCEPTION|FAIL|FAILED|FAILURE)(?-u:\b)",
    r"|(?-u:\b)(WARN|WARNING)(?-u:\b)|(?-u:\b)(INFO|NOTICE)(?-u:\b)",
    r"|(?-u:\b)(DEBUG|TRACE|VERBOSE|FINE|FINER|FINEST)(?-u:\b)",
    r#"|("(?:[^"\\]|\\[^\n\r\x{2028}\x{2029}])*")"#,
);

/// JavaScript's `\s`, the class LocalDiff's YAML key pattern uses.
const JS_SPACE: &str = concat!(
    r"\t\n\x0B\x0C\r \x{A0}\x{1680}\x{2000}-\x{200A}",
    r"\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}",
);

fn is_js_space(c: char) -> bool {
    ('\t'..='\r').contains(&c)
        || ('\u{2000}'..='\u{200a}').contains(&c)
        || matches!(
            c,
            ' ' | '\u{a0}'
                | '\u{1680}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
        )
}

fn log_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(LOG_PATTERN).expect("the log pattern compiles"))
}

/// LocalDiff's `^(\s*(?:-\s+)?)([^\s#'"\-][^:#]*?|"[^"]*"|'[^']*')\s*:(?=\s|$)` without the
/// look-ahead, which `yaml_key` checks: the key and its colon can only end one way, so
/// checking after the match finds what JavaScript's backtracking finds.
fn yaml_key_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let s = JS_SPACE;
        regex::Regex::new(&format!(
            r#"^([{s}]*(?:-[{s}]+)?)([^{s}#'"\-][^:#]*?|"[^"]*"|'[^']*')[{s}]*:"#
        ))
        .expect("the YAML key pattern compiles")
    })
}

/// The key's bytes and the end of its colon.
fn yaml_key(line: &str) -> Option<(usize, usize, usize)> {
    let caps = yaml_key_re().captures(line)?;
    let (all, key) = (caps.get(0)?, caps.get(2)?);
    if line[all.end()..]
        .chars()
        .next()
        .is_some_and(|c| !is_js_space(c))
    {
        return None;
    }
    Some((key.start(), key.end(), all.end()))
}

fn is_ws(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

fn is_id_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

fn is_ident(b: u8) -> bool {
    is_id_start(b) || b.is_ascii_digit()
}

/// JavaScript's ASCII `\w`.
fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `[\w:.\-]`, a markup name character.
fn is_name(b: u8) -> bool {
    is_word(b) || matches!(b, b':' | b'.' | b'-')
}

fn push(out: &mut Vec<Token>, start: usize, end: usize, class: TokenClass) {
    out.push(Token { start, end, class });
}

/// `s.indexOf(needle, from)`.
fn find(s: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    memmem::find(s.get(from..)?, needle).map(|at| at + from)
}

fn same_text(a: &[u8], b: &[u8], ignore_case: bool) -> bool {
    if ignore_case {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

fn is_keyword(word: &[u8], def: &Def) -> bool {
    if def.keywords.is_empty() {
        return false;
    }
    let mut lower = [0u8; 32];
    let word = if def.ignore_case {
        // No keyword is this long.
        let Some(buf) = lower.get_mut(..word.len()) else {
            return false;
        };
        buf.copy_from_slice(word);
        buf.make_ascii_lowercase();
        &*buf
    } else {
        word
    };
    def.keywords
        .binary_search_by(|kw| kw.as_bytes().cmp(word))
        .is_ok()
}

/// The end of a string whose quote is before `i` (`ldStrEnd`).
fn string_end(s: &[u8], mut i: usize, quote: u8, def: &Def) -> Option<usize> {
    while i < s.len() {
        let c = s[i];
        if c == b'\\' && !def.doubled {
            i += 2;
            continue;
        }
        if c == quote {
            if def.doubled && s.get(i + 1) == Some(&quote) {
                i += 2;
                continue;
            }
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

/// The end of the JavaScript regex literal at `i` (after its flags), or `None` when the `/`
/// divides (`ldRegexEnd`): judged by what comes before it, and it must close on the line.
fn regex_end(s: &[u8], i: usize) -> Option<usize> {
    let n = s.len();
    if i + 1 >= n || matches!(s[i + 1], b'/' | b'*') {
        return None;
    }
    // One past the last character before the slash that is not a space.
    let mut p = i;
    while p > 0 && is_ws(s[p - 1]) {
        p -= 1;
    }
    if p > 0 {
        let pc = s[p - 1];
        let before = p.checked_sub(2).map(|at| s[at]);
        if is_ident(pc) {
            let mut q = p - 1;
            while q > 0 && is_ident(s[q - 1]) {
                q -= 1;
            }
            if !keywords::RX_KEYWORDS
                .iter()
                .any(|kw| kw.as_bytes() == &s[q..p])
            {
                return None;
            }
        } else if pc == b'>' {
            // Only `=> /re/`, so a JSX `</tag>` is not a regex.
            if before != Some(b'=') {
                return None;
            }
        } else if !keywords::RX_PRE.contains(&pc)
            || (matches!(pc, b'+' | b'-') && before == Some(pc))
        {
            return None;
        }
    }
    let mut j = i + 1;
    let mut class = false;
    while j < n {
        match s[j] {
            b'\\' => {
                j += 2;
                continue;
            }
            b'[' => class = true,
            b']' => class = false,
            b'/' if !class => break,
            _ => {}
        }
        j += 1;
    }
    // A slash closing right before a comment is not a regex either.
    if j >= n || matches!(s.get(j + 1), Some(b'/' | b'*')) {
        return None;
    }
    j += 1;
    while j < n && is_id_start(s[j]) {
        j += 1;
    }
    Some(j)
}

/// Lexes one line and appends its tokens to `out`, in order and never overlapping (`ldLex`).
pub fn lex_line(line: &str, lang: Language, state: &mut LexState, out: &mut Vec<Token>) {
    let def = lang.def();
    let s = line.as_bytes();
    let n = s.len();
    let mut i = 0;
    if let Some(&(_, close)) = state.block.and_then(|k| def.block.get(k as usize)) {
        let Some(j) = find(s, close.as_bytes(), 0) else {
            if n > 0 {
                push(out, 0, n, TokenClass::Comment);
            }
            return;
        };
        i = j + close.len();
        push(out, 0, i, TokenClass::Comment);
        state.block = None;
    } else if let Some(quote) = state.quote {
        let end = match quote {
            Quote::Three(q) => find(s, &[q; 3], 0).map(|j| j + 3),
            Quote::One(q) => string_end(s, 0, q, def),
        };
        let Some(j) = end else {
            if n > 0 {
                push(out, 0, n, TokenClass::String);
            }
            return;
        };
        push(out, 0, j, TokenClass::String);
        i = j;
        state.quote = None;
    }
    if def.diff {
        let class = if [&b"+++"[..], b"---", b"diff ", b"index "]
            .iter()
            .any(|head| s.starts_with(head))
        {
            Some(TokenClass::Keyword)
        } else {
            match s.first() {
                Some(b'@') => Some(TokenClass::Attr),
                Some(b'+') => Some(TokenClass::Key),
                Some(b'-') => Some(TokenClass::Error),
                _ => None,
            }
        };
        if let Some(class) = class {
            push(out, 0, n, class);
        }
        return;
    }
    if def.log {
        for caps in log_re().captures_iter(line) {
            let Some(all) = caps.get(0) else { continue };
            let class = match (1..=5).find(|&group| caps.get(group).is_some()) {
                Some(1) => TokenClass::DateTime,
                Some(2) => TokenClass::Error,
                Some(3) => TokenClass::Warn,
                Some(4) => TokenClass::Info,
                Some(_) => TokenClass::Muted,
                None => TokenClass::String,
            };
            push(out, all.start(), all.end(), class);
        }
        return;
    }
    if def.markup {
        lex_markup(s, i, state, out);
        return;
    }
    let mut first = i;
    while first < n && is_ws(s[first]) {
        first += 1;
    }
    if first < n {
        let rest = &s[first..];
        for start in def.line_start {
            let start = start.as_bytes();
            // "rem " also matches a line that is just "rem".
            let bare = start.last() == Some(&b' ')
                && same_text(rest, &start[..start.len() - 1], def.ignore_case);
            if bare
                || rest
                    .get(..start.len())
                    .is_some_and(|head| same_text(head, start, def.ignore_case))
            {
                push(out, first, n, TokenClass::Comment);
                return;
            }
        }
    }
    if def.ini_section && s.get(first) == Some(&b'[') {
        push(out, first, n, TokenClass::Keyword);
        return;
    }
    if def.prop_key && i == 0 && first < n {
        let mut j = first;
        while j < n {
            match s[j] {
                b'\\' => j += 2,
                b'=' | b':' | b' ' | b'\t' => break,
                _ => j += 1,
            }
        }
        if j > first {
            push(out, first, j.min(n), TokenClass::Key);
        }
        return;
    }
    if def.yaml_key
        && i == 0
        && let Some((start, end, after)) = yaml_key(line)
    {
        push(out, start, end, TokenClass::Key);
        i = after;
    }
    if def.preprocessor && s.get(first) == Some(&b'#') {
        let mut j = first + 1;
        while j < n && is_ident(s[j]) {
            j += 1;
        }
        push(out, first, j, TokenClass::Keyword);
        i = j;
    }
    'outer: while i < n {
        let c = s[i];
        if def.regex
            && c == b'/'
            && let Some(j) = regex_end(s, i)
        {
            push(out, i, j, TokenClass::String);
            i = j;
            continue;
        }
        for (k, &(open, close)) in def.block.iter().enumerate() {
            if s[i..].starts_with(open.as_bytes()) {
                let Some(j) = find(s, close.as_bytes(), i + open.len()) else {
                    push(out, i, n, TokenClass::Comment);
                    state.block = Some(k as u8);
                    return;
                };
                push(out, i, j + close.len(), TokenClass::Comment);
                i = j + close.len();
                continue 'outer;
            }
        }
        for start in def.line {
            if s[i..].starts_with(start.as_bytes()) && (!def.hash_ws || i == 0 || is_ws(s[i - 1])) {
                push(out, i, n, TokenClass::Comment);
                return;
            }
        }
        if def.quotes.contains(&c) {
            for triple in def.triple {
                let triple = triple.as_bytes();
                if s[i..].starts_with(triple) {
                    let Some(j) = find(s, triple, i + 3) else {
                        push(out, i, n, TokenClass::String);
                        state.quote = Some(Quote::Three(triple[0]));
                        return;
                    };
                    push(out, i, j + 3, TokenClass::String);
                    i = j + 3;
                    continue 'outer;
                }
            }
            let Some(j) = string_end(s, i + 1, c, def) else {
                push(out, i, n, TokenClass::String);
                if def.multi_line.contains(&c) {
                    state.quote = Some(Quote::One(c));
                }
                return;
            };
            let mut class = TokenClass::String;
            if def.json_key {
                let mut t = j;
                while t < n && is_ws(s[t]) {
                    t += 1;
                }
                if s.get(t) == Some(&b':') {
                    class = TokenClass::Key;
                }
            }
            push(out, i, j, class);
            i = j;
            continue;
        }
        if c.is_ascii_digit() && (i == 0 || !is_ident(s[i - 1])) {
            let mut j = i + 1;
            while j < n && (is_ident(s[j]) || s[j] == b'.') {
                j += 1;
            }
            push(out, i, j, TokenClass::Number);
            i = j;
            continue;
        }
        if is_id_start(c) {
            let mut j = i + 1;
            while j < n && is_ident(s[j]) {
                j += 1;
            }
            if is_keyword(&s[i..j], def) {
                push(out, i, j, TokenClass::Keyword);
            }
            i = j;
            continue;
        }
        i += 1;
    }
}

/// Markup from byte `i` on (`ldLexMarkup`): tags, attributes, quoted values, comments, CDATA
/// and entities.
fn lex_markup(s: &[u8], mut i: usize, state: &mut LexState, out: &mut Vec<Token>) {
    let n = s.len();
    while i < n {
        let c = s[i];
        if state.tag {
            if c == b'>' || (matches!(c, b'/' | b'?') && s.get(i + 1) == Some(&b'>')) {
                let end = if c == b'>' { i + 1 } else { i + 2 };
                push(out, i, end, TokenClass::Tag);
                i = end;
                state.tag = false;
            } else if c == b'"' || c == b'\'' {
                let Some(j) = memchr::memchr(c, &s[i + 1..]).map(|at| i + 1 + at) else {
                    push(out, i, n, TokenClass::String);
                    return;
                };
                push(out, i, j + 1, TokenClass::String);
                i = j + 1;
            } else if is_name(c) {
                let mut j = i + 1;
                while j < n && is_name(s[j]) {
                    j += 1;
                }
                push(out, i, j, TokenClass::Attr);
                i = j;
            } else {
                i += 1;
            }
            continue;
        }
        if c == b'<' {
            if s[i..].starts_with(b"<!--") {
                let Some(j) = find(s, b"-->", i + 4) else {
                    push(out, i, n, TokenClass::Comment);
                    state.block = Some(0);
                    return;
                };
                push(out, i, j + 3, TokenClass::Comment);
                i = j + 3;
                continue;
            }
            if s[i..].starts_with(b"<![CDATA[") {
                let Some(j) = find(s, b"]]>", i + 9) else {
                    push(out, i, n, TokenClass::String);
                    return;
                };
                push(out, i, j + 3, TokenClass::String);
                i = j + 3;
                continue;
            }
            // `<[\/?!]?[\w:.\-]*` within 200 characters, longer than the `<` alone.
            let limit = n.min(i + 200);
            let mut j = i + 1;
            if j < limit && matches!(s[j], b'/' | b'?' | b'!') {
                j += 1;
            }
            while j < limit && is_name(s[j]) {
                j += 1;
            }
            if j > i + 1 {
                push(out, i, j, TokenClass::Tag);
                i = j;
                state.tag = true;
                continue;
            }
        }
        if c == b'&' {
            // `&#?\w+;` within 12 characters.
            let limit = n.min(i + 12);
            let mut j = i + 1;
            if j < limit && s[j] == b'#' {
                j += 1;
            }
            let word = j;
            while j < limit && is_word(s[j]) {
                j += 1;
            }
            if j > word && j < limit && s[j] == b';' {
                push(out, i, j + 1, TokenClass::Number);
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
}

/// The line with its comments cut (`ldStripComments`): a comment at the end goes with the
/// spaces before it, one in the middle leaves one space, one at the start keeps the indent.
/// `buf` is scratch space for the tokens. Borrowed when the line has no comment.
pub fn strip_comments<'a>(
    line: &'a str,
    lang: Language,
    state: &mut LexState,
    buf: &mut Vec<Token>,
) -> Cow<'a, str> {
    buf.clear();
    lex_line(line, lang, state, buf);
    let mut comments = buf.iter().filter(|t| t.class == TokenClass::Comment);
    let Some(first) = comments.next() else {
        return Cow::Borrowed(line);
    };
    let mut out = Cow::Borrowed(&line[..first.start]);
    let mut pos = first.end;
    for comment in comments {
        let joined = join_around_comment(&out, &line[pos..comment.start]);
        out = Cow::Owned(joined);
        pos = comment.end;
    }
    let rest = &line[pos..];
    if rest.bytes().all(is_ws) {
        trim_ws_end(out)
    } else {
        Cow::Owned(join_around_comment(&out, rest))
    }
}

fn is_ws_char(c: char) -> bool {
    c == ' ' || c == '\t'
}

fn trim_ws_end(text: Cow<'_, str>) -> Cow<'_, str> {
    match text {
        Cow::Borrowed(text) => Cow::Borrowed(text.trim_end_matches(is_ws_char)),
        Cow::Owned(mut text) => {
            let keep = text.trim_end_matches(is_ws_char).len();
            text.truncate(keep);
            Cow::Owned(text)
        }
    }
}

/// The text on both sides of a cut comment (`ldJoinAroundComment`).
fn join_around_comment(left: &str, right: &str) -> String {
    let right_text = right.trim_start_matches(is_ws_char);
    let left_text = left.trim_end_matches(is_ws_char);
    if left_text.is_empty() {
        // At the start of the text: keep the indent, drop the spaces after the comment.
        return [left, right_text].concat();
    }
    if left_text.len() == left.len() && right_text.len() == right.len() {
        return [left, right].concat();
    }
    [left_text, " ", right_text].concat()
}

#[cfg(test)]
mod tests;
