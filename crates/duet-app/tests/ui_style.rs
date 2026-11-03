//! Headless tests for LocalDiff's control helpers (ui::style).

use std::cell::Cell;
use std::rc::Rc;

use duet_app::palette::Palette;
use duet_app::ui::fonts::Fonts;
use duet_app::ui::icons;
use duet_app::ui::style::{
    LdExt as _, Tone, ld_button, ld_chip, ld_folder_icon, ld_ghost, ld_gutter_button,
    ld_icon_button, ld_kbd, ld_primary, ld_segment, ld_segmented, ld_toggle,
};
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::h_flex;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Context, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement, Point, Render, Role, TestAppContext, Window, WindowBounds,
    WindowOptions, px, size,
};

struct Gallery {
    clicks: Rc<Cell<u32>>,
    enabled: bool,
    /// Like a view in the app, the gallery holds the focus that Tab moves on from.
    focus: FocusHandle,
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current(cx);
        let fonts = Fonts::current(cx);
        let (a, b) = (self.clicks.clone(), self.clicks.clone());
        h_flex()
            .track_focus(&self.focus)
            .child(
                ld_button("g-button", "Save left", &p)
                    .ld_tooltip("Save the left file")
                    .ld_click(true, move |_, _, _| a.set(a.get() + 1)),
            )
            .child(
                ld_icon_button("g-icon", icons::NEXT, "Next difference (F8)", &p)
                    .ld_click(self.enabled, move |_, _, _| b.set(b.get() + 10)),
            )
            .child(ld_primary("g-primary", "Compare", &p))
            .child(ld_ghost("g-ghost", "Show all", &p))
            .child(ld_segmented(
                "g-seg",
                [
                    ld_segment("g-all", "All", true, false, &p).ld_tooltip("All lines (Ctrl+1)"),
                    ld_segment("g-same", "Same", false, true, &p),
                ],
                &p,
            ))
            .child(ld_toggle("g-toggle", "Case", true, &p).ld_tooltip("Ignore case"))
            .child(ld_chip("g-chip", "Changed", 2, Tone::Changed, true, &p))
            .child(ld_gutter_button(
                "g-gutter",
                icons::TO_RIGHT,
                "Copy block to the right (Alt+→)",
                true,
                &p,
            ))
            .child(ld_kbd("Ctrl+F", &fonts.code, &p))
            .child(ld_folder_icon(&p).id("g-folder").test_support())
    }
}

/// Every control, in the order Tab reaches them while all are enabled.
const TAB_ORDER: [&str; 9] = [
    "g-button",
    "g-icon",
    "g-primary",
    "g-ghost",
    "g-all",
    "g-same",
    "g-toggle",
    "g-chip",
    "g-gutter",
];

fn open(cx: &mut TestAppContext, enabled: bool) -> (AnyWindowHandle, Rc<Cell<u32>>) {
    let clicks = Rc::new(Cell::new(0));
    let counter = clicks.clone();
    let (handle, _) = cx.update(|cx| {
        gpui_kit::init(cx);
        duet_app::app::configure_theme(gpui_kit::component::ThemeMode::Light, cx);
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1200.), px(200.)),
        };
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            move |window, cx| {
                let gallery = cx.new(|cx| Gallery {
                    clicks: counter,
                    enabled,
                    focus: cx.focus_handle(),
                });
                window.focus(&gallery.read(cx).focus.clone(), cx);
                gallery
            },
        )
        .expect("open test window")
    });
    (handle, clicks)
}

/// The controls that hold the keyboard focus.
fn focused(window: &Window) -> Vec<&'static str> {
    TAB_ORDER
        .into_iter()
        .filter(|id| window.find(*id).focused() == Some(true))
        .collect()
}

#[gpui_kit::test]
fn buttons_click_unless_disabled(cx: &mut TestAppContext) {
    let (handle, clicks) = open(cx, false);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("g-button", cx);
        window.click("g-icon", cx);
    })
    .unwrap();
    assert_eq!(clicks.get(), 1, "the disabled icon button did nothing");
}

