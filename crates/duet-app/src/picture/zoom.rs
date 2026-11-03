//! The picture viewport maths (R7, R8, R10, `js:364-451`): zoom presets, fit, the centred or
//! scrolled stage, zoom about a point, the pointer's picture pixel, the scrollbars a pane
//! needs, clips, the group marker and the source pixels an enlargement covers. Pure; geometry
//! is in logical px.

use gpui_kit::{Bounds, Pixels, Point, ScrollDelta, Size, point, px, size};

pub const ZOOM_STEPS: [f32; 23] = [
    0.02, 0.05, 0.1, 0.125, 0.167, 0.25, 0.333, 0.5, 0.667, 0.75, 1., 1.5, 2., 3., 4., 6., 8., 12.,
    16., 24., 32., 48., 64.,
];
pub const ZOOM_MIN: f32 = 0.02;
pub const ZOOM_MAX: f32 = 64.;
/// A wheel line counts this many px (LocalDiff's line factor).
pub const LINE_PX: f32 = 33.;
/// Room kept around a fitted picture, and the least room a fit needs.
const FIT_MARGIN: f32 = 6.;
const FIT_MIN_ROOM: f32 = 16.;
/// Slack of the preset walk, so 1.0005 still counts as 100%.
const STEP_SLACK: f32 = 1.001;

fn clamp_zoom(zoom: f32) -> f32 {
    zoom.clamp(ZOOM_MIN, ZOOM_MAX)
}

/// The zoom that fits the whole picture into the pane, never above 100%. None while the pane
/// has no room yet (hidden), or the picture is empty.
pub fn fit_zoom(pane: Size<f32>, dims: (u32, u32)) -> Option<f32> {
    if dims.0 == 0 || dims.1 == 0 {
        return None;
    }
    let (room_w, room_h) = (pane.width - FIT_MARGIN, pane.height - FIT_MARGIN);
    if room_w < FIT_MIN_ROOM || room_h < FIT_MIN_ROOM {
        return None;
    }
    let fit = 1f32.min(room_w / dims.0 as f32).min(room_h / dims.1 as f32);
    Some(clamp_zoom(fit))
}

/// The next preset above (`up`) or below the zoom.
pub fn step_zoom(zoom: f32, up: bool) -> f32 {
    if up {
        ZOOM_STEPS
            .iter()
            .copied()
            .find(|&step| step > zoom * STEP_SLACK)
            .unwrap_or(ZOOM_MAX)
    } else {
        ZOOM_STEPS
            .iter()
            .rev()
            .copied()
            .find(|&step| step < zoom / STEP_SLACK)
            .unwrap_or(ZOOM_MIN)
    }
}

fn delta_px(delta: ScrollDelta) -> Point<f32> {
    match delta {
        ScrollDelta::Lines(lines) => point(lines.x * LINE_PX, lines.y * LINE_PX),
        ScrollDelta::Pixels(px) => point(f32::from(px.x), f32::from(px.y)),
    }
}

/// The Ctrl+wheel zoom factor: `clamp(1.0015^dy, 0.5, 2)`, wheel up zooming in.
pub fn wheel_factor(delta: ScrollDelta) -> f32 {
    1.0015f32.powf(delta_px(delta).y).clamp(0.5, 2.)
}

/// The px to add to the scroll for a plain wheel turn: wheel down scrolls down. Shift (or a
/// sideways wheel) scrolls sideways.
pub fn wheel_scroll(delta: ScrollDelta, shift: bool) -> Point<f32> {
    let d = delta_px(delta);
    if shift {
        point(-(d.x + d.y), 0.)
    } else {
        point(-d.x, -d.y)
    }
}

fn axis_origin(pane: f32, stage: f32, scroll: f32) -> f32 {
    if stage < pane {
        (pane - stage) / 2.
    } else {
        -scroll
    }
}

/// Where the stage's top-left sits in the pane: centred when smaller (CSS `margin: auto`),
/// else shifted back by the scroll.
pub fn stage_origin(pane: Size<f32>, stage: Size<f32>, scroll: Point<f32>) -> Point<f32> {
    point(
        axis_origin(pane.width, stage.width, scroll.x),
        axis_origin(pane.height, stage.height, scroll.y),
    )
}

/// The scroll limited to `0..=stage - pane`; a stage that fits cannot scroll.
pub fn clamp_scroll(pane: Size<f32>, stage: Size<f32>, scroll: Point<f32>) -> Point<f32> {
    point(
        scroll.x.clamp(0., (stage.width - pane.width).max(0.)),
        scroll.y.clamp(0., (stage.height - pane.height).max(0.)),
    )
}

