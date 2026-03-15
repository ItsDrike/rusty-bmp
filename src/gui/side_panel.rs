use eframe::egui;

use bmp::runtime::transform::{
    Brightness, Contrast, ConvolutionFilter, ConvolutionPreset, Grayscale, ImageTransform, InvertColors,
    MirrorHorizontal, MirrorVertical, RotateLeft, RotateRight, Sepia,
};

use crate::BmpViewerApp;

/// Deferred actions from the side panel that need `&mut self` after the closure returns.
pub struct SidePanelActions {
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
    pub open_steg_embed: bool,
    pub open_steg_inspect: bool,
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
        let mut open_steg_embed = false;
        let mut open_steg_inspect = false;

        egui::SidePanel::right("bmp_info")
            .default_width(320.0)
            .width_range(220.0..=panel_max_width)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Inspector");
                    if self.steganography.detected.is_some() {
                        ui.add_space(6.0);
                        ui.colored_label(egui::Color32::from_rgb(80, 200, 120), "\u{25cf} steg")
                            .on_hover_text("This image contains an embedded steganography payload.");
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().id_salt("inspector_scroll").show(ui, |ui| {
                    egui::CollapsingHeader::new("Edit").default_open(true).show(ui, |ui| {
                        let has_image = self.document.transformed_image.is_some();
                        if !has_image {
                            ui.label("Load an image to enable editing tools.");
                            return;
                        }

                        ui.label("Geometry");
                        ui.horizontal(|ui| {
                            ui.small("Rotate:");
                            if ui.small_button("Left").clicked() {
                                apply_op = Some(RotateLeft.into());
                            }
                            if ui.small_button("Right").clicked() {
                                apply_op = Some(RotateRight.into());
                            }
                            if ui.small_button("Arbitrary...").clicked() {
                                open_rotate_any = true;
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.small("Mirror:");
                            if ui.small_button("Horizontal").clicked() {
                                apply_op = Some(MirrorHorizontal.into());
                            }
                            if ui.small_button("Vertical").clicked() {
                                apply_op = Some(MirrorVertical.into());
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
                                apply_op = Some(InvertColors.into());
                            }
                            if ui.small_button("Grayscale").clicked() {
                                apply_op = Some(Grayscale.into());
                            }
                            if ui.small_button("Sepia").clicked() {
                                apply_op = Some(Sepia.into());
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.add_sized([70.0, 0.0], egui::Label::new("Brightness:"));
                            ui.add(egui::Slider::new(
                                &mut self.transforms.tonal.brightness_input,
                                -255..=255,
                            ));
                            if ui
                                .add_enabled(
                                    self.transforms.tonal.brightness_input != 0,
                                    egui::Button::new("Apply").small(),
                                )
                                .clicked()
                            {
                                apply_op = Some(
                                    Brightness {
                                        delta: self.transforms.tonal.brightness_input,
                                    }
                                    .into(),
                                );
                                self.transforms.tonal.brightness_input = 0;
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.add_sized([70.0, 0.0], egui::Label::new("Contrast:"));
                            ui.add(egui::Slider::new(&mut self.transforms.tonal.contrast_input, -255..=255));
                            if ui
                                .add_enabled(
                                    self.transforms.tonal.contrast_input != 0,
                                    egui::Button::new("Apply").small(),
                                )
                                .clicked()
                            {
                                apply_op = Some(
                                    Contrast {
                                        delta: self.transforms.tonal.contrast_input,
                                    }
                                    .into(),
                                );
                                self.transforms.tonal.contrast_input = 0;
                            }
                        });

                        ui.add_space(6.0);
                        ui.label("Convolution");
                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button("Blur").clicked() {
                                apply_op = Some(
                                    ConvolutionPreset {
                                        filter: ConvolutionFilter::Blur,
                                    }
                                    .into(),
                                );
                            }
                            if ui.small_button("Sharpen").clicked() {
                                apply_op = Some(
                                    ConvolutionPreset {
                                        filter: ConvolutionFilter::Sharpen,
                                    }
                                    .into(),
                                );
                            }
                            if ui.small_button("Edge").clicked() {
                                apply_op = Some(
                                    ConvolutionPreset {
                                        filter: ConvolutionFilter::EdgeDetect,
                                    }
                                    .into(),
                                );
                            }
                            if ui.small_button("Emboss").clicked() {
                                apply_op = Some(
                                    ConvolutionPreset {
                                        filter: ConvolutionFilter::Emboss,
                                    }
                                    .into(),
                                );
                            }
                            if ui.small_button("Custom...").clicked() {
                                open_custom_kernel = true;
                            }
                        });

                        ui.add_space(6.0);
                        ui.label("Steganography");
                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button("Embed Data...").clicked() {
                                open_steg_embed = true;
                            }
                            if ui.small_button("Inspect Data...").clicked() {
                                open_steg_inspect = true;
                            }
                        });
                    });

                    egui::CollapsingHeader::new(if self.document.pipeline.is_empty() {
                        "Transforms History".to_owned()
                    } else {
                        format!("Transforms History ({})", self.document.pipeline.len())
                    })
                    .default_open(true)
                    .show(ui, |ui| {
                        let has_history = !self.document.pipeline.is_empty();
                        let has_redo = !self.document.redo_stack.is_empty();

                        if !has_history && !has_redo {
                            ui.label("Apply a transformation to see transform history.");
                        } else {
                            ui.horizontal(|ui| {
                                let can_undo = has_history;
                                let undo_tooltip =
                                    self.document.pipeline.ops().last().map_or_else(
                                        || "Nothing to undo".to_owned(),
                                        |op| format!("Undo {op} (Ctrl+Z)"),
                                    );
                                if ui
                                    .add_enabled(can_undo, egui::Button::new("Undo").small())
                                    .on_hover_text(&undo_tooltip)
                                    .clicked()
                                {
                                    do_undo = true;
                                }

                                let can_redo = has_redo;
                                let redo_tooltip = self.document.redo_stack.last().map_or_else(
                                    || "Nothing to redo".to_owned(),
                                    |op| format!("Redo {op} (Ctrl+Shift+Z)"),
                                );
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

                            for (i, op) in self.document.pipeline.ops().iter().enumerate() {
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
                            if self.document.image_stats.is_empty() {
                                ui.label("Load a BMP file to inspect its metadata.");
                                return;
                            }

                            ui.label("File Info");
                            egui::ScrollArea::vertical()
                                .id_salt("file_info_scroll")
                                .max_height(220.0)
                                .show(ui, |ui| {
                                    ui.monospace(&self.document.image_stats);
                                });

                            ui.separator();
                            ui.label("Decoded Info");
                            egui::ScrollArea::vertical()
                                .id_salt("decoded_info_scroll")
                                .max_height(150.0)
                                .show(ui, |ui| {
                                    ui.monospace(&self.document.decoded_stats);
                                });
                        });

                    egui::CollapsingHeader::new("Color Palette")
                        .default_open(false)
                        .show(ui, |ui| {
                            if self.document.palette_colors.is_empty() {
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
            open_steg_embed,
            open_steg_inspect,
        }
    }

    /// Applies deferred actions returned by [`show_side_panel`].
    pub(crate) fn apply_side_panel_actions(&mut self, ctx: &egui::Context, actions: SidePanelActions) {
        if let Some(op) = actions.apply_op {
            self.apply_and_refresh(ctx, op);
        }
        if actions.open_rotate_any {
            self.transforms.rotate.open = true;
        }
        if actions.open_resize {
            self.open_resize_window();
        }
        if actions.open_skew {
            self.transforms.skew.open = true;
        }
        if actions.open_translate {
            self.transforms.translate.open = true;
        }
        if actions.open_crop {
            self.open_crop_window();
        }
        if actions.open_custom_kernel {
            self.transforms.kernel.open = true;
        }
        if let Some(index) = actions.remove_transform {
            self.document.pipeline.remove(index);
            self.document.redo_stack.clear();
            self.steganography.overwrite_warned = false;
            if let Some(original) = &self.document.original_image {
                let (result, warnings) = self.document.pipeline.apply_with_warnings(original);
                if !warnings.is_empty() {
                    self.status = warnings.join(" ");
                }
                self.update_transformed_image(ctx, result);
            }
        }
        if actions.open_steg_embed {
            self.steganography.embed_open = true;
        }
        if actions.open_steg_inspect {
            self.steganography.inspect_open = true;
        }
        if actions.do_undo {
            self.undo_transform(ctx);
        }
        if actions.do_redo {
            self.redo_transform(ctx);
        }
        if actions.do_clear {
            self.document.pipeline.clear();
            self.document.redo_stack.clear();
            self.steganography.overwrite_warned = false;
            if let Some(original) = &self.document.original_image {
                let result = original.clone();
                self.update_transformed_image(ctx, result);
            }
        }
    }
}
