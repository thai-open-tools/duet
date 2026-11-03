//! The Report ▾ menu (R20, R21; `ops:472-561`): an HTML page, a patch file, a text report, and
//! the patch on the clipboard. Each takes its documents on the UI thread (`begin_snapshot`,
//! PF16) and builds and writes off it. The HTML page stops at its row cap; the patch and the
//! text report hold every changed line, so they are not offered for a side over 64 MiB (M16).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use duet_core::cancel::CancelToken;
use duet_core::text::diff::{DiffInput, DiffOptions, LineDiff, diff_lines};
use duet_core::text::encoding::TextEncoding;
use duet_core::text::lines::EolStyle;
use duet_core::text::report::{
    HTML_MAX_ROWS, PatchOptions, ReportMeta, ReportMode, html_report, report_name, text_report,
    unified_diff,
};
use duet_core::text::rows::RowMap;
use duet_core::text::save::{SaveRequest, save};
use encoding_rs::UTF_8;
use gpui_kit::{ClipboardItem, Context, PromptLevel, Window, actions};

use crate::paths::home_dir;
use crate::text::model::{Snapshot, status_text};
use crate::text::view::TextView;
use crate::ui::menu::MenuEntry;

actions!(
    text_report,
    [ReportHtml, ReportPatch, ReportText, CopyPatch]
);

/// What the HTML report's question offers; the last answer cancels.
pub const HTML_ANSWERS: [&str; 4] = [
    "Differences with context",
    "Differences only",
    "All lines",
    "Cancel",
];
pub const HTML_QUESTION: &str = "Which lines should the report show?";
/// LocalDiff's patch context (`ctx: 3`).
const PATCH_CONTEXT: u32 = 3;
/// Why the patch or the text report did nothing.
const TOO_LARGE: &str = "Patch and text reports are not available for files over 64 MiB.";

/// Stops the reports still building when its tab closes: the patch's own compare of a large
/// pair can take a while.
#[derive(Default)]
pub(crate) struct ReportCancel(CancelToken);

impl Drop for ReportCancel {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// The Report ▾ items. They wait while a compare or re-diff runs and before a result.
pub fn report_menu(view: &TextView) -> Vec<MenuEntry> {
    let on = view.can_report();
    vec![
        MenuEntry::action("HTML report (side by side)…", ReportHtml).enabled(on),
        MenuEntry::action("Patch file (unified diff)…", ReportPatch).enabled(on),
        MenuEntry::action("Text report…", ReportText).enabled(on),
        MenuEntry::Separator,
        MenuEntry::action("Copy patch to the clipboard", CopyPatch).enabled(on),
    ]
}

#[derive(Clone, Copy)]
enum Kind {
    Html(ReportMode),
    Patch,
    Text,
}

impl Kind {
    fn ext(self) -> &'static str {
        match self {
            Kind::Html(_) => ".html",
            Kind::Patch => ".patch",
            Kind::Text => ".txt",
        }
    }
}

/// Everything a report needs from the view, taken on the UI thread.
struct Input {
    snapshot: Snapshot,
    diff: LineDiff,
    rows: Arc<RowMap>,
    ignoring: bool,
    tab_width: usize,
    status: String,
    /// Shown in the report: the file name, "untitled" for a side with no file.
    names: [String; 2],
    /// The side's file name, empty when it has no file; the saved name is built from it.
    files: [String; 2],
    dir: PathBuf,
    date: String,
    /// Set when the tab closes.
    cancel: CancelToken,
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The patch: a plain compare with `ignore_eol` on, as LocalDiff's `makePatch` (R20).
fn patch_of(
    left: &duet_core::text::document::TextDocument,
    right: &duet_core::text::document::TextDocument,
    names: [&str; 2],
    cancel: &CancelToken,
) -> Result<String, String> {
    let diff = diff_lines(
        DiffInput {
            bytes: left.bytes(),
            lines: left.lines(),
        },
        DiffInput {
            bytes: right.bytes(),
            lines: right.lines(),
        },
        DiffOptions {
            ignore_eol: true,
            ..DiffOptions::default()
        },
        cancel,
    )
    .map_err(|err| err.to_string())?;
    Ok(unified_diff(
        left,
        right,
        &diff,
        &PatchOptions {
            left_name: names[0],
            right_name: names[1],
            context: PATCH_CONTEXT,
        },
    ))
}

/// Builds the report's text. Runs off the UI thread: finishing the snapshot indexes lines.
fn build(kind: Kind, input: Input) -> Result<String, String> {
    let (left, right, _) = input.snapshot.finish();
    let title = format!("{} vs {}", input.names[0], input.names[1]);
    let meta = ReportMeta {
        left: &input.names[0],
        right: &input.names[1],
        date: &input.date,
        status: &input.status,
        title: &title,
    };
    match kind {
        Kind::Patch => {
            let name = |side: usize, fallback: &'static str| {
                Some(input.files[side].as_str())
                    .filter(|name| !name.is_empty())
                    .unwrap_or(fallback)
            };
            patch_of(
                &left,
                &right,
                [name(0, "left"), name(1, "right")],
                &input.cancel,
            )
        }
        Kind::Text => Ok(text_report(&left, &right, &input.diff, &meta)),
        Kind::Html(mode) => Ok(html_report(
            &left,
            &right,
            &input.rows,
            mode,
            input.ignoring,
            input.tab_width,
            &meta,
            HTML_MAX_ROWS,
        )),
    }
}

