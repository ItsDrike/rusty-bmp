use std::{fs::File, io::Cursor};

use bmp::{
    raw::Bmp,
    runtime::{
        decode::{DecodedImage, decode_to_rgba},
        encode::encode_rgba_to_bmp,
    },
};

#[allow(dead_code)]
#[path = "bmpsuite/support.rs"]
mod support;

type SpotCase = (&'static str, [u8; 4], [u8; 4], [u8; 4]);

fn decode_bmpsuite(rel_path: &str) -> DecodedImage {
    let path = support::bmpsuite_root().join(rel_path);
    let mut file = File::open(&path).unwrap_or_else(|err| panic!("failed to open {}: {err}", path.display()));
    let bmp = Bmp::read_checked(&mut file).unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()));
    decode_to_rgba(&bmp).unwrap_or_else(|err| panic!("failed to decode {}: {err}", path.display()))
}

fn pixel_rgba(img: &DecodedImage, x: usize, y: usize) -> [u8; 4] {
    let idx = (y * img.width as usize + x) * 4;
    [img.rgba[idx], img.rgba[idx + 1], img.rgba[idx + 2], img.rgba[idx + 3]]
}

#[test]
fn decode_spot_checks_across_encodings() {
    support::require_suite_generated();

    let cases: [SpotCase; 5] = [
        // (file, pixel @ (0,0), pixel @ (10,10), pixel @ (120,60))
        ("g/rgb24.bmp", [255, 0, 0, 255], [215, 82, 82, 255], [99, 99, 123, 255]),
        (
            "g/pal8.bmp",
            [255, 0, 0, 255],
            [255, 85, 102, 255],
            [102, 128, 153, 255],
        ),
        (
            "g/pal8rle.bmp",
            [255, 0, 0, 255],
            [255, 85, 102, 255],
            [102, 128, 153, 255],
        ),
        (
            "g/rgb32bf.bmp",
            [255, 0, 0, 255],
            [215, 82, 82, 255],
            [99, 99, 123, 255],
        ),
        (
            "g/rgb16-565.bmp",
            [255, 0, 0, 255],
            [213, 80, 82, 255],
            [98, 97, 123, 255],
        ),
    ];

    for (rel, p00, p1010, p12060) in cases {
        let img = decode_bmpsuite(rel);
        assert_eq!((img.width, img.height), (127, 64), "{rel}");
        assert_eq!(pixel_rgba(&img, 0, 0), p00, "{rel} @ (0,0)");
        assert_eq!(pixel_rgba(&img, 10, 10), p1010, "{rel} @ (10,10)");
        assert_eq!(pixel_rgba(&img, 120, 60), p12060, "{rel} @ (120,60)");
        assert_eq!(pixel_rgba(&img, 40, 20), [0, 0, 0, 255], "{rel} @ (40,20)");
    }
}

#[test]
fn decode_rle_matches_uncompressed_palette_reference() {
    support::require_suite_generated();

    let pal8 = decode_bmpsuite("g/pal8.bmp");
    let pal8rle = decode_bmpsuite("g/pal8rle.bmp");
    assert_eq!(pal8.width, pal8rle.width);
    assert_eq!(pal8.height, pal8rle.height);
    assert_eq!(pal8.rgba, pal8rle.rgba);
}

#[test]
fn encode_decode_roundtrip_preserves_pixels() {
    let source = DecodedImage {
        width: 3,
        height: 2,
        rgba: vec![
            255, 0, 0, 255, // red
            0, 255, 0, 128, // green with alpha (alpha is expected to be dropped by BI_RGB 32 save)
            0, 0, 255, 64, // blue with alpha
            10, 20, 30, 255, // dark color
            200, 150, 100, 255, // warm
            250, 250, 250, 255, // near white
        ],
    };

    let bmp = encode_rgba_to_bmp(&source).expect("encode to bmp");

    let mut bytes = Cursor::new(Vec::<u8>::new());
    bmp.write_unchecked(&mut bytes).expect("write encoded bmp");
    bytes.set_position(0);

    let reparsed = Bmp::read_checked(&mut bytes).expect("read encoded bmp");
    let decoded = decode_to_rgba(&reparsed).expect("decode encoded bmp");

    assert_eq!(decoded.width, source.width);
    assert_eq!(decoded.height, source.height);

    // Saved format is BI_RGB 32bpp (B,G,R,reserved), so alpha is always 255 after roundtrip.
    let mut expected = source.rgba.clone();
    for px in expected.chunks_exact_mut(4) {
        px[3] = 255;
    }
    assert_eq!(decoded.rgba, expected);
}
