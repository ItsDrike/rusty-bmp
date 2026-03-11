use eframe::egui;

use bmp::runtime::transform::{ImageTransform, RotationInterpolation};

use crate::BmpViewerApp;

impl BmpViewerApp {
    pub(crate) fn show_skew_window(&mut self, ctx: &egui::Context) -> Option<ImageTransform> {
        if !self.skew_open {
            return None;
        }

        let mut open = self.skew_open;
        let mut apply = false;
        let mut close_requested = false;

        egui::Window::new("Skew / Shear")
            .open(&mut open)
            .resizable(false)
            .default_width(340.0)
            .show(ctx, |ui| {
                ui.label("Shear around image center (affine transform).");
                ui.add_space(6.0);

                ui.add(egui::Slider::new(&mut self.skew_x_percent, -100.0..=100.0).text("X shear (%)"));
                ui.add(egui::Slider::new(&mut self.skew_y_percent, -100.0..=100.0).text("Y shear (%)"));

                ui.horizontal(|ui| {
                    if ui.small_button("x +25%").clicked() {
                        self.skew_x_percent += 25.0;
                    }
                    if ui.small_button("x -25%").clicked() {
                        self.skew_x_percent -= 25.0;
                    }
                    if ui.small_button("y +25%").clicked() {
                        self.skew_y_percent += 25.0;
                    }
                    if ui.small_button("y -25%").clicked() {
                        self.skew_y_percent -= 25.0;
                    }
                    if ui.small_button("Reset").clicked() {
                        self.skew_x_percent = 0.0;
                        self.skew_y_percent = 0.0;
                    }
                });

                self.skew_x_percent = self.skew_x_percent.clamp(-100.0, 100.0);
                self.skew_y_percent = self.skew_y_percent.clamp(-100.0, 100.0);

                ui.horizontal(|ui| {
                    ui.label("Interpolation:");
                    egui::ComboBox::from_id_salt("skew_interp")
                        .selected_text(self.skew_interpolation.to_string())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.skew_interpolation,
                                RotationInterpolation::Nearest,
                                RotationInterpolation::Nearest.to_string(),
                            );
                            ui.selectable_value(
                                &mut self.skew_interpolation,
                                RotationInterpolation::Bilinear,
                                RotationInterpolation::Bilinear.to_string(),
                            );
                            ui.selectable_value(
                                &mut self.skew_interpolation,
                                RotationInterpolation::Bicubic,
                                RotationInterpolation::Bicubic.to_string(),
                            );
                        });
                });

                ui.checkbox(&mut self.skew_expand, "Expand canvas to fit full image");

                let kx = self.skew_x_percent / 100.0;
                let ky = self.skew_y_percent / 100.0;
                let det = 1.0 - kx * ky;

                ui.add_space(6.0);
                if det.abs() < 1e-4 {
                    ui.colored_label(
                        egui::Color32::RED,
                        "Invalid parameters: near-singular transform (1 - kx*ky ≈ 0)",
                    );
                } else {
                    ui.colored_label(
                        egui::Color32::GREEN,
                        format!("kx={:+.3}, ky={:+.3}, det={:+.3}", kx, ky, det),
                    );
                }

                ui.horizontal(|ui| {
                    if ui.add_enabled(det.abs() >= 1e-4, egui::Button::new("Apply")).clicked() {
                        apply = true;
                    }
                    if ui.button("Close").clicked() {
                        close_requested = true;
                    }
                });
            });

        self.skew_open = open && !close_requested;

        if !apply {
            return None;
        }

        let x_milli = (self.skew_x_percent * 10.0).round().clamp(-1000.0, 1000.0) as i16;
        let y_milli = (self.skew_y_percent * 10.0).round().clamp(-1000.0, 1000.0) as i16;
        Some(ImageTransform::Skew {
            x_milli,
            y_milli,
            interpolation: self.skew_interpolation,
            expand: self.skew_expand,
        })
    }
}
