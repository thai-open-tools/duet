use super::*;
use crate::cancel::CancelToken;
use crate::folder::classify::{Note, TextFormat, Warning};
use crate::folder::entry::EntryKind;
use crate::folder::report::lines::{
    DetailRow, LineInfo, LineOptions, RowClass, SHEET_ROWS, line_details,
};
use crate::folder::report::{ItemState, SideFacts, report_items};
use crate::folder::run::{CompareMode, FolderOptions, FolderProgress, FolderTextRules};
use crate::folder::tree::Side;
use crate::text::lines::EolStyle;
use chrono::NaiveDate;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;

fn facts(rel: &str, len: u64) -> SideFacts {
    SideFacts {
        rel: rel.into(),
        path: PathBuf::from(rel),
        kind: EntryKind::File,
        len,
        modified: NaiveDate::from_ymd_opt(2026, 10, 20).and_then(|d| d.and_hms_opt(8, 5, 0)),
    }
}

fn item(rel: &str, state: ItemState) -> ReportItem {
    ReportItem {
        node: 0,
        rel: rel.into(),
        state,
        left: (state != ItemState::RightOnly).then(|| facts(rel, 10)),
        right: (state != ItemState::LeftOnly).then(|| facts(rel, 20)),
        newer: None,
        error: None,
        note: None,
        similar: false,
        odd_text: [false; 2],
        whole_folder: None,
    }
}

fn utf8(eol: EolStyle) -> TextFormat {
    TextFormat {
        eol,
        final_newline: true,
        bom: false,
        encoding: encoding_rs::UTF_8,
    }
}

fn options() -> ReportOptions {
    ReportOptions {
        names: ["IWT".to_owned(), "PRD".to_owned()],
        title: String::new(),
        note: String::new(),
        scope: crate::folder::report::ReportScope::All,
        details: true,
        context: 2,
        mask: true,
        unimportant: true,
        same: false,
    }
}

fn context() -> ReportContext {
    ReportContext {
        side_paths: ["C:\\a\\iwt".to_owned(), "C:\\a\\prd".to_owned()],
        criterion: "File contents (modified times not counted)".to_owned(),
        scope: "Every file".to_owned(),
        include: String::new(),
        exclude: String::new(),
        rules: None,
        generated: NaiveDate::from_ymd_opt(2026, 10, 24)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap(),
        generated_utc: "2026-10-24T02:30:00Z".to_owned(),
        version: "9.9.9".to_owned(),
        reads_contents: true,
        tab_width: 4,
        invisible_scan: true,
    }
}

fn html(items: &[ReportItem], lines: Option<&LineDetails>, options: &ReportOptions) -> String {
    folder_html(items, lines, options, &context())
}

fn pos(page: &str, needle: &str) -> usize {
    page.find(needle)
        .unwrap_or_else(|| panic!("missing {needle:?}"))
}

fn info_with(rows: Vec<DetailRow>, changed: u32) -> LineInfo {
    LineInfo {
        counted: true,
        changed,
        places: 1,
        rows,
        ..LineInfo::default()
    }
}

fn line(class: RowClass, l: Option<(u32, &str)>, r: Option<(u32, &str)>) -> DetailRow {
    DetailRow::Line {
        class,
        left: l.map(|(n, s)| (n, s.to_owned())),
        right: r.map(|(n, s)| (n, s.to_owned())),
        marks: None,
    }
}

fn details(file: &str, info: LineInfo) -> LineDetails {
    let mut out = LineDetails::default();
    out.files.insert(file.to_owned(), info);
    out
}

