//! The right-hand overview bar: where the changes are in the whole (filtered) file.

use duet_core::text::rows::RowKind;

use crate::text::model::TextModel;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mark {
    pub top: f32,
    pub bottom: f32,
    pub kind: RowKind,
}

/// One mark per visible change block as fractions of the display height. Marks closer than
/// `min_gap` merge, so a bar a few hundred pixels high gets a bounded number of quads.
pub fn marks(model: &TextModel, min_gap: f32) -> Vec<Mark> {
    let rows = model.row_count();
    if rows == 0 {
        return Vec::new();
    }
    let mut out: Vec<Mark> = Vec::new();
    for (b, block) in model.blocks().iter().enumerate() {
        let (Some(first), Some(last)) = (
            model.visible().display_of(block.start),
            model.visible().display_of(block.end - 1),
        ) else {
            continue;
        };
        let mark = Mark {
            top: first as f32 / rows as f32,
            bottom: (last + 1) as f32 / rows as f32,
            kind: model.block_kind(b),
        };
        match out.last_mut() {
            Some(prev) if mark.top - prev.bottom < min_gap => {
                prev.bottom = mark.bottom;
                if prev.kind != mark.kind {
                    prev.kind = RowKind::Changed;
                }
            }
            _ => out.push(mark),
        }
    }
    out
}

/// The display row under a click at `fraction` (0.0 top .. 1.0 bottom) of the bar.
pub fn display_at(fraction: f32, rows: usize) -> usize {
    if rows == 0 {
        return 0;
    }
    ((fraction.clamp(0.0, 1.0) * rows as f32) as usize).min(rows - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use duet_core::cancel::CancelToken;
    use duet_core::text::diff::{DiffInput, DiffOptions, diff_lines};
    use duet_core::text::document::TextDocument;

    fn model_for(left: &str, right: &str) -> (tempfile::TempDir, TextModel) {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
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

    #[test]
    fn one_mark_per_block_with_its_kind() {
        // rows: 0 a | 1 b→B | 2-7 same | 8 i (left only) | 9 j
        let (_dir, model) = model_for(
            "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n",
            "a\nB\nc\nd\ne\nf\ng\nh\nj\n",
        );
        let marks = marks(&model, 0.0);
        assert_eq!(marks.len(), 2);
        assert_eq!(
            (marks[0].top, marks[0].bottom, marks[0].kind),
            (0.1, 0.2, RowKind::Changed)
        );
        assert_eq!(marks[1].kind, RowKind::LeftOnly);
        assert!((marks[1].top - 0.8).abs() < 1e-6);
    }

    #[test]
    fn close_marks_merge() {
        let (_dir, model) = model_for("a\nb\nc\nd\n", "A\nb\nC\nd\n");
        assert_eq!(marks(&model, 0.0).len(), 2);
        assert_eq!(marks(&model, 0.5).len(), 1);
    }

    #[test]
    fn clicks_map_to_rows_and_clamp() {
        assert_eq!(display_at(0.0, 100), 0);
        assert_eq!(display_at(0.505, 100), 50);
        assert_eq!(display_at(1.0, 100), 99);
        assert_eq!(display_at(-3.0, 100), 0);
        assert_eq!(display_at(0.5, 0), 0);
    }
}
