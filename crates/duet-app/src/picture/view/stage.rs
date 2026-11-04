//! The stage (R6, R7, R8, R10, R15; `js:364-688`): the panes and what each paints in each mode,
//! one zoom and one scroll for every pane with their scrollbars, panning, the slider, blink
//! and the group marker. `render` works out what every pane paints from the last frame's pane
//! bounds and keeps that list, so what the tests read is what the paint closures draw.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::h_flex;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu};
use gpui_kit::{
    AnyElement, BorderStyle, Bounds, ContentMask, Corners, CursorStyle, Div, FontWeight, Hsla,
    MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point, ScrollWheelEvent, Size, Stateful,
    canvas, div, fill, hsla, point, quad, size, transparent_black,
};

use super::*;
use crate::picture::menu::context_menu;
use crate::picture::prefs::Mode;
use crate::picture::textures::{CHECKER_PAD, EnlargeKey, Enlargements, checker};
use crate::picture::zoom::{
    self, bounds_at, center_on, clamp_scroll, clip_to, clip_x, enlarge_factor, fit_zoom,
    group_marker, pattern_cells, scrollbars, split_at, stage_origin, visible_crop, wheel_factor,
    wheel_scroll, zoom_about,
};
use crate::ui::drag::follow_drag;
use crate::ui::fonts::{BADGE_SIZE, SMALL_SIZE};
use crate::ui::menu::fill as fill_menu;
use crate::ui::scroll::{scroll_for_thumb, thumb};
use crate::ui::style::SCROLLBAR_WIDTH;

/// A scrollbar's thickness, taken from its pane's room.
const BAR: f32 = SCROLLBAR_WIDTH;
/// The checkerboard image's side, in px (LocalDiff's 8 px squares).
const CHECKER: f32 = 64.;
const OPEN_OTHER: &str = "Open the other picture to start comparing.";
const BIG_NO_IMAGE: &str = "A picture is larger than 40 megapixels: no difference image.";
const OPEN_BOTH: &str = "Open both pictures first.";
const NO_GROUPS: &str = "No differences above the tolerance.";
const NO_RESULT: &str = "No comparison yet: press F5 to compare.";

/// Where a picture is drawn: one of the two side-by-side panes, or the one pane of the other
/// modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Left,
    Right,
    Single,
}

impl Pane {
    const ALL: [Pane; 3] = [Pane::Left, Pane::Right, Pane::Single];

    fn index(self) -> usize {
        self as usize
    }

    fn id(self) -> &'static str {
        match self {
            Pane::Left => "picture-pane-left",
            Pane::Right => "picture-pane-right",
            Pane::Single => "picture-pane-single",
        }
    }

    fn bar_id(self, vertical: bool) -> &'static str {
        match (self, vertical) {
            (Pane::Left, true) => "picture-left-vscroll",
            (Pane::Left, false) => "picture-left-hscroll",
            (Pane::Right, true) => "picture-right-vscroll",
            (Pane::Right, false) => "picture-right-hscroll",
            (Pane::Single, true) => "picture-single-vscroll",
            (Pane::Single, false) => "picture-single-hscroll",
        }
    }
}

/// One layer as the last frame painted it, in window px.
#[derive(Debug, Clone, PartialEq)]
pub struct Painted {
    pub pane: Pane,
    pub layer: Layer,
    /// The whole picture on screen.
    pub rect: Bounds<Pixels>,
    /// What was drawn of it.
    pub clip: Bounds<Pixels>,
    /// Drawn from a nearest-neighbour enlargement (R7).
    pub enlarged: bool,
}

#[derive(Debug, Clone, Copy)]
enum Drag {
    Pan {
        from: Point<Pixels>,
        scroll: Point<f32>,
    },
    Split,
    Bar {
        pane: Pane,
        vertical: bool,
        /// Where the thumb is held, in px from its start.
        grab: f32,
    },
}

/// The viewport every pane shares (R8), the slider, blink and the current group, and what the
/// last frame measured and painted.
pub(super) struct Stage {
    zoom: f32,
    fit: bool,
    scroll: Point<f32>,
    split: f32,
    pub(super) group: Option<usize>,
    blink_left: bool,
    blink: Option<Task<()>>,
    /// A render since the last blink tick (R15).
    drawn: bool,
    body: Rc<Cell<Bounds<Pixels>>>,
    /// Each pane's viewport as the last frame laid it out (empty when it was not drawn), and
    /// the scrollbars (horizontal, vertical) beside it then.
    cells: [Rc<Cell<Bounds<Pixels>>>; 3],
    bars: [(bool, bool); 3],
    drag: Option<Drag>,
    painted: Vec<Painted>,
    markers: Vec<(Pane, Bounds<Pixels>)>,
    pub(super) enlargements: Enlargements,
}

impl Default for Stage {
    fn default() -> Self {
        Self {
            zoom: 1.,
            fit: true,
            scroll: point(0., 0.),
            split: 0.5,
            group: None,
            blink_left: true,
            blink: None,
            drawn: false,
            body: Rc::default(),
            cells: Default::default(),
            bars: [(false, false); 3],
            drag: None,
            painted: Vec::new(),
            markers: Vec::new(),
            enlargements: Enlargements::default(),
        }
    }
}

/// A pane laid out at the current zoom.
#[derive(Debug, Clone, Copy)]
struct PaneGeom {
    pane: Pane,
    /// Where the pictures show: the pane less its scrollbars.
    viewport: Bounds<Pixels>,
    /// The scrollbars it needs: (horizontal, vertical).
    bars: (bool, bool),
    /// The stage's top-left.
    origin: Point<Pixels>,
}

