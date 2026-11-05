//! LocalDiff's controls (spec section 7). Each one is gpui-kit's own unstyled `Button`, the
//! one under gpui-kit's styled buttons, so it keeps their behaviour: a keyed focus handle that
//! is a tab stop, Enter and Space, an accessibility role, and a disabled state that drops all
//! three. This module adds the look, with colours from the palette. Every control registers
//! for the UI tests and reports its state to accessibility: its visible label as its name (an
//! icon-only control is named by its tooltip; a labelled control's tooltip is its
//! description), `selected` for segments, chips and gutter buttons, and `toggled` for check
//! toggles.

use gpui_kit::TestSupportExt as _;
use gpui_kit::assets::IconName;
use gpui_kit::base::{Button, GlobalState};
use gpui_kit::component::button::{Button as KitButton, ButtonVariants as _};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, AnyView, App, Background, ClickEvent, Div, ElementId, Font, FontWeight,
    HighlightStyle, Hsla, InteractiveElement, Interactivity, IntoElement, MouseButton,
    ParentElement, RenderOnce, Role, SharedString, StatefulInteractiveElement, StyleRefinement,
    Styled, StyledText, Toggled, ViewElement, Window, div, pattern_slash, prelude::*, px, relative,
    transparent_black,
};

use crate::i18n::tr;
use crate::palette::Palette;
pub use crate::status::{StatusPart, Tone, join_parts};
use crate::ui::fonts::BADGE_SIZE;
use crate::ui::icons;
use crate::ui::prose::ProseExt as _;

pub const RADIUS: f32 = 7.;
pub const TAB_RADIUS: f32 = 8.;
pub const GUTTER_RADIUS: f32 = 5.;
/// The vertical scrollbar's track; `app::thin_scrollbars` sets gpui-base's theme to match.
pub const SCROLLBAR_WIDTH: f32 = 11.0;
/// The overview strip at a compare's right edge.
pub const OVERVIEW_WIDTH: f32 = 18.0;
pub const CARD_RADIUS: f32 = 10.;
pub const CONTROL_HEIGHT: f32 = 28.;
pub const SMALL_HEIGHT: f32 = 22.;
/// Stripe and gap of the hatch along the pattern's axis: at 45° that is a 6 px stripe and a
/// 12 px period across the stripes, like LocalDiff's CSS.
pub const HATCH_STRIPE: f32 = 8.5;
/// The id of the label inside a toggle or a dropdown, for the drawn tests (`within` the
/// control's own id).
pub const LABEL_ID: &str = "ld-label";
/// The icon of a full-height icon button.
pub const ICON_SIZE: f32 = 15.;
/// An icon beside a label, or in a small icon button.
pub const LABEL_ICON_SIZE: f32 = 13.;
/// An icon in a 23×18 gutter button, and a caret.
pub const SMALL_ICON_SIZE: f32 = 12.;
/// A toggle's check mark.
const CHECK_SIZE: f32 = 10.;

impl Tone {
    pub fn color(self, p: &Palette) -> Hsla {
        match self {
            Tone::Muted => p.muted,
            Tone::Text => p.text,
            Tone::Changed => p.modified,
            Tone::LeftOnly => p.del,
            Tone::RightOnly => p.ins,
            Tone::Moved => p.moved,
            Tone::Unimportant => p.unimp,
            Tone::Accent => p.accent,
        }
    }
}

/// One of LocalDiff's controls. Its hover look waits for render, so a disabled control can
/// drop it: gpui sets a hover style once and cannot take it back.
pub struct Control {
    button: Button,
    hover: Option<StyleRefinement>,
    enabled: bool,
    /// Whether the control has its accessible name, so a tooltip only describes it.
    named: bool,
}

impl Control {
    fn new(id: impl Into<ElementId>, role: Role) -> Self {
        Self {
            button: Button::new(id).role(role),
            hover: None,
            enabled: true,
            named: false,
        }
    }

