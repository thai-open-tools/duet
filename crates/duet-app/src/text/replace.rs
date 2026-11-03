//! Replacing text: `replace_on`, the recorded edit of one side that typing, pastes, block copies
//! and replacements go through, and Replace and Replace all in the find bar. Replace one edits
//! the current match; Replace all plans each side's edit off the UI thread and applies it only
//! to the pair it was made from.

use std::ops::Range;

use duet_core::text::lines::LineIndex;
use gpui_kit::{Context, SharedString};

use crate::format::count_of;
use crate::text::caret::Caret;
use crate::text::editing::WAIT_NOTICE;
use crate::text::find::{FindScope, Matcher};
use crate::text::undo::Edit;
use crate::text::view::TextView;

/// What a Replace all or Reformat says when the pair changed before its plan landed.
pub(crate) const REPLACE_STOPPED: &str =
    "The text changed while replacing, so nothing was replaced. Press Replace all again.";
pub(crate) const REFORMAT_STOPPED: &str =
    "The text changed while reformatting, so nothing was changed. Press Reformat again.";

/// The pair a Replace all or Reformat plan was made from: the buffer versions, the F9 count and
/// the compare it belongs to. An untouched pair has no buffer versions, so the versions alone
/// would let a plan of one file land on the other after F9, or on a reloaded pair (C1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PairStamp {
    versions: [Option<u64>; 2],
    swaps: u64,
    epoch: u64,
}

/// "left" or "right".
pub(crate) fn side_word(left: bool) -> &'static str {
    if left { "left" } else { "right" }
}

/// What Replace all does to one side: how many matches, and the one edit that makes them
/// (None when no line changes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacePlan {
    pub matches: usize,
    pub edit: Option<(Range<usize>, String)>,
}

/// Replaces every match in `text` (whose lines `lines` index). The edit covers only the span
/// from the first changed byte to the last, so one undo step holds just that.
pub fn plan_replace_all(
    text: &str,
    lines: &LineIndex,
    matcher: &Matcher,
    with: &str,
) -> ReplacePlan {
    let mut plan = ReplacePlan {
        matches: 0,
        edit: None,
    };
    let (mut from, mut upto): (Option<usize>, usize) = (None, 0);
    // The new text from the first changed line on, cut after the last changed one.
    let mut out = String::new();
    let mut keep = 0;
    for line in 0..lines.len() {
        let span = lines.span_with_eol(line);
        let content_len = lines.line_bytes(text.as_bytes(), line).len();
        let content = &text[span.start..span.start + content_len];
        let eol = &text[span.start + content_len..span.end];
        let (new, count) = matcher.replace_all_in_line(content, with);
        plan.matches += count;
        let changed = new != content;
        if from.is_none() && !changed {
            continue;
        }
        from.get_or_insert(span.start);
        out.push_str(&new);
        if changed {
            keep = out.len();
            upto = span.start + content_len;
        }
        out.push_str(eol);
    }
    let Some(from) = from else {
        return plan;
    };
    out.truncate(keep);
    plan.edit = Some(trim_common(text, from..upto, out));
    plan
}

/// Narrows `range` of `text` and its replacement `new` past what they share at both ends.
fn trim_common(text: &str, range: Range<usize>, new: String) -> (Range<usize>, String) {
    let old = &text[range.clone()];
    let mut front = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !(old.is_char_boundary(front) && new.is_char_boundary(front)) {
        front -= 1;
    }
    let room = old.len().min(new.len()) - front;
    let mut back = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    while !(old.is_char_boundary(old.len() - back) && new.is_char_boundary(new.len() - back)) {
        back -= 1;
    }
    (
        range.start + front..range.end - back,
        new[front..new.len() - back].to_owned(),
    )
}

impl TextView {
    /// The pair on screen, for a plan to be checked against when it lands.
    pub(crate) fn pair_stamp(&self) -> Option<PairStamp> {
        Some(PairStamp {
            versions: self.model.as_ref()?.versions(),
            swaps: self.syntax_swaps,
            epoch: self.model_epoch,
        })
    }

    /// F9, a reload or another pair: a Replace all or Reformat still being planned was made
    /// from the text that was on screen, so it stops and the notice says nothing changed.
    pub(crate) fn stop_plans(&mut self) {
        if self
            .find
            .as_mut()
            .and_then(|bar| bar.replacing.take())
            .is_some()
        {
            self.notice = Some(REPLACE_STOPPED.into());
        }
        if self.reformatting.take().is_some() {
            self.notice = Some(REFORMAT_STOPPED.into());
        }
    }

