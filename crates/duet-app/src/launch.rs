//! What the command line asks for: the start screen, two folders, two text files, two
//! binary files, two tables, two pictures or three files to merge.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use duet_core::kind::{CompareKind, detect};

use crate::i18n::{tr, tr_str, trf};

/// The files of a 3-way merge; a slot with no file is None. The output is where the result goes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MergePaths {
    pub left: Option<PathBuf>,
    pub base: Option<PathBuf>,
    pub right: Option<PathBuf>,
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    Start,
    Folders {
        left: PathBuf,
        right: PathBuf,
    },
    Files {
        left: PathBuf,
        right: PathBuf,
    },
    Hex {
        left: PathBuf,
        right: PathBuf,
    },
    Table {
        left: PathBuf,
        right: PathBuf,
    },
    Picture {
        left: PathBuf,
        right: PathBuf,
    },
    Merge(MergePaths),
    /// The Start tab with the left box filled, from `--compare-to` with nothing (usable) stored;
    /// the notice says why, when there is something to say.
    StartWithLeft {
        left: PathBuf,
        notice: Option<String>,
    },
    Invalid(String),
}

/// What the command line asks the process to do. Only `Open` starts the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Open(Launch),
    SelectLeft(PathBuf),
    RegisterShell,
    UnregisterShell,
}

/// Decides the view for two paths, in this order (`kind::detect`): two folders compare as
/// folders; two files that both have picture extensions open as pictures without being read;
/// two workbooks (`.xlsx`, `.xlsm`), or a workbook and a `.csv`, `.tsv` or `.tab`, open as a
/// table when the workbook starts as one does; other files are text, or hex when either one starts
/// with a NUL (this reads the start of both, so not on the UI thread). A file beside a folder, or a
/// path that
/// cannot be read, gives an explanation. A table of CSV files is never detected: it is asked
/// for.
pub fn classify(left: &Path, right: &Path) -> Launch {
    // Errors name the paths as typed; only a success is made absolute.
    let kind = match detect(left, right) {
        Ok(kind) => kind,
        Err(message) => return Launch::Invalid(tr_str(&message).into_owned()),
    };
    let (left, right) = (absolute(left), absolute(right));
    match kind {
        CompareKind::Folder => Launch::Folders { left, right },
        CompareKind::Text => Launch::Files { left, right },
        CompareKind::Hex => Launch::Hex { left, right },
        CompareKind::Table => Launch::Table { left, right },
        CompareKind::Picture => Launch::Picture { left, right },
        CompareKind::Merge => Launch::Invalid(tr("A 3-way merge needs three files.").into()),
    }
}

const USAGE: &str = "Usage: duet [--table] [LEFT RIGHT] | duet LEFT RIGHT BASE [OUTPUT]";

const SHELL_USAGE: &str = "Usage: duet --select-left PATH | duet --compare-to PATH | duet --register-shell | duet --unregister-shell";

/// Parses the Explorer verbs before anything else; every other command line is `from_args`'s.
/// `data_dir` is where `--compare-to` finds the path `--select-left` stored.
pub fn command_from_args(args: &[OsString], data_dir: &Path) -> Command {
    // Known limit: a real file named `--compare-to` (or another verb flag) is taken as the verb.
    // `.\--compare-to` names the file and works.
    match args {
        [flag, path] if flag == "--select-left" => Command::SelectLeft(PathBuf::from(path)),
        [flag, path] if flag == "--compare-to" => {
            Command::Open(crate::shell::compare_to(Path::new(path), data_dir))
        }
        [flag] if flag == "--register-shell" => Command::RegisterShell,
        [flag] if flag == "--unregister-shell" => Command::UnregisterShell,
        [flag, ..]
            if [
                "--select-left",
                "--compare-to",
                "--register-shell",
                "--unregister-shell",
            ]
            .iter()
            .any(|f| flag == f) =>
        {
            Command::Open(Launch::Invalid(SHELL_USAGE.into()))
        }
        _ => Command::Open(from_args(args)),
    }
}

