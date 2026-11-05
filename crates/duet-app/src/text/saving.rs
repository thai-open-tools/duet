//! Saving edited sides and asking before unsaved edits are dropped.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use duet_core::text::buffer::TextBuffer;
use duet_core::text::document::{FileStamp, ReadOnlyReason};
use duet_core::text::encoding::{EncodeError, TextEncoding};
use duet_core::text::lines::EolStyle;
use duet_core::text::save::{SaveError, SaveRequest, same_file, save};
use encoding_rs::UTF_8;
use gpui_kit::{
    AsyncWindowContext, BackgroundExecutor, Context, PromptLevel, SharedString, Task, WeakEntity,
    Window,
};

use crate::i18n::errors::save_error;
use crate::i18n::{answers, tr, tr_key, trf};
use crate::text::editing::WAIT_NOTICE;
use crate::text::view::{TextEvent, TextView};

/// Why a write was refused: the notice, and whether it is the "Still saving {}." one. The flag
/// travels with the text, so nothing reads the displayed sentence back.
pub(crate) struct Refusal {
    text: String,
    still_saving: bool,
}

impl From<String> for Refusal {
    fn from(text: String) -> Self {
        Self {
            text,
            still_saving: false,
        }
    }
}

fn still_saving(path: &Path) -> Refusal {
    Refusal {
        text: trf("Still saving {}.", &[&file_name(path)]),
        still_saving: true,
    }
}

pub const UNSAVED_ANSWERS: [&str; 3] = [tr_key("Save"), tr_key("Discard"), tr_key("Cancel")];
pub const CONFLICT_ANSWERS: [&str; 2] = [tr_key("Cancel"), tr_key("Overwrite")];
/// What a character the target encoding cannot hold offers (R14).
pub const ENCODE_ANSWERS: [&str; 2] = [tr_key("Save as UTF-8"), tr_key("Cancel")];

/// What a write came to: the stamp of the file and the encoding it was written in, or the
/// notice that says why not.
type Written = Result<(FileStamp, TextEncoding), Refusal>;
/// What the first, background attempt of a write returned: the save's own result, or a refusal.
type Attempt = Result<Result<FileStamp, SaveError>, Refusal>;

/// One side's save, ready to run off the UI thread.
struct Job {
    path: PathBuf,
    text: String,
    encoding: TextEncoding,
    eol: EolStyle,
    expected: FileStamp,
    version: u64,
}

