//! LSB steganography: embed and extract arbitrary byte payloads in image pixels.
//!
//! # Header format (80 bits, stored as LSBs in the pixel buffer)
//!
//! ```text
//! Bit offset  Field           Width   Notes
//! ---------------------------------------------------------
//!  0          magic "STEG"   32 bits  0x53 0x54 0x45 0x47
//! 32          version         3 bits  0 = v1; any other value is rejected
//! 35          channel config 13 bits  base-9 packed: r + g*9 + b*81 + a*729
//! 48          payload_len    32 bits  u32 little-endian, bytes in payload
//! ---------------------------------------------------------
//! Total                      80 bits
//! ```
//!
//! The header itself is stored using the same channel/bit-depth configuration
//! it describes.  Detection therefore requires trying all 6561 valid configs.

use std::fmt;

use thiserror::Error;

use crate::runtime::decode::DecodedImage;

use super::model::{ImageTransform, TransformError, TransformOp};

// -----------------------------------------------------------------------------
// Public types
// -----------------------------------------------------------------------------

/// Per-channel LSB depths.  0 means the channel is skipped; 1-8 means that
/// many least-significant bits of the channel are used to carry steg data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StegConfig {
    pub r_bits: u8, // 0..=8
    pub g_bits: u8, // 0..=8
    pub b_bits: u8, // 0..=8
    pub a_bits: u8, // 0..=8
}

/// Metadata decoded from a detected steganography header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StegInfo {
    pub config: StegConfig,
    pub version: u8,
    pub payload_len: u32,
}

#[derive(Debug, Error)]
pub enum StegError {
    #[error("image too small: need {required} bits but only {capacity} available")]
    InsufficientCapacity { required: u64, capacity: u64 },

    #[error("no steganography header found")]
    NotFound,

    #[error("payload length in header ({payload_len} bytes) exceeds image capacity")]
    PayloadTooLarge { payload_len: u32 },

    #[error("unsupported steganography version {0}; only version 0 is supported")]
    UnsupportedVersion(u8),

    #[error("all channels have 0 bits configured; nothing to embed")]
    NoChannels,

    #[error("arithmetic overflow while processing steganography data: {0}")]
    ArithmeticOverflow(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EmbedSteganography {
    pub config: StegConfig,
    pub payload: Vec<u8>,
}

impl fmt::Display for EmbedSteganography {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Embed Steganography ({} bytes, R{}G{}B{}A{})",
            self.payload.len(),
            self.config.r_bits,
            self.config.g_bits,
            self.config.b_bits,
            self.config.a_bits
        )
    }
}

impl TransformOp for EmbedSteganography {
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        Ok(embed(image, self.config, &self.payload)?)
    }

    fn inverse(&self) -> Option<ImageTransform> {
        None
    }

    fn replay_cost(&self) -> u32 {
        2
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RemoveSteganography {
    pub config: StegConfig,
}

impl fmt::Display for RemoveSteganography {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Remove Steganography (R{}G{}B{}A{})",
            self.config.r_bits, self.config.g_bits, self.config.b_bits, self.config.a_bits
        )
    }
}

impl TransformOp for RemoveSteganography {
    fn apply(&self, image: &DecodedImage) -> Result<DecodedImage, TransformError> {
        Ok(remove(image, self.config))
    }

    fn inverse(&self) -> Option<ImageTransform> {
        None
    }

    fn replay_cost(&self) -> u32 {
        1
    }
}

// -----------------------------------------------------------------------------
// StegConfig helpers
// -----------------------------------------------------------------------------

/// Number of bits in the header, independent of config.
const HEADER_BITS: u64 = 80;

impl StegConfig {
    /// Total bits contributed by this config per pixel.
    #[must_use]
    pub const fn bits_per_pixel(self) -> u8 {
        self.r_bits + self.g_bits + self.b_bits + self.a_bits
    }

    /// Total number of bits available for steg data in an image (including the
    /// header).
    #[must_use]
    pub const fn total_bits(self, width: u32, height: u32) -> u64 {
        (width as u64)
            .saturating_mul(height as u64)
            .saturating_mul(self.bits_per_pixel() as u64)
    }

    /// Maximum payload in *bytes* that can be embedded (excluding the header).
    /// Returns 0 if the image is too small to even fit the header.
    #[must_use]
    pub const fn capacity_bytes(self, width: u32, height: u32) -> u64 {
        let total = self.total_bits(width, height);
        total.saturating_sub(HEADER_BITS) / 8
    }

