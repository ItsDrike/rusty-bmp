use eframe::egui;

use crate::{BmpViewerApp, CropDragMode};

impl BmpViewerApp {
    /// Renders the central panel: image display with zoom/pan, pixel inspector, or empty state.
    ///
    /// `text_has_focus` is used to suppress plain-key zoom shortcuts when a text widget is focused.
    pub(crate) fn show_viewer(&mut self, ctx: &egui::Context, text_has_focus: bool) {
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

                // Resolve the effective zoom: 0.0 means "fit to panel".
                let effective_zoom = if self.viewport.zoom == 0.0 {
                    fit_scale
                } else {
                    self.viewport.zoom
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
                    self.viewport.zoom = (effective_zoom * 1.25).clamp(0.01, 50.0);
                }
                if kb_zoom_out {
                    self.viewport.zoom = (effective_zoom / 1.25).clamp(0.01, 50.0);
                }
                if kb_zoom_fit {
                    self.viewport.zoom = 0.0;
                    self.viewport.pan_offset = egui::Vec2::ZERO;
                }
                if kb_zoom_1to1 {
                    self.viewport.zoom = 1.0;
                    self.viewport.pan_offset = egui::Vec2::ZERO;
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
                        let img_center = panel_center + self.viewport.pan_offset;
                        let cursor_rel = pointer - img_center;
                        let ratio = new_zoom / effective_zoom;
                        self.viewport.pan_offset = pointer - panel_center - cursor_rel * ratio;
                    }

                    self.viewport.zoom = new_zoom;
                }

                let mut crop_drag_captured = false;

                // --- Double-click to fit ---
                if response.double_clicked() {
                    self.viewport.zoom = 0.0;
                    self.viewport.pan_offset = egui::Vec2::ZERO;
                }

