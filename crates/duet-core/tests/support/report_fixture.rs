//! One folder compare, built by hand, that reaches most of what a folder report can say: every
//! state, every "needs a closer look" kind, a hidden value, notes, one-sided folders. Included by
//! `#[path]` in the report tests.

use std::path::{Path, PathBuf};

use chrono::{NaiveDate, NaiveDateTime};
use duet_core::folder::classify::{Note, TextFormat, Warning};
use duet_core::folder::entry::EntryKind;
use duet_core::folder::report::lines::{
    DetailRow, LineDetails, LineInfo, RowClass, SheetKind, SheetRow,
};
use duet_core::folder::report::{
    ItemState, ReportContext, ReportItem, ReportOptions, ReportScope, SideFacts,
};
use duet_core::folder::tree::Side;
use duet_core::i18n::Lang;
use duet_core::text::lines::EolStyle;

pub fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 10, day)
        .unwrap()
        .and_hms_opt(hour, minute, 0)
        .unwrap()
}

fn format(eol: EolStyle, bom: bool, encoding: &'static encoding_rs::Encoding) -> TextFormat {
    TextFormat {
        eol,
        final_newline: true,
        bom,
        encoding,
    }
}

/// Where a side's files live: `root` when a test wrote real files, else a made-up place.
fn side(root: Option<&Path>, which: &str, rel: &str, len: u64, day: u32) -> SideFacts {
    SideFacts {
        rel: rel.to_owned(),
        path: match root {
            Some(root) => root.join(which).join(rel),
            None => PathBuf::from(format!("C:\\{which}\\{}", rel.replace('/', "\\"))),
        },
        kind: EntryKind::File,
        len,
        modified: Some(at(day, 8, 5)),
    }
}

fn item(root: Option<&Path>, rel: &str, state: ItemState) -> ReportItem {
    ReportItem {
        node: 0,
        rel: rel.to_owned(),
        state,
        left: (state != ItemState::RightOnly).then(|| side(root, "left", rel, 1500, 20)),
        right: (state != ItemState::LeftOnly).then(|| side(root, "right", rel, 2_500_000, 21)),
        newer: None,
        error: None,
        note: None,
        similar: false,
        odd_text: [false; 2],
        whole_folder: None,
        moved_to: None,
    }
}

/// One more item, to add to the others.
pub fn plain(root: Option<&Path>, rel: &str, state: ItemState) -> ReportItem {
    item(root, rel, state)
}

