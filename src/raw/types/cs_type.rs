use std::io::{Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::raw::wingdi;

/// Specifies how the RGB values in a V4/V5 DIB are to be interpreted with
/// respect to color management.
///
/// The color type does not change how the pixel data is stored in the bitmap.
/// Instead, it defines how the stored RGB values should be interpreted by a
/// color-managed system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpaceType {
    /// LCS_CALIBRATED_RGB
    ///
    /// This mode implies that the RGB values of the pixels in this bitmap are
    /// defined by XYZ primaries (end points) and gammas. The values for these
    /// settings are stored in the DIB header.
    ///
    /// This setting implies that the profile data and profile size information
    /// stored in the header should be ignored, and may contain garbage.
    CalibratedRgb,

    /// LCS_sRGB
    ///
    /// This mode implies that the bitmap is in sRGB color space.
    ///
    /// The gammas, end points, profile data and the profile size data embedded
    /// in the DIB header should be ignored and may hold bogus values.
    SRgb,

    /// LCS_WINDOWS_COLOR_SPACE
    ///
    /// It historically represented the system default profile, which in modern
    /// Windows resolves to sRGB unless the system has a different default
    /// display profile.
    ///
    /// The gammas, end points, profile data and the profile size data embedded
    /// in the DIB header should be ignored and may bogus values.
    WindowsColorSpace,

    /// PROFILE_EMBEDDED
    ///
    /// Only valid in V5+ Bitmaps.
    ///
    /// This indicates that the DIB uses an ICC profile to define the color
    /// space, with the profile data being embedded into the DIB file data.
    ///
    /// The profile data will be embedded in the DIB file at an offset from the
    /// DIB header, given by the profile data information in the header, and
    /// will contain profile size amount of bytes. Generally, this information
    /// will be embedded in the image after the bitmap array (at the end).
    ///
    /// The gammas and end point values set in the DIB header should be ignored
    /// and may hold bogus values.
    ProfileEmbedded,

    /// PROFILE_LINKED
    ///
    /// Only valid in V5+ Bitmaps.
    ///
    /// This indicates that the DIB uses an ICC profile to define the color
    /// space, with the profile path being embedded into the DIB file data.
    ///
    /// The profile path will be a string embedded in the DIB file at an offset
    /// from the DIB header, given by the profile data information in the
    /// header.
    ///
    /// This will be a NULL terminated string, however, the authoritative
    /// information for when the string ends in the file is based on the profile
    /// size value in the header, not by reading until the NULL terminator. That
    /// said, if the NULL terminator is within the read size, the string should
    /// be considered to end there. If the NULL terminator is not encountered
    /// within the read data, the DIB should be considered as corrupted/invalid.
    ///
    /// The embedded string must follow Windows ANSI encoding (code page 1252),
    /// valid UTF-8 should NOT be assumed. Generally, this will be a local
    /// (Windows) file-system path, but it can also be a network path. E.g.:
    ///
    /// - C:\Windows\System32\spool\drivers\color\sRGB Color Space Profile.icm
    /// - \\Server\Share\profile.icm
    ///
    /// The gammas and end point values set in the DIB header should be ignored
    /// and may hold bogus values.
    ProfileLinked,

    /// This color space value is not recognized as any of the common variants.
    Other(u32),
}

impl ColorSpaceType {
    pub(crate) fn read<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        let raw = reader.read_u32::<LittleEndian>()?;

        Ok(match raw {
            wingdi::LCS_CALIBRATED_RGB => Self::CalibratedRgb,
            wingdi::LCS_sRGB => Self::SRgb,
            wingdi::LCS_WINDOWS_COLOR_SPACE => Self::WindowsColorSpace,
            wingdi::PROFILE_EMBEDDED => Self::ProfileEmbedded,
            wingdi::PROFILE_LINKED => Self::ProfileLinked,
            _ => Self::Other(raw),
        })
    }

    pub(crate) fn write<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let raw = self.value();
        writer.write_u32::<LittleEndian>(raw)?;
        Ok(())
    }

    pub(crate) fn value(&self) -> u32 {
        match self {
            Self::CalibratedRgb => wingdi::LCS_CALIBRATED_RGB,
            Self::SRgb => wingdi::LCS_sRGB,
            Self::WindowsColorSpace => wingdi::LCS_WINDOWS_COLOR_SPACE,
            Self::ProfileEmbedded => wingdi::PROFILE_EMBEDDED,
            Self::ProfileLinked => wingdi::PROFILE_LINKED,
            Self::Other(x) => *x,
        }
    }
}
