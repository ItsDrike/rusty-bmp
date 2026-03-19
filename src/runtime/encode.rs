use std::{fs::File, io::BufWriter, path::Path};

use thiserror::Error;

use crate::{
    raw::{
        BitmapCoreData, BitmapCoreHeader, BitmapInfoData, BitmapInfoHeader, BitmapV4Data, BitmapV4Header,
        BitmapV5Data, BitmapV5Header, BitsPerPixel, Bmp, CieXyzTriple, ColorSpaceType, Compression, FileHeader,
        GammaTriple, RgbMasks, RgbQuad, RgbTriple, RgbaMasks,
    },
    runtime::{decode::DecodedImage, quantize},
};

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("arithmetic overflow while preparing BMP")]
    ArithmeticOverflow,

    #[error("save format {format} is not compatible with header version {header}")]
    IncompatibleHeaderFormat {
        format: SaveFormat,
        header: SaveHeaderVersion,
    },

    #[error("dimensions {width}x{height} exceed Core header maximum of 65535")]
    CoreDimensionOverflow { width: u32, height: u32 },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Bmp(#[from] crate::raw::BmpError),

    #[error(transparent)]
    Quantize(#[from] quantize::QuantizeError),
}

/// Selects the BMP pixel format used when saving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SaveFormat {
    /// `BI_RGB`, 1 bit per pixel (monochrome), palette of 2 colors.
    Rgb1,
    /// `BI_RGB`, 4 bits per pixel, palette of up to 16 colors.
    Rgb4,
    /// `BI_RGB`, 8 bits per pixel, palette of up to 256 colors.
    Rgb8,
    /// `BI_RGB`, 16 bits per pixel, RGB555 (no palette).
    Rgb16,
    /// `BI_RGB`, 24 bits per pixel (no palette).
    Rgb24,
    /// `BI_RGB`, 32 bits per pixel (no palette). This is the default.
    Rgb32,
    /// `BI_RLE8`, 8 bits per pixel, run-length encoded.
    Rle8,
    /// `BI_RLE4`, 4 bits per pixel, run-length encoded.
    Rle4,
    /// `BI_BITFIELDS`, 16 bits per pixel with RGB565 masks.
    BitFields16Rgb565,
    /// `BI_BITFIELDS`, 16 bits per pixel with RGB555 masks (same layout as Rgb16 but stored with explicit masks).
    BitFields16Rgb555,
    /// `BI_BITFIELDS`, 32 bits per pixel with standard RGB888 masks (8 bits per channel, no alpha).
    BitFields32,
}

impl SaveFormat {
    /// Returns all supported save format variants for use in UI dropdowns.
    pub const ALL: &[Self] = &[
        Self::Rgb1,
        Self::Rgb4,
        Self::Rgb8,
        Self::Rgb16,
        Self::Rgb24,
        Self::Rgb32,
        Self::Rle8,
        Self::Rle4,
        Self::BitFields16Rgb565,
        Self::BitFields16Rgb555,
        Self::BitFields32,
    ];
}

impl Default for SaveFormat {
    fn default() -> Self {
        Self::Rgb32
    }
}

/// Infer the save format from compression mode, bit depth, and optional color masks.
/// Shared logic for `SaveFormat::from_bmp()` across Info, V4, and V5 variants.
fn infer_format(comp: Compression, bpp: BitsPerPixel, rgb_masks: Option<RgbMasks>) -> SaveFormat {
    #[allow(clippy::match_same_arms)]
    match (comp, bpp) {
        (Compression::Rgb, BitsPerPixel::Bpp1) => SaveFormat::Rgb1,
        (Compression::Rgb, BitsPerPixel::Bpp4) => SaveFormat::Rgb4,
        (Compression::Rgb, BitsPerPixel::Bpp8) => SaveFormat::Rgb8,
        (Compression::Rgb, BitsPerPixel::Bpp16) => SaveFormat::Rgb16,
        (Compression::Rgb, BitsPerPixel::Bpp24) => SaveFormat::Rgb24,
        (Compression::Rgb, BitsPerPixel::Bpp32) => SaveFormat::Rgb32,
        (Compression::Rle8, BitsPerPixel::Bpp8) => SaveFormat::Rle8,
        (Compression::Rle4, BitsPerPixel::Bpp4) => SaveFormat::Rle4,
        (Compression::BitFields, BitsPerPixel::Bpp16) => match rgb_masks {
            Some(masks) if masks == RgbMasks::rgb565() => SaveFormat::BitFields16Rgb565,
            Some(masks) if masks == RgbMasks::rgb555() => SaveFormat::BitFields16Rgb555,
            _ => SaveFormat::Rgb32,
        },
        (Compression::BitFields, BitsPerPixel::Bpp32) => match rgb_masks {
            Some(masks) if masks == RgbMasks::rgb888() => SaveFormat::BitFields32,
            _ => SaveFormat::Rgb32,
        },
        _ => SaveFormat::Rgb32,
    }
}

impl SaveFormat {
    /// Infer the closest supported save format from a loaded [`Bmp`].
    ///
    /// For formats that we cannot save (e.g. JPEG/PNG embedded, or exotic
    /// bitfield layouts), this falls back to [`SaveFormat::Rgb32`].
    #[must_use]
    pub fn from_bmp(bmp: &Bmp) -> Self {
        match bmp {
            // Core header has no compression field; map by bpp only.
            Bmp::Core(core) => match core.bmp_header.bit_count {
                BitsPerPixel::Bpp1 => Self::Rgb1,
                BitsPerPixel::Bpp4 => Self::Rgb4,
                BitsPerPixel::Bpp8 => Self::Rgb8,
                BitsPerPixel::Bpp24 => Self::Rgb24,
                _ => Self::Rgb32,
            },
            Bmp::Info(info) => infer_format(info.bmp_header.compression, info.bmp_header.bit_count, info.color_masks),
            Bmp::V4(v4) => {
                let info = v4.bmp_header.info;
                let masks = v4.bmp_header.masks;
                if matches!(
                    (info.compression, info.bit_count),
                    (Compression::BitFields, BitsPerPixel::Bpp16 | BitsPerPixel::Bpp32)
                ) && masks.alpha_mask != 0
                {
                    Self::Rgb32
                } else {
                    infer_format(info.compression, info.bit_count, Some(masks.into()))
                }
            }
            Bmp::V5(v5) => {
                let info = v5.bmp_header.v4.info;
                let masks = v5.bmp_header.v4.masks;
                if matches!(
                    (info.compression, info.bit_count),
                    (Compression::BitFields, BitsPerPixel::Bpp16 | BitsPerPixel::Bpp32)
                ) && masks.alpha_mask != 0
                {
                    Self::Rgb32
                } else {
                    infer_format(info.compression, info.bit_count, Some(masks.into()))
                }
            }
        }
    }
}

