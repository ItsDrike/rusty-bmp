use std::io::{Read, Seek};

use crate::raw::{
    BitmapCoreHeader, BitmapHeader, BitmapInfoHeader, BitmapV4Header, BitmapV5Header, BmpError, BmpResult, FileHeader,
    RgbMasks,
    helpers::BoundedReader,
    types::{ColorSpaceType, RgbQuad, RgbTriple},
    wingdi,
};

pub(crate) const MAX_COLOR_TABLE_ENTRIES: usize = 1 << 16;
pub(crate) const MAX_PIXEL_BYTES: usize = 512 * 1024 * 1024; // 512 MB

enum ColorTable {
    Core(Vec<RgbTriple>),
    InfoOrLater(Vec<RgbQuad>),
}

impl ColorTable {
    pub(crate) fn read<R: Read>(reader: &mut R, header: &BitmapHeader) -> BmpResult<Self> {
        let entry_count = header.color_table_size()?;
        let entry_count = usize::try_from(entry_count).map_err(|_| BmpError::PaletteTooLarge(entry_count))?;

        if entry_count > MAX_COLOR_TABLE_ENTRIES {
            return Err(BmpError::PaletteTooLarge(entry_count as u32));
        }

        match header {
            BitmapHeader::Core(_) => {
                let mut color_table: Vec<RgbTriple> = Vec::with_capacity(entry_count);

                for _ in 0..entry_count {
                    color_table.push(RgbTriple::read(reader)?);
                }

                Ok(Self::Core(color_table))
            }
            _ => {
                let mut color_table: Vec<RgbQuad> = Vec::with_capacity(entry_count);

                for _ in 0..entry_count {
                    color_table.push(RgbQuad::read(reader)?);
                }

                Ok(Self::InfoOrLater(color_table))
            }
        }
    }
}

pub struct BitmapCoreData {
    pub file_header: FileHeader,

    pub bmp_header: BitmapCoreHeader,

    pub color_table: Vec<RgbTriple>,

    pub bitmap_array: Vec<u8>,
}

pub struct BitmapInfoData {
    pub file_header: FileHeader,

    pub bmp_header: BitmapInfoHeader,

    // TODO: We might want to make this just RgbMasks instead, depending on whether or not we want
    // to support BI_ALPHABITFIELDS
    // TODO: This being an option is perhaps somewhat odd, this structure could also be split up
    // into variant enums for whether or not BI_BITFIELDS is used, and hold this only then. But
    // then again, that might just be overengineering for these raw structs.
    pub color_masks: Option<RgbMasks>,

    pub color_table: Vec<RgbQuad>,

    pub bitmap_array: Vec<u8>,
}
pub struct BitmapV4Data {
    pub file_header: FileHeader,

    pub bmp_header: BitmapV4Header,

    pub color_table: Vec<RgbQuad>,

    pub bitmap_array: Vec<u8>,
}

pub struct BitmapV5Data {
    pub file_header: FileHeader,

    pub bmp_header: BitmapV5Header,

    pub color_table: Vec<RgbQuad>,

    pub bitmap_array: Vec<u8>,

    // TODO: Similarly to color_masks in INFO, this can only be present if cs_type is
    // PROFILE_EMBEDDED or PROFILE_LINKED, and will always be present in those cases,
    // yet it will never be present otherwise. It might make sense to split this up
    // into an enum and hold this only conditionally with two distinct structs, insted
    // of using an Option regardless. Then again, it might be overengineering for just
    // the raw structs.
    pub icc_profile: Option<Vec<u8>>,
}

pub enum Bmp {
    Core(BitmapCoreData),
    Info(BitmapInfoData),
    V4(BitmapV4Data),
    V5(BitmapV5Data),
}

