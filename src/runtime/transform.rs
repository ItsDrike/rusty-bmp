use std::collections::BTreeMap;
use std::fmt;

use rayon::prelude::*;

use crate::runtime::decode::DecodedImage;
use crate::runtime::steganography::{self, StegConfig};

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

    /// Returns a heuristic replay cost for this convolution kernel.
    ///
    /// The cost approximates the amount of work per pixel and is used by the
    /// transform pipeline to decide when to create checkpoints.
    ///
    /// Non-separable kernels require a full 2D convolution with `N × N` taps,
    /// while separable kernels can be applied as two 1D passes (`2 × N` taps).
    ///
    /// Examples:
    /// - 3×3 separable kernel -> `2N = 6`
    /// - 3×3 non-separable kernel -> `N² = 9`
    /// - 5×5 separable kernel -> `2N = 10`
    /// - 5×5 non-separable kernel -> `N² = 25`
    pub fn replay_cost(&self) -> u32 {
        let n = self.size as u32;
        if self.separable().is_some() {
            2 * n
        } else {
            n * n
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RotationInterpolation {
    Nearest,
    Bilinear,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TranslateMode {
    Crop,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageTransform {
    RotateLeft90,
    RotateRight90,
    /// Rotate by an arbitrary angle in 0.1° units.
    RotateAny {
        angle_tenths: i16,
        interpolation: RotationInterpolation,
        expand: bool,
    },
    /// Resize image to explicit output dimensions.
    Resize {
        width: u32,
        height: u32,
        interpolation: RotationInterpolation,
    },
    /// Shear image along X/Y axes by affine skew factors.
    Skew {
        /// X shear factor in thousandths (kx = x_milli / 1000).
        x_milli: i16,
        /// Y shear factor in thousandths (ky = y_milli / 1000).
        y_milli: i16,
        interpolation: RotationInterpolation,
        expand: bool,
    },
    /// Translate image by integer pixel offsets.
    Translate {
        dx: i32,
        dy: i32,
        mode: TranslateMode,
        fill: [u8; 4],
    },
    /// Crop to a rectangle in image coordinates.
    Crop {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    MirrorHorizontal,
    MirrorVertical,
    InvertColors,
    Grayscale,
    Sepia,
    /// Adjust brightness by a signed delta (clamped to 0..=255 per channel).
    Brightness(i16),
    /// Adjust contrast by a signed delta using the standard 259-based formula.
    Contrast(i16),
    /// Apply a convolution filter (blur, sharpen, etc.).
    Convolution(ConvolutionFilter),
    /// Apply a user-defined convolution kernel.
    CustomKernel(Kernel),
    /// Embed a steganographic payload into the image LSBs.
    ///
    /// The payload is stored as a `Vec<u8>` so the transform is self-contained
    /// and can be replayed through the pipeline like any other transform.
    EmbedSteganography {
        config: StegConfig,
        /// The raw bytes to embed (arbitrary binary data; text must be UTF-8
        /// encoded by the caller).
        payload: Vec<u8>,
    },
    /// Remove any steganographic payload embedded with the given config by
    /// zeroing the relevant LSBs.
    RemoveSteganography {
        config: StegConfig,
    },
}

impl fmt::Display for ImageTransform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RotateLeft90 => write!(f, "Rotate Left"),
            Self::RotateRight90 => write!(f, "Rotate Right"),
            Self::RotateAny {
                angle_tenths,
                interpolation,
                expand,
            } => {
                let angle = *angle_tenths as f32 / 10.0;
                let mode = if *expand { "Expand" } else { "Crop" };
                write!(f, "Rotate {angle:+.1}° ({interpolation}, {mode})")
            }
            Self::Resize {
                width,
                height,
                interpolation,
            } => write!(f, "Resize to {width}x{height} ({interpolation})"),
            Self::Skew {
                x_milli,
                y_milli,
                interpolation,
                expand,
            } => {
                let kx = *x_milli as f32 / 1000.0;
                let ky = *y_milli as f32 / 1000.0;
                let mode = if *expand { "Expand" } else { "Crop" };
                write!(f, "Skew x={kx:+.3}, y={ky:+.3} ({interpolation}, {mode})")
            }
            Self::Translate { dx, dy, mode, fill } => write!(
                f,
                "Translate dx={dx:+}, dy={dy:+} ({mode}, fill #{:02X}{:02X}{:02X}{:02X})",
                fill[0], fill[1], fill[2], fill[3]
            ),
            Self::Crop { x, y, width, height } => write!(f, "Crop x={x}, y={y}, {width}x{height}"),
            Self::MirrorHorizontal => write!(f, "Mirror Horizontal"),
            Self::MirrorVertical => write!(f, "Mirror Vertical"),
            Self::InvertColors => write!(f, "Invert Colors"),
            Self::Grayscale => write!(f, "Grayscale"),
            Self::Sepia => write!(f, "Sepia"),
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
            Self::CustomKernel(k) => write!(f, "Custom {}x{}", k.size, k.size),
            Self::EmbedSteganography { config, payload } => write!(
                f,
                "Embed Steganography ({} bytes, R{}G{}B{}A{})",
                payload.len(),
                config.r_bits,
                config.g_bits,
                config.b_bits,
                config.a_bits
            ),
            Self::RemoveSteganography { config } => write!(
                f,
                "Remove Steganography (R{}G{}B{}A{})",
                config.r_bits, config.g_bits, config.b_bits, config.a_bits
            ),
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
            Self::RotateAny { .. } => None,
            Self::Resize { .. } => None,
            Self::Skew { .. } => None,
            Self::Translate { .. } => None,
            Self::Crop { .. } => None,
            Self::MirrorHorizontal => Some(Self::MirrorHorizontal),
            Self::MirrorVertical => Some(Self::MirrorVertical),
            Self::InvertColors => Some(Self::InvertColors),
            // Lossy: clamping destroys information, requires pipeline replay.
            Self::Grayscale => None,
            Self::Sepia => None,
            Self::Brightness(_) => None,
            Self::Contrast(_) => None,
            Self::Convolution(_) => None,
            Self::CustomKernel(_) => None,
            // Steganography: lossy in the sense that the original LSBs are
            // overwritten. Removing undoes an embed and vice versa.
            Self::EmbedSteganography { .. } => None,
            Self::RemoveSteganography { .. } => None,
        }
    }

    /// Estimated relative cost of replaying this transform, used to decide
    /// when to create pipeline checkpoints.
    ///
    /// Invertible transforms return 0 because they never trigger a full
    /// pipeline replay (undo applies the inverse directly). Cheap per-pixel
    /// transforms return 1. Convolutions are significantly more expensive
    /// and return a higher value.
    pub fn replay_cost(&self) -> u32 {
        match self {
            // Invertible — never replayed on undo, so checkpoint cost is 0.
            Self::RotateLeft90
            | Self::RotateRight90
            | Self::MirrorHorizontal
            | Self::MirrorVertical
            | Self::InvertColors => 0,
            // Cheap per-pixel transforms.
            Self::Grayscale | Self::Sepia | Self::Brightness(_) | Self::Contrast(_) => 1,
            // Arbitrary-angle rotation needs interpolation and coordinate transforms.
            Self::RotateAny { interpolation, .. } => match interpolation {
                RotationInterpolation::Nearest => 3,
                RotationInterpolation::Bilinear => 5,
                RotationInterpolation::Bicubic => 8,
            },
            Self::Resize { interpolation, .. } => match interpolation {
                RotationInterpolation::Nearest => 2,
                RotationInterpolation::Bilinear => 4,
                RotationInterpolation::Bicubic => 7,
            },
            Self::Skew { interpolation, .. } => match interpolation {
                RotationInterpolation::Nearest => 3,
                RotationInterpolation::Bilinear => 5,
                RotationInterpolation::Bicubic => 8,
            },
            Self::Translate { .. } => 2,
            Self::Crop { .. } => 1,
            // Convolutions scale with kernel footprint.
            Self::Convolution(filter) => filter.kernel().replay_cost(),
            Self::CustomKernel(kernel) => kernel.replay_cost(),
            // Steganography is a per-pixel LSB pass — similar cost to a cheap
            // color transform.
            Self::EmbedSteganography { .. } => 2,
            Self::RemoveSteganography { .. } => 1,
        }
    }
}

/// Cost threshold that triggers creating a new checkpoint.
/// Accumulated [`ImageTransform::replay_cost()`] since the last checkpoint
/// must exceed this value before a new snapshot is stored.
const CHECKPOINT_COST_THRESHOLD: u32 = 15;

/// Maximum number of checkpoints stored simultaneously. When exceeded, the
/// oldest checkpoint is evicted to bound memory usage.
const MAX_CHECKPOINTS: usize = 5;

#[derive(Debug, Default, Clone)]
pub struct TransformPipeline {
    ops: Vec<ImageTransform>,
    /// Cached intermediate images keyed by pipeline index. A checkpoint at
    /// index `i` stores the image state *after* applying `ops[i]`.
    checkpoints: BTreeMap<usize, DecodedImage>,
    /// Accumulated replay cost since the last checkpoint (or pipeline start).
    cost_since_checkpoint: u32,
}

impl TransformPipeline {
    /// Appends a transform and optionally creates a checkpoint if the
    /// accumulated cost since the last checkpoint exceeds the threshold.
    ///
    /// `current_image` is the image state *before* this transform is applied.
    /// If a checkpoint is warranted, `current_image` is cloned into the cache
    /// (representing the state after the *previous* op, i.e. one index before
    /// the new op).
    pub fn push(&mut self, op: ImageTransform, current_image: Option<&DecodedImage>) {
        let cost = op.replay_cost();
        self.cost_since_checkpoint += cost;

        // Only create checkpoints for non-invertible ops (invertible ops
        // never trigger replay, so checkpoints don't help them).
        if cost > 0 && self.cost_since_checkpoint >= CHECKPOINT_COST_THRESHOLD && !self.ops.is_empty() {
            if let Some(img) = current_image {
                // Store the state *before* this new op (= after the last op).
                let checkpoint_idx = self.ops.len() - 1;
                self.checkpoints.insert(checkpoint_idx, img.clone());

                // Evict oldest if we exceed the cap.
                while self.checkpoints.len() > MAX_CHECKPOINTS {
                    let oldest = *self.checkpoints.keys().next().unwrap();
                    self.checkpoints.remove(&oldest);
                }
            }
            self.cost_since_checkpoint = 0;
        }

        self.ops.push(op);
    }

    pub fn clear(&mut self) {
        self.ops.clear();
        self.checkpoints.clear();
        self.cost_since_checkpoint = 0;
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

    /// Removes an op at `index` and invalidates all checkpoints at or after
    /// that index (they are based on a pipeline that no longer exists).
    pub fn remove(&mut self, index: usize) {
        self.ops.remove(index);
        // Remove checkpoints >= index. After removal, ops shift down, so
        // any checkpoint at index or later is invalid.
        self.checkpoints.retain(|&k, _| k < index);
        self.recompute_cost_since_checkpoint();
    }

    /// Pops the last op and removes its checkpoint if one exists at that index.
    pub fn pop(&mut self) -> Option<ImageTransform> {
        let op = self.ops.pop()?;
        let len = self.ops.len();
        // Remove the checkpoint at the (now gone) index.
        self.checkpoints.remove(&len);
        self.recompute_cost_since_checkpoint();
        Some(op)
    }

    /// Replays the full pipeline starting from `original`, using the nearest
    /// checkpoint to skip already-computed work.
    pub fn apply(&self, original: &DecodedImage) -> DecodedImage {
        self.apply_range(original, self.ops.len())
    }

    /// Replays the full pipeline and returns non-fatal replay warnings.
    ///
    /// Currently this reports steganography embed steps that could not be
    /// applied (typically because the image dimensions changed and capacity
    /// is no longer sufficient).
    pub fn apply_with_warnings(&self, original: &DecodedImage) -> (DecodedImage, Vec<String>) {
        self.apply_range_with_warnings(original, self.ops.len())
    }

    /// Replays ops `[0..count)` starting from `original`, using the nearest
    /// checkpoint at or before the target range to skip work.
    fn apply_range(&self, original: &DecodedImage, count: usize) -> DecodedImage {
        self.apply_range_with_warnings(original, count).0
    }

    fn apply_range_with_warnings(&self, original: &DecodedImage, count: usize) -> (DecodedImage, Vec<String>) {
        // Find the best checkpoint: largest index that is < count.
        let (start_idx, mut out) = self
            .checkpoints
            .range(..count)
            .next_back()
            .map(|(&idx, img)| (idx + 1, img.clone()))
            .unwrap_or_else(|| (0, original.clone()));

        let mut warnings = Vec::new();

        for op in &self.ops[start_idx..count] {
            match op {
                ImageTransform::EmbedSteganography { config, payload } => {
                    match steganography::embed(&out, *config, payload) {
                        Ok(next) => out = next,
                        Err(err) => warnings.push(format!(
                            "Skipped steganography step during replay: payload no longer fits ({err})"
                        )),
                    }
                }
                _ => {
                    out = apply_transform(&out, op);
                }
            }
        }
        (out, warnings)
    }

    /// Recalculates `cost_since_checkpoint` based on the current ops and
    /// checkpoint positions.
    fn recompute_cost_since_checkpoint(&mut self) {
        let last_cp = self.checkpoints.keys().next_back().copied();
        let start = last_cp.map_or(0, |i| i + 1);
        self.cost_since_checkpoint = self.ops[start..].iter().map(|op| op.replay_cost()).sum();
    }
}

pub fn apply_transform(image: &DecodedImage, op: &ImageTransform) -> DecodedImage {
    match op {
        ImageTransform::RotateLeft90 => rotate_left(image),
        ImageTransform::RotateRight90 => rotate_right(image),
        ImageTransform::RotateAny {
            angle_tenths,
            interpolation,
            expand,
        } => rotate_any(image, *angle_tenths as f32 / 10.0, *interpolation, *expand),
        ImageTransform::Resize {
            width,
            height,
            interpolation,
        } => resize_image(image, *width, *height, *interpolation),
        ImageTransform::Skew {
            x_milli,
            y_milli,
            interpolation,
            expand,
        } => skew_image(
            image,
            *x_milli as f32 / 1000.0,
            *y_milli as f32 / 1000.0,
            *interpolation,
            *expand,
        ),
        ImageTransform::Translate { dx, dy, mode, fill } => translate_image(image, *dx, *dy, *mode, *fill),
        ImageTransform::Crop { x, y, width, height } => crop_image(image, *x, *y, *width, *height),
        ImageTransform::MirrorHorizontal => mirror_horizontal(image),
        ImageTransform::MirrorVertical => mirror_vertical(image),
        ImageTransform::InvertColors => invert_colors(image),
        ImageTransform::Grayscale => grayscale(image),
        ImageTransform::Sepia => sepia(image),
        ImageTransform::Brightness(delta) => brightness(image, *delta),
        ImageTransform::Contrast(delta) => contrast(image, *delta),
        ImageTransform::Convolution(filter) => apply_convolution(image, &filter.kernel()),
        ImageTransform::CustomKernel(kernel) => apply_convolution(image, kernel),
        ImageTransform::EmbedSteganography { config, payload } => {
            // Capacity can become insufficient after pipeline edits/replay
            // (e.g. crop inserted before a previously valid embed op).
            // Keep replay resilient: if embedding fails here, leave the image
            // unchanged and let higher-level UI logic surface warnings.
            steganography::embed(image, *config, payload).unwrap_or_else(|_| image.clone())
        }
        ImageTransform::RemoveSteganography { config } => steganography::remove(image, *config),
    }
}

pub fn skew_image(
    image: &DecodedImage,
    kx: f32,
    ky: f32,
    interpolation: RotationInterpolation,
    expand: bool,
) -> DecodedImage {
    let src_w = image.width as usize;
    let src_h = image.height as usize;
    if src_w == 0 || src_h == 0 {
        return image.clone();
    }

    let det = 1.0 - kx * ky;
    if det.abs() < 1e-6 {
        // Nearly singular affine transform: avoid unstable inversion.
        return image.clone();
    }

    let src_cx = (src_w as f32 - 1.0) * 0.5;
    let src_cy = (src_h as f32 - 1.0) * 0.5;

    let (dst_w, dst_h) = if expand {
        let corners = [
            (-src_cx, -src_cy),
            (src_cx, -src_cy),
            (src_cx, src_cy),
            (-src_cx, src_cy),
        ];

        let mut min_x = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for (x, y) in corners {
            let dx = x + kx * y;
            let dy = ky * x + y;
            min_x = min_x.min(dx);
            max_x = max_x.max(dx);
            min_y = min_y.min(dy);
            max_y = max_y.max(dy);
        }

        let w = (max_x - min_x).ceil() as usize + 1;
        let h = (max_y - min_y).ceil() as usize + 1;
        (w.max(1), h.max(1))
    } else {
        (src_w, src_h)
    };

    let dst_cx = (dst_w as f32 - 1.0) * 0.5;
    let dst_cy = (dst_h as f32 - 1.0) * 0.5;

    let row_bytes = dst_w * 4;
    let mut out = vec![0_u8; dst_w * dst_h * 4];
    let inv = 1.0 / det;

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(dy_i, row)| {
        let y = dy_i as f32 - dst_cy;
        for dx_i in 0..dst_w {
            let x = dx_i as f32 - dst_cx;

            // Inverse map: src_rel = A^-1 * dst_rel where
            // A = [[1, kx], [ky, 1]], det = 1 - kx*ky.
            let sx_rel = (x - kx * y) * inv;
            let sy_rel = (-ky * x + y) * inv;

            let sx = sx_rel + src_cx;
            let sy = sy_rel + src_cy;
            let dst = dx_i * 4;
            let sample = sample_rgba(image, sx, sy, interpolation);
            row[dst..dst + 4].copy_from_slice(&sample);
        }
    });

    DecodedImage {
        width: dst_w as u32,
        height: dst_h as u32,
        rgba: out,
    }
}

pub fn translate_image(image: &DecodedImage, dx: i32, dy: i32, mode: TranslateMode, fill: [u8; 4]) -> DecodedImage {
    let src_w = image.width as usize;
    let src_h = image.height as usize;
    if src_w == 0 || src_h == 0 {
        return image.clone();
    }

    let (dst_w, dst_h, x_base, y_base) = match mode {
        TranslateMode::Crop => (src_w, src_h, 0_i32, 0_i32),
        TranslateMode::Expand => (
            src_w + dx.unsigned_abs() as usize,
            src_h + dy.unsigned_abs() as usize,
            (-dx).max(0),
            (-dy).max(0),
        ),
    };

    let mut out = vec![0_u8; dst_w * dst_h * 4];
    out.par_chunks_mut(4).for_each(|px| px.copy_from_slice(&fill));
    let row_bytes = dst_w * 4;

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(dst_y, row)| {
        for dst_x in 0..dst_w {
            let src_x = dst_x as i32 - dx - x_base;
            let src_y = dst_y as i32 - dy - y_base;

            if src_x >= 0 && src_x < src_w as i32 && src_y >= 0 && src_y < src_h as i32 {
                let src = (src_y as usize * src_w + src_x as usize) * 4;
                let dst = dst_x * 4;
                row[dst..dst + 4].copy_from_slice(&image.rgba[src..src + 4]);
            }
        }
    });

    DecodedImage {
        width: dst_w as u32,
        height: dst_h as u32,
        rgba: out,
    }
}

pub fn crop_image(image: &DecodedImage, x: u32, y: u32, width: u32, height: u32) -> DecodedImage {
    let src_w = image.width;
    let src_h = image.height;
    if src_w == 0 || src_h == 0 {
        return image.clone();
    }

    let x0 = x.min(src_w.saturating_sub(1));
    let y0 = y.min(src_h.saturating_sub(1));
    let max_w = src_w - x0;
    let max_h = src_h - y0;
    let out_w = width.max(1).min(max_w);
    let out_h = height.max(1).min(max_h);

    if x0 == 0 && y0 == 0 && out_w == src_w && out_h == src_h {
        return image.clone();
    }

    let out_w_usize = out_w as usize;
    let out_h_usize = out_h as usize;
    let src_w_usize = src_w as usize;
    let row_bytes = out_w_usize * 4;
    let mut out = vec![0_u8; out_w_usize * out_h_usize * 4];

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(dy, row)| {
        let sy = y0 as usize + dy;
        let src = (sy * src_w_usize + x0 as usize) * 4;
        row.copy_from_slice(&image.rgba[src..src + row_bytes]);
    });

    DecodedImage {
        width: out_w,
        height: out_h,
        rgba: out,
    }
}

