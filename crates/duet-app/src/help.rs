//! The Help tab (H1–H7): the language switch, a search box and the grouped topic list on the
//! left, one topic at a time on the right with Previous and Next under it. Thai and English
//! share one code path. gpui would break a Thai line before any Thai letter
//! (`LineWrapper::is_word_char` knows no Thai), so the page never lets gpui wrap its prose:
//! every text is drawn as no-wrap pieces in a wrapping row (H6), with the controls it names as
//! chips and its keys as key caps (U8).

mod en;
pub mod markup;
mod th;
pub mod topics;

use gpui_kit::TestSupportExt as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, FocusHandle, Focusable, Font, FontWeight, KeyBinding,
    ScrollHandle, Subscription, Window, actions, div, point, prelude::*, px, relative,
    transparent_black,
};

use self::markup::Span;
use self::topics::{Block, GROUPS, HelpLang, PageCopy, TOPICS};
use crate::folder::view::legend::sample;
use crate::palette::Palette;
use crate::ui::fonts::{Fonts, UI_SIZE};
use crate::ui::style::{
    LdExt as _, SMALL_ICON_SIZE, ld_button, ld_card, ld_dot, ld_icon, ld_kbd, ld_segment,
    ld_segmented,
};

actions!(help, [FocusSearch, HelpEscape]);

pub const CONTEXT: &str = "HelpView";
/// The text size of the topic's prose.
pub const BODY_SIZE: f32 = UI_SIZE;
/// The prose's line height: room for Thai marks above and below a line (U9).
pub const LINE_HEIGHT: f32 = 1.55;
/// The topic's widest column (U9).
pub const CONTENT_WIDTH: f32 = 760.;
/// A section heading's text size (U9).
pub const HEADING_SIZE: f32 = 15.;
/// A chip's text size: a control named in the prose (U8).
pub const CHIP_SIZE: f32 = 12.5;
/// The left column: the switch, the search box and the topic list (H2).
pub const SIDE_WIDTH: f32 = 240.;
/// A topic-list entry's icon (U10).
const TOC_ICON_SIZE: f32 = 14.;
/// A topic-list entry's line height.
const TOC_LINE_HEIGHT: f32 = 1.4;

pub fn key_bindings() -> Vec<KeyBinding> {
    let c = Some(CONTEXT);
    vec![
        KeyBinding::new("ctrl-f", FocusSearch, c),
        KeyBinding::new("escape", HelpEscape, c),
    ]
}

