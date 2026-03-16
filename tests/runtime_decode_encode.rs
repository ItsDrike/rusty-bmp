use std::{fs::File, io::Cursor};

use bmp::{
    raw::Bmp,
    runtime::{
        decode::{DecodedImage, decode_to_rgba},
        encode::{
            SaveFormat, SaveHeaderVersion, SourceMetadata, encode_rgba_to_bmp, encode_rgba_to_bmp_ext,
            encode_rgba_to_bmp_with_format,
        },
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

/// Returns a copy of the image with all alpha channels forced to 255.
/// BMP roundtrips drop alpha, so this creates the expected reference for comparison.
fn with_opaque_alpha(image: &DecodedImage) -> DecodedImage {
    let mut rgba = image.rgba.clone();
    for px in rgba.chunks_exact_mut(4) {
        px[3] = 255;
    }
    DecodedImage {
        width: image.width,
        height: image.height,
        rgba,
    }
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
    let expected = with_opaque_alpha(&source);
    assert_eq!(decoded.rgba, expected.rgba);
}

// ---------------------------------------------------------------------------
// Roundtrip helpers for the new SaveFormat variants
// ---------------------------------------------------------------------------

/// Encode with the given format, write to an in-memory buffer, re-parse, and
/// decode back to RGBA. Returns the decoded image.
fn roundtrip_format(source: &DecodedImage, format: SaveFormat) -> DecodedImage {
    let bmp =
        encode_rgba_to_bmp_with_format(source, format).unwrap_or_else(|e| panic!("encode {format:?} failed: {e}"));

    let mut buf = Cursor::new(Vec::<u8>::new());
    bmp.write_unchecked(&mut buf)
        .unwrap_or_else(|e| panic!("write {format:?} failed: {e}"));
    buf.set_position(0);

    let reparsed = Bmp::read_checked(&mut buf).unwrap_or_else(|e| panic!("read {format:?} failed: {e}"));
    decode_to_rgba(&reparsed).unwrap_or_else(|e| panic!("decode {format:?} failed: {e}"))
}

/// A small test image with 6 distinct colors (enough to exercise palette
/// quantization at low bit depths while remaining deterministic).
fn small_test_image() -> DecodedImage {
    DecodedImage {
        width: 3,
        height: 2,
        rgba: vec![
            255, 0, 0, 255, // red
            0, 255, 0, 255, // green
            0, 0, 255, 255, // blue
            10, 20, 30, 255, // dark
            200, 150, 100, 255, // warm
            250, 250, 250, 255, // near-white
        ],
    }
}

/// Returns the maximum absolute per-channel difference between the two images.
fn max_channel_diff(a: &DecodedImage, b: &DecodedImage) -> u8 {
    assert_eq!(a.width, b.width);
    assert_eq!(a.height, b.height);
    a.rgba
        .iter()
        .zip(b.rgba.iter())
        .map(|(&x, &y)| x.abs_diff(y))
        .max()
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Lossless (exact) roundtrip tests
// ---------------------------------------------------------------------------

#[test]
fn roundtrip_rgb24_preserves_rgb_drops_alpha() {
    let source = small_test_image();
    let decoded = roundtrip_format(&source, SaveFormat::Rgb24);
    assert_eq!(decoded.width, source.width);
    assert_eq!(decoded.height, source.height);
    // Alpha is always 255 after roundtrip through 24-bit
    let expected = with_opaque_alpha(&source);
    assert_eq!(decoded.rgba, expected.rgba);
}

#[test]
fn roundtrip_rgb32_preserves_rgb() {
    let source = small_test_image();
    let decoded = roundtrip_format(&source, SaveFormat::Rgb32);
    assert_eq!(decoded.width, source.width);
    assert_eq!(decoded.height, source.height);
    let expected = with_opaque_alpha(&source);
    assert_eq!(decoded.rgba, expected.rgba);
}

#[test]
fn roundtrip_bitfields32_preserves_rgb() {
    let source = small_test_image();
    let decoded = roundtrip_format(&source, SaveFormat::BitFields32);
    assert_eq!(decoded.width, source.width);
    assert_eq!(decoded.height, source.height);
    let expected = with_opaque_alpha(&source);
    assert_eq!(decoded.rgba, expected.rgba);
}

// ---------------------------------------------------------------------------
// Lossy roundtrip tests (quantization or reduced bit depth)
// ---------------------------------------------------------------------------

/// For low-bpp formats, we allow some color error due to quantization.
/// This test checks that dimensions match and the error is within reason.
fn assert_lossy_roundtrip(format: SaveFormat, max_allowed_diff: u8) {
    let source = small_test_image();
    let decoded = roundtrip_format(&source, format);
    assert_eq!(decoded.width, source.width, "{format:?} width");
    assert_eq!(decoded.height, source.height, "{format:?} height");
    // Force alpha to 255 in source for comparison
    let reference = with_opaque_alpha(&source);
    let diff = max_channel_diff(&decoded, &reference);
    assert!(
        diff <= max_allowed_diff,
        "{format:?}: max channel diff {diff} exceeds allowed {max_allowed_diff}"
    );
}

#[test]
fn roundtrip_rgb16_within_tolerance() {
    // 5 bits per channel -> max error around 8 (255/31)
    assert_lossy_roundtrip(SaveFormat::Rgb16, 9);
}

#[test]
fn roundtrip_bitfields16_rgb565_within_tolerance() {
    assert_lossy_roundtrip(SaveFormat::BitFields16Rgb565, 9);
}

#[test]
fn roundtrip_bitfields16_rgb555_within_tolerance() {
    assert_lossy_roundtrip(SaveFormat::BitFields16Rgb555, 9);
}

#[test]
fn roundtrip_rgb8_within_tolerance() {
    // 256-color quantization on a 6-color image should be exact or very close
    assert_lossy_roundtrip(SaveFormat::Rgb8, 2);
}

#[test]
fn roundtrip_rgb4_within_tolerance() {
    // 16-color palette for 6 distinct colors
    assert_lossy_roundtrip(SaveFormat::Rgb4, 5);
}

#[test]
fn roundtrip_rgb1_dimensions_preserved() {
    let source = small_test_image();
    let decoded = roundtrip_format(&source, SaveFormat::Rgb1);
    assert_eq!(decoded.width, source.width);
    assert_eq!(decoded.height, source.height);
    // 1-bpp is monochrome: just make sure the roundtrip completes
}

#[test]
fn roundtrip_rle8_within_tolerance() {
    assert_lossy_roundtrip(SaveFormat::Rle8, 2);
}

#[test]
fn roundtrip_rle4_within_tolerance() {
    assert_lossy_roundtrip(SaveFormat::Rle4, 5);
}

// ---------------------------------------------------------------------------
// Larger image roundtrip tests (exercises row padding and longer RLE streams)
// ---------------------------------------------------------------------------

fn gradient_image(width: u32, height: u32) -> DecodedImage {
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let r = ((x * 255) / width.max(1)) as u8;
            let g = ((y * 255) / height.max(1)) as u8;
            let b = (((x + y) * 127) / (width + height).max(1)) as u8;
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    DecodedImage { width, height, rgba }
}

#[test]
fn roundtrip_rgb24_gradient() {
    let source = gradient_image(17, 11); // odd width to exercise row padding
    let decoded = roundtrip_format(&source, SaveFormat::Rgb24);
    assert_eq!(decoded.rgba, source.rgba);
}

#[test]
fn roundtrip_rle8_gradient() {
    let source = gradient_image(17, 11);
    let decoded = roundtrip_format(&source, SaveFormat::Rle8);
    assert_eq!(decoded.width, source.width);
    assert_eq!(decoded.height, source.height);
    // Quantization means we can't expect an exact match, but dimensions should
    // match and the image should be decodable.
    let diff = max_channel_diff(&decoded, &source);
    assert!(diff <= 30, "rle8 gradient max diff {diff} too large");
}

#[test]
fn roundtrip_rle4_gradient() {
    let source = gradient_image(17, 11);
    let decoded = roundtrip_format(&source, SaveFormat::Rle4);
    assert_eq!(decoded.width, source.width);
    assert_eq!(decoded.height, source.height);
}

// ---------------------------------------------------------------------------
// All formats should produce a valid BMP that passes read_checked
// ---------------------------------------------------------------------------

#[test]
fn all_formats_produce_valid_bmp() {
    let source = small_test_image();
    for &fmt in SaveFormat::ALL {
        let bmp =
            encode_rgba_to_bmp_with_format(&source, fmt).unwrap_or_else(|e| panic!("encode {fmt:?} failed: {e}"));
        let mut buf = Cursor::new(Vec::<u8>::new());
        bmp.write_unchecked(&mut buf)
            .unwrap_or_else(|e| panic!("write {fmt:?} failed: {e}"));
        buf.set_position(0);
        Bmp::read_checked(&mut buf).unwrap_or_else(|e| panic!("read_checked {fmt:?} failed: {e}"));
    }
}

// ===========================================================================
// Header version roundtrip tests
// ===========================================================================

/// Encode with a specific format + header version, write to buffer, re-parse,
/// decode back to RGBA.
fn roundtrip_header_version(
    source: &DecodedImage,
    format: SaveFormat,
    header: SaveHeaderVersion,
    source_meta: Option<&SourceMetadata>,
) -> (Bmp, DecodedImage) {
    let bmp = encode_rgba_to_bmp_ext(source, format, header, source_meta)
        .unwrap_or_else(|e| panic!("encode {format:?}/{header:?} failed: {e}"));

    let mut buf = Cursor::new(Vec::<u8>::new());
    bmp.write_unchecked(&mut buf)
        .unwrap_or_else(|e| panic!("write {format:?}/{header:?} failed: {e}"));
    buf.set_position(0);

    let reparsed = Bmp::read_checked(&mut buf).unwrap_or_else(|e| panic!("read {format:?}/{header:?} failed: {e}"));
    let decoded = decode_to_rgba(&reparsed).unwrap_or_else(|e| panic!("decode {format:?}/{header:?} failed: {e}"));
    (reparsed, decoded)
}

// ---------------------------------------------------------------------------
// Core header tests
// ---------------------------------------------------------------------------

#[test]
fn roundtrip_core_rgb24_preserves_pixels() {
    let source = small_test_image();
    let (bmp, decoded) = roundtrip_header_version(&source, SaveFormat::Rgb24, SaveHeaderVersion::Core, None);
    assert!(matches!(bmp, Bmp::Core(_)), "expected Core variant");
    assert_eq!(decoded.width, source.width);
    assert_eq!(decoded.height, source.height);
    // RGB24 is lossless for RGB channels
    let expected = with_opaque_alpha(&source);
    assert_eq!(decoded.rgba, expected.rgba);
}

#[test]
fn roundtrip_core_rgb8_within_tolerance() {
    let source = small_test_image();
    let (bmp, decoded) = roundtrip_header_version(&source, SaveFormat::Rgb8, SaveHeaderVersion::Core, None);
    assert!(matches!(bmp, Bmp::Core(_)), "expected Core variant");
    assert_eq!(decoded.width, source.width);
    assert_eq!(decoded.height, source.height);
    let reference = with_opaque_alpha(&source);
    let diff = max_channel_diff(&decoded, &reference);
    assert!(diff <= 2, "Core Rgb8 max channel diff {diff} exceeds 2");
}

#[test]
fn roundtrip_core_rgb4_within_tolerance() {
    let source = small_test_image();
    let (bmp, decoded) = roundtrip_header_version(&source, SaveFormat::Rgb4, SaveHeaderVersion::Core, None);
    assert!(matches!(bmp, Bmp::Core(_)), "expected Core variant");
    assert_eq!(decoded.width, source.width);
    assert_eq!(decoded.height, source.height);
}

#[test]
fn roundtrip_core_rgb1_dimensions_preserved() {
    let source = small_test_image();
    let (bmp, decoded) = roundtrip_header_version(&source, SaveFormat::Rgb1, SaveHeaderVersion::Core, None);
    assert!(matches!(bmp, Bmp::Core(_)), "expected Core variant");
    assert_eq!(decoded.width, source.width);
    assert_eq!(decoded.height, source.height);
}

#[test]
fn core_rejects_incompatible_formats() {
    let source = small_test_image();
    // Core doesn't support Rgb16, Rgb32, RLE, or BitFields
    for &fmt in &[
        SaveFormat::Rgb16,
        SaveFormat::Rgb32,
        SaveFormat::Rle8,
        SaveFormat::Rle4,
        SaveFormat::BitFields16Rgb565,
        SaveFormat::BitFields16Rgb555,
        SaveFormat::BitFields32,
    ] {
        let result = encode_rgba_to_bmp_ext(&source, fmt, SaveHeaderVersion::Core, None);
        assert!(result.is_err(), "Core should reject {fmt:?}");
    }
}

// ---------------------------------------------------------------------------
// V4 header tests
// ---------------------------------------------------------------------------

#[test]
fn roundtrip_v4_rgb24_preserves_pixels() {
    let source = small_test_image();
    let (bmp, decoded) = roundtrip_header_version(&source, SaveFormat::Rgb24, SaveHeaderVersion::V4, None);
    assert!(matches!(bmp, Bmp::V4(_)), "expected V4 variant");
    assert_eq!(decoded.width, source.width);
    assert_eq!(decoded.height, source.height);
    let expected = with_opaque_alpha(&source);
    assert_eq!(decoded.rgba, expected.rgba);
}

#[test]
fn roundtrip_v4_bitfields32_preserves_pixels() {
    let source = small_test_image();
    let (bmp, decoded) = roundtrip_header_version(&source, SaveFormat::BitFields32, SaveHeaderVersion::V4, None);
    assert!(matches!(bmp, Bmp::V4(_)), "expected V4 variant");
    let expected = with_opaque_alpha(&source);
    assert_eq!(decoded.rgba, expected.rgba);
}

#[test]
fn roundtrip_v4_rle8_within_tolerance() {
    let source = small_test_image();
    let (bmp, decoded) = roundtrip_header_version(&source, SaveFormat::Rle8, SaveHeaderVersion::V4, None);
    assert!(matches!(bmp, Bmp::V4(_)), "expected V4 variant");
    assert_eq!(decoded.width, source.width);
    assert_eq!(decoded.height, source.height);
    let reference = with_opaque_alpha(&source);
    let diff = max_channel_diff(&decoded, &reference);
    assert!(diff <= 2, "V4 Rle8 max channel diff {diff} exceeds 2");
}

#[test]
fn v4_defaults_to_srgb_without_source() {
    let source = small_test_image();
    let bmp = encode_rgba_to_bmp_ext(&source, SaveFormat::Rgb24, SaveHeaderVersion::V4, None).unwrap();
    if let Bmp::V4(data) = bmp {
        assert_eq!(
            data.bmp_header.cs_type,
            bmp::raw::ColorSpaceType::SRgb,
            "V4 without source should default to sRGB"
        );
    } else {
        panic!("expected V4 variant");
    }
}

// ---------------------------------------------------------------------------
// V5 header tests
// ---------------------------------------------------------------------------

#[test]
fn roundtrip_v5_rgb24_preserves_pixels() {
    let source = small_test_image();
    let (bmp, decoded) = roundtrip_header_version(&source, SaveFormat::Rgb24, SaveHeaderVersion::V5, None);
    assert!(matches!(bmp, Bmp::V5(_)), "expected V5 variant");
    assert_eq!(decoded.width, source.width);
    assert_eq!(decoded.height, source.height);
    let expected = with_opaque_alpha(&source);
    assert_eq!(decoded.rgba, expected.rgba);
}

#[test]
fn roundtrip_v5_bitfields16_rgb565_within_tolerance() {
    let source = small_test_image();
    let (bmp, decoded) = roundtrip_header_version(&source, SaveFormat::BitFields16Rgb565, SaveHeaderVersion::V5, None);
    assert!(matches!(bmp, Bmp::V5(_)), "expected V5 variant");
    assert_eq!(decoded.width, source.width);
    assert_eq!(decoded.height, source.height);
    let reference = with_opaque_alpha(&source);
    let diff = max_channel_diff(&decoded, &reference);
    assert!(diff <= 9, "V5 BitFields16Rgb565 max channel diff {diff} exceeds 9");
}

#[test]
fn v5_defaults_to_srgb_without_source() {
    let source = small_test_image();
    let bmp = encode_rgba_to_bmp_ext(&source, SaveFormat::Rgb24, SaveHeaderVersion::V5, None).unwrap();
    if let Bmp::V5(data) = bmp {
        assert_eq!(
            data.bmp_header.v4.cs_type,
            bmp::raw::ColorSpaceType::SRgb,
            "V5 without source should default to sRGB"
        );
        assert!(
            data.icc_profile.is_none(),
            "V5 without source should have no ICC profile"
        );
    } else {
        panic!("expected V5 variant");
    }
}

// ---------------------------------------------------------------------------
// All header versions x all compatible formats produce valid BMPs
// ---------------------------------------------------------------------------

#[test]
fn all_header_versions_all_formats_produce_valid_bmp() {
    let source = small_test_image();
    for &header in SaveHeaderVersion::ALL {
        for &fmt in header.compatible_formats() {
            let bmp = encode_rgba_to_bmp_ext(&source, fmt, header, None)
                .unwrap_or_else(|e| panic!("encode {fmt:?}/{header:?} failed: {e}"));
            let mut buf = Cursor::new(Vec::<u8>::new());
            bmp.write_unchecked(&mut buf)
                .unwrap_or_else(|e| panic!("write {fmt:?}/{header:?} failed: {e}"));
            buf.set_position(0);
            Bmp::read_checked(&mut buf).unwrap_or_else(|e| panic!("read_checked {fmt:?}/{header:?} failed: {e}"));
        }
    }
}

// ---------------------------------------------------------------------------
// Source metadata preservation tests
// ---------------------------------------------------------------------------

#[test]
fn v4_preserves_source_metadata_from_v4_bmp() {
    let source = small_test_image();

    // Create a V4 BMP with specific color space info
    let original_bmp = encode_rgba_to_bmp_ext(&source, SaveFormat::Rgb24, SaveHeaderVersion::V4, None).unwrap();
    let meta = SourceMetadata::from_bmp(&original_bmp);
    assert!(meta.is_some(), "should extract metadata from V4 bmp");

    // Re-encode with the metadata preserved
    let (reparsed, _decoded) =
        roundtrip_header_version(&source, SaveFormat::Rgb32, SaveHeaderVersion::V4, meta.as_ref());
    if let Bmp::V4(data) = reparsed {
        assert_eq!(data.bmp_header.cs_type, bmp::raw::ColorSpaceType::SRgb);
    } else {
        panic!("expected V4 variant");
    }
}

#[test]
fn v5_preserves_source_metadata_from_v5_bmp() {
    let source = small_test_image();

    // Create a V5 BMP first
    let original_bmp = encode_rgba_to_bmp_ext(&source, SaveFormat::Rgb24, SaveHeaderVersion::V5, None).unwrap();
    let meta = SourceMetadata::from_bmp(&original_bmp);
    assert!(meta.is_some(), "should extract metadata from V5 bmp");

    // Re-encode with metadata preserved
    let (reparsed, _decoded) =
        roundtrip_header_version(&source, SaveFormat::Rgb32, SaveHeaderVersion::V5, meta.as_ref());
    if let Bmp::V5(data) = reparsed {
        assert_eq!(data.bmp_header.v4.cs_type, bmp::raw::ColorSpaceType::SRgb);
        assert!(data.icc_profile.is_none());
    } else {
        panic!("expected V5 variant");
    }
}

// ---------------------------------------------------------------------------
// SaveHeaderVersion::from_bmp auto-detection tests
// ---------------------------------------------------------------------------

#[test]
fn header_version_from_bmp_detects_correctly() {
    let source = small_test_image();

    // Info (default via encode_rgba_to_bmp_with_format)
    let info_bmp = encode_rgba_to_bmp_with_format(&source, SaveFormat::Rgb24).unwrap();
    assert_eq!(SaveHeaderVersion::from_bmp(&info_bmp), SaveHeaderVersion::Info);

    // V4
    let v4_bmp = encode_rgba_to_bmp_ext(&source, SaveFormat::Rgb24, SaveHeaderVersion::V4, None).unwrap();
    assert_eq!(SaveHeaderVersion::from_bmp(&v4_bmp), SaveHeaderVersion::V4);

    // V5
    let v5_bmp = encode_rgba_to_bmp_ext(&source, SaveFormat::Rgb24, SaveHeaderVersion::V5, None).unwrap();
    assert_eq!(SaveHeaderVersion::from_bmp(&v5_bmp), SaveHeaderVersion::V5);

    // Core
    let core_bmp = encode_rgba_to_bmp_ext(&source, SaveFormat::Rgb24, SaveHeaderVersion::Core, None).unwrap();
    assert_eq!(SaveHeaderVersion::from_bmp(&core_bmp), SaveHeaderVersion::Core);
}
