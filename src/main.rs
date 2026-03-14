use std::{fs::File, path::PathBuf};

use bmp::{
    raw::Bmp,
    runtime::{
        decode::{DecodedImage, decode_to_rgba},
        encode::{SaveFormat, SaveHeaderVersion, SourceMetadata, encode_rgba_to_bmp_ext, save_bmp_ext},
        steganography::{self, StegInfo},
        transform::{ImageTransform, RotationInterpolation, TransformPipeline, TranslateMode, apply_transform},
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
    pub(crate) pipeline: TransformPipeline,
    /// Transforms that were undone, available for redo. Cleared on new transform or step removal.
    pub(crate) redo_stack: Vec<ImageTransform>,
    pub(crate) save_format: SaveFormat,
    pub(crate) save_header_version: SaveHeaderVersion,
    pub(crate) source_metadata: Option<SourceMetadata>,
    /// Path of the currently loaded file (for "Save" without a dialog).
    pub(crate) loaded_path: Option<PathBuf>,
}

/// Viewport-related UI state for the central image panel.
pub(crate) struct ViewportState {
    /// Cached GPU texture for the currently displayed decoded image.
    pub(crate) texture: Option<egui::TextureHandle>,
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

/// State for the custom kernel editor dialog.
pub(crate) struct KernelToolState {
    /// Whether the custom kernel editor window is open.
    pub(crate) open: bool,
    /// Side length of the custom kernel being edited (1, 3, 5, or 7).
    pub(crate) size: usize,
    /// Per-cell weight strings for the kernel editor (size*size elements).
    pub(crate) weights: Vec<String>,
    /// Divisor string for the kernel editor.
    pub(crate) divisor: String,
    /// Bias string for the kernel editor.
    pub(crate) bias: String,
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
    /// Active crop drag mode for visual crop manipulation.
    pub(crate) drag_mode: Option<CropDragMode>,
    /// Pointer position in image coordinates at drag start.
    pub(crate) drag_start_image: Option<egui::Pos2>,
    /// Crop rect (x, y, w, h) snapshot at drag start.
    pub(crate) drag_start_rect: Option<(u32, u32, u32, u32)>,
}

/// Steganography-related detection state and window inputs.
pub(crate) struct SteganographyUiState {
    /// Steganography detected in the current transformed image, if any.
    pub(crate) detected: Option<StegInfo>,
    /// Whether the "Embed Steganography" window is open.
    pub(crate) embed_open: bool,
    /// Whether the "Inspect Steganography" window is open.
    pub(crate) inspect_open: bool,
    /// Whether we already warned the user this frame that a transform was
    /// applied on top of an embedded steg payload.
    /// Reset to `false` whenever the pipeline's top-most op is no longer steg.
    pub(crate) overwrite_warned: bool,
    /// Path awaiting save confirmation because it would destroy steganography.
    pub(crate) save_confirm_pending: Option<std::path::PathBuf>,
    /// Human-readable reason shown in the save confirmation dialog.
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

pub(crate) struct BmpViewerApp {
    pub(crate) path_input: String,
    /// UI feedback/status message shown in toolbar.
    pub(crate) status: String,
    pub(crate) document: DocumentState,
    pub(crate) viewport: ViewportState,
    pub(crate) transforms: TransformToolState,
    pub(crate) steganography: SteganographyUiState,
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
                pipeline: TransformPipeline::default(),
                redo_stack: Vec::new(),
                save_format: SaveFormat::default(),
                save_header_version: SaveHeaderVersion::default(),
                source_metadata: None,
                loaded_path: None,
            },
            viewport: ViewportState {
                texture: None,
                zoom: 0.0,
                last_effective_zoom: 1.0,
                hovered_pixel: None,
                pan_offset: egui::Vec2::ZERO,
            },
            transforms: TransformToolState {
                kernel: KernelToolState {
                    open: false,
                    size: 3,
                    weights: vec!["0".to_owned(); 9],
                    divisor: "1".to_owned(),
                    bias: "0".to_owned(),
                },
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
                    drag_mode: None,
                    drag_start_image: None,
                    drag_start_rect: None,
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
        }
    }
}

impl BmpViewerApp {
    fn validate_embed_fits_current_image(
        &self,
        current: &DecodedImage,
        op: &ImageTransform,
    ) -> Result<(), steganography::StegError> {
        let ImageTransform::EmbedSteganography { config, payload } = op else {
            return Ok(());
        };
        steganography::embed(current, *config, payload).map(|_| ())
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
        let color =
            egui::ColorImage::from_rgba_unmultiplied([image.width as usize, image.height as usize], &image.rgba);
        self.viewport.texture = Some(ctx.load_texture(label, color, egui::TextureOptions::NEAREST));
        self.document.transformed_image = Some(image);
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

        self.document.pipeline.clear();
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
        self.document.loaded_path = Some(path.clone());
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

    fn apply_transform_now(&mut self, ctx: &egui::Context, op: ImageTransform) {
        if let Some(current) = self.document.transformed_image.as_ref() {
            if matches!(op, ImageTransform::EmbedSteganography { .. }) {
                self.steganography.overwrite_warned = false;
            }

            if let Err(err) = self.validate_embed_fits_current_image(current, &op) {
                self.status = format!(
                    "Embedding aborted: payload no longer fits current image ({}). The steganography transform was not applied.",
                    err
                );
                return;
            }

            let next = apply_transform(current, &op);
            self.document.pipeline.push(op, Some(current));
            self.document.redo_stack.clear();
            self.update_transformed_image(ctx, next);
        }
    }

    pub(crate) fn apply_and_refresh(&mut self, ctx: &egui::Context, op: ImageTransform) {
        let should_confirm_overwrite = !self.steganography.overwrite_warned
            && !matches!(
                op,
                ImageTransform::EmbedSteganography { .. } | ImageTransform::RemoveSteganography { .. }
            )
            && matches!(
                self.document.pipeline.ops().last(),
                Some(ImageTransform::EmbedSteganography { .. })
            );

        if should_confirm_overwrite {
            self.steganography.transform_confirm_pending = Some(op);
            return;
        }

        // Keep the public API shape unchanged for all callers.
        self.apply_transform_now(ctx, op);
    }

    pub(crate) fn undo_transform(&mut self, ctx: &egui::Context) {
        if let Some(op) = self.document.pipeline.pop() {
            if let Some(inv) = op.inverse() {
                self.document.redo_stack.push(op);
                // O(1) path: apply the inverse transform.
                if let Some(current) = self.document.transformed_image.as_ref() {
                    let result = apply_transform(current, &inv);
                    self.update_transformed_image(ctx, result);
                }
            } else {
                self.document.redo_stack.push(op);
                // Lossy transform: replay the remaining pipeline from the original image.
                if let Some(original) = self.document.original_image.as_ref() {
                    let (result, warnings) = self.document.pipeline.apply_with_warnings(original);
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
        if let Some(op) = self.document.redo_stack.pop()
            && let Some(current) = self.document.transformed_image.as_ref()
        {
            if let Err(err) = self.validate_embed_fits_current_image(current, &op) {
                self.status = format!(
                    "Redo skipped: steganography payload no longer fits after prior edits ({}). The embed step was dropped.",
                    err
                );
                return;
            }

            let next = apply_transform(current, &op);
            self.document.pipeline.push(op, Some(current));
            self.update_transformed_image(ctx, next);
        }
    }

    /// Returns whether the currently selected save settings preserve the exact
    /// embedded steganography payload, determined by an in-memory roundtrip.
    fn save_preserves_current_steg_payload(&self) -> Result<bool, String> {
        let (image, info) = match (
            self.document.transformed_image.as_ref(),
            self.steganography.detected.as_ref(),
        ) {
            (Some(img), Some(info)) => (img, info),
            _ => return Ok(true),
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

    pub(crate) fn save_to_path(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        if self.document.transformed_image.is_none() {
            self.status = "Nothing to save".to_owned();
            return;
        }

        // If the image contains steganography and the chosen format would
        // destroy it, open the confirmation dialog instead of saving immediately.
        if self.steganography.detected.is_some() {
            match self.save_preserves_current_steg_payload() {
                Ok(true) => {}
                Ok(false) => {
                    self.steganography.save_confirm_pending = Some(path.to_path_buf());
                    self.steganography.save_confirm_reason = Some(
                        "Roundtrip verification shows the selected format/header does not preserve the hidden payload"
                            .to_owned(),
                    );
                    return;
                }
                Err(err) => {
                    // Conservative fallback: if verification fails, require explicit consent.
                    self.steganography.save_confirm_pending = Some(path.to_path_buf());
                    self.steganography.save_confirm_reason = Some(format!(
                        "Could not verify steganography preservation ({err}); saving may destroy hidden data"
                    ));
                    return;
                }
            }
        }

        self.do_save(ctx, path);
    }

    /// Performs the actual save unconditionally (called either from
    /// `save_to_path` when no steg is present, or after user confirms the
    /// steg-destroy dialog).
    pub(crate) fn do_save(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        let Some(image) = self.document.transformed_image.as_ref() else {
            self.status = "Nothing to save".to_owned();
            return;
        };

        match save_bmp_ext(
            path,
            image,
            self.document.save_format,
            self.document.save_header_version,
            self.document.source_metadata.as_ref(),
        ) {
            Ok(()) => {
                self.steganography.save_confirm_pending = None;
                self.steganography.save_confirm_reason = None;
                let saved_path = path.to_path_buf();
                self.path_input = saved_path.display().to_string();
                self.document.loaded_path = Some(saved_path.clone());
                self.status = format!(
                    "Saved {} ({}, {})",
                    saved_path.display(),
                    self.document.save_format,
                    self.document.save_header_version
                );
                // Re-load from disk so metadata, original_image, and pipeline
                // all reflect the file as it was actually written.
                self.load_path(ctx, saved_path);
            }
            Err(err) => {
                self.steganography.save_confirm_pending = None;
                self.steganography.save_confirm_reason = None;
                self.status = format!("Save failed: {err}");
            }
        }
    }

    pub(crate) fn save_current(&mut self, ctx: &egui::Context) {
        if self.document.transformed_image.is_none() {
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
        let Some(path) = self.document.loaded_path.clone() else {
            self.status = "No file to overwrite".to_owned();
            return;
        };

        self.save_to_path(ctx, &path);
    }

    /// Shows a confirmation dialog when the user is about to save in a format
    /// that would destroy an embedded steganography payload.
    ///
    /// Returns `true` while the dialog is still open (caller should skip other
    /// rendering that depends on interaction).
    pub(crate) fn show_steg_save_confirm_window(&mut self, ctx: &egui::Context) {
        if self.steganography.save_confirm_pending.is_none() {
            return;
        }

        let reason = self.steganography.save_confirm_reason.clone().unwrap_or_else(|| {
            format!(
                "The selected settings ({}, {}) are likely to overwrite LSB data",
                self.document.save_format, self.document.save_header_version
            )
        });

        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new("Warning: Steganography May Be Corrupted")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(400.0)
            .show(ctx, |ui| {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "The selected save format may permanently corrupt the embedded steganographic payload.",
                );
                ui.add_space(4.0);
                ui.label(format!("Reason: {reason}."));
                ui.add_space(8.0);
                ui.label("Save anyway and lose the hidden data?");
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
        if let Some(file) = dropped_files.first()
            && let Some(path) = &file.path
        {
            self.path_input = path.display().to_string();
            self.load_path(ctx, path.clone());
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
        self.show_steg_save_confirm_window(ctx);

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
