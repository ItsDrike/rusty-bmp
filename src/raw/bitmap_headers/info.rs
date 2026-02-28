use std::io::{self, Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::raw::{
    BmpError, BmpResult,
    types::{BitsPerPixel, Compression},
};

/// The BMP INFO (40 byte) header.
///
/// In the Microsoft documentation (wingdi.h), this is referred to as the
/// `BITMAPINFOHEADER` structure.
///
/// Reference:
/// <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-bitmapinfoheader>
///
/// Note:
/// This is the most commonly used format for storing BMPs.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct BitmapInfoHeader {
    /// The width of the bitmap, in pixels.
    ///
    /// Width does not include any scan-line boundary padding.
    ///
    /// If compression is BI_JPEG or BI_PNG, width specifies the width of the
    /// decompressed JPEG or PNG image in pixels.
    pub width: i32,

    /// The height of the bitmap, in pixels.
    ///
    /// * If the value is positive, the bitmap is a bottom-up DIB and its origin
    ///   is the lower-left corner.
    /// * If the value is negative, the bitmap is a top-down DIB and its origin
    ///   is the upper-left corner.
    ///
    /// If the height is negative, indicating a top-down DIB, the compression
    /// must be either BI_RGB or BI_BITFIELDS. Top-down DIBs cannot be compressed.
    ///
    /// If compression is BI_JPEG or BI_PNG, width specifies the height of the
    /// decompressed JPEG or PNG image in pixels.
    pub height: i32,

    /// The number of planes for the target device.
    /// This value must be set to 1.
    ///
    /// This is essentially just a historical artifact.
    pub planes: u16,

    /// The number of bits per pixel / color depth.
    ///
    /// See the [`BitsPerPixel`] enum for more info.
    pub bit_count: BitsPerPixel,

    /// The compression (or encoding) method used for the bitmap data.
    ///
    /// See the [`Compression`] enum for more info.
    pub compression: Compression,

    /// The size, in bytes, of the image.
    ///
    /// This may be set to zero for uncompressed bitmaps.
    ///
    /// If compression is BI_JPEG or BI_PNG, this will be the size of the
    /// JPEG or PNG image buffer.
    pub image_size: u32,

    /// The horizontal resolution, in pixels-per-meter, of the target device.
    ///
    /// An application can use this value to select a bitmap from a resource
    /// group that best matches the characteristics of the current device.
    pub x_resolution_ppm: i32,

    /// The vertical resolution, in pixels-per-meter, of the target device.
    ///
    /// An application can use this value to select a bitmap from a resource
    /// group that best matches the characteristics of the current device.
    pub y_resolution_ppm: i32,

    /// The number of entries in the color table (palette).
    ///
    /// For indexed-color bitmaps (1, 4, or 8 bits per pixel):
    ///
    /// - If the value is non-zero, it specifies the exact number of color table
    ///   palette entries that follow the header.
    /// - If the value is zero, the palette contains the maximum number of
    ///   entries for the given bit depth (2^bit_count).
    ///
    /// For direct-color bitmaps (16, 24 or 32 bits per pixel):
    ///
    /// - If the value is non-zero, it specifies the number of color palette
    ///   entries that follow the header (and compression masks, if any). These
    ///   palette entries are not used for decoding pixel data. Pixel colors are
    ///   defined directly by their stored values. The purpose of having a color
    ///   table for these is mainly historical: This palette was used as a
    ///   display hint to help systems with limited-color hardware efficiently
    ///   map the image to a device palette.
    /// - Most modern BMPs set this value to zero and do not include a palette.
    pub colors_used: u32,

    /// The number of color indexes that are required for displaying the bitmap.
    ///
    /// * A value of zero indicates that all palette entries are equally
    ///   important.
    /// * A non-zero value means that the first N entries in the color table
    ///   palette should be considered important.
    ///
    /// For modern BMP files, this field is almost always 0.
    ///
    /// The purpose of this value is to allow display systems (hardware) with
    /// limited palette capacity to prioritize these colors for more accurate
    /// representation.
    ///
    /// For modern BMP files, this field is almost always 0 and is generally
    /// ignored.
    pub colors_important: u32,
}

