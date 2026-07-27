use crate::scene::{
    AccessibilityRole, AccessibilitySemantics, HitAction, HitRegion, PageScene, SceneDiagnostic,
    SceneNode, SceneNodeId, ScenePrimitive,
};
use crate::{Error, Result};
use onenote_core::{
    Attachment, Color, ElementContent, Image, Ink, ListMarker, ObjectKind, Outline, OutlineElement,
    Page, PageObject, PageObjectRole, Rect, Table, TextBlock,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const DEFAULT_TEXT_COLOR: Color = Color {
    red: 32,
    green: 31,
    blue: 30,
    alpha: 255,
};
const TABLE_BORDER_COLOR: Color = Color {
    red: 166,
    green: 166,
    blue: 166,
    alpha: 255,
};
const PLACEHOLDER_COLOR: Color = Color {
    red: 243,
    green: 242,
    blue: 241,
    alpha: 255,
};

/// Deterministic scene-construction options.
#[derive(Clone, Copy, Debug)]
pub struct SceneOptions {
    /// Minimum logical canvas width.
    pub minimum_canvas_width: f32,
    /// Minimum logical canvas height.
    pub minimum_canvas_height: f32,
    /// Padding inside freeform outlines and table cells.
    pub content_padding: f32,
    /// Default paragraph font size in points for conservative measurement.
    pub default_font_size: f32,
    /// Default line-height multiplier.
    pub line_height: f32,
    /// Maximum generated nodes per page.
    pub max_nodes: usize,
    /// Include native title/date objects in the generated scene.
    pub include_page_title: bool,
    /// Crop the scene bounds to the included content.
    pub crop_to_content: bool,
}

impl Default for SceneOptions {
    fn default() -> Self {
        Self {
            minimum_canvas_width: 1_280.0,
            minimum_canvas_height: 900.0,
            content_padding: 8.0,
            default_font_size: 11.0,
            line_height: 1.35,
            max_nodes: 1_000_000,
            include_page_title: true,
            crop_to_content: false,
        }
    }
}

/// Stateless UI-neutral page scene builder.
#[derive(Clone, Copy, Debug, Default)]
pub struct SceneBuilder {
    options: SceneOptions,
}

impl SceneBuilder {
    /// Construct a builder with explicit deterministic options.
    pub fn with_options(options: SceneOptions) -> Self {
        Self { options }
    }

    /// Build an immutable scene for one source page.
    ///
    /// # Errors
    ///
    /// Returns cancellation or node-limit errors. Unsupported source content
    /// produces placeholders and diagnostics instead of failing the page.
    pub fn build(&self, page: &Page, cancel: &AtomicBool) -> Result<PageScene> {
        let mut state = BuildState {
            options: self.options,
            cancel,
            nodes: Vec::new(),
            hit_regions: Vec::new(),
            diagnostics: Vec::new(),
            sequence: 0,
        };
        state.check_cancelled()?;
        for object in &page.objects {
            state.check_cancelled()?;
            if !self.options.include_page_title && object.role == PageObjectRole::Title {
                continue;
            }
            state.object(object)?;
        }
        state.nodes.sort_by_key(|node| node.z_index);
        state.hit_regions.sort_by_key(|region| {
            state
                .nodes
                .iter()
                .find(|node| node.id == region.node_id)
                .map_or(i32::MIN, |node| node.z_index)
        });
        let bounds = scene_bounds(page, &state.nodes, self.options);
        Ok(PageScene {
            page_id: page.id.clone(),
            bounds,
            nodes: state.nodes,
            hit_regions: state.hit_regions,
            diagnostics: state.diagnostics,
        })
    }
}

struct BuildState<'a> {
    options: SceneOptions,
    cancel: &'a AtomicBool,
    nodes: Vec<SceneNode>,
    hit_regions: Vec<HitRegion>,
    diagnostics: Vec<SceneDiagnostic>,
    sequence: u64,
}