impl SaveFormat {
    /// Returns `true` if saving with this format will bit-exactly preserve the
    /// LSB data embedded by steganography.
    ///
    /// Formats that quantize, downsample, or compress pixel values are unsafe
    /// because they destroy the LSBs used to carry the hidden payload.
    ///
    /// If `steg_config` is provided and the config uses the alpha channel
    /// (`a_bits > 0`), formats that discard alpha (Rgb24, Rgb32 - which stores
    /// the fourth byte as reserved/zero) are also considered unsafe.
    #[must_use]
    pub const fn preserves_steganography(
        self,
        steg_config: Option<crate::runtime::steganography::StegConfig>,
    ) -> bool {
        // Base check: does the format keep all 8 bits per channel intact?
        let base_safe = matches!(self, Self::Rgb24 | Self::Rgb32 | Self::BitFields32);

        if !base_safe {
            return false;
        }

        // Additional check: does the config use alpha, and does this format drop alpha?
        if let Some(config) = steg_config
            && config.a_bits() > 0
        {
            // Rgb24 has no alpha byte at all.
            // Rgb32 stores the 4th byte as a reserved zero - alpha data is lost.
            // BitFields32 uses standard RGB888 masks with no alpha mask.
            // None of the three safe base formats preserve alpha channel data.
            return false;
        }

        true
    }
}

impl std::fmt::Display for SaveFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rgb1 => write!(f, "RGB 1-bpp (monochrome)"),
            Self::Rgb4 => write!(f, "RGB 4-bpp (16 colors)"),
            Self::Rgb8 => write!(f, "RGB 8-bpp (256 colors)"),
            Self::Rgb16 => write!(f, "RGB 16-bpp (RGB555)"),
            Self::Rgb24 => write!(f, "RGB 24-bpp"),
            Self::Rgb32 => write!(f, "RGB 32-bpp (default)"),
            Self::Rle8 => write!(f, "RLE8 (8-bpp compressed)"),
            Self::Rle4 => write!(f, "RLE4 (4-bpp compressed)"),
            Self::BitFields16Rgb565 => write!(f, "BitFields 16-bpp (RGB565)"),
            Self::BitFields16Rgb555 => write!(f, "BitFields 16-bpp (RGB555)"),
            Self::BitFields32 => write!(f, "BitFields 32-bpp"),
        }
    }
}

// ===========================================================================
// SaveHeaderVersion
// ===========================================================================

/// Selects the BMP header version used when saving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SaveHeaderVersion {
    /// `BITMAPCOREHEADER` (12 bytes). Only supports Rgb1/4/8/24.
    /// Dimensions limited to u16 (max 65535). Always bottom-up.
    /// Color table uses 3-byte [`RgbTriple`] entries. Always 2^bpp palette entries.
    Core,
    /// `BITMAPINFOHEADER` (40 bytes). Supports all formats.
    Info,
    /// `BITMAPV4HEADER` (108 bytes). Supports all formats.
    /// Preserves color space, CIE endpoints, and gamma from source if available.
    V4,
    /// `BITMAPV5HEADER` (124 bytes). Supports all formats.
    /// Preserves V4 fields plus intent and ICC profile from source if available.
    V5,
}

impl SaveHeaderVersion {
    /// All supported header versions, for use in UI dropdowns.
    pub const ALL: &[Self] = &[Self::Core, Self::Info, Self::V4, Self::V5];

    /// Returns the formats compatible with this header version.
    ///
    /// Core only supports uncompressed RGB at 1/4/8/24 bpp.
    /// All other versions support every format.
    #[must_use]
    pub const fn compatible_formats(&self) -> &[SaveFormat] {
        match self {
            Self::Core => &[SaveFormat::Rgb1, SaveFormat::Rgb4, SaveFormat::Rgb8, SaveFormat::Rgb24],
            _ => SaveFormat::ALL,
        }
    }

    /// Returns `true` if the given format is compatible with this header version.
    #[must_use]
    pub fn is_compatible(&self, format: SaveFormat) -> bool {
        self.compatible_formats().contains(&format)
    }

    /// Infer the header version from a loaded [`Bmp`].
    #[must_use]
    pub const fn from_bmp(bmp: &Bmp) -> Self {
        match bmp {
            Bmp::Core(_) => Self::Core,
            Bmp::Info(_) => Self::Info,
            Bmp::V4(_) => Self::V4,
            Bmp::V5(_) => Self::V5,
        }
    }
}

impl Default for SaveHeaderVersion {
    fn default() -> Self {
        Self::Info
    }
}

impl std::fmt::Display for SaveHeaderVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Core => write!(f, "Core (12-byte)"),
            Self::Info => write!(f, "Info / V3 (40-byte)"),
            Self::V4 => write!(f, "V4 (108-byte)"),
            Self::V5 => write!(f, "V5 (124-byte)"),
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: row stride with 4-byte alignment
// ---------------------------------------------------------------------------

fn row_stride(width: usize, bits_per_pixel: u16) -> Result<usize, EncodeError> {
    let bits_per_row = width
        .checked_mul(bits_per_pixel as usize)
        .ok_or(EncodeError::ArithmeticOverflow)?;
    let with_padding = bits_per_row.checked_add(31).ok_or(EncodeError::ArithmeticOverflow)?;
    Ok((with_padding / 32) * 4)
}

// ---------------------------------------------------------------------------
// Shared construction helpers
// ---------------------------------------------------------------------------

/// Sums a slice of `u32` values with checked arithmetic.
fn checked_sum(values: &[u32]) -> Result<u32, EncodeError> {
    values.iter().try_fold(0u32, |acc, &v| {
        acc.checked_add(v).ok_or(EncodeError::ArithmeticOverflow)
    })
}

const fn make_file_header(file_size: u32, pixel_data_offset: u32) -> FileHeader {
    FileHeader {
        signature: *b"BM",
        file_size,
        reserved_1: [0; 2],
        reserved_2: [0; 2],
        pixel_data_offset,
    }
}

/// Builds the `BitmapInfoHeader` shared by Info, V4, and V5 header versions.
///
/// The height sign convention is: positive (bottom-up) for RLE formats,
/// negative (top-down) for everything else.
///
/// # Panics
///
/// Panics if `width` or `height` exceed `i32::MAX`, since the BMP header
/// stores dimensions as signed 32-bit integers and they cannot be
/// represented otherwise.
const fn make_info_header(
    width: u32,
    height: u32,
    bpp: BitsPerPixel,
    compression: Compression,
    image_size: u32,
    colors_used: u32,
) -> BitmapInfoHeader {
    assert!(width <= i32::MAX as u32);
    assert!(height <= i32::MAX as u32);

    #[allow(clippy::cast_possible_wrap)]
    BitmapInfoHeader {
        width: width as i32,
        height: match compression {
            Compression::Rle4 | Compression::Rle8 => height as i32,
            _ => -(height as i32),
        },
        planes: 1,
        bit_count: bpp,
        compression,
        image_size,
        x_resolution_ppm: 0,
        y_resolution_ppm: 0,
        colors_used,
        colors_important: 0,
    }
}

// ---------------------------------------------------------------------------
// Quantize helpers
// ---------------------------------------------------------------------------

/// Quantize the image to at most `max_colors` and return `(palette_rgbquad_entries, indices)`.
/// Palette entries are in BMP's BGRA ordering.
fn quantize_image(image: &DecodedImage, max_colors: usize) -> Result<(Vec<RgbQuad>, Vec<u8>), EncodeError> {
    let (palette, indices) = quantize::quantize(image.rgba(), max_colors)?;
    let color_table: Vec<RgbQuad> = palette
        .iter()
        .map(|c| RgbQuad {
            blue: c[2],
            green: c[1],
            red: c[0],
            reserved: 0,
        })
        .collect();
    Ok((color_table, indices))
}