    /// A frame around other controls takes no focus of its own.
    fn no_tab_stop(mut self) -> Self {
        self.button = self.button.focusable(false);
        self
    }

    /// The visible label, which is also the accessible name.
    fn labelled(self, label: SharedString) -> Self {
        self.aria_label(label.clone()).child(label)
    }

    /// The tooltip alone, leaving the accessible name and description as they are.
    fn tip(self, text: SharedString) -> Self {
        self.tooltip(move |window, cx| ld_tip(text.clone(), window, cx))
    }

    /// An icon-only control: the tooltip is its accessible name.
    fn named_by_tip(self, text: SharedString) -> Self {
        self.aria_label(text.clone()).tip(text)
    }

    fn hover_look(&self) -> Option<StyleRefinement> {
        self.hover.clone().filter(|_| self.enabled)
    }

    /// The primary look: solid accent with accent-ink text and icon; its ring is accent ink,
    /// since an accent ring would vanish into the fill.
    pub fn as_primary(self, p: &Palette) -> Self {
        let ink = p.accent_ink;
        self.bg(p.accent)
            .border_color(p.accent)
            .text_color(p.accent_ink)
            .font_weight(FontWeight::SEMIBOLD)
            .focus_visible(move |s| s.border_2().border_color(ink))
    }

    /// The ghost look: no fill and no frame, muted text and icon that turn accent under the
    /// pointer.
    pub fn as_ghost(self, p: &Palette) -> Self {
        let accent = p.accent;
        self.bg(transparent_black())
            .border_color(transparent_black())
            .text_color(p.muted)
            .hover(move |s| s.text_color(accent))
    }

    /// The gpui-kit button this control renders as.
    fn into_button(self) -> Button {
        let look = self.hover_look();
        let Control {
            button, enabled, ..
        } = self;
        button
            .disabled(!enabled)
            .when_some(look, |button, look| button.hover(move |_| look))
            .map(|button| {
                if enabled {
                    // As gpui-kit's styled Button: a press leaves the keyboard focus where it
                    // is and starts no text selection.
                    button.on_mouse_down(MouseButton::Left, |_, window, cx| {
                        window.prevent_default();
                        GlobalState::suppress_text_selection(cx);
                    })
                } else {
                    button.opacity(0.4).cursor_default()
                }
            })
    }
}

impl RenderOnce for Control {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.into_button()
    }
}

impl IntoElement for Control {
    type Element = ViewElement<Self>;

    #[track_caller]
    fn into_element(self) -> Self::Element {
        ViewElement::new(self)
    }
}

impl Styled for Control {
    fn style(&mut self) -> &mut StyleRefinement {
        self.button.style()
    }
}

impl ParentElement for Control {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.button.extend(elements);
    }
}

impl InteractiveElement for Control {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.button.interactivity()
    }

    /// Replaces the hover look, which render applies while the control is enabled.
    fn hover(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.hover = Some(f(StyleRefinement::default()));
        self
    }
}

impl StatefulInteractiveElement for Control {
    /// Names the control; a later `ld_tooltip` then describes it instead of renaming it.
    fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.named = true;
        self.button = self.button.aria_label(label);
        self
    }
}

/// What a screen adds to a control. `ld_click` decides whether the control is enabled, which
/// only a `Control` can act on (render drops its hover look and its tab stop), so this is for
/// `Control`s.
pub trait LdExt: StatefulInteractiveElement + Styled + Sized {
    /// Wires `handler` while `enabled`. Otherwise the control is dimmed to 0.4 and inert, as a
    /// disabled gpui-kit button is: no hover look, no click, no tab stop.
    fn ld_click(
        self,
        enabled: bool,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self;

    /// LocalDiff's small button: 22 px high, 12 px text.
    fn ld_small(self) -> Self {
        self.h(px(SMALL_HEIGHT)).px(px(8.)).text_size(px(12.))
    }

    /// A tooltip. A control with a visible label keeps that label as its accessible name and
    /// gets the tooltip as its description (WCAG 2.5.3, label in name); a control without a
    /// name is named by the tooltip.
    fn ld_tooltip(self, text: impl Into<SharedString>) -> Self;

    /// The "on" state of a toggling button: accent border and text on panel3.
    fn ld_on(self, on: bool, p: &Palette) -> Self {
        if on {
            self.border_color(p.accent)
                .text_color(p.accent)
                .bg(p.panel3)
        } else {
            self
        }
    }
}

impl LdExt for Control {
    fn ld_click(
        mut self,
        enabled: bool,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.enabled = enabled;
        self.button = self.button.on_click(handler);
        self
    }

