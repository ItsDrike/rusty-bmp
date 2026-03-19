//! Wu color quantization over a 3D RGB histogram.
//!
//! # What this module does
//!
//! Given an RGBA buffer, this module builds a palette with at most
//! `max_colors` entries and then maps each pixel to one palette index.
//! It uses the classic Wu quantization approach instead of median-cut.
//!
//! # Wu quantization in plain language
//!
//! 1. We quantize RGB space into a coarse 3D grid (here 32 bins per channel).
//! 2. For each grid cell, we collect statistics about pixels that fall into it
//!    (count, channel sums, and sum of squared channel values).
//! 3. We convert those stats into cumulative "summed-volume" tables so we can
//!    query any 3D box in O(1) time using inclusion-exclusion.
//! 4. Starting from one box covering all colors, we repeatedly split the box
//!    that currently has the largest variance, choosing the split that yields
//!    the best reduction in color error.
//! 5. Each final box becomes one palette entry (its average color), and every
//!    input pixel is mapped by its histogram cell to that box's palette index.
//!
//! The result is usually better palette quality than simple axis-median splits,
//! while still being deterministic and fast enough for real-time save paths.

use thiserror::Error;

/// Number of quantized bits retained per RGB channel for the histogram grid.
const BITS_PER_CHANNEL: u8 = 5;
/// Side length of the 3D histogram including the zero border cell.
const SIDE: usize = (1usize << BITS_PER_CHANNEL) + 1;
/// Total number of histogram cells (`SIDE^3`).
const HISTOGRAM_SIZE: usize = SIDE * SIDE * SIDE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Error)]
pub enum QuantizeError {
    #[error("max_colors must be in 2..=256, got {0}")]
    InvalidMaxColors(usize),
}

/// Flattens a 3D histogram coordinate into a single linear array index.
#[inline]
const fn histogram_index(r: usize, g: usize, b: usize) -> usize {
    (r * SIDE + g) * SIDE + b
}

/// Candidate axis along which a color cube can be split.
#[derive(Clone, Copy)]
enum Axis {
    Red,
    Green,
    Blue,
}

/// A rectangular region ("box") in quantized RGB space.
///
/// The `0` coordinates are exclusive lower bounds and the `1` coordinates are
/// inclusive upper bounds. This boundary convention matches summed-volume table
/// queries and keeps inclusion-exclusion formulas simple and branch-free.
#[derive(Clone, Copy)]
struct Cube {
    r0: usize,
    r1: usize,
    g0: usize,
    g1: usize,
    b0: usize,
    b1: usize,
}

impl Cube {
    /// Returns a cube that spans the entire quantized RGB domain.
    const fn full() -> Self {
        Self {
            r0: 0,
            r1: SIDE - 1,
            g0: 0,
            g1: SIDE - 1,
            b0: 0,
            b1: SIDE - 1,
        }
    }

    /// Returns `true` when this cube still has at least one splittable axis.
    const fn can_split(self) -> bool {
        self.r1 > self.r0 + 1 || self.g1 > self.g0 + 1 || self.b1 > self.b0 + 1
    }
}

/// Summed statistics used by Wu quantization.
///
/// Each vector stores a 3D summed-volume table over histogram cells:
/// - `weight`: number of pixels
/// - `red`/`green`/`blue`: per-channel sums
/// - `sum_squares`: sum of squared channel magnitudes (`r^2 + g^2 + b^2`)
struct Moments {
    weight: Vec<u64>,
    red: Vec<u64>,
    green: Vec<u64>,
    blue: Vec<u64>,
    sum_squares: Vec<f64>,
}

