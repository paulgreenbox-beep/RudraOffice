//! Frozen, `std`-only public contract for Rudra Slides.
//!
//! The crate owns the document model, deterministic identifiers, validation
//! invariants, and the [`PresentationEngine`] boundary. It intentionally has no
//! serializer, renderer, UI, or adapter implementation.

#![warn(missing_docs)]

use std::collections::BTreeSet;
use std::fmt;

/// Deterministic identifier of a presentation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PresentationId(u64);

impl PresentationId {
    /// Creates an identifier from a caller-controlled deterministic value.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the identifier value.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PresentationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Deterministic identifier of a slide, unique within a presentation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SlideId(u64);

impl SlideId {
    /// Creates an identifier from a caller-controlled deterministic value.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the identifier value.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for SlideId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Deterministic identifier of a slide object, unique within a slide.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId(u64);

impl ObjectId {
    /// Creates an identifier from a caller-controlled deterministic value.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the identifier value.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Default slide size in slide units (1280×720).
pub const DEFAULT_SLIDE_SIZE: Size = Size::new(1280.0, 720.0);

/// A two-dimensional size in slide units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    /// Width. Must be finite and non-negative.
    pub width: f32,
    /// Height. Must be finite and non-negative.
    pub height: f32,
}

impl Default for Size {
    fn default() -> Self {
        DEFAULT_SLIDE_SIZE
    }
}

impl Size {
    /// Creates a size.
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    /// Returns whether both dimensions are finite and non-negative.
    pub fn is_valid(self) -> bool {
        self.width.is_finite() && self.height.is_finite() && self.width >= 0.0 && self.height >= 0.0
    }
}

/// A two-dimensional point in slide units.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f32,
    /// Vertical coordinate.
    pub y: f32,
}

impl Point {
    /// The origin point.
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    /// Creates a point.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Returns whether both coordinates are finite.
    pub fn is_valid(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// An axis-aligned rectangle in slide units.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    /// Top-left point.
    pub origin: Point,
    /// Width. Must be finite and non-negative.
    pub width: f32,
    /// Height. Must be finite and non-negative.
    pub height: f32,
}

impl Rect {
    /// Creates a rectangle from an origin and dimensions.
    pub const fn new(origin: Point, width: f32, height: f32) -> Self {
        Self {
            origin,
            width,
            height,
        }
    }

    /// Returns this rectangle's dimensions.
    pub const fn size(self) -> Size {
        Size::new(self.width, self.height)
    }

    /// Returns whether the origin and dimensions are valid.
    pub fn is_valid(self) -> bool {
        self.origin.is_valid() && self.size().is_valid()
    }
}

/// A translation, uniform scale, and rotation transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Translation in slide units.
    pub translation: Point,
    /// Uniform scale. Must be finite and non-negative.
    pub scale: f32,
    /// Rotation in degrees. Must be finite.
    pub rotation_degrees: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    /// Identity transform.
    pub const IDENTITY: Self = Self {
        translation: Point::ZERO,
        scale: 1.0,
        rotation_degrees: 0.0,
    };

    /// Creates a transform with zero rotation.
    pub const fn new(translation: Point, scale: f32) -> Self {
        Self {
            translation,
            scale,
            rotation_degrees: 0.0,
        }
    }

    /// Creates a transform with an explicit rotation angle.
    pub const fn with_rotation(translation: Point, scale: f32, rotation_degrees: f32) -> Self {
        Self {
            translation,
            scale,
            rotation_degrees,
        }
    }

    /// Returns whether the transform is finite, has a non-negative scale,
    /// and a finite rotation angle.
    pub fn is_valid(self) -> bool {
        self.translation.is_valid()
            && self.scale.is_finite()
            && self.scale >= 0.0
            && self.rotation_degrees.is_finite()
    }
}

/// An sRGB color with 8-bit channels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel.
    pub a: u8,
}

impl Color {
    /// Opaque black.
    pub const BLACK: Self = Self::rgba(0, 0, 0, 255);
    /// Transparent black.
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    /// Opaque white.
    pub const WHITE: Self = Self::rgba(255, 255, 255, 255);

    /// Creates an RGBA color.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Creates an opaque RGB color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }
}

/// Horizontal paragraph alignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum TextAlignment {
    /// Align to the left edge.
    #[default]
    Left,
    /// Center lines.
    Center,
    /// Align to the right edge.
    Right,
    /// Stretch lines to the available width.
    Justify,
}

