//! Manual alignment (R11, `ops:188-221`): "Align this line with a line on the other side…"
//! waits for a click on the other side, then pins the two lines together; the compare treats
//! the pair as a fixed point and diffs what lies between the pins on its own.

use duet_core::text::rules::RULES_MAX_BYTES;
use gpui_kit::Context;

use crate::i18n::{tr, trf};
use crate::text::job::RULES_SKIPPED;
use crate::text::model::DisplayRow;
use crate::text::replace::side_word;
use crate::text::view::TextView;

/// The line an alignment started from: its side and 0-based file line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AlignStart {
    pub left: bool,
    pub line: u32,
}

impl TextView {
    /// The pinned line pairs (left, right), as 0-based file lines.
    pub fn anchors(&self) -> &[(u32, u32)] {
        &self.request.rules.anchors
    }

    /// The side and line an alignment is waiting on a click for.
    pub fn aligning(&self) -> Option<(bool, u32)> {
        self.align.map(|a| (a.left, a.line))
    }

    /// The menu row's line starts an alignment; the next press on the other side finishes it.
    pub fn start_align(&mut self, cx: &mut Context<Self>) {
        let start = self.menu_row.take().and_then(|(row, left)| {
            let aligned = self.model.as_ref()?.aligned_row(row)?;
            let line = if left { aligned.left } else { aligned.right }?;
            Some(AlignStart { left, line })
        });
        let Some(start) = start else { return };
        self.align = Some(start);
        self.notice = Some(
            trf(
                "Click the line on the {} side to align with {} line {} (Esc cancels).",
                &[
                    &side_word(!start.left),
                    &side_word(start.left),
                    &(start.line + 1),
                ],
            )
            .into(),
        );
        cx.notify();
    }

    /// A press on the `left` half of display row `display` while an alignment waits: pairs that
    /// line with the one it started from. Any outcome ends the waiting.
    pub fn finish_align(&mut self, display: usize, left: bool, cx: &mut Context<Self>) {
        let Some(start) = self.align.take() else {
            return;
        };
        if left == start.left {
            self.notice = Some(tr("Click a line on the other side.").into());
            cx.notify();
            return;
        }
        let Some(DisplayRow::Row(row)) = self.model.as_ref().map(|m| m.display_row(display)) else {
            self.notice = None;
            cx.notify();
            return;
        };
        let line = self
            .model
            .as_ref()
            .and_then(|m| m.aligned_row(row))
            .and_then(|r| if left { r.left } else { r.right });
        let Some(line) = line else {
            self.notice = Some(tr("That side has no line here.").into());
            cx.notify();
            return;
        };
        let (a, b) = if start.left {
            (start.line, line)
        } else {
            (line, start.line)
        };
        // A pin is a rule: a side too large for the rules keeps it unapplied, and the note says
        // so at once (X-96). Asked before the compare, which may let a large pair's model go.
        let too_large = self.model.as_ref().is_some_and(|m| {
            m.left().bytes().len() > RULES_MAX_BYTES || m.right().bytes().len() > RULES_MAX_BYTES
        });
        // A pin that would cross the new one gives way.
        let anchors = &mut self.request.rules.anchors;
        anchors.retain(|&(x, y)| (x < a && y < b) || (x > a && y > b));
        anchors.push((a, b));
        anchors.sort_by_key(|&(x, _)| x);
        self.recompare(cx);
        self.notice = Some(if too_large {
            tr(RULES_SKIPPED).into()
        } else {
            trf(
                "Aligned left line {} with right line {}.",
                &[&(a + 1), &(b + 1)],
            )
            .into()
        });
        cx.notify();
    }

    /// An edit drops the pins past a side's new line count (R11, `js:776-779`), so "Clear
    /// manual alignments (N)" counts only the pins there are.
    pub(crate) fn drop_stale_anchors(&mut self) {
        let Some(model) = &self.model else { return };
        let (left, right) = (model.line_count(true), model.line_count(false));
        self.request
            .rules
            .anchors
            .retain(|&(a, b)| (a as usize) < left && (b as usize) < right);
    }

    /// Takes every pin away and compares again.
    pub fn clear_anchors(&mut self, cx: &mut Context<Self>) {
        self.menu_row = None;
        if self.request.rules.anchors.is_empty() {
            return;
        }
        self.request.rules.anchors.clear();
        self.recompare(cx);
        self.notice = Some(tr("Manual alignments cleared.").into());
        cx.notify();
    }
}