    fn ld_tooltip(self, text: impl Into<SharedString>) -> Self {
        let text = text.into();
        if self.named {
            self.aria_description(text.clone()).tip(text)
        } else {
            self.aria_label(text.clone()).tip(text)
        }
    }
}

/// The shared button body: panel2, 1 px line border, 7 px radius, accent border on hover,
/// a 2 px accent ring when focused from the keyboard. gpui has no outline, so the ring is the
/// border: a button sized by its label grows 1 px a side while it shows.
fn control(id: impl Into<ElementId>, p: &Palette) -> Control {
    let accent = p.accent;
    Control::new(id, Role::Button)
        .flex_none()
        .h(px(CONTROL_HEIGHT))
        .px(px(9.))
        .gap(px(5.))
        .whitespace_nowrap()
        .rounded(px(RADIUS))
        .border_1()
        .border_color(p.line)
        .bg(p.panel2)
        .text_color(p.text)
        .cursor_pointer()
        .hover(move |s| s.border_color(accent))
        .focus_visible(move |s| s.border_2().border_color(accent))
}

pub fn ld_button(id: impl Into<ElementId>, label: impl Into<SharedString>, p: &Palette) -> Control {
    control(id, p).labelled(label.into())
}

/// A chrome icon from `ui::icons`, `size` px square, in the text colour of the control around
/// it: hover, `ld_on`, the disabled dim and the accent reach it as they reach a label.
pub fn ld_icon(icon: IconName, size: f32) -> Icon {
    Icon::new(icon).size(px(size))
}

/// `ld_button` with a 13 px icon before its label; the label alone is the name.
pub fn ld_icon_label_button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    p: &Palette,
) -> Control {
    control(id, p)
        .child(ld_icon(icon, LABEL_ICON_SIZE))
        .labelled(label.into())
}

/// `ld_button` with a 13 px icon after its label, as "All →" and "Copy →" point where they
/// copy to; the label alone is the name.
pub fn ld_label_icon_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: IconName,
    p: &Palette,
) -> Control {
    control(id, p)
        .labelled(label.into())
        .child(ld_icon(icon, LABEL_ICON_SIZE))
}

/// `ld_button` with the drawn folder before its label: the folder pickers.
pub fn ld_folder_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    p: &Palette,
) -> Control {
    control(id, p)
        .child(ld_folder_icon(p))
        .labelled(label.into())
}

/// A closed folder, 13×10 px in the accent amber: a tab over a rounded body. It is drawn
/// rather than the folder emoji, whose first glyph loads the colour-emoji font (about 18 ms
/// of the folder screen's first draw).
pub fn ld_folder_icon(p: &Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(13.))
        .h(px(10.))
        .child(
            div()
                .flex_none()
                .w(px(6.))
                .h(px(2.))
                .rounded_t(px(1.5))
                .bg(p.accent),
        )
        .child(
            div()
                .flex_none()
                .w_full()
                .h(px(8.))
                .rounded(px(2.))
                .rounded_tl(px(0.))
                .bg(p.accent),
        )
}

