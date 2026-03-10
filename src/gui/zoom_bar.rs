use eframe::egui;

use crate::BmpViewerApp;

impl BmpViewerApp {
    /// Renders the bottom zoom status bar: zoom percentage, pixel info, Fit/1:1 buttons.
    ///
    /// Only shown when a texture is loaded.
    pub(crate) fn show_zoom_bar(&mut self, ctx: &egui::Context) {
        if self.texture.is_none() {
            return;
        }

        egui::TopBottomPanel::bottom("zoom_bar")
            .exact_height(24.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    // Zoom label on the left.
                    let zoom_label = if self.zoom == 0.0 {
                        format!("{:.0}% (Fit)", self.last_effective_zoom * 100.0)
                    } else {
                        format!("{:.0}%", self.zoom * 100.0)
                    };
                    ui.monospace(&zoom_label);

                    // Pixel info (from previous frame's hovered_pixel).
                    if let Some((px, py, rgba)) = self.hovered_pixel {
                        ui.separator();
                        ui.monospace(format!(
                            "({px}, {py})  RGBA({}, {}, {}, {})",
                            rgba[0], rgba[1], rgba[2], rgba[3]
                        ));
                        // Small color swatch.
                        let color = egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
                        let (swatch_rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                        ui.painter().rect_filled(swatch_rect, 2.0, color);
                    }

                    // Push buttons to the right.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let is_1to1 = self.zoom == 1.0;
                        if ui
                            .add_enabled(!is_1to1, egui::Button::new("1:1").small())
                            .on_hover_text("Actual pixel size (1)")
                            .clicked()
                        {
                            self.zoom = 1.0;
                            self.pan_offset = egui::Vec2::ZERO;
                        }

                        let is_fit = self.zoom == 0.0;
                        if ui
                            .add_enabled(!is_fit, egui::Button::new("Fit").small())
                            .on_hover_text("Fit image to panel (0)")
                            .clicked()
                        {
                            self.zoom = 0.0;
                            self.pan_offset = egui::Vec2::ZERO;
                        }
                    });
                });
            });
    }
}
