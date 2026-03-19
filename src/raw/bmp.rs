use std::io::{Read, Seek, SeekFrom, Write};

use crate::raw::{
    BitmapCoreHeader, BitmapHeader, BitmapInfoHeader, BitmapV4Header, BitmapV5Header, FileHeader, RgbMasks,
    error::{BmpError, IccProfileError, IoStage, PixelDataLayoutError, StructuralError, ValidationError},
    helpers::BoundedStream,
    types::{ColorSpaceType, Compression, RgbQuad, RgbTriple},
};

const MAX_COLOR_TABLE_ENTRIES: usize = 1 << 16;
const MAX_PIXEL_BYTES: usize = 512 * 1024 * 1024; // 512 MB
const MAX_ICC_PROFILE_BYTES: usize = 16 * 1024 * 1024; // 16 MB

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DibVariant {
    Core,
    Info,
    V4,
    V5,
}

enum ColorTable {
    Core(Vec<RgbTriple>),
    InfoOrLater(Vec<RgbQuad>),
}

impl ColorTable {
    pub(crate) fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Core(_) => {}
            Self::InfoOrLater(color_table) => {
                for rgb_quad in color_table {
                    rgb_quad.validate()?;
                }
            }
        }

        Ok(())
    }

    pub(crate) fn read_unchecked<R: Read>(reader: &mut R, header: &BitmapHeader) -> Result<Self, StructuralError> {
        let entry_count = header.color_table_size()? as usize;
        if entry_count > MAX_COLOR_TABLE_ENTRIES {
            return Err(StructuralError::StructureUnsafe(format!(
                "Color table contains {entry_count} entries, which is higher than the allowed safe maximum: {MAX_COLOR_TABLE_ENTRIES}"
            )));
        }

        if let BitmapHeader::Core(_) = header {
            let mut color_table: Vec<RgbTriple> = Vec::with_capacity(entry_count);

            for _ in 0..entry_count {
                color_table.push(
                    RgbTriple::read(reader).map_err(|e| StructuralError::from_io(e, IoStage::ReadingColorTable))?,
                );
            }

            Ok(Self::Core(color_table))
        } else {
            let mut color_table: Vec<RgbQuad> = Vec::with_capacity(entry_count);

            for _ in 0..entry_count {
                let rgb_quad = RgbQuad::read_unchecked(reader)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingColorTable))?;
                color_table.push(rgb_quad);
            }

            Ok(Self::InfoOrLater(color_table))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BitmapCoreData {
    pub file_header: FileHeader,

    pub bmp_header: BitmapCoreHeader,

    pub color_table: Vec<RgbTriple>,

    pub bitmap_array: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BitmapInfoData {
    pub file_header: FileHeader,

    pub bmp_header: BitmapInfoHeader,

    // TODO: We might want to make this just ColorMasks (enum) instead, depending on whether or not
    // we want to support BI_ALPHABITFIELDS
    pub color_masks: Option<RgbMasks>,

    pub color_table: Vec<RgbQuad>,

    pub bitmap_array: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BitmapV4Data {
    pub file_header: FileHeader,

    pub bmp_header: BitmapV4Header,

    pub color_table: Vec<RgbQuad>,

    pub bitmap_array: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BitmapV5Data {
    pub file_header: FileHeader,

    pub bmp_header: BitmapV5Header,

    pub color_table: Vec<RgbQuad>,

    pub bitmap_array: Vec<u8>,

    pub icc_profile: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Bmp {
    Core(BitmapCoreData),
    Info(BitmapInfoData),
    V4(BitmapV4Data),
    V5(BitmapV5Data),
}

impl Bmp {
    fn header_color_table_size(header: &BitmapHeader) -> Result<usize, StructuralError> {
        Ok(header.color_table_size()? as usize)
    }

    fn header_pixel_data_size(header: &BitmapHeader) -> Result<usize, StructuralError> {
        Ok(header.pixel_data_size()? as usize)
    }

    const fn validate_color_table_size(stored_size: usize, header_size: usize) -> Result<(), ValidationError> {
        if stored_size != header_size {
            return Err(ValidationError::ColorTableSizeMismatch {
                stored_size,
                header_size,
            });
        }
        Ok(())
    }

    fn validate_pixel_data_size(stored_size: usize, header_size: usize) -> Result<(), BmpError> {
        if stored_size != header_size {
            return Err(ValidationError::PixelDataSizeMismatch {
                stored_size,
                header_size,
            }
            .into());
        }
        if header_size > MAX_PIXEL_BYTES {
            return Err(StructuralError::StructureUnsafe(format!(
                "Pixel data contains {header_size} entries, which is higher than the allowed safe maximum: {MAX_PIXEL_BYTES}"
            ))
            .into());
        }
        Ok(())
    }

    fn validate_rgba_quad_table(color_table: &[RgbQuad]) -> Result<(), ValidationError> {
        for rgb_quad in color_table {
            rgb_quad.validate()?;
        }
        Ok(())
    }

    fn min_pixel_offset(
        dib_header_size: u32,
        color_table_entries: usize,
        color_entry_size: u32,
        extra_size: u32,
    ) -> Result<u32, StructuralError> {
        FileHeader::SIZE
            .checked_add(dib_header_size)
            .and_then(|x| x.checked_add(extra_size))
            .and_then(|x| {
                let color_table_entries_u32 = u32::try_from(color_table_entries).ok()?;
                x.checked_add(color_table_entries_u32.checked_mul(color_entry_size)?)
            })
            .ok_or_else(|| StructuralError::ArithmeticOverflow("Min pixel offset calculation".to_owned()))
    }

    fn pixel_end_with_overlap_check(
        file_header: &FileHeader,
        min_pixel_offset: u32,
        pixel_data_size: usize,
    ) -> Result<u64, BmpError> {
        if file_header.pixel_data_offset < min_pixel_offset {
            return Err(
                ValidationError::PixelDataLayout(PixelDataLayoutError::OverlapsMetadata {
                    pixel_offset_header: file_header.pixel_data_offset,
                    min_offset: min_pixel_offset,
                })
                .into(),
            );
        }
        u64::from(file_header.pixel_data_offset)
            .checked_add(pixel_data_size as u64)
            .ok_or_else(|| StructuralError::ArithmeticOverflow("Pixel data end calculation".to_owned()).into())
    }

    fn validate_file_end(file_size: u32, required_end: u64) -> Result<(), ValidationError> {
        if required_end > u64::from(file_size) {
            return Err(ValidationError::PixelDataLayout(
                PixelDataLayoutError::ExceedsFileSize {
                    pixel_end: required_end,
                    file_size,
                },
            ));
        }
        if required_end != u64::from(file_size) {
            return Err(ValidationError::PixelDataLayout(
                PixelDataLayoutError::DoesNotEndAtFileEnd {
                    pixel_end: required_end,
                    file_size,
                },
            ));
        }
        Ok(())
    }

    fn validate_info_masks(header: &BitmapHeader, masks: Option<&RgbMasks>) -> Result<(), BmpError> {
        match (header.compression(), masks) {
            (Compression::BitFields, Some(masks)) => {
                masks
                    .validate_for_bpp(header.bit_count())
                    .map_err(ValidationError::from)?;
            }
            (Compression::BitFields, None) => {
                return Err(ValidationError::InvalidCompressionForBpp {
                    compression: Compression::BitFields,
                    bpp: header.bit_count(),
                }
                .into());
            }
            (_, Some(_)) => {
                return Err(ValidationError::InvalidCompressionForBpp {
                    compression: header.compression(),
                    bpp: header.bit_count(),
                }
                .into());
            }
            (_, None) => {}
        }
        Ok(())
    }

    /// Validates BMP structural and semantic consistency for the current variant.
    ///
    /// # Errors
    /// Returns [`BmpError`] when any header, mask, table, pixel layout, or ICC profile
    /// invariant is violated.
    pub fn validate(&self) -> Result<(), BmpError> {
        match self {
            Self::Core(data) => {
                data.file_header.validate()?;
                let header = BitmapHeader::Core(data.bmp_header);
                header.validate()?;

                let color_table_size_header = Self::header_color_table_size(&header)?;
                Self::validate_color_table_size(data.color_table.len(), color_table_size_header)?;

                let pixel_data_size_header = Self::header_pixel_data_size(&header)?;
                Self::validate_pixel_data_size(data.bitmap_array.len(), pixel_data_size_header)?;

                let min_pixel_offset =
                    Self::min_pixel_offset(BitmapCoreHeader::HEADER_SIZE, data.color_table.len(), 3, 0)?;
                let pixel_end =
                    Self::pixel_end_with_overlap_check(&data.file_header, min_pixel_offset, pixel_data_size_header)?;
                Self::validate_file_end(data.file_header.file_size, pixel_end)?;
            }
            Self::Info(data) => {
                data.file_header.validate()?;
                let header = BitmapHeader::Info(data.bmp_header);
                header.validate()?;
                Self::validate_info_masks(&header, data.color_masks.as_ref())?;

                let color_table_size_header = Self::header_color_table_size(&header)?;
                Self::validate_color_table_size(data.color_table.len(), color_table_size_header)?;
                Self::validate_rgba_quad_table(&data.color_table)?;

                let pixel_data_size_header = Self::header_pixel_data_size(&header)?;
                Self::validate_pixel_data_size(data.bitmap_array.len(), pixel_data_size_header)?;

                let masks_size = if data.color_masks.is_some() { 12 } else { 0 };
                let min_pixel_offset =
                    Self::min_pixel_offset(BitmapInfoHeader::HEADER_SIZE, data.color_table.len(), 4, masks_size)?;
                let pixel_end =
                    Self::pixel_end_with_overlap_check(&data.file_header, min_pixel_offset, pixel_data_size_header)?;
                Self::validate_file_end(data.file_header.file_size, pixel_end)?;
            }
            Self::V4(data) => {
                data.file_header.validate()?;
                let header = BitmapHeader::V4(data.bmp_header);
                header.validate()?;

                let color_table_size_header = Self::header_color_table_size(&header)?;
                Self::validate_color_table_size(data.color_table.len(), color_table_size_header)?;
                Self::validate_rgba_quad_table(&data.color_table)?;

                let pixel_data_size_header = Self::header_pixel_data_size(&header)?;
                Self::validate_pixel_data_size(data.bitmap_array.len(), pixel_data_size_header)?;

                let min_pixel_offset =
                    Self::min_pixel_offset(BitmapV4Header::HEADER_SIZE, data.color_table.len(), 4, 0)?;
                let pixel_end =
                    Self::pixel_end_with_overlap_check(&data.file_header, min_pixel_offset, pixel_data_size_header)?;
                Self::validate_file_end(data.file_header.file_size, pixel_end)?;
            }
            Self::V5(data) => {
                data.file_header.validate()?;
                let header = BitmapHeader::V5(data.bmp_header);
                header.validate()?;

                let color_table_size_header = Self::header_color_table_size(&header)?;
                Self::validate_color_table_size(data.color_table.len(), color_table_size_header)?;
                Self::validate_rgba_quad_table(&data.color_table)?;

                let pixel_data_size_header = Self::header_pixel_data_size(&header)?;
                Self::validate_pixel_data_size(data.bitmap_array.len(), pixel_data_size_header)?;

                let min_pixel_offset =
                    Self::min_pixel_offset(BitmapV5Header::HEADER_SIZE, data.color_table.len(), 4, 0)?;
                let pixel_end =
                    Self::pixel_end_with_overlap_check(&data.file_header, min_pixel_offset, pixel_data_size_header)?;

                let icc_end = match data.bmp_header.v4.cs_type {
                    ColorSpaceType::ProfileEmbedded | ColorSpaceType::ProfileLinked => {
                        let profile = data.icc_profile.as_ref().ok_or(ValidationError::IccProfile(
                            IccProfileError::MissingDataForProfileColorSpace {
                                cs_type: data.bmp_header.v4.cs_type,
                            },
                        ))?;

                        if profile.len() > MAX_ICC_PROFILE_BYTES {
                            Err(StructuralError::StructureUnsafe(format!(
                                "ICC profile contains {0} bytes, which is higher than the allowed safe maximum: {MAX_ICC_PROFILE_BYTES}",
                                profile.len()
                            )))?;
                        }

                        let profile_size_header = data.bmp_header.profile_size as usize;
                        if profile.len() != profile_size_header {
                            Err(ValidationError::IccProfile(IccProfileError::SizeMismatch {
                                stored_size: profile.len(),
                                header_size: profile_size_header,
                            }))?;
                        }

                        let profile_offset_absolute = u64::from(FileHeader::SIZE)
                            .checked_add(u64::from(data.bmp_header.profile_data))
                            .ok_or_else(|| {
                                StructuralError::ArithmeticOverflow(
                                    "ICC profile absolute offset calculation".to_owned(),
                                )
                            })?;

                        let min_profile_offset = u64::from(FileHeader::SIZE)
                            .checked_add(u64::from(BitmapV5Header::HEADER_SIZE))
                            .and_then(|x| x.checked_add((data.color_table.len() as u64).checked_mul(4)?))
                            .ok_or_else(|| {
                                StructuralError::ArithmeticOverflow("ICC profile min offset calculation".to_owned())
                            })?;

                        if profile_offset_absolute < min_profile_offset {
                            Err(ValidationError::IccProfile(IccProfileError::OverlapsMetadata {
                                profile_offset: profile_offset_absolute,
                                min_offset: min_profile_offset,
                            }))?;
                        }

                        let profile_end =
                            profile_offset_absolute
                                .checked_add(profile.len() as u64)
                                .ok_or_else(|| {
                                    StructuralError::ArithmeticOverflow("ICC profile end calculation".to_owned())
                                })?;
                        if profile_end > u64::from(data.file_header.file_size) {
                            Err(ValidationError::IccProfile(IccProfileError::ExceedsFileSize {
                                profile_end,
                                file_size: data.file_header.file_size,
                            }))?;
                        }

                        profile_end
                    }
                    _ => {
                        if data.icc_profile.is_some()
                            || data.bmp_header.profile_data != 0
                            || data.bmp_header.profile_size != 0
                        {
                            Err(ValidationError::IccProfile(
                                IccProfileError::UnexpectedDataForNonProfileColorSpace {
                                    cs_type: data.bmp_header.v4.cs_type,
                                    profile_data: data.bmp_header.profile_data,
                                    profile_size: data.bmp_header.profile_size,
                                },
                            ))?;
                        }
                        0
                    }
                };

                let required_file_end = pixel_end.max(icc_end);
                Self::validate_file_end(data.file_header.file_size, required_file_end)?;
            }
        }

        Ok(())
    }

    /// Reads a BMP payload and performs validation before returning the parsed structure.
    ///
    /// # Errors
    /// Returns [`BmpError`] for malformed structures, invalid field combinations, out-of-bounds
    /// offsets/sizes, or I/O failures while reading.
    pub fn read_checked<R: Read + Seek>(reader: &mut R) -> Result<Self, BmpError> {
        let file_header =
            FileHeader::read_unchecked(reader).map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;
        file_header.validate()?;

        // Use a custom bounded reader that's limited to the specified file size.
        // The construction of this reader will fail if the specified file_size
        // from the file header is actually outside of the reader's seekable bounds.
        // The start/end seek positions will be bounded to the BMP (e.g. start=0 is
        // the start of the file header). This bounded reader also prevents us from
        // accidentally seeking somewhere outside of the file, e.g. if the BMP encodes
        // invalid offsets.
        reader
            .seek_relative(-i64::from(FileHeader::SIZE))
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;
        let mut reader = BoundedStream::new(reader)
            .shrink_start(SeekFrom::Current(0))
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?
            .cap_to_stream_end()
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?
            .shrink_end(SeekFrom::Current(i64::from(file_header.file_size)))
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;
        reader
            .seek_relative(i64::from(FileHeader::SIZE))
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;

        let bmp_header = BitmapHeader::read_unchecked(&mut reader)?;
        bmp_header.validate()?;

        // The V3 / INFO header supports having embedded color masks.
        // No other variant has support for this, as V4+ embeds the masks into
        // the DIB header directly, and V2 / CORE doesn't have bitfields support
        // at all.
        let masks = if let BitmapHeader::Info(header) = bmp_header
            && header.compression == Compression::BitFields
        {
            let masks = RgbMasks::read_unchecked(&mut reader)
                .map_err(|e| StructuralError::from_io(e, IoStage::ReadingColorMasks))?;
            masks
                .validate_for_bpp(header.bit_count)
                .map_err(ValidationError::from)?;
            Some(masks)
        } else {
            None
        };

        let color_table = ColorTable::read_unchecked(&mut reader, &bmp_header)?;
        color_table.validate()?;

        let pixel_data_pos = u64::from(file_header.pixel_data_offset);

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

        let min_pixel_offset = u64::from(FileHeader::SIZE);
        if pixel_data_pos < min_pixel_offset {
            return Err(
                ValidationError::PixelDataLayout(PixelDataLayoutError::OverlapsMetadata {
                    pixel_offset_header: file_header.pixel_data_offset,
                    min_offset: FileHeader::SIZE,
                })
                .into(),
            );
        }
        if pixel_data_pos > u64::from(file_header.file_size) {
            return Err(ValidationError::PixelDataLayout(PixelDataLayoutError::ExceedsFileSize {
                pixel_end: pixel_data_pos,
                file_size: file_header.file_size,
            })
            .into());
        }

        reader
            .seek(SeekFrom::Start(pixel_data_pos))
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingPixelData))?;

        let pixel_data_size = bmp_header.pixel_data_size()? as usize;
        if pixel_data_size > MAX_PIXEL_BYTES {
            return Err(StructuralError::StructureUnsafe(format!(
                "Pixel data contains {pixel_data_size} entries, which is higher than the allowed safe maximum: {MAX_PIXEL_BYTES}"
            ))
            .into());
        }

        let mut pixel_data = vec![0u8; pixel_data_size];
        reader
            .read_exact(&mut pixel_data)
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingPixelData))?;

        let bmp = match bmp_header {
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
                    let offset = u64::from(header.profile_data)
                        .checked_add(u64::from(FileHeader::SIZE))
                        .ok_or_else(|| {
                            StructuralError::ArithmeticOverflow("ICC profile absolute offset calculation".to_owned())
                        })?;

                    let size = header.profile_size as usize;
                    if size > MAX_ICC_PROFILE_BYTES {
                        return Err(StructuralError::StructureUnsafe(format!(
                            "ICC profile contains {size} bytes, which is higher than the allowed safe maximum: {MAX_ICC_PROFILE_BYTES}"
                        ))
                        .into());
                    }

                    // TODO: Maybe also validate that the offset isn't within the color table / color
                    // masks / dib header, though this isn't that important, as we do validate that
                    // it is within the file offset, so this is purely about preventing it from
                    // reading wrong data, though I'm not even certain that the standard forbids
                    // this. In theory, if the color table bytes do resolve to a valid ICC profile
                    // too, there's not real reason to prevent that, even if it's really dumb and
                    // unlikely. Safety-wise, this isn't important.
                    let profile_end = offset.checked_add(size as u64).ok_or_else(|| {
                        StructuralError::ArithmeticOverflow("ICC profile end calculation".to_owned())
                    })?;
                    if profile_end > u64::from(file_header.file_size) {
                        return Err(ValidationError::IccProfile(IccProfileError::ExceedsFileSize {
                            profile_end,
                            file_size: file_header.file_size,
                        })
                        .into());
                    }

                    reader
                        .seek(SeekFrom::Start(offset))
                        .map_err(|e| StructuralError::from_io(e, IoStage::ReadingIccProfile))?;

                    let mut data = vec![0u8; size];
                    reader
                        .read_exact(&mut data)
                        .map_err(|e| StructuralError::from_io(e, IoStage::ReadingIccProfile))?;

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
        };

        // Leave the reader at the end of the BMP file
        reader
            .seek(SeekFrom::End(0))
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;

        Ok(bmp)
    }

    fn validate_write_layout(&self) -> Result<(), BmpError> {
        match self {
            Self::Core(data) => {
                let header = BitmapHeader::Core(data.bmp_header);
                let pixel_data_size_header = Self::header_pixel_data_size(&header)?;
                Self::validate_pixel_data_size(data.bitmap_array.len(), pixel_data_size_header)?;

                if data.file_header.pixel_data_offset < FileHeader::SIZE {
                    return Err(
                        ValidationError::PixelDataLayout(PixelDataLayoutError::OverlapsMetadata {
                            pixel_offset_header: data.file_header.pixel_data_offset,
                            min_offset: FileHeader::SIZE,
                        })
                        .into(),
                    );
                }
                let pixel_end = u64::from(data.file_header.pixel_data_offset)
                    .checked_add(pixel_data_size_header as u64)
                    .ok_or_else(|| StructuralError::ArithmeticOverflow("Pixel data end calculation".to_owned()))?;
                if pixel_end > u64::from(data.file_header.file_size) {
                    return Err(ValidationError::PixelDataLayout(PixelDataLayoutError::ExceedsFileSize {
                        pixel_end,
                        file_size: data.file_header.file_size,
                    })
                    .into());
                }
            }
            Self::Info(data) => {
                let header = BitmapHeader::Info(data.bmp_header);
                let pixel_data_size_header = Self::header_pixel_data_size(&header)?;
                Self::validate_pixel_data_size(data.bitmap_array.len(), pixel_data_size_header)?;

                if data.file_header.pixel_data_offset < FileHeader::SIZE {
                    return Err(
                        ValidationError::PixelDataLayout(PixelDataLayoutError::OverlapsMetadata {
                            pixel_offset_header: data.file_header.pixel_data_offset,
                            min_offset: FileHeader::SIZE,
                        })
                        .into(),
                    );
                }
                let pixel_end = u64::from(data.file_header.pixel_data_offset)
                    .checked_add(pixel_data_size_header as u64)
                    .ok_or_else(|| StructuralError::ArithmeticOverflow("Pixel data end calculation".to_owned()))?;
                if pixel_end > u64::from(data.file_header.file_size) {
                    return Err(ValidationError::PixelDataLayout(PixelDataLayoutError::ExceedsFileSize {
                        pixel_end,
                        file_size: data.file_header.file_size,
                    })
                    .into());
                }
            }
            Self::V4(data) => {
                let header = BitmapHeader::V4(data.bmp_header);
                let pixel_data_size_header = Self::header_pixel_data_size(&header)?;
                Self::validate_pixel_data_size(data.bitmap_array.len(), pixel_data_size_header)?;

                if data.file_header.pixel_data_offset < FileHeader::SIZE {
                    return Err(
                        ValidationError::PixelDataLayout(PixelDataLayoutError::OverlapsMetadata {
                            pixel_offset_header: data.file_header.pixel_data_offset,
                            min_offset: FileHeader::SIZE,
                        })
                        .into(),
                    );
                }
                let pixel_end = u64::from(data.file_header.pixel_data_offset)
                    .checked_add(pixel_data_size_header as u64)
                    .ok_or_else(|| StructuralError::ArithmeticOverflow("Pixel data end calculation".to_owned()))?;
                if pixel_end > u64::from(data.file_header.file_size) {
                    return Err(ValidationError::PixelDataLayout(PixelDataLayoutError::ExceedsFileSize {
                        pixel_end,
                        file_size: data.file_header.file_size,
                    })
                    .into());
                }
            }
            Self::V5(data) => {
                let header = BitmapHeader::V5(data.bmp_header);
                let pixel_data_size_header = Self::header_pixel_data_size(&header)?;
                Self::validate_pixel_data_size(data.bitmap_array.len(), pixel_data_size_header)?;

                if data.file_header.pixel_data_offset < FileHeader::SIZE {
                    return Err(
                        ValidationError::PixelDataLayout(PixelDataLayoutError::OverlapsMetadata {
                            pixel_offset_header: data.file_header.pixel_data_offset,
                            min_offset: FileHeader::SIZE,
                        })
                        .into(),
                    );
                }
                let pixel_end = u64::from(data.file_header.pixel_data_offset)
                    .checked_add(pixel_data_size_header as u64)
                    .ok_or_else(|| StructuralError::ArithmeticOverflow("Pixel data end calculation".to_owned()))?;
                if pixel_end > u64::from(data.file_header.file_size) {
                    return Err(ValidationError::PixelDataLayout(PixelDataLayoutError::ExceedsFileSize {
                        pixel_end,
                        file_size: data.file_header.file_size,
                    })
                    .into());
                }

                let icc_end = if let Some(profile) = &data.icc_profile {
                    if profile.len() > MAX_ICC_PROFILE_BYTES {
                        return Err(StructuralError::StructureUnsafe(format!(
                            "ICC profile contains {} bytes, which is higher than the allowed safe maximum: {MAX_ICC_PROFILE_BYTES}",
                            profile.len()
                        ))
                        .into());
                    }

                    let profile_offset_absolute = u64::from(FileHeader::SIZE)
                        .checked_add(u64::from(data.bmp_header.profile_data))
                        .ok_or_else(|| {
                            StructuralError::ArithmeticOverflow("ICC profile absolute offset calculation".to_owned())
                        })?;

                    let profile_end = profile_offset_absolute
                        .checked_add(profile.len() as u64)
                        .ok_or_else(|| {
                            StructuralError::ArithmeticOverflow("ICC profile end calculation".to_owned())
                        })?;
                    if profile_end > u64::from(data.file_header.file_size) {
                        return Err(ValidationError::IccProfile(IccProfileError::ExceedsFileSize {
                            profile_end,
                            file_size: data.file_header.file_size,
                        })
                        .into());
                    }

                    profile_end
                } else {
                    0
                };

                let required_file_end = pixel_end.max(icc_end);
                if required_file_end > u64::from(data.file_header.file_size) {
                    return Err(ValidationError::PixelDataLayout(PixelDataLayoutError::ExceedsFileSize {
                        pixel_end: required_file_end,
                        file_size: data.file_header.file_size,
                    })
                    .into());
                }
            }
        }

        Ok(())
    }

    /// Writes the BMP structure as-is without re-deriving field values.
    ///
    /// # Errors
    /// Returns [`BmpError`] if layout validation fails or any write/seek operation fails.
    pub fn write_unchecked<W: Write + Seek>(&self, writer: &mut W) -> Result<(), BmpError> {
        self.validate_write_layout()?;

        let mut writer = BoundedStream::new(writer)
            .shrink_start(SeekFrom::Current(0))
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;

        match self {
            Self::Core(data) => {
                data.file_header
                    .write_unchecked(&mut writer)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;
                BitmapHeader::Core(data.bmp_header)
                    .write_unchecked(&mut writer)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;

                for entry in &data.color_table {
                    entry
                        .write(&mut writer)
                        .map_err(|e| StructuralError::from_io(e, IoStage::ReadingColorTable))?;
                }

                writer
                    .seek(SeekFrom::Start(u64::from(data.file_header.pixel_data_offset)))
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingPixelData))?;
                writer
                    .write_all(&data.bitmap_array)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingPixelData))?;
            }
            Self::Info(data) => {
                data.file_header
                    .write_unchecked(&mut writer)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;
                BitmapHeader::Info(data.bmp_header)
                    .write_unchecked(&mut writer)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;

                if let Some(masks) = &data.color_masks {
                    masks
                        .write_unchecked(&mut writer)
                        .map_err(|e| StructuralError::from_io(e, IoStage::ReadingColorMasks))?;
                }

                for entry in &data.color_table {
                    entry
                        .write_unchecked(&mut writer)
                        .map_err(|e| StructuralError::from_io(e, IoStage::ReadingColorTable))?;
                }

                writer
                    .seek(SeekFrom::Start(u64::from(data.file_header.pixel_data_offset)))
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingPixelData))?;
                writer
                    .write_all(&data.bitmap_array)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingPixelData))?;
            }
            Self::V4(data) => {
                data.file_header
                    .write_unchecked(&mut writer)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;
                BitmapHeader::V4(data.bmp_header)
                    .write_unchecked(&mut writer)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;

                for entry in &data.color_table {
                    entry
                        .write_unchecked(&mut writer)
                        .map_err(|e| StructuralError::from_io(e, IoStage::ReadingColorTable))?;
                }

                writer
                    .seek(SeekFrom::Start(u64::from(data.file_header.pixel_data_offset)))
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingPixelData))?;
                writer
                    .write_all(&data.bitmap_array)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingPixelData))?;
            }
            Self::V5(data) => {
                data.file_header
                    .write_unchecked(&mut writer)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;
                BitmapHeader::V5(data.bmp_header)
                    .write_unchecked(&mut writer)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;

                for entry in &data.color_table {
                    entry
                        .write_unchecked(&mut writer)
                        .map_err(|e| StructuralError::from_io(e, IoStage::ReadingColorTable))?;
                }

                writer
                    .seek(SeekFrom::Start(u64::from(data.file_header.pixel_data_offset)))
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingPixelData))?;
                writer
                    .write_all(&data.bitmap_array)
                    .map_err(|e| StructuralError::from_io(e, IoStage::ReadingPixelData))?;

                if let Some(profile) = &data.icc_profile {
                    let profile_offset = data.bmp_header.profile_data + FileHeader::SIZE;
                    writer
                        .seek(SeekFrom::Start(u64::from(profile_offset)))
                        .map_err(|e| StructuralError::from_io(e, IoStage::ReadingIccProfile))?;
                    writer
                        .write_all(profile)
                        .map_err(|e| StructuralError::from_io(e, IoStage::ReadingIccProfile))?;
                }
            }
        }

        let file_end = match self {
            Self::Core(data) => u64::from(data.file_header.file_size),
            Self::Info(data) => u64::from(data.file_header.file_size),
            Self::V4(data) => u64::from(data.file_header.file_size),
            Self::V5(data) => u64::from(data.file_header.file_size),
        };

        // Leave the writer at the declared end of this BMP payload.
        writer
            .seek(SeekFrom::Start(file_end))
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw::{
        BitsPerPixel, CieXyz, CieXyzTriple, FixedPoint2Dot30, FixedPoint16Dot16, GammaTriple, RgbaMasks,
    };

    fn zero_endpoints() -> CieXyzTriple {
        let zero = CieXyz {
            x: FixedPoint2Dot30::from_raw(0),
            y: FixedPoint2Dot30::from_raw(0),
            z: FixedPoint2Dot30::from_raw(0),
        };
        CieXyzTriple {
            red: zero,
            green: zero,
            blue: zero,
        }
    }

    fn zero_gamma() -> GammaTriple {
        GammaTriple {
            red: FixedPoint16Dot16::from_raw(0),
            green: FixedPoint16Dot16::from_raw(0),
            blue: FixedPoint16Dot16::from_raw(0),
        }
    }

    #[test]
    fn validate_accepts_canonical_info_pixel_offset() {
        let bmp = Bmp::Info(BitmapInfoData {
            file_header: FileHeader {
                signature: *b"BM",
                file_size: 58,
                reserved_1: [0; 2],
                reserved_2: [0; 2],
                pixel_data_offset: FileHeader::SIZE + BitmapInfoHeader::HEADER_SIZE,
            },
            bmp_header: BitmapInfoHeader {
                width: 1,
                height: 1,
                planes: 1,
                bit_count: BitsPerPixel::Bpp24,
                compression: Compression::Rgb,
                image_size: 4,
                x_resolution_ppm: 0,
                y_resolution_ppm: 0,
                colors_used: 0,
                colors_important: 0,
            },
            color_masks: None,
            color_table: Vec::new(),
            bitmap_array: vec![0, 0, 0, 0],
        });

        assert!(bmp.validate().is_ok());
    }

    #[test]
    fn validate_accepts_v5_profile_at_minimum_metadata_boundary() {
        // DIB starts at byte 14. With a V5 header (124 bytes), the first valid
        // profile byte is at absolute offset 138.
        let bmp = Bmp::V5(BitmapV5Data {
            file_header: FileHeader {
                signature: *b"BM",
                file_size: 146,
                reserved_1: [0; 2],
                reserved_2: [0; 2],
                pixel_data_offset: 142,
            },
            bmp_header: BitmapV5Header {
                v4: BitmapV4Header {
                    info: BitmapInfoHeader {
                        width: 1,
                        height: 1,
                        planes: 1,
                        bit_count: BitsPerPixel::Bpp32,
                        compression: Compression::Rgb,
                        image_size: 4,
                        x_resolution_ppm: 0,
                        y_resolution_ppm: 0,
                        colors_used: 0,
                        colors_important: 0,
                    },
                    masks: RgbaMasks {
                        red_mask: 0,
                        green_mask: 0,
                        blue_mask: 0,
                        alpha_mask: 0,
                    },
                    cs_type: ColorSpaceType::ProfileEmbedded,
                    endpoints: zero_endpoints(),
                    gamma: zero_gamma(),
                },
                intent: 0,
                profile_data: BitmapV5Header::HEADER_SIZE,
                profile_size: 4,
                reserved: [0; 4],
            },
            color_table: Vec::new(),
            bitmap_array: vec![0, 0, 0, 0],
            icc_profile: Some(vec![1, 2, 3, 4]),
        });

        assert!(bmp.validate().is_ok());
    }
}
