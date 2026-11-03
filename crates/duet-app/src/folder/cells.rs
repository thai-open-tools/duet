//! Folder row cells: one side of a row, its tree guides, and the width rules that keep names
//! readable on a narrow window.

use std::time::SystemTime;

use duet_core::folder::entry::Entry;
use duet_core::folder::tree::{Node, Status};
use gpui_kit::TestSupportExt as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::{
    AnyElement, App, Background, FontWeight, Hsla, MouseButton, MouseDownEvent, SharedString,
    Window, canvas, div, fill, point, prelude::*, px, size,
};

use crate::format::format_time;
use crate::palette::Palette;
use crate::status::Tone;
use crate::ui::fonts::Fonts;
use crate::ui::style::{
    SCROLLBAR_WIDTH, SMALL_ICON_SIZE, ld_folder_icon, ld_icon, ld_pill, ld_tip,
};

pub(super) const INDENT: f32 = 16.;
pub(super) const MID_COL: f32 = 26.;
pub(super) const SIZE_COL: f32 = 84.;
pub(super) const DATE_COL: f32 = 140.;

/// The middle glyph and its tone: "=" Muted ("=" Moved when the times differ), "≠" Changed,
/// "⚠" LeftOnly (any warning), "≈" Unimportant, "◀" LeftOnly, "▶" RightOnly, "…" Muted, "!" LeftOnly.
pub fn status_glyph(node: &Node, time_differs: bool) -> (&'static str, Tone) {
    match &node.status {
        Status::Different { .. } | Status::Similar { .. } if node.warning().is_some() => {
            ("⚠", Tone::LeftOnly)
        }
        Status::Same if time_differs => ("=", Tone::Moved),
        Status::Same => ("=", Tone::Muted),
        Status::Different { .. } => ("≠", Tone::Changed),
        Status::Similar { .. } => ("≈", Tone::Unimportant),
        Status::LeftOnly => ("◀", Tone::LeftOnly),
        Status::RightOnly => ("▶", Tone::RightOnly),
        Status::Pending => ("…", Tone::Muted),
        Status::Error(_) => ("!", Tone::LeftOnly),
    }
}

/// `status_glyph` with its colour. A plain "=" is dim, so the row never rests on colour alone
/// (spec section 3).
pub(super) fn status_style(
    node: &Node,
    time_differs: bool,
    palette: &Palette,
) -> (&'static str, Hsla) {
    match status_glyph(node, time_differs) {
        ("=", Tone::Muted) => ("=", palette.dim),
        (glyph, tone) => (glyph, tone.color(palette)),
    }
}

/// A name's colour on one side: a warned pair `del`, then `name_color`.
pub(super) fn side_name_color(node: &Node, left: bool, p: &Palette) -> Hsla {
    if node.warning().is_some() {
        p.del
    } else {
        name_color(&node.status, left, p)
    }
}

/// A name's colour on one side: left-only red on the left, right-only green on the right,
/// changed amber on both, everything else muted.
pub fn name_color(status: &Status, left: bool, p: &Palette) -> Hsla {
    match (status, left) {
        (Status::Different { .. } | Status::Similar { .. }, _) => p.modified,
        (Status::LeftOnly, true) => p.del,
        (Status::RightOnly, false) => p.ins,
        (Status::Error(_), _) => p.del,
        _ => p.muted,
    }
}

/// 1 px guides, one per level, painted in one canvas per indented row.
pub(super) fn tree_guides(depth: u32, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            for level in 0..depth {
                let x = bounds.left() + px(INDENT * level as f32 + INDENT / 2.);
                window.paint_quad(fill(
                    gpui_kit::Bounds::new(point(x, bounds.top()), size(px(1.), bounds.size.height)),
                    color,
                ));
            }
        },
    )
    .flex_none()
    .w(px(INDENT * depth as f32))
    .h_full()
}

/// A folder's file-count pill: its text, whether it is neutral (under the All and Same chips),
/// and the tooltip with the folder's whole breakdown.
#[derive(Clone)]
pub(super) struct Pill {
    pub(super) text: SharedString,
    pub(super) neutral: bool,
    pub(super) tip: SharedString,
}

pub(super) struct SideCell<'a, F> {
    pub(super) node: &'a Node,
    pub(super) entry: Option<&'a Entry>,
    pub(super) row: usize,
    pub(super) left: bool,
    /// A folder's caret, open or closed; none for a file or an empty folder.
    pub(super) expander: Option<IconName>,
    /// The size column's text: a file's own size, a folder's total (blank when 0).
    pub(super) size: String,
    /// The date column: a file's time, a folder's newest file below (else its own).
    pub(super) modified: Option<SystemTime>,
    pub(super) newer: bool,
    /// The same contents at another time: the dates are italic.
    pub(super) other_time: bool,
    /// The full path on this side, the name's tooltip.
    pub(super) path: SharedString,
    /// The narrowest the name may get: about four characters and an ellipsis.
    pub(super) name_min: f32,
    /// The count pill: a folder with files shown below it, and room beside the name.
    pub(super) pill: Option<Pill>,
    /// A press on the expander toggles the folder instead of reaching the row.
    pub(super) on_expander: Option<F>,
    /// The invisible-character chip: its tooltip, and whether a hit is in the text or values.
    pub(super) odd: Option<(SharedString, bool)>,
}