/// A clickable card or list entry: a button in the keyboard sense (tab stop, Enter and Space,
/// the 2 px accent ring) that lays its children out as a free-height box. `name` is its
/// accessible name.
pub fn ld_card(id: impl Into<ElementId>, name: impl Into<SharedString>, p: &Palette) -> Control {
    let accent = p.accent;
    Control::new(id, Role::Button)
        .aria_label(name)
        .bg(p.panel)
        .border_1()
        .border_color(p.line)
        .text_color(p.text)
        .text_left()
        .whitespace_normal()
        .cursor_pointer()
        .hover(move |s| s.border_color(accent))
        .focus_visible(move |s| s.border_2().border_color(accent))
}

/// A square icon-only button, named by its tooltip: 28 px with a 15 px icon.
pub fn ld_icon_button(
    id: impl Into<ElementId>,
    icon: IconName,
    tooltip: impl Into<SharedString>,
    p: &Palette,
) -> Control {
    control(id, p)
        .w(px(CONTROL_HEIGHT))
        .px_0()
        .child(ld_icon(icon, ICON_SIZE))
        .named_by_tip(tooltip.into())
}

/// `ld_icon_button` at `ld_small`'s 22 px, with a 13 px icon.
pub fn ld_small_icon_button(
    id: impl Into<ElementId>,
    icon: IconName,
    tooltip: impl Into<SharedString>,
    p: &Palette,
) -> Control {
    control(id, p)
        .ld_small()
        .w(px(SMALL_HEIGHT))
        .px_0()
        .child(ld_icon(icon, LABEL_ICON_SIZE))
        .named_by_tip(tooltip.into())
}

/// Solid accent with accent-ink text (`Control::as_primary`).
pub fn ld_primary(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    p: &Palette,
) -> Control {
    ld_button(id, label, p).as_primary(p)
}

/// A gpui-kit dropdown trigger in its row's type: the label at `text_size` (`SMALL_SIZE` in a
/// sub-bar, `UI_SIZE` in a main row, as the `ld_*` controls beside it draw theirs), then a
/// 12 px caret. gpui-kit's small Button draws its own label at 14 px with a 1.25 line height;
/// this one takes the neutral line height that gpui-base's Button gives every `ld_*` label, so
/// the text box is the font size. The label is the accessible name.
pub fn ld_dropdown(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    text_size: f32,
) -> KitButton {
    dropdown(id.into(), label.into(), text_size, None)
}

/// `ld_dropdown` as quiet text: no frame until hovered, its label and caret in `color`. For a
/// value shown in place that opens a menu: a text side's encoding in its name bar (U16).
pub fn ld_text_dropdown(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    text_size: f32,
    color: Hsla,
) -> KitButton {
    dropdown(id.into(), label.into(), text_size, Some(color))
        .ghost()
        .compact()
}

fn dropdown(id: ElementId, label: SharedString, text_size: f32, color: Option<Hsla>) -> KitButton {
    KitButton::new(id)
        .small()
        .line_height(relative(1.))
        .accessibility_label(label.clone())
        .child(
            div()
                .id(LABEL_ID)
                .test_support()
                .text_size(px(text_size))
                .when_some(color, |label, color| label.text_color(color))
                .child(label),
        )
        .child(
            ld_icon(icons::CARET, SMALL_ICON_SIZE)
                .when_some(color, |caret, color| caret.text_color(color)),
        )
}

pub fn ld_ghost(id: impl Into<ElementId>, label: impl Into<SharedString>, p: &Palette) -> Control {
    control(id, p).as_ghost(p).labelled(label.into())
}

/// The frame of a segmented control: a tab list, while each segment is a tab stop.
pub fn ld_segmented(
    id: impl Into<ElementId>,
    segments: impl IntoIterator<Item = Control>,
    p: &Palette,
) -> Control {
    Control::new(id, Role::TabList)
        .no_tab_stop()
        .flex_none()
        .h(px(26.))
        .rounded(px(RADIUS))
        .border_1()
        .border_color(p.line)
        .overflow_hidden()
        .children(segments)
}

