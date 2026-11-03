//! The help's topics as data (H4): every topic in Thai and in English, made of headings,
//! paragraphs, bullet lists, the shortcut sheet and the folder legend, each with its icon and
//! its group in the topic list (U10). The page draws each text as pieces (H6) with chips and
//! keys (U8, `markup`), so the data keeps every piece short; `tests` holds it to that.

use std::path::Path;

use gpui_kit::assets::IconName;

use super::{en, markup, th};
use crate::folder::view::legend::{LEGEND, LEGEND_TH};
use crate::start::{SHORTCUTS, SHORTCUTS_TH};
use crate::ui::icons;

/// The help's language (H3). With no saved choice the page opens in Thai.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelpLang {
    #[default]
    Th,
    En,
}

impl HelpLang {
    /// The `settings.json` key that keeps the choice.
    pub const KEY: &str = "help.lang";

    pub fn code(self) -> &'static str {
        match self {
            HelpLang::Th => "th",
            HelpLang::En => "en",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "th" => Some(HelpLang::Th),
            "en" => Some(HelpLang::En),
            _ => None,
        }
    }

    /// The saved choice; no settings file, no key or an unknown value gives Thai.
    pub fn load(settings: Option<&Path>) -> Self {
        settings
            .and_then(|path| crate::settings::get_string(path, Self::KEY))
            .and_then(|code| Self::from_code(&code))
            .unwrap_or_default()
    }

    pub fn save(self, settings: &Path) -> std::io::Result<()> {
        crate::settings::set_string(settings, Self::KEY, self.code())
    }
}

/// One part of a topic's body.
#[derive(Debug, Clone, Copy)]
pub enum Block {
    Heading(&'static str),
    Para(&'static str),
    Bullets(&'static [&'static str]),
    /// The Start tab's shortcut sheet, with its keys read from the real bindings.
    Shortcuts,
    /// The folder legend's glyphs and what they mean.
    Legend,
}

/// A topic in one language.
#[derive(Debug, Clone, Copy)]
pub struct TopicText {
    pub title: &'static str,
    /// Search words that are not in the text; a Thai topic also carries the English ones.
    pub keywords: &'static str,
    pub body: &'static [Block],
}

#[derive(Debug, Clone, Copy)]
pub struct Topic {
    pub id: &'static str,
    /// Its icon in the topic list: content, not chrome, so it is not in `ui::icons`.
    pub icon: IconName,
    pub th: TopicText,
    pub en: TopicText,
}

impl Topic {
    pub fn text(&self, lang: HelpLang) -> &TopicText {
        match lang {
            HelpLang::Th => &self.th,
            HelpLang::En => &self.en,
        }
    }
}

/// The fifteen topics in reading order (H5).
pub const TOPICS: [Topic; 15] = [
    Topic {
        id: "start",
        icon: IconName::Rocket,
        th: th::START,
        en: en::START,
    },
    Topic {
        id: "folder",
        icon: IconName::FolderTree,
        th: th::FOLDER,
        en: en::FOLDER,
    },
    Topic {
        id: "legend",
        icon: IconName::Shapes,
        th: th::LEGEND_TOPIC,
        en: en::LEGEND_TOPIC,
    },
    Topic {
        id: "eol",
        icon: IconName::CornerDownLeft,
        th: th::EOL,
        en: en::EOL,
    },
    Topic {
        id: "odd",
        icon: IconName::EyeOff,
        th: th::ODD,
        en: en::ODD,
    },
    Topic {
        id: "ops",
        icon: IconName::FolderSync,
        th: th::OPS,
        en: en::OPS,
    },
    Topic {
        id: "report",
        icon: IconName::FileChartColumn,
        th: th::REPORT,
        en: en::REPORT,
    },
    Topic {
        id: "text",
        icon: IconName::FileDiff,
        th: th::TEXT,
        en: en::TEXT,
    },
    Topic {
        id: "rules",
        icon: IconName::Funnel,
        th: th::RULES,
        en: en::RULES,
    },
    Topic {
        id: "merge",
        icon: IconName::GitMerge,
        th: th::MERGE,
        en: en::MERGE,
    },
    Topic {
        id: "hex",
        icon: IconName::Binary,
        th: th::HEX,
        en: en::HEX,
    },
    Topic {
        id: "picture",
        icon: IconName::Image,
        th: th::PICTURE,
        en: en::PICTURE,
    },
    Topic {
        id: "table",
        icon: IconName::Sheet,
        th: th::TABLE,
        en: en::TABLE,
    },
    Topic {
        id: "keys",
        icon: IconName::Keyboard,
        th: th::KEYS,
        en: en::KEYS,
    },
    Topic {
        id: "faq",
        icon: IconName::CircleQuestionMark,
        th: th::FAQ,
        en: en::FAQ,
    },
];

/// A titled run of the topic list (U10). Its label is not a control.
#[derive(Debug, Clone, Copy)]
pub struct Group {
    pub th: &'static str,
    pub en: &'static str,
    /// Its topics' ids, in `TOPICS` order.
    pub topics: &'static [&'static str],
}

