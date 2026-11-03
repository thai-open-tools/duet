//! A picture as gpui images (R6, R7, R18): BGRA straight-alpha tiles of at most 4096 px, the
//! nearest-neighbour enlargement that keeps pixels sharp above 100% and its cache, and the
//! checkerboard.

use std::sync::{Arc, OnceLock};

use duet_core::picture::Picture;
use gpui_kit::RenderImage;
use image::{Frame, RgbaImage};

/// One atlas texture is at most 16384 px a side (`directx_atlas.rs:171`)...
pub const MAX_TEXTURE: u32 = 16384;
/// ...so a picture is cut well below that.
pub const TILE: u32 = 4096;

pub struct Tile {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub image: Arc<RenderImage>,
}

/// A picture as gpui images, row by row. `width` × `height` is the tiles' own pixels (a
/// preview's, for a big picture).
pub struct Tiles {
    pub width: u32,
    pub height: u32,
    pub tiles: Vec<Tile>,
}

fn frame_image(width: u32, height: u32, bgra: Vec<u8>) -> Arc<RenderImage> {
    let buffer = RgbaImage::from_raw(width, height, bgra).expect("a buffer of width * height * 4");
    Arc::new(RenderImage::new(vec![Frame::new(buffer)]))
}

fn swap_red_blue(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
}

/// A gpui image from straight RGBA bytes; copies them, swapping R and B.
pub fn render_image(width: u32, height: u32, rgba: &[u8]) -> Arc<RenderImage> {
    let mut bgra = rgba.to_vec();
    swap_red_blue(&mut bgra);
    frame_image(width, height, bgra)
}

/// Cuts a picture into tiles of at most `TILE` × `TILE` px.
pub fn to_tiles(picture: &Picture) -> Tiles {
    let mut tiles = Vec::new();
    let stride = picture.width as usize * 4;
    for y in (0..picture.height).step_by(TILE as usize) {
        let height = TILE.min(picture.height - y);
        for x in (0..picture.width).step_by(TILE as usize) {
            let width = TILE.min(picture.width - x);
            let mut bgra = Vec::with_capacity(width as usize * height as usize * 4);
            for row in y..y + height {
                let start = row as usize * stride + x as usize * 4;
                bgra.extend_from_slice(&picture.rgba[start..start + width as usize * 4]);
            }
            swap_red_blue(&mut bgra);
            tiles.push(Tile {
                x,
                y,
                width,
                height,
                image: frame_image(width, height, bgra),
            });
        }
    }
    Tiles {
        width: picture.width,
        height: picture.height,
        tiles,
    }
}

/// Nearest-neighbour enlargement of the source rect `crop` (x, y, w, h), read from the tiles'
/// BGRA bytes: every pixel repeats `factor` × `factor` times. The crop is limited to the tiles,
/// and a crop with nothing of them left is one clear pixel. The factor is lowered so the
/// result fits one atlas texture.
pub fn enlarge(tiles: &Tiles, crop: (u32, u32, u32, u32), factor: u32) -> Arc<RenderImage> {
    let (x0, y0) = (crop.0.min(tiles.width), crop.1.min(tiles.height));
    let (w, h) = (crop.2.min(tiles.width - x0), crop.3.min(tiles.height - y0));
    if w == 0 || h == 0 {
        return frame_image(1, 1, vec![0; 4]);
    }
    let factor = factor.clamp(1, (MAX_TEXTURE / w.max(h)).max(1));
    let columns = tiles.width.div_ceil(TILE).max(1);
    let (out_w, out_h) = (w * factor, h * factor);
    debug_assert!(out_w <= MAX_TEXTURE && out_h <= MAX_TEXTURE);
    let mut out = vec![0u8; out_w as usize * out_h as usize * 4];
    for sy in 0..h {
        let y = y0 + sy;
        // One output row for this source row, then copied `factor` times.
        let row_start = (sy * factor) as usize * out_w as usize * 4;
        for sx in 0..w {
            let x = x0 + sx;
            let Some(tile) = tiles.tiles.get(((y / TILE) * columns + x / TILE) as usize) else {
                continue;
            };
            let Some(bytes) = tile.image.as_bytes(0) else {
                continue;
            };
            let at = ((y - tile.y) as usize * tile.width as usize + (x - tile.x) as usize) * 4;
            let pixel = &bytes[at..at + 4];
            for rep in 0..factor {
                let to = row_start + ((sx * factor + rep) as usize) * 4;
                out[to..to + 4].copy_from_slice(pixel);
            }
        }
        let row_len = out_w as usize * 4;
        for rep in 1..factor as usize {
            out.copy_within(row_start..row_start + row_len, row_start + rep * row_len);
        }
    }
    frame_image(out_w, out_h, out)
}

/// Which enlargement an image is: the tiles it was read from (their first image's id), the
/// source rect and the factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnlargeKey {
    pub source: usize,
    pub crop: (u32, u32, u32, u32),
    pub factor: u32,
}