/// The items. With a `root`, the sides' paths lead to files under it.
pub fn items(root: Option<&Path>) -> Vec<ReportItem> {
    let utf8 = encoding_rs::UTF_8;
    let mut out = Vec::new();
    let mut add = |rel: &str, state: ItemState, change: &dyn Fn(&mut ReportItem)| {
        let mut it = item(root, rel, state);
        change(&mut it);
        out.push(it);
    };
    add("bin/run.sh", ItemState::Different, &|it| {
        it.newer = Some(Side::Left);
        it.note = Some(Note {
            formats: Some([
                format(EolStyle::CrLf, true, utf8),
                format(EolStyle::Lf, false, utf8),
            ]),
            shebang: true,
            soft: false,
            warning: Some(Warning::Script),
        });
    });
    add("app/limit.properties", ItemState::Different, &|it| {
        it.note = Some(Note {
            warning: Some(Warning::OddSpace),
            ..Note::default()
        });
    });
    add("app/Config.xml", ItemState::Different, &|it| {
        it.newer = Some(Side::Right);
        it.right.as_mut().unwrap().rel = "app/config.xml".to_owned();
        it.note = Some(Note {
            warning: Some(Warning::CaseOnly),
            ..Note::default()
        });
    });
    add("app/db.properties", ItemState::Different, &|it| {
        it.newer = Some(Side::Right);
        it.odd_text = [true, true];
        it.note = Some(Note {
            formats: Some([
                format(EolStyle::Lf, false, utf8),
                format(EolStyle::CrLf, false, encoding_rs::WINDOWS_874),
            ]),
            ..Note::default()
        });
    });
    add("app/crlf.cfg", ItemState::Different, &|it| {
        it.note = Some(Note {
            formats: Some([
                format(EolStyle::Mixed, false, utf8),
                format(EolStyle::None, false, utf8),
            ]),
            ..Note::default()
        });
    });
    add("app/cr-only.cfg", ItemState::Different, &|it| {
        it.note = Some(Note {
            formats: Some([
                format(EolStyle::Cr, false, utf8),
                format(EolStyle::Lf, false, utf8),
            ]),
            soft: true,
            ..Note::default()
        });
    });
    add("app/big.dat", ItemState::Different, &|_| {});
    add("app/blob.bin", ItemState::Different, &|_| {});
    add("locked.db", ItemState::Different, &|it| {
        it.error = Some("Access is denied.".to_owned());
        it.odd_text = [false, true];
    });
    add("root.cfg", ItemState::Different, &|_| {});
    add("legacy/old/a.cfg", ItemState::LeftOnly, &|it| {
        it.whole_folder = Some("legacy/old".to_owned());
    });
    add("legacy/old/b.cfg", ItemState::LeftOnly, &|it| {
        it.whole_folder = Some("legacy/old".to_owned());
    });
    add("legacy/c.cfg", ItemState::LeftOnly, &|_| {});
    add("added/x.txt", ItemState::RightOnly, &|it| {
        it.whole_folder = Some("added".to_owned());
    });
    add("top-right.txt", ItemState::RightOnly, &|_| {});
    add("docs/readme.txt", ItemState::Same, &|it| {
        it.similar = true;
        it.note = Some(Note {
            formats: Some([
                format(EolStyle::CrLf, false, utf8),
                format(EolStyle::Lf, false, utf8),
            ]),
            ..Note::default()
        });
    });
    add("docs/rules.txt", ItemState::Same, &|it| {
        it.similar = true;
        it.note = Some(Note {
            soft: true,
            ..Note::default()
        });
    });
    add("docs/rules-and-format.txt", ItemState::Same, &|it| {
        it.similar = true;
        it.note = Some(Note {
            formats: Some([
                format(EolStyle::Lf, true, utf8),
                format(EolStyle::Lf, false, utf8),
            ]),
            soft: true,
            ..Note::default()
        });
    });
    add("docs/bom.txt", ItemState::Same, &|it| {
        it.similar = true;
        it.note = Some(Note {
            formats: Some([
                format(EolStyle::Lf, true, utf8),
                format(EolStyle::Lf, false, utf8),
            ]),
            ..Note::default()
        });
    });
    add("docs/plain.txt", ItemState::Same, &|it| {
        it.newer = Some(Side::Right);
    });
    add("same.cfg", ItemState::Same, &|_| {});
    out
}

#[allow(clippy::single_range_in_vec_init)]
fn row(
    class: RowClass,
    left: Option<(u32, &str)>,
    right: Option<(u32, &str)>,
    marks: bool,
) -> DetailRow {
    DetailRow::Line {
        class,
        left: left.map(|(n, s)| (n, s.to_owned())),
        right: right.map(|(n, s)| (n, s.to_owned())),
        marks: marks.then(|| (vec![0..3], vec![0..4])),
    }
}

fn counted(changed: u32, left_only: u32, right_only: u32, soft: u32, places: u32) -> LineInfo {
    LineInfo {
        counted: true,
        changed,
        left_only,
        right_only,
        soft,
        places,
        ..LineInfo::default()
    }
}

