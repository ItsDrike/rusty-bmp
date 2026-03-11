use std::{fs::File, path::PathBuf};

use bmp::{
    raw::Bmp,
    runtime::{
        decode::{decode_to_rgba, DecodedImage},
        encode::{save_bmp_ext, SaveFormat, SaveHeaderVersion, SourceMetadata},
        transform::{apply_transform, ImageTransform, RotationInterpolation, TransformPipeline, TranslateMode},
    },
};
use eframe::egui;
use rfd::FileDialog;

mod gui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CropDragMode {
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

pub(crate) struct BmpViewerApp {
    pub(crate) path_input: String,
    pub(crate) status: String,
    pub(crate) image_stats: String,
    pub(crate) decoded_stats: String,
    pub(crate) palette_colors: Vec<[u8; 4]>,
    pub(crate) texture: Option<egui::TextureHandle>,
    /// The decoded image before any transforms (kept for pipeline reapply).
    pub(crate) original_image: Option<DecodedImage>,
    pub(crate) transformed_image: Option<DecodedImage>,
    pub(crate) pipeline: TransformPipeline,
    /// Transforms that were undone, available for redo. Cleared on new transform or step removal.
    pub(crate) redo_stack: Vec<ImageTransform>,
    pub(crate) save_format: SaveFormat,
    pub(crate) save_header_version: SaveHeaderVersion,
    pub(crate) source_metadata: Option<SourceMetadata>,
    /// Path of the currently loaded file (for "Save" without a dialog).
    pub(crate) loaded_path: Option<PathBuf>,

    /// Absolute zoom level: screen pixels per image pixel.
    /// A value of 0.0 means "fit the image to the available panel space".
    pub(crate) zoom: f32,
    /// The effective zoom level from the last frame (used for display in the zoom bar).
    pub(crate) last_effective_zoom: f32,
    /// Pixel under the cursor: (x, y, [r, g, b, a]). Stored per-frame for the zoom bar.
    pub(crate) hovered_pixel: Option<(u32, u32, [u8; 4])>,
    /// Pan offset in screen pixels (relative to the centered image position).
    pub(crate) pan_offset: egui::Vec2,

    // --- Convolution / custom kernel editor state ---
    /// Whether the custom kernel editor window is open.
    pub(crate) custom_kernel_open: bool,
    /// Side length of the custom kernel being edited (1, 3, 5, or 7).
    pub(crate) custom_kernel_size: usize,
    /// Per-cell weight strings for the kernel editor (size*size elements).
    pub(crate) custom_kernel_weights: Vec<String>,
    /// Divisor string for the kernel editor.
    pub(crate) custom_kernel_divisor: String,
    /// Bias string for the kernel editor.
    pub(crate) custom_kernel_bias: String,

    /// Whether the arbitrary-angle rotation window is open.
    pub(crate) rotate_any_open: bool,
    /// Angle in degrees used by arbitrary-angle rotation.
    pub(crate) rotate_any_angle: f32,
    /// Interpolation method for arbitrary-angle rotation.
    pub(crate) rotate_any_interpolation: RotationInterpolation,
    /// Whether to expand output canvas to fit the full rotated image.
    pub(crate) rotate_any_expand: bool,

    /// Whether the resize window is open.
    pub(crate) resize_open: bool,
    /// Target width input for resize dialog.
    pub(crate) resize_width_input: String,
    /// Target height input for resize dialog.
    pub(crate) resize_height_input: String,
    /// Keep source aspect ratio when applying resize.
    pub(crate) resize_keep_aspect: bool,
    /// Interpolation method for resize.
    pub(crate) resize_interpolation: RotationInterpolation,

    /// Whether the skew/shear window is open.
    pub(crate) skew_open: bool,
    /// X shear (%) used by skew dialog.
    pub(crate) skew_x_percent: f32,
    /// Y shear (%) used by skew dialog.
    pub(crate) skew_y_percent: f32,
    /// Interpolation method for skew.
    pub(crate) skew_interpolation: RotationInterpolation,
    /// Whether to expand output canvas for skew.
    pub(crate) skew_expand: bool,

