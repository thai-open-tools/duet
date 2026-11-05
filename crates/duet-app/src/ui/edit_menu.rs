//! The right-click menu of a text field, in Thai when the UI is Thai (question 4).
//!
//! The built-in menu comes from gpui-component's own locale table, which has no Thai, so a Thai
//! window would show "Cut", "Copy", "Paste" and "Select All" in English. In Thai a field takes the
//! menu built here; in English it keeps the built-in one, which also greys out what cannot be done
//! now. The builder gets no capabilities, so the Thai menu reads them from the field's state when
//! it opens and greys Cut, Copy and Paste by the same rules.

use gpui_kit::Entity;
use gpui_kit::component::input::{Copy, Cut, Input, InputState, Paste, SelectAll, Textarea};
use gpui_kit::component::native_menu::NativeMenu;

use crate::i18n::{self, Lang, tr};

/// What a field allows right now: the two facts the built-in menu greys its items by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditFacts {
    /// The text can change: the field is neither read-only nor disabled.
    pub editable: bool,
    /// Something is selected and the field is not masked.
    pub copyable: bool,
}

impl EditFacts {
    /// Cut needs text that can change and may be copied out.
    pub fn cut_disabled(self) -> bool {
        !(self.editable && self.copyable)
    }

    pub fn copy_disabled(self) -> bool {
        !self.copyable
    }

    pub fn paste_disabled(self) -> bool {
        !self.editable
    }
}

/// Cut, Copy, Paste and Select all, in the current language, greyed by `facts`.
pub fn thai_edit_menu(menu: NativeMenu, facts: EditFacts) -> NativeMenu {
    menu.menu_with_disabled(tr("Cut"), facts.cut_disabled(), Box::new(Cut))
        .menu_with_disabled(tr("Copy"), facts.copy_disabled(), Box::new(Copy))
        .menu_with_disabled(tr("Paste"), facts.paste_disabled(), Box::new(Paste))
        .separator()
        .menu(tr("Select all"), Box::new(SelectAll))
}

/// `Input::new(state)`, with the Thai menu when the UI is Thai.
pub fn ld_input(state: &Entity<InputState>) -> Input {
    let input = Input::new(state);
    if i18n::current() == Lang::Th {
        let state = state.clone();
        input.context_menu(move |menu, _window, cx| {
            let field = state.read(cx);
            let facts = EditFacts {
                editable: field.is_editable(),
                copyable: field.is_copyable(),
            };
            thai_edit_menu(menu, facts)
        })
    } else {
        input
    }
}

/// `Textarea::new(state)`, with the Thai menu when the UI is Thai.
pub fn ld_textarea(state: &Entity<gpui_kit::component::input::TextareaState>) -> Textarea {
    let area = Textarea::new(state);
    if i18n::current() == Lang::Th {
        let state = state.clone();
        area.context_menu(move |menu, _window, cx| {
            let field = state.read(cx);
            let facts = EditFacts {
                editable: field.is_editable(),
                copyable: field.is_copyable(),
            };
            thai_edit_menu(menu, facts)
        })
    } else {
        area
    }
}

#[cfg(test)]
mod tests {
    use super::EditFacts;

    #[test]
    fn cut_and_copy_are_greyed_with_nothing_selected() {
        let none = EditFacts {
            editable: true,
            copyable: false,
        };
        assert!(none.cut_disabled() && none.copy_disabled());
        assert!(!none.paste_disabled());
    }

    #[test]
    fn a_selection_enables_cut_and_copy() {
        let some = EditFacts {
            editable: true,
            copyable: true,
        };
        assert!(!some.cut_disabled() && !some.copy_disabled());
    }

    #[test]
    fn a_readonly_field_copies_but_does_not_cut_or_paste() {
        let ro = EditFacts {
            editable: false,
            copyable: true,
        };
        assert!(ro.cut_disabled() && ro.paste_disabled() && !ro.copy_disabled());
    }
}
