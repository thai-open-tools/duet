//! The picture diff: largest-channel delta, counts, 16 px difference groups and LocalDiff's
//! difference image (`localdiff/60-image.js:152-247`).

use rayon::prelude::*;

use super::Picture;
use crate::cancel::{CancelToken, Cancelled};

/// Side of a group cell in pixels.
pub const CELL: u32 = 16;
/// Differs by more than the tolerance.
pub const C_DIFF: [u8; 3] = [255, 32, 96];
/// Differs, but within the tolerance.
pub const C_MINOR: [u8; 3] = [64, 110, 210];
/// A pixel that exists in one picture only.
pub const C_ONLY: [u8; 3] = [150, 110, 30];

/// The largest absolute difference of R, G, B and A; 0 when both pixels are fully transparent.
pub fn delta(a: [u8; 4], b: [u8; 4]) -> u8 {
    if a[3] == 0 && b[3] == 0 {
        return 0;
    }
    (0..4).map(|i| a[i].abs_diff(b[i])).max().unwrap_or(0)
}

/// A box around neighbouring difference cells, in pixels, clamped to the overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cluster {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub cells: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureDiff {
    /// Pixels differing by more than the tolerance.
    pub count: u64,
    /// Pixels differing by 1..=tolerance.
    pub minor: u64,
    pub max_delta: u8,
    /// Compared pixels: the overlap area.
    pub total: u64,
    /// The smaller width and height.
    pub overlap: (u32, u32),
    /// The larger width and height.
    pub size: (u32, u32),
    pub size_mismatch: bool,
    /// Sorted by (y, x).
    pub clusters: Vec<Cluster>,
}

impl PictureDiff {
    pub fn is_identical(&self) -> bool {
        self.count == 0 && !self.size_mismatch
    }
}

struct Band {
    count: u64,
    minor: u64,
    max_delta: u8,
    cells: Vec<bool>,
}

fn px(p: &Picture, x: usize, y: usize) -> [u8; 4] {
    let i = (y * p.width as usize + x) * 4;
    [p.rgba[i], p.rgba[i + 1], p.rgba[i + 2], p.rgba[i + 3]]
}

pub fn compare(
    a: &Picture,
    b: &Picture,
    tolerance: u8,
    cancel: &CancelToken,
) -> Result<PictureDiff, Cancelled> {
    let cw = a.width.min(b.width);
    let ch = a.height.min(b.height);
    let gw = cw.div_ceil(CELL) as usize;
    let gh = ch.div_ceil(CELL) as usize;
    let bands: Vec<Band> = (0..gh)
        .into_par_iter()
        .map(|band| {
            cancel.check()?;
            let mut out = Band {
                count: 0,
                minor: 0,
                max_delta: 0,
                cells: vec![false; gw],
            };
            let y0 = band * CELL as usize;
            let y1 = (y0 + CELL as usize).min(ch as usize);
            for y in y0..y1 {
                for x in 0..cw as usize {
                    let d = delta(px(a, x, y), px(b, x, y));
                    if d == 0 {
                        continue;
                    }
                    out.max_delta = out.max_delta.max(d);
                    if d > tolerance {
                        out.count += 1;
                        out.cells[x / CELL as usize] = true;
                    } else {
                        out.minor += 1;
                    }
                }
            }
            Ok(out)
        })
        .collect::<Result<_, Cancelled>>()?;
    let count: u64 = bands.iter().map(|b| b.count).sum();
    let cells: Vec<bool> = bands.iter().flat_map(|b| b.cells.iter().copied()).collect();
    Ok(PictureDiff {
        count,
        minor: bands.iter().map(|b| b.minor).sum(),
        max_delta: bands.iter().map(|b| b.max_delta).max().unwrap_or(0),
        total: cw as u64 * ch as u64,
        overlap: (cw, ch),
        size: (a.width.max(b.width), a.height.max(b.height)),
        size_mismatch: a.width != b.width || a.height != b.height,
        clusters: if count > 0 {
            clusters(&cells, gw, gh, cw, ch)
        } else {
            Vec::new()
        },
    })
}

