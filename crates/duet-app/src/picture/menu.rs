//! The right-click menu of the picture body (A3, `js:714-731`) as plain data a test can read.
//! The mode items show the current mode with a check (R19); the others wait for what they act
//! on, the way the toolbar buttons do.

use crate::picture::prefs::Mode;
use crate::picture::view::{
    PictureActualSize, PictureBlink, PictureDifference, PictureFit, PictureNext, PicturePrev,
    PictureReload, PictureSideBySide, PictureSlider, PictureSwap, PictureView,
};
use crate::ui::menu::MenuEntry;

gpui_kit::actions!(picture_menu, [PictureExport]);

pub fn context_menu(view: &PictureView) -> Vec<MenuEntry> {
    let mode = view.mode();
    let item = |label: &'static str, this: Mode| {
        match this {
            Mode::SideBySide => MenuEntry::action(label, PictureSideBySide),
            Mode::Difference => MenuEntry::action(label, PictureDifference),
            Mode::Slider => MenuEntry::action(label, PictureSlider),
            Mode::Blink => MenuEntry::action(label, PictureBlink),
        }
        .checked(mode == this)
    };
    let walk = view.can_navigate();
    let mut entries = vec![
        MenuEntry::action("Fit to window", PictureFit),
        MenuEntry::action("Actual size (1:1)", PictureActualSize),
        MenuEntry::Separator,
    ];
    entries.extend(Mode::ALL.map(|m| item(m.label(), m)));
    entries.extend([
        MenuEntry::Separator,
        MenuEntry::action("Next difference", PictureNext).enabled(walk),
        MenuEntry::action("Previous difference", PicturePrev).enabled(walk),
        MenuEntry::Separator,
        MenuEntry::action("Swap left and right", PictureSwap).enabled(view.can_swap()),
        MenuEntry::action("Reload", PictureReload).enabled(view.can_reload()),
        MenuEntry::action("Export difference image (PNG)…", PictureExport)
            .enabled(view.can_export()),
    ]);
    entries
}
