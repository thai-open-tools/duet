//! Headless UI tests for the picture view and its tab.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use duet_app::launch::Launch;
use duet_app::picture::prefs::Mode;
use duet_app::picture::view::{BLINK_INTERVAL, Layer, Painted, Pane, PictureEvent, PictureView};
use duet_app::picture::zoom::{center_on, fit_zoom};
use duet_app::workspace::{TabItem, Workspace};
use duet_core::picture::Picture;
use duet_core::picture::decode::Limits;
use duet_core::picture::export::encode_png;
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, Focusable, InputEvent as _, Modifiers,
    MouseMoveEvent, Pixels, Point, Render, ScrollDelta, ScrollWheelEvent, TestAppContext, Window,
    WindowBounds, WindowOptions, point, px, size,
};

fn open_test_window<V: Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        duet_app::app::configure_theme(gpui_kit::component::ThemeMode::Light, cx);
        cx.bind_keys(duet_app::app::key_bindings());
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1200.), px(800.)),
        };
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            build,
        )
        .expect("open test window")
    })
}

const GREY: [u8; 4] = [100, 100, 100, 255];
const BLUE_ISH: [u8; 4] = [100, 100, 200, 255];

fn png(width: u32, height: u32, marked: Option<(u32, u32)>) -> Vec<u8> {
    let mut picture = Picture::filled(width, height, GREY);
    if let Some((x, y)) = marked {
        picture.set_pixel(x, y, BLUE_ISH);
    }
    encode_png(&picture).unwrap()
}

/// a.png and b.png in a temporary folder.
fn write_pair(left: &[u8], right: &[u8]) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.png"), dir.path().join("b.png"));
    std::fs::write(&a, left).unwrap();
    std::fs::write(&b, right).unwrap();
    (dir, a, b)
}

/// 4x4 pictures: `b` differs from `a` at (1, 1).
fn one_difference() -> (tempfile::TempDir, PathBuf, PathBuf) {
    write_pair(&png(4, 4, None), &png(4, 4, Some((1, 1))))
}

fn open_view(
    cx: &mut TestAppContext,
    a: PathBuf,
    b: PathBuf,
    limits: Option<Limits>,
) -> (AnyWindowHandle, Entity<PictureView>) {
    open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| match limits {
            Some(limits) => PictureView::with_limits(a, b, limits, window, cx),
            None => PictureView::new(a, b, window, cx),
        });
        window.focus(&view.focus_handle(cx), cx);
        view
    })
}

async fn wait_picture(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<PictureView>,
) {
    let view = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        !view.read(cx).is_busy()
    })
    .await;
}

fn press(cx: &mut TestAppContext, handle: AnyWindowHandle, key: &str) {
    cx.update_window(handle, |_, window, cx| window.press(key, cx))
        .unwrap();
}

fn name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

fn record_events(cx: &mut TestAppContext, view: &Entity<PictureView>) -> Rc<RefCell<Vec<String>>> {
    let log = Rc::new(RefCell::new(Vec::new()));
    let seen = log.clone();
    cx.update(|cx| {
        cx.subscribe(view, move |_, event: &PictureEvent, _| {
            seen.borrow_mut().push(match event {
                PictureEvent::Back => "back".to_owned(),
                PictureEvent::Swapped { left, right } => {
                    format!("swapped {} {}", name(left), name(right))
                }
                PictureEvent::Open { .. } => "open".to_owned(),
            });
        })
        .detach();
    });
    log
}

async fn open_workspace(
    cx: &mut TestAppContext,
    a: PathBuf,
    b: PathBuf,
) -> (AnyWindowHandle, Entity<Workspace>, Entity<PictureView>) {
    let (handle, workspace) = open_test_window(cx, move |window, cx| {
        cx.new(|cx| Workspace::new(Launch::Picture { left: a, right: b }, window, cx))
    });
    let ws = workspace.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        ws.read(cx).active_idle(cx)
    })
    .await;
    let view = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Picture(view)) => view.clone(),
        _ => panic!("expected a picture tab"),
    });
    (handle, workspace, view)
}

#[gpui_kit::test]
async fn a_picture_tab_opens_decodes_and_compares(cx: &mut TestAppContext) {
    let (_dir, a, b) = one_difference();
    let (handle, workspace) = open_test_window(cx, {
        let (a, b) = (a.clone(), b.clone());
        move |window, cx| {
            cx.new(|cx| Workspace::new(Launch::Picture { left: a, right: b }, window, cx))
        }
    });
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        None,
        "no badge while the pictures load"
    );
    let ws = workspace.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        ws.read(cx).active_idle(cx)
    })
    .await;
    let view = workspace.read_with(cx, |w, _| match w.tabs().active() {
        Some(TabItem::Picture(view)) => view.clone(),
        _ => panic!("expected a picture tab"),
    });
    view.read_with(cx, |v, _| {
        let diff = v.diff().expect("a diff");
        assert_eq!(diff.count, 1);
        assert_eq!(diff.clusters.len(), 1);
        let left = v.side(true).expect("left side");
        assert_eq!(left.name, "a.png");
        assert_eq!((left.width, left.height), (4, 4));
        assert_eq!(left.format, "PNG");
        assert!(!left.big);
        assert!(left.size > 0 && left.modified.is_some());
        assert_eq!(
            v.compared().map(|c| (c.tiles.width, c.tiles.height)),
            Some((4, 4))
        );
    });
    assert_eq!(
        workspace.read_with(cx, |w, _| w
            .tabs()
            .iter()
            .next()
            .map(|(_, key, _)| key.title())),
        Some("a.png ↔ b.png".to_owned())
    );
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        Some("1".to_owned())
    );

    // An identical pair is "ok".
    let (_dir2, a2, b2) = write_pair(&png(4, 4, None), &png(4, 4, None));
    let (_handle, _workspace, same) = open_workspace(cx, a2, b2).await;
    same.read_with(cx, |v, _| {
        assert_eq!(v.diff().map(|d| d.count), Some(0));
        assert_eq!(v.badge().as_deref(), Some("ok"));
    });
}

#[gpui_kit::test]
async fn a_size_mismatch_badges_unequal_even_when_the_overlap_matches(cx: &mut TestAppContext) {
    let (_dir, a, b) = write_pair(&png(4, 4, None), &png(4, 5, None));
    let (_handle, workspace, view) = open_workspace(cx, a, b).await;
    view.read_with(cx, |v, _| {
        let diff = v.diff().expect("a diff");
        assert_eq!(diff.count, 0);
        assert!(diff.size_mismatch);
        assert_eq!(v.badge().as_deref(), Some("≠"));
    });
    assert_eq!(
        workspace.read_with(cx, |w, cx| w.tab_badge(0, cx)),
        Some("≠".to_owned())
    );
}

#[gpui_kit::test]
async fn a_broken_or_avif_side_says_why_and_the_other_side_stays(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.png");
    let b = dir.path().join("b.avif");
    std::fs::write(&a, png(4, 4, None)).unwrap();
    std::fs::write(&b, b"\0\0\0\x1cftypavif\0\0\0\0avifmif1").unwrap();
    let (handle, view) = open_view(cx, a.clone(), b, None);
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(
            v.message(false),
            Some("Can't open b.avif: AVIF is not supported.")
        );
        assert_eq!(v.message(true), None);
        assert!(v.side(true).is_some(), "the other side is loaded");
        assert!(v.side(false).is_none());
        assert!(v.diff().is_none());
        assert_eq!(v.badge().as_deref(), Some("!"));
    });

    // A damaged PNG names the Corrupt text.
    let broken = dir.path().join("b.png");
    std::fs::write(&broken, b"\x89PNG\r\n\x1a\nthis is not a png").unwrap();
    let (handle, view) = open_view(cx, a.clone(), broken.clone(), None);
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let message = v.message(false).expect("a message");
        assert!(message.starts_with("Can't open b.png: "), "{message}");
        assert!(message.contains("damaged"), "{message}");
    });

    // A file deleted under an open tab: F5 says so.
    std::fs::write(&broken, png(4, 4, None)).unwrap();
    let (handle, view) = open_view(cx, a, broken.clone(), None);
    wait_picture(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.message(false).is_none()));
    std::fs::remove_file(&broken).unwrap();
    press(cx, handle, "f5");
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let message = v.message(false).expect("a message");
        assert!(message.starts_with("Can't open b.png: "), "{message}");
        assert!(v.side(false).is_none());
        assert!(v.diff().is_none());
    });
}

#[gpui_kit::test]
async fn a_picture_over_the_pixel_limit_is_shown_but_not_compared(cx: &mut TestAppContext) {
    let (_dir, a, b) = one_difference();
    let limits = Limits {
        max_pixels: 10,
        ..Limits::default()
    };
    let (handle, view) = open_view(cx, a, b, Some(limits));
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert!(v.side(true).expect("left").big);
        assert!(v.too_big());
        assert!(v.diff().is_none());
        assert_eq!(v.badge(), None);
        assert!(!v.is_busy());
        assert_eq!(
            v.notice(),
            Some("No difference can be computed for these pictures.")
        );
    });
}

#[gpui_kit::test]
async fn a_pair_whose_union_is_too_large_is_shown_but_not_compared(cx: &mut TestAppContext) {
    // 40x1 against 1x40: each picture is within the limit, the union (40x40) is not.
    let (_dir, a, b) = write_pair(&png(40, 1, None), &png(1, 40, None));
    let limits = Limits {
        max_pixels: 40,
        ..Limits::default()
    };
    let (handle, view) = open_view(cx, a, b, Some(limits));
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert!(!v.side(true).expect("left").big, "both pictures are shown");
        assert!(!v.side(false).expect("right").big);
        assert!(v.too_big());
        assert!(v.diff().is_none());
        assert!(v.compared().is_none());
        assert_eq!(v.badge(), None);
        assert_eq!(
            v.notice(),
            Some("No difference can be computed for these pictures.")
        );
    });

    // Within the limits the same pair is compared.
    let (_dir, a, b) = write_pair(&png(40, 1, None), &png(1, 40, None));
    let (handle, view) = open_view(cx, a, b, None);
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert!(!v.too_big());
        assert!(v.diff().is_some());
    });
}

#[gpui_kit::test]
async fn f9_swaps_f5_rereads_and_esc_goes_back(cx: &mut TestAppContext) {
    let (_dir, a, b) = one_difference();
    let (handle, view) = open_view(cx, a.clone(), b.clone(), None);
    let events = record_events(cx, &view);
    wait_picture(cx, handle, &view).await;

    press(cx, handle, "f9");
    wait_picture(cx, handle, &view).await;
    assert_eq!(events.borrow().as_slice(), ["swapped b.png a.png"]);
    view.read_with(cx, |v, _| {
        assert_eq!(v.side(true).expect("left").name, "b.png");
        assert_eq!(v.diff().map(|d| d.count), Some(1));
        assert_eq!(v.paths(), (b.as_path(), a.as_path()));
    });

    // The left side now shows b.png; give a.png b's pixels and read again.
    std::fs::write(&a, png(4, 4, Some((1, 1)))).unwrap();
    press(cx, handle, "f5");
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.badge().as_deref(), Some("ok"));
        assert_eq!(v.notice(), Some("Pictures reloaded."));
    });

    press(cx, handle, "escape");
    assert_eq!(events.borrow().last().map(String::as_str), Some("back"));
}

