//! The language of the text duet writes. English text is the key: in English `tr` returns its
//! input, in Thai a table entry, and a missing entry falls back to the English.
//!
//! Core has no app state, so it never sees the UI's language: report text reads the language a
//! `scoped` call set on this thread, and English with no scope.

use std::cell::Cell;
use std::fmt::{Display, Write as _};
use std::marker::PhantomData;
use std::sync::LazyLock;

mod th_report;

/// A language duet reads in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    En,
    Th,
}

impl Lang {
    /// The word a settings file keeps.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Th => "th",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "en" => Some(Lang::En),
            "th" => Some(Lang::Th),
            _ => None,
        }
    }
}

thread_local! {
    static SCOPED: Cell<Option<Lang>> = const { Cell::new(None) };
}

/// This thread's language until it is dropped, when the value it replaced comes back. Not
/// `Send`: it must drop on the thread it changed.
pub struct LangScope {
    previous: Option<Lang>,
    _this_thread: PhantomData<*const ()>,
}

impl Drop for LangScope {
    fn drop(&mut self) {
        set_thread_lang(self.previous);
    }
}

/// Sets this thread's language for as long as the returned value lives.
///
/// Mind the thread: in a test the UI thread is the test thread, so a scope there also feeds core's
/// `tr` for calls made straight from the UI. The running app's UI thread has no scope, and reads
/// the window's language. A UI call into a core report text builder (`criterion_text`,
/// `rules_text`, `default_names`, `describe`) must therefore set the report's language itself;
/// `i18n_hygiene` fails on a call that does not.
#[must_use = "the language goes back when the scope is dropped"]
pub fn scoped(lang: Lang) -> LangScope {
    let previous = scoped_lang();
    set_thread_lang(Some(lang));
    LangScope {
        previous,
        _this_thread: PhantomData,
    }
}

/// This thread's language, when it has one of its own.
#[inline]
pub fn scoped_lang() -> Option<Lang> {
    SCOPED.get()
}

/// Sets this thread's language with no scope to take it back: `scoped` is what code wants.
#[doc(hidden)]
pub fn set_thread_lang(lang: Option<Lang>) {
    SCOPED.set(lang);
}

