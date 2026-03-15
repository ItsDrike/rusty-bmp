use std::fmt;

use rayon::prelude::*;

use crate::runtime::decode::DecodedImage;

/// A convolution kernel of arbitrary odd size (3x3, 5x5, 7x7, ...).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Kernel {
    pub weights: Vec<i32>,
    pub size: usize,
    pub divisor: i32,
    pub bias: i32,
}

impl Kernel {
    /// Creates a new convolution kernel.
    ///
    /// # Panics
    /// Panics if `size` is zero or even, if `weights.len() != size * size`,
    /// or if `divisor == 0`.
    #[must_use]
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

    #[must_use]
    pub fn separable(&self) -> Option<(Vec<i32>, Vec<i32>)> {
        let n = self.size;
        if n == 1 {
            return Some((vec![1], self.weights.clone()));
        }

        let ref_row_idx = (0..n).find(|&y| (0..n).any(|x| self.weights[y * n + x] != 0))?;
        let row_vec: Vec<i32> = (0..n).map(|x| self.weights[ref_row_idx * n + x]).collect();
        let ref_col_idx = (0..n).find(|&x| row_vec[x] != 0)?;
        let ref_val = row_vec[ref_col_idx];

        let mut col_vec = Vec::with_capacity(n);
        for y in 0..n {
            let val = self.weights[y * n + ref_col_idx];
            if val % ref_val != 0 {
                return None;
            }
            col_vec.push(val / ref_val);
        }

        for (y, col) in col_vec.iter().enumerate() {
            for (x, row) in row_vec.iter().enumerate() {
                if *col * *row != self.weights[y * n + x] {
                    return None;
                }
            }
        }

        Some((col_vec, row_vec))
    }

    #[must_use]
    pub fn replay_cost(&self) -> u32 {
        let n = u32::try_from(self.size).unwrap_or(u32::MAX);
        if self.separable().is_some() { 2 * n } else { n * n }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConvolutionFilter {
    Blur,
    Sharpen,
    EdgeDetect,
    Emboss,
}

impl ConvolutionFilter {
    #[must_use]
    pub fn kernel(&self) -> Kernel {
        match self {
            Self::Blur => Kernel::new(vec![1, 2, 1, 2, 4, 2, 1, 2, 1], 3, 16, 0),
            Self::Sharpen => Kernel::new(vec![0, -1, 0, -1, 5, -1, 0, -1, 0], 3, 1, 0),
            Self::EdgeDetect => Kernel::new(vec![-1, -1, -1, -1, 8, -1, -1, -1, -1], 3, 1, 0),
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

#[must_use]
pub fn apply_convolution(image: &DecodedImage, kernel: &Kernel) -> DecodedImage {
    if let Some((col_vec, row_vec)) = kernel.separable() {
        apply_convolution_separable(image, kernel, &col_vec, &row_vec)
    } else {
        apply_convolution_2d(image, kernel)
    }
}

fn apply_convolution_separable(
    image: &DecodedImage,
    kernel: &Kernel,
    col_vec: &[i32],
    row_vec: &[i32],
) -> DecodedImage {
    let w = image.width as usize;
    let h = image.height as usize;
    let half = (kernel.size / 2) as isize;

    let row_channels = w * 3;
    let mut tmp = vec![0i32; h * row_channels];

    tmp.par_chunks_mut(row_channels).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let mut sum_r: i32 = 0;
            let mut sum_g: i32 = 0;
            let mut sum_b: i32 = 0;

            for (k, &weight) in row_vec.iter().enumerate().take(kernel.size) {
                let sx = (x as isize + k as isize - half).clamp(0, w as isize - 1) as usize;
                let src = (y * w + sx) * 4;

                sum_r += i32::from(image.rgba[src]) * weight;
                sum_g += i32::from(image.rgba[src + 1]) * weight;
                sum_b += i32::from(image.rgba[src + 2]) * weight;
            }

            let dst = x * 3;
            row[dst] = sum_r;
            row[dst + 1] = sum_g;
            row[dst + 2] = sum_b;
        }
    });

    let row_bytes = w * 4;
    let mut out = vec![0u8; h * row_bytes];

    out.par_chunks_mut(row_bytes).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let mut sum_r: i32 = 0;
            let mut sum_g: i32 = 0;
            let mut sum_b: i32 = 0;

            for (k, &weight) in col_vec.iter().enumerate().take(kernel.size) {
                let sy = (y as isize + k as isize - half).clamp(0, h as isize - 1) as usize;
                let src = sy * row_channels + x * 3;

                sum_r += tmp[src] * weight;
                sum_g += tmp[src + 1] * weight;
                sum_b += tmp[src + 2] * weight;
            }

            let dst = x * 4;
            row[dst] = (sum_r / kernel.divisor + kernel.bias).clamp(0, 255) as u8;
            row[dst + 1] = (sum_g / kernel.divisor + kernel.bias).clamp(0, 255) as u8;
            row[dst + 2] = (sum_b / kernel.divisor + kernel.bias).clamp(0, 255) as u8;
            row[dst + 3] = image.rgba[(y * w + x) * 4 + 3];
        }
    });

    DecodedImage {
        width: image.width,
        height: image.height,
        rgba: out,
    }
}

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

                    sum_r += i32::from(image.rgba[src]) * weight;
                    sum_g += i32::from(image.rgba[src + 1]) * weight;
                    sum_b += i32::from(image.rgba[src + 2]) * weight;
                }
            }

            let dst = x * 4;
            row[dst] = (sum_r / kernel.divisor + kernel.bias).clamp(0, 255) as u8;
            row[dst + 1] = (sum_g / kernel.divisor + kernel.bias).clamp(0, 255) as u8;
            row[dst + 2] = (sum_b / kernel.divisor + kernel.bias).clamp(0, 255) as u8;
            row[dst + 3] = image.rgba[(y * w + x) * 4 + 3];
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
    use super::{ConvolutionFilter, Kernel, apply_convolution, apply_convolution_2d};
    use crate::runtime::decode::DecodedImage;

    #[test]
    #[should_panic(expected = "kernel size must be odd and positive")]
    fn kernel_rejects_even_size() {
        let _ = Kernel::new(vec![1; 4], 2, 1, 0);
    }

    #[test]
    fn blur_kernel_is_separable() {
        let kernel = ConvolutionFilter::Blur.kernel();
        let (col, row) = kernel.separable().expect("blur kernel should be separable");
        assert_eq!(col, vec![1, 2, 1]);
        assert_eq!(row, vec![1, 2, 1]);
    }

    #[test]
    fn convolution_identity_kernel_preserves_image() {
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
    fn separable_and_2d_paths_produce_identical_results() {
        let image = DecodedImage {
            width: 5,
            height: 5,
            rgba: (0..25)
                .flat_map(|i| {
                    let v = (i * 10) as u8;
                    [v, v.wrapping_add(30), v.wrapping_add(60), 255]
                })
                .collect(),
        };
        let kernel = ConvolutionFilter::Blur.kernel();
        let result_separable = apply_convolution(&image, &kernel);
        let result_2d = apply_convolution_2d(&image, &kernel);
        assert_eq!(result_separable.rgba, result_2d.rgba);
    }
}
