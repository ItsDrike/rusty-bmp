/// Median-cut color quantization.
///
/// Reduces an RGBA image to at most `max_colors` representative palette
/// entries and returns both the palette and a per-pixel index buffer.
/// A single color bucket used during median-cut partitioning.
struct Bucket {
    /// Indices into the original pixel array (stepping by 4).
    pixel_indices: Vec<usize>,
    /// If true, this bucket cannot be split further.
    terminal: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ColorAxis {
    Red,
    Green,
    Blue,
}

impl Bucket {
    #[inline]
    fn channel_value(rgba: &[u8], i: usize, axis: ColorAxis) -> u8 {
        match axis {
            ColorAxis::Red => rgba[i],
            ColorAxis::Green => rgba[i + 1],
            ColorAxis::Blue => rgba[i + 2],
        }
    }

    /// Returns per-channel (R, G, B) ranges for all pixels in this bucket.
    fn channel_ranges(&self, rgba: &[u8]) -> (u8, u8, u8) {
        if self.pixel_indices.is_empty() {
            return (0, 0, 0);
        }

        let (mut min_r, mut min_g, mut min_b) = (255u8, 255u8, 255u8);
        let (mut max_r, mut max_g, mut max_b) = (0u8, 0u8, 0u8);
        for &i in &self.pixel_indices {
            let r = Self::channel_value(rgba, i, ColorAxis::Red);
            let g = Self::channel_value(rgba, i, ColorAxis::Green);
            let b = Self::channel_value(rgba, i, ColorAxis::Blue);
            min_r = min_r.min(r);
            max_r = max_r.max(r);
            min_g = min_g.min(g);
            max_g = max_g.max(g);
            min_b = min_b.min(b);
            max_b = max_b.max(b);
        }

        (max_r - min_r, max_g - min_g, max_b - min_b)
    }

    /// Returns channels ordered from widest range to narrowest range.
    fn widest_axes(&self, rgba: &[u8]) -> [ColorAxis; 3] {
        let (range_r, range_g, range_b) = self.channel_ranges(rgba);

        let mut axes = [
            (ColorAxis::Red, range_r),
            (ColorAxis::Green, range_g),
            (ColorAxis::Blue, range_b),
        ];

        axes.sort_unstable_by(|(axis_a, range_a), (axis_b, range_b)| {
            range_b.cmp(range_a).then_with(|| axis_a.cmp(axis_b))
        });

        [axes[0].0, axes[1].0, axes[2].0]
    }

    /// Finds a split boundary nearest to the bucket median for `axis`.
    ///
    /// Assumes `pixel_indices` are already sorted by the selected axis value.
    /// The returned index `k` satisfies `0 < k < len` and guarantees that
    /// adjacent elements across the boundary differ in axis value, avoiding
    /// splits that keep identical colors on both sides.
    fn find_split_index(&self, rgba: &[u8], axis: ColorAxis) -> Option<usize> {
        let len = self.pixel_indices.len();
        if len < 2 {
            return None;
        }

        let mid = len / 2;

        // Find the first distinct color in the first half of the bucket pixels, mid -> start
        let mut left_candidate = None;
        for k in (1..=mid).rev() {
            let left = Self::channel_value(rgba, self.pixel_indices[k - 1], axis);
            let right = Self::channel_value(rgba, self.pixel_indices[k], axis);
            if left != right {
                left_candidate = Some(k);
                break;
            }
        }

        // Find the first distinct color in the second half of the bucket pixels, mid -> end
        let mut right_candidate = None;
        for k in (mid + 1)..len {
            let left = Self::channel_value(rgba, self.pixel_indices[k - 1], axis);
            let right = Self::channel_value(rgba, self.pixel_indices[k], axis);
            if left != right {
                right_candidate = Some(k);
                break;
            }
        }

        match (left_candidate, right_candidate) {
            // Pick the  candidate closer to mid (for balanced partitions)
            (Some(l), Some(r)) => {
                if mid - l <= r - mid {
                    Some(l)
                } else {
                    Some(r)
                }
            }
            // Pick the single found candidate
            (Some(l), None) => Some(l),
            (None, Some(r)) => Some(r),
            // All axis values are identical in the bucket
            (None, None) => None,
        }
    }

