pub mod diff;
pub mod find;
pub mod format;

use std::fs::File;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Bytes per cached block (LocalDiff's size).
const BLOCK: u64 = 64 << 10;
/// Blocks kept per source: 4 MiB.
const CACHE_BLOCKS: usize = 64;
/// The most one call serves from a synthetic source; far above any window a caller asks for.
const SYNTHETIC_MAX: u64 = 64 << 20;

/// Positioned read. On Windows `seek_read` also moves the handle's cursor, and reads through
/// one synchronous handle queue behind each other. Neither matters here: no handle is shared
/// or kept, because `read_span` opens one for each read and drops it.
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    #[cfg(windows)]
    {
        std::os::windows::fs::FileExt::seek_read(file, buf, offset)
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::FileExt::read_at(file, buf, offset)
    }
}

/// Up to `len` bytes from `offset` of the file at `path`, through a handle opened for this read
/// and closed before it returns, so an open tab never keeps the file or its folder busy. Fewer
/// bytes where the file ends (or the file can't be opened, or a read fails, which reads as the
/// end: F5 opens the file again).
fn read_span(path: &Path, offset: u64, len: usize) -> Vec<u8> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let mut out = vec![0u8; len];
    let mut filled = 0;
    while filled < len {
        match read_at(&file, &mut out[filled..], offset + filled as u64) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    out.truncate(filled);
    out
}

/// The recently read blocks of a file, least recently used first. No handle is kept: each
/// miss opens the file, reads the block and closes it again.
struct FileBacking {
    cache: Mutex<Vec<(u64, Arc<Vec<u8>>)>>,
}

impl FileBacking {
    fn block(&self, path: &Path, index: u64) -> Arc<Vec<u8>> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = cache.iter().position(|(i, _)| *i == index) {
            let hit = cache.remove(at);
            let block = hit.1.clone();
            cache.push(hit);
            return block;
        }
        drop(cache);
        // Read outside the lock; two threads asking for one block just read it twice.
        let block = Arc::new(read_span(path, index * BLOCK, BLOCK as usize));
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= CACHE_BLOCKS {
            cache.remove(0);
        }
        cache.push((index, block.clone()));
        block
    }

    fn read(&self, path: &Path, range: Range<u64>) -> Vec<u8> {
        let want = range.end - range.start;
        // A big span (a diff chunk, a find window) would only churn the cache. Each such
        // chunk opens the file for itself: a compare or find holds no handle between chunks.
        if want >= 2 * BLOCK {
            return read_span(path, range.start, want as usize);
        }
        let mut out = Vec::with_capacity(want as usize);
        for index in range.start / BLOCK..=(range.end - 1) / BLOCK {
            let block = self.block(path, index);
            let base = index * BLOCK;
            let from = range.start.saturating_sub(base) as usize;
            let to = ((range.end - base).min(BLOCK) as usize).min(block.len());
            if from >= to {
                break;
            }
            out.extend_from_slice(&block[from..to]);
            // A short block is the end of the file.
            if block.len() < BLOCK as usize {
                break;
            }
        }
        out
    }
}

enum Backing {
    /// An empty file reads nothing.
    Empty,
    /// Read in blocks, each through a handle opened for that read and closed after it, never
    /// mapped: while a tab shows the file, other programs can rewrite, replace, delete or
    /// exclusively open it, and rename its folder.
    File(FileBacking),
    /// Test helper: `len` virtual bytes, all `fill`.
    Synthetic(u8),
}

/// Bytes of one side, read in blocks from the file on disk; no handle is kept between reads.
pub struct HexSource {
    path: PathBuf,
    backing: Backing,
    len: u64,
}

impl HexSource {
    /// Checks that the file opens and takes its length; that handle is closed again at once.
    pub fn open(path: &Path) -> io::Result<HexSource> {
        let len = File::open(path)?.metadata()?.len();
        let backing = if len == 0 {
            Backing::Empty
        } else {
            Backing::File(FileBacking {
                cache: Mutex::new(Vec::new()),
            })
        };
        Ok(HexSource {
            path: path.to_path_buf(),
            backing,
            len,
        })
    }

    /// A virtual file of `len` bytes of `fill`, holding no file. A call serves at most
    /// 64 MiB, so a 4 GiB source costs nothing.
    #[doc(hidden)]
    pub fn synthetic(len: u64, fill: u8) -> HexSource {
        HexSource {
            path: PathBuf::new(),
            backing: Backing::Synthetic(fill),
            len,
        }
    }

