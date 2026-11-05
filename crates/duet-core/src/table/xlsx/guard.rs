//! What a workbook's package may hold, checked before calamine sees it.
//!
//! Calamine reads all shared strings, the styles, the workbook and its relationships when it opens
//! a file, and reserves room for as many strings as the file says it has. A file of 1 KB that says
//! `uniqueCount="99999999999"` ends the whole process with an allocation failure, which no
//! `catch_unwind` can stop, and one giant `<v>` inside a sheet does the same. The cell budgets
//! cannot help: they count what was already read.
//!
//! So the zip's central directory is read first, and a package is refused past these limits:
//!
//! * more than 2 GiB unpacked in total, or 1 GiB in one part: past the cell and text budgets a
//!   table can hold, and small enough that one giant text node cannot exhaust the memory;
//! * more than 1000 to 1 on a part over 64 MiB: deflate tops out near 1032 to 1, so only a file made
//!   for it gets there at that size;
//! * a `<sst>` whose `uniqueCount` is above the part's size / 7: one string takes more than 7 bytes
//!   of XML. `count` is not held to this: it counts the cells that refer to a string, so a column
//!   of "Yes" and "No" is rightly far above it.
//!
//! A declared size can lie, but only downwards: the `zip` reader calamine reads through stops with
//! an error at a part's declared size (`a_part_cannot_unpack_past_what_it_declares` holds that to
//! the pinned version). A part declaring more than it holds costs nothing, except for the strings'
//! reservation, so their real size is what `uniqueCount` is held to.

use std::io::{Read, Seek};

use zip::ZipArchive;

use crate::cancel::CancelToken;

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

const MAX_TOTAL: u64 = 2 * GIB;
const MAX_PART: u64 = GIB;
const MAX_RATIO: u64 = 1000;
const RATIO_FROM: u64 = 64 * MIB;

/// How much of `xl/sharedStrings.xml` is looked at for its `<sst>` tag.
const SST_HEAD: usize = 16 * 1024;
/// The fewest bytes of XML a shared string takes.
const STRING_BYTES: u64 = 7;
/// The read size for the strings, no larger than calamine's: whatever calamine sees before a
/// part's declared size stops it, this saw first.
const CHUNK: usize = 4 * 1024;

/// Which limit a package went past. The wording is the app's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TooLarge {
    /// More than 2 GiB unpacked in total.
    Total,
    /// One part over 1 GiB unpacked.
    Part,
    /// A part over 64 MiB packed tighter than 1000 to 1.
    Packed,
    /// The shared strings say they hold more than the part's size allows.
    Strings,
}

/// Why the check stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refused {
    TooLarge(TooLarge),
    Cancelled,
}

/// Checks the package in `reader`. A file the zip crate cannot read as an archive passes: calamine
/// opens it with the same crate and says why it cannot.
pub(super) fn check<R: Read + Seek>(reader: R, cancel: &CancelToken) -> Result<(), Refused> {
    if cancel.is_cancelled() {
        return Err(Refused::Cancelled);
    }
    let Ok(mut zip) = ZipArchive::new(reader) else {
        return Ok(());
    };
    check_declared(&zip)?;
    for index in 0..zip.len() {
        let is_strings = zip.by_index_data(index).is_ok_and(|part| {
            part.name_raw()
                .to_ascii_lowercase()
                .ends_with(b"sharedstrings.xml")
        });
        if is_strings && let Ok(mut part) = zip.by_index(index) {
            let (head, size) = read_strings(&mut part, cancel)?;
            check_strings(&head, size)?;
        }
    }
    Ok(())
}

/// What the central directory says. The total first: it speaks for the file, a part only for
/// itself.
fn check_declared<R: Read + Seek>(zip: &ZipArchive<R>) -> Result<(), Refused> {
    let parts = || (0..zip.len()).filter_map(|index| zip.by_index_data(index).ok());
    if parts().fold(0u64, |sum, part| sum.saturating_add(part.size())) > MAX_TOTAL {
        return Err(Refused::TooLarge(TooLarge::Total));
    }
    for part in parts() {
        if part.size() > MAX_PART {
            return Err(Refused::TooLarge(TooLarge::Part));
        }
        if part.size() > RATIO_FROM && part.size() / part.compressed_size().max(1) > MAX_RATIO {
            return Err(Refused::TooLarge(TooLarge::Packed));
        }
    }
    Ok(())
}

