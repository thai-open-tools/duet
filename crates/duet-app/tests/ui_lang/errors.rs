//! Core's error sentences and worded labels in Thai (task 13): each variant said again from its
//! data, with its path and Windows' own text kept.

use std::io;
use std::path::PathBuf;

use duet_app::i18n::errors::{
    action_error, action_text, content_error, decode_error, diff_error, encode_error, folder_error,
    merge_read_error, read_error, rule_error, save_error,
};
use duet_app::i18n::{self, tr};
use duet_app::launch::Launch;
use duet_core::cancel::Cancelled;
use duet_core::folder::actions::ActionError;
use duet_core::folder::content::ContentError;
use duet_core::folder::entry::{Filters, ScanError};
use duet_core::folder::scan::validate_filters;
use duet_core::picture::decode::DecodeError;
use duet_core::text::diff::DiffError;
use duet_core::text::document::LoadError;
use duet_core::text::encoding::EncodeError;
use duet_core::text::merge::ReadError;
use duet_core::text::rules::{RuleError, compile_list};
use duet_core::text::save::SaveError;
use gpui_kit::TestAppContext;

use super::merge::{click, conflict_set, draw, label, open_merge};
use super::text::wait_loaded;
use super::thai_ui;

/// Sentences that are only a path and the system's own text, so Thai says them the same way.
const SAME_IN_THAI: [&str; 2] = ["ActionError::Io", "RuleError"];

/// Sentences that hold nothing but the system's own text, with no word of duet's own.
const ONLY_THEIR_TEXT: [&str; 3] = ["DecodeError::Io", "ReadError::Io", "ContentError::Io"];

const WINDOWS_TEXT: &str = "Access is denied. (os error 5)";

fn path() -> PathBuf {
    PathBuf::from(r"C:\work\notes.txt")
}

fn io_error() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, WINDOWS_TEXT)
}

