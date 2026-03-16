use eframe::egui;

use bmp::runtime::transform::{ImageTransform, RotateAny, RotationInterpolation};

use crate::BmpViewerApp;

impl BmpViewerApp {
    pub(crate) fn show_rotate_any_window(&mut self, ctx: &egui::Context) -> Option<ImageTransform> {
        if !self.transforms.rotate.open {
            return None;
        }

        let mut open = self.transforms.rotate.open;
        let mut apply = false;
        let mut close_requested = false;

        egui::Window::new("Arbitrary Rotation")
            .open(&mut open)
            .resizable(false)
            .default_width(320.0)
            .show(ctx, |ui| {
                ui.label("Rotate around image center.");
                ui.add_space(6.0);

                ui.add(egui::Slider::new(&mut self.transforms.rotate.angle, -180.0..=180.0).text("Angle (deg)"));

                ui.horizontal(|ui| {
                    ui.label("Interpolation:");
                    egui::ComboBox::from_id_salt("rotate_any_interp")
                        .selected_text(self.transforms.rotate.interpolation.to_string())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.transforms.rotate.interpolation,
                                RotationInterpolation::Nearest,
                                RotationInterpolation::Nearest.to_string(),
                            );
                            ui.selectable_value(
                                &mut self.transforms.rotate.interpolation,
                                RotationInterpolation::Bilinear,
                                RotationInterpolation::Bilinear.to_string(),
                            );
                            ui.selectable_value(
                                &mut self.transforms.rotate.interpolation,
                                RotationInterpolation::Bicubic,
                                RotationInterpolation::Bicubic.to_string(),
                            );
                        });
                });

                ui.checkbox(&mut self.transforms.rotate.expand, "Expand canvas to fit full image");

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.small_button("-15 deg").clicked() {
                        self.transforms.rotate.angle -= 15.0;
                    }
                    if ui.small_button("+15 deg").clicked() {
                        self.transforms.rotate.angle += 15.0;
                    }
                    if ui.small_button("-45 deg").clicked() {
                        self.transforms.rotate.angle -= 45.0;
                    }
                    if ui.small_button("+45 deg").clicked() {
                        self.transforms.rotate.angle += 45.0;
                    }
                    if ui.small_button("Reset").clicked() {
                        self.transforms.rotate.angle = 0.0;
                    }
                });

                // Keep editor value bounded.
                self.transforms.rotate.angle = self.transforms.rotate.angle.clamp(-3600.0, 3600.0);

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        apply = true;
                    }
                    if ui.button("Close").clicked() {
                        close_requested = true;
                    }
                });
            });

        self.transforms.rotate.open = open && !close_requested;

        if !apply {
            return None;
        }

        #[allow(clippy::cast_possible_truncation)]
        let angle_tenths = (self.transforms.rotate.angle * 10.0).round().clamp(-36000.0, 36000.0) as i32;
        Some(
            RotateAny {
                angle_tenths,
                interpolation: self.transforms.rotate.interpolation,
                expand: self.transforms.rotate.expand,
            }
            .into(),
        )
    }
}
