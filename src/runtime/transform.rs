use std::fmt;

use rayon::prelude::*;

use crate::runtime::decode::DecodedImage;

/// A convolution kernel of arbitrary odd size (3x3, 5x5, 7x7, ...).
///
/// Weights are stored in row-major order. The `divisor` normalizes the weighted
/// sum, and `bias` is added after division (useful for emboss-style filters
/// where output is centered around 128 instead of 0).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Kernel {
    pub weights: Vec<i32>,
    /// Side length of the kernel (must be odd, >= 1).
    pub size: usize,
    /// Normalization divisor applied after summing weighted neighbors.
    pub divisor: i32,
    /// Constant added after division (e.g. 128 for relief/emboss filters).
    pub bias: i32,
}

impl Kernel {
    /// Create a new kernel. Panics if `size` is even or zero, if `weights`
    /// length does not equal `size * size`, or if `divisor` is zero.
    pub fn new(weights: Vec<i32>, size: usize, divisor: i32, bias: i32) -> Self {
        assert!(
            size > 0 && size % 2 == 1,
            "kernel size must be odd and positive, got {size}"
        );
        assert_eq!(
            weights.len(),
            size * size,
            "expected {} weights for {size}x{size} kernel, got {}",
            size * size,
            weights.len()
        );
        assert_ne!(divisor, 0, "kernel divisor must not be zero");
        Self {
            weights,
            size,
            divisor,
            bias,
        }
    }

    /// Tries to decompose this kernel into two 1D vectors (column, row)
    /// such that `column[y] * row[x] == weights[y * size + x]` for all y, x.
    ///
    /// Returns `Some((col_vec, row_vec))` if the kernel is separable (rank-1),
    /// or `None` if it is not. The combined divisor for the two passes is
    /// `sqrt(divisor)` per pass when divisor is a perfect square, but callers
    /// should use the original `divisor` and `bias` for final normalization.
    ///
    /// A kernel is separable if every row is a scalar multiple of a single
    /// reference row. The column vector holds those scalar factors.
    pub fn separable(&self) -> Option<(Vec<i32>, Vec<i32>)> {
        let n = self.size;
        if n == 1 {
            // 1x1 kernel is trivially separable: [w] = [1] × [w] (or [w] × [1]).
            return Some((vec![1], self.weights.clone()));
        }

        // Find the first row with at least one non-zero weight to use as
        // the reference row vector.
        let ref_row_idx = (0..n).find(|&y| (0..n).any(|x| self.weights[y * n + x] != 0))?; // All-zero kernel is not usefully separable.

        let row_vec: Vec<i32> = (0..n).map(|x| self.weights[ref_row_idx * n + x]).collect();

        // Find the first non-zero element in the reference row. We need this
        // to extract the column scale factors.
        let ref_col_idx = (0..n).find(|&x| row_vec[x] != 0)?;
        let ref_val = row_vec[ref_col_idx];

        // Build the column vector: col[y] = weights[y][ref_col_idx] / ref_val.
        // Each must divide evenly for integer separability.
        let mut col_vec = Vec::with_capacity(n);
        for y in 0..n {
            let val = self.weights[y * n + ref_col_idx];
            if val % ref_val != 0 {
                return None; // Not separable with integers.
            }
            col_vec.push(val / ref_val);
        }

        // Verify: col[y] * row[x] must equal weights[y * n + x] for all y, x.
        for y in 0..n {
            for x in 0..n {
                if col_vec[y] * row_vec[x] != self.weights[y * n + x] {
                    return None;
                }
            }
        }

        Some((col_vec, row_vec))
    }
}

/// Named convolution filter presets.
///
/// Each variant maps to a specific [`Kernel`] via [`ConvolutionFilter::kernel()`].
/// New filters can be added by defining a variant here and returning the
/// appropriate kernel.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConvolutionFilter {
    Blur,
    Sharpen,
    EdgeDetect,
    Emboss,
}

