use super::*;
use TokenClass as T;

fn strip1(lang: Language, line: &str) -> String {
    strip_comments(line, lang, &mut LexState::default(), &mut Vec::new()).into_owned()
}

fn lex1(lang: Language, line: &str) -> Vec<(usize, usize, TokenClass)> {
    let mut out = Vec::new();
    lex_line(line, lang, &mut LexState::default(), &mut out);
    out.iter().map(|t| (t.start, t.end, t.class)).collect()
}

#[test]
fn languages_are_detected_from_the_name() {
    use Language::*;
    for (name, want) in [
        ("a.ts", Js),
        ("x.kt", Java),
        ("Dockerfile", Shell),
        ("Makefile", Shell),
        ("app.log.1", Log),
        ("x.log.2024-01-02", Log),
        ("a.toml", Ini),
        ("a.cfg", Ini),
        ("a.md", Text),
        ("noext", Text),
        ("A.PS1", PowerShell),
        ("a.csproj", Markup),
        ("a.patch", Diff),
        ("a.properties", Properties),
        ("a.rs", CLike),
        ("a.jsonc", Js),
        ("a.json", Json),
        (r"C:\d\b.SQL", Sql),
        // Not in the brief: the edges of the rules.
        ("", Text),
        ("/srv/x.log.2024-01-02.gz", Log),
        ("x.log.gz", Text),
        ("x.log.", Text),
        (".env", Shell),
        ("a.unknown", Text),
    ] {
        assert_eq!(detect_language(name), want, "{name}");
    }
    for lang in Language::ALL {
        assert_eq!(Language::from_id(lang.id()), Some(lang));
    }
    assert_eq!(Language::from_id("C-like"), None);
    assert_eq!(Language::Text.label(), "Plain text");
}

#[test]
fn strip_comments_ports_localdiff() {
    use Language::*;
    for (lang, line, want) in [
        (CLike, "let a = 1; // one", "let a = 1;"),
        (CLike, "int x = /* c */ 5;", "int x = 5;"),
        (CLike, "  /* c */ x", "  x"),
        (Python, "x = 1  # c", "x = 1"),
        (Shell, "echo a#b", "echo a#b"),
        (Shell, "echo a # b", "echo a"),
        (Sql, "select 'it''s' -- c", "select 'it''s'"),
        (Batch, "REM hello", ""),
        (Batch, ":: x", ""),
        (Ini, "; c", ""),
        (Ini, "k=v ; c", "k=v ; c"),
        (Markup, "<a><!-- c --></a>", "<a></a>"),
        (Lua, "x = 1 -- c", "x = 1"),
        (Vb, "x = 1 ' c", "x = 1"),
        (PowerShell, "<# b #> $a", "$a"),
    ] {
        assert_eq!(strip1(lang, line), want, "{lang:?} {line:?}");
    }
    let mut state = LexState::default();
    let mut buf = Vec::new();
    let got: Vec<String> = ["a /* start", "middle", "end */ b"]
        .iter()
        .map(|line| strip_comments(line, CLike, &mut state, &mut buf).into_owned())
        .collect();
    assert_eq!(got, ["a", "", "b"]);
    assert_eq!(state, LexState::default(), "the comment closed");
    assert!(matches!(
        strip_comments("x = 1", CLike, &mut LexState::default(), &mut buf),
        Cow::Borrowed("x = 1")
    ));
}

