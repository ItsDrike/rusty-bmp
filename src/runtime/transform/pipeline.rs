//! Stateless transform pipeline model.
//!
//! [`TransformPipeline`] is intentionally lightweight: it stores an ordered list
//! of [`ImageTransform`] operations and provides replay helpers.
//!
//! If you want cached replay for interactive editing workflows, use
//! [`crate::runtime::transform::TransformPipelineExecutor`].

use thiserror::Error;

use crate::runtime::decode::DecodedImage;

use super::model::{ImageTransform, TransformError};

/// Errors from editing the transform list itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Error)]
pub enum PipelineError {
    /// The requested index is outside the current operation list.
    ///
    /// Fields:
    /// - `index`: requested transform index
    /// - `len`: current pipeline length
    #[error("transform index {index} out of bounds for pipeline length {len}")]
    IndexOutOfBounds { index: usize, len: usize },
}

/// Strict replay failure details.
#[derive(Debug, Error)]
#[error("transform at index {index} failed: {error}")]
pub struct ReplayError {
    /// Index of the transform that failed.
    index: usize,

    /// Operation-specific replay error.
    #[source]
    error: TransformError,
}

impl ReplayError {
    pub(crate) const fn new(index: usize, error: TransformError) -> Self {
        Self { index, error }
    }

    /// Returns the operation index that caused replay to fail.
    #[must_use]
    pub const fn index(&self) -> usize {
        self.index
    }

    /// Returns the underlying operation error.
    #[must_use]
    pub const fn error(&self) -> &TransformError {
        &self.error
    }
}

/// Description of one transform skipped during best-effort replay.
#[derive(Debug)]
pub struct ReplaySkip {
    /// Index of the skipped transform.
    pub index: usize,
    /// Error returned by the failed transform.
    pub error: TransformError,
}

/// Result of best-effort replay.
#[derive(Debug)]
pub struct ReplayReport {
    /// Final image after applying all successful transforms.
    pub image: DecodedImage,
    /// Failed transforms that were skipped in order.
    pub skips: Vec<ReplaySkip>,
}

/// Ordered list of image transforms.
///
/// This type is pure model state. Replaying it does **not** mutate internal
/// caches, and all methods are deterministic for the same source image and op
/// list.
#[derive(Debug, Default, Clone)]
pub struct TransformPipeline {
    ops: Vec<ImageTransform>,
}

impl TransformPipeline {
    /// Appends a transform to the end of the pipeline.
    pub fn push(&mut self, op: ImageTransform) {
        self.ops.push(op);
    }

    /// Removes all transforms from the pipeline.
    pub fn clear(&mut self) {
        self.ops.clear();
    }

    /// Returns transforms in replay order.
    #[must_use]
    pub fn ops(&self) -> &[ImageTransform] {
        &self.ops
    }

    /// Returns `true` when there are no transforms.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Returns the number of stored transforms.
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

