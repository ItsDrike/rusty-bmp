#![allow(
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::items_after_statements,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::suboptimal_flops
)]

use std::{
    collections::HashSet,
    fs::File,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
};

use bmp::{
    raw::Bmp,
    runtime::{
        decode::{DecodedImage, decode_to_rgba},
        encode::{SaveFormat, SaveHeaderVersion, SourceMetadata, encode_rgba_to_bmp_ext, save_bmp_ext},
        steganography::{self, StegInfo},
        transform::{ImageTransform, RotationInterpolation, TransformPipeline, TranslateMode},
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

/// Image/session data that is tied to the currently loaded BMP and transform pipeline.
pub(crate) struct DocumentState {
    pub(crate) image_stats: String,
    pub(crate) decoded_stats: String,
    pub(crate) palette_colors: Vec<[u8; 4]>,
    /// The decoded image before any transforms (kept for pipeline reapply).
    pub(crate) original_image: Option<DecodedImage>,
    pub(crate) transformed_image: Option<DecodedImage>,
    history: TransformHistory,
    pub(crate) save_format: SaveFormat,
    pub(crate) save_header_version: SaveHeaderVersion,
    pub(crate) source_metadata: Option<SourceMetadata>,
    /// Path of the currently loaded file (for "Save" without a dialog).
    pub(crate) loaded_path: Option<PathBuf>,
}

#[derive(Default)]
pub(crate) struct TransformHistory {
    pipeline: TransformPipeline,
    /// Transforms that were undone, available for redo. Cleared on new transform or step removal.
    redo_stack: Vec<ImageTransform>,
}

impl DocumentState {
    pub(crate) const fn history(&self) -> &TransformHistory {
        &self.history
    }

    pub(crate) const fn history_mut(&mut self) -> &mut TransformHistory {
        &mut self.history
    }
}

impl TransformHistory {
    pub(crate) fn clear(&mut self) {
        self.pipeline.clear();
        self.redo_stack.clear();
    }

    pub(crate) fn record_apply(&mut self, op: ImageTransform, current_image: Option<&DecodedImage>) {
        self.pipeline.push(op, current_image);
        self.redo_stack.clear();
    }

    pub(crate) fn pop_undo(&mut self) -> Option<ImageTransform> {
        self.pipeline.pop()
    }

    pub(crate) fn push_redo(&mut self, op: ImageTransform) {
        self.redo_stack.push(op);
    }

    pub(crate) fn pop_redo(&mut self) -> Option<ImageTransform> {
        self.redo_stack.pop()
    }

    pub(crate) fn remove(&mut self, index: usize) {
        self.pipeline.remove(index);
        self.redo_stack.clear();
    }

    pub(crate) fn apply_with_warnings(&self, original: &DecodedImage) -> (DecodedImage, Vec<String>) {
        self.pipeline.apply_with_warnings(original)
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.pipeline.is_empty()
    }

    pub(crate) const fn len(&self) -> usize {
        self.pipeline.len()
    }

    pub(crate) fn ops(&self) -> &[ImageTransform] {
        self.pipeline.ops()
    }

    pub(crate) fn last_applied(&self) -> Option<&ImageTransform> {
        self.pipeline.ops().last()
    }

    pub(crate) const fn has_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub(crate) fn last_redo(&self) -> Option<&ImageTransform> {
        self.redo_stack.last()
    }
}

/// Viewport-related UI state for the central image panel.
pub(crate) struct ViewportState {
    /// Cached GPU texture for the currently displayed decoded image.
    pub(crate) texture: Option<egui::TextureHandle>,
    /// Cached checkerboard texture drawn behind transparent images.
    pub(crate) checker_texture: Option<egui::TextureHandle>,
    /// Tile size (in image pixels) used to build `checker_texture`.
    pub(crate) checker_texture_tile_img_px: u32,
    /// Whether the currently displayed image contains any non-opaque alpha.
    pub(crate) has_transparency: bool,
    /// Checker tile size in image pixels. Adjusted with hysteresis so redraws
    /// only happen when tiles become too small/large on screen.
    pub(crate) checker_tile_img_px: u32,
    /// Absolute zoom level: screen pixels per image pixel.
    /// A value of 0.0 means "fit the image to the available panel space".
    pub(crate) zoom: f32,
    /// The effective zoom level from the last frame (used for display in the zoom bar).
    pub(crate) last_effective_zoom: f32,
    /// Pixel under the cursor: (x, y, [r, g, b, a]). Stored per-frame for the zoom bar.
    pub(crate) hovered_pixel: Option<(u32, u32, [u8; 4])>,
    /// Pan offset in screen pixels (relative to the centered image position).
    pub(crate) pan_offset: egui::Vec2,
}

/// Window/dialog state for transform tools and their per-tool inputs.
pub(crate) struct TransformToolState {
    pub(crate) kernel: KernelToolState,
    pub(crate) rotate: RotateToolState,
    pub(crate) resize: ResizeToolState,
    pub(crate) skew: SkewToolState,
    pub(crate) translate: TranslateToolState,
    pub(crate) crop: CropToolState,
    pub(crate) tonal: TonalAdjustState,
}

/// Allowed side lengths for the custom convolution kernel editor.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum KernelSize {
    One,
    Three,
    Five,
    Seven,
}

impl KernelSize {
    pub(crate) const ALL: [Self; 4] = [Self::One, Self::Three, Self::Five, Self::Seven];

    pub(crate) const fn as_usize(self) -> usize {
        match self {
            Self::One => 1,
            Self::Three => 3,
            Self::Five => 5,
            Self::Seven => 7,
        }
    }

    const fn cell_count(self) -> usize {
        let size = self.as_usize();
        size * size
    }
}

/// State for the custom kernel editor dialog.
pub(crate) struct KernelToolState {
    /// Whether the custom kernel editor window is open.
    pub(crate) open: bool,
    /// Side length of the custom kernel being edited (1, 3, 5, or 7).
    size: KernelSize,
    /// Per-cell weight strings for the kernel editor (size*size elements).
    weights: Vec<String>,
    /// Divisor string for the kernel editor.
    divisor: String,
    /// Bias string for the kernel editor.
    bias: String,
}

impl KernelToolState {
    pub(crate) fn new() -> Self {
        Self {
            open: false,
            size: KernelSize::Three,
            weights: vec!["0".to_owned(); KernelSize::Three.cell_count()],
            divisor: "1".to_owned(),
            bias: "0".to_owned(),
        }
    }

    pub(crate) const fn size(&self) -> KernelSize {
        self.size
    }

    pub(crate) const fn size_value(&self) -> usize {
        self.size.as_usize()
    }

    pub(crate) fn resize_preserving(&mut self, new_size: KernelSize) {
        let old_size = self.size.as_usize();
        let new_size_value = new_size.as_usize();
        let old_weights = std::mem::take(&mut self.weights);
        let mut new_weights = vec!["0".to_owned(); new_size.cell_count()];

        let overlap = old_size.min(new_size_value);
        let old_offset = (old_size - overlap) / 2;
        let new_offset = (new_size_value - overlap) / 2;

        for y in 0..overlap {
            for x in 0..overlap {
                let old_idx = (y + old_offset) * old_size + (x + old_offset);
                let new_idx = (y + new_offset) * new_size_value + (x + new_offset);
                if let Some(value) = old_weights.get(old_idx) {
                    new_weights[new_idx].clone_from(value);
                }
            }
        }

        self.size = new_size;
        self.weights = new_weights;
    }

    pub(crate) fn weights(&self) -> &[String] {
        &self.weights
    }

    pub(crate) fn weights_mut(&mut self) -> &mut [String] {
        &mut self.weights
    }

    pub(crate) fn set_weights(&mut self, weights: Vec<String>) {
        assert_eq!(weights.len(), self.size.cell_count());
        self.weights = weights;
    }

    pub(crate) fn divisor(&self) -> &str {
        &self.divisor
    }

    pub(crate) const fn divisor_mut(&mut self) -> &mut String {
        &mut self.divisor
    }

    pub(crate) fn set_divisor(&mut self, divisor: String) {
        self.divisor = divisor;
    }

    pub(crate) fn bias(&self) -> &str {
        &self.bias
    }

    pub(crate) const fn bias_mut(&mut self) -> &mut String {
        &mut self.bias
    }

    pub(crate) fn set_bias(&mut self, bias: String) {
        self.bias = bias;
    }
}

/// State for arbitrary-angle rotation dialog.
pub(crate) struct RotateToolState {
    /// Whether the arbitrary-angle rotation window is open.
    pub(crate) open: bool,
    /// Angle in degrees used by arbitrary-angle rotation.
    pub(crate) angle: f32,
    /// Interpolation method for arbitrary-angle rotation.
    pub(crate) interpolation: RotationInterpolation,
    /// Whether to expand output canvas to fit the full rotated image.
    pub(crate) expand: bool,
}

/// State for resize dialog.
pub(crate) struct ResizeToolState {
    /// Whether the resize window is open.
    pub(crate) open: bool,
    /// Target width input for resize dialog.
    pub(crate) width_input: String,
    /// Target height input for resize dialog.
    pub(crate) height_input: String,
    /// Keep source aspect ratio when applying resize.
    pub(crate) keep_aspect: bool,
    /// Interpolation method for resize.
    pub(crate) interpolation: RotationInterpolation,
}

/// State for skew/shear dialog.
pub(crate) struct SkewToolState {
    /// Whether the skew/shear window is open.
    pub(crate) open: bool,
    /// X shear (%) used by skew dialog.
    pub(crate) x_percent: f32,
    /// Y shear (%) used by skew dialog.
    pub(crate) y_percent: f32,
    /// Interpolation method for skew.
    pub(crate) interpolation: RotationInterpolation,
    /// Whether to expand output canvas for skew.
    pub(crate) expand: bool,
}

/// State for translate dialog.
pub(crate) struct TranslateToolState {
    /// Whether the translate window is open.
    pub(crate) open: bool,
    /// Horizontal translation in pixels.
    pub(crate) dx: i32,
    /// Vertical translation in pixels.
    pub(crate) dy: i32,
    /// Crop/expand mode for translation.
    pub(crate) mode: TranslateMode,
    /// Fill color for uncovered pixels after translation.
    pub(crate) fill: [u8; 4],
}

/// State for side-panel brightness/contrast controls.
pub(crate) struct TonalAdjustState {
    /// Pending brightness delta configured from side panel controls.
    pub(crate) brightness_input: i16,
    /// Pending contrast delta configured from side panel controls.
    pub(crate) contrast_input: i16,
}

/// State for crop dialog and interactive crop handles in the viewer.
#[derive(Clone, Copy)]
pub(crate) struct CropDragState {
    /// Active crop drag mode for visual crop manipulation.
    pub(crate) mode: CropDragMode,
    /// Pointer position in image coordinates at drag start.
    pub(crate) start_image: egui::Pos2,
    /// Crop rect (x, y, w, h) snapshot at drag start.
    pub(crate) start_rect: (u32, u32, u32, u32),
}

/// State for crop dialog and interactive crop handles in the viewer.
pub(crate) struct CropToolState {
    /// Whether the crop window is open.
    pub(crate) open: bool,
    /// Crop rectangle origin X in image pixels.
    pub(crate) x: u32,
    /// Crop rectangle origin Y in image pixels.
    pub(crate) y: u32,
    /// Crop rectangle width in image pixels.
    pub(crate) width: u32,
    /// Crop rectangle height in image pixels.
    pub(crate) height: u32,
    /// Keep crop rectangle aspect ratio tied to the image aspect ratio.
    pub(crate) keep_aspect: bool,
    /// Active crop drag state for visual crop manipulation.
    pub(crate) drag: Option<CropDragState>,
}

/// Steganography-related detection state and window inputs.
pub(crate) struct SteganographyUiState {
    /// Steganography detected in the current transformed image, if any.
    pub(crate) detected: Option<StegInfo>,
    /// Whether the "Embed Steganography" window is open.
    pub(crate) embed_open: bool,
    /// Whether the "Inspect Steganography" window is open.
    pub(crate) inspect_open: bool,
    /// Whether we already warned the user that a transform was
    /// applied while steganography was detected in the image.
    /// Reset when loading a new image and in undo/embed paths.
    pub(crate) overwrite_warned: bool,
    /// Path awaiting save confirmation because saving may alter image data.
    pub(crate) save_confirm_pending: Option<std::path::PathBuf>,
    /// Human-readable reasons shown in the save confirmation dialog.
    pub(crate) save_confirm_reason: Option<String>,
    /// Transform awaiting confirmation because it would likely corrupt an
    /// existing embedded steganography payload.
    pub(crate) transform_confirm_pending: Option<ImageTransform>,

    // --- Embed window inputs ---
    pub(crate) r_bits: u8,
    pub(crate) g_bits: u8,
    pub(crate) b_bits: u8,
    pub(crate) a_bits: u8,
    pub(crate) text_input: String,

    // --- Inspect window: cached extracted payload ---
    /// Result of the last explicit "Extract" action in the inspect window.
    /// `None` = not yet extracted; `Some(Ok(bytes))` = payload; `Some(Err(msg))` = error.
    pub(crate) extracted: Option<Result<Vec<u8>, String>>,
}

struct PendingSaveTask {
    rx: Receiver<SaveTaskResult>,
}

struct SaveTaskResult {
    path: PathBuf,
    format: SaveFormat,
    header: SaveHeaderVersion,
    result: Result<(), String>,
}

pub(crate) struct BmpViewerApp {
    pub(crate) path_input: String,
    /// UI feedback/status message shown in toolbar.
    pub(crate) status: String,
    pub(crate) document: DocumentState,
    pub(crate) viewport: ViewportState,
    pub(crate) transforms: TransformToolState,
    pub(crate) steganography: SteganographyUiState,
    pending_save: Option<PendingSaveTask>,
}

impl Default for BmpViewerApp {
    fn default() -> Self {
        Self {
            path_input: String::new(),
            status: String::new(),
            document: DocumentState {
                image_stats: String::new(),
                decoded_stats: String::new(),
                palette_colors: Vec::new(),
                original_image: None,
                transformed_image: None,
                history: TransformHistory::default(),
                save_format: SaveFormat::default(),
                save_header_version: SaveHeaderVersion::default(),
                source_metadata: None,
                loaded_path: None,
            },
            viewport: ViewportState {
                texture: None,
                checker_texture: None,
                checker_texture_tile_img_px: 0,
                has_transparency: false,
                checker_tile_img_px: 8,
                zoom: 0.0,
                last_effective_zoom: 1.0,
                hovered_pixel: None,
                pan_offset: egui::Vec2::ZERO,
            },
            transforms: TransformToolState {
                kernel: KernelToolState::new(),
                rotate: RotateToolState {
                    open: false,
                    angle: 0.0,
                    interpolation: RotationInterpolation::Bilinear,
                    expand: true,
                },
                resize: ResizeToolState {
                    open: false,
                    width_input: String::new(),
                    height_input: String::new(),
                    keep_aspect: true,
                    interpolation: RotationInterpolation::Bilinear,
                },
                skew: SkewToolState {
                    open: false,
                    x_percent: 0.0,
                    y_percent: 0.0,
                    interpolation: RotationInterpolation::Bilinear,
                    expand: true,
                },
                translate: TranslateToolState {
                    open: false,
                    dx: 0,
                    dy: 0,
                    mode: TranslateMode::Crop,
                    fill: [0, 0, 0, 0],
                },
                crop: CropToolState {
                    open: false,
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                    keep_aspect: false,
                    drag: None,
                },
                tonal: TonalAdjustState {
                    brightness_input: 0,
                    contrast_input: 0,
                },
            },
            steganography: SteganographyUiState {
                detected: None,
                embed_open: false,
                inspect_open: false,
                overwrite_warned: false,
                save_confirm_pending: None,
                save_confirm_reason: None,
                transform_confirm_pending: None,
                r_bits: 1,
                g_bits: 1,
                b_bits: 1,
                a_bits: 0,
                text_input: String::new(),
                extracted: None,
            },
            pending_save: None,
        }
    }
}

impl BmpViewerApp {
    fn validate_embed_fits_current_image(
        current: &DecodedImage,
        op: &ImageTransform,
    ) -> Result<(), steganography::StegError> {
        let ImageTransform::EmbedSteganography(embed) = op else {
            return Ok(());
        };
        steganography::embed(current, embed.config, &embed.payload).map(|_| ())
    }

    fn update_transformed_image(&mut self, ctx: &egui::Context, image: DecodedImage) {
        self.steganography.detected = bmp::runtime::steganography::detect(&image);
        self.steganography.extracted = None;
        self.set_display_image(ctx, image, "transformed".to_owned());
    }

    pub(crate) fn render_palette_grid(&self, ui: &mut egui::Ui) {
        if self.document.palette_colors.is_empty() {
            return;
        }

        ui.small(format!("{} colors", self.document.palette_colors.len()));

        let swatch_size = 18.0f32;
        let old_spacing = ui.spacing().item_spacing;
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        ui.horizontal_wrapped(|ui| {
            for (i, color) in self.document.palette_colors.iter().copied().enumerate() {
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
        self.viewport.has_transparency = image.pixels().any(|px| px[3] < u8::MAX);
        self.viewport.checker_texture = None;
        self.viewport.checker_texture_tile_img_px = 0;
        let color =
            egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.rgba());
        self.viewport.texture = Some(ctx.load_texture(label, color, egui::TextureOptions::NEAREST));
        self.document.transformed_image = Some(image);
    }

    pub(crate) fn load_path(&mut self, ctx: &egui::Context, path: &Path) {
        let mut file = match File::open(path) {
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

        self.document.history_mut().clear();
        self.document.save_format = SaveFormat::from_bmp(&bmp);
        self.document.save_header_version = SaveHeaderVersion::from_bmp(&bmp);
        self.document.source_metadata = SourceMetadata::from_bmp(&bmp);
        let info = gui::metadata::format_bmp_info_sections(&bmp, &decoded);
        self.document.image_stats = info.image_stats;
        self.document.decoded_stats = info.decoded_stats;
        self.document.palette_colors = gui::palette::extract_palette_colors(&bmp);
        self.steganography.detected = bmp::runtime::steganography::detect(&decoded);
        self.steganography.extracted = None;
        self.steganography.overwrite_warned = false;
        self.steganography.transform_confirm_pending = None;
        self.steganography.save_confirm_pending = None;
        self.steganography.save_confirm_reason = None;
        self.document.original_image = Some(decoded.clone());
        // New image load resets viewport to fit.
        self.viewport.zoom = 0.0;
        self.viewport.pan_offset = egui::Vec2::ZERO;
        self.set_display_image(ctx, decoded, path.to_string_lossy().to_string());
        self.document.loaded_path = Some(path.to_path_buf());
        self.status = format!("Loaded {}", path.display());
    }

    pub(crate) fn pick_and_load(&mut self, ctx: &egui::Context) {
        if let Some(path) = FileDialog::new()
            .add_filter("Bitmap image", &["bmp", "dib"])
            .set_title("Open BMP file")
            .pick_file()
        {
            self.path_input = path.display().to_string();
            self.load_path(ctx, &path);
        }
    }

    fn apply_transform_now(&mut self, ctx: &egui::Context, op: ImageTransform) {
        if let Some(current) = self.document.transformed_image.as_ref() {
            if matches!(op, ImageTransform::EmbedSteganography(_)) {
                self.steganography.overwrite_warned = false;
            }

            if let Err(err) = Self::validate_embed_fits_current_image(current, &op) {
                self.status = format!(
                    "Embedding aborted: payload no longer fits current image ({err}). The steganography transform was not applied."
                );
                return;
            }

            let next = match op.apply(current) {
                Ok(next) => next,
                Err(err) => {
                    self.status = format!("Failed to apply transform {op}: {err}");
                    return;
                }
            };
            let checkpoint_image = current.clone();
            self.document.history_mut().record_apply(op, Some(&checkpoint_image));
            self.update_transformed_image(ctx, next);
        }
    }

    pub(crate) fn apply_and_refresh(&mut self, ctx: &egui::Context, op: ImageTransform) {
        let should_confirm_overwrite = !self.steganography.overwrite_warned
            && !matches!(op, ImageTransform::EmbedSteganography(_))
            && !matches!(op, ImageTransform::RemoveSteganography(_))
            && self.steganography.detected.is_some();

        if should_confirm_overwrite {
            self.steganography.transform_confirm_pending = Some(op);
            return;
        }

        // Keep the public API shape unchanged for all callers.
        self.apply_transform_now(ctx, op);
    }

    pub(crate) fn undo_transform(&mut self, ctx: &egui::Context) {
        if let Some(op) = self.document.history_mut().pop_undo() {
            if let Some(inv) = op.inverse() {
                self.document.history_mut().push_redo(op);
                // O(1) path: apply the inverse transform.
                if let Some(current) = self.document.transformed_image.as_ref() {
                    let result = match inv.apply(current) {
                        Ok(result) => result,
                        Err(err) => {
                            self.status = format!("Undo failed while applying inverse: {err}");
                            return;
                        }
                    };
                    self.update_transformed_image(ctx, result);
                }
            } else {
                self.document.history_mut().push_redo(op);
                // Lossy transform: replay the remaining pipeline from the original image.
                if let Some(original) = self.document.original_image.as_ref() {
                    let (result, warnings) = self.document.history().apply_with_warnings(original);
                    if !warnings.is_empty() {
                        self.status = warnings.join(" ");
                    }
                    self.update_transformed_image(ctx, result);
                }
            }
            // After undo, reset the overwrite warning so it can fire again if needed.
            self.steganography.overwrite_warned = false;
        }
    }

    pub(crate) fn redo_transform(&mut self, ctx: &egui::Context) {
        if let Some(op) = self.document.history_mut().pop_redo()
            && let Some(current) = self.document.transformed_image.as_ref()
        {
            if let Err(err) = Self::validate_embed_fits_current_image(current, &op) {
                self.status = format!(
                    "Redo skipped: steganography payload no longer fits after prior edits ({err}). The embed step was dropped."
                );
                return;
            }

            let next = match op.apply(current) {
                Ok(next) => next,
                Err(err) => {
                    self.status = format!("Redo failed while applying {op}: {err}");
                    return;
                }
            };
            let checkpoint_image = current.clone();
            self.document.history_mut().record_apply(op, Some(&checkpoint_image));
            self.update_transformed_image(ctx, next);
        }
    }

    /// Returns whether the currently selected save settings preserve the exact
    /// embedded steganography payload, determined by an in-memory roundtrip.
    fn save_preserves_current_steg_payload(&self) -> Result<bool, String> {
        let (Some(image), Some(info)) = (
            self.document.transformed_image.as_ref(),
            self.steganography.detected.as_ref(),
        ) else {
            return Ok(true);
        };

        let original_payload = bmp::runtime::steganography::extract(image, info)
            .map_err(|e| format!("failed to extract current payload before save-check: {e}"))?;

        let encoded = encode_rgba_to_bmp_ext(
            image,
            self.document.save_format,
            self.document.save_header_version,
            self.document.source_metadata.as_ref(),
        )
        .map_err(|e| format!("failed to encode save-check roundtrip: {e}"))?;

        let roundtrip = decode_to_rgba(&encoded).map_err(|e| format!("failed to decode save-check roundtrip: {e}"))?;

        let Some(round_info) = bmp::runtime::steganography::detect(&roundtrip) else {
            return Ok(false);
        };

        let round_payload = bmp::runtime::steganography::extract(&roundtrip, &round_info)
            .map_err(|e| format!("failed to extract payload after save-check roundtrip: {e}"))?;

        Ok(round_payload == original_payload)
    }

    /// Fast save-quality checks that avoid costly encode+decode roundtrips.
    fn save_quality_warning_reasons(&self) -> Vec<String> {
        let Some(image) = self.document.transformed_image.as_ref() else {
            return Vec::new();
        };

        let format = self.document.save_format;
        let has_transparency = image.pixels().any(|px| px[3] < u8::MAX);
        let mut reasons = Vec::new();

        if has_transparency && !matches!(format, SaveFormat::Rgb24 | SaveFormat::Rgb32 | SaveFormat::BitFields32) {
            reasons.push("Selected format/header does not preserve alpha; transparency will be lost".to_owned());
        }

        match format {
            SaveFormat::Rgb1 => {
                if unique_rgb_colors_exceed(image, 2) {
                    reasons.push("Image has more than 2 colors and will be quantized to 1-bpp palette".to_owned());
                }
            }
            SaveFormat::Rgb4 | SaveFormat::Rle4 => {
                if unique_rgb_colors_exceed(image, 16) {
                    reasons.push("Image has more than 16 colors and will be quantized to 4-bpp palette".to_owned());
                }
            }
            SaveFormat::Rgb8 | SaveFormat::Rle8 => {
                if unique_rgb_colors_exceed(image, 256) {
                    reasons.push("Image has more than 256 colors and will be quantized to 8-bpp palette".to_owned());
                }
            }
            SaveFormat::Rgb16 | SaveFormat::BitFields16Rgb555 => {
                if !all_pixels_exact_in_5bit_grid(image) {
                    reasons.push("RGB channels will be reduced to RGB555 precision".to_owned());
                }
            }
            SaveFormat::BitFields16Rgb565 => {
                if !all_pixels_exact_in_565_grid(image) {
                    reasons.push("RGB channels will be reduced to RGB565 precision".to_owned());
                }
            }
            SaveFormat::Rgb24 | SaveFormat::Rgb32 | SaveFormat::BitFields32 => {}
        }

        reasons
    }

    pub(crate) fn save_to_path(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        if self.pending_save.is_some() {
            "A save operation is already in progress".clone_into(&mut self.status);
            return;
        }

        if self.document.transformed_image.is_none() {
            "Nothing to save".clone_into(&mut self.status);
            return;
        }

        let mut reasons = Vec::new();

        // If the image contains steganography and the chosen format would
        // destroy it, warn before saving.
        if self.steganography.detected.is_some() {
            match self.save_preserves_current_steg_payload() {
                Ok(true) => {}
                Ok(false) => {
                    reasons.push(
                        "Roundtrip verification shows the selected format/header does not preserve the hidden steganography payload"
                            .to_owned(),
                    );
                }
                Err(err) => {
                    reasons.push(format!(
                        "Could not verify steganography preservation ({err}); saving may destroy hidden data"
                    ));
                }
            }
        }

        reasons.append(&mut self.save_quality_warning_reasons());

        if !reasons.is_empty() {
            self.steganography.save_confirm_pending = Some(path.to_path_buf());
            self.steganography.save_confirm_reason = Some(reasons.join("\n"));
            return;
        }

        self.do_save(ctx, path);
    }

    /// Performs the actual save unconditionally (called either from
    /// `save_to_path` when no steg is present, or after user confirms the
    /// steg-destroy dialog).
    pub(crate) fn do_save(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        if self.pending_save.is_some() {
            "A save operation is already in progress".clone_into(&mut self.status);
            return;
        }

        let Some(image) = self.document.transformed_image.as_ref() else {
            "Nothing to save".clone_into(&mut self.status);
            return;
        };

        let image = image.clone();
        let source = self.document.source_metadata.clone();
        let format = self.document.save_format;
        let header = self.document.save_header_version;
        let save_path = path.to_path_buf();
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let result = save_bmp_ext(&save_path, &image, format, header, source.as_ref()).map_err(|e| e.to_string());
            let _ = tx.send(SaveTaskResult {
                path: save_path,
                format,
                header,
                result,
            });
        });

        self.pending_save = Some(PendingSaveTask { rx });
        self.steganography.save_confirm_pending = None;
        self.steganography.save_confirm_reason = None;
        self.status = format!("Saving {}...", path.display());
        ctx.request_repaint();
    }

    fn poll_pending_save(&mut self, ctx: &egui::Context) {
        let Some(task) = self.pending_save.as_mut() else {
            return;
        };

        let outcome = match task.rx.try_recv() {
            Ok(done) => Some(done),
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(33));
                None
            }
            Err(TryRecvError::Disconnected) => {
                self.pending_save = None;
                "Save failed: worker disconnected".clone_into(&mut self.status);
                None
            }
        };

        let Some(done) = outcome else {
            return;
        };

        self.pending_save = None;
        self.steganography.save_confirm_pending = None;
        self.steganography.save_confirm_reason = None;

        match done.result {
            Ok(()) => {
                self.path_input = done.path.display().to_string();
                self.document.loaded_path = Some(done.path.clone());
                self.status = format!("Saved {} ({}, {})", done.path.display(), done.format, done.header);
                // Re-load from disk so metadata, original_image, and pipeline
                // all reflect the file as it was actually written.
                self.load_path(ctx, &done.path);
            }
            Err(err) => {
                self.status = format!("Save failed: {err}");
            }
        }
    }

    pub(crate) fn save_current(&mut self, ctx: &egui::Context) {
        if self.document.transformed_image.is_none() {
            "Nothing to save".clone_into(&mut self.status);
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
        let Some(path) = self.document.loaded_path.clone() else {
            "No file to overwrite".clone_into(&mut self.status);
            return;
        };

        self.save_to_path(ctx, &path);
    }

    /// Shows a confirmation dialog when the selected save settings may alter
    /// image data (e.g. quantization, alpha loss, steganography loss).
    ///
    /// Returns `true` while the dialog is still open (caller should skip other
    /// rendering that depends on interaction).
    pub(crate) fn show_save_confirm_window(&mut self, ctx: &egui::Context) {
        if self.steganography.save_confirm_pending.is_none() {
            return;
        }

        let reason = self.steganography.save_confirm_reason.clone().unwrap_or_else(|| {
            format!(
                "The selected settings ({}, {}) may alter image data",
                self.document.save_format, self.document.save_header_version
            )
        });

        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new("Warning: Save May Alter Image Data")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(400.0)
            .show(ctx, |ui| {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "The selected save settings do not preserve the currently displayed image exactly.",
                );
                ui.add_space(4.0);
                ui.label("Detected issues:");
                for line in reason.lines() {
                    ui.label(format!("- {line}"));
                }
                ui.add_space(8.0);
                ui.label("Save anyway?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save Anyway").clicked() {
                        confirmed = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                });
            });

        if confirmed {
            let path = self.steganography.save_confirm_pending.take().unwrap();
            self.do_save(ctx, &path);
        } else if cancelled {
            self.steganography.save_confirm_pending = None;
            self.steganography.save_confirm_reason = None;
        }
    }

    /// Shows confirmation before applying a transform that would likely
    /// corrupt a just-embedded steganography payload.
    pub(crate) fn show_steg_transform_confirm_window(&mut self, ctx: &egui::Context) {
        let Some(op) = self.steganography.transform_confirm_pending.as_ref() else {
            return;
        };

        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new("Warning: Transform May Corrupt Steganography")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "This transform is being applied on top of an embedded steganography payload.",
                );
                ui.add_space(4.0);
                ui.label("That will likely destroy or corrupt the hidden data.");
                ui.label(format!("Pending transform: {op}"));
                ui.add_space(8.0);
                ui.label("Apply anyway?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Apply Anyway").clicked() {
                        confirmed = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                });
            });

        if confirmed {
            if let Some(op) = self.steganography.transform_confirm_pending.take() {
                self.steganography.overwrite_warned = true;
                self.apply_transform_now(ctx, op);
            }
        } else if cancelled {
            self.steganography.transform_confirm_pending = None;
        }
    }
}