/// One segment. Its ring trades padding for border, so the segment keeps its width.
pub fn ld_segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    last: bool,
    p: &Palette,
) -> Control {
    let (text, accent) = (p.text, p.accent);
    Control::new(id, Role::Tab)
        .h_full()
        .px(px(9.))
        .whitespace_nowrap()
        .cursor_pointer()
        .bg(if selected { p.panel3 } else { p.panel2 })
        .text_color(if selected { p.accent } else { p.muted })
        .when(!last, |d| d.border_r_1().border_color(p.line))
        .when(!selected, |d| d.hover(move |s| s.text_color(text)))
        .focus_visible(move |s| {
            s.border_2()
                .border_color(accent)
                .pl(px(7.))
                .pr(px(if last { 7. } else { 8. }))
        })
        .aria_selected(selected)
        .labelled(label.into())
}

/// A checkbox-style toggle: a 13 px box, accent with a 10 px check in accent ink when on, then
/// the label. A clear 2 px border holds the place of the ring.
pub fn ld_toggle(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
    p: &Palette,
) -> Control {
    let (text, accent) = (p.text, p.accent);
    let label = label.into();
    let check = div()
        .flex()
        .flex_none()
        .size(px(13.))
        .items_center()
        .justify_center()
        .rounded(px(3.))
        .border_1()
        .border_color(if on { p.accent } else { p.line })
        .bg(if on { p.accent } else { p.panel2 })
        .when(on, |d| {
            d.child(ld_icon(icons::CHECK, CHECK_SIZE).text_color(p.accent_ink))
        });
    Control::new(id, Role::CheckBox)
        .flex_none()
        .h(px(24.))
        .gap(px(6.))
        .whitespace_nowrap()
        .rounded(px(RADIUS))
        .border_2()
        .border_color(transparent_black())
        .cursor_pointer()
        .text_color(if on { p.text } else { p.muted })
        .hover(move |s| s.text_color(text))
        .focus_visible(move |s| s.border_color(accent))
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .child(check)
        .aria_label(label.clone())
        .child(div().id(LABEL_ID).test_support().child(label))
}

/// A filter chip: an outlined pill, muted label and bold count; active is an accent border on
/// panel3. Its ring trades padding for border, so the chip keeps its width.
pub fn ld_chip(
    id: impl Into<ElementId>,
    label: &'static str,
    count: usize,
    tone: Tone,
    active: bool,
    p: &Palette,
) -> Control {
    let accent = p.accent;
    let count = crate::format::format_size(count as u64);
    Control::new(id, Role::Tab)
        .flex_none()
        .h(px(24.))
        .px(px(11.))
        .gap(px(5.))
        .rounded_full()
        .border_1()
        .border_color(if active { p.accent } else { p.line })
        .bg(if active { p.panel3 } else { p.panel2 })
        .text_color(if active { p.text } else { p.muted })
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(move |s| s.border_color(accent))
        .focus_visible(move |s| s.border_2().border_color(accent).px(px(10.)))
        .aria_selected(active)
        .aria_label(format!("{label} {count}"))
        .child(label)
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::BOLD)
                .text_color(tone.color(p))
                .child(count),
        )
}

/// The centre gutter's 23×18 copy buttons with a 12 px icon; the current block's have an
/// accent border.
pub fn ld_gutter_button(
    id: impl Into<ElementId>,
    icon: IconName,
    tooltip: &'static str,
    current: bool,
    p: &Palette,
) -> Control {
    let (accent, panel3) = (p.accent, p.panel3);
    Control::new(id, Role::Button)
        .flex_none()
        .w(px(23.))
        .h(px(18.))
        .rounded(px(GUTTER_RADIUS))
        .border_1()
        .border_color(if current { p.accent } else { p.line })
        .bg(p.panel2)
        .text_color(p.text)
        .cursor_pointer()
        .hover(move |s| s.border_color(accent).text_color(accent).bg(panel3))
        .focus_visible(move |s| s.border_2().border_color(accent))
        .aria_selected(current)
        .child(ld_icon(icon, SMALL_ICON_SIZE))
        .named_by_tip(tooltip.into())
}

