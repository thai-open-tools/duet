//! The options row's choices: the criterion, the time tolerance, the DST rule, the format
//! toggle and the Options menu. Each setter changes the compare, remembers the choice in
//! `settings.json` (R3) and compares again only when the choice can change the result.

use duet_core::folder::classify::reapply_format_rule;
use duet_core::folder::run::CompareMode;
use gpui_kit::component::input::InputState;
use gpui_kit::{Context, Entity, actions};

use super::*;
use crate::folder::prefs::MAX_TOLERANCE_SECS;
use crate::i18n::tr;
use crate::ui::menu::MenuEntry;

actions!(
    folder,
    [
        CriterionContent,
        CriterionQuick,
        CriterionSize,
        CriterionRules,
        ToggleOddScan,
        ToggleConfirmCopy,
        ToggleEnterFolders,
        ToggleFindMoves
    ]
);

/// The criterion button's menu, the current criterion checked.
pub fn criterion_menu(view: &FolderView) -> Vec<MenuEntry> {
    let mode = view.options().mode;
    vec![
        MenuEntry::action(tr("Content"), CriterionContent).checked(mode == CompareMode::Content),
        MenuEntry::action(
            tr("Quick: size and modified time (reads no files)"),
            CriterionQuick,
        )
        .checked(mode == CompareMode::Quick),
        MenuEntry::action(tr("Size only"), CriterionSize).checked(mode == CompareMode::Size),
        MenuEntry::action(tr("Content by the text compare rules"), CriterionRules)
            .checked(mode == CompareMode::Rules),
    ]
}

/// The Options menu: the settings of the folder view.
pub fn options_menu(view: &FolderView) -> Vec<MenuEntry> {
    let prefs = view.prefs();
    vec![
        MenuEntry::action(
            tr("Look for invisible characters after a compare"),
            ToggleOddScan,
        )
        .checked(prefs.odd_scan),
        MenuEntry::action(tr("Ask before copying"), ToggleConfirmCopy).checked(prefs.confirm_copy),
        MenuEntry::action(
            tr("Double-click a folder to compare inside it"),
            ToggleEnterFolders,
        )
        .checked(prefs.enter_folders),
        MenuEntry::action(tr("Find moved files"), ToggleFindMoves).checked(prefs.find_moves),
    ]
}

/// The tolerance field's text as whole seconds: rounded down, 0 to 86,400; text that is not a
/// number is 0 (LocalDiff `+v || 0`).
pub(super) fn parse_tolerance(text: &str) -> u64 {
    let number: f64 = text.trim().parse().unwrap_or(0.);
    if number.is_nan() {
        return 0;
    }
    number.floor().clamp(0., MAX_TOLERANCE_SECS as f64) as u64
}

