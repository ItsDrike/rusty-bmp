use std::fmt::Write as _;

use bmp::{
    raw::{Bmp, Compression},
    runtime::decode::DecodedImage,
};

pub struct BmpInfoSections {
    pub image_stats: String,
    pub decoded_stats: String,
}

fn with_grouping(value: u64) -> String {
    let s = value.to_string();
    let mut out = String::with_capacity(s.len() + (s.len() / 3));
    for (i, ch) in s.chars().rev().enumerate() {
        if i != 0 && (i % 3) == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn format_bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut scaled = value as f64;
    let mut unit = 0usize;
    while scaled >= 1024.0 && unit < (UNITS.len() - 1) {
        scaled /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} B", with_grouping(value))
    } else {
        format!("{} B ({scaled:.2} {})", with_grouping(value), UNITS[unit])
    }
}

fn compression_name(compression: Compression) -> &'static str {
    match compression {
        Compression::Rgb => "BI_RGB",
        Compression::Rle8 => "BI_RLE8",
        Compression::Rle4 => "BI_RLE4",
        Compression::BitFields => "BI_BITFIELDS",
        Compression::Jpeg => "BI_JPEG",
        Compression::Png => "BI_PNG",
        Compression::Other(_) => "UNKNOWN",
    }
}

fn write_decode_stats(out: &mut String, decoded: &DecodedImage, encoded_pixel_bytes: usize) {
    let decoded_bytes = decoded.rgba.len() as u64;
    let _ = writeln!(out, "Decoded RGBA buffer: {}", format_bytes(decoded_bytes));
    let _ = writeln!(out, "Decoded bytes per pixel: 4");

    if encoded_pixel_bytes > 0 {
        let ratio = decoded_bytes as f64 / encoded_pixel_bytes as f64;
        let _ = writeln!(out, "Decode expansion ratio: {ratio:.2}x");
    }
}

fn encoded_pixel_bytes(bmp: &Bmp) -> usize {
    match bmp {
        Bmp::Core(data) => data.bitmap_array.len(),
        Bmp::Info(data) => data.bitmap_array.len(),
        Bmp::V4(data) => data.bitmap_array.len(),
        Bmp::V5(data) => data.bitmap_array.len(),
    }
}

