//! Custom convolution kernel editor state and window UI.

use eframe::egui;

use bmp::runtime::transform::{ConvolutionCustom, ImageTransform, Kernel};

use crate::gui::BmpViewerApp;

/// Allowed side lengths for the custom convolution kernel editor.
#[derive(Clone, Copy, PartialEq, Eq)]
enum KernelSize {
    One,
    Three,
    Five,
    Seven,
}

impl KernelSize {
    const ALL: [Self; 4] = [Self::One, Self::Three, Self::Five, Self::Seven];

    const fn as_usize(self) -> usize {
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
pub(in crate::gui) struct KernelToolState {
    /// Whether the custom kernel editor window is open.
    pub(in crate::gui) open: bool,
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
    pub(in crate::gui) fn new() -> Self {
        Self {
            open: false,
            size: KernelSize::Three,
            weights: vec!["0".to_owned(); KernelSize::Three.cell_count()],
            divisor: "1".to_owned(),
            bias: "0".to_owned(),
        }
    }

    const fn size(&self) -> KernelSize {
        self.size
    }

    const fn size_value(&self) -> usize {
        self.size.as_usize()
    }

    fn resize_preserving(&mut self, new_size: KernelSize) {
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

    fn weights(&self) -> &[String] {
        &self.weights
    }

    fn weights_mut(&mut self) -> &mut [String] {
        &mut self.weights
    }

    fn set_weights(&mut self, weights: Vec<String>) {
        assert_eq!(weights.len(), self.size.cell_count());
        self.weights = weights;
    }

    fn divisor(&self) -> &str {
        &self.divisor
    }

    const fn divisor_mut(&mut self) -> &mut String {
        &mut self.divisor
    }

    fn set_divisor(&mut self, divisor: String) {
        self.divisor = divisor;
    }

    fn bias(&self) -> &str {
        &self.bias
    }

    const fn bias_mut(&mut self) -> &mut String {
        &mut self.bias
    }

    fn set_bias(&mut self, bias: String) {
        self.bias = bias;
    }

    /// Validates the editor contents and converts them into a runtime kernel.
    fn validate(&self) -> Result<Kernel, String> {
        let n = self.size_value();
        let expected = n * n;

        if self.weights().len() != expected {
            return Err(format!("Expected {} weights, got {}", expected, self.weights().len()));
        }

        let mut weights = Vec::with_capacity(expected);
        for (i, s) in self.weights().iter().enumerate() {
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
            .divisor()
            .trim()
            .parse()
            .map_err(|_| format!("Invalid divisor: \"{}\"", self.divisor().trim()))?;

        if divisor == 0 {
            return Err("Divisor must not be zero".to_owned());
        }

        let bias: i32 = self
            .bias()
            .trim()
            .parse()
            .map_err(|_| format!("Invalid bias: \"{}\"", self.bias().trim()))?;

        Kernel::new(weights, n, divisor, bias).map_err(|err| err.to_string())
    }

    /// Returns a 1D Gaussian row approximation for the selected kernel size.
    fn gaussian_row(size: usize) -> Vec<i32> {
        #[expect(clippy::match_same_arms)]
        match size {
            1 => vec![1],
            3 => vec![1, 2, 1],
            5 => vec![1, 4, 6, 4, 1],
            7 => vec![1, 6, 15, 20, 15, 6, 1],
            _ => vec![1],
        }
    }

    /// Builds a separable blur kernel and matching divisor for the selected size.
    fn blur_kernel_for_size(size: usize) -> (Vec<i32>, i32) {
        let row = Self::gaussian_row(size);
        let row_sum: i32 = row.iter().sum();
        let mut weights = Vec::with_capacity(size * size);
        for y in 0..size {
            for x in 0..size {
                weights.push(row[y] * row[x]);
            }
        }
        (weights, row_sum * row_sum)
    }

    /// Replaces the current editor fields with one of the built-in presets.
    fn load_preset(&mut self, preset: KernelPresetKind) {
        let target_size = self.size_value();
        let center = target_size * target_size / 2;

        let (weights, divisor, bias) = match preset {
            KernelPresetKind::Blur => {
                let (weights, divisor) = Self::blur_kernel_for_size(target_size);
                (weights, divisor, 0)
            }
            KernelPresetKind::Sharpen => match target_size {
                1 => (vec![1], 1, 0),
                3 => (vec![0, -1, 0, -1, 5, -1, 0, -1, 0], 1, 0),
                _ => {
                    let (mut weights, divisor) = Self::blur_kernel_for_size(target_size);
                    for w in &mut weights {
                        *w = -*w;
                    }
                    weights[center] += 2 * divisor;
                    (weights, divisor, 0)
                }
            },
            KernelPresetKind::Edge => {
                let mut weights = vec![-1; target_size * target_size];
                let center_weight = i32::try_from(target_size * target_size - 1).unwrap_or(i32::MAX);
                weights[center] = center_weight;
                (weights, 1, 0)
            }
            KernelPresetKind::Emboss => {
                let half = i32::try_from(target_size / 2).unwrap_or(0);
                let mut weights = Vec::with_capacity(target_size * target_size);
                for y in 0..target_size {
                    for x in 0..target_size {
                        let x = i32::try_from(x).unwrap_or(0);
                        let y = i32::try_from(y).unwrap_or(0);
                        weights.push((x - half) + (y - half));
                    }
                }
                (weights, 1, 128)
            }
            KernelPresetKind::Identity => {
                let mut weights = vec![0; target_size * target_size];
                weights[center] = 1;
                (weights, 1, 0)
            }
        };

        self.set_weights(weights.iter().map(std::string::ToString::to_string).collect());
        self.set_divisor(divisor.to_string());
        self.set_bias(bias.to_string());
    }
}

#[derive(Clone, Copy)]
enum KernelPresetKind {
    Blur,
    Sharpen,
    Edge,
    Emboss,
    Identity,
}

impl BmpViewerApp {
    /// Shows the custom kernel editor as a floating `egui::Window`.
    ///
    /// Called from the main `update()` method. Returns `Some(op)` when the
    /// user clicks "Apply" with a valid kernel, so the caller can apply it
    /// via `apply_and_refresh`.
    /// Renders the custom kernel editor and returns a convolution transform when applied.
    pub(in crate::gui) fn show_kernel_editor(&mut self, ctx: &egui::Context) -> Option<ImageTransform> {
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
                    let old_size = self.transforms.kernel.size();
                    let mut selected_size = old_size;
                    egui::ComboBox::from_id_salt("kernel_size")
                        .selected_text(format!("{}x{}", old_size.as_usize(), old_size.as_usize()))
                        .show_ui(ui, |ui| {
                            for size in KernelSize::ALL {
                                let size_value = size.as_usize();
                                ui.selectable_value(&mut selected_size, size, format!("{size_value}x{size_value}"));
                            }
                        });
                    if selected_size != old_size {
                        self.transforms.kernel.resize_preserving(selected_size);
                    }
                });

                ui.add_space(4.0);

                // --- Weight grid ---
                let n = self.transforms.kernel.size_value();
                let half = n / 2;
                egui::Grid::new("kernel_weights_grid")
                    .spacing([4.0, 4.0])
                    .show(ui, |ui| {
                        let weights = self.transforms.kernel.weights_mut();
                        for y in 0..n {
                            for x in 0..n {
                                let idx = y * n + x;
                                let is_center = x == half && y == half;
                                let widget = egui::TextEdit::singleline(&mut weights[idx])
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
                        egui::TextEdit::singleline(self.transforms.kernel.divisor_mut())
                            .desired_width(50.0)
                            .horizontal_align(egui::Align::Center),
                    );
                    ui.label("Bias:");
                    ui.add(
                        egui::TextEdit::singleline(self.transforms.kernel.bias_mut())
                            .desired_width(50.0)
                            .horizontal_align(egui::Align::Center),
                    );
                });

                // --- Preset load buttons ---
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Load preset:");
                    if ui.small_button("Blur").clicked() {
                        self.transforms.kernel.load_preset(KernelPresetKind::Blur);
                    }
                    if ui.small_button("Sharpen").clicked() {
                        self.transforms.kernel.load_preset(KernelPresetKind::Sharpen);
                    }
                    if ui.small_button("Edge").clicked() {
                        self.transforms.kernel.load_preset(KernelPresetKind::Edge);
                    }
                    if ui.small_button("Emboss").clicked() {
                        self.transforms.kernel.load_preset(KernelPresetKind::Emboss);
                    }
                    if ui.small_button("Identity").clicked() {
                        self.transforms.kernel.load_preset(KernelPresetKind::Identity);
                    }
                });

                ui.add_space(8.0);

                // --- Validation & Apply ---
                let validation = self.transforms.kernel.validate();
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
            self.transforms
                .kernel
                .validate()
                .ok()
                .map(|kernel| ConvolutionCustom { kernel }.into())
        } else {
            None
        }
    }
}
