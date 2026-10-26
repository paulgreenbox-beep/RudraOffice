//! Renderer-neutral view models derived from the frozen [`slides_api`] document.
//!
//! `slides-ui` never mutates a document. These types are a frozen, plain,
//! `std`-only projection of [`Slide`] into shapes that the thumbnail strip, the
//! slideshow projection, and render-plan validation can consume without owning
//! serialization or GPU concerns.
//!
//! The only canvas coupling is [`Viewport`] and [`RenderCommand`] inside
//! [`RenderPlan`]; both are frozen `slides-canvas` types.

use slides_api::{
    Background, ImageFormat, ObjectId, ObjectKind, Rect, Size, Slide, SlideId, SlideLayout,
    SlideObject, Text, Transform, Transition,
};
use slides_canvas::{RenderCommand, Viewport};

/// Renderer-neutral view of a positioned slide object.
#[derive(Debug, Clone, PartialEq)]
pub struct SlideObjectView {
    /// Object identifier, unique within its slide.
    pub id: ObjectId,
    /// Axis-aligned object bounds in slide units.
    pub bounds: Rect,
    /// Local transform.
    pub transform: Transform,
    /// Object payload kind.
    pub kind: ObjectKind,
}

impl SlideObjectView {
    /// Creates a view projection of a document object.
    pub fn from_object(object: &SlideObject) -> Self {
        Self {
            id: object.id,
            bounds: object.bounds,
            transform: object.transform,
            kind: object.kind.clone(),
        }
    }
}

/// Renderer-neutral view of a slide.
#[derive(Debug, Clone, PartialEq)]
pub struct SlideView {
    /// Slide identifier, unique within its presentation.
    pub id: SlideId,
    /// Slide size in slide units.
    pub size: Size,
    /// Optional speaker notes.
    pub notes: Option<String>,
    /// Layout preset.
    pub layout: SlideLayout,
    /// Background.
    pub background: Background,
    /// Optional transition settings.
    pub transition: Option<Transition>,
    /// Objects in visual order.
    pub objects: Vec<SlideObjectView>,
}

impl SlideView {
    /// Creates a view projection of a document slide.
    pub fn from_slide(slide: &Slide) -> Self {
        Self {
            id: slide.id,
            size: slide.size,
            notes: slide.notes.clone(),
            layout: slide.layout,
            background: slide.background,
            transition: slide.transition,
            objects: slide
                .objects
                .iter()
                .map(SlideObjectView::from_object)
                .collect(),
        }
    }

    /// Returns all visible text in stable visual/model order.
    ///
    /// Text runs are taken from direct `Text` objects and from `Table` cells in
    /// row-major order. Paragraphs are separated by a single `\n`; empty
    /// paragraphs, cells, and runs are skipped and no labels are fabricated.
    pub fn plain_text(&self) -> String {
        let mut lines = Vec::new();
        for object in &self.objects {
            match &object.kind {
                ObjectKind::Text(text) => push_paragraph_text(&mut lines, text),
                ObjectKind::Table(table) => {
                    for cell in table.cells.iter().flatten() {
                        push_paragraph_text(&mut lines, cell);
                    }
                }
                ObjectKind::Shape(_) | ObjectKind::Line(_) | ObjectKind::Image(_) => {}
            }
        }
        lines.join("\n")
    }

    /// Returns a normalized, bounded summary of [`Self::plain_text`].
    ///
    /// Runs of whitespace collapse to a single ASCII space, leading and trailing
    /// whitespace is trimmed, and the result is truncated to at most 120 Unicode
    /// scalar values. Truncation never splits a scalar value.
    pub fn text_summary(&self) -> String {
        self.plain_text()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(120)
            .collect()
    }

    /// Returns `true` when the slide contains no objects.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Number of objects on the slide.
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Number of image objects on the slide.
    pub fn image_count(&self) -> usize {
        self.objects
            .iter()
            .filter(|object| matches!(object.kind, ObjectKind::Image(_)))
            .count()
    }
}

/// Collects non-empty paragraphs of `text` as newline-separated lines.
fn push_paragraph_text(lines: &mut Vec<String>, text: &Text) {
    for paragraph in &text.paragraphs {
        let run_text: String = paragraph.runs.iter().map(|run| run.text.as_str()).collect();
        if !run_text.is_empty() {
            lines.push(run_text);
        }
    }
}

