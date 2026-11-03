//! Pictures: file sniffing, decoding to RGBA, and the pixel container shared by the diff.

pub mod decode;
pub mod diff;
pub mod export;
pub mod sniff;

use std::fmt;
use std::path::Path;

pub const PICTURE_EXTENSIONS: [&str; 11] = [
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "ico", "tif", "tiff", "svg", "avif",
];

/// A picture over this many pixels is shown from a preview and not compared.
pub const MAX_PIXELS: u64 = 40_000_000;

/// True when the file name has a picture extension, in any case.
pub fn is_picture_name(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PICTURE_EXTENSIONS.iter().any(|p| p.eq_ignore_ascii_case(e)))
}

/// 8-bit RGBA, straight alpha, rows top to bottom.
#[derive(Clone, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Picture {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Picture {
        assert_eq!(
            rgba.len() as u64,
            width as u64 * height as u64 * 4,
            "rgba length must be width * height * 4"
        );
        Picture {
            width,
            height,
            rgba,
        }
    }

    pub fn filled(width: u32, height: u32, pixel: [u8; 4]) -> Picture {
        let rgba = pixel.repeat(width as usize * height as usize);
        Picture {
            width,
            height,
            rgba,
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        Some([
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ])
    }

    /// Panics when `(x, y)` is outside the picture.
    pub fn set_pixel(&mut self, x: u32, y: u32, pixel: [u8; 4]) {
        assert!(
            x < self.width && y < self.height,
            "pixel outside the picture"
        );
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.rgba[i..i + 4].copy_from_slice(&pixel);
    }

    pub fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

impl fmt::Debug for Picture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Picture {}×{}", self.width, self.height)
    }
}

#[cfg(test)]
mod tests;
