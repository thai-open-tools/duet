//! One aligned row's halves (spec 6.4, item 5): an opaque line-number cell coloured by status,
//! then the text on its tint or the hatch. The current difference's number turns accent with a
//! 2 px bar; ≈ rows get a 3 px purple edge.

use std::borrow::Cow;

use duet_core::text::buffer::Change;
use duet_core::text::inline::InlineDiff;
use duet_core::text::rows::Row;
use gpui_kit::{Background, Hsla, StyledText, TextLayout, UnderlineStyle, quad, transparent_black};

use super::stage::{NUMBER_CELL, TEXT_INSET};
use super::*;
use crate::i18n::{tr, tr_key, trf};
use crate::text::display::{DisplayLine, MAX_WRAP_SEGMENTS, display_line, layer, wrap_line};
use crate::ui::fonts::NUMBER_SIZE;
use crate::ui::style::{ld_ghost, ld_tip};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HalfFill {
    None,
    Tint(Hsla),
    /// The empty side opposite an insertion or deletion.
    Hatch,
}

/// How one half of a row is coloured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HalfLook {
    /// Behind the text only; the number cell has its own colour.
    pub fill: HalfFill,
    /// The number cell's background; None leaves the pane's panel colour.
    pub number_bg: Option<Hsla>,
    pub number: Hsla,
    /// Inline character highlights: what changed, or what an ignore option hid.
    pub highlight: Option<Hsla>,
    /// The 3 px left edge of a ≈ row.
    pub edge: Option<Hsla>,
    /// The 2 px bar on the number cell's right edge, down the current difference.
    pub bar: Option<Hsla>,
}

pub fn half_look(
    kind: RowKind,
    left: bool,
    moved: bool,
    unimportant: bool,
    current: bool,
    selected: bool,
    p: &Palette,
) -> HalfLook {
    let plain = HalfLook {
        fill: HalfFill::None,
        number_bg: None,
        number: p.dim,
        highlight: None,
        edge: None,
        bar: None,
    };
    let tinted = |fill, number_bg, number| HalfLook {
        fill: HalfFill::Tint(fill),
        number_bg: Some(number_bg),
        number,
        ..plain
    };
    let mut look = match (kind, left) {
        (RowKind::Same, _) => plain,
        (RowKind::Changed, _) => HalfLook {
            highlight: Some(if left { p.hl_del } else { p.hl_ins }),
            ..tinted(p.bg_mod, p.ln_mod, p.modified)
        },
        (RowKind::LeftOnly, true) => tinted(p.bg_del, p.ln_del, p.del),
        (RowKind::RightOnly, false) => tinted(p.bg_ins, p.ln_ins, p.ins),
        // A line a rule paired with nothing: unimportant, with the hatch opposite (R3).
        (RowKind::SoftLeft, true) | (RowKind::SoftRight, false) => HalfLook {
            fill: HalfFill::Tint(p.bg_unimp),
            number: p.unimp,
            edge: Some(p.unimp),
            ..plain
        },
        _ => HalfLook {
            fill: HalfFill::Hatch,
            ..plain
        },
    };
    if moved && look.fill != HalfFill::Hatch {
        look = HalfLook {
            highlight: look.highlight,
            ..tinted(p.bg_moved, p.ln_mov, p.moved)
        };
    }
    if unimportant {
        look.fill = HalfFill::Tint(p.bg_unimp);
        look.highlight = Some(p.hl_unimp);
        look.edge = Some(p.unimp);
        look.number = p.unimp;
    }
    if current {
        look.number = p.accent;
        look.bar = Some(p.accent);
    }
    if selected {
        look.number_bg = Some(p.sel);
    }
    look
}

/// The geometry every row of a frame is drawn with, from the view's font size (R4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowMetrics {
    /// One line's height in px.
    pub row_height: f32,
    /// The code font's size in px.
    pub font_size: f32,
    /// Lines wrap at the half's columns, one segment a line (R3).
    pub wrap: bool,
}

/// A row the diff calls the same while its text differs: an ignore option or a rule hid the
/// change.
pub fn unimportant(kind: RowKind, ignoring: bool, left: Option<&str>, right: Option<&str>) -> bool {
    kind == RowKind::Same && ignoring && left != right
}