/// Formatting applied to a text run.
#[derive(Debug, Clone, PartialEq)]
pub struct TextFormatting {
    /// Font family name, if specified.
    pub font_family: Option<String>,
    /// Font size in slide units.
    pub font_size: f32,
    /// Bold flag.
    pub bold: bool,
    /// Italic flag.
    pub italic: bool,
    /// Underline flag.
    pub underline: bool,
    /// Text color.
    pub color: Color,
}

impl Default for TextFormatting {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: 12.0,
            bold: false,
            italic: false,
            underline: false,
            color: Color::BLACK,
        }
    }
}

/// A formatted run of text.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextRun {
    /// Run content.
    pub text: String,
    /// Run formatting.
    pub formatting: TextFormatting,
}

impl TextRun {
    /// Creates a plain run with default formatting.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            formatting: TextFormatting::default(),
        }
    }
}

/// Formatting applied to a paragraph.
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphFormatting {
    /// Horizontal alignment.
    pub alignment: TextAlignment,
    /// Line spacing multiplier.
    pub line_spacing: f32,
    /// Left indentation in slide units.
    pub indent: f32,
    /// Optional bullet marker. `None` means no bullet.
    pub bullet: Option<String>,
}

impl Default for ParagraphFormatting {
    fn default() -> Self {
        Self {
            alignment: TextAlignment::Left,
            line_spacing: 1.0,
            indent: 0.0,
            bullet: None,
        }
    }
}

/// A paragraph containing ordered text runs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Paragraph {
    /// Runs in reading order.
    pub runs: Vec<TextRun>,
    /// Paragraph formatting.
    pub formatting: ParagraphFormatting,
}

impl Paragraph {
    /// Creates a paragraph containing one plain run.
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            runs: vec![TextRun::new(text)],
            formatting: ParagraphFormatting::default(),
        }
    }
}

/// Text object payload.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Text {
    /// Paragraphs in visual order.
    pub paragraphs: Vec<Paragraph>,
}

impl Text {
    /// Creates a text object containing one plain paragraph.
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            paragraphs: vec![Paragraph::plain(text)],
        }
    }
}

/// A supported geometric shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ShapeKind {
    /// Rectangle.
    #[default]
    Rectangle,
    /// Ellipse.
    Ellipse,
    /// Rounded rectangle.
    RoundedRectangle,
    /// DrawingML chevron.
    Chevron,
}

/// Shape object payload.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Shape {
    /// Shape geometry kind.
    pub kind: ShapeKind,
    /// Optional fill.
    pub fill: Option<Color>,
    /// Optional outline.
    pub stroke: Option<Color>,
    /// Outline width in slide units.
    pub stroke_width: f32,
}

/// Line object payload.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Line {
    /// Start point.
    pub start: Point,
    /// End point.
    pub end: Point,
    /// Optional stroke color.
    pub stroke: Option<Color>,
    /// Stroke width in slide units.
    pub stroke_width: f32,
}

/// Declared image format.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    /// PNG.
    #[default]
    Png,
    /// JPEG.
    Jpeg,
    /// SVG.
    Svg,
    /// BMP.
    Bmp,
    /// Format not known by the API.
    Unknown,
}

/// Image object payload. The API owns bytes but does not decode them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Image {
    /// Encoded image bytes.
    pub bytes: Vec<u8>,
    /// Declared format.
    pub format: ImageFormat,
    /// Optional alternative text.
    pub alt_text: Option<String>,
}

/// Layout metadata for one table cell in row-major order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableCellLayout {
    /// Number of grid rows owned by this cell.
    pub row_span: usize,
    /// Number of grid columns owned by this cell.
    pub column_span: usize,
    /// Whether this position is covered by another cell's merged rectangle.
    pub is_merge_continuation: bool,
}

impl Default for TableCellLayout {
    fn default() -> Self {
        Self {
            row_span: 1,
            column_span: 1,
            is_merge_continuation: false,
        }
    }
}

/// Table object payload.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Table {
    /// Number of rows.
    pub rows: usize,
    /// Number of columns.
    pub columns: usize,
    /// Positive relative column widths, scaled into the object's bounds.
    pub column_widths: Vec<f32>,
    /// Positive relative row heights, scaled into the object's bounds.
    pub row_heights: Vec<f32>,
    /// Optional text per cell, in row-major order.
    pub cells: Vec<Option<Text>>,
    /// Geometry per cell, in row-major order.
    pub cell_layout: Vec<TableCellLayout>,
}