pub fn resize_image(
    image: &DecodedImage,
    out_width: u32,
    out_height: u32,
    interpolation: RotationInterpolation,
) -> DecodedImage {
    let src_w = image.width as usize;
    let src_h = image.height as usize;
    let dst_w = out_width.max(1) as usize;
    let dst_h = out_height.max(1) as usize;

    if src_w == 0 || src_h == 0 {
        return DecodedImage {
            width: dst_w as u32,
            height: dst_h as u32,
            rgba: vec![0; dst_w * dst_h * 4],
        };
    }

    if src_w == dst_w && src_h == dst_h {
        return image.clone();
    }

    let mut out = vec![0_u8; dst_w * dst_h * 4];
    let row_bytes = dst_w * 4;

    // Pixel-center aligned mapping from destination to source coordinates.
    let sx_scale = src_w as f32 / dst_w as f32;
    let sy_scale = src_h as f32 / dst_h as f32;

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(dy, row)| {
        let sy = (dy as f32 + 0.5) * sy_scale - 0.5;
        for dx in 0..dst_w {
            let sx = (dx as f32 + 0.5) * sx_scale - 0.5;
            let dst = dx * 4;
            let px = sample_rgba(image, sx, sy, interpolation);
            row[dst..dst + 4].copy_from_slice(&px);
        }
    });

    DecodedImage {
        width: dst_w as u32,
        height: dst_h as u32,
        rgba: out,
    }
}