pub struct HelpView {
    lang: HelpLang,
    search: Entity<InputState>,
    /// The sheet's keys from the real bindings, read once.
    keys: Vec<String>,
    /// Each topic's search text in `lang`, built when the language changes (H7).
    texts: Vec<String>,
    /// The topics the query matches, as indices into `TOPICS`.
    shown: Vec<usize>,
    /// The topic on the right; kept while nothing matches, so clearing the query brings it
    /// back.
    current: usize,
    focus: FocusHandle,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for HelpView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl HelpView {
    /// Opens in the saved language, else Thai (H3), at the first topic.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let lang = HelpLang::load(crate::settings::settings_file(cx).as_deref());
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(topics::page_copy(lang).search));
        let subscriptions = vec![cx.subscribe_in(
            &search,
            window,
            |this: &mut Self, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.refilter(cx);
                }
            },
        )];
        let keys = topics::sheet_keys();
        let texts = topics::search_texts(lang, &keys);
        Self {
            lang,
            search,
            keys,
            texts,
            shown: (0..TOPICS.len()).collect(),
            current: 0,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    pub fn lang(&self) -> HelpLang {
        self.lang
    }

    /// The other language: every string is drawn again in it, the topic and the query stay,
    /// the search runs again, and the choice is saved (H3).
    pub fn set_lang(&mut self, lang: HelpLang, window: &mut Window, cx: &mut Context<Self>) {
        if lang == self.lang {
            return;
        }
        self.lang = lang;
        self.texts = topics::search_texts(lang, &self.keys);
        let placeholder = topics::page_copy(lang).search;
        self.search.update(cx, |state, cx| {
            state.set_placeholder(placeholder, window, cx)
        });
        self.refilter(cx);
        if let Some(path) = crate::settings::settings_file(cx)
            && let Err(err) = lang.save(&path)
        {
            tracing::warn!(%err, "could not save the help language");
        }
        cx.notify();
    }

    pub fn search_input(&self) -> &Entity<InputState> {
        &self.search
    }

    pub fn query(&self, cx: &App) -> String {
        self.search.read(cx).value().to_string()
    }

    pub fn set_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |state, cx| state.set_value(query, window, cx));
        // `set_value` sends no change event.
        self.refilter(cx);
    }

    /// The ids of the topics the list shows.
    pub fn shown(&self) -> Vec<&'static str> {
        self.shown.iter().map(|&i| TOPICS[i].id).collect()
    }

    /// The topic on the right; None while no topic matches.
    pub fn current(&self) -> Option<&'static str> {
        self.shown
            .contains(&self.current)
            .then(|| TOPICS[self.current].id)
    }

    /// Shows topic `id` when the list shows it.
    pub fn go_to(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        match TOPICS.iter().position(|t| t.id == id) {
            Some(index) if self.shown.contains(&index) => {
                self.show(index, cx);
                true
            }
            _ => false,
        }
    }

    /// The topic before or after the current one in the list as shown.
    pub fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(at) = self.shown.iter().position(|&i| i == self.current) else {
            return;
        };
        let next = if forward {
            at.checked_add(1)
        } else {
            at.checked_sub(1)
        };
        if let Some(&index) = next.and_then(|n| self.shown.get(n)) {
            self.show(index, cx);
        }
    }

    fn show(&mut self, index: usize, cx: &mut Context<Self>) {
        if index != self.current {
            self.current = index;
            // A new topic is read from its top.
            self.scroll.set_offset(point(px(0.), px(0.)));
        }
        cx.notify();
    }

    /// Runs the search again (H7): the list keeps the matching topics, and when the current
    /// topic no longer matches the first match is shown.
    fn refilter(&mut self, cx: &mut Context<Self>) {
        let query = self.query(cx);
        self.shown = topics::matching(&query, &self.texts);
        if !self.shown.contains(&self.current)
            && let Some(&first) = self.shown.first()
        {
            self.show(first, cx);
        }
        cx.notify();
    }

    /// Ctrl+F: the caret goes to the search box, its text selected.
    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.search.focus_handle(cx), cx);
        self.search
            .update(cx, |state, cx| state.select_all(window, cx));
    }

    /// Escape clears a search that holds text; otherwise it goes on.
    fn on_escape(&mut self, _: &HelpEscape, window: &mut Window, cx: &mut Context<Self>) {
        if self.query(cx).is_empty() {
            cx.propagate();
            return;
        }
        self.set_query("", window, cx);
    }

    fn render_side(
        &self,
        p: &Palette,
        copy: PageCopy,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        // The two segments split the switch's width, each label centred (U7).
        let segment = |lang: HelpLang, index: usize, label: &'static str, last: bool| {
            let on = self.lang == lang;
            ld_segment(("help-lang", index), label, on, last, p)
                .flex_none()
                .w(relative(0.5))
                .justify_center()
                .when(on, |s| s.bg(p.accent).text_color(p.accent_ink))
                .ld_click(
                    true,
                    cx.listener(move |this, _, window, cx| this.set_lang(lang, window, cx)),
                )
        };
        let (accent, panel3) = (p.accent, p.panel3);
        let entry = |i: usize| {
            let on = i == self.current;
            let title = TOPICS[i].text(self.lang).title;
            // One left-aligned column; a title that wraps grows its entry, with room between
            // its rows (HV check 4). The icon sits on the first row.
            ld_card(("help-toc", i), title, p)
                .w_full()
                .h_auto()
                .justify_start()
                .items_start()
                .gap(px(8.))
                .line_height(relative(TOC_LINE_HEIGHT))
                .px(px(9.))
                .py(px(5.))
                .rounded(px(6.))
                .border_color(transparent_black())
                .bg(if on { panel3 } else { transparent_black() })
                .text_color(if on { accent } else { p.text })
                .when(on, |s| s.font_weight(FontWeight::SEMIBOLD))
                .ld_click(true, cx.listener(move |this, _, _, cx| this.show(i, cx)))
                .child(first_line(UI_SIZE * TOC_LINE_HEIGHT).child(
                    ld_icon(TOPICS[i].icon, TOC_ICON_SIZE).text_color(if on {
                        accent
                    } else {
                        p.muted
                    }),
                ))
                .child(
                    prose(title)
                        .flex_1()
                        .id(("help-toc-label", i))
                        .test_support(),
                )
        };
        // The topics in their groups; a group with no topic the search keeps leaves (U10).
        let mut toc = Vec::new();
        for (g, group) in GROUPS.iter().enumerate() {
            let members: Vec<usize> = self
                .shown
                .iter()
                .copied()
                .filter(|&i| topics::group_of(i) == g)
                .collect();
            if members.is_empty() {
                continue;
            }
            let label = group.label(self.lang);
            toc.push(
                div()
                    .id(("help-group", g))
                    .test_support()
                    .aria_label(label)
                    .when(!toc.is_empty(), |d| d.mt(px(10.)))
                    .px(px(9.))
                    .pb(px(2.))
                    .text_size(px(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.dim)
                    .child(prose(label))
                    .into_any_element(),
            );
            toc.extend(members.into_iter().map(|i| entry(i).into_any_element()));
        }
        v_flex()
            .id("help-side")
            .test_support()
            .flex_none()
            .w(px(SIDE_WIDTH))
            .h_full()
            .gap(px(10.))
            .px(px(12.))
            .py(px(14.))
            .bg(p.panel)
            .border_r_1()
            .border_color(p.line)
            .child(
                ld_segmented(
                    "help-lang",
                    [
                        segment(HelpLang::Th, 0, "ไทย", false),
                        segment(HelpLang::En, 1, "English", true),
                    ],
                    p,
                )
                .w_full(),
            )
            .child(
                div()
                    .id("help-search")
                    .test_support()
                    .child(Input::new(&self.search).aria_label(copy.search_name)),
            )
            .child(
                v_flex()
                    .id("help-toc")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .gap(px(2.))
                    .overflow_y_scroll()
                    .children(toc),
            )
    }

    fn render_topic(
        &self,
        p: &Palette,
        fonts: &Fonts,
        copy: PageCopy,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.current().is_none() {
            return div()
                .id("help-empty")
                .test_support()
                .aria_label(copy.empty)
                .flex_1()
                .min_w_0()
                .px(px(28.))
                .py(px(22.))
                .text_color(p.dim)
                .child(prose(copy.empty))
                .into_any_element();
        }
        let text = TOPICS[self.current].text(self.lang);
        let mut pieces = Pieces::new(p, &fonts.code);
        let title = div()
            .id("help-title")
            .test_support()
            .aria_label(text.title)
            .text_size(px(20.))
            .font_weight(FontWeight::BOLD)
            .child(pieces.row(text.title));
        let mut blocks: Vec<AnyElement> = Vec::new();
        for block in text.body {
            blocks.push(match *block {
                // A calm heading in the text colour, marked by a short accent bar (U9).
                Block::Heading(t) => h_flex()
                    .mt(px(8.))
                    .items_start()
                    .gap(px(8.))
                    .child(
                        first_line(HEADING_SIZE * LINE_HEIGHT)
                            .child(div().w(px(3.)).h(px(14.)).rounded(px(1.5)).bg(p.accent)),
                    )
                    .child(
                        pieces
                            .row(t)
                            .flex_1()
                            .text_size(px(HEADING_SIZE))
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .into_any_element(),
                Block::Para(t) => pieces.row(t).into_any_element(),
                Block::Bullets(items) => {
                    let mut list = v_flex().gap(px(5.));
                    for &item in items {
                        list = list.child(
                            h_flex()
                                .items_start()
                                .gap(px(9.))
                                .child(first_line(BODY_SIZE * LINE_HEIGHT).child(ld_dot(4., p.dim)))
                                .child(pieces.row(item).flex_1()),
                        );
                    }
                    list.into_any_element()
                }
                Block::Shortcuts => {
                    let mut sheet = v_flex().gap(px(6.));
                    let labels = topics::shortcut_labels(self.lang);
                    for (keys, label) in self.keys.iter().zip(labels) {
                        sheet =
                            sheet.child(
                                h_flex()
                                    .items_start()
                                    .gap(px(10.))
                                    .child(
                                        h_flex()
                                            .w(px(170.))
                                            .flex_none()
                                            .flex_wrap()
                                            .gap(px(4.))
                                            .children(keys.split(" / ").map(|combo| {
                                                ld_kbd(combo.to_owned(), &fonts.code, p)
                                            })),
                                    )
                                    .child(pieces.row(label).flex_1().text_color(p.muted)),
                            );
                    }
                    sheet.into_any_element()
                }
                Block::Legend => {
                    let mut legend = v_flex().gap(px(6.));
                    for (glyph, meaning) in topics::legend_rows(self.lang) {
                        legend = legend.child(
                            h_flex()
                                .items_start()
                                .gap(px(10.))
                                .child(div().flex_none().w(px(92.)).child(sample(glyph, p)))
                                .child(pieces.row(meaning).flex_1().text_color(p.muted)),
                        );
                    }
                    legend.into_any_element()
                }
            });
        }
        let at = self.shown.iter().position(|&i| i == self.current);
        let has_prev = at.is_some_and(|at| at > 0);
        let has_next = at.is_some_and(|at| at + 1 < self.shown.len());
        let nav = h_flex()
            .mt(px(14.))
            .pt(px(12.))
            .justify_between()
            .border_t_1()
            .border_color(p.line)
            .child(
                ld_button("help-prev", copy.prev, p)
                    .ld_click(has_prev, cx.listener(|this, _, _, cx| this.step(false, cx))),
            )
            .child(
                ld_button("help-next", copy.next, p)
                    .ld_click(has_next, cx.listener(|this, _, _, cx| this.step(true, cx))),
            );
        v_flex()
            .id("help-topic")
            .test_support()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(CONTENT_WIDTH))
                    .gap(px(10.))
                    .px(px(28.))
                    .py(px(22.))
                    .child(title)
                    .children(blocks)
                    .child(nav),
            )
            .into_any_element()
    }
}

