use std::path::PathBuf;

/// Errors returned by the source-native `OneNote` API.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A filesystem operation failed.
    #[error("could not access {path}: {source}")]
    Io {
        /// Path involved in the operation.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// The selected source is not supported by this operation.
    #[error("unsupported OneNote source {path}; expected a .one or .onetoc2 file")]
    UnsupportedSource {
        /// Rejected path.
        path: PathBuf,
    },

    /// The upstream format parser rejected the source.
    #[error("could not parse {path}: {message}")]
    Parse {
        /// Rejected path.
        path: PathBuf,
        /// Parser-provided diagnostic.
        message: String,
    },

    /// A requested payload is not present in this parsed source.
    #[error("resource {id} is not available")]
    ResourceNotFound {
        /// Stable resource identifier.
        id: crate::ResourceId,
    },

    /// A page object exists, but its binary payload is unavailable.
    #[error("resource {id} is unavailable ({status:?})")]
    ResourceUnavailable {
        /// Stable resource identifier.
        id: crate::ResourceId,
        /// Availability state preserved from the source.
        status: crate::ResourceStatus,
    },

    /// A resource exceeded the caller's explicit size limit.
    #[error("resource {id} is {declared_bytes} bytes, above the {limit_bytes}-byte limit")]
    ResourceTooLarge {
        /// Stable resource identifier.
        id: crate::ResourceId,
        /// Size recorded in the `OneNote` source.
        declared_bytes: u64,
        /// Caller-provided maximum.
        limit_bytes: u64,
    },

    /// Reading a lazy resource failed.
    #[error("could not read resource {id}: {source}")]
    ResourceRead {
        /// Stable resource identifier.
        id: crate::ResourceId,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// Writing a lazily loaded resource failed.
    #[error("could not write resource {id}: {source}")]
    ResourceWrite {
        /// Stable resource identifier.
        id: crate::ResourceId,
        /// Underlying output failure.
        #[source]
        source: std::io::Error,
    },

    /// Copying a lazy resource was cancelled by its caller.
    #[error("copying resource {id} was cancelled")]
    ResourceCopyCancelled {
        /// Stable resource identifier.
        id: crate::ResourceId,
    },

    /// The resource payload length disagreed with its declared length.
    #[error(
        "resource {id} contains {actual_bytes} bytes, but its source declares {declared_bytes} bytes"
    )]
    ResourceSizeMismatch {
        /// Stable resource identifier.
        id: crate::ResourceId,
        /// Size recorded in the `OneNote` source.
        declared_bytes: u64,
        /// Number of bytes returned by the lazy reader.
        actual_bytes: u64,
    },

    /// Two projected payloads resolved to the same stable identifier.
    #[error("resource identifier collision for {id}")]
    ResourceCollision {
        /// Conflicting stable resource identifier.
        id: crate::ResourceId,
    },

    /// No supported external CAB extractor is available.
    #[error("could not find 7zz or 7z in PATH; install 7-Zip to open .onepkg files")]
    ExtractorNotFound,

    /// The requested durable extraction destination already exists.
    #[error("extraction destination already exists: {path}")]
    DestinationExists {
        /// Existing destination.
        path: PathBuf,
    },

    /// The package failed structural or path validation.
    #[error("invalid OneNote package {path}: {message}")]
    InvalidPackage {
        /// Rejected package.
        path: PathBuf,
        /// Validation detail.
        message: String,
    },

    /// The external extractor failed.
    #[error("extractor failed while processing {path}: {message}")]
    ExtractionFailed {
        /// Package being processed.
        path: PathBuf,
        /// Bounded failure detail.
        message: String,
    },

    /// Package extraction was cancelled.
    #[error("OneNote package extraction was cancelled")]
    ExtractionCancelled,
}

/// Result type used throughout `onenote-core`.
pub type Result<T> = std::result::Result<T, Error>;
