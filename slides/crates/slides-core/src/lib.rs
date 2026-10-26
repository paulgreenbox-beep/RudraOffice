//! Deterministic, `std`-only presentation editing state machine.
//!
//! `slides-core` owns editor state and all document mutations. The public
//! document and payload types are re-exported from [`slides_api`]; this crate
//! adds only command, event, selection, clipboard, and editor-state types.

#![warn(missing_docs)]

use std::collections::HashSet;
use std::fmt;

pub use slides_api::{
    Background, Color, DEFAULT_SLIDE_SIZE, Image, ImageFormat, Line, ObjectId, ObjectKind,
    Paragraph, ParagraphFormatting, Point, Presentation, PresentationDocument, PresentationId,
    Rect, Shape, ShapeKind, Size, Slide, SlideDirection, SlideId, SlideLayout, SlideObject, Table,
    Text, TextAlignment, TextFormatting, TextRun, Transform, Transition, ValidationError,
};

/// The currently selected slide and zero or more objects on that slide.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Selection {
    /// Selected slide, if any.
    pub slide_id: Option<SlideId>,
    /// Selected object IDs in deterministic visual order.
    pub object_ids: Vec<ObjectId>,
}

impl Selection {
    /// Creates a selection for a slide with no selected objects.
    pub const fn slide(slide_id: SlideId) -> Self {
        Self {
            slide_id: Some(slide_id),
            object_ids: Vec::new(),
        }
    }

    /// Returns whether this selection contains the object ID.
    pub fn contains(&self, object_id: ObjectId) -> bool {
        self.object_ids.contains(&object_id)
    }
}

/// A deterministic description of an editing command.
#[derive(Debug, Clone, PartialEq)]
pub enum EditorCommand {
    /// Add a blank slide at the requested zero-based index.
    AddSlide {
        /// Insertion index. The presentation length appends the slide.
        index: usize,
        /// Layout assigned to the new slide.
        layout: SlideLayout,
    },
    /// Delete a slide by ID.
    DeleteSlide {
        /// Slide to delete.
        slide_id: SlideId,
    },
    /// Duplicate a slide and insert the copy at the requested index.
    DuplicateSlide {
        /// Slide to duplicate.
        slide_id: SlideId,
        /// Insertion index for the copy.
        index: usize,
    },
    /// Reorder a slide to a new zero-based index.
    ReorderSlide {
        /// Slide to move.
        slide_id: SlideId,
        /// Destination index.
        to_index: usize,
    },
    /// Change a slide's layout.
    SetLayout {
        /// Target slide.
        slide_id: SlideId,
        /// New layout.
        layout: SlideLayout,
    },
    /// Change a slide's background.
    SetBackground {
        /// Target slide.
        slide_id: SlideId,
        /// New background.
        background: Background,
    },
    /// Change a slide's size.
    SetSlideSize {
        /// Target slide.
        slide_id: SlideId,
        /// New size.
        size: Size,
    },
    /// Update a slide's speaker notes.
    UpdateNotes {
        /// Target slide.
        slide_id: SlideId,
        /// New notes, or `None` to clear them.
        notes: Option<String>,
    },
    /// Set or clear a slide transition.
    SetTransition {
        /// Target slide.
        slide_id: SlideId,
        /// New transition, or `None` to clear it.
        transition: Option<Transition>,
    },
    /// Create an object with a deterministic ID assigned by the editor.
    CreateObject {
        /// Owning slide.
        slide_id: SlideId,
        /// Object payload.
        kind: ObjectKind,
        /// Object bounds.
        bounds: Rect,
        /// Object transform.
        transform: Transform,
    },
    /// Select one object and its slide.
    SelectObject {
        /// Owning slide.
        slide_id: SlideId,
        /// Object to select.
        object_id: ObjectId,
    },
    /// Select several objects and their slide.
    SelectObjects {
        /// Owning slide.
        slide_id: SlideId,
        /// Object IDs to select, in caller-provided order.
        object_ids: Vec<ObjectId>,
    },
    /// Select a slide and clear its object selection.
    SelectSlide {
        /// Slide to select.
        slide_id: SlideId,
    },
    /// Clear the current selection.
    ClearSelection,
    /// Move an object to an absolute position.
    MoveObject {
        /// Owning slide.
        slide_id: SlideId,
        /// Object to move.
        object_id: ObjectId,
        /// New top-left position.
        position: Point,
    },
    /// Move an object by a finite delta.
    MoveObjectBy {
        /// Owning slide.
        slide_id: SlideId,
        /// Object to move.
        object_id: ObjectId,
        /// Translation delta.
        delta: Point,
    },
    /// Resize an object to absolute bounds.
    ResizeObject {
        /// Owning slide.
        slide_id: SlideId,
        /// Object to resize.
        object_id: ObjectId,
        /// New bounds.
        bounds: Rect,
    },
    /// Rotate an object by writing the angle into its transform.
    RotateObject {
        /// Owning slide.
        slide_id: SlideId,
        /// Object to rotate.
        object_id: ObjectId,
        /// Absolute rotation in degrees.
        angle_degrees: f32,
    },
    /// Delete an object.
    DeleteObject {
        /// Owning slide.
        slide_id: SlideId,
        /// Object to delete.
        object_id: ObjectId,
    },
    /// Copy the specified objects to the editor clipboard.
    CopyObjects {
        /// Owning slide.
        slide_id: SlideId,
        /// Objects to copy, in visual order.
        object_ids: Vec<ObjectId>,
    },
    /// Paste clipboard objects onto a slide.
    PasteObjects {
        /// Destination slide.
        slide_id: SlideId,
        /// Top-left position for the pasted group.
        position: Point,
    },
    /// Duplicate one object in place with a new ID.
    DuplicateObject {
        /// Owning slide.
        slide_id: SlideId,
        /// Object to duplicate.
        object_id: ObjectId,
    },
    /// Bring an object to the front of its slide.
    BringToFront {
        /// Owning slide.
        slide_id: SlideId,
        /// Object to move in the layer stack.
        object_id: ObjectId,
    },
    /// Send an object to the back of its slide.
    SendToBack {
        /// Owning slide.
        slide_id: SlideId,
        /// Object to move in the layer stack.
        object_id: ObjectId,
    },
    /// Move an object one layer toward the front.
    BringForward {
        /// Owning slide.
        slide_id: SlideId,
        /// Object to move in the layer stack.
        object_id: ObjectId,
    },
    /// Move an object one layer toward the back.
    SendBackward {
        /// Owning slide.
        slide_id: SlideId,
        /// Object to move in the layer stack.
        object_id: ObjectId,
    },
    /// Align multiple objects horizontally or vertically.
    AlignObjects {
        /// Owning slide.
        slide_id: SlideId,
        /// Objects to align. At least two are required.
        object_ids: Vec<ObjectId>,
        /// Alignment operation.
        alignment: ObjectAlignment,
    },
    /// Replace the content of one existing text run without changing its formatting.
    ReplaceTextRun {
        /// Owning slide.
        slide_id: SlideId,
        /// Text object containing the run.
        object_id: ObjectId,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// Zero-based run index within the paragraph.
        run_index: usize,
        /// Replacement content.
        text: String,
    },
    /// Apply one run formatting value to every run in a text object.
    SetTextFormatting {
        /// Owning slide.
        slide_id: SlideId,
        /// Text object to format.
        object_id: ObjectId,
        /// New run formatting.
        formatting: TextFormatting,
    },
    /// Apply one text color while preserving every run's other formatting.
    SetTextColor {
        /// Owning slide.
        slide_id: SlideId,
        /// Text object to format.
        object_id: ObjectId,
        /// New color for every run in the text object.
        color: Color,
    },
    /// Apply paragraph formatting to one paragraph in a text object.
    SetParagraphFormatting {
        /// Owning slide.
        slide_id: SlideId,
        /// Text object to format.
        object_id: ObjectId,
        /// Zero-based paragraph index.
        paragraph_index: usize,
        /// New paragraph formatting.
        formatting: ParagraphFormatting,
    },
}

/// Constructs or classifies alignment behavior for object bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectAlignment {
    /// Match the left edge of the first selected object.
    Left,
    /// Match the horizontal center of the first selected object.
    Center,
    /// Match the right edge of the first selected object.
    Right,
    /// Match the top edge of the first selected object.
    Top,
    /// Match the vertical center of the first selected object.
    Middle,
    /// Match the bottom edge of the first selected object.
    Bottom,
}

