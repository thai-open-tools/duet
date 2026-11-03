use proptest::prelude::*;

use super::super::Picture;
use super::*;
use crate::cancel::{CancelToken, Cancelled};

#[test]
fn delta_is_the_largest_channel_difference_and_two_clear_pixels_match() {
    assert_eq!(delta([10, 20, 30, 255], [13, 20, 25, 255]), 5);
    assert_eq!(
        delta([0, 0, 0, 0], [255, 255, 255, 0]),
        0,
        "both fully transparent"
    );
    assert_eq!(delta([0, 0, 0, 0], [0, 0, 0, 1]), 1);
    assert_eq!(delta([0, 0, 0, 255], [255, 0, 0, 0]), 255);
}

#[test]
fn compare_counts_differences_within_and_above_the_tolerance() {
    let a = Picture::filled(4, 4, [100, 100, 100, 255]);
    let mut b = a.clone();
    b.set_pixel(0, 0, [103, 100, 100, 255]);
    b.set_pixel(3, 3, [100, 150, 100, 255]);
    let d = |tol| compare(&a, &b, tol, &CancelToken::new()).unwrap();
    assert_eq!(
        (d(0).count, d(0).minor, d(0).max_delta, d(0).total),
        (2, 0, 50, 16)
    );
    assert_eq!((d(3).count, d(3).minor), (1, 1));
    assert_eq!((d(50).count, d(50).minor), (0, 2));
    assert_eq!(
        (d(0).overlap, d(0).size, d(0).size_mismatch),
        ((4, 4), (4, 4), false)
    );
    assert!(d(50).is_identical() && !d(0).is_identical());
}

#[test]
fn groups_join_neighbouring_16px_cells_in_eight_directions() {
    let a = Picture::filled(64, 64, [0, 0, 0, 255]);
    let mut b = a.clone();
    for (x, y) in [(1, 1), (17, 17), (60, 5), (50, 50)] {
        b.set_pixel(x, y, [255, 0, 0, 255]);
    }
    assert_eq!(
        compare(&a, &b, 0, &CancelToken::new()).unwrap().clusters,
        [
            Cluster {
                x: 0,
                y: 0,
                width: 32,
                height: 32,
                cells: 2
            },
            Cluster {
                x: 48,
                y: 0,
                width: 16,
                height: 16,
                cells: 1
            },
            Cluster {
                x: 48,
                y: 48,
                width: 16,
                height: 16,
                cells: 1
            },
        ]
    );
}

#[test]
fn a_group_box_stops_at_the_overlap_edge() {
    let a = Picture::filled(20, 20, [0, 0, 0, 255]);
    let mut b = Picture::filled(30, 25, [0, 0, 0, 255]);
    b.set_pixel(18, 18, [1, 0, 0, 255]);
    let d = compare(&a, &b, 0, &CancelToken::new()).unwrap();
    assert_eq!(
        d.clusters,
        [Cluster {
            x: 16,
            y: 16,
            width: 4,
            height: 4,
            cells: 1
        }]
    );
    assert_eq!(
        (d.overlap, d.size, d.size_mismatch, d.total),
        ((20, 20), (30, 25), true, 400)
    );
}