    /// Encode the four 0..=8 channel values into a 13-bit base-9 integer.
    /// The result always fits in `u16` (max value 6560 < 8192 = 2^13).
    #[must_use]
    pub const fn encode_config_bits(self) -> u16 {
        self.r_bits as u16 + self.g_bits as u16 * 9 + self.b_bits as u16 * 81 + self.a_bits as u16 * 729
    }

    /// Decode a 13-bit base-9 integer back into a `StegConfig`.
    /// Returns `None` if `raw >= 6561` (out of valid base-9 range) or if any
    /// individual component decoded to > 8.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub const fn decode_config_bits(raw: u16) -> Option<Self> {
        if raw >= 6561 {
            return None;
        }
        let r = (raw % 9) as u8;
        let g = ((raw / 9) % 9) as u8;
        let b = ((raw / 81) % 9) as u8;
        let a = (raw / 729) as u8;
        // Paranoia: each component must be <= 8.
        if r > 8 || g > 8 || b > 8 || a > 8 {
            return None;
        }
        Some(Self {
            r_bits: r,
            g_bits: g,
            b_bits: b,
            a_bits: a,
        })
    }
}

// -----------------------------------------------------------------------------
// Bit-stream primitives
// -----------------------------------------------------------------------------

/// Iterator state that walks the pixel RGBA buffer, extracting or writing bits
/// from/to the LSBs of each enabled channel.
///
/// Channels are visited in R -> G -> B -> A order within each pixel.
/// Pixels are visited left-to-right, top-to-bottom (row-major).
/// Within each channel the LSB is emitted/consumed first (bit 0 first).
struct BitCursor {
    config: StegConfig,
    /// Current pixel index (0 = top-left).
    pixel_idx: usize,
    /// Which channel are we currently in? 0=R, 1=G, 2=B, 3=A.
    channel: u8,
    /// Which bit within the current channel (0 = LSB).
    bit_in_channel: u8,
}

impl BitCursor {
    fn new(config: StegConfig) -> Self {
        debug_assert!(
            config.bits_per_pixel() > 0,
            "BitCursor requires at least one active channel"
        );
        let mut s = Self {
            config,
            pixel_idx: 0,
            channel: 0,
            bit_in_channel: 0,
        };
        // Advance past any leading channels with 0 bits.
        s.skip_empty_channels();
        s
    }

    /// Returns `true` if there are no more bits to emit in the given pixel
    /// count.
    const fn check_exhausted(&self, total_pixels: usize) -> bool {
        self.pixel_idx >= total_pixels
    }

    /// How many bits per channel the current channel contributes.
    const fn current_channel_bits(&self) -> u8 {
        match self.channel {
            0 => self.config.r_bits,
            1 => self.config.g_bits,
            2 => self.config.b_bits,
            3 => self.config.a_bits,
            _ => 0,
        }
    }

    /// Advance `channel` (and optionally `pixel_idx`) past any channels that
    /// contribute 0 bits, so the cursor always sits on an active channel.
    const fn skip_empty_channels(&mut self) {
        loop {
            if self.channel >= 4 {
                self.channel = 0;
                self.pixel_idx += 1;
            }
            if self.current_channel_bits() > 0 {
                break;
            }
            self.channel += 1;
        }
    }

    /// Advance to the next bit position.
    const fn advance(&mut self) {
        self.bit_in_channel += 1;
        if self.bit_in_channel >= self.current_channel_bits() {
            self.bit_in_channel = 0;
            self.channel += 1;
            self.skip_empty_channels();
        }
    }

    /// Byte index into the RGBA flat buffer for the current channel of the
    /// current pixel.
    const fn byte_idx(&self) -> usize {
        self.pixel_idx * 4 + self.channel as usize
    }
}

/// Read a single bit from the flat RGBA buffer at the cursor position.
/// The cursor is NOT advanced.
#[must_use]
#[inline]
fn read_bit(rgba: &[u8], cursor: &BitCursor) -> u8 {
    let byte = rgba[cursor.byte_idx()];
    (byte >> cursor.bit_in_channel) & 1
}

/// Write a single bit into the flat RGBA buffer at the cursor position.
#[inline]
fn write_bit(rgba: &mut [u8], cursor: &BitCursor, bit: u8) {
    let idx = cursor.byte_idx();
    let mask = !(1u8 << cursor.bit_in_channel);
    rgba[idx] = (rgba[idx] & mask) | ((bit & 1) << cursor.bit_in_channel);
}