/// Builds histogram bins and converts them into summed-volume moment tables.
///
/// After this preprocessing, any cube statistic can be queried in O(1) time,
/// which is the key optimization that makes Wu splitting efficient.
fn build_moments(rgba: &[u8]) -> Moments {
    let mut weight = vec![0u64; HISTOGRAM_SIZE];
    let mut red = vec![0u64; HISTOGRAM_SIZE];
    let mut green = vec![0u64; HISTOGRAM_SIZE];
    let mut blue = vec![0u64; HISTOGRAM_SIZE];
    let mut sum_squares = vec![0f64; HISTOGRAM_SIZE];

    for pixel in rgba.chunks_exact(4) {
        let r_u8 = pixel[0];
        let g_u8 = pixel[1];
        let b_u8 = pixel[2];

        let r = usize::from(r_u8 >> (8 - BITS_PER_CHANNEL)) + 1;
        let g = usize::from(g_u8 >> (8 - BITS_PER_CHANNEL)) + 1;
        let b = usize::from(b_u8 >> (8 - BITS_PER_CHANNEL)) + 1;

        let idx = histogram_index(r, g, b);

        weight[idx] += 1;
        red[idx] += u64::from(r_u8);
        green[idx] += u64::from(g_u8);
        blue[idx] += u64::from(b_u8);

        let rf = f64::from(r_u8);
        let gf = f64::from(g_u8);
        let bf = f64::from(b_u8);
        sum_squares[idx] += rf * rf + gf * gf + bf * bf;
    }

    for r in 1..SIDE {
        let mut area_weight = [0u64; SIDE];
        let mut area_red = [0u64; SIDE];
        let mut area_green = [0u64; SIDE];
        let mut area_blue = [0u64; SIDE];
        let mut area_sum_squares = [0f64; SIDE];

        for g in 1..SIDE {
            let mut line_weight = 0u64;
            let mut line_red = 0u64;
            let mut line_green = 0u64;
            let mut line_blue = 0u64;
            let mut line_sum_squares = 0f64;

            for b in 1..SIDE {
                let idx = histogram_index(r, g, b);

                line_weight += weight[idx];
                line_red += red[idx];
                line_green += green[idx];
                line_blue += blue[idx];
                line_sum_squares += sum_squares[idx];

                area_weight[b] += line_weight;
                area_red[b] += line_red;
                area_green[b] += line_green;
                area_blue[b] += line_blue;
                area_sum_squares[b] += line_sum_squares;

                let idx_prev_r = histogram_index(r - 1, g, b);
                weight[idx] = weight[idx_prev_r] + area_weight[b];
                red[idx] = red[idx_prev_r] + area_red[b];
                green[idx] = green[idx_prev_r] + area_green[b];
                blue[idx] = blue[idx_prev_r] + area_blue[b];
                sum_squares[idx] = sum_squares[idx_prev_r] + area_sum_squares[b];
            }
        }
    }

    Moments {
        weight,
        red,
        green,
        blue,
        sum_squares,
    }
}

/// Returns the integral `u64` moment value inside `cube` via inclusion-exclusion.
fn volume_u64(cube: Cube, moments: &[u64]) -> u64 {
    let volume = i128::from(moments[histogram_index(cube.r1, cube.g1, cube.b1)])
        - i128::from(moments[histogram_index(cube.r1, cube.g1, cube.b0)])
        - i128::from(moments[histogram_index(cube.r1, cube.g0, cube.b1)])
        - i128::from(moments[histogram_index(cube.r0, cube.g1, cube.b1)])
        + i128::from(moments[histogram_index(cube.r1, cube.g0, cube.b0)])
        + i128::from(moments[histogram_index(cube.r0, cube.g1, cube.b0)])
        + i128::from(moments[histogram_index(cube.r0, cube.g0, cube.b1)])
        - i128::from(moments[histogram_index(cube.r0, cube.g0, cube.b0)]);

    debug_assert!(volume >= 0);
    #[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    {
        volume as u64
    }
}

/// Returns the integral `f64` moment value inside `cube` via inclusion-exclusion.
fn volume_f64(cube: Cube, moments: &[f64]) -> f64 {
    moments[histogram_index(cube.r1, cube.g1, cube.b1)]
        - moments[histogram_index(cube.r1, cube.g1, cube.b0)]
        - moments[histogram_index(cube.r1, cube.g0, cube.b1)]
        - moments[histogram_index(cube.r0, cube.g1, cube.b1)]
        + moments[histogram_index(cube.r1, cube.g0, cube.b0)]
        + moments[histogram_index(cube.r0, cube.g1, cube.b0)]
        + moments[histogram_index(cube.r0, cube.g0, cube.b1)]
        - moments[histogram_index(cube.r0, cube.g0, cube.b0)]
}

