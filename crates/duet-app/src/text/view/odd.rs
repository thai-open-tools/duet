//! Invisible characters of the text view (R19): the background scan, the status button and the
//! walk from hit to hit.
//!
//! The scan of a side is cached by its buffer version: a compare reads both sides again, a
//! re-diff only the side whose version moved. It runs on the background executor, and a side
//! over 64 MiB is not scanned. The markers drawn in the rows come from `display_line`, not from
//! this scan, so they never wait for it; the details pane shows them too, but only inside the
//! bounded prefix `wrap_prefix` takes of a line (E7).

use super::*;
use crate::text::rowsel::RowSel;
use duet_core::text::odd::{OddHit, OddPlace, OddScan, find_odd, odd_info};

/// A side longer than this is not scanned.
const MAX_ODD_BYTES: usize = 64 << 20;
/// Hits kept per side (the counts cover all of them).
const ODD_HITS: usize = 5_000;
/// Hits the button's tooltip lists.
const TIP_HITS: usize = 6;

/// The scan of one side.
#[derive(Default)]
pub(crate) struct OddSide {
    scan: Option<Arc<OddScan>>,
    /// The buffer version the scan (or the scan under way) reads; None before the first one.
    version: Option<Option<u64>>,
    job: JobSlot,
    /// Scans started so far, for the tests.
    runs: usize,
    /// Scans that had to copy an edited buffer, for the tests.
    copies: usize,
}

/// The order the status button walks the hits of both sides in: (left, index in its scan),
/// those in the text first. Worked out when a scan lands or goes and on F9, never per frame.
#[derive(Default)]
pub(crate) struct OddOrder {
    hits: Vec<(bool, usize)>,
    /// Times the order was worked out, for the tests.
    sorts: std::cell::Cell<usize>,
}

/// The text a scan reads: a document shared with the model, or a copy of an edited buffer.
enum Source {
    Doc(Arc<duet_core::text::document::TextDocument>),
    Text(String),
}

impl Source {
    fn text(&self) -> &str {
        match self {
            Self::Doc(doc) => doc.text().unwrap_or_default(),
            Self::Text(text) => text,
        }
    }
}

fn place_words(place: OddPlace) -> &'static str {
    match place {
        OddPlace::Comment => "in a comment",
        OddPlace::Indent => "in the indentation",
        OddPlace::Text => "in the text",
    }
}

fn characters(n: usize) -> String {
    format!("{n} invisible character{}", if n == 1 { "" } else { "s" })
}

