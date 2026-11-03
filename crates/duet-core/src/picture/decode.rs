//! Reading and decoding a picture into straight-alpha RGBA.
//!
//! Rasters go through the `image` crate the way gpui decodes them (orientation applied, 16-bit
//! and CMYK reduced to 8-bit, no colour management). SVG goes through `resvg`: it never loads a
//! file it links to, and gzip data (SVGZ) is refused, as a file and as a `data:` image.

use std::io::Cursor;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use image::{DynamicImage, ImageDecoder, ImageError, ImageFormat, ImageReader};
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{self, fontdb};

use super::sniff::{Format, sniff};
use super::{MAX_PIXELS, Picture};
use crate::cancel::CancelToken;

const MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_file_bytes: u64,
    /// Above this many pixels a picture gets a preview and no pixels.
    pub max_pixels: u64,
    /// Above this many pixels a picture is refused.
    pub max_decode_pixels: u64,
    /// Longer side of the preview, in pixels.
    pub preview_side: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_file_bytes: 256 * MIB,
            max_pixels: MAX_PIXELS,
            max_decode_pixels: 16384 * 16384,
            preview_side: 4096,
        }
    }
}

#[derive(Debug)]
pub struct Decoded {
    /// The picture's own size, after orientation.
    pub width: u32,
    pub height: u32,
    /// `Format::label`.
    pub format: String,
    /// `None` above `limits.max_pixels`.
    pub pixels: Option<Picture>,
    /// `Some` exactly when `pixels` is `None`.
    pub preview: Option<Picture>,
}

impl Decoded {
    pub fn big(&self) -> bool {
        self.pixels.is_none()
    }

    /// The pixels, or the preview of a big picture.
    pub fn shown(&self) -> &Picture {
        self.pixels
            .as_ref()
            .or(self.preview.as_ref())
            .expect("a decoded picture has pixels or a preview")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("the file is larger than {0} MB.")]
    FileTooLarge(u64),
    #[error("{0}.")]
    Io(String),
    #[error("{0} is not supported.")]
    Unsupported(String),
    #[error("the picture can't be decoded: the format is not supported, or the file is damaged.")]
    Corrupt,
    #[error("the picture has no size.")]
    Empty,
    #[error("the picture is too large to open ({0}×{1}).")]
    TooLarge(u32, u32),
    #[error("operation cancelled")]
    Cancelled,
}

/// Refuses a file over `limits.max_file_bytes` from its metadata, before reading it.
pub fn read(path: &Path, limits: &Limits) -> Result<Vec<u8>, DecodeError> {
    let io = |e: std::io::Error| DecodeError::Io(e.to_string());
    let len = std::fs::metadata(path).map_err(io)?.len();
    if len > limits.max_file_bytes {
        return Err(DecodeError::FileTooLarge(limits.max_file_bytes / MIB));
    }
    std::fs::read(path).map_err(io)
}

fn check(cancel: &CancelToken) -> Result<(), DecodeError> {
    cancel.check().map_err(|_| DecodeError::Cancelled)
}

pub fn decode(
    bytes: &[u8],
    name: &str,
    limits: &Limits,
    cancel: &CancelToken,
) -> Result<Decoded, DecodeError> {
    let format = sniff(bytes, name).ok_or(DecodeError::Corrupt)?;
    check(cancel)?;
    let image_format = match format {
        Format::Avif | Format::Heic => {
            return Err(DecodeError::Unsupported(format.label().to_string()));
        }
        Format::Svg => return decode_svg(bytes, limits, cancel),
        Format::Png => ImageFormat::Png,
        Format::Jpeg => ImageFormat::Jpeg,
        Format::Gif => ImageFormat::Gif,
        Format::Webp => ImageFormat::WebP,
        Format::Ico => match largest_png_entry(bytes) {
            // The `image` crate takes only RGBA PNG entries; Windows and browsers take any.
            Some(png) => return decode_raster(png, format, ImageFormat::Png, limits, cancel),
            None => ImageFormat::Ico,
        },
        Format::Tiff => ImageFormat::Tiff,
        Format::Bmp => ImageFormat::Bmp,
    };
    decode_raster(bytes, format, image_format, limits, cancel)
}

/// What a PNG starts with.
const PNG_SIGNATURE: [u8; 8] = *b"\x89PNG\r\n\x1a\n";

/// The bytes of an icon's largest entry (then the deepest) when that entry is a PNG lying
/// wholly inside the file; None for a BMP entry or a damaged directory.
fn largest_png_entry(bytes: &[u8]) -> Option<&[u8]> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    // A size byte of 0 means 256 px.
    let side = |byte: u8| if byte == 0 { 256 } else { u32::from(byte) };
    let mut best: Option<((u32, u16), usize, usize)> = None;
    for index in 0..usize::from(u16_at(4)?) {
        let entry = 6 + 16 * index;
        let (width, height) = (*bytes.get(entry)?, *bytes.get(entry + 1)?);
        let score = (side(width) * side(height), u16_at(entry + 6)?);
        let (len, offset) = (u32_at(entry + 8)? as usize, u32_at(entry + 12)? as usize);
        if best.is_none_or(|(top, _, _)| score > top) {
            best = Some((score, offset, len));
        }
    }
    let (_, offset, len) = best?;
    let data = bytes.get(offset..offset.checked_add(len)?)?;
    data.starts_with(&PNG_SIGNATURE).then_some(data)
}

