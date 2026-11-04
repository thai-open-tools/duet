//! The rules panel (R8, `ops:564-626`): a bar below the sub-bar with the two regex lists, the
//! presets and the context size, plus "Treat lines like this as unimportant" (R9, `ops:800`).

use duet_core::text::rules::compile_list;
use gpui_kit::component::input::{Textarea, TextareaState};

use super::*;
use crate::text::prefs::{DEFAULT_CONTEXT_LINES, MAX_CONTEXT_LINES};
use crate::ui::fonts::{CODE_SIZE, SMALL_SIZE};
use crate::ui::style::{RADIUS, ld_primary};

/// The text-rule presets (`ops:595-599`); each is added on a new line.
pub const PRESETS: [(&str, &str); 4] = [
    (
        "+ Date and time",
        r"\d{4}[-/.]\d{2}[-/.]\d{2}[ T]\d{1,2}:\d{2}(:\d{2})?([.,]\d+)?",
    ),
    (
        "+ GUID",
        r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
    ),
    ("+ Numbers", r"\d+"),
    ("+ Version", r"\d+\.\d+(\.\d+)*"),
];

const TEXT_HELP: &str = "Text that matches is left out before comparing. Lines that differ only there show purple and do not count.";
const LINES_HELP: &str = "Lines that match are paired as unimportant and do not count.";
const TEXT_PLACEHOLDER: &str = r"e.g. \d{4}-\d{2}-\d{2}[ T][\d:.,]+  (ignore dates and times)";
const LINES_PLACEHOLDER: &str = r"e.g. ^\s*//.*$  or  ^#.*generated";

/// The panel's state while it is open: its fields start from the saved rules and are thrown
/// away by Cancel and Esc.
pub(crate) struct RulesBar {
    text: Entity<TextareaState>,
    lines: Entity<TextareaState>,
    context: Entity<InputState>,
    error: Option<SharedString>,
    /// Which list holds the pattern Apply refused: the text list (true) or the lines list.
    error_in: Option<bool>,
    _subscription: Subscription,
}

fn rules_field(
    placeholder: &'static str,
    value: String,
    window: &mut Window,
    cx: &mut Context<TextView>,
) -> Entity<TextareaState> {
    cx.new(|cx| {
        let mut field = TextareaState::new(window, cx)
            .auto_grow(3, 8)
            .soft_wrap(false)
            .placeholder(placeholder);
        // Not `default_value`: that one counts the rows at paint, too late for this frame's
        // layout, and a notify during paint draws no next frame, so the box stays clipped.
        field.set_value(value, window, cx);
        field
    })
}

/// The context size typed in the panel: 0 to 50; anything that is not a number is the
/// default.
fn parse_context(typed: &str) -> u32 {
    typed
        .trim()
        .parse::<u64>()
        .map_or(DEFAULT_CONTEXT_LINES, |n| {
            n.min(MAX_CONTEXT_LINES.into()) as u32
        })
}

/// `list` with `pattern` on a line of its own after the text already there.
fn with_line(list: &str, pattern: &str) -> String {
    let kept = list.trim_end();
    if kept.is_empty() {
        pattern.to_owned()
    } else {
        format!("{kept}\n{pattern}")
    }
}

impl TextView {
    pub fn rules_open(&self) -> bool {
        self.rules.is_some()
    }

    /// The error of the last Apply, while the panel is open.
    pub fn rules_error(&self) -> Option<&str> {
        self.rules.as_ref()?.error.as_deref()
    }

    /// Either list holds a pattern: the Rules button is lit (`ops:640-641`).
    pub fn rules_lit(&self) -> bool {
        !self.prefs.rules_text.trim().is_empty() || !self.prefs.rules_lines.trim().is_empty()
    }

    /// Lines of context the Context filter shows.
    pub fn context_lines(&self) -> u32 {
        self.prefs.context_lines
    }

