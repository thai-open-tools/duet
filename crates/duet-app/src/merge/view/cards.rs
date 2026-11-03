//! The merge stage's card list (R6, R14, R16): a gpui `list` over the items of
//! `merge::items`, what each change card draws, folding, "show all", the current card and the
//! jump to it, each card's result field (R5) and the card menu's target.

// Marks are lists of byte ranges, so a one-element `vec![a..b]` is intended.
#![allow(clippy::single_range_in_vec_init)]

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::time::Instant;

use duet_core::text::merge::diff3::RegionKind;
use duet_core::text::merge::marks::{CardMarks, ColumnMarks, card_marks};
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    AnyElement, Div, FontWeight, HighlightStyle, Hsla, ListAlignment, ListHorizontalSizingBehavior,
    ListOffset, ListState, MouseButton, MouseDownEvent, Stateful, StyledText, list,
    transparent_black, uniform_list,
};

use super::*;
use crate::merge::items::{
    Banner, COLUMN_MAX, FOLD_MIN, Item, chunk_items, field_placeholder, field_rows, field_value,
    kind_badge, layout_items, range_text, state_text, where_text,
};
use crate::merge::menu::card_menu;
use crate::text::display::{DisplayOptions, display_line};
use crate::ui::fonts::{CODE_SIZE, Fonts, SMALL_SIZE};
use crate::ui::menu::fill;

/// A code row in a stable box or a card column.
pub(super) const ROW_HEIGHT: f32 = 19.;
/// A line number's cell: 6 code characters and its 8 px padding (LocalDiff `.mg-ln > i`).
const NUMBER_WIDTH: f32 = 53.;
/// The tallest a card column grows before it scrolls (`css:105`).
const COLUMN_HEIGHT_MAX: f32 = 360.;
/// Every item's side gutter, and the gap above a banner or a stable box.
const GUTTER: f32 = 14.;
const BANNER_GAP: f32 = 10.;
const BOX_GAP: f32 = 8.;
const BOX_RADIUS: f32 = 8.;
const CARD_RADIUS: f32 = 10.;
/// From a card item's top to its card: a jump scrolls the item to the list's top, so the
/// card sits this far below it (R16, PF1). The current card's outline (2 px, 2 px off the
/// card, `css:93`) takes the last `OUTLINE` of it.
const CARD_TOP: f32 = 16.;
const OUTLINE: f32 = 4.;
/// The list's bottom padding, so the last card can scroll up off the edge (`css:49`).
const LIST_BOTTOM: f32 = 48.;

const SAME: &str = "✓ The three files are the same: nothing to merge.";
const TRAIL_NOTE: &str = "Ignoring trailing whitespace: the parts that match use the left text.";
const EMPTY: &str = "The three files are empty.";

/// One row of a card column, as drawn: the number, the text with its tabs expanded, whether
/// it differs from the base, and the characters that do (byte ranges in `text`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowText {
    pub number: u32,
    pub text: String,
    pub changed: bool,
    pub marks: Vec<Range<usize>>,
}

/// A card column: its head ("Left line 2"), its rows, what it says when it has none, and
/// the link to the lines past `COLUMN_MAX`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnText {
    pub head: String,
    pub rows: Vec<RowText>,
    pub empty: Option<String>,
    pub more: Option<String>,
}

/// What a change card draws: badge, state, place, and the left, base and right columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardText {
    pub badge: &'static str,
    pub state: String,
    pub place: String,
    pub columns: [ColumnText; 3],
}

/// Row `k` of a card column: line `line` of `text` with its tabs expanded. Its marks are
/// mapped onto the shown text, so they follow its tabs; a mark lost to a cut line is dropped
/// (PF11).
fn row_text(text: &MergeText, line: u32, k: usize, marks: &ColumnMarks) -> RowText {
    let display = display_line(text.line(line), DisplayOptions::default());
    let row_marks = marks
        .highlights
        .get(k)
        .into_iter()
        .flatten()
        .filter_map(|mark| display.map_range(mark.clone()))
        .collect();
    RowText {
        number: line + 1,
        text: display.text,
        changed: marks.changed.get(k).copied().unwrap_or(false),
        marks: row_marks,
    }
}

/// One column of a card (`colHTML`). `full` lifts the `COLUMN_MAX` cap.
fn column_text(
    title: &str,
    text: &MergeText,
    range: Range<u32>,
    marks: &ColumnMarks,
    empty: String,
    full: bool,
) -> ColumnText {
    let shown = if full {
        range.end
    } else {
        range.end.min(range.start + COLUMN_MAX)
    };
    let rows = (range.start..shown)
        .map(|line| row_text(text, line, (line - range.start) as usize, marks))
        .collect();
    let more = (shown < range.end).then(|| {
        let left = u64::from(range.end - shown);
        format!(
            "… {}: click to show all",
            count_of(left, "more line", "more lines")
        )
    });
    ColumnText {
        head: format!("{title} {}", range_text(range.clone())),
        rows,
        empty: range.is_empty().then_some(empty),
        more,
    }
}

