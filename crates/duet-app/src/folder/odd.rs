//! The invisible-character scan after a folder compare (R9): the job, its marks and the chips.
//!
//! The scan starts when a compare has finished and its rows are on screen, and writes nothing
//! else. F5, new roots or turning the setting off stop it; Esc does not (it keeps its meaning
//! for the rows). The marks belong to the model they were made for: a new compare drops them,
//! F9 swaps them with the sides. Above `ODD_AUTO_LIMIT` files the scan waits for a click.
//!
//! The scan reads files up to 4 MiB (`ODD_FILE_LIMIT`), where the text tab scans a side up to
//! 64 MiB. The cap is deliberate: the scan is a hint beside the rows, run after every compare.

use duet_core::folder::odd_scan::{OddMarks, OddProgress, scan_odd};
use duet_core::text::odd::{OddPlace, OddScan, odd_info};
use gpui_kit::{Context, Task};

use crate::folder::ops::OddStatus;
use crate::folder::view::FolderView;
use crate::format::count_of;
use crate::i18n::{tr, trf};
use crate::job::JobSlot;

/// A compare of more files than this does not scan by itself (PF3).
pub const ODD_AUTO_LIMIT: usize = 10_000;
/// The scan is a hint beside the rows: it leaves the other cores to the user.
const ODD_THREADS: usize = 2;

/// The scan job of one folder view.
#[derive(Default)]
pub(super) struct OddRun {
    pub(super) job: JobSlot,
    pub(super) marks: Option<OddMarks>,
    pub(super) task: Option<Task<()>>,
    /// The number of files when the compare was too big to scan by itself.
    pub(super) held_back: Option<usize>,
    /// Test hook: replaces `ODD_AUTO_LIMIT`.
    pub(super) auto_limit: Option<usize>,
}

impl OddRun {
    /// Stops the job and forgets everything it found.
    pub(super) fn drop_all(&mut self) {
        self.job.cancel();
        self.task = None;
        self.marks = None;
        self.held_back = None;
    }

    /// The status's view of the scan, for a compare that is `done`.
    pub(super) fn status(&self, enabled: bool, done: bool) -> OddStatus {
        if !enabled || !done {
            OddStatus::Off
        } else if self.job.is_running() {
            OddStatus::Running
        } else if let Some(marks) = &self.marks {
            let (warn, info) = marks.files();
            OddStatus::Done { warn, info }
        } else if let Some(files) = self.held_back {
            OddStatus::HeldBack(files)
        } else {
            OddStatus::Off
        }
    }
}

impl FolderView {
    /// The marks of the model on show; None while a scan runs, before one has run, and when the
    /// setting is off.
    pub fn odd_marks(&self) -> Option<&OddMarks> {
        self.odd.marks.as_ref()
    }

    pub fn odd_scan_running(&self) -> bool {
        self.odd.job.is_running()
    }

    /// Test hook: scan by itself only up to `limit` files.
    #[doc(hidden)]
    pub fn set_odd_auto_limit_for_test(&mut self, limit: usize) {
        self.odd.auto_limit = Some(limit);
    }

    /// After a finished compare: scan if the setting is on and the folder is not too big
    /// (`force` skips the size check: the button).
    pub(super) fn start_odd_scan(&mut self, force: bool, cx: &mut Context<Self>) {
        self.odd.drop_all();
        if !self.prefs.odd_scan {
            self.refresh_status();
            return;
        }
        let Some(model) = &self.model else { return };
        let cmp = model.comparison();
        let files = cmp.nodes.iter().filter(|n| !n.is_dir).count();
        if !force && files > self.odd.auto_limit.unwrap_or(ODD_AUTO_LIMIT) {
            self.odd.held_back = Some(files);
            self.refresh_status();
            cx.notify();
            return;
        }
        // The model's own comparison, shared: a copy of a 100k-node tree would stall the frame.
        let cmp = model.shared_comparison();
        let (left, right) = (self.left.clone(), self.right.clone());
        let (generation, cancel) = self.odd.job.start();
        let background = cx.background_executor().clone();
        self.odd.task = Some(cx.spawn(async move |this, cx| {
            let found = background
                .spawn(async move {
                    let progress = OddProgress::default();
                    scan_odd(&cmp, &left, &right, ODD_THREADS, &progress, &cancel)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if !view.odd.job.is_current(generation) {
                    return;
                }
                view.odd.job.finish(generation);
                view.odd.marks = found.ok();
                view.refresh_status();
                cx.notify();
            });
        }));
        self.refresh_status();
        cx.notify();
    }

    /// The button beside the "not scanned" note.
    pub(super) fn scan_odd_now(&mut self, cx: &mut Context<Self>) {
        if !self.job.is_running() {
            self.start_odd_scan(true, cx);
        }
    }

    /// F9: the finished marks follow their files to the other side; a scan in flight started
    /// from the old sides and is started again. True when it must be started again.
    pub(super) fn swap_odd(&mut self) -> bool {
        let running = self.odd.job.is_running();
        self.odd.job.cancel();
        self.odd.task = None;
        self.odd.marks = self.odd.marks.take().map(OddMarks::swapped);
        running
    }

    /// The compare summary again, with the scan's current state.
    pub(super) fn refresh_status(&mut self) {
        if let Some(model) = self.model.take() {
            let done = !self.job.is_running();
            self.set_model_status(&model, done);
            self.model = Some(model);
        }
    }
}

/// `oddTitle` (js:354-359): what the chip's tooltip says about one file.
pub fn odd_tip(scan: &OddScan) -> String {
    let what = count_of(
        scan.count as u64,
        "invisible character",
        "invisible characters",
    );
    let mut tip = trf(
        "{}: {} in the text or values · {} in comments or indentation",
        &[&what, &scan.warn, &scan.info],
    );
    if let Some(hit) = scan.hits.first() {
        let place = match hit.place {
            OddPlace::Comment => tr("comment"),
            OddPlace::Indent => tr("indentation"),
            OddPlace::Text => tr("text or value"),
        };
        tip.push('\n');
        tip.push_str(&trf(
            "First: line {}, {} ({})",
            &[&(hit.line + 1), &odd_info(hit.ch).1, &place],
        ));
    }
    tip.push('\n');
    tip.push_str(if scan.warn > 0 {
        tr("They can make a value wrong even though it looks right. Open the file and use the button in the status bar to visit each one.")
    } else {
        tr("They are only in comments or indentation and change no value.")
    });
    tip
}

#[cfg(test)]
mod tests {
    use super::*;
    use duet_core::text::odd::OddHit;

    fn scan(warn: usize, info: usize, place: OddPlace) -> OddScan {
        OddScan {
            count: warn + info,
            warn,
            info,
            hits: vec![OddHit {
                line: 4,
                byte: 0,
                ch: '\u{a0}',
                place,
            }],
        }
    }

    #[test]
    fn odd_tips_follow_localdiff() {
        assert_eq!(
            odd_tip(&scan(1, 1, OddPlace::Text)),
            "2 invisible characters: 1 in the text or values · 1 in comments or indentation\nFirst: line 5, U+00A0 no-break space (text or value)\nThey can make a value wrong even though it looks right. Open the file and use the button in the status bar to visit each one."
        );
        assert_eq!(
            odd_tip(&scan(0, 1, OddPlace::Comment)),
            "1 invisible character: 0 in the text or values · 1 in comments or indentation\nFirst: line 5, U+00A0 no-break space (comment)\nThey are only in comments or indentation and change no value."
        );
    }
}
