//! Syntax highlighting jobs of the text view: start, parse, land and keep, plus the test hooks.

use super::*;

/// Why a side is not coloured. A key: the tooltip draws it with `tr`.
pub(crate) const PLAIN_TEXT: &str = tr_key("plain text");
pub(crate) const TURNED_OFF: &str = tr_key("turned off");
pub(crate) const OVER_LIMIT: &str = tr_key("over 10 MiB");

/// How long a lexed side waits after the last edit before it is lexed again (PF10).
const RELEX_DELAY: Duration = Duration::from_millis(150);

impl TextView {
    /// A lexed side is lexed again `RELEX_DELAY` after the last edit (PF10), so a burst of
    /// typing starts one lex; until then its tokens only shift. Each edit re-arms the timer.
    pub(crate) fn schedule_relex(&mut self, left: bool, cx: &mut Context<Self>) {
        let swaps = self.syntax_swaps;
        self.relex[usize::from(!left)] = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RELEX_DELAY).await;
            let _ = this.update(cx, |view, cx| {
                // F9 moved the side, and this timer, to the other side.
                let left = left != ((view.syntax_swaps - swaps) % 2 == 1);
                // This runs inside the timer's task: it ends by itself.
                if let Some(task) = view.relex[usize::from(!left)].take() {
                    task.detach();
                }
                view.parse_syntax(left, cx);
            });
        }));
    }

    /// Parses one side in the background from no colors: after a load, or when Syntax is
    /// switched (there is nothing older to show).
    pub(crate) fn start_syntax(&mut self, left: bool, cx: &mut Context<Self>) {
        drop_syntax(self.syntax[usize::from(!left)].take(), cx);
        self.parse_syntax(left, cx);
    }

    /// (Re)parses one side in the background if it should be highlighted. Until the parse
    /// lands, rows keep drawing the side's current styles, shifted by the edits since.
    pub(crate) fn parse_syntax(&mut self, left: bool, cx: &mut Context<Self>) {
        let index = usize::from(!left);
        self.syntax_job[index].cancel();
        // This parse takes the place of a lex still waiting for a pause.
        self.relex[index] = None;
        let state = match &self.model {
            None => SyntaxState::Off(PLAIN_TEXT),
            Some(_) if !self.syntax_on => SyntaxState::Off(TURNED_OFF),
            Some(model) => {
                let doc = if left { model.left() } else { model.right() };
                let len = model
                    .buffer(left)
                    .map_or(doc.bytes().len(), |buffer| buffer.len());
                if len > MAX_SYNTAX_BYTES || doc.text().is_none() {
                    SyntaxState::Off(OVER_LIMIT)
                } else if let choice @ (SyntaxChoice::Tree(_) | SyntaxChoice::Lex(_)) =
                    syntax_choice(doc.path(), self.language)
                {
                    let text = model.buffer(left).map_or_else(
                        || doc.text().unwrap_or_default().to_owned(),
                        |buffer| buffer.text().to_owned(),
                    );
                    let version = model.versions()[index].unwrap_or(0);
                    let swaps = self.syntax_swaps;
                    let budget = self.syntax_budget;
                    // A tree-sitter parse cannot be interrupted, so a stale one finishes and the
                    // generation check drops it; a stale lex stops at the token (PF10).
                    let (generation, cancel) = self.syntax_job[index].start();
                    let background = cx.background_executor().clone();
                    cx.spawn(async move |this, cx| {
                        let side = background
                            .spawn(async move { SyntaxSide::build(choice, &text, budget, &cancel) })
                            .await;
                        if let Some(side) = side {
                            let _ = this.update(cx, |view, cx| {
                                view.land_syntax(left, swaps, generation, version, side, cx)
                            });
                        }
                    })
                    .detach();
                    SyntaxState::Parsing
                } else {
                    SyntaxState::Off(PLAIN_TEXT)
                }
            }
        };
        if let SyntaxState::Off(_) = state {
            drop_syntax(self.syntax[index].take(), cx);
        }
        self.syntax_state[index] = state;
        cx.notify();
    }

    /// A parse of `left`'s text at buffer `version` finished. Before the first edit both
    /// versions are 0; a side edited while it parsed is parsed again from its newer text.
    pub(super) fn land_syntax(
        &mut self,
        left: bool,
        swaps: u64,
        generation: u64,
        version: u64,
        side: SyntaxSide,
        cx: &mut Context<Self>,
    ) {
        let unused = self.keep_syntax(left, swaps, generation, version, side, cx);
        drop_syntax(unused, cx);
    }

    /// `land_syntax`'s choice: keeps the parse when it is still wanted. Returns the side it lets
    /// go of: the parse itself when it is stale, else the colors it replaces.
    pub(super) fn keep_syntax(
        &mut self,
        left: bool,
        swaps: u64,
        generation: u64,
        version: u64,
        side: SyntaxSide,
        cx: &mut Context<Self>,
    ) -> Option<SyntaxSide> {
        // F9 moved the parsed text, and its job, to the other side.
        let left = left != ((self.syntax_swaps - swaps) % 2 == 1);
        let index = usize::from(!left);
        if !self.syntax_job[index].is_current(generation) {
            return Some(side);
        }
        self.syntax_job[index].finish(generation);
        let Some(now) = self
            .model
            .as_ref()
            .map(|m| m.versions()[index].unwrap_or(0))
        else {
            return Some(side);
        };
        if now != version {
            // A lexed side edited since waits for its timer; anything else parses again now.
            if self.relex[index].is_none() {
                self.parse_syntax(left, cx);
            }
            return Some(side);
        }
        self.syntax_state[index] = SyntaxState::Ready;
        cx.notify();
        self.syntax[index].replace(side)
    }

    /// The Syntax button: colors on or off for both sides.
    pub fn set_syntax(&mut self, on: bool, cx: &mut Context<Self>) {
        self.syntax_on = on;
        self.remember_syntax(on, cx);
        self.start_syntax(true, cx);
        self.start_syntax(false, cx);
    }

    pub fn syntax_state(&self, left: bool) -> SyntaxState {
        self.syntax_state[usize::from(!left)]
    }

    /// Test hook: the UI-thread budget for re-parsing an edit, for both sides now and for the
    /// parses that follow (a zero budget makes every re-parse overrun).
    #[doc(hidden)]
    pub fn set_syntax_budget_for_test(&mut self, budget: Duration) {
        self.syntax_budget = budget;
        for side in self.syntax.iter_mut().flatten() {
            side.set_budget(budget);
        }
    }

    /// Test hook: a handle to one side's syntax colors that stops upgrading once they are dropped.
    #[doc(hidden)]
    pub fn syntax_witness_for_test(&self, left: bool) -> Option<Weak<()>> {
        self.syntax[usize::from(!left)]
            .as_ref()
            .map(SyntaxSide::witness)
    }

    /// Test hook: delivers a finished parse of `left`'s text at buffer `version`, as the newest
    /// parse of that side or (`newest` false) as one whose job was cancelled since.
    #[doc(hidden)]
    pub fn land_syntax_for_test(
        &mut self,
        left: bool,
        newest: bool,
        version: u64,
        side: SyntaxSide,
        cx: &mut Context<Self>,
    ) {
        let index = usize::from(!left);
        let (generation, _cancel) = self.syntax_job[index].start();
        if !newest {
            self.syntax_job[index].cancel();
        }
        self.land_syntax(left, self.syntax_swaps, generation, version, side, cx);
    }

    /// The syntax styles of one whole line, as rows draw them: while a parse runs after edits,
    /// the older styles shifted by them; empty when the side has no colors.
    #[doc(hidden)]
    pub fn line_styles_for_test(
        &self,
        left: bool,
        line: usize,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        match (&self.syntax[usize::from(!left)], &self.model) {
            (Some(side), Some(model)) => {
                let start = model.line_offset(left, line);
                let end = start + model.line_text(left, line).len();
                side.line_styles(start, end, &self.syntax_theme)
            }
            _ => Vec::new(),
        }
    }

    /// Test hook: how half `left` of display row `display` is coloured, built by the calls the
    /// rows make.
    #[doc(hidden)]
    pub fn half_look_for_test(&self, display: usize, left: bool) -> Option<rows::HalfLook> {
        let model = self.model.as_ref()?;
        let DisplayRow::Row(index) = model.display_row(display) else {
            return None;
        };
        let frame = RowFrame::new(
            &self.request,
            (self.caret, self.row_sel),
            self.find.as_ref(),
            &self.syntax,
            &self.syntax_theme,
            self.metrics(),
        );
        let [l, r] = frame.halves(model, display, index, model.row_at(index), None);
        // `render` keeps the syntax theme on the current palette's mode.
        let palette = Palette::of(self.syntax_theme.appearance.is_dark());
        Some(if left { l } else { r }.look(&palette))
    }

    /// Test hook: parses and lexes started on one side so far.
    #[doc(hidden)]
    pub fn syntax_starts_for_test(&self, left: bool) -> u64 {
        self.syntax_job[usize::from(!left)].generation()
    }

    #[doc(hidden)]
    pub fn syntax_tooltip_for_test(&self) -> String {
        self.syntax_tooltip()
    }

    #[doc(hidden)]
    pub fn syntax_label_for_test(&self, left: bool) -> Option<&'static str> {
        self.syntax[usize::from(!left)]
            .as_ref()
            .map(SyntaxSide::label)
    }
}