/// What a pane's paint closure draws for one layer.
#[derive(Default)]
struct LayerPaint {
    rect: Bounds<Pixels>,
    clip: Bounds<Pixels>,
    checker: Vec<Bounds<Pixels>>,
    images: Vec<(Arc<RenderImage>, Bounds<Pixels>)>,
    enlarged: bool,
}

#[derive(Default)]
struct PanePaint {
    layers: Vec<LayerPaint>,
    marker: Option<Bounds<Pixels>>,
}

fn size_of(bounds: Bounds<Pixels>) -> Size<f32> {
    size(f32::from(bounds.size.width), f32::from(bounds.size.height))
}

fn is_empty(bounds: Bounds<Pixels>) -> bool {
    bounds.size.width <= px(0.) || bounds.size.height <= px(0.)
}

fn bar_px(shown: bool) -> Pixels {
    px(if shown { BAR } else { 0. })
}

/// The layers a pane shows in a mode (`js:518-543`), with the size each covers on the stage.
fn layers_of<'a>(
    sides: &'a [Option<Side>; 2],
    compared: &'a Option<Compared>,
    mode: Mode,
    blink_left: bool,
    pane: Pane,
) -> Vec<(Layer, &'a Tiles, (u32, u32))> {
    let side = |index: usize| {
        let layer = if index == LEFT {
            Layer::Left
        } else {
            Layer::Right
        };
        sides[index]
            .as_ref()
            .map(|s| (layer, &s.tiles, (s.info.width, s.info.height)))
    };
    let shown = match pane {
        Pane::Left => vec![side(LEFT)],
        Pane::Right => vec![side(RIGHT)],
        Pane::Single => match mode {
            Mode::Difference => match compared {
                Some(c) => vec![Some((
                    Layer::Difference,
                    &c.tiles,
                    (c.tiles.width, c.tiles.height),
                ))],
                None => vec![side(LEFT).or_else(|| side(RIGHT))],
            },
            Mode::Blink => vec![match (side(LEFT), side(RIGHT)) {
                (Some(left), Some(right)) => Some(if blink_left { left } else { right }),
                (left, right) => left.or(right),
            }],
            Mode::Slider | Mode::SideBySide => vec![side(LEFT), side(RIGHT)],
        },
    };
    shown.into_iter().flatten().collect()
}

/// The enlargement of the source pixels under `clip` and where it goes (R7).
fn enlargement(
    tiles: &Tiles,
    rect: Bounds<Pixels>,
    clip: Bounds<Pixels>,
    per_pixel: (f32, f32),
    viewport: Bounds<Pixels>,
    scale: f32,
    cache: &mut Enlargements,
) -> Option<(Arc<RenderImage>, Bounds<Pixels>)> {
    let crop = visible_crop(rect, clip, (tiles.width, tiles.height))?;
    let factor = enlarge_factor(per_pixel.0.max(per_pixel.1), scale, crop, size_of(viewport))?;
    let key = EnlargeKey {
        source: tiles.tiles[0].image.id.0,
        crop,
        factor,
    };
    let image = cache.get(key, tiles);
    let (x, y, width, height) = crop;
    let at = bounds_at(
        f32::from(rect.left()) + x as f32 * per_pixel.0,
        f32::from(rect.top()) + y as f32 * per_pixel.1,
        width as f32 * per_pixel.0,
        height as f32 * per_pixel.1,
    );
    Some((image, at))
}

/// What one layer paints: the checkerboard and the picture's tiles, or above 100% (once a
/// source pixel covers 2 device px) the enlargement of what shows.
fn paint_layer(
    tiles: &Tiles,
    rect: Bounds<Pixels>,
    clip: Bounds<Pixels>,
    viewport: Bounds<Pixels>,
    scale: f32,
    cache: &mut Enlargements,
) -> LayerPaint {
    let mut paint = LayerPaint {
        rect,
        clip,
        ..LayerPaint::default()
    };
    if is_empty(clip) || tiles.tiles.is_empty() || tiles.width == 0 || tiles.height == 0 {
        return paint;
    }
    paint.checker = pattern_cells(rect, clip, CHECKER);
    let per_pixel = (
        f32::from(rect.size.width) / tiles.width as f32,
        f32::from(rect.size.height) / tiles.height as f32,
    );
    if per_pixel.0.max(per_pixel.1) > 1.
        && let Some(big) = enlargement(tiles, rect, clip, per_pixel, viewport, scale, cache)
    {
        paint.images.push(big);
        paint.enlarged = true;
        return paint;
    }
    let (rx, ry) = (f32::from(rect.left()), f32::from(rect.top()));
    for tile in &tiles.tiles {
        let at = bounds_at(
            rx + tile.x as f32 * per_pixel.0,
            ry + tile.y as f32 * per_pixel.1,
            tile.width as f32 * per_pixel.0,
            tile.height as f32 * per_pixel.1,
        );
        if at.intersects(&clip) {
            paint.images.push((tile.image.clone(), at));
        }
    }
    paint
}

