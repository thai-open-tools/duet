//! The flat list: every file on one line, with sortable columns (LocalDiff's table view).

use std::ops::Range;
use std::time::SystemTime;

use crate::folder::cells::{NAME_SIZE, side_name_color};
use crate::folder::model::SortKey;
use crate::folder::ops::row_tip;
use crate::format::{format_size, format_time};
use crate::palette::Palette;
use crate::ui::fonts::Fonts;
use crate::ui::style::{LdExt as _, SCROLLBAR_WIDTH, ld_ghost, ld_pill, ld_tip};
use duet_core::folder::classify::Warning;
use duet_core::folder::tree::{Node, Side, Status};
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::h_flex;
use gpui_kit::{
    AnyElement, ClickEvent, Context, FontWeight, Hsla, MouseButton, MouseDownEvent, SharedString,
    Window, div, prelude::*, px, transparent_black,
};

use super::*;

const PATH_MIN: f32 = 240.;
const STATUS_COL: f32 = 96.;
const SIZE_COL: f32 = 92.;
const TIME_COL: f32 = 150.;
const GAP: f32 = 8.;

/// The header buttons: id, title, the key they sort by.
const COLUMNS: [(&str, &str, SortKey); 6] = [
    ("folder-sort-path", "Path", SortKey::Path),
    ("folder-sort-status", "Status", SortKey::Status),
    ("folder-sort-left-size", "Size (left)", SortKey::LeftSize),
    ("folder-sort-right-size", "Size (right)", SortKey::RightSize),
    (
        "folder-sort-left-time",
        "Modified (left)",
        SortKey::LeftTime,
    ),
    (
        "folder-sort-right-time",
        "Modified (right)",
        SortKey::RightTime,
    ),
];

/// The status pill's words (`js:637-638`).
pub fn status_label(node: &Node) -> &'static str {
    match (&node.status, node.warning()) {
        (Status::Error(_), _) => "Unreadable",
        (_, Some(Warning::CaseOnly)) => "Case-only name ⚠",
        (_, Some(Warning::OddSpace)) => "Whitespace kind ⚠",
        (_, Some(Warning::Script)) => "Script ⚠",
        (Status::Different { .. }, _) => match node.note.as_deref() {
            Some(note) if note.encoding_differs() => "Encoding differs",
            Some(note) if note.format_differs() => format_word(note.eol_differs()),
            _ => "Different",
        },
        (Status::Similar { .. }, _) => match node.note.as_deref() {
            Some(note) if note.soft => "Unimportant",
            Some(note) => format_word(note.eol_differs()),
            None => "Unimportant",
        },
        (Status::Same, _) => "Same",
        (Status::LeftOnly, _) => "Left only",
        (Status::RightOnly, _) => "Right only",
        (Status::Pending, _) => "Comparing…",
    }
}

fn format_word(eol: bool) -> &'static str {
    if eol { "Line endings only" } else { "BOM only" }
}

/// The pill's text and background.
fn status_colors(node: &Node, p: &Palette) -> (Hsla, Hsla) {
    match (&node.status, node.warning()) {
        (Status::Error(_), _) | (_, Some(_)) => (p.del, p.bg_del),
        (Status::Different { .. }, _) => (p.modified, p.bg_mod),
        (Status::Similar { .. }, _) => (p.unimp, p.bg_unimp),
        (Status::LeftOnly, _) => (p.del, p.bg_del),
        (Status::RightOnly, _) => (p.ins, p.bg_ins),
        (Status::Same | Status::Pending, _) => (p.muted, p.panel3),
    }
}