/// Thumbnail projection of a slide for the thumbnail strip.
///
/// [`Eq`] is implemented manually: the frozen `slides-api` [`Background`] only
/// implements `PartialEq`, but its equality relation is reflexive, symmetric,
/// and transitive, so this projection is still a true equivalence relation.
#[derive(Debug, Clone, PartialEq)]
pub struct ThumbnailPreview {
    /// Slide identifier.
    pub slide_id: SlideId,
    /// Zero-based index in presentation order.
    pub index: usize,
    /// Whether the thumbnail is currently selected.
    pub selected: bool,
    /// Background. A slide without a background maps to [`Background::None`].
    pub background: Background,
    /// Thumbnail size as `(width, height)` in pixels.
    pub size: (u32, u32),
}

impl Eq for ThumbnailPreview {}

impl ThumbnailPreview {
    /// Creates a thumbnail preview from a document slide.
    ///
    /// Sizes are converted with truncation; a slide without an explicit
    /// background maps to [`Background::None`].
    pub fn from_slide(slide: &Slide, index: usize, selected: bool) -> Self {
        Self {
            slide_id: slide.id,
            index,
            selected,
            background: slide.background,
            size: (
                thumbnail_dimension(slide.size.width),
                thumbnail_dimension(slide.size.height),
            ),
        }
    }
}

/// Converts a slide-unit dimension to a pixel dimension.
///
/// Total and deterministic: finite, non-negative values not exceeding `u32::MAX`
/// truncate; anything else (negative, non-finite, overflowing) maps to zero.
fn thumbnail_dimension(value: f32) -> u32 {
    if value.is_finite() && value >= 0.0 && f64::from(value) <= f64::from(u32::MAX) {
        value as u32
    } else {
        0
    }
}

/// Category of a render-time warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderWarningKind {
    /// The image uses a format the renderer cannot display.
    UnsupportedImageFormat(ImageFormat),
    /// An image object carries no bytes.
    MissingImageBytes,
    /// Object geometry is invalid and cannot be rendered.
    InvalidGeometry,
    /// An object kind is not renderable.
    UnsupportedObject(&'static str),
}

/// A warning attached to a specific slide object during plan building.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderWarning {
    /// Identifier of the offending object.
    pub object_id: ObjectId,
    /// Warning category.
    pub kind: RenderWarningKind,
}

/// A validated, renderer-neutral plan for drawing one slide.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderPlan {
    /// Slide identifier the plan was derived from.
    pub slide_id: SlideId,
    /// Slide size in slide units.
    pub slide_size: Size,
    /// Viewport transform at plan time.
    pub viewport: Viewport,
    /// Deterministic render command list.
    pub commands: Vec<RenderCommand>,
    /// Non-fatal issues discovered while building the plan.
    pub warnings: Vec<RenderWarning>,
}

