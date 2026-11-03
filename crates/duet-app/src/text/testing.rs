//! Test-only: a TextModel built from two strings written to a temp folder.

use duet_core::cancel::CancelToken;
use duet_core::text::diff::{DiffInput, DiffOptions, diff_lines};
use duet_core::text::document::TextDocument;

use crate::text::model::TextModel;

pub fn model_for(left: &str, right: &str) -> (tempfile::TempDir, TextModel) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, left).unwrap();
    std::fs::write(&b, right).unwrap();
    let cancel = CancelToken::new();
    let (l, r) = (
        TextDocument::open(&a, &cancel).unwrap(),
        TextDocument::open(&b, &cancel).unwrap(),
    );
    let diff = diff_lines(
        DiffInput {
            bytes: l.bytes(),
            lines: l.lines(),
        },
        DiffInput {
            bytes: r.bytes(),
            lines: r.lines(),
        },
        DiffOptions::default(),
        &cancel,
    )
    .unwrap();
    (dir, TextModel::new(l, r, diff))
}
