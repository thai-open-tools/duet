//! The window root: a tab strip over start, folder, text, hex, table, picture and merge tabs.

use std::path::{Path, PathBuf};

use duet_core::kind::CompareKind;
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{
    App, AsyncWindowContext, ClickEvent, Context, Entity, FocusHandle, Focusable, FontWeight,
    KeyBinding, MouseButton, MouseDownEvent, Pixels, ScrollHandle, Subscription, Task, Window,
    WindowAppearance, actions, div, prelude::*, px, transparent_black,
};

use crate::app::{APP_NAME, recent_file, startup_elapsed};
use crate::folder::model::FolderFilter;
use crate::folder::prefs::FolderPrefs;
use crate::folder::view::{FolderEvent, FolderView};
use crate::help::HelpView;
use crate::hex::view::{HexEvent, HexView};
use crate::launch::Launch;
use crate::merge::view::MergeView;
use crate::palette::Palette;
use crate::picture::view::{PictureEvent, PictureView};
use crate::recent::{self, Recent, RecentKind};
use crate::start::{StartEvent, StartView};
use crate::table::view::{TableEvent, TableView};
use crate::tabs::{TabKey, TabSet, badge_text};
use crate::text::view::{TextEvent, TextView};
use crate::ui::brand::{brand_image, brand_size};
use crate::ui::fonts::{Fonts, UI_SIZE};
use crate::ui::icons;
use crate::ui::style::{LdExt, TAB_RADIUS, ld_dot, ld_icon_button, ld_pill, ld_small_icon_button};

mod merge;

use self::merge::Guarded;

actions!(workspace, [CloseTab, NextTab, NewTab, OpenHelp]);

pub const CONTEXT: &str = "Workspace";

/// Why F9 did nothing in a tab with unsaved edits: the swapped pair has a tab of its own.
const OPEN_ELSEWHERE: &str = "That pair is already open in another tab.";

/// The header's "Works offline" pill needs room; it hides below 1100 px.
pub fn header_pill_visible(width: Pixels) -> bool {
    width >= px(1100.)
}

pub fn key_bindings() -> Vec<KeyBinding> {
    let c = Some(CONTEXT);
    vec![
        KeyBinding::new("ctrl-w", CloseTab, c),
        KeyBinding::new("ctrl-tab", NextTab, c),
        KeyBinding::new("ctrl-t", NewTab, c),
        // On the root, so it works from any tab, also with the caret in a field (H1).
        KeyBinding::new("f1", OpenHelp, c),
    ]
}

pub enum TabItem {
    Start(Entity<StartView>),
    Folder(Entity<FolderView>),
    Text(Entity<TextView>),
    Hex(Entity<HexView>),
    Table(Entity<TableView>),
    Picture(Entity<PictureView>),
    Merge(Entity<MergeView>),
    Help(Entity<HelpView>),
}

pub struct Workspace {
    tabs: TabSet<TabItem>,
    next_start: u32,
    focus: FocusHandle,
    subscriptions: Vec<Subscription>,
    /// (text tab, folder tab it was opened from), for Esc in the text tab.
    origins: Vec<(TabKey, TabKey)>,
    ready_logged: bool,
    /// The window title last set; setting it costs a title-bar repaint, so only on change.
    title: String,
    /// The tab list's scroll, and the (active tab, tab count, window width in whole pixels) it
    /// last brought the active tab into view for.
    tab_scroll: ScrollHandle,
    scrolled_to: Option<(usize, usize, i32)>,
    /// Frames left that ask the list to show the active tab.
    tab_scroll_frames: u8,
}

