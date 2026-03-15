//! Transformation pipeline with replay optimization.
//!
//! This module implements a transformation history system that stores a
//! sequence of [`ImageTransform`] operations and can efficiently replay
//! them to produce a final image.
//!
//! To avoid recomputing the entire transform chain every time, the pipeline
//! periodically stores **checkpoints** containing intermediate images.
//! Replaying then resumes from the nearest checkpoint instead of the
//! original image.
use std::collections::BTreeMap;

use crate::runtime::decode::DecodedImage;
use crate::runtime::steganography;

use super::dispatch::apply_transform;
use super::model::ImageTransform;

/// Minimum accumulated replay cost required before a checkpoint is created.
///
/// The pipeline tracks the computational cost of transformations since the
/// last checkpoint. Once the accumulated cost reaches this threshold,
/// a new checkpoint may be stored.
const CHECKPOINT_COST_THRESHOLD: u32 = 15;

/// Maximum number of stored checkpoints.
///
/// When this limit is exceeded, the oldest checkpoint is removed.
const MAX_CHECKPOINTS: usize = 5;

/// A sequence of image transformations with replay optimization.
///
/// `TransformPipeline` stores a list of [`ImageTransform`] operations that
/// can be applied to an image in order. It also maintains intermediate
/// **checkpoints** to accelerate recomputation when the pipeline changes.
///
/// # Checkpointing strategy
///
/// Each transform reports a heuristic replay cost via
/// [`ImageTransform::replay_cost`]. When the accumulated cost since the
/// last checkpoint exceeds [`CHECKPOINT_COST_THRESHOLD`], the pipeline
/// stores the current image state.
///
/// During replay, the pipeline resumes from the **nearest preceding
/// checkpoint**, reducing the number of transformations that must be
/// recomputed.
///
/// To limit memory usage, only the most recent [`MAX_CHECKPOINTS`] are kept.
///
/// # Typical usage
///
/// ```ignore
/// let mut pipeline = TransformPipeline::default();
///
/// pipeline.push(ImageTransform::Grayscale, Some(&current_image));
/// pipeline.push(ImageTransform::RotateLeft90, Some(&current_image));
///
/// let result = pipeline.apply(&original_image);
/// ```
///
/// The pipeline itself does not own the original image; it only stores
/// transformation steps and optional checkpoints.
#[derive(Debug, Default, Clone)]
pub struct TransformPipeline {
    ops: Vec<ImageTransform>,
    checkpoints: BTreeMap<usize, DecodedImage>,
    cost_since_checkpoint: u32,
}

impl TransformPipeline {
    /// Appends a new transformation to the pipeline.
    ///
    /// If the accumulated replay cost exceeds the checkpoint threshold,
    /// the current image may be stored as a checkpoint to accelerate
    /// future replays.
    ///
    /// # Parameters
    ///
    /// - `op` - transformation to append
    /// - `current_image` - image state after applying all previous
    ///   transformations (used to create a checkpoint if needed)
    pub fn push(&mut self, op: ImageTransform, current_image: Option<&DecodedImage>) {
        let cost = op.replay_cost();
        self.cost_since_checkpoint += cost;

        if cost > 0 && self.cost_since_checkpoint >= CHECKPOINT_COST_THRESHOLD && !self.ops.is_empty() {
            if let Some(img) = current_image {
                let checkpoint_idx = self.ops.len() - 1;
                self.checkpoints.insert(checkpoint_idx, img.clone());

                while self.checkpoints.len() > MAX_CHECKPOINTS {
                    if let Some(oldest) = self.checkpoints.keys().next().copied() {
                        self.checkpoints.remove(&oldest);
                    }
                }
            }
            self.cost_since_checkpoint = 0;
        }

        self.ops.push(op);
    }

    /// Removes all transformations and checkpoints.
    ///
    /// After calling this method the pipeline becomes empty.
    pub fn clear(&mut self) {
        self.ops.clear();
        self.checkpoints.clear();
        self.cost_since_checkpoint = 0;
    }

    /// Returns the list of transformations in the pipeline.
    ///
    /// The returned slice preserves the execution order.
    #[must_use]
    pub fn ops(&self) -> &[ImageTransform] {
        &self.ops
    }