/// The enlargements the stage paints (R7), kept from one frame to the next so a redraw with
/// nothing changed builds nothing. A frame asks for what it paints with `get`; `finish` hands
/// back what it did not ask for, to be dropped from the atlas.
#[derive(Default)]
pub struct Enlargements {
    kept: Vec<(EnlargeKey, Arc<RenderImage>)>,
    used: Vec<(EnlargeKey, Arc<RenderImage>)>,
    built: usize,
}

impl Enlargements {
    /// The enlargement `key` of `tiles`: the one kept from the last frame, else a new one.
    pub fn get(&mut self, key: EnlargeKey, tiles: &Tiles) -> Arc<RenderImage> {
        if let Some((_, image)) = self.used.iter().find(|(k, _)| *k == key) {
            return image.clone();
        }
        let image = match self.kept.iter().position(|(k, _)| *k == key) {
            Some(at) => self.kept.swap_remove(at).1,
            None => {
                self.built += 1;
                enlarge(tiles, key.crop, key.factor)
            }
        };
        self.used.push((key, image.clone()));
        image
    }

    /// Ends a frame: what it used is kept for the next one, the rest is handed back.
    pub fn finish(&mut self) -> Vec<Arc<RenderImage>> {
        let unused = std::mem::replace(&mut self.kept, std::mem::take(&mut self.used));
        unused.into_iter().map(|(_, image)| image).collect()
    }

    /// Every image held, for dropping when the tab closes.
    pub fn images(&self) -> impl Iterator<Item = &Arc<RenderImage>> {
        self.kept.iter().chain(&self.used).map(|(_, image)| image)
    }

    /// How many enlargements were built so far.
    pub fn built(&self) -> usize {
        self.built
    }
}

/// The checkerboard behind pictures (LocalDiff `--im-ck1`/`--im-ck2`, R18): 64 × 64 px of 8 px
/// squares, in the theme's colours. One image per theme serves the whole process, so it is
/// uploaded once per window (16 KB) and never dropped.
pub fn checker(dark: bool) -> Arc<RenderImage> {
    static DARK: OnceLock<Arc<RenderImage>> = OnceLock::new();
    static LIGHT: OnceLock<Arc<RenderImage>> = OnceLock::new();
    let shared = if dark { &DARK } else { &LIGHT };
    shared.get_or_init(|| checker_image(dark)).clone()
}

fn checker_image(dark: bool) -> Arc<RenderImage> {
    let (one, two): ([u8; 3], [u8; 3]) = if dark {
        ([0x2b, 0x31, 0x3b], [0x23, 0x28, 0x31])
    } else {
        ([0xff, 0xff, 0xff], [0xe3, 0xe7, 0xed])
    };
    let mut bgra = Vec::with_capacity(64 * 64 * 4);
    for y in 0..64u32 {
        for x in 0..64u32 {
            let [r, g, b] = if (x / 8 + y / 8) % 2 == 0 { one } else { two };
            bgra.extend_from_slice(&[b, g, r, 255]);
        }
    }
    frame_image(64, 64, bgra)
}