impl BitmapInfoHeader {
    pub const HEADER_SIZE: u32 = 40;

    pub(crate) fn validate(&self) -> BmpResult<()> {
        // Width cannot be zero nor negative
        if self.width <= 0 {
            return Err(BmpError::InvalidWidth(self.width));
        }

        // Height cannot be zero
        if self.height == 0 {
            return Err(BmpError::InvalidHeight(self.height));
        }

        // Planes must always be 1
        if self.planes != 1 {
            return Err(BmpError::InvalidPlanes(self.planes));
        }

        // For the info header, only bpp values of 0, 1, 4, 8, 16, 24 and 32 are allowed
        if !matches!(
            self.bit_count,
            BitsPerPixel::Bpp0
                | BitsPerPixel::Bpp1
                | BitsPerPixel::Bpp4
                | BitsPerPixel::Bpp8
                | BitsPerPixel::Bpp16
                | BitsPerPixel::Bpp24
                | BitsPerPixel::Bpp32
        ) {
            return Err(BmpError::InvalidBitCount(self.bit_count.bit_count()));
        }

        // Top-down dibs cannot be compressed
        if (self.height < 0) && !matches!(self.compression, Compression::Rgb | Compression::BitFields) {
            return Err(BmpError::InvalidCompressionForTopDown {
                compression: self.compression,
            });
        }

        // The RLE compression can only be used with their expected bpp values
        // The BITFIELDS compression can only be used with bpp of 16 or 32
        // The JPEG/PNG compression can only be used with bpp of 0
        if (self.compression == Compression::Rle4 && self.bit_count != BitsPerPixel::Bpp4)
            || (self.compression == Compression::Rle8 && self.bit_count != BitsPerPixel::Bpp8)
            || (self.compression == Compression::BitFields
                && !matches!(self.bit_count, BitsPerPixel::Bpp16 | BitsPerPixel::Bpp32))
            || (matches!(self.compression, Compression::Png | Compression::Jpeg)
                && self.bit_count != BitsPerPixel::Bpp0)
        {
            return Err(BmpError::InvalidCompressionForBpp {
                compression: self.compression,
                bpp: self.bit_count,
            });
        }

        // Image size of 0 is valid only for uncompressed images (RGB or BITFIELDS)
        if self.image_size == 0 && !matches!(self.compression, Compression::Rgb | Compression::BitFields) {
            return Err(BmpError::InvalidImageSizeForCompression {
                image_size: self.image_size,
                compression: self.compression,
            });
        }

        Ok(())
    }

    pub(crate) fn read_unchecked<R: Read>(reader: &mut R) -> io::Result<Self> {
        Ok(Self {
            width: reader.read_i32::<LittleEndian>()?,
            height: reader.read_i32::<LittleEndian>()?,
            planes: reader.read_u16::<LittleEndian>()?,
            bit_count: BitsPerPixel::read(reader)?,
            compression: Compression::read(reader)?,
            image_size: reader.read_u32::<LittleEndian>()?,
            x_resolution_ppm: reader.read_i32::<LittleEndian>()?,
            y_resolution_ppm: reader.read_i32::<LittleEndian>()?,
            colors_used: reader.read_u32::<LittleEndian>()?,
            colors_important: reader.read_u32::<LittleEndian>()?,
        })
    }

    pub(crate) fn write_unchecked<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_i32::<LittleEndian>(self.width)?;
        writer.write_i32::<LittleEndian>(self.height)?;
        writer.write_u16::<LittleEndian>(self.planes)?;
        self.bit_count.write(writer)?;
        self.compression.write(writer)?;
        writer.write_u32::<LittleEndian>(self.image_size)?;
        writer.write_i32::<LittleEndian>(self.x_resolution_ppm)?;
        writer.write_i32::<LittleEndian>(self.y_resolution_ppm)?;
        writer.write_u32::<LittleEndian>(self.colors_used)?;
        writer.write_u32::<LittleEndian>(self.colors_important)?;
        Ok(())
    }
}
