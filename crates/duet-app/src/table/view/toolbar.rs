//! The table view's toolbar (files, navigation, actions) and options row (delimiter, cell
//! rules, key columns, row-state chips).

use duet_core::table::nav::Filter;
use gpui_kit::ClickEvent;
use gpui_kit::FontWeight;
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};

use super::*;
use crate::pathbar::path_input;
use crate::status::Tone;
use crate::table::menu::export_menu;
use crate::ui::fonts::{CODE_SIZE, Fonts, UI_SIZE};
use crate::ui::icons;
use crate::ui::menu::fill as fill_menu;
use crate::ui::style::{
    LdExt as _, RADIUS, ld_button, ld_chip, ld_dropdown, ld_icon_button, ld_separator, ld_tip,
    ld_toggle,
};

const KEYS_TOOLTIP: &str =
    "Column names or 1-based numbers, comma-separated. Empty: rows align by order.";
pub(super) const KEYS_PLACEHOLDER: &str = "e.g. id or 1,3 (empty = row order)";
const NUMBERS_TOOLTIP: &str = "1.0 = 1, 1,000 = 1000, 1e3 = 1000";

/// The chip's tone: what colour its count takes.
fn chip_tone(filter: Filter) -> Tone {
    match filter {
        Filter::All | Filter::Same => Tone::Muted,
        Filter::Differences => Tone::Changed,
        Filter::LeftOnly => Tone::LeftOnly,
        Filter::RightOnly => Tone::RightOnly,
    }
}

impl TableView {
    pub(super) fn render_toolbar(
        &self,
        p: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let stops = !self.view.stops.is_empty();
        let nav = |id: &'static str, icon: IconName, tip: &'static str, jump: Jump| {
            ld_icon_button(id, icon, tip, p).ld_click(
                stops,
                cx.listener(move |this, _, _, cx| this.jump(jump, cx)),
            )
        };
        let (first, prev, next, last) = (
            nav(
                "table-first",
                icons::FIRST,
                "First differing row (Ctrl+Home)",
                Jump::First,
            ),
            nav(
                "table-prev",
                icons::PREV,
                "Previous differing row (F7)",
                Jump::Prev,
            ),
            nav(
                "table-next",
                icons::NEXT,
                "Next differing row (F8)",
                Jump::Next,
            ),
            nav(
                "table-last",
                icons::LAST,
                "Last differing row (Ctrl+End)",
                Jump::Last,
            ),
        );
        h_flex()
            .id("table-toolbar")
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
            .child(self.render_side_controls(true, p, cx))
            .child(ld_separator(p))
            .child(self.render_side_controls(false, p, cx))
            .child(ld_separator(p))
            .child(first)
            .child(prev)
            .child(
                div()
                    .id("table-position")
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
                ld_icon_button("table-swap", icons::SWAP, "Swap sides (F9)", p)
                    .ld_click(true, cx.listener(|this, _, _, cx| this.swap_sides(cx))),
            )
            .child(
                ld_icon_button("table-reload", icons::RELOAD, "Reload (F5)", p)
                    .ld_click(true, cx.listener(|this, _, _, cx| this.reload(cx))),
            )
            .child(
                ld_button("table-find", "Find…", p)
                    .ld_tooltip("Find (Ctrl+F)")
                    .ld_click(
                        self.diff.is_some(),
                        cx.listener(|this, _, window, cx| this.open_find(window, cx)),
                    ),
            )
            .child(self.export_menu_button(cx))
    }

