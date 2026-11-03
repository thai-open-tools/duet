//! The input stage (R10, R17, R18): the mode, the three slots (a file or pasted text), the
//! output box, drops, and what each slot says about itself.

use gpui_kit::component::h_flex;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::{
    AnyElement, ClickEvent, Div, ExternalPaths, FontWeight, HighlightStyle, Hsla,
    PathPromptOptions, Stateful, StyledText, relative,
};

use super::*;
use crate::format::{encoding_label, human_size};
use crate::pathbar::dropped_files_up_to;
use crate::start::clean_path;
use crate::status::{StatusPart, Tone};
use crate::ui::fonts::{CODE_SIZE, Fonts, SMALL_SIZE};
use crate::ui::icons;
use crate::ui::style::{
    LdExt as _, ld_button, ld_icon_button, ld_label_icon_button, ld_segment, ld_segmented,
    ld_separator, ld_small_icon_button, ld_tip, status_bar, status_line_box, status_parts_row,
};

/// A file bigger than this cannot be copied into a paste field: a field that size would freeze
/// the window (PF10).
const EDIT_MAX_BYTES: usize = 2 * 1024 * 1024;
const EDIT_MAX_LINES: u32 = 50_000;
const TOO_LARGE: &str = "This file is too large to edit here. Use files mode.";
const PASTE_STATUS: &str =
    "Paste mode: type or paste the text of all three sides, then press Merge.";
const CLEAR_ALL: &str = "Clear all three slots and the merge result you have not saved?";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Files,
    Paste,
}

/// What a slot shows: its name line and its meta line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SlotText {
    pub name: String,
    pub meta: String,
    /// The slot holds something to merge (drawn solid; an empty slot is dashed).
    pub filled: bool,
    pub edited: bool,
    pub failed: bool,
}

/// One slot's ids and copy.
struct Side {
    slot: &'static str,
    name: &'static str,
    meta: &'static str,
    open: &'static str,
    clear: &'static str,
    paste: &'static str,
    title: &'static str,
    note: &'static str,
    placeholder: &'static str,
}

const SIDES: [Side; 3] = [
    Side {
        slot: "merge-slot-left",
        name: "merge-name-left",
        meta: "merge-meta-left",
        open: "merge-open-left",
        clear: "merge-clear-left",
        paste: "merge-paste-left",
        title: "Left",
        note: "(ours)",
        placeholder: "Type or paste the left (ours) text here",
    },
    Side {
        slot: "merge-slot-base",
        name: "merge-name-base",
        meta: "merge-meta-base",
        open: "merge-open-base",
        clear: "merge-clear-base",
        paste: "merge-paste-base",
        title: "Base",
        note: "(common original)",
        placeholder: "Type or paste the common original (base) text here",
    },
    Side {
        slot: "merge-slot-right",
        name: "merge-name-right",
        meta: "merge-meta-right",
        open: "merge-open-right",
        clear: "merge-clear-right",
        paste: "merge-paste-right",
        title: "Right",
        note: "(theirs)",
        placeholder: "Type or paste the right (theirs) text here",
    },
];

/// "LF", or "LF (mixed)" when the file also has other line endings.
pub(super) fn eol_label(text: &MergeText) -> String {
    let label = text.format.eol.label();
    if text.format.mixed {
        format!("{label} (mixed)")
    } else {
        label.to_owned()
    }
}

fn too_large(text: &MergeText) -> bool {
    text.text().len() > EDIT_MAX_BYTES || text.line_count() > EDIT_MAX_LINES
}

/// A file's text for a paste field: its lines joined by LF, with a final newline when the
/// file has one (LocalDiff `docText`).
fn field_text(text: &MergeText) -> String {
    let count = text.line_count();
    let mut out = String::with_capacity(text.text().len());
    for n in 0..count {
        out.push_str(text.line(n));
        if n + 1 < count || text.format.final_newline {
            out.push('\n');
        }
    }
    out
}

/// Text with parts in a colour and bold, as one wrapping run.
fn rich(parts: &[(&str, Option<Hsla>)]) -> StyledText {
    let mut text = String::new();
    let mut marks = Vec::new();
    for (piece, color) in parts {
        let start = text.len();
        text.push_str(piece);
        if let Some(color) = color {
            marks.push((
                start..text.len(),
                HighlightStyle {
                    color: Some(*color),
                    font_weight: Some(FontWeight::BOLD),
                    ..Default::default()
                },
            ));
        }
    }
    StyledText::new(text).with_highlights(marks)
}

