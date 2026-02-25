use std::io::{self, Read, Seek, SeekFrom};

pub(crate) fn read_array<const N: usize, R: Read>(reader: &mut R) -> std::io::Result<[u8; N]> {
    let mut buf = [0u8; N];
    reader.read_exact(&mut buf)?;
    Ok(buf)
}

pub(crate) struct BoundedReader<R> {
    inner: R,
    start: u64,
    end: u64,
}

impl<R: Read + Seek> BoundedReader<R> {
    pub(crate) fn new(mut inner: R, length: u64) -> io::Result<Self> {
        let start = inner.stream_position()?;
        let end = start
            .checked_add(length)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "length overflow"))?;

        let read_end = inner.seek(SeekFrom::End(0))?;
        if end > read_end {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "bounded region exceeds underlying stream length",
            ));
        }
        inner.seek(SeekFrom::Start(start))?;

        Ok(Self { inner, start, end })
    }

    pub fn remaining(&mut self) -> io::Result<u64> {
        let pos = self.inner.stream_position()?;
        // Invariant: inner pos is always within [start, end]
        Ok(self.end - pos)
    }
}

impl<R: Read + Seek> Read for BoundedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self.remaining()?;
        let max_read = remaining.min(buf.len() as u64) as usize;

        if max_read == 0 {
            return Ok(0);
        }

        self.inner.read(&mut buf[..max_read])
    }
}

impl<R: Read + Seek> Seek for BoundedReader<R> {
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

        if absolute < self.start || absolute > self.end {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "seek outside bounds"));
        }

        self.inner.seek(SeekFrom::Start(absolute))?;

        Ok(absolute - self.start)
    }
}