impl BuildState<'_> {
    fn object(&mut self, object: &PageObject) -> Result<()> {
        match &object.kind {
            ObjectKind::Outline(outline) => self.outline(object, outline),
            ObjectKind::Image(image) => {
                self.image(object, object.bounds, image.clone(), object_z(object, 0))
            }
            ObjectKind::Attachment(attachment) => self.attachment(
                object,
                object.bounds,
                attachment.clone(),
                object_z(object, 0),
            ),
            ObjectKind::Ink(ink) => self.ink(object, object.bounds, ink, object_z(object, 0)),
            ObjectKind::Unknown => self.placeholder(object, object.bounds, object_z(object, 0)),
        }
    }

    fn outline(&mut self, object: &PageObject, outline: &Outline) -> Result<()> {
        let mut cursor = Cursor {
            x: object.bounds.x + self.options.content_padding,
            y: object.bounds.y + self.options.content_padding,
            width: (object.bounds.width - self.options.content_padding * 2.0).max(40.0),
        };
        for element in &outline.elements {
            self.element(object, element, outline, &mut cursor, object_z(object, 0))?;
        }
        Ok(())
    }

    fn element(
        &mut self,
        object: &PageObject,
        element: &OutlineElement,
        outline: &Outline,
        cursor: &mut Cursor,
        z_index: i32,
    ) -> Result<()> {
        self.check_cancelled()?;
        let indent = outline_indent(outline, element.level);
        let original_x = cursor.x;
        let original_width = cursor.width;
        cursor.x += indent;
        cursor.width = (cursor.width - indent).max(40.0);
        let marker = element.list.as_ref().map(list_marker_text);
        for content in &element.content {
            self.content(object, content, cursor, z_index, marker.clone())?;
        }
        for child in &element.children {
            self.element(object, child, outline, cursor, z_index)?;
        }
        cursor.x = original_x;
        cursor.width = original_width;
        Ok(())
    }

    fn content(
        &mut self,
        object: &PageObject,
        content: &ElementContent,
        cursor: &mut Cursor,
        z_index: i32,
        marker: Option<String>,
    ) -> Result<()> {
        match content {
            ElementContent::Text(text) => self.text(object, text, cursor, z_index, marker),
            ElementContent::Table(table) => self.table(object, table, cursor, z_index),
            ElementContent::Image(image) => {
                let bounds = inline_bounds(cursor, image.width, image.height, 240.0, 180.0);
                self.image(object, bounds, image.clone(), z_index)?;
                cursor.y += bounds.height + self.options.content_padding;
                Ok(())
            }
            ElementContent::Attachment(attachment) => {
                let bounds =
                    inline_bounds(cursor, attachment.width, attachment.height, 220.0, 44.0);
                self.attachment(object, bounds, attachment.clone(), z_index)?;
                cursor.y += bounds.height + self.options.content_padding;
                Ok(())
            }
            ElementContent::Ink(ink) => {
                let bounds = Rect {
                    x: cursor.x,
                    y: cursor.y,
                    width: cursor.width,
                    height: ink_height(ink).max(32.0),
                };
                self.ink(object, bounds, ink, z_index)?;
                cursor.y += bounds.height + self.options.content_padding;
                Ok(())
            }
            ElementContent::Unknown => {
                let bounds = Rect {
                    x: cursor.x,
                    y: cursor.y,
                    width: cursor.width.min(240.0),
                    height: 44.0,
                };
                self.placeholder(object, bounds, z_index)?;
                cursor.y += bounds.height + self.options.content_padding;
                Ok(())
            }
        }
    }

    fn text(
        &mut self,
        object: &PageObject,
        text: &TextBlock,
        cursor: &mut Cursor,
        z_index: i32,
        marker: Option<String>,
    ) -> Result<()> {
        let height = estimate_text_height(text, cursor.width, self.options);
        let bounds = Rect {
            x: cursor.x,
            y: cursor.y + text.space_before,
            width: cursor.width,
            height,
        };
        let visible = text.visible_text();
        self.push_node(
            object,
            bounds,
            z_index,
            ScenePrimitive::Text {
                block: Arc::new(text.clone()),
                marker,
            },
            AccessibilitySemantics {
                role: AccessibilityRole::Text,
                label: bounded_label(&visible, 256),
                description: None,
            },
        )?;
        cursor.y = bounds.y + bounds.height + text.space_after;
        Ok(())
    }

    fn table(
        &mut self,
        object: &PageObject,
        table: &Table,
        cursor: &mut Cursor,
        z_index: i32,
    ) -> Result<()> {
        let column_count = table
            .rows
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(table.column_widths.len())
            .max(1);
        let widths = table_widths(table, column_count, cursor.width);
        let table_x = cursor.x;
        let table_y = cursor.y;
        let mut y = table_y;
        for row in &table.rows {
            self.check_cancelled()?;
            let row_height = row
                .iter()
                .enumerate()
                .map(|(index, cell)| {
                    estimate_elements_height(
                        &cell.elements,
                        widths.get(index).copied().unwrap_or(80.0),
                        self.options,
                    )
                })
                .fold(32.0_f32, f32::max);
            let mut x = table_x;
            for (column, width) in widths.iter().copied().enumerate() {
                let cell_bounds = Rect {
                    x,
                    y,
                    width,
                    height: row_height,
                };
                if let Some(cell) = row.get(column) {
                    if let Some(background) = cell.background {
                        self.push_node(
                            object,
                            cell_bounds,
                            z_index,
                            ScenePrimitive::Fill {
                                color: background,
                                corner_radius: 0.0,
                            },
                            AccessibilitySemantics::decoration(),
                        )?;
                    }
                    let mut cell_cursor = Cursor {
                        x: x + self.options.content_padding,
                        y: y + self.options.content_padding,
                        width: (width - self.options.content_padding * 2.0).max(20.0),
                    };
                    let synthetic_outline = Outline {
                        child_level: 0,
                        indents: Vec::new(),
                        user_sized: false,
                        elements: Vec::new(),
                    };
                    for element in &cell.elements {
                        self.element(
                            object,
                            element,
                            &synthetic_outline,
                            &mut cell_cursor,
                            z_index + 1,
                        )?;
                    }
                }
                if table.borders_visible {
                    self.table_cell_lines(object, cell_bounds, z_index + 2)?;
                }
                x += width;
            }
            y += row_height;
        }
        let bounds = Rect {
            x: table_x,
            y: table_y,
            width: widths.iter().sum(),
            height: (y - table_y).max(1.0),
        };
        self.push_node(
            object,
            bounds,
            z_index - 1,
            ScenePrimitive::Fill {
                color: Color {
                    alpha: 0,
                    ..DEFAULT_TEXT_COLOR
                },
                corner_radius: 0.0,
            },
            AccessibilitySemantics {
                role: AccessibilityRole::Table,
                label: format!("Table, {} rows, {column_count} columns", table.rows.len()),
                description: None,
            },
        )?;
        cursor.y = y + self.options.content_padding;
        Ok(())
    }

    fn table_cell_lines(&mut self, object: &PageObject, bounds: Rect, z_index: i32) -> Result<()> {
        for (start, end) in [
            ((bounds.x, bounds.y), (bounds.x + bounds.width, bounds.y)),
            ((bounds.x, bounds.y), (bounds.x, bounds.y + bounds.height)),
            (
                (bounds.x + bounds.width, bounds.y),
                (bounds.x + bounds.width, bounds.y + bounds.height),
            ),
            (
                (bounds.x, bounds.y + bounds.height),
                (bounds.x + bounds.width, bounds.y + bounds.height),
            ),
        ] {
            self.push_node(
                object,
                Rect {
                    x: start.0,
                    y: start.1,
                    width: (end.0 - start.0).abs().max(1.0),
                    height: (end.1 - start.1).abs().max(1.0),
                },
                z_index,
                ScenePrimitive::Line {
                    color: TABLE_BORDER_COLOR,
                    width: 1.0,
                    to_x: end.0,
                    to_y: end.1,
                },
                AccessibilitySemantics::decoration(),
            )?;
        }
        Ok(())
    }

    fn image(
        &mut self,
        object: &PageObject,
        bounds: Rect,
        image: Image,
        z_index: i32,
    ) -> Result<()> {
        let hyperlink = image.hyperlink.clone();
        let label = image
            .alt_text
            .clone()
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| image.resource.name.clone());
        let node_id = self.push_node(
            object,
            bounds,
            if image.is_background {
                z_index.saturating_sub(10_000)
            } else {
                z_index
            },
            ScenePrimitive::Image(image),
            AccessibilitySemantics {
                role: AccessibilityRole::Image,
                label,
                description: None,
            },
        )?;
        if let Some(hyperlink) = hyperlink {
            self.hit_regions.push(HitRegion {
                node_id,
                source_object_id: object.id.clone(),
                bounds,
                action: HitAction::OpenLink(hyperlink),
            });
        }
        Ok(())
    }

    fn attachment(
        &mut self,
        object: &PageObject,
        bounds: Rect,
        attachment: Attachment,
        z_index: i32,
    ) -> Result<()> {
        let resource_id = attachment.resource.id.clone();
        let label = attachment.resource.name.clone();
        let node_id = self.push_node(
            object,
            bounds,
            z_index,
            ScenePrimitive::Attachment(attachment),
            AccessibilitySemantics {
                role: AccessibilityRole::Attachment,
                label,
                description: Some("Embedded file".to_owned()),
            },
        )?;
        self.hit_regions.push(HitRegion {
            node_id,
            source_object_id: object.id.clone(),
            bounds,
            action: HitAction::OpenAttachment(resource_id),
        });
        Ok(())
    }

    fn ink(&mut self, object: &PageObject, bounds: Rect, ink: &Ink, z_index: i32) -> Result<()> {
        self.push_node(
            object,
            bounds,
            z_index,
            ScenePrimitive::Ink {
                strokes: ink.strokes.clone(),
            },
            AccessibilitySemantics {
                role: AccessibilityRole::Drawing,
                label: ink
                    .recognized_text
                    .clone()
                    .unwrap_or_else(|| "Ink drawing".to_owned()),
                description: ink.recognized_text.clone(),
            },
        )?;
        Ok(())
    }

    fn placeholder(&mut self, object: &PageObject, bounds: Rect, z_index: i32) -> Result<()> {
        self.push_node(
            object,
            bounds,
            z_index,
            ScenePrimitive::Fill {
                color: PLACEHOLDER_COLOR,
                corner_radius: 3.0,
            },
            AccessibilitySemantics::decoration(),
        )?;
        self.push_node(
            object,
            bounds,
            z_index + 1,
            ScenePrimitive::Placeholder {
                label: "Unsupported OneNote content".to_owned(),
            },
            AccessibilitySemantics {
                role: AccessibilityRole::Unknown,
                label: "Unsupported OneNote content".to_owned(),
                description: Some(
                    "The source object is retained but cannot yet be rendered".to_owned(),
                ),
            },
        )?;
        self.diagnostics.push(SceneDiagnostic {
            code: "unsupported_content".to_owned(),
            message: "An unsupported source object is represented by a placeholder".to_owned(),
            object_id: Some(object.id.clone()),
        });
        Ok(())
    }

    fn push_node(
        &mut self,
        object: &PageObject,
        bounds: Rect,
        z_index: i32,
        primitive: ScenePrimitive,
        accessibility: AccessibilitySemantics,
    ) -> Result<SceneNodeId> {
        self.check_cancelled()?;
        if self.nodes.len() >= self.options.max_nodes {
            return Err(Error::NodeLimit {
                maximum: self.options.max_nodes,
            });
        }
        let id = SceneNodeId(format!("{}:{}", object.id, self.sequence));
        self.sequence = self.sequence.saturating_add(1);
        self.nodes.push(SceneNode {
            id: id.clone(),
            source_object_id: object.id.clone(),
            bounds: finite_rect(bounds),
            z_index,
            primitive,
            accessibility,
        });
        Ok(id)
    }

    fn check_cancelled(&self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy)]
