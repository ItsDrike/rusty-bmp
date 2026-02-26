use thiserror::Error;

use crate::raw::types::{BitsPerPixel, ColorMaskChannel, ColorSpaceType, Compression};

#[derive(Error, Debug)]
pub enum BmpError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("The BMP file signature: ({0:?}) is invalid, expected 0x4d42 ('BM')")]
    InvalidFileSignature([u8; 2]),

    #[error("The BMP header size value of {0} did not match any supported BMP variant")]
    InvalidHeaderSize(u32),

    #[error("The width value of {0} is invalid")]
    InvalidWidth(i32),

    #[error("The height value of {0} is invalid")]
    InvalidHeight(i32),

    #[error("The number of color planes must be 1, found {0}")]
    InvalidPlanes(u16),

    #[error("The bit count (bits-per-pixel) value of {0} is invalid")]
    InvalidBitCount(u16),

    #[error("Top-Down images (with height < 0) support only RGB or BITFIELDS compression, got {compression:?}")]
    InvalidCompressionForTopDown { compression: Compression },

    #[error("The compression variant {compression:?} cannot be used for bits-per-pixel value of {bpp}")]
    InvalidCompressionForBpp {
        compression: Compression,
        bpp: BitsPerPixel,
    },

    #[error("The image size of {image_size} cannot be used with compression variant {compression:?}")]
    InvalidImageSizeForCompression { image_size: u32, compression: Compression },

    #[error("Non-contiguous {channel} color mask: {mask:#010X}")]
    NonContiguousColorMask { mask: u32, channel: ColorMaskChannel },

    #[error(
        "Overlapping color masks: {channel_a} channel ({mask_a:#010X}) overlaps \
        with {channel_b} channel ({mask_b:#010X})"
    )]
    OverlappingColorMasks {
        mask_a: u32,
        channel_a: ColorMaskChannel,
        mask_b: u32,
        channel_b: ColorMaskChannel,
    },

    #[error("Color mask for {channel} ({mask:#010X}) exceeds the declared {bpp}-bit pixel depth")]
    MaskExceedsBitDepth {
        mask: u32,
        channel: ColorMaskChannel,
        bpp: BitsPerPixel,
    },

    #[error("Invalid value for the color space type field: {0:?}")]
    InvalidColorSpaceType(ColorSpaceType),

    #[error("")] // TODO: figure out what to put here
    InvalidProfileOffset(u32),

    #[error(
        "Color table contains {used} entries, which exceeds the maximum \
        representable count ({max}) for this bit depth"
    )]
    PaletteExceedsBitDepth { used: u64, max: u64 },

    #[error("Color Table size of {0} entries exceeds file bounds or cannot be loaded/represented safely")]
    PaletteTooLarge(u32),

    #[error("Color table with {colors_used} entries is not allowed for compression {compression:?}")]
    PaletteNotAllowedForCompression { compression: Compression, colors_used: u32 },

    #[error(
        "Invalid image size for uncompressed bitmap: header value {header} does not match \
        computed size {expected}"
    )]
    InvalidUncompressedImageSize { expected: u32, header: u32 },

    #[error(
        "The bitmap width, height, and bit depth combination produces an image size that \
        exceeds loadable/representable limits"
    )]
    PixelDataTooLarge,

    #[error("The ICC color profile exceeds loadable/representable limits")]
    IccProfileTooLarge,
}

pub type BmpResult<T> = Result<T, BmpError>;
