use crate::view::{RenderPlan, RenderWarning, RenderWarningKind};
use slides_api::{
    Background, Color, ImageFormat, ObjectId, ObjectKind, Point, Rect, Slide, Text, Transform,
};
use slides_canvas::{RectExt, RenderCommand, Viewport};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Warn {
    InvalidGeometry,
    UnsupportedObject(&'static str),
    MissingImageBytes,
    UnsupportedImageFormat(ImageFormat),
}

fn warn(kind: Warn, object_id: ObjectId) -> RenderWarning {
    RenderWarning {
        object_id,
        kind: match kind {
            Warn::InvalidGeometry => RenderWarningKind::InvalidGeometry,
            Warn::UnsupportedObject(reason) => RenderWarningKind::UnsupportedObject(reason),
            Warn::MissingImageBytes => RenderWarningKind::MissingImageBytes,
            Warn::UnsupportedImageFormat(format) => {
                RenderWarningKind::UnsupportedImageFormat(format)
            }
        },
    }
}

fn normalized(color: Color) -> [f32; 4] {
    [
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
        color.a as f32 / 255.0,
    ]
}

fn build_background_clear(background: Background) -> RenderCommand {
    let color = match background {
        Background::None => [1.0, 1.0, 1.0, 1.0],
        Background::Solid(color) => normalized(color),
    };
    RenderCommand::Clear { color }
}

fn transformed_rect(rect: Rect, transform: Transform, viewport: Viewport) -> Option<Rect> {
    let rect = rect.apply_rect(transform);
    if rect.is_valid() {
        let rect = rect.apply_rect(viewport.transform);
        if rect.is_valid() { Some(rect) } else { None }
    } else {
        None
    }
}

fn rotate_point(point: Point, center: Point, angle: f32) -> Point {
    if !point.is_valid() || !center.is_valid() || !angle.is_finite() {
        return point;
    }
    let (sin, cos) = angle.sin_cos();
    let dx = point.x - center.x;
    let dy = point.y - center.y;
    Point::new(
        center.x + dx * cos - dy * sin,
        center.y + dx * sin + dy * cos,
    )
}

fn transform_point(mut point: Point, transform: Transform, center: Point) -> Point {
    point.x = (point.x + transform.translation.x) * transform.scale;
    point.y = (point.y + transform.translation.y) * transform.scale;
    rotate_point(point, center, transform.rotation_degrees.to_radians())
}

fn translate_scale(point: Point, translation: Point, scale: f32) -> Point {
    Point {
        x: (point.x + translation.x) * scale,
        y: (point.y + translation.y) * scale,
    }
}

fn text_is_empty(text: &Text) -> bool {
    text.paragraphs
        .iter()
        .all(|paragraph| paragraph.runs.iter().all(|run| run.text.is_empty()))
}

fn table_grid_edges(origin: f32, extent: f32, weights: &[f32]) -> Option<Vec<f32>> {
    if !origin.is_finite()
        || !extent.is_finite()
        || extent <= 0.0
        || weights.is_empty()
        || weights
            .iter()
            .any(|weight| !weight.is_finite() || *weight <= 0.0)
    {
        return None;
    }

    let total: f32 = weights.iter().sum();
    let end = origin + extent;
    if !total.is_finite() || total <= 0.0 || !end.is_finite() {
        return None;
    }

    let mut edges = Vec::with_capacity(weights.len() + 1);
    edges.push(origin);
    let mut accumulated = 0.0;
    for (index, weight) in weights.iter().enumerate() {
        accumulated += *weight;
        let edge = if index + 1 == weights.len() {
            end
        } else {
            origin + extent * accumulated / total
        };
        if !edge.is_finite() || edge <= *edges.last()? {
            return None;
        }
        edges.push(edge);
    }
    Some(edges)
}

fn image_format_name(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Svg => "svg",
        ImageFormat::Unknown => "unknown",
    }
}

/// Controls whether a plan is the full slide or a navigation thumbnail.
///
/// A thumbnail must never clone or decode the original raster payload: the
/// central canvas is the source of visual fidelity, while the sidebar remains
/// bounded even for presentations with large embedded media.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum RenderPlanPurpose {
    Full,
    Thumbnail,
}

