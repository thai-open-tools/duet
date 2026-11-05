//! The Report ▾ menu, the export panel and the export job (R9–R11; LocalDiff `rep:662-760`).
//!
//! The panel is drawn over the row list like the sync panel, with its own focus and Esc, and
//! never beside it (PF14). Its fields write straight into `ExportChoices` (PF8). Create
//! remembers the choices, takes a cheap snapshot of the result on show (PF7) and asks where to
//! save first (R11). A job then builds the items, reads the differing files, builds the bytes
//! and writes them whole or not at all (`write_atomic`, PF9), all on the background executor.
//! Esc stops the job before the file is written; a compare that lands meanwhile does not touch
//! it, as it works on its own snapshot.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chrono::{DateTime, Local, Utc};
use duet_core::cancel::CancelToken;
use duet_core::folder::odd_scan::OddMarks;
use duet_core::folder::plan::expand;
use duet_core::folder::report::csv::folder_csv;
use duet_core::folder::report::html::folder_html;
use duet_core::folder::report::lines::{LineOptions, line_details};
use duet_core::folder::report::text::folder_text;
use duet_core::folder::report::xlsx::folder_xlsx;
use duet_core::folder::report::{
    ReportContext, ReportFormat, ReportOptions, ReportScope, criterion_text, default_names,
    is_report_file, report_file_name, report_items, rules_text, write_atomic,
};
use duet_core::folder::run::FolderTextRules;
use duet_core::folder::tree::FolderComparison;
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, Entity, FocusHandle, FontWeight, InteractiveElement, Subscription,
    Task, Window, actions, div, prelude::*, px,
};
use serde_json::{Value, json};

use crate::folder::model::FolderFilter;
use crate::folder::prefs::text_rules_of;
use crate::folder::view::FolderView;
use crate::format::{count_of, format_size};
use crate::i18n::{Lang, live, scoped, tr, tr_key, trf};
use crate::palette::Palette;
use crate::paths::home_dir;
use crate::settings;
use crate::text::prefs::{TextPrefs, pull_shared};
use crate::ui::edit_menu::{ld_input, ld_textarea};
use crate::ui::fonts::Fonts;
use crate::ui::menu::MenuEntry;
use crate::ui::prose::ProseExt as _;
use crate::ui::style::{
    LdExt as _, ld_button, ld_card, ld_primary, ld_segment, ld_segmented, ld_toggle,
};

actions!(
    folder,
    [ReportHtml, ReportXlsx, ReportText, ReportCsv, ExportConfirm]
);

/// The panel's key context, where Enter is `ExportConfirm`.
pub const CONTEXT: &str = "FolderExport";

/// The choices an export remembers (R9).
pub const REPORT_LINES: &str = "folder.report_lines";
pub const REPORT_CONTEXT: &str = "folder.report_context";
pub const REPORT_UNIMPORTANT: &str = "folder.report_unimportant";
pub const REPORT_MASK: &str = "folder.report_mask";
pub const REPORT_SAME: &str = "folder.report_same";
/// `[{ "pair": "{left}|{right}", "names": [A, B] }, …]`, most recent last.
pub const REPORT_NAMES: &str = "folder.report_names";

/// Folder pairs whose names are kept (`rptSaveNames`).
const NAMES_KEPT: usize = 30;
/// The fields' limits (LocalDiff's `maxlength`), applied as the text is read (PF19).
const NAME_MAX: usize = 60;
const TITLE_MAX: usize = 160;
const NOTE_MAX: usize = 2000;
const CONTEXT_MAX: u32 = 20;

const COMPARED_AGAIN: &str = tr_key("The folders were compared again, so the export was closed.");
const EXPORT_RUNNING: &str = tr_key("Wait for the export to finish.");
const NO_FILES: &str = tr_key("No files in the chosen scope.");
const TABLE_HELP: &str = tr_key(
    "The table has every file in the scope, same files too, one row per file with its line counts: filter it further in Excel.",
);
const LANG_HELP: &str = tr_key(
    "The language of the whole report, column names and status words included. Choose English if a script reads the file.",
);
const CRITERION_HELP: &str = tr_key(
    "The criterion does not read contents: choose Content to get differing lines and line counts.",
);
/// The two fields whose placeholder `relabel_export` sets again.
const TITLE_PLACEHOLDER: &str = tr_key("For example: check the config before deploying (optional)");
const NOTE_PLACEHOLDER: &str = tr_key("Optional");

/// Each format's radio: what it is, its id, its name and LocalDiff's words for it.
const FORMATS: [(ReportFormat, &str, &str, &str); 4] = [
    (
        ReportFormat::Html,
        "folder-export-format-html",
        tr_key("HTML"),
        tr_key("easy to read, with a summary and a chart, searchable, prints to PDF"),
    ),
    (
        ReportFormat::Xlsx,
        "folder-export-format-xlsx",
        tr_key("Excel (.xlsx)"),
        tr_key("team tracking: filters, status colours, columns for reviewers"),
    ),
    (
        ReportFormat::Csv,
        "folder-export-format-csv",
        tr_key("CSV"),
        tr_key("one table, to import into other programs or scripts"),
    ),
    (
        ReportFormat::Text,
        "folder-export-format-txt",
        tr_key("Text (.txt)"),
        tr_key("easy to attach to an email or chat"),
    ),
];

fn extension(format: ReportFormat) -> &'static str {
    match format {
        ReportFormat::Html => "html",
        ReportFormat::Xlsx => "xlsx",
        ReportFormat::Csv => "csv",
        ReportFormat::Text => "txt",
    }
}

/// The Report ▾ items. They wait while a compare runs and before a finished result.
pub fn report_menu(view: &FolderView) -> Vec<MenuEntry> {
    let on = view.can_report();
    vec![
        MenuEntry::action(tr("HTML report: summary and per-file details…"), ReportHtml).enabled(on),
        MenuEntry::action(tr("Excel (.xlsx): track the review as a team…"), ReportXlsx).enabled(on),
        MenuEntry::Separator,
        MenuEntry::action(tr("Text report (.txt)…"), ReportText).enabled(on),
        MenuEntry::action(tr("CSV: a table for other programs…"), ReportCsv).enabled(on),
    ]
}

