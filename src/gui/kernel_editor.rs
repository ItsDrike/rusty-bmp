use eframe::egui;

use bmp::runtime::transform::{ConvolutionCustom, ImageTransform, Kernel};

use crate::BmpViewerApp;

impl BmpViewerApp {
    /// Resizes the custom kernel weight grid to match `custom_kernel_size`.
    ///
    /// When growing, new cells default to "0". When shrinking, excess cells
    /// are discarded. The weights are stored in row-major order.
    pub(crate) fn resize_kernel_weights(&mut self) {
        let n = self.transforms.kernel.size;
        let needed = n * n;
        self.transforms.kernel.weights.resize(needed, "0".to_owned());
        self.transforms.kernel.weights.truncate(needed);
    }

    /// Shows the custom kernel editor as a floating `egui::Window`.
    ///
    /// Called from the main `update()` method. Returns `Some(op)` when the
    /// user clicks "Apply" with a valid kernel, so the caller can apply it
    /// via `apply_and_refresh`.
    pub(crate) fn show_kernel_editor(&mut self, ctx: &egui::Context) -> Option<ImageTransform> {
        if !self.transforms.kernel.open {
            return None;
        }

        let mut open = self.transforms.kernel.open;
        let mut apply = false;
        let mut close_requested = false;

        egui::Window::new("Custom Convolution Kernel")
            .open(&mut open)
            .resizable(true)
            .default_width(340.0)
            .show(ctx, |ui| {
                // --- Size selector ---
                ui.horizontal(|ui| {
                    ui.label("Kernel size:");
                    let old_size = self.transforms.kernel.size;
                    egui::ComboBox::from_id_salt("kernel_size")
                        .selected_text(format!(
                            "{}x{}",
                            self.transforms.kernel.size, self.transforms.kernel.size
                        ))
                        .show_ui(ui, |ui| {
                            for &s in &[1, 3, 5, 7] {
                                ui.selectable_value(&mut self.transforms.kernel.size, s, format!("{s}x{s}"));
                            }
                        });
                    if self.transforms.kernel.size != old_size {
                        self.resize_kernel_weights();
                    }
                });

                ui.add_space(4.0);

                // --- Weight grid ---
                let n = self.transforms.kernel.size;
                let half = n / 2;
                egui::Grid::new("kernel_weights_grid")
                    .spacing([4.0, 4.0])
                    .show(ui, |ui| {
                        for y in 0..n {
                            for x in 0..n {
                                let idx = y * n + x;
                                let is_center = x == half && y == half;
                                let widget = egui::TextEdit::singleline(&mut self.transforms.kernel.weights[idx])
                                    .desired_width(36.0)
                                    .horizontal_align(egui::Align::Center);
                                let response = ui.add(widget);
                                if is_center {
                                    // Highlight the center cell with a yellow outline.
                                    let rect = response.rect.expand(1.0);
                                    ui.painter().rect_stroke(
                                        rect,
                                        2.0,
                                        egui::Stroke::new(1.5, egui::Color32::YELLOW),
                                        egui::epaint::StrokeKind::Outside,
                                    );
                                }
                            }
                            ui.end_row();
                        }
                    });

                ui.add_space(4.0);

                // --- Divisor & Bias ---
                ui.horizontal(|ui| {
                    ui.label("Divisor:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.transforms.kernel.divisor)
                            .desired_width(50.0)
                            .horizontal_align(egui::Align::Center),
                    );
                    ui.label("Bias:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.transforms.kernel.bias)
                            .desired_width(50.0)
                            .horizontal_align(egui::Align::Center),
                    );
                });

                // --- Preset load buttons ---
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Load preset:");
                    if ui.small_button("Blur").clicked() {
                        self.load_kernel_preset(&[1, 2, 1, 2, 4, 2, 1, 2, 1], 3, 16, 0);
                    }
                    if ui.small_button("Sharpen").clicked() {
                        self.load_kernel_preset(&[0, -1, 0, -1, 5, -1, 0, -1, 0], 3, 1, 0);
                    }
                    if ui.small_button("Edge").clicked() {
                        self.load_kernel_preset(&[-1, -1, -1, -1, 8, -1, -1, -1, -1], 3, 1, 0);
                    }
                    if ui.small_button("Emboss").clicked() {
                        self.load_kernel_preset(&[-2, -1, 0, -1, 1, 1, 0, 1, 2], 3, 1, 128);
                    }
                    if ui.small_button("Identity").clicked() {
                        self.load_kernel_preset(&[0, 0, 0, 0, 1, 0, 0, 0, 0], 3, 1, 0);
                    }
                });

                ui.add_space(8.0);

                // --- Validation & Apply ---
                let validation = self.validate_custom_kernel();
                match &validation {
                    Ok(_) => {
                        ui.colored_label(egui::Color32::GREEN, "Valid kernel");
                    }
                    Err(msg) => {
                        ui.colored_label(egui::Color32::RED, msg);
                    }
                }

                ui.horizontal(|ui| {
                    if ui.add_enabled(validation.is_ok(), egui::Button::new("Apply")).clicked() {
                        apply = true;
                    }
                    if ui.button("Close").clicked() {
                        close_requested = true;
                    }
                });
            });

        self.transforms.kernel.open = open && !close_requested;

        if apply {
            self.validate_custom_kernel()
                .ok()
                .map(|kernel| ConvolutionCustom { kernel }.into())
        } else {
            None
        }
    }

    /// Validates the current kernel editor fields and returns the `Kernel` or
    /// an error message.
    fn validate_custom_kernel(&self) -> Result<Kernel, String> {
        let n = self.transforms.kernel.size;
        let expected = n * n;

        if self.transforms.kernel.weights.len() != expected {
            return Err(format!(
                "Expected {} weights, got {}",
                expected,
                self.transforms.kernel.weights.len()
            ));
        }

        let mut weights = Vec::with_capacity(expected);
        for (i, s) in self.transforms.kernel.weights.iter().enumerate() {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Err(format!("Weight cell {} is empty", i + 1));
            }
            if let Ok(v) = trimmed.parse::<i32>() {
                weights.push(v);
            } else {
                let row = i / n;
                let col = i % n;
                return Err(format!(
                    "Invalid integer at row {}, col {}: \"{}\"",
                    row + 1,
                    col + 1,
                    trimmed
                ));
            }
        }

        let divisor: i32 = self
            .transforms
            .kernel
            .divisor
            .trim()
            .parse()
            .map_err(|_| format!("Invalid divisor: \"{}\"", self.transforms.kernel.divisor.trim()))?;

        if divisor == 0 {
            return Err("Divisor must not be zero".to_owned());
        }

        let bias: i32 = self
            .transforms
            .kernel
            .bias
            .trim()
            .parse()
            .map_err(|_| format!("Invalid bias: \"{}\"", self.transforms.kernel.bias.trim()))?;

        Ok(Kernel::new(weights, n, divisor, bias))
    }

    /// Loads a preset kernel into the editor fields.
    fn load_kernel_preset(&mut self, weights: &[i32], size: usize, divisor: i32, bias: i32) {
        self.transforms.kernel.size = size;
        self.transforms.kernel.weights = weights.iter().map(std::string::ToString::to_string).collect();
        self.transforms.kernel.divisor = divisor.to_string();
        self.transforms.kernel.bias = bias.to_string();
    }
}
