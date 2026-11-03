//! The picture view's heads' texts, status line, legend and pixel probe (`js:453-479`,
//! `js:598-627`).

use duet_core::picture::diff::{C_DIFF, C_MINOR, C_ONLY, delta, hex_color, percent_text};
use gpui_kit::component::h_flex;
use gpui_kit::{AnyElement, FontWeight, Hsla, div, rgb};

use super::*;
use crate::format::{count_of, format_size, format_time, human_size};
use crate::picture::prefs::Mode;
use crate::status::{StatusPart, Tone};
use crate::ui::fonts::Fonts;
use crate::ui::style::{status_bar, status_line_box, status_parts_row};

const OPEN_OTHER: &str = "Open the other picture to start comparing.";
const TOO_BIG: &str =
    "A picture is larger than 40 megapixels: it is shown, but pixels are not compared.";
const NOT_THE_SAME_SIZE: &str = "Not the same size as the other side";

/// What one head shows: the picture's name, its size line, and how to colour them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadText {
    pub name: String,
    /// "{w}×{h} · {size} · {format}", "Loading…" or "Can't open".
    pub meta: String,
    /// The other side has other dimensions.
    pub dims_differ: bool,
    pub failed: bool,
}

/// What one picture has under the probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Probed {
    Pixel([u8; 4]),
    /// The picture has no pixel there, or is not there.
    Missing,
    /// A picture over the pixel limit: its pixels are not held.
    Big,
}

fn colour(rgb_bytes: [u8; 3]) -> Hsla {
    let [r, g, b] = rgb_bytes.map(u32::from);
    rgb((r << 16) | (g << 8) | b).into()
}

impl PictureView {
    /// "Loading {name}…" while a side loads (both names while both do); None otherwise. The
    /// status says it in every mode, and so does the single pane (I-55, C24).
    pub(super) fn loading_text(&self) -> Option<String> {
        let names: Vec<String> = [true, false]
            .into_iter()
            .filter(|&left| self.loading(left))
            .map(|left| file_name(&self.paths[side_index(left)]))
            .collect();
        (!names.is_empty()).then(|| format!("Loading {}…", names.join(" and ")))
    }

    /// The status line as coloured parts (`js:598-627`).
    pub fn status_parts(&self) -> Vec<StatusPart> {
        if let Some(loading) = self.loading_text() {
            return vec![StatusPart::new(loading, Tone::Muted)];
        }
        let (Some(left), Some(right)) = (&self.sides[LEFT], &self.sides[RIGHT]) else {
            return vec![StatusPart::new(OPEN_OTHER, Tone::Changed)];
        };
        if self.too_big() {
            return vec![StatusPart::new(TOO_BIG, Tone::Changed)];
        }
        let Some(diff) = self.diff() else {
            return Vec::new();
        };
        let mut parts = Vec::new();
        if diff.count == 0 && diff.minor == 0 {
            let text = if diff.size_mismatch {
                "Every pixel of the overlap is the same."
            } else {
                "Every pixel is the same."
            };
            parts.push(StatusPart::new(text, Tone::RightOnly));
        } else if diff.count == 0 {
            parts.push(StatusPart::new(
                "Same within the tolerance",
                Tone::RightOnly,
            ));
            parts.push(StatusPart::new(
                format!(
                    "{}, largest difference {}",
                    count_of(
                        diff.minor,
                        "pixel differs slightly",
                        "pixels differ slightly"
                    ),
                    diff.max_delta
                ),
                Tone::Muted,
            ));
        } else {
            parts.push(StatusPart::new(
                format!(
                    "{} ({})",
                    count_of(diff.count, "pixel differs", "pixels differ"),
                    percent_text(diff.count, diff.total)
                ),
                Tone::LeftOnly,
            ));
            if diff.minor > 0 {
                parts.push(StatusPart::new(
                    format!("{} more within the tolerance", format_size(diff.minor)),
                    Tone::Muted,
                ));
            }
            let at = self
                .current_group()
                .map_or("–".to_owned(), |at| format_size(at as u64 + 1));
            parts.push(StatusPart::new(
                format!("Group {at} / {}", format_size(diff.clusters.len() as u64)),
                Tone::Text,
            ));
        }
        if diff.size_mismatch {
            let (a, b, c) = (&left.info, &right.info, diff.overlap);
            parts.insert(
                1,
                StatusPart::new(
                    format!(
                        "Sizes differ ({}×{} and {}×{}): only the {}×{} overlap is compared.",
                        a.width, a.height, b.width, b.height, c.0, c.1
                    ),
                    Tone::Changed,
                ),
            );
        }
        parts
    }