// ---------------------------------------------------------------------------
// Build BitmapInfoData skeleton
// ---------------------------------------------------------------------------

#[expect(clippy::too_many_arguments, reason = "BMP assembly needs explicit per-field inputs")]
fn build_bmp_info(
    width: u32,
    height: u32,
    bpp: BitsPerPixel,
    compression: Compression,
    image_size: u32,
    color_masks: Option<RgbMasks>,
    color_table: Vec<RgbQuad>,
    bitmap_array: Vec<u8>,
) -> Result<Bmp, EncodeError> {
    let dib_size = BitmapInfoHeader::HEADER_SIZE;
    let masks_size: u32 = if color_masks.is_some() { 12 } else { 0 };
    let color_table_len = u32::try_from(color_table.len()).map_err(|_| EncodeError::ArithmeticOverflow)?;
    let color_table_bytes = color_table_len.checked_mul(4).ok_or(EncodeError::ArithmeticOverflow)?;

    let pixel_offset = checked_sum(&[FileHeader::SIZE, dib_size, masks_size, color_table_bytes])?;
    let file_size = checked_sum(&[pixel_offset, image_size])?;

    Ok(Bmp::Info(BitmapInfoData {
        file_header: make_file_header(file_size, pixel_offset),
        bmp_header: make_info_header(width, height, bpp, compression, image_size, color_table_len),
        color_masks,
        color_table,
        bitmap_array,
    }))
}

// ---------------------------------------------------------------------------
// Build BitmapCoreData skeleton
// ---------------------------------------------------------------------------

/// Core header only supports: Rgb1, Rgb4, Rgb8, Rgb24.
/// No compression, no 16/32-bpp, no RLE, no `BitFields`.
/// Dimensions are u16 (max 65535). Color table uses 3-byte `RgbTriple` entries.
/// Always bottom-up. Must always have exactly 2^bpp palette entries for indexed.
fn build_bmp_core(
    width: u32,
    height: u32,
    bpp: BitsPerPixel,
    image_size: u32,
    color_table: Vec<RgbTriple>,
    bitmap_array: Vec<u8>,
) -> Result<Bmp, EncodeError> {
    if width > u32::from(u16::MAX) || height > u32::from(u16::MAX) {
        return Err(EncodeError::CoreDimensionOverflow { width, height });
    }

    let dib_size = BitmapCoreHeader::HEADER_SIZE;
    let color_table_len = u32::try_from(color_table.len()).map_err(|_| EncodeError::ArithmeticOverflow)?;
    let color_table_bytes = color_table_len
        .checked_mul(3) // RgbTriple = 3 bytes
        .ok_or(EncodeError::ArithmeticOverflow)?;

    let pixel_offset = checked_sum(&[FileHeader::SIZE, dib_size, color_table_bytes])?;
    let file_size = checked_sum(&[pixel_offset, image_size])?;

    Ok(Bmp::Core(BitmapCoreData {
        file_header: make_file_header(file_size, pixel_offset),
        bmp_header: BitmapCoreHeader {
            width: u16::try_from(width).map_err(|_| EncodeError::ArithmeticOverflow)?,
            height: u16::try_from(height).map_err(|_| EncodeError::ArithmeticOverflow)?, // Core is always bottom-up (positive height)
            planes: 1,
            bit_count: bpp,
        },
        color_table,
        bitmap_array,
    }))
}

// ---------------------------------------------------------------------------
// Build BitmapV4Data skeleton
// ---------------------------------------------------------------------------

/// Metadata that can be preserved from a source BMP when saving as V4 or V5.
/// This is extracted from the original loaded file and threaded through transforms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceMetadata {
    /// The source image declares sRGB semantics.
    ///
    /// `intent` is only present when the source was a V5 bitmap.
    SRgb { intent: Option<u32> },
    /// The source image declares the legacy Windows color space.
    ///
    /// `intent` is only present when the source was a V5 bitmap.
    WindowsColorSpace { intent: Option<u32> },
    /// The source image carries calibrated RGB primaries and gamma values.
    ///
    /// `intent` is only present when the source was a V5 bitmap.
    CalibratedRgb {
        endpoints: CieXyzTriple,
        gamma: GammaTriple,
        intent: Option<u32>,
    },
    /// The source image is V5 and carries an embedded ICC profile payload.
    EmbeddedProfile { profile: Vec<u8>, intent: u32 },
    /// The source image is V5 and carries a linked ICC profile path payload.
    ///
    /// The payload is stored as raw bytes because BMP uses Windows ANSI here,
    /// not UTF-8.
    LinkedProfile { profile_path: Vec<u8>, intent: u32 },
}

impl SourceMetadata {
    /// Extract metadata from a loaded Bmp, if it has V4/V5 fields.
    #[must_use]
    pub fn from_bmp(bmp: &Bmp) -> Option<Self> {
        match bmp {
            Bmp::V4(v4) => match v4.bmp_header.cs_type {
                ColorSpaceType::SRgb => Some(Self::SRgb { intent: None }),
                ColorSpaceType::WindowsColorSpace => Some(Self::WindowsColorSpace { intent: None }),
                ColorSpaceType::CalibratedRgb => Some(Self::CalibratedRgb {
                    endpoints: v4.bmp_header.endpoints,
                    gamma: v4.bmp_header.gamma,
                    intent: None,
                }),
                ColorSpaceType::ProfileEmbedded | ColorSpaceType::ProfileLinked | ColorSpaceType::Other(_) => None,
            },
            Bmp::V5(v5) => match v5.bmp_header.v4.cs_type {
                ColorSpaceType::SRgb => Some(Self::SRgb {
                    intent: Some(v5.bmp_header.intent),
                }),
                ColorSpaceType::WindowsColorSpace => Some(Self::WindowsColorSpace {
                    intent: Some(v5.bmp_header.intent),
                }),
                ColorSpaceType::CalibratedRgb => Some(Self::CalibratedRgb {
                    endpoints: v5.bmp_header.v4.endpoints,
                    gamma: v5.bmp_header.v4.gamma,
                    intent: Some(v5.bmp_header.intent),
                }),
                ColorSpaceType::ProfileEmbedded => Some(Self::EmbeddedProfile {
                    profile: v5.icc_profile.clone().unwrap_or_default(),
                    intent: v5.bmp_header.intent,
                }),
                ColorSpaceType::ProfileLinked => Some(Self::LinkedProfile {
                    profile_path: v5.icc_profile.clone().unwrap_or_default(),
                    intent: v5.bmp_header.intent,
                }),
                ColorSpaceType::Other(_) => None,
            },
            _ => None,
        }
    }

