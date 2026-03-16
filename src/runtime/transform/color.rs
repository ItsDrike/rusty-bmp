//! Color and pixel-wise image transformations.
//!
//! These operations modify pixel values independently without changing
//! image geometry. All functions preserve image dimensions and leave
//! the alpha channel unchanged unless otherwise stated.
//!
//! The implementations use Rayon to parallelize processing across rows
//! or pixels for improved performance on multi-core systems.

use std::fmt;

use rayon::prelude::*;

use crate::runtime::decode::DecodedImage;

use super::model::{ImageTransform, TransformError, TransformOp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InvertColors;

impl fmt::Display for InvertColors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Invert Colors")
    }
}

impl TransformOp for InvertColors {
    /// Inverts the RGB color channels of the image.
    ///
    /// Each color component is transformed as:
    ///
    /// ```text
    /// c' = 255 - c
    /// ```
    ///
    /// The alpha channel is left unchanged.
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        let mut out = image.rgba().to_vec();
        out.par_chunks_exact_mut(4).for_each(|px| {
            px[0] = 255 - px[0];
            px[1] = 255 - px[1];
            px[2] = 255 - px[2];
        });

        DecodedImage::new(image.width(), image.height(), out).map_err(TransformError::from)
    }

    fn inverse(&self) -> Option<ImageTransform> {
        Some((*self).into())
    }

    fn replay_cost(&self) -> u32 {
        0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Grayscale;

impl fmt::Display for Grayscale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Grayscale")
    }
}

impl TransformOp for Grayscale {
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
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        let mut out = image.rgba().to_vec();
        out.par_chunks_exact_mut(4).for_each(|px| {
            let luma = (0.299 * f32::from(px[0]) + 0.587 * f32::from(px[1]) + 0.114 * f32::from(px[2])).round() as u8;
            px[0] = luma;
            px[1] = luma;
            px[2] = luma;
        });

        DecodedImage::new(image.width(), image.height(), out).map_err(TransformError::from)
    }

    fn inverse(&self) -> Option<ImageTransform> {
        None
    }

    fn replay_cost(&self) -> u32 {
        1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sepia;

impl fmt::Display for Sepia {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sepia")
    }
}

impl TransformOp for Sepia {
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
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        let mut out = image.rgba().to_vec();
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

        DecodedImage::new(image.width(), image.height(), out).map_err(TransformError::from)
    }

    fn inverse(&self) -> Option<ImageTransform> {
        None
    }

    fn replay_cost(&self) -> u32 {
        1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Brightness {
    pub delta: i16,
}

impl fmt::Display for Brightness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.delta >= 0 {
            write!(f, "Brightness +{}", self.delta)
        } else {
            write!(f, "Brightness {}", self.delta)
        }
    }
}

impl TransformOp for Brightness {
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
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        let mut out = image.rgba().to_vec();
        out.par_chunks_exact_mut(4).for_each(|px| {
            px[0] = (i16::from(px[0]) + self.delta).clamp(0, 255) as u8;
            px[1] = (i16::from(px[1]) + self.delta).clamp(0, 255) as u8;
            px[2] = (i16::from(px[2]) + self.delta).clamp(0, 255) as u8;
        });

        DecodedImage::new(image.width(), image.height(), out).map_err(TransformError::from)
    }

    fn inverse(&self) -> Option<ImageTransform> {
        None
    }

    fn replay_cost(&self) -> u32 {
        1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Contrast {
    pub delta: i16,
}

impl fmt::Display for Contrast {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.delta >= 0 {
            write!(f, "Contrast +{}", self.delta)
        } else {
            write!(f, "Contrast {}", self.delta)
        }
    }
}

impl TransformOp for Contrast {
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
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        let delta_clamped = f32::from(self.delta).clamp(-255.0, 255.0);
        let factor = 259.0 * (delta_clamped + 255.0) / (255.0 * (259.0 - delta_clamped));

