use std::fmt;

use crate::runtime::steganography::StegConfig;

use super::convolution::{ConvolutionFilter, Kernel};
use super::geometry::{RotationInterpolation, TranslateMode};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageTransform {
    RotateLeft90,
    RotateRight90,
    RotateAny {
        angle_tenths: i16,
        interpolation: RotationInterpolation,
        expand: bool,
    },
    Resize {
        width: u32,
        height: u32,
        interpolation: RotationInterpolation,
    },
    Skew {
        x_milli: i16,
        y_milli: i16,
        interpolation: RotationInterpolation,
        expand: bool,
    },
    Translate {
        dx: i32,
        dy: i32,
        mode: TranslateMode,
        fill: [u8; 4],
    },
    Crop {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    MirrorHorizontal,
    MirrorVertical,
    InvertColors,
    Grayscale,
    Sepia,
    Brightness(i16),
    Contrast(i16),
    Convolution(ConvolutionFilter),
    CustomKernel(Kernel),
    EmbedSteganography {
        config: StegConfig,
        payload: Vec<u8>,
    },
    RemoveSteganography {
        config: StegConfig,
    },
}

impl fmt::Display for ImageTransform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RotateLeft90 => write!(f, "Rotate Left"),
            Self::RotateRight90 => write!(f, "Rotate Right"),
            Self::RotateAny {
                angle_tenths,
                interpolation,
                expand,
            } => {
                let angle = f32::from(*angle_tenths) / 10.0;
                let mode = if *expand { "Expand" } else { "Crop" };
                write!(f, "Rotate {angle:+.1} deg ({interpolation}, {mode})")
            }
            Self::Resize {
                width,
                height,
                interpolation,
            } => write!(f, "Resize to {width}x{height} ({interpolation})"),
            Self::Skew {
                x_milli,
                y_milli,
                interpolation,
                expand,
            } => {
                let kx = f32::from(*x_milli) / 1000.0;
                let ky = f32::from(*y_milli) / 1000.0;
                let mode = if *expand { "Expand" } else { "Crop" };
                write!(f, "Skew x={kx:+.3}, y={ky:+.3} ({interpolation}, {mode})")
            }
            Self::Translate { dx, dy, mode, fill } => write!(
                f,
                "Translate dx={dx:+}, dy={dy:+} ({mode}, fill #{:02X}{:02X}{:02X}{:02X})",
                fill[0], fill[1], fill[2], fill[3]
            ),
            Self::Crop { x, y, width, height } => write!(f, "Crop x={x}, y={y}, {width}x{height}"),
            Self::MirrorHorizontal => write!(f, "Mirror Horizontal"),
            Self::MirrorVertical => write!(f, "Mirror Vertical"),
            Self::InvertColors => write!(f, "Invert Colors"),
            Self::Grayscale => write!(f, "Grayscale"),
            Self::Sepia => write!(f, "Sepia"),
            Self::Brightness(delta) => {
                if *delta >= 0 {
                    write!(f, "Brightness +{delta}")
                } else {
                    write!(f, "Brightness {delta}")
                }
            }
            Self::Contrast(delta) => {
                if *delta >= 0 {
                    write!(f, "Contrast +{delta}")
                } else {
                    write!(f, "Contrast {delta}")
                }
            }
            Self::Convolution(filter) => write!(f, "{filter}"),
            Self::CustomKernel(k) => write!(f, "Custom {}x{}", k.size, k.size),
            Self::EmbedSteganography { config, payload } => write!(
                f,
                "Embed Steganography ({} bytes, R{}G{}B{}A{})",
                payload.len(),
                config.r_bits,
                config.g_bits,
                config.b_bits,
                config.a_bits
            ),
            Self::RemoveSteganography { config } => write!(
                f,
                "Remove Steganography (R{}G{}B{}A{})",
                config.r_bits, config.g_bits, config.b_bits, config.a_bits
            ),
        }
    }
}

impl ImageTransform {
    #[must_use]
    pub const fn inverse(&self) -> Option<Self> {
        #[allow(clippy::match_same_arms)]
        match self {
            Self::RotateLeft90 => Some(Self::RotateRight90),
            Self::RotateRight90 => Some(Self::RotateLeft90),
            Self::RotateAny { .. } => None,
            Self::Resize { .. } => None,
            Self::Skew { .. } => None,
            Self::Translate { .. } => None,
            Self::Crop { .. } => None,
            Self::MirrorHorizontal => Some(Self::MirrorHorizontal),
            Self::MirrorVertical => Some(Self::MirrorVertical),
            Self::InvertColors => Some(Self::InvertColors),
            Self::Grayscale => None,
            Self::Sepia => None,
            Self::Brightness(_) => None,
            Self::Contrast(_) => None,
            Self::Convolution(_) => None,
            Self::CustomKernel(_) => None,
            Self::EmbedSteganography { .. } => None,
            Self::RemoveSteganography { .. } => None,
        }
    }