/// Draws a topic's texts as pieces and hands out the ids of its pieces, chips and keys in
/// drawing order, which `topics::topic_pieces` follows, so the drawn tests find each one (H9,
/// U11).
struct Pieces<'a> {
    p: &'a Palette,
    code: &'a Font,
    next: usize,
    chips: usize,
    keys: usize,
}

impl<'a> Pieces<'a> {
    fn new(p: &'a Palette, code: &'a Font) -> Self {
        Self {
            p,
            code,
            next: 0,
            chips: 0,
            keys: 0,
        }
    }

    /// `text` as no-wrap pieces in a wrapping row, at the prose's line height (U9).
    fn row(&mut self, text: &'static str) -> Div {
        let mut row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .min_w_0()
            .line_height(relative(LINE_HEIGHT));
        for piece in topics::pieces(text) {
            let id = self.next;
            self.next += 1;
            let mut drawn = div()
                .id(("help-piece", id))
                .test_support()
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .whitespace_nowrap();
            for span in markup::spans(piece) {
                drawn = match span {
                    Span::Text(t) => drawn.child(t),
                    Span::Chip(label) => drawn.child(self.chip(label)),
                    Span::Key(keys) => drawn.child(self.key(keys)),
                };
            }
            row = row.child(drawn);
        }
        row
    }

