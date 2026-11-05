//! The edit behind a moved line (R12): one replacement that moves a single line of one side.

use std::ops::Range;

use super::TextModel;
use crate::i18n::tr;

impl TextModel {
    /// The one edit that moves line `from` of one side to just before line `to` (0..=line_count;
    /// `to == line_count` is the end). Every moved line keeps or gains a break, except that the
    /// text's last line keeps "no final break" when it had none.
    pub fn move_line_edit(
        &mut self,
        left: bool,
        from: u32,
        to: u32,
    ) -> Result<(Range<usize>, String), String> {
        self.editable(left)?;
        let count = self.line_count(left) as u32;
        if from >= count {
            return Err(tr("That line is gone.").into());
        }
        let to = to.min(count);
        if to == from || to == from + 1 {
            return Err(tr("The line is there already.").into());
        }
        self.begin_edit(left)?;
        // The lines the move shuffles: `first..last`, the moved one at one end.
        let (first, last) = if from < to {
            (from, to)
        } else {
            (to, from + 1)
        };
        let (text, _) = self.side_text(left).expect("begun");
        let offset = |line: u32| self.line_offset(left, line as usize);
        let span = offset(first)..offset(last);
        // Each line's text and its break; only the text's last line can have none.
        let lines: Vec<(&str, &str)> = (first..last)
            .map(|line| {
                let full = &text[offset(line)..offset(line + 1)];
                full.split_at(full.trim_end_matches(['\r', '\n']).len())
            })
            .collect();
        let mut order: Vec<&str> = lines.iter().map(|(line, _)| *line).collect();
        if from < to {
            order.rotate_left(1);
        } else {
            order.rotate_right(1);
        }
        // The breaks stay where they were, so the span ends as it began.
        let moved = order
            .iter()
            .zip(&lines)
            .map(|(line, (_, eol))| format!("{line}{eol}"))
            .collect();
        Ok((span, moved))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use duet_core::text::diff::LineDiff;

    use super::*;
    use crate::text::model::tests::model_for;

    /// Applies the edit `move_line_edit` returns to `text`, as the view does.
    fn moved(text: &str, from: u32, to: u32) -> (String, Range<usize>, String) {
        let (_dir, mut model) = model_for(text, "x\n");
        let (range, new) = model.move_line_edit(true, from, to).unwrap();
        let mut out = text.to_owned();
        out.replace_range(range.clone(), &new);
        (out, range, new)
    }

    #[test]
    fn moving_a_line_keeps_every_break_and_the_final_newline_state() {
        // (text, from, to, result, replaced range, replacement)
        let cases = [
            ("a\nb\nc\nd\n", 0, 3, "b\nc\na\nd\n", 0..6, "b\nc\na\n"),
            ("a\nb\nc\nd\n", 3, 0, "d\na\nb\nc\n", 0..8, "d\na\nb\nc\n"),
            ("a\nb\nc\nd\n", 1, 4, "a\nc\nd\nb\n", 2..8, "c\nd\nb\n"),
            ("a\nb\nc", 2, 0, "c\na\nb", 0..5, "c\na\nb"),
            ("a\nb\nc", 0, 3, "b\nc\na", 0..5, "b\nc\na"),
            ("a\r\nb\r\n", 1, 0, "b\r\na\r\n", 0..6, "b\r\na\r\n"),
            ("a\nb\nc\nd\n", 2, 0, "c\na\nb\nd\n", 0..6, "c\na\nb\n"),
        ];
        for (text, from, to, result, range, new) in cases {
            let got = moved(text, from, to);
            assert_eq!(
                got,
                (result.to_owned(), range, new.to_owned()),
                "{text:?}: {from} -> {to}"
            );
        }
    }

    #[test]
    fn a_line_dropped_where_it_is_or_on_a_side_that_cannot_be_edited_is_refused() {
        let (_dir, mut model) = model_for("a\nb\nc\n", "x\n");
        for to in [1, 2] {
            assert_eq!(
                model.move_line_edit(true, 1, to),
                Err("The line is there already.".to_owned())
            );
        }
        let (left, right, _) = model.sources();
        let mut blocked = TextModel::new(
            Arc::new(left.formatted("a\nb\n".into())),
            right,
            LineDiff {
                hunks: vec![],
                left_lines: 2,
                right_lines: 1,
                ..LineDiff::default()
            },
        );
        let reason = blocked.editable(true).unwrap_err();
        assert_eq!(blocked.move_line_edit(true, 0, 2), Err(reason));
    }
}
