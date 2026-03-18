//! Formatting helpers and cached inspection data shown in the inspector panel.

use std::fmt::Write as _;

use bmp::{
    raw::{Bmp, Compression},
    runtime::decode::DecodedImage,
};

pub(super) struct BmpInfoSections {
    pub(super) image_stats: String,
    pub(super) decoded_stats: String,
}

#[derive(Default)]
/// UI-facing metadata derived from the currently loaded BMP.
pub(in crate::gui) struct DocumentInspection {
    pub(in crate::gui) image_stats: String,
    pub(in crate::gui) decoded_stats: String,
    pub(in crate::gui) palette_colors: Vec<[u8; 4]>,
}

/// Extracts any color table entries for palette-based BMP variants.
pub(super) fn extract_palette_colors(bmp: &Bmp) -> Vec<[u8; 4]> {
    match bmp {
        Bmp::Core(data) => data.color_table.iter().map(|c| [c.red, c.green, c.blue, 255]).collect(),
        Bmp::Info(data) => data.color_table.iter().map(|c| [c.red, c.green, c.blue, 255]).collect(),
        Bmp::V4(data) => data.color_table.iter().map(|c| [c.red, c.green, c.blue, 255]).collect(),
        Bmp::V5(data) => data.color_table.iter().map(|c| [c.red, c.green, c.blue, 255]).collect(),
    }
}

/// Formats an integer with thousands grouping using commas.
///
/// The function inserts a comma every three digits counting from the
/// least significant digit, producing a human-readable representation
/// of large numbers.
///
/// This is used for display purposes in UI metadata (e.g. byte counts)
/// where improved readability is desirable.
///
/// The implementation constructs the grouped string by iterating over
/// the digits in reverse order and inserting separators every three
/// characters, then reversing the result back to normal order.
///
/// # Examples
///
/// ```
/// assert_eq!(with_grouping(0), "0");
/// assert_eq!(with_grouping(12), "12");
/// assert_eq!(with_grouping(1_234), "1,234");
/// assert_eq!(with_grouping(12_345_678), "12,345,678");
/// ```
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

/// Formats a byte count into a human-readable string using binary units.
///
/// The output always includes the exact byte count with thousands grouping,
/// followed by a scaled representation in binary units (`KiB`, `MiB`, `GiB`,
/// `TiB`) when appropriate.
///
/// The scaled value is shown with two decimal places and computed entirely
/// using integer arithmetic to avoid floating-point precision issues.
///
/// # Examples
///
/// ```
/// assert_eq!(format_bytes(999), "999 B");
/// assert_eq!(format_bytes(1_024), "1,024 B (1.00 KiB)");
/// assert_eq!(format_bytes(1_536), "1,536 B (1.50 KiB)");
/// assert_eq!(format_bytes(5 * 1_048_576), "5,242,880 B (5.00 MiB)");
/// ```
///
/// # Notes
///
/// * Binary units are used (`1 KiB = 1024 B`).
/// * The fractional part is truncated to two decimal places rather than
///   rounded, which keeps the implementation simple and fully integer-based.
fn format_bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

    let mut scaled = value;
    let mut unit = 0usize;

    while scaled >= 1024 && unit < UNITS.len() - 1 {
        scaled /= 1024;
        unit += 1;
    }

    if unit == 0 {
        return format!("{} B", with_grouping(value));
    }

    let divisor = 1024u64.pow(u32::try_from(unit).unwrap());
    let whole = value / divisor;
    let remainder = value % divisor;

    let fraction = (remainder * 100) / divisor;

    format!("{} B ({}.{:02} {})", with_grouping(value), whole, fraction, UNITS[unit])
}

const fn compression_name(compression: Compression) -> &'static str {
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
    let decoded_bytes = decoded.rgba().len() as u64;
    let _ = writeln!(out, "Decoded RGBA buffer: {}", format_bytes(decoded_bytes));
    let _ = writeln!(out, "Decoded bytes per pixel: 4");

    if encoded_pixel_bytes > 0 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "ratio is used only for human-readable diagnostics and printed with two decimal places"
        )]
        let ratio = decoded_bytes as f64 / encoded_pixel_bytes as f64;
        let _ = writeln!(out, "Decode expansion ratio: {ratio:.2}x");
    }
}

const fn encoded_pixel_bytes(bmp: &Bmp) -> usize {
    match bmp {
        Bmp::Core(data) => data.bitmap_array.len(),
        Bmp::Info(data) => data.bitmap_array.len(),
        Bmp::V4(data) => data.bitmap_array.len(),
        Bmp::V5(data) => data.bitmap_array.len(),
    }
}

const fn orientation_from_height(height: i32) -> &'static str {
    if height < 0 { "top-down" } else { "bottom-up" }
}

