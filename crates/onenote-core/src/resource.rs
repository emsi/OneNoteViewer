use crate::{Error, ResourceId, ResourceStatus, Result};
use onenote_parser::contents::{EmbeddedFile, FileDataStatus, Image, Picture};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const COPY_BUFFER_BYTES: usize = 64 * 1024;

/// Limits applied while streaming a lazy binary resource.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceCopyOptions {
    /// Maximum number of payload bytes accepted from the source.
    pub limit_bytes: u64,
}

impl ResourceCopyOptions {
    /// Construct options with an explicit maximum payload size.
    pub const fn new(limit_bytes: u64) -> Self {
        Self { limit_bytes }
    }
}

/// Progress reported after a resource chunk has been written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceCopyProgress {
    /// Payload bytes written so far.
    pub copied_bytes: u64,
    /// Payload size declared by the source, when reliable.
    pub declared_bytes: Option<u64>,
}

/// Result of a successful streamed resource copy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceCopyReport {
    /// Total payload bytes written.
    pub bytes_written: u64,
}

/// Cloneable cooperative cancellation handle for resource copies.
#[derive(Clone, Debug, Default)]
pub struct ResourceCopyControl {
    cancelled: Arc<AtomicBool>,
}

impl ResourceCopyControl {
    /// Create a cancellation handle in the active state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation. A running copy observes this between bounded I/O
    /// operations; callers must also cancel a blocked destination backend when
    /// that backend supports cancellation.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum ResourceLoader {
    Image(Image),
    Picture(Picture),
    Attachment(EmbeddedFile),
}

impl ResourceLoader {
    fn size(&self) -> u64 {
        match self {
            Self::Image(image) => image.size().unwrap_or(0),
            Self::Picture(picture) => picture.size(),
            Self::Attachment(file) => file.size(),
        }
    }

    fn status(&self) -> ResourceStatus {
        let status = match self {
            Self::Image(image) => image.data_status(),
            Self::Picture(picture) => picture.data_status(),
            Self::Attachment(file) => file.data_status(),
        };
        resource_status(status)
    }

    fn reader(&self) -> Option<Box<dyn Read>> {
        match self {
            Self::Image(image) => image.read(),
            Self::Picture(picture) => Some(picture.read()),
            Self::Attachment(file) => Some(file.read()),
        }
    }

    fn verified_size(&self) -> Option<u64> {
        match self {
            Self::Image(image) => image.size(),
            Self::Picture(picture) => Some(picture.size()),
            Self::Attachment(file) => Some(file.size()),
        }
    }
}

/// Lazy binary resources retained by a parsed notebook.
///
/// The store is intentionally separate from the serializable semantic model.
/// Reading always requires an explicit byte limit, and merely parsing a
/// notebook never materializes image or attachment payloads.
#[derive(Clone, Debug, Default)]
pub struct ResourceStore {
    loaders: HashMap<ResourceId, ResourceLoader>,
}

impl ResourceStore {
    pub(crate) fn insert(&mut self, id: ResourceId, loader: ResourceLoader) -> Result<()> {
        use std::collections::hash_map::Entry;
        match self.loaders.entry(id.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(loader);
                Ok(())
            }
            Entry::Occupied(_) => Err(Error::ResourceCollision { id }),
        }
    }

    /// Number of lazy payloads available.
    pub fn len(&self) -> usize {
        self.loaders.len()
    }

    /// Whether this store has no resources.
    pub fn is_empty(&self) -> bool {
        self.loaders.is_empty()
    }

    /// Visit the stable identifiers of available resources.
    pub fn resource_ids(&self) -> impl Iterator<Item = &ResourceId> {
        self.loaders.keys()
    }

    /// Return the declared byte size without opening the resource.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ResourceNotFound`] when the identifier does not belong
    /// to this store.
    pub fn declared_size(&self, id: &ResourceId) -> Result<u64> {
        self.loaders
            .get(id)
            .map(ResourceLoader::size)
            .ok_or_else(|| Error::ResourceNotFound { id: id.clone() })
    }

    /// Return whether the referenced source payload can be read.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ResourceNotFound`] when the identifier does not belong
    /// to this store.
    pub fn status(&self, id: &ResourceId) -> Result<ResourceStatus> {
        self.loaders
            .get(id)
            .map(ResourceLoader::status)
            .ok_or_else(|| Error::ResourceNotFound { id: id.clone() })
    }

    /// Read a resource into memory, rejecting it before allocation when its
    /// declared size exceeds `limit_bytes`.
    ///
    /// # Errors
    ///
    /// Returns a not-found error, an unavailable-payload error, a size-limit
    /// error, or an underlying lazy resource read error.
    pub fn read_limited(&self, id: &ResourceId, limit_bytes: u64) -> Result<Vec<u8>> {
        let declared = self.declared_size(id)?;
        let capacity = usize::try_from(declared.min(limit_bytes)).unwrap_or(usize::MAX);
        let mut bytes = Vec::with_capacity(capacity);
        self.copy_to(
            id,
            &mut bytes,
            ResourceCopyOptions::new(limit_bytes),
            &ResourceCopyControl::new(),
            |_| {},
        )?;
        Ok(bytes)
    }

    /// Stream a lazy resource into `writer` without materializing the payload.
    ///
    /// The copy performs blocking I/O and should run away from interactive UI
    /// threads. Progress callbacks run on the calling thread after each
    /// successfully written chunk. The caller owns destination publication and
    /// must discard or roll back partial output after any error.
    ///
    /// # Errors
    ///
    /// Returns a typed error for missing or unavailable resources, size-limit
    /// violations, cancellation, source or destination I/O failures, or a
    /// mismatch between a reliable declared size and the streamed payload.
    pub fn copy_to<W, F>(
        &self,
        id: &ResourceId,
        writer: &mut W,
        options: ResourceCopyOptions,
        control: &ResourceCopyControl,
        progress: F,
    ) -> Result<ResourceCopyReport>
    where
        W: Write,
        F: FnMut(ResourceCopyProgress),
    {
        let loader = self
            .loaders
            .get(id)
            .ok_or_else(|| Error::ResourceNotFound { id: id.clone() })?;
        let status = loader.status();
        if status != ResourceStatus::Available {
            return Err(Error::ResourceUnavailable {
                id: id.clone(),
                status,
            });
        }
        let declared = loader.verified_size();
        if declared.is_some_and(|size| size > options.limit_bytes) {
            return Err(Error::ResourceTooLarge {
                id: id.clone(),
                declared_bytes: declared.unwrap_or_default(),
                limit_bytes: options.limit_bytes,
            });
        }
        if control.is_cancelled() {
            return Err(Error::ResourceCopyCancelled { id: id.clone() });
        }
        let Some(reader) = loader.reader() else {
            return Err(Error::ResourceUnavailable {
                id: id.clone(),
                status: ResourceStatus::Missing,
            });
        };
        copy_reader(id, reader, writer, options, control, declared, progress)
    }
}