/// Line details written out, English notes included: the pin hashes what they make.
pub fn lines_by_hand() -> LineDetails {
    let mut out = LineDetails {
        masked_lines: 3,
        ..LineDetails::default()
    };
    let mut db = counted(2, 1, 1, 0, 3);
    db.rows = vec![
        row(
            RowClass::Same,
            Some((1, "url=jdbc:x")),
            Some((1, "url=jdbc:x")),
            false,
        ),
        DetailRow::Skip(1),
        row(
            RowClass::Mod,
            Some((3, "password=•••••• (hidden #1a47)")),
            Some((3, "password=•••••• (hidden #2b58)")),
            true,
        ),
        row(
            RowClass::Del,
            Some((4, "only <left> & \"gone\"")),
            None,
            false,
        ),
        row(RowClass::Ins, None, Some((4, "only right")), false),
        row(
            RowClass::Soft,
            Some((5, "a b")),
            Some((5, "a\u{a0}b")),
            true,
        ),
        row(RowClass::UDel, Some((6, "x")), None, false),
        row(RowClass::UIns, None, Some((6, "y")), false),
        DetailRow::Skip(2),
        DetailRow::More(1),
    ];
    db.sheet = vec![
        SheetRow {
            kind: SheetKind::Changed,
            place: 1,
            left: Some((3, "password=•••••• (hidden #1a47)".to_owned())),
            right: Some((3, "password=•••••• (hidden #2b58)".to_owned())),
        },
        SheetRow {
            kind: SheetKind::LeftOnly,
            place: 2,
            left: Some((4, "only left".to_owned())),
            right: None,
        },
        SheetRow {
            kind: SheetKind::RightOnly,
            place: 2,
            left: None,
            right: Some((4, "only right".to_owned())),
        },
        SheetRow {
            kind: SheetKind::Soft,
            place: 1,
            left: Some((5, "a b".to_owned())),
            right: Some((5, "a\u{a0}b".to_owned())),
        },
    ];
    out.files.insert("app/db.properties".to_owned(), db);
    let mut run = counted(1, 0, 0, 0, 1);
    run.rows = vec![row(
        RowClass::Mod,
        Some((2, "echo a")),
        Some((2, "echo b")),
        true,
    )];
    out.files.insert("bin/run.sh".to_owned(), run);
    let mut limit = counted(0, 0, 0, 2, 1);
    limit.rows = vec![row(
        RowClass::Soft,
        Some((1, "k = v")),
        Some((1, "k =\u{a0}v")),
        true,
    )];
    out.files.insert("app/limit.properties".to_owned(), limit);
    out.files.insert(
        "app/big.dat".to_owned(),
        LineInfo {
            note: Some(
                "The file is larger than 8 MiB: no lines are shown (open it in duet to see them)."
                    .to_owned(),
            ),
            ..LineInfo::default()
        },
    );
    out.files.insert(
        "app/blob.bin".to_owned(),
        LineInfo {
            note: Some("Binary file: compare it in duet's hex view.".to_owned()),
            ..LineInfo::default()
        },
    );
    out
}

pub fn names() -> [String; 2] {
    ["IWT".to_owned(), "PRD".to_owned()]
}

/// Everything on: lines, hiding, the unimportant list and the same files.
pub fn options(lang: Lang) -> ReportOptions {
    ReportOptions {
        names: names(),
        title: "Check before deploy".to_owned(),
        note: "First line\nSecond line".to_owned(),
        scope: ReportScope::All,
        details: true,
        context: 2,
        mask: true,
        unimportant: true,
        same: true,
        lang,
    }
}

/// Nothing optional: no lines, no hiding, no lists, the criterion not reading contents.
pub fn bare_options(lang: Lang) -> ReportOptions {
    ReportOptions {
        title: String::new(),
        note: String::new(),
        details: false,
        mask: false,
        unimportant: false,
        same: false,
        ..options(lang)
    }
}

pub fn context() -> ReportContext {
    ReportContext {
        side_paths: ["C:\\left".to_owned(), "C:\\right".to_owned()],
        criterion: "File contents (modified times not counted)".to_owned(),
        scope: "All items".to_owned(),
        include: "*.cfg".to_owned(),
        exclude: "tmp".to_owned(),
        rules: Some("Ignore case".to_owned()),
        generated: at(24, 9, 30),
        generated_utc: "2026-10-24T02:30:00Z".to_owned(),
        version: "9.9.9".to_owned(),
        reads_contents: true,
        tab_width: 4,
        invisible_scan: true,
    }
}

pub fn bare_context() -> ReportContext {
    ReportContext {
        criterion: "File size only (contents not read)".to_owned(),
        include: String::new(),
        exclude: String::new(),
        rules: None,
        reads_contents: false,
        invisible_scan: false,
        ..context()
    }
}
