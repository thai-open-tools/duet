//! Shared fixtures for the text, CSV, ZIP and workbook report tests.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;

use chrono::NaiveDate;

use super::lines::{LineDetails, LineInfo, LineOptions, SHEET_ROWS, line_details};
use super::{
    EntryKind, ItemState, ReportContext, ReportItem, ReportOptions, ReportScope, SideFacts,
    report_items,
};
use crate::cancel::CancelToken;
use crate::folder::run::{CompareMode, FolderOptions, FolderProgress, FolderTextRules};

pub fn facts(rel: &str, len: u64) -> SideFacts {
    SideFacts {
        rel: rel.into(),
        path: PathBuf::from(format!("C:\\a\\{rel}")),
        kind: EntryKind::File,
        len,
        modified: None,
    }
}

pub fn item(rel: &str, state: ItemState) -> ReportItem {
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

pub fn options() -> ReportOptions {
    ReportOptions {
        names: ["IWT".to_owned(), "PRD".to_owned()],
        title: "Check".to_owned(),
        note: String::new(),
        scope: ReportScope::Selected,
        details: true,
        context: 2,
        mask: true,
        unimportant: true,
        same: false,
    }
}

pub fn context() -> ReportContext {
    ReportContext {
        side_paths: ["C:\\a\\iwt".to_owned(), "C:\\a\\prd".to_owned()],
        criterion: "File contents (modified times not counted)".to_owned(),
        scope: "Selected files".to_owned(),
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

/// `app/db.properties` compared for real (two changed lines, one of them a password that the
/// masking hides), with its line details. The temp dirs stay alive with the result.
pub fn db_item() -> (ReportItem, LineDetails, [tempfile::TempDir; 2]) {
    compared_pair(
        "app/db.properties",
        "url=jdbc:x\nuser=svc\npassword=Secr3t-A\ntimeout=30\n",
        "url=jdbc:x\nuser=svc\npassword=Secr3t-B\ntimeout=60\n",
        false,
    )
}

/// The body lines of the `pem` pair's private key: three on the left, and the right's
/// replacement for the second one.
pub const PEM_BODY: [&str; 4] = [
    "MIIEowIBAAKCAQEA0Z3VS5JJcds3xfn",
    "q5x/AbC+defLeftBody0001==",
    "k9Jz3mN8pQ7rT2vW6yX1aB4c",
    "q5x/AbC+defRightBody002==",
];

/// `key.pem` holding a private key whose second body line differs, compared for real, with its
/// line rows and Changed lines sheet rows (masking on).
pub fn pem_item() -> (ReportItem, LineDetails, [tempfile::TempDir; 2]) {
    let key = |body: &str| {
        format!(
            "-----BEGIN RSA PRIVATE KEY-----\n{}\n{body}\n{}\n-----END RSA PRIVATE KEY-----\n",
            PEM_BODY[0], PEM_BODY[2]
        )
    };
    compared_pair("key.pem", &key(PEM_BODY[1]), &key(PEM_BODY[3]), true)
}

fn compared_pair(
    rel: &str,
    left: &str,
    right: &str,
    sheet: bool,
) -> (ReportItem, LineDetails, [tempfile::TempDir; 2]) {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for (root, text) in [(l.path(), left), (r.path(), right)] {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
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
    let items = report_items(&cmp, &nodes, l.path(), r.path(), None);
    assert_eq!(items.len(), 1);
    let lines = line_details(
        &items,
        &FolderTextRules::default(),
        &LineOptions {
            mask: true,
            sheet,
            rows: true,
            context: 2,
            file_rows: 300,
            total_rows: 40_000,
            sheet_rows: SHEET_ROWS,
        },
        &AtomicUsize::new(0),
        &CancelToken::new(),
    );
    (items.into_iter().next().unwrap(), lines, [l, r])
}

/// The entries of a stored ZIP in central-directory order, read as an unzip tool reads them:
/// the end record, then each central header, then its local header. Checks every signature,
/// that each entry is stored, and its CRC.
pub fn read_zip(zip: &[u8]) -> Vec<(String, Vec<u8>)> {
    let u16_at = |at: usize| usize::from(u16::from_le_bytes([zip[at], zip[at + 1]]));
    let u32_at = |at: usize| u32::from_le_bytes(zip[at..at + 4].try_into().unwrap()) as usize;
    let end = zip.len() - 22;
    assert_eq!(u32_at(end), 0x0605_4b50, "end record");
    let count = u16_at(end + 10);
    assert_eq!(u16_at(end + 8), count, "entries on this disk");
    let (size, mut at) = (u32_at(end + 12), u32_at(end + 16));
    assert_eq!(
        at + size,
        end,
        "the central directory ends at the end record"
    );
    let mut out = Vec::new();
    for _ in 0..count {
        assert_eq!(u32_at(at), 0x0201_4b50, "central header");
        assert_eq!(u16_at(at + 10), 0, "stored");
        let (crc, len, name_len, local) = (
            u32_at(at + 16),
            u32_at(at + 24),
            u16_at(at + 28),
            u32_at(at + 42),
        );
        let name = String::from_utf8(zip[at + 46..at + 46 + name_len].to_vec()).unwrap();
        assert_eq!(u32_at(local), 0x0403_4b50, "local header of {name}");
        assert_eq!(u32_at(local + 14), crc, "local CRC of {name}");
        let data = local + 30 + u16_at(local + 26) + u16_at(local + 28);
        let bytes = zip[data..data + len].to_vec();
        assert_eq!(crc32fast::hash(&bytes) as usize, crc, "CRC of {name}");
        out.push((name, bytes));
        at += 46 + name_len + u16_at(at + 30) + u16_at(at + 32);
    }
    assert_eq!(at, end, "every central header was read");
    out
}

pub fn counted(changed: u32) -> LineInfo {
    LineInfo {
        counted: true,
        changed,
        places: 1,
        ..LineInfo::default()
    }
}