    const fn v4_header_fields(&self) -> (ColorSpaceType, CieXyzTriple, GammaTriple) {
        match self {
            Self::SRgb { .. } => (ColorSpaceType::SRgb, default_zeroed_endpoints(), default_zeroed_gamma()),
            Self::WindowsColorSpace { .. } => (
                ColorSpaceType::WindowsColorSpace,
                default_zeroed_endpoints(),
                default_zeroed_gamma(),
            ),
            Self::CalibratedRgb { endpoints, gamma, .. } => (ColorSpaceType::CalibratedRgb, *endpoints, *gamma),
            Self::EmbeddedProfile { .. } | Self::LinkedProfile { .. } => {
                (ColorSpaceType::SRgb, default_zeroed_endpoints(), default_zeroed_gamma())
            }
        }
    }

    fn v5_header_fields(&self) -> (ColorSpaceType, CieXyzTriple, GammaTriple, u32, Option<Vec<u8>>) {
        match self {
            Self::SRgb { intent } => (
                ColorSpaceType::SRgb,
                default_zeroed_endpoints(),
                default_zeroed_gamma(),
                intent.unwrap_or(0),
                None,
            ),
            Self::WindowsColorSpace { intent } => (
                ColorSpaceType::WindowsColorSpace,
                default_zeroed_endpoints(),
                default_zeroed_gamma(),
                intent.unwrap_or(0),
                None,
            ),
            Self::CalibratedRgb {
                endpoints,
                gamma,
                intent,
            } => (
                ColorSpaceType::CalibratedRgb,
                *endpoints,
                *gamma,
                intent.unwrap_or(0),
                None,
            ),
            Self::EmbeddedProfile { profile, intent } => (
                ColorSpaceType::ProfileEmbedded,
                default_zeroed_endpoints(),
                default_zeroed_gamma(),
                *intent,
                Some(profile.clone()),
            ),
            Self::LinkedProfile { profile_path, intent } => (
                ColorSpaceType::ProfileLinked,
                default_zeroed_endpoints(),
                default_zeroed_gamma(),
                *intent,
                Some(profile_path.clone()),
            ),
        }
    }
}

const fn default_zeroed_endpoints() -> CieXyzTriple {
    use crate::raw::FixedPoint2Dot30;
    let zero = crate::raw::CieXyz {
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

const fn default_zeroed_gamma() -> GammaTriple {
    use crate::raw::FixedPoint16Dot16;
    GammaTriple {
        red: FixedPoint16Dot16::from_raw(0),
        green: FixedPoint16Dot16::from_raw(0),
        blue: FixedPoint16Dot16::from_raw(0),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "BMP V4 assembly needs explicit per-field inputs"
)]
fn build_bmp_v4(
    width: u32,
    height: u32,
    bpp: BitsPerPixel,
    compression: Compression,
    image_size: u32,
    rgba_masks: RgbaMasks,
    color_table: Vec<RgbQuad>,
    bitmap_array: Vec<u8>,
    source: Option<&SourceMetadata>,
) -> Result<Bmp, EncodeError> {
    let dib_size = BitmapV4Header::HEADER_SIZE;
    // V4 does NOT have separate color masks - they are embedded in the header
    let color_table_len = u32::try_from(color_table.len()).map_err(|_| EncodeError::ArithmeticOverflow)?;
    let color_table_bytes = color_table_len.checked_mul(4).ok_or(EncodeError::ArithmeticOverflow)?;

    let pixel_offset = checked_sum(&[FileHeader::SIZE, dib_size, color_table_bytes])?;
    let file_size = checked_sum(&[pixel_offset, image_size])?;

    let (cs_type, endpoints, gamma) = source.map_or_else(
        || (ColorSpaceType::SRgb, default_zeroed_endpoints(), default_zeroed_gamma()),
        SourceMetadata::v4_header_fields,
    );

    Ok(Bmp::V4(BitmapV4Data {
        file_header: make_file_header(file_size, pixel_offset),
        bmp_header: BitmapV4Header {
            info: make_info_header(width, height, bpp, compression, image_size, color_table_len),
            masks: rgba_masks,
            cs_type,
            endpoints,
            gamma,
        },
        color_table,
        bitmap_array,
    }))
}

// ---------------------------------------------------------------------------
// Build BitmapV5Data skeleton
// ---------------------------------------------------------------------------

#[expect(
    clippy::too_many_arguments,
    reason = "BMP V5 assembly needs explicit per-field inputs"
)]
fn build_bmp_v5(
    width: u32,
    height: u32,
    bpp: BitsPerPixel,
    compression: Compression,
    image_size: u32,
    rgba_masks: RgbaMasks,
    color_table: Vec<RgbQuad>,
    bitmap_array: Vec<u8>,
    source: Option<&SourceMetadata>,
) -> Result<Bmp, EncodeError> {
    let dib_size = BitmapV5Header::HEADER_SIZE;
    let color_table_len = u32::try_from(color_table.len()).map_err(|_| EncodeError::ArithmeticOverflow)?;
    let color_table_bytes = color_table_len.checked_mul(4).ok_or(EncodeError::ArithmeticOverflow)?;

    let pixel_offset = checked_sum(&[FileHeader::SIZE, dib_size, color_table_bytes])?;
    let pixel_end = checked_sum(&[pixel_offset, image_size])?;

    let (cs_type, endpoints, gamma, intent, icc_profile) = source.map_or_else(
        || {
            (
                ColorSpaceType::SRgb,
                default_zeroed_endpoints(),
                default_zeroed_gamma(),
                0,
                None,
            )
        },
        SourceMetadata::v5_header_fields,
    );

    // profile_data is offset from the beginning of the DIB header (i.e. from the start
    // of the DIB header size field, NOT from the start of the file).
    // absolute offset = FileHeader::SIZE + profile_data.
    // We place the ICC profile right after the pixel data.
    let (profile_data, profile_size, file_size) = if let Some(ref profile) = icc_profile {
        let profile_size = u32::try_from(profile.len()).map_err(|_| EncodeError::ArithmeticOverflow)?;
        let pd = pixel_end
            .checked_sub(FileHeader::SIZE)
            .ok_or(EncodeError::ArithmeticOverflow)?;
        let fs = checked_sum(&[pixel_end, profile_size])?;
        (pd, profile_size, fs)
    } else {
        (0, 0, pixel_end)
    };

    Ok(Bmp::V5(BitmapV5Data {
        file_header: make_file_header(file_size, pixel_offset),
        bmp_header: BitmapV5Header {
            v4: BitmapV4Header {
                info: make_info_header(width, height, bpp, compression, image_size, color_table_len),
                masks: rgba_masks,
                cs_type,
                endpoints,
                gamma,
            },
            intent,
            profile_data,
            profile_size,
            reserved: [0; 4],
        },
        color_table,
        bitmap_array,
        icc_profile,
    }))
}

// ===========================================================================
// Individual encoders
// ===========================================================================

// ---------------------------------------------------------------------------
// BI_RGB / BI_BITFIELDS 32-bpp
// ---------------------------------------------------------------------------

