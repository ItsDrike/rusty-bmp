use std::{fs::File, path::PathBuf};

use bmp::{
    raw::Bmp,
    runtime::{
        decode::{DecodedImage, decode_to_rgba},
        encode::save_bmp,
        transform::{ImageTransform, TransformPipeline, apply_transform},
    },
};
use eframe::egui;
use rfd::FileDialog;

#[derive(Default)]
struct BmpViewerApp {
    path_input: String,
    status: String,
    metadata: String,
    texture: Option<egui::TextureHandle>,
    transformed_image: Option<DecodedImage>,
    pipeline: TransformPipeline,
}

impl BmpViewerApp {
    fn set_display_image(&mut self, ctx: &egui::Context, image: DecodedImage, label: String) {
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [image.width as usize, image.height as usize],
            &image.rgba,
        );
        self.texture = Some(ctx.load_texture(label, color, egui::TextureOptions::NEAREST));
        self.metadata = format!("{} x {}", image.width, image.height);
        self.transformed_image = Some(image);
    }

    fn load_path(&mut self, ctx: &egui::Context, path: PathBuf) {
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

        self.pipeline.clear();
        self.set_display_image(ctx, decoded, path.to_string_lossy().to_string());
        self.status = format!("Loaded {}", path.display());
    }

    fn pick_and_load(&mut self, ctx: &egui::Context) {
        if let Some(path) = FileDialog::new()
            .add_filter("Bitmap image", &["bmp", "dib"])
            .set_title("Open BMP file")
            .pick_file()
        {
            self.path_input = path.display().to_string();
            self.load_path(ctx, path);
        }
    }

    fn apply_and_refresh(&mut self, ctx: &egui::Context, op: ImageTransform) {
        if let Some(current) = self.transformed_image.as_ref() {
            let next = apply_transform(current, op);
            self.pipeline.push(op);
            self.set_display_image(ctx, next, "transformed".to_owned());
        }
    }

    fn save_current(&mut self) {
        let Some(image) = self.transformed_image.as_ref() else {
            self.status = "Nothing to save".to_owned();
            return;
        };

        let Some(path) = FileDialog::new()
            .add_filter("Bitmap image", &["bmp"])
            .set_title("Save transformed BMP")
            .set_file_name("transformed.bmp")
            .save_file()
        else {
            return;
        };

        match save_bmp(&path, image) {
            Ok(()) => {
                self.status = format!("Saved {}", path.display());
            }
            Err(err) => {
                self.status = format!("Save failed: {err}");
            }
        }
    }
}

impl eframe::App for BmpViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("BMP Path:");
                let path_edit = ui.add_sized(
                    [ui.available_width() - 140.0, 24.0],
                    egui::TextEdit::singleline(&mut self.path_input)
                        .hint_text("C:\\images\\picture.bmp or /home/user/picture.bmp"),
                );
                let enter = path_edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let browse_clicked = ui.button("Browse...").clicked();
                let load_clicked = ui.button("Load").clicked();
                if browse_clicked {
                    self.pick_and_load(ctx);
                } else if enter || load_clicked {
                    let path = PathBuf::from(self.path_input.trim());
                    if path.as_os_str().is_empty() {
                        self.status = "Please enter a path".to_owned();
                    } else {
                        self.load_path(ctx, path);
                    }
                }
            });

            ui.horizontal(|ui| {
                let rotate_left = ui.button("Rotate Left").clicked();
                let rotate_right = ui.button("Rotate Right").clicked();
                let mirror = ui.button("Mirror").clicked();
                let save_clicked = ui.button("Save As...").clicked();
                if rotate_left {
                    self.apply_and_refresh(ctx, ImageTransform::RotateLeft90);
                }
                if rotate_right {
                    self.apply_and_refresh(ctx, ImageTransform::RotateRight90);
                }
                if mirror {
                    self.apply_and_refresh(ctx, ImageTransform::MirrorHorizontal);
                }
                if save_clicked {
                    self.save_current();
                }
            });
            if !self.status.is_empty() {
                ui.label(&self.status);
            }
            if !self.metadata.is_empty() {
                ui.label(&self.metadata);
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(texture) = &self.texture {
                let avail = ui.available_size();
                let mut size = texture.size_vec2();
                if size.x > avail.x || size.y > avail.y {
                    let scale = (avail.x / size.x).min(avail.y / size.y);
                    if scale.is_finite() && scale > 0.0 {
                        size *= scale;
                    }
                }
                ui.image((texture.id(), size));
            } else {
                ui.label("Load a BMP file to preview it.");
            }
        });
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
