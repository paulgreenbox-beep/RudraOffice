use std::fmt;

use crate::render::{build_plan, build_thumbnail_plan};
use crate::view::{RenderPlan, SlideView, ThumbnailPreview};
use crate::{
    Key, SimpleTransition, SlideshowController, SlideshowMode, SlideshowStart, ThumbnailList,
    UiError, ZoomControl,
};
use slides_api::{
    EngineError, ObjectId, Point, Presentation, PresentationEngine, Rect, Size, SlideId,
    ValidationError,
};
use slides_canvas::Viewport;

/// Public viewer errors.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewerError {
    /// The presentation has no slides.
    NoSlides,
    /// A slide index was outside the current thumbnail list.
    InvalidSlideIndex {
        /// Requested index.
        index: usize,
        /// Number of available slides.
        slide_count: usize,
    },
    /// A zoom value was not finite.
    InvalidZoom {
        /// The supplied non-finite value.
        value: f32,
    },
    /// The document failed validation.
    InvalidDocument(ValidationError),
    /// The presentation engine failed.
    Engine(EngineError),
    /// Slideshow navigation was requested while it was stopped.
    SlideshowNotRunning,
}

impl fmt::Display for ViewerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSlides => formatter.write_str("no slides"),
            Self::InvalidSlideIndex { index, slide_count } => {
                write!(
                    formatter,
                    "invalid slide index {index}; slide count is {slide_count}"
                )
            }
            Self::InvalidZoom { value } => write!(formatter, "invalid zoom {value}"),
            Self::InvalidDocument(error) => write!(formatter, "invalid document: {error}"),
            Self::Engine(error) => write!(formatter, "engine error: {error}"),
            Self::SlideshowNotRunning => formatter.write_str("slideshow is not running"),
        }
    }
}

impl std::error::Error for ViewerError {}

impl From<ValidationError> for ViewerError {
    fn from(value: ValidationError) -> Self {
        Self::InvalidDocument(value)
    }
}

impl From<UiError> for ViewerError {
    fn from(value: UiError) -> Self {
        match value {
            UiError::NoSlides => Self::NoSlides,
            UiError::InvalidSlideIndex { index, slide_count } => {
                Self::InvalidSlideIndex { index, slide_count }
            }
            UiError::InvalidZoom { value } => Self::InvalidZoom { value },
            UiError::SlideshowNotRunning => Self::SlideshowNotRunning,
            UiError::ReadOnly => Self::Engine(EngineError::InvalidInput(
                "read-only error is not applicable to PresentationViewer".to_string(),
            )),
        }
    }
}

/// Read-only presentation viewer state.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PresentationViewer {
    document: Presentation,
    thumbnails: ThumbnailList,
    slideshow: SlideshowController,
    zoom: ZoomControl,
    notes_visible: bool,
    properties_visible: bool,
    selected_object: Option<ObjectId>,
}

impl PresentationViewer {
    /// Creates a viewer from an already-cloned, validated presentation snapshot.
    pub fn from_document(document: Presentation) -> Result<Self, ViewerError> {
        if !document.slides.is_empty() {
            document.validate().map_err(ViewerError::InvalidDocument)?;
        }
        let mut viewer = Self {
            document,
            thumbnails: ThumbnailList::default(),
            slideshow: SlideshowController::default(),
            zoom: ZoomControl::default(),
            notes_visible: false,
            properties_visible: false,
            selected_object: None,
        };
        viewer.rebuild_thumbnails_and_slideshow();
        Ok(viewer)
    }

    /// Loads through an engine and maps load errors exactly to `Engine`.
    pub fn load<E: PresentationEngine + ?Sized>(
        engine: &E,
        bytes: &[u8],
    ) -> Result<Self, ViewerError> {
        let document = engine.load(bytes).map_err(ViewerError::Engine)?;
        Self::from_document(document)
    }