    /// Applies each side's planned edit as one undo step, keeping the caret and the scroll.
    /// Returns the sides that took theirs and the first one that could not (the rest are not
    /// tried).
    pub(crate) fn apply_plan_edits(
        &mut self,
        edits: Vec<(bool, Range<usize>, String)>,
        cx: &mut Context<Self>,
    ) -> (Vec<bool>, Option<bool>) {
        let mut done = Vec::new();
        for (left, range, text) in edits {
            self.undo.seal();
            let applied = self.replace_keeping_view(left, range, &text, cx);
            self.undo.seal();
            if !applied {
                return (done, Some(left));
            }
            done.push(left);
        }
        (done, None)
    }

    /// Replaces `range` on one side as a user edit: recorded for undo, caret after the text.
    /// False (with a notice) when the side cannot be edited. An empty edit changes nothing,
    /// so the IME's empty updates never mark the file modified.
    pub fn replace_on(
        &mut self,
        left: bool,
        range: Range<usize>,
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        self.replace_edit(left, range, text, false, cx)
    }

    /// `replace_on` for an edit the user did not type (replace all): the caret stays on its
    /// text, shifted by the change, and the view does not scroll to it.
    pub(crate) fn replace_keeping_view(
        &mut self,
        left: bool,
        range: Range<usize>,
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        self.replace_edit(left, range, text, true, cx)
    }

    fn replace_edit(
        &mut self,
        left: bool,
        range: Range<usize>,
        text: &str,
        keep_view: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        self.composing = None;
        if range.is_empty() && text.is_empty() {
            return true;
        }
        if self.refused_while_loading(cx) {
            return false;
        }
        let Some(model) = self.model.as_mut() else {
            return false;
        };
        if let Err(reason) = model.begin_edit(left) {
            self.notice = Some(reason.into());
            cx.notify();
            return false;
        }
        let Some(removed) = model
            .buffer(left)
            .and_then(|b| b.text().get(range.clone()))
            .map(str::to_owned)
        else {
            return false;
        };
        let caret_before = self
            .caret
            .filter(|c| c.left == left)
            .map_or(range.start, |c| c.head);
        match self.apply_change(left, range.clone(), text, cx) {
            Err(reason) => {
                self.notice = Some(reason.into());
                cx.notify();
                false
            }
            Ok(_) => {
                self.undo.record(Edit {
                    left,
                    at: range.start,
                    removed,
                    inserted: text.to_owned(),
                    caret_before,
                });
                if keep_view {
                    let shift = |at: usize| match at {
                        at if at <= range.start => at,
                        at if at >= range.end => at + text.len() - range.len(),
                        _ => range.start + text.len(),
                    };
                    if let Some(caret) = &mut self.caret
                        && caret.left == left
                    {
                        caret.head = shift(caret.head);
                        caret.anchor = shift(caret.anchor);
                    }
                    self.menu_row = None;
                } else {
                    self.caret = Some(Caret::at(left, range.start + text.len()));
                    self.row_sel = None;
                    self.menu_row = None;
                    self.ensure_caret_visible(cx);
                }
                cx.notify();
                true
            }
        }
    }