/// Registers the menu's actions on the view's element.
pub(super) fn export_actions<E: InteractiveElement>(el: E, cx: &mut Context<FolderView>) -> E {
    let open = |format| {
        move |this: &mut FolderView, window: &mut Window, cx: &mut Context<FolderView>| {
            this.open_export(format, window, cx)
        }
    };
    let (html, xlsx, text, csv) = (
        open(ReportFormat::Html),
        open(ReportFormat::Xlsx),
        open(ReportFormat::Text),
        open(ReportFormat::Csv),
    );
    el.on_action(cx.listener(move |this, _: &ReportHtml, w, cx| html(this, w, cx)))
        .on_action(cx.listener(move |this, _: &ReportXlsx, w, cx| xlsx(this, w, cx)))
        .on_action(cx.listener(move |this, _: &ReportText, w, cx| text(this, w, cx)))
        .on_action(cx.listener(move |this, _: &ReportCsv, w, cx| csv(this, w, cx)))
}

/// What the panel is set to: the format and the report's choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportChoices {
    pub format: ReportFormat,
    pub options: ReportOptions,
}

/// The panel's text fields.
struct Fields {
    names: [Entity<InputState>; 2],
    title: Entity<InputState>,
    note: Entity<TextareaState>,
    context: Entity<InputState>,
}

#[derive(Clone, Copy)]
enum Field {
    NameA,
    NameB,
    Title,
    Note,
    Context,
}

/// How many files each scope holds, and whether a chip or the path filter hides any row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ScopeCounts {
    all: usize,
    filtered: usize,
    selected: usize,
    filtering: bool,
}

/// What the counts were worked out from: while the panel is open only the chips, the path
/// filter and the selection change it (a compare closes the panel).
#[derive(Debug, Clone, PartialEq, Eq)]
struct CountKey {
    epoch: u64,
    filter: FolderFilter,
    name_filter: Option<String>,
    chosen: usize,
    cursor: Option<usize>,
}

struct ExportPanel {
    choices: ExportChoices,
    /// The names the fields started with: an emptied name falls back to it (`val || nm[0]`).
    prefilled: [String; 2],
    /// The default names in the report's language now: a name still equal to one is not the
    /// person's, so it follows the language and is never remembered.
    defaults: [String; 2],
    fields: Fields,
    focus: FocusHandle,
    counts: Option<(CountKey, ScopeCounts)>,
    _subscriptions: Vec<Subscription>,
}

/// The export job that runs. Dropping it (the tab closes) stops the work.
struct RunningExport {
    id: u64,
    cancel: CancelToken,
    /// Differing files read so far, and how many there are to read (0 until known).
    done: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
    _ticker: Task<()>,
}

impl Drop for RunningExport {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl RunningExport {
    fn progress_text(&self) -> String {
        if self.cancel.is_cancelled() {
            return tr("Cancelling…").to_owned();
        }
        let total = self.total.load(Ordering::Relaxed);
        if total == 0 {
            return tr("Creating the report… · Esc to cancel").to_owned();
        }
        let done = self.done.load(Ordering::Relaxed).min(total);
        trf(
            "Reading the differing files… {} of {} · Esc to cancel",
            &[&format_size(done as u64), &format_size(total as u64)],
        )
    }
}

/// What the job shares with its `RunningExport`.
struct Started {
    id: u64,
    cancel: CancelToken,
    done: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
}

/// Why an export did not finish. A workbook too big for ZIP is told in the window's language when
/// the notice is made, not in the report's, on the job's thread.
enum ExportError {
    TooLarge,
    Write(String),
}

const WORKBOOK_TOO_LARGE: &str =
    tr_key("The workbook is too large for the Excel format (over 4 GiB)");

/// The notice for how an export ended, in the language in force.
fn export_notice(outcome: &Result<bool, ExportError>, path: &Path) -> String {
    match outcome {
        Ok(true) => trf("Exported: {}.", &[&path.display()]),
        Ok(false) => tr("Export cancelled.").to_owned(),
        Err(ExportError::TooLarge) => trf("Can't export: {}.", &[&tr(WORKBOOK_TOO_LARGE)]),
        Err(ExportError::Write(err)) => trf("Can't export: {}.", &[err]),
    }
}

/// The folder view's export state.
#[derive(Default)]
pub(super) struct ExportState {
    panel: Option<ExportPanel>,
    running: Option<RunningExport>,
    runs: u64,
    /// The focus of a panel closed without a window at hand (see `SyncState`).
    closed_focus: Option<FocusHandle>,
    /// Test hook: the job waits before building while set.
    hold: bool,
    /// The file name the last save dialog suggested (PF1: the test platform drops it).
    last_name: Option<String>,
}

/// Everything the job needs, taken on the UI thread at Create (PF7): shared or cheap values
/// only. The items are built from it in the job.
struct ExportJob {
    format: ReportFormat,
    options: ReportOptions,
    context: ReportContext,
    cmp: Arc<FolderComparison>,
    nodes: Vec<u32>,
    roots: [PathBuf; 2],
    odd: Option<OddMarks>,
    rules: FolderTextRules,
}

impl ExportJob {
    /// Builds the report and writes it to `path`. `Ok(false)` when cancelled, with nothing
    /// written.
    fn run(
        self,
        path: &Path,
        cancel: &CancelToken,
        done: &AtomicUsize,
        total: &AtomicUsize,
    ) -> Result<bool, ExportError> {
        if cancel.is_cancelled() {
            return Ok(false);
        }
        // This thread has no language of its own: the report's comes from its options, never
        // from the window's.
        let _lang = scoped(self.options.lang);
        let [left, right] = &self.roots;
        let items = report_items(&self.cmp, &self.nodes, left, right, self.odd.as_ref());
        let o = &self.options;
        // The counts are read whenever the criterion reads contents; "details" only decides
        // whether rows and sheet lines are kept (PF5).
        let to_read = items.iter().filter(|item| item.needs_lines()).count();
        let lines = (self.context.reads_contents && to_read > 0).then(|| {
            total.store(to_read, Ordering::Relaxed);
            let text = matches!(self.format, ReportFormat::Html | ReportFormat::Text);
            let options = LineOptions {
                rows: o.details && text,
                sheet: o.details && self.format == ReportFormat::Xlsx,
                context: o.context,
                mask: o.mask,
                ..LineOptions::default()
            };
            line_details(&items, &self.rules, &options, done, cancel)
        });
        if cancel.is_cancelled() {
            return Ok(false);
        }
        let (lines, context) = (lines.as_ref(), &self.context);
        let bytes = match self.format {
            ReportFormat::Html => folder_html(&items, lines, o, context).into_bytes(),
            ReportFormat::Text => {
                format!("\u{feff}{}", folder_text(&items, lines, o, context)).into_bytes()
            }
            ReportFormat::Csv => folder_csv(&items, lines, &o.names).into_bytes(),
            // M-7: a workbook past 4 GiB fails the export; nothing is written.
            ReportFormat::Xlsx => {
                folder_xlsx(&items, lines, o, context).map_err(|_| ExportError::TooLarge)?
            }
        };
        write_atomic(path, &bytes, cancel).map_err(|err| ExportError::Write(err.to_string()))
    }
}

/// The remembered names of `pair`, if any.
fn remembered_names(list: &Value, pair: &str) -> Option<[String; 2]> {
    let entry = list
        .as_array()?
        .iter()
        .rev()
        .find(|entry| entry.get("pair").and_then(Value::as_str) == Some(pair))?;
    match entry.get("names")?.as_array()?.as_slice() {
        [a, b] => Some([a.as_str()?.to_owned(), b.as_str()?.to_owned()]),
        _ => None,
    }
}

/// What of `names` is worth remembering: a name equal to a default, in either language, is the
/// program's wording and not the person's, so it is kept as empty and read back as the default
/// of the language then.
fn names_to_keep(names: &[String; 2], defaults: [[String; 2]; 2]) -> [String; 2] {
    std::array::from_fn(|i| {
        if defaults.iter().any(|d| d[i] == names[i]) {
            String::new()
        } else {
            names[i].clone()
        }
    })
}

/// The list with `pair`'s names moved to the end, the oldest dropped past `NAMES_KEPT`.
fn names_with(list: Option<Value>, pair: &str, names: &[String; 2]) -> Value {
    let mut entries = match list {
        Some(Value::Array(entries)) => entries,
        _ => Vec::new(),
    };
    entries.retain(|entry| entry.get("pair").and_then(Value::as_str) != Some(pair));
    entries.push(json!({ "pair": pair, "names": names }));
    let cut = entries.len().saturating_sub(NAMES_KEPT);
    entries.drain(..cut);
    Value::Array(entries)
}

/// At most `max` characters.
fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// `new`, the text an edit of `old` left in a field of at most `max` characters, as the field
/// may hold it: of what the edit put in, only the characters that fit stay (F-154), so typing
/// stops at the limit and a paste keeps its start. Also the caret, a byte offset just after
/// what stayed.
fn fit(old: &str, new: &str, max: usize) -> (String, usize) {
    let (old, new): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let shared = old.len().min(new.len());
    let start = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let end = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(shared - start)
        .take_while(|(a, b)| a == b)
        .count();
    let room = max.saturating_sub(start + end);
    let inserted = &new[start..new.len() - end];
    let kept = &inserted[..inserted.len().min(room)];
    let head: String = new[..start].iter().chain(kept).collect();
    let tail: String = new[new.len() - end..].iter().collect();
    let text = cut(&format!("{head}{tail}"), max);
    let caret = head.len().min(text.len());
    (text, caret)
}

/// The context field as LocalDiff reads it (`clamp(+v || 0, 0, 20)`), in whole lines.
fn parse_context(text: &str) -> u32 {
    match text.trim().parse::<f64>() {
        Ok(n) if n.is_finite() => n.floor().clamp(0., f64::from(CONTEXT_MAX)) as u32,
        _ => 0,
    }
}

impl FolderView {
    pub fn export_open(&self) -> bool {
        self.export.panel.is_some()
    }

