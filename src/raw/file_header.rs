use std::io::{self, Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::raw::{error::ValidationError, helpers::read_array};

/// The BMP file header.
///
/// In the Microsoft documentation (wingdi.h), this is referred to as the
/// `BITMAPFILEHEADER` structure.
///
/// This header is always exactly 14 bytes long and is shared by all BMP variants,
/// including the historical OS/2 formats.
///
/// Reference:
/// <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-bitmapfileheader>
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHeader {
    /// The signature / file type must be 0x4d42 (the ASCII string "BM")
    pub signature: [u8; 2],

    /// Total size of the BMP file in bytes.
    pub file_size: u32,

    /// Reserved field.
    ///
    /// In modern Windows BMP files, this field is unused and must be zero.
    ///
    /// This implementation preserves the raw value for completeness but does
    /// not attempt to interpret it.
    pub reserved_1: [u8; 2],

    /// Reserved field.
    ///
    /// In modern Windows BMP files, this field is unused and must be zero.
    ///
    /// This implementation preserves the raw value for completeness but does
    /// not attempt to interpret it.
    pub reserved_2: [u8; 2],

    /// Offset, in bytes from the beginning of the file, to the start of the
    /// pixel data.
    ///
    /// This offset accounts for the file header, DIB header, optional color
    /// masks, and optional color table.
    pub pixel_data_offset: u32,
}

impl FileHeader {
    pub(crate) const SIZE: u32 = 14;

    pub(crate) fn validate(&self) -> Result<(), ValidationError> {
        if self.signature != *b"BM" {
            return Err(ValidationError::InvalidFileSignature(self.signature));
        }

        if self.reserved_1 != [0u8; 2] || self.reserved_2 != [0u8; 2] {
            return Err(ValidationError::InvalidFileReservedData([
                self.reserved_1[0],
                self.reserved_1[1],
                self.reserved_2[0],
                self.reserved_2[1],
            ]));
        }

        Ok(())
    }

    pub(crate) fn read_unchecked<R: Read>(reader: &mut R) -> io::Result<Self> {
        let signature = read_array::<2, _>(reader)?;
        let file_size = reader.read_u32::<LittleEndian>()?;

        let reserved_1 = read_array::<2, _>(reader)?;
        let reserved_2 = read_array::<2, _>(reader)?;

        let pixel_data_offset = reader.read_u32::<LittleEndian>()?;

        Ok(Self {
            signature,
            file_size,
            reserved_1,
            reserved_2,
            pixel_data_offset,
        })
    }

    pub(crate) fn write_unchecked<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.signature)?;
        writer.write_u32::<LittleEndian>(self.file_size)?;
        writer.write_all(&self.reserved_1)?;
        writer.write_all(&self.reserved_2)?;
        writer.write_u32::<LittleEndian>(self.pixel_data_offset)?;
        Ok(())
    }
}
