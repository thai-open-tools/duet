//! What two files that differ in bytes really are (LocalDiff's `ldClassifyText`, `de:951-984`),
//! the notes a folder row keeps about it and the status those notes decide (`deriveFmt`,
//! `applyCase`, `js:388-442`).

use std::borrow::Cow;

use encoding_rs::Encoding;

use crate::cancel::{CancelToken, Cancelled};
use crate::folder::tree::{FolderComparison, Side, Status, newer_side, rollup};
use crate::text::diff::{DiffError, DiffInput, DiffOptions};
use crate::text::encoding::{decode, is_binary};
use crate::text::lines::{EolStyle, LineIndex, detect_eol};
use crate::text::odd::{OddKind, is_odd, odd_info};
use crate::text::rules::{TextRules, diff_with_rules};

/// How one side stores its text (`ldFmtInfo`, `de:944`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextFormat {
    pub eol: EolStyle,
    pub final_newline: bool,
    pub bom: bool,
    pub encoding: &'static Encoding,
}

impl TextFormat {
    /// `eolDesc` (`js:402`): the line endings, then what else is unusual, then the encoding.
    pub fn describe(&self) -> String {
        let eol = match self.eol {
            EolStyle::CrLf => "CRLF (Windows)",
            EolStyle::Lf => "LF (Linux)",
            EolStyle::Cr => "CR (old Mac)",
            EolStyle::Mixed => "mixed line endings",
            EolStyle::None => "one line",
        };
        let mut out = eol.to_owned();
        if !self.final_newline {
            out.push_str(", no final newline");
        }
        if self.bom {
            out.push_str(", BOM");
        }
        out.push_str(", ");
        out.push_str(self.encoding.name());
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Same,
    /// Every line is equal; only line endings, the final newline, a BOM or the encoding differ.
    Format,
    /// Equal once look-alike spaces and zero-width characters are normalised.
    OddSpace,
    /// Equal under the compare rules.
    Soft,
    Different,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub verdict: Verdict,
    /// Both sides' formats, for Format, OddSpace and Soft.
    pub formats: Option<[TextFormat; 2]>,
    pub shebang: bool,
}

/// Step 6 of R5: the diff options and rules one file pair is compared with.
pub struct LineRules<'a> {
    pub options: DiffOptions,
    pub rules: &'a TextRules,
}

impl Classified {
    fn plain(verdict: Verdict) -> Self {
        Self {
            verdict,
            formats: None,
            shebang: false,
        }
    }
}

fn format_of(decoded: &crate::text::encoding::Decoded<'_>) -> TextFormat {
    let text = decoded.text.as_bytes();
    TextFormat {
        eol: detect_eol(text),
        final_newline: matches!(text.last(), Some(b'\n' | b'\r')),
        bom: decoded.encoding.bom,
        encoding: decoded.encoding.encoding,
    }
}

pub fn classify(
    left: &[u8],
    right: &[u8],
    rules: Option<&LineRules<'_>>,
    cancel: &CancelToken,
) -> Result<Classified, Cancelled> {
    cancel.check()?;
    if left == right {
        return Ok(Classified::plain(Verdict::Same));
    }
    if is_binary(left) || is_binary(right) {
        return Ok(Classified::plain(Verdict::Different));
    }
    let (dl, dr) = (decode(left), decode(right));
    if dl.lossy || dr.lossy {
        return Ok(Classified::plain(Verdict::Different));
    }
    let (tl, tr) = (dl.text.as_bytes(), dr.text.as_bytes());
    let (il, ir) = (LineIndex::new(tl), LineIndex::new(tr));
    let formats = [format_of(&dl), format_of(&dr)];
    let shebang = dl.text.starts_with("#!") || dr.text.starts_with("#!");
    let found = |verdict| Classified {
        verdict,
        formats: Some(formats),
        shebang,
    };
    let same_count = il.len() == ir.len();
    if same_count {
        if (0..il.len()).all(|n| il.line_bytes(tl, n) == ir.line_bytes(tr, n)) {
            return Ok(found(Verdict::Format));
        }
        cancel.check()?;
        let line = |bytes, index: &LineIndex, n| {
            std::str::from_utf8(index.line_bytes(bytes, n)).unwrap_or_default()
        };
        if (0..il.len()).all(|n| odd_norm(line(tl, &il, n)) == odd_norm(line(tr, &ir, n))) {
            return Ok(found(Verdict::OddSpace));
        }
    }
    cancel.check()?;
    if let Some(rules) = rules {
        let options = DiffOptions {
            ignore_eol: true,
            ..rules.options
        };
        let left_in = DiffInput {
            bytes: tl,
            lines: &il,
        };
        let right_in = DiffInput {
            bytes: tr,
            lines: &ir,
        };
        match diff_with_rules(left_in, right_in, options, rules.rules, cancel) {
            Ok((diff, _)) if diff.hunks.is_empty() => return Ok(found(Verdict::Soft)),
            Ok(_) | Err(DiffError::TooManyLines(_)) => {}
            Err(DiffError::Cancelled(c)) => return Err(c),
        }
    }
    Ok(Classified::plain(Verdict::Different))
}