fn unique_rgb_colors_exceed(image: &DecodedImage, limit: usize) -> bool {
    let mut unique = HashSet::with_capacity(limit.saturating_add(1));
    for px in image.pixels() {
        unique.insert([px[0], px[1], px[2]]);
        if unique.len() > limit {
            return true;
        }
    }
    false
}

/// Returns `true` if every pixel in the image lies exactly on the RGB555
/// quantization grid.
///
/// The check simulates a roundtrip through 5-bit color precision:
///
/// 1. Each RGB channel is reduced from 8 bits to 5 bits using
///    integer rounding.
/// 2. The value is then expanded back to 8 bits.
/// 3. If the reconstructed value matches the original channel value,
///    that channel is representable exactly in RGB555.
///
/// If all pixels pass this test, encoding the image to RGB555 will not
/// introduce any color quantization error.
///
/// This function is used as a fast pre-check for save formats that store
/// colors in 5-bit channels (e.g. `RGB555`) so the application can warn
/// the user when precision would be lost.
fn all_pixels_exact_in_5bit_grid(image: &DecodedImage) -> bool {
    image.pixels().all(|px| {
        // Lossy convert color bits to 0..=32 range (5-bit)
        let r5 = (u16::from(px[0]) * 31 + 127) / 255;
        let g5 = (u16::from(px[1]) * 31 + 127) / 255;
        let b5 = (u16::from(px[2]) * 31 + 127) / 255;

        // Get back an equivalent 0..=255 (8-bit) value from the reduced vals
        // Safe: The casts in here never truncate, calculation guarantees u8 range
        #[allow(clippy::cast_possible_truncation)]
        let r8 = ((r5 * 255 + 15) / 31) as u8;
        #[allow(clippy::cast_possible_truncation)]
        let g8 = ((g5 * 255 + 15) / 31) as u8;
        #[allow(clippy::cast_possible_truncation)]
        let b8 = ((b5 * 255 + 15) / 31) as u8;

        // Check whether the original color values match the reduced range values
        px[0] == r8 && px[1] == g8 && px[2] == b8
    })
}

