use std::path::{Path, PathBuf};

use bmp::{
    raw::Bmp,
    runtime::{
        decode::DecodedImage,
        encode::SourceMetadata,
        transform::{
            ImageTransform, ReplayReport, ReplaySkip, TransformPipelineExecutor, TransformPipelineExecutorConfig,
        },
    },
};

/// GUI-specific replay-cost budget before creating a checkpoint snapshot.
const HISTORY_CHECKPOINT_COST_THRESHOLD: u32 = 15;
/// GUI-specific cap for cached replay checkpoints.
const HISTORY_MAX_CHECKPOINTS: usize = 5;

/// Image/session data tied to the currently loaded BMP and transform pipeline.
#[derive(Default)]
pub(in crate::gui) struct DocumentState {
    loaded: Option<LoadedDocument>,
}

/// Per-file session state that only exists while an image is loaded.
///
/// Keeping these fields in one struct prevents partially-loaded document
/// states (for example, history without an image, or a transformed image
/// without a corresponding original baseline).
struct LoadedDocument {
    /// Decoded image as originally loaded from disk.
    ///
    /// This snapshot is kept immutable so lossy operations can be replayed from
    /// a stable baseline when rebuilding the transform pipeline (for example,
    /// after removing a middle transform or undoing a non-invertible step).
    original_image: DecodedImage,
    /// Current image after applying all transforms in `history`.
    ///
    /// `None` means the current view is identical to `original_image`
    /// (typically when no transforms are applied), which avoids storing a
    /// duplicate full-size buffer.
    transformed_image: Option<DecodedImage>,
    /// Applied transform pipeline and redo stack for this loaded session.
    history: TransformHistory,
    /// Source BMP color metadata preserved for re-encoding, when available.
    source_metadata: Option<SourceMetadata>,
    /// Path of the loaded file used by overwrite-save actions.
    loaded_path: PathBuf,
}

pub(in crate::gui) struct TransformHistory {
    /// Undo/redo transform list with GUI-configured replay caching.
    pipeline: TransformPipelineExecutor,
    /// Transforms that were undone, available for redo. Cleared on new transform or step removal.
    redo_stack: Vec<ImageTransform>,
}

impl Default for TransformHistory {
    fn default() -> Self {
        Self {
            pipeline: TransformPipelineExecutor::with_config(TransformPipelineExecutorConfig::new(
                HISTORY_CHECKPOINT_COST_THRESHOLD,
                HISTORY_MAX_CHECKPOINTS,
            )),
            redo_stack: Vec::new(),
        }
    }
}

impl DocumentState {
    /// Replaces the active document session with a freshly loaded BMP.
    pub(in crate::gui) fn load_bmp(&mut self, bmp: &Bmp, decoded: DecodedImage, loaded_path: PathBuf) {
        self.loaded = Some(LoadedDocument {
            original_image: decoded,
            transformed_image: None,
            history: TransformHistory::default(),
            source_metadata: SourceMetadata::from_bmp(bmp),
            loaded_path,
        });
    }

    /// Returns the currently displayed image, falling back to the original image when no transformed copy exists yet.
    pub(in crate::gui) fn transformed_image(&self) -> Option<&DecodedImage> {
        self.loaded
            .as_ref()
            .map(|doc| doc.transformed_image.as_ref().unwrap_or(&doc.original_image))
    }

    fn replace_transformed_image(&mut self, image: DecodedImage) -> Option<()> {
        let doc = self.loaded.as_mut()?;
        if doc.history.is_empty() {
            doc.transformed_image = None;
        } else {
            doc.transformed_image = Some(image);
        }
        Some(())
    }

    pub(in crate::gui) fn source_metadata(&self) -> Option<&SourceMetadata> {
        self.loaded.as_ref().and_then(|doc| doc.source_metadata.as_ref())
    }

    pub(in crate::gui) fn source_metadata_cloned(&self) -> Option<SourceMetadata> {
        self.loaded.as_ref().and_then(|doc| doc.source_metadata.clone())
    }

    pub(in crate::gui) fn loaded_path(&self) -> Option<&Path> {
        self.loaded.as_ref().map(|doc| doc.loaded_path.as_path())
    }

    pub(in crate::gui) fn history(&self) -> Option<&TransformHistory> {
        self.loaded.as_ref().map(|doc| &doc.history)
    }