    /// Replaces the current match (the first, if none is current) and moves on to the next.
    /// False, with a notice, when nothing was replaced.
    pub fn replace_one(&mut self, with: &str, cx: &mut Context<Self>) -> bool {
        let Some(bar) = self.find.as_ref() else {
            return false;
        };
        let Some(result) = &bar.result else {
            return false;
        };
        if result.error.is_some() {
            return self.say("Invalid pattern", cx);
        }
        let Some(hit) = result.hits.get(bar.current.unwrap_or(0)).cloned() else {
            return self.say("No matches to replace.", cx);
        };
        let Ok(matcher) = Matcher::new(&bar.query) else {
            return self.say("Invalid pattern", cx);
        };
        let Some(model) = self.model.as_ref() else {
            return false;
        };
        if let Err(reason) = model.editable(hit.left) {
            return self.say(reason, cx);
        }
        let line = hit.line as usize;
        let current = (line < model.line_count(hit.left))
            .then(|| model.line_text(hit.left, line).into_owned());
        let replaced = current
            .as_deref()
            .and_then(|text| matcher.replace_at(text, hit.range.clone(), with));
        let (Some(current), Some(replaced)) = (current, replaced) else {
            self.run_find(cx);
            return self.say(
                "The text here changed: the search ran again. Press Replace once more.",
                cx,
            );
        };
        if replaced == current {
            self.say(
                format!(
                    "{}, but the replacement changes nothing.",
                    count_of(1, "match", "matches")
                ),
                cx,
            );
            self.step_find(true, cx);
            return false;
        }
        // Only the match changes; the rest of the line is as it was.
        let piece = &replaced[hit.range.start..replaced.len() - (current.len() - hit.range.end)];
        let line_start = match model.buffer(hit.left) {
            Some(buffer) => buffer.line_start(line),
            None => {
                let doc = if hit.left {
                    model.left()
                } else {
                    model.right()
                };
                doc.lines().span_with_eol(line).start
            }
        };
        let at = line_start + hit.range.start;
        let (row, resume) = (hit.row, hit.range.start + piece.len());
        self.undo.seal();
        let done = self.replace_on(hit.left, at..at + hit.range.len(), piece, cx);
        self.undo.seal();
        if done {
            if let Some(bar) = &mut self.find {
                bar.resume = Some((row, hit.left, resume));
            }
            self.run_find(cx);
        }
        done
    }