#[test]
fn the_difference_image_matches_localdiff() {
    let (white, black) = ([255, 255, 255, 255], [0, 0, 0, 255]);
    let mut a = Picture::filled(3, 1, white);
    a.set_pixel(1, 0, black);
    let mut b = Picture::filled(2, 2, black);
    b.set_pixel(0, 0, white);
    b.set_pixel(1, 0, [5, 0, 0, 255]);
    let img = diff_image(&a, &b, 3, &CancelToken::new()).unwrap();
    assert_eq!((img.width, img.height), (3, 2));
    assert_eq!(
        img.pixel(0, 0),
        Some([102, 102, 102, 255]),
        "same: the left pixel's grey"
    );
    assert_eq!(img.pixel(1, 0), Some([255, 32, 96, 255]), "differs");
    assert_eq!(
        img.pixel(2, 0),
        Some([133, 107, 55, 255]),
        "left only, over white"
    );
    assert_eq!(
        img.pixel(0, 1),
        Some([104, 78, 26, 255]),
        "right only, over black"
    );
    assert_eq!(img.pixel(2, 1), Some([0, 0, 0, 0]), "on neither picture");
    assert_eq!(
        diff_image(&a, &b, 10, &CancelToken::new())
            .unwrap()
            .pixel(1, 0),
        Some([64, 110, 210, 255])
    );
    let clear = diff_image(
        &Picture::filled(1, 1, [0, 0, 0, 0]),
        &Picture::filled(1, 1, [9, 9, 9, 0]),
        0,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(
        clear.pixel(0, 0),
        Some([61, 61, 61, 255]),
        "two clear pixels match; a clear pixel's grey"
    );
}

#[test]
fn only_the_overlap_is_compared_when_sizes_differ() {
    let mut a = Picture::filled(4, 2, [255, 255, 255, 255]);
    a.set_pixel(3, 1, [0, 0, 0, 255]);
    let mut b = Picture::filled(2, 3, [255, 255, 255, 255]);
    b.set_pixel(0, 2, [0, 0, 0, 255]);
    let d = compare(&a, &b, 0, &CancelToken::new()).unwrap();
    assert_eq!(d.count, 0);
    assert_eq!(d.total, 4);
    assert_eq!((d.overlap, d.size), ((2, 2), (4, 3)));
    assert!(d.size_mismatch && !d.is_identical());
}

#[test]
fn a_1x100_and_100x1_pair_share_one_pixel() {
    let a = Picture::filled(1, 100, [255, 255, 255, 255]);
    let b = Picture::filled(100, 1, [255, 255, 255, 255]);
    let token = CancelToken::new();
    let d = compare(&a, &b, 0, &token).unwrap();
    assert_eq!((d.total, d.overlap, d.size), (1, (1, 1), (100, 100)));
    assert!(d.size_mismatch);
    let img = diff_image(&a, &b, 0, &token).unwrap();
    assert_eq!((img.width, img.height), (100, 100));
    // 255 white: lum 255, v 255, grey 20 + 255 * 0.32 = 101.6 -> 102
    assert_eq!(img.pixel(0, 0), Some([102, 102, 102, 255]));
    let amber = img.pixel(0, 50).unwrap();
    assert_eq!(amber[3], 255);
    assert_eq!(img.pixel(50, 0), Some(amber));
    assert_ne!(amber, [102, 102, 102, 255]);
    assert_eq!(img.pixel(50, 50), Some([0, 0, 0, 0]));
}

#[test]
fn compare_and_the_image_stop_on_cancel() {
    let a = Picture::filled(40, 40, [0, 0, 0, 255]);
    let token = CancelToken::new();
    token.cancel();
    assert_eq!(compare(&a, &a, 0, &token), Err(Cancelled));
    assert_eq!(diff_image(&a, &a, 0, &token).map(|_| ()), Err(Cancelled));
}

#[test]
fn percent_and_colour_texts_follow_localdiff() {
    assert_eq!(percent_text(0, 0), "0%");
    assert_eq!(percent_text(1, 1_000_000), "<0.01%");
    assert_eq!(percent_text(1, 3), "33.33%");
    assert_eq!(percent_text(1, 16), "6.25%");
    assert_eq!(percent_text(0, 5), "0.00%");
    assert_eq!(hex_color([255, 0, 16, 255]), "#ff0010");
    assert_eq!(hex_color([1, 2, 3, 128]), "#01020380");
}

fn random_picture(w: u32, h: u32, seed: u64) -> Picture {
    const LEVELS: [u8; 5] = [0, 1, 2, 128, 255];
    let mut s = seed | 1;
    let rgba = (0..w as usize * h as usize * 4)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            LEVELS[(s % 5) as usize]
        })
        .collect();
    Picture::new(w, h, rgba)
}

proptest! {
    #[test]
    fn counts_add_up_and_every_counted_pixel_lies_in_a_group(
        aw in 1u32..40, ah in 1u32..40, bw in 1u32..40, bh in 1u32..40,
        seed in any::<u64>(), tol in any::<u8>(),
    ) {
        let a = random_picture(aw, ah, seed);
        let b = random_picture(bw, bh, seed.wrapping_add(7));
        let token = CancelToken::new();
        let d = compare(&a, &b, tol, &token).unwrap();
        let (cw, ch) = (aw.min(bw), ah.min(bh));
        prop_assert_eq!(d.overlap, (cw, ch));
        let mut same = 0u64;
        for y in 0..ch {
            for x in 0..cw {
                let v = delta(a.pixel(x, y).unwrap(), b.pixel(x, y).unwrap());
                if v == 0 { same += 1; }
                if v > tol {
                    let inside = d.clusters.iter().any(|c| {
                        x >= c.x && x < c.x + c.width && y >= c.y && y < c.y + c.height
                    });
                    prop_assert!(inside);
                }
            }
        }
        prop_assert_eq!(d.count + d.minor + same, d.total);
        prop_assert_eq!(d.total, cw as u64 * ch as u64);
        for c in &d.clusters {
            prop_assert!(c.cells >= 1 && c.width > 0 && c.height > 0);
            prop_assert!(c.x + c.width <= cw && c.y + c.height <= ch);
        }
        prop_assert!(d.clusters.windows(2).all(|w| (w[0].y, w[0].x) < (w[1].y, w[1].x)));
        let img = diff_image(&a, &b, tol, &token).unwrap();
        prop_assert_eq!((img.width, img.height), (aw.max(bw), ah.max(bh)));
        for y in 0..ch {
            for x in 0..cw {
                let v = delta(a.pixel(x, y).unwrap(), b.pixel(x, y).unwrap());
                let p = img.pixel(x, y).unwrap();
                prop_assert_eq!(p == [C_DIFF[0], C_DIFF[1], C_DIFF[2], 255], v > tol);
            }
        }
    }
}