impl Table {
    /// Creates an empty table. Returns `None` for zero dimensions or overflow.
    pub fn new(rows: usize, columns: usize) -> Option<Self> {
        let length = rows.checked_mul(columns)?;
        if rows == 0 || columns == 0 {
            return None;
        }
        Some(Self {
            rows,
            columns,
            column_widths: vec![1.0; columns],
            row_heights: vec![1.0; rows],
            cells: vec![None; length],
            cell_layout: vec![TableCellLayout::default(); length],
        })
    }

    /// Returns whether dimensions, storage and merged-cell geometry agree.
    pub fn is_valid(&self) -> bool {
        let Some(length) = self.rows.checked_mul(self.columns) else {
            return false;
        };
        if self.rows == 0
            || self.columns == 0
            || self.cells.len() != length
            || self.cell_layout.len() != length
            || self.column_widths.len() != self.columns
            || self.row_heights.len() != self.rows
        {
            return false;
        }
        if self
            .column_widths
            .iter()
            .chain(self.row_heights.iter())
            .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return false;
        }
        let column_total: f32 = self.column_widths.iter().sum();
        let row_total: f32 = self.row_heights.iter().sum();
        if !column_total.is_finite() || !row_total.is_finite() {
            return false;
        }

        let mut covered = vec![false; length];
        for (index, layout) in self.cell_layout.iter().enumerate() {
            if layout.is_merge_continuation {
                continue;
            }
            if layout.row_span == 0 || layout.column_span == 0 {
                return false;
            }
            let row = index / self.columns;
            let column = index % self.columns;
            let Some(end_row) = row.checked_add(layout.row_span) else {
                return false;
            };
            let Some(end_column) = column.checked_add(layout.column_span) else {
                return false;
            };
            if end_row > self.rows || end_column > self.columns {
                return false;
            }
            for covered_row in row..end_row {
                let Some(row_offset) = covered_row.checked_mul(self.columns) else {
                    return false;
                };
                for covered_column in column..end_column {
                    let Some(covered_index) = row_offset.checked_add(covered_column) else {
                        return false;
                    };
                    if covered[covered_index] {
                        return false;
                    }
                    covered[covered_index] = true;
                }
            }
        }

        self.cell_layout
            .iter()
            .enumerate()
            .all(|(index, layout)| !layout.is_merge_continuation || covered[index])
    }
}

/// Payload kind of a slide object.
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectKind {
    /// Text payload.
    Text(Text),
    /// Geometric shape payload.
    Shape(Shape),
    /// Line payload.
    Line(Line),
    /// Image payload.
    Image(Image),
    /// Table payload.
    Table(Table),
}

impl ObjectKind {
    /// Returns a stable diagnostic name.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Shape(_) => "shape",
            Self::Line(_) => "line",
            Self::Image(_) => "image",
            Self::Table(_) => "table",
        }
    }
}

/// A positioned object on a slide.
#[derive(Debug, Clone, PartialEq)]
pub struct SlideObject {
    /// Object identifier unique within its slide.
    pub id: ObjectId,
    /// Object payload.
    pub kind: ObjectKind,
    /// Axis-aligned object bounds.
    pub bounds: Rect,
    /// Local transform.
    pub transform: Transform,
}

impl SlideObject {
    /// Creates a positioned object.
    pub const fn new(id: ObjectId, kind: ObjectKind, bounds: Rect, transform: Transform) -> Self {
        Self {
            id,
            kind,
            bounds,
            transform,
        }
    }

    /// Returns whether object geometry and payload are valid.
    pub fn is_valid(&self) -> bool {
        self.bounds.is_valid() && self.transform.is_valid() && payload_is_valid(&self.kind)
    }
}

fn payload_is_valid(kind: &ObjectKind) -> bool {
    match kind {
        ObjectKind::Line(line) => {
            line.start.is_valid()
                && line.end.is_valid()
                && line.stroke_width.is_finite()
                && line.stroke_width >= 0.0
        }
        ObjectKind::Shape(shape) => shape.stroke_width.is_finite() && shape.stroke_width >= 0.0,
        ObjectKind::Table(table) => table.is_valid(),
        ObjectKind::Text(_) | ObjectKind::Image(_) => true,
    }
}

/// Layout preset for a slide.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SlideLayout {
    /// No placeholders.
    #[default]
    Blank,
    /// Title placeholder.
    Title,
    /// Title and content placeholders.
    TitleAndContent,
    /// Two content columns.
    TwoColumns,
    /// Application-defined layout.
    Custom,
}

/// Slide background.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Background {
    /// No explicit background.
    #[default]
    None,
    /// Solid color background.
    Solid(Color),
}

