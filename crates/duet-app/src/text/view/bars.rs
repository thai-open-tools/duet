//! The text view's find and go-to bars: their state, opening, stepping and closing.

use super::*;

impl TextView {
    pub fn find_open(&self) -> bool {
        self.find.is_some()
    }

    /// Test hook: the find bar's field, while the bar is open.
    #[doc(hidden)]
    pub fn find_input_for_test(&self) -> Option<Entity<InputState>> {
        self.find.as_ref().map(|bar| bar.input.clone())
    }

    /// "3 of 120", "120 matches", "No matches" or "Searching…"; None with the bar closed.
    pub fn find_status(&self) -> Option<String> {
        let bar = self.find.as_ref()?;
        let Some(result) = &bar.result else {
            return Some("Searching…".into());
        };
        if result.error.is_some() {
            return Some("Invalid pattern".into());
        }
        let total = result.hits.len();
        let plus = if result.truncated { "+" } else { "" };
        Some(match (total, bar.current) {
            (0, _) => "No matches".into(),
            (_, Some(i)) => format!(
                "{} of {}{plus}",
                format_size(i as u64 + 1),
                format_size(total as u64)
            ),
            (_, None) if !result.truncated => count_of(total as u64, "match", "matches"),
            (_, None) => format!("{}+ matches", format_size(total as u64)),
        })
    }

