//! The table view's find bar and go-to-row bar (LocalDiff `findNext`, `gotoRow`). Both are
//! small bars under the options row; the search and the jump run on the UI thread over the
//! in-memory tables. One search call looks at no more than `FIND_CELLS` cells, and F3 goes on.

use duet_core::table::nav::{FindResume, Found, Search, find, row_for_line};
use gpui_kit::AnyElement;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;

use super::*;
use crate::ui::fonts::SMALL_SIZE;
use crate::ui::icons;
use crate::ui::style::{LdExt as _, ld_icon_button};

const FIND_PLACEHOLDER: &str = "Find in every cell, both sides";
const GOTO_PLACEHOLDER: &str = "Line in the file (the header is line 1)";
const NO_LINE: &str = "Type a line number.";
const STILL_SEARCHING: &str = "Still searching: press F3 to go on.";

/// A bar's field and its Enter subscription.
pub(super) struct Bar {
    input: Entity<InputState>,
    _subscription: Subscription,
}

/// A find that stopped at its cells: what it looked for, which way, from where, and where it
/// stopped.
pub(super) struct Paused {
    text: String,
    forward: bool,
    from: Option<usize>,
    resume: FindResume,
}

impl TableView {
    fn new_bar(
        placeholder: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
        on_enter: impl Fn(&mut Self, bool, &mut Window, &mut Context<Self>) + 'static,
    ) -> Bar {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let _subscription = cx.subscribe_in(
            &input,
            window,
            move |this: &mut Self, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { shift, .. } = event {
                    on_enter(this, *shift, window, cx);
                }
            },
        );
        Bar {
            input,
            _subscription,
        }
    }

    /// Ctrl+F and the Find button: opens the bar and puts the keys in its field.
    pub(super) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.diff.is_none() {
            return;
        }
        self.goto = None;
        if self.find.is_none() {
            let bar = Self::new_bar(FIND_PLACEHOLDER, window, cx, |this, shift, _, cx| {
                this.step_find(!shift, cx)
            });
            let last = self.last_find.clone();
            bar.input
                .update(cx, |state, cx| state.set_value(last, window, cx));
            self.find = Some(bar);
        }
        if let Some(bar) = &self.find {
            window.focus(&bar.input.focus_handle(cx), cx);
        }
        cx.notify();
    }

    /// Ctrl+G: opens the go-to bar and puts the keys in its field.
    pub(super) fn open_goto(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.diff.is_none() {
            return;
        }
        self.find = None;
        if self.goto.is_none() {
            self.goto = Some(Self::new_bar(
                GOTO_PLACEHOLDER,
                window,
                cx,
                |this, _, window, cx| this.submit_goto(window, cx),
            ));
        }
        if let Some(bar) = &self.goto {
            window.focus(&bar.input.focus_handle(cx), cx);
        }
        cx.notify();
    }

    pub(super) fn bar_open(&self) -> bool {
        self.find.is_some() || self.goto.is_some()
    }

    /// Drops the bars; the keys go back to the rows.
    pub(super) fn close_bars(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find = None;
        self.goto = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The focus handles of the open bars' fields.
    pub(super) fn bar_inputs(&self, cx: &App) -> Vec<FocusHandle> {
        [&self.find, &self.goto]
            .into_iter()
            .flatten()
            .map(|bar| bar.input.focus_handle(cx))
            .collect()
    }

    /// F3 and Shift+F3: the next or previous match; with no query yet, the bar opens.
    pub(super) fn find_step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_none() && self.last_find.is_empty() {
            self.open_find(window, cx);
        } else {
            self.step_find(forward, cx);
        }
    }

    /// Finds the field's text (else the last query) in a cell of either side, from just past
    /// the selected row and wrapping once, and selects the cell.
    pub(super) fn step_find(&mut self, forward: bool, cx: &mut Context<Self>) {
        if self.diff.is_none() {
            return;
        }
        let text = match &self.find {
            Some(bar) => bar.input.read(cx).value().to_string(),
            None => self.last_find.clone(),
        };
        if text.is_empty() {
            return;
        }
        self.last_find.clone_from(&text);
        let from = self.selection.and_then(|(row, _)| self.view.position(row));
        // The same text, way and start go on from where the last call stopped.
        let resume = self
            .find_paused
            .take()
            .filter(|p| p.text == text && p.forward == forward && p.from == from)
            .map_or_else(FindResume::default, |p| p.resume);
        let search = Search {
            needle: &text,
            backward: !forward,
            from,
            resume,
            cells: self.find_cells,
        };
        let found = self.diff.as_ref().map(|diff| {
            find(
                diff,
                self.compared[LEFT].as_deref(),
                self.compared[RIGHT].as_deref(),
                self.compared_options.header,
                &self.view,
                &search,
            )
        });
        match found {
            Some(Found::At(pos, column)) => {
                self.notice = None;
                let row = self.view.rows[pos] as usize;
                self.select(row, Some(column), true, cx);
            }
            Some(Found::Paused(resume)) => {
                self.notice = Some(STILL_SEARCHING.into());
                self.find_paused = Some(Paused {
                    text,
                    forward,
                    from,
                    resume,
                });
                cx.notify();
            }
            Some(Found::Nothing) | None => {
                self.notice = Some(format!("Not found: {text}.").into());
                cx.notify();
            }
        }
    }

    /// Test hook: how many cells one find call may look at.
    #[doc(hidden)]
    pub fn set_find_cells_for_test(&mut self, cells: usize) {
        self.find_cells = cells;
    }

    /// Enter in the go-to field: selects the row at that file line.
    fn submit_goto(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bar) = &self.goto else {
            return;
        };
        let typed = bar.input.read(cx).value().trim().to_owned();
        if typed.is_empty() || !typed.bytes().all(|b| b.is_ascii_digit()) {
            self.notice = Some(NO_LINE.into());
            cx.notify();
            return;
        }
        let row = self.diff.as_ref().and_then(|diff| {
            let line = typed.parse().ok()?;
            row_for_line(
                diff,
                self.compared[LEFT].as_deref(),
                self.compared[RIGHT].as_deref(),
                self.compared_options.header,
                line,
            )
        });
        let Some(row) = row else {
            self.notice = Some(format!("No row at line {typed}.").into());
            cx.notify();
            return;
        };
        self.notice = None;
        if self.view.position(row).is_none() {
            self.set_prefs(|prefs| prefs.filter = Filter::All, cx);
        }
        let column = self
            .diff
            .as_ref()
            .and_then(|diff| diff.diff_columns(row).first().map(|&c| c as usize));
        self.close_bars(window, cx);
        self.select(row, column, true, cx);
    }

    #[doc(hidden)]
    pub fn find_input_for_test(&self) -> Option<Entity<InputState>> {
        self.find.as_ref().map(|bar| bar.input.clone())
    }

    #[doc(hidden)]
    pub fn goto_input_for_test(&self) -> Option<Entity<InputState>> {
        self.goto.as_ref().map(|bar| bar.input.clone())
    }

    fn bar_frame(
        id: &'static str,
        p: &Palette,
        children: impl IntoIterator<Item = AnyElement>,
    ) -> AnyElement {
        h_flex()
            .id(id)
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
            .children(children)
            .into_any_element()
    }

    /// The find bar under the options: the field, the two steps and close.
    pub(super) fn render_find(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bar = self.find.as_ref()?;
        Some(Self::bar_frame(
            "table-find-bar",
            p,
            [
                div()
                    .id("table-find-input")
                    .test_support()
                    .w(px(260.))
                    .child(Input::new(&bar.input).aria_label("Find"))
                    .into_any_element(),
                ld_icon_button("table-find-prev", icons::PREV, "Previous (Shift+F3)", p)
                    .ld_click(
                        true,
                        cx.listener(|this, _, _, cx| this.step_find(false, cx)),
                    )
                    .into_any_element(),
                ld_icon_button("table-find-next", icons::NEXT, "Next (F3)", p)
                    .ld_click(true, cx.listener(|this, _, _, cx| this.step_find(true, cx)))
                    .into_any_element(),
                div().flex_1().into_any_element(),
                ld_icon_button("table-find-close", icons::CLOSE, "Close (Esc)", p)
                    .ld_click(
                        true,
                        cx.listener(|this, _, window, cx| this.close_bars(window, cx)),
                    )
                    .into_any_element(),
            ],
        ))
    }

    /// The go-to bar under the options: the field and close.
    pub(super) fn render_goto(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bar = self.goto.as_ref()?;
        Some(Self::bar_frame(
            "table-goto",
            p,
            [
                div()
                    .id("table-goto-input")
                    .test_support()
                    .w(px(260.))
                    .child(Input::new(&bar.input).aria_label("Go to line"))
                    .into_any_element(),
                div().flex_1().into_any_element(),
                ld_icon_button("table-goto-close", icons::CLOSE, "Close (Esc)", p)
                    .ld_click(
                        true,
                        cx.listener(|this, _, window, cx| this.close_bars(window, cx)),
                    )
                    .into_any_element(),
            ],
        ))
    }
}
