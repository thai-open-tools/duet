//! Checks over the sources and the Thai table (D3): every key has an entry, no entry is unused,
//! every value is ready to draw, and no literal a reader sees skips `tr`.
//!
//! Every file of the app is scanned (the ratchet is done); only `EXEMPT` files may hold English that
//! a reader never sees. Each check is a function over its input, with a self-test on fake input.

#[path = "../../duet-core/tests/support/rust_scan.rs"]
mod rust_scan;

use std::collections::BTreeSet;
use std::path::Path;

use duet_app::help::markup::{self, Span};
use duet_app::help::topics::{self, HelpLang, MAX_PIECE_CHARS, TOPICS, is_thai_mark};
use duet_app::i18n::th;
use rust_scan::{Lit, scan};

/// Core files whose text reaches the UI, under `crates/duet-core/src`, in the same scan.
const CORE_UI_FILES: &[&str] = &[
    "kind.rs",
    "text/merge/session.rs",
    "text/merge/output.rs",
    "table/nav.rs",
    "table/mod.rs",
    "table/xlsx.rs",
    "folder/sync.rs",
    "folder/classify.rs",
    "folder/plan.rs",
    "folder/tree.rs",
    "hex/find.rs",
    "hex/format.rs",
    "picture/mod.rs",
    "text/rules.rs",
    "text/sort_keys/mod.rs",
];

/// Files whose literals are never a label, each with its reason (rule 3).
const EXEMPT: &[(&str, &str)] = &[
    ("crash.rs", "crash reports stay in English"),
    ("main.rs", "the log and the exit code only"),
    ("text/syntax.rs", "grammar names and tree-sitter queries"),
    ("ui/fonts.rs", "font family names"),
    ("settings.rs", "the settings file's own words"),
    ("app.rs", "start-up: the app's name, environment variables"),
    ("paths.rs", "file system prefixes"),
    ("palette.rs", "colours"),
    ("probe.rs", "file signatures"),
    ("help/", "the help holds both languages itself"),
    (
        "shell_verbs.rs",
        "Explorer's menu verbs (in both languages) and registry paths, written once for Windows",
    ),
    ("i18n/", "the mechanism and its tables"),
];

