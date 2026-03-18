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
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
mod geometry;
mod model;
mod pipeline;
pub mod steganography;

pub use color::{Brightness, Contrast, Grayscale, InvertColors, Sepia};
pub use convolution::{ConvolutionCustom, ConvolutionFilter, ConvolutionPreset, Kernel, KernelError};
pub use geometry::{
    Crop, GeometryValidationError, MirrorHorizontal, MirrorVertical, Resize, RotateAny, RotateLeft, RotateRight,
    RotationInterpolation, Skew, Translate, TranslateMode,
};
pub use model::{ImageTransform, TransformError, TransformOp};
pub use pipeline::TransformPipeline;
pub use steganography::{EmbedSteganography, RemoveSteganography};
