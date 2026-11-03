//! The merge stage's info bar, status line and whole-result preview (R15, `js:703-781`).

use std::ops::Range;
use std::time::Duration;

use gpui_kit::component::h_flex;
use gpui_kit::{AnyElement, FontWeight, relative, uniform_list};

use super::cards::{ROW_HEIGHT, number_cell};
use super::input::eol_label;
use super::*;
use crate::format::encoding_label;
use crate::status::{StatusPart, Tone};
use crate::text::display::{DisplayOptions, display_line};
use crate::ui::fonts::{CODE_SIZE, Fonts, SMALL_SIZE};
use crate::ui::icons;
use crate::ui::style::{
    LdExt as _, ld_button, ld_icon_button, ld_tip, status_bar, status_line_box, status_parts_row,
};

/// The preview is rebuilt this long after the last change (`debounce(.., 120)`).
pub const PREVIEW_DELAY: Duration = Duration::from_millis(120);

/// What the preview shows: the assembled lines, and how many conflicts are still open in
/// them (as markers).
pub(super) struct Preview {
    lines: Vec<String>,
    unresolved: usize,
}

/// One part of the info bar: a coloured label, a name or path, and what is known about it.
struct InfoPiece {
    label: &'static str,
    name: String,
    meta: String,
    /// The name is a file name or path (drawn in the code font), not a sentence.
    code: bool,
}

/// The status bar's "Line endings differ" part, which has a tooltip.
const EOL_DIFFERS: &str = "Line endings differ";
const EOL_TIP: &str = "The result uses the left file's line endings";

impl MergeView {
    pub fn show_result(&self) -> bool {
        self.show_result
    }

    /// The "Show the whole result" toggle. Turning it on builds the preview at once.
    pub fn set_show_result(&mut self, on: bool, cx: &mut Context<Self>) {
        self.show_result = on;
        self.preview_task = None;
        self.refresh_preview(cx);
    }

    /// What the preview shows, after its delay: the assembled lines, an open conflict written
    /// out with markers.
    pub fn preview_lines(&self) -> Option<&[String]> {
        self.preview
            .as_ref()
            .map(|preview| preview.lines.as_slice())
    }

    /// Builds the preview from the session now (or drops it when it is off).
    pub(super) fn refresh_preview(&mut self, cx: &mut Context<Self>) {
        self.preview = match (self.show_result, self.session.as_ref()) {
            (true, Some(session)) => Some(Preview {
                lines: session
                    .result_lines()
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                unresolved: session.unresolved(),
            }),
            _ => None,
        };
        cx.notify();
    }