/// One literal in one file, that is never shown or is a name, and why (rule 4). An entry that
/// matches nothing fails the test, so the list cannot rot.
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "launch.rs",
        "Usage: duet [--table] [LEFT RIGHT] | duet LEFT RIGHT BASE [OUTPUT]",
        "the usage line stays English (plan A)",
    ),
    (
        "launch.rs",
        "Usage: duet --select-left PATH | duet --compare-to PATH | duet --register-shell | duet --unregister-shell",
        "the usage line stays English (plan A)",
    ),
    (
        "launch.rs",
        "/dev/null",
        "the name git passes for a missing side",
    ),
    (
        "launch.rs",
        "no file",
        "the name of the empty file, which the tab shows",
    ),
    (
        "shell.rs",
        "the path is not valid Unicode",
        "an error that is only logged",
    ),
    ("theme.rs", "duet dark", "a syntax theme's name"),
    ("theme.rs", "duet light", "a syntax theme's name"),
    ("workspace.rs", "Workspace", "a key context's name"),
    (
        "help.rs",
        "ไทย",
        "the Help switch's label: each language names itself",
    ),
    (
        "help.rs",
        "English",
        "the Help switch's label: each language names itself",
    ),
    ("text/prefs.rs", "UTF-8 BOM", "an encoding's name"),
    (
        "core/text/merge/output.rs",
        "<<<<<<< left",
        "a conflict marker, written into the merged file as git writes it",
    ),
    (
        "core/text/merge/output.rs",
        "||||||| base",
        "a conflict marker, written into the merged file as git writes it",
    ),
    (
        "core/text/merge/output.rs",
        ">>>>>>> right",
        "a conflict marker, written into the merged file as git writes it",
    ),
    (
        "core/table/nav.rs",
        "left",
        "the key settings.json keeps the filter under",
    ),
    (
        "core/table/nav.rs",
        "right",
        "the key settings.json keeps the filter under",
    ),
    (
        "core/table/xlsx.rs",
        "sheet:type",
        "the reader's name for an error, matched and never drawn",
    ),
    (
        "core/table/xlsx.rs",
        "#DIV/0!",
        "Excel's own error text, stored in English in every file",
    ),
    (
        "core/table/xlsx.rs",
        "#NAME?",
        "Excel's own error text, stored in English in every file",
    ),
    (
        "core/table/xlsx.rs",
        "#NULL!",
        "Excel's own error text, stored in English in every file",
    ),
    (
        "core/table/xlsx.rs",
        "#NUM!",
        "Excel's own error text, stored in English in every file",
    ),
    (
        "core/table/xlsx.rs",
        "#REF!",
        "Excel's own error text, stored in English in every file",
    ),
    (
        "core/table/xlsx.rs",
        "#VALUE!",
        "Excel's own error text, stored in English in every file",
    ),
    (
        "core/table/xlsx.rs",
        "#DATA!",
        "Excel's own error text, stored in English in every file",
    ),
    (
        "core/picture/mod.rs",
        "Picture {}×{}",
        "Debug output, never drawn",
    ),
    (
        "core/folder/classify.rs",
        "Src",
        "a name in a test helper the scanner reads as code",
    ),
    (
        "core/folder/classify.rs",
        "Src/a.java",
        "a name in a test helper the scanner reads as code",
    ),
    (
        "core/folder/classify.rs",
        "src/a.java",
        "a name in a test helper the scanner reads as code",
    ),
    (
        "merge/view.rs",
        "MergeText > Input",
        "a key context path, never shown",
    ),
    (
        "workspace.rs",
        "ไทย",
        "the header control's label: each language names itself",
    ),
    ("format.rs", "KiB", "a unit's name"),
    ("format.rs", "MiB", "a unit's name"),
    ("format.rs", "GiB", "a unit's name"),
    ("format.rs", "TiB", "a unit's name"),
    (
        "folder/model.rs",
        "left",
        "the key settings.json keeps the filter under",
    ),
    (
        "folder/model.rs",
        "right",
        "the key settings.json keeps the filter under",
    ),
    ("text/view.rs", "left", "the Left arrow key's name"),
    ("text/view.rs", "right", "the Right arrow key's name"),
    (
        "text/view/rules_bar.rs",
        "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
        "a regular expression, which the preset adds as it is",
    ),
    (
        "text/view/rules_bar.rs",
        "^#.*generated",
        "a regular expression in an example the field shows as it is",
    ),
    (
        "text/find.rs",
        "0-9A-Za-z_\\x{0E00}-\\x{0E7F}",
        "a regular expression class, never drawn",
    ),
    (
        "text/report.rs",
        "left",
        "the patch's file name when a side has none; the patch stays English",
    ),
    (
        "text/report.rs",
        "right",
        "the patch's file name when a side has none; the patch stays English",
    ),
    (
        "ui/menu.rs",
        "ไทย",
        "a language's own name, the same in both languages",
    ),
    (
        "ui/menu.rs",
        "English",
        "a language's own name, the same in both languages",
    ),
    ("folder/view.rs", "left", "the Left arrow key's name"),
    ("folder/view.rs", "right", "the Right arrow key's name"),
    ("table/view.rs", "left", "the Left arrow key's name"),
    ("table/view.rs", "right", "the Right arrow key's name"),
    ("hex/view.rs", "left", "the Left arrow key's name"),
    ("hex/view.rs", "right", "the Right arrow key's name"),
    ("picture/view.rs", "ctrl-=", "a key binding"),
    ("picture/view.rs", "ctrl-+", "a key binding"),
    (
        "folder/ops.rs",
        "line endings or BOM",
        "a reason code that `detail` matches, never drawn",
    ),
    (
        "folder/export_panel.rs",
        "ไทย",
        "a language's own name, the same in both languages",
    ),
    (
        "folder/export_panel.rs",
        "English",
        "a language's own name, the same in both languages",
    ),
    (
        "folder/export_panel.rs",
        "%Y-%m-%dT%H:%M:%SZ",
        "a time format for the report's machine-readable stamp",
    ),
    (
        "folder/sync_panel.rs",
        "old/a.txt",
        "a path in a test helper",
    ),
    (
        "folder/sync_panel.rs",
        "old/b.txt",
        "a path in a test helper",
    ),
];

/// Calls that mark a literal as a key, or that make one of its arguments one.
const KEY_CALLS: &[&str] = &["tr", "trf", "tr_key", "live", "count_of", "trc"];

/// Calls whose literals never reach a reader (rule 1).
const NON_TEXT_CALLS: &[&str] = &[
    "info!",
    "warn!",
    "error!",
    "debug!",
    "trace!",
    "expect",
    "panic!",
    "unreachable!",
    "assert!",
    "assert_eq!",
    "assert_ne!",
    "debug_assert!",
    "debug_assert_eq!",
    "id",
    "key_context",
    "include_bytes!",
    "env!",
    "var",
    "var_os",
    "json!",
    "get_string",
    "get_bool",
    "get_u64",
    "get_f64",
    "get_json",
    "set_string",
    "set_bool",
    "set_u64",
    "set_f64",
    "set_json",
    "Regex",
    "with_extension",
    "starts_with",
    "ends_with",
    "strip_prefix",
    "strip_suffix",
    "contains",
    "split",
    "split_once",
    "replace",
];

/// Lower-case words that are text, which the id rule would otherwise let through (rule 2).
const TEXT_WORDS: &[&str] = &["left", "right", "untitled", "invisible", "ok"];

/// A call's last path segment: `crate::i18n::tr` is `tr`.
fn last_segment(call: &str) -> &str {
    call.rsplit("::").next().unwrap_or(call)
}