fn stage_of(dims: (u32, u32), zoom: f32) -> Size<f32> {
    Size {
        width: dims.0 as f32 * zoom,
        height: dims.1 as f32 * zoom,
    }
}

/// The new scroll after zooming `from` → `to` about `anchor` (pane px), clamped.
pub fn zoom_about(
    anchor: Point<f32>,
    pane: Size<f32>,
    dims: (u32, u32),
    from: f32,
    scroll: Point<f32>,
    to: f32,
) -> Point<f32> {
    let origin = stage_origin(pane, stage_of(dims, from), scroll);
    let (ix, iy) = ((anchor.x - origin.x) / from, (anchor.y - origin.y) / from);
    clamp_scroll(
        pane,
        stage_of(dims, to),
        point(ix * to - anchor.x, iy * to - anchor.y),
    )
}

/// The scroll that brings the middle of `rect` (x, y, w, h in picture px) to the middle of the
/// pane, clamped.
pub fn center_on(
    rect: (u32, u32, u32, u32),
    pane: Size<f32>,
    dims: (u32, u32),
    zoom: f32,
) -> Point<f32> {
    let (cx, cy) = (
        (rect.0 as f32 + rect.2 as f32 / 2.) * zoom,
        (rect.1 as f32 + rect.3 as f32 / 2.) * zoom,
    );
    clamp_scroll(
        pane,
        stage_of(dims, zoom),
        point(cx - pane.width / 2., cy - pane.height / 2.),
    )
}

/// The picture pixel under a pane point, None outside the picture.
pub fn image_point(
    pointer: Point<f32>,
    pane: Size<f32>,
    dims: (u32, u32),
    zoom: f32,
    scroll: Point<f32>,
) -> Option<(u32, u32)> {
    let origin = stage_origin(pane, stage_of(dims, zoom), scroll);
    let (x, y) = ((pointer.x - origin.x) / zoom, (pointer.y - origin.y) / zoom);
    if x < 0. || y < 0. || x >= dims.0 as f32 || y >= dims.1 as f32 {
        return None;
    }
    Some((x as u32, y as u32))
}

/// The slider divider's place across the picture, 0..=1.
pub fn split_at(
    pointer_x: f32,
    pane: Size<f32>,
    dims: (u32, u32),
    zoom: f32,
    scroll: Point<f32>,
) -> f32 {
    let stage = stage_of(dims, zoom);
    if stage.width <= 0. {
        return 0.;
    }
    let origin = stage_origin(pane, stage, scroll);
    ((pointer_x - origin.x) / stage.width).clamp(0., 1.)
}

/// "100%": the zoom ×100, rounded half away from zero (PF10).
pub fn zoom_label(zoom: f32) -> String {
    format!("{}%", (zoom * 100.).round() as i64)
}

pub fn bounds_at(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
}

/// The scrollbars a pane of `area` needs for `stage`, each `bar` px thick: (horizontal,
/// vertical). Each takes room from the other axis, as `overflow: auto` does.
pub fn scrollbars(area: Size<f32>, stage: Size<f32>, bar: f32) -> (bool, bool) {
    let mut horizontal = stage.width > area.width;
    let mut vertical = stage.height > area.height;
    if vertical && !horizontal {
        horizontal = stage.width > area.width - bar;
    }
    if horizontal && !vertical {
        vertical = stage.height > area.height - bar;
    }
    (horizontal, vertical)
}

