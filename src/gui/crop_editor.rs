use eframe::egui;

use bmp::runtime::transform::{Crop, ImageTransform};

use crate::BmpViewerApp;

impl BmpViewerApp {
    pub(crate) fn open_crop_window(&mut self) {
        let Some(image) = self.document.transformed_image.as_ref() else {
            "Load an image first".clone_into(&mut self.status);
            return;
        };

        self.transforms.crop.x = (image.width().saturating_sub(1)) / 2;
        self.transforms.crop.y = (image.height().saturating_sub(1)) / 2;
        self.transforms.crop.width = image.width().max(1);
        self.transforms.crop.height = image.height().max(1);
        self.transforms.crop.drag_mode = None;
        self.transforms.crop.drag_start_image = None;
        self.transforms.crop.drag_start_rect = None;
        self.transforms.crop.open = true;
    }

    pub(crate) fn show_crop_window(&mut self, ctx: &egui::Context) -> Option<ImageTransform> {
        if !self.transforms.crop.open {
            return None;
        }

        let Some(image) = self.document.transformed_image.as_ref() else {
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
                        let ratio = img_h as f32 / img_w as f32;
                        self.transforms.crop.height =
                            ((self.transforms.crop.width as f32 * ratio).round().max(1.0)) as u32;
                    }
                    if self.transforms.crop.keep_aspect
                        && height_resp.changed()
                        && !width_resp.has_focus()
                        && img_h > 0
                    {
                        let ratio = img_w as f32 / img_h as f32;
                        self.transforms.crop.width =
                            ((self.transforms.crop.height as f32 * ratio).round().max(1.0)) as u32;
                    }
                });

                ui.horizontal(|ui| {
                    let keep_aspect_resp = ui.checkbox(&mut self.transforms.crop.keep_aspect, "Keep aspect ratio");
                    if self.transforms.crop.keep_aspect && keep_aspect_resp.changed() && img_w > 0 {
                        let ratio = img_h as f32 / img_w as f32;
                        self.transforms.crop.height =
                            ((self.transforms.crop.width as f32 * ratio).round().max(1.0)) as u32;
                    }
                });

                self.clamp_crop_inputs(img_w, img_h);

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

                self.clamp_crop_inputs(img_w, img_h);

                let (x, y, w, h) = clamped_crop_rect(
                    self.transforms.crop.x,
                    self.transforms.crop.y,
                    self.transforms.crop.width,
                    self.transforms.crop.height,
                    img_w,
                    img_h,
                );

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

        let (x, y, width, height) = clamped_crop_rect(
            self.transforms.crop.x,
            self.transforms.crop.y,
            self.transforms.crop.width,
            self.transforms.crop.height,
            img_w,
            img_h,
        );

        Some(Crop { x, y, width, height }.into())
    }

    fn clamp_crop_inputs(&mut self, img_w: u32, img_h: u32) {
        self.transforms.crop.x = self.transforms.crop.x.min(img_w.saturating_sub(1));
        self.transforms.crop.y = self.transforms.crop.y.min(img_h.saturating_sub(1));
        self.transforms.crop.width = self.transforms.crop.width.max(1).min(img_w.max(1));
        self.transforms.crop.height = self.transforms.crop.height.max(1).min(img_h.max(1));
    }

    /// Returns the clamped crop rectangle (top-left + size) for the given image size.
    ///
    /// Crop dialog fields are center-based; this converts them to the runtime crop
    /// rectangle representation used by the transform and by viewer preview overlays.
    pub(crate) fn crop_rect_for_image(&self, img_w: u32, img_h: u32) -> (u32, u32, u32, u32) {
        clamped_crop_rect(
            self.transforms.crop.x,
            self.transforms.crop.y,
            self.transforms.crop.width,
            self.transforms.crop.height,
            img_w,
            img_h,
        )
    }

    /// Sets center-based crop inputs from a top-left crop rectangle.
    pub(crate) fn set_crop_from_rect(&mut self, x: u32, y: u32, width: u32, height: u32, img_w: u32, img_h: u32) {
        let w = width.max(1).min(img_w.max(1));
        let h = height.max(1).min(img_h.max(1));
        let x0 = x.min(img_w.saturating_sub(w));
        let y0 = y.min(img_h.saturating_sub(h));
        self.transforms.crop.width = w;
        self.transforms.crop.height = h;
        self.transforms.crop.x = (x0 + w / 2).min(img_w.saturating_sub(1));
        self.transforms.crop.y = (y0 + h / 2).min(img_h.saturating_sub(1));
    }
}

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