    /// Whether the translate window is open.
    pub(crate) translate_open: bool,
    /// Horizontal translation in pixels.
    pub(crate) translate_dx: i32,
    /// Vertical translation in pixels.
    pub(crate) translate_dy: i32,
    /// Crop/expand mode for translation.
    pub(crate) translate_mode: TranslateMode,
    /// Fill color for uncovered pixels after translation.
    pub(crate) translate_fill: [u8; 4],

    /// Pending brightness delta configured from side panel controls.
    pub(crate) brightness_input: i16,
    /// Pending contrast delta configured from side panel controls.
    pub(crate) contrast_input: i16,

    /// Whether the crop window is open.
    pub(crate) crop_open: bool,
    /// Crop rectangle origin X in image pixels.
    pub(crate) crop_x: u32,
    /// Crop rectangle origin Y in image pixels.
    pub(crate) crop_y: u32,
    /// Crop rectangle width in image pixels.
    pub(crate) crop_width: u32,
    /// Crop rectangle height in image pixels.
    pub(crate) crop_height: u32,
    /// Keep crop rectangle aspect ratio tied to the image aspect ratio.
    pub(crate) crop_keep_aspect: bool,
    /// Active crop drag mode for visual crop manipulation.
    pub(crate) crop_drag_mode: Option<CropDragMode>,
    /// Pointer position in image coordinates at drag start.
    pub(crate) crop_drag_start_image: Option<egui::Pos2>,
    /// Crop rect (x, y, w, h) snapshot at drag start.
    pub(crate) crop_drag_start_rect: Option<(u32, u32, u32, u32)>,
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
            redo_stack: Vec::new(),
            save_format: SaveFormat::default(),
            save_header_version: SaveHeaderVersion::default(),
            source_metadata: None,
            loaded_path: None,
            zoom: 0.0,
            last_effective_zoom: 1.0,
            hovered_pixel: None,
            pan_offset: egui::Vec2::ZERO,
            custom_kernel_open: false,
            custom_kernel_size: 3,
            custom_kernel_weights: vec!["0".to_owned(); 9],
            custom_kernel_divisor: "1".to_owned(),
            custom_kernel_bias: "0".to_owned(),
            rotate_any_open: false,
            rotate_any_angle: 0.0,
            rotate_any_interpolation: RotationInterpolation::Bilinear,
            rotate_any_expand: true,
            resize_open: false,
            resize_width_input: String::new(),
            resize_height_input: String::new(),
            resize_keep_aspect: true,
            resize_interpolation: RotationInterpolation::Bilinear,
            skew_open: false,
            skew_x_percent: 0.0,
            skew_y_percent: 0.0,
            skew_interpolation: RotationInterpolation::Bilinear,
            skew_expand: true,
            translate_open: false,
            translate_dx: 0,
            translate_dy: 0,
            translate_mode: TranslateMode::Crop,
            translate_fill: [0, 0, 0, 0],
            brightness_input: 0,
            contrast_input: 0,
            crop_open: false,
            crop_x: 0,
            crop_y: 0,
            crop_width: 1,
            crop_height: 1,
            crop_keep_aspect: false,
            crop_drag_mode: None,
            crop_drag_start_image: None,
            crop_drag_start_rect: None,
        }
    }
}

