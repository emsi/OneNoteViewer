//! Embeddable GTK renderer for UI-neutral `OneNote` page scenes.

#![forbid(unsafe_code)]

mod canvas;
mod image_cache;
mod math_cache;
mod text;
mod view;

pub use canvas::{normalize_zoom, PageCanvas, DEFAULT_ZOOM, MAX_ZOOM, MIN_ZOOM};
pub use math_cache::TypstMathBackend;
pub use view::PageView;

/// The crate API version during the pre-1.0 implementation phase.
pub const API_VERSION: u32 = onenote_render::API_VERSION;