    /// Returns `true` if the pipeline contains no transformations.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Returns the number of transformations stored in the pipeline.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.ops.len()
    }

    /// Removes the transformation at the specified index.
    ///
    /// Any checkpoints that occur at or after the removed transform
    /// are discarded because they may no longer be valid.
    pub fn remove(&mut self, index: usize) {
        self.ops.remove(index);
        self.checkpoints.retain(|&k, _| k < index);
        self.recompute_cost_since_checkpoint();
    }

    /// Removes and returns the most recently added transformation.
    ///
    /// Any checkpoint associated with that transform is also removed.
    pub fn pop(&mut self) -> Option<ImageTransform> {
        let op = self.ops.pop()?;
        let len = self.ops.len();
        self.checkpoints.remove(&len);
        self.recompute_cost_since_checkpoint();
        Some(op)
    }

    /// Applies the entire pipeline to an image.
    ///
    /// Replay starts from the nearest available checkpoint to minimize
    /// recomputation.
    ///
    /// # Parameters
    ///
    /// - `original` - the source image before any transformations
    ///
    /// # Returns
    ///
    /// The fully transformed image.
    #[must_use]
    pub fn apply(&self, original: &DecodedImage) -> DecodedImage {
        self.apply_range(original, self.ops.len())
    }

    /// Applies the pipeline and returns any replay warnings.
    ///
    /// Certain operations (such as steganography embedding) may fail
    /// during replay if their original preconditions are no longer
    /// satisfied. In such cases the step is skipped and a warning
    /// message is recorded.
    ///
    /// # Returns
    ///
    /// A tuple containing:
    ///
    /// - the resulting image
    /// - a list of warnings generated during replay
    #[must_use]
    pub fn apply_with_warnings(&self, original: &DecodedImage) -> (DecodedImage, Vec<String>) {
        self.apply_range_with_warnings(original, self.ops.len())
    }

    /// Applies the first `count` transformations in the pipeline.
    fn apply_range(&self, original: &DecodedImage, count: usize) -> DecodedImage {
        self.apply_range_with_warnings(original, count).0
    }

    /// Applies transformations while collecting replay warnings.
    fn apply_range_with_warnings(&self, original: &DecodedImage, count: usize) -> (DecodedImage, Vec<String>) {
        let (start_idx, mut out) = self
            .checkpoints
            .range(..count)
            .next_back()
            .map_or_else(|| (0, original.clone()), |(&idx, img)| (idx + 1, img.clone()));

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
                _ => out = apply_transform(&out, op),
            }
        }
        (out, warnings)
    }

    /// Recomputes the accumulated replay cost since the most recent checkpoint.
    fn recompute_cost_since_checkpoint(&mut self) {
        let last_cp = self.checkpoints.keys().next_back().copied();
        let start = last_cp.map_or(0, |i| i + 1);
        self.cost_since_checkpoint = self.ops[start..].iter().map(ImageTransform::replay_cost).sum();
    }
}

#[cfg(test)]
mod tests {
    use super::{TransformPipeline, CHECKPOINT_COST_THRESHOLD, MAX_CHECKPOINTS};
    use crate::runtime::decode::DecodedImage;
    use crate::runtime::transform::dispatch::apply_transform;
    use crate::runtime::transform::geometry::{RotationInterpolation, TranslateMode};
    use crate::runtime::transform::model::ImageTransform;

    fn test_image() -> DecodedImage {
        DecodedImage {
            width: 2,
            height: 2,
            rgba: vec![10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255],
        }
    }

    #[test]
    fn no_checkpoint_below_threshold() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        for _ in 0..5 {
            let cur = pipeline.apply(&img);
            pipeline.push(ImageTransform::Brightness(1), Some(&cur));
        }
        assert!(pipeline.checkpoints.is_empty());
    }

    #[test]
    fn checkpoint_created_at_threshold() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        let mut cur = img;
        for _ in 0..=CHECKPOINT_COST_THRESHOLD {
            pipeline.push(ImageTransform::Brightness(1), Some(&cur));
            cur = apply_transform(&cur, &ImageTransform::Brightness(1));
        }
        assert!(!pipeline.checkpoints.is_empty());
    }

    #[test]
    fn max_checkpoints_enforced() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        let mut cur = img.clone();
        let batches = MAX_CHECKPOINTS + 3;

        for _ in 0..batches {
            for _ in 0..=CHECKPOINT_COST_THRESHOLD {
                pipeline.push(ImageTransform::Brightness(1), Some(&cur));
                cur = apply_transform(&cur, &ImageTransform::Brightness(1));
            }
        }

        assert!(pipeline.checkpoints.len() <= MAX_CHECKPOINTS);
        let result = pipeline.apply(&img);
        assert_eq!(result.rgba, cur.rgba);
    }

    #[test]
    fn replay_cost_variants_stay_reasonable() {
        assert_eq!(ImageTransform::Grayscale.replay_cost(), 1);
        assert_eq!(
            ImageTransform::Translate {
                dx: 1,
                dy: -1,
                mode: TranslateMode::Crop,
                fill: [0, 0, 0, 0],
            }
            .replay_cost(),
            2
        );
        assert!(
            ImageTransform::RotateAny {
                angle_tenths: 123,
                interpolation: RotationInterpolation::Bicubic,
                expand: true,
            }
            .replay_cost()
                > 1
        );
    }
}