/// Transition direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SlideDirection {
    /// Enter from the left.
    #[default]
    Left,
    /// Enter from the right.
    Right,
    /// Enter from above.
    Up,
    /// Enter from below.
    Down,
}

/// Slide transition settings.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Transition {
    /// Duration in milliseconds. Zero means disabled/invalid.
    pub duration_ms: u32,
    /// Direction of the transition.
    pub direction: SlideDirection,
}

impl Transition {
    /// Creates transition settings.
    pub const fn new(duration_ms: u32, direction: SlideDirection) -> Self {
        Self {
            duration_ms,
            direction,
        }
    }

    /// Returns whether this transition has a positive duration.
    pub const fn is_valid(self) -> bool {
        self.duration_ms > 0
    }
}

/// A slide in a presentation.
#[derive(Debug, Clone, PartialEq)]
pub struct Slide {
    /// Slide identifier unique within its presentation.
    pub id: SlideId,
    /// Slide size in slide units.
    pub size: Size,
    /// Optional speaker notes.
    pub notes: Option<String>,
    /// Layout preset.
    pub layout: SlideLayout,
    /// Background.
    pub background: Background,
    /// Optional transition.
    pub transition: Option<Transition>,
    /// Objects in visual order.
    pub objects: Vec<SlideObject>,
}

impl Default for Slide {
    fn default() -> Self {
        Self {
            id: SlideId::default(),
            size: DEFAULT_SLIDE_SIZE,
            notes: None,
            layout: SlideLayout::default(),
            background: Background::default(),
            transition: None,
            objects: Vec::new(),
        }
    }
}

impl Slide {
    /// Creates an empty blank slide.
    pub fn new(id: SlideId) -> Self {
        Self {
            id,
            ..Self::default()
        }
    }

    /// Adds an object and checks its local invariants.
    pub fn add_object(&mut self, object: SlideObject) -> Result<(), ValidationError> {
        if object.id.get() == 0 {
            return Err(ValidationError::InvalidObjectId {
                slide_id: self.id,
                object_id: object.id,
            });
        }
        if self.objects.iter().any(|existing| existing.id == object.id) {
            return Err(ValidationError::DuplicateObjectId {
                slide_id: self.id,
                object_id: object.id,
            });
        }
        validate_object(&object)?;
        self.objects.push(object);
        Ok(())
    }

    /// Returns the object with the requested ID.
    pub fn object(&self, id: ObjectId) -> Option<&SlideObject> {
        self.objects.iter().find(|object| object.id == id)
    }

    /// Returns a mutable object with the requested ID.
    pub fn object_mut(&mut self, id: ObjectId) -> Option<&mut SlideObject> {
        self.objects.iter_mut().find(|object| object.id == id)
    }

    /// Validates object IDs, geometry, payloads, transition settings, and slide size.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.size.width <= 0.0
            || self.size.height <= 0.0
            || !self.size.width.is_finite()
            || !self.size.height.is_finite()
        {
            return Err(ValidationError::InvalidSlideSize { slide_id: self.id });
        }
        if self.id.get() == 0 {
            return Err(ValidationError::InvalidSlideId { slide_id: self.id });
        }
        let mut object_ids = BTreeSet::new();
        for object in &self.objects {
            if object.id.get() == 0 {
                return Err(ValidationError::InvalidObjectId {
                    slide_id: self.id,
                    object_id: object.id,
                });
            }
            if !object_ids.insert(object.id) {
                return Err(ValidationError::DuplicateObjectId {
                    slide_id: self.id,
                    object_id: object.id,
                });
            }
            validate_object(object)?;
        }
        match self.transition {
            Some(transition) if !transition.is_valid() => {
                return Err(ValidationError::InvalidTransition { slide_id: self.id });
            }
            _ => {}
        }
        Ok(())
    }
}

fn validate_object(object: &SlideObject) -> Result<(), ValidationError> {
    if !object.bounds.is_valid() {
        return Err(ValidationError::InvalidGeometry {
            object_id: object.id,
            message: "bounds must be finite and non-negative".into(),
        });
    }
    if !object.transform.is_valid() {
        return Err(ValidationError::InvalidTransform {
            object_id: object.id,
            message: "transform must be finite and non-negative in scale".into(),
        });
    }
    if !payload_is_valid(&object.kind) {
        return Err(ValidationError::InvalidPayload {
            object_id: object.id,
            message: format!("{} payload is invalid", object.kind.name()),
        });
    }
    Ok(())
}

