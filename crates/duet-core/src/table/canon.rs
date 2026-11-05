//! Cell comparison rules: exact decimal canonicalisation and the three optional normalisations.

use std::borrow::Cow;

use crate::table::xlsx::unmarked;

/// The value as an exact decimal string, or `None` when it is not a number. Never goes through
/// `f64` (long account numbers must not round equal) except for absurd exponents.
///
/// Grammar: `[+-]? (\d{1,3}(,\d{3})+ | \d*) (\.\d*)? ([eE][+-]?\d+)?`, at most 64 chars.
pub fn num_canon(value: &str) -> Option<String> {
    let s = value.trim();
    if s.is_empty() || s.len() > 64 {
        return None;
    }
    let b = s.as_bytes();
    if !matches!(b[0], b'0'..=b'9' | b'+' | b'-' | b'.') {
        return None;
    }
    let mut i = 0;
    let negative = b[0] == b'-';
    if matches!(b[0], b'+' | b'-') {
        i = 1;
    }
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();

    let mut int_part = String::new();
    let run = digits(i);
    if b.get(i + run) == Some(&b',') {
        if !(1..=3).contains(&run) {
            return None;
        }
        int_part.push_str(&s[i..i + run]);
        i += run;
        while b.get(i) == Some(&b',') {
            if digits(i + 1) < 3 {
                return None;
            }
            int_part.push_str(&s[i + 1..i + 4]);
            i += 4;
        }
    } else {
        int_part.push_str(&s[i..i + run]);
        i += run;
    }

    let mut frac = "";
    if b.get(i) == Some(&b'.') {
        let n = digits(i + 1);
        frac = &s[i + 1..i + 1 + n];
        i += 1 + n;
    }
    if int_part.is_empty() && frac.is_empty() {
        return None;
    }

    let mut exponent: Option<i64> = None;
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        let neg = b.get(i) == Some(&b'-');
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let n = digits(i);
        if n == 0 {
            return None;
        }
        let mut e: i64 = 0;
        for &c in &b[i..i + n] {
            e = e.saturating_mul(10).saturating_add(i64::from(c - b'0'));
        }
        i += n;
        exponent = Some(if neg { -e } else { e });
    }
    if i != b.len() {
        return None;
    }

    let mut ip: &str = int_part.trim_start_matches('0');
    let mut fp: String = frac.to_string();
    let mut shifted_int = String::new();
    if let Some(exp) = exponent {
        let digits_all = format!("{ip}{fp}");
        let point = (ip.len() as i64).saturating_add(exp);
        // Exponents this extreme leave exact arithmetic to f64 (0 or infinity).
        if point <= -400 || point >= 400 {
            let x: f64 = s.replace(',', "").parse().ok()?;
            if !x.is_finite() {
                return None;
            }
            return Some(if x == 0.0 {
                "0".to_string()
            } else {
                x.to_string()
            });
        }
        let len = digits_all.len() as i64;
        if point <= 0 {
            shifted_int.clear();
            fp = format!("{}{}", "0".repeat((-point) as usize), digits_all);
        } else if point >= len {
            shifted_int = format!("{}{}", digits_all, "0".repeat((point - len) as usize));
            fp.clear();
        } else {
            shifted_int = digits_all[..point as usize].to_string();
            fp = digits_all[point as usize..].to_string();
        }
        ip = &shifted_int;
    }
    let ip = ip.trim_start_matches('0');
    let fp = fp.trim_end_matches('0');
    if ip.is_empty() && fp.is_empty() {
        return Some("0".to_string());
    }
    let mut out = String::with_capacity(ip.len() + fp.len() + 2);
    if negative {
        out.push('-');
    }
    out.push_str(if ip.is_empty() { "0" } else { ip });
    if !fp.is_empty() {
        out.push('.');
        out.push_str(fp);
    }
    Some(out)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CellRules {
    pub ignore_case: bool,
    pub trim: bool,
    pub numbers: bool,
    /// Both tables carry the text marker (`xlsx`), so the numbers rule reads a marked number as
    /// the number it spells. [`compare`](super::compare::compare) sets it from the tables.
    pub marked: bool,
}

