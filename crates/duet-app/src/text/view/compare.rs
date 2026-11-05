//! The compare jobs of the text view: reload, apply, swap, re-diff and the options that re-run them.

use super::*;
use duet_core::text::rules::{TextRules, compile_list};

impl TextView {
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.notice = None;
        self.marked = None;
        self.sync_rules(cx);
        // A re-diff, composition or plan of the buffers this replaces must never reach the
        // result.
        self.drop_buffer_state();
        // A pair over 64 MiB costs hundreds of MB; holding it while the new compare builds
        // its own would double the peak.
        if self.mapped.contains(&true)
            && let Some(model) = self.model.take()
        {
            self.pending_filter = Some(model.filter());
        }
        let (generation, cancel) = self.job.start();
        let request = self.request.clone();
        let background = cx.background_executor().clone();
        self.task = Some(cx.spawn(async move |this, cx| {
            let outcome = background
                .spawn(async move { run(&request, &cancel) })
                .await;
            let _ = this.update(cx, |view, cx| view.apply(generation, outcome, cx));
        }));
        cx.notify();
    }

    pub(super) fn apply(&mut self, generation: u64, outcome: TextOutcome, cx: &mut Context<Self>) {
        if !self.job.is_current(generation) {
            return;
        }
        self.job.finish(generation);
        self.watch.reloaded();
        // The bars may change under a drag in progress; it ends with the old result.
        self.dragging_hscroll = false;
        self.dragging_overview = false;
        // Typing during the reload went into the old buffers.
        self.drop_buffer_state();
        if self.notice.as_deref() == Some(tr(WAIT_NOTICE)) {
            self.notice = None;
        }
        let pending = self.pending_filter.take();
        let filter = self.model.as_ref().map(TextModel::filter).or(pending);
        match outcome {
            TextOutcome::Ready(model) => {
                let mut model = *model;
                // Built on the job's thread: said again here, in the language now in force.
                model.relabel_notes();
                let mapped =
                    |doc: &TextDocument| doc.read_only_reason() == Some(ReadOnlyReason::TooLarge);
                self.mapped = [mapped(model.left()), mapped(model.right())];
                if !self
                    .model
                    .as_ref()
                    .is_some_and(|old| same_text(old, &model))
                {
                    self.undo.clear();
                }
                if let Some(old) = &self.model {
                    model.set_expanded(old.expanded().to_vec());
                }
                if let Some(filter) = filter {
                    model.set_filter(filter);
                }
                self.caret = self.caret.and_then(|c| carried_caret(&mut model, c));
                self.row_sel = None;
                self.menu_row = None;
                self.model = Some(model);
                self.note_programmatic_scroll();
                self.check_wrap_limit();
                self.widest.set(0);
                self.clear_line_widths();
                self.message = None;
                self.message_hint = None;
                self.run_find(cx);
            }
            TextOutcome::Binary { identical } => {
                self.model = None;
                self.menu_row = None;
                self.caret = None;
                self.set_message(
                    live(if identical {
                        "Binary files are identical."
                    } else {
                        "Binary files differ."
                    }),
                    BINARY_HINT,
                );
            }
            TextOutcome::TooLargeAsText => {
                // The binary notice stays; it is the same pair, so Reload compares as before.
                self.request.force_text = false;
                self.model = None;
                self.menu_row = None;
                self.caret = None;
                if self.message.is_none() {
                    self.set_message(live("Binary files differ."), BINARY_HINT);
                }
                self.notice = Some(tr(TOO_LARGE_AS_TEXT).into());
            }
            TextOutcome::Failed(mut message) => {
                message.relabel();
                self.model = None;
                self.menu_row = None;
                self.caret = None;
                self.set_message(message, FAILED_HINT);
            }
        }
        self.start_syntax(true, cx);
        self.start_syntax(false, cx);
        self.start_odd(true, false, cx);
        self.start_odd(false, false, cx);
        cx.notify();
    }

    /// F9: exchanges the sides. The model on screen flips in place; a running compare also
    /// restarts with the sides swapped, so its late result can never land the wrong way round.
    pub fn swap_sides(&mut self, cx: &mut Context<Self>) {
        self.flip_sides(cx);
        cx.emit(TextEvent::Swapped {
            left: self.request.left.clone(),
            right: self.request.right.clone(),
        });
        cx.notify();
    }

    /// Takes back a swap the workspace refused: the sides flip back, so the tab still shows the
    /// pair its key names, and `notice` says why.
    pub fn undo_swap(&mut self, notice: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.flip_sides(cx);
        self.notice = Some(notice.into());
        cx.notify();
    }

    pub(super) fn flip_sides(&mut self, cx: &mut Context<Self>) {
        let request = &mut self.request;
        std::mem::swap(&mut request.left, &mut request.right);
        std::mem::swap(&mut request.left_encoding, &mut request.right_encoding);
        self.mapped.swap(0, 1);
        self.watch.swap();
        self.marked = None;
        self.clear_line_widths();
        if let Some(caret) = &mut self.caret {
            caret.left = !caret.left;
        }
        if let Some(sel) = &mut self.row_sel {
            sel.left = !sel.left;
        }
        self.menu_row = None;
        self.align = None;
        // A Replace all or Reformat being planned was made from the sides as they were (C1).
        self.stop_plans();
        for anchor in &mut self.request.rules.anchors {
            *anchor = (anchor.1, anchor.0);
        }
        if let Some(bar) = &mut self.find {
            bar.query.scope = match bar.query.scope {
                FindScope::Left => FindScope::Right,
                FindScope::Right => FindScope::Left,
                FindScope::Both => FindScope::Both,
            };
        }
        // Flip what is on screen at once, so labels and rows always agree, even if a running
        // compare is then cancelled; a running compare restarts with the sides swapped.
        let running = self.job.is_running();
        if let Some(model) = &mut self.model {
            model.swap_sides();
        }
        self.undo.swap_sides();
        self.syntax.swap(0, 1);
        self.syntax_state.swap(0, 1);
        self.syntax_job.swap(0, 1);
        self.relex.swap(0, 1);
        self.odd.swap(0, 1);
        self.reorder_odd();
        self.syntax_swaps += 1;
        if running || self.model.is_none() {
            self.reload(cx);
        } else {
            // A pending re-diff of the unswapped buffers would land the wrong way round.
            if self.rediff_job.is_running() {
                self.schedule_rediff(cx);
            }
            self.run_find(cx);
        }
    }

    /// The whitespace, case and line-ending options; each tab after this starts with them.
    pub fn set_diff_options(&mut self, options: DiffOptions, cx: &mut Context<Self>) {
        self.request.options = options;
        self.remember_options(options, cx);
        self.recompare(cx);
    }

    /// An option that only changes how the text is compared: edited buffers are re-diffed in
    /// place, otherwise the files are read again. Either way the rules are built again first.
    pub(crate) fn recompare(&mut self, cx: &mut Context<Self>) {
        if self
            .model
            .as_ref()
            .is_some_and(|m| m.buffer(true).is_some() || m.buffer(false).is_some())
        {
            self.sync_rules(cx);
            self.schedule_rediff(cx);
        } else {
            self.reload(cx);
        }
    }

    /// The rules the next compare uses, from the remembered choices (R4): blank lines, the
    /// rules panel's two lists (as every tab shares them, I2), and the comments of the Type
    /// menu's language (else the detected one) while Comments is on.
    fn sync_rules(&mut self, cx: &App) {
        self.pull_shared(cx);
        self.request.rules = TextRules {
            ignore_blank: self.prefs.ignore_blank,
            // A list that does not compile (a hand-edited settings file) is left out; the
            // panel checks what it saves.
            text: compile_list(&self.prefs.rules_text).unwrap_or_default(),
            lines: compile_list(&self.prefs.rules_lines).unwrap_or_default(),
            comments: self.prefs.ignore_comments.then(|| self.comment_language()),
            // Manual alignments belong to the pair, not to the remembered choices.
            anchors: std::mem::take(&mut self.request.rules.anchors),
        };
    }

    /// Whether a rule beyond whitespace, case and line endings is on (comments only count for a
    /// language that has them).
    pub fn rules_active(&self) -> bool {
        self.request.rules.is_active()
    }

    /// Blank lines are ignored: they pair as unimportant rows and do not count (R3).
    pub fn set_ignore_blank(&mut self, on: bool, cx: &mut Context<Self>) {
        self.remember_ignore_blank(on, cx);
        self.recompare(cx);
    }

    /// Comments are cut by the lexer of `comment_language()` (R7). Plain text has none, so the
    /// notice says to pick a type (`ops:994`); JSON, a log or a patch has no comment syntax, so
    /// the notice says nothing is ignored (X-73).
    pub fn set_ignore_comments(&mut self, on: bool, cx: &mut Context<Self>) {
        self.remember_ignore_comments(on, cx);
        self.recompare(cx);
        let language = self.comment_language();
        if on && !language.has_comments() {
            self.notice = Some(if language == Language::Text {
                tr(UNKNOWN_TYPE).into()
            } else {
                trf(
                    "{} files have no comments, so nothing is ignored.",
                    &[&language.label()],
                )
                .into()
            });
            cx.notify();
        }
    }

    /// Reads one side as `encoding` (None = detect). A side over 64 MiB is read straight from
    /// disk, so its encoding cannot change.
    pub fn set_encoding(
        &mut self,
        left: bool,
        encoding: Option<&'static Encoding>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mapped[if left { 0 } else { 1 }] {
            self.notice = Some(
                tr("The encoding of a file over 64 MiB can't be changed: it is read straight from disk.")
                    .into(),
            );
            cx.notify();
            return;
        }
        self.reload_guarded(window, cx, move |view| {
            if left {
                view.request.left_encoding = encoding;
            } else {
                view.request.right_encoding = encoding;
            }
        });
    }

    pub fn cycle_encoding(&mut self, left: bool, window: &mut Window, cx: &mut Context<Self>) {
        let current = if left {
            self.request.left_encoding
        } else {
            self.request.right_encoding
        };
        self.set_encoding(left, next_encoding(current), window, cx);
    }

    /// The Format button: compare pretty-printed JSON or XML. Not for files over 64 MiB.
    pub fn set_format(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        if on && self.mapped.contains(&true) {
            self.notice = Some(tr("Format is not available for files over 64 MiB.").into());
            cx.notify();
            return;
        }
        self.reload_guarded(window, cx, move |view| view.request.format = on);
    }

    /// The Sort keys button: compare JSON or YAML with its keys sorted and indented the same.
    /// Not for files over 64 MiB.
    pub fn set_sort_keys(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        if on && self.mapped.contains(&true) {
            self.notice = Some(tr("Sort keys is not available for files over 64 MiB.").into());
            cx.notify();
            return;
        }
        self.reload_guarded(window, cx, move |view| {
            view.request.sort_keys = on;
            // A pin names a line of the text as it was; the sorted text has other lines.
            view.request.rules.anchors.clear();
        });
    }

    /// `set_sort_keys` without the unsaved-edits prompt, for tests that have nothing to save.
    #[doc(hidden)]
    pub fn set_sort_keys_for_test(&mut self, on: bool, cx: &mut Context<Self>) {
        self.request.sort_keys = on;
        self.request.rules.anchors.clear();
        self.reload(cx);
    }

    /// `set_format` without the unsaved-edits prompt, for tests that have nothing to save.
    #[doc(hidden)]
    pub fn set_format_for_test(&mut self, on: bool, cx: &mut Context<Self>) {
        self.request.format = on;
        self.reload(cx);
    }

    pub fn set_detect_moves(&mut self, on: bool, cx: &mut Context<Self>) {
        self.request.detect_moves = on;
        self.remember_moves(on, cx);
        self.recompare(cx);
    }

    pub fn toggle_ignore(&mut self, choice: IgnoreChoice, cx: &mut Context<Self>) {
        let options = apply_ignore(self.request.options, choice);
        self.set_diff_options(options, cx);
    }

    /// The whitespace menu: one of its six modes (R2).
    pub fn set_whitespace(&mut self, mode: WhitespaceMode, cx: &mut Context<Self>) {
        if self.request.options.whitespace != mode {
            let options = DiffOptions {
                whitespace: mode,
                ..self.request.options
            };
            self.set_diff_options(options, cx);
        }
    }
}

/// `set_ignore_comments` on a file of no known type (`ops:994`).
const UNKNOWN_TYPE: &str =
    tr_key("This file's type is unknown: pick one under Type so comments can be found.");