#[test]
fn strings_and_regex_literals_hide_comment_markers() {
    use Language::*;
    for (lang, line, want) in [
        (CLike, r#"s = "http://x"; // c"#, r#"s = "http://x";"#),
        (Js, r"x = /\/\//; // c", r"x = /\/\//;"),
        (Js, "a = b / c; // d", "a = b / c;"),
        (Python, r##"s = "# not""##, r##"s = "# not""##),
        (Yaml, "a: b#c", "a: b#c"),
        (Yaml, "a: b # c", "a: b"),
    ] {
        assert_eq!(strip1(lang, line), want, "{lang:?} {line:?}");
    }
}

#[test]
fn lex_line_classifies_tokens() {
    use Language::*;
    assert_eq!(
        lex1(CLike, "let x = 42; // hi"),
        [(0, 3, T::Keyword), (8, 10, T::Number), (12, 17, T::Comment)]
    );
    assert_eq!(
        lex1(Json, r#"{"a": "b"}"#),
        [(1, 4, T::Key), (6, 9, T::String)]
    );
    assert_eq!(
        lex1(Log, "2024-01-02 10:11:12 ERROR boom"),
        [(0, 19, T::DateTime), (20, 25, T::Error)]
    );
    assert_eq!(lex1(Diff, "+added"), [(0, 6, T::Key)]);
    assert_eq!(lex1(Diff, "@@ -1 +1 @@"), [(0, 11, T::Attr)]);
    assert_eq!(lex1(Ini, "[sec]"), [(0, 5, T::Keyword)]);
    assert_eq!(lex1(Ini, "key=1"), [(0, 3, T::Key)]);
    // Thai text in a string: offsets are bytes, on character boundaries.
    let line = "s = \"ไทย\"; // ก";
    let tokens = lex1(CLike, line);
    assert_eq!(tokens, [(4, 15, T::String), (17, line.len(), T::Comment)]);
    assert_eq!(&line[4..15], "\"ไทย\"");
    // A fresh state is outside any comment (LocalDiff's `bc: -1`).
    assert!(lex1(CLike, "a */ b").is_empty());
}

fn strip_lines(lang: Language, lines: &[&str]) -> Vec<String> {
    let (mut state, mut buf) = (LexState::default(), Vec::new());
    lines
        .iter()
        .map(|line| strip_comments(line, lang, &mut state, &mut buf).into_owned())
        .collect()
}

fn lex_lines(lang: Language, lines: &[&str]) -> Vec<Vec<(usize, usize, TokenClass)>> {
    let mut state = LexState::default();
    lines
        .iter()
        .map(|line| {
            let mut out = Vec::new();
            lex_line(line, lang, &mut state, &mut out);
            out.iter().map(|t| (t.start, t.end, t.class)).collect()
        })
        .collect()
}

/// Expected values from LocalDiff's own `ldStripComments` and `ldLex`.
#[test]
fn state_and_edge_cases_match_localdiff() {
    use Language::*;
    let cases: [(Language, &[&str], &[&str]); 16] = [
        (
            Python,
            &[r#"x = """a"#, "b # no", r#"c""" # yes"#],
            &[r#"x = """a"#, "b # no", r#"c""""#],
        ),
        (
            Js,
            &["s = `a", "// in", "b` // out"],
            &["s = `a", "// in", "b`"],
        ),
        (Go, &["s = `a", "b` // out"], &["s = `a", "b`"]),
        (
            Java,
            &[r#"s = """"#, "// in", r#""""; // out"#],
            &[r#"s = """"#, "// in", r#"""";"#],
        ),
        (
            Markup,
            &[r#"<a href="x""#, "  b=1><!-- c", "d --> e"],
            &[r#"<a href="x""#, "  b=1>", "e"],
        ),
        (
            Batch,
            &["rem", "@REM x", "  Rem", "remark", "echo x"],
            &["", "", "", "remark", "echo x"],
        ),
        (Sql, &[r"select 'a\' -- c"], &[r"select 'a\'"]),
        (Js, &["y = i++ / 2; // c"], &["y = i++ / 2;"]),
        (Js, &["return /x/.test(s) // c"], &["return /x/.test(s)"]),
        (Js, &["f = () => /re/g; // c"], &["f = () => /re/g;"]),
        (Js, &["x = a</b>/ 2; // c"], &["x = a</b>/ 2;"]),
        (
            Yaml,
            &[r#""k:x": v # c"#, "- k: v # c", "k:v # c"],
            &[r#""k:x": v"#, "- k: v", "k:v"],
        ),
        (
            Properties,
            &[r"a\ b=c # not", "! note"],
            &[r"a\ b=c # not", ""],
        ),
        (PowerShell, &["$a <# one", "two #> $b # c"], &["$a", "$b"]),
        (
            CLike,
            &["#include <a> // c", "a /* x */ b /* y */ c"],
            &["#include <a>", "a b c"],
        ),
        (Css, &[r#"a { b: "/*" } /* c */"#], &[r#"a { b: "/*" }"#]),
    ];
    for (lang, lines, want) in cases {
        assert_eq!(strip_lines(lang, lines), want, "{lang:?} {lines:?}");
    }
    assert_eq!(
        lex_lines(Python, &[r#"x = """a"#, "b # no", r#"c""" # yes"#]),
        [
            vec![(4, 8, T::String)],
            vec![(0, 6, T::String)],
            vec![(0, 4, T::String), (5, 10, T::Comment)],
        ]
    );
    assert_eq!(
        lex_lines(Markup, &[r#"<a href="x""#, "  b=1><!-- c", "d --> e"]),
        [
            vec![(0, 2, T::Tag), (3, 7, T::Attr), (8, 11, T::String)],
            vec![
                (2, 3, T::Attr),
                (4, 5, T::Attr),
                (5, 6, T::Tag),
                (6, 12, T::Comment)
            ],
            vec![(0, 5, T::Comment)],
        ]
    );
    assert_eq!(
        lex_lines(Js, &["return /x/.test(s) // c"]),
        [vec![
            (0, 6, T::Keyword),
            (7, 10, T::String),
            (19, 23, T::Comment)
        ]]
    );
    assert_eq!(
        lex_lines(Yaml, &[r#""k:x": v # c"#, "- k: v # c", "k:v # c"]),
        [
            vec![(0, 5, T::Key), (9, 12, T::Comment)],
            vec![(2, 3, T::Key), (7, 10, T::Comment)],
            vec![(4, 7, T::Comment)],
        ]
    );
    assert_eq!(
        lex_lines(Sql, &[r"select 'a\' -- c"]),
        [vec![
            (0, 6, T::Keyword),
            (7, 11, T::String),
            (12, 16, T::Comment)
        ]]
    );
    assert_eq!(
        lex_lines(CLike, &["#include <a> // c"]),
        [vec![(0, 8, T::Keyword), (13, 17, T::Comment)]]
    );
}

#[test]
fn keyword_lists_are_sorted_and_short() {
    let lists: [(&[&str], bool); 11] = [
        (keywords::CLIKE, false),
        (keywords::PYTHON, false),
        (keywords::SHELL, false),
        (keywords::POWERSHELL, true),
        (keywords::BATCH, true),
        (keywords::SQL, true),
        (keywords::JSON, false),
        (keywords::YAML, false),
        (keywords::VB, true),
        (keywords::LUA, false),
        (keywords::RX_KEYWORDS, true),
    ];
    for (list, lower) in lists {
        assert!(
            list.windows(2).all(|w| w[0].as_bytes() < w[1].as_bytes()),
            "{list:?}"
        );
        assert!(
            list.iter()
                .all(|w| w.len() <= 32 && w.bytes().all(is_ident))
        );
        if lower {
            assert!(
                list.iter()
                    .all(|w| !w.bytes().any(|b| b.is_ascii_uppercase()))
            );
        }
    }
    assert_eq!(lex1(Language::Sql, "SeLeCt x"), [(0, 6, T::Keyword)]);
    assert_eq!(lex1(Language::Python, "none None"), [(5, 9, T::Keyword)]);
}

#[test]
fn the_yaml_space_class_is_javascripts() {
    let re = regex::Regex::new(&format!("^[{JS_SPACE}]$")).unwrap();
    for c in (0..0x11000u32).filter_map(char::from_u32) {
        assert_eq!(
            re.is_match(c.encode_utf8(&mut [0; 4])),
            is_js_space(c),
            "{c:?}"
        );
    }
}

const PIECES: [&str; 40] = [
    "a",
    "let",
    "42",
    "3.5",
    " ",
    "\t",
    "//",
    "/",
    "/*",
    "*/",
    "#",
    "--",
    "'",
    "\"",
    "`",
    "\"\"\"",
    "'''",
    "\\",
    "<",
    "<!--",
    "-->",
    ">",
    "/>",
    "<a",
    "&amp;",
    "[",
    "]",
    "=",
    ":",
    ";",
    "rem ",
    "::",
    "ก",
    "\u{1f600}",
    "\u{a0}",
    "+++",
    "@",
    "ERROR",
    "10:11:12",
    "k: ",
];

fn text_strategy() -> impl proptest::strategy::Strategy<Value = Vec<String>> {
    let line = proptest::collection::vec(proptest::sample::select(PIECES.to_vec()), 0..10)
        .prop_map(|pieces| pieces.concat());
    proptest::collection::vec(line, 1..6)
}

use proptest::strategy::Strategy as _;

proptest::proptest! {
    #[test]
    fn tokens_are_ordered_and_on_character_boundaries(lines in text_strategy()) {
        for lang in Language::ALL {
            let (mut state, mut strip_state) = (LexState::default(), LexState::default());
            let mut buf = Vec::new();
            for line in &lines {
                let mut out = Vec::new();
                lex_line(line, lang, &mut state, &mut out);
                let mut prev = 0;
                for t in &out {
                    proptest::prop_assert!(
                        prev <= t.start && t.start < t.end && t.end <= line.len()
                    );
                    proptest::prop_assert!(
                        line.is_char_boundary(t.start) && line.is_char_boundary(t.end)
                    );
                    prev = t.end;
                }
                // A comment always shortens the line, so "cut" and "had a comment" agree.
                let cut = strip_comments(line, lang, &mut strip_state, &mut buf);
                let has_comment = out.iter().any(|t| t.class == T::Comment);
                proptest::prop_assert_eq!(
                    cut.len() < line.len(),
                    has_comment,
                    "{:?} {:?}",
                    lang,
                    line
                );
                proptest::prop_assert!(lang.has_comments() || !has_comment);
            }
        }
    }
}
