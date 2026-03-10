use std::{fs::File, path::PathBuf};

use bmp::{
    raw::Bmp,
    runtime::{
        decode::{decode_to_rgba, DecodedImage},
        encode::{save_bmp_ext, SaveFormat, SaveHeaderVersion, SourceMetadata},
        transform::{apply_transform, ImageTransform, TransformPipeline},
    },
};
use eframe::egui;
use rfd::FileDialog;

mod gui;

struct BmpViewerApp {
    path_input: String,
    status: String,
    image_stats: String,
    decoded_stats: String,
    palette_colors: Vec<[u8; 4]>,
    texture: Option<egui::TextureHandle>,
    transformed_image: Option<DecodedImage>,
    pipeline: TransformPipeline,
    save_format: SaveFormat,
    save_header_version: SaveHeaderVersion,
    source_metadata: Option<SourceMetadata>,

    /// Current zoom level (1.0 = fit-to-window, >1.0 = zoomed in).
    zoom: f32,
    /// Pan offset in screen pixels (relative to the centered image position).
    pan_offset: egui::Vec2,
}

impl Default for BmpViewerApp {
    fn default() -> Self {
        Self {
            path_input: String::new(),
            status: String::new(),
            image_stats: String::new(),
            decoded_stats: String::new(),
            palette_colors: Vec::new(),
            texture: None,
            transformed_image: None,
            pipeline: TransformPipeline::default(),
            save_format: SaveFormat::default(),
            save_header_version: SaveHeaderVersion::default(),
            source_metadata: None,
            zoom: 1.0,
            pan_offset: egui::Vec2::ZERO,
        }
    }
}

impl BmpViewerApp {
    fn render_palette_grid(&self, ui: &mut egui::Ui) {
        if self.palette_colors.is_empty() {
            return;
        }

        ui.small(format!("{} colors", self.palette_colors.len()));

        let swatch_size = 18.0f32;
        let old_spacing = ui.spacing().item_spacing;
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        ui.horizontal_wrapped(|ui| {
            for (i, color) in self.palette_colors.iter().copied().enumerate() {
                let rgba = egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(swatch_size, swatch_size), egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, rgba);

                response.on_hover_text(format!(
                    "#{i}\nRGB({}, {}, {})\n#{:02X}{:02X}{:02X}",
                    color[0], color[1], color[2], color[0], color[1], color[2]
                ));
            }
        });
        ui.spacing_mut().item_spacing = old_spacing;
    }

    fn set_display_image(&mut self, ctx: &egui::Context, image: DecodedImage, label: String) {
        let color =
            egui::ColorImage::from_rgba_unmultiplied([image.width as usize, image.height as usize], &image.rgba);
        self.texture = Some(ctx.load_texture(label, color, egui::TextureOptions::NEAREST));
        self.transformed_image = Some(image);
        self.zoom = 1.0;
        self.pan_offset = egui::Vec2::ZERO;
    }

    fn load_path(&mut self, ctx: &egui::Context, path: PathBuf) {
        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(err) => {
                self.status = format!("Failed to open {}: {err}", path.display());
                return;
            }
        };

        let bmp = match Bmp::read_checked(&mut file) {
            Ok(bmp) => bmp,
            Err(err) => {
                self.status = format!("Parse failed for {}: {err}", path.display());
                return;
            }
        };

        let decoded = match decode_to_rgba(&bmp) {
            Ok(image) => image,
            Err(err) => {
                self.status = format!("Decode failed for {}: {err}", path.display());
                return;
            }
        };

        self.pipeline.clear();
        self.save_format = SaveFormat::from_bmp(&bmp);
        self.save_header_version = SaveHeaderVersion::from_bmp(&bmp);
        self.source_metadata = SourceMetadata::from_bmp(&bmp);
        let info = gui::metadata::format_bmp_info_sections(&bmp, &decoded);
        self.image_stats = info.image_stats;
        self.decoded_stats = info.decoded_stats;
        self.palette_colors = gui::palette::extract_palette_colors(&bmp);
        self.set_display_image(ctx, decoded, path.to_string_lossy().to_string());
        self.status = format!("Loaded {}", path.display());
    }

    fn pick_and_load(&mut self, ctx: &egui::Context) {
        if let Some(path) = FileDialog::new()
            .add_filter("Bitmap image", &["bmp", "dib"])
            .set_title("Open BMP file")
            .pick_file()
        {
            self.path_input = path.display().to_string();
            self.load_path(ctx, path);
        }
    }

    fn apply_and_refresh(&mut self, ctx: &egui::Context, op: ImageTransform) {
        if let Some(current) = self.transformed_image.as_ref() {
            let next = apply_transform(current, op);
            self.pipeline.push(op);
            self.set_display_image(ctx, next, "transformed".to_owned());
        }
    }

    fn save_current(&mut self) {
        let Some(image) = self.transformed_image.as_ref() else {
            self.status = "Nothing to save".to_owned();
            return;
        };

        let Some(path) = FileDialog::new()
            .add_filter("Bitmap image", &["bmp"])
            .set_title("Save transformed BMP")
            .set_file_name("transformed.bmp")
            .save_file()
        else {
            return;
        };

        match save_bmp_ext(
            &path,
            image,
            self.save_format,
            self.save_header_version,
            self.source_metadata.as_ref(),
        ) {
            Ok(()) => {
                self.status = format!(
                    "Saved {} ({}, {})",
                    path.display(),
                    self.save_format,
                    self.save_header_version
                );
            }
            Err(err) => {
                self.status = format!("Save failed: {err}");
            }
        }
    }
}

