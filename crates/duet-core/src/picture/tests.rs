use std::io::Cursor;
use std::path::Path;

use image::{DynamicImage, ImageFormat, RgbaImage};

use super::decode::{DecodeError, Limits, decode, read};
use super::sniff::{Format, format_label, sniff};
use super::{Picture, is_picture_name};
use crate::cancel::CancelToken;

fn sample() -> Picture {
    Picture::new(
        3,
        2,
        vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, //
            255, 255, 255, 255, 0, 0, 0, 255, 10, 20, 30, 128,
        ],
    )
}

fn opaque() -> Picture {
    let mut p = sample();
    p.set_pixel(2, 1, [10, 20, 30, 255]);
    p
}

fn encode(p: &Picture, format: ImageFormat) -> Vec<u8> {
    let img = DynamicImage::ImageRgba8(
        RgbaImage::from_raw(p.width, p.height, p.rgba.clone()).expect("size matches"),
    );
    let img = if format == ImageFormat::Jpeg {
        DynamicImage::ImageRgb8(img.to_rgb8())
    } else {
        img
    };
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, format).expect("encodes");
    out.into_inner()
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            let sextet = (n >> (18 - 6 * i)) & 63;
            out.push(if i <= chunk.len() {
                T[sextet as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// `bytes` as a gzip stream of one stored (uncompressed) deflate block.
fn gzip(bytes: &[u8]) -> Vec<u8> {
    let crc = !bytes.iter().fold(!0u32, |crc, &byte| {
        (0..8).fold(crc ^ u32::from(byte), |c, _| {
            if c & 1 == 1 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            }
        })
    });
    let len = u16::try_from(bytes.len()).expect("one stored block");
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff, 1];
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&(!len).to_le_bytes());
    out.extend_from_slice(bytes);
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out
}

const RED_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="#ff0000"/></svg>"##;

fn dec(bytes: &[u8], name: &str) -> Result<super::decode::Decoded, DecodeError> {
    decode(bytes, name, &Limits::default(), &CancelToken::new())
}

#[test]
fn sniff_reads_magic_bytes_before_the_extension() {
    assert_eq!(sniff(b"\x89PNG\r\n\x1a\n", "x.jpg"), Some(Format::Png));
    assert_eq!(sniff(b"\xff\xd8\xff\xe0", "x.png"), Some(Format::Jpeg));
    assert_eq!(sniff(b"GIF89a", "x"), Some(Format::Gif));
    assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 ", "x"), Some(Format::Webp));
    assert_eq!(sniff(&[0, 0, 1, 0, 1, 0], "x"), Some(Format::Ico));
    assert_eq!(
        sniff(&[0, 0, 2, 0, 1, 0], "x"),
        Some(Format::Ico),
        "a cursor"
    );
    assert_eq!(sniff(b"\0\0\0\x1cftypavif", "x.png"), Some(Format::Avif));
    assert_eq!(sniff(b"\0\0\0\x18ftypheic", "x"), Some(Format::Heic));
    assert_eq!(sniff(b"II*\0", "x"), Some(Format::Tiff));
    assert_eq!(sniff(b"MM\0*", "x"), Some(Format::Tiff));
    assert_eq!(sniff(b"BM\0\0", "x"), Some(Format::Bmp));
    assert_eq!(
        sniff(b"<?xml version=\"1.0\"?>\n<SVG width=\"1\">", "x.txt"),
        Some(Format::Svg)
    );
    assert_eq!(
        sniff(b"plain", "x.TIF"),
        Some(Format::Tiff),
        "the extension when no magic matches"
    );
    assert_eq!(sniff(b"plain", "x.cur"), Some(Format::Ico));
    assert_eq!(sniff(b"plain", "x.xyz"), None);
    assert_eq!(
        (
            format_label(b"plain", "x.xyz"),
            format_label(b"plain", "noext")
        ),
        ("XYZ".to_string(), "?".to_string())
    );
}