    /// Opens the panel with the saved rules in its fields and the focus in the first.
    pub fn open_rules(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rules.is_none() {
            // Another tab may have saved newer lists.
            self.pull_shared(cx);
            let text = rules_field(TEXT_PLACEHOLDER, self.prefs.rules_text.clone(), window, cx);
            let lines = rules_field(
                LINES_PLACEHOLDER,
                self.prefs.rules_lines.clone(),
                window,
                cx,
            );
            let typed = self.prefs.context_lines.to_string();
            let context = cx.new(|cx| InputState::new(window, cx).default_value(typed));
            let subscription = cx.subscribe_in(
                &context,
                window,
                |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.apply_rules(window, cx);
                    }
                },
            );
            self.rules = Some(RulesBar {
                text,
                lines,
                context,
                error: None,
                error_in: None,
                _subscription: subscription,
            });
        }
        if let Some(bar) = &self.rules {
            window.focus(&bar.text.focus_handle(cx), cx);
        }
        cx.notify();
    }

    /// Adds preset `index` on a new line after the text rules.
    pub fn add_preset(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(bar), Some((_, pattern))) = (&self.rules, PRESETS.get(index)) else {
            return;
        };
        bar.text.update(cx, |field, cx| {
            let text = with_line(field.value().as_ref(), pattern);
            field.set_value(text, window, cx);
        });
        cx.notify();
    }

    /// Checks both lists, then saves them, closes the panel and compares again. An invalid
    /// pattern keeps the panel open with its name and changes nothing.
    pub fn apply_rules(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(bar) = &self.rules else {
            return false;
        };
        let text = bar.text.read(cx).value().to_string();
        let lines = bar.lines.read(cx).value().to_string();
        let context = parse_context(&bar.context.read(cx).value());
        if let Some((in_text, err)) = [(true, &text), (false, &lines)]
            .into_iter()
            .find_map(|(in_text, list)| Some((in_text, compile_list(list).err()?)))
        {
            if let Some(bar) = &mut self.rules {
                bar.error = Some(format!("Invalid pattern: {err}").into());
                bar.error_in = Some(in_text);
            }
            cx.notify();
            return false;
        }
        self.pull_shared(cx);
        let changed = text != self.prefs.rules_text || lines != self.prefs.rules_lines;
        self.remember_rules((text, lines, context), cx);
        self.close_rules(window, cx);
        // The Context filter shows the new size at once.
        match self.model.as_ref().map(TextModel::filter) {
            Some(TextFilter::Context(_)) => self.set_filter(TextFilter::Context(context), cx),
            _ => {
                if let Some(TextFilter::Context(_)) = self.pending_filter {
                    self.pending_filter = Some(TextFilter::Context(context));
                }
            }
        }
        if changed {
            self.recompare(cx);
        }
        true
    }

    /// Cancel: the fields are thrown away, nothing is saved.
    pub fn cancel_rules(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_rules(window, cx);
    }

    pub(super) fn close_rules(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rules.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// The menu row's line, as an exact line rule on a new line of the line rules (R9, A10):
    /// saved, and the pair compared again. A placeholder half has no line.
    pub(crate) fn treat_menu_row_as_unimportant(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((row, left)) = self.menu_row.take() else {
            return;
        };
        let Some(model) = self.model.as_ref() else {
            return;
        };
        let Some(aligned) = model.aligned_row(row) else {
            return;
        };
        let Some(line) = (if left { aligned.left } else { aligned.right }) else {
            return;
        };
        let pattern = format!("^{}$", regex::escape(&model.line_text(left, line as usize)));
        // Added to the list every tab shares, not to an old copy of it (I2).
        self.pull_shared(cx);
        let lines = with_line(&self.prefs.rules_lines, &pattern);
        if let Some(bar) = &self.rules {
            // An open panel shows the new rule too, so a later Apply keeps it.
            bar.lines
                .update(cx, |field, cx| field.set_value(lines.clone(), window, cx));
        }
        let (text, context) = (self.prefs.rules_text.clone(), self.prefs.context_lines);
        self.remember_rules((text, lines, context), cx);
        self.recompare(cx);
        cx.notify();
    }

    /// Test hook: the rules saved and compared again, without the panel.
    #[doc(hidden)]
    pub fn set_rules_for_test(&mut self, text: &str, lines: &str, cx: &mut Context<Self>) {
        self.pull_shared(cx);
        let context = self.prefs.context_lines;
        self.remember_rules((text.to_owned(), lines.to_owned(), context), cx);
        self.recompare(cx);
    }

    /// Test hook: the panel's three fields, while it is open.
    #[doc(hidden)]
    pub fn set_rules_fields_for_test(
        &mut self,
        text: &str,
        lines: &str,
        context: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(bar) = &self.rules else {
            return;
        };
        bar.text
            .update(cx, |field, cx| field.set_value(text.to_owned(), window, cx));
        bar.lines.update(cx, |field, cx| {
            field.set_value(lines.to_owned(), window, cx)
        });
        bar.context.update(cx, |field, cx| {
            field.set_value(context.to_owned(), window, cx)
        });
    }

    /// Test hook: the first field's text, while the panel is open.
    #[doc(hidden)]
    pub fn rules_text_for_test(&self, cx: &App) -> Option<String> {
        Some(self.rules.as_ref()?.text.read(cx).value().to_string())
    }

    /// Test hook: the second field's text, while the panel is open.
    #[doc(hidden)]
    pub fn rules_lines_for_test(&self, cx: &App) -> Option<String> {
        Some(self.rules.as_ref()?.lines.read(cx).value().to_string())
    }

    /// A list field's border: `del` while it holds the pattern Apply refused (R12).
    fn rules_field_border(&self, text: bool, p: &Palette) -> gpui_kit::Hsla {
        if self.rules.as_ref().and_then(|bar| bar.error_in) == Some(text) {
            p.del
        } else {
            p.line
        }
    }

    /// Test hook: the border colour of the text list's field (`text`) or the lines list's.
    #[doc(hidden)]
    pub fn rules_border_for_test(&self, text: bool, cx: &App) -> gpui_kit::Hsla {
        self.rules_field_border(text, &Palette::current(cx))
    }

    pub(super) fn render_rules(
        &self,
        p: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let bar = self.rules.as_ref()?;
        let field = |id: &'static str, title: &'static str, state: &Entity<TextareaState>| {
            let border = self.rules_field_border(id == "text-rules-text", p);
            v_flex()
                .flex_1()
                .min_w(px(260.))
                .gap(px(3.))
                .child(div().text_color(p.muted).child(title))
                .child(
                    div()
                        .id(id)
                        .test_support()
                        .w_full()
                        .rounded(px(RADIUS))
                        .border_1()
                        .border_color(border)
                        .bg(p.bg)
                        .font(fonts.code.clone())
                        .child(
                            Textarea::new(state)
                                .appearance(false)
                                .text_size(px(CODE_SIZE)),
                        ),
                )
        };
        let help = |text: &'static str| div().text_color(p.muted).child(text);
        let presets = h_flex()
            .flex_wrap()
            .gap(px(6.))
            .children(PRESETS.iter().enumerate().map(|(index, (label, _))| {
                ld_button(format!("text-rules-preset-{index}"), *label, p)
                    .ld_small()
                    .ld_click(
                        true,
                        cx.listener(move |this, _, window, cx| this.add_preset(index, window, cx)),
                    )
            }));
        Some(
            v_flex()
                .id("text-rules")
                .test_support()
                .flex_none()
                .gap(px(6.))
                .px(px(12.))
                .py(px(6.))
                .bg(p.panel3)
                .border_b_1()
                .border_color(p.line)
                .text_size(px(SMALL_SIZE))
                .child(
                    h_flex()
                        .flex_wrap()
                        .items_start()
                        .gap(px(12.))
                        .child(
                            field(
                                "text-rules-text",
                                "Unimportant text (one regex per line)",
                                &bar.text,
                            )
                            .child(help(TEXT_HELP))
                            .child(presets),
                        )
                        .child(
                            field(
                                "text-rules-lines",
                                "Unimportant lines (one regex per whole line)",
                                &bar.lines,
                            )
                            .child(help(LINES_HELP)),
                        ),
                )
                .child(
                    h_flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.))
                        .child("Context lines")
                        .child(
                            div()
                                .id("text-rules-context")
                                .test_support()
                                .w(px(80.))
                                .child(Input::new(&bar.context).aria_label("Context lines")),
                        )
                        .children(bar.error.clone().map(|error| {
                            div()
                                .id("text-rules-error")
                                .test_support()
                                .text_color(p.del)
                                .child(error)
                        }))
                        .child(div().flex_1())
                        .child(ld_button("text-rules-cancel", "Cancel", p).ld_click(
                            true,
                            cx.listener(|this, _, window, cx| this.cancel_rules(window, cx)),
                        ))
                        .child(ld_primary("text-rules-apply", "Apply", p).ld_click(
                            true,
                            cx.listener(|this, _, window, cx| {
                                this.apply_rules(window, cx);
                            }),
                        )),
                )
                .into_any_element(),
        )
    }
}
