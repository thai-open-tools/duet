//! The start screen, LocalDiff's Home. The new compare leads: the welcome card once, the path
//! form, which is also the drop zone, and six tiles, one per kind of compare. The recent pairs
//! and a short sheet of keys are secondary, on the right (U12).

use std::path::{Path, PathBuf};

use gpui_kit::TestSupportExt as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, ExternalPaths, FocusHandle, Focusable,
    FontWeight, Global, KeyBinding, MouseButton, MouseDownEvent, PathPromptOptions, Pixels,
    SharedString, Subscription, Task, WeakEntity, Window, div, prelude::*, px, relative,
};

use crate::app::recent_file;
use crate::help::topics::{self, HelpLang};
use crate::i18n::{paired, tr, tr_key, trf};
use crate::launch::{Launch, MergePaths, classify};
use crate::palette::Palette;
use crate::recent::{self, Recent, RecentKind};
use crate::settings;
use crate::ui::edit_menu::ld_input;
use crate::ui::fonts::Fonts;
use crate::ui::icons;
use crate::ui::menu::{MenuEntry, fill};
use crate::ui::prose::ProseExt as _;
use crate::ui::style::{
    CARD_RADIUS, LdExt, ld_button, ld_card, ld_ghost, ld_icon, ld_icon_label_button, ld_kbd,
    ld_primary, ld_small_icon_button,
};
use duet_core::kind::CompareKind;

/// The sheet's rows: action names and what they do. The keys come from the real bindings, so
/// the sheet cannot drift from them.
pub const SHORTCUTS: &[(&[&str], &str)] = &[
    (
        &["text::PrevChange", "text::NextChange"],
        paired("Previous or next difference"),
    ),
    (
        &["text::FirstChange", "text::LastChange"],
        paired("First or last difference"),
    ),
    (
        &["text::CopyBlockRight", "text::CopyBlockLeft"],
        paired("Copy the block to the right or left"),
    ),
    (&["text::Undo", "text::Redo"], paired("Undo or redo")),
    (&["text::Save"], paired("Save the edited side")),
    (
        &["text::OpenFind", "text::FindNext"],
        paired("Find, then the next match"),
    ),
    (&["text::OpenGoTo"], paired("Go to line")),
    (
        &["text::Reload"],
        paired("Reload the files, or compare the folders again"),
    ),
    (&["text::SwapSides"], paired("Swap left and right")),
    (
        &[
            "text::FilterAll",
            "text::FilterDifferences",
            "text::FilterContext",
            "text::FilterSame",
        ],
        paired("Show all, differences, context or same"),
    ),
    (
        &["folder::DeleteSelected"],
        paired("Delete the selected files"),
    ),
    (
        &["workspace::NewTab", "workspace::CloseTab"],
        paired("New tab or close tab"),
    ),
    (&["workspace::NextTab"], paired("Next tab")),
    (&["workspace::OpenHelp"], paired("Help")),
    (&["text::Escape"], paired("Cancel, close a bar or go back")),
];

/// The sheet's rows in Thai, for the help (H4): each English label beside its Thai twin, in
/// `SHORTCUTS` order.
pub const SHORTCUTS_TH: &[(&str, &str)] = &[
    (
        paired("Previous or next difference"),
        paired("จุดต่างก่อนหน้า / ถัดไป"),
    ),
    (
        paired("First or last difference"),
        paired("จุดต่างแรก / สุดท้าย"),
    ),
    (
        paired("Copy the block to the right or left"),
        paired("คัดลอกบล็อกไปขวา / ซ้าย"),
    ),
    (paired("Undo or redo"), paired("ย้อนกลับ / ทำซ้ำ")),
    (paired("Save the edited side"), paired("บันทึกฝั่งที่แก้ไข")),
    (
        paired("Find, then the next match"),
        paired("ค้นหา แล้วไปจุดที่พบถัดไป"),
    ),
    (paired("Go to line"), paired("ไปบรรทัด")),
    (
        paired("Reload the files, or compare the folders again"),
        paired("โหลดไฟล์ใหม่ หรือเทียบโฟลเดอร์อีกครั้ง"),
    ),
    (paired("Swap left and right"), paired("สลับซ้าย-ขวา")),
    (
        paired("Show all, differences, context or same"),
        paired("แสดงทั้งหมด / เฉพาะที่ต่าง / จุดต่างพร้อมบรรทัดรอบข้าง / เฉพาะที่เหมือน"),
    ),
    (
        paired("Delete the selected files"),
        paired("ลบไฟล์ที่เลือก (ไปถังขยะ)"),
    ),
    (paired("New tab or close tab"), paired("แท็บใหม่ / ปิดแท็บ")),
    (paired("Next tab"), paired("แท็บถัดไป")),
    (paired("Help"), paired("เปิดคู่มือนี้")),
    (
        paired("Cancel, close a bar or go back"),
        paired("ยกเลิก / ปิดแถบ / กลับหน้าก่อน"),
    ),
];

/// The Start tab's short sheet (U12): the rows reached for first, each a row of `SHORTCUTS`,
/// which the Help's keys topic lists in full.
pub const START_SHORTCUTS: &[(&[&str], &str)] = &[
    (
        &["text::PrevChange", "text::NextChange"],
        paired("Previous or next difference"),
    ),
    (
        &["text::CopyBlockRight", "text::CopyBlockLeft"],
        paired("Copy the block to the right or left"),
    ),
    (&["text::Undo", "text::Redo"], paired("Undo or redo")),
    (&["text::Save"], paired("Save the edited side")),
    (
        &["text::OpenFind", "text::FindNext"],
        paired("Find, then the next match"),
    ),
    (
        &["text::Reload"],
        paired("Reload the files, or compare the folders again"),
    ),
    (
        &["workspace::NewTab", "workspace::CloseTab"],
        paired("New tab or close tab"),
    ),
    (&["workspace::OpenHelp"], paired("Help")),
];

/// Each row's keys, "F7 / F8": the first binding of each action, as Windows names it.
pub fn shortcut_rows(bindings: &[KeyBinding]) -> Vec<(String, &'static str)> {
    rows_with_keys(SHORTCUTS, bindings)
}