/// A presentation document.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Presentation {
    /// Presentation identifier.
    pub id: PresentationId,
    /// Slides in presentation order.
    pub slides: Vec<Slide>,
}

impl Presentation {
    /// Creates an empty presentation. Call [`Self::validate`] before saving.
    pub fn new(id: PresentationId) -> Self {
        Self {
            id,
            slides: Vec::new(),
        }
    }

    /// Adds a slide while preserving identifier and slide invariants.
    pub fn add_slide(&mut self, slide: Slide) -> Result<(), ValidationError> {
        if self.slides.iter().any(|existing| existing.id == slide.id) {
            return Err(ValidationError::DuplicateSlideId { slide_id: slide.id });
        }
        slide.validate()?;
        self.slides.push(slide);
        Ok(())
    }

    /// Returns a slide by ID.
    pub fn slide(&self, id: SlideId) -> Option<&Slide> {
        self.slides.iter().find(|slide| slide.id == id)
    }

    /// Returns a mutable slide by ID.
    pub fn slide_mut(&mut self, id: SlideId) -> Option<&mut Slide> {
        self.slides.iter_mut().find(|slide| slide.id == id)
    }

    /// Validates the non-empty presentation and all nested invariants.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.id.get() == 0 {
            return Err(ValidationError::InvalidPresentationId { id: self.id });
        }
        if self.slides.is_empty() {
            return Err(ValidationError::EmptyPresentation);
        }
        let mut slide_ids = BTreeSet::new();
        for slide in &self.slides {
            if !slide_ids.insert(slide.id) {
                return Err(ValidationError::DuplicateSlideId { slide_id: slide.id });
            }
            slide.validate()?;
        }
        Ok(())
    }
}

/// Compatibility name for callers using the early document spelling.
pub type PresentationDocument = Presentation;

/// Returns the next deterministic slide ID after the largest existing ID.
pub fn next_slide_id(presentation: &Presentation) -> SlideId {
    SlideId::new(
        presentation
            .slides
            .iter()
            .map(|slide| slide.id.get())
            .max()
            .unwrap_or(0)
            .saturating_add(1),
    )
}

/// Returns the next deterministic object ID after the largest existing ID.
pub fn next_object_id(slide: &Slide) -> ObjectId {
    ObjectId::new(
        slide
            .objects
            .iter()
            .map(|object| object.id.get())
            .max()
            .unwrap_or(0)
            .saturating_add(1),
    )
}

/// Validation failure for a document invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// Presentation contains no slides.
    EmptyPresentation,
    /// Presentation identifier is zero.
    InvalidPresentationId {
        /// Invalid presentation identifier.
        id: PresentationId,
    },
    /// Slide identifier is zero.
    InvalidSlideId {
        /// Invalid slide identifier.
        slide_id: SlideId,
    },
    /// Object identifier is zero.
    InvalidObjectId {
        /// Owning slide ID.
        slide_id: SlideId,
        /// Invalid object identifier.
        object_id: ObjectId,
    },
    /// A slide ID is repeated.
    DuplicateSlideId {
        /// Repeated ID.
        slide_id: SlideId,
    },
    /// An object ID is repeated inside a slide.
    DuplicateObjectId {
        /// Owning slide ID.
        slide_id: SlideId,
        /// Repeated object ID.
        object_id: ObjectId,
    },
    /// A slide's size is invalid.
    InvalidSlideSize {
        /// Slide ID.
        slide_id: SlideId,
    },
    /// Object bounds are invalid.
    InvalidGeometry {
        /// Object ID.
        object_id: ObjectId,
        /// Diagnostic message.
        message: String,
    },
    /// Object transform is invalid.
    InvalidTransform {
        /// Object ID.
        object_id: ObjectId,
        /// Diagnostic message.
        message: String,
    },
    /// Object payload is invalid.
    InvalidPayload {
        /// Object ID.
        object_id: ObjectId,
        /// Diagnostic message.
        message: String,
    },
    /// Transition settings are invalid.
    InvalidTransition {
        /// Owning slide ID.
        slide_id: SlideId,
    },
    /// A reference in an extension payload could not be resolved.
    BrokenReference {
        /// Source area of the reference.
        source: String,
        /// Diagnostic message.
        message: String,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPresentation => {
                formatter.write_str("presentation must contain at least one slide")
            }
            Self::InvalidPresentationId { id } => {
                write!(formatter, "invalid presentation id {id}")
            }
            Self::InvalidSlideId { slide_id } => {
                write!(formatter, "invalid slide id {slide_id}")
            }
            Self::InvalidObjectId {
                slide_id,
                object_id,
            } => write!(
                formatter,
                "invalid object id {object_id} in slide {slide_id}"
            ),
            Self::DuplicateSlideId { slide_id } => {
                write!(formatter, "duplicate slide id {slide_id}")
            }
            Self::DuplicateObjectId {
                slide_id,
                object_id,
            } => write!(
                formatter,
                "duplicate object id {object_id} in slide {slide_id}"
            ),
            Self::InvalidSlideSize { slide_id } => {
                write!(formatter, "invalid size for slide {slide_id}")
            }
            Self::InvalidGeometry { object_id, message } => {
                write!(
                    formatter,
                    "invalid geometry for object {object_id}: {message}"
                )
            }
            Self::InvalidTransform { object_id, message } => {
                write!(
                    formatter,
                    "invalid transform for object {object_id}: {message}"
                )
            }
            Self::InvalidPayload { object_id, message } => {
                write!(
                    formatter,
                    "invalid payload for object {object_id}: {message}"
                )
            }
            Self::InvalidTransition { slide_id } => {
                write!(formatter, "invalid transition in slide {slide_id}")
            }
            Self::BrokenReference { source, message } => {
                write!(formatter, "broken reference in {source}: {message}")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Engine feature advertised by a [`PresentationEngine`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Loading and saving bytes.
    LoadSave,
    /// Multiple slides.
    Slides,
    /// Shapes.
    Shapes,
    /// Text and paragraph formatting.
    Text,
    /// Images.
    Images,
    /// Tables.
    Tables,
    /// Transitions or animations.
    Animations,
    /// Speaker notes.
    Notes,
}

impl Capability {
    /// Stable lower-case capability name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LoadSave => "load_save",
            Self::Slides => "slides",
            Self::Shapes => "shapes",
            Self::Text => "text",
            Self::Images => "images",
            Self::Tables => "tables",
            Self::Animations => "animations",
            Self::Notes => "notes",
        }
    }
}

