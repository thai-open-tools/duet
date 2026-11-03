//! The table view's names, navigation position and status line.

use super::*;
use crate::format::{encoding_label, format_size, human_size};
use crate::status::{StatusPart, Tone};
use crate::ui::style::{status_bar, status_line_box, status_parts_row};

const OPEN_OTHER_SIDE: &str = "Open the other side to start comparing.";

/// "comma (,)", with " (auto)" when it was detected.
pub(super) fn delimiter_label(info: &SideInfo) -> String {
    let label = info.delimiter.label().to_lowercase();
    if info.auto {
        format!("{label} (auto)")
    } else {
        label
    }
}

impl TableView {
    /// Data rows of a loaded side: the records, less the header record when the header is on.
    fn data_rows(&self, info: &SideInfo) -> usize {
        info.rows
            .saturating_sub(usize::from(self.prefs.header && info.rows > 0))
    }

    /// "1,200 × 8": data rows by columns.
    pub(super) fn size_text(&self, info: &SideInfo) -> String {
        format!(
            "{} × {}",
            format_size(self.data_rows(info) as u64),
            format_size(info.columns as u64)
        )
    }

    /// "a.csv  1,200 × 8" (name, rows × columns), or "No file chosen".
    pub fn name_text(&self, left: bool) -> String {
        match self.side(left) {
            Some(info) => format!("{}  {}", info.name, self.size_text(info)),
            None => "No file chosen".to_owned(),
        }
    }

    /// The name's tooltip: path, size, encoding, delimiter, rows and columns.
    pub fn name_tooltip(&self, left: bool) -> Option<String> {
        let info = self.side(left)?;
        let place = info
            .path
            .as_ref()
            .map_or_else(|| info.name.clone(), |path| path.display().to_string());
        Some(format!(
            "{place}\n{} · {} · Delimiter: {}\n{} rows × {} columns",
            human_size(info.size),
            encoding_label(&info.encoding),
            delimiter_label(info),
            format_size(self.data_rows(info) as u64),
            format_size(info.columns as u64),
        ))
    }

    /// The differing rows the filter shows, as "n / m"; "–" for n when the selection is not one
    /// of them.
    pub fn position_text(&self) -> String {
        let at = self
            .selected_stop()
            .map_or("–".to_owned(), |i| format_size(i as u64 + 1));
        format!("{at} / {}", format_size(self.view.stops.len() as u64))
    }

    /// The index into `view.stops` of the selected row, when it is a stop.
    fn selected_stop(&self) -> Option<usize> {
        let pos = self.view.position(self.selection?.0)?;
        self.view.stops.binary_search(&(pos as u32)).ok()
    }

    /// The status line as coloured parts.
    pub fn status_parts(&self) -> Vec<StatusPart> {
        let mut parts = Vec::new();
        if let Some(diff) = &self.diff {
            self.result_parts(diff, &mut parts);
        } else if !self.is_busy() && self.messages.iter().any(Option::is_some) {
            // A side failed to open: there is nothing to compare yet.
            parts.push(StatusPart::new(OPEN_OTHER_SIDE, Tone::Changed));
        }
        for left in [true, false] {
            if let Some(info) = self.side(left) {
                parts.push(StatusPart::new(
                    format!(
                        "{}: {} · {}",
                        if left { "Left" } else { "Right" },
                        encoding_label(&info.encoding),
                        delimiter_label(info)
                    ),
                    Tone::Muted,
                ));
            }
        }
        parts
    }

    fn result_parts(&self, diff: &TableDiff, parts: &mut Vec<StatusPart>) {
        let counts = &diff.counts;
        let n = |count: usize| format_size(count as u64);
        if !diff.both {
            parts.push(StatusPart::new(OPEN_OTHER_SIDE, Tone::Changed));
        } else if diff.is_identical() {
            parts.push(StatusPart::new(
                "The tables are identical cell for cell.",
                Tone::Text,
            ));
        }
        parts.extend([
            StatusPart::new(format!("Same {}", n(counts.same)), Tone::Muted),
            StatusPart::new(format!("Differ {}", n(counts.differs)), Tone::Changed),
            StatusPart::new(format!("Left only {}", n(counts.left_only)), Tone::LeftOnly),
            StatusPart::new(
                format!("Right only {}", n(counts.right_only)),
                Tone::RightOnly,
            ),
            StatusPart::new(format!("Cells differ {}", n(counts.cells)), Tone::Muted),
        ]);
        if !diff.both {
            return;
        }
        let mut columns = format!("Columns matched {}", n(diff.matched.len()));
        if diff.left_only_columns > 0 {
            columns.push_str(&format!(" · left only {}", n(diff.left_only_columns)));
        }
        if diff.right_only_columns > 0 {
            columns.push_str(&format!(" · right only {}", n(diff.right_only_columns)));
        }
        parts.push(StatusPart::new(columns, Tone::Muted));
        if diff.keys.is_empty() {
            parts.push(StatusPart::new("Aligned by row order", Tone::Muted));
        } else {
            let names: Vec<&str> = diff
                .keys
                .iter()
                .map(|&key| diff.columns[key].name.as_str())
                .collect();
            let mut keyed = format!("Paired by key: {}", names.join(", "));
            if diff.duplicates > 0 {
                keyed.push_str(&format!(" · duplicate keys {}", n(diff.duplicates)));
            }
            parts.push(StatusPart::new(keyed, Tone::Muted));
        }
        if !diff.bad_keys.is_empty() {
            parts.push(StatusPart::new(
                format!("Key column not found: {}", diff.bad_keys.join(", ")),
                Tone::LeftOnly,
            ));
        }
    }

    /// Test hook: what a progress tick shows while a side parses (a test parses too fast for
    /// a tick to see it).
    #[doc(hidden)]
    pub fn show_progress_for_test(&mut self, text: &str) {
        self.progress_text = Some(text.to_owned());
    }

    pub fn status_text(&self) -> String {
        crate::status::join_parts(&self.status_parts())
    }

    /// The status bar: the parts, then the notice (parse progress, a jump's refusal, a key
    /// warning).
    pub(super) fn render_status(&self, p: &Palette) -> impl IntoElement {
        let parts = self.status_parts();
        let notice = self.notice().map(|notice| {
            div()
                .id("table-notice")
                .test_support()
                .flex_none()
                .text_color(p.text)
                .child(notice.to_owned())
        });
        status_bar("table-status", p)
            .child(if parts.is_empty() {
                status_line_box("")
            } else {
                status_parts_row(&parts, p)
            })
            .children(notice)
    }
}
