//! Encoding detection, decoding and re-encoding for text files.

use std::borrow::Cow;

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};

/// Leading bytes inspected for NUL when deciding a file is binary.
pub const BINARY_SNIFF_LEN: usize = 8 * 1024;
/// Bytes fed to the legacy-encoding detector.
const DETECT_SAMPLE_LEN: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextEncoding {
    pub encoding: &'static Encoding,
    pub bom: bool,
}

#[derive(Debug)]
pub struct Decoded<'a> {
    pub text: Cow<'a, str>,
    pub encoding: TextEncoding,
    /// True when some bytes could not be decoded and became U+FFFD.
    pub lossy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EncodeError {
    #[error("text contains a character that {encoding} cannot represent (byte offset {position})")]
    Unrepresentable {
        encoding: &'static str,
        position: usize,
    },
}

pub fn is_binary(bytes: &[u8]) -> bool {
    if Encoding::for_bom(bytes).is_some() {
        return false;
    }
    bytes[..bytes.len().min(BINARY_SNIFF_LEN)].contains(&0)
}

pub fn detect(bytes: &[u8]) -> &'static Encoding {
    if let Some((encoding, _)) = Encoding::for_bom(bytes) {
        return encoding;
    }
    if std::str::from_utf8(bytes).is_ok() {
        return UTF_8;
    }
    let sample = &bytes[..bytes.len().min(DETECT_SAMPLE_LEN)];
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
    detector.feed(sample, true);
    detector.guess(None, Utf8Detection::Deny)
}

pub fn decode(bytes: &[u8]) -> Decoded<'_> {
    decode_as(bytes, detect(bytes))
}

pub fn decode_as<'a>(bytes: &'a [u8], encoding: &'static Encoding) -> Decoded<'a> {
    let (body, bom) = match Encoding::for_bom(bytes) {
        Some((found, len)) if found == encoding => (&bytes[len..], true),
        _ => (bytes, false),
    };
    let (text, lossy) = encoding.decode_without_bom_handling(body);
    Decoded {
        text,
        encoding: TextEncoding { encoding, bom },
        lossy,
    }
}

pub fn encode(text: &str, target: TextEncoding) -> Result<Vec<u8>, EncodeError> {
    let encoding = target.encoding;
    if encoding == UTF_16LE || encoding == UTF_16BE {
        // encoding_rs never encodes to UTF-16 (it maps it to UTF-8), so do it here.
        let little = encoding == UTF_16LE;
        let mut out = Vec::with_capacity(text.len() * 2 + 2);
        if target.bom {
            out.extend_from_slice(if little { &[0xFF, 0xFE] } else { &[0xFE, 0xFF] });
        }
        for unit in text.encode_utf16() {
            out.extend_from_slice(&if little {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        return Ok(out);
    }
    if encoding == UTF_8 {
        let mut out = Vec::with_capacity(text.len() + 3);
        if target.bom {
            out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        out.extend_from_slice(text.as_bytes());
        return Ok(out);
    }
    let (bytes, _, had_errors) = encoding.encode(text);
    if had_errors {
        return Err(EncodeError::Unrepresentable {
            encoding: encoding.name(),
            position: first_unrepresentable(text, encoding),
        });
    }
    Ok(bytes.into_owned())
}

fn first_unrepresentable(text: &str, encoding: &'static Encoding) -> usize {
    let mut buf = [0u8; 4];
    text.char_indices()
        .find(|(_, c)| encoding.encode(c.encode_utf8(&mut buf)).2)
        .map_or(0, |(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::WINDOWS_874;

    const THAI: &str = "ภาษาไทยเป็นภาษาราชการของประเทศไทย สวัสดีครับ ขอบคุณมาก ";

    fn enc(encoding: &'static Encoding, bom: bool) -> TextEncoding {
        TextEncoding { encoding, bom }
    }

    #[test]
    fn plain_utf8_is_detected_and_borrowed() {
        let d = decode(THAI.as_bytes());
        assert_eq!(d.encoding, enc(UTF_8, false));
        assert!(matches!(d.text, Cow::Borrowed(_)));
        assert_eq!(d.text, THAI);
        assert!(!d.lossy);
    }

    #[test]
    fn utf8_bom_is_stripped_and_remembered() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"abc");
        let d = decode(&bytes);
        assert_eq!(d.encoding, enc(UTF_8, true));
        assert_eq!(d.text, "abc");
    }

    #[test]
    fn utf16_with_bom_both_byte_orders() {
        let le = encode("ก1", enc(UTF_16LE, true)).unwrap();
        assert_eq!(le, vec![0xFF, 0xFE, 0x01, 0x0E, 0x31, 0x00]);
        let d = decode(&le);
        assert_eq!(d.text, "ก1");
        assert_eq!(d.encoding, enc(UTF_16LE, true));

        let be = encode("ก1", enc(UTF_16BE, true)).unwrap();
        assert_eq!(be, vec![0xFE, 0xFF, 0x0E, 0x01, 0x00, 0x31]);
        assert_eq!(decode(&be).text, "ก1");
    }

    #[test]
    fn tis620_thai_is_detected_as_windows_874() {
        let text = THAI.repeat(4);
        let (bytes, _, had_errors) = WINDOWS_874.encode(&text);
        assert!(!had_errors);
        let d = decode(&bytes);
        assert_eq!(d.encoding.encoding, WINDOWS_874);
        assert_eq!(d.text, text);
        assert!(!d.lossy);
    }

    #[test]
    fn forcing_utf8_on_invalid_bytes_is_lossy() {
        let d = decode_as(b"abc\xFFdef", UTF_8);
        assert!(d.lossy);
        assert_eq!(d.text, "abc\u{FFFD}def");
    }

    #[test]
    fn binary_sniffing() {
        assert!(is_binary(b"abc\0def"));
        assert!(!is_binary(b"plain text"));
        assert!(!is_binary(&[0xFF, 0xFE, 0x41, 0x00]));
    }

    #[test]
    fn roundtrip_each_supported_target() {
        for target in [
            enc(UTF_8, false),
            enc(UTF_8, true),
            enc(UTF_16LE, true),
            enc(UTF_16BE, true),
            enc(WINDOWS_874, false),
        ] {
            let bytes = encode(THAI, target).unwrap();
            let d = decode_as(&bytes, target.encoding);
            assert_eq!(d.text, THAI, "{target:?}");
            assert_eq!(d.encoding, target);
        }
    }

    #[test]
    fn unrepresentable_character_reports_position() {
        let err = encode("ab😀", enc(WINDOWS_874, false)).unwrap_err();
        assert_eq!(
            err,
            EncodeError::Unrepresentable {
                encoding: "windows-874",
                position: 2
            }
        );
    }
}
