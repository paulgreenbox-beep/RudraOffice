//! Headless-testable UI state and composition for Rudra Slides.
//!
//! `slides-ui` owns transient presentation-editor view state, composition, action
//! routing, and slideshow navigation. It does not mutate a document. Document
//! mutations are forwarded through [`EditorCommandRouter`] so `slides-core` remains
//! the owner of `EditorCommand` and core invariants.
//!
//! This crate is `std`-only and has no Makepad dependency. A Makepad adapter can
//! implement [`CanvasHostExtension`] at the application boundary.

#![warn(missing_docs)]

mod render;
mod toolbar;
mod view;
mod viewer;

pub use render::build_plan;
pub use toolbar::{
    ToolbarControl, ToolbarControlId, ToolbarControlKind, ToolbarDisabledReason, ToolbarGroup,
    ToolbarGroupId, ToolbarTab, ToolbarTabId, ToolbarView, project_toolbar,
};
pub use view::{
    RenderPlan, RenderWarning, RenderWarningKind, SlideObjectView, SlideView, ThumbnailPreview,
};
pub use viewer::{PresentationViewer, ViewerAction, ViewerError};

use slides_api::{EngineError, ObjectId, Presentation, PresentationEngine, SlideId};
use slides_core::{EditorCommand, EditorError, PresentationEditor, Size, SlideLayout};

/// Minimum supported editor zoom.
pub const MIN_ZOOM: f32 = 0.25;
/// Maximum supported editor zoom.
pub const MAX_ZOOM: f32 = 4.0;
/// Default editor zoom.
pub const DEFAULT_ZOOM: f32 = 1.0;
/// Zoom increment used by the zoom controls.
pub const ZOOM_STEP: f32 = 0.1;

/// Errors produced while applying transient UI actions.
#[derive(Debug, Clone, PartialEq)]
pub enum UiError {
    /// An action requires at least one slide.
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
    /// Slideshow navigation was requested while it was stopped.
    SlideshowNotRunning,
    /// The current UI snapshot is read-only and cannot mutate the document.
    ReadOnly,
}

impl std::fmt::Display for UiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSlides => formatter.write_str("the presentation has no slides"),
            Self::InvalidSlideIndex { index, slide_count } => formatter.write_fmt(format_args!(
                "slide index {index} is outside a list of {slide_count} slides"
            )),
            Self::InvalidZoom { value } => {
                formatter.write_fmt(format_args!("zoom must be finite, got {value:?}"))
            }
            Self::SlideshowNotRunning => formatter.write_str("slideshow is not running"),
            Self::ReadOnly => formatter.write_str("the UI snapshot is read-only"),
        }
    }
}

impl std::error::Error for UiError {}

/// Keyboard keys understood by editor and slideshow routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Move to the previous slide.
    Left,
    /// Move to the next slide.
    Right,
    /// Move to the previous slide.
    Up,
    /// Move to the next slide.
    Down,
    /// Move to the first slide.
    Home,
    /// Move to the last slide.
    End,
    /// Move one slide backward.
    PageUp,
    /// Move one slide forward.
    PageDown,
    /// Advance the slideshow.
    Space,
    /// Exit slideshow mode.
    Escape,
    /// Toggle fullscreen presentation.
    F11,
}

/// Whether a slideshow starts at the first slide or the editor's current slide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlideshowStart {
    /// Start at slide zero.
    First,
    /// Start at the currently selected slide.
    Current,
}

/// Explicit intent originating in the UI.
///
/// The first group is transient UI state. The second group is document intent
/// and must be routed to `slides-core` through [`EditorCommandRouter`]. The exact
/// `EditorCommand` type is deliberately not recreated here: `slides-core`
/// owns that contract and this layer only records user intent.
#[derive(Debug, Clone, PartialEq)]
pub enum UiAction {
    /// Select a thumbnail by zero-based index.
    SelectSlide(usize),
    /// Select an object in the properties panel.
    SelectObject(Option<ObjectId>),
    /// Move editor selection to the first slide.
    FirstSlide,
    /// Move editor selection to the previous slide.
    PreviousSlide,
    /// Move editor selection to the next slide.
    NextSlide,
    /// Move editor selection to the last slide.
    LastSlide,
    /// Increase editor zoom by [`ZOOM_STEP`].
    ZoomIn,
    /// Decrease editor zoom by [`ZOOM_STEP`].
    ZoomOut,
    /// Set editor zoom, clamped to [`MIN_ZOOM`]..=[`MAX_ZOOM`].
    SetZoom(f32),
    /// Restore [`DEFAULT_ZOOM`].
    ResetZoom,
    /// Toggle the properties panel.
    ToggleProperties,
    /// Toggle the notes panel.
    ToggleNotes,
    /// Toggle fullscreen state of the slideshow controller.
    ToggleFullscreen,
    /// Start slideshow from the first or current slide.
    StartSlideshow(SlideshowStart),
    /// Stop slideshow mode.
    StopSlideshow,
    /// Handle a keyboard key.
    KeyPress(Key),
    /// Create a slide through the core editor command route.
    NewSlide,
    /// Delete the current slide through the core editor command route.
    DeleteCurrentSlide,
    /// Duplicate the current slide through the core editor command route.
    DuplicateCurrentSlide,
    /// Reorder a slide through the core editor command route.
    ReorderSlide {
        /// Source index.
        from: usize,
        /// Destination index.
        to: usize,
    },
    /// Change the current slide's size through the core editor command route.
    SetSlideSize {
        /// Target slide.
        slide_id: SlideId,
        /// New width in slide units.
        width: u32,
        /// New height in slide units.
        height: u32,
    },
    /// Commit speaker notes through the core editor command route.
    ///
    /// The current API has no notes field; this remains an explicit intent until
    /// the core contract provides the matching command.
    UpdateNotes {
        /// Target slide.
        slide_id: SlideId,
        /// Notes text.
        text: String,
    },
    /// Replace one existing text run without changing its formatting.
    ReplaceTextRun {
        /// Owning slide.
        slide_id: SlideId,
        /// Text object containing the run.
        object_id: ObjectId,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// Zero-based run index within that paragraph.
        run_index: usize,
        /// Replacement text.
        text: String,
    },
}

impl UiAction {
    /// Returns whether the action must be handled by `slides-core`.
    pub fn is_core_command(&self) -> bool {
        matches!(
            self,
            Self::NewSlide
                | Self::DeleteCurrentSlide
                | Self::DuplicateCurrentSlide
                | Self::ReorderSlide { .. }
                | Self::SetSlideSize { .. }
                | Self::UpdateNotes { .. }
                | Self::ReplaceTextRun { .. }
        )
    }

    /// Returns a stable label useful for the Ribbon and status bar.
    pub fn label(&self) -> &'static str {
        match self {
            Self::SelectSlide(_) => "Select slide",
            Self::SelectObject(_) => "Select object",
            Self::FirstSlide => "First slide",
            Self::PreviousSlide => "Previous slide",
            Self::NextSlide => "Next slide",
            Self::LastSlide => "Last slide",
            Self::ZoomIn => "Zoom in",
            Self::ZoomOut => "Zoom out",
            Self::SetZoom(_) => "Set zoom",
            Self::ResetZoom => "Reset zoom",
            Self::ToggleProperties => "Toggle properties",
            Self::ToggleNotes => "Toggle notes",
            Self::ToggleFullscreen => "Toggle fullscreen",
            Self::StartSlideshow(_) => "Start slideshow",
            Self::StopSlideshow => "Stop slideshow",
            Self::KeyPress(_) => "Keyboard input",
            Self::NewSlide => "New slide",
            Self::DeleteCurrentSlide => "Delete slide",
            Self::DuplicateCurrentSlide => "Duplicate slide",
            Self::ReorderSlide { .. } => "Reorder slide",
            Self::SetSlideSize { .. } => "Set slide size",
            Self::UpdateNotes { .. } => "Update notes",
            Self::ReplaceTextRun { .. } => "Replace text run",
        }
    }
}

