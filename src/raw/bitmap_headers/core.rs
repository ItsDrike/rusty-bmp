use std::io::{self, Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::raw::{
    error::{StructuralError, ValidationError},
    types::BitsPerPixel,
    DibVariant,
};

/// The BMP CORE (12 byte) header.
///
/// In the Microsoft documentation (wingdi.h), this is referred to as the
/// `BITMAPCOREHEADER` structure.
///
/// This header was used by Windows 2.x and OS/2 1.x.
///
/// Note that this implementation follows the Microsoft specification, not the OS/2 spec.
/// That said, these headers are entirely compatible, which does coincidentally mean that
/// OS/2 1.x BMP files will parse too.
///
/// Reference:
/// <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-bitmapcoreheader>
///
/// Note:
/// This format is obsolete and rarely encountered today.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct BitmapCoreHeader {
    /// The width of the bitmap, in pixels.
    ///
    /// Width does not include any scan-line boundary padding.
    pub width: u16,

    /// The height of the bitmap, in pixels.
    pub height: u16,

    /// The number of planes for the target device.
    /// This value must be set to 1.
    ///
    /// This is essentially just a historical artifact.
    pub planes: u16,

    /// The number of bits per pixel / color depth.
    pub bit_count: BitsPerPixel,
}

impl BitmapCoreHeader {
    pub const HEADER_SIZE: u32 = 12;

    pub(crate) fn validate(&self) -> Result<(), ValidationError> {
        // Width cannot be zero
        if self.width == 0 {
            return Err(ValidationError::InvalidWidth(i32::from(self.width)));
        }

        // Height cannot be zero
        if self.height == 0 {
            return Err(ValidationError::InvalidHeight(i32::from(self.height)));
        }

        // Planes must always be 1
        if self.planes != 1 {
            return Err(ValidationError::InvalidPlanes(self.planes));
        }

        self.bit_count.validate(DibVariant::Core)?;

        Ok(())
    }

    pub(crate) fn read_unchecked<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        Ok(Self {
            width: reader.read_u16::<LittleEndian>()?,
            height: reader.read_u16::<LittleEndian>()?,
            planes: reader.read_u16::<LittleEndian>()?,
            bit_count: BitsPerPixel::read(reader)?,
        })
    }

    pub(crate) fn write_unchecked<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_u16::<LittleEndian>(self.width)?;
        writer.write_u16::<LittleEndian>(self.height)?;
        writer.write_u16::<LittleEndian>(self.planes)?;
        self.bit_count.write(writer)?;
        Ok(())
    }

    pub(crate) fn color_table_size(&self) -> Result<u32, StructuralError> {
        Ok(match self.bit_count {
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
            BitsPerPixel::Bpp24 => 0,
            _ => {
                return Err(StructuralError::UnsupportedStructure(format!(
                    "cannot compute color table size for unsupported bits-per-pixel value: {0}",
                    self.bit_count
                )));
            }
        })
    }

    pub(crate) fn pixel_data_size(&self) -> Result<u32, StructuralError> {
        let bits = self.bit_count.bit_count();

        let row_stride = (bits as u32)
            .checked_mul(self.width as u32)
            .and_then(|bits_per_row| bits_per_row.checked_add(31))
            .map(|x| (x / 32) * 4)
            .ok_or(StructuralError::ArithmeticOverflow(
                "row stride (pixel data size)".to_owned(),
            ))?;

        let image_size = row_stride
            .checked_mul(self.height as u32)
            .ok_or(StructuralError::ArithmeticOverflow(
                "image size (pixel data size)".to_owned(),
            ))?;

        Ok(image_size)
    }
}