#[gpui_kit::test]
async fn escape_cancels_a_running_load_first(cx: &mut TestAppContext) {
    let (_dir, a, b) = one_difference();
    let (handle, view) = open_view(cx, a, b, None);
    let events = record_events(cx, &view);
    // The test executor runs background work only when parked: the loads have not landed.
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    press(cx, handle, "escape");
    view.read_with(cx, |v, _| {
        assert!(!v.is_busy());
        assert_eq!(v.notice(), Some("Cancelled: the result is incomplete."));
    });
    assert!(events.borrow().is_empty(), "the first Esc only cancels");
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        assert!(v.diff().is_none(), "a cancelled load lands nothing");
        assert!(!v.is_busy());
    });
    press(cx, handle, "escape");
    assert_eq!(events.borrow().as_slice(), ["back"]);
}

#[gpui_kit::test]
async fn f9_during_a_load_moves_the_pending_side(cx: &mut TestAppContext) {
    let (_dir, a, b) = one_difference();
    let (handle, view) = open_view(cx, a, b, None);
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    press(cx, handle, "f9");
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.side(true).expect("left").name, "b.png");
        assert_eq!(v.side(false).expect("right").name, "a.png");
        assert_eq!(v.diff().map(|d| d.count), Some(1));
    });
}

#[gpui_kit::test]
async fn pictures_without_files_compare_and_have_nothing_to_reload(cx: &mut TestAppContext) {
    let left = Picture::filled(3, 3, GREY);
    let mut right = left.clone();
    right.set_pixel(0, 0, BLUE_ISH);
    let (handle, view) = open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| PictureView::with_pictures_for_test(left, right, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let left = v.side(true).expect("left");
        assert_eq!(
            (left.name.as_str(), left.format.as_str()),
            ("left.png", "RGBA")
        );
        assert_eq!(left.size, 3 * 3 * 4);
        assert_eq!(left.path, None);
        assert_eq!(v.diff().map(|d| d.count), Some(1));
    });
    press(cx, handle, "f5");
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        assert_eq!(v.notice(), Some("No pictures to reload."));
        assert!(v.diff().is_some(), "nothing was thrown away");
    });
}

#[gpui_kit::test]
async fn closing_a_picture_tab_releases_its_images(cx: &mut TestAppContext) {
    let (_dir, a, b) = one_difference();
    let (handle, workspace, view) = open_workspace(cx, a, b).await;
    assert_eq!(view.read_with(cx, |v, _| v.release_count()), 0);
    let held = view.read_with(cx, |v, _| v.image_ids_for_test());
    assert_eq!(held.len(), 3, "left, right and the difference image");
    press(cx, handle, "ctrl-w");
    assert_eq!(view.read_with(cx, |v, _| v.release_count()), 1);
    let mut released = view.read_with(cx, |v, _| v.released_ids_for_test());
    released.sort_unstable();
    let mut held = held;
    held.sort_unstable();
    assert_eq!(released, held, "every held image went to drop_image");
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().len()),
        1,
        "the Start tab replaces it"
    );
}

#[gpui_kit::test]
async fn swapping_into_an_open_pair_releases_the_closing_tab_images(cx: &mut TestAppContext) {
    let (_dir, a, b) = one_difference();
    let (handle, workspace, first) = open_workspace(cx, a.clone(), b.clone()).await;
    // The swapped pair b/a, open in its own tab.
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_picture(b.clone(), a.clone(), None, window, cx)
        })
    })
    .unwrap();
    assert_eq!(workspace.read_with(cx, |w, _| w.tabs().len()), 2);
    // F9 in the first tab lands on the pair that is already open: this tab closes.
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
        first.update(cx, |v, cx| v.swap_sides(cx));
        let _ = window;
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(workspace.read_with(cx, |w, _| w.tabs().len()), 1);
    assert_eq!(first.read_with(cx, |v, _| v.release_count()), 1);
    assert_eq!(
        first.read_with(cx, |v, _| v.released_ids_for_test().len()),
        3,
        "left, right and the difference image"
    );
}

#[gpui_kit::test]
async fn loaded_sides_and_the_difference_image_have_display_tiles(cx: &mut TestAppContext) {
    use duet_app::picture::view::Layer;
    // 4x4 against 6x5: the difference image is the larger.
    let (_dir, a, b) = write_pair(&png(4, 4, None), &png(6, 5, None));
    let (handle, view) = open_view(cx, a, b, None);
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.tiles_for_test(Layer::Left), Some((4, 4)));
        assert_eq!(v.tiles_for_test(Layer::Right), Some((6, 5)));
        assert_eq!(v.tiles_for_test(Layer::Difference), Some((6, 5)));
    });
    // A big picture keeps only its preview, and nothing is compared.
    let (_dir, a, b) = write_pair(&png(20, 10, None), &png(20, 10, None));
    let limits = Limits {
        max_pixels: 100,
        preview_side: 8,
        ..Limits::default()
    };
    let (handle, view) = open_view(cx, a, b, Some(limits));
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.tiles_for_test(Layer::Left), Some((8, 4)));
        assert_eq!(v.tiles_for_test(Layer::Difference), None);
    });
}

#[gpui_kit::test]
async fn replaced_textures_are_retired_and_dropped_on_the_next_frame(cx: &mut TestAppContext) {
    let (_dir, a, b) = one_difference();
    let (handle, view) = open_view(cx, a, b, None);
    wait_picture(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    assert_eq!(view.read_with(cx, |v, _| v.retired_for_test()), 0);
    assert_eq!(view.read_with(cx, |v, _| v.dropped_for_test()), 0);
    press(cx, handle, "f5");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |v, _| v.is_busy()), "the reload landed");
    // The landing replaced the old textures; gpui draws a notified window right after the
    // update, so the frame has already dropped them (a frame is the only thing that does).
    assert!(
        view.read_with(cx, |v, _| v.dropped_for_test()) >= 3,
        "the replaced sides and difference image were dropped"
    );
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    assert_eq!(view.read_with(cx, |v, _| v.retired_for_test()), 0);
}

#[gpui_kit::test]
async fn the_badge_counts_groups_not_pixels(cx: &mut TestAppContext) {
    // A 2x2 block of differing pixels: 4 pixels, one group.
    let mut right = Picture::filled(8, 8, GREY);
    for (x, y) in [(2, 2), (3, 2), (2, 3), (3, 3)] {
        right.set_pixel(x, y, BLUE_ISH);
    }
    let (_dir, a, b) = write_pair(&png(8, 8, None), &encode_png(&right).unwrap());
    let (handle, view) = open_view(cx, a, b, None);
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let diff = v.diff().expect("a diff");
        assert_eq!((diff.count, diff.clusters.len()), (4, 1));
        assert_eq!(v.badge().as_deref(), Some("1"));
    });
}

#[gpui_kit::test]
async fn f9_recomputes_from_the_swapped_sides(cx: &mut TestAppContext) {
    use duet_core::cancel::CancelToken;
    use duet_core::picture::diff::diff_image;
    // Different sizes make the two orientations differ.
    let (_dir, a, b) = write_pair(&png(4, 4, None), &png(2, 2, None));
    let (handle, view) = open_view(cx, a, b, None);
    wait_picture(cx, handle, &view).await;
    press(cx, handle, "f9");
    view.read_with(cx, |v, _| {
        assert!(v.compared().is_none(), "the old orientation is dropped");
        assert!(v.is_busy());
        assert_eq!(v.badge(), None);
    });
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let c = v.compared().expect("recomputed");
        assert_eq!((c.sides[0].width, c.sides[1].width), (2, 4));
        let expected = diff_image(&c.sides[0], &c.sides[1], 0, &CancelToken::new()).unwrap();
        // The tiles hold the difference image as BGRA: swap back and compare.
        let tile = &c.tiles.tiles[0];
        assert_eq!((tile.width, tile.height), (expected.width, expected.height));
        let mut shown = tile.image.as_bytes(0).unwrap().to_vec();
        shown.chunks_exact_mut(4).for_each(|p| p.swap(0, 2));
        assert!(
            shown == expected.rgba,
            "the image is made from the new left"
        );
        assert_eq!(v.side(true).expect("left").width, 2);
    });
}

// ---- The stage (Task 5) ----

/// Two decoded pictures with no file, named left.png and right.png.
fn open_pictures(
    cx: &mut TestAppContext,
    left: Picture,
    right: Picture,
) -> (AnyWindowHandle, Entity<PictureView>) {
    open_test_window(cx, move |window, cx| {
        let view = cx.new(|cx| PictureView::with_pictures_for_test(left, right, window, cx));
        window.focus(&view.focus_handle(cx), cx);
        view
    })
}

