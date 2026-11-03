//! Picture format detection: magic bytes first, then the extension (LocalDiff `imSniff`).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Gif,
    Webp,
    Ico,
    Avif,
    Heic,
    Tiff,
    Bmp,
    Svg,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
            Format::Gif => "GIF",
            Format::Webp => "WEBP",
            Format::Ico => "ICO",
            Format::Avif => "AVIF",
            Format::Heic => "HEIC",
            Format::Tiff => "TIFF",
            Format::Bmp => "BMP",
            Format::Svg => "SVG",
        }
    }
}

fn extension(name: &str) -> Option<String> {
    let (_, ext) = name.rsplit_once('.')?;
    if ext.is_empty() || ext.contains(['\\', '/']) {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

fn by_magic(b: &[u8]) -> Option<Format> {
    if b.len() < 4 {
        return None;
    }
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(Format::Png)
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Format::Jpeg)
    } else if b.starts_with(b"GIF8") {
        Some(Format::Gif)
    } else if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some(Format::Webp)
    } else if b[0] == 0 && b[1] == 0 && (b[2] == 1 || b[2] == 2) && b[3] == 0 {
        Some(Format::Ico)
    } else if b.len() >= 12 && &b[4..8] == b"ftyp" {
        match &b[8..12] {
            b"avif" | b"avis" => Some(Format::Avif),
            b"heic" | b"heix" | b"mif1" | b"msf1" | b"hevc" => Some(Format::Heic),
            _ => None,
        }
    } else if b.starts_with(&[b'I', b'I', 0x2A, 0]) || b.starts_with(&[b'M', b'M', 0, 0x2A]) {
        Some(Format::Tiff)
    } else if b.starts_with(b"BM") {
        Some(Format::Bmp)
    } else {
        None
    }
}

/// `<svg` followed by whitespace or `>`, ignoring case, in the first 4096 bytes.
fn looks_like_svg(b: &[u8]) -> bool {
    let head = &b[..b.len().min(4096)];
    head.windows(5).any(|w| {
        w[..4].eq_ignore_ascii_case(b"<svg") && (w[4].is_ascii_whitespace() || w[4] == b'>')
    })
}

pub fn sniff(bytes: &[u8], name: &str) -> Option<Format> {
    if let Some(f) = by_magic(bytes) {
        return Some(f);
    }
    if looks_like_svg(bytes) {
        return Some(Format::Svg);
    }
    Some(match extension(name)?.as_str() {
        "jpg" | "jpeg" => Format::Jpeg,
        "png" => Format::Png,
        "gif" => Format::Gif,
        "bmp" => Format::Bmp,
        "webp" => Format::Webp,
        "ico" | "cur" => Format::Ico,
        "avif" => Format::Avif,
        "svg" => Format::Svg,
        "tif" | "tiff" => Format::Tiff,
        "heic" => Format::Heic,
        _ => return None,
    })
}

/// The format's label, else the extension in capitals, else "?".
pub fn format_label(bytes: &[u8], name: &str) -> String {
    match sniff(bytes, name) {
        Some(f) => f.label().to_string(),
        None => extension(name).map_or_else(|| "?".to_string(), |e| e.to_ascii_uppercase()),
    }
}
