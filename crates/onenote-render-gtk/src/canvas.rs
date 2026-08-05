use crate::image_cache::{self, DecodedImage, MAX_TEXTURE_CACHE_BYTES};
use crate::math_cache::{self, MathKey, MathSize, TypstMathBackend};
use crate::resolved_layout::ResolvedLayout;
use crate::text;
use gtk::gdk;
use gtk::glib;
use gtk::graphene;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use num_traits::ToPrimitive;
use onenote_core::{Color, MathSpan, Rect, ResourceId, ResourceStore, TextStyle};
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

mod imp {
    use super::{
        gdk, glib, ActionHandler, Arc, Cell, HashMap, HashSet, MathKey, MathLayoutBackend,
        OnceLock, PageScene, Rc, RefCell, ResolvedLayout, ResourceId, ResourceStore, SceneNodeId,
        TypstMathBackend, CANVAS_MARGIN, DEFAULT_ZOOM, MAX_ZOOM, MIN_ZOOM,
    };
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    pub struct PageCanvas {
        pub(super) scene: RefCell<Option<Arc<PageScene>>>,
        pub(super) resources: RefCell<Option<Arc<ResourceStore>>>,
        pub(super) zoom: Cell<f32>,
        pub(super) default_text_color: RefCell<gdk::RGBA>,
        pub(super) action_handler: RefCell<Option<ActionHandler>>,
        pub(super) text_layouts: RefCell<HashMap<SceneNodeId, Rc<crate::text::TextLayout>>>,
        pub(super) resolved_layout: RefCell<Option<CachedResolvedLayout>>,
        pub(super) layout_generation: Cell<u64>,
        pub(super) textures: RefCell<HashMap<ResourceId, CachedTexture>>,
        pub(super) pending: RefCell<HashSet<ResourceId>>,
        pub(super) failed: RefCell<HashSet<ResourceId>>,
        pub(super) texture_bytes: Cell<usize>,
        pub(super) math_textures: RefCell<HashMap<MathKey, CachedMathTexture>>,
        pub(super) math_pending: RefCell<HashSet<MathKey>>,
        pub(super) math_errors: RefCell<HashMap<MathKey, String>>,
        pub(super) math_texture_bytes: Cell<usize>,
        pub(super) math_backend: RefCell<Arc<dyn MathLayoutBackend>>,
        pub(super) math_generation: Cell<u64>,
    }

