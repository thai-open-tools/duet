//! The folder view's three toolbar rows: paths, compare options and filter chips.

use crate::folder::batch::DeleteAsk;
use crate::folder::export_panel::report_menu;
use crate::folder::model::{FolderFilter, FolderModel};
use crate::folder::ops::Direction;
use crate::folder::sync_panel::SyncScope;
use crate::palette::Palette;
use crate::ui::fonts::{Fonts, SMALL_SIZE};
use crate::ui::icons;
use crate::ui::menu::fill as fill_menu;
use crate::ui::style::{
    LdExt as _, RADIUS, ld_button, ld_chip, ld_dropdown, ld_folder_button, ld_icon_button,
    ld_icon_label_button, ld_label_icon_button, ld_separator, ld_toggle,
};
use duet_core::folder::run::CompareMode;
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::{Sizable as _, h_flex};
use gpui_kit::{AnyElement, Context, Entity, Role, div, prelude::*, px};

use super::legend::legend_panel;
use super::options::{criterion_menu, options_menu};
use super::*;

const FORMAT_TOOLTIP: &str = "Files whose lines all match but whose line endings, final newline or BOM differ count as different";

impl FolderView {
    /// Row 1: pickers, mono path boxes, swap, up and Compare.
    pub(super) fn render_paths(
        &self,
        p: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let up = self.left.parent().is_some() && self.right.parent().is_some();
        let path_box = |content: AnyElement| {
            h_flex()
                .flex_1()
                .min_w_0()
                .max_w(px(520.))
                .h(px(28.))
                .px(px(8.))
                .rounded(px(RADIUS))
                .border_1()
                .border_color(p.line)
                .bg(p.panel2)
                .font(fonts.code.clone())
                .text_size(px(12.))
                .child(content)
        };
        h_flex()
            .id("folder-paths")
            .test_support()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .px(px(12.))
            .py(px(6.))
            .bg(p.panel)
            .border_b_1()
            .border_color(p.line)
            .child(
                ld_folder_button("folder-pick-left", "Left", p)
                    .ld_tooltip("Choose the left folder")
                    .ld_click(
                        true,
                        cx.listener(|this, _, window, cx| this.pick_root(true, window, cx)),
                    ),
            )
            .child(path_box(self.path_box_content(
                true,
                "folder-left-path",
                p,
                cx,
            )))
            .child(
                ld_icon_button("folder-swap", icons::SWAP, "Swap sides (F9)", p)
                    .ld_click(true, cx.listener(|this, _, _, cx| this.swap_sides(cx))),
            )
            .child(
                ld_folder_button("folder-pick-right", "Right", p)
                    .ld_tooltip("Choose the right folder")
                    .ld_click(
                        true,
                        cx.listener(|this, _, window, cx| this.pick_root(false, window, cx)),
                    ),
            )
            .child(path_box(self.path_box_content(
                false,
                "folder-right-path",
                p,
                cx,
            )))
            .child(
                ld_icon_button("folder-up", icons::UP, "Up one folder on both sides", p).ld_click(
                    up,
                    cx.listener(|this, _, _, cx| {
                        this.go_up(cx);
                    }),
                ),
            )
            .child(
                ld_icon_label_button("folder-refresh", icons::RELOAD, "Compare", p)
                    .as_primary(p)
                    .ld_tooltip("Compare again (F5)")
                    .ld_click(
                        true,
                        cx.listener(|this, _, window, cx| this.on_refresh(&Refresh, window, cx)),
                    ),
            )
    }

