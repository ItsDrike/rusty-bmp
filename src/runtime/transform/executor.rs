//! Optional cached replay executor for [`TransformPipeline`](super::TransformPipeline).
//!
//! The core pipeline type is intentionally stateless. This executor provides an
//! opt-in cache layer for repeated replays in interactive scenarios (for
//! example for faster undo/redo histories in a GUI).

use std::collections::BTreeMap;

use crate::runtime::decode::DecodedImage;

use super::{
    model::ImageTransform,
    pipeline::{PipelineError, ReplayError, ReplayReport, ReplaySkip, TransformPipeline},
};

/// Runtime policy for checkpointed replay caching.
///
/// Use `max_checkpoints = 0` to disable caching entirely.
///
/// `checkpoint_cost_threshold` is evaluated against the cumulative
/// [`ImageTransform::replay_cost`](super::ImageTransform::replay_cost) since the
/// last checkpoint candidate in a replay run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransformPipelineExecutorConfig {
    /// Cost budget before the next successful replay step becomes a checkpoint.
    ///
    /// `0` means checkpoint every successful operation whose replay cost is
    /// non-zero.
    pub checkpoint_cost_threshold: u32,
    /// Maximum number of checkpoint snapshots to keep.
    ///
    /// Oldest checkpoints are dropped when this limit is exceeded.
    pub max_checkpoints: usize,
}

impl TransformPipelineExecutorConfig {
    /// Creates a replay-cache configuration.
    #[must_use]
    pub const fn new(checkpoint_cost_threshold: u32, max_checkpoints: usize) -> Self {
        Self {
            checkpoint_cost_threshold,
            max_checkpoints,
        }
    }
}

/// Mutable replay executor with optional internal checkpoint caching.
///
/// This type owns a [`TransformPipeline`] and maintains replay checkpoints that
/// can speed up repeated replays from the same source image lineage.
#[derive(Debug, Clone)]
pub struct TransformPipelineExecutor {
    pipeline: TransformPipeline,
    checkpoints: BTreeMap<usize, DecodedImage>,
    config: TransformPipelineExecutorConfig,
}

impl TransformPipelineExecutor {
    /// Creates an executor around an existing pipeline.
    ///
    /// The checkpoint cache starts empty.
    #[must_use]
    pub const fn new(pipeline: TransformPipeline, config: TransformPipelineExecutorConfig) -> Self {
        Self {
            pipeline,
            checkpoints: BTreeMap::new(),
            config,
        }
    }

    /// Creates an executor with an empty pipeline and a custom cache policy.
    #[must_use]
    pub fn with_config(config: TransformPipelineExecutorConfig) -> Self {
        Self::new(TransformPipeline::default(), config)
    }

    /// Returns the current cache policy.
    #[must_use]
    pub const fn config(&self) -> TransformPipelineExecutorConfig {
        self.config
    }

    /// Replaces the cache policy and clears existing checkpoints.
    ///
    /// Clearing is required because old checkpoints were created under a
    /// different policy and cost budget.
    pub fn set_config(&mut self, config: TransformPipelineExecutorConfig) {
        self.config = config;
        self.checkpoints.clear();
    }

    /// Returns transforms in replay order.
    #[must_use]
    pub fn ops(&self) -> &[ImageTransform] {
        self.pipeline.ops()
    }

