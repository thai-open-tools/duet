//! A file changed under an open compare (R18, LocalDiff `ops:444-470`): polling, not a watcher.
//!
//! While the tab is drawn, every `WATCH_INTERVAL` the background stats both files. Each tick
//! asks for a frame, so a tab on screen that nothing redraws keeps polling, and stops when a
//! tick finds no frame since the last one (a hidden tab draws none). The stat keeps no handle
//! open, so an open tab never locks a file or its folder (P5 R5). The UI thread only compares
//! the answer with the stamp the model holds.
//!
//! A change shows only when two polls in a row saw the same unlike state, and a stat taken
//! before one of the tab's own saves landed is thrown away (PF7), so a save never reports itself.

use std::path::{Path, PathBuf};
use std::time::Duration;

use duet_core::text::document::FileStamp;
use gpui_kit::component::h_flex;
use gpui_kit::{
    AsyncApp, Context, SharedString, Task, TestSupportExt as _, WeakEntity, Window, div,
    prelude::*, px,
};

use crate::i18n::{tr, trf};
use crate::palette::Palette;
use crate::text::view::TextView;
use crate::ui::fonts::SMALL_SIZE;
use crate::ui::style::{LdExt as _, ld_button};

pub const WATCH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskChange {
    Changed,
    Gone,
}

/// What one stat of a file found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Seen {
    Stamp(FileStamp),
    Gone,
}

impl Seen {
    fn change(self) -> DiskChange {
        match self {
            Seen::Stamp(_) => DiskChange::Changed,
            Seen::Gone => DiskChange::Gone,
        }
    }
}

/// The poll's memory, per side (index 0 is the left).
#[derive(Default)]
pub(crate) struct Watch {
    task: Option<Task<()>>,
    /// A frame was drawn since the last tick.
    frame: bool,
    /// Bumped whenever an answer in flight could belong to another state: a save, a swap, a
    /// new pair.
    epoch: u64,
    /// The unlike state the last poll saw.
    pending: [Option<Seen>; 2],
    /// The unlike state two polls agreed on: the notice.
    shown: [Option<Seen>; 2],
    /// The unlike state "Ignore" dismissed.
    ignored: [Option<Seen>; 2],
}

impl Watch {
    /// One side's answer. True when what the notice shows changed.
    pub(crate) fn observe(&mut self, side: usize, seen: Seen, disk: FileStamp) -> bool {
        if seen == Seen::Stamp(disk) {
            self.pending[side] = None;
            self.ignored[side] = None;
            return self.shown[side].take().is_some();
        }
        if self.ignored[side] == Some(seen) {
            self.pending[side] = None;
            return false;
        }
        if self.pending[side] == Some(seen) {
            return self.shown[side].replace(seen) != Some(seen);
        }
        self.pending[side] = Some(seen);
        false
    }

    /// An answer taken at `epoch` is still good.
    pub(crate) fn is_current(&self, epoch: u64) -> bool {
        self.epoch == epoch
    }

    pub(crate) fn invalidate(&mut self) {
        self.epoch += 1;
    }

    /// F9: the notices move with their files.
    pub(crate) fn swap(&mut self) {
        self.invalidate();
        self.pending.swap(0, 1);
        self.shown.swap(0, 1);
        self.ignored.swap(0, 1);
    }

    /// A new compare read the files again: what was pending or shown is old news.
    pub(crate) fn reloaded(&mut self) {
        self.pending = [None; 2];
        self.shown = [None; 2];
    }

    /// A new pair: nothing carries over, not even an ignored change.
    pub(crate) fn new_pair(&mut self) {
        self.invalidate();
        self.reloaded();
        self.ignored = [None; 2];
    }

    /// The tab wrote this side's file (or gave it a new one): the stamp is ours.
    pub(crate) fn saved(&mut self, left: bool) {
        let side = usize::from(!left);
        self.invalidate();
        self.pending[side] = None;
        self.shown[side] = None;
        self.ignored[side] = None;
    }

    /// "Ignore": no notice until the file changes again.
    pub(crate) fn ignore(&mut self) {
        for side in 0..2 {
            if let Some(seen) = self.shown[side].take() {
                self.ignored[side] = Some(seen);
            }
        }
    }

    pub(crate) fn shown(&self, left: bool) -> Option<DiskChange> {
        self.shown[usize::from(!left)].map(Seen::change)
    }