/// Two frames: the first measures the panes, the second lays the pictures out in them.
fn draw(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn bounds_of(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> Bounds<Pixels> {
    cx.update_window(handle, |_, window, _| window.find(id).bounds())
        .unwrap()
}

fn has(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> bool {
    cx.update_window(handle, |_, window, _| window.try_find(id).is_some())
        .unwrap()
}

fn label_of(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> Option<String> {
    cx.update_window(handle, |_, window, _| {
        window.find(id).label().map(str::to_owned)
    })
    .unwrap()
}

fn painted(cx: &mut TestAppContext, view: &Entity<PictureView>) -> Vec<Painted> {
    view.read_with(cx, |v, _| v.painted_for_test())
}

fn layers(cx: &mut TestAppContext, view: &Entity<PictureView>) -> Vec<(Pane, Layer)> {
    painted(cx, view)
        .iter()
        .map(|p| (p.pane, p.layer))
        .collect()
}

fn scroll_of(cx: &mut TestAppContext, view: &Entity<PictureView>) -> Point<f32> {
    view.read_with(cx, |v, _| v.scroll())
}

fn zoom_of(cx: &mut TestAppContext, view: &Entity<PictureView>) -> f32 {
    view.read_with(cx, |v, _| v.zoom())
}

fn pane_size(b: Bounds<Pixels>) -> gpui_kit::Size<f32> {
    size(f32::from(b.size.width), f32::from(b.size.height))
}

#[track_caller]
fn assert_near(got: f32, want: f32, what: &str) {
    assert!((got - want).abs() < 1e-3, "{what}: got {got}, want {want}");
}

#[track_caller]
fn assert_point_near(got: Point<f32>, want: Point<f32>, what: &str) {
    assert_near(got.x, want.x, what);
    assert_near(got.y, want.y, what);
}

/// A wheel turn at `at`, after the pointer has moved there.
fn wheel(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    at: Point<Pixels>,
    delta: ScrollDelta,
    modifiers: Modifiers,
) {
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(
            MouseMoveEvent {
                position: at,
                pressed_button: None,
                modifiers: Modifiers::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
        window.dispatch_event(
            ScrollWheelEvent {
                position: at,
                delta,
                modifiers,
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

fn drag(cx: &mut TestAppContext, handle: AnyWindowHandle, from: Point<Pixels>, to: Point<Pixels>) {
    cx.update_window(handle, |_, window, cx| window.drag(from, to, cx))
        .unwrap();
}

/// Drags both scrollbar thumbs of a pane back to their start: the scroll becomes (0, 0).
fn scroll_home(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    vscroll: &'static str,
    hscroll: &'static str,
) {
    let bar = bounds_of(cx, handle, vscroll);
    let x = bar.center().x;
    drag(
        cx,
        handle,
        point(x, bar.top() + px(1.)),
        point(x, bar.top() - px(40.)),
    );
    draw(cx, handle);
    let bar = bounds_of(cx, handle, hscroll);
    let y = bar.center().y;
    drag(
        cx,
        handle,
        point(bar.left() + px(1.), y),
        point(bar.left() - px(40.), y),
    );
    draw(cx, handle);
}

fn grey(width: u32, height: u32) -> Picture {
    Picture::filled(width, height, GREY)
}

#[gpui_kit::test]
async fn side_by_side_paints_each_picture_centred_in_its_half(cx: &mut TestAppContext) {
    let (handle, view) = open_pictures(cx, grey(100, 50), grey(100, 50));
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    let left = bounds_of(cx, handle, "picture-pane-left");
    let right = bounds_of(cx, handle, "picture-pane-right");
    assert!(left.right() < right.left(), "a gap between the panes");
    assert!(
        (left.size.width - right.size.width).abs() <= px(1.),
        "each pane takes half: {left:?} {right:?}"
    );
    let shown = painted(cx, &view);
    assert_eq!(
        layers(cx, &view),
        [(Pane::Left, Layer::Left), (Pane::Right, Layer::Right)]
    );
    for (entry, pane) in shown.iter().zip([left, right]) {
        assert_eq!(entry.rect.size, size(px(100.), px(50.)));
        let (got, want) = (entry.rect.center(), pane.center());
        assert_point_near(
            point(f32::from(got.x), f32::from(got.y)),
            point(f32::from(want.x), f32::from(want.y)),
            "centred in its pane",
        );
        assert_eq!(entry.clip, entry.rect, "the whole picture is drawn");
        assert!(!entry.enlarged);
    }
    view.read_with(cx, |v, _| {
        assert_eq!(v.zoom(), 1.);
        assert!(v.is_fit());
        assert_eq!(v.mode(), Mode::SideBySide);
    });
    assert!(
        !has(cx, handle, "picture-left-vscroll"),
        "nothing to scroll"
    );
    assert!(!has(cx, handle, "picture-pane-single"));
}

#[gpui_kit::test]
async fn fit_actual_size_and_the_zoom_keys(cx: &mut TestAppContext) {
    let (handle, view) = open_pictures(cx, grey(3000, 2000), grey(3000, 2000));
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    let left = bounds_of(cx, handle, "picture-pane-left");
    let fit = fit_zoom(pane_size(left), (3000, 2000)).unwrap();
    assert!(fit < 1.);
    assert_eq!(zoom_of(cx, &view), fit);
    assert!(view.read_with(cx, |v, _| v.is_fit()));
    assert!(
        !has(cx, handle, "picture-left-hscroll"),
        "a fitted picture does not scroll"
    );

    press(cx, handle, "ctrl-1");
    assert_eq!(zoom_of(cx, &view), 1.);
    assert!(!view.read_with(cx, |v, _| v.is_fit()));
    press(cx, handle, "ctrl-=");
    assert_eq!(zoom_of(cx, &view), 1.5);
    press(cx, handle, "ctrl--");
    press(cx, handle, "ctrl--");
    assert_eq!(zoom_of(cx, &view), 0.75);
    // Ctrl+Shift+= and Ctrl+numpad-plus both arrive as "ctrl-+".
    press(cx, handle, "ctrl-+");
    assert_eq!(zoom_of(cx, &view), 1.);
    draw(cx, handle);
    assert!(
        has(cx, handle, "picture-left-hscroll"),
        "100% overflows the pane"
    );
    assert!(has(cx, handle, "picture-right-vscroll"));

    press(cx, handle, "ctrl-0");
    draw(cx, handle);
    let left = bounds_of(cx, handle, "picture-pane-left");
    assert_eq!(
        zoom_of(cx, &view),
        fit_zoom(pane_size(left), (3000, 2000)).unwrap()
    );
    assert!(view.read_with(cx, |v, _| v.is_fit()));
}

#[gpui_kit::test]
async fn ctrl_wheel_zooms_about_the_pointer_and_both_panes_pan_together(cx: &mut TestAppContext) {
    // PF6: larger than the panes, at 100%.
    let (handle, view) = open_pictures(cx, grey(2000, 1500), grey(2000, 1500));
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    press(cx, handle, "ctrl-1");
    draw(cx, handle);
    scroll_home(cx, handle, "picture-left-vscroll", "picture-left-hscroll");
    assert_eq!(scroll_of(cx, &view), point(0., 0.), "the thumbs went home");

    let left = bounds_of(cx, handle, "picture-pane-left");
    let rect = painted(cx, &view)[0].rect;
    let pointer = rect.origin + point(px(200.5), px(150.5));
    assert!(left.contains(&pointer));
    wheel(
        cx,
        handle,
        pointer,
        ScrollDelta::Lines(point(0., 3.)),
        Modifiers::control(),
    );
    draw(cx, handle);
    let zoom = zoom_of(cx, &view);
    assert!((zoom - 1.15997).abs() < 1e-4, "zoom {zoom}");
    let rect = painted(cx, &view)[0].rect;
    let under = (
        (f32::from(pointer.x - rect.left()) / zoom).floor(),
        (f32::from(pointer.y - rect.top()) / zoom).floor(),
    );
    assert_eq!(
        under,
        (200., 150.),
        "the same pixel stays under the pointer"
    );

    let before = scroll_of(cx, &view);
    wheel(
        cx,
        handle,
        pointer,
        ScrollDelta::Lines(point(0., -3.)),
        Modifiers::default(),
    );
    assert_point_near(
        scroll_of(cx, &view),
        point(before.x, before.y + 99.),
        "a plain wheel scrolls down",
    );
    assert_eq!(zoom_of(cx, &view), zoom);

    let before = scroll_of(cx, &view);
    drag(
        cx,
        handle,
        point(px(300.), px(300.)),
        point(px(250.), px(280.)),
    );
    draw(cx, handle);
    assert_point_near(
        scroll_of(cx, &view),
        point(before.x + 50., before.y + 20.),
        "a drag pans",
    );
    let shown = painted(cx, &view);
    let right = bounds_of(cx, handle, "picture-pane-right");
    let left = bounds_of(cx, handle, "picture-pane-left");
    let offset = |rect: Bounds<Pixels>, pane: Bounds<Pixels>| {
        let at = rect.origin - pane.origin;
        point(f32::from(at.x), f32::from(at.y))
    };
    assert_point_near(
        offset(shown[1].rect, right),
        offset(shown[0].rect, left),
        "both panes pan together",
    );
}

#[gpui_kit::test]
async fn the_difference_mode_paints_the_difference_image(cx: &mut TestAppContext) {
    let mut right = grey(2000, 900);
    right.set_pixel(10, 10, BLUE_ISH);
    let (handle, view) = open_pictures(cx, grey(1800, 1000), right);
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    let side_fit = zoom_of(cx, &view);
    press(cx, handle, "2");
    draw(cx, handle);
    assert_eq!(view.read_with(cx, |v, _| v.mode()), Mode::Difference);
    let single = bounds_of(cx, handle, "picture-pane-single");
    let zoom = zoom_of(cx, &view);
    assert_eq!(zoom, fit_zoom(pane_size(single), (2000, 1000)).unwrap());
    assert!(
        zoom > side_fit,
        "the single pane is wider: {zoom} > {side_fit}"
    );
    let shown = painted(cx, &view);
    assert_eq!(layers(cx, &view), [(Pane::Single, Layer::Difference)]);
    assert_near(f32::from(shown[0].rect.size.width), 2000. * zoom, "width");
    assert_near(f32::from(shown[0].rect.size.height), 1000. * zoom, "height");
    assert_eq!(
        view.read_with(cx, |v, _| v.message_overlay_for_test()),
        None
    );
    assert!(!has(cx, handle, "picture-pane-left"));

    // Without the right picture the left one is shown, with a word about it.
    let (dir, a, _b) = one_difference();
    let missing = dir.path().join("gone.png");
    let (handle, view) = open_view(cx, a.clone(), missing, None);
    // The loads land only when the executor parks: side by side says they load.
    draw(cx, handle);
    assert_eq!(
        label_of(cx, handle, "picture-left-placeholder").as_deref(),
        Some("Loading a.png…")
    );
    wait_picture(cx, handle, &view).await;
    assert!(
        !has(cx, handle, "picture-left-placeholder"),
        "a.png is shown"
    );
    let why = label_of(cx, handle, "picture-right-placeholder").expect("a reason");
    assert!(why.starts_with("Can't open gone.png"), "{why}");
    press(cx, handle, "2");
    draw(cx, handle);
    assert_eq!(layers(cx, &view), [(Pane::Single, Layer::Left)]);
    let open_other = "Open the other picture to start comparing.";
    assert_eq!(
        view.read_with(cx, |v, _| v.message_overlay_for_test()),
        Some(open_other.to_owned())
    );
    assert_eq!(
        label_of(cx, handle, "picture-message"),
        Some(open_other.to_owned())
    );

    // A pair over the pixel limit has no difference image.
    let (_dir, a, b) = one_difference();
    let limits = Limits {
        max_pixels: 10,
        ..Limits::default()
    };
    let (handle, view) = open_view(cx, a, b, Some(limits));
    wait_picture(cx, handle, &view).await;
    press(cx, handle, "2");
    draw(cx, handle);
    assert_eq!(layers(cx, &view), [(Pane::Single, Layer::Left)]);
    assert_eq!(
        view.read_with(cx, |v, _| v.message_overlay_for_test()),
        Some("A picture is larger than 40 megapixels: no difference image.".to_owned())
    );
}

#[gpui_kit::test]
async fn the_slider_divides_left_and_right_where_it_is_dragged(cx: &mut TestAppContext) {
    let (handle, view) = open_pictures(cx, grey(2000, 1500), grey(2000, 1500));
    wait_picture(cx, handle, &view).await;
    press(cx, handle, "3");
    press(cx, handle, "ctrl-1");
    draw(cx, handle);
    assert_eq!(view.read_with(cx, |v, _| v.split()), 0.5);
    let single = bounds_of(cx, handle, "picture-pane-single");
    // Scrolled down a little, so a pan would show.
    wheel(
        cx,
        handle,
        single.center(),
        ScrollDelta::Lines(point(0., -3.)),
        Modifiers::default(),
    );
    draw(cx, handle);
    assert_eq!(
        layers(cx, &view),
        [(Pane::Single, Layer::Left), (Pane::Single, Layer::Right)]
    );
    let split_x = |cx: &mut TestAppContext, fraction: f32| {
        let rect = painted(cx, &view)[0].rect;
        f32::from(rect.left()) + fraction * f32::from(rect.size.width)
    };
    let shown = painted(cx, &view);
    let half = split_x(cx, 0.5);
    assert_near(
        f32::from(shown[0].clip.right()),
        half,
        "the left ends at the divider",
    );
    assert_near(
        f32::from(shown[1].clip.left()),
        half,
        "the right starts there",
    );
    assert!(has(cx, handle, "picture-split"));

    let before = scroll_of(cx, &view);
    let quarter = split_x(cx, 0.25);
    drag(
        cx,
        handle,
        single.center(),
        point(px(quarter), single.center().y + px(30.)),
    );
    draw(cx, handle);
    let split = view.read_with(cx, |v, _| v.split());
    assert!((split - 0.25).abs() < 0.001, "split {split}");
    assert_eq!(scroll_of(cx, &view), before, "the drag does not pan");
    let shown = painted(cx, &view);
    assert_near(
        f32::from(shown[0].clip.right()),
        quarter,
        "the left clip moved",
    );
    assert_near(
        f32::from(shown[1].clip.left()),
        quarter,
        "the right clip moved",
    );
}

#[gpui_kit::test]
async fn blink_alternates_every_600_ms_while_drawn(cx: &mut TestAppContext) {
    let mut right = grey(4, 4);
    right.set_pixel(1, 1, BLUE_ISH);
    let (handle, view) = open_pictures(cx, grey(4, 4), right);
    wait_picture(cx, handle, &view).await;
    press(cx, handle, "4");
    draw(cx, handle);
    let badge = |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.blink_badge_for_test());
    assert_eq!(layers(cx, &view), [(Pane::Single, Layer::Left)]);
    assert_eq!(badge(cx).as_deref(), Some("Left: left.png"));
    assert_eq!(
        label_of(cx, handle, "picture-blink-badge").as_deref(),
        Some("Left: left.png")
    );
    assert!(view.read_with(cx, |v, _| v.is_blinking() && v.blink_shows_left()));
    cx.executor().advance_clock(BLINK_INTERVAL);
    cx.run_until_parked();
    draw(cx, handle);
    assert_eq!(layers(cx, &view), [(Pane::Single, Layer::Right)]);
    assert_eq!(badge(cx).as_deref(), Some("Right: right.png"));
    cx.executor().advance_clock(BLINK_INTERVAL);
    cx.run_until_parked();
    draw(cx, handle);
    assert_eq!(layers(cx, &view), [(Pane::Single, Layer::Left)]);

    // A tab that is not drawn stops blinking; drawing it again starts it.
    let (_dir, a, b) = one_difference();
    let (handle, workspace, view) = open_workspace(cx, a, b).await;
    view.update(cx, |v, cx| v.set_mode(Mode::Blink, cx));
    draw(cx, handle);
    assert!(view.read_with(cx, |v, _| v.is_blinking()));
    let (_dir2, c, d) = write_pair(&png(4, 4, None), &png(4, 4, None));
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| w.open_picture(c, d, None, window, cx))
    })
    .unwrap();
    draw(cx, handle);
    cx.executor().advance_clock(Duration::from_millis(1800));
    cx.run_until_parked();
    assert!(
        !view.read_with(cx, |v, _| v.is_blinking()),
        "an undrawn tab stops blinking"
    );
    workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
    draw(cx, handle);
    assert!(
        view.read_with(cx, |v, _| v.is_blinking()),
        "drawn again, it blinks"
    );
}

#[gpui_kit::test]
async fn next_and_previous_groups_wrap_and_come_into_view(cx: &mut TestAppContext) {
    let black = Picture::filled(2000, 2000, [0, 0, 0, 255]);
    let mut red = black.clone();
    red.set_pixel(100, 100, [255, 0, 0, 255]);
    red.set_pixel(1900, 1900, [255, 0, 0, 255]);
    let (handle, view) = open_pictures(cx, black, red);
    let events = record_events(cx, &view);
    wait_picture(cx, handle, &view).await;
    press(cx, handle, "ctrl-1");
    draw(cx, handle);
    let clusters = view.read_with(cx, |v, _| v.diff().unwrap().clusters.clone());
    assert_eq!(clusters.len(), 2);
    let left = bounds_of(cx, handle, "picture-pane-left");
    let centred = |i: usize| {
        let c = clusters[i];
        center_on(
            (c.x, c.y, c.width, c.height),
            pane_size(left),
            (2000, 2000),
            1.,
        )
    };
    let group = |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.current_group());
    assert_eq!(group(cx), None);
    press(cx, handle, "f8");
    assert_eq!(group(cx), Some(0));
    assert_point_near(scroll_of(cx, &view), centred(0), "group 0 in view");
    press(cx, handle, "f8");
    assert_eq!(group(cx), Some(1));
    assert_point_near(scroll_of(cx, &view), centred(1), "group 1 in view");
    press(cx, handle, "f8");
    assert_eq!(group(cx), Some(0), "next wraps to the first");
    press(cx, handle, "f7");
    assert_eq!(group(cx), Some(1), "previous wraps to the last");
    draw(cx, handle);

    let shown = painted(cx, &view);
    let markers = view.read_with(cx, |v, _| v.marker_for_test());
    assert_eq!(
        markers.iter().map(|(pane, _)| *pane).collect::<Vec<_>>(),
        [Pane::Left, Pane::Right]
    );
    let c = clusters[1];
    for ((_, marker), entry) in markers.iter().zip(&shown) {
        let want = Bounds::new(
            entry.rect.origin + point(px(c.x as f32 - 3.), px(c.y as f32 - 3.)),
            size(px(c.width as f32 + 6.), px(c.height as f32 + 6.)),
        );
        assert_eq!(*marker, want, "the R10 box around the group");
    }

    press(cx, handle, "escape");
    assert_eq!(group(cx), None, "Escape clears the group first");
    assert!(events.borrow().is_empty());
    press(cx, handle, "escape");
    assert_eq!(events.borrow().as_slice(), ["back"]);

    let notice_after_f8 = |cx: &mut TestAppContext, handle, view: &Entity<PictureView>| {
        press(cx, handle, "f8");
        view.read_with(cx, |v, _| v.notice().map(str::to_owned))
    };
    let (handle, same) = open_pictures(cx, grey(4, 4), grey(4, 4));
    wait_picture(cx, handle, &same).await;
    assert_eq!(
        notice_after_f8(cx, handle, &same).as_deref(),
        Some("No differences above the tolerance.")
    );
    let (dir, a, _b) = one_difference();
    let (handle, half) = open_view(cx, a, dir.path().join("gone.png"), None);
    wait_picture(cx, handle, &half).await;
    assert_eq!(
        notice_after_f8(cx, handle, &half).as_deref(),
        Some("Open both pictures first.")
    );
}

#[gpui_kit::test]
async fn zoomed_in_pixels_are_drawn_sharp(cx: &mut TestAppContext) {
    let mut right = grey(4, 4);
    right.set_pixel(1, 1, BLUE_ISH);
    let (handle, view) = open_pictures(cx, grey(4, 4), right);
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    assert!(painted(cx, &view).iter().all(|p| !p.enlarged));
    view.update(cx, |v, cx| v.set_zoom(8., None, cx));
    draw(cx, handle);
    let shown = painted(cx, &view);
    assert_eq!(shown.len(), 2);
    assert!(shown.iter().all(|p| p.enlarged), "{shown:?}");
    assert_eq!(shown[0].rect.size, size(px(32.), px(32.)));
    let built = view.read_with(cx, |v, _| v.enlargements_built_for_test());
    assert_eq!(built, 2, "one per pane");
    draw(cx, handle);
    assert_eq!(
        view.read_with(cx, |v, _| v.enlargements_built_for_test()),
        built,
        "an unchanged frame builds nothing"
    );
    view.update(cx, |v, cx| v.set_zoom(1., None, cx));
    draw(cx, handle);
    assert!(painted(cx, &view).iter().all(|p| !p.enlarged));
}

#[gpui_kit::test]
async fn a_mode_switch_keeps_the_scroll_and_refits_while_fit(cx: &mut TestAppContext) {
    let (handle, view) = open_pictures(cx, grey(3000, 2000), grey(3000, 2000));
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    let side_fit = zoom_of(cx, &view);
    press(cx, handle, "2");
    draw(cx, handle);
    let single = bounds_of(cx, handle, "picture-pane-single");
    let single_fit = fit_zoom(pane_size(single), (3000, 2000)).unwrap();
    assert!(single_fit > side_fit);
    assert_eq!(
        zoom_of(cx, &view),
        single_fit,
        "refitted for the single pane"
    );
    press(cx, handle, "1");
    draw(cx, handle);
    assert_eq!(zoom_of(cx, &view), side_fit, "and back");

    press(cx, handle, "ctrl-1");
    draw(cx, handle);
    scroll_home(cx, handle, "picture-left-vscroll", "picture-left-hscroll");
    drag(
        cx,
        handle,
        point(px(400.), px(400.)),
        point(px(280.), px(320.)),
    );
    draw(cx, handle);
    assert_point_near(scroll_of(cx, &view), point(120., 80.), "panned");
    press(cx, handle, "2");
    draw(cx, handle);
    assert_point_near(scroll_of(cx, &view), point(120., 80.), "the scroll is kept");
    assert_eq!(zoom_of(cx, &view), 1.);
    assert!(!view.read_with(cx, |v, _| v.is_fit()));
}

// ---- Toolbar, heads, tolerance, status, legend and probe (Task 6) ----

use duet_app::format::human_size;
use duet_app::picture::view::HeadText;
use duet_app::status::Tone;
use gpui_kit::component::input::InputState;
use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};

fn use_settings(cx: &mut TestAppContext) -> tempfile::TempDir {
    let data = tempfile::tempdir().unwrap();
    let data_dir = data.path().to_path_buf();
    cx.update(|cx| cx.set_global(duet_app::app::AppPaths { data_dir }));
    data
}

fn saved_setting(data: &tempfile::TempDir, key: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(data.path().join("settings.json")).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    saved[key].clone()
}

fn click(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
}

/// 4x4 pictures: the left is grey, the right has `marks` set.
fn marked(marks: &[((u32, u32), [u8; 4])]) -> (Picture, Picture) {
    let mut right = grey(4, 4);
    for &((x, y), pixel) in marks {
        right.set_pixel(x, y, pixel);
    }
    (grey(4, 4), right)
}

/// Gives the tolerance box the keys, replaces its text with `text` and presses `key`, if any.
fn type_tolerance(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<PictureView>,
    text: &str,
    key: Option<&str>,
) {
    let input: Entity<InputState> = view.read_with(cx, |v, _| v.tolerance_input_for_test());
    cx.update_window(handle, |_, window, cx| {
        window.focus(&input.focus_handle(cx), cx);
        window.press("ctrl-a", cx);
        window.input(text, cx);
        if let Some(key) = key {
            window.press(key, cx);
        }
    })
    .unwrap();
}

fn box_text(cx: &mut TestAppContext, view: &Entity<PictureView>) -> String {
    let input: Entity<InputState> = view.read_with(cx, |v, _| v.tolerance_input_for_test());
    input.read_with(cx, |state, _| state.value().to_string())
}

fn counts(cx: &mut TestAppContext, view: &Entity<PictureView>) -> (u64, u64) {
    view.read_with(cx, |v, _| {
        let diff = v.diff().expect("a diff");
        (diff.count, diff.minor)
    })
}

fn status_texts(cx: &mut TestAppContext, view: &Entity<PictureView>) -> Vec<String> {
    view.read_with(cx, |v, _| {
        v.status_parts().into_iter().map(|p| p.text).collect()
    })
}

#[gpui_kit::test]
async fn the_mode_buttons_and_digits_switch_modes_and_are_saved(cx: &mut TestAppContext) {
    let data = use_settings(cx);
    let (left, right) = marked(&[]);
    let (handle, view) = open_pictures(cx, left, right);
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    for id in [
        "picture-toolbar",
        "picture-mode",
        "picture-mode-side",
        "picture-mode-diff",
        "picture-mode-slider",
        "picture-mode-blink",
        "picture-status",
    ] {
        assert!(has(cx, handle, id), "{id} is drawn");
    }
    assert_eq!(view.read_with(cx, |v, _| v.mode()), Mode::SideBySide);

    click(cx, handle, "picture-mode-diff");
    assert_eq!(view.read_with(cx, |v, _| v.mode()), Mode::Difference);
    assert_eq!(saved_setting(&data, "picture.mode"), "diff");

    press(cx, handle, "1");
    assert_eq!(view.read_with(cx, |v, _| v.mode()), Mode::SideBySide);
    assert_eq!(saved_setting(&data, "picture.mode"), "side");

    click(cx, handle, "picture-mode-slider");
    assert_eq!(saved_setting(&data, "picture.mode"), "slider");
    let (left, right) = marked(&[]);
    let (_handle, second) = open_pictures(cx, left, right);
    assert_eq!(
        second.read_with(cx, |v, _| v.mode()),
        Mode::Slider,
        "a new view opens in the saved mode"
    );
}

#[gpui_kit::test]
async fn tolerance_box_and_slider_recompute_and_are_saved(cx: &mut TestAppContext) {
    let data = use_settings(cx);
    let (left, right) = marked(&[((1, 1), [100, 100, 105, 255])]);
    let (handle, view) = open_pictures(cx, left, right);
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    for id in ["picture-tolerance", "picture-tolerance-input"] {
        assert!(has(cx, handle, id), "{id} is drawn");
    }
    assert_eq!(counts(cx, &view), (1, 0));

    type_tolerance(cx, handle, &view, "5", Some("enter"));
    wait_picture(cx, handle, &view).await;
    assert_eq!(counts(cx, &view), (0, 1));
    assert_eq!(saved_setting(&data, "picture.tolerance"), 5);
    assert_eq!(view.read_with(cx, |v, _| v.tolerance()), 5);

    type_tolerance(cx, handle, &view, "300", Some("enter"));
    wait_picture(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.tolerance()), 255);
    assert_eq!(box_text(cx, &view), "255");
    assert_eq!(saved_setting(&data, "picture.tolerance"), 255);

    // The number box refuses letters; a lone dot is as far from a number as it gets.
    type_tolerance(cx, handle, &view, ".", Some("enter"));
    wait_picture(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.tolerance()), 0);
    assert_eq!(box_text(cx, &view), "0");
    assert_eq!(counts(cx, &view), (1, 0));

    let slider: Entity<SliderState> = view.read_with(cx, |v, _| v.slider_for_test());
    slider.update(cx, |_, cx| {
        cx.emit(SliderEvent::Release(SliderValue::Single(7.)))
    });
    wait_picture(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.tolerance()), 7);
    assert_eq!(box_text(cx, &view), "7");
    assert_eq!(saved_setting(&data, "picture.tolerance"), 7);
    assert_eq!(counts(cx, &view), (0, 1));
    assert_eq!(
        slider.read_with(cx, |s, _| s.value()),
        SliderValue::Single(7.)
    );

    // The step buttons add one at once.
    let input: Entity<InputState> = view.read_with(cx, |v, _| v.tolerance_input_for_test());
    input.update(cx, |_, cx| {
        cx.emit(gpui_kit::component::input::NumberInputEvent::Step(
            gpui_kit::component::input::StepAction::Increment,
        ))
    });
    wait_picture(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.tolerance()), 8);
    assert_eq!(box_text(cx, &view), "8");
}

