//! The UI's language. English text is the key: in English `tr` returns its input without a
//! lookup, in Thai a table entry, and a missing entry falls back to the English.
//!
//! `tr` goes at the call site, never inside a widget: a widget cannot tell a label from a file
//! name, and a file called "Compare" stays as it is.
//!
//! The running app has one language, shared by every thread. A test never shares it: there `set`
//! writes the test's own thread, so parallel tests cannot see each other's language.

use std::borrow::Cow;
use std::cell::RefCell;
use std::fmt::{self, Display};
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering::Relaxed};

pub use duet_core::i18n::{Lang, LangScope, fill, scoped, tr_key};
use duet_core::i18n::{lookup, scoped_lang, set_thread_lang};
use gpui_kit::{App, PromptButton, SharedString};

pub mod errors;
#[doc(hidden)]
pub mod th;

/// The process-wide language, read when a thread has no language of its own.
static UI: AtomicU8 = AtomicU8::new(0);
/// Whether `set` writes `UI`: only once `share_process_wide` has run.
static SHARED: AtomicBool = AtomicBool::new(false);

const fn encode(lang: Lang) -> u8 {
    match lang {
        Lang::En => 0,
        Lang::Th => 1,
    }
}

const fn decode(value: u8) -> Lang {
    match value {
        0 => Lang::En,
        _ => Lang::Th,
    }
}

/// This thread's scope, else the process-wide language.
#[inline]
pub fn current() -> Lang {
    scoped_lang().unwrap_or_else(|| decode(UI.load(Relaxed)))
}

/// Chooses the language: for every thread once `app::run` has shared it, otherwise (every test)
/// for this thread only.
pub fn set(lang: Lang) {
    if SHARED.load(Relaxed) {
        UI.store(encode(lang), Relaxed);
    } else {
        set_thread_lang(Some(lang));
    }
}

/// From here on `set` reaches every thread. Only `app::run` calls this, so a test can never write
/// the process-wide language, even through the header or the Help switch.
pub fn share_process_wide() {
    SHARED.store(true, Relaxed);
}

/// The `settings.json` key that keeps the choice.
pub const LANG_KEY: &str = "ui.lang";
/// The key that kept the Help tab's own choice before: read, never written.
const OLD_HELP_KEY: &str = "help.lang";

/// The language saved in the settings: `ui.lang`, then `help.lang`. An unknown value counts as
/// none.
pub fn saved_lang(settings: &serde_json::Map<String, serde_json::Value>) -> Option<Lang> {
    [LANG_KEY, OLD_HELP_KEY].iter().find_map(|key| {
        let code = settings.get(*key)?.as_str()?;
        Lang::from_code(code)
    })
}

/// The saved choice; with none, Thai on Thai Windows and otherwise English.
pub fn first_run_lang(saved: Option<Lang>, system_thai: bool) -> Lang {
    saved.unwrap_or(if system_thai { Lang::Th } else { Lang::En })
}

/// Whether a Windows language id is Thai: `LANG_THAI` in its low ten bits, whatever the region.
pub fn is_thai_langid(id: u16) -> bool {
    id & 0x3ff == 0x1e
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    safe fn GetUserDefaultUILanguage() -> u16;
}

