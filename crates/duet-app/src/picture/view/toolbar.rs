//! The picture view's toolbar (modes, zoom, tolerance, groups, files), its two heads and the
//! tolerance controls (R9; `html:2-27`, `js:664-675`).

use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState, NumberInput, NumberInputEvent};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{Sizable as _, h_flex};
use gpui_kit::{AnyElement, ClickEvent, Entity, Subscription, Task, Window, div};

use super::*;
use crate::pathbar::path_input;
use crate::picture::prefs::Mode;
use crate::picture::zoom::zoom_label;
use crate::ui::fonts::{CODE_SIZE, Fonts};
use crate::ui::icons;
use crate::ui::style::{
    LdExt as _, ld_button, ld_icon_button, ld_segment, ld_segmented, ld_separator, ld_tip,
};

/// A tolerance change waits this long after the last one before it is computed (R9).
const TOLERANCE_DELAY: Duration = Duration::from_millis(90);
/// Above this overlap, in px, a lazy tolerance change waits (R9).
const LAZY_OVERLAP: u64 = 2_000_000;
/// The slider gives each of the 256 tolerances a pixel of its own, so a drag reaches any one
/// exactly (I-40).
const SLIDER_WIDTH: f32 = 256.;
/// The number box: its step buttons and room for "255" between them (I-40).
const BOX_WIDTH: f32 = 96.;

const MODE_TIPS: [(Mode, &str, &str); 4] = [
    (
        Mode::SideBySide,
        "picture-mode-side",
        "Both pictures side by side: they pan and zoom together (1)",
    ),
    (
        Mode::Difference,
        "picture-mode-diff",
        "Difference image: differing pixels in red on a dimmed copy of the left picture (2)",
    ),
    (
        Mode::Slider,
        "picture-mode-slider",
        "One over the other: drag the divider to compare left and right (3)",
    ),
    (
        Mode::Blink,
        "picture-mode-blink",
        "Show the left and right picture in turn every 0.6 seconds (4)",
    ),
];

/// The tolerance slider and box, and what each shows and has applied.
pub(super) struct ToleranceControls {
    pub(super) input: Entity<InputState>,
    pub(super) slider: Entity<SliderState>,
    /// The wait before a lazy change is computed, replaced on every change.
    pub(super) task: Option<Task<()>>,
    /// A lazy change waits for its delay.
    pub(super) pending: bool,
    slider_shown: u8,
    /// The value the box's text stands for; None when the text must be rewritten.
    box_shown: Option<u8>,
    /// The tolerance the newest compare was started with (at first, the one loaded); None once
    /// Esc cancelled a compare, so the next apply computes whatever the value.
    pub(super) started: Option<u8>,
    /// The tolerance in `settings.json`.
    saved: u8,
    _subscriptions: Vec<Subscription>,
}

/// `clamp(round(number), 0, 255)`; text that is not a number is 0 (`js:664`).
fn parse_tolerance(text: &str) -> u8 {
    let number: f64 = text.trim().parse().unwrap_or(0.);
    if number.is_finite() {
        number.round().clamp(0., 255.) as u8
    } else {
        0
    }
}

impl ToleranceControls {
    pub(super) fn new(value: u8, window: &mut Window, cx: &mut Context<PictureView>) -> Self {
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(value.to_string(), window, cx);
            state
        });
        let slider = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(255.)
                .step(1.)
                .default_value(f32::from(value))
        });
        let _subscriptions = vec![
            cx.subscribe_in(
                &input,
                window,
                |this: &mut PictureView, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => this.tolerance_typed(window, cx),
                    InputEvent::PressEnter { .. } | InputEvent::Blur => {
                        this.tolerance_entered(window, cx)
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &input,
                window,
                |this: &mut PictureView, _, event: &NumberInputEvent, window, cx| {
                    let NumberInputEvent::Step(action) = event;
                    let up = matches!(action, gpui_kit::component::input::StepAction::Increment);
                    this.tolerance_stepped(up, window, cx)
                },
            ),
            cx.subscribe_in(
                &slider,
                window,
                |this: &mut PictureView, _, event: &SliderEvent, window, cx| {
                    let (SliderEvent::Change(at) | SliderEvent::Release(at)) = event;
                    let value = at.start().round().clamp(0., 255.) as u8;
                    this.tolerance_slid(value, matches!(event, SliderEvent::Change(_)), window, cx)
                },
            ),
        ];
        Self {
            input,
            slider,
            task: None,
            pending: false,
            slider_shown: value,
            box_shown: Some(value),
            started: Some(value),
            saved: value,
            _subscriptions,
        }
    }
}

impl PictureView {
    pub fn tolerance(&self) -> u8 {
        self.prefs.tolerance
    }

