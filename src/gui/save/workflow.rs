//! File-dialog, confirmation, and background save execution logic.

use std::{
    path::Path,
    sync::mpsc::{self, TryRecvError},
    time::Duration,
};

use bmp::runtime::{decode::DecodedImage, steganography::StegInfo};
use eframe::egui;
use rfd::FileDialog;

use crate::gui::document::DocumentState;

use super::{
    quality::warning_reasons,
    state::{PendingSaveTask, SavePoll, SaveState, SaveTaskResult},
};

impl SaveState {
    /// Opens a "Save As" dialog and starts saving the current image.
    pub(in crate::gui) fn save_current(
        &mut self,
        ctx: &egui::Context,
        document: &DocumentState,
        detected: Option<&StegInfo>,
    ) -> Result<(), String> {
        if self.pending_save.is_some() {
            return Err("A save operation is already in progress".to_owned());
        }

        if document.transformed_image().is_none() {
            return Err("Nothing to save".to_owned());
        }

        let Some(path) = FileDialog::new()
            .add_filter("Bitmap image", &["bmp"])
            .set_title("Save transformed BMP")
            .set_file_name("transformed.bmp")
            .save_file()
        else {
            return Ok(());
        };

        self.save_to_path(ctx, document, detected, &path)
    }

    /// Saves to the currently loaded file path without opening a picker.
    pub(in crate::gui) fn save_overwrite(
        &mut self,
        ctx: &egui::Context,
        document: &DocumentState,
        detected: Option<&StegInfo>,
    ) -> Result<(), String> {
        let Some(path) = document.loaded_path() else {
            return Err("No file to overwrite".to_owned());
        };

        self.save_to_path(ctx, document, detected, path)
    }

    /// Shows the warning dialog for lossy or potentially destructive save settings.
    pub(in crate::gui) fn show_confirm_window(
        &mut self,
        ctx: &egui::Context,
        document: &DocumentState,
    ) -> Result<(), String> {
        if self.save_confirm_pending.is_none() {
            return Ok(());
        }

        let reason = self.save_confirm_reason.clone().unwrap_or_else(|| {
            format!(
                "The selected settings ({}, {}) may alter image data",
                self.save_format, self.save_header_version
            )
        });

        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new("Warning: Save May Alter Image Data")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(400.0)
            .show(ctx, |ui| {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "The selected save settings do not preserve the currently displayed image exactly.",
                );
                ui.add_space(4.0);
                ui.label("Detected issues:");
                for line in reason.lines() {
                    ui.label(format!("- {line}"));
                }
                ui.add_space(8.0);
                ui.label("Save anyway?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save Anyway").clicked() {
                        confirmed = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                });
            });

        if confirmed {
            let Some(path) = self.save_confirm_pending.clone() else {
                return Ok(());
            };
            let Some(image) = document.transformed_image() else {
                self.clear_confirmation();
                return Err("Nothing to save".to_owned());
            };
            self.start_save(ctx, image, document.source_metadata_cloned(), &path)?;
        } else if cancelled {
            self.clear_confirmation();
        }

        Ok(())
    }

    /// Polls the background save worker and reports completed outcomes.
    pub(in crate::gui) fn poll_pending(&mut self, ctx: &egui::Context) -> SavePoll {
        let Some(task) = self.pending_save.as_mut() else {
            return SavePoll::None;
        };

        let outcome = match task.rx.try_recv() {
            Ok(done) => Some(done),
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(33));
                None
            }
            Err(TryRecvError::Disconnected) => {
                self.pending_save = None;
                return SavePoll::Failed("Save failed: worker disconnected".to_owned());
            }
        };

        let Some(done) = outcome else {
            return SavePoll::None;
        };

        self.pending_save = None;
        self.clear_confirmation();

        match done.result {
            Ok(()) => SavePoll::Saved {
                path: done.path,
                format: done.format,
                header: done.header,
            },
            Err(err) => SavePoll::Failed(format!("Save failed: {err}")),
        }
    }

    /// Evaluates warnings for a target path and either asks for confirmation or starts saving.
    fn save_to_path(
        &mut self,
        ctx: &egui::Context,
        document: &DocumentState,
        detected: Option<&StegInfo>,
        path: &Path,
    ) -> Result<(), String> {
        if self.pending_save.is_some() {
            return Err("A save operation is already in progress".to_owned());
        }

        let Some(image) = document.transformed_image() else {
            return Err("Nothing to save".to_owned());
        };

        let reasons = warning_reasons(
            image,
            self.save_format,
            self.save_header_version,
            document.source_metadata(),
            detected,
        );

        if !reasons.is_empty() {
            self.save_confirm_pending = Some(path.to_path_buf());
            self.save_confirm_reason = Some(reasons.join("\n"));
            return Ok(());
        }

        self.start_save(ctx, image, document.source_metadata_cloned(), path)
    }

    /// Starts the asynchronous save worker for the provided image snapshot.
    fn start_save(
        &mut self,
        ctx: &egui::Context,
        image: &DecodedImage,
        source_metadata: Option<bmp::runtime::encode::SourceMetadata>,
        path: &Path,
    ) -> Result<(), String> {
        if self.pending_save.is_some() {
            return Err("A save operation is already in progress".to_owned());
        }

        let image = image.clone();
        let format = self.save_format;
        let header = self.save_header_version;
        let save_path = path.to_path_buf();
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let result =
                bmp::runtime::encode::save_bmp_ext(&save_path, &image, format, header, source_metadata.as_ref())
                    .map_err(|e| e.to_string());
            let _ = tx.send(SaveTaskResult {
                path: save_path,
                format,
                header,
                result,
            });
        });

        self.pending_save = Some(PendingSaveTask { rx });
        self.clear_confirmation();
        ctx.request_repaint();
        Ok(())
    }
}