impl TextView {
    /// Reads side `left` again in the background. `keep`: the older result stays on screen until
    /// the new one lands (an edit); otherwise it goes at once (another file).
    pub(crate) fn start_odd(&mut self, left: bool, keep: bool, cx: &mut Context<Self>) {
        let index = usize::from(!left);
        self.odd[index].job.cancel();
        if !keep {
            self.odd[index].scan = None;
            self.reorder_odd();
        }
        // The model's document whenever it holds the side's text (after a re-diff landed it
        // does): only a buffer ahead of its document is copied (I3).
        let source = self.model.as_ref().and_then(|model| {
            let source = match model.current_document(left) {
                Some(doc) => {
                    if doc.text().is_none_or(|text| text.len() > MAX_ODD_BYTES) {
                        return None;
                    }
                    Source::Doc(doc.clone())
                }
                None => {
                    let buffer = model.buffer(left)?;
                    if buffer.len() > MAX_ODD_BYTES {
                        return None;
                    }
                    self.odd[index].copies += 1;
                    Source::Text(buffer.text().to_owned())
                }
            };
            Some((source, model.versions()[index]))
        });
        let Some((source, version)) = source else {
            let side = &mut self.odd[index];
            (side.scan, side.version) = (None, None);
            self.reorder_odd();
            cx.notify();
            return;
        };
        let (language, swaps) = (self.comment_language(), self.syntax_swaps);
        let side = &mut self.odd[index];
        side.version = Some(version);
        side.runs += 1;
        let (generation, cancel) = side.job.start();
        let background = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let scan = background
                .spawn(async move { find_odd(source.text(), language, ODD_HITS, &cancel).ok() })
                .await;
            if let Some(scan) = scan {
                let _ = this.update(cx, |view, cx| {
                    view.land_odd(left, swaps, generation, scan, cx)
                });
            }
        })
        .detach();
    }

    /// A re-diff landed: reads again the sides whose text moved since their scan.
    pub(crate) fn rescan_stale_odd(&mut self, cx: &mut Context<Self>) {
        let Some(versions) = self.model.as_ref().map(TextModel::versions) else {
            return;
        };
        for left in [true, false] {
            let index = usize::from(!left);
            if self.odd[index].version != Some(versions[index]) {
                self.start_odd(left, true, cx);
            }
        }
    }

    fn land_odd(
        &mut self,
        left: bool,
        swaps: u64,
        generation: u64,
        scan: OddScan,
        cx: &mut Context<Self>,
    ) {
        // F9 moved the scanned text, and its job, to the other side.
        let left = left != ((self.syntax_swaps - swaps) % 2 == 1);
        let side = &mut self.odd[usize::from(!left)];
        if !side.job.is_current(generation) {
            return;
        }
        side.job.finish(generation);
        side.scan = Some(Arc::new(scan));
        self.reorder_odd();
        cx.notify();
    }

    /// Puts the hits of both sides in the order the button walks them: those in the text first,
    /// then the rest, the left side before the right and each side in its own order. The walk
    /// starts again from the first.
    pub(crate) fn reorder_odd(&mut self) {
        let mut order = Vec::new();
        for in_text in [true, false] {
            for left in [true, false] {
                if let Some(scan) = self.odd[usize::from(!left)].scan.as_deref() {
                    order.extend(
                        scan.hits
                            .iter()
                            .enumerate()
                            .filter(|(_, hit)| (hit.place == OddPlace::Text) == in_text)
                            .map(|(at, _)| (left, at)),
                    );
                }
            }
        }
        self.odd_order.hits = order;
        let sorts = &self.odd_order.sorts;
        sorts.set(sorts.get() + 1);
        self.odd_pos = None;
    }

    /// (hits in the text, hits in comments or indentation) over both sides; None when there
    /// are none.
    pub fn odd_counts(&self) -> Option<(usize, usize)> {
        self.model.as_ref()?;
        let (warn, info) = self
            .odd
            .iter()
            .filter_map(|side| side.scan.as_deref())
            .fold((0, 0), |(w, i), scan| (w + scan.warn, i + scan.info));
        (warn + info > 0).then_some((warn, info))
    }

    /// The status button: a warning for hits in the text, a plain count for the rest.
    pub fn odd_button_text(&self) -> Option<String> {
        let (warn, info) = self.odd_counts()?;
        Some(if warn > 0 {
            format!("⚠ {} in the text", characters(warn))
        } else {
            format!("{} in comments or indentation", characters(info))
        })
    }

    /// Hit `at` of the walk (see `reorder_odd`) and its side.
    fn odd_hit(&self, at: usize) -> Option<(bool, &OddHit)> {
        let &(left, index) = self.odd_order.hits.get(at)?;
        let scan = self.odd[usize::from(!left)].scan.as_deref()?;
        Some((left, scan.hits.get(index)?))
    }

    fn odd_hit_text((left, hit): (bool, &OddHit)) -> String {
        format!(
            "{} line {}: {} ({})",
            if left { "Left" } else { "Right" },
            hit.line + 1,
            odd_info(hit.ch).1,
            place_words(hit.place)
        )
    }

    pub fn odd_tooltip(&self) -> Option<String> {
        let (warn, _) = self.odd_counts()?;
        let count = |left: bool| {
            self.odd[usize::from(!left)]
                .scan
                .as_deref()
                .map_or(0, |scan| scan.count)
        };
        let mut tip = format!(
            "Left {} · Right {}. Click for the next one (Shift+click: previous).",
            count(true),
            count(false)
        );
        for hit in (0..TIP_HITS).map_while(|at| self.odd_hit(at)) {
            tip.push('\n');
            tip.push_str(&Self::odd_hit_text(hit));
        }
        if warn > 0 {
            tip.push_str(
                "\n\nInvisible characters inside values can make equal-looking text differ.",
            );
        }
        Some(tip)
    }

    /// Goes to the next (or previous) hit: its row is selected and centred, and the notice says
    /// which one it is. False when there is none to go to.
    pub fn next_odd(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        let total = self.odd_order.hits.len();
        if total == 0 {
            return false;
        }
        let at = match (self.odd_pos, forward) {
            (None, true) => 0,
            (None, false) => total - 1,
            (Some(at), true) => (at + 1) % total,
            (Some(at), false) => (at + total - 1) % total,
        };
        let Some(hit) = self.odd_hit(at) else {
            return false;
        };
        let text = Self::odd_hit_text(hit);
        let (left, line) = (hit.0, hit.1.line);
        self.odd_pos = Some(at);
        let Some(model) = self.model.as_mut() else {
            return false;
        };
        let Some(row) = model.row_of_line(left, line) else {
            return false;
        };
        if let Some(display) = model.reveal_row(row) {
            self.scroll_to_display(display, ScrollStrategy::Center);
        }
        self.row_sel = Some(RowSel {
            left,
            anchor: row,
            head: row,
        });
        self.caret = None;
        self.menu_row = None;
        self.notice = Some(format!("{} / {total} · {text}", at + 1).into());
        cx.notify();
        true
    }

    /// The status bar's button; None without hits.
    pub(super) fn render_odd(
        &self,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let text = self.odd_button_text()?;
        let warn = self.odd_counts().is_some_and(|(warn, _)| warn > 0);
        let button = ld_button("text-odd", text, palette)
            .ld_small()
            .when(warn, |b| {
                b.text_color(palette.del)
                    .border_color(palette.del)
                    .bg(palette.bg_del)
            })
            .when(!warn, |b| b.text_color(palette.muted));
        Some(
            button
                .ld_tooltip(self.odd_tooltip().unwrap_or_default())
                .ld_click(
                    true,
                    cx.listener(|this, event: &ClickEvent, _, cx| {
                        this.next_odd(!event.modifiers().shift, cx);
                    }),
                ),
        )
    }

    #[doc(hidden)]
    pub fn odd_scans_for_test(&self) -> [usize; 2] {
        [self.odd[0].runs, self.odd[1].runs]
    }

    /// Test hook: scans that copied an edited buffer, per side.
    #[doc(hidden)]
    pub fn odd_copies_for_test(&self) -> [usize; 2] {
        [self.odd[0].copies, self.odd[1].copies]
    }

    /// Test hook: how often the order of the hits was worked out.
    #[doc(hidden)]
    pub fn odd_sorts_for_test(&self) -> usize {
        self.odd_order.sorts.get()
    }
}
