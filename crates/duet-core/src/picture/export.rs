//! Exporting the difference image as a PNG.

use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};

use super::Picture;

pub fn encode_png(picture: &Picture) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(
            &picture.rgba,
            picture.width,
            picture.height,
            ExtendedColorType::Rgba8,
        )
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// `diff_{left stem}_vs_{right stem}.png` (R13).
pub fn export_name(left: &str, right: &str) -> String {
    format!("diff_{}_vs_{}.png", stem(left), stem(right))
}

/// The name without its last extension, unsafe runs as `_`, at most 60 chars, `image` if empty.
fn stem(name: &str) -> String {
    let base = name.rfind('.').map_or(name, |i| &name[..i]);
    let mut out = String::new();
    let mut in_run = false;
    for c in base.chars() {
        if c.is_whitespace() || "\\/:*?\"<>|".contains(c) {
            if !in_run {
                out.push('_');
            }
            in_run = true;
        } else {
            out.push(c);
            in_run = false;
        }
    }
    let out: String = out.chars().take(60).collect();
    if out.is_empty() {
        "image".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests;
