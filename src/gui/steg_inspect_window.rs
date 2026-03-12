use eframe::egui;

use bmp::runtime::{steganography, transform::ImageTransform};

use crate::BmpViewerApp;

impl BmpViewerApp {
    pub(crate) fn show_steg_inspect_window(&mut self, ctx: &egui::Context) -> Option<ImageTransform> {
        if !self.steg_inspect_open {
            return None;
        }

        let mut open = self.steg_inspect_open;
        let mut close_requested = false;
        let mut do_extract = false;
        let mut do_remove = false;

        egui::Window::new("Inspect Steganography")
            .open(&mut open)
            .resizable(false)
            .default_width(380.0)
            .show(ctx, |ui| {
                match &self.steg_detected {
                    None => {
                        ui.colored_label(egui::Color32::DARK_GRAY, "No steganography detected in this image.");
                    }
                    Some(info) => {
                        // ── Header info ────────────────────────────────────
                        ui.label("Detected steganography header:");
                        egui::Grid::new("steg_inspect_grid")
                            .num_columns(2)
                            .spacing([12.0, 2.0])
                            .show(ui, |ui| {
                                ui.label("Version:");
                                ui.monospace(info.version.to_string());
                                ui.end_row();

                                ui.label("Payload size:");
                                ui.monospace(format!("{} bytes", info.payload_len));
                                ui.end_row();

                                ui.label("Red bits:");
                                ui.monospace(info.config.r_bits.to_string());
                                ui.end_row();

                                ui.label("Green bits:");
                                ui.monospace(info.config.g_bits.to_string());
                                ui.end_row();

                                ui.label("Blue bits:");
                                ui.monospace(info.config.b_bits.to_string());
                                ui.end_row();

                                ui.label("Alpha bits:");
                                ui.monospace(info.config.a_bits.to_string());
                                ui.end_row();
                            });

                        ui.add_space(6.0);

                        // ── Extract payload ────────────────────────────────
                        ui.horizontal(|ui| {
                            if ui.button("Extract Payload").clicked() {
                                do_extract = true;
                            }
                            if ui
                                .button("Remove Steganography")
                                .on_hover_text(
                                    "Zero all LSBs used by this configuration, \
                                     removing the hidden payload from the image.",
                                )
                                .clicked()
                            {
                                do_remove = true;
                            }
                        });

                        // ── Extraction result ──────────────────────────────
                        if let Some(result) = &self.steg_extracted {
                            ui.add_space(6.0);
                            match result {
                                Err(msg) => {
                                    ui.colored_label(egui::Color32::RED, format!("Error: {msg}"));
                                }
                                Ok(bytes) => {
                                    match std::str::from_utf8(bytes) {
                                        Ok(text) => {
                                            ui.label("Payload (UTF-8 text):");
                                            egui::ScrollArea::vertical()
                                                .id_salt("steg_inspect_text")
                                                .max_height(150.0)
                                                .show(ui, |ui| {
                                                    // Read-only text display.
                                                    let mut display = text.to_owned();
                                                    ui.add(
                                                        egui::TextEdit::multiline(&mut display)
                                                            .desired_width(f32::INFINITY)
                                                            .desired_rows(5)
                                                            .interactive(false),
                                                    );
                                                });

                                            if ui.button("Copy to Clipboard").clicked() {
                                                ctx.copy_text(text.to_owned());
                                            }
                                        }
                                        Err(_) => {
                                            // Not valid UTF-8: show a hex dump of the
                                            // first 256 bytes.
                                            ui.label(format!(
                                                "Payload (binary, {} bytes — showing first 256):",
                                                bytes.len()
                                            ));
                                            let hex: String = bytes
                                                .iter()
                                                .take(256)
                                                .enumerate()
                                                .map(|(i, b)| {
                                                    if i > 0 && i % 16 == 0 {
                                                        format!("\n{b:02X}")
                                                    } else if i > 0 {
                                                        format!(" {b:02X}")
                                                    } else {
                                                        format!("{b:02X}")
                                                    }
                                                })
                                                .collect();
                                            egui::ScrollArea::vertical()
                                                .id_salt("steg_inspect_hex")
                                                .max_height(120.0)
                                                .show(ui, |ui| {
                                                    ui.monospace(&hex);
                                                });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                ui.add_space(6.0);
                if ui.button("Close").clicked() {
                    close_requested = true;
                }
            });

        self.steg_inspect_open = open && !close_requested;

        // Handle extract: do it here while we still have &mut self, then store
        // the result in steg_extracted.
        if do_extract {
            if let (Some(img), Some(info)) = (&self.transformed_image, &self.steg_detected) {
                let result = steganography::extract(img, info).map_err(|e| e.to_string());
                self.steg_extracted = Some(result);
            }
        }

        if do_remove {
            if let Some(info) = self.steg_detected.as_ref() {
                return Some(ImageTransform::RemoveSteganography { config: info.config });
            }
        }

        None
    }
}