pub fn format_bmp_info_sections(bmp: &Bmp, decoded: &DecodedImage) -> BmpInfoSections {
    let mut out = String::new();
    match bmp {
        Bmp::Core(data) => {
            let _ = writeln!(&mut out, "Variant: CORE (BITMAPCOREHEADER)");
            let _ = writeln!(
                &mut out,
                "Size: {} x {} px",
                data.bmp_header.width, data.bmp_header.height
            );
            let _ = writeln!(&mut out, "Orientation: bottom-up");
            let _ = writeln!(&mut out, "Bits per pixel: {}", data.bmp_header.bit_count.bit_count());
            let _ = writeln!(&mut out, "Compression: BI_RGB (implicit)");
            let _ = writeln!(&mut out, "Palette entries: {}", data.color_table.len());
            let _ = writeln!(
                &mut out,
                "Pixel data size: {}",
                format_bytes(data.bitmap_array.len() as u64)
            );
            let _ = writeln!(
                &mut out,
                "File size: {}",
                format_bytes(data.file_header.file_size as u64)
            );
            let _ = writeln!(
                &mut out,
                "Pixel data offset: {}",
                format_bytes(data.file_header.pixel_data_offset as u64)
            );
        }
        Bmp::Info(data) => {
            let h = data.bmp_header;
            let _ = writeln!(&mut out, "Variant: INFO (BITMAPINFOHEADER)");
            let _ = writeln!(&mut out, "Size: {} x {} px", decoded.width, decoded.height);
            let _ = writeln!(
                &mut out,
                "Orientation: {}",
                if h.height < 0 { "top-down" } else { "bottom-up" }
            );
            let _ = writeln!(&mut out, "Bits per pixel: {}", h.bit_count.bit_count());
            let _ = writeln!(
                &mut out,
                "Compression: {} ({:?})",
                compression_name(h.compression),
                h.compression
            );
            let _ = writeln!(&mut out, "Header image_size: {}", format_bytes(h.image_size as u64));
            let _ = writeln!(
                &mut out,
                "Pixel data size: {}",
                format_bytes(data.bitmap_array.len() as u64)
            );
            let _ = writeln!(&mut out, "Palette entries: {}", data.color_table.len());
            if let Some(masks) = data.color_masks {
                let _ = writeln!(
                    &mut out,
                    "Bit masks: R={:#010X} G={:#010X} B={:#010X}",
                    masks.red_mask, masks.green_mask, masks.blue_mask
                );
            }
            let _ = writeln!(
                &mut out,
                "File size: {}",
                format_bytes(data.file_header.file_size as u64)
            );
            let _ = writeln!(
                &mut out,
                "Pixel data offset: {}",
                format_bytes(data.file_header.pixel_data_offset as u64)
            );
        }
        Bmp::V4(data) => {
            let h = data.bmp_header.info;
            let m = data.bmp_header.masks;
            let _ = writeln!(&mut out, "Variant: V4 (BITMAPV4HEADER)");
            let _ = writeln!(&mut out, "Size: {} x {} px", decoded.width, decoded.height);
            let _ = writeln!(
                &mut out,
                "Orientation: {}",
                if h.height < 0 { "top-down" } else { "bottom-up" }
            );
            let _ = writeln!(&mut out, "Bits per pixel: {}", h.bit_count.bit_count());
            let _ = writeln!(
                &mut out,
                "Compression: {} ({:?})",
                compression_name(h.compression),
                h.compression
            );
            let _ = writeln!(&mut out, "Header image_size: {}", format_bytes(h.image_size as u64));
            let _ = writeln!(
                &mut out,
                "Pixel data size: {}",
                format_bytes(data.bitmap_array.len() as u64)
            );
            let _ = writeln!(&mut out, "Palette entries: {}", data.color_table.len());
            let _ = writeln!(
                &mut out,
                "Bit masks: R={:#010X} G={:#010X} B={:#010X} A={:#010X}",
                m.red_mask, m.green_mask, m.blue_mask, m.alpha_mask
            );
            let _ = writeln!(&mut out, "Color space: {:?}", data.bmp_header.cs_type);
            let _ = writeln!(
                &mut out,
                "File size: {}",
                format_bytes(data.file_header.file_size as u64)
            );
            let _ = writeln!(
                &mut out,
                "Pixel data offset: {}",
                format_bytes(data.file_header.pixel_data_offset as u64)
            );
        }
        Bmp::V5(data) => {
            let h = data.bmp_header.v4.info;
            let m = data.bmp_header.v4.masks;
            let _ = writeln!(&mut out, "Variant: V5 (BITMAPV5HEADER)");
            let _ = writeln!(&mut out, "Size: {} x {} px", decoded.width, decoded.height);
            let _ = writeln!(
                &mut out,
                "Orientation: {}",
                if h.height < 0 { "top-down" } else { "bottom-up" }
            );
            let _ = writeln!(&mut out, "Bits per pixel: {}", h.bit_count.bit_count());
            let _ = writeln!(
                &mut out,
                "Compression: {} ({:?})",
                compression_name(h.compression),
                h.compression
            );
            let _ = writeln!(&mut out, "Header image_size: {}", format_bytes(h.image_size as u64));
            let _ = writeln!(
                &mut out,
                "Pixel data size: {}",
                format_bytes(data.bitmap_array.len() as u64)
            );
            let _ = writeln!(&mut out, "Palette entries: {}", data.color_table.len());
            let _ = writeln!(
                &mut out,
                "Bit masks: R={:#010X} G={:#010X} B={:#010X} A={:#010X}",
                m.red_mask, m.green_mask, m.blue_mask, m.alpha_mask
            );
            let _ = writeln!(&mut out, "Color space: {:?}", data.bmp_header.v4.cs_type);
            let _ = writeln!(&mut out, "Intent: {}", data.bmp_header.intent);
            let _ = writeln!(
                &mut out,
                "Profile offset: {}",
                format_bytes(data.bmp_header.profile_data as u64)
            );
            let _ = writeln!(
                &mut out,
                "Profile size: {}",
                format_bytes(data.bmp_header.profile_size as u64)
            );
            let _ = writeln!(
                &mut out,
                "ICC profile bytes loaded: {}",
                format_bytes(data.icc_profile.as_ref().map_or(0, Vec::len) as u64)
            );
            let _ = writeln!(
                &mut out,
                "File size: {}",
                format_bytes(data.file_header.file_size as u64)
            );
            let _ = writeln!(
                &mut out,
                "Pixel data offset: {}",
                format_bytes(data.file_header.pixel_data_offset as u64)
            );
        }
    }

    let mut decoded_stats = String::new();
    write_decode_stats(&mut decoded_stats, decoded, encoded_pixel_bytes(bmp));

    BmpInfoSections {
        image_stats: out,
        decoded_stats,
    }
}

#[cfg(test)]
mod tests {
    use super::{compression_name, format_bytes, with_grouping, write_decode_stats};
    use bmp::raw::Compression;
    use bmp::runtime::decode::DecodedImage;

    #[test]
    fn grouping_formats_with_commas() {
        assert_eq!(with_grouping(0), "0");
        assert_eq!(with_grouping(12), "12");
        assert_eq!(with_grouping(1_234), "1,234");
        assert_eq!(with_grouping(12_345_678), "12,345,678");
    }

    #[test]
    fn bytes_formatting_uses_binary_units() {
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1_024), "1,024 B (1.00 KiB)");
        assert_eq!(format_bytes(1_536), "1,536 B (1.50 KiB)");
        assert_eq!(format_bytes(5 * 1_048_576), "5,242,880 B (5.00 MiB)");
    }

    #[test]
    fn compression_names_match_bi_constants() {
        assert_eq!(compression_name(Compression::Rgb), "BI_RGB");
        assert_eq!(compression_name(Compression::Rle4), "BI_RLE4");
        assert_eq!(compression_name(Compression::Rle8), "BI_RLE8");
        assert_eq!(compression_name(Compression::BitFields), "BI_BITFIELDS");
        assert_eq!(compression_name(Compression::Jpeg), "BI_JPEG");
        assert_eq!(compression_name(Compression::Png), "BI_PNG");
        assert_eq!(compression_name(Compression::Other(123)), "UNKNOWN");
    }

    #[test]
    fn decode_stats_report_memory_and_ratio() {
        let decoded = DecodedImage {
            width: 2,
            height: 1,
            rgba: vec![0; 8],
        };
        let mut out = String::new();
        write_decode_stats(&mut out, &decoded, 4);

        assert!(out.contains("Decoded RGBA buffer: 8 B"));
        assert!(out.contains("Decoded bytes per pixel: 4"));
        assert!(out.contains("Decode expansion ratio: 2.00x"));
    }
}
