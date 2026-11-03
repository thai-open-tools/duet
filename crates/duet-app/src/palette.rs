//! The colour tokens for both themes, carried over from LocalDiff (the author's earlier Electron
//! compare app, which duet replaces). These are the only colours duet writes;
//! views read them through `Palette::current`, which follows gpui-kit's theme mode.

use std::sync::LazyLock;

use gpui_kit::component::Theme;
use gpui_kit::{App, Hsla, rgb};

/// LocalDiff's fixed syntax colours (`--sx-*`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Syntax {
    pub comment: Hsla,
    pub string: Hsla,
    pub number: Hsla,
    pub keyword: Hsla,
    /// Types and functions.
    pub type_: Hsla,
    pub tag: Hsla,
    pub attr: Hsla,
    /// Decorators, labels and preprocessor lines.
    pub decorator: Hsla,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub bg: Hsla,
    pub panel: Hsla,
    pub panel2: Hsla,
    pub panel3: Hsla,
    pub line: Hsla,
    pub line2: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub dim: Hsla,
    pub accent: Hsla,
    /// Text on the accent.
    pub accent_ink: Hsla,
    pub del: Hsla,
    pub ins: Hsla,
    pub modified: Hsla,
    pub moved: Hsla,
    pub unimp: Hsla,
    pub bg_del: Hsla,
    pub bg_ins: Hsla,
    pub bg_mod: Hsla,
    pub bg_moved: Hsla,
    pub bg_unimp: Hsla,
    /// Opaque line-number cells of difference rows.
    pub ln_del: Hsla,
    pub ln_ins: Hsla,
    pub ln_mod: Hsla,
    pub ln_mov: Hsla,
    pub hl_del: Hsla,
    pub hl_ins: Hsla,
    pub hl_unimp: Hsla,
    /// Find matches; the current one also gets an accent outline.
    pub find: Hsla,
    pub sel: Hsla,
    /// The wash over the current difference's rows.
    pub cur_row: Hsla,
    /// The overview's viewport box.
    pub viewport: Hsla,
    pub tree_guide: Hsla,
    /// Stripes on filler rows and missing folder cells.
    pub hatch: Hsla,
    pub sx: Syntax,
}

static DARK: LazyLock<Palette> = LazyLock::new(Palette::build_dark);
static LIGHT: LazyLock<Palette> = LazyLock::new(Palette::build_light);

impl Palette {
    pub fn dark() -> Self {
        *DARK
    }

    pub fn light() -> Self {
        *LIGHT
    }