                // Re-resolve after possible changes above.
                let effective_zoom = if self.viewport.zoom == 0.0 {
                    fit_scale
                } else {
                    self.viewport.zoom
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
                let img_rect = egui::Rect::from_center_size(img_center, display_size);

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

                // --- Crop preview overlay + interactive visual editing ---
                if self.transforms.crop.open {
                    if let Some((image_width, image_height)) = self
                        .document
                        .transformed_image
                        .as_ref()
                        .map(|image| (image.width(), image.height()))
                    {
                        let (cx, cy, cw, ch) = self.crop_rect_for_image(image_width, image_height);

                        #[allow(clippy::cast_precision_loss)]
                        let crop_min = egui::pos2(
                            img_rect.min.x + cx as f32 * effective_zoom,
                            img_rect.min.y + cy as f32 * effective_zoom,
                        );
                        #[allow(clippy::cast_precision_loss)]
                        let crop_size = egui::vec2(cw as f32 * effective_zoom, ch as f32 * effective_zoom);
                        let crop_rect = egui::Rect::from_min_size(crop_min, crop_size);

                        // Dim outside crop region, but only over the image area.
                        let shade = egui::Color32::from_black_alpha(110);
                        if crop_rect.top() > img_rect.top() {
                            painter.rect_filled(
                                egui::Rect::from_min_max(
                                    egui::pos2(img_rect.left(), img_rect.top()),
                                    egui::pos2(img_rect.right(), crop_rect.top()),
                                ),
                                0.0,
                                shade,
                            );
                        }
                        if crop_rect.bottom() < img_rect.bottom() {
                            painter.rect_filled(
                                egui::Rect::from_min_max(
                                    egui::pos2(img_rect.left(), crop_rect.bottom()),
                                    egui::pos2(img_rect.right(), img_rect.bottom()),
                                ),
                                0.0,
                                shade,
                            );
                        }
                        if crop_rect.left() > img_rect.left() {
                            painter.rect_filled(
                                egui::Rect::from_min_max(
                                    egui::pos2(img_rect.left(), crop_rect.top()),
                                    egui::pos2(crop_rect.left(), crop_rect.bottom()),
                                ),
                                0.0,
                                shade,
                            );
                        }
                        if crop_rect.right() < img_rect.right() {
                            painter.rect_filled(
                                egui::Rect::from_min_max(
                                    egui::pos2(crop_rect.right(), crop_rect.top()),
                                    egui::pos2(img_rect.right(), crop_rect.bottom()),
                                ),
                                0.0,
                                shade,
                            );
                        }

                        // High-contrast crop rectangle.
                        painter.rect_stroke(
                            crop_rect.expand(1.0),
                            0.0,
                            egui::Stroke::new(1.0, egui::Color32::BLACK),
                            egui::epaint::StrokeKind::Outside,
                        );
                        painter.rect_stroke(
                            crop_rect,
                            0.0,
                            egui::Stroke::new(2.0, egui::Color32::from_rgb(80, 220, 120)),
                            egui::epaint::StrokeKind::Outside,
                        );

                        // Handle markers for visual resizing.
                        let hs = 5.0;
                        let handles = crop_handle_rects(crop_rect, hs);
                        for (_, r) in handles {
                            painter.rect_filled(r, 1.0, egui::Color32::from_rgb(80, 220, 120));
                            painter.rect_stroke(
                                r,
                                1.0,
                                egui::Stroke::new(1.0, egui::Color32::BLACK),
                                egui::epaint::StrokeKind::Outside,
                            );
                        }

                        if let Some(pointer) = response.hover_pos()
                            && let Some(mode) = pick_crop_drag_mode(pointer, crop_rect, hs + 4.0)
                        {
                            let icon = cursor_icon_for_crop_mode(mode);
                            ui.ctx().set_cursor_icon(icon);
                        }

                        // Start interactive crop manipulation.
                        if response.drag_started()
                            && let Some(pointer) = response.interact_pointer_pos()
                            && start_crop_drag(
                                self,
                                pointer,
                                img_rect,
                                crop_rect,
                                hs + 4.0,
                                effective_zoom,
                                (cx, cy, cw, ch),
                            )
                        {
                            crop_drag_captured = true;
                        }

                        // Fallback capture: if drag_started was not latched this frame,
                        // attempt to start crop interaction while already dragging.
                        if response.dragged()
                            && self.transforms.crop.drag_mode.is_none()
                            && let Some(pointer) = response.interact_pointer_pos()
                            && start_crop_drag(
                                self,
                                pointer,
                                img_rect,
                                crop_rect,
                                hs + 4.0,
                                effective_zoom,
                                (cx, cy, cw, ch),
                            )
                        {
                            crop_drag_captured = true;
                        }

                        // Apply ongoing drag delta.
                        if response.dragged()
                            && let (Some(mode), Some(start_rect), Some(start_pos), Some(pointer)) = (
                                self.transforms.crop.drag_mode,
                                self.transforms.crop.drag_start_rect,
                                self.transforms.crop.drag_start_image,
                                response.interact_pointer_pos(),
                            )
                        {
                            crop_drag_captured = true;
                            let cur = screen_to_image(pointer, img_rect, effective_zoom);
                            #[allow(clippy::cast_possible_truncation)]
                            let dx = (cur.x - start_pos.x).round() as i32;
                            #[allow(clippy::cast_possible_truncation)]
                            let dy = (cur.y - start_pos.y).round() as i32;
                            let (nx, ny, nw, nh) =
                                dragged_crop_rect(mode, start_rect, dx, dy, image_width, image_height);
                            self.set_crop_from_rect(nx, ny, nw, nh, image_width, image_height);
                        }

                        // Finish drag.
                        if response.drag_stopped() {
                            self.transforms.crop.drag_mode = None;
                            self.transforms.crop.drag_start_image = None;
                            self.transforms.crop.drag_start_rect = None;
                        }
                    }
                } else {
                    self.transforms.crop.drag_mode = None;
                    self.transforms.crop.drag_start_image = None;
                    self.transforms.crop.drag_start_rect = None;
                }

                // --- Drag to pan (only when crop interaction is not active) ---
                if response.dragged() && self.transforms.crop.drag_mode.is_none() && !crop_drag_captured {
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

                    if let Some(image) = &self.document.transformed_image
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

    fn ensure_checker_texture(&mut self, ctx: &egui::Context, tile_img_px: u32) {
        if self.document.transformed_image.is_none() {
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

fn screen_to_image(pointer: egui::Pos2, img_rect: egui::Rect, zoom: f32) -> egui::Pos2 {
    egui::pos2((pointer.x - img_rect.min.x) / zoom, (pointer.y - img_rect.min.y) / zoom)
}

fn start_crop_drag(
    app: &mut BmpViewerApp,
    pointer: egui::Pos2,
    img_rect: egui::Rect,
    crop_rect: egui::Rect,
    pick_radius: f32,
    zoom: f32,
    start_rect: (u32, u32, u32, u32),
) -> bool {
    if !img_rect.contains(pointer) {
        return false;
    }
    let Some(mode) = pick_crop_drag_mode(pointer, crop_rect, pick_radius) else {
        return false;
    };

    app.transforms.crop.drag_mode = Some(mode);
    app.transforms.crop.drag_start_rect = Some(start_rect);
    app.transforms.crop.drag_start_image = Some(screen_to_image(pointer, img_rect, zoom));
    true
}

fn crop_handle_rects(crop_rect: egui::Rect, half_size: f32) -> [(CropDragMode, egui::Rect); 8] {
    let left = crop_rect.left();
    let right = crop_rect.right();
    let top = crop_rect.top();
    let bottom = crop_rect.bottom();
    let cx = (left + right) * 0.5;
    let cy = (top + bottom) * 0.5;
    [
        (
            CropDragMode::TopLeft,
            egui::Rect::from_center_size(egui::pos2(left, top), egui::vec2(half_size * 2.0, half_size * 2.0)),
        ),
        (
            CropDragMode::TopRight,
            egui::Rect::from_center_size(egui::pos2(right, top), egui::vec2(half_size * 2.0, half_size * 2.0)),
        ),
        (
            CropDragMode::BottomLeft,
            egui::Rect::from_center_size(egui::pos2(left, bottom), egui::vec2(half_size * 2.0, half_size * 2.0)),
        ),
        (
            CropDragMode::BottomRight,
            egui::Rect::from_center_size(egui::pos2(right, bottom), egui::vec2(half_size * 2.0, half_size * 2.0)),
        ),
        (
            CropDragMode::Top,
            egui::Rect::from_center_size(egui::pos2(cx, top), egui::vec2(half_size * 2.0, half_size * 2.0)),
        ),
        (
            CropDragMode::Bottom,
            egui::Rect::from_center_size(egui::pos2(cx, bottom), egui::vec2(half_size * 2.0, half_size * 2.0)),
        ),
        (
            CropDragMode::Left,
            egui::Rect::from_center_size(egui::pos2(left, cy), egui::vec2(half_size * 2.0, half_size * 2.0)),
        ),
        (
            CropDragMode::Right,
            egui::Rect::from_center_size(egui::pos2(right, cy), egui::vec2(half_size * 2.0, half_size * 2.0)),
        ),
    ]
}

fn pick_crop_drag_mode(pointer: egui::Pos2, crop_rect: egui::Rect, handle_half_size: f32) -> Option<CropDragMode> {
    for (mode, r) in crop_handle_rects(crop_rect, handle_half_size) {
        if r.contains(pointer) {
            return Some(mode);
        }
    }
    if crop_rect.contains(pointer) {
        Some(CropDragMode::Move)
    } else {
        None
    }
}

const fn cursor_icon_for_crop_mode(mode: CropDragMode) -> egui::CursorIcon {
    match mode {
        CropDragMode::Move => egui::CursorIcon::Grab,
        CropDragMode::Left | CropDragMode::Right => egui::CursorIcon::ResizeHorizontal,
        CropDragMode::Top | CropDragMode::Bottom => egui::CursorIcon::ResizeVertical,
        CropDragMode::TopLeft | CropDragMode::BottomRight => egui::CursorIcon::ResizeNwSe,
        CropDragMode::TopRight | CropDragMode::BottomLeft => egui::CursorIcon::ResizeNeSw,
    }
}

fn dragged_crop_rect(
    mode: CropDragMode,
    start: (u32, u32, u32, u32),
    dx: i32,
    dy: i32,
    img_w: u32,
    img_h: u32,
) -> (u32, u32, u32, u32) {
    let (sx, sy, sw, sh) = start;
    let img_w = i64::from(img_w);
    let img_h = i64::from(img_h);
    let mut x = i64::from(sx);
    let mut y = i64::from(sy);
    let mut w = i64::from(sw);
    let mut h = i64::from(sh);
    let dx = i64::from(dx);
    let dy = i64::from(dy);

    match mode {
        CropDragMode::Move => {
            x += dx;
            y += dy;
        }
        CropDragMode::Left => {
            x += dx;
            w -= dx;
        }
        CropDragMode::Right => {
            w += dx;
        }
        CropDragMode::Top => {
            y += dy;
            h -= dy;
        }
        CropDragMode::Bottom => {
            h += dy;
        }
        CropDragMode::TopLeft => {
            x += dx;
            y += dy;
            w -= dx;
            h -= dy;
        }
        CropDragMode::TopRight => {
            y += dy;
            w += dx;
            h -= dy;
        }
        CropDragMode::BottomLeft => {
            x += dx;
            w -= dx;
            h += dy;
        }
        CropDragMode::BottomRight => {
            w += dx;
            h += dy;
        }
    }

    // Enforce minimum size first.
    w = w.max(1);
    h = h.max(1);

    // Clamp position/size into image bounds.
    x = x.clamp(0, img_w - 1);
    y = y.clamp(0, img_h - 1);
    w = w.min(img_w);
    h = h.min(img_h);

    if x + w > img_w {
        if matches!(
            mode,
            CropDragMode::Left | CropDragMode::TopLeft | CropDragMode::BottomLeft | CropDragMode::Move
        ) {
            x = img_w - w;
        } else {
            w = img_w - x;
        }
    }

    if y + h > img_h {
        if matches!(
            mode,
            CropDragMode::Top | CropDragMode::TopLeft | CropDragMode::TopRight | CropDragMode::Move
        ) {
            y = img_h - h;
        } else {
            h = img_h - y;
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (x as u32, y as u32, w as u32, h as u32)
}
