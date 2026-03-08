use std::{fs::File, io::BufWriter, path::Path};

use thiserror::Error;

use crate::{
    raw::{BitmapInfoData, BitmapInfoHeader, BitsPerPixel, Bmp, Compression, FileHeader},
    runtime::decode::DecodedImage,
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

pub fn encode_rgba_to_bmp(image: &DecodedImage) -> Result<Bmp, EncodeError> {
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

    let file_header_size = FileHeader::SIZE;
    let dib_size = 4_u32 + BitmapInfoHeader::HEADER_SIZE;
    let pixel_offset = file_header_size
        .checked_add(dib_size)
        .ok_or(EncodeError::ArithmeticOverflow)?;
    let image_size = u32::try_from(pixel_bytes).map_err(|_| EncodeError::ArithmeticOverflow)?;
    let file_size = pixel_offset
        .checked_add(image_size)
        .ok_or(EncodeError::ArithmeticOverflow)?;

    let mut bmp_pixels = Vec::with_capacity(pixel_bytes);
    for px in image.rgba.chunks_exact(4) {
        // BI_RGB 32bpp stores channels as B, G, R, reserved.
        bmp_pixels.extend_from_slice(&[px[2], px[1], px[0], 0]);
    }

    // Store as top-down BI_RGB 32bpp so the in-memory row order can be preserved.
    let info = BitmapInfoHeader {
        width: image.width as i32,
        height: -(image.height as i32),
        planes: 1,
        bit_count: BitsPerPixel::Bpp32,
        compression: Compression::Rgb,
        image_size,
        x_resolution_ppm: 0,
        y_resolution_ppm: 0,
        colors_used: 0,
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
        color_masks: None,
        color_table: Vec::new(),
        bitmap_array: bmp_pixels,
    }))
}

pub fn save_bmp(path: &Path, image: &DecodedImage) -> Result<(), EncodeError> {
    let bmp = encode_rgba_to_bmp(image)?;
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    bmp.write_unchecked(&mut writer)?;
    Ok(())
}
