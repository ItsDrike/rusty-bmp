use thiserror::Error;

use crate::raw::{BitsPerPixel, Bmp, Compression, RgbMasks, RgbaMasks};

#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompressionDecoder {
    Rgb,
    Rle4,
    Rle8,
    BitFields,
    Other(u32),
}

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("unsupported compression for decode: {0:?}")]
    UnsupportedCompression(CompressionDecoder),

    #[error("invalid image dimensions: {width}x{height}")]
    InvalidDimensions { width: i32, height: i32 },

    #[error("pixel buffer too small: need {required} bytes, found {actual}")]
    PixelBufferTooSmall { required: usize, actual: usize },

    #[error("palette index out of range at ({x}, {y}): index={index}, palette_len={palette_len}")]
    PaletteIndexOutOfRange {
        x: usize,
        y: usize,
        index: usize,
        palette_len: usize,
    },

    #[error("invalid RLE stream: {0}")]
    InvalidRle(&'static str),

    #[error("RLE operation moved outside image bounds")]
    RleOutOfBounds,

    #[error("BITFIELDS decode requires channel masks")]
    MissingBitfieldMasks,

    #[error("BITFIELDS decode supports only 16 or 32 bpp, got {0}")]
    InvalidBitfieldsBpp(BitsPerPixel),

    #[error("unsupported bits-per-pixel for RGB decode: {0}")]
    UnsupportedBitsPerPixel(BitsPerPixel),

    #[error("arithmetic overflow while decoding: {0}")]
    ArithmeticOverflow(&'static str),

    #[error("decoded RGBA output too large: {decoded_size} bytes exceeds safe max {max_allowed}")]
    DecodedImageTooLarge { decoded_size: usize, max_allowed: usize },
}

const MAX_DECODED_RGBA_BYTES: usize = 512 * 1024 * 1024;

fn rgba_output_len(width: usize, height: usize) -> Result<usize, DecodeError> {
    let pixels = width
        .checked_mul(height)
        .ok_or(DecodeError::ArithmeticOverflow("pixel count"))?;
    let bytes = pixels
        .checked_mul(4)
        .ok_or(DecodeError::ArithmeticOverflow("RGBA output size"))?;
    if bytes > MAX_DECODED_RGBA_BYTES {
        return Err(DecodeError::DecodedImageTooLarge {
            decoded_size: bytes,
            max_allowed: MAX_DECODED_RGBA_BYTES,
        });
    }
    Ok(bytes)
}

fn row_stride(width: usize, bits_per_pixel: u16) -> Result<usize, DecodeError> {
    let bits_per_row = width
        .checked_mul(bits_per_pixel as usize)
        .ok_or(DecodeError::ArithmeticOverflow("bits per row"))?;
    let with_padding = bits_per_row
        .checked_add(31)
        .ok_or(DecodeError::ArithmeticOverflow("row padding"))?;
    Ok((with_padding / 32) * 4)
}

#[allow(clippy::cast_possible_truncation)]
const fn rgb555_to_rgba(px: u16) -> [u8; 4] {
    let r = (((px >> 10) & 0x1f) * 255 / 31) as u8;
    let g = (((px >> 5) & 0x1f) * 255 / 31) as u8;
    let b = ((px & 0x1f) * 255 / 31) as u8;
    [r, g, b, 255]
}

fn decode_indexed_row(
    bpp: BitsPerPixel,
    row_data: &[u8],
    y: usize,
    width: usize,
    palette: &[[u8; 4]],
    out: &mut [u8],
) -> Result<(), DecodeError> {
    let mut write_pixel = |x: usize, idx: usize| -> Result<(), DecodeError> {
        let color = palette.get(idx).ok_or(DecodeError::PaletteIndexOutOfRange {
            x,
            y,
            index: idx,
            palette_len: palette.len(),
        })?;
        let dst = (y * width + x) * 4;
        out[dst..dst + 4].copy_from_slice(color);
        Ok(())
    };

    match bpp {
        BitsPerPixel::Bpp1 => {
            for x in 0..width {
                let byte = row_data[x / 8];
                let bit = 7 - (x % 8);
                let idx = ((byte >> bit) & 0x1) as usize;
                write_pixel(x, idx)?;
            }
        }
        BitsPerPixel::Bpp4 => {
            for x in 0..width {
                let byte = row_data[x / 2];
                let idx = if x % 2 == 0 { (byte >> 4) & 0x0f } else { byte & 0x0f };
                write_pixel(x, idx as usize)?;
            }
        }
        BitsPerPixel::Bpp8 => {
            for (x, &idx) in row_data.iter().take(width).enumerate() {
                write_pixel(x, idx as usize)?;
            }
        }
        _ => return Err(DecodeError::UnsupportedBitsPerPixel(bpp)),
    }

    Ok(())
}

fn decode_rgb_pixels(
    pixel_data: &[u8],
    width: usize,
    height: usize,
    top_down: bool,
    bpp: BitsPerPixel,
    palette: &[[u8; 4]],
) -> Result<Vec<u8>, DecodeError> {
    let stride = row_stride(width, bpp.bit_count())?;
    let required = stride
        .checked_mul(height)
        .ok_or(DecodeError::ArithmeticOverflow("required pixel data size"))?;
    if pixel_data.len() < required {
        return Err(DecodeError::PixelBufferTooSmall {
            required,
            actual: pixel_data.len(),
        });
    }

    let mut out = vec![0_u8; rgba_output_len(width, height)?];

    for y_out in 0..height {
        let y_src = if top_down { y_out } else { height - 1 - y_out };
        let row_start = y_src * stride;
        let row_end = row_start + stride;
        let row = &pixel_data[row_start..row_end];

        match bpp {
            BitsPerPixel::Bpp1 | BitsPerPixel::Bpp4 | BitsPerPixel::Bpp8 => {
                decode_indexed_row(bpp, row, y_out, width, palette, &mut out)?;
            }
            BitsPerPixel::Bpp16 => {
                for x in 0..width {
                    let i = x * 2;
                    let px = u16::from_le_bytes([row[i], row[i + 1]]);
                    let rgba = rgb555_to_rgba(px);
                    let dst = (y_out * width + x) * 4;
                    out[dst..dst + 4].copy_from_slice(&rgba);
                }
            }
            BitsPerPixel::Bpp24 => {
                for x in 0..width {
                    let i = x * 3;
                    let b = row[i];
                    let g = row[i + 1];
                    let r = row[i + 2];
                    let dst = (y_out * width + x) * 4;
                    out[dst..dst + 4].copy_from_slice(&[r, g, b, 255]);
                }
            }
            BitsPerPixel::Bpp32 => {
                for x in 0..width {
                    let i = x * 4;
                    let b = row[i];
                    let g = row[i + 1];
                    let r = row[i + 2];
                    let dst = (y_out * width + x) * 4;
                    out[dst..dst + 4].copy_from_slice(&[r, g, b, 255]);
                }
            }
            _ => return Err(DecodeError::UnsupportedBitsPerPixel(bpp)),
        }
    }

    Ok(out)
}

/// Extracts a masked color channel from `px` and scales it to an 8-bit value.
///
/// The function isolates the bits selected by `mask`, right-aligns them,
/// and linearly scales the resulting value to the full `0..=255` range.
/// This is used for decoding RGB channel values from bitfields in packed
/// pixel formats (e.g. BMP `BI_BITFIELDS`).
///
/// The number of bits used by the channel is determined from `mask`, so
/// formats like 5-6-5 or arbitrary bitfield layouts are handled correctly.
///
/// If `mask` is zero, the function returns `0`.
///
/// # Examples
///
/// ```text
/// px = 0b1111100000000000, mask = 0b1111100000000000 -> 255
/// px = 0b0000011111100000, mask = 0b0000011111100000 -> 255
/// px = 0b0000000000011111, mask = 0b0000000000011111 -> 255
/// ```
fn scale_masked_channel(px: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }

    let shift = mask.trailing_zeros();
    let bits = mask.count_ones();

    // This helper only explicitly supports channels that fit into 8 bits
    // Anything larger would just waste those bits anyways, as we map the
    // range into 0..=255 (u8) anyways.
    debug_assert!(bits <= 8);

    let raw: u32 = (px & mask) >> shift; // always <= 255
    let max: u32 = (1u32 << bits) - 1; // always <= 255

    // BMP masks should be contiguous
    debug_assert_eq!(mask >> shift, max);

    // Safe: This explicitly maps the range into 0..=255, u8 cast is safe
    #[allow(clippy::cast_possible_truncation)]
    let mapped = ((raw * 255) / max) as u8;

    mapped
}

