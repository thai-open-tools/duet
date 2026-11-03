//! Find in the hex view: patterns and the searching of one side.

use std::sync::atomic::{AtomicU64, Ordering};

use memchr::memmem;

use crate::cancel::CancelToken;
use crate::hex::HexSource;

/// Bytes looked at per step, so a cancel lands within one chunk.
const CHUNK: u64 = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    Bytes(Vec<u8>),
    Text(String),
}

impl Pattern {
    pub fn needle(&self) -> &[u8] {
        match self {
            Pattern::Bytes(bytes) => bytes,
            Pattern::Text(text) => text.as_bytes(),
        }
    }
}

/// Hex input is pairs of hex digits, spaces optional, `0x` optional per byte.
pub fn parse_pattern(text: &str, hex: bool) -> Result<Pattern, String> {
    if !hex {
        return if text.is_empty() {
            Err("Type some text to find.".into())
        } else {
            Ok(Pattern::Text(text.to_string()))
        };
    }
    let bad = || "Type hex bytes, such as 4D 5A.".to_string();
    let mut bytes = Vec::new();
    // Spaces separate bytes only; a pair never contains one.
    for token in text.split_whitespace() {
        let mut rest = token;
        while !rest.is_empty() {
            if let Some(r) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
                rest = r;
            }
            let pair = rest.get(..2).ok_or_else(bad)?;
            if !pair.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(bad());
            }
            bytes.push(u8::from_str_radix(pair, 16).map_err(|_| bad())?);
            rest = &rest[2..];
        }
    }
    if bytes.is_empty() {
        return Err(bad());
    }
    Ok(Pattern::Bytes(bytes))
}

/// The next match of `needle` on one side, starting just after `from` (forward) or just before
/// it (backward), wrapping once; `wrapped` says it passed the end. None when absent or cancelled.
/// `done` counts the bytes looked at, for progress: at most about the file's length.
pub fn find(
    source: &HexSource,
    needle: &[u8],
    from: u64,
    dir: Direction,
    cancel: &CancelToken,
    done: &AtomicU64,
) -> Option<(u64, bool)> {
    if needle.is_empty() {
        return None;
    }
    let len = source.len();
    let finder = memmem::Finder::new(needle);
    let scan = |lo: u64, hi: u64, last: bool| scan(source, &finder, lo, hi, last, cancel, done);
    match dir {
        Direction::Forward => {
            let after = from.saturating_add(1).min(len);
            scan(after, len, false)
                .map(|at| (at, false))
                .or_else(|| scan(0, after, false).map(|at| (at, true)))
        }
        Direction::Backward => {
            let before = from.min(len);
            scan(0, before, true)
                .map(|at| (at, false))
                .or_else(|| scan(before, len, true).map(|at| (at, true)))
        }
    }
}

/// The first (or last) match that starts in `lo..hi`; None when absent or cancelled.
fn scan(
    source: &HexSource,
    finder: &memmem::Finder<'_>,
    lo: u64,
    hi: u64,
    last: bool,
    cancel: &CancelToken,
    done: &AtomicU64,
) -> Option<u64> {
    if !last {
        return scan_window(source, finder, lo, hi, false, cancel, done);
    }
    // Backward: windows from the top; the last match of the first window that has one.
    let mut top = hi;
    while top > lo {
        let bottom = top.saturating_sub(CHUNK).max(lo);
        if let Some(at) = scan_window(source, finder, bottom, top, true, cancel, done) {
            return Some(at);
        }
        top = bottom;
    }
    None
}

