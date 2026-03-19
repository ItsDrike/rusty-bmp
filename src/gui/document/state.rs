use std::path::{Path, PathBuf};

use bmp::{
    raw::Bmp,
    runtime::{
        decode::DecodedImage,
        encode::SourceMetadata,
        transform::{ImageTransform, TransformPipeline},
    },
};

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

#[derive(Default)]
pub(in crate::gui) struct TransformHistory {
    pipeline: TransformPipeline,
    /// Transforms that were undone, available for redo. Cleared on new transform or step removal.
    redo_stack: Vec<ImageTransform>,
}

/// Result of a document edit that produced a new image and optional replay warnings.
pub(in crate::gui) struct DocumentImageChange {
    /// The new image produced by the edit.
    pub(in crate::gui) image: DecodedImage,
    /// Warnings collected while replaying lossy transforms.
    pub(in crate::gui) warnings: Vec<String>,
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

    pub(in crate::gui) fn original_image(&self) -> Option<&DecodedImage> {
        self.loaded.as_ref().map(|doc| &doc.original_image)
    }

    fn replace_transformed_image(&mut self, image: DecodedImage) -> Option<DecodedImage> {
        let doc = self.loaded.as_mut()?;
        if doc.history.is_empty() {
            doc.transformed_image = None;
        } else {
            doc.transformed_image = Some(image.clone());
        }
        Some(image)
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
    pub(in crate::gui) fn apply_transform(&mut self, op: ImageTransform) -> Result<Option<DecodedImage>, String> {
        let Some(current) = self.transformed_image() else {
            return Ok(None);
        };

        let next = op
            .apply(current)
            .map_err(|err| format!("Failed to apply transform {op}: {err}"))?;
        let checkpoint_image = current.clone();
        if let Some(history) = self.history_mut() {
            history.record_apply(op, Some(&checkpoint_image));
        }

        Ok(self.replace_transformed_image(next))
    }

    /// Undoes the most recent transform, replaying from the original image when necessary.
    pub(in crate::gui) fn undo(&mut self) -> Result<Option<DocumentImageChange>, String> {
        let Some(op) = self.history_mut().and_then(TransformHistory::pop_undo) else {
            return Ok(None);
        };

        if let Some(inv) = op.inverse() {
            if let Some(history) = self.history_mut() {
                history.push_redo(op);
            }

            let Some(current) = self.transformed_image() else {
                return Ok(None);
            };

            let result = inv
                .apply(current)
                .map_err(|err| format!("Undo failed while applying inverse: {err}"))?;

            let image = self
                .replace_transformed_image(result)
                .expect("loaded document should still exist during undo");
            return Ok(Some(DocumentImageChange {
                image,
                warnings: Vec::new(),
            }));
        }

        if let Some(history) = self.history_mut() {
            history.push_redo(op);
        }

        let (Some(original), Some(history)) = (self.original_image(), self.history()) else {
            return Ok(None);
        };

        let (result, warnings) = history.apply_with_warnings(original);
        let image = self
            .replace_transformed_image(result)
            .expect("loaded document should still exist during undo replay");
        Ok(Some(DocumentImageChange { image, warnings }))
    }

    /// Removes and returns the next redo operation from history.
    pub(in crate::gui) fn pop_redo(&mut self) -> Option<ImageTransform> {
        self.history_mut().and_then(TransformHistory::pop_redo)
    }

    /// Applies a previously popped redo operation back onto the current image.
    pub(in crate::gui) fn apply_redo(&mut self, op: ImageTransform) -> Result<Option<DecodedImage>, String> {
        let Some(current) = self.transformed_image() else {
            return Ok(None);
        };

        let next = op
            .apply(current)
            .map_err(|err| format!("Redo failed while applying {op}: {err}"))?;
        let checkpoint_image = current.clone();
        if let Some(history) = self.history_mut() {
            history.record_redo_apply(op, Some(&checkpoint_image));
        }

        let image = self
            .replace_transformed_image(next)
            .expect("loaded document should still exist during redo");
        Ok(Some(image))
    }

    /// Removes a transform from history and rebuilds the image from the remaining pipeline.
    pub(in crate::gui) fn remove_transform(&mut self, index: usize) -> Option<DocumentImageChange> {
        let removed = self.history_mut().is_some_and(|history| history.remove(index));
        if !removed {
            return None;
        }

        let (Some(original), Some(history)) = (self.original_image(), self.history()) else {
            return None;
        };

        let (result, warnings) = history.apply_with_warnings(original);
        let image = self.replace_transformed_image(result)?;
        Some(DocumentImageChange { image, warnings })
    }

    /// Clears all transforms and restores the original loaded image.
    pub(in crate::gui) fn clear_transform_history(&mut self) -> Option<DecodedImage> {
        if let Some(history) = self.history_mut() {
            history.clear();
        }

        let result = self.original_image()?.clone();
        self.replace_transformed_image(result)
    }
}

impl TransformHistory {
    pub(in crate::gui) fn clear(&mut self) {
        self.pipeline.clear();
        self.redo_stack.clear();
    }

    pub(in crate::gui) fn record_apply(&mut self, op: ImageTransform, current_image: Option<&DecodedImage>) {
        self.pipeline.push(op, current_image);
        self.redo_stack.clear();
    }

    pub(in crate::gui) fn record_redo_apply(&mut self, op: ImageTransform, current_image: Option<&DecodedImage>) {
        self.pipeline.push(op, current_image);
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

    /// Replays the stored pipeline and collects warnings for lossy steps.
    pub(in crate::gui) fn apply_with_warnings(&self, original: &DecodedImage) -> (DecodedImage, Vec<String>) {
        self.pipeline.apply_with_warnings(original)
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
                .is_some()
        );
        assert!(
            state
                .apply_transform(second.clone())
                .expect("second apply should succeed")
                .is_some()
        );
        assert_eq!(state.history().expect("history should exist").len(), 2);

        assert!(state.undo().expect("first undo should succeed").is_some());
        assert!(state.undo().expect("second undo should succeed").is_some());

        let history = state.history().expect("history should exist after undo");
        assert_eq!(history.len(), 0);
        assert_eq!(history.last_redo(), Some(&first));

        let first_redo = state.pop_redo().expect("first redo op should exist");
        assert_eq!(first_redo, first);
        assert!(state.apply_redo(first_redo).expect("first redo should apply").is_some());

        let history = state.history().expect("history should exist after first redo");
        assert_eq!(history.len(), 1);
        assert_eq!(history.last_redo(), Some(&second));

        let second_redo = state.pop_redo().expect("second redo op should still exist");
        assert_eq!(second_redo, second);
        assert!(
            state
                .apply_redo(second_redo)
                .expect("second redo should apply")
                .is_some()
        );

        let history = state.history().expect("history should exist after second redo");
        assert_eq!(history.len(), 2);
        assert!(!history.has_redo());
    }
}
