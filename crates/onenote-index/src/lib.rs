//! Rebuildable multi-notebook indexing and structured query APIs.

#![forbid(unsafe_code)]

mod document;
mod error;
mod index;
mod model;
pub mod protocol;
mod query;

pub use error::{Error, Result};
pub use index::SearchIndex;
pub use model::{
    IndexProfile, IndexProgress, IndexUpdate, MatchedField, SearchFilters, SearchHit, SearchQuery,
    SourceStatus, TextRange, TextSnippet,
};

/// The crate API version during the pre-1.0 implementation phase.
pub const API_VERSION: u32 = onenote_core::API_VERSION;

/// Current private `SQLite` schema version.
pub const SCHEMA_VERSION: u32 = 2;

/// Version of the model-to-search-document projection.
///
/// Increment this when the same loaded notebook model would produce different
/// indexed documents. Caller-selected loader behavior belongs in
/// [`IndexProfile::configuration`], not in this constant.
pub const INDEX_PROJECTION_VERSION: u32 = 1;
