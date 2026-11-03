//! The status bar's warnings about the two files as they are (R17, LocalDiff `updateStat`):
//! line endings, encodings, the final newline and a CRLF script.

use duet_core::text::document::TextDocument;
use duet_core::text::lines::EolStyle;

const CRLF_SCRIPT: &str = "A CRLF script won't run on Linux: pick LF under On save";
const SCRIPT_EXTENSIONS: [&str; 6] = [".sh", ".ksh", ".bash", ".zsh", ".py", ".pl"];

/// The one line-ending style a side has; None for no breaks or a mix.
fn style(doc: &TextDocument) -> Option<&'static str> {
    match doc.eol() {
        EolStyle::Lf => Some("LF"),
        EolStyle::CrLf => Some("CRLF"),
        EolStyle::Cr => Some("CR"),
        EolStyle::None | EolStyle::Mixed => None,
    }
}

fn encoding_name(doc: &TextDocument) -> String {
    let encoding = doc.encoding();
    let bom = if encoding.bom { " with BOM" } else { "" };
    format!("{}{bom}", encoding.encoding.name())
}

/// The side ends with a line break, read from its bytes.
fn ends_with_newline(doc: &TextDocument) -> bool {
    matches!(doc.bytes().last(), Some(b'\n' | b'\r'))
}

/// A shell, Python or Perl file: by its name or its `#!` line.
pub fn is_script(doc: &TextDocument) -> bool {
    let named = doc.path().file_name().is_some_and(|name| {
        let name = name.to_string_lossy().to_lowercase();
        SCRIPT_EXTENSIONS.iter().any(|ext| name.ends_with(ext))
    });
    named || doc.bytes().starts_with(b"#!")
}

/// What differs between the two files that a save could carry into a different system.
pub fn file_warnings(left: &TextDocument, right: &TextDocument) -> Vec<String> {
    let mut warnings = Vec::new();
    let both_have_lines = left.line_count() > 0 && right.line_count() > 0;
    if let (Some(a), Some(b)) = (style(left), style(right))
        && a != b
        && both_have_lines
    {
        warnings.push(format!("Line endings differ ({a} / {b})"));
    }
    let (mixed_left, mixed_right) = (
        left.eol() == EolStyle::Mixed,
        right.eol() == EolStyle::Mixed,
    );
    if mixed_left || mixed_right {
        let where_ = match (mixed_left, mixed_right) {
            (true, true) => "on both sides",
            (true, false) => "on the left",
            _ => "on the right",
        };
        warnings.push(format!("Mixed line endings {where_}"));
    }
    if left.encoding() != right.encoding() {
        warnings.push(format!(
            "Encodings differ ({} / {})",
            encoding_name(left),
            encoding_name(right)
        ));
    }
    if both_have_lines && ends_with_newline(left) != ends_with_newline(right) {
        warnings.push("Final newline differs".to_owned());
    }
    let crlf_script = |doc: &TextDocument| {
        is_script(doc)
            && matches!(doc.eol(), EolStyle::CrLf | EolStyle::Cr | EolStyle::Mixed)
            && doc.line_count() > 1
    };
    if crlf_script(left) || crlf_script(right) {
        warnings.push(CRLF_SCRIPT.to_owned());
    }
    warnings
}

/// The status bar's text: "⚠ " and the parts joined by " · ". None when there is nothing.
pub fn warnings_text(left: &TextDocument, right: &TextDocument) -> Option<String> {
    let warnings = file_warnings(left, right);
    (!warnings.is_empty()).then(|| format!("⚠ {}", warnings.join(" · ")))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use duet_core::cancel::CancelToken;

    use super::*;

    fn open(dir: &Path, name: &str, bytes: &[u8]) -> TextDocument {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        TextDocument::open(&path, &CancelToken::new()).unwrap()
    }

    fn warnings(left: (&str, &[u8]), right: (&str, &[u8])) -> Vec<String> {
        let dir = tempfile::tempdir().unwrap();
        let a = open(&dir.path().join("l"), left.0, left.1);
        let b = open(&dir.path().join("r"), right.0, right.1);
        file_warnings(&a, &b)
    }

    #[test]
    fn file_warnings_name_each_mismatch() {
        let txt = |l: &'static [u8], r: &'static [u8]| warnings(("a.txt", l), ("b.txt", r));
        assert_eq!(
            txt(b"a\nb\n", b"a\r\nb\r\n"),
            ["Line endings differ (LF / CRLF)"]
        );
        assert_eq!(
            txt(b"a\nb\r\n", b"a\nb\n"),
            ["Mixed line endings on the left"]
        );
        assert_eq!(
            txt(b"a\r\nb\n", b"a\r\nb\n"),
            ["Mixed line endings on both sides"]
        );
        assert_eq!(
            txt(b"a\n", b"\xEF\xBB\xBFa\n"),
            ["Encodings differ (UTF-8 / UTF-8 with BOM)"]
        );
        assert_eq!(txt(b"a\n", b"a"), ["Final newline differs"]);
        assert_eq!(
            warnings(
                ("a.txt", b"#!/bin/sh\r\necho\r\n"),
                ("b.txt", b"#!/bin/sh\necho\n")
            ),
            [
                "Line endings differ (CRLF / LF)",
                "A CRLF script won't run on Linux: pick LF under On save"
            ]
        );
        assert_eq!(
            warnings(("x.py", b"a\r\nb\r\n"), ("x.py", b"a\r\nb\r\n")),
            ["A CRLF script won't run on Linux: pick LF under On save"]
        );
        assert!(txt(b"a\n", b"a\n").is_empty());
    }

    #[test]
    fn edge_cases_give_no_warning() {
        let txt = |l: &'static [u8], r: &'static [u8]| warnings(("a.txt", l), ("b.txt", r));
        // A one-line CRLF script, an empty side, a side with no breaks at all.
        assert!(warnings(("x.sh", b"echo\r\n"), ("y.sh", b"echo\r\n")).is_empty());
        assert!(txt(b"", b"a\n").is_empty());
        assert_eq!(txt(b"a", b"a\r\n"), ["Final newline differs"]);
    }

    #[test]
    fn is_script_reads_the_name_or_the_first_line() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_script(&open(dir.path(), "RUN.SH", b"x\n")));
        assert!(is_script(&open(
            dir.path(),
            "tool",
            b"#!/usr/bin/env python\n"
        )));
        assert!(!is_script(&open(dir.path(), "notes.txt", b"x\n")));
    }
}
