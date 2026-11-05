//! The two Explorer right-click verbs: their names and command lines (pure) and the code that
//! writes them to the registry. The installer (`installer/duet.iss`) writes the same keys with the
//! same command lines.

use std::io;
use std::path::Path;

use crate::i18n::Lang;

/// Key-name prefix of the real verbs. A test installer variant uses its own, so it can never
/// touch these.
pub const VERB_PREFIX: &str = "duet";

/// Where the verbs go, under `HKEY_CURRENT_USER`: files (`*`) and folders.
pub const CLASSES_PATH: &str = r"Software\Classes";

/// The keys under `Software\Classes` that carry a `shell` subkey with the verbs.
pub const ROOTS: [&str; 2] = ["*", "Directory"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verb {
    pub key: String,
    pub label: String,
    pub command: String,
}

/// The two verbs' names in `lang`: the words of the installer's `VerbSelectLeft` and
/// `VerbCompareTo` in each language.
pub fn labels(lang: Lang) -> [&'static str; 2] {
    match lang {
        Lang::En => ["Select as left", "Compare to selected left"],
        Lang::Th => ["เลือกเป็นฝั่งซ้าย", "เทียบกับฝั่งซ้ายที่เลือกไว้"],
    }
}

/// The "Select as left" and "Compare to selected left" verbs, named in the language in force
/// (`--register-shell` runs after `i18n::startup`, so that is the saved or first-run one). The
/// exe path and `%1` are quoted, so a path with spaces or Thai letters arrives as one argument.
pub fn verbs(exe: &Path, prefix: &str) -> [Verb; 2] {
    verbs_in(exe, prefix, crate::i18n::current())
}

/// `verbs` named in `lang`.
pub fn verbs_in(exe: &Path, prefix: &str, lang: Lang) -> [Verb; 2] {
    let exe = exe.display();
    let [select, compare] = labels(lang);
    [
        Verb {
            key: format!("{prefix}.SelectLeft"),
            label: select.into(),
            command: format!("\"{exe}\" --select-left \"%1\""),
        },
        Verb {
            key: format!("{prefix}.CompareTo"),
            label: compare.into(),
            command: format!("\"{exe}\" --compare-to \"%1\""),
        },
    ]
}

/// `{classes}\{root}\shell\{key}`
pub fn key_path(classes: &str, root: &str, verb: &Verb) -> String {
    format!(r"{classes}\{root}\shell\{}", verb.key)
}

/// Writes the verbs under the current user's `Software\Classes` (no administrator rights).
#[cfg(windows)]
pub fn register(exe: &Path, prefix: &str) -> io::Result<()> {
    register_under(windows_registry::CURRENT_USER, CLASSES_PATH, exe, prefix)
}

/// Removes them again; verbs that are not there are not an error.
#[cfg(windows)]
pub fn unregister(prefix: &str) -> io::Result<()> {
    unregister_under(windows_registry::CURRENT_USER, CLASSES_PATH, prefix)
}

#[cfg(not(windows))]
pub fn register(_exe: &Path, _prefix: &str) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

#[cfg(not(windows))]
pub fn unregister(_prefix: &str) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// `register` with the registry root and the `Software\Classes` path as parameters, so a test can
/// write under a throwaway key instead of the real verbs.
#[cfg(windows)]
pub fn register_under(
    root: &windows_registry::Key,
    classes: &str,
    exe: &Path,
    prefix: &str,
) -> io::Result<()> {
    let icon = format!("\"{}\",0", exe.display());
    for base in ROOTS {
        for verb in verbs(exe, prefix) {
            let key = root
                .create(key_path(classes, base, &verb))
                .map_err(io::Error::from)?;
            key.set_string("", &verb.label).map_err(io::Error::from)?;
            key.set_string("Icon", &icon).map_err(io::Error::from)?;
            key.set_string("MultiSelectModel", "Single")
                .map_err(io::Error::from)?;
            let command = key.create("command").map_err(io::Error::from)?;
            command
                .set_string("", &verb.command)
                .map_err(io::Error::from)?;
        }
    }
    Ok(())
}

/// `unregister` with the root and path as parameters (see `register_under`).
#[cfg(windows)]
pub fn unregister_under(
    root: &windows_registry::Key,
    classes: &str,
    prefix: &str,
) -> io::Result<()> {
    // The exe path does not matter for the key names.
    for base in ROOTS {
        for verb in verbs(Path::new(""), prefix) {
            let path = key_path(classes, base, &verb);
            if root.open(&path).is_err() {
                continue;
            }
            root.remove_tree(&path).map_err(io::Error::from)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = r"C:\Users\ชาญ ชล\Programs\duet\duet.exe";

    #[test]
    fn the_commands_quote_the_exe_and_the_placeholder() {
        let [select, compare] = verbs(Path::new(EXE), "duet");
        assert_eq!(select.key, "duet.SelectLeft");
        assert_eq!(compare.key, "duet.CompareTo");
        assert_eq!(select.label, "Select as left");
        assert_eq!(compare.label, "Compare to selected left");
        assert_eq!(
            select.command,
            r#""C:\Users\ชาญ ชล\Programs\duet\duet.exe" --select-left "%1""#
        );
        assert_eq!(
            compare.command,
            r#""C:\Users\ชาญ ชล\Programs\duet\duet.exe" --compare-to "%1""#
        );
    }

    #[test]
    fn the_labels_are_chosen_by_language_and_match_the_installer() {
        assert_eq!(
            labels(Lang::En),
            ["Select as left", "Compare to selected left"]
        );
        assert_eq!(labels(Lang::Th), ["เลือกเป็นฝั่งซ้าย", "เทียบกับฝั่งซ้ายที่เลือกไว้"]);
        let [select, compare] = verbs_in(Path::new(EXE), "duet", Lang::Th);
        assert_eq!(select.label, "เลือกเป็นฝั่งซ้าย");
        assert_eq!(compare.label, "เทียบกับฝั่งซ้ายที่เลือกไว้");
        // Only the label moves; the key and the command are the same in both languages.
        let [english, _] = verbs_in(Path::new(EXE), "duet", Lang::En);
        assert_eq!(
            (&select.key, &select.command),
            (&english.key, &english.command)
        );
        let iss = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../installer/duet.iss"),
        )
        .unwrap();
        for (lang, [first, second]) in [Lang::En, Lang::Th].map(|lang| (lang, labels(lang))) {
            let code = lang.code();
            assert!(
                iss.contains(&format!("{code}.VerbSelectLeft={first}")),
                "{code}"
            );
            assert!(
                iss.contains(&format!("{code}.VerbCompareTo={second}")),
                "{code}"
            );
        }
    }

    #[test]
    fn a_test_prefix_changes_every_key() {
        let real = verbs(Path::new(EXE), VERB_PREFIX);
        let test = verbs(Path::new(EXE), "duet-installtest");
        for (a, b) in real.iter().zip(&test) {
            assert_ne!(a.key, b.key);
            assert_ne!(
                key_path(CLASSES_PATH, "*", a),
                key_path(CLASSES_PATH, "*", b)
            );
        }
    }

    #[test]
    fn the_key_path_sits_under_the_shell_key_of_each_root() {
        let [select, _] = verbs(Path::new(EXE), "duet");
        assert_eq!(
            key_path(CLASSES_PATH, "Directory", &select),
            r"Software\Classes\Directory\shell\duet.SelectLeft"
        );
    }

    /// Deletes the throwaway key even when an assert fails.
    #[cfg(windows)]
    struct Scratch(String);

    #[cfg(windows)]
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = windows_registry::CURRENT_USER.remove_tree(&self.0);
        }
    }

    /// Writes under `HKCU\Software\duet-test-<pid>-<nanos>`, never under the real
    /// `Software\Classes`, and deletes that key afterwards.
    #[cfg(windows)]
    #[test]
    fn register_then_unregister_leaves_no_keys() {
        use windows_registry::CURRENT_USER;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let classes = format!(r"Software\duet-test-{}-{nanos}", std::process::id());
        let _scratch = Scratch(classes.clone());
        let exe = Path::new(EXE);
        register_under(CURRENT_USER, &classes, exe, VERB_PREFIX).unwrap();
        // Another program's verbs under the same roots must survive.
        register_under(CURRENT_USER, &classes, exe, "duet-sibling").unwrap();
        for base in ROOTS {
            for verb in verbs(exe, VERB_PREFIX) {
                let key = CURRENT_USER
                    .open(key_path(&classes, base, &verb))
                    .unwrap_or_else(|e| panic!("{base} {}: {e}", verb.key));
                assert_eq!(key.get_string("").unwrap(), verb.label);
                assert_eq!(key.get_string("MultiSelectModel").unwrap(), "Single");
                assert_eq!(key.get_string("Icon").unwrap(), format!("\"{EXE}\",0"));
                let command = key.open("command").unwrap();
                assert_eq!(command.get_string("").unwrap(), verb.command);
            }
        }
        unregister_under(CURRENT_USER, &classes, VERB_PREFIX).unwrap();
        for base in ROOTS {
            for verb in verbs(exe, VERB_PREFIX) {
                assert!(
                    CURRENT_USER.open(key_path(&classes, base, &verb)).is_err(),
                    "{base} {} is still there",
                    verb.key
                );
            }
        }
        for base in ROOTS {
            for verb in verbs(exe, "duet-sibling") {
                let key = CURRENT_USER
                    .open(key_path(&classes, base, &verb))
                    .unwrap_or_else(|e| panic!("{base} {} was deleted: {e}", verb.key));
                assert!(key.open("command").is_ok(), "{base} {}", verb.key);
            }
        }
        unregister_under(CURRENT_USER, &classes, VERB_PREFIX).unwrap();
    }

    /// The installer writes the same command lines by hand, so the flags are compared as text.
    #[test]
    fn the_installer_script_holds_the_same_flags_as_the_verbs() {
        let iss = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../installer/duet.iss"),
        )
        .unwrap();
        for verb in verbs(Path::new(EXE), VERB_PREFIX) {
            let flag = verb
                .command
                .strip_suffix(" \"%1\"")
                .and_then(|rest| rest.rsplit(' ').next())
                .unwrap();
            assert!(flag.starts_with("--"), "{flag}");
            let line = format!("\"\"{{app}}\\{{#AppExe}}\"\" {flag} \"\"%1\"\"");
            assert_eq!(
                iss.matches(&line).count(),
                ROOTS.len(),
                "duet.iss needs `{line}` once for each of {ROOTS:?}"
            );
        }
    }
}
