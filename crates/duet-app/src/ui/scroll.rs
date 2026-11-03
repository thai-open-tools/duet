//! Scrollbar thumb maths shared by the views with a custom viewport (hex, table).

/// The shortest scrollbar thumb, in px.
pub const MIN_THUMB: f64 = 24.;

/// A thumb's start and length along a track `track` px long, for a view of `shown` of `total`
/// units scrolled `at` units in: rows on the vertical bar, px on the horizontal one. In f64,
/// so a 4 GiB pair's rows never drift.
pub fn thumb(track: f32, total: u64, shown: u64, at: u64) -> (f32, f32) {
    let track = f64::from(track);
    if total <= shown {
        return (0., track as f32);
    }
    let length = (track * shown as f64 / total as f64)
        .max(MIN_THUMB)
        .min(track);
    let room = (total - shown) as f64;
    let start = (track - length) * (at as f64 / room).min(1.);
    (start as f32, length as f32)
}

/// The scroll, in units, that puts the thumb's start `start` px along the track.
pub fn scroll_for_thumb(track: f32, total: u64, shown: u64, start: f32) -> u64 {
    if total <= shown {
        return 0;
    }
    let (_, length) = thumb(track, total, shown, 0);
    let room = f64::from(track) - f64::from(length);
    let max = total - shown;
    if room <= 0. {
        return 0;
    }
    let fraction = (f64::from(start) / room).clamp(0., 1.);
    ((fraction * max as f64).round() as u64).min(max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thumb_maps_rows_both_ways_even_for_huge_files() {
        let rows = (4u64 << 30) / 16;
        let (y, h) = thumb(600., rows, 30, 0);
        assert_eq!((y, h), (0., 24.), "a long file gets the shortest thumb");
        let (y, _) = thumb(600., rows, 30, rows - 30);
        assert_eq!(y, 576., "the last page puts the thumb at the end");
        assert_eq!(scroll_for_thumb(600., rows, 30, 576.), rows - 30);
        assert_eq!(scroll_for_thumb(600., rows, 30, 9_000.), rows - 30);
        assert_eq!(scroll_for_thumb(600., rows, 30, -5.), 0);
        let huge = u64::MAX / 8;
        assert_eq!(
            scroll_for_thumb(600., huge, 30, 600.),
            huge - 30,
            "no f64 overshoot"
        );
        assert_eq!(thumb(600., 10, 30, 0), (0., 600.), "everything fits");
        assert_eq!(scroll_for_thumb(600., 10, 30, 300.), 0);
        // The horizontal bar maps px the same way: a 2,000 px row in a 1,000 px body.
        assert_eq!(thumb(1000., 2000, 1000, 1000), (500., 500.));
        assert_eq!(scroll_for_thumb(1000., 2000, 1000, 250.), 500);
    }
}