    /// "Export", a dropdown with the differences as CSV and the HTML report.
    fn export_menu_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (view, focus) = (cx.entity().downgrade(), self.focus.clone());
        ld_dropdown("table-export", "Export", UI_SIZE)
            .tooltip("Export the differences")
            .disabled(!self.can_export())
            .dropdown_menu(move |menu, _, cx| {
                let entries = view
                    .upgrade()
                    .map(|view| export_menu(view.read(cx)))
                    .unwrap_or_default();
                fill_menu(menu, entries, Some(focus.clone()))
            })
    }

    /// One side's open and paste buttons and its name, which a click turns into a path box.
    fn render_side_controls(
        &self,
        left: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (side, open_id, paste_id, name_id) = if left {
            (
                "left",
                "table-open-left",
                "table-paste-left",
                "table-left-name",
            )
        } else {
            (
                "right",
                "table-open-right",
                "table-paste-right",
                "table-right-name",
            )
        };
        let open_tip = if left {
            "Open the left file (Ctrl+O)"
        } else {
            "Open the right file (Ctrl+Shift+O)"
        };
        let info = self.side(left);
        if let Some(edit) = self.path_edit.as_ref().filter(|e| e.left() == left) {
            let id = if left {
                "table-left-path"
            } else {
                "table-right-path"
            };
            return h_flex()
                .gap(px(5.))
                .items_center()
                .child(div().w(px(380.)).child(path_input(id, edit.edit(), p)))
                .into_any_element();
        }
        let label = div()
            .id(name_id)
            .cursor_text()
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.begin_path_edit(left, window, cx)
            }))
            .test_support()
            .flex_none()
            .max_w(px(260.))
            .truncate()
            .aria_label(self.name_text(left))
            .text_color(if info.is_some() { p.text } else { p.muted })
            .child(info.map_or("No file chosen".to_owned(), |info| info.name.clone()))
            .children(info.map(|info| {
                div()
                    .ml(px(8.))
                    .text_color(p.dim)
                    .child(self.size_text(info))
            }));
        let label = match self.name_tooltip(left) {
            Some(tip) => label
                .tooltip(move |window, cx| ld_tip(tip.clone().into(), window, cx))
                .into_any_element(),
            None => label.into_any_element(),
        };
        h_flex()
            .gap(px(5.))
            .items_center()
            .child(
                ld_button(open_id, format!("Open {side}…"), p)
                    .ld_tooltip(open_tip)
                    .ld_click(
                        true,
                        cx.listener(move |this, _, window, cx| this.pick_file(left, window, cx)),
                    ),
            )
            .child(
                ld_icon_button(
                    paste_id,
                    icons::PASTE_TABLE,
                    format!("Paste the clipboard as the {side} table"),
                    p,
                )
                .ld_click(
                    true,
                    cx.listener(move |this, _, _, cx| this.paste_side(left, cx)),
                ),
            )
            .child(label)
            .into_any_element()
    }

    pub(super) fn render_options(
        &self,
        p: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let view = cx.entity();
        let toggle = |id: &'static str,
                      label: &'static str,
                      tip: &'static str,
                      on: bool,
                      change: fn(&mut TablePrefs)| {
            let view = view.clone();
            ld_toggle(id, label, on, p)
                .ld_tooltip(tip)
                .ld_click(true, move |_, _, cx| {
                    view.update(cx, |this, cx| this.set_prefs(change, cx))
                })
        };
        let counts = self.diff.as_ref().map(|diff| diff.counts);
        let chip = |filter: Filter| {
            let view = view.clone();
            let count = counts.map_or(0, |counts| filter.count(&counts));
            ld_chip(
                format!("table-chip-{}", filter.key()),
                filter.label(),
                count,
                chip_tone(filter),
                self.prefs.filter == filter,
                p,
            )
            .ld_click(true, move |_, _, cx| {
                view.update(cx, |this, cx| {
                    this.set_prefs(|prefs| prefs.filter = filter, cx)
                })
            })
        };
        h_flex()
            .id("table-options")
            .test_support()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .px(px(10.))
            .py(px(5.))
            .bg(p.panel)
            .border_b_1()
            .border_color(p.line)
            .child(self.delimiter_menu(cx))
            .child(toggle(
                "table-header",
                "First row is a header",
                "Treat the first row as column names",
                self.prefs.header,
                |prefs| prefs.header = !prefs.header,
            ))
            .child(toggle(
                "table-ignore-case",
                "Ignore case",
                "Compare text without regard to upper and lower case",
                self.prefs.ignore_case,
                |prefs| prefs.ignore_case = !prefs.ignore_case,
            ))
            .child(toggle(
                "table-trim",
                "Trim spaces",
                "Ignore spaces at the start and end of a cell",
                self.prefs.trim,
                |prefs| prefs.trim = !prefs.trim,
            ))
            .child(toggle(
                "table-numbers",
                "Compare numbers by value",
                NUMBERS_TOOLTIP,
                self.prefs.numbers,
                |prefs| prefs.numbers = !prefs.numbers,
            ))
            .child(ld_separator(p))
            .child(
                h_flex()
                    .id("table-keys")
                    .test_support()
                    .gap(px(6.))
                    .items_center()
                    .tooltip(|window, cx| ld_tip(KEYS_TOOLTIP.into(), window, cx))
                    .child(div().flex_none().text_color(p.muted).child("Key columns"))
                    .child(
                        div().w(px(260.)).child(
                            Input::new(&self.keys_input)
                                .font(fonts.code.clone())
                                .text_size(px(CODE_SIZE))
                                .aria_label("Key columns"),
                        ),
                    ),
            )
            .child(ld_separator(p))
            .child(h_flex().gap(px(5.)).children(Filter::ALL.map(chip)))
    }

    /// "Delimiter: Auto", with Auto and the four delimiters in its menu.
    fn delimiter_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let chosen = self.prefs.delimiter;
        let this = cx.entity().downgrade();
        ld_dropdown(
            "table-delimiter",
            format!("Delimiter: {}", chosen.map_or("Auto", Delimiter::label)),
            UI_SIZE,
        )
        .dropdown_menu(move |menu, _, _| {
            let choices = std::iter::once(None).chain(Delimiter::ALL.map(Some));
            choices.fold(menu, |menu, choice| {
                let this = this.clone();
                menu.item(
                    PopupMenuItem::new(choice.map_or("Auto", Delimiter::label))
                        .checked(chosen == choice)
                        .on_click(move |_, _, cx| {
                            let _ = this.update(cx, |view, cx| {
                                view.set_prefs(|prefs| prefs.delimiter = choice, cx)
                            });
                        }),
                )
            })
        })
    }
}
