use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Construct an identifier from its persisted representation.
            ///
            /// Callers normally receive identifiers from the parser or index;
            /// this constructor supports durable storage and protocol adapters.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Return the stable identifier as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

string_id!(SourceId);
string_id!(SourceFingerprint);
string_id!(SectionId);
string_id!(PageId);
string_id!(ObjectId);
string_id!(ResourceId);

/// An RGBA color with straight-alpha channels.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Color {
    /// Red channel.
    pub red: u8,
    /// Green channel.
    pub green: u8,
    /// Blue channel.
    pub blue: u8,
    /// Alpha channel.
    pub alpha: u8,
}

/// A rectangle in logical 96-DPI display pixels.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Rect {
    /// Horizontal position relative to the page origin.
    pub x: f32,
    /// Vertical position relative to the page origin.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

impl Rect {
    /// Return the union of this rectangle and another.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = (self.x + self.width).max(other.x + other.width);
        let bottom = (self.y + self.height).max(other.y + other.height);
        Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    }
}

/// Severity of a non-fatal compatibility diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    /// Information useful to compatibility investigations.
    Info,
    /// Content was partially recovered or omitted.
    Warning,
}

/// A structured non-fatal parsing or compatibility diagnostic.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Diagnostic {
    /// Severity.
    pub severity: DiagnosticSeverity,
    /// Stable machine-readable category.
    pub code: String,
    /// Human-readable detail.
    pub message: String,
    /// Page identifier when the issue is page-specific.
    pub page_id: Option<PageId>,
}

/// A parsed notebook tree and all projected page content.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Notebook {
    /// Stable identity of this source path.
    pub source_id: SourceId,
    /// Metadata-manifest fingerprint of the source tree at parse time.
    pub fingerprint: SourceFingerprint,
    /// Display name derived from the notebook folder.
    pub name: String,
    /// Optional notebook accent color.
    pub color: Option<Color>,
    /// Ordered section and section-group tree.
    pub entries: Vec<NotebookEntry>,
    /// Non-fatal issues collected while parsing.
    pub diagnostics: Vec<Diagnostic>,
}

impl Notebook {
    /// Visit every section in notebook order.
    pub fn sections(&self) -> impl Iterator<Item = &Section> {
        fn append<'a>(entries: &'a [NotebookEntry], output: &mut Vec<&'a Section>) {
            for entry in entries {
                match entry {
                    NotebookEntry::Section(section) => output.push(section),
                    NotebookEntry::Group(group) => append(&group.entries, output),
                }
            }
        }

        let mut sections = Vec::new();
        append(&self.entries, &mut sections);
        sections.into_iter()
    }

    /// Visit every page in notebook order.
    pub fn pages(&self) -> impl Iterator<Item = &Page> {
        self.sections().flat_map(|section| section.pages.iter())
    }
}

/// An ordered notebook tree entry.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NotebookEntry {
    /// A native `.one` section.
    Section(Section),
    /// A folder-backed group of sections.
    Group(SectionGroup),
}

/// A group in the notebook section tree.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SectionGroup {
    /// Stable identity within the source.
    pub id: SectionId,
    /// Display name.
    pub name: String,
    /// Ordered children.
    pub entries: Vec<NotebookEntry>,
}

/// A native `OneNote` section.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Section {
    /// Stable identity within the source.
    pub id: SectionId,
    /// Display name.
    pub name: String,
    /// Optional section tab color.
    pub color: Option<Color>,
    /// Ordered pages, including subpage levels.
    pub pages: Vec<Page>,
    /// Non-fatal section diagnostics.
    pub diagnostics: Vec<Diagnostic>,
}

/// A native `OneNote` page.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Page {
    /// Stable page identity.
    pub id: PageId,
    /// Native link target GUID when present.
    pub native_id: String,
    /// Display title.
    pub title: String,
    /// Depth in the section page hierarchy.
    pub level: i32,
    /// Creation time in ISO-8601 form.
    pub created_at: String,
    /// Last update time in ISO-8601 form.
    pub updated_at: String,
    /// Optional author metadata.
    pub author: Option<String>,
    /// Page height when explicitly recorded.
    pub height: Option<f32>,
    /// Positioned objects in source stacking order.
    pub objects: Vec<PageObject>,
}

impl Page {
    /// Build searchable visible text without loading binary resources.
    pub fn visible_text(&self) -> String {
        let mut output = String::new();
        for object in &self.objects {
            object.append_visible_text(&mut output);
        }
        output
    }

    /// Bounding box containing every projected page object.
    pub fn content_bounds(&self) -> Option<Rect> {
        self.objects
            .iter()
            .map(|object| object.bounds)
            .reduce(Rect::union)
    }
}

/// A positioned top-level page object.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PageObject {
    /// Stable identity within the page.
    pub id: ObjectId,
    /// Position and extent in logical display pixels.
    pub bounds: Rect,
    /// Source stacking order. Higher values are later in source order.
    pub z_index: u32,
    /// Projected object content.
    pub kind: ObjectKind,
}

