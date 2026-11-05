//! The text view's two toolbar rows and its find and go-to bars (spec 6.4, items 1-3).

use super::*;
use gpui_kit::FontWeight;

use crate::format::count_of;
use crate::i18n::{tr, trf};
use crate::text::prefs::{SaveEncoding, SaveEol, TAB_WIDTHS};
use crate::text::report::{report_language_menu, report_menu};
use crate::ui::edit_menu::ld_input;
use crate::ui::fonts::{Fonts, SMALL_SIZE};
use crate::ui::icons;
use crate::ui::style::{
    RADIUS, ld_button, ld_dropdown, ld_icon_button, ld_icon_label_button, ld_label_icon_button,
    ld_segment, ld_segmented, ld_separator, ld_text_dropdown, ld_tip, ld_toggle,
};

/// "3 / 6" at a difference, "– / 6" before the first F8, "" before the first compare.
pub fn position_text(model: Option<&TextModel>) -> String {
    model.map_or_else(String::new, |m| {
        let at = m
            .current_block()
            .map_or("–".to_owned(), |b| (b + 1).to_string());
        format!("{at} / {}", m.block_count())
    })
}

/// The whitespace menu's items, one per mode (R2).
const WHITESPACE_ITEMS: [(WhitespaceMode, &str); 6] = [
    (WhitespaceMode::None, tr_key("Compare whitespace")),
    (
        WhitespaceMode::Trailing,
        tr_key("Ignore trailing whitespace"),
    ),
    (WhitespaceMode::Leading, tr_key("Ignore leading whitespace")),
    (
        WhitespaceMode::Edges,
        tr_key("Ignore whitespace at line edges"),
    ),
    (
        WhitespaceMode::Amount,
        tr_key("Ignore the amount of whitespace"),
    ),
    (WhitespaceMode::All, tr_key("Ignore all whitespace")),
];

/// The "On save" menu's line-ending items (R13).
const SAVE_EOLS: [(SaveEol, &str); 3] = [
    (SaveEol::Keep, tr_key("Line endings: keep")),
    (SaveEol::CrLf, tr_key("Line endings: CRLF")),
    (SaveEol::Lf, tr_key("Line endings: LF")),
];

/// The "On save" menu's encoding items (R13).
const SAVE_ENCODINGS: [(SaveEncoding, &str); 5] = [
    (SaveEncoding::Keep, tr_key("Encoding: keep")),
    (SaveEncoding::Utf8, tr_key("Encoding: UTF-8")),
    (SaveEncoding::Utf8Bom, tr_key("Encoding: UTF-8 with BOM")),
    (
        SaveEncoding::Windows874,
        tr_key("Encoding: Thai (windows-874)"),
    ),
    (SaveEncoding::Utf16Le, tr_key("Encoding: UTF-16LE")),
];

/// A font size as the dropdown prints it: "12.5" as is, the others as integers.
fn font_size_text(size: f32) -> String {
    if size.fract() == 0. {
        format!("{size:.0}")
    } else {
        size.to_string()
    }
}

