use eframe::egui;
use eframe::egui::color_picker;

use bmp::runtime::transform::{ImageTransform, Translate, TranslateMode};

use crate::BmpViewerApp;

impl BmpViewerApp {
    pub(crate) fn show_translate_window(&mut self, ctx: &egui::Context) -> Option<ImageTransform> {
        if !self.transforms.translate.open {
            return None;
        }

        let Some(current) = self.document.transformed_image() else {
            self.transforms.translate.open = false;
            return None;
        };

        let mut open = self.transforms.translate.open;
        let mut apply = false;
        let mut close_requested = false;

        egui::Window::new("Translate / Shift")
            .open(&mut open)
            .resizable(false)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.label(format!("Current size: {}x{}", current.width(), current.height()));
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label("dx:");
                    ui.add(egui::DragValue::new(&mut self.transforms.translate.dx).speed(1));
                    ui.label("dy:");
                    ui.add(egui::DragValue::new(&mut self.transforms.translate.dy).speed(1));
                });

                ui.horizontal(|ui| {
                    if ui.small_button("dx -10").clicked() {
                        self.transforms.translate.dx -= 10;
                    }
                    if ui.small_button("dx +10").clicked() {
                        self.transforms.translate.dx += 10;
                    }
                    if ui.small_button("dy -10").clicked() {
                        self.transforms.translate.dy -= 10;
                    }
                    if ui.small_button("dy +10").clicked() {
                        self.transforms.translate.dy += 10;
                    }
                    if ui.small_button("Reset").clicked() {
                        self.transforms.translate.dx = 0;
                        self.transforms.translate.dy = 0;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Mode:");
                    egui::ComboBox::from_id_salt("translate_mode")
                        .selected_text(self.transforms.translate.mode.to_string())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.transforms.translate.mode, TranslateMode::Crop, "Crop");
                            ui.selectable_value(&mut self.transforms.translate.mode, TranslateMode::Expand, "Expand");
                        });
                });

                ui.horizontal(|ui| {
                    ui.label("Fill:");
                    let prev_fill = self.transforms.translate.fill;
                    let mut color = egui::Color32::from_rgba_unmultiplied(
                        self.transforms.translate.fill[0],
                        self.transforms.translate.fill[1],
                        self.transforms.translate.fill[2],
                        self.transforms.translate.fill[3],
                    );
                    let response =
                        color_picker::color_edit_button_srgba(ui, &mut color, color_picker::Alpha::OnlyBlend);
                    let mut next_fill = [color.r(), color.g(), color.b(), color.a()];
                    // If user opens the picker while still on default transparent fill,
                    // promote alpha so color changes are immediately visible.
                    if prev_fill == [0, 0, 0, 0] && response.clicked() {
                        next_fill[3] = 255;
                    }
                    self.transforms.translate.fill = next_fill;

                    if ui.small_button("Transparent").clicked() {
                        self.transforms.translate.fill = [0, 0, 0, 0];
                    }
                    if ui.small_button("Black").clicked() {
                        self.transforms.translate.fill = [0, 0, 0, 255];
                    }
                    if ui.small_button("White").clicked() {
                        self.transforms.translate.fill = [255, 255, 255, 255];
                    }
                });

                if matches!(self.transforms.translate.mode, TranslateMode::Expand) {
                    let new_w = current.width() + self.transforms.translate.dx.unsigned_abs();
                    let new_h = current.height() + self.transforms.translate.dy.unsigned_abs();
                    ui.small(format!("Output size: {new_w}x{new_h}"));
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

        self.transforms.translate.open = open && !close_requested;

        if !apply {
            return None;
        }

        Some(
            Translate {
                dx: self.transforms.translate.dx,
                dy: self.transforms.translate.dy,
                mode: self.transforms.translate.mode,
                fill: self.transforms.translate.fill,
            }
            .into(),
        )
    }
}
