//! Secret masking for report lines (`rptMaskLine`, `rep:116-159`, R5): a value that looks like a
//! password or a key is replaced by "•••••• (hidden #xxxx)". The four hex digits are a
//! fingerprint, so a changed secret still shows as changed without being readable.
//!
//! JavaScript's `\w` is `[A-Za-z0-9_]` here (Rust's is Unicode), and the two back-references of
//! LocalDiff's patterns are checked after the match instead (a closing tag name, a closing
//! quote); neither changes a result on LocalDiff's cases.
//!
//! LocalDiff masks one line at a time. `mask_lines` adds what spans lines: private-key blocks
//! and YAML block values under a secret's name.

use std::borrow::Cow;
use std::sync::OnceLock;

use regex::{Captures, Regex};

/// `rptHide`'s fingerprint: the top 16 bits of FNV-1a over the value's UTF-16 code units.
pub fn fingerprint(value: &str) -> String {
    let mut hash: u32 = 0x811c_9dc5;
    for unit in value.encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_mul(16_777_619);
    }
    format!("{:04x}", hash >> 16)
}

/// "•••••• (hidden #xxxx)".
pub fn hidden(value: &str) -> String {
    format!("•••••• (hidden #{})", fingerprint(value))
}

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("a fixed pattern"))
}

/// `rptSecretName`: a key whose value should not travel in a report.
pub fn secret_name(key: &str) -> bool {
    static KEY: OnceLock<Regex> = OnceLock::new();
    static WORD: OnceLock<Regex> = OnceLock::new();
    static CAMEL: OnceLock<Regex> = OnceLock::new();
    // "pass" and "salt" must be words of their own (PASS, zip-pass, KeyPairPass), not bypass,
    // passive or compass.
    regex(
        &KEY,
        r"(?i)(passw(?:or)?d|passwd|pwd|_pw$|^pw$|pass_?phrase|secret|token|credential|api[_.-]?key|private[_.-]?key|storepass|keypass|[._-]key$|^key$)",
    )
    .is_match(key)
        || regex(&WORD, r"(?i)(^|[^A-Za-z])(pass|salt)([^A-Za-z]|$)").is_match(key)
        || regex(&CAMEL, r"[a-z](Pass|Salt)([^a-z]|$)").is_match(key)
}

fn placeholder(value: &str) -> bool {
    static PLACEHOLDER: OnceLock<Regex> = OnceLock::new();
    regex(
        &PLACEHOLDER,
        r"^\$\{[^}]*\}$|^\$[A-Za-z_][A-Za-z0-9_]*$|^%[^%]+%$",
    )
    .is_match(value)
}

