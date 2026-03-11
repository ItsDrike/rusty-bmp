use eframe::egui;

use crate::BmpViewerApp;

impl BmpViewerApp {
    /// Renders the central panel: image display with zoom/pan, pixel inspector, or empty state.
    ///
    /// `text_has_focus` is used to suppress plain-key zoom shortcuts when a text widget is focused.
    pub(crate) fn show_viewer(&mut self, ctx: &egui::Context, text_has_focus: bool) {
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

                // --- Crop preview overlay (enabled while crop dialog is open) ---
                if self.crop_open {
                    if let Some(image) = &self.transformed_image {
                        let (cx, cy, cw, ch) = self.crop_rect_for_image(image.width, image.height);
                        let crop_min = egui::pos2(
                            img_rect.min.x + cx as f32 * effective_zoom,
                            img_rect.min.y + cy as f32 * effective_zoom,
                        );
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
                    }
                }

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
}