    pub(super) fn open_find(&mut self, _: &OpenFind, window: &mut Window, cx: &mut Context<Self>) {
        let prefill = self.find_prefill();
        if self.find.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find text"));
            let replace = cx.new(|cx| InputState::new(window, cx).placeholder("Replace with"));
            let subscriptions = vec![
                cx.subscribe_in(
                    &input,
                    window,
                    |this: &mut Self, input, event: &InputEvent, _, cx| match event {
                        InputEvent::Change => {
                            let text = input.read(cx).value().to_string();
                            this.set_find_text(text, cx);
                        }
                        InputEvent::PressEnter { shift, .. } => this.step_find(!*shift, cx),
                        _ => {}
                    },
                ),
                cx.subscribe_in(
                    &replace,
                    window,
                    |this: &mut Self, replace, event: &InputEvent, _, cx| {
                        if matches!(event, InputEvent::Change)
                            && let Some(bar) = &mut this.find
                        {
                            bar.replace_text = replace.read(cx).value().to_string();
                        }
                    },
                ),
            ];
            self.find = Some(FindBar {
                input,
                replace,
                replace_text: String::new(),
                query: FindQuery::default(),
                result: Some(FindResult::default()),
                current: None,
                resume: None,
                job: JobSlot::default(),
                task: None,
                replacing: None,
                _subscriptions: subscriptions,
            });
        }
        if let Some(text) = prefill
            && let Some(bar) = &mut self.find
        {
            // Selected, so typing replaces it (X-103).
            bar.input.update(cx, |field, cx| {
                field.set_value(text.clone(), window, cx);
                field.select_all(window, cx);
            });
            bar.query.text = text;
            self.run_find(cx);
        }
        if let Some(bar) = &self.find {
            window.focus(&bar.input.focus_handle(cx), cx);
        }
        cx.notify();
    }

    /// What Ctrl+F starts with: the caret's selection, else the line of a one-row selection,
    /// when it is one line under 200 characters (ops:276-277).
    fn find_prefill(&self) -> Option<String> {
        let one_line = |text: String| {
            (!text.is_empty() && !text.contains(['\n', '\r']) && text.chars().count() < 200)
                .then_some(text)
        };
        if self.caret.is_some() {
            return one_line(self.selected_text()?);
        }
        let sel = self.row_sel?;
        let model = self.model.as_ref()?;
        let lines = model.side_lines(sel.rows(), sel.left);
        if lines.len() != 1 {
            return None;
        }
        one_line(model.line_text(sel.left, lines.start as usize).into_owned())
    }

    pub fn set_find_text(&mut self, text: String, cx: &mut Context<Self>) {
        if let Some(bar) = &mut self.find {
            bar.query.text = text;
        }
        self.run_find(cx);
    }

    pub fn set_find_scope(&mut self, scope: FindScope, cx: &mut Context<Self>) {
        if let Some(bar) = &mut self.find {
            bar.query.scope = scope;
        }
        self.run_find(cx);
    }

    pub fn toggle_match_case(&mut self, cx: &mut Context<Self>) {
        if let Some(bar) = &mut self.find {
            bar.query.match_case = !bar.query.match_case;
        }
        self.run_find(cx);
    }

    pub fn toggle_whole_word(&mut self, cx: &mut Context<Self>) {
        if let Some(bar) = &mut self.find {
            bar.query.whole_word = !bar.query.whole_word;
        }
        self.run_find(cx);
    }

    pub fn toggle_regex(&mut self, cx: &mut Context<Self>) {
        if let Some(bar) = &mut self.find {
            bar.query.regex = !bar.query.regex;
        }
        self.run_find(cx);
    }

    /// The pattern error of the last search, while there is one.
    pub fn find_error(&self) -> Option<&str> {
        self.find.as_ref()?.result.as_ref()?.error.as_deref()
    }

    /// The find field's border while its pattern is invalid (R12); None keeps its own.
    pub(super) fn find_field_border(&self, p: &Palette) -> Option<gpui_kit::Hsla> {
        self.find_error().map(|_| p.del)
    }

    /// Test hook: the border colour the find field is drawn with, if not its own.
    #[doc(hidden)]
    pub fn find_border_for_test(&self, cx: &App) -> Option<gpui_kit::Hsla> {
        self.find_field_border(&Palette::current(cx))
    }

    /// Test hook: the current match as (left side, line).
    #[doc(hidden)]
    pub fn find_current_for_test(&self) -> Option<(bool, u32)> {
        let bar = self.find.as_ref()?;
        let hit = bar.result.as_ref()?.hits.get(bar.current?)?;
        Some((hit.left, hit.line))
    }

    /// Test hook: the replacement text, as if typed in the replace field.
    #[doc(hidden)]
    pub fn set_replace_text_for_test(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(bar) = &mut self.find {
            bar.replace_text = text.to_owned();
            cx.notify();
        }
    }

    /// Starts a background search for the bar's query; a newer search cancels this one.
    pub(crate) fn run_find(&mut self, cx: &mut Context<Self>) {
        let (Some(bar), Some(model)) = (self.find.as_mut(), self.model.as_ref()) else {
            return;
        };
        let (generation, cancel) = bar.job.start();
        bar.result = None;
        bar.current = None;
        let query = bar.query.clone();
        let (left, right, rows) = model.sources();
        let background = cx.background_executor().clone();
        bar.task = Some(cx.spawn(async move |this, cx| {
            let found = background
                .spawn(async move { find(&left, &right, &rows, &query, &cancel) })
                .await;
            let _ = this.update(cx, |view, cx| {
                if let Some(bar) = view
                    .find
                    .as_mut()
                    .filter(|bar| bar.job.is_current(generation))
                {
                    bar.job.finish(generation);
                    let landed = found.is_ok();
                    if let Ok(result) = found {
                        bar.result = Some(result);
                    }
                    if landed && let Some(from) = bar.resume.take() {
                        view.select_hit_from(from, cx);
                    }
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    pub(crate) fn step_find(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(bar) = self.find.as_mut() else {
            return;
        };
        let Some(count) = bar.result.as_ref().map(|r| r.hits.len()).filter(|&n| n > 0) else {
            return;
        };
        let next = match bar.current {
            None if forward => 0,
            None => count - 1,
            Some(i) if forward => (i + 1) % count,
            Some(i) => (i + count - 1) % count,
        };
        self.select_hit(next, cx);
    }

    /// Makes hit `index` current and scrolls to it.
    fn select_hit(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(bar) = self.find.as_mut() else {
            return;
        };
        bar.current = Some(index);
        let row = bar.result.as_ref().map_or(0, |r| r.hits[index].row);
        if let Some(display) = self.model.as_mut().and_then(|m| m.reveal_row(row)) {
            self.scroll_to_display(display, ScrollStrategy::Center);
        }
        cx.notify();
    }

    /// Makes the first hit at or after `from` (row, left side, byte in the line) current,
    /// wrapping to the first: where a replace left off.
    pub(crate) fn select_hit_from(&mut self, from: (u32, bool, usize), cx: &mut Context<Self>) {
        let Some(result) = self.find.as_ref().and_then(|bar| bar.result.as_ref()) else {
            return;
        };
        if result.hits.is_empty() {
            return;
        }
        let key = |row: u32, left: bool, at: usize| (row, !left, at);
        let index = result
            .hits
            .iter()
            .position(|h| key(h.row, h.left, h.range.start) >= key(from.0, from.1, from.2))
            .unwrap_or(0);
        self.select_hit(index, cx);
    }

    pub(super) fn find_next(&mut self, _: &FindNext, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_none() {
            return self.open_find(&OpenFind, window, cx);
        }
        self.step_find(true, cx);
    }

    pub(super) fn find_prev(&mut self, _: &FindPrev, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_none() {
            return self.open_find(&OpenFind, window, cx);
        }
        self.step_find(false, cx);
    }

    pub fn goto_open(&self) -> bool {
        self.goto.is_some()
    }

    pub fn goto_error(&self) -> Option<&str> {
        self.goto.as_ref().and_then(|b| b.error.as_deref())
    }

    pub fn marked_row(&self) -> Option<u32> {
        self.marked
    }

    pub(super) fn open_goto(&mut self, _: &OpenGoTo, window: &mut Window, cx: &mut Context<Self>) {
        self.find = None;
        if self.goto.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Line number"));
            let subscription = cx.subscribe_in(
                &input,
                window,
                |this: &mut Self, input, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        let text = input.read(cx).value().to_string();
                        let left = this.goto.as_ref().is_none_or(|b| b.left);
                        if this.go_to_line(left, &text, cx) {
                            this.close_bars(window, cx);
                        }
                    }
                },
            );
            self.goto = Some(GoToBar {
                input,
                left: true,
                error: None,
                _subscription: subscription,
            });
        }
        if let Some(bar) = &self.goto {
            window.focus(&bar.input.focus_handle(cx), cx);
        }
        cx.notify();
    }

    pub fn set_goto_side(&mut self, left: bool, cx: &mut Context<Self>) {
        if let Some(bar) = &mut self.goto {
            bar.left = left;
            bar.error = None;
            cx.notify();
        }
    }

    /// Scrolls to `text` (a 1-based line number) on one side and marks that row; on bad input
    /// the bar shows why and nothing moves.
    pub fn go_to_line(&mut self, left: bool, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(model) = self.model.as_mut() else {
            return false;
        };
        // The buffer's count: the documents lag behind edits until the re-diff lands.
        match parse_line(text, model.line_count(left)) {
            Ok(line) => {
                let row = model
                    .row_of_line(left, line)
                    .expect("parse_line clamps to the side");
                if let Some(display) = model.reveal_row(row) {
                    self.scroll_to_display(display, ScrollStrategy::Center);
                }
                self.marked = Some(row);
                if let Some(bar) = &mut self.goto {
                    bar.error = None;
                }
                cx.notify();
                true
            }
            Err(message) => {
                if let Some(bar) = &mut self.goto {
                    bar.error = Some(message.into());
                }
                cx.notify();
                false
            }
        }
    }

    /// Closes the find, go-to and rules bars and gives focus back to the rows.
    pub fn close_bars(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find = None;
        self.goto = None;
        self.rules = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }
}

/// The find bar's state while it is open.
pub(crate) struct FindBar {
    pub(crate) input: Entity<InputState>,
    pub(crate) replace: Entity<InputState>,
    /// The replace field's text.
    pub(crate) replace_text: String,
    pub(crate) query: FindQuery,
    /// None while a search runs.
    pub(crate) result: Option<FindResult>,
    pub(crate) current: Option<usize>,
    /// After a replace: the first hit at or after this (row, left side, byte) becomes current
    /// when the next search lands.
    pub(crate) resume: Option<(u32, bool, usize)>,
    pub(crate) job: JobSlot,
    pub(crate) task: Option<Task<()>>,
    /// The background build of a replace all, until it lands.
    pub(crate) replacing: Option<Task<()>>,
    pub(crate) _subscriptions: Vec<Subscription>,
}

/// The go-to-line bar's state while it is open.
pub(crate) struct GoToBar {
    pub(super) input: Entity<InputState>,
    pub(super) left: bool,
    pub(super) error: Option<SharedString>,
    pub(super) _subscription: Subscription,
}
