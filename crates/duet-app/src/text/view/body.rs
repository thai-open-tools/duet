//! The text view's row list: building the rows of one frame and measuring line widths.

use gpui_kit::Background;

use super::*;
use crate::i18n::tr;
use crate::ui::icons;
use rows::{LineWidths, RowMetrics};

/// The text each half drew, by (display row, left): what clicks and the IME hit-test.
type Layouts = Rc<RefCell<HashMap<(usize, bool), HalfText>>>;

/// What every row of one `render_rows` call shares, set up once for the call rather than once
/// per row (PF15): the uniform list makes one call a frame, the wrapped list one per row.
struct RowCtx<'a> {
    frame: RowFrame<'a>,
    palette: Palette,
    hatch: Background,
    display: DisplayOptions,
    metrics: RowMetrics,
    marked: Option<u32>,
    layouts: &'a Layouts,
    widest: &'a Cell<usize>,
    widths: &'a RefCell<LineWidths>,
    /// Each half is as wide as the names bar's cell above it, once the list is measured.
    half_width: Option<f32>,
    /// The sides a copy arrow may point at.
    to_left: bool,
    to_right: bool,
}

impl TextView {
    /// The uniform list's rows `range`.
    pub(super) fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let palette = Palette::current(cx);
        let widest_before = self.widest.get();
        // Wrapped rows keep the width a resize drag froze (X-18).
        let list_width = f32::from(if self.wrap {
            self.rows_width()
        } else {
            self.list_bounds.get().size.width
        });
        let metrics = self.metrics();
        let frame = RowFrame::new(
            &self.request,
            (self.caret, self.row_sel),
            self.find.as_ref(),
            &self.syntax,
            &self.syntax_theme,
            metrics,
        );
        let Some(model) = &mut self.model else {
            return Vec::new();
        };
        let ctx = RowCtx {
            frame,
            hatch: crate::ui::style::hatch(&palette, window),
            palette,
            display: self.display,
            metrics,
            marked: self.marked,
            layouts: &self.layouts,
            widest: &self.widest,
            widths: &self.line_widths,
            half_width: (list_width > 0.).then(|| {
                let c = stage_columns(list_width + OVERVIEW_WIDTH);
                c.left.end - c.left.start
            }),
            // A copy arrow only points at a side that can take the copy, from a side that shows
            // its own text; the formatted and the sorted view and files over 64 MiB cannot.
            // Alt+arrow still says why.
            to_left: model.can_copy_to(true),
            to_right: model.can_copy_to(false),
        };
        self.rows_built += range.len();
        let rows = range.map(|i| row(&ctx, model, i, cx)).collect();
        // Rows are laid out after `render` drew the horizontal scrollbar; a wider line
        // needs one more frame to show it at its new size.
        if self.widest.get() > widest_before {
            window.request_animation_frame();
        }
        self.count_line_widths(cx);
        rows
    }

    /// The wrapped list's row `display`: the list asks for its rows one at a time.
    pub(super) fn render_row(
        &mut self,
        display: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_rows(display..display + 1, window, cx)
            .pop()
            .unwrap_or_else(|| div().into_any_element())
    }

    /// The text the long-line counts were taken from is gone: a new compare, or F9.
    pub(super) fn clear_line_widths(&mut self) {
        self.line_widths.borrow_mut().clear();
        self.width_job.cancel();
    }

    /// Counts the long lines the rows asked for, off the UI thread. It waits while an edit's
    /// re-diff is pending: typing changes the line under the count, so the count runs once the
    /// typing pauses. A count that lands after a newer edit to its side is dropped, and the
    /// line is asked for again.
    pub(crate) fn count_line_widths(&mut self, cx: &mut Context<Self>) {
        if self.width_job.is_running() || self.rediff_job.is_running() {
            return;
        }
        let Some(model) = &self.model else {
            return;
        };
        let wanted = self.line_widths.borrow_mut().take_wanted();
        let versions = model.versions();
        let tab_width = self.display.tab_width;
        let lines: Vec<_> = wanted
            .into_iter()
            .filter(|&(left, line)| (line as usize) < model.line_count(left))
            .map(|(left, line)| {
                let text = model.line_text(left, line as usize).into_owned();
                ((left, line), versions[usize::from(!left)], text)
            })
            .collect();
        if lines.is_empty() {
            return;
        }
        let (generation, _cancel) = self.width_job.start();
        let background = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let counted = background
                .spawn(async move {
                    lines
                        .into_iter()
                        .map(|(key, version, text)| (key, version, columns(&text, tab_width)))
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if !view.width_job.is_current(generation) {
                    return;
                }
                view.width_job.finish(generation);
                let now = view.model.as_ref().map(TextModel::versions);
                let mut widths = view.line_widths.borrow_mut();
                for (key, version, n) in counted {
                    if now.is_some_and(|v| v[usize::from(!key.0)] == version)
                        && view.display.tab_width == tab_width
                    {
                        widths.counted(key, tab_width, n);
                        view.widest.set(view.widest.get().max(n));
                    }
                }
                drop(widths);
                // Lines whose count was dropped are asked for again by the next frame.
                cx.notify();
            });
        })
        .detach();
    }
}