#[gpui_kit::test]
fn controls_expose_labels_states_and_sizes(cx: &mut TestAppContext) {
    let (handle, clicks) = open(cx, true);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("g-icon", cx);
        assert_eq!(window.find("g-icon").label(), Some("Next difference (F8)"));
        assert_eq!(
            window.find("g-gutter").label(),
            Some("Copy block to the right (Alt+→)")
        );
        assert_eq!(window.find("g-all").selected(), Some(true));
        assert_eq!(window.find("g-same").selected(), Some(false));
        assert_eq!(window.find("g-toggle").checked(), Some(true));
        assert_eq!(window.find("g-chip").selected(), Some(true));
        assert_eq!(
            window.find("g-gutter").selected(),
            Some(true),
            "the current block's buttons"
        );
        let icon = window.find("g-icon").bounds().size;
        assert_eq!((icon.width, icon.height), (px(28.), px(28.)));
        let gutter = window.find("g-gutter").bounds().size;
        assert_eq!((gutter.width, gutter.height), (px(23.), px(18.)));
        let folder = window.find("g-folder").bounds().size;
        assert_eq!(
            (folder.width, folder.height),
            (px(13.), px(10.)),
            "the drawn folder icon"
        );
        assert_eq!(window.find("g-button").bounds().size.height, px(28.));
    })
    .unwrap();
    assert_eq!(clicks.get(), 10);
}

#[gpui_kit::test]
fn every_control_has_an_accessibility_role(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, true);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for id in ["g-button", "g-icon", "g-primary", "g-ghost", "g-gutter"] {
            assert_eq!(window.find(id).role(), Some(Role::Button), "{id}");
        }
        assert_eq!(window.find("g-seg").role(), Some(Role::TabList));
        for id in ["g-all", "g-same", "g-chip"] {
            assert_eq!(window.find(id).role(), Some(Role::Tab), "{id}");
        }
        assert_eq!(window.find("g-toggle").role(), Some(Role::CheckBox));
        // Tabs take no name from their text, and a check box's text would carry the ✓.
        assert_eq!(window.find("g-all").label(), Some("All"));
        assert_eq!(window.find("g-chip").label(), Some("Changed 2"));
        assert_eq!(window.find("g-toggle").label(), Some("Case"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_tooltip_leaves_a_visible_label_as_the_name(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, true);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("g-button").label(), Some("Save left"));
        assert_eq!(window.find("g-all").label(), Some("All"));
        assert_eq!(window.find("g-toggle").label(), Some("Case"));
        assert_eq!(
            window.find("g-icon").label(),
            Some("Next difference (F8)"),
            "an icon is named by its tooltip"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn tab_reaches_every_control_like_a_gpui_kit_button(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, true);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for id in TAB_ORDER {
            assert_eq!(
                window.find(id).focused(),
                Some(false),
                "{id} can take focus"
            );
        }
        assert_eq!(
            window.find("g-seg").focused(),
            None,
            "the segmented frame is no tab stop of its own"
        );
        for id in TAB_ORDER {
            window.press("tab", cx);
            assert_eq!(focused(window), [id]);
        }
        window.press("tab", cx);
        assert_eq!(focused(window), ["g-button"], "Tab wraps around");
    })
    .unwrap();
}

#[gpui_kit::test]
fn keyboard_focus_draws_the_ring_and_the_mouse_hides_it(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, true);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let width = window.find("g-button").bounds().size.width;
        window.press("tab", cx);
        assert_eq!(focused(window), ["g-button"]);
        assert_eq!(
            window.find("g-button").bounds().size.width,
            width + px(2.),
            "focus_visible turns the 1 px border into the 2 px accent ring"
        );
        window.click("g-primary", cx);
        assert_eq!(
            focused(window),
            ["g-button"],
            "a click leaves the focus where it was, as gpui-kit's Button does"
        );
        assert_eq!(
            window.find("g-button").bounds().size.width,
            width,
            "the ring shows for keyboard focus only"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn enter_and_space_press_the_focused_control(cx: &mut TestAppContext) {
    let (handle, clicks) = open(cx, true);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("tab", cx);
        window.press("enter", cx);
        window.press("space", cx);
    })
    .unwrap();
    assert_eq!(clicks.get(), 2);
}

#[gpui_kit::test]
fn a_disabled_control_is_no_tab_stop(cx: &mut TestAppContext) {
    let (handle, clicks) = open(cx, false);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("g-icon").focused(),
            None,
            "no focus while disabled"
        );
        window.press("tab", cx);
        window.press("tab", cx);
        assert_eq!(
            focused(window),
            ["g-primary"],
            "Tab skips the disabled button"
        );
        window.press("enter", cx);
    })
    .unwrap();
    assert_eq!(clicks.get(), 0);
}
