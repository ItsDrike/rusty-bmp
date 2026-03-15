use std::collections::BTreeMap;

use crate::runtime::decode::DecodedImage;
use crate::runtime::steganography;

use super::dispatch::apply_transform;
use super::model::ImageTransform;

const CHECKPOINT_COST_THRESHOLD: u32 = 15;
const MAX_CHECKPOINTS: usize = 5;

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

    pub fn remove(&mut self, index: usize) {
        self.ops.remove(index);
        self.checkpoints.retain(|&k, _| k < index);
        self.recompute_cost_since_checkpoint();
    }

    pub fn pop(&mut self) -> Option<ImageTransform> {
        let op = self.ops.pop()?;
        let len = self.ops.len();
        self.checkpoints.remove(&len);
        self.recompute_cost_since_checkpoint();
        Some(op)
    }

    #[must_use]
    pub fn apply(&self, original: &DecodedImage) -> DecodedImage {
        self.apply_range(original, self.ops.len())
    }

    #[must_use]
    pub fn apply_with_warnings(&self, original: &DecodedImage) -> (DecodedImage, Vec<String>) {
        self.apply_range_with_warnings(original, self.ops.len())
    }

    fn apply_range(&self, original: &DecodedImage, count: usize) -> DecodedImage {
        self.apply_range_with_warnings(original, count).0
    }

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

    fn recompute_cost_since_checkpoint(&mut self) {
        let last_cp = self.checkpoints.keys().next_back().copied();
        let start = last_cp.map_or(0, |i| i + 1);
        self.cost_since_checkpoint = self.ops[start..].iter().map(ImageTransform::replay_cost).sum();
    }
}

#[cfg(test)]
mod tests {
    use super::{CHECKPOINT_COST_THRESHOLD, MAX_CHECKPOINTS, TransformPipeline};
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