/// The keys of `lits` (D3): a literal whose innermost call is `tr`, `tr_key` or `live`, the
/// template of `trf`, the `many` of `count_of`, or `context|key` from the first two of `trc`.
fn keys_of(lits: &[Lit]) -> Vec<String> {
    let mut keys = Vec::new();
    let mut context: Option<&str> = None;
    for lit in lits {
        let Some(call) = lit.calls.first() else {
            continue;
        };
        match (last_segment(call), lit.arg) {
            ("tr" | "tr_key" | "live", _) | ("trf", 0) | ("count_of", 2) => {
                keys.push(lit.text.clone());
            }
            ("trc", 0) => context = Some(&lit.text),
            ("trc", 1) => {
                if let Some(context) = context.take() {
                    keys.push(format!("{context}|{}", lit.text));
                }
            }
            _ => {}
        }
    }
    keys
}

/// Rule 2: nothing a reader takes for a word: no run of two letters once its holes are gone, an id
/// or a settings key, an action's name, one CamelCase token, an all-caps name, or a file extension.
fn shape_cannot_be_text(text: &str) -> bool {
    let text = without_holes(text);
    let text = text.trim();
    let chars: Vec<char> = text.chars().collect();
    if !chars
        .windows(2)
        .any(|pair| pair[0].is_alphabetic() && pair[1].is_alphabetic())
    {
        return true;
    }
    if TEXT_WORDS.contains(&text) {
        return false;
    }
    let id_char =
        |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.');
    let word_char = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let action = text.split_once("::").is_some_and(|(a, b)| {
        [a, b]
            .iter()
            .all(|w| !w.is_empty() && w.chars().all(word_char))
    });
    let all_caps = chars
        .iter()
        .all(|&c| c.is_ascii_uppercase() || c.is_ascii_digit() || "-+_./():,".contains(c));
    let extension = text
        .strip_prefix('.')
        .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_alphanumeric()));
    chars.iter().all(|&c| id_char(c)) || action || is_camel_case(text) || all_caps || extension
}