pub fn rotate_any(
    image: &DecodedImage,
    angle_degrees: f32,
    interpolation: RotationInterpolation,
    expand: bool,
) -> DecodedImage {
    let src_w = image.width as usize;
    let src_h = image.height as usize;
    if src_w == 0 || src_h == 0 {
        return image.clone();
    }

    if expand || src_w == src_h {
        let turns = (angle_degrees / 90.0).round() as i32;
        let snapped = turns as f32 * 90.0;
        if (angle_degrees - snapped).abs() < 1e-4 {
            match turns.rem_euclid(4) {
                0 => return image.clone(),
                1 => return rotate_left(image),
                2 => return rotate_left(&rotate_left(image)),
                3 => return rotate_right(image),
                _ => unreachable!(),
            }
        }
    }

    let angle = angle_degrees.to_radians();
    let cos = angle.cos();
    let sin = angle.sin();

    let src_cx = (src_w as f32 - 1.0) * 0.5;
    let src_cy = (src_h as f32 - 1.0) * 0.5;

    let (dst_w, dst_h) = if expand {
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

            // Inverse map from destination -> source.
            let sx = x * cos + y * sin + src_cx;
            let sy = -x * sin + y * cos + src_cy;

            let dst = dx * 4;
            let sample = sample_rgba(image, sx, sy, interpolation);
            row[dst..dst + 4].copy_from_slice(&sample);
        }
    });

    DecodedImage {
        width: dst_w as u32,
        height: dst_h as u32,
        rgba: out,
    }
}