fn decode_bitfields_pixels(
    pixel_data: &[u8],
    width: usize,
    height: usize,
    top_down: bool,
    bpp: BitsPerPixel,
    masks: RgbaMasks,
) -> Result<Vec<u8>, DecodeError> {
    if !matches!(bpp, BitsPerPixel::Bpp16 | BitsPerPixel::Bpp32) {
        return Err(DecodeError::InvalidBitfieldsBpp(bpp));
    }

    let stride = row_stride(width, bpp.bit_count())?;
    let required = stride
        .checked_mul(height)
        .ok_or(DecodeError::ArithmeticOverflow("required pixel data size"))?;
    if pixel_data.len() < required {
        return Err(DecodeError::PixelBufferTooSmall {
            required,
            actual: pixel_data.len(),
        });
    }

    let mut out = vec![0_u8; rgba_output_len(width, height)?];
    for y_out in 0..height {
        let y_src = if top_down { y_out } else { height - 1 - y_out };
        let row_start = y_src * stride;
        let row_end = row_start + stride;
        let row = &pixel_data[row_start..row_end];

        for x in 0..width {
            let px = match bpp {
                BitsPerPixel::Bpp16 => {
                    let i = x * 2;
                    u32::from(u16::from_le_bytes([row[i], row[i + 1]]))
                }
                BitsPerPixel::Bpp32 => {
                    let i = x * 4;
                    u32::from_le_bytes([row[i], row[i + 1], row[i + 2], row[i + 3]])
                }
                _ => unreachable!(),
            };

            let r = scale_masked_channel(px, masks.red_mask);
            let g = scale_masked_channel(px, masks.green_mask);
            let b = scale_masked_channel(px, masks.blue_mask);
            let a = if masks.alpha_mask == 0 {
                255
            } else {
                scale_masked_channel(px, masks.alpha_mask)
            };

            let dst = (y_out * width + x) * 4;
            out[dst..dst + 4].copy_from_slice(&[r, g, b, a]);
        }
    }

    Ok(out)
}