/// `ldOddNorm` (`de:909`): look-alike spaces become " ", zero-width characters vanish,
/// controls stay.
pub fn odd_norm(line: &str) -> Cow<'_, str> {
    if !line.chars().any(is_odd) {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    for ch in line.chars() {
        if !is_odd(ch) {
            out.push(ch);
            continue;
        }
        match odd_info(ch).2 {
            OddKind::Space => out.push(' '),
            OddKind::Zero => {}
            OddKind::Control => out.push(ch),
        }
    }
    Cow::Owned(out)
}

const SCRIPT_ENDINGS: [&str; 11] = [
    ".sh", ".ksh", ".bash", ".zsh", ".csh", ".py", ".pl", ".awk", ".profile", ".bashrc", ".kshrc",
];

/// `isScriptPath` (`js:388`): a name Linux runs as a script, any case.
pub fn is_script(rel: &str) -> bool {
    let lower = rel.to_lowercase();
    SCRIPT_ENDINGS.iter().any(|ending| lower.ends_with(ending))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Warning {
    /// A script with CRLF or a BOM: Linux may refuse to run it.
    Script,
    /// The files differ only in look-alike spaces or zero-width characters.
    OddSpace,
    /// The names differ only in case and the contents are the same.
    CaseOnly,
}

/// What a classified row remembers about its pair.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Note {
    pub formats: Option<[TextFormat; 2]>,
    pub shebang: bool,
    /// The compare rules hid a difference between the files.
    pub soft: bool,
    pub warning: Option<Warning>,
}

impl Note {
    pub fn encoding_differs(&self) -> bool {
        self.formats.is_some_and(|[a, b]| a.encoding != b.encoding)
    }

    /// Line endings, final newline, BOM or encoding.
    pub fn format_differs(&self) -> bool {
        self.formats.is_some_and(|[a, b]| a != b)
    }

    /// Line endings or final newline (LocalDiff's `fmtShort`: "EOL only" against "BOM only").
    pub fn eol_differs(&self) -> bool {
        self.formats
            .is_some_and(|[a, b]| a.eol != b.eol || a.final_newline != b.final_newline)
    }

    pub fn swapped(&self) -> Note {
        Note {
            formats: self.formats.map(|[a, b]| [b, a]),
            ..self.clone()
        }
    }
}

/// `deriveFmt` for a pair whose lines are equal (or equal under the rules).
fn derive_formats(
    formats: [TextFormat; 2],
    shebang: bool,
    soft: bool,
    rel: &str,
    newer: Option<Side>,
    format_important: bool,
) -> (Status, Note) {
    let [a, b] = formats;
    let eol_diff = a.eol != b.eol && a.eol != EolStyle::None && b.eol != EolStyle::None;
    let crlf = |eol| matches!(eol, EolStyle::CrLf | EolStyle::Cr | EolStyle::Mixed);
    let script = shebang || is_script(rel);
    let warn = script && ((eol_diff && (crlf(a.eol) || crlf(b.eol))) || a.bom != b.bom);
    let mut note = Note {
        formats: Some(formats),
        shebang,
        soft,
        warning: warn.then_some(Warning::Script),
    };
    let differs = warn || a.encoding != b.encoding || (format_important && note.format_differs());
    let status = if differs {
        Status::Different { newer }
    } else {
        Status::Similar { newer }
    };
    if !differs {
        note.warning = None;
    }
    (status, note)
}