impl ConvolutionFilter {
    /// Returns the convolution kernel for this filter preset.
    pub fn kernel(&self) -> Kernel {
        match self {
            // Gaussian blur 3x3 — weighted average, softens image.
            Self::Blur => Kernel::new(vec![1, 2, 1, 2, 4, 2, 1, 2, 1], 3, 16, 0),
            // Sharpening — emphasizes differences from neighbors.
            Self::Sharpen => Kernel::new(vec![0, -1, 0, -1, 5, -1, 0, -1, 0], 3, 1, 0),
            // Laplacian edge detection — highlights regions of rapid intensity change.
            Self::EdgeDetect => Kernel::new(vec![-1, -1, -1, -1, 8, -1, -1, -1, -1], 3, 1, 0),
            // Emboss — directional relief effect, biased to gray midpoint.
            Self::Emboss => Kernel::new(vec![-2, -1, 0, -1, 1, 1, 0, 1, 2], 3, 1, 128),
        }
    }
}

impl fmt::Display for ConvolutionFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blur => write!(f, "Blur"),
            Self::Sharpen => write!(f, "Sharpen"),
            Self::EdgeDetect => write!(f, "Edge Detect"),
            Self::Emboss => write!(f, "Emboss"),
        }
    }
}

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
    /// Apply a convolution filter (blur, sharpen, etc.).
    Convolution(ConvolutionFilter),
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
            Self::Convolution(filter) => write!(f, "{filter}"),
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
            Self::Convolution(_) => None,
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
        ImageTransform::Convolution(filter) => apply_convolution(image, &filter.kernel()),
    }
}

pub fn rotate_left(image: &DecodedImage) -> DecodedImage {
    let src_w = image.width as usize;
    let src_h = image.height as usize;
    let dst_w = src_h;
    let dst_h = src_w;
    let row_bytes = dst_w * 4;
    let mut out = vec![0_u8; dst_w * dst_h * 4];

    // Iterate over output rows in parallel.
    // Output row dst_y corresponds to source column x = src_w - 1 - dst_y.
    out.par_chunks_mut(row_bytes).enumerate().for_each(|(dst_y, row)| {
        let x = src_w - 1 - dst_y;
        for dst_x in 0..dst_w {
            let y = dst_x; // source row
            let src = (y * src_w + x) * 4;
            let dst = dst_x * 4;
            row[dst..dst + 4].copy_from_slice(&image.rgba[src..src + 4]);
        }
    });

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
    let row_bytes = dst_w * 4;
    let mut out = vec![0_u8; dst_w * dst_h * 4];

    // Iterate over output rows in parallel.
    // Output row dst_y corresponds to source column x = dst_y.
    out.par_chunks_mut(row_bytes).enumerate().for_each(|(dst_y, row)| {
        let x = dst_y;
        for dst_x in 0..dst_w {
            let y = src_h - 1 - dst_x; // source row
            let src = (y * src_w + x) * 4;
            let dst = dst_x * 4;
            row[dst..dst + 4].copy_from_slice(&image.rgba[src..src + 4]);
        }
    });

    DecodedImage {
        width: dst_w as u32,
        height: dst_h as u32,
        rgba: out,
    }
}

pub fn mirror_horizontal(image: &DecodedImage) -> DecodedImage {
    let w = image.width as usize;
    let h = image.height as usize;
    let row_bytes = w * 4;
    let mut out = vec![0_u8; w * h * 4];

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let src = (y * w + x) * 4;
            let dst_x = w - 1 - x;
            let dst = dst_x * 4;
            row[dst..dst + 4].copy_from_slice(&image.rgba[src..src + 4]);
        }
    });

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

pub fn mirror_vertical(image: &DecodedImage) -> DecodedImage {
    let w = image.width as usize;
    let h = image.height as usize;
    let row_bytes = w * 4;
    let mut out = vec![0_u8; w * h * 4];

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(y, row)| {
        let src_y = h - 1 - y;
        let src = src_y * row_bytes;
        row.copy_from_slice(&image.rgba[src..src + row_bytes]);
    });

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

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

