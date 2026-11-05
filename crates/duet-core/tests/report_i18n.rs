//! The folder reports in two languages. English is pinned byte for byte by a CRC-32 of each
//! output of a fixed compare; Thai is held to the table, to its shape and to leaving no English
//! behind.

#[path = "support/report_fixture.rs"]
mod fixture;
#[path = "support/report_text.rs"]
mod report_text;
#[path = "support/rust_scan.rs"]
mod rust_scan;
#[path = "support/text_table_fixture.rs"]
mod text_table;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::AtomicUsize;

use duet_core::cancel::CancelToken;
use duet_core::folder::report::csv::{csv_columns, folder_csv, row_data};
use duet_core::folder::report::html::folder_html;
use duet_core::folder::report::lines::{LineDetails, LineOptions, line_details};
use duet_core::folder::report::mask::hidden;
use duet_core::folder::report::text::folder_text;
use duet_core::folder::report::xlsx::folder_xlsx;
use duet_core::folder::report::{ReportContext, ReportItem};
use duet_core::folder::run::FolderTextRules;
use duet_core::i18n::{Lang, scoped, tr};
use report_text::assert_no_stray_english;

fn crc(bytes: &[u8]) -> u32 {
    crc32fast::hash(bytes)
}

/// Every output of the hand-built compare, in English, as `(name, CRC-32)`.
fn english_outputs() -> Vec<(String, u32)> {
    let items = fixture::items(None);
    let lines = fixture::lines_by_hand();
    let mut out = Vec::new();
    for (label, options, context, with_lines) in [
        ("full", fixture::options(Lang::En), fixture::context(), true),
        (
            "bare",
            fixture::bare_options(Lang::En),
            fixture::bare_context(),
            false,
        ),
    ] {
        let lines = with_lines.then_some(&lines);
        let html = folder_html(&items, lines, &options, &context);
        out.push((format!("{label} html"), crc(html.as_bytes())));
        let text = folder_text(&items, lines, &options, &context);
        out.push((format!("{label} text"), crc(text.as_bytes())));
        let csv = folder_csv(&items, lines, &options.names);
        out.push((format!("{label} csv"), crc(csv.as_bytes())));
        let book = folder_xlsx(&items, lines, &options, &context).unwrap();
        out.push((format!("{label} xlsx"), crc(&book)));
    }
    for (name, text) in text_table::text_outputs(ENGLISH_STATUS, ENGLISH_DATE) {
        out.push((format!("text {name}"), crc(text.as_bytes())));
    }
    for (name, text) in text_table::table_outputs(ENGLISH_DESCRIBE, ENGLISH_DATE) {
        out.push((format!("table {name}"), crc(text.as_bytes())));
    }
    out
}

const ENGLISH_STATUS: &str = "3 differences · 2 lines changed";
const ENGLISH_DATE: &str = "2026-11-05 10:00";
const ENGLISH_DESCRIBE: [&str; 2] = [
    r"C:\a\l.csv (UTF-8, comma (,) auto)",
    r"C:\b\r.csv (UTF-8, comma (,) auto)",
];

/// The CRC-32 of each English output of the fixed compare, taken before the reports learnt a
/// second language. A report edit that moves one of these changed English.
const ENGLISH_PIN: &[(&str, u32)] = &[
    ("full html", 0xb36a_fe05),
    ("full text", 0x8cd8_9e42),
    ("full csv", 0xb43d_29b3),
    ("full xlsx", 0x3440_9c84),
    ("bare html", 0x26ef_4815),
    ("bare text", 0x49c9_a6b3),
    ("bare csv", 0x52c9_a8c4),
    ("bare xlsx", 0xc012_973a),
    ("text text report", 0x9b18_faaf),
    ("text text report no differences", 0xad5f_5bf9),
    ("text patch", 0xefe7_3ad3),
    ("text html context", 0x00ea_ba49),
    ("text html differences", 0xefb5_d0bc),
    ("text html all", 0x8277_1030),
    ("text html soft", 0x385f_239f),
    ("text html stopped", 0x35fb_78ab),
    ("table keyed csv", 0x14e3_f84c),
    ("table plain csv", 0x8de3_765a),
    ("table keyed html", 0x8b4c_f6a7),
    ("table plain html", 0x6e0e_cad0),
    ("table same html", 0x8011_75b6),
];

