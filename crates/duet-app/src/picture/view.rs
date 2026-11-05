//! The picture compare tab (R2, R9, R10, R11, R14, R20): both pictures are read, decoded and
//! compared off the UI thread, and the stage (`view/stage.rs`) shows them in four modes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use duet_core::cancel::{CancelToken, Cancelled};
use duet_core::picture::Picture;
use duet_core::picture::decode::{DecodeError, Limits, decode, read};
use duet_core::picture::diff::{PictureDiff, compare, diff_image};
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::v_flex;
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Focusable, KeyBinding, RenderImage, SharedString,
    Task, Window, actions, prelude::*, px,
};

use crate::i18n::errors::decode_error;
use crate::i18n::{LiveText, tr, tr_key, trf};
use crate::job::JobSlot;
use crate::palette::Palette;
use crate::pathbar::ActiveEdit;
use crate::picture::menu::PictureExport;
use crate::picture::prefs::PicturePrefs;
use crate::picture::textures::{Tiles, images, to_tiles};
use crate::settings;
use crate::tabs::badge_text;
use crate::ui::fonts::{Fonts, UI_SIZE};

mod export;
mod open;
mod stage;
mod status;
mod toolbar;

pub use stage::{Painted, Pane};
pub use status::HeadText;

actions!(
    picture,
    [
        PictureEscape,
        PictureReload,
        PictureSwap,
        PictureSideBySide,
        PictureDifference,
        PictureSlider,
        PictureBlink,
        PictureFit,
        PictureActualSize,
        PictureZoomIn,
        PictureZoomOut,
        PictureNext,
        PicturePrev,
        PictureOpenLeft,
        PictureOpenRight
    ]
);

pub const CONTEXT: &str = "PictureView";
/// The blink mode shows the other picture this often.
pub const BLINK_INTERVAL: Duration = Duration::from_millis(600);

const CANCELLED: &str = tr_key("Cancelled: the result is incomplete.");
const RELOADED: &str = tr_key("Pictures reloaded.");
const NOTHING_TO_RELOAD: &str = tr_key("No pictures to reload.");
/// A picture over the pixel limit, or a pair too large to hold a difference image (R10).
const NO_DIFFERENCE: &str = tr_key("No difference can be computed for these pictures.");

pub fn key_bindings() -> Vec<KeyBinding> {
    let c = Some(CONTEXT);
    vec![
        KeyBinding::new("escape", PictureEscape, c),
        KeyBinding::new("f5", PictureReload, c),
        KeyBinding::new("f9", PictureSwap, c),
        KeyBinding::new("1", PictureSideBySide, c),
        KeyBinding::new("2", PictureDifference, c),
        KeyBinding::new("3", PictureSlider, c),
        KeyBinding::new("4", PictureBlink, c),
        KeyBinding::new("ctrl-0", PictureFit, c),
        KeyBinding::new("ctrl-1", PictureActualSize, c),
        KeyBinding::new("ctrl-=", PictureZoomIn, c),
        KeyBinding::new("ctrl-+", PictureZoomIn, c),
        KeyBinding::new("ctrl--", PictureZoomOut, c),
        KeyBinding::new("f8", PictureNext, c),
        KeyBinding::new("f7", PicturePrev, c),
        KeyBinding::new("ctrl-o", PictureOpenLeft, c),
        KeyBinding::new("ctrl-shift-o", PictureOpenRight, c),
    ]
}

pub enum PictureEvent {
    /// Escape with nothing to cancel: back to the folder tab this one was opened from.
    Back,
    Swapped {
        left: PathBuf,
        right: PathBuf,
    },
    Open {
        left: PathBuf,
        right: PathBuf,
    },
}

/// What is known about one loaded side.
#[derive(Debug, Clone)]
pub struct SideInfo {
    pub name: String,
    /// None for a picture that has no file.
    pub path: Option<PathBuf>,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub format: String,
    pub width: u32,
    pub height: u32,
    /// Over the pixel limit: only a preview is held, and the pair is not compared.
    pub big: bool,
}

/// One loaded side: its facts and what is held of its pixels.
struct Side {
    info: SideInfo,
    /// None when the picture is over the pixel limit.
    pixels: Option<Arc<Picture>>,
    /// Some exactly when `pixels` is None.
    preview: Option<Arc<Picture>>,
    /// The shown pixels (the preview's, for a big picture) as gpui images.
    tiles: Tiles,
}

