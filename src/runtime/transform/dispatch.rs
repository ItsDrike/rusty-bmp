use crate::runtime::decode::DecodedImage;
use crate::runtime::steganography;

use super::color::{brightness, contrast, grayscale, invert_colors, sepia};
use super::convolution::apply_convolution;
use super::geometry::{
    crop_image, mirror_horizontal, mirror_vertical, resize_image, rotate_any, rotate_left, rotate_right, skew_image,
    translate_image,
};
use super::model::ImageTransform;

/// Applies a single transformation operation to an image.
///
/// This function acts as the central dispatcher for the transformation
/// system. It matches the provided [`ImageTransform`] variant and forwards
/// execution to the corresponding implementation.
///
/// The transformation is applied **immutably**: the input image is never
/// modified and a new [`DecodedImage`] is always returned.
///
/// # Error handling
///
/// Most transformations are deterministic and cannot fail.
/// For transformations that can fail (e.g. steganography embedding because the
/// payload does not fit the image), the original image is returned unchanged and
/// this function silently passes anyways.
#[must_use]
pub fn apply_transform(image: &DecodedImage, op: &ImageTransform) -> DecodedImage {
    match op {
        ImageTransform::RotateLeft90 => rotate_left(image),
        ImageTransform::RotateRight90 => rotate_right(image),
        ImageTransform::RotateAny {
            angle_tenths,
            interpolation,
            expand,
        } => rotate_any(image, f32::from(*angle_tenths) / 10.0, *interpolation, *expand),
        ImageTransform::Resize {
            width,
            height,
            interpolation,
        } => resize_image(image, *width, *height, *interpolation),
        ImageTransform::Skew {
            x_milli,
            y_milli,
            interpolation,
            expand,
        } => skew_image(
            image,
            f32::from(*x_milli) / 1000.0,
            f32::from(*y_milli) / 1000.0,
            *interpolation,
            *expand,
        ),
        ImageTransform::Translate { dx, dy, mode, fill } => translate_image(image, *dx, *dy, *mode, *fill),
        ImageTransform::Crop { x, y, width, height } => crop_image(image, *x, *y, *width, *height),
        ImageTransform::MirrorHorizontal => mirror_horizontal(image),
        ImageTransform::MirrorVertical => mirror_vertical(image),
        ImageTransform::InvertColors => invert_colors(image),
        ImageTransform::Grayscale => grayscale(image),
        ImageTransform::Sepia => sepia(image),
        ImageTransform::Brightness(delta) => brightness(image, *delta),
        ImageTransform::Contrast(delta) => contrast(image, *delta),
        ImageTransform::Convolution(filter) => apply_convolution(image, &filter.kernel()),
        ImageTransform::CustomKernel(kernel) => apply_convolution(image, kernel),
        ImageTransform::EmbedSteganography { config, payload } => {
            steganography::embed(image, *config, payload).unwrap_or_else(|_| image.clone())
        }
        ImageTransform::RemoveSteganography { config } => steganography::remove(image, *config),
    }
}

#[cfg(test)]
mod tests {
    use super::apply_transform;
    use crate::runtime::decode::DecodedImage;
    use crate::runtime::transform::model::ImageTransform;

    #[test]
    fn apply_then_inverse_is_identity_for_invertible_ops() {
        let image = DecodedImage {
            width: 3,
            height: 2,
            rgba: vec![
                10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255, 130, 140, 150, 255, 160, 170,
                180, 255,
            ],
        };

        for op in [
            ImageTransform::RotateLeft90,
            ImageTransform::RotateRight90,
            ImageTransform::MirrorHorizontal,
            ImageTransform::MirrorVertical,
            ImageTransform::InvertColors,
        ] {
            let inv = op.inverse().expect("reversible transform should have inverse");
            let transformed = apply_transform(&image, &op);
            let restored = apply_transform(&transformed, &inv);
            assert_eq!(restored.rgba, image.rgba);
        }
    }
}