/// The first bytes of the strings and their real size. A part that stops unpacking stops here too:
/// calamine, reading the same bytes, reports it.
fn read_strings(part: &mut impl Read, cancel: &CancelToken) -> Result<(Vec<u8>, u64), Refused> {
    let mut buffer = [0u8; CHUNK];
    let mut head = Vec::new();
    let mut size = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(Refused::Cancelled);
        }
        let got = match part.read(&mut buffer) {
            Ok(0) | Err(_) => return Ok((head, size)),
            Ok(got) => got,
        };
        let room = SST_HEAD - head.len();
        head.extend_from_slice(&buffer[..got.min(room)]);
        size += got as u64;
    }
}

/// `head` is the start of a shared string table of `size` bytes. Calamine reserves room for the
/// `uniqueCount` of its `<sst>`, and the process ends when it cannot, so that number has to be one
/// the size can hold. A tag this reading cannot find may be one calamine can, so it is refused,
/// unless all of the part was seen: then calamine finds no tag either, and calls the file damaged.
fn check_strings(head: &[u8], size: u64) -> Result<(), Refused> {
    let refused = Err(Refused::TooLarge(TooLarge::Strings));
    match sst_tag(head) {
        SstTag::Found(tag) if unique_count(tag) > size / STRING_BYTES => refused,
        SstTag::Found(_) => Ok(()),
        SstTag::Missing if head.len() as u64 == size => Ok(()),
        SstTag::Missing | SstTag::Hidden => refused,
    }
}

enum SstTag<'a> {
    /// The inside of the first `<sst ...>` tag.
    Found(&'a [u8]),
    /// No such tag in what was read.
    Missing,
    /// A document type came first: what is inside one is not plain elements.
    Hidden,
}

/// The first `sst` start tag in `head`, read as calamine's XML reader does: comments, processing
/// instructions, CDATA and other elements are passed over, and a tag ends at the first `>` outside
/// quotes.
fn sst_tag(head: &[u8]) -> SstTag<'_> {
    let mut at = 0;
    while let Some(open) = head[at..].iter().position(|&b| b == b'<') {
        let rest = &head[at + open..];
        let (end, is_element) = if rest.starts_with(b"<!--") {
            (find(rest, b"-->").map(|i| i + 3), false)
        } else if rest.starts_with(b"<![CDATA[") {
            (find(rest, b"]]>").map(|i| i + 3), false)
        } else if rest.starts_with(b"<?") {
            (find(rest, b"?>").map(|i| i + 2), false)
        } else if rest.starts_with(b"<!") {
            return SstTag::Hidden;
        } else if rest.starts_with(b"</") {
            (find(rest, b">").map(|i| i + 1), false)
        } else {
            (tag_end(rest).map(|i| i + 1), true)
        };
        let Some(end) = end else {
            return SstTag::Missing;
        };
        if is_element {
            let tag = &rest[1..end - 1];
            let name_len = tag
                .iter()
                .position(|b| b.is_ascii_whitespace() || *b == b'/')
                .unwrap_or(tag.len());
            let local = tag[..name_len].rsplit(|b| *b == b':').next().unwrap_or(tag);
            if local == b"sst" {
                return SstTag::Found(tag);
            }
        }
        at += open + end;
    }
    SstTag::Missing
}

fn find(text: &[u8], what: &[u8]) -> Option<usize> {
    text.windows(what.len()).position(|w| w == what)
}

/// Where the tag at the start of `text` ends: the first `>` outside quotes.
fn tag_end(text: &[u8]) -> Option<usize> {
    let mut quote = None;
    for (i, &b) in text.iter().enumerate() {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b'>' => return Some(i),
            None => {}
        }
    }
    None
}