/// A key chip: panel2, 1 px line border with a 2 px bottom, 11 px code text.
pub fn ld_kbd(keys: impl Into<SharedString>, code: &Font, p: &Palette) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .rounded(px(5.))
        .border_1()
        .border_b_2()
        .border_color(p.line)
        .bg(p.panel2)
        .font(code.clone())
        .text_size(px(11.))
        .text_color(p.muted)
        .child(keys.into())
}

/// The 1×20 px line between toolbar groups.
pub fn ld_separator(p: &Palette) -> Div {
    div().flex_none().w(px(1.)).h(px(20.)).mx(px(3.)).bg(p.line)
}

/// A count badge: 10 px bold text in a full-radius pill.
pub fn ld_pill(text: impl Into<SharedString>, fg: Hsla, bg: Hsla) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .rounded_full()
        .bg(bg)
        .text_color(fg)
        .text_size(px(BADGE_SIZE))
        .font_weight(FontWeight::BOLD)
        .child(text.into())
}

pub fn ld_dot(size: f32, color: Hsla) -> Div {
    div().flex_none().size(px(size)).rounded_full().bg(color)
}

/// The widest a tooltip grows; longer text wraps, a path at its separators.
pub const TOOLTIP_MAX_WIDTH: f32 = 480.;

/// A tooltip whose text wraps within `TOOLTIP_MAX_WIDTH`, so a long path or message never
/// runs off the window. Every tooltip duet shows is one of these.
pub fn ld_tip(text: SharedString, window: &mut Window, cx: &mut App) -> AnyView {
    Tooltip::element(move |_, _| tip_text(text.clone())).build(window, cx)
}

/// A rough width per character of tooltip text.
const TOOLTIP_CHAR_WIDTH: f32 = 6.5;

/// A tooltip's text. In Thai its pieces are a wrapping row, which with no width of its own
/// shrinks to the narrowest piece: one word a line. So it asks for the width the text would
/// take, up to the tooltip's limit.
pub fn tip_text(text: SharedString) -> impl IntoElement {
    let wanted = text.chars().count() as f32 * TOOLTIP_CHAR_WIDTH;
    div()
        .id("tooltip-text")
        .test_support()
        .aria_label(text.clone())
        .min_w(px(wanted.min(TOOLTIP_MAX_WIDTH)))
        .max_w(px(TOOLTIP_MAX_WIDTH))
        .prose(text)
}

/// Below this window width the status bar drops its legend.
pub const LEGEND_MIN_WIDTH: f32 = 1250.;

/// The bar under a view: panel background, 12 px muted text, a 1 px line on top, 28 px at
/// least. Its children are the status text and, on a wide window, the legend.
pub fn status_bar(id: &'static str, p: &Palette) -> Control {
    Control::new(id, Role::Group)
        .no_tab_stop()
        .flex()
        .flex_row()
        .items_center()
        .flex_none()
        .min_h(px(28.))
        .px(px(12.))
        .py(px(4.))
        .gap(px(12.))
        .bg(p.panel)
        .border_t_1()
        .border_color(p.line)
        .text_size(px(12.))
        // The inherited line height is the font size; the parts row clips to it, which cut the
        // descenders of "y" and "g".
        .line_height(px(16.))
        .text_color(p.muted)
}

/// Status parts as one line of text, each in its tone, with a muted " · " between them.
pub fn status_parts_text(parts: &[StatusPart], p: &Palette) -> StyledText {
    let mut text = String::new();
    let mut highlights = Vec::new();
    let mut push = |piece: &str, color: Hsla| {
        let start = text.len();
        text.push_str(piece);
        if !piece.is_empty() {
            let style = HighlightStyle {
                color: Some(color),
                ..Default::default()
            };
            highlights.push((start..text.len(), style));
        }
    };
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            push(" · ", p.muted);
        }
        push(&part.text, part.tone.color(p));
    }
    StyledText::new(text).with_highlights(highlights)
}

