//! Application start-up: window options, key bindings and the root view.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gpui_kit::component::scroll::ScrollbarMode;
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{
    App, AppContext, AssetSource, DummyKeyboardMapper, Global, KeyBinding, Keystroke, Pixels, Size,
    TitlebarOptions, WindowAppearance, WindowBounds, WindowOptions, px, size,
};

use crate::launch::Launch;
use crate::palette::Palette;
use crate::ui::fonts::Fonts;
use crate::ui::style::SCROLLBAR_WIDTH;
use crate::workspace::Workspace;

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// Where duet keeps its own files. `run` sets `%LOCALAPPDATA%\duet`; tests set a temp folder
/// or nothing, which turns the recent list off.
pub struct AppPaths {
    pub data_dir: PathBuf,
}

impl Global for AppPaths {}

pub fn recent_file(cx: &App) -> Option<PathBuf> {
    cx.try_global::<AppPaths>()
        .map(|paths| paths.data_dir.join("recent.txt"))
}

/// Call first thing in `main`; `startup_elapsed` measures from here.
pub fn mark_process_start() {
    let _ = PROCESS_START.set(Instant::now());
}

pub fn startup_elapsed() -> Duration {
    PROCESS_START.get().map_or(Duration::ZERO, Instant::elapsed)
}

/// The default window is 1400x900, shrunk to 90% of the display so it fits laptops at 125%
/// scale (1920x1080 at 125% is 1536x864 logical pixels).
pub fn window_size(display: Option<Size<Pixels>>) -> Size<Pixels> {
    let fit = |want: Pixels, have: Option<Pixels>| match have {
        Some(have) if have * 0.9 < want => have * 0.9,
        _ => want,
    };
    size(
        fit(px(1400.), display.map(|d| d.width)),
        fit(px(900.), display.map(|d| d.height)),
    )
}

/// `DUET_WINDOW_SIZE=1400x850`: an exact client size in logical pixels, for screenshots.
pub fn window_size_override(text: Option<&str>) -> Option<Size<Pixels>> {
    let (w, h) = text?.trim().split_once(['x', 'X'])?;
    let (w, h): (f32, f32) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (w >= 320. && h >= 240.).then(|| size(px(w), px(h)))
}

