//! Heuristics and roundtrip checks used to warn about lossy saves.

use std::collections::HashSet;

use bmp::runtime::{
    decode::{DecodedImage, decode_to_rgba},
    encode::{SaveFormat, SaveHeaderVersion, SourceMetadata, encode_rgba_to_bmp_ext},
    steganography::{self, StegInfo},
};

/// Builds human-readable save warnings for the current image and settings.
pub(super) fn warning_reasons(
    image: &DecodedImage,
    format: SaveFormat,
    header: SaveHeaderVersion,
    source_metadata: Option<&SourceMetadata>,
    detected: Option<&StegInfo>,
) -> Vec<String> {
    let mut reasons = Vec::new();

    if let Some(info) = detected {
        match preserves_steg_payload(image, format, header, source_metadata, info) {
            Ok(true) => {}
            Ok(false) => {
                reasons.push(
                    "Roundtrip verification shows the selected format/header does not preserve the hidden steganography payload"
                        .to_owned(),
                );
            }
            Err(err) => {
                reasons.push(format!(
                    "Could not verify steganography preservation ({err}); saving may destroy hidden data"
                ));
            }
        }
    }

    let has_transparency = image.pixels().any(|px| px[3] < u8::MAX);
    if has_transparency && !matches!(format, SaveFormat::Rgb24 | SaveFormat::Rgb32 | SaveFormat::BitFields32) {
        reasons.push("Selected format/header does not preserve alpha; transparency will be lost".to_owned());
    }

    match format {
        SaveFormat::Rgb1 => {
            if unique_rgb_colors_exceed(image, 2) {
                reasons.push("Image has more than 2 colors and will be quantized to 1-bpp palette".to_owned());
            }
        }
        SaveFormat::Rgb4 | SaveFormat::Rle4 => {
            if unique_rgb_colors_exceed(image, 16) {
                reasons.push("Image has more than 16 colors and will be quantized to 4-bpp palette".to_owned());
            }
        }
        SaveFormat::Rgb8 | SaveFormat::Rle8 => {
            if unique_rgb_colors_exceed(image, 256) {
                reasons.push("Image has more than 256 colors and will be quantized to 8-bpp palette".to_owned());
            }
        }
        SaveFormat::Rgb16 | SaveFormat::BitFields16Rgb555 => {
            if !all_pixels_exact_in_5bit_grid(image) {
                reasons.push("RGB channels will be reduced to RGB555 precision".to_owned());
            }
        }
        SaveFormat::BitFields16Rgb565 => {
            if !all_pixels_exact_in_565_grid(image) {
                reasons.push("RGB channels will be reduced to RGB565 precision".to_owned());
            }
        }
        SaveFormat::Rgb24 | SaveFormat::Rgb32 | SaveFormat::BitFields32 => {}
    }

    reasons
}

/// Performs an encode/decode roundtrip to see whether the hidden payload survives the selected save settings.
fn preserves_steg_payload(
    image: &DecodedImage,
    format: SaveFormat,
    header: SaveHeaderVersion,
    source_metadata: Option<&SourceMetadata>,
    info: &StegInfo,
) -> Result<bool, String> {
    let original_payload = steganography::extract(image, info)
        .map_err(|e| format!("failed to extract current payload before save-check: {e}"))?;

    let encoded = encode_rgba_to_bmp_ext(image, format, header, source_metadata)
        .map_err(|e| format!("failed to encode save-check roundtrip: {e}"))?;

    let roundtrip = decode_to_rgba(&encoded).map_err(|e| format!("failed to decode save-check roundtrip: {e}"))?;

    let Some(round_info) = steganography::detect(&roundtrip)
        .map_err(|e| format!("failed to detect payload after save-check roundtrip: {e}"))?
    else {
        return Ok(false);
    };

    let round_payload = steganography::extract(&roundtrip, &round_info)
        .map_err(|e| format!("failed to extract payload after save-check roundtrip: {e}"))?;

    Ok(round_payload == original_payload)
}

/// Returns `true` once the image exceeds a palette-friendly number of RGB colors.
fn unique_rgb_colors_exceed(image: &DecodedImage, limit: usize) -> bool {
    let mut unique = HashSet::with_capacity(limit.saturating_add(1));
    for px in image.pixels() {
        unique.insert([px[0], px[1], px[2]]);
        if unique.len() > limit {
            return true;
        }
    }
    false
}

/// Returns `true` if every pixel already lies on the RGB555 quantization grid.
///
/// Callers use this to decide whether saving as a 16-bit RGB555 format would
/// actually change any visible pixel values.
fn all_pixels_exact_in_5bit_grid(image: &DecodedImage) -> bool {
    image.pixels().all(|px| {
        let r5 = (u16::from(px[0]) * 31 + 127) / 255;
        let g5 = (u16::from(px[1]) * 31 + 127) / 255;
        let b5 = (u16::from(px[2]) * 31 + 127) / 255;

        #[allow(clippy::cast_possible_truncation)]
        let r8 = ((r5 * 255 + 15) / 31) as u8;
        #[allow(clippy::cast_possible_truncation)]
        let g8 = ((g5 * 255 + 15) / 31) as u8;
        #[allow(clippy::cast_possible_truncation)]
        let b8 = ((b5 * 255 + 15) / 31) as u8;

        px[0] == r8 && px[1] == g8 && px[2] == b8
    })
}

/// Returns `true` if every pixel already lies on the RGB565 quantization grid.
///
/// This is used to avoid warning about RGB565 output when the current image is
/// already exactly representable in that format.
fn all_pixels_exact_in_565_grid(image: &DecodedImage) -> bool {
    image.pixels().all(|px| {
        let r5 = (u16::from(px[0]) * 31 + 127) / 255;
        let g6 = (u16::from(px[1]) * 63 + 127) / 255;
        let b5 = (u16::from(px[2]) * 31 + 127) / 255;

        #[allow(clippy::cast_possible_truncation)]
        let r8 = ((r5 * 255 + 15) / 31) as u8;
        #[allow(clippy::cast_possible_truncation)]
        let g8 = ((g6 * 255 + 31) / 63) as u8;
        #[allow(clippy::cast_possible_truncation)]
        let b8 = ((b5 * 255 + 15) / 31) as u8;

        px[0] == r8 && px[1] == g8 && px[2] == b8
    })
}