/// Returns `true` if every pixel in the image lies exactly on the RGB565
/// quantization grid.
///
/// RGB565 stores colors using:
///
/// - 5 bits for red
/// - 6 bits for green
/// - 5 bits for blue
///
/// This function checks whether each channel would survive a
/// quantization roundtrip without change:
///
/// 1. Convert the 8-bit channel to its reduced precision (5 or 6 bits)
///    using integer rounding.
/// 2. Expand the reduced value back to 8 bits.
/// 3. Compare with the original channel value.
///
/// If all pixels pass, saving the image as RGB565 will not introduce
/// color quantization artifacts.
///
/// Used by save-quality checks to warn the user when converting an
/// image to RGB565 would reduce color precision.
fn all_pixels_exact_in_565_grid(image: &DecodedImage) -> bool {
    image.pixels().all(|px| {
        // Lossy convert color bits to 0..=32 (5-bit) / 0..=128 (6-bit) range
        let r5 = (u16::from(px[0]) * 31 + 127) / 255;
        let g6 = (u16::from(px[1]) * 63 + 127) / 255;
        let b5 = (u16::from(px[2]) * 31 + 127) / 255;

        // Get back an equivalent 0..=255 (8-bit) value from the reduced vals
        // Safe: The casts in here never truncate, calculation guarantees u8 range
        #[allow(clippy::cast_possible_truncation)]
        let r8 = ((r5 * 255 + 15) / 31) as u8;
        #[allow(clippy::cast_possible_truncation)]
        let g8 = ((g6 * 255 + 31) / 63) as u8;
        #[allow(clippy::cast_possible_truncation)]
        let b8 = ((b5 * 255 + 15) / 31) as u8;

        // Check whether the original color values match the reduced range values
        px[0] == r8 && px[1] == g8 && px[2] == b8
    })
}

