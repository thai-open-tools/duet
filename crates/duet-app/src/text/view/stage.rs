//! The stage's columns (spec 6.4, items 4-5): left pane | centre gutter | right pane |
//! scrollbar | overview. The names bar and the rows use the same widths, so they line up.

use std::ops::Range;

use super::*;
use crate::i18n::tr;
use crate::pathbar::{path_bar_click, path_input};
use crate::text::menu::path_menu;
use crate::ui::menu::fill as fill_menu;
use crate::ui::style::{SCROLLBAR_WIDTH, ld_dot};

/// A half's line-number cell (the number and its right border).
pub const NUMBER_CELL: f32 = 52.0;
/// Space between the number cell and the text.
pub const TEXT_INSET: f32 = 10.0;
/// The copy-block buttons' column between the halves.
pub const CENTRE_GUTTER: f32 = 56.0;
pub use crate::ui::style::OVERVIEW_WIDTH;

#[derive(Debug, Clone, PartialEq)]
pub struct StageColumns {
    pub left: Range<f32>,
    pub gutter: Range<f32>,
    pub right: Range<f32>,
    pub scrollbar: Range<f32>,
    pub overview: Range<f32>,
}

/// Where each column sits in a text view `width` wide.
pub fn stage_columns(width: f32) -> StageColumns {
    let half = ((width - OVERVIEW_WIDTH - SCROLLBAR_WIDTH - CENTRE_GUTTER) / 2.).max(0.);
    let gutter = half..half + CENTRE_GUTTER;
    let right = gutter.end..gutter.end + half;
    let scrollbar = right.end..right.end + SCROLLBAR_WIDTH;
    StageColumns {
        left: 0.0..half,
        overview: scrollbar.end..scrollbar.end + OVERVIEW_WIDTH,
        gutter,
        right,
        scrollbar,
    }
}

/// The text columns one half shows in a row list `list_width` wide.
pub fn view_columns(list_width: Pixels, advance: Pixels) -> usize {
    let half = (list_width - px(SCROLLBAR_WIDTH + CENTRE_GUTTER)) / 2.;
    let text = half - px(NUMBER_CELL + TEXT_INSET);
    // f32 rounding can land an exact fit (3 x 7.5 px) a hair under its count.
    (text / advance + 1e-3).floor().max(1.) as usize
}

/// The horizontal thumb's narrowest width, so a minified line's thumb can still be grabbed.
const MIN_THUMB: f32 = 16.;

fn thumb_width(bar: f32, range: usize, shown: usize) -> f32 {
    (bar * shown as f32 / range.max(shown).max(1) as f32).clamp(MIN_THUMB.min(bar), bar)
}

/// The horizontal thumb on a bar `bar` px wide: its left edge from the bar's start and its
/// width, for `skip` of `range` columns with `shown` of them on screen. The thumb travels the
/// bar less its own width, so a thumb held at its 16 px minimum still ends where the bar does
/// when the line's end is on screen.
pub fn hscroll_thumb(bar: f32, range: usize, shown: usize, skip: usize) -> (f32, f32) {
    let width = thumb_width(bar, range, shown);
    let room = range.saturating_sub(shown);
    let at = if room == 0 {
        0.
    } else {
        skip.min(room) as f32 / room as f32
    };
    ((bar - width) * at, width)
}

/// The skip that puts the thumb's left edge `left` px along the bar, as a drag does.
pub fn hscroll_skip(bar: f32, left: f32, range: usize, shown: usize) -> usize {
    let width = thumb_width(bar, range, shown);
    let room = range.saturating_sub(shown);
    if room == 0 || bar <= width {
        return 0;
    }
    let at = (left / (bar - width)).clamp(0., 1.);
    (at * room as f32).round() as usize
}