/// Boundary implemented by `slides-core` to turn UI intent into its frozen
/// `EditorCommand` and apply it to the editor.
///
/// `slides-ui` intentionally accepts the UI action. Its adapter maps each
/// document action to the exact core command and keeps mutation logic in core.
/// No parallel command DTO is introduced here.
pub trait EditorCommandRouter {
    /// Error returned by the core command route.
    type Error;

    /// Convert and dispatch one document-mutating UI intent.
    fn route(&mut self, action: &UiAction) -> Result<(), Self::Error>;
}

/// Boundary for a renderer integration, including a future Makepad host.
///
/// Methods only synchronize view state. A renderer must not own presentation
/// mutation or invent a second editor command model.
pub trait CanvasHostExtension {
    /// Attach the slide currently hosted by the central canvas.
    fn set_slide(&mut self, slide_id: Option<SlideId>);
    /// Synchronize canvas zoom with editor view state.
    fn set_zoom(&mut self, zoom: f32);
    /// Ask the renderer to repaint.
    fn request_repaint(&mut self);
}

/// One item in the thumbnail list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    /// Slide identifier from `slides-api`.
    pub slide_id: SlideId,
    /// Zero-based position in presentation order.
    pub index: usize,
    /// UI label. The API has no title field, so the default is `Slide N`.
    pub label: String,
    /// Whether the thumbnail is selected in the editor.
    pub selected: bool,
}

/// Headless thumbnail list state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ThumbnailList {
    /// Items in presentation order.
    pub items: Vec<Thumbnail>,
    /// Selected zero-based item, if any.
    pub selected: Option<usize>,
}

impl ThumbnailList {
    /// Builds thumbnail state from an API presentation snapshot.
    pub fn from_document(document: &Presentation) -> Self {
        let items = document
            .slides
            .iter()
            .enumerate()
            .map(|(index, slide)| Thumbnail {
                slide_id: slide.id,
                index,
                label: format!("Slide {}", index + 1),
                selected: false,
            })
            .collect::<Vec<_>>();
        let selected = (!items.is_empty()).then_some(0);
        let mut result = Self { items, selected };
        result.sync_selected_flags();
        result
    }

    /// Number of thumbnails.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns whether the thumbnail list is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Selects an item and returns its slide identifier.
    pub fn select(&mut self, index: usize) -> Result<SlideId, UiError> {
        let item = self.items.get(index).ok_or(UiError::InvalidSlideIndex {
            index,
            slide_count: self.items.len(),
        })?;
        let slide_id = item.slide_id;
        self.selected = Some(index);
        self.sync_selected_flags();
        Ok(slide_id)
    }

    /// Selects the first thumbnail, if one exists.
    pub fn select_first(&mut self) -> Option<SlideId> {
        let slide_id = self.items.first().map(|item| item.slide_id)?;
        self.selected = Some(0);
        self.sync_selected_flags();
        Some(slide_id)
    }

    /// Selects the last thumbnail, if one exists.
    pub fn select_last(&mut self) -> Option<SlideId> {
        let slide_id = self.items.last().map(|item| item.slide_id)?;
        self.selected = Some(self.items.len() - 1);
        self.sync_selected_flags();
        Some(slide_id)
    }

    /// Selects the previous item, clamped at the first item.
    pub fn select_previous(&mut self) -> Option<SlideId> {
        if self.items.is_empty() {
            return None;
        }
        let index = self.selected.unwrap_or(0).saturating_sub(1);
        self.selected = Some(index);
        self.sync_selected_flags();
        Some(self.items[index].slide_id)
    }

    /// Selects the next item, clamped at the last item.
    pub fn select_next(&mut self) -> Option<SlideId> {
        if self.items.is_empty() {
            return None;
        }
        let last = self.items.len() - 1;
        let index = self.selected.unwrap_or(0).saturating_add(1).min(last);
        self.selected = Some(index);
        self.sync_selected_flags();
        Some(self.items[index].slide_id)
    }

    fn sync_selected_flags(&mut self) {
        for (index, item) in self.items.iter_mut().enumerate() {
            item.selected = self.selected == Some(index);
        }
    }
}

/// State of the central canvas host. Rendering remains an extension boundary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CanvasHostState {
    /// Slide attached to the central canvas.
    pub slide_id: Option<SlideId>,
    /// Whether the host is visible in the composition.
    pub visible: bool,
    /// Whether keyboard focus is currently in the canvas.
    pub focused: bool,
}

/// Properties panel state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PropertiesPanelState {
    /// Whether the panel is visible.
    pub visible: bool,
    /// Object selected for property editing.
    pub selected_object: Option<ObjectId>,
}

/// Notes panel state.
///
/// `draft` is loaded from the selected slide's `notes` field on
/// `from_document`, `sync_from_document`, and navigation actions.
/// An empty `UpdateNotes` text maps to `None` when committed to core.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotesPanelState {
    /// Whether the panel is visible.
    pub visible: bool,
    /// Slide whose notes are displayed.
    pub slide_id: Option<SlideId>,
    /// Transient text currently shown/edited by the panel.
    ///
    /// Loaded from `Slide.notes` on document sync and navigation.
    /// Cleared to empty when the selected slide has no notes.
    pub draft: String,
    /// Whether the draft is in editing mode.
    pub editing: bool,
}

/// Ribbon tabs used by the headless composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RibbonTab {
    /// Clipboard and slide commands.
    #[default]
    Home,
    /// Insert commands.
    Insert,
    /// Layout and theme commands.
    Design,
    /// Presentation commands.
    Slideshow,
    /// View and zoom commands.
    View,
}

/// Ribbon state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RibbonState {
    /// Active tab.
    pub active_tab: RibbonTab,
    /// Label of the most recently dispatched action.
    pub last_action: Option<String>,
}

/// Status bar state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusBarState {
    /// Human-readable status message.
    pub message: String,
    /// Whether the document has edits not yet persisted by the host.
    pub dirty: bool,
    /// One-based current slide number.
    pub current_slide: Option<usize>,
    /// Total number of slides.
    pub slide_count: usize,
}

/// Editor zoom state and control behavior.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomControl {
    /// Current clamped zoom factor.
    pub value: f32,
    /// Minimum accepted zoom factor.
    pub min: f32,
    /// Maximum accepted zoom factor.
    pub max: f32,
    /// Increment for plus/minus controls.
    pub step: f32,
}

impl Default for ZoomControl {
    fn default() -> Self {
        Self {
            value: DEFAULT_ZOOM,
            min: MIN_ZOOM,
            max: MAX_ZOOM,
            step: ZOOM_STEP,
        }
    }
}