/// A deterministic description of one successful state mutation.
#[derive(Debug, Clone, PartialEq)]
pub enum EditorEvent {
    /// A slide was inserted.
    SlideAdded {
        /// New slide ID.
        slide_id: SlideId,
        /// New slide index.
        index: usize,
    },
    /// A slide was deleted.
    SlideDeleted {
        /// Deleted slide ID.
        slide_id: SlideId,
    },
    /// A slide was duplicated.
    SlideDuplicated {
        /// Source slide ID.
        source_slide_id: SlideId,
        /// New slide ID.
        slide_id: SlideId,
        /// New slide index.
        index: usize,
    },
    /// A slide changed order.
    SlideReordered {
        /// Moved slide ID.
        slide_id: SlideId,
        /// Original index.
        from_index: usize,
        /// New index.
        to_index: usize,
    },
    /// A slide layout changed.
    LayoutChanged {
        /// Target slide ID.
        slide_id: SlideId,
        /// New layout.
        layout: SlideLayout,
    },
    /// A slide background changed.
    BackgroundChanged {
        /// Target slide ID.
        slide_id: SlideId,
        /// New background.
        background: Background,
    },
    /// A slide size changed.
    SlideSizeChanged {
        /// Target slide ID.
        slide_id: SlideId,
        /// New size.
        size: Size,
    },
    /// A slide's notes changed.
    NotesChanged {
        /// Target slide ID.
        slide_id: SlideId,
        /// New notes, or `None` if cleared.
        notes: Option<String>,
    },
    /// A slide transition changed.
    TransitionChanged {
        /// Target slide ID.
        slide_id: SlideId,
        /// New transition.
        transition: Option<Transition>,
    },
    /// An object was created.
    ObjectCreated {
        /// Owning slide ID.
        slide_id: SlideId,
        /// New object ID.
        object_id: ObjectId,
    },
    /// An object was deleted.
    ObjectDeleted {
        /// Owning slide ID.
        slide_id: SlideId,
        /// Deleted object ID.
        object_id: ObjectId,
    },
    /// An object was duplicated.
    ObjectDuplicated {
        /// Owning slide ID.
        slide_id: SlideId,
        /// Source object ID.
        source_object_id: ObjectId,
        /// New object ID.
        object_id: ObjectId,
    },
    /// A selection changed.
    SelectionChanged {
        /// New selection.
        selection: Selection,
    },
    /// An object moved.
    ObjectMoved {
        /// Owning slide ID.
        slide_id: SlideId,
        /// Moved object ID.
        object_id: ObjectId,
        /// New bounds.
        bounds: Rect,
    },
    /// An object was resized.
    ObjectResized {
        /// Owning slide ID.
        slide_id: SlideId,
        /// Resized object ID.
        object_id: ObjectId,
        /// New bounds.
        bounds: Rect,
    },
    /// An object's rotation changed.
    ObjectRotated {
        /// Owning slide ID.
        slide_id: SlideId,
        /// Rotated object ID.
        object_id: ObjectId,
        /// New absolute angle.
        angle_degrees: f32,
    },
    /// Objects were copied to the clipboard.
    ObjectsCopied {
        /// Number of copied objects.
        count: usize,
    },
    /// Objects were pasted from the clipboard.
    ObjectsPasted {
        /// Destination slide ID.
        slide_id: SlideId,
        /// New object IDs, in paste order.
        object_ids: Vec<ObjectId>,
    },
    /// A layer order changed.
    LayerOrderChanged {
        /// Owning slide ID.
        slide_id: SlideId,
        /// New object order, back to front.
        object_ids: Vec<ObjectId>,
    },
    /// Objects were aligned.
    ObjectsAligned {
        /// Owning slide ID.
        slide_id: SlideId,
        /// Aligned object IDs.
        object_ids: Vec<ObjectId>,
        /// Alignment used.
        alignment: ObjectAlignment,
    },
    /// The content of one text run changed.
    TextContentChanged {
        /// Owning slide ID.
        slide_id: SlideId,
        /// Text object ID.
        object_id: ObjectId,
        /// Changed paragraph index.
        paragraph_index: usize,
        /// Changed run index.
        run_index: usize,
    },
    /// Text run formatting changed.
    TextFormattingChanged {
        /// Owning slide ID.
        slide_id: SlideId,
        /// Formatted object ID.
        object_id: ObjectId,
    },
    /// Paragraph formatting changed.
    ParagraphFormattingChanged {
        /// Owning slide ID.
        slide_id: SlideId,
        /// Formatted object ID.
        object_id: ObjectId,
        /// Formatted paragraph index.
        paragraph_index: usize,
    },
}

/// Typed failure returned by [`PresentationEditor::apply`].
#[derive(Debug, Clone, PartialEq)]
pub enum EditorError {
    /// The initial or current presentation violates an API invariant.
    InvalidPresentation(ValidationError),
    /// A slide ID was not found.
    InvalidSlideId(SlideId),
    /// An object ID was not found on the requested slide.
    InvalidObjectId {
        /// Owning slide ID.
        slide_id: SlideId,
        /// Missing object ID.
        object_id: ObjectId,
    },
    /// An index is outside the valid range.
    InvalidIndex {
        /// Operation name.
        operation: &'static str,
        /// Requested index.
        index: usize,
        /// Exclusive upper bound.
        length: usize,
    },
    /// Deleting the last slide is not allowed.
    CannotDeleteLastSlide,
    /// A command supplied invalid geometry.
    InvalidGeometry,
    /// A command supplied an invalid transform.
    InvalidTransform,
    /// A command supplied an invalid slide size.
    InvalidSlideSize,
    /// A command supplied a non-finite or negative text value.
    InvalidTextFormatting,
    /// A command supplied a non-finite rotation angle.
    InvalidRotation,
    /// A command targeted a non-text object.
    NotTextObject {
        /// Owning slide ID.
        slide_id: SlideId,
        /// Target object ID.
        object_id: ObjectId,
    },
    /// A paragraph index is outside the text object.
    InvalidParagraphIndex {
        /// Requested index.
        index: usize,
        /// Number of paragraphs.
        length: usize,
    },

    /// The selected object list is empty, duplicated, or has fewer than two objects.
    InvalidObjectSelection,
    /// The clipboard has no objects.
    EmptyClipboard,
    /// A deterministic ID could not be allocated.
    IdExhausted,
    /// An arithmetic operation would produce invalid geometry.
    GeometryOverflow,
}

impl fmt::Display for EditorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPresentation(error) => write!(formatter, "invalid presentation: {error}"),
            Self::InvalidSlideId(id) => write!(formatter, "invalid slide id {id}"),
            Self::InvalidObjectId {
                slide_id,
                object_id,
            } => {
                write!(
                    formatter,
                    "invalid object id {object_id} on slide {slide_id}"
                )
            }
            Self::InvalidIndex {
                operation,
                index,
                length,
            } => write!(
                formatter,
                "invalid {operation} index {index}; valid length is {length}"
            ),
            Self::CannotDeleteLastSlide => formatter.write_str("cannot delete the last slide"),
            Self::InvalidGeometry => formatter.write_str("invalid geometry"),
            Self::InvalidTransform => formatter.write_str("invalid transform"),
            Self::InvalidSlideSize => formatter.write_str("invalid slide size"),
            Self::InvalidTextFormatting => formatter.write_str("invalid text formatting"),
            Self::InvalidRotation => formatter.write_str("invalid rotation"),
            Self::NotTextObject {
                slide_id,
                object_id,
            } => {
                write!(
                    formatter,
                    "object {object_id} on slide {slide_id} is not text"
                )
            }
            Self::InvalidParagraphIndex { index, length } => {
                write!(
                    formatter,
                    "invalid paragraph index {index}; text has {length} paragraphs"
                )
            }

            Self::InvalidObjectSelection => formatter.write_str("invalid object selection"),
            Self::EmptyClipboard => formatter.write_str("clipboard is empty"),
            Self::IdExhausted => formatter.write_str("deterministic identifier space exhausted"),
            Self::GeometryOverflow => formatter.write_str("geometry arithmetic overflowed"),
        }
    }
}

impl std::error::Error for EditorError {}

/// Mutable document editor with deterministic selection, clipboard, IDs, and events.
#[derive(Debug, Clone, PartialEq)]
pub struct PresentationEditor {
    presentation: Presentation,
    selection: Selection,
    clipboard: Vec<ClipboardObject>,
}

