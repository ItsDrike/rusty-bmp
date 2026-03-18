//! Bottom status bar for zoom controls and hovered-pixel details.

use eframe::egui;

use super::ZoomMode;
use crate::gui::BmpViewerApp;

impl BmpViewerApp {
    /// Renders the bottom zoom status bar: zoom percentage, pixel info, Fit/1:1 buttons.
    ///
    /// Only shown when a texture is loaded.
    pub(in crate::gui) fn show_zoom_bar(&mut self, ctx: &egui::Context) {
        if self.viewport.texture.is_none() {
            return;
        }

        egui::TopBottomPanel::bottom("zoom_bar")
            .exact_height(24.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    // Zoom label on the left.
                    let zoom_label = match self.viewport.zoom {
                        ZoomMode::Fit => format!("{:.0}% (Fit)", self.viewport.last_effective_zoom * 100.0),
                        ZoomMode::Scale(scale) => format!("{:.0}%", scale * 100.0),
                    };
                    ui.monospace(&zoom_label);

                    // Pixel info (from previous frame's hovered_pixel).
                    if let Some((px, py, rgba)) = self.viewport.hovered_pixel {
                        ui.separator();
                        ui.monospace(format!(
                            "({px}, {py})  RGBA({}, {}, {}, {})",
                            rgba[0], rgba[1], rgba[2], rgba[3]
                        ));
                        // Small color swatch.
                        let color = egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
                        let (swatch_rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                        ui.painter().rect_filled(swatch_rect, 2.0, color);
                    } else if self.viewport.last_effective_zoom < 8.0 {
                        ui.separator();
                        ui.label(egui::RichText::new("Zoom in to inspect pixel values").weak().italics());
                    }

                    // Push buttons to the right.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let is_1to1 = matches!(
                            self.viewport.zoom,
                            ZoomMode::Scale(scale) if (scale - 1.0).abs() < f32::EPSILON
                        );
                        if ui
                            .add_enabled(!is_1to1, egui::Button::new("1:1").small())
                            .on_hover_text("Actual pixel size (1)")
                            .clicked()
                        {
                            self.viewport.zoom = ZoomMode::Scale(1.0);
                            self.viewport.pan_offset = egui::Vec2::ZERO;
                        }

                        let is_fit = matches!(self.viewport.zoom, ZoomMode::Fit);
                        if ui
                            .add_enabled(!is_fit, egui::Button::new("Fit").small())
                            .on_hover_text("Fit image to panel")
                            .clicked()
                        {
                            self.viewport.zoom = ZoomMode::Fit;
                            self.viewport.pan_offset = egui::Vec2::ZERO;
                        }
                    });
                });
            });
    }
}