impl FolderView {
    /// The criterion button's label; the button draws its caret.
    pub fn mode_button_label(&self) -> &'static str {
        match self.options.mode {
            CompareMode::Quick => tr("Quick (size + time)"),
            CompareMode::Content => tr("Content"),
            CompareMode::Size => tr("Size only"),
            CompareMode::Rules => tr("Text rules"),
        }
    }

    #[doc(hidden)]
    pub fn tolerance_input(&self) -> &Entity<InputState> {
        &self.tolerance
    }

    /// Another criterion: remembered, and the folders are compared again.
    pub fn set_mode(&mut self, mode: CompareMode, cx: &mut Context<Self>) {
        if self.options.mode == mode {
            return;
        }
        self.prefs.mode = mode;
        self.remember(prefs::CRITERION, cx);
        self.update_options(cx, |o| o.mode = mode);
    }

    /// Whole seconds, at most a day; an unchanged value does nothing. Only Quick compares
    /// again, as only Quick pairs files by time; the other criteria use it on their next
    /// compare for "newer".
    pub fn set_tolerance(&mut self, secs: u64, cx: &mut Context<Self>) {
        let secs = secs.min(MAX_TOLERANCE_SECS);
        if self.options.quick.tolerance.as_secs() == secs {
            return;
        }
        self.prefs.tolerance_secs = secs;
        self.remember(prefs::TIME_TOLERANCE, cx);
        self.change_quick_rule(cx, |o| o.quick.tolerance = Duration::from_secs(secs));
    }

    /// "Ignore 1 h DST shift", with the same effect as the tolerance.
    pub fn set_ignore_dst(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.options.quick.ignore_dst == on {
            return;
        }
        self.prefs.ignore_dst = on;
        self.remember(prefs::IGNORE_DST, cx);
        self.change_quick_rule(cx, |o| o.quick.ignore_dst = on);
    }

    fn change_quick_rule(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut FolderOptions),
    ) {
        if self.options.mode == CompareMode::Quick {
            self.update_options(cx, change);
        } else {
            change(&mut self.options);
            cx.notify();
        }
    }

    /// R4: counting line-ending and BOM differences re-derives the shown verdicts without
    /// reading a file; while a compare runs it restarts the compare instead.
    pub fn set_format_important(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.options.format_important == on {
            return;
        }
        self.prefs.format_important = on;
        self.remember(prefs::FORMAT_IMPORTANT, cx);
        self.options.format_important = on;
        if self.job.is_running() {
            self.refresh(cx);
        } else {
            self.rebuild_for_format(on, cx);
        }
    }

    /// The same result with the format rule applied again; the view state carries over.
    fn rebuild_for_format(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(old) = self.model.take() {
            let mut cmp = old.comparison().clone();
            reapply_format_rule(&mut cmp, on);
            let mut model = FolderModel::new(cmp);
            model.carry_over(&old);
            self.plan_epoch += 1;
            self.close_sync_for_compare(cx);
            self.set_model_status(&model, true);
            self.model = Some(model);
        }
        cx.notify();
    }

    pub fn set_odd_scan(&mut self, on: bool, cx: &mut Context<Self>) {
        self.prefs.odd_scan = on;
        self.remember(prefs::ODD_SCAN, cx);
        if on && self.model.is_some() && !self.job.is_running() {
            self.start_odd_scan(false, cx);
        } else {
            self.odd.drop_all();
            self.refresh_status();
        }
        cx.notify();
    }

    /// "Find moved files": remembered. Only a criterion that reads contents has a pass to
    /// start or drop; turning it off keeps the result and loses the tags without a read, turning
    /// it on compares again.
    pub fn set_find_moves(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.options.find_moves == on {
            return;
        }
        self.prefs.find_moves = on;
        self.remember(prefs::FIND_MOVES, cx);
        self.options.find_moves = on;
        if !self.options.mode.reads_contents() {
            cx.notify();
        } else if on || self.job.is_running() {
            self.refresh(cx);
        } else {
            self.rebuild_without_moves(cx);
        }
    }

    fn rebuild_without_moves(&mut self, cx: &mut Context<Self>) {
        if let Some(old) = self.model.take() {
            let mut cmp = old.comparison().clone();
            cmp.moved.clear();
            let mut model = FolderModel::new(cmp);
            model.carry_over(&old);
            self.plan_epoch += 1;
            self.close_sync_for_compare(cx);
            self.moves_stopped = false;
            self.set_model_status(&model, true);
            self.model = Some(model);
        }
        cx.notify();
    }

    pub fn set_confirm_copy(&mut self, on: bool, cx: &mut Context<Self>) {
        self.prefs.confirm_copy = on;
        self.remember(prefs::CONFIRM_COPY, cx);
        cx.notify();
    }

    pub fn set_enter_folders(&mut self, on: bool, cx: &mut Context<Self>) {
        self.prefs.enter_folders = on;
        self.remember(prefs::DOUBLE_CLICK, cx);
        cx.notify();
    }

    /// Enter or blur in the tolerance field: apply the number, then show what was applied.
    pub(super) fn tolerance_entered(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.tolerance.read(cx).value().to_string();
        self.set_tolerance(parse_tolerance(&text), cx);
        let shown = self.options.quick.tolerance.as_secs().to_string();
        if text != shown {
            self.tolerance
                .update(cx, |state, cx| state.set_value(shown, window, cx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_tolerance;

    #[test]
    fn tolerance_text_rounds_down_clamps_and_reads_junk_as_zero() {
        assert_eq!(parse_tolerance("7"), 7);
        assert_eq!(parse_tolerance(" 2.9 "), 2);
        assert_eq!(parse_tolerance("90000"), 86_400);
        assert_eq!(parse_tolerance("-5"), 0);
        assert_eq!(parse_tolerance("abc"), 0);
        assert_eq!(parse_tolerance(""), 0);
        assert_eq!(parse_tolerance("NaN"), 0);
    }
}
