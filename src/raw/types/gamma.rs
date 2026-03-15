use std::io::{self, Read, Write};

use crate::raw::FixedPoint16Dot16;

/// Gamma correction values for the RGB channels.
///
/// This is only meaningful when the color space type is `CalibratedRgb`.
///
/// Values are in fixed-point 16.16 format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GammaTriple {
    /// Toned response curve for red.
    pub red: FixedPoint16Dot16,

    /// Toned response curve for green.
    pub green: FixedPoint16Dot16,

    /// Toned response curve for blue.
    pub blue: FixedPoint16Dot16,
}

impl GammaTriple {
    pub(crate) fn read<R: Read>(reader: &mut R) -> io::Result<Self> {
        Ok(Self {
            red: FixedPoint16Dot16::read(reader)?,
            green: FixedPoint16Dot16::read(reader)?,
            blue: FixedPoint16Dot16::read(reader)?,
        })
    }

    pub(crate) fn write<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        self.red.write(writer)?;
        self.green.write(writer)?;
        self.blue.write(writer)?;
        Ok(())
    }
}
