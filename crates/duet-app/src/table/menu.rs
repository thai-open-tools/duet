//! The table view's menus (A4): the right-click row and column-name menus and the Export menu.
//! All are plain data built from the view's state, so a test can read them; their items run
//! the actions below, which the view handles.

use crate::i18n::{tr, trf};
use crate::table::view::{MenuTarget, TableView};
use crate::ui::menu::MenuEntry;

gpui_kit::actions!(
    table_menu,
    [
        TableCopyLeftValue,
        TableCopyRightValue,
        TableCopyLeftRow,
        TableCopyRightRow,
        TableUseKey,
        TableAddKey,
        TableClearKeys,
        TableExportCsv,
        TableExportHtml,
        TableReportLangThai,
        TableReportLangEnglish
    ]
);

/// The menu of the selected cell: what to copy from each side, and the key columns (LocalDiff
/// `ctxMenu`). Empty when the right press hit no row, so blank space opens no menu.
pub fn row_menu(view: &TableView) -> Vec<MenuEntry> {
    let Some(target) = view.menu_target() else {
        return Vec::new();
    };
    let mut entries = vec![
        MenuEntry::action(tr("Copy left value"), TableCopyLeftValue).enabled(target.left_value),
        MenuEntry::action(tr("Copy right value"), TableCopyRightValue).enabled(target.right_value),
        MenuEntry::action(tr("Copy left row (CSV)"), TableCopyLeftRow).enabled(target.left_row),
        MenuEntry::action(tr("Copy right row (CSV)"), TableCopyRightRow).enabled(target.right_row),
        MenuEntry::Separator,
    ];
    entries.extend(key_items(target));
    entries
}

/// The menu of a column name: the key items alone (LocalDiff `ctxMenu` on a header cell).
/// Empty when the right press hit no name.
pub fn head_menu(view: &TableView) -> Vec<MenuEntry> {
    view.head_target().map(key_items).unwrap_or_default()
}

/// "Use …" and "Add …" for a column on both sides (a one-sided column gets neither), then
/// "Clear key columns", enabled whenever the key box holds something.
fn key_items(target: MenuTarget) -> Vec<MenuEntry> {
    let mut entries = Vec::new();
    if let Some(column) = target.column.filter(|column| column.on_both_sides) {
        entries.push(MenuEntry::action(
            trf("Use “{}” as the key column", &[&column.name]),
            TableUseKey,
        ));
        entries.push(
            MenuEntry::action(
                trf("Add “{}” to the key columns", &[&column.name]),
                TableAddKey,
            )
            .enabled(target.keys_set && !column.is_key),
        );
    }
    entries.push(
        MenuEntry::action(tr("Clear key columns (align by row order)"), TableClearKeys)
            .enabled(target.keys_text),
    );
    entries
}

/// The language items that end the `Export ▾` menu: Thai and English, the report's language
/// checked. They are apart from `export_menu`, whose items wait for a finished comparison.
pub fn export_language_menu(view: &TableView) -> Vec<MenuEntry> {
    crate::ui::menu::language_items(
        view.report_lang(),
        TableReportLangThai,
        TableReportLangEnglish,
    )
}

/// The `Export ▾` menu: both items wait for a finished comparison of two loaded sides.
pub fn export_menu(view: &TableView) -> Vec<MenuEntry> {
    let ready = view.can_export();
    vec![
        MenuEntry::action(tr("Differences (CSV)"), TableExportCsv).enabled(ready),
        MenuEntry::action(tr("HTML report"), TableExportHtml).enabled(ready),
    ]
}
