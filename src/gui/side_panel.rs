use eframe::egui;

use crate::BmpViewerApp;

/// Deferred actions from the side panel that need `&mut self` after the closure returns.
pub(crate) struct SidePanelActions {
    pub remove_transform: Option<usize>,
    pub do_undo: bool,
    pub do_redo: bool,
}

impl BmpViewerApp {
    /// Renders the right side panel: file info, decoded info, transforms, palette.
    ///
    /// Returns deferred actions (undo/redo/remove) that must be applied by the caller
    /// after this method returns, since the panel closure borrows `&self`.
    pub(crate) fn show_side_panel(&mut self, ctx: &egui::Context) -> SidePanelActions {
        let window_width = ctx.available_rect().width();
        let panel_max_width = (window_width - 220.0).clamp(220.0, 460.0);

        let mut remove_transform: Option<usize> = None;
        let mut do_undo = false;
        let mut do_redo = false;

        egui::SidePanel::right("bmp_info")
            .default_width(320.0)
            .width_range(220.0..=panel_max_width)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("BMP Details");
                ui.separator();
                if self.image_stats.is_empty() {
                    ui.label("Load a BMP file to inspect its metadata.");
                } else {
                    let available_height = ui.available_height();
                    let has_palette = !self.palette_colors.is_empty();
                    let has_transforms = !self.pipeline.is_empty() || !self.redo_stack.is_empty();

                    // Reserve a fixed height for the transforms section when present.
                    // Each row is ~20px; cap the section at roughly 6 visible rows.
                    // Add extra space for the undo/redo button row.
                    let transform_height = if has_transforms {
                        let list_h = self.pipeline.len() as f32 * 22.0 + 10.0;
                        let buttons_h = 26.0;
                        (list_h + buttons_h).min(170.0)
                    } else {
                        0.0
                    };
                    // Account for separators/labels (~20px each section header).
                    let overhead = if has_transforms { 24.0 } else { 0.0 };
                    let remaining = (available_height - transform_height - overhead).max(200.0);

                    let (file_height, decoded_height, palette_height) = if has_palette {
                        let file_h = (remaining * 0.45).max(100.0);
                        let decoded_h = (remaining * 0.20).max(80.0);
                        let palette_h = (remaining - file_h - decoded_h).max(100.0);
                        (file_h, decoded_h, Some(palette_h))
                    } else {
                        let file_h = (remaining * 0.62).max(120.0);
                        let decoded_h = (remaining - file_h).max(90.0);
                        (file_h, decoded_h, None)
                    };

                    ui.label("File Info");
                    egui::ScrollArea::vertical()
                        .id_salt("file_info_scroll")
                        .max_height(file_height)
                        .show(ui, |ui| {
                            ui.monospace(&self.image_stats);
                        });

                    ui.separator();
                    ui.label("Decoded Info");
                    egui::ScrollArea::vertical()
                        .id_salt("decoded_info_scroll")
                        .max_height(decoded_height)
                        .show(ui, |ui| {
                            ui.monospace(&self.decoded_stats);
                        });

                    if has_transforms {
                        ui.separator();
                        ui.label(if self.pipeline.is_empty() {
                            "Transforms".to_owned()
                        } else {
                            format!("Transforms ({})", self.pipeline.len())
                        });
                        egui::ScrollArea::vertical()
                            .id_salt("transforms_scroll")
                            .max_height(transform_height)
                            .show(ui, |ui| {
                                // Undo / Redo buttons.
                                ui.horizontal(|ui| {
                                    let can_undo = !self.pipeline.is_empty();
                                    let undo_tooltip = if let Some(op) = self.pipeline.ops().last() {
                                        format!("Undo {} (Ctrl+Z)", op)
                                    } else {
                                        "Nothing to undo".to_owned()
                                    };
                                    if ui
                                        .add_enabled(can_undo, egui::Button::new("Undo").small())
                                        .on_hover_text(&undo_tooltip)
                                        .clicked()
                                    {
                                        do_undo = true;
                                    }

                                    let can_redo = !self.redo_stack.is_empty();
                                    let redo_tooltip = if let Some(op) = self.redo_stack.last() {
                                        format!("Redo {} (Ctrl+Shift+Z)", op)
                                    } else {
                                        "Nothing to redo".to_owned()
                                    };
                                    if ui
                                        .add_enabled(can_redo, egui::Button::new("Redo").small())
                                        .on_hover_text(&redo_tooltip)
                                        .clicked()
                                    {
                                        do_redo = true;
                                    }
                                });

                                for (i, op) in self.pipeline.ops().iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.monospace(format!("{}.", i + 1));
                                        ui.label(op.to_string());
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui
                                                .small_button("\u{00d7}")
                                                .on_hover_text("Remove this transform")
                                                .clicked()
                                            {
                                                remove_transform = Some(i);
                                            }
                                        });
                                    });
                                }
                            });
                    }

                    if let Some(palette_height) = palette_height {
                        ui.separator();
                        ui.label("Color Palette");
                        egui::ScrollArea::vertical()
                            .id_salt("palette_scroll")
                            .max_height(palette_height)
                            .show(ui, |ui| {
                                self.render_palette_grid(ui);
                            });
                    }
                }
            });

        SidePanelActions {
            remove_transform,
            do_undo,
            do_redo,
        }
    }

    /// Applies deferred actions returned by [`show_side_panel`].
    pub(crate) fn apply_side_panel_actions(&mut self, ctx: &egui::Context, actions: SidePanelActions) {
        if let Some(index) = actions.remove_transform {
            self.pipeline.remove(index);
            self.redo_stack.clear();
            if let Some(original) = &self.original_image {
                let result = self.pipeline.apply(original);
                self.set_display_image(ctx, result, "transformed".to_owned());
            }
        }
        if actions.do_undo {
            self.undo_transform(ctx);
        }
        if actions.do_redo {
            self.redo_transform(ctx);
        }
    }
}