impl MergeView {
    /// The paste fields and the output box, with what their events do.
    pub(super) fn make_inputs(
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (
        [Entity<TextareaState>; 3],
        Entity<InputState>,
        Vec<Subscription>,
    ) {
        let mut subscriptions = Vec::new();
        let pastes = [LEFT, BASE, RIGHT].map(|slot| {
            let field = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder(SIDES[slot].placeholder)
                    .soft_wrap(false)
            });
            subscriptions.push(cx.subscribe_in(
                &field,
                window,
                move |this: &mut Self, _, event: &InputEvent, _, cx| {
                    // The first change marks the slot edited; filling a field emits none.
                    if matches!(event, InputEvent::Change) && !this.edited[slot] {
                        this.edited[slot] = true;
                        cx.notify();
                    }
                },
            ));
            field
        });
        let output = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("(optional) output file: leave empty to choose when saving")
        });
        subscriptions.push(cx.subscribe_in(
            &output,
            window,
            |this: &mut Self, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value();
                    let typed = clean_path(&text);
                    this.output = (!typed.is_empty()).then(|| PathBuf::from(typed));
                    cx.notify();
                }
            },
        ));
        (pastes, output, subscriptions)
    }

    pub fn mode(&self) -> InputMode {
        self.mode
    }

    /// Files or Paste text. Paste fills each unedited field with its file's text, unless a
    /// file is too big for a field (PF10).
    pub fn set_mode(&mut self, mode: InputMode, window: &mut Window, cx: &mut Context<Self>) {
        if mode == self.mode {
            return;
        }
        if mode == InputMode::Paste {
            let big = (LEFT..=RIGHT).any(|slot| {
                !self.edited[slot] && self.slots[slot].as_deref().is_some_and(too_large)
            });
            if big {
                self.notice = Some(TOO_LARGE.into());
                cx.notify();
                return;
            }
            for slot in LEFT..=RIGHT {
                if let (false, Some(file)) = (self.edited[slot], &self.slots[slot]) {
                    let text = field_text(file);
                    self.pastes[slot].update(cx, |field, cx| field.set_value(text, window, cx));
                }
            }
        }
        self.mode = mode;
        cx.notify();
    }

    /// Shows a stage. The merge stage needs a session.
    pub fn set_stage(&mut self, stage: Stage, cx: &mut Context<Self>) {
        if stage == Stage::Merge && self.session.is_none() {
            return;
        }
        self.stage = stage;
        cx.notify();
    }

    /// The slot holds a file or edited text.
    pub(super) fn filled(&self, slot: usize) -> bool {
        self.slots[slot].is_some() || self.edited[slot]
    }

    /// Merge ▸ can run: no file is being read, and either paste mode (an empty side merges as
    /// empty text) or all three slots are filled.
    pub fn can_run(&self) -> bool {
        !self.loads_running()
            && (self.mode == InputMode::Paste || (LEFT..=RIGHT).all(|slot| self.filled(slot)))
    }

    /// What `slot` shows; a slot past the right reads as blank.
    pub fn slot_text(&self, slot: usize) -> SlotText {
        let blank = SlotText::default();
        if slot > RIGHT {
            return blank;
        }
        if self.loading(slot) {
            return SlotText {
                name: self.names[slot].to_string(),
                meta: "Reading…".to_owned(),
                ..blank
            };
        }
        if let Some(message) = self.slot_message(slot) {
            return SlotText {
                name: self.names[slot].to_string(),
                meta: message.to_owned(),
                failed: true,
                ..blank
            };
        }
        let file = self.slot(slot);
        if self.edited[slot] {
            let (name, meta) = match file {
                Some(file) => (
                    format!("{} (edited)", file.name),
                    format!(
                        "Uses the text in the box below · keeps {} · {}",
                        encoding_label(&file.encoding),
                        eol_label(file)
                    ),
                ),
                None => (
                    "Typed or pasted text".to_owned(),
                    "Uses the text in the box below".to_owned(),
                ),
            };
            return SlotText {
                name,
                meta,
                filled: true,
                edited: true,
                ..blank
            };
        }
        match file {
            Some(file) => {
                let mut meta = format!(
                    "{} · {} · {} · {}",
                    count_of(u64::from(file.line_count()), "line", "lines"),
                    human_size(file.size),
                    encoding_label(&file.encoding),
                    eol_label(file)
                );
                if file.line_count() > 0 && !file.format.final_newline {
                    meta.push_str(" · no newline at the end");
                }
                if file.lossy {
                    meta.push_str(" · some bytes did not decode");
                }
                SlotText {
                    name: file.name.clone(),
                    meta,
                    filled: true,
                    ..blank
                }
            }
            None => SlotText {
                name: "No file chosen".to_owned(),
                meta: "Press “Open…” or drop a file here".to_owned(),
                ..blank
            },
        }
    }

    /// Empties a slot: its file, its message, its field and any read still running for it.
    pub fn clear_slot(&mut self, slot: usize, window: &mut Window, cx: &mut Context<Self>) {
        if slot >= self.slots.len() {
            return;
        }
        self.loads[slot].cancel();
        self.load_ids[slot] = 0;
        self.load_tasks[slot] = None;
        // A merge of the old slot is not wanted any more.
        self.merge_job.cancel();
        self.merge_task = None;
        self.slots[slot] = None;
        self.messages[slot] = None;
        self.failed[slot] = None;
        self.edited[slot] = false;
        self.names[slot] = SharedString::default();
        self.pastes[slot].update(cx, |field, cx| field.set_value("", window, cx));
        // With its last read gone, a Reload is over: a file chosen later must not merge over
        // choices without asking (M2).
        if !self.loads_running() {
            self.force_next_run = false;
            self.quiet_next_run = false;
            self.reloaded = None;
        }
        cx.notify();
    }

    /// Empties all three slots and the merge, back to the input stage. The output is kept
    /// (LocalDiff `js:845-853`). Choices that are not saved are asked about first.
    pub fn clear_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_dirty() {
            self.clear_everything(window, cx);
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            CLEAR_ALL,
            None,
            &["Cancel", "Clear"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(1) {
                let _ = this.update_in(cx, |view, window, cx| view.clear_everything(window, cx));
            }
        })
        .detach();
    }

    fn clear_everything(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A result field with the keys goes with the session; the output box stays.
        let in_result = self
            .result_fields()
            .any(|field| field.focus_handle(cx).contains_focused(window, cx));
        if in_result {
            window.focus(&self.focus, cx);
        }
        for slot in LEFT..=RIGHT {
            self.clear_slot(slot, window, cx);
        }
        self.session = None;
        self.rebuild_cards();
        self.refresh_preview(cx);
        self.force_next_run = false;
        self.quiet_next_run = false;
        self.reloaded = None;
        self.notice = None;
        self.stage = Stage::Input;
        cx.notify();
    }

    /// Files dropped on the view (R18): three or more fill left, base and right and switch to
    /// files mode; fewer start at the slot under the pointer, else the first empty slot, else
    /// the left, and wrap round. A drop returns to the input stage.
    pub fn drop_paths(
        &mut self,
        paths: &[PathBuf],
        target: Option<usize>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let used = match dropped_files_up_to(paths, 3) {
            Ok(used) => used.to_vec(),
            Err(notice) => {
                self.notice = Some(notice.into());
                cx.notify();
                return;
            }
        };
        if used.is_empty() {
            return;
        }
        let slots: Vec<usize> = if used.len() >= 3 {
            vec![LEFT, BASE, RIGHT]
        } else {
            let start = target
                .filter(|&slot| slot <= RIGHT)
                .or_else(|| (LEFT..=RIGHT).find(|&slot| !self.filled(slot)))
                .unwrap_or(LEFT);
            (0..used.len()).map(|k| (start + k) % 3).collect()
        };
        self.stage = Stage::Input;
        if used.len() >= 3 {
            self.mode = InputMode::Files;
        }
        for (slot, path) in slots.into_iter().zip(used) {
            self.start_load(slot, path, cx);
        }
        cx.notify();
    }

    /// The status bar's parts: the input stage's count, or the merge stage's line.
    pub fn status_parts(&self) -> Vec<StatusPart> {
        if self.stage == Stage::Merge {
            return self.merge_status_parts();
        }
        let text = match self.mode {
            InputMode::Paste => PASTE_STATUS.to_owned(),
            InputMode::Files => {
                let chosen = (LEFT..=RIGHT).filter(|&slot| self.filled(slot)).count();
                format!("{chosen} of 3 files chosen")
            }
        };
        vec![StatusPart::new(text, Tone::Muted)]
    }

    /// The three texts to merge, or None with a notice naming the slots that are not chosen.
    /// An edited slot gives its field's text (a file keeps its encoding, BOM and line ending,
    /// but has no path); in paste mode an empty slot gives its field's text too (R10).
    pub(super) fn gather(&mut self, cx: &mut Context<Self>) -> Option<[Arc<MergeText>; 3]> {
        let mut texts = Vec::with_capacity(3);
        let mut missing = Vec::new();
        for (slot, side_name) in SLOT_NAMES.into_iter().enumerate() {
            let typed = |this: &Self, cx: &App| this.pastes[slot].read(cx).value().to_string();
            if self.edited[slot] {
                let text = typed(self, cx);
                texts.push(Arc::new(match &self.slots[slot] {
                    Some(file) => {
                        let mut edited =
                            MergeText::new(text, file.encoding).keeping_eol(file.format.eol);
                        edited.name = file.name.clone();
                        edited
                    }
                    None => Self::typed_text(slot, text),
                }));
            } else if let Some(file) = &self.slots[slot] {
                texts.push(Arc::clone(file));
            } else if self.mode == InputMode::Paste {
                let text = typed(self, cx);
                texts.push(Arc::new(Self::typed_text(slot, text)));
            } else {
                missing.push(side_name);
            }
        }
        if !missing.is_empty() {
            self.notice = Some(format!("Not chosen yet: {}.", missing.join(", ")).into());
            cx.notify();
            return None;
        }
        let [left, base, right]: [Arc<MergeText>; 3] = texts.try_into().ok()?;
        Some([left, base, right])
    }

    /// Typed text: UTF-8 without a BOM, named after its slot ("(empty)" when it has none).
    fn typed_text(slot: usize, text: String) -> MergeText {
        let name = if text.is_empty() {
            "(empty)".to_owned()
        } else {
            format!("{} text", SLOT_NAMES[slot])
        };
        let mut typed = MergeText::typed(text);
        typed.name = name;
        typed
    }

    /// A file just landed in `slot` (or failed): the field follows. In paste mode it holds the
    /// file's text, unless that is too big for a field, which returns to files mode (PF10).
    pub(super) fn fill_field(&mut self, slot: usize, window: &mut Window, cx: &mut Context<Self>) {
        let mut text = String::new();
        if let (InputMode::Paste, Some(file)) = (self.mode, &self.slots[slot]) {
            if too_large(file) {
                self.mode = InputMode::Files;
                self.notice = Some(TOO_LARGE.into());
            } else {
                text = field_text(file);
            }
        }
        self.pastes[slot].update(cx, |field, cx| field.set_value(text, window, cx));
    }

    #[doc(hidden)]
    pub fn paste_input_for_test(&self, slot: usize) -> Entity<TextareaState> {
        self.pastes[slot].clone()
    }

    #[doc(hidden)]
    pub fn output_input_for_test(&self) -> Entity<InputState> {
        self.output_input.clone()
    }

    /// The focus handles of the view's inputs: the output box, the paste fields and the
    /// result fields made so far.
    pub(super) fn inputs(&self, cx: &App) -> Vec<FocusHandle> {
        let mut inputs = vec![self.output_input.focus_handle(cx)];
        inputs.extend(self.pastes.iter().map(|field| field.focus_handle(cx)));
        inputs.extend(self.result_fields().map(|field| field.focus_handle(cx)));
        inputs
    }

    /// Keys that mean something in a field stay with it: the view's guarded keys run only
    /// when no input of the view has the keys, else they propagate to the input.
    pub(super) fn takes_key(&self, window: &Window, cx: &mut Context<Self>) -> bool {
        let free = !self
            .inputs(cx)
            .iter()
            .any(|input| input.contains_focused(window, cx));
        if !free {
            cx.propagate();
        }
        free
    }

    /// Esc in a box or field of the view: the view takes the keys back, which ends the edit
    /// (a result field's typing session ends now, not at the next frame's blur). Returns
    /// whether it did.
    pub(super) fn leave_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let inside = self
            .inputs(cx)
            .iter()
            .any(|input| input.contains_focused(window, cx));
        if inside {
            window.focus(&self.focus, cx);
            if let Some(session) = self.session.as_mut() {
                session.end_edit();
            }
        }
        inside
    }

    /// Open…, Ctrl+O and Ctrl+Shift+O: pick a file for a slot.
    pub(super) fn pick_file(&mut self, slot: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.set_stage(Stage::Input, cx);
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |view, cx| view.open_slot_path(slot, path, cx));
        })
        .detach();
    }

    /// Ctrl+Enter merges from the input stage (free: it works from a field too).
    pub(super) fn on_run(&mut self, _: &MergeRun, window: &mut Window, cx: &mut Context<Self>) {
        if self.stage == Stage::Input && self.can_run() {
            self.run(false, window, cx);
        }
    }

    pub(super) fn on_open_left(
        &mut self,
        _: &MergeOpenLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.takes_key(window, cx) {
            self.pick_file(LEFT, window, cx);
        }
    }

    pub(super) fn on_open_right(
        &mut self,
        _: &MergeOpenRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.takes_key(window, cx) {
            self.pick_file(RIGHT, window, cx);
        }
    }

    /// The input stage: the bar, the slots with the hint, and the status bar.
    pub(super) fn render_input_stage(
        &self,
        root: Stateful<Div>,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let fonts = Fonts::current(cx);
        root.child(self.render_input_bar(p, &fonts, cx))
            .child(self.render_input_body(p, &fonts, cx))
            .child(self.render_input_status(p))
    }

    fn render_input_bar(
        &self,
        p: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mode_segment = |id: &'static str,
                            label: &'static str,
                            tip: &'static str,
                            mode: InputMode,
                            last: bool| {
            ld_segment(id, label, self.mode == mode, last, p)
                .ld_tooltip(tip)
                .ld_click(
                    true,
                    cx.listener(move |this, _, window, cx| this.set_mode(mode, window, cx)),
                )
        };
        let segments = [
            mode_segment(
                "merge-mode-files",
                "Files",
                "Open files or drop them here",
                InputMode::Files,
                false,
            ),
            mode_segment(
                "merge-mode-paste",
                "Paste text",
                "Type or paste the text of all three sides",
                InputMode::Paste,
                true,
            ),
        ];
        h_flex()
            .id("merge-input-bar")
            .test_support()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap(px(5.))
            .px(px(10.))
            .py(px(5.))
            .bg(p.panel)
            .border_b_1()
            .border_color(p.line)
            .child(ld_segmented("merge-mode", segments, p))
            .child(ld_separator(p))
            .child(div().text_color(p.muted).child("Output:"))
            .child(
                div()
                    .id("merge-output")
                    .test_support()
                    .flex_1()
                    .min_w(px(180.))
                    .max_w(px(460.))
                    .font(fonts.code.clone())
                    .text_size(px(CODE_SIZE))
                    .child(Input::new(&self.output_input).aria_label("Output file")),
            )
            .child(div().flex_1())
            .child(
                ld_icon_button("merge-swap", icons::SWAP, "Swap left and right (F9)", p).ld_click(
                    true,
                    cx.listener(|this, _, window, cx| this.swap_sides(window, cx)),
                ),
            )
            .child(
                ld_button("merge-clear", "Clear all", p)
                    .ld_tooltip("Empty all three slots")
                    .ld_click(
                        true,
                        cx.listener(|this, _, window, cx| this.clear_all(window, cx)),
                    ),
            )
            .child(
                ld_label_icon_button("merge-run", "Merge", icons::RUN, p)
                    .as_primary(p)
                    .ld_tooltip("Merge the three sides (Ctrl+Enter)")
                    .ld_click(
                        self.can_run(),
                        cx.listener(|this, _, window, cx| this.run(false, window, cx)),
                    ),
            )
    }

    fn render_input_body(
        &self,
        p: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let paste = self.mode == InputMode::Paste;
        let slots = h_flex()
            .gap(px(10.))
            .items_stretch()
            .when(paste, |d| d.flex_1().min_h_0())
            .children(
                SIDES
                    .iter()
                    .enumerate()
                    .map(|(slot, side)| self.render_slot(slot, side, p, fonts, cx)),
            );
        v_flex()
            .id("merge-input")
            .flex_1()
            .min_h_0()
            .p(px(14.))
            .gap(px(12.))
            .overflow_y_scroll()
            .child(slots)
            .when(!paste, |d| d.child(self.render_hint(p)))
    }

    fn render_slot(
        &self,
        slot: usize,
        side: &'static Side,
        p: &Palette,
        fonts: &Fonts,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let text = self.slot_text(slot);
        let paste = self.mode == InputMode::Paste;
        let title_color = [p.moved, p.muted, p.ins][slot];
        let path_tip: Option<SharedString> = self
            .slot(slot)
            .and_then(|file| file.path.as_ref())
            .map(|path| path.display().to_string().into());
        let (accent, panel3) = (p.accent, p.panel3);
        v_flex()
            .id(side.slot)
            .test_support()
            .flex_1()
            .min_w_0()
            .gap(px(6.))
            .px(px(12.))
            .py(px(10.))
            .bg(p.panel)
            .border_1()
            .border_color(p.line)
            .rounded(px(10.))
            .when(!text.filled, |d| d.border_dashed())
            .when(!paste, |d| d.min_h(px(112.)))
            .drag_over::<ExternalPaths>(move |style, _, _, _| {
                style.border_color(accent).border_dashed().bg(panel3)
            })
            .on_drop(
                cx.listener(move |this, dropped: &ExternalPaths, window, cx| {
                    cx.stop_propagation();
                    this.drop_paths(dropped.paths(), Some(slot), window, cx)
                }),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap(px(7.))
                    // In a narrow slot the title and its note wrap, so the buttons beside them
                    // stay whole (M-21).
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .flex_wrap()
                            .items_center()
                            .gap_x(px(7.))
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(title_color)
                                    .child(side.title),
                            )
                            .child(div().min_w_0().text_color(p.muted).child(side.note)),
                    )
                    .child(ld_button(side.open, "Open…", p).ld_small().ld_click(
                        true,
                        cx.listener(move |this, _, window, cx| this.pick_file(slot, window, cx)),
                    ))
                    .child(
                        ld_small_icon_button(side.clear, icons::CLOSE, "Empty this slot", p)
                            .ld_click(
                                true,
                                cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.clear_slot(slot, window, cx)
                                }),
                            ),
                    ),
            )
            .child(
                div()
                    .id(side.name)
                    .test_support()
                    .text_size(px(SMALL_SIZE))
                    .when(text.filled, |d| d.font(fonts.code.clone()))
                    .text_color(if text.filled { p.text } else { p.dim })
                    .when_some(path_tip, |d, tip| {
                        d.tooltip(move |window, cx| ld_tip(tip.clone(), window, cx))
                    })
                    .child(text.name.clone()),
            )
            .child(
                div()
                    .id(side.meta)
                    .test_support()
                    .text_size(px(11.5))
                    .text_color(if text.failed { p.del } else { p.muted })
                    .child(text.meta.clone()),
            )
            .when(paste, |d| {
                d.child(
                    div()
                        .id(side.paste)
                        .test_support()
                        .key_context(TEXT_CONTEXT)
                        .flex_1()
                        .min_h(px(200.))
                        .font(fonts.code.clone())
                        .text_size(px(CODE_SIZE))
                        .child(Textarea::new(&self.pastes[slot]).h(relative(1.))),
                )
            })
            .into_any_element()
    }

    /// The explanation under the slots in files mode.
    fn render_hint(&self, p: &Palette) -> impl IntoElement {
        let (left, base, right) = (Some(p.moved), Some(p.muted), Some(p.ins));
        v_flex()
            .id("merge-hint")
            .test_support()
            .px(px(20.))
            .py(px(28.))
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::BOLD)
                    .child("3-way merge: combine the edits of two sides."),
            )
            .child(rich(&[
                ("Choose ", None),
                ("Left", left),
                (" (your version), ", None),
                ("Base", base),
                (" (the common original both sides started from) and ", None),
                ("Right", right),
                (" (their version).", None),
            ]))
            .child(rich(&[
                (
                    "Parts changed on one side only, or changed the same way on both, merge on their own. Parts both sides changed differently are ",
                    None,
                ),
                ("conflicts", Some(p.del)),
                (
                    ": choose left, right, both, base or type the result, then save.",
                    None,
                ),
            ]))
            .child(
                div()
                    .text_color(p.dim)
                    .child("Drop 3 files at once for left, base and right in that order, drop 1 file on its slot, or switch to “Paste text”."),
            )
    }

    fn render_input_status(&self, p: &Palette) -> impl IntoElement {
        let parts = self.status_parts();
        let notice = self.notice.clone().map(|notice| {
            div()
                .id("merge-notice")
                .test_support()
                .flex_none()
                .text_color(p.text)
                .child(notice)
        });
        status_bar("merge-status", p)
            .child(if parts.is_empty() {
                status_line_box("")
            } else {
                status_parts_row(&parts, p)
            })
            .children(notice)
    }
}