/// The full column counts of lines too long to show whole, which size the horizontal
/// scrollbar. A row shows at most `MAX_DISPLAY_BYTES` of a line, so its own count stops at the
/// cut, and walking a whole line of 2 MiB costs up to about 20 ms when it is not ASCII.
///
/// So a frame never walks one: it reads the count kept here, and a line without a current
/// count is counted off the UI thread (`TextView::count_line_widths`) while the frame makes do
/// with what it knows. An edit marks only the lines it changed as stale, keeping their old
/// count until the new one lands, and moves the counts of the lines after it.
#[derive(Default)]
pub(crate) struct LineWidths {
    lines: HashMap<(bool, u32), LineWidth>,
    /// Lines a frame needed counted, in the order asked.
    wanted: Vec<(bool, u32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineWidth {
    columns: usize,
    tab_width: usize,
    /// Counted from the line's current text with this tab width.
    current: bool,
}

impl LineWidths {
    /// Long lines are rare; past this many the list starts again rather than grow.
    const MAX: usize = 1024;
    /// An edit that leaves more lines than this keeps stale counts for the first ones only.
    const MAX_STALE: u32 = 16;

    /// The full width of line `line` of side `left`, shown up to column `shown`, as far as it is
    /// known, never walking the line. A line without a current count is queued to be counted.
    pub(crate) fn columns(&mut self, key: (bool, u32), tab_width: usize, shown: usize) -> usize {
        match self.lines.get(&key) {
            Some(w) if w.current && w.tab_width == tab_width => w.columns,
            Some(w) => {
                let known = w.columns.max(shown);
                self.want(key);
                known
            }
            None => {
                if self.lines.len() >= Self::MAX {
                    self.lines.clear();
                }
                let stale = LineWidth {
                    columns: 0,
                    tab_width,
                    current: false,
                };
                self.lines.insert(key, stale);
                self.want(key);
                shown
            }
        }
    }

    fn want(&mut self, key: (bool, u32)) {
        if !self.wanted.contains(&key) {
            self.wanted.push(key);
        }
    }

    /// The lines to count off the UI thread, emptying the queue.
    pub(crate) fn take_wanted(&mut self) -> Vec<(bool, u32)> {
        std::mem::take(&mut self.wanted)
    }

    /// A count from the background, for the line's current text.
    pub(crate) fn counted(&mut self, key: (bool, u32), tab_width: usize, columns: usize) {
        let width = LineWidth {
            columns,
            tab_width,
            current: true,
        };
        self.lines.insert(key, width);
    }

    /// Lines `change.line..+old_lines` of side `left` became `+new_lines`: their counts go
    /// stale but stay as the best guess, and the lines after them move with the edit. The
    /// other side and the lines before the edit keep their counts.
    pub(crate) fn edited(&mut self, left: bool, change: &Change) {
        let (start, old_end) = (change.line, change.line + change.old_lines);
        let delta = i64::from(change.new_lines) - i64::from(change.old_lines);
        let mut replaced: Option<LineWidth> = None;
        let lines = std::mem::take(&mut self.lines);
        for ((side, line), width) in lines {
            let line = match line {
                _ if side != left || line < start => line,
                _ if line < old_end => {
                    let widest = replaced.map_or(0, |r| r.columns).max(width.columns);
                    replaced = Some(LineWidth {
                        columns: widest,
                        ..width
                    });
                    continue;
                }
                _ => (i64::from(line) + delta) as u32,
            };
            self.lines.insert((side, line), width);
        }
        if let Some(width) = replaced {
            let stale = LineWidth {
                current: false,
                ..width
            };
            for line in start..start + change.new_lines.min(Self::MAX_STALE) {
                self.lines.insert((left, line), stale);
            }
        }
        // Queued lines of this side may have moved; the next frame asks again.
        self.wanted.retain(|&(side, _)| side != left);
    }

    /// Lines changed under the counts: a new compare, or F9.
    pub(crate) fn clear(&mut self) {
        self.lines.clear();
        self.wanted.clear();
    }
}

/// What every row of one frame shares. `render_rows` and `half_look_for_test` both build the
/// halves through it, so the test sees the look the rows draw.
pub(super) struct RowFrame<'a> {
    caret: Option<Caret>,
    /// The rows picked on the line numbers.
    row_sel: Option<RowSel>,
    /// An ignore option or a rule is on, so a row the diff matched may still differ.
    ignoring: bool,
    /// The pinned line pairs, which get an anchor mark on their numbers.
    anchors: Vec<(u32, u32)>,
    find: Option<(&'a FindResult, Option<usize>)>,
    syntax: &'a [Option<SyntaxSide>; 2],
    theme: &'a HighlightTheme,
    metrics: RowMetrics,
}

impl<'a> RowFrame<'a> {
    pub(super) fn new(
        request: &TextRequest,
        (caret, row_sel): (Option<Caret>, Option<RowSel>),
        find: Option<&'a FindBar>,
        syntax: &'a [Option<SyntaxSide>; 2],
        theme: &'a HighlightTheme,
        metrics: RowMetrics,
    ) -> Self {
        Self {
            caret,
            row_sel,
            ignoring: request.ignoring(),
            anchors: request.rules.anchors.clone(),
            find: find.and_then(|bar| bar.result.as_ref().map(|r| (r, bar.current))),
            syntax,
            theme,
            metrics,
        }
    }

    /// The inline highlights the model refines for RowMap row `index`: a changed pair's, or
    /// what an ignore option or a rule hid on a pair it matched.
    pub(super) fn inline(
        &self,
        model: &mut TextModel,
        index: u32,
        kind: RowKind,
    ) -> Option<Arc<InlineDiff>> {
        match kind {
            RowKind::Changed => model.inline_at(index),
            RowKind::Same if self.ignoring => model.hidden_inline_at(index),
            _ => None,
        }
    }

    /// Both halves of RowMap row `index` (`row`), drawn at display row `display`. Each line's
    /// text is fetched once, here.
    pub(super) fn halves<'b>(
        &'b self,
        model: &'b TextModel,
        display: usize,
        index: u32,
        row: Row,
        inline: Option<&'b InlineDiff>,
    ) -> [Half<'b>; 2] {
        let text = |left: bool, line: Option<u32>| line.map(|l| model.line_text(left, l as usize));
        let (left_text, right_text) = (text(true, row.left), text(false, row.right));
        let soft = unimportant(
            row.kind,
            self.ignoring,
            left_text.as_deref(),
            right_text.as_deref(),
        );
        let current = model.row_in_current_block(index);
        let any_moves = !model.moves().is_empty();
        let make = |pane: Pane, line: Option<u32>, text: Option<Cow<'b, str>>| {
            let left = pane == Pane::Left;
            let (caret, selection, selected) = self.caret_in(model, left, line);
            let (found, current_match) = self.found(index, left);
            let anchored = line.is_some_and(|l| {
                self.anchors
                    .iter()
                    .any(|&(a, b)| (if left { a } else { b }) == l)
            });
            Half {
                text,
                line,
                kind: row.kind,
                pane,
                inline,
                current,
                found,
                current_match,
                moved: if any_moves {
                    model.moved_partner_of(row, left)
                } else {
                    None
                },
                unimportant: soft,
                anchored,
                selected,
                row_selected: self
                    .row_sel
                    .is_some_and(|s| s.left == left && s.rows().contains(&index)),
                row: display,
                caret,
                selection,
                syntax: self.syntax_line(model, left, line),
                metrics: self.metrics,
            }
        };
        [
            make(Pane::Left, row.left, left_text),
            make(Pane::Right, row.right, right_text),
        ]
    }