#[test]
fn the_html_report_reads_in_order_and_names_its_sides() {
    let mut script = item("bin/run.sh", ItemState::Different);
    script.note = Some(Note {
        warning: Some(Warning::Script),
        ..Note::default()
    });
    let mut odd = item("app/limit.properties", ItemState::Different);
    odd.note = Some(Note {
        warning: Some(Warning::OddSpace),
        ..Note::default()
    });
    let mut case = item("app/Config.xml", ItemState::Different);
    case.right.as_mut().unwrap().rel = "app/config.xml".into();
    let mut crlf = item("readme.txt", ItemState::Same);
    crlf.similar = true;
    crlf.note = Some(Note {
        formats: Some([utf8(EolStyle::CrLf), utf8(EolStyle::Lf)]),
        ..Note::default()
    });
    let items = [
        script,
        odd,
        case,
        crlf,
        item("docs/a.txt", ItemState::LeftOnly),
    ];
    let mut opts = options();
    opts.title = "Check before deploy".to_owned();
    let page = html(&items, None, &opts);

    assert!(page.starts_with("<!DOCTYPE html><html lang=\"en\">"));
    assert!(page.contains("<h1>Check before deploy</h1>"));
    assert!(page.contains("<title>Check before deploy — folder compare report</title>"));
    let order = [
        "<h2>Summary</h2>",
        "<h2>⚠ Needs a closer look",
        "<h2>By folder",
        "<h2>Files",
        "<h2>Differs only in unimportant parts",
        "<h2>Legend</h2>",
    ];
    let at: Vec<usize> = order.iter().map(|h| pos(&page, h)).collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "order: {at:?}");
    assert!(page.contains("<span class=\"tag\">A</span><b>IWT</b>"));
    assert!(page.contains("<span class=\"tag\">B</span><b>PRD</b>"));
    let before_legend = &page[..pos(&page, "<h2>Legend</h2>")];
    assert!(!before_legend.contains("Left:") && !before_legend.contains("Right:"));
    assert!(page.contains("duet · folder compare report"));
    assert!(page.contains("2026-10-24 09:30"));
    assert!(page.contains("Made with duet 9.9.9"));
}

#[test]
fn the_default_title_names_both_sides() {
    let page = html(&[item("a.txt", ItemState::Same)], None, &options());
    assert!(page.contains("<h1>IWT compared with PRD</h1>"));
}

#[test]
fn kpis_match_the_counts() {
    let mut unimportant = item("u.txt", ItemState::Same);
    unimportant.similar = true;
    let mut items: Vec<ReportItem> = Vec::new();
    for i in 0..4 {
        items.push(item(&format!("d{i}.txt"), ItemState::Different));
    }
    for i in 0..2 {
        items.push(item(&format!("l{i}.txt"), ItemState::LeftOnly));
    }
    items.push(item("r.txt", ItemState::RightOnly));
    items.push(item("s.txt", ItemState::Same));
    items.push(unimportant);
    let page = html(&items, None, &options());
    let values: Vec<&str> = page
        .split("<div class=\"v\">")
        .skip(1)
        .map(|rest| rest.split('<').next().unwrap())
        .collect();
    assert_eq!(values, ["9", "4", "2", "1", "2"]);
    assert!(page.contains("<div class=\"p\">Mismatched 7 (78%)</div>"));
    assert!(page.contains("Including 1 that differ only in unimportant parts"));
    assert!(page.contains("Only in IWT"));
    assert!(page.contains("Not in PRD"));
    assert!(page.contains(
        "<b>7</b> of 9 files do not match: 4 with different contents, 2 only in IWT, 1 only in PRD"
    ));
    assert!(page.contains("1 file differs only in the file format"));
}

#[test]
fn nothing_mismatched_says_so() {
    let page = html(&[item("a.txt", ItemState::Same)], None, &options());
    assert!(page.contains("Every file matches: nothing to check."));
    assert!(!page.contains("Needs a closer look"));
    assert!(!page.contains("<h2>By folder"));
    assert!(page.contains("Contents match"));
}

#[test]
fn the_attention_section_lists_each_kind_with_links() {
    let mut script = item("bin/run.sh", ItemState::Different);
    script.note = Some(Note {
        warning: Some(Warning::Script),
        ..Note::default()
    });
    let mut odd = item("app/limit.properties", ItemState::Different);
    odd.note = Some(Note {
        warning: Some(Warning::OddSpace),
        ..Note::default()
    });
    let mut case = item("app/Config.xml", ItemState::Different);
    case.right.as_mut().unwrap().rel = "app/config.xml".into();
    let page = html(&[script, odd, case], None, &options());

    let start = pos(&page, "<h2>⚠ Needs a closer look");
    let end = pos(&page, "<h2>By folder");
    let section = &page[start..end];
    assert!(section.contains("<small>3 items</small>"));
    for title in [
        "Linux scripts with CRLF or a BOM — 1 file",
        "Differs only in the kind of whitespace or invisible characters — 1 file",
        "File names that differ only in case — 1 file",
    ] {
        assert!(section.contains(title), "{title}");
    }
    assert_eq!(section.matches("href=\"#f").count(), 3);
    assert!(section.contains("(IWT: Config.xml · PRD: config.xml)"));
    // Every link points at a card that exists.
    for href in section.split("href=\"#").skip(1) {
        let id = href.split('"').next().unwrap();
        assert!(page.contains(&format!("id=\"{id}\"")), "no card {id}");
    }
    assert!(page.contains("⚠ <b>3</b> items need a closer look: see the next section"));
}

