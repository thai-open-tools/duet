//! Deterministic benchmark datasets for the spec's performance budget.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use rayon::prelude::*;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// left/ and right/ trees of small files; every Nth right file grows by one line.
    Tree {
        out: PathBuf,
        #[arg(long, default_value_t = 100_000)]
        files: u32,
        #[arg(long, default_value_t = 1.0)]
        change_pct: f64,
    },
    /// left.txt and right.txt of unique lines; every Nth right line is replaced. With long-pct,
    /// about that share of the lines (seeded) run on to 150 to 400 characters, so they wrap.
    Text {
        out: PathBuf,
        #[arg(long, default_value_t = 1024)]
        size_mb: u64,
        #[arg(long, default_value_t = 1.0)]
        change_pct: f64,
        #[arg(long, default_value_t = 0.0)]
        long_pct: f64,
    },
    /// left.bin and right.bin of seeded pseudo-random bytes, streamed in 1 MiB chunks; about
    /// change-pct percent of the 4 KiB blocks of right.bin differ by one byte.
    Binary {
        out: PathBuf,
        #[arg(long, default_value_t = 4096)]
        size_mb: u64,
        #[arg(long, default_value_t = 0.01)]
        change_pct: f64,
    },
    /// left.csv and right.csv: a header `id,c2,..,cN`, unique integer ids, about 1 cell in 50
    /// quoted with a comma. The right file changes one cell in change-pct percent of the rows,
    /// moves 1% of the rows, drops 0.1% and adds 0.1% new ids. Seeded, so reproducible.
    Table {
        out: PathBuf,
        #[arg(long, default_value_t = 60_000)]
        rows: u64,
        #[arg(long, default_value_t = 20)]
        cols: u64,
        #[arg(long, default_value_t = 2.0)]
        change_pct: f64,
    },
    /// left.png and right.png, size x size, seeded: a gradient with noise. The right changes
    /// round(cells x change-pct / 100) of the 16 px cells, picked by a seeded shuffle, by +40 on
    /// red (wrapping); one changed cell in every 1,000 gets +2 instead (inside a typical tolerance). The +2
    /// branch never triggers at the bench default (655 changed cells).
    Picture {
        out: PathBuf,
        #[arg(long, default_value_t = 4096)]
        size: u32,
        #[arg(long, default_value_t = 1.0)]
        change_pct: f64,
    },
    /// base.txt, left.txt and right.txt of unique lines for the 3-way merge. A seeded shuffle picks
    /// disjoint line sets: round(lines x conflict-pct / 100) lines changed on both sides
    /// differently (" L" on the left, " R" on the right), then round(lines x change-pct / 100)
    /// changed on the left only (" L") and as many on the right only (" R").
    Merge {
        out: PathBuf,
        #[arg(long, default_value_t = 12_000)]
        lines: u64,
        #[arg(long, default_value_t = 5.0)]
        change_pct: f64,
        #[arg(long, default_value_t = 1.0)]
        conflict_pct: f64,
    },
    /// Reads every file once in parallel: the baseline throughput for content compare.
    Readall {
        dirs: Vec<PathBuf>,
        #[arg(long, default_value_t = 0)]
        threads: usize,
    },
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

fn every(change_pct: f64) -> u64 {
    (100.0 / change_pct).round().max(1.0) as u64
}

fn write_with_mtime(path: &Path, bytes: &[u8], mtime: SystemTime) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    File::options().write(true).open(path)?.set_modified(mtime)
}

fn tree(out: &Path, files: u32, change_pct: f64) -> io::Result<()> {
    let stamp = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let step = every(change_pct);
    (0..files).into_par_iter().try_for_each(|i| {
        let rel = format!("d{:04}/f{:06}.txt", i / 100, i);
        let body = format!("file {i} {}\n", "x".repeat(150));
        write_with_mtime(&out.join("left").join(&rel), body.as_bytes(), stamp)?;
        let right = if u64::from(i) % step == 0 {
            format!("{body}changed\n")
        } else {
            body
        };
        write_with_mtime(&out.join("right").join(&rel), right.as_bytes(), stamp)
    })
}

/// What a long text line runs on with, repeated as far as it needs.
const FILLER: &str = " consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore";

