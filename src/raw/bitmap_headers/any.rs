use std::io::{Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::raw::{
    BitmapCoreHeader, BmpError, BmpResult,
    bitmap_headers::{BitmapInfoHeader, BitmapV4Header, BitmapV5Header},
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
            Self::V4(header) => header.validate_v4(),
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

        header.validate()?;
        Ok(header)
    }

    pub(crate) fn write_unchecked<W: Write>(&self, writer: &mut W) -> BmpResult<()> {
        self.validate()?;

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

    pub(crate) fn bit_count(&self) -> u16 {
        match self {
            Self::Core(h) => h.bit_count,
            Self::Info(h) => h.bit_count,
            Self::V4(h) => h.info.bit_count,
            Self::V5(h) => h.v4.info.bit_count,
        }
    }

    pub(crate) fn compression(&self) -> u32 {
        match self {
            Self::Core(_) => wingdi::BI_RGB,
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

    pub(crate) fn color_table_size(&self) -> BmpResult<u32> {
        let bit_count = self.bit_count();
        if bit_count > 32 {
            return Err(BmpError::InvalidBitCount(bit_count));
        }

        let max_colors = 1u64 << bit_count;

        let colors_used = match self {
            // The CORE variant doesn't hold the size of the color palette.
            // It acts the same as if there was a 0 here in the other variants.
            Self::Core(_) => 0,
            Self::Info(h) => h.colors_used,
            Self::V4(h) => h.info.colors_used,
            Self::V5(h) => h.v4.info.colors_used,
        };

        if colors_used == 0 {
            return Ok(match bit_count {
                1 | 4 | 8 => max_colors as u32, // indexed bitmap
                16 | 24 | 32 => 0,              // direct / packed bitmap
                _ => return Err(BmpError::InvalidBitCount(bit_count)),
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
        let image_size = match self {
            // doesn't hold image_size, but only has BI_RGB, so we'd compute it anyways
            Self::Core(_) => 0,
            Self::Info(h) => h.image_size,
            Self::V4(h) => h.info.image_size,
            Self::V5(h) => h.v4.info.image_size,
        };

        if matches!(self.compression(), wingdi::BI_RGB | wingdi::BI_BITFIELDS) {
            let width = self.width().unsigned_abs();
            let height = self.height().unsigned_abs();

            if width == 0 {
                return Err(BmpError::InvalidWidth(width as i32));
            }
            if height == 0 {
                return Err(BmpError::InvalidHeight(height as i32));
            }

            let bpp = u32::from(self.bit_count());
            if !matches!(bpp, 1 | 4 | 8 | 16 | 24 | 32) {
                return Err(BmpError::InvalidBitCount(bpp as u16));
            }

            let bits_per_row = bpp.checked_mul(width).ok_or(BmpError::PixelDataTooLarge)?;
            let row_size = (bits_per_row.checked_add(31).ok_or(BmpError::PixelDataTooLarge)? / 32)
                .checked_mul(4)
                .ok_or(BmpError::PixelDataTooLarge)?;

            let pixel_array_size = row_size.checked_mul(height).ok_or(BmpError::PixelDataTooLarge)?;

            // In most cases, for uncompressed images, the image_size will be 0. However, if it
            // isn't, it should always match the computed size. If it doesn't, we end with an
            // error, as it means the header is malformed in some way. Either the width/height/bpp
            // is wrong, and now doesn't match the image_size, or the image_size is wrong. But we
            // have no way of telling which information we should trust, and if the data is
            // malformed, even if we tried to naively continue and accept the computed size as
            // truth, it could easily result in the image showing up as malformed.
            if image_size != 0 && pixel_array_size != image_size {
                return Err(BmpError::InvalidUncompressedImageSize {
                    expected: pixel_array_size,
                    header: image_size,
                });
            }

            return Ok(pixel_array_size);
        }

        if image_size == 0 {
            return Err(BmpError::InvalidImageSizeForCompression {
                image_size,
                compression: self.compression(),
            });
        }

        Ok(image_size)
    }
}
