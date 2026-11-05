//! The right-click menu on the hex rows (A2): what it offers for the current selection and
//! differences. The items run the view's own actions, so they do what the keys do.

use crate::hex::view::{HexCopy, HexCopyOffset, HexFind, HexGoTo, HexNext, HexPrev, HexView};
use crate::i18n::tr;
use crate::ui::menu::MenuEntry;

/// The menu of the byte last right-clicked: the copy label follows the selection, and the
/// difference items need a difference to walk to.
pub fn row_menu(view: &HexView) -> Vec<MenuEntry> {
    let diff = view.can_navigate();
    vec![
        MenuEntry::action(
            if view.selection().is_some() {
                tr("Copy bytes (hex)")
            } else {
                tr("Copy this row")
            },
            HexCopy,
        ),
        MenuEntry::action(tr("Copy offset"), HexCopyOffset),
        MenuEntry::Separator,
        MenuEntry::action(tr("Previous difference"), HexPrev).enabled(diff),
        MenuEntry::action(tr("Next difference"), HexNext).enabled(diff),
        MenuEntry::Separator,
        MenuEntry::action(tr("Go to offset…"), HexGoTo),
        MenuEntry::action(tr("Find…"), HexFind),
    ]
}
