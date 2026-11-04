//! The app icon in the window header: the pictures `scripts/make-icon.py` draws for each size,
//! drawn one texel to one device pixel, so the header shows the taskbar's icon and stays sharp.

use std::sync::{Arc, OnceLock};

use gpui_kit::{Pixels, RenderImage, px};

use crate::picture::textures::render_image;

/// The icon's side at scale 1, in px.
const SIDE: f32 = 26.;

const FRAMES: [(u32, &[u8]); 9] = [
    (26, include_bytes!("../../assets/header/duet-26.png")),
    (32, include_bytes!("../../assets/header/duet-32.png")),
    (40, include_bytes!("../../assets/header/duet-40.png")),
    (46, include_bytes!("../../assets/header/duet-46.png")),
    (52, include_bytes!("../../assets/header/duet-52.png")),
    (58, include_bytes!("../../assets/header/duet-58.png")),
    (64, include_bytes!("../../assets/header/duet-64.png")),
    (78, include_bytes!("../../assets/header/duet-78.png")),
    (92, include_bytes!("../../assets/header/duet-92.png")),
];

fn frame_index(scale: f32) -> usize {
    let want = SIDE * scale;
    let distance = |i: usize| (FRAMES[i].0 as f32 - want).abs();
    (0..FRAMES.len())
        .min_by(|&a, &b| distance(a).total_cmp(&distance(b)))
        .unwrap_or(0)
}

/// The side in device pixels of the picture drawn at `scale`.
pub fn brand_frame(scale: f32) -> u32 {
    FRAMES[frame_index(scale)].0
}

/// The size that puts the picture of `scale` on exactly its own count of device pixels.
pub fn brand_size(scale: f32) -> Pixels {
    px(brand_frame(scale) as f32 / scale)
}

/// The picture for `scale`, decoded once.
pub fn brand_image(scale: f32) -> Arc<RenderImage> {
    static IMAGES: [OnceLock<Arc<RenderImage>>; FRAMES.len()] =
        [const { OnceLock::new() }; FRAMES.len()];
    let index = frame_index(scale);
    IMAGES[index]
        .get_or_init(|| {
            let (side, bytes) = FRAMES[index];
            let rgba = image::load_from_memory(bytes)
                .expect("an embedded icon decodes")
                .to_rgba8();
            debug_assert_eq!(rgba.width(), side);
            render_image(rgba.width(), rgba.height(), rgba.as_raw())
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_frame_is_the_size_it_says_with_a_clear_corner() {
        for (side, bytes) in FRAMES {
            let rgba = image::load_from_memory(bytes).unwrap().to_rgba8();
            assert_eq!((rgba.width(), rgba.height()), (side, side));
            assert_eq!(rgba.get_pixel(0, 0)[3], 0, "the {side} px frame's corner");
            let centre = rgba.get_pixel(side / 2, side / 2);
            assert_eq!(centre[3], 255, "the {side} px frame's centre");
        }
    }
}
