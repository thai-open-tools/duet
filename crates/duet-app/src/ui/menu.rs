//! Right-click menus as plain data a test can read, and the one place that turns them into a
//! gpui-kit popup (spec P4). Items that run a view action show its shortcut.

use std::rc::Rc;

use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::{Action, App, FocusHandle, SharedString, Window};

type Run = Rc<dyn Fn(&mut Window, &mut App)>;

pub enum MenuEntry {
    Action {
        label: SharedString,
        action: Box<dyn Action>,
        enabled: bool,
        /// None: no check column. Some: a check column, ticked when true (R19).
        checked: Option<bool>,
    },
    Click {
        label: SharedString,
        run: Run,
        enabled: bool,
    },
    Separator,
}

impl MenuEntry {
    pub fn action(label: impl Into<SharedString>, action: impl Action) -> Self {
        Self::Action {
            label: label.into(),
            action: Box::new(action),
            enabled: true,
            checked: None,
        }
    }

    pub fn click(
        label: impl Into<SharedString>,
        run: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self::Click {
            label: label.into(),
            run: Rc::new(run),
            enabled: true,
        }
    }

    /// Shows a check column on an action item, ticked when `on` (LocalDiff's "● " mode marks).
    pub fn checked(mut self, on: bool) -> Self {
        if let Self::Action { checked, .. } = &mut self {
            *checked = Some(on);
        }
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        match &mut self {
            Self::Action { enabled, .. } | Self::Click { enabled, .. } => *enabled = on,
            Self::Separator => {}
        }
        self
    }
}

/// No separator at either end or twice in a row; a menu of separators only is empty.
pub fn tidy(entries: Vec<MenuEntry>) -> Vec<MenuEntry> {
    let mut out: Vec<MenuEntry> = Vec::with_capacity(entries.len());
    for entry in entries {
        let separator = matches!(entry, MenuEntry::Separator);
        if separator && out.last().is_none_or(|e| matches!(e, MenuEntry::Separator)) {
            continue;
        }
        out.push(entry);
    }
    if out
        .last()
        .is_some_and(|e| matches!(e, MenuEntry::Separator))
    {
        out.pop();
    }
    out
}

fn marked(label: &str, enabled: bool, checked: bool) -> String {
    let label = if checked {
        format!("● {label}")
    } else {
        label.to_owned()
    };
    if enabled { label } else { format!("({label})") }
}

/// What a menu shows, for tests: labels, "-" for a separator, a disabled item in brackets.
pub fn outline(entries: &[MenuEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|e| match e {
            MenuEntry::Separator => "-".to_owned(),
            MenuEntry::Action {
                label,
                enabled,
                checked,
                ..
            } => marked(label, *enabled, *checked == Some(true)),
            MenuEntry::Click { label, enabled, .. } => marked(label, *enabled, false),
        })
        .collect()
}

/// Fills a popup. Action items go to `focus` (the view's handle), which is also where the
/// popup looks up the shortcut it shows.
pub fn fill(mut menu: PopupMenu, entries: Vec<MenuEntry>, focus: Option<FocusHandle>) -> PopupMenu {
    if let Some(focus) = focus {
        menu = menu.action_context(focus);
    }
    for entry in tidy(entries) {
        menu = match entry {
            MenuEntry::Separator => menu.separator(),
            MenuEntry::Action {
                label,
                action,
                enabled,
                checked,
            } => match checked {
                Some(on) => menu.menu_with_check_and_disabled(label, on, action, !enabled),
                None => menu.menu_with_disabled(label, action, !enabled),
            },
            MenuEntry::Click {
                label,
                run,
                enabled,
            } => menu.item(
                PopupMenuItem::new(label)
                    .disabled(!enabled)
                    .on_click(move |_, window, cx| run(window, cx)),
            ),
        };
    }
    menu
}

/// `fill` for a right-click menu over a view. gpui-kit gives the keys to the popup it builds,
/// even an empty one it never draws, so the view's keys went nowhere until the next click
/// (T-62): an empty menu hands them straight back to `focus`.
pub fn fill_or_hand_back(
    menu: PopupMenu,
    entries: Vec<MenuEntry>,
    focus: FocusHandle,
    window: &mut Window,
    cx: &mut App,
) -> PopupMenu {
    let entries = tidy(entries);
    if entries.is_empty() {
        let back = focus.clone();
        // The popup takes the keys once this builder returns; this runs after that.
        window.defer(cx, move |window, cx| back.focus(window, cx));
    }
    fill(menu, entries, Some(focus))
}

/// Runs the item called `label` as the popup would: a click item runs its closure, an action
/// item focuses `focus` and dispatches. False when the label is missing or the item disabled.
pub fn activate(
    entries: Vec<MenuEntry>,
    label: &str,
    focus: Option<&FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    for entry in tidy(entries) {
        match entry {
            MenuEntry::Click {
                label: l,
                run,
                enabled,
            } if l.as_ref() == label => {
                if enabled {
                    run(window, cx);
                }
                return enabled;
            }
            MenuEntry::Action {
                label: l,
                action,
                enabled,
                ..
            } if l.as_ref() == label => {
                if enabled {
                    if let Some(focus) = focus {
                        focus.focus(window, cx);
                    }
                    window.dispatch_action(action, cx);
                }
                return enabled;
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    gpui_kit::actions!(menu_test, [Ping]);

    #[test]
    fn tidy_drops_separators_at_the_ends_and_doubles() {
        let entries = vec![
            MenuEntry::Separator,
            MenuEntry::action("Open", Ping),
            MenuEntry::Separator,
            MenuEntry::Separator,
            MenuEntry::action("Close", Ping).enabled(false),
            MenuEntry::Separator,
        ];
        assert_eq!(outline(&tidy(entries)), ["Open", "-", "(Close)"]);
    }

    #[test]
    fn an_empty_menu_stays_empty() {
        assert!(tidy(vec![MenuEntry::Separator, MenuEntry::Separator]).is_empty());
    }

    #[test]
    fn a_checked_item_is_marked_in_the_outline() {
        assert_eq!(
            outline(&[MenuEntry::action("Blink", Ping).checked(true)]),
            ["● Blink"]
        );
        assert_eq!(
            outline(&[MenuEntry::action("Blink", Ping)
                .checked(true)
                .enabled(false)]),
            ["(● Blink)"]
        );
        assert_eq!(
            outline(&[MenuEntry::action("Slider", Ping).checked(false)]),
            ["Slider"]
        );
    }

    #[gpui_kit::test]
    fn activate_refuses_a_disabled_item_and_a_missing_label(cx: &mut gpui_kit::TestAppContext) {
        use std::cell::Cell;
        let ran = Rc::new(Cell::new(0));
        let window = cx.add_empty_window();
        window.update(|window, cx| {
            let make = || {
                let (a, b) = (ran.clone(), ran.clone());
                vec![
                    MenuEntry::click("Go", move |_, _| a.set(a.get() + 1)),
                    MenuEntry::click("Stop", move |_, _| b.set(b.get() + 100)).enabled(false),
                ]
            };
            assert!(!activate(make(), "Stop", None, window, cx));
            assert!(!activate(make(), "Nope", None, window, cx));
            assert_eq!(ran.get(), 0);
            assert!(activate(make(), "Go", None, window, cx));
            assert_eq!(ran.get(), 1);
        });
    }
}
