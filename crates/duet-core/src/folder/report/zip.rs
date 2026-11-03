//! A stored (uncompressed) ZIP writer, LocalDiff's `ldZipStore` (`sheet:115-146`): enough for
//! the Excel workbook, whose parts are small XML files. No ZIP64: an archive that would need
//! it (a part or an offset past 4 GiB, or 65,535 entries) is refused, never written.

use std::fmt;

use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};

#[cfg(test)]
mod tests;

/// The archive needs ZIP64, which this writer does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLarge;

impl fmt::Display for TooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("The workbook is too large for the Excel format (over 4 GiB)")
    }
}

impl std::error::Error for TooLarge {}

/// Whether entries of these (name, data) lengths fit a ZIP without ZIP64 (M-7): every size and
/// offset below 0xFFFFFFFF, fewer than 0xFFFF entries (both values are ZIP64's markers) and
/// names of at most 65,535 bytes. Checked in every build, on lengths alone.
pub fn fits(entries: impl IntoIterator<Item = (usize, usize)>) -> Result<(), TooLarge> {
    let limit = u64::from(u32::MAX);
    let (mut offset, mut central, mut count) = (0u64, 0u64, 0u64);
    for (name, data) in entries {
        let (name, data) = (name as u64, data as u64);
        if offset >= limit || data >= limit || name > u64::from(u16::MAX) {
            return Err(TooLarge);
        }
        offset += 30 + name + data;
        central += 46 + name;
        count += 1;
    }
    if offset >= limit || central >= limit || count >= u64::from(u16::MAX) {
        return Err(TooLarge);
    }
    Ok(())
}

/// The entries in the order given: stored, UTF-8 names (flag 0x0800), version 20, the DOS time
/// and date of `when`.
pub fn stored_zip(files: &[(&str, &[u8])], when: NaiveDateTime) -> Result<Vec<u8>, TooLarge> {
    fits(files.iter().map(|(name, data)| (name.len(), data.len())))?;
    let (time, date) = dos_time(when);
    let size: usize = files
        .iter()
        .map(|(name, data)| 30 + 46 + 2 * name.len() + data.len())
        .sum::<usize>()
        + 22;
    let mut out = Vec::with_capacity(size);
    let mut central = Vec::with_capacity(files.iter().map(|(name, _)| 46 + name.len()).sum());
    for (name, data) in files {
        let crc = crc32fast::hash(data);
        let offset = out.len() as u32;
        // `fits` above keeps every one of these casts in range.
        let (len, name_len) = (data.len() as u32, name.len() as u16);

        put32(&mut out, 0x0403_4b50);
        put16(&mut out, 20); // version needed
        put16(&mut out, 0x0800); // UTF-8 names
        put16(&mut out, 0); // stored
        put16(&mut out, time);
        put16(&mut out, date);
        put32(&mut out, crc);
        put32(&mut out, len);
        put32(&mut out, len);
        put16(&mut out, name_len);
        put16(&mut out, 0); // no extra field
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        put32(&mut central, 0x0201_4b50);
        put16(&mut central, 20); // made by (MS-DOS, 2.0)
        put16(&mut central, 20);
        put16(&mut central, 0x0800);
        put16(&mut central, 0);
        put16(&mut central, time);
        put16(&mut central, date);
        put32(&mut central, crc);
        put32(&mut central, len);
        put32(&mut central, len);
        put16(&mut central, name_len);
        // Extra field, comment, disk number, internal and external attributes.
        central.extend_from_slice(&[0; 12]);
        put32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }
    let (central_at, central_len) = (out.len() as u32, central.len() as u32);
    out.extend_from_slice(&central);
    put32(&mut out, 0x0605_4b50);
    put16(&mut out, 0); // this disk
    put16(&mut out, 0); // the disk the central directory starts on
    put16(&mut out, files.len() as u16);
    put16(&mut out, files.len() as u16);
    put32(&mut out, central_len);
    put32(&mut out, central_at);
    put16(&mut out, 0); // no comment
    Ok(out)
}

/// MS-DOS time (2-second steps) and date. Its years run from 1980 to 2107: a time outside
/// them is clamped, where LocalDiff's formula wraps (PF19).
fn dos_time(when: NaiveDateTime) -> (u16, u16) {
    let first = NaiveDate::from_ymd_opt(1980, 1, 1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .unwrap_or_default();
    let last = NaiveDate::from_ymd_opt(2107, 12, 31)
        .and_then(|d| d.and_hms_opt(23, 59, 58))
        .unwrap_or_default();
    let when = when.clamp(first, last);
    let time = (when.hour() << 11) | (when.minute() << 5) | (when.second() / 2);
    let date = (((when.year() - 1980) as u32) << 9) | (when.month() << 5) | when.day();
    (time as u16, date as u16)
}

fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