    /// Atomically replaces the document snapshot.
    ///
    /// Validation is performed first; on failure `self` is unchanged and
    /// `Err(ViewerError::InvalidDocument)` is returned.
    pub fn sync_from_document(&mut self, document: &Presentation) -> Result<(), ViewerError> {
        let _validated = (!document.slides.is_empty())
            .then(|| document.validate())
            .transpose()?;

        let selected_slide_id = self.selected_slide_id_for_replacement(document);

        let mut next = Self::from_document(document.clone())?;
        if let Some(slide_id) = selected_slide_id {
            let index = next
                .thumbnails
                .items
                .iter()
                .position(|item| item.slide_id == slide_id)
                .or_else(|| next.thumbnails.items.first().map(|item| item.index));
            if let Some(index) = index {
                let _ = next.thumbnails.select(index);
            }
        }

        next.zoom = self.zoom;
        next.notes_visible = self.notes_visible;
        next.properties_visible = self.properties_visible;
        next.slideshow.fullscreen = self.slideshow.fullscreen;

        *self = next;
        Ok(())
    }

    /// Returns the owned presentation snapshot.
    pub fn document(&self) -> &Presentation {
        &self.document
    }

    /// Number of slides in the snapshot.
    pub fn slide_count(&self) -> usize {
        self.document.slides.len()
    }

    /// Whether the snapshot has no slides.
    pub fn is_empty(&self) -> bool {
        self.document.slides.is_empty()
    }

    /// Zero-based currently selected slide index, if any.
    pub fn current_index(&self) -> Option<usize> {
        self.thumbnails.selected
    }

    /// Currently selected slide identifier.
    pub fn current_slide_id(&self) -> Option<SlideId> {
        self.thumbnails
            .selected
            .and_then(|index| self.thumbnails.items.get(index))
            .map(|item| item.slide_id)
    }

    /// Returns a read-only slide view for the requested index.
    pub fn slide_view(&self, index: usize) -> Option<SlideView> {
        self.document.slides.get(index).map(SlideView::from_slide)
    }

    /// Returns a read-only view of the currently selected slide.
    pub fn current_view(&self) -> Option<SlideView> {
        self.current_index()
            .and_then(|index| self.slide_view(index))
    }

    /// Returns preview metadata for all thumbnails.
    pub fn thumbnail_views(&self) -> Vec<ThumbnailPreview> {
        self.thumbnails
            .items
            .iter()
            .map(|item| {
                ThumbnailPreview::from_slide(
                    self.document
                        .slide(item.slide_id)
                        .expect("thumbnail list is built from document"),
                    item.index,
                    item.selected,
                )
            })
            .collect()
    }

    /// Notes for a slide by zero-based index.
    pub fn notes(&self, index: usize) -> Option<&str> {
        self.document
            .slides
            .get(index)
            .and_then(|slide| slide.notes.as_deref())
    }

    /// Notes for the currently selected slide.
    pub fn current_notes(&self) -> Option<&str> {
        self.current_index().and_then(|index| self.notes(index))
    }

    /// Currently selected object identifier.
    pub fn selected_object(&self) -> Option<ObjectId> {
        self.selected_object
    }

    /// Whether the notes panel is visible.
    pub fn notes_visible(&self) -> bool {
        self.notes_visible
    }

    /// Whether the properties panel is visible.
    pub fn properties_visible(&self) -> bool {
        self.properties_visible
    }

    /// Strictly selects a slide by index.
    pub fn select(&mut self, index: usize) -> Result<SlideId, ViewerError> {
        if self.is_empty() {
            return Err(ViewerError::NoSlides);
        }
        let previous_slide_id = self.current_slide_id();
        let slide_id = self.thumbnails.select(index).map_err(ViewerError::from)?;
        self.sync_selected_object(previous_slide_id);
        Ok(slide_id)
    }

    /// Selects the first slide, if any.
    pub fn first(&mut self) -> Result<SlideId, ViewerError> {
        if self.is_empty() {
            return Err(ViewerError::NoSlides);
        }
        let previous_slide_id = self.current_slide_id();
        let slide_id = self
            .thumbnails
            .select_first()
            .ok_or(ViewerError::NoSlides)?;
        self.sync_selected_object(previous_slide_id);
        Ok(slide_id)
    }

    /// Selects the previous slide, clamped at the first slide.
    pub fn previous(&mut self) -> Result<SlideId, ViewerError> {
        if self.is_empty() {
            return Err(ViewerError::NoSlides);
        }
        let previous_slide_id = self.current_slide_id();
        let slide_id = self
            .thumbnails
            .select_previous()
            .ok_or(ViewerError::NoSlides)?;
        self.sync_selected_object(previous_slide_id);
        Ok(slide_id)
    }

