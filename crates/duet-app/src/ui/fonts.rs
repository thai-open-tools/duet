//! The type scale (spec section 4) and the font families: Segoe UI for the UI, Cascadia Mono
//! (else Consolas) for code, each with Leelawadee UI as the Thai fallback.

use gpui_kit::component::Theme;
use gpui_kit::{App, Font, FontFallbacks, Global, Pixels, Window, font, px};

pub const UI_SIZE: f32 = 13.5;
pub const CODE_SIZE: f32 = 12.5;
pub const NUMBER_SIZE: f32 = 11.0;
/// Sub-toolbar and status bar.
pub const SMALL_SIZE: f32 = 12.0;
/// Pills and badges.
pub const BADGE_SIZE: f32 = 10.0;
pub const THAI_FALLBACK: &str = "Leelawadee UI";
pub const UI_FAMILIES: &[&str] = &["Segoe UI"];
pub const CODE_FAMILIES: &[&str] = &["Cascadia Mono", "Consolas"];

#[derive(Clone)]
pub struct Fonts {
    pub ui: Font,
    pub code: Font,
}

impl Global for Fonts {}

/// The first of `wanted` that is installed.
pub fn pick_family<'a>(wanted: &[&'a str], installed: &[String]) -> Option<&'a str> {
    wanted
        .iter()
        .copied()
        .find(|w| installed.iter().any(|name| name == w))
}

/// `family`, falling back to the families after it in `wanted`, then to Leelawadee UI.
pub fn with_fallbacks(family: &str, wanted: &[&str]) -> Font {
    let after = wanted.iter().skip_while(|w| **w != family).skip(1);
    let mut fallbacks: Vec<String> = after.map(|w| w.to_string()).collect();
    fallbacks.push(THAI_FALLBACK.to_owned());
    Font {
        fallbacks: Some(FontFallbacks::from_fonts(fallbacks)),
        ..font(family.to_owned())
    }
}

/// Picks the installed families once per process. GPUI fails on a family it cannot find, so
/// an uninstalled choice falls back to the theme's own resolved family.
pub fn install(cx: &mut App) {
    if cx.has_global::<Fonts>() {
        return;
    }
    let installed = cx.text_system().all_font_names();
    let theme = Theme::global(cx);
    let ui = pick_family(UI_FAMILIES, &installed)
        .map_or_else(|| theme.font_family.to_string(), str::to_owned);
    let code = pick_family(CODE_FAMILIES, &installed)
        .map_or_else(|| theme.mono_font_family.to_string(), str::to_owned);
    cx.set_global(Fonts {
        ui: with_fallbacks(&ui, UI_FAMILIES),
        code: with_fallbacks(&code, CODE_FAMILIES),
    });
}

impl Fonts {
    /// The installed fonts, or the theme's families (with the Thai fallback) before `install`.
    pub fn current(cx: &App) -> Fonts {
        cx.try_global::<Fonts>().cloned().unwrap_or_else(|| {
            let theme = Theme::global(cx);
            Fonts {
                ui: with_fallbacks(&theme.font_family, &[]),
                code: with_fallbacks(&theme.mono_font_family, &[]),
            }
        })
    }
}

/// One column of code text: what the column count and the tests measure with.
pub fn code_advance(window: &Window, code: &Font) -> Pixels {
    code_advance_at(window, code, CODE_SIZE)
}

/// One column of code text at `size` px, the text view's chosen font size (R4).
pub fn code_advance_at(window: &Window, code: &Font, size: f32) -> Pixels {
    let id = window.text_system().resolve_font(code);
    window
        .text_system()
        .ch_advance(id, px(size))
        .unwrap_or(px(size * 0.6))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn the_first_installed_family_wins() {
        assert_eq!(
            pick_family(CODE_FAMILIES, &names(&["Arial", "Consolas"])),
            Some("Consolas")
        );
        assert_eq!(
            pick_family(CODE_FAMILIES, &names(&["Consolas", "Cascadia Mono"])),
            Some("Cascadia Mono"),
            "the order of `wanted` decides, not the order installed"
        );
        assert_eq!(pick_family(UI_FAMILIES, &names(&["Arial"])), None);
    }

    #[test]
    fn fallbacks_end_with_leelawadee_for_thai() {
        let code = with_fallbacks("Cascadia Mono", CODE_FAMILIES);
        assert_eq!(code.family.as_ref(), "Cascadia Mono");
        assert_eq!(
            code.fallbacks.unwrap().fallback_list(),
            ["Consolas", "Leelawadee UI"]
        );
        let consolas = with_fallbacks("Consolas", CODE_FAMILIES);
        assert_eq!(
            consolas.fallbacks.unwrap().fallback_list(),
            ["Leelawadee UI"]
        );
        let ui = with_fallbacks("Segoe UI", UI_FAMILIES);
        assert_eq!(ui.fallbacks.unwrap().fallback_list(), ["Leelawadee UI"]);
    }
}