struct IndexedPixelWriter<'a> {
    width: usize,
    height: usize,
    top_down: bool,
    palette: &'a [[u8; 4]],
}

impl IndexedPixelWriter<'_> {
    fn write(&self, out: &mut [u8], x: usize, y: usize, idx: usize) -> Result<(), DecodeError> {
        if x >= self.width || y >= self.height {
            return Err(DecodeError::RleOutOfBounds);
        }
        let color = self.palette.get(idx).ok_or(DecodeError::PaletteIndexOutOfRange {
            x,
            y,
            index: idx,
            palette_len: self.palette.len(),
        })?;
        let y_out = if self.top_down { y } else { self.height - 1 - y };
        let dst = (y_out * self.width + x) * 4;
        out[dst..dst + 4].copy_from_slice(color);
        Ok(())
    }
}

fn decode_rle8_pixels(
    pixel_data: &[u8],
    width: usize,
    height: usize,
    top_down: bool,
    palette: &[[u8; 4]],
) -> Result<Vec<u8>, DecodeError> {
    let mut out = vec![0_u8; rgba_output_len(width, height)?];
    let pixel_writer = IndexedPixelWriter {
        width,
        height,
        top_down,
        palette,
    };
    let mut i = 0usize;
    let mut x = 0usize;
    let mut y = 0usize;

    while i + 1 < pixel_data.len() {
        let count = pixel_data[i];
        let value = pixel_data[i + 1];
        i += 2;

        if count > 0 {
            for _ in 0..count {
                pixel_writer.write(&mut out, x, y, value as usize)?;
                x += 1;
                if x > width {
                    return Err(DecodeError::RleOutOfBounds);
                }
            }
            continue;
        }

        match value {
            0 => {
                x = 0;
                y += 1;
                if y >= height {
                    return Ok(out);
                }
            }
            1 => return Ok(out),
            2 => {
                if i + 1 >= pixel_data.len() {
                    return Err(DecodeError::InvalidRle("delta escape missing dx/dy"));
                }
                let dx = pixel_data[i] as usize;
                let dy = pixel_data[i + 1] as usize;
                i += 2;
                x += dx;
                y += dy;
                if x > width || y >= height {
                    return Err(DecodeError::RleOutOfBounds);
                }
            }
            n => {
                let n = n as usize;
                if i + n > pixel_data.len() {
                    return Err(DecodeError::InvalidRle("absolute run exceeds stream"));
                }
                for &idx in &pixel_data[i..i + n] {
                    pixel_writer.write(&mut out, x, y, idx as usize)?;
                    x += 1;
                    if x > width {
                        return Err(DecodeError::RleOutOfBounds);
                    }
                }
                i += n;
                if (n & 1) == 1 {
                    if i >= pixel_data.len() {
                        return Err(DecodeError::InvalidRle("absolute run pad byte missing"));
                    }
                    i += 1;
                }
            }
        }
    }

    Ok(out)
}