impl CellRules {
    pub fn is_plain(&self) -> bool {
        !self.ignore_case && !self.trim && !self.numbers
    }

    /// Numbers first (a number is its canonical text, a marked one too), then trim, then
    /// lowercase.
    pub fn normalize<'a>(&self, value: &'a str) -> Cow<'a, str> {
        if self.numbers {
            let bare = if self.marked { unmarked(value) } else { value };
            if let Some(n) = num_canon(bare) {
                return Cow::Owned(n);
            }
        }
        let value = if self.trim { value.trim() } else { value };
        if self.ignore_case {
            let lower = value.to_lowercase();
            if lower != value {
                return Cow::Owned(lower);
            }
        }
        Cow::Borrowed(value)
    }

    /// Raw equal, or equal once normalised.
    pub fn equal(&self, a: &str, b: &str) -> bool {
        a == b || (!self.is_plain() && self.normalize(a) == self.normalize(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_canonicalise_exactly() {
        for (raw, want) in [
            ("1.0", "1"),
            ("1,000", "1000"),
            ("1e3", "1000"),
            ("1.5e3", "1500"),
            ("-0", "0"),
            ("-0.0", "0"),
            ("+5", "5"),
            ("007", "7"),
            (".5", "0.5"),
            ("-0.50", "-0.5"),
            (" 42 ", "42"),
            ("1e-2", "0.01"),
            ("12.5e-1", "1.25"),
            ("1.", "1"),
            ("1e-401", "0"),
            ("12345678901234567890", "12345678901234567890"),
        ] {
            assert_eq!(num_canon(raw).as_deref(), Some(want), "{raw}");
        }
        for raw in [
            "",
            "abc",
            "1,00",
            "1.2.3",
            "e5",
            "-",
            "0x10",
            "1e400",
            &"9".repeat(65),
        ] {
            assert_eq!(num_canon(raw), None, "{raw}");
        }
        assert_ne!(
            num_canon("12345678901234567891"),
            num_canon("12345678901234567890")
        );
    }

    #[test]
    fn exponent_just_inside_the_limit_stays_exact() {
        // point = 1 - 400 = -399: still inside (-400, 400), so 399 zeros then the digit.
        let want = format!("0.{}1", "0".repeat(399));
        assert_eq!(num_canon("1e-400").as_deref(), Some(want.as_str()));
        assert_eq!(num_canon("1e-401").as_deref(), Some("0"));
    }

    #[test]
    fn absurd_exponents_take_the_f64_path_without_overflow() {
        assert_eq!(num_canon("1e9999999999999999999"), None);
        assert_eq!(num_canon("1e-9999999999999999999").as_deref(), Some("0"));
    }

    #[test]
    fn numbers_trim_and_case_rules_decide_cell_equality() {
        let plain = CellRules::default();
        assert!(!plain.equal("1,000", "1000"));
        assert!(
            CellRules {
                numbers: true,
                ..plain
            }
            .equal("1,000", "1000.00")
        );
        assert!(
            CellRules {
                trim: true,
                ..plain
            }
            .equal(" x ", "x")
        );
        assert!(
            CellRules {
                ignore_case: true,
                ..plain
            }
            .equal("Äb", "äB")
        );
        assert!(
            !CellRules {
                ignore_case: true,
                ..plain
            }
            .equal("1.0", "1"),
            "numbers only with the numbers rule"
        );
    }

    #[test]
    fn edge_shapes_follow_the_grammar() {
        for raw in ["1,000,000", "-1,234.50", "+.5e1", "0e5", "1E+2", "1e0"] {
            assert!(num_canon(raw).is_some(), "{raw}");
        }
        assert_eq!(num_canon("0e5").as_deref(), Some("0"));
        assert_eq!(num_canon("+.5e1").as_deref(), Some("5"));
        for raw in [
            "1,0000", ",1", "1234,567", ".", ".e5", "1e", "1e+", "-,5", "5-",
        ] {
            assert_eq!(num_canon(raw), None, "{raw}");
        }
        assert!(CellRules::default().is_plain());
        assert!(
            !CellRules {
                trim: true,
                ..Default::default()
            }
            .is_plain()
        );
    }
}