pub fn window_options(cx: &App) -> WindowOptions {
    let display = cx.primary_display().map(|d| d.bounds().size);
    let size = window_size_override(std::env::var("DUET_WINDOW_SIZE").ok().as_deref())
        .unwrap_or_else(|| window_size(display));
    WindowOptions {
        window_bounds: Some(WindowBounds::centered(size, cx)),
        titlebar: Some(TitlebarOptions {
            title: Some("duet".into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub fn key_bindings() -> Vec<KeyBinding> {
    let mut bindings = crate::workspace::key_bindings();
    bindings.extend(crate::folder::view::key_bindings());
    bindings.extend(crate::text::view::key_bindings());
    bindings.extend(crate::hex::view::key_bindings());
    bindings.extend(crate::table::view::key_bindings());
    bindings.extend(crate::picture::view::key_bindings());
    bindings.extend(crate::merge::view::key_bindings());
    bindings.extend(crate::help::key_bindings());
    bindings
}

/// The same bindings named by the active keyboard layout, for those it names differently.
/// gpui names a key by the character the layout gives it, so with the Thai layout Ctrl+3
/// arrives as "ctrl--" and a "ctrl-3" binding never fires. `map` turns a US-layout keystroke
/// into the layout's keystroke for the same physical key.
pub fn layout_bindings(
    bindings: &[KeyBinding],
    map: impl Fn(Keystroke) -> Keystroke,
) -> Vec<KeyBinding> {
    bindings
        .iter()
        .filter_map(|binding| {
            let keys: Vec<String> = binding
                .keystrokes()
                .iter()
                .map(|k| map(k.inner().clone()).unparse())
                .collect();
            let unchanged = binding
                .keystrokes()
                .iter()
                .zip(&keys)
                .all(|(k, mapped)| k.inner().unparse() == *mapped);
            if unchanged {
                return None;
            }
            KeyBinding::load(
                &keys.join(" "),
                binding.action().boxed_clone(),
                binding.predicate(),
                false,
                None,
                &DummyKeyboardMapper,
            )
            .ok()
        })
        .collect()
}

/// Binds the shortcuts, plus their names on the active keyboard layout, now and whenever the
/// layout changes.
fn bind_shortcuts(cx: &mut App) {
    let mapper = cx.keyboard_mapper().clone();
    let mapped = layout_bindings(&key_bindings(), |keystroke| {
        mapper.map_key_equivalent(keystroke, true).inner().clone()
    });
    cx.bind_keys(mapped);
}

/// One mode for everything: gpui-kit's theme takes the palette's colours, LocalDiff's 7 px
/// radius and duet's fonts, so inputs, menus and scrollbars match duet's own controls.
/// Scrollbars always show: in a long diff the thumb is where you are.
pub fn configure_theme(mode: ThemeMode, cx: &mut App) {
    crate::ui::fonts::install(cx);
    let fonts = Fonts::current(cx);
    Theme::change(mode, None, cx);
    let p = Palette::of(mode.is_dark());
    Theme::update(cx, |theme| {
        theme.font_family = fonts.ui.family.clone();
        theme.mono_font_family = fonts.code.family.clone();
        theme.scrollbar_mode = ScrollbarMode::Always;
        theme.radius = px(7.);
        theme.radius_lg = px(10.);
        theme.background = p.panel2;
        theme.foreground = p.text;
        theme.border = p.line;
        theme.input = p.line;
        theme.ring = p.accent;
        theme.caret = p.text;
        theme.selection = p.sel;
        theme.primary = p.accent;
        theme.primary_hover = p.accent;
        theme.primary_active = p.accent;
        theme.primary_foreground = p.accent_ink;
        theme.secondary = p.panel2;
        theme.secondary_hover = p.panel3;
        theme.secondary_active = p.panel3;
        theme.secondary_foreground = p.text;
        theme.button = p.panel2;
        theme.button_hover = p.panel3;
        theme.button_active = p.panel3;
        theme.button_foreground = p.text;
        theme.muted = p.panel2;
        theme.muted_foreground = p.muted;
        theme.accent = p.panel3;
        theme.accent_foreground = p.text;
        theme.popover = p.panel;
        theme.popover_foreground = p.text;
        theme.list_hover = p.panel3;
        theme.list_active = p.panel3;
        theme.list_active_border = p.accent;
        theme.scrollbar = gpui_kit::transparent_black();
        theme.scrollbar_thumb = p.line;
        theme.scrollbar_thumb_hover = p.dim;
    });
    thin_scrollbars(&p, cx);
}

/// LocalDiff's 11 px scrollbars: a line-coloured thumb inset from a transparent track.
/// gpui-base reads the track width from its own theme, which `Theme::update` rebuilds, so this
/// runs after it.
fn thin_scrollbars(p: &Palette, cx: &mut App) {
    use gpui_kit::base::ScrollbarThumbStyle;
    let thumb =
        |s: ScrollbarThumbStyle, color| s.bg(color).width(px(5.)).inset(px(3.)).radius(px(3.));
    let base = gpui_kit::base::Theme::global_mut(cx);
    let styles = base
        .scrollbar
        .styles()
        .clone()
        .track(|t| t.width(px(SCROLLBAR_WIDTH)))
        .thumb(|t| thumb(t, p.line))
        .thumb_hover(|t| thumb(t, p.dim))
        .thumb_active(|t| thumb(t, p.dim));
    base.scrollbar = base.scrollbar.clone().with_styles(styles);
    cx.refresh_windows();
}

// The icons gpui-kit's widgets draw for duet (a menu item's check mark, and the chevrons and
// clear button its inputs and menus can show), then the rest of `ui::icons`, duet's own, then
// the help topics' own (`help::topics::TOPICS`; FolderSync, CircleQuestionMark and the five
// compare kinds' icons are shared with `ui::icons`).
// Each SVG is compiled in with `include_bytes`, in every build profile, so nothing is read from
// disk or fetched; a name missing here draws as nothing (`only_the_icons_duet_draws_are_embedded`).
gpui_kit::assets::icon_assets!(
    DuetIcons,
    [
        Check,
        ChevronDown,
        ChevronRight,
        Close,
        ArrowDownToLine,
        ArrowLeft,
        ArrowLeftRight,
        ArrowRight,
        ArrowUp,
        ArrowUpToLine,
        ChevronUp,
        CircleQuestionMark,
        ClipboardPaste,
        Copy,
        FolderSync,
        Hash,
        Info,
        ListChevronsDownUp,
        ListChevronsUpDown,
        Moon,
        Plus,
        Redo2,
        RefreshCw,
        RotateCcw,
        Search,
        Sun,
        Undo2,
        X,
        ZoomIn,
        ZoomOut,
        ClipboardType,
        Clock,
        Files,
        Upload,
        Binary,
        CornerDownLeft,
        EyeOff,
        FileChartColumn,
        FileDiff,
        FolderTree,
        Funnel,
        GitMerge,
        Image,
        Keyboard,
        Rocket,
        Shapes,
        Sheet,
    ]
);

/// The only asset source duet registers: the icons above.
pub fn assets() -> impl AssetSource {
    DuetIcons
}

pub fn run(launch: Launch) {
    let application = gpui_kit::application().with_assets(assets());
    application.run(move |cx| {
        gpui_kit::init(cx);
        crate::text::syntax::register_extra_languages();
        let data_dir = crate::crash::data_dir();
        let saved = crate::theme::load(&data_dir.join(crate::theme::THEME_FILE));
        let system_dark = matches!(
            cx.window_appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        let env = std::env::var("DUET_THEME").ok();
        configure_theme(
            crate::theme::initial_mode(env.as_deref(), saved, system_dark),
            cx,
        );
        cx.set_global(AppPaths { data_dir });
        cx.bind_keys(key_bindings());
        bind_shortcuts(cx);
        cx.on_keyboard_layout_change(bind_shortcuts).detach();
        gpui_kit::open_window(window_options(cx), cx, |window, cx| {
            cx.new(|cx| Workspace::new(launch, window, cx))
        })
        .expect("open the main window");
        // gpui's open_window lays out and paints the first frame before it returns.
        tracing::info!(ms = startup_elapsed().as_millis() as u64, "first frame");
        if std::env::var_os("DUET_EXIT_AFTER_FIRST_FRAME").is_some() {
            cx.quit();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digit_shortcuts_are_bound_again_for_the_keyboard_layout() {
        // The Thai layout gives the "3" key the character "-", so gpui names Ctrl+3 "ctrl--".
        let thai = |mut keystroke: Keystroke| {
            if keystroke.key == "3" {
                keystroke.key = "-".into();
            }
            keystroke
        };
        let mapped = layout_bindings(&crate::text::view::key_bindings(), thai);
        let typed = [Keystroke::parse("ctrl--").unwrap()];
        let context = mapped
            .iter()
            .find(|b| b.match_keystrokes(&typed) == Some(false))
            .expect("Ctrl+3 on the Thai layout is bound");
        assert_eq!(context.action().name(), "text::FilterContext");
        assert!(
            mapped.iter().all(|b| b.keystrokes()[0].inner().key == "-"),
            "keys the layout leaves alone are not bound twice"
        );
    }

    #[test]
    fn ctrl_f_is_bound_again_for_the_thai_layout() {
        // Thai Kedmanee names the "f" key "ด".
        let thai = |mut keystroke: Keystroke| {
            if keystroke.key == "f" {
                keystroke.key = "ด".into();
            }
            keystroke
        };
        let mapped = layout_bindings(&crate::folder::view::key_bindings(), thai);
        let typed = [Keystroke::parse("ctrl-ด").unwrap()];
        let found = mapped
            .iter()
            .find(|b| b.match_keystrokes(&typed) == Some(false))
            .expect("Ctrl+F on the Thai layout is bound");
        assert_eq!(found.action().name(), "folder::FocusFilter");
    }

    #[test]
    fn the_hex_filter_digits_are_bound_again_for_the_keyboard_layout() {
        let thai = |mut keystroke: Keystroke| {
            if keystroke.key == "2" {
                keystroke.key = "-".into();
            }
            keystroke
        };
        let mapped = layout_bindings(&crate::hex::view::key_bindings(), thai);
        let typed = [Keystroke::parse("ctrl--").unwrap()];
        let bound = mapped
            .iter()
            .find(|b| b.match_keystrokes(&typed) == Some(false))
            .expect("Ctrl+2 on the Thai layout is bound");
        assert_eq!(bound.action().name(), "hex::HexOnlyDiff");
    }

    #[test]
    fn the_table_filter_digits_and_letters_are_bound_again_for_the_keyboard_layout() {
        // Thai Kedmanee names the "1", "2" and "3" keys "ๅ", "/" and "-", and the "n" and "p"
        // keys "ื" (sara uee) and "ย" (yo yak).
        let thai = |mut keystroke: Keystroke| {
            keystroke.key = match keystroke.key.as_str() {
                "1" => "ๅ",
                "2" => "/",
                "3" => "-",
                "n" => "ื",
                "p" => "ย",
                other => other,
            }
            .into();
            keystroke
        };
        let mapped = layout_bindings(&crate::table::view::key_bindings(), thai);
        let action = |typed: &str| {
            let typed = [Keystroke::parse(typed).unwrap()];
            mapped
                .iter()
                .find(|b| b.match_keystrokes(&typed) == Some(false))
                .map(|b| b.action().name())
        };
        assert_eq!(action("ctrl-ๅ"), Some("table::TableShowAll"));
        assert_eq!(action("ctrl-/"), Some("table::TableShowDiffs"));
        assert_eq!(action("ctrl--"), Some("table::TableShowSame"));
        assert_eq!(action("ื"), Some("table::TableNext"));
        assert_eq!(action("ย"), Some("table::TablePrev"));
    }

    #[test]
    fn the_picture_digits_and_zoom_keys_are_bound_again_for_the_keyboard_layout() {
        // Thai Kedmanee names the "1", "2", "3", "4" and "0" keys "ๅ", "/", "-", "ภ" and "จ".
        let thai = |mut keystroke: Keystroke| {
            keystroke.key = match keystroke.key.as_str() {
                "1" => "ๅ",
                "2" => "/",
                "3" => "-",
                "4" => "ภ",
                "0" => "จ",
                other => other,
            }
            .into();
            keystroke
        };
        let mapped = layout_bindings(&crate::picture::view::key_bindings(), thai);
        let action = |typed: &str| {
            let typed = [Keystroke::parse(typed).unwrap()];
            mapped
                .iter()
                .find(|b| b.match_keystrokes(&typed) == Some(false))
                .map(|b| b.action().name())
        };
        assert_eq!(action("ๅ"), Some("picture::PictureSideBySide"));
        assert_eq!(action("/"), Some("picture::PictureDifference"));
        assert_eq!(action("-"), Some("picture::PictureSlider"));
        assert_eq!(action("ภ"), Some("picture::PictureBlink"));
        assert_eq!(action("ctrl-จ"), Some("picture::PictureFit"));
        assert_eq!(action("ctrl-ๅ"), Some("picture::PictureActualSize"));
    }

    #[test]
    fn the_thai_layout_binds_the_text_view_letters_again() {
        // Thai Kedmanee names the "n", "p", "r" and "l" keys "ื", "ย", "พ" and "ส".
        let thai = |mut keystroke: Keystroke| {
            keystroke.key = match keystroke.key.as_str() {
                "n" => "ื",
                "p" => "ย",
                "r" => "พ",
                "l" => "ส",
                other => other,
            }
            .into();
            keystroke
        };
        let mapped = layout_bindings(&crate::text::view::key_bindings(), thai);
        let action = |typed: &str| {
            let typed = [Keystroke::parse(typed).unwrap()];
            mapped
                .iter()
                .find(|b| b.match_keystrokes(&typed) == Some(false))
                .map(|b| b.action().name())
        };
        assert_eq!(action("ื"), Some("text::NextChangeKey"));
        assert_eq!(action("ย"), Some("text::PrevChangeKey"));
        assert_eq!(action("ctrl-พ"), Some("text::CopyBlockRight"));
        assert_eq!(action("ctrl-ส"), Some("text::CopyBlockLeft"));
    }

    #[test]
    fn menus_find_their_check_mark_and_chevrons() {
        use gpui_kit::component::{IconName, IconNamed as _};
        let icons = [
            IconName::Check,
            IconName::ChevronDown,
            IconName::ChevronRight,
            IconName::Close,
        ];
        for icon in icons {
            let path = icon.path();
            assert!(
                assets().load(&path).ok().flatten().is_some(),
                "{path} is embedded"
            );
        }
    }

    #[test]
    fn every_icon_of_the_table_is_embedded() {
        for icon in crate::ui::icons::ALL {
            let path = icon.path();
            assert!(
                assets().load(&path).ok().flatten().is_some(),
                "{path} is embedded: one that is not draws as nothing"
            );
        }
    }

    #[test]
    fn only_the_icons_duet_draws_are_embedded() {
        use gpui_kit::SharedString;
        use gpui_kit::component::{IconName as WidgetIcon, IconNamed as _};
        use std::collections::BTreeSet;
        let widgets = [
            WidgetIcon::Check,
            WidgetIcon::ChevronDown,
            WidgetIcon::ChevronRight,
            WidgetIcon::Close,
        ]
        .map(|icon| icon.path());
        let help = crate::help::topics::TOPICS.map(|topic| topic.icon.path());
        let drawn: BTreeSet<SharedString> = crate::ui::icons::ALL
            .iter()
            .map(|icon| icon.path())
            .chain(widgets)
            .chain(help)
            .collect();
        let embedded: BTreeSet<SharedString> =
            assets().list("icons/").unwrap().into_iter().collect();
        assert_eq!(
            embedded, drawn,
            "the widgets' four, the icon table's and the help topics' (about 300 bytes each), \
             and nothing else"
        );
    }

    #[test]
    fn the_window_fits_a_laptop_screen_at_125_percent() {
        assert_eq!(window_size(None), size(px(1400.), px(900.)));
        assert_eq!(
            window_size(Some(size(px(2560.), px(1440.)))),
            size(px(1400.), px(900.))
        );
        // 1920x1080 at 125% scale is 1536x864 logical pixels.
        let laptop = window_size(Some(size(px(1536.), px(864.))));
        assert!((f32::from(laptop.width) - 1382.4).abs() < 0.01);
        assert!((f32::from(laptop.height) - 777.6).abs() < 0.01);
    }

    #[test]
    fn screenshots_can_ask_for_an_exact_window_size() {
        assert_eq!(
            window_size_override(Some("1400x850")),
            Some(size(px(1400.), px(850.)))
        );
        assert_eq!(
            window_size_override(Some(" 1200 X 800 ")),
            Some(size(px(1200.), px(800.)))
        );
        assert_eq!(window_size_override(Some("big")), None);
        assert_eq!(
            window_size_override(Some("10x10")),
            None,
            "too small to be meant"
        );
        assert_eq!(window_size_override(None), None);
    }
}
