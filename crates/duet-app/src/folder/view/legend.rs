//! The legend: what every symbol and colour of the folder list means.

use gpui_kit::TestSupportExt as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{AnyElement, FontWeight, IntoElement, deferred, div, prelude::*, px};

use crate::help::topics::{self, HelpLang};
use crate::i18n::{paired, tr, tr_key};
use crate::palette::Palette;
use crate::ui::prose::ProseExt as _;
use crate::ui::style::ld_pill;

/// A glyph (or chip text) and its meaning (`85-help.js:134-151`).
pub const LEGEND: [(&str, &str); 13] = [
    (paired("≠"), paired("Different: the contents differ")),
    (
        paired("≈"),
        paired(
            "Differs only in unimportant parts (line endings, final newline, BOM, or the text rules); counted with Same",
        ),
    ),
    (
        paired("⚠"),
        paired(
            "Counts as different and needs a look: a Linux script with CRLF or a BOM, files that differ only in the kind of whitespace (they look the same), or names that differ only in case (two files on Linux)",
        ),
    ),
    (
        paired("⚠ invisible"),
        paired(
            "The file has invisible characters (NBSP, LRM, zero-width) in its text or values: a value may be wrong without anyone seeing it",
        ),
    ),
    (
        paired("invisible"),
        paired("Invisible characters only in comments or indentation: no value changes"),
    ),
    (paired("="), paired("Same in every byte")),
    (paired("◀"), paired("Left only (red name)")),
    (paired("▶"), paired("Right only (green name)")),
    (
        paired("▲ date"),
        paired("The newer side; shown for different files only"),
    ),
    (
        paired("other time"),
        paired("Same contents but different modified times (not counted as a difference)"),
    ),
    (
        paired("3 files"),
        paired("Files inside that match the current filter (files, not lines)"),
    ),
    (
        paired("hatched"),
        paired("The item is missing on that side"),
    ),
    (paired("!"), paired("Unreadable (no permission, or locked)")),
];

/// The legend in Thai, for the help (H4): each glyph beside its Thai meaning, in `LEGEND`
/// order.
pub const LEGEND_TH: [(&str, &str); 13] = [
    (paired("≠"), paired("ต่างกัน: เนื้อหาไม่เหมือนกัน")),
    (
        paired("≈"),
        paired("ต่างแค่ส่วนที่ไม่สำคัญ (ท้ายบรรทัด ขึ้นบรรทัดท้ายไฟล์ BOM หรือตามกฎข้อความ) นับอยู่ในกลุ่ม เหมือนกัน"),
    ),
    (
        paired("⚠"),
        paired(
            "นับว่าต่างและควรตรวจ: สคริปต์ Linux ที่มี CRLF หรือ BOM, ไฟล์ที่ต่างแค่ชนิดช่องว่าง (ตาเห็นเหมือนกัน) หรือชื่อต่างแค่ตัวพิมพ์เล็กใหญ่ (บน Linux เป็นสองไฟล์)",
        ),
    ),
    (
        paired("⚠ invisible"),
        paired("ไฟล์มีอักขระที่มองไม่เห็น (NBSP, LRM, zero-width) ในเนื้อหาหรือค่า อาจทำให้ค่าผิดโดยไม่มีใครเห็น"),
    ),
    (
        paired("invisible"),
        paired("มีอักขระที่มองไม่เห็น แต่อยู่แค่ในคอมเมนต์หรือย่อหน้า ค่ายังไม่เปลี่ยน"),
    ),
    (paired("="), paired("เหมือนกันทุกไบต์")),
    (paired("◀"), paired("มีเฉพาะฝั่งซ้าย (ชื่อสีแดง)")),
    (paired("▶"), paired("มีเฉพาะฝั่งขวา (ชื่อสีเขียว)")),
    (paired("▲ date"), paired("ฝั่งที่ใหม่กว่า แสดงเฉพาะไฟล์ที่ต่างกัน")),
    (
        paired("other time"),
        paired("เนื้อหาเหมือนกัน แต่เวลาแก้ไขต่างกัน (ไม่นับว่าต่าง)"),
    ),
    (
        paired("3 files"),
        paired("จำนวนไฟล์ข้างในที่ตรงกับตัวกรองตอนนี้ (นับไฟล์ ไม่ใช่บรรทัด)"),
    ),
    (paired("hatched"), paired("ช่องลายทแยง: ฝั่งนั้นไม่มีรายการนี้")),
    (paired("!"), paired("อ่านไม่ได้ (สิทธิ์ไม่พอ หรือไฟล์ถูกล็อก)")),
];

/// The samples that are words, not symbols. The list draws the same words, so a sample reads as the
/// list does.
const INVISIBLE_WARN: &str = tr_key("⚠ invisible");
const INVISIBLE_INFO: &str = tr_key("invisible");
const FILE_COUNT: &str = tr_key("3 files");
const NEWER: &str = tr_key("▲ date");
const OTHER_TIME: &str = tr_key("other time");
const HATCHED: &str = tr_key("hatched");

/// The sample of one legend row, drawn as the list draws that symbol. `glyph` is the row's English
/// text, which names the row in both languages.
pub fn sample(glyph: &'static str, p: &Palette) -> AnyElement {
    let text = |color| {
        div()
            .text_color(color)
            .font_weight(FontWeight::BOLD)
            .child(glyph)
    };
    match glyph {
        "≠" => text(p.modified).into_any_element(),
        "≈" => text(p.unimp).into_any_element(),
        "⚠" | "!" | "◀" => text(p.del).into_any_element(),
        "▶" => text(p.ins).into_any_element(),
        "=" => text(p.dim).into_any_element(),
        INVISIBLE_WARN => ld_pill(tr(glyph), p.del, p.bg_del).into_any_element(),
        INVISIBLE_INFO => ld_pill(tr(glyph), p.muted, p.panel3).into_any_element(),
        FILE_COUNT => ld_pill(tr(glyph), p.modified, p.bg_mod).into_any_element(),
        NEWER => div()
            .text_color(p.accent)
            .font_weight(FontWeight::BOLD)
            .child(tr(glyph))
            .into_any_element(),
        OTHER_TIME => div()
            .italic()
            .text_color(p.moved)
            .child(tr(glyph))
            .into_any_element(),
        HATCHED => div()
            .text_color(p.muted)
            .child(tr(glyph))
            .into_any_element(),
        _ => div().text_color(p.muted).child(glyph).into_any_element(),
    }
}

/// The panel under the chip row: one row per `LEGEND` entry. It floats above the list, so
/// opening it moves nothing.
pub(super) fn legend_panel(p: &Palette) -> impl IntoElement {
    deferred(
        v_flex()
            .id("folder-legend-panel")
            .test_support()
            .occlude()
            .absolute()
            .top(px(28.))
            .left_0()
            .w(px(560.))
            .p(px(10.))
            .gap(px(6.))
            .bg(p.panel3)
            .border_1()
            .border_color(p.line)
            .rounded(px(7.))
            .text_size(px(12.))
            .text_color(p.text)
            .children(
                topics::legend_rows(HelpLang::current())
                    .into_iter()
                    .enumerate()
                    .map(|(i, (glyph, meaning))| {
                        h_flex()
                            .id(("folder-legend-row", i))
                            .test_support()
                            .gap(px(10.))
                            .items_start()
                            .child(div().flex_none().w(px(92.)).child(sample(glyph, p)))
                            .child(div().flex_1().min_w_0().text_color(p.muted).prose(meaning))
                    }),
            ),
    )
    .with_priority(1)
}
