//! Geometric image transformations.
//!
//! This module implements spatial transformations that modify the
//! positions of pixels rather than their color values.
//!
//! Most operations support multiple interpolation methods and use
//! Rayon for parallel processing across image rows.

use std::fmt;

use rayon::prelude::*;

use crate::runtime::decode::DecodedImage;

use super::model::{ImageTransform, TransformError, TransformOp};

/// Interpolation methods used when sampling pixels at non-integer coordinates.
///
/// These are primarily used during geometric transformations such as
/// rotation, resizing, and skewing where the source pixel location
/// does not fall exactly on integer pixel coordinates.
///
/// Higher-quality interpolation methods produce smoother results but
/// require more computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RotationInterpolation {
    /// Nearest-neighbor interpolation.
    ///
    /// The closest pixel is selected without blending.
    ///
    /// This is the fastest method but can produce blocky or jagged
    /// artifacts, especially when scaling or rotating images.
    ///
    /// For pixel art, this method is often preferred.
    Nearest,

    /// Bilinear interpolation.
    ///
    /// Computes a weighted average of the 4 nearest pixels.
    ///
    /// Produces smoother results than nearest-neighbor and is commonly
    /// used for real-time image transformations due to its balance
    /// between quality and performance.
    Bilinear,

    /// Bicubic interpolation.
    ///
    /// Uses a 4x4 neighborhood of pixels and a cubic interpolation
    /// function to produce smoother results than bilinear interpolation.
    ///
    /// This method is slower but provides higher-quality resampling,
    /// particularly when enlarging images.
    Bicubic,
}

impl fmt::Display for RotationInterpolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nearest => write!(f, "Nearest"),
            Self::Bilinear => write!(f, "Bilinear"),
            Self::Bicubic => write!(f, "Bicubic"),
        }
    }
}

/// Determines how image bounds are handled when translating (shifting)
/// an image.
///
/// Translation moves every pixel by `(dx, dy)`, which may cause parts
/// of the image to move outside the original bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TranslateMode {
    /// Keeps the output image the same size as the input.
    ///
    /// Pixels shifted outside the image bounds are discarded,
    /// effectively cropping the translated image.
    Crop,

    /// Expands the output canvas so the entire translated image fits.
    ///
    /// Newly exposed areas are filled with a specified background color.
    Expand,
}

impl fmt::Display for TranslateMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Crop => write!(f, "Crop"),
            Self::Expand => write!(f, "Expand"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RotateLeft;

impl fmt::Display for RotateLeft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Rotate Left")
    }
}

impl TransformOp for RotateLeft {
    /// Rotates the image 90 deg counterclockwise.
    ///
    /// The output image dimensions become:
    ///
    /// ```text
    /// width'  = height
    /// height' = width
    /// ```
    ///
    /// Pixel mapping:
    ///
    /// ```text
    /// (x, y) -> (y, width - 1 - x)
    /// ```
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        let src_w = image.width() as usize;
        let src_h = image.height() as usize;
        let dst_w = src_h;
        let dst_h = src_w;
        let row_bytes = dst_w * 4;
        let mut out = vec![0_u8; dst_w * dst_h * 4];
        let src_rgba = image.rgba();

        out.par_chunks_mut(row_bytes).enumerate().for_each(|(dst_y, row)| {
            let x = src_w - 1 - dst_y;
            for dst_x in 0..dst_w {
                let y = dst_x;
                let src = (y * src_w + x) * 4;
                let dst = dst_x * 4;
                row[dst..dst + 4].copy_from_slice(&src_rgba[src..src + 4]);
            }
        });

        DecodedImage::new(dst_w as u32, dst_h as u32, out).map_err(TransformError::from)
    }

    fn inverse(&self) -> Option<ImageTransform> {
        Some(RotateRight.into())
    }

    fn replay_cost(&self) -> u32 {
        0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RotateRight;

impl fmt::Display for RotateRight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Rotate Right")
    }
}

