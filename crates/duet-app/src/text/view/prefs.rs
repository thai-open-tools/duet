//! The remembered display and compare choices (R7, PF9): the setters that write them, one key
//! each.

use super::*;
use crate::settings;
use crate::text::prefs::{
    self, FilterKey, MOVES, SHOW_SPACES, SYNTAX, TAB_WIDTH, TAB_WIDTHS, TextPrefs, WRAP,
};

impl TextView {
    pub fn prefs(&self) -> &TextPrefs {
        &self.prefs
    }

    /// The tab stops are every `width` columns, 1 to 16 (R5).
    pub fn set_tab_width(&mut self, width: u8, cx: &mut Context<Self>) {
        let width = width.clamp(*TAB_WIDTHS.start(), *TAB_WIDTHS.end());
        self.display.tab_width = width.into();
        self.prefs.tab_width = width;
        self.remember(TAB_WIDTH, cx);
        cx.notify();
    }

    /// Spaces and tabs drawn as `·` and `→`.
    pub fn set_show_spaces(&mut self, on: bool, cx: &mut Context<Self>) {
        self.display.show_whitespace = on;
        self.prefs.show_spaces = on;
        self.remember(SHOW_SPACES, cx);
        cx.notify();
    }

    /// What a save does to the line endings (R13); every tab shares it.
    pub fn set_save_eol(&mut self, choice: prefs::SaveEol, cx: &mut Context<Self>) {
        self.pull_shared(cx);
        self.prefs.save_eol = choice;
        prefs::push_shared(&self.prefs, cx);
        self.remember(prefs::SAVE_EOL, cx);
        cx.notify();
    }

    /// What a save does to the encoding (R13); every tab shares it.
    pub fn set_save_encoding(&mut self, choice: prefs::SaveEncoding, cx: &mut Context<Self>) {
        self.pull_shared(cx);
        self.prefs.save_encoding = choice;
        prefs::push_shared(&self.prefs, cx);
        self.remember(prefs::SAVE_ENCODING, cx);
        cx.notify();
    }

    /// The choices every tab shares (the rules lists, context lines, On save), copied in
    /// before this tab compares, saves or shows them (I2).
    pub(crate) fn pull_shared(&mut self, cx: &App) {
        prefs::pull_shared(&mut self.prefs, cx);
    }

    /// Records one choice in `prefs` and writes just its key; a write error is logged, not
    /// shown.
    pub(crate) fn remember(&mut self, key: &'static str, cx: &mut Context<Self>) {
        let file = settings::settings_file(cx);
        if let Err(err) = self.prefs.save_key(key, file.as_deref()) {
            tracing::warn!(%err, key, "could not remember a text view choice");
        }
    }

    pub(crate) fn remember_wrap(&mut self, on: bool, cx: &mut Context<Self>) {
        self.prefs.wrap = on;
        self.remember(WRAP, cx);
    }

    pub(crate) fn remember_syntax(&mut self, on: bool, cx: &mut Context<Self>) {
        self.prefs.syntax = on;
        self.remember(SYNTAX, cx);
    }

    pub(crate) fn remember_moves(&mut self, on: bool, cx: &mut Context<Self>) {
        self.prefs.moves = on;
        self.remember(MOVES, cx);
    }

    pub(crate) fn remember_font_size(&mut self, cx: &mut Context<Self>) {
        self.prefs.font_size = self.font_size;
        self.remember(prefs::FONT_SIZE, cx);
    }

    pub(crate) fn remember_filter(&mut self, filter: TextFilter, cx: &mut Context<Self>) {
        self.prefs.filter = FilterKey::of(filter);
        self.remember(prefs::FILTER, cx);
    }

    /// The whitespace, case and line-ending options; only the keys that changed are written.
    pub(crate) fn remember_options(&mut self, options: DiffOptions, cx: &mut Context<Self>) {
        let p = &mut self.prefs;
        let changed = [
            (p.whitespace != options.whitespace, prefs::WHITESPACE),
            (p.ignore_case != options.ignore_case, prefs::IGNORE_CASE),
            (p.ignore_eol != options.ignore_eol, prefs::IGNORE_EOL),
        ];
        (p.whitespace, p.ignore_case, p.ignore_eol) =
            (options.whitespace, options.ignore_case, options.ignore_eol);
        for (_, key) in changed.into_iter().filter(|(changed, _)| *changed) {
            self.remember(key, cx);
        }
    }

    pub(crate) fn remember_ignore_blank(&mut self, on: bool, cx: &mut Context<Self>) {
        self.prefs.ignore_blank = on;
        self.remember(prefs::IGNORE_BLANK, cx);
    }

    /// The rules panel's lists and context size, for every tab; only the keys that changed are
    /// written. The caller made them from the shared ones.
    pub(crate) fn remember_rules(
        &mut self,
        (text, lines, context): (String, String, u32),
        cx: &mut Context<Self>,
    ) {
        self.pull_shared(cx);
        let p = &mut self.prefs;
        let changed = [
            (p.rules_text != text, prefs::RULES_TEXT),
            (p.rules_lines != lines, prefs::RULES_LINES),
            (p.context_lines != context, prefs::CONTEXT_LINES),
        ];
        (p.rules_text, p.rules_lines, p.context_lines) = (text, lines, context);
        prefs::push_shared(&self.prefs, cx);
        for (_, key) in changed.into_iter().filter(|(changed, _)| *changed) {
            self.remember(key, cx);
        }
    }

    pub(crate) fn remember_ignore_comments(&mut self, on: bool, cx: &mut Context<Self>) {
        self.prefs.ignore_comments = on;
        self.remember(prefs::IGNORE_COMMENTS, cx);
    }
}
