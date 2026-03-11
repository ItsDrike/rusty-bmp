use eframe::egui;
use eframe::egui::color_picker;

use bmp::runtime::transform::{ImageTransform, TranslateMode};

use crate::BmpViewerApp;

impl BmpViewerApp {
    pub(crate) fn show_translate_window(&mut self, ctx: &egui::Context) -> Option<ImageTransform> {
        if !self.translate_open {
            return None;
        }

        let Some(current) = self.transformed_image.as_ref() else {
            self.translate_open = false;
            return None;
        };

        let mut open = self.translate_open;
        let mut apply = false;
        let mut close_requested = false;

        egui::Window::new("Translate / Shift")
            .open(&mut open)
            .resizable(false)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.label(format!("Current size: {}x{}", current.width, current.height));
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label("dx:");
                    ui.add(egui::DragValue::new(&mut self.translate_dx).speed(1));
                    ui.label("dy:");
                    ui.add(egui::DragValue::new(&mut self.translate_dy).speed(1));
                });

                ui.horizontal(|ui| {
                    if ui.small_button("dx -10").clicked() {
                        self.translate_dx -= 10;
                    }
                    if ui.small_button("dx +10").clicked() {
                        self.translate_dx += 10;
                    }
                    if ui.small_button("dy -10").clicked() {
                        self.translate_dy -= 10;
                    }
                    if ui.small_button("dy +10").clicked() {
                        self.translate_dy += 10;
                    }
                    if ui.small_button("Reset").clicked() {
                        self.translate_dx = 0;
                        self.translate_dy = 0;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Mode:");
                    egui::ComboBox::from_id_salt("translate_mode")
                        .selected_text(self.translate_mode.to_string())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.translate_mode, TranslateMode::Crop, "Crop");
                            ui.selectable_value(&mut self.translate_mode, TranslateMode::Expand, "Expand");
                        });
                });

                ui.horizontal(|ui| {
                    ui.label("Fill:");
                    let prev_fill = self.translate_fill;
                    let mut color = egui::Color32::from_rgba_unmultiplied(
                        self.translate_fill[0],
                        self.translate_fill[1],
                        self.translate_fill[2],
                        self.translate_fill[3],
                    );
                    let response =
                        color_picker::color_edit_button_srgba(ui, &mut color, color_picker::Alpha::OnlyBlend);
                    let mut next_fill = [color.r(), color.g(), color.b(), color.a()];
                    // If user opens the picker while still on default transparent fill,
                    // promote alpha so color changes are immediately visible.
                    if prev_fill == [0, 0, 0, 0] && response.clicked() {
                        next_fill[3] = 255;
                    }
                    self.translate_fill = next_fill;

                    if ui.small_button("Transparent").clicked() {
                        self.translate_fill = [0, 0, 0, 0];
                    }
                    if ui.small_button("Black").clicked() {
                        self.translate_fill = [0, 0, 0, 255];
                    }
                    if ui.small_button("White").clicked() {
                        self.translate_fill = [255, 255, 255, 255];
                    }
                });

                if matches!(self.translate_mode, TranslateMode::Expand) {
                    let new_w = current.width + self.translate_dx.unsigned_abs();
                    let new_h = current.height + self.translate_dy.unsigned_abs();
                    ui.small(format!("Output size: {}x{}", new_w, new_h));
                }

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

        self.translate_open = open && !close_requested;

        if !apply {
            return None;
        }

        Some(ImageTransform::Translate {
            dx: self.translate_dx,
            dy: self.translate_dy,
            mode: self.translate_mode,
            fill: self.translate_fill,
        })
    }
}
