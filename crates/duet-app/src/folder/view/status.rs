//! The folder view's status bar: the compare summary and the note about the selection.

use crate::folder::model::FolderModel;
use crate::folder::ops::{selection_text, status_parts};
use crate::palette::Palette;
use crate::status::join_parts;
use crate::ui::style::{
    LdExt as _, ld_button, ld_tip, status_bar, status_line_box, status_parts_text,
};
use gpui_kit::TestSupportExt as _;
use gpui_kit::{AnyElement, SharedString, div, prelude::*, px};

use super::*;

impl FolderView {
    /// The note about the selected row, shown after the counts.
    pub(super) fn selection_note(&self) -> Option<String> {
        let m = self.model.as_ref()?;
        // After a Ctrl+click empties the set it reads 0 until the next rebuild: only many count.
        if m.chosen_count() > 1 {
            return Some(format!("{} selected", m.chosen_count()));
        }
        let index = m.node_index(m.selected_row()?);
        Some(selection_text(
            m.node(index),
            m.differing_below(index),
            self.options.mode,
        ))
    }

    /// The one builder of the compare summary: the counts of `model` in the status bar.
    pub(in crate::folder) fn set_model_status(&mut self, model: &FolderModel, done: bool) {
        let parts = status_parts(
            &model.counts(),
            model.filter(),
            &self.options,
            done,
            self.took,
            self.odd.status(self.prefs.odd_scan, done),
        );
        self.set_status(join_parts(&parts));
        self.status_parts = parts;
    }

    /// The status line: the compare summary plus a note about the selected row, or a notice
    /// in their place.
    pub fn status_line(&self) -> SharedString {
        if let Some(notice) = &self.notice {
            return notice.clone();
        }
        match self.selection_note() {
            Some(selection) => format!("{} · {selection}", self.status).into(),
            None => self.status.clone(),
        }
    }

    /// The bar under the list: the summary parts and the selection note, or a notice.
    pub(super) fn render_status(
        &mut self,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selection = self.selection_note();
        let bar = status_bar("folder-status", palette);
        let element = match (&self.notice, self.status_parts.is_empty()) {
            (None, false) => {
                let text = status_parts_text(&self.status_parts, palette);
                self.status_shown = Some(text.layout().clone());
                bar.child(status_line_box(text))
                    .children(selection.map(|note| {
                        div()
                            .id("folder-selection")
                            .test_support()
                            .flex_initial()
                            .min_w_0()
                            .max_w(px(520.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(note)
                    }))
            }
            _ => {
                self.status_shown = None;
                // A notice or progress line a narrow bar cuts short reads in full here.
                let line = self.status_line();
                let tip = line.clone();
                bar.child(
                    status_line_box(line)
                        .id("folder-status-text")
                        .test_support()
                        .tooltip(move |window, cx| ld_tip(tip.clone(), window, cx)),
                )
            }
        };
        let scan_button = self.odd.held_back.is_some() && self.prefs.odd_scan;
        element
            .when(scan_button, |bar| {
                bar.child(
                    ld_button("folder-odd-scan", "Look for invisible characters", palette)
                        .ld_small()
                        .ld_tooltip("Read every file up to 4 MiB and look for invisible characters")
                        .ld_click(
                            !self.job.is_running(),
                            cx.listener(|this, _, _, cx| this.scan_odd_now(cx)),
                        ),
                )
            })
            .into_any_element()
    }
}
