//! Status-line pieces as plain data. Models build them; the UI gives each tone its colour
//! (`ui::style`), so nothing here knows about gpui.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Muted,
    Text,
    Changed,
    LeftOnly,
    RightOnly,
    Moved,
    Unimportant,
    Accent,
}

/// One coloured piece of a status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusPart {
    pub text: String,
    pub tone: Tone,
}

impl StatusPart {
    pub fn new(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            text: text.into(),
            tone,
        }
    }
}

pub fn join_parts(parts: &[StatusPart]) -> String {
    parts
        .iter()
        .map(|part| part.text.as_str())
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_parts_join_with_middle_dots() {
        let parts = [
            StatusPart::new("2 differences", Tone::Muted),
            StatusPart::new("1 changed", Tone::Changed),
        ];
        assert_eq!(join_parts(&parts), "2 differences · 1 changed");
        assert_eq!(join_parts(&[]), "");
    }
}