        let mut out = image.rgba().to_vec();
        out.par_chunks_exact_mut(4).for_each(|px| {
            px[0] = (factor * (f32::from(px[0]) - 128.0) + 128.0).round().clamp(0.0, 255.0) as u8;
            px[1] = (factor * (f32::from(px[1]) - 128.0) + 128.0).round().clamp(0.0, 255.0) as u8;
            px[2] = (factor * (f32::from(px[2]) - 128.0) + 128.0).round().clamp(0.0, 255.0) as u8;
        });

        DecodedImage::new(image.width(), image.height(), out).map_err(TransformError::from)
    }

    fn inverse(&self) -> Option<ImageTransform> {
        None
    }

    fn replay_cost(&self) -> u32 {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::{Brightness, Contrast, Grayscale, InvertColors, Sepia};
    use crate::runtime::decode::DecodedImage;
    use crate::runtime::transform::{ImageTransform, TransformOp};

    #[test]
    fn invert_colors_flips_rgb_and_keeps_alpha() {
        let image = DecodedImage::new(2, 1, vec![10, 20, 30, 40, 100, 150, 200, 250]).expect("valid image");
        let inverted = InvertColors.apply(&image).expect("invert colors should always succeed");
        assert_eq!(inverted.rgba(), vec![245, 235, 225, 40, 155, 105, 55, 250]);
    }

    #[test]
    fn grayscale_uses_perceptual_weights() {
        let image = DecodedImage::new(1, 1, vec![100, 150, 200, 128]).expect("valid image");
        let gray = Grayscale.apply(&image).expect("grayscale should always succeed");
        assert_eq!(gray.rgba()[0], 141);
        assert_eq!(gray.rgba()[1], 141);
        assert_eq!(gray.rgba()[2], 141);
        assert_eq!(gray.rgba()[3], 128);
    }

    #[test]
    fn sepia_clamps_to_255() {
        let image = DecodedImage::new(1, 1, vec![255, 255, 255, 255]).expect("valid image");
        let result = Sepia.apply(&image).expect("sepia should always succeed");
        assert_eq!(result.rgba()[0], 255);
        assert_eq!(result.rgba()[1], 255);
    }

    #[test]
    fn brightness_zero_is_identity() {
        let image = DecodedImage::new(1, 1, vec![42, 128, 200, 255]).expect("valid image");
        let result = Brightness { delta: 0 }
            .apply(&image)
            .expect("brightness should always succeed");
        assert_eq!(result.rgba(), image.rgba());
    }

    #[test]
    fn contrast_zero_is_identity() {
        let image = DecodedImage::new(1, 1, vec![42, 128, 200, 255]).expect("valid image");
        let result = Contrast { delta: 0 }
            .apply(&image)
            .expect("contrast should always succeed");
        assert_eq!(result.rgba(), image.rgba());
    }

    #[test]
    fn invert_is_self_inverse() {
        let inv = InvertColors.inverse().expect("invert should self-invert");
        assert!(matches!(inv, ImageTransform::InvertColors(_)));
    }

    #[test]
    fn apply_then_inverse_is_identity_for_invert() {
        let image = DecodedImage::new(2, 1, vec![10, 20, 30, 255, 40, 50, 60, 255]).expect("valid image");
        let ops: Vec<ImageTransform> = vec![InvertColors.into()];
        for op in ops {
            let inv = op.inverse().expect("invert should have inverse");
            let transformed = op.apply(&image).expect("apply should succeed");
            let restored = inv.apply(&transformed).expect("inverse apply should succeed");
            assert_eq!(restored.rgba(), image.rgba());
        }
    }

    #[test]
    fn smoke_construct_color_ops() {
        let _ = InvertColors;
        let _ = Grayscale;
        let _ = super::Sepia;
        let _ = Brightness { delta: 5 };
        let _ = Contrast { delta: -7 };
    }
}
