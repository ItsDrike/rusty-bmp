use std::io::{self, Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

/// A 2.30 fixed-point number.
///
/// This format uses 2 integer bits and 30 fractional bits, together stored as a
/// 32-bit unsigned integer.
///
/// This format is used in a BMP header to represent CIE XYZ color space
/// endpoint coordinates.
///
/// Example:
/// raw value: 0x40000000 -> 1.0.
/// raw value: 0x48000000 -> 1.5.
/// raw value: 0xFFFFFFFF -> just a smidgen under 4.0.
///
/// In the Microsoft documentation (wingdi.h), this is referred to as the
/// `FXPT2DOT30` structure.
///
/// ## Assumed meaning
///
/// Note that the Microsoft documentation does not actually explain this format
/// beyond saying it's a "fix point (2.30)" format. This implementation follows
/// a Stack Overflow question about this, which links to a 'Programming Windows'
/// book, which apparently explains this format. I did not read the book, but I
/// trust the Stack Overflow answer for this, which explains the format as
/// having 2-bit integer part and a 30-bit fractional part. Ref:
/// <https://stackoverflow.com/questions/20864752/how-is-defined-the-data-type-fxpt2dot30-in-the-bmp-file-structure>
///
/// Though this still does not mention anything more, not even whether or not
/// the format is meant to be signed or not. From Wine's open-source repo
/// holding a copy of the wingdi.h file:
/// <https://github.com/wine-mirror/wine/blob/222c976140d1b66c71769296f856f6523782b6c9/include/wingdi.h#L160>
/// It's defined with a type-def from LONG, which would somewhat imply it's a
/// signed value.
///
/// However, following a description of the DIB format from the webpage:
/// <https://flylib.com/books/en/4.267.1.73/1/>
/// It describes this format as having a maximum value of just under 4.0 for
/// 0xFFFFFFFF. This would imply it being unsigned.
///
/// Additionally, when the meaning is considered (being a CIE XYZ endpoint),
/// negative values are a bit odd to see, so that would tell us it's probably
/// unsigned, then again, values above 2.0 (max if it were signed) don't really
/// make much sense for CIE XYZ endpoint values either.
///
/// This implementation makes an assumption that the value is unsigned, but
/// that's all it is, an undocumented assumption. For most cases, it should be
/// fine, as again, CIE XYZ values should realistically be positive and never
/// cross 2.0, so whether or not we represent it as signed or unsigned, it's
/// likely that we won't even get to values where this becomes a concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FixedPoint2Dot30 {
    /// The raw fixed point value.
    raw: u32,
}

impl FixedPoint2Dot30 {
    pub const FRACTIONAL_BITS: u32 = 30;

    pub const SCALE_F64: f64 = (1u64 << Self::FRACTIONAL_BITS) as f64;
    pub const SCALE_F32: f32 = (1u32 << Self::FRACTIONAL_BITS) as f32;

    /// Creates a fixed-point value from its raw 32-bit representation.
    ///
    /// The caller is responsible for ensuring that the value follows the
    /// 2.30 fixed-point interpretation.
    pub const fn from_raw(raw: u32) -> Self {
        Self { raw }
    }

    /// Returns the underlying raw 32-bit representation.
    pub const fn raw(self) -> u32 {
        self.raw
    }

    /// Converts the fixed-point value to `f64`.
    ///
    /// The conversion is performed as:
    ///
    /// `value = raw / 2^30`
    ///
    /// This conversion is exact for all representable 2.30 values.
    pub const fn to_f64(self) -> f64 {
        self.raw as f64 / Self::SCALE_F64
    }

    /// Converts the fixed-point value to `f32`.
    ///
    /// Note that `f32` has fewer mantissa bits (23) than the 30 fractional
    /// bits stored in this format, so precision may be lost.
    pub const fn to_f32(self) -> f32 {
        self.raw as f32 / Self::SCALE_F32
    }

    /// Attempts to encode an `f64` as an unsigned 2.30 fixed-point value.
    ///
    /// The value is scaled by `2^30` and rounded to the nearest integer.
    /// Returns `None` if the input is not finite, or is outside the
    /// representable range (`0.0 ..= u32::MAX / 2^30` ~= 0..4).
    pub fn try_from_f64(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }

        let scaled = (value * Self::SCALE_F64).round();
        if scaled < 0.0 || scaled > u32::MAX as f64 {
            return None;
        }