fn encode_32bpp(image: &DecodedImage, compression: Compression, masks: Option<RgbMasks>) -> Result<Bmp, EncodeError> {
    let pixel_bytes = image.rgba().len();
    let image_size = u32::try_from(pixel_bytes).map_err(|_| EncodeError::ArithmeticOverflow)?;

    let mut bmp_pixels = Vec::with_capacity(pixel_bytes);
    for px in image.pixels() {
        // 32bpp stores B, G, R, reserved (both BI_RGB and BI_BITFIELDS with RGB888 masks)
        bmp_pixels.extend_from_slice(&[px[2], px[1], px[0], 0]);
    }

    build_bmp_info(
        image.width(),
        image.height(),
        BitsPerPixel::Bpp32,
        compression,
        image_size,
        masks,
        Vec::new(),
        bmp_pixels,
    )
}

// ---------------------------------------------------------------------------
// BI_RGB 24-bpp
// ---------------------------------------------------------------------------

fn encode_rgb24(image: &DecodedImage) -> Result<Bmp, EncodeError> {
    let w = image.width() as usize;
    let h = image.height() as usize;
    let rgba = image.rgba();
    let stride = row_stride(w, 24)?;
    let image_size = u32::try_from(stride * h).map_err(|_| EncodeError::ArithmeticOverflow)?;

    let mut bmp_pixels = vec![0u8; stride * h];
    for y in 0..h {
        let row_start = y * stride;
        for x in 0..w {
            let src = (y * w + x) * 4;
            let dst = row_start + x * 3;
            bmp_pixels[dst] = rgba[src + 2]; // B
            bmp_pixels[dst + 1] = rgba[src + 1]; // G
            bmp_pixels[dst + 2] = rgba[src]; // R
        }
    }

    build_bmp_info(
        image.width(),
        image.height(),
        BitsPerPixel::Bpp24,
        Compression::Rgb,
        image_size,
        None,
        Vec::new(),
        bmp_pixels,
    )
}

// ---------------------------------------------------------------------------
// BI_RGB 16-bpp (RGB555)
// ---------------------------------------------------------------------------

fn encode_rgb16(image: &DecodedImage) -> Result<Bmp, EncodeError> {
    let w = image.width() as usize;
    let h = image.height() as usize;
    let rgba = image.rgba();
    let stride = row_stride(w, 16)?;
    let image_size = u32::try_from(stride * h).map_err(|_| EncodeError::ArithmeticOverflow)?;

    let mut bmp_pixels = vec![0u8; stride * h];
    for y in 0..h {
        let row_start = y * stride;
        for x in 0..w {
            let src = (y * w + x) * 4;
            let r5 = (u16::from(rgba[src]) * 31 + 127) / 255;
            let g5 = (u16::from(rgba[src + 1]) * 31 + 127) / 255;
            let b5 = (u16::from(rgba[src + 2]) * 31 + 127) / 255;
            let px16: u16 = (r5 << 10) | (g5 << 5) | b5;
            let dst = row_start + x * 2;
            bmp_pixels[dst..dst + 2].copy_from_slice(&px16.to_le_bytes());
        }
    }

    build_bmp_info(
        image.width(),
        image.height(),
        BitsPerPixel::Bpp16,
        Compression::Rgb,
        image_size,
        None,
        Vec::new(),
        bmp_pixels,
    )
}

// ---------------------------------------------------------------------------
// Indexed BI_RGB (1, 4, 8 bpp)
// ---------------------------------------------------------------------------

fn encode_indexed_rgb(image: &DecodedImage, bpp: BitsPerPixel) -> Result<Bmp, EncodeError> {
    let max_colors: usize = match bpp {
        BitsPerPixel::Bpp1 => 2,
        BitsPerPixel::Bpp4 => 16,
        BitsPerPixel::Bpp8 => 256,
        _ => unreachable!(),
    };

    let (color_table, indices) = quantize_image(image, max_colors)?;

    let w = image.width() as usize;
    let h = image.height() as usize;
    let bits = bpp.bit_count();
    let stride = row_stride(w, bits)?;
    let image_size = u32::try_from(stride * h).map_err(|_| EncodeError::ArithmeticOverflow)?;

    let mut bmp_pixels = vec![0u8; stride * h];
    for y in 0..h {
        let row_start = y * stride;
        for x in 0..w {
            let idx = indices[y * w + x];
            match bpp {
                BitsPerPixel::Bpp8 => {
                    bmp_pixels[row_start + x] = idx;
                }
                BitsPerPixel::Bpp4 => {
                    let byte_pos = row_start + x / 2;
                    if x % 2 == 0 {
                        bmp_pixels[byte_pos] |= (idx & 0x0f) << 4;
                    } else {
                        bmp_pixels[byte_pos] |= idx & 0x0f;
                    }
                }
                BitsPerPixel::Bpp1 => {
                    let byte_pos = row_start + x / 8;
                    let bit = 7 - (x % 8);
                    if idx & 1 != 0 {
                        bmp_pixels[byte_pos] |= 1 << bit;
                    }
                }
                _ => unreachable!(),
            }
        }
    }

    build_bmp_info(
        image.width(),
        image.height(),
        bpp,
        Compression::Rgb,
        image_size,
        None,
        color_table,
        bmp_pixels,
    )
}

// ---------------------------------------------------------------------------
// BI_RLE8
// ---------------------------------------------------------------------------

fn encode_rle8(image: &DecodedImage) -> Result<Bmp, EncodeError> {
    let (color_table, indices) = quantize_image(image, 256)?;

    let w = image.width() as usize;
    let h = image.height() as usize;

    // RLE is bottom-up, so we iterate rows bottom-to-top.
    let mut rle_data: Vec<u8> = Vec::new();

    for y in (0..h).rev() {
        let row_start = y * w;
        let row = &indices[row_start..row_start + w];

        let mut x = 0;
        while x < w {
            // Count how many consecutive identical values follow.
            let val = row[x];
            let mut run_len = 1usize;
            while x + run_len < w && row[x + run_len] == val && run_len < 255 {
                run_len += 1;
            }

            if run_len >= 3 {
                // Encoded run
                #[allow(clippy::cast_possible_truncation)]
                rle_data.push(run_len as u8);
                rle_data.push(val);
                x += run_len;
            } else {
                // Try to build an absolute run of non-repeating values.
                let abs_start = x;
                let mut abs_len = 0usize;
                while x + abs_len < w && abs_len < 255 {
                    // Look ahead: if next 3+ bytes are the same, break.
                    let cur = row[x + abs_len];
                    let same_ahead = (1..3)
                        .take_while(|&d| x + abs_len + d < w && row[x + abs_len + d] == cur)
                        .count()
                        + 1;
                    if same_ahead >= 3 && abs_len > 0 {
                        break;
                    }
                    abs_len += 1;
                }

                if abs_len < 3 {
                    // Too short for absolute mode; emit as short encoded runs.
                    for i in 0..abs_len {
                        rle_data.push(1);
                        rle_data.push(row[abs_start + i]);
                    }
                } else {
                    // Absolute mode escape: 0x00, count, then count bytes (word-aligned).
                    rle_data.push(0);
                    #[allow(clippy::cast_possible_truncation)]
                    rle_data.push(abs_len as u8);
                    for i in 0..abs_len {
                        rle_data.push(row[abs_start + i]);
                    }
                    if abs_len % 2 != 0 {
                        rle_data.push(0); // pad to word boundary
                    }
                }
                x += abs_len;
            }
        }

        // End-of-line
        if y > 0 {
            rle_data.push(0);
            rle_data.push(0);
        }
    }

    // End-of-bitmap
    rle_data.push(0);
    rle_data.push(1);

    let image_size = u32::try_from(rle_data.len()).map_err(|_| EncodeError::ArithmeticOverflow)?;

    build_bmp_info(
        image.width(),
        image.height(),
        BitsPerPixel::Bpp8,
        Compression::Rle8,
        image_size,
        None,
        color_table,
        rle_data,
    )
}

