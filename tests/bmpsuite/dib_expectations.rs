use bmp::raw::{BitsPerPixel, Bmp, Compression};
use rstest::rstest;

use super::support::{bmpsuite_root, parse_bmp, require_suite_generated};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DibVariant {
    Core,
    Info,
    V4,
    V5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DibExpectation {
    variant: DibVariant,
    width: i32,
    height: i32,
    bpp: BitsPerPixel,
    compression: Compression,
    color_table_len: usize,
    info_masks: Option<[u32; 3]>,
}

#[allow(clippy::cognitive_complexity)]
fn assert_dib_expectation(rel_path: &str, expected: DibExpectation) {
    let path = bmpsuite_root().join(rel_path);
    let parsed = parse_bmp(&path).unwrap_or_else(|err| panic!("expected successful parse for {rel_path}: {err}"));

    match parsed {
        Bmp::Core(data) => {
            assert_eq!(expected.variant, DibVariant::Core, "{rel_path}: variant");
            assert_eq!(i32::from(data.bmp_header.width), expected.width, "{rel_path}: width");
            assert_eq!(i32::from(data.bmp_header.height), expected.height, "{rel_path}: height");
            assert_eq!(data.bmp_header.bit_count, expected.bpp, "{rel_path}: bpp");
            assert_eq!(Compression::Rgb, expected.compression, "{rel_path}: compression");
            assert_eq!(
                data.color_table.len(),
                expected.color_table_len,
                "{rel_path}: color table"
            );
        }
        Bmp::Info(data) => {
            assert_eq!(expected.variant, DibVariant::Info, "{rel_path}: variant");
            assert_eq!(data.bmp_header.width, expected.width, "{rel_path}: width");
            assert_eq!(data.bmp_header.height, expected.height, "{rel_path}: height");
            assert_eq!(data.bmp_header.bit_count, expected.bpp, "{rel_path}: bpp");
            assert_eq!(
                data.bmp_header.compression, expected.compression,
                "{rel_path}: compression"
            );
            assert_eq!(
                data.color_table.len(),
                expected.color_table_len,
                "{rel_path}: color table"
            );

            match (data.color_masks, expected.info_masks) {
                (None, None) => {}
                (Some(masks), Some(expected_masks)) => {
                    assert_eq!(
                        [masks.red_mask, masks.green_mask, masks.blue_mask],
                        expected_masks,
                        "{rel_path}: bitfield masks"
                    );
                }
                (actual, expected_masks) => {
                    panic!("{rel_path}: mask mismatch, actual={actual:?}, expected={expected_masks:?}")
                }
            }
        }
        Bmp::V4(data) => {
            assert_eq!(expected.variant, DibVariant::V4, "{rel_path}: variant");
            assert_eq!(data.bmp_header.info.width, expected.width, "{rel_path}: width");
            assert_eq!(data.bmp_header.info.height, expected.height, "{rel_path}: height");
            assert_eq!(data.bmp_header.info.bit_count, expected.bpp, "{rel_path}: bpp");
            assert_eq!(
                data.bmp_header.info.compression, expected.compression,
                "{rel_path}: compression"
            );
            assert_eq!(
                data.color_table.len(),
                expected.color_table_len,
                "{rel_path}: color table"
            );
            assert_eq!(expected.info_masks, None, "{rel_path}: V4 masks are embedded in DIB");
        }
        Bmp::V5(data) => {
            assert_eq!(expected.variant, DibVariant::V5, "{rel_path}: variant");
            assert_eq!(data.bmp_header.v4.info.width, expected.width, "{rel_path}: width");
            assert_eq!(data.bmp_header.v4.info.height, expected.height, "{rel_path}: height");
            assert_eq!(data.bmp_header.v4.info.bit_count, expected.bpp, "{rel_path}: bpp");
            assert_eq!(
                data.bmp_header.v4.info.compression, expected.compression,
                "{rel_path}: compression"
            );
            assert_eq!(
                data.color_table.len(),
                expected.color_table_len,
                "{rel_path}: color table"
            );
            assert_eq!(expected.info_masks, None, "{rel_path}: V5 masks are embedded in DIB");
        }
    }
}

#[rstest]
#[case::pal8os2(
    "g/pal8os2.bmp",
    DibExpectation {
        variant: DibVariant::Core,
        width: 127,
        height: 64,
        bpp: BitsPerPixel::Bpp8,
        compression: Compression::Rgb,
        color_table_len: 256,
        info_masks: None,
    }
)]
#[case::pal4rle(
    "g/pal4rle.bmp",
    DibExpectation {
        variant: DibVariant::Info,
        width: 127,
        height: 64,
        bpp: BitsPerPixel::Bpp4,
        compression: Compression::Rle4,
        color_table_len: 12,
        info_masks: None,
    }
)]
#[case::pal8rle(
    "g/pal8rle.bmp",
    DibExpectation {
        variant: DibVariant::Info,
        width: 127,
        height: 64,
        bpp: BitsPerPixel::Bpp8,
        compression: Compression::Rle8,
        color_table_len: 252,
        info_masks: None,
    }
)]
#[case::pal8topdown(
    "g/pal8topdown.bmp",
    DibExpectation {
        variant: DibVariant::Info,
        width: 127,
        height: -64,
        bpp: BitsPerPixel::Bpp8,
        compression: Compression::Rgb,
        color_table_len: 252,
        info_masks: None,
    }
)]
#[case::rgb16(
    "g/rgb16.bmp",
    DibExpectation {
        variant: DibVariant::Info,
        width: 127,
        height: 64,
        bpp: BitsPerPixel::Bpp16,
        compression: Compression::Rgb,
        color_table_len: 0,
        info_masks: None,
    }
)]
#[case::rgb16_565(
    "g/rgb16-565.bmp",
    DibExpectation {
        variant: DibVariant::Info,
        width: 127,
        height: 64,
        bpp: BitsPerPixel::Bpp16,
        compression: Compression::BitFields,
        color_table_len: 0,
        info_masks: Some([0x0000_F800, 0x0000_07E0, 0x0000_001F]),
    }
)]
#[case::rgb24(
    "g/rgb24.bmp",
    DibExpectation {
        variant: DibVariant::Info,
        width: 127,
        height: 64,
        bpp: BitsPerPixel::Bpp24,
        compression: Compression::Rgb,
        color_table_len: 0,
        info_masks: None,
    }
)]
#[case::rgb32bf(
    "g/rgb32bf.bmp",
    DibExpectation {
        variant: DibVariant::Info,
        width: 127,
        height: 64,
        bpp: BitsPerPixel::Bpp32,
        compression: Compression::BitFields,
        color_table_len: 0,
        info_masks: Some([0xFF00_0000, 0x0000_0FF0, 0x00FF_0000]),
    }
)]
#[case::pal8v4(
    "g/pal8v4.bmp",
    DibExpectation {
        variant: DibVariant::V4,
        width: 127,
        height: 64,
        bpp: BitsPerPixel::Bpp8,
        compression: Compression::Rgb,
        color_table_len: 252,
        info_masks: None,
    }
)]
#[case::pal8v5(
    "g/pal8v5.bmp",
    DibExpectation {
        variant: DibVariant::V5,
        width: 127,
        height: 64,
        bpp: BitsPerPixel::Bpp8,
        compression: Compression::Rgb,
        color_table_len: 252,
        info_masks: None,
    }
)]
#[case::rgb24jpeg_v5(
    "q/rgb24jpeg.bmp",
    DibExpectation {
        variant: DibVariant::V5,
        width: 127,
        height: 64,
        bpp: BitsPerPixel::Bpp0,
        compression: Compression::Jpeg,
        color_table_len: 0,
        info_masks: None,
    }
)]
fn bmpsuite_selected_dib_fields_match_expected_values(#[case] rel_path: &str, #[case] expected: DibExpectation) {
    require_suite_generated();
    assert_dib_expectation(rel_path, expected);
}