#[test]
fn decode_reads_each_raster_format() {
    let cases: [(ImageFormat, &str, Picture); 5] = [
        (ImageFormat::Png, "PNG", sample()),
        (ImageFormat::Tiff, "TIFF", opaque()),
        (ImageFormat::WebP, "WEBP", sample()),
        (ImageFormat::Ico, "ICO", sample()),
        (ImageFormat::Bmp, "BMP", opaque()),
    ];
    for (format, label, expected) in cases {
        let d = dec(&encode(&expected, format), "x").unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!((d.width, d.height, d.format.as_str()), (3, 2, label));
        assert_eq!(d.pixels, Some(expected), "{label}");
        assert!(d.preview.is_none(), "{label}");
    }
    let d = dec(&encode(&opaque(), ImageFormat::Jpeg), "x").expect("jpeg");
    assert_eq!((d.width, d.height, d.format.as_str()), (3, 2, "JPEG"));
}

/// An ICO whose entries are PNGs of RGB pixels with no alpha (as Pillow writes them, I-11),
/// each `side` px square and filled with `rgb`.
fn png_icon(entries: &[(u32, [u8; 3])]) -> Vec<u8> {
    let pngs: Vec<Vec<u8>> = entries
        .iter()
        .map(|&(side, rgb)| {
            let img =
                DynamicImage::ImageRgb8(image::RgbImage::from_pixel(side, side, image::Rgb(rgb)));
            let mut out = Cursor::new(Vec::new());
            img.write_to(&mut out, ImageFormat::Png).expect("encodes");
            out.into_inner()
        })
        .collect();
    let count = entries.len() as u16;
    let mut out = vec![0, 0, 1, 0];
    out.extend_from_slice(&count.to_le_bytes());
    let mut offset = 6 + 16 * u32::from(count);
    for (&(side, _), png) in entries.iter().zip(&pngs) {
        let byte = if side >= 256 { 0 } else { side as u8 };
        // Width, height, colours, reserved, planes 0 and 32 bits per pixel, as Pillow writes.
        out.extend_from_slice(&[byte, byte, 0, 0, 0, 0, 32, 0]);
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for png in pngs {
        out.extend_from_slice(&png);
    }
    out
}

#[test]
fn an_icon_of_rgb_png_entries_shows_its_largest_entry() {
    let icon = png_icon(&[(16, [255, 0, 0]), (32, [0, 255, 0]), (120, [0, 0, 255])]);
    let d = dec(&icon, "f.ico").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((d.width, d.height, d.format.as_str()), (120, 120, "ICO"));
    let pixels = d.pixels.expect("pixels");
    assert_eq!(pixels.pixel(119, 119), Some([0, 0, 255, 255]));
    // An entry that runs past the end of the file is still a damaged file.
    let last = 6 + 16 * 2;
    let offset = u32::from_le_bytes(icon[last + 12..last + 16].try_into().unwrap()) as usize;
    assert_eq!(
        dec(&icon[..offset + 20], "f.ico").unwrap_err(),
        DecodeError::Corrupt
    );
}

#[test]
fn gif_decodes_its_first_frame() {
    use image::codecs::gif::GifEncoder;
    let frame = |rgba: [u8; 4]| image::Frame::new(RgbaImage::from_pixel(2, 2, image::Rgba(rgba)));
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut out);
        enc.encode_frames([frame([255, 0, 0, 255]), frame([0, 0, 255, 255])])
            .expect("gif");
    }
    let d = dec(&out, "x.gif").expect("decodes");
    assert_eq!((d.width, d.height, d.format.as_str()), (2, 2, "GIF"));
    let p = d.pixels.expect("pixels").pixel(0, 0).expect("pixel");
    assert!(p[0] > 200 && p[2] < 50, "first frame is red: {p:?}");
}

#[test]
fn svg_renders_through_resvg_at_its_own_size() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="3"><rect width="2" height="3" fill="#ff0000"/></svg>"##;
    let d = dec(svg, "x.svg").expect("renders");
    assert_eq!((d.width, d.height, d.format.as_str()), (4, 3, "SVG"));
    let p = d.pixels.expect("pixels");
    assert_eq!(p.pixel(1, 0), Some([255, 0, 0, 255]));
    assert_eq!(p.pixel(3, 2), Some([0, 0, 0, 0]));

    let d = dec(
        br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 10"/>"#,
        "x.svg",
    )
    .expect("viewBox");
    assert_eq!((d.width, d.height), (20, 10));

    let d = dec(br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#, "x.svg").expect("no size");
    assert_eq!((d.width, d.height), (300, 150));
}

