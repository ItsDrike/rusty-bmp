use eframe::egui;

use bmp::runtime::transform::ImageTransform;

use crate::BmpViewerApp;

impl BmpViewerApp {
    pub(crate) fn open_crop_window(&mut self) {
        let Some(image) = self.transformed_image.as_ref() else {
            self.status = "Load an image first".to_owned();
            return;
        };

        self.crop_x = (image.width.saturating_sub(1)) / 2;
        self.crop_y = (image.height.saturating_sub(1)) / 2;
        self.crop_width = image.width.max(1);
        self.crop_height = image.height.max(1);
        self.crop_open = true;
    }

    pub(crate) fn show_crop_window(&mut self, ctx: &egui::Context) -> Option<ImageTransform> {
        if !self.crop_open {
            return None;
        }

        let Some(image) = self.transformed_image.as_ref() else {
            self.crop_open = false;
            return None;
        };
        let img_w = image.width;
        let img_h = image.height;

        let mut open = self.crop_open;
        let mut apply = false;
        let mut close_requested = false;

        egui::Window::new("Crop")
            .open(&mut open)
            .resizable(false)
            .default_width(340.0)
            .show(ctx, |ui| {
                ui.label(format!("Image size: {}x{}", img_w, img_h));
                ui.small("x/y are crop center coordinates.");
                ui.add_space(6.0);

                // Persisted crop fields are intentionally kept on app state so the
                // viewer can later reuse them for an interactive overlay preview.
                ui.horizontal(|ui| {
                    ui.label("center x:");
                    ui.add(
                        egui::DragValue::new(&mut self.crop_x)
                            .speed(1)
                            .range(0..=img_w.saturating_sub(1)),
                    );
                    ui.label("center y:");
                    ui.add(
                        egui::DragValue::new(&mut self.crop_y)
                            .speed(1)
                            .range(0..=img_h.saturating_sub(1)),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("width:");
                    let width_resp = ui.add(
                        egui::DragValue::new(&mut self.crop_width)
                            .speed(1)
                            .range(1..=img_w.max(1)),
                    );
                    ui.label("height:");
                    let height_resp = ui.add(
                        egui::DragValue::new(&mut self.crop_height)
                            .speed(1)
                            .range(1..=img_h.max(1)),
                    );

                    if self.crop_keep_aspect && width_resp.changed() && !height_resp.has_focus() && img_w > 0 {
                        let ratio = img_h as f32 / img_w as f32;
                        self.crop_height = ((self.crop_width as f32 * ratio).round().max(1.0)) as u32;
                    }
                    if self.crop_keep_aspect && height_resp.changed() && !width_resp.has_focus() && img_h > 0 {
                        let ratio = img_w as f32 / img_h as f32;
                        self.crop_width = ((self.crop_height as f32 * ratio).round().max(1.0)) as u32;
                    }
                });

                ui.horizontal(|ui| {
                    let keep_aspect_resp = ui.checkbox(&mut self.crop_keep_aspect, "Keep aspect ratio");
                    if self.crop_keep_aspect && keep_aspect_resp.changed() && img_w > 0 {
                        let ratio = img_h as f32 / img_w as f32;
                        self.crop_height = ((self.crop_width as f32 * ratio).round().max(1.0)) as u32;
                    }
                });

                self.clamp_crop_inputs(img_w, img_h);

                ui.horizontal(|ui| {
                    if ui.small_button("Reset full").clicked() {
                        self.crop_x = (img_w.saturating_sub(1)) / 2;
                        self.crop_y = (img_h.saturating_sub(1)) / 2;
                        self.crop_width = img_w.max(1);
                        self.crop_height = img_h.max(1);
                    }
                    if ui.small_button("Center 50%").clicked() {
                        self.crop_width = (img_w / 2).max(1);
                        self.crop_height = (img_h / 2).max(1);
                        self.crop_x = (img_w.saturating_sub(1)) / 2;
                        self.crop_y = (img_h.saturating_sub(1)) / 2;
                    }
                });

                self.clamp_crop_inputs(img_w, img_h);

                let (x, y, w, h) = clamped_crop_rect(
                    self.crop_x,
                    self.crop_y,
                    self.crop_width,
                    self.crop_height,
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

        self.crop_open = open && !close_requested;

        if !apply {
            return None;
        }

        let (x, y, width, height) = clamped_crop_rect(
            self.crop_x,
            self.crop_y,
            self.crop_width,
            self.crop_height,
            img_w,
            img_h,
        );

        Some(ImageTransform::Crop { x, y, width, height })
    }

    fn clamp_crop_inputs(&mut self, img_w: u32, img_h: u32) {
        self.crop_x = self.crop_x.min(img_w.saturating_sub(1));
        self.crop_y = self.crop_y.min(img_h.saturating_sub(1));
        self.crop_width = self.crop_width.max(1).min(img_w.max(1));
        self.crop_height = self.crop_height.max(1).min(img_h.max(1));
    }

    /// Returns the clamped crop rectangle (top-left + size) for the given image size.
    ///
    /// Crop dialog fields are center-based; this converts them to the runtime crop
    /// rectangle representation used by the transform and by viewer preview overlays.
    pub(crate) fn crop_rect_for_image(&self, img_w: u32, img_h: u32) -> (u32, u32, u32, u32) {
        clamped_crop_rect(
            self.crop_x,
            self.crop_y,
            self.crop_width,
            self.crop_height,
            img_w,
            img_h,
        )
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