/// A layer of the stage that is made of tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Left,
    Right,
    Difference,
}

/// A finished compare and the pictures and tolerance it was made from, so what is shown (and
/// later exported) never mixes a new tolerance with an old result.
pub struct Compared {
    pub diff: Arc<PictureDiff>,
    /// The difference image as gpui images; export rebuilds the RGBA image from `sides`.
    pub tiles: Tiles,
    pub sides: [Arc<Picture>; 2],
    pub tolerance: u8,
}

enum LoadError {
    Cancelled,
    Message(LiveText),
}

const LEFT: usize = 0;
const RIGHT: usize = 1;

fn side_index(left: bool) -> usize {
    if left { LEFT } else { RIGHT }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Reads and decodes one side. Runs on the background executor.
fn load_side(path: &Path, limits: &Limits, cancel: &CancelToken) -> Result<Side, LoadError> {
    let name = file_name(path);
    let fail = |err: DecodeError| match err {
        DecodeError::Cancelled => LoadError::Cancelled,
        err => {
            // Built from data, so a switch of language says it again (`LiveText::relabel`).
            let name = name.clone();
            LoadError::Message(LiveText::new(move || {
                trf("Can't open {}: {}", &[&name, &decode_error(&err)])
            }))
        }
    };
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let bytes = read(path, limits).map_err(&fail)?;
    let decoded = decode(&bytes, &name, limits, cancel).map_err(&fail)?;
    let info = SideInfo {
        name: name.clone(),
        path: Some(path.to_path_buf()),
        size: bytes.len() as u64,
        modified,
        format: decoded.format.clone(),
        width: decoded.width,
        height: decoded.height,
        big: decoded.big(),
    };
    let pixels = decoded.pixels.map(Arc::new);
    let preview = decoded.preview.map(Arc::new);
    let shown = pixels.as_ref().or(preview.as_ref());
    let tiles = shown.map_or_else(
        || to_tiles(&Picture::new(0, 0, Vec::new())),
        |picture| to_tiles(picture),
    );
    Ok(Side {
        info,
        pixels,
        preview,
        tiles,
    })
}

pub struct PictureView {
    paths: [PathBuf; 2],
    sides: [Option<Side>; 2],
    /// Why a side could not be opened.
    messages: [Option<LiveText>; 2],
    limits: Limits,
    /// One load per side, and the id of the load each side waits for (0 for none). A result
    /// lands on the side that holds its id, so a swap during a load moves the pending load with
    /// its side (R20); the slots and tasks swap along.
    loads: [JobSlot; 2],
    load_ids: [u64; 2],
    load_tasks: [Option<Task<()>>; 2],
    next_load_id: u64,
    /// Waiting for the loads of an F5.
    reloading: bool,
    diff_job: JobSlot,
    diff_task: Option<Task<()>>,
    /// When the running compare started, and how long the tab's first one took to land (the
    /// bench hook logs it once drawn).
    diff_started: Instant,
    first_diff_ms: Option<u64>,
    first_diff_logged: bool,
    /// The result shown; it stays until a newer one replaces it or the pair cannot be compared.
    compared: Option<Compared>,
    prefs: PicturePrefs,
    tolerance: toolbar::ToleranceControls,
    /// The picture pixel under the pointer (R9's probe).
    probe: Option<(u32, u32)>,
    notice: Option<SharedString>,
    /// A notice for the pair the workspace is about to load: `load_paths` clears the notice,
    /// then shows this one. A switch of language drops it, like any notice.
    carried: Option<SharedString>,
    /// The head whose name is being edited as a path.
    path_edit: Option<ActiveEdit>,
    /// Replaced images, dropped from the GPU atlas by the next `render` (R6).
    retired: Vec<Arc<RenderImage>>,
    /// How many times the tab was told to give its images back, and which images that gave back.
    released: usize,
    released_ids: Vec<usize>,
    /// How many retired images the frames dropped.
    dropped: usize,
    stage: stage::Stage,
    focus: FocusHandle,
}

impl EventEmitter<PictureEvent> for PictureView {}

impl Focusable for PictureView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PictureView {
    pub fn new(left: PathBuf, right: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_limits(left, right, Limits::default(), window, cx)
    }

    #[doc(hidden)]
    pub fn with_limits(
        left: PathBuf,
        right: PathBuf,
        limits: Limits,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::blank(left, right, limits, window, cx);
        view.start_side(LEFT, false, cx);
        view.start_side(RIGHT, false, cx);
        view
    }

    /// Two decoded sides with no file (LocalDiff `_setDecoded`).
    #[doc(hidden)]
    pub fn with_pictures_for_test(
        left: Picture,
        right: Picture,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::blank(
            PathBuf::new(),
            PathBuf::new(),
            Limits::default(),
            window,
            cx,
        );
        for (index, picture) in [left, right].into_iter().enumerate() {
            let info = SideInfo {
                name: if index == LEFT {
                    "left.png"
                } else {
                    "right.png"
                }
                .to_owned(),
                path: None,
                size: picture.rgba.len() as u64,
                modified: None,
                format: "RGBA".to_owned(),
                width: picture.width,
                height: picture.height,
                big: false,
            };
            let tiles = to_tiles(&picture);
            view.sides[index] = Some(Side {
                info,
                pixels: Some(Arc::new(picture)),
                preview: None,
                tiles,
            });
        }
        view.compare_now(cx);
        view
    }

    fn blank(
        left: PathBuf,
        right: PathBuf,
        limits: Limits,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let prefs = PicturePrefs::load(settings::settings_file(cx).as_deref());
        let tolerance = toolbar::ToleranceControls::new(prefs.tolerance, window, cx);
        Self {
            paths: [left, right],
            sides: [None, None],
            messages: [None, None],
            limits,
            loads: [JobSlot::default(), JobSlot::default()],
            load_ids: [0, 0],
            load_tasks: [None, None],
            next_load_id: 0,
            reloading: false,
            diff_job: JobSlot::default(),
            diff_task: None,
            diff_started: Instant::now(),
            first_diff_ms: None,
            first_diff_logged: false,
            compared: None,
            prefs,
            tolerance,
            probe: None,
            notice: None,
            carried: None,
            path_edit: None,
            retired: Vec::new(),
            released: 0,
            released_ids: Vec::new(),
            dropped: 0,
            stage: stage::Stage::default(),
            // A tab stop, so Tab comes back to the view after a button or field.
            focus: cx.focus_handle().tab_stop(true),
        }
    }

    /// A side decoding or a difference being computed.
    pub fn is_busy(&self) -> bool {
        self.loading(true)
            || self.loading(false)
            || self.diff_job.is_running()
            || self.tolerance.pending
    }

    pub fn side(&self, left: bool) -> Option<&SideInfo> {
        self.sides[side_index(left)].as_ref().map(|s| &s.info)
    }

    /// The pixels shown for a side: the picture, or the preview of one over the limit.
    pub fn shown(&self, left: bool) -> Option<&Arc<Picture>> {
        let side = self.sides[side_index(left)].as_ref()?;
        side.pixels.as_ref().or(side.preview.as_ref())
    }

    pub fn loading(&self, left: bool) -> bool {
        self.loads[side_index(left)].is_running()
    }

    /// Why that side could not be opened.
    pub fn message(&self, left: bool) -> Option<&str> {
        self.messages[side_index(left)].as_deref()
    }

    /// The shown result; None before both sides are loaded, or when they are not compared.
    pub fn diff(&self) -> Option<&PictureDiff> {
        self.compared.as_ref().map(|c| &*c.diff)
    }

    pub fn compared(&self) -> Option<&Compared> {
        self.compared.as_ref()
    }

    /// Both sides are loaded and cannot be compared: one is over the pixel limit, or the union
    /// of their sizes is too large for a difference image (more than twice the limit).
    pub fn too_big(&self) -> bool {
        let (Some(left), Some(right)) = (&self.sides[LEFT], &self.sides[RIGHT]) else {
            return false;
        };
        let (left, right) = (&left.info, &right.info);
        let union =
            u64::from(left.width.max(right.width)) * u64::from(left.height.max(right.height));
        left.big || right.big || union > self.limits.max_pixels.saturating_mul(2)
    }

    /// The language changed: a notice is one-shot feedback in the old language, so it goes and the
    /// status falls back to what it works out. A failed side's message is said again.
    pub fn relabel(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.notice = None;
        self.carried = None;
        for message in self.messages.iter_mut().flatten() {
            message.relabel();
        }
        cx.notify();
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// The two paths; empty for a picture that has no file.
    pub fn paths(&self) -> (&Path, &Path) {
        (&self.paths[LEFT], &self.paths[RIGHT])
    }

    pub fn prefs(&self) -> &PicturePrefs {
        &self.prefs
    }

    /// The tab's badge (R11): none while busy or when the pair is not compared; "!" when a side
    /// did not open; "ok" for identical pictures; "≠" when only the sizes differ; otherwise the
    /// number of differing pixels.
    pub fn badge(&self) -> Option<String> {
        if self.is_busy() {
            return None;
        }
        if self.messages.iter().any(Option::is_some) {
            return Some("!".to_owned());
        }
        if self.too_big() {
            return None;
        }
        let diff = self.diff()?;
        if diff.is_identical() {
            return Some(badge_text(0));
        }
        match diff.count {
            0 => Some("≠".to_owned()),
            _ => Some(badge_text(diff.clusters.len())),
        }
    }

    /// Shows other pictures in this tab (the workspace re-keys the tab first).
    pub fn load_paths(&mut self, left: PathBuf, right: PathBuf, cx: &mut Context<Self>) {
        self.paths = [left, right];
        self.reloading = false;
        self.notice = self.carried.take();
        self.path_edit = None;
        self.retire_compared();
        self.start_side(LEFT, false, cx);
        self.start_side(RIGHT, false, cx);
    }

    /// F5: reads the files again. What is shown stays until the new pictures land.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let with_file = [LEFT, RIGHT].map(|i| !self.paths[i].as_os_str().is_empty());
        if !with_file.contains(&true) {
            self.notice = Some(tr(NOTHING_TO_RELOAD).into());
            cx.notify();
            return;
        }
        self.notice = None;
        self.reloading = true;
        for index in [LEFT, RIGHT] {
            if with_file[index] {
                self.start_side(index, true, cx);
            }
        }
    }

    /// F9: the sides change places, in this tab and in its key. A load still running goes with
    /// its side (R20).
    pub fn swap_sides(&mut self, cx: &mut Context<Self>) {
        self.paths.swap(LEFT, RIGHT);
        self.sides.swap(LEFT, RIGHT);
        self.messages.swap(LEFT, RIGHT);
        self.loads.swap(LEFT, RIGHT);
        self.load_ids.swap(LEFT, RIGHT);
        self.load_tasks.swap(LEFT, RIGHT);
        // The result was made in the old orientation: it would draw mirrored.
        self.retire_compared();
        cx.emit(PictureEvent::Swapped {
            left: self.paths[LEFT].clone(),
            right: self.paths[RIGHT].clone(),
        });
        if self.loads_running() {
            // The compare starts when the last load lands.
            self.diff_job.cancel();
            self.diff_task = None;
            cx.notify();
        } else {
            self.compare_now(cx);
        }
    }

    fn loads_running(&self) -> bool {
        self.loads.iter().any(JobSlot::is_running)
    }

    /// Starts reading and decoding one side. `keep`: the side already shown stays until the
    /// new one lands (LocalDiff `keep`); otherwise it is cleared to loading.
    fn start_side(&mut self, index: usize, keep: bool, cx: &mut Context<Self>) {
        // A result made from the old pictures is not wanted any more.
        self.diff_job.cancel();
        self.diff_task = None;
        let (generation, cancel) = self.loads[index].start();
        self.next_load_id += 1;
        let id = self.next_load_id;
        self.load_ids[index] = id;
        if !keep {
            self.set_side(index, None);
            self.messages[index] = None;
        }
        let path = self.paths[index].clone();
        let limits = self.limits;
        let background = cx.background_executor().clone();
        self.load_tasks[index] = Some(cx.spawn(async move |this, cx| {
            let outcome = background
                .spawn(async move { load_side(&path, &limits, &cancel) })
                .await;
            let _ = this.update(cx, |view, cx| view.loaded(id, generation, outcome, cx));
        }));
        cx.notify();
    }

    /// Load `id` finished. It lands on the side that waits for it, if that is still so.
    fn loaded(
        &mut self,
        id: u64,
        generation: u64,
        outcome: Result<Side, LoadError>,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.load_ids.iter().position(|&waiting| waiting == id) else {
            return;
        };
        if !self.loads[index].is_current(generation) {
            return;
        }
        self.loads[index].finish(generation);
        self.load_ids[index] = 0;
        match outcome {
            Ok(side) => {
                self.set_side(index, Some(side));
                self.messages[index] = None;
            }
            Err(LoadError::Message(mut message)) => {
                self.set_side(index, None);
                // Built on the job's thread: said again here, in the language now in force.
                message.relabel();
                self.messages[index] = Some(message);
            }
            Err(LoadError::Cancelled) => {
                cx.notify();
                return;
            }
        }
        if self.loads_running() {
            cx.notify();
            return;
        }
        if std::mem::take(&mut self.reloading) {
            self.notice = Some(tr(RELOADED).into());
        }
        self.compare_now(cx);
    }

    /// Both sides are loaded: compares them in the background. Anything less, or a pair that
    /// cannot be compared, leaves no result.
    fn compare_now(&mut self, cx: &mut Context<Self>) {
        self.diff_job.cancel();
        self.diff_task = None;
        let pictures = match (&self.sides[LEFT], &self.sides[RIGHT]) {
            (Some(left), Some(right)) if !self.too_big() => {
                left.pixels.clone().zip(right.pixels.clone())
            }
            _ => None,
        };
        let Some((left, right)) = pictures else {
            self.retire_compared();
            if self.too_big() {
                self.notice = Some(tr(NO_DIFFERENCE).into());
            }
            cx.notify();
            return;
        };
        let tolerance = self.prefs.tolerance;
        self.tolerance.started = Some(tolerance);
        let (generation, cancel) = self.diff_job.start();
        self.diff_started = Instant::now();
        let background = cx.background_executor().clone();
        self.diff_task = Some(cx.spawn(async move |this, cx| {
            let outcome = background
                .spawn(async move {
                    let diff = compare(&left, &right, tolerance, &cancel)?;
                    let image = diff_image(&left, &right, tolerance, &cancel)?;
                    // The RGBA image is not kept: export rebuilds it from the sides.
                    let tiles = to_tiles(&image);
                    drop(image);
                    Ok::<_, Cancelled>(Compared {
                        diff: Arc::new(diff),
                        tiles,
                        sides: [left, right],
                        tolerance,
                    })
                })
                .await;
            let _ = this.update(cx, |view, cx| view.applied(generation, outcome.ok(), cx));
        }));
        cx.notify();
    }

    /// A finished compare replaces the shown one, unless a newer start superseded it.
    fn applied(&mut self, generation: u64, outcome: Option<Compared>, cx: &mut Context<Self>) {
        if !self.diff_job.is_current(generation) {
            return;
        }
        self.diff_job.finish(generation);
        if outcome.is_some() {
            self.first_diff_ms
                .get_or_insert(self.diff_started.elapsed().as_millis() as u64);
            self.retire_compared();
            self.compared = outcome;
        }
        cx.notify();
    }

    /// Stops every load and the compare. What was shown stays, the tolerance controls show what
    /// it was made with, and the notice says it is incomplete.
    fn cancel_work(&mut self, cx: &mut Context<Self>) {
        for index in [LEFT, RIGHT] {
            self.loads[index].cancel();
            self.load_ids[index] = 0;
            self.load_tasks[index] = None;
        }
        self.diff_job.cancel();
        self.diff_task = None;
        self.restore_tolerance(cx);
        self.reloading = false;
        self.notice = Some(tr(CANCELLED).into());
        cx.notify();
    }

    /// Replaces a side; the old side's images wait to be dropped.
    fn set_side(&mut self, index: usize, side: Option<Side>) {
        if let Some(old) = std::mem::replace(&mut self.sides[index], side) {
            self.retire(old.tiles);
        }
    }

    /// Takes the shown result away; its images wait to be dropped, and its groups are gone.
    fn retire_compared(&mut self) {
        self.stage.group = None;
        if let Some(old) = self.compared.take() {
            self.retire(old.tiles);
        }
    }

    fn retire(&mut self, tiles: Tiles) {
        self.retired.extend(images(&tiles).cloned());
    }

    /// Every image the view holds, live ones first.
    fn live_images(&self) -> Vec<Arc<RenderImage>> {
        let sides = self.sides.iter().flatten().map(|side| &side.tiles);
        let shown = self.compared.iter().map(|c| &c.tiles);
        let enlarged = self.stage.enlargements.images();
        sides
            .chain(shown)
            .flat_map(images)
            .chain(enlarged)
            .cloned()
            .collect()
    }

    /// Gives back what the tab holds on the GPU, live and retired images alike. The workspace
    /// calls it when the tab closes.
    pub fn release_images(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.released += 1;
        let all = self.live_images().into_iter().chain(self.retired.drain(..));
        for image in all {
            self.released_ids.push(image.id.0);
            cx.drop_image(image, Some(window));
        }
    }

    /// The tiles' size of a layer, if it has any.
    #[doc(hidden)]
    pub fn tiles_for_test(&self, layer: Layer) -> Option<(u32, u32)> {
        let tiles = match layer {
            Layer::Left => &self.sides[LEFT].as_ref()?.tiles,
            Layer::Right => &self.sides[RIGHT].as_ref()?.tiles,
            Layer::Difference => &self.compared.as_ref()?.tiles,
        };
        Some((tiles.width, tiles.height))
    }

    /// How many replaced images wait for the next frame to drop them.
    #[doc(hidden)]
    pub fn retired_for_test(&self) -> usize {
        self.retired.len()
    }

    /// How many retired images the frames have dropped.
    #[doc(hidden)]
    pub fn dropped_for_test(&self) -> usize {
        self.dropped
    }

    /// The ids of the images held now.
    #[doc(hidden)]
    pub fn image_ids_for_test(&self) -> Vec<usize> {
        self.live_images().iter().map(|image| image.id.0).collect()
    }

    /// The ids of the images `release_images` gave back.
    #[doc(hidden)]
    pub fn released_ids_for_test(&self) -> Vec<usize> {
        self.released_ids.clone()
    }

    /// How many times `release_images` ran, for the tests.
    #[doc(hidden)]
    pub fn release_count(&self) -> usize {
        self.released
    }

    /// Esc ends a path edit, else cancels a running load or compare, else forgets the current
    /// group, else goes back to the folder tab (R14, PF4). The path box is dealt with before
    /// `takes_key`, which would pass the key to it.
    fn on_escape(&mut self, _: &PictureEscape, window: &mut Window, cx: &mut Context<Self>) {
        if self.path_edit.is_some() {
            self.cancel_path_edit(window, cx);
            return;
        }
        if !self.takes_key(window, cx) {
            return;
        }
        if self.is_busy() {
            self.cancel_work(cx);
        } else if self.stage.group.take().is_some() {
            cx.notify();
        } else {
            cx.emit(PictureEvent::Back);
        }
    }

    /// Keys that mean something in a field stay with it: the view's keys run only when no
    /// input of this view has the keys, else they propagate to the input.
    fn takes_key(&self, window: &Window, cx: &mut Context<Self>) -> bool {
        let free = !self
            .inputs(cx)
            .iter()
            .any(|input| input.contains_focused(window, cx));
        if !free {
            cx.propagate();
        }
        free
    }

    /// The focus handles of the view's inputs: the tolerance box and the open path box.
    fn inputs(&self, cx: &App) -> Vec<FocusHandle> {
        let mut inputs = vec![self.tolerance.input.focus_handle(cx)];
        inputs.extend(self.path_input(cx));
        inputs
    }
}

impl Render for PictureView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for image in self.retired.drain(..) {
            self.dropped += 1;
            cx.drop_image(image, Some(window));
        }
        self.sync_tolerance_controls(window, cx);
        let palette = Palette::current(cx);
        let fonts = Fonts::current(cx);
        let toolbar = self.render_toolbar(&palette, &fonts, cx);
        let heads = self.render_heads(&palette, &fonts, cx);
        let stage = self.render_stage(&palette, window, cx);
        let status = self.render_status(&palette, &fonts);
        let root = v_flex()
            .id("picture-view")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .bg(palette.bg)
            .text_color(palette.text)
            .text_size(px(UI_SIZE))
            .on_action(cx.listener(Self::on_escape))
            .on_action(cx.listener(|this, _: &PictureReload, w, cx| {
                if this.takes_key(w, cx) {
                    this.reload(cx)
                }
            }))
            .on_action(cx.listener(|this, _: &PictureSwap, w, cx| {
                if this.takes_key(w, cx) {
                    this.swap_sides(cx)
                }
            }));
        Self::open_actions(self.stage_actions(root, cx), cx)
            .child(toolbar)
            .child(heads)
            .child(stage)
            .child(status)
    }
}
