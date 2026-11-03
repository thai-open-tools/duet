//! Go to line: parse what was typed into a 0-based line of one side.

use crate::format::format_size;

/// "12" is line index 11 of a side with `count` lines. Past the end clamps to the last line;
/// thousands separators are allowed.
pub fn parse_line(text: &str, count: usize) -> Result<u32, String> {
    if count == 0 {
        return Err("That side has no lines.".into());
    }
    let message = || {
        format!(
            "Enter a line number from 1 to {}.",
            format_size(count as u64)
        )
    };
    let number: u64 = text
        .trim()
        .replace(',', "")
        .parse()
        .map_err(|_| message())?;
    if number == 0 {
        return Err(message());
    }
    Ok((number.min(count as u64) - 1) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_numbers_are_one_based_and_clamp_to_the_end() {
        assert_eq!(parse_line("12", 50), Ok(11));
        assert_eq!(parse_line(" 1,000 ", 5_000), Ok(999));
        assert_eq!(
            parse_line("999", 10),
            Ok(9),
            "past the end goes to the last line"
        );
        let message = Err("Enter a line number from 1 to 10.".to_owned());
        assert_eq!(parse_line("0", 10), message);
        assert_eq!(parse_line("abc", 10), message);
        assert_eq!(parse_line("", 10), message);
        assert_eq!(
            parse_line("3", 0),
            Err("That side has no lines.".to_owned())
        );
    }
}
