//! The folder view's list: the column titles and one row per visible node.

use crate::folder::cells::{
    DATE_COL, MID_COL, MovedTag, NAME_SIZE, Pill, SIZE_COL, SideCell, TAG_SHARE, name_full_width,
    name_min_width, pill_fits, side_cell, side_width, status_style,
};
use crate::folder::model::FolderFilter;
use crate::folder::odd::odd_tip;
use crate::folder::ops::{pill_tip, row_tip};
use crate::format::{count_of, format_size};
use crate::i18n::{tr, trf};
use crate::palette::Palette;
use crate::ui::fonts::{CODE_SIZE, Fonts, code_advance};
use crate::ui::icons;
use crate::ui::style::{SCROLLBAR_WIDTH, hatch, ld_tip};
use duet_core::folder::tree::{Side, Status};
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::h_flex;
use gpui_kit::{
    AnyElement, ClickEvent, Context, FontWeight, MouseButton, MouseDownEvent, SharedString, Window,
    div, prelude::*, px, transparent_black,
};
use std::ops::Range;
use std::rc::Rc;

use super::*;

/// The characters of a tag that must fit beside the name, or the tag is left off.
const MOVED_TAG_MIN_CHARS: usize = 10;

/// The tag of a moved or renamed file for both layouts. `cell_width` is the cell the name and
/// the tag share when it is known, and `fits(chars)` says whether the whole name and `chars`
/// characters of tag fit in it: the name wins, so the tag is the first to go and takes only
/// what the name leaves.
pub(super) fn moved_tag_of(
    model: &FolderModel,
    index: u32,
    row: usize,
    cell_width: Option<f32>,
    fits: &dyn Fn(usize) -> bool,
    cx: &mut Context<FolderView>,
) -> Option<MovedTag> {
    let other = model.node(model.moved_partner(index)?).rel.clone();
    let text = trf("Moved ↔ {}", &[&other]);
    if let Some(cell) = cell_width {
        // The tag is cut to what is left, so it only goes when not even its first letters fit.
        let room = ((cell * TAG_SHARE - 12.) / 6.).max(0.) as usize;
        let shortest = text.chars().count().min(room).min(MOVED_TAG_MIN_CHARS);
        if !fits(shortest) {
            return None;
        }
    }
    let tip = if model.node(index).status == Status::LeftOnly {
        trf(
            "Moved or renamed: the same contents are at {} on the right. Click to go there.",
            &[&other],
        )
    } else {
        trf(
            "Moved or renamed: the same contents are at {} on the left. Click to go there.",
            &[&other],
        )
    };
    let press = Rc::new(cx.listener(move |this, _: &MouseDownEvent, window, cx| {
        this.jump_to_partner(index, cx);
        // The tag stops the press, so the row press (which takes the focus) never runs:
        // without this, Down and Enter would go on to whatever held the focus.
        window.focus(&this.focus, cx);
    }));
    Some(MovedTag {
        row,
        text: text.into(),
        tip: tip.into(),
        press,
    })
}