/// `rptMaskLine`.
pub fn mask_line(line: &str) -> Cow<'_, str> {
    static HINT: OnceLock<Regex> = OnceLock::new();
    static WHOLE: OnceLock<Regex> = OnceLock::new();
    static SPACED: OnceLock<Regex> = OnceLock::new();
    static ELEMENT: OnceLock<Regex> = OnceLock::new();
    static QUOTED: OnceLock<Regex> = OnceLock::new();
    static BARE: OnceLock<Regex> = OnceLock::new();
    if !regex(&HINT, r"(?i)pass|pwd|secret|token|credential|salt|key|pw").is_match(line) {
        return Cow::Borrowed(line);
    }
    // The whole line is `key = value` or `key: value` (properties, yaml, ini; a commented-out
    // line too).
    let whole = regex(
        &WHOLE,
        r"^(\s*[#;!]?\s*)([A-Za-z0-9_.\-\[\]/@$]+)(\s*[=:]\s*)(.*?)(\s*)$",
    );
    if let Some(m) = whole.captures(line)
        && !m[4].is_empty()
        && secret_name(&m[2])
    {
        if placeholder(&m[4]) {
            return Cow::Borrowed(line);
        }
        return Cow::Owned(format!(
            "{}{}{}{}{}",
            &m[1],
            &m[2],
            &m[3],
            hidden(&m[4]),
            &m[5]
        ));
    }
    // Space-separated properties: `storepass changeit`.
    let spaced = regex(&SPACED, r"^(\s*)([A-Za-z0-9_.\-]+)(\s+)([^\s=:].*?)(\s*)$");
    if let Some(m) = spaced.captures(line)
        && secret_name(&m[2])
        && !placeholder(&m[4])
    {
        return Cow::Owned(format!(
            "{}{}{}{}{}",
            &m[1],
            &m[2],
            &m[3],
            hidden(&m[4]),
            &m[5]
        ));
    }
    // In the middle of a line: `<PASS>v</PASS>`, `password="v"`, `export DB_PASSWORD=v`,
    // `jdbc:...;password=v`, `x#KeyPairPass=v`.
    let element = regex(
        &ELEMENT,
        r"<(!--\s*)?([A-Za-z0-9_.:]+(?:-[A-Za-z0-9_.:]+)*)>([^<]+)</([A-Za-z0-9_.:]+(?:-[A-Za-z0-9_.:]+)*)(\s*--)?>",
    );
    let quoted = regex(
        &QUOTED,
        r#"(([A-Za-z0-9_.:-]+)["']?\s*[=:]\s*)(["'])([^"']*)(["'])"#,
    );
    let bare = regex(&BARE, r#"(([A-Za-z0-9_.:-]+)\s*[=:]\s*)([^\s"'<>,;&]+)"#);
    let step = element.replace_all(line, |c: &Captures| {
        let (tag, value) = (&c[2], &c[3]);
        if tag == &c[4] && secret_name(tag) && !placeholder(value.trim()) {
            format!(
                "<{}{tag}>{}</{tag}{}>",
                c.get(1).map_or("", |m| m.as_str()),
                hidden(value),
                c.get(5).map_or("", |m| m.as_str()),
            )
        } else {
            c[0].to_owned()
        }
    });
    let step = quoted.replace_all(&step, |c: &Captures| {
        let (quote, value) = (&c[3], &c[4]);
        if quote == &c[5] && !value.is_empty() && secret_name(&c[2]) && !placeholder(value) {
            format!("{}{quote}{}{quote}", &c[1], hidden(value))
        } else {
            c[0].to_owned()
        }
    });
    let step = bare.replace_all(&step, |c: &Captures| {
        let value = &c[3];
        if secret_name(&c[2])
            && !placeholder(value)
            && !value.contains("••••")
            && !value.starts_with('(')
        {
            format!("{}{}", &c[1], hidden(value))
        } else {
            c[0].to_owned()
        }
    });
    if step == line {
        Cow::Borrowed(line)
    } else {
        Cow::Owned(step.into_owned())
    }
}

/// The first and last lines of a private key: RSA, EC, DSA, OPENSSH, ENCRYPTED, PGP's
/// "… BLOCK" and the SSH2 form with four dashes. Public blocks (CERTIFICATE, PUBLIC KEY) differ.
fn key_begin() -> &'static Regex {
    static BEGIN: OnceLock<Regex> = OnceLock::new();
    regex(
        &BEGIN,
        r"-{4,5} ?BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)? ?-{4,5}",
    )
}

fn key_end() -> &'static Regex {
    static END: OnceLock<Regex> = OnceLock::new();
    regex(
        &END,
        r"-{4,5} ?END (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)? ?-{4,5}",
    )
}

/// `text` behind its own fingerprint, after its leading whitespace; blank text stays as it
/// is. Returns whether anything was hidden.
fn push_hidden(out: &mut String, text: &str) -> bool {
    let value = text.trim_start();
    if value.is_empty() {
        out.push_str(text);
        return false;
    }
    out.push_str(&text[..text.len() - value.len()]);
    out.push_str(&hidden(value));
    true
}

/// Hides the private-key text of one line. `open` says whether a key runs on from the line
/// above, and is left saying whether it runs on to the next. None when nothing is hidden.
fn hide_keys(line: &str, open: &mut bool) -> Option<String> {
    let (begin, end) = (key_begin(), key_end());
    if !*open && !begin.is_match(line) {
        return None;
    }
    let (mut out, mut rest, mut changed) = (String::with_capacity(line.len()), line, false);
    loop {
        let mut one_line = false;
        if !*open {
            let Some(m) = begin.find(rest) else {
                out.push_str(rest);
                break;
            };
            out.push_str(&rest[..m.end()]);
            rest = &rest[m.end()..];
            *open = true;
            if rest.trim().is_empty() {
                // The marker ends the line: the key's lines follow.
                out.push_str(rest);
                break;
            }
            // A key on one line (JSON's "\n" form): hidden up to its END marker or the end of
            // the line.
            one_line = true;
        }
        let stop = end.find(rest);
        changed |= push_hidden(&mut out, &rest[..stop.map_or(rest.len(), |m| m.start())]);
        let Some(m) = stop else {
            // A block with no END line is hidden to the end of the file (fail closed).
            *open = !one_line;
            break;
        };
        out.push_str(m.as_str());
        rest = &rest[m.end()..];
        *open = false;
    }
    changed.then_some(out)
}

