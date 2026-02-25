use std::io::{Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

/// Recognized valid Bits-Per-Pixel (color depth) values for a DIB
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitsPerPixel {
    /// The number of bits per pixel is specified or implied by the JPEG or PNG
    /// file format.
    Bpp0,

    /// The bitmap is monochrome.
    ///
    /// The DIB contains a color palette with two entries. Each bit in the
    /// bitmap array represents a pixel. If the bit is clear, the pixel is
    /// displayed with the color of the first entry in the color palette. If the
    /// bit is set, the pixel has the color of the second entry in the palette.
    Bpp1,

    /// The bitmap has a maximum of 16 distinct colors.
    ///
    /// The DIB contains a color palette with up to 16 entires. Each pixel in
    /// the bitmap is represented by a 4-bit index into the color palette table.
    ///
    /// For example, if the first byte in the bitmap is 0x1F, the byte
    /// represents two pixels. The first pixel contains the color in the second
    /// color palette table entry, and the second pixel contains the color in
    /// the sixteenth table entry.
    Bpp4,

    /// The bitmap has a maximum of 256 distinct colors.
    ///
    /// The DIB contains a color palette table with up to 256 entires. In this
    /// case, each byte in the bitmap array represents a single pixel.
    Bpp8,

    /// The bitmap has a maximum of 2^16 colors.
    ///
    /// If the compression is set to BI_RGB, each WORD (2 bytes) in the bitmap
    /// array represents a single pixel. The relative intensities of red, green,
    /// and blue are represented in the RGB555 format (with five bits for each
    /// color component. The value for blue is the least significant five bits,
    /// followed by five bits each for green and red. The most significant bit
    /// is not used).
    ///
    /// In this mode, the color palette is used only for optimizing colors on
    /// palette-based devices, and must contain the number of entries specified
    /// by the colors_used value of the DIB header.
    ///
    /// If the compression is set to BI_BITFIELDS, the color palette contains
    /// three 2-byte color masks that specify the red, green and blue components
    /// respectively, of each pixel. Each byte in the bitmap array represents a
    /// single pixel, with the bit mask being used to extract the individual
    /// color components. The stored bit masks must not overlap. All the bits in
    /// the pixel do not need to be used.
    Bpp16,

    /// The bitmap has a maximum of 2^24 colors.
    ///
    /// In this format, each 3-byte triplet in the bitmap array represents the
    /// relative intensities of blue, green and red, respectively, for a pixel.
    ///
    /// In this mode, the color palette is used only for optimizing colors on
    /// palette-based devices, and must contain the number of entries specified
    /// by the colors_used value of the DIB header.
    Bpp24,

    /// The bitmap has a maximum of 2^32 colors.
    ///
    /// If the compression is set to BI_RGB, each DWORD (4-bytes) in the bitmap
    /// array represents a single pixel. The relative intensities of blue, green,
    /// and red for a pixel are represented in the RGB888 format (with the value
    /// of blue in the least significant 8 bits, followed by 8 bits each for
    /// green and red. The high byte in each DWORD is not used.)
    ///
    /// In this mode, the color palette is used only for optimizing colors on
    /// palette-based devices, and must contain the number of entries specified
    /// by the colors_used value of the DIB header.
    ///
    /// If the compression is set to BI_BITFIELDS, the color palette will
    /// contain three DWORD color masks that specify the red, green and blue
    /// components of each pixel. Each DWORD in the bitmap array represents a
    /// single pixel.
    Bpp32,

    /// This bits-per-pixel value is not recognized as any of the common variants.
    Other(u16),
}

impl BitsPerPixel {
    pub(crate) fn read<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        let raw = reader.read_u16::<LittleEndian>()?;
        Ok(match raw {
            0 => Self::Bpp0,
            1 => Self::Bpp1,
            4 => Self::Bpp4,
            8 => Self::Bpp8,
            16 => Self::Bpp16,
            24 => Self::Bpp24,
            32 => Self::Bpp32,
            _ => Self::Other(raw),
        })
    }

    pub(crate) fn write<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let raw: u16 = self.bit_count();
        writer.write_u16::<LittleEndian>(raw)
    }

    #[inline]
    #[must_use]
    pub fn bit_count(self) -> u16 {
        match self {
            Self::Bpp0 => 0,
            Self::Bpp1 => 1,
            Self::Bpp4 => 4,
            Self::Bpp8 => 8,
            Self::Bpp16 => 16,
            Self::Bpp24 => 24,
            Self::Bpp32 => 32,
            Self::Other(raw) => raw,
        }
    }
}

impl std::fmt::Display for BitsPerPixel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.bit_count())
    }
}
