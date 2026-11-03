//! Find in a text compare: matches on one or both sides, in aligned-row order. Runs off the
//! UI thread over the documents' lines, so a 1 GB pair is searched without copying it.

use std::ops::Range;

use duet_core::cancel::{CancelToken, Cancelled};
use duet_core::text::document::TextDocument;
use duet_core::text::rows::RowMap;
use regex::{Captures, Regex, RegexBuilder};

use crate::text::model::Visible;

/// Matches past this many are dropped; the bar then says "100,000+".
pub const MAX_HITS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FindScope {
    #[default]
    Both,
    Left,
    Right,
}

impl FindScope {
    pub fn label(self) -> &'static str {
        match self {
            FindScope::Both => "Both",
            FindScope::Left => "Left",
            FindScope::Right => "Right",
        }
    }

    pub fn includes(self, left: bool) -> bool {
        match self {
            FindScope::Both => true,
            FindScope::Left => left,
            FindScope::Right => !left,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FindQuery {
    pub text: String,
    pub scope: FindScope,
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub row: u32,
    pub left: bool,
    pub line: u32,
    /// Byte range in the source line.
    pub range: Range<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FindResult {
    pub hits: Vec<Hit>,
    pub truncated: bool,
    pub error: Option<String>,
}

impl FindResult {
    /// (index, hit) for the hits on one side of `row`.
    pub fn on_row(&self, row: u32, left: bool) -> impl Iterator<Item = (usize, &Hit)> {
        let start = self.hits.partition_point(|h| h.row < row);
        self.hits[start..]
            .iter()
            .enumerate()
            .take_while(move |(_, h)| h.row == row)
            .filter(move |(_, h)| h.left == left)
            .map(move |(i, h)| (start + i, h))
    }
}

/// Byte ranges of `needle` in `line`. Without `match_case` both are lowercased char by char
/// and the ranges map back to the original bytes.
pub fn find_in_line(line: &str, needle: &str, match_case: bool) -> Vec<Range<usize>> {
    find_accepted(line, needle, match_case, |_| true)
}

/// The length of the character `text` starts with (1 past its end, so a scan always moves on).
fn char_len(text: &str) -> usize {
    text.chars().next().map_or(1, char::len_utf8)
}

/// `find_in_line`, keeping only the ranges `accept` takes: left to right and not overlapping. A
/// range it refuses is tried again one character on, as a look-behind would, so "--" finds the
/// bounded one in "a---" (M6).
fn find_accepted(
    line: &str,
    needle: &str,
    match_case: bool,
    accept: impl Fn(&Range<usize>) -> bool,
) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    if needle.is_empty() {
        return found;
    }
    if match_case {
        let mut pos = 0;
        while let Some(at) = line[pos..].find(needle) {
            let range = pos + at..pos + at + needle.len();
            if accept(&range) {
                pos = range.end;
                found.push(range);
            } else {
                pos = range.start + char_len(&line[range.start..]);
            }
        }
        return found;
    }
    let needle = needle.to_lowercase();
    let mut lower = String::with_capacity(line.len());
    // origin[i] = the source byte where lowered byte i came from.
    let mut origin = Vec::with_capacity(line.len() + 1);
    for (at, ch) in line.char_indices() {
        let before = lower.len();
        lower.extend(ch.to_lowercase());
        origin.extend(std::iter::repeat_n(at, lower.len() - before));
    }
    origin.push(line.len());
    let mut pos = 0;
    while let Some(at) = lower[pos..].find(&needle) {
        let at = pos + at;
        let start = origin[at];
        let mut end = origin[at + needle.len()];
        if end <= start {
            // The match ended inside one source char's lowering: take the whole char.
            end = line[start..]
                .chars()
                .next()
                .map_or(line.len(), |c| start + c.len_utf8());
        }
        if accept(&(start..end)) {
            found.push(start..end);
            pos = at + needle.len();
        } else {
            pos = at + char_len(&lower[at..]);
        }
    }
    found
}

/// Word characters for "whole word", as LocalDiff counts them: ASCII letters and digits, `_`
/// and the Thai block (a Thai word is not cut at its vowels and tone marks).
pub fn is_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ('\u{0E00}'..='\u{0E7F}').contains(&ch)
}

/// The name of the group a whole-word regex ends with: it takes the character after the match
/// (or the end of the line) so the engine can back off to a longer alternative.
const WORD_END: &str = "ww_end__";
/// The regex spelling of the characters `is_word_char` accepts.
const WORD_CLASS: &str = r"0-9A-Za-z_\x{0E00}-\x{0E7F}";

/// What a find query matches with, built once per search.
pub enum Matcher {
    Literal {
        needle: String,
        match_case: bool,
        whole_word: bool,
    },
    Regex {
        re: Regex,
        whole_word: bool,
    },
}

impl Matcher {
    /// Err with the pattern error's text when a regex does not compile.
    pub fn new(query: &FindQuery) -> Result<Matcher, String> {
        if !query.regex {
            return Ok(Matcher::Literal {
                needle: query.text.clone(),
                match_case: query.match_case,
                whole_word: query.whole_word,
            });
        }
        let build = |source: &str| {
            RegexBuilder::new(source)
                .case_insensitive(!query.match_case)
                .build()
                .map_err(|e| e.to_string())
        };
        // The pattern alone first, so its error names the user's text, not the wrapper.
        let plain = build(&query.text)?;
        if !query.whole_word {
            return Ok(Matcher::Regex {
                re: plain,
                whole_word: false,
            });
        }
        let wrap = |user: &str| format!("(?:{user})(?P<{WORD_END}>\\z|[^{WORD_CLASS}])");
        // A verbose (`(?x)`) pattern that ends in a comment would swallow the wrapper: a line
        // break ends the comment, and a verbose pattern ignores it (M7).
        let re = build(&wrap(&query.text))
            .or_else(|err| build(&wrap(&format!("{}\n", query.text))).map_err(|_| err))?;
        Ok(Matcher::Regex {
            re,
            whole_word: true,
        })
    }

    /// Byte ranges of the non-empty matches in `line`, in order.
    pub fn find_in_line(&self, line: &str) -> Vec<Range<usize>> {
        match self {
            Matcher::Literal {
                needle,
                match_case,
                whole_word,
            } => find_accepted(line, needle, *match_case, |r| {
                !*whole_word || is_bounded(line, r)
            }),
            Matcher::Regex { re, whole_word } => {
                let mut found = Vec::new();
                scan_regex(re, *whole_word, line, false, |range, _| found.push(range));
                found
            }
        }
    }

    /// `line` with the match at `range` replaced; None when the match is no longer there.
    /// A regex replacement reads `$1`, `$&`, `$$` and the like as JavaScript does.
    pub fn replace_at(&self, line: &str, range: Range<usize>, with: &str) -> Option<String> {
        match self {
            Matcher::Literal { .. } => {
                if !self.find_in_line(line).contains(&range) {
                    return None;
                }
                Some(splice(line, &range, with))
            }
            Matcher::Regex { re, whole_word } => {
                let mut replaced = None;
                scan_regex(re, *whole_word, line, true, |found, caps| {
                    if found == range && replaced.is_none() {
                        let mut text = String::new();
                        expand(re, *whole_word, with, line, &found, caps, &mut text);
                        replaced = Some(splice(line, &found, &text));
                    }
                });
                replaced
            }
        }
    }

    /// `line` with every match replaced, and how many there were.
    pub fn replace_all_in_line(&self, line: &str, with: &str) -> (String, usize) {
        let mut out = String::new();
        let (mut done, mut count) = (0, 0);
        match self {
            Matcher::Literal { .. } => {
                for range in self.find_in_line(line) {
                    out.push_str(&line[done..range.start]);
                    out.push_str(with);
                    done = range.end;
                    count += 1;
                }
            }
            Matcher::Regex { re, whole_word } => {
                scan_regex(re, *whole_word, line, true, |range, caps| {
                    out.push_str(&line[done..range.start]);
                    expand(re, *whole_word, with, line, &range, caps, &mut out);
                    done = range.end;
                    count += 1;
                });
            }
        }
        if count == 0 {
            return (line.to_owned(), 0);
        }
        out.push_str(&line[done..]);
        (out, count)
    }
}

fn splice(line: &str, range: &Range<usize>, with: &str) -> String {
    let mut out = String::with_capacity(line.len() + with.len());
    out.push_str(&line[..range.start]);
    out.push_str(with);
    out.push_str(&line[range.end..]);
    out
}

/// No word character touches either end of `range`.
fn is_bounded(line: &str, range: &Range<usize>) -> bool {
    !line[..range.start]
        .chars()
        .next_back()
        .is_some_and(is_word_char)
        && !line[range.end..].chars().next().is_some_and(is_word_char)
}

/// Calls `hit` for each non-empty match of `re`, left to right and not overlapping. A
/// whole-word match must also start after a non-word character (the end is in the pattern);
/// one that does not is retried one character on, as a lookbehind would. `captures` asks for
/// the groups, which a whole-word search always has.
fn scan_regex(
    re: &Regex,
    whole_word: bool,
    line: &str,
    captures: bool,
    mut hit: impl FnMut(Range<usize>, Option<&Captures<'_>>),
) {
    let mut pos = 0;
    while pos <= line.len() {
        let (start, end, caps) = if whole_word || captures {
            let Some(caps) = re.captures_at(line, pos) else {
                return;
            };
            let whole = caps.get(0).expect("group 0");
            let end = if whole_word {
                caps.name(WORD_END).map_or(whole.end(), |g| g.start())
            } else {
                whole.end()
            };
            (whole.start(), end, Some(caps))
        } else {
            let Some(found) = re.find_at(line, pos) else {
                return;
            };
            (found.start(), found.end(), None)
        };
        let blocked = whole_word && line[..start].chars().next_back().is_some_and(is_word_char);
        if end <= start || blocked {
            // Past the first character of the match (or of the empty match) and look again.
            match line[start..].chars().next() {
                Some(ch) => pos = start + ch.len_utf8(),
                None => return,
            }
            continue;
        }
        hit(start..end, caps.as_ref());
        pos = end;
    }
}

/// Appends the replacement for the match `range` to `out`: `$&` the match, `` $` `` and `$'`
/// the text before and after it, `$1`..`$99` a group (only if the pattern has that many),
/// `$<name>` a named group, `${1}` and `${name}` a group as the regex crate writes them (X-104),
/// `$$` a dollar sign; any other `$` stays as typed.
fn expand(
    re: &Regex,
    whole_word: bool,
    with: &str,
    line: &str,
    range: &Range<usize>,
    caps: Option<&Captures<'_>>,
    out: &mut String,
) {
    let groups = re.captures_len() - 1 - usize::from(whole_word);
    let group = |n: usize| caps.and_then(|c| c.get(n)).map_or("", |m| m.as_str());
    let bytes = with.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'$' || at + 1 == bytes.len() {
            let next = with[at..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&with[at..at + next]);
            at += next;
            continue;
        }
        let (text, used): (&str, usize) = match bytes[at + 1] {
            b'$' => ("$", 2),
            b'&' => (&line[range.clone()], 2),
            b'`' => (&line[..range.start], 2),
            b'\'' => (&line[range.end..], 2),
            d @ b'0'..=b'9' => {
                let one = usize::from(d - b'0');
                let two = bytes
                    .get(at + 2)
                    .filter(|b| b.is_ascii_digit())
                    .map(|b| one * 10 + usize::from(b - b'0'));
                match two {
                    Some(n) if (1..=groups).contains(&n) => (group(n), 3),
                    _ if (1..=groups).contains(&one) => (group(one), 2),
                    _ => ("$", 1),
                }
            }
            // Literal, as in JavaScript, unless the user's pattern names a group (the
            // whole-word wrapper's own name does not count, M7).
            b'<' if re.capture_names().flatten().any(|name| name != WORD_END) => {
                match with[at + 2..].find('>') {
                    Some(close) => {
                        let name = &with[at + 2..at + 2 + close];
                        let text = caps.and_then(|c| c.name(name)).map_or("", |m| m.as_str());
                        (text, close + 3)
                    }
                    None => ("$", 1),
                }
            }
            // A group the pattern has, by number or by the user's name; else as typed.
            b'{' => {
                let braced = with[at + 2..].find('}').and_then(|close| {
                    let inner = &with[at + 2..at + 2 + close];
                    let text = if !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit()) {
                        inner
                            .parse::<usize>()
                            .ok()
                            .filter(|n| (1..=groups).contains(n))
                            .map(group)
                    } else if inner != WORD_END && re.capture_names().flatten().any(|n| n == inner)
                    {
                        Some(caps.and_then(|c| c.name(inner)).map_or("", |m| m.as_str()))
                    } else {
                        None
                    };
                    text.map(|text| (text, close + 3))
                });
                braced.unwrap_or(("$", 1))
            }
            _ => ("$", 1),
        };
        out.push_str(text);
        at += used;
    }
}

