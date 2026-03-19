//! Main image canvas with zoom, pan, and pixel inspection.

use eframe::egui;

use crate::gui::{BmpViewerApp, panels::viewer::ZoomMode};

impl BmpViewerApp {
    /// Renders the central panel: image display with zoom/pan, pixel inspector, or empty state.
    ///
    /// `text_has_focus` is used to suppress plain-key zoom shortcuts when a text widget is focused.
    pub(in crate::gui) fn show_viewer(&mut self, ctx: &egui::Context, text_has_focus: bool) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some((texture_id, tex_size)) = self
                .viewport
                .texture
                .as_ref()
                .map(|texture| (texture.id(), texture.size_vec2()))
            {
                let avail = ui.available_size();

                // Scale that fits the entire image within the panel (aspect-ratio preserving).
                let fit_scale = {
                    let s = (avail.x / tex_size.x).min(avail.y / tex_size.y);
                    if s.is_finite() && s > 0.0 { s } else { 1.0 }
                };

                let effective_zoom = match self.viewport.zoom {
                    ZoomMode::Fit => fit_scale,
                    ZoomMode::Scale(scale) => scale,
                };

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
                    self.viewport.zoom = ZoomMode::Scale((effective_zoom * 1.25).max(0.01));
                }
                if kb_zoom_out {
                    self.viewport.zoom = ZoomMode::Scale((effective_zoom / 1.25).max(0.01));
                }
                if kb_zoom_fit {
                    self.viewport.zoom = ZoomMode::Fit;
                    self.viewport.pan_offset = egui::Vec2::ZERO;
                }
                if kb_zoom_1to1 {
                    self.viewport.zoom = ZoomMode::Scale(1.0);
                    self.viewport.pan_offset = egui::Vec2::ZERO;
                }

                // --- Scroll-to-zoom (anchored to cursor position) ---
                let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll_delta != 0.0 && response.hovered() {
                    let zoom_factor = (scroll_delta * 0.002).exp();
                    let new_zoom = (effective_zoom * zoom_factor).max(0.01);

                    // Zoom towards the cursor: adjust pan so the point under
                    // the cursor stays fixed.
                    if let Some(pointer) = response.hover_pos() {
                        let panel_center = panel_rect.center();
                        let img_center = panel_center + self.viewport.pan_offset;
                        let cursor_rel = pointer - img_center;
                        let ratio = new_zoom / effective_zoom;
                        self.viewport.pan_offset = pointer - panel_center - cursor_rel * ratio;
                    }

                    self.viewport.zoom = ZoomMode::Scale(new_zoom);
                }

                // --- Double-click to fit ---
                if response.double_clicked() {
                    self.viewport.zoom = ZoomMode::Fit;
                    self.viewport.pan_offset = egui::Vec2::ZERO;
                }

                // Re-resolve after possible changes above.
                let effective_zoom = match self.viewport.zoom {
                    ZoomMode::Fit => fit_scale,
                    ZoomMode::Scale(scale) => scale,
                };
                let display_size = tex_size * effective_zoom;

                // Clamp pan so the image can't be dragged entirely off-screen.
                let margin = display_size * 0.4;
                let max_pan_x = ((display_size.x - avail.x) / 2.0 + margin.x).max(0.0);
                let max_pan_y = ((display_size.y - avail.y) / 2.0 + margin.y).max(0.0);
                self.viewport.pan_offset.x = self.viewport.pan_offset.x.clamp(-max_pan_x, max_pan_x);
                self.viewport.pan_offset.y = self.viewport.pan_offset.y.clamp(-max_pan_y, max_pan_y);

                // Position the image centered in the panel, offset by pan.
                let img_center = panel_rect.center() + self.viewport.pan_offset;
                let img_rect = snap_rect_to_pixel_grid(egui::Rect::from_center_size(img_center, display_size), ctx);