fn decode_raster(
    bytes: &[u8],
    format: Format,
    image_format: ImageFormat,
    limits: &Limits,
    cancel: &CancelToken,
) -> Result<Decoded, DecodeError> {
    let mut decoder = ImageReader::with_format(Cursor::new(bytes), image_format)
        .into_decoder()
        .map_err(|_| DecodeError::Corrupt)?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 {
        return Err(DecodeError::Empty);
    }
    if w as u64 * h as u64 > limits.max_decode_pixels {
        return Err(DecodeError::TooLarge(w, h));
    }
    let mut image_limits = image::Limits::default();
    image_limits.max_alloc = Some(limits.max_decode_pixels.saturating_mul(4) + 64 * MIB);
    decoder
        .set_limits(image_limits)
        .map_err(|_| DecodeError::TooLarge(w, h))?;
    let orientation = decoder.orientation().map_err(|_| DecodeError::Corrupt)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(|e| match e {
        ImageError::Limits(_) => DecodeError::TooLarge(w, h),
        _ => DecodeError::Corrupt,
    })?;
    image.apply_orientation(orientation);
    check(cancel)?;

    let (width, height) = (image.width(), image.height());
    let label = format.label().to_string();
    if width as u64 * height as u64 > limits.max_pixels {
        let (pw, ph) = preview_size(width, height, limits.preview_side);
        let thumb = image::imageops::thumbnail(&image, pw, ph);
        check(cancel)?;
        let preview = Picture::new(pw, ph, thumb.into_raw());
        return Ok(Decoded {
            width,
            height,
            format: label,
            pixels: None,
            preview: Some(preview),
        });
    }
    let rgba = image.into_rgba8().into_raw();
    check(cancel)?;
    Ok(Decoded {
        width,
        height,
        format: label,
        pixels: Some(Picture::new(width, height, rgba)),
        preview: None,
    })
}

/// `max(1, side × length / longest)` for each axis, in integers.
fn preview_size(w: u32, h: u32, side: u32) -> (u32, u32) {
    let longest = w.max(h) as u64;
    let scale = |v: u32| ((v as u64 * side as u64 / longest).max(1)) as u32;
    (scale(w), scale(h))
}

/// True when the root `<svg ...>` tag has a width, height or viewBox attribute.
fn root_declares_size(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let Some(start) = text.to_ascii_lowercase().find("<svg") else {
        return false;
    };
    let tag = &text[start..];
    let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
    ["width", "height", "viewBox"].iter().any(|name| {
        tag.match_indices(name).any(|(i, _)| {
            let before = tag[..i].chars().next_back();
            let after = tag[i + name.len()..].trim_start();
            before.is_some_and(char::is_whitespace) && after.starts_with('=')
        })
    })
}

fn system_fonts() -> Arc<fontdb::Database> {
    static FONTS: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

/// What a gzip stream starts with: usvg inflates such data (SVGZ) with no limit on the result.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// usvg's `data:` URL resolver, except that gzip data (an SVGZ) resolves to nothing. A nested
/// SVG forwards its `data:` URLs to the outer resolver, so this covers every depth.
pub(super) fn svg_data_resolver() -> usvg::ImageHrefDataResolverFn<'static> {
    let resolve = usvg::ImageHrefResolver::default_data_resolver();
    Box::new(move |mime, data, options| {
        if data.starts_with(&GZIP_MAGIC) {
            None
        } else {
            resolve(mime, data, options)
        }
    })
}

fn decode_svg(bytes: &[u8], limits: &Limits, cancel: &CancelToken) -> Result<Decoded, DecodeError> {
    // LocalDiff's `<img>` never inflated SVGZ either; refusing it keeps a gzip bomb out.
    if bytes.starts_with(&GZIP_MAGIC) {
        return Err(DecodeError::Unsupported("SVGZ".to_string()));
    }
    let mut options = usvg::Options {
        resources_dir: None,
        default_size: usvg::Size::from_wh(300.0, 150.0).expect("positive size"),
        fontdb: system_fonts(),
        ..usvg::Options::default()
    };
    // A linked file is never read; `data:` URLs still resolve, gzip ones to nothing.
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    options.image_href_resolver.resolve_data = svg_data_resolver();
    let tree = usvg::Tree::from_data(bytes, &options).map_err(|e| match e {
        usvg::Error::InvalidSize => DecodeError::Empty,
        _ => DecodeError::Corrupt,
    })?;
    let size = tree.size().to_int_size();
    let (mut w, mut h) = (size.width(), size.height());
    // usvg 0.46 never applies `default_size` to a root without width, height or viewBox, so an
    // empty one comes out 100 x 100. A browser gives 300 x 150. (A sizeless root with content
    // keeps usvg's content box.)
    if tree.root().children().is_empty() && !root_declares_size(bytes) {
        (w, h) = (300, 150);
    }
    if w == 0 || h == 0 {
        return Err(DecodeError::Empty);
    }
    if w as u64 * h as u64 > limits.max_decode_pixels {
        return Err(DecodeError::TooLarge(w, h));
    }
    check(cancel)?;

    let big = w as u64 * h as u64 > limits.max_pixels;
    let (rw, rh) = if big {
        preview_size(w, h, limits.preview_side)
    } else {
        (w, h)
    };
    let mut pixmap = Pixmap::new(rw, rh).ok_or(DecodeError::TooLarge(w, h))?;
    let transform = Transform::from_scale(rw as f32 / w as f32, rh as f32 / h as f32);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    check(cancel)?;

    let mut rgba = Vec::with_capacity(rw as usize * rh as usize * 4);
    for px in pixmap.pixels() {
        let c = px.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    check(cancel)?;
    let picture = Picture::new(rw, rh, rgba);
    let label = Format::Svg.label().to_string();
    Ok(if big {
        Decoded {
            width: w,
            height: h,
            format: label,
            pixels: None,
            preview: Some(picture),
        }
    } else {
        Decoded {
            width: w,
            height: h,
            format: label,
            pixels: Some(picture),
            preview: None,
        }
    })
}