impl PageObject {
    /// Build searchable visible text for this object without loading resources.
    pub fn visible_text(&self) -> String {
        let mut output = String::new();
        self.append_visible_text(&mut output);
        output
    }

    fn append_visible_text(&self, output: &mut String) {
        self.kind.append_visible_text(output);
    }
}

/// Content of a positioned page object.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ObjectKind {
    /// A freeform text/content outline.
    Outline(Outline),
    /// A directly positioned image.
    Image(Image),
    /// A directly positioned attachment.
    Attachment(Attachment),
    /// A directly positioned ink drawing.
    Ink(Ink),
    /// Unsupported content retained as an explicit placeholder.
    Unknown,
}

impl ObjectKind {
    fn append_visible_text(&self, output: &mut String) {
        match self {
            Self::Outline(outline) => outline.append_visible_text(output),
            Self::Image(image) => append_text(output, image.search_text.as_deref()),
            Self::Attachment(attachment) => append_text(output, Some(&attachment.resource.name)),
            Self::Ink(ink) => append_text(output, ink.recognized_text.as_deref()),
            Self::Unknown => {}
        }
    }
}

/// A freeform outline with source layout properties and nested elements.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Outline {
    /// Initial nesting level.
    pub child_level: u8,
    /// Per-level indent distances in logical pixels.
    pub indents: Vec<f32>,
    /// Whether the width was explicitly set by the author.
    pub user_sized: bool,
    /// Ordered semantic elements.
    pub elements: Vec<OutlineElement>,
}

impl Outline {
    fn append_visible_text(&self, output: &mut String) {
        for element in &self.elements {
            element.append_visible_text(output);
        }
    }
}

/// An outline element and any nested child elements.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OutlineElement {
    /// Source nesting level.
    pub level: u8,
    /// Optional list marker definition.
    pub list: Option<ListMarker>,
    /// Ordered inline/block content.
    pub content: Vec<ElementContent>,
    /// Nested elements.
    pub children: Vec<OutlineElement>,
}

impl OutlineElement {
    fn append_visible_text(&self, output: &mut String) {
        for content in &self.content {
            content.append_visible_text(output);
        }
        for child in &self.children {
            child.append_visible_text(output);
        }
    }
}

/// Formatting for a bullet or numbered-list marker.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ListMarker {
    /// `OneNote` list format expression.
    pub format: String,
    /// Optional numbering restart.
    pub restart: Option<i32>,
    /// Optional marker font.
    pub font: Option<String>,
}

/// Content inside an outline element or table cell.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ElementContent {
    /// Rich text.
    Text(TextBlock),
    /// Nested table.
    Table(Table),
    /// Inline image.
    Image(Image),
    /// Inline attachment.
    Attachment(Attachment),
    /// Inline ink.
    Ink(Ink),
    /// Unsupported content retained as a placeholder.
    Unknown,
}

impl ElementContent {
    fn append_visible_text(&self, output: &mut String) {
        match self {
            Self::Text(text) => append_text(output, Some(&text.visible_text())),
            Self::Table(table) => {
                for row in &table.rows {
                    for cell in row {
                        for element in &cell.elements {
                            element.append_visible_text(output);
                        }
                    }
                }
            }
            Self::Image(image) => append_text(output, image.search_text.as_deref()),
            Self::Attachment(attachment) => append_text(output, Some(&attachment.resource.name)),
            Self::Ink(ink) => append_text(output, ink.recognized_text.as_deref()),
            Self::Unknown => {}
        }
    }
}

/// A rich-text paragraph with UTF-16 source run boundaries.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TextBlock {
    /// Original paragraph text.
    pub text: String,
    /// Base paragraph style.
    pub base_style: TextStyle,
    /// Style runs whose offsets use `OneNote`'s UTF-16 code-unit indexing.
    pub runs: Vec<TextRun>,
    /// Paragraph alignment.
    pub alignment: TextAlignment,
    /// Top margin in logical pixels.
    pub space_before: f32,
    /// Bottom margin in logical pixels.
    pub space_after: f32,
    /// Exact line spacing in logical pixels when recorded.
    pub line_spacing: Option<f32>,
}

impl TextBlock {
    /// Return display text after excluding source runs marked hidden.
    pub fn visible_text(&self) -> String {
        if self.runs.is_empty() {
            return if self.base_style.hidden {
                String::new()
            } else {
                self.text.clone()
            };
        }

        let mut output = String::with_capacity(self.text.len());
        let mut utf16_offset = 0_u32;
        let mut run_index = 0_usize;
        for character in self.text.chars() {
            while self
                .runs
                .get(run_index)
                .is_some_and(|run| utf16_offset >= run.end_utf16)
            {
                run_index += 1;
            }
            let hidden = self
                .runs
                .get(run_index)
                .filter(|run| utf16_offset >= run.start_utf16)
                .map_or(self.base_style.hidden, |run| run.style.hidden);
            if !hidden {
                output.push(character);
            }
            utf16_offset += if character.len_utf16() == 1 { 1 } else { 2 };
        }
        output
    }
}