/// Every image of the tiles, for dropping.
pub fn images(tiles: &Tiles) -> impl Iterator<Item = &Arc<RenderImage>> {
    tiles.tiles.iter().map(|tile| &tile.image)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(image: &RenderImage) -> &[u8] {
        image.as_bytes(0).unwrap()
    }

    #[test]
    fn tiles_cover_the_picture_in_4096_px_squares_as_bgra() {
        let mut picture = Picture::filled(5000, 3, [0, 0, 0, 0]);
        picture.set_pixel(0, 0, [1, 2, 3, 4]);
        picture.set_pixel(4096, 2, [5, 6, 7, 8]);
        let tiles = to_tiles(&picture);
        let rects: Vec<_> = tiles
            .tiles
            .iter()
            .map(|t| (t.x, t.y, t.width, t.height))
            .collect();
        assert_eq!(rects, [(0, 0, 4096, 3), (4096, 0, 904, 3)]);
        assert_eq!(&bytes(&tiles.tiles[0].image)[..4], [3, 2, 1, 4]);
        let at = 2 * 904 * 4;
        assert_eq!(&bytes(&tiles.tiles[1].image)[at..at + 4], [7, 6, 5, 8]);
        assert_eq!((tiles.width, tiles.height), (5000, 3));
    }

    #[test]
    fn a_tall_picture_gets_a_second_row_of_tiles() {
        let tiles = to_tiles(&Picture::filled(2, 4100, [9, 9, 9, 255]));
        let rects: Vec<_> = tiles.tiles.iter().map(|t| (t.y, t.height)).collect();
        assert_eq!(rects, [(0, 4096), (4096, 4)]);
    }

    #[test]
    fn an_enlargement_repeats_each_pixel() {
        let picture = Picture::new(2, 1, vec![10, 20, 30, 255, 40, 50, 60, 255]);
        let big = enlarge(&to_tiles(&picture), (0, 0, 2, 1), 3);
        assert_eq!((big.size(0).width.0, big.size(0).height.0), (6, 3));
        let data = bytes(&big);
        for row in 0..3 {
            for col in 0..6 {
                let at = (row * 6 + col) * 4;
                let want = if col < 3 {
                    [30, 20, 10, 255]
                } else {
                    [60, 50, 40, 255]
                };
                assert_eq!(&data[at..at + 4], want, "row {row} col {col}");
            }
        }
    }

    #[test]
    fn an_enlargement_reads_across_tiles() {
        let mut picture = Picture::filled(4100, 1, [0, 0, 0, 255]);
        picture.set_pixel(4094, 0, [1, 1, 1, 255]);
        picture.set_pixel(4095, 0, [2, 2, 2, 255]);
        picture.set_pixel(4096, 0, [3, 3, 3, 255]);
        picture.set_pixel(4097, 0, [4, 4, 4, 255]);
        let big = enlarge(&to_tiles(&picture), (4094, 0, 4, 1), 2);
        assert_eq!((big.size(0).width.0, big.size(0).height.0), (8, 2));
        let firsts: Vec<u8> = (0..8).map(|i| bytes(&big)[i * 4]).collect();
        assert_eq!(firsts, [1, 1, 2, 2, 3, 3, 4, 4]);
    }

    #[test]
    fn a_crop_past_the_edge_is_one_clear_pixel_and_the_factor_fits_a_texture() {
        let tiles = to_tiles(&Picture::filled(3, 2, [9, 9, 9, 255]));
        for crop in [(3, 0, 2, 2), (0, 2, 1, 1), (1, 1, 0, 1)] {
            let image = enlarge(&tiles, crop, 4);
            assert_eq!((image.size(0).width.0, image.size(0).height.0), (1, 1));
            assert_eq!(bytes(&image), [0, 0, 0, 0], "{crop:?}");
        }
        // A crop running past the edge keeps what is inside.
        let image = enlarge(&tiles, (2, 1, 9, 9), 2);
        assert_eq!((image.size(0).width.0, image.size(0).height.0), (2, 2));
        // 4096 px at ×8 would be 32768 px wide: the factor drops to 4.
        let wide = to_tiles(&Picture::filled(4096, 1, [1, 2, 3, 255]));
        let image = enlarge(&wide, (0, 0, 4096, 1), 8);
        assert_eq!(
            (image.size(0).width.0, image.size(0).height.0),
            (MAX_TEXTURE as i32, 4)
        );
    }

    #[test]
    fn an_unchanged_frame_builds_no_enlargement_and_unused_ones_are_handed_back() {
        let tiles = to_tiles(&Picture::filled(4, 4, [5, 5, 5, 255]));
        let key = |x| EnlargeKey {
            source: tiles.tiles[0].image.id.0,
            crop: (x, 0, 2, 2),
            factor: 3,
        };
        let mut cache = Enlargements::default();
        let first = cache.get(key(0), &tiles);
        assert_eq!(
            cache.get(key(0), &tiles).id,
            first.id,
            "asked twice, built once"
        );
        assert!(cache.finish().is_empty());
        assert_eq!(cache.built(), 1);
        assert_eq!(
            cache.get(key(0), &tiles).id,
            first.id,
            "the next frame reuses it"
        );
        let other = cache.get(key(1), &tiles);
        assert_eq!(cache.built(), 2);
        assert!(cache.finish().is_empty());
        let unused = cache.finish();
        let mut ids: Vec<_> = unused.iter().map(|image| image.id).collect();
        ids.sort_by_key(|id| id.0);
        assert_eq!(
            ids,
            [first.id, other.id],
            "a frame that used none hands both back"
        );
        assert_eq!(cache.images().count(), 0);
    }

    #[test]
    fn one_checkerboard_serves_every_frame() {
        assert_eq!(checker(true).id, checker(true).id);
        assert_ne!(checker(true).id, checker(false).id);
    }

    #[test]
    fn the_checkerboard_alternates_8px_squares_in_the_theme_colours() {
        let px = |image: &RenderImage, x: usize, y: usize| {
            let at = (y * 64 + x) * 4;
            bytes(image)[at..at + 4].to_vec()
        };
        let dark = checker(true);
        assert_eq!(px(&dark, 0, 0), [0x3b, 0x31, 0x2b, 255]);
        assert_eq!(px(&dark, 8, 0), [0x31, 0x28, 0x23, 255]);
        assert_eq!(px(&dark, 8, 8), px(&dark, 0, 0));
        let light = checker(false);
        assert_eq!(px(&light, 0, 0), [0xff, 0xff, 0xff, 255]);
        assert_eq!(px(&light, 8, 0), [0xed, 0xe7, 0xe3, 255]);
    }
}
