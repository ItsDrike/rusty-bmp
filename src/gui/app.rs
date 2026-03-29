//! Top-level GUI application state and the `eframe::App` update loop.

use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
};

use bmp::runtime::transform::ImageTransform;
use eframe::egui;
use rfd::FileDialog;

use super::{
    document::{DocumentInspection, DocumentState},
    panels::ViewportState,
    save::{SavePoll, SaveState},
    session::{EditSession, LoadSession},
    steganography::SteganographyUiState,
    tools::TransformToolState,
};

struct PendingOpenTask {
    rx: Receiver<Option<PathBuf>>,
}

enum OpenPoll {
    None,
    Selected(PathBuf),
    Cancelled,
    Failed(String),
}

#[derive(Default)]
/// Root UI state for the interactive BMP viewer application.
pub(super) struct BmpViewerApp {
    pub(in crate::gui) path_input: String,
    /// UI feedback/status message shown in toolbar.
    pub(in crate::gui) status: String,
    pending_open: Option<PendingOpenTask>,
    pub(in crate::gui) document: DocumentState,
    pub(in crate::gui) inspection: DocumentInspection,
    pub(in crate::gui) viewport: ViewportState,
    pub(in crate::gui) transforms: TransformToolState,
    pub(in crate::gui) steganography: SteganographyUiState,
    pub(in crate::gui) save: SaveState,
}

impl BmpViewerApp {
    pub(in crate::gui) const fn load_session(&mut self) -> LoadSession<'_> {
        LoadSession::new(
            &mut self.document,
            &mut self.inspection,
            &mut self.viewport,
            &mut self.steganography,
            &mut self.save,
        )
    }

    pub(in crate::gui) const fn edit_session(&mut self) -> EditSession<'_> {
        EditSession::new(&mut self.document, &mut self.viewport, &mut self.steganography)
    }

    /// Loads a BMP from the given path and updates the toolbar status.
    pub(in crate::gui) fn load_path(&mut self, ctx: &egui::Context, path: &Path) {
        let result = {
            let mut session = self.load_session();
            session.load_path(ctx, path)
        };
        self.status = match result {
            Ok(status) => status,
            Err(err) => err,
        };
    }

    /// Opens a file picker and loads the selected BMP.
    pub(in crate::gui) fn pick_and_load(&mut self, ctx: &egui::Context) {
        if self.pending_open.is_some() {
            "A file picker is already open".clone_into(&mut self.status);
            return;
        }

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let path = FileDialog::new()
                .add_filter("Bitmap image", &["bmp", "dib"])
                .set_title("Open BMP file")
                .pick_file();
            let _ = tx.send(path);
        });

        self.pending_open = Some(PendingOpenTask { rx });
        ctx.request_repaint();
    }

    fn poll_pending_open(&mut self, ctx: &egui::Context) -> OpenPoll {
        let Some(task) = self.pending_open.as_mut() else {
            return OpenPoll::None;
        };

        let picked = match task.rx.try_recv() {
            Ok(path) => Some(path),
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(33));
                None
            }
            Err(TryRecvError::Disconnected) => {
                self.pending_open = None;
                return OpenPoll::Failed("Failed to open file picker: worker disconnected".to_owned());
            }
        };

        let Some(path) = picked else {
            return OpenPoll::None;
        };

        self.pending_open = None;

        match path {
            Some(path) => OpenPoll::Selected(path),
            None => OpenPoll::Cancelled,
        }
    }

    /// Applies a transform immediately and surfaces any failure through the status line.
    pub(in crate::gui) fn apply_transform_now(&mut self, ctx: &egui::Context, op: ImageTransform) {
        let result = {
            let mut session = self.edit_session();
            session.apply_transform_now(ctx, op)
        };
        if let Err(err) = result {
            self.status = err;
        }
    }

    /// Applies a transform, first requesting confirmation if steg data may be corrupted.
    pub(in crate::gui) fn apply_and_refresh(&mut self, ctx: &egui::Context, op: ImageTransform) {
        let result = {
            let mut session = self.edit_session();
            session.apply_or_queue_transform(ctx, op)
        };
        if let Err(err) = result {
            self.status = err;
        }
    }

    /// Undoes the most recent transform and updates the status line if needed.
    pub(in crate::gui) fn undo_transform(&mut self, ctx: &egui::Context) {
        let result = {
            let mut session = self.edit_session();
            session.undo(ctx)
        };
        match result {
            Ok(Some(status)) => self.status = status,
            Ok(None) => {}
            Err(err) => self.status = err,
        }
    }

    /// Redoes the next transform and updates the status line if needed.
    pub(in crate::gui) fn redo_transform(&mut self, ctx: &egui::Context) {
        let result = {
            let mut session = self.edit_session();
            session.redo(ctx)
        };
        if let Err(err) = result {
            self.status = err;
        }
    }
}

impl eframe::App for BmpViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        match self.poll_pending_open(ctx) {
            OpenPoll::None | OpenPoll::Cancelled => {}
            OpenPoll::Selected(path) => {
                self.path_input = path.display().to_string();
                self.load_path(ctx, &path);
            }
            OpenPoll::Failed(err) => {
                self.status = err;
            }
        }

        match self.save.poll_pending(ctx) {
            SavePoll::None => {}
            SavePoll::Saved { path, format, header } => {
                self.path_input = path.display().to_string();
                self.status = format!("Saved {} ({}, {})", path.display(), format, header);
                self.load_path(ctx, &path);
            }
            SavePoll::Failed(err) => {
                self.status = err;
            }
        }

        let text_has_focus = ctx.memory(|m| m.focused().is_some());
        let kb = ctx.input(|i| {
            let cmd = i.modifiers.command;
            let shift = i.modifiers.shift;
            (
                cmd && i.key_pressed(egui::Key::O),
                cmd && !shift && i.key_pressed(egui::Key::S),
                cmd && shift && i.key_pressed(egui::Key::S),
                !text_has_focus && cmd && !shift && i.key_pressed(egui::Key::Z),
                !text_has_focus && cmd && (shift && i.key_pressed(egui::Key::Z) || i.key_pressed(egui::Key::Y)),
            )
        });
        let (kb_open, kb_save, kb_save_as, kb_undo, kb_redo) = kb;

        if kb_open {
            self.pick_and_load(ctx);
        }
        if kb_save
            && let Err(err) = self
                .save
                .save_overwrite(ctx, &self.document, self.steganography.detected.as_ref())
        {
            self.status = err;
        }
        if kb_save_as
            && let Err(err) = self
                .save
                .save_current(ctx, &self.document, self.steganography.detected.as_ref())
        {
            self.status = err;
        }
        if kb_undo {
            self.undo_transform(ctx);
        }
        if kb_redo {
            self.redo_transform(ctx);
        }

        let dropped_files = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(file) = dropped_files.first()
            && let Some(path) = &file.path
        {
            self.path_input = path.display().to_string();
            self.load_path(ctx, path);
        }

        self.show_toolbar(ctx);

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
        if let Some(op) = self.steganography.show_transform_confirm_window(ctx) {
            self.apply_transform_now(ctx, op);
        }
        if let Err(err) = self.save.show_confirm_window(ctx, &self.document) {
            self.status = err;
        }

        let side_actions = self.show_side_panel(ctx);
        self.apply_side_panel_actions(ctx, side_actions);

        self.show_zoom_bar(ctx);
        self.show_viewer(ctx, text_has_focus);
    }
}