impl TextView {
    pub(super) fn render_toolbar(
        &self,
        p: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let model = self.model.as_ref();
        let blocks = model.is_some_and(|m| m.block_count() > 0);
        // Copying all needs a target that can be edited, and a source that shows its own text:
        // not the formatted or the sorted view.
        let can_take = |left: bool| model.is_some_and(|m| m.can_copy_to(left));
        h_flex()
            .id("text-toolbar")
            .test_support()
            .flex_none()
            .flex_wrap()
            .items_center()
            // 4 px between controls, not 5: Sort keys came 33 px short of one row at 1400 px with
            // the test fonts (Plan D).
            .gap(px(4.))
            .px(px(10.))
            .py(px(5.))
            .bg(p.panel)
            .border_b_1()
            .border_color(p.line)
            .children(self.has_origin.then(|| {
                ld_icon_label_button("text-back", icons::BACK, tr("Back"), p)
                    .ld_tooltip(tr("Back to the folder compare (Esc)"))
                    .ld_click(true, cx.listener(|_, _, _, cx| cx.emit(TextEvent::Back)))
            }))
            .children(self.has_origin.then(|| ld_separator(p)))
            .child(
                ld_icon_button("text-swap", icons::SWAP, tr("Swap sides (F9)"), p)
                    .ld_click(true, cx.listener(|this, _, _, cx| this.swap_sides(cx))),
            )
            .child(
                ld_icon_button("text-reload", icons::RELOAD, tr("Reload (F5)"), p).ld_click(
                    true,
                    cx.listener(|this, _, window, cx| this.on_reload(&Reload, window, cx)),
                ),
            )
            .child(ld_separator(p))
            .child(
                ld_icon_button(
                    "text-first",
                    icons::FIRST,
                    tr("First difference (Ctrl+Home)"),
                    p,
                )
                .ld_click(
                    blocks,
                    cx.listener(|this, _, window, cx| this.first_change(&FirstChange, window, cx)),
                ),
            )
            .child(
                ld_icon_button("text-prev", icons::PREV, tr("Previous difference (F7)"), p)
                    .ld_click(
                        blocks,
                        cx.listener(|this, _, window, cx| {
                            this.prev_change(&PrevChange, window, cx)
                        }),
                    ),
            )
            .child(
                div()
                    .id("text-position")
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
                    .child(position_text(model)),
            )
            .child(
                ld_icon_button("text-next", icons::NEXT, tr("Next difference (F8)"), p).ld_click(
                    blocks,
                    cx.listener(|this, _, window, cx| this.next_change(&NextChange, window, cx)),
                ),
            )
            .child(
                ld_icon_button(
                    "text-last",
                    icons::LAST,
                    tr("Last difference (Ctrl+End)"),
                    p,
                )
                .ld_click(
                    blocks,
                    cx.listener(|this, _, window, cx| this.last_change(&LastChange, window, cx)),
                ),
            )
            .child(ld_separator(p))
            .child(
                ld_label_icon_button("text-copy-all-right", tr("All"), icons::TO_RIGHT, p)
                    .ld_tooltip(tr("Copy all to the right"))
                    .ld_click(
                        can_take(false),
                        cx.listener(|this, _, _, cx| {
                            this.copy_all(false, cx);
                        }),
                    ),
            )
            .child(
                ld_icon_label_button("text-copy-all-left", icons::TO_LEFT, tr("All"), p)
                    .ld_tooltip(tr("Copy all to the left"))
                    .ld_click(
                        can_take(true),
                        cx.listener(|this, _, _, cx| {
                            this.copy_all(true, cx);
                        }),
                    ),
            )
            .child(
                ld_icon_button("text-undo", icons::UNDO, tr("Undo (Ctrl+Z)"), p).ld_click(
                    self.undo.can_undo(),
                    cx.listener(|this, _, _, cx| this.undo_edit(cx)),
                ),
            )
            .child(
                ld_icon_button("text-redo", icons::REDO, tr("Redo (Ctrl+Y)"), p).ld_click(
                    self.undo.can_redo(),
                    cx.listener(|this, _, _, cx| this.redo_edit(cx)),
                ),
            )
            .child(ld_separator(p))
            .child(
                ld_button("text-format", tr("Format"), p)
                    .ld_on(self.request.format, p)
                    .ld_tooltip(tr("Pretty-print JSON or XML on both sides (read-only)"))
                    .ld_click(
                        true,
                        cx.listener(|this, _, window, cx| {
                            let on = !this.request.format;
                            this.set_format(on, window, cx)
                        }),
                    ),
            )
            .child(
                ld_button("text-sort-keys", tr("Sort keys"), p)
                    .ld_on(self.request.sort_keys, p)
                    .ld_tooltip(tr(
                        "Sort the keys of JSON or YAML on both sides, indented the same (read-only). Line numbers are those of the sorted text.",
                    ))
                    .ld_click(
                        true,
                        cx.listener(|this, _, window, cx| {
                            let on = !this.request.sort_keys;
                            this.set_sort_keys(on, window, cx)
                        }),
                    ),
            )
            .child(
                ld_button("text-reformat", tr("Reformat"), p)
                    .ld_tooltip(tr(
                        "Re-indent JSON or XML, else trim trailing spaces, as an edit you can save",
                    ))
                    .ld_click(true, cx.listener(|this, _, _, cx| this.reformat_sides(cx))),
            )
            .child(
                ld_button("text-save-left", tr("Save left"), p)
                    .ld_tooltip(tr("Save the left file"))
                    .ld_click(
                        self.save_enabled(true),
                        cx.listener(|this, _, window, cx| {
                            this.save_side(true, window, cx).detach()
                        }),
                    ),
            )
            .child(
                ld_button("text-save-right", tr("Save right"), p)
                    .ld_tooltip(tr("Save the right file"))
                    .ld_click(
                        self.save_enabled(false),
                        cx.listener(|this, _, window, cx| {
                            this.save_side(false, window, cx).detach()
                        }),
                    ),
            )
            .child(self.report_menu_button(cx))
            .child(div().flex_1())
            // The save choices and the font size end the row, so the sub-bar keeps one line
            // (U16).
            .child(self.save_options_menu(cx))
            .child(self.font_menu(cx))
            .child(ld_separator(p))
            .child(
                ld_icon_button("text-find-open", icons::FIND, tr("Find (Ctrl+F)"), p).ld_click(
                    true,
                    cx.listener(|this, _, window, cx| this.open_find(&OpenFind, window, cx)),
                ),
            )
            .child(
                ld_icon_button(
                    "text-goto-open",
                    icons::GO_TO_LINE,
                    tr("Go to line (Ctrl+G)"),
                    p,
                )
                .ld_click(
                    true,
                    cx.listener(|this, _, window, cx| this.open_goto(&OpenGoTo, window, cx)),
                ),
            )
    }