    /// "Tolerance 5 · Side by side", or "· computing…" while the result for it is on its way.
    pub fn tolerance_text(&self) -> String {
        let computing = self.tolerance.pending || self.diff_job.is_running();
        format!(
            "Tolerance {} · {}",
            self.prefs.tolerance,
            if computing {
                "computing…"
            } else {
                self.prefs.mode.label()
            }
        )
    }

    /// What a side's head shows.
    pub fn head_text(&self, left: bool) -> HeadText {
        let index = side_index(left);
        let named = |index: usize| {
            let path = &self.paths[index];
            if path.as_os_str().is_empty() {
                String::new()
            } else {
                file_name(path)
            }
        };
        if let Some(side) = &self.sides[index] {
            let info = &side.info;
            let dims_differ = self.sides[1 - index].as_ref().is_some_and(|other| {
                (other.info.width, other.info.height) != (info.width, info.height)
            });
            return HeadText {
                name: info.name.clone(),
                meta: format!(
                    "{}×{} · {} · {}",
                    info.width,
                    info.height,
                    human_size(info.size),
                    info.format
                ),
                dims_differ,
                failed: false,
            };
        }
        let failed = self.messages[index].is_some();
        let meta = if failed {
            "Can't open"
        } else if self.loads[index].is_running() {
            "Loading…"
        } else {
            ""
        };
        HeadText {
            name: named(index),
            meta: meta.to_owned(),
            dims_differ: false,
            failed,
        }
    }

    /// The tooltip of a head's name: the full path, or the name of a picture with no file
    /// (LocalDiff `path || name`, `js:490`); None while the head names nothing.
    pub fn name_tooltip(&self, left: bool) -> Option<String> {
        let index = side_index(left);
        let name = self.head_text(left).name;
        if name.is_empty() {
            return None;
        }
        let path = self.sides[index]
            .as_ref()
            .and_then(|side| side.info.path.clone())
            .unwrap_or_else(|| self.paths[index].clone());
        Some(if path.as_os_str().is_empty() {
            name
        } else {
            path.display().to_string()
        })
    }

