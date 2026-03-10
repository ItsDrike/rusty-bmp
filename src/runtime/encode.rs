use std::{fs::File, io::BufWriter, path::Path};

use thiserror::Error;

use crate::{
    raw::{BitmapInfoData, BitmapInfoHeader, BitsPerPixel, Bmp, Compression, FileHeader, RgbMasks, RgbQuad},
    runtime::{decode::DecodedImage, quantize},
};

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("invalid dimensions for encoding: {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },

    #[error("pixel buffer size mismatch: expected {expected} bytes, found {actual}")]
    PixelBufferSizeMismatch { expected: usize, actual: usize },

    #[error("arithmetic overflow while preparing BMP")]
    ArithmeticOverflow,

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Bmp(#[from] crate::raw::BmpError),
}

/// Selects the BMP pixel format used when saving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveFormat {
    /// BI_RGB, 1 bit per pixel (monochrome), palette of 2 colors.
    Rgb1,
    /// BI_RGB, 4 bits per pixel, palette of up to 16 colors.
    Rgb4,
    /// BI_RGB, 8 bits per pixel, palette of up to 256 colors.
    Rgb8,
    /// BI_RGB, 16 bits per pixel, RGB555 (no palette).
    Rgb16,
    /// BI_RGB, 24 bits per pixel (no palette).
    Rgb24,
    /// BI_RGB, 32 bits per pixel (no palette). This is the default.
    Rgb32,
    /// BI_RLE8, 8 bits per pixel, run-length encoded.
    Rle8,
    /// BI_RLE4, 4 bits per pixel, run-length encoded.
    Rle4,
    /// BI_BITFIELDS, 16 bits per pixel with RGB565 masks.
    BitFields16Rgb565,
    /// BI_BITFIELDS, 16 bits per pixel with RGB555 masks (same layout as Rgb16 but stored with explicit masks).
    BitFields16Rgb555,
    /// BI_BITFIELDS, 32 bits per pixel with standard RGB888 masks (8 bits per channel, no alpha).
    BitFields32,
}

