//! Display formatting for sizes and times.

use std::time::SystemTime;

use duet_core::text::encoding::TextEncoding;

/// Bytes with thousands separators, as in "1,234,567".
pub fn format_size(bytes: u64) -> String {
    let digits = bytes.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "64 B", "1.5 KiB", "4.0 GiB": the size at a glance. A value that rounds to 1024 moves up
/// a unit, so 1,048,575 bytes read "1.0 MiB".
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.;
    let mut unit = 0;
    while (value * 10.).round() >= 10240. && unit + 1 < UNITS.len() {
        value /= 1024.;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// A count and its noun, with thousands separators: "1 file", "0 files", "1,234 files". The one
/// place a count meets a noun, so the singular is never missed. Thai has no plural: `many` is the
/// key, whatever the count, and the digits stay Arabic.
pub fn count_of(n: u64, one: &'static str, many: &'static str) -> String {
    let noun = match crate::i18n::current() {
        crate::i18n::Lang::En if n == 1 => one,
        _ => crate::i18n::tr(many),
    };
    format!("{} {noun}", format_size(n))
}

/// The encoding as the status and the name tooltip show it: "UTF-8", "UTF-8 (BOM)".
pub fn encoding_label(encoding: &TextEncoding) -> String {
    let name = encoding.encoding.name();
    if encoding.bom {
        format!("{name} (BOM)")
    } else {
        name.to_owned()
    }
}

/// Local time as "YYYY-MM-DD HH:MM".
pub fn format_time(time: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(time)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn human_sizes_move_up_a_unit_when_they_round_to_1024() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1536), "1.5 KiB");
        assert_eq!(human_size(1_048_575), "1.0 MiB");
        assert_eq!(human_size(1_048_000), "1023.4 KiB");
        assert_eq!(human_size(4 << 30), "4.0 GiB");
    }

    #[test]
    fn sizes_get_thousands_separators() {
        assert_eq!(format_size(0), "0");
        assert_eq!(format_size(999), "999");
        assert_eq!(format_size(1_000), "1,000");
        assert_eq!(format_size(1_234_567), "1,234,567");
    }

    #[test]
    fn a_count_of_one_takes_the_singular() {
        assert_eq!(count_of(0, "file", "files"), "0 files");
        assert_eq!(count_of(1, "file", "files"), "1 file");
        assert_eq!(count_of(2, "file", "files"), "2 files");
        assert_eq!(count_of(1_000, "line", "lines"), "1,000 lines");
    }

    #[test]
    fn times_use_a_fixed_width_local_format() {
        let text = format_time(UNIX_EPOCH + Duration::from_secs(1_700_000_000));
        assert_eq!(text.len(), 16, "{text}");
        assert_eq!(&text[4..5], "-");
        assert_eq!(&text[10..11], " ");
        assert!(text.starts_with("2023-11-1"), "{text}");
    }
}
