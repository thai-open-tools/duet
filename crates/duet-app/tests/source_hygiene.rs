//! Checks over the source files themselves.

use std::path::Path;

/// A cp1252 round trip (PowerShell 5.1 `Get-Content` without `-Encoding`) turns "…" into
/// mojibake that starts with U+00E2 or U+00C2. None of the sources may carry such a sequence.
#[test]
fn sources_have_no_cp1252_mojibake() {
    // Written as escapes so this file does not match itself.
    const BAD: [&str; 6] = [
        "\u{e2}\u{20ac}", // from – … and curly quotes
        "\u{e2}\u{2013}", // from ▶
        "\u{e2}\u{2014}", // from ◀
        "\u{e2}\u{2c6}",  // from −
        "\u{c2}\u{b7}",   // from ·
        "\u{e2}\u{178}",  // from ⟳
    ];
    fn visit(dir: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                if BAD.iter().any(|bad| text.contains(bad)) {
                    found.push(path.display().to_string());
                }
            }
        }
    }
    let mut found = Vec::new();
    for dir in ["src", "tests"] {
        visit(&Path::new(env!("CARGO_MANIFEST_DIR")).join(dir), &mut found);
    }
    assert!(found.is_empty(), "mojibake in {found:?}");
}

/// The first colour-emoji glyph a process draws loads the emoji font, which cost the folder
/// screen's first draw about 18 ms. Icons are drawn or come from the text fonts instead.
#[test]
fn the_ui_draws_no_colour_emoji() {
    fn visit(dir: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                // Unit tests, at the end of a file, may type one to test grapheme handling.
                let code = text.split("#[cfg(test)]").next().unwrap_or_default();
                // The emoji blocks of the supplementary planes (U+1F000 to U+1FAFF).
                if let Some(c) = code
                    .chars()
                    .find(|c| ('\u{1f000}'..='\u{1faff}').contains(c))
                {
                    found.push(format!("{} (U+{:04X})", path.display(), c as u32));
                }
            }
        }
    }
    let mut found = Vec::new();
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    assert!(found.is_empty(), "colour emoji in {found:?}");
}

/// The text of each call in `code` that starts with `opener`, up to its closing parenthesis.
fn calls<'a>(code: &'a str, opener: &str) -> Vec<&'a str> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = code[from..].find(opener) {
        let start = from + at + opener.len();
        let (mut depth, mut quoted, mut escaped) = (1, false, false);
        let mut end = code.len();
        for (i, c) in code[start..].char_indices() {
            if quoted {
                match c {
                    _ if escaped => escaped = false,
                    '\\' => escaped = true,
                    '"' => quoted = false,
                    _ => {}
                }
                continue;
            }
            match c {
                '"' => quoted = true,
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        found.push(&code[start..end]);
        from = start;
    }
    found
}

/// The string literals in `code`, without their quotes.
fn literals(code: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut open: Option<usize> = None;
    let mut escaped = false;
    for (i, c) in code.char_indices() {
        match open {
            Some(_) if escaped => escaped = false,
            Some(_) if c == '\\' => escaped = true,
            Some(start) if c == '"' => {
                found.push(&code[start..i]);
                open = None;
            }
            Some(_) => {}
            None if c == '"' => open = Some(i + 1),
            None => {}
        }
    }
    found
}

/// Chrome icons are Lucide SVGs from `ui::icons`. A control's label or a menu entry never
/// carries an arrow, triangle or symbol character: Segoe UI lacks most of them, so Windows drew
/// each from another font, at another size and baseline. Content glyphs (the folder status
/// column, the text markers, tab titles) are data, not labels, and stay text.
#[test]
fn control_labels_carry_no_icon_characters() {
    const ICON_CHARACTERS: &str = "⤒▲▼⤓↶↷⌕⧉⇄⇆↻↺◀▶⏮⏭▾▸ⓘ◐✓×→←−↑";
    const LABELLED: [&str; 12] = [
        "ld_button(",
        "ld_ghost(",
        "ld_primary(",
        "ld_segment(",
        "ld_toggle(",
        "ld_chip(",
        "ld_card(",
        "ld_icon_label_button(",
        "ld_label_icon_button(",
        ".label(",
        "MenuEntry::action(",
        "PopupMenuItem::new(",
    ];
    fn visit(dir: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                // Unit tests, at the end of a file, may name a glyph they check is gone.
                let code = text.split("#[cfg(test)]").next().unwrap_or_default();
                for opener in LABELLED {
                    for call in calls(code, opener) {
                        for literal in literals(call) {
                            if literal.chars().any(|c| ICON_CHARACTERS.contains(c)) {
                                found.push(format!("{}: {literal:?}", path.display()));
                            }
                        }
                    }
                }
            }
        }
    }
    let mut found = Vec::new();
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    assert!(
        found.is_empty(),
        "icon characters in labels; use an icon from ui::icons: {found:#?}"
    );
}

/// A kit menu row can wrap a Thai label (see `ui::menu::one_line`), so every row is made in one
/// place. A view that wants a menu row calls `ui::menu::row`.
#[test]
fn menu_rows_are_made_in_one_place() {
    fn visit(dir: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs")
                && path.file_name().is_some_and(|n| n != "menu.rs")
                && std::fs::read_to_string(&path)
                    .unwrap()
                    .contains("PopupMenuItem::new(")
            {
                found.push(path.display().to_string());
            }
        }
    }
    let mut found = Vec::new();
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    assert!(found.is_empty(), "use ui::menu::row: {found:?}");
}
