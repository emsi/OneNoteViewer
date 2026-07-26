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

    /// A resource exceeded the caller's explicit memory limit.
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