impl TransformOp for RotateRight {
    /// Rotates the image 90 deg clockwise.
    ///
    /// The output image dimensions become:
    ///
    /// ```text
    /// width'  = height
    /// height' = width
    /// ```
    ///
    /// Pixel mapping:
    ///
    /// ```text
    /// (x, y) -> (height - 1 - y, x)
    /// ```
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        let src_w = image.width() as usize;
        let src_h = image.height() as usize;
        let dst_w = src_h;
        let dst_h = src_w;
        let row_bytes = dst_w * 4;
        let mut out = vec![0_u8; dst_w * dst_h * 4];
        let src_rgba = image.rgba();

        out.par_chunks_mut(row_bytes).enumerate().for_each(|(dst_y, row)| {
            let x = dst_y;
            for dst_x in 0..dst_w {
                let y = src_h - 1 - dst_x;
                let src = (y * src_w + x) * 4;
                let dst = dst_x * 4;
                row[dst..dst + 4].copy_from_slice(&src_rgba[src..src + 4]);
            }
        });

        DecodedImage::new(dst_w as u32, dst_h as u32, out).map_err(TransformError::from)
    }

    fn inverse(&self) -> Option<ImageTransform> {
        Some(RotateLeft.into())
    }

    fn replay_cost(&self) -> u32 {
        0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RotateAny {
    pub angle_tenths: i32,
    pub interpolation: RotationInterpolation,
    pub expand: bool,
}

impl RotateAny {
    /// Returns the rotation angle in degrees.
    ///
    /// The internal representation stores the angle in tenths of a degree
    /// (`angle_tenths`). This converts it to a floating-point degree value for
    /// use in trigonometric calculations.
    fn angle_degrees(self) -> f32 {
        debug_assert!((-36000..=36000).contains(&self.angle_tenths));

        // Safe: f32 has 24 bits of precision, which fits the expected range of angle_tenths
        #[allow(clippy::cast_precision_loss)]
        return self.angle_tenths as f32 / 10.0;
    }
}

impl fmt::Display for RotateAny {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let angle = self.angle_degrees();
        let mode = if self.expand { "Expand" } else { "Crop" };
        write!(f, "Rotate {angle:+.1} deg ({}, {mode})", self.interpolation)
    }
}

impl TransformOp for RotateAny {
    /// Rotates the image by an arbitrary angle.
    ///
    /// The rotation is performed around the image center using inverse
    /// coordinate mapping.
    ///
    /// If the angle is approximately a multiple of 90 deg, this operation
    /// dispatches to the specialized fast rotations.
    ///
    /// Pixels outside the source image are filled with transparent black.
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        let angle_degrees = self.angle_degrees();

        let src_w = image.width() as usize;
        let src_h = image.height() as usize;

        if self.expand || src_w == src_h {
            let turns = (angle_degrees / 90.0).round() as i32;
            let snapped = turns as f32 * 90.0;
            if (angle_degrees - snapped).abs() < 1e-4 {
                return match turns.rem_euclid(4) {
                    0 => Ok(image.clone()),
                    1 => RotateLeft.apply(image),
                    2 => {
                        let once = RotateLeft.apply(image)?;
                        RotateLeft.apply(&once)
                    }
                    3 => RotateRight.apply(image),
                    _ => unreachable!(),
                };
            }
        }

        let angle = angle_degrees.to_radians();
        let cos = angle.cos();
        let sin = angle.sin();

        let src_cx = (src_w as f32 - 1.0) * 0.5;
        let src_cy = (src_h as f32 - 1.0) * 0.5;

        let (dst_w, dst_h) = if self.expand {
            let abs_cos = cos.abs();
            let abs_sin = sin.abs();
            let w_f = src_w as f32 * abs_cos + src_h as f32 * abs_sin;
            let h_f = src_w as f32 * abs_sin + src_h as f32 * abs_cos;
            let w = if (w_f - w_f.round()).abs() < 1e-4 {
                w_f.round() as usize
            } else {
                w_f.ceil() as usize
            };
            let h = if (h_f - h_f.round()).abs() < 1e-4 {
                h_f.round() as usize
            } else {
                h_f.ceil() as usize
            };
            (w.max(1), h.max(1))
        } else {
            (src_w, src_h)
        };

