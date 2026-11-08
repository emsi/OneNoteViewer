use crate::image_cache::{self, DecodedImage, ImageDecodeFailure, MAX_TEXTURE_CACHE_BYTES};
use crate::math_cache::{self, MathKey, MathSize, TypstMathBackend};
use crate::resolved_layout::ResolvedLayout;
use crate::text;
use crate::{
    EditableImage, EditableStroke, EditableTextBox, ErasedStroke, InkEditDocument, InkEditTool,
};
use crate::{FindMatch, FindOptions};
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::graphene;
use gtk::pango;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use num_traits::ToPrimitive;
use onenote_core::{Color, MathSpan, Rect, ResourceId, ResourceStatus, ResourceStore, TextStyle};
use onenote_render::{
    HitAction, MathLayoutBackend, PageScene, SceneNode, SceneNodeId, ScenePrimitive,
};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::OnceLock;

const CANVAS_MARGIN: f32 = 32.0;
const VIEWPORT_OVERSCAN: f32 = 256.0;
/// Default page zoom scale (100%).
pub const DEFAULT_ZOOM: f32 = 1.0;
/// Smallest supported page zoom scale (25%).
pub const MIN_ZOOM: f32 = 0.25;
/// Largest supported page zoom scale (400%).
pub const MAX_ZOOM: f32 = 4.0;
type ActionHandler = Rc<dyn Fn(HitAction)>;
type InkEditHandler = Rc<dyn Fn(InkEditDocument)>;
type TextSelectionHandler = Rc<dyn Fn(String, String)>;

#[derive(Default)]
struct FindHighlights {
    matches: Vec<FindMatch>,
    active: Option<usize>,
    highlight_all: bool,
}

mod imp {
    use super::{
        gdk, glib, ActionHandler, Arc, Cell, Color, EditableStroke, FindHighlights, HashMap, HashSet,
        ImageDecodeFailure, InkEditDocument, InkEditHandler, InkEditTool, MathKey,
        MathLayoutBackend, OnceLock, PageScene, Rc, RefCell, ResolvedLayout, ResourceId,
        ResourceStore, SceneNodeId, TextSelectionHandler, TypstMathBackend, CANVAS_MARGIN,
        DEFAULT_ZOOM, MAX_ZOOM, MIN_ZOOM,
    };
    use gtk::prelude::*;
    use gtk::subclass::prelude::ObjectSubclassIsExt;
    use gtk::subclass::prelude::*;

    pub struct PageCanvas {
        pub(super) scene: RefCell<Option<Arc<PageScene>>>,
        pub(super) resources: RefCell<Option<Arc<ResourceStore>>>,
        pub(super) zoom: Cell<f32>,
        pub(super) default_text_color: RefCell<gdk::RGBA>,
        pub(super) action_handler: RefCell<Option<ActionHandler>>,
        pub(super) hovered_attachment: RefCell<Option<ResourceId>>,
        pub(super) focused_attachment: Cell<Option<usize>>,
        pub(super) text_layouts: RefCell<HashMap<SceneNodeId, Rc<crate::text::TextLayout>>>,
        pub(super) resolved_layout: RefCell<Option<CachedResolvedLayout>>,
        pub(super) layout_generation: Cell<u64>,
        pub(super) textures: RefCell<HashMap<ResourceId, CachedTexture>>,
        pub(super) pending: RefCell<HashSet<ResourceId>>,
        pub(super) failed: RefCell<HashMap<ResourceId, ImageDecodeFailure>>,
        pub(super) texture_bytes: Cell<usize>,
        pub(super) texture_generation: Cell<u64>,
        pub(super) math_textures: RefCell<HashMap<MathKey, CachedMathTexture>>,
        pub(super) math_pending: RefCell<HashSet<MathKey>>,
        pub(super) math_errors: RefCell<HashMap<MathKey, String>>,
        pub(super) math_texture_bytes: Cell<usize>,
        pub(super) math_backend: RefCell<Arc<dyn MathLayoutBackend>>,
        pub(super) math_generation: Cell<u64>,
        pub(super) find_highlights: RefCell<FindHighlights>,
        pub(super) ink_tool: Cell<InkEditTool>,
        pub(super) ink_edits: RefCell<InkEditDocument>,
        pub(super) pending_ink: RefCell<Option<EditableStroke>>,
        pub(super) ink_undo: RefCell<Vec<InkEditDocument>>,
        pub(super) ink_edit_handler: RefCell<Option<InkEditHandler>>,
        pub(super) selected_text_node: RefCell<Option<SceneNodeId>>,
        pub(super) cursor_char_idx: Cell<usize>,
        pub(super) cursor_blink_visible: Cell<bool>,
        pub(super) text_selection_handler: RefCell<Option<TextSelectionHandler>>,
        pub(super) drag_start: Cell<(f64, f64)>,
        pub(super) ink_drag_dirty: Cell<bool>,
        pub(super) pen_color: Cell<Color>,
        pub(super) pen_width: Cell<f32>,
        #[cfg(test)]
        pub(super) viewport_redraw_requests: Cell<u64>,
    }