impl ZoomControl {
    /// Sets zoom after validating finiteness and clamping to the control bounds.
    pub fn set(&mut self, value: f32) -> Result<f32, UiError> {
        if !value.is_finite() {
            return Err(UiError::InvalidZoom { value });
        }
        self.value = value.clamp(self.min, self.max);
        Ok(self.value)
    }

    /// Increases zoom and clamps at the maximum.
    pub fn increase(&mut self) -> f32 {
        let _ = self.set(self.value + self.step);
        self.value
    }

    /// Decreases zoom and clamps at the minimum.
    pub fn decrease(&mut self) -> f32 {
        let _ = self.set(self.value - self.step);
        self.value
    }

    /// Restores the default zoom within the configured bounds.
    pub fn reset(&mut self) -> f32 {
        let _ = self.set(DEFAULT_ZOOM);
        self.value
    }
}

/// A renderer-neutral simple slideshow transition.
///
/// This is transient presentation state, not a replacement for a future frozen
/// document transition DTO. The current API has no transition field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SimpleTransition {
    /// Switch immediately.
    #[default]
    None,
    /// Cross-fade between slides.
    Fade,
}

/// Slideshow lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SlideshowMode {
    /// No slideshow is running.
    #[default]
    Stopped,
    /// Slideshow is presenting a slide.
    Presenting,
}

/// Headless slideshow controller.
#[derive(Debug, Clone, PartialEq)]
pub struct SlideshowController {
    /// Current lifecycle state.
    pub mode: SlideshowMode,
    /// Zero-based slide currently being presented.
    pub current: Option<usize>,
    /// Number of slides in the presentation snapshot.
    pub slide_count: usize,
    /// Whether presentation is fullscreen.
    pub fullscreen: bool,
    /// Transition selected for the next navigation.
    pub transition: SimpleTransition,
    /// Whether a transition is currently active.
    pub transition_active: bool,
    /// Transition progress in the inclusive range `0.0..=1.0`.
    pub transition_progress: f32,
}

impl Default for SlideshowController {
    fn default() -> Self {
        Self {
            mode: SlideshowMode::Stopped,
            current: None,
            slide_count: 0,
            fullscreen: false,
            transition: SimpleTransition::None,
            transition_active: false,
            transition_progress: 1.0,
        }
    }
}

impl SlideshowController {
    /// Updates the available slide count and keeps current within bounds.
    pub fn set_slide_count(&mut self, slide_count: usize) {
        self.slide_count = slide_count;
        self.current = match (self.current, slide_count) {
            (_, 0) => None,
            (Some(index), count) => Some(index.min(count - 1)),
            (None, _) => None,
        };
        if slide_count == 0 {
            self.stop();
        }
    }

    /// Starts from the first slide or from the supplied current selection.
    pub fn start(&mut self, start: SlideshowStart, selected: Option<usize>) -> Result<(), UiError> {
        if self.slide_count == 0 {
            return Err(UiError::NoSlides);
        }
        self.current = Some(match start {
            SlideshowStart::First => 0,
            SlideshowStart::Current => selected.unwrap_or(0).min(self.slide_count - 1),
        });
        self.mode = SlideshowMode::Presenting;
        self.transition_active = false;
        self.transition_progress = 1.0;
        Ok(())
    }

    /// Stops presentation and clears its transient transition.
    pub fn stop(&mut self) {
        self.mode = SlideshowMode::Stopped;
        self.transition_active = false;
        self.transition_progress = 1.0;
    }

    /// Toggles fullscreen independently of the host window implementation.
    pub fn toggle_fullscreen(&mut self) -> bool {
        self.fullscreen = !self.fullscreen;
        self.fullscreen
    }

    /// Sets fullscreen state and returns the new value.
    pub fn set_fullscreen(&mut self, fullscreen: bool) -> bool {
        self.fullscreen = fullscreen;
        fullscreen
    }

    /// Selects the transition used by subsequent navigation.
    pub fn set_transition(&mut self, transition: SimpleTransition) {
        self.transition = transition;
    }

    /// Moves to an absolute slide index, clamped to the available range.
    pub fn go_to(&mut self, index: usize) -> Result<usize, UiError> {
        if self.mode != SlideshowMode::Presenting {
            return Err(UiError::SlideshowNotRunning);
        }
        if self.slide_count == 0 {
            return Err(UiError::NoSlides);
        }
        let destination = index.min(self.slide_count - 1);
        self.current = Some(destination);
        self.begin_transition();
        Ok(destination)
    }

    /// Moves one slide backward, clamped at zero.
    pub fn previous(&mut self) -> Result<usize, UiError> {
        let current = self.current.unwrap_or(0);
        self.go_to(current.saturating_sub(1))
    }

    /// Moves one slide forward, clamped at the last slide.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<usize, UiError> {
        let current = self.current.unwrap_or(0);
        self.go_to(current.saturating_add(1))
    }

    /// Moves to the first slide.
    pub fn first(&mut self) -> Result<usize, UiError> {
        self.go_to(0)
    }

    /// Moves to the last slide.
    pub fn last(&mut self) -> Result<usize, UiError> {
        self.go_to(self.slide_count.saturating_sub(1))
    }

    /// Begins the configured simple transition.
    pub fn begin_transition(&mut self) {
        self.transition_active = self.transition != SimpleTransition::None;
        self.transition_progress = if self.transition_active { 0.0 } else { 1.0 };
    }

    /// Advances transition progress and returns whether it is still active.
    pub fn advance_transition(&mut self, delta: f32) -> bool {
        if !self.transition_active {
            return false;
        }
        if delta.is_finite() && delta > 0.0 {
            self.transition_progress = (self.transition_progress + delta).clamp(0.0, 1.0);
        }
        if self.transition_progress >= 1.0 {
            self.transition_active = false;
            false
        } else {
            true
        }
    }

    /// Routes a keyboard key while presenting. Returns the resulting slide index,
    /// or `None` for keys that only affect lifecycle state.
    pub fn handle_key(&mut self, key: Key) -> Result<Option<usize>, UiError> {
        match key {
            Key::Left | Key::Up | Key::PageUp => self.previous().map(Some),
            Key::Right | Key::Down | Key::PageDown | Key::Space => self.next().map(Some),
            Key::Home => self.first().map(Some),
            Key::End => self.last().map(Some),
            Key::Escape => {
                self.stop();
                Ok(None)
            }
            Key::F11 => {
                self.toggle_fullscreen();
                Ok(None)
            }
        }
    }
}

/// Complete headless UI composition for an editor window.
#[derive(Debug, Clone, PartialEq)]
pub struct EditorUi {
    /// Private immutable snapshot of the document the UI is bound to.
    ///
    /// Navigation, notes loading, and document syncs read from this snapshot so
    /// that the UI never needs to keep a mutable handle on `slides-core`. It is
    /// replaced wholesale by [`EditorUi::from_document`] and
    /// [`EditorUi::sync_from_document`].
    document: Presentation,
    /// Left-side thumbnail list.
    pub thumbnails: ThumbnailList,
    /// Central canvas host state.
    pub canvas: CanvasHostState,
    /// Properties inspector state.
    pub properties: PropertiesPanelState,
    /// Speaker notes panel state.
    pub notes: NotesPanelState,
    /// Top Ribbon state.
    pub ribbon: RibbonState,
    /// Bottom status bar state.
    pub status_bar: StatusBarState,
    /// Editor zoom control.
    pub zoom: ZoomControl,
    /// Slideshow controller.
    pub slideshow: SlideshowController,
    /// Whether the UI is bound to a read-only document snapshot.
    read_only: bool,
}

