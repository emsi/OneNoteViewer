//! UI-neutral `OneNote` page layout and retained scene construction.

#![forbid(unsafe_code)]

mod builder;
mod error;
mod math;
mod scene;

pub use builder::{SceneBuilder, SceneOptions};
pub use error::{Error, Result};
pub use math::{to_typst_math, MathLayoutBackend, MathLayoutRequest, MathRaster};
pub use scene::{
    AccessibilityRole, AccessibilitySemantics, HitAction, HitRegion, PageScene, SceneDiagnostic,
    SceneFlowId, SceneFlowPosition, SceneNode, SceneNodeId, ScenePrimitive,
};

/// The crate API version during the pre-1.0 implementation phase.
pub const API_VERSION: u32 = onenote_core::API_VERSION;