    impl Default for PageCanvas {
        fn default() -> Self {
            Self {
                scene: RefCell::default(),
                resources: RefCell::default(),
                zoom: Cell::new(DEFAULT_ZOOM),
                default_text_color: RefCell::new(gdk::RGBA::BLACK),
                action_handler: RefCell::default(),
                text_layouts: RefCell::default(),
                resolved_layout: RefCell::default(),
                layout_generation: Cell::default(),
                textures: RefCell::default(),
                pending: RefCell::default(),
                failed: RefCell::default(),
                texture_bytes: Cell::default(),
                math_textures: RefCell::default(),
                math_pending: RefCell::default(),
                math_errors: RefCell::default(),
                math_texture_bytes: Cell::default(),
                math_backend: RefCell::new(Arc::new(TypstMathBackend::new())),
                math_generation: Cell::default(),
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
                }
            });
            object.add_controller(motion);
        }
    }

    impl WidgetImpl for PageCanvas {
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
        self.invalidate_text_geometry();
        imp.textures.borrow_mut().clear();
        imp.pending.borrow_mut().clear();
        imp.failed.borrow_mut().clear();
        imp.texture_bytes.set(0);
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

    /// Supply the lazy resources belonging to the scene's loaded notebook.
    pub fn set_resources(&self, resources: Option<Arc<ResourceStore>>) {
        *self.imp().resources.borrow_mut() = resources;
        self.imp().textures.borrow_mut().clear();
        self.imp().pending.borrow_mut().clear();
        self.imp().failed.borrow_mut().clear();
        self.imp().texture_bytes.set(0);
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
        if let (Some(action), Some(handler)) = (action, self.imp().action_handler.borrow().as_ref())
        {
            handler(action);
        }
    }

    fn update_pointer_at(&self, x: f64, y: f64) {
        if let Some(HitAction::OpenLink(target)) = self.action_at(x, y) {
            self.set_cursor_from_name(Some("pointer"));
            self.set_tooltip_text(Some(&target));
        } else {
            self.set_cursor_from_name(None);
            self.set_tooltip_text(None);
        }
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
        snapshot.restore();
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
            ScenePrimitive::Image(image) => {
                if let Some(texture) = self.texture(&image.resource.id) {
                    snapshot.append_texture(&texture, &graphene_rect(node_bounds));
                } else if self.imp().failed.borrow().contains(&image.resource.id) {
                    snapshot.append_color(
                        &gdk::RGBA::new(1.0, 0.92, 0.92, 1.0),
                        &graphene_rect(node_bounds),
                    );
                    self.snapshot_label(
                        snapshot,
                        node_bounds,
                        "Image unavailable",
                        gdk::RGBA::new(0.55, 0.08, 0.08, 1.0),
                    );
                } else {
                    snapshot.append_color(
                        &gdk::RGBA::new(0.94, 0.94, 0.94, 1.0),
                        &graphene_rect(node_bounds),
                    );
                    self.request_texture(image.resource.id.clone());
                }
            }
            ScenePrimitive::Attachment(attachment) => {
                snapshot.append_color(
                    &gdk::RGBA::new(0.95, 0.96, 0.98, 1.0),
                    &graphene_rect(node_bounds),
                );
                self.snapshot_label(
                    snapshot,
                    node_bounds,
                    &attachment.resource.name,
                    gdk::RGBA::new(0.12, 0.25, 0.45, 1.0),
                );
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

    fn request_texture(&self, id: ResourceId) {
        let imp = self.imp();
        if imp.failed.borrow().contains(&id)
            || imp.pending.borrow().contains(&id)
            || imp.textures.borrow().contains_key(&id)
        {
            return;
        }
        let Some(resources) = imp.resources.borrow().clone() else {
            return;
        };
        imp.pending.borrow_mut().insert(id.clone());
        let weak: glib::SendWeakRef<Self> = self.downgrade().into();
        image_cache::spawn_decode(resources, id, move |id, decoded| {
            glib::MainContext::default().invoke(move || {
                if let Some(canvas) = weak.upgrade() {
                    canvas.finish_texture(id, decoded);
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

    fn finish_texture(&self, id: ResourceId, decoded: Option<DecodedImage>) {
        let imp = self.imp();
        imp.pending.borrow_mut().remove(&id);
        let Some(decoded) = decoded else {
            imp.failed.borrow_mut().insert(id);
            self.queue_draw();
            return;
        };
        let (id, texture, bytes) = image_cache::texture(decoded);
        if bytes > MAX_TEXTURE_CACHE_BYTES {
            imp.failed.borrow_mut().insert(id);
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

fn contains(rect: Rect, x: f32, y: f32) -> bool {
    x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
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
    use super::{normalize_zoom, DEFAULT_ZOOM, MAX_ZOOM, MIN_ZOOM};

    #[test]
    fn zoom_normalization_is_finite_and_bounded() {
        assert_zoom_eq(normalize_zoom(0.1), MIN_ZOOM);
        assert_zoom_eq(normalize_zoom(10.0), MAX_ZOOM);
        assert_zoom_eq(normalize_zoom(f32::NAN), DEFAULT_ZOOM);
        assert_zoom_eq(normalize_zoom(f32::INFINITY), DEFAULT_ZOOM);
        assert_zoom_eq(normalize_zoom(1.21), 1.21);
    }

    fn assert_zoom_eq(actual: f32, expected: f32) {
        assert!((actual - expected).abs() <= f32::EPSILON);
    }
}