    /// Returns `true` when there are no transforms.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.pipeline.is_empty()
    }

    /// Returns the number of stored transforms.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.pipeline.len()
    }

    /// Appends a transform to the end of the pipeline.
    ///
    /// Existing checkpoints remain valid because they refer to prefixes that do
    /// not include the newly appended operation.
    pub fn push(&mut self, op: ImageTransform) {
        self.pipeline.push(op);
    }

    /// Removes all transforms and clears the checkpoint cache.
    pub fn clear(&mut self) {
        self.pipeline.clear();
        self.checkpoints.clear();
    }

    /// Removes the transform at `index`.
    ///
    /// # Errors
    /// Returns [`PipelineError::IndexOutOfBounds`] if `index` does not point to
    /// an existing transform.
    pub fn remove(&mut self, index: usize) -> Result<ImageTransform, PipelineError> {
        let removed = self.pipeline.remove(index)?;
        self.drop_checkpoints_at_or_after(index);
        Ok(removed)
    }

    /// Removes and returns the last transform, if any.
    ///
    /// Checkpoints at or past the new tail are dropped.
    pub fn pop(&mut self) -> Option<ImageTransform> {
        let op = self.pipeline.pop()?;
        self.drop_checkpoints_at_or_after(self.pipeline.len());
        Some(op)
    }

    /// Replays all transforms and fails on the first error.
    ///
    /// Successful runs may update internal checkpoint cache state according to
    /// [`TransformPipelineExecutorConfig`].
    ///
    /// # Errors
    /// Returns [`ReplayError`] with the failing operation index.
    pub fn replay_strict(&mut self, original: &DecodedImage) -> Result<DecodedImage, ReplayError> {
        let count = self.pipeline.len();
        let (result, pending_checkpoints): (DecodedImage, Vec<(usize, DecodedImage)>) = {
            let (start_idx, start_image) = self.replay_start(original, count);
            let mut out: Option<DecodedImage> = None;
            let mut pending = Vec::new();
            let mut cost_since_checkpoint = 0u32;

            for (offset, op) in self.pipeline.ops()[start_idx..count].iter().enumerate() {
                let input = out.as_ref().unwrap_or(start_image);
                let next = op
                    .apply(input)
                    .map_err(|error| ReplayError::new(start_idx + offset, error))?;

                if self.should_checkpoint(op.replay_cost(), &mut cost_since_checkpoint) {
                    let checkpoint_idx = start_idx + offset;
                    pending.push((checkpoint_idx, next.clone()));
                }

                out = Some(next);
            }

            (out.unwrap_or_else(|| start_image.clone()), pending)
        };

        self.store_pending_checkpoints(pending_checkpoints);
        Ok(result)
    }

    /// Replays all transforms, skipping failed operations.
    ///
    /// While replay remains successful, this can update internal checkpoints.
    /// After the first skipped transform in a run, checkpoint updates are
    /// suspended for the rest of that replay to avoid caching partial results
    /// past a known failure.
    #[must_use]
    pub fn replay_best_effort(&mut self, original: &DecodedImage) -> ReplayReport {
        let count = self.pipeline.len();
        let (report, pending_checkpoints): (ReplayReport, Vec<(usize, DecodedImage)>) = {
            let (start_idx, start_image) = self.replay_start(original, count);
            let mut out: Option<DecodedImage> = None;
            let mut skips = Vec::new();
            let mut pending = Vec::new();
            let mut cost_since_checkpoint = 0u32;
            let mut can_checkpoint = true;

            for (offset, op) in self.pipeline.ops()[start_idx..count].iter().enumerate() {
                let input = out.as_ref().unwrap_or(start_image);
                match op.apply(input) {
                    Ok(next) => {
                        if can_checkpoint && self.should_checkpoint(op.replay_cost(), &mut cost_since_checkpoint) {
                            let checkpoint_idx = start_idx + offset;
                            pending.push((checkpoint_idx, next.clone()));
                        }
                        out = Some(next);
                    }
                    Err(error) => {
                        skips.push(ReplaySkip {
                            index: start_idx + offset,
                            error,
                        });
                        can_checkpoint = false;
                    }
                }
            }

            (
                ReplayReport {
                    image: out.unwrap_or_else(|| start_image.clone()),
                    skips,
                },
                pending,
            )
        };

        self.store_pending_checkpoints(pending_checkpoints);
        report
    }

    /// Chooses the replay start point for `0..count` operations.
    ///
    /// Returns:
    /// - the first transform index that still needs replay, and
    /// - the image snapshot to replay from.
    ///
    /// If a checkpoint exists before `count`, this starts right after the
    /// newest such checkpoint. Otherwise it starts from `original` at index `0`.
    fn replay_start<'a>(&'a self, original: &'a DecodedImage, count: usize) -> (usize, &'a DecodedImage) {
        self.checkpoints
            .range(..count)
            .next_back()
            .map_or((0, original), |(&idx, image)| (idx + 1, image))
    }

    /// Updates running replay cost and decides whether to emit a checkpoint.
    ///
    /// Checkpointing is disabled when `max_checkpoints == 0`.
    /// Operations with zero replay cost never trigger checkpoints.
    const fn should_checkpoint(&self, op_cost: u32, cost_since_checkpoint: &mut u32) -> bool {
        if self.config.max_checkpoints == 0 || op_cost == 0 {
            return false;
        }

        *cost_since_checkpoint = cost_since_checkpoint.saturating_add(op_cost);
        if *cost_since_checkpoint < self.config.checkpoint_cost_threshold {
            return false;
        }

        *cost_since_checkpoint = 0;
        true
    }

    /// Inserts replay-generated checkpoints and enforces cache size limits.
    ///
    /// If the cache exceeds `max_checkpoints`, oldest entries are evicted first.
    fn store_pending_checkpoints(&mut self, pending: Vec<(usize, DecodedImage)>) {
        for (idx, image) in pending {
            self.checkpoints.insert(idx, image);
        }

        while self.checkpoints.len() > self.config.max_checkpoints {
            let _ = self.checkpoints.pop_first();
        }
    }

    /// Drops checkpoints that depend on transforms at or after `cutoff`.
    ///
    /// This is used after mutating the pipeline shape (remove/pop), where any
    /// checkpoint from `cutoff` onward may no longer match the operation list.
    fn drop_checkpoints_at_or_after(&mut self, cutoff: usize) {
        let _ = self.checkpoints.split_off(&cutoff);
    }
}

