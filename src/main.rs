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
    /// The decoded image before any transforms (kept for pipeline reapply).
    original_image: Option<DecodedImage>,
    transformed_image: Option<DecodedImage>,
    pipeline: TransformPipeline,
    save_format: SaveFormat,
    save_header_version: SaveHeaderVersion,
    source_metadata: Option<SourceMetadata>,
    /// Path of the currently loaded file (for "Save" without a dialog).
    loaded_path: Option<PathBuf>,

    /// Absolute zoom level: screen pixels per image pixel.
    /// A value of 0.0 means "fit the image to the available panel space".
    zoom: f32,
    /// The effective zoom level from the last frame (used for display in the zoom bar).
    last_effective_zoom: f32,
    /// Pixel under the cursor: (x, y, [r, g, b, a]). Stored per-frame for the zoom bar.
    hovered_pixel: Option<(u32, u32, [u8; 4])>,
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
            original_image: None,
            transformed_image: None,
            pipeline: TransformPipeline::default(),
            save_format: SaveFormat::default(),
            save_header_version: SaveHeaderVersion::default(),
            source_metadata: None,
            loaded_path: None,
            zoom: 0.0,
            last_effective_zoom: 1.0,
            hovered_pixel: None,
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
        self.zoom = 0.0;
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
        self.original_image = Some(decoded.clone());
        self.set_display_image(ctx, decoded, path.to_string_lossy().to_string());
        self.loaded_path = Some(path.clone());
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

    fn save_to_path(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        let Some(image) = self.transformed_image.as_ref() else {
            self.status = "Nothing to save".to_owned();
            return;
        };

        match save_bmp_ext(
            path,
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
                // Re-load from disk so metadata, original_image, and pipeline
                // all reflect the file as it was actually written.
                self.load_path(ctx, path.to_path_buf());
            }
            Err(err) => {
                self.status = format!("Save failed: {err}");
            }
        }
    }

    fn save_current(&mut self, ctx: &egui::Context) {
        if self.transformed_image.is_none() {
            self.status = "Nothing to save".to_owned();
            return;
        }

        let Some(path) = FileDialog::new()
            .add_filter("Bitmap image", &["bmp"])
            .set_title("Save transformed BMP")
            .set_file_name("transformed.bmp")
            .save_file()
        else {
            return;
        };

        self.save_to_path(ctx, &path);
    }

    fn save_overwrite(&mut self, ctx: &egui::Context) {
        let Some(path) = self.loaded_path.clone() else {
            self.status = "No file to overwrite".to_owned();
            return;
        };

        self.save_to_path(ctx, &path);
    }
}