impl Workspace {
    pub fn new(launch: Launch, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut workspace = Self {
            tabs: TabSet::default(),
            next_start: 0,
            focus: cx.focus_handle(),
            subscriptions: Vec::new(),
            origins: Vec::new(),
            ready_logged: false,
            title: String::new(),
            tab_scroll: ScrollHandle::new(),
            scrolled_to: None,
            tab_scroll_frames: 0,
        };
        workspace.open_launch(launch, window, cx);
        // Windows switching between light and dark is followed until the user picks a mode.
        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            let dark = matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            );
            crate::theme::system_changed(dark, cx);
        });
        workspace.subscriptions.push(appearance);
        let this = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            let Some(this) = this.upgrade() else {
                return true;
            };
            if !this.read(cx).any_dirty(cx) {
                return true;
            }
            let ok = this.update(cx, |w, cx| w.confirm_close_all(window, cx));
            window
                .spawn(cx, async move |cx| {
                    if ok.await {
                        let _ = cx.update(|window, _| window.remove_window());
                    }
                })
                .detach();
            false
        });
        workspace
    }

    pub fn tabs(&self) -> &TabSet<TabItem> {
        &self.tabs
    }

    /// Test hook for switching tabs without hit-testing the strip.
    pub fn tabs_mut_for_test(&mut self) -> &mut TabSet<TabItem> {
        &mut self.tabs
    }

    pub fn open_launch(&mut self, launch: Launch, window: &mut Window, cx: &mut Context<Self>) {
        match launch {
            Launch::Start => self.open_start(None, window, cx),
            Launch::Invalid(message) => self.open_start(Some(message), window, cx),
            Launch::Folders { left, right } => self.open_folders(left, right, window, cx),
            Launch::Files { left, right } => self.open_files(left, right, None, window, cx),
            Launch::Hex { left, right } => self.open_hex(left, right, None, window, cx),
            Launch::Table { left, right } => self.open_table(left, right, None, window, cx),
            Launch::Picture { left, right } => self.open_picture(left, right, None, window, cx),
            Launch::Merge(paths) => self.open_merge(paths, window, cx),
        }
    }

    pub fn open_start(
        &mut self,
        error: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.new(|cx| StartView::new(error, window, cx));
        self.subscriptions.push(cx.subscribe_in(
            &view,
            window,
            |this, _, event: &StartEvent, window, cx| match event {
                StartEvent::Compare(launch) => this.open_launch(launch.clone(), window, cx),
                StartEvent::OpenHelp(topic) => this.open_help_at(*topic, window, cx),
                StartEvent::WelcomeDismissed => this.hide_welcome(cx),
            },
        ));
        let key = TabKey::Start(self.next_start);
        self.next_start += 1;
        self.tabs.open(key, || TabItem::Start(view));
        self.focus_active(window, cx);
        cx.notify();
    }

    pub fn open_folders(
        &mut self,
        left: PathBuf,
        right: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.remember(RecentKind::Folders, &left, &right, cx);
        let key = TabKey::Folders(left.clone(), right.clone());
        if self.tabs.find(&key).is_none() {
            let options =
                FolderPrefs::load(crate::settings::settings_file(cx).as_deref()).options();
            let view = cx.new(|cx| FolderView::new(left, right, options, window, cx));
            self.subscriptions.push(cx.subscribe_in(
                &view,
                window,
                |this, view, event: &FolderEvent, window, cx| match event {
                    FolderEvent::OpenFiles { left, right, kind } => {
                        // Looked up now, not captured: a swap changes the folder tab's key.
                        let origin =
                            this.key_of(|item| matches!(item, TabItem::Folder(v) if v == view));
                        let (left, right) = (left.clone(), right.clone());
                        match kind {
                            Some(CompareKind::Hex) => {
                                this.open_hex(left, right, origin, window, cx)
                            }
                            Some(CompareKind::Table) => {
                                this.open_table(left, right, origin, window, cx)
                            }
                            Some(CompareKind::Picture) => {
                                this.open_picture(left, right, origin, window, cx)
                            }
                            _ => this.open_files(left, right, origin, window, cx),
                        }
                    }
                    FolderEvent::Swapped { left, right } | FolderEvent::Rooted { left, right } => {
                        // Found by its view, never by its old key: keys only name pairs.
                        let new = TabKey::Folders(left.clone(), right.clone());
                        if let Some(index) =
                            this.index_of(|item| matches!(item, TabItem::Folder(v) if v == view))
                        {
                            // A folder tab holds no unsaved edits, so the re-key always lands.
                            this.rekey(index, new, window, cx);
                        }
                        if matches!(event, FolderEvent::Rooted { .. }) {
                            this.remember(RecentKind::Folders, left, right, cx);
                        }
                        cx.notify();
                    }
                },
            ));
            self.subscriptions
                .push(cx.observe(&view, |_, _, cx| cx.notify()));
            self.tabs.open(key, || TabItem::Folder(view));
        } else {
            self.tabs.open(key, || unreachable!("the tab exists"));
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    pub fn open_files(
        &mut self,
        left: PathBuf,
        right: PathBuf,
        origin: Option<TabKey>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Files opened from a folder compare would push the folder pair off the list.
        if origin.is_none() {
            self.remember(RecentKind::Files, &left, &right, cx);
        }
        let key = TabKey::Files(left.clone(), right.clone());
        if let Some(origin) = origin {
            self.origins.retain(|(files, _)| files != &key);
            self.origins.push((key.clone(), origin));
        }
        if self.tabs.find(&key).is_none() {
            let view = cx.new(|cx| TextView::new(left, right, cx));
            self.subscriptions.push(cx.subscribe_in(
                &view,
                window,
                |this, view, event: &TextEvent, window, cx| {
                    this.on_text_event(view, event, window, cx)
                },
            ));
            self.subscriptions
                .push(cx.observe(&view, |_, _, cx| cx.notify()));
            self.tabs.open(key.clone(), || TabItem::Text(view));
        } else {
            self.tabs
                .open(key.clone(), || unreachable!("the tab exists"));
        }
        // Back shows while the pair has an origin, also when the tab was reopened without one.
        let has_origin = self.origins.iter().any(|(files, _)| *files == key);
        if let Some(TabItem::Text(view)) = self.tabs.active() {
            view.update(cx, |view, cx| view.set_has_origin(has_origin, cx));
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    pub fn open_hex(
        &mut self,
        left: PathBuf,
        right: PathBuf,
        origin: Option<TabKey>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Like files, a pair opened from a folder compare stays out of the recent list.
        if origin.is_none() {
            self.remember(RecentKind::Hex, &left, &right, cx);
        }
        let key = TabKey::Hex(left.clone(), right.clone());
        if let Some(origin) = origin {
            self.origins.retain(|(hex, _)| hex != &key);
            self.origins.push((key.clone(), origin));
        }
        if self.tabs.find(&key).is_none() {
            let view = cx.new(|cx| HexView::new(left, right, window, cx));
            self.subscriptions.push(cx.subscribe_in(
                &view,
                window,
                |this, view, event: &HexEvent, window, cx| {
                    this.on_hex_event(view, event, window, cx)
                },
            ));
            self.subscriptions
                .push(cx.observe(&view, |_, _, cx| cx.notify()));
            self.tabs.open(key, || TabItem::Hex(view));
        } else {
            self.tabs.open(key, || unreachable!("the tab exists"));
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    pub fn open_table(
        &mut self,
        left: PathBuf,
        right: PathBuf,
        origin: Option<TabKey>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Like files, a pair opened from a folder compare stays out of the recent list.
        if origin.is_none() {
            self.remember(RecentKind::Table, &left, &right, cx);
        }
        let key = TabKey::Table(left.clone(), right.clone());
        if let Some(origin) = origin {
            self.origins.retain(|(table, _)| table != &key);
            self.origins.push((key.clone(), origin));
        }
        if self.tabs.find(&key).is_none() {
            let view = cx.new(|cx| TableView::new(left, right, window, cx));
            self.subscriptions.push(cx.subscribe_in(
                &view,
                window,
                |this, view, event: &TableEvent, window, cx| {
                    this.on_table_event(view, event, window, cx)
                },
            ));
            self.subscriptions
                .push(cx.observe(&view, |_, _, cx| cx.notify()));
            self.tabs.open(key, || TabItem::Table(view));
        } else {
            self.tabs.open(key, || unreachable!("the tab exists"));
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    pub fn open_picture(
        &mut self,
        left: PathBuf,
        right: PathBuf,
        origin: Option<TabKey>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Like files, a pair opened from a folder compare stays out of the recent list.
        if origin.is_none() {
            self.remember(RecentKind::Picture, &left, &right, cx);
        }
        let key = TabKey::Picture(left.clone(), right.clone());
        if let Some(origin) = origin {
            self.origins.retain(|(picture, _)| picture != &key);
            self.origins.push((key.clone(), origin));
        }
        if self.tabs.find(&key).is_none() {
            let view = cx.new(|cx| PictureView::new(left, right, window, cx));
            self.subscriptions.push(cx.subscribe_in(
                &view,
                window,
                |this, view, event: &PictureEvent, window, cx| {
                    this.on_picture_event(view, event, window, cx)
                },
            ));
            self.subscriptions
                .push(cx.observe(&view, |_, _, cx| cx.notify()));
            self.tabs.open(key, || TabItem::Picture(view));
        } else {
            self.tabs.open(key, || unreachable!("the tab exists"));
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// A text tab's "Open in hex": the same pair in a hex tab beside it, which goes back to the
    /// folder the text tab came from, if any.
    fn open_text_pair_as(
        &mut self,
        view: &Entity<TextView>,
        kind: CompareKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self.key_of(|item| matches!(item, TabItem::Text(v) if v == view)) else {
            return;
        };
        let origin = self
            .origins
            .iter()
            .find(|(files, _)| *files == key)
            .map(|(_, folder)| folder.clone());
        let request = &view.read(cx).request;
        let (left, right) = (request.left.clone(), request.right.clone());
        match kind {
            CompareKind::Hex => self.open_hex(left, right, origin, window, cx),
            CompareKind::Table => self.open_table(left, right, origin, window, cx),
            CompareKind::Picture => self.open_picture(left, right, origin, window, cx),
            CompareKind::Text | CompareKind::Folder | CompareKind::Merge => {}
        }
    }

    /// Moves the pair to the top of the recent list, which open Start tabs then show. A write
    /// failure is logged, not shown: the compare itself worked.
    fn remember(&self, kind: RecentKind, left: &Path, right: &Path, cx: &mut Context<Self>) {
        // An untitled side has no file to reopen (R15).
        if left.as_os_str().is_empty() || right.as_os_str().is_empty() {
            return;
        }
        let entry = Recent {
            kind,
            left: left.to_path_buf(),
            right: right.to_path_buf(),
            base: None,
        };
        self.remember_entry(entry, cx);
    }

    fn remember_entry(&self, entry: Recent, cx: &mut Context<Self>) {
        let Some(path) = recent_file(cx) else {
            return;
        };
        if let Err(err) = recent::record(&path, entry) {
            tracing::warn!(%err, "could not update the recent list");
        }
        for (_, _, item) in self.tabs.iter() {
            if let TabItem::Start(view) = item {
                view.update(cx, |view, cx| view.reload_recent(cx));
            }
        }
    }

    /// The badge for tab `index`; None for the start screen and while comparing.
    pub fn tab_badge(&self, index: usize, cx: &App) -> Option<String> {
        let (_, _, item) = self.tabs.iter().nth(index)?;
        match item {
            TabItem::Start(_) | TabItem::Help(_) => None,
            TabItem::Text(view) => {
                let view = view.read(cx);
                if view.is_busy() {
                    return None;
                }
                match (view.model(), view.message()) {
                    (Some(model), _) => Some(badge_text(model.block_count())),
                    (None, Some("Binary files are identical.")) => Some(badge_text(0)),
                    (None, Some("Binary files differ.")) => Some("≠".to_owned()),
                    (None, _) => Some("!".to_owned()),
                }
            }
            TabItem::Hex(view) => {
                let view = view.read(cx);
                if view.is_busy() {
                    return None;
                }
                match view.diff() {
                    Some(diff) if diff.ranges.is_empty() => Some(badge_text(0)),
                    Some(_) => Some(badge_text(view.block_count())),
                    None => Some("!".to_owned()),
                }
            }
            TabItem::Table(view) => view.read(cx).badge(),
            TabItem::Picture(view) => view.read(cx).badge(),
            TabItem::Merge(view) => view.read(cx).badge(),
            TabItem::Folder(view) => {
                let view = view.read(cx);
                if view.is_busy() {
                    return None;
                }
                let counts = view.model()?.counts();
                Some(badge_text(FolderFilter::Mismatch.count(&counts)))
            }
        }
    }

    fn key_of(&self, is: impl Fn(&TabItem) -> bool) -> Option<TabKey> {
        self.tabs
            .iter()
            .find(|(_, _, item)| is(item))
            .map(|(_, key, _)| key.clone())
    }

    fn index_of(&self, is: impl Fn(&TabItem) -> bool) -> Option<usize> {
        self.tabs
            .iter()
            .find(|(_, _, item)| is(item))
            .map(|(i, _, _)| i)
    }

    /// Tab `index` now shows the pair `new` (a swap, Up or a picker): rename it and keep the
    /// folder a text tab came from. When another tab already shows `new`, that tab is
    /// activated and this one closes, as opening an open pair does. A tab with unsaved edits
    /// cannot close: then nothing changes and this returns false, and the caller takes the
    /// change back in the view, so a tab's key always names what it shows.
    fn rekey(
        &mut self,
        index: usize,
        new: TabKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(old) = self.tabs.iter().nth(index).map(|(_, key, _)| key.clone()) else {
            return true;
        };
        match self.tabs.find(&new) {
            Some(existing) if existing != index => {
                if self.tab_dirty(index, cx) {
                    return false;
                }
                self.origins.retain(|(files, _)| *files != old);
                for (_, folder) in &mut self.origins {
                    if *folder == old {
                        *folder = new.clone();
                    }
                }
                self.release_picture_images(index, window, cx);
                self.tabs.close(index);
                if let Some(at) = self.tabs.find(&new) {
                    self.tabs.activate(at);
                }
                self.focus_active(window, cx);
                return true;
            }
            _ => self.tabs.rekey(index, new.clone()),
        }
        for (files, folder) in &mut self.origins {
            if *files == old {
                *files = new.clone();
            }
            if *folder == old {
                *folder = new.clone();
            }
        }
        true
    }

    fn on_text_event(
        &mut self,
        view: &Entity<TextView>,
        event: &TextEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextEvent::Back => {
                let Some(key) = self.key_of(|item| matches!(item, TabItem::Text(v) if v == view))
                else {
                    return;
                };
                let origin = self
                    .origins
                    .iter()
                    .find(|(files, _)| *files == key)
                    .map(|(_, folder)| folder.clone());
                if let Some(index) = origin.and_then(|folder| self.tabs.find(&folder)) {
                    self.tabs.activate(index);
                    self.focus_active(window, cx);
                    cx.notify();
                }
            }
            TextEvent::Swapped { left, right } => {
                let Some(index) =
                    self.index_of(|item| matches!(item, TabItem::Text(v) if v == view))
                else {
                    return;
                };
                let new = TabKey::Files(left.clone(), right.clone());
                if !self.rekey(index, new, window, cx) {
                    view.update(cx, |v, cx| v.undo_swap(OPEN_ELSEWHERE, cx));
                }
                cx.notify();
            }
            TextEvent::Renamed { left, right } => {
                let Some(index) =
                    self.index_of(|item| matches!(item, TabItem::Text(v) if v == view))
                else {
                    return;
                };
                let new = TabKey::Files(left.clone(), right.clone());
                // No undo here: the file is already written, so on a refusal the tab keeps its
                // old key.
                if !self.rekey(index, new, window, cx) {
                    tracing::warn!(left = %left.display(), right = %right.display(), "Save As: the pair is open in another tab, so this tab keeps its old key");
                    view.update(cx, |v, cx| {
                        v.show_notice("Saved. This pair is already open in another tab.", cx)
                    });
                }
                cx.notify();
            }
            TextEvent::OpenAs { kind } => self.open_text_pair_as(view, *kind, window, cx),
            TextEvent::Open { left, right } => {
                let Some(index) =
                    self.index_of(|item| matches!(item, TabItem::Text(v) if v == view))
                else {
                    return;
                };
                let new = TabKey::Files(left.clone(), right.clone());
                // The pair is open in another tab: go there and leave this one as it is.
                if let Some(existing) = self.tabs.find(&new)
                    && existing != index
                {
                    self.tabs.activate(existing);
                    self.focus_active(window, cx);
                    cx.notify();
                    return;
                }
                if self.rekey(index, new, window, cx) {
                    self.remember(RecentKind::Files, left, right, cx);
                    view.update(cx, |v, cx| v.load_paths(left.clone(), right.clone(), cx));
                }
                cx.notify();
            }
        }
    }

    fn on_hex_event(
        &mut self,
        view: &Entity<HexView>,
        event: &HexEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.index_of(|item| matches!(item, TabItem::Hex(v) if v == view)) else {
            return;
        };
        match event {
            HexEvent::Back => {
                let Some(key) = self.tabs.iter().nth(index).map(|(_, k, _)| k.clone()) else {
                    return;
                };
                let origin = self
                    .origins
                    .iter()
                    .find(|(hex, _)| *hex == key)
                    .map(|(_, folder)| folder.clone());
                if let Some(at) = origin.and_then(|folder| self.tabs.find(&folder)) {
                    self.tabs.activate(at);
                    self.focus_active(window, cx);
                    cx.notify();
                }
            }
            HexEvent::Swapped { left, right } => {
                // A hex tab holds no edits, so the re-key always lands.
                self.rekey(index, TabKey::Hex(left.clone(), right.clone()), window, cx);
                cx.notify();
            }
            HexEvent::Open { left, right } => {
                let new = TabKey::Hex(left.clone(), right.clone());
                // The pair is open in another tab: go there and leave this one as it is.
                if let Some(existing) = self.tabs.find(&new)
                    && existing != index
                {
                    self.tabs.activate(existing);
                    self.focus_active(window, cx);
                    cx.notify();
                    return;
                }
                if self.rekey(index, new, window, cx) {
                    self.remember(RecentKind::Hex, left, right, cx);
                    view.update(cx, |v, cx| v.load_paths(left.clone(), right.clone(), cx));
                }
                cx.notify();
            }
        }
    }

    fn on_table_event(
        &mut self,
        view: &Entity<TableView>,
        event: &TableEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.index_of(|item| matches!(item, TabItem::Table(v) if v == view))
        else {
            return;
        };
        match event {
            TableEvent::Back => {
                let Some(key) = self.tabs.iter().nth(index).map(|(_, k, _)| k.clone()) else {
                    return;
                };
                let origin = self
                    .origins
                    .iter()
                    .find(|(table, _)| *table == key)
                    .map(|(_, folder)| folder.clone());
                if let Some(at) = origin.and_then(|folder| self.tabs.find(&folder)) {
                    self.tabs.activate(at);
                    self.focus_active(window, cx);
                    cx.notify();
                }
            }
            TableEvent::Swapped { left, right } => {
                // A table tab holds no edits, so the re-key always lands.
                self.rekey(
                    index,
                    TabKey::Table(left.clone(), right.clone()),
                    window,
                    cx,
                );
                cx.notify();
            }
            TableEvent::Open { left, right } => {
                let new = TabKey::Table(left.clone(), right.clone());
                // The pair is open in another tab: go there and leave this one as it is.
                if let Some(existing) = self.tabs.find(&new)
                    && existing != index
                {
                    self.tabs.activate(existing);
                    self.focus_active(window, cx);
                    cx.notify();
                    return;
                }
                if self.rekey(index, new, window, cx) {
                    self.remember(RecentKind::Table, left, right, cx);
                    view.update(cx, |v, cx| v.load_paths(left.clone(), right.clone(), cx));
                }
                cx.notify();
            }
        }
    }

    fn on_picture_event(
        &mut self,
        view: &Entity<PictureView>,
        event: &PictureEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.index_of(|item| matches!(item, TabItem::Picture(v) if v == view))
        else {
            return;
        };
        match event {
            PictureEvent::Back => {
                let Some(key) = self.tabs.iter().nth(index).map(|(_, k, _)| k.clone()) else {
                    return;
                };
                let origin = self
                    .origins
                    .iter()
                    .find(|(picture, _)| *picture == key)
                    .map(|(_, folder)| folder.clone());
                if let Some(at) = origin.and_then(|folder| self.tabs.find(&folder)) {
                    self.tabs.activate(at);
                    self.focus_active(window, cx);
                    cx.notify();
                }
            }
            PictureEvent::Swapped { left, right } => {
                // A picture tab holds no edits, so the re-key always lands.
                self.rekey(
                    index,
                    TabKey::Picture(left.clone(), right.clone()),
                    window,
                    cx,
                );
                cx.notify();
            }
            PictureEvent::Open { left, right } => {
                let new = TabKey::Picture(left.clone(), right.clone());
                // The pair is open in another tab: go there and leave this one as it is.
                if let Some(existing) = self.tabs.find(&new)
                    && existing != index
                {
                    self.tabs.activate(existing);
                    self.focus_active(window, cx);
                    cx.notify();
                    return;
                }
                if self.rekey(index, new, window, cx) {
                    self.remember(RecentKind::Picture, left, right, cx);
                    view.update(cx, |v, cx| v.load_paths(left.clone(), right.clone(), cx));
                }
                cx.notify();
            }
        }
    }

    /// Gives the active tab the keys. A Start tab also reads the recent list again, which a
    /// compare elsewhere may have changed.
    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = match self.tabs.active() {
            Some(TabItem::Start(view)) => {
                view.update(cx, |view, cx| {
                    view.reload_recent(cx);
                    view.reload_welcome(cx);
                });
                view.focus_handle(cx)
            }
            Some(TabItem::Folder(view)) => view.focus_handle(cx),
            Some(TabItem::Text(view)) => view.focus_handle(cx),
            Some(TabItem::Hex(view)) => view.focus_handle(cx),
            Some(TabItem::Table(view)) => view.focus_handle(cx),
            Some(TabItem::Picture(view)) => view.focus_handle(cx),
            Some(TabItem::Merge(view)) => view.focus_handle(cx),
            Some(TabItem::Help(view)) => view.focus_handle(cx),
            None => self.focus.clone(),
        };
        window.focus(&handle, cx);
    }

    /// Some text tab holds unsaved edits.
    pub fn any_dirty(&self, cx: &App) -> bool {
        self.tabs.iter().any(|(i, _, _)| self.tab_dirty(i, cx))
    }

    /// Tab `index` is a text or merge tab with unsaved edits.
    pub fn tab_dirty(&self, index: usize, cx: &App) -> bool {
        self.tabs.iter().any(|(i, _, item)| {
            i == index
                && match item {
                    TabItem::Text(v) => v.read(cx).is_dirty(),
                    TabItem::Merge(v) => v.read(cx).is_dirty(),
                    _ => false,
                }
        })
    }

    /// F1 and the `?` button: the window's one Help tab, opened or found again (H1).
    pub fn open_help(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.find(&TabKey::Help).is_none() {
            let view = cx.new(|cx| HelpView::new(window, cx));
            self.tabs.open(TabKey::Help, || TabItem::Help(view));
        } else {
            self.tabs
                .open(TabKey::Help, || unreachable!("the tab exists"));
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// The Help tab, at `topic` when one is named: the Start tab's link to the keys (U12). A
    /// search that hides the topic is cleared first.
    pub fn open_help_at(
        &mut self,
        topic: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_help(window, cx);
        if let (Some(topic), Some(TabItem::Help(view))) = (topic, self.tabs.active()) {
            view.update(cx, |help, cx| {
                if !help.go_to(topic, cx) {
                    help.set_query("", window, cx);
                    help.go_to(topic, cx);
                }
            });
        }
    }

    /// A Start tab's welcome card was dismissed: every Start tab hides it (U15).
    fn hide_welcome(&self, cx: &mut Context<Self>) {
        for (_, _, item) in self.tabs.iter() {
            if let TabItem::Start(view) = item {
                view.update(cx, |view, cx| view.hide_welcome(cx));
            }
        }
    }

    fn render_header(
        &self,
        p: &Palette,
        fonts: &Fonts,
        wide: bool,
        scale: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        h_flex()
            .id("app-header")
            .test_support()
            .flex_none()
            .h(px(44.))
            .px(px(12.))
            .gap(px(14.))
            .bg(p.panel)
            .border_b_1()
            .border_color(p.line)
            .child(
                h_flex()
                    .id("header-brand")
                    .test_support()
                    .gap(px(9.))
                    .child(
                        div()
                            .id("header-brand-icon")
                            .test_support()
                            .flex_none()
                            .size(brand_size(scale))
                            .child(gpui_kit::img(brand_image(scale)).size_full()),
                    )
                    // No letter-spacing in gpui: four letters 2.3 px apart (LocalDiff's .18em).
                    .child(
                        h_flex()
                            .gap(px(2.3))
                            .font(fonts.code.clone())
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .children(["D", "U", "E", "T"]),
                    ),
            )
            .child(div().flex_1())
            .when(wide, |d| {
                d.child(
                    h_flex()
                        .id("header-offline")
                        .test_support()
                        .h(px(24.))
                        .px(px(10.))
                        .gap(px(6.))
                        .rounded_full()
                        .border_1()
                        .border_dashed()
                        .border_color(p.line)
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child(ld_dot(7., p.ins))
                        .child("Works offline · 100% local"),
                )
            })
            .child(
                ld_icon_button("header-help", icons::HELP, "Help (F1)", p).ld_click(
                    true,
                    cx.listener(|this, _, window, cx| this.open_help(window, cx)),
                ),
            )
            .child(
                ld_icon_button(
                    "header-theme",
                    // The theme a click gives.
                    if p.dark {
                        icons::TO_LIGHT
                    } else {
                        icons::TO_DARK
                    },
                    "Switch between light and dark",
                    p,
                )
                .ld_click(
                    true,
                    cx.listener(|_, _, _, cx| {
                        crate::theme::toggle(cx);
                    }),
                ),
            )
    }

    fn render_tabs(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let active = self.tabs.active_index();
        let (del, panel3, accent) = (p.del, p.panel3, p.accent);
        let dirty: Vec<bool> = (0..self.tabs.len())
            .map(|i| self.tab_dirty(i, cx))
            .collect();
        let badges: Vec<Option<String>> = (0..self.tabs.len())
            .map(|i| self.tab_badge(i, cx))
            .collect();
        // The tabs scroll on their own, so the + after them never scrolls out of view.
        let list = h_flex()
            .id("tab-list")
            .min_w_0()
            .items_end()
            .gap(px(4.))
            .overflow_x_scroll()
            .track_scroll(&self.tab_scroll)
            .children(self.tabs.iter().map(|(i, key, _)| {
                let on = active == Some(i);
                let dirty = dirty[i];
                h_flex()
                    .id(("tab", i))
                    .test_support()
                    .relative()
                    .flex_none()
                    .max_w(px(280.))
                    .gap(px(7.))
                    .pl(px(12.))
                    .pr(px(9.))
                    .pt(px(5.))
                    .pb(px(6.))
                    .rounded_t(px(TAB_RADIUS))
                    .border_1()
                    .border_b_0()
                    .border_color(p.line)
                    .bg(if on { p.panel } else { p.panel2 })
                    .text_color(if on { p.text } else { p.muted })
                    .text_size(px(12.5))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.tabs.activate(i);
                        this.focus_active(window, cx);
                        cx.notify();
                    }))
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.request_close(i, window, cx);
                        }),
                    )
                    .child(div().min_w_0().truncate().child(key.title()))
                    .when(dirty, |d| {
                        d.child(ld_dot(7., p.accent).id(("tab-dirty", i)).test_support())
                    })
                    .children(badges[i].clone().map(|badge| {
                        let ok = badge == "ok";
                        let (fg, bg) = if ok {
                            (p.ins, p.bg_ins)
                        } else {
                            (p.modified, p.bg_mod)
                        };
                        ld_pill(badge, fg, bg).id(("tab-badge", i)).test_support()
                    }))
                    .child(
                        ld_small_icon_button(
                            ("tab-close", i),
                            icons::CLOSE,
                            "Close tab (Ctrl+W)",
                            p,
                        )
                        .w(px(20.))
                        .h(px(18.))
                        .rounded(px(4.))
                        .border_color(transparent_black())
                        .bg(transparent_black())
                        .text_color(p.dim)
                        .hover(move |s| s.text_color(del).bg(panel3))
                        .ld_click(
                            true,
                            cx.listener(move |this, _: &ClickEvent, window, cx| {
                                cx.stop_propagation();
                                this.request_close(i, window, cx);
                            }),
                        ),
                    )
                    // The active tab's 2 px accent bar; gpui borders share one colour, so it
                    // is a child.
                    .when(on, |d| {
                        d.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(2.))
                                .bg(p.accent),
                        )
                    })
            }));
        h_flex()
            .id("tab-strip")
            .test_support()
            .flex_none()
            .min_h(px(33.))
            .items_end()
            .gap(px(4.))
            .px(px(10.))
            .pt(px(5.))
            .bg(p.bg)
            .border_b_1()
            .border_color(p.line)
            .child(list)
            .child(
                ld_icon_button("tab-new", icons::NEW_TAB, "New compare (Ctrl+T)", p)
                    .w(px(34.))
                    .h(px(27.))
                    .rounded_t(px(TAB_RADIUS))
                    .rounded_b(px(0.))
                    .border_b_0()
                    .text_color(p.dim)
                    .hover(move |s| s.text_color(accent).border_color(accent))
                    .ld_click(
                        true,
                        cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.open_start(None, window, cx)
                        }),
                    ),
            )
    }

    /// Asks about every tab with unsaved edits, one after another; false if any says Cancel.
    pub fn confirm_close_all(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Task<bool> {
        let mut guards: Vec<Guarded> = self
            .tabs
            .iter()
            .filter_map(|(_, _, item)| Guarded::of(item, cx))
            .collect();
        // The first prompt opens before this returns, so a closing window shows it at once.
        let first = (!guards.is_empty()).then(|| guards.remove(0).confirm(window, cx));
        cx.spawn_in(window, async move |_, cx| {
            if let Some(first) = first
                && !first.await
            {
                return false;
            }
            for guard in guards {
                let Some(ok) = guard.confirm_async(cx) else {
                    return false;
                };
                if !ok.await {
                    return false;
                }
            }
            true
        })
    }

    /// Closes tab `index`, asking first when it holds unsaved edits.
    fn request_close(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let guard = self
            .tabs
            .iter()
            .find(|(i, _, _)| *i == index)
            .and_then(|(_, _, item)| Guarded::any(item));
        let Some(guard) = guard else {
            return self.close_tab(index, window, cx);
        };
        let ok = guard.confirm(window, cx);
        cx.spawn_in(window, async move |this, cx| {
            if ok.await {
                let _ = this.update_in(cx, |w, window, cx| {
                    // The tab may have moved while the prompt was open.
                    let at = w
                        .tabs
                        .iter()
                        .find(|(_, _, item)| guard.is(item))
                        .map(|(i, _, _)| i);
                    if let Some(i) = at {
                        w.close_tab(i, window, cx);
                    }
                });
            }
        })
        .detach();
    }

    /// A picture tab holds images in the GPU atlas, which live until they are dropped: whatever
    /// closes a tab hands them back first.
    fn release_picture_images(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.tabs.iter().find_map(|(i, _, item)| match item {
            TabItem::Picture(view) if i == index => Some(view.clone()),
            _ => None,
        });
        if let Some(view) = view {
            view.update(cx, |view, cx| view.release_images(window, cx));
        }
    }

    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.release_picture_images(index, window, cx);
        let closed = self.tabs.iter().nth(index).map(|(_, key, _)| key.clone());
        let was_active = self.tabs.active_index() == Some(index);
        self.tabs.close(index);
        // A tab opened from a folder hands the focus back to it, not to its neighbour (R13).
        if let Some(closed) = closed {
            let origin = self
                .origins
                .iter()
                .find(|(files, _)| *files == closed)
                .map(|(_, folder)| folder.clone());
            // A tab opened from the closed one has nothing to go back to (M6).
            self.origins
                .retain(|(files, folder)| *files != closed && *folder != closed);
            if let Some(at) = origin
                .filter(|_| was_active)
                .and_then(|f| self.tabs.find(&f))
            {
                self.tabs.activate(at);
            }
        }
        for (_, key, item) in self.tabs.iter() {
            if let TabItem::Text(view) = item {
                let on = self.origins.iter().any(|(files, _)| files == key);
                view.update(cx, |view, cx| view.set_has_origin(on, cx));
            }
        }
        if self.tabs.is_empty() {
            self.open_start(None, window, cx);
        } else {
            self.focus_active(window, cx);
        }
        cx.notify();
    }
}