#[gpui_kit::test]
async fn a_tolerance_change_mid_diff_drops_the_stale_result(cx: &mut TestAppContext) {
    let mut right = grey(1500, 1500);
    right.set_pixel(10, 10, [100, 100, 105, 255]);
    let (handle, view) = open_pictures(cx, grey(1500, 1500), right);
    // The first diff (tolerance 0) has not landed.
    view.update(cx, |v, cx| v.set_tolerance(5, false, cx));
    wait_picture(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.diff_tolerance_for_test()),
        Some(5)
    );
    assert_eq!(counts(cx, &view), (0, 1));

    // A lazy change on a large overlap waits 90 ms.
    view.update(cx, |v, cx| v.set_tolerance(6, true, cx));
    assert_eq!(
        view.read_with(cx, |v, _| v.tolerance_text()),
        "Tolerance 6 · computing…"
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.diff_tolerance_for_test()),
        Some(5)
    );
    cx.executor().advance_clock(Duration::from_millis(90));
    wait_picture(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.diff_tolerance_for_test()),
        Some(6)
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.tolerance_text()),
        "Tolerance 6 · Side by side"
    );
}

#[gpui_kit::test]
async fn escape_during_a_tolerance_compare_puts_the_shown_tolerance_back(cx: &mut TestAppContext) {
    let data = use_settings(cx);
    let mut right = grey(1500, 1500);
    right.set_pixel(10, 10, [100, 100, 105, 255]);
    let (handle, view) = open_pictures(cx, grey(1500, 1500), right);
    wait_picture(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_tolerance(5, false, cx));
    assert!(view.read_with(cx, |v, _| v.is_busy()), "the compare runs");
    press(cx, handle, "escape");
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        assert_eq!(v.notice(), Some("Cancelled: the result is incomplete."));
        assert_eq!(v.diff_tolerance_for_test(), Some(0), "the old result stays");
        assert_eq!(Some(v.tolerance()), v.diff_tolerance_for_test());
    });
    draw(cx, handle);
    assert_eq!(box_text(cx, &view), "0");
    let slider: Entity<SliderState> = view.read_with(cx, |v, _| v.slider_for_test());
    assert_eq!(
        slider.read_with(cx, |s, _| s.value()),
        SliderValue::Single(0.)
    );
    assert_eq!(saved_setting(&data, "picture.tolerance"), 0);

    // The same value again is computed, not taken as already applied.
    view.update(cx, |v, cx| v.set_tolerance(5, false, cx));
    wait_picture(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.diff_tolerance_for_test()),
        Some(5)
    );
    assert_eq!(counts(cx, &view), (0, 1));
}