/// The card of a change chunk (`cardHTML`); None for a stable chunk.
pub(crate) fn card_text_of(session: &MergeSession, chunk: usize, full: bool) -> Option<CardText> {
    let region = session.regions().get(chunk)?;
    if region.kind == RegionKind::Stable {
        return None;
    }
    let (left, base, right) = (session.left(), session.base(), session.right());
    let marks = card_marks(left, base, right, region, session.trail());
    card_text_with(session, chunk, full, &marks)
}

/// `card_text_of` with the card's marks made already.
fn card_text_with(
    session: &MergeSession,
    chunk: usize,
    full: bool,
    marks: &CardMarks,
) -> Option<CardText> {
    let region = session.regions().get(chunk)?;
    if region.kind == RegionKind::Stable {
        return None;
    }
    let (left, base, right) = (session.left(), session.base(), session.right());
    let removed = |side: &str| {
        if region.base.is_empty() {
            "(no lines)".to_owned()
        } else {
            format!("({side} removed this part)")
        }
    };
    Some(CardText {
        badge: kind_badge(region.kind),
        state: state_text(session.resolution(chunk)),
        place: where_text(region.base.clone()),
        columns: [
            column_text(
                "Left",
                left,
                region.left.clone(),
                &marks.left,
                removed("left"),
                full,
            ),
            column_text(
                "Base",
                base,
                region.base.clone(),
                &marks.base,
                "(not in the base: new lines)".to_owned(),
                full,
            ),
            column_text(
                "Right",
                right,
                region.right.clone(),
                &marks.right,
                removed("right"),
                full,
            ),
        ],
    })
}

/// The list and what it shows, beside the session.
pub(super) struct Cards {
    list: ListState,
    items: Vec<Item>,
    /// The first item of each chunk: its card, or the top of its stable box.
    starts: Vec<usize>,
    /// Stable chunks shown whole, and change chunks whose columns show every line, with each
    /// column's widest line (the width its rows scroll over).
    expanded: HashSet<usize>,
    full: HashMap<usize, [usize; 3]>,
    /// Each card's marks, made once per session: a card's text is built again from them.
    marks: Vec<Option<Rc<CardMarks>>>,
    /// What each card draws, built the first time it is drawn (PF9). Whatever changes a card
    /// clears its entry.
    texts: Vec<Option<Rc<CardText>>>,
    /// Each change card's result field, made the first time the card is drawn and dropped
    /// with its session (R5).
    fields: Vec<Option<ResultField>>,
    /// The card last right-pressed, which the card menu acts on.
    menu_target: Option<usize>,
    /// How many code rows the cards have built, for the tests.
    rows_built: usize,
    /// The bench hook: when the tab's first merge started, until its list is drawn.
    first_merge: Option<Instant>,
    ready_logged: bool,
}

/// A result field and what its events do.
struct ResultField {
    state: Entity<TextareaState>,
    _subscription: Subscription,
}

impl Cards {
    pub(super) fn new() -> Cards {
        Cards {
            list: ListState::new(0, ListAlignment::Top, px(200.)),
            items: Vec::new(),
            starts: Vec::new(),
            expanded: HashSet::new(),
            full: HashMap::new(),
            marks: Vec::new(),
            texts: Vec::new(),
            fields: Vec::new(),
            menu_target: None,
            rows_built: 0,
            first_merge: None,
            ready_logged: false,
        }
    }

    /// For a new session (or none): every fold closed, every column capped, no field yet, and
    /// the list back at its top.
    fn rebuild(&mut self, session: Option<&MergeSession>) {
        self.expanded.clear();
        self.full.clear();
        self.items = session.map_or_else(Vec::new, |s| layout_items(s, &self.expanded));
        let regions = session.map_or(0, |s| s.regions().len());
        self.marks = vec![None; regions];
        self.texts = vec![None; regions];
        self.fields = (0..regions).map(|_| None).collect();
        self.menu_target = None;
        self.index();
        self.list.reset(self.items.len());
    }

    /// Chunks come in order and each has at least one item.
    fn index(&mut self) {
        self.starts.clear();
        for (ix, item) in self.items.iter().enumerate() {
            if item.chunk() == Some(self.starts.len()) {
                self.starts.push(ix);
            }
        }
    }

    /// The items of `chunk`: they follow each other.
    fn span(&self, chunk: usize) -> Option<Range<usize>> {
        let start = *self.starts.get(chunk)?;
        let len = self.items[start..]
            .iter()
            .take_while(|item| item.chunk() == Some(chunk))
            .count();
        Some(start..start + len)
    }
}

/// How one card column looks: its id, the colour of its title, and its changed lines.
struct ColumnLook {
    id: &'static str,
    title: Hsla,
    changed: Hsla,
    number: Hsla,
    highlight: Option<Hsla>,
}