    /// The caret's byte and the selected bytes within `line` of one side, and whether the
    /// selection reaches the line (an empty line inside it, or only its line break, included).
    fn caret_in(
        &self,
        model: &TextModel,
        left: bool,
        line: Option<u32>,
    ) -> (Option<usize>, Option<Range<usize>>, bool) {
        let (Some(c), Some(line)) = (self.caret, line) else {
            return (None, None, false);
        };
        if c.left != left {
            return (None, None, false);
        }
        let Some(buffer) = model.buffer(left) else {
            return (None, None, false);
        };
        let (start, end) = (
            buffer.line_start(line as usize),
            buffer.line_end(line as usize),
        );
        let head = (start..=end).contains(&c.head).then(|| c.head - start);
        let selection = c.selection();
        let selected = selection
            .as_ref()
            .is_some_and(|r| r.start <= end && r.end > start);
        let selection = selection.and_then(|r| {
            let (a, b) = (r.start.max(start), r.end.min(end));
            (a < b).then(|| a - start..b - start)
        });
        (head, selection, selected)
    }

    /// Every find match on one side of RowMap row `index`, in order, and the current one.
    fn found(&self, index: u32, left: bool) -> (Vec<Range<usize>>, Option<Range<usize>>) {
        let (mut found, mut current) = (Vec::new(), None);
        if let Some((result, at)) = self.find {
            for (i, hit) in result.on_row(index, left) {
                if Some(i) == at {
                    current = Some(hit.range.clone());
                }
                found.push(hit.range.clone());
            }
        }
        (found, current)
    }

    fn syntax_line(
        &self,
        model: &TextModel,
        left: bool,
        line: Option<u32>,
    ) -> Option<SyntaxLine<'a>> {
        Some(SyntaxLine {
            side: self.syntax[usize::from(!left)].as_ref()?,
            start: model.line_offset(left, line? as usize),
            theme: self.theme,
        })
    }
}

/// What one half of a row draws.
pub(super) struct Half<'a> {
    /// The line's text; None on the empty side opposite an insertion or deletion.
    text: Option<Cow<'a, str>>,
    line: Option<u32>,
    kind: RowKind,
    pane: Pane,
    inline: Option<&'a InlineDiff>,
    /// The row is in the current difference block.
    pub(super) current: bool,
    /// Every find match in this line, in order.
    found: Vec<Range<usize>>,
    /// The current find match, when it is in this line: outlined as well.
    current_match: Option<Range<usize>>,
    /// The other side's line this one moved to or from.
    moved: Option<u32>,
    /// ≈: the diff matched the row while an ignore option or a rule hid how its text differs.
    unimportant: bool,
    /// A manual alignment pins this line to one on the other side.
    anchored: bool,
    /// The caret's selection reaches this line.
    selected: bool,
    /// The row is in the row selection on the line numbers.
    row_selected: bool,
    /// The display row, for element ids such as `("text-cut-left", row)`.
    row: usize,
    /// The caret's byte within this line, if it is here.
    caret: Option<usize>,
    /// Selected bytes within this line.
    selection: Option<Range<usize>>,
    /// Syntax colors, when this side's parse is ready.
    syntax: Option<SyntaxLine<'a>>,
    metrics: RowMetrics,
}

impl Half<'_> {
    pub(super) fn look(&self, p: &Palette) -> HalfLook {
        let mut look = half_look(
            self.kind,
            self.pane == Pane::Left,
            self.moved.is_some(),
            self.unimportant,
            self.current,
            self.selected,
            p,
        );
        if self.row_selected {
            look.number_bg = Some(p.sel);
            look.number = p.text;
        }
        look
    }
}

/// Where one line starts in its side's parsed text, for its syntax colors.
struct SyntaxLine<'a> {
    side: &'a SyntaxSide,
    start: usize,
    theme: &'a HighlightTheme,
}