    #[must_use]
    pub fn replay_cost(&self) -> u32 {
        #[allow(clippy::match_same_arms)]
        match self {
            Self::RotateLeft90
            | Self::RotateRight90
            | Self::MirrorHorizontal
            | Self::MirrorVertical
            | Self::InvertColors => 0,
            Self::Grayscale | Self::Sepia | Self::Brightness(_) | Self::Contrast(_) => 1,
            Self::RotateAny { interpolation, .. } => match interpolation {
                RotationInterpolation::Nearest => 3,
                RotationInterpolation::Bilinear => 5,
                RotationInterpolation::Bicubic => 8,
            },
            Self::Resize { interpolation, .. } => match interpolation {
                RotationInterpolation::Nearest => 2,
                RotationInterpolation::Bilinear => 4,
                RotationInterpolation::Bicubic => 7,
            },
            Self::Skew { interpolation, .. } => match interpolation {
                RotationInterpolation::Nearest => 3,
                RotationInterpolation::Bilinear => 5,
                RotationInterpolation::Bicubic => 8,
            },
            Self::Translate { .. } => 2,
            Self::Crop { .. } => 1,
            Self::Convolution(filter) => filter.kernel().replay_cost(),
            Self::CustomKernel(kernel) => kernel.replay_cost(),
            Self::EmbedSteganography { .. } => 2,
            Self::RemoveSteganography { .. } => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ImageTransform;
    use crate::runtime::transform::convolution::{ConvolutionFilter, Kernel};
    use crate::runtime::transform::geometry::{RotationInterpolation, TranslateMode};

    #[test]
    fn inverse_of_rotate_left_is_rotate_right() {
        assert_eq!(
            ImageTransform::RotateLeft90.inverse(),
            Some(ImageTransform::RotateRight90)
        );
        assert_eq!(
            ImageTransform::RotateRight90.inverse(),
            Some(ImageTransform::RotateLeft90)
        );
    }

    #[test]
    fn self_inverse_transforms() {
        assert_eq!(
            ImageTransform::MirrorHorizontal.inverse(),
            Some(ImageTransform::MirrorHorizontal)
        );
        assert_eq!(
            ImageTransform::MirrorVertical.inverse(),
            Some(ImageTransform::MirrorVertical)
        );
        assert_eq!(
            ImageTransform::InvertColors.inverse(),
            Some(ImageTransform::InvertColors)
        );
    }

    #[test]
    fn rotate_any_display_format() {
        let op = ImageTransform::RotateAny {
            angle_tenths: -125,
            interpolation: RotationInterpolation::Bicubic,
            expand: false,
        };
        assert_eq!(op.to_string(), "Rotate -12.5 deg (Bicubic, Crop)");
    }

    #[test]
    fn resize_display_format() {
        let op = ImageTransform::Resize {
            width: 640,
            height: 480,
            interpolation: RotationInterpolation::Nearest,
        };
        assert_eq!(op.to_string(), "Resize to 640x480 (Nearest)");
    }

    #[test]
    fn skew_display_format() {
        let op = ImageTransform::Skew {
            x_milli: 250,
            y_milli: -125,
            interpolation: RotationInterpolation::Bilinear,
            expand: false,
        };
        assert_eq!(op.to_string(), "Skew x=+0.250, y=-0.125 (Bilinear, Crop)");
    }

    #[test]
    fn translate_display_format() {
        let op = ImageTransform::Translate {
            dx: -12,
            dy: 7,
            mode: TranslateMode::Expand,
            fill: [0x10, 0x20, 0x30, 0x40],
        };
        assert_eq!(op.to_string(), "Translate dx=-12, dy=+7 (Expand, fill #10203040)");
    }

    #[test]
    fn replay_cost_rotate_any_depends_on_interpolation() {
        let nearest = ImageTransform::RotateAny {
            angle_tenths: 123,
            interpolation: RotationInterpolation::Nearest,
            expand: true,
        }
        .replay_cost();
        let bilinear = ImageTransform::RotateAny {
            angle_tenths: 123,
            interpolation: RotationInterpolation::Bilinear,
            expand: true,
        }
        .replay_cost();
        let bicubic = ImageTransform::RotateAny {
            angle_tenths: 123,
            interpolation: RotationInterpolation::Bicubic,
            expand: true,
        }
        .replay_cost();
        assert!(bilinear > nearest);
        assert!(bicubic > bilinear);
    }

    #[test]
    fn custom_kernel_display_format() {
        let k3 = Kernel::new(vec![0; 9], 3, 1, 0);
        assert_eq!(ImageTransform::CustomKernel(k3).to_string(), "Custom 3x3");
        let k5 = Kernel::new(vec![0; 25], 5, 1, 0);
        assert_eq!(ImageTransform::CustomKernel(k5).to_string(), "Custom 5x5");
    }

    #[test]
    fn convolution_display_formats() {
        assert_eq!(ImageTransform::Convolution(ConvolutionFilter::Blur).to_string(), "Blur");
        assert_eq!(
            ImageTransform::Convolution(ConvolutionFilter::EdgeDetect).to_string(),
            "Edge Detect"
        );
    }
}