fn column_looks(p: &Palette) -> [ColumnLook; 3] {
    [
        ColumnLook {
            id: "merge-col-left",
            title: p.moved,
            changed: p.bg_moved,
            number: p.ln_mov,
            highlight: Some(p.sel),
        },
        ColumnLook {
            id: "merge-col-base",
            title: p.muted,
            changed: p.bg_del,
            number: p.ln_del,
            highlight: None,
        },
        ColumnLook {
            id: "merge-col-right",
            title: p.ins,
            changed: p.bg_ins,
            number: p.ln_ins,
            highlight: Some(p.hl_ins),
        },
    ]
}

/// The kind's colour and its badge background.
fn kind_colors(kind: RegionKind, p: &Palette) -> (Hsla, Hsla) {
    match kind {
        RegionKind::Left => (p.moved, p.bg_moved),
        RegionKind::Right => (p.ins, p.bg_ins),
        RegionKind::Both => (p.unimp, p.bg_unimp),
        RegionKind::Conflict | RegionKind::Stable => (p.del, p.bg_del),
    }
}

/// An item's frame: the side gutter and the space above it.
fn item_frame(top: f32) -> Div {
    div().w_full().px(px(GUTTER)).pt(px(top))
}

/// A line number's cell (`.mg-ln > i`).
pub(super) fn number_cell(number: u32, bg: Hsla, p: &Palette) -> Div {
    h_flex()
        .flex_none()
        .w(px(NUMBER_WIDTH))
        .h_full()
        .justify_end()
        .pr(px(8.))
        .mr(px(8.))
        .bg(bg)
        .border_r_1()
        .border_color(p.line2)
        .text_color(p.dim)
        .child(number.to_string())
}

impl MergeView {
    pub fn items(&self) -> &[Item] {
        &self.cards.items
    }

    /// What the card of `chunk` shows (the list draws a column shown in full as it scrolls);
    /// None for a stable chunk.
    pub fn card_text(&self, chunk: usize) -> Option<CardText> {
        let full = self.cards.full.contains_key(&chunk);
        if !full && let Some(Some(text)) = self.cards.texts.get(chunk) {
            return Some(CardText::clone(text));
        }
        card_text_of(self.session.as_ref()?, chunk, full)
    }

    /// The marks of the card of `chunk`, made once per session.
    fn marks_of(&mut self, chunk: usize) -> Option<Rc<CardMarks>> {
        if let Some(Some(marks)) = self.cards.marks.get(chunk) {
            return Some(marks.clone());
        }
        let session = self.session.as_ref()?;
        let region = session.regions().get(chunk)?;
        if region.kind == RegionKind::Stable {
            return None;
        }
        let (left, base, right) = (session.left(), session.base(), session.right());
        let marks = Rc::new(card_marks(left, base, right, region, session.trail()));
        *self.cards.marks.get_mut(chunk)? = Some(marks.clone());
        Some(marks)
    }

    /// The card's text for drawing: built once, then kept until the card changes (PF9). Its
    /// columns hold their first `COLUMN_MAX` rows; a column shown in full draws its rows as
    /// they scroll into view (`full_rows`, M-82).
    fn drawn_card(&mut self, chunk: usize) -> Option<Rc<CardText>> {
        if let Some(Some(text)) = self.cards.texts.get(chunk) {
            return Some(text.clone());
        }
        let marks = self.marks_of(chunk)?;
        let text = Rc::new(card_text_with(
            self.session.as_ref()?,
            chunk,
            false,
            &marks,
        )?);
        if let Some(slot) = self.cards.texts.get_mut(chunk) {
            *slot = Some(text.clone());
        }
        Some(text)
    }

    /// Rows `range` of column `k` of a card shown in full, built as they come into view: a
    /// column of 50,000 lines draws the twenty it shows (M-82).
    fn full_rows(
        &mut self,
        chunk: usize,
        k: usize,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let p = Palette::current(cx);
        let looks = column_looks(&p);
        let (Some(session), Some(Some(marks))) =
            (self.session.as_ref(), self.cards.marks.get(chunk))
        else {
            return Vec::new();
        };
        let Some(region) = session.regions().get(chunk) else {
            return Vec::new();
        };
        let (text, lines, column) = match k {
            0 => (session.left(), &region.left, &marks.left),
            1 => (session.base(), &region.base, &marks.base),
            _ => (session.right(), &region.right, &marks.right),
        };
        let rows: Vec<AnyElement> = range
            .filter(|&i| i < lines.len())
            .map(|i| {
                let row = row_text(text, lines.start + i as u32, i, column);
                code_row(&row, &looks[k], &p).into_any_element()
            })
            .collect();
        self.cards.rows_built += rows.len();
        rows
    }

