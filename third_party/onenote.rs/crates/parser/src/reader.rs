use crate::errors::{ErrorKind, Result};
use crate::fs::FileSource;
use crate::fs::file_source::BytesSource;
use bytes::Bytes;
use std::ops::Range;
use std::sync::Arc;

/// A random-access cursor over a [`FileSource`].
///
/// Owns its position within the source and tracks a half-open
/// `[start, end)` view. Sub-readers from [`Reader::slice`] share the
/// source via [`Arc`] but have independent positions.
///
/// When the source returns `Some` from [`FileSource::as_bytes`] (the
/// common case), the reader caches the buffer and hot-path operations
/// index it directly. Otherwise, reads go through
/// [`FileSource::read_at`].
pub(crate) struct Reader {
    source: Arc<dyn FileSource>,
    /// Cached full buffer for in-memory backings.
    cached: Option<Bytes>,
    /// Current absolute position within the source.
    position: u64,
    /// Lower bound of this reader's view (inclusive, absolute). Recorded
    /// for diagnostic clarity; reads use `position` and `end`.
    #[allow(dead_code)]
    start: u64,
    /// Upper bound of this reader's view (exclusive, absolute).
    end: u64,
}

impl Reader {
    /// Create a reader by copying the given slice into a refcounted buffer.
    /// Primarily for tests and small in-memory parses.
    pub(crate) fn new(data: &[u8]) -> Self {
        Self::from_bytes(Bytes::copy_from_slice(data))
    }

    /// Create a reader over an owned [`Bytes`] buffer (zero-copy).
    pub(crate) fn from_bytes(bytes: Bytes) -> Self {
        let source: Arc<dyn FileSource> = Arc::new(BytesSource::new(bytes));
        Self::from_source(source)
    }

    /// Create a reader over the given [`FileSource`], spanning its full extent.
    pub(crate) fn from_source(source: Arc<dyn FileSource>) -> Self {
        let end = source.byte_length();
        let cached = source.as_bytes();
        Self {
            source,
            cached,
            position: 0,
            start: 0,
            end,
        }
    }

    /// Bytes remaining between the cursor and the end of this reader's view.
    pub(crate) fn remaining(&self) -> usize {
        self.end.saturating_sub(self.position) as usize
    }

    pub(crate) fn advance(&mut self, count: usize) -> Result<()> {
        if self.remaining() < count {
            return Err(ErrorKind::UnexpectedEof.into());
        }
        self.position += count as u64;
        Ok(())
    }

    /// Read `count` bytes, advancing the cursor.
    ///
    /// Refcount-shared view of the source when the backing is in-memory;
    /// a fresh allocation otherwise.
    pub(crate) fn read(&mut self, count: usize) -> Result<Bytes> {
        if self.remaining() < count {
            return Err(ErrorKind::UnexpectedEof.into());
        }
        let bytes = self.fetch(self.position, count)?;
        self.position += count as u64;
        Ok(bytes)
    }

    /// Read `count` bytes without advancing the cursor.
    ///
    /// Refcount-shared view of the source when the backing is in-memory; a
    /// single `read_at` call against a lazy backing.
    pub(crate) fn peek_bytes(&self, count: usize) -> Result<Bytes> {
        if self.remaining() < count {
            return Err(ErrorKind::UnexpectedEof.into());
        }
        self.fetch(self.position, count)
    }

    fn fetch(&self, offset: u64, len: usize) -> Result<Bytes> {
        if let Some(buf) = &self.cached {
            let start = offset as usize;
            return Ok(buf.slice(start..start + len));
        }
        Ok(self.source.read_at(offset, len)?)
    }

    pub(crate) fn get_u8(&mut self) -> Result<u8> {
        let b = self.read_array::<1>()?;
        Ok(b[0])
    }

    pub(crate) fn get_u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.read_array::<2>()?))
    }

    pub(crate) fn get_u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.read_array::<4>()?))
    }

    pub(crate) fn get_u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.read_array::<8>()?))
    }

    pub(crate) fn get_u128(&mut self) -> Result<u128> {
        Ok(u128::from_le_bytes(self.read_array::<16>()?))
    }

    pub(crate) fn get_f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.read_array::<4>()?))
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
        if self.remaining() < N {
            return Err(ErrorKind::UnexpectedEof.into());
        }
        let mut out = [0u8; N];
        if let Some(buf) = &self.cached {
            let start = self.position as usize;
            out.copy_from_slice(&buf[start..start + N]);
        } else {
            let bytes = self.source.read_at(self.position, N)?;
            out.copy_from_slice(&bytes);
        }
        self.position += N as u64;
        Ok(out)
    }

    /// Produce a sub-reader bounded to the given absolute byte range.
    ///
    /// The new reader shares the underlying source via [`Arc`] but starts
    /// at `range.start` with its own independent position. Operations on
    /// the sub-reader don't affect this reader's position.
    pub(crate) fn slice(&self, range: Range<usize>) -> Result<Reader> {
        let total = self.source.byte_length();
        if range.start as u64 > total || range.end as u64 > total {
            return Err(ErrorKind::UnexpectedEof.into());
        }
        Ok(Reader {
            source: self.source.clone(),
            cached: self.cached.clone(),
            position: range.start as u64,
            start: range.start as u64,
            end: range.end as u64,
        })
    }

    /// Refcount-shared handle to the underlying source.
    pub(crate) fn source(&self) -> Arc<dyn FileSource> {
        self.source.clone()
    }

    /// The reader's current absolute position within the source.
    pub(crate) fn position(&self) -> u64 {
        self.position
    }
}

#[cfg(test)]
mod tests {
    use super::Reader;

    #[test]
    fn test_read_and_advance() {
        let data = [1u8, 2, 3, 4];
        let mut reader = Reader::new(&data);

        assert_eq!(reader.remaining(), 4);
        assert_eq!(&*reader.read(2).unwrap(), &[1, 2]);
        assert_eq!(reader.remaining(), 2);

        reader.advance(1).unwrap();
        assert_eq!(reader.remaining(), 1);
        assert_eq!(reader.get_u8().unwrap(), 4);
        assert!(reader.get_u8().is_err());
    }

    #[test]
    fn test_get_numeric_types() {
        let data = [
            0x34, 0x12, // u16 = 0x1234
            0x78, 0x56, 0x34, 0x12, // u32 = 0x12345678
            0xEF, 0xCD, 0xAB, 0x89, 0x67, 0x45, 0x23, 0x01, // u64
        ];
        let mut reader = Reader::new(&data);

        assert_eq!(reader.get_u16().unwrap(), 0x1234);
        assert_eq!(reader.get_u32().unwrap(), 0x1234_5678);
        assert_eq!(reader.get_u64().unwrap(), 0x0123_4567_89AB_CDEF);
    }

    #[test]
    fn test_get_f32() {
        let data = [0x00, 0x00, 0x80, 0x3F]; // 1.0 in LE
        let mut reader = Reader::new(&data);

        assert_eq!(reader.get_f32().unwrap(), 1.0);
        assert!(reader.get_u8().is_err());
    }

    #[test]
    fn test_slice_isolated_positions() {
        let data = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut reader = Reader::new(&data);
        assert_eq!(reader.get_u8().unwrap(), 1);

        let mut sub = reader.slice(2..6).unwrap();
        assert_eq!(sub.remaining(), 4);
        assert_eq!(sub.get_u8().unwrap(), 3);

        // The original reader's position is unaffected.
        assert_eq!(reader.get_u8().unwrap(), 2);
    }
}