/// The status bar's text: it takes the room the bar has left, and a line too long for it
/// ends in "…" instead of being cut mid-word or wrapping (the bar never grows a line). Anything
/// after it in the bar keeps its own room first. The whole sentence, `plain`, is its tooltip,
/// drawn by `.prose` so Thai wraps between phrases there. `notice` is the colour of a notice,
/// which shrinks before it grows the bar.
fn status_box(
    id: &'static str,
    plain: impl Into<SharedString>,
    text: impl IntoElement,
    notice: Option<Hsla>,
) -> AnyElement {
    let tip: SharedString = plain.into();
    let has_text = !tip.is_empty();
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        // A Thai mark on the last letter draws past the measured width, and the cut would take it.
        .pr(px(4.))
        .truncate()
        .when_some(notice, |line, color| line.flex_initial().text_color(color))
        .child(text)
        .when(has_text, |line| {
            line.tooltip(move |window, cx| ld_tip(tip.clone(), window, cx))
        })
        .test_support()
        .into_any_element()
}

/// The status bar's text, in one line.
pub fn status_line_box(
    id: &'static str,
    plain: impl Into<SharedString>,
    text: impl IntoElement,
) -> AnyElement {
    status_box(id, plain, text, None)
}

/// Status parts, each in its tone, with a muted " · " between them, as the bar's text.
pub fn status_parts_row(id: &'static str, parts: &[StatusPart], p: &Palette) -> AnyElement {
    status_box(id, join_parts(parts), status_parts_text(parts, p), None)
}

/// A notice in the bar (progress, a refusal, a warning): text colour, one line, cut with an
/// ellipsis, the whole sentence in its tooltip.
pub fn status_notice(id: &'static str, notice: impl Into<SharedString>, p: &Palette) -> AnyElement {
    let notice: SharedString = notice.into();
    status_box(id, notice.clone(), notice, Some(p.text))
}

/// The five colours of the diff, each named, so no colour stands alone.
pub fn legend(id: &'static str, p: &Palette) -> Control {
    let item = |color: Hsla, label: &'static str| {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(5.))
            .child(div().flex_none().size(px(9.)).rounded(px(2.)).bg(color))
            .child(label)
    };
    Control::new(id, Role::Group)
        .no_tab_stop()
        .flex()
        .flex_row()
        .items_center()
        .flex_none()
        .gap(px(12.))
        .whitespace_nowrap()
        .child(item(p.modified, tr("Changed")))
        .child(item(p.del, tr("Left only")))
        .child(item(p.ins, tr("Right only")))
        .child(item(p.moved, tr("Moved")))
        .child(item(p.unimp, tr("Unimportant")))
}