/// Whether Windows shows its own text in Thai; false off Windows.
pub fn system_is_thai() -> bool {
    #[cfg(windows)]
    {
        is_thai_langid(GetUserDefaultUILanguage())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// The language duet starts in, with `data_dir`'s settings and the given Windows language.
pub fn starting_lang(data_dir: &Path, system_thai: bool) -> Lang {
    let settings = crate::settings::read_all(&data_dir.join(crate::settings::SETTINGS_FILE));
    first_run_lang(saved_lang(&settings), system_thai)
}

/// Sets the language the process starts in. Nothing is saved until the user chooses. `main` calls
/// this before it reads the arguments, whose errors are text. Only here does the machine's language
/// come in, so it never reaches a test.
pub fn startup(data_dir: &Path) {
    UI.store(encode(starting_lang(data_dir, system_is_thai())), Relaxed);
}

/// The user chose `lang`: it takes effect and is saved. Tabs and windows are the caller's.
pub fn choose(lang: Lang, cx: &App) {
    set(lang);
    if let Some(path) = crate::settings::settings_file(cx)
        && let Err(err) = crate::settings::set_string(&path, LANG_KEY, lang.code())
    {
        tracing::warn!(%err, "could not save the language");
    }
}

/// `key` in the current language: the same string in English, with no lookup.
#[inline]
pub fn tr(key: &'static str) -> &'static str {
    match current() {
        Lang::En => key,
        Lang::Th => thai(key),
    }
}

/// `tr` on a template, with its holes filled: `{}` in order, `{0}` numbered (`fill`).
pub fn trf(key: &'static str, args: &[&dyn Display]) -> String {
    fill(tr(key), args)
}

/// One English label with two Thai meanings: its entry's key is `context|key`. English returns
/// `key`.
pub fn trc(context: &'static str, key: &'static str) -> &'static str {
    match current() {
        Lang::En => key,
        Lang::Th => lookup_in_context(th::table(), context, key).unwrap_or_else(|| {
            record_missing_in_context(context, key);
            key
        }),
    }
}

/// Text kept in a field or sent from a job, which the view's `relabel` builds again when the
/// language changes. The closure holds what the text is made from. It dereferences to `str`, so an
/// `Option<LiveText>` reads as `Option<&str>` through `as_deref`.
#[derive(Clone)]
pub struct LiveText {
    make: Arc<dyn Fn() -> String + Send + Sync>,
    text: SharedString,
}

impl LiveText {
    pub fn new(make: impl Fn() -> String + Send + Sync + 'static) -> Self {
        let text = make().into();
        Self {
            make: Arc::new(make),
            text,
        }
    }

    /// Builds the text again in the current language.
    pub fn relabel(&mut self) {
        self.text = (self.make)().into();
    }

    /// The text built now in the language now in force, the stored text left as it is: for a
    /// report written in a language other than the window's.
    pub fn said(&self) -> String {
        (self.make)()
    }
}

/// A `LiveText` of one fixed key.
pub fn live(key: &'static str) -> LiveText {
    LiveText::new(move || tr(key).to_owned())
}

impl fmt::Debug for LiveText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("LiveText").field(&self.text).finish()
    }
}

impl Display for LiveText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Deref for LiveText {
    type Target = str;

    fn deref(&self) -> &str {
        &self.text
    }
}

/// A fixed core sentence that arrives as a string: said again in the language in force, and
/// kept as it is when the table has no entry for it.
impl From<String> for LiveText {
    fn from(text: String) -> Self {
        LiveText::new(move || tr_str(&text).into_owned())
    }
}

impl From<&str> for LiveText {
    fn from(text: &str) -> Self {
        text.to_owned().into()
    }
}

impl From<&LiveText> for SharedString {
    fn from(text: &LiveText) -> Self {
        text.text.clone()
    }
}

/// The two answers every prompt may hold, which gpui reads as the Windows OK and Cancel buttons.
pub const OK: &str = tr_key("OK");
pub const CANCEL: &str = tr_key("Cancel");