/// A find hit's highlight: the find fill, with the text in the plain text colour, since a
/// syntax colour on the fill can be hard to read. The dark fill is faint against its row, so
/// an underline in the current match's accent marks every hit as well.
pub(super) fn find_hit_style(p: &Palette) -> HighlightStyle {
    HighlightStyle {
        color: Some(p.text),
        background_color: Some(p.find),
        underline: Some(UnderlineStyle {
            thickness: px(2.),
            color: Some(p.accent),
            wavy: false,
        }),
        ..Default::default()
    }
}

/// An invisible character's marker: red on a red tint, or dim when it is only indentation.
pub(super) fn odd_style(p: &Palette, lead: bool) -> HighlightStyle {
    if lead {
        HighlightStyle {
            color: Some(p.dim),
            ..Default::default()
        }
    } else {
        HighlightStyle {
            color: Some(p.del),
            background_color: Some(p.bg_del),
            ..Default::default()
        }
    }
}

/// `layer`, without the merge and its allocations when either list is empty (most rows).
fn stack(
    base: Vec<(Range<usize>, HighlightStyle)>,
    top: Vec<(Range<usize>, HighlightStyle)>,
) -> Vec<(Range<usize>, HighlightStyle)> {
    if top.is_empty() {
        base
    } else if base.is_empty() {
        top
    } else {
        layer(&base, &top)
    }
}

/// The layers one shown line of half `h` draws: syntax at the bottom, then the inline diff,
/// find hits and the selection.
fn highlights(
    h: &Half<'_>,
    shown: &DisplayLine,
    look: &HalfLook,
    p: &Palette,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let left = h.pane == Pane::Left;
    // Only the bytes the line shows are queried, so a long minified line stays cheap.
    let syntax = h.syntax.as_ref().map_or_else(Vec::new, |s| {
        let span = shown.source_span();
        s.side
            .line_styles(s.start + span.start, s.start + span.end, s.theme)
            .into_iter()
            .filter_map(|(r, style)| {
                let r = r.start + span.start..r.end + span.start;
                shown.map_range(r).map(|r| (r, style))
            })
            .collect::<Vec<_>>()
    });
    let background = |r: Range<usize>, color: Option<Hsla>| {
        (
            r,
            HighlightStyle {
                background_color: color,
                ..Default::default()
            },
        )
    };
    let ranges = h
        .inline
        .map(|d| if left { &d.left } else { &d.right })
        .into_iter()
        .flatten()
        .filter_map(|r| shown.map_range(r.clone()))
        .map(|r| background(r, look.highlight))
        .collect::<Vec<_>>();
    let marks = shown
        .odd
        .iter()
        .map(|(r, lead)| (r.clone(), odd_style(p, *lead)))
        .collect::<Vec<_>>();
    let hit = find_hit_style(p);
    let top = h
        .found
        .iter()
        .filter_map(|r| shown.map_range(r.clone()))
        .map(|r| (r, hit))
        .collect::<Vec<_>>();
    let selected = h
        .selection
        .clone()
        .and_then(|r| shown.map_range(r))
        .map(|r| background(r, Some(p.sel)));
    stack(
        stack(stack(stack(syntax, marks), ranges), top),
        selected.into_iter().collect(),
    )
}

/// The text one half drew this frame: for hit-testing, the caret and the IME.
pub(crate) struct RowText {
    pub layout: TextLayout,
    pub shown: DisplayLine,
    pub line: u32,
}

/// What one half drew: its line, or a wrapped line's segments in order (R3). An unwrapped half
/// keeps its one line inline, with no Vec (PF15).
pub(crate) enum HalfText {
    One(RowText),
    Many(Vec<RowText>),
}

impl HalfText {
    pub fn lines(&self) -> &[RowText] {
        match self {
            Self::One(text) => std::slice::from_ref(text),
            Self::Many(texts) => texts,
        }
    }

    /// The source line every segment shows a part of.
    pub(crate) fn line(&self) -> Option<u32> {
        self.lines().first().map(|t| t.line)
    }

    /// The line to hit-test window point `at` on, and the point to give it: the last segment
    /// whose top is at or above `at`, with `at` moved inside it.
    pub(crate) fn under(&self, at: Point<Pixels>) -> Option<(&RowText, Point<Pixels>)> {
        match self {
            Self::One(text) => Some((text, at)),
            Self::Many(texts) => {
                let text = texts
                    .iter()
                    .rev()
                    .find(|t| t.layout.bounds().top() <= at.y)
                    .or(texts.first())?;
                let bounds = text.layout.bounds();
                Some((
                    text,
                    point(at.x, at.y.max(bounds.top()).min(bounds.bottom())),
                ))
            }
        }
    }

    /// The line that shows source byte `byte` (a grapheme start) and its display byte there. A
    /// segment ends where the next one starts, which shows that byte; the last shows the end.
    pub(crate) fn holding(&self, byte: usize) -> Option<(&RowText, usize)> {
        let lines = self.lines();
        lines.iter().enumerate().find_map(|(k, t)| {
            let at = t.shown.display_at(byte)?;
            (k + 1 == lines.len() || byte < t.shown.source_span().end).then_some((t, at))
        })
    }
}

/// One shown line of a half, ready to draw: its text with every highlight, where the caret and
/// the current find match fall in it, and what clicks hit-test.
struct Drawn {
    styled: StyledText,
    caret_at: Option<(TextLayout, usize)>,
    outline: Option<(TextLayout, Range<usize>)>,
    text: Option<RowText>,
}

