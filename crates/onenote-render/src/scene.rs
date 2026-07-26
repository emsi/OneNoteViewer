use onenote_core::{
    Attachment, Color, Image, InkStroke, ObjectId, PageId, Rect, ResourceId, TextBlock,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Stable identity of a generated scene node within one page scene.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SceneNodeId(pub String);

/// An immutable retained page scene.
#[derive(Clone, Debug)]
pub struct PageScene {
    /// Source page identity.
    pub page_id: PageId,
    /// Logical canvas bounds, including negative source coordinates.
    pub bounds: Rect,
    /// Draw nodes in ascending stacking and construction order.
    pub nodes: Vec<SceneNode>,
    /// Interactive regions.
    pub hit_regions: Vec<HitRegion>,
    /// Compatibility and approximation diagnostics.
    pub diagnostics: Vec<SceneDiagnostic>,
}

impl PageScene {
    /// Visit nodes intersecting a viewport expanded by `overscan` pixels.
    pub fn visible_nodes(&self, viewport: Rect, overscan: f32) -> impl Iterator<Item = &SceneNode> {
        let overscan = overscan.max(0.0);
        let expanded = Rect {
            x: viewport.x - overscan,
            y: viewport.y - overscan,
            width: viewport.width + overscan * 2.0,
            height: viewport.height + overscan * 2.0,
        };
        self.nodes
            .iter()
            .filter(move |node| intersects(node.bounds, expanded))
    }

    /// Return the highest-stacked hit region containing a logical point.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<&HitRegion> {
        self.hit_regions
            .iter()
            .rev()
            .find(|region| contains(region.bounds, x, y))
    }
}

/// One retained draw node.
#[derive(Clone, Debug)]
pub struct SceneNode {
    /// Generated identity.
    pub id: SceneNodeId,
    /// Owning top-level source object.
    pub source_object_id: ObjectId,
    /// Conservative logical bounds used for culling and hit testing.
    pub bounds: Rect,
    /// Effective stacking order.
    pub z_index: i32,
    /// UI-neutral content.
    pub primitive: ScenePrimitive,
    /// Accessibility role and text.
    pub accessibility: AccessibilitySemantics,
}

/// UI-neutral retained draw primitive.
#[derive(Clone, Debug)]
pub enum ScenePrimitive {
    /// Solid rectangle, used for cell shading and placeholders.
    Fill {
        /// Fill color.
        color: Color,
        /// Corner radius in logical pixels.
        corner_radius: f32,
    },
    /// One rich-text paragraph and its layout constraint.
    Text {
        /// Shared source text and run formatting.
        block: Arc<TextBlock>,
        /// Optional bullet or number marker.
        marker: Option<String>,
    },
    /// Lazy image reference.
    Image(Image),
    /// Inert attachment representation.
    Attachment(Attachment),
    /// Ink strokes in source ink coordinates, translated by the node bounds.
    Ink {
        /// Flattened stroke list.
        strokes: Vec<InkStroke>,
    },
    /// One table/grid line.
    Line {
        /// Line color.
        color: Color,
        /// Stroke width in logical pixels.
        width: f32,
        /// End point.
        to_x: f32,
        /// End point.
        to_y: f32,
    },
    /// Explicit unsupported-content surface.
    Placeholder {
        /// Stable placeholder category.
        label: String,
    },
}

/// Semantic role exposed by renderer adapters.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessibilityRole {
    /// Paragraph or text run collection.
    Text,
    /// Image/printout.
    Image,
    /// Embedded file.
    Attachment,
    /// Table.
    Table,
    /// Table cell.
    TableCell,
    /// Ink/drawing surface.
    Drawing,
    /// Unsupported content placeholder.
    Unknown,
    /// Decorative surface excluded from normal reading.
    Decoration,
}

/// UI-neutral accessibility data.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AccessibilitySemantics {
    /// Semantic role.
    pub role: AccessibilityRole,
    /// Short name.
    pub label: String,
    /// Optional longer plain-text description.
    pub description: Option<String>,
}

impl AccessibilitySemantics {
    pub(crate) fn decoration() -> Self {
        Self {
            role: AccessibilityRole::Decoration,
            label: String::new(),
            description: None,
        }
    }
}

/// One interactive logical region.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HitRegion {
    /// Generated node identity.
    pub node_id: SceneNodeId,
    /// Owning source object.
    pub source_object_id: ObjectId,
    /// Logical bounds.
    pub bounds: Rect,
    /// Host-owned action.
    pub action: HitAction,
}

/// Inert action returned to an embedding host.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum HitAction {
    /// Ask the host to open or confirm an external URI.
    OpenLink(String),
    /// Ask the host to handle an attachment payload.
    OpenAttachment(ResourceId),
    /// Select/focus the source page object.
    SelectObject(ObjectId),
}

/// Non-fatal scene compatibility diagnostic.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SceneDiagnostic {
    /// Stable category.
    pub code: String,
    /// Bounded human-readable detail.
    pub message: String,
    /// Owning object.
    pub object_id: Option<ObjectId>,
}

fn intersects(left: Rect, right: Rect) -> bool {
    left.x <= right.x + right.width
        && left.x + left.width >= right.x
        && left.y <= right.y + right.height
        && left.y + left.height >= right.y
}

fn contains(rect: Rect, x: f32, y: f32) -> bool {
    x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
}