    /// Makes `value` the tolerance (R9): the slider and the box show it, and the pictures are
    /// compared again. `lazy` (a slider move or typing) waits 90 ms after the last change when
    /// the overlap is large; anything else is applied and saved at once.
    pub fn set_tolerance(&mut self, value: u8, lazy: bool, cx: &mut Context<Self>) {
        self.prefs.tolerance = value;
        if lazy && self.overlap_pixels() > LAZY_OVERLAP {
            self.tolerance.pending = true;
            self.tolerance.task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(TOLERANCE_DELAY).await;
                let _ = this.update(cx, |view, cx| view.apply_tolerance(true, cx));
            }));
            cx.notify();
        } else {
            self.apply_tolerance(!lazy, cx);
        }
    }

    /// The pictures are compared with the tolerance now; `save` remembers it.
    fn apply_tolerance(&mut self, save: bool, cx: &mut Context<Self>) {
        self.tolerance.task = None;
        self.tolerance.pending = false;
        let value = self.prefs.tolerance;
        if save {
            self.save_tolerance(cx);
        }
        if self.tolerance.started != Some(value) && !self.loads_running() {
            self.compare_now(cx);
        } else {
            cx.notify();
        }
    }

    /// Writes the tolerance to `settings.json` unless it is there already.
    fn save_tolerance(&mut self, cx: &mut Context<Self>) {
        let value = self.prefs.tolerance;
        if self.tolerance.saved != value {
            self.tolerance.saved = value;
            if let Err(err) = self.prefs.save(settings::settings_file(cx).as_deref()) {
                tracing::warn!(%err, "could not remember the picture tolerance");
            }
        }
    }

    /// Esc stopped the work: a waiting change is dropped, the controls go back to the tolerance
    /// of the result on screen (remembered as such), and the next apply computes.
    pub(super) fn restore_tolerance(&mut self, cx: &mut Context<Self>) {
        self.tolerance.task = None;
        self.tolerance.pending = false;
        self.tolerance.started = None;
        if let Some(shown) = self.compared.as_ref().map(|c| c.tolerance) {
            self.prefs.tolerance = shown;
            self.save_tolerance(cx);
        }
    }

    /// The area both pictures share, in px.
    fn overlap_pixels(&self) -> u64 {
        match (&self.sides[LEFT], &self.sides[RIGHT]) {
            (Some(left), Some(right)) => {
                u64::from(left.info.width.min(right.info.width))
                    * u64::from(left.info.height.min(right.info.height))
            }
            _ => 0,
        }
    }

    /// The tolerance the shown difference was made with.
    #[doc(hidden)]
    pub fn diff_tolerance_for_test(&self) -> Option<u8> {
        self.compared.as_ref().map(|c| c.tolerance)
    }

    #[doc(hidden)]
    pub fn tolerance_input_for_test(&self) -> Entity<InputState> {
        self.tolerance.input.clone()
    }

    #[doc(hidden)]
    pub fn slider_for_test(&self) -> Entity<SliderState> {
        self.tolerance.slider.clone()
    }

    /// Makes the slider and the box show the tolerance, unless they already do.
    pub(super) fn sync_tolerance_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.prefs.tolerance;
        if self.tolerance.slider_shown != value {
            self.tolerance.slider_shown = value;
            self.tolerance.slider.update(cx, |state, cx| {
                state.set_value(f32::from(value), window, cx)
            });
        }
        if self.tolerance.box_shown != Some(value) {
            self.tolerance.box_shown = Some(value);
            self.tolerance.input.update(cx, |state, cx| {
                state.set_value(value.to_string(), window, cx)
            });
        }
    }

    fn box_text(&self, cx: &App) -> String {
        self.tolerance.input.read(cx).value().to_string()
    }

    /// The box's text changed: a number applies after the delay; the text stays as typed.
    fn tolerance_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.box_text(cx);
        if text.trim().is_empty() {
            return;
        }
        let value = parse_tolerance(&text);
        self.tolerance.box_shown = Some(value);
        if value != self.prefs.tolerance {
            self.set_tolerance(value, true, cx);
        }
        self.sync_tolerance_controls(window, cx);
    }

    /// Enter or leaving the box: applies it at once, and the text becomes the clamped number.
    fn tolerance_entered(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = parse_tolerance(&self.box_text(cx));
        self.tolerance.box_shown = None;
        self.set_tolerance(value, false, cx);
        self.sync_tolerance_controls(window, cx);
    }

    /// The box's step buttons add or take one at once.
    fn tolerance_stepped(&mut self, up: bool, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.box_text(cx);
        let now = if text.trim().is_empty() {
            self.prefs.tolerance
        } else {
            parse_tolerance(&text)
        };
        let value = if up {
            now.saturating_add(1)
        } else {
            now.saturating_sub(1)
        };
        self.tolerance.box_shown = None;
        self.set_tolerance(value, false, cx);
        self.sync_tolerance_controls(window, cx);
    }

    /// The slider moved (`lazy`) or was let go.
    fn tolerance_slid(
        &mut self,
        value: u8,
        lazy: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if lazy {
            // Mid-drag the slider already shows it; a release makes sure it does.
            self.tolerance.slider_shown = value;
        }
        self.set_tolerance(value, lazy, cx);
        self.sync_tolerance_controls(window, cx);
    }

    pub(super) fn render_toolbar(
        &self,
        p: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mode = self.mode();
        let segments = MODE_TIPS.map(|(this_mode, id, tip)| {
            ld_segment(
                id,
                this_mode.label(),
                mode == this_mode,
                this_mode == Mode::Blink,
                p,
            )
            .ld_tooltip(tip)
            .ld_click(
                true,
                cx.listener(move |this, _, _, cx| this.set_mode(this_mode, cx)),
            )
        });
        let fit = self.is_fit();
        let actual = !fit && (self.zoom() - 1.).abs() < 1e-3;
        let label = zoom_label(self.zoom());
        let can_navigate = self.can_navigate();
        h_flex()
            .id("picture-toolbar")
            .test_support()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap(px(5.))
            .px(px(10.))
            .py(px(5.))
            .bg(p.panel)
            .border_b_1()
            .border_color(p.line)
            .child(ld_segmented("picture-mode", segments, p))
            .child(ld_separator(p))
            .child(
                ld_button("picture-zoom-fit", "Fit", p)
                    .ld_tooltip("Fit to the window (Ctrl+0)")
                    .ld_on(fit, p)
                    .ld_click(true, cx.listener(|this, _, _, cx| this.fit(cx))),
            )
            .child(
                ld_button("picture-zoom-1", "1:1", p)
                    .ld_tooltip("Actual size, 100% (Ctrl+1)")
                    .ld_on(actual, p)
                    .ld_click(
                        true,
                        cx.listener(|this, _, _, cx| this.set_zoom(1., None, cx)),
                    ),
            )
            .child(
                ld_icon_button(
                    "picture-zoom-out",
                    icons::ZOOM_OUT,
                    "Zoom out (Ctrl+− or Ctrl+wheel)",
                    p,
                )
                .ld_click(
                    true,
                    cx.listener(|this, _, _, cx| this.step_zoom(false, cx)),
                ),
            )
            .child(
                div()
                    .id("picture-zoom-pct")
                    .test_support()
                    .flex_none()
                    .min_w(px(44.))
                    .text_center()
                    .font(fonts.code.clone())
                    .text_size(px(CODE_SIZE))
                    .text_color(p.muted)
                    .aria_label(label.clone())
                    .child(label),
            )
            .child(
                ld_icon_button(
                    "picture-zoom-in",
                    icons::ZOOM_IN,
                    "Zoom in (Ctrl++ or Ctrl+wheel)",
                    p,
                )
                .ld_click(true, cx.listener(|this, _, _, cx| this.step_zoom(true, cx))),
            )
            .child(ld_separator(p))
            .child(self.render_tolerance(p, fonts))
            .child(ld_separator(p))
            .child(
                ld_icon_button(
                    "picture-prev",
                    icons::PREV,
                    "Previous difference group (F7)",
                    p,
                )
                .ld_click(
                    can_navigate,
                    cx.listener(|this, _, _, cx| this.go(false, cx)),
                ),
            )
            .child(
                ld_icon_button("picture-next", icons::NEXT, "Next difference group (F8)", p)
                    .ld_click(
                        can_navigate,
                        cx.listener(|this, _, _, cx| this.go(true, cx)),
                    ),
            )
            .child(ld_separator(p))
            .child(
                ld_icon_button("picture-swap", icons::SWAP, "Swap left and right (F9)", p)
                    .ld_click(
                        self.can_swap(),
                        cx.listener(|this, _, _, cx| this.swap_sides(cx)),
                    ),
            )
            .child(
                ld_icon_button(
                    "picture-reload",
                    icons::RELOAD,
                    "Read both pictures from disk again (F5)",
                    p,
                )
                .ld_click(
                    self.can_reload(),
                    cx.listener(|this, _, _, cx| this.reload(cx)),
                ),
            )
            .child(
                ld_button("picture-export", "Export difference image", p)
                    .ld_tooltip("Save the difference image as a PNG file")
                    .ld_click(
                        self.can_export(),
                        cx.listener(|this, _, window, cx| this.export(window, cx)),
                    ),
            )
            .into_any_element()
    }

    /// "Tolerance", the slider and the number box.
    fn render_tolerance(&self, p: &Palette, fonts: &Fonts) -> impl IntoElement {
        const TIP: &str = "Pixels whose every channel (R, G, B, A) differs by this much or less count as the same";
        h_flex()
            .id("picture-tolerance-group")
            .test_support()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .tooltip(|window, cx| ld_tip(TIP.into(), window, cx))
            .child(div().flex_none().text_color(p.muted).child("Tolerance"))
            .child(
                div()
                    .id("picture-tolerance")
                    .test_support()
                    .flex_none()
                    .w(px(SLIDER_WIDTH))
                    .child(Slider::new(&self.tolerance.slider).w_full()),
            )
            .child(
                div()
                    .id("picture-tolerance-input")
                    .test_support()
                    .flex_none()
                    .w(px(BOX_WIDTH))
                    .font(fonts.code.clone())
                    .text_size(px(CODE_SIZE))
                    .child(NumberInput::new(&self.tolerance.input).small()),
            )
    }

    /// The two heads: each side's open button, name and size line.
    pub(super) fn render_heads(
        &self,
        p: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .id("picture-heads")
            .test_support()
            .flex_none()
            .bg(p.panel)
            .border_b_1()
            .border_color(p.line)
            .child(self.render_head(true, p, fonts, cx))
            .child(self.render_head(false, p, fonts, cx))
            .into_any_element()
    }

    fn render_head(
        &self,
        left: bool,
        p: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (open_id, open_label, open_tip, name_id, meta_id) = if left {
            (
                "picture-open-left",
                "Open left…",
                "Choose the left picture (Ctrl+O)",
                "picture-left-name",
                "picture-left-meta",
            )
        } else {
            (
                "picture-open-right",
                "Open right…",
                "Choose the right picture (Ctrl+Shift+O)",
                "picture-right-name",
                "picture-right-meta",
            )
        };
        let head = self.head_text(left);
        let named = !head.name.is_empty();
        let meta_color = if head.failed { p.del } else { p.muted };
        let (dims, rest) = match head.meta.split_once(" · ") {
            Some((dims, rest)) if !head.failed => (dims.to_owned(), Some(rest.to_owned())),
            _ => (head.meta.clone(), None),
        };
        let dims_color = if head.dims_differ {
            p.modified
        } else {
            meta_color
        };
        let meta = div()
            .id(meta_id)
            .test_support()
            .flex_none()
            .flex()
            .gap(px(4.))
            .font(fonts.code.clone())
            .text_size(px(CODE_SIZE))
            .text_color(meta_color)
            .aria_label(head.meta.clone())
            .child(div().text_color(dims_color).child(dims))
            .children(rest.map(|rest| div().child(format!("· {rest}"))));
        let meta = match self.head_tooltip(left) {
            Some(tip) => meta
                .tooltip(move |window, cx| ld_tip(tip.clone().into(), window, cx))
                .into_any_element(),
            None => meta.into_any_element(),
        };
        h_flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(px(8.))
            .px(px(10.))
            .py(px(5.))
            .child(
                ld_button(open_id, open_label, p)
                    .ld_tooltip(open_tip)
                    .ld_click(
                        true,
                        cx.listener(move |this, _, window, cx| this.pick_file(left, window, cx)),
                    ),
            )
            .child(self.render_name(left, name_id, named, head.name.clone(), p, cx))
            .child(meta)
            .into_any_element()
    }

    /// The head's name, or the path box a click on it turns it into.
    fn render_name(
        &self,
        left: bool,
        name_id: &'static str,
        named: bool,
        name: String,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(edit) = self.path_edit.as_ref().filter(|e| e.left() == left) {
            let id = if left {
                "picture-left-path"
            } else {
                "picture-right-path"
            };
            return div()
                .flex_1()
                .min_w_0()
                .child(path_input(id, edit.edit(), p))
                .into_any_element();
        }
        let label = div()
            .id(name_id)
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.begin_path_edit(left, window, cx)
            }))
            .cursor_text()
            .test_support()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_color(if named { p.text } else { p.muted })
            .aria_label(name.clone())
            .child(if named {
                name
            } else {
                "No picture chosen".to_owned()
            });
        match self.name_tooltip(left) {
            Some(tip) => label
                .tooltip(move |window, cx| ld_tip(tip.clone().into(), window, cx))
                .into_any_element(),
            None => label.into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_tolerance;

    #[test]
    fn the_box_text_is_rounded_clamped_and_not_a_number_is_zero() {
        assert_eq!(parse_tolerance("5"), 5);
        assert_eq!(parse_tolerance(" 12 "), 12);
        assert_eq!(parse_tolerance("5.5"), 6);
        assert_eq!(parse_tolerance("300"), 255);
        assert_eq!(parse_tolerance("-4"), 0);
        assert_eq!(parse_tolerance("abc"), 0);
        assert_eq!(parse_tolerance(""), 0);
        assert_eq!(parse_tolerance("inf"), 0);
        assert_eq!(parse_tolerance("1e2"), 100);
    }
}
