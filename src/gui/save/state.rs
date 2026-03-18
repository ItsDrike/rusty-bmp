//! Core save-related state shared by the save workflow.

use std::{path::PathBuf, sync::mpsc::Receiver};

use bmp::runtime::encode::{SaveFormat, SaveHeaderVersion};

/// Pending background save worker.
pub(super) struct PendingSaveTask {
    pub(super) rx: Receiver<SaveTaskResult>,
}

/// Result reported by a completed background save worker.
pub(super) struct SaveTaskResult {
    pub(super) path: PathBuf,
    pub(super) format: SaveFormat,
    pub(super) header: SaveHeaderVersion,
    pub(super) result: Result<(), String>,
}

/// Outcome of polling the asynchronous save worker.
pub(in crate::gui) enum SavePoll {
    None,
    Saved {
        path: PathBuf,
        format: SaveFormat,
        header: SaveHeaderVersion,
    },
    Failed(String),
}

/// User-selected save options plus transient save dialog/task state.
#[derive(Default)]
pub(in crate::gui) struct SaveState {
    pub(in crate::gui) save_format: SaveFormat,
    pub(in crate::gui) save_header_version: SaveHeaderVersion,
    pub(super) pending_save: Option<PendingSaveTask>,
    pub(super) save_confirm_pending: Option<PathBuf>,
    pub(super) save_confirm_reason: Option<String>,
}

impl SaveState {
    /// Updates the selected header version and coerces the save format if needed.
    pub(in crate::gui) fn set_header_version(&mut self, version: SaveHeaderVersion) {
        self.save_header_version = version;
        if !self.save_header_version.is_compatible(self.save_format) {
            self.save_format = self.save_header_version.compatible_formats()[0];
        }
    }

    pub(in crate::gui) const fn set_save_format(&mut self, format: SaveFormat) {
        self.save_format = format;
    }

    /// Reinitializes save options to match a newly loaded BMP.
    pub(in crate::gui) fn reset_for_loaded_bmp(&mut self, format: SaveFormat, header: SaveHeaderVersion) {
        self.set_save_format(format);
        self.set_header_version(header);
        self.clear_confirmation();
    }

    /// Clears any pending save-confirm dialog state.
    pub(super) fn clear_confirmation(&mut self) {
        self.save_confirm_pending = None;
        self.save_confirm_reason = None;
    }
}
