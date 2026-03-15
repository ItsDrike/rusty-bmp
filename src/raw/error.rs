use thiserror::Error;

use crate::raw::types::{BitsPerPixel, ColorMaskChannel, ColorSpaceType, Compression, RgbQuad};

#[derive(Debug, Clone, Copy)]
pub enum IoStage {
    ReadingFileHeader,
    ReadingDibHeader,
    ReadingColorMasks,
    ReadingColorTable,
    ReadingPixelData,
    ReadingIccProfile,
}

impl std::fmt::Display for IoStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::ReadingFileHeader => "reading file header",
            Self::ReadingDibHeader => "reading DIB header",
            Self::ReadingColorMasks => "reading color masks",
            Self::ReadingColorTable => "reading color table",
            Self::ReadingPixelData => "reading pixel data",
            Self::ReadingIccProfile => "reading ICC profile",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Error)]
pub enum BmpError {
    #[error(transparent)]
    Structural(#[from] StructuralError),

    #[error(transparent)]
    Validation(#[from] ValidationError),
}

/// Errors indicating that the BMP structure cannot be interpreted safely.
///
/// The parser is intentionally permissive and allows reading and writing BMP
/// structures without performing full validation. However, some situations make
/// it impossible to determine the structure of the file in a well-defined way
/// according to the BMP specification. In such cases, parsing is aborted with a
/// `StructuralError`.
///
/// These errors represent failures that prevent the parser from safely
/// determining the layout of the BMP data. This includes:
///
/// - I/O failures that occur while reading or writing BMP data
/// - arithmetic overflows while computing sizes or offsets
/// - structures that cannot be interpreted according to the BMP specification
/// - inputs rejected due to explicit safety limits (for example extremely large
///   sizes that could lead to excessive memory allocation)
///
/// Unlike [`ValidationError`], structural errors are not about BMP files being
/// invalid according to the specification. Instead, they indicate that the
/// parser cannot safely continue because the structure of the file cannot be
/// determined.
#[derive(Debug, Error)]
pub enum StructuralError {
    #[error("I/O error while {stage}: {source}")]
    Io { source: std::io::Error, stage: IoStage },

    #[error("Arithmetic overflow: {0}")]
    ArithmeticOverflow(String),

    #[error("Given BMP structure cannot be processed safely: {0}")]
    StructureUnsafe(String),

    #[error("unsupported BMP structure: {0}")]
    UnsupportedStructure(String),
}

impl StructuralError {
    pub(crate) const fn from_io(source: std::io::Error, stage: IoStage) -> Self {
        Self::Io { source, stage }
    }
}

/// Errors indicating that a parsed BMP structure violates rules defined by
/// the BMP specification.
///
/// Unlike [`StructuralError`], these errors do not necessarily mean that the
/// file cannot be parsed safely. The parser is intentionally permissive and
/// may still successfully read BMP data that does not strictly conform to
/// the format specification.
///
/// These errors are returned only when explicit validation is performed
/// (for example by calling `validate()`), allowing callers to decide whether
/// strict compliance with the BMP specification is desired.
#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("The BMP file signature: ({0:?}) is invalid, expected 0x4d42 ('BM')")]
    InvalidFileSignature([u8; 2]),

    #[error("The BMP file header has non-zero reserved data: {0:?}")]
    InvalidFileReservedData([u8; 4]),

    #[error("Invalid width value: {0}")]
    InvalidWidth(i32),

    #[error("Invalid height value: {0}")]
    InvalidHeight(i32),

    #[error("Number of planes must be 1, found {0}")]
    InvalidPlanes(u16),

    #[error("Invalid bitcount value: {0}")]
    InvalidBitCount(BitsPerPixel),

    #[error("Top-Down images (with height < 0) support only RGB or BITFIELDS compression, got {0:?}")]
    InvalidCompressionForTopDown(Compression),

    #[error("The compression variant {compression:?} cannot be used for bits-per-pixel value of {bpp}")]
    InvalidCompressionForBpp {
        compression: Compression,
        bpp: BitsPerPixel,
    },

    #[error("A compression value of {0} is not recognized as any known BMP compression variant")]
    UnknownCompression(u32),

    #[error("compressed images ({0:?}) must specify a non-zero image_size")]
    CompressedImageMissingSize(Compression),

    #[error(transparent)]
    InvalidColorMasks(#[from] ColorMaskError),

    #[error("Invalid value for the color space type field: {0:?}")]
    InvalidColorSpaceType(ColorSpaceType),

    #[error("Color table with {colors_used} entries is not allowed for compression {compression:?}")]
    PaletteNotAllowedForCompression { compression: Compression, colors_used: u32 },

    #[error(
        "The reported image size from the BMP header ({reported}) does not match the computed uncompressed image size ({computed})"
    )]
    UncompressedImageSizeMismatch { reported: u32, computed: u32 },