    pub fn of(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    /// The palette of the mode gpui-kit's theme shows now; `theme::toggle` switches it.
    pub fn current(cx: &App) -> Self {
        Self::of(Theme::global(cx).mode.is_dark())
    }

    fn build_dark() -> Self {
        Self {
            dark: true,
            bg: solid(0x0e1116),
            panel: solid(0x151a21),
            panel2: solid(0x1a2029),
            panel3: solid(0x202734),
            line: solid(0x2a3140),
            line2: solid(0x222936),
            text: solid(0xd7dde7),
            muted: solid(0x8b94a3),
            dim: solid(0x5d6674),
            accent: solid(0xe0aa4a),
            accent_ink: solid(0x17120a),
            del: solid(0xe26471),
            ins: solid(0x4db986),
            modified: solid(0xd9a53c),
            moved: solid(0x6e9fe8),
            unimp: solid(0xa093d8),
            bg_del: tint(0xe26471, 0.13),
            bg_ins: tint(0x4db986, 0.12),
            bg_mod: tint(0xd9a53c, 0.10),
            bg_moved: tint(0x6e9fe8, 0.12),
            bg_unimp: tint(0xa093d8, 0.08),
            ln_del: solid(0x291d24),
            ln_ins: solid(0x18271f),
            ln_mod: solid(0x292315),
            ln_mov: solid(0x1b2534),
            hl_del: tint(0xe26471, 0.42),
            hl_ins: tint(0x4db986, 0.38),
            hl_unimp: tint(0xa093d8, 0.30),
            // LocalDiff's .45 left its hits' text under 4.5:1; .26 keeps the hue and clears it
            // on every row tint (`find_hits_read_at_4_5_to_1_on_every_row`).
            find: tint(0xffd60a, 0.26),
            sel: tint(0x6e9fe8, 0.28),
            cur_row: tint(0xe0aa4a, 0.09),
            viewport: tint(0xe0aa4a, 0.14),
            tree_guide: tint(0x96a0b4, 0.20),
            hatch: tint(0x808ca0, 0.07),
            sx: Syntax {
                comment: solid(0x6f7a8a),
                string: solid(0xa5c97a),
                number: solid(0xd9946b),
                keyword: solid(0xc792ea),
                type_: solid(0x82aaff),
                tag: solid(0x7fb5d9),
                attr: solid(0xd9b56b),
                decorator: solid(0x6fb3b8),
            },
        }
    }

    fn build_light() -> Self {
        Self {
            dark: false,
            bg: solid(0xeef1f5),
            panel: solid(0xffffff),
            panel2: solid(0xf5f7fa),
            panel3: solid(0xeceff4),
            line: solid(0xd5dbe4),
            line2: solid(0xe2e7ee),
            text: solid(0x232830),
            muted: solid(0x66707e),
            dim: solid(0x98a1ae),
            accent: solid(0x9a6a12),
            accent_ink: solid(0xfffdf7),
            del: solid(0xc23d52),
            ins: solid(0x1e8a5c),
            modified: solid(0xa5770f),
            moved: solid(0x35639f),
            unimp: solid(0x6b5fb0),
            bg_del: tint(0xc23d52, 0.10),
            bg_ins: tint(0x1e8a5c, 0.10),
            bg_mod: tint(0xa5770f, 0.10),
            bg_moved: tint(0x35639f, 0.10),
            bg_unimp: tint(0x6b5fb0, 0.07),
            ln_del: solid(0xf6e4e7),
            ln_ins: solid(0xe2f1ea),
            ln_mod: solid(0xf3ecd9),
            ln_mov: solid(0xe2ebf6),
            hl_del: tint(0xc23d52, 0.28),
            hl_ins: tint(0x1e8a5c, 0.25),
            hl_unimp: tint(0x6b5fb0, 0.22),
            find: tint(0xffc400, 0.55),
            sel: tint(0x35639f, 0.22),
            cur_row: tint(0xe0aa4a, 0.09),
            viewport: tint(0xe0aa4a, 0.14),
            tree_guide: tint(0x46556e, 0.18),
            hatch: tint(0x808ca0, 0.07),
            sx: Syntax {
                comment: solid(0x8a93a0),
                string: solid(0x3f7d1e),
                number: solid(0xb0522a),
                keyword: solid(0x8a3fb0),
                type_: solid(0x2f5fb3),
                tag: solid(0x1f6f9f),
                attr: solid(0x8a6416),
                decorator: solid(0x237a80),
            },
        }
    }
}

fn solid(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn tint(hex: u32, alpha: f32) -> Hsla {
    let mut color: Hsla = rgb(hex).into();
    color.a = alpha;
    color
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::Rgba;

    /// Each channel within 1.5/255 of `hex` at `alpha` (an HSL round trip loses a little).
    fn assert_token(name: &str, actual: Hsla, hex: u32, alpha: f32) {
        let got: Rgba = actual.into();
        let want = [
            ((hex >> 16) & 0xff) as f32 / 255.,
            ((hex >> 8) & 0xff) as f32 / 255.,
            (hex & 0xff) as f32 / 255.,
            alpha,
        ];
        for (g, w) in [got.r, got.g, got.b, got.a].into_iter().zip(want) {
            assert!(
                (g - w).abs() <= 1.5 / 255.,
                "{name}: {got:?} is not #{hex:06x} at {alpha}"
            );
        }
    }

    #[test]
    fn dark_tokens_are_localdiffs() {
        let p = Palette::dark();
        assert!(p.dark);
        for (name, actual, hex, alpha) in [
            ("bg", p.bg, 0x0e1116, 1.0),
            ("panel", p.panel, 0x151a21, 1.0),
            ("panel2", p.panel2, 0x1a2029, 1.0),
            ("panel3", p.panel3, 0x202734, 1.0),
            ("line", p.line, 0x2a3140, 1.0),
            ("line2", p.line2, 0x222936, 1.0),
            ("text", p.text, 0xd7dde7, 1.0),
            ("muted", p.muted, 0x8b94a3, 1.0),
            ("dim", p.dim, 0x5d6674, 1.0),
            ("accent", p.accent, 0xe0aa4a, 1.0),
            ("accent_ink", p.accent_ink, 0x17120a, 1.0),
            ("del", p.del, 0xe26471, 1.0),
            ("ins", p.ins, 0x4db986, 1.0),
            ("modified", p.modified, 0xd9a53c, 1.0),
            ("moved", p.moved, 0x6e9fe8, 1.0),
            ("unimp", p.unimp, 0xa093d8, 1.0),
            ("bg_del", p.bg_del, 0xe26471, 0.13),
            ("bg_ins", p.bg_ins, 0x4db986, 0.12),
            ("bg_mod", p.bg_mod, 0xd9a53c, 0.10),
            ("bg_moved", p.bg_moved, 0x6e9fe8, 0.12),
            ("bg_unimp", p.bg_unimp, 0xa093d8, 0.08),
            ("ln_del", p.ln_del, 0x291d24, 1.0),
            ("ln_ins", p.ln_ins, 0x18271f, 1.0),
            ("ln_mod", p.ln_mod, 0x292315, 1.0),
            ("ln_mov", p.ln_mov, 0x1b2534, 1.0),
            ("hl_del", p.hl_del, 0xe26471, 0.42),
            ("hl_ins", p.hl_ins, 0x4db986, 0.38),
            ("hl_unimp", p.hl_unimp, 0xa093d8, 0.30),
            ("find", p.find, 0xffd60a, 0.26),
            ("sel", p.sel, 0x6e9fe8, 0.28),
            ("tree_guide", p.tree_guide, 0x96a0b4, 0.20),
            ("sx.comment", p.sx.comment, 0x6f7a8a, 1.0),
            ("sx.string", p.sx.string, 0xa5c97a, 1.0),
            ("sx.number", p.sx.number, 0xd9946b, 1.0),
            ("sx.keyword", p.sx.keyword, 0xc792ea, 1.0),
            ("sx.type", p.sx.type_, 0x82aaff, 1.0),
            ("sx.tag", p.sx.tag, 0x7fb5d9, 1.0),
            ("sx.attr", p.sx.attr, 0xd9b56b, 1.0),
            ("sx.decorator", p.sx.decorator, 0x6fb3b8, 1.0),
        ] {
            assert_token(name, actual, hex, alpha);
        }
    }

    #[test]
    fn light_tokens_are_localdiffs() {
        let p = Palette::light();
        assert!(!p.dark);
        for (name, actual, hex, alpha) in [
            ("bg", p.bg, 0xeef1f5, 1.0),
            ("panel", p.panel, 0xffffff, 1.0),
            ("panel2", p.panel2, 0xf5f7fa, 1.0),
            ("panel3", p.panel3, 0xeceff4, 1.0),
            ("line", p.line, 0xd5dbe4, 1.0),
            ("line2", p.line2, 0xe2e7ee, 1.0),
            ("text", p.text, 0x232830, 1.0),
            ("muted", p.muted, 0x66707e, 1.0),
            ("dim", p.dim, 0x98a1ae, 1.0),
            ("accent", p.accent, 0x9a6a12, 1.0),
            ("accent_ink", p.accent_ink, 0xfffdf7, 1.0),
            ("del", p.del, 0xc23d52, 1.0),
            ("ins", p.ins, 0x1e8a5c, 1.0),
            ("modified", p.modified, 0xa5770f, 1.0),
            ("moved", p.moved, 0x35639f, 1.0),
            ("unimp", p.unimp, 0x6b5fb0, 1.0),
            ("bg_del", p.bg_del, 0xc23d52, 0.10),
            ("bg_ins", p.bg_ins, 0x1e8a5c, 0.10),
            ("bg_mod", p.bg_mod, 0xa5770f, 0.10),
            ("bg_moved", p.bg_moved, 0x35639f, 0.10),
            ("bg_unimp", p.bg_unimp, 0x6b5fb0, 0.07),
            ("ln_del", p.ln_del, 0xf6e4e7, 1.0),
            ("ln_ins", p.ln_ins, 0xe2f1ea, 1.0),
            ("ln_mod", p.ln_mod, 0xf3ecd9, 1.0),
            ("ln_mov", p.ln_mov, 0xe2ebf6, 1.0),
            ("hl_del", p.hl_del, 0xc23d52, 0.28),
            ("hl_ins", p.hl_ins, 0x1e8a5c, 0.25),
            ("hl_unimp", p.hl_unimp, 0x6b5fb0, 0.22),
            ("find", p.find, 0xffc400, 0.55),
            ("sel", p.sel, 0x35639f, 0.22),
            ("tree_guide", p.tree_guide, 0x46556e, 0.18),
            ("sx.comment", p.sx.comment, 0x8a93a0, 1.0),
            ("sx.string", p.sx.string, 0x3f7d1e, 1.0),
            ("sx.number", p.sx.number, 0xb0522a, 1.0),
            ("sx.keyword", p.sx.keyword, 0x8a3fb0, 1.0),
            ("sx.type", p.sx.type_, 0x2f5fb3, 1.0),
            ("sx.tag", p.sx.tag, 0x1f6f9f, 1.0),
            ("sx.attr", p.sx.attr, 0x8a6416, 1.0),
            ("sx.decorator", p.sx.decorator, 0x237a80, 1.0),
        ] {
            assert_token(name, actual, hex, alpha);
        }
    }

    /// `top` painted over the opaque `base`.
    fn over(top: Hsla, base: Rgba) -> Rgba {
        let top: Rgba = top.into();
        let mix = |t: f32, b: f32| t * top.a + b * (1. - top.a);
        Rgba {
            r: mix(top.r, base.r),
            g: mix(top.g, base.g),
            b: mix(top.b, base.b),
            a: 1.,
        }
    }

    /// WCAG 2's contrast ratio of two opaque colours.
    fn contrast(a: Rgba, b: Rgba) -> f32 {
        let channel = |c: f32| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let luminance =
            |c: Rgba| 0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b);
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn find_hits_read_at_4_5_to_1_on_every_row() {
        for p in [Palette::dark(), Palette::light()] {
            // Under a hit: the list's panel, the current difference's wash, then a row tint.
            for wash in [None, Some(p.cur_row)] {
                let tints = [p.bg_del, p.bg_ins, p.bg_mod, p.bg_moved, p.bg_unimp];
                for tint in std::iter::once(None).chain(tints.map(Some)) {
                    let mut base: Rgba = p.panel.into();
                    for layer in [wash, tint].into_iter().flatten() {
                        base = over(layer, base);
                    }
                    let ratio = contrast(p.text.into(), over(p.find, base));
                    assert!(
                        ratio >= 4.5,
                        "dark {}: text on a find hit is {ratio:.2}:1 (wash {wash:?}, tint {tint:?})",
                        p.dark
                    );
                    // The hit's underline (the accent) marks it against the row (WCAG 1.4.11).
                    let mark = contrast(p.accent.into(), base);
                    assert!(
                        mark >= 3.,
                        "dark {}: a hit's underline is {mark:.2}:1 on its row",
                        p.dark
                    );
                }
            }
        }
    }

    #[test]
    fn the_current_wash_the_viewport_and_the_hatch_are_the_same_in_both_themes() {
        for p in [Palette::dark(), Palette::light()] {
            assert_token("cur_row", p.cur_row, 0xe0aa4a, 0.09);
            assert_token("viewport", p.viewport, 0xe0aa4a, 0.14);
            assert_token("hatch", p.hatch, 0x808ca0, 0.07);
        }
        assert_eq!(Palette::of(true), Palette::dark());
        assert_eq!(Palette::of(false), Palette::light());
    }
}
