use std::{
    io::{Read, Seek, SeekFrom, Write},
    sync::Arc,
};

use crate::raw::{
    BitmapCoreHeader, BitmapHeader, BitmapInfoHeader, BitmapV4Header, BitmapV5Header, FileHeader, RgbMasks,
    error::{BmpError, IccProfileError, IoStage, PixelDataLayoutError, StructuralError, ValidationError},
    helpers::{BoundedStream, ColorTable},
    types::{ColorSpaceType, Compression, RgbQuad, RgbTriple},
};

const MAX_PIXEL_BYTES: usize = 512 * 1024 * 1024; // 512 MB
const MAX_ICC_PROFILE_BYTES: usize = 16 * 1024 * 1024; // 16 MB

/// Identifies the DIB header family used by a BMP payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DibVariant {
    /// OS/2 `BITMAPCOREHEADER` (CORE/V2 style layout).
    Core,
    /// Windows `BITMAPINFOHEADER` (INFO/V3 style layout).
    Info,
    /// Windows `BITMAPV4HEADER`.
    V4,
    /// Windows `BITMAPV5HEADER`.
    V5,
}

/// Parsed BMP data for CORE/V2-style bitmaps.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BitmapCoreData {
    /// File-level BMP header (`BITMAPFILEHEADER`).
    pub file_header: FileHeader,

    /// CORE DIB header (`BITMAPCOREHEADER`).
    pub bmp_header: BitmapCoreHeader,

    /// Optional palette/color table encoded as `RGBTRIPLE` entries.
    pub color_table: Arc<[RgbTriple]>,

    /// Raw encoded pixel payload.
    pub bitmap_array: Arc<[u8]>,
}

/// Parsed BMP data for INFO/V3-style bitmaps.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BitmapInfoData {
    /// File-level BMP header (`BITMAPFILEHEADER`).
    pub file_header: FileHeader,

    /// INFO DIB header (`BITMAPINFOHEADER`).
    pub bmp_header: BitmapInfoHeader,

    /// Optional bitfield masks present for `BI_BITFIELDS` INFO payloads.
    // TODO: We might want to make this just ColorMasks (enum) instead, depending on whether or not
    // we want to support BI_ALPHABITFIELDS
    pub color_masks: Option<RgbMasks>,

    /// Optional palette/color table encoded as `RGBQUAD` entries.
    pub color_table: Arc<[RgbQuad]>,

    /// Raw encoded pixel payload.
    pub bitmap_array: Arc<[u8]>,
}

/// Parsed BMP data for V4-header bitmaps.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BitmapV4Data {
    /// File-level BMP header (`BITMAPFILEHEADER`).
    pub file_header: FileHeader,

    /// V4 DIB header (`BITMAPV4HEADER`).
    pub bmp_header: BitmapV4Header,

    /// Optional palette/color table encoded as `RGBQUAD` entries.
    pub color_table: Arc<[RgbQuad]>,

    /// Raw encoded pixel payload.
    pub bitmap_array: Arc<[u8]>,
}

/// Parsed BMP data for V5-header bitmaps.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BitmapV5Data {
    /// File-level BMP header (`BITMAPFILEHEADER`).
    pub file_header: FileHeader,

    /// V5 DIB header (`BITMAPV5HEADER`).
    pub bmp_header: BitmapV5Header,

    /// Optional palette/color table encoded as `RGBQUAD` entries.
    pub color_table: Arc<[RgbQuad]>,

    /// Raw encoded pixel payload.
    pub bitmap_array: Arc<[u8]>,

    /// Optional embedded/linked ICC profile payload for profile color spaces.
    pub icc_profile: Option<Arc<[u8]>>,
}

/// Raw BMP container keyed by concrete DIB header generation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Bmp {
    /// CORE/V2 bitmap payload.
    Core(BitmapCoreData),
    /// INFO/V3 bitmap payload.
    Info(BitmapInfoData),
    /// V4 bitmap payload.
    V4(BitmapV4Data),
    /// V5 bitmap payload.
    V5(BitmapV5Data),
}

impl Bmp {
    /// Returns a shared reference to the BMP file header.
    ///
    /// This is a zero-cost view over the variant-specific storage.
    #[must_use]
    pub const fn file_header(&self) -> &FileHeader {
        match self {
            Self::Core(data) => &data.file_header,
            Self::Info(data) => &data.file_header,
            Self::V4(data) => &data.file_header,
            Self::V5(data) => &data.file_header,
        }
    }

