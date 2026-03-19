//! Right-hand inspector panel for edit tools, history, and file details.

use eframe::egui;

use bmp::runtime::transform::{
    Brightness, Contrast, ConvolutionFilter, ConvolutionPreset, Grayscale, ImageTransform, InvertColors,
    MirrorHorizontal, MirrorVertical, RotateLeft, RotateRight, Sepia,
};

use crate::gui::BmpViewerApp;

/// Deferred actions from the side panel that need `&mut self` after the closure returns.
pub(in crate::gui) enum InspectorAction {
    ApplyTransform(ImageTransform),
    OpenRotateAny,
    OpenResize,
    OpenSkew,
    OpenTranslate,
    OpenCrop,
    OpenCustomKernel,
    RemoveTransform(usize),
    OpenStegEmbed,
    OpenStegInspect,
    Undo,
    Redo,
    ClearHistory,
}

impl BmpViewerApp {
    /// Renders the right side panel: file info, decoded info, transforms, palette.
    ///
    /// Returns deferred actions that must be applied by the caller
    /// after this method returns, since the panel closure borrows `&self`.
    pub(in crate::gui) fn show_side_panel(&mut self, ctx: &egui::Context) -> Vec<InspectorAction> {
        let window_width = ctx.available_rect().width();
        let panel_max_width = (window_width - 220.0).clamp(220.0, 460.0);

        let mut actions = Vec::new();

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
                        let has_image = self.document.transformed_image().is_some();
                        if !has_image {
                            ui.label("Load an image to enable editing tools.");
                            return;
                        }

                        ui.label("Geometry");
                        ui.horizontal(|ui| {
                            ui.small("Rotate:");
                            if ui.small_button("Left").clicked() {
                                actions.push(InspectorAction::ApplyTransform(RotateLeft.into()));
                            }
                            if ui.small_button("Right").clicked() {
                                actions.push(InspectorAction::ApplyTransform(RotateRight.into()));
                            }
                            if ui.small_button("Arbitrary...").clicked() {
                                actions.push(InspectorAction::OpenRotateAny);
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.small("Mirror:");
                            if ui.small_button("Horizontal").clicked() {
                                actions.push(InspectorAction::ApplyTransform(MirrorHorizontal.into()));
                            }
                            if ui.small_button("Vertical").clicked() {
                                actions.push(InspectorAction::ApplyTransform(MirrorVertical.into()));
                            }
                        });
                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button("Resize...").clicked() {
                                actions.push(InspectorAction::OpenResize);
                            }
                            if ui.small_button("Skew...").clicked() {
                                actions.push(InspectorAction::OpenSkew);
                            }
                            if ui.small_button("Translate...").clicked() {
                                actions.push(InspectorAction::OpenTranslate);
                            }
                            if ui.small_button("Crop...").clicked() {
                                actions.push(InspectorAction::OpenCrop);
                            }
                        });

