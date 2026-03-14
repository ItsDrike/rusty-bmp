use std::io::{self, Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::raw::{error::ColorMaskError, types::BitsPerPixel};

/// Returns `true` if the given bitmask consists of a single contiguous run of
/// set bits.
///
/// A contiguous mask has the form `0b00..0011..1100..00` with no gaps.
/// A mask of `0` is considered non-contiguous.
#[inline]
fn check_bitmask_contiguous(mask: u32) -> bool {
    if mask == 0 {
        return false;
    }

    let shifted = mask >> mask.trailing_zeros();
    (shifted & (shifted + 1)) == 0
}

/// Returns the first pair of masks that share at least one common bit.
///
/// Each mask is associated with a [`ColorMaskChannel`]. If two masks overlap,
/// the overlapping pair (with their channels) is returned. Otherwise, `None`
/// is returned.
#[inline]
fn find_overlapping_masks(
    masks: &[(u32, ColorMaskChannel)],
) -> Option<(u32, ColorMaskChannel, u32, ColorMaskChannel)> {
    for i in 0..masks.len() {
        for j in (i + 1)..masks.len() {
            let (mask_a, channel_a) = masks[i];
            let (mask_b, channel_b) = masks[j];

            if mask_a & mask_b != 0 {
                return Some((mask_a, channel_a, mask_b, channel_b));
            }
        }
    }

    None
}

/// Validates that the provided color masks are well-formed.
///
/// Ensures that:
/// - Each non-zero mask is contiguous.
/// - No two masks overlap.
///
/// Returns an appropriate [`ColorMaskError`] if validation fails.
fn validate_masks(masks: &[(u32, ColorMaskChannel)]) -> Result<(), ColorMaskError> {
    for &(mask, channel) in masks {
        if mask != 0 && !check_bitmask_contiguous(mask) {
            return Err(ColorMaskError::NonContiguous { mask, channel });
        }
    }

    if let Some((mask_a, channel_a, mask_b, channel_b)) = find_overlapping_masks(masks) {
        return Err(ColorMaskError::Overlapping {
            mask_a,
            channel_a,
            mask_b,
            channel_b,
        });
    }

    Ok(())
}

/// Validates color masks against a given pixel bit depth (`bpp`).
///
/// Performs structural validation (contiguous, non-overlapping masks) and
/// ensures that all mask bits fit within the specified number of bits per
/// pixel.
fn validate_masks_for_bpp(masks: &[(u32, ColorMaskChannel)], bpp: BitsPerPixel) -> Result<(), ColorMaskError> {
    // First perform structural validation
    validate_masks(masks)?;

    // Construct a mask with the lowest `bit_count` bits set (e.g. bit_count=24 -> 0x00FFFFFF).
    // Note that if bit_count is over 32, this will silently ignore it and assume 32 to avoid
    // overflows. (bit_count validity should be checked elsewhere)
    let bit_count = bpp.bit_count();
    let pixel_mask = if bit_count == 0 {
        0
    } else {
        u32::MAX >> (32u16.saturating_sub(bit_count))
    };

    for &(mask, channel) in masks {
        if mask & !pixel_mask != 0 {
            return Err(ColorMaskError::ExceedsBitDepth { mask, channel, bpp });
        }
    }

    Ok(())
}

/// Identifies which color channel a bitmask corresponds to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorMaskChannel {
    Red,
    Green,
    Blue,
    Alpha,
}

impl core::fmt::Display for ColorMaskChannel {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let name = match self {
            Self::Red => "red",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Alpha => "alpha",
        };
        f.write_str(name)
    }
}

/// A bitfield mask layout, either RGB or RGBA.
///
/// This enum provides a unified abstraction over [`RgbMasks`] and
/// [`RgbaMasks`].
///
/// Generally, the RGB variant is used with the V3 header and is embedded into
/// the BMP file after the header only if the compression is BI_BITFIELDS. The
/// V4 header on the other hand directly contains the color masks, including
/// the alpha channel.
///
/// Note that for the V3 embedded masks, some BMPs utilize a special compression
/// variant called `BI_ALPHABITFIELDS`, which implies that the contained
/// bitmasks do hold an alpha mask too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorMasks {
    /// Red, green, and blue masks only.
    Rgb(RgbMasks),

    /// Red, green, blue, and alpha masks.
    Rgba(RgbaMasks),
}

/// Bitfield channel masks used with `BI_BITFIELDS` compression (no alpha).
///
/// These masks specify which bits in a pixel value correspond to which color
/// channel. Each mask must be contiguous and must not overlap with any other
/// channel mask.
///
/// The bits in the pixel are ordered from most significant to least significant
/// bits.
///
/// Identical to [`RgbaMasks`], but without an alpha channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RgbMasks {
    pub red_mask: u32,
    pub green_mask: u32,
    pub blue_mask: u32,
}

/// Bitfield channel masks used with `BI_BITFIELDS` compression.
///
/// These masks specify which bits in a pixel value correspond to which color
/// channel. Each mask must be contiguous and must not overlap with any other
/// channel mask.
///
/// The bits in the pixel are ordered from most significant to least significant
/// bits.
///
/// This structure represents layouts that include an explicit alpha mask.
///
/// ## Examples
///
/// A 16-bit bitmap using the RGB555 format would specify five bits each of red,
/// green, blue and alpha, as follows:
///
///```text
/// red   = 0b0111110000000000  (0x7C00)
/// green = 0b0000001111100000  (0x03E0)
/// blue  = 0b0000000000011111  (0x001F)
/// alpha = 0b0000000000000000  (0x0000)
///```
///
/// A 32-bit bitmap using the RGBA8888 format would specify eight bits each of
/// red, green, and blue using the mask values as follows:
///
/// ```text
/// alpha = 0b11111111_00000000_00000000_00000000  (0xFF000000)
/// red   = 0b00000000_11111111_00000000_00000000  (0x00FF0000)
/// green = 0b00000000_00000000_11111111_00000000  (0x0000FF00)
/// blue  = 0b00000000_00000000_00000000_11111111  (0x000000FF)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RgbaMasks {
    pub red_mask: u32,
    pub green_mask: u32,
    pub blue_mask: u32,
    pub alpha_mask: u32,
}

