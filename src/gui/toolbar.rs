use std::path::PathBuf;

use eframe::egui;

use bmp::runtime::{encode::SaveHeaderVersion, transform::ImageTransform};

use crate::{BmpViewerApp, ConvolutionSelection};

impl BmpViewerApp {
    /// Renders the top toolbar panel: path input, transform buttons, save options.
    pub(crate) fn show_toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("BMP Path:");
                let path_edit_width = (ui.available_width() - 140.0).max(80.0);
                let path_edit = ui.add_sized(
                    [path_edit_width, 24.0],
                    egui::TextEdit::singleline(&mut self.path_input)
                        .hint_text("C:\\images\\picture.bmp or /home/user/picture.bmp"),
                );
                let enter = path_edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let browse_clicked = ui.button("Browse...").clicked();
                let load_clicked = ui.button("Load").clicked();
                if browse_clicked {
                    self.pick_and_load(ctx);
                } else if enter || load_clicked {
                    let path = PathBuf::from(self.path_input.trim());
                    if path.as_os_str().is_empty() {
                        self.status = "Please enter a path".to_owned();
                    } else {
                        self.load_path(ctx, path);
                    }
                }
            });

            ui.separator();

            ui.horizontal(|ui| {
                let rotate_left = ui.button("Rotate Left").clicked();
                let rotate_right = ui.button("Rotate Right").clicked();
                let rotate_any = ui.button("Rotate...").clicked();
                let mirror_h = ui.button("Mirror H").clicked();
                let mirror_v = ui.button("Mirror V").clicked();
                let invert = ui.button("Invert Colors").clicked();
                let gray = ui.button("Grayscale").clicked();
                let sepia_btn = ui.button("Sepia").clicked();
                ui.separator();
                let bright_down = ui.button("Brightness -").clicked();
                let bright_up = ui.button("Brightness +").clicked();
                ui.separator();
                let contrast_down = ui.button("Contrast -").clicked();
                let contrast_up = ui.button("Contrast +").clicked();
                ui.separator();
                egui::ComboBox::from_id_salt("conv_filter")
                    .selected_text(self.conv_selection.to_string())
                    .width(100.0)
                    .show_ui(ui, |ui| {
                        for sel in ConvolutionSelection::all() {
                            let label = sel.to_string();
                            ui.selectable_value(&mut self.conv_selection, sel, label);
                        }
                    });
                let apply_conv = ui.button("Apply").clicked();
                if apply_conv {
                    match &self.conv_selection {
                        ConvolutionSelection::Preset(filter) => {
                            let op = ImageTransform::Convolution(filter.clone());
                            self.apply_and_refresh(ctx, op);
                        }
                        ConvolutionSelection::Custom => {
                            self.custom_kernel_open = true;
                        }
                    }
                }
                if rotate_left {
                    self.apply_and_refresh(ctx, ImageTransform::RotateLeft90);
                }
                if rotate_right {
                    self.apply_and_refresh(ctx, ImageTransform::RotateRight90);
                }
                if rotate_any {
                    self.rotate_any_open = true;
                }
                if mirror_h {
                    self.apply_and_refresh(ctx, ImageTransform::MirrorHorizontal);
                }
                if mirror_v {
                    self.apply_and_refresh(ctx, ImageTransform::MirrorVertical);
                }
                if invert {
                    self.apply_and_refresh(ctx, ImageTransform::InvertColors);
                }
                if gray {
                    self.apply_and_refresh(ctx, ImageTransform::Grayscale);
                }
                if sepia_btn {
                    self.apply_and_refresh(ctx, ImageTransform::Sepia);
                }
                if bright_down {
                    self.apply_and_refresh(ctx, ImageTransform::Brightness(-10));
                }
                if bright_up {
                    self.apply_and_refresh(ctx, ImageTransform::Brightness(10));
                }
                if contrast_down {
                    self.apply_and_refresh(ctx, ImageTransform::Contrast(-10));
                }
                if contrast_up {
                    self.apply_and_refresh(ctx, ImageTransform::Contrast(10));
                }
            });

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Header:");
                egui::ComboBox::from_id_salt("save_header_version")
                    .selected_text(self.save_header_version.to_string())
                    .show_ui(ui, |ui| {
                        for &ver in SaveHeaderVersion::ALL {
                            ui.selectable_value(&mut self.save_header_version, ver, ver.to_string());
                        }
                    });
                // If the current format is not compatible with the selected header
                // version, reset to the first compatible format.
                if !self.save_header_version.is_compatible(self.save_format) {
                    self.save_format = self.save_header_version.compatible_formats()[0];
                }
                ui.label("Format:");
                egui::ComboBox::from_id_salt("save_format")
                    .selected_text(self.save_format.to_string())
                    .show_ui(ui, |ui| {
                        for &fmt in self.save_header_version.compatible_formats() {
                            ui.selectable_value(&mut self.save_format, fmt, fmt.to_string());
                        }
                    });
                let save_as_clicked = ui.button("Save As...").clicked();
                let can_save = self.loaded_path.is_some() && self.transformed_image.is_some();
                let save_clicked = ui.add_enabled(can_save, egui::Button::new("Save")).clicked();
                if save_as_clicked {
                    self.save_current(ctx);
                }
                if save_clicked {
                    self.save_overwrite(ctx);
                }
            });
            if !self.status.is_empty() {
                ui.label(&self.status);
            }
        });
    }
}