#[derive(Debug, Clone, PartialEq)]
struct ClipboardObject {
    object: SlideObject,
}

impl PresentationEditor {
    /// Creates an editor after validating the supplied presentation.
    pub fn new(presentation: Presentation) -> Result<Self, EditorError> {
        presentation
            .validate()
            .map_err(EditorError::InvalidPresentation)?;
        Ok(Self {
            presentation,
            selection: Selection::default(),
            clipboard: Vec::new(),
        })
    }

    /// Returns the current presentation by shared reference.
    pub const fn presentation(&self) -> &Presentation {
        &self.presentation
    }

    /// Returns the current selection.
    pub const fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Returns the number of objects in the clipboard.
    pub const fn clipboard_len(&self) -> usize {
        self.clipboard.len()
    }

    /// Returns the rotation angle of an object in degrees.
    pub fn rotation(&self, slide_id: SlideId, object_id: ObjectId) -> Result<f32, EditorError> {
        let object = self.object(slide_id, object_id)?;
        Ok(object.transform.rotation_degrees)
    }

    /// Returns a slide object by its owning slide and object IDs.
    pub fn object(
        &self,
        slide_id: SlideId,
        object_id: ObjectId,
    ) -> Result<&SlideObject, EditorError> {
        let slide = self.slide(slide_id)?;
        slide.object(object_id).ok_or(EditorError::InvalidObjectId {
            slide_id,
            object_id,
        })
    }

    /// Returns a slide by ID.
    pub fn slide(&self, slide_id: SlideId) -> Result<&Slide, EditorError> {
        self.presentation
            .slide(slide_id)
            .ok_or(EditorError::InvalidSlideId(slide_id))
    }

    /// Applies one command atomically and returns its deterministic events.
    ///
    /// The editor applies the command to a private clone and commits that clone
    /// only after every validation and mutation succeeds. Failed commands thus
    /// preserve the document, selection, clipboard, and event output (no events
    /// are returned on failure).
    pub fn apply(&mut self, command: EditorCommand) -> Result<Vec<EditorEvent>, EditorError> {
        let mut next = self.clone();
        let events = next.apply_mut(command)?;
        next.assert_invariants()?;
        *self = next;
        Ok(events)
    }

    /// Applies a borrowed command without requiring the caller to clone it.
    pub fn apply_ref(&mut self, command: &EditorCommand) -> Result<Vec<EditorEvent>, EditorError> {
        self.apply(command.clone())
    }

    fn apply_mut(&mut self, command: EditorCommand) -> Result<Vec<EditorEvent>, EditorError> {
        match command {
            EditorCommand::AddSlide { index, layout } => self.add_slide(index, layout),
            EditorCommand::DeleteSlide { slide_id } => self.delete_slide(slide_id),
            EditorCommand::DuplicateSlide { slide_id, index } => {
                self.duplicate_slide(slide_id, index)
            }
            EditorCommand::ReorderSlide { slide_id, to_index } => {
                self.reorder_slide(slide_id, to_index)
            }
            EditorCommand::SetLayout { slide_id, layout } => self.set_layout(slide_id, layout),
            EditorCommand::SetBackground {
                slide_id,
                background,
            } => self.set_background(slide_id, background),
            EditorCommand::SetSlideSize { slide_id, size } => self.set_slide_size(slide_id, size),
            EditorCommand::UpdateNotes { slide_id, notes } => self.update_notes(slide_id, notes),
            EditorCommand::SetTransition {
                slide_id,
                transition,
            } => self.set_transition(slide_id, transition),
            EditorCommand::CreateObject {
                slide_id,
                kind,
                bounds,
                transform,
            } => self.create_object(slide_id, kind, bounds, transform),
            EditorCommand::SelectObject {
                slide_id,
                object_id,
            } => self.select_object(slide_id, object_id),
            EditorCommand::SelectObjects {
                slide_id,
                object_ids,
            } => self.select_objects(slide_id, object_ids),
            EditorCommand::SelectSlide { slide_id } => self.select_slide(slide_id),
            EditorCommand::ClearSelection => {
                self.selection = Selection::default();
                Ok(vec![EditorEvent::SelectionChanged {
                    selection: self.selection.clone(),
                }])
            }
            EditorCommand::MoveObject {
                slide_id,
                object_id,
                position,
            } => self.move_object(slide_id, object_id, position),
            EditorCommand::MoveObjectBy {
                slide_id,
                object_id,
                delta,
            } => self.move_object_by(slide_id, object_id, delta),
            EditorCommand::ResizeObject {
                slide_id,
                object_id,
                bounds,
            } => self.resize_object(slide_id, object_id, bounds),
            EditorCommand::RotateObject {
                slide_id,
                object_id,
                angle_degrees,
            } => self.rotate_object(slide_id, object_id, angle_degrees),
            EditorCommand::DeleteObject {
                slide_id,
                object_id,
            } => self.delete_object(slide_id, object_id),
            EditorCommand::CopyObjects {
                slide_id,
                object_ids,
            } => self.copy_objects(slide_id, object_ids),
            EditorCommand::PasteObjects { slide_id, position } => {
                self.paste_objects(slide_id, position)
            }
            EditorCommand::DuplicateObject {
                slide_id,
                object_id,
            } => self.duplicate_object(slide_id, object_id),
            EditorCommand::BringToFront {
                slide_id,
                object_id,
            } => self.layer_order(slide_id, object_id, LayerOperation::Front),
            EditorCommand::SendToBack {
                slide_id,
                object_id,
            } => self.layer_order(slide_id, object_id, LayerOperation::Back),
            EditorCommand::BringForward {
                slide_id,
                object_id,
            } => self.layer_order(slide_id, object_id, LayerOperation::Forward),
            EditorCommand::SendBackward {
                slide_id,
                object_id,
            } => self.layer_order(slide_id, object_id, LayerOperation::Backward),
            EditorCommand::AlignObjects {
                slide_id,
                object_ids,
                alignment,
            } => self.align_objects(slide_id, object_ids, alignment),
            EditorCommand::ReplaceTextRun {
                slide_id,
                object_id,
                paragraph_index,
                run_index,
                text,
            } => self.replace_text_run(slide_id, object_id, paragraph_index, run_index, text),
            EditorCommand::SetTextFormatting {
                slide_id,
                object_id,
                formatting,
            } => self.set_text_formatting(slide_id, object_id, formatting),
            EditorCommand::SetTextColor {
                slide_id,
                object_id,
                color,
            } => self.set_text_color(slide_id, object_id, color),
            EditorCommand::SetParagraphFormatting {
                slide_id,
                object_id,
                paragraph_index,
                formatting,
            } => self.set_paragraph_formatting(slide_id, object_id, paragraph_index, formatting),
        }
    }

    fn add_slide(
        &mut self,
        index: usize,
        layout: SlideLayout,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        if index > self.presentation.slides.len() {
            return Err(EditorError::InvalidIndex {
                operation: "slide insertion",
                index,
                length: self.presentation.slides.len(),
            });
        }
        let id = next_slide_id(&self.presentation)?;
        self.presentation.slides.insert(
            index,
            Slide {
                id,
                layout,
                ..Slide::default()
            },
        );
        Ok(vec![EditorEvent::SlideAdded {
            slide_id: id,
            index,
        }])
    }

    fn delete_slide(&mut self, slide_id: SlideId) -> Result<Vec<EditorEvent>, EditorError> {
        let index = self.slide_index(slide_id)?;
        if self.presentation.slides.len() == 1 {
            return Err(EditorError::CannotDeleteLastSlide);
        }
        self.presentation.slides.remove(index);
        let mut events = vec![EditorEvent::SlideDeleted { slide_id }];
        if self.selection.slide_id == Some(slide_id) {
            let replacement =
                self.presentation.slides[index.min(self.presentation.slides.len() - 1)].id;
            self.selection = Selection::slide(replacement);
            events.push(EditorEvent::SelectionChanged {
                selection: self.selection.clone(),
            });
        }
        Ok(events)
    }