    pub(super) fn render_subbar(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let filter = self
            .model
            .as_ref()
            .map_or(TextFilter::All, TextModel::filter);
        let options = self.request.options;
        let language = {
            let this = cx.entity().downgrade();
            let (chosen, detected) = (self.language, self.detected_language());
            let label = chosen.map_or(tr("Auto"), Language::label);
            ld_dropdown("text-language", trf("Type: {}", &[&label]), SMALL_SIZE).dropdown_menu(
                move |menu, _, _| {
                    let item = |label: String, language: Option<Language>| {
                        let this = this.clone();
                        crate::ui::menu::row(label)
                            .checked(chosen == language)
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, |view, cx| view.set_language(language, cx));
                            })
                    };
                    Language::ALL.into_iter().fold(
                        menu.item(item(trf("Auto ({})", &[&detected.label()]), None)),
                        |menu, language| {
                            menu.item(item(language.label().to_owned(), Some(language)))
                        },
                    )
                },
            )
        };
        let segment = |id: &'static str,
                       label: &'static str,
                       tip: &'static str,
                       target: TextFilter,
                       last: bool| {
            ld_segment(id, label, filter == target, last, p)
                .ld_tooltip(tip)
                .ld_click(
                    true,
                    cx.listener(move |this, _, _, cx| this.set_filter(target, cx)),
                )
        };
        let whitespace = {
            let this = cx.entity().downgrade();
            let label = match options.whitespace {
                WhitespaceMode::None => tr("Whitespace: compare"),
                WhitespaceMode::Trailing => tr("Whitespace: ignore trailing"),
                WhitespaceMode::Leading => tr("Whitespace: ignore leading"),
                WhitespaceMode::Edges => tr("Whitespace: ignore at edges"),
                WhitespaceMode::Amount => tr("Whitespace: ignore amount"),
                WhitespaceMode::All => tr("Whitespace: ignore all"),
            };
            ld_dropdown("text-ignore-ws", label, SMALL_SIZE).dropdown_menu(move |menu, _, _| {
                let item = |label: &'static str, mode: WhitespaceMode| {
                    let this = this.clone();
                    crate::ui::menu::row(tr(label))
                        .checked(options.whitespace == mode)
                        .on_click(move |_, _, cx| {
                            let _ = this.update(cx, |view, cx| view.set_whitespace(mode, cx));
                        })
                };
                WHITESPACE_ITEMS
                    .into_iter()
                    .fold(menu, |menu, (mode, label)| menu.item(item(label, mode)))
            })
        };
        // 6 px between controls, not 8: with the font size and the encodings moved out, the
        // row still came 1 px short of one line at 1400 px (U16).
        h_flex()
            .id("text-subbar")
            .test_support()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .px(px(12.))
            .py(px(4.))
            .bg(p.panel2)
            .border_b_1()
            .border_color(p.line)
            .text_size(px(SMALL_SIZE))
            .child(ld_segmented(
                "text-filter",
                [
                    segment(
                        "text-filter-all",
                        tr("All"),
                        tr("All lines (Ctrl+1)"),
                        TextFilter::All,
                        false,
                    ),
                    segment(
                        "text-filter-diffs",
                        tr("Differences"),
                        tr("Differences only (Ctrl+2)"),
                        TextFilter::Differences,
                        false,
                    ),
                    segment(
                        "text-filter-context",
                        tr("Context"),
                        tr("Differences with context (Ctrl+3)"),
                        TextFilter::Context(self.context_lines()),
                        false,
                    ),
                    segment(
                        "text-filter-same",
                        tr("Same"),
                        tr("Same lines only (Ctrl+4)"),
                        TextFilter::Same,
                        true,
                    ),
                ],
                p,
            ))
            .child(ld_separator(p))
            .child(div().text_color(p.muted).child(tr("Ignore")))
            .child(whitespace)
            .child(
                ld_toggle("text-ignore-case", tr("Case"), options.ignore_case, p).ld_click(
                    true,
                    cx.listener(|this, _, _, cx| this.toggle_ignore(IgnoreChoice::Case, cx)),
                ),
            )
            .child(
                ld_toggle("text-ignore-eol", tr("Line endings"), options.ignore_eol, p).ld_click(
                    true,
                    cx.listener(|this, _, _, cx| this.toggle_ignore(IgnoreChoice::LineEndings, cx)),
                ),
            )
            .child(
                ld_toggle(
                    "text-ignore-blank",
                    tr("Blank lines"),
                    self.prefs.ignore_blank,
                    p,
                )
                .ld_click(
                    true,
                    cx.listener(|this, _, _, cx| {
                        let on = !this.prefs.ignore_blank;
                        this.set_ignore_blank(on, cx)
                    }),
                ),
            )
            .child(
                ld_toggle(
                    "text-ignore-comments",
                    tr("Comments"),
                    self.prefs.ignore_comments,
                    p,
                )
                .ld_click(
                    true,
                    cx.listener(|this, _, _, cx| {
                        let on = !this.prefs.ignore_comments;
                        this.set_ignore_comments(on, cx)
                    }),
                ),
            )
            .child(
                ld_button("text-rules-open", tr("Rules…"), p)
                    .ld_on(self.rules_lit(), p)
                    .ld_tooltip(tr("Text and line rules, and the context size"))
                    .ld_click(
                        true,
                        // A second click closes the panel, as Cancel does (X-74).
                        cx.listener(|this, _, window, cx| {
                            if this.rules.is_some() {
                                this.cancel_rules(window, cx)
                            } else {
                                this.open_rules(window, cx)
                            }
                        }),
                    ),
            )
            .child(ld_separator(p))
            .child(
                ld_toggle("text-moves", tr("Moves"), self.request.detect_moves, p)
                    .ld_tooltip(tr("Mark lines that moved"))
                    .ld_click(
                        true,
                        cx.listener(|this, _, _, cx| {
                            let on = !this.request.detect_moves;
                            this.set_detect_moves(on, cx)
                        }),
                    ),
            )
            .child(
                ld_toggle("text-syntax", tr("Syntax"), self.syntax_on, p)
                    .ld_tooltip(self.syntax_tooltip())
                    .ld_click(
                        true,
                        cx.listener(|this, _, _, cx| this.set_syntax(!this.syntax_on, cx)),
                    ),
            )
            .child(language)
            .child(
                ld_toggle(
                    "text-show-ws",
                    tr("Show spaces"),
                    self.display.show_whitespace,
                    p,
                )
                .ld_tooltip(tr("Show spaces and tabs"))
                .ld_click(
                    true,
                    cx.listener(|this, _, window, cx| {
                        this.toggle_whitespace(&ToggleWhitespace, window, cx)
                    }),
                ),
            )
            .child(
                ld_toggle("text-wrap", tr("Wrap"), self.wrap, p)
                    .ld_tooltip(tr("Wrap long lines"))
                    .ld_click(
                        true,
                        cx.listener(|this, _, _, cx| {
                            let on = !this.wrap;
                            this.set_wrap(on, cx);
                        }),
                    ),
            )
            .child(
                ld_toggle("text-details", tr("Details"), self.prefs.details, p)
                    .ld_tooltip(tr("Show the whole line under the rows"))
                    .ld_click(
                        true,
                        cx.listener(|this, _, _, cx| {
                            let on = !this.details();
                            this.set_details(on, cx);
                        }),
                    ),
            )
            .child(self.tab_menu(cx))
    }

    /// "Report", a dropdown: the HTML, patch and text reports and the copied patch (R21).
    fn report_menu_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (view, focus) = (cx.entity().downgrade(), self.focus.clone());
        ld_dropdown("text-report", tr("Report"), UI_SIZE)
            .tooltip(tr(
                "Save the comparison as an HTML page, a patch or a text report",
            ))
            .dropdown_menu(move |menu, _, cx| {
                let entries = view
                    .upgrade()
                    .map(|view| {
                        let view = view.read(cx);
                        let mut entries = report_menu(view);
                        entries.extend(report_language_menu(view));
                        entries
                    })
                    .unwrap_or_default();
                fill_menu(menu, entries, Some(focus.clone()))
            })
    }

    /// "On save: keep", or what a save changes: "On save: LF · UTF-8" (R13).
    fn save_options_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (eol, encoding) = (self.prefs.save_eol, self.prefs.save_encoding);
        let mut changes = Vec::new();
        if eol != SaveEol::Keep {
            changes.push(eol.label());
        }
        if encoding != SaveEncoding::Keep {
            changes.push(encoding.label());
        }
        let label = if changes.is_empty() {
            tr("On save: keep").to_owned()
        } else {
            trf("On save: {}", &[&changes.join(" · ")])
        };
        let this = cx.entity().downgrade();
        ld_dropdown("text-save-options", label, UI_SIZE)
            .tooltip(tr(
                "Change the line endings or the encoding of the files you save",
            ))
            .dropdown_menu(move |menu, _, _| {
                let menu = SAVE_EOLS.iter().fold(menu, |menu, (choice, text)| {
                    let (this, choice) = (this.clone(), *choice);
                    menu.item(
                        crate::ui::menu::row(tr(text))
                            .checked(eol == choice)
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, |view, cx| view.set_save_eol(choice, cx));
                            }),
                    )
                });
                SAVE_ENCODINGS
                    .iter()
                    .fold(menu.separator(), |menu, (choice, text)| {
                        let (this, choice) = (this.clone(), *choice);
                        menu.item(
                            crate::ui::menu::row(tr(text))
                                .checked(encoding == choice)
                                .on_click(move |_, _, cx| {
                                    let _ = this
                                        .update(cx, |view, cx| view.set_save_encoding(choice, cx));
                                }),
                        )
                    })
            })
    }

    /// "Tab 4", with 1 to 16 in its menu (R5).
    fn tab_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let chosen = self.display.tab_width;
        let this = cx.entity().downgrade();
        ld_dropdown("text-tab", trf("Tab {}", &[&chosen]), SMALL_SIZE)
            .tooltip(tr("Tab width"))
            .dropdown_menu(move |menu, _, _| {
                TAB_WIDTHS.fold(menu, |menu, width| {
                    let this = this.clone();
                    menu.item(
                        crate::ui::menu::row(width.to_string())
                            .checked(chosen == usize::from(width))
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, |view, cx| view.set_tab_width(width, cx));
                            }),
                    )
                })
            })
    }

    /// "Font 12.5", with the sizes of `FONT_SIZES` in its menu (R4).
    fn font_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let chosen = self.font_size;
        let this = cx.entity().downgrade();
        ld_dropdown(
            "text-font",
            trf("Font {}", &[&font_size_text(chosen)]),
            UI_SIZE,
        )
        .tooltip(tr("Font size"))
        .dropdown_menu(move |menu, _, _| {
            FONT_SIZES.iter().fold(menu, |menu, &size| {
                let this = this.clone();
                menu.item(
                    crate::ui::menu::row(font_size_text(size))
                        .checked(chosen == size)
                        .on_click(move |_, _, cx| {
                            let _ = this.update(cx, |view, cx| view.set_font_size(size, cx));
                        }),
                )
            })
        })
    }

    /// One side's encoding (U16): its name bar's "UTF-8 · LF · 12 lines" as quiet text that
    /// opens the encodings, which the sub-bar's "Left: UTF-8 (auto)" used to. Before the first
    /// compare it reads "Auto" or the chosen encoding.
    pub(super) fn encoding_menu(
        &self,
        left: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let chosen = if left {
            self.request.left_encoding
        } else {
            self.request.right_encoding
        };
        let detected = self.model.as_ref().map(|m| {
            if left {
                m.left().encoding().encoding
            } else {
                m.right().encoding().encoding
            }
        });
        let label = self.model.as_ref().map_or_else(
            || encoding_text(chosen, detected),
            |m| {
                let doc = if left { m.left() } else { m.right() };
                let lines = count_of(m.line_count(left) as u64, "line", "lines");
                format!("{} · {lines}", doc_info(doc))
            },
        );
        let side = tr(if left { "Left" } else { "Right" });
        let this = cx.entity().downgrade();
        ld_text_dropdown(
            if left {
                "text-enc-left"
            } else {
                "text-enc-right"
            },
            label.clone(),
            11.,
            p.dim,
        )
        .accessibility_label(format!("{side}: {label}"))
        .tooltip(trf(
            "{} encoding: {}",
            &[&side, &encoding_text(chosen, detected)],
        ))
        .dropdown_menu(move |menu, _, _| {
            encoding_choices().fold(menu, |menu, (label, encoding)| {
                let this = this.clone();
                menu.item(
                    crate::ui::menu::row(label)
                        .checked(chosen == encoding)
                        .on_click(move |_, window, cx| {
                            let _ = this.update(cx, |view, cx| {
                                view.set_encoding(left, encoding, window, cx)
                            });
                        }),
                )
            })
        })
    }

    pub(super) fn render_find(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bar = self.find.as_ref()?;
        let scope = |id: &'static str, scope: FindScope, last: bool| {
            ld_segment(id, tr(scope.label()), bar.query.scope == scope, last, p).ld_click(
                true,
                cx.listener(move |this, _, _, cx| this.set_find_scope(scope, cx)),
            )
        };
        Some(
            h_flex()
                .id("text-find")
                .test_support()
                .flex_none()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .px(px(12.))
                .py(px(4.))
                .bg(p.panel3)
                .border_b_1()
                .border_color(p.line)
                .text_size(px(SMALL_SIZE))
                .child(
                    div()
                        .id("text-find-field")
                        .test_support()
                        .w(px(220.))
                        .tooltip(|window, cx| ld_tip(tr(FIND_TOOLTIP).into(), window, cx))
                        .child(
                            ld_input(&bar.input)
                                .aria_label(tr("Find"))
                                .when_some(self.find_field_border(p), |input, color| {
                                    input.border_color(color)
                                }),
                        ),
                )
                .child(ld_segmented(
                    "text-find-scope",
                    [
                        scope("text-find-both", FindScope::Both, false),
                        scope("text-find-left", FindScope::Left, false),
                        scope("text-find-right", FindScope::Right, true),
                    ],
                    p,
                ))
                .child(
                    ld_toggle("text-find-case", tr("Aa"), bar.query.match_case, p)
                        .ld_tooltip(tr("Match case"))
                        .ld_click(
                            true,
                            cx.listener(|this, _, _, cx| this.toggle_match_case(cx)),
                        ),
                )
                .child(
                    ld_toggle("text-find-word", tr("Word"), bar.query.whole_word, p)
                        .ld_tooltip(tr("Whole word"))
                        .ld_click(
                            true,
                            cx.listener(|this, _, _, cx| this.toggle_whole_word(cx)),
                        ),
                )
                .child(
                    ld_toggle("text-find-regex", ".*", bar.query.regex, p)
                        .ld_tooltip(tr("Regular expression"))
                        .ld_click(true, cx.listener(|this, _, _, cx| this.toggle_regex(cx))),
                )
                .child(
                    ld_icon_button(
                        "text-find-prev",
                        icons::PREV,
                        tr("Previous match (Shift+F3)"),
                        p,
                    )
                    .ld_click(
                        true,
                        cx.listener(|this, _, _, cx| this.step_find(false, cx)),
                    ),
                )
                .child(
                    ld_icon_button("text-find-next", icons::NEXT, tr("Next match (F3)"), p)
                        .ld_click(true, cx.listener(|this, _, _, cx| this.step_find(true, cx))),
                )
                .child(
                    div()
                        .id("text-find-status")
                        .test_support()
                        .px_1()
                        .text_color(p.muted)
                        .child(self.find_status().unwrap_or_default()),
                )
                .child(
                    div()
                        .id("text-find-replace")
                        .test_support()
                        .w(px(200.))
                        .child(ld_input(&bar.replace).aria_label(tr("Replace with"))),
                )
                .child(ld_button("text-replace-one", tr("Replace"), p).ld_click(
                    true,
                    cx.listener(|this, _, _, cx| {
                        let with = this.find.as_ref().map(|bar| bar.replace_text.clone());
                        this.replace_one(&with.unwrap_or_default(), cx);
                    }),
                ))
                .child(
                    ld_button("text-replace-all", tr("Replace all"), p).ld_click(
                        true,
                        cx.listener(|this, _, _, cx| {
                            let with = this.find.as_ref().map(|bar| bar.replace_text.clone());
                            this.replace_all(&with.unwrap_or_default(), cx);
                        }),
                    ),
                )
                .child(div().flex_1())
                .child(
                    ld_icon_button("text-find-close", icons::CLOSE, tr("Close (Esc)"), p).ld_click(
                        true,
                        cx.listener(|this, _, window, cx| this.close_bars(window, cx)),
                    ),
                )
                .into_any_element(),
        )
    }

    pub(super) fn render_goto(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bar = self.goto.as_ref()?;
        let side = |id: &'static str, label: &'static str, left: bool| {
            ld_segment(id, label, bar.left == left, !left, p).ld_click(
                true,
                cx.listener(move |this, _, _, cx| this.set_goto_side(left, cx)),
            )
        };
        let lines = self.model.as_ref().map_or(0, |m| m.line_count(bar.left));
        Some(
            h_flex()
                .id("text-goto")
                .test_support()
                .flex_none()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .px(px(12.))
                .py(px(4.))
                .bg(p.panel3)
                .border_b_1()
                .border_color(p.line)
                .text_size(px(SMALL_SIZE))
                .child(tr("Go to line"))
                .child(
                    div()
                        .w(px(120.))
                        .child(ld_input(&bar.input).aria_label(tr("Go to line"))),
                )
                .child(ld_segmented(
                    "text-goto-side",
                    [
                        side("text-goto-left", tr("Left"), true),
                        side("text-goto-right", tr("Right"), false),
                    ],
                    p,
                ))
                .child(
                    div()
                        .text_color(p.muted)
                        .child(trf("of {}", &[&format_size(lines as u64)])),
                )
                .children(bar.error.clone().map(|error| {
                    div()
                        .id("text-goto-error")
                        .test_support()
                        .text_color(p.del)
                        .prose(error)
                }))
                .into_any_element(),
        )
    }
}

