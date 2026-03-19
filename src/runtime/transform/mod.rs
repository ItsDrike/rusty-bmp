//! High-level image transform APIs.
//!
//! Most users only need:
//! - [`TransformPipeline`] for stateless transform lists and replay
//! - [`TransformPipelineExecutor`] for optional checkpointed replay caching
//! - [`ImageTransform`] for storing heterogeneous transform operations

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
mod executor;
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

/// Color/intensity transforms.
pub use color::{Brightness, Contrast, Grayscale, InvertColors, Sepia};
/// Convolution filters and custom kernels.
pub use convolution::{ConvolutionCustom, ConvolutionFilter, ConvolutionPreset, Kernel, KernelError};
/// Optional replay executor with checkpoint caching.
pub use executor::{TransformPipelineExecutor, TransformPipelineExecutorConfig};
/// Geometry/shape transforms.
pub use geometry::{
    Crop, GeometryValidationError, MirrorHorizontal, MirrorVertical, Resize, RotateAny, RotateLeft, RotateRight,
    RotationInterpolation, Skew, Translate, TranslateMode,
};
/// Core transform trait/object model.
pub use model::{ImageTransform, TransformError, TransformOp};
/// Stateless pipeline model and replay result types.
pub use pipeline::{PipelineError, ReplayError, ReplayReport, ReplaySkip, TransformPipeline};
/// Steganography transforms.
pub use steganography::{EmbedSteganography, RemoveSteganography};
