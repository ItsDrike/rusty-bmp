use std::io::{self, Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::raw::{
    error::{StructuralError, ValidationError},
    types::{BitsPerPixel, Compression},
    DibVariant,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

    pub(crate) fn validate(&self) -> Result<(), ValidationError> {
        // Width cannot be zero nor negative
        if self.width <= 0 {
            Err(ValidationError::InvalidWidth(self.width))?;
        }

        // Height cannot be zero
        if self.height == 0 {
            Err(ValidationError::InvalidHeight(self.height))?;
        }

        // Planes must always be 1
        if self.planes != 1 {
            Err(ValidationError::InvalidPlanes(self.planes))?;
        }

        self.bit_count.validate(DibVariant::Info)?;

        self.compression.validate_for_bpp(self.bit_count)?;

        // Top-down dibscannot be compressed
        if (self.height < 0) && !matches!(self.compression, Compression::Rgb | Compression::BitFields) {
            return Err(ValidationError::InvalidCompressionForTopDown(self.compression));
        }

        // Image size of 0 is valid only for uncompressed images (RGB or BITFIELDS)
        if self.image_size == 0 && !matches!(self.compression, Compression::Rgb | Compression::BitFields) {
            return Err(ValidationError::CompressedImageMissingSize(self.compression));
        }

        // For compressed images, the computed image size must always match the image size
        // reported in the header, if it was specified (non-zero). If it doesn't, it suggest
        // that the header is malformed.
        if self.image_size != 0
            && matches!(self.compression, Compression::Rgb | Compression::BitFields)
            && let Ok(computed) = self.pixel_data_size()
            && computed != self.image_size
        {
            return Err(ValidationError::UncompressedImageSizeMismatch {
                reported: self.image_size,
                computed,
            });
        }

        // This would suggest there is meant to be a color table with JPEG/PNG
        // encoded image. That makes no sense though and we should refuse it.
        if matches!(self.compression, Compression::Jpeg | Compression::Png) && self.colors_used != 0 {
            return Err(ValidationError::PaletteNotAllowedForCompression {
                compression: self.compression,
                colors_used: self.colors_used,
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

    pub(crate) fn color_table_size(&self) -> Result<u32, StructuralError> {
        // This is a special case, only valid when compression is JPEG/PNG
        // (We don't validate it here, but this should mean colors_used = 0
        // in the header too, validation does enforce this, we intentionally
        // don't)
        if self.bit_count == BitsPerPixel::Bpp0 {
            return Ok(0);
        }

        if self.colors_used == 0 {
            return Ok(match self.bit_count {
                // indexed bitmap
                // (the color table has as many entries as there are representable
                // colors for the bit count)
                BitsPerPixel::Bpp1 | BitsPerPixel::Bpp4 | BitsPerPixel::Bpp8 => {
                    let bits = self.bit_count.bit_count();

                    // compute max colors for this amount of bits
                    1u32.checked_shl(bits as u32).ok_or_else(|| {
                        // should never happen (1u32 << 1 | 4 | 8 cannot overflow)
                        StructuralError::ArithmeticOverflow(format!(
                            "bit count of {0} is too large to safely compute max colors for the color table size",
                            bits
                        ))
                    })?
                }
                // direct / packed bitmap
                // (doesn't use the color table)
                BitsPerPixel::Bpp16 | BitsPerPixel::Bpp24 | BitsPerPixel::Bpp32 => 0,
                // no other bpp values are supported
                // (no defined way to compute the color table size if not given)
                _ => {
                    return Err(StructuralError::UnsupportedStructure(format!(
                        "cannot compute color table size for unsupported bits-per-pixel value: {0}",
                        self.bit_count
                    )));
                }
            });
        }

        Ok(self.colors_used)
    }

    pub(crate) fn pixel_data_size(&self) -> Result<u32, StructuralError> {
        match self.compression {
            // uncompressed formats: compute row stride and total size dynamically
            Compression::Rgb | Compression::BitFields => {
                let width = self.width.unsigned_abs();
                let height = self.height.unsigned_abs();
                let bits = self.bit_count.bit_count();

                let row_stride = (bits as u32)
                    .checked_mul(width)
                    .and_then(|bits_per_row| bits_per_row.checked_add(31))
                    .map(|x| (x / 32) * 4)
                    .ok_or(StructuralError::ArithmeticOverflow(
                        "row stride (pixel data size)".to_owned(),
                    ))?;

                let image_size_computed =
                    row_stride
                        .checked_mul(height)
                        .ok_or(StructuralError::ArithmeticOverflow(
                            "image size (pixel data size)".to_owned(),
                        ))?;

                // This intentionally ignores the image_size field from the header and uses the
                // computed size, as that is the structurally valid size, size mismatch is checked
                // during validation, if checking is desired.
                Ok(image_size_computed)
            }
            // for other compressed formats, we can only obtain the size info from the header.
            _ => Ok(self.image_size),
        }
    }
}