    /// Selects the next slide, clamped at the last slide.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<SlideId, ViewerError> {
        if self.is_empty() {
            return Err(ViewerError::NoSlides);
        }
        let previous_slide_id = self.current_slide_id();
        let slide_id = self.thumbnails.select_next().ok_or(ViewerError::NoSlides)?;
        self.sync_selected_object(previous_slide_id);
        Ok(slide_id)
    }

    /// Selects the last slide, if any.
    pub fn last(&mut self) -> Result<SlideId, ViewerError> {
        if self.is_empty() {
            return Err(ViewerError::NoSlides);
        }
        let previous_slide_id = self.current_slide_id();
        let slide_id = self.thumbnails.select_last().ok_or(ViewerError::NoSlides)?;
        self.sync_selected_object(previous_slide_id);
        Ok(slide_id)
    }

    /// Sets zoom after validating finiteness and bounds.
    pub fn set_zoom(&mut self, value: f32) -> Result<f32, ViewerError> {
        self.zoom.set(value).map_err(|error| error.into())
    }

    /// Increases zoom.
    pub fn zoom_in(&mut self) -> f32 {
        self.zoom.increase()
    }

    /// Decreases zoom.
    pub fn zoom_out(&mut self) -> f32 {
        self.zoom.decrease()
    }

    /// Resets zoom to the control default.
    pub fn zoom_reset(&mut self) -> f32 {
        self.zoom.reset()
    }

    /// Starts a slideshow from the selected or first slide.
    pub fn start_slideshow(&mut self, start: SlideshowStart) -> Result<(), ViewerError> {
        let target = match start {
            SlideshowStart::First => 0,
            SlideshowStart::Current => self.current_index().unwrap_or(0).min(self.slide_count()),
        };
        let transition = self.transition_for(target);
        self.slideshow.set_transition(transition);
        self.slideshow
            .start(start, self.thumbnails.selected)
            .map_err(|error| error.into())
    }

    /// Stops an active slideshow.
    pub fn stop_slideshow(&mut self) {
        self.slideshow.stop();
    }

    /// Routes a key while a slideshow is running.
    pub fn slideshow_key(&mut self, key: Key) -> Result<Option<usize>, ViewerError> {
        if matches!(self.slideshow.mode, SlideshowMode::Stopped) {
            return Err(ViewerError::SlideshowNotRunning);
        }

        let target = match key {
            Key::Escape => {
                self.stop_slideshow();
                return Ok(None);
            }
            Key::Left | Key::Up | Key::PageUp => self
                .current_index()
                .map(|current| current.saturating_sub(1)),
            Key::Right | Key::Down | Key::PageDown | Key::Space => self
                .current_index()
                .map(|current| (current + 1).min(self.slide_count().saturating_sub(1))),
            Key::Home => Some(0),
            Key::End => Some(self.slide_count().saturating_sub(1)),
            Key::F11 => {
                self.slideshow.toggle_fullscreen();
                return Ok(None);
            }
        };

        if let Some(target) = target {
            self.slideshow.set_transition(self.transition_for(target));
        }

        let result = self
            .slideshow
            .handle_key(key)
            .map(|option| {
                option.filter(|_| matches!(self.slideshow.mode, SlideshowMode::Presenting))
            })
            .map_err(ViewerError::from)?;

        if let Some(target) = result {
            let _ = self.select(target);
        }

        Ok(result)
    }

    /// Currently presented index, only while presenting.
    pub fn presented_index(&self) -> Option<usize> {
        if matches!(self.slideshow.mode, SlideshowMode::Presenting) {
            self.slideshow.current
        } else {
            None
        }
    }

    /// Read-only view of the presented slide, only while presenting.
    pub fn presented_view(&self) -> Option<SlideView> {
        self.presented_index()
            .and_then(|index| self.slide_view(index))
    }

    /// Builds a render plan for a slide by index and viewport.
    pub fn render_plan(&self, index: usize, viewport: Viewport) -> Option<RenderPlan> {
        let slide = self.document.slides.get(index)?;
        Some(build_plan(slide, viewport))
    }

    /// Builds a lightweight thumbnail render plan.
    ///
    /// It preserves slide geometry, text, shapes, and tables, but replaces
    /// raster media with frames so that the navigation pane does not duplicate
    /// or decode full-size images. The central slide always uses `render_plan`.
    pub fn thumbnail_plan(&self, index: usize, box_size: Size) -> Option<RenderPlan> {
        let slide = self.document.slides.get(index)?;
        let viewport = Viewport::fit(
            Rect::new(Point::ZERO, slide.size.width, slide.size.height),
            Rect::new(Point::ZERO, box_size.width, box_size.height),
        );
        Some(build_thumbnail_plan(slide, viewport))
    }

    /// Builds a presented-slide render plan if a slideshow is running.
    pub fn presented_plan(&self, box_size: Size) -> Option<RenderPlan> {
        let index = self.presented_index()?;
        let slide = self.document.slides.get(index)?;
        let viewport = Viewport::fit(
            Rect::new(Point::ZERO, slide.size.width, slide.size.height),
            Rect::new(Point::ZERO, box_size.width, box_size.height),
        );
        Some(build_plan(slide, viewport))
    }

    /// Dispatches one read-only viewer action.
    pub fn dispatch(&mut self, action: ViewerAction) -> Result<(), ViewerError> {
        match action {
            ViewerAction::SelectSlide(index) => {
                self.select(index)?;
            }
            ViewerAction::First => {
                self.first()?;
            }
            ViewerAction::Previous => {
                self.previous()?;
            }
            ViewerAction::Next => {
                self.next()?;
            }
            ViewerAction::Last => {
                self.last()?;
            }
            ViewerAction::ZoomIn => {
                self.zoom_in();
            }
            ViewerAction::ZoomOut => {
                self.zoom_out();
            }
            ViewerAction::SetZoom(value) => {
                self.set_zoom(value)?;
            }
            ViewerAction::Reset => {
                self.zoom_reset();
            }
            ViewerAction::SelectObject(object_id) => {
                self.selected_object = object_id;
                self.sync_selected_object_from_action(object_id);
            }
            ViewerAction::ToggleNotes => {
                self.notes_visible = !self.notes_visible;
            }
            ViewerAction::ToggleProperties => {
                self.properties_visible = !self.properties_visible;
            }
            ViewerAction::StartSlideshow(start) => {
                self.start_slideshow(start)?;
            }
            ViewerAction::StopSlideshow => {
                self.stop_slideshow();
            }
            ViewerAction::ToggleFullscreen => {
                self.slideshow.toggle_fullscreen();
            }
            ViewerAction::KeyPress(key) => {
                self.slideshow_key(key)?;
            }
        }
        Ok(())
    }

    fn transition_for(&self, target: usize) -> SimpleTransition {
        let slide = self.document.slides.get(target);
        slide
            .and_then(|slide| slide.transition)
            .map(|_| SimpleTransition::Fade)
            .unwrap_or(SimpleTransition::None)
    }

    fn rebuild_thumbnails_and_slideshow(&mut self) {
        self.thumbnails = ThumbnailList::from_document(&self.document);
        self.slideshow.slide_count = self.thumbnails.len();
        self.slideshow.current = self.thumbnails.selected;
        if self.thumbnails.is_empty() {
            self.slideshow.stop();
        }
    }

    fn selected_slide_id_for_replacement(&self, next_document: &Presentation) -> Option<SlideId> {
        if let Some(slide_id) = self.current_slide_id()
            && next_document.slide(slide_id).is_some()
        {
            return Some(slide_id);
        }
        None
    }

    fn sync_selected_object(&mut self, previous_slide_id: Option<SlideId>) {
        // Object IDs are slide-local; equal IDs on another slide identify
        // different objects and must not inherit the previous selection.
        if self.current_slide_id() != previous_slide_id {
            self.selected_object = None;
            return;
        }
        if let Some(slide_id) = self.current_slide_id()
            && let Some(slide) = self.document.slide(slide_id)
        {
            if self
                .selected_object
                .and_then(|object_id| slide.object(object_id))
                .is_none()
            {
                self.selected_object = None;
            }
            return;
        }
        self.selected_object = None;
    }

    fn sync_selected_object_from_action(&mut self, object_id: Option<ObjectId>) {
        if let (Some(slide_id), Some(object_id)) = (self.current_slide_id(), object_id)
            && let Some(slide) = self.document.slide(slide_id)
            && slide.object(object_id).is_some()
        {
            self.selected_object = Some(object_id);
            return;
        }
        self.selected_object = None;
    }
}

