//! Save, save to, export with markers, copy, and the question before unsaved choices are
//! dropped (R8, R9; `js:262-359`).

use duet_core::text::encoding::TextEncoding;
use duet_core::text::lines::EolStyle;
use duet_core::text::merge::output::conflicts_name;
use duet_core::text::save::{SaveError, SaveRequest, save};
use gpui_kit::ClipboardItem;

use super::*;
use crate::paths::home_dir;
use crate::text::saving::UNSAVED_ANSWERS;

/// The dialog's file name when the left has none (`suggestName`).
const DEFAULT_NAME: &str = "merged.txt";
const NOT_MERGED: &str = "Nothing merged yet: press Merge first.";
const NOT_FULL_OUTPUT: &str =
    "Type a full path for the output file, such as C:\\folder\\merged.txt.";
const OUTPUT_IS_FOLDER: &str = "The output must be a file, not a folder.";
const MARKERS_DETAIL: &str = "Saving now writes them as conflict markers: <<<<<<< left … ||||||| base … ======= … >>>>>>> right";

/// What a write is for: Save keeps the path as the output and clears the dirty flag.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Save,
    Export,
}

/// The result as it stood when the write began.
struct Snapshot {
    text: Arc<String>,
    /// The output's encoding and BOM: the left's, else the base's or the right's (M-62).
    encoding: TextEncoding,
    /// UTF-8 was agreed for this result already.
    utf8: bool,
    unresolved: usize,
}

const UTF8: TextEncoding = TextEncoding {
    encoding: encoding_rs::UTF_8,
    bom: false,
};

fn markers(count: usize) -> String {
    count_of(count as u64, "conflict", "conflicts")
}

/// The output as a plain full path, or why it can't take the result (I2): a relative path
/// would land in duet's working folder. Spellings such as C:/x/y or a path with ".." become
/// the plain absolute path, as the folder view's typed paths do.
fn output_path(typed: &Path) -> Result<PathBuf, &'static str> {
    if !typed.is_absolute() {
        return Err(NOT_FULL_OUTPUT);
    }
    let path = std::path::absolute(typed).unwrap_or_else(|_| typed.to_path_buf());
    if path.is_dir() {
        return Err(OUTPUT_IS_FOLDER);
    }
    Ok(path)
}

/// A full path's folder, if it has one.
fn folder_of(path: &Path) -> Option<&Path> {
    path.parent().filter(|dir| dir.is_absolute())
}

impl MergeView {
    /// The left's folder, else the user's home: where Export starts.
    fn left_dir(&self) -> PathBuf {
        self.session
            .as_ref()
            .and_then(|s| s.left().path.as_deref())
            .and_then(folder_of)
            .map_or_else(home_dir, Path::to_path_buf)
    }

    /// Save result to… starts in the output's folder, else the left's (`suggestName`).
    fn save_dir(&self) -> PathBuf {
        self.output
            .as_deref()
            .and_then(folder_of)
            .map_or_else(|| self.left_dir(), Path::to_path_buf)
    }

    /// The output's file name, else the left's, else "merged.txt" (`suggestName`, M3).
    fn suggest_name(&self) -> String {
        self.output
            .as_deref()
            .or_else(|| self.session.as_ref()?.left().path.as_deref())
            .and_then(Path::file_name)
            .map_or_else(
                || DEFAULT_NAME.to_owned(),
                |name| name.to_string_lossy().into_owned(),
            )
    }

    /// Where a dialog starts and the name it suggests: Save result to…, or the export.
    #[doc(hidden)]
    pub fn dialog_for_test(&self, export: bool) -> (PathBuf, String) {
        let name = self.suggest_name();
        if export {
            (self.left_dir(), conflicts_name(&name))
        } else {
            (self.save_dir(), name)
        }
    }

    fn snapshot(&self) -> Option<Snapshot> {
        let session = self.session.as_ref()?;
        Some(Snapshot {
            text: Arc::new(session.result_text()),
            encoding: session.output_format().encoding,
            utf8: self.utf8_agreed,
            unresolved: session.unresolved(),
        })
    }

    /// Ctrl+S and Save result (`saveResult`): true once the file is written. Unresolved
    /// conflicts ask first. `choose` always asks for the path (Save result to…).
    pub fn save(
        &mut self,
        choose: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        // A field with the keys gives them back first, which ends its edit (R7).
        self.leave_input(window, cx);
        let Some(session) = self.session.as_ref() else {
            self.show_notice(NOT_MERGED, cx);
            return Task::ready(false);
        };
        let open = session.unresolved();
        // A typed output is checked before anything is asked (I2).
        let fixed = match self.output.as_deref().filter(|_| !choose).map(output_path) {
            Some(Err(problem)) => {
                self.show_notice(problem, cx);
                return Task::ready(false);
            }
            Some(Ok(path)) => Some(path),
            None => None,
        };
        if open == 0 {
            return self.save_to(fixed, window, cx);
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!(
                "{} not resolved.",
                count_of(open as u64, "conflict is", "conflicts are")
            ),
            Some(MARKERS_DETAIL),
            &["Cancel", "Save with conflict markers"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(1) {
                return false;
            }
            match this.update_in(cx, |view, window, cx| view.save_to(fixed, window, cx)) {
                Ok(task) => task.await,
                Err(_) => false,
            }
        })
    }

    /// Writes to the checked output, else to the path the dialog returns.
    fn save_to(
        &mut self,
        fixed: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        let name = self.suggest_name();
        self.write_result(fixed, name, Purpose::Save, window, cx)
    }