/// The pane's pictures: per layer the checkerboard, the picture and a 1 px edge. Its prepaint
/// keeps the pane's bounds for the next frame and asks for that frame when they change.
fn layers_canvas(
    cell: Rc<Cell<Bounds<Pixels>>>,
    layers: Vec<LayerPaint>,
    checker: Arc<RenderImage>,
    edge: Hsla,
) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            if cell.replace(bounds) != bounds {
                window.request_animation_frame();
            }
        },
        move |_, (), window, _| {
            for layer in &layers {
                if is_empty(layer.clip) {
                    continue;
                }
                for &at in &layer.checker {
                    let image = checker.clone();
                    let shown = layer.clip.intersect(&at);
                    let whole = at.dilate(px(CHECKER_PAD));
                    let _ = window.paint_image(shown, whole, Corners::default(), image, 0, false);
                }
                for (image, at) in &layer.images {
                    let image = image.clone();
                    let _ =
                        window.paint_image(layer.clip, *at, Corners::default(), image, 0, false);
                }
                let mask = ContentMask {
                    bounds: layer.clip.dilate(px(1.)),
                };
                window.with_content_mask(Some(mask), |window| {
                    window.paint_quad(quad(
                        layer.rect.dilate(px(1.)),
                        px(0.),
                        transparent_black(),
                        px(1.),
                        edge,
                        BorderStyle::Solid,
                    ));
                });
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// The group marker: a 2 px accent border inside R18's dark ring.
fn marker_canvas(marker: Option<Bounds<Pixels>>, accent: Hsla) -> impl IntoElement {
    let ring = hsla(0., 0., 0., 0.55);
    canvas(
        |_, _, _| {},
        move |_, (), window, _| {
            if let Some(at) = marker {
                window.paint_quad(quad(
                    at.dilate(px(1.)),
                    px(3.),
                    transparent_black(),
                    px(1.),
                    ring,
                    BorderStyle::Solid,
                ));
                window.paint_quad(quad(
                    at,
                    px(2.),
                    transparent_black(),
                    px(2.),
                    accent,
                    BorderStyle::Solid,
                ));
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

fn drag_moved(view: &mut PictureView, at: Point<Pixels>, cx: &mut Context<PictureView>) {
    match view.stage.drag {
        Some(Drag::Pan { from, scroll }) => {
            let moved = at - from;
            let to = point(scroll.x - f32::from(moved.x), scroll.y - f32::from(moved.y));
            view.set_scroll(to, cx);
        }
        Some(Drag::Split) => view.split_to(at, cx),
        Some(Drag::Bar {
            pane,
            vertical,
            grab,
        }) => view.bar_to(pane, vertical, grab, at, cx),
        None => {}
    }
}

fn drag_ended(view: &mut PictureView, cx: &mut Context<PictureView>) {
    if view.stage.drag.take().is_some() {
        cx.notify();
    }
}

impl PictureView {
    pub fn mode(&self) -> Mode {
        self.prefs.mode
    }

    /// Shows the pictures another way. The scroll stays (R8), a fitted zoom fits the new pane,
    /// the mode is remembered (R9), and blink starts on the left picture.
    pub fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if self.prefs.mode != mode {
            self.prefs.mode = mode;
            if let Err(err) = self.prefs.save(settings::settings_file(cx).as_deref()) {
                tracing::warn!(%err, "could not remember the picture mode");
            }
            if mode == Mode::Blink {
                self.stage.blink_left = true;
            }
            self.refit();
        }
        cx.notify();
    }

    pub fn zoom(&self) -> f32 {
        self.stage.zoom
    }

    /// The zoom follows the pane: the whole picture, never above 100%.
    pub fn is_fit(&self) -> bool {
        self.stage.fit
    }

    /// How far the stage is scrolled, in px; every pane shares it.
    pub fn scroll(&self) -> Point<f32> {
        self.stage.scroll
    }

    /// Ctrl+0: fits the pictures into the pane, now and whenever it changes.
    pub fn fit(&mut self, cx: &mut Context<Self>) {
        self.stage.fit = true;
        self.refit();
        cx.notify();
    }

    /// Zooms about `anchor` (window px; the picture point under it stays), else about the
    /// middle of the first pane.
    pub fn set_zoom(&mut self, zoom: f32, anchor: Option<Point<Pixels>>, cx: &mut Context<Self>) {
        let to = zoom.clamp(zoom::ZOOM_MIN, zoom::ZOOM_MAX);
        let panes = self.geometry();
        let under = anchor.and_then(|at| {
            panes
                .iter()
                .find(|g| g.viewport.contains(&at))
                .map(|g| (*g, at))
        });
        let about = under.or_else(|| panes.first().map(|g| (*g, g.viewport.center())));
        if let Some((g, at)) = about {
            let local = at - g.viewport.origin;
            self.stage.scroll = zoom_about(
                point(f32::from(local.x), f32::from(local.y)),
                size_of(g.viewport),
                self.stage_dims(),
                self.stage.zoom,
                self.stage.scroll,
                to,
            );
        }
        self.stage.zoom = to;
        self.stage.fit = false;
        cx.notify();
    }

    /// Ctrl+= / Ctrl+−: the next zoom preset up or down.
    pub fn step_zoom(&mut self, up: bool, cx: &mut Context<Self>) {
        self.set_zoom(zoom::step_zoom(self.stage.zoom, up), None, cx);
    }

    /// The slider divider's place across the pictures, 0 to 1.
    pub fn split(&self) -> f32 {
        self.stage.split
    }

    pub fn is_blinking(&self) -> bool {
        self.stage.blink.is_some()
    }

    pub fn blink_shows_left(&self) -> bool {
        self.stage.blink_left
    }

    pub fn current_group(&self) -> Option<usize> {
        self.stage.group
    }

    /// F8 / F7: the next or previous group of differences, wrapping (R10). It is centred in the
    /// pane and every pane marks it.
    pub fn go(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(diff) = self.diff() else {
            if !self.both_ready() {
                self.notice = Some(OPEN_BOTH.into());
            } else if self.too_big() {
                self.notice = Some(NO_DIFFERENCE.into());
            } else if !self.is_busy() {
                // Esc cancelled the compare that would have made one.
                self.notice = Some(NO_RESULT.into());
            }
            cx.notify();
            return;
        };
        let groups = diff.clusters.len();
        if groups == 0 {
            self.notice = Some(NO_GROUPS.into());
            cx.notify();
            return;
        }
        let at = match self.stage.group {
            Some(at) if at < groups => {
                if forward {
                    (at + 1) % groups
                } else {
                    (at + groups - 1) % groups
                }
            }
            _ if forward => 0,
            _ => groups - 1,
        };
        let c = diff.clusters[at];
        self.stage.group = Some(at);
        if let Some(g) = self.geometry().first() {
            self.stage.scroll = center_on(
                (c.x, c.y, c.width, c.height),
                size_of(g.viewport),
                self.stage_dims(),
                self.stage.zoom,
            );
        }
        self.notice = None;
        cx.notify();
    }

    #[doc(hidden)]
    pub fn painted_for_test(&self) -> Vec<Painted> {
        self.stage.painted.clone()
    }

    /// The group marker of each pane, as the last frame drew it.
    #[doc(hidden)]
    pub fn marker_for_test(&self) -> Vec<(Pane, Bounds<Pixels>)> {
        self.stage.markers.clone()
    }

    #[doc(hidden)]
    pub fn message_overlay_for_test(&self) -> Option<String> {
        self.message_overlay().map(String::from)
    }

    #[doc(hidden)]
    pub fn blink_badge_for_test(&self) -> Option<String> {
        self.blink_badge().map(|(text, _)| text)
    }

    #[doc(hidden)]
    pub fn enlargements_built_for_test(&self) -> usize {
        self.stage.enlargements.built()
    }

    /// The mode, zoom and group keys (behind `takes_key`, as every key of the view).
    pub(super) fn stage_actions(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        root.on_action(cx.listener(|this, _: &PictureSideBySide, w, cx| {
            if this.takes_key(w, cx) {
                this.set_mode(Mode::SideBySide, cx)
            }
        }))
        .on_action(cx.listener(|this, _: &PictureDifference, w, cx| {
            if this.takes_key(w, cx) {
                this.set_mode(Mode::Difference, cx)
            }
        }))
        .on_action(cx.listener(|this, _: &PictureSlider, w, cx| {
            if this.takes_key(w, cx) {
                this.set_mode(Mode::Slider, cx)
            }
        }))
        .on_action(cx.listener(|this, _: &PictureBlink, w, cx| {
            if this.takes_key(w, cx) {
                this.set_mode(Mode::Blink, cx)
            }
        }))
        .on_action(cx.listener(|this, _: &PictureFit, w, cx| {
            if this.takes_key(w, cx) {
                this.fit(cx)
            }
        }))
        .on_action(cx.listener(|this, _: &PictureActualSize, w, cx| {
            if this.takes_key(w, cx) {
                this.set_zoom(1., None, cx)
            }
        }))
        .on_action(cx.listener(|this, _: &PictureZoomIn, w, cx| {
            if this.takes_key(w, cx) {
                this.step_zoom(true, cx)
            }
        }))
        .on_action(cx.listener(|this, _: &PictureZoomOut, w, cx| {
            if this.takes_key(w, cx) {
                this.step_zoom(false, cx)
            }
        }))
        .on_action(cx.listener(|this, _: &PictureNext, w, cx| {
            if this.takes_key(w, cx) {
                this.go(true, cx)
            }
        }))
        .on_action(cx.listener(|this, _: &PicturePrev, w, cx| {
            if this.takes_key(w, cx) {
                this.go(false, cx)
            }
        }))
    }

    /// The larger width and height of the loaded sides (LocalDiff `dims`).
    fn stage_dims(&self) -> (u32, u32) {
        self.sides.iter().flatten().fold((0, 0), |(w, h), side| {
            (w.max(side.info.width), h.max(side.info.height))
        })
    }

    fn stage_size(&self) -> Size<f32> {
        let (w, h) = self.stage_dims();
        size(w as f32 * self.stage.zoom, h as f32 * self.stage.zoom)
    }

    fn mode_panes(&self) -> &'static [Pane] {
        match self.prefs.mode {
            Mode::SideBySide => &[Pane::Left, Pane::Right],
            _ => &[Pane::Single],
        }
    }

    fn both_ready(&self) -> bool {
        self.sides.iter().all(Option::is_some)
    }

    fn slider_active(&self) -> bool {
        self.prefs.mode == Mode::Slider && self.both_ready()
    }

    /// A pane's room, its scrollbars included, as the last frame laid it out. A pane that
    /// frame did not draw gets its share of the body.
    fn area(&self, pane: Pane) -> Option<Bounds<Pixels>> {
        let seen = self.stage.cells[pane.index()].get();
        if !is_empty(seen) {
            let (horizontal, vertical) = self.stage.bars[pane.index()];
            let room = size(
                seen.size.width + bar_px(vertical),
                seen.size.height + bar_px(horizontal),
            );
            return Some(Bounds::new(seen.origin, room));
        }
        let body = self.stage.body.get();
        if is_empty(body) {
            return None;
        }
        let half = (body.size.width - px(1.)) / 2.;
        let height = body.size.height;
        Some(match pane {
            Pane::Single => body,
            Pane::Left => Bounds::new(body.origin, size(half, height)),
            Pane::Right => Bounds::new(point(body.right() - half, body.top()), size(half, height)),
        })
    }

    /// The mode's panes that have room, laid out at the current zoom and scroll.
    fn geometry(&self) -> Vec<PaneGeom> {
        let stage = self.stage_size();
        self.mode_panes()
            .iter()
            .filter_map(|&pane| {
                let area = self.area(pane)?;
                let bars = scrollbars(size_of(area), stage, BAR);
                let viewport = Bounds::new(
                    area.origin,
                    size(
                        area.size.width - bar_px(bars.1),
                        area.size.height - bar_px(bars.0),
                    ),
                );
                let at = stage_origin(size_of(viewport), stage, self.stage.scroll);
                Some(PaneGeom {
                    pane,
                    viewport,
                    bars,
                    origin: viewport.origin + point(px(at.x), px(at.y)),
                })
            })
            .collect()
    }

    /// The scroll limited to what the first pane can show of the stage.
    fn clamped(&self, scroll: Point<f32>) -> Point<f32> {
        match self.geometry().first() {
            Some(g) => clamp_scroll(size_of(g.viewport), self.stage_size(), scroll),
            None => scroll,
        }
    }

    fn set_scroll(&mut self, scroll: Point<f32>, cx: &mut Context<Self>) {
        self.stage.scroll = self.clamped(scroll);
        cx.notify();
    }

    /// While fitting, the zoom that fits the first pane now (R8).
    fn refit(&mut self) {
        if !self.stage.fit {
            return;
        }
        let fitted = self
            .area(self.mode_panes()[0])
            .and_then(|area| fit_zoom(size_of(area), self.stage_dims()));
        if let Some(zoom) = fitted {
            self.stage.zoom = zoom;
        }
    }

    /// The pointer moved over a pane: the probe reads the picture pixel under it, if any.
    fn probe_at(&mut self, pane: Pane, at: Point<Pixels>, cx: &mut Context<Self>) {
        let found = self
            .geometry()
            .into_iter()
            .find(|g| g.pane == pane)
            .and_then(|g| {
                let local = at - g.viewport.origin;
                zoom::image_point(
                    point(f32::from(local.x), f32::from(local.y)),
                    size_of(g.viewport),
                    self.stage_dims(),
                    self.stage.zoom,
                    self.stage.scroll,
                )
            });
        if self.probe != found {
            self.probe = found;
            cx.notify();
        }
    }

    fn on_wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if self.stage_dims().0 == 0 {
            return;
        }
        let modifiers = event.modifiers;
        if modifiers.control || modifiers.platform {
            let zoom = self.stage.zoom * wheel_factor(event.delta);
            self.set_zoom(zoom, Some(event.position), cx);
        } else {
            let by = wheel_scroll(event.delta, modifiers.shift);
            self.set_scroll(self.stage.scroll + by, cx);
        }
    }

    /// The left button went down in a pane: the slider's divider follows it in Slider mode with
    /// both pictures, anything else pans.
    fn press_pane(&mut self, pane: Pane, at: Point<Pixels>, cx: &mut Context<Self>) {
        if self.sides.iter().all(Option::is_none) {
            return;
        }
        if pane == Pane::Single && self.slider_active() {
            self.stage.drag = Some(Drag::Split);
            self.split_to(at, cx);
        } else {
            self.stage.drag = Some(Drag::Pan {
                from: at,
                scroll: self.stage.scroll,
            });
        }
        cx.notify();
    }

    fn split_to(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(g) = self.geometry().into_iter().find(|g| g.pane == Pane::Single) else {
            return;
        };
        self.stage.split = split_at(
            f32::from(at.x - g.viewport.left()),
            size_of(g.viewport),
            self.stage_dims(),
            self.stage.zoom,
            self.stage.scroll,
        );
        cx.notify();
    }

    /// A scrollbar's track (start and length along it) and what it maps: the stage's px, the
    /// px shown and the px scrolled.
    fn bar_track(&self, g: &PaneGeom, vertical: bool) -> (f32, f32, u64, u64, u64) {
        let stage = self.stage_size();
        let (start, length, total, at) = if vertical {
            (
                g.viewport.top(),
                g.viewport.size.height,
                stage.height,
                self.stage.scroll.y,
            )
        } else {
            (
                g.viewport.left(),
                g.viewport.size.width,
                stage.width,
                self.stage.scroll.x,
            )
        };
        let length = f32::from(length);
        (
            f32::from(start),
            length,
            total.ceil() as u64,
            length.floor() as u64,
            at.round() as u64,
        )
    }

    /// The left button went down on a scrollbar: on the thumb, it is held by that point; on
    /// the track, the thumb's middle jumps there.
    fn press_bar(&mut self, pane: Pane, vertical: bool, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(g) = self.geometry().into_iter().find(|g| g.pane == pane) else {
            return;
        };
        let (start, length, total, shown, scrolled) = self.bar_track(&g, vertical);
        if total <= shown {
            return;
        }
        let along = f32::from(if vertical { at.y } else { at.x }) - start;
        let (thumb_at, thumb_length) = thumb(length, total, shown, scrolled);
        let on_thumb = (thumb_at..=thumb_at + thumb_length).contains(&along);
        let grab = if on_thumb {
            along - thumb_at
        } else {
            thumb_length / 2.
        };
        self.stage.drag = Some(Drag::Bar {
            pane,
            vertical,
            grab,
        });
        if !on_thumb {
            self.bar_to(pane, vertical, grab, at, cx);
        }
        cx.notify();
    }

    /// Scrolls so the held thumb's grabbed point is under `at`.
    fn bar_to(
        &mut self,
        pane: Pane,
        vertical: bool,
        grab: f32,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(g) = self.geometry().into_iter().find(|g| g.pane == pane) else {
            return;
        };
        let (start, length, total, shown, _) = self.bar_track(&g, vertical);
        let along = f32::from(if vertical { at.y } else { at.x }) - start - grab;
        let to = scroll_for_thumb(length, total, shown, along) as f32;
        let mut scroll = self.stage.scroll;
        if vertical {
            scroll.y = to;
        } else {
            scroll.x = to;
        }
        self.set_scroll(scroll, cx);
    }

    /// Blink runs while the tab is drawn (R15): each render notes it was drawn and starts the
    /// timer if it is not running.
    fn sync_blink(&mut self, cx: &mut Context<Self>) {
        if self.prefs.mode != Mode::Blink || !self.both_ready() {
            self.stage.blink = None;
            return;
        }
        self.stage.drawn = true;
        if self.stage.blink.is_some() {
            return;
        }
        self.stage.blink = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK_INTERVAL).await;
                let ticking = this
                    .update(cx, |view, cx| view.blink_tick(cx))
                    .unwrap_or(false);
                if !ticking {
                    break;
                }
            }
        }));
    }

    /// One blink: the other picture shows. A tick that finds no render since the last one
    /// stops the timer; the next render starts it again.
    fn blink_tick(&mut self, cx: &mut Context<Self>) -> bool {
        if !std::mem::take(&mut self.stage.drawn) {
            // This tick runs inside the task: it ends by itself.
            if let Some(task) = self.stage.blink.take() {
                task.detach();
            }
            return false;
        }
        self.stage.blink_left = !self.stage.blink_left;
        cx.notify();
        true
    }

    /// The word over the single pane, in the modes that compare in one pane: that a side
    /// loads, else that one is missing or failed (I-55, C24), else why there is no image.
    fn message_overlay(&self) -> Option<SharedString> {
        if self.prefs.mode == Mode::SideBySide {
            return None;
        }
        if let Some(loading) = self.loading_text() {
            return Some(loading.into());
        }
        if !self.both_ready() {
            return Some(OPEN_OTHER.into());
        }
        (self.prefs.mode == Mode::Difference && self.too_big()).then(|| {
            if self.sides.iter().flatten().any(|side| side.info.big) {
                BIG_NO_IMAGE.into()
            } else {
                NO_DIFFERENCE.into()
            }
        })
    }

    /// Blink's badge and whether it names the right picture.
    fn blink_badge(&self) -> Option<(String, bool)> {
        if self.prefs.mode != Mode::Blink || !self.both_ready() {
            return None;
        }
        let left = self.stage.blink_left;
        let name = &self.side(left)?.name;
        let label = if left { "Left" } else { "Right" };
        Some((format!("{label}: {name}"), !left))
    }

    /// A side pane's word while its picture is not there: why it failed, or that it loads.
    fn placeholder_text(&self, left: bool) -> Option<(String, bool)> {
        let index = side_index(left);
        if self.sides[index].is_some() {
            return None;
        }
        if let Some(message) = &self.messages[index] {
            return Some((message.to_string(), true));
        }
        self.loads[index]
            .is_running()
            .then(|| (format!("Loading {}…", file_name(&self.paths[index])), false))
    }

    /// What every pane paints this frame, from `geoms`; keeps the list for the tests and hands
    /// the enlargements no pane used to the retired list.
    fn frame_paint(&mut self, geoms: &[PaneGeom], scale: f32) -> Vec<PanePaint> {
        let zoom = self.stage.zoom;
        let split = self
            .slider_active()
            .then(|| self.stage.split * self.stage_dims().0 as f32 * zoom);
        let group = self
            .stage
            .group
            .and_then(|at| self.diff()?.clusters.get(at).copied());
        let (mode, blink_left) = (self.prefs.mode, self.stage.blink_left);
        let (mut painted, mut markers) = (Vec::new(), Vec::new());
        let mut out = Vec::with_capacity(geoms.len());
        for g in geoms {
            let mut layers = Vec::new();
            for (layer, tiles, dims) in
                layers_of(&self.sides, &self.compared, mode, blink_left, g.pane)
            {
                let rect = Bounds::new(
                    g.origin,
                    size(px(dims.0 as f32 * zoom), px(dims.1 as f32 * zoom)),
                );
                let mut clip = clip_to(rect, g.viewport);
                if let Some(x) = split {
                    let at = f32::from(g.origin.x) + x;
                    clip = match layer {
                        Layer::Left => clip_x(clip, None, Some(at)),
                        Layer::Right => clip_x(clip, Some(at), None),
                        Layer::Difference => clip,
                    };
                }
                let paint = paint_layer(
                    tiles,
                    rect,
                    clip,
                    g.viewport,
                    scale,
                    &mut self.stage.enlargements,
                );
                painted.push(Painted {
                    pane: g.pane,
                    layer,
                    rect,
                    clip,
                    enlarged: paint.enlarged,
                });
                layers.push(paint);
            }
            let marker = group.map(|c| group_marker(g.origin, (c.x, c.y, c.width, c.height), zoom));
            markers.extend(marker.map(|at| (g.pane, at)));
            out.push(PanePaint { layers, marker });
        }
        let unused = self.stage.enlargements.finish();
        self.retired.extend(unused);
        self.stage.painted = painted;
        self.stage.markers = markers;
        out
    }

    /// The bench hook: once the tab's first diff is applied and drawn, logs how long it took
    /// from the job's start (both sides decoded) to its result applied, tiles included, then
    /// with `DUET_EXIT_AFTER_FIRST_ROWS` quits on the next frame, as the hex rows do.
    fn log_first_diff(&mut self, window: &mut Window) {
        let Some(ms) = self.first_diff_ms else {
            return;
        };
        if self.first_diff_logged || self.stage.painted.is_empty() {
            return;
        }
        self.first_diff_logged = true;
        window.on_next_frame(move |_, cx| {
            tracing::info!(ms, "picture diff");
            if std::env::var_os("DUET_EXIT_AFTER_FIRST_ROWS").is_some() {
                cx.quit();
            }
        });
    }

    /// The body: the mode's panes, then the blink badge and the message over them.
    pub(super) fn render_stage(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let panes = self.mode_panes();
        for pane in Pane::ALL {
            if !panes.contains(&pane) {
                self.stage.cells[pane.index()].set(Bounds::default());
                self.stage.bars[pane.index()] = (false, false);
            }
        }
        self.refit();
        self.stage.scroll = self.clamped(self.stage.scroll);
        let geoms = self.geometry();
        for g in &geoms {
            self.stage.bars[g.pane.index()] = g.bars;
        }
        let paints = self.frame_paint(&geoms, window.scale_factor());
        self.sync_blink(cx);
        self.log_first_diff(window);

        let mut parts: Vec<_> = geoms.into_iter().zip(paints).collect();
        let boxes: Vec<AnyElement> = panes
            .iter()
            .map(|&pane| {
                let part = parts
                    .iter()
                    .position(|(g, _)| g.pane == pane)
                    .map(|at| parts.remove(at));
                self.pane_box(pane, part, p, cx)
            })
            .collect();
        let row = if self.prefs.mode == Mode::SideBySide {
            h_flex()
                .size_full()
                .gap(px(1.))
                .bg(p.line)
                .children(boxes)
                .into_any_element()
        } else {
            div().size_full().children(boxes).into_any_element()
        };
        let body = self.stage.body.clone();
        let dragging = self.stage.drag.is_some().then(|| cx.entity().downgrade());
        div()
            .id("picture-body")
            .test_support()
            // Leaving the panes clears the probe.
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered && this.probe.take().is_some() {
                    cx.notify();
                }
            }))
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .bg(p.bg)
            .child(
                canvas(
                    move |bounds, window, _| {
                        if body.replace(bounds) != bounds {
                            window.request_animation_frame();
                        }
                    },
                    // A held pan, divider or scrollbar follows the pointer across the window.
                    move |_, (), window, _| {
                        if let Some(view) = dragging {
                            follow_drag(view, window, drag_moved, drag_ended);
                        }
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(row)
            .children(self.blink_badge_element(p))
            .children(self.message_element(p))
            .child(self.drop_zones(p, cx))
            .context_menu({
                let (view, focus) = (cx.entity().downgrade(), self.focus.clone());
                move |menu, _, cx: &mut Context<PopupMenu>| {
                    let entries = view
                        .upgrade()
                        .map(|v| context_menu(v.read(cx)))
                        .unwrap_or_default();
                    fill_menu(menu, entries, Some(focus.clone()))
                }
            })
            .into_any_element()
    }

    /// A pane with its scrollbars on the axes that overflow.
    fn pane_box(
        &self,
        pane: Pane,
        part: Option<(PaneGeom, PanePaint)>,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let geom = part.as_ref().map(|(g, _)| *g);
        let (horizontal, vertical) = geom.map_or((false, false), |g| g.bars);
        let vbar = geom
            .filter(|_| vertical)
            .map(|g| self.scrollbar(&g, true, p, cx));
        let hbar = geom
            .filter(|_| horizontal)
            .map(|g| self.scrollbar(&g, false, p, cx));
        let column = v_flex().min_w_0().bg(p.bg);
        let column = if pane == Pane::Single {
            column.size_full()
        } else {
            column.flex_1().h_full()
        };
        column
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.viewport_element(pane, part, p, cx))
                    .children(vbar),
            )
            .children(hbar.map(|bar| {
                h_flex()
                    .flex_none()
                    .w_full()
                    .h(px(BAR))
                    .child(bar)
                    .when(vertical, |row| {
                        row.child(div().flex_none().w(px(BAR)).h_full().bg(p.panel))
                    })
            }))
            .into_any_element()
    }

    /// Where a pane's pictures show: the wheel zooms or scrolls, a left drag pans or moves the
    /// divider.
    fn viewport_element(
        &self,
        pane: Pane,
        part: Option<(PaneGeom, PanePaint)>,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (geom, paint) = part.unzip();
        let paint = paint.unwrap_or_default();
        let cursor = if matches!(self.stage.drag, Some(Drag::Pan { .. })) {
            CursorStyle::ClosedHand
        } else if pane == Pane::Single && self.slider_active() {
            CursorStyle::ResizeLeftRight
        } else {
            CursorStyle::OpenHand
        };
        div()
            .id(pane.id())
            .test_support()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .cursor(cursor)
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| this.on_wheel(e, cx)))
            .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                this.probe_at(pane, e.position, cx)
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    this.press_pane(pane, e.position, cx)
                }),
            )
            // A quick click's release ends the drag here, before a frame has drawn the
            // window-wide listeners.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| drag_ended(this, cx)),
            )
            .child(layers_canvas(
                self.stage.cells[pane.index()].clone(),
                paint.layers,
                checker(p.dark),
                p.line,
            ))
            .children(geom.and_then(|g| self.divider(&g, p)))
            .child(marker_canvas(paint.marker, p.accent))
            .children(self.placeholder(pane, p))
            .into_any_element()
    }

    /// One scrollbar of a pane, as the hex view's: the thumb maps the scroll over the stage.
    fn scrollbar(
        &self,
        g: &PaneGeom,
        vertical: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (_, _, total, shown, at) = self.bar_track(g, vertical);
        let (pane, track, thumb_color) = (g.pane, p.panel, p.line);
        let bar = div().id(pane.bar_id(vertical)).test_support();
        let bar = if vertical {
            bar.flex_none().w(px(BAR)).h_full()
        } else {
            bar.flex_1().min_w_0().h_full()
        };
        bar.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                this.press_bar(pane, vertical, e.position, cx)
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| drag_ended(this, cx)),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| drag_ended(this, cx)),
        )
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    window.paint_quad(fill(bounds, track));
                    let along = if vertical {
                        bounds.size.height
                    } else {
                        bounds.size.width
                    };
                    let (start, length) = thumb(f32::from(along), total, shown, at);
                    let thumb_bounds = if vertical {
                        Bounds::new(
                            point(bounds.left() + px(3.), bounds.top() + px(start)),
                            size(px(5.), px(length)),
                        )
                    } else {
                        Bounds::new(
                            point(bounds.left() + px(start), bounds.top() + px(3.)),
                            size(px(length), px(5.)),
                        )
                    };
                    window.paint_quad(quad(
                        thumb_bounds,
                        px(3.),
                        thumb_color,
                        px(0.),
                        transparent_black(),
                        BorderStyle::Solid,
                    ));
                },
            )
            .size_full(),
        )
        .into_any_element()
    }

    /// The slider's divider (`picture-split`) with its two tags, across the stage.
    fn divider(&self, g: &PaneGeom, p: &Palette) -> Option<AnyElement> {
        if g.pane != Pane::Single || !self.slider_active() {
            return None;
        }
        let (width, height) = self.stage_dims();
        let zoom = self.stage.zoom;
        let x = f32::from(g.origin.x - g.viewport.left()) + self.stage.split * width as f32 * zoom;
        let top = f32::from(g.origin.y - g.viewport.top());
        let tag = |text: &'static str| {
            div()
                .absolute()
                .top(px(6.))
                .whitespace_nowrap()
                .px(px(7.))
                .rounded_full()
                .bg(p.panel3)
                .border_1()
                .border_color(p.accent)
                .text_color(p.text)
                .text_size(px(BADGE_SIZE))
                .child(text)
        };
        Some(
            div()
                .id("picture-split")
                .test_support()
                .absolute()
                .left(px(x - 1.))
                .top(px(top))
                .w(px(2.))
                .h(px(height as f32 * zoom))
                .bg(p.accent)
                .child(tag("◀ Left").right(px(8.)))
                .child(tag("Right ▶").left(px(8.)))
                .into_any_element(),
        )
    }

    /// A side pane's "Loading {name}…" or the reason its picture did not open.
    fn placeholder(&self, pane: Pane, p: &Palette) -> Option<AnyElement> {
        let (left, id) = match pane {
            Pane::Left => (true, "picture-left-placeholder"),
            Pane::Right => (false, "picture-right-placeholder"),
            Pane::Single => return None,
        };
        let (text, failed) = self.placeholder_text(left)?;
        Some(
            div()
                .id(id)
                .test_support()
                .aria_label(text.clone())
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(px(20.))
                .text_size(px(SMALL_SIZE))
                .text_color(if failed { p.del } else { p.dim })
                .child(text)
                .into_any_element(),
        )
    }

    fn blink_badge_element(&self, p: &Palette) -> Option<AnyElement> {
        let (text, right) = self.blink_badge()?;
        Some(
            h_flex()
                .absolute()
                .top(px(10.))
                .left_0()
                .w_full()
                .justify_center()
                .child(
                    div()
                        .id("picture-blink-badge")
                        .test_support()
                        .aria_label(text.clone())
                        .px(px(12.))
                        .py(px(2.))
                        .rounded_full()
                        .bg(p.panel3)
                        .border_1()
                        .border_color(if right { p.moved } else { p.accent })
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(SMALL_SIZE))
                        .whitespace_nowrap()
                        .child(text),
                )
                .into_any_element(),
        )
    }

    fn message_element(&self, p: &Palette) -> Option<AnyElement> {
        let text = self.message_overlay()?;
        Some(
            h_flex()
                .absolute()
                .bottom(px(12.))
                .left_0()
                .w_full()
                .justify_center()
                .child(
                    div()
                        .id("picture-message")
                        .test_support()
                        .aria_label(text.clone())
                        .px(px(12.))
                        .py(px(5.))
                        .rounded(px(8.))
                        .bg(p.panel3)
                        .border_1()
                        .border_color(p.line)
                        .text_color(p.muted)
                        .text_size(px(SMALL_SIZE))
                        .child(text),
                )
                .into_any_element(),
        )
    }
}
