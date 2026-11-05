//! A fixed text compare and a fixed table compare, and every output the text and table reports
//! write from them. Included by `#[path]` in `report_i18n.rs`. The outputs read the language of
//! the thread that calls `text_outputs` and `table_outputs`.

use duet_core::cancel::CancelToken;
use duet_core::table::compare::{CompareOptions, compare};
use duet_core::table::parse::parse_str;
use duet_core::table::report::{ReportSide, differences_csv, html_report as table_html};
use duet_core::table::{Delimiter, Table};
use duet_core::text::diff::{DiffInput, DiffOptions, LineDiff, diff_lines};
use duet_core::text::document::TextDocument;
use duet_core::text::report::{
    PatchOptions, ReportMeta, ReportMode, html_report, text_report, unified_diff,
};
use duet_core::text::rows::RowMap;

const LEFT: &str = "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota x\nkappa\n\
lambda <b> & \"q\"\nmu\nnu\nxi\nomicron\npi\n";
const RIGHT: &str = "alpha\nBETA\ndelta\nepsilon\nzeta\neta\ntheta\niota\u{a0}x\nkappa\n\
lambda <i> & \"r\"\nmu\nnu\nxi\nomicron\npi\nrho\n";

/// The documents of the text compare, and what compared them.
pub struct TextPair {
    _dir: tempfile::TempDir,
    pub left: TextDocument,
    pub right: TextDocument,
    pub diff: LineDiff,
}

pub fn text_pair(options: DiffOptions) -> TextPair {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("l.txt"), dir.path().join("r.txt"));
    std::fs::write(&a, LEFT).unwrap();
    std::fs::write(&b, RIGHT).unwrap();
    let cancel = CancelToken::new();
    let left = TextDocument::open(&a, &cancel).unwrap();
    let right = TextDocument::open(&b, &cancel).unwrap();
    let diff = diff_lines(
        DiffInput {
            bytes: left.bytes(),
            lines: left.lines(),
        },
        DiffInput {
            bytes: right.bytes(),
            lines: right.lines(),
        },
        options,
        &cancel,
    )
    .unwrap();
    TextPair {
        _dir: dir,
        left,
        right,
        diff,
    }
}

/// The `status` a view puts in the report: the caller's own words, so a fixture has one fixed
/// value for English and another for Thai.
pub fn meta<'a>(status: &'a str, date: &'a str) -> ReportMeta<'a> {
    ReportMeta {
        left: "left <1>.txt",
        right: "right & 2.txt",
        date,
        status,
        // The app says it in the report's language; a fixture does the same by the scope.
        title: if duet_core::i18n::scoped_lang() == Some(duet_core::i18n::Lang::Th) {
            "left <1>.txt กับ right & 2.txt"
        } else {
            "left <1>.txt vs right & 2.txt"
        },
    }
}

/// Every output of the text reports, as `(name, text)`.
pub fn text_outputs(status: &str, date: &str) -> Vec<(&'static str, String)> {
    let plain = text_pair(DiffOptions {
        ignore_eol: true,
        ..DiffOptions::default()
    });
    let soft = text_pair(DiffOptions {
        ignore_case: true,
        ..DiffOptions::default()
    });
    let meta = meta(status, date);
    let plain_rows = RowMap::new(&plain.diff);
    let soft_rows = RowMap::new(&soft.diff);
    let page = |pair: &TextPair, rows: &RowMap, mode, ignoring, max_rows| {
        html_report(
            &pair.left,
            &pair.right,
            rows,
            mode,
            ignoring,
            4,
            &meta,
            max_rows,
        )
    };
    vec![
        (
            "text report",
            text_report(&plain.left, &plain.right, &plain.diff, &meta),
        ),
        (
            "text report no differences",
            text_report(
                &soft.left,
                &soft.left,
                &text_pair_same(&soft.left).diff,
                &meta,
            ),
        ),
        (
            "patch",
            unified_diff(
                &plain.left,
                &plain.right,
                &plain.diff,
                &PatchOptions {
                    left_name: "l.txt",
                    right_name: "r.txt",
                    context: 3,
                },
            ),
        ),
        (
            "html context",
            page(&plain, &plain_rows, ReportMode::Context(1), false, 1000),
        ),
        (
            "html differences",
            page(&plain, &plain_rows, ReportMode::Differences, false, 1000),
        ),
        (
            "html all",
            page(&plain, &plain_rows, ReportMode::All, false, 1000),
        ),
        (
            "html soft",
            page(&soft, &soft_rows, ReportMode::All, true, 1000),
        ),
        (
            "html stopped",
            page(&plain, &plain_rows, ReportMode::All, false, 5),
        ),
    ]
}

/// A document compared with itself: no differences.
fn text_pair_same(doc: &TextDocument) -> SameDiff {
    let cancel = CancelToken::new();
    let diff = diff_lines(
        DiffInput {
            bytes: doc.bytes(),
            lines: doc.lines(),
        },
        DiffInput {
            bytes: doc.bytes(),
            lines: doc.lines(),
        },
        DiffOptions::default(),
        &cancel,
    )
    .unwrap();
    SameDiff { diff }
}

struct SameDiff {
    diff: LineDiff,
}

fn csv(text: &str) -> Table {
    parse_str(text, Delimiter::Comma)
}

fn side<'a>(table: &'a Table, name: &'a str, describe: &str) -> ReportSide<'a> {
    ReportSide {
        table,
        name,
        describe: describe.to_owned(),
    }
}

/// Every output of the table reports, as `(name, text)`.
pub fn table_outputs(describe: [&str; 2], generated: &str) -> Vec<(&'static str, String)> {
    let cancel = CancelToken::new();
    let keyed_options = CompareOptions {
        header: true,
        keys: "id".into(),
        ..Default::default()
    };
    let (kl, kr) = (
        csv("id,v\n1,a\n2,b\n3,c\n3,x\n"),
        csv("id,v\n3,c\n1,A\n4,d\n3,y\n"),
    );
    let keyed = compare(Some(&kl), Some(&kr), &keyed_options, &cancel).unwrap();
    // One-sided columns, markup in the cells, no key, rules on.
    let plain_options = CompareOptions {
        header: true,
        ..Default::default()
    };
    let (pl, pr) = (
        csv("a,b,x\n1,<2>,p\n5,& \"q\",q\n"),
        csv("a,c,x\n1,3,p\n5,& \"q\",Q\n"),
    );
    let plain = compare(Some(&pl), Some(&pr), &plain_options, &cancel).unwrap();
    // The same table twice: no differing rows.
    let same = compare(Some(&kl), Some(&kl), &keyed_options, &cancel).unwrap();
    let page = |diff, l, r, options| {
        table_html(
            diff,
            &side(l, "left <1>.csv", describe[0]),
            &side(r, "right & 2.csv", describe[1]),
            options,
            generated,
        )
    };
    vec![
        ("keyed csv", differences_csv(&keyed, &kl, &kr, true)),
        ("plain csv", differences_csv(&plain, &pl, &pr, true)),
        ("keyed html", page(&keyed, &kl, &kr, &keyed_options)),
        ("plain html", page(&plain, &pl, &pr, &plain_options)),
        ("same html", page(&same, &kl, &kl, &keyed_options)),
    ]
}