/// Read `n` bits (LSB first) from the buffer starting at cursor, advancing
/// the cursor.  `n` must be <= 64.
#[must_use]
fn read_bits(rgba: &[u8], cursor: &mut BitCursor, n: u8, total_pixels: usize) -> Option<u64> {
    debug_assert!(n <= 64);
    let mut value: u64 = 0;
    for i in 0..n {
        if cursor.check_exhausted(total_pixels) {
            return None;
        }
        let bit = u64::from(read_bit(rgba, cursor));
        value |= bit << i;
        cursor.advance();
    }
    Some(value)
}

/// Skip `n` bits from the buffer starting at `cursor`, advancing the cursor
/// without reading any value.
///
/// Returns `None` if the cursor reaches the end of the image before all bits
/// are skipped.
fn skip_bits(cursor: &mut BitCursor, n: u64, total_pixels: usize) -> Option<()> {
    for _ in 0..n {
        if cursor.check_exhausted(total_pixels) {
            return None;
        }
        cursor.advance();
    }
    Some(())
}

/// Write `n` bits (LSB first) into the buffer starting at cursor, advancing
/// the cursor.  `n` must be <= 64.
fn write_bits(rgba: &mut [u8], cursor: &mut BitCursor, value: u64, n: u8, total_pixels: usize) -> bool {
    for i in 0..n {
        if cursor.check_exhausted(total_pixels) {
            return false;
        }
        let bit = ((value >> i) & 1) as u8;
        write_bit(rgba, cursor, bit);
        cursor.advance();
    }
    true
}

// -----------------------------------------------------------------------------
// Header read / write
// -----------------------------------------------------------------------------

/// Write the 80-bit steg header into the RGBA buffer starting at bit-cursor
/// position 0, using `config` as the embedding parameters.
///
/// The cursor must already be positioned at the start (pixel 0, channel 0,
/// bit 0 for the first active channel).
fn write_header(
    rgba: &mut [u8],
    cursor: &mut BitCursor,
    config: StegConfig,
    payload_len: u32,
    total_pixels: usize,
) -> bool {
    // Magic: "STEG" = 0x53 0x54 0x45 0x47, 32 bits, LSB of each byte first.
    let magic: u32 = u32::from_le_bytes(*b"STEG");
    if !write_bits(rgba, cursor, u64::from(magic), 32, total_pixels) {
        return false;
    }
    // version: 3 bits, value 0.
    if !write_bits(rgba, cursor, 0, 3, total_pixels) {
        return false;
    }
    // config: 13 bits.
    let config_bits = u64::from(config.encode_config_bits());
    if !write_bits(rgba, cursor, config_bits, 13, total_pixels) {
        return false;
    }
    // payload_len: 32 bits, LE.
    if !write_bits(rgba, cursor, u64::from(payload_len), 32, total_pixels) {
        return false;
    }
    true
}

/// Try to read and validate the 80-bit header using `config` as the decoding
/// parameters.  Returns `Some(StegInfo)` on success, `None` on any mismatch.
///
/// Fails early: returns `None` as soon as the first byte of the magic is
/// wrong ('S' = 0x53).
fn try_read_header(image: &DecodedImage, config: StegConfig) -> Option<StegInfo> {
    let rgba = image.rgba();
    let total_pixels = image.pixel_count();
    let mut cursor = BitCursor::new(config);
    let mut bits_consumed: u64 = 0;

    let mut read_n = |n: u8| {
        let v = read_bits(rgba, &mut cursor, n, total_pixels)?;
        bits_consumed += u64::from(n);
        Some(v)
    };

    #[allow(clippy::cast_possible_truncation)]
    let mut read_u8 = || read_n(8).map(|v| v as u8);

    // Fail early: check one byte of magic at a time.
    // 'S' = 0x53
    if read_u8()? != b'S' {
        return None;
    }
    // 'T' = 0x54
    if read_u8()? != b'T' {
        return None;
    }
    // 'E' = 0x45
    if read_u8()? != b'E' {
        return None;
    }
    // 'G' = 0x47
    if read_u8()? != b'G' {
        return None;
    }

    // version: 3 bits.
    #[allow(clippy::cast_possible_truncation)]
    let version = read_n(3)? as u8;
    if version != 0 {
        return None;
    }

    // config: 13 bits.
    #[allow(clippy::cast_possible_truncation)]
    let config_raw = read_n(13)? as u16;
    // Validate: must decode back to a valid config AND must match the config
    // we are currently using for this brute-force attempt.
    let decoded_config = StegConfig::decode_config_bits(config_raw)?;
    if decoded_config != config {
        return None;
    }

    // payload_len: 32 bits.
    #[allow(clippy::cast_possible_truncation)]
    let payload_len = read_n(32)? as u32;

    // Sanity: the payload must fit in the remaining capacity.
    let total_bits = total_pixels as u64 * u64::from(config.bits_per_pixel());
    let remaining_bits = total_bits.saturating_sub(bits_consumed);
    if u64::from(payload_len) * 8 > remaining_bits {
        return None;
    }

    Some(StegInfo {
        config,
        version,
        payload_len,
    })
}