/// Computes within-cube variance (total squared color error) for `cube`.
///
/// Larger variance means colors in the cube are less homogeneous and therefore
/// more worth splitting into smaller cubes.
#[expect(clippy::cast_precision_loss)]
fn variance(cube: Cube, moments: &Moments) -> f64 {
    let weight = volume_u64(cube, &moments.weight);
    if weight == 0 {
        return 0.0;
    }

    let red = volume_u64(cube, &moments.red) as f64;
    let green = volume_u64(cube, &moments.green) as f64;
    let blue = volume_u64(cube, &moments.blue) as f64;
    let squares = volume_f64(cube, &moments.sum_squares);

    squares - ((red * red + green * green + blue * blue) / weight as f64)
}

/// Computes Wu's split objective for one side of a partition.
///
/// This is proportional to squared mean magnitude weighted by population and is
/// combined for both sides when evaluating a candidate cut.
#[expect(clippy::cast_precision_loss)]
fn weighted_color_score(red: u64, green: u64, blue: u64, weight: u64) -> f64 {
    if weight == 0 {
        return 0.0;
    }

    let red = red as f64;
    let green = green as f64;
    let blue = blue as f64;

    (red * red + green * green + blue * blue) / weight as f64
}

/// Searches the best split point for `cube` along one `axis`.
///
/// Returns `(cut_position, score)` where `score` is the objective value for the
/// resulting two sub-cubes. `None` means no valid non-empty split exists.
fn maximize(cube: Cube, axis: Axis, moments: &Moments) -> Option<(usize, f64)> {
    let total_weight = volume_u64(cube, &moments.weight);
    if total_weight == 0 {
        return None;
    }

    let total_red = volume_u64(cube, &moments.red);
    let total_green = volume_u64(cube, &moments.green);
    let total_blue = volume_u64(cube, &moments.blue);

    let (start, end) = match axis {
        Axis::Red => (cube.r0 + 1, cube.r1),
        Axis::Green => (cube.g0 + 1, cube.g1),
        Axis::Blue => (cube.b0 + 1, cube.b1),
    };

    if start >= end {
        return None;
    }

    let mut best_cut = None;
    let mut best_score = f64::NEG_INFINITY;

    for cut in start..end {
        let mut lower = cube;
        let mut upper = cube;

        match axis {
            Axis::Red => {
                lower.r1 = cut;
                upper.r0 = cut;
            }
            Axis::Green => {
                lower.g1 = cut;
                upper.g0 = cut;
            }
            Axis::Blue => {
                lower.b1 = cut;
                upper.b0 = cut;
            }
        }

        let lower_weight = volume_u64(lower, &moments.weight);
        let upper_weight = total_weight - lower_weight;
        if lower_weight == 0 || upper_weight == 0 {
            continue;
        }

        let lower_red = volume_u64(lower, &moments.red);
        let lower_green = volume_u64(lower, &moments.green);
        let lower_blue = volume_u64(lower, &moments.blue);

        let upper_red = total_red - lower_red;
        let upper_green = total_green - lower_green;
        let upper_blue = total_blue - lower_blue;

        let score = weighted_color_score(lower_red, lower_green, lower_blue, lower_weight)
            + weighted_color_score(upper_red, upper_green, upper_blue, upper_weight);

        if score > best_score {
            best_score = score;
            best_cut = Some(cut);
        }
    }

    best_cut.map(|cut| (cut, best_score))
}