/// Walks `lo..hi` forward in chunks that overlap by `needle.len() - 1`, advancing by the length
/// `bytes` really returned (a source may serve less than asked); `done` grows by each step.
fn scan_window(
    source: &HexSource,
    finder: &memmem::Finder<'_>,
    lo: u64,
    hi: u64,
    last: bool,
    cancel: &CancelToken,
    done: &AtomicU64,
) -> Option<u64> {
    let n = finder.needle().len() as u64;
    let len = source.len();
    let mut pos = lo;
    let mut found = None;
    while pos < hi {
        if cancel.is_cancelled() {
            return None;
        }
        let end = (pos + CHUNK + n - 1).min(hi.saturating_add(n - 1)).min(len);
        let slice = source.bytes(pos..end);
        if (slice.len() as u64) < n {
            break;
        }
        // Overlapping matches count, so each search resumes one byte after the last start.
        let mut at = 0usize;
        while let Some(m) = finder.find(&slice[at..]) {
            let start = pos + (at + m) as u64;
            if start >= hi {
                break;
            }
            found = Some(start);
            if !last {
                return found;
            }
            at += m + 1;
        }
        let step = (slice.len() as u64 - (n - 1)).max(1);
        done.fetch_add(step, Ordering::Relaxed);
        pos += step;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_with(len: usize, marks: &[usize]) -> (tempfile::TempDir, HexSource) {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = vec![0u8; len];
        for &at in marks {
            bytes[at..at + 2].copy_from_slice(b"MZ");
        }
        let path = dir.path().join("f.bin");
        std::fs::write(&path, bytes).unwrap();
        let source = HexSource::open(&path).unwrap();
        (dir, source)
    }

    #[test]
    fn find_wraps_once_and_reports_it() {
        let (_dir, source) = source_with(1024, &[100, 900]);
        let go = CancelToken::new();
        let fwd = |from| {
            find(
                &source,
                b"MZ",
                from,
                Direction::Forward,
                &go,
                &AtomicU64::new(0),
            )
        };
        let back = |from| {
            find(
                &source,
                b"MZ",
                from,
                Direction::Backward,
                &go,
                &AtomicU64::new(0),
            )
        };
        assert_eq!(fwd(950), Some((100, true)));
        assert_eq!(fwd(0), Some((100, false)));
        assert_eq!(fwd(100), Some((900, false)));
        assert_eq!(back(50), Some((900, true)));
        assert_eq!(back(900), Some((100, false)));
        assert_eq!(
            find(
                &source,
                b"QQ",
                0,
                Direction::Forward,
                &go,
                &AtomicU64::new(0)
            ),
            None
        );
        assert_eq!(
            find(
                &source,
                b"QQ",
                0,
                Direction::Backward,
                &go,
                &AtomicU64::new(0)
            ),
            None
        );
        assert_eq!(
            find(&source, b"", 0, Direction::Forward, &go, &AtomicU64::new(0)),
            None
        );
        let stop = CancelToken::new();
        stop.cancel();
        assert_eq!(
            find(
                &source,
                b"MZ",
                0,
                Direction::Forward,
                &stop,
                &AtomicU64::new(0)
            ),
            None
        );
        assert_eq!(
            find(
                &source,
                b"MZ",
                0,
                Direction::Backward,
                &stop,
                &AtomicU64::new(0)
            ),
            None
        );
    }

    #[test]
    fn find_sees_a_lone_match_again_after_wrapping_onto_it() {
        let (_dir, source) = source_with(64, &[10]);
        let go = CancelToken::new();
        assert_eq!(
            find(
                &source,
                b"MZ",
                10,
                Direction::Forward,
                &go,
                &AtomicU64::new(0)
            ),
            Some((10, true))
        );
        assert_eq!(
            find(
                &source,
                b"MZ",
                10,
                Direction::Backward,
                &go,
                &AtomicU64::new(0)
            ),
            Some((10, true))
        );
    }

    #[test]
    fn find_crosses_chunk_borders_and_finds_overlaps() {
        let chunk = 1usize << 20;
        let (_dir, source) = source_with(3 << 20, &[chunk - 1, 2 * chunk + 5]);
        let go = CancelToken::new();
        assert_eq!(
            find(
                &source,
                b"MZ",
                0,
                Direction::Forward,
                &go,
                &AtomicU64::new(0)
            ),
            Some(((chunk - 1) as u64, false))
        );
        assert_eq!(
            find(
                &source,
                b"MZ",
                (3 << 20) - 1,
                Direction::Backward,
                &go,
                &AtomicU64::new(0)
            ),
            Some(((2 * chunk + 5) as u64, false))
        );
        assert_eq!(
            find(
                &source,
                b"MZ",
                (2 * chunk + 5) as u64,
                Direction::Backward,
                &go,
                &AtomicU64::new(0)
            ),
            Some(((chunk - 1) as u64, false))
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, b"xaaay").unwrap();
        let aaa = HexSource::open(&path).unwrap();
        assert_eq!(
            find(&aaa, b"aa", 1, Direction::Forward, &go, &AtomicU64::new(0)),
            Some((2, false))
        );
        assert_eq!(
            find(&aaa, b"aa", 3, Direction::Backward, &go, &AtomicU64::new(0)),
            Some((2, false))
        );
    }

    #[test]
    fn find_on_a_file_that_shrank_ends_instead_of_hanging() {
        let (dir, source) = source_with(3 << 20, &[100]);
        std::fs::write(dir.path().join("f.bin"), b"tiny").unwrap();
        let go = CancelToken::new();
        assert_eq!(
            find(
                &source,
                b"MZ",
                0,
                Direction::Forward,
                &go,
                &AtomicU64::new(0)
            ),
            None
        );
        assert_eq!(
            find(
                &source,
                b"MZ",
                0,
                Direction::Backward,
                &go,
                &AtomicU64::new(0)
            ),
            None
        );
    }

    #[test]
    fn find_advances_by_what_a_synthetic_source_returns() {
        let source = HexSource::synthetic(5 << 20, 7);
        let go = CancelToken::new();
        assert_eq!(
            find(
                &source,
                &[7, 7, 7],
                3 << 20,
                Direction::Forward,
                &go,
                &AtomicU64::new(0)
            ),
            Some(((3 << 20) + 1, false))
        );
        assert_eq!(
            find(
                &source,
                &[8],
                0,
                Direction::Forward,
                &go,
                &AtomicU64::new(0)
            ),
            None
        );
    }

    #[test]
    fn find_counts_the_bytes_it_looked_at() {
        let len = 3u64 << 20;
        let (_dir, source) = source_with(len as usize, &[]);
        let go = CancelToken::new();
        for dir in [Direction::Forward, Direction::Backward] {
            let done = AtomicU64::new(0);
            assert_eq!(find(&source, b"MZ", 5, dir, &go, &done), None);
            // The whole file once, over both legs of the wrap.
            let looked = done.load(Ordering::Relaxed);
            assert!(looked.abs_diff(len) <= 2, "{dir:?}: {looked}");
        }
    }

    #[test]
    fn hex_patterns_accept_spaces_and_0x() {
        assert_eq!(parse_pattern("4D 5A", true).unwrap().needle(), b"MZ");
        assert_eq!(parse_pattern("4d5a", true).unwrap().needle(), b"MZ");
        assert_eq!(parse_pattern("0x4d0x5a", true).unwrap().needle(), b"MZ");
        assert_eq!(parse_pattern("0x4D 0x5A", true).unwrap().needle(), b"MZ");
        assert!(parse_pattern("4D 5", true).is_err());
        assert!(parse_pattern("ZZ", true).is_err());
        assert!(parse_pattern("+A", true).is_err());
        assert!(parse_pattern("4D +5", true).is_err());
        assert!(parse_pattern("4 D", true).is_err());
        assert_eq!(
            parse_pattern("", true).unwrap_err(),
            "Type hex bytes, such as 4D 5A."
        );
        assert_eq!(
            parse_pattern("ทดสอบ", false).unwrap().needle(),
            "ทดสอบ".as_bytes()
        );
        assert_eq!(
            parse_pattern("", false).unwrap_err(),
            "Type some text to find."
        );
    }
}