#[test]
fn svg_text_draws_with_the_system_fonts() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="30"><text x="2" y="24" font-size="24" font-family="Arial">Hi</text></svg>"#;
    let p = dec(svg, "x.svg").expect("renders").pixels.expect("pixels");
    assert!(
        p.rgba.chunks_exact(4).any(|px| px[3] > 0),
        "some pixel is drawn"
    );
}

#[test]
fn an_svg_never_loads_files_it_links_to() {
    let dir = tempfile::tempdir().expect("tempdir");
    let png = encode(&Picture::filled(4, 4, [255, 0, 0, 255]), ImageFormat::Png);
    let path = dir.path().join("red.png");
    std::fs::write(&path, &png).expect("write");
    let abs = path.to_string_lossy().replace('\\', "/");
    let svg = |href: &str| {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="4" height="4"><image width="4" height="4" xlink:href="{href}"/></svg>"#
        )
    };
    for href in [abs.clone(), format!("file:///{abs}")] {
        let p = dec(svg(&href).as_bytes(), "x.svg")
            .expect("renders")
            .pixels
            .expect("pixels");
        assert_eq!(p.pixel(1, 1), Some([0, 0, 0, 0]), "{href}");
    }
    let data = format!("data:image/png;base64,{}", base64(&png));
    let p = dec(svg(&data).as_bytes(), "x.svg")
        .expect("renders")
        .pixels
        .expect("pixels");
    assert_eq!(p.pixel(1, 1), Some([255, 0, 0, 255]));
}

#[test]
fn an_svgz_is_refused_before_it_is_inflated() {
    let e = dec(&gzip(RED_SVG), "x.svg").expect_err("svgz");
    assert_eq!(e, DecodeError::Unsupported("SVGZ".into()));
    assert_eq!(e.to_string(), "SVGZ is not supported.");
}

#[test]
fn a_gzip_svg_in_a_data_url_is_not_drawn() {
    let svg = |href: String| {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="4" height="4"><image width="4" height="4" xlink:href="{href}"/></svg>"#
        )
    };
    let red = |href: String| {
        dec(svg(href).as_bytes(), "x.svg")
            .expect("renders")
            .pixels
            .expect("pixels")
            .pixel(1, 1)
    };
    let plain = base64(RED_SVG);
    assert_eq!(
        red(format!("data:image/svg+xml;base64,{plain}")),
        Some([255, 0, 0, 255]),
        "a plain SVG in a data: URL is drawn"
    );
    let packed = base64(&gzip(RED_SVG));
    for href in [
        format!("data:image/svg+xml;base64,{packed}"),
        // No type: usvg sniffs the bytes and tries an SVG.
        format!("data:;base64,{packed}"),
    ] {
        assert_eq!(red(href.clone()), Some([0, 0, 0, 0]), "{href}");
    }
}

#[test]
fn the_data_resolver_turns_gzip_data_away() {
    use std::sync::Arc;

    use resvg::usvg::{ImageKind, Options};

    use super::decode::svg_data_resolver;

    let resolve = svg_data_resolver();
    let options = Options::default();
    let packed = Arc::new(gzip(RED_SVG));
    for mime in ["image/svg+xml", "text/plain", "image/png"] {
        assert!(resolve(mime, packed.clone(), &options).is_none(), "{mime}");
    }
    let plain = resolve("image/svg+xml", Arc::new(RED_SVG.to_vec()), &options);
    assert!(
        matches!(plain, Some(ImageKind::SVG(_))),
        "a plain SVG resolves"
    );
}