impl Group {
    pub fn label(&self, lang: HelpLang) -> &'static str {
        match lang {
            HelpLang::Th => self.th,
            HelpLang::En => self.en,
        }
    }
}

/// The topic list's four groups; together they hold every topic once, in `TOPICS` order.
pub const GROUPS: [Group; 4] = [
    Group {
        th: "พื้นฐาน",
        en: "Basics",
        topics: &["start"],
    },
    Group {
        th: "โฟลเดอร์",
        en: "Folders",
        topics: &["folder", "legend", "eol", "odd", "ops", "report"],
    },
    Group {
        th: "ไฟล์",
        en: "Files",
        topics: &["text", "rules", "merge", "hex", "picture", "table"],
    },
    Group {
        th: "อ้างอิง",
        en: "Reference",
        topics: &["keys", "faq"],
    },
];

/// The group that holds `TOPICS[index]`.
pub fn group_of(index: usize) -> usize {
    let id = TOPICS[index].id;
    GROUPS
        .iter()
        .position(|group| group.topics.contains(&id))
        .unwrap_or(GROUPS.len() - 1)
}

/// The icon a chip draws before its name, for the icon-only controls the guide names by their
/// tooltip (U8), so the reader can find them on screen. The theme button shows the theme a
/// click gives, as the header's does.
pub fn chip_icon(label: &str, dark: bool) -> Option<IconName> {
    Some(match label {
        "New compare" => icons::NEW_TAB,
        "Close tab" | "Remove from the list" => icons::CLOSE,
        "Swap sides" => icons::SWAP,
        "Up one folder on both sides" => icons::UP,
        "Zoom in" => icons::ZOOM_IN,
        "Zoom out" => icons::ZOOM_OUT,
        "Switch between light and dark" if dark => icons::TO_LIGHT,
        "Switch between light and dark" => icons::TO_DARK,
        _ => return None,
    })
}

/// The page's own words in one language (H3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageCopy {
    /// The search box's placeholder.
    pub search: &'static str,
    /// The search box's accessible name.
    pub search_name: &'static str,
    pub empty: &'static str,
    pub prev: &'static str,
    pub next: &'static str,
}

pub fn page_copy(lang: HelpLang) -> PageCopy {
    match lang {
        HelpLang::Th => th::PAGE,
        HelpLang::En => en::PAGE,
    }
}

/// The Start tab's welcome card in one language (U15): the help's language, so the card is
/// drawn as the help draws its prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WelcomeCopy {
    pub title: &'static str,
    pub lines: [&'static str; 3],
    /// The button that opens the Help tab.
    pub open: &'static str,
    pub dismiss: &'static str,
}

pub fn welcome_copy(lang: HelpLang) -> WelcomeCopy {
    match lang {
        HelpLang::Th => th::WELCOME,
        HelpLang::En => en::WELCOME,
    }
}

/// What each row of the shortcut sheet does, in `lang`, in the sheet's order.
pub fn shortcut_labels(lang: HelpLang) -> Vec<&'static str> {
    match lang {
        HelpLang::Th => SHORTCUTS_TH.iter().map(|(_, thai)| *thai).collect(),
        HelpLang::En => SHORTCUTS.iter().map(|(_, label)| *label).collect(),
    }
}

/// The folder legend's rows in `lang`: the glyph as the list draws it, and its meaning.
pub fn legend_rows(lang: HelpLang) -> Vec<(&'static str, &'static str)> {
    match lang {
        HelpLang::Th => LEGEND
            .iter()
            .zip(LEGEND_TH.iter())
            .map(|((glyph, _), (_, thai))| (*glyph, *thai))
            .collect(),
        HelpLang::En => LEGEND.to_vec(),
    }
}

/// The sheet's keys, "F7 / F8", from the real bindings.
pub fn sheet_keys() -> Vec<String> {
    crate::start::shortcut_rows(&crate::app::key_bindings())
        .into_iter()
        .map(|(keys, _)| keys)
        .collect()
}

/// The text a search looks in (H7): the title, the keywords and every block's text as the
/// reader sees it (chips and keys without their brackets), the sheet's keys included,
/// lower-cased.
pub fn topic_text(topic: &Topic, lang: HelpLang, keys: &[String]) -> String {
    let text = topic.text(lang);
    let mut out = vec![text.title.to_owned(), text.keywords.to_owned()];
    for block in text.body {
        match block {
            Block::Heading(t) | Block::Para(t) => out.push(markup::visible(t)),
            Block::Bullets(items) => out.extend(items.iter().map(|t| markup::visible(t))),
            Block::Shortcuts => {
                for (keys, label) in keys.iter().zip(shortcut_labels(lang)) {
                    out.push(format!("{keys} {label}"));
                }
            }
            Block::Legend => {
                for (glyph, meaning) in legend_rows(lang) {
                    out.push(format!("{glyph} {meaning}"));
                }
            }
        }
    }
    out.join("\n").to_lowercase()
}

