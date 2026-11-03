use super::super::Picture;
use super::super::decode::{Limits, decode};
use super::*;
use crate::cancel::CancelToken;

#[test]
fn the_export_png_round_trips_and_names_are_safe() {
    let mut p = Picture::filled(3, 2, [10, 20, 30, 255]);
    p.set_pixel(1, 1, [255, 32, 96, 255]);
    p.set_pixel(2, 0, [0, 0, 0, 0]);
    let bytes = encode_png(&p).unwrap();
    assert!(bytes.starts_with(b"\x89PNG"));
    let back = decode(&bytes, "x.png", &Limits::default(), &CancelToken::new()).unwrap();
    assert_eq!(back.pixels.unwrap(), p);

    assert_eq!(
        export_name("my pic.png", "b:c?.svg"),
        "diff_my_pic_vs_b_c_.png"
    );
    assert_eq!(export_name("ภาพ ใหม่.jpeg", "x"), "diff_ภาพ_ใหม่_vs_x.png");
    assert_eq!(export_name("", "a.b.c.png"), "diff_image_vs_a.b.c.png");
    assert_eq!(export_name(".png", "x"), "diff_image_vs_x.png");
    let long = "a".repeat(80) + ".png";
    assert_eq!(
        export_name(&long, "x"),
        format!("diff_{}_vs_x.png", "a".repeat(60))
    );
}
