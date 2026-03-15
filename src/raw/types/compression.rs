use std::io::{self, Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::raw::{BitsPerPixel, error::ValidationError, helpers::wingdi};

/// Compression methods defined for BMP files.
///
/// The numeric values are part of the BMP file format specification.
/// Not all compression methods are valid or meaningful for all DIB
/// header versions or bits-per-pixel values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Compression {
    /// `BI_RGB` (0) - no compression.
    ///
    /// * If the `bits_per_pixel` amount is 1, 4 or 8, the bitmap array
    ///   values are indices into the color table.
    /// * If the `bits_per_pixel` amount is 16, 24, or 32, the bitmap array
    ///   specifies the actual intensities of blue, green, and red rather than
    ///   using color table indexes. With bpp of 16, the format uses RGB 555
    ///
    /// This is the most common choice.
    Rgb,

    /// `BI_RLE8` (1) - run-length encoding for 8-bit paletted images.
    ///
    /// The compression format is a two-byte format consisting of a count byte
    /// followed by a byte containing a color index.
    ///
    /// This is only valid when used with 8-bpp bitmaps.
    Rle8,

    /// `BI_RLE4` (2) - run-length encoding for 4-bit paletted images.
    ///
    /// The compression format is a two-byte format consisting of a count byte
    /// followed by two word-length color indexes.
    ///
    /// This is only valid when used with 4-bpp bitmaps.
    Rle4,

    /// `BI_BITFIELDS` (3).
    ///
    /// Specifies that the bitmap is not compressed and that the color masks for
    /// the red, green, and blue components of each pixel are specified through
    /// explicitly defined bitmasks.
    ///
    /// This is only valid when used with 16- and 32-bpp bitmaps.
    ///
    /// Note that the OS/2 V2 bitmap format used the compression value of 3 for
    /// Huffman 1D compression. The bitfields meaning of it follows the Windows
    /// specification of the BMP format.
    BitFields,

    /// `BI_JPEG` (4).
    ///
    /// Specifies that the image is compressed using the JPEG file Interchange
    /// Format. The bitmap array holds embedded JPEG data.
    Jpeg,

    /// `BI_PNG` (5).
    ///
    /// Specifies that the image is compressed using the PNG file Interchange
    /// Format. The bitmap array holds embedded PNG data.
    Png,

    /// This compression value is not recognized as any of the common variants.
    Other(u32),
}

impl Compression {
    pub(crate) fn read<R: Read>(reader: &mut R) -> io::Result<Self> {
        let raw = reader.read_u32::<LittleEndian>()?;
        Ok(match raw {
            wingdi::BI_RGB => Self::Rgb,
            wingdi::BI_RLE4 => Self::Rle4,
            wingdi::BI_RLE8 => Self::Rle8,
            wingdi::BI_BITFIELDS => Self::BitFields,
            wingdi::BI_JPEG => Self::Jpeg,
            wingdi::BI_PNG => Self::Png,
            _ => Self::Other(raw),
        })
    }

    pub(crate) fn write<W: Write>(self, writer: &mut W) -> io::Result<()> {
        let raw = self.value();
        writer.write_u32::<LittleEndian>(raw)?;
        Ok(())
    }

    pub(crate) const fn value(self) -> u32 {
        match self {
            Self::Rgb => wingdi::BI_RGB,
            Self::Rle4 => wingdi::BI_RLE4,
            Self::Rle8 => wingdi::BI_RLE8,
            Self::BitFields => wingdi::BI_BITFIELDS,
            Self::Jpeg => wingdi::BI_JPEG,
            Self::Png => wingdi::BI_PNG,
            Self::Other(x) => x,
        }
    }

    /// Validates that the compression method is compatible with the given
    /// bits-per-pixel (`bpp`) value.
    ///
    /// The following constraints are enforced:
    /// - RLE4 compression requires 4 bits per pixel
    /// - RLE8 compression requires 8 bits per pixel
    /// - BITFIELDS compression is only valid for 16 or 32 bits per pixel
    /// - RGB compression is only valid for 24 bits per pixel
    /// - JPEG and PNG compression require `bpp = 0`
    pub(crate) const fn validate_for_bpp(self, bpp: BitsPerPixel) -> Result<(), ValidationError> {
        #[allow(clippy::match_same_arms)]
        match (self, bpp) {
            (Self::Rle4, BitsPerPixel::Bpp4) => {}
            (Self::Rle8, BitsPerPixel::Bpp8) => {}
            (Self::BitFields, BitsPerPixel::Bpp16 | BitsPerPixel::Bpp32) => {}
            (
                Self::Rgb,
                BitsPerPixel::Bpp1
                | BitsPerPixel::Bpp4
                | BitsPerPixel::Bpp8
                | BitsPerPixel::Bpp16
                | BitsPerPixel::Bpp24
                | BitsPerPixel::Bpp32,
            ) => {}
            (Self::Png | Self::Jpeg, BitsPerPixel::Bpp0) => {}
            (Self::Other(raw_bpp), _) => return Err(ValidationError::UnknownCompression(raw_bpp)),
            _ => {
                return Err(ValidationError::InvalidCompressionForBpp { compression: self, bpp });
            }
        }

        Ok(())
    }
}