/// The buttons of a prompt, each in the current language. gpui reads only the English words `ok` and
/// `cancel` as the Windows buttons, and a dialog with no Cancel button vetoes Esc and the title-bar
/// X, so `"Cancel"` and `"OK"` keep their kind whatever their label says (D10).
pub fn answers(keys: &[&'static str]) -> Vec<PromptButton> {
    keys.iter()
        .map(|&key| {
            if key.eq_ignore_ascii_case(CANCEL) {
                PromptButton::cancel(tr(key))
            } else if key.eq_ignore_ascii_case(OK) {
                PromptButton::ok(tr(key))
            } else {
                PromptButton::new(tr(key))
            }
        })
        .collect()
}

/// Marks a literal whose Thai sits in a table of its own (`SHORTCUTS_TH`, `LEGEND_TH`), not in the
/// UI table, for the hygiene test.
pub const fn paired(key: &'static str) -> &'static str {
    key
}

/// A fixed sentence that arrives as a `String`, from core. Unknown text stays as it is.
pub fn tr_str(text: &str) -> Cow<'_, str> {
    match current() {
        Lang::En => Cow::Borrowed(text),
        Lang::Th => Cow::Borrowed(lookup(th::table(), text).unwrap_or(text)),
    }
}

fn thai(key: &'static str) -> &'static str {
    lookup(th::table(), key).unwrap_or_else(|| {
        record_missing(key);
        key
    })
}

/// A binary search for the entry `context|key`, compared piece by piece so nothing is allocated.
fn lookup_in_context(
    sorted: &[(&'static str, &'static str)],
    context: &str,
    key: &str,
) -> Option<&'static str> {
    let joined = || {
        context
            .bytes()
            .chain(std::iter::once(b'|'))
            .chain(key.bytes())
    };
    sorted
        .binary_search_by(|(entry, _)| entry.bytes().cmp(joined()))
        .ok()
        .map(|at| sorted[at].1)
}

thread_local! {
    static MISSING: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

/// Notes a key with no Thai entry, once. The list cannot grow past the keys in the source.
#[cold]
fn record_missing(key: &'static str) {
    MISSING.with_borrow_mut(|missing| {
        if !missing.contains(&key) {
            missing.push(key);
        }
    });
}

/// `record_missing` for `context|key`. A frame draws the same label again and again, so the text
/// is built only for a pair not yet noted.
#[cold]
fn record_missing_in_context(context: &'static str, key: &'static str) {
    MISSING.with_borrow_mut(|missing| {
        let noted = missing.iter().any(|entry| {
            entry
                .strip_prefix(context)
                .and_then(|rest| rest.strip_prefix('|'))
                == Some(key)
        });
        if !noted {
            missing.push(Box::leak(format!("{context}|{key}").into_boxed_str()));
        }
    });
}

/// The keys a Thai `tr` found no entry for, on this thread.
#[doc(hidden)]
pub fn missing_keys_for_test() -> Vec<&'static str> {
    MISSING.with_borrow(Vec::clone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::count_of;

    #[test]
    fn english_tr_returns_the_key_itself() {
        const KEY: &str = "Compare files";
        assert!(std::ptr::eq(tr(KEY), KEY), "no lookup, so no other string");
        const FILES: &str = "files";
        assert!(std::ptr::eq(tr(FILES), FILES), "even when the table has it");
        assert_eq!(trf("{} of {}", &[&1, &2]), "1 of 2");
        assert!(missing_keys_for_test().is_empty());
    }

    #[test]
    fn thai_tr_finds_the_entry_and_a_missing_key_falls_back_and_is_recorded() {
        let _th = scoped(Lang::Th);
        assert_eq!(tr("files"), "ไฟล์");
        assert!(missing_keys_for_test().is_empty());
        const MISSING: &str = "No such key in any table";
        assert!(std::ptr::eq(tr(MISSING), MISSING));
        assert!(std::ptr::eq(tr(MISSING), MISSING));
        assert_eq!(missing_keys_for_test(), [MISSING], "named once");
        assert_eq!(trf("{} not in a table", &[&2]), "2 not in a table");
        assert_eq!(missing_keys_for_test(), [MISSING, "{} not in a table"]);
        let other = std::thread::spawn(missing_keys_for_test).join().unwrap();
        assert!(other.is_empty(), "on this thread only");
    }

    #[test]
    fn trc_finds_its_own_entry_and_english_returns_the_label() {
        const TABLE: &[(&str, &str)] = &[
            ("Left", "ซ้าย"),
            ("panel|Left", "ฝั่งซ้าย"),
            ("panel|Lefty", "อื่น"),
            ("tab|Left", "แท็บซ้าย"),
        ];
        assert_eq!(lookup_in_context(TABLE, "panel", "Left"), Some("ฝั่งซ้าย"));
        assert_eq!(lookup_in_context(TABLE, "tab", "Left"), Some("แท็บซ้าย"));
        assert_eq!(lookup_in_context(TABLE, "tab", "Lef"), None, "exact");
        assert_eq!(lookup_in_context(TABLE, "pane", "Left"), None);
        const LABEL: &str = "Left";
        assert!(std::ptr::eq(trc("panel", LABEL), LABEL), "English");
        let _th = scoped(Lang::Th);
        assert!(
            std::ptr::eq(trc("no such context", LABEL), LABEL),
            "a missing entry falls back"
        );
        assert_eq!(missing_keys_for_test(), ["no such context|Left"]);
    }

    #[test]
    fn set_under_test_stays_on_this_thread() {
        assert_eq!(current(), Lang::En, "English is the default under test");
        set(Lang::Th);
        assert_eq!(current(), Lang::Th);
        assert_eq!(tr("files"), "ไฟล์");
        let other = std::thread::spawn(current).join().unwrap();
        assert_eq!(other, Lang::En, "another test's thread never sees it");
        {
            let _en = scoped(Lang::En);
            assert_eq!(current(), Lang::En);
        }
        assert_eq!(current(), Lang::Th);
        set(Lang::En);
        assert_eq!(current(), Lang::En);
    }

    #[test]
    fn count_of_uses_the_thai_noun_and_arabic_digits() {
        assert_eq!(count_of(1_234, "file", "files"), "1,234 files");
        let _th = scoped(Lang::Th);
        assert_eq!(count_of(1_234, "file", "files"), "1,234 ไฟล์");
        assert_eq!(count_of(1, "file", "files"), "1 ไฟล์", "no singular in Thai");
    }

    #[test]
    fn paired_and_tr_str_mark_and_read_text() {
        assert_eq!(paired("Help"), "Help");
        const CONST_KEY: &str = tr_key("Help");
        assert_eq!(CONST_KEY, "Help");
        let owned = String::from("files");
        assert_eq!(tr_str(&owned), "files");
        let _th = scoped(Lang::Th);
        assert_eq!(tr_str(&owned), "ไฟล์");
        assert_eq!(tr_str("not in the table"), "not in the table");
        assert!(
            missing_keys_for_test().is_empty(),
            "a sentence that arrives as a String has no key to record"
        );
    }

    #[test]
    fn saved_lang_prefers_ui_lang_then_help_lang_and_ignores_garbage() {
        let map = |json: serde_json::Value| json.as_object().unwrap().clone();
        assert_eq!(saved_lang(&map(serde_json::json!({}))), None);
        assert_eq!(
            saved_lang(&map(serde_json::json!({ "help.lang": "th" }))),
            Some(Lang::Th)
        );
        assert_eq!(
            saved_lang(&map(
                serde_json::json!({ "ui.lang": "en", "help.lang": "th" })
            )),
            Some(Lang::En),
            "ui.lang first"
        );
        assert_eq!(
            saved_lang(&map(
                serde_json::json!({ "ui.lang": "fr", "help.lang": "th" })
            )),
            Some(Lang::Th),
            "an unknown value counts as none"
        );
        assert_eq!(
            saved_lang(&map(serde_json::json!({ "ui.lang": 1, "help.lang": "xx" }))),
            None
        );
    }

    #[test]
    fn first_run_follows_a_saved_choice_then_windows() {
        assert_eq!(first_run_lang(None, true), Lang::Th);
        assert_eq!(first_run_lang(None, false), Lang::En);
        assert_eq!(first_run_lang(Some(Lang::En), true), Lang::En);
        assert_eq!(first_run_lang(Some(Lang::Th), false), Lang::Th);
    }

    #[test]
    fn only_thai_langids_count() {
        assert!(is_thai_langid(0x041E), "Thai (Thailand)");
        assert!(!is_thai_langid(0x0409), "English (United States)");
        assert!(!is_thai_langid(0x0454), "Lao");
    }

    #[test]
    fn a_live_text_follows_the_language() {
        let mut text = live("files");
        assert_eq!(&*text, "files");
        assert_eq!(text.to_string(), "files");
        let held = |text: &LiveText| Some(text.clone());
        assert_eq!(held(&text).as_deref(), Some("files"), "as a field holds it");
        let _th = scoped(Lang::Th);
        assert_eq!(&*text, "files", "as it was built, until it is relabelled");
        text.relabel();
        assert_eq!(&*text, "ไฟล์");
        assert_eq!(SharedString::from(&text), "ไฟล์");

        // A sentence built from data keeps the data and follows the language.
        let name = String::from("a.txt");
        let mut sentence = LiveText::new(move || trf("{} files", &[&name]));
        assert_eq!(&*sentence, "a.txt files");
        let _en = scoped(Lang::En);
        sentence.relabel();
        assert_eq!(&*sentence, "a.txt files");
        assert!(format!("{sentence:?}").contains("a.txt files"));
    }

    #[test]
    fn thai_answers_keep_cancel_as_the_windows_cancel_button() {
        let _th = scoped(Lang::Th);
        let buttons = answers(&["Save", "Cancel"]);
        assert!(matches!(&buttons[0], PromptButton::Other(label) if label == tr("Save")));
        assert!(matches!(&buttons[1], PromptButton::Cancel(label) if label == "ยกเลิก"));
        assert!(matches!(answers(&["OK"])[0], PromptButton::Ok(_)));
        assert_eq!(answers(&["OK"])[0].label(), "ตกลง");
        assert_eq!(buttons[0].label(), "บันทึก");
        let untranslated = answers(&["No such answer"]);
        assert_eq!(
            untranslated[0].label(),
            "No such answer",
            "no entry: the English stays"
        );
        let _en = scoped(Lang::En);
        assert!(
            matches!(&answers(&["Cancel"])[0], PromptButton::Cancel(label) if label == "Cancel")
        );
    }

    #[test]
    fn every_table_entry_is_in_the_table_sorted() {
        let table = th::table();
        assert!(table.windows(2).all(|pair| pair[0].0 <= pair[1].0));
        for (key, value) in th::common::ENTRIES {
            assert_eq!(lookup(table, key), Some(*value));
        }
    }
}