/// Splits `cube` using the best axis/cut combination available.
///
/// Returns two non-empty cubes on success, or `None` if this cube cannot be
/// split without creating an empty side.
fn split_cube(cube: Cube, moments: &Moments) -> Option<(Cube, Cube)> {
    let mut best_axis = None;
    let mut best_cut = 0usize;
    let mut best_score = f64::NEG_INFINITY;

    for axis in [Axis::Red, Axis::Green, Axis::Blue] {
        if let Some((cut, score)) = maximize(cube, axis, moments)
            && score > best_score
        {
            best_score = score;
            best_axis = Some(axis);
            best_cut = cut;
        }
    }

    let axis = best_axis?;
    let mut first = cube;
    let mut second = cube;

    match axis {
        Axis::Red => {
            first.r1 = best_cut;
            second.r0 = best_cut;
        }
        Axis::Green => {
            first.g1 = best_cut;
            second.g0 = best_cut;
        }
        Axis::Blue => {
            first.b1 = best_cut;
            second.b0 = best_cut;
        }
    }

    if volume_u64(first, &moments.weight) == 0 || volume_u64(second, &moments.weight) == 0 {
        return None;
    }

    Some((first, second))
}

/// Quantizes the RGBA pixel buffer down to at most `max_colors` colors.
///
/// Alpha is ignored during palette fitting. Output palette entries always use
/// `255` alpha because BMP paletted targets in this project are opaque.
///
/// Algorithm outline:
/// 1. Build summed histogram moments over quantized RGB space.
/// 2. Iteratively split the currently highest-variance cube.
/// 3. Convert final cubes into average-color palette entries.
/// 4. Map every source pixel to the cube/palette index of its RGB bin.
///
/// Returns `(palette, indices)` where `palette` has at most `max_colors`
/// entries (each `[R, G, B, 255]`) and `indices` has one entry per pixel.
///
/// # Errors
/// Returns [`QuantizeError::InvalidMaxColors`] if `max_colors` is outside
/// `2..=256`.
pub fn quantize(rgba: &[u8], max_colors: usize) -> Result<(Vec<[u8; 4]>, Vec<u8>), QuantizeError> {
    if !(2..=256).contains(&max_colors) {
        return Err(QuantizeError::InvalidMaxColors(max_colors));
    }

    let moments = build_moments(rgba);
    let mut cubes = vec![Cube::full()];
    let mut variances = vec![variance(Cube::full(), &moments)];

    while cubes.len() < max_colors {
        let split_idx = variances
            .iter()
            .enumerate()
            .filter(|(idx, var)| **var > 0.0 && cubes[*idx].can_split())
            .max_by(|(_, lhs), (_, rhs)| lhs.total_cmp(rhs))
            .map(|(idx, _)| idx);

        let Some(split_idx) = split_idx else {
            break;
        };

        let cube = cubes[split_idx];
        if let Some((first, second)) = split_cube(cube, &moments) {
            cubes[split_idx] = first;
            variances[split_idx] = variance(first, &moments);

            cubes.push(second);
            variances.push(variance(second, &moments));
        } else {
            variances[split_idx] = 0.0;
        }
    }

    let mut palette = Vec::with_capacity(cubes.len());
    for cube in &cubes {
        let weight = volume_u64(*cube, &moments.weight);
        if weight == 0 {
            palette.push([0, 0, 0, 255]);
            continue;
        }

        let red = (volume_u64(*cube, &moments.red) + (weight / 2)) / weight;
        let green = (volume_u64(*cube, &moments.green) + (weight / 2)) / weight;
        let blue = (volume_u64(*cube, &moments.blue) + (weight / 2)) / weight;

        #[expect(clippy::cast_possible_truncation)]
        let red = red as u8;
        #[expect(clippy::cast_possible_truncation)]
        let green = green as u8;
        #[expect(clippy::cast_possible_truncation)]
        let blue = blue as u8;

        palette.push([red, green, blue, 255]);
    }

    let mut tags = vec![0u8; HISTOGRAM_SIZE];
    for (palette_idx, cube) in cubes.iter().enumerate() {
        #[expect(clippy::cast_possible_truncation)]
        let palette_idx = palette_idx as u8;

        for r in (cube.r0 + 1)..=cube.r1 {
            for g in (cube.g0 + 1)..=cube.g1 {
                for b in (cube.b0 + 1)..=cube.b1 {
                    tags[histogram_index(r, g, b)] = palette_idx;
                }
            }
        }
    }

    let pixel_count = rgba.len() / 4;
    let mut indices = vec![0u8; pixel_count];
    for (pixel_idx, pixel) in rgba.chunks_exact(4).enumerate() {
        let r = usize::from(pixel[0] >> (8 - BITS_PER_CHANNEL)) + 1;
        let g = usize::from(pixel[1] >> (8 - BITS_PER_CHANNEL)) + 1;
        let b = usize::from(pixel[2] >> (8 - BITS_PER_CHANNEL)) + 1;

        indices[pixel_idx] = tags[histogram_index(r, g, b)];
    }

    Ok((palette, indices))
}