    fn duplicate_slide(
        &mut self,
        slide_id: SlideId,
        index: usize,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        if index > self.presentation.slides.len() {
            return Err(EditorError::InvalidIndex {
                operation: "slide insertion",
                index,
                length: self.presentation.slides.len(),
            });
        }
        let source_index = self.slide_index(slide_id)?;
        let mut duplicate = self.presentation.slides[source_index].clone();
        let new_id = next_slide_id(&self.presentation)?;
        duplicate.id = new_id;
        let source_object_ids = duplicate
            .objects
            .iter()
            .map(|object| object.id)
            .collect::<Vec<_>>();
        let mut allocated_object_ids: Vec<ObjectId> = Vec::with_capacity(duplicate.objects.len());
        for object in &mut duplicate.objects {
            let object_id = next_unused_id(
                source_object_ids
                    .iter()
                    .map(|id| id.get())
                    .chain(allocated_object_ids.iter().map(|id| id.get())),
            )?;
            object.id = ObjectId::new(object_id);
            allocated_object_ids.push(ObjectId::new(object_id));
        }
        self.presentation.slides.insert(index, duplicate);
        Ok(vec![EditorEvent::SlideDuplicated {
            source_slide_id: slide_id,
            slide_id: new_id,
            index,
        }])
    }

    fn reorder_slide(
        &mut self,
        slide_id: SlideId,
        to_index: usize,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        let from_index = self.slide_index(slide_id)?;
        if to_index >= self.presentation.slides.len() {
            return Err(EditorError::InvalidIndex {
                operation: "slide reorder",
                index: to_index,
                length: self.presentation.slides.len(),
            });
        }
        let slide = self.presentation.slides.remove(from_index);
        self.presentation.slides.insert(to_index, slide);
        Ok(vec![EditorEvent::SlideReordered {
            slide_id,
            from_index,
            to_index,
        }])
    }

    fn set_layout(
        &mut self,
        slide_id: SlideId,
        layout: SlideLayout,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        self.slide_mut(slide_id)?.layout = layout;
        Ok(vec![EditorEvent::LayoutChanged { slide_id, layout }])
    }

    fn set_background(
        &mut self,
        slide_id: SlideId,
        background: Background,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        self.slide_mut(slide_id)?.background = background;
        Ok(vec![EditorEvent::BackgroundChanged {
            slide_id,
            background,
        }])
    }

    fn set_slide_size(
        &mut self,
        slide_id: SlideId,
        size: Size,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        if !size.is_valid() {
            return Err(EditorError::InvalidSlideSize);
        }
        self.slide_mut(slide_id)?.size = size;
        Ok(vec![EditorEvent::SlideSizeChanged { slide_id, size }])
    }

    fn update_notes(
        &mut self,
        slide_id: SlideId,
        notes: Option<String>,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        self.slide_mut(slide_id)?.notes = notes.clone();
        Ok(vec![EditorEvent::NotesChanged { slide_id, notes }])
    }

    fn set_transition(
        &mut self,
        slide_id: SlideId,
        transition: Option<Transition>,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        if transition.is_some_and(|value| !value.is_valid()) {
            return Err(EditorError::InvalidPresentation(
                ValidationError::InvalidTransition { slide_id },
            ));
        }
        self.slide_mut(slide_id)?.transition = transition;
        Ok(vec![EditorEvent::TransitionChanged {
            slide_id,
            transition,
        }])
    }

    fn create_object(
        &mut self,
        slide_id: SlideId,
        kind: ObjectKind,
        bounds: Rect,
        transform: Transform,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        validate_geometry(bounds, transform)?;
        let id = {
            let slide = self.slide(slide_id)?;
            next_object_id(slide)?
        };
        let object = SlideObject::new(id, kind, bounds, transform);
        self.slide_mut(slide_id)?
            .add_object(object)
            .map_err(EditorError::InvalidPresentation)?;
        Ok(vec![EditorEvent::ObjectCreated {
            slide_id,
            object_id: id,
        }])
    }

    fn select_slide(&mut self, slide_id: SlideId) -> Result<Vec<EditorEvent>, EditorError> {
        self.slide(slide_id)?;
        self.selection = Selection::slide(slide_id);
        Ok(vec![EditorEvent::SelectionChanged {
            selection: self.selection.clone(),
        }])
    }

    fn select_object(
        &mut self,
        slide_id: SlideId,
        object_id: ObjectId,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        self.object(slide_id, object_id)?;
        self.selection = Selection {
            slide_id: Some(slide_id),
            object_ids: vec![object_id],
        };
        Ok(vec![EditorEvent::SelectionChanged {
            selection: self.selection.clone(),
        }])
    }

    fn select_objects(
        &mut self,
        slide_id: SlideId,
        object_ids: Vec<ObjectId>,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        validate_object_ids(self.slide(slide_id)?, &object_ids, 1)?;
        self.selection = Selection {
            slide_id: Some(slide_id),
            object_ids,
        };
        Ok(vec![EditorEvent::SelectionChanged {
            selection: self.selection.clone(),
        }])
    }

    fn move_object(
        &mut self,
        slide_id: SlideId,
        object_id: ObjectId,
        position: Point,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        if !position.is_valid() {
            return Err(EditorError::InvalidGeometry);
        }
        let object = self.object_mut(slide_id, object_id)?;
        let bounds = Rect::new(position, object.bounds.width, object.bounds.height);
        if !bounds.is_valid() {
            return Err(EditorError::InvalidGeometry);
        }
        object.bounds = bounds;
        Ok(vec![EditorEvent::ObjectMoved {
            slide_id,
            object_id,
            bounds,
        }])
    }

    fn move_object_by(
        &mut self,
        slide_id: SlideId,
        object_id: ObjectId,
        delta: Point,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        if !delta.is_valid() {
            return Err(EditorError::InvalidGeometry);
        }
        let object = self.object(slide_id, object_id)?;
        let position = Point::new(
            object.bounds.origin.x + delta.x,
            object.bounds.origin.y + delta.y,
        );
        if !position.is_valid() {
            return Err(EditorError::GeometryOverflow);
        }
        self.move_object(slide_id, object_id, position)
    }

    fn resize_object(
        &mut self,
        slide_id: SlideId,
        object_id: ObjectId,
        bounds: Rect,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        if !bounds.is_valid() {
            return Err(EditorError::InvalidGeometry);
        }
        self.object_mut(slide_id, object_id)?.bounds = bounds;
        Ok(vec![EditorEvent::ObjectResized {
            slide_id,
            object_id,
            bounds,
        }])
    }

    fn rotate_object(
        &mut self,
        slide_id: SlideId,
        object_id: ObjectId,
        angle_degrees: f32,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        if !angle_degrees.is_finite() {
            return Err(EditorError::InvalidRotation);
        }
        let object = self.object_mut(slide_id, object_id)?;
        object.transform.rotation_degrees = angle_degrees;
        Ok(vec![EditorEvent::ObjectRotated {
            slide_id,
            object_id,
            angle_degrees,
        }])
    }

    fn delete_object(
        &mut self,
        slide_id: SlideId,
        object_id: ObjectId,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        let slide = self.slide_mut(slide_id)?;
        let index = slide
            .objects
            .iter()
            .position(|object| object.id == object_id)
            .ok_or(EditorError::InvalidObjectId {
                slide_id,
                object_id,
            })?;
        slide.objects.remove(index);
        let was_selected = self.selection.contains(object_id);
        let previous_selection = self.selection.clone();
        self.selection.object_ids.retain(|id| *id != object_id);
        if self.selection.object_ids.is_empty() && self.selection.slide_id == Some(slide_id) {
            self.selection = Selection::slide(slide_id);
        }
        let mut events = vec![EditorEvent::ObjectDeleted {
            slide_id,
            object_id,
        }];
        if was_selected && self.selection != previous_selection {
            events.push(EditorEvent::SelectionChanged {
                selection: self.selection.clone(),
            });
        }
        Ok(events)
    }

    fn copy_objects(
        &mut self,
        slide_id: SlideId,
        object_ids: Vec<ObjectId>,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        validate_object_ids(self.slide(slide_id)?, &object_ids, 1)?;
        self.clipboard = object_ids
            .iter()
            .map(|id| {
                let object = self
                    .object(slide_id, *id)
                    .expect("validated object IDs must resolve");
                ClipboardObject {
                    object: object.clone(),
                }
            })
            .collect();
        Ok(vec![EditorEvent::ObjectsCopied {
            count: self.clipboard.len(),
        }])
    }

