//! Core identifiers, geometry, and engine interface for PDF documents.
//!
//! This crate intentionally has no PDF parser or renderer implementation and
//! no dependencies on application or UI frameworks.

use std::error::Error;
use std::fmt;

/// Stable identifier for a document managed by a [`PdfEngine`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DocumentId(u64);

impl DocumentId {
    /// Creates an identifier from its numeric value.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric value of this identifier.
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Stable identifier for a page within a document.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PageId(u64);

impl PageId {
    /// Creates an identifier from its numeric value.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric value of this identifier.
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Basic information returned after a document is parsed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentInfo {
    pub document_id: DocumentId,
    pub page_count: u32,
    pub page_ids: Vec<PageId>,
}

impl DocumentInfo {
    /// Creates document metadata, deriving `page_count` from `page_ids`.
    pub fn new(document_id: DocumentId, page_ids: Vec<PageId>) -> Self {
        let page_count = u32::try_from(page_ids.len()).unwrap_or(u32::MAX);
        Self {
            document_id,
            page_count,
            page_ids,
        }
    }
}

/// A PDF page's effective rectangular geometry, measured in points.
///
/// The crop box is retained as supplied. When it is absent, the media box is
/// the effective page box. Width and height account for page rotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageGeometry {
    pub media_box: [f32; 4],
    pub crop_box: Option<[f32; 4]>,
    pub width_points: f32,
    pub height_points: f32,
    pub rotation_degrees: i32,
}

impl PageGeometry {
    /// Creates validated page geometry from PDF box coordinates.
    ///
    /// Boxes use `[lower_left_x, lower_left_y, upper_right_x,
    /// upper_right_y]`. A crop box must be finite, have positive area, and be
    /// contained by the media box. Rotation is restricted to quarter turns.
    pub fn new(
        media_box: [f32; 4],
        crop_box: Option<[f32; 4]>,
        rotation: i32,
    ) -> Result<Self, PageGeometryError> {
        validate_box(media_box).map_err(|_| PageGeometryError::InvalidMediaBox)?;

        if let Some(crop) = crop_box {
            validate_box(crop).map_err(|_| PageGeometryError::InvalidCropBox)?;
            if crop[0] < media_box[0]
                || crop[1] < media_box[1]
                || crop[2] > media_box[2]
                || crop[3] > media_box[3]
            {
                return Err(PageGeometryError::CropBoxOutsideMediaBox);
            }
        }

        if !matches!(rotation, 0 | 90 | 180 | 270) {
            return Err(PageGeometryError::InvalidRotation(rotation));
        }

        let effective_box = crop_box.unwrap_or(media_box);
        let box_width = effective_box[2] - effective_box[0];
        let box_height = effective_box[3] - effective_box[1];
        let (width_points, height_points) = match rotation {
            90 | 270 => (box_height, box_width),
            _ => (box_width, box_height),
        };

        Ok(Self {
            media_box,
            crop_box,
            width_points,
            height_points,
            rotation_degrees: rotation,
        })
    }

    /// Returns US Letter page geometry (612 × 792 points, portrait).
    pub fn letter() -> Self {
        // These constant, ordered coordinates satisfy all constructor checks.
        Self {
            media_box: [0.0, 0.0, 612.0, 792.0],
            crop_box: None,
            width_points: 612.0,
            height_points: 792.0,
            rotation_degrees: 0,
        }
    }
}

fn validate_box(rect: [f32; 4]) -> Result<(), ()> {
    if rect.iter().any(|coordinate| !coordinate.is_finite()) {
        return Err(());
    }
    let width = rect[2] - rect[0];
    let height = rect[3] - rect[1];
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(());
    }
    Ok(())
}

/// Validation errors produced when constructing [`PageGeometry`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageGeometryError {
    InvalidMediaBox,
    InvalidCropBox,
    CropBoxOutsideMediaBox,
    InvalidRotation(i32),
}

impl fmt::Display for PageGeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMediaBox => write!(f, "media box must be finite and have positive area"),
            Self::InvalidCropBox => write!(f, "crop box must be finite and have positive area"),
            Self::CropBoxOutsideMediaBox => {
                write!(f, "crop box must be contained within the media box")
            }
            Self::InvalidRotation(rotation) => write!(
                f,
                "rotation must be 0, 90, 180, or 270 degrees (got {rotation})"
            ),
        }
    }
}

impl Error for PageGeometryError {}

/// Error returned by a PDF engine operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PdfError {
    message: String,
}

impl PdfError {
    /// Creates an engine error with a human-readable message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for PdfError {}

impl From<PageGeometryError> for PdfError {
    fn from(error: PageGeometryError) -> Self {
        Self::new(error.to_string())
    }
}

/// Convenient result type for PDF engine operations.
pub type PdfResult<T> = Result<T, PdfError>;

/// Backend interface for parsing, inspecting, and rendering PDF documents.
pub trait PdfEngine {
    /// Parses PDF bytes and returns metadata for the resulting document.
    fn parse(&mut self, bytes: &[u8]) -> PdfResult<DocumentInfo>;

    /// Returns geometry for a page previously identified by `parse`.
    fn page_geometry(
        &mut self,
        document_id: DocumentId,
        page_id: PageId,
    ) -> PdfResult<PageGeometry>;

    /// Renders a page at the requested scale and returns encoded image bytes.
    fn render_page(
        &mut self,
        document_id: DocumentId,
        page_id: PageId,
        scale: f32,
    ) -> PdfResult<Vec<u8>>;
}