pub fn from_args(args: &[OsString]) -> Launch {
    match args {
        [] => Launch::Start,
        // `--table` and one path is not a pair, nor a "--table" file.
        [flag, _] if flag == "--table" => Launch::Invalid(USAGE.into()),
        [left, right] => match (null_to_empty(left), null_to_empty(right)) {
            (Ok(left), Ok(right)) => classify(&left, &right),
            (Err(message), _) | (_, Err(message)) => Launch::Invalid(message),
        },
        [flag, left, right] if flag == "--table" => {
            classify_table(Path::new(left), Path::new(right))
        }
        [flag, ..] if flag == "--table" => Launch::Invalid(USAGE.into()),
        [left, right, base] => classify_merge(left.as_ref(), right.as_ref(), base.as_ref(), None),
        [left, right, base, output] => classify_merge(
            left.as_ref(),
            right.as_ref(),
            base.as_ref(),
            Some(output.as_ref()),
        ),
        _ => Launch::Invalid(USAGE.into()),
    }
}

/// A side that is the null device (`nul`, or `/dev/null` as git passes it for an added or deleted
/// file) is an empty file instead; any other path is returned as typed.
fn null_to_empty(arg: &OsString) -> Result<PathBuf, String> {
    if is_null_device(arg) {
        empty_side().map_err(|err| {
            trf(
                "Cannot create the empty file for {}: {}",
                &[&arg.to_string_lossy(), &err],
            )
        })
    } else {
        Ok(PathBuf::from(arg))
    }
}

/// `nul` or `/dev/null`, without regard to case. Only the text decides, never the file system.
fn is_null_device(path: &std::ffi::OsStr) -> bool {
    let text = path.to_string_lossy();
    text.eq_ignore_ascii_case("nul") || text.eq_ignore_ascii_case("/dev/null")
}

/// An empty, read-only file called `no file` in the temp folder, made if missing. Its name is what
/// the tab shows for the side that does not exist. It is shared by every window, so an empty one
/// is left alone (rewriting it would change its modified time under another window), and the
/// read-only attribute makes duet treat the side as read-only, so nobody saves into it.
fn empty_side() -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join("duet-empty-side");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join("no file");
    match std::fs::metadata(&file) {
        Ok(meta) if meta.len() == 0 => {
            if !meta.permissions().readonly() {
                set_readonly(&file, true)?;
            }
            return Ok(file);
        }
        // Somehow it has content: clear the attribute so it can be emptied.
        Ok(_) => set_readonly(&file, false)?,
        Err(_) => {}
    }
    if let Err(err) = std::fs::write(&file, b"") {
        // Another window made it read-only between our check and our write: that is fine.
        if std::fs::metadata(&file)
            .is_ok_and(|meta| meta.len() == 0 && meta.permissions().readonly())
        {
            return Ok(file);
        }
        return Err(err);
    }
    set_readonly(&file, true)?;
    Ok(file)
}

fn set_readonly(file: &Path, readonly: bool) -> std::io::Result<()> {
    let mut permissions = std::fs::metadata(file)?.permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(readonly);
    std::fs::set_permissions(file, permissions)
}

/// `duet L R BASE [OUT]`: three files to merge, in the command-line order. Errors name a path
/// as typed; the output may not exist yet but cannot be a folder.
fn classify_merge(left: &Path, right: &Path, base: &Path, output: Option<&Path>) -> Launch {
    for path in [left, right, base] {
        match std::fs::metadata(path) {
            Ok(meta) if meta.is_dir() => {
                return Launch::Invalid(tr("A 3-way merge needs three files.").into());
            }
            Ok(_) => {}
            Err(err) => return Launch::Invalid(format!("{}: {err}", path.display())),
        }
    }
    if output.is_some_and(Path::is_dir) {
        return Launch::Invalid(tr("The output must be a file, not a folder.").into());
    }
    Launch::Merge(MergePaths {
        left: Some(absolute(left)),
        base: Some(absolute(base)),
        right: Some(absolute(right)),
        output: output.map(absolute),
    })
}

