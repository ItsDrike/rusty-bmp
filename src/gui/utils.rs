/// Computes a dimension scaled to preserve an aspect ratio.
///
/// This helper is used by the crop UI when "Keep aspect ratio" is enabled.
/// It scales `base` by the ratio `other_dim / base_dim` and returns the
/// resulting dimension rounded to the nearest integer pixel.
///
/// Floating-point arithmetic is used because the crop UI performs interactive
/// resizing where fractional intermediate values are expected. The final value
/// is rounded to the nearest integer and clamped to at least one pixel.
///
/// The calculation is intentionally isolated in this helper so that the
/// necessary lint expectations for float conversions do not need to be
/// repeated at each call site.
///
/// # Parameters
///
/// * `base` - The dimension being scaled (e.g. crop width)
/// * `other_dim` - The dimension whose size determines the ratio (e.g. image height)
/// * `base_dim` - The dimension used as the ratio denominator (e.g. image width)
///
/// `base` is a dimension that has changed (for example a width).
/// `other_dim / base_dim` describes the aspect ratio that should be preserved.
///
/// # Returns
///
/// The scaled dimension rounded to the nearest integer pixel, with a minimum
/// value of `1`.
///
/// # Example
///
/// An image with size 1920 x 1080 has an aspect ratio of 1080 / 1920.
/// If the width becomes 400 and we want to keep the same ratio, the
/// corresponding height is 255:
///
/// ```text
/// assert_eq!(scaled_dim(400, 1080, 1920), 225); // maintain 16:9 aspect ratio
/// ```
#[inline]
pub(super) fn scaled_dim(base: u32, other_dim: u32, base_dim: u32) -> u32 {
    debug_assert!(base_dim > 0);

    #[expect(
        clippy::cast_precision_loss,
        reason = "u32 -> f32 precision loss is acceptable for UI aspect-ratio math; result is rounded to pixel units"
    )]
    let ratio = other_dim as f32 / base_dim as f32;

    #[expect(
        clippy::cast_precision_loss,
        reason = "u32 -> f32 precision loss is acceptable for UI sizing; final value is rounded and clamped"
    )]
    let scaled = (base as f32 * ratio).round().max(1.0);

    #[expect(clippy::cast_precision_loss)]
    {
        debug_assert!(scaled <= u32::MAX as f32);
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "value was rounded to an integer pixel value before casting"
    )]
    #[expect(
        clippy::cast_sign_loss,
        reason = "scaled value is guaranteed positive due to `.max(1.0)`"
    )]
    {
        scaled as u32
    }
}

/// Scales a dimension using an explicit floating-point multiplier.
///
/// This helper is used for simple UI resizing shortcuts where the user
/// specifies a direct scale factor (for example `0.5` for 50% or `2.0`
/// for 200%). The result is computed as:
///
/// ```text
/// round(value x factor)
/// ```
///
/// The result is always clamped to at least one pixel so that the resulting
/// dimension cannot become zero.
///
/// Floating-point arithmetic is acceptable here because this function is used
/// exclusively for GUI sizing operations where sub-pixel intermediate values
/// are expected and the final value is rounded to integer pixels.
///
/// # Parameters
///
/// * `value` - The original dimension to scale.
/// * `factor` - The multiplier applied to the dimension.
///
/// # Returns
///
/// The scaled dimension rounded to the nearest integer pixel, with a minimum
/// value of `1`.
#[inline]
pub(super) fn scaled_dim_by_factor(value: u32, factor: f32) -> u32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "u32 -> f32 precision loss is acceptable for UI scaling operations"
    )]
    let scaled = (value as f32 * factor).round().max(1.0);

    #[expect(clippy::cast_precision_loss)]
    {
        debug_assert!(scaled <= u32::MAX as f32);
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "value was rounded to an integer pixel value before casting"
    )]
    #[expect(
        clippy::cast_sign_loss,
        reason = "scaled value is guaranteed positive due to `.max(1.0)`"
    )]
    {
        scaled as u32
    }
}