    /// The card of `chunk` changed: its text is built again and its height measured again,
    /// even while it is off screen (R6).
    pub(super) fn refresh_card(&mut self, chunk: usize) {
        if let Some(text) = self.cards.texts.get_mut(chunk) {
            *text = None;
        }
        if let Some(&ix) = self.cards.starts.get(chunk) {
            self.cards.list.remeasure_items(ix..ix + 1);
        }
    }

    /// The list for the session now held: folds closed, at the top.
    pub(super) fn rebuild_cards(&mut self) {
        self.cards.rebuild(self.session.as_ref());
    }

    /// The result field of `chunk`, once its card was drawn.
    pub(super) fn field(&self, chunk: usize) -> Option<&Entity<TextareaState>> {
        self.cards
            .fields
            .get(chunk)?
            .as_ref()
            .map(|field| &field.state)
    }

    /// Every result field made so far.
    pub(super) fn result_fields(&self) -> impl Iterator<Item = &Entity<TextareaState>> {
        self.cards.fields.iter().flatten().map(|field| &field.state)
    }

    /// The field of `chunk`, made on the card's first draw with the part's result (R5). The
    /// list draws its items while holding its state, which making an entity does not touch.
    fn result_field(
        &mut self,
        chunk: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TextareaState>> {
        if let Some(field) = self.field(chunk) {
            return Some(field.clone());
        }
        if chunk >= self.cards.fields.len() {
            return None;
        }
        let resolution = self.session.as_ref()?.resolution(chunk);
        let (text, resolved) = (field_value(resolution), resolution.is_some());
        let (rows, rows_max) = field_rows(resolved);
        let state = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(rows, rows_max)
                .soft_wrap(false)
                .placeholder(field_placeholder(resolved))
                .default_value(text)
        });
        let subscription = cx.subscribe_in(
            &state,
            window,
            move |this: &mut Self, field, event: &InputEvent, window, cx| {
                this.on_field_event(chunk, field, event, window, cx)
            },
        );
        self.cards.fields[chunk] = Some(ResultField {
            state: state.clone(),
            _subscription: subscription,
        });
        Some(state)
    }

    /// Keeps the card of `chunk` drawn while its field has the keys, even scrolled out of view:
    /// the list draws an item whose focus handle holds the focus (PF2). Called from the
    /// field's focus and blur events, never while the list draws.
    pub(super) fn keep_field_drawn(&mut self, chunk: usize, focus: Option<FocusHandle>) {
        let Some(&ix) = self.cards.starts.get(chunk) else {
            return;
        };
        if self.cards.items.get(ix) != Some(&Item::Card(chunk)) {
            return;
        }
        let list = &self.cards.list;
        let top = list.logical_scroll_top();
        list.splice_focusable(ix..ix + 1, [focus]);
        // A splice of the item at the top of the view would move the view to that item's top.
        if top.item_ix == ix {
            list.scroll_to(top);
        }
    }

    /// The card the menu acts on: the one last right-pressed.
    pub fn menu_target(&self) -> Option<usize> {
        self.cards.menu_target
    }

    /// How many code rows of card columns were built for drawing so far.
    #[doc(hidden)]
    pub fn code_rows_built_for_test(&self) -> usize {
        self.cards.rows_built
    }

    /// A merge landed; the first one of the tab is timed until its list is drawn.
    pub(super) fn time_first_merge(&mut self, started: Instant) {
        if !self.cards.ready_logged {
            self.cards.first_merge.get_or_insert(started);
        }
    }

    /// A stable run over `FOLD_MIN` lines opens, or folds again.
    pub fn toggle_fold(&mut self, chunk: usize, cx: &mut Context<Self>) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let foldable = session
            .regions()
            .get(chunk)
            .is_some_and(|r| r.kind == RegionKind::Stable && r.left.end - r.left.start > FOLD_MIN);
        if !foldable {
            return;
        }
        let cards = &mut self.cards;
        let Some(span) = cards.span(chunk) else {
            return;
        };
        let open = cards.expanded.insert(chunk);
        if !open {
            cards.expanded.remove(&chunk);
        }
        let items = chunk_items(session, chunk, open);
        let count = items.len();
        cards.items.splice(span.clone(), items);
        cards.list.splice(span, count);
        cards.index();
        cx.notify();
    }

    /// "Show all": the card's columns show every line. Each column's widest line is found
    /// now, for the width its rows scroll over.
    pub fn show_all(&mut self, chunk: usize, cx: &mut Context<Self>) {
        let is_card = self
            .cards
            .starts
            .get(chunk)
            .is_some_and(|&ix| self.cards.items[ix] == Item::Card(chunk));
        let Some(session) = self.session.as_ref() else {
            return;
        };
        if !is_card || self.cards.full.contains_key(&chunk) {
            return;
        }
        let region = &session.regions()[chunk];
        let widest = [
            (session.left(), &region.left),
            (session.base(), &region.base),
            (session.right(), &region.right),
        ]
        .map(|(text, lines)| {
            lines
                .clone()
                .max_by_key(|&line| text.line(line).len())
                .map_or(0, |line| (line - lines.start) as usize)
        });
        self.cards.full.insert(chunk, widest);
        self.refresh_card(chunk);
        cx.notify();
    }

    /// A click on a card makes it the current change.
    pub fn set_current(&mut self, chunk: usize, cx: &mut Context<Self>) {
        if let Some(session) = self.session.as_mut() {
            session.set_current(chunk);
            cx.notify();
        }
    }

    /// Scrolls the item of `chunk` to the list's top; a card's own top space then puts it
    /// 16 px below (R16, PF1).
    pub(super) fn scroll_to_chunk(&self, chunk: usize) {
        if let Some(&item_ix) = self.cards.starts.get(chunk) {
            self.cards.list.scroll_to(ListOffset {
                item_ix,
                offset_in_item: px(0.),
            });
        }
    }

    /// The card's item lies inside the list's bounds (its top inside, and its bottom too
    /// unless it is taller than the list).
    #[doc(hidden)]
    pub fn card_visible_for_test(&self, chunk: usize) -> bool {
        let Some(&ix) = self.cards.starts.get(chunk) else {
            return false;
        };
        if self.cards.items.get(ix) != Some(&Item::Card(chunk)) {
            return false;
        }
        let Some(card) = self.cards.list.bounds_for_item(ix) else {
            return false;
        };
        let list = self.cards.list.viewport_bounds();
        let top_inside = card.top() >= list.top() && card.top() < list.bottom();
        top_inside && (card.bottom() <= list.bottom() || card.size.height > list.size.height)
    }

    /// The merge stage: the notice, then the list (or why there is none).
    pub(super) fn render_merge_stage(
        &mut self,
        root: Stateful<Div>,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let fonts = Fonts::current(cx);
        let body = if self.session.is_some() {
            self.log_merge_ready(window);
            div()
                .id("merge-list")
                .test_support()
                .relative()
                .flex_1()
                .min_w_0()
                .h_full()
                // A right press anywhere in the list first forgets the menu's card (capture
                // runs before the cards); a card's own right press then sets it, so a press
                // elsewhere, such as in a result field, opens no stale card menu.
                .capture_any_mouse_down(cx.listener(|this, e: &MouseDownEvent, _, _| {
                    if e.button == MouseButton::Right {
                        this.cards.menu_target = None;
                    }
                }))
                .child(
                    list(self.cards.list.clone(), cx.processor(Self::render_item))
                        .size_full()
                        .pb(px(LIST_BOTTOM)),
                )
                .vertical_scrollbar(&self.cards.list)
                .into_any_element()
        } else {
            div()
                .id("merge-placeholder")
                .test_support()
                .p(px(16.))
                .text_color(p.muted)
                .child(self.body_text())
                .into_any_element()
        };
        let info = self
            .session
            .is_some()
            .then(|| self.render_info_bar(p, &fonts, cx).into_any_element());
        let preview = self.render_preview(p, cx);
        root.children(info)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(body)
                    .children(preview),
            )
            .child(self.render_merge_status(p))
    }

    /// The bench hook: the tab's first merge is applied and its list drawn. Logs how long it
    /// took from the merge job's start (all three texts in hand), then with
    /// `DUET_EXIT_AFTER_FIRST_ROWS` quits on the next frame, as the picture view does.
    fn log_merge_ready(&mut self, window: &mut Window) {
        let Some(started) = self.cards.first_merge.take() else {
            return;
        };
        self.cards.ready_logged = true;
        window.on_next_frame(move |_, cx| {
            let ms = started.elapsed().as_millis() as u64;
            tracing::info!(ms, "merge ready");
            if std::env::var_os("DUET_EXIT_AFTER_FIRST_ROWS").is_some() {
                cx.quit();
            }
        });
    }

    /// One item of the list. The list holds its state while it calls this, so nothing here
    /// may touch the `ListState` (PF2).
    fn render_item(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::current(cx);
        let fonts = Fonts::current(cx);
        let Some(item) = self.cards.items.get(ix).copied() else {
            return div().into_any_element();
        };
        match item {
            Item::Banner(banner) => render_banner(banner, &p),
            Item::Row {
                line, first, last, ..
            } => self.render_row(line, first, last, &p, &fonts),
            Item::Fold { chunk, hidden } => {
                let text = format!(
                    "⋯ {}: click to expand",
                    count_of(u64::from(hidden), "same line", "same lines")
                );
                render_bar(chunk, false, text, &p, cx)
            }
            Item::Collapse { chunk, lines } => {
                let text = format!(
                    "▴ Collapse {}",
                    count_of(u64::from(lines), "same line", "same lines")
                );
                render_bar(chunk, true, text, &p, cx)
            }
            Item::Card(chunk) => self.render_card(chunk, &p, &fonts, window, cx),
        }
    }

    /// A line of a stable box, in the left's text (R3), muted.
    fn render_row(
        &self,
        line: u32,
        first: bool,
        last: bool,
        p: &Palette,
        fonts: &Fonts,
    ) -> AnyElement {
        let text = self
            .session
            .as_ref()
            .map(|s| display_line(s.left().line(line), DisplayOptions::default()).text)
            .unwrap_or_default();
        item_frame(if first { BOX_GAP } else { 0. })
            .child(
                h_flex()
                    .h(px(ROW_HEIGHT))
                    .overflow_hidden()
                    .bg(p.panel)
                    .border_x_1()
                    .border_color(p.line2)
                    .when(first, |d| d.border_t_1().rounded_t(px(BOX_RADIUS)))
                    .when(last, |d| d.border_b_1().rounded_b(px(BOX_RADIUS)))
                    .font(fonts.code.clone())
                    .text_size(px(CODE_SIZE))
                    .text_color(p.muted)
                    .whitespace_nowrap()
                    .child(
                        number_cell(line + 1, p.panel2, p)
                            .when(first, |d| d.rounded_tl(px(BOX_RADIUS - 1.)))
                            .when(last, |d| d.rounded_bl(px(BOX_RADIUS - 1.))),
                    )
                    .child(div().flex_none().pr(px(12.)).child(text)),
            )
            .into_any_element()
    }

    /// A change card (`cardHTML`): the head, the three columns, then the result's bar and
    /// field. Its frame never changes size, so being current (the accent outline) or resolved
    /// (the ring) needs no new measure.
    fn render_card(
        &mut self,
        chunk: usize,
        p: &Palette,
        fonts: &Fonts,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (Some(text), Some(field)) =
            (self.drawn_card(chunk), self.result_field(chunk, window, cx))
        else {
            return div().into_any_element();
        };
        let Some(session) = self.session.as_ref() else {
            return div().into_any_element();
        };
        let region = &session.regions()[chunk];
        let kind = region.kind;
        // A column over `COLUMN_MAX` lines shown in full draws its rows as they scroll into
        // view (M-82): its length, and its widest line's index.
        let lens = [&region.left, &region.base, &region.right].map(|lines| lines.len());
        let full = self.cards.full.get(&chunk).copied();
        let scrolled: [Option<FullRows>; 3] = std::array::from_fn(|k| {
            let widest = full?[k];
            text.columns[k].more.is_some().then_some(FullRows {
                len: lens[k],
                widest,
            })
        });
        self.cards.rows_built += text
            .columns
            .iter()
            .zip(&scrolled)
            .filter(|(_, scrolled)| scrolled.is_none())
            .map(|(column, _)| column.rows.len())
            .sum::<usize>();
        let resolved = session.resolution(chunk).is_some();
        let current = session.current() == Some(chunk);
        let open_conflict = kind == RegionKind::Conflict && !resolved;
        let (kind_color, kind_bg) = kind_colors(kind, p);

        let state = div()
            .text_color(if resolved { p.ins } else { p.del })
            .when(!resolved, |d| d.font_weight(FontWeight::SEMIBOLD))
            .child(text.state.clone());
        let head = h_flex()
            .id(("merge-card-head", chunk))
            .test_support()
            .items_center()
            .gap(px(10.))
            .px(px(10.))
            .py(px(5.))
            .bg(p.panel2)
            .border_b_1()
            .border_color(p.line2)
            .rounded_tl(px(5.))
            .rounded_tr(px(CARD_RADIUS - 1.))
            .text_size(px(SMALL_SIZE))
            .child(
                div()
                    .flex_none()
                    .px(px(9.))
                    .rounded_full()
                    .border_1()
                    .border_color(kind_color)
                    .bg(kind_bg)
                    .text_color(kind_color)
                    .text_size(px(11.))
                    .font_weight(FontWeight::BOLD)
                    .child(text.badge),
            )
            .child(state)
            .child(div().flex_1())
            .child(
                div()
                    .text_color(p.dim)
                    .text_size(px(11.5))
                    .child(text.place.clone()),
            );
        let columns = h_flex().items_stretch().children(
            column_looks(p)
                .into_iter()
                .zip(text.columns.iter())
                .zip(scrolled)
                .enumerate()
                .map(|(k, ((look, column), rows))| {
                    render_column((chunk, k), &look, column, rows, p, fonts, cx)
                }),
        );
        // The card menu opens on the head, the columns and the result bar; the field keeps its
        // own text menu (`js:915`). A right press there picks the card the menu acts on.
        let view = cx.weak_entity();
        let menu = move |menu: PopupMenu, _: &mut Window, cx: &mut Context<PopupMenu>| {
            let Some(view) = view.upgrade() else {
                return menu;
            };
            let view = view.read(cx);
            fill(menu, card_menu(view), Some(view.focus.clone()))
        };
        let menu_zone = v_flex()
            .id(("merge-card-menu", chunk))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.cards.menu_target = Some(chunk);
                    this.set_current(chunk, cx);
                }),
            )
            .child(head)
            .child(columns)
            .child(self.render_result_bar(chunk, p, cx))
            .context_menu(menu);
        // The kind's colour shows as the 4 px left edge (`css:80-89`): the body covers the
        // rest of it, and its border is the other three sides.
        let body = v_flex()
            .flex_1()
            .min_w_0()
            .bg(p.panel)
            .rounded_l(px(6.))
            .rounded_r(px(CARD_RADIUS))
            .border_1()
            .border_l_0()
            .border_color(if open_conflict { p.del } else { p.line })
            .child(menu_zone)
            .child(self.render_result_field(chunk, &field, p, fonts, window, cx));
        let card = h_flex()
            .items_stretch()
            .rounded(px(CARD_RADIUS))
            .bg(kind_color)
            .pl(px(4.))
            .child(body);
        // An open conflict has a 1 px ring (`css:91`); the current card a 2 px accent outline
        // 2 px off it (`css:93`). Both always take their room, in a clear colour when off.
        let ring = div()
            .rounded(px(CARD_RADIUS + 1.))
            .border_1()
            .border_color(if open_conflict {
                p.del
            } else {
                transparent_black()
            })
            .child(card);
        item_frame(CARD_TOP - OUTLINE)
            .child(
                div()
                    .id(("merge-card", chunk))
                    .test_support()
                    .p(px(1.))
                    .rounded(px(CARD_RADIUS + OUTLINE))
                    .border_2()
                    .border_color(if current {
                        p.accent
                    } else {
                        transparent_black()
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.set_current(chunk, cx)))
                    .child(ring),
            )
            .into_any_element()
    }
}

