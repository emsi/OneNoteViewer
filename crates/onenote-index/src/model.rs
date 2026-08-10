use onenote_core::{ObjectId, PageId, Rect, SectionId, SourceFingerprint, SourceId};
use serde::{Deserialize, Serialize};

/// Progress emitted synchronously while indexing a source.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IndexProgress {
    /// Source being indexed.
    pub source_id: SourceId,
    /// Pages committed to the current un-published transaction.
    pub pages_completed: usize,
    /// Total pages in the submitted source model.
    pub pages_total: usize,
}

/// Persisted status of one indexed notebook source.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourceStatus {
    /// Stable source identity.
    pub source_id: SourceId,
    /// Fingerprint of the fully published generation.
    pub fingerprint: SourceFingerprint,
    /// Notebook display name.
    pub notebook_name: String,
    /// Indexed pages.
    pub page_count: usize,
}

/// Structured filters shared by library and protocol clients.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SearchFilters {
    /// Empty means all indexed sources.
    pub source_ids: Vec<SourceId>,
    /// Empty means every section.
    pub section_ids: Vec<SectionId>,
    /// Restrict to pages with or without attachments.
    pub has_attachments: Option<bool>,
    /// Inclusive ISO-8601 lower bound.
    pub updated_after: Option<String>,
    /// Inclusive ISO-8601 upper bound.
    pub updated_before: Option<String>,
}

/// A bounded, structured search request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SearchQuery {
    /// Ordinary query text with optional quoted phrases and trailing prefixes.
    pub text: String,
    /// Structured metadata filters.
    #[serde(default)]
    pub filters: SearchFilters,
    /// Maximum results, clamped by the API ceiling.
    #[serde(default = "default_result_limit")]
    pub limit: usize,
    /// Maximum displayed snippet characters.
    #[serde(default = "default_snippet_characters")]
    pub snippet_characters: usize,
}

impl SearchQuery {
    /// Construct a simple all-source query with safe defaults.
    pub fn simple(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            filters: SearchFilters::default(),
            limit: default_result_limit(),
            snippet_characters: default_snippet_characters(),
        }
    }
}

/// Logical field most likely responsible for a match.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchedField {
    /// Page title.
    Title,
    /// Visible rich text.
    Body,
    /// Image alternative/OCR text.
    AltText,
    /// Stored handwriting recognition.
    InkText,
    /// Embedded filename/type.
    Attachment,
    /// Visible or normalized link text.
    Link,
    /// Notebook/group/section path.
    Path,
    /// Match could not be attributed more narrowly.
    Other,
}

/// Byte range to highlight within [`TextSnippet::text`].
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TextRange {
    /// Inclusive UTF-8 byte offset.
    pub start_byte: usize,
    /// Exclusive UTF-8 byte offset.
    pub end_byte: usize,
}

/// Plain-text snippet and UI-native highlight ranges.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TextSnippet {
    /// Original plain text with no HTML markup.
    pub text: String,
    /// Non-overlapping match ranges.
    pub highlights: Vec<TextRange>,
}

/// One ranked, resolvable page match.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SearchHit {
    /// Higher is better; only comparable within this result set.
    pub rank: f64,
    /// Most likely matching logical field.
    pub matched_field: MatchedField,
    /// Bounded plain-text context.
    pub snippet: TextSnippet,
    /// Fingerprint of the indexed source generation.
    pub source_fingerprint: SourceFingerprint,
    /// Stable source locator.
    pub source_id: SourceId,
    /// Notebook display context.
    pub notebook_name: String,
    /// Stable section locator.
    pub section_id: SectionId,
    /// Section display context.
    pub section_name: String,
    /// Full notebook/group/section breadcrumb for display, never identity.
    #[serde(default)]
    pub path: String,
    /// Stable page locator.
    pub page_id: PageId,
    /// Page display title.
    pub page_title: String,
    /// Nearest matching top-level object when resolvable.
    pub object_id: Option<ObjectId>,
    /// Geometry of `object_id`, in logical pixels.
    pub bounds: Option<Rect>,
    /// Last source update time.
    pub updated_at: String,
}

const fn default_result_limit() -> usize {
    50
}

const fn default_snippet_characters() -> usize {
    240
}