        let dst_cx = (dst_w as f32 - 1.0) * 0.5;
        let dst_cy = (dst_h as f32 - 1.0) * 0.5;
        let row_bytes = dst_w * 4;
        let mut out = vec![0_u8; dst_w * dst_h * 4];

        out.par_chunks_mut(row_bytes).enumerate().for_each(|(dy, row)| {
            let y = dy as f32 - dst_cy;
            for dx in 0..dst_w {
                let x = dx as f32 - dst_cx;
                let sx = x * cos + y * sin + src_cx;
                let sy = -x * sin + y * cos + src_cy;

                let dst = dx * 4;
                let sample = sample_rgba(image, sx, sy, self.interpolation);
                row[dst..dst + 4].copy_from_slice(&sample);
            }
        });

        DecodedImage::new(dst_w as u32, dst_h as u32, out).map_err(TransformError::from)
    }

    fn inverse(&self) -> Option<ImageTransform> {
        None
    }

    fn replay_cost(&self) -> u32 {
        match self.interpolation {
            RotationInterpolation::Nearest => 3,
            RotationInterpolation::Bilinear => 5,
            RotationInterpolation::Bicubic => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Resize {
    pub width: u32,
    pub height: u32,
    pub interpolation: RotationInterpolation,
}

impl fmt::Display for Resize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Resize to {}x{} ({})", self.width, self.height, self.interpolation)
    }
}

impl TransformOp for Resize {
    /// Resizes the image to the configured dimensions.
    ///
    /// Resampling uses center-based coordinate mapping and the selected
    /// interpolation method.
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        let src_w = image.width();
        let src_h = image.height();
        let dst_w = self.width.max(1);
        let dst_h = self.height.max(1);

        if src_w == dst_w && src_h == dst_h {
            return Ok(image.clone());
        }

        let row_bytes = (dst_w as usize) * 4;
        let len = row_bytes * dst_h as usize;
        let mut out = vec![0_u8; len];

        let sx_scale = src_w as f32 / dst_w as f32;
        let sy_scale = src_h as f32 / dst_h as f32;

        out.par_chunks_mut(row_bytes).enumerate().for_each(|(dy, row)| {
            let sy = ((dy as f32 + 0.5) * sy_scale - 0.5).clamp(0.0, src_h as f32 - 1.0);
            for dx in 0..dst_w {
                let sx = ((dx as f32 + 0.5) * sx_scale - 0.5).clamp(0.0, src_w as f32 - 1.0);
                let dst = dx as usize * 4;
                let px = sample_rgba(image, sx, sy, self.interpolation);
                row[dst..dst + 4].copy_from_slice(&px);
            }
        });