impl eframe::App for BmpViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_pending_save(ctx);

        // --- Global keyboard shortcuts ---
        let text_has_focus = ctx.memory(|m| m.focused().is_some());
        let kb = ctx.input(|i| {
            let cmd = i.modifiers.command; // Ctrl on Linux/Windows, Cmd on macOS
            let shift = i.modifiers.shift;
            (
                cmd && i.key_pressed(egui::Key::O),                              // Open
                cmd && !shift && i.key_pressed(egui::Key::S),                    // Save
                cmd && shift && i.key_pressed(egui::Key::S),                     // Save As
                !text_has_focus && cmd && !shift && i.key_pressed(egui::Key::Z), // Undo
                !text_has_focus
                    && cmd
                    && (shift && i.key_pressed(egui::Key::Z) // Redo (Ctrl+Shift+Z)
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
        if let Some(file) = dropped_files.first()
            && let Some(path) = &file.path
        {
            self.path_input = path.display().to_string();
            self.load_path(ctx, path);
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
        if let Some(op) = self.show_steg_embed_window(ctx) {
            self.apply_and_refresh(ctx, op);
        }
        if let Some(op) = self.show_steg_inspect_window(ctx) {
            self.apply_and_refresh(ctx, op);
        }
        self.show_steg_transform_confirm_window(ctx);
        self.show_save_confirm_window(ctx);

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