/// The largest whole number any `uniqueCount` in `tag` is set to, or 0. Generous on purpose: spaces
/// around the `=` and either quote all count, and a number too long for `u64` is `u64::MAX`.
fn unique_count(tag: &[u8]) -> u64 {
    const KEY: &[u8] = b"uniqueCount";
    let skip_spaces = |mut i: usize| {
        while tag.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        i
    };
    let mut most = 0;
    for start in (0..tag.len()).filter(|&i| tag[i..].starts_with(KEY)) {
        let equals = skip_spaces(start + KEY.len());
        if tag.get(equals) != Some(&b'=') {
            continue;
        }
        let open = skip_spaces(equals + 1);
        let Some(&quote) = tag.get(open).filter(|q| **q == b'"' || **q == b'\'') else {
            continue;
        };
        let value = &tag[open + 1..];
        let Some(close) = value.iter().position(|&b| b == quote) else {
            continue;
        };
        let digits = &value[..close];
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            continue;
        }
        let number = digits.iter().fold(0u64, |n, &d| {
            n.saturating_mul(10).saturating_add(u64::from(d - b'0'))
        });
        most = most.max(number);
    }
    most
}

#[cfg(test)]
mod tests {
    use super::super::fixture::{declare_size, deflated_zip, zip};
    use super::*;
    use std::io::Cursor;

    fn run(bytes: &[u8]) -> Result<(), Refused> {
        check(Cursor::new(bytes), &CancelToken::new())
    }

    fn part(name: &str, data: &[u8]) -> (String, Vec<u8>) {
        (name.to_owned(), data.to_vec())
    }

    #[test]
    fn what_a_file_declares_is_held_to_the_limits() {
        for (declared, want) in [
            (0x7000_0000_u32, TooLarge::Part),
            ((100 * MIB) as u32, TooLarge::Packed),
        ] {
            let mut bytes = zip(&[part("a.xml", b"<a/>")]);
            declare_size(&mut bytes, "a.xml", declared);
            assert_eq!(run(&bytes), Err(Refused::TooLarge(want)), "{declared}");
        }
        // Three parts under the part limit, over the total. Each is also packed past the ratio, so
        // the total has to come first.
        let mut bytes = zip(&[part("a", b"x"), part("b", b"x"), part("c", b"x")]);
        for name in ["a", "b", "c"] {
            declare_size(&mut bytes, name, (GIB - 1) as u32);
        }
        assert_eq!(run(&bytes), Err(Refused::TooLarge(TooLarge::Total)));
        // A big part packed as a real one is: 1000 to 1 is the limit, not 64 MiB.
        let mut bytes = zip(&[part("a.xml", &vec![b'x'; 256 * 1024])]);
        declare_size(&mut bytes, "a.xml", (65 * MIB) as u32);
        assert_eq!(run(&bytes), Ok(()));
    }

    #[test]
    fn a_part_cannot_unpack_past_what_it_declares() {
        // The reader calamine reads through stops at the declared size, so a lie gets a part no
        // further than the limits above already allow.
        for bytes in [
            deflated_zip(&[part("a.xml", &vec![b'x'; 3 * MIB as usize])]),
            zip(&[part("a.xml", &vec![b'x'; 3 * MIB as usize])]),
        ] {
            let mut bytes = bytes;
            declare_size(&mut bytes, "a.xml", 100_000);
            assert_eq!(run(&bytes), Ok(()));
            let mut archive = ZipArchive::new(Cursor::new(&bytes[..])).unwrap();
            let mut file = archive.by_name("a.xml").unwrap();
            let mut read = 0;
            let mut buffer = [0u8; 8192];
            let error = loop {
                match file.read(&mut buffer) {
                    Ok(0) => panic!("the whole part came out, {read} bytes"),
                    Ok(got) => read += got,
                    Err(error) => break error,
                }
            };
            assert!(read <= 100_000, "{read}");
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{error}");
            assert_eq!(
                error.raw_os_error(),
                None,
                "the file's fault, not the system's"
            );
        }
    }

