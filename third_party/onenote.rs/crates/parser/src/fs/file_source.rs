//! [`FileSource`] implementations and the page cache that fronts them.
//!
//! A [`FileSource`] is the byte-level interface the parser reads through:
//! it answers `read_at(offset, len)` against some backing storage. This
//! module provides the building blocks:
//!
//! - [`BytesSource`] — zero-copy view over an in-memory [`Bytes`] buffer.
//!   Returned by the default [`FileSystem::open_file`] when the consumer
//!   only implements [`FileSystem::read_file`].
//! - [`FileBackedSource`] (native-fs only) — positional reads against an
//!   open [`std::fs::File`] via `pread` (Unix) or `seek_read` (Windows).
//!   No caching of its own.
//! - [`CachedFileSource`] — page-aligned LRU decorator (≈ 4 MiB resident)
//!   plus a single-slot last-access fast path. Amortises the cost of
//!   structural-parse peek sequences that hammer the same page; a no-op
//!   when the inner source is already fully in memory.
//!
//! [`NativeFs::open_file`] wraps a [`FileBackedSource`] in
//! [`CachedFileSource`] so multi-GB notebooks parse with a working set
//! proportional to active reads, not file size.

use crate::fs::FileSource;
use bytes::Bytes;
#[cfg(feature = "native-fs")]
use std::fs::File;
use std::io::Error;

/// A [`FileSource`] backed by an in-memory [`Bytes`] buffer.
///
/// `read_at` returns a refcount-shared slice into the buffer (zero-copy);
/// `as_bytes` returns the full buffer. Used by the default
/// [`FileSystem::open_file`] when the consumer only provides
/// [`FileSystem::read_file`].
pub struct BytesSource {
    bytes: Bytes,
}

impl BytesSource {
    /// Wrap a [`Bytes`] buffer as a [`FileSource`].
    pub fn new(bytes: Bytes) -> Self {
        Self { bytes }
    }
}

impl FileSource for BytesSource {
    fn byte_length(&self) -> u64 {
        self.bytes.len() as u64
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<Bytes, Error> {
        let start = offset as usize;
        let end = start
            .checked_add(len)
            .filter(|&e| e <= self.bytes.len())
            .ok_or_else(|| {
                Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!(
                        "BytesSource::read_at out of bounds: offset={offset} len={len} byte_length={}",
                        self.bytes.len()
                    ),
                )
            })?;
        Ok(self.bytes.slice(start..end))
    }

    fn as_bytes(&self) -> Option<Bytes> {
        Some(self.bytes.clone())
    }
}

/// Page size for [`CachedFileSource`]'s LRU cache, in bytes.
///
/// 4 KiB matches the OS page granularity, so every miss against a
/// `NativeFs` source is exactly one kernel page fault into the OS page
/// cache. Larger pages amortise per-fetch setup but waste cache slots
/// on the small (often single-byte) reads that dominate the
/// structural-parse hot loop; smaller pages do the opposite.
/// Empirically a tie with 16 KiB on small fixtures and a clear win on
/// multi-MB files.
const PAGE_SIZE: u64 = 4096;

/// LRU page-cache capacity for [`CachedFileSource`].
///
/// Empirically chosen across fixtures from 22 KB to 154 MB. The benefit
/// plateaus past ~16 pages on small workloads; large-FSSHTTPB
/// workloads continue to benefit (marginally) up to 1024. 1024 pages ×
/// 4 KiB = 4 MiB of resident cache per source.
const CACHE_PAGES: std::num::NonZeroUsize = std::num::NonZeroUsize::new(1024).unwrap();

/// A [`FileSource`] backed by an open [`std::fs::File`] handle.
///
/// Issues positional reads via `FileExt::read_exact_at` (Unix) or
/// `FileExt::seek_read` (Windows). No caching of its own — wrap in
/// [`CachedFileSource`] for that, which is what [`NativeFs::open_file`]
/// does.
#[cfg(feature = "native-fs")]
pub(crate) struct FileBackedSource {
    pub(crate) file: File,
    pub(crate) byte_length: u64,
}