impl Default for EditorUi {
    fn default() -> Self {
        Self::from_document(&Presentation::default())
    }
}

impl EditorUi {
    /// Builds a UI composition from an immutable API document snapshot.
    pub fn from_document(document: &Presentation) -> Self {
        let thumbnails = ThumbnailList::from_document(document);
        let selected_id = thumbnails
            .selected
            .and_then(|index| thumbnails.items.get(index))
            .map(|item| item.slide_id);
        let mut slideshow = SlideshowController::default();
        slideshow.set_slide_count(thumbnails.len());
        let mut ui = Self {
            document: document.clone(),
            thumbnails,
            canvas: CanvasHostState {
                slide_id: selected_id,
                visible: true,
                focused: false,
            },
            properties: PropertiesPanelState {
                visible: true,
                selected_object: None,
            },
            notes: NotesPanelState {
                visible: true,
                slide_id: selected_id,
                draft: document
                    .slides
                    .iter()
                    .find(|slide| selected_id == Some(slide.id))
                    .and_then(|slide| slide.notes.as_deref())
                    .unwrap_or_default()
                    .to_string(),
                editing: false,
            },
            ribbon: RibbonState::default(),
            status_bar: StatusBarState::default(),
            zoom: ZoomControl::default(),
            slideshow,
            read_only: false,
        };
        ui.refresh_status();
        ui
    }

    /// Builds a read-only UI snapshot from bytes loaded through a presentation engine.
    pub fn from_pptx<E: PresentationEngine + ?Sized>(
        engine: &E,
        bytes: &[u8],
    ) -> Result<Self, EngineError> {
        let document = engine.load(bytes)?;
        let mut ui = Self::from_document(&document);
        ui.read_only = true;
        Ok(ui)
    }

    /// Replaces the current UI snapshot from bytes loaded through a presentation engine.
    ///
    /// The replacement is atomic: a brand-new viewer is constructed first, and
    /// `self` is only replaced after the load succeeds. Load errors leave this
    /// instance byte-for-byte unchanged.
    pub fn replace_from_pptx<E: PresentationEngine + ?Sized>(
        &mut self,
        engine: &E,
        bytes: &[u8],
    ) -> Result<(), EngineError> {
        let replacement = Self::from_pptx(engine, bytes)?;
        *self = replacement;
        Ok(())
    }

    /// Returns whether the UI snapshot is read-only.
    pub fn read_only(&self) -> bool {
        self.read_only
    }

    /// Replaces state derived from a new core/API document snapshot.
    ///
    /// The previous selection is remembered by `SlideId`; the snapshot and
    /// thumbnails are then replaced, the selection is restored to the matching
    /// slide if it still exists or otherwise to the first slide, and the flags,
    /// canvas, status, and notes draft are re-synchronised from the new
    /// snapshot.
    pub fn sync_from_document(&mut self, document: &Presentation) {
        let previous_selected_id = self.current_slide_id();
        self.document = document.clone();
        self.thumbnails = ThumbnailList::from_document(document);
        self.slideshow.set_slide_count(self.thumbnails.len());
        // Restore selection by SlideId when possible, otherwise fall back to the
        // first slide (which matches `ThumbnailList::from_document`).
        let restored_index = previous_selected_id.and_then(|slide_id| {
            self.thumbnails
                .items
                .iter()
                .position(|item| item.slide_id == slide_id)
        });
        self.thumbnails.selected = match restored_index {
            Some(index) => Some(index),
            None => (!self.thumbnails.items.is_empty()).then_some(0),
        };
        self.thumbnails.sync_selected_flags();
        self.sync_selected_view_state();
        self.refresh_status();
        self.read_only = false;
    }

    /// Dispatches an action, routing document mutations to the supplied core port.
    pub fn dispatch<R: EditorCommandRouter>(
        &mut self,
        action: UiAction,
        router: &mut R,
    ) -> Result<(), DispatchError<R::Error>> {
        if self.read_only && action.is_core_command() {
            return Err(DispatchError::Ui(UiError::ReadOnly));
        }
        self.ribbon.last_action = Some(action.label().to_string());
        if action.is_core_command() {
            router.route(&action).map_err(DispatchError::Core)?;
            self.status_bar.dirty = true;
            self.status_bar.message = format!("{} routed to core", action.label());
            return Ok(());
        }
        self.apply_local_action(&action).map_err(DispatchError::Ui)
    }

    /// Dispatches a document-mutating UI action directly through `slides-core`,
    /// mapping each `UiAction` variant to the exact `EditorCommand` form.
    ///
    /// This call is UI-atomic: the ribbon, status bar, and document snapshot
    /// are not mutated until both the mapping to the core command and the
    /// `PresentationEditor::apply` succeed. `apply` itself is atomic
    /// (clone-and-commit), so on error the editor is unchanged and the UI is
    /// restored from a clone captured before mapping. On success the UI is
    /// re-synchronised from the updated document, the ribbon records the last
    /// action, and the status bar is marked dirty with an applied message.
    pub fn dispatch_to_core(
        &mut self,
        action: UiAction,
        editor: &mut PresentationEditor,
    ) -> Result<(), DispatchError<EditorError>> {
        if self.read_only && action.is_core_command() {
            return Err(DispatchError::Ui(UiError::ReadOnly));
        }
        // Take a clone snapshot before mutating the editor so we can restore
        // the UI byte-for-byte on failure. `PresentationEditor::apply` already
        // uses clone-and-commit, so on a core error the editor is also unchanged
        // and only the UI needs the rollback.
        let snapshot = self.clone();
        let command = match self.map_to_core_command(&action) {
            Ok(command) => command,
            Err(error) => {
                *self = snapshot;
                return Err(DispatchError::Ui(error));
            }
        };
        if let Err(error) = editor.apply(command) {
            *self = snapshot;
            return Err(DispatchError::Core(error));
        }
        // Core succeeded — refresh UI from the updated document.
        self.sync_from_document(editor.presentation());
        self.ribbon.last_action = Some(action.label().to_string());
        self.status_bar.dirty = true;
        self.status_bar.message = format!("{} applied", action.label());
        Ok(())
    }

