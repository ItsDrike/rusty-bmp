use std::fmt;

use thiserror::Error;

use crate::runtime::decode::{DecodedImage, DecodedImageError};

use super::{
    color::{Brightness, Contrast, Grayscale, InvertColors, Sepia},
    convolution::{ConvolutionCustom, ConvolutionPreset},
    geometry::{Crop, MirrorHorizontal, MirrorVertical, Resize, RotateAny, RotateLeft, RotateRight, Skew, Translate},
    steganography::{self, EmbedSteganography, RemoveSteganography},
};

#[derive(Debug, Error)]
pub enum TransformError {
    #[error("steganography error: {0}")]
    Steganography(#[from] steganography::StegError),

    #[error("invalid decoded image: {0}")]
    InvalidImage(#[from] DecodedImageError),
}

pub trait TransformOp: fmt::Display + fmt::Debug + Send + Sync {
    /// Applies this transformation to `image`.
    ///
    /// # Errors
    /// Returns [`TransformError`] if the operation cannot be applied.
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError>;

    #[must_use]
    fn inverse(&self) -> Option<ImageTransform>;

    #[must_use]
    fn replay_cost(&self) -> u32;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageTransform {
    RotateLeft(RotateLeft),
    RotateRight(RotateRight),
    RotateAny(RotateAny),
    Resize(Resize),
    Skew(Skew),
    Translate(Translate),
    Crop(Crop),
    MirrorHorizontal(MirrorHorizontal),
    MirrorVertical(MirrorVertical),
    InvertColors(InvertColors),
    Grayscale(Grayscale),
    Sepia(Sepia),
    Brightness(Brightness),
    Contrast(Contrast),
    ConvolutionPreset(ConvolutionPreset),
    ConvolutionCustom(ConvolutionCustom),
    EmbedSteganography(EmbedSteganography),
    RemoveSteganography(RemoveSteganography),
}

impl ImageTransform {
    #[must_use]
    pub fn inverse(&self) -> Option<Self> {
        match self {
            Self::RotateLeft(op) => op.inverse(),
            Self::RotateRight(op) => op.inverse(),
            Self::RotateAny(op) => op.inverse(),
            Self::Resize(op) => op.inverse(),
            Self::Skew(op) => op.inverse(),
            Self::Translate(op) => op.inverse(),
            Self::Crop(op) => op.inverse(),
            Self::MirrorHorizontal(op) => op.inverse(),
            Self::MirrorVertical(op) => op.inverse(),
            Self::InvertColors(op) => op.inverse(),
            Self::Grayscale(op) => op.inverse(),
            Self::Sepia(op) => op.inverse(),
            Self::Brightness(op) => op.inverse(),
            Self::Contrast(op) => op.inverse(),
            Self::ConvolutionPreset(op) => op.inverse(),
            Self::ConvolutionCustom(op) => op.inverse(),
            Self::EmbedSteganography(op) => op.inverse(),
            Self::RemoveSteganography(op) => op.inverse(),
        }
    }

    #[must_use]
    pub fn replay_cost(&self) -> u32 {
        match self {
            Self::RotateLeft(op) => op.replay_cost(),
            Self::RotateRight(op) => op.replay_cost(),
            Self::RotateAny(op) => op.replay_cost(),
            Self::Resize(op) => op.replay_cost(),
            Self::Skew(op) => op.replay_cost(),
            Self::Translate(op) => op.replay_cost(),
            Self::Crop(op) => op.replay_cost(),
            Self::MirrorHorizontal(op) => op.replay_cost(),
            Self::MirrorVertical(op) => op.replay_cost(),
            Self::InvertColors(op) => op.replay_cost(),
            Self::Grayscale(op) => op.replay_cost(),
            Self::Sepia(op) => op.replay_cost(),
            Self::Brightness(op) => op.replay_cost(),
            Self::Contrast(op) => op.replay_cost(),
            Self::ConvolutionPreset(op) => op.replay_cost(),
            Self::ConvolutionCustom(op) => op.replay_cost(),
            Self::EmbedSteganography(op) => op.replay_cost(),
            Self::RemoveSteganography(op) => op.replay_cost(),
        }
    }

    /// Applies this transformation to `image`.
    ///
    /// # Errors
    /// Returns [`TransformError`] if the operation cannot be applied.
    pub fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        match self {
            Self::RotateLeft(op) => op.apply(image),
            Self::RotateRight(op) => op.apply(image),
            Self::RotateAny(op) => op.apply(image),
            Self::Resize(op) => op.apply(image),
            Self::Skew(op) => op.apply(image),
            Self::Translate(op) => op.apply(image),
            Self::Crop(op) => op.apply(image),
            Self::MirrorHorizontal(op) => op.apply(image),
            Self::MirrorVertical(op) => op.apply(image),
            Self::InvertColors(op) => op.apply(image),
            Self::Grayscale(op) => op.apply(image),
            Self::Sepia(op) => op.apply(image),
            Self::Brightness(op) => op.apply(image),
            Self::Contrast(op) => op.apply(image),
            Self::ConvolutionPreset(op) => op.apply(image),
            Self::ConvolutionCustom(op) => op.apply(image),
            Self::EmbedSteganography(op) => op.apply(image),
            Self::RemoveSteganography(op) => op.apply(image),
        }
    }
}

impl fmt::Display for ImageTransform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RotateLeft(op) => write!(f, "{op}"),
            Self::RotateRight(op) => write!(f, "{op}"),
            Self::RotateAny(op) => write!(f, "{op}"),
            Self::Resize(op) => write!(f, "{op}"),
            Self::Skew(op) => write!(f, "{op}"),
            Self::Translate(op) => write!(f, "{op}"),
            Self::Crop(op) => write!(f, "{op}"),
            Self::MirrorHorizontal(op) => write!(f, "{op}"),
            Self::MirrorVertical(op) => write!(f, "{op}"),
            Self::InvertColors(op) => write!(f, "{op}"),
            Self::Grayscale(op) => write!(f, "{op}"),
            Self::Sepia(op) => write!(f, "{op}"),
            Self::Brightness(op) => write!(f, "{op}"),
            Self::Contrast(op) => write!(f, "{op}"),
            Self::ConvolutionPreset(op) => write!(f, "{op}"),
            Self::ConvolutionCustom(op) => write!(f, "{op}"),
            Self::EmbedSteganography(op) => write!(f, "{op}"),
            Self::RemoveSteganography(op) => write!(f, "{op}"),
        }
    }
}