    pub(in crate::gui) fn history_mut(&mut self) -> Option<&mut TransformHistory> {
        self.loaded.as_mut().map(|doc| &mut doc.history)
    }

    /// Applies a transform and stores the resulting image and history entry.
    pub(in crate::gui) fn apply_transform(&mut self, op: ImageTransform) -> Result<bool, String> {
        let Some(current) = self.transformed_image() else {
            return Ok(false);
        };

        let next = op
            .apply(current)
            .map_err(|err| format!("Failed to apply transform {op}: {err}"))?;
        if let Some(history) = self.history_mut() {
            history.record_apply(op);
        }

        let () = self
            .replace_transformed_image(next)
            .expect("loaded document should still exist while applying a transform");
        Ok(true)
    }

    /// Undoes the most recent transform, replaying from the original image when necessary.
    pub(in crate::gui) fn undo(&mut self) -> Result<Option<Vec<String>>, String> {
        let Some(op) = self.history_mut().and_then(TransformHistory::pop_undo) else {
            return Ok(None);
        };

        if let Some(inv) = op.inverse() {
            if let Some(history) = self.history_mut() {
                history.push_redo(op);
            }

            if self.history().is_some_and(TransformHistory::is_empty) {
                if let Some(doc) = self.loaded.as_mut() {
                    doc.transformed_image = None;
                }
                return Ok(Some(Vec::new()));
            }

            let Some(current) = self.transformed_image() else {
                return Ok(None);
            };

            let result = inv
                .apply(current)
                .map_err(|err| format!("Undo failed while applying inverse: {err}"))?;

            let () = self
                .replace_transformed_image(result)
                .expect("loaded document should still exist during undo");
            return Ok(Some(Vec::new()));
        }

        if let Some(history) = self.history_mut() {
            history.push_redo(op);
        }

        let (result, warnings) = {
            let Some(doc) = self.loaded.as_mut() else {
                return Ok(None);
            };

            if doc.history.is_empty() {
                doc.transformed_image = None;
                return Ok(Some(Vec::new()));
            }

            doc.history.replay_best_effort(&doc.original_image)
        };
        let () = self
            .replace_transformed_image(result)
            .expect("loaded document should still exist during undo replay");
        Ok(Some(warnings))
    }

    /// Removes and returns the next redo operation from history.
    pub(in crate::gui) fn pop_redo(&mut self) -> Option<ImageTransform> {
        self.history_mut().and_then(TransformHistory::pop_redo)
    }

    /// Applies a previously popped redo operation back onto the current image.
    pub(in crate::gui) fn apply_redo(&mut self, op: ImageTransform) -> Result<bool, String> {
        let Some(current) = self.transformed_image() else {
            return Ok(false);
        };

        let next = op
            .apply(current)
            .map_err(|err| format!("Redo failed while applying {op}: {err}"))?;
        if let Some(history) = self.history_mut() {
            history.record_redo_apply(op);
        }

        let () = self
            .replace_transformed_image(next)
            .expect("loaded document should still exist during redo");
        Ok(true)
    }

    /// Removes a transform from history and rebuilds the image from the remaining pipeline.
    pub(in crate::gui) fn remove_transform(&mut self, index: usize) -> Option<Vec<String>> {
        let (result, warnings) = {
            let doc = self.loaded.as_mut()?;
            if !doc.history.remove(index) {
                return None;
            }

            if doc.history.is_empty() {
                doc.transformed_image = None;
                return Some(Vec::new());
            }

            doc.history.replay_best_effort(&doc.original_image)
        };
        let () = self.replace_transformed_image(result)?;
        Some(warnings)
    }

    /// Clears all transforms and restores the original loaded image.
    pub(in crate::gui) fn clear_transform_history(&mut self) -> bool {
        let Some(doc) = self.loaded.as_mut() else {
            return false;
        };

        doc.history.clear();
        doc.transformed_image = None;
        true
    }
}

impl TransformHistory {
    pub(in crate::gui) fn clear(&mut self) {
        self.pipeline.clear();
        self.redo_stack.clear();
    }

    pub(in crate::gui) fn record_apply(&mut self, op: ImageTransform) {
        self.pipeline.push(op);
        self.redo_stack.clear();
    }

    pub(in crate::gui) fn record_redo_apply(&mut self, op: ImageTransform) {
        self.pipeline.push(op);
    }