// ---------------------------------------------------------------------------
// BI_RLE4
// ---------------------------------------------------------------------------

fn encode_rle4(image: &DecodedImage) -> Result<Bmp, EncodeError> {
    let (color_table, indices) = quantize_image(image, 16)?;

    let w = image.width() as usize;
    let h = image.height() as usize;

    let mut rle_data: Vec<u8> = Vec::new();

    // RLE4 is bottom-up
    for y in (0..h).rev() {
        let row_start = y * w;
        let row = &indices[row_start..row_start + w];

        let mut x = 0;
        while x < w {
            // In RLE4, an encoded run stores two nibbles in the value byte,
            // alternating between the high and low nibble for `count` pixels.
            // The simplest approach: detect runs of a single repeated color.
            let val = row[x];
            let mut run_len = 1usize;
            while x + run_len < w && row[x + run_len] == val && run_len < 255 {
                run_len += 1;
            }

            if run_len >= 3 {
                // Encoded run: pack the same nibble into both halves.
                let packed = (val << 4) | val;
                #[allow(clippy::cast_possible_truncation)]
                rle_data.push(run_len as u8);
                rle_data.push(packed);
                x += run_len;
            } else {
                // Absolute mode
                let abs_start = x;
                let mut abs_len = 0usize;
                while x + abs_len < w && abs_len < 255 {
                    let cur = row[x + abs_len];
                    let same_ahead = (1..3)
                        .take_while(|&d| x + abs_len + d < w && row[x + abs_len + d] == cur)
                        .count()
                        + 1;
                    if same_ahead >= 3 && abs_len > 0 {
                        break;
                    }
                    abs_len += 1;
                }

                if abs_len < 3 {
                    // Short runs
                    for i in 0..abs_len {
                        let v = row[abs_start + i];
                        rle_data.push(1);
                        rle_data.push((v << 4) | v);
                    }
                } else {
                    // Absolute escape
                    rle_data.push(0);
                    #[allow(clippy::cast_possible_truncation)]
                    rle_data.push(abs_len as u8);
                    let bytes_needed = abs_len.div_ceil(2);
                    for b in 0..bytes_needed {
                        let hi = row[abs_start + b * 2] & 0x0f;
                        let lo = if abs_start + b * 2 + 1 < abs_start + abs_len {
                            row[abs_start + b * 2 + 1] & 0x0f
                        } else {
                            0
                        };
                        rle_data.push((hi << 4) | lo);
                    }
                    if bytes_needed % 2 != 0 {
                        rle_data.push(0); // word-align
                    }
                }
                x += abs_len;
            }
        }

        // End-of-line
        if y > 0 {
            rle_data.push(0);
            rle_data.push(0);
        }
    }

    // End-of-bitmap
    rle_data.push(0);
    rle_data.push(1);

    let image_size = u32::try_from(rle_data.len()).map_err(|_| EncodeError::ArithmeticOverflow)?;

    build_bmp_info(
        image.width(),
        image.height(),
        BitsPerPixel::Bpp4,
        Compression::Rle4,
        image_size,
        None,
        color_table,
        rle_data,
    )
}

// ---------------------------------------------------------------------------
// BI_BITFIELDS 16-bpp  (RGB565 or RGB555)
// ---------------------------------------------------------------------------

fn encode_bitfields16(image: &DecodedImage, masks: RgbMasks) -> Result<Bmp, EncodeError> {
    let w = image.width() as usize;
    let h = image.height() as usize;
    let rgba = image.rgba();
    let stride = row_stride(w, 16)?;
    let image_size = u32::try_from(stride * h).map_err(|_| EncodeError::ArithmeticOverflow)?;

    // Pre-compute shifts and widths from the masks.
    let r_shift = masks.red_mask.trailing_zeros();
    let r_bits = masks.red_mask.count_ones();
    let g_shift = masks.green_mask.trailing_zeros();
    let g_bits = masks.green_mask.count_ones();
    let b_shift = masks.blue_mask.trailing_zeros();
    let b_bits = masks.blue_mask.count_ones();

    let r_max = (1u16 << r_bits) - 1;
    let g_max = (1u16 << g_bits) - 1;
    let b_max = (1u16 << b_bits) - 1;

    let mut bmp_pixels = vec![0u8; stride * h];
    for y in 0..h {
        let row_start = y * stride;
        for x in 0..w {
            let src = (y * w + x) * 4;
            let r = u16::from(rgba[src]);
            let g = u16::from(rgba[src + 1]);
            let b = u16::from(rgba[src + 2]);

            let rv = (r * r_max + 127) / 255;
            let gv = (g * g_max + 127) / 255;
            let bv = (b * b_max + 127) / 255;

            let px16: u16 = (rv << r_shift) | (gv << g_shift) | (bv << b_shift);
            let dst = row_start + x * 2;
            bmp_pixels[dst..dst + 2].copy_from_slice(&px16.to_le_bytes());
        }
    }

    build_bmp_info(
        image.width(),
        image.height(),
        BitsPerPixel::Bpp16,
        Compression::BitFields,
        image_size,
        Some(masks),
        Vec::new(),
        bmp_pixels,
    )
}

// ===========================================================================
// Public API
// ===========================================================================