// -----------------------------------------------------------------------------
// Public API
// -----------------------------------------------------------------------------

/// Embed `payload` bytes into a clone of `image` using `config` as the LSB
/// parameters.
///
/// Returns a new `DecodedImage` with the payload hidden in the pixel LSBs.
/// The original image is not modified.
///
/// # Errors
/// Returns [`StegError`] if no channels are enabled, arithmetic overflows
/// occur, or payload capacity is insufficient.
#[allow(clippy::missing_panics_doc)]
pub fn embed(image: &DecodedImage, config: StegConfig, payload: &[u8]) -> Result<DecodedImage, StegError> {
    if config.bits_per_pixel() == 0 {
        return Err(StegError::NoChannels);
    }

    let total_pixels = image.pixel_count();
    let payload_len_u32 =
        u32::try_from(payload.len()).map_err(|_| StegError::ArithmeticOverflow("payload length cast"))?;
    let payload_bits = u64::from(payload_len_u32)
        .checked_mul(8)
        .ok_or(StegError::ArithmeticOverflow("payload bits"))?;
    let required_bits = HEADER_BITS
        .checked_add(payload_bits)
        .ok_or(StegError::ArithmeticOverflow("required bits"))?;
    let available_bits = u64::try_from(total_pixels)
        .map_err(|_| StegError::ArithmeticOverflow("total pixel count cast"))?
        .checked_mul(u64::from(config.bits_per_pixel()))
        .ok_or(StegError::ArithmeticOverflow("available bits"))?;

    if required_bits > available_bits {
        return Err(StegError::InsufficientCapacity {
            required: required_bits,
            capacity: available_bits,
        });
    }

    let mut rgba = image.rgba().to_vec();
    let mut cursor = BitCursor::new(config);

    // Write the 80-bit header.
    if !write_header(rgba.as_mut_slice(), &mut cursor, config, payload_len_u32, total_pixels) {
        return Err(StegError::InsufficientCapacity {
            required: required_bits,
            capacity: available_bits,
        });
    }

    // Write payload bytes, each 8 bits LSB-first.
    for &byte in payload {
        if !write_bits(rgba.as_mut_slice(), &mut cursor, u64::from(byte), 8, total_pixels) {
            return Err(StegError::InsufficientCapacity {
                required: required_bits,
                capacity: available_bits,
            });
        }
    }

    Ok(DecodedImage::new(image.width(), image.height(), rgba)
        .expect("embed preserves source dimensions and RGBA buffer length"))
}

/// Remove embedded steganography from `image` for the given `config`.
///
/// If a valid header is present for `config`, only the exact bit range used by
/// that header plus payload is zeroed. If no valid header is found, the image
/// is returned unchanged.
#[must_use]
#[allow(clippy::missing_panics_doc)]
pub fn remove(image: &DecodedImage, config: StegConfig) -> DecodedImage {
    if config.bits_per_pixel() == 0 {
        return image.clone();
    }

    let Some(info) = try_read_header(image, config) else {
        return image.clone();
    };

    let mut rgba = image.rgba().to_vec();
    let total_pixels = image.pixel_count();

    let bits_to_clear = HEADER_BITS + u64::from(info.payload_len) * 8;
    let mut cursor = BitCursor::new(config);

    for _ in 0..bits_to_clear {
        if cursor.check_exhausted(total_pixels) {
            break;
        }
        write_bit(rgba.as_mut_slice(), &cursor, 0);
        cursor.advance();
    }

    DecodedImage::new(image.width(), image.height(), rgba)
        .expect("remove preserves source dimensions and RGBA buffer length")
}