/// A banner over the cards (`js:477-482`).
fn render_banner(banner: Banner, p: &Palette) -> AnyElement {
    let (id, text) = match banner {
        Banner::Same => ("merge-banner-same", SAME),
        Banner::TrailNote => ("merge-banner-trail", TRAIL_NOTE),
        Banner::Empty => ("merge-banner-empty", EMPTY),
    };
    let body = div()
        .id(id)
        .test_support()
        .aria_label(text)
        .px(px(12.))
        .py(px(8.))
        .rounded(px(BOX_RADIUS))
        .border_1()
        .text_size(px(CODE_SIZE))
        .child(text);
    let body = match banner {
        Banner::Same => body
            .text_color(p.ins)
            .border_color(p.ins)
            .bg(p.bg_ins)
            .font_weight(FontWeight::SEMIBOLD),
        Banner::TrailNote => body.text_color(p.muted).border_color(p.line).bg(p.panel),
        Banner::Empty => body.text_color(p.dim).border_color(transparent_black()),
    };
    item_frame(BANNER_GAP).child(body).into_any_element()
}

/// The bar of a folded stable run, or of an open one (on top of its box); a click toggles it
/// (`.mg-fold`).
fn render_bar(
    chunk: usize,
    open: bool,
    text: String,
    p: &Palette,
    cx: &mut Context<MergeView>,
) -> AnyElement {
    let accent = p.accent;
    let bar = div()
        .id((if open { "merge-collapse" } else { "merge-fold" }, chunk))
        .test_support()
        .aria_label(text.clone())
        .px(px(12.))
        .py(px(3.))
        .bg(p.panel2)
        .border_b_1()
        .when(!open, |d| d.border_t_1())
        .when(open, |d| d.rounded_t(px(BOX_RADIUS - 1.)))
        .border_dashed()
        .border_color(p.line)
        .text_size(px(SMALL_SIZE))
        .text_color(p.muted)
        .cursor_pointer()
        .hover(move |s| s.text_color(accent))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_fold(chunk, cx)))
        .child(text);
    item_frame(if open { BOX_GAP } else { 0. })
        .child(
            div()
                .bg(p.panel)
                .border_x_1()
                .border_color(p.line2)
                .when(open, |d| d.border_t_1().rounded_t(px(BOX_RADIUS)))
                .child(bar),
        )
        .into_any_element()
}

