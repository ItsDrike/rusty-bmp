use eframe::egui;

use bmp::runtime::transform::{ImageTransform, RotationInterpolation};

use crate::BmpViewerApp;

impl BmpViewerApp {
    pub(crate) fn open_resize_window(&mut self) {
        if let Some(img) = self.transformed_image.as_ref() {
            self.resize_width_input = img.width.to_string();
            self.resize_height_input = img.height.to_string();
            self.resize_open = true;
        } else {
            self.status = "Load an image first".to_owned();
        }
    }

    pub(crate) fn show_resize_window(&mut self, ctx: &egui::Context) -> Option<ImageTransform> {
        if !self.resize_open {
            return None;
        }

        let Some(current) = self.transformed_image.as_ref() else {
            self.resize_open = false;
            return None;
        };

        let mut open = self.resize_open;
        let mut apply = false;
        let mut close_requested = false;

        egui::Window::new("Resize Image")
            .open(&mut open)
            .resizable(false)
            .default_width(320.0)
            .show(ctx, |ui| {
                ui.label(format!("Current size: {}x{}", current.width, current.height));
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label("Width:");
                    let width_resp =
                        ui.add(egui::TextEdit::singleline(&mut self.resize_width_input).desired_width(80.0));
                    ui.label("Height:");
                    let height_resp =
                        ui.add(egui::TextEdit::singleline(&mut self.resize_height_input).desired_width(80.0));

                    if self.resize_keep_aspect
                        && width_resp.changed()
                        && !height_resp.has_focus()
                        && let Ok(w) = self.resize_width_input.trim().parse::<u32>()
                        && w > 0
                    {
                        let ratio = current.height as f32 / current.width as f32;
                        let h = ((w as f32 * ratio).round().max(1.0)) as u32;
                        self.resize_height_input = h.to_string();
                    }

                    if self.resize_keep_aspect
                        && height_resp.changed()
                        && !width_resp.has_focus()
                        && let Ok(h) = self.resize_height_input.trim().parse::<u32>()
                        && h > 0
                    {
                        let ratio = current.width as f32 / current.height as f32;
                        let w = ((h as f32 * ratio).round().max(1.0)) as u32;
                        self.resize_width_input = w.to_string();
                    }
                });

                ui.horizontal(|ui| {
                    let keep_aspect_resp = ui.checkbox(&mut self.resize_keep_aspect, "Keep aspect ratio");
                    if self.resize_keep_aspect
                        && keep_aspect_resp.changed()
                        && let Ok(w) = self.resize_width_input.trim().parse::<u32>()
                        && w > 0
                    {
                        let ratio = current.height as f32 / current.width as f32;
                        let h = ((w as f32 * ratio).round().max(1.0)) as u32;
                        self.resize_height_input = h.to_string();
                    }
                    ui.label("Interpolation:");
                    egui::ComboBox::from_id_salt("resize_interp")
                        .selected_text(self.resize_interpolation.to_string())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.resize_interpolation,
                                RotationInterpolation::Nearest,
                                RotationInterpolation::Nearest.to_string(),
                            );
                            ui.selectable_value(
                                &mut self.resize_interpolation,
                                RotationInterpolation::Bilinear,
                                RotationInterpolation::Bilinear.to_string(),
                            );
                            ui.selectable_value(
                                &mut self.resize_interpolation,
                                RotationInterpolation::Bicubic,
                                RotationInterpolation::Bicubic.to_string(),
                            );
                        });
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.small_button("50%").clicked() {
                        self.resize_width_input = ((current.width as f32 * 0.5).round().max(1.0) as u32).to_string();
                        self.resize_height_input = ((current.height as f32 * 0.5).round().max(1.0) as u32).to_string();
                    }
                    if ui.small_button("200%").clicked() {
                        self.resize_width_input = ((current.width as f32 * 2.0).round().max(1.0) as u32).to_string();
                        self.resize_height_input = ((current.height as f32 * 2.0).round().max(1.0) as u32).to_string();
                    }
                    if ui.small_button("Reset").clicked() {
                        self.resize_width_input = current.width.to_string();
                        self.resize_height_input = current.height.to_string();
                    }
                });

                let validation = validate_resize_inputs(
                    &self.resize_width_input,
                    &self.resize_height_input,
                    current.width,
                    current.height,
                    self.resize_keep_aspect,
                );

                ui.add_space(6.0);
                match &validation {
                    Ok((w, h)) => {
                        ui.colored_label(egui::Color32::GREEN, format!("Target: {}x{}", w, h));
                    }
                    Err(msg) => {
                        ui.colored_label(egui::Color32::RED, msg.to_string());
                    }
                }

                ui.horizontal(|ui| {
                    if ui.add_enabled(validation.is_ok(), egui::Button::new("Apply")).clicked() {
                        apply = true;
                    }
                    if ui.button("Close").clicked() {
                        close_requested = true;
                    }
                });
            });

        self.resize_open = open && !close_requested;

        if !apply {
            return None;
        }

        let Ok((width, height)) = validate_resize_inputs(
            &self.resize_width_input,
            &self.resize_height_input,
            current.width,
            current.height,
            self.resize_keep_aspect,
        ) else {
            return None;
        };

        Some(ImageTransform::Resize {
            width,
            height,
            interpolation: self.resize_interpolation,
        })
    }
}

fn validate_resize_inputs(
    width_input: &str,
    height_input: &str,
    cur_w: u32,
    cur_h: u32,
    keep_aspect: bool,
) -> Result<(u32, u32), &'static str> {
    let w = width_input.trim().parse::<u32>().map_err(|_| "Invalid width")?;
    let mut h = height_input.trim().parse::<u32>().map_err(|_| "Invalid height")?;

    if w == 0 || h == 0 {
        return Err("Width and height must be at least 1");
    }

    if keep_aspect && cur_w > 0 && cur_h > 0 {
        let current_ratio = cur_h as f32 / cur_w as f32;
        h = ((w as f32 * current_ratio).round().max(1.0)) as u32;
        // Reflect auto-adjusted value by returning it.
    }

    // Safety cap against accidental gigantic allocations.
    const MAX_DIM: u32 = 16_384;
    if w > MAX_DIM || h > MAX_DIM {
        return Err("Dimensions too large (max 16384)");
    }

    Ok((w, h))
}
