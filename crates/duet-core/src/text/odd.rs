//! Invisible characters (`LD_ODD_RE`, `ldOddInfo`, `ldFindOdd`, `de:884-941`): characters that
//! look like a space or like nothing at all, named and placed in the text, a comment or the
//! indentation.

use crate::cancel::{CancelToken, Cancelled};
use crate::text::lexer::{Language, LexState, Token, TokenClass, lex_line};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OddKind {
    /// Looks like a space (no-break, em, ideographic …).
    Space,
    /// Zero-width, bidi and other format characters.
    Zero,
    /// C0 and C1 controls.
    Control,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OddPlace {
    Comment,
    Indent,
    Text,
}

/// A hit at byte `byte` of line `line` (without line endings).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OddHit {
    pub line: u32,
    pub byte: usize,
    pub ch: char,
    pub place: OddPlace,
}

/// Every hit is counted; `hits` keeps the first `cap`. `warn` counts the hits in the text,
/// `info` those in comments or indentation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OddScan {
    pub count: usize,
    pub warn: usize,
    pub info: usize,
    pub hits: Vec<OddHit>,
}

/// R19's set (`LD_ODD_RE`).
pub fn is_odd(ch: char) -> bool {
    matches!(
        ch,
        '\u{0}'..='\u{8}'
            | '\u{b}'
            | '\u{c}'
            | '\u{e}'..='\u{1f}'
            | '\u{7f}'..='\u{9f}'
            | '\u{a0}'
            | '\u{ad}'
            | '\u{1680}'
            | '\u{180e}'
            | '\u{2000}'..='\u{200f}'
            | '\u{2028}'..='\u{202f}'
            | '\u{205f}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

/// The odd characters that look like a space (`LD_ODD_SPACE_RE`).
fn is_odd_space(ch: char) -> bool {
    matches!(
        ch,
        '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

/// `is_odd`, except that a zero-width joiner or non-joiner after a visible non-ASCII
/// character is part of the writing (an emoji sequence, Indic conjuncts) and is not counted
/// (PF15).
pub fn is_odd_after(prev: Option<char>, ch: char) -> bool {
    if !is_odd(ch) {
        return false;
    }
    let joiner = ch == '\u{200c}' || ch == '\u{200d}';
    !(joiner && prev.is_some_and(|p| !p.is_ascii() && !p.is_whitespace() && !is_odd(p)))
}

/// (short label, name, kind), as `ldOddInfo`.
pub fn odd_info(ch: char) -> (String, String, OddKind) {
    let code = ch as u32;
    let hex = format!("U+{code:04X}");
    if code < 32 || (127..=159).contains(&code) {
        let label = match code {
            0 => "NUL".to_owned(),
            11 => "VT".to_owned(),
            12 => "FF".to_owned(),
            _ => hex.clone(),
        };
        return (label, format!("{hex} control character"), OddKind::Control);
    }
    if let Some(name) = code
        .checked_sub(0x2000)
        .and_then(|at| SPACE_NAMES.get(at as usize))
    {
        return ("⍽".to_owned(), format!("{hex} {name}"), OddKind::Space);
    }
    match odd_name(code) {
        Some((label, name)) => {
            let kind = if code == 0xa0 || label == "⍽" {
                OddKind::Space
            } else {
                OddKind::Zero
            };
            (label.to_owned(), format!("{hex} {name}"), kind)
        }
        None => (hex.clone(), hex, OddKind::Zero),
    }
}

/// U+2000 to U+200A.
const SPACE_NAMES: [&str; 11] = [
    "en quad",
    "em quad",
    "en space",
    "em space",
    "three-per-em space",
    "four-per-em space",
    "six-per-em space",
    "figure space",
    "punctuation space",
    "thin space",
    "hair space",
];

/// `LD_ODD_NAMES`, with LocalDiff's Thai notes in English.
fn odd_name(code: u32) -> Option<(&'static str, &'static str)> {
    Some(match code {
        0x00A0 => ("NBSP", "no-break space"),
        0x00AD => ("SHY", "soft hyphen"),
        0x1680 => ("⍽", "ogham space"),
        0x180E => ("MVS", "mongolian vowel separator"),
        0x200B => ("ZWSP", "zero-width space"),
        0x200C => ("ZWNJ", "zero-width non-joiner"),
        0x200D => ("ZWJ", "zero-width joiner"),
        0x200E => ("LRM", "left-to-right mark"),
        0x200F => ("RLM", "right-to-left mark"),
        0x2028 => ("LSEP", "line separator"),
        0x2029 => ("PSEP", "paragraph separator"),
        0x202A => ("LRE", "left-to-right embedding"),
        0x202B => ("RLE", "right-to-left embedding"),
        0x202C => ("PDF", "pop directional formatting"),
        0x202D => ("LRO", "left-to-right override"),
        0x202E => ("RLO", "right-to-left override"),
        0x202F => ("⍽", "narrow no-break space"),
        0x205F => ("⍽", "medium mathematical space"),
        0x2060 => ("WJ", "word joiner"),
        0x2066 => ("LRI", "left-to-right isolate"),
        0x2067 => ("RLI", "right-to-left isolate"),
        0x2068 => ("FSI", "first strong isolate"),
        0x2069 => ("PDI", "pop directional isolate"),
        0x3000 => ("⍽", "ideographic (full-width) space"),
        0xFEFF => ("BOM", "zero-width no-break space / BOM inside the file"),
        _ => return None,
    })
}

/// The lines of `text` without their endings, split as `LineIndex` splits them.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let bytes = rest.as_bytes();
        let (line, next) = match memchr::memchr2(b'\n', b'\r', bytes) {
            Some(at) => {
                let eol = if bytes[at] == b'\r' && bytes.get(at + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                (&rest[..at], &rest[at + eol..])
            }
            None => (rest, ""),
        };
        rest = next;
        Some(line)
    })
}

/// Whether the line holds a character of R19's set; ASCII lines are decided on their bytes.
fn has_odd(line: &str) -> bool {
    line.bytes().any(|b| (b < 0x20 && b != b'\t') || b >= 0x7f) && line.chars().any(is_odd)
}

const CHECK_EVERY: usize = 4096;

/// Finds the invisible characters of `text`, line by line as `LineIndex` splits it
/// (`ldFindOdd`). The lexer runs over every line up to the last hit, so a hit inside a
/// comment that started lines before is placed in the comment. Leading space-like characters
/// are indentation, except in YAML, Python and Shell, where indentation matters.
pub fn find_odd(
    text: &str,
    lang: Language,
    cap: usize,
    cancel: &CancelToken,
) -> Result<OddScan, Cancelled> {
    let mut scan = OddScan::default();
    let mut last = None;
    for (n, line) in lines(text).enumerate() {
        if n % CHECK_EVERY == 0 {
            cancel.check()?;
        }
        if has_odd(line) {
            last = Some(n);
        }
    }
    let Some(last) = last else {
        return Ok(scan);
    };
    let lex = lang.has_comments();
    let indent_matters = matches!(lang, Language::Yaml | Language::Python | Language::Shell);
    let mut state = LexState::default();
    let mut tokens = Vec::new();
    for (n, line) in lines(text).take(last + 1).enumerate() {
        if n % CHECK_EVERY == 0 {
            cancel.check()?;
        }
        if lex {
            tokens.clear();
            lex_line(line, lang, &mut state, &mut tokens);
        }
        if !has_odd(line) {
            continue;
        }
        let lead = line.len()
            - line
                .trim_start_matches(|c| c == ' ' || c == '\t' || is_odd_space(c))
                .len();
        // Tokens are in order and never overlap, so one pointer walks them with the hits.
        let mut token = 0;
        let mut prev = None;
        for (byte, ch) in line.char_indices() {
            let odd = is_odd_after(prev, ch);
            prev = Some(ch);
            if !odd {
                continue;
            }
            while tokens.get(token).is_some_and(|t: &Token| t.end <= byte) {
                token += 1;
            }
            let place = if tokens
                .get(token)
                .is_some_and(|t| t.class == TokenClass::Comment && t.start <= byte)
            {
                OddPlace::Comment
            } else if byte < lead && is_odd_space(ch) && !indent_matters {
                OddPlace::Indent
            } else {
                OddPlace::Text
            };
            scan.count += 1;
            if place == OddPlace::Text {
                scan.warn += 1;
            } else {
                scan.info += 1;
            }
            if scan.hits.len() < cap {
                scan.hits.push(OddHit {
                    line: n as u32,
                    byte,
                    ch,
                    place,
                });
            }
        }
    }
    Ok(scan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(text: &str, lang: Language) -> OddScan {
        find_odd(text, lang, 5_000, &CancelToken::new()).unwrap()
    }

    fn info(label: &str, name: &str, kind: OddKind) -> (String, String, OddKind) {
        (label.to_owned(), name.to_owned(), kind)
    }

    #[test]
    fn odd_characters_are_named_and_placed() {
        assert_eq!(
            odd_info('\u{a0}'),
            info("NBSP", "U+00A0 no-break space", OddKind::Space)
        );
        assert_eq!(
            odd_info('\u{200b}'),
            info("ZWSP", "U+200B zero-width space", OddKind::Zero)
        );
        assert_eq!(
            odd_info('\u{1}'),
            info("U+0001", "U+0001 control character", OddKind::Control)
        );
        assert_eq!(
            odd_info('\u{2003}'),
            info("⍽", "U+2003 em space", OddKind::Space)
        );
        assert_eq!(
            odd_info('\u{feff}').1,
            "U+FEFF zero-width no-break space / BOM inside the file"
        );
        assert!(!is_odd('\t') && !is_odd('\n') && !is_odd(' '));

        assert_eq!(
            scan("a\u{a0}b\n", Language::Text),
            OddScan {
                count: 1,
                warn: 1,
                info: 0,
                hits: vec![OddHit {
                    line: 0,
                    byte: 1,
                    ch: '\u{a0}',
                    place: OddPlace::Text
                }],
            }
        );
        let indent = scan("  \u{a0}x\n", Language::CLike);
        assert_eq!(indent.hits[0].place, OddPlace::Indent);
        assert_eq!((indent.warn, indent.info), (0, 1));
        assert_eq!(
            scan("  \u{a0}x\n", Language::Python).hits[0].place,
            OddPlace::Text,
            "indentation matters in Python"
        );
        assert_eq!(
            scan("// \u{200b}\n", Language::CLike).hits[0].place,
            OddPlace::Comment
        );
        let capped = find_odd(
            "\u{a0}a\u{200b}\nb\u{1}\n",
            Language::Text,
            1,
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(capped.count, 3);
        assert_eq!(capped.hits.len(), 1);
    }

    #[test]
    fn the_set_has_localdiffs_edges() {
        for (ch, odd) in [
            ('\u{8}', true),
            ('\u{d}', false),
            ('\u{e}', true),
            ('\u{7e}', false),
            ('\u{7f}', true),
            ('\u{9f}', true),
            ('\u{a1}', false),
            ('\u{2010}', false),
            ('\u{2027}', false),
            ('\u{2028}', true),
            ('\u{2030}', false),
            ('\u{205e}', false),
            ('\u{2064}', true),
            ('\u{2065}', false),
            ('\u{2069}', true),
            ('\u{206a}', false),
            ('ก', false),
        ] {
            assert_eq!(is_odd(ch), odd, "{ch:?}");
        }
        assert_eq!(odd_info('\u{0}').0, "NUL");
        assert_eq!(odd_info('\u{c}').0, "FF");
        assert_eq!(odd_info('\u{85}').2, OddKind::Control);
        assert_eq!(
            odd_info('\u{3000}'),
            info("⍽", "U+3000 ideographic (full-width) space", OddKind::Space)
        );
        assert_eq!(
            odd_info('\u{2061}'),
            info("U+2061", "U+2061", OddKind::Zero),
            "unnamed, as in LocalDiff"
        );
    }

    #[test]
    fn joiners_inside_writing_are_not_invisible() {
        // PF15: an emoji sequence and a Devanagari conjunct keep their joiners.
        assert_eq!(
            scan("\u{1f468}\u{200d}\u{1f469}\n", Language::Text).count,
            0
        );
        assert_eq!(
            scan("\u{915}\u{94d}\u{200d}\u{937}\n", Language::Text).count,
            0
        );
        assert_eq!(scan("ก\u{200c}x\n", Language::Text).count, 0);
        // After ASCII, a space, another invisible character or at the line start they count.
        assert_eq!(scan("a\u{200d}b\n", Language::Text).count, 1);
        assert_eq!(scan("\u{200d}b\n", Language::Text).count, 1);
        assert_eq!(scan("\u{a0}\u{200c}b\n", Language::Text).count, 2);
        assert_eq!(scan("\u{200b}\u{200d}\n", Language::Text).count, 2);
        assert!(is_odd('\u{200d}'), "the set itself is LocalDiff's");
        assert!(!is_odd_after(Some('\u{1f468}'), '\u{200d}'));
        assert!(is_odd_after(Some('a'), '\u{200d}'));
        assert!(
            is_odd_after(Some('\u{1f468}'), '\u{200b}'),
            "only the joiners"
        );
    }

    #[test]
    fn hits_are_placed_on_duets_lines() {
        // CRLF, CR and LF all end a line, as in `LineIndex`.
        let hits = scan("x\r\n\u{a0}a\rb\u{200b}\nc", Language::Text).hits;
        let places: Vec<_> = hits.iter().map(|h| (h.line, h.byte)).collect();
        assert_eq!(places, [(1, 0), (2, 1)]);
        // A block comment opened lines before still holds the hit.
        let hits = scan("/* a\n\nb \u{200b}\n*/ \u{200b}\n", Language::CLike).hits;
        let places: Vec<_> = hits.iter().map(|h| (h.line, h.place)).collect();
        assert_eq!(places, [(2, OddPlace::Comment), (3, OddPlace::Text)]);
        assert_eq!(
            scan("<!-- \u{a0} --> \u{a0}\n", Language::Markup)
                .hits
                .iter()
                .map(|h| h.place)
                .collect::<Vec<_>>(),
            [OddPlace::Comment, OddPlace::Text]
        );
        assert_eq!(scan("plain text\n", Language::CLike), OddScan::default());
        assert_eq!(scan("", Language::Text), OddScan::default());
    }

    #[test]
    fn a_cancelled_scan_stops() {
        let cancel = CancelToken::new();
        cancel.cancel();
        assert_eq!(
            find_odd("a\u{a0}\n", Language::Text, 10, &cancel),
            Err(Cancelled)
        );
    }
}