/// Every worded variant core's error types have, each with what its Thai must still hold. The
/// `ScanError::ThreadPool` variant wraps a rayon error no test can build.
fn every_variant() -> Vec<(&'static str, String, Vec<String>)> {
    let p = path().display().to_string();
    let bad_glob = validate_filters(&Filters {
        include: vec!["[".to_owned()],
        exclude: Vec::new(),
    })
    .unwrap_err();
    assert!(matches!(bad_glob, ScanError::Filter(_)));
    let rule = compile_list("(").unwrap_err();
    // The English text of each variant, and what its Thai must keep. The first test compares the
    // mapper's English with it, the second its Thai.
    vec![
        (
            "ActionError::WouldOverwrite",
            ActionError::WouldOverwrite(path()).to_string(),
            vec![p.clone()],
        ),
        (
            "ActionError::Io",
            ActionError::Io {
                path: path(),
                source: io_error(),
            }
            .to_string(),
            vec![p.clone(), WINDOWS_TEXT.to_owned()],
        ),
        (
            "ActionError::Trash",
            ActionError::Trash {
                path: path(),
                message: "The device is not ready.".into(),
            }
            .to_string(),
            vec![p.clone(), "The device is not ready.".to_owned()],
        ),
        (
            "ActionError::DestinationIsLink",
            ActionError::DestinationIsLink(path()).to_string(),
            vec![p.clone()],
        ),
        (
            "ActionError::IntoItself",
            ActionError::IntoItself {
                src: path(),
                dst: PathBuf::from(r"C:\work\notes.txt\inner"),
            }
            .to_string(),
            vec![p.clone(), r"C:\work\notes.txt\inner".to_owned()],
        ),
        (
            "ActionError::Changed",
            ActionError::Changed(path()).to_string(),
            vec![],
        ),
        (
            "ScanError::Filter",
            bad_glob.to_string(),
            vec!["[".to_owned()],
        ),
        (
            "ScanError::Root",
            ScanError::Root {
                path: path(),
                source: io_error(),
            }
            .to_string(),
            vec![p.clone(), WINDOWS_TEXT.to_owned()],
        ),
        (
            "ScanError::NotADirectory",
            ScanError::NotADirectory(path()).to_string(),
            vec![p.clone()],
        ),
        (
            "ScanError::Cancelled",
            ScanError::Cancelled(Cancelled).to_string(),
            vec![],
        ),
        (
            "DecodeError::FileTooLarge",
            DecodeError::FileTooLarge(160).to_string(),
            vec!["160".to_owned()],
        ),
        (
            "DecodeError::Io",
            DecodeError::Io(WINDOWS_TEXT.into()).to_string(),
            vec![WINDOWS_TEXT.to_owned()],
        ),
        (
            "DecodeError::Unsupported",
            DecodeError::Unsupported("psd".into()).to_string(),
            vec!["psd".to_owned()],
        ),
        (
            "DecodeError::Corrupt",
            DecodeError::Corrupt.to_string(),
            vec![],
        ),
        ("DecodeError::Empty", DecodeError::Empty.to_string(), vec![]),
        (
            "DecodeError::TooLarge",
            DecodeError::TooLarge(70_000, 3).to_string(),
            vec!["70000".to_owned(), "3".to_owned()],
        ),
        (
            "DecodeError::Cancelled",
            DecodeError::Cancelled.to_string(),
            vec![],
        ),
        (
            "ReadError::FileTooLarge",
            ReadError::FileTooLarge(64).to_string(),
            vec!["64".to_owned()],
        ),
        ("ReadError::Binary", ReadError::Binary.to_string(), vec![]),
        (
            "ReadError::Io",
            ReadError::Io(WINDOWS_TEXT.into()).to_string(),
            vec![WINDOWS_TEXT.to_owned()],
        ),
        (
            "ReadError::Cancelled",
            ReadError::Cancelled.to_string(),
            vec![],
        ),
        (
            "LoadError::Io",
            LoadError::Io {
                path: path(),
                source: io_error(),
            }
            .to_string(),
            vec![p.clone(), WINDOWS_TEXT.to_owned()],
        ),
        (
            "LoadError::Binary",
            LoadError::Binary { path: path() }.to_string(),
            vec![p.clone()],
        ),
        (
            "LoadError::TooLargeAsText",
            LoadError::TooLargeAsText { path: path() }.to_string(),
            vec![p.clone()],
        ),
        (
            "LoadError::Cancelled",
            LoadError::Cancelled(Cancelled).to_string(),
            vec![],
        ),
        (
            "SaveError::ChangedOnDisk",
            SaveError::ChangedOnDisk { path: path() }.to_string(),
            vec![p.clone()],
        ),
        (
            "SaveError::Encode",
            SaveError::Encode(EncodeError::Unrepresentable {
                encoding: "windows-874",
                position: 12,
            })
            .to_string(),
            vec!["windows-874".to_owned(), "12".to_owned()],
        ),
        (
            "SaveError::Io",
            SaveError::Io {
                path: path(),
                source: io_error(),
            }
            .to_string(),
            vec![p.clone(), WINDOWS_TEXT.to_owned()],
        ),
        (
            "EncodeError::Unrepresentable",
            EncodeError::Unrepresentable {
                encoding: "windows-874",
                position: 12,
            }
            .to_string(),
            vec!["windows-874".to_owned(), "12".to_owned()],
        ),
        (
            "DiffError::Cancelled",
            DiffError::Cancelled(Cancelled).to_string(),
            vec![],
        ),
        (
            "DiffError::TooManyLines",
            DiffError::TooManyLines(9_000_000).to_string(),
            vec!["9000000".to_owned()],
        ),
        (
            "ContentError::Io",
            ContentError::Io(io_error()).to_string(),
            vec![WINDOWS_TEXT.to_owned()],
        ),
        (
            "ContentError::Cancelled",
            ContentError::Cancelled(Cancelled).to_string(),
            vec![],
        ),
        ("RuleError", rule.to_string(), vec!["(".to_owned()]),
    ]
}

