use std::collections::BTreeMap;

use thiserror::Error;

use crate::runtime::decode::DecodedImage;

use super::model::{ImageTransform, TransformError};

const CHECKPOINT_COST_THRESHOLD: u32 = 15;
const MAX_CHECKPOINTS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Error)]
pub enum PipelineError {
    #[error("transform index {index} out of bounds for pipeline length {len}")]
    IndexOutOfBounds { index: usize, len: usize },
}

#[derive(Debug, Default, Clone)]
pub struct TransformPipeline {
    ops: Vec<ImageTransform>,
    checkpoints: BTreeMap<usize, DecodedImage>,
    cost_since_checkpoint: u32,
}

impl TransformPipeline {
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

    pub fn clear(&mut self) {
        self.ops.clear();
        self.checkpoints.clear();
        self.cost_since_checkpoint = 0;
    }

    #[must_use]
    pub fn ops(&self) -> &[ImageTransform] {
        &self.ops
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.ops.len()
    }

    /// Removes the transform at `index`.
    ///
    /// # Errors
    /// Returns [`PipelineError::IndexOutOfBounds`] if `index` does not point to
    /// an existing transform.
    pub fn remove(&mut self, index: usize) -> Result<ImageTransform, PipelineError> {
        if index >= self.ops.len() {
            return Err(PipelineError::IndexOutOfBounds {
                index,
                len: self.ops.len(),
            });
        }

        let removed = self.ops.remove(index);
        self.checkpoints.retain(|&k, _| k < index);
        self.recompute_cost_since_checkpoint();
        Ok(removed)
    }

    pub fn pop(&mut self) -> Option<ImageTransform> {
        let op = self.ops.pop()?;
        let len = self.ops.len();
        self.checkpoints.remove(&len);
        self.recompute_cost_since_checkpoint();
        Some(op)
    }

    /// Applies all operations in the pipeline.
    ///
    /// # Errors
    /// Returns [`TransformError`] if any operation fails while replaying.
    pub fn apply(&self, original: &DecodedImage) -> Result<DecodedImage, TransformError> {
        self.apply_range(original, self.ops.len())
    }

    #[must_use]
    pub fn apply_with_warnings(&self, original: &DecodedImage) -> (DecodedImage, Vec<String>) {
        self.apply_range_with_warnings(original, self.ops.len())
    }

    fn apply_range(&self, original: &DecodedImage, count: usize) -> Result<DecodedImage, TransformError> {
        let (start_idx, mut out) = self
            .checkpoints
            .range(..count)
            .next_back()
            .map_or_else(|| (0, original.clone()), |(&idx, img)| (idx + 1, img.clone()));

        for op in &self.ops[start_idx..count] {
            out = op.apply(&out)?;
        }

        Ok(out)
    }

    fn apply_range_with_warnings(&self, original: &DecodedImage, count: usize) -> (DecodedImage, Vec<String>) {
        let (start_idx, mut out) = self
            .checkpoints
            .range(..count)
            .next_back()
            .map_or_else(|| (0, original.clone()), |(&idx, img)| (idx + 1, img.clone()));

        let mut warnings = Vec::new();
        for op in &self.ops[start_idx..count] {
            match op.apply(&out) {
                Ok(next) => out = next,
                Err(err) => warnings.push(format!("Skipped {op} during replay: {err}")),
            }
        }
        (out, warnings)
    }

    fn recompute_cost_since_checkpoint(&mut self) {
        let last_cp = self.checkpoints.keys().next_back().copied();
        let start = last_cp.map_or(0, |i| i + 1);
        self.cost_since_checkpoint = self.ops[start..].iter().map(ImageTransform::replay_cost).sum();
    }
}

#[cfg(test)]
mod tests {
    use super::{CHECKPOINT_COST_THRESHOLD, MAX_CHECKPOINTS, PipelineError, TransformPipeline};
    use crate::runtime::decode::DecodedImage;
    use crate::runtime::transform::{
        Brightness, Grayscale, RotateAny, RotationInterpolation, TransformOp, Translate, TranslateMode,
    };

    fn test_image() -> DecodedImage {
        DecodedImage::new(
            2,
            2,
            vec![10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255],
        )
        .expect("valid test image")
    }

    #[test]
    fn no_checkpoint_below_threshold() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        for _ in 0..5 {
            let cur = pipeline.apply(&img).unwrap_or_else(|_| img.clone());
            pipeline.push(Brightness { delta: 1 }.into(), Some(&cur));
        }
        assert!(pipeline.checkpoints.is_empty());
    }

    #[test]
    fn checkpoint_created_at_threshold() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        let mut cur = img;
        for _ in 0..=CHECKPOINT_COST_THRESHOLD {
            pipeline.push(Brightness { delta: 1 }.into(), Some(&cur));
            cur = Brightness { delta: 1 }.apply(&cur).expect("brightness should succeed");
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
                pipeline.push(Brightness { delta: 1 }.into(), Some(&cur));
                cur = Brightness { delta: 1 }.apply(&cur).expect("brightness should succeed");
            }
        }

        assert!(pipeline.checkpoints.len() <= MAX_CHECKPOINTS);
        let result = pipeline.apply(&img).expect("pipeline apply should succeed");
        assert_eq!(result.rgba(), cur.rgba());
    }

    #[test]
    fn replay_cost_variants_stay_reasonable() {
        assert_eq!(Grayscale.replay_cost(), 1);
        assert_eq!(
            Translate {
                dx: 1,
                dy: -1,
                mode: TranslateMode::Crop,
                fill: [0, 0, 0, 0],
            }
            .replay_cost(),
            2
        );
        assert!(
            RotateAny {
                angle_tenths: 123,
                interpolation: RotationInterpolation::Bicubic,
                expand: true,
            }
            .replay_cost()
                > 1
        );
    }

    #[test]
    fn remove_returns_error_for_out_of_bounds_index() {
        let mut pipeline = TransformPipeline::default();
        pipeline.push(Brightness { delta: 1 }.into(), None);

        let err = pipeline
            .remove(1)
            .expect_err("removing outside the pipeline length should fail");
        assert_eq!(err, PipelineError::IndexOutOfBounds { index: 1, len: 1 });
        assert_eq!(pipeline.len(), 1);
    }

    #[test]
    fn remove_returns_removed_transform_when_index_is_valid() {
        let mut pipeline = TransformPipeline::default();
        let first: super::ImageTransform = Brightness { delta: 1 }.into();
        let second: super::ImageTransform = Grayscale.into();
        pipeline.push(first.clone(), None);
        pipeline.push(second.clone(), None);

        let removed = pipeline.remove(0).expect("removing a valid index should succeed");

        assert_eq!(removed, first);
        assert_eq!(pipeline.ops(), &[second]);
    }
}
