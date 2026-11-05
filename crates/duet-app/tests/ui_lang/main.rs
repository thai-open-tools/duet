//! Headless UI tests for the language: the header control and what a switch reaches, then one
//! module per area, each opening its view in Thai.
//!
//! A test that wants Thai writes `let _th = thai_ui();`. A switch made through a control writes
//! this test's thread only, so tests running side by side never see it.

mod errors;
mod folder;
mod folder_ops;
mod hex;
mod merge;
mod picture;
mod report;
mod shell;
mod start;
mod sweep;
mod table;
mod text;
mod text_actions;

use duet_app::i18n::{Lang, LangScope, scoped};
use duet_app::launch::Launch;
use duet_app::workspace::Workspace;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, Point, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};

fn thai_ui() -> LangScope {
    scoped(Lang::Th)
}

/// A window on a workspace, 1400 px wide, so the header shows everything it can.
fn open(cx: &mut TestAppContext, launch: Launch) -> (AnyWindowHandle, Entity<Workspace>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.bind_keys(duet_app::app::key_bindings());
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(1400.), px(800.)),
                })),
                ..Default::default()
            },
            cx,
            move |window, cx| cx.new(|cx| Workspace::new(launch, window, cx)),
        )
        .expect("open test window")
    })
}

fn with_data_dir(cx: &mut TestAppContext, dir: &std::path::Path) {
    let data_dir = dir.to_path_buf();
    cx.update(|cx| cx.set_global(duet_app::app::AppPaths { data_dir }));
}