    /// Maps a `UiAction` that targets the core editor to the exact
    /// `EditorCommand` variant. Returns `UiError::NoSlides` when a slide
    /// context is required but the presentation is empty.
    fn map_to_core_command(&self, action: &UiAction) -> Result<EditorCommand, UiError> {
        match action {
            UiAction::NewSlide => {
                let index = self.thumbnails.selected.map_or(0, |i| i + 1);
                Ok(EditorCommand::AddSlide {
                    index,
                    layout: SlideLayout::Blank,
                })
            }
            UiAction::DeleteCurrentSlide => {
                let slide_id = self.current_slide_id().ok_or(UiError::NoSlides)?;
                Ok(EditorCommand::DeleteSlide { slide_id })
            }
            UiAction::DuplicateCurrentSlide => {
                let slide_id = self.current_slide_id().ok_or(UiError::NoSlides)?;
                let index = self.thumbnails.selected.map_or(0, |i| i + 1);
                Ok(EditorCommand::DuplicateSlide { slide_id, index })
            }
            UiAction::ReorderSlide { from, to } => {
                let slide_id = self
                    .thumbnails
                    .items
                    .get(*from)
                    .ok_or(UiError::InvalidSlideIndex {
                        index: *from,
                        slide_count: self.thumbnails.items.len(),
                    })?
                    .slide_id;
                Ok(EditorCommand::ReorderSlide {
                    slide_id,
                    to_index: *to,
                })
            }
            UiAction::SetSlideSize {
                slide_id,
                width,
                height,
            } => Ok(EditorCommand::SetSlideSize {
                slide_id: *slide_id,
                size: Size::new(*width as f32, *height as f32),
            }),
            UiAction::UpdateNotes { slide_id, text } => Ok(EditorCommand::UpdateNotes {
                slide_id: *slide_id,
                notes: if text.is_empty() {
                    None
                } else {
                    Some(text.clone())
                },
            }),
            UiAction::ReplaceTextRun {
                slide_id,
                object_id,
                paragraph_index,
                run_index,
                text,
            } => Ok(EditorCommand::ReplaceTextRun {
                slide_id: *slide_id,
                object_id: *object_id,
                paragraph_index: *paragraph_index,
                run_index: *run_index,
                text: text.clone(),
            }),
            _ => Err(UiError::NoSlides),
        }
    }

    /// Applies a local action without a core route.
    pub fn apply_local_action(&mut self, action: &UiAction) -> Result<(), UiError> {
        match action {
            UiAction::SelectSlide(index) => {
                self.thumbnails.select(*index)?;
                self.sync_selected_view_state();
            }
            UiAction::SelectObject(object_id) => {
                self.properties.selected_object = *object_id;
            }
            UiAction::FirstSlide => {
                if self.thumbnails.select_first().is_none() {
                    return Err(UiError::NoSlides);
                }
                self.sync_selected_view_state();
            }
            UiAction::PreviousSlide => {
                if self.thumbnails.select_previous().is_none() {
                    return Err(UiError::NoSlides);
                }
                self.sync_selected_view_state();
            }
            UiAction::NextSlide => {
                if self.thumbnails.select_next().is_none() {
                    return Err(UiError::NoSlides);
                }
                self.sync_selected_view_state();
            }
            UiAction::LastSlide => {
                if self.thumbnails.select_last().is_none() {
                    return Err(UiError::NoSlides);
                }
                self.sync_selected_view_state();
            }
            UiAction::ZoomIn => {
                self.zoom.increase();
            }
            UiAction::ZoomOut => {
                self.zoom.decrease();
            }
            UiAction::SetZoom(value) => {
                self.zoom.set(*value)?;
            }
            UiAction::ResetZoom => {
                self.zoom.reset();
            }
            UiAction::ToggleProperties => {
                self.properties.visible = !self.properties.visible;
            }
            UiAction::ToggleNotes => {
                self.notes.visible = !self.notes.visible;
            }
            UiAction::ToggleFullscreen => {
                self.slideshow.toggle_fullscreen();
            }
            UiAction::StartSlideshow(start) => {
                self.slideshow.start(*start, self.thumbnails.selected)?;
            }
            UiAction::StopSlideshow => {
                self.slideshow.stop();
            }
            UiAction::KeyPress(key) => {
                if self.slideshow.mode == SlideshowMode::Presenting {
                    if let Some(index) = self.slideshow.handle_key(*key)? {
                        self.select_index_if_valid(index);
                    }
                } else if matches!(key, Key::F11) {
                    self.slideshow.toggle_fullscreen();
                }
            }
            UiAction::NewSlide
            | UiAction::DeleteCurrentSlide
            | UiAction::DuplicateCurrentSlide
            | UiAction::ReorderSlide { .. }
            | UiAction::SetSlideSize { .. }
            | UiAction::UpdateNotes { .. }
            | UiAction::ReplaceTextRun { .. } => {
                // These variants are handled by dispatch() before this method.
            }
        }
        self.refresh_status();
        Ok(())
    }

    /// Returns the selected zero-based slide index.
    pub fn current_slide_index(&self) -> Option<usize> {
        self.thumbnails.selected
    }

    /// Returns the selected slide id.
    pub fn current_slide_id(&self) -> Option<SlideId> {
        self.thumbnails
            .selected
            .and_then(|index| self.thumbnails.items.get(index))
            .map(|item| item.slide_id)
    }

    /// Returns shape IDs available on a slide for properties-panel hit testing.
    pub fn object_ids(document: &Presentation, slide_id: SlideId) -> Vec<ObjectId> {
        document
            .slides
            .iter()
            .find(|slide| slide.id == slide_id)
            .map(|slide| {
                slide
                    .objects
                    .iter()
                    .map(|object| object.id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    fn select_index_if_valid(&mut self, index: usize) {
        if index < self.thumbnails.len() {
            self.thumbnails.selected = Some(index);
            self.thumbnails.sync_selected_flags();
            self.sync_selected_view_state();
        }
    }

    /// Synchronises canvas, notes, and status fields with the currently
    /// selected slide using the private document snapshot.
    ///
    /// The notes draft is loaded from the selected slide's `notes` field, or
    /// cleared to an empty string when the slide has no notes or no slide is
    /// selected. Navigation callers always go through this method so the draft
    /// tracks the visible selection.
    fn sync_selected_view_state(&mut self) {
        let selected_id = self.current_slide_id();
        self.canvas.slide_id = selected_id;
        self.notes.slide_id = selected_id;
        self.notes.draft = selected_id
            .and_then(|id| {
                self.document
                    .slides
                    .iter()
                    .find(|slide| slide.id == id)
                    .and_then(|slide| slide.notes.as_deref())
            })
            .unwrap_or_default()
            .to_string();
        self.refresh_status();
    }

    fn refresh_status(&mut self) {
        self.status_bar.slide_count = self.thumbnails.len();
        self.status_bar.current_slide = self.current_slide_index().map(|index| index + 1);
        if self.status_bar.message.is_empty() {
            self.status_bar.message = if self.thumbnails.is_empty() {
                "No slides".to_string()
            } else {
                "Ready".to_string()
            };
        }
    }
}

/// Result error for [`EditorUi::dispatch`].
#[derive(Debug, Clone, PartialEq)]
pub enum DispatchError<E> {
    /// The local UI state rejected the action.
    Ui(UiError),
    /// The core command route rejected the action.
    Core(E),
}

impl<E: std::fmt::Display> std::fmt::Display for DispatchError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ui(error) => error.fmt(formatter),
            Self::Core(error) => formatter.write_fmt(format_args!("core route error: {error}")),
        }
    }
}

impl<E: std::fmt::Debug + std::fmt::Display> std::error::Error for DispatchError<E> {}

#[cfg(test)]
mod tests {
    use super::*;
    use slides_api::{ObjectKind, Point, Rect, Shape, Slide, SlideObject, Text, Transform};

    fn document(count: usize) -> Presentation {
        let mut presentation = Presentation {
            slides: (0..count)
                .map(|index| Slide::new(SlideId::new(index as u64 + 1)))
                .collect(),
            ..Presentation::default()
        };
        presentation.id = slides_api::PresentationId::new(1);
        presentation
    }

    fn object(id: u64, kind: ObjectKind) -> SlideObject {
        SlideObject::new(
            ObjectId::new(id),
            kind,
            Rect::new(Point::ZERO, 10.0, 10.0),
            Transform::IDENTITY,
        )
    }