    /// The length when the file was opened.
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The bytes in `range` that exist (clamped to the file); empty past the end. If the file
    /// shrank since it was opened, the missing tail reads as the end, so a call can return
    /// fewer bytes than asked: callers loop on the length they got. Synthetic sources serve
    /// at most 64 MiB per call.
    pub fn bytes(&self, range: Range<u64>) -> Vec<u8> {
        let end = range.end.min(self.len);
        if range.start >= end {
            return Vec::new();
        }
        match &self.backing {
            Backing::Empty => Vec::new(),
            Backing::File(file) => file.read(&self.path, range.start..end),
            Backing::Synthetic(fill) => {
                vec![*fill; (end - range.start).min(SYNTHETIC_MAX) as usize]
            }
        }
    }
}

pub struct HexPair {
    pub left: HexSource,
    pub right: HexSource,
}

impl HexPair {
    pub fn open(left: &Path, right: &Path) -> io::Result<HexPair> {
        Ok(HexPair {
            left: HexSource::open(left)?,
            right: HexSource::open(right)?,
        })
    }

    pub fn max_len(&self) -> u64 {
        self.left.len().max(self.right.len())
    }

    pub fn common_len(&self) -> u64 {
        self.left.len().min(self.right.len())
    }

    pub fn swap(&mut self) {
        std::mem::swap(&mut self.left, &mut self.right);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterned(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 7 + i / 251) as u8).collect()
    }

    #[test]
    fn an_open_source_does_not_lock_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, patterned(300_000)).unwrap();
        let source = HexSource::open(&path).unwrap();
        assert_eq!(source.bytes(0..4), patterned(4));
        // Another program may rewrite, replace or delete the file while it is open here.
        std::fs::write(&path, b"short").unwrap();
        let other = dir.path().join("b.bin");
        std::fs::write(&other, b"x").unwrap();
        std::fs::rename(&other, &path).unwrap();
        std::fs::remove_file(&path).unwrap();
    }

    /// No handle outlives a read: while a source is open, its folder can be renamed and an
    /// exclusive (share-none) writer can open the file, as with no tab open at all.
    #[cfg(windows)]
    #[test]
    fn an_open_source_keeps_no_handle_between_reads() {
        use std::os::windows::fs::OpenOptionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        std::fs::create_dir(&out).unwrap();
        let path = out.join("a.bin");
        std::fs::write(&path, patterned(300_000)).unwrap();
        let source = HexSource::open(&path).unwrap();
        // A cached block (the rows) and a big span (a compare or find chunk).
        assert_eq!(source.bytes(0..4), patterned(4));
        assert_eq!(source.bytes(0..200_000), patterned(200_000));
        let exclusive = std::fs::OpenOptions::new()
            .write(true)
            .share_mode(0)
            .open(&path);
        assert!(exclusive.is_ok(), "an exclusive open failed: {exclusive:?}");
        drop(exclusive);
        std::fs::rename(&out, dir.path().join("out2")).expect("the folder renames");
        // The cached block still serves; a block never read now reads as the end.
        assert_eq!(source.bytes(0..4), patterned(4));
        assert!(source.bytes(250_000..250_010).is_empty());
    }

    #[test]
    fn a_file_that_shrank_reads_as_ended() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, patterned(300_000)).unwrap();
        let source = HexSource::open(&path).unwrap();
        std::fs::write(&path, b"0123456789").unwrap();
        assert_eq!(source.len(), 300_000, "the length is from open time");
        assert_eq!(source.bytes(2..6), b"2345");
        assert_eq!(source.bytes(5..20), b"56789", "fewer bytes, not an error");
        assert!(source.bytes(100_000..100_100).is_empty());
        assert!(source.bytes(0..300_000).len() <= 10);
    }

    #[test]
    fn reads_match_the_file_across_block_seams_and_big_spans() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        let data = patterned(500_000);
        std::fs::write(&path, &data).unwrap();
        let source = HexSource::open(&path).unwrap();
        for (a, b) in [
            (0usize, 16),
            (65_530, 65_545),
            (65_536, 65_537),
            (131_070, 131_075),
            (10, 400_000),
            (499_990, 500_000),
            (499_990, 600_000),
        ] {
            let end = b.min(data.len());
            assert_eq!(source.bytes(a as u64..b as u64), &data[a..end], "{a}..{b}");
        }
        // Many distinct blocks, then the first again: the cache evicts but never lies.
        for block in 0..8 {
            let at = block * 65_536 + 3;
            assert_eq!(source.bytes(at as u64..at as u64 + 9), &data[at..at + 9]);
        }
        assert_eq!(source.bytes(3..12), &data[3..12]);
    }
}
