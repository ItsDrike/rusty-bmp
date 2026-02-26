use std::io::{Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::raw::{BmpError, BmpResult, bitmap_headers::BitmapV4Header, helpers::read_array, types::ColorSpaceType};

/// The BMP V5 (124 byte) header.
///
/// In the Microsoft documentation (wingdi.h), this is referred to as the
/// `BITMAPV5HEADER` structure.
///
/// This format was introduced in Windows NT 5.0 and Windows 95.
///
/// This format cleanly extends the V4 header with additional fields,
/// specifically, fields for ICC color profile support.
///
/// Reference:
/// <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-bitmapv5header>
///
/// Note:
/// This is a fairly commonly used format for storing modern BMPs, though not as
/// common as the INFO or V4 header variants.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct BitmapV5Header {
    /// This format fully contains all fields from the V4 header.
    ///
    /// These fields are preserved in it through composition.
    pub v4: BitmapV4Header,

    /// Rendering intent for bitmap.
    ///
    /// The rendering intent determines how colors are mapped when converting from
    /// the bitmap's source color space to a destination color space, especially
    /// when colors fall outside the destination gamut.
    ///
    /// This field is only meaningful when color management is used (e.g. when an
    /// ICC profile is embedded or linked). It does not affect how pixel data is
    /// stored.
    pub intent: u32,

    /// The offset, in bytes, from the beginning of the bitmap header
    /// structure to the start of the profile data.
    ///
    /// The meaning of this depends on the color space type (`v4.cs_type`).
    ///
    /// * If the color space is neither PROFILE_EMBEDDED nor PROFILE_LINKED,
    ///   the value of this can be ignored.
    ///
    /// * If the color space is PROFILE_LINKED, profile data under this offset
    ///   will contain a NULL terminated string following the Windows ANSI
    ///   encoding (code page 1252), holding a path to the linked profile.
    ///   Generally, this will be a file-system path, but it can also be a
    ///   network path. E.g.:
    ///
    ///     - C:\Windows\System32\spool\drivers\color\profile.icm
    ///     - \\Server\Share\profile.icm
    ///
    /// * If the color space is PROFILE_EMBEDDED, the profile data is stored at
    ///   this offset will be the full embedded ICC profile structure.
    pub profile_data: u32,

    /// Size, in bytes, of embedded profile data (from `profile_data` offset).
    ///
    /// For linked profiles, even though they use a NULL terminated string, this
    /// field is authoritative for the string size, if NULL is encountered
    /// sooner, the string can be assumed to end at that point, however, if NULL
    /// isn't encountered within this size, the color profile data should be
    /// considered as malformed.
    pub profile_size: u32,

    /// This member has been reserved.
    ///
    /// Its value should be set to zero.
    pub reserved: [u8; 4],
}

impl BitmapV5Header {
    pub const HEADER_SIZE: u32 = 124;

    pub(crate) fn validate(&self) -> BmpResult<()> {
        self.v4.validate_base()?;

        // Only the following values are allowed for the color space type field
        if !matches!(
            self.v4.cs_type,
            ColorSpaceType::CalibratedRgb
                | ColorSpaceType::SRgb
                | ColorSpaceType::WindowsColorSpace
                | ColorSpaceType::ProfileEmbedded
                | ColorSpaceType::ProfileLinked
        ) {
            return Err(BmpError::InvalidColorSpaceType(self.v4.cs_type));
        }

        // If we have profile data in the DIB, `profile_data` holds an offset
        // for where they are, from the beginning of the header. That means it
        // should never be less than the header size (the profile data must be
        // outside of the header)
        if matches!(
            self.v4.cs_type,
            ColorSpaceType::ProfileEmbedded | ColorSpaceType::ProfileLinked
        ) && self.profile_data < Self::HEADER_SIZE
        {
            return Err(BmpError::InvalidProfileOffset(self.profile_data));
        }

        Ok(())
    }

    pub(crate) fn read_unchecked<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        Ok(Self {
            v4: BitmapV4Header::read_unchecked(reader)?,
            intent: reader.read_u32::<LittleEndian>()?,
            profile_data: reader.read_u32::<LittleEndian>()?,
            profile_size: reader.read_u32::<LittleEndian>()?,
            reserved: read_array::<4, _>(reader)?,
        })
    }

    pub(crate) fn write_unchecked<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        self.v4.write_unchecked(writer)?;
        writer.write_u32::<LittleEndian>(self.intent)?;
        writer.write_u32::<LittleEndian>(self.profile_data)?;
        writer.write_u32::<LittleEndian>(self.profile_size)?;
        writer.write_all(&self.reserved)?;

        Ok(())
    }
}
