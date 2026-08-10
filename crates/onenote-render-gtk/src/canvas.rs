use crate::image_cache::{self, DecodedImage, ImageDecodeFailure, MAX_TEXTURE_CACHE_BYTES};
use crate::math_cache::{self, MathKey, MathSize, TypstMathBackend};
use crate::resolved_layout::ResolvedLayout;
use crate::text;
use crate::{FindMatch, FindOptions};
use gtk::gdk;
use gtk::glib;
use gtk::graphene;
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

#[derive(Default)]
struct FindHighlights {
    matches: Vec<FindMatch>,
    active: Option<usize>,
    highlight_all: bool,
}

mod imp {
    use super::{
        gdk, glib, ActionHandler, Arc, Cell, FindHighlights, HashMap, HashSet, ImageDecodeFailure,
        MathKey, MathLayoutBackend, OnceLock, PageScene, Rc, RefCell, ResolvedLayout, ResourceId,
        ResourceStore, SceneNodeId, TypstMathBackend, CANVAS_MARGIN, DEFAULT_ZOOM, MAX_ZOOM,
        MIN_ZOOM,
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
            click.connect_released(move |_, _, x, y| {
                if let Some(object) = weak.upgrade() {
                    object.activate_at(x, y);
                }
            });
            object.add_controller(click);
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
            keys.connect_key_pressed(move |_, key, _, _| {
                let Some(object) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                if matches!(key, gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::space) {
                    if object.activate_focused_attachment() {
                        return glib::Propagation::Stop;
                    }
                } else if key == gdk::Key::Escape {
                    object.imp().focused_attachment.set(None);
                    object.queue_draw();
                }
                glib::Propagation::Proceed
            });
            object.add_controller(keys);
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

    fn update_pointer_at(&self, x: f64, y: f64) {
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
                self.set_cursor_from_name(None);
                self.set_tooltip_text(None);
                self.set_hovered_attachment(None);
            }
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

    fn text_layout(
        &self,
        node: &SceneNode,
        block: &onenote_core::TextBlock,
        marker: Option<&str>,
    ) -> Rc<text::TextLayout> {
        if let Some(layout) = self.imp().text_layouts.borrow().get(&node.id).cloned() {
            return layout;
        }
        let layout = Rc::new(text::layout_with_math(
            &self.pango_context(),
            block,
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
        for node in resolved.visible_nodes(&scene, viewport, VIEWPORT_OVERSCAN) {
            self.snapshot_node(snapshot, node, resolved.node_bounds(node));
        }
        self.snapshot_find_highlights(snapshot, viewport);
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
                let layout = self.text_layout(node, block, marker.as_deref());
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
            ScenePrimitive::Ink { strokes } => snapshot_ink(snapshot, node_bounds, strokes),
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

fn snapshot_ink(snapshot: &gtk::Snapshot, bounds: Rect, strokes: &[onenote_core::InkStroke]) {
    let cairo = snapshot.append_cairo(&graphene_rect(bounds));
    let Some(source_bounds) = ink_bounds(strokes) else {
        return;
    };
    let scale_x = f64::from(bounds.width / source_bounds.width.max(1.0));
    let scale_y = f64::from(bounds.height / source_bounds.height.max(1.0));
    for stroke in strokes {
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
        cairo.move_to(
            f64::from(first.x - source_bounds.x) * scale_x,
            f64::from(first.y - source_bounds.y) * scale_y,
        );
        for point in &stroke.points[1..] {
            cairo.line_to(
                f64::from(point.x - source_bounds.x) * scale_x,
                f64::from(point.y - source_bounds.y) * scale_y,
            );
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
    cairo.translate(4.0, f64::from((bounds.height - icon_size) / 2.0));
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

fn ink_bounds(strokes: &[onenote_core::InkStroke]) -> Option<Rect> {
    strokes
        .iter()
        .flat_map(|stroke| stroke.points.iter())
        .map(|point| Rect {
            x: point.x,
            y: point.y,
            width: 0.0,
            height: 0.0,
        })
        .reduce(Rect::union)
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