    /// Replaces every match on the sides the scope names. The new text is planned off the UI
    /// thread; if the pair changed meanwhile (an edit, F9, a reload) nothing is replaced. True
    /// when the work started; the notice says what came of it.
    pub fn replace_all(&mut self, with: &str, cx: &mut Context<Self>) -> bool {
        let Some(bar) = self.find.as_ref() else {
            return false;
        };
        if bar.query.text.is_empty() {
            return self.say("No matches to replace.", cx);
        }
        let Ok(matcher) = Matcher::new(&bar.query) else {
            return self.say("Invalid pattern", cx);
        };
        let scope: FindScope = bar.query.scope;
        if self.refused_while_loading(cx) {
            return false;
        }
        let Some(model) = self.model.as_ref() else {
            return false;
        };
        let mut sides = Vec::new();
        let mut skipped = Vec::new();
        for left in [true, false] {
            if !scope.includes(left) {
                continue;
            }
            match model.editable(left) {
                Ok(()) => sides.push(left),
                Err(reason) => skipped.push((left, reason)),
            }
        }
        if sides.is_empty() {
            let reason = skipped.into_iter().next().map(|(_, reason)| reason);
            return self.say(
                reason.unwrap_or_else(|| "There is nothing to replace.".into()),
                cx,
            );
        }
        // Only a buffer ahead of its document is copied here; its lines are indexed in the
        // background (PF16).
        let snapshot = model.begin_snapshot();
        let Some(stamp) = self.pair_stamp() else {
            return false;
        };
        let (with, background) = (with.to_owned(), cx.background_executor().clone());
        let task = cx.spawn(async move |this, cx| {
            let plans = background
                .spawn(async move {
                    let (left_doc, right_doc, _) = snapshot.finish();
                    sides
                        .into_iter()
                        .map(|left| {
                            let doc = if left { &left_doc } else { &right_doc };
                            let plan = doc.text().map_or(
                                ReplacePlan {
                                    matches: 0,
                                    edit: None,
                                },
                                |text| plan_replace_all(text, doc.lines(), &matcher, &with),
                            );
                            (left, plan)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.apply_replace_all(plans, stamp, skipped, cx)
            });
        });
        if let Some(bar) = &mut self.find {
            bar.replacing = Some(task);
        }
        true
    }

    /// The UI thread's end of Replace all: one undo step per side, the caret and the scroll
    /// where they were.
    fn apply_replace_all(
        &mut self,
        plans: Vec<(bool, ReplacePlan)>,
        stamp: PairStamp,
        skipped: Vec<(bool, String)>,
        cx: &mut Context<Self>,
    ) {
        if let Some(bar) = &mut self.find {
            bar.replacing = None;
        }
        if self.pair_stamp() != Some(stamp) {
            self.say(REPLACE_STOPPED, cx);
            return;
        }
        let total: usize = plans.iter().map(|(_, plan)| plan.matches).sum();
        if total == 0 {
            self.say("No matches to replace.", cx);
            return;
        }
        if plans.iter().all(|(_, plan)| plan.edit.is_none()) {
            self.say(
                format!(
                    "{}, but the replacement changes nothing.",
                    count_of(total as u64, "match", "matches")
                ),
                cx,
            );
            return;
        }
        if self.job.is_running() {
            self.say(WAIT_NOTICE, cx);
            return;
        }
        let matches = |side: bool| -> usize {
            plans
                .iter()
                .filter(|(left, _)| *left == side)
                .map(|(_, plan)| plan.matches)
                .sum()
        };
        let edits = plans
            .iter()
            .filter_map(|(left, plan)| {
                let (range, text) = plan.edit.clone()?;
                Some((*left, range, text))
            })
            .collect();
        let (done, failed) = self.apply_plan_edits(edits, cx);
        // A side that could not take its edit is named; the other one's stays (M9).
        let mut said = match failed {
            None => format!("Replaced {}.", count_of(total as u64, "match", "matches")),
            Some(side) if done.is_empty() => format!(
                "Nothing was replaced: the {} side could not be changed.",
                side_word(side)
            ),
            Some(side) => format!(
                "Replaced {} on the {} side only; the {} side could not be changed.",
                count_of(matches(done[0]) as u64, "match", "matches"),
                side_word(done[0]),
                side_word(side)
            ),
        };
        for (left, reason) in skipped {
            said.push_str(&format!(
                " The {} side was skipped: {reason}",
                if left { "left" } else { "right" }
            ));
        }
        self.say(said, cx);
        self.run_find(cx);
    }

    /// Test hook: lands Replace all plans as if their background build had just finished.
    #[doc(hidden)]
    pub fn apply_replace_plans_for_test(
        &mut self,
        plans: Vec<(bool, ReplacePlan)>,
        cx: &mut Context<Self>,
    ) {
        let Some(stamp) = self.pair_stamp() else {
            return;
        };
        self.apply_replace_all(plans, stamp, Vec::new(), cx);
    }

    /// Shows `notice` in the status; always false, for the early returns above.
    fn say(&mut self, notice: impl Into<SharedString>, cx: &mut Context<Self>) -> bool {
        self.notice = Some(notice.into());
        cx.notify();
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::find::FindQuery;
    use duet_core::text::lines::LineIndex;

    fn plan(text: &str, find: &str, with: &str) -> ReplacePlan {
        let matcher = Matcher::new(&FindQuery {
            text: find.into(),
            match_case: true,
            ..FindQuery::default()
        })
        .unwrap();
        plan_replace_all(text, &LineIndex::new(text.as_bytes()), &matcher, with)
    }

    fn apply(text: &str, plan: &ReplacePlan) -> String {
        let (range, new) = plan.edit.clone().unwrap();
        format!("{}{new}{}", &text[..range.start], &text[range.end..])
    }

    #[test]
    fn the_plan_edits_only_the_changed_span() {
        let text = "keep\nxa x\nmid\nx\nkeep\n";
        let p = plan(text, "x", "y");
        assert_eq!(p.matches, 3);
        let (range, new) = p.edit.clone().unwrap();
        assert_eq!(&text[range.clone()], "xa x\nmid\nx");
        assert_eq!(new, "ya y\nmid\ny");
        assert_eq!(apply(text, &p), "keep\nya y\nmid\ny\nkeep\n");
    }

    #[test]
    fn the_plan_keeps_each_lines_own_break_and_trims_what_is_shared() {
        let text = "a\r\nxb\r\nc\nxb\rd";
        let p = plan(text, "x", "z");
        assert_eq!(apply(text, &p), "a\r\nzb\r\nc\nzb\rd");
        let (range, new) = p.edit.unwrap();
        assert_eq!((&text[range], new.as_str()), ("xb\r\nc\nx", "zb\r\nc\nz"));
        // A match whose replacement ends the same way narrows to the changed part.
        let p = plan("ab\n", "a", "ca");
        assert_eq!(p.edit, Some((0..0, "c".into())));
        let p = plan("ünï\n", "n", "ñ");
        assert_eq!(apply("ünï\n", &p), "üñï\n");
    }

    #[test]
    fn the_plan_counts_matches_even_when_nothing_changes() {
        let p = plan("x\nx\n", "x", "x");
        assert_eq!((p.matches, p.edit), (2, None));
        let p = plan("abc\n", "q", "z");
        assert_eq!((p.matches, p.edit), (0, None));
    }
}
