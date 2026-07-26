/// Errors produced while constructing a UI-neutral scene.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Error {
    /// The caller cancelled construction.
    #[error("scene construction was cancelled")]
    Cancelled,

    /// The configured scene-node ceiling was exceeded.
    #[error("page scene exceeds the {maximum}-node limit")]
    NodeLimit {
        /// Configured maximum.
        maximum: usize,
    },
}

/// Result type used throughout `onenote-render`.
pub type Result<T> = std::result::Result<T, Error>;
