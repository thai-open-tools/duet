//! Compare rules: blank lines, comments, text and line regexes. Ignored lines leave the key
//! text; they come back as soft runs and paired `Same` rows where LocalDiff puts them
//! (`de:676-771`).

use std::borrow::Cow;
use std::ops::Range;

use crate::cancel::CancelToken;
use crate::text::diff::{
    DiffError, DiffInput, DiffOptions, LineDiff, LineHunk, count_soft_pairs_checked,
    diff_core_anchored, diff_lines, normalize_whitespace,
};
use crate::text::lexer::{Language, LexState, strip_comments};
use crate::text::lines::LineIndex;

pub const RULES_MAX_BYTES: usize = crate::text::document::EDIT_LIMIT as usize;

#[derive(Debug, Clone, Default)]
pub struct TextRules {
    pub ignore_blank: bool,
    /// Unimportant text: every match is cut from the line's key.
    pub text: Vec<regex::Regex>,
    /// Unimportant lines: a match anywhere in the raw line ignores the line.
    pub lines: Vec<regex::Regex>,
    /// The language whose comments are cut; `Some` only while comments are ignored.
    pub comments: Option<Language>,
    /// Manual alignments: file lines (left, right) forced to pair.
    pub anchors: Vec<(u32, u32)>,
}

impl TextRules {
    /// Whether any rule changes the compare. Inactive rules take the plain `diff_lines` path;
    /// so do comments of a language without comment syntax (`de:680`).
    pub fn is_active(&self) -> bool {
        self.ignore_blank
            || !self.text.is_empty()
            || !self.lines.is_empty()
            || self.comments.is_some_and(Language::has_comments)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{pattern}: {reason}")]
pub struct RuleError {
    pub pattern: String,
    pub reason: String,
}

/// One pattern per line; lines are trimmed, blank ones skipped.
pub fn compile_list(text: &str) -> Result<Vec<regex::Regex>, RuleError> {
    text.lines()
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty())
        .map(|pattern| {
            regex::Regex::new(pattern).map_err(|err| RuleError {
                pattern: pattern.to_owned(),
                reason: short_reason(&err),
            })
        })
        .collect()
}

/// The regex crate's message is a multi-line diagram; the panel shows its `error:` line.
fn short_reason(err: &regex::Error) -> String {
    let full = err.to_string();
    let line = full
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("error: "))
        .unwrap_or_else(|| {
            full.lines()
                .next()
                .unwrap_or(crate::i18n::tr_key("invalid pattern"))
        });
    line.trim().to_owned()
}

/// A line of only space, tab, no-break space, ideographic space, form feed or vertical tab
/// (LocalDiff's `LD_BLANK_RE`).
pub fn is_blank(line: &str) -> bool {
    line.chars()
        .all(|c| matches!(c, ' ' | '\t' | '\u{a0}' | '\u{3000}' | '\u{c}' | '\u{b}'))
}

/// The compare under the rules. The bool says whether the rules were applied (false over
/// `RULES_MAX_BYTES`).
pub fn diff_with_rules(
    left: DiffInput<'_>,
    right: DiffInput<'_>,
    opts: DiffOptions,
    rules: &TextRules,
    cancel: &CancelToken,
) -> Result<(LineDiff, bool), DiffError> {
    diff_with_rules_limited(left, right, opts, rules, cancel, RULES_MAX_BYTES)
}