/// `shown`, a line of half `h`. `last`: the line's end is in `shown`, so a caret there shows on
/// it; a wrapped segment ends where the next one starts, and that one shows the caret.
fn draw_line(h: &Half<'_>, shown: DisplayLine, look: &HalfLook, p: &Palette, last: bool) -> Drawn {
    let styled =
        StyledText::new(shown.text.clone()).with_highlights(highlights(h, &shown, look, p));
    let layout = styled.layout().clone();
    // A caret at the line's end maps through the final point, past the last byte.
    let caret_at = h
        .caret
        .filter(|&c| last || c < shown.source_span().end)
        .and_then(|c| shown.display_at(c))
        .map(|d| (layout.clone(), d));
    let outline = h
        .current_match
        .clone()
        .and_then(|r| shown.map_range(r))
        .map(|r| (layout.clone(), r));
    let text = h.line.map(|line| RowText {
        layout,
        shown,
        line,
    });
    Drawn {
        styled,
        caret_at,
        outline,
        text,
    }
}

/// The caret and the current find match's 2 px accent outline, painted over `line`, the
/// element that holds their text.
fn marks(
    line: Div,
    caret_at: Option<(TextLayout, usize)>,
    outline: Option<(TextLayout, Range<usize>)>,
    p: &Palette,
    row_height: f32,
) -> Div {
    line.when_some(caret_at, |d, (layout, display_ix)| {
        let color = p.text;
        d.child(
            canvas(
                |_, _, _| {},
                move |_, (), window, _| {
                    if let Some(p) = layout.position_for_index(display_ix) {
                        window.paint_quad(fill(
                            Bounds::new(
                                point(p.x, p.y + px(2.)),
                                size(px(2.), px(row_height - 4.)),
                            ),
                            color,
                        ));
                    }
                },
            )
            .absolute()
            .size_full(),
        )
    })
    // Painted like the caret, only on the line that holds the match.
    .when_some(outline, |d, (layout, range)| {
        let accent = p.accent;
        d.child(
            canvas(
                |_, _, _| {},
                move |_, (), window, _| {
                    if let (Some(a), Some(b)) = (
                        layout.position_for_index(range.start),
                        layout.position_for_index(range.end),
                    ) {
                        window.paint_quad(quad(
                            Bounds::from_corners(
                                point(a.x - px(1.), a.y + px(1.)),
                                point(b.x + px(1.), a.y + px(row_height - 1.)),
                            ),
                            px(2.),
                            transparent_black(),
                            px(2.),
                            accent,
                            BorderStyle::Solid,
                        ));
                    }
                },
            )
            .absolute()
            .size_full(),
        )
    })
}

/// The tag after a moved line's text, "↷ N" on the left and "↶ N" on the right, N being the
/// partner's 1-based line (R12). A press starts a move drag; a click goes to the partner.
fn moved_tag(
    left: bool,
    row: usize,
    partner: u32,
    down: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    p: &Palette,
) -> AnyElement {
    let (id, arrow, tip) = if left {
        (
            "text-moved-left",
            '↷',
            tr_key(
                "This line is on the right at line {}. Click to go there; drag to move this line.",
            ),
        )
    } else {
        (
            "text-moved-right",
            '↶',
            tr_key(
                "This line is on the left at line {}. Click to go there; drag to move this line.",
            ),
        )
    };
    let label = SharedString::from(format!("{arrow} {}", partner + 1));
    let tip = SharedString::from(trf(tip, &[&(partner + 1)]));
    div()
        .id((id, row))
        .test_support()
        .aria_label(label.clone())
        .flex_none()
        .ml(px(8.))
        .text_size(px(NUMBER_SIZE))
        .text_color(p.moved)
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, down)
        .tooltip(move |window, cx| ld_tip(tip.clone(), window, cx))
        .child(label)
        .into_any_element()
}