        Ok(self.ops.remove(index))
    }

    /// Removes and returns the last transform, if any.
    pub fn pop(&mut self) -> Option<ImageTransform> {
        self.ops.pop()
    }

    /// Replays all transforms and fails on the first error.
    ///
    /// # Errors
    /// Returns [`ReplayError`] with the failing operation index.
    pub fn replay_strict(&self, original: &DecodedImage) -> Result<DecodedImage, ReplayError> {
        self.replay_range_strict(original, 0, self.ops.len())
    }

    /// Replays all transforms, skipping failed operations.
    ///
    /// The output image always includes every successful transform in order.
    /// Failed transforms are recorded in [`ReplayReport::skips`].
    #[must_use]
    pub fn replay_best_effort(&self, original: &DecodedImage) -> ReplayReport {
        self.replay_range_best_effort(original, 0, self.ops.len())
    }

    /// Replays a contiguous operation range in strict mode.
    ///
    /// `start_image` is treated as the image state immediately before
    /// `start_idx`, and operations in `start_idx..count` are applied in order.
    /// The first transform error aborts replay.
    fn replay_range_strict(
        &self,
        start_image: &DecodedImage,
        start_idx: usize,
        count: usize,
    ) -> Result<DecodedImage, ReplayError> {
        let mut out: Option<DecodedImage> = None;
        for (offset, op) in self.ops[start_idx..count].iter().enumerate() {
            let input = out.as_ref().unwrap_or(start_image);
            let next = op
                .apply(input)
                .map_err(|error| ReplayError::new(start_idx + offset, error))?;
            out = Some(next);
        }

        Ok(out.unwrap_or_else(|| start_image.clone()))
    }

    /// Replays a contiguous operation range in best-effort mode.
    ///
    /// `start_image` is treated as the image state immediately before
    /// `start_idx`, and operations in `start_idx..count` are attempted in order.
    /// Failed operations are recorded in the returned report and skipped.
    #[must_use]
    fn replay_range_best_effort(&self, start_image: &DecodedImage, start_idx: usize, count: usize) -> ReplayReport {
        let mut out: Option<DecodedImage> = None;
        let mut skips = Vec::new();

        for (offset, op) in self.ops[start_idx..count].iter().enumerate() {
            let input = out.as_ref().unwrap_or(start_image);
            match op.apply(input) {
                Ok(next) => out = Some(next),
                Err(error) => {
                    skips.push(ReplaySkip {
                        index: start_idx + offset,
                        error,
                    });
                }
            }
        }

        ReplayReport {
            image: out.unwrap_or_else(|| start_image.clone()),
            skips,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PipelineError, TransformPipeline};
    use crate::runtime::decode::DecodedImage;
    use crate::runtime::transform::{
        Brightness, Crop, Grayscale, RotateAny, RotationInterpolation, TransformOp, Translate, TranslateMode,
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
    fn replay_strict_replays_ops_in_order() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        let brighten = Brightness::new(10);
        pipeline.push(brighten.into());
        pipeline.push(Grayscale.into());

        let manual = Grayscale
            .apply(&brighten.apply(&img).expect("brightness should succeed"))
            .expect("grayscale should succeed");
        let replayed = pipeline.replay_strict(&img).expect("strict replay should succeed");
        assert_eq!(replayed.rgba(), manual.rgba());
    }

    #[test]
    fn replay_strict_reports_failing_transform_index() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        pipeline.push(Brightness::new(1).into());
        let invalid_crop = Crop::try_new(3, 0, 1, 1).expect("non-zero crop dimensions should be accepted");
        pipeline.push(invalid_crop.into());
        pipeline.push(Grayscale.into());

        let err = pipeline
            .replay_strict(&img)
            .expect_err("strict replay should fail on out-of-bounds crop");
        assert_eq!(err.index(), 1);
    }

    #[test]
    fn replay_best_effort_skips_failed_transforms() {
        let img = test_image();
        let mut pipeline = TransformPipeline::default();
        pipeline.push(Brightness::new(1).into());
        let invalid_crop = Crop::try_new(3, 0, 1, 1).expect("non-zero crop dimensions should be accepted");
        pipeline.push(invalid_crop.into());
        pipeline.push(Grayscale.into());

        let report = pipeline.replay_best_effort(&img);

        assert_eq!(report.skips.len(), 1);
        assert_eq!(report.skips[0].index, 1);

        let manual = Grayscale
            .apply(&Brightness::new(1).apply(&img).expect("brightness should succeed"))
            .expect("grayscale should succeed");
        assert_eq!(report.image.rgba(), manual.rgba());
    }

    #[test]
    fn replay_cost_variants_stay_reasonable() {
        assert_eq!(Grayscale.replay_cost(), 1);
        assert_eq!(
            Translate::new(1, -1, TranslateMode::Crop, [0, 0, 0, 0]).replay_cost(),
            2
        );
        assert!(RotateAny::new(123, RotationInterpolation::Bicubic, true).replay_cost() > 1);
    }

    #[test]
    fn remove_returns_error_for_out_of_bounds_index() {
        let mut pipeline = TransformPipeline::default();
        pipeline.push(Brightness::new(1).into());

        let err = pipeline
            .remove(1)
            .expect_err("removing outside the pipeline length should fail");
        assert_eq!(err, PipelineError::IndexOutOfBounds { index: 1, len: 1 });
        assert_eq!(pipeline.len(), 1);
    }

    #[test]
    fn remove_returns_removed_transform_when_index_is_valid() {
        let mut pipeline = TransformPipeline::default();
        let first: super::ImageTransform = Brightness::new(1).into();
        let second: super::ImageTransform = Grayscale.into();
        pipeline.push(first.clone());
        pipeline.push(second.clone());

        let removed = pipeline.remove(0).expect("removing a valid index should succeed");

        assert_eq!(removed, first);
        assert_eq!(pipeline.ops(), &[second]);
    }
}
