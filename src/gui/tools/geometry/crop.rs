//! Crop tool state, crop dialog UI, and crop interaction helpers.

use eframe::egui;

use bmp::runtime::transform::{Crop, ImageTransform};

use crate::gui::BmpViewerApp;

use super::math::scaled_dim;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CropDragMode {
    Move,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Drag snapshot used while adjusting the crop interactively in the viewer.
#[derive(Clone, Copy)]
struct CropDragState {
    /// Active crop drag mode for visual crop manipulation.
    mode: CropDragMode,
    /// Pointer position in image coordinates at drag start.
    start_image: egui::Pos2,
    /// Crop rect (x, y, w, h) snapshot at drag start.
    start_rect: (u32, u32, u32, u32),
}

/// State for the crop dialog and crop preview overlay.
pub(in crate::gui) struct CropToolState {
    /// Whether the crop window is open.
    pub(in crate::gui) open: bool,
    /// Crop rectangle origin X in image pixels.
    pub(in crate::gui) x: u32,
    /// Crop rectangle origin Y in image pixels.
    pub(in crate::gui) y: u32,
    /// Crop rectangle width in image pixels.
    pub(in crate::gui) width: u32,
    /// Crop rectangle height in image pixels.
    pub(in crate::gui) height: u32,
    /// Keep crop rectangle aspect ratio tied to the image aspect ratio.
    pub(in crate::gui) keep_aspect: bool,
    /// Active crop drag state for visual crop manipulation.
    drag: Option<CropDragState>,
}

impl CropToolState {
    pub(in crate::gui) const fn new() -> Self {
        Self {
            open: false,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            keep_aspect: false,
            drag: None,
        }
    }

    /// Opens the crop tool centered on the full current image.
    pub(in crate::gui) fn open_for_image(&mut self, img_w: u32, img_h: u32) {
        self.x = (img_w.saturating_sub(1)) / 2;
        self.y = (img_h.saturating_sub(1)) / 2;
        self.width = img_w.max(1);
        self.height = img_h.max(1);
        self.drag = None;
        self.open = true;
    }

    fn clamp_inputs(&mut self, img_w: u32, img_h: u32) {
        self.x = self.x.min(img_w.saturating_sub(1));
        self.y = self.y.min(img_h.saturating_sub(1));
        self.width = self.width.max(1).min(img_w.max(1));
        self.height = self.height.max(1).min(img_h.max(1));
    }

    /// Returns the clamped top-left crop rectangle for the current center-based inputs.
    pub(in crate::gui) fn rect_for_image(&self, img_w: u32, img_h: u32) -> (u32, u32, u32, u32) {
        clamped_crop_rect(self.x, self.y, self.width, self.height, img_w, img_h)
    }

    /// Updates the center-based crop inputs from a top-left rectangle.
    pub(in crate::gui) fn set_from_rect(&mut self, x: u32, y: u32, width: u32, height: u32, img_w: u32, img_h: u32) {
        let w = width.max(1).min(img_w.max(1));
        let h = height.max(1).min(img_h.max(1));
        let x0 = x.min(img_w.saturating_sub(w));
        let y0 = y.min(img_h.saturating_sub(h));
        self.width = w;
        self.height = h;
        self.x = (x0 + w / 2).min(img_w.saturating_sub(1));
        self.y = (y0 + h / 2).min(img_h.saturating_sub(1));
    }

    pub(in crate::gui) const fn has_drag(&self) -> bool {
        self.drag.is_some()
    }

    /// Updates the cursor when hovering over a crop edge, corner, or body.
    pub(in crate::gui) fn set_hover_cursor(
        ctx: &egui::Context,
        pointer: egui::Pos2,
        crop_rect: egui::Rect,
        handle_half_size: f32,
    ) {
        if let Some(mode) = pick_crop_drag_mode(pointer, crop_rect, handle_half_size) {
            ctx.set_cursor_icon(cursor_icon_for_crop_mode(mode));
        }
    }

    /// Starts a crop drag if the pointer hits a crop handle or the crop body.
    pub(in crate::gui) fn try_begin_drag(
        &mut self,
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

        self.drag = Some(CropDragState {
            mode,
            start_rect,
            start_image: screen_to_image(pointer, img_rect, zoom),
        });
        true
    }

    /// Applies the current drag delta to the crop rectangle.
    pub(in crate::gui) fn apply_drag(
        &mut self,
        pointer: egui::Pos2,
        img_rect: egui::Rect,
        zoom: f32,
        img_w: u32,
        img_h: u32,
    ) -> bool {
        let Some(drag) = self.drag else {
            return false;
        };

        let cur = screen_to_image(pointer, img_rect, zoom);
        #[allow(clippy::cast_possible_truncation)]
        let dx = (cur.x - drag.start_image.x).round() as i32;
        #[allow(clippy::cast_possible_truncation)]
        let dy = (cur.y - drag.start_image.y).round() as i32;
        let (nx, ny, nw, nh) = dragged_crop_rect(drag.mode, drag.start_rect, dx, dy, img_w, img_h);
        self.set_from_rect(nx, ny, nw, nh, img_w, img_h);
        true
    }

    pub(in crate::gui) const fn clear_drag(&mut self) {
        self.drag = None;
    }
}

impl BmpViewerApp {
    /// Renders the crop window and returns a crop transform when applied.
    pub(in crate::gui) fn show_crop_window(&mut self, ctx: &egui::Context) -> Option<ImageTransform> {
        if !self.transforms.crop.open {
            return None;
        }

        let Some(image) = self.document.transformed_image() else {
            self.transforms.crop.open = false;
            return None;
        };
        let img_w = image.width();
        let img_h = image.height();

        let mut open = self.transforms.crop.open;
        let mut apply = false;
        let mut close_requested = false;

        egui::Window::new("Crop")
            .open(&mut open)
            .resizable(false)
            .default_width(340.0)
            .show(ctx, |ui| {
                ui.label(format!("Image size: {img_w}x{img_h}"));
                ui.small("x/y are crop center coordinates.");
                ui.add_space(6.0);

                // Persisted crop fields are intentionally kept on app state so the
                // viewer can later reuse them for an interactive overlay preview.
                ui.horizontal(|ui| {
                    ui.label("center x:");
                    ui.add(
                        egui::DragValue::new(&mut self.transforms.crop.x)
                            .speed(1)
                            .range(0..=img_w.saturating_sub(1)),
                    );
                    ui.label("center y:");
                    ui.add(
                        egui::DragValue::new(&mut self.transforms.crop.y)
                            .speed(1)
                            .range(0..=img_h.saturating_sub(1)),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("width:");
                    let width_resp = ui.add(
                        egui::DragValue::new(&mut self.transforms.crop.width)
                            .speed(1)
                            .range(1..=img_w.max(1)),
                    );
                    ui.label("height:");
                    let height_resp = ui.add(
                        egui::DragValue::new(&mut self.transforms.crop.height)
                            .speed(1)
                            .range(1..=img_h.max(1)),
                    );

                    if self.transforms.crop.keep_aspect
                        && width_resp.changed()
                        && !height_resp.has_focus()
                        && img_w > 0
                    {
                        self.transforms.crop.height = scaled_dim(self.transforms.crop.width, img_h, img_w);
                    }
                    if self.transforms.crop.keep_aspect
                        && height_resp.changed()
                        && !width_resp.has_focus()
                        && img_h > 0
                    {
                        self.transforms.crop.width = scaled_dim(self.transforms.crop.height, img_w, img_h);
                    }
                });

                ui.horizontal(|ui| {
                    let keep_aspect_resp = ui.checkbox(&mut self.transforms.crop.keep_aspect, "Keep aspect ratio");
                    if self.transforms.crop.keep_aspect && keep_aspect_resp.changed() && img_w > 0 {
                        self.transforms.crop.height = scaled_dim(self.transforms.crop.width, img_h, img_w);
                    }
                });

                self.transforms.crop.clamp_inputs(img_w, img_h);

                ui.horizontal(|ui| {
                    if ui.small_button("Reset full").clicked() {
                        self.transforms.crop.x = (img_w.saturating_sub(1)) / 2;
                        self.transforms.crop.y = (img_h.saturating_sub(1)) / 2;
                        self.transforms.crop.width = img_w.max(1);
                        self.transforms.crop.height = img_h.max(1);
                    }
                    if ui.small_button("Center 50%").clicked() {
                        self.transforms.crop.width = (img_w / 2).max(1);
                        self.transforms.crop.height = (img_h / 2).max(1);
                        self.transforms.crop.x = (img_w.saturating_sub(1)) / 2;
                        self.transforms.crop.y = (img_h.saturating_sub(1)) / 2;
                    }
                });

                self.transforms.crop.clamp_inputs(img_w, img_h);

                let (x, y, w, h) = self.transforms.crop.rect_for_image(img_w, img_h);

                ui.add_space(6.0);
                ui.colored_label(
                    egui::Color32::GREEN,
                    format!("Result: top-left=({x}, {y}), size={w}x{h}"),
                );

                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        apply = true;
                    }
                    if ui.button("Close").clicked() {
                        close_requested = true;
                    }
                });
            });

        self.transforms.crop.open = open && !close_requested;

        if !apply {
            return None;
        }

        let (x, y, width, height) = self.transforms.crop.rect_for_image(img_w, img_h);

        let crop = match Crop::try_new(x, y, width, height) {
            Ok(crop) => crop,
            Err(err) => {
                self.status = format!("Invalid crop settings: {err}");
                return None;
            }
        };

        Some(crop.into())
    }
}