/// Marks a literal as a key, in a `const` or a core label, for the code that draws it to pass to
/// `tr`. The hygiene tests read the mark; the value is the literal, so a test that compares the
/// constant keeps passing.
pub const fn tr_key(key: &'static str) -> &'static str {
    key
}

/// `template` with its holes filled: `{}` takes the arguments in order and `{0}` names one, so a
/// Thai value can reorder them. One pass: an argument is never read as a template. What is not a
/// hole, or a hole with no argument, stays as it is.
pub fn fill(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + args.len() * 8);
    let mut next = 0;
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        rest = &rest[open..];
        let hole = rest.find('}').and_then(|close| {
            let inside = &rest[1..close];
            let index = if inside.is_empty() {
                next += 1;
                next - 1
            } else if inside.bytes().all(|b| b.is_ascii_digit()) {
                inside.parse().ok()?
            } else {
                return None;
            };
            Some((close, args.get(index)?))
        });
        match hole {
            Some((close, arg)) => {
                let _ = write!(out, "{arg}");
                rest = &rest[close + 1..];
            }
            // The brace is text; what follows it is read again.
            None => {
                out.push('{');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The tables of each area as one list sorted by key, every entry kept: a duplicate is for the
/// hygiene test to find, not for this to hide.
pub fn merged(
    parts: &[&'static [(&'static str, &'static str)]],
) -> Vec<(&'static str, &'static str)> {
    let mut all: Vec<_> = parts.iter().flat_map(|part| part.iter().copied()).collect();
    all.sort_by_key(|(key, _)| *key);
    all
}

/// `key`'s value in a list sorted by key, by binary search.
pub fn lookup(sorted: &[(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    sorted
        .binary_search_by_key(&key, |(entry, _)| entry)
        .ok()
        .map(|at| sorted[at].1)
}

static REPORT_TABLE: LazyLock<Vec<(&'static str, &'static str)>> =
    LazyLock::new(|| merged(&[th_report::ENTRIES]));

/// Report text in this thread's language; English with no scope.
pub fn tr(key: &'static str) -> &'static str {
    match scoped_lang().unwrap_or_default() {
        Lang::En => key,
        Lang::Th => lookup(&REPORT_TABLE, key).unwrap_or(key),
    }
}

/// The report table as written, for the tests that hold it to the sources.
#[doc(hidden)]
pub fn report_entries() -> &'static [(&'static str, &'static str)] {
    th_report::ENTRIES
}

/// `tr` on a template, with its holes filled (`fill`).
pub fn trf(key: &'static str, args: &[&dyn Display]) -> String {
    fill(tr(key), args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_replaces_holes_in_order_and_never_rescans_an_argument() {
        assert_eq!(fill("{} and {}", &[&"{}", &2]), "{} and 2");
        assert_eq!(fill("no holes", &[]), "no holes");
        assert_eq!(fill("{}{}", &[&1, &2]), "12");
    }

    #[test]
    fn fill_takes_numbered_holes_so_thai_can_reorder() {
        assert_eq!(fill("{1} ก {0}", &[&"a", &"b"]), "b ก a");
        assert_eq!(fill("{0} {0}", &[&"a"]), "a a");
        // An argument that looks like a hole stays as it is.
        assert_eq!(fill("{1} {0}", &[&"{1}", &"{0}"]), "{0} {1}");
    }

    #[test]
    fn fill_leaves_what_is_not_a_hole() {
        assert_eq!(fill("{name} {", &[&1]), "{name} {");
        assert_eq!(fill("{} {}", &[&1]), "1 {}", "no argument for the hole");
        assert_eq!(fill("{5}", &[&1]), "{5}");
        assert_eq!(fill("ไฟล์ {} ไฟล์", &[&3]), "ไฟล์ 3 ไฟล์");
    }

    #[test]
    fn a_scope_restores_what_it_replaced_and_stays_on_its_thread() {
        assert_eq!(scoped_lang(), None);
        {
            let _outer = scoped(Lang::Th);
            assert_eq!(scoped_lang(), Some(Lang::Th));
            {
                let _inner = scoped(Lang::En);
                assert_eq!(scoped_lang(), Some(Lang::En));
            }
            assert_eq!(scoped_lang(), Some(Lang::Th));
            let other = std::thread::spawn(scoped_lang).join().unwrap();
            assert_eq!(other, None, "another thread sees no scope");
        }
        assert_eq!(scoped_lang(), None);
    }

    #[test]
    fn lookup_is_exact_and_merged_keeps_every_entry_sorted() {
        const FIRST: &[(&str, &str)] = &[("b", "2"), ("d", "4")];
        const SECOND: &[(&str, &str)] = &[("c", "3"), ("a", "1"), ("b", "again")];
        let table = merged(&[FIRST, SECOND]);
        assert_eq!(table.len(), 5, "a duplicate stays, for the hygiene test");
        assert!(table.windows(2).all(|pair| pair[0].0 <= pair[1].0));
        assert_eq!(lookup(&table, "a"), Some("1"));
        assert_eq!(lookup(&table, "d"), Some("4"));
        assert_eq!(lookup(&table, "e"), None);
        assert_eq!(lookup(&table, ""), None);
        assert_eq!(lookup(&table, "A"), None, "exact");
        assert_eq!(lookup(&[], "a"), None);
    }

    #[test]
    fn report_text_is_english_without_a_scope() {
        const KEY: &str = "Not a report word";
        assert!(std::ptr::eq(tr(KEY), KEY));
        assert_eq!(trf("{} files", &[&3]), "3 files");
        let _th = scoped(Lang::Th);
        assert_eq!(tr(KEY), KEY, "a missing entry falls back to English");
    }

    #[test]
    fn a_language_has_its_code() {
        assert_eq!(Lang::default(), Lang::En);
        for lang in [Lang::En, Lang::Th] {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
        }
        assert_eq!(Lang::Th.code(), "th");
        assert_eq!(Lang::from_code("fr"), None);
        assert_eq!(tr_key("x"), "x");
    }
}