/// Engine error returned by load/save operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// Input bytes or model values are invalid.
    InvalidInput(String),
    /// Input bytes are not recognized by the engine.
    InvalidFormat,
    /// Requested capability is not implemented.
    UnsupportedCapability(String),
    /// A bounded engine limit was exceeded.
    LimitExceeded {
        /// Maximum accepted value.
        limit: u64,
        /// Actual supplied value.
        actual: u64,
        /// Limit context.
        context: String,
    },
    /// Input was not valid UTF-8 where text was required.
    Utf8Error,
    /// A document reference failed integrity checks.
    ReferenceIntegrity(String),
    /// Codec-specific failure.
    CodecError(String),
    /// Model validation failed before serialization.
    ValidationError(ValidationError),
}

impl fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => write!(formatter, "invalid input: {message}"),
            Self::InvalidFormat => formatter.write_str("invalid format"),
            Self::UnsupportedCapability(capability) => {
                write!(formatter, "unsupported capability: {capability}")
            }
            Self::LimitExceeded {
                limit,
                actual,
                context,
            } => write!(formatter, "limit exceeded in {context}: {actual} > {limit}"),
            Self::Utf8Error => formatter.write_str("invalid UTF-8 sequence"),
            Self::ReferenceIntegrity(message) => {
                write!(formatter, "reference integrity violation: {message}")
            }
            Self::CodecError(message) => write!(formatter, "codec error: {message}"),
            Self::ValidationError(error) => write!(formatter, "validation error: {error}"),
        }
    }
}

impl std::error::Error for EngineError {}

/// Format engine boundary owned by the API crate.
///
/// The trait operates only on in-memory bytes and the public model.
pub trait PresentationEngine {
    /// Loads a presentation from encoded bytes.
    fn load(&self, bytes: &[u8]) -> Result<Presentation, EngineError>;

    /// Saves a presentation as encoded bytes.
    fn save(&self, presentation: &Presentation) -> Result<Vec<u8>, EngineError>;

    /// Returns the capabilities supported by this engine.
    fn capabilities(&self) -> &[Capability] {
        &[]
    }

