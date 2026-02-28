use std::io::{self, Read, Write};

use byteorder::{ReadBytesExt, WriteBytesExt};

use crate::raw::{BmpError, BmpResult};

/// Describes a color consisting of relative intensities of red, green, and blue.
///
/// In the Microsoft documentation (wingdi.h), this is referred to as the
/// `RGBTRIPLE` structure.
///
/// Note: The color components are stored in BGR order, not RGB. The first byte is
/// blue, the second is green, and the third is red.
///
/// Reference:
/// <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-rgbtriple>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbTriple {
    /// The intensity of blue in the color.
    pub blue: u8,

    /// The intensity of green in the color.
    pub green: u8,

    /// The intensity of red in the color.
    pub red: u8,
}

impl RgbTriple {
    pub(crate) fn read<R: Read>(reader: &mut R) -> io::Result<Self> {
        Ok(Self {
            blue: reader.read_u8()?,
            green: reader.read_u8()?,
            red: reader.read_u8()?,
        })
    }

    pub(crate) fn write<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_u8(self.blue)?;
        writer.write_u8(self.green)?;
        writer.write_u8(self.red)?;
        Ok(())
    }
}

/// Describes a color consisting of relative intensities of red, green, and blue.
///
/// In the Microsoft documentation (wingdi.h), this is referred to as the
/// `RGBQUAD` structure.
///
/// Note: This is often mistaken for RGBA/BGRA, that is however not the case here;
/// according to the spec, the color components here are stored in BGR order (not
/// RGB), with first byte is blue, the second is green, third is red, last is
/// reserved (not alpha).
///
/// Reference:
/// <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-rgbquad>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbQuad {
    /// The intensity of blue in the color.
    pub blue: u8,

    /// The intensity of green in the color.
    pub green: u8,

    /// The intensity of red in the color.
    pub red: u8,

    /// This member is reserved and must be zero.
    pub reserved: u8,
}

impl RgbQuad {
    pub(crate) fn validate(&self) -> BmpResult<()> {
        if self.reserved != 0 {
            return Err(BmpError::InvalidRgbQuad(*self));
        }

        Ok(())
    }

    pub(crate) fn read_unchecked<R: Read>(reader: &mut R) -> io::Result<Self> {
        Ok(Self {
            blue: reader.read_u8()?,
            green: reader.read_u8()?,
            red: reader.read_u8()?,
            reserved: reader.read_u8()?,
        })
    }

    pub(crate) fn write_unchecked<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_u8(self.blue)?;
        writer.write_u8(self.green)?;
        writer.write_u8(self.red)?;
        writer.write_u8(self.reserved)?;
        Ok(())
    }
}
