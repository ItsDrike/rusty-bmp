use eframe::egui;

use bmp::runtime::{steganography::StegConfig, transform::ImageTransform};

use crate::BmpViewerApp;

impl BmpViewerApp {
    pub(crate) fn show_steg_embed_window(&mut self, ctx: &egui::Context) -> Option<ImageTransform> {
        if !self.steg_embed_open {
            return None;
        }

        let mut open = self.steg_embed_open;
        let mut apply = false;
        let mut close_requested = false;

        egui::Window::new("Embed Steganography Data")
            .open(&mut open)
            .resizable(false)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.label("Embed arbitrary UTF-8 text into the image LSBs.");
                ui.add_space(4.0);

                // Channel configuration
                ui.label("Bits per channel (0 = skip channel):");
                egui::Grid::new("steg_channel_grid")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Red:");
                        ui.add(egui::Slider::new(&mut self.steg_r_bits, 0u8..=8).suffix(" bits"));
                        ui.end_row();

                        ui.label("Green:");
                        ui.add(egui::Slider::new(&mut self.steg_g_bits, 0u8..=8).suffix(" bits"));
                        ui.end_row();

                        ui.label("Blue:");
                        ui.add(egui::Slider::new(&mut self.steg_b_bits, 0u8..=8).suffix(" bits"));
                        ui.end_row();

                        ui.label("Alpha:");
                        ui.add(egui::Slider::new(&mut self.steg_a_bits, 0u8..=8).suffix(" bits"));
                        ui.end_row();
                    });

                ui.add_space(4.0);

                // Capacity indicator
                let config = StegConfig {
                    r_bits: self.steg_r_bits,
                    g_bits: self.steg_g_bits,
                    b_bits: self.steg_b_bits,
                    a_bits: self.steg_a_bits,
                };

                let (capacity_bytes, payload_bytes) = if let Some(img) = &self.transformed_image {
                    let cap = config.capacity_bytes(img.width, img.height);
                    let payload = self.steg_text_input.len() as u64;
                    (Some(cap), payload)
                } else {
                    (None, 0)
                };

                let no_channels = config.bits_per_pixel() == 0;

                match capacity_bytes {
                    None => {
                        ui.colored_label(egui::Color32::DARK_GRAY, "No image loaded.");
                    }
                    Some(_) if no_channels => {
                        ui.colored_label(egui::Color32::RED, "No channels selected - cannot embed.");
                    }
                    Some(cap) => {
                        let (color, label) = if payload_bytes > cap {
                            (
                                egui::Color32::RED,
                                format!("Capacity: {cap} bytes - payload too large ({payload_bytes} bytes)"),
                            )
                        } else {
                            (
                                egui::Color32::from_rgb(80, 200, 120),
                                format!("Capacity: {cap} bytes (payload: {payload_bytes} bytes)"),
                            )
                        };
                        ui.colored_label(color, label);
                    }
                }

                ui.add_space(6.0);

                // Text input
                ui.label("Text to embed (UTF-8):");
                egui::ScrollArea::vertical()
                    .id_salt("steg_embed_text")
                    .max_height(120.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.steg_text_input)
                                .desired_width(f32::INFINITY)
                                .desired_rows(5),
                        );
                    });

                ui.add_space(8.0);

                // Validation and apply
                let valid = !no_channels
                    && capacity_bytes.is_some()
                    && !self.steg_text_input.is_empty()
                    && capacity_bytes.is_some_and(|c| payload_bytes <= c);

                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(valid, egui::Button::new("Embed"))
                        .on_disabled_hover_text(if no_channels {
                            "Select at least one channel"
                        } else if self.steg_text_input.is_empty() {
                            "Enter text to embed"
                        } else if capacity_bytes.is_none() {
                            "No image loaded"
                        } else {
                            "Payload exceeds capacity"
                        })
                        .clicked()
                    {
                        apply = true;
                    }
                    if ui.button("Close").clicked() {
                        close_requested = true;
                    }
                });
            });

        self.steg_embed_open = open && !close_requested;

        if !apply {
            return None;
        }

        let config = StegConfig {
            r_bits: self.steg_r_bits,
            g_bits: self.steg_g_bits,
            b_bits: self.steg_b_bits,
            a_bits: self.steg_a_bits,
        };
        let payload = self.steg_text_input.as_bytes().to_vec();

        Some(ImageTransform::EmbedSteganography { config, payload })
    }
}
