//! What one side of a text row shows: tabs expanded, optional whitespace glyphs, a horizontal
//! scroll offset in columns, and a length cap so huge lines are never shaped whole. Inline
//! highlight ranges (byte ranges in the source line) are mapped onto the displayed text.

use std::ops::Range;

use duet_core::text::odd::{OddKind, is_odd, is_odd_after, odd_info};
use gpui_kit::HighlightStyle;
use unicode_segmentation::UnicodeSegmentation;

/// Displayed bytes per line; longer lines end with "…" (Review Focus 2).
pub const MAX_DISPLAY_BYTES: usize = 4096;

/// The most segments a wrapped line shows; the last ends with "…" when text remains (R3).
pub const MAX_WRAP_SEGMENTS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayOptions {
    pub tab_width: usize,
    pub show_whitespace: bool,
    /// Columns scrolled off the left edge (one column per grapheme, tabs to the next stop).
    pub skip_columns: usize,
    /// Text columns one half shows (0 = not measured yet).
    pub view_columns: usize,
}

impl Default for DisplayOptions {
    fn default() -> Self {
        Self {
            tab_width: 4,
            show_whitespace: false,
            skip_columns: 0,
            view_columns: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayLine {
    pub text: String,
    /// Columns reached where the line ends or is cut.
    pub end_column: usize,
    /// The line was longer than `MAX_DISPLAY_BYTES` and ends with "…".
    pub cut: bool,
    /// The display bytes of each invisible-character marker; true when it sits in the line's
    /// leading indentation.
    pub odd: Vec<(Range<usize>, bool)>,
    /// (source byte, display byte) at every shown grapheme start, then one end entry.
    points: Vec<(usize, usize)>,
}

/// A plain ASCII byte: always a grapheme of its own next to another plain one. CR is left out
/// (CR LF is one grapheme).
fn plain(byte: u8) -> bool {
    byte.is_ascii() && byte != b'\r'
}

/// Walks `text` from its start one grapheme at a time (a tab to the next stop of `tab_width`,
/// anything else one column) and stops before the first grapheme that would end past column
/// `limit`. Returns where it stopped: (byte, column).
///
/// A caret at the end of a minified line sits millions of columns in, and this runs on every
/// keystroke and frame, so plain ASCII is counted by the byte. Between two plain ASCII bytes
/// there is always a grapheme boundary; only the rest goes through grapheme segmentation, with
/// the plain byte on each side of it (a base before its marks, a letter after a Prepend).
pub fn walk_columns(text: &str, tab_width: usize, limit: usize) -> (usize, usize) {
    let tab = tab_width.max(1);
    let bytes = text.as_bytes();
    if text.is_ascii() && !bytes.contains(&b'\t') && !bytes.contains(&b'\r') {
        let n = text.len().min(limit);
        return (n, n);
    }
    let mut column = 0;
    let mut at = 0;
    while at < bytes.len() {
        let run = bytes[at..]
            .iter()
            .position(|&b| !plain(b))
            .map_or(bytes.len(), |i| at + i);
        // The run's last byte may be the base of what follows, so it goes with that.
        let ascii_end = if run < bytes.len() {
            run.saturating_sub(1).max(at)
        } else {
            run
        };
        if let Some(stop) = walk_ascii(&text[at..ascii_end], tab, limit, &mut column) {
            return (at + stop, column);
        }
        at = ascii_end;
        if at == bytes.len() {
            break;
        }
        let mut next = at + 1;
        while next < bytes.len() && !(plain(bytes[next - 1]) && plain(bytes[next])) {
            next += 1;
        }
        for (i, grapheme) in text[at..next].grapheme_indices(true) {
            let width = if grapheme == "\t" {
                tab - column % tab
            } else {
                1
            };
            if width > limit - column {
                return (at + i, column);
            }
            column += width;
        }
        at = next;
    }
    (text.len(), column)
}

/// `walk_columns` over plain ASCII, from `column`: one column a byte, a tab to its stop.
/// Returns the byte it stopped before, or None when all of `text` fits.
fn walk_ascii(text: &str, tab: usize, limit: usize, column: &mut usize) -> Option<usize> {
    let mut i = 0;
    while i < text.len() {
        let next_tab = text[i..].find('\t').map(|k| i + k);
        let end = next_tab.unwrap_or(text.len());
        let room = limit - *column;
        if end - i > room {
            *column += room;
            return Some(i + room);
        }
        *column += end - i;
        i = end;
        if let Some(t) = next_tab {
            let width = tab - *column % tab;
            if width > limit - *column {
                return Some(t);
            }
            *column += width;
            i = t + 1;
        }
    }
    None
}

/// Columns `text` takes on screen, as `display_line` counts them but never cut.
pub fn columns(text: &str, tab_width: usize) -> usize {
    walk_columns(text, tab_width, usize::MAX).1
}

/// What an invisible character shows as: "⍽" for one that looks like a space, "◇" for the
/// rest (R19). One column each, as the character itself counts, so wrapping stays put.
fn marker(ch: char) -> (char, bool) {
    if odd_info(ch).2 == OddKind::Space {
        ('⍽', true)
    } else {
        ('◇', false)
    }
}

/// Whether grapheme `g` can hold an invisible character: not plain printable ASCII.
fn may_be_odd(g: &str) -> bool {
    !g.is_ascii() || g.as_bytes().first().is_some_and(|&b| b < 0x20 || b == 0x7f)
}

/// The bytes of `line` before its first character that is neither a space, a tab nor an
/// invisible space-like one: the indentation.
fn indent_len(line: &str) -> usize {
    line.len()
        - line
            .trim_start_matches(|c| c == ' ' || c == '\t' || (is_odd(c) && marker(c).1))
            .len()
}

/// Pushes grapheme `g` (at source byte `at`) with each invisible character replaced by its
/// marker. A marker that follows a base character in the same grapheme (a stray joiner) has no
/// column of its own.
fn push_marked(
    text: &mut String,
    odd: &mut Vec<(Range<usize>, bool)>,
    source: &str,
    at: usize,
    g: &str,
    indent: &mut Option<usize>,
) {
    let mut prev = source[..at].chars().next_back();
    for ch in g.chars() {
        if is_odd_after(prev, ch) {
            let (mark, space) = marker(ch);
            let lead = *indent.get_or_insert_with(|| indent_len(source));
            let start = text.len();
            text.push(mark);
            odd.push((start..text.len(), space && at < lead));
        } else {
            text.push(ch);
        }
        prev = Some(ch);
    }
}

pub fn display_line(source: &str, opts: DisplayOptions) -> DisplayLine {
    let tab = opts.tab_width.max(1);
    let mut text = String::new();
    let mut points = Vec::new();
    let mut odd = Vec::new();
    let mut indent = None;
    // The columns scrolled off the left edge.
    let (skipped, mut column) = walk_columns(source, tab, opts.skip_columns);
    let mut cut_at = None;
    for (at, grapheme) in source[skipped..].grapheme_indices(true) {
        let at = skipped + at;
        let start = column;
        column += if grapheme == "\t" {
            tab - column % tab
        } else {
            1
        };
        if text.len() >= MAX_DISPLAY_BYTES {
            cut_at = Some(at);
            break;
        }
        points.push((at, text.len()));
        // A tab that crosses the left edge shows only its remaining columns.
        let shown = column - start.max(opts.skip_columns);
        match grapheme {
            "\t" if opts.show_whitespace => {
                text.push('→');
                text.extend(std::iter::repeat_n(' ', shown - 1));
            }
            "\t" => text.extend(std::iter::repeat_n(' ', shown)),
            " " if opts.show_whitespace => text.push('·'),
            other if may_be_odd(other) => {
                push_marked(&mut text, &mut odd, source, at, other, &mut indent);
            }
            other => text.push_str(other),
        }
    }
    points.push((cut_at.unwrap_or(source.len()), text.len()));
    if cut_at.is_some() {
        text.push('…');
    }
    DisplayLine {
        text,
        end_column: column,
        cut: cut_at.is_some(),
        odd,
        points,
    }
}

/// A Thai leading vowel (เ แ โ ใ ไ) is a grapheme of its own, but it is written before the
/// consonant it follows in speech: a line never breaks after one (PF14).
fn leads(grapheme: &str) -> bool {
    grapheme.starts_with(|c| ('\u{E40}'..='\u{E44}').contains(&c))
}

/// A Thai vowel or sign written after its consonant that is still a grapheme of its own (ฯ ะ า
/// ๅ ๆ): a line never breaks before one, which would leave it alone at the line's start.
fn follows(grapheme: &str) -> bool {
    grapheme.starts_with(['\u{E2F}', '\u{E30}', '\u{E32}', '\u{E45}', '\u{E46}'])
}

/// Byte offsets where each segment of `source` starts when its text is `width` columns wide
/// (R3); the first is 0, an empty line gives [0], at most `max` starts.
///
/// Columns count as `walk_columns` counts them, with tab stops from each segment's start. A
/// segment breaks after its last space or tab past its first grapheme, otherwise before the
/// grapheme that overflows: never inside a grapheme cluster, never after a Thai leading vowel
/// and never before a Thai following one. A segment's first grapheme always fits, however
/// wide. Width 0 means the columns are not measured yet: one segment. It walks at most `max`
/// segments of the line, never the rest.
pub fn wrap_starts(source: &str, tab_width: usize, width: usize, max: usize) -> Vec<usize> {
    let mut starts = vec![0];
    if width == 0 {
        return starts;
    }
    let (tab, max) = (tab_width.max(1), max.max(1));
    let (mut segment, mut column) = (0, 0);
    // Just after the segment's last space or tab past its first grapheme.
    let mut after_space = None;
    // Where the grapheme being placed may break before: before a leading vowel ahead of it.
    let (mut unit, mut joined) = (0, false);
    for (at, grapheme) in source.grapheme_indices(true) {
        if starts.len() >= max {
            break;
        }
        let follower = follows(grapheme);
        if !joined && !follower {
            unit = at;
        }
        joined = leads(grapheme);
        if follower && after_space == Some(at) {
            after_space = None;
        }
        loop {
            let wide = if grapheme == "\t" {
                tab - column % tab
            } else {
                1
            };
            if at == segment || column + wide <= width {
                column += wide;
                break;
            }
            let start = after_space.unwrap_or(unit);
            if start <= segment {
                // A leading vowel opens the segment: its consonant stays on the line too.
                column += wide;
                break;
            }
            starts.push(start);
            if starts.len() >= max {
                return starts;
            }
            (segment, after_space) = (start, None);
            column = columns(&source[segment..at], tab);
        }
        if at > segment && (grapheme == " " || grapheme == "\t") {
            after_space = Some(at + grapheme.len());
        }
    }
    starts
}

/// `source[range]` as one line whose tab stops count from `range.start`; its points map source
/// bytes of the whole line, so `map_range`, `display_at` and `source_at` take whole-line
/// offsets. A wrapped line never scrolls sideways, so `opts.skip_columns` is not used.
pub fn display_segment(source: &str, range: Range<usize>, opts: DisplayOptions) -> DisplayLine {
    let mut line = display_line(
        &source[range.clone()],
        DisplayOptions {
            skip_columns: 0,
            ..opts
        },
    );
    for point in &mut line.points {
        point.0 += range.start;
    }
    line
}

/// What a line wrapped at `width` columns shows, one `display_segment` a segment (R3). At most
/// `MAX_WRAP_SEGMENTS`: when text remains after them, the last is cut to fit "…" in `width`.
pub fn wrap_line(source: &str, opts: DisplayOptions, width: usize) -> Vec<DisplayLine> {
    wrap_prefix(source, false, opts, width)
}

/// `wrap_line` of the first bytes of a longer line when `more` is set: its last segment then
/// ends with "…" as a capped line's does (the details pane, I3).
pub fn wrap_prefix(
    source: &str,
    more: bool,
    opts: DisplayOptions,
    width: usize,
) -> Vec<DisplayLine> {
    // One start past the cap says that text remains, without walking the rest of the line.
    let starts = wrap_starts(source, opts.tab_width, width, MAX_WRAP_SEGMENTS + 1);
    let shown = starts.len().min(MAX_WRAP_SEGMENTS);
    let capped = more || starts.len() > MAX_WRAP_SEGMENTS;
    (0..shown)
        .map(|k| {
            let (start, end) = (
                starts[k],
                starts.get(k + 1).copied().unwrap_or(source.len()),
            );
            if capped && k + 1 == shown {
                let tab = opts.tab_width.max(1);
                let (fit, _) = walk_columns(&source[start..end], tab, width.saturating_sub(1));
                let mut cut = display_segment(source, start..start + fit, opts);
                cut.text.push('…');
                cut.cut = true;
                cut
            } else {
                display_segment(source, start..end, opts)
            }
        })
        .collect()
}

impl DisplayLine {
    pub fn map_range(&self, range: Range<usize>) -> Option<Range<usize>> {
        let to_display = |source: usize| {
            let i = self.points.partition_point(|&(s, _)| s < source);
            self.points.get(i).map(|&(_, d)| d)
        };
        let start = to_display(range.start)?;
        let end = to_display(range.end).unwrap_or(self.points.last()?.1);
        (start < end).then_some(start..end)
    }

    /// The source bytes the line shows: from the first column on screen to the end or the cut.
    pub fn source_span(&self) -> Range<usize> {
        let at = |p: Option<&(usize, usize)>| p.map_or(0, |&(source, _)| source);
        at(self.points.first())..at(self.points.last())
    }

    /// The display byte where source byte `source` (a grapheme start) is shown; None when it
    /// is scrolled off to the left or cut off at the right.
    pub fn display_at(&self, source: usize) -> Option<usize> {
        let i = self.points.partition_point(|&(s, _)| s < source);
        let (s, d) = *self.points.get(i)?;
        (s == source).then_some(d)
    }

    /// The source byte of the grapheme shown at display byte `display`; past the end, the
    /// line's length (or where it was cut).
    pub fn source_at(&self, display: usize) -> usize {
        let i = self.points.partition_point(|&(_, d)| d <= display);
        match i {
            0 => self.points.first().map_or(0, |p| p.0),
            i => self.points[i - 1].0,
        }
    }
}

/// Combines two sorted, non-overlapping highlight lists into one of the same shape, which
/// `StyledText::with_highlights` needs. Where they overlap, `top` is merged onto `base`
/// (`HighlightStyle::highlight`): a syntax color stays under a diff background, and `top`'s
/// own color or background wins.
pub fn layer(
    base: &[(Range<usize>, HighlightStyle)],
    top: &[(Range<usize>, HighlightStyle)],
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut points: Vec<usize> = base
        .iter()
        .chain(top)
        .flat_map(|(r, _)| [r.start, r.end])
        .collect();
    points.sort_unstable();
    points.dedup();
    // Both lists are sorted, so one cursor each walks them along with the points.
    let (mut b, mut t) = (0, 0);
    let style_at = |list: &[(Range<usize>, HighlightStyle)], i: &mut usize, at: usize| {
        while list.get(*i).is_some_and(|(r, _)| r.end <= at) {
            *i += 1;
        }
        list.get(*i).filter(|(r, _)| r.start <= at).map(|(_, s)| *s)
    };
    let mut out: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    for pair in points.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        let style = match (style_at(base, &mut b, start), style_at(top, &mut t, start)) {
            (Some(under), Some(over)) => Some(under.highlight(over)),
            (under, over) => over.or(under),
        };
        if let Some(style) = style {
            match out.last_mut() {
                Some((range, last)) if range.end == start && *last == style => range.end = end,
                _ => out.push((start..end, style)),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::Hsla;

    fn opts(tab_width: usize, show_whitespace: bool, skip_columns: usize) -> DisplayOptions {
        DisplayOptions {
            tab_width,
            show_whitespace,
            skip_columns,
            view_columns: 0,
        }
    }

    /// Pieces that join graphemes across their edges: combining and spacing marks, a Prepend
    /// character, ZWJ, emoji, regional indicators, CR and LF, tabs and plain ASCII.
    const PIECES: [&str; 13] = [
        "a", "\t", "\u{301}", "\u{600}", "\u{200D}", "👍", "🇹", "\r", "\n", "ก", "\u{E48}",
        "\u{E33}", "é",
    ];

    /// Every string of up to three pieces, alone and between ASCII runs.
    fn tricky_strings() -> Vec<String> {
        let mut out = vec![String::new()];
        let mut level = vec![String::new()];
        for _ in 0..3 {
            level = level
                .iter()
                .flat_map(|s| PIECES.iter().map(move |p| format!("{s}{p}")))
                .collect();
            out.extend(level.iter().cloned());
        }
        let wrapped: Vec<String> = out.iter().map(|s| format!("xy{s}zw")).collect();
        out.extend(wrapped);
        out.extend(
            level
                .iter()
                .map(|s| s.chars().map(|c| format!("ab{c}")).collect::<String>()),
        );
        out
    }

    /// `walk_columns` one grapheme at a time, as it was before its fast paths.
    fn reference_walk(text: &str, tab: usize, limit: usize) -> (usize, usize) {
        let mut column = 0;
        for (at, g) in text.grapheme_indices(true) {
            let width = if g == "\t" { tab - column % tab } else { 1 };
            if column + width > limit {
                return (at, column);
            }
            column += width;
        }
        (text.len(), column)
    }

    /// `display_line` as it was before the skip went through `walk_columns`.
    fn reference_line(source: &str, opts: DisplayOptions) -> DisplayLine {
        let tab = opts.tab_width.max(1);
        let mut text = String::new();
        let mut points = Vec::new();
        let mut column = 0usize;
        let mut cut_at = None;
        for (at, grapheme) in source.grapheme_indices(true) {
            let start = column;
            column += if grapheme == "\t" {
                tab - column % tab
            } else {
                1
            };
            if column <= opts.skip_columns {
                continue;
            }
            if text.len() >= MAX_DISPLAY_BYTES {
                cut_at = Some(at);
                break;
            }
            points.push((at, text.len()));
            let shown = column - start.max(opts.skip_columns);
            match grapheme {
                "\t" if opts.show_whitespace => {
                    text.push('→');
                    text.extend(std::iter::repeat_n(' ', shown - 1));
                }
                "\t" => text.extend(std::iter::repeat_n(' ', shown)),
                " " if opts.show_whitespace => text.push('·'),
                other => text.push_str(other),
            }
        }
        points.push((cut_at.unwrap_or(source.len()), text.len()));
        if cut_at.is_some() {
            text.push('…');
        }
        DisplayLine {
            text,
            end_column: column,
            cut: cut_at.is_some(),
            odd: Vec::new(),
            points,
        }
    }

    /// `display_line` against `reference_line`: the same, except that invisible characters
    /// show as markers, which the reference does not know (the markers have their own test).
    fn same_as_reference(s: &str, opts: DisplayOptions) {
        let (shown, reference) = (display_line(s, opts), reference_line(s, opts));
        if shown.odd.is_empty() {
            assert_eq!(shown, reference, "{s:?}, skip {}", opts.skip_columns);
        } else {
            assert_eq!(
                (shown.end_column, shown.cut),
                (reference.end_column, reference.cut),
                "{s:?}, skip {}",
                opts.skip_columns
            );
            let sources = |l: &DisplayLine| l.points.iter().map(|p| p.0).collect::<Vec<_>>();
            assert_eq!(sources(&shown), sources(&reference), "{s:?}");
        }
    }

    #[test]
    fn odd_characters_show_one_marker_each() {
        let o = DisplayOptions::default();
        let line = display_line("a\u{a0}b", o);
        assert_eq!(line.text, "a⍽b");
        assert_eq!(line.odd, vec![(1..4, false)]);
        assert_eq!(line.end_column, 3);
        // The marker maps like the character it replaces.
        assert_eq!(line.map_range(1..3), Some(1..4));
        let line = display_line("\u{200b}x", o);
        assert_eq!((line.text.as_str(), line.odd), ("◇x", vec![(0..3, false)]));
        let line = display_line("\u{a0}\u{a0}x", o);
        assert_eq!(line.text, "⍽⍽x");
        assert_eq!(line.odd, vec![(0..3, true), (3..6, true)]);
        let line = display_line("  \u{a0}x\u{a0}", o);
        assert_eq!(line.odd, vec![(2..5, true), (6..9, false)]);
        let line = display_line("\u{0}", o);
        assert_eq!((line.text.as_str(), line.odd.len()), ("◇", 1));
        let line = display_line("\tx", o);
        assert_eq!((line.text.as_str(), line.odd.len()), ("    x", 0));
        // Scrolled: the markers of the columns still shown, indentation read from the source.
        let line = display_line(
            "\u{a0}\u{a0}x",
            DisplayOptions {
                skip_columns: 1,
                ..o
            },
        );
        assert_eq!((line.text.as_str(), line.odd), ("⍽x", vec![(0..3, true)]));
    }

    #[test]
    fn a_joiner_after_a_visible_non_ascii_character_is_part_of_the_writing() {
        let o = DisplayOptions::default();
        // A family emoji and an Indic conjunct keep their joiners, marked or counted nowhere.
        for s in ["👨\u{200d}👩", "क\u{200d}ष"] {
            let line = display_line(s, o);
            assert_eq!((line.text.as_str(), line.odd.len()), (s, 0), "{s:?}");
        }
        // After ASCII it is a stray one: it gets a marker and no column of its own.
        let line = display_line("a\u{200d}b", o);
        assert_eq!(line.text, "a◇b");
        assert_eq!(line.odd, vec![(1..4, false)]);
        assert_eq!(line.end_column, 2);
    }

    /// E8: wrapping measures the raw source, so a marker must take as many columns as the
    /// character it replaces.
    #[test]
    fn markers_keep_the_columns_of_the_source() {
        for s in [
            "a\u{a0}b\u{200b}c",
            "\u{3000}\u{a0}x\u{0}y",
            "x\u{feff}y\u{2028}",
        ] {
            let line = display_line(s, DisplayOptions::default());
            assert_eq!(line.end_column, columns(s, 4), "{s:?}");
            assert_eq!(columns(&line.text, 4), columns(s, 4), "{s:?}");
            for width in 1..6 {
                assert_eq!(
                    wrap_starts(s, 4, width, 50).len(),
                    wrap_starts(&line.text, 4, width, 50).len(),
                    "{s:?} at {width}"
                );
            }
        }
    }

    #[test]
    fn column_walks_match_a_grapheme_by_grapheme_walk() {
        for s in tricky_strings() {
            for tab in [1, 4] {
                for limit in [0, 1, 2, 3, 5, 8, usize::MAX] {
                    assert_eq!(
                        walk_columns(&s, tab, limit),
                        reference_walk(&s, tab, limit),
                        "{s:?}, tab {tab}, limit {limit}"
                    );
                }
            }
        }
    }

    #[test]
    fn scrolled_lines_match_a_grapheme_by_grapheme_walk() {
        for s in tricky_strings() {
            for skip in [0, 1, 2, 3, 6] {
                let opts = opts(4, true, skip);
                same_as_reference(&s, opts);
            }
        }
        let long = format!("{}\tก่{}", "x".repeat(5_000), "y".repeat(5_000));
        for skip in [0, 4_990, 5_003, 9_000, 20_000] {
            let opts = opts(4, false, skip);
            same_as_reference(&long, opts);
        }
    }

    /// 8 MiB of ASCII on one line, as in a minified file.
    fn minified() -> String {
        "x".repeat(8 << 20)
    }

    #[test]
    fn a_line_scrolled_millions_of_columns_is_drawn_without_walking_them() {
        let line = minified();
        let started = std::time::Instant::now();
        let shown = display_line(&line, opts(4, false, line.len() - 10));
        let columns = columns(&line, 4);
        let took = started.elapsed();
        assert_eq!((shown.text.len(), columns), (10, line.len()));
        assert!(took < std::time::Duration::from_secs(1), "{took:?}");
    }

    #[test]
    fn tabs_expand_to_the_next_stop_and_whitespace_can_show() {
        assert_eq!(display_line("a\tb", opts(4, false, 0)).text, "a   b");
        assert_eq!(display_line("ab\tc", opts(4, false, 0)).text, "ab  c");
        assert_eq!(display_line("a\tb c", opts(4, true, 0)).text, "a→  b·c");
    }

    #[test]
    fn skipping_columns_scrolls_horizontally() {
        let line = display_line("abcdef", opts(4, false, 2));
        assert_eq!(line.text, "cdef");
        assert_eq!(
            line.map_range(0..3),
            Some(0..1),
            "only the visible part of a range remains"
        );
        assert_eq!(
            line.map_range(0..2),
            None,
            "a range fully scrolled off maps to nothing"
        );
        assert_eq!(line.map_range(4..6), Some(2..4));
    }

    #[test]
    fn thai_ranges_map_to_grapheme_boundaries_after_tabs_and_skip() {
        let source = "ก\tที่นี่";
        let thi = source.find("ที่").unwrap();
        let range = thi..thi + "ที่".len();
        let plain = display_line(source, opts(4, false, 0));
        assert_eq!(plain.text, "ก   ที่นี่");
        assert_eq!(&plain.text[plain.map_range(range.clone()).unwrap()], "ที่");
        let scrolled = display_line(source, opts(4, false, 1));
        assert_eq!(
            scrolled.text, "   ที่นี่",
            "the rest of the tab stays as spaces"
        );
        assert_eq!(&scrolled.text[scrolled.map_range(range).unwrap()], "ที่");
    }

    #[test]
    fn very_long_lines_are_cut_with_an_ellipsis() {
        let source = "x".repeat(5 * 1024 * 1024);
        let line = display_line(&source, DisplayOptions::default());
        assert!(line.text.len() <= MAX_DISPLAY_BYTES + '…'.len_utf8());
        assert!(line.text.ends_with('…'));
        assert_eq!(line.map_range(source.len() - 2..source.len()), None);
        assert_eq!(line.map_range(0..3), Some(0..3));
    }

    #[test]
    fn empty_and_end_of_line_ranges() {
        let line = display_line("", DisplayOptions::default());
        assert_eq!(line.text, "");
        assert_eq!(line.map_range(0..0), None);
        let line = display_line("abc", DisplayOptions::default());
        assert_eq!(line.map_range(1..3), Some(1..3));
    }

    #[test]
    fn lines_report_their_width_and_map_both_ways() {
        let line = display_line("a\tก่b", opts(4, false, 0));
        assert_eq!(line.text, "a   ก่b");
        assert_eq!(
            line.end_column, 6,
            "a, a tab to column 4, one Thai cluster, b"
        );
        assert!(!line.cut);
        let thai = "a\t".len();
        assert_eq!(line.display_at(thai), Some(4));
        assert_eq!(line.source_at(4), thai);
        assert_eq!(line.source_at(2), 1, "inside a tab's spaces is the tab");
        assert_eq!(line.source_at(99), "a\tก่b".len());
        let long = display_line(&"x".repeat(9_000), DisplayOptions::default());
        assert!(long.cut);
    }

    #[test]
    fn layered_highlights_split_where_they_overlap() {
        let style = |hue: f32| HighlightStyle {
            background_color: Some(gpui_kit::hsla(hue, 1., 0.5, 1.)),
            ..Default::default()
        };
        let (a, b) = (style(0.1), style(0.6));
        assert_eq!(layer(&[(0..4, a)], &[(2..6, b)]), [(0..2, a), (2..6, b)]);
        assert_eq!(
            layer(&[(0..10, a)], &[(3..5, b)]),
            [(0..3, a), (3..5, b), (5..10, a)]
        );
        assert_eq!(layer(&[], &[(1..2, b)]), [(1..2, b)]);
    }

    #[test]
    fn the_source_span_is_what_the_line_shows() {
        assert_eq!(
            display_line("abcdef", opts(4, false, 2)).source_span(),
            2..6
        );
        assert_eq!(display_line("ab", opts(4, false, 5)).source_span(), 2..2);
        let long = "x".repeat(9_000);
        let cut = display_line(&long, DisplayOptions::default());
        assert_eq!(cut.source_span(), 0..MAX_DISPLAY_BYTES);
    }

    /// The Thai leading vowels เ แ โ ใ ไ.
    const LEADING: [char; 5] = ['\u{E40}', '\u{E41}', '\u{E42}', '\u{E43}', '\u{E44}'];

    #[test]
    fn a_thai_following_vowel_never_opens_a_segment() {
        assert_eq!(
            wrap_starts("กากากา", 4, 3, 200),
            [0, 6, 12],
            "the line breaks before the consonant, not between it and its า"
        );
        let thai = "ภาษาไทยมีกาน้ำชาและกะทิ ต่าง ๆ ฯลฯ นานาๆ ครั้งๅ";
        for width in 1..=12 {
            let starts = wrap_starts(thai, 4, width, 200);
            for &start in &starts[1..] {
                assert!(
                    !thai[start..]
                        .starts_with(['\u{E2F}', '\u{E30}', '\u{E32}', '\u{E45}', '\u{E46}']),
                    "width {width}: {start} of {starts:?} opens with a following vowel"
                );
            }
        }
    }

    #[test]
    fn wrap_starts_break_after_spaces_and_never_inside_a_cluster() {
        assert_eq!(wrap_starts("aaaa bbbb cccc", 4, 6, 200), [0, 5, 10]);
        assert_eq!(wrap_starts("abcdefghij", 4, 4, 200), [0, 4, 8]);
        assert_eq!(wrap_starts("", 4, 10, 200), [0]);
        assert_eq!(wrap_starts("short", 4, 10, 200), [0]);
        assert_eq!(wrap_starts("a      b", 4, 3, 200), [0, 3, 6]);
        assert_eq!(
            wrap_starts("\tab", 4, 4, 200),
            [0, 1],
            "a tab that starts its segment is no place to break after"
        );
        assert_eq!(
            wrap_starts("น้ำน้ำน้ำ", 4, 2, 200),
            [0, 18],
            "each น้ำ is one grapheme of 9 bytes"
        );
        let thai = "ภาษาไทยที่มีวรรณยุกต์ ไม่ตัดกลางตัว";
        let boundaries: Vec<usize> = thai.grapheme_indices(true).map(|(i, _)| i).collect();
        for width in 1..=12 {
            let starts = wrap_starts(thai, 4, width, 200);
            assert_eq!(starts[0], 0, "width {width}");
            assert!(starts.windows(2).all(|w| w[0] < w[1]), "{starts:?}");
            for &start in &starts {
                assert!(
                    boundaries.contains(&start),
                    "width {width}: {start} of {starts:?} is inside a cluster"
                );
            }
            for &start in &starts[1..] {
                assert!(
                    !thai[..start].ends_with(LEADING),
                    "width {width}: {start} of {starts:?} breaks after a leading vowel"
                );
            }
        }
        assert_eq!(wrap_starts(&"x".repeat(50), 4, 10, 3), [0, 10, 20]);
        // PF14.
        assert_eq!(
            wrap_starts(&"x".repeat(300), 4, 0, 200),
            [0],
            "width 0: the columns are not measured yet"
        );
        assert_eq!(
            wrap_starts("ไทย", 4, 1, 200),
            [0, 6],
            "ไ starts the segment, so ท stays with it"
        );
        assert_eq!(
            wrap_starts("กไท", 4, 2, 200),
            [0, 3],
            "the break moves back before ไ"
        );
    }

    #[test]
    fn segments_show_their_own_part_with_tabs_from_their_start() {
        let segment = display_segment("ab\tcd", 2..5, opts(4, false, 0));
        assert_eq!(segment.text, "    cd");
        assert_eq!(segment.end_column, 6);
        assert_eq!(segment.map_range(3..5), Some(4..6));
        assert_eq!(segment.source_at(4), 3);
        assert_eq!(segment.display_at(0), None, "before the segment");
        assert_eq!(segment.source_span(), 2..5, "whole-line offsets");
        assert_eq!(
            display_segment("ab\tcd", 2..5, opts(4, false, 3)).text,
            "    cd",
            "a wrapped line never scrolls sideways"
        );
    }

    #[test]
    fn a_wrapped_line_shows_at_most_its_cap_and_ends_with_an_ellipsis() {
        let opts = opts(4, false, 0);
        let texts = |segments: &[DisplayLine]| -> Vec<String> {
            segments.iter().map(|s| s.text.clone()).collect()
        };
        let short = wrap_line("aaaa bbbb cccc", opts, 6);
        assert_eq!(texts(&short), ["aaaa ", "bbbb ", "cccc"]);
        assert_eq!(short[1].source_span(), 5..10);
        assert!(short.iter().all(|s| !s.cut));
        assert_eq!(texts(&wrap_line("", opts, 6)), [""]);

        let exact = "x".repeat(MAX_WRAP_SEGMENTS * 10);
        let segments = wrap_line(&exact, opts, 10);
        assert_eq!(segments.len(), MAX_WRAP_SEGMENTS);
        let last = segments.last().unwrap();
        assert_eq!((last.text.as_str(), last.cut), ("xxxxxxxxxx", false));

        let long = "x".repeat(MAX_WRAP_SEGMENTS * 10 + 5);
        let segments = wrap_line(&long, opts, 10);
        assert_eq!(segments.len(), MAX_WRAP_SEGMENTS);
        let last = segments.last().unwrap();
        assert_eq!(
            last.text,
            format!("{}…", "x".repeat(9)),
            "the rest does not fit, so the last segment is cut to fit its ellipsis"
        );
        assert!(last.cut);
        let start = (MAX_WRAP_SEGMENTS - 1) * 10;
        assert_eq!(last.source_span(), start..start + 9);

        // The first bytes of a longer line (I3): the last segment says that text follows.
        let cut = wrap_prefix("xxxxxxxxxx", true, opts, 4);
        assert_eq!(texts(&cut), ["xxxx", "xxxx", "xx…"]);
        assert!(cut.last().unwrap().cut);
        assert_eq!(
            texts(&wrap_prefix("xxxxxxxx", true, opts, 4)),
            ["xxxx", "xxx…"]
        );
        assert_eq!(
            wrap_prefix(&long, false, opts, 10),
            wrap_line(&long, opts, 10)
        );
    }

    #[test]
    fn layering_keeps_the_syntax_color_under_a_diff_background() {
        let red: Hsla = gpui_kit::red();
        let bg: Hsla = gpui_kit::blue();
        let base = vec![(
            0..4,
            HighlightStyle {
                color: Some(red),
                ..Default::default()
            },
        )];
        let top = vec![(
            2..6,
            HighlightStyle {
                background_color: Some(bg),
                ..Default::default()
            },
        )];
        let merged = layer(&base, &top);
        assert_eq!(merged.len(), 3);
        assert_eq!(
            merged[0],
            (
                0..2,
                HighlightStyle {
                    color: Some(red),
                    ..Default::default()
                }
            )
        );
        assert_eq!(merged[1].1.color, Some(red));
        assert_eq!(merged[1].1.background_color, Some(bg));
        assert_eq!(
            merged[2],
            (
                4..6,
                HighlightStyle {
                    background_color: Some(bg),
                    ..Default::default()
                }
            )
        );
    }
}
