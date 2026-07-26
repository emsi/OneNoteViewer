//! Source-native `OneNote` discovery, parsing, and public domain types.

#![forbid(unsafe_code)]

mod error;
mod model;
mod package;
mod parser;
mod resource;

pub use error::{Error, Result};
pub use model::{
    Attachment, Color, Diagnostic, DiagnosticSeverity, ElementContent, Image, Ink, InkPoint,
    InkStroke, ListMarker, Notebook, NotebookEntry, ObjectId, ObjectKind, Outline, OutlineElement,
    Page, PageId, PageObject, Rect, ResourceId, ResourceRef, Section, SectionGroup, SectionId,
    SourceId, Table, TableCell, TextAlignment, TextBlock, TextRun, TextStyle,
};
pub use package::{ExtractionReport, OnePkgExtractor};
pub use parser::{LoadedNotebook, OneNoteLoader, ParseLimits};
pub use resource::ResourceStore;

/// The crate API version during the pre-1.0 implementation phase.
pub const API_VERSION: u32 = 1;

/// Logical display pixels per `OneNote` half-inch layout unit at 96 DPI.
pub const PIXELS_PER_HALF_INCH: f32 = 48.0;