#[gpui_kit::test]
async fn f8_after_a_cancelled_compare_says_there_is_no_result(cx: &mut TestAppContext) {
    let (_dir, a, b) = one_difference();
    let (handle, view) = open_view(cx, a, b, None);
    wait_picture(cx, handle, &view).await;
    press(cx, handle, "f9");
    assert!(view.read_with(cx, |v, _| v.is_busy()), "F9 compares again");
    press(cx, handle, "escape");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.diff().is_none()));
    press(cx, handle, "f8");
    assert_eq!(
        notice_of(cx, &view).as_deref(),
        Some("No comparison yet: press F5 to compare.")
    );
    press(cx, handle, "f5");
    wait_picture(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.diff().map(|d| d.count)),
        Some(1)
    );
}

#[gpui_kit::test]
async fn f5_during_a_compare_shows_only_the_reloaded_pictures(cx: &mut TestAppContext) {
    let (_dir, a, b) = one_difference();
    let (handle, view) = open_view(cx, a.clone(), b, None);
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    let dropped = view.read_with(cx, |v, _| v.dropped_for_test());
    // A compare of the old pictures runs when F5 reads a left picture that now matches.
    view.update(cx, |v, cx| v.set_tolerance(3, false, cx));
    assert!(view.read_with(cx, |v, _| v.is_busy()), "the compare runs");
    std::fs::write(&a, png(4, 4, Some((1, 1)))).unwrap();
    press(cx, handle, "f5");
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    view.read_with(cx, |v, _| {
        let c = v.compared().expect("compared");
        assert!(Arc::ptr_eq(&c.sides[0], v.shown(true).expect("left")));
        assert!(Arc::ptr_eq(&c.sides[1], v.shown(false).expect("right")));
        assert_eq!(c.tolerance, 3);
        assert_eq!(v.badge().as_deref(), Some("ok"));
        assert_eq!(v.notice(), Some("Pictures reloaded."));
        assert_eq!(
            v.dropped_for_test() - dropped,
            3,
            "the two old sides and the old difference image went, and no stale result came \
             in between"
        );
    });
}

#[gpui_kit::test]
async fn the_tolerance_box_shows_its_value_and_the_slider_reaches_each_one(
    cx: &mut TestAppContext,
) {
    // I-40: one pixel off by 3.
    let (left, right) = marked(&[((1, 1), [100, 100, 103, 255])]);
    let (handle, view) = open_pictures(cx, left, right);
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    let input: Entity<InputState> = view.read_with(cx, |v, _| v.tolerance_input_for_test());
    // The text has room for "255" between the step buttons.
    let text = input.read_with(cx, |state, _| state.input_bounds());
    assert!(text.size.width >= px(28.), "the box's text is {text:?}");
    // A plain click in the middle places the caret and changes nothing.
    let before = view.read_with(cx, |v, _| v.tolerance());
    click(cx, handle, "picture-tolerance-input");
    wait_picture(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.tolerance()), before);
    assert_eq!(box_text(cx, &view), before.to_string());
    let focused = cx
        .update_window(handle, |_, window, cx| {
            input.focus_handle(cx).is_focused(window)
        })
        .unwrap();
    assert!(focused, "the click gave the box the keys");
    // A selection is replaced by what is typed.
    cx.update_window(handle, |_, window, cx| {
        window.press("ctrl-a", cx);
        window.input("50", cx);
        window.press("enter", cx);
    })
    .unwrap();
    wait_picture(cx, handle, &view).await;
    assert_eq!(view.read_with(cx, |v, _| v.tolerance()), 50);
    // Every value has a whole pixel of its own on the slider: 3 is pressed exactly, and the
    // box and the counts follow.
    let slider: Entity<SliderState> = view.read_with(cx, |v, _| v.slider_for_test());
    let track = slider.read_with(cx, |s, _| s.bounds());
    let width = f32::from(track.size.width);
    let value_at = |x: u32| (255. * x as f32 / width).round() as u32;
    let reached: std::collections::BTreeSet<u32> = (0..=width as u32).map(value_at).collect();
    assert_eq!(reached.len(), 256, "a {width} px slider skips values");
    let three = (0..=width as u32).find(|&x| value_at(x) == 3).unwrap();
    let group = bounds_of(cx, handle, "picture-tolerance");
    let offset = point(
        track.left() - group.left() + px(three as f32),
        group.size.height / 2.,
    );
    cx.update_window(handle, |_, window, cx| {
        window.click_at("picture-tolerance", offset, cx)
    })
    .unwrap();
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    assert_eq!(view.read_with(cx, |v, _| v.tolerance()), 3);
    assert_eq!(box_text(cx, &view), "3");
    assert_eq!(
        counts(cx, &view),
        (0, 1),
        "the off-by-3 pixel is within the tolerance"
    );
}

