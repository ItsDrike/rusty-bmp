//! Shared viewer viewport state such as textures, zoom, and pan.

use bmp::runtime::decode::DecodedImage;
use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq)]
/// How the image should currently be scaled in the viewer.
pub(in crate::gui) enum ZoomMode {
    Fit,
    Scale(f32),
}

/// Viewport-related UI state for the central image panel.
pub(in crate::gui) struct ViewportState {
    /// Cached GPU texture for the currently displayed decoded image.
    pub(in crate::gui) texture: Option<egui::TextureHandle>,
    /// Cached checkerboard texture drawn behind transparent images.
    pub(in crate::gui) checker_texture: Option<egui::TextureHandle>,
    /// Tile size (in image pixels) used to build `checker_texture`.
    pub(in crate::gui) checker_texture_tile_img_px: u32,
    /// Whether the currently displayed image contains any non-opaque alpha.
    pub(in crate::gui) has_transparency: bool,
    /// Checker tile size in image pixels. Adjusted with hysteresis so redraws
    /// only happen when tiles become too small/large on screen.
    pub(in crate::gui) checker_tile_img_px: u32,
    /// Zoom level for image display.
    pub(in crate::gui) zoom: ZoomMode,
    /// The effective zoom level from the last frame (used for display in the zoom bar).
    pub(in crate::gui) last_effective_zoom: f32,
    /// Pixel under the cursor: (x, y, [r, g, b, a]). Stored per-frame for the zoom bar.
    pub(in crate::gui) hovered_pixel: Option<(u32, u32, [u8; 4])>,
    /// Pan offset in screen pixels (relative to the centered image position).
    pub(in crate::gui) pan_offset: egui::Vec2,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            texture: None,
            checker_texture: None,
            checker_texture_tile_img_px: 0,
            has_transparency: false,
            checker_tile_img_px: 8,
            zoom: ZoomMode::Fit,
            last_effective_zoom: 1.0,
            hovered_pixel: None,
            pan_offset: egui::Vec2::ZERO,
        }
    }
}

impl ViewportState {
    /// Resets zoom and pan so the next frame starts from a fit-to-screen view.
    pub(in crate::gui) const fn reset_for_new_image(&mut self) {
        self.zoom = ZoomMode::Fit;
        self.pan_offset = egui::Vec2::ZERO;
    }

    /// Uploads a decoded image into the viewer texture and refreshes transparency caches.
    pub(in crate::gui) fn set_display_image(&mut self, ctx: &egui::Context, image: &DecodedImage, label: String) {
        self.has_transparency = image.pixels().any(|px| px[3] < u8::MAX);
        self.checker_texture = None;
        self.checker_texture_tile_img_px = 0;
        let color =
            egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.rgba());
        self.texture = Some(ctx.load_texture(label, color, egui::TextureOptions::NEAREST));
    }
}
