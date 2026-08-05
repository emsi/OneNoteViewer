use crate::{normalize_zoom, PageCanvas};
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use onenote_core::{ObjectId, Rect, ResourceStore};
use onenote_render::{HitAction, PageScene};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

/// Embeddable scrolled page view with pan, zoom, culling, and hit dispatch.
#[derive(Clone)]
pub struct PageView {
    root: gtk::ScrolledWindow,
    canvas: PageCanvas,
}

impl PageView {
    /// Construct an empty view. GTK must already be initialized.
    pub fn new() -> Self {
        let canvas = PageCanvas::new();
        canvas.set_hexpand(true);
        canvas.set_vexpand(true);
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .kinetic_scrolling(true)
            .overlay_scrolling(true)
            .child(&canvas)
            .build();
        install_pan(&canvas, &root);
        install_zoom(&canvas, &root);
        Self { root, canvas }
    }

    /// Root widget to embed in a host layout.
    pub fn widget(&self) -> &gtk::ScrolledWindow {
        &self.root
    }

    /// Underlying retained canvas.
    pub fn canvas(&self) -> &PageCanvas {
        &self.canvas
    }

    /// Replace the current scene.
    pub fn set_scene(&self, scene: Option<Arc<PageScene>>) {
        self.canvas.set_scene(scene);
        self.root.hadjustment().set_value(0.0);
        self.root.vadjustment().set_value(0.0);
    }

    /// Supply the loaded notebook's lazy resource store.
    pub fn set_resources(&self, resources: Option<Arc<ResourceStore>>) {
        self.canvas.set_resources(resources);
    }

    /// Set the host callback for inert scene actions.
    pub fn set_action_handler(&self, handler: Option<impl Fn(HitAction) + 'static>) {
        self.canvas.set_action_handler(handler);
    }

    /// Set zoom while preserving the viewport center.
    pub fn set_zoom(&self, zoom: f32) {
        let old_zoom = self.canvas.zoom();
        let zoom = normalize_zoom(zoom);
        if (zoom - old_zoom).abs() <= f32::EPSILON {
            return;
        }
        let horizontal = self.root.hadjustment();
        let vertical = self.root.vadjustment();
        let center_x = horizontal.value() + horizontal.page_size() / 2.0;
        let center_y = vertical.value() + vertical.page_size() / 2.0;
        let ratio = f64::from(zoom / old_zoom);
        self.canvas.set_zoom(zoom);
        horizontal.set_value(center_x * ratio - horizontal.page_size() / 2.0);
        vertical.set_value(center_y * ratio - vertical.page_size() / 2.0);
    }

    /// Current zoom.
    pub fn zoom(&self) -> f32 {
        self.canvas.zoom()
    }

    /// Connect a callback that runs after the effective zoom changes.
    ///
    /// The callback observes every zoom source, including the built-in
    /// Ctrl+wheel gesture and host calls to [`Self::set_zoom`].
    pub fn connect_zoom_changed(&self, callback: impl Fn(f32) + 'static) -> glib::SignalHandlerId {
        self.canvas
            .connect_notify_local(Some("zoom"), move |canvas, _| callback(canvas.zoom()))
    }

    /// Set the fallback for text whose `OneNote` style uses automatic color.
    ///
    /// This lets embedding applications adapt automatic text to their page
    /// surface without changing explicit colors stored in the notebook.
    pub fn set_default_text_color(&self, color: &gdk::RGBA) {
        self.canvas.set_default_text_color(color);
    }

    /// Current fallback for text whose `OneNote` style uses automatic color.
    pub fn default_text_color(&self) -> gdk::RGBA {
        self.canvas.default_text_color()
    }

    /// Scroll a logical object rectangle into view.
    pub fn reveal(&self, bounds: Rect) {
        let Some(scene) = self.canvas.scene() else {
            return;
        };
        let scene_bounds = self.canvas.resolved_scene_bounds().unwrap_or(scene.bounds);
        let zoom = f64::from(self.zoom());
        let x = f64::from(bounds.x - scene_bounds.x + 32.0) * zoom;
        let y = f64::from(bounds.y - scene_bounds.y + 32.0) * zoom;
        reveal_adjustment(&self.root.hadjustment(), x, f64::from(bounds.width) * zoom);
        reveal_adjustment(&self.root.vadjustment(), y, f64::from(bounds.height) * zoom);
    }

    /// Scroll a source object into view using its adapter-resolved geometry.
    pub fn reveal_source_object(&self, source: &ObjectId, fallback: Rect) {
        let bounds = self
            .canvas
            .resolved_source_bounds(source)
            .unwrap_or(fallback);
        self.reveal(bounds);
    }
}

impl Default for PageView {
    fn default() -> Self {
        Self::new()
    }
}

fn install_pan(canvas: &PageCanvas, root: &gtk::ScrolledWindow) {
    let drag = gtk::GestureDrag::new();
    drag.set_button(gdk::BUTTON_MIDDLE);
    let start = Rc::new(Cell::new((0.0, 0.0)));
    let start_begin = Rc::clone(&start);
    let horizontal = root.hadjustment();
    let vertical = root.vadjustment();
    drag.connect_drag_begin(move |_, _, _| {
        start_begin.set((horizontal.value(), vertical.value()));
    });
    let horizontal = root.hadjustment();
    let vertical = root.vadjustment();
    drag.connect_drag_update(move |_, offset_x, offset_y| {
        let (start_x, start_y) = start.get();
        horizontal.set_value(start_x - offset_x);
        vertical.set_value(start_y - offset_y);
    });
    canvas.add_controller(drag);
}

fn install_zoom(canvas: &PageCanvas, root: &gtk::ScrolledWindow) {
    let scroll = gtk::EventControllerScroll::new(
        gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::DISCRETE,
    );
    let controller_canvas = canvas.clone();
    let root = root.clone();
    scroll.connect_scroll(move |controller, _, delta_y| {
        if !controller
            .current_event_state()
            .contains(gdk::ModifierType::CONTROL_MASK)
        {
            return glib::Propagation::Proceed;
        }
        let view = PageView {
            root: root.clone(),
            canvas: controller_canvas.clone(),
        };
        let factor = if delta_y < 0.0 { 1.1 } else { 1.0 / 1.1 };
        view.set_zoom(view.zoom() * factor);
        glib::Propagation::Stop
    });
    canvas.add_controller(scroll);
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