#[gpui_kit::test]
async fn typing_in_the_tolerance_box_keeps_its_digits(cx: &mut TestAppContext) {
    let (left, right) = marked(&[]);
    let (handle, view) = open_pictures(cx, left, right);
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    let events = record_events(cx, &view);
    view.update(cx, |v, cx| v.set_zoom(2., None, cx));
    let input: Entity<InputState> = view.read_with(cx, |v, _| v.tolerance_input_for_test());
    cx.update_window(handle, |_, window, cx| {
        window.focus(&input.focus_handle(cx), cx);
        window.press("ctrl-a", cx);
    })
    .unwrap();
    press(cx, handle, "2");
    press(cx, handle, "4");
    assert_eq!(box_text(cx, &view), "24");
    assert_eq!(view.read_with(cx, |v, _| v.mode()), Mode::SideBySide);
    press(cx, handle, "f9");
    press(cx, handle, "ctrl-0");
    press(cx, handle, "escape");
    assert_eq!(zoom_of(cx, &view), 2.);
    assert!(events.borrow().is_empty(), "{:?}", events.borrow());
    assert_eq!(view.read_with(cx, |v, _| v.tolerance()), 24);
}

#[gpui_kit::test]
async fn the_status_names_counts_sizes_and_groups(cx: &mut TestAppContext) {
    let five = ((1, 1), [100, 100, 105, 255]);
    let fifty = ((2, 2), [100, 100, 150, 255]);

    // Identical.
    let (left, right) = marked(&[]);
    let (handle, view) = open_pictures(cx, left, right);
    wait_picture(cx, handle, &view).await;
    assert_eq!(status_texts(cx, &view), ["Every pixel is the same."]);
    assert_eq!(
        view.read_with(cx, |v, _| v.status_parts()[0].tone),
        Tone::RightOnly
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.tolerance_text()),
        "Tolerance 0 · Side by side"
    );

    // One difference.
    let (left, right) = marked(&[five]);
    let (handle, view) = open_pictures(cx, left, right);
    wait_picture(cx, handle, &view).await;
    assert_eq!(
        status_texts(cx, &view),
        ["1 pixel differs (6.25%)", "Group – / 1"]
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.status_parts()[0].tone),
        Tone::LeftOnly
    );
    press(cx, handle, "f8");
    assert_eq!(
        status_texts(cx, &view),
        ["1 pixel differs (6.25%)", "Group 1 / 1"]
    );

    // One above the tolerance and one within it.
    let (left, right) = marked(&[five, fifty]);
    let (handle, view) = open_pictures(cx, left, right);
    view.update(cx, |v, cx| v.set_tolerance(5, false, cx));
    wait_picture(cx, handle, &view).await;
    assert_eq!(
        status_texts(cx, &view),
        [
            "1 pixel differs (6.25%)",
            "1 more within the tolerance",
            "Group – / 1"
        ]
    );

    // Everything within the tolerance.
    let (left, right) = marked(&[five]);
    let (handle, view) = open_pictures(cx, left, right);
    view.update(cx, |v, cx| v.set_tolerance(5, false, cx));
    wait_picture(cx, handle, &view).await;
    assert_eq!(
        status_texts(cx, &view),
        [
            "Same within the tolerance",
            "1 pixel differs slightly, largest difference 5"
        ]
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.status_parts()[0].tone),
        Tone::RightOnly
    );

    // Different sizes with an identical overlap.
    let (handle, view) = open_pictures(cx, grey(4, 4), grey(4, 5));
    wait_picture(cx, handle, &view).await;
    assert_eq!(
        status_texts(cx, &view),
        [
            "Every pixel of the overlap is the same.",
            "Sizes differ (4×4 and 4×5): only the 4×4 overlap is compared."
        ]
    );

    // A failed side.
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.png");
    let broken = dir.path().join("b.png");
    std::fs::write(&a, png(4, 4, None)).unwrap();
    std::fs::write(&broken, b"\x89PNG\r\n\x1a\nnot a png").unwrap();
    let (handle, view) = open_view(cx, a, broken, None);
    wait_picture(cx, handle, &view).await;
    assert_eq!(
        status_texts(cx, &view),
        ["Open the other picture to start comparing."]
    );

    // Too big.
    let (_dir, a, b) = one_difference();
    let limits = Limits {
        max_pixels: 10,
        ..Limits::default()
    };
    let (handle, view) = open_view(cx, a, b, Some(limits));
    wait_picture(cx, handle, &view).await;
    assert_eq!(
        status_texts(cx, &view),
        ["A picture is larger than 40 megapixels: it is shown, but pixels are not compared."]
    );
}

#[gpui_kit::test]
async fn the_heads_show_name_size_and_format(cx: &mut TestAppContext) {
    let (_dir, a, b) = one_difference();
    let len = std::fs::metadata(&a).unwrap().len();
    let (handle, view) = open_view(cx, a, b.clone(), None);
    view.read_with(cx, |v, _| {
        assert_eq!(v.head_text(true).meta, "Loading…");
    });
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    for id in [
        "picture-heads",
        "picture-open-left",
        "picture-open-right",
        "picture-left-name",
        "picture-right-name",
        "picture-left-meta",
        "picture-right-meta",
    ] {
        assert!(has(cx, handle, id), "{id} is drawn");
    }
    assert_eq!(
        view.read_with(cx, |v, _| v.head_text(true)),
        HeadText {
            name: "a.png".to_owned(),
            meta: format!("4×4 · {} · PNG", human_size(len)),
            dims_differ: false,
            failed: false,
        }
    );

    let (_dir, a, other) = write_pair(&png(4, 4, None), &png(4, 5, None));
    let (handle, view) = open_view(cx, a, other, None);
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert!(v.head_text(true).dims_differ);
        assert!(v.head_text(false).dims_differ);
        assert!(v.head_text(false).meta.starts_with("4×5 · "));
    });

    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join("b.png");
    std::fs::write(&broken, b"\x89PNG\r\n\x1a\nnot a png").unwrap();
    let a = dir.path().join("a.png");
    std::fs::write(&a, png(4, 4, None)).unwrap();
    let (handle, view) = open_view(cx, a, broken, None);
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        let head = v.head_text(false);
        assert_eq!(head.meta, "Can't open");
        assert!(head.failed);
        assert_eq!(head.name, "b.png");
        assert!(!v.head_text(true).failed);
    });
}

#[gpui_kit::test]
async fn a_head_name_shows_its_full_path_on_hover(cx: &mut TestAppContext) {
    let (_dir, a, b) = one_difference();
    let (handle, view) = open_view(cx, a.clone(), b, None);
    wait_picture(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.hover("picture-left-name", cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("tooltip-text").is_some(),
            "hovering the name shows its tooltip"
        );
    })
    .unwrap();
    view.read_with(cx, |v, _| {
        assert_eq!(v.name_tooltip(true), Some(a.display().to_string()));
    });

    // A picture with no file is named by its name; a head that names nothing has no tooltip.
    let (_handle, view) = open_pictures(cx, grey(2, 2), grey(2, 2));
    view.read_with(cx, |v, _| {
        assert_eq!(v.name_tooltip(false).as_deref(), Some("right.png"));
    });
    let (handle, view) = open_view(cx, PathBuf::new(), PathBuf::new(), None);
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert_eq!(v.head_text(true).name, "");
        assert_eq!(v.name_tooltip(true), None);
    });
}