struct Cursor {
    x: f32,
    y: f32,
    width: f32,
}

fn object_z(object: &PageObject, offset: i32) -> i32 {
    i32::try_from(object.z_index)
        .unwrap_or(i32::MAX)
        .saturating_mul(100)
        .saturating_add(offset)
}

fn outline_indent(outline: &Outline, level: u8) -> f32 {
    outline
        .indents
        .get(usize::from(level))
        .copied()
        .unwrap_or_else(|| f32::from(level) * 20.0)
        .max(0.0)
}

fn list_marker_text(marker: &ListMarker) -> String {
    if marker.format.is_empty() {
        "•".to_owned()
    } else {
        marker.format.clone()
    }
}

fn inline_bounds(
    cursor: &Cursor,
    width: Option<f32>,
    height: Option<f32>,
    default_width: f32,
    default_height: f32,
) -> Rect {
    Rect {
        x: cursor.x,
        y: cursor.y,
        width: width.unwrap_or(default_width).min(cursor.width).max(1.0),
        height: height.unwrap_or(default_height).max(1.0),
    }
}

fn table_widths(table: &Table, columns: usize, available: f32) -> Vec<f32> {
    let declared: Vec<_> = table
        .column_widths
        .iter()
        .copied()
        .take(columns)
        .map(|width| width.max(24.0))
        .collect();
    if declared.len() == columns && declared.iter().sum::<f32>() > 0.0 {
        return declared;
    }
    let column_count = f32::from(u16::try_from(columns).unwrap_or(u16::MAX));
    vec![(available / column_count).max(40.0); columns]
}