impl Workspace {
    /// The active tab has finished its first compare, or is the Start tab: what a screenshot
    /// waits for.
    pub fn active_idle(&self, cx: &App) -> bool {
        match self.tabs.active() {
            Some(TabItem::Start(_)) | Some(TabItem::Help(_)) => true,
            Some(TabItem::Text(view)) => {
                let view = view.read(cx);
                !view.is_busy() && (view.model().is_some() || view.message().is_some())
            }
            Some(TabItem::Folder(view)) => {
                let view = view.read(cx);
                !view.is_busy() && view.model().is_some()
            }
            Some(TabItem::Hex(view)) => {
                let view = view.read(cx);
                !view.is_busy() && (view.diff().is_some() || view.message().is_some())
            }
            Some(TabItem::Table(view)) => {
                let view = view.read(cx);
                !view.is_busy()
                    && (view.diff().is_some()
                        || view.message(true).is_some()
                        || view.message(false).is_some())
            }
            Some(TabItem::Picture(view)) => {
                let view = view.read(cx);
                !view.is_busy()
                    && (view.diff().is_some()
                        || view.too_big()
                        || view.message(true).is_some()
                        || view.message(false).is_some())
            }
            Some(TabItem::Merge(view)) => !view.read(cx).is_busy(),
            None => false,
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::current(cx);
        if !self.ready_logged && self.active_idle(cx) {
            self.ready_logged = true;
            // Logged after this frame is drawn: the screenshot harness captures then.
            window.on_next_frame(|_, _| {
                tracing::info!(ms = startup_elapsed().as_millis() as u64, "workspace ready");
            });
        }
        let title = self
            .tabs
            .iter()
            .find(|(i, _, _)| Some(*i) == self.tabs.active_index())
            .map_or_else(
                || APP_NAME.to_owned(),
                |(_, key, _)| format!("{} — {APP_NAME}", key.title()),
            );
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
        // A tab opened, closed or switched to scrolls into view, whichever way it happened, and
        // so does the active tab when the window's width changes.
        // The list measures the tab against its bounds from the frame before, which a resize has
        // just changed, so the request is made on the next frame too.
        let width = f32::from(window.viewport_size().width).round() as i32;
        let shown = self
            .tabs
            .active_index()
            .map(|i| (i, self.tabs.len(), width));
        if shown != self.scrolled_to {
            self.scrolled_to = shown;
            self.tab_scroll_frames = 2;
        }
        if self.tab_scroll_frames > 0 {
            if let Some((active, _, _)) = shown {
                self.tab_scroll.scroll_to_item(active);
            }
            self.tab_scroll_frames -= 1;
            if self.tab_scroll_frames > 0 {
                window.request_animation_frame();
            }
        }
        let fonts = Fonts::current(cx);
        let wide = header_pill_visible(window.viewport_size().width);
        let header = self.render_header(&palette, &fonts, wide, window.scale_factor(), cx);
        let strip = self.render_tabs(&palette, cx);
        let content = match self.tabs.active() {
            Some(TabItem::Start(view)) => view.clone().into_any_element(),
            Some(TabItem::Folder(view)) => view.clone().into_any_element(),
            Some(TabItem::Text(view)) => view.clone().into_any_element(),
            Some(TabItem::Hex(view)) => view.clone().into_any_element(),
            Some(TabItem::Table(view)) => view.clone().into_any_element(),
            Some(TabItem::Picture(view)) => view.clone().into_any_element(),
            Some(TabItem::Merge(view)) => view.clone().into_any_element(),
            Some(TabItem::Help(view)) => view.clone().into_any_element(),
            None => div().into_any_element(),
        };
        v_flex()
            .id("workspace")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .bg(palette.bg)
            .text_color(palette.text)
            .font(fonts.ui.clone())
            .text_size(px(UI_SIZE))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                if let Some(index) = this.tabs.active_index() {
                    this.request_close(index, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| {
                this.tabs.next();
                this.focus_active(window, cx);
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, _: &NewTab, window, cx| this.open_start(None, window, cx)),
            )
            .on_action(cx.listener(|this, _: &OpenHelp, window, cx| this.open_help(window, cx)))
            .child(header)
            .child(strip)
            .child(div().flex_1().min_h_0().child(content))
    }
}