/// Display row `i`: a fold, or both halves with the copy arrows between them.
fn row(
    ctx: &RowCtx<'_>,
    model: &mut TextModel,
    i: usize,
    cx: &mut Context<TextView>,
) -> AnyElement {
    let palette = &ctx.palette;
    // One lookup per row: the rest works from the RowMap index.
    let index = match model.display_row(i) {
        DisplayRow::Fold { start, end } => {
            return rows::fold_row(i, end - start, ctx.metrics, palette, cx);
        }
        DisplayRow::Row(index) => index,
    };
    let model_row = Some(index);
    let row = model.row_at(index);
    let inline = ctx.frame.inline(model, index, row.kind);
    let model_block_start = model.block_starting_at(index);
    let [left, right] = ctx.frame.halves(model, i, index, row, inline.as_deref());
    let current = left.current;
    let number_down = |left: bool| {
        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
            this.press_number(i, left, e, window, cx)
        })
    };
    let tag_down = |left: bool| {
        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
            this.press_moved_tag(i, left, e, window, cx)
        })
    };
    let draw = |h, downs| {
        half(
            h,
            ctx.display,
            palette,
            ctx.hatch,
            ctx.widest,
            ctx.widths,
            downs,
        )
    };
    let (left, left_text) = draw(left, (number_down(true), tag_down(true)));
    let (right, right_text) = draw(right, (number_down(false), tag_down(false)));
    // Both halves of a wrapped row are as tall as the one with more segments (R3).
    let lines = |text: &Option<HalfText>| text.as_ref().map_or(1, |t| t.lines().len());
    let lines = lines(&left_text).max(lines(&right_text));
    // A row's text is for hit-testing once the row is prepainted: rows the list only measured
    // (the uniform list's sizing row, the wrapped list's overdraw) have no position, and the
    // IME's queries would panic on them (PF1).
    let texts = RefCell::new(Some([left_text, right_text]));
    let layouts = ctx.layouts.clone();
    let left = clickable(left, i, true, cx);
    let right = clickable(right, i, false, cx);
    let (left, right) = match ctx.half_width {
        Some(w) => (left.flex_none().w(px(w)), right.flex_none().w(px(w))),
        None => (left, right),
    };
    let (to_left, to_right) = (ctx.to_left, ctx.to_right);
    h_flex()
        .on_children_prepainted(move |_, _, _| {
            let Some([left, right]) = texts.take() else {
                return;
            };
            let mut layouts = layouts.borrow_mut();
            for (text, side) in [(left, true), (right, false)] {
                if let Some(text) = text {
                    layouts.insert((i, side), text);
                }
            }
        })
        .id(("text-row", i))
        .test_support()
        .relative()
        .h(px(ctx.metrics.row_height * lines as f32))
        .w_full()
        .pr(px(SCROLLBAR_WIDTH))
        .when(current, |d| d.bg(palette.cur_row))
        .child(left)
        .child({
            let current_block = model.current_block();
            div()
                .id(("text-gutter", i))
                .test_support()
                .flex_none()
                .w(px(CENTRE_GUTTER))
                .h_full()
                .flex()
                .justify_center()
                .gap(px(3.))
                .items_center()
                .border_l_1()
                .border_r_1()
                .border_color(palette.line2)
                .bg(palette.bg)
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, window, cx| this.press_right(i, None, window, cx)),
                )
                .when_some(model_block_start, |d, block| {
                    let current = current_block == Some(block);
                    d.when(to_right, |d| {
                        d.child(
                            ld_gutter_button(
                                ("copy-right", block),
                                icons::TO_RIGHT,
                                tr("Copy block to the right (Alt+→)"),
                                current,
                                palette,
                            )
                            .ld_click(
                                true,
                                cx.listener(move |this, _, _, cx| {
                                    this.copy_block(block, false, cx);
                                }),
                            ),
                        )
                    })
                    .when(to_left, |d| {
                        d.child(
                            ld_gutter_button(
                                ("copy-left", block),
                                icons::TO_LEFT,
                                tr("Copy block to the left (Alt+←)"),
                                current,
                                palette,
                            )
                            .ld_click(
                                true,
                                cx.listener(move |this, _, _, cx| {
                                    this.copy_block(block, true, cx);
                                }),
                            ),
                        )
                    })
                })
        })
        .child(right)
        // The go-to row's accent edge, drawn over the row so it takes no width.
        .when(ctx.marked.is_some() && model_row == ctx.marked, |d| {
            d.child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .h_full()
                    .w(px(2.))
                    .bg(palette.accent),
            )
        })
        .into_any_element()
}