/// A card column shown in full (M-82): how many lines it has, and the index of its widest.
#[derive(Debug, Clone, Copy)]
struct FullRows {
    len: usize,
    widest: usize,
}

/// One card column (`colHTML`): its head, then its rows in a box at most 360 px tall that
/// scrolls both ways, or what it says when it has none, then the "show all" link. A column
/// shown in full (`full`) is a list that builds the rows in view only. The first argument is
/// the card's chunk and the column's index.
fn render_column(
    (chunk, k): (usize, usize),
    look: &ColumnLook,
    column: &ColumnText,
    full: Option<FullRows>,
    p: &Palette,
    fonts: &Fonts,
    cx: &mut Context<MergeView>,
) -> AnyElement {
    let last = k == 2;
    let (title, range) = column
        .head
        .split_once(' ')
        .unwrap_or((column.head.as_str(), ""));
    let head = h_flex()
        .gap(px(4.))
        .px(px(10.))
        .py(px(2.))
        .bg(p.panel2)
        .border_b_1()
        .border_color(p.line2)
        .text_size(px(11.))
        .text_color(p.dim)
        .child(
            div()
                .font_weight(FontWeight::BOLD)
                .text_color(look.title)
                .child(title.to_owned()),
        )
        .child(range.to_owned());
    if let Some(full) = full {
        let rows = uniform_list(
            ("merge-rows", chunk * 3 + k),
            full.len,
            cx.processor(move |view: &mut MergeView, range: Range<usize>, _, cx| {
                view.full_rows(chunk, k, range, cx)
            }),
        )
        .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
        .with_width_from_item(Some(full.widest))
        .w_full()
        .h(px((full.len as f32 * ROW_HEIGHT).min(COLUMN_HEIGHT_MAX)))
        .font(fonts.code.clone())
        .text_size(px(CODE_SIZE))
        .whitespace_nowrap();
        return v_flex()
            .id((look.id, chunk))
            .test_support()
            .flex_1()
            .min_w_0()
            .when(!last, |d| d.border_r_1().border_color(p.line2))
            .child(head)
            .child(rows)
            .into_any_element();
    }
    let lines: AnyElement = match &column.empty {
        Some(empty) => div()
            .px(px(10.))
            .py(px(3.))
            .text_size(px(11.5))
            .italic()
            .text_color(p.dim)
            .child(empty.clone())
            .into_any_element(),
        None => v_flex()
            .flex_none()
            .min_w_full()
            .font(fonts.code.clone())
            .text_size(px(CODE_SIZE))
            .whitespace_nowrap()
            .children(column.rows.iter().map(|row| code_row(row, look, p)))
            .into_any_element(),
    };
    let more = column.more.clone().map(|more| {
        div()
            .id(("merge-more", chunk))
            .test_support()
            .aria_label(more.clone())
            .px(px(10.))
            .py(px(3.))
            .bg(p.panel2)
            .border_t_1()
            .border_color(p.line2)
            .text_size(px(11.5))
            .text_color(p.accent)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.show_all(chunk, cx)))
            .child(more)
    });
    v_flex()
        .id((look.id, chunk))
        .test_support()
        .flex_1()
        .min_w_0()
        .when(!last, |d| d.border_r_1().border_color(p.line2))
        .child(head)
        .child(
            h_flex()
                .id("code")
                .flex_grow_1()
                .items_start()
                .max_h(px(COLUMN_HEIGHT_MAX))
                .overflow_scroll()
                .child(lines),
        )
        .children(more)
        .into_any_element()
}

