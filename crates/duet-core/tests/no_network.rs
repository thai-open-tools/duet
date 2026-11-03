//! Lock 1 of the offline guarantee: duet source must never install an HTTP client into gpui.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().is_some_and(|n| n == "target") {
            continue;
        }
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn duet_source_never_installs_an_http_client() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    // Built by concatenation so this file does not match itself.
    let forbidden = [
        ["with_", "http_client"].concat(),
        ["set_", "http_client"].concat(),
    ];
    let mut files = Vec::new();
    rust_files(&crates, &mut files);
    assert!(
        !files.is_empty(),
        "no sources found under {}",
        crates.display()
    );
    let mut hits = Vec::new();
    for file in &files {
        let source = fs::read_to_string(file).unwrap_or_default();
        for pattern in &forbidden {
            if source.contains(pattern.as_str()) {
                hits.push(format!("{}: {pattern}", file.display()));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "network client installation found: {hits:?}"
    );
}
