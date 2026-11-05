//! The right-click menu of a change card (`js:914-930`) as plain data a test can read. A right
//! press on a card's head, columns or result bar makes that card the target and the current
//! change; the items act on the current change, so the first two show their Alt+arrow keys.

use duet_core::text::merge::session::How;

use crate::i18n::tr;
use crate::merge::view::{
    MergeReset, MergeUseBase, MergeUseLeft, MergeUseLeftRight, MergeUseRight, MergeUseRightLeft,
    MergeView,
};
use crate::ui::menu::MenuEntry;

/// The card menu; empty (so no menu opens) when no card was right-pressed.
pub fn card_menu(view: &MergeView) -> Vec<MenuEntry> {
    if view.menu_target().is_none() {
        return Vec::new();
    }
    vec![
        MenuEntry::action(tr(How::Left.label()), MergeUseLeft),
        MenuEntry::action(tr(How::Right.label()), MergeUseRight),
        MenuEntry::action(tr(How::LeftThenRight.label()), MergeUseLeftRight),
        MenuEntry::action(tr(How::RightThenLeft.label()), MergeUseRightLeft),
        MenuEntry::action(tr(How::Base.label()), MergeUseBase),
        MenuEntry::Separator,
        MenuEntry::action(tr("Reset to default"), MergeReset),
    ]
}
