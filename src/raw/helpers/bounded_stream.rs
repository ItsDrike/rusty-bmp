use std::io::{self, Read, Seek, SeekFrom, Write};

/// A `Seek`-based stream wrapper that restricts all I/O operations to a
/// shrinking window `[start, end]` within the underlying stream.
///
/// The window is always valid and monotonic:
///
/// - `start <= end`
/// - The window may only shrink over time (never expand).
/// - The inner cursor is never allowed to move outside `[start, end]`.
///
/// `SeekFrom::Start` is interpreted relative to `start`, meaning:
///
/// - If `start == 0`, behavior matches the underlying stream.
/// - If `start > 0`, `SeekFrom::Start(0)` seeks to the window start.
#[derive(Debug)]
pub struct BoundedStream<R: Seek> {
    inner: R,
    abs_start: u64,
    abs_end: u64,
}

impl<R: Seek> BoundedStream<R> {
    /// Creates a new `BoundedStream` over the underlying stream.
    ///
    /// The current position of the underlying stream is preserved.
    /// The initial window is `[0, u64::MAX]`.
    pub const fn new(inner: R) -> Self {
        Self {
            inner,
            abs_start: 0,
            abs_end: u64::MAX,
        }
    }

    /// Shrinks the window's upper bound to the underlying stream's end.
    ///
    /// If the stream end lies within the current bounds, the upper bound is
    /// reduced. If it lies beyond the current upper bound, this is a no-op.
    ///
    /// This operation never expands the window.
    pub fn cap_to_stream_end(mut self) -> io::Result<Self> {
        // Find the stream end by seeking there, then seek back to where we were
        let cur = self.inner.stream_position()?;
        let end = self.inner.seek(SeekFrom::End(0))?;
        self.inner.seek(SeekFrom::Start(cur))?;

        // If the actual stream end is beyond our bounds, this is a no-op;
        // otherwise, shrink the end.
        if self.check_bounds(end).is_ok() {
            self.abs_end = end;
        }

        Ok(self)
    }

    /// Shrinks the lower bound to the position resolved from `pos`.
    ///
    /// Fails if:
    ///
    /// - The position is beyond the upper bound.
    /// - The position is before the existing lower bound.
    ///
    /// This operation never expands the window.
    pub fn shrink_start(mut self, pos: SeekFrom) -> io::Result<Self> {
        let absolute = self.get_absolute(pos)?;
        self.check_bounds(absolute)?;
        self.abs_start = absolute;
        Ok(self)
    }

    /// Shrinks the upper bound to the position resolved from `pos`.
    ///
    /// Fails if:
    /// - The position is beyond the upper bound.
    /// - The position is before the existing lower bound.
    ///
    /// This operation never expands the window.
    pub fn shrink_end(mut self, pos: SeekFrom) -> io::Result<Self> {
        let absolute = self.get_absolute(pos)?;
        self.check_bounds(absolute)?;
        self.abs_end = absolute;
        Ok(self)
    }

    /// Returns the number of bytes remaining until the upper bound.
    pub fn remaining(&mut self) -> io::Result<u64> {
        let pos = self.inner.stream_position()?;
        // Invariant: inner pos is always within [start, end]
        debug_assert!(pos <= self.abs_end);
        Ok(self.abs_end - pos)
    }

    /// Convert a relative `SeekFrom` based position into an absolute position
    ///
    /// This will be the absolute position in the inner seekable stream.
    fn get_absolute(&mut self, relative_pos: SeekFrom) -> io::Result<u64> {
        Ok(match relative_pos {
            SeekFrom::Start(off) => self
                .abs_start
                .checked_add(off)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "overflow"))?,
            SeekFrom::End(off) => {
                if off >= 0 {
                    self.abs_end
                        .checked_add(off.unsigned_abs())
                        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "overflow"))?
                } else {
                    self.abs_end
                        .checked_sub(off.unsigned_abs())
                        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "overflow"))?
                }
            }
            SeekFrom::Current(off) => {
                let cur = self.inner.stream_position()?;

                if off >= 0 {
                    cur.checked_add(off.unsigned_abs())
                        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "overflow"))?
                } else {
                    cur.checked_sub(off.unsigned_abs())
                        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "overflow"))?
                }
            }
        })
    }

    /// Validates that `absolute` lies within `[start, end]`.
    fn check_bounds(&self, absolute: u64) -> io::Result<()> {
        if absolute < self.abs_start {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "position before start bound",
            ));
        }

        if absolute > self.abs_end {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "position after end bound"));
        }

        Ok(())
    }
}

impl<R: Seek> Seek for BoundedStream<R> {
    /// Seeks within the current window.
    ///
    /// - `SeekFrom::Start(off)` is interpreted as `start + off`.
    /// - `SeekFrom::End(off)` is interpreted as `end + off`.
    /// - `SeekFrom::Current(off)` is relative to the current position.
    ///
    /// Fails if the resulting position would lie outside `[start, end]`.
    ///
    /// Returns the position relative to `start`.
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let absolute = self.get_absolute(pos)?;
        self.check_bounds(absolute)?;
        self.inner.seek(SeekFrom::Start(absolute))?;
        Ok(absolute - self.abs_start)
    }
}

impl<R: Read + Seek> Read for BoundedStream<R> {
    /// Reads at most the remaining bytes within the window.
    ///
    /// Reading at the upper bound behaves like EOF and returns `Ok(0)`.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self.remaining()?;

        #[expect(clippy::cast_possible_truncation)]
        let max_read = buf.len().min(remaining.min(usize::MAX as u64) as usize);

        if max_read == 0 {
            return Ok(0);
        }

        self.inner.read(&mut buf[..max_read])
    }
}

impl<R: Seek + Write> Write for BoundedStream<R> {
    /// Writes at most the remaining bytes within the window.
    ///
    /// Writing at the upper bound returns `Ok(0)`.
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let remaining = self.remaining()?;
        let max_write = buf.len().min(usize::try_from(remaining).unwrap_or(usize::MAX));

        if max_write == 0 {
            return Ok(0);
        }

        self.inner.write(&buf[..max_write])
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