    #[test]
    fn a_stopped_check_says_so() {
        let bytes = zip(&[part("a.xml", b"<a/>")]);
        let cancel = CancelToken::new();
        cancel.cancel();
        assert_eq!(
            check(Cursor::new(&bytes[..]), &cancel),
            Err(Refused::Cancelled)
        );
        // In the strings too.
        let mut one_read = Cursor::new(vec![b'x'; CHUNK * 2]);
        assert_eq!(
            read_strings(&mut one_read, &cancel),
            Err(Refused::Cancelled)
        );
    }

    fn strings(xml: &str) -> Result<(), Refused> {
        run(&zip(&[part("xl/sharedStrings.xml", xml.as_bytes())]))
    }

    const TOO_MANY: Result<(), Refused> = Err(Refused::TooLarge(TooLarge::Strings));

    #[test]
    fn shared_strings_may_not_claim_more_than_their_size_holds() {
        assert_eq!(
            strings("<sst uniqueCount=\"99999999999\"><si><t>a</t></si></sst>"),
            TOO_MANY
        );
        assert_eq!(strings("<sst uniqueCount=\"9\"></sst>"), TOO_MANY);
        // The ways calamine's own reader would still see the attribute.
        for xml in [
            "<x:sst uniqueCount=\"99999999999\"/>",
            "<?xml version=\"1.0\"?><sst uniqueCount = '99999999999'></sst>",
            "<!-- <sst uniqueCount=\"1\"> --><sst uniqueCount=\"99999999999\"></sst>",
            "<other a=\">\"/><sst uniqueCount=\"99999999999999999999999\"></sst>",
        ] {
            assert_eq!(strings(xml), TOO_MANY, "{xml}");
        }
        // Whatever the part's name looks like.
        let bytes = zip(&[part("XL/SHAREDSTRINGS.XML", b"<sst uniqueCount=\"99\"/>")]);
        assert_eq!(run(&bytes), TOO_MANY);
    }

    #[test]
    fn the_size_that_counts_is_the_one_the_strings_really_have() {
        let xml = b"<sst uniqueCount=\"100000\"></sst>";
        let mut bytes = deflated_zip(&[part("xl/sharedStrings.xml", xml)]);
        // 10 MiB declared would make room for the claim, but the part holds a few bytes. (Packed
        // tighter than any limit looks at: it is under 64 MiB.)
        declare_size(&mut bytes, "xl/sharedStrings.xml", (10 * MIB) as u32);
        assert_eq!(run(&bytes), TOO_MANY);
    }

    #[test]
    fn a_count_of_references_may_be_far_above_the_size() {
        // One word used by a million cells: `count` is what Excel writes, and it is right.
        assert_eq!(
            strings("<sst count=\"1000000\" uniqueCount=\"1\"><si><t>Yes</t></si></sst>"),
            Ok(())
        );
    }

    #[test]
    fn shared_strings_that_say_nothing_are_for_calamine_to_judge() {
        assert_eq!(strings("<sst><si><t>a</t></si></sst>"), Ok(()));
        assert_eq!(strings("<notsst uniqueCount=\"99999999999\"/>"), Ok(()));
        assert_eq!(strings("<sst uniqueCount=\"many\"></sst>"), Ok(()));
    }

    #[test]
    fn a_tag_that_cannot_be_found_in_the_first_bytes_is_refused() {
        // Padding pushes the tag past what is read: nothing vouches for what calamine would see.
        let padded = format!(
            "<!-- {} --><sst uniqueCount=\"99999999999\"></sst>",
            "x".repeat(SST_HEAD)
        );
        assert_eq!(strings(&padded), TOO_MANY);
        // A document type can hide a tag from a reader that does not expand it.
        assert_eq!(
            strings("<!DOCTYPE a [<!-- > --><sst uniqueCount=\"1\">]><sst uniqueCount=\"999\"/>"),
            TOO_MANY
        );
    }

    #[test]
    fn a_file_that_is_not_an_archive_is_for_calamine_to_word() {
        assert_eq!(run(b"PK not really"), Ok(()));
    }

    #[test]
    fn a_healthy_package_passes() {
        let sheet = "<c>1</c>".repeat(10_000);
        let bytes = deflated_zip(&[part("xl/worksheets/sheet1.xml", sheet.as_bytes())]);
        assert_eq!(run(&bytes), Ok(()));
    }
}