fn estimate_elements_height(elements: &[OutlineElement], width: f32, options: SceneOptions) -> f32 {
    elements
        .iter()
        .map(|element| {
            let own: f32 = element
                .content
                .iter()
                .map(|content| match content {
                    ElementContent::Text(text) => estimate_text_height(text, width, options),
                    ElementContent::Table(table) => {
                        f32::from(u16::try_from(table.rows.len()).unwrap_or(u16::MAX)) * 32.0
                    }
                    ElementContent::Image(image) => image.height.unwrap_or(180.0),
                    ElementContent::Attachment(attachment) => attachment.height.unwrap_or(44.0),
                    ElementContent::Ink(ink) => ink_height(ink),
                    ElementContent::Unknown => 44.0,
                })
                .sum();
            own + estimate_elements_height(&element.children, width - 20.0, options)
        })
        .sum::<f32>()
        .max(24.0)
        + options.content_padding * 2.0
}

fn estimate_text_height(text: &TextBlock, width: f32, options: SceneOptions) -> f32 {
    let font_size = text
        .runs
        .iter()
        .filter_map(|run| run.style.font_size)
        .chain(text.base_style.font_size)
        .fold(options.default_font_size, f32::max);
    let average_character_width = font_size * 0.55;
    let characters_per_line = (width / average_character_width).floor().max(1.0);
    let visible_characters = text.visible_text().chars().count();
    let visible_characters = f32::from(u16::try_from(visible_characters).unwrap_or(u16::MAX));
    let lines = (visible_characters / characters_per_line).ceil().max(1.0);
    text.line_spacing
        .unwrap_or(font_size * options.line_height)
        .max(font_size)
        * lines
}