        Some(Self { raw: scaled as u32 })
    }

    /// Encodes an `f64` as an unsigned 2.30 fixed-point value, saturating on overflow.
    ///
    /// The value is scaled by `2^30` and rounded to the nearest integer.
    /// Values outside of the representable range are clamped to the nearest
    /// representable value (`0.0 ..= u32::MAX / 2^30` ~= 0..4).
    ///
    /// Non-finite values are represented as 0.
    pub fn from_f64_clamped(value: f64) -> Self {
        if !value.is_finite() {
            return Self { raw: 0 };
        }

        let scaled = (value * Self::SCALE_F64).round();
        let clamped = scaled.clamp(0.0, u32::MAX as f64);

        Self { raw: clamped as u32 }
    }
}

impl core::fmt::Display for FixedPoint2Dot30 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.to_f64())
    }
}

impl FixedPoint2Dot30 {
    pub(crate) fn read<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        let raw = reader.read_u32::<LittleEndian>()?;
        Ok(Self { raw })
    }

    pub(crate) fn write<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_u32::<LittleEndian>(self.raw)
    }
}

/// An unsigned 16.16 fixed-point number.
///
/// This format uses the upper 16 bits as the unsigned integer value, and the
/// lower 16 bits for the fractional part, stored as a 32-bit unsigned integer.
///
/// Numeric range: [0.0, 65536.0)
///
/// Example:
///
/// raw value: 0x00010000 -> 1.0
/// raw value: 0x00018000 -> 1.5
/// raw value: 0x0000C000 -> 0.75
///
/// In the Microsoft documentation (wingdi.h), this is only documented in the
/// description of the gamma fields in the BMP v4+ header, and this type is not
/// given it's own type definition. But it's meaning is explained sufficiently
/// in the field descriptions.
/// Reference:
/// <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-bitmapv4header>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FixedPoint16Dot16 {
    /// The raw fixed point value.
    raw: u32,
}

impl FixedPoint16Dot16 {
    pub const FRACTIONAL_BITS: u32 = 16;

    pub const SCALE_F64: f64 = (1u64 << Self::FRACTIONAL_BITS) as f64;
    pub const SCALE_F32: f32 = (1u32 << Self::FRACTIONAL_BITS) as f32;

    /// Creates a fixed-point value from its raw 32-bit representation.
    pub const fn from_raw(raw: u32) -> Self {
        Self { raw }
    }

    /// Returns the underlying raw representation.
    pub const fn raw(self) -> u32 {
        self.raw
    }

    /// Converts the fixed-point value to `f64`.
    ///
    /// Computed as: `raw / 2^16`.
    pub const fn to_f64(self) -> f64 {
        self.raw as f64 / Self::SCALE_F64
    }

    /// Converts the fixed-point value to `f32`.
    ///
    /// Computed as: `raw / 2^16`.
    pub const fn to_f32(self) -> f32 {
        self.raw as f32 / Self::SCALE_F32
    }

    /// Attempts to encode an `f64` as an unsigned 16.16 fixed-point value.
    ///
    /// The value is scaled by `2^16` and rounded to the nearest integer.
    /// Returns `None` if the input is not finite, or is outside the
    /// representable range (`0.0 ..= u32::MAX / 2^16` ~= 0..65536).
    pub fn try_from_f64(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }

        let scaled = (value * Self::SCALE_F64).round();
        if scaled < 0.0 || scaled > u32::MAX as f64 {
            return None;
        }

        Some(Self { raw: scaled as u32 })
    }

    /// Encodes an `f64` as an unsigned 16.16 fixed-point value, saturating on overflow.
    ///
    ///
    /// The value is scaled by `2^16` and rounded to the nearest integer.
    /// Values outside of the representable range are clamped to the nearest
    /// representable value (`0.0 ..= u32::MAX / 2^16` ~= 0..65536).
    ///
    /// Non-finite values are represented as 0.
    pub fn from_f64_clamped(value: f64) -> Self {
        if !value.is_finite() {
            return Self { raw: 0 };
        }

        let scaled = (value * Self::SCALE_F64).round();
        let clamped = scaled.clamp(0.0, u32::MAX as f64);

        Self { raw: clamped as u32 }
    }
}

impl core::fmt::Display for FixedPoint16Dot16 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.to_f64())
    }
}

impl FixedPoint16Dot16 {
    pub(crate) fn read<R: Read>(reader: &mut R) -> io::Result<Self> {
        let raw = reader.read_u32::<LittleEndian>()?;
        Ok(Self { raw })
    }

    pub(crate) fn write<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_u32::<LittleEndian>(self.raw)
    }
}