fn copy_reader<R, W, F>(
    id: &ResourceId,
    mut reader: R,
    writer: &mut W,
    options: ResourceCopyOptions,
    control: &ResourceCopyControl,
    declared_bytes: Option<u64>,
    mut progress: F,
) -> Result<ResourceCopyReport>
where
    R: Read,
    W: Write,
    F: FnMut(ResourceCopyProgress),
{
    if declared_bytes.is_some_and(|size| size > options.limit_bytes) {
        return Err(Error::ResourceTooLarge {
            id: id.clone(),
            declared_bytes: declared_bytes.unwrap_or_default(),
            limit_bytes: options.limit_bytes,
        });
    }
    if control.is_cancelled() {
        return Err(Error::ResourceCopyCancelled { id: id.clone() });
    }
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES].into_boxed_slice();
    let mut copied_bytes = 0_u64;
    progress(ResourceCopyProgress {
        copied_bytes,
        declared_bytes,
    });
    loop {
        if control.is_cancelled() {
            return Err(Error::ResourceCopyCancelled { id: id.clone() });
        }
        let count = reader
            .read(&mut buffer)
            .map_err(|source| Error::ResourceRead {
                id: id.clone(),
                source,
            })?;
        if count == 0 {
            break;
        }
        let next = copied_bytes.saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
        if next > options.limit_bytes {
            return Err(Error::ResourceTooLarge {
                id: id.clone(),
                declared_bytes: next,
                limit_bytes: options.limit_bytes,
            });
        }
        if let Err(source) = writer.write_all(&buffer[..count]) {
            if control.is_cancelled() {
                return Err(Error::ResourceCopyCancelled { id: id.clone() });
            }
            return Err(Error::ResourceWrite {
                id: id.clone(),
                source,
            });
        }
        copied_bytes = next;
        progress(ResourceCopyProgress {
            copied_bytes,
            declared_bytes,
        });
    }
    if let Some(declared_bytes) = declared_bytes {
        if copied_bytes != declared_bytes {
            return Err(Error::ResourceSizeMismatch {
                id: id.clone(),
                declared_bytes,
                actual_bytes: copied_bytes,
            });
        }
    }
    Ok(ResourceCopyReport {
        bytes_written: copied_bytes,
    })
}

