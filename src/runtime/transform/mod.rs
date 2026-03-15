#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
mod color;
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
mod convolution;
mod dispatch;
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
mod geometry;
mod model;
mod pipeline;

pub use color::{brightness, contrast, grayscale, invert_colors, sepia};
pub use convolution::{ConvolutionFilter, Kernel, apply_convolution};
pub use dispatch::apply_transform;
pub use geometry::{
    RotationInterpolation, TranslateMode, crop_image, mirror_horizontal, mirror_vertical, resize_image, rotate_any,
    rotate_left, rotate_right, skew_image, translate_image,
};
pub use model::ImageTransform;
pub use pipeline::TransformPipeline;