/// The indent of a YAML block value (`key: |`, `key: >-`, `- key: |`) when the key is a
/// secret's name: the lines indented deeper below it hold the value.
fn yaml_secret_block(line: &str) -> Option<usize> {
    static BLOCK: OnceLock<Regex> = OnceLock::new();
    let m = regex(
        &BLOCK,
        r#"^(\s*(?:-\s+)?)["']?([A-Za-z0-9_.\-\[\]/@$]+)["']?\s*:\s*[|>][0-9+-]{0,2}\s*(?:#.*)?$"#,
    )
    .captures(line)?;
    secret_name(&m[2]).then(|| m[1].len())
}

fn indent_of(line: &str) -> usize {
    line.bytes()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .count()
}

/// `mask_line` over a whole file, with the state one line cannot carry (K3, M-5):
/// - each line of a `-----BEGIN … PRIVATE KEY-----` block is hidden with its own fingerprint,
///   while the BEGIN and END lines stay; a block with no END line is hidden to the end;
/// - a key on one line (JSON's `"-----BEGIN … PRIVATE KEY-----\n…"`) is hidden after its
///   BEGIN marker, up to its END marker or the end of the line;
/// - each line of a YAML block value under a secret's name (`password: |`) is hidden.
///
/// Every other line goes through `mask_line`. Returns how many lines it changed.
pub fn mask_lines(lines: &mut [String]) -> usize {
    let (mut key_open, mut yaml) = (false, None::<usize>);
    let mut changed = 0;
    for line in lines.iter_mut() {
        if let Some(indent) = yaml
            && !line.trim().is_empty()
            && indent_of(line) <= indent
        {
            yaml = None;
        }
        let new = if yaml.is_some() && !key_open && !key_begin().is_match(line) {
            let mut out = String::with_capacity(line.len());
            push_hidden(&mut out, line).then_some(out)
        } else {
            if yaml.is_none() && !key_open {
                yaml = yaml_secret_block(line);
            }
            let keyed = hide_keys(line, &mut key_open);
            let masked = match mask_line(keyed.as_deref().unwrap_or(line)) {
                Cow::Owned(masked) => Some(masked),
                Cow::Borrowed(_) => None,
            };
            masked.or(keyed)
        };
        if let Some(new) = new {
            *line = new;
            changed += 1;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "•••••• (hidden #";

    fn hid(hex: &str) -> String {
        format!("{H}{hex})")
    }

    #[test]
    fn secret_values_are_hidden_with_a_fingerprint() {
        let cases: Vec<(String, String)> = vec![
            (
                "db.password=abc".into(),
                format!("db.password={}", hid("1a47")),
            ),
            (
                "<PASS>abc</PASS>".into(),
                format!("<PASS>{}</PASS>", hid("1a47")),
            ),
            (
                "\t<!--sap-password>abc</sap-password-->".into(),
                format!("\t<!--sap-password>{}</sap-password-->", hid("1a47")),
            ),
            (
                "KeyPairPass=abc".into(),
                format!("KeyPairPass={}", hid("1a47")),
            ),
            (
                "export DB_PASSWORD=abc".into(),
                format!("export DB_PASSWORD={}", hid("1a47")),
            ),
            (
                "url=jdbc:x;password=abc;db=y".into(),
                format!("url=jdbc:x;password={};db=y", hid("1a47")),
            ),
            (
                "\"password\": \"abc\",".into(),
                format!("\"password\": \"{}\",", hid("1a47")),
            ),
            ("storepass abc".into(), format!("storepass {}", hid("1a47"))),
            (
                "<HASH_SALT>abc</HASH_SALT>".into(),
                format!("<HASH_SALT>{}</HASH_SALT>", hid("1a47")),
            ),
            ("aes.key=abc".into(), format!("aes.key={}", hid("1a47"))),
            (
                "password=Secr3t-A".into(),
                format!("password={}", hid("d397")),
            ),
            (
                "password=Secr3t-B".into(),
                format!("password={}", hid("d097")),
            ),
            (
                "  <PASS>Zz9-same</PASS>".into(),
                format!("  <PASS>{}</PASS>", hid("f077")),
            ),
            ("token: \"x y\"".into(), format!("token: {}", hid("ad09"))),
            ("pwd = abc  ".into(), format!("pwd = {}  ", hid("1a47"))),
        ];
        for (input, want) in cases {
            let got = mask_line(&input);
            assert_eq!(got, want, "input {input:?}");
            assert!(matches!(got, Cow::Owned(_)), "input {input:?}");
        }
        for same in [
            "password=${DB_PASS}",
            "bypass=true",
            "passive.mode=on",
            "<MODE>passive</MODE>",
            "key_store = /p/k.jks",
            "keyword=abc",
            "# password must be rotated",
        ] {
            assert!(
                matches!(mask_line(same), Cow::Borrowed(s) if s == same),
                "{same}"
            );
        }
    }

    #[test]
    fn fingerprints_follow_localdiff() {
        for (value, want) in [
            ("abc", "1a47"),
            ("Secr3t-A", "d397"),
            ("Secr3t-B", "d097"),
            ("Zz9-same", "f077"),
            ("รหัส", "3141"),
        ] {
            assert_eq!(fingerprint(value), want, "{value}");
        }
        assert_eq!(hidden("abc"), hid("1a47"));
    }

    #[test]
    fn secret_names_follow_localdiff() {
        for name in [
            "password",
            "db.password",
            "KeyPairPass",
            "zip-pass",
            "ZIP_PASS",
            "aes.key",
            "key",
            "api_key",
            "apiKey",
            "HASH_SALT",
            "pwd",
            "my_pw",
            "pw",
            "secretary",
        ] {
            assert!(secret_name(name), "{name}");
        }
        for name in ["bypass", "compass", "keyword", "salty"] {
            assert!(!secret_name(name), "{name}");
        }
    }

    fn masked(lines: &[&str]) -> (Vec<String>, usize) {
        let mut out: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
        let changed = mask_lines(&mut out);
        (out, changed)
    }

    #[test]
    fn private_key_blocks_are_hidden_line_by_line() {
        let (begin, end) = (
            "-----BEGIN RSA PRIVATE KEY-----",
            "-----END RSA PRIVATE KEY-----",
        );
        let (out, changed) = masked(&[
            "a=1",
            begin,
            "MIIEowIBAAKCAQEA0Z3",
            "q5x/AbC+def==",
            end,
            "b=2",
        ]);
        assert_eq!(
            out,
            [
                "a=1".to_owned(),
                begin.to_owned(),
                hidden("MIIEowIBAAKCAQEA0Z3"),
                hidden("q5x/AbC+def=="),
                end.to_owned(),
                "b=2".to_owned(),
            ]
        );
        assert_eq!(changed, 2);

        for kind in ["OPENSSH ", "EC ", "ENCRYPTED ", "DSA ", ""] {
            let begin = format!("-----BEGIN {kind}PRIVATE KEY-----");
            let end = format!("-----END {kind}PRIVATE KEY-----");
            let (out, changed) = masked(&[begin.as_str(), "b3BlbnNzaC1rZXk", end.as_str()]);
            assert_eq!(
                out,
                [begin.clone(), hidden("b3BlbnNzaC1rZXk"), end],
                "{kind}"
            );
            assert_eq!(changed, 1, "{kind}");
        }
        // The SSH2 form has four dashes and spaces inside them, and a header line.
        let ssh2 = [
            "---- BEGIN SSH2 ENCRYPTED PRIVATE KEY ----",
            "Comment: \"rsa-key\"",
            "P2/56wAAAi4AAAA",
            "---- END SSH2 ENCRYPTED PRIVATE KEY ----",
        ];
        let (out, changed) = masked(&ssh2);
        assert_eq!(
            out,
            [
                ssh2[0].to_owned(),
                hidden(ssh2[1]),
                hidden(ssh2[2]),
                ssh2[3].to_owned(),
            ]
        );
        assert_eq!(changed, 2);

        // PGP's header line is hidden too; its blank line stays blank.
        let (out, changed) = masked(&[
            "-----BEGIN PGP PRIVATE KEY BLOCK-----",
            "Version: GnuPG v2",
            "",
            "lQOYBF0",
            "=Ab1c",
            "-----END PGP PRIVATE KEY BLOCK-----",
        ]);
        assert_eq!(
            out,
            [
                "-----BEGIN PGP PRIVATE KEY BLOCK-----".to_owned(),
                hidden("Version: GnuPG v2"),
                String::new(),
                hidden("lQOYBF0"),
                hidden("=Ab1c"),
                "-----END PGP PRIVATE KEY BLOCK-----".to_owned(),
            ]
        );
        assert_eq!(changed, 3);

        // Indented under a YAML key the masking does not know: the indent stays.
        let (out, changed) = masked(&[
            "tls:",
            "  data: |",
            "    -----BEGIN PRIVATE KEY-----",
            "    MIIEvQIBADANBg",
            "    -----END PRIVATE KEY-----",
            "  name: x",
        ]);
        assert_eq!(
            out,
            [
                "tls:".to_owned(),
                "  data: |".to_owned(),
                "    -----BEGIN PRIVATE KEY-----".to_owned(),
                format!("    {}", hidden("MIIEvQIBADANBg")),
                "    -----END PRIVATE KEY-----".to_owned(),
                "  name: x".to_owned(),
            ]
        );
        assert_eq!(changed, 1);

        // A block with no END line is hidden to the end of the file.
        let (out, changed) = masked(&["-----BEGIN PRIVATE KEY-----", "MIIEvQ", "user=x"]);
        assert_eq!(
            out,
            [
                "-----BEGIN PRIVATE KEY-----".to_owned(),
                hidden("MIIEvQ"),
                hidden("user=x"),
            ]
        );
        assert_eq!(changed, 2);

        // A key on one line (JSON's "\n" form) under a name the masking does not know.
        let json = r#"  "value": "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBg\n-----END PRIVATE KEY-----\n","#;
        let (out, changed) = masked(&[json, "  \"id\": 7"]);
        assert!(!out[0].contains("MIIE"), "{}", out[0]);
        assert_eq!(
            out[0],
            format!(
                r#"  "value": "-----BEGIN PRIVATE KEY-----{}-----END PRIVATE KEY-----\n","#,
                hidden(r"\nMIIEvQIBADANBg\n")
            )
        );
        assert_eq!((out[1].as_str(), changed), ("  \"id\": 7", 1));
        // Without its END marker it is hidden to the end of the line, and the next line stays.
        let (out, changed) = masked(&[r#""v": "-----BEGIN PRIVATE KEY-----\nMIIEvQ"#, "x=1"]);
        assert_eq!(
            out,
            [
                format!(
                    r#""v": "-----BEGIN PRIVATE KEY-----{}"#,
                    hidden(r"\nMIIEvQ")
                ),
                "x=1".to_owned(),
            ]
        );
        assert_eq!(changed, 1);

        // Public blocks stay as they are.
        for kind in ["CERTIFICATE", "PUBLIC KEY", "RSA PUBLIC KEY"] {
            let block = [
                format!("-----BEGIN {kind}-----"),
                "MIIDdzCCAl+gAwIBAgIE".to_owned(),
                format!("-----END {kind}-----"),
            ];
            let lines: Vec<&str> = block.iter().map(String::as_str).collect();
            assert_eq!(masked(&lines), (block.to_vec(), 0), "{kind}");
        }
    }

    #[test]
    fn yaml_block_values_under_a_secret_key_are_hidden() {
        let (out, changed) = masked(&[
            "db:",
            "  password: |",
            "    hunter2",
            "    more",
            "  user: x",
        ]);
        assert_eq!(
            out,
            [
                "db:".to_owned(),
                format!("  password: {}", hidden("|")),
                format!("    {}", hidden("hunter2")),
                format!("    {}", hidden("more")),
                "  user: x".to_owned(),
            ]
        );
        assert_eq!(changed, 3);

        // Folded, with a blank line inside, and as a list item.
        let (out, changed) = masked(&["token: >-", "  abc", "", "  def", "next: 1"]);
        assert_eq!(
            out,
            [
                format!("token: {}", hidden(">-")),
                format!("  {}", hidden("abc")),
                String::new(),
                format!("  {}", hidden("def")),
                "next: 1".to_owned(),
            ]
        );
        assert_eq!(changed, 3);
        let (out, changed) = masked(&["- password: |", "    s3cret", "  user: x"]);
        assert_eq!(
            out,
            [
                format!("- password: {}", hidden("|")),
                format!("    {}", hidden("s3cret")),
                "  user: x".to_owned(),
            ]
        );
        assert_eq!(changed, 2);

        // A private key under a secret name keeps its BEGIN and END lines.
        let (out, changed) = masked(&[
            "tls.key: |",
            "  -----BEGIN EC PRIVATE KEY-----",
            "  MHcCAQEE",
            "  -----END EC PRIVATE KEY-----",
            "name: x",
        ]);
        assert_eq!(
            out,
            [
                format!("tls.key: {}", hidden("|")),
                "  -----BEGIN EC PRIVATE KEY-----".to_owned(),
                format!("  {}", hidden("MHcCAQEE")),
                "  -----END EC PRIVATE KEY-----".to_owned(),
                "name: x".to_owned(),
            ]
        );
        assert_eq!(changed, 2);

        // A block under a name that is not a secret stays.
        let plain = ["description: |", "  some text", "x: 1"];
        assert_eq!(masked(&plain), (plain.map(str::to_owned).to_vec(), 0));
    }
}