/// Convert a `Bmp::Info` (as produced by all individual encoders) to the
/// target header version. For `Info`, this is a no-op. For other versions,
/// the pixel data and color table are moved into the appropriate struct.
fn wrap_with_header(
    bitmap: Bmp,
    header_version: SaveHeaderVersion,
    source: Option<&SourceMetadata>,
) -> Result<Bmp, EncodeError> {
    match header_version {
        SaveHeaderVersion::Info => Ok(bitmap),
        SaveHeaderVersion::Core => {
            // Extract the Info data
            let Bmp::Info(info) = bitmap else {
                unreachable!("individual encoders always produce Bmp::Info")
            };

            // Core needs RgbTriple color table, bottom-up rows, and u16 dimensions
            let width = u32::try_from(info.bmp_header.width).map_err(|_| EncodeError::ArithmeticOverflow)?;
            let height = info.bmp_header.height.unsigned_abs();

            let bits_per_pixel = info.bmp_header.bit_count;

            // Convert RgbQuad color table to RgbTriple
            // Core always requires exactly 2^bpp entries for indexed formats
            let required_entries: usize = match bits_per_pixel {
                BitsPerPixel::Bpp1 => 2,
                BitsPerPixel::Bpp4 => 16,
                BitsPerPixel::Bpp8 => 256,
                _ => 0,
            };

            let mut core_color_table: Vec<RgbTriple> = info
                .color_table
                .iter()
                .map(|q| RgbTriple {
                    blue: q.blue,
                    green: q.green,
                    red: q.red,
                })
                .collect();

            // Pad to required size if needed (Core requires exactly 2^bpp entries)
            while core_color_table.len() < required_entries {
                core_color_table.push(RgbTriple {
                    blue: 0,
                    green: 0,
                    red: 0,
                });
            }

            // Core is always bottom-up. If the Info encoder produced top-down
            // (negative height), we need to flip the pixel rows.
            let bitmap_array = if info.bmp_header.height < 0 {
                // top-down -> need to flip to bottom-up
                let bits = bits_per_pixel.bit_count();
                let stride = row_stride(width as usize, bits)?;
                let h = height as usize;
                let mut flipped = vec![0u8; info.bitmap_array.len()];
                for y in 0..h {
                    let src_start = y * stride;
                    let dst_start = (h - 1 - y) * stride;
                    flipped[dst_start..dst_start + stride]
                        .copy_from_slice(&info.bitmap_array[src_start..src_start + stride]);
                }
                flipped
            } else {
                // already bottom-up (RLE)
                info.bitmap_array
            };

            let image_size = u32::try_from(bitmap_array.len()).map_err(|_| EncodeError::ArithmeticOverflow)?;

            build_bmp_core(
                width,
                height,
                bits_per_pixel,
                image_size,
                core_color_table,
                bitmap_array,
            )
        }
        SaveHeaderVersion::V4 | SaveHeaderVersion::V5 => {
            let Bmp::Info(info) = bitmap else {
                unreachable!("individual encoders always produce Bmp::Info")
            };

            let rgba_masks = info.color_masks.map_or(
                RgbaMasks {
                    red_mask: 0,
                    green_mask: 0,
                    blue_mask: 0,
                    alpha_mask: 0,
                },
                RgbaMasks::from,
            );

            let width = u32::try_from(info.bmp_header.width).map_err(|_| EncodeError::ArithmeticOverflow)?;
            let height = info.bmp_header.height.unsigned_abs();
            let bits_per_pixel = info.bmp_header.bit_count;
            let compression = info.bmp_header.compression;
            let img_size = info.bmp_header.image_size;

            match header_version {
                SaveHeaderVersion::V4 => build_bmp_v4(
                    width,
                    height,
                    bits_per_pixel,
                    compression,
                    img_size,
                    rgba_masks,
                    info.color_table,
                    info.bitmap_array,
                    source,
                ),
                SaveHeaderVersion::V5 => build_bmp_v5(
                    width,
                    height,
                    bits_per_pixel,
                    compression,
                    img_size,
                    rgba_masks,
                    info.color_table,
                    info.bitmap_array,
                    source,
                ),
                _ => unreachable!(),
            }
        }
    }
}

/// Encodes a decoded RGBA image into a BMP using the default format
/// (32-bit uncompressed RGB). This preserves the original API.
///
/// # Errors
/// Returns [`EncodeError`] if BMP assembly/write metadata overflows occur.
pub fn encode_rgba_to_bmp(image: &DecodedImage) -> Result<Bmp, EncodeError> {
    encode_rgba_to_bmp_with_format(image, SaveFormat::Rgb32)
}

/// Encodes a decoded RGBA image into a BMP using the specified [`SaveFormat`].
///
/// This always produces a `Bmp::Info` (V3) header. For other header versions,
/// use [`encode_rgba_to_bmp_ext`].
///
/// # Errors
/// Returns [`EncodeError`] if the chosen format cannot be encoded safely.
pub fn encode_rgba_to_bmp_with_format(image: &DecodedImage, format: SaveFormat) -> Result<Bmp, EncodeError> {
    match format {
        SaveFormat::Rgb32 => encode_32bpp(image, Compression::Rgb, None),
        SaveFormat::Rgb24 => encode_rgb24(image),
        SaveFormat::Rgb16 => encode_rgb16(image),
        SaveFormat::Rgb8 => encode_indexed_rgb(image, BitsPerPixel::Bpp8),
        SaveFormat::Rgb4 => encode_indexed_rgb(image, BitsPerPixel::Bpp4),
        SaveFormat::Rgb1 => encode_indexed_rgb(image, BitsPerPixel::Bpp1),
        SaveFormat::Rle8 => encode_rle8(image),
        SaveFormat::Rle4 => encode_rle4(image),
        SaveFormat::BitFields16Rgb565 => encode_bitfields16(image, RgbMasks::rgb565()),
        SaveFormat::BitFields16Rgb555 => encode_bitfields16(image, RgbMasks::rgb555()),
        SaveFormat::BitFields32 => encode_32bpp(image, Compression::BitFields, Some(RgbMasks::rgb888())),
    }
}

/// Encodes a decoded RGBA image into a BMP using the specified pixel format
/// and header version, optionally preserving metadata from a source BMP.
///
/// # Errors
/// Returns [`EncodeError`] if the format is incompatible with the requested
/// header version or encoding fails.
pub fn encode_rgba_to_bmp_ext(
    image: &DecodedImage,
    format: SaveFormat,
    header_version: SaveHeaderVersion,
    source: Option<&SourceMetadata>,
) -> Result<Bmp, EncodeError> {
    if !header_version.is_compatible(format) {
        return Err(EncodeError::IncompatibleHeaderFormat {
            format,
            header: header_version,
        });
    }

    // First encode using the Info-based encoders (which is what all individual
    // encoders produce), then wrap with the target header version.
    let info_bmp = encode_rgba_to_bmp_with_format(image, format)?;
    wrap_with_header(info_bmp, header_version, source)
}

/// Saves a decoded RGBA image to a BMP file using the default format (32-bit
/// uncompressed RGB). This preserves the original API.
///
/// # Errors
/// Returns [`EncodeError`] if encoding or file I/O fails.
pub fn save_bmp(path: &Path, image: &DecodedImage) -> Result<(), EncodeError> {
    save_bmp_with_format(path, image, SaveFormat::Rgb32)
}

/// Saves a decoded RGBA image to a BMP file using the specified [`SaveFormat`].
///
/// # Errors
/// Returns [`EncodeError`] if encoding or file I/O fails.
pub fn save_bmp_with_format(path: &Path, image: &DecodedImage, format: SaveFormat) -> Result<(), EncodeError> {
    let bmp = encode_rgba_to_bmp_with_format(image, format)?;
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    bmp.write_unchecked(&mut writer)?;
    Ok(())
}

