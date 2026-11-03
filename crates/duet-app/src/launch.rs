//! What the command line asks for: the start screen, two folders, two text files, two
//! binary files, two tables, two pictures or three files to merge.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use duet_core::kind::{CompareKind, detect};

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
    Folders { left: PathBuf, right: PathBuf },
    Files { left: PathBuf, right: PathBuf },
    Hex { left: PathBuf, right: PathBuf },
    Table { left: PathBuf, right: PathBuf },
    Picture { left: PathBuf, right: PathBuf },
    Merge(MergePaths),
    Invalid(String),
}

/// Decides the view for two paths, in this order (`kind::detect`): two folders compare as
/// folders; two files that both have picture extensions open as pictures without being read;
/// other files are text, or hex when either one starts with a NUL (this reads the start of both,
/// so not on the UI thread). A file beside a folder, or a path that cannot be read, gives an
/// explanation. A table is never detected: it is always asked for.
pub fn classify(left: &Path, right: &Path) -> Launch {
    // Errors name the paths as typed; only a success is made absolute.
    let kind = match detect(left, right) {
        Ok(kind) => kind,
        Err(message) => return Launch::Invalid(message),
    };
    let (left, right) = (absolute(left), absolute(right));
    match kind {
        CompareKind::Folder => Launch::Folders { left, right },
        CompareKind::Text => Launch::Files { left, right },
        CompareKind::Hex => Launch::Hex { left, right },
        CompareKind::Table => Launch::Table { left, right },
        CompareKind::Picture => Launch::Picture { left, right },
        CompareKind::Merge => Launch::Invalid("A 3-way merge needs three files.".into()),
    }
}

const USAGE: &str = "Usage: duet [--table] [LEFT RIGHT] | duet LEFT RIGHT BASE [OUTPUT]";

pub fn from_args(args: &[OsString]) -> Launch {
    match args {
        [] => Launch::Start,
        // `--table` and one path is not a pair, nor a "--table" file.
        [flag, _] if flag == "--table" => Launch::Invalid(USAGE.into()),
        [left, right] => classify(Path::new(left), Path::new(right)),
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

/// `duet L R BASE [OUT]`: three files to merge, in the command-line order. Errors name a path
/// as typed; the output may not exist yet but cannot be a folder.
fn classify_merge(left: &Path, right: &Path, base: &Path, output: Option<&Path>) -> Launch {
    for path in [left, right, base] {
        match std::fs::metadata(path) {
            Ok(meta) if meta.is_dir() => {
                return Launch::Invalid("A 3-way merge needs three files.".into());
            }
            Ok(_) => {}
            Err(err) => return Launch::Invalid(format!("{}: {err}", path.display())),
        }
    }
    if output.is_some_and(Path::is_dir) {
        return Launch::Invalid("The output must be a file, not a folder.".into());
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
        Ok(CompareKind::Folder) => Launch::Invalid("--table needs two files.".into()),
        Ok(_) => Launch::Table {
            left: absolute(left),
            right: absolute(right),
        },
        Err(message) => Launch::Invalid(message),
    }
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
