//! Offset parsing and hex-dump line formatting.

/// Parses `0x1F40`, `1F40h` or plain decimal. Hex without a marker is not accepted. Spaces,
/// `_` and `,` are ignored, so the status line's `1,048,576` reads back as it shows.
pub fn parse_offset(text: &str) -> Option<u64> {
    let t: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != ',')
        .collect();
    let t = t.as_str();
    let radix_digits = |digits: &str, radix: u32| {
        if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
            return None;
        }
        u64::from_str_radix(digits, radix).ok()
    };
    if let Some(rest) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        radix_digits(rest, 16)
    } else if let Some(rest) = t.strip_suffix('h').or_else(|| t.strip_suffix('H')) {
        radix_digits(rest, 16)
    } else {
        radix_digits(t, 10)
    }
}

/// Uppercase hex, at least 8 digits, more when `max` needs them.
pub fn format_offset(offset: u64, max: u64) -> String {
    let needed = (u64::BITS - max.leading_zeros()).div_ceil(4) as usize;
    format!("{:0width$X}", offset, width = needed.max(8))
}

pub fn bytes_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 3);
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0xF)]));
    }
    out
}

/// `OFFSET  HEX...  ASCII`. A short row pads its hex column so the ASCII lines up.
pub fn hex_line(offset: u64, bytes: &[u8], width: usize, max: u64) -> String {
    // A row never holds more bytes than the column width.
    debug_assert!(bytes.len() <= width);
    let mut line = format_offset(offset, max);
    line.push_str("  ");
    line.push_str(&bytes_hex(bytes));
    for _ in bytes.len()..width {
        line.push_str("   ");
    }
    line.push_str("  ");
    line.extend(bytes.iter().map(|&b| {
        if (0x20..=0x7E).contains(&b) {
            b as char
        } else {
            '.'
        }
    }));
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goto_parses_hex_and_decimal_and_clamps() {
        assert_eq!(parse_offset("0x1F40"), Some(0x1F40));
        assert_eq!(parse_offset("1F40h"), Some(0x1F40));
        assert_eq!(parse_offset("1f40H"), Some(0x1F40));
        assert_eq!(parse_offset("0X10"), Some(0x10));
        assert_eq!(parse_offset(" 8000 "), Some(8000));
        assert_eq!(parse_offset("0xFFFFFFFF"), Some(0xFFFF_FFFF));
        assert_eq!(parse_offset("zz"), None);
        assert_eq!(parse_offset(""), None);
        assert_eq!(parse_offset("0x"), None);
        assert_eq!(parse_offset("h"), None);
        assert_eq!(parse_offset("1F40"), None);
        assert_eq!(parse_offset("0x1FFFFFFFFFFFFFFFF"), None);
        assert_eq!(parse_offset(&"9".repeat(30)), None);
    }

    #[test]
    fn goto_ignores_spaces_underscores_and_commas() {
        // The status line shows "Offset 0x00100000 (1,048,576)": its decimal is accepted.
        assert_eq!(parse_offset("1,048,576"), Some(1_048_576));
        assert_eq!(parse_offset(" 1 048 576 "), Some(1_048_576));
        assert_eq!(parse_offset("1_048_576"), Some(1_048_576));
        assert_eq!(parse_offset("0x0010_0000"), Some(0x10_0000));
        assert_eq!(parse_offset("0x 1F40"), Some(0x1F40));
        assert_eq!(parse_offset("1F 40h"), Some(0x1F40));
        // Bare hex stays refused (Task 1 ruling), separators or not.
        assert_eq!(parse_offset("1F40"), None);
        assert_eq!(parse_offset("1F,40"), None);
        assert_eq!(parse_offset(",_ "), None);
        assert_eq!(parse_offset("0x,"), None);
    }

    #[test]
    fn offsets_widen_past_four_gib() {
        assert_eq!(format_offset(0x1F40, 0x1000), "00001F40");
        assert_eq!(format_offset(0x1_0000_0000, 0x1_0000_0001), "100000000");
        assert_eq!(format_offset(u64::MAX, u64::MAX).len(), 16);
        assert_eq!(format_offset(0, 0), "00000000");
    }

    #[test]
    fn bytes_are_uppercase_hex_pairs() {
        assert_eq!(bytes_hex(b"MZ\x90"), "4D 5A 90");
        assert_eq!(bytes_hex(&[]), "");
    }

    #[test]
    fn every_byte_value_formats_as_two_uppercase_digits() {
        let all: Vec<u8> = (0..=255).collect();
        let expected: Vec<String> = all.iter().map(|b| format!("{b:02X}")).collect();
        assert_eq!(bytes_hex(&all), expected.join(" "));
    }

    #[test]
    fn a_hex_line_shows_offset_hex_and_ascii() {
        assert_eq!(
            hex_line(0x10, b"MZ\x90\x00", 4, 0x20),
            "00000010  4D 5A 90 00  MZ.."
        );
    }

    #[test]
    fn a_short_last_row_pads_so_ascii_lines_up() {
        assert_eq!(hex_line(0, b"AB", 4, 0x20), "00000000  41 42        AB");
    }
}
