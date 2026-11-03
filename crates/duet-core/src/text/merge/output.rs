//! The merged text: result lines with conflict markers, the output format, names
//! (LocalDiff `40-merge.js:239-268`, `js:330`).

use std::ops::Range;

use super::diff3::RegionKind;
use super::session::MergeSession;
use super::{Eol, MergeText};
use crate::text::encoding::TextEncoding;

pub const MARKERS: [&str; 4] = ["<<<<<<< left", "||||||| base", "=======", ">>>>>>> right"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputFormat {
    pub eol: Eol,
    pub final_newline: bool,
    /// The encoding and BOM the result is saved in.
    pub encoding: TextEncoding,
}

fn lines_of<'a>(text: &'a MergeText, range: &Range<u32>) -> impl Iterator<Item = &'a str> {
    range.clone().map(move |n| text.line(n))
}

impl MergeSession {
    /// The merged lines, an open conflict written out with markers (`assembleLines(true)`).
    pub fn result_lines(&self) -> Vec<&str> {
        let (left, base, right) = (self.left(), self.base(), self.right());
        let mut out = Vec::new();
        for (i, region) in self.regions().iter().enumerate() {
            if region.kind == RegionKind::Stable {
                out.extend(lines_of(left, &region.left));
            } else if let Some(res) = self.resolution(i) {
                out.extend(res.lines.iter().map(String::as_str));
            } else {
                out.push(MARKERS[0]);
                out.extend(lines_of(left, &region.left));
                out.push(MARKERS[1]);
                out.extend(lines_of(base, &region.base));
                out.push(MARKERS[2]);
                out.extend(lines_of(right, &region.right));
                out.push(MARKERS[3]);
            }
        }
        out
    }

    /// `result_lines().len()` without building them (`outLineCount`).
    pub fn result_line_count(&self) -> usize {
        let len = |r: &Range<u32>| (r.end - r.start) as usize;
        self.regions()
            .iter()
            .enumerate()
            .map(|(i, region)| {
                if region.kind == RegionKind::Stable {
                    len(&region.left)
                } else if let Some(res) = self.resolution(i) {
                    res.lines.len()
                } else {
                    4 + len(&region.left) + len(&region.base) + len(&region.right)
                }
            })
            .sum()
    }

    /// The left's line ending, final newline and encoding; with no left lines, the base's,
    /// else the right's (`outFmt`; the encoding follows them too, M-62).
    pub fn output_format(&self) -> OutputFormat {
        let source = if self.left().line_count() > 0 {
            self.left()
        } else if self.base().line_count() > 0 {
            self.base()
        } else if self.right().line_count() > 0 {
            self.right()
        } else {
            self.left()
        };
        OutputFormat {
            eol: source.format.eol,
            final_newline: source.format.final_newline,
            encoding: source.encoding,
        }
    }

    /// The text to write: lines joined with the output's line ending. Empty with no lines.
    pub fn result_text(&self) -> String {
        let lines = self.result_lines();
        if lines.is_empty() {
            return String::new();
        }
        let format = self.output_format();
        let eol = format.eol.as_str();
        let mut out = lines.join(eol);
        if format.final_newline {
            out.push_str(eol);
        }
        out
    }
}

/// "a.txt" -> "a.conflicts.txt": ".conflicts" goes before the last extension, else at the end.
pub fn conflicts_name(name: &str) -> String {
    match name.rfind('.') {
        Some(dot) => format!("{}.conflicts{}", &name[..dot], &name[dot..]),
        None => format!("{name}.conflicts"),
    }
}

/// The lines of a typed result: split on CRLF, CR or LF; an empty text has none.
pub fn split_typed(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                lines.push(text[start..i].to_owned());
                i += 1;
                start = i + 1;
            }
            b'\r' | b'\n' => {
                lines.push(text[start..i].to_owned());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    lines.push(text[start..].to_owned());
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancelToken;
    use crate::text::merge::MergeText;
    use crate::text::merge::diff3::diff3;
    use crate::text::merge::session::MergeSession;
    use std::sync::Arc;

    fn raw(l: &str, b: &str, r: &str) -> MergeSession {
        let mk = |t: &str| Arc::new(MergeText::typed(t.to_owned()));
        let (l, b, r) = (mk(l), mk(b), mk(r));
        let regions = diff3(&l, &b, &r, false, &CancelToken::new()).unwrap();
        MergeSession::new(l, b, r, regions, false)
    }

    #[test]
    fn the_result_follows_the_left_files_format() {
        let s = raw("a\r\nb\r\n", "a\r\nb\r\n", "a\nB\n");
        assert_eq!(s.result_text(), "a\r\nB\r\n");
        assert_eq!(
            s.output_format(),
            OutputFormat {
                eol: Eol::CrLf,
                final_newline: true,
                encoding: TextEncoding {
                    encoding: encoding_rs::UTF_8,
                    bom: false
                }
            }
        );
        assert_eq!(raw("a\nb", "a\nb", "a\nb").result_text(), "a\nb");
        let e = raw("", "x\r\n", "x\r\ny\r\n");
        assert_eq!(
            e.result_text(),
            "<<<<<<< left\r\n||||||| base\r\nx\r\n=======\r\nx\r\ny\r\n>>>>>>> right\r\n"
        );
        assert_eq!(raw("", "", "").result_text(), "");
    }

    #[test]
    fn an_empty_left_takes_the_bases_encoding_too() {
        // M-62: the encoding comes from the same file as the line ending.
        let utf8 = TextEncoding {
            encoding: encoding_rs::UTF_8,
            bom: false,
        };
        let latin = TextEncoding {
            encoding: encoding_rs::WINDOWS_1252,
            bom: false,
        };
        let session =
            |l: (&str, TextEncoding), b: (&str, TextEncoding), r: (&str, TextEncoding)| {
                let mk = |(t, e): (&str, TextEncoding)| Arc::new(MergeText::new(t.to_owned(), e));
                let (l, b, r) = (mk(l), mk(b), mk(r));
                let regions = diff3(&l, &b, &r, false, &CancelToken::new()).unwrap();
                MergeSession::new(l, b, r, regions, false)
            };
        let s = session(
            ("", utf8),
            ("caf\u{e9}\r\nbase two\r\n", latin),
            ("caf\u{e9}\nright two\n", utf8),
        );
        assert_eq!(s.output_format().encoding, latin);
        assert_eq!(s.output_format().eol, Eol::CrLf);
        let s = session(("x\n", utf8), ("x\n", latin), ("y\n", latin));
        assert_eq!(
            s.output_format().encoding,
            utf8,
            "the left's, when it has lines"
        );
    }

    #[test]
    fn names_and_typed_text_follow_localdiff() {
        assert_eq!(conflicts_name("a.txt"), "a.conflicts.txt");
        assert_eq!(conflicts_name("a.b.txt"), "a.b.conflicts.txt");
        assert_eq!(conflicts_name("Makefile"), "Makefile.conflicts");
        assert_eq!(conflicts_name(".gitignore"), ".conflicts.gitignore");
        assert_eq!(conflicts_name(""), ".conflicts");
        assert!(split_typed("").is_empty());
        assert_eq!(split_typed("a"), ["a"]);
        assert_eq!(split_typed("a\n"), ["a", ""]);
        assert_eq!(split_typed("a\r\nb\rc"), ["a", "b", "c"]);
    }
}
