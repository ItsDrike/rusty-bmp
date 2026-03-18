//! Resize tool state and resize dialog UI.

use eframe::egui;

use bmp::runtime::transform::{ImageTransform, Resize, RotationInterpolation};

use crate::gui::BmpViewerApp;

use super::math::{scaled_dim, scaled_dim_by_factor};

/// State for resize dialog.
pub(in crate::gui) struct ResizeToolState {
    /// Whether the resize window is open.
    pub(in crate::gui) open: bool,
    /// Target width input for resize dialog.
    pub(in crate::gui) width_input: String,
    /// Target height input for resize dialog.
    pub(in crate::gui) height_input: String,
    /// Keep source aspect ratio when applying resize.
    pub(in crate::gui) keep_aspect: bool,
    /// Interpolation method for resize.
    pub(in crate::gui) interpolation: RotationInterpolation,
}

impl ResizeToolState {
    /// Initializes the resize dialog inputs from the current image dimensions.
    pub(in crate::gui) fn open_for_image(&mut self, width: u32, height: u32) {
        self.width_input = width.to_string();
        self.height_input = height.to_string();
        self.open = true;
    }

    /// Parses and validates the dialog inputs against the current image size.
    fn validated_dims(&self, cur_w: u32, cur_h: u32) -> Result<(u32, u32), &'static str> {
        validate_resize_inputs(&self.width_input, &self.height_input, cur_w, cur_h, self.keep_aspect)
    }
}

impl BmpViewerApp {
    /// Renders the resize window and returns a resize transform when applied.
    pub(in crate::gui) fn show_resize_window(&mut self, ctx: &egui::Context) -> Option<ImageTransform> {
        if !self.transforms.resize.open {
            return None;
        }

        let Some(current) = self.document.transformed_image() else {
            self.transforms.resize.open = false;
            return None;
        };

        let mut open = self.transforms.resize.open;
        let mut apply = false;
        let mut close_requested = false;

        egui::Window::new("Resize Image")
            .open(&mut open)
            .resizable(false)
            .default_width(320.0)
            .show(ctx, |ui| {
                ui.label(format!("Current size: {}x{}", current.width(), current.height()));
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label("Width:");
                    let width_resp = ui
                        .add(egui::TextEdit::singleline(&mut self.transforms.resize.width_input).desired_width(80.0));
                    ui.label("Height:");
                    let height_resp = ui
                        .add(egui::TextEdit::singleline(&mut self.transforms.resize.height_input).desired_width(80.0));

                    if self.transforms.resize.keep_aspect
                        && width_resp.changed()
                        && !height_resp.has_focus()
                        && let Ok(w) = self.transforms.resize.width_input.trim().parse::<u32>()
                        && w > 0
                    {
                        let h = scaled_dim(w, current.height(), current.width());
                        self.transforms.resize.height_input = h.to_string();
                    }

                    if self.transforms.resize.keep_aspect
                        && height_resp.changed()
                        && !width_resp.has_focus()
                        && let Ok(h) = self.transforms.resize.height_input.trim().parse::<u32>()
                        && h > 0
                    {
                        let w = scaled_dim(h, current.width(), current.height());
                        self.transforms.resize.width_input = w.to_string();
                    }
                });

                ui.horizontal(|ui| {
                    let keep_aspect_resp = ui.checkbox(&mut self.transforms.resize.keep_aspect, "Keep aspect ratio");
                    if self.transforms.resize.keep_aspect
                        && keep_aspect_resp.changed()
                        && let Ok(w) = self.transforms.resize.width_input.trim().parse::<u32>()
                        && w > 0
                    {
                        let h = scaled_dim(w, current.height(), current.width());
                        self.transforms.resize.height_input = h.to_string();
                    }
                    ui.label("Interpolation:");
                    egui::ComboBox::from_id_salt("resize_interp")
                        .selected_text(self.transforms.resize.interpolation.to_string())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.transforms.resize.interpolation,
                                RotationInterpolation::Nearest,
                                RotationInterpolation::Nearest.to_string(),
                            );
                            ui.selectable_value(
                                &mut self.transforms.resize.interpolation,
                                RotationInterpolation::Bilinear,
                                RotationInterpolation::Bilinear.to_string(),
                            );
                            ui.selectable_value(
                                &mut self.transforms.resize.interpolation,
                                RotationInterpolation::Bicubic,
                                RotationInterpolation::Bicubic.to_string(),
                            );
                        });
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.small_button("50%").clicked() {
                        let half_width = scaled_dim_by_factor(current.width(), 0.5);
                        let half_height = scaled_dim_by_factor(current.height(), 0.5);
                        self.transforms.resize.width_input = half_width.to_string();
                        self.transforms.resize.height_input = half_height.to_string();
                    }
                    if ui.small_button("200%").clicked() {
                        let double_width = scaled_dim_by_factor(current.width(), 2.0);
                        let double_height = scaled_dim_by_factor(current.height(), 2.0);
                        self.transforms.resize.width_input = double_width.to_string();
                        self.transforms.resize.height_input = double_height.to_string();
                    }
                    if ui.small_button("Reset").clicked() {
                        self.transforms.resize.width_input = current.width().to_string();
                        self.transforms.resize.height_input = current.height().to_string();
                    }
                });

                let validation = self.transforms.resize.validated_dims(current.width(), current.height());

                ui.add_space(6.0);
                match &validation {
                    Ok((w, h)) => {
                        ui.colored_label(egui::Color32::GREEN, format!("Target: {w}x{h}"));
                    }
                    Err(msg) => {
                        ui.colored_label(egui::Color32::RED, (*msg).to_owned());
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

        self.transforms.resize.open = open && !close_requested;

        if !apply {
            return None;
        }

        let Ok((width, height)) = self.transforms.resize.validated_dims(current.width(), current.height()) else {
            return None;
        };

        let resize = match Resize::try_new(width, height, self.transforms.resize.interpolation) {
            Ok(resize) => resize,
            Err(err) => {
                self.status = format!("Invalid resize settings: {err}");
                return None;
            }
        };

        Some(resize.into())
    }
}

/// Validates resize text inputs and normalizes them into concrete target dimensions.
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
        h = scaled_dim(w, cur_h, cur_w);
    }

    // Safety cap against accidental gigantic allocations.
    const MAX_DIM: u32 = 16_384;
    if w > MAX_DIM || h > MAX_DIM {
        return Err("Dimensions too large (max 16384)");
    }

    Ok((w, h))
}