/// R6 for one classified pair.
pub fn derive_status(
    c: &Classified,
    rel: &str,
    newer: Option<Side>,
    format_important: bool,
) -> (Status, Option<Box<Note>>) {
    match (c.verdict, c.formats) {
        (Verdict::Same, _) => (Status::Same, None),
        (Verdict::Format | Verdict::Soft, Some(formats)) => {
            let soft = c.verdict == Verdict::Soft;
            let (status, note) =
                derive_formats(formats, c.shebang, soft, rel, newer, format_important);
            (status, Some(Box::new(note)))
        }
        (Verdict::OddSpace, formats) => (
            Status::Different { newer },
            Some(Box::new(Note {
                formats,
                shebang: c.shebang,
                soft: false,
                warning: Some(Warning::OddSpace),
            })),
        ),
        _ => (Status::Different { newer }, None),
    }
}

/// R6's case-only rule over every file pair; the caller rolls up.
pub fn apply_case(cmp: &mut FolderComparison) {
    let quick = cmp.quick;
    for node in &mut cmp.nodes {
        if !node.case_differs() || !matches!(node.status, Status::Same | Status::Similar { .. }) {
            continue;
        }
        let newer = match (&node.left, &node.right) {
            (Some(l), Some(r)) => newer_side(l, r, quick),
            _ => None,
        };
        node.status = Status::Different { newer };
        node.note.get_or_insert_with(Default::default).warning = Some(Warning::CaseOnly);
    }
}

