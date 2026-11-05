//! The table view's names, navigation position and status line.

use super::*;
use crate::format::{encoding_label, format_size, human_size};
use crate::i18n::{tr, tr_key, trf};
use crate::status::{StatusPart, Tone};
use crate::ui::style::{status_bar, status_line_box, status_notice, status_parts_row};

const OPEN_OTHER_SIDE: &str = tr_key("Open the other side to start comparing.");

/// "comma (,)", with " (auto)" when it was detected.
pub(super) fn delimiter_label(info: &SideInfo) -> String {
    let label = tr(info.delimiter.label()).to_lowercase();
    if info.auto {
        trf("{} (auto)", &[&label])
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
            None => self.unread_name(left),
        }
    }

    /// A side with nothing loaded: the file it was given (a refused file, or one a cancelled
    /// open never finished reading), so the label names what the tab does; "No file chosen"
    /// when it has none.
    pub(super) fn unread_name(&self, left: bool) -> String {
        let index = side_index(left);
        if self.pasted[index] || self.paths[index].as_os_str().is_empty() {
            tr("No file chosen").to_owned()
        } else {
            file_name(&self.paths[index])
        }
    }

    /// The files that could not be read, named for the status line.
    fn refused_names(&self) -> String {
        [true, false]
            .into_iter()
            .filter(|&left| self.messages[side_index(left)].is_some())
            .map(|left| self.unread_name(left))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The name's tooltip: path, size, encoding, delimiter, rows and columns.
    pub fn name_tooltip(&self, left: bool) -> Option<String> {
        let info = self.side(left)?;
        let place = info
            .path
            .as_ref()
            .map_or_else(|| info.name.clone(), |path| path.display().to_string());
        let how = match &info.book {
            Some(book) => trf(
                "{} · sheet “{}” ({} of {})",
                &[
                    &human_size(info.size),
                    &book.sheets[book.shown].name,
                    &(book.shown + 1),
                    &book.sheets.len(),
                ],
            ),
            None => trf(
                "{} · {} · Delimiter: {}",
                &[
                    &human_size(info.size),
                    &encoding_label(&info.encoding),
                    &delimiter_label(info),
                ],
            ),
        };
        Some(format!(
            "{place}\n{how}\n{}",
            trf(
                "{} rows × {} columns",
                &[
                    &format_size(self.data_rows(info) as u64),
                    &format_size(info.columns as u64),
                ],
            ),
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
            let next = if self.another_sheet_to_try() {
                tr("Choose another sheet.").to_owned()
            } else {
                trf(
                    "{} could not be read. Open another file to start comparing.",
                    &[&self.refused_names()],
                )
            };
            parts.push(StatusPart::new(next, Tone::Changed));
        }
        for left in [true, false] {
            if let Some(info) = self.side(left) {
                let side = tr(if left { "Left" } else { "Right" });
                let text = match self.sheet_label(left) {
                    Some(sheet) => trf("{}: {}", &[&side, &sheet]),
                    None => trf(
                        "{}: {} · {}",
                        &[
                            &side,
                            &encoding_label(&info.encoding),
                            &delimiter_label(info),
                        ],
                    ),
                };
                parts.push(StatusPart::new(text, Tone::Muted));
            }
        }
        parts
    }

    fn result_parts(&self, diff: &TableDiff, parts: &mut Vec<StatusPart>) {
        let counts = &diff.counts;
        let n = |count: usize| format_size(count as u64);
        if !diff.both {
            parts.push(StatusPart::new(tr(OPEN_OTHER_SIDE), Tone::Changed));
        } else if diff.is_identical() {
            parts.push(StatusPart::new(self.identical_text(), Tone::Text));
        }
        // What was left out comes first when it qualifies "identical", last otherwise.
        let unchecked: Vec<StatusPart> = diff
            .both
            .then(|| self.unchecked_notes())
            .into_iter()
            .flatten()
            .map(|note| StatusPart::new(note, Tone::Changed))
            .collect();
        let (first, last) = if diff.is_identical() {
            (unchecked, Vec::new())
        } else {
            (Vec::new(), unchecked)
        };
        parts.extend(first);
        parts.extend([
            StatusPart::new(trf("Same {}", &[&n(counts.same)]), Tone::Muted),
            StatusPart::new(trf("Differ {}", &[&n(counts.differs)]), Tone::Changed),
            StatusPart::new(trf("Left only {}", &[&n(counts.left_only)]), Tone::LeftOnly),
            StatusPart::new(
                trf("Right only {}", &[&n(counts.right_only)]),
                Tone::RightOnly,
            ),
            StatusPart::new(trf("Cells differ {}", &[&n(counts.cells)]), Tone::Muted),
        ]);
        if !diff.both {
            return;
        }
        let mut columns = trf("Columns matched {}", &[&n(diff.matched.len())]);
        if diff.left_only_columns > 0 {
            columns.push_str(&trf(" · left only {}", &[&n(diff.left_only_columns)]));
        }
        if diff.right_only_columns > 0 {
            columns.push_str(&trf(" · right only {}", &[&n(diff.right_only_columns)]));
        }
        parts.push(StatusPart::new(columns, Tone::Muted));
        if diff.keys.is_empty() {
            parts.push(StatusPart::new(tr("Aligned by row order"), Tone::Muted));
        } else {
            let names: Vec<&str> = diff
                .keys
                .iter()
                .map(|&key| diff.columns[key].name.as_str())
                .collect();
            let mut keyed = trf("Paired by key: {}", &[&names.join(", ")]);
            if diff.duplicates > 0 {
                keyed.push_str(&trf(" · duplicate keys {}", &[&n(diff.duplicates)]));
            }
            parts.push(StatusPart::new(keyed, Tone::Muted));
        }
        if !diff.bad_keys.is_empty() {
            parts.push(StatusPart::new(
                trf("Key column not found: {}", &[&diff.bad_keys.join(", ")]),
                Tone::LeftOnly,
            ));
        }
        parts.extend(last);
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
        let notice = self
            .notice()
            .map(|notice| status_notice("table-notice", notice, p));
        status_bar("table-status", p)
            .child(if parts.is_empty() {
                status_line_box("table-status-text", "", "")
            } else {
                status_parts_row("table-status-text", &parts, p)
            })
            .children(notice)
    }
}