/// The Start tab's sheet, read from the bindings as `shortcut_rows` reads the whole one.
pub fn start_shortcut_rows(bindings: &[KeyBinding]) -> Vec<(String, &'static str)> {
    rows_with_keys(START_SHORTCUTS, bindings)
}

fn rows_with_keys(
    rows: &'static [(&'static [&'static str], &'static str)],
    bindings: &[KeyBinding],
) -> Vec<(String, &'static str)> {
    rows.iter()
        .map(|(actions, label)| {
            let keys: Vec<String> = actions
                .iter()
                .filter_map(|name| {
                    let binding = bindings.iter().find(|b| b.action().name() == *name)?;
                    Some(
                        binding
                            .keystrokes()
                            .iter()
                            .map(|k| Kbd::format(k.inner()))
                            .collect::<Vec<_>>()
                            .join(" "),
                    )
                })
                .collect();
            (keys.join(" / "), *label)
        })
        .collect()
}

/// The widest the page grows, and its padding at each side.
const PAGE_WIDTH: f32 = 1400.;
const PAGE_PADDING: f32 = 28.;
/// The left column's share of a two-column page.
const LEFT_SHARE: f32 = 0.535;
/// Below this left-column width the tiles take two columns (U12).
const THREE_TILES_WIDTH: f32 = 640.;

/// Home is one column below 1000 px.
pub fn home_columns(width: Pixels) -> usize {
    if width < px(1000.) { 1 } else { 2 }
}

/// The tiles' columns in a window `width` wide: three, or two when the left column is
/// narrower than about 640 px (U12).
pub fn tile_columns(width: Pixels) -> usize {
    let page = f32::from(width).min(PAGE_WIDTH) - 2. * PAGE_PADDING;
    let left = if home_columns(width) == 1 {
        page
    } else {
        page * LEFT_SHARE
    };
    if left < THREE_TILES_WIDTH { 2 } else { 3 }
}

/// The `settings.json` key that keeps the welcome card's dismissal (U15).
pub const WELCOME_KEY: &str = "start.welcome";
/// The welcome card shows while `start.welcome` is below this.
pub const WELCOME_REV: u64 = 1;

/// Whether the welcome card is due: while `start.welcome` (a number) is below `WELCOME_REV`
/// (U15). No settings file, no key or a value that is not a number shows it.
pub fn welcome_due(settings: Option<&Path>) -> bool {
    let seen = settings
        .and_then(|path| settings::get_f64(path, WELCOME_KEY))
        .unwrap_or(0.);
    seen < WELCOME_REV as f64
}

/// Set when the welcome card is dismissed, so this run's later Start tabs keep it hidden even
/// when settings.json could not be written.
struct WelcomeSeen;

impl Global for WelcomeSeen {}