fn sample_rgba(image: &DecodedImage, x: f32, y: f32, interpolation: RotationInterpolation) -> [u8; 4] {
    let w = image.width as i32;
    let h = image.height as i32;
    // Allow a tiny epsilon for floating-point error near borders (e.g. exact
    // 90° rotations without canvas expansion), then clamp into valid bounds.
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
            let xi = x.round() as i32;
            let yi = y.round() as i32;
            pixel_at(image, xi, yi)
        }
        RotationInterpolation::Bilinear => {
            let x0 = x.floor() as i32;
            let y0 = y.floor() as i32;
            let x1 = (x0 + 1).min(w - 1);
            let y1 = (y0 + 1).min(h - 1);

            let tx = x - x0 as f32;
            let ty = y - y0 as f32;

            let p00 = pixel_at(image, x0, y0);
            let p10 = pixel_at(image, x1, y0);
            let p01 = pixel_at(image, x0, y1);
            let p11 = pixel_at(image, x1, y1);

            let mut out = [0_u8; 4];
            for c in 0..4 {
                let a = p00[c] as f32 * (1.0 - tx) + p10[c] as f32 * tx;
                let b = p01[c] as f32 * (1.0 - tx) + p11[c] as f32 * tx;
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
            for c in 0..4 {
                let mut sum = 0.0f32;
                for (j, &w_y) in wy.iter().enumerate() {
                    let sy = (y0 + j as i32 - 1).clamp(0, h - 1);
                    for (i, &w_x) in wx.iter().enumerate() {
                        let sx = (x0 + i as i32 - 1).clamp(0, w - 1);
                        sum += pixel_at(image, sx, sy)[c] as f32 * w_x * w_y;
                    }
                }
                out[c] = sum.round().clamp(0.0, 255.0) as u8;
            }
            out
        }
    }
}

