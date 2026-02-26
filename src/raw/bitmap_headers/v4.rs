use std::io::{Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::raw::{
    BmpError, BmpResult,
    bitmap_headers::BitmapInfoHeader,
    types::{CieXyzTriple, GammaTriple, RgbaMasks},
    wingdi,
};

/// The BMP V4 (108 byte) header.
///
/// In the Microsoft documentation (wingdi.h), this is referred to as the
/// `BITMAPV4HEADER` structure.
///
/// This format was introduced in Windows NT 4.0 and Windows 95.
///
/// This format cleanly extends the INFO header with additional fields,
/// specifically, fields for color masks and color space information.
///
/// Reference:
/// <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-bitmapv4header>
///
/// Note:
/// This is a fairly commonly used format for storing modern BMPs, though not as
/// common as the INFO header variant.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct BitmapV4Header {
    /// This format fully contains all fields from the INFO header.
    ///
    /// These fields are preserved in it through composition.
    pub info: BitmapInfoHeader,

    /// Specifies which bits in a pixel value correspond to which channel.
    /// Only used in 16- and 32-bit bitmaps.
    ///
    /// See the [`RgbaMasks`] structure for more info.
    pub masks: RgbaMasks,

    /// Specifies how the RGB values in the DIB are to be interpreted with
    /// respect to color management.
    ///
    /// The color type does not change how the pixel data is stored in the bitmap.
    /// Instead, it defines how the stored RGB values should be interpreted by a
    /// color-managed system.
    pub cs_type: u32,

    /// Defines the CIE XYZ endpoints for red, green and blue.
    ///
    /// This is only meaningful when cs_type is LCS_CALIBRATED_RGB.
    ///
    /// See the [`CieXyzTriple`] structure for more info.
    pub endpoints: CieXyzTriple,

    /// The toned response curves for red, green and blue channels.
    ///
    /// See the [`GammaTriple`] structure for more info.
    pub gamma: GammaTriple,
}

impl BitmapV4Header {
    pub const HEADER_SIZE: u32 = 108;

    pub(crate) fn validate(&self) -> BmpResult<()> {
        self.validate_base()?;

        // Only the following color space type values are allowed for V4
        if !matches!(
            self.cs_type,
            wingdi::LCS_CALIBRATED_RGB | wingdi::LCS_sRGB | wingdi::LCS_WINDOWS_COLOR_SPACE
        ) {
            return Err(BmpError::InvalidColorSpaceType(self.cs_type));
        }

        Ok(())
    }

    /// Validation logic that's shared to this variant, and also any other
    /// variants that contain this header as a composite value.
    ///
    /// This function only contains the non-specific validation that the other
    /// variants can reliably call and re-use, without validation code duplication,
    /// and without bringing in the invariants that do change between the header
    /// versions.
    pub(crate) fn validate_base(&self) -> BmpResult<()> {
        self.info.validate()?;
        self.masks.validate_for_bpp(self.info.bit_count)?;

        Ok(())
    }

    pub(crate) fn read_unchecked<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        Ok(Self {
            info: BitmapInfoHeader::read_unchecked(reader)?,
            masks: RgbaMasks::read_unchecked(reader)?,
            cs_type: reader.read_u32::<LittleEndian>()?,
            endpoints: CieXyzTriple::read(reader)?,
            gamma: GammaTriple::read(reader)?,
        })
    }

    pub(crate) fn write_unchecked<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        self.info.write_unchecked(writer)?;
        self.masks.write_unchecked(writer)?;
        writer.write_u32::<LittleEndian>(self.cs_type)?;
        self.endpoints.write(writer)?;
        self.gamma.write(writer)?;

        Ok(())
    }
}