/// Every topic's search text in `lang`, in `TOPICS` order.
pub fn search_texts(lang: HelpLang, keys: &[String]) -> Vec<String> {
    TOPICS
        .iter()
        .map(|topic| topic_text(topic, lang, keys))
        .collect()
}

/// The topics whose text holds the trimmed, lower-cased query (H7); an empty query gives
/// every topic.
pub fn matching(query: &str, texts: &[String]) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    texts
        .iter()
        .enumerate()
        .filter(|(_, text)| text.contains(&query))
        .map(|(i, _)| i)
        .collect()
}

/// A text as the page draws it (H6): split after each space outside a chip or a key, so every
/// piece is a word, a Thai phrase, a chip or a key with its trailing space. Each piece is one
/// no-wrap element, and lines break only between pieces.
pub fn pieces(text: &'static str) -> Vec<&'static str> {
    markup::pieces(text)
}

/// Every piece of a topic's page in drawing order: the title, then each block's texts (the
/// sheet's and the legend's descriptions included). The page numbers its pieces in this order.
pub fn topic_pieces(topic: &Topic, lang: HelpLang) -> Vec<&'static str> {
    let text = topic.text(lang);
    let mut out = pieces(text.title);
    for block in text.body {
        match block {
            Block::Heading(t) | Block::Para(t) => out.extend(pieces(t)),
            Block::Bullets(items) => out.extend(items.iter().flat_map(|t| pieces(t))),
            Block::Shortcuts => out.extend(shortcut_labels(lang).into_iter().flat_map(pieces)),
            Block::Legend => {
                out.extend(legend_rows(lang).into_iter().flat_map(|(_, m)| pieces(m)));
            }
        }
    }
    out
}

/// The longest piece the page draws, in characters the reader sees (H6).
pub const MAX_PIECE_CHARS: usize = 40;

/// Thai vowels and tone marks that sit on a consonant: a piece must never start with one.
pub fn is_thai_mark(c: char) -> bool {
    matches!(c, '\u{0E31}' | '\u{0E34}'..='\u{0E3A}' | '\u{0E47}'..='\u{0E4E}')
}

#[cfg(test)]
mod tests {
    use super::*;

    const TITLES: [(&str, &str, &str); 15] = [
        ("start", "Getting started", "เริ่มต้นใช้งาน"),
        ("folder", "Comparing folders", "เทียบโฟลเดอร์"),
        ("legend", "Symbols and colours", "สัญลักษณ์และสี"),
        ("eol", "Line endings and BOM only", "ต่างแค่ท้ายบรรทัดหรือ BOM"),
        ("odd", "Invisible characters", "อักขระที่มองไม่เห็น"),
        ("ops", "Copy, delete and sync", "คัดลอก ลบ และซิงก์"),
        ("report", "Folder reports", "รายงานผลเทียบโฟลเดอร์"),
        ("text", "Comparing and editing text", "เทียบและแก้ไขข้อความ"),
        (
            "rules",
            "Ignoring unimportant differences",
            "ข้ามความต่างที่ไม่สำคัญ",
        ),
        ("merge", "3-way merge", "รวมสามทาง (3-way merge)"),
        ("hex", "Hex", "เทียบแบบ Hex"),
        ("picture", "Pictures", "เทียบรูปภาพ"),
        ("table", "Tables (CSV, TSV)", "เทียบตาราง (CSV, TSV)"),
        ("keys", "Keyboard shortcuts", "คีย์ลัด"),
        ("faq", "Questions", "คำถามที่พบบ่อย"),
    ];

