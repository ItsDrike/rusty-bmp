//! Transform tool state grouped by editing feature.

mod color;
mod geometry;

use bmp::runtime::transform::{RotationInterpolation, TranslateMode};

pub(in crate::gui) use color::{KernelToolState, TonalAdjustState};
pub(in crate::gui) use geometry::{
    CropToolState, ResizeToolState, RotateToolState, SkewToolState, TranslateToolState,
};

/// Window/dialog state for transform tools and their per-tool inputs.
pub(in crate::gui) struct TransformToolState {
    pub(in crate::gui) kernel: KernelToolState,
    pub(in crate::gui) rotate: RotateToolState,
    pub(in crate::gui) resize: ResizeToolState,
    pub(in crate::gui) skew: SkewToolState,
    pub(in crate::gui) translate: TranslateToolState,
    pub(in crate::gui) crop: CropToolState,
    pub(in crate::gui) tonal: TonalAdjustState,
}

impl Default for TransformToolState {
    fn default() -> Self {
        Self {
            kernel: KernelToolState::new(),
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
            crop: CropToolState::new(),
            tonal: TonalAdjustState {
                brightness_input: 0,
                contrast_input: 0,
            },
        }
    }
}