/// Whether the welcome card is due in this run.
fn welcome_now(cx: &App) -> bool {
    !cx.has_global::<WelcomeSeen>() && welcome_due(settings::settings_file(cx).as_deref())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StartMode {
    Files,
    Folders,
    Merge,
    Hex,
    Table,
    Picture,
}

pub enum StartEvent {
    Compare(Launch),
    /// Open the Help tab, at the named topic.
    OpenHelp(Option<&'static str>),
    /// The welcome card was dismissed: every Start tab hides it.
    WelcomeDismissed,
}

pub struct StartView {
    left: Entity<InputState>,
    right: Entity<InputState>,
    error: Option<SharedString>,
    /// Recent pairs, each with whether both paths still exist (None until checked).
    recent: Vec<(Recent, Option<bool>)>,
    /// The recent entry last right-clicked: what its menu acts on.
    menu_recent: Option<usize>,
    /// The whole recent list shows, not its first `RECENT_SHOWN`; not saved (U13).
    all_recent: bool,
    /// The welcome card shows (U15).
    welcome: bool,
    /// Counts the checks started; a check that is not the newest is stale.
    check_generation: u64,
    /// Set by the binary-files card: the next compare opens its files as this kind, whatever
    /// they hold. Cleared by that compare, by cancelling the card's picker and by typing in
    /// either box.
    forced_kind: Option<CompareKind>,
    check_task: Option<Task<()>>,
    weak: WeakEntity<Self>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<StartEvent> for StartView {}

impl Focusable for StartView {
    /// Focusing the start screen puts the caret in the left box.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.left.focus_handle(cx)
    }
}

impl StartView {
    pub fn new(error: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let left = cx.new(|cx| InputState::new(window, cx).placeholder(tr("Left file or folder")));
        let right =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr("Right file or folder")));
        let on_input = |this: &mut Self,
                        _: &Entity<InputState>,
                        event: &InputEvent,
                        _: &mut Window,
                        cx: &mut Context<Self>| {
            match event {
                InputEvent::PressEnter { .. } => this.compare(cx),
                // Typing ends the binary-files card's force: a typed pair is detected. A
                // picker or a drop fills the box without this event.
                InputEvent::Change => this.forced_kind = None,
                _ => {}
            }
        };
        let subscriptions = vec![
            cx.subscribe_in(&left, window, on_input),
            cx.subscribe_in(&right, window, on_input),
        ];
        // The recent list stays empty here: the workspace activates every Start tab it opens,
        // and activating one reads the list (`reload_recent`).
        Self {
            left,
            right,
            error: error.map(Into::into),
            recent: Vec::new(),
            menu_recent: None,
            all_recent: false,
            welcome: welcome_now(cx),
            check_generation: 0,
            forced_kind: None,
            check_task: None,
            weak: cx.entity().downgrade(),
            _subscriptions: subscriptions,
        }
    }

    /// Reads the recent list again: a compare in another tab or window may have changed it
    /// since this screen opened. Entries already checked keep their mark; the others are
    /// checked off the UI thread, since a path on an offline share can block for seconds.
    pub fn reload_recent(&mut self, cx: &mut Context<Self>) {
        let fresh = recent_file(cx)
            .map(|path| recent::load(&path))
            .unwrap_or_default();
        if fresh.iter().eq(self.recent.iter().map(|(entry, _)| entry)) {
            return;
        }
        let known = std::mem::take(&mut self.recent);
        self.recent = fresh
            .into_iter()
            .map(|entry| {
                let exists = known
                    .iter()
                    .find(|(e, _)| *e == entry)
                    .and_then(|(_, x)| *x);
                (entry, exists)
            })
            .collect();
        let unchecked: Vec<Recent> = self
            .recent
            .iter()
            .filter(|(_, exists)| exists.is_none())
            .map(|(entry, _)| entry.clone())
            .collect();
        if !unchecked.is_empty() {
            let background = cx.background_executor().clone();
            cx.spawn(async move |this, cx| {
                let found = background
                    .spawn(async move {
                        unchecked
                            .into_iter()
                            .map(|e| {
                                let exists = e.left.exists()
                                    && e.right.exists()
                                    && e.base.as_ref().is_none_or(|base| base.exists());
                                (e, exists)
                            })
                            .collect::<Vec<_>>()
                    })
                    .await;
                let _ = this.update(cx, |view, cx| {
                    for (checked, found) in found {
                        for (entry, exists) in &mut view.recent {
                            if *entry == checked {
                                *exists = Some(found);
                            }
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    pub fn recent(&self) -> &[(Recent, Option<bool>)] {
        &self.recent
    }

    /// The welcome card's language while it shows (U15): the UI's.
    pub fn welcome(&self) -> Option<HelpLang> {
        self.welcome.then(HelpLang::current)
    }

    /// The language changed: an error shown here is one-shot feedback in the old language, so it
    /// goes, and the two boxes' placeholders are set again (they are stored in the boxes).
    pub fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        for (input, key) in [
            (&self.left, tr_key("Left file or folder")),
            (&self.right, tr_key("Right file or folder")),
        ] {
            input.update(cx, |state, cx| state.set_placeholder(tr(key), window, cx));
        }
        cx.notify();
    }

    /// Reads the welcome card's state again as the tab is shown: another window may have
    /// dismissed the card.
    pub fn reload_welcome(&mut self, cx: &mut Context<Self>) {
        if self.welcome && !welcome_now(cx) {
            self.welcome = false;
            cx.notify();
        }
    }

    /// Either welcome button: `start.welcome` is saved, and every Start tab hides the card.
    fn dismiss_welcome(&mut self, cx: &mut Context<Self>) {
        cx.set_global(WelcomeSeen);
        if let Some(path) = settings::settings_file(cx)
            && let Err(err) = settings::set_u64(&path, WELCOME_KEY, WELCOME_REV)
        {
            tracing::warn!(%err, "could not save the welcome card's dismissal");
        }
        self.welcome = false;
        cx.emit(StartEvent::WelcomeDismissed);
        cx.notify();
    }

    /// Another Start tab dismissed the welcome card.
    pub fn hide_welcome(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.welcome) {
            cx.notify();
        }
    }

    pub fn left_input(&self) -> &Entity<InputState> {
        &self.left
    }

    pub fn right_input(&self) -> &Entity<InputState> {
        &self.right
    }

    /// Opens a recent pair; a path that is gone explains itself like a typed one.
    pub fn open_recent(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some((entry, _)) = self.recent.get(index) else {
            return;
        };
        self.menu_recent = None;
        let (left, right, kind) = (entry.left.clone(), entry.right.clone(), entry.kind);
        if kind == RecentKind::Merge {
            // Its three files are named in the entry: no check to make.
            cx.emit(StartEvent::Compare(Launch::Merge(MergePaths {
                left: Some(left),
                base: entry.base.clone(),
                right: Some(right),
                output: None,
            })));
            return;
        }
        let forced = match kind {
            RecentKind::Hex => Some(CompareKind::Hex),
            RecentKind::Table => Some(CompareKind::Table),
            RecentKind::Picture => Some(CompareKind::Picture),
            RecentKind::Folders | RecentKind::Files | RecentKind::Merge => None,
        };
        self.check(left, right, forced, None, cx);
    }

    /// Decides the view for a pair off the UI thread (it reads the start of both files, which
    /// on a network share or a cloud placeholder can take seconds), then opens it or shows
    /// why not. `form` is the typed text the check was started from: if the boxes hold
    /// something else when it returns, or a newer check was started, the answer is dropped.
    /// `forced` reopens a hex or table entry as such even when the files read as text by now.
    fn check(
        &mut self,
        left: PathBuf,
        right: PathBuf,
        forced: Option<CompareKind>,
        form: Option<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        self.check_generation += 1;
        let generation = self.check_generation;
        let background = cx.background_executor().clone();
        self.check_task = Some(cx.spawn(async move |this, cx| {
            let launch = background
                .spawn(async move { classify(&left, &right) })
                .await;
            let _ = this.update(cx, |view, cx| {
                if generation != view.check_generation || view.form_changed(&form, cx) {
                    return;
                }
                let launch = match (launch, forced) {
                    (
                        Launch::Files { left, right }
                        | Launch::Picture { left, right }
                        | Launch::Table { left, right },
                        Some(CompareKind::Hex),
                    ) => Launch::Hex { left, right },
                    (
                        Launch::Files { left, right }
                        | Launch::Hex { left, right }
                        | Launch::Picture { left, right },
                        Some(CompareKind::Table),
                    ) => Launch::Table { left, right },
                    (
                        Launch::Files { left, right }
                        | Launch::Hex { left, right }
                        | Launch::Picture { left, right }
                        | Launch::Table { left, right },
                        Some(CompareKind::Picture),
                    ) => Launch::Picture { left, right },
                    (other, _) => other,
                };
                match launch {
                    Launch::Invalid(message) => {
                        view.error = Some(message.into());
                        cx.notify();
                    }
                    launch => {
                        view.error = None;
                        cx.emit(StartEvent::Compare(launch));
                    }
                }
            });
        }));
    }

    /// The boxes no longer hold the text a check was started from.
    fn form_changed(&self, form: &Option<(String, String)>, cx: &App) -> bool {
        form.as_ref().is_some_and(|(left, right)| {
            clean_path(&self.left.read(cx).value()) != left
                || clean_path(&self.right.read(cx).value()) != right
        })
    }

    /// The menu of the recent entry last right-clicked; empty (so no menu opens) when none was.
    pub fn recent_menu(&self) -> Vec<MenuEntry> {
        let Some(i) = self.menu_recent.filter(|i| *i < self.recent.len()) else {
            return Vec::new();
        };
        let (open, remove) = (self.weak.clone(), self.weak.clone());
        vec![
            MenuEntry::click(tr("Open"), move |_, cx| {
                open.update(cx, |this, cx| this.open_recent(i, cx)).ok();
            }),
            MenuEntry::click(tr("Remove from list"), move |_, cx| {
                remove.update(cx, |this, cx| this.remove_recent(i, cx)).ok();
            }),
        ]
    }

    pub fn menu_target_for_test(&self) -> Option<usize> {
        self.menu_recent
    }

    pub fn remove_recent(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.recent.len() {
            return;
        }
        self.menu_recent = None;
        let (entry, _) = self.recent.remove(index);
        if let Some(path) = recent_file(cx)
            && let Err(err) = recent::remove(&path, &entry)
        {
            tracing::warn!(%err, "could not update the recent list");
        }
        cx.notify();
    }

    pub fn clear_recent(&mut self, cx: &mut Context<Self>) {
        self.recent.clear();
        if let Some(path) = recent_file(cx)
            && let Err(err) = recent::clear(&path)
        {
            tracing::warn!(%err, "could not clear the recent list");
        }
        cx.notify();
    }

    /// A mode card: the left box takes the caret, and that mode's picker opens for it.
    pub fn start_mode(&mut self, folders: bool, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.left.focus_handle(cx), cx);
        self.browse(true, folders, false, window, cx);
    }

    /// The binary-files card: like the files card, but the pair opens in hex even when both
    /// files read as text. Cancelling its picker cancels that.
    pub fn start_hex_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.forced_kind = Some(CompareKind::Hex);
        window.focus(&self.left.focus_handle(cx), cx);
        self.browse(true, false, true, window, cx);
    }

    /// The table card: like the hex card, but the pair opens as a table.
    pub fn start_table_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.forced_kind = Some(CompareKind::Table);
        window.focus(&self.left.focus_handle(cx), cx);
        self.browse(true, false, true, window, cx);
    }

    /// The picture card: like the hex card, but the pair opens as pictures.
    pub fn start_picture_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.forced_kind = Some(CompareKind::Picture);
        window.focus(&self.left.focus_handle(cx), cx);
        self.browse(true, false, true, window, cx);
    }

    fn set_box(&mut self, left: bool, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let state = if left {
            self.left.clone()
        } else {
            self.right.clone()
        };
        state.update(cx, |state, cx| state.set_value(text, window, cx));
    }

    /// Files or folders dropped from Explorer. Two or more fill both boxes and compare; one
    /// fills the box it was dropped on, or the first empty box when dropped elsewhere.
    pub fn drop_paths(
        &mut self,
        paths: &[PathBuf],
        target: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = |path: &PathBuf| path.display().to_string();
        match (paths, target) {
            ([], _) => {}
            ([one], Some(left)) => self.set_box(left, &text(one), window, cx),
            ([one], None) => {
                let left = self.left.read(cx).value().trim().is_empty();
                self.set_box(left, &text(one), window, cx);
            }
            ([a, b, ..], _) => {
                self.set_box(true, &text(a), window, cx);
                self.set_box(false, &text(b), window, cx);
                self.compare(cx);
            }
        }
        cx.notify();
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn set_paths(
        &mut self,
        left: &str,
        right: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.left
            .update(cx, |state, cx| state.set_value(left, window, cx));
        self.right
            .update(cx, |state, cx| state.set_value(right, window, cx));
    }

    pub fn compare(&mut self, cx: &mut Context<Self>) {
        let left = clean_path(&self.left.read(cx).value()).to_owned();
        let right = clean_path(&self.right.read(cx).value()).to_owned();
        if left.is_empty() || right.is_empty() {
            self.error = Some(tr("Choose two files or two folders.").into());
            cx.notify();
            return;
        }
        let paths = (PathBuf::from(&left), PathBuf::from(&right));
        let forced = self.forced_kind.take();
        self.check(paths.0, paths.1, forced, Some((left, right)), cx);
    }

    /// A picker for one box. `forced_card`: the binary-files, table or picture card asked, so a cancel ends its force.
    fn browse(
        &mut self,
        left_side: bool,
        folders: bool,
        forced_card: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: !folders,
            directories: folders,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let path = match picked.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(path) = path else {
                if forced_card {
                    let _ = this.update(cx, |view, _| view.forced_kind = None);
                }
                return;
            };
            let _ = this.update_in(cx, |view, window, cx| {
                let target = if left_side {
                    view.left.clone()
                } else {
                    view.right.clone()
                };
                let text = path.display().to_string();
                target.update(cx, |state, cx| state.set_value(text.as_str(), window, cx));
            });
        })
        .detach();
    }

    fn path_row(
        &self,
        label: &'static str,
        left_side: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = if left_side { &self.left } else { &self.right };
        let side = if left_side {
            "start-left"
        } else {
            "start-right"
        };
        let id = |suffix: &str| SharedString::from(format!("{side}-{suffix}"));
        h_flex()
            .id(id("row"))
            .test_support()
            .gap(px(8.))
            .items_center()
            .w_full()
            .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
                cx.stop_propagation();
                this.drop_paths(paths.paths(), Some(left_side), window, cx);
            }))
            .child(
                div()
                    .w(px(44.))
                    .flex_none()
                    .text_color(p.muted)
                    .child(tr(label)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(ld_input(state).aria_label(if left_side {
                        tr("Left path")
                    } else {
                        tr("Right path")
                    })),
            )
            .child(ld_button(id("file"), tr("File…"), p).ld_click(
                true,
                cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.browse(left_side, false, false, window, cx)
                }),
            ))
            .child(ld_button(id("folder"), tr("Folder…"), p).ld_click(
                true,
                cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.browse(left_side, true, false, window, cx)
                }),
            ))
    }

    /// The welcome card (U15), in the UI's language and drawn as the help draws its prose
    /// (H6): the title and three lines, then the guide and dismiss buttons.
    fn render_welcome(
        &self,
        lang: HelpLang,
        p: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let copy = topics::welcome_copy(lang);
        let [first, second, third] = copy.lines;
        let mut rows = crate::help::prose_rows(&[copy.title, first, second, third], p, &fonts.code);
        let lines: Vec<Div> = rows.drain(1..).collect();
        let title = rows.remove(0);
        h_flex()
            .id("start-welcome")
            .test_support()
            .flex_wrap()
            .items_end()
            .gap(px(12.))
            .px(px(14.))
            .py(px(12.))
            .bg(p.panel)
            .border_1()
            .border_color(p.line)
            .rounded(px(CARD_RADIUS))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(240.))
                    .gap(px(2.))
                    .child(
                        h_flex()
                            .items_start()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .h(px(WELCOME_TITLE_SIZE * crate::help::LINE_HEIGHT))
                                    .child(
                                        div().w(px(3.)).h(px(14.)).rounded(px(1.5)).bg(p.accent),
                                    ),
                            )
                            .child(
                                div()
                                    .id("start-welcome-title")
                                    .test_support()
                                    .aria_label(copy.title)
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(WELCOME_TITLE_SIZE))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            ),
                    )
                    .child(v_flex().pl(px(11.)).text_color(p.muted).children(lines)),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap(px(8.))
                    .child(ld_button("start-welcome-help", copy.open, p).ld_click(
                        true,
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.dismiss_welcome(cx);
                            cx.emit(StartEvent::OpenHelp(None));
                        }),
                    ))
                    .child(ld_ghost("start-welcome-close", copy.dismiss, p).ld_click(
                        true,
                        cx.listener(|this, _: &ClickEvent, _, cx| this.dismiss_welcome(cx)),
                    )),
            )
    }

    /// The path form, which is also the drop zone: the drop hint, both rows, then the error on
    /// the left with Paste text… and Compare on the right. Its border turns accent while files
    /// are dragged over the page.
    fn render_form(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let error = self.error.clone().map(|error| {
            div()
                .id("start-error")
                .test_support()
                .text_size(px(12.5))
                .text_color(p.del)
                .prose(error)
        });
        let accent = p.accent;
        v_flex()
            .id("start-form")
            .test_support()
            .gap(px(10.))
            .px(px(14.))
            .py(px(12.))
            .bg(p.panel)
            .border_1()
            .border_color(p.line)
            .rounded(px(CARD_RADIUS))
            .group_drag_over::<ExternalPaths>(DROP_GROUP, move |s| s.border_color(accent))
            .child(
                h_flex()
                    .id("start-drop-hint")
                    .test_support()
                    .gap(px(8.))
                    .child(ld_icon(icons::DROP, 18.).text_color(accent))
                    .child(
                        div()
                            .min_w_0()
                            .text_size(px(12.5))
                            .text_color(p.muted)
                            .prose(tr("Drop two files or folders here, or type their paths")),
                    ),
            )
            .child(self.path_row(tr_key("Left"), true, p, cx))
            .child(self.path_row(tr_key("Right"), false, p, cx))
            .child(
                h_flex()
                    .gap(px(10.))
                    .items_center()
                    .child(div().flex_1().min_w_0().children(error))
                    .child(
                        ld_icon_label_button(PASTE_TEXT.0, icons::PASTE_TEXT, tr(PASTE_TEXT.1), p)
                            .as_ghost(p)
                            .ld_tooltip(
                                tr("Two empty sides to type or paste into; you can open files into either side later"),
                            )
                            // Two empty paths: the untitled pair (R15).
                            .ld_click(
                                true,
                                cx.listener(|_, _: &ClickEvent, _, cx| {
                                    cx.emit(StartEvent::Compare(Launch::Files {
                                        left: PathBuf::new(),
                                        right: PathBuf::new(),
                                    }))
                                }),
                            ),
                    )
                    .child(ld_primary("start-compare", tr("Compare"), p).ld_click(
                        true,
                        cx.listener(|this, _: &ClickEvent, _, cx| this.compare(cx)),
                    )),
            )
    }

    /// A tile (U12): a 34 px square with its 20 px icon, then the title and its line.
    fn mode_card(
        &self,
        mode: StartMode,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (id, title, text) = mode_copy(mode);
        let (accent, panel2) = (p.accent, p.panel2);
        ld_card(id, tr(title), p)
            .flex()
            .flex_row()
            .items_start()
            .justify_start()
            .h_auto()
            .min_w_0()
            .gap(px(12.))
            .px(px(14.))
            .py(px(12.))
            .rounded(px(CARD_RADIUS))
            .hover(move |s| s.border_color(accent).bg(panel2))
            .ld_click(
                true,
                cx.listener(move |this, _: &ClickEvent, window, cx| match mode {
                    StartMode::Hex => this.start_hex_mode(window, cx),
                    StartMode::Table => this.start_table_mode(window, cx),
                    StartMode::Picture => this.start_picture_mode(window, cx),
                    StartMode::Merge => {
                        cx.emit(StartEvent::Compare(Launch::Merge(MergePaths::default())))
                    }
                    mode => this.start_mode(mode == StartMode::Folders, window, cx),
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .size(px(34.))
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .bg(p.panel3)
                    .child(ld_icon(mode_icon(mode), 20.).text_color(accent)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::BOLD)
                            .child(tr(title)),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(relative(1.4))
                            .text_color(p.muted)
                            .prose(tr(text)),
                    ),
            )
    }

    /// The six tiles in a grid of `columns`; each row's tiles stretch to its tallest.
    fn render_tiles(
        &self,
        columns: usize,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("start-tiles")
            .test_support()
            .grid()
            .grid_cols(columns as u16)
            .gap(px(10.))
            .children(TILES.map(|mode| self.mode_card(mode, p, cx)))
    }

    fn render_recent_heading(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .items_center()
            .justify_between()
            .h(px(22.))
            .child(heading(tr("Recent"), p))
            .children((!self.recent.is_empty()).then(|| {
                ld_button("start-recent-clear", tr("Clear"), p)
                    .ld_small()
                    .ld_click(
                        true,
                        cx.listener(|this, _: &ClickEvent, _, cx| this.clear_recent(cx)),
                    )
            }))
    }

    /// The recent list (U13): one row per pair, the first `RECENT_SHOWN` unless all are asked
    /// for. A row shows the kind's icon, the names and, dimmed, the folder they live in; its
    /// tooltip holds the full paths.
    fn render_recent(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let (accent, line2) = (p.accent, p.line2);
        // A right press anywhere in the list first forgets the target (capture runs before the
        // entries); an entry's own right press then sets it, so blank space opens no stale menu.
        let list = v_flex()
            .id("start-recent-list")
            .test_support()
            .gap(px(4.))
            .capture_any_mouse_down(cx.listener(|this, e: &MouseDownEvent, _, _| {
                if e.button == MouseButton::Right {
                    this.menu_recent = None;
                }
            }));
        // The menu wraps the list, not each entry; the entry's right-click picks its pair.
        let view = cx.entity().downgrade();
        let menu = move |menu, _: &mut Window, cx: &mut Context<PopupMenu>| {
            let entries = view
                .upgrade()
                .map(|v| v.read(cx).recent_menu())
                .unwrap_or_default();
            fill(menu, entries, None)
        };
        if self.recent.is_empty() {
            return list
                .child(
                    h_flex()
                        .id("start-recent-empty")
                        .test_support()
                        .aria_label(tr(NO_RECENT))
                        .gap(px(8.))
                        .text_size(px(12.5))
                        .text_color(p.dim)
                        .child(ld_icon(icons::NO_RECENT, 16.))
                        .child(div().flex_1().min_w_0().prose(tr(NO_RECENT))),
                )
                .context_menu(menu);
        }
        let shown = if self.all_recent {
            self.recent.len()
        } else {
            RECENT_SHOWN
        };
        list.children(
            self.recent
                .iter()
                .take(shown)
                .enumerate()
                .map(|(i, (entry, exists))| {
                    let kind = entry.kind.label();
                    // The name reads as before: a merge names its base between the sides, as the
                    // merge tab lays them out.
                    let name = match &entry.base {
                        Some(base) => trf(
                            "{}: {}, {} and {}",
                            &[
                                &kind,
                                &entry.left.display(),
                                &base.display(),
                                &entry.right.display(),
                            ],
                        ),
                        None => trf(
                            "{}: {} and {}",
                            &[&kind, &entry.left.display(), &entry.right.display()],
                        ),
                    };
                    let line = entry.line();
                    // The × is the entry's sibling, drawn over its right edge, so the entry is
                    // never a button inside a button; the entry keeps room for it.
                    div()
                        .relative()
                        .min_w_0()
                        .when(*exists == Some(false), |d| d.opacity(0.5))
                        .child(
                            ld_card(("start-recent", i), name, p)
                                .ld_tooltip(entry.tooltip())
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_start()
                                .h(px(RECENT_HEIGHT))
                                .gap(px(8.))
                                .min_w_0()
                                .pl(px(10.))
                                .pr(px(10. + REMOVE_WIDTH + 8.))
                                .whitespace_nowrap()
                                .border_color(line2)
                                .rounded(px(8.))
                                .hover(move |s| s.border_color(accent))
                                .ld_click(
                                    true,
                                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        this.open_recent(i, cx)
                                    }),
                                )
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(move |this, _, _, cx| {
                                        this.menu_recent = Some(i);
                                        cx.notify();
                                    }),
                                )
                                .child(ld_icon(kind_icon(entry.kind), 16.).text_color(accent))
                                .child(
                                    div()
                                        .id(("start-recent-names", i))
                                        .test_support()
                                        .aria_label(line.names.clone())
                                        .min_w_0()
                                        .truncate()
                                        .child(line.names),
                                )
                                .when(!line.place.is_empty(), |entry| {
                                    entry.child(
                                        div()
                                            .id(("start-recent-place", i))
                                            .test_support()
                                            .aria_label(line.place.clone())
                                            .flex_1()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis_start()
                                            .text_size(px(12.))
                                            .text_color(p.dim)
                                            .child(line.place),
                                    )
                                }),
                        )
                        .child(
                            h_flex()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .right(px(10.))
                                .items_center()
                                .child(
                                    ld_small_icon_button(
                                        ("start-recent-remove", i),
                                        icons::CLOSE,
                                        tr("Remove from the list"),
                                        p,
                                    )
                                    .w(px(REMOVE_WIDTH))
                                    .ld_click(
                                        true,
                                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                                            // The entry lies under the ×; the click is the
                                            // ×'s alone.
                                            cx.stop_propagation();
                                            this.remove_recent(i, cx)
                                        }),
                                    ),
                                ),
                        )
                }),
        )
        .context_menu(menu)
    }

    /// "Show all (N)" or "Show fewer", when the list is longer than `RECENT_SHOWN`.
    fn render_recent_more(&self, p: &Palette, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        (self.recent.len() > RECENT_SHOWN).then(|| {
            let label = if self.all_recent {
                tr("Show fewer").to_owned()
            } else {
                trf("Show all ({})", &[&self.recent.len()])
            };
            h_flex().child(ld_ghost("start-recent-more", label, p).ld_small().ld_click(
                true,
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.all_recent = !this.all_recent;
                    cx.notify();
                }),
            ))
        })
    }
}