    /// Row 2: how files are compared, which names take part, and the path filter.
    pub(super) fn render_options(
        &self,
        p: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mode = self.options.mode;
        let quick = mode == CompareMode::Quick;
        let formats = matches!(mode, CompareMode::Content | CompareMode::Rules);
        let mask = |id: &'static str, name: &'static str, input: &Entity<InputState>| {
            div()
                .id(id)
                .test_support()
                .w(px(220.))
                .font(fonts.code.clone())
                .child(Input::new(input).small().aria_label(name))
        };
        // A label and its field wrap as one, so a narrow window never parts them.
        let labelled = |id: &'static str, text: &'static str, field: AnyElement| {
            h_flex()
                .flex_none()
                .items_center()
                .gap(px(8.))
                .child(div().id(id).test_support().text_color(p.muted).child(text))
                .child(field)
        };
        let (view, focus) = (cx.entity().downgrade(), self.focus.clone());
        let criterion = {
            let (view, focus) = (view.clone(), focus.clone());
            ld_dropdown("folder-mode", self.mode_button_label(), SMALL_SIZE)
                .tooltip("How files are compared")
                .dropdown_menu(move |menu, _, cx| {
                    let entries = view
                        .upgrade()
                        .map(|view| criterion_menu(view.read(cx)))
                        .unwrap_or_default();
                    fill_menu(menu, entries, Some(focus.clone()))
                })
        };
        let options = ld_dropdown("folder-options-menu", "Options", SMALL_SIZE)
            .tooltip("More settings of the folder compare")
            .dropdown_menu(move |menu, _, cx| {
                let entries = view
                    .upgrade()
                    .map(|view| options_menu(view.read(cx)))
                    .unwrap_or_default();
                fill_menu(menu, entries, Some(focus.clone()))
            });
        h_flex()
            .id("folder-options")
            .test_support()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(4.))
            .bg(p.panel2)
            .border_b_1()
            .border_color(p.line)
            .text_size(px(12.))
            .child(labelled(
                "folder-criterion-label",
                "Criterion",
                criterion.into_any_element(),
            ))
            .when(quick, |row| {
                row.child(
                    h_flex()
                        .flex_none()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .id("folder-tolerance-label")
                                .test_support()
                                .text_color(p.muted)
                                .child("Tolerance"),
                        )
                        .child(
                            div()
                                .id("folder-tolerance")
                                .test_support()
                                .w(px(64.))
                                .font(fonts.code.clone())
                                .child(
                                    Input::new(&self.tolerance)
                                        .small()
                                        .aria_label("Time tolerance in seconds"),
                                ),
                        )
                        .child(div().text_color(p.muted).child("s")),
                )
                .child(
                    ld_toggle(
                        "folder-dst",
                        "Ignore 1 h DST shift",
                        self.options.quick.ignore_dst,
                        p,
                    )
                    .ld_click(
                        true,
                        cx.listener(|this, _, _, cx| {
                            this.set_ignore_dst(!this.options.quick.ignore_dst, cx)
                        }),
                    ),
                )
            })
            .when(formats, |row| {
                row.child(
                    ld_toggle(
                        "folder-format-important",
                        "Count line-ending and BOM differences",
                        self.options.format_important,
                        p,
                    )
                    .ld_tooltip(FORMAT_TOOLTIP)
                    .ld_click(
                        true,
                        cx.listener(|this, _, _, cx| {
                            this.set_format_important(!this.options.format_important, cx)
                        }),
                    ),
                )
            })
            .child(ld_separator(p))
            .child(labelled(
                "folder-include-label",
                "Include",
                mask("folder-include", "Include masks", &self.include).into_any_element(),
            ))
            .child(labelled(
                "folder-exclude-label",
                "Exclude",
                mask("folder-exclude", "Exclude masks", &self.exclude).into_any_element(),
            ))
            // The name filter joins the masks, so the toolbar keeps three rows (U17).
            .child(
                div()
                    .id("folder-name-filter")
                    .test_support()
                    .w(px(200.))
                    .child(
                        Input::new(&self.name_filter)
                            .small()
                            .aria_label("Filter paths"),
                    ),
            )
            .child(div().ml_auto().child(options))
    }

    /// Row 3: filter chips with their counts, expand and collapse, and the file actions.
    pub(super) fn render_chips(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let counts = self
            .model
            .as_ref()
            .map(FolderModel::counts)
            .unwrap_or_default();
        let current = self
            .model
            .as_ref()
            .map(FolderModel::filter)
            .unwrap_or_default();
        let flat = self.prefs.layout == Layout::Flat;
        h_flex()
            .id("folder-chips")
            .test_support()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .px(px(12.))
            .py(px(6.))
            .bg(p.panel)
            .border_b_1()
            .border_color(p.line)
            // The tab list holds the chips alone; the buttons and the field after them are not
            // tabs.
            .child(
                h_flex()
                    .id("folder-chip-tabs")
                    .test_support()
                    .role(Role::TabList)
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .children(FolderFilter::ALL.into_iter().map(|filter| {
                        ld_chip(
                            filter.chip_id(),
                            filter.chip_label(),
                            filter.count(&counts),
                            filter.chip_tone(),
                            filter == current,
                            p,
                        )
                        .ld_click(
                            true,
                            cx.listener(move |this, _, _, cx| this.set_filter(filter, cx)),
                        )
                    })),
            )
            .child(ld_separator(p))
            .child(
                ld_button(
                    "folder-layout",
                    if flat {
                        "View: flat list"
                    } else {
                        "View: tree"
                    },
                    p,
                )
                .ld_small()
                .ld_tooltip("Switch between the two-sided tree and a flat list you can sort")
                .ld_click(
                    true,
                    cx.listener(move |this, _, _, cx| {
                        this.set_layout(if flat { Layout::Tree } else { Layout::Flat }, cx)
                    }),
                ),
            )
            // The flat list has no folders to open or close.
            .child(
                ld_icon_button("folder-expand-all", icons::EXPAND_ALL, "Expand all", p).ld_click(
                    !flat,
                    cx.listener(|this, _, window, cx| this.expand_all(&ExpandAll, window, cx)),
                ),
            )
            .child(
                ld_icon_button(
                    "folder-collapse-all",
                    icons::COLLAPSE_ALL,
                    "Collapse all",
                    p,
                )
                .ld_click(
                    !flat,
                    cx.listener(|this, _, window, cx| this.collapse_all(&CollapseAll, window, cx)),
                ),
            )
            // The wrapper holds the button and the panel, so a press on the button is not a
            // press "outside" and the button's click toggles.
            .child(
                div()
                    .id("folder-legend-wrap")
                    .relative()
                    .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        if this.legend_open {
                            this.legend_open = false;
                            cx.notify();
                        }
                    }))
                    .child(
                        ld_icon_label_button("folder-legend", icons::LEGEND, "Legend", p)
                            .ld_small()
                            .ld_on(self.legend_open, p)
                            .ld_tooltip("What the symbols and colours mean")
                            .ld_click(true, cx.listener(|this, _, _, cx| this.toggle_legend(cx))),
                    )
                    .when(self.legend_open, |d| d.child(legend_panel(p))),
            )
            .child(ld_separator(p))
            .child(
                ld_label_icon_button("folder-copy-right", "Copy", icons::TO_RIGHT, p)
                    .ld_small()
                    .ld_tooltip("Copy to the right (Alt+→)")
                    .ld_click(
                        true,
                        cx.listener(|this, _, window, cx| {
                            this.copy(Direction::LeftToRight, window, cx)
                        }),
                    ),
            )
            .child(
                ld_icon_label_button("folder-copy-left", icons::TO_LEFT, "Copy", p)
                    .ld_small()
                    .ld_tooltip("Copy to the left (Alt+←)")
                    .ld_click(
                        true,
                        cx.listener(|this, _, window, cx| {
                            this.copy(Direction::RightToLeft, window, cx)
                        }),
                    ),
            )
            .child(
                ld_button("folder-delete", "Delete…", p)
                    .ld_small()
                    .ld_tooltip("Delete (Del)")
                    .ld_click(
                        true,
                        cx.listener(|this, _, window, cx| {
                            this.delete_selection(DeleteAsk::Choose, window, cx)
                        }),
                    ),
            )
            .child(
                ld_button("folder-reveal", "Show in Explorer", p)
                    .ld_small()
                    .ld_tooltip("Show the selected row in Explorer")
                    .ld_click(
                        true,
                        cx.listener(|this, _, window, cx| {
                            this.reveal_selected(&RevealSelected, window, cx)
                        }),
                    ),
            )
            .child(
                ld_icon_label_button("folder-sync", icons::SYNC, "Sync…", p)
                    .ld_small()
                    .ld_tooltip("Sync the folders: update or mirror, with a preview first")
                    .ld_click(
                        true,
                        cx.listener(|this, _, window, cx| {
                            this.open_sync(SyncScope::All, window, cx)
                        }),
                    ),
            )
            .child({
                let (view, focus) = (cx.entity().downgrade(), self.focus.clone());
                ld_dropdown("folder-report", "Report", SMALL_SIZE)
                    .tooltip("Reports: HTML, Excel, text or CSV")
                    .disabled(!self.can_report())
                    .dropdown_menu(move |menu, _, cx| {
                        let entries = view
                            .upgrade()
                            .map(|view| report_menu(view.read(cx)))
                            .unwrap_or_default();
                        fill_menu(menu, entries, Some(focus.clone()))
                    })
            })
    }
}