    #[cfg(test)]
    pub(crate) fn is_ignored(&self, left: bool) -> bool {
        self.ignored[usize::from(!left)].is_some()
    }
}

fn stat(path: &Path) -> Option<Seen> {
    match std::fs::metadata(path) {
        Ok(meta) => Some(Seen::Stamp(FileStamp::of(&meta))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(Seen::Gone),
        // Locked, no access, a network hiccup: no news.
        Err(_) => None,
    }
}

/// What a poll starts from: the state it must still find on the way back, and the files.
struct Poll {
    epoch: u64,
    paths: [Option<PathBuf>; 2],
}

impl TextView {
    /// Called by every frame: the tab is shown, so it is watched.
    pub(crate) fn watch_frame(&mut self, cx: &mut Context<Self>) {
        self.watch.frame = true;
        if self.watch.task.is_none() {
            self.watch.task = Some(cx.spawn(async move |this, cx| {
                // The first poll is at once; the following ones every interval, until a tick
                // finds that no frame was drawn since the last.
                poll_once(&this, cx).await;
                loop {
                    cx.background_executor().timer(WATCH_INTERVAL).await;
                    let drawn = this.update(cx, |view, cx| {
                        let drawn = std::mem::take(&mut view.watch.frame);
                        if drawn {
                            // A shown tab draws the frame this asks for even when nothing else
                            // changes, so it keeps polling; a hidden one draws none (M13).
                            cx.notify();
                        } else {
                            view.watch.task = None;
                        }
                        drawn
                    });
                    if !matches!(drawn, Ok(true)) {
                        return;
                    }
                    poll_once(&this, cx).await;
                }
            }));
        }
    }

    /// The files to stat: set sides that no save of ours is writing. None while a compare runs
    /// or nothing is shown.
    fn begin_poll(&self) -> Option<Poll> {
        self.model.as_ref()?;
        if self.job.is_running() {
            return None;
        }
        let path = |p: &PathBuf| {
            (!p.as_os_str().is_empty() && !self.saving.contains(p)).then(|| p.clone())
        };
        let paths = [path(&self.request.left), path(&self.request.right)];
        paths.iter().any(Option::is_some).then_some(Poll {
            epoch: self.watch.epoch,
            paths,
        })
    }

    fn finish_poll(&mut self, epoch: u64, seen: [Option<Seen>; 2], cx: &mut Context<Self>) {
        if !self.watch.is_current(epoch) || self.job.is_running() {
            return;
        }
        let Some(model) = &self.model else {
            return;
        };
        let mut changed = false;
        for (side, seen) in seen.into_iter().enumerate() {
            if let Some(seen) = seen {
                changed |= self.watch.observe(side, seen, model.disk_stamp(side == 0));
            }
        }
        if changed {
            cx.notify();
        }
    }

    /// The bar's sentence; None when no file changed. The left side is named first.
    pub fn disk_notice(&self) -> Option<String> {
        [true, false].into_iter().find_map(|left| {
            let change = self.watch.shown(left)?;
            let path = if left {
                &self.request.left
            } else {
                &self.request.right
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let side = tr(if left { "left" } else { "right" });
            Some(match change {
                DiskChange::Changed => {
                    trf("⚠ {} ({}) was changed by another program.", &[&name, &side])
                }
                DiskChange::Gone => trf("⚠ {} ({}) was deleted or moved.", &[&name, &side]),
            })
        })
    }

    /// "Ignore": the bar goes, and comes back when the file changes again.
    pub fn ignore_disk_change(&mut self, cx: &mut Context<Self>) {
        self.watch.ignore();
        cx.notify();
    }

    /// Polls twice at once (the second is the one that confirms), without waiting for the timer.
    #[doc(hidden)]
    pub fn check_disk_for_test(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            poll_once(&this, cx).await;
            poll_once(&this, cx).await;
        })
        .detach();
    }

    /// A poll task is waiting for its next tick.
    #[doc(hidden)]
    pub fn is_watching_for_test(&self) -> bool {
        self.watch.task.is_some()
    }

    /// The disk-change bar: the sentence and its two buttons, in the find bar's look.
    pub(super) fn render_disk_notice(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        let text: SharedString = self.disk_notice()?.into();
        Some(
            h_flex()
                .id("text-disk-notice")
                .test_support()
                .flex_none()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .px(px(12.))
                .py(px(4.))
                .bg(p.panel3)
                .border_b_1()
                .border_color(p.line)
                .text_size(px(SMALL_SIZE))
                .child(div().text_color(p.modified).child(text))
                .child(ld_button("text-disk-reload", tr("Reload"), p).ld_click(
                    true,
                    cx.listener(|this, _, window: &mut Window, cx| {
                        this.reload_guarded(window, cx, |_| {})
                    }),
                ))
                .child(ld_button("text-disk-ignore", tr("Ignore"), p).ld_click(
                    true,
                    cx.listener(|this, _, _, cx| this.ignore_disk_change(cx)),
                ))
                .into_any_element(),
        )
    }
}