    impl Default for PageCanvas {
        fn default() -> Self {
            Self {
                scene: RefCell::default(),
                resources: RefCell::default(),
                zoom: Cell::new(DEFAULT_ZOOM),
                default_text_color: RefCell::new(gdk::RGBA::BLACK),
                action_handler: RefCell::default(),
                hovered_attachment: RefCell::default(),
                focused_attachment: Cell::default(),
                text_layouts: RefCell::default(),
                resolved_layout: RefCell::default(),
                layout_generation: Cell::default(),
                textures: RefCell::default(),
                pending: RefCell::default(),
                failed: RefCell::default(),
                texture_bytes: Cell::default(),
                texture_generation: Cell::default(),
                math_textures: RefCell::default(),
                math_pending: RefCell::default(),
                math_errors: RefCell::default(),
                math_texture_bytes: Cell::default(),
                math_backend: RefCell::new(Arc::new(TypstMathBackend::new())),
                math_generation: Cell::default(),
                find_highlights: RefCell::default(),
                ink_tool: Cell::default(),
                ink_edits: RefCell::default(),
                pending_ink: RefCell::default(),
                ink_undo: RefCell::default(),
                ink_edit_handler: RefCell::default(),
                selected_text_node: RefCell::default(),
                cursor_char_idx: Cell::new(0),
                cursor_blink_visible: Cell::new(true),
                text_selection_handler: RefCell::default(),
                drag_start: Cell::new((0.0, 0.0)),
                ink_drag_dirty: Cell::default(),
                pen_color: Cell::new(Color {
                    red: 32,
                    green: 31,
                    blue: 30,
                    alpha: 255,
                }),
                pen_width: Cell::new(2.0),
                #[cfg(test)]
                viewport_redraw_requests: Cell::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PageCanvas {
        const NAME: &'static str = "OneNotePageCanvas";
        type Type = super::PageCanvas;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for PageCanvas {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPERTIES: OnceLock<Vec<glib::ParamSpec>> = OnceLock::new();
            PROPERTIES.get_or_init(|| {
                vec![glib::ParamSpecFloat::builder("zoom")
                    .minimum(MIN_ZOOM)
                    .maximum(MAX_ZOOM)
                    .default_value(DEFAULT_ZOOM)
                    .read_only()
                    .explicit_notify()
                    .build()]
            })
        }

        fn property(&self, _id: usize, spec: &glib::ParamSpec) -> glib::Value {
            match spec.name() {
                "zoom" => self.zoom.get().to_value(),
                _ => unimplemented!(),
            }
        }

        fn constructed(&self) {
            self.parent_constructed();
            let object = self.obj();
            object.set_focusable(true);
            object.set_overflow(gtk::Overflow::Hidden);
            let click = gtk::GestureClick::new();
            click.set_button(gdk::BUTTON_PRIMARY);
            let weak = object.downgrade();
            click.connect_released(move |_, _n_press, x, y| {
                if let Some(object) = weak.upgrade() {
                    object.activate_at(x, y);
                }
            });
            object.add_controller(click);
            let drag = gtk::GestureDrag::new();
            drag.set_button(gdk::BUTTON_PRIMARY);
            let weak = object.downgrade();
            drag.connect_drag_begin(move |_, x, y| {
                if let Some(object) = weak.upgrade() {
                    object.imp().drag_start.set((x, y));
                    object.begin_ink_edit(x, y);
                }
            });
            let weak = object.downgrade();
            drag.connect_drag_update(move |_, offset_x, offset_y| {
                if let Some(object) = weak.upgrade() {
                    let (start_x, start_y) = object.imp().drag_start.get();
                    object.update_ink_edit(start_x + offset_x, start_y + offset_y);
                }
            });
            let weak = object.downgrade();
            drag.connect_drag_end(move |_, offset_x, offset_y| {
                if let Some(object) = weak.upgrade() {
                    let (start_x, start_y) = object.imp().drag_start.get();
                    object.finish_ink_edit(start_x + offset_x, start_y + offset_y);
                }
            });
            object.add_controller(drag);
            let motion = gtk::EventControllerMotion::new();
            let weak = object.downgrade();
            motion.connect_motion(move |_, x, y| {
                if let Some(object) = weak.upgrade() {
                    object.update_pointer_at(x, y);
                }
            });
            let weak = object.downgrade();
            motion.connect_leave(move |_| {
                if let Some(object) = weak.upgrade() {
                    object.set_cursor_from_name(None);
                    object.set_tooltip_text(None);
                    object.set_hovered_attachment(None);
                }
            });
            object.add_controller(motion);
            let keys = gtk::EventControllerKey::new();
            let weak = object.downgrade();
            keys.connect_key_pressed(move |_, key, _, state| {
                let Some(object) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                if matches!(key, gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::space)
                    && object.imp().selected_text_node.borrow().is_none()
                {
                    if object.activate_focused_attachment() {
                        return glib::Propagation::Stop;
                    }
                } else if key == gdk::Key::Escape && object.imp().selected_text_node.borrow().is_none() {
                    object.imp().focused_attachment.set(None);
                    object.queue_draw();
                }
                object.handle_key_pressed(key, state)
            });
            object.add_controller(keys);

            let weak = object.downgrade();
            glib::timeout_add_local(std::time::Duration::from_millis(530), move || {
                let Some(object) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if object.imp().selected_text_node.borrow().is_some() && object.has_focus() {
                    let cur = object.imp().cursor_blink_visible.get();
                    object.imp().cursor_blink_visible.set(!cur);
                    object.queue_draw();
                }
                glib::ControlFlow::Continue
            });
        }
    }

    impl WidgetImpl for PageCanvas {
        fn focus(&self, direction: gtk::DirectionType) -> bool {
            if matches!(
                direction,
                gtk::DirectionType::TabForward | gtk::DirectionType::TabBackward
            ) && self.obj().move_attachment_focus(direction)
            {
                return true;
            }
            self.parent_focus(direction)
        }

        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let Some(scene) = self.scene.borrow().clone() else {
                return (1, 1, -1, -1);
            };
            let resolved = self.obj().resolved_layout(&scene);
            let logical = match orientation {
                gtk::Orientation::Horizontal => resolved.bounds.width + CANVAS_MARGIN * 2.0,
                gtk::Orientation::Vertical => resolved.bounds.height + CANVAS_MARGIN * 2.0,
                _ => 1.0,
            };
            let size = logical * self.zoom.get();
            let size = super::finite_f32_to_i32(size.ceil()).max(1);
            (size, size, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let object = self.obj();
            object.snapshot_scene(snapshot);
        }
    }

    pub(super) struct CachedTexture {
        pub(super) texture: gdk::Texture,
        pub(super) bytes: usize,
    }

    pub(super) struct CachedMathTexture {
        pub(super) texture: gdk::Texture,
        pub(super) size: super::MathSize,
        pub(super) bytes: usize,
    }

    pub(super) struct CachedResolvedLayout {
        pub(super) generation: u64,
        pub(super) layout: Rc<ResolvedLayout>,
    }
}

glib::wrapper! {
    /// Embeddable snapshot-based page canvas.
    pub struct PageCanvas(ObjectSubclass<imp::PageCanvas>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PageCanvas {
    /// Construct an empty canvas. GTK must already be initialized.
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    /// Replace the current immutable scene and clear per-page texture state.
    pub fn set_scene(&self, scene: Option<Arc<PageScene>>) {
        let imp = self.imp();
        *imp.scene.borrow_mut() = scene;
        *imp.ink_edits.borrow_mut() = InkEditDocument::default();
        *imp.selected_text_node.borrow_mut() = None;
        imp.cursor_char_idx.set(0);
        imp.cursor_blink_visible.set(true);
        imp.pending_ink.borrow_mut().take();
        imp.ink_undo.borrow_mut().clear();
        imp.hovered_attachment.borrow_mut().take();
        imp.focused_attachment.set(None);
        *imp.find_highlights.borrow_mut() = FindHighlights::default();
        self.invalidate_text_geometry();
        imp.textures.borrow_mut().clear();
        imp.pending.borrow_mut().clear();
        imp.failed.borrow_mut().clear();
        imp.texture_bytes.set(0);
        imp.texture_generation
            .set(imp.texture_generation.get().wrapping_add(1));
        imp.math_textures.borrow_mut().clear();
        imp.math_pending.borrow_mut().clear();
        imp.math_errors.borrow_mut().clear();
        imp.math_texture_bytes.set(0);
        imp.math_generation
            .set(imp.math_generation.get().wrapping_add(1));
        self.queue_resize();
        self.queue_draw();
    }

    /// Select the pointer tool for local page ink editing.
    pub fn set_ink_tool(&self, tool: InkEditTool) {
        self.imp().ink_tool.set(tool);
        self.set_cursor_from_name(match tool {
            InkEditTool::View => None,
            InkEditTool::Pen => Some("crosshair"),
            InkEditTool::Eraser => Some("not-allowed"),
        });
    }

    /// Set the pen color for drawing ink.
    pub fn set_pen_color(&self, color: Color) {
        self.imp().pen_color.set(color);
    }

    /// Current pen color for drawing ink.
    pub fn pen_color(&self) -> Color {
        self.imp().pen_color.get()
    }

    /// Set the pen width in logical points.
    pub fn set_pen_width(&self, width: f32) {
        self.imp().pen_width.set(width.max(0.5));
    }

    /// Current pen width in logical points.
    pub fn pen_width(&self) -> f32 {
        self.imp().pen_width.get()
    }

    /// Replace local edits layered over the current page.
    pub fn set_ink_edits(&self, edits: InkEditDocument) {
        *self.imp().ink_edits.borrow_mut() = edits;
        self.imp().pending_ink.borrow_mut().take();
        self.imp().ink_undo.borrow_mut().clear();
        self.invalidate_text_geometry();
        self.queue_resize();
        self.queue_draw();
    }

    /// Return the text currently associated with a given node.
    pub fn text_for_node(&self, node_id: &SceneNodeId) -> Option<String> {
        let edits = self.imp().ink_edits.borrow();
        if let Some(tb) = edits.added_text_boxes.iter().find(|tb| tb.id == node_id.0) {
            return Some(tb.text.clone());
        }
        let scene = self.scene()?;
        let node = scene.nodes.iter().find(|node| &node.id == node_id)?;
        let ScenePrimitive::Text { block, .. } = &node.primitive else {
            return None;
        };
        let text = edits
            .text_replacements
            .get(&node.id.0)
            .cloned()
            .unwrap_or_else(|| block.visible_text());
        Some(text)
    }

    /// Add a new text box at page logical coordinates (x, y).
    pub fn add_text_box_at(&self, x: f32, y: f32, initial_text: &str) -> String {
        let timestamp = glib::DateTime::now_local().map_or(0, |dt| dt.to_unix() * 1_000_000 + dt.microsecond() as i64);
        let id = format!("textbox_{timestamp}");
        let new_box = EditableTextBox {
            id: id.clone(),
            x,
            y,
            width: 340.0,
            height: 48.0,
            text: initial_text.to_string(),
        };
        self.imp().ink_edits.borrow_mut().added_text_boxes.push(new_box);
        *self.imp().selected_text_node.borrow_mut() = Some(SceneNodeId(id.clone()));
        self.imp().cursor_char_idx.set(initial_text.len());
        self.imp().cursor_blink_visible.set(true);
        self.invalidate_text_geometry();
        self.queue_resize();
        self.queue_draw();
        self.emit_ink_edits_changed();
        id
    }

    /// Add an image at page logical coordinates (x, y).
    pub fn add_image_at(&self, x: f32, y: f32, file_path: &str) -> Option<String> {
        let file = gio::File::for_path(file_path);
        let texture = gdk::Texture::from_file(&file).ok()?;
        let orig_w = texture.width() as f32;
        let orig_h = texture.height() as f32;
        let scale = (450.0 / orig_w).min(1.0);
        let width = orig_w * scale;
        let height = orig_h * scale;

        let timestamp = glib::DateTime::now_local().map_or(0, |dt| dt.to_unix());
        let id = format!("image_{timestamp}");
        let new_img = EditableImage {
            id: id.clone(),
            x,
            y,
            width,
            height,
            file_path: file_path.to_string(),
        };
        self.imp().ink_edits.borrow_mut().added_images.push(new_img);
        *self.imp().selected_text_node.borrow_mut() = Some(SceneNodeId(id.clone()));
        self.queue_draw();
        self.emit_ink_edits_changed();
        Some(id)
    }

    /// Set callback when a text node is selected on the page.
    pub fn set_text_selection_handler(&self, handler: Option<impl Fn(String, String) + 'static>) {
        *self.imp().text_selection_handler.borrow_mut() =
            handler.map(|h| Rc::new(h) as TextSelectionHandler);
    }

    /// Trigger edit on the currently selected text node.
    pub fn trigger_edit_on_selected(&self) {
        if let Some((node_id, text)) = self.selected_text() {
            if let Some(handler) = self.imp().text_selection_handler.borrow().as_ref() {
                handler(node_id, text);
            }
        }
    }

    /// Select a text node and place cursor at a specific character offset.
    pub fn select_node_by_id(&self, node_id: &str, cursor: usize) {
        *self.imp().selected_text_node.borrow_mut() = Some(SceneNodeId(node_id.to_string()));
        self.imp().cursor_char_idx.set(cursor);
        self.imp().cursor_blink_visible.set(true);
        self.queue_draw();
    }

    /// Toggle task checkbox on the currently selected text node if applicable.
    pub fn toggle_selected_task(&self) -> bool {
        let Some((node_id, text)) = self.selected_text() else {
            return false;
        };
        if text.starts_with("[ ] ") {
            self.set_text_replacement(&node_id, text.replacen("[ ] ", "[x] ", 1))
        } else if text.starts_with("[x] ") {
            self.set_text_replacement(&node_id, text.replacen("[x] ", "[ ] ", 1))
        } else if text.starts_with("[X] ") {
            self.set_text_replacement(&node_id, text.replacen("[X] ", "[ ] ", 1))
        } else if text.starts_with("☐ ") {
            self.set_text_replacement(&node_id, text.replacen("☐ ", "☑ ", 1))
        } else if text.starts_with("☑ ") {
            self.set_text_replacement(&node_id, text.replacen("☑ ", "☐ ", 1))
        } else {
            self.set_text_replacement(&node_id, format!("☐ {text}"))
        }
    }

    /// Return the selected text node and its currently displayed text.
    pub fn selected_text(&self) -> Option<(String, String)> {
        let selected = self.imp().selected_text_node.borrow().clone()?;
        let text = self.text_for_node(&selected)?;
        Some((selected.0, text))
    }

    /// Apply a local replacement to one text node without changing its source file.
    pub fn set_text_replacement(&self, node_id: &str, replacement: String) -> bool {
        {
            let mut edits = self.imp().ink_edits.borrow_mut();
            if let Some(tb) = edits.added_text_boxes.iter_mut().find(|tb| tb.id == node_id) {
                tb.text = replacement;
                drop(edits);
                self.invalidate_text_geometry();
                self.queue_resize();
                self.queue_draw();
                self.emit_ink_edits_changed();
                return true;
            }
        }
        let Some(scene) = self.scene() else {
            return false;
        };
        let Some(node) = scene.nodes.iter().find(|node| node.id.0 == node_id) else {
            return false;
        };
        let ScenePrimitive::Text { block, .. } = &node.primitive else {
            return false;
        };
        let original = block.visible_text();
        let mut edits = self.imp().ink_edits.borrow_mut();
        if replacement == original {
            edits.text_replacements.remove(node_id);
        } else {
            edits
                .text_replacements
                .insert(node_id.to_owned(), replacement);
        }
        drop(edits);
        self.invalidate_text_geometry();
        self.queue_resize();
        self.queue_draw();
        self.emit_ink_edits_changed();
        true
    }

    /// Return the local edit layer for persistence by the host application.
    pub fn ink_edits(&self) -> InkEditDocument {
        self.imp().ink_edits.borrow().clone()
    }

    /// Notify the host after a completed local ink edit.
    pub fn set_ink_edit_handler(&self, handler: Option<impl Fn(InkEditDocument) + 'static>) {
        *self.imp().ink_edit_handler.borrow_mut() =
            handler.map(|handler| Rc::new(handler) as InkEditHandler);
    }

    /// Undo the most recent local ink edit.
    pub fn undo_ink_edit(&self) -> bool {
        let Some(previous) = self.imp().ink_undo.borrow_mut().pop() else {
            return false;
        };
        *self.imp().ink_edits.borrow_mut() = previous;
        self.emit_ink_edits_changed();
        self.queue_draw();
        true
    }

    /// Whether a local ink edit can be undone.
    pub fn can_undo_ink_edit(&self) -> bool {
        !self.imp().ink_undo.borrow().is_empty()
    }

    /// Current scene.
    pub fn scene(&self) -> Option<Arc<PageScene>> {
        self.imp().scene.borrow().clone()
    }

    /// Find literal occurrences in displayed text and searchable page metadata.
    pub fn find(&self, query: &str, options: FindOptions, limit: usize) -> Vec<FindMatch> {
        let Some(scene) = self.scene() else {
            return Vec::new();
        };
        let resolved = self.resolved_layout(&scene);
        let mut matches = Vec::new();
        for node in &scene.nodes {
            if matches.len() >= limit {
                break;
            }
            let node_bounds = resolved.node_bounds(node);
            matches.extend(self.find_in_node(
                node,
                node_bounds,
                query,
                options,
                limit - matches.len(),
            ));
        }
        matches
    }

    fn find_in_node(
        &self,
        node: &SceneNode,
        node_bounds: Rect,
        query: &str,
        options: FindOptions,
        limit: usize,
    ) -> Vec<FindMatch> {
        let mut matches = Vec::new();
        match &node.primitive {
            ScenePrimitive::Text { block, marker } => {
                let layout = self.text_layout(node, block, marker.as_deref());
                for range in
                    crate::find_text_ranges(layout.layout.text().as_str(), query, options, limit)
                {
                    let bounds = layout
                        .match_rectangles(range)
                        .into_iter()
                        .map(|bounds| translate_rect(bounds, node_bounds.x, node_bounds.y))
                        .collect();
                    if let Some(found) = FindMatch::new(bounds) {
                        matches.push(found);
                    }
                }
                for math in &layout.math {
                    if matches.len() >= limit {
                        break;
                    }
                    if !crate::find_text_ranges(&math.fallback, query, options, 1).is_empty() {
                        matches.push(FindMatch {
                            bounds: vec![Rect {
                                x: node_bounds.x + math.x,
                                y: node_bounds.y + math.y,
                                width: math.width,
                                height: math.height,
                            }],
                        });
                    }
                }
                for link in &block.links {
                    if matches.len() >= limit {
                        break;
                    }
                    if !crate::find_text_ranges(&link.target, query, options, 1).is_empty() {
                        matches.push(FindMatch {
                            bounds: vec![node_bounds],
                        });
                    }
                }
            }
            ScenePrimitive::Image(image) => {
                append_metadata_match(
                    &mut matches,
                    limit,
                    node_bounds,
                    [
                        image.alt_text.as_deref(),
                        image.search_text.as_deref(),
                        image.hyperlink.as_deref(),
                        Some(image.resource.name.as_str()),
                    ],
                    query,
                    options,
                );
            }
            ScenePrimitive::Attachment(attachment) => {
                append_metadata_match(
                    &mut matches,
                    limit,
                    node_bounds,
                    [
                        Some(attachment.resource.name.as_str()),
                        Some(attachment.resource.media_type.as_str()),
                        None,
                        None,
                    ],
                    query,
                    options,
                );
            }
            ScenePrimitive::Ink { .. } | ScenePrimitive::Placeholder { .. } => {
                append_metadata_match(
                    &mut matches,
                    limit,
                    node_bounds,
                    [
                        Some(node.accessibility.label.as_str()),
                        node.accessibility.description.as_deref(),
                        None,
                        None,
                    ],
                    query,
                    options,
                );
            }
            ScenePrimitive::Fill { .. } | ScenePrimitive::Line { .. } => {}
        }
        matches
    }

    /// Replace the page highlights without changing the scene or viewport.
    pub fn set_find_highlights(
        &self,
        matches: Vec<FindMatch>,
        active: Option<usize>,
        highlight_all: bool,
    ) {
        *self.imp().find_highlights.borrow_mut() = FindHighlights {
            matches,
            active,
            highlight_all,
        };
        self.queue_draw();
    }

    /// Supply the lazy resources belonging to the scene's loaded notebook.
    pub fn set_resources(&self, resources: Option<Arc<ResourceStore>>) {
        *self.imp().resources.borrow_mut() = resources;
        self.imp().textures.borrow_mut().clear();
        self.imp().pending.borrow_mut().clear();
        self.imp().failed.borrow_mut().clear();
        self.imp().texture_bytes.set(0);
        self.imp()
            .texture_generation
            .set(self.imp().texture_generation.get().wrapping_add(1));
        self.queue_draw();
    }

    /// Set canvas zoom, clamped to 25%-400%.
    pub fn set_zoom(&self, zoom: f32) {
        let zoom = normalize_zoom(zoom);
        if (self.imp().zoom.get() - zoom).abs() > f32::EPSILON {
            self.imp().zoom.set(zoom);
            self.invalidate_text_geometry();
            self.imp().math_textures.borrow_mut().clear();
            self.imp().math_pending.borrow_mut().clear();
            self.imp().math_errors.borrow_mut().clear();
            self.imp().math_texture_bytes.set(0);
            self.imp()
                .math_generation
                .set(self.imp().math_generation.get().wrapping_add(1));
            self.queue_resize();
            self.queue_draw();
            self.notify("zoom");
        }
    }

    /// Current zoom scale.
    pub fn zoom(&self) -> f32 {
        self.imp().zoom.get()
    }

    /// Set the fallback for text whose `OneNote` style uses automatic color.
    ///
    /// Explicit foreground colors stored in the page continue to override this
    /// host-provided value.
    pub fn set_default_text_color(&self, color: &gdk::RGBA) {
        *self.imp().default_text_color.borrow_mut() = *color;
        self.invalidate_text_geometry();
        self.imp().math_textures.borrow_mut().clear();
        self.imp().math_pending.borrow_mut().clear();
        self.imp().math_errors.borrow_mut().clear();
        self.imp().math_texture_bytes.set(0);
        self.imp()
            .math_generation
            .set(self.imp().math_generation.get().wrapping_add(1));
        self.queue_resize();
        self.queue_draw();
    }

    /// Current fallback for text whose `OneNote` style uses automatic color.
    pub fn default_text_color(&self) -> gdk::RGBA {
        *self.imp().default_text_color.borrow()
    }

    pub(crate) fn queue_viewport_redraw(&self) {
        #[cfg(test)]
        self.imp()
            .viewport_redraw_requests
            .set(self.imp().viewport_redraw_requests.get().wrapping_add(1));
        self.queue_draw();
    }

    #[cfg(test)]
    pub(crate) fn viewport_redraw_requests(&self) -> u64 {
        self.imp().viewport_redraw_requests.get()
    }

    /// Replace the native math backend used for future equations.
    ///
    /// Existing equation textures are discarded so the new backend takes
    /// effect on the next snapshot. Rendering still occurs off the GTK thread.
    pub fn set_math_backend(&self, backend: Arc<dyn MathLayoutBackend>) {
        let imp = self.imp();
        *imp.math_backend.borrow_mut() = backend;
        self.invalidate_text_geometry();
        imp.math_textures.borrow_mut().clear();
        imp.math_pending.borrow_mut().clear();
        imp.math_errors.borrow_mut().clear();
        imp.math_texture_bytes.set(0);
        imp.math_generation
            .set(imp.math_generation.get().wrapping_add(1));
        self.queue_resize();
        self.queue_draw();
    }

    /// Set the host callback for link, attachment, and selection actions.
    pub fn set_action_handler(&self, handler: Option<impl Fn(HitAction) + 'static>) {
        *self.imp().action_handler.borrow_mut() =
            handler.map(|handler| Rc::new(handler) as ActionHandler);
    }

    fn activate_at(&self, x: f64, y: f64) {
        if self.imp().ink_tool.get() != InkEditTool::View {
            return;
        }
        self.grab_focus();
        self.select_or_create_text_at(x, y);
        let action = self.action_at(x, y);
        if let Some(HitAction::OpenAttachment(resource_id)) = &action {
            self.focus_attachment(resource_id);
            self.grab_focus();
        }
        if let (Some(action), Some(handler)) = (action, self.imp().action_handler.borrow().as_ref())
        {
            handler(action);
        }
    }

    fn select_or_create_text_at(&self, x: f64, y: f64) {
        let Some(scene) = self.scene() else {
            return;
        };
        let resolved = self.resolved_layout(&scene);
        let logical_x =
            f64_to_f32(x / f64::from(self.zoom())) + resolved.bounds.x - CANVAS_MARGIN;
        let logical_y =
            f64_to_f32(y / f64::from(self.zoom())) + resolved.bounds.y - CANVAS_MARGIN;

        // Prune previous empty text box if the user clicked away without typing anything
        {
            let mut edits = self.imp().ink_edits.borrow_mut();
            if let Some(prev) = self.imp().selected_text_node.borrow().as_ref() {
                edits.added_text_boxes.retain(|tb| tb.id != prev.0 || !tb.text.trim().is_empty());
            }
        }

        // Check scene text nodes
        let mut hit: Option<(SceneNodeId, f32, f32)> = scene.nodes.iter().rev().find_map(|node| {
            let ScenePrimitive::Text { .. } = &node.primitive else {
                return None;
            };
            let bounds = resolved.node_bounds(node);
            if contains(bounds, logical_x, logical_y) {
                Some((node.id.clone(), logical_x - bounds.x, logical_y - bounds.y))
            } else {
                None
            }
        });

        // Check added text boxes
        if hit.is_none() {
            let edits = self.imp().ink_edits.borrow();
            for tb in edits.added_text_boxes.iter().rev() {
                let bounds = Rect { x: tb.x, y: tb.y, width: tb.width, height: tb.height };
                if contains(bounds, logical_x, logical_y) {
                    hit = Some((SceneNodeId(tb.id.clone()), logical_x - bounds.x, logical_y - bounds.y));
                    break;
                }
            }
        }

        // Check added images
        if hit.is_none() {
            let edits = self.imp().ink_edits.borrow();
            for img in edits.added_images.iter().rev() {
                let bounds = Rect { x: img.x, y: img.y, width: img.width, height: img.height };
                if contains(bounds, logical_x, logical_y) {
                    hit = Some((SceneNodeId(img.id.clone()), logical_x - bounds.x, logical_y - bounds.y));
                    break;
                }
            }
        }

        if let Some((node_id, rel_x, rel_y)) = hit {
            // Checkbox toggle if clicked near left edge on to-do item
            if let Some(text) = self.text_for_node(&node_id) {
                if rel_x < 28.0 && (text.starts_with("[ ] ") || text.starts_with("[x] ") || text.starts_with("[X] ") || text.starts_with("☐ ") || text.starts_with("☑ ")) {
                    let toggled = if text.starts_with("[ ] ") {
                        text.replacen("[ ] ", "[x] ", 1)
                    } else if text.starts_with("[x] ") {
                        text.replacen("[x] ", "[ ] ", 1)
                    } else if text.starts_with("[X] ") {
                        text.replacen("[X] ", "[ ] ", 1)
                    } else if text.starts_with("☐ ") {
                        text.replacen("☐ ", "☑ ", 1)
                    } else {
                        text.replacen("☑ ", "☐ ", 1)
                    };
                    self.set_text_replacement(&node_id.0, toggled);
                }
            }

            *self.imp().selected_text_node.borrow_mut() = Some(node_id.clone());

            // Compute cursor character index using layout
            if let Some(text) = self.text_for_node(&node_id) {
                let layout = self.get_layout_for_node(&node_id, &text);
                let (_, byte_idx, trailing) = layout.xy_to_index(text::to_pango_units(rel_x), text::to_pango_units(rel_y));
                let mut cursor = if byte_idx >= 0 {
                    let b = byte_idx as usize;
                    if trailing > 0 && b < text.len() {
                        text[b..].char_indices().nth(1).map(|(i, _)| b + i).unwrap_or(text.len())
                    } else {
                        b.min(text.len())
                    }
                } else {
                    text.len()
                };
                while !text.is_char_boundary(cursor) && cursor > 0 {
                    cursor -= 1;
                }
                self.imp().cursor_char_idx.set(cursor);
            } else {
                self.imp().cursor_char_idx.set(0);
            }
            self.imp().cursor_blink_visible.set(true);

            if let Some(handler) = self.imp().text_selection_handler.borrow().as_ref() {
                if let Some(text) = self.text_for_node(&node_id) {
                    handler(node_id.0.clone(), text);
                }
            }
        } else {
            // Clicked on empty canvas!
            // In OneNote, clicking anywhere on blank paper creates a floating note container and places cursor there!
            let new_id = self.add_text_box_at(logical_x, logical_y, "");
            self.imp().cursor_char_idx.set(0);
            self.imp().cursor_blink_visible.set(true);
            if let Some(handler) = self.imp().text_selection_handler.borrow().as_ref() {
                handler(new_id, String::new());
            }
        }

        self.queue_draw();
    }

    fn get_layout_for_node(&self, node_id: &SceneNodeId, text: &str) -> pango::Layout {
        if let Some(scene) = self.scene() {
            if let Some(node) = scene.nodes.iter().find(|n| &n.id == node_id) {
                if let ScenePrimitive::Text { block, marker } = &node.primitive {
                    return self.text_layout(node, block, marker.as_deref()).layout.clone();
                }
            }
        }
        let default_style = onenote_core::TextStyle::default();
        let (clean_text, base_style, runs) = Self::parse_formatted_text(&default_style, text);
        let block = onenote_core::TextBlock {
            text: clean_text,
            base_style,
            runs,
            math: Vec::new(),
            links: Vec::new(),
            alignment: onenote_core::TextAlignment::Left,
            space_before: 0.0,
            space_after: 0.0,
            line_spacing: None,
        };
        text::layout_with_math(&self.pango_context(), &block, None, 340.0, |_, _| None).layout
    }

    fn handle_key_pressed(&self, key: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
        if self.imp().ink_tool.get() != InkEditTool::View {
            return glib::Propagation::Proceed;
        }
        let Some(node_id) = self.imp().selected_text_node.borrow().clone() else {
            return glib::Propagation::Proceed;
        };

        let Some(mut text) = self.text_for_node(&node_id) else {
            return glib::Propagation::Proceed;
        };

        let mut cursor = self.imp().cursor_char_idx.get().min(text.len());
        while !text.is_char_boundary(cursor) && cursor > 0 {
            cursor -= 1;
        }

        match key {
            gdk::Key::Escape => {
                let is_empty = text.trim().is_empty();
                if is_empty {
                    self.imp().ink_edits.borrow_mut().added_text_boxes.retain(|tb| tb.id != node_id.0);
                    self.emit_ink_edits_changed();
                }
                *self.imp().selected_text_node.borrow_mut() = None;
                self.queue_draw();
                return glib::Propagation::Stop;
            }
            gdk::Key::BackSpace => {
                if cursor > 0 {
                    let prev_idx = text[..cursor].char_indices().last().map(|(i, _)| i).unwrap_or(0);
                    text.drain(prev_idx..cursor);
                    cursor = prev_idx;
                    self.update_node_text(&node_id, text, cursor);
                }
                return glib::Propagation::Stop;
            }
            gdk::Key::Delete => {
                if cursor < text.len() {
                    let next_idx = text[cursor..].char_indices().nth(1).map(|(i, _)| cursor + i).unwrap_or(text.len());
                    text.drain(cursor..next_idx);
                    self.update_node_text(&node_id, text, cursor);
                }
                return glib::Propagation::Stop;
            }
            gdk::Key::Left => {
                if cursor > 0 {
                    cursor = text[..cursor].char_indices().last().map(|(i, _)| i).unwrap_or(0);
                    self.imp().cursor_char_idx.set(cursor);
                    self.imp().cursor_blink_visible.set(true);
                    self.queue_draw();
                }
                return glib::Propagation::Stop;
            }
            gdk::Key::Right => {
                if cursor < text.len() {
                    cursor = text[cursor..].char_indices().nth(1).map(|(i, _)| cursor + i).unwrap_or(text.len());
                    self.imp().cursor_char_idx.set(cursor);
                    self.imp().cursor_blink_visible.set(true);
                    self.queue_draw();
                }
                return glib::Propagation::Stop;
            }
            gdk::Key::Home => {
                let start_of_line = text[..cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
                self.imp().cursor_char_idx.set(start_of_line);
                self.imp().cursor_blink_visible.set(true);
                self.queue_draw();
                return glib::Propagation::Stop;
            }
            gdk::Key::End => {
                let end_of_line = text[cursor..].find('\n').map(|i| cursor + i).unwrap_or(text.len());
                self.imp().cursor_char_idx.set(end_of_line);
                self.imp().cursor_blink_visible.set(true);
                self.queue_draw();
                return glib::Propagation::Stop;
            }
            gdk::Key::Up => {
                let line_start = text[..cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
                let col = cursor - line_start;
                if line_start > 0 {
                    let prev_line_start = text[..line_start - 1].rfind('\n').map(|i| i + 1).unwrap_or(0);
                    let prev_line_len = (line_start - 1) - prev_line_start;
                    cursor = prev_line_start + col.min(prev_line_len);
                    while !text.is_char_boundary(cursor) && cursor > 0 {
                        cursor -= 1;
                    }
                    self.imp().cursor_char_idx.set(cursor);
                    self.imp().cursor_blink_visible.set(true);
                    self.queue_draw();
                }
                return glib::Propagation::Stop;
            }
            gdk::Key::Down => {
                if let Some(nl) = text[cursor..].find('\n').map(|i| cursor + i) {
                    let line_start = text[..cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
                    let col = cursor - line_start;
                    let next_line_start = nl + 1;
                    let next_line_len = text[next_line_start..].find('\n').unwrap_or_else(|| text.len() - next_line_start);
                    cursor = next_line_start + col.min(next_line_len);
                    while !text.is_char_boundary(cursor) && cursor > 0 {
                        cursor -= 1;
                    }
                    self.imp().cursor_char_idx.set(cursor);
                    self.imp().cursor_blink_visible.set(true);
                    self.queue_draw();
                }
                return glib::Propagation::Stop;
            }
            gdk::Key::Tab => {
                text.insert_str(cursor, "    ");
                cursor += 4;
                self.update_node_text(&node_id, text, cursor);
                return glib::Propagation::Stop;
            }
            gdk::Key::Return | gdk::Key::KP_Enter => {
                text.insert(cursor, '\n');
                cursor += 1;
                self.update_node_text(&node_id, text, cursor);
                return glib::Propagation::Stop;
            }
            _ => {
                if state.contains(gdk::ModifierType::CONTROL_MASK) || state.contains(gdk::ModifierType::ALT_MASK) {
                    return glib::Propagation::Proceed;
                }
                if let Some(ch) = key.to_unicode() {
                    if !ch.is_control() {
                        text.insert(cursor, ch);
                        cursor += ch.len_utf8();
                        self.update_node_text(&node_id, text, cursor);
                        return glib::Propagation::Stop;
                    }
                }
            }
        }
        glib::Propagation::Proceed
    }

    fn update_node_text(&self, node_id: &SceneNodeId, text: String, cursor: usize) {
        self.set_text_replacement(&node_id.0, text.clone());
        self.imp().cursor_char_idx.set(cursor);
        self.imp().cursor_blink_visible.set(true);
        if let Some(handler) = self.imp().text_selection_handler.borrow().as_ref() {
            handler(node_id.0.clone(), text);
        }
        self.queue_draw();
    }

    fn update_pointer_at(&self, x: f64, y: f64) {
        if self.imp().ink_tool.get() != InkEditTool::View {
            return;
        }
        match self.action_at(x, y) {
            Some(HitAction::OpenLink(target)) => {
                self.set_cursor_from_name(Some("pointer"));
                self.set_tooltip_text(Some(&target));
                self.set_hovered_attachment(None);
            }
            Some(HitAction::OpenAttachment(resource_id)) => {
                self.set_cursor_from_name(Some("pointer"));
                let name = self.attachment_name(&resource_id);
                let tooltip = name.as_deref().map(text::glib_text);
                self.set_tooltip_text(tooltip.as_deref());
                self.set_hovered_attachment(Some(resource_id));
            }
            Some(HitAction::SelectObject(_)) | None => {
                self.set_cursor_from_name(Some("text"));
                self.set_tooltip_text(None);
                self.set_hovered_attachment(None);
            }
        }
    }

    fn begin_ink_edit(&self, x: f64, y: f64) {
        let tool = self.imp().ink_tool.get();
        if tool == InkEditTool::View || self.scene().is_none() {
            return;
        }
        let point = self.page_point(x, y);
        self.imp().ink_drag_dirty.set(false);
        match tool {
            InkEditTool::Pen => {
                *self.imp().pending_ink.borrow_mut() = Some(EditableStroke {
                    points: vec![point],
                    color: self.imp().pen_color.get(),
                    width: self.imp().pen_width.get(),
                });
            }
            InkEditTool::Eraser => {
                self.imp().ink_undo.borrow_mut().push(self.ink_edits());
                self.erase_ink_at(point);
            }
            InkEditTool::View => {}
        }
        self.queue_draw();
    }

    fn update_ink_edit(&self, x: f64, y: f64) {
        let point = self.page_point(x, y);
        match self.imp().ink_tool.get() {
            InkEditTool::Pen => {
                if let Some(stroke) = self.imp().pending_ink.borrow_mut().as_mut() {
                    if stroke.points.last().is_none_or(|last| {
                        (last.x - point.x).abs() + (last.y - point.y).abs() >= 0.7
                    }) {
                        stroke.points.push(point);
                        self.queue_draw();
                    }
                }
            }
            InkEditTool::Eraser => {
                self.erase_ink_at(point);
                self.queue_draw();
            }
            InkEditTool::View => {}
        }
    }

    fn finish_ink_edit(&self, x: f64, y: f64) {
        match self.imp().ink_tool.get() {
            InkEditTool::Pen => {
                self.update_ink_edit(x, y);
                if let Some(stroke) = self.imp().pending_ink.borrow_mut().take() {
                    if !stroke.points.is_empty() {
                        self.imp().ink_undo.borrow_mut().push(self.ink_edits());
                        self.imp().ink_edits.borrow_mut().added_strokes.push(stroke);
                        self.emit_ink_edits_changed();
                    }
                }
            }
            InkEditTool::Eraser => {
                self.update_ink_edit(x, y);
                if self.imp().ink_drag_dirty.replace(false) {
                    self.emit_ink_edits_changed();
                } else {
                    self.imp().ink_undo.borrow_mut().pop();
                }
            }
            InkEditTool::View => {}
        }
        self.queue_draw();
    }

    fn page_point(&self, x: f64, y: f64) -> onenote_core::InkPoint {
        let Some(scene) = self.scene() else {
            return onenote_core::InkPoint { x: 0.0, y: 0.0 };
        };
        let resolved = self.resolved_layout(&scene);
        onenote_core::InkPoint {
            x: f64_to_f32(x / f64::from(self.zoom())) + resolved.bounds.x - CANVAS_MARGIN,
            y: f64_to_f32(y / f64::from(self.zoom())) + resolved.bounds.y - CANVAS_MARGIN,
        }
    }

    fn emit_ink_edits_changed(&self) {
        let document = self.ink_edits();
        if let Some(handler) = self.imp().ink_edit_handler.borrow().as_ref() {
            handler(document);
        }
    }

    fn erase_ink_at(&self, point: onenote_core::InkPoint) {
        let Some(scene) = self.scene() else {
            return;
        };
        let resolved = self.resolved_layout(&scene);
        let radius = 10.0_f32;
        for node in &scene.nodes {
            let ScenePrimitive::Ink { strokes } = &node.primitive else {
                continue;
            };
            let bounds = resolved.node_bounds(node);
            if point.x < bounds.x - radius
                || point.x > bounds.x + bounds.width + radius
                || point.y < bounds.y - radius
                || point.y > bounds.y + bounds.height + radius
            {
                continue;
            }
            for (stroke_index, stroke) in strokes.iter().enumerate() {
                let hit = if stroke.points.len() == 1 {
                    point_segment_distance(point, stroke.points[0], stroke.points[0]) <= radius
                } else {
                    stroke.points.windows(2).any(|segment| {
                        point_segment_distance(point, segment[0], segment[1]) <= radius
                    })
                };
                if hit {
                    let erased = ErasedStroke {
                        object_id: node.source_object_id.as_str().to_owned(),
                        stroke_index,
                    };
                    let mut edits = self.imp().ink_edits.borrow_mut();
                    if !edits.erased_strokes.contains(&erased) {
                        edits.erased_strokes.push(erased);
                        self.imp().ink_drag_dirty.set(true);
                    }
                }
            }
        }
        let erased_added: Vec<usize> = self
            .imp()
            .ink_edits
            .borrow()
            .added_strokes
            .iter()
            .enumerate()
            .filter_map(|(index, stroke)| {
                let hit = if stroke.points.len() == 1 {
                    point_segment_distance(point, stroke.points[0], stroke.points[0]) <= radius
                } else {
                    stroke.points.windows(2).any(|segment| {
                        point_segment_distance(point, segment[0], segment[1]) <= radius
                    })
                };
                hit.then_some(index)
            })
            .collect();
        if !erased_added.is_empty() {
            let mut edits = self.imp().ink_edits.borrow_mut();
            for index in erased_added.into_iter().rev() {
                edits.added_strokes.remove(index);
            }
            self.imp().ink_drag_dirty.set(true);
        }
        if self.imp().ink_drag_dirty.get() {
            self.queue_draw();
        }
    }

    fn set_hovered_attachment(&self, resource_id: Option<ResourceId>) {
        if *self.imp().hovered_attachment.borrow() == resource_id {
            return;
        }
        *self.imp().hovered_attachment.borrow_mut() = resource_id;
        self.queue_draw();
    }

    fn attachment_name(&self, resource_id: &ResourceId) -> Option<String> {
        self.scene()?
            .nodes
            .iter()
            .find_map(|node| match &node.primitive {
                ScenePrimitive::Attachment(attachment)
                    if attachment.resource.id == *resource_id =>
                {
                    Some(attachment.resource.name.clone())
                }
                _ => None,
            })
    }

    fn attachment_actions(&self) -> Vec<(ResourceId, HitAction, Rect)> {
        let Some(scene) = self.scene() else {
            return Vec::new();
        };
        let resolved = self.resolved_layout(&scene);
        scene
            .hit_regions
            .iter()
            .filter_map(|region| {
                let HitAction::OpenAttachment(resource_id) = &region.action else {
                    return None;
                };
                let node = scene.nodes.iter().find(|node| node.id == region.node_id)?;
                Some((
                    resource_id.clone(),
                    region.action.clone(),
                    resolved.hit_region_bounds(node, region),
                ))
            })
            .collect()
    }

    fn focus_attachment(&self, resource_id: &ResourceId) {
        if let Some(index) = self
            .attachment_actions()
            .iter()
            .position(|(candidate, _, _)| candidate == resource_id)
        {
            self.imp().focused_attachment.set(Some(index));
            self.reveal_attachment(index);
            self.queue_draw();
        }
    }

    fn move_attachment_focus(&self, direction: gtk::DirectionType) -> bool {
        let targets = self.attachment_actions();
        if targets.is_empty() {
            return false;
        }
        let backwards = direction == gtk::DirectionType::TabBackward;
        let next = next_attachment_index(
            self.imp().focused_attachment.get(),
            targets.len(),
            backwards,
        );
        let Some(next) = next else {
            self.imp().focused_attachment.set(None);
            self.queue_draw();
            return false;
        };
        if !self.has_focus() && !self.grab_focus() {
            return false;
        }
        self.imp().focused_attachment.set(Some(next));
        self.reveal_attachment(next);
        self.queue_draw();
        true
    }

    fn activate_focused_attachment(&self) -> bool {
        let Some(index) = self.imp().focused_attachment.get() else {
            return false;
        };
        let Some((_, action, _)) = self.attachment_actions().get(index).cloned() else {
            self.imp().focused_attachment.set(None);
            return false;
        };
        let Some(handler) = self.imp().action_handler.borrow().as_ref().cloned() else {
            return false;
        };
        handler(action);
        true
    }

    fn reveal_attachment(&self, index: usize) {
        let Some((_, _, bounds)) = self.attachment_actions().get(index).cloned() else {
            return;
        };
        let Some(scene) = self.scene() else {
            return;
        };
        let Some(root) = self
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_then(|widget| widget.downcast::<gtk::ScrolledWindow>().ok())
        else {
            return;
        };
        let resolved = self.resolved_layout(&scene);
        let zoom = f64::from(self.zoom());
        let x = f64::from(bounds.x - resolved.bounds.x + CANVAS_MARGIN) * zoom;
        let y = f64::from(bounds.y - resolved.bounds.y + CANVAS_MARGIN) * zoom;
        reveal_adjustment(&root.hadjustment(), x, f64::from(bounds.width) * zoom);
        reveal_adjustment(&root.vadjustment(), y, f64::from(bounds.height) * zoom);
    }

    fn action_at(&self, x: f64, y: f64) -> Option<HitAction> {
        let scene = self.scene()?;
        let resolved = self.resolved_layout(&scene);
        let zoom = f64::from(self.zoom());
        let logical_x = f64_to_f32(x / zoom) + resolved.bounds.x - CANVAS_MARGIN;
        let logical_y = f64_to_f32(y / zoom) + resolved.bounds.y - CANVAS_MARGIN;
        for node in scene.nodes.iter().rev() {
            let node_bounds = resolved.node_bounds(node);
            if !contains(node_bounds, logical_x, logical_y) {
                continue;
            }
            if let ScenePrimitive::Text { block, marker } = &node.primitive {
                let layout = self.text_layout(node, block, marker.as_deref());
                if let Some(target) =
                    layout.link_at(logical_x - node_bounds.x, logical_y - node_bounds.y)
                {
                    return Some(HitAction::OpenLink(target.to_owned()));
                }
            }
            if let Some(region) = scene.hit_regions.iter().rev().find(|region| {
                region.node_id == node.id
                    && contains(
                        resolved.hit_region_bounds(node, region),
                        logical_x,
                        logical_y,
                    )
            }) {
                return Some(region.action.clone());
            }
        }
        None
    }

/// Parse basic markdown syntax (# heading, **bold**, *italic*, <u>underline</u>, ~~strike~~)
/// into clean display text and exact TextRun ranges.
pub fn parse_formatted_text(
    base: &onenote_core::TextStyle,
    input: &str,
) -> (String, onenote_core::TextStyle, Vec<onenote_core::TextRun>) {
    let mut base_style = base.clone();
    let trimmed = input.trim_start();
    let text_to_parse = if trimmed.starts_with("# ") {
        base_style.bold = true;
        base_style.font_size = Some(24.0);
        trimmed[2..].trim_start()
    } else if trimmed.starts_with("## ") {
        base_style.bold = true;
        base_style.font_size = Some(18.0);
        trimmed[3..].trim_start()
    } else if trimmed.starts_with("### ") {
        base_style.bold = true;
        base_style.font_size = Some(15.0);
        trimmed[4..].trim_start()
    } else if trimmed.starts_with("> ") {
        base_style.italic = true;
        trimmed[2..].trim_start()
    } else {
        input
    };

    let mut clean_text = String::with_capacity(text_to_parse.len());
    let mut runs = Vec::new();
    let mut chars = text_to_parse.chars().peekable();
    let mut current_utf16: u32 = 0;

    while let Some(c) = chars.next() {
        if c == '*' && chars.peek() == Some(&'*') {
            chars.next();
            let mut content = String::new();
            let mut closed = false;
            while let Some(bc) = chars.next() {
                if bc == '*' && chars.peek() == Some(&'*') {
                    chars.next();
                    closed = true;
                    break;
                } else {
                    content.push(bc);
                }
            }
            if closed {
                let start_utf16 = current_utf16;
                let len_utf16 = content.encode_utf16().count() as u32;
                current_utf16 += len_utf16;
                clean_text.push_str(&content);
                let mut style = base_style.clone();
                style.bold = true;
                runs.push(onenote_core::TextRun {
                    start_utf16,
                    end_utf16: current_utf16,
                    style,
                });
            } else {
                clean_text.push_str("**");
                current_utf16 += 2;
                clean_text.push_str(&content);
                current_utf16 += content.encode_utf16().count() as u32;
            }
        } else if c == '*' {
            let mut content = String::new();
            let mut closed = false;
            while let Some(ic) = chars.next() {
                if ic == '*' {
                    closed = true;
                    break;
                } else {
                    content.push(ic);
                }
            }
            if closed && !content.is_empty() {
                let start_utf16 = current_utf16;
                let len_utf16 = content.encode_utf16().count() as u32;
                current_utf16 += len_utf16;
                clean_text.push_str(&content);
                let mut style = base_style.clone();
                style.italic = true;
                runs.push(onenote_core::TextRun {
                    start_utf16,
                    end_utf16: current_utf16,
                    style,
                });
            } else {
                clean_text.push('*');
                current_utf16 += 1;
                clean_text.push_str(&content);
                current_utf16 += content.encode_utf16().count() as u32;
            }
        } else if c == '~' && chars.peek() == Some(&'~') {
            chars.next();
            let mut content = String::new();
            let mut closed = false;
            while let Some(sc) = chars.next() {
                if sc == '~' && chars.peek() == Some(&'~') {
                    chars.next();
                    closed = true;
                    break;
                } else {
                    content.push(sc);
                }
            }
            if closed {
                let start_utf16 = current_utf16;
                let len_utf16 = content.encode_utf16().count() as u32;
                current_utf16 += len_utf16;
                clean_text.push_str(&content);
                let mut style = base_style.clone();
                style.strikethrough = true;
                runs.push(onenote_core::TextRun {
                    start_utf16,
                    end_utf16: current_utf16,
                    style,
                });
            } else {
                clean_text.push_str("~~");
                current_utf16 += 2;
                clean_text.push_str(&content);
                current_utf16 += content.encode_utf16().count() as u32;
            }
        } else {
            let utf16_len = c.len_utf16() as u32;
            current_utf16 += utf16_len;
            clean_text.push(c);
        }
    }

    (clean_text, base_style, runs)
}

    fn text_layout(
        &self,
        node: &SceneNode,
        block: &onenote_core::TextBlock,
        marker: Option<&str>,
    ) -> Rc<text::TextLayout> {
        if let Some(layout) = self.imp().text_layouts.borrow().get(&node.id).cloned() {
            return layout;
        }
        let edited_block = self
            .imp()
            .ink_edits
            .borrow()
            .text_replacements
            .get(&node.id.0)
            .map(|replacement| {
                let (clean_text, base_style, runs) =
                    Self::parse_formatted_text(&block.base_style, replacement);
                let mut edited = block.clone();
                edited.text = clean_text;
                edited.base_style = base_style;
                edited.runs = runs;
                edited.math.clear();
                edited.links.clear();
                edited
            });
        let layout_block = edited_block.as_ref().unwrap_or(block);
        let layout = Rc::new(text::layout_with_math(
            &self.pango_context(),
            layout_block,
            marker,
            node.bounds.width,
            |span, style| self.math_shape(span, style),
        ));
        self.imp()
            .text_layouts
            .borrow_mut()
            .insert(node.id.clone(), Rc::clone(&layout));
        layout
    }

    fn resolved_layout(&self, scene: &PageScene) -> Rc<ResolvedLayout> {
        let generation = self.imp().layout_generation.get();
        if let Some(cached) = self.imp().resolved_layout.borrow().as_ref() {
            if cached.generation == generation {
                return Rc::clone(&cached.layout);
            }
        }

        let measured_heights = scene
            .nodes
            .iter()
            .filter_map(|node| match &node.primitive {
                ScenePrimitive::Text { block, marker } => Some((
                    node.id.clone(),
                    self.text_layout(node, block, marker.as_deref())
                        .measured_height(),
                )),
                _ => None,
            })
            .collect::<HashMap<_, _>>();
        let layout = Rc::new(ResolvedLayout::new(scene, &measured_heights));
        *self.imp().resolved_layout.borrow_mut() = Some(imp::CachedResolvedLayout {
            generation,
            layout: Rc::clone(&layout),
        });
        layout
    }

    fn invalidate_text_geometry(&self) {
        let imp = self.imp();
        imp.text_layouts.borrow_mut().clear();
        imp.resolved_layout.borrow_mut().take();
        imp.layout_generation
            .set(imp.layout_generation.get().wrapping_add(1));
    }

    pub(crate) fn resolved_scene_bounds(&self) -> Option<Rect> {
        let scene = self.scene()?;
        Some(self.resolved_layout(&scene).bounds)
    }

    pub(crate) fn resolved_source_bounds(&self, source: &onenote_core::ObjectId) -> Option<Rect> {
        let scene = self.scene()?;
        self.resolved_layout(&scene).source_bounds(&scene, source)
    }

    fn snapshot_scene(&self, snapshot: &gtk::Snapshot) {
        let Some(scene) = self.scene() else {
            return;
        };
        let resolved = self.resolved_layout(&scene);
        let viewport = self.logical_viewport(resolved.bounds);
        snapshot.save();
        snapshot.scale(self.zoom(), self.zoom());
        snapshot.translate(&graphene::Point::new(
            CANVAS_MARGIN - resolved.bounds.x,
            CANVAS_MARGIN - resolved.bounds.y,
        ));
        let edits = self.imp().ink_edits.borrow();
        let mut ink_batch: Vec<(&SceneNode, Rect, &[onenote_core::InkStroke])> = Vec::new();
        let mut ink_batch_bounds: Option<Rect> = None;

        for node in resolved.visible_nodes(&scene, viewport, VIEWPORT_OVERSCAN) {
            let node_bounds = resolved.node_bounds(node);
            if let ScenePrimitive::Ink { strokes } = &node.primitive {
                let exceeds_limit = ink_batch_bounds
                    .is_some_and(|b| b.width > 4096.0 || b.height > 4096.0);
                if exceeds_limit {
                    if let Some(bounds) = ink_batch_bounds.take() {
                        snapshot_ink_batch(snapshot, bounds, &ink_batch, &edits.erased_strokes);
                        ink_batch.clear();
                    }
                }
                ink_batch_bounds = Some(ink_batch_bounds.map_or(node_bounds, |b| Rect::union(b, node_bounds)));
                ink_batch.push((node, node_bounds, strokes));
            } else {
                if let Some(bounds) = ink_batch_bounds.take() {
                    snapshot_ink_batch(snapshot, bounds, &ink_batch, &edits.erased_strokes);
                    ink_batch.clear();
                }
                self.snapshot_node(snapshot, node, node_bounds);
            }
        }
        if let Some(bounds) = ink_batch_bounds.take() {
            snapshot_ink_batch(snapshot, bounds, &ink_batch, &edits.erased_strokes);
            ink_batch.clear();
        }
        self.snapshot_find_highlights(snapshot, viewport);
        let edits = self.imp().ink_edits.borrow();
        snapshot_editable_ink(
            snapshot,
            resolved.bounds,
            &edits.added_strokes,
            self.imp().pending_ink.borrow().as_ref(),
        );

        // Render added images anywhere on page
        for img in &edits.added_images {
            let file = gio::File::for_path(&img.file_path);
            if let Ok(texture) = gdk::Texture::from_file(&file) {
                let rect = graphene_rect(Rect {
                    x: img.x,
                    y: img.y,
                    width: img.width,
                    height: img.height,
                });
                snapshot.append_texture(&texture, &rect);
                if self.imp().selected_text_node.borrow().as_ref().map(|s| &s.0) == Some(&img.id) {
                    snapshot_attachment_border(
                        snapshot,
                        Rect {
                            x: img.x,
                            y: img.y,
                            width: img.width,
                            height: img.height,
                        },
                        gdk::RGBA::new(0.46, 0.12, 0.65, 0.85),
                        2.0,
                    );
                }
            }
        }

        // Render added text boxes anywhere on page
        for tb in &edits.added_text_boxes {
            let default_style = onenote_core::TextStyle::default();
            let (clean_text, base_style, runs) = Self::parse_formatted_text(&default_style, &tb.text);
            let text_block = onenote_core::TextBlock {
                text: clean_text.clone(),
                base_style,
                runs,
                math: Vec::new(),
                links: Vec::new(),
                alignment: onenote_core::TextAlignment::Left,
                space_before: 0.0,
                space_after: 0.0,
                line_spacing: None,
            };
            let layout = text::layout_with_math(
                &self.pango_context(),
                &text_block,
                None,
                tb.width,
                |_, _| None,
            );
            let bounds = Rect {
                x: tb.x,
                y: tb.y,
                width: tb.width,
                height: tb.height,
            };
            let is_selected = self.imp().selected_text_node.borrow().as_ref().map(|s| &s.0) == Some(&tb.id);
            if is_selected {
                let pad = 4.0;
                let cont_bounds = Rect {
                    x: bounds.x - pad,
                    y: bounds.y - pad,
                    width: bounds.width + pad * 2.0,
                    height: bounds.height + pad * 2.0,
                };
                snapshot.append_color(
                    &gdk::RGBA::new(0.46, 0.12, 0.65, 0.08),
                    &graphene_rect(cont_bounds),
                );
                snapshot_attachment_border(
                    snapshot,
                    cont_bounds,
                    gdk::RGBA::new(0.46, 0.12, 0.65, 0.75),
                    1.5,
                );
                let handle_bounds = Rect {
                    x: cont_bounds.x,
                    y: (cont_bounds.y - 6.0).max(0.0),
                    width: cont_bounds.width.min(60.0).max(24.0),
                    height: 5.0,
                };
                snapshot.append_color(
                    &gdk::RGBA::new(0.46, 0.12, 0.65, 0.85),
                    &graphene_rect(handle_bounds),
                );

                if self.imp().cursor_blink_visible.get() {
                    let cursor_byte = self.imp().cursor_char_idx.get().min(clean_text.len());
                    let pos = layout.layout.index_to_pos(i32::try_from(cursor_byte).unwrap_or(0));
                    let cur_x = bounds.x + text::from_pango_units(pos.x());
                    let cur_y = bounds.y + text::from_pango_units(pos.y());
                    let cur_h = text::from_pango_units(pos.height()).max(18.0);
                    let cursor_rect = Rect {
                        x: cur_x,
                        y: cur_y,
                        width: 2.0,
                        height: cur_h,
                    };
                    snapshot.append_color(
                        &gdk::RGBA::new(0.12, 0.12, 0.15, 1.0),
                        &graphene_rect(cursor_rect),
                    );
                }
            }
            snapshot.save();
            snapshot.translate(&graphene::Point::new(tb.x, tb.y));
            if clean_text.is_empty() && is_selected {
                let placeholder_block = onenote_core::TextBlock {
                    text: "Type note here...".to_string(),
                    base_style: {
                        let mut s = onenote_core::TextStyle::default();
                        s.foreground = Some(Color { red: 160, green: 160, blue: 160, alpha: 180 });
                        s
                    },
                    runs: Vec::new(),
                    math: Vec::new(),
                    links: Vec::new(),
                    alignment: onenote_core::TextAlignment::Left,
                    space_before: 0.0,
                    space_after: 0.0,
                    line_spacing: None,
                };
                let pl_layout = text::layout_with_math(
                    &self.pango_context(),
                    &placeholder_block,
                    None,
                    tb.width,
                    |_, _| None,
                );
                snapshot.append_layout(&pl_layout.layout, &gdk::RGBA::new(0.65, 0.65, 0.65, 0.6));
            } else {
                snapshot.append_layout(&layout.layout, &self.default_text_color());
            }
            snapshot.restore();
        }

        snapshot.restore();
    }

    fn snapshot_find_highlights(&self, snapshot: &gtk::Snapshot, viewport: Rect) {
        let highlights = self.imp().find_highlights.borrow();
        for (index, found) in highlights.matches.iter().enumerate() {
            if !highlights.highlight_all && Some(index) != highlights.active {
                continue;
            }
            let color = if Some(index) == highlights.active {
                gdk::RGBA::new(1.0, 0.55, 0.05, 0.58)
            } else {
                gdk::RGBA::new(1.0, 0.82, 0.10, 0.34)
            };
            for bounds in &found.bounds {
                if rects_intersect(*bounds, viewport) {
                    snapshot.append_color(&color, &graphene_rect(*bounds));
                }
            }
        }
    }

    fn logical_viewport(&self, scene_bounds: Rect) -> Rect {
        let Some(scrolled) = self
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_then(|widget| widget.downcast::<gtk::ScrolledWindow>().ok())
        else {
            return scene_bounds;
        };
        let zoom = f64::from(self.zoom());
        Rect {
            x: f64_to_f32(scrolled.hadjustment().value() / zoom) + scene_bounds.x - CANVAS_MARGIN,
            y: f64_to_f32(scrolled.vadjustment().value() / zoom) + scene_bounds.y - CANVAS_MARGIN,
            width: scrolled.width().to_f32().unwrap_or(0.0) / self.zoom(),
            height: scrolled.height().to_f32().unwrap_or(0.0) / self.zoom(),
        }
    }

    fn snapshot_node(&self, snapshot: &gtk::Snapshot, node: &SceneNode, node_bounds: Rect) {
        match &node.primitive {
            ScenePrimitive::Fill {
                color,
                corner_radius: _,
            } => snapshot.append_color(&rgba(*color), &graphene_rect(node_bounds)),
            ScenePrimitive::Text { block, marker } => {
                let is_selected = self.imp().selected_text_node.borrow().as_ref() == Some(&node.id);
                if is_selected {
                    let pad = 4.0;
                    let cont_bounds = Rect {
                        x: node_bounds.x - pad,
                        y: node_bounds.y - pad,
                        width: node_bounds.width + pad * 2.0,
                        height: node_bounds.height + pad * 2.0,
                    };
                    snapshot.append_color(
                        &gdk::RGBA::new(0.46, 0.12, 0.65, 0.08),
                        &graphene_rect(cont_bounds),
                    );
                    snapshot_attachment_border(
                        snapshot,
                        cont_bounds,
                        gdk::RGBA::new(0.46, 0.12, 0.65, 0.75),
                        1.5,
                    );
                    let handle_bounds = Rect {
                        x: cont_bounds.x,
                        y: (cont_bounds.y - 6.0).max(0.0),
                        width: cont_bounds.width.min(60.0).max(24.0),
                        height: 5.0,
                    };
                    snapshot.append_color(
                        &gdk::RGBA::new(0.46, 0.12, 0.65, 0.85),
                        &graphene_rect(handle_bounds),
                    );
                }
                let layout = self.text_layout(node, block, marker.as_deref());
                if is_selected && self.imp().cursor_blink_visible.get() {
                    let current_text = self.text_for_node(&node.id).unwrap_or_else(|| block.visible_text());
                    let cursor_byte = self.imp().cursor_char_idx.get().min(current_text.len());
                    let pos = layout.layout.index_to_pos(i32::try_from(cursor_byte).unwrap_or(0));
                    let cur_x = node_bounds.x + text::from_pango_units(pos.x());
                    let cur_y = node_bounds.y + text::from_pango_units(pos.y());
                    let cur_h = text::from_pango_units(pos.height()).max(18.0);
                    let cursor_rect = Rect {
                        x: cur_x,
                        y: cur_y,
                        width: 2.0,
                        height: cur_h,
                    };
                    snapshot.append_color(
                        &gdk::RGBA::new(0.12, 0.12, 0.15, 1.0),
                        &graphene_rect(cursor_rect),
                    );
                }
                snapshot.save();
                snapshot.translate(&graphene::Point::new(node_bounds.x, node_bounds.y));
                snapshot.append_layout(&layout.layout, &self.default_text_color());
                for math in &layout.math {
                    let bounds = Rect {
                        x: math.x,
                        y: math.y,
                        width: math.width,
                        height: math.height,
                    };
                    if let Some(texture) = self.math_texture(&math.key) {
                        snapshot.append_texture(&texture, &graphene_rect(bounds));
                    } else if self.imp().math_errors.borrow().contains_key(&math.key) {
                        snapshot.append_color(
                            &gdk::RGBA::new(1.0, 0.92, 0.92, 0.72),
                            &graphene_rect(bounds),
                        );
                        self.snapshot_label(
                            snapshot,
                            bounds,
                            &format!("⚠ {}", math.fallback),
                            gdk::RGBA::new(0.55, 0.08, 0.08, 1.0),
                        );
                    } else {
                        self.snapshot_label(
                            snapshot,
                            bounds,
                            &math.fallback,
                            self.default_text_color(),
                        );
                    }
                }
                snapshot.restore();
            }
            ScenePrimitive::Image(image) => self.snapshot_image(snapshot, node_bounds, image),
            ScenePrimitive::Attachment(attachment) => {
                let hovered = self.imp().hovered_attachment.borrow().as_ref()
                    == Some(&attachment.resource.id);
                let focused = self.has_focus()
                    && self
                        .imp()
                        .focused_attachment
                        .get()
                        .and_then(|index| self.attachment_actions().get(index).cloned())
                        .is_some_and(|(resource_id, _, _)| resource_id == attachment.resource.id);
                self.snapshot_attachment(snapshot, node_bounds, attachment, hovered, focused);
            }
            ScenePrimitive::Ink { strokes } => {
                let edits = self.imp().ink_edits.borrow();
                snapshot_ink(
                    snapshot,
                    node_bounds,
                    strokes,
                    &edits.erased_strokes,
                    node.source_object_id.as_str(),
                );
            }
            ScenePrimitive::Line {
                color,
                width,
                to_x,
                to_y,
            } => snapshot_line(
                snapshot,
                node.bounds,
                node_bounds,
                *to_x,
                *to_y,
                *color,
                *width,
            ),
            ScenePrimitive::Placeholder { label } => {
                self.snapshot_label(snapshot, node_bounds, label, self.default_text_color());
            }
        }
    }

    fn snapshot_image(&self, snapshot: &gtk::Snapshot, bounds: Rect, image: &onenote_core::Image) {
        if let Some(texture) = self.texture(&image.resource.id) {
            snapshot.append_texture(&texture, &graphene_rect(bounds));
            return;
        }
        let Some(primary_failure) = self.image_failure(&image.resource.id) else {
            Self::snapshot_image_loading(snapshot, bounds);
            self.request_texture(image.resource.id.clone());
            return;
        };
        let Some(fallback) = &image.web_fallback else {
            self.snapshot_image_failure(
                snapshot,
                bounds,
                image_failure_label(primary_failure, None),
            );
            return;
        };
        if let Some(texture) = self.texture(&fallback.id) {
            snapshot.append_texture(&texture, &graphene_rect(bounds));
        } else if let Some(fallback_failure) = self.image_failure(&fallback.id) {
            self.snapshot_image_failure(
                snapshot,
                bounds,
                image_failure_label(primary_failure, Some(fallback_failure)),
            );
        } else {
            Self::snapshot_image_loading(snapshot, bounds);
            self.request_texture(fallback.id.clone());
        }
    }

    fn snapshot_attachment(
        &self,
        snapshot: &gtk::Snapshot,
        bounds: Rect,
        attachment: &onenote_core::Attachment,
        hovered: bool,
        focused: bool,
    ) {
        let text_color = self.default_text_color();
        let dark_surface = perceived_lightness(text_color) > 0.55;
        let background = if dark_surface {
            if hovered {
                gdk::RGBA::new(0.20, 0.21, 0.23, 1.0)
            } else {
                gdk::RGBA::new(0.14, 0.15, 0.17, 1.0)
            }
        } else if hovered {
            gdk::RGBA::new(0.91, 0.93, 0.96, 1.0)
        } else {
            gdk::RGBA::new(0.96, 0.97, 0.98, 1.0)
        };
        let border = if dark_surface {
            gdk::RGBA::new(0.42, 0.44, 0.48, 1.0)
        } else {
            gdk::RGBA::new(0.65, 0.68, 0.73, 1.0)
        };
        let accent = if attachment.resource.status == ResourceStatus::Available {
            if dark_surface {
                gdk::RGBA::new(0.55, 0.75, 0.98, 1.0)
            } else {
                gdk::RGBA::new(0.12, 0.36, 0.64, 1.0)
            }
        } else {
            gdk::RGBA::new(0.78, 0.22, 0.24, 1.0)
        };
        snapshot.append_color(&background, &graphene_rect(bounds));
        snapshot_attachment_border(snapshot, bounds, border, 1.0);
        let icon_bounds = Rect {
            x: bounds.x + 4.0,
            y: bounds.y + (bounds.height - 24.0).max(0.0) / 2.0,
            width: bounds.width.clamp(1.0, 24.0),
            height: bounds.height.clamp(1.0, 24.0),
        };
        if let Some(icon) = &attachment.icon {
            if let Some(texture) = self.texture(&icon.id) {
                snapshot.append_texture(&texture, &graphene_rect(icon_bounds));
            } else {
                snapshot_file_icon(snapshot, bounds, accent);
                if self.image_failure(&icon.id).is_none() {
                    self.request_texture(icon.id.clone());
                }
            }
        } else {
            snapshot_file_icon(snapshot, bounds, accent);
        }

        let label_bounds = Rect {
            x: bounds.x + 32.0,
            y: bounds.y,
            width: (bounds.width - 32.0).max(1.0),
            height: bounds.height,
        };
        self.snapshot_label(
            snapshot,
            label_bounds,
            &attachment.resource.name,
            text_color,
        );
        if focused {
            snapshot_attachment_border(
                snapshot,
                Rect {
                    x: bounds.x + 2.0,
                    y: bounds.y + 2.0,
                    width: (bounds.width - 4.0).max(1.0),
                    height: (bounds.height - 4.0).max(1.0),
                },
                accent,
                2.0,
            );
        }
    }

    fn snapshot_label(
        &self,
        snapshot: &gtk::Snapshot,
        bounds: Rect,
        label: &str,
        color: gdk::RGBA,
    ) {
        let layout = gtk::pango::Layout::new(&self.pango_context());
        let label = text::glib_text(label);
        layout.set_text(&label);
        layout.set_width(to_pango_units((bounds.width - 16.0).max(1.0)));
        layout.set_ellipsize(gtk::pango::EllipsizeMode::End);
        snapshot.save();
        snapshot.translate(&graphene::Point::new(bounds.x + 8.0, bounds.y + 8.0));
        snapshot.append_layout(&layout, &color);
        snapshot.restore();
    }

    fn texture(&self, id: &ResourceId) -> Option<gdk::Texture> {
        self.imp()
            .textures
            .borrow()
            .get(id)
            .map(|entry| entry.texture.clone())
    }

    fn image_failure(&self, id: &ResourceId) -> Option<ImageDecodeFailure> {
        self.imp().failed.borrow().get(id).copied()
    }

    fn snapshot_image_loading(snapshot: &gtk::Snapshot, bounds: Rect) {
        snapshot.append_color(
            &gdk::RGBA::new(0.94, 0.94, 0.94, 1.0),
            &graphene_rect(bounds),
        );
    }

    fn snapshot_image_failure(&self, snapshot: &gtk::Snapshot, bounds: Rect, label: &str) {
        snapshot.append_color(
            &gdk::RGBA::new(1.0, 0.92, 0.92, 1.0),
            &graphene_rect(bounds),
        );
        self.snapshot_label(
            snapshot,
            bounds,
            label,
            gdk::RGBA::new(0.55, 0.08, 0.08, 1.0),
        );
    }

    fn request_texture(&self, id: ResourceId) {
        let imp = self.imp();
        if imp.failed.borrow().contains_key(&id)
            || imp.pending.borrow().contains(&id)
            || imp.textures.borrow().contains_key(&id)
        {
            return;
        }
        let Some(resources) = imp.resources.borrow().clone() else {
            return;
        };
        imp.pending.borrow_mut().insert(id.clone());
        let generation = imp.texture_generation.get();
        let weak: glib::SendWeakRef<Self> = self.downgrade().into();
        image_cache::spawn_decode(resources, id, move |id, decoded| {
            glib::MainContext::default().invoke(move || {
                if let Some(canvas) = weak.upgrade() {
                    canvas.finish_texture(generation, id, decoded);
                }
            });
        });
    }

    fn math_shape(&self, span: &MathSpan, style: &TextStyle) -> Option<(MathKey, MathSize)> {
        if span.diagnostic.is_some() {
            return None;
        }
        let default_color = color_from_rgba(self.default_text_color());
        let key = MathKey::new(span, style, default_color, self.zoom())?;
        if let Some(entry) = self.imp().math_textures.borrow().get(&key) {
            return Some((key, entry.size));
        }
        let estimate = key.estimated_size();
        if !self.imp().math_errors.borrow().contains_key(&key) {
            self.request_math(key.clone(), span.expression.as_ref()?.clone());
        }
        Some((key, estimate))
    }

    fn math_texture(&self, key: &MathKey) -> Option<gdk::Texture> {
        self.imp()
            .math_textures
            .borrow()
            .get(key)
            .map(|entry| entry.texture.clone())
    }

    fn request_math(&self, key: MathKey, expression: onenote_core::MathExpression) {
        let imp = self.imp();
        if imp.math_pending.borrow().contains(&key)
            || imp.math_textures.borrow().contains_key(&key)
            || imp.math_errors.borrow().contains_key(&key)
        {
            return;
        }
        imp.math_pending.borrow_mut().insert(key.clone());
        let weak: glib::SendWeakRef<Self> = self.downgrade().into();
        let backend = imp.math_backend.borrow().clone();
        let generation = imp.math_generation.get();
        math_cache::spawn_render(backend, key, expression, move |key, rendered| {
            glib::MainContext::default().invoke(move || {
                if let Some(canvas) = weak.upgrade() {
                    canvas.finish_math(generation, key, rendered);
                }
            });
        });
    }

    fn finish_math(
        &self,
        generation: u64,
        key: MathKey,
        rendered: Result<onenote_render::MathRaster, String>,
    ) {
        const MAX_MATH_TEXTURE_CACHE_BYTES: usize = 128 * 1024 * 1024;

        let imp = self.imp();
        if generation != imp.math_generation.get() {
            return;
        }
        imp.math_pending.borrow_mut().remove(&key);
        let raster = match rendered {
            Ok(raster) => raster,
            Err(error) => {
                imp.math_errors.borrow_mut().insert(key, error);
                self.invalidate_text_geometry();
                self.queue_resize();
                self.queue_draw();
                return;
            }
        };
        let bytes = raster.rgba.len();
        if bytes > MAX_MATH_TEXTURE_CACHE_BYTES {
            imp.math_errors
                .borrow_mut()
                .insert(key, "math texture exceeds cache limit".to_owned());
            self.invalidate_text_geometry();
            self.queue_resize();
            self.queue_draw();
            return;
        }
        if imp.math_texture_bytes.get().saturating_add(bytes) > MAX_MATH_TEXTURE_CACHE_BYTES {
            imp.math_textures.borrow_mut().clear();
            imp.math_texture_bytes.set(0);
        }
        let width = i32::try_from(raster.width).unwrap_or(i32::MAX);
        let height = i32::try_from(raster.height).unwrap_or(i32::MAX);
        let stride = usize::try_from(width).unwrap_or(0) * 4;
        let owned = glib::Bytes::from_owned(raster.rgba);
        let texture =
            gdk::MemoryTexture::new(width, height, gdk::MemoryFormat::R8g8b8a8, &owned, stride)
                .upcast();
        let size = MathSize {
            width: raster.logical_width,
            height: raster.logical_height,
            baseline: raster.baseline,
        };
        imp.math_textures.borrow_mut().insert(
            key,
            imp::CachedMathTexture {
                texture,
                size,
                bytes,
            },
        );
        self.invalidate_text_geometry();
        imp.math_texture_bytes.set(
            imp.math_textures
                .borrow()
                .values()
                .map(|entry| entry.bytes)
                .sum(),
        );
        self.queue_resize();
        self.queue_draw();
    }

    fn finish_texture(
        &self,
        generation: u64,
        id: ResourceId,
        decoded: Result<DecodedImage, ImageDecodeFailure>,
    ) {
        let imp = self.imp();
        if generation != imp.texture_generation.get() {
            return;
        }
        imp.pending.borrow_mut().remove(&id);
        let decoded = match decoded {
            Ok(decoded) => decoded,
            Err(failure) => {
                imp.failed.borrow_mut().insert(id, failure);
                self.queue_draw();
                return;
            }
        };
        let (id, texture, bytes) = image_cache::texture(decoded);
        if bytes > MAX_TEXTURE_CACHE_BYTES {
            imp.failed
                .borrow_mut()
                .insert(id, ImageDecodeFailure::CannotDisplay);
            self.queue_draw();
            return;
        }
        if imp.texture_bytes.get().saturating_add(bytes) > MAX_TEXTURE_CACHE_BYTES {
            imp.textures.borrow_mut().clear();
            imp.texture_bytes.set(0);
        }
        imp.textures
            .borrow_mut()
            .insert(id, imp::CachedTexture { texture, bytes });
        imp.texture_bytes.set(
            imp.textures
                .borrow()
                .values()
                .map(|entry| entry.bytes)
                .sum(),
        );
        self.queue_draw();
    }
}

/// Return a finite zoom scale within the renderer's supported bounds.
pub fn normalize_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        DEFAULT_ZOOM
    }
}

impl Default for PageCanvas {
    fn default() -> Self {
        Self::new()
    }
}

fn snapshot_ink_batch(
    snapshot: &gtk::Snapshot,
    bounds: Rect,
    batch: &[(&SceneNode, Rect, &[onenote_core::InkStroke])],
    erased: &[ErasedStroke],
) {
    if batch.is_empty() {
        return;
    }
    let cairo = snapshot.append_cairo(&graphene_rect(bounds));
    cairo.translate(f64::from(bounds.x), f64::from(bounds.y));
    for (node, node_bounds, strokes) in batch {
        let dx = node_bounds.x - node.bounds.x;
        let dy = node_bounds.y - node.bounds.y;
        let object_id = node.source_object_id.as_str();
        for (index, stroke) in strokes.iter().enumerate() {
            if erased
                .iter()
                .any(|entry| entry.object_id == object_id && entry.stroke_index == index)
            {
                continue;
            }
            let Some(first) = stroke.points.first() else {
                continue;
            };
            let color = stroke.color.unwrap_or(Color {
                red: 32,
                green: 31,
                blue: 30,
                alpha: 255,
            });
            cairo.set_source_rgba(
                f64::from(color.red) / 255.0,
                f64::from(color.green) / 255.0,
                f64::from(color.blue) / 255.0,
                f64::from(stroke.opacity) / 255.0,
            );
            cairo.set_line_width(f64::from(stroke.width.max(1.0)));
            cairo.set_line_cap(gtk::cairo::LineCap::Round);
            cairo.set_line_join(gtk::cairo::LineJoin::Round);
            let first_x = f64::from(first.x + dx - bounds.x);
            let first_y = f64::from(first.y + dy - bounds.y);
            cairo.move_to(first_x, first_y);
            if stroke.points.len() == 1 {
                cairo.arc(
                    first_x,
                    first_y,
                    f64::from(stroke.width.max(1.0)) / 2.0,
                    0.0,
                    std::f64::consts::TAU,
                );
                let _ignored = cairo.fill();
                continue;
            }
            for point in &stroke.points[1..] {
                cairo.line_to(
                    f64::from(point.x + dx - bounds.x),
                    f64::from(point.y + dy - bounds.y),
                );
            }
            let _ignored = cairo.stroke();
        }
    }
}

fn snapshot_ink(
    snapshot: &gtk::Snapshot,
    bounds: Rect,
    strokes: &[onenote_core::InkStroke],
    erased: &[ErasedStroke],
    object_id: &str,
) {
    if strokes.is_empty() {
        return;
    }
    let cairo = snapshot.append_cairo(&graphene_rect(bounds));
    cairo.translate(f64::from(bounds.x), f64::from(bounds.y));
    for (index, stroke) in strokes.iter().enumerate() {
        if erased
            .iter()
            .any(|entry| entry.object_id == object_id && entry.stroke_index == index)
        {
            continue;
        }
        let Some(first) = stroke.points.first() else {
            continue;
        };
        let color = stroke.color.unwrap_or(Color {
            red: 32,
            green: 31,
            blue: 30,
            alpha: 255,
        });
        cairo.set_source_rgba(
            f64::from(color.red) / 255.0,
            f64::from(color.green) / 255.0,
            f64::from(color.blue) / 255.0,
            f64::from(stroke.opacity) / 255.0,
        );
        cairo.set_line_width(f64::from(stroke.width.max(1.0)));
        cairo.set_line_cap(gtk::cairo::LineCap::Round);
        cairo.set_line_join(gtk::cairo::LineJoin::Round);
        let first_x = f64::from(first.x - bounds.x);
        let first_y = f64::from(first.y - bounds.y);
        cairo.move_to(first_x, first_y);
        if stroke.points.len() == 1 {
            cairo.arc(
                first_x,
                first_y,
                f64::from(stroke.width.max(1.0)) / 2.0,
                0.0,
                std::f64::consts::TAU,
            );
            let _ignored = cairo.fill();
            continue;
        }
        for point in &stroke.points[1..] {
            cairo.line_to(
                f64::from(point.x - bounds.x),
                f64::from(point.y - bounds.y),
            );
        }
        let _ignored = cairo.stroke();
    }
}

fn snapshot_editable_ink(
    snapshot: &gtk::Snapshot,
    bounds: Rect,
    strokes: &[EditableStroke],
    pending: Option<&EditableStroke>,
) {
    if strokes.is_empty() && pending.is_none() {
        return;
    }
    let cairo = snapshot.append_cairo(&graphene_rect(bounds));
    cairo.translate(f64::from(bounds.x), f64::from(bounds.y));
    for stroke in strokes.iter().chain(pending) {
        let Some(first) = stroke.points.first() else {
            continue;
        };
        cairo.set_source_rgba(
            f64::from(stroke.color.red) / 255.0,
            f64::from(stroke.color.green) / 255.0,
            f64::from(stroke.color.blue) / 255.0,
            f64::from(stroke.color.alpha) / 255.0,
        );
        cairo.set_line_width(f64::from(stroke.width.max(1.0)));
        cairo.set_line_cap(gtk::cairo::LineCap::Round);
        cairo.set_line_join(gtk::cairo::LineJoin::Round);
        let x = f64::from(first.x - bounds.x);
        let y = f64::from(first.y - bounds.y);
        cairo.move_to(x, y);
        if stroke.points.len() == 1 {
            cairo.arc(
                x,
                y,
                f64::from(stroke.width.max(1.0)) / 2.0,
                0.0,
                std::f64::consts::TAU,
            );
            let _ignored = cairo.fill();
            continue;
        }
        for point in &stroke.points[1..] {
            cairo.line_to(f64::from(point.x - bounds.x), f64::from(point.y - bounds.y));
        }
        let _ignored = cairo.stroke();
    }
}

fn snapshot_line(
    snapshot: &gtk::Snapshot,
    authored_bounds: Rect,
    resolved_bounds: Rect,
    to_x: f32,
    to_y: f32,
    color: Color,
    width: f32,
) {
    let cairo = snapshot.append_cairo(&graphene_rect(resolved_bounds));
    cairo.translate(f64::from(resolved_bounds.x), f64::from(resolved_bounds.y));
    cairo.set_source_rgba(
        f64::from(color.red) / 255.0,
        f64::from(color.green) / 255.0,
        f64::from(color.blue) / 255.0,
        f64::from(color.alpha) / 255.0,
    );
    cairo.set_line_width(f64::from(width.max(1.0)));
    cairo.move_to(0.0, 0.0);
    cairo.line_to(
        f64::from(to_x - authored_bounds.x),
        f64::from(to_y - authored_bounds.y),
    );
    let _ignored = cairo.stroke();
}

fn snapshot_attachment_border(
    snapshot: &gtk::Snapshot,
    bounds: Rect,
    color: gdk::RGBA,
    width: f64,
) {
    let cairo = snapshot.append_cairo(&graphene_rect(bounds));
    cairo.translate(f64::from(bounds.x), f64::from(bounds.y));
    cairo.set_source_rgba(
        f64::from(color.red()),
        f64::from(color.green()),
        f64::from(color.blue()),
        f64::from(color.alpha()),
    );
    cairo.set_line_width(width);
    let inset = width / 2.0;
    cairo.rectangle(
        inset,
        inset,
        (f64::from(bounds.width) - width).max(0.0),
        (f64::from(bounds.height) - width).max(0.0),
    );
    let _ignored = cairo.stroke();
}

fn snapshot_file_icon(snapshot: &gtk::Snapshot, bounds: Rect, color: gdk::RGBA) {
    let icon_size = bounds.height.min(24.0).min(bounds.width).max(1.0);
    let scale = f64::from(icon_size / 24.0);
    let cairo = snapshot.append_cairo(&graphene_rect(bounds));
    cairo.translate(
        f64::from(bounds.x) + 4.0,
        f64::from(bounds.y) + f64::from((bounds.height - icon_size) / 2.0),
    );
    cairo.scale(scale, scale);
    cairo.set_source_rgba(
        f64::from(color.red()),
        f64::from(color.green()),
        f64::from(color.blue()),
        f64::from(color.alpha()),
    );
    cairo.set_line_width(2.0);
    cairo.set_line_cap(gtk::cairo::LineCap::Round);
    cairo.set_line_join(gtk::cairo::LineJoin::Round);
    // Lucide "file" icon geometry, rendered directly by the snapshot canvas.
    cairo.move_to(14.0, 2.0);
    cairo.line_to(6.0, 2.0);
    cairo.curve_to(4.9, 2.0, 4.0, 2.9, 4.0, 4.0);
    cairo.line_to(4.0, 20.0);
    cairo.curve_to(4.0, 21.1, 4.9, 22.0, 6.0, 22.0);
    cairo.line_to(18.0, 22.0);
    cairo.curve_to(19.1, 22.0, 20.0, 21.1, 20.0, 20.0);
    cairo.line_to(20.0, 8.0);
    cairo.close_path();
    let _ignored = cairo.stroke();
    cairo.move_to(14.0, 2.0);
    cairo.line_to(14.0, 8.0);
    cairo.line_to(20.0, 8.0);
    let _ignored = cairo.stroke();
}

fn perceived_lightness(color: gdk::RGBA) -> f32 {
    color.red() * 0.2126 + color.green() * 0.7152 + color.blue() * 0.0722
}

fn image_failure_label(
    primary: ImageDecodeFailure,
    fallback: Option<ImageDecodeFailure>,
) -> &'static str {
    if primary == ImageDecodeFailure::Unavailable
        && fallback.is_none_or(|failure| failure == ImageDecodeFailure::Unavailable)
    {
        "Image unavailable"
    } else {
        "Image cannot be displayed"
    }
}


fn point_segment_distance(
    point: onenote_core::InkPoint,
    start: onenote_core::InkPoint,
    end: onenote_core::InkPoint,
) -> f32 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length_squared = dx * dx + dy * dy;
    if length_squared <= f32::EPSILON {
        return ((point.x - start.x).powi(2) + (point.y - start.y).powi(2)).sqrt();
    }
    let projection =
        (((point.x - start.x) * dx + (point.y - start.y) * dy) / length_squared).clamp(0.0, 1.0);
    let nearest_x = start.x + projection * dx;
    let nearest_y = start.y + projection * dy;
    ((point.x - nearest_x).powi(2) + (point.y - nearest_y).powi(2)).sqrt()
}

fn rgba(color: Color) -> gdk::RGBA {
    gdk::RGBA::new(
        f32::from(color.red) / 255.0,
        f32::from(color.green) / 255.0,
        f32::from(color.blue) / 255.0,
        f32::from(color.alpha) / 255.0,
    )
}

fn color_from_rgba(color: gdk::RGBA) -> Color {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round().to_u8().unwrap_or(0);
    Color {
        red: channel(color.red()),
        green: channel(color.green()),
        blue: channel(color.blue()),
        alpha: channel(color.alpha()),
    }
}

fn graphene_rect(rect: Rect) -> graphene::Rect {
    graphene::Rect::new(rect.x, rect.y, rect.width, rect.height)
}

fn translate_rect(rect: Rect, x: f32, y: f32) -> Rect {
    Rect {
        x: rect.x + x,
        y: rect.y + y,
        ..rect
    }
}

fn append_metadata_match(
    matches: &mut Vec<FindMatch>,
    limit: usize,
    bounds: Rect,
    values: [Option<&str>; 4],
    query: &str,
    options: FindOptions,
) {
    if matches.len() < limit
        && values
            .into_iter()
            .flatten()
            .any(|value| !crate::find_text_ranges(value, query, options, 1).is_empty())
    {
        matches.push(FindMatch {
            bounds: vec![bounds],
        });
    }
}

fn rects_intersect(left: Rect, right: Rect) -> bool {
    left.x < right.x + right.width
        && left.x + left.width > right.x
        && left.y < right.y + right.height
        && left.y + left.height > right.y
}

fn contains(rect: Rect, x: f32, y: f32) -> bool {
    x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
}

fn next_attachment_index(current: Option<usize>, len: usize, backwards: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match current {
        None => Some(if backwards { len - 1 } else { 0 }),
        Some(0) if backwards => None,
        Some(index) if !backwards && index + 1 >= len => None,
        Some(index) if backwards => Some(index - 1),
        Some(index) => Some(index + 1),
    }
}

fn reveal_adjustment(adjustment: &gtk::Adjustment, start: f64, extent: f64) {
    let end = start + extent;
    let visible_start = adjustment.value();
    let visible_end = visible_start + adjustment.page_size();
    if start < visible_start {
        adjustment.set_value(start);
    } else if end > visible_end {
        adjustment.set_value(end - adjustment.page_size());
    }
}

fn to_pango_units(value: f32) -> i32 {
    finite_f32_to_i32(value * 1_024.0)
}

fn finite_f32_to_i32(value: f32) -> i32 {
    if value.is_nan() {
        0
    } else if value.is_sign_negative() {
        value.to_i32().unwrap_or(i32::MIN)
    } else {
        value.to_i32().unwrap_or(i32::MAX)
    }
}

fn f64_to_f32(value: f64) -> f32 {
    if value.is_nan() {
        0.0
    } else if value.is_sign_negative() {
        value.to_f32().unwrap_or(f32::MIN)
    } else {
        value.to_f32().unwrap_or(f32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        image_failure_label, next_attachment_index, normalize_zoom, ImageDecodeFailure, PageCanvas,
        DEFAULT_ZOOM, MAX_ZOOM, MIN_ZOOM,
    };
    use crate::FindOptions;
    use gtk::prelude::*;
    use gtk::subclass::prelude::ObjectSubclassIsExt;
    use onenote_core::{
        Attachment, ObjectId, PageId, Rect, ResourceId, ResourceRef, ResourceStatus,
    };
    use onenote_render::{
        AccessibilityRole, AccessibilitySemantics, HitAction, HitRegion, PageScene, SceneNode,
        SceneNodeId, ScenePrimitive,
    };
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    #[test]
    fn zoom_normalization_is_finite_and_bounded() {
        assert_zoom_eq(normalize_zoom(0.1), MIN_ZOOM);
        assert_zoom_eq(normalize_zoom(10.0), MAX_ZOOM);
        assert_zoom_eq(normalize_zoom(f32::NAN), DEFAULT_ZOOM);
        assert_zoom_eq(normalize_zoom(f32::INFINITY), DEFAULT_ZOOM);
        assert_zoom_eq(normalize_zoom(1.21), 1.21);
    }

    #[test]
    fn attachment_focus_enters_moves_and_leaves_at_boundaries() {
        assert_eq!(next_attachment_index(None, 3, false), Some(0));
        assert_eq!(next_attachment_index(Some(0), 3, false), Some(1));
        assert_eq!(next_attachment_index(Some(2), 3, false), None);
        assert_eq!(next_attachment_index(None, 3, true), Some(2));
        assert_eq!(next_attachment_index(Some(2), 3, true), Some(1));
        assert_eq!(next_attachment_index(Some(0), 3, true), None);
        assert_eq!(next_attachment_index(None, 0, false), None);
    }

    #[test]
    fn image_failure_labels_distinguish_missing_and_undecodable_data() {
        assert_eq!(
            image_failure_label(ImageDecodeFailure::Unavailable, None),
            "Image unavailable"
        );
        assert_eq!(
            image_failure_label(
                ImageDecodeFailure::Unavailable,
                Some(ImageDecodeFailure::Unavailable)
            ),
            "Image unavailable"
        );
        assert_eq!(
            image_failure_label(
                ImageDecodeFailure::CannotDisplay,
                Some(ImageDecodeFailure::Unavailable)
            ),
            "Image cannot be displayed"
        );
    }

    #[test]
    fn attachment_pointer_and_click_dispatch_the_resource_action() {
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return;
        }
        if gtk::init().is_err() {
            return;
        }
        let resource_id = ResourceId::new("attachment");
        let node_id = SceneNodeId("node".to_owned());
        let object_id = ObjectId::new("object");
        let bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 180.0,
            height: 44.0,
        };
        let scene = PageScene {
            page_id: PageId::new("page"),
            bounds,
            nodes: vec![SceneNode {
                id: node_id.clone(),
                source_object_id: object_id.clone(),
                bounds,
                flow_path: Vec::new(),
                z_index: 0,
                primitive: ScenePrimitive::Attachment(Attachment {
                    resource: ResourceRef {
                        id: resource_id.clone(),
                        name: "manual\0.pdf".to_owned(),
                        media_type: "application/pdf".to_owned(),
                        size: 12,
                        status: ResourceStatus::Available,
                    },
                    icon: None,
                    width: None,
                    height: None,
                }),
                accessibility: AccessibilitySemantics {
                    role: AccessibilityRole::Attachment,
                    label: "manual.pdf".to_owned(),
                    description: None,
                },
            }],
            hit_regions: vec![HitRegion {
                node_id,
                source_object_id: object_id,
                bounds,
                action: HitAction::OpenAttachment(resource_id.clone()),
            }],
            diagnostics: Vec::new(),
        };
        let canvas = PageCanvas::new();
        canvas.set_scene(Some(Arc::new(scene)));
        let attachment_matches = canvas.find("MANUAL", FindOptions::default(), 10);
        assert_eq!(attachment_matches.len(), 1);
        assert_eq!(attachment_matches[0].bounds, vec![bounds]);
        canvas.set_find_highlights(attachment_matches, Some(0), true);
        for color in [gtk::gdk::RGBA::BLACK, gtk::gdk::RGBA::WHITE] {
            canvas.set_default_text_color(&color);
            let snapshot = gtk::Snapshot::new();
            canvas.snapshot_scene(&snapshot);
            assert!(snapshot.to_node().is_some());
        }
        let dispatched = Rc::new(RefCell::new(Vec::new()));
        let dispatched_from_handler = Rc::clone(&dispatched);
        canvas.set_action_handler(Some(move |action| {
            dispatched_from_handler.borrow_mut().push(action);
        }));

        canvas.update_pointer_at(40.0, 40.0);
        assert_eq!(canvas.tooltip_text().as_deref(), Some("manual�.pdf"));
        canvas.activate_at(40.0, 40.0);

        assert_eq!(
            dispatched.borrow().as_slice(),
            &[HitAction::OpenAttachment(resource_id)]
        );
        assert_eq!(canvas.imp().focused_attachment.get(), Some(0));
        assert!(canvas.activate_focused_attachment());
        assert_eq!(dispatched.borrow().len(), 2);
    }

    fn assert_zoom_eq(actual: f32, expected: f32) {
        assert!((actual - expected).abs() <= f32::EPSILON);
    }
}