    /// Attempts to split this bucket along the median of the widest axis.
    ///
    /// Returns the (possibly unchanged) left bucket and an optional right
    /// bucket when a split point exists.
    fn split(mut self, rgba: &[u8]) -> (Self, Option<Self>) {
        let axes = self.widest_axes(rgba);

        for axis in axes {
            self.pixel_indices
                .sort_unstable_by_key(|&i| Self::channel_value(rgba, i, axis));
            if let Some(split_at) = self.find_split_index(rgba, axis) {
                let right = self.pixel_indices.split_off(split_at);
                return (
                    self,
                    Some(Self {
                        pixel_indices: right,
                        terminal: false,
                    }),
                );
            }
        }

        (self, None)
    }

    /// Computes the average color of all pixels in this bucket.
    fn average_color(&self, rgba: &[u8]) -> [u8; 4] {
        if self.pixel_indices.is_empty() {
            return [0, 0, 0, 255];
        }

        let (mut sum_r, mut sum_g, mut sum_b) = (0u64, 0u64, 0u64);
        for &i in &self.pixel_indices {
            debug_assert!(i + 3 < rgba.len());
            sum_r += u64::from(rgba[i]);
            sum_g += u64::from(rgba[i + 1]);
            sum_b += u64::from(rgba[i + 2]);
        }
        let n = self.pixel_indices.len() as u64;

        // Safe: We have a sum of N u8 values, divided by N -> safe u8
        // (This will floor - int division)
        #[allow(clippy::cast_possible_truncation)]
        [(sum_r / n) as u8, (sum_g / n) as u8, (sum_b / n) as u8, 255]
    }
}

/// Quantizes the RGBA pixel buffer down to at most `max_colors` colors.
///
/// The splitter avoids splitting pure buckets and only splits where channel
/// values change, which keeps low-color images lossless when they fit within
/// `max_colors` while still using median-cut behavior for complex images.
///
/// Returns `(palette, indices)` where `palette` has at most `max_colors`
/// entries (each `[R, G, B, 255]`) and `indices` has one entry per pixel.
///
/// # Panics
/// Panics if `max_colors` is not in `2..=256`.
#[must_use]
pub fn quantize(rgba: &[u8], max_colors: usize) -> (Vec<[u8; 4]>, Vec<u8>) {
    assert!((2..=256).contains(&max_colors));

    let pixel_count = rgba.len() / 4;

    // Initial bucket with all pixels.
    let initial = Bucket {
        pixel_indices: (0..pixel_count).map(|i| i * 4).collect(),
        terminal: false,
    };
    let mut buckets = vec![initial];

    // Repeatedly split the largest bucket until we have enough.
    while buckets.len() < max_colors {
        // Find the largest bucket that also has the most color-distinctiveness
        // and can still be split.
        let split_idx = buckets
            .iter()
            .enumerate()
            .filter(|(_, b)| !b.terminal && b.pixel_indices.len() >= 2)
            .max_by_key(|(_, b)| {
                let (r_range, g_range, b_range) = b.channel_ranges(rgba);
                let volume = usize::from(r_range) * usize::from(g_range) * usize::from(b_range);
                volume * b.pixel_indices.len()
            })
            .map(|(idx, _)| idx);

        let Some(split_idx) = split_idx else {
            break;
        };

        let bucket = buckets.swap_remove(split_idx);
        let (mut left, right) = bucket.split(rgba);
        if let Some(right) = right {
            buckets.push(left);
            buckets.push(right);
        } else {
            left.terminal = true;
            buckets.push(left);
        }
    }

    let mut palette = Vec::with_capacity(buckets.len());
    let mut indices = vec![0u8; pixel_count];

    for (palette_idx, bucket) in buckets.iter().enumerate() {
        // Safe: the palette index is the bucket position, and we guarantee that
        // have at most 256 buckets (in the max_colors assert) -> fits in u8
        #[allow(clippy::cast_possible_truncation)]
        let palette_idx = palette_idx as u8;

        palette.push(bucket.average_color(rgba));
        for &off in &bucket.pixel_indices {
            indices[off / 4] = palette_idx;
        }
    }

    (palette, indices)
}