    fn paste_objects(
        &mut self,
        slide_id: SlideId,
        position: Point,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        if !position.is_valid() {
            return Err(EditorError::InvalidGeometry);
        }
        self.slide(slide_id)?;
        if self.clipboard.is_empty() {
            return Err(EditorError::EmptyClipboard);
        }
        let anchor = self.clipboard[0].object.bounds.origin;
        let delta = Point::new(position.x - anchor.x, position.y - anchor.y);
        if !delta.is_valid() {
            return Err(EditorError::GeometryOverflow);
        }
        let mut next_id = self.next_object_id_for_slide(slide_id)?;
        let mut used_ids: HashSet<ObjectId> = self
            .slide(slide_id)?
            .objects
            .iter()
            .map(|object| object.id)
            .collect();
        let mut pasted = Vec::with_capacity(self.clipboard.len());
        for item in &self.clipboard {
            let origin = Point::new(
                item.object.bounds.origin.x + delta.x,
                item.object.bounds.origin.y + delta.y,
            );
            let bounds = Rect::new(origin, item.object.bounds.width, item.object.bounds.height);
            if !bounds.is_valid() {
                return Err(EditorError::GeometryOverflow);
            }
            while used_ids.contains(&next_id) {
                next_id = next_id
                    .get()
                    .checked_add(1)
                    .map(ObjectId::new)
                    .ok_or(EditorError::IdExhausted)?;
            }
            let id = next_id;
            used_ids.insert(id);
            next_id = id
                .get()
                .checked_add(1)
                .map(ObjectId::new)
                .ok_or(EditorError::IdExhausted)?;
            let mut object = item.object.clone();
            object.id = id;
            object.bounds = bounds;
            pasted.push(object);
        }
        let ids = pasted.iter().map(|object| object.id).collect::<Vec<_>>();
        {
            let slide = self.slide_mut(slide_id)?;
            for object in pasted.iter() {
                slide
                    .add_object(object.clone())
                    .map_err(EditorError::InvalidPresentation)?;
            }
        }
        self.selection = Selection {
            slide_id: Some(slide_id),
            object_ids: ids.clone(),
        };
        Ok(vec![
            EditorEvent::ObjectsPasted {
                slide_id,
                object_ids: ids.clone(),
            },
            EditorEvent::SelectionChanged {
                selection: self.selection.clone(),
            },
        ])
    }

    fn duplicate_object(
        &mut self,
        slide_id: SlideId,
        object_id: ObjectId,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        let source = self.object(slide_id, object_id)?.clone();
        let new_id = self.next_object_id_for_slide(slide_id)?;
        let mut duplicate = source;
        duplicate.id = new_id;
        self.slide_mut(slide_id)?
            .add_object(duplicate)
            .map_err(EditorError::InvalidPresentation)?;
        Ok(vec![EditorEvent::ObjectDuplicated {
            slide_id,
            source_object_id: object_id,
            object_id: new_id,
        }])
    }

    fn layer_order(
        &mut self,
        slide_id: SlideId,
        object_id: ObjectId,
        operation: LayerOperation,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        let slide = self.slide_mut(slide_id)?;
        let index = slide
            .objects
            .iter()
            .position(|object| object.id == object_id)
            .ok_or(EditorError::InvalidObjectId {
                slide_id,
                object_id,
            })?;
        let destination = match operation {
            LayerOperation::Front => slide.objects.len() - 1,
            LayerOperation::Back => 0,
            LayerOperation::Forward => (index + 1).min(slide.objects.len() - 1),
            LayerOperation::Backward => index.saturating_sub(1),
        };
        let object = slide.objects.remove(index);
        slide.objects.insert(destination, object);
        Ok(vec![EditorEvent::LayerOrderChanged {
            slide_id,
            object_ids: slide.objects.iter().map(|object| object.id).collect(),
        }])
    }

    fn align_objects(
        &mut self,
        slide_id: SlideId,
        object_ids: Vec<ObjectId>,
        alignment: ObjectAlignment,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        validate_object_ids(self.slide(slide_id)?, &object_ids, 2)?;
        let reference = self.object(slide_id, object_ids[0])?.bounds;
        let slide = self.slide_mut(slide_id)?;
        for object_id in &object_ids[1..] {
            let object = slide
                .object_mut(*object_id)
                .ok_or(EditorError::InvalidObjectId {
                    slide_id,
                    object_id: *object_id,
                })?;
            let origin = match alignment {
                ObjectAlignment::Left => Point::new(reference.origin.x, object.bounds.origin.y),
                ObjectAlignment::Center => Point::new(
                    reference.origin.x + (reference.width - object.bounds.width) / 2.0,
                    object.bounds.origin.y,
                ),
                ObjectAlignment::Right => Point::new(
                    reference.origin.x + reference.width - object.bounds.width,
                    object.bounds.origin.y,
                ),
                ObjectAlignment::Top => Point::new(object.bounds.origin.x, reference.origin.y),
                ObjectAlignment::Middle => Point::new(
                    object.bounds.origin.x,
                    reference.origin.y + (reference.height - object.bounds.height) / 2.0,
                ),
                ObjectAlignment::Bottom => Point::new(
                    object.bounds.origin.x,
                    reference.origin.y + reference.height - object.bounds.height,
                ),
            };
            let bounds = Rect::new(origin, object.bounds.width, object.bounds.height);
            if !bounds.is_valid() {
                return Err(EditorError::GeometryOverflow);
            }
            object.bounds = bounds;
        }
        Ok(vec![EditorEvent::ObjectsAligned {
            slide_id,
            object_ids,
            alignment,
        }])
    }

    fn replace_text_run(
        &mut self,
        slide_id: SlideId,
        object_id: ObjectId,
        paragraph_index: usize,
        run_index: usize,
        replacement: String,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        let object = self.object_mut(slide_id, object_id)?;
        let text = match &mut object.kind {
            ObjectKind::Text(text) => text,
            _ => {
                return Err(EditorError::NotTextObject {
                    slide_id,
                    object_id,
                });
            }
        };
        let paragraph_length = text.paragraphs.len();
        let paragraph =
            text.paragraphs
                .get_mut(paragraph_index)
                .ok_or(EditorError::InvalidParagraphIndex {
                    index: paragraph_index,
                    length: paragraph_length,
                })?;
        let run_length = paragraph.runs.len();
        let run = paragraph
            .runs
            .get_mut(run_index)
            .ok_or(EditorError::InvalidIndex {
                operation: "text run",
                index: run_index,
                length: run_length,
            })?;
        run.text = replacement;
        Ok(vec![EditorEvent::TextContentChanged {
            slide_id,
            object_id,
            paragraph_index,
            run_index,
        }])
    }

    fn set_text_formatting(
        &mut self,
        slide_id: SlideId,
        object_id: ObjectId,
        formatting: TextFormatting,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        validate_text_formatting(&formatting)?;
        let object = self.object_mut(slide_id, object_id)?;
        let text = match &mut object.kind {
            ObjectKind::Text(text) => text,
            _ => {
                return Err(EditorError::NotTextObject {
                    slide_id,
                    object_id,
                });
            }
        };
        for paragraph in &mut text.paragraphs {
            for run in &mut paragraph.runs {
                run.formatting = formatting.clone();
            }
        }
        Ok(vec![EditorEvent::TextFormattingChanged {
            slide_id,
            object_id,
        }])
    }

    fn set_text_color(
        &mut self,
        slide_id: SlideId,
        object_id: ObjectId,
        color: Color,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        let object = self.object_mut(slide_id, object_id)?;
        let text = match &mut object.kind {
            ObjectKind::Text(text) => text,
            _ => {
                return Err(EditorError::NotTextObject {
                    slide_id,
                    object_id,
                });
            }
        };
        for paragraph in &mut text.paragraphs {
            for run in &mut paragraph.runs {
                run.formatting.color = color;
            }
        }
        Ok(vec![EditorEvent::TextFormattingChanged {
            slide_id,
            object_id,
        }])
    }

    fn set_paragraph_formatting(
        &mut self,
        slide_id: SlideId,
        object_id: ObjectId,
        paragraph_index: usize,
        formatting: ParagraphFormatting,
    ) -> Result<Vec<EditorEvent>, EditorError> {
        validate_paragraph_formatting(&formatting)?;
        let object = self.object_mut(slide_id, object_id)?;
        let text = match &mut object.kind {
            ObjectKind::Text(text) => text,
            _ => {
                return Err(EditorError::NotTextObject {
                    slide_id,
                    object_id,
                });
            }
        };
        let paragraph_length = text.paragraphs.len();
        let paragraph =
            text.paragraphs
                .get_mut(paragraph_index)
                .ok_or(EditorError::InvalidParagraphIndex {
                    index: paragraph_index,
                    length: paragraph_length,
                })?;
        paragraph.formatting = formatting;
        Ok(vec![EditorEvent::ParagraphFormattingChanged {
            slide_id,
            object_id,
            paragraph_index,
        }])
    }

