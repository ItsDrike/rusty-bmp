use std::fmt;

use crate::runtime::decode::DecodedImage;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageTransform {
    RotateLeft90,
    RotateRight90,
    MirrorHorizontal,
    MirrorVertical,
    InvertColors,
    Grayscale,
    /// Adjust brightness by a signed delta (clamped to 0..=255 per channel).
    Brightness(i16),
    /// Adjust contrast by a signed delta using the standard 259-based formula.
    Contrast(i16),
}

impl fmt::Display for ImageTransform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RotateLeft90 => write!(f, "Rotate Left"),
            Self::RotateRight90 => write!(f, "Rotate Right"),
            Self::MirrorHorizontal => write!(f, "Mirror Horizontal"),
            Self::MirrorVertical => write!(f, "Mirror Vertical"),
            Self::InvertColors => write!(f, "Invert Colors"),
            Self::Grayscale => write!(f, "Grayscale"),
            Self::Brightness(delta) => {
                if *delta >= 0 {
                    write!(f, "Brightness +{delta}")
                } else {
                    write!(f, "Brightness {delta}")
                }
            }
            Self::Contrast(delta) => {
                if *delta >= 0 {
                    write!(f, "Contrast +{delta}")
                } else {
                    write!(f, "Contrast {delta}")
                }
            }
        }
    }
}