/// `inner` cut to `outer`; exactly `inner` when it lies inside.
pub fn clip_to(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> Bounds<Pixels> {
    let inside = outer.left() <= inner.left()
        && inner.right() <= outer.right()
        && outer.top() <= inner.top()
        && inner.bottom() <= outer.bottom();
    if inside {
        inner
    } else {
        inner.intersect(&outer)
    }
}

/// `bounds` cut sideways to start at `from` and end at `to`, where given (the slider).
pub fn clip_x(bounds: Bounds<Pixels>, from: Option<f32>, to: Option<f32>) -> Bounds<Pixels> {
    let (left, right) = (f32::from(bounds.left()), f32::from(bounds.right()));
    let left = from.map_or(left, |from| left.max(from));
    let right = to.map_or(right, |to| right.min(to)).max(left);
    bounds_at(
        left,
        f32::from(bounds.top()),
        right - left,
        f32::from(bounds.size.height),
    )
}

/// The group marker around `rect` (x, y, w, h in picture px) of a stage whose top-left is at
/// `origin` (R10): 3 px outside the group on every side.
pub fn group_marker(
    origin: Point<Pixels>,
    rect: (u32, u32, u32, u32),
    zoom: f32,
) -> Bounds<Pixels> {
    bounds_at(
        f32::from(origin.x) + rect.0 as f32 * zoom - 3.,
        f32::from(origin.y) + rect.1 as f32 * zoom - 3.,
        rect.2 as f32 * zoom + 6.,
        rect.3 as f32 * zoom + 6.,
    )
}

/// The `side` px squares of a pattern anchored at `rect`'s top-left that `clip` touches.
pub fn pattern_cells(rect: Bounds<Pixels>, clip: Bounds<Pixels>, side: f32) -> Vec<Bounds<Pixels>> {
    let (rx, ry) = (f32::from(rect.left()), f32::from(rect.top()));
    let cells = |from: Pixels, to: Pixels, origin: f32| {
        let first = ((f32::from(from) - origin) / side).floor().max(0.) as u32;
        let end = ((f32::from(to) - origin) / side).ceil().max(0.) as u32;
        first..end
    };
    let columns = cells(clip.left(), clip.right(), rx);
    cells(clip.top(), clip.bottom(), ry)
        .flat_map(|row| {
            columns.clone().map(move |column| {
                bounds_at(
                    rx + column as f32 * side,
                    ry + row as f32 * side,
                    side,
                    side,
                )
            })
        })
        .collect()
}

/// The source pixels (x, y, w, h) of a picture `dims` px large, drawn over `rect`, that `clip`
/// shows; None when it shows none.
pub fn visible_crop(
    rect: Bounds<Pixels>,
    clip: Bounds<Pixels>,
    dims: (u32, u32),
) -> Option<(u32, u32, u32, u32)> {
    if dims.0 == 0 || dims.1 == 0 {
        return None;
    }
    let span = |from: Pixels, to: Pixels, start: Pixels, length: Pixels, count: u32| {
        let per = f32::from(length) / count as f32;
        let first = ((f32::from(from - start)) / per).floor().max(0.) as u32;
        let end = ((f32::from(to - start) / per).ceil().max(0.) as u32).min(count);
        (first.min(end), end)
    };
    let (x0, x1) = span(
        clip.left(),
        clip.right(),
        rect.left(),
        rect.size.width,
        dims.0,
    );
    let (y0, y1) = span(
        clip.top(),
        clip.bottom(),
        rect.top(),
        rect.size.height,
        dims.1,
    );
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1 - x0, y1 - y0))
}

/// Below this many device px a source pixel, the GPU's linear filter draws the tiles (R7).
const ENLARGE_MIN_DEVICE_PX: f32 = 2.;