#[cfg(feature = "native-fs")]
impl FileSource for FileBackedSource {
    fn byte_length(&self) -> u64 {
        self.byte_length
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<Bytes, Error> {
        let mut buf = vec![0u8; len];
        read_exact_at(&self.file, offset, &mut buf)?;
        Ok(Bytes::from(buf))
    }
    // `as_bytes` defaults to `None` — the file isn't resident.
}

/// A [`FileSource`] decorator that fronts another `FileSource` with a
/// page-aligned LRU cache.
///
/// Any [`FileSource`] implementation can wrap itself in
/// `CachedFileSource` to amortize fetch cost across consecutive
/// structural reads. The cache holds [`CACHE_PAGES`] pages of
/// [`PAGE_SIZE`] bytes each (≈ 4 MiB resident); a single-slot
/// last-access fast path sidesteps the LRU's hash lookup + reorder
/// when consecutive reads hit the same page (the dominant pattern in
/// `compact_u64` / `ObjectHeader::parse` peek sequences).
///
/// The decorator is most useful when the inner source's `read_at` is
/// expensive — a `pread` syscall (native), a JS/WASM callback, an HTTP
/// range request. For sources whose `as_bytes()` already returns the
/// full buffer in memory (e.g. [`BytesSource`]), the wrapper is a
/// pass-through: the parser's [`Reader`](crate::reader) takes its
/// own fast path off the cached `Bytes`.
pub struct CachedFileSource<S: FileSource> {
    inner: S,
    cache: std::sync::Mutex<lru::LruCache<u64, Bytes>>,
    /// Single-slot last-access cache, checked before the LRU.
    last: std::sync::Mutex<Option<(u64, Bytes)>>,
}

impl<S: FileSource> CachedFileSource<S> {
    /// Wrap `inner` with the default page cache configuration.
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            cache: std::sync::Mutex::new(lru::LruCache::new(CACHE_PAGES)),
            last: std::sync::Mutex::new(None),
        }
    }

    /// Fetch a single page (≤ [`PAGE_SIZE`] bytes near EOF), preferring
    /// the last-access cache, then the LRU, then the inner source.
    fn page(&self, page_start: u64) -> Result<Bytes, Error> {
        // Fast path: same page as the previous call.
        if let Some((p, b)) = &*self.last.lock().unwrap() {
            if *p == page_start {
                return Ok(b.clone());
            }
        }

        let page = {
            let mut cache = self.cache.lock().unwrap();
            if let Some(page) = cache.get(&page_start) {
                page.clone()
            } else {
                let len = (self.inner.byte_length() - page_start).min(PAGE_SIZE) as usize;
                let p = self.inner.read_at(page_start, len)?;
                cache.put(page_start, p.clone());
                p
            }
        };

        *self.last.lock().unwrap() = Some((page_start, page.clone()));
        Ok(page)
    }
}

impl<S: FileSource> FileSource for CachedFileSource<S> {
    fn byte_length(&self) -> u64 {
        self.inner.byte_length()
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<Bytes, Error> {
        if len == 0 {
            return Ok(Bytes::new());
        }
        let end = offset + len as u64;
        let first_page = (offset / PAGE_SIZE) * PAGE_SIZE;
        let last_page = ((end - 1) / PAGE_SIZE) * PAGE_SIZE;

        if first_page == last_page {
            // Fast path: read fits within a single cached page.
            let page = self.page(first_page)?;
            let inner = (offset - first_page) as usize;
            return Ok(page.slice(inner..inner + len));
        }

        // Slow path: stitch across pages into a fresh buffer.
        let mut buf = vec![0u8; len];
        let mut written = 0;
        let mut page_start = first_page;
        while page_start <= last_page {
            let page = self.page(page_start)?;
            let copy_start = offset.max(page_start) - page_start;
            let copy_end = end.min(page_start + page.len() as u64) - page_start;
            let chunk = &page[copy_start as usize..copy_end as usize];
            buf[written..written + chunk.len()].copy_from_slice(chunk);
            written += chunk.len();
            page_start += PAGE_SIZE;
        }
        Ok(Bytes::from(buf))
    }

    /// Forwards the inner source's `as_bytes`. The `Reader`'s cached
    /// path then bypasses our `read_at` entirely when the inner source
    /// is fully in memory.
    fn as_bytes(&self) -> Option<Bytes> {
        self.inner.as_bytes()
    }
}

#[cfg(all(feature = "native-fs", unix))]
fn read_exact_at(file: &File, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
    use std::os::unix::fs::FileExt;
    file.read_exact_at(buf, offset)
}

#[cfg(all(feature = "native-fs", windows))]
fn read_exact_at(file: &File, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
    use std::os::windows::fs::FileExt;
    let mut total = 0;
    while total < buf.len() {
        let n = file.seek_read(&mut buf[total..], offset + total as u64)?;
        if n == 0 {
            return Err(Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "FileBackedSource::read_at hit end of file before satisfying request",
            ));
        }
        total += n;
    }
    Ok(())
}