    pub fn export_choices(&self) -> Option<&ExportChoices> {
        self.export.panel.as_ref().map(|panel| &panel.choices)
    }

    pub fn export_running(&self) -> bool {
        self.export.running.is_some()
    }

    /// A finished result is on show and no compare runs: the Report ▾ button and its items.
    pub fn can_report(&self) -> bool {
        !self.is_busy() && self.result_ready()
    }

    /// Test hook: while on, a started export waits before it builds anything.
    #[doc(hidden)]
    pub fn hold_export_for_test(&mut self, on: bool) {
        self.export.hold = on;
    }

    /// Test hook: the file name the last save dialog suggested (PF1).
    #[doc(hidden)]
    pub fn last_export_name_for_test(&self) -> Option<&str> {
        self.export.last_name.as_deref()
    }

    /// Test hook: what the name A, name B, title and note fields hold.
    #[doc(hidden)]
    pub fn export_field_texts_for_test(&self, cx: &App) -> Option<[String; 4]> {
        let f = &self.export.panel.as_ref()?.fields;
        let line = |field: &Entity<InputState>| field.read(cx).value().to_string();
        Some([
            line(&f.names[0]),
            line(&f.names[1]),
            line(&f.title),
            f.note.read(cx).value().to_string(),
        ])
    }

    /// Test hook: the placeholders the title and note fields show.
    #[doc(hidden)]
    pub fn export_placeholders_for_test(&self, cx: &App) -> Option<[String; 2]> {
        let f = &self.export.panel.as_ref()?.fields;
        Some([
            f.title.read(cx).presentation().placeholder().to_string(),
            f.note.read(cx).presentation().placeholder().to_string(),
        ])
    }

    /// PF14: true, with a notice, while a batch or an export runs. A copy, delete, touch or
    /// sync waits for either: a file the export holds open could not go to the Recycle Bin.
    pub(super) fn refuse_while_working(&mut self, cx: &mut Context<Self>) -> bool {
        if self.refuse_while_batch(cx) {
            return true;
        }
        if self.export.running.is_none() {
            return false;
        }
        self.notice = Some(tr(EXPORT_RUNNING).into());
        cx.notify();
        true
    }

    /// R11: true, with a notice, when an export cannot start now.
    fn refuse_export(&mut self, cx: &mut Context<Self>) -> bool {
        if self.refuse_while_batch(cx) {
            return true;
        }
        let why = if self.export.running.is_some() {
            Some(tr(EXPORT_RUNNING))
        } else if self.is_busy() {
            Some(tr("Wait for the compare to finish."))
        } else if !self.result_ready() {
            Some(tr("Compare the folders first."))
        } else {
            None
        };
        if let Some(why) = why {
            self.notice = Some(why.into());
            cx.notify();
        }
        why.is_some()
    }

    /// The two sides' default names as `lang` says them.
    fn default_names_in(&self, lang: Lang) -> [String; 2] {
        let _lang = scoped(lang);
        default_names(&self.left, &self.right)
    }

    /// The key the names are remembered under: the roots as typed.
    fn pair_key(&self) -> String {
        format!("{}|{}", self.left.display(), self.right.display())
    }