fn text(out: &Path, size_mb: u64, change_pct: f64, long_pct: f64) -> io::Result<()> {
    fs::create_dir_all(out)?;
    let target = size_mb * 1024 * 1024;
    let step = every(change_pct);
    let mut left = BufWriter::with_capacity(1 << 20, File::create(out.join("left.txt"))?);
    let mut right = BufWriter::with_capacity(1 << 20, File::create(out.join("right.txt"))?);
    let mut rng = Lcg(42);
    // Its own generator, so the default lines stay what they always were.
    let mut long = Lcg(7);
    let (mut written, mut n) = (0u64, 0u64);
    while written < target {
        let mut line = format!(
            "{n:010} {:016x} {:016x} lorem ipsum dolor sit amet",
            rng.next(),
            rng.next()
        );
        if long_pct > 0.0 && (long.next() % 10_000) as f64 / 100.0 < long_pct {
            let width = 150 + (long.next() % 251) as usize;
            line.extend(FILLER.chars().cycle().take(width - line.len()));
        }
        line.push('\n');
        left.write_all(line.as_bytes())?;
        if n % step == 0 {
            right.write_all(format!("{n:010} CHANGED\n").as_bytes())?;
        } else {
            right.write_all(line.as_bytes())?;
        }
        written += line.len() as u64;
        n += 1;
    }
    left.flush()?;
    right.flush()
}

// Not Lcg: its low bits are weak, and the block pick takes `% BLOCK`.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

const CHUNK: usize = 1 << 20;
const BLOCK: usize = 4096;

/// Streams the pair chunk by chunk and returns how many 4 KiB blocks of right.bin were changed.
fn binary(out: &Path, size_mb: u64, change_pct: f64) -> io::Result<u64> {
    fs::create_dir_all(out)?;
    let mut left = BufWriter::with_capacity(CHUNK, File::create(out.join("left.bin"))?);
    let mut right = BufWriter::with_capacity(CHUNK, File::create(out.join("right.bin"))?);
    let mut data = XorShift(0x9E37_79B9_7F4A_7C15);
    let mut picks = XorShift(0xD1B5_4A32_D192_ED03);
    let threshold = (change_pct / 100.0 * u64::MAX as f64) as u64;
    let mut buf = vec![0u8; CHUNK];
    let mut changed = 0;
    for _ in 0..size_mb {
        for word in buf.chunks_exact_mut(8) {
            word.copy_from_slice(&data.next().to_le_bytes());
        }
        left.write_all(&buf)?;
        for block in buf.chunks_exact_mut(BLOCK) {
            if picks.next() < threshold {
                let at = (picks.next() % BLOCK as u64) as usize;
                block[at] ^= 0xFF;
                changed += 1;
            }
        }
        right.write_all(&buf)?;
    }
    left.flush()?;
    right.flush()?;
    Ok(changed)
}

/// One cell as CSV text. `salt` 0 is the left value; any other salt gives a different value.
fn table_cell(id: u64, col: u64, salt: u64) -> String {
    let mut h =
        XorShift(id.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ col.wrapping_mul(0xD1B5_4A32) ^ 1);
    h.next();
    let a = h.next() % 100_000;
    let b = h.next() % 50;
    let a = a + salt * 100_000;
    if b == 0 {
        format!("\"item {a}, lot {col}\"")
    } else {
        format!("v{a}")
    }
}

fn table_line(id: u64, cols: u64, changed_col: Option<u64>) -> String {
    let mut line = id.to_string();
    for c in 2..=cols {
        line.push(',');
        line.push_str(&table_cell(id, c, u64::from(changed_col == Some(c))));
    }
    line.push_str("\r\n");
    line
}