    #[derive(Default)]
    struct MockRouter {
        actions: Vec<UiAction>,
    }

    impl EditorCommandRouter for MockRouter {
        type Error = &'static str;

        fn route(&mut self, action: &UiAction) -> Result<(), Self::Error> {
            self.actions.push(action.clone());
            Ok(())
        }
    }

    #[test]
    fn thumbnail_navigation_is_bounded_in_both_directions() {
        let mut ui = EditorUi::from_document(&document(3));
        assert_eq!(ui.current_slide_index(), Some(0));

        ui.apply_local_action(&UiAction::PreviousSlide).unwrap();
        assert_eq!(ui.current_slide_index(), Some(0));
        ui.apply_local_action(&UiAction::LastSlide).unwrap();
        assert_eq!(ui.current_slide_index(), Some(2));
        ui.apply_local_action(&UiAction::NextSlide).unwrap();
        assert_eq!(ui.current_slide_index(), Some(2));
    }

    #[test]
    fn empty_document_rejects_selection_navigation() {
        let mut ui = EditorUi::from_document(&document(0));
        assert_eq!(ui.current_slide_index(), None);
        assert_eq!(
            ui.apply_local_action(&UiAction::NextSlide),
            Err(UiError::NoSlides)
        );
    }

    #[test]
    fn core_mutations_are_routed_without_local_document_mutation() {
        let mut ui = EditorUi::from_document(&document(2));
        let mut router = MockRouter::default();
        ui.dispatch(UiAction::NewSlide, &mut router).unwrap();
        ui.dispatch(
            UiAction::SetSlideSize {
                slide_id: SlideId::new(1),
                width: 1920,
                height: 1080,
            },
            &mut router,
        )
        .unwrap();
        assert_eq!(
            router.actions,
            vec![
                UiAction::NewSlide,
                UiAction::SetSlideSize {
                    slide_id: SlideId::new(1),
                    width: 1920,
                    height: 1080,
                },
            ]
        );
        assert_eq!(ui.thumbnails.len(), 2);
        assert!(ui.status_bar.dirty);
    }

    #[test]
    fn local_action_does_not_cross_core_boundary() {
        let mut ui = EditorUi::from_document(&document(1));
        let mut router = MockRouter::default();
        ui.dispatch(UiAction::ToggleNotes, &mut router).unwrap();
        assert!(router.actions.is_empty());
        assert!(!ui.notes.visible);
    }

    #[test]
    fn fullscreen_and_zoom_state_are_clamped_and_toggleable() {
        let mut ui = EditorUi::from_document(&document(1));
        ui.apply_local_action(&UiAction::SetZoom(100.0)).unwrap();
        assert_eq!(ui.zoom.value, MAX_ZOOM);
        ui.apply_local_action(&UiAction::ZoomOut).unwrap();
        assert!(ui.zoom.value < MAX_ZOOM);
        ui.apply_local_action(&UiAction::SetZoom(-100.0)).unwrap();
        assert_eq!(ui.zoom.value, MIN_ZOOM);
        assert!(matches!(
            ui.apply_local_action(&UiAction::SetZoom(f32::NAN)),
            Err(UiError::InvalidZoom { .. })
        ));
        assert!(!ui.slideshow.fullscreen);
        ui.apply_local_action(&UiAction::ToggleFullscreen).unwrap();
        assert!(ui.slideshow.fullscreen);
        ui.apply_local_action(&UiAction::ToggleFullscreen).unwrap();
        assert!(!ui.slideshow.fullscreen);
    }

    #[test]
    fn slideshow_starts_first_or_current_and_handles_keyboard_bounds() {
        let mut ui = EditorUi::from_document(&document(3));
        ui.apply_local_action(&UiAction::SelectSlide(1)).unwrap();
        ui.apply_local_action(&UiAction::StartSlideshow(SlideshowStart::Current))
            .unwrap();
        assert_eq!(ui.slideshow.current, Some(1));
        ui.apply_local_action(&UiAction::KeyPress(Key::Right))
            .unwrap();
        assert_eq!(ui.slideshow.current, Some(2));
        ui.apply_local_action(&UiAction::KeyPress(Key::Right))
            .unwrap();
        assert_eq!(ui.slideshow.current, Some(2));
        ui.apply_local_action(&UiAction::KeyPress(Key::Home))
            .unwrap();
        assert_eq!(ui.slideshow.current, Some(0));
        ui.apply_local_action(&UiAction::KeyPress(Key::Escape))
            .unwrap();
        assert_eq!(ui.slideshow.mode, SlideshowMode::Stopped);

        ui.apply_local_action(&UiAction::StartSlideshow(SlideshowStart::First))
            .unwrap();
        assert_eq!(ui.slideshow.current, Some(0));
    }

    #[test]
    fn slideshow_transition_is_simple_and_progresses_to_completion() {
        let mut controller = SlideshowController::default();
        controller.set_slide_count(2);
        controller.set_transition(SimpleTransition::Fade);
        controller.start(SlideshowStart::First, None).unwrap();
        controller.next().unwrap();
        assert_eq!(controller.transition_progress, 0.0);
        assert!(controller.transition_active);
        assert!(controller.advance_transition(0.5));
        assert!(!controller.advance_transition(0.5));
        assert!(!controller.transition_active);
    }

    #[test]
    fn keyboard_f11_toggles_fullscreen_at_editor_boundary() {
        let mut ui = EditorUi::from_document(&document(1));
        ui.apply_local_action(&UiAction::KeyPress(Key::F11))
            .unwrap();
        assert!(ui.slideshow.fullscreen);
    }

    #[test]
    fn properties_can_derive_object_ids_from_public_slide_objects() {
        let mut slide = Slide::new(SlideId::new(7));
        slide
            .add_object(object(11, ObjectKind::Text(Text::plain("title"))))
            .unwrap();
        slide
            .add_object(object(12, ObjectKind::Shape(Shape::default())))
            .unwrap();
        slide
            .add_object(object(13, ObjectKind::Shape(Shape::default())))
            .unwrap();
        let document = Presentation {
            slides: vec![slide],
            ..Presentation::default()
        };
        assert_eq!(
            EditorUi::object_ids(&document, SlideId::new(7)),
            vec![ObjectId::new(11), ObjectId::new(12), ObjectId::new(13)]
        );
    }

    fn presentation_with_notes(count: usize) -> Presentation {
        let mut presentation = Presentation::new(slides_api::PresentationId::new(1));
        for index in 0..count {
            let mut slide = Slide::new(SlideId::new(index as u64 + 1));
            slide.notes = Some(format!("Notes for slide {}", index + 1));
            presentation.slides.push(slide);
        }
        presentation
    }

    fn presentation_with_mixed_notes() -> Presentation {
        let mut presentation = Presentation::new(slides_api::PresentationId::new(1));
        let mut first = Slide::new(SlideId::new(1));
        first.notes = Some("Hello".to_string());
        let mut second = Slide::new(SlideId::new(2));
        second.notes = Some("World".to_string());
        let third = Slide::new(SlideId::new(3));
        // third.notes = None
        presentation.slides.push(first);
        presentation.slides.push(second);
        presentation.slides.push(third);
        presentation
    }