                // Clip to the panel and paint.
                let painter = ui.painter_at(panel_rect);
                if self.viewport.has_transparency {
                    update_checker_tile_with_hysteresis(&mut self.viewport.checker_tile_img_px, effective_zoom);
                    self.ensure_checker_texture(ctx, self.viewport.checker_tile_img_px);
                    if let Some(checker) = self.viewport.checker_texture.as_ref() {
                        #[allow(clippy::cast_precision_loss)]
                        let repeats_x = (tex_size.x / (self.viewport.checker_tile_img_px as f32 * 2.0)).max(1.0);
                        #[allow(clippy::cast_precision_loss)]
                        let repeats_y = (tex_size.y / (self.viewport.checker_tile_img_px as f32 * 2.0)).max(1.0);
                        painter.image(
                            checker.id(),
                            img_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(repeats_x, repeats_y)),
                            egui::Color32::WHITE,
                        );
                    }
                }
                painter.image(
                    texture_id,
                    img_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );

                let crop_drag_captured = self.show_crop_overlay(ui, &response, &painter, img_rect, effective_zoom);

                // --- Drag to pan (only when crop interaction is not active) ---
                let pan_drag = response.dragged_by(egui::PointerButton::Middle)
                    || (response.dragged_by(egui::PointerButton::Primary)
                        && !self.transforms.crop.has_drag()
                        && !crop_drag_captured);
                if pan_drag {
                    // `drag_delta()` is this frame's pointer movement, so add it to the running pan offset.
                    self.viewport.pan_offset += response.drag_delta();
                }

                // --- Pixel inspector (only at high zoom where pixels are visible) ---
                const MIN_PIXEL_SIZE: f32 = 8.0;
                self.viewport.hovered_pixel = None;
                if effective_zoom >= MIN_PIXEL_SIZE
                    && let Some(pointer) = response.hover_pos()
                    && img_rect.contains(pointer)
                {
                    // Map screen position to image pixel coordinates.
                    let rel = pointer - img_rect.min;
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let px = (rel.x / effective_zoom) as u32;
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let py = (rel.y / effective_zoom) as u32;

                    if let Some(image) = self.document.transformed_image()
                        && let Some(rgba) = image.pixel(px, py)
                    {
                        self.viewport.hovered_pixel = Some((px, py, rgba));

                        // Draw highlight outline around the hovered pixel.
                        #[allow(clippy::cast_precision_loss)]
                        let pixel_screen_x = img_rect.min.x + px as f32 * effective_zoom;
                        #[allow(clippy::cast_precision_loss)]
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

                // Store effective zoom for the zoom bar (rendered before this panel).
                self.viewport.last_effective_zoom = effective_zoom;
            } else {
                Self::show_empty_state(ui);
            }
        });
    }

    /// Renders the empty state when no image is loaded.
    /// Renders the placeholder viewer when no image is loaded.
    fn show_empty_state(ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 3.0);
            ui.heading("No image loaded");
            ui.add_space(8.0);
            ui.label("Use the Browse button or Ctrl+O to open a BMP file.");
            ui.label("You can also type a path into the text field above.");

            // Drag and drop hint, with a Wayland warning.
            let on_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
            let warn_color = egui::Color32::from_rgb(200, 170, 60);
            ui.add_space(12.0);
            if on_wayland {
                ui.colored_label(warn_color, "Warning: Drag and drop is not available under Wayland.");
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
                ui.label("Or drag and drop a BMP file anywhere in this window.");
            }
        });
    }

    /// Ensures the checkerboard background texture matches the requested tile size.
    fn ensure_checker_texture(&mut self, ctx: &egui::Context, tile_img_px: u32) {
        if self.document.transformed_image().is_none() {
            self.viewport.checker_texture = None;
            self.viewport.checker_texture_tile_img_px = 0;
            return;
        }

        let same_config =
            self.viewport.checker_texture.is_some() && self.viewport.checker_texture_tile_img_px == tile_img_px;
        if same_config {
            return;
        }

        let checker = make_checkerboard_image(tile_img_px as usize);
        self.viewport.checker_texture =
            Some(ctx.load_texture("checker-background", checker, egui::TextureOptions::NEAREST_REPEAT));
        self.viewport.checker_texture_tile_img_px = tile_img_px;
    }
}

/// Builds a simple two-tone checkerboard texture image.
fn make_checkerboard_image(tile: usize) -> egui::ColorImage {
    let light = egui::Color32::from_gray(210);
    let dark = egui::Color32::from_gray(170);
    let tile = tile.max(1);
    let width = tile * 2;
    let height = tile * 2;
    let mut image = egui::ColorImage::new([width, height], dark);

    for y in 0..height {
        for x in 0..width {
            let is_light = ((x / tile) + (y / tile)) % 2 == 0;
            image.pixels[y * width + x] = if is_light { light } else { dark };
        }
    }

    image
}

/// Adjusts checkerboard tile size with hysteresis to avoid constant texture rebuilds.
fn update_checker_tile_with_hysteresis(tile: &mut u32, zoom: f32) {
    // Keep tile size in image-space, but redraw only when the tile becomes too
    // small or too large on screen.
    const MIN_IMG_TILE: u32 = 1;
    const MAX_IMG_TILE: u32 = 128;
    const MIN_SCREEN_TILE: f32 = 10.0;
    const MAX_SCREEN_TILE: f32 = 24.0;

    if *tile == 0 {
        *tile = 8;
    }

    #[allow(clippy::cast_precision_loss)]
    while (*tile as f32 * zoom) < MIN_SCREEN_TILE && *tile < MAX_IMG_TILE {
        *tile = (*tile * 2).min(MAX_IMG_TILE);
    }

    #[allow(clippy::cast_precision_loss)]
    while (*tile as f32 * zoom) > MAX_SCREEN_TILE && *tile > MIN_IMG_TILE {
        *tile = (*tile / 2).max(MIN_IMG_TILE);
    }
}

/// Snaps a rectangle to the current pixel grid for sharper nearest-neighbor rendering.
fn snap_rect_to_pixel_grid(rect: egui::Rect, ctx: &egui::Context) -> egui::Rect {
    let pixel = 1.0 / ctx.pixels_per_point();
    let snap = |v: f32| (v / pixel).round() * pixel;
    egui::Rect::from_min_max(
        egui::pos2(snap(rect.min.x), snap(rect.min.y)),
        egui::pos2(snap(rect.max.x), snap(rect.max.y)),
    )
}