    /// Returns the DIB header as a unified [`BitmapHeader`] value.
    ///
    /// This clones the inner header to provide a variant-agnostic representation.
    #[inline]
    #[must_use]
    pub fn bitmap_header(&self) -> BitmapHeader {
        match self {
            Self::Core(data) => BitmapHeader::Core(data.bmp_header.clone()),
            Self::Info(data) => BitmapHeader::Info(data.bmp_header.clone()),
            Self::V4(data) => BitmapHeader::V4(data.bmp_header.clone()),
            Self::V5(data) => BitmapHeader::V5(data.bmp_header.clone()),
        }
    }

    /// Returns the encoded bitmap pixel payload as a raw byte slice.
    ///
    /// The layout/interpretation depends on the header compression and bit depth.
    #[inline]
    #[must_use]
    pub fn bitmap_array(&self) -> &[u8] {
        match self {
            Self::Core(data) => &data.bitmap_array,
            Self::Info(data) => &data.bitmap_array,
            Self::V4(data) => &data.bitmap_array,
            Self::V5(data) => &data.bitmap_array,
        }
    }

    /// Builds a temporary internal color-table helper for shared operations.
    ///
    /// This only clones the backing [`Arc`] and therefore does not duplicate
    /// color table buffers.
    #[inline]
    #[must_use]
    fn color_table(&self) -> ColorTable {
        match self {
            Self::Core(data) => ColorTable::Core(Arc::clone(&data.color_table)),
            Self::Info(data) => ColorTable::InfoOrLater(Arc::clone(&data.color_table)),
            Self::V4(data) => ColorTable::InfoOrLater(Arc::clone(&data.color_table)),
            Self::V5(data) => ColorTable::InfoOrLater(Arc::clone(&data.color_table)),
        }
    }

    /// Validates BMP structural and semantic consistency for the current variant.
    ///
    /// # Errors
    /// Returns [`BmpError`] when any header, mask, table, pixel layout, or ICC profile
    /// invariant is violated.
    pub fn validate(&self) -> Result<(), BmpError> {
        let file_header = self.file_header();
        file_header.validate()?;

        let header = self.bitmap_header();
        header.validate()?;

        let color_table_size_header = header.color_table_size()? as usize;
        let pixel_data_size_header = header.pixel_data_size()? as usize;

        let color_table = self.color_table();
        color_table.validate(color_table_size_header)?;

        Self::validate_pixel_data_size(self.bitmap_array().len(), pixel_data_size_header)?;

        let (dib_header_size, color_entry_size, extra_size) = match self {
            Self::Core(_) => (BitmapCoreHeader::HEADER_SIZE, 3, 0),
            Self::Info(data) => {
                Self::validate_info_masks(&header, data.color_masks.as_ref())?;
                let masks_size = if data.color_masks.is_some() { 12 } else { 0 };
                (BitmapInfoHeader::HEADER_SIZE, 4, masks_size)
            }
            Self::V4(_) => (BitmapV4Header::HEADER_SIZE, 4, 0),
            Self::V5(_) => (BitmapV5Header::HEADER_SIZE, 4, 0),
        };

        let min_pixel_offset =
            Self::min_pixel_offset(dib_header_size, color_table.len(), color_entry_size, extra_size)?;
        let pixel_end = Self::pixel_end_with_overlap_check(file_header, min_pixel_offset, pixel_data_size_header)?;

        let icc_end = match self {
            Self::V5(data) => Self::validate_v5_icc_profile(data, color_table.len(), file_header.file_size)?,
            _ => 0,
        };

        let required_file_end = pixel_end.max(icc_end);
        Self::validate_file_end(file_header.file_size, required_file_end)?;

        Ok(())
    }