/// Writes left.csv and right.csv; returns (right rows, changed rows, moved rows, dropped, added).
fn table(
    out: &Path,
    rows: u64,
    cols: u64,
    change_pct: f64,
) -> io::Result<(usize, u64, u64, u64, u64)> {
    fs::create_dir_all(out)?;
    let mut header = String::from("id");
    for c in 2..=cols {
        header.push_str(&format!(",c{c}"));
    }
    header.push_str("\r\n");
    let mut left = BufWriter::with_capacity(CHUNK, File::create(out.join("left.csv"))?);
    left.write_all(header.as_bytes())?;
    let mut picks = XorShift(0x2545_F491_4F6C_DD1D);
    let unit = |r: u64| (r >> 11) as f64 / (1u64 << 53) as f64 * 100.0;
    let (mut changed, mut dropped, mut added) = (0, 0, 0);
    let mut right_lines: Vec<String> = Vec::with_capacity(rows as usize);
    for id in 1..=rows {
        left.write_all(table_line(id, cols, None).as_bytes())?;
        if unit(picks.next()) < 0.1 {
            dropped += 1;
        } else if cols >= 2 && unit(picks.next()) < change_pct {
            let col = 2 + picks.next() % (cols - 1);
            right_lines.push(table_line(id, cols, Some(col)));
            changed += 1;
        } else {
            right_lines.push(table_line(id, cols, None));
        }
        if unit(picks.next()) < 0.1 {
            added += 1;
            right_lines.push(table_line(rows + added, cols, None));
        }
    }
    left.flush()?;
    let mut moved = 0;
    let want = right_lines.len() as u64 / 100;
    while moved < want {
        let from = (picks.next() % right_lines.len() as u64) as usize;
        let line = right_lines.remove(from);
        let to = (picks.next() % (right_lines.len() as u64 + 1)) as usize;
        right_lines.insert(to, line);
        moved += 1;
    }
    let mut right = BufWriter::with_capacity(CHUNK, File::create(out.join("right.csv"))?);
    right.write_all(header.as_bytes())?;
    for line in &right_lines {
        right.write_all(line.as_bytes())?;
    }
    right.flush()?;
    Ok((right_lines.len(), changed, moved, dropped, added))
}

const CELL: u32 = 16;

/// Writes left.png and right.png (RGBA, opaque) and returns how many cells of right.png changed.
fn picture(out: &Path, size: u32, change_pct: f64) -> io::Result<u64> {
    fs::create_dir_all(out)?;
    let side = size as usize;
    let mut noise = XorShift(0x6A09_E667_F3BC_C908);
    let mut pixels = vec![255u8; side * side * 4];
    for (i, px) in pixels.chunks_exact_mut(4).enumerate() {
        let (x, y) = ((i % side) as u64, (i / side) as u64);
        let n = (noise.next() % 8) as u8;
        px[0] = ((x * 215 / size as u64) as u8).saturating_add(n);
        px[1] = ((y * 215 / size as u64) as u8).saturating_add(n);
        px[2] = (((x + y) * 107 / size as u64) as u8).saturating_add(n);
    }
    write_png(&out.join("left.png"), &pixels, size)?;

    let per_row = size.div_ceil(CELL);
    let cells = u64::from(per_row) * u64::from(per_row);
    let want = ((cells as f64 * change_pct / 100.0).round() as u64).min(cells);
    let mut order: Vec<u32> = (0..cells as u32).collect();
    let mut picks = XorShift(0xA409_3822_299F_31D0);
    for i in (1..order.len()).rev() {
        order.swap(i, (picks.next() % (i as u64 + 1)) as usize);
    }
    for (k, &cell) in order.iter().take(want as usize).enumerate() {
        // Every 1,000th changed cell stays inside a typical tolerance.
        let step = if k % 1000 == 999 { 2 } else { 40 };
        let (cx, cy) = ((cell % per_row) * CELL, (cell / per_row) * CELL);
        for y in cy..(cy + CELL).min(size) {
            for x in cx..(cx + CELL).min(size) {
                let at = (y as usize * side + x as usize) * 4;
                pixels[at] = pixels[at].wrapping_add(step);
            }
        }
    }
    write_png(&out.join("right.png"), &pixels, size)?;
    Ok(want)
}

fn write_png(path: &Path, rgba: &[u8], size: u32) -> io::Result<()> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::{ExtendedColorType, ImageEncoder};
    let file = BufWriter::with_capacity(CHUNK, File::create(path)?);
    PngEncoder::new_with_quality(file, CompressionType::Fast, FilterType::Sub)
        .write_image(rgba, size, size, ExtendedColorType::Rgba8)
        .map_err(io::Error::other)
}

const MERGE_WORDS: [&str; 8] = [
    "alpha", "bravo", "delta", "ember", "frost", "grove", "harbor", "isle",
];

