use std::fs::{self, File};
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, UNIX_EPOCH};

fn cli(args: &[&Path], flags: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_duet-cli"))
        .args(args)
        .args(flags)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn put(path: &Path, body: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
    let fixed = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(fixed)
        .unwrap();
}

#[test]
fn identical_files_exit_zero() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    put(&a, b"same\n");
    put(&b, b"same\n");
    assert_eq!(cli(&[&a, &b], &[]).status.code(), Some(0));
}

#[test]
fn changed_line_prints_hunk_and_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    put(&a, b"a\nb\nc\n");
    put(&b, b"a\nX\nc\n");
    let out = cli(&[&a, &b], &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stdout(&out).contains("@@ -2,1 +2,1 @@\n-b\n+X\n"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn json_output_for_text() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    put(&a, b"a\nb\n");
    put(&b, b"a\nc\n");
    let out = cli(&[&a, &b], &["--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["kind"], "text");
    assert_eq!(v["identical"], false);
    assert_eq!(v["hunks"][0]["left"], serde_json::json!([1, 2]));
    assert_eq!(v["left_encoding"], "UTF-8");
}

#[test]
fn thai_lines_reach_stdout_intact() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    put(&a, "สวัสดี\n".as_bytes());
    put(&b, "ลาก่อน\n".as_bytes());
    let text = stdout(&cli(&[&a, &b], &[]));
    assert!(text.contains("-สวัสดี") && text.contains("+ลาก่อน"), "{text}");
}

#[test]
fn ignore_options_are_applied() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    put(&a, b"  Hello\r\n");
    put(&b, b"hello\n");
    let flags = ["--ignore-ws", "edges", "--ignore-case", "--ignore-eol"];
    assert_eq!(cli(&[&a, &b], &flags).status.code(), Some(0));
}

#[test]
fn folder_compare_lists_orphans() {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("only.txt"), b"x");
    let out = cli(&[l.path(), r.path()], &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("<\tonly.txt\n"), "{}", stdout(&out));
}

#[test]
fn identical_folders_exit_zero() {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("a/b.txt"), b"x");
    put(&r.path().join("a/b.txt"), b"x");
    assert_eq!(cli(&[l.path(), r.path()], &[]).status.code(), Some(0));
}

#[test]
fn content_flag_catches_same_size_change() {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("f.bin"), b"aaaa");
    put(&r.path().join("f.bin"), b"aaab");
    assert_eq!(cli(&[l.path(), r.path()], &[]).status.code(), Some(0));
    assert_eq!(
        cli(&[l.path(), r.path()], &["--content"]).status.code(),
        Some(1)
    );
}

#[test]
fn folder_json_output() {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&r.path().join("new.txt"), b"x");
    let out = cli(&[l.path(), r.path()], &["--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["kind"], "folder");
    assert_eq!(v["nodes"][0]["rel"], "new.txt");
    assert_eq!(v["nodes"][0]["status"], "right_only");
}

#[test]
fn missing_path_exits_two() {
    let dir = tempfile::tempdir().unwrap();
    let out = cli(&[&dir.path().join("nope"), dir.path()], &[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(!out.stderr.is_empty());
}

#[test]
fn file_versus_folder_exits_two() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("f.txt");
    put(&file, b"x");
    assert_eq!(cli(&[&file, dir.path()], &[]).status.code(), Some(2));
}

#[test]
fn binary_files_are_compared_bytewise() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.bin"), dir.path().join("b.bin"));
    put(&a, b"a\0b");
    put(&b, b"a\0c");
    let out = cli(&[&a, &b], &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("binary files differ"));
}

#[test]
fn timings_go_to_stderr_and_quiet_shortens_output() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    put(&a, b"a\n");
    put(&b, b"b\n");
    let out = cli(&[&a, &b], &["--timings", "--quiet"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("timing: total_ms="));
    assert_eq!(stdout(&out), "different: 1 hunk(s)\n");
}

fn text_pair(
    left: &[u8],
    right: &[u8],
) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    put(&a, left);
    put(&b, right);
    (dir, a, b)
}

