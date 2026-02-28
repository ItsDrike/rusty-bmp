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
pub(crate) struct BoundedStream<R> {
    inner: R,
    start: u64,
    end: u64,
}

impl<R: Seek> BoundedStream<R> {
    /// Creates a new `BoundedStream` over the entire underlying stream.
    ///
    /// The current position of the underlying stream is preserved.
    /// The initial window is `[0, stream_length]`.
    ///
    /// This performs one temporary seek to end, to determine the stream length.
    pub(crate) fn new(mut inner: R) -> io::Result<Self> {
        let cur_pos = inner.stream_position()?;
        let end = inner.seek(SeekFrom::End(0))?;
        inner.seek(SeekFrom::Start(cur_pos))?;

        Ok(Self { inner, start: 0, end })
    }

    /// Shrinks the lower bound of the window to the current position.
    ///
    /// Fails if:
    ///
    /// - The current position is beyond the upper bound.
    /// - The current position is before the existing lower bound.
    ///
    /// This operation never expands the window.
    pub(crate) fn with_start(mut self) -> io::Result<Self> {
        let pos = self.inner.stream_position()?;

        if pos > self.end {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "start beyond end bound"));
        }
        if pos < self.start {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "start beyond existing start bound",
            ));
        }

        self.start = pos;
        Ok(self)
    }

    /// Shrinks the upper bound of the window to `current_position + length`.
    ///
    /// Fails if the new bound would exceed the existing upper bound.
    ///
    /// This operation never expands the window.
    pub(crate) fn with_end(mut self, length: u64) -> io::Result<Self> {
        let pos = self.inner.stream_position()?;
        let new_end = pos
            .checked_add(length)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "length overflow"))?;

        if new_end > self.end {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "bounded region exceeds stream length",
            ));
        }

        self.end = new_end;
        Ok(self)
    }

    /// Returns the number of bytes remaining until the upper bound.
    pub(crate) fn remaining(&mut self) -> io::Result<u64> {
        let pos = self.inner.stream_position()?;
        // Invariant: inner pos is always within [start, end]
        debug_assert!(pos <= self.end);
        Ok(self.end - pos)
    }

    /// Validates that `absolute` lies within `[start, end]`.
    fn check_bounds(&self, absolute: u64) -> io::Result<()> {
        if absolute < self.start {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "position before start bound",
            ));
        }

        if absolute > self.end {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "position after end bound"));
        }

        Ok(())
    }
}

impl<R: Read + Seek> Read for BoundedStream<R> {
    /// Reads at most the remaining bytes within the window.
    ///
    /// Reading at the upper bound behaves like EOF and returns `Ok(0)`.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self.remaining()?;
        let max_read = remaining.min(buf.len() as u64) as usize;

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
        let max_write = remaining.min(buf.len() as u64) as usize;

        if max_write == 0 {
            return Ok(0);
        }

        self.inner.write(&buf[..max_write])
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
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
        let absolute = match pos {
            SeekFrom::Start(off) => self
                .start
                .checked_add(off)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "overflow"))?,
            SeekFrom::End(off) => {
                if off >= 0 {
                    self.end
                        .checked_add(off as u64)
                        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "overflow"))?
                } else {
                    self.end
                        .checked_sub(off.unsigned_abs())
                        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "overflow"))?
                }
            }
            SeekFrom::Current(off) => {
                let cur = self.inner.stream_position()?;

                if off >= 0 {
                    cur.checked_add(off as u64)
                        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "overflow"))?
                } else {
                    cur.checked_sub(off.unsigned_abs())
                        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "overflow"))?
                }
            }
        };

        self.check_bounds(absolute)?;
        self.inner.seek(SeekFrom::Start(absolute))?;
        debug_assert!(absolute >= self.start);
        Ok(absolute - self.start)
    }
}
