use std::io::{self, Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::raw::{BmpError, BmpResult, types::BitsPerPixel};

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

    pub(crate) fn validate(&self) -> BmpResult<()> {
        // Width cannot be zero
        if self.width == 0 {
            return Err(BmpError::InvalidWidth(i32::from(self.width)));
        }

        // Height cannot be zero
        if self.height == 0 {
            return Err(BmpError::InvalidHeight(i32::from(self.height)));
        }

        // Planes must always be 1
        if self.planes != 1 {
            return Err(BmpError::InvalidPlanes(self.planes));
        }

        // For the core header, only bpp values of: 1, 4, 8 or 24 are accepted
        if !matches!(
            self.bit_count,
            BitsPerPixel::Bpp1 | BitsPerPixel::Bpp4 | BitsPerPixel::Bpp8 | BitsPerPixel::Bpp24
        ) {
            return Err(BmpError::InvalidBitCount(self.bit_count.bit_count()));
        }

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
}