    /// "Export with conflict markers": a new file named `name.conflicts.ext`; the dirty flag
    /// and the output stay as they are.
    pub fn export_markers(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Task<bool> {
        self.leave_input(window, cx);
        if self.session.is_none() {
            self.show_notice(NOT_MERGED, cx);
            return Task::ready(false);
        }
        let name = conflicts_name(&self.suggest_name());
        self.write_result(None, name, Purpose::Export, window, cx)
    }

    /// Writes the result to `fixed`, else to the path the dialog returns. A character the
    /// output's encoding cannot hold asks once for UTF-8.
    fn write_result(
        &mut self,
        fixed: Option<PathBuf>,
        name: String,
        purpose: Purpose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        let dir = match purpose {
            Purpose::Save => self.save_dir(),
            Purpose::Export => self.left_dir(),
        };
        let picked = fixed
            .is_none()
            .then(|| cx.prompt_for_new_path(&dir, Some(&name)));
        cx.spawn_in(window, async move |this, cx| {
            let path = match (fixed, picked) {
                (Some(path), _) => path,
                (None, Some(picked)) => match picked.await {
                    Ok(Ok(Some(path))) => path,
                    _ => return false,
                },
                (None, None) => return false,
            };
            let Ok(Some(snapshot)) = this.update(cx, |view, _| view.snapshot()) else {
                return false;
            };
            let background = cx.background_executor().clone();
            let write = |encoding: TextEncoding| {
                let (path, text) = (path.clone(), snapshot.text.clone());
                background.spawn(async move {
                    save(&SaveRequest {
                        path: &path,
                        text: &text,
                        encoding,
                        eol: EolStyle::None,
                        expected: None,
                    })
                })
            };
            let mut as_utf8 = snapshot.utf8;
            let mut outcome = write(if as_utf8 { UTF8 } else { snapshot.encoding }).await;
            if !as_utf8 && matches!(outcome, Err(SaveError::Encode(_))) {
                let Ok(answer) = this.update_in(cx, |_, window, cx| {
                    window.prompt(
                        PromptLevel::Warning,
                        &format!(
                            "The result has characters {} can't hold.",
                            snapshot.encoding.encoding.name()
                        ),
                        Some("Save it as UTF-8 instead?"),
                        &["Cancel", "Save as UTF-8"],
                        cx,
                    )
                }) else {
                    return false;
                };
                if answer.await != Ok(1) {
                    return false;
                }
                // The answer holds for this result (LocalDiff `encOk`).
                let _ = this.update(cx, |view, cx| {
                    view.utf8_agreed = true;
                    cx.notify();
                });
                as_utf8 = true;
                outcome = write(UTF8).await;
            }
            let saved = outcome.is_ok();
            let _ = this.update_in(cx, |view, window, cx| {
                view.written(&path, purpose, &snapshot, as_utf8, outcome, window, cx)
            });
            saved
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn written(
        &mut self,
        path: &Path,
        purpose: Purpose,
        snapshot: &Snapshot,
        as_utf8: bool,
        outcome: Result<duet_core::text::document::FileStamp, SaveError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shown = path.display();
        let notice = match (outcome, purpose) {
            (Err(err), Purpose::Save) => format!("Can't save: {err}. The file was not changed."),
            (Err(err), Purpose::Export) => {
                format!("Can't export: {err}. The file was not changed.")
            }
            (Ok(_), Purpose::Export) => {
                let how = if snapshot.unresolved == 0 {
                    "no conflicts left".to_owned()
                } else {
                    format!("{} as markers", markers(snapshot.unresolved))
                };
                format!("Exported: {shown} ({how}).")
            }
            (Ok(_), Purpose::Save) => {
                self.set_output(Some(path.to_path_buf()), window, cx);
                // Choices made while the file was written are not in it.
                if let Some(session) = self.session.as_mut()
                    && session.result_text() == *snapshot.text
                {
                    session.mark_saved();
                }
                let mut text = format!("Saved the result: {shown}");
                if as_utf8 {
                    text.push_str(" (as UTF-8)");
                }
                if snapshot.unresolved > 0 {
                    text.push_str(&format!(
                        " ({} written as markers)",
                        markers(snapshot.unresolved)
                    ));
                }
                text.push('.');
                text
            }
        };
        self.notice = Some(notice.into());
        cx.notify();
    }

    /// Copy result (`js:340-359`): the whole result, unresolved conflicts as markers.
    pub fn copy_result(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.as_ref() else {
            self.show_notice(NOT_MERGED, cx);
            return;
        };
        let lines = session.result_line_count();
        let open = session.unresolved();
        cx.write_to_clipboard(ClipboardItem::new_string(session.result_text()));
        let mut text = format!(
            "Copied the result: {}",
            count_of(lines as u64, "line", "lines")
        );
        if open > 0 {
            text.push_str(&format!(", {} as markers", markers(open)));
        }
        text.push('.');
        self.show_notice(text, cx);
    }

    /// Ctrl+S works from a field too (free key).
    pub(super) fn on_save(&mut self, _: &MergeSave, window: &mut Window, cx: &mut Context<Self>) {
        self.save(false, window, cx).detach();
    }

    /// The question before the tab or the window closes over choices that are not saved.
    pub fn confirm_discard(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Task<bool> {
        if !self.is_dirty() {
            return Task::ready(true);
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            "Save the merge result?",
            Some("Your conflict choices are lost if you don't save them."),
            &UNSAVED_ANSWERS,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| match answer.await {
            Ok(0) => match this.update_in(cx, |view, window, cx| view.save(false, window, cx)) {
                Ok(task) => task.await,
                Err(_) => false,
            },
            Ok(1) => true,
            _ => false,
        })
    }
}
