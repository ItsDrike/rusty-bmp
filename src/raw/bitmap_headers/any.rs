use std::io::{Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::raw::{
    BitmapCoreHeader, BmpError, BmpResult,
    bitmap_headers::{BitmapInfoHeader, BitmapV4Header, BitmapV5Header},
    types::{BitsPerPixel, Compression},
    wingdi,
};

pub enum BitmapHeader {
    Core(BitmapCoreHeader),
    Info(BitmapInfoHeader),
    V4(BitmapV4Header),
    V5(BitmapV5Header),
}

impl BitmapHeader {
    pub(crate) fn validate(&self) -> BmpResult<()> {
        match self {
            Self::Core(header) => header.validate(),
            Self::Info(header) => header.validate(),
            Self::V4(header) => header.validate(),
            Self::V5(header) => header.validate(),
        }
    }

    pub(crate) fn read_unchecked<R: Read>(reader: &mut R) -> BmpResult<Self> {
        let size = reader.read_u32::<LittleEndian>()?;

        let header = match size {
            BitmapCoreHeader::HEADER_SIZE => Self::Core(BitmapCoreHeader::read_unchecked(reader)?),
            BitmapInfoHeader::HEADER_SIZE => Self::Info(BitmapInfoHeader::read_unchecked(reader)?),
            BitmapV4Header::HEADER_SIZE => Self::V4(BitmapV4Header::read_unchecked(reader)?),
            BitmapV5Header::HEADER_SIZE => Self::V5(BitmapV5Header::read_unchecked(reader)?),
            _ => return Err(BmpError::InvalidHeaderSize(size)),
        };

        Ok(header)
    }

    pub(crate) fn write_unchecked<W: Write>(&self, writer: &mut W) -> BmpResult<()> {
        match self {
            Self::Core(header) => {
                writer.write_u32::<LittleEndian>(BitmapCoreHeader::HEADER_SIZE)?;
                header.write_unchecked(writer)?;
            }
            Self::Info(header) => {
                writer.write_u32::<LittleEndian>(BitmapInfoHeader::HEADER_SIZE)?;
                header.write_unchecked(writer)?;
            }
            Self::V4(header) => {
                writer.write_u32::<LittleEndian>(BitmapV4Header::HEADER_SIZE)?;
                header.write_unchecked(writer)?;
            }
            Self::V5(header) => {
                writer.write_u32::<LittleEndian>(BitmapV5Header::HEADER_SIZE)?;
                header.write_unchecked(writer)?;
            }
        }

        Ok(())
    }

    pub(crate) fn bit_count(&self) -> BitsPerPixel {
        match self {
            Self::Core(h) => h.bit_count,
            Self::Info(h) => h.bit_count,
            Self::V4(h) => h.info.bit_count,
            Self::V5(h) => h.v4.info.bit_count,
        }
    }

    pub(crate) fn compression(&self) -> Compression {
        match self {
            Self::Core(_) => Compression::Rgb,
            Self::Info(h) => h.compression,
            Self::V4(h) => h.info.compression,
            Self::V5(h) => h.v4.info.compression,
        }
    }

    pub(crate) fn width(&self) -> i32 {
        match self {
            Self::Core(h) => h.width as i32,
            Self::Info(h) => h.width,
            Self::V4(h) => h.info.width,
            Self::V5(h) => h.v4.info.width,
        }
    }

    pub(crate) fn height(&self) -> i32 {
        match self {
            Self::Core(h) => h.height as i32,
            Self::Info(h) => h.height,
            Self::V4(h) => h.info.height,
            Self::V5(h) => h.v4.info.height,
        }
    }

    pub(crate) fn image_size(&self) -> u32 {
        match self {
            // doesn't hold image_size, but only has BI_RGB, so the image_size is
            // computable and equivalent to being 0 in the other variants.
            Self::Core(_) => 0,
            Self::Info(h) => h.image_size,
            Self::V4(h) => h.info.image_size,
            Self::V5(h) => h.v4.info.image_size,
        }
    }

    pub(crate) fn color_table_size(&self) -> BmpResult<u32> {
        let bit_count = self.bit_count();

        let colors_used = match self {
            // The CORE variant doesn't hold the size of the color palette.
            // It acts the same as if there was a 0 here in the other variants.
            Self::Core(_) => 0,
            Self::Info(h) => h.colors_used,
            Self::V4(h) => h.info.colors_used,
            Self::V5(h) => h.v4.info.colors_used,
        };

        // This is a special case, only valid when compression is JPEG/PNG
        if bit_count == BitsPerPixel::Bpp0 {
            let compression = self.compression();
            if !matches!(compression, Compression::Jpeg | Compression::Png) {
                return Err(BmpError::InvalidCompressionForBpp {
                    compression: self.compression(),
                    bpp: bit_count,
                });
            }

            // This would suggest there is meant to be a color table with a JPEG/PNG
            // encoded image. That makes no sense though and we should refuse it.
            if colors_used != 0 {
                return Err(BmpError::PaletteNotAllowedForCompression {
                    colors_used,
                    compression,
                });
            }

            return Ok(0);
        }

        // Check to make sure max_colors doesn't overflow on max_colors
        match bit_count {
            BitsPerPixel::Bpp0 => unreachable!("handled above"),
            BitsPerPixel::Other(x) => return Err(BmpError::InvalidBitCount(x)),
            _ => {}
        }
        let max_colors = 1u64 << bit_count.bit_count();

        if colors_used == 0 {
            return Ok(match bit_count {
                BitsPerPixel::Bpp1 | BitsPerPixel::Bpp4 | BitsPerPixel::Bpp8 => max_colors as u32, // indexed bitmap
                BitsPerPixel::Bpp16 | BitsPerPixel::Bpp24 | BitsPerPixel::Bpp32 => 0, // direct / packed bitmap
                _ => return Err(BmpError::InvalidBitCount(bit_count.bit_count())),
            });
        }

        // This is not technically spec-safe, as the spec does not actually
        // define an upper limit for the colors used amount, however, it makes
        // no sense to ever have this value be larger than max_colors, as the
        // other colors in the table would then just be unused.
        //
        // The only reason that I can see where this could be higher is when an
        // attacker is trying to maliciously craft an invalid BMP to do
        // something weird.
        //
        // For that reason, we reject these in here explicitly. Realistically,
        // no valid BMPs should be violating this.
        if colors_used as u64 > max_colors {
            return Err(BmpError::PaletteExceedsBitDepth {
                used: colors_used as u64,
                max: max_colors,
            });
        }

        Ok(colors_used)
    }

    pub(crate) fn pixel_data_size(&self) -> BmpResult<u32> {
        let image_size = self.image_size();
        let bpp = self.bit_count();
        let compression = self.compression();

        match compression {
            // uncompressed
            Compression::Rgb | Compression::BitFields => {
                let width = self.width().unsigned_abs();
                let height = self.height().unsigned_abs();

                if width == 0 {
                    return Err(BmpError::InvalidWidth(width as i32));
                }
                if height == 0 {
                    return Err(BmpError::InvalidHeight(height as i32));
                }

                match bpp {
                    BitsPerPixel::Bpp0 => return Err(BmpError::InvalidCompressionForBpp { compression, bpp }),
                    BitsPerPixel::Other(x) => return Err(BmpError::InvalidBitCount(x)),
                    _ => {}
                }

                let bits_per_row = (bpp.bit_count() as u32)
                    .checked_mul(width)
                    .ok_or(BmpError::PixelDataTooLarge)?;
                let row_size = (bits_per_row.checked_add(31).ok_or(BmpError::PixelDataTooLarge)? / 32)
                    .checked_mul(4)
                    .ok_or(BmpError::PixelDataTooLarge)?;

                let image_size_computed = row_size.checked_mul(height).ok_or(BmpError::PixelDataTooLarge)?;

                // In most cases, for uncompressed images, the image_size will be 0. However, if it
                // isn't, it should always match the computed size. If it doesn't, we end with an
                // error, as it means the header is malformed in some way. Either the width/height/bpp
                // is wrong, and now doesn't match the image_size, or the image_size is wrong. But we
                // have no way of telling which information we should trust, and if the data is
                // malformed, even if we tried to naively continue and accept the computed size as
                // truth, it could easily result in the image showing up as malformed.
                if image_size != 0 && image_size_computed != image_size {
                    return Err(BmpError::InvalidUncompressedImageSize {
                        expected: image_size_computed,
                        header: image_size,
                    });
                }

                Ok(image_size_computed)
            }
            // other compressed formats
            _ => {
                if image_size == 0 {
                    return Err(BmpError::InvalidImageSizeForCompression {
                        image_size,
                        compression,
                    });
                }

                Ok(image_size)
            }
        }
    }
}
