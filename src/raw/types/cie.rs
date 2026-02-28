use std::io::{self, Read, Write};

use crate::raw::FixedPoint2Dot30;

/// A CIE XYZ color space endpoint.
///
/// This structure contains the x,y, and z coordinates of a specific color in
/// a specified color space.
///
/// In the Microsoft documentation (wingdi.h), this is referred to as the
/// `CIEXYZ` structure.
///
/// Reference:
/// <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-ciexyz>
///
/// See the 1931 CIE XYZ standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CieXyz {
    /// The x coordinate in fix point (2.30).
    pub x: FixedPoint2Dot30,

    /// The y coordinate in fix point (2.30).
    pub y: FixedPoint2Dot30,

    /// The z coordinate in fix point (2.30).
    pub z: FixedPoint2Dot30,
}

impl CieXyz {
    pub(crate) fn read<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        Ok(Self {
            x: FixedPoint2Dot30::read(reader)?,
            y: FixedPoint2Dot30::read(reader)?,
            z: FixedPoint2Dot30::read(reader)?,
        })
    }

    pub(crate) fn write<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        self.x.write(writer)?;
        self.y.write(writer)?;
        self.z.write(writer)?;
        Ok(())
    }
}

/// Defines the CIE XYZ endpoints for red, green, and blue.
///
/// This specifies the CIE X, Y, and Z coordinates for the red, green, and
/// blue endpoints for the logical color space associated with the bitmap.
///
/// This is only meaningful when the color space type is CalibratedRgb.
///
/// In the Microsoft documentation (wingdi.h), this is referred to as the
/// `CIEXYZTRIPLE` structure.
///
/// Reference:
/// <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-ciexyztriple>
///
/// See the 1931 CIE XYZ standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CieXyzTriple {
    /// The xyz coordinates of red endpoint.
    pub red: CieXyz,

    /// The xyz coordinates of green endpoint.
    pub green: CieXyz,

    /// The xyz coordinates of blue endpoint.
    pub blue: CieXyz,
}

impl CieXyzTriple {
    pub(crate) fn read<R: Read>(reader: &mut R) -> io::Result<Self> {
        Ok(Self {
            red: CieXyz::read(reader)?,
            green: CieXyz::read(reader)?,
            blue: CieXyz::read(reader)?,
        })
    }

    pub(crate) fn write<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        self.red.write(writer)?;
        self.green.write(writer)?;
        self.blue.write(writer)?;
        Ok(())
    }
}