/// Extract the payload from `image` using the config described in `info`.
///
/// The caller should have obtained `info` from [`detect`] or the GUI.
///
/// # Errors
/// Returns [`StegError`] if channel config is invalid or payload/header bounds
/// checks fail.
pub fn extract(image: &DecodedImage, info: &StegInfo) -> Result<Vec<u8>, StegError> {
    let config = info.config;
    if config.bits_per_pixel() == 0 {
        return Err(StegError::NoChannels);
    }

    let total_pixels = image.pixel_count();
    let total_bits = u64::try_from(total_pixels)
        .map_err(|_| StegError::ArithmeticOverflow("total pixel count cast"))?
        .checked_mul(u64::from(config.bits_per_pixel()))
        .ok_or(StegError::ArithmeticOverflow("total bits"))?;
    let remaining_bits = total_bits.saturating_sub(HEADER_BITS);

    let payload_bits = u64::from(info.payload_len)
        .checked_mul(8)
        .ok_or(StegError::ArithmeticOverflow("payload bits"))?;

    if payload_bits > remaining_bits {
        return Err(StegError::PayloadTooLarge {
            payload_len: info.payload_len,
        });
    }

    // Skip past the header (80 bits) - we trust the passed info
    let mut cursor = BitCursor::new(config);
    skip_bits(&mut cursor, 80, total_pixels).ok_or(StegError::PayloadTooLarge {
        payload_len: info.payload_len,
    })?;

    let mut payload = Vec::with_capacity(info.payload_len as usize);
    for _ in 0..info.payload_len {
        #[allow(clippy::cast_possible_truncation)]
        let byte = read_bits(image.rgba(), &mut cursor, 8, total_pixels).ok_or(StegError::PayloadTooLarge {
            payload_len: info.payload_len,
        })? as u8;
        payload.push(byte);
    }

    Ok(payload)
}