    /// Legacy spelling retained as a forwarding default.
    fn supported_capabilities(&self) -> &[Capability] {
        self.capabilities()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(id: u64) -> SlideObject {
        SlideObject::new(
            ObjectId::new(id),
            ObjectKind::Text(Text::plain("text")),
            Rect::new(Point::ZERO, 10.0, 10.0),
            Transform::IDENTITY,
        )
    }

    #[test]
    fn ids_are_deterministic_and_orderable() {
        assert_eq!(PresentationId::new(7).get(), 7);
        assert_eq!(SlideId::new(3), SlideId::new(3));
        assert!(ObjectId::new(1) < ObjectId::new(2));
        assert_eq!(PresentationId::new(7).to_string(), "7");
    }

    #[test]
    fn constructors_and_geometry_validation_work() {
        assert!(Size::new(10.0, 20.0).is_valid());
        assert!(!Size::new(-1.0, 20.0).is_valid());
        assert!(Size::new(0.0, 0.0).is_valid());
        assert!(Rect::new(Point::ZERO, 2.0, 3.0).is_valid());
        assert!(!Rect::new(Point::new(f32::NAN, 0.0), 1.0, 1.0).is_valid());
        assert!(Transform::IDENTITY.is_valid());
        assert!(!Transform::new(Point::ZERO, f32::INFINITY).is_valid());
        assert!(Transform::with_rotation(Point::ZERO, 1.0, 45.0).is_valid());
        assert!(!Transform::with_rotation(Point::ZERO, 1.0, f32::NAN).is_valid());
    }

    #[test]
    fn slide_size_and_notes_work() {
        let mut slide = Slide::new(SlideId::new(1));
        assert_eq!(slide.size, DEFAULT_SLIDE_SIZE);
        assert_eq!(slide.notes, None);
        slide.notes = Some("hello".to_string());
        assert_eq!(slide.notes.as_deref(), Some("hello"));
        slide.size = Size::new(800.0, 600.0);
        assert!(slide.validate().is_ok());
        slide.size = Size::new(-1.0, 600.0);
        assert!(matches!(
            slide.validate(),
            Err(ValidationError::InvalidSlideSize { .. })
        ));
    }

    #[test]
    fn invalid_slide_size_is_rejected() {
        let mut slide = Slide::new(SlideId::new(1));
        slide.size = Size::new(0.0, 720.0);
        assert!(matches!(
            slide.validate(),
            Err(ValidationError::InvalidSlideSize { .. })
        ));
        slide.size = Size::new(f32::NAN, 720.0);
        assert!(matches!(
            slide.validate(),
            Err(ValidationError::InvalidSlideSize { .. })
        ));
    }

    #[test]
    fn slide_and_presentation_validation_check_ids_and_empty_documents() {
        let mut presentation = Presentation::new(PresentationId::new(1));
        assert_eq!(
            presentation.validate(),
            Err(ValidationError::EmptyPresentation)
        );

        let mut slide = Slide::new(SlideId::new(1));
        slide.add_object(object(4)).unwrap();
        assert_eq!(
            slide.add_object(object(4)),
            Err(ValidationError::DuplicateObjectId {
                slide_id: SlideId::new(1),
                object_id: ObjectId::new(4),
            })
        );
        presentation.add_slide(slide).unwrap();
        assert!(presentation.validate().is_ok());
        assert_eq!(next_slide_id(&presentation), SlideId::new(2));
        assert_eq!(
            next_object_id(presentation.slide(SlideId::new(1)).unwrap()),
            ObjectId::new(5)
        );
    }

    #[test]
    fn invalid_nested_geometry_is_rejected() {
        let invalid = SlideObject::new(
            ObjectId::new(1),
            ObjectKind::Shape(Shape {
                stroke_width: -1.0,
                ..Shape::default()
            }),
            Rect::new(Point::ZERO, 10.0, 10.0),
            Transform::IDENTITY,
        );
        let mut slide = Slide::new(SlideId::new(1));
        assert!(matches!(
            slide.add_object(invalid),
            Err(ValidationError::InvalidPayload { .. })
        ));
    }

    #[test]
    fn formatting_and_table_constructors_are_stable() {
        let paragraph = Paragraph::plain("hello");
        assert_eq!(paragraph.runs[0].text, "hello");
        assert_eq!(paragraph.formatting.alignment, TextAlignment::Left);
        let table = Table::new(2, 3).unwrap();
        assert!(table.is_valid());
        assert_eq!(table.cells.len(), 6);
        assert_eq!(table.column_widths, vec![1.0; 3]);
        assert_eq!(table.row_heights, vec![1.0; 2]);
        assert!(Table::new(0, 3).is_none());
    }

    #[test]
    fn table_grid_dimensions_must_match_shape_and_be_positive() {
        let mut table = Table::new(2, 2).unwrap();
        table.column_widths = vec![1.0];
        assert!(!table.is_valid());

        table.column_widths = vec![1.0, 0.0];
        assert!(!table.is_valid());

        table.column_widths = vec![1.0, 1.0];
        table.row_heights = vec![1.0, f32::INFINITY];
        assert!(!table.is_valid());

        table.row_heights = vec![1.0, 2.0];
        assert!(table.is_valid());
    }

    #[test]
    fn zero_slide_size_is_invalid_while_size_is_valid() {
        // Size::is_valid treats zero as valid (finite, non-negative).
        let zero = Size::new(0.0, 0.0);
        assert!(zero.is_valid());
        // Slide::validate requires strictly positive dimensions, so zero slide size fails.
        let mut slide = Slide::new(SlideId::new(1));
        slide.size = zero;
        assert!(matches!(
            slide.validate(),
            Err(ValidationError::InvalidSlideSize { .. })
        ));
    }

    #[test]
    fn zero_ids_are_rejected_during_validation() {
        // Presentation id == 0 -> InvalidPresentationId
        let mut presentation = Presentation::new(PresentationId::new(0));
        presentation
            .add_slide(Slide::new(SlideId::new(1)))
            .expect("valid slide");
        assert_eq!(
            presentation.validate(),
            Err(ValidationError::InvalidPresentationId {
                id: PresentationId::new(0)
            })
        );
        assert!(
            presentation
                .validate()
                .unwrap_err()
                .to_string()
                .contains("invalid presentation id 0")
        );

        // Slide id == 0 -> InvalidSlideId (caught at add_slide via Slide::validate)
        let mut presentation = Presentation::new(PresentationId::new(1));
        assert_eq!(
            presentation.add_slide(Slide::new(SlideId::new(0))),
            Err(ValidationError::InvalidSlideId {
                slide_id: SlideId::new(0)
            })
        );
        assert!(
            presentation
                .add_slide(Slide::new(SlideId::new(0)))
                .unwrap_err()
                .to_string()
                .contains("invalid slide id 0")
        );

        // Object id == 0 -> InvalidObjectId
        let zero_object = SlideObject::new(
            ObjectId::new(0),
            ObjectKind::Text(Text::plain("text")),
            Rect::new(Point::ZERO, 10.0, 10.0),
            Transform::IDENTITY,
        );
        let mut slide = Slide::new(SlideId::new(1));
        assert_eq!(
            slide.add_object(zero_object.clone()),
            Err(ValidationError::InvalidObjectId {
                slide_id: SlideId::new(1),
                object_id: ObjectId::new(0),
            })
        );
        let mut presentation = Presentation::new(PresentationId::new(1));
        let mut slide = Slide::new(SlideId::new(1));
        slide.objects.push(zero_object);
        presentation
            .add_slide(slide)
            .expect_err("slide add validates object ids");
        assert_eq!(
            presentation.validate(),
            Err(ValidationError::EmptyPresentation)
        );
        // Also confirm validate() rejects an inline zero object.
        let mut presentation = Presentation::new(PresentationId::new(1));
        let mut slide = Slide::new(SlideId::new(1));
        slide.objects.push(SlideObject::new(
            ObjectId::new(0),
            ObjectKind::Text(Text::plain("x")),
            Rect::new(Point::ZERO, 5.0, 5.0),
            Transform::IDENTITY,
        ));
        presentation
            .add_slide(slide)
            .expect_err("slide add validates object ids");
    }

    #[test]
    fn engine_errors_and_trait_compile_facing_contract() {
        let error = EngineError::ValidationError(ValidationError::EmptyPresentation);
        assert!(error.to_string().contains("validation error"));
        assert_eq!(Capability::Text.as_str(), "text");

        struct TestEngine;
        static CAPS: [Capability; 2] = [Capability::LoadSave, Capability::Slides];
        impl PresentationEngine for TestEngine {
            fn load(&self, _bytes: &[u8]) -> Result<Presentation, EngineError> {
                Err(EngineError::InvalidFormat)
            }
            fn save(&self, _presentation: &Presentation) -> Result<Vec<u8>, EngineError> {
                Ok(vec![1, 2, 3])
            }
            fn capabilities(&self) -> &[Capability] {
                &CAPS
            }
        }

        let engine = TestEngine;
        assert_eq!(engine.capabilities(), &CAPS);
        assert!(matches!(engine.load(&[]), Err(EngineError::InvalidFormat)));
        assert_eq!(
            engine
                .save(&Presentation::new(PresentationId::new(1)))
                .unwrap(),
            vec![1, 2, 3]
        );
    }
}