/// The nearest-neighbour factor for `crop`, `per_pixel` logical px a source pixel at device
/// `scale` (R7): `ceil(per_pixel × scale)`, lowered so the enlargement stays within about the
/// pane's device size (one source pixel of slack at each end). None when a source pixel covers
/// under 2 device px, or the factor would drop below 2: the tiles are painted as they are.
pub fn enlarge_factor(
    per_pixel: f32,
    scale: f32,
    crop: (u32, u32, u32, u32),
    pane: Size<f32>,
) -> Option<u32> {
    let device = per_pixel * scale;
    if device < ENLARGE_MIN_DEVICE_PX {
        return None;
    }
    let wanted = device.ceil() as u32;
    let room = |logical: f32| (logical * scale).ceil() as u32 + 2 * wanted;
    let factor = wanted
        .min(room(pane.width) / crop.2.max(1))
        .min(room(pane.height) / crop.3.max(1));
    (factor >= 2).then_some(factor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scrollbar_takes_room_from_the_other_axis() {
        let (area, bar) = (size(400., 300.), 11.);
        assert_eq!(scrollbars(area, size(400., 300.), bar), (false, false));
        assert_eq!(scrollbars(area, size(401., 100.), bar), (true, false));
        assert_eq!(scrollbars(area, size(100., 301.), bar), (false, true));
        // Taller than the pane: the vertical bar leaves too little width for 395 px.
        assert_eq!(scrollbars(area, size(395., 301.), bar), (true, true));
        assert_eq!(scrollbars(area, size(389., 301.), bar), (false, true));
        assert_eq!(scrollbars(area, size(401., 295.), bar), (true, true));
    }

    #[test]
    fn clips_keep_an_inside_rect_and_cut_sideways() {
        let rect = bounds_at(10., 10., 100., 50.);
        assert_eq!(clip_to(rect, bounds_at(0., 0., 400., 300.)), rect);
        assert_eq!(
            clip_to(rect, bounds_at(50., 0., 400., 300.)),
            bounds_at(50., 10., 60., 50.)
        );
        assert_eq!(clip_x(rect, None, Some(35.)), bounds_at(10., 10., 25., 50.));
        assert_eq!(
            clip_x(rect, Some(200.), None),
            bounds_at(200., 10., 0., 50.),
            "nothing left"
        );
    }

    #[test]
    fn the_marker_and_the_checkerboard_follow_the_stage() {
        assert_eq!(
            group_marker(point(px(100.), px(50.)), (16, 32, 16, 8), 2.),
            bounds_at(129., 111., 38., 22.)
        );
        let rect = bounds_at(-100., 0., 1000., 100.);
        let cells = pattern_cells(rect, bounds_at(0., 0., 200., 100.), 64.);
        // Columns 1..5 (from -36 px to 220 px) and rows 0..2.
        assert_eq!(cells.len(), 4 * 2);
        assert_eq!(cells[0], bounds_at(-36., 0., 64., 64.));
    }

    #[test]
    fn an_enlargement_covers_the_shown_pixels_and_about_the_pane() {
        // A 100x100 picture at 8 px a pixel, its stage at (-20, -20) in a 200x100 pane.
        let rect = bounds_at(-20., -20., 800., 800.);
        let pane = bounds_at(0., 0., 200., 100.);
        let crop = visible_crop(rect, clip_to(rect, pane), (100, 100));
        assert_eq!(crop, Some((2, 2, 26, 13)), "every partly shown pixel");
        assert_eq!(
            visible_crop(rect, bounds_at(900., 0., 0., 10.), (100, 100)),
            None
        );
        assert_eq!(visible_crop(rect, pane, (0, 0)), None);
        let pane = size(200., 100.);
        assert_eq!(enlarge_factor(8., 2., (2, 2, 26, 13), pane), Some(16));
        assert_eq!(enlarge_factor(1.0001, 1., (0, 0, 200, 100), pane), None);
        // A crop far wider than the pane cannot take a factor: the tiles are painted.
        assert_eq!(enlarge_factor(8., 1., (0, 0, 400, 1), pane), None);
    }

    #[test]
    fn enlargement_waits_for_two_device_px_and_stays_about_one_pane() {
        // 116% and 150% on a 2560 px pane at scale 1: a source pixel is under 2 device px.
        let pane = size(2560., 1300.);
        assert_eq!(enlarge_factor(1.16, 1., (0, 0, 2209, 1123), pane), None);
        assert_eq!(enlarge_factor(1.5, 1., (0, 0, 1709, 869), pane), None);
        // The same 116% on a 2x display is 2.32 device px: ceil would be 3 (6627 px for a
        // 5120 px pane), the cap gives 2.
        assert_eq!(enlarge_factor(1.16, 2., (0, 0, 2209, 1123), pane), Some(2));
        // Whole device factors keep the full factor.
        assert_eq!(enlarge_factor(2., 1., (0, 0, 1282, 652), pane), Some(2));
        assert_eq!(enlarge_factor(8., 2., (0, 0, 322, 164), pane), Some(16));
        // 300% at scale 1.25 is 3.75 device px: 4 would be 2736 px for a 2560 px pane.
        let narrow = size(2048., 1040.);
        assert_eq!(enlarge_factor(3., 1.25, (0, 0, 684, 349), narrow), Some(3));

        // The output is never more than about the pane's device size.
        for (per_pixel, scale) in [(1.16, 2.), (2., 1.), (2.5, 1.), (3., 1.25), (8., 1.5)] {
            let (w, h) = (2048f32, 1040f32);
            let crop = |logical: f32| (logical / per_pixel).ceil() as u32 + 1;
            let (cw, ch) = (crop(w), crop(h));
            let factor =
                enlarge_factor(per_pixel, scale, (0, 0, cw, ch), size(w, h)).expect("enlarged");
            for (cropped, logical) in [(cw, w), (ch, h)] {
                let device = (logical * scale).ceil() as u32;
                assert!(
                    cropped * factor <= device + 2 * factor,
                    "{per_pixel} at {scale}: {cropped} x {factor} > {device}"
                );
            }
        }
    }

    #[test]
    fn zoom_steps_walk_the_23_presets() {
        assert_eq!(ZOOM_STEPS.len(), 23);
        for (from, up, want) in [
            (1., true, 1.5),
            (1., false, 0.75),
            (0.7, true, 0.75),
            (0.7, false, 0.667),
            (64., true, 64.),
            (0.02, false, 0.02),
            (100., false, 64.),
            (0.01, true, 0.02),
            (1.0005, true, 1.5),
            (0.333, true, 0.5),
        ] {
            assert_eq!(step_zoom(from, up), want, "{from} {up}");
        }
    }

    #[test]
    fn fit_never_enlarges_and_keeps_a_6px_margin() {
        let pane = size(806., 606.);
        assert_eq!(fit_zoom(pane, (400, 300)), Some(1.));
        assert_eq!(fit_zoom(pane, (1600, 1200)), Some(0.5));
        assert_eq!(fit_zoom(pane, (8000, 100)), Some(0.1));
        assert_eq!(fit_zoom(pane, (1_000_000, 1)), Some(0.02), "clamped to 2%");
        assert_eq!(
            fit_zoom(size(21., 500.), (10, 10)),
            None,
            "under 16 px of room"
        );
        assert_eq!(fit_zoom(pane, (0, 0)), None);
    }

    #[test]
    fn ctrl_wheel_zooms_like_localdiff() {
        let notch = wheel_factor(ScrollDelta::Lines(point(0., 3.)));
        assert!((notch - 1.0015f32.powf(99.)).abs() < 1e-5 && notch > 1.159 && notch < 1.161);
        assert!((wheel_factor(ScrollDelta::Lines(point(0., -3.))) * notch - 1.).abs() < 1e-4);
        assert_eq!(
            wheel_factor(ScrollDelta::Pixels(point(px(0.), px(-1000.)))),
            0.5
        );
        assert_eq!(
            wheel_factor(ScrollDelta::Pixels(point(px(0.), px(1000.)))),
            2.
        );
        assert_eq!(
            wheel_scroll(ScrollDelta::Lines(point(0., -3.)), false),
            point(0., 99.)
        );
        assert_eq!(
            wheel_scroll(ScrollDelta::Lines(point(0., -3.)), true),
            point(99., 0.)
        );
    }

    #[test]
    fn the_stage_centres_when_smaller_and_scrolls_when_larger() {
        let pane = size(400., 300.);
        assert_eq!(
            stage_origin(pane, size(100., 50.), point(30., 30.)),
            point(150., 125.),
            "centred"
        );
        assert_eq!(
            stage_origin(pane, size(1000., 50.), point(30., 30.)),
            point(-30., 125.)
        );
        assert_eq!(
            clamp_scroll(pane, size(1000., 800.), point(-5., 900.)),
            point(0., 500.)
        );
        assert_eq!(
            clamp_scroll(pane, size(100., 50.), point(30., 30.)),
            point(0., 0.)
        );
    }

    #[test]
    fn zooming_keeps_the_pixel_under_the_pointer() {
        let pane = size(400., 400.);
        let scroll = zoom_about(point(100., 100.), pane, (1000, 1000), 1., point(0., 0.), 2.);
        assert_eq!(scroll, point(100., 100.));
        assert_eq!(
            image_point(point(100., 100.), pane, (1000, 1000), 2., scroll),
            Some((100, 100))
        );
        assert_eq!(
            zoom_about(point(0., 0.), pane, (100, 100), 1., point(0., 0.), 2.),
            point(0., 0.),
            "a stage that fits stays centred"
        );
    }

    #[test]
    fn a_group_comes_to_the_middle_of_the_pane() {
        assert_eq!(
            center_on((480, 480, 32, 32), size(400., 300.), (2000, 2000), 1.),
            point(296., 346.)
        );
        assert_eq!(
            center_on((0, 0, 16, 16), size(400., 300.), (2000, 2000), 1.),
            point(0., 0.),
            "clamped"
        );
    }

    #[test]
    fn the_pointer_maps_to_picture_pixels_and_the_split() {
        // a 100×100 picture at zoom 2 in a 400×400 pane: a 200×200 stage centred at (100, 100)
        let (pane, dims, zero) = (size(400., 400.), (100, 100), point(0., 0.));
        assert_eq!(
            image_point(point(100., 100.), pane, dims, 2., zero),
            Some((0, 0))
        );
        assert_eq!(
            image_point(point(299.9, 299.9), pane, dims, 2., zero),
            Some((99, 99))
        );
        assert_eq!(image_point(point(300., 150.), pane, dims, 2., zero), None);
        assert_eq!(image_point(point(99., 150.), pane, dims, 2., zero), None);
        assert_eq!(split_at(150., pane, dims, 2., zero), 0.25);
        assert_eq!(
            (
                split_at(0., pane, dims, 2., zero),
                split_at(400., pane, dims, 2., zero)
            ),
            (0., 1.)
        );
        assert_eq!(
            (zoom_label(0.667), zoom_label(0.125), zoom_label(64.)),
            ("67%".into(), "13%".into(), "6400%".into())
        );
    }
}