/// Builds a deterministic full-fidelity render plan for one slide and viewport.
pub fn build_plan(slide: &Slide, viewport: Viewport) -> RenderPlan {
    build_plan_for_purpose(slide, viewport, RenderPlanPurpose::Full)
}

/// Builds a bounded navigation-thumbnail plan without raster image payloads.
pub(crate) fn build_thumbnail_plan(slide: &Slide, viewport: Viewport) -> RenderPlan {
    build_plan_for_purpose(slide, viewport, RenderPlanPurpose::Thumbnail)
}

fn build_plan_for_purpose(
    slide: &Slide,
    viewport: Viewport,
    purpose: RenderPlanPurpose,
) -> RenderPlan {
    let mut commands = Vec::new();
    let mut warnings = Vec::new();

    commands.push(build_background_clear(slide.background));

    for object in &slide.objects {
        let id = object.id;
        let object_transform = object.transform;
        let rect = match transformed_rect(object.bounds, object_transform, viewport) {
            Some(rect) => rect,
            None => {
                warnings.push(warn(Warn::InvalidGeometry, id));
                continue;
            }
        };

        match &object.kind {
            ObjectKind::Text(text) => {
                if text_is_empty(text) {
                    commands.push(RenderCommand::FillRect {
                        rect,
                        color: [0.96, 0.96, 0.96, 1.0],
                    });
                    commands.push(RenderCommand::PushClip { rect });
                    commands.push(RenderCommand::DrawText {
                        rect,
                        text: Text::plain("[empty text]"),
                        color: [0.0, 0.0, 0.0, 1.0],
                    });
                    commands.push(RenderCommand::PopClip);
                    warnings.push(warn(Warn::UnsupportedObject("empty text"), id));
                } else {
                    commands.push(RenderCommand::PushClip { rect });
                    commands.push(RenderCommand::DrawText {
                        rect,
                        text: text.clone(),
                        color: [0.0, 0.0, 0.0, 1.0],
                    });
                    commands.push(RenderCommand::PopClip);
                }
            }
            ObjectKind::Image(image) => {
                if matches!(purpose, RenderPlanPurpose::Thumbnail) {
                    // Sidebar thumbnails intentionally show a lightweight image
                    // frame. Do not clone `image.bytes`: one large image must
                    // not be copied and decoded for every visible thumbnail.
                    commands.push(RenderCommand::FillRect {
                        rect,
                        color: [0.84, 0.87, 0.91, 1.0],
                    });
                    commands.push(RenderCommand::StrokeRect {
                        rect,
                        color: [0.55, 0.62, 0.70, 1.0],
                        width: 0.75,
                    });
                } else if image.bytes.is_empty() {
                    commands.push(RenderCommand::FillRect {
                        rect,
                        color: [0.96, 0.96, 0.96, 1.0],
                    });
                    commands.push(RenderCommand::PushClip { rect });
                    commands.push(RenderCommand::DrawText {
                        rect,
                        text: Text::plain(format!(
                            "[image: {}]",
                            image
                                .alt_text
                                .as_deref()
                                .unwrap_or(image_format_name(image.format))
                        )),
                        color: [0.0, 0.0, 0.0, 1.0],
                    });
                    commands.push(RenderCommand::PopClip);
                    warnings.push(warn(Warn::MissingImageBytes, id));
                } else if matches!(
                    image.format,
                    ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Bmp
                ) {
                    commands.push(RenderCommand::DrawImage {
                        rect,
                        format: image.format,
                        bytes: image.bytes.clone(),
                        alt_text: image.alt_text.clone(),
                    });
                } else {
                    commands.push(RenderCommand::FillRect {
                        rect,
                        color: [0.96, 0.96, 0.96, 1.0],
                    });
                    commands.push(RenderCommand::PushClip { rect });
                    commands.push(RenderCommand::DrawText {
                        rect,
                        text: Text::plain(format!(
                            "[image: {}]",
                            image
                                .alt_text
                                .as_deref()
                                .unwrap_or(image_format_name(image.format))
                        )),
                        color: [0.0, 0.0, 0.0, 1.0],
                    });
                    commands.push(RenderCommand::PopClip);
                    warnings.push(warn(Warn::UnsupportedImageFormat(image.format), id));
                }
            }
            ObjectKind::Shape(shape) => {
                let stroke_width = shape
                    .stroke
                    .map(|_| shape.stroke_width * object_transform.scale * viewport.transform.scale)
                    .unwrap_or(0.0);
                if !stroke_width.is_finite() || stroke_width < 0.0 {
                    warnings.push(warn(Warn::InvalidGeometry, id));
                    continue;
                }
                if let Some(fill_color) = shape.fill {
                    commands.push(match shape.kind {
                        slides_api::ShapeKind::Ellipse => RenderCommand::FillEllipse {
                            rect,
                            color: normalized(fill_color),
                        },
                        slides_api::ShapeKind::Chevron => RenderCommand::FillChevron {
                            rect,
                            color: normalized(fill_color),
                        },
                        slides_api::ShapeKind::Rectangle
                        | slides_api::ShapeKind::RoundedRectangle => RenderCommand::FillRect {
                            rect,
                            color: normalized(fill_color),
                        },
                    });
                }
                if let Some(stroke_color) = shape.stroke {
                    commands.push(match shape.kind {
                        slides_api::ShapeKind::Ellipse => RenderCommand::StrokeEllipse {
                            rect,
                            color: normalized(stroke_color),
                            width: stroke_width.max(0.0),
                        },
                        slides_api::ShapeKind::Chevron => RenderCommand::StrokeChevron {
                            rect,
                            color: normalized(stroke_color),
                            width: stroke_width.max(0.0),
                        },
                        slides_api::ShapeKind::Rectangle
                        | slides_api::ShapeKind::RoundedRectangle => RenderCommand::StrokeRect {
                            rect,
                            color: normalized(stroke_color),
                            width: stroke_width.max(0.0),
                        },
                    });
                }
                if shape.fill.is_none() && shape.stroke.is_none() {
                    commands.push(match shape.kind {
                        slides_api::ShapeKind::Ellipse => RenderCommand::FillEllipse {
                            rect,
                            color: [0.96, 0.96, 0.96, 1.0],
                        },
                        slides_api::ShapeKind::Chevron => RenderCommand::FillChevron {
                            rect,
                            color: [0.96, 0.96, 0.96, 1.0],
                        },
                        slides_api::ShapeKind::Rectangle
                        | slides_api::ShapeKind::RoundedRectangle => RenderCommand::FillRect {
                            rect,
                            color: [0.96, 0.96, 0.96, 1.0],
                        },
                    });
                    warnings.push(warn(Warn::UnsupportedObject("unpainted shape"), id));
                }
            }
            ObjectKind::Line(line) => {
                let obj_center = translate_scale(
                    object.bounds.center(),
                    object_transform.translation,
                    object_transform.scale,
                );
                let mut start = transform_point(line.start, object_transform, obj_center);
                let mut end = transform_point(line.end, object_transform, obj_center);
                let post_obj_rect = match Rect::new(
                    Point::new(start.x.min(end.x), start.y.min(end.y)),
                    (end.x - start.x).abs(),
                    (end.y - start.y).abs(),
                ) {
                    rect if rect.is_valid() => rect,
                    _ => {
                        warnings.push(warn(Warn::InvalidGeometry, id));
                        continue;
                    }
                };
                let vp_center = translate_scale(
                    post_obj_rect.center(),
                    viewport.transform.translation,
                    viewport.transform.scale,
                );
                start = transform_point(start, viewport.transform, vp_center);
                end = transform_point(end, viewport.transform, vp_center);

                let width = line.stroke_width * object_transform.scale * viewport.transform.scale;
                if !width.is_finite() || width < 0.0 {
                    warnings.push(warn(Warn::InvalidGeometry, id));
                    continue;
                }

                commands.push(RenderCommand::DrawLine {
                    start,
                    end,
                    color: line.stroke.map(normalized).unwrap_or([0.0, 0.0, 0.0, 1.0]),
                    width: width.max(0.0),
                });
            }
            ObjectKind::Table(table) => {
                if !table.is_valid() {
                    warnings.push(warn(Warn::InvalidGeometry, id));
                    continue;
                }
                let Some(column_edges) =
                    table_grid_edges(rect.origin.x, rect.width, &table.column_widths)
                else {
                    warnings.push(warn(Warn::InvalidGeometry, id));
                    continue;
                };
                let Some(row_edges) =
                    table_grid_edges(rect.origin.y, rect.height, &table.row_heights)
                else {
                    warnings.push(warn(Warn::InvalidGeometry, id));
                    continue;
                };
                let stroke_width = (viewport.transform.scale * object_transform.scale).max(0.0);
                if !stroke_width.is_finite() {
                    warnings.push(warn(Warn::InvalidGeometry, id));
                    continue;
                }

                let mut cell_rects = Vec::new();
                let mut all_cells_valid = true;
                for row in 0..table.rows {
                    for column in 0..table.columns {
                        let index = row * table.columns + column;
                        let layout = table.cell_layout[index];
                        if layout.is_merge_continuation {
                            continue;
                        }
                        let end_column = column + layout.column_span;
                        let end_row = row + layout.row_span;
                        let cell_rect = Rect::new(
                            Point::new(column_edges[column], row_edges[row]),
                            column_edges[end_column] - column_edges[column],
                            row_edges[end_row] - row_edges[row],
                        );
                        if !cell_rect.is_valid() {
                            all_cells_valid = false;
                        } else {
                            cell_rects.push((index, cell_rect));
                        }
                    }
                }
                if !all_cells_valid || cell_rects.is_empty() {
                    warnings.push(warn(Warn::InvalidGeometry, id));
                    continue;
                }

                for (index, cell_rect) in cell_rects {
                    commands.push(RenderCommand::StrokeRect {
                        rect: cell_rect,
                        color: [0.0, 0.0, 0.0, 1.0],
                        width: stroke_width,
                    });
                    if let Some(cell) = table.cells.get(index)
                        && let Some(text) = cell
                    {
                        if text_is_empty(text) {
                            commands.push(RenderCommand::PushClip { rect: cell_rect });
                            commands.push(RenderCommand::DrawText {
                                rect: cell_rect,
                                text: Text::plain("[empty text]"),
                                color: [0.0, 0.0, 0.0, 1.0],
                            });
                            commands.push(RenderCommand::PopClip);
                            warnings.push(warn(Warn::UnsupportedObject("empty text"), id));
                        } else {
                            commands.push(RenderCommand::PushClip { rect: cell_rect });
                            commands.push(RenderCommand::DrawText {
                                rect: cell_rect,
                                text: text.clone(),
                                color: [0.0, 0.0, 0.0, 1.0],
                            });
                            commands.push(RenderCommand::PopClip);
                        }
                    }
                }
            }
        }
    }

    RenderPlan {
        slide_id: slide.id,
        slide_size: slide.size,
        viewport,
        commands,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slides_api::{Color, ImageFormat, ObjectId, Size, SlideObject, Table, Text};

    fn sample_slide(objects: Vec<SlideObject>) -> Slide {
        Slide {
            id: slides_api::SlideId::new(1),
            size: Size::new(100.0, 100.0),
            notes: None,
            layout: slides_api::SlideLayout::Blank,
            background: Background::None,
            transition: None,
            objects,
        }
    }

    fn test_viewport() -> Viewport {
        Viewport {
            transform: Transform::IDENTITY,
        }
    }

    #[test]
    fn background_clear_is_first() {
        let mut slide = sample_slide(vec![]);
        slide.background = Background::None;
        let plan = build_plan(&slide, test_viewport());
        assert!(matches!(
            plan.commands[0],
            RenderCommand::Clear { color } if color == [1.0, 1.0, 1.0, 1.0]
        ));
        assert!(plan.validate());

        slide.background = Background::Solid(Color::rgb(10, 20, 30));
        let plan = build_plan(&slide, test_viewport());
        assert!(matches!(
            plan.commands[0],
            RenderCommand::Clear { color } if color == [10.0/255.0, 20.0/255.0, 30.0/255.0, 1.0]
        ));
        assert!(plan.validate());
    }

    #[test]
    fn unsupported_image_gets_placeholder() {
        let objects = vec![
            SlideObject::new(
                ObjectId::new(1),
                ObjectKind::Image(slides_api::Image {
                    bytes: vec![1, 2, 3],
                    format: ImageFormat::Svg,
                    alt_text: Some("logo".into()),
                }),
                Rect::new(Point::ZERO, 20.0, 20.0),
                Transform::IDENTITY,
            ),
            SlideObject::new(
                ObjectId::new(2),
                ObjectKind::Image(slides_api::Image {
                    bytes: vec![],
                    format: ImageFormat::Png,
                    alt_text: None,
                }),
                Rect::new(Point::ZERO, 20.0, 20.0),
                Transform::IDENTITY,
            ),
        ];
        let slide = sample_slide(objects);
        let plan = build_plan(&slide, test_viewport());
        assert!(plan.validate());
        let warnings: Vec<_> = plan
            .warnings
            .iter()
            .map(|warning| (warning.kind.clone(), warning.object_id))
            .collect();
        assert_eq!(
            warnings,
            vec![
                (
                    RenderWarningKind::UnsupportedImageFormat(ImageFormat::Svg),
                    ObjectId::new(1)
                ),
                (RenderWarningKind::MissingImageBytes, ObjectId::new(2)),
            ]
        );
    }

    #[test]
    fn invalid_stroke_emits_no_partial_fill() {
        let shape = slides_api::Shape {
            kind: slides_api::ShapeKind::Rectangle,
            fill: Some(Color::BLACK),
            stroke: Some(Color::TRANSPARENT),
            stroke_width: -1.0,
        };
        let object = SlideObject::new(
            ObjectId::new(1),
            ObjectKind::Shape(shape),
            Rect::new(Point::ZERO, 20.0, 20.0),
            Transform::IDENTITY,
        );
        let slide = sample_slide(vec![object]);
        let plan = build_plan(&slide, test_viewport());
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(plan.warnings.len(), 1);
        assert!(matches!(
            plan.warnings[0].kind,
            RenderWarningKind::InvalidGeometry
        ));
    }

    #[test]
    fn invalid_transform_is_skipped() {
        let object = SlideObject::new(
            ObjectId::new(1),
            ObjectKind::Text(Text::plain("bad")),
            Rect::new(Point::ZERO, 10.0, 10.0),
            Transform {
                translation: Point::ZERO,
                scale: f32::NAN,
                rotation_degrees: 0.0,
            },
        );
        let slide = sample_slide(vec![object]);
        let plan = build_plan(&slide, test_viewport());
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(plan.warnings.len(), 1);
        assert!(matches!(
            plan.warnings[0].kind,
            RenderWarningKind::InvalidGeometry
        ));
        assert!(plan.validate());
    }

    #[test]
    fn thumbnail_plan_replaces_raster_payload_with_lightweight_frame() {
        let image_bytes = vec![9_u8; 1_048_576];
        let object = SlideObject::new(
            ObjectId::new(1),
            ObjectKind::Image(slides_api::Image {
                bytes: image_bytes.clone(),
                format: ImageFormat::Png,
                alt_text: Some("large image".into()),
            }),
            Rect::new(Point::ZERO, 20.0, 20.0),
            Transform::IDENTITY,
        );
        let slide = sample_slide(vec![object]);
        let full = build_plan(&slide, test_viewport());
        let thumbnail = build_thumbnail_plan(&slide, test_viewport());

        assert!(matches!(
            full.commands[1],
            RenderCommand::DrawImage { ref bytes, .. } if bytes.len() == image_bytes.len()
        ));
        assert!(thumbnail.validate());
        assert!(
            !thumbnail
                .commands
                .iter()
                .any(|command| matches!(command, RenderCommand::DrawImage { .. }))
        );
        assert!(matches!(
            thumbnail.commands[1],
            RenderCommand::FillRect { .. }
        ));
        assert!(matches!(
            thumbnail.commands[2],
            RenderCommand::StrokeRect { .. }
        ));
    }

    #[test]
    fn image_bytes_are_preserved() {
        let object = SlideObject::new(
            ObjectId::new(1),
            ObjectKind::Image(slides_api::Image {
                bytes: vec![9, 9, 9],
                format: ImageFormat::Png,
                alt_text: Some("img".into()),
            }),
            Rect::new(Point::ZERO, 20.0, 20.0),
            Transform::IDENTITY,
        );
        let slide = sample_slide(vec![object]);
        let plan = build_plan(&slide, test_viewport());
        assert_eq!(plan.commands.len(), 2);
        assert!(matches!(
            plan.commands[1],
            RenderCommand::DrawImage { ref bytes, .. } if bytes == &[9, 9, 9]
        ));
        assert!(plan.validate());
    }

    #[test]
    fn line_transform_and_viewport_placement() {
        let object = SlideObject::new(
            ObjectId::new(1),
            ObjectKind::Line(slides_api::Line {
                start: Point::new(0.0, 0.0),
                end: Point::new(10.0, 0.0),
                stroke: Some(Color::BLACK),
                stroke_width: 2.0,
            }),
            Rect::new(Point::new(0.0, -5.0), 10.0, 10.0),
            Transform::with_rotation(Point::new(0.0, 0.0), 1.0, 90.0),
        );
        let viewport = Viewport {
            transform: Transform::IDENTITY,
        };
        let slide = sample_slide(vec![object]);
        let plan = build_plan(&slide, viewport);
        assert!(plan.validate());
        assert_eq!(plan.commands.len(), 2);
        let RenderCommand::DrawLine { start, end, .. } = &plan.commands[1] else {
            panic!("expected DrawLine")
        };
        // After object rotation of 90 degrees around transformed bounds center (5,0),
        // start (0,0) -> (5,-5), end (10,0) -> (5,5).
        assert_eq!(*start, Point::new(5.0, -5.0));
        assert_eq!(*end, Point::new(5.0, 5.0));
    }

    #[test]
    fn table_commands_and_empty_cell_warning() {
        let mut table = Table::new(2, 2).unwrap();
        table.cells[0] = Some(Text::plain("A"));
        table.cells[1] = Some(Text::plain(""));
        table.cells[2] = None;
        table.cells[3] = Some(Text::plain("B"));
        let objects = vec![SlideObject::new(
            ObjectId::new(1),
            ObjectKind::Table(table),
            Rect::new(Point::ZERO, 100.0, 100.0),
            Transform::IDENTITY,
        )];
        let slide = sample_slide(objects);
        let plan = build_plan(&slide, test_viewport());
        assert!(plan.validate());
        assert_eq!(plan.warnings.len(), 1);
        assert!(matches!(
            plan.warnings[0].kind,
            RenderWarningKind::UnsupportedObject("empty text")
        ));
        // 1 background clear + 4 StrokeRect + 3 cells x PushClip/DrawText/PopClip
        assert_eq!(plan.commands.len(), 14);
    }

    #[test]
    fn table_commands_preserve_merged_cell_geometry() {
        let mut table = Table::new(2, 3).unwrap();
        table.cells[0] = Some(Text::plain("Header"));
        table.cells[2] = Some(Text::plain("Top"));
        table.cells[5] = Some(Text::plain("Bottom"));
        table.cell_layout[0].row_span = 2;
        table.cell_layout[0].column_span = 2;
        table.cell_layout[1].is_merge_continuation = true;
        table.cell_layout[3].is_merge_continuation = true;
        table.cell_layout[4].is_merge_continuation = true;
        assert!(table.is_valid());

        let slide = sample_slide(vec![SlideObject::new(
            ObjectId::new(1),
            ObjectKind::Table(table),
            Rect::new(Point::ZERO, 120.0, 80.0),
            Transform::IDENTITY,
        )]);
        let plan = build_plan(&slide, test_viewport());
        assert!(plan.validate());

        let cell_rects = plan
            .commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::StrokeRect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(cell_rects.len(), 3);
        assert!(cell_rects.contains(&Rect::new(Point::ZERO, 80.0, 80.0)));
        assert!(cell_rects.contains(&Rect::new(Point::new(80.0, 0.0), 40.0, 40.0)));
        assert!(cell_rects.contains(&Rect::new(Point::new(80.0, 40.0), 40.0, 40.0)));
        // 1 background clear + 3 merged-aware borders + 3 cells x Push/Draw/Pop.
        assert_eq!(plan.commands.len(), 13);
    }

    #[test]
    fn table_commands_preserve_non_uniform_grid_dimensions() {
        let mut table = Table::new(2, 2).unwrap();
        table.column_widths = vec![1.0, 3.0];
        table.row_heights = vec![1.0, 3.0];
        assert!(table.is_valid());

        let slide = sample_slide(vec![SlideObject::new(
            ObjectId::new(1),
            ObjectKind::Table(table),
            Rect::new(Point::ZERO, 400.0, 200.0),
            Transform::IDENTITY,
        )]);
        let plan = build_plan(&slide, test_viewport());
        assert!(plan.validate());

        let cell_rects = plan
            .commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::StrokeRect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(cell_rects.len(), 4);
        assert!(cell_rects.contains(&Rect::new(Point::ZERO, 100.0, 50.0)));
        assert!(cell_rects.contains(&Rect::new(Point::new(100.0, 0.0), 300.0, 50.0)));
        assert!(cell_rects.contains(&Rect::new(Point::new(0.0, 50.0), 100.0, 150.0)));
        assert!(cell_rects.contains(&Rect::new(Point::new(100.0, 50.0), 300.0, 150.0)));
    }

    #[test]
    fn invalid_table_is_skipped() {
        let table = Table {
            rows: 2,
            columns: 2,
            column_widths: vec![1.0; 2],
            row_heights: vec![1.0; 2],
            cells: vec![None; 3],
            cell_layout: vec![slides_api::TableCellLayout::default(); 4],
        };
        let objects = vec![SlideObject::new(
            ObjectId::new(1),
            ObjectKind::Table(table),
            Rect::new(Point::ZERO, 100.0, 100.0),
            Transform::IDENTITY,
        )];
        let slide = sample_slide(objects);
        let plan = build_plan(&slide, test_viewport());
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(plan.warnings.len(), 1);
        assert!(matches!(
            plan.warnings[0].kind,
            RenderWarningKind::InvalidGeometry
        ));
    }

    #[test]
    fn determinism_holds_for_same_inputs() {
        let objects = vec![
            SlideObject::new(
                ObjectId::new(1),
                ObjectKind::Shape(slides_api::Shape {
                    kind: slides_api::ShapeKind::Rectangle,
                    fill: Some(Color::WHITE),
                    stroke: Some(Color::BLACK),
                    stroke_width: 2.0,
                }),
                Rect::new(Point::ZERO, 20.0, 20.0),
                Transform::IDENTITY,
            ),
            SlideObject::new(
                ObjectId::new(2),
                ObjectKind::Line(slides_api::Line {
                    start: Point::new(0.0, 0.0),
                    end: Point::new(10.0, 10.0),
                    stroke: Some(Color::BLACK),
                    stroke_width: 1.0,
                }),
                Rect::new(Point::ZERO, 10.0, 10.0),
                Transform::IDENTITY,
            ),
        ];
        let slide = sample_slide(objects);
        let viewport = test_viewport();
        let first = build_plan(&slide, viewport);
        let second = build_plan(&slide, viewport);
        assert_eq!(first, second);
    }

    #[test]
    fn ellipse_shape_emits_ellipse_commands_and_preserves_plan_validity() {
        let shape = slides_api::Shape {
            kind: slides_api::ShapeKind::Ellipse,
            fill: Some(Color::rgb(20, 40, 60)),
            stroke: Some(Color::BLACK),
            stroke_width: 2.0,
        };
        let object = SlideObject::new(
            ObjectId::new(1),
            ObjectKind::Shape(shape),
            Rect::new(Point::ZERO, 40.0, 20.0),
            Transform::IDENTITY,
        );
        let plan = build_plan(&sample_slide(vec![object]), test_viewport());
        assert!(plan.validate());
        assert!(matches!(
            plan.commands[1],
            RenderCommand::FillEllipse { .. }
        ));
        assert!(matches!(
            plan.commands[2],
            RenderCommand::StrokeEllipse { .. }
        ));
    }

    #[test]
    fn chevron_shape_emits_chevron_commands_and_preserves_plan_validity() {
        let shape = slides_api::Shape {
            kind: slides_api::ShapeKind::Chevron,
            fill: Some(Color::rgb(0, 70, 127)),
            stroke: Some(Color::BLACK),
            stroke_width: 2.0,
        };
        let object = SlideObject::new(
            ObjectId::new(1),
            ObjectKind::Shape(shape),
            Rect::new(Point::ZERO, 40.0, 20.0),
            Transform::IDENTITY,
        );
        let plan = build_plan(&sample_slide(vec![object]), test_viewport());
        assert!(plan.validate());
        assert!(matches!(
            plan.commands[1],
            RenderCommand::FillChevron { .. }
        ));
        assert!(matches!(
            plan.commands[2],
            RenderCommand::StrokeChevron { .. }
        ));
    }
}