    pub(in crate::gui) fn pop_undo(&mut self) -> Option<ImageTransform> {
        self.pipeline.pop()
    }

    pub(in crate::gui) fn push_redo(&mut self, op: ImageTransform) {
        self.redo_stack.push(op);
    }

    pub(in crate::gui) fn pop_redo(&mut self) -> Option<ImageTransform> {
        self.redo_stack.pop()
    }

    pub(in crate::gui) fn remove(&mut self, index: usize) -> bool {
        if self.pipeline.remove(index).is_err() {
            return false;
        }
        self.redo_stack.clear();
        true
    }

    /// Replays the stored pipeline in best-effort mode and collects warnings.
    pub(in crate::gui) fn replay_best_effort(&mut self, original: &DecodedImage) -> (DecodedImage, Vec<String>) {
        let ReplayReport { image, skips } = self.pipeline.replay_best_effort(original);
        let ops = self.pipeline.ops();
        let warnings = skips
            .into_iter()
            .map(|skip| format_replay_skip(&skip, ops.get(skip.index)))
            .collect();
        (image, warnings)
    }

    pub(in crate::gui) const fn is_empty(&self) -> bool {
        self.pipeline.is_empty()
    }

    pub(in crate::gui) const fn len(&self) -> usize {
        self.pipeline.len()
    }

    pub(in crate::gui) fn ops(&self) -> &[ImageTransform] {
        self.pipeline.ops()
    }

    pub(in crate::gui) fn last_applied(&self) -> Option<&ImageTransform> {
        self.pipeline.ops().last()
    }

    pub(in crate::gui) const fn has_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub(in crate::gui) fn last_redo(&self) -> Option<&ImageTransform> {
        self.redo_stack.last()
    }
}

/// Formats a runtime replay skip into a GUI-facing warning message.
///
/// The optional transform reference is looked up from current pipeline state.
/// If that lookup fails, the message falls back to index-based wording.
fn format_replay_skip(skip: &ReplaySkip, op: Option<&ImageTransform>) -> String {
    op.map_or_else(
        || {
            format!(
                "Skipped transform at index {} during replay: {}",
                skip.index, skip.error
            )
        },
        |op| format!("Skipped {} during replay: {}", op, skip.error),
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bmp::runtime::{
        decode::DecodedImage,
        transform::{ImageTransform, InvertColors, MirrorHorizontal},
    };

    use super::{DocumentState, LoadedDocument, TransformHistory};

    fn test_image() -> DecodedImage {
        DecodedImage::new(
            2,
            2,
            vec![10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255],
        )
        .expect("valid test image")
    }

    fn loaded_state() -> DocumentState {
        DocumentState {
            loaded: Some(LoadedDocument {
                original_image: test_image(),
                transformed_image: None,
                history: TransformHistory::default(),
                source_metadata: None,
                loaded_path: PathBuf::from("test.bmp"),
            }),
        }
    }

    #[test]
    fn redo_keeps_remaining_undone_steps() {
        let mut state = loaded_state();
        let first: ImageTransform = InvertColors.into();
        let second: ImageTransform = MirrorHorizontal.into();

        assert!(
            state
                .apply_transform(first.clone())
                .expect("first apply should succeed")
        );
        assert!(
            state
                .apply_transform(second.clone())
                .expect("second apply should succeed")
        );
        assert_eq!(state.history().expect("history should exist").len(), 2);

        assert!(state.undo().expect("first undo should succeed").is_some());
        assert!(state.undo().expect("second undo should succeed").is_some());

        let history = state.history().expect("history should exist after undo");
        assert_eq!(history.len(), 0);
        assert_eq!(history.last_redo(), Some(&first));

        let first_redo = state.pop_redo().expect("first redo op should exist");
        assert_eq!(first_redo, first);
        assert!(state.apply_redo(first_redo).expect("first redo should apply"));

        let history = state.history().expect("history should exist after first redo");
        assert_eq!(history.len(), 1);
        assert_eq!(history.last_redo(), Some(&second));

        let second_redo = state.pop_redo().expect("second redo op should still exist");
        assert_eq!(second_redo, second);
        assert!(state.apply_redo(second_redo).expect("second redo should apply"));

        let history = state.history().expect("history should exist after second redo");
        assert_eq!(history.len(), 2);
        assert!(!history.has_redo());
    }
}