        DecodedImage::new(dst_w, dst_h, out).map_err(TransformError::from)
    }

    fn inverse(&self) -> Option<ImageTransform> {
        None
    }

    fn replay_cost(&self) -> u32 {
        match self.interpolation {
            RotationInterpolation::Nearest => 2,
            RotationInterpolation::Bilinear => 4,
            RotationInterpolation::Bicubic => 7,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Skew {
    pub x_milli: i16,
    pub y_milli: i16,
    pub interpolation: RotationInterpolation,
    pub expand: bool,
}

impl fmt::Display for Skew {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kx = f32::from(self.x_milli) / 1000.0;
        let ky = f32::from(self.y_milli) / 1000.0;
        let mode = if self.expand { "Expand" } else { "Crop" };
        write!(f, "Skew x={kx:+.3}, y={ky:+.3} ({}, {mode})", self.interpolation)
    }
}

impl TransformOp for Skew {
    /// Applies a shear (skew) transformation to the image.
    ///
    /// The transformation is centered around the image center and uses inverse
    /// mapping, with optional canvas expansion.
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        let src_w = image.width();
        let src_h = image.height();

        let kx = f64::from(self.x_milli) / 1000.0;
        let ky = f64::from(self.y_milli) / 1000.0;

        let det = 1.0_f64 - kx * ky;
        if det.abs() < 1e-6 {
            return Ok(image.clone());
        }

        let src_cx = (f64::from(src_w) - 1.0) * 0.5;
        let src_cy = (f64::from(src_h) - 1.0) * 0.5;

        let (dst_w, dst_h) = if self.expand {
            let corners = [
                (-src_cx, -src_cy),
                (src_cx, -src_cy),
                (src_cx, src_cy),
                (-src_cx, src_cy),
            ];

            let mut min_x = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_y = f64::NEG_INFINITY;

            for (x, y) in corners {
                let dx = x + kx * y;
                let dy = ky * x + y;
                min_x = min_x.min(dx);
                max_x = max_x.max(dx);
                min_y = min_y.min(dy);
                max_y = max_y.max(dy);
            }

            let w_f = (max_x - min_x).ceil().max(0.0);
            let h_f = (max_y - min_y).ceil().max(0.0);

            if !w_f.is_finite() || !h_f.is_finite() {
                return Ok(image.clone());
            }
            if w_f > f64::from(u32::MAX - 1) || h_f > f64::from(u32::MAX - 1) {
                return Ok(image.clone());
            }

            let w_u32 = w_f as u32 + 1;
            let h_u32 = h_f as u32 + 1;

            (w_u32.max(1), h_u32.max(1))
        } else {
            (src_w, src_h)
        };

        let dst_cx = (f64::from(dst_w) - 1.0) * 0.5;
        let dst_cy = (f64::from(dst_h) - 1.0) * 0.5;

        let row_bytes = (dst_w * 4) as usize;
        let len = row_bytes * dst_h as usize;
        let mut out = vec![0_u8; len];
        let inv = 1.0 / det;

        out.par_chunks_mut(row_bytes).enumerate().for_each(|(dy_i, row)| {
            let dy_i = dy_i as u32;
            let y = f64::from(dy_i) - dst_cy;
            for dx_i in 0..dst_w {
                let x = f64::from(dx_i) - dst_cx;
                let sx_rel = (x - kx * y) * inv;
                let sy_rel = (-ky * x + y) * inv;

                let sx = sx_rel + src_cx;
                let sy = sy_rel + src_cy;
                let dst = dx_i as usize * 4;
                let sample = sample_rgba(image, sx as f32, sy as f32, self.interpolation);
                row[dst..dst + 4].copy_from_slice(&sample);
            }
        });

        DecodedImage::new(dst_w, dst_h, out).map_err(TransformError::from)
    }

    fn inverse(&self) -> Option<ImageTransform> {
        None
    }

    fn replay_cost(&self) -> u32 {
        match self.interpolation {
            RotationInterpolation::Nearest => 3,
            RotationInterpolation::Bilinear => 5,
            RotationInterpolation::Bicubic => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Translate {
    pub dx: i32,
    pub dy: i32,
    pub mode: TranslateMode,
    pub fill: [u8; 4],
}

impl fmt::Display for Translate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Translate dx={:+}, dy={:+} ({}, fill #{:02X}{:02X}{:02X}{:02X})",
            self.dx, self.dy, self.mode, self.fill[0], self.fill[1], self.fill[2], self.fill[3]
        )
    }
}

impl TransformOp for Translate {
    /// Translates (shifts) the image by the configured offset.
    ///
    /// Output bounds are handled according to `mode`; newly exposed pixels are
    /// filled with `fill`.
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        let src_w = image.width() as usize;
        let src_h = image.height() as usize;
        let src_w_i32 = image.width_i32();
        let src_h_i32 = image.height_i32();

        let (dst_w, dst_h, x_base, y_base) = match self.mode {
            TranslateMode::Crop => (src_w, src_h, 0_i32, 0_i32),
            TranslateMode::Expand => (
                src_w + self.dx.unsigned_abs() as usize,
                src_h + self.dy.unsigned_abs() as usize,
                (-self.dx).max(0),
                (-self.dy).max(0),
            ),
        };

        let row_bytes = dst_w * 4;
        let mut out = vec![0_u8; row_bytes * dst_h];
        out.par_chunks_mut(4).for_each(|px| px.copy_from_slice(&self.fill));
        let src_rgba = image.rgba();

        out.par_chunks_mut(row_bytes).enumerate().for_each(|(dst_y, row)| {
            for dst_x in 0..dst_w {
                let src_x = dst_x as i32 - self.dx - x_base;
                let src_y = dst_y as i32 - self.dy - y_base;

                if src_x >= 0 && src_x < src_w_i32 && src_y >= 0 && src_y < src_h_i32 {
                    let src = (src_y as usize * src_w + src_x as usize) * 4;
                    let dst = dst_x * 4;
                    row[dst..dst + 4].copy_from_slice(&src_rgba[src..src + 4]);
                }
            }
        });

        DecodedImage::new(dst_w as u32, dst_h as u32, out).map_err(TransformError::from)
    }

    fn inverse(&self) -> Option<ImageTransform> {
        None
    }

    fn replay_cost(&self) -> u32 {
        2
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl fmt::Display for Crop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Crop x={}, y={}, {}x{}", self.x, self.y, self.width, self.height)
    }
}

