//! `assert_no_stray_english`: the check that a Thai report left no English behind. Included by
//! `#[path]` in the report tests.

/// Every run of three or more ASCII letters in `output`, once markup, entities and the fixture's
/// own names and paths are taken out, must be one of `allowed`. It reads attribute text too,
/// which the source scanner cannot see. Style and script bodies are code and are left out.
///
/// `strip` holds the names and paths the fixture put in; `allowed` the words that stay English
/// in a Thai report (format names, `duet`, `conflict`).
pub fn assert_no_stray_english(output: &str, strip: &[&str], allowed: &[&str]) {
    let mut text = output.to_owned();
    // Longest first, so a path is gone before a name inside it.
    let mut strip: Vec<&str> = strip.iter().copied().filter(|s| !s.is_empty()).collect();
    strip.sort_by_key(|s| std::cmp::Reverse(s.len()));
    for each in strip {
        text = text.replace(each, " ");
    }
    let text = without_markup(&text);
    let mut stray: Vec<String> = Vec::new();
    let mut word = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphabetic() {
            word.push(c);
        } else {
            if (word.len() >= 3 || word == "vs")
                && !allowed.contains(&word.as_str())
                && !stray.contains(&word)
            {
                stray.push(std::mem::take(&mut word));
            }
            word.clear();
        }
    }
    assert!(stray.is_empty(), "English left in a Thai report: {stray:?}");
}

/// `text` with every `<…>` tag and `&…;` entity replaced by a space, and style and script bodies.
fn without_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let tail = &rest[open..];
        if let Some(body) = tail.strip_prefix("<style>") {
            let end = body.find("</style>").map_or(body.len(), |at| at + 8);
            rest = &body[end..];
            out.push(' ');
            continue;
        }
        if let Some(body) = tail.strip_prefix("<script>") {
            // Code, judged by the report tests against the English script, line by line.
            let end = body.find("</script>").map_or(body.len(), |at| at + 9);
            rest = &body[end..];
            out.push(' ');
            continue;
        }
        // An attribute value can hold text (`title`, `placeholder`, `prompt`): keep those.
        let close = tail.find('>').map_or(tail.len(), |at| at + 1);
        out.push(' ');
        out.push_str(&attribute_text(&tail[..close]));
        rest = &tail[close..];
    }
    out.push_str(rest);
    strip_entities(&out)
}

/// The text of the `title`, `placeholder`, `prompt`, `promptTitle` and `display` attributes.
fn attribute_text(tag: &str) -> String {
    let mut out = String::new();
    for name in ["title", "placeholder", "prompt", "promptTitle", "display"] {
        let needle = format!(" {name}=\"");
        if let Some(at) = tag.find(&needle) {
            let value = &tag[at + needle.len()..];
            out.push(' ');
            out.push_str(&value[..value.find('"').unwrap_or(value.len())]);
        }
    }
    out
}

fn strip_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        match tail.find(';') {
            Some(semi)
                if semi <= 10
                    && tail[1..semi]
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '#') =>
            {
                out.push(' ');
                rest = &tail[semi + 1..];
            }
            _ => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}