#[expect(
    clippy::too_many_arguments,
    reason = "metadata output keeps explicit fields for readability"
)]
fn write_common_section(
    out: &mut String,
    variant: &str,
    width: u32,
    height: u32,
    orientation: &str,
    bits_per_pixel: u16,
    compression_line: &str,
    header_image_size: Option<u32>,
    pixel_data_size: usize,
    palette_entries: usize,
    file_size: u32,
    pixel_data_offset: u32,
) {
    let _ = writeln!(out, "Variant: {variant}");
    let _ = writeln!(out, "Size: {width} x {height} px");
    let _ = writeln!(out, "Orientation: {orientation}");
    let _ = writeln!(out, "Bits per pixel: {bits_per_pixel}");
    let _ = writeln!(out, "Compression: {compression_line}");
    if let Some(image_size) = header_image_size {
        let _ = writeln!(out, "Header image_size: {}", format_bytes(u64::from(image_size)));
    }
    let _ = writeln!(out, "Pixel data size: {}", format_bytes(pixel_data_size as u64));
    let _ = writeln!(out, "Palette entries: {palette_entries}");
    let _ = writeln!(out, "File size: {}", format_bytes(u64::from(file_size)));
    let _ = writeln!(out, "Pixel data offset: {}", format_bytes(u64::from(pixel_data_offset)));
}

/// Formats the inspector's human-readable BMP metadata sections.
pub(super) fn format_bmp_info_sections(bmp: &Bmp, decoded: &DecodedImage) -> BmpInfoSections {
    let mut out = String::new();
    match bmp {
        Bmp::Core(data) => {
            write_common_section(
                &mut out,
                "CORE (BITMAPCOREHEADER)",
                u32::from(data.bmp_header.width),
                u32::from(data.bmp_header.height),
                "bottom-up",
                data.bmp_header.bit_count.bit_count(),
                "BI_RGB (implicit)",
                None,
                data.bitmap_array.len(),
                data.color_table.len(),
                data.file_header.file_size,
                data.file_header.pixel_data_offset,
            );
        }
        Bmp::Info(data) => {
            let h = data.bmp_header;
            let compression_line = format!("{} ({:?})", compression_name(h.compression), h.compression);
            write_common_section(
                &mut out,
                "INFO (BITMAPINFOHEADER)",
                decoded.width(),
                decoded.height(),
                orientation_from_height(h.height),
                h.bit_count.bit_count(),
                &compression_line,
                Some(h.image_size),
                data.bitmap_array.len(),
                data.color_table.len(),
                data.file_header.file_size,
                data.file_header.pixel_data_offset,
            );
            if let Some(masks) = data.color_masks {
                let _ = writeln!(
                    &mut out,
                    "Bit masks: R={:#010X} G={:#010X} B={:#010X}",
                    masks.red_mask, masks.green_mask, masks.blue_mask
                );
            }
        }
        Bmp::V4(data) => {
            let h = data.bmp_header.info;
            let m = data.bmp_header.masks;
            let compression_line = format!("{} ({:?})", compression_name(h.compression), h.compression);
            write_common_section(
                &mut out,
                "V4 (BITMAPV4HEADER)",
                decoded.width(),
                decoded.height(),
                orientation_from_height(h.height),
                h.bit_count.bit_count(),
                &compression_line,
                Some(h.image_size),
                data.bitmap_array.len(),
                data.color_table.len(),
                data.file_header.file_size,
                data.file_header.pixel_data_offset,
            );
            let _ = writeln!(
                &mut out,
                "Bit masks: R={:#010X} G={:#010X} B={:#010X} A={:#010X}",
                m.red_mask, m.green_mask, m.blue_mask, m.alpha_mask
            );
            let _ = writeln!(&mut out, "Color space: {:?}", data.bmp_header.cs_type);
        }
        Bmp::V5(data) => {
            let h = data.bmp_header.v4.info;
            let m = data.bmp_header.v4.masks;
            let compression_line = format!("{} ({:?})", compression_name(h.compression), h.compression);
            write_common_section(
                &mut out,
                "V5 (BITMAPV5HEADER)",
                decoded.width(),
                decoded.height(),
                orientation_from_height(h.height),
                h.bit_count.bit_count(),
                &compression_line,
                Some(h.image_size),
                data.bitmap_array.len(),
                data.color_table.len(),
                data.file_header.file_size,
                data.file_header.pixel_data_offset,
            );
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
                format_bytes(u64::from(data.bmp_header.profile_data))
            );
            let _ = writeln!(
                &mut out,
                "Profile size: {}",
                format_bytes(u64::from(data.bmp_header.profile_size))
            );
            let _ = writeln!(
                &mut out,
                "ICC profile bytes loaded: {}",
                format_bytes(data.icc_profile.as_ref().map_or(0, Vec::len) as u64)
            );
        }
    }

    let encoded_bytes = encoded_pixel_bytes(bmp);
    let mut decoded_stats = String::new();
    write_decode_stats(&mut decoded_stats, decoded, encoded_bytes);

    BmpInfoSections {
        image_stats: out,
        decoded_stats,
    }
}

impl DocumentInspection {
    /// Builds inspector-facing derived data for a newly loaded BMP.
    pub(in crate::gui) fn from_bmp(bmp: &Bmp, decoded: &DecodedImage) -> Self {
        let info = format_bmp_info_sections(bmp, decoded);
        Self {
            image_stats: info.image_stats,
            decoded_stats: info.decoded_stats,
            palette_colors: extract_palette_colors(bmp),
        }
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
        let decoded = DecodedImage::new(2, 1, vec![0; 8]).expect("valid decoded image");
        let mut out = String::new();
        write_decode_stats(&mut out, &decoded, 4);

        assert!(out.contains("Decoded RGBA buffer: 8 B"));
        assert!(out.contains("Decoded bytes per pixel: 4"));
        assert!(out.contains("Decode expansion ratio: 2.00x"));
    }
}