    #[test]
    fn navigation_between_two_notes_values_reloads_draft() {
        let presentation = presentation_with_notes(2);
        let mut ui = EditorUi::from_document(&presentation);
        assert_eq!(ui.notes.draft, "Notes for slide 1");
        ui.apply_local_action(&UiAction::NextSlide).unwrap();
        assert_eq!(ui.notes.draft, "Notes for slide 2");
        ui.apply_local_action(&UiAction::PreviousSlide).unwrap();
        assert_eq!(ui.notes.draft, "Notes for slide 1");
    }

    #[test]
    fn navigation_to_slide_without_notes_clears_draft() {
        let presentation = presentation_with_mixed_notes();
        let mut ui = EditorUi::from_document(&presentation);
        assert_eq!(ui.notes.draft, "Hello");
        ui.apply_local_action(&UiAction::NextSlide).unwrap();
        assert_eq!(ui.notes.draft, "World");
        ui.apply_local_action(&UiAction::NextSlide).unwrap();
        assert_eq!(ui.notes.draft, "");
    }

    #[test]
    fn selected_slide_persists_through_set_slide_size_and_update_notes() {
        let presentation = presentation_with_mixed_notes();
        let mut editor = PresentationEditor::new(presentation).unwrap();
        let mut ui = EditorUi::from_document(editor.presentation());
        ui.apply_local_action(&UiAction::NextSlide).unwrap();
        let selected_before = ui.current_slide_id();
        assert_eq!(selected_before, Some(SlideId::new(2)));

        ui.dispatch_to_core(
            UiAction::SetSlideSize {
                slide_id: SlideId::new(2),
                width: 1280,
                height: 720,
            },
            &mut editor,
        )
        .unwrap();
        assert_eq!(ui.current_slide_id(), Some(SlideId::new(2)));

        ui.dispatch_to_core(
            UiAction::UpdateNotes {
                slide_id: SlideId::new(2),
                text: "Updated world".to_string(),
            },
            &mut editor,
        )
        .unwrap();
        assert_eq!(ui.current_slide_id(), Some(SlideId::new(2)));
        assert_eq!(ui.notes.draft, "Updated world");
    }

    #[test]
    fn replace_text_run_intent_reaches_core_and_preserves_formatting() {
        let mut presentation = document(1);
        let mut text = Text::plain("старый текст");
        text.paragraphs[0].runs[0].formatting.italic = true;
        presentation.slides[0]
            .add_object(object(7, ObjectKind::Text(text)))
            .unwrap();
        let mut editor = PresentationEditor::new(presentation).unwrap();
        let mut ui = EditorUi::from_document(editor.presentation());
        let action = UiAction::ReplaceTextRun {
            slide_id: SlideId::new(1),
            object_id: ObjectId::new(7),
            paragraph_index: 0,
            run_index: 0,
            text: "новый текст".to_owned(),
        };
        assert!(action.is_core_command());

        ui.dispatch_to_core(action, &mut editor).unwrap();

        let updated = editor.object(SlideId::new(1), ObjectId::new(7)).unwrap();
        let ObjectKind::Text(text) = &updated.kind else {
            panic!("expected text object")
        };
        let run = &text.paragraphs[0].runs[0];
        assert_eq!(run.text, "новый текст");
        assert!(run.formatting.italic);
        assert_eq!(ui.ribbon.last_action.as_deref(), Some("Replace text run"));
        assert!(ui.status_bar.dirty);
    }

    #[test]
    fn invalid_text_run_intent_does_not_change_ui_or_document() {
        let mut presentation = document(1);
        presentation.slides[0]
            .add_object(object(7, ObjectKind::Text(Text::plain("старый"))))
            .unwrap();
        let mut editor = PresentationEditor::new(presentation).unwrap();
        let mut ui = EditorUi::from_document(editor.presentation());
        let before_ui = ui.clone();
        let before_editor = editor.clone();

        let result = ui.dispatch_to_core(
            UiAction::ReplaceTextRun {
                slide_id: SlideId::new(1),
                object_id: ObjectId::new(7),
                paragraph_index: 0,
                run_index: 99,
                text: "не должно примениться".to_owned(),
            },
            &mut editor,
        );

        assert!(matches!(
            result,
            Err(DispatchError::Core(EditorError::InvalidIndex { .. }))
        ));
        assert_eq!(ui, before_ui);
        assert_eq!(editor, before_editor);
    }

    #[test]
    fn dispatch_to_core_failure_leaves_ui_and_editor_unchanged() {
        let presentation = presentation_with_notes(1);
        let mut editor = PresentationEditor::new(presentation).unwrap();
        let mut ui = EditorUi::from_document(editor.presentation());
        let ui_before = ui.clone();
        let editor_before = editor.clone();

        let result = ui
            .dispatch_to_core(UiAction::DeleteCurrentSlide, &mut editor)
            .unwrap_err();
        assert!(matches!(
            result,
            DispatchError::Core(slides_core::EditorError::CannotDeleteLastSlide)
        ));
        assert_eq!(ui, ui_before);
        assert_eq!(editor, editor_before);
    }

    #[test]
    fn dispatch_to_core_routes_new_duplicate_reorder_delete() {
        let presentation = presentation_with_notes(3);
        let mut editor = PresentationEditor::new(presentation).unwrap();
        let mut ui = EditorUi::from_document(editor.presentation());

        // NewSlide: adds a blank slide after the current selection.
        ui.dispatch_to_core(UiAction::NewSlide, &mut editor)
            .unwrap();
        assert_eq!(ui.thumbnails.len(), 4);
        assert!(ui.status_bar.dirty);
        assert!(ui.ribbon.last_action.is_some());

        // DuplicateCurrentSlide: duplicates slide 1 immediately after it.
        ui.apply_local_action(&UiAction::SelectSlide(0)).unwrap();
        let source_id = ui.current_slide_id().unwrap();
        ui.dispatch_to_core(UiAction::DuplicateCurrentSlide, &mut editor)
            .unwrap();
        assert_eq!(ui.thumbnails.len(), 5);
        assert!(ui.thumbnails.items.iter().any(|item| {
            item.slide_id != source_id
                && editor
                    .presentation()
                    .slides
                    .iter()
                    .find(|slide| slide.id == source_id)
                    .map(|slide| slide.objects.len())
                    .unwrap_or(0)
                    == editor
                        .presentation()
                        .slides
                        .iter()
                        .find(|slide| slide.id == item.slide_id)
                        .map(|slide| slide.objects.len())
                        .unwrap_or(0)
        }));

        // ReorderSlide: move the duplicate back to position 0.
        let duplicate_index = ui
            .thumbnails
            .items
            .iter()
            .position(|item| item.slide_id != source_id)
            .unwrap();
        let duplicate_id = ui.thumbnails.items[duplicate_index].slide_id;
        ui.dispatch_to_core(
            UiAction::ReorderSlide {
                from: duplicate_index,
                to: 0,
            },
            &mut editor,
        )
        .unwrap();
        assert_eq!(ui.thumbnails.items[0].slide_id, duplicate_id);

        // DeleteCurrentSlide: remove the duplicate; back to 4 slides.
        ui.dispatch_to_core(UiAction::DeleteCurrentSlide, &mut editor)
            .unwrap();
        assert_eq!(ui.thumbnails.len(), 4);
    }