    /// A control's name (U8): panel2, a 1 px line border, radius 4, 12.5 px text in the text
    /// colour; an icon-only control's chip shows its icon too.
    fn chip(&mut self, label: &'static str) -> impl IntoElement + use<> {
        let id = self.chips;
        self.chips += 1;
        let p = self.p;
        div()
            .id(("help-chip", id))
            .test_support()
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .px(px(5.))
            .rounded(px(4.))
            .border_1()
            .border_color(p.line)
            .bg(p.panel2)
            .text_size(px(CHIP_SIZE))
            .line_height(px(16.))
            .font_weight(FontWeight::NORMAL)
            .text_color(p.text)
            .when_some(topics::chip_icon(label, p.dark), |chip, icon| {
                chip.child(ld_icon(icon, SMALL_ICON_SIZE))
            })
            .child(label)
    }

    /// Keys, as the shortcut sheet draws them (U8).
    fn key(&mut self, keys: &'static str) -> impl IntoElement + use<> {
        let id = self.keys;
        self.keys += 1;
        ld_kbd(keys, self.code, self.p)
            .id(("help-key", id))
            .test_support()
    }
}

/// Texts drawn as the topic page draws its prose (H6, U8): each a row of no-wrap pieces, its
/// keys as key caps. For the help's one surface outside the page, the Start tab's welcome card
/// (U15); the pieces are numbered across the rows.
pub fn prose_rows(texts: &[&'static str], p: &Palette, code: &Font) -> Vec<Div> {
    let mut pieces = Pieces::new(p, code);
    texts.iter().map(|text| pieces.row(text)).collect()
}

/// A box one line high that centres what it holds on a text's first line: a bullet's dot, a
/// heading's bar, a topic-list icon.
fn first_line(height: f32) -> Div {
    div().flex().flex_none().items_center().h(px(height))
}

/// A text as no-wrap pieces in a wrapping row (H6), for the topic list and the empty message.
fn prose(text: &'static str) -> Div {
    div().flex().flex_row().flex_wrap().min_w_0().children(
        topics::pieces(text)
            .into_iter()
            .map(|piece| div().flex_none().whitespace_nowrap().child(piece)),
    )
}

impl Render for HelpView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current(cx);
        let fonts = Fonts::current(cx);
        let copy = topics::page_copy(self.lang);
        let side = self.render_side(&p, copy, cx);
        let topic = self.render_topic(&p, &fonts, copy, cx);
        h_flex()
            .id("help-view")
            .test_support()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .items_start()
            .bg(p.bg)
            .text_color(p.text)
            .font(fonts.ui.clone())
            .text_size(px(BODY_SIZE))
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::on_escape))
            .child(side)
            .child(topic)
    }
}