/// The recent entry's × button.
const REMOVE_WIDTH: f32 = 22.;
/// A recent entry's height: one row (U13).
const RECENT_HEIGHT: f32 = 32.;
/// The recent entries shown until "Show all" (U13).
const RECENT_SHOWN: usize = 8;
const NO_RECENT: &str = tr_key("Compared pairs show up here.");
/// The welcome card's title size.
const WELCOME_TITLE_SIZE: f32 = 15.;
/// The page's group, whose drag-over lights up the form (U12).
const DROP_GROUP: &str = "start-drop";

/// The six tiles, in their grid's order (U12).
const TILES: [StartMode; 6] = [
    StartMode::Files,
    StartMode::Folders,
    StartMode::Merge,
    StartMode::Hex,
    StartMode::Table,
    StartMode::Picture,
];

/// The form's "Paste text…" button: the untitled pair, under the old text card's id.
const PASTE_TEXT: (&str, &str) = ("start-mode-text", tr_key("Paste text…"));

/// A tile's id, title and one-line description.
fn mode_copy(mode: StartMode) -> (&'static str, &'static str, &'static str) {
    match mode {
        StartMode::Picture => (
            "start-mode-picture",
            tr_key("Compare pictures"),
            tr_key("PNG, JPEG, GIF, BMP, WebP, ICO, TIFF or SVG, pixel by pixel."),
        ),
        StartMode::Table => (
            "start-mode-table",
            tr_key("Compare tables"),
            tr_key(
                "CSV, TSV or Excel (.xlsx) sheets, cell by cell: rows paired by key or by order.",
            ),
        ),
        StartMode::Hex => (
            "start-mode-hex",
            tr_key("Compare binary files"),
            tr_key("Byte by byte, side by side, for any file."),
        ),
        StartMode::Folders => (
            "start-mode-folders",
            tr_key("Compare folders"),
            tr_key("Two folder trees: what differs, what is missing, copy and delete."),
        ),
        StartMode::Merge => (
            "start-mode-merge",
            tr_key("3-way merge"),
            tr_key("Combine two edited copies of a file with their common original."),
        ),
        StartMode::Files => (
            "start-mode-files",
            tr_key("Compare files"),
            tr_key("Two text files side by side: changes, moved lines, edits and saves."),
        ),
    }
}