impl BmpViewerApp {
    pub(crate) fn render_palette_grid(&self, ui: &mut egui::Ui) {
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

    pub(crate) fn set_display_image(&mut self, ctx: &egui::Context, image: DecodedImage, label: String) {
        let color =
            egui::ColorImage::from_rgba_unmultiplied([image.width as usize, image.height as usize], &image.rgba);
        self.texture = Some(ctx.load_texture(label, color, egui::TextureOptions::NEAREST));
        self.transformed_image = Some(image);
    }

    pub(crate) fn load_path(&mut self, ctx: &egui::Context, path: PathBuf) {
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
        // New image load resets viewport to fit.
        self.zoom = 0.0;
        self.pan_offset = egui::Vec2::ZERO;
        self.set_display_image(ctx, decoded, path.to_string_lossy().to_string());
        self.loaded_path = Some(path.clone());
        self.status = format!("Loaded {}", path.display());
    }

    pub(crate) fn pick_and_load(&mut self, ctx: &egui::Context) {
        if let Some(path) = FileDialog::new()
            .add_filter("Bitmap image", &["bmp", "dib"])
            .set_title("Open BMP file")
            .pick_file()
        {
            self.path_input = path.display().to_string();
            self.load_path(ctx, path);
        }
    }

    pub(crate) fn apply_and_refresh(&mut self, ctx: &egui::Context, op: ImageTransform) {
        if let Some(current) = self.transformed_image.as_ref() {
            let next = apply_transform(current, &op);
            self.pipeline.push(op, Some(current));
            self.redo_stack.clear();
            self.set_display_image(ctx, next, "transformed".to_owned());
        }
    }

    pub(crate) fn undo_transform(&mut self, ctx: &egui::Context) {
        if let Some(op) = self.pipeline.pop() {
            if let Some(inv) = op.inverse() {
                self.redo_stack.push(op);
                // O(1) path: apply the inverse transform.
                if let Some(current) = self.transformed_image.as_ref() {
                    let result = apply_transform(current, &inv);
                    self.set_display_image(ctx, result, "transformed".to_owned());
                }
            } else {
                self.redo_stack.push(op);
                // Lossy transform: replay the remaining pipeline from the original image.
                if let Some(original) = self.original_image.as_ref() {
                    let result = self.pipeline.apply(original);
                    self.set_display_image(ctx, result, "transformed".to_owned());
                }
            }
        }
    }

    pub(crate) fn redo_transform(&mut self, ctx: &egui::Context) {
        if let Some(op) = self.redo_stack.pop() {
            if let Some(current) = self.transformed_image.as_ref() {
                let next = apply_transform(current, &op);
                self.pipeline.push(op, Some(current));
                self.set_display_image(ctx, next, "transformed".to_owned());
            }
        }
    }

    pub(crate) fn save_to_path(&mut self, ctx: &egui::Context, path: &std::path::Path) {
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

    pub(crate) fn save_current(&mut self, ctx: &egui::Context) {
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

    pub(crate) fn save_overwrite(&mut self, ctx: &egui::Context) {
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
                cmd && !shift && i.key_pressed(egui::Key::Z), // Undo
                cmd && (shift && i.key_pressed(egui::Key::Z)  // Redo (Ctrl+Shift+Z)
                    || i.key_pressed(egui::Key::Y)), // Redo (Ctrl+Y)
            )
        });
        let (kb_open, kb_save, kb_save_as, kb_undo, kb_redo) = kb;

        if kb_open {
            self.pick_and_load(ctx);
        }
        if kb_save {
            self.save_overwrite(ctx);
        }
        if kb_save_as {
            self.save_current(ctx);
        }
        if kb_undo {
            self.undo_transform(ctx);
        }
        if kb_redo {
            self.redo_transform(ctx);
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

        // --- Panels (order matters: top/side/bottom claim space before central) ---
        self.show_toolbar(ctx);

        // --- Floating windows ---
        if let Some(op) = self.show_rotate_any_window(ctx) {
            self.apply_and_refresh(ctx, op);
        }
        if let Some(op) = self.show_resize_window(ctx) {
            self.apply_and_refresh(ctx, op);
        }
        if let Some(op) = self.show_skew_window(ctx) {
            self.apply_and_refresh(ctx, op);
        }
        if let Some(op) = self.show_translate_window(ctx) {
            self.apply_and_refresh(ctx, op);
        }
        if let Some(op) = self.show_crop_window(ctx) {
            self.apply_and_refresh(ctx, op);
        }
        if let Some(op) = self.show_kernel_editor(ctx) {
            self.apply_and_refresh(ctx, op);
        }

        let side_actions = self.show_side_panel(ctx);
        self.apply_side_panel_actions(ctx, side_actions);

        self.show_zoom_bar(ctx);
        self.show_viewer(ctx, text_has_focus);
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