fn diff_with_rules_limited(
    left: DiffInput<'_>,
    right: DiffInput<'_>,
    opts: DiffOptions,
    rules: &TextRules,
    cancel: &CancelToken,
    max_bytes: usize,
) -> Result<(LineDiff, bool), DiffError> {
    if !rules.is_active() && rules.anchors.is_empty() {
        return diff_lines(left, right, opts, cancel).map(|diff| (diff, true));
    }
    if left.bytes.len() > max_bytes || right.bytes.len() > max_bytes {
        return diff_lines(left, right, opts, cancel).map(|diff| (diff, false));
    }
    let keys = [
        side_keys(left, opts, rules, cancel)?,
        side_keys(right, opts, rules, cancel)?,
    ];
    let indexes = [LineIndex::new(&keys[0].text), LineIndex::new(&keys[1].text)];
    // Every option was applied while building the keys, so the key text is compared as it is.
    // The anchors are file lines; the diff sees only the kept ones, so an anchor on an ignored
    // line has no place in it.
    let anchors: Vec<(u32, u32)> = rules
        .anchors
        .iter()
        .filter_map(|&(l, r)| {
            Some((
                keys[0].kept.binary_search(&l).ok()? as u32,
                keys[1].kept.binary_search(&r).ok()? as u32,
            ))
        })
        .collect();
    let (kept, _) = diff_core_anchored(
        DiffInput {
            bytes: &keys[0].text,
            lines: &indexes[0],
        },
        DiffInput {
            bytes: &keys[1].text,
            lines: &indexes[1],
        },
        DiffOptions::default(),
        cancel,
        None,
        &anchors,
    )?;
    let mut diff = walk_back(
        &kept,
        [&keys[0].kept, &keys[1].kept],
        [left.lines.len() as u32, right.lines.len() as u32],
        cancel,
    )?;
    diff.soft_pairs =
        count_soft_pairs_checked(left, right, &diff, cancel).map_err(DiffError::Cancelled)?;
    Ok((diff, true))
}

/// One side's key text (a key and a newline per kept line) and the kept lines' numbers.
struct SideKeys {
    text: Vec<u8>,
    kept: Vec<u32>,
}

/// The key of a line follows LocalDiff's order (`de:676-708`): comments cut, text rules,
/// whitespace mode, case, then the line ending as a tag byte unless endings are ignored. An
/// ignored line has no key.
fn side_keys(
    side: DiffInput<'_>,
    opts: DiffOptions,
    rules: &TextRules,
    cancel: &CancelToken,
) -> Result<SideKeys, DiffError> {
    let lines = side.lines.len();
    let mut out = SideKeys {
        text: Vec::with_capacity(side.bytes.len() + lines),
        kept: Vec::with_capacity(lines),
    };
    let comments = rules.comments.filter(|lang| lang.has_comments());
    let mut state = LexState::default();
    let mut tokens = Vec::new();
    for line in 0..lines {
        if line % 65_536 == 0 {
            cancel.check().map_err(DiffError::Cancelled)?;
        }
        let raw = String::from_utf8_lossy(side.lines.line_bytes(side.bytes, line));
        // The lexer sees every line, ignored ones included, so a comment that an ignored line
        // opens or closes is still known.
        let cut = comments.map(|lang| strip_comments(&raw, lang, &mut state, &mut tokens));
        if (rules.ignore_blank && is_blank(&raw)) || rules.lines.iter().any(|re| re.is_match(&raw))
        {
            continue;
        }
        let stripped = cut.as_deref().is_some_and(|cut| cut != &*raw);
        let mut text: Cow<'_, str> = cut.unwrap_or(Cow::Borrowed(&raw));
        for re in &rules.text {
            if let Cow::Owned(cut) = re.replace_all(&text, "") {
                text = Cow::Owned(cut);
            }
        }
        // A line the comment cut left blank is ignored; blankness is tested after the text
        // rules, as LocalDiff does (PF20).
        if stripped && is_blank(&text) && !is_blank(&raw) {
            continue;
        }
        let text = normalize_whitespace(&text, opts.whitespace);
        let start = out.text.len();
        if opts.ignore_case {
            out.text.extend_from_slice(text.to_lowercase().as_bytes());
        } else {
            out.text.extend_from_slice(text.as_bytes());
        }
        if !opts.ignore_eol {
            out.text.push(match side.lines.eol_bytes(side.bytes, line) {
                b"\r\n" => 1,
                b"\n" => 2,
                b"\r" => 3,
                _ => 0,
            });
        }
        debug_assert!(
            !out.text[start..].contains(&b'\r') && !out.text[start..].contains(&b'\n'),
            "a key never holds a line break"
        );
        out.text.push(b'\n');
        out.kept.push(line as u32);
    }
    Ok(out)
}