                        ui.add_space(6.0);
                        ui.label("Color");
                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button("Invert").clicked() {
                                actions.push(InspectorAction::ApplyTransform(InvertColors.into()));
                            }
                            if ui.small_button("Grayscale").clicked() {
                                actions.push(InspectorAction::ApplyTransform(Grayscale.into()));
                            }
                            if ui.small_button("Sepia").clicked() {
                                actions.push(InspectorAction::ApplyTransform(Sepia.into()));
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
                                actions.push(InspectorAction::ApplyTransform(
                                    Brightness::new(self.transforms.tonal.brightness_input).into(),
                                ));
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
                                actions.push(InspectorAction::ApplyTransform(
                                    Contrast::new(self.transforms.tonal.contrast_input).into(),
                                ));
                                self.transforms.tonal.contrast_input = 0;
                            }
                        });

                        ui.add_space(6.0);
                        ui.label("Convolution");
                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button("Blur").clicked() {
                                actions.push(InspectorAction::ApplyTransform(
                                    ConvolutionPreset::new(ConvolutionFilter::Blur).into(),
                                ));
                            }
                            if ui.small_button("Sharpen").clicked() {
                                actions.push(InspectorAction::ApplyTransform(
                                    ConvolutionPreset::new(ConvolutionFilter::Sharpen).into(),
                                ));
                            }
                            if ui.small_button("Edge").clicked() {
                                actions.push(InspectorAction::ApplyTransform(
                                    ConvolutionPreset::new(ConvolutionFilter::EdgeDetect).into(),
                                ));
                            }
                            if ui.small_button("Emboss").clicked() {
                                actions.push(InspectorAction::ApplyTransform(
                                    ConvolutionPreset::new(ConvolutionFilter::Emboss).into(),
                                ));
                            }
                            if ui.small_button("Custom...").clicked() {
                                actions.push(InspectorAction::OpenCustomKernel);
                            }
                        });

                        ui.add_space(6.0);
                        ui.label("Steganography");
                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button("Embed Data...").clicked() {
                                actions.push(InspectorAction::OpenStegEmbed);
                            }
                            if ui.small_button("Inspect Data...").clicked() {
                                actions.push(InspectorAction::OpenStegInspect);
                            }
                        });
                    });

                    egui::CollapsingHeader::new(match self.document.history() {
                        Some(history) if !history.is_empty() => {
                            format!("Transforms History ({})", history.len())
                        }
                        _ => "Transforms History".to_owned(),
                    })
                    .default_open(true)
                    .show(ui, |ui| {
                        let Some(history) = self.document.history() else {
                            ui.label("Load an image to enable transform history.");
                            return;
                        };

                        let has_history = !history.is_empty();
                        let has_redo = history.has_redo();

                        if !has_history && !has_redo {
                            ui.label("Apply a transformation to see transform history.");
                        } else {
                            ui.horizontal(|ui| {
                                let can_undo = has_history;
                                let undo_tooltip = history
                                    .last_applied()
                                    .map_or_else(|| "Nothing to undo".to_owned(), |op| format!("Undo {op} (Ctrl+Z)"));
                                if ui
                                    .add_enabled(can_undo, egui::Button::new("Undo").small())
                                    .on_hover_text(&undo_tooltip)
                                    .clicked()
                                {
                                    actions.push(InspectorAction::Undo);
                                }

                                let can_redo = has_redo;
                                let redo_tooltip = history.last_redo().map_or_else(
                                    || "Nothing to redo".to_owned(),
                                    |op| format!("Redo {op} (Ctrl+Shift+Z)"),
                                );
                                if ui
                                    .add_enabled(can_redo, egui::Button::new("Redo").small())
                                    .on_hover_text(&redo_tooltip)
                                    .clicked()
                                {
                                    actions.push(InspectorAction::Redo);
                                }

                                let can_clear = has_history;
                                if ui
                                    .add_enabled(can_clear, egui::Button::new("Clear").small())
                                    .on_hover_text("Remove all transforms")
                                    .clicked()
                                {
                                    actions.push(InspectorAction::ClearHistory);
                                }
                            });

                            for (i, op) in history.ops().iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.monospace(format!("{}.", i + 1));
                                    ui.label(op.to_string());
                                    let right_padding = 18.0;
                                    let button_width = 18.0;
                                    let spacer = (ui.available_width() - button_width - right_padding).max(0.0);
                                    if spacer > 0.0 {
                                        ui.add_space(spacer);
                                    }
                                    if ui
                                        .add_sized([button_width, 18.0], egui::Button::new("\u{00d7}").small())
                                        .on_hover_text("Remove this transform")
                                        .clicked()
                                    {
                                        actions.push(InspectorAction::RemoveTransform(i));
                                    }
                                });
                            }
                        }
                    });

                    egui::CollapsingHeader::new("BMP Details")
                        .default_open(true)
                        .show(ui, |ui| {
                            if self.inspection.image_stats.is_empty() {
                                ui.label("Load a BMP file to inspect its metadata.");
                                return;
                            }

                            ui.label("File Info");
                            egui::ScrollArea::vertical()
                                .id_salt("file_info_scroll")
                                .max_height(220.0)
                                .show(ui, |ui| {
                                    ui.monospace(&self.inspection.image_stats);
                                });

                            ui.separator();
                            ui.label("Decoded Info");
                            egui::ScrollArea::vertical()
                                .id_salt("decoded_info_scroll")
                                .max_height(150.0)
                                .show(ui, |ui| {
                                    ui.monospace(&self.inspection.decoded_stats);
                                });
                        });

                    egui::CollapsingHeader::new("Color Palette")
                        .default_open(false)
                        .show(ui, |ui| {
                            if self.inspection.palette_colors.is_empty() {
                                ui.label("No palette available for this image.");
                            } else {
                                egui::ScrollArea::vertical()
                                    .id_salt("palette_scroll")
                                    .max_height(220.0)
                                    .show(ui, |ui| {
                                        render_palette_grid(ui, &self.inspection.palette_colors);
                                    });
                            }
                        });
                });
            });

        actions
    }

    /// Applies deferred actions returned by [`show_side_panel`].
    pub(in crate::gui) fn apply_side_panel_actions(&mut self, ctx: &egui::Context, actions: Vec<InspectorAction>) {
        for action in actions {
            match action {
                InspectorAction::ApplyTransform(op) => self.apply_and_refresh(ctx, op),
                InspectorAction::OpenRotateAny => self.transforms.rotate.open = true,
                InspectorAction::OpenResize => {
                    if let Some(img) = self.document.transformed_image() {
                        self.transforms.resize.open_for_image(img.width(), img.height());
                    } else {
                        "Load an image first".clone_into(&mut self.status);
                    }
                }
                InspectorAction::OpenSkew => self.transforms.skew.open = true,
                InspectorAction::OpenTranslate => self.transforms.translate.open = true,
                InspectorAction::OpenCrop => {
                    if let Some(img) = self.document.transformed_image() {
                        self.transforms.crop.open_for_image(img.width(), img.height());
                    } else {
                        "Load an image first".clone_into(&mut self.status);
                    }
                }
                InspectorAction::OpenCustomKernel => self.transforms.kernel.open = true,
                InspectorAction::RemoveTransform(index) => {
                    let status = {
                        let mut session = self.edit_session();
                        session.remove_transform(ctx, index)
                    };
                    if let Some(status) = status {
                        self.status = status;
                    }
                }
                InspectorAction::OpenStegEmbed => self.steganography.embed_open = true,
                InspectorAction::OpenStegInspect => self.steganography.inspect_open = true,
                InspectorAction::Undo => self.undo_transform(ctx),
                InspectorAction::Redo => self.redo_transform(ctx),
                InspectorAction::ClearHistory => {
                    let mut session = self.edit_session();
                    session.clear_transform_history(ctx);
                }
            }
        }
    }
}

/// Renders a grid of palette swatches with hover text for exact RGB values.
fn render_palette_grid(ui: &mut egui::Ui, colors: &[[u8; 4]]) {
    if colors.is_empty() {
        return;
    }

    ui.small(format!("{} colors", colors.len()));

    let swatch_size = 18.0f32;
    let old_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
    ui.horizontal_wrapped(|ui| {
        for (i, color) in colors.iter().copied().enumerate() {
            let rgba = egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
            let (rect, response) = ui.allocate_exact_size(egui::vec2(swatch_size, swatch_size), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, rgba);

            response.on_hover_text(format!(
                "#{i}\nRGB({}, {}, {})\n#{:02X}{:02X}{:02X}",
                color[0], color[1], color[2], color[0], color[1], color[2]
            ));
        }
    });
    ui.spacing_mut().item_spacing = old_spacing;
}
