//! The Reformat button (R16): JSON or XML re-indented, else trailing spaces trimmed, as edits on
//! both sides. The new text is worked out off the UI thread and applied only to the pair it was
//! made from, one undo step per side.

use std::ops::Range;

use duet_core::text::format::{ReformatKind, reformat};
use gpui_kit::{Context, SharedString};

use crate::text::editing::WAIT_NOTICE;
use crate::text::replace::{PairStamp, REFORMAT_STOPPED, side_word};
use crate::text::view::TextView;

/// What Reformat does to one side: the one edit that makes the new text, and how it tidied.
type Plan = Option<(Range<usize>, String, ReformatKind)>;

/// The replacement that turns `old` into `new`: only the span between their common start and
/// common end, so the undo step holds just that. None when they are equal.
fn changed_span(old: &str, new: &str) -> Option<(Range<usize>, String)> {
    let mut start = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !(old.is_char_boundary(start) && new.is_char_boundary(start)) {
        start -= 1;
    }
    let room = old.len().min(new.len()) - start;
    let mut end = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    while !(old.is_char_boundary(old.len() - end) && new.is_char_boundary(new.len() - end)) {
        end -= 1;
    }
    let range = start..old.len() - end;
    (range.len() + (new.len() - end - start) > 0)
        .then(|| (range, new[start..new.len() - end].to_owned()))
}

fn how(kind: ReformatKind) -> &'static str {
    match kind {
        ReformatKind::Json => "re-indented JSON",
        ReformatKind::Xml => "re-indented XML",
        ReformatKind::Trimmed => "trimmed trailing spaces",
    }
}

fn side_name(left: bool) -> &'static str {
    if left { "Left" } else { "Right" }
}

impl TextView {
    /// Reformats every side that can be edited. The notice says what each side came to.
    pub fn reformat_sides(&mut self, cx: &mut Context<Self>) {
        if self.refused_while_loading(cx) {
            return;
        }
        let Some(model) = self.model.as_ref() else {
            return;
        };
        let mut sides = Vec::new();
        let mut skipped = Vec::new();
        for left in [true, false] {
            match model.editable(left) {
                Ok(()) => sides.push(left),
                Err(reason) => skipped.push((left, reason)),
            }
        }
        if sides.is_empty() {
            let reason = skipped.into_iter().next().map(|(_, reason)| reason);
            self.tell(
                reason.unwrap_or_else(|| "There is nothing to reformat.".into()),
                cx,
            );
            return;
        }
        // Only a buffer ahead of its document is copied here; its lines are indexed in the
        // background (PF16).
        let snapshot = model.begin_snapshot();
        let Some(stamp) = self.pair_stamp() else {
            return;
        };
        let background = cx.background_executor().clone();
        self.reformatting = Some(cx.spawn(async move |this, cx| {
            let plans = background
                .spawn(async move {
                    let (left_doc, right_doc, _) = snapshot.finish();
                    sides
                        .into_iter()
                        .map(|left| {
                            let doc = if left { &left_doc } else { &right_doc };
                            let plan: Plan = doc.text().and_then(|text| {
                                let done = reformat(text)?;
                                let (range, with) = changed_span(text, &done.text)?;
                                Some((range, with, done.kind))
                            });
                            (left, plan)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.apply_reformat(plans, stamp, skipped, cx)
            });
        }));
    }

    fn apply_reformat(
        &mut self,
        plans: Vec<(bool, Plan)>,
        stamp: PairStamp,
        skipped: Vec<(bool, String)>,
        cx: &mut Context<Self>,
    ) {
        self.reformatting = None;
        if self.pair_stamp() != Some(stamp) {
            self.tell(REFORMAT_STOPPED, cx);
            return;
        }
        if self.job.is_running() {
            self.tell(WAIT_NOTICE, cx);
            return;
        }
        let kinds: Vec<(bool, ReformatKind)> = plans
            .iter()
            .filter_map(|(left, plan)| Some((*left, plan.as_ref()?.2)))
            .collect();
        let edits = plans
            .into_iter()
            .filter_map(|(left, plan)| plan.map(|(range, with, _)| (left, range, with)))
            .collect();
        let (done, failed) = self.apply_plan_edits(edits, cx);
        let parts: Vec<String> = kinds
            .iter()
            .filter(|(left, _)| done.contains(left))
            .map(|&(left, kind)| format!("{}: {}", side_name(left), how(kind)))
            .collect();
        // A side that could not take its edit is named; the other one's stays (M9).
        let mut said = match failed {
            None if parts.is_empty() => {
                "Nothing to reformat: both sides are already tidy.".to_owned()
            }
            None => format!("{}.", parts.join(" · ")),
            Some(side) if parts.is_empty() => format!(
                "Nothing was reformatted: the {} side could not be changed.",
                side_word(side)
            ),
            Some(side) => format!(
                "{}. The {} side could not be changed.",
                parts.join(" · "),
                side_word(side)
            ),
        };
        for (left, reason) in skipped {
            said.push_str(&format!(
                " The {} side was skipped: {reason}",
                side_word(left)
            ));
        }
        self.tell(said, cx);
    }

    fn tell(&mut self, notice: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.notice = Some(notice.into());
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(old: &str, new: &str) -> Option<(String, Range<usize>)> {
        let (range, with) = changed_span(old, new)?;
        let mut out = old.to_owned();
        out.replace_range(range.clone(), &with);
        Some((out, range))
    }

    #[test]
    fn the_edit_covers_only_the_changed_span() {
        assert_eq!(
            apply("a\nb  \nc\n", "a\nb\nc\n"),
            Some(("a\nb\nc\n".into(), 3..5))
        );
        assert_eq!(apply("same", "same"), None);
        assert_eq!(apply("x", ""), Some((String::new(), 0..1)));
        // Overlapping start and end of a repeated run: still one consistent edit.
        assert_eq!(apply("aaa", "aa").map(|(text, _)| text), Some("aa".into()));
        // Never cuts inside a character.
        assert_eq!(apply("é", "è").map(|(text, _)| text), Some("è".into()));
        assert_eq!(
            apply("ก\u{e49}", "ก").map(|(text, _)| text),
            Some("ก".into())
        );
    }
}
