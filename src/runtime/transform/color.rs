//! Color and pixel-wise image transformations.
//!
//! These operations modify pixel values independently without changing
//! image geometry. All functions preserve image dimensions and leave
//! the alpha channel unchanged unless otherwise stated.
//!
//! The implementations use Rayon to parallelize processing across rows
//! or pixels for improved performance on multi-core systems.

use rayon::prelude::*;

use crate::runtime::decode::DecodedImage;

/// Inverts the RGB color channels of the image.
///
/// Each color component is transformed as:
///
/// ```text
/// c' = 255 - c
/// ```
///
/// The alpha channel is left unchanged.
#[must_use]
pub fn invert_colors(image: &DecodedImage) -> DecodedImage {
    let mut out = image.rgba.clone();
    out.par_chunks_exact_mut(4).for_each(|px| {
        px[0] = 255 - px[0];
        px[1] = 255 - px[1];
        px[2] = 255 - px[2];
    });

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

/// Converts the image to grayscale using perceptual luminance weights.
///
/// The grayscale value is computed using the standard Rec.601 luma formula:
///
/// ```text
/// Y = 0.299 R + 0.587 G + 0.114 B
/// ```
///
/// The resulting luminance replaces the RGB channels while the alpha
/// channel remains unchanged.
#[must_use]
pub fn grayscale(image: &DecodedImage) -> DecodedImage {
    let mut out = image.rgba.clone();
    out.par_chunks_exact_mut(4).for_each(|px| {
        let luma = (0.299 * f32::from(px[0]) + 0.587 * f32::from(px[1]) + 0.114 * f32::from(px[2])).round() as u8;
        px[0] = luma;
        px[1] = luma;
        px[2] = luma;
    });

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

/// Applies a sepia tone effect to the image.
///
/// The sepia transformation is computed using the classic sepia matrix:
///
/// ```text
/// R' = 0.393R + 0.769G + 0.189B
/// G' = 0.349R + 0.686G + 0.168B
/// B' = 0.272R + 0.534G + 0.131B
/// ```
///
/// Results are clamped to `[0, 255]`. The alpha channel is preserved.
#[must_use]
pub fn sepia(image: &DecodedImage) -> DecodedImage {
    let mut out = image.rgba.clone();
    out.par_chunks_exact_mut(4).for_each(|px| {
        let r = f32::from(px[0]);
        let g = f32::from(px[1]);
        let b = f32::from(px[2]);
        let sr = (0.393 * r + 0.769 * g + 0.189 * b).round().min(255.0) as u8;
        let sg = (0.349 * r + 0.686 * g + 0.168 * b).round().min(255.0) as u8;
        let sb = (0.272 * r + 0.534 * g + 0.131 * b).round().min(255.0) as u8;
        px[0] = sr;
        px[1] = sg;
        px[2] = sb;
    });

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

/// Adjusts image brightness by adding a constant offset to RGB channels.
///
/// Each channel is transformed as:
///
/// ```text
/// c' = clamp(c + delta, 0, 255)
/// ```
///
/// Positive values increase brightness, negative values darken the image.
/// The alpha channel is not modified.
#[must_use]
pub fn brightness(image: &DecodedImage, delta: i16) -> DecodedImage {
    let mut out = image.rgba.clone();
    out.par_chunks_exact_mut(4).for_each(|px| {
        px[0] = (i16::from(px[0]) + delta).clamp(0, 255) as u8;
        px[1] = (i16::from(px[1]) + delta).clamp(0, 255) as u8;
        px[2] = (i16::from(px[2]) + delta).clamp(0, 255) as u8;
    });

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

/// Adjusts image contrast using a standard contrast scaling formula.
///
/// The transformation applied to each color channel is:
///
/// ```text
/// factor = 259 * (delta + 255) / (255 * (259 - delta))
/// c' = factor * (c - 128) + 128
/// ```
///
/// where `delta` controls the contrast strength:
///
/// * `delta = 0` -> no change
/// * positive values -> increase contrast
/// * negative values -> decrease contrast
///
/// The result is clamped to `[0, 255]`. The alpha channel is preserved.
#[must_use]
pub fn contrast(image: &DecodedImage, delta: i16) -> DecodedImage {
    let delta_clamped = f32::from(delta).clamp(-255.0, 255.0);
    let factor = 259.0 * (delta_clamped + 255.0) / (255.0 * (259.0 - delta_clamped));

    let mut out = image.rgba.clone();
    out.par_chunks_exact_mut(4).for_each(|px| {
        px[0] = (factor * (f32::from(px[0]) - 128.0) + 128.0).round().clamp(0.0, 255.0) as u8;
        px[1] = (factor * (f32::from(px[1]) - 128.0) + 128.0).round().clamp(0.0, 255.0) as u8;
        px[2] = (factor * (f32::from(px[2]) - 128.0) + 128.0).round().clamp(0.0, 255.0) as u8;
    });

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

#[cfg(test)]
mod tests {
    use super::{brightness, contrast, grayscale, invert_colors, sepia};
    use crate::runtime::decode::DecodedImage;

    #[test]
    fn invert_colors_flips_rgb_and_keeps_alpha() {
        let image = DecodedImage {
            width: 2,
            height: 1,
            rgba: vec![10, 20, 30, 40, 100, 150, 200, 250],
        };
        let inverted = invert_colors(&image);
        assert_eq!(inverted.rgba, vec![245, 235, 225, 40, 155, 105, 55, 250]);
    }

    #[test]
    fn grayscale_uses_perceptual_weights() {
        let image = DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![100, 150, 200, 128],
        };
        let gray = grayscale(&image);
        assert_eq!(gray.rgba[0], 141);
        assert_eq!(gray.rgba[1], 141);
        assert_eq!(gray.rgba[2], 141);
        assert_eq!(gray.rgba[3], 128);
    }

    #[test]
    fn sepia_clamps_to_255() {
        let image = DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![255, 255, 255, 255],
        };
        let result = sepia(&image);
        assert_eq!(result.rgba[0], 255);
        assert_eq!(result.rgba[1], 255);
    }

    #[test]
    fn brightness_zero_is_identity() {
        let image = DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![42, 128, 200, 255],
        };
        let result = brightness(&image, 0);
        assert_eq!(result.rgba, image.rgba);
    }

    #[test]
    fn contrast_zero_is_identity() {
        let image = DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![42, 128, 200, 255],
        };
        let result = contrast(&image, 0);
        assert_eq!(result.rgba, image.rgba);
    }
}
