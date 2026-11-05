//! Find in the hex view (R8): the find bar, and the search of one side on the background
//! executor. This file is a child module of `view` (see its `#[path]`), so it reaches the view's
//! private state.

use duet_core::hex::HexSource;
use duet_core::hex::find::{Direction, find, parse_pattern};
use gpui_kit::AnyElement;

use super::*;
use crate::i18n::{tr, tr_key, trf};
use crate::ui::edit_menu::ld_input;
use crate::ui::fonts::SMALL_SIZE;
use crate::ui::icons;
use crate::ui::style::{LdExt as _, ld_icon_button, ld_segment, ld_segmented, ld_tip};

const FIND_TOOLTIP: &str = tr_key("Enter: next match. Shift+Enter: previous match.");
const WRAPPED_START: &str = tr_key("Wrapped to the start of the file.");
const WRAPPED_END: &str = tr_key("Wrapped to the end of the file.");

/// The find bar's state while it is open.
pub(super) struct FindBar {
    input: Entity<InputState>,
    /// Hex bytes (true) or text (false).
    hex: bool,
    /// The left file is searched (true) or the right one.
    left: bool,
    _subscription: Subscription,
}

/// The next match on `source`. With `inclusive` (no cursor yet), a forward search may match at
/// `from` itself: it starts at the top row, which has not been looked at. `done` counts the
/// bytes looked at, for the status line's percentage.
fn search(
    source: &HexSource,
    needle: &[u8],
    from: u64,
    inclusive: bool,
    dir: Direction,
    cancel: &CancelToken,
    done: &AtomicU64,
) -> Option<(u64, bool)> {
    if inclusive && dir == Direction::Forward && !needle.is_empty() {
        let end = from.saturating_add(needle.len() as u64);
        if end <= source.len() && source.bytes(from..end) == needle {
            return Some((from, false));
        }
    }
    find(source, needle, from, dir, cancel, done)
}