/// The 45° stripes of filler cells. gpui paints the pattern in device pixels, so the stripe
/// scales with the window.
pub fn hatch(p: &Palette, window: &Window) -> Background {
    let stripe = HATCH_STRIPE * window.scale_factor();
    pattern_slash(p.hatch, stripe, stripe)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui_kit::{Context, Element as _, Render, TestAppContext, accesskit, canvas};

    use super::*;

    /// The label and description gpui publishes for a control, read from the accessibility
    /// node its gpui-kit button writes.
    type Published = (Option<String>, Option<String>);

    struct Probe {
        published: Rc<RefCell<Vec<Published>>>,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let published = self.published.clone();
            canvas(
                move |_, window, cx| {
                    let p = Palette::dark();
                    let controls = [
                        ld_button("b", "Save left", &p).ld_tooltip("Save the left file"),
                        ld_segment("s", "All", true, false, &p).ld_tooltip("All lines (Ctrl+1)"),
                        ld_toggle("t", "Moves", false, &p).ld_tooltip("Mark lines that moved"),
                        ld_icon_button("i", icons::NEXT, "Next difference (F8)", &p),
                        Control::new("n", Role::Button).ld_tooltip("Close"),
                        ld_label_icon_button("a", "All", icons::TO_RIGHT, &p)
                            .ld_tooltip("Copy all to the right"),
                        ld_icon_label_button("l", icons::LEGEND, "Legend", &p)
                            .ld_tooltip("What the symbols and colours mean"),
                    ];
                    *published.borrow_mut() = controls
                        .into_iter()
                        .map(|control| {
                            let mut node = accesskit::Node::new(Role::Unknown);
                            control
                                .into_button()
                                .render(window, cx)
                                .into_element()
                                .write_a11y_info(&mut node);
                            (
                                node.label().map(str::to_owned),
                                node.description().map(str::to_owned),
                            )
                        })
                        .collect();
                },
                |_, _, _, _| {},
            )
        }
    }

    #[gpui_kit::test]
    fn a_tooltip_names_only_a_control_without_a_label(cx: &mut TestAppContext) {
        let published = Rc::new(RefCell::new(Vec::new()));
        let probe = published.clone();
        let (_, cx) = cx.add_window_view(move |_, _| Probe { published: probe });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let text = |s: &str| Some(s.to_owned());
        assert_eq!(
            *published.borrow(),
            [
                (text("Save left"), text("Save the left file")),
                (text("All"), text("All lines (Ctrl+1)")),
                (text("Moves"), text("Mark lines that moved")),
                (text("Next difference (F8)"), None),
                (text("Close"), None),
                (text("All"), text("Copy all to the right")),
                (text("Legend"), text("What the symbols and colours mean")),
            ],
            "a visible label stays the name and the tooltip describes it, with or without an \
             icon beside it; an icon alone is named by its tooltip"
        );
    }

    #[test]
    fn tones_use_the_status_colours() {
        let p = Palette::dark();
        assert_eq!(Tone::Changed.color(&p), p.modified);
        assert_eq!(Tone::LeftOnly.color(&p), p.del);
        assert_eq!(Tone::RightOnly.color(&p), p.ins);
        assert_eq!(Tone::Moved.color(&p), p.moved);
        assert_eq!(Tone::Unimportant.color(&p), p.unimp);
        assert_eq!(Tone::Muted.color(&p), p.muted);
    }

    #[test]
    fn a_disabled_control_keeps_its_look_under_the_pointer() {
        let p = Palette::dark();
        let noop = |_: &ClickEvent, _: &mut Window, _: &mut App| {};
        let controls = |enabled: bool| {
            [
                ld_button("b", "Save", &p).ld_click(enabled, noop),
                ld_icon_button("i", icons::NEXT, "Next difference (F8)", &p)
                    .ld_click(enabled, noop),
                ld_small_icon_button("x", icons::CLOSE, "Empty this slot", &p)
                    .ld_click(enabled, noop),
                ld_label_icon_button("a", "All", icons::TO_RIGHT, &p).ld_click(enabled, noop),
                ld_primary("p", "Compare", &p).ld_click(enabled, noop),
                ld_ghost("g", "Show all", &p).ld_click(enabled, noop),
                ld_segment("s", "Same", false, true, &p).ld_click(enabled, noop),
                ld_toggle("t", "Case", false, &p).ld_click(enabled, noop),
                ld_chip("c", "Changed", 2, Tone::Changed, false, &p).ld_click(enabled, noop),
                ld_gutter_button(
                    "u",
                    icons::TO_RIGHT,
                    "Copy block to the right (Alt+→)",
                    false,
                    &p,
                )
                .ld_click(enabled, noop),
            ]
        };
        for control in controls(true) {
            assert!(
                control.hover_look().is_some(),
                "enabled controls answer the pointer"
            );
        }
        for control in controls(false) {
            assert!(control.hover_look().is_none(), "disabled controls do not");
        }
    }
}