fn run(job: &Job, encoding: TextEncoding, check: bool) -> Result<FileStamp, SaveError> {
    save(&SaveRequest {
        path: &job.path,
        text: &job.text,
        encoding,
        eol: job.eol,
        expected: check.then_some(job.expected),
    })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn failure_notice(err: &SaveError) -> String {
    match err {
        SaveError::Encode(EncodeError::Unrepresentable { encoding, .. }) => trf(
            "Can't save: the text has a character {} can't hold. The file was not changed.",
            &[encoding],
        ),
        other => trf(
            "Can't save: {}. The file was not changed.",
            &[&save_error(other)],
        ),
    }
}

/// Finishes a write after its first attempt: a file changed on disk asks before it is
/// overwritten (only while `check` is on), and a character the encoding cannot hold offers
/// UTF-8 (R14). None: the conflict prompt was cancelled. Err: refused, the notice is final.
async fn write_job(
    this: &WeakEntity<TextView>,
    cx: &mut AsyncWindowContext,
    background: &BackgroundExecutor,
    job: &Arc<Job>,
    first: Attempt,
    mut check: bool,
) -> Option<Written> {
    let mut encoding = job.encoding;
    let mut offered = false;
    let mut outcome = first;
    loop {
        let rerun = |encoding, check| {
            let job = job.clone();
            background.spawn(async move { Ok(run(&job, encoding, check)) })
        };
        match outcome {
            Err(notice) => return Some(Err(notice)),
            Ok(Ok(stamp)) => return Some(Ok((stamp, encoding))),
            Ok(Err(SaveError::ChangedOnDisk { path })) if check => {
                let name = file_name(&path);
                let answer = this.update_in(cx, |_, window, cx| {
                    window.prompt(
                        PromptLevel::Warning,
                        &trf("{} changed on disk since it was opened.", &[&name]),
                        Some(tr(
                            "Overwrite it with your edits, or cancel and keep editing?",
                        )),
                        &answers(&CONFLICT_ANSWERS),
                        cx,
                    )
                });
                if !matches!(answer.ok()?.await, Ok(1)) {
                    return None;
                }
                check = false;
                outcome = rerun(encoding, check).await;
            }
            Ok(Err(SaveError::Encode(EncodeError::Unrepresentable { encoding: name, .. })))
                if !offered =>
            {
                let file = file_name(&job.path);
                let detail = trf(
                    "It has a character {} can't hold. Save it as UTF-8 instead? The file's encoding changes.",
                    &[&name],
                );
                let answer = this.update_in(cx, |_, window, cx| {
                    window.prompt(
                        PromptLevel::Warning,
                        &trf("{} can't be saved as {}.", &[&file, &name]),
                        Some(&detail),
                        &answers(&ENCODE_ANSWERS),
                        cx,
                    )
                });
                if !matches!(answer.ok()?.await, Ok(0)) {
                    return Some(Err(trf(
                        "Not saved: {} has a character {} can't hold.",
                        &[&file, &name],
                    )
                    .into()));
                }
                offered = true;
                encoding = TextEncoding {
                    encoding: UTF_8,
                    bom: false,
                };
                outcome = rerun(encoding, check).await;
            }
            Ok(Err(other)) => return Some(Err(failure_notice(&other).into())),
        }
    }
}

impl TextView {
    /// Shows a refusal as the notice, remembering whether it is the "Still saving" one.
    fn refuse(&mut self, refusal: Refusal) {
        let text: SharedString = refusal.text.into();
        self.still_saving_notice = refusal.still_saving.then(|| text.clone());
        self.notice = Some(text);
    }

    /// The notice on show is the "Still saving" refusal, not something that replaced it.
    fn showing_still_saving(&self) -> bool {
        self.still_saving_notice.is_some() && self.notice == self.still_saving_notice
    }

    pub fn is_dirty(&self) -> bool {
        self.model.as_ref().is_some_and(|m| m.any_dirty())
    }

    /// K1: after a save, F5 reads the side as it was written. A side forced to an encoding takes
    /// the one written, never the old one and never Auto. On Auto, a save that changed the
    /// encoding to one detection cannot tell apart (no BOM, not UTF-8: windows-874, Shift_JIS,
    /// GBK, windows-1252) forces it too. Only while the side still holds `path`.
    fn read_as_written(
        &mut self,
        left: bool,
        path: &Path,
        before: TextEncoding,
        written: TextEncoding,
    ) {
        if self
            .model
            .as_ref()
            .is_none_or(|model| model.side(left).path() != path)
        {
            return;
        }
        let forced = if left {
            &mut self.request.left_encoding
        } else {
            &mut self.request.right_encoding
        };
        let unsure = written != before && !written.bom && written.encoding != UTF_8;
        if forced.is_some() || unsure {
            *forced = Some(written.encoding);
        }
    }

    /// The file a save of one side writes; None when the side has no edits to save.
    fn save_path(&self, left: bool) -> Option<&Path> {
        let model = self.model.as_ref()?;
        model.buffer(left)?;
        Some(if left { model.left() } else { model.right() }.path())
    }

    fn save_job(&self, left: bool) -> Option<Job> {
        let model = self.model.as_ref()?;
        let buffer = model.buffer(left)?;
        let doc = if left { model.left() } else { model.right() };
        Some(Job {
            path: doc.path().to_path_buf(),
            text: buffer.text().to_owned(),
            encoding: self.prefs.save_encoding.apply(doc.encoding()),
            eol: self.prefs.save_eol.apply(doc.eol()),
            expected: model.disk_stamp(left),
            version: buffer.version(),
        })
    }

    /// Writes one side. A file changed on disk since it was loaded asks first. While a save of
    /// the same file still runs, this one is skipped (false) with a notice: the running one
    /// would make it see its own write as a change on disk. The check comes before the text is
    /// copied, so a skipped save of a large file costs nothing.
    pub fn save_side(
        &mut self,
        left: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        let Some(path) = self.save_path(left) else {
            return Task::ready(true);
        };
        // An untitled side has no file to write: it asks where (R15).
        if path.as_os_str().is_empty() {
            return self.save_untitled(left, window, cx);
        }
        // Only the spelling is compared here; resolving names touches the disk (a UNC path can be
        // slow), so a different spelling of a running save's file is refused by the job.
        if self.saving.iter().any(|saving| saving == path) {
            self.refuse(still_saving(path));
            cx.notify();
            return Task::ready(false);
        }
        // The On save choices every tab shares (I2).
        self.pull_shared(cx);
        let Some(job) = self.save_job(left) else {
            return Task::ready(true);
        };
        let busy = self.saving.clone();
        self.saving.push(job.path.clone());
        self.watch.invalidate();
        self.undo.seal();
        let swaps = self.syntax_swaps;
        let background = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let job = Arc::new(job);
            let first = {
                let job = job.clone();
                background
                    .spawn(async move {
                        if busy.iter().any(|s| same_file(s, &job.path)) {
                            return Err(still_saving(&job.path));
                        }
                        Ok(run(&job, job.encoding, true))
                    })
                    .await
            };
            let result = write_job(&this, cx, &background, &job, first, true).await;
            this.update(cx, |view, cx| {
                view.saving.retain(|path| *path != job.path);
                match result {
                    None => false,
                    Some(Ok((stamp, encoding))) => {
                        // F9 while the save ran moved the file, and its buffer, to the other
                        // side; `mark_written` still checks that the side holds `job.path`.
                        let left = left != ((view.syntax_swaps - swaps) % 2 == 1);
                        let before = view.model.as_ref().map(|m| m.side(left).encoding());
                        if let Some(model) = &mut view.model {
                            model.mark_written(left, &job.path, &job.text, job.version, stamp);
                            model.set_encoding_after_save(left, &job.path, encoding, job.eol);
                        }
                        if let Some(before) = before {
                            view.read_as_written(left, &job.path, before, encoding);
                        }
                        view.watch.saved(left);
                        // A refusal that landed first (a differently spelled save) stays.
                        if !view.showing_still_saving() {
                            view.notice = None;
                        }
                        cx.notify();
                        true
                    }
                    Some(Err(refusal)) => {
                        view.refuse(refusal);
                        cx.notify();
                        false
                    }
                }
            })
            .unwrap_or(false)
        })
    }

    /// Saves an untitled side: asks for a file, then Save As writes it there. A side with no
    /// edits has nothing to save. False when the dialog is cancelled.
    fn save_untitled(
        &mut self,
        left: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        if !self.model.as_ref().is_some_and(|m| m.is_dirty(left)) {
            return Task::ready(true);
        }
        let picked = cx.prompt_for_new_path(Path::new(""), Some("untitled.txt"));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return false;
            };
            match this.update_in(cx, |view, window, cx| {
                view.save_side_as(left, path, window, cx)
            }) {
                Ok(task) => task.await,
                Err(_) => false,
            }
        })
    }

    /// Save As of a sorted or formatted view would write the changed text over a file (or into a
    /// new one) as if it were the file's own: say so and write nothing. True when refused.
    pub(crate) fn refuse_save_as_of_view(&mut self, left: bool, cx: &mut Context<Self>) -> bool {
        let Some(reason) = self.model.as_ref().and_then(|m| m.derived_view(left)) else {
            return false;
        };
        self.notice = Some(
            match reason {
                ReadOnlyReason::Sorted => {
                    tr("The sorted view can't be saved as a new file. Turn Sort keys off first.")
                }
                _ => tr("The formatted view can't be saved as a new file. Turn Format off first."),
            }
            .into(),
        );
        cx.notify();
        true
    }

    /// Save As (rule R9): writes the side's text to `path` in its encoding and line endings,
    /// without a disk-stamp check (the dialog asked about overwriting), and the side then
    /// refers to `path`; the workspace re-keys the tab on `Renamed`. Undo history is kept.
    pub fn save_side_as(
        &mut self,
        left: bool,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        if self.job.is_running() {
            self.notice = Some(tr(WAIT_NOTICE).into());
            cx.notify();
            return Task::ready(false);
        }
        self.pull_shared(cx);
        if self.refuse_save_as_of_view(left, cx) {
            return Task::ready(false);
        }
        let Some(model) = self.model.as_ref() else {
            return Task::ready(false);
        };
        let doc = if left { model.left() } else { model.right() };
        let other_path = if left { model.right() } else { model.left() }
            .path()
            .to_path_buf();
        if model.buffer(left).is_none() && doc.is_lossy() {
            self.notice = Some(
                tr("This file did not decode cleanly, so it can't be saved as a new file.").into(),
            );
            cx.notify();
            return Task::ready(false);
        }
        let origin = doc.path().to_path_buf();
        let (text, version) = match (model.buffer(left), doc.text()) {
            (Some(buffer), _) => (buffer.text().to_owned(), Some(buffer.version())),
            (None, Some(text)) => (text.to_owned(), None),
            (None, None) => {
                self.notice = Some(
                    tr("This file is over 64 MiB, so it can't be saved as a new file.").into(),
                );
                cx.notify();
                return Task::ready(false);
            }
        };
        let job = Job {
            path,
            text,
            encoding: self.prefs.save_encoding.apply(doc.encoding()),
            eol: self.prefs.save_eol.apply(doc.eol()),
            expected: model.disk_stamp(left),
            version: version.unwrap_or_default(),
        };
        let busy = self.saving.clone();
        self.saving.push(job.path.clone());
        self.watch.invalidate();
        self.undo.seal();
        let swaps = self.syntax_swaps;
        let background = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let job = Arc::new(job);
            // The same-file checks read the disk (a UNC path can be slow), so they run here.
            let first = {
                let job = job.clone();
                let origin = origin.clone();
                background
                    .spawn(async move {
                        if !other_path.as_os_str().is_empty() && same_file(&job.path, &other_path) {
                            return Err(tr(
                                "That file is open on the other side. Pick another name.",
                            )
                            .to_owned()
                            .into());
                        }
                        if let Some(in_flight) = [job.path.as_path(), origin.as_path()]
                            .into_iter()
                            .find(|p| busy.iter().any(|s| same_file(s, p)))
                        {
                            return Err(still_saving(in_flight));
                        }
                        Ok(run(&job, job.encoding, false))
                    })
                    .await
            };
            let result = write_job(&this, cx, &background, &job, first, false).await;
            this.update(cx, |view, cx| {
                if let Some(at) = view.saving.iter().position(|path| *path == job.path) {
                    view.saving.remove(at);
                }
                let Some(result) = result else {
                    return false;
                };
                match result {
                    Ok((stamp, encoding)) => {
                        let left = left != ((view.syntax_swaps - swaps) % 2 == 1);
                        let request = &view.request;
                        // A reload, open or paste while the write ran: the file is written,
                        // but this side is no longer the one that was saved.
                        if *(if left { &request.left } else { &request.right }) != origin {
                            view.notice = Some(
                                trf(
                                    "Saved {}, but this side now shows another file.",
                                    &[&file_name(&job.path)],
                                )
                                .into(),
                            );
                            cx.notify();
                            return false;
                        }
                        let before = view.model.as_ref().map(|m| m.side(left).encoding());
                        if let Some(model) = &mut view.model {
                            model.mark_saved_as(left, job.path.clone(), &job.text, version, stamp);
                            model.set_encoding_after_save(left, &job.path, encoding, job.eol);
                        }
                        if let Some(before) = before {
                            view.read_as_written(left, &job.path, before, encoding);
                        }
                        view.watch.saved(left);
                        // A refusal that landed first (another save of the same file) stays.
                        if !view.showing_still_saving() {
                            view.notice = None;
                        }
                        let request = &mut view.request;
                        if left {
                            request.left = job.path.clone();
                        } else {
                            request.right = job.path.clone();
                        }
                        let (left, right) = (request.left.clone(), request.right.clone());
                        cx.emit(TextEvent::Renamed { left, right });
                        cx.notify();
                        true
                    }
                    Err(refusal) => {
                        view.refuse(refusal);
                        cx.notify();
                        false
                    }
                }
            })
            .unwrap_or(false)
        })
    }

    /// Writes every dirty side; true when all of them saved. A file open on both sides is
    /// written once. When both sides are dirty with different text, nothing is written and the
    /// result is false with a notice: either write would drop the other side's edits. Ctrl+S on
    /// one side still saves that side.
    pub fn save_all(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Task<bool> {
        if self.same_file_with_different_edits() {
            self.notice = Some(
                tr("Both sides are the same file with different edits. Save one side.").into(),
            );
            cx.notify();
            return Task::ready(false);
        }
        let mut paths: Vec<&Path> = Vec::new();
        let sides: Vec<bool> = [true, false]
            .into_iter()
            .filter(|&l| {
                let dirty = self.model.as_ref().is_some_and(|m| m.is_dirty(l));
                match self.save_path(l).filter(|_| dirty) {
                    // Untitled sides are different files whatever their (empty) paths say.
                    Some(path) if path.as_os_str().is_empty() => true,
                    Some(path) if !paths.contains(&path) => {
                        paths.push(path);
                        true
                    }
                    _ => false,
                }
            })
            .collect();
        let (untitled, named): (Vec<bool>, Vec<bool>) =
            sides.into_iter().partition(|&l| self.is_untitled(l));
        let tasks: Vec<Task<bool>> = named
            .into_iter()
            .map(|l| self.save_side(l, window, cx))
            .collect();
        cx.spawn_in(window, async move |this, cx| {
            let mut all = true;
            for task in tasks {
                all &= task.await;
            }
            // One Save As dialog at a time; cancelling one stops the rest.
            for left in untitled {
                let Ok(task) =
                    this.update_in(cx, |view, window, cx| view.save_side(left, window, cx))
                else {
                    return false;
                };
                if !task.await {
                    return false;
                }
            }
            all
        })
    }

    /// Both sides are edited copies of one file, and their texts differ.
    fn same_file_with_different_edits(&self) -> bool {
        let Some(model) = &self.model else {
            return false;
        };
        model.is_dirty(true)
            && model.is_dirty(false)
            && self.save_path(true) == self.save_path(false)
            && !self.is_untitled(true)
            && model.buffer(true).map(TextBuffer::text) != model.buffer(false).map(TextBuffer::text)
    }

    /// Ctrl+S: the caret's side when it is dirty, otherwise every dirty side.
    pub(crate) fn save_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let caret_side = self
            .caret
            .map(|c| c.left)
            .filter(|&l| self.model.as_ref().is_some_and(|m| m.is_dirty(l)));
        match caret_side {
            Some(left) => self.save_side(left, window, cx).detach(),
            None => self.save_all(window, cx).detach(),
        }
    }

    /// Asks what to do with unsaved edits. True when the caller may go on (saved, discarded,
    /// or nothing to lose).
    pub fn confirm_discard(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Task<bool> {
        if !self.is_dirty() {
            return Task::ready(true);
        }
        let names = self.dirty_names();
        let answer = window.prompt(
            PromptLevel::Warning,
            &trf("Save changes to {}?", &[&names]),
            Some(tr("Your edits are lost if you don't save them.")),
            &answers(&UNSAVED_ANSWERS),
            cx,
        );
        cx.spawn_in(window, async move |this, cx| match answer.await {
            Ok(0) => match this.update_in(cx, |view, window, cx| view.save_all(window, cx)) {
                Ok(task) => task.await,
                Err(_) => false,
            },
            Ok(1) => true,
            _ => false,
        })
    }

    fn dirty_names(&self) -> String {
        let Some(model) = &self.model else {
            return String::new();
        };
        let name = |left: bool| {
            let doc = if left { model.left() } else { model.right() };
            match doc.path().file_name() {
                Some(name) => name.to_string_lossy().into_owned(),
                None if doc.path().as_os_str().is_empty() => tr("untitled").to_owned(),
                None => String::new(),
            }
        };
        match (model.is_dirty(true), model.is_dirty(false)) {
            (true, true) if self.is_untitled(true) && self.is_untitled(false) => {
                tr("both untitled sides").to_owned()
            }
            (true, true) => trf("{} and {}", &[&name(true), &name(false)]),
            (true, false) => name(true),
            _ => name(false),
        }
    }

    /// Reloads once unsaved edits are saved or discarded, at once when there are none; `prepare`
    /// changes the request first. Cancel keeps the edits and reloads nothing.
    pub(crate) fn reload_guarded(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        prepare: impl FnOnce(&mut Self) + 'static,
    ) {
        if !self.is_dirty() {
            prepare(self);
            self.reload(cx);
            return;
        }
        let ok = self.confirm_discard(window, cx);
        cx.spawn(async move |this, cx| {
            if ok.await {
                let _ = this.update(cx, |view, cx| {
                    // The model goes with the edits: its rows and documents may hold the edited
                    // text, so only the reload, which always follows, can show the file again.
                    if let Some(model) = view.model.take() {
                        view.pending_filter = Some(model.filter());
                    }
                    view.caret = None;
                    view.undo.clear();
                    prepare(view);
                    view.reload(cx);
                });
            }
        })
        .detach();
    }
}