/// One half of an aligned row: the line-number cell, then the displayed text on its tint or the
/// hatch, with inline, find and selection highlights. A line wider than the half ends in a "›"
/// marker; every drawn line widens `widest` to its full width, which `widths` keeps for lines
/// too long to show whole. With wrap on, the text is a column of segments instead and never
/// wider than the half (R3). Also returns what the text drew, for clicks.
pub(super) fn half(
    h: Half<'_>,
    display: DisplayOptions,
    p: &Palette,
    hatch: Background,
    widest: &Cell<usize>,
    widths: &RefCell<LineWidths>,
    (number_down, tag_down): (
        impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
        impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ),
) -> (Div, Option<HalfText>) {
    let left = h.pane == Pane::Left;
    let look = h.look(p);
    let row_height = h.metrics.row_height;
    let wrap = h.metrics.wrap;
    let number = div()
        .relative()
        .flex()
        .flex_none()
        .w(px(NUMBER_CELL))
        .h_full()
        // A wrapped row's number stays on its first line.
        .map(|d| {
            if wrap {
                d.items_start()
            } else {
                d.items_center()
            }
        })
        .justify_end()
        .pl(px(4.))
        .pr(px(8.))
        .text_size(px(NUMBER_SIZE))
        .text_color(look.number)
        .on_mouse_down(MouseButton::Left, number_down)
        .when_some(look.number_bg, |d, c| d.bg(c))
        .map(|d| match look.bar {
            Some(bar) => d.border_r_2().border_color(bar),
            None => d.border_r_1().border_color(p.line2),
        })
        // gpui borders share one colour, so the ≈ edge is a child; only ≈ rows have it.
        .when_some(look.edge, |d, edge| {
            d.child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .h_full()
                    .w(px(3.))
                    .bg(edge),
            )
        })
        .when(h.anchored, |d| {
            d.child(
                div()
                    .id((
                        if left {
                            "text-anchor-left"
                        } else {
                            "text-anchor-right"
                        },
                        h.row,
                    ))
                    .test_support()
                    .mr(px(3.))
                    .text_size(px(10.))
                    .text_color(p.accent)
                    .child("⚓"),
            )
        })
        .child(h.line.map(|l| (l + 1).to_string()).unwrap_or_default());
    let tag = h
        .moved
        .filter(|_| h.text.is_some())
        .map(|partner| moved_tag(left, h.row, partner, tag_down, p));
    // A row, so the "moved" tag follows the text; wrapped, a column of segment lines.
    let area = if wrap { v_flex() } else { h_flex() }
        .relative()
        .flex_1()
        .min_w_0()
        .h_full()
        .pl(px(TEXT_INSET))
        .overflow_hidden()
        .whitespace_nowrap()
        .map(|d| match look.fill {
            HalfFill::Tint(c) => d.bg(c),
            HalfFill::Hatch => d.bg(hatch),
            HalfFill::None => d,
        });
    let (area, row_text) = if wrap {
        wrapped(area, &h, display, &look, p, tag)
    } else {
        let line = h.line;
        let text = h.text.as_deref().map(|text| {
            let shown = display_line(text, display);
            // A row stops at its cut; the scrollbar maps the whole line.
            let full = match line {
                Some(line) if shown.cut => {
                    widths
                        .borrow_mut()
                        .columns((left, line), display.tab_width, shown.end_column)
                }
                _ => shown.end_column,
            };
            widest.set(widest.get().max(full));
            let overflow = display.view_columns > 0
                && (shown.cut || shown.end_column > display.skip_columns + display.view_columns);
            (draw_line(&h, shown, &look, p, true), overflow)
        });
        let overflow = text.as_ref().is_some_and(|t| t.1);
        let (styled, caret_at, outline, row_text) = match text {
            Some((drawn, _)) => (
                Some(drawn.styled),
                drawn.caret_at,
                drawn.outline,
                drawn.text.map(HalfText::One),
            ),
            None => (None, None, None, None),
        };
        let area = area.children(styled).children(tag);
        let area = marks(area, caret_at, outline, p, row_height).when(overflow, |d| {
            let id = if left {
                "text-cut-left"
            } else {
                "text-cut-right"
            };
            // Opaque, so the text it covers does not show through: the number cell's status
            // colour, or the panel under a translucent selection.
            let under = look.number_bg.filter(|c| c.a >= 1.).unwrap_or(p.panel);
            d.child(
                div()
                    .id((id, h.row))
                    .test_support()
                    .absolute()
                    .right_0()
                    .top_0()
                    .h_full()
                    .px_1()
                    .bg(under)
                    .text_color(p.accent)
                    .child("›"),
            )
        });
        (area, row_text)
    };
    let cell = h_flex().flex_1().min_w_0().h_full().child(number);
    let cell = if look.fill == HalfFill::Hatch {
        // The filler's id sits on the hatched cell itself; no element is added for it.
        let id = if left {
            "text-filler-left"
        } else {
            "text-filler-right"
        };
        cell.child(area.id((id, h.row)).test_support())
    } else {
        cell.child(area)
    };
    (cell, row_text)
}

/// A wrapped half's text (R3): one line `row_height` tall a segment, each with its own
/// highlights, find hits, selection, caret and current match; the "moved" tag follows the
/// last. Segment `k` of display row `d` is `("text-seg-left", d * MAX_WRAP_SEGMENTS + k)`.
fn wrapped(
    area: Div,
    h: &Half<'_>,
    display: DisplayOptions,
    look: &HalfLook,
    p: &Palette,
    mut tag: Option<AnyElement>,
) -> (Div, Option<HalfText>) {
    let Some(text) = h.text.as_deref() else {
        return (area, None);
    };
    let left = h.pane == Pane::Left;
    let id = if left {
        "text-seg-left"
    } else {
        "text-seg-right"
    };
    let row_height = h.metrics.row_height;
    let segments = wrap_line(text, display, display.view_columns);
    let count = segments.len();
    let mut texts = Vec::with_capacity(count);
    let mut area = area;
    for (k, shown) in segments.into_iter().enumerate() {
        let last = k + 1 == count;
        let drawn = draw_line(h, shown, look, p, last);
        let line = h_flex()
            .relative()
            .flex_none()
            .h(px(row_height))
            .child(drawn.styled)
            .children(if last { tag.take() } else { None });
        area = area.child(
            marks(line, drawn.caret_at, drawn.outline, p, row_height)
                .id((id, h.row * MAX_WRAP_SEGMENTS + k))
                .test_support(),
        );
        texts.extend(drawn.text);
    }
    (area, (!texts.is_empty()).then(|| HalfText::Many(texts)))
}