/// A styled range in a rich-text paragraph.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TextRun {
    /// Inclusive UTF-16 start offset.
    pub start_utf16: u32,
    /// Exclusive UTF-16 end offset.
    pub end_utf16: u32,
    /// Style applied to this range.
    pub style: TextStyle,
}

/// Text and paragraph styling independent of any UI toolkit.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct TextStyle {
    /// Font family.
    pub font: Option<String>,
    /// Font size in typographic points.
    pub font_size: Option<f32>,
    /// Foreground color; `None` means automatic.
    pub foreground: Option<Color>,
    /// Highlight color; `None` means automatic or absent.
    pub highlight: Option<Color>,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underline.
    pub underline: bool,
    /// Strike-through.
    pub strikethrough: bool,
    /// Superscript.
    pub superscript: bool,
    /// Subscript.
    pub subscript: bool,
    /// Hidden source marker text.
    pub hidden: bool,
    /// Hyperlink display or marker run.
    pub hyperlink: bool,
    /// Protected hyperlink display text.
    pub hyperlink_protected: bool,
    /// LCID language code.
    pub language_code: Option<u32>,
}

/// Paragraph alignment.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlignment {
    /// Left aligned or unspecified.
    #[default]
    Left,
    /// Center aligned.
    Center,
    /// Right aligned.
    Right,
    /// Source value not understood.
    Unknown,
}

/// A table with source column sizing and nested cell content.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Table {
    /// Rows and cells in source order.
    pub rows: Vec<Vec<TableCell>>,
    /// Column widths in logical pixels.
    pub column_widths: Vec<f32>,
    /// Per-column locked-width flags.
    pub locked_columns: Vec<bool>,
    /// Whether cell borders are visible.
    pub borders_visible: bool,
}

/// A table cell.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TableCell {
    /// Optional cell background.
    pub background: Option<Color>,
    /// Maximum source width in logical pixels.
    pub max_width: Option<f32>,
    /// Nested outline elements.
    pub elements: Vec<OutlineElement>,
}

/// Metadata and lazy-payload reference for an image.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Image {
    /// Lazy binary payload.
    pub resource: ResourceRef,
    /// Display width in logical pixels.
    pub width: Option<f32>,
    /// Display height in logical pixels.
    pub height: Option<f32>,
    /// Alternative or OCR text.
    pub alt_text: Option<String>,
    /// Additional recognized/searchable text.
    pub search_text: Option<String>,
    /// Optional hyperlink.
    pub hyperlink: Option<String>,
    /// Whether the image is a page background.
    pub is_background: bool,
}

/// Metadata and lazy-payload reference for an embedded attachment.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Attachment {
    /// Lazy binary payload.
    pub resource: ResourceRef,
    /// Display width in logical pixels.
    pub width: Option<f32>,
    /// Display height in logical pixels.
    pub height: Option<f32>,
}

/// Metadata for a payload retained in a [`crate::ResourceStore`].
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResourceRef {
    /// Stable resource identity.
    pub id: ResourceId,
    /// Untrusted source display filename, never a filesystem path.
    pub name: String,
    /// Best-effort media type.
    pub media_type: String,
    /// Declared byte size.
    pub size: u64,
}

/// A projected ink drawing.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Ink {
    /// Ink paths, including paths nested in source ink groups.
    pub strokes: Vec<InkStroke>,
    /// Optional handwriting recognition text.
    pub recognized_text: Option<String>,
}

/// A single ink path.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InkStroke {
    /// Path points in source ink coordinates.
    pub points: Vec<InkPoint>,
    /// Source stroke width.
    pub width: f32,
    /// Source stroke height.
    pub height: f32,
    /// Best-effort stroke color.
    pub color: Option<Color>,
    /// Source opacity.
    pub opacity: u8,
    /// Per-stroke recognized word.
    pub recognized_word: Option<String>,
}

/// A point in an ink path.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct InkPoint {
    /// Horizontal coordinate.
    pub x: f32,
    /// Vertical coordinate.
    pub y: f32,
}

fn append_text(output: &mut String, text: Option<&str>) {
    let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
        return;
    };
    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str(text);
}

#[cfg(test)]
mod tests {
    use super::{TextBlock, TextRun, TextStyle};

    #[test]
    fn visible_text_uses_utf16_run_offsets() {
        let visible = TextStyle::default();
        let hidden = TextStyle {
            hidden: true,
            ..TextStyle::default()
        };
        let text = TextBlock {
            text: "A😀hidden Z".to_owned(),
            base_style: visible.clone(),
            runs: vec![
                TextRun {
                    start_utf16: 0,
                    end_utf16: 3,
                    style: visible.clone(),
                },
                TextRun {
                    start_utf16: 3,
                    end_utf16: 9,
                    style: hidden,
                },
                TextRun {
                    start_utf16: 9,
                    end_utf16: 11,
                    style: visible,
                },
            ],
            alignment: super::TextAlignment::default(),
            space_before: 0.0,
            space_after: 0.0,
            line_spacing: None,
        };

        assert_eq!(text.visible_text(), "A😀 Z");
    }
}