    /// Reads a BMP payload with only the structural checks required for safe parsing.
    ///
    /// This does not perform semantic/spec validation. Call [`Self::validate`] explicitly
    /// if strict BMP compliance checks are desired.
    ///
    /// Hint: You will almost always want to call validate, as otherwise, you cannot even
    /// trust that the file which was read was in fact a BMP, and working with it in any
    /// capacity could easily result in issues. The reason we don't do this automatically
    /// is mostly semantic and for the few edge cases where you might just wish to analyze
    /// a potentially invalid BMP.
    ///
    /// # Errors
    /// Returns [`StructuralError`] for malformed structures, out-of-bounds offsets/sizes,
    /// arithmetic overflows, memory-safety limits, or I/O failures while reading.
    pub fn read_unchecked<R: Read + Seek>(reader: &mut R) -> Result<Self, StructuralError> {
        let file_header =
            FileHeader::read_unchecked(reader).map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;

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

        // The V3 / INFO header supports having embedded color masks.
        // No other variant has support for this, as V4+ embeds the masks into
        // the DIB header directly, and V2 / CORE doesn't have bitfields support
        // at all.
        let masks = if let BitmapHeader::Info(header) = &bmp_header
            && header.compression == Compression::BitFields
        {
            let masks = RgbMasks::read_unchecked(&mut reader)
                .map_err(|e| StructuralError::from_io(e, IoStage::ReadingColorMasks))?;
            Some(masks)
        } else {
            None
        };

        let color_table = ColorTable::read_unchecked(&mut reader, &bmp_header)?;

        let pixel_data_size = bmp_header.pixel_data_size()? as usize;
        if pixel_data_size > MAX_PIXEL_BYTES {
            return Err(StructuralError::StructureUnsafe(format!(
                "Pixel data contains {pixel_data_size} entries, which is higher than the allowed safe maximum: {MAX_PIXEL_BYTES}"
            )));
        }

        let pixel_data_pos = u64::from(file_header.pixel_data_offset);
        if pixel_data_pos > u64::from(file_header.file_size) {
            return Err(PixelDataLayoutError::ExceedsFileSize {
                pixel_end: pixel_data_pos,
                file_size: file_header.file_size,
            }
            .into());
        }

        // TODO: Check if there are some further data embedded in the BMP before the
        // pixel data. If yes, it could be the ICC color profiles (though these usually
        // come after the bitmap array, the spec does technically allow them to be here
        // too), alternatively, it could also be some custom metadata that a specific
        // application chose to embed into the BMP without violating the standard.
        //
        // We might want to collect the information about what's in this gap, even if
        // we can't interpret it as it's non-standard. Though, we should only do so if
        // this actually isn't the ICC profile, as that would then just duplicate data
        // and potentially be misleading. Though differentiating that + handling this
        // cleanly might become messy. Especially if the ICC profile data is somewhere
        // in the middle of this gap for example.
        //
        // let gap_pos = reader.stream_position()?;
        // let metadata_size = pixel_data_pos - gap_pos;

        reader
            .seek(SeekFrom::Start(pixel_data_pos))
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingPixelData))?;

        let mut pixel_data = vec![0u8; pixel_data_size];
        reader
            .read_exact(&mut pixel_data)
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingPixelData))?;

        // Masks are only present for INFO+BITFIELDS, and absent for all other variants.
        debug_assert!(matches!((&bmp_header, &masks), (BitmapHeader::Info(_), _) | (_, None)));

        let bmp = match (bmp_header, color_table) {
            (BitmapHeader::Core(header), ColorTable::Core(color_table)) => Self::Core(BitmapCoreData {
                file_header,
                bmp_header: header,
                color_table,
                bitmap_array: Arc::from(pixel_data),
            }),
            (BitmapHeader::Info(header), ColorTable::InfoOrLater(color_table)) => Self::Info(BitmapInfoData {
                file_header,
                bmp_header: header,
                color_masks: masks,
                color_table,
                bitmap_array: Arc::from(pixel_data),
            }),
            (BitmapHeader::V4(header), ColorTable::InfoOrLater(color_table)) => Self::V4(BitmapV4Data {
                file_header,
                bmp_header: header,
                color_table,
                bitmap_array: Arc::from(pixel_data),
            }),
            (BitmapHeader::V5(header), ColorTable::InfoOrLater(color_table)) => {
                let icc_profile = Self::read_v5_icc_profile(&mut reader, &header)?;

                Self::V5(BitmapV5Data {
                    file_header,
                    bmp_header: header,
                    color_table,
                    bitmap_array: Arc::from(pixel_data),
                    icc_profile,
                })
            }
            _ => unreachable!("color table variant must match DIB header variant"),
        };

        // Leave the reader at the end of the BMP file
        // TODO: Should we check that the reader is at the file end by the time we're done reading
        // instead? The answer is likely no, there is no real reason to prevent reads of BMPs that
        // have gaps from the final embedded data and the file end in terms of structural
        // coherency, and the standard doesn't seem to say anything about this being wrong anyways.
        // But if this is the case, we might want to consider whether we want to retrieve the data
        // in this gap too, some programs might be using it to store some metadata.
        reader
            .seek(SeekFrom::End(0))
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;

        Ok(bmp)
    }

    /// Writes the BMP structure as-is without re-deriving field values.
    ///
    /// # Errors
    /// Returns [`StructuralError`] if layout checks fail or any write/seek operation fails.
    pub fn write_unchecked<W: Write + Seek>(&self, writer: &mut W) -> Result<(), StructuralError> {
        self.validate_write_layout()?;

        let mut writer = BoundedStream::new(writer)
            .shrink_start(SeekFrom::Current(0))
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;

        let file_header = self.file_header();
        file_header
            .write_unchecked(&mut writer)
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;
        self.bitmap_header()
            .write_unchecked(&mut writer)
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;

        let pixel_data_offset = u64::from(file_header.pixel_data_offset);

        if let Self::Info(data) = self
            && let Some(masks) = &data.color_masks
        {
            masks
                .write_unchecked(&mut writer)
                .map_err(|e| StructuralError::from_io(e, IoStage::ReadingColorMasks))?;
        }

        let color_table = self.color_table();
        color_table.write_unchecked(&mut writer)?;

        writer
            .seek(SeekFrom::Start(pixel_data_offset))
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingPixelData))?;
        writer
            .write_all(self.bitmap_array())
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingPixelData))?;

        if let Self::V5(data) = self
            && let Some(profile) = &data.icc_profile
        {
            let profile_offset = data.bmp_header.profile_data + FileHeader::SIZE;
            writer
                .seek(SeekFrom::Start(u64::from(profile_offset)))
                .map_err(|e| StructuralError::from_io(e, IoStage::ReadingIccProfile))?;
            writer
                .write_all(profile)
                .map_err(|e| StructuralError::from_io(e, IoStage::ReadingIccProfile))?;
        }

        let file_end = u64::from(file_header.file_size);

        // Leave the writer at the declared end of this BMP payload.
        writer
            .seek(SeekFrom::Start(file_end))
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingFileHeader))?;

        Ok(())
    }

    /// Validates pixel-data length consistency against the DIB header declaration.
    ///
    /// # Errors
    /// Returns [`BmpError`] if the stored byte count differs from the header size
    /// or if the declared size exceeds memory-safety limits.
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

    /// Computes the minimum legal pixel-data offset for the given header layout.
    ///
    /// The computed value includes file header, DIB header, optional extra data
    /// (such as INFO bit masks), and color table bytes.
    ///
    /// # Errors
    /// Returns [`StructuralError`] if intermediate arithmetic overflows.
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

    /// Computes the end offset of pixel data while enforcing metadata overlap rules.
    ///
    /// # Errors
    /// Returns [`BmpError`] if the header pixel offset overlaps metadata or if
    /// calculating the pixel end offset overflows.
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

    /// Verifies that logical payload end matches the declared BMP file size.
    ///
    /// # Errors
    /// Returns [`ValidationError`] if the payload exceeds file size or does not
    /// terminate exactly at the declared file end.
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

    /// Validates INFO-header optional bitfield mask presence and compatibility.
    ///
    /// # Errors
    /// Returns [`BmpError`] when `BI_BITFIELDS` masks are missing, unexpected
    /// masks are present for non-bitfields compression, or mask bit-depth rules
    /// are violated.
    fn validate_info_masks(header: &BitmapHeader, masks: Option<&RgbMasks>) -> Result<(), BmpError> {
        let compression = header.compression();
        let bpp = header.bit_count();

        if compression == Compression::BitFields {
            let Some(masks) = masks else {
                return Err(ValidationError::InvalidCompressionForBpp {
                    compression: Compression::BitFields,
                    bpp,
                }
                .into());
            };

            masks.validate_for_bpp(bpp).map_err(ValidationError::from)?;
            return Ok(());
        }

        if masks.is_some() {
            return Err(ValidationError::InvalidCompressionForBpp { compression, bpp }.into());
        }

        Ok(())
    }

    /// Reads V5 ICC profile bytes when the color space type requires a profile.
    ///
    /// For non-profile color spaces this returns `Ok(None)`.
    ///
    /// # Errors
    /// Returns [`StructuralError`] for unsafe sizes, arithmetic overflows, or
    /// I/O failures while reading profile data.
    fn read_v5_icc_profile<R: Read + Seek>(
        reader: &mut R,
        header: &BitmapV5Header,
    ) -> Result<Option<Arc<[u8]>>, StructuralError> {
        if !matches!(
            header.v4.cs_type,
            ColorSpaceType::ProfileEmbedded | ColorSpaceType::ProfileLinked
        ) {
            return Ok(None);
        }

        let offset = u64::from(header.profile_data)
            .checked_add(u64::from(FileHeader::SIZE))
            .ok_or_else(|| {
                StructuralError::ArithmeticOverflow("ICC profile absolute offset calculation".to_owned())
            })?;

        let size = header.profile_size as usize;
        if size > MAX_ICC_PROFILE_BYTES {
            return Err(StructuralError::StructureUnsafe(format!(
                "ICC profile contains {size} bytes, which is higher than the allowed safe maximum: {MAX_ICC_PROFILE_BYTES}"
            )));
        }

        // TODO: Maybe also validate that the offset isn't within the color table / color
        // masks / dib header, though this isn't that important, as we do validate that
        // it is within the file offset, so this is purely about preventing it from
        // reading wrong data, though I'm not even certain that the standard forbids
        // this. In theory, if the color table bytes do resolve to a valid ICC profile
        // too, there's not real reason to prevent that, even if it's really dumb and
        // unlikely. Safety-wise, this isn't important.
        offset
            .checked_add(size as u64)
            .ok_or_else(|| StructuralError::ArithmeticOverflow("ICC profile end calculation".to_owned()))?;

        reader
            .seek(SeekFrom::Start(offset))
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingIccProfile))?;

        let mut data = vec![0u8; size];
        reader
            .read_exact(&mut data)
            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingIccProfile))?;

        Ok(Some(Arc::from(data)))
    }

    /// Validates V5 ICC profile metadata consistency and returns its end offset.
    ///
    /// Returns `0` when no ICC profile region is expected.
    ///
    /// # Errors
    /// Returns [`BmpError`] when profile presence/size/offset constraints are
    /// violated or when required arithmetic overflows.
    fn validate_v5_icc_profile(data: &BitmapV5Data, color_table_len: usize, file_size: u32) -> Result<u64, BmpError> {
        match data.bmp_header.v4.cs_type {
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
                        StructuralError::ArithmeticOverflow("ICC profile absolute offset calculation".to_owned())
                    })?;

                let min_profile_offset = u64::from(FileHeader::SIZE)
                    .checked_add(u64::from(BitmapV5Header::HEADER_SIZE))
                    .and_then(|x| x.checked_add((color_table_len as u64).checked_mul(4)?))
                    .ok_or_else(|| {
                        StructuralError::ArithmeticOverflow("ICC profile min offset calculation".to_owned())
                    })?;

                if profile_offset_absolute < min_profile_offset {
                    Err(ValidationError::IccProfile(IccProfileError::OverlapsMetadata {
                        profile_offset: profile_offset_absolute,
                        min_offset: min_profile_offset,
                    }))?;
                }

                let profile_end = profile_offset_absolute
                    .checked_add(profile.len() as u64)
                    .ok_or_else(|| StructuralError::ArithmeticOverflow("ICC profile end calculation".to_owned()))?;
                if profile_end > u64::from(file_size) {
                    Err(ValidationError::IccProfile(IccProfileError::ExceedsFileSize {
                        profile_end,
                        file_size,
                    }))?;
                }

                Ok(profile_end)
            }
            _ => {
                if data.icc_profile.is_some() || data.bmp_header.profile_data != 0 || data.bmp_header.profile_size != 0
                {
                    Err(ValidationError::IccProfile(
                        IccProfileError::UnexpectedDataForNonProfileColorSpace {
                            cs_type: data.bmp_header.v4.cs_type,
                            profile_data: data.bmp_header.profile_data,
                            profile_size: data.bmp_header.profile_size,
                        },
                    ))?;
                }
                Ok(0)
            }
        }
    }

    /// Performs write-path structural preflight checks.
    ///
    /// Unlike [`Self::validate`], this check is focused on constraints required
    /// for safe serialization (buffer sizes and on-disk bounds), not full BMP
    /// semantic validation.
    ///
    /// # Errors
    /// Returns [`StructuralError`] if serialized layout would be out of bounds,
    /// unsafe, or internally inconsistent for writing.
    fn validate_write_layout(&self) -> Result<(), StructuralError> {
        let header = self.bitmap_header();
        let pixel_data_size_header = header.pixel_data_size()? as usize;
        let pixel_data_size_stored = self.bitmap_array().len();
        if pixel_data_size_stored != pixel_data_size_header {
            return Err(StructuralError::PixelDataSizeMismatch {
                stored_size: pixel_data_size_stored,
                header_size: pixel_data_size_header,
            });
        }
        if pixel_data_size_header > MAX_PIXEL_BYTES {
            return Err(StructuralError::StructureUnsafe(format!(
                "Pixel data contains {pixel_data_size_header} entries, which is higher than the allowed safe maximum: {MAX_PIXEL_BYTES}"
            )));
        }

        let file_header = self.file_header();
        if file_header.pixel_data_offset < FileHeader::SIZE {
            return Err(PixelDataLayoutError::OverlapsMetadata {
                pixel_offset_header: file_header.pixel_data_offset,
                min_offset: FileHeader::SIZE,
            }
            .into());
        }

        let pixel_end = u64::from(file_header.pixel_data_offset)
            .checked_add(pixel_data_size_header as u64)
            .ok_or_else(|| StructuralError::ArithmeticOverflow("Pixel data end calculation".to_owned()))?;
        if pixel_end > u64::from(file_header.file_size) {
            return Err(PixelDataLayoutError::ExceedsFileSize {
                pixel_end,
                file_size: file_header.file_size,
            }
            .into());
        }

        if let Self::V5(data) = self {
            let icc_end = if let Some(profile) = &data.icc_profile {
                if profile.len() > MAX_ICC_PROFILE_BYTES {
                    return Err(StructuralError::StructureUnsafe(format!(
                        "ICC profile contains {} bytes, which is higher than the allowed safe maximum: {MAX_ICC_PROFILE_BYTES}",
                        profile.len()
                    )));
                }

                let profile_offset_absolute = u64::from(FileHeader::SIZE)
                    .checked_add(u64::from(data.bmp_header.profile_data))
                    .ok_or_else(|| {
                        StructuralError::ArithmeticOverflow("ICC profile absolute offset calculation".to_owned())
                    })?;

                let profile_end = profile_offset_absolute
                    .checked_add(profile.len() as u64)
                    .ok_or_else(|| StructuralError::ArithmeticOverflow("ICC profile end calculation".to_owned()))?;
                if profile_end > u64::from(file_header.file_size) {
                    return Err(IccProfileError::ExceedsFileSize {
                        profile_end,
                        file_size: file_header.file_size,
                    }
                    .into());
                }

                profile_end
            } else {
                0
            };

            let required_file_end = pixel_end.max(icc_end);
            if required_file_end > u64::from(file_header.file_size) {
                return Err(PixelDataLayoutError::ExceedsFileSize {
                    pixel_end: required_file_end,
                    file_size: file_header.file_size,
                }
                .into());
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw::{
        BitsPerPixel, CieXyz, CieXyzTriple, FixedPoint2Dot30, FixedPoint16Dot16, GammaTriple, RgbaMasks,
    };

    /// Returns a zeroed CIEXYZ endpoint triple for test fixture construction.
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

    /// Returns a zeroed gamma triple for test fixture construction.
    fn zero_gamma() -> GammaTriple {
        GammaTriple {
            red: FixedPoint16Dot16::from_raw(0),
            green: FixedPoint16Dot16::from_raw(0),
            blue: FixedPoint16Dot16::from_raw(0),
        }
    }

    /// Ensures canonical INFO pixel offset passes validation.
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
            color_table: Arc::from(Vec::<RgbQuad>::new()),
            bitmap_array: Arc::from(vec![0, 0, 0, 0]),
        });

        assert!(bmp.validate().is_ok());
    }

    /// Ensures V5 profile located at first legal metadata boundary is accepted.
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
            color_table: Arc::from(Vec::<RgbQuad>::new()),
            bitmap_array: Arc::from(vec![0, 0, 0, 0]),
            icc_profile: Some(Arc::from(vec![1, 2, 3, 4])),
        });

        assert!(bmp.validate().is_ok());
    }

    /// Ensures `read_unchecked` does not enforce semantic signature validation.
    #[test]
    fn read_unchecked_defers_signature_validation() {
        use std::io::Cursor;

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
            color_table: Arc::from(Vec::<RgbQuad>::new()),
            bitmap_array: Arc::from(vec![0, 0, 0, 0]),
        });

        let mut encoded = Cursor::new(Vec::<u8>::new());
        bmp.write_unchecked(&mut encoded).expect("serialize test bmp");
        let mut bytes = encoded.into_inner();
        bytes[0] = b'Z';
        bytes[1] = b'Z';

        let mut cursor = Cursor::new(bytes);
        let parsed = Bmp::read_unchecked(&mut cursor).expect("read_unchecked should not enforce file signature");
        assert!(matches!(
            parsed.validate(),
            Err(BmpError::Validation(ValidationError::InvalidFileSignature([
                b'Z', b'Z'
            ])))
        ));
    }
}