/// What each line of half `h` layers on its text, as `half` draws it: one list for an
/// unwrapped half, one a segment for a wrapped one, none for a side with no line here.
pub(super) fn half_highlights(
    h: &Half<'_>,
    display: DisplayOptions,
    p: &Palette,
) -> Vec<Vec<(Range<usize>, HighlightStyle)>> {
    let Some(text) = h.text.as_deref() else {
        return Vec::new();
    };
    let look = h.look(p);
    let lines = if h.metrics.wrap {
        wrap_line(text, display, display.view_columns)
    } else {
        vec![display_line(text, display)]
    };
    lines
        .iter()
        .map(|shown| highlights(h, shown, &look, p))
        .collect()
}

/// A run of hidden lines (filtered views): centred dim text on panel2, accent on hover, with
/// the show-more and show-all actions as small ghost buttons.
pub fn fold_row(
    display: usize,
    hidden: u32,
    metrics: RowMetrics,
    p: &Palette,
    cx: &mut Context<TextView>,
) -> AnyElement {
    let accent = p.accent;
    h_flex()
        .id(("text-fold", display))
        .test_support()
        .h(px(metrics.row_height))
        .w_full()
        .pr(px(SCROLLBAR_WIDTH))
        .gap(px(8.))
        .items_center()
        .justify_center()
        .bg(p.panel2)
        .text_size(px(NUMBER_SIZE))
        .text_color(p.dim)
        .hover(move |s| s.text_color(accent))
        .child(format!(
            "⋯ {}",
            count_of(u64::from(hidden), "unchanged line", "unchanged lines")
        ))
        .child(
            ld_ghost(
                ("text-fold-more", display),
                trf("Show {} more", &[&FOLD_STEP]),
                p,
            )
            .h(px(16.))
            .px(px(6.))
            .text_size(px(NUMBER_SIZE))
            .ld_click(
                true,
                cx.listener(move |this, _, _, cx| this.expand_fold(display, Some(FOLD_STEP), cx)),
            ),
        )
        .child(
            ld_ghost(("text-fold-all", display), tr("Show all"), p)
                .h(px(16.))
                .px(px(6.))
                .text_size(px(NUMBER_SIZE))
                .ld_click(
                    true,
                    cx.listener(move |this, _, _, cx| this.expand_fold(display, None, cx)),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn differences_tint_the_text_and_colour_the_number_cell() {
        let p = Palette::dark();
        let changed = half_look(RowKind::Changed, true, false, false, false, false, &p);
        assert_eq!(changed.fill, HalfFill::Tint(p.bg_mod));
        assert_eq!(
            (changed.number_bg, changed.number),
            (Some(p.ln_mod), p.modified)
        );
        assert_eq!(
            changed.highlight,
            Some(p.hl_del),
            "the left side's inline highlight"
        );
        assert_eq!(
            half_look(RowKind::Changed, false, false, false, false, false, &p).highlight,
            Some(p.hl_ins)
        );
        let gone = half_look(RowKind::LeftOnly, true, false, false, false, false, &p);
        assert_eq!(
            (gone.fill, gone.number_bg, gone.number),
            (HalfFill::Tint(p.bg_del), Some(p.ln_del), p.del)
        );
        let added = half_look(RowKind::RightOnly, false, false, false, false, false, &p);
        assert_eq!(
            (added.fill, added.number_bg, added.number),
            (HalfFill::Tint(p.bg_ins), Some(p.ln_ins), p.ins)
        );
        let same = half_look(RowKind::Same, true, false, false, false, false, &p);
        assert_eq!(
            (same.fill, same.number_bg, same.number),
            (HalfFill::None, None, p.dim)
        );
    }

    #[test]
    fn the_empty_side_of_an_insert_or_delete_is_hatched() {
        let p = Palette::light();
        assert_eq!(
            half_look(RowKind::LeftOnly, false, false, false, false, false, &p).fill,
            HalfFill::Hatch
        );
        assert_eq!(
            half_look(RowKind::RightOnly, true, false, false, false, false, &p).fill,
            HalfFill::Hatch
        );
    }

    #[test]
    fn moved_current_unimportant_and_selected_rows() {
        let p = Palette::dark();
        let moved = half_look(RowKind::LeftOnly, true, true, false, false, false, &p);
        assert_eq!(
            (moved.fill, moved.number_bg, moved.number),
            (HalfFill::Tint(p.bg_moved), Some(p.ln_mov), p.moved)
        );
        let current = half_look(RowKind::Changed, true, false, false, true, false, &p);
        assert_eq!((current.number, current.bar), (p.accent, Some(p.accent)));
        let current_filler = half_look(RowKind::LeftOnly, false, false, false, true, false, &p);
        assert_eq!(
            current_filler.bar,
            Some(p.accent),
            "the bar runs down the whole block"
        );
        // Ruling (Task 8): ≈ rows take bg_unimp, and hl_unimp for what the option hid.
        let soft = half_look(RowKind::Same, true, false, true, false, false, &p);
        assert_eq!(
            (soft.edge, soft.number, soft.fill),
            (Some(p.unimp), p.unimp, HalfFill::Tint(p.bg_unimp))
        );
        assert_eq!(soft.highlight, Some(p.hl_unimp));
        let selected = half_look(RowKind::Changed, true, false, false, false, true, &p);
        assert_eq!(selected.number_bg, Some(p.sel));
    }

    #[test]
    fn soft_rows_look_unimportant() {
        for p in [Palette::dark(), Palette::light()] {
            for (kind, left) in [(RowKind::SoftLeft, true), (RowKind::SoftRight, false)] {
                let look = half_look(kind, left, false, false, false, false, &p);
                assert_eq!(look.fill, HalfFill::Tint(p.bg_unimp), "{kind:?}");
                assert_eq!((look.number, look.edge), (p.unimp, Some(p.unimp)));
                assert_eq!(look.highlight, None, "nothing to refine on one side");
            }
            for (kind, left) in [(RowKind::SoftLeft, false), (RowKind::SoftRight, true)] {
                let look = half_look(kind, left, false, false, false, false, &p);
                assert_eq!(look.fill, HalfFill::Hatch, "{kind:?} on the other half");
                assert_eq!(look.edge, None);
            }
        }
    }

    /// Lines `line..line + old` became `line..line + new`.
    fn change(line: u32, old: u32, new: u32) -> Change {
        Change {
            line,
            old_lines: old,
            new_lines: new,
            start_byte: 0,
            old_end_byte: 0,
            new_end_byte: 0,
            start_point: (0, 0),
            old_end_point: (0, 0),
            new_end_point: (0, 0),
        }
    }

    #[test]
    fn a_frame_never_walks_a_line_it_has_no_count_for() {
        let mut widths = LineWidths::default();
        assert_eq!(
            widths.columns((true, 7), 4, 4_100),
            4_100,
            "unknown: what the row shows, until the count lands"
        );
        assert_eq!(widths.take_wanted(), [(true, 7)]);
        widths.counted((true, 7), 4, 20_000);
        assert_eq!(widths.columns((true, 7), 4, 4_100), 20_000);
        assert!(widths.take_wanted().is_empty(), "a current count is enough");
        assert_eq!(
            widths.columns((true, 7), 8, 4_100),
            20_000,
            "a new tab width keeps the old count as a guess"
        );
        assert_eq!(widths.take_wanted(), [(true, 7)]);
    }

    #[test]
    fn an_edit_recounts_only_the_lines_it_changed() {
        let mut widths = LineWidths::default();
        widths.counted((true, 3), 4, 9_000);
        widths.counted((true, 7), 4, 20_000);
        widths.counted((false, 3), 4, 5_000);
        widths.edited(true, &change(3, 1, 1));
        assert_eq!(widths.columns((true, 7), 4, 4_100), 20_000);
        assert_eq!(widths.columns((false, 3), 4, 4_100), 5_000);
        assert!(
            widths.take_wanted().is_empty(),
            "the untouched long line and the other side keep their counts"
        );
        assert_eq!(
            widths.columns((true, 3), 4, 4_100),
            9_000,
            "the edited line keeps its stale count while typing"
        );
        assert_eq!(widths.take_wanted(), [(true, 3)], "and is counted again");
    }

    #[test]
    fn counts_after_an_inserted_or_removed_line_move_with_it() {
        let mut widths = LineWidths::default();
        widths.counted((true, 10), 4, 20_000);
        widths.counted((true, 1), 4, 7_000);
        // Two lines inserted into line 2.
        widths.edited(true, &change(2, 1, 3));
        assert_eq!(widths.columns((true, 12), 4, 4_100), 20_000);
        assert_eq!(widths.columns((true, 1), 4, 4_100), 7_000);
        assert!(widths.take_wanted().is_empty());
        // Lines 0 to 2 joined into one.
        widths.edited(true, &change(0, 3, 1));
        assert_eq!(widths.columns((true, 10), 4, 4_100), 20_000);
        assert!(widths.take_wanted().is_empty());
        assert_eq!(
            widths.columns((true, 0), 4, 4_100),
            7_000,
            "the joined line starts from the widest count it absorbed"
        );
        assert_eq!(widths.take_wanted(), [(true, 0)]);
        widths.clear();
        assert_eq!(widths.columns((true, 10), 4, 4_100), 4_100);
    }

    #[test]
    fn find_hits_draw_their_text_in_the_text_colour() {
        for p in [Palette::dark(), Palette::light()] {
            let hit = find_hit_style(&p);
            assert_eq!(
                (hit.background_color, hit.color),
                (Some(p.find), Some(p.text))
            );
            assert_eq!(
                hit.underline.and_then(|u| u.color),
                Some(p.accent),
                "an underline in the current match's colour marks every hit"
            );
            let syntax = HighlightStyle {
                color: Some(p.sx.type_),
                ..Default::default()
            };
            let merged = layer(&[(0..4, syntax)], &[(1..3, hit)]);
            assert_eq!(
                merged[1],
                (1..3, hit),
                "the hit's colour replaces the syntax colour under it"
            );
        }
    }

    #[test]
    fn only_hidden_changes_are_unimportant() {
        assert!(unimportant(RowKind::Same, true, Some("A b"), Some("a  b")));
        assert!(!unimportant(
            RowKind::Same,
            true,
            Some("same"),
            Some("same")
        ));
        assert!(
            !unimportant(RowKind::Same, false, Some("A"), Some("a")),
            "no ignore option, nothing hidden"
        );
        assert!(!unimportant(RowKind::Changed, true, Some("x"), Some("y")));
    }
}
