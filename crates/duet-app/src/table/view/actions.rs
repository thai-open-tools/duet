//! What the table view's menus do (A4): copy a cell or a row, set the key columns from a
//! column, and the handlers that bind those items, the open keys and the file drop to the view.

use duet_core::table::compare::{Column, data_offset};
use duet_core::table::nav::side_value;
use duet_core::table::report::record_csv;
use gpui_kit::{ClipboardItem, ExternalPaths, MouseButton, MouseDownEvent};

use super::*;
use crate::table::menu::{
    TableAddKey, TableClearKeys, TableCopyLeftRow, TableCopyLeftValue, TableCopyRightRow,
    TableCopyRightValue, TableExportCsv, TableExportHtml, TableUseKey,
};

/// Where the last right press landed, which decides the menu the popup shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum MenuPress {
    /// Neither a row nor a column name: no menu (LocalDiff `ctxMenu`, the folder view).
    #[default]
    Nothing,
    Row,
    /// The name of this column, on either half.
    Head(usize),
}

/// The menu's column, as the menu words it.
pub struct MenuColumn {
    pub name: String,
    pub on_both_sides: bool,
    pub is_key: bool,
}

/// What the selection (or a column name) allows, for a menu to turn into items.
#[derive(Default)]
pub struct MenuTarget {
    pub column: Option<MenuColumn>,
    pub left_value: bool,
    pub right_value: bool,
    pub left_row: bool,
    pub right_row: bool,
    /// The comparison pairs rows by key columns.
    pub keys_set: bool,
    /// The key columns box holds something.
    pub keys_text: bool,
}

/// The text that names `columns[c]` in the key box (LocalDiff `keyToken`): its left name, or
/// `#n` (its 1-based position in the left file) when the name could not be told apart: no
/// header, a comma in it, a number, or another column with the same name ignoring case.
pub fn key_token(columns: &[Column], header: bool, c: usize) -> String {
    let column = &columns[c];
    let name = column.left_name.as_str();
    let number = format!("#{}", column.left.unwrap_or(c) + 1);
    let digits = name.strip_prefix('#').unwrap_or(name);
    let numeric = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    if !header || name.contains(',') || numeric {
        return number;
    }
    let lower = name.to_lowercase();
    let shared = columns.iter().enumerate().any(|(i, other)| {
        i != c
            && (other.left_name.to_lowercase() == lower || other.right_name.to_lowercase() == lower)
    });
    if shared { number } else { name.to_owned() }
}

impl TableView {
    /// What the selected cell offers the row menu; None unless the last right press hit a row.
    pub fn menu_target(&self) -> Option<MenuTarget> {
        if self.menu_press != MenuPress::Row {
            return None;
        }
        let diff = self.diff.as_ref()?;
        let column = self.selection.and_then(|(_, column)| column);
        let mut target = self.keys_target(diff, column);
        let Some((row, paired)) = self
            .selection
            .and_then(|(row, _)| Some((row, diff.rows.get(row)?)))
        else {
            return Some(target);
        };
        let value = |left: bool| {
            column
                .and_then(|c| {
                    let table = self.compared[side_index(left)].as_deref();
                    side_value(diff, table, self.compared_options.header, row, c, left)
                })
                .is_some()
        };
        target.left_value = value(true);
        target.right_value = value(false);
        target.left_row = paired.left.is_some();
        target.right_row = paired.right.is_some();
        Some(target)
    }

    /// What the column name under the last right press offers its menu, if it was on one.
    pub fn head_target(&self) -> Option<MenuTarget> {
        let MenuPress::Head(column) = self.menu_press else {
            return None;
        };
        let diff = self.diff.as_ref()?;
        diff.columns.get(column)?;
        Some(self.keys_target(diff, Some(column)))
    }

    /// The key items' part of a target: `column` and the key columns, whatever is selected.
    fn keys_target(&self, diff: &TableDiff, column: Option<usize>) -> MenuTarget {
        MenuTarget {
            column: column
                .and_then(|c| diff.columns.get(c).map(|col| (c, col)))
                .map(|(c, col)| MenuColumn {
                    name: col.name.clone(),
                    on_both_sides: col.left.is_some() && col.right.is_some(),
                    is_key: diff.keys.contains(&c),
                }),
            keys_set: !diff.keys.is_empty(),
            keys_text: !self.prefs.keys.trim().is_empty(),
            ..MenuTarget::default()
        }
    }

    fn copy_text(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.notice = Some("Copied.".into());
        cx.notify();
    }

    /// The selected cell's raw value on one side.
    fn copy_value(&mut self, left: bool, cx: &mut Context<Self>) {
        let (Some(diff), Some((row, Some(column)))) = (&self.diff, self.selection) else {
            return;
        };
        let table = self.compared[side_index(left)].as_deref();
        let Some(value) = side_value(diff, table, self.compared_options.header, row, column, left)
        else {
            return;
        };
        let value = value.to_owned();
        self.copy_text(value, cx);
    }