/// One poll: stat on the background executor, compare on the UI thread.
async fn poll_once(this: &WeakEntity<TextView>, cx: &mut AsyncApp) {
    let Ok(Some(Poll { epoch, paths })) = this.update(cx, |view, _| view.begin_poll()) else {
        return;
    };
    let seen = cx
        .background_executor()
        .spawn(async move { paths.map(|path| path.and_then(|p| stat(&p))) })
        .await;
    let _ = this.update(cx, |view, cx| view.finish_poll(epoch, seen, cx));
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    fn stamp(secs: u64) -> FileStamp {
        FileStamp {
            len: 3,
            modified: Some(UNIX_EPOCH + Duration::from_secs(secs)),
        }
    }

    #[test]
    fn a_change_shows_after_two_matching_polls() {
        let mut watch = Watch::default();
        let seen = Seen::Stamp(stamp(9));
        assert!(!watch.observe(1, seen, stamp(1)));
        assert_eq!(watch.shown(false), None);
        assert!(watch.observe(1, seen, stamp(1)));
        assert_eq!(watch.shown(false), Some(DiskChange::Changed));
        // A different state restarts the count.
        let mut watch = Watch::default();
        watch.observe(1, seen, stamp(1));
        assert!(!watch.observe(1, Seen::Gone, stamp(1)));
        assert_eq!(watch.shown(false), None);
        assert!(watch.observe(1, Seen::Gone, stamp(1)));
        assert_eq!(watch.shown(false), Some(DiskChange::Gone));
    }

    #[test]
    fn the_file_going_back_clears_the_notice() {
        let mut watch = Watch::default();
        let seen = Seen::Stamp(stamp(9));
        watch.observe(0, seen, stamp(1));
        watch.observe(0, seen, stamp(1));
        assert!(watch.observe(0, Seen::Stamp(stamp(1)), stamp(1)));
        assert_eq!(watch.shown(true), None);
    }

    #[test]
    fn ignore_holds_until_the_file_changes_again() {
        let mut watch = Watch::default();
        let first = Seen::Stamp(stamp(9));
        watch.observe(1, first, stamp(1));
        watch.observe(1, first, stamp(1));
        watch.ignore();
        assert_eq!(watch.shown(false), None);
        assert!(!watch.observe(1, first, stamp(1)));
        assert_eq!(watch.shown(false), None);
        let second = Seen::Stamp(stamp(10));
        assert!(!watch.observe(1, second, stamp(1)));
        assert!(watch.observe(1, second, stamp(1)));
        assert_eq!(watch.shown(false), Some(DiskChange::Changed));
    }

    #[test]
    fn a_stat_taken_before_our_save_is_stale() {
        let mut watch = Watch::default();
        let epoch = watch.epoch;
        watch.saved(false);
        assert!(!watch.is_current(epoch));
        assert!(watch.is_current(watch.epoch));
    }

    #[test]
    fn notices_follow_their_file_across_a_swap() {
        let mut watch = Watch::default();
        let seen = Seen::Stamp(stamp(9));
        watch.observe(1, seen, stamp(1));
        watch.observe(1, seen, stamp(1));
        watch.swap();
        assert_eq!(watch.shown(true), Some(DiskChange::Changed));
        assert_eq!(watch.shown(false), None);
    }

    #[test]
    fn a_new_pair_forgets_what_was_ignored() {
        let mut watch = Watch::default();
        let seen = Seen::Stamp(stamp(9));
        watch.observe(1, seen, stamp(1));
        watch.observe(1, seen, stamp(1));
        watch.ignore();
        assert!(watch.is_ignored(false));
        watch.new_pair();
        assert!(!watch.is_ignored(false));
    }
}