/// Brute-force all 6561 valid `StegConfig` combinations to find a valid
/// steganography header in `image`.
///
/// Returns the first `StegInfo` that passes all header checks, or `None` if
/// the image does not appear to contain steganography.
#[must_use]
pub fn detect(image: &DecodedImage) -> Option<StegInfo> {
    // Need at least enough pixels to hold the header.
    // Even in the best case (all channels, 8 bits each = 32 bits/pixel), that
    // is ceil(80/32) = 3 pixels.  We'll let the individual reads handle the
    // capacity check.

    for a_bits in 0u8..=8 {
        for b_bits in 0u8..=8 {
            for g_bits in 0u8..=8 {
                for r_bits in 0u8..=8 {
                    let config = StegConfig {
                        r_bits,
                        g_bits,
                        b_bits,
                        a_bits,
                    };
                    if config.bits_per_pixel() == 0 {
                        continue;
                    }
                    if let Some(info) = try_read_header(image, config) {
                        return Some(info);
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{BitCursor, StegConfig, StegError, StegInfo, detect, embed, extract, remove, write_bit};
    use crate::runtime::decode::DecodedImage;

    fn patterned_image(width: u32, height: u32) -> DecodedImage {
        let mut rgba = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for y in 0..height {
            #[allow(clippy::cast_possible_truncation)]
            for x in 0..width {
                rgba.push(((x * 31 + y * 17 + 11) % 256) as u8);
                rgba.push(((x * 19 + y * 29 + 53) % 256) as u8);
                rgba.push(((x * 43 + y * 7 + 97) % 256) as u8);
                rgba.push(((x * 13 + y * 37 + 191) % 256) as u8);
            }
        }
        DecodedImage::new(width, height, rgba).expect("valid patterned image")
    }

    #[test]
    fn config_encoding_roundtrips_for_all_valid_values() {
        for raw in 0u16..6561 {
            let config = StegConfig::decode_config_bits(raw).expect("raw value should decode");
            assert_eq!(config.encode_config_bits(), raw);
        }
        assert!(StegConfig::decode_config_bits(6561).is_none());
        assert!(StegConfig::decode_config_bits(8191).is_none());
    }

    #[test]
    fn config_capacity_helpers_behave_as_expected() {
        let config = StegConfig {
            r_bits: 1,
            g_bits: 2,
            b_bits: 3,
            a_bits: 0,
        };
        assert_eq!(config.bits_per_pixel(), 6);
        assert_eq!(config.total_bits(10, 5), 300);
        assert_eq!(config.capacity_bytes(10, 5), 27);

        let tiny = StegConfig {
            r_bits: 1,
            g_bits: 0,
            b_bits: 0,
            a_bits: 0,
        };
        assert_eq!(tiny.capacity_bytes(2, 2), 0);
    }

    #[test]
    fn embed_detect_extract_roundtrip_succeeds() {
        let image = patterned_image(16, 16);
        let config = StegConfig {
            r_bits: 2,
            g_bits: 1,
            b_bits: 2,
            a_bits: 0,
        };
        let payload = b"steganography test payload";

        let embedded = embed(&image, config, payload).expect("embed should succeed");
        let info = detect(&embedded).expect("detect should find embedded payload");
        assert_eq!(info.config, config);
        assert_eq!(info.version, 0);
        assert_eq!(info.payload_len, u32::try_from(payload.len()).unwrap());

        let extracted = extract(&embedded, &info).expect("extract should succeed");
        assert_eq!(extracted, payload);
    }

    #[test]
    fn embed_does_not_modify_original_image() {
        let image = patterned_image(8, 8);
        let config = StegConfig {
            r_bits: 1,
            g_bits: 1,
            b_bits: 1,
            a_bits: 1,
        };
        let original = image.rgba().to_vec();

        let _embedded = embed(&image, config, b"abc").expect("embed should succeed");
        assert_eq!(image.rgba(), original);
    }

    #[test]
    fn detect_returns_none_when_no_header_is_present() {
        let clean = patterned_image(8, 8);
        assert!(detect(&clean).is_none());
    }

    #[test]
    fn remove_clears_only_header_and_payload_bits() {
        let image = patterned_image(12, 12);
        let config = StegConfig {
            r_bits: 3,
            g_bits: 2,
            b_bits: 1,
            a_bits: 0,
        };
        let payload = b"hidden";
        let embedded = embed(&image, config, payload).expect("embed should succeed");
        let stripped = remove(&embedded, config);

        assert!(detect(&stripped).is_none());

        let mut expected = embedded.rgba().to_vec();
        let mut cursor = BitCursor::new(config);
        let bits_to_clear = 80_u64 + (payload.len() as u64) * 8;
        for _ in 0..bits_to_clear {
            write_bit(expected.as_mut_slice(), &cursor, 0);
            cursor.advance();
        }

        assert_eq!(stripped.rgba(), expected);
    }

    #[test]
    fn remove_leaves_image_unchanged_when_no_valid_header_exists() {
        let image = patterned_image(12, 12);
        let config = StegConfig {
            r_bits: 2,
            g_bits: 1,
            b_bits: 0,
            a_bits: 0,
        };

        let stripped = remove(&image, config);
        assert_eq!(stripped.rgba(), image.rgba());
        assert_eq!(stripped.dimensions(), image.dimensions());
    }

    #[test]
    fn embed_rejects_no_channels_config() {
        let image = patterned_image(8, 8);
        let config = StegConfig {
            r_bits: 0,
            g_bits: 0,
            b_bits: 0,
            a_bits: 0,
        };

        let err = embed(&image, config, b"x").expect_err("must reject no-channel config");
        assert!(matches!(err, StegError::NoChannels));
    }

    #[test]
    fn embed_rejects_insufficient_capacity() {
        let image = patterned_image(2, 2);
        let config = StegConfig {
            r_bits: 1,
            g_bits: 0,
            b_bits: 0,
            a_bits: 0,
        };

        let err = embed(&image, config, b"").expect_err("must reject when header cannot fit");
        assert!(matches!(
            err,
            StegError::InsufficientCapacity {
                required: 80,
                capacity: 4
            }
        ));
    }

    #[test]
    fn extract_rejects_no_channels_config() {
        let image = patterned_image(8, 8);
        let info = StegInfo {
            config: StegConfig {
                r_bits: 0,
                g_bits: 0,
                b_bits: 0,
                a_bits: 0,
            },
            version: 0,
            payload_len: 0,
        };

        let err = extract(&image, &info).expect_err("must reject no-channel config");
        assert!(matches!(err, StegError::NoChannels));
    }

    #[test]
    fn extract_rejects_payload_too_large_from_header_info() {
        let image = patterned_image(8, 8);
        let info = StegInfo {
            config: StegConfig {
                r_bits: 1,
                g_bits: 1,
                b_bits: 1,
                a_bits: 0,
            },
            version: 0,
            payload_len: u32::MAX,
        };

        let err = extract(&image, &info).expect_err("payload must be rejected if it does not fit");
        assert!(matches!(err, StegError::PayloadTooLarge { .. }));
    }
}