impl FolderView {
    pub(super) fn render_flat_rows(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let palette = Palette::current(cx);
        let fonts = Fonts::current(cx);
        let focused = self.focus.is_focused(window);
        let mode = self.options.mode;
        let Some(model) = &self.model else {
            return Vec::new();
        };
        let selected = model.selected_row();
        range
            .map(|row| {
                let index = model.node_index(row);
                let node = model.node(index);
                let time_differs = model.comparison().time_differs(index);
                let newer = match node.status {
                    Status::Different { newer } | Status::Similar { newer } => newer,
                    _ => None,
                };
                let other_time =
                    time_differs && matches!(node.status, Status::Same | Status::Similar { .. });
                let (rel, label) = (node.rel.clone(), status_label(node));
                let split = rel.rfind('/').map_or(0, |at| at + 1);
                let (dir, base) = (rel[..split].to_owned(), rel[split..].to_owned());
                let (fg, bg) = status_colors(node, &palette);
                let tip: Option<SharedString> = match &node.status {
                    Status::Error(message) => Some(message.clone().into()),
                    _ => row_tip(node, time_differs, mode).map(Into::into),
                };
                let size = |left: bool| -> String {
                    if left { &node.left } else { &node.right }
                        .as_ref()
                        .map(|e| format_size(e.len))
                        .unwrap_or_default()
                };
                let time = |left: bool| -> (String, bool) {
                    let modified: Option<SystemTime> = if left { &node.left } else { &node.right }
                        .as_ref()
                        .and_then(|e| e.modified);
                    let text = modified.map(format_time).unwrap_or_default();
                    let is_newer = newer == Some(if left { Side::Left } else { Side::Right });
                    (
                        if is_newer {
                            format!("▲ {text}")
                        } else {
                            text
                        },
                        is_newer,
                    )
                };
                let is_selected = selected == Some(row);
                let number = |id: &'static str, text: String, width: f32| {
                    div()
                        .id((id, row))
                        .test_support()
                        .when(!text.is_empty(), |d| d.aria_label(text.clone()))
                        .flex_none()
                        .w(px(width))
                        .text_right()
                        .text_size(px(11.5))
                        .text_color(palette.muted)
                        .child(text)
                };
                let date = |id: &'static str, (text, is_newer): (String, bool)| {
                    div()
                        .id((id, row))
                        .test_support()
                        .when(!text.is_empty(), |d| d.aria_label(text.clone()))
                        .flex_none()
                        .w(px(TIME_COL))
                        .truncate()
                        .text_size(px(11.))
                        .text_color(if is_newer {
                            palette.accent
                        } else {
                            palette.muted
                        })
                        .when(is_newer, |d| d.font_weight(FontWeight::BOLD))
                        .when(other_time, |d| d.italic())
                        .child(text)
                };
                h_flex()
                    .id(("folder-flat-row", row))
                    .test_support()
                    .relative()
                    .h(px(ROW_HEIGHT))
                    .w_full()
                    .pr(px(SCROLLBAR_WIDTH))
                    .px(px(8.))
                    .gap(px(GAP))
                    .items_center()
                    // Always 1 px, so a selected row's content lines up with its neighbours.
                    .border_1()
                    .border_color(if is_selected && focused {
                        palette.accent
                    } else {
                        transparent_black()
                    })
                    .hover(|d| d.bg(palette.panel3))
                    .when(model.is_chosen(row), |d| d.bg(palette.sel))
                    .when(is_selected, |d| {
                        d.child(
                            div()
                                .id(("folder-selected-bar", row))
                                .test_support()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(3.))
                                .bg(palette.moved),
                        )
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.press_row(row, event.modifiers, event.position, window, cx)
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                            this.menu_press(row, window, cx)
                        }),
                    )
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.click_row(row, event.click_count(), event.modifiers(), window, cx)
                    }))
                    .child(
                        h_flex()
                            .id(("folder-flat-path", row))
                            .test_support()
                            .aria_label(rel.clone())
                            .flex_1()
                            .min_w(px(PATH_MIN))
                            .overflow_hidden()
                            .font(fonts.code.clone())
                            .text_size(px(NAME_SIZE))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(palette.dim)
                                    .child(dir),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(side_name_color(
                                        node,
                                        node.left.is_some(),
                                        &palette,
                                    ))
                                    .child(base),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .w(px(STATUS_COL))
                            .gap(px(4.))
                            .items_center()
                            .child(
                                ld_pill(label, fg, bg)
                                    .id(("folder-flat-status", row))
                                    .test_support()
                                    .aria_label(label)
                                    .when_some(tip, |d, tip| {
                                        d.tooltip(move |window, cx| ld_tip(tip.clone(), window, cx))
                                    }),
                            )
                            .when(other_time && node.status == Status::Same, |d| {
                                d.child(
                                    div()
                                        .id(("folder-flat-other-time", row))
                                        .test_support()
                                        .text_size(px(10.))
                                        .text_color(palette.moved)
                                        .child("other time"),
                                )
                            }),
                    )
                    .child(number("folder-flat-size-left", size(true), SIZE_COL))
                    .child(number("folder-flat-size-right", size(false), SIZE_COL))
                    .child(date("folder-flat-time-left", time(true)))
                    .child(date("folder-flat-time-right", time(false)))
                    .into_any_element()
            })
            .collect()
    }

    /// The flat list's column titles: buttons that sort, the sorted one marked.
    pub(super) fn render_flat_header(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (sorted, desc) = self
            .model
            .as_ref()
            .map_or((SortKey::Path, false), |m| m.sort());
        let button = |(id, title, key): (&'static str, &'static str, SortKey)| {
            let on = key == sorted;
            let text = if !on {
                title.to_owned()
            } else if desc {
                format!("{title} ▼")
            } else {
                format!("{title} ▲")
            };
            let mut button = ld_ghost(id, text, p)
                .ld_small()
                .px_0()
                .border_0()
                .text_size(px(11.5))
                .ld_click(
                    true,
                    cx.listener(move |this, _, _, cx| this.sort_by(key, cx)),
                );
            if on {
                button = button.text_color(p.accent);
            }
            button
        };
        let cell = |column: (&'static str, &'static str, SortKey)| {
            let numeric = matches!(column.2, SortKey::LeftSize | SortKey::RightSize);
            let width = match column.2 {
                SortKey::Path => None,
                SortKey::Status => Some(STATUS_COL),
                SortKey::LeftSize | SortKey::RightSize => Some(SIZE_COL),
                SortKey::LeftTime | SortKey::RightTime => Some(TIME_COL),
            };
            let wrap = h_flex().min_w_0().overflow_hidden();
            let wrap = match width {
                Some(width) => wrap.flex_none().w(px(width)),
                None => wrap.flex_1().min_w(px(PATH_MIN)),
            };
            wrap.when(numeric, |d| d.justify_end())
                .child(button(column))
        };
        h_flex()
            .id("folder-flat-header")
            .test_support()
            .flex_none()
            .h(px(26.))
            .w_full()
            .pr(px(SCROLLBAR_WIDTH))
            .px(px(8.))
            .gap(px(GAP))
            .items_center()
            .bg(p.panel2)
            .border_b_1()
            .border_color(p.line)
            .text_size(px(11.5))
            .text_color(p.muted)
            .children(COLUMNS.map(cell))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use duet_core::folder::classify::{Note, TextFormat};
    use duet_core::text::lines::EolStyle;

    fn node(status: Status, note: Option<Note>) -> Node {
        Node {
            name: "x".into(),
            rel: "x".into(),
            depth: 0,
            parent: None,
            is_dir: false,
            left: None,
            right: None,
            status,
            note: note.map(Box::new),
        }
    }

    #[test]
    fn the_pill_words_follow_localdiff() {
        let word = |n: &Node| status_label(n);
        assert_eq!(word(&node(Status::Same, None)), "Same");
        assert_eq!(
            word(&node(Status::Different { newer: None }, None)),
            "Different"
        );
        assert_eq!(word(&node(Status::LeftOnly, None)), "Left only");
        assert_eq!(word(&node(Status::RightOnly, None)), "Right only");
        assert_eq!(word(&node(Status::Error("no".into()), None)), "Unreadable");
        for (warning, text) in [
            (Warning::CaseOnly, "Case-only name ⚠"),
            (Warning::OddSpace, "Whitespace kind ⚠"),
            (Warning::Script, "Script ⚠"),
        ] {
            let note = Note {
                warning: Some(warning),
                ..Note::default()
            };
            assert_eq!(
                word(&node(Status::Different { newer: None }, Some(note))),
                text
            );
        }
        let soft = Note {
            soft: true,
            ..Note::default()
        };
        assert_eq!(
            word(&node(Status::Similar { newer: None }, Some(soft))),
            "Unimportant"
        );
        let lf = TextFormat {
            eol: EolStyle::Lf,
            final_newline: true,
            bom: false,
            encoding: encoding_rs::UTF_8,
        };
        let formats = |other: TextFormat| Note {
            formats: Some([lf, other]),
            ..Note::default()
        };
        let similar = |note| node(Status::Similar { newer: None }, Some(note));
        let crlf = TextFormat {
            eol: EolStyle::CrLf,
            ..lf
        };
        let bom = TextFormat { bom: true, ..lf };
        let thai = TextFormat {
            encoding: encoding_rs::WINDOWS_874,
            ..lf
        };
        assert_eq!(word(&similar(formats(crlf))), "Line endings only");
        assert_eq!(word(&similar(formats(bom))), "BOM only");
        assert_eq!(
            word(&node(
                Status::Different { newer: None },
                Some(formats(thai))
            )),
            "Encoding differs"
        );
        assert_eq!(
            word(&node(
                Status::Different { newer: None },
                Some(formats(crlf))
            )),
            "Line endings only",
            "with the format toggle on"
        );
    }
}