    /// The selected row on one side as a comma-separated line, whatever the file's delimiter
    /// (LocalDiff `recCsv(…, ',')`).
    fn copy_row(&mut self, left: bool, cx: &mut Context<Self>) {
        let (Some(diff), Some((row, _))) = (&self.diff, self.selection) else {
            return;
        };
        let paired = diff.rows.get(row);
        let data = paired.and_then(|paired| if left { paired.left } else { paired.right });
        let (Some(data), Some(table)) = (data, self.compared[side_index(left)].as_deref()) else {
            return;
        };
        let record = table.record(data as usize + data_offset(table, self.compared_options.header));
        let text = record_csv(record, ',');
        self.copy_text(text, cx);
    }

    /// Makes `keys` the key columns: the box shows them at once and the rows pair again.
    fn set_keys(&mut self, keys: String, window: &mut Window, cx: &mut Context<Self>) {
        self.keys_task = None;
        self.keys_shown.clone_from(&keys);
        let shown = keys.clone();
        self.keys_input
            .update(cx, |state, cx| state.set_value(shown, window, cx));
        self.set_prefs(|prefs| prefs.keys = keys, cx);
    }

    /// "Use … as the key column" (`add` false) or "Add … to the key columns" for the menu's
    /// column (the name pressed, else the selected cell's), if both sides have it.
    fn key_from_menu(&mut self, add: bool, window: &mut Window, cx: &mut Context<Self>) {
        let column = match self.menu_press {
            MenuPress::Head(c) => Some(c),
            MenuPress::Row => self.selection.and_then(|(_, c)| c),
            MenuPress::Nothing => None,
        };
        let (Some(diff), Some(c)) = (&self.diff, column) else {
            return;
        };
        let both = diff
            .columns
            .get(c)
            .is_some_and(|col| col.left.is_some() && col.right.is_some());
        if !both || (add && (diff.keys.is_empty() || diff.keys.contains(&c))) {
            return;
        }
        let token = key_token(&diff.columns, self.compared_options.header, c);
        let current = self.prefs.keys.trim();
        let keys = if add && !current.is_empty() {
            format!("{current}, {token}")
        } else {
            token
        };
        self.set_keys(keys, window, cx);
    }

    /// The right-click menu's items, the open keys and the file drop, bound to the view.
    pub(super) fn bind_actions<E: InteractiveElement>(element: E, cx: &mut Context<Self>) -> E {
        element
            .on_action(cx.listener(|this, _: &TableCopyLeftValue, w, cx| {
                if this.takes_key(w, cx) {
                    this.copy_value(true, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TableCopyRightValue, w, cx| {
                if this.takes_key(w, cx) {
                    this.copy_value(false, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TableCopyLeftRow, w, cx| {
                if this.takes_key(w, cx) {
                    this.copy_row(true, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TableCopyRightRow, w, cx| {
                if this.takes_key(w, cx) {
                    this.copy_row(false, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TableUseKey, w, cx| {
                if this.takes_key(w, cx) {
                    this.key_from_menu(false, w, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TableAddKey, w, cx| {
                if this.takes_key(w, cx) {
                    this.key_from_menu(true, w, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TableClearKeys, w, cx| {
                if this.takes_key(w, cx) {
                    this.set_keys(String::new(), w, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TableExportCsv, w, cx| {
                if this.takes_key(w, cx) {
                    this.export(false, w, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TableExportHtml, w, cx| {
                if this.takes_key(w, cx) {
                    this.export(true, w, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TableOpenLeft, w, cx| {
                if this.takes_key(w, cx) {
                    this.pick_file(true, w, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &TableOpenRight, w, cx| {
                if this.takes_key(w, cx) {
                    this.pick_file(false, w, cx)
                }
            }))
            .on_drop(
                cx.listener(|this, paths: &ExternalPaths, w, cx| {
                    this.on_files_dropped(paths, w, cx)
                }),
            )
    }

    /// A right press takes the cell under it (the same as a left press) before the popup is
    /// built, so the menu is about that cell. Below the last row there is no menu, and the
    /// selection stays.
    pub(super) fn press_right(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button == MouseButton::Right {
            self.menu_press = match self.hit(event.position) {
                Some(_) => MenuPress::Row,
                None => MenuPress::Nothing,
            };
            self.press_rows(event, window, cx);
        }
    }

    /// A right press on the column names remembers the column for its menu; the selection
    /// stays (LocalDiff `ctxMenu` on a header cell).
    pub(super) fn press_head(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button == MouseButton::Right {
            self.menu_press = match self.head_column(event.position) {
                Some(column) => MenuPress::Head(column),
                None => MenuPress::Nothing,
            };
            window.focus(&self.focus, cx);
        }
    }
}