    #[error("Given RgbQuad structure breaks expected invariants")]
    InvalidRgbQuad(RgbQuad),

    #[error("Stored color table size ({stored_size}) doesn't match the encoded table size {header_size}")]
    ColorTableSizeMismatch { stored_size: usize, header_size: usize },

    #[error("Stored pixel data size ({stored_size}) doesn't match the encoded size {header_size}")]
    PixelDataSizeMismatch { stored_size: usize, header_size: usize },

    #[error(transparent)]
    PixelDataLayout(#[from] PixelDataLayoutError),

    #[error(transparent)]
    IccProfile(#[from] IccProfileError),
}

#[derive(Debug, Error)]
pub enum PixelDataLayoutError {
    #[error("Pixel data offset from file header overlaps with other data: {pixel_offset_header} < {min_offset}")]
    OverlapsMetadata { pixel_offset_header: u32, min_offset: u32 },

    #[error("Pixel data too large (ends beyond the defined file size)")]
    ExceedsFileSize { pixel_end: u64, file_size: u32 },

    #[error("Pixel data ends before the file end - leaves a gap in the file")]
    DoesNotEndAtFileEnd { pixel_end: u64, file_size: u32 },
}

#[derive(Debug, Error)]
pub enum ColorMaskError {
    #[error("non-contiguous {channel} color mask: {mask:#010X}")]
    NonContiguous { mask: u32, channel: ColorMaskChannel },

    #[error(
        "overlapping color masks: {channel_a} ({mask_a:#010X}) overlaps \
         with {channel_b} ({mask_b:#010X})"
    )]
    Overlapping {
        mask_a: u32,
        channel_a: ColorMaskChannel,
        mask_b: u32,
        channel_b: ColorMaskChannel,
    },

    #[error("color mask for {channel} ({mask:#010X}) exceeds declared {bpp}-bit depth")]
    ExceedsBitDepth {
        mask: u32,
        channel: ColorMaskChannel,
        bpp: BitsPerPixel,
    },
}

#[derive(Debug, Error)]
pub enum IccProfileError {
    #[error("ICC profile data is required for color space type {cs_type:?}")]
    MissingDataForProfileColorSpace { cs_type: ColorSpaceType },

    #[error(
        "ICC profile fields must be zero when color space type is {cs_type:?}, got profile_data={profile_data}, profile_size={profile_size}"
    )]
    UnexpectedDataForNonProfileColorSpace {
        cs_type: ColorSpaceType,
        profile_data: u32,
        profile_size: u32,
    },

    #[error("Stored ICC profile size ({stored_size}) doesn't match the encoded size {header_size}")]
    SizeMismatch { stored_size: usize, header_size: usize },

    #[error("ICC profile offset overlaps with BMP metadata: {profile_offset} < {min_offset}")]
    OverlapsMetadata { profile_offset: u64, min_offset: u64 },

    #[error("ICC profile ends beyond the defined file size")]
    ExceedsFileSize { profile_end: u64, file_size: u32 },
}