    /// Opens the panel for `format`, or switches an open one to it. The panel takes the keys;
    /// the sync panel closes (PF14).
    pub fn open_export(
        &mut self,
        format: ReportFormat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.refuse_export(cx) {
            return;
        }
        self.notice = None;
        self.close_sync_in(window, cx);
        self.export.closed_focus = None;
        if self.export.panel.is_none() {
            let panel = self.new_export_panel(format, window, cx);
            self.export.panel = Some(panel);
        }
        if let Some(panel) = &mut self.export.panel {
            panel.choices.format = format;
            window.focus(&panel.focus, cx);
        }
        cx.notify();
    }

    /// A panel with the remembered choices and names (R9), or LocalDiff's defaults.
    fn new_export_panel(
        &mut self,
        format: ReportFormat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ExportPanel {
        let saved = settings::settings_file(cx)
            .map(|file| settings::read_all(&file))
            .unwrap_or_default();
        let flag =
            |key: &str, default: bool| saved.get(key).and_then(Value::as_bool).unwrap_or(default);
        // The report starts in the language the window is in, and the panel does not save it.
        let lang = crate::i18n::current();
        let defaults = self.default_names_in(lang);
        // A remembered empty name is a default that was not kept.
        let names = saved
            .get(REPORT_NAMES)
            .and_then(|list| remembered_names(list, &self.pair_key()))
            .map(|kept| {
                let mut kept = kept;
                for (name, default) in kept.iter_mut().zip(&defaults) {
                    if name.is_empty() {
                        name.clone_from(default);
                    }
                }
                kept
            })
            .unwrap_or_else(|| defaults.clone());
        let context = saved
            .get(REPORT_CONTEXT)
            .and_then(Value::as_u64)
            .map_or(2, |n| n.min(u64::from(CONTEXT_MAX)) as u32);
        let options = ReportOptions {
            names: names.clone(),
            title: String::new(),
            note: String::new(),
            scope: ReportScope::All,
            details: self.options().mode.reads_contents() && flag(REPORT_LINES, true),
            context,
            mask: flag(REPORT_MASK, true),
            unimportant: flag(REPORT_UNIMPORTANT, true),
            same: flag(REPORT_SAME, false),
            lang,
        };
        let mut line = |value: &str, placeholder: &'static str| {
            let value = value.to_owned();
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            })
        };
        let fields = Fields {
            names: [line(&names[0], ""), line(&names[1], "")],
            title: line("", tr(TITLE_PLACEHOLDER)),
            context: line(&context.to_string(), ""),
            note: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(2, 6)
                    .placeholder(tr(NOTE_PLACEHOLDER))
            }),
        };
        let on_line = |field: Field| {
            move |this: &mut Self,
                  input: &Entity<InputState>,
                  event: &InputEvent,
                  window: &mut Window,
                  cx: &mut Context<Self>| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value().to_string();
                    if let Some((text, caret)) = this.export_field_changed(field, &text, cx) {
                        input.update(cx, |state, cx| {
                            state.set_value(text, window, cx);
                            state.set_selected_range(caret..caret, cx);
                        });
                    }
                }
            }
        };
        let subscriptions = vec![
            cx.subscribe_in(&fields.names[0], window, on_line(Field::NameA)),
            cx.subscribe_in(&fields.names[1], window, on_line(Field::NameB)),
            cx.subscribe_in(&fields.title, window, on_line(Field::Title)),
            cx.subscribe_in(&fields.context, window, on_line(Field::Context)),
            cx.subscribe_in(
                &fields.note,
                window,
                |this: &mut Self,
                 input: &Entity<TextareaState>,
                 event: &InputEvent,
                 window: &mut Window,
                 cx: &mut Context<Self>| {
                    if matches!(event, InputEvent::Change) {
                        let text = input.read(cx).value().to_string();
                        if let Some((text, caret)) =
                            this.export_field_changed(Field::Note, &text, cx)
                        {
                            input.update(cx, |state, cx| {
                                state.set_value(text, window, cx);
                                state.set_selected_range(caret..caret, cx);
                            });
                        }
                    }
                },
            ),
        ];
        ExportPanel {
            choices: ExportChoices { format, options },
            prefilled: names,
            defaults,
            fields,
            focus: cx.focus_handle().tab_stop(true),
            counts: None,
            _subscriptions: subscriptions,
        }
    }

    /// A field's text goes straight into the choices (PF8). An edit that takes a field past
    /// its limit keeps only what fits (`fit`, F-154): that text and its caret come back for the
    /// caller to put in the field.
    fn export_field_changed(
        &mut self,
        field: Field,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Option<(String, usize)> {
        let o = &mut self.export.panel.as_mut()?.choices.options;
        let (kept, max) = match field {
            Field::NameA => (&mut o.names[0], NAME_MAX),
            Field::NameB => (&mut o.names[1], NAME_MAX),
            Field::Title => (&mut o.title, TITLE_MAX),
            Field::Note => (&mut o.note, NOTE_MAX),
            Field::Context => {
                o.context = parse_context(text);
                cx.notify();
                return None;
            }
        };
        // `kept` is the field's text before this edit.
        let fitted = (text.chars().count() > max).then(|| fit(kept, text, max));
        *kept = fitted
            .as_ref()
            .map_or_else(|| text.to_owned(), |(t, _)| t.clone());
        cx.notify();
        fitted
    }

    fn change_export(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut ExportChoices)) {
        if let Some(panel) = &mut self.export.panel {
            change(&mut panel.choices);
            cx.notify();
        }
    }

    /// The report's language changed: a name still at its default is rebuilt in the new one,
    /// fields included, and a name the person typed stays.
    fn set_export_lang(&mut self, lang: Lang, window: &mut Window, cx: &mut Context<Self>) {
        let fresh = self.default_names_in(lang);
        let Some(panel) = &mut self.export.panel else {
            return;
        };
        panel.choices.options.lang = lang;
        for (i, new) in fresh.iter().enumerate() {
            let old = &panel.defaults[i];
            if panel.prefilled[i] == *old {
                panel.prefilled[i].clone_from(new);
            }
            if panel.choices.options.names[i] == *old {
                panel.choices.options.names[i].clone_from(new);
                let text = new.clone();
                panel.fields.names[i].update(cx, |state, cx| state.set_value(text, window, cx));
            }
        }
        panel.defaults = fresh;
        cx.notify();
    }

    /// Test hook: changes the choices as the panel's controls would, fields included (PF8).
    #[doc(hidden)]
    pub fn set_export_choices_for_test(
        &mut self,
        change: impl FnOnce(&mut ExportChoices),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = &mut self.export.panel else {
            return;
        };
        change(&mut panel.choices);
        let o = &panel.choices.options;
        let lines = [
            (&panel.fields.names[0], o.names[0].clone()),
            (&panel.fields.names[1], o.names[1].clone()),
            (&panel.fields.title, o.title.clone()),
            (&panel.fields.context, o.context.to_string()),
        ];
        for (field, text) in lines {
            field.update(cx, |state, cx| state.set_value(text, window, cx));
        }
        let note = o.note.clone();
        panel
            .fields
            .note
            .update(cx, |state, cx| state.set_value(note, window, cx));
        cx.notify();
    }

    /// The nodes of `scope` that are report files, in tree order (PF13: one predicate).
    fn scope_nodes(&self, scope: ReportScope) -> Vec<u32> {
        let Some(model) = &self.model else {
            return Vec::new();
        };
        let cmp = model.comparison();
        let files = (0..cmp.nodes.len() as u32).filter(|&n| is_report_file(&cmp.nodes[n as usize]));
        match scope {
            ReportScope::All => files.collect(),
            ReportScope::Filtered => files.filter(|&n| model.passes_filters(n)).collect(),
            ReportScope::Selected => {
                expand(cmp, &model.picked(), &|n| model.passes_filters(n)).files
            }
        }
    }

    /// The scopes' file counts, worked out again only when what they depend on changed (PF14:
    /// at render, from the live model). A scope no longer offered falls back to All.
    fn export_counts(&mut self) -> ScopeCounts {
        let Some(model) = &self.model else {
            return ScopeCounts::default();
        };
        let key = CountKey {
            epoch: self.plan_epoch,
            filter: model.filter(),
            name_filter: model.name_filter().map(str::to_owned),
            chosen: model.chosen_count(),
            cursor: model.selected_row(),
        };
        let filtering = key.filter != FolderFilter::All || key.name_filter.is_some();
        if let Some((_, counts)) = self
            .export
            .panel
            .as_ref()
            .and_then(|panel| panel.counts.as_ref())
            .filter(|(old, _)| *old == key)
        {
            return *counts;
        }
        let counts = ScopeCounts {
            all: self.scope_nodes(ReportScope::All).len(),
            filtered: if filtering {
                self.scope_nodes(ReportScope::Filtered).len()
            } else {
                0
            },
            selected: self.scope_nodes(ReportScope::Selected).len(),
            filtering,
        };
        if let Some(panel) = &mut self.export.panel {
            panel.counts = Some((key, counts));
            let scope = &mut panel.choices.options.scope;
            let gone = match scope {
                ReportScope::All => false,
                ReportScope::Filtered => !counts.filtering,
                ReportScope::Selected => counts.selected == 0,
            };
            if gone {
                *scope = ReportScope::All;
            }
        }
        counts
    }

    fn set_export_scope(&mut self, scope: ReportScope, cx: &mut Context<Self>) {
        let counts = self.export_counts();
        let offered = match scope {
            ReportScope::All => true,
            ReportScope::Filtered => counts.filtering,
            ReportScope::Selected => counts.selected > 0,
        };
        if offered {
            self.change_export(cx, |c| c.options.scope = scope);
        }
    }

    /// The scope as the report names it, in the language `export_context` has set.
    fn scope_text(&self, scope: ReportScope) -> String {
        match scope {
            ReportScope::All => tr("All items").to_owned(),
            ReportScope::Selected => tr("Selected items only").to_owned(),
            ReportScope::Filtered => {
                let Some(model) = &self.model else {
                    return String::new();
                };
                let mut text = trf("As filtered: “{}”", &[&tr(model.filter().chip_label())]);
                if let Some(needle) = model.name_filter() {
                    text.push_str(&trf(" + path filter “{}”", &[&needle]));
                }
                text
            }
        }
    }

    /// The choices as Create uses them: names trimmed (an empty one is the name it started
    /// with), equal names told apart, title and note trimmed (`rep:722-733`).
    fn finished_choices(&self) -> Option<ExportChoices> {
        let panel = self.export.panel.as_ref()?;
        let mut choices = panel.choices.clone();
        let o = &mut choices.options;
        for (name, prefilled) in o.names.iter_mut().zip(&panel.prefilled) {
            let trimmed = name.trim();
            *name = if trimmed.is_empty() {
                prefilled.clone()
            } else {
                trimmed.to_owned()
            };
        }
        if o.names[0] == o.names[1] {
            o.names = [format!("{} (A)", o.names[0]), format!("{} (B)", o.names[1])];
        }
        o.title = o.title.trim().to_owned();
        o.note = o.note.trim().to_owned();
        if !self.options().mode.reads_contents() {
            o.details = false;
        }
        Some(choices)
    }

    /// Writes what Create used, as LocalDiff does: the names for this pair always; the lines
    /// and masking choices unless CSV (and the lines only when they could be chosen); context,
    /// the unimportant list and the same list for HTML and text (R9).
    fn remember_export(&self, choices: &ExportChoices, cx: &mut Context<Self>) {
        let Some(file) = settings::settings_file(cx) else {
            return;
        };
        let (format, o) = (choices.format, &choices.options);
        let names = names_with(
            settings::get_json(&file, REPORT_NAMES),
            &self.pair_key(),
            &names_to_keep(
                &o.names,
                [
                    self.default_names_in(Lang::En),
                    self.default_names_in(Lang::Th),
                ],
            ),
        );
        let mut writes: Vec<(&str, Value)> = vec![(REPORT_NAMES, names)];
        if format != ReportFormat::Csv {
            if self.options().mode.reads_contents() {
                writes.push((REPORT_LINES, o.details.into()));
            }
            writes.push((REPORT_MASK, o.mask.into()));
        }
        if matches!(format, ReportFormat::Html | ReportFormat::Text) {
            writes.push((REPORT_CONTEXT, o.context.into()));
            writes.push((REPORT_UNIMPORTANT, o.unimportant.into()));
            writes.push((REPORT_SAME, o.same.into()));
        }
        for (key, value) in writes {
            if let Err(err) = settings::set_json(&file, key, value) {
                tracing::warn!(%err, key, "could not remember a report choice");
            }
        }
    }

    /// What the report says about the compare, from the same text choices as its line compare
    /// (G12).
    fn export_context(
        &self,
        lang: Lang,
        scope: ReportScope,
        rules: &FolderTextRules,
        tab_width: usize,
        now: DateTime<Local>,
    ) -> ReportContext {
        // The text this builds from labels reads in the report's language, not the window's.
        let _lang = scoped(lang);
        let options = self.options();
        ReportContext {
            side_paths: [
                self.left.display().to_string(),
                self.right.display().to_string(),
            ],
            criterion: criterion_text(options.mode).to_owned(),
            scope: self.scope_text(scope),
            include: options.filters.include.join("; "),
            exclude: options.filters.exclude.join("; "),
            rules: Some(rules_text(rules)),
            generated: now.naive_local(),
            generated_utc: now
                .with_timezone(&Utc)
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            reads_contents: options.mode.reads_contents(),
            tab_width,
            invisible_scan: self.odd_marks().is_some(),
        }
    }

    /// Enter in the panel (PF2): only on the panel itself, so a field or a focused button
    /// takes its own Enter.
    fn export_confirm(&mut self, _: &ExportConfirm, window: &mut Window, cx: &mut Context<Self>) {
        let on_panel = self
            .export
            .panel
            .as_ref()
            .is_some_and(|panel| panel.focus.is_focused(window));
        if on_panel {
            self.start_export(window, cx);
        } else {
            cx.propagate();
        }
    }

    /// Create (R11): remembers the choices, takes the snapshot, closes the panel and asks
    /// where to save; the job starts once a path is chosen.
    pub fn start_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.export.panel.is_none() || self.refuse_export(cx) {
            return;
        }
        // The counts first: they drop a scope that is no longer offered.
        self.export_counts();
        let Some(choices) = self.finished_choices() else {
            return;
        };
        self.remember_export(&choices, cx);
        let nodes = self.scope_nodes(choices.options.scope);
        if nodes.is_empty() {
            self.notice = Some(tr(NO_FILES).into());
            cx.notify();
            return;
        }
        let Some(model) = &self.model else {
            return;
        };
        let mut prefs = TextPrefs::load(settings::settings_file(cx).as_deref());
        pull_shared(&mut prefs, cx);
        let rules = text_rules_of(&prefs);
        let now = Local::now();
        let context = self.export_context(
            choices.options.lang,
            choices.options.scope,
            &rules,
            usize::from(prefs.tab_width),
            now,
        );
        let job = ExportJob {
            format: choices.format,
            options: choices.options,
            context,
            cmp: model.shared_comparison(),
            nodes,
            roots: [self.left.clone(), self.right.clone()],
            odd: self.odd_marks().cloned(),
            rules,
        };
        self.close_export_in(window, cx);
        let file = report_file_name(&job.options.names, now.date_naive(), extension(job.format));
        self.export.last_name = Some(file.clone());
        let dir = self
            .left
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .map_or_else(home_dir, Path::to_path_buf);
        let picked = cx.prompt_for_new_path(&dir, Some(&file));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let Ok(Some(started)) = this.update(cx, |view, cx| view.begin_export(cx)) else {
                return;
            };
            let Started {
                id,
                cancel,
                done,
                total,
            } = started;
            loop {
                let held = this.update(cx, |view, _| view.export.hold).unwrap_or(false);
                if !held || cancel.is_cancelled() {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(10))
                    .await;
            }
            let target = path.clone();
            let outcome = cx
                .background_executor()
                .spawn(async move { job.run(&target, &cancel, &done, &total) })
                .await;
            let _ = this.update(cx, |view, cx| view.finish_export(id, outcome, &path, cx));
        })
        .detach();
    }

    /// Registers the job and shows its progress every 100 ms. None, with a notice, when a
    /// batch or another export started while the dialog was open.
    fn begin_export(&mut self, cx: &mut Context<Self>) -> Option<Started> {
        if self.refuse_while_working(cx) {
            return None;
        }
        self.notice = None;
        self.export.runs += 1;
        let id = self.export.runs;
        let cancel = CancelToken::new();
        let (done, total) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let running = this
                    .update(cx, |view, cx| {
                        view.show_export_progress();
                        cx.notify();
                        view.export.running.is_some()
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        });
        self.export.running = Some(RunningExport {
            id,
            cancel: cancel.clone(),
            done: done.clone(),
            total: total.clone(),
            _ticker: ticker,
        });
        self.show_export_progress();
        cx.notify();
        Some(Started {
            id,
            cancel,
            done,
            total,
        })
    }

    /// The job's line in the status bar, unless a compare shows its own.
    fn show_export_progress(&mut self) {
        if self.is_busy() {
            return;
        }
        if let Some(text) = self
            .export
            .running
            .as_ref()
            .map(RunningExport::progress_text)
        {
            self.set_status(text);
        }
    }

    /// The job ended: the compare's line comes back, and the notice says how it went.
    fn finish_export(
        &mut self,
        id: u64,
        outcome: Result<bool, ExportError>,
        path: &Path,
        cx: &mut Context<Self>,
    ) {
        if self.export.running.as_ref().map(|run| run.id) != Some(id) {
            return;
        }
        self.export.running = None;
        if !self.is_busy() {
            self.refresh_status();
        }
        self.notice = Some(export_notice(&outcome, path).into());
        cx.notify();
    }

    /// Esc: stops a running export before its file is written. False when none runs.
    pub(super) fn cancel_export(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(run) = &self.export.running else {
            return false;
        };
        run.cancel.cancel();
        self.set_status_live(live("Cancelling…"));
        cx.notify();
        true
    }

    /// The language changed: the two fields that keep a placeholder set it again. The running
    /// export's line follows on its next tick.
    pub(super) fn relabel_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = &self.export.panel else {
            return;
        };
        panel.fields.title.update(cx, |state, cx| {
            state.set_placeholder(tr(TITLE_PLACEHOLDER), window, cx)
        });
        panel.fields.note.update(cx, |state, cx| {
            state.set_placeholder(tr(NOTE_PLACEHOLDER), window, cx)
        });
    }

    /// Closes the panel. Without a window at hand, the next frame gives the rows the focus if
    /// the panel held it.
    pub(super) fn close_export(&mut self, cx: &mut Context<Self>) {
        if let Some(panel) = self.export.panel.take() {
            self.export.closed_focus = Some(panel.focus);
            cx.notify();
        }
    }

    /// Closes the panel; the rows take the focus if it was in the panel.
    pub(super) fn close_export_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.export.panel.take() {
            if panel.focus.contains_focused(window, cx) {
                window.focus(&self.focus, cx);
            }
            cx.notify();
        }
    }

    /// Any compare closes the panel: its choices describe the result being replaced.
    pub(super) fn close_export_for_compare(&mut self, cx: &mut Context<Self>) {
        if self.export.panel.is_some() {
            self.close_export(cx);
            self.notice = Some(tr(COMPARED_AGAIN).into());
        }
    }

    /// The panel over the row list (R9), or None while it is closed.
    pub(super) fn render_export_panel(
        &mut self,
        p: &Palette,
        fonts: &Fonts,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if let Some(old) = self.export.closed_focus.take()
            && (window.focused(cx).is_none() || old.contains_focused(window, cx))
        {
            window.focus(&self.focus, cx);
        }
        self.export.panel.as_ref()?;
        let counts = self.export_counts();
        let reads = self.options().mode.reads_contents();
        let panel = self.export.panel.as_ref()?;
        let (format, o) = (panel.choices.format, &panel.choices.options);
        let text_like = matches!(format, ReportFormat::Html | ReportFormat::Text);
        let table = matches!(format, ReportFormat::Xlsx | ReportFormat::Csv);
        let csv = format == ReportFormat::Csv;
        let row = |label: &'static str, control: AnyElement| {
            h_flex()
                .items_start()
                .gap(px(10.))
                .child(
                    div()
                        .flex_none()
                        .w(px(180.))
                        .pt(px(4.))
                        .text_color(p.muted)
                        .child(label),
                )
                .child(div().flex_1().min_w_0().child(control))
        };
        let field =
            |id: &'static str, width: f32, input: &Entity<InputState>, name: &'static str| {
                div()
                    .id(id)
                    .test_support()
                    .w(px(width))
                    .child(ld_input(input).small().aria_label(name))
                    .into_any_element()
            };
        let formats = FORMATS.iter().map(|&(each, id, name, text)| {
            let on = format == each;
            ld_card(id, format!("{}: {}", tr(name), tr(text)), p)
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(6.))
                .px(px(9.))
                .py(px(5.))
                .rounded(px(8.))
                .when(on, |card| card.border_color(p.accent).bg(p.panel3))
                .aria_selected(on)
                .child(div().font_weight(FontWeight::SEMIBOLD).child(tr(name)))
                .child(div().text_color(p.dim).prose(format!("— {}", tr(text))))
                .ld_click(
                    true,
                    cx.listener(move |this, _, _, cx| this.change_export(cx, |c| c.format = each)),
                )
        });
        let scope = ld_segmented(
            "folder-export-scope",
            [
                ld_segment(
                    "folder-export-scope-all",
                    trf("All items ({})", &[&format_size(counts.all as u64)]),
                    o.scope == ReportScope::All,
                    false,
                    p,
                )
                .ld_click(
                    true,
                    cx.listener(|this, _, _, cx| this.set_export_scope(ReportScope::All, cx)),
                ),
                ld_segment(
                    "folder-export-scope-filtered",
                    tr("As filtered now"),
                    o.scope == ReportScope::Filtered,
                    false,
                    p,
                )
                .ld_click(
                    counts.filtering,
                    cx.listener(|this, _, _, cx| this.set_export_scope(ReportScope::Filtered, cx)),
                ),
                ld_segment(
                    "folder-export-scope-selected",
                    trf(
                        "Selected items only ({})",
                        &[&count_of(counts.selected as u64, "file", "files")],
                    ),
                    o.scope == ReportScope::Selected,
                    true,
                    p,
                )
                .ld_click(
                    counts.selected > 0,
                    cx.listener(|this, _, _, cx| this.set_export_scope(ReportScope::Selected, cx)),
                ),
            ],
            p,
        );
        let lang = ld_segmented(
            "folder-export-lang",
            [
                ld_segment(
                    ("folder-export-lang", 0usize),
                    "ไทย",
                    o.lang == Lang::Th,
                    false,
                    p,
                )
                .ld_click(
                    true,
                    cx.listener(|this, _, window, cx| this.set_export_lang(Lang::Th, window, cx)),
                ),
                ld_segment(
                    ("folder-export-lang", 1usize),
                    "English",
                    o.lang == Lang::En,
                    true,
                    p,
                )
                .ld_click(
                    true,
                    cx.listener(|this, _, window, cx| this.set_export_lang(Lang::En, window, cx)),
                ),
            ],
            p,
        );
        let details_label = if format == ReportFormat::Xlsx {
            tr("Add a “Changed lines” sheet (one row per line)")
        } else {
            tr("Show each file's differing lines")
        };
        let toggle = |id: &'static str,
                      label: &'static str,
                      on: bool,
                      enabled: bool,
                      flip: fn(&mut ReportOptions)| {
            ld_toggle(id, label, on, p).ld_click(
                enabled,
                cx.listener(move |this, _, _, cx| this.change_export(cx, |c| flip(&mut c.options))),
            )
        };
        let contents = v_flex()
            .gap(px(4.))
            .child(
                h_flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(10.))
                    .child(toggle(
                        "folder-export-details",
                        details_label,
                        o.details,
                        reads,
                        |o| o.details = !o.details,
                    ))
                    .when(text_like, |line| {
                        line.child(
                            h_flex()
                                .items_center()
                                .gap(px(6.))
                                .child(div().text_color(p.muted).child(tr("Context lines")))
                                .child(field(
                                    "folder-export-context",
                                    56.,
                                    &panel.fields.context,
                                    tr("Context lines"),
                                )),
                        )
                    }),
            )
            .child(toggle(
                "folder-export-mask",
                tr("Hide values that look like passwords, keys or tokens (recommended before sharing)"),
                o.mask,
                true,
                |o| o.mask = !o.mask,
            ))
            .when(text_like, |list| {
                list.child(toggle(
                    "folder-export-unimportant",
                    tr("List files that differ only in unimportant parts (EOL, BOM, rules)"),
                    o.unimportant,
                    true,
                    |o| o.unimportant = !o.unimportant,
                ))
                .child(toggle(
                    "folder-export-same",
                    tr("List the same files too"),
                    o.same,
                    true,
                    |o| o.same = !o.same,
                ))
            });
        let help = |id: &'static str, text: &'static str| {
            div()
                .id(id)
                .test_support()
                .aria_label(text)
                .text_size(px(11.5))
                .text_color(p.dim)
                .prose(text)
        };
        let fields = v_flex()
            .id("folder-export-fields")
            .test_support()
            .min_h(px(72.))
            .overflow_y_scroll()
            .gap(px(8.))
            .child(row(
                tr("Format"),
                v_flex().gap(px(4.)).children(formats).into_any_element(),
            ))
            .child(row(
                tr("Name for the left side (A)"),
                field(
                    "folder-export-name-a",
                    260.,
                    &panel.fields.names[0],
                    tr("Name for the left side (A)"),
                ),
            ))
            .child(row(
                tr("Name for the right side (B)"),
                field(
                    "folder-export-name-b",
                    260.,
                    &panel.fields.names[1],
                    tr("Name for the right side (B)"),
                ),
            ))
            .when(!csv, |body| {
                body.child(row(
                    tr("Report title"),
                    field(
                        "folder-export-title",
                        420.,
                        &panel.fields.title,
                        tr("Report title"),
                    ),
                ))
                .child(row(
                    tr("Note"),
                    div()
                        .id("folder-export-note")
                        .test_support()
                        .w(px(420.))
                        .font(fonts.ui.clone())
                        .child(ld_textarea(&panel.fields.note))
                        .into_any_element(),
                ))
            })
            .child(row(tr("Scope"), scope.into_any_element()))
            .child(row(tr("Report language"), lang.into_any_element()))
            .child(help("folder-export-help-lang", tr(LANG_HELP)))
            .when(!csv, |body| {
                body.child(row(tr("Contents"), contents.into_any_element()))
            })
            .when(table, |body| {
                body.child(help("folder-export-help-table", tr(TABLE_HELP)))
            })
            .when(!reads, |body| {
                body.child(help("folder-export-help-criterion", tr(CRITERION_HELP)))
            });
        let body = v_flex()
            .id("folder-export-panel")
            .test_support()
            .track_focus(&panel.focus)
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::export_confirm))
            .w_full()
            .max_w(px(760.))
            // No taller than the window: the fields scroll, so Create stays on screen.
            .max_h_full()
            .p(px(14.))
            .gap(px(8.))
            .rounded(px(10.))
            .border_1()
            .border_color(p.line)
            .bg(p.panel)
            .text_size(px(12.5))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(14.))
                    .prose(tr("Create a report or export the compare")),
            )
            .child(fields)
            .child(
                h_flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(ld_button("folder-export-cancel", tr("Cancel"), p).ld_click(
                        true,
                        cx.listener(|this, _, window, cx| this.close_export_in(window, cx)),
                    ))
                    .child(
                        ld_primary("folder-export-create", tr("Create"), p).ld_click(
                            true,
                            cx.listener(|this, _, window, cx| this.start_export(window, cx)),
                        ),
                    ),
            );
        Some(
            div()
                .id("folder-export-overlay")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .overflow_y_scroll()
                .flex()
                .justify_center()
                .items_start()
                .p(px(12.))
                .bg(p.bg)
                .child(body)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workbook_too_large_is_told_in_the_window_language() {
        let path = Path::new("r.xlsx");
        let english = export_notice(&Err(ExportError::TooLarge), path);
        assert_eq!(
            english,
            "Can't export: The workbook is too large for the Excel format (over 4 GiB)."
        );
        let _th = scoped(Lang::Th);
        let thai = export_notice(&Err(ExportError::TooLarge), path);
        assert_ne!(thai, english);
        assert!(
            !thai.contains("workbook") && thai.contains("Excel"),
            "{thai}"
        );
    }

    fn pair_names(list: &Value, pair: &str) -> Option<[String; 2]> {
        remembered_names(list, pair)
    }

    #[test]
    fn names_are_kept_per_pair_most_recent_last_and_thirty_at_most() {
        let ab = ["A".to_owned(), "B".to_owned()];
        let list = names_with(None, "l|r", &ab);
        assert_eq!(list, json!([{ "pair": "l|r", "names": ["A", "B"] }]));
        assert_eq!(pair_names(&list, "l|r"), Some(ab.clone()));
        assert_eq!(
            pair_names(&list, "r|l"),
            None,
            "F9's reversed pair is another key"
        );
        let mut list = list;
        for i in 0..30 {
            list = names_with(Some(list), &format!("p{i}"), &ab);
        }
        let entries = list.as_array().unwrap();
        assert_eq!(entries.len(), 30);
        assert_eq!(entries[0]["pair"], "p0", "the oldest pair went");
        // Saving a pair again moves it to the end, with its new names.
        let xy = ["X".to_owned(), "Y".to_owned()];
        let list = names_with(Some(list), "p3", &xy);
        let entries = list.as_array().unwrap();
        assert_eq!((entries.len(), &entries[29]["pair"]), (30, &json!("p3")));
        assert_eq!(pair_names(&list, "p3"), Some(xy));
        assert_eq!(
            pair_names(&json!([{ "pair": "p", "names": ["only one"] }]), "p"),
            None
        );
        assert_eq!(
            names_with(Some(json!("garbage")), "p", &ab)
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn fields_are_read_within_their_limits() {
        assert_eq!(cut("abcdef", 3), "abc");
        assert_eq!(cut("ไทยไทย", 3), "ไทย", "characters, not bytes");
        assert_eq!(parse_context("2"), 2);
        assert_eq!(parse_context(" 7.9 "), 7);
        assert_eq!(parse_context("99"), 20);
        assert_eq!(parse_context("-3"), 0);
        assert_eq!(parse_context(""), 0);
        assert_eq!(parse_context("abc"), 0);
        assert_eq!(parse_context("inf"), 0);
    }

    /// F-154: an edit that would pass the limit keeps only what fits of what it inserted.
    #[test]
    fn an_edit_past_the_limit_keeps_what_fits() {
        // Typed at the end of a full field: refused, the caret stays at the end.
        assert_eq!(fit("abc", "abcd", 3), ("abc".to_owned(), 3));
        // Typed in the middle: refused, the caret stays where the text went in.
        assert_eq!(fit("abc", "aXbc", 3), ("abc".to_owned(), 1));
        // A paste keeps its first characters, up to the limit.
        assert_eq!(fit("ab", "a12345b", 5), ("a123b".to_owned(), 4));
        assert_eq!(fit("", "ไทยไทย", 4), ("ไทยไ".to_owned(), 12));
        // A paste over a selection: what it replaced makes room.
        assert_eq!(fit("abcde", "aXYZWVe", 5), ("aXYZe".to_owned(), 4));
        // Within the limit nothing changes.
        assert_eq!(fit("ab", "abc", 3), ("abc".to_owned(), 3));
        // An old text over the limit (never typed here) is cut at the limit.
        assert_eq!(fit("abcdef", "abcdefg", 3).0, "abc");
    }
}