/// R4's toggle: re-derives every node with a note's formats, applies the case rule again and
/// rolls up. No file is read.
pub fn reapply_format_rule(cmp: &mut FolderComparison, format_important: bool) {
    let quick = cmp.quick;
    for node in &mut cmp.nodes {
        let Some(note) = node.note.as_deref() else {
            continue;
        };
        let Some(formats) = note.formats else {
            continue;
        };
        if note.warning == Some(Warning::OddSpace) {
            continue;
        }
        let newer = match (&node.left, &node.right) {
            (Some(l), Some(r)) => newer_side(l, r, quick),
            _ => None,
        };
        let (status, fresh) = derive_formats(
            formats,
            note.shebang,
            note.soft,
            &node.rel,
            newer,
            format_important,
        );
        node.status = status;
        node.note = Some(Box::new(fresh));
    }
    apply_case(cmp);
    rollup(cmp);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder::entry::{Entry, EntryKind};
    use crate::folder::tree::{QuickRule, build_tree};
    use crate::text::diff::WhitespaceMode;
    use crate::text::lexer::Language;
    use encoding_rs::{UTF_8, WINDOWS_874};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const THAI_ONCE: &str = "ภาษาไทยเป็นภาษาราชการของประเทศไทย สวัสดีครับ ขอบคุณมาก ";

    fn thai() -> String {
        THAI_ONCE.repeat(4)
    }

    fn fmt(
        eol: EolStyle,
        final_newline: bool,
        bom: bool,
        encoding: &'static Encoding,
    ) -> TextFormat {
        TextFormat {
            eol,
            final_newline,
            bom,
            encoding,
        }
    }

    fn u(eol: EolStyle, final_newline: bool) -> TextFormat {
        fmt(eol, final_newline, false, UTF_8)
    }

    fn run(l: &[u8], r: &[u8]) -> Classified {
        classify(l, r, None, &CancelToken::default()).unwrap()
    }

    fn run_rules(l: &str, r: &str, options: DiffOptions, rules: &TextRules) -> Classified {
        let rules = LineRules { options, rules };
        classify(
            l.as_bytes(),
            r.as_bytes(),
            Some(&rules),
            &CancelToken::default(),
        )
        .unwrap()
    }

    type Case = (
        &'static str,
        Vec<u8>,
        Vec<u8>,
        Verdict,
        Option<[TextFormat; 2]>,
        bool,
    );
    type Row = (
        Verdict,
        [TextFormat; 2],
        bool,
        &'static str,
        bool,
        Status,
        Option<Warning>,
        bool,
    );

    #[test]
    fn classify_follows_localdiff() {
        use EolStyle::{Cr, CrLf, Lf, Mixed, None as NoEol};
        let cases: Vec<Case> = vec![
            (
                "crlf vs lf",
                b"a\r\nb\r\n".to_vec(),
                b"a\nb\n".to_vec(),
                Verdict::Format,
                Some([u(CrLf, true), u(Lf, true)]),
                false,
            ),
            (
                "final newline",
                b"a\nb".to_vec(),
                b"a\nb\n".to_vec(),
                Verdict::Format,
                Some([u(Lf, false), u(Lf, true)]),
                false,
            ),
            (
                "one line",
                b"x".to_vec(),
                b"x\n".to_vec(),
                Verdict::Format,
                Some([u(NoEol, false), u(Lf, true)]),
                false,
            ),
            (
                "bom",
                "\u{feff}x\n".as_bytes().to_vec(),
                b"x\n".to_vec(),
                Verdict::Format,
                Some([fmt(Lf, true, true, UTF_8), u(Lf, true)]),
                false,
            ),
            (
                "mixed",
                b"a\r\nb\n".to_vec(),
                b"a\nb\n".to_vec(),
                Verdict::Format,
                Some([u(Mixed, true), u(Lf, true)]),
                false,
            ),
            (
                "cr",
                b"a\rb\r".to_vec(),
                b"a\nb\n".to_vec(),
                Verdict::Format,
                Some([u(Cr, true), u(Lf, true)]),
                false,
            ),
            (
                "encoding",
                WINDOWS_874.encode(&format!("{}\n", thai())).0.into_owned(),
                format!("{}\n", thai()).into_bytes(),
                Verdict::Format,
                Some([fmt(Lf, true, false, WINDOWS_874), u(Lf, true)]),
                false,
            ),
            (
                "nbsp",
                b"timeout = 30\n".to_vec(),
                "timeout =\u{a0}30\n".as_bytes().to_vec(),
                Verdict::OddSpace,
                Some([u(Lf, true), u(Lf, true)]),
                false,
            ),
            (
                "zero width",
                b"ab\n".to_vec(),
                "a\u{200b}b\n".as_bytes().to_vec(),
                Verdict::OddSpace,
                Some([u(Lf, true), u(Lf, true)]),
                false,
            ),
            (
                "controls differ",
                "a\u{7}b\n".as_bytes().to_vec(),
                "a\u{8}b\n".as_bytes().to_vec(),
                Verdict::Different,
                None,
                false,
            ),
            (
                "text differs",
                b"a\n".to_vec(),
                b"b\n".to_vec(),
                Verdict::Different,
                None,
                false,
            ),
            (
                "odd and real difference",
                "a\u{a0}b\nc\n".as_bytes().to_vec(),
                b"a b\nd\n".to_vec(),
                Verdict::Different,
                None,
                false,
            ),
            (
                "odd and line count",
                "a\u{a0}b\n".as_bytes().to_vec(),
                b"a b\n\n".to_vec(),
                Verdict::Different,
                None,
                false,
            ),
            (
                "shebang",
                b"#!/bin/bash\necho hi\n".to_vec(),
                b"#!/bin/bash\r\necho hi\r\n".to_vec(),
                Verdict::Format,
                Some([u(Lf, true), u(CrLf, true)]),
                true,
            ),
            (
                "binary",
                vec![0, 1, 2],
                vec![0, 1, 3],
                Verdict::Different,
                None,
                false,
            ),
            (
                "empty vs newline",
                b"".to_vec(),
                b"\n".to_vec(),
                Verdict::Different,
                None,
                false,
            ),
            (
                "same",
                b"same".to_vec(),
                b"same".to_vec(),
                Verdict::Same,
                None,
                false,
            ),
        ];
        for (name, l, r, verdict, formats, shebang) in cases {
            let c = run(&l, &r);
            assert_eq!(
                c,
                Classified {
                    verdict,
                    formats,
                    shebang
                },
                "{name}"
            );
        }
    }

    #[test]
    fn rules_make_a_pair_soft() {
        let all = DiffOptions {
            whitespace: WhitespaceMode::All,
            ..DiffOptions::default()
        };
        let none = TextRules::default();
        assert_eq!(
            run_rules("x  y\n", "x y\n", all, &none).verdict,
            Verdict::Soft
        );
        assert_eq!(
            run_rules("a\n", "b\n", all, &none).verdict,
            Verdict::Different
        );
        let script = run_rules(
            "#!/bin/bash\necho  hi\n",
            "#!/bin/bash\r\necho hi\r\n",
            all,
            &none,
        );
        assert_eq!(script.verdict, Verdict::Soft);
        assert_eq!(
            script.formats,
            Some([u(EolStyle::Lf, true), u(EolStyle::CrLf, true)])
        );
        assert!(script.shebang);
        let blank = TextRules {
            ignore_blank: true,
            ..TextRules::default()
        };
        assert_eq!(
            run_rules("a\n\nb\n", "a\nb\n", DiffOptions::default(), &blank).verdict,
            Verdict::Soft
        );
        let case = DiffOptions {
            ignore_case: true,
            ..DiffOptions::default()
        };
        assert_eq!(run_rules("A\n", "a\n", case, &none).verdict, Verdict::Soft);
        let comments = TextRules {
            comments: Some(Language::CLike),
            ..TextRules::default()
        };
        assert_eq!(
            run_rules(
                "let a = 1; // x\nlet b = 2;\n",
                "let a = 1;\nlet b = 2;\n",
                DiffOptions::default(),
                &comments
            )
            .verdict,
            Verdict::Soft
        );
    }

    #[test]
    fn a_cancelled_token_stops_classify() {
        let cancel = CancelToken::default();
        cancel.cancel();
        assert_eq!(classify(b"a", b"b", None, &cancel), Err(Cancelled));
    }

    #[test]
    fn odd_norm_maps_spaces_and_drops_zero_widths() {
        assert_eq!(odd_norm("a\u{a0}b\u{200b}c\u{7}"), "a bc\u{7}");
        assert!(matches!(odd_norm("plain"), Cow::Borrowed(_)));
    }

    #[test]
    fn script_names_follow_localdiff() {
        for yes in [
            "a.SH",
            "x/y.ksh",
            "a.bash",
            "a.zsh",
            "a.csh",
            "a.py",
            "a.pl",
            "a.awk",
            "x.profile",
            ".bashrc",
            "d/.kshrc",
        ] {
            assert!(is_script(yes), "{yes}");
        }
        for no in ["a.shx", "sh", "a.txt", "a.ps1"] {
            assert!(!is_script(no), "{no}");
        }
    }

    #[test]
    fn formats_describe_themselves() {
        use EolStyle::{Cr, CrLf, Lf, Mixed, None as NoEol};
        assert_eq!(u(CrLf, true).describe(), "CRLF (Windows), UTF-8");
        assert_eq!(
            fmt(Lf, false, true, UTF_8).describe(),
            "LF (Linux), no final newline, BOM, UTF-8"
        );
        assert_eq!(u(Cr, true).describe(), "CR (old Mac), UTF-8");
        assert_eq!(u(Mixed, true).describe(), "mixed line endings, UTF-8");
        assert_eq!(
            u(NoEol, false).describe(),
            "one line, no final newline, UTF-8"
        );
        assert!(
            fmt(Lf, true, false, WINDOWS_874)
                .describe()
                .ends_with(", windows-874")
        );
    }

    fn classified(verdict: Verdict, formats: Option<[TextFormat; 2]>, shebang: bool) -> Classified {
        Classified {
            verdict,
            formats,
            shebang,
        }
    }

    #[test]
    fn formats_decide_the_status_as_localdiff_does() {
        use EolStyle::{CrLf, Lf, Mixed, None as NoEol};
        let similar = Status::Similar { newer: None };
        let different = Status::Different { newer: None };
        let crlf_lf = [u(CrLf, true), u(Lf, true)];
        let lf_crlf = [u(Lf, true), u(CrLf, true)];
        let bom_pair = [fmt(Lf, true, false, UTF_8), fmt(Lf, true, true, UTF_8)];
        let enc_pair = [fmt(Lf, true, false, WINDOWS_874), u(Lf, true)];
        let lf_lf = [u(Lf, true), u(Lf, true)];
        // (verdict, formats, shebang, rel, important, status, warning, soft)
        let rows: Vec<Row> = vec![
            (
                Verdict::Format,
                crlf_lf,
                false,
                "a.txt",
                false,
                similar.clone(),
                None,
                false,
            ),
            (
                Verdict::Format,
                crlf_lf,
                false,
                "a.txt",
                true,
                different.clone(),
                None,
                false,
            ),
            (
                Verdict::Format,
                lf_crlf,
                false,
                "bin/run.sh",
                false,
                different.clone(),
                Some(Warning::Script),
                false,
            ),
            (
                Verdict::Format,
                lf_crlf,
                true,
                "run",
                false,
                different.clone(),
                Some(Warning::Script),
                false,
            ),
            (
                Verdict::Format,
                bom_pair,
                false,
                "x.py",
                false,
                different.clone(),
                Some(Warning::Script),
                false,
            ),
            (
                Verdict::Format,
                [u(NoEol, false), u(CrLf, true)],
                false,
                "x.sh",
                false,
                similar.clone(),
                None,
                false,
            ),
            (
                Verdict::Format,
                [u(Mixed, true), u(Lf, true)],
                false,
                "x.sh",
                false,
                different.clone(),
                Some(Warning::Script),
                false,
            ),
            (
                Verdict::Format,
                enc_pair,
                false,
                "a.txt",
                false,
                different.clone(),
                None,
                false,
            ),
            (
                Verdict::Soft,
                lf_lf,
                false,
                "a.txt",
                true,
                similar.clone(),
                None,
                true,
            ),
            (
                Verdict::Soft,
                lf_crlf,
                true,
                "run.sh",
                false,
                different.clone(),
                Some(Warning::Script),
                true,
            ),
        ];
        for (verdict, formats, shebang, rel, important, status, warning, soft) in rows {
            let c = classified(verdict, Some(formats), shebang);
            let (got, note) = derive_status(&c, rel, None, important);
            let note = note.expect("a note");
            assert_eq!(got, status, "{verdict:?} {rel} {important}");
            assert_eq!(note.warning, warning, "{rel}");
            assert_eq!(note.soft, soft, "{rel}");
            assert_eq!(note.formats, Some(formats), "{rel}");
        }
        let (_, note) = derive_status(
            &classified(Verdict::Format, Some(enc_pair), false),
            "a.txt",
            None,
            false,
        );
        assert!(note.unwrap().encoding_differs());

        let odd = classified(Verdict::OddSpace, Some(lf_lf), false);
        let (status, note) = derive_status(&odd, "t.properties", None, false);
        assert_eq!(status, different);
        assert_eq!(note.unwrap().warning, Some(Warning::OddSpace));
        assert_eq!(
            derive_status(&classified(Verdict::Same, None, false), "a", None, true),
            (Status::Same, None)
        );
        let newer = Some(Side::Right);
        assert_eq!(
            derive_status(
                &classified(Verdict::Different, None, false),
                "a",
                newer,
                true
            ),
            (Status::Different { newer }, None)
        );
        assert_eq!(
            derive_status(
                &classified(Verdict::Format, Some(crlf_lf), false),
                "a.txt",
                newer,
                false
            )
            .0,
            Status::Similar { newer }
        );
    }

    #[test]
    fn note_questions() {
        use EolStyle::{CrLf, Lf};
        let eol = Note {
            formats: Some([u(CrLf, true), u(Lf, true)]),
            ..Note::default()
        };
        assert!(eol.format_differs() && eol.eol_differs() && !eol.encoding_differs());
        let nl = Note {
            formats: Some([u(Lf, false), u(Lf, true)]),
            ..Note::default()
        };
        assert!(nl.eol_differs());
        let bom = Note {
            formats: Some([fmt(Lf, true, true, UTF_8), u(Lf, true)]),
            ..Note::default()
        };
        assert!(bom.format_differs() && !bom.eol_differs());
        assert!(!Note::default().format_differs());
    }

    fn at(secs: u64) -> Option<SystemTime> {
        Some(UNIX_EPOCH + Duration::from_secs(secs))
    }

    fn file(rel: &str, len: u64, secs: u64) -> Entry {
        Entry {
            rel: rel.into(),
            kind: EntryKind::File,
            len,
            modified: at(secs),
            error: None,
            link_target: None,
        }
    }

    fn dir(rel: &str) -> Entry {
        Entry {
            rel: rel.into(),
            kind: EntryKind::Dir,
            len: 0,
            modified: at(0),
            error: None,
            link_target: None,
        }
    }

    fn case_only_tree() -> FolderComparison {
        build_tree(
            vec![dir("Src"), file("Src/a.java", 5, 10)],
            vec![dir("src"), file("src/a.java", 5, 10)],
            true,
        )
    }

    fn find<'a>(cmp: &'a FolderComparison, rel: &str) -> &'a crate::folder::tree::Node {
        cmp.nodes.iter().find(|n| n.rel == rel).unwrap()
    }

    #[test]
    fn case_only_names_become_different_with_a_warning() {
        let mut cmp = case_only_tree();
        apply_case(&mut cmp);
        rollup(&mut cmp);
        let file_node = find(&cmp, "Src/a.java");
        assert_eq!(file_node.status, Status::Different { newer: None });
        assert_eq!(file_node.warning(), Some(Warning::CaseOnly));
        let folder = find(&cmp, "Src");
        assert_eq!(folder.status, Status::Different { newer: None });
        assert_eq!(folder.warning(), None);
        assert!(!folder.case_differs(), "folders are not file pairs");

        let mut sizes = build_tree(vec![file("A.txt", 1, 1)], vec![file("a.txt", 2, 1)], true);
        apply_case(&mut sizes);
        assert_eq!(sizes.nodes[0].status, Status::Different { newer: None });
        assert_eq!(sizes.nodes[0].warning(), None);
        assert!(sizes.nodes[0].case_differs());

        let same = build_tree(
            vec![file("same.txt", 1, 1)],
            vec![file("same.txt", 1, 1)],
            true,
        );
        assert!(!same.nodes[0].case_differs());
    }

    #[test]
    fn the_format_rule_can_be_flipped_without_reading_again() {
        let mut cmp = build_tree(
            vec![
                dir("d"),
                file("d/x.txt", 3, 1),
                dir("Src"),
                file("Src/a.java", 5, 10),
            ],
            vec![
                dir("d"),
                file("d/x.txt", 3, 1),
                dir("src"),
                file("src/a.java", 5, 10),
            ],
            true,
        );
        let i = cmp.nodes.iter().position(|n| n.rel == "d/x.txt").unwrap();
        cmp.nodes[i].status = Status::Similar { newer: None };
        cmp.nodes[i].note = Some(Box::new(Note {
            formats: Some([u(EolStyle::CrLf, true), u(EolStyle::Lf, true)]),
            ..Note::default()
        }));
        rollup(&mut cmp);
        apply_case(&mut cmp);
        rollup(&mut cmp);

        reapply_format_rule(&mut cmp, true);
        assert_eq!(
            find(&cmp, "d/x.txt").status,
            Status::Different { newer: None }
        );
        assert_eq!(find(&cmp, "d").status, Status::Different { newer: None });
        assert_eq!(find(&cmp, "Src/a.java").warning(), Some(Warning::CaseOnly));
        assert_eq!(
            find(&cmp, "Src/a.java").status,
            Status::Different { newer: None }
        );

        reapply_format_rule(&mut cmp, false);
        assert_eq!(
            find(&cmp, "d/x.txt").status,
            Status::Similar { newer: None }
        );
        assert_eq!(find(&cmp, "d").status, Status::Similar { newer: None });
        assert_eq!(find(&cmp, "Src/a.java").warning(), Some(Warning::CaseOnly));
        assert_eq!(
            find(&cmp, "Src/a.java").status,
            Status::Different { newer: None }
        );
    }

    #[test]
    fn notes_swap_with_the_sides() {
        let mut cmp = build_tree(vec![file("x.txt", 3, 1)], vec![file("x.txt", 3, 1)], true);
        cmp.nodes[0].note = Some(Box::new(Note {
            formats: Some([u(EolStyle::CrLf, true), u(EolStyle::Lf, true)]),
            ..Note::default()
        }));
        cmp.swap_sides();
        assert_eq!(
            cmp.nodes[0].note.as_ref().unwrap().formats,
            Some([u(EolStyle::Lf, true), u(EolStyle::CrLf, true)])
        );
    }

    #[test]
    fn times_differ_beyond_the_rule() {
        let mut cmp = build_tree(
            vec![file("a", 1, 100), file("only", 1, 100)],
            vec![file("a", 1, 103)],
            true,
        );
        let a = cmp.nodes.iter().position(|n| n.rel == "a").unwrap() as u32;
        let only = cmp.nodes.iter().position(|n| n.rel == "only").unwrap() as u32;
        let rule = |secs| QuickRule {
            tolerance: Duration::from_secs(secs),
            ignore_dst: false,
        };
        cmp.quick = rule(2);
        assert!(cmp.time_differs(a));
        cmp.quick = rule(5);
        assert!(!cmp.time_differs(a));
        assert!(!cmp.time_differs(only));
    }
}
