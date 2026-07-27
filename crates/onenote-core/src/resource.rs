use crate::{Error, ResourceId, ResourceStatus, Result};
use onenote_parser::contents::{EmbeddedFile, FileDataStatus, Image};
use std::collections::HashMap;
use std::io::Read;

#[derive(Clone, Debug)]
pub(crate) enum ResourceLoader {
    Image(Image),
    Attachment(EmbeddedFile),
}

impl ResourceLoader {
    fn size(&self) -> u64 {
        match self {
            Self::Image(image) => image.size().unwrap_or(0),
            Self::Attachment(file) => file.size(),
        }
    }

    fn status(&self) -> ResourceStatus {
        let status = match self {
            Self::Image(image) => image.data_status(),
            Self::Attachment(file) => file.data_status(),
        };
        resource_status(status)
    }

    fn reader(&self) -> Option<Box<dyn Read>> {
        match self {
            Self::Image(image) => image.read(),
            Self::Attachment(file) => Some(file.read()),
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
    pub(crate) fn insert(&mut self, id: ResourceId, loader: ResourceLoader) {
        self.loaders.insert(id, loader);
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
        let declared = loader.size();
        if declared > limit_bytes {
            return Err(Error::ResourceTooLarge {
                id: id.clone(),
                declared_bytes: declared,
                limit_bytes,
            });
        }

        let capacity = usize::try_from(declared.min(limit_bytes)).unwrap_or(usize::MAX);
        let mut bytes = Vec::with_capacity(capacity);
        let Some(reader) = loader.reader() else {
            return Err(Error::ResourceUnavailable {
                id: id.clone(),
                status: ResourceStatus::Missing,
            });
        };
        reader
            .take(limit_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|source| Error::ResourceRead {
                id: id.clone(),
                source,
            })?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit_bytes {
            return Err(Error::ResourceTooLarge {
                id: id.clone(),
                declared_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                limit_bytes,
            });
        }
        Ok(bytes)
    }
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