impl FolderView {
    pub(super) fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if self.prefs.layout == Layout::Flat {
            return self.render_flat_rows(range, window, cx);
        }
        let palette = Palette::current(cx);
        let fonts = Fonts::current(cx);
        let hatch = hatch(&palette, window);
        let focused = self.focus.is_focused(window);
        let mode = self.options.mode;
        let Some(model) = &self.model else {
            return Vec::new();
        };
        let selected = model.selected_row();
        // The list's width from the last frame; 0 before the first, when every pill shows.
        let list = f32::from(self.scroll.0.borrow().base_handle.bounds().size.width);
        let cell_width = (list > 0.).then(|| side_width(list));
        let advance = f32::from(code_advance(window, &fonts.code)) * NAME_SIZE / CODE_SIZE;
        range
            .map(|row| {
                let index = model.node_index(row);
                let node = model.node(index);
                let time_differs = model.comparison().time_differs(index);
                let (glyph, color) = status_style(node, time_differs, &palette);
                let expander = match (model.has_children(index), model.is_expanded(index)) {
                    (false, _) => None,
                    (true, true) => Some(icons::TREE_OPEN),
                    (true, false) => Some(icons::TREE_CLOSED),
                };
                let newer = match node.status {
                    Status::Different { newer } | Status::Similar { newer } => newer,
                    _ => None,
                };
                let below = if node.is_dir {
                    model.differing_below(index)
                } else {
                    0
                };
                let tip: Option<SharedString> = match &node.status {
                    Status::Error(message) => Some(message.clone().into()),
                    _ if node.is_dir => {
                        (below > 0).then(|| trf("{} differing below", &[&below]).into())
                    }
                    _ => row_tip(node, time_differs, mode).map(Into::into),
                };
                let is_selected = selected == Some(row);
                let name_min = name_min_width(&node.name, advance);
                let shown = if node.is_dir {
                    model.files_shown_below(index)
                } else {
                    0
                };
                let pill_text = count_of(u64::from(shown), "file", "files");
                let pill = (shown > 0
                    && cell_width
                        .is_none_or(|cell| pill_fits(cell, node.depth, name_min, pill_text.len())))
                .then(|| Pill {
                    text: pill_text.into(),
                    neutral: matches!(model.filter(), FolderFilter::All | FolderFilter::Same),
                    tip: pill_tip(
                        model.filter(),
                        model.name_filter().is_some(),
                        model.breakdown(index),
                    )
                    .into(),
                });
                let name_full = name_full_width(&node.name, advance);
                let depth = node.depth;
                let moved = moved_tag_of(
                    model,
                    index,
                    row,
                    cell_width,
                    &|chars| cell_width.is_none_or(|cell| pill_fits(cell, depth, name_full, chars)),
                    cx,
                );
                let cell = |left: bool| {
                    side_cell(
                        SideCell {
                            node,
                            entry: if left {
                                node.left.as_ref()
                            } else {
                                node.right.as_ref()
                            },
                            row,
                            left,
                            expander,
                            size: if node.is_dir {
                                model
                                    .folder_size(index, left)
                                    .filter(|&bytes| bytes > 0)
                                    .map(format_size)
                                    .unwrap_or_default()
                            } else {
                                format_size(
                                    if left { &node.left } else { &node.right }
                                        .as_ref()
                                        .map_or(0, |e| e.len),
                                )
                            },
                            modified: if node.is_dir {
                                model.folder_newest(index, left)
                            } else {
                                if left { &node.left } else { &node.right }
                                    .as_ref()
                                    .and_then(|e| e.modified)
                            },
                            newer: newer == Some(if left { Side::Left } else { Side::Right }),
                            other_time: time_differs
                                && matches!(node.status, Status::Same | Status::Similar { .. }),
                            path: {
                                let (root, entry) = if left {
                                    (&self.left, &node.left)
                                } else {
                                    (&self.right, &node.right)
                                };
                                entry.as_ref().map_or_else(Default::default, |e| {
                                    root.join(&e.rel).display().to_string().into()
                                })
                            },
                            name_min,
                            moved: moved.clone(),
                            pill: pill.clone(),
                            odd: self
                                .odd
                                .marks
                                .as_ref()
                                .and_then(|marks| marks.get(index, left))
                                .map(|scan| (odd_tip(scan).into(), scan.warn > 0)),
                            on_expander: expander.is_some().then(|| {
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.toggle_expander(row, cx)
                                })
                            }),
                        },
                        &palette,
                        &fonts,
                        hatch,
                    )
                };
                h_flex()
                    .id(("folder-row", row))
                    // Registers the row for the UI test harness; inert in normal builds.
                    .test_support()
                    .relative()
                    .h(px(ROW_HEIGHT))
                    .w_full()
                    .pr(px(SCROLLBAR_WIDTH))
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
                    .child(cell(true))
                    .child(
                        h_flex()
                            .id(("folder-mid", row))
                            .test_support()
                            .flex_none()
                            .w(px(MID_COL))
                            .h_full()
                            .justify_center()
                            .items_center()
                            .border_l_1()
                            .border_r_1()
                            .border_color(palette.line2)
                            .text_size(px(if glyph == "⚠" { 12. } else { 11. }))
                            .font_weight(FontWeight::BOLD)
                            .text_color(color)
                            .child(glyph)
                            .when_some(tip, |d, tip| {
                                d.tooltip(move |window, cx| ld_tip(tip.clone(), window, cx))
                            }),
                    )
                    .child(cell(false))
                    .into_any_element()
            })
            .collect()
    }

    /// Column titles per side with the centre glyph column between them.
    pub(super) fn render_header(&self, p: &Palette) -> impl IntoElement {
        let side = || {
            h_flex()
                .flex_1()
                .min_w_0()
                .px(px(8.))
                .gap(px(6.))
                .child(div().flex_1().child(tr("Name")))
                .child(div().w(px(SIZE_COL)).text_right().child(tr("Size")))
                .child(div().w(px(DATE_COL)).child(tr("Modified")))
        };
        h_flex()
            .id("folder-header")
            .test_support()
            .flex_none()
            .h(px(26.))
            .w_full()
            .pr(px(SCROLLBAR_WIDTH))
            .items_center()
            .bg(p.panel2)
            .border_b_1()
            .border_color(p.line)
            .text_size(px(11.5))
            .text_color(p.muted)
            .child(side())
            .child(
                div()
                    .flex_none()
                    .w(px(MID_COL))
                    .h_full()
                    .border_l_1()
                    .border_r_1()
                    .border_color(p.line2),
            )
            .child(side())
    }
}