/// Each variant through its mapper, in the order of `every_variant`.
fn mapped() -> Vec<String> {
    let bad_glob = validate_filters(&Filters {
        include: vec!["[".to_owned()],
        exclude: Vec::new(),
    })
    .unwrap_err();
    let rule: RuleError = compile_list("(").unwrap_err();
    vec![
        action_error(&ActionError::WouldOverwrite(path())),
        action_error(&ActionError::Io {
            path: path(),
            source: io_error(),
        }),
        action_error(&ActionError::Trash {
            path: path(),
            message: "The device is not ready.".into(),
        }),
        action_error(&ActionError::DestinationIsLink(path())),
        action_error(&ActionError::IntoItself {
            src: path(),
            dst: PathBuf::from(r"C:\work\notes.txt\inner"),
        }),
        action_error(&ActionError::Changed(path())),
        folder_error(&bad_glob),
        folder_error(&ScanError::Root {
            path: path(),
            source: io_error(),
        }),
        folder_error(&ScanError::NotADirectory(path())),
        folder_error(&ScanError::Cancelled(Cancelled)),
        decode_error(&DecodeError::FileTooLarge(160)),
        decode_error(&DecodeError::Io(WINDOWS_TEXT.into())),
        decode_error(&DecodeError::Unsupported("psd".into())),
        decode_error(&DecodeError::Corrupt),
        decode_error(&DecodeError::Empty),
        decode_error(&DecodeError::TooLarge(70_000, 3)),
        decode_error(&DecodeError::Cancelled),
        merge_read_error(&ReadError::FileTooLarge(64)),
        merge_read_error(&ReadError::Binary),
        merge_read_error(&ReadError::Io(WINDOWS_TEXT.into())),
        merge_read_error(&ReadError::Cancelled),
        read_error(&LoadError::Io {
            path: path(),
            source: io_error(),
        }),
        read_error(&LoadError::Binary { path: path() }),
        read_error(&LoadError::TooLargeAsText { path: path() }),
        read_error(&LoadError::Cancelled(Cancelled)),
        save_error(&SaveError::ChangedOnDisk { path: path() }),
        save_error(&SaveError::Encode(EncodeError::Unrepresentable {
            encoding: "windows-874",
            position: 12,
        })),
        save_error(&SaveError::Io {
            path: path(),
            source: io_error(),
        }),
        encode_error(&EncodeError::Unrepresentable {
            encoding: "windows-874",
            position: 12,
        }),
        diff_error(&DiffError::Cancelled(Cancelled)),
        diff_error(&DiffError::TooManyLines(9_000_000)),
        content_error(&ContentError::Io(io_error())),
        content_error(&ContentError::Cancelled(Cancelled)),
        rule_error(&rule),
    ]
}

#[test]
fn every_error_variant_reads_as_before_in_english() {
    let (variants, mapped) = (every_variant(), mapped());
    assert_eq!(variants.len(), mapped.len());
    for ((name, english, _), said) in variants.iter().zip(&mapped) {
        assert_eq!(said, english, "{name}");
    }
    assert!(i18n::missing_keys_for_test().is_empty());
}