impl eframe::App for BmpViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // --- Global keyboard shortcuts ---
        let text_has_focus = ctx.memory(|m| m.focused().is_some());
        let kb = ctx.input(|i| {
            let cmd = i.modifiers.command; // Ctrl on Linux/Windows, Cmd on macOS
            let shift = i.modifiers.shift;
            (
                cmd && i.key_pressed(egui::Key::O),           // Open
                cmd && !shift && i.key_pressed(egui::Key::S), // Save
                cmd && shift && i.key_pressed(egui::Key::S),  // Save As
            )
        });
        let (kb_open, kb_save, kb_save_as) = kb;

        if kb_open {
            self.pick_and_load(ctx);
        }
        if kb_save {
            self.save_overwrite(ctx);
        }
        if kb_save_as {
            self.save_current(ctx);
        }

        // --- Drag & drop file loading ---
        // Note: this relies on winit's WindowEvent::DroppedFile, which is NOT
        // implemented on Wayland as of winit 0.30.x (see winit#1881). It works
        // fine on X11 and will work on Wayland once winit merges DnD support.
        let dropped_files = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(file) = dropped_files.first() {
            if let Some(path) = &file.path {
                self.path_input = path.display().to_string();
                self.load_path(ctx, path.clone());
            }
        }
        if !dropped_files.is_empty() {
            eprintln!(
                "[dnd] dropped {} file(s): {:?}",
                dropped_files.len(),
                dropped_files
                    .iter()
                    .map(|f| (&f.path, &f.name, f.bytes.as_ref().map(|b| b.len())))
                    .collect::<Vec<_>>()
            );
        }
        if let Some(file) = dropped_files.first() {
            if let Some(path) = &file.path {
                self.path_input = path.display().to_string();
                self.load_path(ctx, path.clone());
            }
        }

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
                let save_as_clicked = ui.button("Save As...").clicked();
                let can_save = self.loaded_path.is_some() && self.transformed_image.is_some();
                let save_clicked = ui.add_enabled(can_save, egui::Button::new("Save")).clicked();
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

        let window_width = ctx.available_rect().width();
        let panel_max_width = (window_width - 220.0).clamp(220.0, 460.0);

        let mut remove_transform: Option<usize> = None;

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
                    let has_transforms = !self.pipeline.is_empty();

                    // Reserve a fixed height for the transforms section when present.
                    // Each row is ~20px; cap the section at roughly 6 visible rows.
                    let transform_height = if has_transforms {
                        (self.pipeline.len() as f32 * 22.0 + 10.0).min(140.0)
                    } else {
                        0.0
                    };
                    // Account for separators/labels (~20px each section header).
                    let overhead = if has_transforms { 24.0 } else { 0.0 };
                    let remaining = (available_height - transform_height - overhead).max(200.0);

                    let (file_height, decoded_height, palette_height) = if has_palette {
                        let file_h = (remaining * 0.45).max(100.0);
                        let decoded_h = (remaining * 0.20).max(80.0);
                        let palette_h = (remaining - file_h - decoded_h).max(100.0);
                        (file_h, decoded_h, Some(palette_h))
                    } else {
                        let file_h = (remaining * 0.62).max(120.0);
                        let decoded_h = (remaining - file_h).max(90.0);
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

                    if has_transforms {
                        ui.separator();
                        ui.label(format!("Transforms ({})", self.pipeline.len()));
                        egui::ScrollArea::vertical()
                            .id_salt("transforms_scroll")
                            .max_height(transform_height)
                            .show(ui, |ui| {
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
                            });
                    }

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

        // Handle transform removal outside the panel closure (needs &mut self).
        if let Some(index) = remove_transform {
            self.pipeline.remove(index);
            if let Some(original) = &self.original_image {
                let result = self.pipeline.apply(original);
                self.set_display_image(ctx, result, "transformed".to_owned());
            }
        }

        // --- Zoom status bar (below the viewer, above CentralPanel) ---
        if self.texture.is_some() {
            egui::TopBottomPanel::bottom("zoom_bar")
                .exact_height(24.0)
                .show(ctx, |ui| {
                    ui.horizontal_centered(|ui| {
                        // Zoom label on the left.
                        let zoom_label = if self.zoom == 0.0 {
                            format!("{:.0}% (Fit)", self.last_effective_zoom * 100.0)
                        } else {
                            format!("{:.0}%", self.zoom * 100.0)
                        };
                        ui.monospace(&zoom_label);

                        // Pixel info (from previous frame's hovered_pixel).
                        if let Some((px, py, rgba)) = self.hovered_pixel {
                            ui.separator();
                            ui.monospace(format!(
                                "({px}, {py})  RGBA({}, {}, {}, {})",
                                rgba[0], rgba[1], rgba[2], rgba[3]
                            ));
                            // Small color swatch.
                            let color = egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
                            let (swatch_rect, _) =
                                ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                            ui.painter().rect_filled(swatch_rect, 2.0, color);
                        }

                        // Push buttons to the right.
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let is_1to1 = self.zoom == 1.0;
                            if ui
                                .add_enabled(!is_1to1, egui::Button::new("1:1").small())
                                .on_hover_text("Actual pixel size (1)")
                                .clicked()
                            {
                                self.zoom = 1.0;
                                self.pan_offset = egui::Vec2::ZERO;
                            }

                            let is_fit = self.zoom == 0.0;
                            if ui
                                .add_enabled(!is_fit, egui::Button::new("Fit").small())
                                .on_hover_text("Fit image to panel (0)")
                                .clicked()
                            {
                                self.zoom = 0.0;
                                self.pan_offset = egui::Vec2::ZERO;
                            }
                        });
                    });
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(texture) = &self.texture {
                let avail = ui.available_size();
                let tex_size = texture.size_vec2();

                // Scale that fits the entire image within the panel (aspect-ratio preserving).
                let fit_scale = {
                    let s = (avail.x / tex_size.x).min(avail.y / tex_size.y);
                    if s.is_finite() && s > 0.0 {
                        s
                    } else {
                        1.0
                    }
                };

                // Resolve the effective zoom: 0.0 means "fit to panel".
                let effective_zoom = if self.zoom == 0.0 { fit_scale } else { self.zoom };

                // Allocate the full available area and sense drag + scroll.
                let (panel_rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());

                // --- Keyboard zoom shortcuts (need panel context for fit_scale) ---
                let (kb_zoom_in, kb_zoom_out, kb_zoom_fit, kb_zoom_1to1) = ui.input(|i| {
                    let cmd = i.modifiers.command;
                    let shift = i.modifiers.shift;
                    let plain = !text_has_focus && !cmd && !i.modifiers.alt;
                    (
                        plain && (i.key_pressed(egui::Key::Equals) || i.key_pressed(egui::Key::Plus)), // Zoom In
                        plain && !shift && i.key_pressed(egui::Key::Minus),                            // Zoom Out
                        plain && !shift && i.key_pressed(egui::Key::Num0),                             // Fit to window
                        plain && !shift && i.key_pressed(egui::Key::Num1), // 1:1 actual pixels
                    )
                });

                if kb_zoom_in {
                    self.zoom = (effective_zoom * 1.25).clamp(0.01, 50.0);
                }
                if kb_zoom_out {
                    self.zoom = (effective_zoom / 1.25).clamp(0.01, 50.0);
                }
                if kb_zoom_fit {
                    self.zoom = 0.0;
                    self.pan_offset = egui::Vec2::ZERO;
                }
                if kb_zoom_1to1 {
                    self.zoom = 1.0;
                    self.pan_offset = egui::Vec2::ZERO;
                }

                // --- Scroll-to-zoom (anchored to cursor position) ---
                let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll_delta != 0.0 && response.hovered() {
                    let zoom_factor = (scroll_delta * 0.002).exp();
                    let new_zoom = (effective_zoom * zoom_factor).clamp(0.01, 50.0);

                    // Zoom towards the cursor: adjust pan so the point under
                    // the cursor stays fixed.
                    if let Some(pointer) = response.hover_pos() {
                        let panel_center = panel_rect.center();
                        let img_center = panel_center + self.pan_offset;
                        let cursor_rel = pointer - img_center;
                        let ratio = new_zoom / effective_zoom;
                        self.pan_offset = pointer - panel_center - cursor_rel * ratio;
                    }

                    self.zoom = new_zoom;
                }

                // --- Drag to pan ---
                if response.dragged() {
                    self.pan_offset += response.drag_delta();
                }

                // --- Double-click to fit ---
                if response.double_clicked() {
                    self.zoom = 0.0;
                    self.pan_offset = egui::Vec2::ZERO;
                }

                // Re-resolve after possible changes above.
                let effective_zoom = if self.zoom == 0.0 { fit_scale } else { self.zoom };
                let display_size = tex_size * effective_zoom;

                // Clamp pan so the image can't be dragged entirely off-screen.
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

                // --- Pixel inspector (only at high zoom where pixels are visible) ---
                const MIN_PIXEL_SIZE: f32 = 8.0;
                self.hovered_pixel = None;
                if effective_zoom >= MIN_PIXEL_SIZE {
                    if let Some(pointer) = response.hover_pos() {
                        if img_rect.contains(pointer) {
                            // Map screen position to image pixel coordinates.
                            let rel = pointer - img_rect.min;
                            let px = (rel.x / effective_zoom) as u32;
                            let py = (rel.y / effective_zoom) as u32;

                            if let Some(image) = &self.transformed_image {
                                if px < image.width && py < image.height {
                                    let idx = ((py * image.width + px) * 4) as usize;
                                    let rgba = [
                                        image.rgba[idx],
                                        image.rgba[idx + 1],
                                        image.rgba[idx + 2],
                                        image.rgba[idx + 3],
                                    ];
                                    self.hovered_pixel = Some((px, py, rgba));

                                    // Draw highlight outline around the hovered pixel.
                                    let pixel_screen_x = img_rect.min.x + px as f32 * effective_zoom;
                                    let pixel_screen_y = img_rect.min.y + py as f32 * effective_zoom;
                                    let pixel_rect = egui::Rect::from_min_size(
                                        egui::pos2(pixel_screen_x, pixel_screen_y),
                                        egui::vec2(effective_zoom, effective_zoom),
                                    );
                                    // Use a contrasting outline: white with a black inner border
                                    // so it's visible on any pixel color.
                                    painter.rect_stroke(
                                        pixel_rect.expand(1.0),
                                        0.0,
                                        egui::Stroke::new(1.0, egui::Color32::BLACK),
                                        egui::epaint::StrokeKind::Outside,
                                    );
                                    painter.rect_stroke(
                                        pixel_rect,
                                        0.0,
                                        egui::Stroke::new(1.0, egui::Color32::WHITE),
                                        egui::epaint::StrokeKind::Outside,
                                    );
                                }
                            }
                        }
                    }
                }

                // Store effective zoom for the zoom bar (rendered before this panel).
                self.last_effective_zoom = effective_zoom;
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.heading("No image loaded");
                    ui.add_space(8.0);
                    ui.label("Use the Browse button or Ctrl+O to open a BMP file.");
                    ui.label("You can also type a path into the text field above.");

                    // Drag & drop hint — but warn on Wayland where it doesn't work.
                    let on_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
                    let warn_color = egui::Color32::from_rgb(200, 170, 60);
                    if on_wayland {
                        ui.add_space(12.0);
                        ui.colored_label(warn_color, "⚠ Drag & drop is not available under Wayland.");
                        let exe = std::env::current_exe()
                            .ok()
                            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                            .unwrap_or_else(|| env!("CARGO_PKG_NAME").to_owned());
                        ui.colored_label(warn_color, "To enable it, restart in X11 mode (requires XWayland):");
                        ui.label(
                            egui::RichText::new(format!("env -u WAYLAND_DISPLAY {exe}"))
                                .monospace()
                                .color(warn_color),
                        );
                    } else {
                        ui.add_space(12.0);
                        ui.label("Or drag and drop a BMP file anywhere in this window.");
                    }
                });
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
