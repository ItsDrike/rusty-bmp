use std::io::{self, Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::raw::{
    BitmapCoreHeader,
    bitmap_headers::{BitmapInfoHeader, BitmapV4Header, BitmapV5Header},
    error::{IoStage, StructuralError, ValidationError},
    types::{BitsPerPixel, Compression},
};

pub enum BitmapHeader {
    Core(BitmapCoreHeader),
    Info(BitmapInfoHeader),
    V4(BitmapV4Header),
    V5(BitmapV5Header),
}

impl BitmapHeader {
    pub(crate) fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Core(header) => header.validate(),
            Self::Info(header) => header.validate(),
            Self::V4(header) => header.validate(),
            Self::V5(header) => header.validate(),
        }
    }

    pub(crate) fn read_unchecked<R: Read>(reader: &mut R) -> Result<Self, StructuralError> {
        let size = reader
            .read_u32::<LittleEndian>()
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingDibHeader))?;

        let header = match size {
            BitmapCoreHeader::HEADER_SIZE => Self::Core(
                BitmapCoreHeader::read_unchecked(reader)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingDibHeader))?,
            ),
            BitmapInfoHeader::HEADER_SIZE => Self::Info(
                BitmapInfoHeader::read_unchecked(reader)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingDibHeader))?,
            ),
            BitmapV4Header::HEADER_SIZE => Self::V4(
                BitmapV4Header::read_unchecked(reader)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingDibHeader))?,
            ),
            BitmapV5Header::HEADER_SIZE => Self::V5(
                BitmapV5Header::read_unchecked(reader)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingDibHeader))?,
            ),
            _ => {
                return Err(StructuralError::UnsupportedStructure(format!(
                    "The BMP header size value of {size} did not match any supported BMP variant"
                )))
            }
        };

        Ok(header)
    }

    pub(crate) fn write_unchecked<W: Write>(&self, writer: &mut W) -> io::Result<()> {
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

    #[inline]
    pub(crate) fn bit_count(&self) -> BitsPerPixel {
        match self {
            Self::Core(h) => h.bit_count,
            Self::Info(h) => h.bit_count,
            Self::V4(h) => h.info.bit_count,
            Self::V5(h) => h.v4.info.bit_count,
        }
    }

    #[inline]
    pub(crate) fn compression(&self) -> Compression {
        match self {
            Self::Core(_) => Compression::Rgb,
            Self::Info(h) => h.compression,
            Self::V4(h) => h.info.compression,
            Self::V5(h) => h.v4.info.compression,
        }
    }

    #[inline]
    pub(crate) fn color_table_size(&self) -> Result<u32, StructuralError> {
        match self {
            Self::Core(h) => h.color_table_size(),
            Self::Info(h) => h.color_table_size(),
            Self::V4(h) => h.info.color_table_size(),
            Self::V5(h) => h.v4.info.color_table_size(),
        }
    }

    #[inline]
    pub(crate) fn pixel_data_size(&self) -> Result<u32, StructuralError> {
        match self {
            Self::Core(h) => h.pixel_data_size(),
            Self::Info(h) => h.pixel_data_size(),
            Self::V4(h) => h.info.pixel_data_size(),
            Self::V5(h) => h.v4.info.pixel_data_size(),
        }
    }
}
