use super::*;
use crate::folder::report::test_data::read_zip;
use chrono::NaiveDate;

fn at(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(y, m, d)
        .unwrap()
        .and_hms_opt(h, min, s)
        .unwrap()
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

#[test]
fn crc_and_stored_zip_follow_the_format() {
    assert_eq!(crc32fast::hash(b"123456789"), 0xCBF4_3926);
    let zip = stored_zip(&[("a.txt", b"hi")], at(2026, 10, 23, 10, 15, 30)).unwrap();
    // 30 + 5 + 2 local, 46 + 5 central, 22 end.
    assert_eq!(zip.len(), 110);
    let (time, date) = (
        (10 << 11) | (15 << 5) | 15,
        ((2026 - 1980) << 9) | (10 << 5) | 23,
    );
    assert_eq!((time, date), (20975, 23895));
    let crc = crc32fast::hash(b"hi");

    assert_eq!(&zip[..4], b"PK\x03\x04");
    assert_eq!(u16_at(&zip, 4), 20, "version needed");
    assert_eq!(u16_at(&zip, 6), 0x0800, "UTF-8 names");
    assert_eq!(u16_at(&zip, 8), 0, "stored");
    assert_eq!((u16_at(&zip, 10), u16_at(&zip, 12)), (time, date));
    assert_eq!(u32_at(&zip, 14), crc);
    assert_eq!((u32_at(&zip, 18), u32_at(&zip, 22)), (2, 2));
    assert_eq!((u16_at(&zip, 26), u16_at(&zip, 28)), (5, 0));
    assert_eq!(&zip[30..37], b"a.txthi");

    let c = 37;
    assert_eq!(&zip[c..c + 4], b"PK\x01\x02");
    assert_eq!((u16_at(&zip, c + 4), u16_at(&zip, c + 6)), (20, 20));
    assert_eq!((u16_at(&zip, c + 8), u16_at(&zip, c + 10)), (0x0800, 0));
    assert_eq!((u16_at(&zip, c + 12), u16_at(&zip, c + 14)), (time, date));
    assert_eq!(u32_at(&zip, c + 16), crc);
    assert_eq!((u32_at(&zip, c + 20), u32_at(&zip, c + 24)), (2, 2));
    assert_eq!(u16_at(&zip, c + 28), 5);
    assert_eq!(u32_at(&zip, c + 42), 0, "the local header's offset");
    assert_eq!(&zip[c + 46..c + 51], b"a.txt");

    let e = 88;
    assert_eq!(&zip[e..e + 4], b"PK\x05\x06");
    assert_eq!((u16_at(&zip, e + 8), u16_at(&zip, e + 10)), (1, 1));
    assert_eq!(u32_at(&zip, e + 12), 51, "central directory size");
    assert_eq!(u32_at(&zip, e + 16), 37, "central directory offset");
    assert_eq!(u16_at(&zip, e + 20), 0, "no comment");

    assert_eq!(read_zip(&zip), [("a.txt".to_owned(), b"hi".to_vec())]);
}

#[test]
fn entries_follow_each_other_with_their_offsets() {
    let big = vec![b'x'; 1000];
    let zip = stored_zip(
        &[
            ("xl/workbook.xml", b"<a/>"),
            ("ไทย.txt", &big),
            ("empty", b""),
        ],
        at(2026, 1, 2, 3, 4, 5),
    )
    .unwrap();
    let entries = read_zip(&zip);
    let names: Vec<&str> = entries.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["xl/workbook.xml", "ไทย.txt", "empty"]);
    assert_eq!(entries[1].1, big);
    assert!(entries[2].1.is_empty());
    // The second local header starts after the first entry's header, name and data.
    let second = 30 + "xl/workbook.xml".len() + 4;
    assert_eq!(&zip[second..second + 4], b"PK\x03\x04");
    assert_eq!(
        u16_at(&zip, second + 10),
        (3 << 11) | (4 << 5) | 2,
        "seconds / 2"
    );
}

#[test]
fn dos_dates_stay_inside_their_range() {
    // DOS dates start in 1980 and keep the year in 7 bits: 1980 to 2107.
    let early = stored_zip(&[("a", b"")], at(1970, 6, 15, 12, 0, 0)).unwrap();
    assert_eq!((u16_at(&early, 10), u16_at(&early, 12)), (0, (1 << 5) | 1));
    let late = stored_zip(&[("a", b"")], at(2150, 6, 15, 12, 0, 0)).unwrap();
    assert_eq!(
        (u16_at(&late, 10), u16_at(&late, 12)),
        ((23 << 11) | (59 << 5) | 29, (127 << 9) | (12 << 5) | 31)
    );
}

/// M-7: an archive that needs ZIP64 is refused in every build, never written with wrapped
/// sizes. Checked on lengths alone: nothing near 4 GiB is allocated.
#[test]
fn an_archive_that_needs_zip64_is_refused() {
    const GIB: usize = 1 << 30;
    let limit = u32::MAX as usize;
    assert_eq!(fits([(10, 100), (20, 3 * GIB)]), Ok(()));
    // A part of 4 GiB or more (0xFFFFFFFF itself is ZIP64's marker).
    assert_eq!(fits([(10, limit)]), Err(TooLarge));
    assert_eq!(fits([(10, 5 * GIB)]), Err(TooLarge));
    // Parts that each fit, but the central directory would start past 4 GiB.
    assert_eq!(fits([(10, 2 * GIB), (10, 2 * GIB)]), Err(TooLarge));
    // A local header that starts past 4 GiB.
    assert_eq!(fits([(1, 3 * GIB), (1, 3 * GIB), (1, 0)]), Err(TooLarge));
    // 65,535 entries is ZIP64's marker too; 65,534 fit.
    assert_eq!(fits(std::iter::repeat_n((1, 0), 65_534)), Ok(()));
    assert_eq!(fits(std::iter::repeat_n((1, 0), 65_535)), Err(TooLarge));
    // A name longer than 65,535 bytes.
    assert_eq!(fits([(65_536, 0)]), Err(TooLarge));
    assert_eq!(
        TooLarge.to_string(),
        "The workbook is too large for the Excel format (over 4 GiB)"
    );
    // The writer checks before it writes anything.
    let name = "n".repeat(65_536);
    assert_eq!(
        stored_zip(&[(name.as_str(), b"")], at(2026, 1, 2, 3, 4, 5)),
        Err(TooLarge)
    );
}