/// 8-way connected components over the cell grid, with an explicit stack.
fn clusters(cells: &[bool], gw: usize, gh: usize, cw: u32, ch: u32) -> Vec<Cluster> {
    let mut seen = vec![false; cells.len()];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..cells.len() {
        if !cells[start] || seen[start] {
            continue;
        }
        let (mut x0, mut y0, mut x1, mut y1, mut n) = (gw, gh, 0, 0, 0u32);
        seen[start] = true;
        stack.push(start);
        while let Some(c) = stack.pop() {
            let (cx, cy) = (c % gw, c / gw);
            n += 1;
            x0 = x0.min(cx);
            x1 = x1.max(cx);
            y0 = y0.min(cy);
            y1 = y1.max(cy);
            for ny in cy.saturating_sub(1)..=(cy + 1).min(gh - 1) {
                for nx in cx.saturating_sub(1)..=(cx + 1).min(gw - 1) {
                    let j = ny * gw + nx;
                    if cells[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        let (px0, py0) = (x0 as u32 * CELL, y0 as u32 * CELL);
        out.push(Cluster {
            x: px0,
            y: py0,
            width: cw.min((x1 as u32 + 1) * CELL) - px0,
            height: ch.min((y1 as u32 + 1) * CELL) - py0,
            cells: n,
        });
    }
    out.sort_by_key(|c| (c.y, c.x));
    out
}

/// The pixel's faded grey, unrounded (LocalDiff `gray`).
fn grey(p: [u8; 4]) -> f64 {
    let lum = (p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8;
    let a = p[3] as u32;
    let v = (lum * a + 128 * (255 - a)) as f64 / 255.0;
    20.0 + v * 0.32
}

/// A `Uint8ClampedArray` store: clamp, then round half to even.
fn to_u8(v: f64) -> u8 {
    v.clamp(0.0, 255.0).round_ties_even() as u8
}

/// The difference image, `max width × max height`.
pub fn diff_image(
    a: &Picture,
    b: &Picture,
    tolerance: u8,
    cancel: &CancelToken,
) -> Result<Picture, Cancelled> {
    let w = a.width.max(b.width) as usize;
    let h = a.height.max(b.height) as usize;
    let mut rgba = vec![0u8; w * h * 4];
    if w > 0 {
        rgba.par_chunks_mut(w * 4 * CELL as usize)
            .enumerate()
            .try_for_each(|(band, chunk)| {
                cancel.check()?;
                for (r, row) in chunk.chunks_exact_mut(w * 4).enumerate() {
                    let y = band * CELL as usize + r;
                    for (x, out) in row.chunks_exact_mut(4).enumerate() {
                        let in_a = x < a.width as usize && y < a.height as usize;
                        let in_b = x < b.width as usize && y < b.height as usize;
                        let rgb: [u8; 3] = if in_a && in_b {
                            let pa = px(a, x, y);
                            let d = delta(pa, px(b, x, y));
                            if d > tolerance {
                                C_DIFF
                            } else if d > 0 {
                                C_MINOR
                            } else {
                                [to_u8(grey(pa)); 3]
                            }
                        } else if in_a || in_b {
                            let v = grey(if in_a { px(a, x, y) } else { px(b, x, y) });
                            C_ONLY.map(|c| to_u8(c as f64 * 0.65 + v * 0.35))
                        } else {
                            continue;
                        };
                        out[..3].copy_from_slice(&rgb);
                        out[3] = 255;
                    }
                }
                Ok(())
            })?;
    }
    Ok(Picture::new(w as u32, h as u32, rgba))
}

/// `#rrggbb`, plus the alpha byte when the pixel is not opaque.
pub fn hex_color(pixel: [u8; 4]) -> String {
    let mut s = format!("#{:02x}{:02x}{:02x}", pixel[0], pixel[1], pixel[2]);
    if pixel[3] < 255 {
        s.push_str(&format!("{:02x}", pixel[3]));
    }
    s
}

/// LocalDiff `pctStr` (`js:58-63`).
pub fn percent_text(n: u64, total: u64) -> String {
    if total == 0 {
        return "0%".to_string();
    }
    let p = n as f64 / total as f64 * 100.0;
    if n > 0 && p < 0.01 {
        return "<0.01%".to_string();
    }
    format!("{p:.2}%")
}

#[cfg(test)]
mod tests;