fn ink_height(ink: &Ink) -> f32 {
    ink.strokes
        .iter()
        .map(|stroke| stroke.height)
        .fold(0.0_f32, f32::max)
}

fn bounded_label(text: &str, maximum: usize) -> String {
    let mut label: String = text.chars().take(maximum).collect();
    if text.chars().count() > maximum {
        label.push('…');
    }
    label
}

fn finite_rect(rect: Rect) -> Rect {
    Rect {
        x: finite(rect.x),
        y: finite(rect.y),
        width: finite(rect.width).max(0.0),
        height: finite(rect.height).max(0.0),
    }
}

fn finite(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

fn scene_bounds(page: &Page, nodes: &[SceneNode], options: SceneOptions) -> Rect {
    if options.crop_to_content {
        let content = nodes
            .iter()
            .map(|node| node.bounds)
            .reduce(Rect::union)
            .unwrap_or_default();
        return Rect {
            x: content.x,
            y: content.y,
            width: content.width.max(options.minimum_canvas_width),
            height: content.height.max(options.minimum_canvas_height),
        };
    }
    let initial = Rect {
        x: 0.0,
        y: 0.0,
        width: options.minimum_canvas_width,
        height: page
            .height
            .unwrap_or(options.minimum_canvas_height)
            .max(options.minimum_canvas_height),
    };
    nodes
        .iter()
        .map(|node| node.bounds)
        .fold(initial, Rect::union)
}

#[cfg(test)]
mod tests {
    use super::{SceneBuilder, SceneOptions};
    use crate::{Error, ScenePrimitive};
    use onenote_core::{ObjectId, ObjectKind, Page, PageId, PageObject, PageObjectRole, Rect};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn preserves_freeform_bounds_stacking_and_unknown_placeholders() {
        let page = Page {
            id: PageId::new("page"),
            native_id: "native".to_owned(),
            title: "Title".to_owned(),
            level: 0,
            created_at: String::new(),
            updated_at: String::new(),
            author: None,
            height: None,
            objects: vec![
                PageObject {
                    id: ObjectId::new("first"),
                    role: PageObjectRole::Body,
                    bounds: Rect {
                        x: -40.0,
                        y: 20.0,
                        width: 200.0,
                        height: 100.0,
                    },
                    z_index: 3,
                    kind: ObjectKind::Unknown,
                },
                PageObject {
                    id: ObjectId::new("second"),
                    role: PageObjectRole::Body,
                    bounds: Rect {
                        x: 20.0,
                        y: 40.0,
                        width: 200.0,
                        height: 100.0,
                    },
                    z_index: 5,
                    kind: ObjectKind::Unknown,
                },
            ],
        };
        let scene = SceneBuilder::default()
            .build(&page, &AtomicBool::new(false))
            .expect("scene");
        assert!((scene.bounds.x + 40.0).abs() < f32::EPSILON);
        assert_eq!(scene.diagnostics.len(), 2);
        assert!(scene
            .nodes
            .windows(2)
            .all(|nodes| nodes[0].z_index <= nodes[1].z_index));
        assert!(scene
            .nodes
            .iter()
            .any(|node| matches!(node.primitive, ScenePrimitive::Placeholder { .. })));
        assert_eq!(
            scene
                .visible_nodes(
                    Rect {
                        x: -50.0,
                        y: 0.0,
                        width: 20.0,
                        height: 200.0,
                    },
                    0.0,
                )
                .count(),
            2
        );
    }

    #[test]
    fn can_exclude_title_objects_and_crop_to_body() {
        let page = Page {
            id: PageId::new("page"),
            native_id: "native".to_owned(),
            title: "Title".to_owned(),
            level: 0,
            created_at: String::new(),
            updated_at: String::new(),
            author: None,
            height: Some(2_000.0),
            objects: vec![
                PageObject {
                    id: ObjectId::new("title"),
                    role: PageObjectRole::Title,
                    bounds: Rect {
                        x: 40.0,
                        y: 30.0,
                        width: 300.0,
                        height: 80.0,
                    },
                    z_index: 0,
                    kind: ObjectKind::Unknown,
                },
                PageObject {
                    id: ObjectId::new("body"),
                    role: PageObjectRole::Body,
                    bounds: Rect {
                        x: 120.0,
                        y: 260.0,
                        width: 400.0,
                        height: 300.0,
                    },
                    z_index: 1,
                    kind: ObjectKind::Unknown,
                },
            ],
        };
        let scene = SceneBuilder::with_options(SceneOptions {
            include_page_title: false,
            crop_to_content: true,
            ..SceneOptions::default()
        })
        .build(&page, &AtomicBool::new(false))
        .expect("scene");

        assert_eq!(scene.nodes.len(), 2);
        assert!(scene
            .nodes
            .iter()
            .all(|node| node.source_object_id == ObjectId::new("body")));
        assert!((scene.bounds.x - 120.0).abs() < f32::EPSILON);
        assert!((scene.bounds.y - 260.0).abs() < f32::EPSILON);
    }

    #[test]
    fn cancellation_prevents_construction() {
        let page = Page {
            id: PageId::new("page"),
            native_id: String::new(),
            title: String::new(),
            level: 0,
            created_at: String::new(),
            updated_at: String::new(),
            author: None,
            height: None,
            objects: Vec::new(),
        };
        assert_eq!(
            SceneBuilder::default()
                .build(&page, &AtomicBool::new(true))
                .expect_err("cancelled"),
            Error::Cancelled
        );
        let mut page = page;
        page.objects.push(PageObject {
            id: ObjectId::new("object"),
            role: PageObjectRole::Body,
            bounds: Rect::default(),
            z_index: 0,
            kind: ObjectKind::Unknown,
        });
        assert_eq!(
            SceneBuilder::default()
                .build(&page, &AtomicBool::new(true))
                .expect_err("cancelled"),
            Error::Cancelled
        );
    }
}
