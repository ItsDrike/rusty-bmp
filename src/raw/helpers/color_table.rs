//! Internal color-table helper type used by raw BMP implementations.
//!
//! This module centralizes color-table reading, writing, and validation logic,
//! so internal callers like `Bmp` can share one implementation across header
//! variants. It is intentionally crate-internal and not part of the public
//! API surface.

use std::{
    io::{Read, Write},
    sync::Arc,
};

use crate::raw::{BitmapHeader, IoStage, RgbQuad, RgbTriple, StructuralError, ValidationError};

const MAX_COLOR_TABLE_ENTRIES: usize = 1 << 16; // 65_536 entries (just enough for 16-bpp platted images)

/// Internal color-table representation used by crate-internal BMP code paths.
///
/// This helper exists to avoid duplicating per-variant table logic in parser,
/// serializer, and validator implementations.
pub(in crate::raw) enum ColorTable {
    /// CORE/V2 table entries (`RGBTRIPLE`).
    Core(Arc<[RgbTriple]>),
    /// INFO+/V3+ table entries (`RGBQUAD`).
    InfoOrLater(Arc<[RgbQuad]>),
}

/// Internal adapter that yields all color table entries as `RgbQuad` values.
///
/// CORE entries are converted on the fly with `reserved = 0`.
enum ColorTableRgbQuadIter<'a> {
    /// Iterator over `RGBTRIPLE` entries.
    Core(std::slice::Iter<'a, RgbTriple>),
    /// Iterator over `RGBQUAD` entries.
    InfoOrLater(std::slice::Iter<'a, RgbQuad>),
}

impl Iterator for ColorTableRgbQuadIter<'_> {
    type Item = RgbQuad;

    /// Returns the next entry as an `RgbQuad`.
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Core(iter) => iter.next().copied().map(RgbQuad::from),
            Self::InfoOrLater(iter) => iter.next().copied(),
        }
    }

    /// Returns exact lower/upper bounds for remaining entries.
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl ExactSizeIterator for ColorTableRgbQuadIter<'_> {
    /// Returns the exact number of remaining entries.
    fn len(&self) -> usize {
        match self {
            Self::Core(iter) => iter.len(),
            Self::InfoOrLater(iter) => iter.len(),
        }
    }
}

impl ColorTable {
    /// Reads a color table from `reader` based on the provided BMP header type.
    ///
    /// Uses only structural checks required for safe parsing.
    ///
    /// # Errors
    /// Returns [`StructuralError`] for I/O failures, arithmetic issues, or when
    /// the declared table is too large for configured safety limits.
    pub(in crate::raw) fn read_unchecked<R: Read>(
        reader: &mut R,
        header: &BitmapHeader,
    ) -> Result<Self, StructuralError> {
        let entry_count = header.color_table_size()? as usize;
        if entry_count > MAX_COLOR_TABLE_ENTRIES {
            return Err(StructuralError::StructureUnsafe(format!(
                "Color table contains {entry_count} entries, which is higher than the allowed safe maximum: {MAX_COLOR_TABLE_ENTRIES}"
            )));
        }

        match header {
            BitmapHeader::Core(_) => {
                let mut color_table: Vec<RgbTriple> = Vec::with_capacity(entry_count);

                for _ in 0..entry_count {
                    color_table.push(
                        RgbTriple::read(reader)
                            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingColorTable))?,
                    );
                }

                Ok(Self::Core(Arc::from(color_table)))
            }
            BitmapHeader::Info(_) | BitmapHeader::V4(_) | BitmapHeader::V5(_) => {
                let mut color_table: Vec<RgbQuad> = Vec::with_capacity(entry_count);

                for _ in 0..entry_count {
                    color_table.push(
                        RgbQuad::read_unchecked(reader)
                            .map_err(|e| StructuralError::from_io(e, IoStage::ReadingColorTable))?,
                    );
                }

                Ok(Self::InfoOrLater(Arc::from(color_table)))
            }
        }
    }

    /// Writes this color table to `writer` without semantic re-validation.
    ///
    /// # Errors
    /// Returns [`StructuralError`] for I/O failures while writing entries.
    pub(in crate::raw) fn write_unchecked<W: Write>(&self, writer: &mut W) -> Result<(), StructuralError> {
        match self {
            Self::Core(color_table) => {
                for entry in color_table.iter() {
                    entry
                        .write(writer)
                        .map_err(|e| StructuralError::from_io(e, IoStage::ReadingColorTable))?;
                }
            }
            Self::InfoOrLater(color_table) => {
                for entry in color_table.iter() {
                    entry
                        .write_unchecked(writer)
                        .map_err(|e| StructuralError::from_io(e, IoStage::ReadingColorTable))?;
                }
            }
        }

        Ok(())
    }

    #[inline]
    #[must_use]
    /// Returns the number of entries stored in this table.
    pub(in crate::raw) fn len(&self) -> usize {
        match self {
            Self::Core(color_table) => color_table.len(),
            Self::InfoOrLater(color_table) => color_table.len(),
        }
    }

    #[inline]
    #[must_use]
    /// Returns a view of entries normalized to `RgbQuad` values.
    fn rgb_quads(&self) -> ColorTableRgbQuadIter<'_> {
        match self {
            Self::Core(color_table) => ColorTableRgbQuadIter::Core(color_table.iter()),
            Self::InfoOrLater(color_table) => ColorTableRgbQuadIter::InfoOrLater(color_table.iter()),
        }
    }

    /// Validates per-entry invariants.
    ///
    /// CORE tables have no per-entry reserved-byte invariant (nothing to validate),
    /// while INFO+/V3+ entries are validated as `RgbQuad`s.
    fn validate_entries(&self) -> Result<(), ValidationError> {
        if matches!(self, Self::Core(_)) {
            return Ok(());
        }

        for rgb_quad in self.rgb_quads() {
            rgb_quad.validate()?;
        }

        Ok(())
    }

    /// Validates table size against the header and validates table entries.
    ///
    /// # Errors
    /// Returns [`ValidationError`] when size or entry invariants are violated.
    pub(in crate::raw) fn validate(&self, header_size: usize) -> Result<(), ValidationError> {
        let stored_size = self.len();
        if stored_size != header_size {
            return Err(ValidationError::ColorTableSizeMismatch {
                stored_size,
                header_size,
            });
        }

        self.validate_entries()
    }
}
