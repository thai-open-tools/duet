//! Its own binary, with one test: this is the only test that calls `share_process_wide`, and
//! nothing can take that back.

use duet_app::i18n::{Lang, current, scoped, set, share_process_wide, tr};

#[test]
fn after_sharing_a_set_reaches_other_threads_and_a_scope_still_wins() {
    let elsewhere = || std::thread::spawn(current).join().unwrap();
    assert_eq!(current(), Lang::En);
    set(Lang::Th);
    assert_eq!(current(), Lang::Th, "before sharing, on this thread");
    assert_eq!(elsewhere(), Lang::En, "and only on this thread");
    duet_core::i18n::set_thread_lang(None);

    share_process_wide();
    set(Lang::Th);
    assert_eq!(current(), Lang::Th);
    assert_eq!(elsewhere(), Lang::Th, "another thread sees it");
    assert_eq!(tr("files"), "ไฟล์");
    {
        let _en = scoped(Lang::En);
        assert_eq!(current(), Lang::En, "a scope wins");
        assert_eq!(tr("files"), "files");
        assert_eq!(elsewhere(), Lang::Th, "on its own thread only");
    }
    assert_eq!(current(), Lang::Th, "and leaves");
    set(Lang::En);
    assert_eq!(elsewhere(), Lang::En);

    // `main` sets the starting language for every thread, from a data folder of its own here.
    let data = tempfile::tempdir().unwrap();
    std::fs::write(
        data.path().join("settings.json"),
        r#"{ "help.lang": "th" }"#,
    )
    .unwrap();
    duet_app::i18n::startup(data.path());
    assert_eq!(current(), Lang::Th);
    assert_eq!(elsewhere(), Lang::Th);
    assert!(
        !std::fs::read_to_string(data.path().join("settings.json"))
            .unwrap()
            .contains("ui.lang"),
        "nothing is saved until the user chooses"
    );
}