/// Moves the pointer to `at` and draws.
fn hover(cx: &mut TestAppContext, handle: AnyWindowHandle, at: Point<Pixels>) {
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(
            MouseMoveEvent {
                position: at,
                pressed_button: None,
                modifiers: Modifiers::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

fn rect_of(cx: &mut TestAppContext, view: &Entity<PictureView>, pane: Pane) -> Bounds<Pixels> {
    painted(cx, view)
        .iter()
        .find(|p| p.pane == pane && p.layer != Layer::Difference)
        .expect("a painted picture")
        .rect
}

fn probe(cx: &mut TestAppContext, view: &Entity<PictureView>) -> Option<String> {
    view.read_with(cx, |v, _| v.probe_text())
}

#[gpui_kit::test]
async fn the_probe_reads_both_pictures_under_the_pointer(cx: &mut TestAppContext) {
    let (left, right) = marked(&[((1, 1), BLUE_ISH)]);
    let (handle, view) = open_pictures(cx, left, right);
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    view.update(cx, |v, cx| v.set_zoom(1., None, cx));
    draw(cx, handle);
    assert_eq!(probe(cx, &view), None);

    let rect = rect_of(cx, &view, Pane::Left);
    hover(cx, handle, rect.origin + point(px(1.5), px(1.5)));
    assert_eq!(
        probe(cx, &view).as_deref(),
        Some("1, 1 · Left #646464 · Right #6464c8")
    );
    assert!(view.read_with(cx, |v, _| v.probe_differs()));
    draw(cx, handle);
    assert!(has(cx, handle, "picture-probe"));

    hover(cx, handle, rect.origin + point(px(0.5), px(0.5)));
    assert_eq!(
        probe(cx, &view).as_deref(),
        Some("0, 0 · Left #646464 · Right #646464")
    );
    assert!(!view.read_with(cx, |v, _| v.probe_differs()));

    // The right pane reads the same pixel.
    let right_rect = rect_of(cx, &view, Pane::Right);
    hover(cx, handle, right_rect.origin + point(px(1.5), px(1.5)));
    assert_eq!(
        probe(cx, &view).as_deref(),
        Some("1, 1 · Left #646464 · Right #6464c8")
    );

    // Outside the picture, in the pane.
    hover(cx, handle, rect.origin - point(px(3.), px(3.)));
    assert_eq!(probe(cx, &view), None);

    // A tolerance at the delta makes the pair the same.
    hover(cx, handle, rect.origin + point(px(1.5), px(1.5)));
    view.update(cx, |v, cx| v.set_tolerance(100, false, cx));
    assert!(!view.read_with(cx, |v, _| v.probe_differs()));
    view.update(cx, |v, cx| v.set_tolerance(99, false, cx));
    assert!(view.read_with(cx, |v, _| v.probe_differs()));
}

#[gpui_kit::test]
async fn the_probe_marks_a_missing_pixel_and_a_big_side(cx: &mut TestAppContext) {
    let (handle, view) = open_pictures(cx, grey(4, 4), grey(2, 2));
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    view.update(cx, |v, cx| v.set_zoom(1., None, cx));
    draw(cx, handle);
    let rect = rect_of(cx, &view, Pane::Left);
    hover(cx, handle, rect.origin + point(px(3.5), px(3.5)));
    assert_eq!(
        probe(cx, &view).as_deref(),
        Some("3, 3 · Left #646464 · Right —")
    );
    assert!(!view.read_with(cx, |v, _| v.probe_differs()));

    let (_dir, a, b) = one_difference();
    let limits = Limits {
        max_pixels: 10,
        ..Limits::default()
    };
    let (handle, view) = open_view(cx, a, b, Some(limits));
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    view.update(cx, |v, cx| v.set_zoom(1., None, cx));
    draw(cx, handle);
    let rect = rect_of(cx, &view, Pane::Left);
    hover(cx, handle, rect.origin + point(px(0.5), px(0.5)));
    assert_eq!(probe(cx, &view).as_deref(), Some("0, 0 · Left ? · Right ?"));
    assert!(!view.read_with(cx, |v, _| v.probe_differs()));
}

#[gpui_kit::test]
async fn the_legend_shows_in_difference_mode_only(cx: &mut TestAppContext) {
    let (left, right) = marked(&[((1, 1), BLUE_ISH)]);
    let (handle, view) = open_pictures(cx, left, right);
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    assert!(!has(cx, handle, "picture-legend"));

    view.update(cx, |v, cx| v.set_mode(Mode::Difference, cx));
    draw(cx, handle);
    assert!(has(cx, handle, "picture-legend"));
    for (id, label) in [
        ("picture-legend-diff", "Differs"),
        ("picture-legend-minor", "Within tolerance"),
        ("picture-legend-only", "One side only"),
    ] {
        assert_eq!(label_of(cx, handle, id).as_deref(), Some(label), "{id}");
    }

    view.update(cx, |v, cx| v.set_mode(Mode::Blink, cx));
    draw(cx, handle);
    assert!(!has(cx, handle, "picture-legend"));
}

#[gpui_kit::test]
async fn buttons_follow_what_they_need(cx: &mut TestAppContext) {
    let (handle, view) = open_pictures(cx, grey(4, 4), grey(4, 4));
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    for id in [
        "picture-zoom-fit",
        "picture-zoom-1",
        "picture-zoom-out",
        "picture-zoom-pct",
        "picture-zoom-in",
        "picture-prev",
        "picture-next",
        "picture-swap",
        "picture-reload",
        "picture-export",
    ] {
        assert!(has(cx, handle, id), "{id} is drawn");
    }
    view.read_with(cx, |v, _| {
        assert!(v.can_export());
        assert!(!v.can_navigate());
        assert!(!v.can_reload(), "pictures without files");
        assert!(v.can_swap());
    });
    assert_eq!(
        label_of(cx, handle, "picture-zoom-pct").as_deref(),
        Some("100%")
    );

    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.png");
    let broken = dir.path().join("b.png");
    std::fs::write(&a, png(4, 4, None)).unwrap();
    std::fs::write(&broken, b"\x89PNG\r\n\x1a\nnot a png").unwrap();
    let (handle, view) = open_view(cx, a, broken, None);
    wait_picture(cx, handle, &view).await;
    view.read_with(cx, |v, _| {
        assert!(!v.can_export());
        assert!(!v.can_navigate());
        assert!(v.can_reload());
        assert!(v.can_swap());
    });

    let (left, right) = marked(&[((1, 1), BLUE_ISH)]);
    let (handle, view) = open_pictures(cx, left, right);
    wait_picture(cx, handle, &view).await;
    assert!(view.read_with(cx, |v, _| v.can_navigate()));
    // Clicking the swap button swaps.
    draw(cx, handle);
    click(cx, handle, "picture-swap");
    wait_picture(cx, handle, &view).await;
    assert_eq!(
        view.read_with(cx, |v, _| v.side(true).map(|s| s.name.clone()))
            .as_deref(),
        Some("right.png")
    );
}

// ---- Menu, export, open, typed paths, drop and Escape (Task 7) ----

fn run_menu(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<PictureView>,
    label: &str,
) -> bool {
    let entries = view.read_with(cx, |v, _| duet_app::picture::menu::context_menu(v));
    let view = view.clone();
    let ran = cx
        .update_window(handle, |_, window, cx| {
            let focus = view.focus_handle(cx);
            duet_app::ui::menu::activate(entries, label, Some(&focus), window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    ran
}

fn menu_outline(cx: &mut TestAppContext, view: &Entity<PictureView>) -> Vec<String> {
    view.read_with(cx, |v, _| {
        duet_app::ui::menu::outline(&duet_app::picture::menu::context_menu(v))
    })
}

fn notice_of(cx: &mut TestAppContext, view: &Entity<PictureView>) -> Option<String> {
    view.read_with(cx, |v, _| v.notice().map(str::to_owned))
}

#[gpui_kit::test]
async fn the_picture_menu_matches_the_state(cx: &mut TestAppContext) {
    let (_dir, a, b) = write_pair(&png(4, 4, None), &png(4, 4, None));
    let (handle, view) = open_view(cx, a, b, None);
    wait_picture(cx, handle, &view).await;
    assert_eq!(
        menu_outline(cx, &view),
        [
            "Fit to window",
            "Actual size (1:1)",
            "-",
            "● Side by side",
            "Difference",
            "Slider",
            "Blink",
            "-",
            "(Next difference)",
            "(Previous difference)",
            "-",
            "Swap left and right",
            "Reload",
            "Export difference image (PNG)…"
        ]
    );

    assert!(run_menu(cx, handle, &view, "Blink"));
    let outline = menu_outline(cx, &view);
    assert!(outline.contains(&"● Blink".to_owned()), "{outline:?}");
    assert!(outline.contains(&"Side by side".to_owned()), "{outline:?}");
    assert_eq!(view.read_with(cx, |v, _| v.prefs().mode), Mode::Blink);

    // A differing pair has groups to walk.
    let (_dir2, a, b) = one_difference();
    let (handle2, differing) = open_view(cx, a, b, None);
    wait_picture(cx, handle2, &differing).await;
    let outline = menu_outline(cx, &differing);
    assert!(
        outline.contains(&"Next difference".to_owned()),
        "{outline:?}"
    );
    assert!(outline.contains(&"Previous difference".to_owned()));

    // A failed side cannot export.
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.png");
    let broken = dir.path().join("b.png");
    std::fs::write(&a, png(4, 4, None)).unwrap();
    std::fs::write(&broken, b"not a png at all").unwrap();
    let (handle3, failed) = open_view(cx, a, broken, None);
    wait_picture(cx, handle3, &failed).await;
    let outline = menu_outline(cx, &failed);
    assert!(
        outline.contains(&"(Export difference image (PNG)…)".to_owned()),
        "{outline:?}"
    );
}

#[gpui_kit::test]
async fn menu_items_run_their_actions(cx: &mut TestAppContext) {
    let (_dir, a, b) = one_difference();
    let (handle, view) = open_view(cx, a.clone(), b.clone(), None);
    let events = record_events(cx, &view);
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    assert!(run_menu(cx, handle, &view, "Actual size (1:1)"));
    assert_eq!(zoom_of(cx, &view), 1.);
    assert!(!view.read_with(cx, |v, _| v.is_fit()));
    assert!(run_menu(cx, handle, &view, "Fit to window"));
    assert!(view.read_with(cx, |v, _| v.is_fit()));
    assert!(run_menu(cx, handle, &view, "Next difference"));
    assert_eq!(view.read_with(cx, |v, _| v.current_group()), Some(0));
    assert!(run_menu(cx, handle, &view, "Swap left and right"));
    wait_picture(cx, handle, &view).await;
    assert_eq!(events.borrow().as_slice(), ["swapped b.png a.png"]);
    std::fs::write(&a, png(4, 4, Some((1, 1)))).unwrap();
    assert!(run_menu(cx, handle, &view, "Reload"));
    wait_picture(cx, handle, &view).await;
    assert_eq!(notice_of(cx, &view).as_deref(), Some("Pictures reloaded."));
}

async fn pick_export(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<PictureView>,
    target: impl FnOnce(&Path) -> Option<PathBuf>,
) {
    // A fresh load clears the last export's notice, so the wait below sees this one.
    view.update(cx, |v, cx| v.reload(cx));
    wait_picture(cx, handle, view).await;
    assert!(run_menu(cx, handle, view, "Export difference image (PNG)…"));
    assert!(cx.did_prompt_for_new_path(), "export asks where to save");
    cx.simulate_new_path_selection(target);
    let seen = view.clone();
    cx.wait_for(handle, Duration::from_secs(20), move |_, cx| {
        seen.read(cx)
            .notice()
            .is_some_and(|n| n.starts_with("Exported: ") || n.starts_with("Can't export: "))
    })
    .await;
}

#[gpui_kit::test]
async fn export_writes_the_difference_png(cx: &mut TestAppContext) {
    let (dir, a, b) = one_difference();
    let (handle, view) = open_view(cx, a, b, None);
    wait_picture(cx, handle, &view).await;
    view.update(cx, |v, cx| v.set_tolerance(5, false, cx));
    wait_picture(cx, handle, &view).await;
    let folder = dir.path().to_path_buf();
    let out = folder.join("out.png");
    let target = out.clone();
    pick_export(cx, handle, &view, move |start| {
        assert_eq!(start, folder, "the prompt starts in the left file's folder");
        Some(target)
    })
    .await;
    assert_eq!(
        notice_of(cx, &view),
        Some(format!("Exported: {}.", out.display()))
    );
    let bytes = std::fs::read(&out).unwrap();
    let written = duet_core::picture::decode::decode(
        &bytes,
        "out.png",
        &Limits::default(),
        &duet_core::cancel::CancelToken::new(),
    )
    .unwrap()
    .pixels
    .expect("a small picture");
    let (sides, tolerance) = view.read_with(cx, |v, _| {
        let c = v.compared().expect("compared");
        (c.sides.clone(), c.tolerance)
    });
    assert_eq!(tolerance, 5);
    let want = duet_core::picture::diff::diff_image(
        &sides[0],
        &sides[1],
        tolerance,
        &duet_core::cancel::CancelToken::new(),
    )
    .unwrap();
    assert_eq!((written.width, written.height), (want.width, want.height));
    assert!(
        written.rgba == want.rgba,
        "the file is the difference image"
    );

    let missing = dir.path().join("none").join("x.png");
    pick_export(cx, handle, &view, move |_| Some(missing)).await;
    let notice = notice_of(cx, &view);
    assert!(
        notice
            .as_deref()
            .is_some_and(|n| n.starts_with("Can't export: ") && n.ends_with('.')),
        "{notice:?}"
    );
}

#[gpui_kit::test]
async fn while_a_side_loads_every_mode_says_loading_its_name(cx: &mut TestAppContext) {
    // I-55 (C24): Ctrl+Shift+O, the pick, then the frames drawn while the right side loads.
    let (dir, a, b) = one_difference();
    let big = dir.path().join("big42.png");
    std::fs::write(&big, png(64, 64, None)).unwrap();
    let (handle, _workspace, view) = open_workspace(cx, a, b).await;
    for (mode_key, mode) in [("2", Mode::Difference), ("1", Mode::SideBySide)] {
        press(cx, handle, mode_key);
        draw(cx, handle);
        assert_eq!(view.read_with(cx, |v, _| v.mode()), mode);
        press(cx, handle, "ctrl-shift-o");
        assert!(cx.did_prompt_for_paths());
        let picked = big.clone();
        cx.simulate_path_prompt_response(move |_| Some(vec![picked]));
        // Step until the new right side is loading, then look at that frame.
        for _ in 0..100_000 {
            if view.read_with(cx, |v, _| v.loading(false)) || !cx.executor().tick() {
                break;
            }
        }
        assert!(
            view.read_with(cx, |v, _| v.loading(false)),
            "{mode:?}: the right side loads"
        );
        draw(cx, handle);
        // The pair is opened again, so the left may still be loading too: then both are named.
        let status = status_texts(cx, &view);
        let loading =
            |text: &str| text == "Loading big42.png…" || text == "Loading a.png and big42.png…";
        assert!(
            status.len() == 1 && loading(&status[0]),
            "{mode:?}: the status names the picture: {status:?}"
        );
        let message = view.read_with(cx, |v, _| v.message_overlay_for_test());
        if mode == Mode::SideBySide {
            assert_eq!(message, None, "side by side: the pane says it");
        } else {
            assert_eq!(message.as_deref(), Some(status[0].as_str()), "{mode:?}");
            assert_eq!(
                label_of(cx, handle, "picture-message"),
                Some(status[0].clone()),
                "{mode:?}: drawn over the pane"
            );
        }
        wait_picture(cx, handle, &view).await;
        draw(cx, handle);
        assert!(
            !status_texts(cx, &view)
                .iter()
                .any(|t| t.starts_with("Loading")),
            "{mode:?}: loaded"
        );
        assert_eq!(
            view.read_with(cx, |v, _| v.message_overlay_for_test()),
            None
        );
        // a.png goes back on the right, so the next round's pick is a change again.
        let back = view.read_with(cx, |v, _| v.paths().0.to_path_buf());
        view.update(cx, |v, cx| v.drop_paths(&[back], false, cx));
        cx.run_until_parked();
        wait_picture(cx, handle, &view).await;
    }
}

#[gpui_kit::test]
async fn open_typed_path_and_drop_replace_a_side(cx: &mut TestAppContext) {
    let (dir, a, b) = one_difference();
    let (c, d, e) = (
        dir.path().join("c.png"),
        dir.path().join("d.png"),
        dir.path().join("e.png"),
    );
    for p in [&c, &d, &e] {
        std::fs::write(p, png(4, 4, None)).unwrap();
    }
    let (handle, workspace, view) = open_workspace(cx, a.clone(), b.clone()).await;
    let paths =
        |cx: &mut TestAppContext| view.read_with(cx, |v, _| (name(v.paths().0), name(v.paths().1)));
    let pair = |l: &str, r: &str| (l.to_owned(), r.to_owned());

    press(cx, handle, "ctrl-o");
    assert!(cx.did_prompt_for_paths());
    let picked = c.clone();
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.files && !options.directories && !options.multiple);
        Some(vec![picked])
    });
    cx.run_until_parked();
    wait_picture(cx, handle, &view).await;
    assert_eq!(paths(cx), pair("c.png", "b.png"));

    draw(cx, handle);
    click(cx, handle, "picture-right-name");
    assert_eq!(
        view.read_with(cx, |v, _| v.path_edit_for_test()),
        Some(false)
    );
    view.update(cx, |v, cx| {
        v.set_path_edit_text_for_test(d.to_str().unwrap(), cx)
    });
    press(cx, handle, "enter");
    cx.run_until_parked();
    wait_picture(cx, handle, &view).await;
    assert_eq!(paths(cx), pair("c.png", "d.png"));
    assert_eq!(view.read_with(cx, |v, _| v.path_edit_for_test()), None);

    view.update(cx, |v, cx| {
        v.drop_paths(std::slice::from_ref(&e), false, cx)
    });
    cx.run_until_parked();
    wait_picture(cx, handle, &view).await;
    assert_eq!(paths(cx), pair("c.png", "e.png"));

    view.update(cx, |v, cx| {
        v.drop_paths(&[a.clone(), b.clone(), c.clone()], false, cx)
    });
    cx.run_until_parked();
    wait_picture(cx, handle, &view).await;
    assert_eq!(paths(cx), pair("a.png", "b.png"));
    assert_eq!(
        notice_of(cx, &view).as_deref(),
        Some("3 files dropped: the first two are used.")
    );

    view.update(cx, |v, cx| {
        v.drop_paths(&[dir.path().to_path_buf()], true, cx)
    });
    assert_eq!(
        notice_of(cx, &view).as_deref(),
        Some(duet_app::pathbar::FOLDER_DROP_NOTICE)
    );
    assert_eq!(paths(cx), pair("a.png", "b.png"));

    // A pair that another tab shows activates that tab.
    cx.update_window(handle, |_, window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_picture(c.clone(), d.clone(), None, window, cx)
        })
    })
    .unwrap();
    cx.run_until_parked();
    let tabs = workspace.read_with(cx, |w, _| w.tabs().len());
    cx.update_window(handle, |_, _, cx| {
        workspace.update(cx, |w, _| w.tabs_mut_for_test().activate(0));
        view.update(cx, |v, cx| v.drop_paths(&[c.clone(), d.clone()], true, cx));
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(workspace.read_with(cx, |w, _| w.tabs().len()), tabs);
    assert_eq!(
        workspace.read_with(cx, |w, _| w.tabs().active_index()),
        Some(tabs - 1),
        "the tab that already shows the pair is active"
    );
    assert_eq!(paths(cx), pair("a.png", "b.png"), "this tab is unchanged");
}

#[gpui_kit::test]
async fn digits_f9_and_f5_typed_in_the_path_box_stay_with_it(cx: &mut TestAppContext) {
    let (_dir, a, b) = one_difference();
    let (handle, view) = open_view(cx, a.clone(), b.clone(), None);
    let events = record_events(cx, &view);
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    click(cx, handle, "picture-left-name");
    assert_eq!(
        view.read_with(cx, |v, _| v.path_edit_for_test()),
        Some(true)
    );
    // The path is all selected: the digits replace it.
    for key in ["2", "3", "4", "f9", "f5"] {
        press(cx, handle, key);
    }
    view.read_with(cx, |v, cx| {
        let text = v.path_edit_state_for_test(cx).map(|(text, _)| text);
        assert_eq!(text.as_deref(), Some("234"));
        assert_eq!(v.mode(), Mode::SideBySide, "no digit switched the mode");
        assert_eq!(v.paths(), (a.as_path(), b.as_path()), "F9 did not swap");
        assert!(!v.is_busy(), "F5 did not reload");
    });
    assert!(events.borrow().is_empty(), "{:?}", events.borrow());
}

#[gpui_kit::test]
async fn escape_ends_the_path_edit_then_cancels_then_clears_the_group_then_goes_back(
    cx: &mut TestAppContext,
) {
    let (_dir, a, b) = one_difference();
    let (handle, view) = open_view(cx, a, b, None);
    let events = record_events(cx, &view);
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    view.update(cx, |v, cx| v.go(true, cx));
    assert_eq!(view.read_with(cx, |v, _| v.current_group()), Some(0));

    // The path box is open while a load runs: Esc ends the edit only.
    view.update(cx, |v, cx| v.reload(cx));
    assert!(view.read_with(cx, |v, _| v.is_busy()));
    draw(cx, handle);
    click(cx, handle, "picture-left-name");
    assert_eq!(
        view.read_with(cx, |v, _| v.path_edit_for_test()),
        Some(true)
    );
    press(cx, handle, "escape");
    assert_eq!(view.read_with(cx, |v, _| v.path_edit_for_test()), None);
    assert!(
        view.read_with(cx, |v, _| v.is_busy()),
        "the load still runs"
    );
    assert!(events.borrow().is_empty());

    press(cx, handle, "escape");
    assert!(!view.read_with(cx, |v, _| v.is_busy()));
    assert_eq!(
        notice_of(cx, &view).as_deref(),
        Some("Cancelled: the result is incomplete.")
    );
    assert_eq!(view.read_with(cx, |v, _| v.current_group()), Some(0));

    press(cx, handle, "escape");
    assert_eq!(view.read_with(cx, |v, _| v.current_group()), None);
    assert!(events.borrow().is_empty());
    press(cx, handle, "escape");
    assert_eq!(events.borrow().as_slice(), ["back"]);
}

#[gpui_kit::test]
async fn the_open_and_export_buttons_ask_and_the_body_has_drop_zones(cx: &mut TestAppContext) {
    let (_dir, a, b) = one_difference();
    let (handle, view) = open_view(cx, a, b, None);
    wait_picture(cx, handle, &view).await;
    draw(cx, handle);
    assert!(has(cx, handle, "picture-drop-left") && has(cx, handle, "picture-drop-right"));
    let (left, right) = (
        bounds_of(cx, handle, "picture-drop-left"),
        bounds_of(cx, handle, "picture-drop-right"),
    );
    assert!(
        left.right() <= right.left() + px(1.),
        "two halves, side by side"
    );
    for id in ["picture-open-left", "picture-open-right"] {
        click(cx, handle, id);
        assert!(cx.did_prompt_for_paths(), "{id} asks for a file");
        cx.simulate_path_prompt_response(|_| None);
    }
    click(cx, handle, "picture-export");
    assert!(
        cx.did_prompt_for_new_path(),
        "the button asks where to save"
    );
}

#[gpui_kit::test]
async fn a_file_dragged_over_the_body_shows_both_drop_zones(cx: &mut TestAppContext) {
    let (dir, a, b) = one_difference();
    let (handle, view) = open_view(cx, a, b, None);
    wait_picture(cx, handle, &view).await;
    let visible = |cx: &mut TestAppContext, id: &'static str| {
        cx.update_window(handle, |_, window, _| window.find(id).visible())
            .unwrap()
    };
    draw(cx, handle);
    for id in ["picture-drop-left", "picture-drop-right"] {
        assert!(!visible(cx, id), "{id} is see-through at rest");
    }
    cx.update_window(handle, |_, window, cx| {
        let position = window.find("picture-drop-right").bounds().center();
        let mut paths = gpui_kit::ExternalPaths::default();
        paths.0.push(dir.path().join("c.png"));
        window.dispatch_event(
            gpui_kit::PlatformInput::FileDrop(gpui_kit::FileDropEvent::Entered { position, paths }),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    for id in ["picture-drop-left", "picture-drop-right"] {
        assert!(visible(cx, id), "{id} shows while a file is over the body");
    }
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(
            gpui_kit::PlatformInput::FileDrop(gpui_kit::FileDropEvent::Exited),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    assert!(
        !visible(cx, "picture-drop-right"),
        "gone when the drag leaves"
    );
}
