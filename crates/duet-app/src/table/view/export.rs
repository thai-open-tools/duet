//! Export ▾ (R13): the differences as a CSV or the whole comparison as an HTML report. The
//! file is built and written off the UI thread, after the save dialog.

use super::status::delimiter_label;
use super::*;
use crate::format::encoding_label;
use crate::paths::home_dir;
use duet_core::table::report::{ReportSide, differences_csv, export_name, html_report};

impl TableView {
    /// Both sides are loaded and compared, nothing is running, and the result shown was made
    /// from the tables and options in use: after Esc stops a swap or an option change, the old
    /// result stays on screen but no longer matches the names and options it would be saved
    /// with.
    pub fn can_export(&self) -> bool {
        let current = self
            .tables
            .iter()
            .zip(&self.compared)
            .all(|pair| match pair {
                (Some(table), Some(compared)) => Arc::ptr_eq(table, compared),
                _ => false,
            });
        !self.is_busy()
            && self.diff.as_ref().is_some_and(|diff| diff.both)
            && current
            && self.compared_options == self.prefs.compare_options()
    }

    /// "{path or name} ({encoding}, {delimiter})", as the status line words the last two.
    fn describe_side(&self, left: bool) -> Option<String> {
        let info = self.side(left)?;
        let place = info
            .path
            .as_ref()
            .map_or_else(|| info.name.clone(), |path| path.display().to_string());
        Some(format!(
            "{place} ({}, {})",
            encoding_label(&info.encoding),
            delimiter_label(info)
        ))
    }

    /// Asks where to save, then writes the CSV or the HTML report there.
    pub(super) fn export(&mut self, html: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_export() {
            return;
        }
        let (Some(diff), [Some(left), Some(right)]) = (self.diff.clone(), self.compared.clone())
        else {
            return;
        };
        let (Some(left_info), Some(right_info)) = (self.side(true), self.side(false)) else {
            return;
        };
        let names = [left_info.name.clone(), right_info.name.clone()];
        let describe = [self.describe_side(true), self.describe_side(false)];
        let options = self.compared_options.clone();
        let file = export_name(&names[LEFT], &names[RIGHT], html);
        let dir = left_info
            .path
            .as_deref()
            .and_then(Path::parent)
            .filter(|dir| !dir.as_os_str().is_empty())
            .map_or_else(home_dir, Path::to_path_buf);
        let picked = cx.prompt_for_new_path(&dir, Some(&file));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let target = path.clone();
            let written = cx.background_executor().spawn(async move {
                let text = if html {
                    let sides = [
                        ReportSide {
                            table: &left,
                            name: &names[LEFT],
                            describe: describe[LEFT].clone().unwrap_or_default(),
                        },
                        ReportSide {
                            table: &right,
                            name: &names[RIGHT],
                            describe: describe[RIGHT].clone().unwrap_or_default(),
                        },
                    ];
                    let generated = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
                    html_report(&diff, &sides[LEFT], &sides[RIGHT], &options, &generated)
                } else {
                    differences_csv(&diff, &left, &right, options.header)
                };
                std::fs::write(&target, text)
            });
            let result = written.await;
            let _ = this.update(cx, |view, cx| {
                view.notice = Some(
                    match result {
                        Ok(()) => format!("Exported: {}.", path.display()),
                        Err(err) => format!("Can't export: {err}."),
                    }
                    .into(),
                );
                cx.notify();
            });
        })
        .detach();
    }
}
