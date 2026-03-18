//! Interactive crop preview overlay drawn on top of the viewer canvas.

use eframe::egui;

use crate::gui::BmpViewerApp;

impl BmpViewerApp {
    /// Draws the crop overlay and handles in-canvas crop dragging.
    pub(super) fn show_crop_overlay(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        img_rect: egui::Rect,
        effective_zoom: f32,
    ) -> bool {
        let mut crop_drag_captured = false;

        if self.transforms.crop.open {
            if let Some((image_width, image_height)) = self
                .document
                .transformed_image()
                .map(|image| (image.width(), image.height()))
            {
                let (cx, cy, cw, ch) = self.transforms.crop.rect_for_image(image_width, image_height);

                #[allow(clippy::cast_precision_loss)]
                let crop_min = egui::pos2(
                    img_rect.min.x + cx as f32 * effective_zoom,
                    img_rect.min.y + cy as f32 * effective_zoom,
                );
                #[allow(clippy::cast_precision_loss)]
                let crop_size = egui::vec2(cw as f32 * effective_zoom, ch as f32 * effective_zoom);
                let crop_rect = egui::Rect::from_min_size(crop_min, crop_size);

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

                let hs = 5.0;
                let handles = crop_handle_rects(crop_rect, hs);
                for r in handles {
                    painter.rect_filled(r, 1.0, egui::Color32::from_rgb(80, 220, 120));
                    painter.rect_stroke(
                        r,
                        1.0,
                        egui::Stroke::new(1.0, egui::Color32::BLACK),
                        egui::epaint::StrokeKind::Outside,
                    );
                }

                if let Some(pointer) = response.hover_pos() {
                    crate::gui::tools::CropToolState::set_hover_cursor(ui.ctx(), pointer, crop_rect, hs + 4.0);
                }

                if response.drag_started_by(egui::PointerButton::Primary)
                    && let Some(pointer) = response.interact_pointer_pos()
                    && self.transforms.crop.try_begin_drag(
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

                if response.dragged_by(egui::PointerButton::Primary)
                    && !self.transforms.crop.has_drag()
                    && let Some(pointer) = response.interact_pointer_pos()
                    && self.transforms.crop.try_begin_drag(
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

                if response.dragged_by(egui::PointerButton::Primary)
                    && let Some(pointer) = response.interact_pointer_pos()
                {
                    crop_drag_captured |=
                        self.transforms
                            .crop
                            .apply_drag(pointer, img_rect, effective_zoom, image_width, image_height);
                }

                if response.drag_stopped_by(egui::PointerButton::Primary) {
                    self.transforms.crop.clear_drag();
                }
            }
        } else {
            self.transforms.crop.clear_drag();
        }

        crop_drag_captured
    }
}

/// Returns the screen-space rectangles used to draw the crop resize handles.
fn crop_handle_rects(crop_rect: egui::Rect, half_size: f32) -> [egui::Rect; 8] {
    let left = crop_rect.left();
    let right = crop_rect.right();
    let top = crop_rect.top();
    let bottom = crop_rect.bottom();
    let cx = (left + right) * 0.5;
    let cy = (top + bottom) * 0.5;
    [
        egui::Rect::from_center_size(egui::pos2(left, top), egui::vec2(half_size * 2.0, half_size * 2.0)),
        egui::Rect::from_center_size(egui::pos2(right, top), egui::vec2(half_size * 2.0, half_size * 2.0)),
        egui::Rect::from_center_size(egui::pos2(left, bottom), egui::vec2(half_size * 2.0, half_size * 2.0)),
        egui::Rect::from_center_size(egui::pos2(right, bottom), egui::vec2(half_size * 2.0, half_size * 2.0)),
        egui::Rect::from_center_size(egui::pos2(cx, top), egui::vec2(half_size * 2.0, half_size * 2.0)),
        egui::Rect::from_center_size(egui::pos2(cx, bottom), egui::vec2(half_size * 2.0, half_size * 2.0)),
        egui::Rect::from_center_size(egui::pos2(left, cy), egui::vec2(half_size * 2.0, half_size * 2.0)),
        egui::Rect::from_center_size(egui::pos2(right, cy), egui::vec2(half_size * 2.0, half_size * 2.0)),
    ]
}
