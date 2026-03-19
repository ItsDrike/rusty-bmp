//! Shared steganography UI state and confirmation helpers.

use std::sync::Arc;

use bmp::runtime::{
    decode::DecodedImage,
    steganography::{self, StegInfo},
    transform::ImageTransform,
};
use eframe::egui;

/// Steganography-related detection state and window inputs.
pub(in crate::gui) struct SteganographyUiState {
    /// Steganography detected in the current transformed image, if any.
    pub(in crate::gui) detected: Option<StegInfo>,
    /// Whether the "Embed Steganography" window is open.
    pub(in crate::gui) embed_open: bool,
    /// Whether the "Inspect Steganography" window is open.
    pub(in crate::gui) inspect_open: bool,
    /// Whether we already warned the user that a transform was
    /// applied while steganography was detected in the image.
    /// Reset when loading a new image and in undo/embed paths.
    pub(in crate::gui) overwrite_warned: bool,
    /// Transform awaiting confirmation because it would likely corrupt an
    /// existing embedded steganography payload.
    pub(in crate::gui) transform_confirm_pending: Option<ImageTransform>,

    // --- Embed window inputs ---
    pub(in crate::gui) r_bits: u8,
    pub(in crate::gui) g_bits: u8,
    pub(in crate::gui) b_bits: u8,
    pub(in crate::gui) a_bits: u8,
    pub(in crate::gui) text_input: String,

    // --- Inspect window: cached extracted payload ---
    /// Result of the last explicit "Extract" action in the inspect window.
    /// `None` = not yet extracted; `Some(Ok(bytes))` = payload; `Some(Err(msg))` = error.
    pub(in crate::gui) extracted: Option<Result<Arc<[u8]>, String>>,
}

impl Default for SteganographyUiState {
    fn default() -> Self {
        Self {
            detected: None,
            embed_open: false,
            inspect_open: false,
            overwrite_warned: false,
            transform_confirm_pending: None,
            r_bits: 1,
            g_bits: 1,
            b_bits: 1,
            a_bits: 0,
            text_input: String::new(),
            extracted: None,
        }
    }
}

impl SteganographyUiState {
    /// Verifies that an embed transform still fits the current image state.
    pub(in crate::gui) fn validate_embed_transform(
        current: &DecodedImage,
        op: &ImageTransform,
        failure_message: &str,
    ) -> Result<(), String> {
        let ImageTransform::EmbedSteganography(embed) = op else {
            return Ok(());
        };

        steganography::embed(current, embed.config, &embed.payload)
            .map(|_| ())
            .map_err(|err| format!("{failure_message} ({err})"))
    }

    /// Replaces all session-coupled steg state for a newly loaded image.
    pub(in crate::gui) fn reset_for_loaded_image(&mut self, image: &DecodedImage) {
        self.detected = steganography::detect(image);
        self.extracted = None;
        self.overwrite_warned = false;
        self.transform_confirm_pending = None;
    }

    /// Refreshes detection and extraction state after the displayed image changes.
    pub(in crate::gui) fn refresh_for_image(&mut self, image: &DecodedImage) {
        self.detected = steganography::detect(image);
        self.extracted = None;
    }

    pub(in crate::gui) const fn clear_overwrite_warning(&mut self) {
        self.overwrite_warned = false;
    }

    pub(in crate::gui) const fn should_confirm_transform(&self, op: &ImageTransform) -> bool {
        !self.overwrite_warned
            && !matches!(op, ImageTransform::EmbedSteganography(_))
            && !matches!(op, ImageTransform::RemoveSteganography(_))
            && self.detected.is_some()
    }

    /// Queues a transform for confirmation because it may corrupt hidden data.
    pub(in crate::gui) fn queue_transform_confirmation(&mut self, op: ImageTransform) {
        self.transform_confirm_pending = Some(op);
    }

    /// Stores the most recent extract result shown by the inspect dialog.
    pub(in crate::gui) fn set_extracted(&mut self, result: Result<Arc<[u8]>, String>) {
        self.extracted = Some(result);
    }

    /// Shows confirmation before applying a transform that would likely
    /// corrupt a just-embedded steganography payload.
    pub(in crate::gui) fn show_transform_confirm_window(&mut self, ctx: &egui::Context) -> Option<ImageTransform> {
        let op = self.transform_confirm_pending.as_ref()?;

        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new("Warning: Transform May Corrupt Steganography")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "This transform is being applied on top of an embedded steganography payload.",
                );
                ui.add_space(4.0);
                ui.label("That will likely destroy or corrupt the hidden data.");
                ui.label(format!("Pending transform: {op}"));
                ui.add_space(8.0);
                ui.label("Apply anyway?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Apply Anyway").clicked() {
                        confirmed = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                });
            });

        if confirmed {
            let op = self.transform_confirm_pending.take();
            self.overwrite_warned = true;
            op
        } else if cancelled {
            self.transform_confirm_pending = None;
            None
        } else {
            None
        }
    }
}