    fn topic(id: &str) -> &'static Topic {
        TOPICS.iter().find(|t| t.id == id).unwrap()
    }

    /// Every text the page draws as pieces in `lang`, named for the failure message.
    fn drawn_texts(lang: HelpLang) -> Vec<(String, &'static str)> {
        let mut out = Vec::new();
        for topic in &TOPICS {
            let text = topic.text(lang);
            out.push((format!("{} title", topic.id), text.title));
            for (b, block) in text.body.iter().enumerate() {
                match block {
                    Block::Heading(t) | Block::Para(t) => {
                        out.push((format!("{} block {b}", topic.id), *t))
                    }
                    Block::Bullets(items) => out.extend(
                        items
                            .iter()
                            .enumerate()
                            .map(|(i, t)| (format!("{} block {b} item {i}", topic.id), *t)),
                    ),
                    Block::Shortcuts | Block::Legend => {}
                }
            }
        }
        for (i, label) in shortcut_labels(lang).into_iter().enumerate() {
            out.push((format!("sheet row {i}"), label));
        }
        for (i, (_, meaning)) in legend_rows(lang).into_iter().enumerate() {
            out.push((format!("legend row {i}"), meaning));
        }
        let copy = page_copy(lang);
        for (name, t) in [
            ("empty", copy.empty),
            ("prev", copy.prev),
            ("next", copy.next),
        ] {
            out.push((name.to_owned(), t));
        }
        // The Start tab's welcome card is drawn as pieces too (U15).
        let welcome = welcome_copy(lang);
        out.push(("welcome title".to_owned(), welcome.title));
        for (i, t) in welcome.lines.into_iter().enumerate() {
            out.push((format!("welcome line {i}"), t));
        }
        out
    }

    #[test]
    fn the_welcome_card_reads_as_in_the_brief_in_both_languages() {
        let (th, en) = (welcome_copy(HelpLang::Th), welcome_copy(HelpLang::En));
        assert_eq!(th.title, "ยินดีต้อนรับสู่ duet");
        assert_eq!(en.title, "Welcome to duet");
        assert_eq!(
            th.lines.map(markup::visible),
            [
                "ลากไฟล์หรือโฟลเดอร์สองรายการมาวาง เพื่อเทียบทันที",
                "หรือเลือกชนิดการเทียบจากการ์ดด้านล่าง",
                "กด F1 เปิดคู่มือ (ภาษาไทยและอังกฤษ)",
            ]
        );
        assert_eq!(
            en.lines.map(markup::visible),
            [
                "Drop two files or folders to compare them at once",
                "or pick a compare type below",
                "Press F1 for the guide (Thai and English)",
            ]
        );
        assert_eq!((th.open, th.dismiss), ("เปิดคู่มือ", "ไม่ต้องแสดงอีก"));
        assert_eq!(
            (en.open, en.dismiss),
            ("Open the guide", "Don't show again")
        );
        let keys = |copy: WelcomeCopy| -> Vec<String> {
            copy.lines
                .iter()
                .flat_map(|t| markup::marks(t))
                .map(|span| span.text().to_owned())
                .collect()
        };
        assert_eq!(keys(th), ["F1"], "F1 is drawn as a key cap");
        assert_eq!(keys(th), keys(en));
    }

    #[test]
    fn the_topics_are_the_fifteen_of_the_brief_in_its_order() {
        let got: Vec<(&str, &str, &str)> = TOPICS
            .iter()
            .map(|t| (t.id, t.en.title, t.th.title))
            .collect();
        assert_eq!(got, TITLES);
        for topic in &TOPICS {
            for lang in [HelpLang::Th, HelpLang::En] {
                assert!(
                    !topic.text(lang).body.is_empty(),
                    "{} has a body in {lang:?}",
                    topic.id
                );
                assert!(!topic.text(lang).keywords.trim().is_empty());
            }
        }
        assert!(matches!(topic("keys").en.body, [.., Block::Shortcuts]));
        assert!(matches!(topic("keys").th.body, [.., Block::Shortcuts]));
        for lang in [HelpLang::Th, HelpLang::En] {
            assert!(
                topic("legend")
                    .text(lang)
                    .body
                    .iter()
                    .any(|b| matches!(b, Block::Legend))
            );
        }
    }

    #[test]
    fn both_languages_have_the_same_shape() {
        // The Thai and English versions say the same things: block for block, item for item.
        for topic in &TOPICS {
            let shape = |text: &TopicText| -> Vec<(u8, usize)> {
                text.body
                    .iter()
                    .map(|b| match b {
                        Block::Heading(_) => (0, 1),
                        Block::Para(_) => (1, 1),
                        Block::Bullets(items) => (2, items.len()),
                        Block::Shortcuts => (3, 1),
                        Block::Legend => (4, 1),
                    })
                    .collect()
            };
            assert_eq!(shape(&topic.th), shape(&topic.en), "{}", topic.id);
        }
    }

    /// The texts of a topic's headings, paragraphs and bullets: where chips and keys are written.
    fn body_texts(text: &TopicText) -> Vec<&'static str> {
        let mut out = Vec::new();
        for block in text.body {
            match block {
                Block::Heading(t) | Block::Para(t) => out.push(*t),
                Block::Bullets(items) => out.extend(items.iter().copied()),
                Block::Shortcuts | Block::Legend => {}
            }
        }
        out
    }

    #[test]
    fn every_drawn_text_splits_into_short_pieces_that_never_start_with_a_thai_mark() {
        for lang in [HelpLang::Th, HelpLang::En] {
            for (name, text) in drawn_texts(lang) {
                assert!(!text.is_empty(), "{lang:?} {name} is empty");
                assert!(
                    !text.contains("  ") && !text.starts_with(' ') && !text.ends_with(' '),
                    "{lang:?} {name}: a doubled or edge space makes an empty piece: {text:?}"
                );
                for piece in pieces(text) {
                    // A chip or a key counts by the text the reader sees (U8).
                    let seen = markup::visible(piece);
                    let chars = seen.chars().count();
                    assert!(
                        chars <= MAX_PIECE_CHARS,
                        "{lang:?} {name}: {chars} characters in {piece:?}"
                    );
                    let first = seen.chars().next().unwrap();
                    assert!(
                        !is_thai_mark(first),
                        "{lang:?} {name}: {piece:?} starts with a Thai mark"
                    );
                }
            }
        }
        assert_eq!(pieces("ab cd  "), ["ab ", "cd ", " "]);
        assert_eq!(pieces("เทียบ โฟลเดอร์"), ["เทียบ ", "โฟลเดอร์"]);
        assert!(is_thai_mark('\u{0E31}') && is_thai_mark('\u{0E48}') && is_thai_mark('\u{0E4E}'));
        assert!(!is_thai_mark('ก') && !is_thai_mark('เ') && !is_thai_mark('\u{0E33}'));
    }

    #[test]
    fn every_text_has_balanced_markup_and_known_keys() {
        for lang in [HelpLang::Th, HelpLang::En] {
            for (name, text) in drawn_texts(lang) {
                let spans = markup::parse(text)
                    .unwrap_or_else(|err| panic!("{lang:?} {name}: {err} in {text:?}"));
                for span in spans {
                    match span {
                        markup::Span::Key(keys) => assert!(
                            markup::keys_are_known(keys),
                            "{lang:?} {name}: {{{keys}}} names an unknown key"
                        ),
                        markup::Span::Chip(label) => assert!(
                            label.trim() == label,
                            "{lang:?} {name}: [{label}] has an edge space"
                        ),
                        markup::Span::Text(_) => {}
                    }
                }
            }
        }
    }

    /// Text in quotes that is not a control: a message, a badge, a column, a word, a value.
    /// Topic titles, quoted to point at another topic, are allowed too.
    const QUOTED: [&str; 8] = [
        "/bin/bash^M: bad interpreter",
        "3 files",
        "⚠ invisible",
        "invisible",
        "other time",
        "Result",
        "left",
        "ซ้าย",
    ];

    #[test]
    fn quotes_are_left_only_around_literal_text() {
        // A control is a [chip] now (U8); quotes stay around literal text only.
        let titles: Vec<&str> = TOPICS
            .iter()
            .flat_map(|t| [t.th.title, t.en.title])
            .collect();
        for lang in [HelpLang::Th, HelpLang::En] {
            for (name, text) in drawn_texts(lang) {
                let parts: Vec<&str> = text.split('"').collect();
                assert!(parts.len() % 2 == 1, "{lang:?} {name}: an odd quote");
                for quoted in parts.iter().skip(1).step_by(2) {
                    assert!(
                        QUOTED.contains(quoted) || titles.contains(quoted),
                        "{lang:?} {name}: \"{quoted}\" names a control: write it as [{quoted}]"
                    );
                }
            }
        }
    }

    /// The string literals of Rust `code` before its tests, unescaped as far as labels need.
    /// Comments, character literals and lifetimes are skipped.
    fn string_literals(code: &str) -> Vec<String> {
        let code = code.split("#[cfg(test)]").next().unwrap_or_default();
        let chars: Vec<char> = code.chars().collect();
        let at = |i: usize| chars.get(i).copied();
        let mut out = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '/' if at(i + 1) == Some('/') => {
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                }
                '/' if at(i + 1) == Some('*') => {
                    i += 2;
                    while i < chars.len() && !(chars[i] == '*' && at(i + 1) == Some('/')) {
                        i += 1;
                    }
                    i += 2;
                }
                '\'' if at(i + 1) == Some('\\') => {
                    // '\n', '\'', '\u{2026}'
                    i += 3;
                    while i < chars.len() && chars[i] != '\'' {
                        i += 1;
                    }
                    i += 1;
                }
                // 'x' or '"'; otherwise a lifetime.
                '\'' if at(i + 2) == Some('\'') => i += 3,
                'r' if matches!(at(i + 1), Some('"' | '#'))
                    && !(i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_')) =>
                {
                    let mut j = i + 1;
                    while at(j) == Some('#') {
                        j += 1;
                    }
                    let hashes = j - i - 1;
                    if at(j) != Some('"') {
                        i += 1;
                        continue;
                    }
                    let start = j + 1;
                    let mut end = start;
                    while end < chars.len()
                        && !(chars[end] == '"' && (1..=hashes).all(|h| at(end + h) == Some('#')))
                    {
                        end += 1;
                    }
                    out.push(chars[start..end.min(chars.len())].iter().collect());
                    i = end + 1 + hashes;
                }
                '"' => {
                    let mut text = String::new();
                    let mut j = i + 1;
                    while j < chars.len() && chars[j] != '"' {
                        if chars[j] != '\\' {
                            text.push(chars[j]);
                            j += 1;
                            continue;
                        }
                        match at(j + 1) {
                            Some('u') if at(j + 2) == Some('{') => {
                                let close = (j..chars.len()).find(|&k| chars[k] == '}');
                                let code = close
                                    .map(|close| chars[j + 3..close].iter().collect::<String>())
                                    .and_then(|hex| u32::from_str_radix(&hex, 16).ok())
                                    .and_then(char::from_u32);
                                text.extend(code);
                                j = close.map_or(j + 2, |close| close + 1);
                            }
                            Some('\n' | '\r') => {
                                j += 1;
                                while matches!(at(j), Some(' ' | '\t' | '\n' | '\r')) {
                                    j += 1;
                                }
                            }
                            Some('n') => {
                                text.push('\n');
                                j += 2;
                            }
                            Some(c) => {
                                text.push(c);
                                j += 2;
                            }
                            None => j += 1,
                        }
                    }
                    out.push(text);
                    i = j + 1;
                }
                _ => i += 1,
            }
        }
        out
    }

    /// Whether `label` is a text the format string `format` can write: its fixed parts, which
    /// must hold a letter, in order, and something for each `{…}`.
    fn fits_format(format: &str, label: &str) -> bool {
        let mut fixed = Vec::new();
        let mut rest = format;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else {
                return false;
            };
            fixed.push(&rest[..open]);
            rest = &rest[open + close + 1..];
        }
        fixed.push(rest);
        if fixed.len() < 2 || !fixed.iter().any(|f| f.chars().any(char::is_alphabetic)) {
            return false;
        }
        let (first, last) = (fixed[0], fixed[fixed.len() - 1]);
        let Some(mut body) = label
            .strip_prefix(first)
            .and_then(|body| body.strip_suffix(last))
        else {
            return false;
        };
        for part in &fixed[1..fixed.len() - 1] {
            // Each hole takes at least one character.
            let found = body
                .char_indices()
                .skip(1)
                .map(|(i, _)| i)
                .find(|&i| body[i..].starts_with(part));
            let Some(at) = found else {
                return false;
            };
            body = &body[at + part.len()..];
        }
        !body.is_empty()
    }

    /// Whether some literal shows `label`: the literal itself, a tooltip that adds its key in
    /// brackets ("Swap sides (F9)"), or a format string that writes it ("Tab {chosen}").
    fn names_a_control(label: &str, literals: &[String]) -> bool {
        literals.iter().any(|literal| {
            literal == label
                || literal
                    .strip_prefix(label)
                    .is_some_and(|rest| rest.starts_with(" (") && rest.ends_with(')'))
                || (literal.contains('{') && fits_format(literal, label))
        })
    }

    /// Chips that name a control by fewer words than it shows, and the whole label.
    const SHORTENED: [(&str, &str, &str); 1] = [(
        "Hide values that look like passwords…",
        "Hide values that look like passwords, keys or tokens (recommended before sharing)",
        "the export panel's masking toggle: its whole label is longer than a piece (H6)",
    )];

    /// Every string literal of the app's and the core's code (where the merge choices are
    /// named), outside the help itself.
    fn source_literals() -> Vec<String> {
        fn visit(dir: &Path, skip: &[std::path::PathBuf], out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if skip.contains(&path) {
                    continue;
                }
                if path.is_dir() {
                    visit(&path, skip, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.extend(string_literals(&std::fs::read_to_string(&path).unwrap()));
                }
            }
        }
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let app = crates.join("duet-app").join("src");
        let skip = [app.join("help"), app.join("help.rs")];
        let mut out = Vec::new();
        visit(&app, &skip, &mut out);
        visit(&crates.join("duet-core").join("src"), &skip, &mut out);
        out
    }

    #[test]
    fn the_literal_scanner_reads_rust_strings() {
        let code = "let a = \"Swap sides (F9)\"; // \"not this\"\n\
                    let b = '\"'; let c = r#\"raw \"x\"\"#; fn f<'a>() {}\n\
                    let d = \"Use the left text (Alt+\\u{2190})\"; /* \"nor this\" */\n\
                    #[cfg(test)] let e = \"a test\";";
        assert_eq!(
            string_literals(code),
            ["Swap sides (F9)", "raw \"x\"", "Use the left text (Alt+←)"]
        );
        let literals = string_literals(code);
        assert!(names_a_control("Swap sides", &literals));
        assert!(!names_a_control("Swap", &literals));
        assert!(fits_format("Tab {chosen}", "Tab 4"));
        assert!(fits_format("Open {side}…", "Open left…"));
        assert!(!fits_format("Tab {chosen}", "Tab "));
        assert!(
            !fits_format("{side}: {}", "Left: UTF-8"),
            "no letter to match"
        );
        assert!(!fits_format("Tab {chosen}", "Font 4"));
    }

    #[test]
    fn every_chip_names_a_control_duet_shows() {
        let literals = source_literals();
        assert!(literals.len() > 1000, "the sources were read");
        for (short, whole, _why) in SHORTENED {
            assert!(names_a_control(whole, &literals), "{whole}");
            assert!(whole.starts_with(short.trim_end_matches('…')));
        }
        for lang in [HelpLang::Th, HelpLang::En] {
            for (name, text) in drawn_texts(lang) {
                for span in markup::marks(text) {
                    let markup::Span::Chip(label) = span else {
                        continue;
                    };
                    assert!(
                        names_a_control(label, &literals)
                            || SHORTENED.iter().any(|(short, ..)| *short == label),
                        "{lang:?} {name}: no control shows [{label}]"
                    );
                }
            }
        }
    }

    #[test]
    fn the_guide_names_the_start_page_as_it_is() {
        // U12: the "Compare text" card became the form's "Paste text…" button. The chip test
        // above accepts [Compare text] through the merge's "{} text" format string, so this
        // pins the change itself.
        let chips = |text: &TopicText| -> Vec<&'static str> {
            body_texts(text)
                .into_iter()
                .flat_map(markup::marks)
                .filter_map(|span| match span {
                    markup::Span::Chip(label) => Some(label),
                    _ => None,
                })
                .collect()
        };
        for topic in &TOPICS {
            for lang in [HelpLang::Th, HelpLang::En] {
                assert!(
                    !chips(topic.text(lang)).contains(&"Compare text"),
                    "{} in {lang:?} names the old card",
                    topic.id
                );
            }
        }
        for lang in [HelpLang::Th, HelpLang::En] {
            let start = chips(topic("start").text(lang));
            for label in [
                "Paste text…",
                "Compare files",
                "Compare folders",
                "3-way merge",
                "Compare binary files",
                "Compare tables",
                "Compare pictures",
            ] {
                assert!(
                    start.contains(&label),
                    "{lang:?}: the start topic names [{label}]"
                );
            }
            assert!(chips(topic("text").text(lang)).contains(&"Paste text…"));
        }
    }

    #[test]
    fn both_languages_name_the_same_controls_and_keys() {
        // The controller's guard against the two languages drifting apart (U8).
        for topic in &TOPICS {
            let marks = |text: &TopicText| -> Vec<String> {
                let mut out: Vec<String> = body_texts(text)
                    .into_iter()
                    .flat_map(markup::marks)
                    .map(|span| match span {
                        markup::Span::Chip(label) => format!("[{label}]"),
                        markup::Span::Key(keys) => format!("{{{keys}}}"),
                        markup::Span::Text(t) => t.to_owned(),
                    })
                    .collect();
                out.sort();
                out
            };
            assert_eq!(marks(&topic.th), marks(&topic.en), "{}", topic.id);
        }
    }

    #[test]
    fn the_groups_hold_every_topic_once_in_order() {
        let grouped: Vec<&str> = GROUPS
            .iter()
            .flat_map(|g| g.topics.iter().copied())
            .collect();
        let ids: Vec<&str> = TOPICS.iter().map(|t| t.id).collect();
        assert_eq!(grouped, ids);
        assert_eq!(
            GROUPS.map(|g| (g.th, g.en)),
            [
                ("พื้นฐาน", "Basics"),
                ("โฟลเดอร์", "Folders"),
                ("ไฟล์", "Files"),
                ("อ้างอิง", "Reference")
            ]
        );
        assert_eq!(group_of(0), 0);
        assert_eq!(group_of(TOPICS.len() - 1), 3);
        let at = |id: &str| TOPICS.iter().position(|t| t.id == id).unwrap();
        assert_eq!(group_of(at("report")), 1);
        assert_eq!(group_of(at("text")), 2);
    }

    #[test]
    fn every_topic_has_its_icon() {
        let icons: Vec<(&str, IconName)> = TOPICS.iter().map(|t| (t.id, t.icon)).collect();
        assert_eq!(
            icons,
            [
                ("start", IconName::Rocket),
                ("folder", IconName::FolderTree),
                ("legend", IconName::Shapes),
                ("eol", IconName::CornerDownLeft),
                ("odd", IconName::EyeOff),
                ("ops", IconName::FolderSync),
                ("report", IconName::FileChartColumn),
                ("text", IconName::FileDiff),
                ("rules", IconName::Funnel),
                ("merge", IconName::GitMerge),
                ("hex", IconName::Binary),
                ("picture", IconName::Image),
                ("table", IconName::Sheet),
                ("keys", IconName::Keyboard),
                ("faq", IconName::CircleQuestionMark),
            ]
        );
    }

    #[test]
    fn chip_icons_are_for_chips_the_guide_uses() {
        let chips: Vec<&str> = [HelpLang::Th, HelpLang::En]
            .into_iter()
            .flat_map(drawn_texts)
            .flat_map(|(_, text)| markup::marks(text))
            .filter_map(|span| match span {
                markup::Span::Chip(label) => Some(label),
                _ => None,
            })
            .collect();
        for label in [
            "New compare",
            "Close tab",
            "Remove from the list",
            "Swap sides",
            "Up one folder on both sides",
            "Zoom in",
            "Zoom out",
            "Switch between light and dark",
        ] {
            assert!(chip_icon(label, false).is_some(), "{label}");
            assert!(chips.contains(&label), "the guide names [{label}]");
        }
        assert_eq!(
            chip_icon("Compare", true),
            None,
            "a labelled control: words only"
        );
        assert_eq!(
            chip_icon("Switch between light and dark", true),
            Some(icons::TO_LIGHT)
        );
        assert_eq!(
            chip_icon("Switch between light and dark", false),
            Some(icons::TO_DARK)
        );
    }

    #[test]
    fn the_thai_tables_follow_the_sheet_and_the_legend() {
        assert_eq!(SHORTCUTS_TH.len(), SHORTCUTS.len());
        for ((_, english), (key, thai)) in SHORTCUTS.iter().zip(SHORTCUTS_TH.iter()) {
            assert_eq!(
                english, key,
                "the Thai sheet row for {english:?} is in its place"
            );
            assert!(!thai.is_empty() && thai != english);
        }
        assert_eq!(LEGEND_TH.len(), LEGEND.len());
        for ((glyph, english), (key, thai)) in LEGEND.iter().zip(LEGEND_TH.iter()) {
            assert_eq!(
                glyph, key,
                "the Thai legend row for {glyph:?} is in its place"
            );
            assert!(!thai.is_empty() && thai != english);
        }
        assert_eq!(legend_rows(HelpLang::Th)[0], ("≠", LEGEND_TH[0].1));
        assert_eq!(shortcut_labels(HelpLang::En).len(), SHORTCUTS.len());
    }

    #[test]
    fn search_finds_titles_keywords_and_body_text_in_both_languages() {
        let keys = sheet_keys();
        let (th, en) = (
            search_texts(HelpLang::Th, &keys),
            search_texts(HelpLang::En, &keys),
        );
        let at = |id: &str| TOPICS.iter().position(|t| t.id == id).unwrap();
        assert_eq!(matching("", &th), (0..15).collect::<Vec<_>>());
        assert_eq!(matching("   ", &en).len(), 15);
        assert!(matching("ZERO-WIDTH", &en).contains(&at("odd")));
        assert!(matching("  Mirror ", &en).contains(&at("ops")));
        assert!(matching("ซิงก์", &th).contains(&at("ops")));
        assert!(matching("no such words xyz", &en).is_empty());
        assert!(matching("no such words xyz", &th).is_empty());
        assert!(matching("ซิงก์", &en).is_empty());
        // The sheet and the legend are searched in the page's language.
        assert!(th[at("keys")].contains("ไปบรรทัด"));
        assert!(en[at("keys")].contains("go to line"));
        assert!(
            en[at("keys")].contains("f1"),
            "the keys themselves: {}",
            en[at("keys")]
        );
        assert!(th[at("legend")].contains(&LEGEND_TH[0].1.to_lowercase()));
        assert!(en[at("legend")].contains(&LEGEND[0].1.to_lowercase()));
    }

    #[test]
    fn every_thai_topic_carries_the_english_search_words() {
        // An English word finds the topic in Thai mode too (H5).
        let keys = sheet_keys();
        for topic in &TOPICS {
            let thai = topic_text(topic, HelpLang::Th, &keys);
            for word in topic.en.keywords.split_whitespace() {
                assert!(
                    thai.contains(&word.to_lowercase()),
                    "{}: the Thai text lacks {word:?}",
                    topic.id
                );
            }
        }
    }

    #[test]
    fn topic_pieces_follow_the_title_and_the_blocks() {
        let keys = topic("keys");
        let pieces_th = topic_pieces(keys, HelpLang::Th);
        assert_eq!(pieces_th[0], pieces(keys.th.title)[0]);
        let sheet: usize = shortcut_labels(HelpLang::Th)
            .into_iter()
            .map(|l| pieces(l).len())
            .sum();
        assert!(
            pieces_th.len() > sheet,
            "the sheet's descriptions are pieces too"
        );
        assert!(pieces_th.ends_with(&pieces(SHORTCUTS_TH.last().unwrap().1)));
    }

    #[test]
    fn the_language_is_kept_under_help_lang() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(HelpLang::load(None), HelpLang::Th, "no settings file: Thai");
        assert_eq!(HelpLang::load(Some(&path)), HelpLang::Th, "no key: Thai");
        HelpLang::En.save(&path).unwrap();
        assert_eq!(
            crate::settings::get_string(&path, "help.lang").as_deref(),
            Some("en")
        );
        assert_eq!(HelpLang::load(Some(&path)), HelpLang::En);
        crate::settings::set_string(&path, "help.lang", "fr").unwrap();
        assert_eq!(
            HelpLang::load(Some(&path)),
            HelpLang::Th,
            "an unknown value: Thai"
        );
        HelpLang::Th.save(&path).unwrap();
        assert_eq!(
            crate::settings::get_string(&path, "help.lang").as_deref(),
            Some("th")
        );
    }
}