/// A code row of a card column: changed lines take the side's colour, and the characters
/// that changed a stronger one (`css:108-119`).
fn code_row(row: &RowText, look: &ColumnLook, p: &Palette) -> Div {
    let highlights = match look.highlight {
        Some(color) => row
            .marks
            .iter()
            .map(|mark| {
                (
                    mark.clone(),
                    HighlightStyle {
                        background_color: Some(color),
                        ..Default::default()
                    },
                )
            })
            .collect(),
        None => Vec::new(),
    };
    h_flex()
        .h(px(ROW_HEIGHT))
        .when(row.changed, |d| d.bg(look.changed))
        .child(number_cell(
            row.number,
            if row.changed { look.number } else { p.panel2 },
            p,
        ))
        .child(
            div()
                .flex_none()
                .pr(px(12.))
                .child(StyledText::new(row.text.clone()).with_highlights(highlights)),
        )
}

#[cfg(test)]
mod tests {
    use duet_core::cancel::CancelToken;
    use duet_core::text::merge::diff3::diff3;

    use super::*;

    fn text(lines: &[&str]) -> Arc<MergeText> {
        Arc::new(MergeText::typed(
            lines.iter().map(|l| format!("{l}\n")).collect(),
        ))
    }

    #[test]
    fn marks_move_with_the_tabs_before_them() {
        let (left, base) = (text(&["\tone 2"]), text(&["\tone two"]));
        let regions = diff3(&left, &base, &base, false, &CancelToken::new()).unwrap();
        let session = MergeSession::new(left, base.clone(), base, regions, false);
        let card = card_text_of(&session, 0, false).expect("one change");
        assert_eq!(
            card.columns[0].rows,
            [RowText {
                number: 1,
                text: "    one 2".to_owned(),
                changed: true,
                marks: vec![8..9],
            }],
            "the source mark 5..6 shows at 8..9 once the tab is four spaces"
        );
        assert_eq!(card.columns[1].rows[0].text, "    one two");
    }
}