#[cfg(test)]
mod tests {
    use super::{TransformPipelineExecutor, TransformPipelineExecutorConfig};
    use crate::runtime::decode::DecodedImage;
    use crate::runtime::transform::{Brightness, Crop, Grayscale, TransformOp};

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
        let cfg = TransformPipelineExecutorConfig::new(15, 5);
        let mut exec = TransformPipelineExecutor::with_config(cfg);

        for _ in 0..5 {
            exec.push(Brightness::new(1).into());
        }

        let _ = exec.replay_strict(&img).expect("strict replay should succeed");
        assert!(exec.checkpoints.is_empty());
    }

    #[test]
    fn checkpoint_created_at_threshold() {
        let img = test_image();
        let cfg = TransformPipelineExecutorConfig::new(3, 5);
        let mut exec = TransformPipelineExecutor::with_config(cfg);

        for _ in 0..3 {
            exec.push(Brightness::new(1).into());
        }

        let _ = exec.replay_strict(&img).expect("strict replay should succeed");
        assert!(!exec.checkpoints.is_empty());
    }

    #[test]
    fn max_checkpoints_enforced() {
        let img = test_image();
        let cfg = TransformPipelineExecutorConfig::new(1, 2);
        let mut exec = TransformPipelineExecutor::with_config(cfg);
        let mut manual = img.clone();

        for _ in 0..8 {
            exec.push(Brightness::new(1).into());
            manual = Brightness::new(1).apply(&manual).expect("brightness should succeed");
        }

        let replayed = exec.replay_strict(&img).expect("strict replay should succeed");
        assert_eq!(replayed.rgba(), manual.rgba());
        assert!(exec.checkpoints.len() <= cfg.max_checkpoints);
    }

    #[test]
    fn remove_invalidates_future_checkpoints() {
        let img = test_image();
        let cfg = TransformPipelineExecutorConfig::new(1, 10);
        let mut exec = TransformPipelineExecutor::with_config(cfg);

        for _ in 0..6 {
            exec.push(Brightness::new(1).into());
        }
        let _ = exec.replay_strict(&img).expect("strict replay should succeed");
        assert!(!exec.checkpoints.is_empty());

        let _ = exec.remove(2).expect("removing valid index should succeed");
        assert!(exec.checkpoints.keys().all(|&idx| idx < 2));
    }

    #[test]
    fn strict_failure_does_not_commit_partial_checkpoints() {
        let img = test_image();
        let cfg = TransformPipelineExecutorConfig::new(1, 10);
        let mut exec = TransformPipelineExecutor::with_config(cfg);
        exec.push(Brightness::new(1).into());
        let invalid_crop = Crop::try_new(3, 0, 1, 1).expect("non-zero crop dimensions should be accepted");
        exec.push(invalid_crop.into());

        let err = exec
            .replay_strict(&img)
            .expect_err("strict replay should fail on out-of-bounds crop");
        assert_eq!(err.index(), 1);
        assert!(exec.checkpoints.is_empty());
    }

    #[test]
    fn best_effort_stops_checkpointing_after_first_skip() {
        let img = test_image();
        let cfg = TransformPipelineExecutorConfig::new(1, 10);
        let mut exec = TransformPipelineExecutor::with_config(cfg);
        exec.push(Brightness::new(1).into());
        let invalid_crop = Crop::try_new(3, 0, 1, 1).expect("non-zero crop dimensions should be accepted");
        exec.push(invalid_crop.into());
        exec.push(Grayscale.into());

        let report = exec.replay_best_effort(&img);

        assert_eq!(report.skips.len(), 1);
        assert_eq!(report.skips[0].index, 1);
        assert!(exec.checkpoints.contains_key(&0));
        assert!(!exec.checkpoints.contains_key(&2));
    }
}
