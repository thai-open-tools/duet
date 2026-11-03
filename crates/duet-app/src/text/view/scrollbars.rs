//! The overview strip and the horizontal scrollbar of the text view.

use gpui_kit::ScrollWheelEvent;

use super::*;

impl TextView {
    pub(super) fn overview_fraction(&self, y: Pixels) -> Option<f32> {
        let bounds = self.overview_bounds.get();
        (bounds.size.height > px(0.)).then(|| (y - bounds.top()) / bounds.size.height)
    }

    pub(super) fn render_overview(
        &self,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let marks = self
            .model
            .as_ref()
            .map(|m| marks(m, 0.003))
            .unwrap_or_default();
        let ticks = self
            .find
            .as_ref()
            .and_then(|b| b.result.as_ref())
            .zip(self.model.as_ref())
            .map(|(r, m)| find_ticks(r, m.visible()))
            .unwrap_or_default();
        let viewport = self.overview_thumb();
        let p = *palette;
        let bounds_cell = self.overview_bounds.clone();
        let dragging = self.dragging_overview.then(|| cx.entity().downgrade());
        div()
            .id("text-overview")
            .test_support()
            .w(px(OVERVIEW_WIDTH))
            .h_full()
            .bg(palette.panel2)
            .border_l_1()
            .border_color(palette.line2)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if let Some(fraction) = this.overview_fraction(event.position.y) {
                        this.dragging_overview = true;
                        this.scroll_to_fraction(fraction, cx);
                    }
                }),
            )
            // The wheel scrolls the rows, a line a notch (R17); negative is down.
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                let row_height = px(this.row_height());
                this.rows.scroll_by(-event.delta.pixel_delta(row_height).y);
                cx.stop_propagation();
                cx.notify();
            }))
            // A release seen here ends the drag even before a frame has drawn the window-wide
            // listeners, so a quick click never leaves one running.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| end_overview(this, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| end_overview(this, cx)),
            )
            .child(
                canvas(
                    move |bounds, _, _| bounds_cell.set(bounds),
                    move |bounds, (), window, _| {
                        let height = bounds.size.height;
                        for mark in &marks {
                            let color = match mark.kind {
                                RowKind::LeftOnly => p.del,
                                RowKind::RightOnly => p.ins,
                                _ => p.modified,
                            };
                            let top = bounds.top() + height * mark.top;
                            let tall = (height * (mark.bottom - mark.top)).max(px(2.));
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(bounds.left() + px(3.), top),
                                    size(bounds.size.width - px(6.), tall),
                                ),
                                color,
                            ));
                        }
                        for tick in &ticks {
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(bounds.left(), bounds.top() + height * *tick),
                                    size(px(5.), px(2.)),
                                ),
                                p.accent,
                            ));
                        }
                        if let Some((top, bottom)) = viewport {
                            window.paint_quad(quad(
                                Bounds::new(
                                    point(bounds.left() + px(1.), bounds.top() + height * top),
                                    size(
                                        bounds.size.width - px(2.),
                                        (height * (bottom - top)).max(px(4.)),
                                    ),
                                ),
                                px(2.),
                                p.viewport,
                                px(1.),
                                p.accent,
                                BorderStyle::Solid,
                            ));
                        }
                        if let Some(view) = dragging {
                            follow_drag(view, window, drag_overview, end_overview);
                        }
                    },
                )
                .size_full(),
            )
    }

    /// The overview's thumb as fractions of the strip; None while there are no rows.
    fn overview_thumb(&self) -> Option<(f32, f32)> {
        let rows = self.model.as_ref().map_or(0, TextModel::row_count);
        self.rows
            .viewport(rows, px(self.row_height()), &|row| self.wrapped_height(row))
    }

    /// Test hook: the overview's thumb as the next frame draws it.
    #[doc(hidden)]
    pub fn overview_thumb_for_test(&self) -> Option<(f32, f32)> {
        self.overview_thumb()
    }

    /// Scrolls so the spot under `fraction` of the overview bar is centred (its row, with wrap
    /// off): the user's scroll, so not through `scroll_to_display`.
    pub fn scroll_to_fraction(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let rows = self.model.as_ref().map_or(0, TextModel::row_count);
        if rows > 0 {
            self.rows
                .scroll_to_fraction(fraction, rows, &|row| self.wrapped_height(row));
            cx.notify();
        }
    }

    /// The columns the horizontal scrollbar maps and how many a half shows; None while no
    /// drawn line is wider than a half, and while lines wrap (R3). The caret can scroll a few
    /// columns past the widest line's end, and the range then reaches that far, so the thumb
    /// never leaves the bar.
    pub(super) fn hscroll_range(&self) -> Option<(usize, usize)> {
        let (widest, columns) = (self.widest.get(), self.display.view_columns);
        (!self.wrap && columns > 0 && widest > columns)
            .then(|| (widest.max(self.display.skip_columns + columns), columns))
    }

    /// Test hook: the columns the horizontal scrollbar maps, as the drawn lines sized it.
    #[doc(hidden)]
    pub fn widest_for_test(&self) -> usize {
        self.widest.get()
    }

    /// Test hook: the horizontal thumb's left edge and width, and the bar's width, as the last
    /// frame drew them.
    #[doc(hidden)]
    pub fn hscroll_thumb_for_test(&self) -> Option<(f32, f32, f32)> {
        let bar = f32::from(self.hscroll_bounds.get().size.width);
        let (range, shown) = self.hscroll_range()?;
        let (x, w) = hscroll_thumb(bar, range, shown, self.display.skip_columns);
        Some((x, w, bar))
    }

    /// The horizontal scrollbar, while the widest drawn line is wider than a half.
    pub(super) fn render_hscroll(
        &self,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (range, shown) = self.hscroll_range()?;
        let skip = self.display.skip_columns;
        let cell = self.hscroll_bounds.clone();
        let (track, thumb) = (palette.panel, palette.line);
        let dragging = self.dragging_hscroll.then(|| cx.entity().downgrade());
        Some(
            div()
                .id("text-hscroll")
                .test_support()
                .h(px(SCROLLBAR_WIDTH))
                .w_full()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        this.press_hscroll(event.position.x, cx);
                    }),
                )
                // As on the overview: a quick click's release ends the drag here.
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| end_hscroll(this, cx)),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| end_hscroll(this, cx)),
                )
                .child(
                    canvas(
                        move |bounds, _, _| cell.set(bounds),
                        move |bounds, (), window, _| {
                            window.paint_quad(fill(bounds, track));
                            let bar = f32::from(bounds.size.width);
                            let (x, w) = hscroll_thumb(bar, range, shown, skip);
                            window.paint_quad(quad(
                                Bounds::new(
                                    point(bounds.left() + px(x), bounds.top() + px(3.)),
                                    size(px(w), px(5.)),
                                ),
                                px(3.),
                                thumb,
                                px(0.),
                                transparent_black(),
                                BorderStyle::Solid,
                            ));
                            if let Some(view) = dragging {
                                follow_drag(view, window, drag_hscroll, end_hscroll);
                            }
                        },
                    )
                    .size_full(),
                )
                .into_any_element(),
        )
    }

    /// The left button went down at `x` on the horizontal bar. On the thumb, the thumb is held
    /// by that point and nothing moves yet; on the track, the thumb's middle jumps there.
    pub(super) fn press_hscroll(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let bounds = self.hscroll_bounds.get();
        let Some((range, shown)) = self.hscroll_range() else {
            return;
        };
        self.dragging_hscroll = true;
        let bar = f32::from(bounds.size.width);
        let at = f32::from(x - bounds.left());
        let (left, width) = hscroll_thumb(bar, range, shown, self.display.skip_columns);
        if (left..=left + width).contains(&at) {
            self.hscroll_grab = at - left;
        } else {
            self.hscroll_grab = width / 2.;
            self.hscroll_to(x, cx);
        }
    }

    /// Scrolls so the held thumb's grabbed point is under `x` on the bar.
    pub(super) fn hscroll_to(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let bounds = self.hscroll_bounds.get();
        if bounds.size.width <= px(0.) {
            return;
        }
        // The widest line alone: the caret's few columns past it are no place to drag to.
        self.display.skip_columns = hscroll_skip(
            f32::from(bounds.size.width),
            f32::from(x - bounds.left()) - self.hscroll_grab,
            self.widest.get(),
            self.display.view_columns,
        );
        cx.notify();
    }
}

fn drag_hscroll(view: &mut TextView, at: Point<Pixels>, cx: &mut Context<TextView>) {
    view.hscroll_to(at.x, cx);
}

fn end_hscroll(view: &mut TextView, cx: &mut Context<TextView>) {
    view.dragging_hscroll = false;
    cx.notify();
}

fn drag_overview(view: &mut TextView, at: Point<Pixels>, cx: &mut Context<TextView>) {
    if let Some(fraction) = view.overview_fraction(at.y) {
        view.scroll_to_fraction(fraction.clamp(0.0, 1.0), cx);
    }
}

fn end_overview(view: &mut TextView, cx: &mut Context<TextView>) {
    view.dragging_overview = false;
    cx.notify();
}