/// Puts the ignored lines back (`ldRows`, `de:724-731`). Between two kept matches the ignored
/// lines pair one to one from the top and the longer side's extras form a soft run; an ignored
/// line inside a hunk's span stays in the hunk.
fn walk_back(
    kept: &LineDiff,
    kept_lines: [&[u32]; 2],
    totals: [u32; 2],
    cancel: &CancelToken,
) -> Result<LineDiff, DiffError> {
    let mut walk = Walk {
        diff: LineDiff {
            left_lines: totals[0],
            right_lines: totals[1],
            ..LineDiff::default()
        },
        cur: (0, 0),
        kept: kept_lines,
    };
    // Next kept line of each side.
    let mut next = (0usize, 0usize);
    for hunk in &kept.hunks {
        walk.matches(&mut next, hunk.left.start as usize, cancel)?;
        walk.hunk(hunk);
        next = (hunk.left.end as usize, hunk.right.end as usize);
    }
    walk.matches(&mut next, walk.kept[0].len(), cancel)?;
    walk.gap(totals[0], totals[1]);
    Ok(walk.diff)
}

struct Walk<'a> {
    diff: LineDiff,
    /// The first line of each side not yet laid out.
    cur: (u32, u32),
    kept: [&'a [u32]; 2],
}

impl Walk<'_> {
    /// Lays out the ignored lines up to `to`: they pair one to one and the extras make a soft run.
    fn gap(&mut self, to_l: u32, to_r: u32) {
        if to_l - self.cur.0 != to_r - self.cur.1 {
            self.diff.soft.push(LineHunk {
                left: self.cur.0..to_l,
                right: self.cur.1..to_r,
            });
        }
        self.cur = (to_l, to_r);
    }

    /// The kept lines both sides share, up to kept line `upto` of the left side.
    fn matches(
        &mut self,
        next: &mut (usize, usize),
        upto: usize,
        cancel: &CancelToken,
    ) -> Result<(), DiffError> {
        while next.0 < upto {
            if next.0.is_multiple_of(65_536) {
                cancel.check().map_err(DiffError::Cancelled)?;
            }
            let (x, y) = (self.kept[0][next.0], self.kept[1][next.1]);
            self.gap(x, y);
            self.cur = (x + 1, y + 1);
            *next = (next.0 + 1, next.1 + 1);
        }
        Ok(())
    }

    /// A hunk over kept lines: it spans the first to the last kept line of each side, ignored
    /// lines between them included. A side with no kept line is empty at the line after the
    /// previous match.
    fn hunk(&mut self, hunk: &LineHunk) {
        let span = |kept: &[u32], range: &Range<u32>| {
            (!range.is_empty())
                .then(|| (kept[range.start as usize], kept[range.end as usize - 1] + 1))
        };
        let left = span(self.kept[0], &hunk.left);
        let right = span(self.kept[1], &hunk.right);
        self.gap(
            left.map_or(self.cur.0, |s| s.0),
            right.map_or(self.cur.1, |s| s.0),
        );
        let end = (
            left.map_or(self.cur.0, |s| s.1),
            right.map_or(self.cur.1, |s| s.1),
        );
        self.diff.hunks.push(LineHunk {
            left: self.cur.0..end.0,
            right: self.cur.1..end.1,
        });
        self.cur = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::diff::{LineHunk, WhitespaceMode};
    use crate::text::lines::LineIndex;
    use crate::text::rows::{RowKind, RowMap};
    use std::ops::Range;

    fn hk(l: Range<u32>, r: Range<u32>) -> LineHunk {
        LineHunk { left: l, right: r }
    }

    fn rules(blank: bool, text: &[&str], lines: &[&str]) -> TextRules {
        TextRules {
            ignore_blank: blank,
            text: text.iter().map(|p| regex::Regex::new(p).unwrap()).collect(),
            lines: lines
                .iter()
                .map(|p| regex::Regex::new(p).unwrap())
                .collect(),
            ..TextRules::default()
        }
    }

    fn comment_rules(lang: Language) -> TextRules {
        TextRules {
            comments: Some(lang),
            ..TextRules::default()
        }
    }

    fn run_rules(
        left: &str,
        right: &str,
        opts: DiffOptions,
        rules: &TextRules,
    ) -> (LineDiff, bool) {
        let (li, ri) = (
            LineIndex::new(left.as_bytes()),
            LineIndex::new(right.as_bytes()),
        );
        diff_with_rules(
            DiffInput {
                bytes: left.as_bytes(),
                lines: &li,
            },
            DiffInput {
                bytes: right.as_bytes(),
                lines: &ri,
            },
            opts,
            rules,
            &CancelToken::new(),
        )
        .unwrap()
    }

    fn plain(left: &str, right: &str, opts: DiffOptions) -> LineDiff {
        let (li, ri) = (
            LineIndex::new(left.as_bytes()),
            LineIndex::new(right.as_bytes()),
        );
        diff_lines(
            DiffInput {
                bytes: left.as_bytes(),
                lines: &li,
            },
            DiffInput {
                bytes: right.as_bytes(),
                lines: &ri,
            },
            opts,
            &CancelToken::new(),
        )
        .unwrap()
    }

    #[test]
    fn blank_lines_are_ignored_and_pair_as_unimportant() {
        let r = rules(true, &[], &[]);
        let (d, applied) = run_rules("a\n\nb\n", "a\nb\n", DiffOptions::default(), &r);
        assert!(applied);
        assert!(d.hunks.is_empty());
        assert_eq!(d.soft, vec![hk(1..2, 1..1)]);
        assert_eq!(d.soft_pairs, 0);
        let (d, _) = run_rules("a\n\nb\n", "a\n  \nb\n", DiffOptions::default(), &r);
        assert!(d.hunks.is_empty());
        assert!(d.soft.is_empty());
        assert_eq!(d.soft_pairs, 1);
    }

    #[test]
    fn a_line_rule_ignores_matching_lines() {
        let r = rules(false, &[], &["^#"]);
        let (d, _) = run_rules(
            "x\n# a\ny\n",
            "x\ny\n# b\n# c\n",
            DiffOptions::default(),
            &r,
        );
        assert!(d.hunks.is_empty());
        assert_eq!(d.soft, vec![hk(1..2, 1..1), hk(3..3, 2..4)]);
    }

    #[test]
    fn a_text_rule_cuts_matches_from_the_key() {
        let r = rules(false, &[r"\d{4}-\d{2}-\d{2}"], &[]);
        let (d, _) = run_rules(
            "log 2024-01-01 ok\n",
            "log 2025-02-03 ok\n",
            DiffOptions::default(),
            &r,
        );
        assert!(d.hunks.is_empty());
        assert_eq!(d.soft_pairs, 1);
        let (d, _) = run_rules(
            "log 2024-01-01 ok\n",
            "log 2025-02-03 fail\n",
            DiffOptions::default(),
            &r,
        );
        assert_eq!(d.hunks.len(), 1);
    }

    #[test]
    fn ignored_lines_go_back_where_localdiff_puts_them() {
        let r = rules(true, &[], &[]);
        let o = DiffOptions::default();
        let (d, _) = run_rules("a\nX\n\nY\nb\n", "a\nP\nQ\nb\n", o, &r);
        assert_eq!(d.hunks, vec![hk(1..4, 1..3)], "the blank line stays inside");
        assert!(d.soft.is_empty());
        let (d, _) = run_rules("\n\na\n", "a\n\n", o, &r);
        assert_eq!(d.soft, vec![hk(0..2, 0..0), hk(3..3, 1..2)]);
        let (d, _) = run_rules("", "\n", o, &r);
        assert_eq!(d.soft, vec![hk(0..0, 0..1)]);
    }

    #[test]
    fn rules_off_take_the_plain_path() {
        let pairs: [(&str, &str, WhitespaceMode); 5] = [
            ("a \n", "a\n", WhitespaceMode::Trailing),
            ("  a\n", "a\n", WhitespaceMode::Leading),
            ("a  b\n", "a b\n", WhitespaceMode::Amount),
            ("a  b\n", "a b\n", WhitespaceMode::Edges),
            ("a b\n", "ab\n", WhitespaceMode::All),
        ];
        for (l, r, whitespace) in pairs {
            let opts = DiffOptions {
                whitespace,
                ..DiffOptions::default()
            };
            assert_eq!(
                run_rules(l, r, opts, &TextRules::default()),
                (plain(l, r, opts), true),
                "{whitespace:?}"
            );
        }
    }

    #[test]
    fn rules_are_skipped_over_the_size_limit() {
        let r = rules(true, &[], &[]);
        let (left, right) = ("aaaaa\n", "aaaaa\n\n");
        let (li, ri) = (
            LineIndex::new(left.as_bytes()),
            LineIndex::new(right.as_bytes()),
        );
        let (l, rt) = (
            DiffInput {
                bytes: left.as_bytes(),
                lines: &li,
            },
            DiffInput {
                bytes: right.as_bytes(),
                lines: &ri,
            },
        );
        let got =
            diff_with_rules_limited(l, rt, DiffOptions::default(), &r, &CancelToken::new(), 4)
                .unwrap();
        assert_eq!(got, (plain(left, right, DiffOptions::default()), false));
        assert_eq!(
            got.0.hunks.len(),
            1,
            "the plain compare sees the extra blank line"
        );
    }

    fn anchored(left: &str, right: &str, anchors: &[(u32, u32)]) -> LineDiff {
        let r = TextRules {
            anchors: anchors.to_vec(),
            ..TextRules::default()
        };
        run_rules(left, right, DiffOptions::default(), &r).0
    }

    #[test]
    fn an_anchor_pairs_two_lines_the_diff_would_not() {
        let d = anchored(
            "a
b
c
",
            "c
a
b
",
            &[(2, 0)],
        );
        assert_eq!(d.hunks, vec![hk(0..2, 0..0), hk(3..3, 1..3)]);
        let d = anchored(
            "a
x
b
",
            "a
y
z
b
",
            &[(1, 2)],
        );
        assert_eq!(d.hunks, vec![hk(1..1, 1..2), hk(1..2, 2..3)]);
        let map = RowMap::new(&d);
        let rows: Vec<_> = (0..map.row_count())
            .map(|i| {
                let r = map.row(i);
                (r.kind, r.left, r.right)
            })
            .collect();
        assert_eq!(
            rows,
            vec![
                (RowKind::Same, Some(0), Some(0)),
                (RowKind::RightOnly, None, Some(1)),
                (RowKind::Changed, Some(1), Some(2)),
                (RowKind::Same, Some(2), Some(3)),
            ]
        );
    }

    #[test]
    fn anchors_that_cross_or_fall_outside_are_skipped() {
        let plain_hunks = anchored(
            "a
b
c
",
            "c
a
b
",
            &[],
        )
        .hunks;
        let both = anchored(
            "a
b
c
",
            "c
a
b
",
            &[(2, 0), (1, 1)],
        );
        assert_eq!(
            both,
            anchored(
                "a
b
c
",
                "c
a
b
",
                &[(1, 1)]
            )
        );
        assert_ne!(both.hunks, plain_hunks, "the usable anchor still applies");
        let past = anchored(
            "a
b
c
",
            "c
a
b
",
            &[(9, 0), (2, 7)],
        );
        assert_eq!(past.hunks, plain_hunks);
        let dup = anchored(
            "a
b
c
",
            "c
a
b
",
            &[(2, 0), (2, 1)],
        );
        assert_eq!(dup.hunks, vec![hk(0..2, 0..0), hk(3..3, 1..3)]);
        let r = rules(true, &[], &[]);
        let r = TextRules {
            anchors: vec![(1, 1)],
            ..r
        };
        let (d, _) = run_rules(
            "a

b
",
            "a
b
",
            DiffOptions::default(),
            &r,
        );
        let (plain_blank, _) = run_rules(
            "a

b
",
            "a
b
",
            DiffOptions::default(),
            &rules(true, &[], &[]),
        );
        assert_eq!(d, plain_blank, "an anchor on a blank line is dropped");
    }

    #[test]
    fn anchors_work_with_rules_on() {
        // Left line 3 ("c") is paired with right line 0; the "#" lines are ignored.
        let r = TextRules {
            lines: vec![regex::Regex::new("^#").unwrap()],
            anchors: vec![(3, 1)],
            ..TextRules::default()
        };
        let (d, _) = run_rules(
            "a
# x
b
c
",
            "# y
c
a
b
",
            DiffOptions::default(),
            &r,
        );
        assert_eq!(d.hunks, vec![hk(0..3, 0..0), hk(4..4, 2..4)]);
        assert_eq!(d.soft, vec![hk(3..3, 0..1)]);
    }

    #[test]
    fn invalid_patterns_are_named() {
        let err = compile_list("\\d+\n(oops\n").unwrap_err();
        assert_eq!(err.pattern, "(oops");
        assert!(err.to_string().starts_with("(oops: "), "{err}");
        assert!(!err.reason.contains('\n'), "one line: {:?}", err.reason);
        let ok = compile_list("  \n \\d+  \n").unwrap();
        assert_eq!(ok.len(), 1);
        assert_eq!(ok[0].as_str(), "\\d+");
        assert!(compile_list("(?<=a)b").is_err(), "no look-behind (R1)");
    }

    #[test]
    fn rules_keep_the_whitespace_case_and_line_ending_options() {
        let r = rules(true, &[], &[]);
        let (d, _) = run_rules("a\r\n", "a\n", DiffOptions::default(), &r);
        assert_eq!(d.hunks.len(), 1, "the line ending is part of the key");
        let eol = DiffOptions {
            ignore_eol: true,
            ..DiffOptions::default()
        };
        let (d, _) = run_rules("a\r\n", "a\n", eol, &r);
        assert!(d.hunks.is_empty());
        assert_eq!(d.soft_pairs, 0);
        let both = DiffOptions {
            whitespace: WhitespaceMode::Amount,
            ignore_case: true,
            ..DiffOptions::default()
        };
        let (d, _) = run_rules("A b\n", "a  B\n", both, &r);
        assert!(d.hunks.is_empty());
        assert_eq!(d.soft_pairs, 1);
    }

    #[test]
    fn comments_are_cut_and_comment_only_lines_are_ignored() {
        let o = DiffOptions::default();
        let r = comment_rules(Language::CLike);
        let (d, applied) = run_rules(
            "a = 1; // x\n/* note */\nb = 2;\n",
            "a = 1;\nb = 2;\n",
            o,
            &r,
        );
        assert!(applied);
        assert!(d.hunks.is_empty());
        assert_eq!(d.soft, vec![hk(1..2, 1..1)]);
        assert_eq!(d.soft_pairs, 1);
        let (d, _) = run_rules("x\n/* a\nb */\ny\n", "x\ny\n", o, &r);
        assert!(d.hunks.is_empty());
        assert_eq!(d.soft, vec![hk(1..3, 1..1)]);
        let json = comment_rules(Language::Json);
        let (d, _) = run_rules("a // b\n", "a\n", o, &json);
        assert_eq!(d.hunks.len(), 1, "JSON has no comments");
        assert_eq!(
            run_rules("a // b\n", "a\n", o, &json),
            (plain("a // b\n", "a\n", o), true),
            "a language without comments takes the plain path"
        );
    }

    #[test]
    fn the_lexer_state_runs_through_ignored_lines() {
        // Line 1 is ignored by a line rule, yet it closes the block comment, so "b" is code.
        let mut r = rules(false, &[], &["^#"]);
        r.comments = Some(Language::CLike);
        let (d, _) = run_rules(
            "a /* x\n# skip */\nb\n",
            "a\nb\n",
            DiffOptions::default(),
            &r,
        );
        assert!(d.hunks.is_empty());
        assert_eq!(d.soft, vec![hk(1..2, 1..1)]);
    }

    #[test]
    fn the_blank_test_follows_the_text_rules() {
        // PF20: a line left as only a timestamp by the comment cut, then emptied by a text
        // rule, is ignored (LocalDiff tests blankness after the text rules).
        let mut r = rules(false, &[r"\d{4}-\d{2}-\d{2}"], &[]);
        r.comments = Some(Language::CLike);
        let o = DiffOptions::default();
        let (d, _) = run_rules("x\n2024-01-01 // note\ny\n", "x\ny\n", o, &r);
        assert!(d.hunks.is_empty());
        assert_eq!(d.soft, vec![hk(1..2, 1..1)]);
        // Without a comment cut, a line the text rule empties still counts.
        let (d, _) = run_rules("x\n2024-01-01\ny\n", "x\ny\n", o, &r);
        assert_eq!(d.hunks, vec![hk(1..2, 1..1)]);
    }

    #[test]
    fn blank_means_the_localdiff_set() {
        assert!(is_blank(""));
        assert!(is_blank(" \t\u{a0}\u{3000}\u{c}\u{b}"));
        assert!(!is_blank(" x "));
        assert!(!is_blank("\u{2003}"), "em space is not in LD_BLANK_RE");
    }

    const LINES: [&str; 6] = ["a", "b", "", "  ", "#x", "#y"];

    fn lines_strategy() -> impl proptest::strategy::Strategy<Value = String> {
        proptest::collection::vec(proptest::sample::select(LINES.to_vec()), 0..12)
            .prop_map(|v| v.iter().map(|l| format!("{l}\n")).collect::<String>())
    }

    use proptest::strategy::Strategy as _;

    proptest::proptest! {
        #[test]
        fn every_line_keeps_exactly_one_row_with_rules(
            left in lines_strategy(),
            right in lines_strategy(),
        ) {
            let r = rules(true, &[], &["^#"]);
            let (d, applied) = run_rules(&left, &right, DiffOptions::default(), &r);
            proptest::prop_assert!(applied);
            // Hunks and soft runs are in order and never overlap on either side.
            let mut prev = (0u32, 0u32);
            for (_, h) in d.ordered() {
                proptest::prop_assert!(h.left.start >= prev.0 && h.right.start >= prev.1);
                proptest::prop_assert!(h.left.end <= d.left_lines && h.right.end <= d.right_lines);
                proptest::prop_assert!(h.left.start <= h.left.end && h.right.start <= h.right.end);
                proptest::prop_assert!(!(h.left.is_empty() && h.right.is_empty()));
                prev = (h.left.end, h.right.end);
            }
            let map = RowMap::new(&d);
            let (mut next_left, mut next_right) = (0u32, 0u32);
            let l_lines: Vec<&str> = left.split_inclusive('\n').collect();
            let r_lines: Vec<&str> = right.split_inclusive('\n').collect();
            for row in 0..map.row_count() {
                let row = map.row(row);
                proptest::prop_assert!(row.left.is_some() || row.right.is_some());
                if let Some(l) = row.left {
                    proptest::prop_assert_eq!(l, next_left);
                    next_left += 1;
                }
                if let Some(rt) = row.right {
                    proptest::prop_assert_eq!(rt, next_right);
                    next_right += 1;
                }
                // A Same row of two kept lines holds equal keys.
                if let (RowKind::Same, Some(l), Some(rt)) = (row.kind, row.left, row.right) {
                    let (a, b) = (l_lines[l as usize].trim_end(), r_lines[rt as usize].trim_end());
                    let kept = |s: &str| !is_blank(s) && !s.starts_with('#');
                    if kept(a) && kept(b) {
                        proptest::prop_assert_eq!(a, b);
                    }
                }
            }
            proptest::prop_assert_eq!((next_left, next_right), (d.left_lines, d.right_lines));
        }
    }
}
