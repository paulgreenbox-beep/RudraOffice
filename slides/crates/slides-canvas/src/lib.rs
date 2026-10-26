mod canvas;
mod interaction;
mod render;
mod types;

pub use canvas::{CanvasHost, CanvasLayer};
pub use interaction::{
    BoundingBox, CanvasInteraction, DragMode, Guides, HitTest, InteractionTransition, LockedHidden,
    SelectionHandles, SnapAxis, SnapTolerance, guide, hit_test, interaction_commands, snap,
    snap_axis,
};
pub use render::{RenderBackend, RenderBackendError, RenderCommand, RenderCommandEncoder};
pub use slides_api::{Point, Rect, Transform};
pub use types::{RectExt, Viewport};