impl ImageTransform {
    /// Returns the transform that reverses the effect of `self`, or `None`
    /// if the transform is lossy and requires a full pipeline replay to undo.
    pub fn inverse(&self) -> Option<Self> {
        match self {
            Self::RotateLeft90 => Some(Self::RotateRight90),
            Self::RotateRight90 => Some(Self::RotateLeft90),
            Self::MirrorHorizontal => Some(Self::MirrorHorizontal),
            Self::MirrorVertical => Some(Self::MirrorVertical),
            Self::InvertColors => Some(Self::InvertColors),
            // Lossy: clamping destroys information, requires pipeline replay.
            Self::Grayscale => None,
            Self::Brightness(_) => None,
            Self::Contrast(_) => None,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct TransformPipeline {
    ops: Vec<ImageTransform>,
}

impl TransformPipeline {
    pub fn push(&mut self, op: ImageTransform) {
        self.ops.push(op);
    }

    pub fn clear(&mut self) {
        self.ops.clear();
    }

    pub fn ops(&self) -> &[ImageTransform] {
        &self.ops
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn remove(&mut self, index: usize) {
        self.ops.remove(index);
    }

    pub fn pop(&mut self) -> Option<ImageTransform> {
        self.ops.pop()
    }

    pub fn apply(&self, image: &DecodedImage) -> DecodedImage {
        let mut out = image.clone();
        for op in &self.ops {
            out = apply_transform(&out, op);
        }
        out
    }
}

pub fn apply_transform(image: &DecodedImage, op: &ImageTransform) -> DecodedImage {
    match op {
        ImageTransform::RotateLeft90 => rotate_left(image),
        ImageTransform::RotateRight90 => rotate_right(image),
        ImageTransform::MirrorHorizontal => mirror_horizontal(image),
        ImageTransform::MirrorVertical => mirror_vertical(image),
        ImageTransform::InvertColors => invert_colors(image),
        ImageTransform::Grayscale => grayscale(image),
        ImageTransform::Brightness(delta) => brightness(image, *delta),
        ImageTransform::Contrast(delta) => contrast(image, *delta),
    }
}

pub fn rotate_left(image: &DecodedImage) -> DecodedImage {
    let src_w = image.width as usize;
    let src_h = image.height as usize;
    let dst_w = src_h;
    let dst_h = src_w;
    let mut out = vec![0_u8; dst_w * dst_h * 4];

    for y in 0..src_h {
        for x in 0..src_w {
            let src = (y * src_w + x) * 4;
            let dst_x = y;
            let dst_y = src_w - 1 - x;
            let dst = (dst_y * dst_w + dst_x) * 4;
            out[dst..dst + 4].copy_from_slice(&image.rgba[src..src + 4]);
        }
    }

    DecodedImage {
        width: dst_w as u32,
        height: dst_h as u32,
        rgba: out,
    }
}

pub fn rotate_right(image: &DecodedImage) -> DecodedImage {
    let src_w = image.width as usize;
    let src_h = image.height as usize;
    let dst_w = src_h;
    let dst_h = src_w;
    let mut out = vec![0_u8; dst_w * dst_h * 4];

    for y in 0..src_h {
        for x in 0..src_w {
            let src = (y * src_w + x) * 4;
            let dst_x = src_h - 1 - y;
            let dst_y = x;
            let dst = (dst_y * dst_w + dst_x) * 4;
            out[dst..dst + 4].copy_from_slice(&image.rgba[src..src + 4]);
        }
    }

    DecodedImage {
        width: dst_w as u32,
        height: dst_h as u32,
        rgba: out,
    }
}

pub fn mirror_horizontal(image: &DecodedImage) -> DecodedImage {
    let w = image.width as usize;
    let h = image.height as usize;
    let mut out = vec![0_u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            let src = (y * w + x) * 4;
            let dst_x = w - 1 - x;
            let dst = (y * w + dst_x) * 4;
            out[dst..dst + 4].copy_from_slice(&image.rgba[src..src + 4]);
        }
    }

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

pub fn mirror_vertical(image: &DecodedImage) -> DecodedImage {
    let w = image.width as usize;
    let h = image.height as usize;
    let mut out = vec![0_u8; w * h * 4];

    for y in 0..h {
        let dst_y = h - 1 - y;
        let src = y * w * 4;
        let dst = dst_y * w * 4;
        out[dst..dst + w * 4].copy_from_slice(&image.rgba[src..src + w * 4]);
    }

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

pub fn invert_colors(image: &DecodedImage) -> DecodedImage {
    let mut out = image.rgba.clone();
    for px in out.chunks_exact_mut(4) {
        px[0] = 255 - px[0];
        px[1] = 255 - px[1];
        px[2] = 255 - px[2];
    }

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

pub fn grayscale(image: &DecodedImage) -> DecodedImage {
    let mut out = image.rgba.clone();
    for px in out.chunks_exact_mut(4) {
        // ITU-R BT.601 luma coefficients (standard perceptual weights).
        let luma = (0.299 * px[0] as f32 + 0.587 * px[1] as f32 + 0.114 * px[2] as f32).round() as u8;
        px[0] = luma;
        px[1] = luma;
        px[2] = luma;
        // Alpha unchanged.
    }

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

pub fn brightness(image: &DecodedImage, delta: i16) -> DecodedImage {
    let mut out = image.rgba.clone();
    for px in out.chunks_exact_mut(4) {
        px[0] = (px[0] as i16 + delta).clamp(0, 255) as u8;
        px[1] = (px[1] as i16 + delta).clamp(0, 255) as u8;
        px[2] = (px[2] as i16 + delta).clamp(0, 255) as u8;
        // Alpha unchanged.
    }

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

pub fn contrast(image: &DecodedImage, delta: i16) -> DecodedImage {
    // Standard contrast formula:
    //   factor = 259 * (delta + 255) / (255 * (259 - delta))
    //   new = clamp(factor * (old - 128) + 128, 0, 255)
    // The delta range is clamped to -255..=255 to avoid division by zero.
    let delta_clamped = (delta as f32).clamp(-255.0, 255.0);
    let factor = 259.0 * (delta_clamped + 255.0) / (255.0 * (259.0 - delta_clamped));

    let mut out = image.rgba.clone();
    for px in out.chunks_exact_mut(4) {
        px[0] = (factor * (px[0] as f32 - 128.0) + 128.0).round().clamp(0.0, 255.0) as u8;
        px[1] = (factor * (px[1] as f32 - 128.0) + 128.0).round().clamp(0.0, 255.0) as u8;
        px[2] = (factor * (px[2] as f32 - 128.0) + 128.0).round().clamp(0.0, 255.0) as u8;
        // Alpha unchanged.
    }

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_transform, invert_colors, ImageTransform};
    use crate::runtime::decode::DecodedImage;

    #[test]
    fn invert_colors_flips_rgb_and_keeps_alpha() {
        let image = DecodedImage {
            width: 2,
            height: 1,
            rgba: vec![
                10, 20, 30, 40, // pixel 0
                100, 150, 200, 250, // pixel 1
            ],
        };

        let inverted = invert_colors(&image);
        assert_eq!(inverted.width, 2);
        assert_eq!(inverted.height, 1);
        assert_eq!(
            inverted.rgba,
            vec![
                245, 235, 225, 40, // alpha unchanged
                155, 105, 55, 250
            ]
        );
    }

    #[test]
    fn inverse_of_rotate_left_is_rotate_right() {
        assert_eq!(
            ImageTransform::RotateLeft90.inverse(),
            Some(ImageTransform::RotateRight90)
        );
        assert_eq!(
            ImageTransform::RotateRight90.inverse(),
            Some(ImageTransform::RotateLeft90)
        );
    }

    #[test]
    fn self_inverse_transforms() {
        assert_eq!(
            ImageTransform::MirrorHorizontal.inverse(),
            Some(ImageTransform::MirrorHorizontal)
        );
        assert_eq!(
            ImageTransform::MirrorVertical.inverse(),
            Some(ImageTransform::MirrorVertical)
        );
        assert_eq!(
            ImageTransform::InvertColors.inverse(),
            Some(ImageTransform::InvertColors)
        );
    }

    #[test]
    fn lossy_transforms_have_no_inverse() {
        assert_eq!(ImageTransform::Grayscale.inverse(), None);
        assert_eq!(ImageTransform::Brightness(10).inverse(), None);
        assert_eq!(ImageTransform::Brightness(-10).inverse(), None);
        assert_eq!(ImageTransform::Contrast(10).inverse(), None);
        assert_eq!(ImageTransform::Contrast(-10).inverse(), None);
    }

    #[test]
    fn apply_then_inverse_is_identity() {
        let image = DecodedImage {
            width: 3,
            height: 2,
            rgba: vec![
                10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255, 130, 140, 150, 255, 160, 170,
                180, 255,
            ],
        };

        for op in [
            ImageTransform::RotateLeft90,
            ImageTransform::RotateRight90,
            ImageTransform::MirrorHorizontal,
            ImageTransform::MirrorVertical,
            ImageTransform::InvertColors,
        ] {
            let inv = op.inverse().expect("reversible transform should have an inverse");
            let transformed = apply_transform(&image, &op);
            let restored = apply_transform(&transformed, &inv);
            assert_eq!(restored.width, image.width, "width mismatch for {op}");
            assert_eq!(restored.height, image.height, "height mismatch for {op}");
            assert_eq!(restored.rgba, image.rgba, "pixel data mismatch for {op}");
        }
    }

    #[test]
    fn grayscale_uses_perceptual_weights() {
        let image = DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![100, 150, 200, 128],
        };
        let gray = super::grayscale(&image);
        // BT.601: 0.299*100 + 0.587*150 + 0.114*200 = 29.9 + 88.05 + 22.8 = 140.75 → 141
        assert_eq!(gray.rgba[0], 141);
        assert_eq!(gray.rgba[1], 141);
        assert_eq!(gray.rgba[2], 141);
        assert_eq!(gray.rgba[3], 128); // alpha unchanged
    }

    #[test]
    fn brightness_positive_adds_and_clamps() {
        let image = DecodedImage {
            width: 2,
            height: 1,
            rgba: vec![
                100, 150, 200, 128, // pixel 0: 200+80=280 → clamped to 255
                10, 20, 30, 255, // pixel 1: no clamping needed
            ],
        };
        let result = super::brightness(&image, 80);
        assert_eq!(result.rgba[0], 180); // 100+80
        assert_eq!(result.rgba[1], 230); // 150+80
        assert_eq!(result.rgba[2], 255); // 200+80=280 → 255
        assert_eq!(result.rgba[3], 128); // alpha unchanged
        assert_eq!(result.rgba[4], 90); // 10+80
        assert_eq!(result.rgba[5], 100); // 20+80
        assert_eq!(result.rgba[6], 110); // 30+80
        assert_eq!(result.rgba[7], 255); // alpha unchanged
    }

    #[test]
    fn brightness_negative_subtracts_and_clamps() {
        let image = DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![30, 100, 200, 64],
        };
        let result = super::brightness(&image, -50);
        assert_eq!(result.rgba[0], 0); // 30-50=-20 → 0
        assert_eq!(result.rgba[1], 50); // 100-50
        assert_eq!(result.rgba[2], 150); // 200-50
        assert_eq!(result.rgba[3], 64); // alpha unchanged
    }

    #[test]
    fn brightness_zero_is_identity() {
        let image = DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![42, 128, 200, 255],
        };
        let result = super::brightness(&image, 0);
        assert_eq!(result.rgba, image.rgba);
    }

    #[test]
    fn brightness_display_format() {
        assert_eq!(ImageTransform::Brightness(10).to_string(), "Brightness +10");
        assert_eq!(ImageTransform::Brightness(-10).to_string(), "Brightness -10");
        assert_eq!(ImageTransform::Brightness(0).to_string(), "Brightness +0");
    }

    #[test]
    fn contrast_positive_increases_spread() {
        // Positive contrast pushes values away from 128.
        let image = DecodedImage {
            width: 2,
            height: 1,
            rgba: vec![
                100, 128, 200, 255, // pixel 0: 100 < 128 → darker, 128 → same, 200 > 128 → brighter
                0, 255, 64, 128, // pixel 1: extremes stay clamped
            ],
        };
        let result = super::contrast(&image, 50);
        // value < 128 should decrease, value > 128 should increase, 128 stays 128.
        assert!(result.rgba[0] < 100, "dark channel should get darker");
        assert_eq!(result.rgba[1], 128, "midpoint should stay at 128");
        assert!(result.rgba[2] > 200, "bright channel should get brighter");
        assert_eq!(result.rgba[3], 255, "alpha unchanged");
        assert_eq!(result.rgba[4], 0, "already at 0, stays 0");
        assert_eq!(result.rgba[5], 255, "already at 255, stays 255");
        assert_eq!(result.rgba[7], 128, "alpha unchanged");
    }

    #[test]
    fn contrast_negative_reduces_spread() {
        // Negative contrast pulls values toward 128.
        let image = DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![50, 200, 128, 255],
        };
        let result = super::contrast(&image, -50);
        assert!(result.rgba[0] > 50, "dark channel should move toward 128");
        assert!(result.rgba[1] < 200, "bright channel should move toward 128");
        assert_eq!(result.rgba[2], 128, "midpoint stays at 128");
        assert_eq!(result.rgba[3], 255, "alpha unchanged");
    }

    #[test]
    fn contrast_zero_is_identity() {
        let image = DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![42, 128, 200, 255],
        };
        let result = super::contrast(&image, 0);
        assert_eq!(result.rgba, image.rgba);
    }

    #[test]
    fn contrast_display_format() {
        assert_eq!(ImageTransform::Contrast(10).to_string(), "Contrast +10");
        assert_eq!(ImageTransform::Contrast(-10).to_string(), "Contrast -10");
        assert_eq!(ImageTransform::Contrast(0).to_string(), "Contrast +0");
    }
}
