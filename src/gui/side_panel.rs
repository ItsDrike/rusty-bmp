use eframe::egui;

use bmp::runtime::transform::{ConvolutionFilter, ImageTransform};

use crate::BmpViewerApp;

/// Deferred actions from the side panel that need `&mut self` after the closure returns.
pub(crate) struct SidePanelActions {
    pub remove_transform: Option<usize>,
    pub do_undo: bool,
    pub do_redo: bool,
    pub do_clear: bool,
    pub apply_op: Option<ImageTransform>,
    pub open_rotate_any: bool,
    pub open_resize: bool,
    pub open_skew: bool,
    pub open_translate: bool,
    pub open_crop: bool,
    pub open_custom_kernel: bool,
}

impl BmpViewerApp {
    /// Renders the right side panel: file info, decoded info, transforms, palette.
    ///
    /// Returns deferred actions (undo/redo/remove) that must be applied by the caller
    /// after this method returns, since the panel closure borrows `&self`.
    pub(crate) fn show_side_panel(&mut self, ctx: &egui::Context) -> SidePanelActions {
        let window_width = ctx.available_rect().width();
        let panel_max_width = (window_width - 220.0).clamp(220.0, 460.0);

        let mut remove_transform: Option<usize> = None;
        let mut do_undo = false;
        let mut do_redo = false;
        let mut do_clear = false;
        let mut apply_op: Option<ImageTransform> = None;
        let mut open_rotate_any = false;
        let mut open_resize = false;
        let mut open_skew = false;
        let mut open_translate = false;
        let mut open_crop = false;
        let mut open_custom_kernel = false;

        egui::SidePanel::right("bmp_info")
            .default_width(320.0)
            .width_range(220.0..=panel_max_width)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Inspector");
                ui.separator();
                egui::ScrollArea::vertical().id_salt("inspector_scroll").show(ui, |ui| {
                    egui::CollapsingHeader::new("Edit").default_open(true).show(ui, |ui| {
                        let has_image = self.transformed_image.is_some();
                        if !has_image {
                            ui.label("Load an image to enable editing tools.");
                            return;
                        }

                        ui.label("Geometry");
                        ui.horizontal(|ui| {
                            ui.small("Rotate:");
                            if ui.small_button("Left").clicked() {
                                apply_op = Some(ImageTransform::RotateLeft90);
                            }
                            if ui.small_button("Right").clicked() {
                                apply_op = Some(ImageTransform::RotateRight90);
                            }
                            if ui.small_button("Arbitrary...").clicked() {
                                open_rotate_any = true;
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.small("Mirror:");
                            if ui.small_button("Horizontal").clicked() {
                                apply_op = Some(ImageTransform::MirrorHorizontal);
                            }
                            if ui.small_button("Vertical").clicked() {
                                apply_op = Some(ImageTransform::MirrorVertical);
                            }
                        });
                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button("Resize...").clicked() {
                                open_resize = true;
                            }
                            if ui.small_button("Skew...").clicked() {
                                open_skew = true;
                            }
                            if ui.small_button("Translate...").clicked() {
                                open_translate = true;
                            }
                            if ui.small_button("Crop...").clicked() {
                                open_crop = true;
                            }
                        });

                        ui.add_space(6.0);
                        ui.label("Color");
                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button("Invert").clicked() {
                                apply_op = Some(ImageTransform::InvertColors);
                            }
                            if ui.small_button("Grayscale").clicked() {
                                apply_op = Some(ImageTransform::Grayscale);
                            }
                            if ui.small_button("Sepia").clicked() {
                                apply_op = Some(ImageTransform::Sepia);
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.small("Brightness:");
                            ui.add(egui::Slider::new(&mut self.brightness_input, -255..=255));
                            if ui
                                .add_enabled(self.brightness_input != 0, egui::Button::new("Apply").small())
                                .clicked()
                            {
                                apply_op = Some(ImageTransform::Brightness(self.brightness_input));
                                self.brightness_input = 0;
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.small("Contrast:");
                            ui.add(egui::Slider::new(&mut self.contrast_input, -255..=255));
                            if ui
                                .add_enabled(self.contrast_input != 0, egui::Button::new("Apply").small())
                                .clicked()
                            {
                                apply_op = Some(ImageTransform::Contrast(self.contrast_input));
                                self.contrast_input = 0;
                            }
                        });

                        ui.add_space(6.0);
                        ui.label("Convolution");
                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button("Blur").clicked() {
                                apply_op = Some(ImageTransform::Convolution(ConvolutionFilter::Blur));
                            }
                            if ui.small_button("Sharpen").clicked() {
                                apply_op = Some(ImageTransform::Convolution(ConvolutionFilter::Sharpen));
                            }
                            if ui.small_button("Edge").clicked() {
                                apply_op = Some(ImageTransform::Convolution(ConvolutionFilter::EdgeDetect));
                            }
                            if ui.small_button("Emboss").clicked() {
                                apply_op = Some(ImageTransform::Convolution(ConvolutionFilter::Emboss));
                            }
                            if ui.small_button("Custom...").clicked() {
                                open_custom_kernel = true;
                            }
                        });
                    });

