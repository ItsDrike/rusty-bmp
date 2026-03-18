use std::path::PathBuf;

use eframe::egui;

use bmp::runtime::encode::SaveHeaderVersion;

use crate::BmpViewerApp;

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
                        "Please enter a path".clone_into(&mut self.status);
                    } else {
                        self.load_path(ctx, &path);
                    }
                }
            });

            ui.separator();

            if !self.status.is_empty() {
                ui.label(&self.status);
                ui.separator();
            }

            ui.horizontal(|ui| {
                ui.label("Header:");
                egui::ComboBox::from_id_salt("save_header_version")
                    .selected_text(self.document.save_header_version.to_string())
                    .show_ui(ui, |ui| {
                        for &ver in SaveHeaderVersion::ALL {
                            ui.selectable_value(&mut self.document.save_header_version, ver, ver.to_string());
                        }
                    });
                // If the current format is not compatible with the selected header
                // version, reset to the first compatible format.
                if !self
                    .document
                    .save_header_version
                    .is_compatible(self.document.save_format)
                {
                    self.document.save_format = self.document.save_header_version.compatible_formats()[0];
                }
                ui.label("Format:");
                egui::ComboBox::from_id_salt("save_format")
                    .selected_text(self.document.save_format.to_string())
                    .show_ui(ui, |ui| {
                        for &fmt in self.document.save_header_version.compatible_formats() {
                            ui.selectable_value(&mut self.document.save_format, fmt, fmt.to_string());
                        }
                    });
                let save_as_clicked = ui.button("Save As...").clicked();
                let can_save = self.document.loaded_path().is_some() && self.document.transformed_image().is_some();
                let save_clicked = ui.add_enabled(can_save, egui::Button::new("Save")).clicked();
                if save_as_clicked {
                    self.save_current(ctx);
                }
                if save_clicked {
                    self.save_overwrite(ctx);
                }
            });
        });
    }
}