impl RenderPlan {
    /// Returns `true` when the slide size is strictly positive and finite and
    /// every command satisfies [`RenderCommand::is_valid`].
    ///
    /// Clip balance is not validated here.
    pub fn validate(&self) -> bool {
        self.slide_size.width.is_finite()
            && self.slide_size.height.is_finite()
            && self.slide_size.width > 0.0
            && self.slide_size.height > 0.0
            && self.commands.iter().all(RenderCommand::is_valid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slides_api::{
        Color, Image, ImageFormat, Paragraph, Point, SlideDirection, SlideLayout, Table, TextRun,
        Transition,
    };

    fn text_object(id: u64, text: &str) -> SlideObject {
        SlideObject::new(
            ObjectId::new(id),
            ObjectKind::Text(Text::plain(text)),
            Rect::new(Point::ZERO, 100.0, 50.0),
            Transform::IDENTITY,
        )
    }

    #[test]
    fn from_slide_projects_order_content_and_metadata() {
        let mut slide = Slide::new(SlideId::new(3));
        slide.size = Size::new(1920.0, 1080.0);
        slide.notes = Some("speaker notes".to_string());
        slide.layout = SlideLayout::TitleAndContent;
        slide.background = Background::Solid(Color::rgb(10, 20, 30));
        slide.transition = Some(Transition::new(250, SlideDirection::Left));

        slide.add_object(text_object(1, "Hello")).unwrap();

        slide
            .add_object(SlideObject::new(
                ObjectId::new(2),
                ObjectKind::Image(Image {
                    bytes: vec![1, 2, 3],
                    format: ImageFormat::Png,
                    alt_text: None,
                }),
                Rect::new(Point::ZERO, 10.0, 10.0),
                Transform::IDENTITY,
            ))
            .unwrap();

        let mut table = Table::new(2, 2).unwrap();
        table.cells[0] = Some(Text::plain("A"));
        table.cells[3] = Some(Text {
            paragraphs: vec![Paragraph {
                runs: vec![TextRun::new("sec"), TextRun::new("ond")],
                ..Paragraph::default()
            }],
        });
        slide
            .add_object(SlideObject::new(
                ObjectId::new(3),
                ObjectKind::Table(table),
                Rect::new(Point::ZERO, 40.0, 40.0),
                Transform::IDENTITY,
            ))
            .unwrap();

        let view = SlideView::from_slide(&slide);
        assert_eq!(view.id, SlideId::new(3));
        assert_eq!(view.size, Size::new(1920.0, 1080.0));
        assert_eq!(view.notes.as_deref(), Some("speaker notes"));
        assert_eq!(view.layout, SlideLayout::TitleAndContent);
        assert_eq!(view.background, Background::Solid(Color::rgb(10, 20, 30)));
        assert_eq!(
            view.transition,
            Some(Transition::new(250, SlideDirection::Left))
        );
        assert_eq!(view.object_count(), 3);
        assert_eq!(view.image_count(), 1);
        assert!(!view.is_empty());

        // Model order: text, image, table. Table cells are read row-major,
        // empty cells skipped; paragraphs join on '\n'; no labels fabricated.
        assert_eq!(view.plain_text(), "Hello\nA\nsecond");
        assert_eq!(view.text_summary(), "Hello A second");

        assert_eq!(view.objects[0].id, ObjectId::new(1));
        assert_eq!(view.objects[0].kind, ObjectKind::Text(Text::plain("Hello")));
        assert!(matches!(view.objects[1].kind, ObjectKind::Image(_)));
        assert!(matches!(view.objects[2].kind, ObjectKind::Table(_)));
    }

    #[test]
    fn empty_slide_has_no_objects_and_no_text() {
        let view = SlideView::from_slide(&Slide::new(SlideId::new(7)));
        assert!(view.is_empty());
        assert_eq!(view.object_count(), 0);
        assert_eq!(view.image_count(), 0);
        assert_eq!(view.plain_text(), "");
        assert_eq!(view.text_summary(), "");
    }

    #[test]
    fn text_summary_normalizes_whitespace_without_touching_plain_text() {
        let text = Text {
            paragraphs: vec![
                Paragraph::plain("  leading\tand\r\n  internal "),
                Paragraph::plain("   spaced    out   "),
            ],
        };
        let mut slide = Slide::new(SlideId::new(2));
        slide
            .add_object(SlideObject::new(
                ObjectId::new(1),
                ObjectKind::Text(text),
                Rect::new(Point::ZERO, 100.0, 50.0),
                Transform::IDENTITY,
            ))
            .unwrap();
        let view = SlideView::from_slide(&slide);
        assert_eq!(
            view.plain_text(),
            "  leading\tand\r\n  internal \n   spaced    out   "
        );
        assert_eq!(view.text_summary(), "leading and internal spaced out");
    }

    #[test]
    fn text_summary_truncates_by_unicode_scalar_values() {
        let long = "😀".repeat(100) + &"x".repeat(50);
        let mut slide = Slide::new(SlideId::new(1));
        slide.add_object(text_object(1, &long)).unwrap();
        let view = SlideView::from_slide(&slide);
        let summary = view.text_summary();
        // 150 scalar values in, 120 out; every emoji is a single scalar value.
        assert_eq!(summary.chars().count(), 120);
        assert_eq!(summary, "😀".repeat(100) + &"x".repeat(20));
    }

    #[test]
    fn thumbnail_preview_maps_selection_background_and_valid_size() {
        let mut slide = Slide::new(SlideId::new(5));
        slide.background = Background::Solid(Color::rgb(200, 100, 0));
        let preview = ThumbnailPreview::from_slide(&slide, 2, true);
        assert_eq!(preview.slide_id, SlideId::new(5));
        assert_eq!(preview.index, 2);
        assert!(preview.selected);
        assert_eq!(
            preview.background,
            Background::Solid(Color::rgb(200, 100, 0))
        );
        assert_eq!(preview.size, (1280, 720));

        let plain = ThumbnailPreview::from_slide(&Slide::new(SlideId::new(6)), 0, false);
        assert!(!plain.selected);
        assert_eq!(plain.background, Background::None);
        assert_eq!(plain.size, (1280, 720));
    }

    #[test]
    fn thumbnail_preview_size_conversion_is_total_and_deterministic() {
        let mut slide = Slide::new(SlideId::new(8));

        slide.size = Size::new(-1.0, 720.0);
        assert_eq!(
            ThumbnailPreview::from_slide(&slide, 0, false).size,
            (0, 720)
        );

        slide.size = Size::new(f32::NAN, f32::INFINITY);
        assert_eq!(ThumbnailPreview::from_slide(&slide, 0, false).size, (0, 0));

        slide.size = Size::new(12.9, 3.9);
        assert_eq!(ThumbnailPreview::from_slide(&slide, 0, false).size, (12, 3));

        slide.size = Size::new(4_294_967_296.0, 1.0);
        assert_eq!(ThumbnailPreview::from_slide(&slide, 0, false).size, (0, 1));
    }

    #[test]
    fn render_warning_carries_object_id_and_kind() {
        let warning = RenderWarning {
            object_id: ObjectId::new(9),
            kind: RenderWarningKind::UnsupportedImageFormat(ImageFormat::Svg),
        };
        assert_eq!(warning.object_id, ObjectId::new(9));
        assert_eq!(
            warning.kind,
            RenderWarningKind::UnsupportedImageFormat(ImageFormat::Svg)
        );
        assert_eq!(
            RenderWarningKind::UnsupportedObject("shape"),
            RenderWarningKind::UnsupportedObject("shape")
        );
        assert_eq!(
            RenderWarningKind::MissingImageBytes,
            RenderWarningKind::MissingImageBytes
        );
        assert_eq!(
            RenderWarningKind::InvalidGeometry,
            RenderWarningKind::InvalidGeometry
        );
    }

    #[test]
    fn render_plan_validate_accepts_valid_commands() {
        let plan = RenderPlan {
            slide_id: SlideId::new(1),
            slide_size: Size::new(1280.0, 720.0),
            viewport: Viewport::IDENTITY,
            commands: vec![
                RenderCommand::Clear {
                    color: [1.0, 1.0, 1.0, 1.0],
                },
                RenderCommand::FillRect {
                    rect: Rect::new(Point::ZERO, 100.0, 100.0),
                    color: [0.0, 0.0, 0.0, 1.0],
                },
                RenderCommand::PushClip {
                    rect: Rect::new(Point::ZERO, 10.0, 10.0),
                },
                RenderCommand::PopClip,
            ],
            warnings: Vec::new(),
        };
        assert!(plan.validate());
    }

    #[test]
    fn render_plan_validate_rejects_invalid_geometry_and_zero_slide_size() {
        let valid = RenderPlan {
            slide_id: SlideId::new(1),
            slide_size: Size::new(1280.0, 720.0),
            viewport: Viewport::IDENTITY,
            commands: vec![RenderCommand::Clear {
                color: [0.0, 0.0, 0.0, 1.0],
            }],
            warnings: Vec::new(),
        };

        let invalid_rect = RenderPlan {
            commands: vec![RenderCommand::FillRect {
                rect: Rect::new(Point::new(f32::NAN, 0.0), 100.0, 100.0),
                color: [0.0, 0.0, 0.0, 1.0],
            }],
            ..valid.clone()
        };
        assert!(!invalid_rect.validate());

        let zero_size = RenderPlan {
            slide_size: Size::new(0.0, 720.0),
            ..valid.clone()
        };
        assert!(!zero_size.validate());

        let non_finite_size = RenderPlan {
            slide_size: Size::new(f32::INFINITY, 720.0),
            ..valid.clone()
        };
        assert!(!non_finite_size.validate());

        let empty_image = RenderPlan {
            commands: vec![RenderCommand::DrawImage {
                rect: Rect::new(Point::ZERO, 10.0, 10.0),
                format: ImageFormat::Png,
                bytes: Vec::new(),
                alt_text: None,
            }],
            ..valid
        };
        assert!(!empty_image.validate());
    }
}