                    egui::CollapsingHeader::new(if self.pipeline.is_empty() {
                        "Transforms History".to_owned()
                    } else {
                        format!("Transforms History ({})", self.pipeline.len())
                    })
                    .default_open(true)
                    .show(ui, |ui| {
                        let has_history = !self.pipeline.is_empty();
                        let has_redo = !self.redo_stack.is_empty();

                        if !has_history && !has_redo {
                            ui.label("Apply a transformation to see transform history.");
                        } else {
                            ui.horizontal(|ui| {
                                let can_undo = has_history;
                                let undo_tooltip = if let Some(op) = self.pipeline.ops().last() {
                                    format!("Undo {} (Ctrl+Z)", op)
                                } else {
                                    "Nothing to undo".to_owned()
                                };
                                if ui
                                    .add_enabled(can_undo, egui::Button::new("Undo").small())
                                    .on_hover_text(&undo_tooltip)
                                    .clicked()
                                {
                                    do_undo = true;
                                }

                                let can_redo = has_redo;
                                let redo_tooltip = if let Some(op) = self.redo_stack.last() {
                                    format!("Redo {} (Ctrl+Shift+Z)", op)
                                } else {
                                    "Nothing to redo".to_owned()
                                };
                                if ui
                                    .add_enabled(can_redo, egui::Button::new("Redo").small())
                                    .on_hover_text(&redo_tooltip)
                                    .clicked()
                                {
                                    do_redo = true;
                                }

                                let can_clear = has_history;
                                if ui
                                    .add_enabled(can_clear, egui::Button::new("Clear").small())
                                    .on_hover_text("Remove all transforms")
                                    .clicked()
                                {
                                    do_clear = true;
                                }
                            });

                            for (i, op) in self.pipeline.ops().iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.monospace(format!("{}.", i + 1));
                                    ui.label(op.to_string());
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui
                                            .small_button("\u{00d7}")
                                            .on_hover_text("Remove this transform")
                                            .clicked()
                                        {
                                            remove_transform = Some(i);
                                        }
                                    });
                                });
                            }
                        }
                    });

                    egui::CollapsingHeader::new("BMP Details")
                        .default_open(true)
                        .show(ui, |ui| {
                            if self.image_stats.is_empty() {
                                ui.label("Load a BMP file to inspect its metadata.");
                                return;
                            }

                            ui.label("File Info");
                            egui::ScrollArea::vertical()
                                .id_salt("file_info_scroll")
                                .max_height(220.0)
                                .show(ui, |ui| {
                                    ui.monospace(&self.image_stats);
                                });

                            ui.separator();
                            ui.label("Decoded Info");
                            egui::ScrollArea::vertical()
                                .id_salt("decoded_info_scroll")
                                .max_height(150.0)
                                .show(ui, |ui| {
                                    ui.monospace(&self.decoded_stats);
                                });
                        });

                    egui::CollapsingHeader::new("Color Palette")
                        .default_open(false)
                        .show(ui, |ui| {
                            if self.palette_colors.is_empty() {
                                ui.label("No palette available for this image.");
                            } else {
                                egui::ScrollArea::vertical()
                                    .id_salt("palette_scroll")
                                    .max_height(220.0)
                                    .show(ui, |ui| {
                                        self.render_palette_grid(ui);
                                    });
                            }
                        });
                });
            });

        SidePanelActions {
            remove_transform,
            do_undo,
            do_redo,
            do_clear,
            apply_op,
            open_rotate_any,
            open_resize,
            open_skew,
            open_translate,
            open_crop,
            open_custom_kernel,
        }
    }

    /// Applies deferred actions returned by [`show_side_panel`].
    pub(crate) fn apply_side_panel_actions(&mut self, ctx: &egui::Context, actions: SidePanelActions) {
        if let Some(op) = actions.apply_op {
            self.apply_and_refresh(ctx, op);
        }
        if actions.open_rotate_any {
            self.rotate_any_open = true;
        }
        if actions.open_resize {
            self.open_resize_window();
        }
        if actions.open_skew {
            self.skew_open = true;
        }
        if actions.open_translate {
            self.translate_open = true;
        }
        if actions.open_crop {
            self.open_crop_window();
        }
        if actions.open_custom_kernel {
            self.custom_kernel_open = true;
        }
        if let Some(index) = actions.remove_transform {
            self.pipeline.remove(index);
            self.redo_stack.clear();
            if let Some(original) = &self.original_image {
                let result = self.pipeline.apply(original);
                self.set_display_image(ctx, result, "transformed".to_owned());
            }
        }
        if actions.do_undo {
            self.undo_transform(ctx);
        }
        if actions.do_redo {
            self.redo_transform(ctx);
        }
        if actions.do_clear {
            self.pipeline.clear();
            self.redo_stack.clear();
            if let Some(original) = &self.original_image {
                let result = original.clone();
                self.set_display_image(ctx, result, "transformed".to_owned());
            }
        }
    }
}
