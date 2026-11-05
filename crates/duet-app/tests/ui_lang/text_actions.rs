//! The text view's saving, prompts and notices in Thai.

use std::time::Duration;

use duet_app::i18n::{self, answers, tr, trf};
use duet_app::text::saving::{CONFLICT_ANSWERS, ENCODE_ANSWERS, UNSAVED_ANSWERS};
use duet_app::text::view::TextView;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, PromptButton, TestAppContext};

use super::text::{open_text, wait_loaded};
use super::thai_ui;

async fn wait_for_prompt(cx: &mut TestAppContext) {
    for _ in 0..2000 {
        if cx.has_pending_prompt() {
            return;
        }
        cx.executor().timer(Duration::from_millis(10)).await;
    }
    panic!("no prompt appeared");
}

/// A typed "x" on the left side, so the view holds an unsaved edit.
fn type_x(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<TextView>) {
    view.update(cx, |v, cx| v.set_caret_for_test(true, 1, cx));
    cx.update_window(handle, |_, window, cx| window.input("x", cx))
        .unwrap();
}

#[test]
fn every_save_prompt_keeps_cancel_as_the_windows_cancel_button() {
    let _th = thai_ui();
    let kinds = |buttons: &[PromptButton]| {
        buttons
            .iter()
            .map(|b| match b {
                PromptButton::Ok(_) => "ok",
                PromptButton::Cancel(_) => "cancel",
                PromptButton::Other(_) => "other",
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        kinds(&answers(&UNSAVED_ANSWERS)),
        ["other", "other", "cancel"]
    );
    assert_eq!(kinds(&answers(&CONFLICT_ANSWERS)), ["cancel", "other"]);
    assert_eq!(kinds(&answers(&ENCODE_ANSWERS)), ["other", "cancel"]);
    let PromptButton::Cancel(label) = &answers(&UNSAVED_ANSWERS)[2] else {
        panic!("Cancel is the third answer");
    };
    assert_eq!(label.as_ref(), tr("Cancel"));
}

#[gpui_kit::test]
async fn thai_save_prompt_saves_on_its_first_answer_and_esc_cancels(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let (dir, handle, _workspace, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    type_x(cx, handle, &view);
    // The window's own Cancel (what Esc and the title-bar X answer) keeps the edit.
    let asked = cx
        .update_window(handle, |_, window, cx| {
            view.update(cx, |v, cx| v.confirm_discard(window, cx))
        })
        .unwrap();
    wait_for_prompt(cx).await;
    let (message, detail) = cx.pending_prompt().expect("a prompt");
    assert_eq!(message, trf("Save changes to {}?", &[&"a.txt"]));
    assert_eq!(detail, tr("Your edits are lost if you don't save them."));
    cx.simulate_prompt_answer(tr("Cancel"));
    assert!(!asked.await, "Cancel lets nothing go on");
    assert!(view.read_with(cx, |v, _| v.is_dirty()), "the edit stays");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "a\n"
    );
    // The first answer saves, whatever its label says.
    let asked = cx
        .update_window(handle, |_, window, cx| {
            view.update(cx, |v, cx| v.confirm_discard(window, cx))
        })
        .unwrap();
    wait_for_prompt(cx).await;
    cx.simulate_prompt_answer(tr("Save"));
    assert!(asked.await, "saved, so the caller may go on");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "ax\n"
    );
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

#[gpui_kit::test]
async fn a_still_saving_notice_stays_in_thai(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let (dir, handle, _workspace, view) = open_text(cx, "a\n", "a\n");
    wait_loaded(cx, handle, &view).await;
    type_x(cx, handle, &view);
    // Two saves of one file in one go: the second is refused, and says why.
    let first = cx
        .update_window(handle, |_, window, cx| {
            view.update(cx, |v, cx| {
                let first = v.save_side(true, window, cx);
                let _second = v.save_side(true, window, cx);
                first
            })
        })
        .unwrap();
    let expected = trf("Still saving {}.", &[&"a.txt"]);
    let notice = view.read_with(cx, |v, _| v.notice().map(str::to_owned));
    assert_eq!(notice.as_deref(), Some(expected.as_str()));
    assert_ne!(expected, "Still saving a.txt.", "it is Thai");
    assert!(first.await);
    assert!(!cx.has_pending_prompt());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "ax\n"
    );
    // The refusal that landed first is not wiped by the save finishing, in either language.
    let notice = view.read_with(cx, |v, _| v.notice().map(str::to_owned));
    assert_eq!(notice.as_deref(), Some(expected.as_str()));
}

#[gpui_kit::test]
async fn the_thai_binary_tab_still_offers_open_in_hex(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let (_dir, handle, workspace, view) = open_text(cx, "a\0b", "a\0c");
    wait_loaded(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let drawn = cx
        .update_window(handle, |_, window, _| {
            window
                .try_find("text-open-hex")
                .map(|found| found.label().map(str::to_owned))
        })
        .unwrap();
    assert_eq!(drawn, Some(Some(tr("Open in hex").to_owned())));
    cx.update_window(handle, |_, window, cx| window.click("text-open-hex", cx))
        .unwrap();
    cx.run_until_parked();
    let kind = workspace.read_with(cx, |w, _| {
        matches!(
            w.tabs().active(),
            Some(duet_app::workspace::TabItem::Hex(_))
        )
    });
    assert!(kind, "Open in hex still opens the hex tab in Thai");
}