/// Saves a decoded RGBA image to a BMP file using the specified pixel format,
/// header version, and optional source metadata.
///
/// # Errors
/// Returns [`EncodeError`] if compatibility checks fail, encoding fails, or file
/// I/O fails.
pub fn save_bmp_ext(
    path: &Path,
    image: &DecodedImage,
    format: SaveFormat,
    header_version: SaveHeaderVersion,
    source: Option<&SourceMetadata>,
) -> Result<(), EncodeError> {
    let bmp = encode_rgba_to_bmp_ext(image, format, header_version, source)?;
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    bmp.write_unchecked(&mut writer)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_image() -> DecodedImage {
        DecodedImage::new(1, 1, vec![10, 20, 30, 255]).unwrap()
    }

    #[test]
    fn v4_downgrades_profile_metadata_to_srgb() {
        let source = SourceMetadata::EmbeddedProfile {
            profile: vec![1, 2, 3, 4],
            intent: 7,
        };

        let bmp =
            encode_rgba_to_bmp_ext(&tiny_image(), SaveFormat::Rgb32, SaveHeaderVersion::V4, Some(&source)).unwrap();
        let Bmp::V4(data) = bmp else {
            panic!("expected V4 bitmap");
        };

        assert_eq!(data.bmp_header.cs_type, ColorSpaceType::SRgb);
        assert_eq!(data.bmp_header.endpoints, default_zeroed_endpoints());
        assert_eq!(data.bmp_header.gamma, default_zeroed_gamma());
    }

    #[test]
    fn v5_preserves_linked_profile_metadata() {
        let profile_path = vec![b'C', b':', b'\\', b'p', b'.', b'i', b'c', b'm', 0];
        let source = SourceMetadata::LinkedProfile {
            profile_path: profile_path.clone(),
            intent: 3,
        };

        let bmp =
            encode_rgba_to_bmp_ext(&tiny_image(), SaveFormat::Rgb32, SaveHeaderVersion::V5, Some(&source)).unwrap();
        let Bmp::V5(data) = bmp else {
            panic!("expected V5 bitmap");
        };

        assert_eq!(data.bmp_header.v4.cs_type, ColorSpaceType::ProfileLinked);
        assert_eq!(data.bmp_header.intent, 3);
        assert_eq!(data.icc_profile, Some(profile_path));
        assert_eq!(SourceMetadata::from_bmp(&Bmp::V5(data)), Some(source));
    }

    #[test]
    fn encoder_uses_canonical_pixel_offsets() {
        let info = encode_rgba_to_bmp_ext(&tiny_image(), SaveFormat::Rgb24, SaveHeaderVersion::Info, None).unwrap();
        let Bmp::Info(info) = info else {
            panic!("expected Info bitmap");
        };
        assert_eq!(
            info.file_header.pixel_data_offset,
            FileHeader::SIZE + BitmapInfoHeader::HEADER_SIZE
        );

        let core = encode_rgba_to_bmp_ext(&tiny_image(), SaveFormat::Rgb24, SaveHeaderVersion::Core, None).unwrap();
        let Bmp::Core(core) = core else {
            panic!("expected Core bitmap");
        };
        assert_eq!(
            core.file_header.pixel_data_offset,
            FileHeader::SIZE + BitmapCoreHeader::HEADER_SIZE
        );

        let v4 = encode_rgba_to_bmp_ext(&tiny_image(), SaveFormat::Rgb24, SaveHeaderVersion::V4, None).unwrap();
        let Bmp::V4(v4) = v4 else {
            panic!("expected V4 bitmap");
        };
        assert_eq!(
            v4.file_header.pixel_data_offset,
            FileHeader::SIZE + BitmapV4Header::HEADER_SIZE
        );

        let v5 = encode_rgba_to_bmp_ext(&tiny_image(), SaveFormat::Rgb24, SaveHeaderVersion::V5, None).unwrap();
        let Bmp::V5(v5) = v5 else {
            panic!("expected V5 bitmap");
        };
        assert_eq!(
            v5.file_header.pixel_data_offset,
            FileHeader::SIZE + BitmapV5Header::HEADER_SIZE
        );
    }

    #[test]
    fn v5_profile_data_offset_is_relative_to_dib_start() {
        let source = SourceMetadata::EmbeddedProfile {
            profile: vec![1, 2, 3, 4],
            intent: 7,
        };

        let bmp =
            encode_rgba_to_bmp_ext(&tiny_image(), SaveFormat::Rgb32, SaveHeaderVersion::V5, Some(&source)).unwrap();
        let Bmp::V5(data) = bmp else {
            panic!("expected V5 bitmap");
        };

        assert_eq!(
            data.file_header.pixel_data_offset,
            FileHeader::SIZE + BitmapV5Header::HEADER_SIZE
        );
        assert_eq!(
            data.bmp_header.profile_data,
            BitmapV5Header::HEADER_SIZE + data.bmp_header.v4.info.image_size
        );
    }

    #[test]
    fn from_bmp_preserves_supported_bitfields_layouts() {
        let bmp_565 = encode_rgba_to_bmp_with_format(&tiny_image(), SaveFormat::BitFields16Rgb565).unwrap();
        assert_eq!(SaveFormat::from_bmp(&bmp_565), SaveFormat::BitFields16Rgb565);

        let bmp_555 = encode_rgba_to_bmp_with_format(&tiny_image(), SaveFormat::BitFields16Rgb555).unwrap();
        assert_eq!(SaveFormat::from_bmp(&bmp_555), SaveFormat::BitFields16Rgb555);

        let bmp_888 = encode_rgba_to_bmp_with_format(&tiny_image(), SaveFormat::BitFields32).unwrap();
        assert_eq!(SaveFormat::from_bmp(&bmp_888), SaveFormat::BitFields32);
    }

    #[test]
    fn from_bmp_falls_back_for_exotic_bitfields_layouts() {
        let mut bmp_16 = encode_rgba_to_bmp_with_format(&tiny_image(), SaveFormat::BitFields16Rgb555).unwrap();
        let Bmp::Info(info_16) = &mut bmp_16 else {
            panic!("expected Info bitmap");
        };
        info_16.color_masks = Some(RgbMasks {
            red_mask: 0x0000_0F00,
            green_mask: 0x0000_00F0,
            blue_mask: 0x0000_000F,
        });
        assert_eq!(SaveFormat::from_bmp(&bmp_16), SaveFormat::Rgb32);

        let mut bmp_32 = encode_rgba_to_bmp_with_format(&tiny_image(), SaveFormat::BitFields32).unwrap();
        let Bmp::Info(info_32) = &mut bmp_32 else {
            panic!("expected Info bitmap");
        };
        info_32.color_masks = Some(RgbMasks {
            red_mask: 0x0000_00FF,
            green_mask: 0x0000_FF00,
            blue_mask: 0x00FF_0000,
        });
        assert_eq!(SaveFormat::from_bmp(&bmp_32), SaveFormat::Rgb32);
    }

    #[test]
    fn from_bmp_falls_back_for_bitfields_with_alpha_masks() {
        let mut bmp_16 = encode_rgba_to_bmp_ext(
            &tiny_image(),
            SaveFormat::BitFields16Rgb555,
            SaveHeaderVersion::V4,
            None,
        )
        .unwrap();
        let Bmp::V4(v4_16) = &mut bmp_16 else {
            panic!("expected V4 bitmap");
        };
        v4_16.bmp_header.masks.alpha_mask = 0x0000_8000;
        assert_eq!(SaveFormat::from_bmp(&bmp_16), SaveFormat::Rgb32);

        let mut bmp_32 =
            encode_rgba_to_bmp_ext(&tiny_image(), SaveFormat::BitFields32, SaveHeaderVersion::V4, None).unwrap();
        let Bmp::V4(v4_32) = &mut bmp_32 else {
            panic!("expected V4 bitmap");
        };
        v4_32.bmp_header.masks.alpha_mask = 0xFF00_0000;
        assert_eq!(SaveFormat::from_bmp(&bmp_32), SaveFormat::Rgb32);
    }
}