#[test]
fn english_report_bytes_are_unchanged() {
    let now = english_outputs();
    let got: Vec<(&str, u32)> = now.iter().map(|(n, c)| (n.as_str(), *c)).collect();
    assert_eq!(got, ENGLISH_PIN, "an English report changed");
}

// ---- The table against the sources ----

/// The sources whose text reaches a report, as `(path under src, code)`. `classify.rs` is there
/// for the words `TextFormat::describe` writes into one.
fn report_sources() -> Vec<(String, String)> {
    fn visit(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.is_dir() {
                visit(root, &path, out);
            } else if path.extension().is_some_and(|e| e == "rs")
                && name != "tests.rs"
                && name != "test_data.rs"
            {
                let relative = path.strip_prefix(root).unwrap().to_string_lossy();
                out.push((
                    relative.replace('\\', "/"),
                    std::fs::read_to_string(&path).unwrap(),
                ));
            }
        }
    }
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    visit(&src, &src.join("folder/report"), &mut out);
    for file in ["folder/classify.rs", "table/report.rs", "text/report.rs"] {
        out.push((
            file.to_owned(),
            std::fs::read_to_string(src.join(file)).unwrap(),
        ));
    }
    out.sort();
    out
}

/// The keys of `lits`: what `tr`, `tr_key` and `trf` are given, and the noun a `count_text`
/// call ends with.
fn keys_of(lits: &[rust_scan::Lit]) -> Vec<String> {
    lits.iter()
        .filter(|lit| {
            let Some(call) = lit.calls.first() else {
                return false;
            };
            matches!(
                (call.rsplit("::").next().unwrap_or(call), lit.arg),
                ("tr" | "tr_key", _) | ("trf", 0) | ("count_text", 2)
            )
        })
        .map(|lit| lit.text.clone())
        // `count_text(n, "", "")` is a bare number.
        .filter(|key| !key.is_empty())
        .collect()
}

