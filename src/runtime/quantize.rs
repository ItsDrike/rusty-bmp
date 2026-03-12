/// Median-cut color quantization.
///
/// Reduces an RGBA image to at most `max_colors` representative palette
/// entries and returns both the palette and a per-pixel index buffer.
use rayon::prelude::*;

/// A single color bucket used during median-cut partitioning.
struct Bucket {
    /// Indices into the original pixel array (stepping by 4).
    pixel_indices: Vec<usize>,
}

#[derive(Clone, Copy)]
enum ColorAxis {
    Red,
    Green,
    Blue,
}

impl Bucket {
    /// Returns the color channel with the greatest range in this bucket.
    fn widest_axis(&self, rgba: &[u8]) -> ColorAxis {
        let (mut min_r, mut min_g, mut min_b) = (255u8, 255u8, 255u8);
        let (mut max_r, mut max_g, mut max_b) = (0u8, 0u8, 0u8);
        for &i in &self.pixel_indices {
            let r = rgba[i];
            let g = rgba[i + 1];
            let b = rgba[i + 2];
            min_r = min_r.min(r);
            max_r = max_r.max(r);
            min_g = min_g.min(g);
            max_g = max_g.max(g);
            min_b = min_b.min(b);
            max_b = max_b.max(b);
        }
        let range_r = max_r - min_r;
        let range_g = max_g - min_g;
        let range_b = max_b - min_b;
        if range_r >= range_g && range_r >= range_b {
            ColorAxis::Red
        } else if range_g >= range_b {
            ColorAxis::Green
        } else {
            ColorAxis::Blue
        }
    }

    /// Splits this bucket along the median of the widest axis, returning two
    /// new buckets.
    fn split(mut self, rgba: &[u8]) -> (Bucket, Bucket) {
        let axis = self.widest_axis(rgba);
        self.pixel_indices.sort_unstable_by_key(|&i| match axis {
            ColorAxis::Red => rgba[i],
            ColorAxis::Green => rgba[i + 1],
            ColorAxis::Blue => rgba[i + 2],
        });
        let mid = self.pixel_indices.len() / 2;
        let right = self.pixel_indices.split_off(mid);
        (
            Bucket {
                pixel_indices: self.pixel_indices,
            },
            Bucket { pixel_indices: right },
        )
    }

    /// Computes the average color of all pixels in this bucket.
    fn average_color(&self, rgba: &[u8]) -> [u8; 4] {
        if self.pixel_indices.is_empty() {
            return [0, 0, 0, 255];
        }
        let (mut sum_r, mut sum_g, mut sum_b) = (0u64, 0u64, 0u64);
        for &i in &self.pixel_indices {
            sum_r += rgba[i] as u64;
            sum_g += rgba[i + 1] as u64;
            sum_b += rgba[i + 2] as u64;
        }
        let n = self.pixel_indices.len() as u64;
        [(sum_r / n) as u8, (sum_g / n) as u8, (sum_b / n) as u8, 255]
    }
}

/// Finds the nearest palette entry to a given RGB color, returning the index.
///
/// Uses the Squared Euclidean Distance in RGB color space to find the closest
/// color match.
fn nearest_palette_index(palette: &[[u8; 4]], r: u8, g: u8, b: u8) -> usize {
    let mut best_idx = 0;
    let mut best_dist = u32::MAX;
    for (i, entry) in palette.iter().enumerate() {
        let dr = r as i32 - entry[0] as i32;
        let dg = g as i32 - entry[1] as i32;
        let db = b as i32 - entry[2] as i32;
        // Each difference is in -255..=255, so squared fits comfortably in i32
        // (max 65025) and the sum (max 195075) fits in u32.
        let dist = (dr * dr + dg * dg + db * db) as u32;
        if dist < best_dist {
            best_dist = dist;
            best_idx = i;
            if dist == 0 {
                break;
            }
        }
    }
    best_idx
}

/// Tries to build an exact palette from the image's unique RGB colors.
///
/// If the image has at most `max_colors` distinct RGB values, returns
/// `Some((palette, indices))` with no color loss.  Otherwise returns `None`
/// so the caller can fall back to median-cut.
fn try_exact_palette(rgba: &[u8], max_colors: usize) -> Option<(Vec<[u8; 4]>, Vec<u8>)> {
    use std::collections::HashMap;

    let pixel_count = rgba.len() / 4;
    // Map (R, G, B) -> palette index.  We stop as soon as we exceed
    // max_colors unique values.
    let mut color_map: HashMap<(u8, u8, u8), u8> = HashMap::new();
    let mut palette: Vec<[u8; 4]> = Vec::new();
    let mut indices: Vec<u8> = Vec::with_capacity(pixel_count);

    for i in 0..pixel_count {
        let off = i * 4;
        let key = (rgba[off], rgba[off + 1], rgba[off + 2]);
        let idx = match color_map.get(&key) {
            Some(&idx) => idx,
            None => {
                if palette.len() >= max_colors {
                    return None; // Too many unique colors.
                }
                let idx = palette.len() as u8;
                palette.push([key.0, key.1, key.2, 255]);
                color_map.insert(key, idx);
                idx
            }
        };
        indices.push(idx);
    }

    Some((palette, indices))
}

/// Quantizes the RGBA pixel buffer down to at most `max_colors` colors.
///
/// If the image already contains `max_colors` or fewer distinct RGB values the
/// exact colors are preserved without any averaging. Otherwise the median-cut
/// algorithm is used to approximate the palette.
///
/// Returns `(palette, indices)` where `palette` has at most `max_colors`
/// entries (each `[R, G, B, 255]`) and `indices` has one entry per pixel.
pub fn quantize(rgba: &[u8], max_colors: usize) -> (Vec<[u8; 4]>, Vec<u8>) {
    assert!((2..=256).contains(&max_colors));

    // Fast path: if the image already fits in the target palette size,
    // preserve the exact colors — no averaging, no extra entries.
    if let Some(exact) = try_exact_palette(rgba, max_colors) {
        return exact;
    }

    let pixel_count = rgba.len() / 4;

    // Initial bucket with all pixels.
    let initial = Bucket {
        pixel_indices: (0..pixel_count).map(|i| i * 4).collect(),
    };
    let mut buckets = vec![initial];

    // Repeatedly split the largest bucket until we have enough.
    while buckets.len() < max_colors {
        // Find the bucket with the most pixels to split.
        let (split_idx, _) = buckets
            .iter()
            .enumerate()
            .filter(|(_, b)| b.pixel_indices.len() >= 2)
            .max_by_key(|(_, b)| b.pixel_indices.len())
            .unwrap_or((0, &buckets[0]));

        if buckets[split_idx].pixel_indices.len() < 2 {
            break; // Can't split single-pixel buckets.
        }

        let bucket = buckets.swap_remove(split_idx);
        let (a, b) = bucket.split(rgba);
        buckets.push(a);
        buckets.push(b);
    }

    let palette: Vec<[u8; 4]> = buckets.iter().map(|b| b.average_color(rgba)).collect();

    // Map each pixel to the nearest palette entry (parallelized — this is the
    // most expensive step, ~256 distance computations per pixel).
    let indices: Vec<u8> = (0..pixel_count)
        .into_par_iter()
        .map(|i| {
            let off = i * 4;
            nearest_palette_index(&palette, rgba[off], rgba[off + 1], rgba[off + 2]) as u8
        })
        .collect();

    (palette, indices)
}