/// Converts center-based crop inputs into a top-left rectangle clamped to the image bounds.
fn clamped_crop_rect(
    center_x: u32,
    center_y: u32,
    width: u32,
    height: u32,
    img_w: u32,
    img_h: u32,
) -> (u32, u32, u32, u32) {
    let cx = center_x.min(img_w.saturating_sub(1));
    let cy = center_y.min(img_h.saturating_sub(1));
    let w = width.max(1).min(img_w.max(1));
    let h = height.max(1).min(img_h.max(1));

    let mut x0 = cx.saturating_sub(w / 2);
    let mut y0 = cy.saturating_sub(h / 2);
    if x0 + w > img_w {
        x0 = img_w - w;
    }
    if y0 + h > img_h {
        y0 = img_h - h;
    }

    (x0, y0, w, h)
}

/// Maps a pointer position in viewer space into image-space coordinates.
fn screen_to_image(pointer: egui::Pos2, img_rect: egui::Rect, zoom: f32) -> egui::Pos2 {
    egui::pos2((pointer.x - img_rect.min.x) / zoom, (pointer.y - img_rect.min.y) / zoom)
}

/// Returns which part of the crop rectangle the pointer is interacting with.
fn pick_crop_drag_mode(pointer: egui::Pos2, crop_rect: egui::Rect, handle_half_size: f32) -> Option<CropDragMode> {
    for (mode, rect) in crop_handle_rects(crop_rect, handle_half_size) {
        if rect.contains(pointer) {
            return Some(mode);
        }
    }
    if crop_rect.contains(pointer) {
        Some(CropDragMode::Move)
    } else {
        None
    }
}

/// Returns the handle rectangles used to detect crop drag interactions.
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

const fn cursor_icon_for_crop_mode(mode: CropDragMode) -> egui::CursorIcon {
    match mode {
        CropDragMode::Move => egui::CursorIcon::Grab,
        CropDragMode::Left | CropDragMode::Right => egui::CursorIcon::ResizeHorizontal,
        CropDragMode::Top | CropDragMode::Bottom => egui::CursorIcon::ResizeVertical,
        CropDragMode::TopLeft | CropDragMode::BottomRight => egui::CursorIcon::ResizeNwSe,
        CropDragMode::TopRight | CropDragMode::BottomLeft => egui::CursorIcon::ResizeNeSw,
    }
}

/// Applies a drag delta to a crop rectangle while keeping it within image bounds.
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

    w = w.max(1);
    h = h.max(1);
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