impl eframe::App for BmpViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

            ui.horizontal(|ui| {
                let rotate_left = ui.button("Rotate Left").clicked();
                let rotate_right = ui.button("Rotate Right").clicked();
                let mirror = ui.button("Mirror").clicked();
                let invert = ui.button("Invert Colors").clicked();
                ui.separator();
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
                let save_clicked = ui.button("Save As...").clicked();
                if rotate_left {
                    self.apply_and_refresh(ctx, ImageTransform::RotateLeft90);
                }
                if rotate_right {
                    self.apply_and_refresh(ctx, ImageTransform::RotateRight90);
                }
                if mirror {
                    self.apply_and_refresh(ctx, ImageTransform::MirrorHorizontal);
                }
                if invert {
                    self.apply_and_refresh(ctx, ImageTransform::InvertColors);
                }
                if save_clicked {
                    self.save_current();
                }
            });
            if !self.status.is_empty() {
                ui.label(&self.status);
            }
        });

        let window_width = ctx.available_rect().width();
        let panel_max_width = (window_width - 220.0).clamp(220.0, 460.0);

        egui::SidePanel::right("bmp_info")
            .default_width(320.0)
            .width_range(220.0..=panel_max_width)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("BMP Details");
                ui.separator();
                if self.image_stats.is_empty() {
                    ui.label("Load a BMP file to inspect its metadata.");
                } else {
                    let available_height = ui.available_height();
                    let has_palette = !self.palette_colors.is_empty();
                    let (file_height, decoded_height, palette_height) = if has_palette {
                        let file_h = (available_height * 0.38).max(100.0);
                        let decoded_h = (available_height * 0.18).max(80.0);
                        let palette_h = (available_height - file_h - decoded_h).max(120.0);
                        (file_h, decoded_h, Some(palette_h))
                    } else {
                        let file_h = (available_height * 0.62).max(120.0);
                        let decoded_h = (available_height - file_h).max(90.0);
                        (file_h, decoded_h, None)
                    };

                    ui.label("File Info");
                    egui::ScrollArea::vertical()
                        .id_salt("file_info_scroll")
                        .max_height(file_height)
                        .show(ui, |ui| {
                            ui.monospace(&self.image_stats);
                        });

                    ui.separator();
                    ui.label("Decoded Info");
                    egui::ScrollArea::vertical()
                        .id_salt("decoded_info_scroll")
                        .max_height(decoded_height)
                        .show(ui, |ui| {
                            ui.monospace(&self.decoded_stats);
                        });

                    if let Some(palette_height) = palette_height {
                        ui.separator();
                        ui.label("Color Palette");
                        egui::ScrollArea::vertical()
                            .id_salt("palette_scroll")
                            .max_height(palette_height)
                            .show(ui, |ui| {
                                self.render_palette_grid(ui);
                            });
                    }
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(texture) = &self.texture {
                let avail = ui.available_size();
                let tex_size = texture.size_vec2();

                // Compute the base scale that fits the image within the panel
                // (same as the old logic, but now it's the "1x" baseline).
                let base_scale = if tex_size.x > avail.x || tex_size.y > avail.y {
                    let s = (avail.x / tex_size.x).min(avail.y / tex_size.y);
                    if s.is_finite() && s > 0.0 {
                        s
                    } else {
                        1.0
                    }
                } else {
                    1.0
                };

                let display_size = tex_size * base_scale * self.zoom;

                // Allocate the full available area and sense drag + scroll.
                let (panel_rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());

                // --- Scroll-to-zoom (anchored to cursor position) ---
                let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll_delta != 0.0 && response.hovered() {
                    let zoom_factor = (scroll_delta * 0.002).exp();
                    let new_zoom = (self.zoom * zoom_factor).clamp(0.1, 50.0);

                    // Zoom towards the cursor: adjust pan so the point under
                    // the cursor stays fixed.
                    if let Some(pointer) = response.hover_pos() {
                        let panel_center = panel_rect.center();
                        // The image center in screen space (before this zoom step):
                        let img_center = panel_center + self.pan_offset;
                        // Cursor position relative to the image center:
                        let cursor_rel = pointer - img_center;
                        // After zooming, the same image point should stay under
                        // the cursor. Scale the offset accordingly.
                        let ratio = new_zoom / self.zoom;
                        self.pan_offset = pointer - panel_center - cursor_rel * ratio;
                    }

                    self.zoom = new_zoom;
                }

                // --- Drag to pan ---
                if response.dragged() {
                    self.pan_offset += response.drag_delta();
                }

                // --- Double-click to reset zoom ---
                if response.double_clicked() {
                    self.zoom = 1.0;
                    self.pan_offset = egui::Vec2::ZERO;
                }

                // Clamp pan so the image can't be dragged entirely off-screen.
                // Allow dragging until only 10% of the image remains visible.
                let margin = display_size * 0.4;
                let max_pan_x = ((display_size.x - avail.x) / 2.0 + margin.x).max(0.0);
                let max_pan_y = ((display_size.y - avail.y) / 2.0 + margin.y).max(0.0);
                self.pan_offset.x = self.pan_offset.x.clamp(-max_pan_x, max_pan_x);
                self.pan_offset.y = self.pan_offset.y.clamp(-max_pan_y, max_pan_y);

                // Position the image centered in the panel, offset by pan.
                let img_center = panel_rect.center() + self.pan_offset;
                let img_rect = egui::Rect::from_center_size(img_center, display_size);

                // Clip to the panel and paint.
                let painter = ui.painter_at(panel_rect);
                painter.image(
                    texture.id(),
                    img_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );

                // Show zoom level indicator when zoomed.
                if (self.zoom - 1.0).abs() > 0.01 {
                    let zoom_text = format!("{:.0}%", self.zoom * base_scale * 100.0);
                    let text_pos = panel_rect.left_bottom() + egui::vec2(8.0, -8.0);
                    painter.text(
                        text_pos,
                        egui::Align2::LEFT_BOTTOM,
                        zoom_text,
                        egui::FontId::proportional(13.0),
                        ui.visuals().text_color(),
                    );
                }
            } else {
                ui.label("Load a BMP file to preview it.");
            }
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "BMP Viewer",
        options,
        Box::new(|_cc| Ok(Box::<BmpViewerApp>::default())),
    )
}
