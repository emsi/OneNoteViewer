use onenote_core::{Color, InkPoint};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A locally authored ink stroke layered over an immutable OneNote page.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EditableStroke {
    /// Points in page logical pixels.
    pub points: Vec<InkPoint>,
    /// Stroke color.
    pub color: Color,
    /// Width in page logical pixels.
    pub width: f32,
}

/// Stable reference to one imported stroke hidden by the local edit layer.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ErasedStroke {
    /// Stable source object identity.
    pub object_id: String,
    /// Position in that object's original stroke list.
    pub stroke_index: usize,
}

/// A locally authored text box placed anywhere on the page.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EditableTextBox {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub text: String,
}

/// A locally added image placed anywhere on the page.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EditableImage {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub file_path: String,
}

/// Edits saved alongside the source section, never inside the `.one` file.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct InkEditDocument {
    /// New strokes authored in this application.
    pub added_strokes: Vec<EditableStroke>,
    /// Original strokes hidden in this application.
    pub erased_strokes: Vec<ErasedStroke>,
    /// Text replacements keyed by stable scene-node identity.
    #[serde(default)]
    pub text_replacements: BTreeMap<String, String>,
    /// Text boxes placed anywhere on the page.
    #[serde(default)]
    pub added_text_boxes: Vec<EditableTextBox>,
    /// Images placed anywhere on the page.
    #[serde(default)]
    pub added_images: Vec<EditableImage>,
}

/// Active pointer tool for the page canvas.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InkEditTool {
    /// Normal viewing and navigation.
    #[default]
    View,
    /// Add a blue freehand stroke.
    Pen,
    /// Hide imported ink strokes locally.
    Eraser,
}
