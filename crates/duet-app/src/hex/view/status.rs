//! The hex view's file bar and status line.

use std::path::Path;

use duet_core::hex::format::format_offset;

use super::*;
use crate::format::{count_of, format_size, human_size};
use crate::i18n::{tr, tr_key, trf};
use crate::pathbar::{path_bar_click, path_input};
use crate::status::{StatusPart, Tone};
use crate::ui::style::{status_bar, status_line_box, status_notice, status_parts_row};
use gpui_kit::ClickEvent;

/// `done` of `total` as 0 to 99: a job shows 100 only by ending.
fn percent(done: &AtomicU64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    let done = done.load(std::sync::atomic::Ordering::Relaxed);
    (done.saturating_mul(100) / total).min(99)
}

/// What the file bar shows for a side: "1.5 KiB (1,536 bytes)".
fn size_text(bytes: u64) -> String {
    trf("{} ({} bytes)", &[&human_size(bytes), &format_size(bytes)])
}

impl HexView {
    /// The status line as coloured parts (see the Task 5 brief for the order).
    pub fn status_parts(&self) -> Vec<StatusPart> {
        let Some(pair) = &self.pair else {
            return Vec::new();
        };
        let (left, right) = (pair.left.len(), pair.right.len());
        let mut parts = vec![
            StatusPart::new(trf("Left: {}", &[&human_size(left)]), Tone::Muted),
            StatusPart::new(trf("Right: {}", &[&human_size(right)]), Tone::Muted),
        ];
        if left != right {
            let (more, side) = if left > right {
                (left - right, tr_key("left"))
            } else {
                (right - left, tr_key("right"))
            };
            parts.push(StatusPart::new(
                trf(
                    "Sizes differ by {} ({} is longer)",
                    &[&count_of(more, "byte", "bytes"), &tr(side)],
                ),
                Tone::Changed,
            ));
        }
        match &self.diff {
            _ if self.is_busy() => parts.push(StatusPart::new(
                trf("Comparing… {}%", &[&self.progress_percent()]),
                Tone::Muted,
            )),
            Some(diff) if diff.diff_bytes == 0 => {
                parts.push(StatusPart::new(tr("Identical byte for byte."), Tone::Muted))
            }
            Some(diff) => parts.push(StatusPart::new(
                trf(
                    "Different bytes: {} · Differences: {} ({})",
                    &[
                        &format_size(diff.diff_bytes),
                        &format_size(self.blocks.len() as u64),
                        &count_of(diff.ranges.len() as u64, "run", "runs"),
                    ],
                ),
                Tone::Changed,
            )),
            None => {}
        }
        if self.is_finding() {
            parts.push(StatusPart::new(
                trf(
                    "Searching… {}%",
                    &[&percent(&self.find_progress, self.find_total)],
                ),
                Tone::Muted,
            ));
        }
        if self.diff.as_ref().is_some_and(|d| d.truncated) {
            parts.push(StatusPart::new(tr(TRUNCATED_NOTICE), Tone::Changed));
        }
        if self.cancelled {
            parts.push(StatusPart::new(
                tr("Cancelled: the result is incomplete."),
                Tone::Changed,
            ));
        }
        if let Some(part) = self.cursor_part(pair) {
            parts.push(part);
        }
        if let Some((_, range)) = &self.selection
            && range.end - range.start > 1
        {
            parts.push(StatusPart::new(
                trf(
                    "{} selected",
                    &[&count_of(range.end - range.start, "byte", "bytes")],
                ),
                Tone::Accent,
            ));
        }
        parts
    }

    /// "Offset 0x00000010 (16) · left 00 right FF" for the cursor, else for the top row.
    fn cursor_part(&self, pair: &HexPair) -> Option<StatusPart> {
        let offset = self.cursor.map_or_else(|| self.top_byte(), |c| c.1);
        if offset >= pair.max_len() {
            return None;
        }
        let byte = |source: &HexSource| {
            source
                .bytes(offset..offset + 1)
                .first()
                .map_or("--".to_owned(), |b| format!("{b:02X}"))
        };
        Some(StatusPart::new(
            trf(
                "Offset 0x{} ({}) · left {} right {}",
                &[
                    &format_offset(offset, pair.max_len() - 1),
                    &format_size(offset),
                    &byte(&pair.left),
                    &byte(&pair.right),
                ],
            ),
            Tone::Text,
        ))
    }

    pub fn status_text(&self) -> String {
        crate::status::join_parts(&self.status_parts())
    }

    /// How much of the compare is done, 0 to 99.
    fn progress_percent(&self) -> u64 {
        let total = self.pair.as_ref().map_or(0, |p| p.common_len());
        percent(&self.progress, total)
    }

    pub(super) fn render_status(&self, p: &Palette) -> impl IntoElement {
        let line = match (&self.notice, self.pair.is_some()) {
            (Some(notice), _) => status_notice("hex-status-text", notice.clone(), p),
            (None, true) => status_parts_row("hex-status-text", &self.status_parts(), p),
            (None, false) => status_line_box("hex-status-text", "", ""),
        };
        status_bar("hex-status", p).child(line)
    }

    /// Each side's tag, path and size over its own half of the window.
    pub(super) fn render_files(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let side = |left: bool| {
            let (tag, path, size_id): (_, &Path, _) = if left {
                (tr("Left"), &self.left, "hex-left-size")
            } else {
                (tr("Right"), &self.right, "hex-right-size")
            };
            let id = if left {
                "hex-left-path"
            } else {
                "hex-right-path"
            };
            let size = self.pair.as_ref().map(|pair| {
                size_text(if left {
                    pair.left.len()
                } else {
                    pair.right.len()
                })
            });
            h_flex()
                .flex_1()
                .min_w_0()
                .gap(px(8.))
                .px(px(10.))
                .items_center()
                .child(
                    div()
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(p.accent)
                        .child(tag.to_uppercase()),
                )
                .child(match self.path_edit.as_ref().filter(|e| e.left() == left) {
                    Some(edit) => path_input(id, edit.edit(), p),
                    None => path_bar_click(
                        id,
                        path,
                        p,
                        cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.begin_path_edit(left, window, cx)
                        }),
                    )
                    .into_any_element(),
                })
                .children(size.map(|size| {
                    div()
                        .id(size_id)
                        .test_support()
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(p.dim)
                        .child(size)
                }))
        };
        h_flex()
            .id("hex-files")
            .test_support()
            .flex_none()
            .h(px(30.))
            .bg(p.panel2)
            .border_b_1()
            .border_color(p.line)
            .child(side(true))
            .child(div().flex_none().w(px(1.)).h_full().bg(p.line2))
            .child(side(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_bar_gives_both_forms_of_the_size() {
        assert_eq!(size_text(1536), "1.5 KiB (1,536 bytes)");
    }
}
