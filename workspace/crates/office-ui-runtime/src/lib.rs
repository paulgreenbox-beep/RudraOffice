//! Transitional Rudra Office compatibility facade.
//!
//! Production implementation lives in `Rust_Makepad_Toolkit`. New consumers
//! should import `makepad-toolkit-core` and `makepad-toolkit-widgets` directly.

pub use makepad_toolkit_widgets::*;

/// Transitional name retained for the accepted Rudra Office adapter contract.
pub type OfficeFrame = DocumentFrame;

/// Transitional name retained for the accepted Rudra Office runtime contract.
pub type OfficeUiRuntime = CanvasRuntime;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_aliases_resolve_to_toolkit_types() {
        let frame = OfficeFrame::new(PageStack::new(vec![PageSurface::a4("compat")]));
        let runtime = OfficeUiRuntime::default();
        assert_eq!(frame.pages.pages.len(), 1);
        assert_eq!(runtime.zoom_percent(), 100);
    }
}