/// Two or more capitalised words with no space: `HexView`, not `KiB` or `Hello`.
fn is_camel_case(text: &str) -> bool {
    let mut words = 0;
    let mut chars = text.chars().peekable();
    while let Some(first) = chars.next() {
        if !first.is_ascii_uppercase() {
            return false;
        }
        let mut lower = 0;
        while chars
            .next_if(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            .is_some()
        {
            lower += 1;
        }
        if lower == 0 {
            return false;
        }
        words += 1;
    }
    words >= 2
}

/// `text` without its `{…}` holes.
fn without_holes(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        out.push_str(&rest[..open]);
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

/// Whether `path` is the file `entry` names or lies in the folder it names.
fn covers(entry: &str, path: &str) -> bool {
    path == entry || (entry.ends_with('/') && path.starts_with(entry))
}

/// Rule 1: its call chain holds a key call, `paired`, or a call that is never text.
fn call_excuses(lit: &Lit) -> bool {
    lit.calls.iter().any(|call| {
        let last = last_segment(call);
        KEY_CALLS.contains(&last)
            || last == "paired"
            || call
                .split("::")
                .any(|segment| NON_TEXT_CALLS.contains(&segment))
    })
}

/// The literals of `file` a reader would see in English whatever the language, as "file:line
/// text", and the `allowed` entries that excused one.
fn flagged_in(
    file: &str,
    code: &str,
    allowed: &[(&'static str, &'static str, &'static str)],
) -> (Vec<String>, Vec<(&'static str, &'static str)>) {
    let mut flagged = Vec::new();
    let mut used = Vec::new();
    if EXEMPT.iter().any(|(entry, _)| covers(entry, file)) {
        return (flagged, used);
    }
    for lit in scan(code) {
        if call_excuses(&lit) || shape_cannot_be_text(&lit.text) {
            continue;
        }
        match allowed
            .iter()
            .find(|(entry, literal, _)| covers(entry, file) && *literal == lit.text)
        {
            Some((entry, literal, _)) => used.push((*entry, *literal)),
            None => flagged.push(format!("{file}:{} {:?}", lit.line, lit.text)),
        }
    }
    (flagged, used)
}

/// Each `(path, code)` under `dir`, its path relative to `dir` with forward slashes. A file of
/// tests is left out.
fn rust_files(dir: &Path) -> Vec<(String, String)> {
    fn visit(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, out);
            } else if path.extension().is_some_and(|e| e == "rs")
                && path.file_name().is_some_and(|name| name != "tests.rs")
            {
                let relative = path.strip_prefix(root).unwrap().to_string_lossy();
                out.push((
                    relative.replace('\\', "/"),
                    std::fs::read_to_string(&path).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    visit(dir, dir, &mut out);
    out.sort();
    out
}

fn app_sources() -> Vec<(String, String)> {
    rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
}

/// Every file of the app, and `CORE_UI_FILES`.
fn converted_sources() -> Vec<(String, String)> {
    let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("../duet-core/src");
    let mut out = app_sources();
    out.extend(CORE_UI_FILES.iter().map(|file| {
        (
            format!("core/{file}"),
            std::fs::read_to_string(core.join(file)).unwrap(),
        )
    }));
    out
}

/// The keys of `sources` with no entry in `table`.
fn keys_without_entry(sources: &[(String, String)], table: &[(&str, &str)]) -> Vec<String> {
    let known: BTreeSet<&str> = table.iter().map(|(key, _)| *key).collect();
    let mut missing = Vec::new();
    for (file, code) in sources {
        missing.extend(
            keys_of(&scan(code))
                .into_iter()
                .filter(|key| !known.contains(key.as_str()))
                .map(|key| format!("{file}: {key:?}")),
        );
    }
    missing
}

/// The entries of `table` that are no key in `sources` and no chip.
fn unused_entries(
    sources: &[(String, String)],
    chips: &BTreeSet<&str>,
    table: &[(&'static str, &str)],
) -> Vec<&'static str> {
    let keys: BTreeSet<String> = sources
        .iter()
        .flat_map(|(_, code)| keys_of(&scan(code)))
        .collect();
    table
        .iter()
        .map(|(key, _)| *key)
        .filter(|key| !keys.contains(*key) && !chips.contains(key))
        .collect()
}

/// The holes of `text`: how many `{}`, the numbers of the numbered ones, and whether any is named.
fn holes(text: &str) -> (usize, Vec<usize>, bool) {
    let (mut plain, mut numbered, mut named) = (0, Vec::new(), false);
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        let inside = &rest[open + 1..open + close];
        if inside.is_empty() {
            plain += 1;
        } else if let Ok(number) = inside.parse() {
            numbered.push(number);
        } else {
            named = true;
        }
        rest = &rest[open + close + 1..];
    }
    (plain, numbered, named)
}

/// What is wrong with `table`'s entries (D3, D5): an empty value, a key twice, a named hole in a
/// key, holes that differ from the key's, a Thai digit, or a piece that is too long or starts with
/// a mark. A piece is what `.prose` draws without a break: the text up to and including a space.
fn not_ready(table: &[(&str, &str)]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for (key, value) in table {
        let mut problem = |what: &str| problems.push(format!("{key:?} → {value:?}: {what}"));
        if !seen.insert(*key) {
            problem("the key is in the table twice");
        }
        if value.is_empty() {
            problem("no value");
        }
        let (want, key_numbered, key_named) = holes(key);
        if !key_numbered.is_empty() || key_named {
            problem("a key's holes are only {}");
        }
        let (plain, mut numbered, named) = holes(value);
        numbered.sort_unstable();
        let in_order = (plain == want && numbered.is_empty())
            || (plain == 0 && numbered == (0..want).collect::<Vec<_>>());
        if named || !in_order {
            problem("not the key's holes");
        }
        if value
            .chars()
            .any(|c| ('\u{0E50}'..='\u{0E59}').contains(&c))
        {
            problem("a Thai digit");
        }
        for piece in value.split_inclusive(' ') {
            if piece.chars().count() > MAX_PIECE_CHARS {
                problem("a piece over the longest a line can hold");
            }
            if piece.chars().next().is_some_and(is_thai_mark) {
                problem("a piece starts with a Thai mark");
            }
        }
    }
    problems
}

// ---- The scanner ----

#[test]
fn the_scanner_reads_rust() {
    let code = r####"
// "a line comment"
/* "a block" /* "nested" */ comment */
#[error("an attribute")]
enum E { A }
#[doc = "another]"]
fn outer<'a>(x: &'a str) -> char {
    let quote = '"';
    let escaped = '\'';
    let slash = '\\';
    first("one", second("two", 3), ["three", "four"]);
    let raw = r#"raw "x""#;
    let plain_raw = r"C:\dir";
    let bytes = b"no text";
    let bytes_raw = br#"nor this"#;
    let byte = b'"';
    a.b::<Vec<u8>, u8>("after generics");
    Path::new("a path");
    x.method("arrow \u{2190} tab\t \"quoted\" \x41").other(
        "second line",
    );
    format!("{} {name}", 1);
    vec!["in a macro"];
    tr(if x { "a" } else { "b" });
    foo(Bar { a: "field", b: 1 }, "next");
    foo(1, (2, "tuple"), "last");
    if (x) { "no call" }
    let long = "one \
                two";
    r#type("raw identifier");
    let thai = "ไทย";
    'q'
}

#[cfg(test)]
mod tests {
    #[test]
    fn hidden() {
        call("under cfg(test)");
    }
}

#[test]
fn also_hidden() {
    call("under test");
}

#[cfg(test)]
const X: [&str; 1] = ["a test const"];

fn after() {
    call("after the tests");
}
"####;
    let lits = scan(code);
    let get = |text: &str| {
        lits.iter()
            .find(|lit| lit.text == text)
            .unwrap_or_else(|| panic!("{text:?} is missing from {lits:#?}"))
    };
    let chain = |text: &str| {
        get(text)
            .calls
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    };

    for hidden in [
        "a line comment",
        "a block",
        "nested",
        "an attribute",
        "another]",
        "no text",
        "nor this",
        "under cfg(test)",
        "under test",
        "a test const",
    ] {
        assert!(lits.iter().all(|lit| lit.text != hidden), "{hidden:?}");
    }
    assert_eq!(chain("one"), ["first"]);
    assert_eq!(get("one").arg, 0);
    assert_eq!(chain("two"), ["second", "first"]);
    assert_eq!(get("two").arg, 0);
    assert_eq!(chain("three"), ["first"]);
    assert_eq!(get("three").arg, 2);
    assert_eq!(get("four").arg, 2, "an array is one argument");
    assert_eq!(chain("raw \"x\""), [] as [&str; 0]);
    assert_eq!(chain("C:\\dir"), [] as [&str; 0]);
    assert_eq!(chain("after generics"), ["b"]);
    assert_eq!(chain("a path"), ["Path::new"]);
    assert_eq!(chain("arrow \u{2190} tab\t \"quoted\" A"), ["method"]);
    assert_eq!(chain("second line"), ["other"]);
    assert_eq!(get("second line").line, get("a path").line + 2);
    assert_eq!(chain("{} {name}"), ["format!"]);
    assert_eq!(chain("in a macro"), ["vec!"]);
    assert_eq!(chain("a"), ["tr"]);
    assert_eq!(get("a").arg, 0);
    assert_eq!(chain("b"), ["tr"]);
    assert_eq!(get("b").arg, 0);
    assert_eq!(chain("field"), ["foo"]);
    assert_eq!(get("field").arg, 0, "a struct's commas are its own");
    assert_eq!(get("next").arg, 1);
    assert_eq!(chain("tuple"), ["foo"]);
    assert_eq!(get("tuple").arg, 1);
    assert_eq!(get("last").arg, 2);
    assert_eq!(chain("no call"), [] as [&str; 0], "`if (x)` is no call");
    assert_eq!(get("one two").calls.len(), 0, "a line continuation");
    assert_eq!(chain("raw identifier"), ["r#type"]);
    assert_eq!(chain("ไทย"), [] as [&str; 0]);
    assert_eq!(
        chain("after the tests"),
        ["call"],
        "and it reads on after them"
    );
    assert_eq!(get("one").line, 11);
}

// ---- The rules, on fake input ----

#[test]
fn the_rules_flag_a_label_and_pass_an_id() {
    let flags = |code: &str| flagged_in("area/view.rs", code, &[]).0;
    let code = r#"
fn render() {
    ld_button("save-left", "Save left", p);
    div().id("Not an id's text").child("Open left…");
    ld_tooltip(tr("Reload"));
    trf("Merged {} automatically", &[&n]);
    count_of(n, "file", "files");
    trc("panel", "Left");
    paired("Help");
    tracing::warn!("a log line");
    Regex::new("a regex");
    window.prompt(level, "Delete it?", None, &["OK"], cx);
}
"#;
    assert_eq!(
        flags(code),
        [
            "area/view.rs:3 \"Save left\"",
            "area/view.rs:4 \"Open left…\"",
            "area/view.rs:12 \"Delete it?\"",
        ],
        "`OK` is all caps: it is `answers` that keeps it from a prompt"
    );

    for not_text in [
        "text::Save",
        "HexView",
        "folder-sync",
        "folder.report_mask",
        "UTF-16LE",
        "CRLF",
        ".html",
        "{} ↔ {}",
        "{value:.1} {}",
        "{bytes} B",
        "%Y-%m-%d %H:%M",
        "≠",
        "a",
    ] {
        assert!(shape_cannot_be_text(not_text), "{not_text:?}");
    }
    for text in [
        "Save left",
        "left",
        "right",
        "untitled",
        "invisible",
        "ok",
        "ไทย",
        "KiB",
        "Hello",
        "Merged {} automatically",
    ] {
        assert!(!shape_cannot_be_text(text), "{text:?}");
    }

    // An exempt file, and one literal in one file.
    assert!(
        flagged_in("help/en.rs", "x(\"Words here\");", &[])
            .0
            .is_empty()
    );
    assert!(
        flagged_in("crash.rs", "x(\"Words here\");", &[])
            .0
            .is_empty()
    );
    let allowed = [("area/view.rs", "Aa", "a toggle's face")];
    let (flagged, used) = flagged_in("area/view.rs", r#"x("Aa"); x("Words");"#, &allowed);
    assert_eq!(flagged, ["area/view.rs:1 \"Words\""]);
    assert_eq!(used, [("area/view.rs", "Aa")]);
    assert!(flagged_in("other.rs", r#"x("Aa");"#, &allowed).0.len() == 1);
    assert!(covers("folder/", "folder/view.rs") && !covers("folder/", "x/folder/view.rs"));
    assert!(covers("format.rs", "format.rs") && !covers("format.rs", "x/format.rs"));
}

#[test]
fn the_keys_of_a_source_are_what_tr_is_given() {
    let lits = scan(
        r#"
fn f() {
    tr("plain");
    crate::i18n::tr("by path");
    trf("{} holes", &[&"not a key"]);
    tr_key("marked");
    live("kept");
    count_of(n, "file", "files");
    trc("panel", "Left");
    tr(if x { "one" } else { "two" });
    format!("{}", "not a key either");
    child(tr("nested"));
}
"#,
    );
    assert_eq!(
        keys_of(&lits),
        [
            "plain",
            "by path",
            "{} holes",
            "marked",
            "kept",
            "files",
            "panel|Left",
            "one",
            "two",
            "nested"
        ]
    );
}

#[test]
fn a_key_with_no_entry_and_an_entry_with_no_key_are_found() {
    let sources = [("a.rs".to_owned(), r#"tr("has"); tr("has not");"#.to_owned())];
    let table = [("has", "มี"), ("nobody uses", "ไม่มี"), ("a chip", "ชิป")];
    assert_eq!(keys_without_entry(&sources, &table), ["a.rs: \"has not\""]);
    let chips = BTreeSet::from(["a chip"]);
    assert_eq!(unused_entries(&sources, &chips, &table), ["nobody uses"]);
}

#[test]
fn a_value_that_cannot_be_drawn_is_found() {
    let long = "ก".repeat(MAX_PIECE_CHARS + 1);
    let long: &str = &long;
    let mark = "ไฟล์ \u{0E48}ก";
    let bad: [(&str, &str, &str); 9] = [
        ("Empty", "", "no value"),
        ("Twice", "ก", "twice"),
        ("{name} here", "{} ก", "only {}"),
        ("{} and {}", "{} ก", "not the key's holes"),
        ("{} and {}", "{0} ก {0}", "not the key's holes"),
        ("{} once", "{name} ก", "not the key's holes"),
        ("Digit", "๑ ไฟล์", "a Thai digit"),
        ("Long", long, "a piece over"),
        ("Mark", mark, "starts with a Thai mark"),
    ];
    for (key, value, why) in bad {
        let mut table = vec![(key, value)];
        if key == "Twice" {
            table.push((key, value));
        }
        let problems = not_ready(&table);
        assert!(
            problems.iter().any(|p| p.contains(why)),
            "{key:?} → {value:?}: {problems:?}"
        );
    }
    let good = [
        ("{} of {}", "{} จาก {}"),
        ("{} and {}", "{1} และ {0}"),
        ("Plain", "ไฟล์ทั้งหมด"),
        ("Long words", "คำยาวคำหนึ่ง อีกคำหนึ่ง"),
        ("panel|Left", "ฝั่งซ้าย"),
    ];
    assert_eq!(not_ready(&good), [] as [String; 0]);
}

// ---- Prose goes through the helper (D5) ----

/// The longest text `.child(tr(..))` may draw: past this a Thai sentence can wrap, and a wrapped
/// Thai line breaks inside a word unless `.prose()` draws it.
const MAX_CHILD_CHARS: usize = 28;

/// The words a field holding a sentence is called by, which `.child(field)` must not draw.
const PROSE_FIELDS: &[&str] = &["notice", "message", "hint", "error", "summary", "note"];

/// A place that draws a sentence on a line that cannot wrap, so `.child` is right: a status bar
/// truncates, and a notice there ends in an ellipsis instead of a break (D5).
// Every entry is a status bar: the status line is one line that truncates (`status_line_box`), so a sentence there never wraps.
// Notices in a bar go through `status_notice`, which cuts to one line and shows the whole sentence as a `.prose` tooltip.
const PROSE_ALLOWED: &[(&str, &str)] = &[("folder/view/status.rs", ".child(note…)")];

/// The places in `code` where a sentence is drawn by `.child(..)` and not by `.prose(..)`: a `tr`
/// or `trf` literal over `MAX_CHILD_CHARS`, or a field that holds a sentence.
fn prose_children(file: &str, code: &str) -> Vec<String> {
    let mut found = Vec::new();
    if EXEMPT.iter().any(|(entry, _)| covers(entry, file)) {
        return found;
    }
    for lit in scan(code) {
        let mut calls = lit.calls.iter().map(|call| last_segment(call));
        let (first, second) = (calls.next(), calls.next());
        if matches!(first, Some("tr" | "trf"))
            && second == Some("child")
            && lit.text.chars().count() > MAX_CHILD_CHARS
        {
            found.push(format!("{file}:{} {:?}", lit.line, lit.text));
        }
    }
    let mut in_tests = false;
    for (number, line) in code.lines().enumerate() {
        // The scanner skips test code; here everything from the first `#[cfg(test)]` on is test code
        // in this crate's files.
        in_tests |= line.trim_start().starts_with("#[cfg(test)]");
        if in_tests {
            break;
        }
        let mut rest = line;
        while let Some(at) = rest.find(".child(") {
            rest = &rest[at + ".child(".len()..];
            let name: String = rest
                .trim_start()
                .trim_start_matches("self.")
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let after = rest.trim_start().trim_start_matches("self.")[name.len()..]
                .chars()
                .next();
            if PROSE_FIELDS.contains(&name.as_str()) && !after.is_some_and(|c| c == '(' || c == ':')
            {
                found.push(format!("{file}:{} .child({name}…)", number + 1));
            }
        }
    }
    found
}

// ---- The real sources ----

#[test]
fn every_key_has_a_thai_entry() {
    let missing = keys_without_entry(&converted_sources(), th::table());
    assert!(missing.is_empty(), "no Thai entry: {missing:#?}");
}

/// The chips `help/th.rs` writes, each drawn through `tr` (D7).
fn thai_chips() -> BTreeSet<&'static str> {
    let mut chips = BTreeSet::new();
    let welcome = topics::welcome_copy(HelpLang::Th);
    let copy = [welcome.title]
        .into_iter()
        .chain(welcome.lines)
        .flat_map(topics::pieces);
    let pages = TOPICS
        .iter()
        .flat_map(|topic| topics::topic_pieces(topic, HelpLang::Th));
    for piece in pages.chain(copy) {
        for span in markup::marks(piece) {
            if let Span::Chip(label) = span {
                chips.insert(label);
            }
        }
    }
    chips
}

#[test]
fn no_thai_entry_is_unused() {
    let unused = unused_entries(&converted_sources(), &thai_chips(), th::table());
    assert!(unused.is_empty(), "no key uses: {unused:#?}");
}

#[test]
fn thai_values_are_ready_to_draw() {
    let problems = not_ready(th::table());
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn no_prose_child_skips_the_helper() {
    // The check itself, on fake input.
    let code = r#"
fn render() {
    div().child(tr("Short label"));
    div().child(tr("A sentence that is longer than a label is"));
    div().child(trf("Merged {} automatically; {} a choice.", &[&a, &b]));
    div().child(notice);
    div().child(self.message.clone());
    div().child(message_title());
    div().prose(tr("A sentence that is longer than a label is"));
}
"#;
    let found = prose_children("area/view.rs", code);
    assert_eq!(
        found,
        [
            "area/view.rs:4 \"A sentence that is longer than a label is\"",
            "area/view.rs:5 \"Merged {} automatically; {} a choice.\"",
            "area/view.rs:6 .child(notice…)",
            "area/view.rs:7 .child(message…)",
        ],
        "{found:#?}"
    );

    let mut flagged = Vec::new();
    let mut used = BTreeSet::new();
    for (file, code) in converted_sources() {
        for place in prose_children(&file, &code) {
            match PROSE_ALLOWED
                .iter()
                .find(|(entry, text)| covers(entry, &file) && place.contains(text))
            {
                Some((entry, text)) => {
                    used.insert((*entry, *text));
                }
                None => flagged.push(place),
            }
        }
    }
    assert!(
        flagged.is_empty(),
        "draw a sentence with .prose(): {flagged:#?}"
    );
    let stale: Vec<_> = PROSE_ALLOWED
        .iter()
        .filter(|entry| !used.contains(entry))
        .collect();
    assert!(
        stale.is_empty(),
        "PROSE_ALLOWED entries that match nothing: {stale:#?}"
    );
}

#[test]
fn no_user_facing_literal_skips_tr() {
    let mut flagged = Vec::new();
    let mut used = BTreeSet::new();
    for (file, code) in converted_sources() {
        let (found, excused) = flagged_in(&file, &code, ALLOWED);
        flagged.extend(found);
        used.extend(excused);
    }
    assert!(flagged.is_empty(), "no tr: {flagged:#?}");
    let stale: Vec<_> = ALLOWED
        .iter()
        .filter(|(file, literal, _)| !used.contains(&(*file, *literal)))
        .collect();
    assert!(
        stale.is_empty(),
        "ALLOWED entries that match nothing: {stale:#?}"
    );
}

// ---- A key const shown bare ----
//
// The scan reads literals, and a `tr_key` const holds one only where it is declared, so using the
// const as text (`CONST.into()`, `show_notice(CONST, ..)`) shows the English key in Thai and is
// not flagged by the literal rules above. A key const goes through `tr` where it becomes text.

/// Functions that show their argument as it is.
const TEXT_SINKS: &[&str] = &["show_notice", "undo_swap", "tell", "say"];

/// The `&str` consts of `code` that hold a `tr_key(..)`.
fn key_consts(code: &str) -> Vec<String> {
    let re =
        regex::Regex::new(r"const\s+([A-Z][A-Z0-9_]*)\s*:\s*&(?:'static\s+)?str\s*=\s*tr_key\(")
            .unwrap();
    re.captures_iter(code).map(|c| c[1].to_owned()).collect()
}

/// The places in `code` (test code left out) where one of `names` becomes text without `tr`.
fn bare_key_uses(file: &str, code: &str, names: &[String]) -> Vec<String> {
    if names.is_empty() {
        return Vec::new();
    }
    let code = code.split("#[cfg(test)]").next().unwrap_or(code);
    let (sinks, names) = (TEXT_SINKS.join("|"), names.join("|"));
    let re = regex::Regex::new(&format!(
        r"\b(?P<name>{names})\s*\.\s*(?:into|to_owned|to_string)\s*\(\s*\)|\b(?:{sinks})\s*\(\s*(?:self\s*\.\s*)?(?P<arg>{names})\b"
    ))
    .unwrap();
    let mut found: Vec<String> = re
        .captures_iter(code)
        .map(|hit| {
            let all = hit.get(0).unwrap();
            let name = hit.name("name").or(hit.name("arg")).unwrap().as_str();
            let line = code[..all.start()].matches('\n').count() + 1;
            format!("{file}:{line} {name} shown without tr")
        })
        .collect();
    found.sort();
    found
}

#[test]
fn a_key_const_is_never_shown_bare() {
    // The check itself, on fake input.
    let code = r#"
pub(crate) const WAIT: &str = tr_key("Wait for it.");
const LONG: &str =
    tr_key("A key on its own line");
const PLAIN: &str = "not a key";
fn f() {
    self.notice = Some(tr(WAIT).into());
    self.notice = Some(WAIT.into());
    view.show_notice(WAIT, cx);
    view.undo_swap(LONG, window, cx);
    view.show_notice(tr(LONG), cx);
    let _ = PLAIN.into();
    live(WAIT);
}
#[cfg(test)]
mod tests {
    fn g() { x.show_notice(WAIT, cx); }
}
"#;
    let names = key_consts(code);
    assert_eq!(names, ["WAIT", "LONG"]);
    assert_eq!(
        bare_key_uses("a.rs", code, &names),
        [
            "a.rs:10 LONG shown without tr",
            "a.rs:8 WAIT shown without tr",
            "a.rs:9 WAIT shown without tr",
        ]
    );

    let sources = converted_sources();
    let names: Vec<String> = sources
        .iter()
        .flat_map(|(_, code)| key_consts(code))
        .collect();
    assert!(names.len() > 50, "the scan finds the key consts");
    let found: Vec<String> = sources
        .iter()
        .flat_map(|(file, code)| bare_key_uses(file, code, &names))
        .collect();
    assert!(found.is_empty(), "wrap in tr(..): {found:#?}");
}

// ---- Core's report text builders belong in a job ----
//
// `scoped(Th)` sets the language of the thread it runs on, and core's `tr` reads that. A test
// thread is the UI thread, so a UI call into a core text builder reads Thai in a test; the real UI
// thread has no scope of its own, and reads the window's language, which is also what the builders
// would read unless the report's language differs. A builder called where no `scoped(..)` has set
// the report's language would be right in the test and wrong in the app.

/// The core functions that build the words of a report, which the language in force decides.
const REPORT_TEXT_BUILDERS: &[&str] = &[
    "criterion_text(",
    "rules_text(",
    "default_names(",
    ".describe(",
];

/// The calls in `code` (test code left out) of a report text builder from a function that never
/// calls `scoped(`, as "file:line call".
fn builders_outside_a_scope(file: &str, code: &str) -> Vec<String> {
    let code = code.split("#[cfg(test)]").next().unwrap_or(code);
    let lines: Vec<&str> = code.lines().collect();
    let mut found = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with("use ") || trimmed.contains("fn ") {
            continue;
        }
        for call in REPORT_TEXT_BUILDERS {
            if !line.contains(call)
                || (!call.starts_with('.') && line.contains(&format!(".{call}")))
            {
                continue;
            }
            // The enclosing function: the nearest earlier line that opens one.
            let start = (0..=at)
                .rev()
                .find(|&i| {
                    let t = lines[i].trim_start();
                    t.starts_with("fn ")
                        || t.starts_with("pub fn ")
                        || t.starts_with("pub(crate) fn ")
                        || t.starts_with("pub(super) fn ")
                })
                .unwrap_or(0);
            if !lines[start..=at].iter().any(|l| l.contains("scoped(")) {
                found.push(format!("{file}:{} {}", at + 1, call.trim_matches('.')));
            }
        }
    }
    found
}

#[test]
fn a_ui_call_to_a_report_text_builder_sets_the_report_language() {
    // The check itself, on fake input.
    let code = r#"
fn in_a_job() {
    let _lang = scoped(self.options.lang);
    let text = criterion_text(mode);
}
fn on_the_ui_thread() {
    let text = criterion_text(mode);
    let names = default_names(&a, &b);
}
fn also_fine(lang: Lang) -> [String; 2] {
    let _lang = scoped(lang);
    default_names(&self.left, &self.right)
}
fn other() {
    self.prefs.rules_text.clone();
    let side = lf.describe();
}
#[cfg(test)]
mod tests {
    fn t() { let x = lf.describe(); }
}
"#;
    assert_eq!(
        builders_outside_a_scope("a.rs", code),
        [
            "a.rs:7 criterion_text(",
            "a.rs:8 default_names(",
            "a.rs:16 describe("
        ]
    );
    let found: Vec<String> = converted_sources()
        .iter()
        .flat_map(|(file, code)| builders_outside_a_scope(file, code))
        .collect();
    assert!(
        found.is_empty(),
        "set the report's language with scoped(..) first: {found:#?}"
    );
}