fn decode_rle4_pixels(
    pixel_data: &[u8],
    width: usize,
    height: usize,
    top_down: bool,
    palette: &[[u8; 4]],
) -> Result<Vec<u8>, DecodeError> {
    let mut out = vec![0_u8; rgba_output_len(width, height)?];
    let pixel_writer = IndexedPixelWriter {
        width,
        height,
        top_down,
        palette,
    };
    let mut i = 0usize;
    let mut x = 0usize;
    let mut y = 0usize;

    while i + 1 < pixel_data.len() {
        let count = pixel_data[i];
        let value = pixel_data[i + 1];
        i += 2;

        if count > 0 {
            let hi = (value >> 4) as usize;
            let lo = (value & 0x0f) as usize;
            for k in 0..(count as usize) {
                let idx = if (k & 1) == 0 { hi } else { lo };
                pixel_writer.write(&mut out, x, y, idx)?;
                x += 1;
                if x > width {
                    return Err(DecodeError::RleOutOfBounds);
                }
            }
            continue;
        }

        match value {
            0 => {
                x = 0;
                y += 1;
                if y >= height {
                    return Ok(out);
                }
            }
            1 => return Ok(out),
            2 => {
                if i + 1 >= pixel_data.len() {
                    return Err(DecodeError::InvalidRle("delta escape missing dx/dy"));
                }
                let dx = pixel_data[i] as usize;
                let dy = pixel_data[i + 1] as usize;
                i += 2;
                x += dx;
                y += dy;
                if x > width || y >= height {
                    return Err(DecodeError::RleOutOfBounds);
                }
            }
            n => {
                let n = n as usize;
                let bytes = n.div_ceil(2);
                if i + bytes > pixel_data.len() {
                    return Err(DecodeError::InvalidRle("absolute run exceeds stream"));
                }
                for p in 0..n {
                    let b = pixel_data[i + (p / 2)];
                    let idx = if (p & 1) == 0 {
                        (b >> 4) as usize
                    } else {
                        (b & 0x0f) as usize
                    };
                    pixel_writer.write(&mut out, x, y, idx)?;
                    x += 1;
                    if x > width {
                        return Err(DecodeError::RleOutOfBounds);
                    }
                }
                i += bytes;
                if (bytes & 1) == 1 {
                    if i >= pixel_data.len() {
                        return Err(DecodeError::InvalidRle("absolute run pad byte missing"));
                    }
                    i += 1;
                }
            }
        }
    }

    Ok(out)
}

