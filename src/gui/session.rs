//! Narrow workflow helpers for loading documents and mutating the active session.

use std::{fs::File, path::Path};

use bmp::{
    raw::Bmp,
    runtime::{decode::decode_to_rgba, transform::ImageTransform},
};
use eframe::egui;

use super::{
    document::{DocumentInspection, DocumentState},
    panels::ViewportState,
    save::SaveState,
    steganography::SteganographyUiState,
};

/// Load-time workflow that prepares document, inspection, save, and viewer state together.
pub(in crate::gui) struct LoadSession<'a> {
    document: &'a mut DocumentState,
    inspection: &'a mut DocumentInspection,
    viewport: &'a mut ViewportState,
    steganography: &'a mut SteganographyUiState,
    save: &'a mut SaveState,
}

impl<'a> LoadSession<'a> {
    /// Loads a BMP from disk and synchronizes all load-coupled UI state.
    pub(in crate::gui) const fn new(
        document: &'a mut DocumentState,
        inspection: &'a mut DocumentInspection,
        viewport: &'a mut ViewportState,
        steganography: &'a mut SteganographyUiState,
        save: &'a mut SaveState,
    ) -> Self {
        Self {
            document,
            inspection,
            viewport,
            steganography,
            save,
        }
    }

    /// Loads a BMP from disk and synchronizes all load-coupled UI state.
    pub(in crate::gui) fn load_path(&mut self, ctx: &egui::Context, path: &Path) -> Result<String, String> {
        let mut file = File::open(path).map_err(|err| format!("Failed to open {}: {err}", path.display()))?;

        let bmp =
            Bmp::read_unchecked(&mut file).map_err(|err| format!("Parse failed for {}: {err}", path.display()))?;
        bmp.validate()
            .map_err(|err| format!("Validation failed for {}: {err}", path.display()))?;

        let decoded = decode_to_rgba(&bmp).map_err(|err| format!("Decode failed for {}: {err}", path.display()))?;

        self.save.reset_for_loaded_bmp(
            bmp::runtime::encode::SaveFormat::from_bmp(&bmp),
            bmp::runtime::encode::SaveHeaderVersion::from_bmp(&bmp),
        );
        *self.inspection = DocumentInspection::from_bmp(&bmp, &decoded);
        self.document.load_bmp(&bmp, decoded.clone(), path.to_path_buf());
        self.steganography.reset_for_loaded_image(&decoded);
        self.viewport.reset_for_new_image();
        self.viewport
            .set_display_image(ctx, &decoded, path.to_string_lossy().to_string());

        Ok(format!("Loaded {}", path.display()))
    }
}

/// Edit-time workflow that applies transforms and keeps dependent UI state in sync.
pub(in crate::gui) struct EditSession<'a> {
    document: &'a mut DocumentState,
    viewport: &'a mut ViewportState,
    steganography: &'a mut SteganographyUiState,
}

impl<'a> EditSession<'a> {
    pub(in crate::gui) const fn new(
        document: &'a mut DocumentState,
        viewport: &'a mut ViewportState,
        steganography: &'a mut SteganographyUiState,
    ) -> Self {
        Self {
            document,
            viewport,
            steganography,
        }
    }

    /// Applies a transform immediately or queues a confirmation if steg data is present.
    pub(in crate::gui) fn apply_or_queue_transform(
        &mut self,
        ctx: &egui::Context,
        op: ImageTransform,
    ) -> Result<(), String> {
        if self.steganography.should_confirm_transform(&op) {
            self.steganography.queue_transform_confirmation(op);
            return Ok(());
        }

        self.apply_transform_now(ctx, op)
    }

    /// Applies a transform to the current document and refreshes dependent UI state.
    pub(in crate::gui) fn apply_transform_now(
        &mut self,
        ctx: &egui::Context,
        op: ImageTransform,
    ) -> Result<(), String> {
        if let Some(current) = self.document.transformed_image() {
            if matches!(op, ImageTransform::EmbedSteganography(_)) {
                self.steganography.clear_overwrite_warning();
            }

            SteganographyUiState::validate_embed_transform(
                current,
                &op,
                "Embedding aborted: payload no longer fits current image. The steganography transform was not applied.",
            )?;

            if self.document.apply_transform(op)? {
                self.refresh_current_document_image(ctx);
            }
        }

        Ok(())
    }

    /// Undoes the most recent transform and returns any replay warning text.
    pub(in crate::gui) fn undo(&mut self, ctx: &egui::Context) -> Result<Option<String>, String> {
        match self.document.undo()? {
            Some(warnings) => {
                self.steganography.clear_overwrite_warning();
                self.refresh_current_document_image(ctx);
                if warnings.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(warnings.join(" ")))
                }
            }
            None => Ok(None),
        }
    }

    /// Reapplies the next redo step if it is still valid for the current image.
    pub(in crate::gui) fn redo(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let Some(op) = self.document.pop_redo() else {
            return Ok(());
        };

        if let Some(current) = self.document.transformed_image() {
            SteganographyUiState::validate_embed_transform(
                current,
                &op,
                "Redo skipped: steganography payload no longer fits after prior edits. The embed step was dropped.",
            )?;
        }

        if self.document.apply_redo(op)? {
            self.refresh_current_document_image(ctx);
        }

        Ok(())
    }

    /// Removes one transform from history and rebuilds the displayed image.
    pub(in crate::gui) fn remove_transform(&mut self, ctx: &egui::Context, index: usize) -> Option<String> {
        let warnings = self.document.remove_transform(index)?;
        self.steganography.clear_overwrite_warning();
        self.refresh_current_document_image(ctx);
        if warnings.is_empty() {
            None
        } else {
            Some(warnings.join(" "))
        }
    }

    /// Drops all transform history and restores the original loaded image.
    pub(in crate::gui) fn clear_transform_history(&mut self, ctx: &egui::Context) {
        if self.document.clear_transform_history() {
            self.steganography.clear_overwrite_warning();
            self.refresh_current_document_image(ctx);
        }
    }

    fn refresh_current_document_image(&mut self, ctx: &egui::Context) {
        let (document, steganography, viewport) = (&self.document, &mut self.steganography, &mut self.viewport);
        if let Some(image) = document.transformed_image() {
            steganography.refresh_for_image(image);
            viewport.set_display_image(ctx, image, "transformed".to_owned());
        }
    }
}