#[test]
fn file_cards_show_lines_newer_side_and_masks() {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    fs::create_dir(l.path().join("app")).unwrap();
    fs::create_dir(r.path().join("app")).unwrap();
    fs::write(
        l.path().join("app/db.properties"),
        "url=jdbc:x\nuser=svc\npassword=Secr3t-A\ntimeout=30\n",
    )
    .unwrap();
    fs::write(
        r.path().join("app/db.properties"),
        "url=jdbc:x\nuser=svc\npassword=Secr3t-B\ntimeout=60\n",
    )
    .unwrap();
    let cmp = crate::folder::run::compare_folders(
        l.path(),
        r.path(),
        &FolderOptions {
            mode: CompareMode::Content,
            ..FolderOptions::default()
        },
        &FolderProgress::default(),
        &CancelToken::new(),
    )
    .unwrap();
    let nodes: Vec<u32> = (0..cmp.nodes.len() as u32)
        .filter(|&n| !cmp.nodes[n as usize].is_dir)
        .collect();
    let mut items = report_items(&cmp, &nodes, l.path(), r.path(), None);
    items[0].newer = Some(Side::Right);
    let lines = line_details(
        &items,
        &FolderTextRules::default(),
        &LineOptions {
            mask: true,
            sheet: false,
            rows: true,
            context: 2,
            file_rows: 300,
            total_rows: 40_000,
            sheet_rows: SHEET_ROWS,
        },
        &AtomicUsize::new(0),
        &CancelToken::new(),
    );
    let page = html(&items, Some(&lines), &options());
    assert!(page.contains("Lines: 2 changed"));
    assert!(page.contains("PRD was modified later"));
    assert!(page.contains("<th colspan=\"2\">A · IWT</th>"));
    assert!(page.contains("<th colspan=\"2\">B · PRD</th>"));
    assert!(!page.contains("Secr3t-A") && !page.contains("Secr3t-B"));
    // The changed characters of a pair are wrapped in <span class="ch">.
    let plain = page
        .replace("<span class=\"ch\">", "")
        .replace("</span>", "");
    assert!(plain.contains("(hidden #d397)") && plain.contains("(hidden #d097)"));
    assert!(page.contains("<tr class=\"mod\">"));
    assert!(!page.contains("Line compare"), "no rules text was given");
    assert!(page.contains("<b>Values that look like passwords or keys are hidden</b> (2 lines)"));
    assert!(page.contains("IWT: 49 B · modified"));
}