pub fn grayscale(image: &DecodedImage) -> DecodedImage {
    let mut out = image.rgba.clone();
    out.par_chunks_exact_mut(4).for_each(|px| {
        // ITU-R BT.601 luma coefficients (standard perceptual weights).
        let luma = (0.299 * px[0] as f32 + 0.587 * px[1] as f32 + 0.114 * px[2] as f32).round() as u8;
        px[0] = luma;
        px[1] = luma;
        px[2] = luma;
        // Alpha unchanged.
    });

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

pub fn brightness(image: &DecodedImage, delta: i16) -> DecodedImage {
    let mut out = image.rgba.clone();
    out.par_chunks_exact_mut(4).for_each(|px| {
        px[0] = (px[0] as i16 + delta).clamp(0, 255) as u8;
        px[1] = (px[1] as i16 + delta).clamp(0, 255) as u8;
        px[2] = (px[2] as i16 + delta).clamp(0, 255) as u8;
        // Alpha unchanged.
    });

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
    out.par_chunks_exact_mut(4).for_each(|px| {
        px[0] = (factor * (px[0] as f32 - 128.0) + 128.0).round().clamp(0.0, 255.0) as u8;
        px[1] = (factor * (px[1] as f32 - 128.0) + 128.0).round().clamp(0.0, 255.0) as u8;
        px[2] = (factor * (px[2] as f32 - 128.0) + 128.0).round().clamp(0.0, 255.0) as u8;
        // Alpha unchanged.
    });

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

/// Apply an arbitrary NxN convolution kernel to an image.
///
/// If the kernel is separable (rank-1), a faster two-pass approach is used
/// (horizontal then vertical), reducing the per-pixel work from N² to 2N
/// multiply-accumulates. Otherwise, the standard 2D convolution is applied.
///
/// Out-of-bounds neighbor coordinates are clamped to the nearest edge pixel.
/// Alpha is passed through unchanged. Both paths are parallelized with rayon.
pub fn apply_convolution(image: &DecodedImage, kernel: &Kernel) -> DecodedImage {
    if let Some((col_vec, row_vec)) = kernel.separable() {
        apply_convolution_separable(image, kernel, &col_vec, &row_vec)
    } else {
        apply_convolution_2d(image, kernel)
    }
}

/// Two-pass separable convolution: horizontal pass with `row_vec`, then
/// vertical pass with `col_vec`. The divisor and bias are applied only in
/// the second pass to avoid double-rounding.
///
/// The intermediate buffer stores `i32` per channel to preserve precision
/// between passes.
fn apply_convolution_separable(
    image: &DecodedImage,
    kernel: &Kernel,
    col_vec: &[i32],
    row_vec: &[i32],
) -> DecodedImage {
    let w = image.width as usize;
    let h = image.height as usize;
    let half = (kernel.size / 2) as isize;

    // --- Pass 1: horizontal (convolve each row with row_vec) ---
    // Store intermediate results as i32 to avoid clamping between passes.
    // Layout: 3 channels (R, G, B) per pixel, row-major.
    let row_channels = w * 3;
    let mut tmp = vec![0i32; h * row_channels];

    tmp.par_chunks_mut(row_channels).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let mut sum_r: i32 = 0;
            let mut sum_g: i32 = 0;
            let mut sum_b: i32 = 0;

            for k in 0..kernel.size {
                let sx = (x as isize + k as isize - half).clamp(0, w as isize - 1) as usize;
                let src = (y * w + sx) * 4;
                let weight = row_vec[k];

                sum_r += image.rgba[src] as i32 * weight;
                sum_g += image.rgba[src + 1] as i32 * weight;
                sum_b += image.rgba[src + 2] as i32 * weight;
            }

            let dst = x * 3;
            row[dst] = sum_r;
            row[dst + 1] = sum_g;
            row[dst + 2] = sum_b;
        }
    });

    // --- Pass 2: vertical (convolve each column with col_vec) ---
    let row_bytes = w * 4;
    let mut out = vec![0u8; h * row_bytes];

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let mut sum_r: i32 = 0;
            let mut sum_g: i32 = 0;
            let mut sum_b: i32 = 0;

            for k in 0..kernel.size {
                let sy = (y as isize + k as isize - half).clamp(0, h as isize - 1) as usize;
                let src = sy * row_channels + x * 3;
                let weight = col_vec[k];

                sum_r += tmp[src] * weight;
                sum_g += tmp[src + 1] * weight;
                sum_b += tmp[src + 2] * weight;
            }

            let dst = x * 4;
            row[dst] = (sum_r / kernel.divisor + kernel.bias).clamp(0, 255) as u8;
            row[dst + 1] = (sum_g / kernel.divisor + kernel.bias).clamp(0, 255) as u8;
            row[dst + 2] = (sum_b / kernel.divisor + kernel.bias).clamp(0, 255) as u8;
            row[dst + 3] = image.rgba[(y * w + x) * 4 + 3]; // alpha unchanged
        }
    });

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

