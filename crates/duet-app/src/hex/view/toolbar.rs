//! The hex view's toolbar: navigation, file actions, bytes per row, the filter, go to, and
//! the find and copy buttons.

use gpui_kit::FontWeight;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};

use super::*;
use crate::ui::fonts::{Fonts, UI_SIZE};
use crate::ui::icons;
use crate::ui::style::{
    LdExt as _, RADIUS, ld_button, ld_dropdown, ld_icon_button, ld_segment, ld_segmented,
    ld_separator,
};

impl HexView {
    pub(super) fn render_toolbar(
        &self,
        p: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let blocks = self.can_navigate();
        let ready = self.pair.is_some();
        let nav = |id: &'static str, icon: IconName, tip: &'static str, action: Nav| {
            ld_icon_button(id, icon, tip, p).ld_click(
                blocks,
                cx.listener(move |this, _, _, cx| this.navigate(action, cx)),
            )
        };
        let filter = |id: &'static str, label: &'static str, only: bool, last: bool| {
            ld_segment(id, label, self.only_diff == only, last, p).ld_click(
                true,
                cx.listener(move |this, _, _, cx| this.set_only_differences(only, cx)),
            )
        };
        let (first, prev, next, last) = (
            nav(
                "hex-first",
                icons::FIRST,
                "First difference (Ctrl+Home)",
                Nav::First,
            ),
            nav(
                "hex-prev",
                icons::PREV,
                "Previous difference (F7)",
                Nav::Prev,
            ),
            nav("hex-next", icons::NEXT, "Next difference (F8)", Nav::Next),
            nav(
                "hex-last",
                icons::LAST,
                "Last difference (Ctrl+End)",
                Nav::Last,
            ),
        );
        let (all, differences) = (
            filter("hex-filter-all", "All", false, false),
            filter("hex-filter-diff", "Differences only", true, true),
        );
        h_flex()
            .id("hex-toolbar")
            .test_support()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap(px(5.))
            .px(px(10.))
            .py(px(5.))
            .bg(p.panel)
            .border_b_1()
            .border_color(p.line)
            .child(first)
            .child(prev)
            .child(
                div()
                    .id("hex-position")
                    .test_support()
                    .flex()
                    .flex_none()
                    .min_w(px(70.))
                    .h(px(28.))
                    .px(px(10.))
                    .items_center()
                    .justify_center()
                    .rounded(px(RADIUS))
                    .border_1()
                    .border_color(p.line)
                    .bg(p.panel2)
                    .font(fonts.code.clone())
                    .text_size(px(12.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(p.accent)
                    .child(self.position_text()),
            )
            .child(next)
            .child(last)
            .child(ld_separator(p))
            .child(
                ld_icon_button("hex-swap", icons::SWAP, "Swap sides (F9)", p)
                    .ld_click(true, cx.listener(|this, _, _, cx| this.swap_sides(cx))),
            )
            .child(
                ld_icon_button("hex-reload", icons::RELOAD, "Reload (F5)", p)
                    .ld_click(true, cx.listener(|this, _, _, cx| this.reload(cx))),
            )
            .child(ld_separator(p))
            .child(self.bytes_per_row_menu(cx))
            .child(ld_segmented("hex-filter", [all, differences], p))
            .child(ld_separator(p))
            .child(
                div()
                    .id("hex-goto")
                    .test_support()
                    .w(px(190.))
                    .child(Input::new(&self.goto).aria_label("Go to offset")),
            )
            .child(
                ld_button("hex-goto-go", "Go to", p)
                    .ld_tooltip("Go to the offset (Enter)")
                    .ld_click(
                        ready,
                        cx.listener(|this, _, window, cx| this.submit_goto(window, cx)),
                    ),
            )
            .child(div().flex_1())
            .child(
                ld_button("hex-find", "Find…", p)
                    .ld_tooltip("Find (Ctrl+F)")
                    .ld_click(
                        ready,
                        cx.listener(|this, _, window, cx| this.open_find(window, cx)),
                    ),
            )
            .child(
                ld_button("hex-copy", "Copy hex", p)
                    .ld_tooltip("Copy hex (Ctrl+C)")
                    .ld_click(ready, cx.listener(|this, _, _, cx| this.copy_hex(cx))),
            )
    }

    /// "Bytes per row: 16", with 8, 16 and 32 in its menu.
    fn bytes_per_row_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let chosen = self.bytes_per_row;
        let this = cx.entity().downgrade();
        ld_dropdown("hex-bpr", format!("Bytes per row: {chosen}"), UI_SIZE).dropdown_menu(
            move |menu, _, _| {
                BYTES_PER_ROW.iter().fold(menu, |menu, &bpr| {
                    let this = this.clone();
                    menu.item(
                        PopupMenuItem::new(bpr.to_string())
                            .checked(chosen == bpr)
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, |view, cx| view.set_bytes_per_row(bpr, cx));
                            }),
                    )
                })
            },
        )
    }
}