impl TextView {
    /// Each side's path, its encoding, line endings and line count (the encoding's dropdown,
    /// U16), and a dot while it holds unsaved edits, over the same columns as the rows.
    pub(crate) fn render_names(
        &self,
        p: &Palette,
        width: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let columns = stage_columns(width);
        let mut side = |left: bool| {
            let path = if left {
                &self.request.left
            } else {
                &self.request.right
            };
            let model = self.model.as_ref();
            let dirty = model.is_some_and(|m| m.is_dirty(left));
            h_flex()
                .id(if left {
                    "text-names-left"
                } else {
                    "text-names-right"
                })
                .test_support()
                .context_menu({
                    let (view, focus) = (cx.entity().downgrade(), self.focus.clone());
                    move |menu, _, cx: &mut Context<PopupMenu>| {
                        let entries = view
                            .upgrade()
                            .map(|v| path_menu(v.read(cx), left))
                            .unwrap_or_default();
                        fill_menu(menu, entries, Some(focus.clone()))
                    }
                })
                .flex_none()
                .w(px(columns.left.end - columns.left.start))
                .h_full()
                .gap(px(8.))
                .px(px(10.))
                .overflow_hidden()
                .when(dirty, |d| {
                    d.child(
                        ld_dot(8., p.accent)
                            .id(if left {
                                "text-dirty-left"
                            } else {
                                "text-dirty-right"
                            })
                            .test_support(),
                    )
                })
                .child({
                    let id = if left {
                        "text-left-path"
                    } else {
                        "text-right-path"
                    };
                    match self.path_edit.as_ref().filter(|e| e.left() == left) {
                        Some(edit) => path_input(id, edit.edit(), p),
                        // No file yet: the name is a hint, and a click still opens the box.
                        None if path.as_os_str().is_empty() => h_flex()
                            .id(id)
                            .test_support()
                            .flex_1()
                            .min_w_0()
                            .cursor_text()
                            .text_color(p.muted)
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.begin_path_edit(left, window, cx)
                            }))
                            .child(tr("untitled"))
                            .into_any_element(),
                        None => path_bar_click(
                            id,
                            path,
                            p,
                            cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.begin_path_edit(left, window, cx)
                            }),
                        )
                        .into_any_element(),
                    }
                })
                .child(
                    div()
                        .id(if left {
                            "text-info-left"
                        } else {
                            "text-info-right"
                        })
                        .test_support()
                        .flex_none()
                        .child(self.encoding_menu(left, p, cx)),
                )
        };
        h_flex()
            .id("text-names")
            .test_support()
            .flex_none()
            .h(px(30.))
            .bg(p.panel2)
            .border_b_1()
            .border_color(p.line)
            .child(side(true))
            .child(
                div()
                    .id("text-names-gutter")
                    .test_support()
                    .flex_none()
                    .w(px(columns.gutter.end - columns.gutter.start))
                    .h_full()
                    .bg(p.bg)
                    .border_l_1()
                    .border_r_1()
                    .border_color(p.line2),
            )
            .child(side(false))
            .child(
                div()
                    .id("text-names-tail")
                    .test_support()
                    .flex_none()
                    .w(px(columns.overview.end - columns.scrollbar.start))
                    .h_full(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::px;

    #[test]
    fn the_columns_tile_the_width_and_the_halves_match() {
        for width in [700., 900., 1200., 1400., 1919.5] {
            let c = stage_columns(width);
            let len = |r: &Range<f32>| r.end - r.start;
            assert_eq!(c.left.start, 0.);
            assert!(
                (len(&c.left) - len(&c.right)).abs() < 1e-3,
                "{width}: {c:?}"
            );
            assert!((c.gutter.start - c.left.end).abs() < 1e-3);
            assert!((len(&c.gutter) - CENTRE_GUTTER).abs() < 1e-3);
            assert!((c.right.start - c.gutter.end).abs() < 1e-3);
            assert!((c.scrollbar.start - c.right.end).abs() < 1e-3);
            assert!((len(&c.scrollbar) - SCROLLBAR_WIDTH).abs() < 1e-3);
            assert!((c.overview.start - c.scrollbar.end).abs() < 1e-3);
            assert!(
                (c.overview.end - width).abs() < 1e-3
                    && (len(&c.overview) - OVERVIEW_WIDTH).abs() < 1e-3
            );
        }
    }

    #[test]
    fn the_horizontal_thumb_maps_the_whole_line_and_stays_on_the_bar() {
        let bar = 1000.;
        // 20,000 columns with 80 on screen: the thumb is at its 16 px minimum.
        assert_eq!(hscroll_thumb(bar, 20_000, 80, 0), (0., 16.));
        let (x, w) = hscroll_thumb(bar, 20_000, 80, 20_000 - 80);
        assert!(
            (x + w - bar).abs() < 1e-3,
            "at the end the thumb ends the bar: {x} {w}"
        );
        let (x, w) = hscroll_thumb(bar, 20_000, 80, 30_000);
        assert!(
            x + w <= bar + 1e-3,
            "a skip past the end keeps it on the bar"
        );
        assert_eq!(
            hscroll_thumb(bar, 200, 100, 100),
            (500., 500.),
            "half the line shown, scrolled to its end"
        );
        // A thumb's left edge maps back to its skip.
        for skip in [0, 5_000, 9_960, 19_920] {
            let (x, _) = hscroll_thumb(bar, 20_000, 80, skip);
            let back = hscroll_skip(bar, x, 20_000, 80);
            assert!(back.abs_diff(skip) <= 20, "{skip} comes back as {back}");
        }
        assert_eq!(hscroll_skip(bar, bar, 20_000, 80), 19_920, "the bar's end");
        assert_eq!(hscroll_skip(bar, -50., 20_000, 80), 0);
    }

    #[test]
    fn a_half_counts_every_column_its_text_area_holds() {
        let advance = px(7.5);
        for n in 2..=300 {
            // Per half: the number cell, the inset, then n columns; the halves share the centre
            // gutter, and the rows leave room for the vertical scrollbar.
            let list = px(2. * (NUMBER_CELL + TEXT_INSET + n as f32 * 7.5)
                + CENTRE_GUTTER
                + SCROLLBAR_WIDTH);
            assert_eq!(view_columns(list, advance), n, "exactly {n} columns fit");
            assert_eq!(
                view_columns(list - px(1.), advance),
                n - 1,
                "a pixel short of {n}"
            );
        }
        assert_eq!(view_columns(px(10.), advance), 1, "never 0 once measured");
    }
}