impl HexView {
    /// Ctrl+F: opens the bar (on the cursor's side) and puts the keys in its field.
    pub(super) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pair.is_none() {
            return;
        }
        if self.find.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("Find")));
            let subscription = cx.subscribe_in(
                &input,
                window,
                |this: &mut Self, _, event: &InputEvent, _, cx| {
                    if let InputEvent::PressEnter { shift, .. } = event {
                        this.step_find(!*shift, cx);
                    }
                },
            );
            self.find = Some(FindBar {
                input,
                hex: true,
                left: self.cursor.is_none_or(|c| c.0),
                _subscription: subscription,
            });
        }
        if let Some(bar) = &self.find {
            window.focus(&bar.input.focus_handle(cx), cx);
        }
        cx.notify();
    }

    /// The language changed: the find field's placeholder is said again.
    pub(super) fn relabel_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(bar) = &self.find {
            bar.input.update(cx, |state, cx| {
                state.set_placeholder(tr("Find"), window, cx)
            });
        }
    }

    /// Stops a running search and drops the bar; the keys go back to the rows.
    pub(super) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_find();
        self.find = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// F3 and Shift+F3: the next or previous match; the bar opens first when it is closed.
    pub(super) fn find_step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_none() {
            self.open_find(window, cx);
        } else {
            self.step_find(forward, cx);
        }
    }

    /// Whether the find field has the keys.
    pub(super) fn find_focused(&self, window: &Window, cx: &App) -> bool {
        self.find
            .as_ref()
            .is_some_and(|bar| bar.input.focus_handle(cx).is_focused(window))
    }

    pub fn find_open(&self) -> bool {
        self.find.is_some()
    }

    /// A search is running.
    pub fn is_finding(&self) -> bool {
        self.find_job.is_running()
    }

    /// Stops a running search; nothing moves.
    pub(super) fn cancel_find(&mut self) {
        self.find_job.cancel();
        self.find_task = None;
        self.find_ticker = None;
    }

    /// Searches the chosen side from just past the cursor (or from the top row), forward or
    /// backward, wrapping once.
    pub(super) fn step_find(&mut self, forward: bool, cx: &mut Context<Self>) {
        let (Some(bar), Some(pair)) = (&self.find, self.pair.clone()) else {
            return;
        };
        self.notice = None;
        let typed = bar.input.read(cx).value().to_string();
        let (left, hex) = (bar.left, bar.hex);
        let pattern = match parse_pattern(&typed, hex) {
            Ok(pattern) => pattern,
            Err(message) => {
                self.notice = Some(message.into());
                cx.notify();
                return;
            }
        };
        let (from, inclusive) = match self.cursor {
            Some((_, at)) => (at, false),
            None => (self.top_byte(), true),
        };
        let dir = if forward {
            Direction::Forward
        } else {
            Direction::Backward
        };
        let (generation, cancel) = self.find_job.start();
        let needle = pattern.needle().to_vec();
        let len = needle.len() as u64;
        // "Searching… N%" in the status line, the same way the compare shows its progress.
        let progress: Arc<AtomicU64> = Arc::default();
        self.find_progress = progress.clone();
        self.find_total = if left {
            pair.left.len()
        } else {
            pair.right.len()
        };
        self.find_ticker = Some(Self::ticker(Self::is_finding, cx));
        let background = cx.background_executor().clone();
        self.find_task = Some(cx.spawn(async move |this, cx| {
            let found = background
                .spawn(async move {
                    let source = if left { &pair.left } else { &pair.right };
                    search(source, &needle, from, inclusive, dir, &cancel, &progress)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.found(generation, left, dir, len, typed.trim(), found, cx)
            });
        }));
        cx.notify();
    }

    /// A search finished: the cursor goes to the match and the match is selected.
    #[allow(clippy::too_many_arguments)]
    fn found(
        &mut self,
        generation: u64,
        left: bool,
        dir: Direction,
        len: u64,
        typed: &str,
        found: Option<(u64, bool)>,
        cx: &mut Context<Self>,
    ) {
        // Stopped, replaced by a newer search, or the files were opened again.
        if !self.find_job.is_current(generation) {
            return;
        }
        self.find_job.finish(generation);
        match found {
            Some((at, wrapped)) => {
                self.place_cursor(left, at, cx);
                self.selection = Some((left, at..at + len));
                // The cursor move may have said "Showing all rows…": both notices stay.
                let shown = self.notice.take();
                let wrap = wrapped.then_some(match dir {
                    Direction::Forward => tr(WRAPPED_START),
                    Direction::Backward => tr(WRAPPED_END),
                });
                self.notice = match (shown, wrap) {
                    (Some(shown), Some(wrap)) => Some(format!("{shown} {wrap}").into()),
                    (shown, wrap) => shown.or_else(|| wrap.map(Into::into)),
                };
            }
            None => self.notice = Some(trf("Not found: {}.", &[&typed]).into()),
        }
        cx.notify();
    }

    /// Test hook: delivers a forward match at `at` as if a search had just found it.
    #[doc(hidden)]
    pub fn found_for_test(&mut self, left: bool, at: u64, len: u64, cx: &mut Context<Self>) {
        let (generation, _) = self.find_job.start();
        self.found(
            generation,
            left,
            Direction::Forward,
            len,
            "",
            Some((at, false)),
            cx,
        );
    }

    fn set_find_hex(&mut self, hex: bool, cx: &mut Context<Self>) {
        if let Some(bar) = &mut self.find {
            bar.hex = hex;
            cx.notify();
        }
    }

    fn set_find_left(&mut self, left: bool, cx: &mut Context<Self>) {
        if let Some(bar) = &mut self.find {
            bar.left = left;
            cx.notify();
        }
    }

    /// The find bar under the toolbar: the field, what it holds, which file, and the steps.
    pub(super) fn render_find(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bar = self.find.as_ref()?;
        let mode = |id: &'static str, label: &'static str, hex: bool, last: bool| {
            ld_segment(id, label, bar.hex == hex, last, p).ld_click(
                true,
                cx.listener(move |this, _, _, cx| this.set_find_hex(hex, cx)),
            )
        };
        let side = |id: &'static str, label: &'static str, left: bool, last: bool| {
            ld_segment(id, label, bar.left == left, last, p).ld_click(
                true,
                cx.listener(move |this, _, _, cx| this.set_find_left(left, cx)),
            )
        };
        Some(
            h_flex()
                .id("hex-find-bar")
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
                        .id("hex-find-input")
                        .test_support()
                        .w(px(220.))
                        .tooltip(|window, cx| ld_tip(tr(FIND_TOOLTIP).into(), window, cx))
                        .child(ld_input(&bar.input).aria_label(tr("Find"))),
                )
                .child(ld_segmented(
                    "hex-find-mode",
                    [
                        mode("hex-find-hex", tr("Hex bytes"), true, false),
                        mode("hex-find-text", tr("Text"), false, true),
                    ],
                    p,
                ))
                .child(ld_segmented(
                    "hex-find-side",
                    [
                        side("hex-find-left", tr("Left"), true, false),
                        side("hex-find-right", tr("Right"), false, true),
                    ],
                    p,
                ))
                .child(
                    ld_icon_button("hex-find-prev", icons::PREV, tr("Previous (Shift+F3)"), p)
                        .ld_click(
                            true,
                            cx.listener(|this, _, _, cx| this.step_find(false, cx)),
                        ),
                )
                .child(
                    ld_icon_button("hex-find-next", icons::NEXT, tr("Next (F3)"), p)
                        .ld_click(true, cx.listener(|this, _, _, cx| this.step_find(true, cx))),
                )
                .child(div().flex_1())
                .child(
                    ld_icon_button("hex-find-close", icons::CLOSE, tr("Close (Esc)"), p).ld_click(
                        true,
                        cx.listener(|this, _, window, cx| this.close_find(window, cx)),
                    ),
                )
                .into_any_element(),
        )
    }
}