impl Bmp {
    pub fn read<R: Read + Seek>(reader: &mut R) -> BmpResult<Self> {
        let file_header = FileHeader::read(reader)?;

        // Use a custom bounded reader that's limited to the specified file size.
        // The construction of this reader will fail if the specified file_size
        // from the file header is actually outside of the reader's seekable bounds.
        // The start/end seek positions will be bounded to the BMP (e.g. start=0 is
        // the start of the file header). This bounded reader also prevents us from
        // accidentally seeking somewhere outside of the file, e.g. if the BMP encodes
        // invalid offsets.
        reader.seek_relative(-(FileHeader::SIZE as i64))?;
        let mut reader = BoundedReader::new(reader, file_header.file_size as u64)?;
        reader.seek_relative(FileHeader::SIZE as i64)?;

        let bmp_header = BitmapHeader::read_unchecked(&mut reader)?;
        bmp_header.validate()?;

        // The V3 / INFO header supports having embedded color masks.
        // No other variant has support for this, as V4+ embeds the masks into
        // the DIB header directly, and V2 / CORE doesn't have bitfields support
        // at all.
        let masks = if let BitmapHeader::Info(header) = bmp_header
            && header.compression == wingdi::BI_BITFIELDS
        {
            let masks = RgbMasks::read_unchecked(&mut reader)?;
            masks.validate_for_bpp(header.bit_count)?;
            Some(masks)
        } else {
            None
        };

        let color_table = ColorTable::read(&mut reader, &bmp_header)?;

        let pixel_data_pos = file_header.pixel_data_offset as u64;

        // Check if there are some further data embedded in the BMP before the pixel
        // data. If yes, it could be the ICC color profiles (though these usually come
        // after the bitmap array, the spec does allow them to be here too),
        // alternatively, it could also be some custom metadata that a specific
        // application chose to embed into the BMP without violating the standard.
        //
        // We might want to collect the information about what's in this gap, even if
        // we can't interpret it as it's non-standard. Though, we should only do so if
        // this actually isn't the ICC profile, as that would then just duplicate data.
        // Though differentiating that + handling this cleanly might become messy.
        //
        // let gap_pos = reader.stream_position()?;
        // let metadata_size = pixel_data_pos - gap_pos;

        reader.seek(std::io::SeekFrom::Start(pixel_data_pos))?;

        let pixel_data_size = bmp_header.pixel_data_size()?;
        let pixel_data_size = usize::try_from(pixel_data_size).map_err(|_| BmpError::PixelDataTooLarge)?;

        if pixel_data_size > MAX_PIXEL_BYTES {
            return Err(BmpError::PixelDataTooLarge);
        }

        let mut pixel_data = vec![0u8; pixel_data_size];
        reader.read_exact(&mut pixel_data)?;

        Ok(match bmp_header {
            BitmapHeader::Core(header) => {
                debug_assert_eq!(masks, None); // core always uses BI_RGB

                if let ColorTable::Core(color_table_vec) = color_table {
                    Self::Core(BitmapCoreData {
                        file_header,
                        bmp_header: header,
                        color_table: color_table_vec,
                        bitmap_array: pixel_data,
                    })
                } else {
                    unreachable!()
                }
            }
            BitmapHeader::Info(header) => {
                if let ColorTable::InfoOrLater(color_table_vec) = color_table {
                    Self::Info(BitmapInfoData {
                        file_header,
                        bmp_header: header,
                        color_masks: masks,
                        color_table: color_table_vec,
                        bitmap_array: pixel_data,
                    })
                } else {
                    unreachable!()
                }
            }
            BitmapHeader::V4(header) => {
                debug_assert_eq!(masks, None); // embedded into header directly

                if let ColorTable::InfoOrLater(color_table_vec) = color_table {
                    Self::V4(BitmapV4Data {
                        file_header,
                        bmp_header: header,
                        color_table: color_table_vec,
                        bitmap_array: pixel_data,
                    })
                } else {
                    unreachable!()
                }
            }
            BitmapHeader::V5(header) => {
                debug_assert_eq!(masks, None); // embedded into header directly

                let icc_profile = if matches!(
                    header.v4.cs_type,
                    ColorSpaceType::ProfileEmbedded | ColorSpaceType::ProfileLinked
                ) {
                    let offset = header.profile_data as u64 + FileHeader::SIZE as u64;
                    let size = usize::try_from(header.profile_size).map_err(|_| BmpError::IccProfileTooLarge)?;

                    // TODO: Maybe also validate that the offset isn't within the color table / dib header
                    // though this isn't that important.

                    reader.seek(std::io::SeekFrom::Start(offset))?;

                    let mut data = vec![0u8; size];
                    reader.read_exact(&mut data)?;
                    Some(data)
                } else {
                    None
                };

                if let ColorTable::InfoOrLater(color_table_vec) = color_table {
                    Self::V5(BitmapV5Data {
                        file_header,
                        bmp_header: header,
                        color_table: color_table_vec,
                        bitmap_array: pixel_data,
                        icc_profile,
                    })
                } else {
                    unreachable!()
                }
            }
        })
    }
}

// TODO: Implement write