/// Searches the sides `query.scope` names (a bad regex gives an empty result with `error`); checks `cancel` every 64K lines.
pub fn find(
    left: &TextDocument,
    right: &TextDocument,
    rows: &RowMap,
    query: &FindQuery,
    cancel: &CancelToken,
) -> Result<FindResult, Cancelled> {
    let mut result = FindResult::default();
    if query.text.is_empty() {
        return Ok(result);
    }
    let matcher = match Matcher::new(query) {
        Ok(matcher) => matcher,
        Err(error) => {
            result.error = Some(error);
            return Ok(result);
        }
    };
    'sides: for (is_left, doc) in [(true, left), (false, right)] {
        if !query.scope.includes(is_left) {
            continue;
        }
        for line in 0..doc.line_count() {
            if line % 65_536 == 0 {
                cancel.check()?;
            }
            for range in matcher.find_in_line(&doc.line(line)) {
                if result.hits.len() == 2 * MAX_HITS {
                    result.truncated = true;
                    break 'sides;
                }
                result.hits.push(Hit {
                    row: rows.row_of(is_left, line as u32),
                    left: is_left,
                    line: line as u32,
                    range,
                });
            }
        }
    }
    result.hits.sort_by_key(|h| (h.row, !h.left, h.range.start));
    if result.hits.len() > MAX_HITS {
        result.hits.truncate(MAX_HITS);
        result.truncated = true;
    }
    Ok(result)
}