    /// The tooltip of a head's size line.
    pub(super) fn head_tooltip(&self, left: bool) -> Option<String> {
        let index = side_index(left);
        if let Some(message) = &self.messages[index] {
            return Some(message.to_string());
        }
        let side = self.sides[index].as_ref()?;
        let mut lines = Vec::new();
        if self.head_text(left).dims_differ {
            lines.push(NOT_THE_SAME_SIZE.to_owned());
        }
        if let Some(modified) = side.info.modified {
            lines.push(format!("Modified {}", format_time(modified)));
        }
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    /// What each picture has at the probed pixel, left then right.
    fn probed(&self) -> Option<((u32, u32), [Probed; 2])> {
        let (x, y) = self.probe?;
        if self.sides.iter().all(Option::is_none) {
            return None;
        }
        let at = |index: usize| match &self.sides[index] {
            None => Probed::Missing,
            Some(side) if side.info.big => Probed::Big,
            Some(side) => side
                .pixels
                .as_ref()
                .and_then(|picture| picture.pixel(x, y))
                .map_or(Probed::Missing, Probed::Pixel),
        };
        Some(((x, y), [at(LEFT), at(RIGHT)]))
    }

    /// "{x}, {y} · Left {value} · Right {value}" for the pixel under the pointer, without the
    /// swatches; None while the pointer is not over a picture.
    pub fn probe_text(&self) -> Option<String> {
        let ((x, y), values) = self.probed()?;
        let value = |probed: Probed| match probed {
            Probed::Pixel(pixel) => hex_color(pixel),
            Probed::Missing => "—".to_owned(),
            Probed::Big => "?".to_owned(),
        };
        Some(format!(
            "{x}, {y} · Left {} · Right {}",
            value(values[LEFT]),
            value(values[RIGHT])
        ))
    }

    /// Both pictures have the pixel and it differs by more than the tolerance.
    pub fn probe_differs(&self) -> bool {
        match self.probed() {
            Some((_, [Probed::Pixel(a), Probed::Pixel(b)])) => delta(a, b) > self.prefs.tolerance,
            _ => false,
        }
    }

    /// The export button: a computed difference.
    pub fn can_export(&self) -> bool {
        self.compared.is_some()
    }

    /// The group buttons: the difference has groups.
    pub fn can_navigate(&self) -> bool {
        self.diff().is_some_and(|diff| !diff.clusters.is_empty())
    }

    /// The swap button: any side is there, loading or failed.
    pub fn can_swap(&self) -> bool {
        self.sides.iter().any(Option::is_some)
            || self.messages.iter().any(Option::is_some)
            || self.loads_running()
    }

    /// The reload button: a side has a file.
    pub fn can_reload(&self) -> bool {
        self.paths.iter().any(|path| !path.as_os_str().is_empty())
    }

    /// The legend of the difference image's colours (R18).
    fn render_legend(&self, p: &Palette) -> impl IntoElement {
        let item = |id: &'static str, color: [u8; 3], label: &'static str| {
            h_flex()
                .id(id)
                .test_support()
                .flex_none()
                .items_center()
                .gap(px(5.))
                .aria_label(label)
                .child(
                    div()
                        .flex_none()
                        .size(px(9.))
                        .rounded(px(2.))
                        .bg(colour(color)),
                )
                .child(label)
        };
        h_flex()
            .id("picture-legend")
            .test_support()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .whitespace_nowrap()
            .text_color(p.muted)
            .child(item("picture-legend-diff", C_DIFF, "Differs"))
            .child(item("picture-legend-minor", C_MINOR, "Within tolerance"))
            .child(item("picture-legend-only", C_ONLY, "One side only"))
    }

    /// The probe: the pixel's place and each picture's value after a swatch of its colour.
    fn render_probe(&self, p: &Palette, fonts: &Fonts) -> Option<impl IntoElement> {
        let ((x, y), values) = self.probed()?;
        let differs = self.probe_differs();
        let value = move |probed: Probed| {
            let (text, swatch) = match probed {
                Probed::Pixel(pixel) => (hex_color(pixel), Some(pixel)),
                Probed::Missing => ("—".to_owned(), None),
                Probed::Big => ("?".to_owned(), None),
            };
            let text = div()
                .when(differs, |d| {
                    d.text_color(p.del).font_weight(FontWeight::BOLD)
                })
                .child(text);
            h_flex()
                .gap(px(4.))
                .items_center()
                .children(swatch.map(|pixel| {
                    div()
                        .flex_none()
                        .size(px(10.))
                        .border_1()
                        .border_color(p.line)
                        .bg(colour([pixel[0], pixel[1], pixel[2]]))
                }))
                .child(text)
        };
        Some(
            h_flex()
                .id("picture-probe")
                .test_support()
                .flex_none()
                .items_center()
                .gap(px(4.))
                .whitespace_nowrap()
                .font(fonts.code.clone())
                .child(format!("{x}, {y} · Left"))
                .child(value(values[LEFT]))
                .child("· Right")
                .child(value(values[RIGHT])),
        )
    }

    /// The status bar: the parts, then the legend, the probe, the tolerance and the notice.
    pub(super) fn render_status(&self, p: &Palette, fonts: &Fonts) -> AnyElement {
        let parts = self.status_parts();
        let legend = (self.mode() == Mode::Difference && self.compared.is_some())
            .then(|| self.render_legend(p));
        let notice = self.notice().map(|notice| {
            div()
                .id("picture-notice")
                .test_support()
                .flex_none()
                .text_color(p.text)
                .child(notice.to_owned())
        });
        status_bar("picture-status", p)
            .child(if parts.is_empty() {
                status_line_box("")
            } else {
                status_parts_row(&parts, p)
            })
            .children(legend)
            .children(self.render_probe(p, fonts))
            .child(
                div()
                    .id("picture-status-tolerance")
                    .test_support()
                    .flex_none()
                    .child(self.tolerance_text()),
            )
            .children(notice)
            .into_any_element()
    }
}