/// `duet --table L R`: two files of any content open as tables; folders are refused.
fn classify_table(left: &Path, right: &Path) -> Launch {
    match detect(left, right) {
        Ok(CompareKind::Folder) => Launch::Invalid(tr("--table needs two files.").into()),
        Ok(_) => Launch::Table {
            left: absolute(left),
            right: absolute(right),
        },
        Err(message) => Launch::Invalid(tr_str(&message).into_owned()),
    }
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::compare_to;

    #[test]
    fn no_arguments_open_the_start_screen() {
        assert_eq!(from_args(&[]), Launch::Start);
    }

    #[test]
    fn two_folders_and_two_files_pick_their_view() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        assert!(matches!(
            from_args(&[a.clone().into(), b.clone().into()]),
            Launch::Folders { .. }
        ));
        let (f, g) = (dir.path().join("f.txt"), dir.path().join("g.txt"));
        std::fs::write(&f, "1").unwrap();
        std::fs::write(&g, "2").unwrap();
        assert_eq!(
            from_args(&[f.clone().into(), g.clone().into()]),
            Launch::Files { left: f, right: g }
        );
    }

    #[test]
    fn launch_routes_a_binary_pair_to_hex() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.bin"), dir.path().join("b.bin"));
        std::fs::write(&a, [1u8, 0, 2]).unwrap();
        std::fs::write(&b, [1u8, 0, 3]).unwrap();
        assert_eq!(
            from_args(&[a.clone().into(), b.clone().into()]),
            Launch::Hex {
                left: a.clone(),
                right: b
            }
        );
        let t = dir.path().join("t.txt");
        std::fs::write(&t, "text").unwrap();
        assert!(
            matches!(from_args(&[a.into(), t.into()]), Launch::Hex { .. }),
            "one binary side is enough"
        );
        let (x, y) = (dir.path().join("x.txt"), dir.path().join("y.txt"));
        std::fs::write(&x, "1").unwrap();
        std::fs::write(&y, "2").unwrap();
        assert_eq!(
            from_args(&[x.clone().into(), y.clone().into()]),
            Launch::Files { left: x, right: y }
        );
    }

    #[test]
    fn a_missing_path_is_named_as_typed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "1").unwrap();
        let Launch::Invalid(message) = classify(Path::new("no-such-file.txt"), &file) else {
            panic!("a missing path is invalid");
        };
        assert!(message.starts_with("no-such-file.txt: "), "{message}");
    }

    #[test]
    fn mixed_missing_and_extra_arguments_explain_themselves() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "1").unwrap();
        let mixed = from_args(&[file.clone().into(), dir.path().into()]);
        assert_eq!(
            mixed,
            Launch::Invalid("Cannot compare a file with a folder.".into())
        );
        let missing = from_args(&[file.into(), dir.path().join("nope").into()]);
        assert!(matches!(missing, Launch::Invalid(m) if m.contains("nope")));
        assert!(matches!(from_args(&["a".into()]), Launch::Invalid(m) if m.starts_with("Usage")));
    }

    #[test]
    fn launch_routes_two_pictures_to_picture() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.png"), dir.path().join("b.png"));
        std::fs::write(&a, b"\x89PNG\0\0").unwrap();
        std::fs::write(&b, b"\x89PNG\0\x01").unwrap();
        let abs = |p: &Path| std::path::absolute(p).unwrap();
        assert_eq!(
            from_args(&[a.clone().into(), b.clone().into()]),
            Launch::Picture {
                left: abs(&a),
                right: abs(&b)
            }
        );
        assert_eq!(
            from_args(&["--table".into(), a.clone().into(), b.clone().into()]),
            Launch::Table {
                left: abs(&a),
                right: abs(&b)
            },
            "a forced kind wins over the extensions"
        );
        let t = dir.path().join("t.txt");
        std::fs::write(&t, "text").unwrap();
        assert!(
            matches!(from_args(&[a.into(), t.into()]), Launch::Hex { .. }),
            "a NUL picture beside a text file is still hex"
        );
    }

    #[test]
    fn text_under_a_workbook_name_opens_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let file = |name: &str, bytes: &[u8]| {
            let p = dir.path().join(name);
            std::fs::write(&p, bytes).unwrap();
            p
        };
        let a = file("export.xlsx", b"id,name\n1,Ann\n");
        let b = file("export2.xlsx", b"<html></html>\n");
        let c = file("empty.xlsx", b"");
        assert!(matches!(
            from_args(&[a.clone().into(), b.into()]),
            Launch::Files { .. }
        ));
        assert!(matches!(
            from_args(&[a.into(), c.into()]),
            Launch::Files { .. }
        ));
    }

    #[test]
    fn launch_routes_two_workbooks_and_a_workbook_with_csv_to_table() {
        let dir = tempfile::tempdir().unwrap();
        let file = |name: &str| {
            let p = dir.path().join(name);
            std::fs::write(&p, b"PK  ").unwrap();
            p
        };
        let (a, b, c, t) = (file("a.xlsx"), file("b.xlsx"), file("b.csv"), file("t.txt"));
        let abs = |p: &Path| std::path::absolute(p).unwrap();
        let table = |l: &Path, r: &Path| Launch::Table {
            left: abs(l),
            right: abs(r),
        };
        assert_eq!(
            from_args(&[a.clone().into(), b.clone().into()]),
            table(&a, &b)
        );
        assert_eq!(
            from_args(&[a.clone().into(), c.clone().into()]),
            table(&a, &c)
        );
        assert_eq!(
            from_args(&[c.clone().into(), a.clone().into()]),
            table(&c, &a)
        );
        assert!(matches!(
            from_args(&[a.clone().into(), t.clone().into()]),
            Launch::Hex { .. }
        ));
        assert_eq!(
            from_args(&["--table".into(), a.clone().into(), b.clone().into()]),
            table(&a, &b)
        );
        // Start's check and a folder row's Enter both go through `classify` with no forced kind.
        assert_eq!(classify(&a, &b), table(&a, &b));
    }

    #[test]
    fn launch_takes_a_table_flag() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.csv"), dir.path().join("b.csv"));
        std::fs::write(&a, "id,v\n1,a\n").unwrap();
        std::fs::write(&b, "id,v\n1,b\n").unwrap();
        assert_eq!(
            from_args(&["--table".into(), a.clone().into(), b.clone().into()]),
            Launch::Table {
                left: a.clone(),
                right: b.clone()
            }
        );
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        assert_eq!(
            from_args(&["--table".into(), sub.clone().into(), sub.clone().into()]),
            Launch::Invalid("--table needs two files.".into())
        );
        assert_eq!(
            from_args(&["--table".into(), a.clone().into(), sub.into()]),
            Launch::Invalid("Cannot compare a file with a folder.".into())
        );
        assert_eq!(
            from_args(&[a.clone().into(), b.clone().into()]),
            Launch::Files { left: a, right: b },
            "without the flag a CSV pair stays text"
        );
        assert_eq!(
            from_args(&["--table".into()]),
            Launch::Invalid(USAGE.into())
        );
        assert_eq!(
            from_args(&["--table".into(), dir.path().join("a.csv").into()]),
            Launch::Invalid(USAGE.into()),
            "one path after the flag is not a pair"
        );
    }

    #[test]
    fn launch_takes_three_or_four_paths_as_a_merge() {
        let dir = tempfile::tempdir().unwrap();
        let abs = |p: &Path| std::path::absolute(p).unwrap();
        let (l, r, b) = (
            dir.path().join("l.txt"),
            dir.path().join("r.txt"),
            dir.path().join("b.txt"),
        );
        for file in [&l, &r, &b] {
            std::fs::write(file, "x\n").unwrap();
        }
        let args = |paths: &[&PathBuf]| -> Vec<OsString> {
            paths.iter().map(|p| p.as_os_str().to_owned()).collect()
        };
        assert_eq!(
            from_args(&args(&[&l, &r, &b])),
            Launch::Merge(MergePaths {
                left: Some(abs(&l)),
                base: Some(abs(&b)),
                right: Some(abs(&r)),
                output: None,
            }),
            "the command line orders them left, right, base"
        );
        let out = dir.path().join("out.txt");
        assert_eq!(
            from_args(&args(&[&l, &r, &b, &out])),
            Launch::Merge(MergePaths {
                left: Some(abs(&l)),
                base: Some(abs(&b)),
                right: Some(abs(&r)),
                output: Some(abs(&out)),
            }),
            "an output that does not exist yet is fine"
        );

        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        for folder_at in 0..3 {
            let mut three = [&l, &r, &b];
            three[folder_at] = &sub;
            assert_eq!(
                from_args(&args(&three)),
                Launch::Invalid("A 3-way merge needs three files.".into()),
                "a folder in slot {folder_at}"
            );
        }
        let nope = dir.path().join("nope.txt");
        let Launch::Invalid(message) = from_args(&args(&[&l, &r, &nope])) else {
            panic!("a missing file is invalid");
        };
        assert!(
            message.starts_with(&format!("{}: ", nope.display())),
            "{message}"
        );
        assert_eq!(
            from_args(&args(&[&l, &r, &b, &sub])),
            Launch::Invalid("The output must be a file, not a folder.".into())
        );

        let table = [
            "--table".into(),
            l.clone().into(),
            r.clone().into(),
            b.clone().into(),
        ];
        assert_eq!(from_args(&table), Launch::Invalid(USAGE.into()));
        let five = args(&[&l, &r, &b, &out, &out]);
        assert_eq!(from_args(&five), Launch::Invalid(USAGE.into()));
        assert_eq!(
            USAGE,
            "Usage: duet [--table] [LEFT RIGHT] | duet LEFT RIGHT BASE [OUTPUT]"
        );
    }

    fn os(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    fn abs(p: &Path) -> PathBuf {
        std::path::absolute(p).unwrap()
    }

    #[test]
    fn select_left_and_the_shell_flags_are_commands() {
        let data = tempfile::tempdir().unwrap();
        assert_eq!(
            command_from_args(&os(&["--select-left", "a.txt"]), data.path()),
            Command::SelectLeft(PathBuf::from("a.txt"))
        );
        assert_eq!(
            command_from_args(&os(&["--register-shell"]), data.path()),
            Command::RegisterShell
        );
        assert_eq!(
            command_from_args(&os(&["--unregister-shell"]), data.path()),
            Command::UnregisterShell
        );
        let file = data.path().join("f.txt");
        std::fs::write(&file, "x").unwrap();
        let other = tempfile::tempdir().unwrap();
        let args = [OsString::from("--compare-to"), file.clone().into()];
        assert_eq!(
            command_from_args(&args, other.path()),
            Command::Open(Launch::StartWithLeft {
                left: abs(&file),
                notice: None
            })
        );
    }

    #[test]
    fn a_shell_flag_with_the_wrong_arguments_is_a_usage_error() {
        let data = tempfile::tempdir().unwrap();
        for args in [
            &["--select-left"][..],
            &["--compare-to"],
            &["--compare-to", "a", "b"],
            &["--select-left", "a", "b"],
            &["--register-shell", "x"],
            &["--unregister-shell", "x"],
        ] {
            let Command::Open(Launch::Invalid(m)) = command_from_args(&os(args), data.path())
            else {
                panic!("{args:?} should be a usage error");
            };
            assert!(m.starts_with("Usage: duet --select-left"), "{m}");
        }
    }

    #[test]
    fn compare_to_with_nothing_stored_opens_start_with_this_path_as_left() {
        let data = tempfile::tempdir().unwrap();
        let p = Path::new("some").join("where.txt");
        assert_eq!(
            compare_to(&p, data.path()),
            Launch::StartWithLeft {
                left: abs(&p),
                notice: None
            }
        );
    }

    #[test]
    fn compare_to_with_a_vanished_left_names_it() {
        let data = tempfile::tempdir().unwrap();
        let gone = data.path().join("gone ไทย.txt");
        std::fs::write(&gone, "x").unwrap();
        crate::shell::store_left(data.path(), &gone).unwrap();
        std::fs::remove_file(&gone).unwrap();
        let right = data.path().join("r.txt");
        std::fs::write(&right, "y").unwrap();
        let Launch::StartWithLeft { left, notice } = compare_to(&right, data.path()) else {
            panic!("a vanished left opens the start tab");
        };
        assert_eq!(left, abs(&right));
        let notice = notice.expect("a notice");
        assert!(notice.contains(&gone.display().to_string()), "{notice}");
        assert!(notice.ends_with("no longer exists"), "{notice}");
    }

    #[test]
    fn compare_to_a_file_against_a_folder_uses_the_existing_message() {
        let data = tempfile::tempdir().unwrap();
        let file = data.path().join("f.txt");
        std::fs::write(&file, "x").unwrap();
        crate::shell::store_left(data.path(), &file).unwrap();
        assert_eq!(
            compare_to(data.path(), data.path()),
            Launch::Invalid("Cannot compare a file with a folder.".into())
        );
    }

    #[test]
    fn compare_to_clears_the_stored_path_even_when_invalid() {
        let data = tempfile::tempdir().unwrap();
        let file = data.path().join("f.txt");
        std::fs::write(&file, "x").unwrap();
        crate::shell::store_left(data.path(), &file).unwrap();
        assert!(matches!(
            compare_to(data.path(), data.path()),
            Launch::Invalid(_)
        ));
        assert_eq!(crate::shell::take_left(data.path()), None);
    }

    #[test]
    fn compare_to_self_opens_an_identical_compare() {
        let data = tempfile::tempdir().unwrap();
        let file = data.path().join("f.txt");
        std::fs::write(&file, "x").unwrap();
        crate::shell::store_left(data.path(), &file).unwrap();
        assert_eq!(
            compare_to(&file, data.path()),
            Launch::Files {
                left: abs(&file),
                right: abs(&file)
            }
        );
    }

    #[test]
    fn compare_to_after_select_compares_the_two() {
        let data = tempfile::tempdir().unwrap();
        let (a, b) = (data.path().join("a.txt"), data.path().join("b.txt"));
        std::fs::write(&a, "1").unwrap();
        std::fs::write(&b, "2").unwrap();
        crate::shell::store_left(data.path(), &a).unwrap();
        assert_eq!(
            compare_to(&b, data.path()),
            Launch::Files {
                left: abs(&a),
                right: abs(&b)
            }
        );
    }

    fn assert_empty_side(path: &Path) {
        assert_eq!(path.file_name().unwrap(), "no file");
        assert_eq!(std::fs::metadata(path).unwrap().len(), 0, "{path:?}");
    }

    #[test]
    fn a_null_device_as_one_side_is_an_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("real.txt");
        std::fs::write(&file, "text\n").unwrap();
        for null in ["nul", "NUL", "Nul", "/dev/null", "/DEV/NULL"] {
            let Launch::Files { left, right } = from_args(&os(&[null, file.to_str().unwrap()]))
            else {
                panic!("{null} on the left should open a text compare");
            };
            assert_empty_side(&left);
            assert_eq!(right, file);
            let Launch::Files { left, right } = from_args(&[file.clone().into(), null.into()])
            else {
                panic!("{null} on the right should open a text compare");
            };
            assert_eq!(left, file);
            assert_empty_side(&right);
        }
    }

    #[test]
    fn two_null_sides_compare_two_empty_files() {
        let Launch::Files { left, right } = from_args(&os(&["nul", "/dev/null"])) else {
            panic!("two null sides should open a text compare");
        };
        assert_empty_side(&left);
        assert_empty_side(&right);
    }

    #[test]
    fn a_null_device_against_a_folder_keeps_the_existing_message() {
        let dir = tempfile::tempdir().unwrap();
        for args in [
            [OsString::from("nul"), dir.path().into()],
            [dir.path().into(), OsString::from("nul")],
        ] {
            assert_eq!(
                from_args(&args),
                Launch::Invalid("Cannot compare a file with a folder.".into())
            );
        }
    }

    #[test]
    fn only_nul_and_dev_null_are_null_devices() {
        use std::ffi::OsStr;
        for yes in ["nul", "NUL", "Nul", "/dev/null", "/DEV/NULL"] {
            assert!(is_null_device(OsStr::new(yes)), "{yes}");
        }
        // `nul.txt` is a device name before Windows 11, so the check never asks the file system.
        for no in ["nul.txt", "annul", "dev/null", "", "nul ", "/dev/null/x"] {
            assert!(!is_null_device(OsStr::new(no)), "{no}");
        }
    }

    #[test]
    fn the_shared_empty_side_is_read_only_and_left_alone_by_a_second_launch() {
        let first = empty_side().unwrap();
        assert_empty_side(&first);
        assert!(std::fs::metadata(&first).unwrap().permissions().readonly());
        let before = std::fs::metadata(&first).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        let second = empty_side().unwrap();
        assert_eq!(first, second);
        let after = std::fs::metadata(&second).unwrap().modified().unwrap();
        assert_eq!(before, after, "a second launch must not rewrite the file");
        assert!(std::fs::metadata(&second).unwrap().permissions().readonly());
    }

    #[test]
    fn existing_arguments_still_parse() {
        let data = tempfile::tempdir().unwrap();
        let (a, b) = (data.path().join("a.txt"), data.path().join("b.txt"));
        std::fs::write(&a, "1").unwrap();
        std::fs::write(&b, "2").unwrap();
        let args = [OsString::from(a), OsString::from(b)];
        assert_eq!(
            command_from_args(&args, data.path()),
            Command::Open(from_args(&args))
        );
        assert_eq!(
            command_from_args(&[], data.path()),
            Command::Open(Launch::Start)
        );
    }
}