#[test]
fn avif_heic_and_broken_files_are_refused_with_a_reason() {
    let e = dec(b"\0\0\0\x1cftypavif\0\0\0\0", "a.avif").expect_err("avif");
    assert_eq!(e, DecodeError::Unsupported("AVIF".into()));
    assert_eq!(e.to_string(), "AVIF is not supported.");
    let e = dec(b"\0\0\0\x18ftypheic\0\0\0\0", "a.heic").expect_err("heic");
    assert_eq!(e.to_string(), "HEIC is not supported.");

    let corrupt =
        "the picture can't be decoded: the format is not supported, or the file is damaged.";
    for (bytes, name) in [
        (&b"\x89PNG\r\n\x1a\nbroken"[..], "a.png"),
        (&b"just text"[..], "a.txt"),
    ] {
        let e = dec(bytes, name).expect_err("corrupt");
        assert_eq!(e, DecodeError::Corrupt);
        assert_eq!(e.to_string(), corrupt);
    }
    let e = dec(
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="5"/>"#,
        "a.svg",
    )
    .expect_err("empty");
    assert_eq!(e, DecodeError::Empty);
}

#[test]
fn a_picture_above_the_pixel_limit_gets_a_preview_not_pixels() {
    let limits = Limits {
        max_pixels: 100,
        preview_side: 8,
        ..Limits::default()
    };
    let go = |bytes: &[u8], name: &str, limits: &Limits| {
        decode(bytes, name, limits, &CancelToken::new())
    };
    let png = encode(&Picture::filled(20, 10, [9, 9, 9, 255]), ImageFormat::Png);
    let d = go(&png, "x.png", &limits).expect("decodes");
    assert!(d.big());
    assert_eq!((d.width, d.height), (20, 10));
    assert!(d.pixels.is_none());
    let preview = d.preview.as_ref().expect("preview");
    assert_eq!((preview.width, preview.height), (8, 4));
    assert_eq!(preview.pixel(0, 0), Some([9, 9, 9, 255]));
    assert_eq!(d.shown().width, 8);

    let ten = encode(&Picture::filled(10, 10, [9, 9, 9, 255]), ImageFormat::Png);
    let d = go(&ten, "x.png", &limits).expect("decodes");
    assert!(!d.big(), "exactly the limit is compared");
    assert!(d.pixels.is_some() && d.preview.is_none());

    let tight = Limits {
        max_decode_pixels: 150,
        ..limits
    };
    assert_eq!(
        go(&png, "x.png", &tight).expect_err("too large"),
        DecodeError::TooLarge(20, 10)
    );

    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10"/></svg>"#;
    let d = go(svg, "x.svg", &limits).expect("svg");
    assert_eq!((d.width, d.height), (20, 10));
    let preview = d.preview.expect("svg preview");
    assert_eq!((preview.width, preview.height), (8, 4));
    assert!(d.pixels.is_none());
}

#[test]
fn read_refuses_a_file_over_the_limit_before_reading_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("f.bin");
    std::fs::write(&path, b"hello world").expect("write");
    let small = Limits {
        max_file_bytes: 10,
        ..Limits::default()
    };
    assert!(matches!(
        read(&path, &small),
        Err(DecodeError::FileTooLarge(_))
    ));
    assert_eq!(read(&path, &Limits::default()).expect("reads").len(), 11);
    assert!(matches!(
        read(&dir.path().join("missing"), &Limits::default()),
        Err(DecodeError::Io(_))
    ));
    assert_eq!(
        DecodeError::FileTooLarge(256).to_string(),
        "the file is larger than 256 MB."
    );
}

#[test]
fn decode_stops_on_cancel() {
    let token = CancelToken::new();
    token.cancel();
    let png = encode(&sample(), ImageFormat::Png);
    let r = decode(&png, "x.png", &Limits::default(), &token);
    assert_eq!(r.expect_err("cancelled"), DecodeError::Cancelled);
}

#[test]
fn picture_names_are_recognised_by_extension() {
    for n in [
        "a.PNG", "a.jpg", "a.jpeg", "a.gif", "a.bmp", "a.webp", "a.ico", "a.tif", "a.tiff",
        "a.svg", "a.avif",
    ] {
        assert!(is_picture_name(Path::new(n)), "{n}");
    }
    for n in ["a.png.txt", "png", "a.svgz", "a.heic"] {
        assert!(!is_picture_name(Path::new(n)), "{n}");
    }
    assert_eq!(
        format!("{:?}", Picture::filled(4, 3, [0; 4])),
        "Picture 4×3"
    );
}