impl TransformOp for Crop {
    /// Extracts a rectangular region from the image.
    ///
    /// The crop rectangle is clamped to source bounds and output size is at
    /// least `1x1`.
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        let src_w = image.width();
        let src_h = image.height();

        let x0 = self.x.min(src_w.saturating_sub(1));
        let y0 = self.y.min(src_h.saturating_sub(1));
        let max_w = src_w - x0;
        let max_h = src_h - y0;
        let out_w = self.width.max(1).min(max_w);
        let out_h = self.height.max(1).min(max_h);

        if x0 == 0 && y0 == 0 && out_w == src_w && out_h == src_h {
            return Ok(image.clone());
        }

        let dst_width = out_w as usize;
        let dst_height = out_h as usize;
        let src_w_usize = src_w as usize;
        let row_bytes = dst_width * 4;
        let mut out = vec![0_u8; row_bytes * dst_height];
        let src_rgba = image.rgba();

        out.par_chunks_mut(row_bytes).enumerate().for_each(|(dy, row)| {
            let sy = y0 as usize + dy;
            let src = (sy * src_w_usize + x0 as usize) * 4;
            row.copy_from_slice(&src_rgba[src..src + row_bytes]);
        });

        DecodedImage::new(out_w, out_h, out).map_err(TransformError::from)
    }

    fn inverse(&self) -> Option<ImageTransform> {
        None
    }

    fn replay_cost(&self) -> u32 {
        1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MirrorHorizontal;

impl fmt::Display for MirrorHorizontal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Mirror Horizontal")
    }
}

impl TransformOp for MirrorHorizontal {
    /// Mirrors the image horizontally (left <-> right).
    ///
    /// Each row is reversed while keeping image dimensions unchanged.
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        let w = image.width() as usize;
        let row_bytes = w * 4;
        let src_rgba = image.rgba();
        let mut out = vec![0_u8; src_rgba.len()];