#[test]
fn every_error_variant_reads_in_thai_and_keeps_its_path() {
    let (variants, english) = (every_variant(), mapped());
    let _th = thai_ui();
    let thai = mapped();
    for (((name, said_in_english, keeps), said), plain) in variants.iter().zip(&thai).zip(&english)
    {
        if !SAME_IN_THAI.contains(name) && !ONLY_THEIR_TEXT.contains(name) {
            assert_ne!(said, said_in_english, "{name} stays English");
        }
        assert_eq!(plain, said_in_english);
        for kept in keeps {
            assert!(said.contains(kept), "{name}: {said:?} lost {kept:?}");
        }
        // Not one Latin word of the sentence is left but the ones core's text keeps.
        assert!(
            said.chars().any(|c| ('\u{0E00}'..='\u{0E7F}').contains(&c))
                || SAME_IN_THAI.contains(name)
                || ONLY_THEIR_TEXT.contains(name),
            "{name}: {said:?} holds no Thai"
        );
    }
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

#[test]
fn an_error_text_a_report_holds_is_said_again_in_thai() {
    let known = [
        ActionError::WouldOverwrite(path()),
        ActionError::Trash {
            path: path(),
            message: "The device is not ready.".into(),
        },
        ActionError::DestinationIsLink(path()),
        ActionError::IntoItself {
            src: path(),
            dst: PathBuf::from(r"C:\work\notes.txt\inner"),
        },
        ActionError::Changed(path()),
    ];
    for err in &known {
        assert_eq!(action_text(&err.to_string()), err.to_string(), "{err}");
    }
    let _th = thai_ui();
    for err in &known {
        assert_eq!(action_text(&err.to_string()), action_error(err), "{err}");
        assert_ne!(action_text(&err.to_string()), err.to_string(), "{err}");
    }
    // Windows' own text is not ours to say again.
    let own = ActionError::Io {
        path: path(),
        source: io_error(),
    };
    assert_eq!(action_text(&own.to_string()), action_error(&own));
    assert_eq!(action_text("denied"), "denied");
    assert!(action_text(&own.to_string()).contains(WINDOWS_TEXT));
}

/// The trash crate's own debug text is not for a person: both languages say it in plain words,
/// and a readable system sentence stays.
#[test]
fn the_trash_crates_raw_text_is_said_in_plain_words() {
    let raw = ActionError::Trash {
        path: path(),
        message: r#"Error during a `trash` operation: Unknown { description: "Some operations were aborted" }"#.into(),
    };
    let english = action_error(&raw);
    assert!(
        !english.contains("Unknown {") && !english.contains("`trash`"),
        "{english}"
    );
    assert!(english.starts_with("cannot move "), "{english}");
    assert_eq!(action_text(&raw.to_string()), english);
    let _th = thai_ui();
    let thai = action_error(&raw);
    assert!(
        !thai.contains("Unknown {") && !thai.contains("`trash`"),
        "{thai}"
    );
    assert!(thai.contains("ถังขยะ"), "{thai}");
    assert_eq!(action_text(&raw.to_string()), thai);
}

#[gpui_kit::test]
async fn a_thai_failed_open_keeps_windows_own_text(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let dir = tempfile::tempdir().unwrap();
    let (a, missing) = (dir.path().join("a.txt"), dir.path().join("missing.txt"));
    std::fs::write(&a, "x\n").unwrap();
    let (handle, workspace) = super::open(
        cx,
        Launch::Files {
            left: a,
            right: missing.clone(),
        },
    );
    let view = super::text::text_of(&workspace, cx);
    wait_loaded(cx, handle, &view).await;
    let message = view
        .read_with(cx, |v, _| v.message().map(str::to_owned))
        .expect("the open failed");
    assert!(message.starts_with("อ่าน "), "{message}");
    assert!(
        message.contains(&missing.display().to_string()),
        "{message}"
    );
    // The tail is the system's own text for the missing file, whatever language Windows is in.
    let system = std::fs::read(&missing).unwrap_err().to_string();
    assert!(message.ends_with(&system), "{message} / {system}");
}

#[gpui_kit::test]
async fn merge_choices_read_in_thai(cx: &mut TestAppContext) {
    let _th = thai_ui();
    let dir = tempfile::tempdir().unwrap();
    let (handle, _workspace, view) = open_merge(cx, conflict_set(dir.path())).await;
    draw(cx, handle);
    for (id, key) in [
        ("merge-use-left", "Use left"),
        ("merge-use-right", "Use right"),
        ("merge-use-left-right", "Left + right"),
        ("merge-use-right-left", "Right + left"),
        ("merge-use-base", "Use base"),
    ] {
        let drawn = label(cx, handle, (id, 1usize));
        assert_eq!(drawn.as_deref(), Some(tr(key)), "{id}");
        assert_ne!(tr(key), key, "{key} has Thai");
    }
    let position = label(cx, handle, "merge-pos").expect("the position is drawn");
    assert!(position.starts_with(tr("Conflict")), "{position}");
    assert!(position.ends_with(" / 1"), "{position}");
    assert_ne!(tr("Conflict"), "Conflict");
    click(cx, handle, ("merge-use-left", 1usize));
    let card = view.read_with(cx, |v, _| v.card_text(1)).unwrap();
    assert_eq!(card.state, format!("✓ {}", tr("Use left")));
    assert_eq!(tr("Use the left text (Alt+←)"), "ใช้ข้อความฝั่งซ้าย (Alt+←)");
    assert_eq!(i18n::missing_keys_for_test(), Vec::<&str>::new());
}

#[test]
fn the_refusal_of_overlapping_roots_reads_in_thai() {
    use duet_core::folder::sync::{SyncPlan, run_sync};
    // The text core writes when one folder is inside the other, taken from a real run.
    let dir = tempfile::tempdir().unwrap();
    let inner = dir.path().join("inner");
    std::fs::create_dir(&inner).unwrap();
    let report = run_sync(
        &SyncPlan::default(),
        dir.path(),
        &inner,
        &|_| Ok(()),
        &duet_core::cancel::CancelToken::new(),
        &std::sync::atomic::AtomicUsize::new(0),
    );
    let english = report.to_right.failed[0].1.clone();
    assert_eq!(action_text(&english), english, "English is as it was");
    let _th = thai_ui();
    let thai = action_text(&english);
    assert_ne!(thai, english);
    assert_eq!(
        thai,
        tr("one folder is inside the other, so nothing was synced")
    );
}