impl TextView {
    /// A result is shown and nothing is running.
    pub fn can_report(&self) -> bool {
        self.model.is_some() && !self.is_busy()
    }

    fn report_input(&self) -> Option<Input> {
        let model = self.model.as_ref().filter(|_| !self.is_busy())?;
        let (diff, rows) = model.report_parts();
        let path = |left: bool| {
            if left {
                &self.request.left
            } else {
                &self.request.right
            }
        };
        let name = |left: bool| match file_name(path(left)) {
            name if name.is_empty() => "untitled".to_owned(),
            name => name,
        };
        Some(Input {
            snapshot: model.begin_snapshot(),
            diff,
            rows,
            ignoring: self.request.ignoring(),
            tab_width: usize::from(self.prefs.tab_width),
            status: status_text(model),
            names: [name(true), name(false)],
            files: [file_name(path(true)), file_name(path(false))],
            dir: path(true)
                .parent()
                .filter(|dir| !dir.as_os_str().is_empty())
                .map_or_else(home_dir, Path::to_path_buf),
            date: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
            cancel: self.report_cancel.0.clone(),
        })
    }

    /// The patch and the text report of a side over 64 MiB could run to gigabytes: they say so
    /// instead (the HTML page stops at its row cap).
    fn refused_as_too_large(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.mapped.contains(&true) {
            return false;
        }
        self.show_notice(TOO_LARGE, cx);
        true
    }

    /// Asks which lines the HTML report shows, then where to save it.
    pub fn report_html(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.report_input() else {
            return;
        };
        self.pull_shared(cx);
        let context = self.prefs.context_lines;
        let detail = format!("Around each difference: {context} lines of context.");
        let answer = window.prompt(
            PromptLevel::Info,
            HTML_QUESTION,
            Some(&detail),
            &HTML_ANSWERS,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let mode = match answer.await {
                Ok(0) => ReportMode::Context(context),
                Ok(1) => ReportMode::Differences,
                Ok(2) => ReportMode::All,
                _ => return,
            };
            let _ = this.update_in(cx, |view, window, cx| {
                view.save_report(Kind::Html(mode), input, window, cx)
            });
        })
        .detach();
    }

    pub fn report_patch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.refused_as_too_large(cx) {
            return;
        }
        if let Some(input) = self.report_input() {
            self.save_report(Kind::Patch, input, window, cx);
        }
    }

    pub fn report_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.refused_as_too_large(cx) {
            return;
        }
        if let Some(input) = self.report_input() {
            self.save_report(Kind::Text, input, window, cx);
        }
    }

    /// Asks where to save, then builds and writes the report off the UI thread.
    fn save_report(
        &mut self,
        kind: Kind,
        input: Input,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let file = report_name(&input.files[0], &input.files[1], kind.ext());
        let picked = cx.prompt_for_new_path(&input.dir, Some(&file));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let target = path.clone();
            let written = cx.background_executor().spawn(async move {
                let text = build(kind, input)?;
                save(&SaveRequest {
                    path: &target,
                    text: &text,
                    encoding: TextEncoding {
                        encoding: UTF_8,
                        bom: false,
                    },
                    eol: EolStyle::None,
                    expected: None,
                })
                .map(|_| ())
                .map_err(|err| err.to_string())
            });
            let result = written.await;
            let _ = this.update(cx, |view, cx| {
                view.show_notice(
                    match result {
                        Ok(()) => format!("Exported: {}.", path.display()),
                        Err(err) => format!("Can't export: {err}."),
                    },
                    cx,
                );
            });
        })
        .detach();
    }

    /// Puts the patch on the clipboard.
    pub fn copy_patch(&mut self, cx: &mut Context<Self>) {
        if self.refused_as_too_large(cx) {
            return;
        }
        let Some(input) = self.report_input() else {
            return;
        };
        let built = cx
            .background_executor()
            .spawn(async move { build(Kind::Patch, input) });
        cx.spawn(async move |this, cx| {
            let result = built.await;
            let _ = this.update(cx, |view, cx| match result {
                Ok(patch) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(patch));
                    view.show_notice("Patch copied to the clipboard.", cx);
                }
                Err(err) => view.show_notice(format!("Can't copy the patch: {err}."), cx),
            });
        })
        .detach();
    }

    /// Test hook: which sides the last result read straight from disk (over 64 MiB).
    #[doc(hidden)]
    pub fn set_mapped_for_test(&mut self, mapped: [bool; 2]) {
        self.mapped = mapped;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::testing::model_for;

    #[test]
    fn a_cancelled_patch_stops() {
        let (_dir, model) = model_for("a\nb\n", "a\nc\n");
        let (left, right, _) = model.snapshot();
        let names = ["a.txt", "b.txt"];
        assert!(patch_of(&left, &right, names, &CancelToken::new()).is_ok());
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(patch_of(&left, &right, names, &cancel).is_err());
    }
}