        out.par_chunks_mut(row_bytes).enumerate().for_each(|(y, row)| {
            for x in 0..w {
                let src = (y * w + x) * 4;
                let dst_x = w - 1 - x;
                let dst = dst_x * 4;
                row[dst..dst + 4].copy_from_slice(&src_rgba[src..src + 4]);
            }
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
pub struct MirrorVertical;

impl fmt::Display for MirrorVertical {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Mirror Vertical")
    }
}

impl TransformOp for MirrorVertical {
    /// Mirrors the image vertically (top <-> bottom).
    ///
    /// Rows are swapped while keeping image dimensions unchanged.
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        let w = image.width() as usize;
        let h = image.height() as usize;
        let row_bytes = w * 4;
        let src_rgba = image.rgba();
        let mut out = vec![0_u8; src_rgba.len()];

        out.par_chunks_mut(row_bytes).enumerate().for_each(|(y, row)| {
            let src_y = h - 1 - y;
            let src = src_y * row_bytes;
            row.copy_from_slice(&src_rgba[src..src + row_bytes]);
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

/// Samples a pixel from the image at fractional coordinates.
///
/// The sampling method depends on the chosen interpolation mode.
///
/// # Interpolation
///
/// * `Nearest` - nearest neighbor sampling.
/// * `Bilinear` - weighted average of the 4 nearest pixels.
/// * `Bicubic` - cubic interpolation using a 4x4 pixel neighborhood.
///
/// Coordinates outside the image return transparent black.
fn sample_rgba(image: &DecodedImage, x: f32, y: f32, interpolation: RotationInterpolation) -> [u8; 4] {
    let w = image.width_i32();
    let h = image.height_i32();
    let max_x = (w - 1) as f32;
    let max_y = (h - 1) as f32;
    const EPS: f32 = 1e-3;
    if x < -EPS || y < -EPS || x > max_x + EPS || y > max_y + EPS {
        return [0, 0, 0, 0];
    }
    let x = x.clamp(0.0, max_x);
    let y = y.clamp(0.0, max_y);

    match interpolation {
        RotationInterpolation::Nearest => {
            #[allow(clippy::cast_sign_loss)]
            let xi = x.round() as u32;
            #[allow(clippy::cast_sign_loss)]
            let yi = y.round() as u32;
            // SAFETY: coordinates are clamped to `[0, w-1] x [0, h-1]` above.
            unsafe { image.pixel_unchecked(xi, yi) }
        }
        RotationInterpolation::Bilinear => {
            let x0 = x.floor() as i32;
            let y0 = y.floor() as i32;
            let x1 = (x0 + 1).min(w - 1);
            let y1 = (y0 + 1).min(h - 1);

            let tx = x - x0 as f32;
            let ty = y - y0 as f32;

            #[allow(clippy::cast_sign_loss)]
            let (x0u, y0u, x1u, y1u) = (x0 as u32, y0 as u32, x1 as u32, y1 as u32);
            // SAFETY: `(x0,y0)`, `(x1,y0)`, `(x0,y1)`, `(x1,y1)` are clamped to image bounds.
            let p00 = unsafe { image.pixel_unchecked(x0u, y0u) };
            // SAFETY: see above.
            let p10 = unsafe { image.pixel_unchecked(x1u, y0u) };
            // SAFETY: see above.
            let p01 = unsafe { image.pixel_unchecked(x0u, y1u) };
            // SAFETY: see above.
            let p11 = unsafe { image.pixel_unchecked(x1u, y1u) };

            let mut out = [0_u8; 4];
            for c in 0..4 {
                let a = f32::from(p00[c]) * (1.0 - tx) + f32::from(p10[c]) * tx;
                let b = f32::from(p01[c]) * (1.0 - tx) + f32::from(p11[c]) * tx;
                out[c] = (a * (1.0 - ty) + b * ty).round().clamp(0.0, 255.0) as u8;
            }
            out
        }
        RotationInterpolation::Bicubic => {
            let x0 = x.floor() as i32;
            let y0 = y.floor() as i32;
            let tx = x - x0 as f32;
            let ty = y - y0 as f32;

            let wx = [
                cubic_weight(1.0 + tx),
                cubic_weight(tx),
                cubic_weight(1.0 - tx),
                cubic_weight(2.0 - tx),
            ];
            let wy = [
                cubic_weight(1.0 + ty),
                cubic_weight(ty),
                cubic_weight(1.0 - ty),
                cubic_weight(2.0 - ty),
            ];

            let mut out = [0_u8; 4];
            for (c, out_chan) in out.iter_mut().enumerate() {
                let mut sum = 0.0f32;
                for (j, &w_y) in wy.iter().enumerate() {
                    let sy = (y0 + j as i32 - 1).clamp(0, h - 1);
                    for (i, &w_x) in wx.iter().enumerate() {
                        let sx = (x0 + i as i32 - 1).clamp(0, w - 1);
                        #[allow(clippy::cast_sign_loss)]
                        let (sx, sy) = (sx as u32, sy as u32);
                        // SAFETY: `(sx, sy)` is clamped to image bounds.
                        let px = unsafe { image.pixel_unchecked(sx, sy) };
                        sum += f32::from(px[c]) * w_x * w_y;
                    }
                }
                *out_chan = sum.round().clamp(0.0, 255.0) as u8;
            }
            out
        }
    }
}

/// Cubic interpolation kernel used for bicubic sampling.
///
/// Implements a Catmull-Rom style cubic filter (`a = -0.5`),
/// commonly used for smooth image resampling.
fn cubic_weight(t: f32) -> f32 {
    let a = -0.5f32;
    let x = t.abs();
    if x <= 1.0 {
        (a + 2.0) * x * x * x - (a + 3.0) * x * x + 1.0
    } else if x < 2.0 {
        a * x * x * x - 5.0 * a * x * x + 8.0 * a * x - 4.0 * a
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Crop, ImageTransform, MirrorHorizontal, MirrorVertical, Resize, RotateAny, RotateLeft, RotateRight,
        RotationInterpolation, Skew, TransformOp, Translate, TranslateMode,
    };
    use crate::runtime::decode::DecodedImage;

    #[test]
    fn rotate_any_zero_is_identity() {
        let image = DecodedImage::new(
            3,
            2,
            vec![
                10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255, 130, 140, 150, 255, 160, 170,
                180, 255,
            ],
        )
        .expect("valid image");
        let out = RotateAny {
            angle_tenths: 0,
            interpolation: RotationInterpolation::Bilinear,
            expand: true,
        }
        .apply(&image)
        .expect("rotate any should always succeed");
        assert_eq!(out.rgba(), image.rgba());
    }

    #[test]
    fn rotate_any_90_matches_rotate_left() {
        let image = DecodedImage::new(
            3,
            2,
            vec![
                1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18, 255,
            ],
        )
        .expect("valid image");
        let expected = RotateLeft.apply(&image).expect("rotate left should always succeed");
        let got = RotateAny {
            angle_tenths: 900,
            interpolation: RotationInterpolation::Nearest,
            expand: true,
        }
        .apply(&image)
        .expect("rotate any should always succeed");
        assert_eq!(got.rgba(), expected.rgba());
    }

    #[test]
    fn resize_identity_preserves_image() {
        let image = DecodedImage::new(
            3,
            2,
            vec![
                1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18, 255,
            ],
        )
        .expect("valid image");
        let out = Resize {
            width: 3,
            height: 2,
            interpolation: RotationInterpolation::Bicubic,
        }
        .apply(&image)
        .expect("resize should always succeed");
        assert_eq!(out.rgba(), image.rgba());
    }

    #[test]
    fn resize_upscale_keeps_opaque_edges() {
        let image = DecodedImage::new(
            2,
            2,
            vec![10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255],
        )
        .expect("valid image");

        for interpolation in [
            RotationInterpolation::Nearest,
            RotationInterpolation::Bilinear,
            RotationInterpolation::Bicubic,
        ] {
            let out = Resize {
                width: 3,
                height: 3,
                interpolation,
            }
            .apply(&image)
            .expect("resize should always succeed");

            for px in out.rgba().chunks_exact(4) {
                assert_eq!(px[3], 255, "unexpected transparent pixel with {interpolation:?}");
            }
        }
    }

    #[test]
    fn skew_zero_is_identity() {
        let image = DecodedImage::new(
            3,
            2,
            vec![
                1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18, 255,
            ],
        )
        .expect("valid image");
        let out = Skew {
            x_milli: 0,
            y_milli: 0,
            interpolation: RotationInterpolation::Nearest,
            expand: false,
        }
        .apply(&image)
        .expect("skew should always succeed");
        assert_eq!(out.rgba(), image.rgba());
    }

    #[test]
    fn translate_zero_is_identity() {
        let image = DecodedImage::new(
            3,
            2,
            vec![
                1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18, 255,
            ],
        )
        .expect("valid image");
        let out = Translate {
            dx: 0,
            dy: 0,
            mode: TranslateMode::Crop,
            fill: [0, 0, 0, 0],
        }
        .apply(&image)
        .expect("translate should always succeed");
        assert_eq!(out.rgba(), image.rgba());
    }

    #[test]
    fn crop_full_image_is_identity() {
        let image = DecodedImage::new(
            2,
            2,
            vec![10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255],
        )
        .expect("valid image");
        let out = Crop {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        }
        .apply(&image)
        .expect("crop should always succeed");
        assert_eq!(out.rgba(), image.rgba());
    }

    #[test]
    fn inverse_of_rotate_left_is_rotate_right() {
        let inv = RotateLeft.inverse().expect("rotate left should have inverse");
        assert!(matches!(inv, ImageTransform::RotateRight(_)));
    }

    #[test]
    fn self_inverse_geometry_transforms() {
        let inv = MirrorHorizontal
            .inverse()
            .expect("mirror horizontal should self-invert");
        assert!(matches!(inv, ImageTransform::MirrorHorizontal(_)));

        let inv = MirrorVertical.inverse().expect("mirror vertical should self-invert");
        assert!(matches!(inv, ImageTransform::MirrorVertical(_)));
    }

    #[test]
    fn rotate_any_display_format() {
        let op = RotateAny {
            angle_tenths: -125,
            interpolation: RotationInterpolation::Bicubic,
            expand: false,
        };
        assert_eq!(op.to_string(), "Rotate -12.5 deg (Bicubic, Crop)");
    }

    #[test]
    fn translate_display_format() {
        let op = Translate {
            dx: -12,
            dy: 7,
            mode: TranslateMode::Expand,
            fill: [0x10, 0x20, 0x30, 0x40],
        };
        assert_eq!(op.to_string(), "Translate dx=-12, dy=+7 (Expand, fill #10203040)");
    }

    #[test]
    fn replay_cost_rotate_any_depends_on_interpolation() {
        let nearest = RotateAny {
            angle_tenths: 123,
            interpolation: RotationInterpolation::Nearest,
            expand: true,
        }
        .replay_cost();
        let bilinear = RotateAny {
            angle_tenths: 123,
            interpolation: RotationInterpolation::Bilinear,
            expand: true,
        }
        .replay_cost();
        assert!(bilinear > nearest);
    }

    #[test]
    fn apply_then_inverse_is_identity_for_invertible_ops() {
        let image = DecodedImage::new(
            3,
            2,
            vec![
                10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255, 130, 140, 150, 255, 160, 170,
                180, 255,
            ],
        )
        .expect("valid image");

        let ops: Vec<ImageTransform> = vec![
            RotateLeft.into(),
            RotateRight.into(),
            MirrorHorizontal.into(),
            MirrorVertical.into(),
        ];

        for op in ops {
            let inv = op.inverse().expect("reversible transform should have inverse");
            let transformed = op.apply(&image).expect("apply should succeed");
            let restored = inv.apply(&transformed).expect("inverse apply should succeed");
            assert_eq!(restored.rgba(), image.rgba());
        }
    }

    #[test]
    fn smoke_construct_geometry_ops() {
        let _ = RotateLeft;
        let _ = RotateRight;
        let _ = RotateAny {
            angle_tenths: 15,
            interpolation: RotationInterpolation::Nearest,
            expand: false,
        };
        let _ = Resize {
            width: 10,
            height: 10,
            interpolation: RotationInterpolation::Bilinear,
        };
        let _ = Skew {
            x_milli: 10,
            y_milli: 20,
            interpolation: RotationInterpolation::Bicubic,
            expand: true,
        };
        let _ = Translate {
            dx: 1,
            dy: 2,
            mode: TranslateMode::Crop,
            fill: [0, 0, 0, 0],
        };
        let _ = Crop {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
    }
}
