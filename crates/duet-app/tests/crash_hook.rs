//! The panic hook is process-global, so this binary holds exactly one test.

#[test]
fn a_panic_leaves_a_crash_report() {
    let dir = tempfile::tempdir().unwrap();
    duet_app::crash::install_panic_hook(dir.path().to_path_buf());
    let result = std::panic::catch_unwind(|| panic!("duet test panic"));
    assert!(result.is_err());
    let reports: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect();
    assert_eq!(reports.len(), 1);
    assert!(
        reports[0].contains("message: duet test panic"),
        "{}",
        reports[0]
    );
}