/// A tile's icon: its kind of compare.
fn mode_icon(mode: StartMode) -> IconName {
    match mode {
        StartMode::Files => icons::KIND_FILES,
        StartMode::Folders => icons::KIND_FOLDERS,
        StartMode::Merge => icons::KIND_MERGE,
        StartMode::Hex => icons::KIND_HEX,
        StartMode::Table => icons::KIND_TABLE,
        StartMode::Picture => icons::KIND_PICTURE,
    }
}

/// A recent entry's icon: its kind's tile's.
fn kind_icon(kind: RecentKind) -> IconName {
    mode_icon(match kind {
        RecentKind::Files => StartMode::Files,
        RecentKind::Folders => StartMode::Folders,
        RecentKind::Merge => StartMode::Merge,
        RecentKind::Hex => StartMode::Hex,
        RecentKind::Table => StartMode::Table,
        RecentKind::Picture => StartMode::Picture,
    })
}

fn heading(text: &'static str, p: &Palette) -> Div {
    div()
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(p.accent)
        .child(text)
}

/// A sheet row's label in the UI's language. The Thai sits in `SHORTCUTS_TH`, in the order of
/// `SHORTCUTS`: it is not in the UI table, where "Help" would read as a tab title.
fn sheet_label(english: &'static str) -> &'static str {
    match HelpLang::current() {
        HelpLang::En => english,
        HelpLang::Th => SHORTCUTS
            .iter()
            .zip(topics::shortcut_labels(HelpLang::Th))
            .find(|((_, label), _)| *label == english)
            .map_or(english, |(_, thai)| thai),
    }
}

/// The short sheet (`START_SHORTCUTS`), its keys from the real bindings.
fn render_shortcuts(p: &Palette, fonts: &Fonts) -> impl IntoElement {
    v_flex()
        .id("start-shortcuts")
        .test_support()
        .gap(px(6.))
        .children(
            start_shortcut_rows(&crate::app::key_bindings())
                .into_iter()
                .enumerate()
                .map(|(i, (keys, label))| {
                    h_flex()
                        .id(("start-shortcut", i))
                        .test_support()
                        .gap(px(10.))
                        .items_start()
                        .child(
                            h_flex()
                                .w(px(170.))
                                .flex_none()
                                .flex_wrap()
                                .gap(px(4.))
                                .children(
                                    keys.split(" / ")
                                        .map(|combo| ld_kbd(combo.to_owned(), &fonts.code, p)),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(12.5))
                                .text_color(p.muted)
                                .child(sheet_label(label)),
                        )
                }),
        )
}

/// A typed or pasted path without surrounding whitespace or the quotes Explorer's
/// "Copy as path" adds.
pub fn clean_path(text: &str) -> &str {
    let text = text.trim();
    text.strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .map_or(text, str::trim)
}

impl Render for StartView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current(cx);
        let fonts = Fonts::current(cx);
        let width = window.viewport_size().width;
        let one = home_columns(width) == 1;
        let welcome = self
            .welcome()
            .map(|lang| self.render_welcome(lang, &p, &fonts, cx));
        let left = v_flex()
            .id("start-col-left")
            .test_support()
            .gap(px(12.))
            .min_w_0()
            .map(|d| {
                if one {
                    d.w_full()
                } else {
                    d.flex_basis(relative(LEFT_SHARE))
                }
            })
            .children(welcome)
            .child(heading(tr("New compare"), &p))
            .child(self.render_form(&p, cx))
            .child(heading(tr("Or pick a type"), &p))
            .child(self.render_tiles(tile_columns(width), &p, cx));
        let link = ld_ghost("start-shortcuts-help", tr("All shortcuts in Help (F1)"), &p)
            .ld_small()
            .ld_click(
                true,
                cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(StartEvent::OpenHelp(Some("keys")))),
            );
        let right = v_flex()
            .id("start-col-right")
            .test_support()
            .gap(px(12.))
            .min_w_0()
            .map(|d| if one { d.w_full() } else { d.flex_1() })
            .child(self.render_recent_heading(&p, cx))
            .child(self.render_recent(&p, cx))
            .children(self.render_recent_more(&p, cx))
            .child(heading(tr("Keyboard shortcuts"), &p))
            .child(render_shortcuts(&p, &fonts))
            .child(h_flex().child(link));
        div()
            .id("start-view")
            .test_support()
            .group(DROP_GROUP)
            .size_full()
            .overflow_y_scroll()
            .bg(p.bg)
            .text_color(p.text)
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.drop_paths(paths.paths(), None, window, cx)
            }))
            .child(
                div()
                    .flex()
                    .when(one, |d| d.flex_col())
                    .gap(px(28.))
                    .px(px(PAGE_PADDING))
                    .py(px(22.))
                    .max_w(px(PAGE_WIDTH))
                    .child(left)
                    .child(right),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_card_names_workbooks() {
        assert!(mode_copy(StartMode::Table).2.contains(".xlsx"));
    }

    #[test]
    fn typed_paths_lose_whitespace_and_copy_as_path_quotes() {
        assert_eq!(clean_path("  C:\\a b\\x.txt \n"), "C:\\a b\\x.txt");
        assert_eq!(clean_path("\"C:\\a b\\x.txt\""), "C:\\a b\\x.txt");
        assert_eq!(clean_path(" \"D:\\dir\" "), "D:\\dir");
        assert_eq!(clean_path("\""), "\"");
    }

    #[test]
    fn the_shortcut_sheet_reads_the_real_key_bindings() {
        let rows = shortcut_rows(&crate::app::key_bindings());
        assert_eq!(rows.len(), SHORTCUTS.len());
        let keys = |label: &str| {
            rows.iter()
                .find(|(_, l)| *l == label)
                .map(|(k, _)| k.as_str())
                .unwrap()
        };
        assert_eq!(keys("Previous or next difference"), "F7 / F8");
        assert_eq!(keys("First or last difference"), "Ctrl+Home / Ctrl+End");
        assert_eq!(keys("Undo or redo"), "Ctrl+Z / Ctrl+Y");
        assert_eq!(
            keys("Copy the block to the right or left"),
            "Alt+Right / Alt+Left"
        );
        assert_eq!(keys("Help"), "F1", "F1 opens the help from any tab");
        for ((actions, label), (keys, _)) in SHORTCUTS.iter().zip(&rows) {
            assert_eq!(
                keys.split(" / ").count(),
                actions.len(),
                "every action of {label:?} has a binding: {keys:?}"
            );
        }
    }

    #[test]
    fn every_tile_has_its_own_id_and_icon_and_the_merge_tile_reads_as_in_localdiff() {
        let mut ids: Vec<&str> = TILES.iter().map(|m| mode_copy(*m).0).collect();
        ids.push(PASTE_TEXT.0);
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 7, "six distinct tiles and Paste text…");
        assert_eq!(
            mode_copy(StartMode::Merge),
            (
                "start-mode-merge",
                "3-way merge",
                "Combine two edited copies of a file with their common original."
            )
        );
        let icons: Vec<(&str, IconName)> = TILES
            .iter()
            .map(|m| (mode_copy(*m).1, mode_icon(*m)))
            .collect();
        assert_eq!(
            icons,
            [
                ("Compare files", IconName::Files),
                ("Compare folders", IconName::FolderTree),
                ("3-way merge", IconName::GitMerge),
                ("Compare binary files", IconName::Binary),
                ("Compare tables", IconName::Sheet),
                ("Compare pictures", IconName::Image),
            ]
        );
    }

    #[test]
    fn paste_text_takes_the_text_cards_id() {
        assert_eq!(PASTE_TEXT, ("start-mode-text", "Paste text…"));
    }

    #[test]
    fn a_recent_entry_shows_its_kinds_icon() {
        use RecentKind::*;
        let icons = [Files, Folders, Merge, Hex, Table, Picture].map(kind_icon);
        assert_eq!(
            icons,
            [
                IconName::Files,
                IconName::FolderTree,
                IconName::GitMerge,
                IconName::Binary,
                IconName::Sheet,
                IconName::Image,
            ]
        );
    }

    #[test]
    fn home_goes_to_one_column_on_a_narrow_window() {
        assert_eq!(home_columns(px(1400.)), 2);
        assert_eq!(home_columns(px(1000.)), 2);
        assert_eq!(home_columns(px(999.)), 1);
    }

    #[test]
    fn the_tiles_take_three_columns_unless_the_left_column_is_narrow() {
        // Two columns: the left one is 53.5% of a page at most 1400 px wide with 28 px sides.
        assert_eq!(tile_columns(px(1400.)), 3, "a 719 px left column");
        assert_eq!(tile_columns(px(1920.)), 3, "the page stops at 1400 px");
        assert_eq!(tile_columns(px(1200.)), 2, "a 612 px left column");
        assert_eq!(tile_columns(px(1260.)), 3, "644 px");
        assert_eq!(tile_columns(px(1250.)), 2, "639 px");
        // One column: the whole page.
        assert_eq!(tile_columns(px(999.)), 3);
        assert_eq!(tile_columns(px(700.)), 3, "644 px");
        assert_eq!(tile_columns(px(690.)), 2, "634 px");
    }

    #[test]
    fn the_start_sheet_is_eight_rows_of_the_full_sheet_in_order() {
        let labels: Vec<&str> = START_SHORTCUTS.iter().map(|(_, label)| *label).collect();
        assert_eq!(
            labels,
            [
                "Previous or next difference",
                "Copy the block to the right or left",
                "Undo or redo",
                "Save the edited side",
                "Find, then the next match",
                "Reload the files, or compare the folders again",
                "New tab or close tab",
                "Help",
            ]
        );
        for row in START_SHORTCUTS {
            assert!(SHORTCUTS.contains(row), "{:?} is a row of SHORTCUTS", row.1);
        }
        let rows = start_shortcut_rows(&crate::app::key_bindings());
        assert_eq!(rows.len(), 8);
        assert_eq!(
            rows[0],
            ("F7 / F8".to_owned(), "Previous or next difference")
        );
        assert_eq!(rows[7], ("F1".to_owned(), "Help"));
    }

    #[test]
    fn the_welcome_card_is_due_until_start_welcome_reaches_its_revision() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert!(welcome_due(None), "no data folder");
        assert!(welcome_due(Some(&path)), "no file");
        let set = |text: &str| std::fs::write(&path, text).unwrap();
        set(r#"{ "start.welcome": 0 }"#);
        assert!(welcome_due(Some(&path)), "below");
        set(r#"{ "start.welcome": 1 }"#);
        assert!(!welcome_due(Some(&path)), "seen");
        set(r#"{ "start.welcome": 2.0 }"#);
        assert!(!welcome_due(Some(&path)), "a later revision");
        set(r#"{ "start.welcome": "yes" }"#);
        assert!(welcome_due(Some(&path)), "not a number");
        set("not json {{");
        assert!(welcome_due(Some(&path)), "garbage");
        std::fs::remove_file(&path).unwrap();
        crate::settings::set_u64(&path, WELCOME_KEY, WELCOME_REV).unwrap();
        assert!(!welcome_due(Some(&path)), "as a dismissal writes it");
    }
}