impl RgbMasks {
    fn as_slice(&self) -> [(u32, ColorMaskChannel); 3] {
        [
            (self.red_mask, ColorMaskChannel::Red),
            (self.green_mask, ColorMaskChannel::Green),
            (self.blue_mask, ColorMaskChannel::Blue),
        ]
    }

    pub(crate) fn validate_for_bpp(&self, bpp: BitsPerPixel) -> Result<(), ColorMaskError> {
        validate_masks_for_bpp(&self.as_slice(), bpp)
    }

    pub(crate) fn read_unchecked<R: Read>(reader: &mut R) -> io::Result<Self> {
        Ok(Self {
            red_mask: reader.read_u32::<LittleEndian>()?,
            green_mask: reader.read_u32::<LittleEndian>()?,
            blue_mask: reader.read_u32::<LittleEndian>()?,
        })
    }

    pub(crate) fn write_unchecked<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_u32::<LittleEndian>(self.red_mask)?;
        writer.write_u32::<LittleEndian>(self.green_mask)?;
        writer.write_u32::<LittleEndian>(self.blue_mask)?;

        Ok(())
    }
}

impl RgbaMasks {
    fn as_slice(&self) -> [(u32, ColorMaskChannel); 4] {
        [
            (self.red_mask, ColorMaskChannel::Red),
            (self.green_mask, ColorMaskChannel::Green),
            (self.blue_mask, ColorMaskChannel::Blue),
            (self.alpha_mask, ColorMaskChannel::Alpha),
        ]
    }

    pub(crate) fn validate_for_bpp(&self, bpp: BitsPerPixel) -> Result<(), ColorMaskError> {
        validate_masks_for_bpp(&self.as_slice(), bpp)
    }

    pub(crate) fn read_unchecked<R: Read>(reader: &mut R) -> io::Result<Self> {
        Ok(Self {
            red_mask: reader.read_u32::<LittleEndian>()?,
            green_mask: reader.read_u32::<LittleEndian>()?,
            blue_mask: reader.read_u32::<LittleEndian>()?,
            alpha_mask: reader.read_u32::<LittleEndian>()?,
        })
    }

    pub(crate) fn write_unchecked<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_u32::<LittleEndian>(self.red_mask)?;
        writer.write_u32::<LittleEndian>(self.green_mask)?;
        writer.write_u32::<LittleEndian>(self.blue_mask)?;
        writer.write_u32::<LittleEndian>(self.alpha_mask)?;

        Ok(())
    }
}

impl From<RgbMasks> for RgbaMasks {
    /// This assumes an alpha mask of 0
    fn from(value: RgbMasks) -> Self {
        Self {
            red_mask: value.red_mask,
            green_mask: value.green_mask,
            blue_mask: value.blue_mask,
            alpha_mask: 0,
        }
    }
}

impl From<RgbaMasks> for RgbMasks {
    /// This drops the alpha mask
    fn from(value: RgbaMasks) -> Self {
        Self {
            red_mask: value.red_mask,
            green_mask: value.green_mask,
            blue_mask: value.blue_mask,
        }
    }
}

impl RgbMasks {
    /// Returns the default RGB555 bit masks used by 16-bit BI_RGB bitmaps.
    ///
    /// Layout (LSB -> MSB):
    /// - Bits 0-4   : Blue   (5 bits)
    /// - Bits 5-9   : Green  (5 bits)
    /// - Bits 10-14 : Red    (5 bits)
    /// - Bit 15     : Unused (1 bit)
    ///
    /// Alpha is not used.
    #[must_use]
    pub const fn rgb555() -> Self {
        Self {
            red_mask: 0x0000_7C00,
            green_mask: 0x0000_03E0,
            blue_mask: 0x0000_001F,
        }
    }

    /// Returns RGB565 bit masks commonly used with 16-bit BI_BITFIELDS bitmaps.
    ///
    /// Layout (LSB -> MSB):
    /// - Bits 0-4   : Blue  (5 bits)
    /// - Bits 5-10  : Green (6 bits)
    /// - Bits 11-15 : Red   (5 bits)
    ///
    /// Alpha is not used.
    #[must_use]
    pub const fn rgb565() -> Self {
        Self {
            red_mask: 0x0000_F800,
            green_mask: 0x0000_07E0,
            blue_mask: 0x0000_001F,
        }
    }

    /// Returns the default RGB888 bit masks used by 32-bit BI_RGB bitmaps.
    ///
    /// Layout (LSB -> MSB):
    /// - Bits 0-7   : Blue   (8 bits)
    /// - Bits 8-15  : Green  (8 bits)
    /// - Bits 16-23 : Red    (8 bits)
    /// - Bits 24-31 : Unused (8 bits)
    ///
    /// Alpha is not used.
    #[must_use]
    pub const fn rgb888() -> Self {
        Self {
            red_mask: 0x00FF_0000,
            green_mask: 0x0000_FF00,
            blue_mask: 0x0000_00FF,
        }
    }
}