fn source_keys() -> BTreeSet<String> {
    report_sources()
        .iter()
        .flat_map(|(_, code)| keys_of(&rust_scan::scan(code)))
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

/// What is wrong with the entries of `table`: no value, a key twice, holes that are not the
/// key's, a Thai digit, or a character a template writes raw (`<`, `>`, `&`, `"`).
fn not_ready(table: &[(&str, &str)]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for (key, value) in table {
        let mut problem = |what: &str| problems.push(format!("{key:?} -> {value:?}: {what}"));
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
        // The text report's own `<` and `>` are in its keys; a value adds none.
        if value
            .chars()
            .any(|c| matches!(c, '<' | '>' | '&' | '"') && !key.contains(c))
        {
            problem("a character a template writes raw");
        }
    }
    problems
}

#[test]
fn every_report_key_has_a_thai_entry() {
    let known: BTreeSet<&str> = duet_core::i18n::report_entries()
        .iter()
        .map(|(key, _)| *key)
        .collect();
    let missing: Vec<String> = source_keys()
        .into_iter()
        .filter(|key| !known.contains(key.as_str()))
        .collect();
    assert!(missing.is_empty(), "no Thai entry: {missing:#?}");
}

#[test]
fn no_report_entry_is_unused() {
    let keys = source_keys();
    let unused: Vec<&str> = duet_core::i18n::report_entries()
        .iter()
        .map(|(key, _)| *key)
        .filter(|key| !keys.contains(*key))
        .collect();
    assert!(unused.is_empty(), "no key uses: {unused:#?}");
}

#[test]
fn thai_values_are_ready_to_write() {
    let problems = not_ready(duet_core::i18n::report_entries());
    assert!(problems.is_empty(), "{problems:#?}");
    // The check itself, on fake input.
    let fake = [
        ("{} files", "{} <b>ไฟล์</b>"),
        ("a", ""),
        ("b {}", "ข"),
        ("c", "๑"),
    ];
    assert_eq!(not_ready(&fake).len(), 4, "{:?}", not_ready(&fake));
}

// ---- Thai output ----

/// Words that stay English in a Thai report: format and encoding names, the product's name, the
/// words of a config file, and the words of the page's own markup and script.
const ALLOWED: &[&str] = &[
    "duet",
    "hex",
    "CRLF",
    "LF",
    "CR",
    "BOM",
    "EOL",
    "UTF",
    "YAML",
    "BEGIN",
    "END",
    "PRIVATE",
    "KEY",
    "Linux",
    "Windows",
    "Mac",
    "NBSP",
    "LRM",
    "MiB",
    "KiB",
    "GiB",
    "TiB",
    "config",
    "encoding",
    "path",
    "password",
    "conflict",
    "bash",
    "bad",
    "interpreter",
    "html",
    "lang",
    "charset",
    "utf",
    "width",
    "device",
    "initial",
    "scale",
    "viewport",
    "meta",
    "type",
    "search",
    "checkbox",
    "button",
    "checked",
    "toLocaleString",
    "textContent",
    "cnt",
    "windows",
    // The legend's placeholder for a fingerprint, and the Unicode names in a tooltip, which stay
    // English (question 6).
    "xxxx",
    "break",
    "space",
    // File extensions, in the CSV's Type column.
    "dat",
    "xml",
    "cfg",
    "properties",
    "env",
    "txt",
    "bin",
    "sh",
    "db",
];

/// The names and paths the fixture put into an output, which are the compare's own words.
fn fixture_words(root: &Path, items: &[ReportItem]) -> Vec<String> {
    let mut words: Vec<String> = vec![
        root.to_string_lossy().into_owned(),
        "IWT".into(),
        "PRD".into(),
        "Check before deploy".into(),
        "First line".into(),
        "Second line".into(),
        "9.9.9".into(),
        "Access is denied.".into(),
        "*.cfg".into(),
        "tmp".into(),
        "C:\\left".into(),
        "C:\\right".into(),
    ];
    for item in items {
        words.push(item.rel.clone());
        for side in [&item.left, &item.right].into_iter().flatten() {
            words.push(side.rel.clone());
            words.push(side.path.to_string_lossy().into_owned());
        }
        words.extend(item.rel.split('/').map(str::to_owned));
    }
    words.extend(
        [
            "url", "jdbc", "user", "svc", "timeout", "Secr", "echo", "keep", "last", "extra",
            "one", "two", "only",
        ]
        .map(str::to_owned),
    );
    words
}

/// The fixture's items with real files under `root`, and the line details read from them in
/// `lang`: a password, a binary file, a file over the limit, a look-alike space.
fn real_compare(root: &Path, lang: Lang) -> (Vec<ReportItem>, LineDetails) {
    let mut items = fixture::items(Some(root));
    items.push(fixture::plain(
        Some(root),
        "app/secrets.env",
        duet_core::folder::report::ItemState::Different,
    ));
    let write = |rel: &str, left: &[u8], right: &[u8]| {
        for (side, bytes) in [("left", left), ("right", right)] {
            let path = root.join(side).join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
    };
    write(
        "app/secrets.env",
        b"url=jdbc:x\nuser=svc\npassword=Secr3t-A\ntimeout=30\nkeep\nkeep\nkeep\nkeep\nkeep\nlast=1\n",
        b"url=jdbc:x\nuser=svc\npassword=Secr3t-B\ntimeout=60\nkeep\nkeep\nkeep\nkeep\nkeep\nlast=2\nextra\n",
    );
    write(
        "app/limit.properties",
        "k = v\n".as_bytes(),
        "k =\u{a0}v\n".as_bytes(),
    );
    write("app/cr-only.cfg", b"a\r", b"a\n");
    write("root.cfg", b"one\n", b"two\n");
    write("app/blob.bin", b"\0\x01\x02", b"\0\x01\x03");
    let big = vec![b'x'; 8 * 1024 * 1024 + 1];
    write("app/big.dat", &big, &big);
    let _lang = scoped(lang);
    let lines = line_details(
        &items,
        &FolderTextRules::default(),
        &LineOptions {
            rows: true,
            sheet: true,
            ..LineOptions::default()
        },
        &AtomicUsize::new(0),
        &CancelToken::new(),
    );
    (items, lines)
}

/// A context in Thai: the words the app builds it from.
fn thai_context(bare: bool) -> ReportContext {
    use duet_core::folder::report::criterion_text;
    use duet_core::folder::run::CompareMode;
    let _th = scoped(Lang::Th);
    let mut context = if bare {
        fixture::bare_context()
    } else {
        fixture::context()
    };
    context.criterion = criterion_text(if bare {
        CompareMode::Size
    } else {
        CompareMode::Content
    })
    .to_owned();
    context.scope = "ทุกรายการ".to_owned();
    context.rules = context.rules.map(|_| "ไม่สนใจตัวพิมพ์เล็กใหญ่".to_owned());
    context
}

#[test]
fn thai_csv_names_its_columns_in_thai_and_keeps_the_bom() {
    let root = tempfile::tempdir().unwrap();
    let (items, lines) = real_compare(root.path(), Lang::Th);
    let _th = scoped(Lang::Th);
    let csv = folder_csv(&items, Some(&lines), &fixture::names());
    assert!(csv.starts_with('\u{feff}'), "the BOM stays");
    let header = csv.trim_start_matches('\u{feff}').lines().next().unwrap();
    assert_eq!(
        header,
        "ลำดับ,โฟลเดอร์,ชื่อไฟล์,ชนิด,สถานะ,ควรตรวจดู,เปลี่ยน (บรรทัด),เฉพาะใน IWT (บรรทัด),\
         เฉพาะใน PRD (บรรทัด),จุด,หมายเหตุ,แก้ไขทีหลัง,ขนาด IWT (ไบต์),ขนาด PRD (ไบต์),\
         แก้ไขเมื่อ IWT,แก้ไขเมื่อ PRD,path IWT,path PRD,ผู้ตรวจ,ผลตรวจ,หมายเหตุผู้ตรวจ"
    );
    for status in ["ต่างกัน", "เฉพาะใน IWT", "เฉพาะใน PRD", "เหมือนกัน"]
    {
        assert!(csv.contains(&format!(",{status},")), "{status}");
    }
    assert!(csv.contains("(ระดับบนสุด)"));
    assert!(csv.ends_with("\r\n"));
    assert_eq!(csv_columns(&fixture::names()).len(), 21);
}

#[test]
fn thai_text_report_reads_in_thai() {
    let root = tempfile::tempdir().unwrap();
    let (items, lines) = real_compare(root.path(), Lang::Th);
    let options = fixture::options(Lang::Th);
    // The writer takes the language from the options, not from the thread.
    assert_eq!(duet_core::i18n::scoped_lang(), None);
    let text = folder_text(&items, Some(&lines), &options, &thai_context(false));
    assert_eq!(duet_core::i18n::scoped_lang(), None, "the scope is gone");
    assert!(text.starts_with("รายงานเทียบโฟลเดอร์ — Check before deploy\r\n"));
    for part in [
        "สรุป",
        "ควรตรวจดูเป็นพิเศษ",
        "รายละเอียดแยกตามโฟลเดอร์",
        "[(ระดับบนสุด)]",
        "ต่างกันเฉพาะส่วนที่ไม่สำคัญ (นับเป็นเหมือนกัน)",
        "  ≠ ต่างกัน ",
        "  < เฉพาะใน A ",
        "(ซ่อนค่า #",
        "เกณฑ์: เนื้อหาไฟล์ (ไม่นับเวลาแก้ไข)",
        " · ซ่อนค่าที่ดูเหมือนรหัสผ่านหรือคีย์",
    ] {
        assert!(text.contains(part), "{part:?} in\n{text}");
    }
    // Numbers stay Arabic digits.
    assert!(!text.chars().any(|c| ('\u{0E50}'..='\u{0E59}').contains(&c)));
}

#[test]
fn a_hidden_value_reads_in_thai_and_keeps_its_fingerprint() {
    assert_eq!(hidden("abc"), "•••••• (hidden #1a47)");
    let _th = scoped(Lang::Th);
    assert_eq!(hidden("abc"), "•••••• (ซ่อนค่า #1a47)");
}

#[test]
fn thai_csv_and_text_carry_no_stray_english() {
    let root = tempfile::tempdir().unwrap();
    let (items, lines) = real_compare(root.path(), Lang::Th);
    let words = fixture_words(root.path(), &items);
    let strip: Vec<&str> = words.iter().map(String::as_str).collect();
    let _th = scoped(Lang::Th);
    let csv = folder_csv(&items, Some(&lines), &fixture::names());
    assert_no_stray_english(&csv, &strip, ALLOWED);
    for (options, context, with_lines) in [
        (fixture::options(Lang::Th), thai_context(false), true),
        (fixture::bare_options(Lang::Th), thai_context(true), false),
    ] {
        let text = folder_text(&items, with_lines.then_some(&lines), &options, &context);
        assert_no_stray_english(&text, &strip, ALLOWED);
    }
}

#[test]
fn a_report_without_a_scope_is_english_whatever_the_ui_says() {
    let items = fixture::items(None);
    let names = fixture::names();
    // No scope: English, in every writer and helper.
    let csv = folder_csv(&items, None, &names);
    assert!(csv.contains("No.,Folder,File name,Type,Status,Needs a look"));
    assert_eq!(csv_columns(&names)[0], "No.");
    assert_eq!(tr("Summary"), "Summary");
    assert_eq!(hidden("abc"), "•••••• (hidden #1a47)");
    let row = row_data(&items[0], 0, &names, None);
    assert_eq!(row.state, "Different");
    // A Thai thread asking for an English report gets one, and keeps its own language after.
    let english_text = folder_text(
        &items,
        None,
        &fixture::options(Lang::En),
        &fixture::context(),
    );
    let _th = scoped(Lang::Th);
    let asked_in_english = folder_text(
        &items,
        None,
        &fixture::options(Lang::En),
        &fixture::context(),
    );
    assert_eq!(asked_in_english, english_text);
    assert_eq!(duet_core::i18n::scoped_lang(), Some(Lang::Th));
    let asked_in_thai = folder_text(
        &items,
        None,
        &fixture::options(Lang::Th),
        &fixture::context(),
    );
    assert_ne!(asked_in_thai, english_text);
}

// ---- The HTML page in Thai ----

/// The page's script, whole.
fn script_of(page: &str) -> &str {
    let from = page.find("<script>").expect("a script") + 8;
    &page[from..from + page[from..].find("</script>").unwrap()]
}

#[test]
fn thai_html_sets_lang_th() {
    let items = fixture::items(None);
    let lines = fixture::lines_by_hand();
    for (lang, code) in [(Lang::Th, "th"), (Lang::En, "en")] {
        let page = folder_html(
            &items,
            Some(&lines),
            &fixture::options(lang),
            &thai_context(false),
        );
        assert!(
            page.starts_with(&format!("<!DOCTYPE html><html lang=\"{code}\">")),
            "{}",
            &page[..60]
        );
    }
}

#[test]
fn thai_html_counts_files_in_thai_with_arabic_digits() {
    let items = fixture::items(None);
    let lines = fixture::lines_by_hand();
    let th = folder_html(
        &items,
        Some(&lines),
        &fixture::options(Lang::Th),
        &thai_context(false),
    );
    let en = folder_html(
        &items,
        Some(&lines),
        &fixture::options(Lang::En),
        &fixture::context(),
    );
    let (th_script, en_script) = (script_of(&th), script_of(&en));
    assert!(
        th_script.contains("toLocaleString(\"en-US\")"),
        "Arabic digits"
    );
    assert!(th_script.contains("แสดง "), "{th_script}");
    assert!(!th_script.contains("Showing"));
    assert!(!th_script.contains("(n===1?"), "no plural in Thai");
    assert!(en_script.contains("\"Showing \"+n.toLocaleString()+(n===1?\" file\":\" files\")"));
    // Nothing else in the script moves: cut the count line out of both.
    let cut = |script: &str| {
        let from = script.find("cnt.textContent=").unwrap();
        let to = from + script[from..].find(";}").unwrap();
        format!("{}{}", &script[..from], &script[to..])
    };
    assert_eq!(cut(th_script), cut(en_script));
    // The digits on the page are Arabic too.
    assert!(!th.chars().any(|c| ('\u{0E50}'..='\u{0E59}').contains(&c)));
}

#[test]
fn thai_html_carries_no_stray_english() {
    let root = tempfile::tempdir().unwrap();
    let (items, lines) = real_compare(root.path(), Lang::Th);
    let words = fixture_words(root.path(), &items);
    let strip: Vec<&str> = words.iter().map(String::as_str).collect();
    for (options, context, with_lines) in [
        (fixture::options(Lang::Th), thai_context(false), true),
        (fixture::bare_options(Lang::Th), thai_context(true), false),
    ] {
        let page = folder_html(&items, with_lines.then_some(&lines), &options, &context);
        assert_no_stray_english(&page, &strip, ALLOWED);
    }
}

// ---- The text and table reports ----

/// What the app writes for a side in a Thai report: its own words, built under the same scope.
const THAI_DESCRIBE: [&str; 2] = [
    r"C:\a\l.csv (UTF-8, ตัวคั่นคอมมา (,) ตรวจเอง)",
    r"C:\b\r.csv (UTF-8, ตัวคั่นคอมมา (,) ตรวจเอง)",
];
const THAI_STATUS: &str = "พบ 3 จุดต่าง";

/// The text compare's own words: the file contents and names the fixture put in.
const TEXT_TABLE_WORDS: &[&str] = &[
    "left <1>.txt",
    "right & 2.txt",
    "left &lt;1&gt;.txt",
    "right &amp; 2.txt",
    "left &lt;1&gt;.csv",
    "right &amp; 2.csv",
    "left <1>.csv",
    "right & 2.csv",
    "alpha",
    "beta",
    "BETA",
    "gamma",
    "delta",
    "epsilon",
    "zeta",
    "eta",
    "theta",
    "iota",
    "kappa",
    "lambda",
    "omicron",
    "rho",
    r"C:\a\l.csv",
    r"C:\b\r.csv",
];

fn by_name(outputs: Vec<(&'static str, String)>, name: &str) -> String {
    outputs
        .into_iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("no output {name}"))
        .1
}

fn thai_text_outputs() -> Vec<(&'static str, String)> {
    let _th = scoped(Lang::Th);
    text_table::text_outputs(THAI_STATUS, "2026-11-05 10:00")
}

fn thai_table_outputs() -> Vec<(&'static str, String)> {
    let _th = scoped(Lang::Th);
    text_table::table_outputs(THAI_DESCRIBE, "2026-11-05 10:00")
}

#[test]
fn thai_text_html_and_text_reports_read_in_thai() {
    let outputs = thai_text_outputs();
    let text = by_name(thai_text_outputs(), "text report");
    assert!(
        text.starts_with(
            "\u{feff}duet: รายงานเทียบข้อความ\r\nฝั่งซ้าย: left <1>.txt\r\nฝั่งขวา: right & 2.txt\r\nวันที่  : 2026-11-05 10:00\r\nพบ 3 จุดต่าง\r\n\r\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("=== จุดต่างที่ 1 · ซ้าย 2-3 · ขวา 2-2 ==="),
        "{text}"
    );
    assert!(text.contains("· ซ้าย แทรกที่ 17 · ขวา 16-16 ==="), "{text}");
    let none = by_name(thai_text_outputs(), "text report no differences");
    assert!(none.starts_with("\u{feff}duet: รายงานเทียบข้อความ"));

    for name in [
        "html context",
        "html differences",
        "html all",
        "html soft",
        "html stopped",
    ] {
        let page = by_name(thai_text_outputs(), name);
        assert!(
            page.starts_with("<!DOCTYPE html><html lang=\"th\">"),
            "{name}"
        );
        for part in [
            "<h1>รายงานเทียบข้อความ</h1>",
            "ฝั่งซ้าย: <b>left &lt;1&gt;.txt</b>",
            "ฝั่งขวา: <b>right &amp; 2.txt</b>",
            "สร้างเมื่อ 2026-11-05 10:00 ด้วย duet",
        ] {
            assert!(page.contains(part), "{name}: {part}");
        }
        assert!(
            !page.contains("Text compare report") && !page.contains("Made "),
            "{name}"
        );
    }
    let context = by_name(outputs, "html context");
    assert!(context.contains("⋯ 3 บรรทัดที่ไม่เปลี่ยน ⋯"), "{context}");
    assert!(!context.contains("unchanged"));
    let stopped = by_name(thai_text_outputs(), "html stopped");
    assert!(stopped.contains("รายงานหยุดที่นี่: เกิน 5 แถว"), "{stopped}");
}

#[test]
fn the_patch_stays_english_in_a_thai_report_language() {
    let english = by_name(
        text_table::text_outputs(ENGLISH_STATUS, ENGLISH_DATE),
        "patch",
    );
    let thai = by_name(thai_text_outputs(), "patch");
    assert_eq!(thai, english, "a patch is a format that tools read");
    assert!(thai.starts_with("--- a/l.txt\n+++ b/r.txt\n@@ "));
}

#[test]
fn thai_table_html_and_differences_csv_read_in_thai() {
    let keyed = by_name(thai_table_outputs(), "keyed csv");
    assert!(
        keyed.starts_with("\u{feff}สถานะ,แถวซ้าย,แถวขวา,คีย์,คอลัมน์,ค่าซ้าย,ค่าขวา\r\n"),
        "{keyed}"
    );
    for part in [
        "ต่างกัน,2,3,1,v,a,A",
        "เฉพาะขวา,,4,4,(ทั้งแถว),,",
        "เฉพาะซ้าย,3,,2,(ทั้งแถว),",
    ] {
        assert!(keyed.contains(part), "{part} in\n{keyed}");
    }
    let plain = by_name(thai_table_outputs(), "plain csv");
    assert!(plain.contains("คอลัมน์เฉพาะซ้าย,,,,b,,"), "{plain}");
    assert!(plain.contains("คอลัมน์เฉพาะขวา,,,,c,,"), "{plain}");

    let html = by_name(thai_table_outputs(), "keyed html");
    assert!(html.starts_with("<!DOCTYPE html><html lang=\"th\">"));
    for part in [
        "<h1>รายงานเทียบตาราง</h1>",
        "<th>ฝั่งซ้าย</th>",
        "<th>ฝั่งขวา</th>",
        "<th>ตัวเลือก</th>",
        "<th>สร้างเมื่อ</th>",
        "<span>เหมือนกัน <b>",
        "<span>ต่างกัน <b>",
        "<span>เฉพาะซ้าย <b>",
        "<th>สถานะ</th><th>แถวซ้าย</th><th>แถวขวา</th>",
        "คีย์: id",
        "แถวแรกเป็นหัวตาราง",
        "<td>ต่างกัน</td>",
    ] {
        assert!(html.contains(part), "{part} in\n{html}");
    }
    let plain_html = by_name(thai_table_outputs(), "plain html");
    assert!(plain_html.contains("title=\"เฉพาะซ้าย\"") && plain_html.contains("title=\"เฉพาะขวา\""));
    assert!(plain_html.contains("จับคู่ตามลำดับแถว"));
    let same = by_name(thai_table_outputs(), "same html");
    assert!(same.contains("<p>ไม่มีแถวที่ต่าง</p>"), "{same}");
    for page in [&html, &plain_html, &same] {
        assert!(!page.chars().any(|c| ('\u{0E50}'..='\u{0E59}').contains(&c)));
    }
}

#[test]
fn thai_text_and_table_reports_carry_no_stray_english() {
    let mut words: Vec<&str> = TEXT_TABLE_WORDS.to_vec();
    words.extend(["Secr3t"]);
    for (name, output) in thai_text_outputs() {
        if name == "patch" {
            continue;
        }
        assert_no_stray_english(&output, &words, ALLOWED);
    }
    for (_, output) in thai_table_outputs() {
        assert_no_stray_english(&output, &words, ALLOWED);
    }
}