/// The size of tree names, in the code font.
pub(super) const NAME_SIZE: f32 = 12.;
/// Names keep this many characters on a narrow window; the fifth column is the ellipsis.
const NAME_MIN_CHARS: usize = 4;

/// The narrowest a name may get: about four characters and the ellipsis after them, or all of
/// a name that short. `advance` is one column of the code font at `NAME_SIZE`.
pub fn name_min_width(name: &str, advance: f32) -> f32 {
    use unicode_segmentation::UnicodeSegmentation as _;
    name.graphemes(true).take(NAME_MIN_CHARS + 1).count() as f32 * advance
}

/// The width of one side cell in a row list `list` px wide (see `render_rows`).
pub(super) fn side_width(list: f32) -> f32 {
    // The row's 1 px border on each side and the scrollbar's room, then the glyph column.
    ((list - SCROLLBAR_WIDTH - 2. - MID_COL) / 2.).max(0.)
}

/// Whether a folder's count pill fits in a side cell `cell` px wide beside a name kept at
/// `name_min`, with the size and date columns at full width. When it does not, the pill gives
/// way first, so a narrow window never squeezes a name below a few characters to show a count.
pub fn pill_fits(cell: f32, depth: u32, name_min: f32, pill_chars: usize) -> bool {
    // Guides, expander, folder icon, name, pill, spacer, size, date: one gap between each.
    let parts = if depth > 0 { 8. } else { 7. };
    // The pill's characters, about 6 px each, and its 6 px padding a side.
    let pill = pill_chars as f32 * 6. + 12.;
    let fixed = 16.
        + INDENT * depth as f32
        + SMALL_ICON_SIZE
        + 13.
        + SIZE_COL
        + DATE_COL
        + 6. * (parts - 1.);
    fixed + name_min + pill <= cell
}