impl TextView {
    /// The Syntax toggle's tooltip, from what each side's file is coloured as.
    pub(super) fn syntax_tooltip(&self) -> String {
        let languages = self.model.as_ref().map_or([None, None], |m| {
            [m.left().path(), m.right().path()]
                .map(|path| syntax_choice(path, self.language).label())
        });
        syntax_tooltip(self.syntax_state, languages)
    }
}

/// The Syntax toggle's tooltip: each side's language, or why it is plain, e.g.
/// "Left: rust. Right: over 10 MiB, so plain."
fn syntax_tooltip(states: [SyntaxState; 2], languages: [Option<&'static str>; 2]) -> String {
    let side = |name: &str, state: SyntaxState, language: Option<&str>| {
        let language = language.unwrap_or(tr(PLAIN_TEXT));
        match state {
            SyntaxState::Ready => trf("{}: {}.", &[&name, &language]),
            SyntaxState::Parsing => trf("{}: {}, parsing.", &[&name, &language]),
            SyntaxState::Off(PLAIN_TEXT) => trf("{}: plain text.", &[&name]),
            SyntaxState::Off(why) => trf("{}: {}, so plain.", &[&name, &tr(why)]),
        }
    };
    format!(
        "{} {}",
        side(tr("Left"), states[0], languages[0]),
        side(tr("Right"), states[1], languages[1])
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_sizes_print_as_integers_except_12_5() {
        let printed: Vec<_> = FONT_SIZES.iter().map(|&s| font_size_text(s)).collect();
        assert_eq!(
            printed,
            [
                "9", "10", "11", "12", "12.5", "13", "14", "15", "16", "18", "20", "22", "24"
            ]
        );
    }

    #[test]
    fn the_whitespace_menu_lists_every_mode_in_order() {
        let modes: Vec<_> = WHITESPACE_ITEMS.iter().map(|(mode, _)| *mode).collect();
        assert_eq!(modes, WhitespaceMode::ALL);
        assert_eq!(WHITESPACE_ITEMS[2].1, "Ignore leading whitespace");
        assert_eq!(WHITESPACE_ITEMS[4].1, "Ignore the amount of whitespace");
    }

    #[test]
    fn the_syntax_tooltip_names_each_sides_language_or_why_it_is_plain() {
        let tip = syntax_tooltip(
            [SyntaxState::Ready, SyntaxState::Off("over 10 MiB")],
            [Some("rust"), Some("rust")],
        );
        assert_eq!(tip, "Left: rust. Right: over 10 MiB, so plain.");
        let tip = syntax_tooltip(
            [SyntaxState::Parsing, SyntaxState::Off("plain text")],
            [Some("powershell"), None],
        );
        assert_eq!(tip, "Left: powershell, parsing. Right: plain text.");
    }
}