    /// A change to the result: the preview follows `PREVIEW_DELAY` after the last one (a newer
    /// change replaces the timer).
    pub(super) fn schedule_preview(&mut self, cx: &mut Context<Self>) {
        if !self.show_result || self.session.is_none() {
            return;
        }
        self.preview_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PREVIEW_DELAY).await;
            let _ = this.update(cx, |view, cx| view.refresh_preview(cx));
        }));
    }

    /// "17 lines · 2 conflicts as markers".
    fn preview_count(&self) -> Option<String> {
        let preview = self.preview.as_ref()?;
        let mut text = count_of(preview.lines.len() as u64, "line", "lines");
        if preview.unresolved > 0 {
            let open = count_of(preview.unresolved as u64, "conflict", "conflicts");
            text.push_str(&format!(" · {open} as markers"));
        }
        Some(text)
    }

    fn info_pieces(&self) -> Vec<InfoPiece> {
        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        let side = |label, text: &MergeText| InfoPiece {
            label,
            name: text.name.clone(),
            meta: format!("{} · {}", encoding_label(&text.encoding), eol_label(text)),
            code: true,
        };
        let result = match self.output.as_deref() {
            Some(path) => InfoPiece {
                label: "→ Result:",
                name: path.display().to_string(),
                meta: String::new(),
                code: true,
            },
            None => InfoPiece {
                label: "→ Result:",
                name: "chosen when saving".to_owned(),
                meta: String::new(),
                code: false,
            },
        };
        vec![
            side("Left:", session.left()),
            side("Base:", session.base()),
            side("Right:", session.right()),
            result,
        ]
    }

    /// The info bar's parts as text: "Left: left.txt UTF-8 · LF", ..., "→ Result: ...".
    pub fn info_parts(&self) -> Vec<String> {
        self.info_pieces()
            .into_iter()
            .map(|piece| {
                if piece.meta.is_empty() {
                    format!("{} {}", piece.label, piece.name)
                } else {
                    format!("{} {} {}", piece.label, piece.name, piece.meta)
                }
            })
            .collect()
    }

    /// The merge stage's status line (`updateStatus`).
    pub(super) fn merge_status_parts(&self) -> Vec<StatusPart> {
        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        let unresolved = session.unresolved();
        let (left, base, right) = (session.left(), session.base(), session.right());
        let mut parts = vec![
            StatusPart::new(
                count_of(
                    session.automatic() as u64,
                    "change merged automatically",
                    "changes merged automatically",
                ),
                Tone::RightOnly,
            ),
            StatusPart::new(
                format!(
                    "Conflicts left: {unresolved} / {}",
                    session.conflicts().len()
                ),
                if unresolved > 0 {
                    Tone::LeftOnly
                } else {
                    Tone::RightOnly
                },
            ),
        ];
        // The result's format comes from the left, else the base or the right (M-62).
        let format = session.output_format();
        let encoding = if self.utf8_agreed {
            format!("UTF-8 (instead of {})", encoding_label(&format.encoding))
        } else {
            encoding_label(&format.encoding)
        };
        parts.push(StatusPart::new(
            format!(
                "Result: {} · {encoding} · {}",
                count_of(session.result_line_count() as u64, "line", "lines"),
                format.eol.label()
            ),
            Tone::Text,
        ));
        let named = |label: &str, text: &MergeText| {
            format!(
                "{label} {} · {}",
                encoding_label(&text.encoding),
                eol_label(text)
            )
        };
        parts.push(StatusPart::new(
            format!(
                "{} | {} | {}",
                named("Left", left),
                named("Base", base),
                named("Right", right)
            ),
            Tone::Muted,
        ));
        if left.format.eol != base.format.eol || left.format.eol != right.format.eol {
            parts.push(StatusPart::new(EOL_DIFFERS, Tone::Changed));
        }
        if session.trail() {
            parts.push(StatusPart::new("Ignoring trailing whitespace", Tone::Moved));
        }
        if session.is_dirty() {
            parts.push(StatusPart::new("● Not saved", Tone::Changed));
        } else if unresolved == 0 {
            parts.push(StatusPart::new("✓ Ready to save", Tone::RightOnly));
        }
        parts
    }

    /// The sub-bar under the toolbar: what each side is and where the result goes.
    pub(super) fn render_info_bar(
        &self,
        p: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = [p.moved, p.muted, p.ins, p.text];
        let pieces = self
            .info_pieces()
            .into_iter()
            .zip(colors)
            .map(|(piece, color)| {
                h_flex()
                    .gap(px(5.))
                    .items_baseline()
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_color(color)
                            .child(piece.label),
                    )
                    .child(
                        div()
                            .when(piece.code, |d| {
                                d.font(fonts.code.clone()).text_size(px(SMALL_SIZE))
                            })
                            .text_color(if piece.code { p.text } else { p.dim })
                            .child(piece.name),
                    )
                    .when(!piece.meta.is_empty(), |d| {
                        d.child(div().text_color(p.dim).child(piece.meta))
                    })
            });
        h_flex()
            .id("merge-info")
            .test_support()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap(px(14.))
            .px(px(10.))
            .py(px(4.))
            .bg(p.panel2)
            .border_b_1()
            .border_color(p.line)
            .children(pieces)
            .child(div().flex_1())
            .child(
                ld_button("merge-save-as", "Save result to…", p)
                    .ld_small()
                    .ld_tooltip("Choose where to save the result")
                    .ld_click(
                        self.session.is_some(),
                        cx.listener(|this, _, window, cx| this.save(true, window, cx).detach()),
                    ),
            )
    }

    /// The status bar of the merge stage: the parts, then the notice (it used to head the
    /// list and made it jump when it came and went).
    pub(super) fn render_merge_status(&self, p: &Palette) -> impl IntoElement {
        let parts = self.merge_status_parts();
        let line = if parts.is_empty() {
            status_line_box("")
        } else if parts.iter().any(|part| part.text == EOL_DIFFERS) {
            // The one part with a tooltip: each part is its own piece.
            let mut row = h_flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap();
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    row = row.child(div().text_color(p.muted).child(" · "));
                }
                let piece = div()
                    .text_color(part.tone.color(p))
                    .child(part.text.clone());
                row = row.child(if part.text == EOL_DIFFERS {
                    piece
                        .id("merge-status-eol")
                        .test_support()
                        .tooltip(|window, cx| ld_tip(EOL_TIP.into(), window, cx))
                        .into_any_element()
                } else {
                    piece.into_any_element()
                });
            }
            row
        } else {
            status_parts_row(&parts, p)
        };
        let notice = self.notice.clone().map(|notice| {
            div()
                .id("merge-notice")
                .test_support()
                .flex_none()
                .text_color(p.text)
                .child(notice)
        });
        status_bar("merge-status", p).child(line).children(notice)
    }

    /// The whole-result panel beside the list (`mgOut`), when it is on.
    pub(super) fn render_preview(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let count = self.preview_count()?;
        let lines = self.preview.as_ref()?.lines.len();
        Some(
            v_flex()
                .id("merge-preview")
                .test_support()
                .flex_none()
                .w(relative(0.38))
                .min_w(px(280.))
                .h_full()
                .bg(p.panel)
                .border_l_1()
                .border_color(p.line)
                .child(
                    h_flex()
                        .flex_none()
                        .items_center()
                        .gap(px(8.))
                        .px(px(10.))
                        .py(px(5.))
                        .border_b_1()
                        .border_color(p.line)
                        .child(div().font_weight(FontWeight::BOLD).child("Whole result"))
                        .child(
                            div()
                                .id("merge-preview-count")
                                .test_support()
                                .aria_label(SharedString::from(count.clone()))
                                .text_color(p.muted)
                                .child(count),
                        )
                        .child(div().flex_1())
                        .child(
                            ld_icon_button(
                                "merge-preview-close",
                                icons::CLOSE,
                                "Close the whole result",
                                p,
                            )
                            .ld_click(
                                true,
                                cx.listener(|this, _, _, cx| this.set_show_result(false, cx)),
                            ),
                        ),
                )
                .child(
                    div().flex_1().min_h_0().child(
                        uniform_list(
                            "merge-preview-list",
                            lines,
                            cx.processor(Self::render_preview_rows),
                        )
                        .size_full(),
                    ),
                ),
        )
    }

    /// The preview's visible rows: a line number and the line, in the code font.
    fn render_preview_rows(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let p = Palette::current(cx);
        let fonts = Fonts::current(cx);
        let Some(preview) = self.preview.as_ref() else {
            return Vec::new();
        };
        range
            .filter_map(|ix| Some((ix, preview.lines.get(ix)?)))
            .map(|(ix, line)| {
                h_flex()
                    .h(px(ROW_HEIGHT))
                    .font(fonts.code.clone())
                    .text_size(px(CODE_SIZE))
                    .child(number_cell(ix as u32 + 1, p.panel2, &p))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(display_line(line, DisplayOptions::default()).text),
                    )
                    .into_any_element()
            })
            .collect()
    }
}
