//! The body of a compare with no lines on either side (R15): two empty halves that take a click,
//! so pasted or typed text has somewhere to go.

use gpui_kit::{MouseButton, MouseDownEvent};

use super::stage::{CENTRE_GUTTER, OVERVIEW_WIDTH};
use super::*;
use crate::ui::style::SCROLLBAR_WIDTH;

const EMPTY_HINT: &str = "Type or paste here (Ctrl+V), or open a file (Ctrl+O).";
const CARET_WIDTH: f32 = 1.5;

impl TextView {
    /// Neither side has a line: nothing to list, though both sides can be typed into.
    pub(crate) fn is_empty_pair(&self) -> bool {
        self.model
            .as_ref()
            .is_some_and(|m| m.line_count(true) == 0 && m.line_count(false) == 0)
    }

    pub(super) fn render_empty(
        &self,
        palette: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let half = |left: bool| {
            let caret_here = self.caret.is_some_and(|c| c.left == left);
            div()
                .id(if left {
                    "text-empty-left"
                } else {
                    "text-empty-right"
                })
                .test_support()
                .relative()
                .flex_1()
                .h_full()
                .bg(palette.panel)
                .cursor_text()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                        this.click_empty_side(left, window, cx)
                    }),
                )
                .child(
                    div()
                        .px(px(12.))
                        .py(px(8.))
                        .text_color(palette.muted)
                        .child(EMPTY_HINT),
                )
                .when(caret_here, |d| {
                    d.child(
                        div()
                            .absolute()
                            .left(px(12.))
                            .top(px(8.))
                            .w(px(CARET_WIDTH))
                            .h(px(self.row_height() - 4.))
                            .bg(palette.accent),
                    )
                })
        };
        h_flex()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .relative()
                    .flex_1()
                    .h_full()
                    .font(fonts.code.clone())
                    .text_size(px(self.font_size))
                    .line_height(px(self.row_height()))
                    .child({
                        // The names bar above measures its columns from these bounds.
                        let cell = self.list_bounds.clone();
                        canvas(
                            move |bounds, window, _| {
                                if cell.replace(bounds).size.width != bounds.size.width {
                                    window.request_animation_frame();
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full()
                    })
                    .child(
                        h_flex()
                            .size_full()
                            .child(half(true))
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(CENTRE_GUTTER))
                                    .h_full()
                                    .bg(palette.bg),
                            )
                            .child(half(false))
                            .child(div().flex_none().w(px(SCROLLBAR_WIDTH)).h_full()),
                    )
                    // Typing and the IME reach the caret through this handler.
                    .when(self.caret.is_some(), |d| {
                        let entity = cx.entity();
                        let focus = self.focus.clone();
                        d.child(
                            canvas(
                                |_, _, _| {},
                                move |bounds, (), window, cx| {
                                    window.handle_input(
                                        &focus,
                                        ElementInputHandler::new(bounds, entity),
                                        cx,
                                    );
                                },
                            )
                            .absolute()
                            .size_full(),
                        )
                    }),
            )
            .child(div().flex_none().w(px(OVERVIEW_WIDTH)).h_full())
            .into_any_element()
    }
}