#[test]
fn undecodable_bytes_that_differ_are_not_identical() {
    // "café" vs "cafè" written as Latin-1 bytes inside a UTF-8 BOM file: both decode to "caf\u{FFFD}".
    let (_dir, a, b) = text_pair(b"\xEF\xBB\xBFcaf\xE9\n", b"\xEF\xBB\xBFcaf\xE8\n");
    let out = cli(&[&a, &b], &[]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    let v: serde_json::Value = serde_json::from_slice(&cli(&[&a, &b], &["--json"]).stdout).unwrap();
    assert_eq!(v["identical"], false);
    assert_eq!(v["left_lossy"], true);
}

#[test]
fn utf16_unpaired_surrogates_that_differ_are_not_identical() {
    let (_dir, a, b) = text_pair(b"\xFF\xFE\x00\xD8\x41\x00", b"\xFF\xFE\x01\xD8\x41\x00");
    assert_eq!(cli(&[&a, &b], &[]).status.code(), Some(1));
}

#[test]
fn bom_versus_no_bom_is_reported_and_exits_one() {
    let (_dir, a, b) = text_pair(b"\xEF\xBB\xBFsame\n", b"same\n");
    let out = cli(&[&a, &b], &["--quiet"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("bytes differ"), "{}", stdout(&out));
    let v: serde_json::Value = serde_json::from_slice(&cli(&[&a, &b], &["--json"]).stdout).unwrap();
    assert_eq!(
        (v["left_bom"].clone(), v["right_bom"].clone()),
        (true.into(), false.into())
    );
}

#[test]
fn identical_text_with_ignore_options_still_exits_zero() {
    let (_dir, a, b) = text_pair(b"same\r\n", b"same\n");
    assert_eq!(cli(&[&a, &b], &["--ignore-eol"]).status.code(), Some(0));
}

#[test]
fn line_ending_only_hunks_show_the_line_endings() {
    let (_dir, a, b) = text_pair(b"a\r\nb\r\n", b"a\nb\n");
    let text = stdout(&cli(&[&a, &b], &[]));
    assert!(text.contains("line endings only"), "{text}");
    // The terminators are spelled out, never written raw.
    assert!(text.contains(r"-a\r\n") && text.contains(r"+a\n"), "{text}");
    assert!(!text.contains('\r'), "{text:?}");
}

#[test]
fn missing_final_newline_is_visible() {
    let (_dir, a, b) = text_pair(b"a\nb", b"a\nb\n");
    let text = stdout(&cli(&[&a, &b], &[]));
    assert!(
        text.contains("-b(no newline)") && text.contains(r"+b\n"),
        "{text}"
    );
}

#[test]
fn content_changes_print_plain_lines() {
    let (_dir, a, b) = text_pair(b"a\r\n", b"b\r\n");
    let text = stdout(&cli(&[&a, &b], &[]));
    assert_eq!(text, "@@ -1,1 +1,1 @@\n-a\n+b\n");
}

#[test]
fn help_explains_exit_codes_and_row_symbols() {
    let out = Command::new(env!("CARGO_BIN_EXE_duet-cli"))
        .arg("--help")
        .output()
        .unwrap();
    let help = stdout(&out);
    for needle in [
        "Exit codes",
        "0 = identical",
        "1 = different",
        "2 = error",
        "! different",
        "< left only",
        "> right only",
        "--ignore-eol",
        "Ignore line-ending",
    ] {
        assert!(help.contains(needle), "missing {needle:?} in:\n{help}");
    }
}

#[test]
fn dst_shift_and_tolerance_flags() {
    let (l, r) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    put(&l.path().join("a.txt"), b"same");
    put(&r.path().join("a.txt"), b"same");
    let shifted = UNIX_EPOCH + Duration::from_secs(1_700_000_000 + 3_600);
    File::options()
        .write(true)
        .open(r.path().join("a.txt"))
        .unwrap()
        .set_modified(shifted)
        .unwrap();
    assert_eq!(cli(&[l.path(), r.path()], &[]).status.code(), Some(1));
    assert_eq!(
        cli(&[l.path(), r.path()], &["--ignore-dst"]).status.code(),
        Some(0)
    );
    assert_eq!(
        cli(&[l.path(), r.path()], &["--time-tolerance", "4000"])
            .status
            .code(),
        Some(0)
    );
}