/// Read-only viewer actions.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewerAction {
    /// Select a slide by zero-based index.
    SelectSlide(usize),
    /// Move to the first slide.
    First,
    /// Move to the previous slide.
    Previous,
    /// Move to the next slide.
    Next,
    /// Move to the last slide.
    Last,
    /// Increase zoom.
    ZoomIn,
    /// Decrease zoom.
    ZoomOut,
    /// Set zoom directly.
    SetZoom(f32),
    /// Reset zoom to default.
    Reset,
    /// Select an object for inspection.
    SelectObject(Option<ObjectId>),
    /// Toggle the notes panel.
    ToggleNotes,
    /// Toggle the properties panel.
    ToggleProperties,
    /// Start a slideshow.
    StartSlideshow(SlideshowStart),
    /// Stop an active slideshow.
    StopSlideshow,
    /// Toggle fullscreen.
    ToggleFullscreen,
    /// Handle a keyboard event.
    KeyPress(Key),
}

impl ViewerAction {
    /// Human-readable label for logging and telemetry.
    pub fn label(&self) -> &'static str {
        match self {
            Self::SelectSlide(_) => "Select slide",
            Self::First => "First",
            Self::Previous => "Previous",
            Self::Next => "Next",
            Self::Last => "Last",
            Self::ZoomIn => "Zoom in",
            Self::ZoomOut => "Zoom out",
            Self::SetZoom(_) => "Set zoom",
            Self::Reset => "Reset",
            Self::SelectObject(_) => "Select object",
            Self::ToggleNotes => "Toggle notes",
            Self::ToggleProperties => "Toggle properties",
            Self::StartSlideshow(_) => "Start slideshow",
            Self::StopSlideshow => "Stop slideshow",
            Self::ToggleFullscreen => "Toggle fullscreen",
            Self::KeyPress(_) => "Key press",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slides_api::{ObjectKind, Point, Rect, Slide, SlideObject, Text, Transform, Transition};

    fn empty_presentation() -> Presentation {
        Presentation::new(slides_api::PresentationId::new(1))
    }

    fn presentation_with_slides(count: usize) -> Presentation {
        let mut presentation = Presentation::new(slides_api::PresentationId::new(1));
        for index in 0..count {
            let mut slide = Slide::new(slides_api::SlideId::new((index + 1) as u64));
            slide.notes = Some(format!("Notes {index}"));
            presentation.add_slide(slide).unwrap();
        }
        presentation
    }

    fn viewer_with_selected_object(index: usize) -> PresentationViewer {
        let mut presentation = presentation_with_slides(2);
        for slide in &mut presentation.slides {
            slide
                .add_object(SlideObject::new(
                    ObjectId::new(1),
                    ObjectKind::Text(Text::plain(format!("Text on slide {}", slide.id))),
                    Rect::new(Point::new(10.0, 20.0), 120.0, 50.0),
                    Transform::IDENTITY,
                ))
                .unwrap();
        }
        let mut viewer = PresentationViewer::from_document(presentation).unwrap();
        viewer.select(index).unwrap();
        viewer
            .dispatch(ViewerAction::SelectObject(Some(ObjectId::new(1))))
            .unwrap();
        viewer
    }

    #[test]
    fn empty_deck_is_valid() {
        let mut viewer = PresentationViewer::from_document(empty_presentation()).unwrap();
        assert!(viewer.is_empty());
        assert_eq!(viewer.slide_count(), 0);
        assert!(matches!(viewer.first(), Err(ViewerError::NoSlides)));
        assert!(matches!(viewer.select(0), Err(ViewerError::NoSlides)));
    }

    #[test]
    fn strict_and_bounded_navigation() {
        let mut viewer = PresentationViewer::from_document(presentation_with_slides(2)).unwrap();
        assert_eq!(viewer.select(1).unwrap(), slides_api::SlideId::new(2));
        assert!(matches!(
            viewer.select(2),
            Err(ViewerError::InvalidSlideIndex { .. })
        ));
        viewer.last().unwrap();
        viewer.next().unwrap();
        assert_eq!(viewer.current_index(), Some(1));
        viewer.first().unwrap();
        viewer.previous().unwrap();
        assert_eq!(viewer.current_index(), Some(0));
    }

    #[test]
    fn selection_is_cleared_when_navigation_changes_slide() {
        for (start_index, action, target_index) in [
            (0, ViewerAction::SelectSlide(1), 1),
            (0, ViewerAction::Next, 1),
            (0, ViewerAction::Last, 1),
            (1, ViewerAction::SelectSlide(0), 0),
            (1, ViewerAction::Previous, 0),
            (1, ViewerAction::First, 0),
        ] {
            let mut viewer = viewer_with_selected_object(start_index);
            let document = viewer.document().clone();
            assert_eq!(viewer.selected_object(), Some(ObjectId::new(1)));

            viewer.dispatch(action.clone()).unwrap();

            assert_eq!(viewer.current_index(), Some(target_index), "{action:?}");
            assert_eq!(viewer.selected_object(), None, "{action:?}");
            assert_eq!(viewer.document(), &document, "{action:?}");
        }
    }

    #[test]
    fn selection_is_preserved_for_same_slide_and_clamped_navigation() {
        for (index, action) in [
            (0, ViewerAction::SelectSlide(0)),
            (0, ViewerAction::First),
            (0, ViewerAction::Previous),
            (1, ViewerAction::SelectSlide(1)),
            (1, ViewerAction::Last),
            (1, ViewerAction::Next),
        ] {
            let mut viewer = viewer_with_selected_object(index);
            let before = viewer.clone();

            viewer.dispatch(action.clone()).unwrap();

            assert_eq!(viewer, before, "{action:?}");
        }
    }

    #[test]
    fn selection_is_unchanged_after_invalid_slide_index() {
        let mut viewer = viewer_with_selected_object(1);
        viewer.set_zoom(2.0).unwrap();
        viewer.dispatch(ViewerAction::ToggleNotes).unwrap();
        let before = viewer.clone();

        for index in [2, usize::MAX] {
            assert!(matches!(
                viewer.dispatch(ViewerAction::SelectSlide(index)),
                Err(ViewerError::InvalidSlideIndex { .. })
            ));
            assert_eq!(viewer, before);
        }
    }

    #[test]
    fn selection_is_cleared_by_slideshow_navigation() {
        for (start_index, key, target_index) in [
            (0, Key::Right, 1),
            (0, Key::Down, 1),
            (0, Key::PageDown, 1),
            (0, Key::Space, 1),
            (0, Key::End, 1),
            (1, Key::Left, 0),
            (1, Key::Up, 0),
            (1, Key::PageUp, 0),
            (1, Key::Home, 0),
        ] {
            let mut viewer = viewer_with_selected_object(start_index);
            let document = viewer.document().clone();
            viewer.start_slideshow(SlideshowStart::Current).unwrap();

            viewer.dispatch(ViewerAction::KeyPress(key)).unwrap();

            assert_eq!(viewer.current_index(), Some(target_index), "{key:?}");
            assert_eq!(viewer.presented_index(), Some(target_index), "{key:?}");
            assert_eq!(viewer.selected_object(), None, "{key:?}");
            assert_eq!(viewer.document(), &document, "{key:?}");
        }
    }

    #[test]
    fn selection_is_preserved_for_clamped_slideshow_navigation() {
        for (index, key) in [
            (0, Key::Left),
            (0, Key::Up),
            (0, Key::PageUp),
            (0, Key::Home),
            (1, Key::Right),
            (1, Key::Down),
            (1, Key::PageDown),
            (1, Key::Space),
            (1, Key::End),
        ] {
            let mut viewer = viewer_with_selected_object(index);
            let document = viewer.document().clone();
            viewer.start_slideshow(SlideshowStart::Current).unwrap();

            viewer.dispatch(ViewerAction::KeyPress(key)).unwrap();

            assert_eq!(viewer.current_index(), Some(index), "{key:?}");
            assert_eq!(viewer.presented_index(), Some(index), "{key:?}");
            assert_eq!(viewer.selected_object(), Some(ObjectId::new(1)), "{key:?}");
            assert_eq!(viewer.document(), &document, "{key:?}");
        }
    }

    #[test]
    fn mock_engine_load_maps_errors_exactly() {
        struct ErrorEngine;
        impl PresentationEngine for ErrorEngine {
            fn load(&self, _: &[u8]) -> Result<Presentation, EngineError> {
                Err(EngineError::InvalidInput("bad".into()))
            }
            fn save(&self, _: &Presentation) -> Result<Vec<u8>, EngineError> {
                Ok(Vec::new())
            }
        }
        let error = PresentationViewer::load(&ErrorEngine, b"bytes").unwrap_err();
        assert_eq!(
            error,
            ViewerError::Engine(EngineError::InvalidInput("bad".into()))
        );
    }

    #[test]
    fn invalid_nonempty_document_fails_from_document() {
        let mut bad = empty_presentation();
        let mut bad_slide = Slide::new(slides_api::SlideId::new(2));
        bad_slide.size = slides_api::Size::new(-1.0, 1.0);
        bad.slides = vec![bad_slide];
        let err = PresentationViewer::from_document(bad).unwrap_err();
        assert!(matches!(
            err,
            ViewerError::InvalidDocument(ValidationError::InvalidSlideSize { .. })
        ));
    }

    #[test]
    fn sync_atomic_preserves_state_on_invalid_replacement() {
        let mut viewer = PresentationViewer::from_document(presentation_with_slides(2)).unwrap();
        viewer.select(1).unwrap();
        viewer.set_zoom(2.0).unwrap();
        viewer.notes_visible = true;

        let mut invalid = empty_presentation();
        let bad_slide = Slide {
            id: slides_api::SlideId::new(3),
            size: slides_api::Size::new(0.0, 0.0),
            ..Slide::new(slides_api::SlideId::new(3))
        };
        invalid.slides = vec![bad_slide];
        assert!(matches!(
            viewer.sync_from_document(&invalid),
            Err(ViewerError::InvalidDocument(
                ValidationError::InvalidSlideSize { .. }
            ))
        ));
        assert_eq!(viewer.slide_count(), 2);
        assert_eq!(viewer.current_index(), Some(1));
        assert_eq!(viewer.zoom.value, 2.0);
        assert!(viewer.notes_visible());
    }

    #[test]
    fn selected_slide_id_retains_on_sync() {
        let mut viewer = PresentationViewer::from_document(presentation_with_slides(3)).unwrap();
        viewer.select(2).unwrap();

        let replacement = presentation_with_slides(3);
        viewer.sync_from_document(&replacement).unwrap();
        assert_eq!(viewer.current_index(), Some(2));
    }

    #[test]
    fn sync_falls_back_to_first_when_selected_slide_removed() {
        let mut viewer = PresentationViewer::from_document(presentation_with_slides(3)).unwrap();
        viewer.select(2).unwrap();

        let replacement = presentation_with_slides(2);
        viewer.sync_from_document(&replacement).unwrap();
        assert_eq!(viewer.current_index(), Some(0));
        assert_eq!(viewer.current_slide_id(), Some(slides_api::SlideId::new(1)));
    }

    #[test]
    fn sync_success_preserves_zoom_and_panel_state() {
        let mut viewer = PresentationViewer::from_document(presentation_with_slides(2)).unwrap();
        viewer.select(1).unwrap();
        viewer.set_zoom(2.0).unwrap();
        viewer.notes_visible = true;
        viewer.properties_visible = true;
        viewer.slideshow.toggle_fullscreen();

        let replacement = presentation_with_slides(3);
        viewer.sync_from_document(&replacement).unwrap();
        assert_eq!(viewer.current_index(), Some(1));
        assert_eq!(viewer.zoom.value, 2.0);
        assert!(viewer.notes_visible());
        assert!(viewer.properties_visible());
        assert!(viewer.slideshow.fullscreen);
    }

    #[test]
    fn notes_accessors() {
        let mut viewer = PresentationViewer::from_document(presentation_with_slides(2)).unwrap();
        assert_eq!(viewer.notes(1), Some("Notes 1"));
        viewer.next().unwrap();
        assert_eq!(viewer.current_notes(), Some("Notes 1"));
    }

    #[test]
    fn zoom_nan_rejected() {
        let mut viewer = PresentationViewer::from_document(empty_presentation()).unwrap();
        assert!(
            matches!(viewer.set_zoom(f32::NAN), Err(ViewerError::InvalidZoom { value }) if value.is_nan())
        );
    }

    #[test]
    fn slideshow_start_key_escape_presented_ties() {
        let mut viewer = PresentationViewer::from_document(presentation_with_slides(2)).unwrap();
        viewer.start_slideshow(SlideshowStart::First).unwrap();
        assert_eq!(viewer.presented_index(), Some(0));
        assert!(matches!(viewer.slideshow_key(Key::Right), Ok(Some(1))));
        assert_eq!(viewer.presented_index(), Some(1));
        viewer.slideshow_key(Key::Escape).unwrap();
        assert_eq!(viewer.presented_index(), None);
    }

    #[test]
    fn slideshow_fade_maps_only_for_target_with_valid_transition() {
        let mut viewer = PresentationViewer::from_document(presentation_with_slides(1)).unwrap();
        viewer.start_slideshow(SlideshowStart::First).unwrap();
        assert_eq!(viewer.slideshow.transition, SimpleTransition::None);

        let mut presentation = empty_presentation();
        let mut slide = Slide::new(slides_api::SlideId::new(1));
        slide.transition = Some(Transition::new(200, slides_api::SlideDirection::Left));
        presentation.add_slide(slide).unwrap();
        let mut fade_viewer = PresentationViewer::from_document(presentation).unwrap();
        fade_viewer.start_slideshow(SlideshowStart::First).unwrap();
        fade_viewer.next().unwrap();
        assert_eq!(fade_viewer.slideshow.transition, SimpleTransition::Fade);
    }

    #[test]
    fn view_and_thumbnail_metadata_selectors() {
        let viewer = PresentationViewer::from_document(presentation_with_slides(2)).unwrap();
        let first = viewer.slide_view(0).unwrap();
        let _second = viewer.slide_view(1).unwrap();
        assert!(viewer.slide_view(2).is_none());
        assert_eq!(first.id, slides_api::SlideId::new(1));
        assert_eq!(first.size, slides_api::Size::new(1280.0, 720.0));

        let thumbnails = viewer.thumbnail_views();
        assert_eq!(thumbnails.len(), 2);
        assert_eq!(thumbnails[0].slide_id, slides_api::SlideId::new(1));
        assert_eq!(thumbnails[0].index, 0);
        assert_eq!(thumbnails[0].size, (1280, 720));
        assert!(thumbnails[0].selected);
        assert!(!thumbnails[1].selected);
    }

    #[test]
    fn transition_for_target_uses_slide_transition() {
        let viewer = PresentationViewer::from_document(presentation_with_slides(1)).unwrap();
        assert_eq!(viewer.transition_for(0), SimpleTransition::None);

        let mut presentation = empty_presentation();
        let mut slide = Slide::new(slides_api::SlideId::new(1));
        slide.transition = Some(Transition::new(200, slides_api::SlideDirection::Left));
        presentation.add_slide(slide).unwrap();
        let transition_viewer = PresentationViewer::from_document(presentation).unwrap();
        assert_eq!(transition_viewer.transition_for(0), SimpleTransition::Fade);
    }

    #[test]
    fn thumbnail_plan_uses_fit_viewport() {
        let viewer = PresentationViewer::from_document(presentation_with_slides(1)).unwrap();
        let plan = viewer
            .thumbnail_plan(0, slides_api::Size::new(100.0, 100.0))
            .unwrap();
        assert_eq!(plan.slide_id, slides_api::SlideId::new(1));
        assert!(plan.validate());
    }

    #[test]
    fn presented_plan_only_while_presenting() {
        let mut viewer = PresentationViewer::from_document(presentation_with_slides(1)).unwrap();
        assert!(
            viewer
                .presented_plan(slides_api::Size::new(100.0, 100.0))
                .is_none()
        );

        viewer.start_slideshow(SlideshowStart::First).unwrap();
        let plan = viewer
            .presented_plan(slides_api::Size::new(100.0, 100.0))
            .unwrap();
        assert_eq!(plan.slide_id, slides_api::SlideId::new(1));
        assert!(plan.validate());
    }

    #[test]
    fn slideshow_key_syncs_thumbnail_selection() {
        let mut viewer = PresentationViewer::from_document(presentation_with_slides(2)).unwrap();
        viewer.start_slideshow(SlideshowStart::First).unwrap();
        viewer.slideshow_key(Key::Right).unwrap();
        assert_eq!(viewer.current_index(), Some(1));
        assert!(viewer.thumbnail_views()[1].selected);
    }

    #[test]
    fn viewer_error_from_read_only_uses_engine_invalid_input() {
        let error = ViewerError::from(UiError::ReadOnly);
        assert!(matches!(
            error,
            ViewerError::Engine(EngineError::InvalidInput(_))
        ));
    }
}