/// Writes base.txt, left.txt and right.txt; returns (planted conflicts, left only, right only).
fn merge(
    out: &Path,
    lines: u64,
    change_pct: f64,
    conflict_pct: f64,
) -> io::Result<(u64, u64, u64)> {
    let share = |pct: f64| (lines as f64 * pct / 100.0).round() as u64;
    let (conflicts, one_side) = (share(conflict_pct), share(change_pct));
    if conflicts + 2 * one_side > lines {
        return Err(io::Error::other(
            "the changes need more lines than the file has",
        ));
    }
    fs::create_dir_all(out)?;
    let mut words = XorShift(0x3C6E_F372_FE94_F82B);
    let base: Vec<String> = (0..lines)
        .map(|i| {
            let word = MERGE_WORDS[(words.next() % MERGE_WORDS.len() as u64) as usize];
            format!("line {i:06} {word}")
        })
        .collect();
    let mut order: Vec<usize> = (0..base.len()).collect();
    let mut picks = XorShift(0xBB67_AE85_84CA_A73B);
    for i in (1..order.len()).rev() {
        order.swap(i, (picks.next() % (i as u64 + 1)) as usize);
    }
    let (both, rest) = order.split_at(conflicts as usize);
    let (left_only, rest) = rest.split_at(one_side as usize);
    let right_only = &rest[..one_side as usize];
    let (mut left, mut right) = (base.clone(), base.clone());
    for &i in both {
        left[i].push_str(" L");
        right[i].push_str(" R");
    }
    for &i in left_only {
        left[i].push_str(" L");
    }
    for &i in right_only {
        right[i].push_str(" R");
    }
    for (name, side) in [
        ("base.txt", &base),
        ("left.txt", &left),
        ("right.txt", &right),
    ] {
        let mut file = BufWriter::new(File::create(out.join(name))?);
        for line in side {
            writeln!(file, "{line}")?;
        }
        file.flush()?;
    }
    Ok((conflicts, one_side, one_side))
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for item in fs::read_dir(dir)? {
        let item = item?;
        if item.file_type()?.is_dir() {
            collect(&item.path(), out)?;
        } else {
            out.push(item.path());
        }
    }
    Ok(())
}

fn readall(dirs: &[PathBuf], threads: usize) -> io::Result<()> {
    let mut files = Vec::new();
    for dir in dirs {
        collect(dir, &mut files)?;
    }
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(io::Error::other)?;
    let start = Instant::now();
    let bytes: u64 = pool.install(|| {
        files
            .par_iter()
            .map(|f| fs::read(f).map(|b| b.len() as u64))
            .sum::<io::Result<u64>>()
    })?;
    println!(
        "readall: files={} bytes={} ms={}",
        files.len(),
        bytes,
        start.elapsed().as_millis()
    );
    Ok(())
}

