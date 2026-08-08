//! Source-native `OneNote` discovery, parsing, and public domain types.

#![forbid(unsafe_code)]

mod error;
mod math;
mod model;
mod package;
mod parser;
mod resource;

pub use error::{Error, Result};
pub use math::{MathExpression, MathNode, MathSpan};
pub use model::{
    Attachment, Color, Diagnostic, DiagnosticSeverity, ElementContent, Image, Ink, InkPoint,
    InkStroke, ListMarker, ListMarkerPart, ListNumberFormat, Notebook, NotebookEntry, ObjectId,
    ObjectKind, Outline, OutlineElement, Page, PageId, PageObject, PageObjectRole, Rect,
    ResourceId, ResourceRef, ResourceStatus, Section, SectionGroup, SectionId, SourceFingerprint,
    SourceId, Table, TableCell, TextAlignment, TextBlock, TextLink, TextLinkOrigin, TextRun,
    TextStyle,
};
pub use package::{ExtractionPhase, ExtractionReport, OnePkgExtractor};
pub use parser::{LoadOptions, LoadedNotebook, OneNoteLoader, ParseLimits};
pub use resource::{
    ResourceCopyControl, ResourceCopyOptions, ResourceCopyProgress, ResourceCopyReport,
    ResourceStore,
};

/// The crate API version during the pre-1.0 implementation phase.
pub const API_VERSION: u32 = 7;

/// Logical display pixels per `OneNote` half-inch layout unit at 96 DPI.
pub const PIXELS_PER_HALF_INCH: f32 = 48.0;