#[cfg(test)]
mod tests {
    use super::{QuantizeError, quantize};

    fn rgba_from_colors(colors: &[[u8; 3]], repeats: usize) -> Vec<u8> {
        let mut rgba = Vec::with_capacity(colors.len() * repeats * 4);
        for _ in 0..repeats {
            for [r, g, b] in colors {
                rgba.extend_from_slice(&[*r, *g, *b, 255]);
            }
        }
        rgba
    }

    fn reconstruct_rgba(palette: &[[u8; 4]], indices: &[u8]) -> Vec<u8> {
        let mut rgba = Vec::with_capacity(indices.len() * 4);
        for &idx in indices {
            rgba.extend_from_slice(&palette[usize::from(idx)]);
        }
        rgba
    }

    #[test]
    fn quantize_is_lossless_when_distinct_colors_fit_palette() {
        let colors = [
            [0, 0, 0],
            [32, 0, 0],
            [0, 32, 0],
            [0, 0, 32],
            [64, 64, 0],
            [0, 64, 64],
            [96, 32, 16],
            [128, 96, 32],
        ];
        let rgba = rgba_from_colors(&colors, 3);

        let (palette, indices) = quantize(&rgba, colors.len()).expect("palette size should be valid");
        let reconstructed = reconstruct_rgba(&palette, &indices);

        assert_eq!(reconstructed, rgba);
    }

    #[test]
    fn quantize_is_lossless_when_palette_limit_exceeds_distinct_colors() {
        let colors = [[16, 32, 48], [48, 80, 112], [160, 32, 96], [224, 192, 32]];
        let rgba = rgba_from_colors(&colors, 4);

        let (palette, indices) = quantize(&rgba, 256).expect("palette size should be valid");
        let reconstructed = reconstruct_rgba(&palette, &indices);

        assert_eq!(reconstructed, rgba);
    }

    #[test]
    fn quantize_outputs_valid_palette_and_indices() {
        let colors = [
            [0, 0, 0],
            [16, 0, 0],
            [32, 0, 0],
            [48, 0, 0],
            [64, 0, 0],
            [80, 0, 0],
            [96, 0, 0],
            [112, 0, 0],
            [0, 16, 0],
            [0, 32, 0],
            [0, 48, 0],
            [0, 64, 0],
            [0, 80, 0],
            [0, 96, 0],
            [0, 112, 0],
            [0, 0, 16],
            [0, 0, 32],
            [0, 0, 48],
            [0, 0, 64],
            [0, 0, 80],
        ];
        let rgba = rgba_from_colors(&colors, 2);

        let (palette, indices) = quantize(&rgba, 8).expect("palette size should be valid");

        assert!(!palette.is_empty());
        assert!(palette.len() <= 8);
        assert_eq!(indices.len(), rgba.len() / 4);
        assert!(indices.iter().all(|&idx| usize::from(idx) < palette.len()));
    }

    #[test]
    fn quantize_is_deterministic_for_same_input() {
        let colors = [[8, 16, 24], [24, 40, 56], [40, 72, 104], [88, 120, 152], [200, 24, 88]];
        let rgba = rgba_from_colors(&colors, 5);

        let first = quantize(&rgba, 4).expect("palette size should be valid");
        let second = quantize(&rgba, 4).expect("palette size should be valid");

        assert_eq!(first, second);
    }

    #[test]
    fn quantize_rejects_invalid_palette_sizes() {
        let rgba = rgba_from_colors(&[[0, 0, 0], [255, 255, 255]], 1);

        assert_eq!(quantize(&rgba, 1), Err(QuantizeError::InvalidMaxColors(1)));
        assert_eq!(quantize(&rgba, 257), Err(QuantizeError::InvalidMaxColors(257)));
    }
}