/// Standard 2D convolution for non-separable kernels.
fn apply_convolution_2d(image: &DecodedImage, kernel: &Kernel) -> DecodedImage {
    let w = image.width as usize;
    let h = image.height as usize;
    let half = (kernel.size / 2) as isize;
    let row_bytes = w * 4;
    let mut out = vec![0u8; h * row_bytes];

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let mut sum_r: i32 = 0;
            let mut sum_g: i32 = 0;
            let mut sum_b: i32 = 0;

            for ky in 0..kernel.size {
                for kx in 0..kernel.size {
                    let sy = (y as isize + ky as isize - half).clamp(0, h as isize - 1) as usize;
                    let sx = (x as isize + kx as isize - half).clamp(0, w as isize - 1) as usize;
                    let src = (sy * w + sx) * 4;
                    let weight = kernel.weights[ky * kernel.size + kx];

                    sum_r += image.rgba[src] as i32 * weight;
                    sum_g += image.rgba[src + 1] as i32 * weight;
                    sum_b += image.rgba[src + 2] as i32 * weight;
                }
            }

            let dst = x * 4;
            row[dst] = (sum_r / kernel.divisor + kernel.bias).clamp(0, 255) as u8;
            row[dst + 1] = (sum_g / kernel.divisor + kernel.bias).clamp(0, 255) as u8;
            row[dst + 2] = (sum_b / kernel.divisor + kernel.bias).clamp(0, 255) as u8;
            row[dst + 3] = image.rgba[(y * w + x) * 4 + 3]; // alpha unchanged
        }
    });

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_convolution, apply_transform, invert_colors, ConvolutionFilter, ImageTransform, Kernel};
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
        assert_eq!(ImageTransform::Convolution(ConvolutionFilter::Blur).inverse(), None);
        assert_eq!(ImageTransform::Convolution(ConvolutionFilter::Sharpen).inverse(), None);
        assert_eq!(
            ImageTransform::Convolution(ConvolutionFilter::EdgeDetect).inverse(),
            None
        );
        assert_eq!(ImageTransform::Convolution(ConvolutionFilter::Emboss).inverse(), None);
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

    // --- Kernel validation ---

    #[test]
    #[should_panic(expected = "kernel size must be odd and positive")]
    fn kernel_rejects_even_size() {
        Kernel::new(vec![1; 4], 2, 1, 0);
    }

    #[test]
    #[should_panic(expected = "kernel size must be odd and positive")]
    fn kernel_rejects_zero_size() {
        Kernel::new(vec![], 0, 1, 0);
    }

    #[test]
    #[should_panic(expected = "expected 9 weights")]
    fn kernel_rejects_wrong_weight_count() {
        Kernel::new(vec![1, 2, 3, 4], 3, 1, 0);
    }

    #[test]
    #[should_panic(expected = "kernel divisor must not be zero")]
    fn kernel_rejects_zero_divisor() {
        Kernel::new(vec![1; 9], 3, 0, 0);
    }

    // --- Convolution engine ---

    #[test]
    fn convolution_identity_kernel_preserves_image() {
        // Identity kernel: [0,0,0, 0,1,0, 0,0,0] / 1
        let image = DecodedImage {
            width: 3,
            height: 3,
            rgba: vec![
                10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255, 130, 140, 150, 255, 160, 170,
                180, 255, 190, 200, 210, 255, 220, 230, 240, 255, 250, 240, 230, 255,
            ],
        };
        let kernel = Kernel::new(vec![0, 0, 0, 0, 1, 0, 0, 0, 0], 3, 1, 0);
        let result = apply_convolution(&image, &kernel);
        assert_eq!(result.rgba, image.rgba);
    }

    #[test]
    fn convolution_preserves_alpha() {
        let image = DecodedImage {
            width: 2,
            height: 2,
            rgba: vec![
                100, 100, 100, 42, 100, 100, 100, 99, 100, 100, 100, 200, 100, 100, 100, 0,
            ],
        };
        let kernel = Kernel::new(vec![1; 9], 3, 9, 0);
        let result = apply_convolution(&image, &kernel);
        // Alpha should be passed through unchanged.
        assert_eq!(result.rgba[3], 42);
        assert_eq!(result.rgba[7], 99);
        assert_eq!(result.rgba[11], 200);
        assert_eq!(result.rgba[15], 0);
    }

    #[test]
    fn convolution_uniform_image_unchanged_by_blur() {
        // A uniform-color image should be unchanged by any averaging filter.
        let image = DecodedImage {
            width: 4,
            height: 4,
            rgba: [120, 80, 200, 255].repeat(16),
        };
        let result = apply_convolution(&image, &ConvolutionFilter::Blur.kernel());
        assert_eq!(result.rgba, image.rgba);
    }

    #[test]
    fn convolution_blur_reduces_contrast() {
        // A 3x3 image with a bright center pixel — blur should reduce the center value.
        let image = DecodedImage {
            width: 3,
            height: 3,
            rgba: vec![
                0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0,
                255, 0, 0, 0, 255, 0, 0, 0, 255,
            ],
        };
        let result = apply_convolution(&image, &ConvolutionFilter::Blur.kernel());
        let center = (1 * 3 + 1) * 4;
        // Center was 255, should now be 255*4/16 = 63 (only center weight 4 hits the bright pixel).
        assert_eq!(result.rgba[center], 63);
        assert_eq!(result.rgba[center + 1], 63);
        assert_eq!(result.rgba[center + 2], 63);
    }

    #[test]
    fn convolution_bias_offsets_result() {
        // With bias=128 and identity kernel, result should be original + 128 (clamped).
        let image = DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![50, 100, 200, 255],
        };
        let kernel = Kernel::new(vec![1], 1, 1, 128);
        let result = apply_convolution(&image, &kernel);
        assert_eq!(result.rgba[0], 178); // 50+128
        assert_eq!(result.rgba[1], 228); // 100+128
        assert_eq!(result.rgba[2], 255); // 200+128=328 → clamped to 255
    }

    #[test]
    fn convolution_clamps_negative_to_zero() {
        // Edge detect on uniform image should produce zeros (all differences are 0).
        let image = DecodedImage {
            width: 3,
            height: 3,
            rgba: [100, 100, 100, 255].repeat(9),
        };
        let result = apply_convolution(&image, &ConvolutionFilter::EdgeDetect.kernel());
        for chunk in result.rgba.chunks_exact(4) {
            assert_eq!(chunk[0], 0);
            assert_eq!(chunk[1], 0);
            assert_eq!(chunk[2], 0);
        }
    }

    #[test]
    fn convolution_display_formats() {
        assert_eq!(ImageTransform::Convolution(ConvolutionFilter::Blur).to_string(), "Blur");
        assert_eq!(
            ImageTransform::Convolution(ConvolutionFilter::Sharpen).to_string(),
            "Sharpen"
        );
        assert_eq!(
            ImageTransform::Convolution(ConvolutionFilter::EdgeDetect).to_string(),
            "Edge Detect"
        );
        assert_eq!(
            ImageTransform::Convolution(ConvolutionFilter::Emboss).to_string(),
            "Emboss"
        );
    }

    // --- Separable kernel tests ---

    #[test]
    fn blur_kernel_is_separable() {
        let kernel = ConvolutionFilter::Blur.kernel();
        let (col, row) = kernel.separable().expect("blur kernel should be separable");
        // [1,2,1;2,4,2;1,2,1] = [1,2,1]^T × [1,2,1]
        assert_eq!(col, vec![1, 2, 1]);
        assert_eq!(row, vec![1, 2, 1]);
    }

    #[test]
    fn sharpen_kernel_is_not_separable() {
        let kernel = ConvolutionFilter::Sharpen.kernel();
        assert!(kernel.separable().is_none(), "sharpen should not be separable");
    }

    #[test]
    fn edge_detect_kernel_is_not_separable() {
        let kernel = ConvolutionFilter::EdgeDetect.kernel();
        assert!(kernel.separable().is_none(), "edge detect should not be separable");
    }

    #[test]
    fn emboss_kernel_is_not_separable() {
        let kernel = ConvolutionFilter::Emboss.kernel();
        assert!(kernel.separable().is_none(), "emboss should not be separable");
    }

    #[test]
    fn identity_1x1_kernel_is_separable() {
        let kernel = Kernel::new(vec![1], 1, 1, 0);
        let (col, row) = kernel.separable().expect("1x1 kernel should be separable");
        assert_eq!(col, vec![1]);
        assert_eq!(row, vec![1]);
    }

    #[test]
    fn gaussian_5x5_is_separable() {
        // 5x5 Gaussian: [1,4,6,4,1]^T × [1,4,6,4,1], divisor=256
        let row_vec = vec![1, 4, 6, 4, 1];
        let mut weights = Vec::with_capacity(25);
        for &r in &row_vec {
            for &c in &row_vec {
                weights.push(r * c);
            }
        }
        let kernel = Kernel::new(weights, 5, 256, 0);
        let (col, row) = kernel.separable().expect("5x5 Gaussian should be separable");
        assert_eq!(col, vec![1, 4, 6, 4, 1]);
        assert_eq!(row, vec![1, 4, 6, 4, 1]);
    }

    #[test]
    fn non_separable_arbitrary_kernel() {
        // A 3x3 kernel that is clearly not rank-1.
        let kernel = Kernel::new(vec![1, 0, 1, 0, 1, 0, 1, 0, 1], 3, 1, 0);
        assert!(kernel.separable().is_none());
    }

    #[test]
    fn separable_and_2d_paths_produce_identical_results() {
        use super::apply_convolution_2d;

        // Use the blur kernel (separable) on a non-trivial image.
        // The dispatcher should use the separable path; we also force 2D and compare.
        let image = DecodedImage {
            width: 5,
            height: 5,
            // Gradient pattern so the result is non-trivial.
            rgba: (0..25)
                .flat_map(|i| {
                    let v = (i * 10) as u8;
                    [v, v.wrapping_add(30), v.wrapping_add(60), 255]
                })
                .collect(),
        };
        let kernel = ConvolutionFilter::Blur.kernel();
        assert!(kernel.separable().is_some(), "precondition: blur is separable");

        let result_separable = apply_convolution(&image, &kernel); // dispatches to separable
        let result_2d = apply_convolution_2d(&image, &kernel); // force 2D path

        assert_eq!(result_separable.width, result_2d.width);
        assert_eq!(result_separable.height, result_2d.height);
        assert_eq!(
            result_separable.rgba, result_2d.rgba,
            "separable and 2D convolution paths must produce identical output"
        );
    }
}
