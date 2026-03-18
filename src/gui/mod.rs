//! GUI application shell and feature modules for the BMP viewer.

mod app;
mod document;
mod panels;
mod save;
mod session;
mod steganography;
mod tools;

use app::BmpViewerApp;

pub fn run() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "BMP Viewer",
        options,
        Box::new(|_cc| Ok(Box::<app::BmpViewerApp>::default())),
    )
}
