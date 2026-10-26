use super::interaction::{CanvasInteraction, LockedHidden, SnapTolerance};
use super::render::{RenderBackend, RenderBackendError, RenderCommand, RenderCommandEncoder};
use crate::types::Viewport;
use slides_api::{Point, Rect};

/// Host boundary between canvas and editor/app layers.
///
/// The host owns document/editor state. Canvas owns only transient viewport and
/// interaction state and sends editor commands back through this boundary.
pub trait CanvasHost {
    fn viewport(&self) -> Viewport;
    fn apply_viewport(&mut self, viewport: Viewport);
    fn visible_bounds(&self) -> Rect;
    fn locked_hidden_state(&self, element_id: u64) -> LockedHidden;
    fn request_editor_command(
        &mut self,
        command: CanvasInteraction,
    ) -> Result<(), RenderBackendError>;
}

/// Headless canvas layer.
pub struct CanvasLayer<Backend, Host>
where
    Backend: RenderBackend,
    Host: CanvasHost,
{
    pub backend: Backend,
    pub host: Host,
    pub snap: SnapTolerance,
}

impl<Backend, Host> CanvasLayer<Backend, Host>
where
    Backend: RenderBackend,
    Host: CanvasHost,
{
    pub fn new(backend: Backend, host: Host, snap: SnapTolerance) -> Self {
        Self {
            backend,
            host,
            snap,
        }
    }

    pub fn render(&self) -> Result<Vec<RenderCommand>, RenderBackendError> {
        let mut encoder = self.backend.create_encoder();
        encoder.encode_clear([0.12, 0.12, 0.14, 1.0]);
        self.backend.present(encoder)
    }

    pub fn viewport(&self) -> Viewport {
        self.host.viewport()
    }

    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.host.apply_viewport(viewport.clamped());
    }

    pub fn set_viewport_clamped(&mut self, viewport: Viewport, content_bounds: Rect) {
        let viewport_bounds = self.host.visible_bounds();
        self.host.apply_viewport(
            viewport
                .clamped()
                .clamp_pan(content_bounds, viewport_bounds),
        );
    }

    pub fn pan(&mut self, delta: Point) {
        if !delta.x.is_finite() || !delta.y.is_finite() {
            return;
        }
        self.set_viewport(self.host.viewport().pan_by(delta));
    }

    pub fn pan_clamped(&mut self, delta: Point, content_bounds: Rect) {
        if !delta.x.is_finite() || !delta.y.is_finite() {
            return;
        }
        let viewport = self.host.viewport().pan_by(delta);
        self.set_viewport_clamped(viewport, content_bounds);
    }

    pub fn zoom(&mut self, scale: f32) {
        if !scale.is_finite() || scale <= 0.0 {
            return;
        }
        self.set_viewport(self.host.viewport().with_zoom(scale));
    }

    pub fn dispatch_interaction(
        &mut self,
        mut interaction: CanvasInteraction,
    ) -> Result<(), RenderBackendError> {
        if !interaction.point.x.is_finite() || !interaction.point.y.is_finite() {
            return Ok(());
        }
        interaction.locked_hidden = self.host.locked_hidden_state(interaction.element_id);
        if interaction.is_dispatchable() {
            self.host.request_editor_command(interaction)
        } else {
            Ok(())
        }
    }
}