/// Overview-bar positions (0..1) of the visible hits, at most about 5,000 of them.
pub fn find_ticks(result: &FindResult, visible: &Visible) -> Vec<f32> {
    let rows = visible.len();
    if rows == 0 {
        return Vec::new();
    }
    let step = (result.hits.len() / 5_000).max(1);
    let mut ticks: Vec<f32> = Vec::new();
    for hit in result.hits.iter().step_by(step) {
        if let Some(display) = visible.display_of(hit.row) {
            let at = display as f32 / rows as f32;
            if ticks.last().is_none_or(|last| at - last > 0.002) {
                ticks.push(at);
            }
        }
    }
    ticks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::display::{DisplayOptions, display_line};
    use crate::text::testing::model_for;

    #[test]
    fn case_insensitive_ranges_map_back_to_the_source() {
        assert_eq!(
            find_in_line("Hello HELLO hello", "hello", false),
            [0..5, 6..11, 12..17]
        );
        assert_eq!(
            find_in_line("Hello HELLO hello", "hello", true),
            vec![12..17]
        );
        let line = "Ünïcode ÜNÏCODE";
        assert_eq!(find_in_line(line, "ünïcode", false), [0..9, 10..19]);
        assert!(find_in_line("abc", "", false).is_empty());
    }

    fn matcher(text: &str, match_case: bool, whole_word: bool, regex: bool) -> Matcher {
        Matcher::new(&FindQuery {
            text: text.into(),
            match_case,
            whole_word,
            regex,
            ..FindQuery::default()
        })
        .unwrap()
    }

    #[test]
    fn matcher_finds_words_regexes_and_skips_empty_matches() {
        let cat = matcher("cat", true, true, false);
        assert_eq!(cat.find_in_line("cat concat cat_x cat"), [0..3, 17..20]);
        let thai = matcher("แมว", true, true, false);
        assert_eq!(thai.find_in_line("แมวกินปลา แมว"), vec![28..37]);
        let foo = matcher("fo+", false, false, true);
        assert_eq!(foo.find_in_line("Foo foo FOO"), [0..3, 4..7, 8..11]);
        assert!(
            matcher("x*", false, false, true)
                .find_in_line("abc")
                .is_empty()
        );
        let err = Matcher::new(&FindQuery {
            text: "(".into(),
            regex: true,
            ..FindQuery::default()
        })
        .err()
        .unwrap();
        assert!(err.contains('('), "{err}");
        let uni = matcher("ünïcode", false, false, false);
        assert_eq!(uni.find_in_line("Ünïcode ÜNÏCODE"), [0..9, 10..19]);
        // A regex whole word: a longer alternative at the same place still wins.
        let alt = matcher("cat|category", true, true, true);
        assert_eq!(alt.find_in_line("category cat"), [0..8, 9..12]);
        assert_eq!(alt.find_in_line("catalog"), Vec::<Range<usize>>::new());
    }

    #[test]
    fn replacements_follow_javascript_rules() {
        let re = matcher(r"a=(\d)", true, false, true);
        assert_eq!(
            re.replace_at("a=1 a=2", 4..7, "b=$1").as_deref(),
            Some("a=1 b=2")
        );
        assert_eq!(
            re.replace_at("a=1 a=2", 0..3, "b=$1"),
            Some("b=1 a=2".into())
        );
        assert_eq!(
            re.replace_at("a=1 a=2", 1..4, "b=$1"),
            None,
            "no longer there"
        );
        assert_eq!(re.replace_at("x=1", 0..3, "q"), None);
        let x = matcher("x=(\\d)", true, false, true);
        assert_eq!(x.replace_at("x=1", 0..3, "y$1z").as_deref(), Some("y1z"));
        assert_eq!(x.replace_at("x=1", 0..3, "$1_s").as_deref(), Some("1_s"));
        assert_eq!(
            x.replace_at("x=1", 0..3, "$9|$$|$&|$0").as_deref(),
            Some("$9|$|x=1|$0")
        );
        assert_eq!(x.replace_at("x=1", 0..3, "[$`|$']").as_deref(), Some("[|]"));
        let x = matcher("x", true, false, true);
        assert_eq!(x.replace_all_in_line("x x", "[$&]"), ("[x] [x]".into(), 2));
        let lit = matcher("x", true, false, false);
        assert_eq!(
            lit.replace_all_in_line("x x", "$1$&"),
            ("$1$& $1$&".into(), 2)
        );
        assert_eq!(lit.replace_at("ax", 1..2, "$1").as_deref(), Some("a$1"));
        assert_eq!(lit.replace_at("ax", 0..1, "$1"), None);
        assert_eq!(
            matcher("q", true, false, false).replace_all_in_line("abc", "z"),
            ("abc".into(), 0)
        );
        let named = matcher("(?<k>a)b", true, false, true);
        assert_eq!(named.replace_all_in_line("ab", "<$<k>>"), ("<a>".into(), 1));
    }

    #[test]
    fn braced_group_references_expand() {
        // X-104: `${1}` and `${name}` expand like `$1` and `$<name>`; `$1x` stays group 1 + "x".
        let re = matcher(r"(?P<name>a)(\d+)", true, false, true);
        let line = "a1 b22";
        assert_eq!(re.replace_all_in_line(line, "${1}x"), ("ax b22".into(), 1));
        assert_eq!(re.replace_all_in_line(line, "${name}"), ("a b22".into(), 1));
        assert_eq!(re.replace_all_in_line(line, "$1x"), ("ax b22".into(), 1));
        assert_eq!(
            re.replace_all_in_line(line, "${2}${1}"),
            ("1a b22".into(), 1)
        );
        assert_eq!(
            re.replace_at(line, 0..2, "<${name}>").as_deref(),
            Some("<a> b22")
        );
        // A group the pattern lacks, an empty or an unclosed brace stays as typed.
        assert_eq!(
            re.replace_all_in_line("a1", "${9}|${}|${x}|${1"),
            ("${9}|${}|${x}|${1".into(), 1)
        );
        // Without named groups `${1}` still expands; the whole-word wrapper's group is not one.
        let plain = matcher("c(a)t", true, true, true);
        assert_eq!(
            plain.replace_all_in_line("cat", "${1}${2}"),
            ("a${2}".into(), 1)
        );
        assert_eq!(
            plain.replace_all_in_line("cat", &format!("${{{WORD_END}}}")),
            (format!("${{{WORD_END}}}"), 1)
        );
    }

    #[test]
    fn a_literal_whole_word_retries_inside_an_unbounded_match() {
        // "--" at 1..3 touches the "a"; LocalDiff's look-behind finds the one at 2..4.
        for match_case in [true, false] {
            let dashes = matcher("--", match_case, true, false);
            assert_eq!(dashes.find_in_line("a---"), vec![2..4], "case {match_case}");
            assert_eq!(dashes.find_in_line("-- --"), [0..2, 3..5]);
        }
        let x = matcher("xx", false, true, false);
        assert_eq!(x.find_in_line("axxx xx"), vec![5..7]);
        assert_eq!(
            matcher("ab", false, true, false).replace_all_in_line("xab ab", "c"),
            ("xab c".into(), 1)
        );
    }

    #[test]
    fn the_whole_word_wrapper_leaves_the_users_pattern_alone() {
        // A verbose pattern that ends in a comment still compiles, and still matches.
        let commented = matcher("(?x) cat  # the animal", true, true, true);
        assert_eq!(commented.find_in_line("a cat concat"), vec![2..5]);
        // `$<name>` is literal when the user's pattern names no group, as in JavaScript.
        let unnamed = matcher("c(a)t", true, true, true);
        assert_eq!(
            unnamed.replace_all_in_line("cat", "$<x>$1"),
            ("$<x>a".into(), 1)
        );
        let named = matcher("c(?<v>a)t", true, true, true);
        assert_eq!(
            named.replace_all_in_line("cat", "[$<v>]"),
            ("[a]".into(), 1)
        );
    }

    #[test]
    fn word_characters_include_thai() {
        assert!(is_word_char('a') && is_word_char('Z') && is_word_char('7') && is_word_char('_'));
        assert!(is_word_char('ก') && is_word_char('๙'));
        assert!(!is_word_char(' ') && !is_word_char('-') && !is_word_char('é'));
    }

    #[test]
    fn find_reports_a_bad_pattern_and_honours_whole_word() {
        let (_dir, model) = model_for("cat concat\n", "cat\n");
        let (left, right, rows) = model.sources();
        let query = FindQuery {
            text: "cat".into(),
            whole_word: true,
            ..FindQuery::default()
        };
        let found = find(&left, &right, &rows, &query, &CancelToken::new()).unwrap();
        assert_eq!((found.hits.len(), found.error), (2, None));
        let bad = FindQuery {
            text: "(".into(),
            regex: true,
            ..FindQuery::default()
        };
        let found = find(&left, &right, &rows, &bad, &CancelToken::new()).unwrap();
        assert!(found.hits.is_empty() && found.error.is_some());
    }

    #[test]
    fn thai_matches_highlight_whole_graphemes() {
        let line = "ภาษาไทย ไทย";
        let hits = find_in_line(line, "ไทย", false);
        assert_eq!(hits.len(), 2);
        let shown = display_line(line, DisplayOptions::default());
        for hit in hits {
            let range = shown.map_range(hit).unwrap();
            assert_eq!(&shown.text[range], "ไทย");
        }
    }

    #[test]
    fn hits_come_in_row_order_left_before_right() {
        let (_dir, model) = model_for("alpha\nbeta\nalpha beta\n", "beta\nalpha\ngamma\n");
        let (left, right, rows) = model.sources();
        let query = FindQuery {
            text: "alpha".into(),
            ..FindQuery::default()
        };
        let result = find(&left, &right, &rows, &query, &CancelToken::new()).unwrap();
        assert_eq!(result.hits.len(), 3);
        assert!(
            result
                .hits
                .windows(2)
                .all(|w| (w[0].row, !w[0].left) <= (w[1].row, !w[1].left))
        );
        let left_only = FindQuery {
            scope: FindScope::Left,
            ..query
        };
        let found = find(&left, &right, &rows, &left_only, &CancelToken::new()).unwrap();
        assert_eq!(found.hits.len(), 2);
    }

    #[test]
    fn a_cancelled_search_stops() {
        let (_dir, model) = model_for("a\n", "a\n");
        let (left, right, rows) = model.sources();
        let cancel = CancelToken::new();
        cancel.cancel();
        let query = FindQuery {
            text: "a".into(),
            ..FindQuery::default()
        };
        assert!(find(&left, &right, &rows, &query, &cancel).is_err());
    }
}