    fn slide_index(&self, slide_id: SlideId) -> Result<usize, EditorError> {
        self.presentation
            .slides
            .iter()
            .position(|slide| slide.id == slide_id)
            .ok_or(EditorError::InvalidSlideId(slide_id))
    }

    fn slide_mut(&mut self, slide_id: SlideId) -> Result<&mut Slide, EditorError> {
        self.presentation
            .slide_mut(slide_id)
            .ok_or(EditorError::InvalidSlideId(slide_id))
    }

    fn object_mut(
        &mut self,
        slide_id: SlideId,
        object_id: ObjectId,
    ) -> Result<&mut SlideObject, EditorError> {
        self.slide_mut(slide_id)?
            .object_mut(object_id)
            .ok_or(EditorError::InvalidObjectId {
                slide_id,
                object_id,
            })
    }

    fn next_object_id_for_slide(&self, slide_id: SlideId) -> Result<ObjectId, EditorError> {
        next_object_id(self.slide(slide_id)?)
    }

    fn assert_invariants(&self) -> Result<(), EditorError> {
        self.presentation
            .validate()
            .map_err(EditorError::InvalidPresentation)?;
        if let Some(slide_id) = self.selection.slide_id {
            let slide = self.slide(slide_id)?;
            validate_object_ids(slide, &self.selection.object_ids, 0)?;
        } else if !self.selection.object_ids.is_empty() {
            return Err(EditorError::InvalidObjectSelection);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum LayerOperation {
    Front,
    Back,
    Forward,
    Backward,
}

fn validate_geometry(bounds: Rect, transform: Transform) -> Result<(), EditorError> {
    if !bounds.is_valid() {
        return Err(EditorError::InvalidGeometry);
    }
    if !transform.is_valid() {
        return Err(EditorError::InvalidTransform);
    }
    Ok(())
}

fn validate_object_ids(
    slide: &Slide,
    object_ids: &[ObjectId],
    minimum: usize,
) -> Result<(), EditorError> {
    if object_ids.len() < minimum
        || object_ids
            .iter()
            .any(|id| object_ids[..].iter().filter(|other| *other == id).count() > 1)
    {
        return Err(EditorError::InvalidObjectSelection);
    }
    for object_id in object_ids {
        if slide.object(*object_id).is_none() {
            return Err(EditorError::InvalidObjectId {
                slide_id: slide.id,
                object_id: *object_id,
            });
        }
    }
    Ok(())
}

fn validate_text_formatting(formatting: &TextFormatting) -> Result<(), EditorError> {
    if !formatting.font_size.is_finite() || formatting.font_size < 0.0 {
        return Err(EditorError::InvalidTextFormatting);
    }
    Ok(())
}

fn validate_paragraph_formatting(formatting: &ParagraphFormatting) -> Result<(), EditorError> {
    if !formatting.line_spacing.is_finite()
        || formatting.line_spacing <= 0.0
        || !formatting.indent.is_finite()
        || formatting.indent < 0.0
    {
        return Err(EditorError::InvalidTextFormatting);
    }
    Ok(())
}

fn next_slide_id(presentation: &Presentation) -> Result<SlideId, EditorError> {
    let used = presentation.slides.iter().map(|slide| slide.id.get());
    next_unused_id(used).map(SlideId::new)
}

fn next_object_id(slide: &Slide) -> Result<ObjectId, EditorError> {
    let used = slide.objects.iter().map(|object| object.id.get());
    next_unused_id(used).map(ObjectId::new)
}

fn next_unused_id<I>(used: I) -> Result<u64, EditorError>
where
    I: IntoIterator<Item = u64>,
{
    let mut used = used.into_iter().collect::<Vec<_>>();
    used.sort_unstable();
    let mut candidate = 1u64;
    for value in used {
        if value == candidate {
            candidate = candidate.checked_add(1).ok_or(EditorError::IdExhausted)?;
        } else if value > candidate {
            break;
        }
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presentation() -> Presentation {
        let mut presentation = Presentation::new(PresentationId::new(7));
        presentation
            .add_slide(Slide::new(SlideId::new(1)))
            .expect("valid slide");
        presentation
    }

    fn editor() -> PresentationEditor {
        PresentationEditor::new(presentation()).expect("valid presentation")
    }

    fn text_object() -> ObjectKind {
        ObjectKind::Text(Text::plain("hello"))
    }

    fn shape_object() -> ObjectKind {
        ObjectKind::Shape(Shape {
            fill: Some(Color::WHITE),
            ..Shape::default()
        })
    }

    fn create(editor: &mut PresentationEditor, kind: ObjectKind) -> ObjectId {
        editor
            .apply(EditorCommand::CreateObject {
                slide_id: SlideId::new(1),
                kind,
                bounds: Rect::new(Point::new(10.0, 20.0), 30.0, 40.0),
                transform: Transform::IDENTITY,
            })
            .expect("create")
            .into_iter()
            .find_map(|event| match event {
                EditorEvent::ObjectCreated { object_id, .. } => Some(object_id),
                _ => None,
            })
            .expect("creation event")
    }

    #[test]
    fn slide_commands_and_deterministic_ids_work() {
        let mut editor = editor();
        let events = editor
            .apply(EditorCommand::AddSlide {
                index: 1,
                layout: SlideLayout::Title,
            })
            .expect("add");
        assert_eq!(
            events,
            vec![EditorEvent::SlideAdded {
                slide_id: SlideId::new(2),
                index: 1,
            }]
        );
        let duplicate = editor
            .apply(EditorCommand::DuplicateSlide {
                slide_id: SlideId::new(1),
                index: 2,
            })
            .expect("duplicate");
        assert_eq!(
            duplicate[0],
            EditorEvent::SlideDuplicated {
                source_slide_id: SlideId::new(1),
                slide_id: SlideId::new(3),
                index: 2,
            }
        );
        editor
            .apply(EditorCommand::SetLayout {
                slide_id: SlideId::new(3),
                layout: SlideLayout::TwoColumns,
            })
            .expect("layout");
        editor
            .apply(EditorCommand::SetBackground {
                slide_id: SlideId::new(3),
                background: Background::Solid(Color::WHITE),
            })
            .expect("background");
        editor
            .apply(EditorCommand::SetTransition {
                slide_id: SlideId::new(3),
                transition: Some(Transition::new(250, SlideDirection::Right)),
            })
            .expect("transition");
        editor
            .apply(EditorCommand::ReorderSlide {
                slide_id: SlideId::new(3),
                to_index: 0,
            })
            .expect("reorder");
        assert_eq!(editor.presentation().slides[0].id, SlideId::new(3));
        editor
            .apply(EditorCommand::DeleteSlide {
                slide_id: SlideId::new(2),
            })
            .expect("delete");
        assert_eq!(editor.presentation().slides.len(), 2);
    }

    #[test]
    fn set_slide_size_and_update_notes_work() {
        let mut editor = editor();
        editor
            .apply(EditorCommand::SetSlideSize {
                slide_id: SlideId::new(1),
                size: Size::new(800.0, 600.0),
            })
            .expect("set size");
        assert_eq!(
            editor.presentation().slide(SlideId::new(1)).unwrap().size,
            Size::new(800.0, 600.0)
        );
        editor
            .apply(EditorCommand::UpdateNotes {
                slide_id: SlideId::new(1),
                notes: Some("my notes".to_string()),
            })
            .expect("update notes");
        assert_eq!(
            editor
                .presentation()
                .slide(SlideId::new(1))
                .unwrap()
                .notes
                .as_deref(),
            Some("my notes")
        );
        editor
            .apply(EditorCommand::UpdateNotes {
                slide_id: SlideId::new(1),
                notes: None,
            })
            .expect("clear notes");
        assert_eq!(
            editor.presentation().slide(SlideId::new(1)).unwrap().notes,
            None
        );
    }

    #[test]
    fn invalid_slide_size_is_rejected() {
        let mut editor = editor();
        // Zero width passes Size::is_valid (non-negative) but fails Slide validation (>0).
        assert_eq!(
            editor.apply(EditorCommand::SetSlideSize {
                slide_id: SlideId::new(1),
                size: Size::new(0.0, 720.0),
            }),
            Err(EditorError::InvalidPresentation(
                ValidationError::InvalidSlideSize {
                    slide_id: SlideId::new(1)
                }
            ))
        );
        // NaN fails Size::is_valid directly in set_slide_size.
        assert_eq!(
            editor.apply(EditorCommand::SetSlideSize {
                slide_id: SlideId::new(1),
                size: Size::new(f32::NAN, 720.0),
            }),
            Err(EditorError::InvalidSlideSize)
        );
    }

    #[test]
    fn object_lifecycle_geometry_rotation_and_layers_work() {
        let mut editor = editor();
        let first = create(&mut editor, shape_object());
        let second = create(&mut editor, text_object());
        assert_eq!(first, ObjectId::new(1));
        assert_eq!(second, ObjectId::new(2));
        editor
            .apply(EditorCommand::MoveObject {
                slide_id: SlideId::new(1),
                object_id: first,
                position: Point::new(100.0, 110.0),
            })
            .expect("move");
        editor
            .apply(EditorCommand::ResizeObject {
                slide_id: SlideId::new(1),
                object_id: first,
                bounds: Rect::new(Point::new(100.0, 110.0), 50.0, 60.0),
            })
            .expect("resize");
        editor
            .apply(EditorCommand::RotateObject {
                slide_id: SlideId::new(1),
                object_id: first,
                angle_degrees: 45.0,
            })
            .expect("rotate");
        let obj = editor.object(SlideId::new(1), first).unwrap();
        assert_eq!(obj.transform.rotation_degrees, 45.0);
        editor
            .apply(EditorCommand::BringToFront {
                slide_id: SlideId::new(1),
                object_id: first,
            })
            .expect("front");
        assert_eq!(editor.presentation().slides[0].objects[1].id, first);
        editor
            .apply(EditorCommand::SendToBack {
                slide_id: SlideId::new(1),
                object_id: first,
            })
            .expect("back");
        assert_eq!(editor.presentation().slides[0].objects[0].id, first);
        let duplicate = editor
            .apply(EditorCommand::DuplicateObject {
                slide_id: SlideId::new(1),
                object_id: first,
            })
            .expect("object duplicate");
        assert_eq!(duplicate.len(), 1);
        let dup_id = match &duplicate[0] {
            EditorEvent::ObjectDuplicated { object_id, .. } => *object_id,
            _ => panic!("expected ObjectDuplicated"),
        };
        let dup_obj = editor.object(SlideId::new(1), dup_id).unwrap();
        assert_eq!(dup_obj.transform.rotation_degrees, 45.0);
        editor
            .apply(EditorCommand::DeleteObject {
                slide_id: SlideId::new(1),
                object_id: first,
            })
            .expect("object delete");
        assert!(editor.object(SlideId::new(1), first).is_err());
    }

    #[test]
    fn selection_alignment_clipboard_and_paste_are_deterministic() {
        let mut editor = editor();
        let first = create(&mut editor, shape_object());
        let second = create(&mut editor, shape_object());
        editor
            .apply(EditorCommand::MoveObject {
                slide_id: SlideId::new(1),
                object_id: second,
                position: Point::new(100.0, 200.0),
            })
            .expect("move second");
        editor
            .apply(EditorCommand::SelectObjects {
                slide_id: SlideId::new(1),
                object_ids: vec![first, second],
            })
            .expect("select");
        editor
            .apply(EditorCommand::AlignObjects {
                slide_id: SlideId::new(1),
                object_ids: vec![first, second],
                alignment: ObjectAlignment::Left,
            })
            .expect("align");
        assert_eq!(
            editor
                .object(SlideId::new(1), second)
                .unwrap()
                .bounds
                .origin
                .x,
            10.0
        );
        editor
            .apply(EditorCommand::CopyObjects {
                slide_id: SlideId::new(1),
                object_ids: vec![first, second],
            })
            .expect("copy");
        let events = editor
            .apply(EditorCommand::PasteObjects {
                slide_id: SlideId::new(1),
                position: Point::new(200.0, 300.0),
            })
            .expect("paste");
        assert_eq!(
            events[0],
            EditorEvent::ObjectsPasted {
                slide_id: SlideId::new(1),
                object_ids: vec![ObjectId::new(3), ObjectId::new(4)],
            }
        );
        assert_eq!(
            editor.selection().object_ids,
            vec![ObjectId::new(3), ObjectId::new(4)]
        );
    }

    #[test]
    fn replace_text_run_preserves_neighboring_content_formatting_and_geometry() {
        let mut editor = editor();
        let mut first = TextRun::new("Первый");
        first.formatting.bold = true;
        let mut target = TextRun::new("старый");
        target.formatting.italic = true;
        target.formatting.font_family = Some("Aptos".to_owned());
        let original = Text {
            paragraphs: vec![
                Paragraph::plain("Другой абзац"),
                Paragraph {
                    runs: vec![first.clone(), target.clone()],
                    formatting: ParagraphFormatting {
                        alignment: TextAlignment::Center,
                        ..ParagraphFormatting::default()
                    },
                },
            ],
        };
        let object_id = create(&mut editor, ObjectKind::Text(original.clone()));
        let neighbor_id = create(&mut editor, text_object());
        let before = editor.object(SlideId::new(1), object_id).unwrap().clone();
        let neighbor_before = editor.object(SlideId::new(1), neighbor_id).unwrap().clone();
        let selection_before = editor.selection().clone();

        let events = editor
            .apply(EditorCommand::ReplaceTextRun {
                slide_id: SlideId::new(1),
                object_id,
                paragraph_index: 1,
                run_index: 1,
                text: "новый".to_owned(),
            })
            .expect("replace one existing run");
        assert_eq!(
            events,
            vec![EditorEvent::TextContentChanged {
                slide_id: SlideId::new(1),
                object_id,
                paragraph_index: 1,
                run_index: 1,
            }]
        );

        let after = editor.object(SlideId::new(1), object_id).unwrap();
        let ObjectKind::Text(text) = &after.kind else {
            panic!("expected text")
        };
        assert_eq!(text.paragraphs[0], original.paragraphs[0]);
        assert_eq!(text.paragraphs[1].runs[0], first);
        assert_eq!(text.paragraphs[1].runs[1].text, "новый");
        assert_eq!(text.paragraphs[1].runs[1].formatting, target.formatting);
        assert_eq!(
            text.paragraphs[1].formatting,
            original.paragraphs[1].formatting
        );
        assert_eq!(after.bounds, before.bounds);
        assert_eq!(after.transform, before.transform);
        assert_eq!(
            editor.object(SlideId::new(1), neighbor_id),
            Ok(&neighbor_before)
        );
        assert_eq!(editor.selection(), &selection_before);
    }

    #[test]
    fn replace_text_run_rejects_invalid_targets_atomically() {
        let mut editor = editor();
        let text_id = create(&mut editor, text_object());
        let shape_id = create(&mut editor, shape_object());
        editor
            .apply(EditorCommand::SelectObject {
                slide_id: SlideId::new(1),
                object_id: text_id,
            })
            .expect("select text object");
        let before = editor.clone();
        let command =
            |slide_id, object_id, paragraph_index, run_index| EditorCommand::ReplaceTextRun {
                slide_id,
                object_id,
                paragraph_index,
                run_index,
                text: "replacement".to_owned(),
            };
        let cases = [
            (
                command(SlideId::new(99), text_id, 0, 0),
                EditorError::InvalidSlideId(SlideId::new(99)),
            ),
            (
                command(SlideId::new(1), ObjectId::new(99), 0, 0),
                EditorError::InvalidObjectId {
                    slide_id: SlideId::new(1),
                    object_id: ObjectId::new(99),
                },
            ),
            (
                command(SlideId::new(1), shape_id, 0, 0),
                EditorError::NotTextObject {
                    slide_id: SlideId::new(1),
                    object_id: shape_id,
                },
            ),
            (
                command(SlideId::new(1), text_id, 1, 0),
                EditorError::InvalidParagraphIndex {
                    index: 1,
                    length: 1,
                },
            ),
            (
                command(SlideId::new(1), text_id, 0, 1),
                EditorError::InvalidIndex {
                    operation: "text run",
                    index: 1,
                    length: 1,
                },
            ),
        ];
        for (command, expected) in cases {
            assert_eq!(editor.apply(command), Err(expected));
            assert_eq!(editor, before);
        }
    }

    #[test]
    fn text_formatting_reaches_runs_and_paragraphs() {
        let mut editor = editor();
        let object_id = create(&mut editor, text_object());
        let formatting = TextFormatting {
            font_size: 24.0,
            bold: true,
            color: Color::rgb(1, 2, 3),
            ..TextFormatting::default()
        };
        editor
            .apply(EditorCommand::SetTextFormatting {
                slide_id: SlideId::new(1),
                object_id,
                formatting: formatting.clone(),
            })
            .expect("format text");
        let object = editor.object(SlideId::new(1), object_id).unwrap();
        let ObjectKind::Text(text) = &object.kind else {
            panic!("expected text")
        };
        assert_eq!(text.paragraphs[0].runs[0].formatting, formatting);
        editor
            .apply(EditorCommand::SetParagraphFormatting {
                slide_id: SlideId::new(1),
                object_id,
                paragraph_index: 0,
                formatting: ParagraphFormatting {
                    alignment: TextAlignment::Center,
                    ..ParagraphFormatting::default()
                },
            })
            .expect("format paragraph");
        assert_eq!(text_alignment(&editor, object_id), TextAlignment::Center);
    }

    #[test]
    fn text_color_preserves_each_runs_other_formatting() {
        let mut editor = editor();
        let mut first = TextRun::new("Первый");
        first.formatting.bold = true;
        first.formatting.font_size = 24.0;
        let mut second = TextRun::new(" второй");
        second.formatting.italic = true;
        second.formatting.font_family = Some("Aptos".to_owned());
        let object_id = create(
            &mut editor,
            ObjectKind::Text(Text {
                paragraphs: vec![Paragraph {
                    runs: vec![first.clone(), second.clone()],
                    ..Paragraph::default()
                }],
            }),
        );

        let color = Color::rgb(37, 99, 235);
        editor
            .apply(EditorCommand::SetTextColor {
                slide_id: SlideId::new(1),
                object_id,
                color,
            })
            .expect("text color must apply");

        let ObjectKind::Text(text) = &editor
            .object(SlideId::new(1), object_id)
            .expect("text object")
            .kind
        else {
            panic!("expected text")
        };
        assert_eq!(text.paragraphs[0].runs[0].formatting.color, color);
        assert_eq!(text.paragraphs[0].runs[1].formatting.color, color);
        assert!(text.paragraphs[0].runs[0].formatting.bold);
        assert_eq!(text.paragraphs[0].runs[0].formatting.font_size, 24.0);
        assert!(text.paragraphs[0].runs[1].formatting.italic);
        assert_eq!(
            text.paragraphs[0].runs[1].formatting.font_family.as_deref(),
            Some("Aptos")
        );
    }

    fn text_alignment(editor: &PresentationEditor, object_id: ObjectId) -> TextAlignment {
        let ObjectKind::Text(text) = &editor.object(SlideId::new(1), object_id).unwrap().kind
        else {
            panic!("expected text")
        };
        text.paragraphs[0].formatting.alignment
    }

    #[test]
    fn invalid_commands_are_atomic() {
        let mut editor = editor();
        let object_id = create(&mut editor, shape_object());
        let before = editor.clone();
        assert_eq!(
            editor.apply(EditorCommand::MoveObject {
                slide_id: SlideId::new(1),
                object_id,
                position: Point::new(f32::NAN, 0.0),
            }),
            Err(EditorError::InvalidGeometry)
        );
        assert_eq!(editor, before);
        assert_eq!(
            editor.apply(EditorCommand::SetTransition {
                slide_id: SlideId::new(1),
                transition: Some(Transition::default()),
            }),
            Err(EditorError::InvalidPresentation(
                ValidationError::InvalidTransition {
                    slide_id: SlideId::new(1),
                }
            ))
        );
        assert_eq!(editor, before);
        assert_eq!(
            editor.apply(EditorCommand::PasteObjects {
                slide_id: SlideId::new(1),
                position: Point::ZERO,
            }),
            Err(EditorError::EmptyClipboard)
        );
        assert_eq!(editor, before);
    }

    #[test]
    fn last_slide_and_invalid_indices_are_rejected() {
        let mut editor = editor();
        let before = editor.clone();
        assert_eq!(
            editor.apply(EditorCommand::DeleteSlide {
                slide_id: SlideId::new(1),
            }),
            Err(EditorError::CannotDeleteLastSlide)
        );
        assert_eq!(editor, before);
        assert_eq!(
            editor.apply(EditorCommand::AddSlide {
                index: 9,
                layout: SlideLayout::Blank,
            }),
            Err(EditorError::InvalidIndex {
                operation: "slide insertion",
                index: 9,
                length: 1,
            })
        );
        assert_eq!(editor, before);
    }

    #[test]
    fn deleting_only_selected_object_clears_selection_and_emits_both_events() {
        let mut editor = editor();
        let first = create(&mut editor, shape_object());
        let second = create(&mut editor, shape_object());
        editor
            .apply(EditorCommand::SelectObject {
                slide_id: SlideId::new(1),
                object_id: first,
            })
            .expect("select first");
        let events = editor
            .apply(EditorCommand::DeleteObject {
                slide_id: SlideId::new(1),
                object_id: first,
            })
            .expect("delete");
        assert_eq!(
            events,
            vec![
                EditorEvent::ObjectDeleted {
                    slide_id: SlideId::new(1),
                    object_id: first,
                },
                EditorEvent::SelectionChanged {
                    selection: Selection::slide(SlideId::new(1)),
                },
            ]
        );
        assert_eq!(editor.selection(), &Selection::slide(SlideId::new(1)));
        assert!(editor.object(SlideId::new(1), first).is_err());
        assert!(editor.object(SlideId::new(1), second).is_ok());
    }

    #[test]
    fn deleting_one_of_many_selected_objects_keeps_remaining_selection() {
        let mut editor = editor();
        let first = create(&mut editor, shape_object());
        let second = create(&mut editor, shape_object());
        let third = create(&mut editor, shape_object());
        editor
            .apply(EditorCommand::SelectObjects {
                slide_id: SlideId::new(1),
                object_ids: vec![first, second, third],
            })
            .expect("select all");
        let events = editor
            .apply(EditorCommand::DeleteObject {
                slide_id: SlideId::new(1),
                object_id: second,
            })
            .expect("delete middle");
        assert_eq!(
            events,
            vec![
                EditorEvent::ObjectDeleted {
                    slide_id: SlideId::new(1),
                    object_id: second,
                },
                EditorEvent::SelectionChanged {
                    selection: Selection {
                        slide_id: Some(SlideId::new(1)),
                        object_ids: vec![first, third],
                    },
                },
            ]
        );
        assert_eq!(editor.selection().object_ids, vec![first, third]);
        assert!(editor.object(SlideId::new(1), second).is_err());
        assert!(editor.object(SlideId::new(1), first).is_ok());
        assert!(editor.object(SlideId::new(1), third).is_ok());
    }

    #[test]
    fn deleting_unselected_object_emits_only_object_deleted() {
        let mut editor = editor();
        let first = create(&mut editor, shape_object());
        let second = create(&mut editor, shape_object());
        editor
            .apply(EditorCommand::SelectObject {
                slide_id: SlideId::new(1),
                object_id: first,
            })
            .expect("select first");
        let events = editor
            .apply(EditorCommand::DeleteObject {
                slide_id: SlideId::new(1),
                object_id: second,
            })
            .expect("delete unselected");
        assert_eq!(
            events,
            vec![EditorEvent::ObjectDeleted {
                slide_id: SlideId::new(1),
                object_id: second,
            }]
        );
        assert_eq!(editor.selection().object_ids, vec![first]);
    }

    #[test]
    fn command_sequences_are_deterministic() {
        let mut left = editor();
        let mut right = editor();
        let commands = vec![
            EditorCommand::AddSlide {
                index: 1,
                layout: SlideLayout::TitleAndContent,
            },
            EditorCommand::CreateObject {
                slide_id: SlideId::new(1),
                kind: text_object(),
                bounds: Rect::new(Point::ZERO, 10.0, 10.0),
                transform: Transform::IDENTITY,
            },
        ];
        let mut left_events = Vec::new();
        let mut right_events = Vec::new();
        for command in &commands {
            left_events.extend(left.apply_ref(command).unwrap());
            right_events.extend(right.apply_ref(command).unwrap());
        }
        assert_eq!(left, right);
        assert_eq!(left_events, right_events);
    }
}