impl SaveFormat {
    /// Returns all supported save format variants for use in UI dropdowns.
    pub const ALL: &[SaveFormat] = &[
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

impl SaveFormat {
    /// Infer the closest supported save format from a loaded [`Bmp`].
    ///
    /// For formats that we cannot save (e.g. Core header, JPEG/PNG embedded,
    /// or exotic bitfield layouts), this falls back to [`SaveFormat::Rgb32`].
    pub fn from_bmp(bmp: &Bmp) -> Self {
        use crate::raw::{BitsPerPixel, Compression, RgbMasks};

        match bmp {
            // Core header has no compression field; map by bpp only.
            Bmp::Core(core) => match core.bmp_header.bit_count {
                BitsPerPixel::Bpp1 => Self::Rgb1,
                BitsPerPixel::Bpp4 => Self::Rgb4,
                BitsPerPixel::Bpp8 => Self::Rgb8,
                BitsPerPixel::Bpp24 => Self::Rgb24,
                _ => Self::Rgb32,
            },

            // Info (V3) header — uses compression + bpp + optional color masks.
            Bmp::Info(info) => {
                let bpp = info.bmp_header.bit_count;
                let comp = info.bmp_header.compression;
                match (comp, bpp) {
                    (Compression::Rgb, BitsPerPixel::Bpp1) => Self::Rgb1,
                    (Compression::Rgb, BitsPerPixel::Bpp4) => Self::Rgb4,
                    (Compression::Rgb, BitsPerPixel::Bpp8) => Self::Rgb8,
                    (Compression::Rgb, BitsPerPixel::Bpp16) => Self::Rgb16,
                    (Compression::Rgb, BitsPerPixel::Bpp24) => Self::Rgb24,
                    (Compression::Rgb, BitsPerPixel::Bpp32) => Self::Rgb32,
                    (Compression::Rle8, BitsPerPixel::Bpp8) => Self::Rle8,
                    (Compression::Rle4, BitsPerPixel::Bpp4) => Self::Rle4,
                    (Compression::BitFields, BitsPerPixel::Bpp16) => {
                        // Distinguish RGB565 vs RGB555 by inspecting color masks.
                        match &info.color_masks {
                            Some(masks) if *masks == RgbMasks::rgb565() => Self::BitFields16Rgb565,
                            _ => Self::BitFields16Rgb555,
                        }
                    }
                    (Compression::BitFields, BitsPerPixel::Bpp32) => Self::BitFields32,
                    _ => Self::Rgb32,
                }
            }

            // V4 header — compression + bpp live inside v4.info; masks are in v4.masks.
            Bmp::V4(v4) => {
                let bpp = v4.bmp_header.info.bit_count;
                let comp = v4.bmp_header.info.compression;
                match (comp, bpp) {
                    (Compression::Rgb, BitsPerPixel::Bpp1) => Self::Rgb1,
                    (Compression::Rgb, BitsPerPixel::Bpp4) => Self::Rgb4,
                    (Compression::Rgb, BitsPerPixel::Bpp8) => Self::Rgb8,
                    (Compression::Rgb, BitsPerPixel::Bpp16) => Self::Rgb16,
                    (Compression::Rgb, BitsPerPixel::Bpp24) => Self::Rgb24,
                    (Compression::Rgb, BitsPerPixel::Bpp32) => Self::Rgb32,
                    (Compression::Rle8, BitsPerPixel::Bpp8) => Self::Rle8,
                    (Compression::Rle4, BitsPerPixel::Bpp4) => Self::Rle4,
                    (Compression::BitFields, BitsPerPixel::Bpp16) => {
                        let masks: RgbMasks = v4.bmp_header.masks.into();
                        if masks == RgbMasks::rgb565() {
                            Self::BitFields16Rgb565
                        } else {
                            Self::BitFields16Rgb555
                        }
                    }
                    (Compression::BitFields, BitsPerPixel::Bpp32) => Self::BitFields32,
                    _ => Self::Rgb32,
                }
            }

            // V5 header — compression + bpp in v5.v4.info; masks in v5.v4.masks.
            Bmp::V5(v5) => {
                let bpp = v5.bmp_header.v4.info.bit_count;
                let comp = v5.bmp_header.v4.info.compression;
                match (comp, bpp) {
                    (Compression::Rgb, BitsPerPixel::Bpp1) => Self::Rgb1,
                    (Compression::Rgb, BitsPerPixel::Bpp4) => Self::Rgb4,
                    (Compression::Rgb, BitsPerPixel::Bpp8) => Self::Rgb8,
                    (Compression::Rgb, BitsPerPixel::Bpp16) => Self::Rgb16,
                    (Compression::Rgb, BitsPerPixel::Bpp24) => Self::Rgb24,
                    (Compression::Rgb, BitsPerPixel::Bpp32) => Self::Rgb32,
                    (Compression::Rle8, BitsPerPixel::Bpp8) => Self::Rle8,
                    (Compression::Rle4, BitsPerPixel::Bpp4) => Self::Rle4,
                    (Compression::BitFields, BitsPerPixel::Bpp16) => {
                        let masks: RgbMasks = v5.bmp_header.v4.masks.into();
                        if masks == RgbMasks::rgb565() {
                            Self::BitFields16Rgb565
                        } else {
                            Self::BitFields16Rgb555
                        }
                    }
                    (Compression::BitFields, BitsPerPixel::Bpp32) => Self::BitFields32,
                    _ => Self::Rgb32,
                }
            }
        }
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
// Validate shared preconditions
// ---------------------------------------------------------------------------

fn validate_image(image: &DecodedImage) -> Result<usize, EncodeError> {
    if image.width == 0 || image.height == 0 || image.width > i32::MAX as u32 || image.height > i32::MAX as u32 {
        return Err(EncodeError::InvalidDimensions {
            width: image.width,
            height: image.height,
        });
    }

    let pixel_bytes = (image.width as usize)
        .checked_mul(image.height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(EncodeError::ArithmeticOverflow)?;
    if image.rgba.len() != pixel_bytes {
        return Err(EncodeError::PixelBufferSizeMismatch {
            expected: pixel_bytes,
            actual: image.rgba.len(),
        });
    }

    Ok(pixel_bytes)
}

// ---------------------------------------------------------------------------
// Quantize helpers
// ---------------------------------------------------------------------------

/// Quantize the image to at most `max_colors` and return `(palette_rgbquad_entries, indices)`.
/// Palette entries are in BMP's BGRA ordering.
fn quantize_image(image: &DecodedImage, max_colors: usize) -> (Vec<RgbQuad>, Vec<u8>) {
    let (palette, indices) = quantize::quantize(&image.rgba, max_colors);
    let color_table: Vec<RgbQuad> = palette
        .iter()
        .map(|c| RgbQuad {
            blue: c[2],
            green: c[1],
            red: c[0],
            reserved: 0,
        })
        .collect();
    (color_table, indices)
}

// ---------------------------------------------------------------------------
// Build BitmapInfoData skeleton
// ---------------------------------------------------------------------------

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
    let file_header_size = FileHeader::SIZE;
    // DIB header = 4 (size field) + 40 (BITMAPINFOHEADER body)
    let dib_size = 4_u32 + BitmapInfoHeader::HEADER_SIZE;
    let masks_size: u32 = if color_masks.is_some() { 12 } else { 0 };
    let color_table_size = (color_table.len() as u32)
        .checked_mul(4)
        .ok_or(EncodeError::ArithmeticOverflow)?;

    let pixel_offset = file_header_size
        .checked_add(dib_size)
        .and_then(|x| x.checked_add(masks_size))
        .and_then(|x| x.checked_add(color_table_size))
        .ok_or(EncodeError::ArithmeticOverflow)?;

    let file_size = pixel_offset
        .checked_add(image_size)
        .ok_or(EncodeError::ArithmeticOverflow)?;

    let info = BitmapInfoHeader {
        width: width as i32,
        // top-down for uncompressed, bottom-up for RLE (spec requirement)
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
        colors_used: color_table.len() as u32,
        colors_important: 0,
    };

    Ok(Bmp::Info(BitmapInfoData {
        file_header: FileHeader {
            signature: *b"BM",
            file_size,
            reserved_1: [0; 2],
            reserved_2: [0; 2],
            pixel_data_offset: pixel_offset,
        },
        bmp_header: info,
        color_masks,
        color_table,
        bitmap_array,
    }))
}

// ===========================================================================
// Individual encoders
// ===========================================================================

// ---------------------------------------------------------------------------
// BI_RGB 32-bpp  (original behaviour, kept for backward compat)
// ---------------------------------------------------------------------------

fn encode_rgb32(image: &DecodedImage) -> Result<Bmp, EncodeError> {
    let w = image.width as usize;
    let h = image.height as usize;
    let pixel_bytes = w * h * 4;
    let image_size = u32::try_from(pixel_bytes).map_err(|_| EncodeError::ArithmeticOverflow)?;

    let mut bmp_pixels = Vec::with_capacity(pixel_bytes);
    for px in image.rgba.chunks_exact(4) {
        // BI_RGB 32bpp stores B, G, R, reserved
        bmp_pixels.extend_from_slice(&[px[2], px[1], px[0], 0]);
    }

    build_bmp_info(
        image.width,
        image.height,
        BitsPerPixel::Bpp32,
        Compression::Rgb,
        image_size,
        None,
        Vec::new(),
        bmp_pixels,
    )
}

// ---------------------------------------------------------------------------
// BI_RGB 24-bpp
// ---------------------------------------------------------------------------

fn encode_rgb24(image: &DecodedImage) -> Result<Bmp, EncodeError> {
    let w = image.width as usize;
    let h = image.height as usize;
    let stride = row_stride(w, 24)?;
    let image_size = u32::try_from(stride * h).map_err(|_| EncodeError::ArithmeticOverflow)?;

    let mut bmp_pixels = vec![0u8; stride * h];
    for y in 0..h {
        let row_start = y * stride;
        for x in 0..w {
            let src = (y * w + x) * 4;
            let dst = row_start + x * 3;
            bmp_pixels[dst] = image.rgba[src + 2]; // B
            bmp_pixels[dst + 1] = image.rgba[src + 1]; // G
            bmp_pixels[dst + 2] = image.rgba[src]; // R
        }
    }

    build_bmp_info(
        image.width,
        image.height,
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
    let w = image.width as usize;
    let h = image.height as usize;
    let stride = row_stride(w, 16)?;
    let image_size = u32::try_from(stride * h).map_err(|_| EncodeError::ArithmeticOverflow)?;

    let mut bmp_pixels = vec![0u8; stride * h];
    for y in 0..h {
        let row_start = y * stride;
        for x in 0..w {
            let src = (y * w + x) * 4;
            let r5 = (image.rgba[src] as u16 * 31 + 127) / 255;
            let g5 = (image.rgba[src + 1] as u16 * 31 + 127) / 255;
            let b5 = (image.rgba[src + 2] as u16 * 31 + 127) / 255;
            let px16: u16 = (r5 << 10) | (g5 << 5) | b5;
            let dst = row_start + x * 2;
            bmp_pixels[dst..dst + 2].copy_from_slice(&px16.to_le_bytes());
        }
    }

    build_bmp_info(
        image.width,
        image.height,
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

    let (color_table, indices) = quantize_image(image, max_colors);

    let w = image.width as usize;
    let h = image.height as usize;
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
        image.width,
        image.height,
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
    let (color_table, indices) = quantize_image(image, 256);

    let w = image.width as usize;
    let h = image.height as usize;

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
        image.width,
        image.height,
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
    let (color_table, indices) = quantize_image(image, 16);

    let w = image.width as usize;
    let h = image.height as usize;

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
        image.width,
        image.height,
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
    let w = image.width as usize;
    let h = image.height as usize;
    let stride = row_stride(w, 16)?;
    let image_size = u32::try_from(stride * h).map_err(|_| EncodeError::ArithmeticOverflow)?;

    // Pre-compute shifts and widths from the masks.
    let r_shift = masks.red_mask.trailing_zeros();
    let r_bits = masks.red_mask.count_ones();
    let g_shift = masks.green_mask.trailing_zeros();
    let g_bits = masks.green_mask.count_ones();
    let b_shift = masks.blue_mask.trailing_zeros();
    let b_bits = masks.blue_mask.count_ones();

    let mut bmp_pixels = vec![0u8; stride * h];
    for y in 0..h {
        let row_start = y * stride;
        for x in 0..w {
            let src = (y * w + x) * 4;
            let r = image.rgba[src] as u16;
            let g = image.rgba[src + 1] as u16;
            let b = image.rgba[src + 2] as u16;

            let r_max = (1u16 << r_bits) - 1;
            let g_max = (1u16 << g_bits) - 1;
            let b_max = (1u16 << b_bits) - 1;

            let rv = (r * r_max + 127) / 255;
            let gv = (g * g_max + 127) / 255;
            let bv = (b * b_max + 127) / 255;

            let px16: u16 = (rv << r_shift) | (gv << g_shift) | (bv << b_shift);
            let dst = row_start + x * 2;
            bmp_pixels[dst..dst + 2].copy_from_slice(&px16.to_le_bytes());
        }
    }

    build_bmp_info(
        image.width,
        image.height,
        BitsPerPixel::Bpp16,
        Compression::BitFields,
        image_size,
        Some(masks),
        Vec::new(),
        bmp_pixels,
    )
}

// ---------------------------------------------------------------------------
// BI_BITFIELDS 32-bpp
// ---------------------------------------------------------------------------

fn encode_bitfields32(image: &DecodedImage) -> Result<Bmp, EncodeError> {
    let w = image.width as usize;
    let h = image.height as usize;
    let pixel_bytes = w * h * 4;
    let image_size = u32::try_from(pixel_bytes).map_err(|_| EncodeError::ArithmeticOverflow)?;

    // Standard RGB888 masks: R=0x00FF0000, G=0x0000FF00, B=0x000000FF
    let masks = RgbMasks::rgb888();

    let mut bmp_pixels = Vec::with_capacity(pixel_bytes);
    for px in image.rgba.chunks_exact(4) {
        // Pack as 0x00RRGGBB in little-endian
        bmp_pixels.extend_from_slice(&[px[2], px[1], px[0], 0]);
    }

    build_bmp_info(
        image.width,
        image.height,
        BitsPerPixel::Bpp32,
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

/// Encodes a decoded RGBA image into a BMP using the default format
/// (32-bit uncompressed RGB). This preserves the original API.
pub fn encode_rgba_to_bmp(image: &DecodedImage) -> Result<Bmp, EncodeError> {
    encode_rgba_to_bmp_with_format(image, SaveFormat::Rgb32)
}

/// Encodes a decoded RGBA image into a BMP using the specified [`SaveFormat`].
pub fn encode_rgba_to_bmp_with_format(image: &DecodedImage, format: SaveFormat) -> Result<Bmp, EncodeError> {
    validate_image(image)?;

    match format {
        SaveFormat::Rgb32 => encode_rgb32(image),
        SaveFormat::Rgb24 => encode_rgb24(image),
        SaveFormat::Rgb16 => encode_rgb16(image),
        SaveFormat::Rgb8 => encode_indexed_rgb(image, BitsPerPixel::Bpp8),
        SaveFormat::Rgb4 => encode_indexed_rgb(image, BitsPerPixel::Bpp4),
        SaveFormat::Rgb1 => encode_indexed_rgb(image, BitsPerPixel::Bpp1),
        SaveFormat::Rle8 => encode_rle8(image),
        SaveFormat::Rle4 => encode_rle4(image),
        SaveFormat::BitFields16Rgb565 => encode_bitfields16(image, RgbMasks::rgb565()),
        SaveFormat::BitFields16Rgb555 => encode_bitfields16(image, RgbMasks::rgb555()),
        SaveFormat::BitFields32 => encode_bitfields32(image),
    }
}

/// Saves a decoded RGBA image to a BMP file using the default format (32-bit
/// uncompressed RGB). This preserves the original API.
pub fn save_bmp(path: &Path, image: &DecodedImage) -> Result<(), EncodeError> {
    save_bmp_with_format(path, image, SaveFormat::Rgb32)
}

/// Saves a decoded RGBA image to a BMP file using the specified [`SaveFormat`].
pub fn save_bmp_with_format(path: &Path, image: &DecodedImage, format: SaveFormat) -> Result<(), EncodeError> {
    let bmp = encode_rgba_to_bmp_with_format(image, format)?;
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    bmp.write_unchecked(&mut writer)?;
    Ok(())
}