#[test]
fn file_content_and_names_are_escaped() {
    let name = "a&b<c>.txt";
    let items = [item(name, ItemState::Different)];
    let lines = details(
        name,
        info_with(
            vec![line(
                RowClass::Mod,
                Some((1, "<script>alert(1)</script>")),
                Some((1, "x \"q\" & y")),
            )],
            1,
        ),
    );
    let mut opts = options();
    opts.title = "<b>t</b>".to_owned();
    opts.note = "n & <i>".to_owned();
    let page = html(&items, Some(&lines), &opts);
    assert!(page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(page.contains("a&amp;b&lt;c&gt;.txt"));
    assert!(!page.contains("a&b<c>"));
    assert!(page.contains("<h1>&lt;b&gt;t&lt;/b&gt;</h1>"));
    assert!(page.contains("n &amp; &lt;i&gt;"));
    assert_eq!(page.matches("<script").count(), 1, "only our own script");
}

#[test]
fn rows_use_their_class_and_a_placeholder_for_a_missing_side() {
    let name = "a.txt";
    let rows = vec![
        line(RowClass::Del, Some((1, "only left")), None),
        DetailRow::Skip(1_200),
        line(RowClass::Ins, None, Some((9, "only right"))),
        DetailRow::More(7),
    ];
    let lines = details(name, info_with(rows, 0));
    let page = html(
        &[item(name, ItemState::Different)],
        Some(&lines),
        &options(),
    );
    assert!(page.contains(
        "<tr class=\"del\"><td class=\"ln\">1</td><td class=\"l\">only left</td><td class=\"ln\"></td><td class=\"ph\"></td></tr>"
    ));
    assert!(page.contains(
        "<tr class=\"ins\"><td class=\"ln\"></td><td class=\"ph\"></td><td class=\"ln\">9</td><td class=\"r\">only right</td></tr>"
    ));
    assert!(page.contains("⋯ 1,200 same lines ⋯"));
    assert!(page.contains("… 7 more rows: open this file in duet to see them all"));
}

#[test]
fn a_whole_one_sided_folder_is_one_line() {
    let mut items: Vec<ReportItem> = (0..4)
        .map(|i| {
            let mut it = item(&format!("legacy/f{i}.txt"), ItemState::LeftOnly);
            it.whole_folder = Some("legacy".into());
            it
        })
        .collect();
    items.push(item("top.txt", ItemState::LeftOnly));
    let page = html(&items, None, &options());
    let at = pos(
        &page,
        "legacy/</span><span class=\"pill left\"><i></i>The whole folder is missing in PRD</span>",
    );
    assert!(page[at..].contains("4 files"));
    assert!(page.contains("<summary>File list</summary>"));
    // The file inside is listed by its path below the folder; the top file stands on its own.
    assert!(page.contains("f0.txt <span class=\"sz\">10 B</span>"));
    assert!(page.contains("<span class=\"mono\">📁 (top level)</span>"));
    assert!(page.contains("top.txt <span class=\"sz\">10 B</span>"));
    assert!(page.contains("◀ Only in IWT (not in PRD) — 5 files"));
}

#[test]
fn same_files_are_listed_only_when_asked() {
    let items = [
        item("same.cfg", ItemState::Same),
        item("x.cfg", ItemState::Different),
    ];
    let mut opts = options();
    let without = html(&items, None, &opts);
    assert!(!without.contains("same.cfg"));
    opts.same = true;
    let with = html(&items, None, &opts);
    assert!(with.contains("same.cfg"));
    assert!(with.contains("= Same — 1 file"));
}

#[test]
fn the_unimportant_section_names_the_reason() {
    let mut crlf = item("readme.txt", ItemState::Same);
    crlf.similar = true;
    crlf.note = Some(Note {
        formats: Some([utf8(EolStyle::CrLf), utf8(EolStyle::Lf)]),
        ..Note::default()
    });
    let items = [crlf, item("x.cfg", ItemState::Different)];
    let page = html(&items, None, &options());
    let at = pos(&page, "<h2>Differs only in unimportant parts");
    let section = &page[at..pos(&page, "<h2>Legend</h2>")];
    assert!(section.contains("counted as Same · 1 file"));
    assert!(section.contains("<summary>Show the list</summary>"));
    assert!(section.contains("Only the file format differs (EOL CRLF / LF) — every line matches"));
    let mut opts = options();
    opts.unimportant = false;
    assert!(!html(&items, None, &opts).contains("<h2>Differs only in unimportant parts"));
}

#[test]
fn details_off_keeps_the_counts() {
    let name = "a.txt";
    let lines = details(name, info_with(Vec::new(), 2));
    let mut opts = options();
    opts.details = false;
    let page = html(&[item(name, ItemState::Different)], Some(&lines), &opts);
    assert!(!page.contains("<table class=\"dt\">"));
    assert!(page.contains("Lines: 2 changed"));
    assert!(page.contains("Differing lines were not requested"));
}

#[test]
fn a_file_without_lines_explains_why() {
    let mut case = item("Config.xml", ItemState::Different);
    case.right.as_mut().unwrap().rel = "config.xml".into();
    case.note = Some(Note {
        warning: Some(Warning::CaseOnly),
        ..Note::default()
    });
    let mut format = item("f.txt", ItemState::Different);
    format.note = Some(Note {
        formats: Some([utf8(EolStyle::CrLf), utf8(EolStyle::Lf)]),
        ..Note::default()
    });
    let mut broken = item("b.txt", ItemState::Different);
    broken.error = Some("denied".into());
    let page = html(&[case, format, broken], None, &options());
    assert!(page.contains("Same contents; the names differ only in case: Config.xml / config.xml"));
    assert!(
        page.contains("Every line matches; only the file format differs — IWT: CRLF (Windows)")
    );
    assert!(page.contains("Could not read the file: the contents were not compared."));

    let mut quick = context();
    quick.criterion = crate::folder::report::criterion_text(CompareMode::Quick).to_owned();
    quick.reads_contents = false;
    let page = folder_html(
        &[item("q.txt", ItemState::Different)],
        None,
        &options(),
        &quick,
    );
    assert!(page.contains("Compared by time and size (contents not read)"));
    quick.criterion = crate::folder::report::criterion_text(CompareMode::Size).to_owned();
    let page = folder_html(
        &[item("q.txt", ItemState::Different)],
        None,
        &options(),
        &quick,
    );
    assert!(page.contains("Compared by size (contents not read)"));
}

#[test]
fn folder_anchors_are_percent_encoded_and_match_their_links() {
    let items = [
        item("my docs/a.txt", ItemState::Different),
        item("zeta/b.txt", ItemState::Different),
        item("alpha/c.txt", ItemState::Different),
    ];
    let page = html(&items, None, &options());
    assert!(page.contains("href=\"#g-my%20docs\""));
    assert!(page.contains("id=\"g-my%20docs\""));
}

#[test]
fn the_page_says_when_the_invisible_scan_did_not_run() {
    let mut ctx = context();
    ctx.invisible_scan = false;
    let page = folder_html(
        &[item("a.txt", ItemState::Different)],
        None,
        &options(),
        &ctx,
    );
    // I-1: PF12's sentence, the same in every format.
    assert!(
        page.contains(
            "Invisible characters: <b>not scanned</b> (run the scan in the folder tab first)"
        ),
        "{page}"
    );
    let page = html(&[item("a.txt", ItemState::Different)], None, &options());
    assert!(!page.contains("not scanned"));
}

#[test]
fn the_report_makes_no_external_requests() {
    let name = "a.txt";
    let lines = details(
        name,
        info_with(vec![line(RowClass::Mod, Some((1, "a")), Some((1, "b")))], 1),
    );
    let mut ctx = context();
    ctx.rules = Some("Every character counts".to_owned());
    let mut opts = options();
    opts.same = true;
    let page = folder_html(
        &[
            item(name, ItemState::Different),
            item("s.txt", ItemState::Same),
            item("l.txt", ItemState::LeftOnly),
        ],
        Some(&lines),
        &opts,
        &ctx,
    );
    for banned in [
        "http:", "https:", "//", "src=", "@import", "url(", "<link", "<iframe", "<img", "<object",
    ] {
        assert!(!page.contains(banned), "found {banned:?}");
    }
    assert_eq!(page.matches("<script").count(), 1);
    assert!(page.contains("works offline; no data leaves this computer"));
    assert!(page.contains("Line compare: <b>Every character counts</b>"));
}

#[test]
fn the_legend_explains_the_masking() {
    let lines = LineDetails {
        masked_lines: 3,
        ..LineDetails::default()
    };
    let page = html(
        &[item("a.txt", ItemState::Different)],
        Some(&lines),
        &options(),
    );
    let legend = &page[pos(&page, "<h2>Legend</h2>")..];
    assert!(legend.contains("•••••• (hidden #xxxx)"));
    assert!(legend.contains("different fingerprints mean different values"));
    // C-1 (K3) and M-5: private keys and YAML block values are hidden; other secrets that
    // span lines are not.
    assert!(legend.contains(
        "<dt>Private keys</dt><dd>Each line of a -----BEGIN … PRIVATE KEY----- block is hidden with its own fingerprint; the BEGIN and END lines stay</dd>"
    ));
    assert!(legend.contains(
        "<dt>Multi-line values</dt><dd>Each line of a YAML block value under a password or key name (password: |) is hidden the same way</dd>"
    ));
    assert!(
        legend
            .contains("<dt>Not hidden</dt><dd>Other secrets that span lines are not detected</dd>")
    );
    assert!(!legend.contains("private-key blocks) are not detected"));
    let mut opts = options();
    opts.mask = false;
    let page = html(&[item("a.txt", ItemState::Different)], Some(&lines), &opts);
    assert!(!page.contains("hidden #xxxx"));
    assert!(!page.contains("look like passwords"));
}