fn main() -> io::Result<()> {
    match Cli::parse().cmd {
        Cmd::Tree {
            out,
            files,
            change_pct,
        } => tree(&out, files, change_pct),
        Cmd::Text {
            out,
            size_mb,
            change_pct,
            long_pct,
        } => text(&out, size_mb, change_pct, long_pct),
        Cmd::Binary {
            out,
            size_mb,
            change_pct,
        } => {
            if !(0.0..=100.0).contains(&change_pct) {
                return Err(io::Error::other("--change-pct must be within 0..=100"));
            }
            let changed = binary(&out, size_mb, change_pct)?;
            println!("binary: size_mb={size_mb} changed_blocks={changed}");
            Ok(())
        }
        Cmd::Table {
            out,
            rows,
            cols,
            change_pct,
        } => {
            if !(0.0..=100.0).contains(&change_pct) || rows == 0 || cols == 0 {
                return Err(io::Error::other(
                    "--change-pct must be within 0..=100; --rows and --cols must be positive",
                ));
            }
            let (right, changed, moved, dropped, added) = table(&out, rows, cols, change_pct)?;
            println!(
                "table: rows={rows} cols={cols} right_rows={right} changed={changed} \
                 moved={moved} dropped={dropped} added={added}"
            );
            Ok(())
        }
        Cmd::Picture {
            out,
            size,
            change_pct,
        } => {
            if !(0.0..=100.0).contains(&change_pct) || size == 0 {
                return Err(io::Error::other(
                    "--change-pct must be within 0..=100; --size must be positive",
                ));
            }
            let changed = picture(&out, size, change_pct)?;
            println!("picture: size={size} changed_cells={changed}");
            Ok(())
        }
        Cmd::Merge {
            out,
            lines,
            change_pct,
            conflict_pct,
        } => {
            if !(0.0..=100.0).contains(&change_pct)
                || !(0.0..=100.0).contains(&conflict_pct)
                || lines == 0
            {
                return Err(io::Error::other(
                    "--change-pct and --conflict-pct must be within 0..=100; --lines must be positive",
                ));
            }
            let (conflicts, left_only, right_only) = merge(&out, lines, change_pct, conflict_pct)?;
            println!(
                "merge: lines={lines} planted_conflicts={conflicts} left_only={left_only} \
                 right_only={right_only}"
            );
            Ok(())
        }
        Cmd::Readall { dirs, threads } => readall(&dirs, threads),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_pair_is_one_mib_and_differs_in_the_changed_blocks() {
        let dir = std::env::temp_dir().join(format!("duet-benchgen-binary-{}", std::process::id()));
        // 10% of 256 blocks: about 26 expected.
        let changed = binary(&dir, 1, 10.0).unwrap();
        let left = fs::read(dir.join("left.bin")).unwrap();
        let right = fs::read(dir.join("right.bin")).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!((left.len(), right.len()), (1 << 20, 1 << 20));
        let differing = left.iter().zip(&right).filter(|(a, b)| a != b).count() as u64;
        assert_eq!(differing, changed);
        assert!((5..=60).contains(&changed), "changed {changed}");
    }

    /// Splits one CSV line into fields, honouring double quotes.
    fn fields(line: &str) -> usize {
        let (mut n, mut quoted) = (1, false);
        for ch in line.chars() {
            match ch {
                '"' => quoted = !quoted,
                ',' if !quoted => n += 1,
                _ => {}
            }
        }
        n
    }

    fn read_lines(path: &Path) -> Vec<String> {
        let text = fs::read_to_string(path).unwrap();
        text.lines().map(str::to_string).collect()
    }

    #[test]
    fn long_pct_makes_that_share_of_text_lines_150_to_400_characters() {
        let dir = std::env::temp_dir().join(format!("duet-benchgen-text-{}", std::process::id()));
        text(&dir.join("long"), 1, 1.0, 30.0).unwrap();
        text(&dir.join("plain"), 1, 1.0, 0.0).unwrap();
        let long = read_lines(&dir.join("long").join("left.txt"));
        let right = read_lines(&dir.join("long").join("right.txt"));
        let plain = read_lines(&dir.join("plain").join("left.txt"));
        fs::remove_dir_all(&dir).unwrap();
        let wide = long.iter().filter(|l| l.len() > 71).count();
        let share = wide as f64 / long.len() as f64;
        assert!(
            (0.25..=0.35).contains(&share),
            "{wide} of {} lines",
            long.len()
        );
        assert!(
            long.iter()
                .all(|l| l.len() == 71 || (150..=400).contains(&l.len()))
        );
        assert_eq!(
            long.len(),
            right.len(),
            "one right line for every left line"
        );
        assert!(
            plain.iter().all(|l| l.len() == 71),
            "no long lines without the flag"
        );
    }

    #[test]
    fn table_pair_has_a_header_unique_ids_and_equal_field_counts() {
        let dir = std::env::temp_dir().join(format!("duet-benchgen-table-{}", std::process::id()));
        table(&dir, 100, 5, 20.0).unwrap();
        let left = read_lines(&dir.join("left.csv"));
        let right = read_lines(&dir.join("right.csv"));
        let again_dir = dir.join("again");
        table(&again_dir, 100, 5, 20.0).unwrap();
        let again = read_lines(&again_dir.join("right.csv"));
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(left[0], "id,c2,c3,c4,c5");
        assert_eq!(right[0], "id,c2,c3,c4,c5");
        assert_eq!(left.len(), 101);
        assert!((96..=106).contains(&right.len()), "right {}", right.len());
        for line in left.iter().chain(&right) {
            assert_eq!(fields(line), 5, "{line}");
        }
        let mut ids: Vec<u64> = right[1..]
            .iter()
            .map(|l| l.split(',').next().unwrap().parse().unwrap())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), right.len() - 1);
        assert_ne!(left[1..], right[1..], "20% of 100 rows must differ");
        assert_eq!(right, again, "the generator is deterministic");
    }

    #[test]
    fn about_one_cell_in_fifty_is_quoted_with_a_comma() {
        let quoted = (1..=10_000)
            .flat_map(|id| (2..=10).map(move |c| table_cell(id, c, 0)))
            .filter(|cell| cell.starts_with('"') && cell.contains(','))
            .count();
        // 90,000 cells: 1,800 expected.
        assert!((1_500..=2_100).contains(&quoted), "quoted {quoted}");
    }

    #[test]
    fn picture_pair_decodes_and_changes_exactly_the_asked_cells() {
        let dir =
            std::env::temp_dir().join(format!("duet-benchgen-picture-{}", std::process::id()));
        let changed = picture(&dir, 64, 25.0).unwrap();
        let left = image::open(dir.join("left.png")).unwrap().to_rgba8();
        let right = image::open(dir.join("right.png")).unwrap().to_rgba8();
        let again_dir = dir.join("again");
        picture(&again_dir, 64, 25.0).unwrap();
        let again = image::open(again_dir.join("right.png")).unwrap().to_rgba8();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(left.dimensions(), (64, 64));
        assert_eq!(right.dimensions(), (64, 64));
        assert_eq!(changed, 4);
        let mut cells = std::collections::HashSet::new();
        for (x, y, px) in left.enumerate_pixels() {
            if right.get_pixel(x, y) != px {
                cells.insert((x / 16, y / 16));
            }
        }
        assert_eq!(cells.len(), 4, "4 of the 16 cells must differ");
        assert_eq!(right, again, "the generator is deterministic");
    }

    #[test]
    fn picture_cell_number_1000_changes_red_by_two_and_the_others_by_forty() {
        let dir = std::env::temp_dir().join(format!("duet-benchgen-pic2-{}", std::process::id()));
        // 512 / 16 = 32, so 1,024 cells, all changed: only k = 999 gets the small step.
        assert_eq!(picture(&dir, 512, 100.0).unwrap(), 1024);
        let left = image::open(dir.join("left.png")).unwrap().to_rgba8();
        let right = image::open(dir.join("right.png")).unwrap().to_rgba8();
        fs::remove_dir_all(&dir).unwrap();
        let mut deltas = std::collections::HashMap::new();
        for cy in 0..32 {
            for cx in 0..32 {
                let (x, y) = (cx * 16, cy * 16);
                let d = right.get_pixel(x, y)[0].wrapping_sub(left.get_pixel(x, y)[0]);
                *deltas.entry(d).or_insert(0) += 1;
            }
        }
        assert_eq!(deltas.get(&2), Some(&1), "{deltas:?}");
        assert_eq!(deltas.get(&40), Some(&1023), "{deltas:?}");
    }

    /// A test's folder under the temp dir, removed when the test ends, failed or not. (The
    /// crate has no tempfile and gets no new dependency.)
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            Scratch(
                std::env::temp_dir().join(format!("duet-benchgen-{name}-{}", std::process::id())),
            )
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_scratch_folder_goes_even_when_its_test_fails() {
        let scratch = Scratch::new("scratch");
        let dir = scratch.0.clone();
        let failed = std::panic::catch_unwind(move || {
            let scratch = scratch;
            fs::create_dir_all(&scratch.0).unwrap();
            fs::write(scratch.0.join("left.txt"), "x").unwrap();
            panic!("the test fails");
        });
        assert!(failed.is_err());
        assert!(!dir.exists(), "{} is left behind", dir.display());
    }

    #[test]
    fn merge_set_plants_disjoint_conflicts_and_one_sided_edits() {
        let scratch = Scratch::new("merge");
        let dir = &scratch.0;
        let planted = merge(dir, 100, 10.0, 5.0).unwrap();
        let read = |name: &str| read_lines(&dir.join(name));
        let (base, left, right) = (read("base.txt"), read("left.txt"), read("right.txt"));
        let again_dir = dir.join("again");
        merge(&again_dir, 100, 10.0, 5.0).unwrap();
        let again = read_lines(&again_dir.join("left.txt"));
        assert_eq!(planted, (5, 10, 10));
        assert_eq!((base.len(), left.len(), right.len()), (100, 100, 100));
        let mut unique = base.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), 100, "base lines are unique");
        let (mut both, mut left_only, mut right_only) = (0, 0, 0);
        for i in 0..100 {
            match (left[i] != base[i], right[i] != base[i]) {
                (true, true) => {
                    assert_ne!(left[i], right[i], "line {i}: a conflict must differ");
                    both += 1;
                }
                (true, false) => left_only += 1,
                (false, true) => right_only += 1,
                (false, false) => {}
            }
        }
        assert_eq!((both, left_only, right_only), (5, 10, 10));
        assert_eq!(left, again, "the generator is deterministic");
    }
}