/// Decodes a parsed BMP into an RGBA pixel buffer.
///
/// # Errors
/// Returns [`DecodeError`] when dimensions, compression, masks, palette usage,
/// or encoded pixel data are invalid or inconsistent.
pub fn decode_to_rgba(bmp: &Bmp) -> Result<DecodedImage, DecodeError> {
    let (width_i32, height_i32, bit_count, compression, pixel_data, palette, top_down, bitfields_masks) = match bmp {
        Bmp::Core(data) => (
            i32::from(data.bmp_header.width),
            i32::from(data.bmp_header.height),
            data.bmp_header.bit_count,
            Compression::Rgb,
            data.bitmap_array.as_slice(),
            data.color_table
                .iter()
                .map(|c| [c.red, c.green, c.blue, 255])
                .collect::<Vec<_>>(),
            false,
            None,
        ),
        Bmp::Info(data) => (
            data.bmp_header.width,
            data.bmp_header.height,
            data.bmp_header.bit_count,
            data.bmp_header.compression,
            data.bitmap_array.as_slice(),
            data.color_table
                .iter()
                .map(|c| [c.red, c.green, c.blue, 255])
                .collect::<Vec<_>>(),
            data.bmp_header.height < 0,
            data.color_masks.map(RgbaMasks::from),
        ),
        Bmp::V4(data) => (
            data.bmp_header.info.width,
            data.bmp_header.info.height,
            data.bmp_header.info.bit_count,
            data.bmp_header.info.compression,
            data.bitmap_array.as_slice(),
            data.color_table
                .iter()
                .map(|c| [c.red, c.green, c.blue, 255])
                .collect::<Vec<_>>(),
            data.bmp_header.info.height < 0,
            Some(data.bmp_header.masks),
        ),
        Bmp::V5(data) => (
            data.bmp_header.v4.info.width,
            data.bmp_header.v4.info.height,
            data.bmp_header.v4.info.bit_count,
            data.bmp_header.v4.info.compression,
            data.bitmap_array.as_slice(),
            data.color_table
                .iter()
                .map(|c| [c.red, c.green, c.blue, 255])
                .collect::<Vec<_>>(),
            data.bmp_header.v4.info.height < 0,
            Some(data.bmp_header.v4.masks),
        ),
    };

    let decoder = match compression {
        Compression::Rgb => CompressionDecoder::Rgb,
        Compression::Rle4 => CompressionDecoder::Rle4,
        Compression::Rle8 => CompressionDecoder::Rle8,
        Compression::BitFields => CompressionDecoder::BitFields,
        Compression::Jpeg => CompressionDecoder::Other(4),
        Compression::Png => CompressionDecoder::Other(5),
        Compression::Other(x) => CompressionDecoder::Other(x),
    };

    if width_i32 <= 0 || height_i32 == 0 {
        return Err(DecodeError::InvalidDimensions {
            width: width_i32,
            height: height_i32,
        });
    }

    // Safe: We already checked that this is >= 0
    #[allow(clippy::cast_sign_loss)]
    let width = width_i32 as usize;

    let height = height_i32.unsigned_abs() as usize;

    let rgba = match decoder {
        CompressionDecoder::Rgb => decode_rgb_pixels(pixel_data, width, height, top_down, bit_count, &palette)?,
        CompressionDecoder::Rle8 => decode_rle8_pixels(pixel_data, width, height, top_down, &palette)?,
        CompressionDecoder::Rle4 => decode_rle4_pixels(pixel_data, width, height, top_down, &palette)?,
        CompressionDecoder::BitFields => {
            let masks = if let Some(masks) = bitfields_masks {
                masks
            } else {
                match bit_count {
                    BitsPerPixel::Bpp16 => RgbMasks::rgb555().into(),
                    BitsPerPixel::Bpp32 => RgbMasks::rgb888().into(),
                    _ => return Err(DecodeError::MissingBitfieldMasks),
                }
            };
            decode_bitfields_pixels(pixel_data, width, height, top_down, bit_count, masks)?
        }
        CompressionDecoder::Other(x) => return Err(DecodeError::UnsupportedCompression(CompressionDecoder::Other(x))),
    };

    Ok(DecodedImage {
        width: u32::try_from(width).map_err(|_| DecodeError::ArithmeticOverflow("width output cast"))?,
        height: u32::try_from(height).map_err(|_| DecodeError::ArithmeticOverflow("height output cast"))?,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::{DecodeError, MAX_DECODED_RGBA_BYTES, rgba_output_len};

    #[test]
    fn rgba_output_len_rejects_multiplication_overflow() {
        let err = rgba_output_len(usize::MAX, 2).expect_err("must fail on overflow");
        assert!(matches!(err, DecodeError::ArithmeticOverflow("pixel count")));
    }

    #[test]
    fn rgba_output_len_rejects_excessive_allocation() {
        let over_limit_pixels = (MAX_DECODED_RGBA_BYTES / 4) + 1;
        let err = rgba_output_len(over_limit_pixels, 1).expect_err("must enforce decoded size cap");
        assert!(matches!(err, DecodeError::DecodedImageTooLarge { .. }));
    }
}