    #[test]
    fn from_pptx_forwards_bytes_and_returns_read_only_snapshot() {
        let document = document(1);
        struct SuccessEngine {
            received: std::cell::RefCell<Vec<u8>>,
            document: Presentation,
        }

        impl PresentationEngine for SuccessEngine {
            fn load(&self, bytes: &[u8]) -> Result<Presentation, EngineError> {
                *self.received.borrow_mut() = bytes.to_vec();
                Ok(self.document.clone())
            }

            fn save(&self, _presentation: &Presentation) -> Result<Vec<u8>, EngineError> {
                unimplemented!("read-only snapshot should not save")
            }
        }

        let bytes = b"valid bytes";
        let engine = SuccessEngine {
            received: std::cell::RefCell::new(Vec::new()),
            document: document.clone(),
        };
        let ui = EditorUi::from_pptx(&engine, bytes).unwrap();
        assert!(ui.read_only());
        assert_eq!(*engine.received.borrow(), bytes);
        assert_eq!(ui.thumbnails.len(), document.slides.len());
    }

    #[test]
    fn public_pptx_fixture_navigation_selects_the_imported_slide() {
        const SUPPORTED: &[u8] =
            include_bytes!("../../../../fixtures/public/slides/supported-roundtrip.pptx");

        let engine = slides_pptx_adapter::office_oxide_adapter();
        let document = engine.load(SUPPORTED).unwrap();
        let mut ui = EditorUi::from_pptx(&engine, SUPPORTED).unwrap();
        assert!(ui.read_only());
        assert_eq!(ui.thumbnails.len(), 2);
        assert_eq!(ui.current_slide_index(), Some(0));
        assert_eq!(ui.current_slide_id(), Some(document.slides[0].id));

        ui.apply_local_action(&UiAction::NextSlide).unwrap();
        assert_eq!(ui.current_slide_index(), Some(1));
        assert_eq!(ui.current_slide_id(), Some(document.slides[1].id));
        assert_eq!(ui.canvas.slide_id, Some(document.slides[1].id));
        assert_eq!(ui.notes.slide_id, Some(document.slides[1].id));
        assert!(!ui.thumbnails.items[0].selected);
        assert!(ui.thumbnails.items[1].selected);

        ui.apply_local_action(&UiAction::PreviousSlide).unwrap();
        assert_eq!(ui.current_slide_id(), Some(document.slides[0].id));
        assert!(ui.thumbnails.items[0].selected);
        assert!(!ui.thumbnails.items[1].selected);
    }

    #[test]
    fn from_pptx_preserves_engine_error_verbatim() {
        let expected = EngineError::LimitExceeded {
            limit: 10,
            actual: 11,
            context: "slides".into(),
        };
        struct LimitEngine(EngineError);

        impl PresentationEngine for LimitEngine {
            fn load(&self, _bytes: &[u8]) -> Result<Presentation, EngineError> {
                Err(self.0.clone())
            }

            fn save(&self, _presentation: &Presentation) -> Result<Vec<u8>, EngineError> {
                unimplemented!("read-only snapshot should not save")
            }
        }

        let error = EditorUi::from_pptx(&LimitEngine(expected.clone()), b"bytes")
            .expect_err("load should fail before UI construction");
        assert_eq!(error, expected);
    }

    #[test]
    fn replace_from_pptx_error_preserves_ui_state() {
        let error = EngineError::InvalidFormat;
        struct FailingEngine(EngineError);

        impl PresentationEngine for FailingEngine {
            fn load(&self, _bytes: &[u8]) -> Result<Presentation, EngineError> {
                Err(self.0.clone())
            }

            fn save(&self, _presentation: &Presentation) -> Result<Vec<u8>, EngineError> {
                unimplemented!("read-only snapshot should not save")
            }
        }

        let mut ui = EditorUi::from_document(&document(2));
        ui.zoom.value = 2.0;
        ui.apply_local_action(&UiAction::ToggleProperties).unwrap();
        let ui_before = ui.clone();
        let result = ui.replace_from_pptx(&FailingEngine(error.clone()), b"bytes");
        assert_eq!(result.unwrap_err(), error);
        assert_eq!(ui, ui_before);
    }

    #[test]
    fn read_only_blocks_core_commands_but_allows_local_navigation() {
        let document = document(2);
        struct SuccessEngine {
            received: std::cell::RefCell<Vec<u8>>,
            document: Presentation,
        }

        impl PresentationEngine for SuccessEngine {
            fn load(&self, bytes: &[u8]) -> Result<Presentation, EngineError> {
                *self.received.borrow_mut() = bytes.to_vec();
                Ok(self.document.clone())
            }

            fn save(&self, _presentation: &Presentation) -> Result<Vec<u8>, EngineError> {
                unimplemented!("read-only snapshot should not save")
            }
        }

        let engine = SuccessEngine {
            received: std::cell::RefCell::new(Vec::new()),
            document: document.clone(),
        };
        let mut ui = EditorUi::from_pptx(&engine, b"pptx").unwrap();
        let mut router = MockRouter::default();
        let error = ui.dispatch(UiAction::NewSlide, &mut router).unwrap_err();
        assert!(matches!(error, DispatchError::Ui(UiError::ReadOnly)));
        assert!(router.actions.is_empty());
        assert!(!ui.status_bar.dirty);
        ui.apply_local_action(&UiAction::SelectSlide(1)).unwrap();
        assert_eq!(ui.current_slide_index(), Some(1));
        ui.apply_local_action(&UiAction::ZoomIn).unwrap();
        ui.apply_local_action(&UiAction::StartSlideshow(SlideshowStart::First))
            .unwrap();
    }

    #[test]
    fn read_only_blocks_dispatch_to_core_and_allows_editor() {
        struct LimitEngine;

        impl PresentationEngine for LimitEngine {
            fn load(&self, _bytes: &[u8]) -> Result<Presentation, EngineError> {
                Ok(document(1))
            }

            fn save(&self, _presentation: &Presentation) -> Result<Vec<u8>, EngineError> {
                unimplemented!("read-only snapshot should not save")
            }
        }

        let mut ui = EditorUi::from_pptx(&LimitEngine, b"pptx").unwrap();
        let mut editor = PresentationEditor::new(document(1)).unwrap();
        let error = ui
            .dispatch_to_core(UiAction::DeleteCurrentSlide, &mut editor)
            .unwrap_err();
        assert!(matches!(error, DispatchError::Ui(UiError::ReadOnly)));
        let text_error = ui
            .dispatch_to_core(
                UiAction::ReplaceTextRun {
                    slide_id: SlideId::new(1),
                    object_id: ObjectId::new(7),
                    paragraph_index: 0,
                    run_index: 0,
                    text: "rejected".to_owned(),
                },
                &mut editor,
            )
            .unwrap_err();
        assert!(matches!(text_error, DispatchError::Ui(UiError::ReadOnly)));
        assert_eq!(editor.presentation().slides.len(), 1);
    }

    #[test]
    fn from_document_remains_editable() {
        let presentation = document(1);
        let mut editor = PresentationEditor::new(presentation.clone()).unwrap();
        let mut ui = EditorUi::from_document(&presentation);
        assert!(!ui.read_only());
        ui.apply_local_action(&UiAction::ZoomIn).unwrap();
        ui.dispatch_to_core(UiAction::DuplicateCurrentSlide, &mut editor)
            .unwrap();
        assert_eq!(ui.thumbnails.len(), 2);
    }
}
