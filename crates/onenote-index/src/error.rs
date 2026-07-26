/// Errors returned by the public index and query API.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Error {
    /// `SQLite` storage failed. Internal SQL details are not a public contract.
    #[error("index database operation failed: {message}")]
    Database {
        /// Bounded implementation diagnostic.
        message: String,
    },

    /// The database was created by an unsupported schema generation.
    #[error("unsupported index schema {found}; expected {expected}")]
    IncompatibleSchema {
        /// Schema stored in the database.
        found: u32,
        /// Schema supported by this build.
        expected: u32,
    },

    /// Structured query text was invalid.
    #[error("invalid search query: {message}")]
    InvalidQuery {
        /// User-readable validation detail.
        message: String,
    },

    /// The caller cancelled ingestion or search.
    #[error("index operation was cancelled")]
    Cancelled,

    /// A caller-provided bound exceeds the public API ceiling.
    #[error("requested result limit {requested} exceeds maximum {maximum}")]
    ResultLimit {
        /// Requested number of hits.
        requested: usize,
        /// Maximum allowed number of hits.
        maximum: usize,
    },
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database {
            message: error.to_string(),
        }
    }
}

/// Result type used throughout `onenote-index`.
pub type Result<T> = std::result::Result<T, Error>;