/// One side of a row: guides, expander, folder icon, name, count pill, size, modified time. A
/// missing side is the hatch so the two panes stay visibly aligned.
pub(super) fn side_cell<F>(
    c: SideCell<F>,
    p: &Palette,
    fonts: &Fonts,
    hatch: Background,
) -> AnyElement
where
    F: Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
{
    let SideCell {
        node,
        entry,
        row,
        left,
        expander,
        size,
        modified,
        newer,
        other_time,
        path,
        name_min,
        pill,
        on_expander,
        odd,
    } = c;
    let Some(entry) = entry else {
        let id = if left {
            "folder-missing-left"
        } else {
            "folder-missing-right"
        };
        return div()
            .id((id, row))
            .test_support()
            .flex_1()
            .h_full()
            .bg(hatch)
            .into_any_element();
    };
    let time = modified.map(format_time).unwrap_or_default();
    let (name_id, count_id, odd_id, size_id) = if left {
        (
            "folder-name-left",
            "folder-count-left",
            "folder-odd-left",
            "folder-size-left",
        )
    } else {
        (
            "folder-name-right",
            "folder-count-right",
            "folder-odd-right",
            "folder-size-right",
        )
    };
    let name: SharedString = entry.name().to_owned().into();
    h_flex()
        .flex_1()
        .min_w_0()
        .h_full()
        .px(px(8.))
        .gap(px(6.))
        .items_center()
        .when(node.depth > 0, |d| {
            d.child(tree_guides(node.depth, p.tree_guide))
        })
        .child(
            div()
                .flex_none()
                .w(px(SMALL_ICON_SIZE))
                .text_color(p.dim)
                .when_some(on_expander, |d, press| {
                    d.on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        cx.stop_propagation();
                        press(event, window, cx)
                    })
                })
                .when_some(expander, |d, icon| d.child(ld_icon(icon, SMALL_ICON_SIZE))),
        )
        .when(node.is_dir, |d| d.child(ld_folder_icon(p)))
        .child(
            div()
                .id((name_id, row))
                .test_support()
                .aria_label(name.clone())
                .flex_initial()
                .min_w(px(name_min))
                .truncate()
                .font(fonts.code.clone())
                .text_size(px(NAME_SIZE))
                .text_color(side_name_color(node, left, p))
                .when(node.is_dir, |d| d.font_weight(FontWeight::BOLD))
                .tooltip(move |window, cx| ld_tip(path.clone(), window, cx))
                .child(name),
        )
        .when_some(odd, |d, (tip, warn)| {
            let chip = if warn {
                ld_pill("⚠ invisible", p.del, p.bg_del)
            } else {
                ld_pill("invisible", p.muted, p.panel3)
            };
            d.child(
                chip.id((odd_id, row))
                    .test_support()
                    .tooltip(move |window, cx| ld_tip(tip.clone(), window, cx)),
            )
        })
        .when_some(pill, |d, pill| {
            let (fg, bg) = if pill.neutral {
                (p.muted, p.panel3)
            } else {
                (p.modified, p.bg_mod)
            };
            let tip = pill.tip;
            d.child(
                ld_pill(pill.text, fg, bg)
                    .id((count_id, row))
                    .test_support()
                    .tooltip(move |window, cx| ld_tip(tip.clone(), window, cx)),
            )
        })
        .child(div().flex_1())
        .child(
            div()
                .id((size_id, row))
                .test_support()
                .when(!size.is_empty(), |d| d.aria_label(size.clone()))
                .flex_none()
                .w(px(SIZE_COL))
                .text_right()
                .text_size(px(11.5))
                .text_color(p.muted)
                .child(size),
        )
        .child({
            // Last to give way: once the name is down to its few characters, the date ends in
            // "…" rather than running into the next column.
            let mut date = div();
            date.style().flex_shrink = Some(0.001);
            date.w(px(DATE_COL))
                .min_w_0()
                .truncate()
                .text_size(px(11.))
                .text_color(if newer { p.accent } else { p.muted })
                .when(newer, |d| d.font_weight(FontWeight::BOLD))
                .when(other_time, |d| d.italic())
                .child(if newer { format!("▲ {time}") } else { time })
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use duet_core::folder::classify::{Note, Warning};

    #[test]
    fn names_take_their_status_colour_on_their_own_side() {
        let p = Palette::dark();
        assert_eq!(name_color(&Status::LeftOnly, true, &p), p.del);
        assert_eq!(name_color(&Status::RightOnly, false, &p), p.ins);
        assert_eq!(
            name_color(&Status::Different { newer: None }, true, &p),
            p.modified
        );
        assert_eq!(
            name_color(&Status::Different { newer: None }, false, &p),
            p.modified
        );
        assert_eq!(name_color(&Status::Same, true, &p), p.muted);
        assert_eq!(
            name_color(&Status::LeftOnly, false, &p),
            p.muted,
            "never drawn: the side is missing"
        );
    }

    #[test]
    fn the_count_pill_gives_way_before_a_name_goes_under_four_characters() {
        let advance = 7.2;
        let name = name_min_width("charlie", advance);
        assert_eq!(name, 5. * advance, "four characters and the ellipsis");
        assert_eq!(
            name_min_width("src", advance),
            3. * advance,
            "a short name whole"
        );
        assert_eq!(
            name_min_width("โฟลเดอร์", advance),
            5. * advance,
            "by grapheme"
        );
        assert!(pill_fits(side_width(1400.), 3, name, "12 files".len()));
        assert!(
            !pill_fits(side_width(700.), 1, name, "1 file".len()),
            "at 700 px the pill would squeeze the name"
        );
    }

    fn node(status: Status) -> Node {
        Node {
            name: "x".into(),
            rel: "x".into(),
            depth: 0,
            parent: None,
            is_dir: false,
            left: None,
            right: None,
            status,
            note: None,
        }
    }

    fn warned(warning: Warning) -> Node {
        let mut n = node(Status::Different { newer: None });
        n.note = Some(Box::new(Note {
            warning: Some(warning),
            ..Note::default()
        }));
        n
    }

    #[test]
    fn same_rows_keep_a_dim_glyph() {
        let p = Palette::light();
        assert_eq!(status_glyph(&node(Status::Same), false), ("=", Tone::Muted));
        assert_eq!(status_style(&node(Status::Same), false, &p), ("=", p.dim));
        assert_eq!(status_style(&node(Status::LeftOnly), false, &p).0, "◀");
        assert_eq!(
            status_style(&node(Status::Same), true, &p),
            ("=", p.moved),
            "the same contents at another time"
        );
    }

    #[test]
    fn status_glyphs_follow_localdiff() {
        let glyph = |n: &Node, time| status_glyph(n, time);
        assert_eq!(glyph(&node(Status::Same), true), ("=", Tone::Moved));
        assert_eq!(
            glyph(&node(Status::Different { newer: None }), false),
            ("≠", Tone::Changed)
        );
        assert_eq!(
            glyph(&node(Status::Similar { newer: None }), false),
            ("≈", Tone::Unimportant)
        );
        for warning in [Warning::Script, Warning::OddSpace, Warning::CaseOnly] {
            assert_eq!(glyph(&warned(warning), false), ("⚠", Tone::LeftOnly));
        }
        assert_eq!(glyph(&node(Status::LeftOnly), false), ("◀", Tone::LeftOnly));
        assert_eq!(
            glyph(&node(Status::RightOnly), false),
            ("▶", Tone::RightOnly)
        );
        assert_eq!(glyph(&node(Status::Pending), false), ("…", Tone::Muted));
        assert_eq!(
            glyph(&node(Status::Error("no".into())), false),
            ("!", Tone::LeftOnly)
        );
    }
}
