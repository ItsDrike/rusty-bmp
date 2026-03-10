use std::fmt;

use crate::runtime::decode::DecodedImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageTransform {
    RotateLeft90,
    RotateRight90,
    MirrorHorizontal,
    MirrorVertical,
    InvertColors,
    Grayscale,
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
        }
    }
}

impl ImageTransform {
    /// Returns the transform that reverses the effect of `self`, or `None`
    /// if the transform is lossy and requires a full pipeline replay to undo.
    pub fn inverse(self) -> Option<Self> {
        match self {
            Self::RotateLeft90 => Some(Self::RotateRight90),
            Self::RotateRight90 => Some(Self::RotateLeft90),
            Self::MirrorHorizontal => Some(Self::MirrorHorizontal),
            Self::MirrorVertical => Some(Self::MirrorVertical),
            Self::InvertColors => Some(Self::InvertColors),
            Self::Grayscale => None,
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
            out = apply_transform(&out, *op);
        }
        out
    }
}

pub fn apply_transform(image: &DecodedImage, op: ImageTransform) -> DecodedImage {
    match op {
        ImageTransform::RotateLeft90 => rotate_left(image),
        ImageTransform::RotateRight90 => rotate_right(image),
        ImageTransform::MirrorHorizontal => mirror_horizontal(image),
        ImageTransform::MirrorVertical => mirror_vertical(image),
        ImageTransform::InvertColors => invert_colors(image),
        ImageTransform::Grayscale => grayscale(image),
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
            let transformed = apply_transform(&image, op);
            let restored = apply_transform(&transformed, inv);
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
}