// The upstream non-exhaustive enum requires a conservative fallback.
#[allow(clippy::match_same_arms)]
pub(crate) fn resource_status(status: FileDataStatus) -> ResourceStatus {
    match status {
        FileDataStatus::Available => ResourceStatus::Available,
        FileDataStatus::Missing => ResourceStatus::Missing,
        FileDataStatus::Invalid => ResourceStatus::Invalid,
        _ => ResourceStatus::Invalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor};

    fn id() -> ResourceId {
        ResourceId::new("resource")
    }

    #[test]
    fn copy_reader_streams_exact_bytes_and_monotonic_progress() {
        let payload = vec![0x5a; COPY_BUFFER_BYTES * 2 + 17];
        let mut output = Vec::new();
        let mut updates = Vec::new();
        let report = copy_reader(
            &id(),
            Cursor::new(&payload),
            &mut output,
            ResourceCopyOptions::new(payload.len() as u64),
            &ResourceCopyControl::default(),
            Some(payload.len() as u64),
            |progress| updates.push(progress),
        )
        .expect("copy");

        assert_eq!(output, payload);
        assert_eq!(report.bytes_written, payload.len() as u64);
        assert_eq!(updates.first().expect("first").copied_bytes, 0);
        assert_eq!(
            updates.last().expect("last").copied_bytes,
            report.bytes_written
        );
        assert!(updates
            .windows(2)
            .all(|pair| pair[0].copied_bytes <= pair[1].copied_bytes));
    }

    #[test]
    fn copy_reader_rejects_actual_limit_overrun() {
        let mut output = Vec::new();
        let error = copy_reader(
            &id(),
            Cursor::new(vec![1_u8; 9]),
            &mut output,
            ResourceCopyOptions::new(8),
            &ResourceCopyControl::default(),
            None,
            |_| {},
        )
        .expect_err("limit");
        assert!(matches!(error, Error::ResourceTooLarge { .. }));
    }

    #[test]
    fn copy_reader_detects_short_declared_payload() {
        let mut output = Vec::new();
        let error = copy_reader(
            &id(),
            Cursor::new(vec![1_u8; 4]),
            &mut output,
            ResourceCopyOptions::new(8),
            &ResourceCopyControl::default(),
            Some(5),
            |_| {},
        )
        .expect_err("mismatch");
        assert!(matches!(
            error,
            Error::ResourceSizeMismatch {
                declared_bytes: 5,
                actual_bytes: 4,
                ..
            }
        ));
    }

    #[test]
    fn copy_reader_rejects_declared_limit_before_reading() {
        struct PanicReader;
        impl Read for PanicReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                panic!("reader must not be opened")
            }
        }
        let error = copy_reader(
            &id(),
            PanicReader,
            &mut Vec::new(),
            ResourceCopyOptions::new(4),
            &ResourceCopyControl::default(),
            Some(5),
            |_| {},
        )
        .expect_err("declared limit");
        assert!(matches!(error, Error::ResourceTooLarge { .. }));
    }

    #[test]
    fn copy_reader_honors_pre_copy_cancellation() {
        let control = ResourceCopyControl::default();
        control.cancel();
        let error = copy_reader(
            &id(),
            Cursor::new([1_u8]),
            &mut Vec::new(),
            ResourceCopyOptions::new(1),
            &control,
            Some(1),
            |_| {},
        )
        .expect_err("cancelled");
        assert!(matches!(error, Error::ResourceCopyCancelled { .. }));
    }

    #[test]
    fn copy_reader_uses_a_fixed_bounded_buffer() {
        struct RecordingReader {
            remaining: usize,
            largest_request: usize,
        }
        impl Read for RecordingReader {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                self.largest_request = self.largest_request.max(buffer.len());
                let count = self.remaining.min(buffer.len());
                buffer[..count].fill(1);
                self.remaining -= count;
                Ok(count)
            }
        }
        let mut reader = RecordingReader {
            remaining: COPY_BUFFER_BYTES * 3 + 1,
            largest_request: 0,
        };
        let expected = reader.remaining;
        let report = copy_reader(
            &id(),
            &mut reader,
            &mut io::sink(),
            ResourceCopyOptions::new(expected as u64),
            &ResourceCopyControl::default(),
            Some(expected as u64),
            |_| {},
        )
        .expect("copy");
        assert_eq!(report.bytes_written, expected as u64);
        assert_eq!(reader.largest_request, COPY_BUFFER_BYTES);
    }

    #[test]
    fn copy_reader_preserves_reader_failures() {
        struct FailingReader;
        impl Read for FailingReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("read failed"))
            }
        }
        let error = copy_reader(
            &id(),
            FailingReader,
            &mut Vec::new(),
            ResourceCopyOptions::new(1),
            &ResourceCopyControl::default(),
            None,
            |_| {},
        )
        .expect_err("reader failure");
        assert!(matches!(error, Error::ResourceRead { .. }));
    }

    struct CancellingWriter<'a> {
        control: &'a ResourceCopyControl,
        bytes: usize,
    }

    impl Write for CancellingWriter<'_> {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.bytes += buffer.len();
            self.control.cancel();
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn copy_reader_observes_mid_copy_cancellation() {
        let control = ResourceCopyControl::default();
        let mut writer = CancellingWriter {
            control: &control,
            bytes: 0,
        };
        let error = copy_reader(
            &id(),
            Cursor::new(vec![1_u8; COPY_BUFFER_BYTES + 1]),
            &mut writer,
            ResourceCopyOptions::new(u64::MAX),
            &control,
            None,
            |_| {},
        )
        .expect_err("cancelled");
        assert!(matches!(error, Error::ResourceCopyCancelled { .. }));
        assert_eq!(writer.bytes, COPY_BUFFER_BYTES);
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("write failed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn copy_reader_preserves_writer_failures() {
        let error = copy_reader(
            &id(),
            Cursor::new([1_u8]),
            &mut FailingWriter,
            ResourceCopyOptions::new(1),
            &ResourceCopyControl::default(),
            Some(1),
            |_| {},
        )
        .expect_err("writer failure");
        assert!(matches!(error, Error::ResourceWrite { .. }));
    }
}