impl From<RotateLeft> for ImageTransform {
    fn from(value: RotateLeft) -> Self {
        Self::RotateLeft(value)
    }
}
impl From<RotateRight> for ImageTransform {
    fn from(value: RotateRight) -> Self {
        Self::RotateRight(value)
    }
}
impl From<RotateAny> for ImageTransform {
    fn from(value: RotateAny) -> Self {
        Self::RotateAny(value)
    }
}
impl From<Resize> for ImageTransform {
    fn from(value: Resize) -> Self {
        Self::Resize(value)
    }
}
impl From<Skew> for ImageTransform {
    fn from(value: Skew) -> Self {
        Self::Skew(value)
    }
}
impl From<Translate> for ImageTransform {
    fn from(value: Translate) -> Self {
        Self::Translate(value)
    }
}
impl From<Crop> for ImageTransform {
    fn from(value: Crop) -> Self {
        Self::Crop(value)
    }
}
impl From<MirrorHorizontal> for ImageTransform {
    fn from(value: MirrorHorizontal) -> Self {
        Self::MirrorHorizontal(value)
    }
}
impl From<MirrorVertical> for ImageTransform {
    fn from(value: MirrorVertical) -> Self {
        Self::MirrorVertical(value)
    }
}
impl From<InvertColors> for ImageTransform {
    fn from(value: InvertColors) -> Self {
        Self::InvertColors(value)
    }
}
impl From<Grayscale> for ImageTransform {
    fn from(value: Grayscale) -> Self {
        Self::Grayscale(value)
    }
}
impl From<Sepia> for ImageTransform {
    fn from(value: Sepia) -> Self {
        Self::Sepia(value)
    }
}
impl From<Brightness> for ImageTransform {
    fn from(value: Brightness) -> Self {
        Self::Brightness(value)
    }
}
impl From<Contrast> for ImageTransform {
    fn from(value: Contrast) -> Self {
        Self::Contrast(value)
    }
}
impl From<ConvolutionPreset> for ImageTransform {
    fn from(value: ConvolutionPreset) -> Self {
        Self::ConvolutionPreset(value)
    }
}
impl From<ConvolutionCustom> for ImageTransform {
    fn from(value: ConvolutionCustom) -> Self {
        Self::ConvolutionCustom(value)
    }
}
impl From<EmbedSteganography> for ImageTransform {
    fn from(value: EmbedSteganography) -> Self {
        Self::EmbedSteganography(value)
    }
}
impl From<RemoveSteganography> for ImageTransform {
    fn from(value: RemoveSteganography) -> Self {
        Self::RemoveSteganography(value)
    }
}