fn cubic_weight(t: f32) -> f32 {
    // Catmull-Rom spline (a = -0.5), common bicubic kernel for image resampling.
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

fn pixel_at(image: &DecodedImage, x: i32, y: i32) -> [u8; 4] {
    let w = image.width as usize;
    let idx = (y as usize * w + x as usize) * 4;
    [
        image.rgba[idx],
        image.rgba[idx + 1],
        image.rgba[idx + 2],
        image.rgba[idx + 3],
    ]
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

pub fn sepia(image: &DecodedImage) -> DecodedImage {
    let mut out = image.rgba.clone();
    out.par_chunks_exact_mut(4).for_each(|px| {
        let r = px[0] as f32;
        let g = px[1] as f32;
        let b = px[2] as f32;
        // Standard sepia tone matrix (Microsoft-recommended coefficients).
        let sr = (0.393 * r + 0.769 * g + 0.189 * b).round().min(255.0) as u8;
        let sg = (0.349 * r + 0.686 * g + 0.168 * b).round().min(255.0) as u8;
        let sb = (0.272 * r + 0.534 * g + 0.131 * b).round().min(255.0) as u8;
        px[0] = sr;
        px[1] = sg;
        px[2] = sb;
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
    use super::{
        apply_convolution, apply_transform, crop_image, invert_colors, resize_image, rotate_any, sepia, skew_image,
        translate_image, ConvolutionFilter, ImageTransform, Kernel, RotationInterpolation, TransformPipeline,
        TranslateMode, CHECKPOINT_COST_THRESHOLD,
    };
    use crate::runtime::decode::DecodedImage;
    use crate::runtime::steganography::{self, StegConfig};

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
        assert_eq!(ImageTransform::Sepia.inverse(), None);
        assert_eq!(
            ImageTransform::RotateAny {
                angle_tenths: 125,
                interpolation: RotationInterpolation::Bilinear,
                expand: true,
            }
            .inverse(),
            None
        );
        assert_eq!(
            ImageTransform::Resize {
                width: 320,
                height: 240,
                interpolation: RotationInterpolation::Bilinear,
            }
            .inverse(),
            None
        );
        assert_eq!(
            ImageTransform::Skew {
                x_milli: 250,
                y_milli: 0,
                interpolation: RotationInterpolation::Bilinear,
                expand: true,
            }
            .inverse(),
            None
        );
        assert_eq!(
            ImageTransform::Translate {
                dx: 10,
                dy: -4,
                mode: TranslateMode::Crop,
                fill: [0, 0, 0, 0],
            }
            .inverse(),
            None
        );
        assert_eq!(
            ImageTransform::Crop {
                x: 3,
                y: 4,
                width: 20,
                height: 10,
            }
            .inverse(),
            None
        );
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
        assert_eq!(
            ImageTransform::CustomKernel(Kernel::new(vec![0, -1, 0, -1, 5, -1, 0, -1, 0], 3, 1, 0)).inverse(),
            None
        );
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
    fn sepia_applies_correct_tone_matrix() {
        let image = DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![100, 150, 200, 128],
        };
        let result = sepia(&image);
        // R: 0.393*100 + 0.769*150 + 0.189*200 = 39.3 + 115.35 + 37.8 = 192.45 → 192
        // G: 0.349*100 + 0.686*150 + 0.168*200 = 34.9 + 102.9  + 33.6 = 171.4  → 171
        // B: 0.272*100 + 0.534*150 + 0.131*200 = 27.2 + 80.1   + 26.2 = 133.5  → 134
        assert_eq!(result.rgba[0], 192);
        assert_eq!(result.rgba[1], 171);
        assert_eq!(result.rgba[2], 134);
        assert_eq!(result.rgba[3], 128); // alpha unchanged
    }

    #[test]
    fn sepia_clamps_to_255() {
        // White pixel: all coefficients sum > 1.0 for R channel (0.393+0.769+0.189=1.351).
        let image = DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![255, 255, 255, 255],
        };
        let result = sepia(&image);
        // R: 1.351 * 255 = 344.5 → clamped to 255
        // G: 1.203 * 255 = 306.8 → clamped to 255
        // B: 0.937 * 255 = 238.9 → 239
        assert_eq!(result.rgba[0], 255);
        assert_eq!(result.rgba[1], 255);
        assert_eq!(result.rgba[2], 239);
    }

    #[test]
    fn sepia_black_stays_black() {
        let image = DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
        };
        let result = sepia(&image);
        assert_eq!(result.rgba[0], 0);
        assert_eq!(result.rgba[1], 0);
        assert_eq!(result.rgba[2], 0);
        assert_eq!(result.rgba[3], 255);
    }

    #[test]
    fn sepia_preserves_dimensions_and_alpha() {
        let image = DecodedImage {
            width: 3,
            height: 2,
            rgba: (0..6).flat_map(|i| [i * 40, i * 30, i * 20, 100 + i]).collect(),
        };
        let result = sepia(&image);
        assert_eq!(result.width, 3);
        assert_eq!(result.height, 2);
        // Verify all alpha values are preserved.
        for (i, chunk) in result.rgba.chunks_exact(4).enumerate() {
            assert_eq!(chunk[3], 100 + i as u8, "alpha mismatch at pixel {i}");
        }
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

    #[test]
    fn rotate_any_display_format() {
        let op = ImageTransform::RotateAny {
            angle_tenths: -125,
            interpolation: RotationInterpolation::Bicubic,
            expand: false,
        };
        assert_eq!(op.to_string(), "Rotate -12.5° (Bicubic, Crop)");
    }

    #[test]
    fn resize_display_format() {
        let op = ImageTransform::Resize {
            width: 640,
            height: 480,
            interpolation: RotationInterpolation::Nearest,
        };
        assert_eq!(op.to_string(), "Resize to 640x480 (Nearest)");
    }

    #[test]
    fn skew_display_format() {
        let op = ImageTransform::Skew {
            x_milli: 250,
            y_milli: -125,
            interpolation: RotationInterpolation::Bilinear,
            expand: false,
        };
        assert_eq!(op.to_string(), "Skew x=+0.250, y=-0.125 (Bilinear, Crop)");
    }

    #[test]
    fn translate_display_format() {
        let op = ImageTransform::Translate {
            dx: -12,
            dy: 7,
            mode: TranslateMode::Expand,
            fill: [0x10, 0x20, 0x30, 0x40],
        };
        assert_eq!(op.to_string(), "Translate dx=-12, dy=+7 (Expand, fill #10203040)");
    }

    #[test]
    fn crop_display_format() {
        let op = ImageTransform::Crop {
            x: 12,
            y: 7,
            width: 100,
            height: 80,
        };
        assert_eq!(op.to_string(), "Crop x=12, y=7, 100x80");
    }

    #[test]
    fn rotate_any_zero_is_identity() {
        let image = DecodedImage {
            width: 3,
            height: 2,
            rgba: vec![
                10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255, 130, 140, 150, 255, 160, 170,
                180, 255,
            ],
        };

        let out = rotate_any(&image, 0.0, RotationInterpolation::Bilinear, true);
        assert_eq!(out.width, image.width);
        assert_eq!(out.height, image.height);
        assert_eq!(out.rgba, image.rgba);

        let out_bicubic = rotate_any(&image, 0.0, RotationInterpolation::Bicubic, true);
        assert_eq!(out_bicubic.width, image.width);
        assert_eq!(out_bicubic.height, image.height);
        assert_eq!(out_bicubic.rgba, image.rgba);
    }

    #[test]
    fn rotate_any_90_matches_rotate_left() {
        let image = DecodedImage {
            width: 3,
            height: 2,
            rgba: vec![
                1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18, 255,
            ],
        };

        let expected = super::rotate_left(&image);
        let got = rotate_any(&image, 90.0, RotationInterpolation::Nearest, true);
        assert_eq!(got.width, expected.width);
        assert_eq!(got.height, expected.height);
        assert_eq!(got.rgba, expected.rgba);
    }

    #[test]
    fn rotate_any_90_no_expand_square_matches_rotate_left() {
        let image = DecodedImage {
            width: 4,
            height: 4,
            rgba: vec![
                255, 255, 255, 255, 10, 0, 0, 255, 20, 0, 0, 255, 255, 255, 255, 255, 30, 0, 0, 255, 40, 0, 0, 255,
                50, 0, 0, 255, 60, 0, 0, 255, 70, 0, 0, 255, 80, 0, 0, 255, 90, 0, 0, 255, 100, 0, 0, 255, 255, 255,
                255, 255, 110, 0, 0, 255, 120, 0, 0, 255, 255, 255, 255, 255,
            ],
        };

        let expected = super::rotate_left(&image);
        let got = rotate_any(&image, 90.0, RotationInterpolation::Nearest, false);
        assert_eq!(got.width, expected.width);
        assert_eq!(got.height, expected.height);
        assert_eq!(got.rgba, expected.rgba);
    }

    #[test]
    fn rotate_any_expand_grows_canvas_for_diagonal() {
        let image = DecodedImage {
            width: 4,
            height: 4,
            rgba: vec![255; 4 * 4 * 4],
        };

        let out = rotate_any(&image, 45.0, RotationInterpolation::Nearest, true);
        assert!(out.width > image.width);
        assert!(out.height > image.height);
    }

    #[test]
    fn resize_identity_preserves_image() {
        let image = DecodedImage {
            width: 3,
            height: 2,
            rgba: vec![
                1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18, 255,
            ],
        };
        let out = resize_image(&image, 3, 2, RotationInterpolation::Bicubic);
        assert_eq!(out.width, image.width);
        assert_eq!(out.height, image.height);
        assert_eq!(out.rgba, image.rgba);
    }

    #[test]
    fn resize_nearest_2x2_to_1x1_samples_center() {
        let image = DecodedImage {
            width: 2,
            height: 2,
            rgba: vec![10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255],
        };
        let out = resize_image(&image, 1, 1, RotationInterpolation::Nearest);
        assert_eq!(out.width, 1);
        assert_eq!(out.height, 1);
        assert_eq!(out.rgba, vec![100, 110, 120, 255]);
    }

    #[test]
    fn skew_zero_is_identity() {
        let image = DecodedImage {
            width: 3,
            height: 2,
            rgba: vec![
                1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18, 255,
            ],
        };
        let out = skew_image(&image, 0.0, 0.0, RotationInterpolation::Nearest, false);
        assert_eq!(out.width, image.width);
        assert_eq!(out.height, image.height);
        assert_eq!(out.rgba, image.rgba);
    }

    #[test]
    fn skew_expand_can_grow_canvas() {
        let image = DecodedImage {
            width: 5,
            height: 5,
            rgba: vec![255; 5 * 5 * 4],
        };
        let out = skew_image(&image, 0.7, 0.0, RotationInterpolation::Nearest, true);
        assert!(out.width >= image.width);
        assert!(out.height >= image.height);
        assert!(out.width > image.width || out.height > image.height);
    }

    #[test]
    fn translate_zero_is_identity() {
        let image = DecodedImage {
            width: 3,
            height: 2,
            rgba: vec![
                1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18, 255,
            ],
        };
        let out = translate_image(&image, 0, 0, TranslateMode::Crop, [0, 0, 0, 0]);
        assert_eq!(out.width, image.width);
        assert_eq!(out.height, image.height);
        assert_eq!(out.rgba, image.rgba);
    }

    #[test]
    fn translate_crop_right_uses_fill_on_left() {
        let image = DecodedImage {
            width: 2,
            height: 2,
            rgba: vec![10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255],
        };
        let fill = [1, 2, 3, 4];
        let out = translate_image(&image, 1, 0, TranslateMode::Crop, fill);
        assert_eq!(out.width, 2);
        assert_eq!(out.height, 2);
        assert_eq!(out.rgba, vec![1, 2, 3, 4, 10, 20, 30, 255, 1, 2, 3, 4, 70, 80, 90, 255]);
    }

    #[test]
    fn translate_expand_left_grows_canvas_and_keeps_all_pixels() {
        let image = DecodedImage {
            width: 2,
            height: 1,
            rgba: vec![10, 20, 30, 255, 40, 50, 60, 255],
        };
        let fill = [0, 0, 0, 0];
        let out = translate_image(&image, -1, 0, TranslateMode::Expand, fill);
        assert_eq!(out.width, 3);
        assert_eq!(out.height, 1);
        assert_eq!(out.rgba, vec![10, 20, 30, 255, 40, 50, 60, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn crop_full_image_is_identity() {
        let image = DecodedImage {
            width: 2,
            height: 2,
            rgba: vec![10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255],
        };
        let out = crop_image(&image, 0, 0, 2, 2);
        assert_eq!(out.width, image.width);
        assert_eq!(out.height, image.height);
        assert_eq!(out.rgba, image.rgba);
    }

    #[test]
    fn crop_center_region_extracts_expected_pixels() {
        let image = DecodedImage {
            width: 3,
            height: 2,
            rgba: vec![
                1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255, 5, 0, 0, 255, 6, 0, 0, 255,
            ],
        };
        let out = crop_image(&image, 1, 0, 2, 2);
        assert_eq!(out.width, 2);
        assert_eq!(out.height, 2);
        assert_eq!(out.rgba, vec![2, 0, 0, 255, 3, 0, 0, 255, 5, 0, 0, 255, 6, 0, 0, 255]);
    }

    #[test]
    fn crop_out_of_bounds_is_clamped() {
        let image = DecodedImage {
            width: 4,
            height: 3,
            rgba: (0..4 * 3 * 4).map(|v| v as u8).collect(),
        };
        let out = crop_image(&image, 3, 2, 10, 10);
        assert_eq!(out.width, 1);
        assert_eq!(out.height, 1);
    }

    #[test]
    fn custom_kernel_display_format() {
        let k3 = Kernel::new(vec![0; 9], 3, 1, 0);
        assert_eq!(ImageTransform::CustomKernel(k3).to_string(), "Custom 3x3");
        let k5 = Kernel::new(vec![0; 25], 5, 1, 0);
        assert_eq!(ImageTransform::CustomKernel(k5).to_string(), "Custom 5x5");
        let k1 = Kernel::new(vec![1], 1, 1, 0);
        assert_eq!(ImageTransform::CustomKernel(k1).to_string(), "Custom 1x1");
    }

    #[test]
    fn custom_kernel_replay_cost_scales_with_size() {
        let k3 = Kernel::new(vec![0; 9], 3, 1, 0);
        let k5 = Kernel::new(vec![0; 25], 5, 1, 0);
        let c3 = ImageTransform::CustomKernel(k3).replay_cost();
        let c5 = ImageTransform::CustomKernel(k5).replay_cost();
        assert!(c5 > c3, "larger custom kernels should have higher replay cost");
    }

    #[test]
    fn custom_kernel_applies_same_as_preset() {
        // Using the sharpen kernel as a custom kernel should produce the
        // same result as the Sharpen preset.
        let image = DecodedImage {
            width: 4,
            height: 4,
            rgba: (0..64).collect(),
        };
        let preset_result = apply_transform(&image, &ImageTransform::Convolution(ConvolutionFilter::Sharpen));
        let sharpen_kernel = ConvolutionFilter::Sharpen.kernel();
        let custom_result = apply_transform(&image, &ImageTransform::CustomKernel(sharpen_kernel));
        assert_eq!(preset_result.rgba, custom_result.rgba);
    }

    #[test]
    fn custom_identity_kernel_preserves_image() {
        // Identity kernel: center = 1, rest = 0, divisor = 1, bias = 0.
        let kernel = Kernel::new(vec![0, 0, 0, 0, 1, 0, 0, 0, 0], 3, 1, 0);
        let image = DecodedImage {
            width: 3,
            height: 3,
            rgba: vec![
                10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255, 130, 140, 150, 255, 160, 170,
                180, 255, 190, 200, 210, 255, 220, 230, 240, 255, 250, 245, 235, 255,
            ],
        };
        let result = apply_transform(&image, &ImageTransform::CustomKernel(kernel));
        assert_eq!(result.rgba, image.rgba);
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

    // --- Checkpoint / cost-based caching tests ---

    fn test_image() -> DecodedImage {
        DecodedImage {
            width: 2,
            height: 2,
            rgba: vec![10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255],
        }
    }

    #[test]
    fn replay_cost_invertible_is_zero() {
        assert_eq!(ImageTransform::RotateLeft90.replay_cost(), 0);
        assert_eq!(ImageTransform::RotateRight90.replay_cost(), 0);
        assert_eq!(ImageTransform::MirrorHorizontal.replay_cost(), 0);
        assert_eq!(ImageTransform::MirrorVertical.replay_cost(), 0);
        assert_eq!(ImageTransform::InvertColors.replay_cost(), 0);
    }

    #[test]
    fn replay_cost_lossy_pixel_ops() {
        assert_eq!(ImageTransform::Grayscale.replay_cost(), 1);
        assert_eq!(ImageTransform::Sepia.replay_cost(), 1);
        assert_eq!(ImageTransform::Brightness(10).replay_cost(), 1);
        assert_eq!(ImageTransform::Contrast(-5).replay_cost(), 1);
    }

    #[test]
    fn replay_cost_convolution_is_higher() {
        let cost = ImageTransform::Convolution(ConvolutionFilter::Blur).replay_cost();
        assert!(cost > 1, "convolution should have higher cost than simple pixel ops");
        let rotate_cost = ImageTransform::RotateAny {
            angle_tenths: 333,
            interpolation: RotationInterpolation::Bilinear,
            expand: true,
        }
        .replay_cost();
        assert!(
            rotate_cost > 1,
            "arbitrary rotation should have higher cost than simple pixel ops"
        );
    }

    #[test]
    fn replay_cost_rotate_any_depends_on_interpolation() {
        let nearest = ImageTransform::RotateAny {
            angle_tenths: 123,
            interpolation: RotationInterpolation::Nearest,
            expand: true,
        }
        .replay_cost();
        let bilinear = ImageTransform::RotateAny {
            angle_tenths: 123,
            interpolation: RotationInterpolation::Bilinear,
            expand: true,
        }
        .replay_cost();
        let bicubic = ImageTransform::RotateAny {
            angle_tenths: 123,
            interpolation: RotationInterpolation::Bicubic,
            expand: true,
        }
        .replay_cost();
        assert!(bilinear > nearest, "bilinear rotation should be costlier than nearest");
        assert!(bicubic > bilinear, "bicubic rotation should be costlier than bilinear");
    }

    #[test]
    fn replay_cost_resize_depends_on_interpolation() {
        let nearest = ImageTransform::Resize {
            width: 64,
            height: 64,
            interpolation: RotationInterpolation::Nearest,
        }
        .replay_cost();
        let bilinear = ImageTransform::Resize {
            width: 64,
            height: 64,
            interpolation: RotationInterpolation::Bilinear,
        }
        .replay_cost();
        let bicubic = ImageTransform::Resize {
            width: 64,
            height: 64,
            interpolation: RotationInterpolation::Bicubic,
        }
        .replay_cost();
        assert!(bilinear > nearest, "bilinear resize should be costlier than nearest");
        assert!(bicubic > bilinear, "bicubic resize should be costlier than bilinear");
    }

    #[test]
    fn replay_cost_skew_depends_on_interpolation() {
        let nearest = ImageTransform::Skew {
            x_milli: 200,
            y_milli: 0,
            interpolation: RotationInterpolation::Nearest,
            expand: true,
        }
        .replay_cost();
        let bilinear = ImageTransform::Skew {
            x_milli: 200,
            y_milli: 0,
            interpolation: RotationInterpolation::Bilinear,
            expand: true,
        }
        .replay_cost();
        let bicubic = ImageTransform::Skew {
            x_milli: 200,
            y_milli: 0,
            interpolation: RotationInterpolation::Bicubic,
            expand: true,
        }
        .replay_cost();
        assert!(bilinear > nearest, "bilinear skew should be costlier than nearest");
        assert!(bicubic > bilinear, "bicubic skew should be costlier than bilinear");
    }

    #[test]
    fn replay_cost_translate_is_low_nonzero() {
        let cost = ImageTransform::Translate {
            dx: 10,
            dy: -3,
            mode: TranslateMode::Crop,
            fill: [0, 0, 0, 0],
        }
        .replay_cost();
        assert_eq!(cost, 2);
    }

    #[test]
    fn replay_cost_crop_is_one() {
        let cost = ImageTransform::Crop {
            x: 1,
            y: 1,
            width: 10,
            height: 8,
        }
        .replay_cost();
        assert_eq!(cost, 1);
    }

    #[test]
    fn no_checkpoint_below_threshold() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        // Push cheap ops that won't reach the threshold.
        for _ in 0..5 {
            let cur = pipeline.apply(&img);
            pipeline.push(ImageTransform::Brightness(1), Some(&cur));
        }
        // Cost = 5, threshold = 10 — no checkpoint yet.
        assert!(pipeline.checkpoints.is_empty());
    }

    #[test]
    fn checkpoint_created_at_threshold() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        let mut cur = img.clone();
        // Accumulate cost to reach the threshold. Each brightness op costs 1,
        // so we need CHECKPOINT_COST_THRESHOLD + 1 ops (the first op has no
        // predecessor to checkpoint from, then we need threshold cost).
        for _ in 0..=CHECKPOINT_COST_THRESHOLD {
            pipeline.push(ImageTransform::Brightness(1), Some(&cur));
            cur = apply_transform(&cur, &ImageTransform::Brightness(1));
        }
        assert!(
            !pipeline.checkpoints.is_empty(),
            "checkpoint should be created after reaching cost threshold"
        );
    }

    #[test]
    fn checkpoint_created_faster_with_convolutions() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        let mut cur = img.clone();
        // 2 convolutions cost 10, plus one more op should trigger checkpoint.
        let conv = ImageTransform::Convolution(ConvolutionFilter::Blur);
        for _ in 0..3 {
            pipeline.push(conv.clone(), Some(&cur));
            cur = apply_transform(&cur, &conv);
        }
        assert!(
            !pipeline.checkpoints.is_empty(),
            "convolutions should trigger checkpoint faster than cheap ops"
        );
    }

    #[test]
    fn invertible_ops_do_not_trigger_checkpoint() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        // Push 50 invertible ops — none should create checkpoints since cost is 0.
        for _ in 0..50 {
            let cur = pipeline.apply(&img);
            pipeline.push(ImageTransform::MirrorHorizontal, Some(&cur));
        }
        assert!(
            pipeline.checkpoints.is_empty(),
            "invertible ops should never trigger checkpoints"
        );
    }

    #[test]
    fn apply_uses_checkpoint_and_produces_correct_result() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        let mut cur = img.clone();

        // Build up enough ops to create a checkpoint, then add more.
        let n = (CHECKPOINT_COST_THRESHOLD + 5) as usize;
        for _ in 0..n {
            pipeline.push(ImageTransform::Brightness(1), Some(&cur));
            cur = apply_transform(&cur, &ImageTransform::Brightness(1));
        }
        assert!(!pipeline.checkpoints.is_empty(), "precondition: checkpoint exists");

        // apply() from original should produce the same result as sequential application.
        let result = pipeline.apply(&img);
        assert_eq!(
            result.rgba, cur.rgba,
            "checkpoint-accelerated apply must match sequential"
        );
    }

    #[test]
    fn pop_removes_checkpoint_at_popped_index() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        let mut cur = img.clone();

        // Create enough ops to get a checkpoint.
        for _ in 0..(CHECKPOINT_COST_THRESHOLD + 2) as usize {
            pipeline.push(ImageTransform::Brightness(1), Some(&cur));
            cur = apply_transform(&cur, &ImageTransform::Brightness(1));
        }
        let cp_count_before = pipeline.checkpoints.len();
        assert!(cp_count_before > 0);

        // Pop ops until we pop past the checkpoint.
        while !pipeline.checkpoints.is_empty() {
            pipeline.pop();
        }
        assert!(
            pipeline.checkpoints.is_empty(),
            "popping should eventually clear checkpoints"
        );
    }

    #[test]
    fn clear_removes_all_checkpoints() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        let mut cur = img.clone();

        for _ in 0..(CHECKPOINT_COST_THRESHOLD + 2) as usize {
            pipeline.push(ImageTransform::Brightness(1), Some(&cur));
            cur = apply_transform(&cur, &ImageTransform::Brightness(1));
        }
        assert!(!pipeline.checkpoints.is_empty());

        pipeline.clear();
        assert!(pipeline.checkpoints.is_empty());
        assert!(pipeline.ops.is_empty());
    }

    #[test]
    fn max_checkpoints_enforced() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        let mut cur = img.clone();

        // Push enough ops to create more than MAX_CHECKPOINTS checkpoints.
        // Each batch of (threshold+1) ops creates one checkpoint.
        let batches = super::MAX_CHECKPOINTS + 3;
        for _ in 0..batches {
            for _ in 0..=CHECKPOINT_COST_THRESHOLD {
                pipeline.push(ImageTransform::Brightness(1), Some(&cur));
                cur = apply_transform(&cur, &ImageTransform::Brightness(1));
            }
        }

        assert!(
            pipeline.checkpoints.len() <= super::MAX_CHECKPOINTS,
            "should not exceed MAX_CHECKPOINTS ({}) but got {}",
            super::MAX_CHECKPOINTS,
            pipeline.checkpoints.len()
        );

        // Result should still be correct despite evictions.
        let result = pipeline.apply(&img);
        assert_eq!(result.rgba, cur.rgba);
    }

    #[test]
    fn remove_invalidates_checkpoints_at_and_after_index() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        let mut cur = img.clone();

        for _ in 0..(CHECKPOINT_COST_THRESHOLD + 2) as usize {
            pipeline.push(ImageTransform::Brightness(1), Some(&cur));
            cur = apply_transform(&cur, &ImageTransform::Brightness(1));
        }
        assert!(!pipeline.checkpoints.is_empty());

        // Remove op at index 0 — all checkpoints should be invalidated.
        pipeline.remove(0);
        assert!(
            pipeline.checkpoints.is_empty(),
            "removing index 0 should invalidate all checkpoints"
        );
    }
}
