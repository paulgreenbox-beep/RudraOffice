use std::io::Cursor;

use zip::ZipArchive;

use office_oxide::{
    Document, DocumentFormat,
    ir::{
        DocumentIR, Element as IrElement, Image as IrImage, ImageFormat as IrImageFormat,
        InlineContent as IrInlineContent, LineSpacing as IrLineSpacing, List as IrList,
        ParagraphAlignment as IrParagraphAlignment, Section, TextBox as IrTextBox,
        TextSpan as IrTextSpan, UnderlineStyle as IrUnderlineStyle,
    },
    pptx::write::{PptxWriter, Run as PptxRun},
    pptx::{
        AutoShape as PptxAutoShape, GraphicContent as PptxGraphicContent, PptxDocument,
        Shape as PptxShape, ShapePosition as PptxShapePosition, Table as PptxTable,
        TextBody as PptxTextBody, TextContent as PptxTextContent,
    },
};
use slides_api::{
    Background, Capability, Color, EngineError, Image as SlideImage,
    ImageFormat as SlideImageFormat, Line as SlideLine, ObjectId, ObjectKind, Paragraph,
    ParagraphFormatting, Point, Presentation, PresentationId, Rect, Shape as SlideShape, ShapeKind,
    Size, Slide, SlideId, SlideObject, Table as SlideTable, TableCellLayout, Text, TextAlignment,
    TextFormatting, TextRun, Transform,
};

use crate::{Limits, PresentationCodec};

const PPTX_WIDTH_EMU: u64 = 12_192_000;
const PPTX_HEIGHT_EMU: u64 = 6_858_000;
const EMU_PER_TWIP: u64 = 635;
const DEFAULT_SLIDE_WIDTH: f32 = 1_280.0;
const DEFAULT_SLIDE_HEIGHT: f32 = 720.0;
const LIST_INDENT_UNITS: f32 = 24.0;
const MAX_PPTX_ZIP_ENTRIES: usize = 20_000;
const MAX_PPTX_ZIP_ENTRY_BYTES: u64 = 512 * 1024 * 1024;
const MAX_PPTX_DECOMPRESSED_PACKAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

static CAPABILITIES: [Capability; 4] = [
    Capability::LoadSave,
    Capability::Slides,
    Capability::Text,
    Capability::Images,
];

/// Real OOXML/PPTX codec powered by the locally vendored `office_oxide`.
///
/// The decoder projects the public OfficeOxide PPTX model instead of its
/// text-oriented IR. That preserves the baseline visual subset needed by the
/// viewer: positioned text, images, basic auto-shape placeholders, connector
/// lines and DrawingML tables. Source auto-shape geometry and styling are not
/// exposed by OfficeOxide yet, so a neutral rectangle is intentionally used
/// for non-text shapes; unknown graphic frames are rendered as a labelled
/// placeholder rather than being silently lost. Group transform fidelity and
/// PPTX export of shapes/tables remain unsupported.
///
/// `decode` contains a narrow panic fence around the real OOXML parser. Any
/// unwinding panic is mapped to `EngineError::CodecError`; normal typed errors
/// are preserved unchanged. This does not guard decompressed package size or
/// ZIP central-directory structure: ZIP-bomb / decompressed-package bounds
/// remain a caller-deployment hardening concern if truly unmitigated.
#[derive(Clone, Copy, Debug, Default)]
pub struct OfficeOxideCodec;

impl PresentationCodec for OfficeOxideCodec {
    fn capabilities(&self) -> &[Capability] {
        &CAPABILITIES
    }

    fn decode(&self, bytes: &[u8], limits: &Limits) -> Result<Presentation, EngineError> {
        limits.validate()?;
        limits.check_input_len(bytes.len())?;
        preflight_pptx_package(bytes)?;

        // Narrow panic fence: the real OOXML parser below can unwind on hostile
        // bytes even where it returns typed errors for the paths it detects.
        // Any unwinding panic is converted deterministically to a generic
        // `EngineError::CodecError`; the panic payload is never exposed.
        std::panic::catch_unwind(|| {
            let document = Document::from_reader(Cursor::new(bytes.to_vec()), DocumentFormat::Pptx)
                .map_err(|error| {
                    EngineError::CodecError(format!("office-oxide could not read PPTX: {error}"))
                })?;
            if let Some(pptx) = document.as_pptx() {
                return project_pptx_visual_subset(pptx, limits);
            }
            let ir = document.to_ir();
            let slide_count =
                u32::try_from(ir.sections.len()).map_err(|_| EngineError::LimitExceeded {
                    limit: u64::from(limits.max_slides),
                    actual: ir.sections.len() as u64,
                    context: "slides".to_string(),
                })?;
            limits.check_slides(slide_count)?;

            let mut presentation = Presentation::new(PresentationId::new(1));
            let mut total_shapes = 0_u64;
            let mut total_text_bytes = 0_u64;
            let mut total_image_bytes = 0_u64;

            for (index, section) in ir.sections.iter().enumerate() {
                let slide_id = SlideId::new(u64::try_from(index + 1).map_err(|_| {
                    EngineError::CodecError("slide identifier overflow".to_string())
                })?);
                let canvas = section_canvas_emu(section);
                let mut slide = Slide::new(slide_id);
                slide.size = slide_size_from_canvas(canvas)?;
                let mut next_object_id = 1_u64;
                let mut flow = Vec::new();

                for element in &section.elements {
                    if let IrElement::TextBox(text_box) = element {
                        push_positioned_elements(
                            &flow,
                            default_text_bounds(slide.size),
                            slide.size,
                            canvas,
                            &mut slide,
                            &mut next_object_id,
                            limits,
                            &mut total_shapes,
                            &mut total_text_bytes,
                            &mut total_image_bytes,
                        )?;
                        flow.clear();
                        let bounds = text_box_bounds(text_box, slide.size, canvas)?
                            .unwrap_or_else(|| default_text_bounds(slide.size));
                        push_positioned_elements(
                            &text_box.content,
                            bounds,
                            slide.size,
                            canvas,
                            &mut slide,
                            &mut next_object_id,
                            limits,
                            &mut total_shapes,
                            &mut total_text_bytes,
                            &mut total_image_bytes,
                        )?;
                    } else {
                        flow.push(element.clone());
                    }
                }
                push_positioned_elements(
                    &flow,
                    default_text_bounds(slide.size),
                    slide.size,
                    canvas,
                    &mut slide,
                    &mut next_object_id,
                    limits,
                    &mut total_shapes,
                    &mut total_text_bytes,
                    &mut total_image_bytes,
                )?;
                presentation.slides.push(slide);
            }

            presentation
                .validate()
                .map_err(EngineError::ValidationError)?;
            Ok(presentation)
        })
        .map_err(|_| {
            EngineError::CodecError("office-oxide codec panicked while decoding PPTX".to_string())
        })?
    }

    fn encode(&self, document: &Presentation, limits: &Limits) -> Result<Vec<u8>, EngineError> {
        limits.validate()?;
        document.validate().map_err(EngineError::ValidationError)?;

        let slide_count =
            u32::try_from(document.slides.len()).map_err(|_| EngineError::LimitExceeded {
                limit: u64::from(limits.max_slides),
                actual: document.slides.len() as u64,
                context: "slides".to_string(),
            })?;
        limits.check_slides(slide_count)?;

        let mut writer = PptxWriter::new();
        let expected_size = if let Some(first_slide) = document.slides.first() {
            let size = validate_slide_size(first_slide.size.width, first_slide.size.height)?;
            writer.set_presentation_size(size.0, size.1);
            Some(size)
        } else {
            None
        };

        let mut total_shapes = 0_u64;
        let mut total_text_bytes = 0_u64;
        let mut total_image_bytes = 0_u64;
        for slide in &document.slides {
            if slide.notes.is_some() {
                return Err(EngineError::UnsupportedCapability(
                    "PPTX speaker notes export".to_string(),
                ));
            }
            if slide.transition.is_some() {
                return Err(EngineError::UnsupportedCapability(
                    "PPTX transitions export".to_string(),
                ));
            }
            let size = validate_slide_size(slide.size.width, slide.size.height)?;
            if Some(size) != expected_size {
                return Err(EngineError::UnsupportedCapability(
                    "different slide sizes in one PPTX".to_string(),
                ));
            }

            let objects =
                u32::try_from(slide.objects.len()).map_err(|_| EngineError::LimitExceeded {
                    limit: u64::from(limits.max_shapes_per_slide),
                    actual: slide.objects.len() as u64,
                    context: "shapes per slide".to_string(),
                })?;
            limits.check_shapes_per_slide(objects)?;
            total_shapes = total_shapes
                .checked_add(u64::from(objects))
                .ok_or_else(|| {
                    EngineError::CodecError("total PPTX object count overflow".to_string())
                })?;
            limits.check_total_shapes(total_shapes)?;

            let target = writer.add_slide();
            for object in &slide.objects {
                if object.transform != Transform::IDENTITY {
                    return Err(EngineError::UnsupportedCapability(
                        "PPTX object transforms".to_string(),
                    ));
                }
                match &object.kind {
                    ObjectKind::Text(text) => {
                        let value = flatten_text(text);
                        limits.check_text_len(value.len())?;
                        total_text_bytes = total_text_bytes
                            .checked_add(value.len() as u64)
                            .ok_or_else(|| {
                                EngineError::CodecError(
                                    "total PPTX text length overflow".to_string(),
                                )
                            })?;
                        limits.check_total_text(total_text_bytes)?;
                        let (x, y, width, height) = object_bounds_to_emu(
                            object.bounds.origin.x,
                            object.bounds.origin.y,
                            object.bounds.width,
                            object.bounds.height,
                            slide.size.width,
                            slide.size.height,
                            size.0,
                            size.1,
                        )?;
                        let runs = text_to_pptx_runs(text);
                        if runs.is_empty() {
                            target.add_text_box("", x, y, width, height);
                        } else {
                            target.add_rich_text_box(&runs, x, y, width, height);
                        }
                    }
                    ObjectKind::Image(image) => {
                        let format = pptx_image_format(image)?;
                        add_embedded_image_bytes(
                            &mut total_image_bytes,
                            image.bytes.len(),
                            limits.max_output_bytes,
                            "embedded image bytes",
                        )?;
                        let (x, y, width, height) = object_bounds_to_emu(
                            object.bounds.origin.x,
                            object.bounds.origin.y,
                            object.bounds.width,
                            object.bounds.height,
                            slide.size.width,
                            slide.size.height,
                            size.0,
                            size.1,
                        )?;
                        target.add_image(
                            image.bytes.clone(),
                            format,
                            x,
                            y,
                            u64::try_from(width).map_err(|_| {
                                EngineError::CodecError("PPTX image width is negative".to_string())
                            })?,
                            u64::try_from(height).map_err(|_| {
                                EngineError::CodecError("PPTX image height is negative".to_string())
                            })?,
                        );
                    }
                    unsupported => {
                        return Err(EngineError::UnsupportedCapability(format!(
                            "PPTX {} export",
                            unsupported.name()
                        )));
                    }
                }
            }
        }

        let mut output = Cursor::new(Vec::new());
        writer.write_to(&mut output).map_err(|error| {
            EngineError::CodecError(format!("office-oxide could not write PPTX: {error}"))
        })?;
        let bytes = output.into_inner();
        limits.check_output_len(bytes.len() as u64)?;
        Ok(bytes)
    }
}

/// Inspect the OOXML central directory before Office Oxide starts inflating
/// entries. The input-byte limit protects the read buffer; these structural
/// bounds protect decompressed package and entry allocations separately.
fn preflight_pptx_package(bytes: &[u8]) -> Result<(), EngineError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| EngineError::CodecError(format!("invalid PPTX ZIP package: {error}")))?;
    if archive.len() > MAX_PPTX_ZIP_ENTRIES {
        return Err(EngineError::LimitExceeded {
            limit: MAX_PPTX_ZIP_ENTRIES as u64,
            actual: archive.len() as u64,
            context: "PPTX ZIP entries".to_string(),
        });
    }

    let mut decompressed_bytes = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            EngineError::CodecError(format!("invalid PPTX ZIP entry {index}: {error}"))
        })?;
        let entry_bytes = entry.size();
        if entry_bytes > MAX_PPTX_ZIP_ENTRY_BYTES {
            return Err(EngineError::LimitExceeded {
                limit: MAX_PPTX_ZIP_ENTRY_BYTES,
                actual: entry_bytes,
                context: format!("PPTX ZIP entry {index} decompressed bytes"),
            });
        }
        decompressed_bytes = decompressed_bytes.checked_add(entry_bytes).ok_or_else(|| {
            EngineError::LimitExceeded {
                limit: MAX_PPTX_DECOMPRESSED_PACKAGE_BYTES,
                actual: u64::MAX,
                context: "PPTX decompressed package bytes".to_string(),
            }
        })?;
        if decompressed_bytes > MAX_PPTX_DECOMPRESSED_PACKAGE_BYTES {
            return Err(EngineError::LimitExceeded {
                limit: MAX_PPTX_DECOMPRESSED_PACKAGE_BYTES,
                actual: decompressed_bytes,
                context: "PPTX decompressed package bytes".to_string(),
            });
        }
    }
    Ok(())
}

/// Projects OfficeOxide's PPTX-specific model before the generic IR has a
/// chance to flatten tables and discard non-text shapes.
fn project_pptx_visual_subset(
    document: &PptxDocument,
    limits: &Limits,
) -> Result<Presentation, EngineError> {
    let slide_count =
        u32::try_from(document.slides.len()).map_err(|_| EngineError::LimitExceeded {
            limit: u64::from(limits.max_slides),
            actual: document.slides.len() as u64,
            context: "slides".to_string(),
        })?;
    limits.check_slides(slide_count)?;

    let canvas = pptx_canvas_emu(document);
    let slide_size = slide_size_from_canvas(canvas)?;
    let mut presentation = Presentation::new(PresentationId::new(1));
    let mut total_shapes = 0_u64;
    let mut total_text_bytes = 0_u64;
    let mut total_image_bytes = 0_u64;

    for (index, source_slide) in document.slides.iter().enumerate() {
        let slide_id = SlideId::new(
            u64::try_from(index + 1)
                .map_err(|_| EngineError::CodecError("slide identifier overflow".to_string()))?,
        );
        let mut slide = Slide::new(slide_id);
        slide.size = slide_size;
        slide.notes = source_slide.notes.clone();
        slide.background = source_slide
            .background_rgb
            .map(|[red, green, blue]| Background::Solid(Color::rgb(red, green, blue)))
            .unwrap_or(Background::None);
        let mut next_object_id = 1_u64;

        for shape in &source_slide.shapes {
            project_pptx_shape(
                shape,
                slide_size,
                canvas,
                &mut slide,
                &mut next_object_id,
                limits,
                &mut total_shapes,
                &mut total_text_bytes,
                &mut total_image_bytes,
            )?;
        }
        presentation.slides.push(slide);
    }

    presentation
        .validate()
        .map_err(EngineError::ValidationError)?;
    Ok(presentation)
}

fn pptx_canvas_emu(document: &PptxDocument) -> (u64, u64) {
    let Some(size) = document.presentation.slide_size.as_ref() else {
        return (PPTX_WIDTH_EMU, PPTX_HEIGHT_EMU);
    };
    let (Ok(width), Ok(height)) = (u64::try_from(size.cx), u64::try_from(size.cy)) else {
        return (PPTX_WIDTH_EMU, PPTX_HEIGHT_EMU);
    };
    if width == 0 || height == 0 {
        (PPTX_WIDTH_EMU, PPTX_HEIGHT_EMU)
    } else {
        (width, height)
    }
}

#[allow(clippy::too_many_arguments)]
fn project_pptx_shape(
    source: &PptxShape,
    slide_size: Size,
    canvas: (u64, u64),
    slide: &mut Slide,
    next_object_id: &mut u64,
    limits: &Limits,
    total_shapes: &mut u64,
    total_text_bytes: &mut u64,
    total_image_bytes: &mut u64,
) -> Result<(), EngineError> {
    match source {
        PptxShape::AutoShape(shape) => {
            let bounds = pptx_shape_bounds(shape.position.as_ref(), slide_size, canvas);
            if let Some(text) = shape
                .text_body
                .as_ref()
                .and_then(pptx_text_body_to_text)
                .filter(|text| !flatten_text(text).trim().is_empty())
            {
                push_projected_object(
                    ObjectKind::Text(text),
                    bounds,
                    slide,
                    next_object_id,
                    limits,
                    total_shapes,
                    total_text_bytes,
                    total_image_bytes,
                )
            } else {
                push_projected_object(
                    ObjectKind::Shape(projected_auto_shape_visual(shape)),
                    bounds,
                    slide,
                    next_object_id,
                    limits,
                    total_shapes,
                    total_text_bytes,
                    total_image_bytes,
                )
            }
        }
        PptxShape::Picture(picture) => push_projected_object(
            ObjectKind::Image(SlideImage {
                bytes: picture.data.clone().unwrap_or_default(),
                format: pptx_image_format_from_extension(picture.format.as_deref()),
                alt_text: picture.alt_text.clone(),
            }),
            pptx_shape_bounds(picture.position.as_ref(), slide_size, canvas),
            slide,
            next_object_id,
            limits,
            total_shapes,
            total_text_bytes,
            total_image_bytes,
        ),
        PptxShape::Group(group) => {
            if group.children.is_empty() {
                return push_pptx_graphic_placeholder(
                    pptx_shape_bounds(group.position.as_ref(), slide_size, canvas),
                    slide,
                    next_object_id,
                    limits,
                    total_shapes,
                    total_text_bytes,
                    total_image_bytes,
                );
            }
            // OfficeOxide currently exposes children in their parsed local
            // coordinate systems but not the full group transform. Flattening
            // preserves visible child content without claiming exact group
            // transform fidelity.
            for child in &group.children {
                project_pptx_shape(
                    child,
                    slide_size,
                    canvas,
                    slide,
                    next_object_id,
                    limits,
                    total_shapes,
                    total_text_bytes,
                    total_image_bytes,
                )?;
            }
            Ok(())
        }
        PptxShape::GraphicFrame(frame) => {
            let bounds = pptx_shape_bounds(frame.position.as_ref(), slide_size, canvas);
            match &frame.content {
                PptxGraphicContent::Table(table) => project_pptx_table(
                    table,
                    bounds,
                    slide,
                    next_object_id,
                    limits,
                    total_shapes,
                    total_text_bytes,
                    total_image_bytes,
                ),
                PptxGraphicContent::Unknown => push_pptx_graphic_placeholder(
                    bounds,
                    slide,
                    next_object_id,
                    limits,
                    total_shapes,
                    total_text_bytes,
                    total_image_bytes,
                ),
            }
        }
        PptxShape::Connector(connector) => {
            let bounds = pptx_shape_bounds(connector.position.as_ref(), slide_size, canvas);
            push_projected_object(
                ObjectKind::Line(SlideLine {
                    start: bounds.origin,
                    end: Point::new(
                        bounds.origin.x + bounds.width,
                        bounds.origin.y + bounds.height,
                    ),
                    stroke: Some(Color::rgb(71, 85, 105)),
                    stroke_width: 1.5,
                }),
                bounds,
                slide,
                next_object_id,
                limits,
                total_shapes,
                total_text_bytes,
                total_image_bytes,
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn project_pptx_table(
    source: &PptxTable,
    bounds: Rect,
    slide: &mut Slide,
    next_object_id: &mut u64,
    limits: &Limits,
    total_shapes: &mut u64,
    total_text_bytes: &mut u64,
    total_image_bytes: &mut u64,
) -> Result<(), EngineError> {
    let rows = source.rows.len();
    let columns = source
        .rows
        .iter()
        .map(|row| row.cells.len())
        .max()
        .unwrap_or(0);
    let Some(mut table) = SlideTable::new(rows, columns) else {
        return push_pptx_graphic_placeholder(
            bounds,
            slide,
            next_object_id,
            limits,
            total_shapes,
            total_text_bytes,
            total_image_bytes,
        );
    };

    if source.grid_column_widths.len() == columns
        && source.grid_column_widths.iter().all(|width| *width > 0)
    {
        table.column_widths = source
            .grid_column_widths
            .iter()
            .map(|width| *width as f32)
            .collect();
    }
    if let Some(row_heights) = source
        .rows
        .iter()
        .map(|row| row.height)
        .collect::<Option<Vec<_>>>()
        && row_heights.iter().all(|height| *height > 0)
    {
        table.row_heights = row_heights
            .into_iter()
            .map(|height| height as f32)
            .collect();
    }

    for (row_index, row) in source.rows.iter().enumerate() {
        for (column_index, cell) in row.cells.iter().enumerate() {
            let index = row_index * columns + column_index;
            table.cell_layout[index] = TableCellLayout {
                row_span: cell.row_span.max(1) as usize,
                column_span: cell.grid_span.max(1) as usize,
                is_merge_continuation: cell.h_merge || cell.v_merge,
            };
            if cell.h_merge || cell.v_merge {
                continue;
            }
            let Some(text) = cell
                .text_body
                .as_ref()
                .and_then(pptx_text_body_to_text)
                .filter(|text| !flatten_text(text).trim().is_empty())
            else {
                continue;
            };
            table.cells[index] = Some(text);
        }
    }

    push_projected_object(
        ObjectKind::Table(table),
        bounds,
        slide,
        next_object_id,
        limits,
        total_shapes,
        total_text_bytes,
        total_image_bytes,
    )
}

#[allow(clippy::too_many_arguments)]
fn push_pptx_graphic_placeholder(
    bounds: Rect,
    slide: &mut Slide,
    next_object_id: &mut u64,
    limits: &Limits,
    total_shapes: &mut u64,
    total_text_bytes: &mut u64,
    total_image_bytes: &mut u64,
) -> Result<(), EngineError> {
    push_projected_object(
        ObjectKind::Shape(SlideShape {
            kind: ShapeKind::Rectangle,
            fill: Some(Color::rgb(255, 247, 237)),
            stroke: Some(Color::rgb(217, 119, 6)),
            stroke_width: 1.5,
        }),
        bounds,
        slide,
        next_object_id,
        limits,
        total_shapes,
        total_text_bytes,
        total_image_bytes,
    )?;
    push_projected_object(
        ObjectKind::Text(Text::plain("[Неподдерживаемый графический объект PPTX]")),
        bounds,
        slide,
        next_object_id,
        limits,
        total_shapes,
        total_text_bytes,
        total_image_bytes,
    )
}

fn projected_auto_shape_visual(shape: &PptxAutoShape) -> SlideShape {
    let kind = match shape.preset_geometry.as_deref() {
        Some("ellipse") => ShapeKind::Ellipse,
        Some("roundRect") => ShapeKind::RoundedRectangle,
        Some("chevron") => ShapeKind::Chevron,
        _ => ShapeKind::Rectangle,
    };
    let explicit_fill = shape.solid_fill_rgb.map(rgb);
    let has_explicit_fill = explicit_fill.is_some();
    SlideShape {
        kind,
        fill: explicit_fill.or(Some(Color::rgb(226, 232, 240))),
        stroke: (!has_explicit_fill).then_some(Color::rgb(100, 116, 139)),
        stroke_width: if has_explicit_fill { 0.0 } else { 1.0 },
    }
}

fn pptx_shape_bounds(
    position: Option<&PptxShapePosition>,
    slide_size: Size,
    canvas: (u64, u64),
) -> Rect {
    let Some(position) = position else {
        return default_text_bounds(slide_size);
    };
    let (Ok(x), Ok(y), Ok(width), Ok(height)) = (
        emu_to_slide_signed(position.x, canvas.0, slide_size.width),
        emu_to_slide_signed(position.y, canvas.1, slide_size.height),
        emu_to_slide_signed(position.cx, canvas.0, slide_size.width),
        emu_to_slide_signed(position.cy, canvas.1, slide_size.height),
    ) else {
        return default_text_bounds(slide_size);
    };
    let bounds = Rect::new(Point::new(x, y), width, height);
    if width > 0.0 && height > 0.0 && bounds.is_valid() {
        bounds
    } else {
        default_text_bounds(slide_size)
    }
}

fn pptx_text_body_to_text(body: &PptxTextBody) -> Option<Text> {
    let paragraphs = body
        .paragraphs
        .iter()
        .map(|paragraph| Paragraph {
            runs: paragraph
                .content
                .iter()
                .filter_map(pptx_text_content_to_run)
                .collect(),
            formatting: ParagraphFormatting {
                alignment: paragraph
                    .alignment
                    .as_ref()
                    .map(text_alignment)
                    .unwrap_or_default(),
                line_spacing: 1.0,
                indent: paragraph.level as f32 * LIST_INDENT_UNITS,
                bullet: None,
            },
        })
        .collect::<Vec<_>>();
    (!paragraphs.is_empty()).then_some(Text { paragraphs })
}

fn pptx_text_content_to_run(content: &PptxTextContent) -> Option<TextRun> {
    match content {
        PptxTextContent::Run(run) if !run.text.is_empty() => Some(TextRun {
            text: run.text.clone(),
            formatting: TextFormatting {
                font_family: run.font_name.clone(),
                font_size: run
                    .font_size_hundredths_pt
                    .map(|value| value as f32 / 100.0)
                    .filter(|value| value.is_finite() && *value > 0.0)
                    .unwrap_or(12.0),
                bold: run.bold.unwrap_or(false),
                italic: run.italic.unwrap_or(false),
                underline: run.underline,
                color: run.color_rgb.map(rgb).unwrap_or(Color::BLACK),
            },
        }),
        PptxTextContent::LineBreak => Some(TextRun::new("\n")),
        PptxTextContent::Field(field) if !field.text.is_empty() => {
            Some(TextRun::new(field.text.clone()))
        }
        PptxTextContent::Run(_) | PptxTextContent::Field(_) => None,
    }
}

fn pptx_image_format_from_extension(extension: Option<&str>) -> SlideImageFormat {
    match extension.map(str::to_ascii_lowercase).as_deref() {
        Some("png") => SlideImageFormat::Png,
        Some("jpg") | Some("jpeg") => SlideImageFormat::Jpeg,
        Some("bmp") => SlideImageFormat::Bmp,
        Some("svg") => SlideImageFormat::Svg,
        _ => SlideImageFormat::Unknown,
    }
}

#[allow(clippy::too_many_arguments)]
fn push_projected_object(
    kind: ObjectKind,
    bounds: Rect,
    slide: &mut Slide,
    next_object_id: &mut u64,
    limits: &Limits,
    total_shapes: &mut u64,
    total_text_bytes: &mut u64,
    total_image_bytes: &mut u64,
) -> Result<(), EngineError> {
    match &kind {
        ObjectKind::Text(text) => {
            account_projected_text(text, limits, total_text_bytes)?;
        }
        ObjectKind::Table(table) => {
            for text in table.cells.iter().flatten() {
                account_projected_text(text, limits, total_text_bytes)?;
            }
        }
        ObjectKind::Image(image) => {
            add_embedded_image_bytes(
                total_image_bytes,
                image.bytes.len(),
                limits.max_input_bytes,
                "embedded image bytes",
            )?;
        }
        ObjectKind::Shape(_) | ObjectKind::Line(_) => {}
    }

    let next_shapes = slide.objects.len().checked_add(1).ok_or_else(|| {
        EngineError::CodecError("PPTX shapes-per-slide count overflow".to_string())
    })?;
    limits.check_shapes_per_slide(u32::try_from(next_shapes).map_err(|_| {
        EngineError::LimitExceeded {
            limit: u64::from(limits.max_shapes_per_slide),
            actual: next_shapes as u64,
            context: "shapes per slide".to_string(),
        }
    })?)?;
    *total_shapes = total_shapes
        .checked_add(1)
        .ok_or_else(|| EngineError::CodecError("PPTX object count overflow".to_string()))?;
    limits.check_total_shapes(*total_shapes)?;

    let id = ObjectId::new(*next_object_id);
    *next_object_id = next_object_id
        .checked_add(1)
        .ok_or_else(|| EngineError::CodecError("PPTX object identifier overflow".to_string()))?;
    slide
        .objects
        .push(SlideObject::new(id, kind, bounds, Transform::IDENTITY));
    Ok(())
}

fn account_projected_text(
    text: &Text,
    limits: &Limits,
    total_text_bytes: &mut u64,
) -> Result<(), EngineError> {
    let value = flatten_text(text);
    if value.trim().is_empty() {
        return Ok(());
    }
    limits.check_text_len(value.len())?;
    *total_text_bytes = total_text_bytes
        .checked_add(value.len() as u64)
        .ok_or_else(|| EngineError::CodecError("total PPTX text length overflow".to_string()))?;
    limits.check_total_text(*total_text_bytes)
}

#[allow(clippy::too_many_arguments)]
fn push_positioned_elements(
    elements: &[IrElement],
    bounds: Rect,
    slide_size: Size,
    canvas: (u64, u64),
    slide: &mut Slide,
    next_object_id: &mut u64,
    limits: &Limits,
    total_shapes: &mut u64,
    total_text_bytes: &mut u64,
    total_image_bytes: &mut u64,
) -> Result<(), EngineError> {
    let mut text_elements = Vec::new();
    for element in elements {
        if let IrElement::Image(image) = element
            && let Some(image) = slide_image_from_ir(image)
        {
            push_text_elements_inner(
                &text_elements,
                bounds,
                slide_size,
                canvas,
                slide,
                next_object_id,
                limits,
                total_shapes,
                total_text_bytes,
            )?;
            text_elements.clear();
            push_image_element(
                image,
                bounds,
                slide,
                next_object_id,
                limits,
                total_shapes,
                total_image_bytes,
            )?;
            continue;
        }
        text_elements.push(element.clone());
    }
    push_text_elements_inner(
        &text_elements,
        bounds,
        slide_size,
        canvas,
        slide,
        next_object_id,
        limits,
        total_shapes,
        total_text_bytes,
    )
}

#[allow(clippy::too_many_arguments)]
fn push_image_element(
    image: SlideImage,
    bounds: Rect,
    slide: &mut Slide,
    next_object_id: &mut u64,
    limits: &Limits,
    total_shapes: &mut u64,
    total_image_bytes: &mut u64,
) -> Result<(), EngineError> {
    add_embedded_image_bytes(
        total_image_bytes,
        image.bytes.len(),
        limits.max_input_bytes,
        "embedded image bytes",
    )?;
    let next_shapes = slide.objects.len().checked_add(1).ok_or_else(|| {
        EngineError::CodecError("PPTX shapes-per-slide count overflow".to_string())
    })?;
    limits.check_shapes_per_slide(u32::try_from(next_shapes).map_err(|_| {
        EngineError::LimitExceeded {
            limit: u64::from(limits.max_shapes_per_slide),
            actual: next_shapes as u64,
            context: "shapes per slide".to_string(),
        }
    })?)?;
    *total_shapes = total_shapes
        .checked_add(1)
        .ok_or_else(|| EngineError::CodecError("PPTX object count overflow".to_string()))?;
    limits.check_total_shapes(*total_shapes)?;

    let id = ObjectId::new(*next_object_id);
    *next_object_id = next_object_id
        .checked_add(1)
        .ok_or_else(|| EngineError::CodecError("PPTX object identifier overflow".to_string()))?;
    slide.objects.push(SlideObject::new(
        id,
        ObjectKind::Image(image),
        bounds,
        Transform::IDENTITY,
    ));
    Ok(())
}

fn slide_image_from_ir(image: &IrImage) -> Option<SlideImage> {
    let format = match image.format.as_ref()? {
        IrImageFormat::Png => SlideImageFormat::Png,
        IrImageFormat::Jpeg => SlideImageFormat::Jpeg,
        IrImageFormat::Bmp => SlideImageFormat::Bmp,
        IrImageFormat::Gif | IrImageFormat::Tiff | IrImageFormat::Emf | IrImageFormat::Wmf => {
            return None;
        }
    };
    let bytes = image.data.clone()?;
    if bytes.is_empty() || !image_bytes_match_format(&bytes, format) {
        return None;
    }
    Some(SlideImage {
        bytes,
        format,
        alt_text: image.alt_text.clone(),
    })
}
#[allow(clippy::too_many_arguments)]
fn push_text_elements_inner(
    elements: &[IrElement],
    bounds: Rect,
    slide_size: Size,
    canvas: (u64, u64),
    slide: &mut Slide,
    next_object_id: &mut u64,
    limits: &Limits,
    total_shapes: &mut u64,
    total_text_bytes: &mut u64,
) -> Result<(), EngineError> {
    let Some(text) = text_from_elements(elements, slide_size, canvas) else {
        return Ok(());
    };
    let plain_text = flatten_text(&text);
    if plain_text.trim().is_empty() {
        return Ok(());
    }
    limits.check_text_len(plain_text.len())?;
    *total_text_bytes = total_text_bytes
        .checked_add(plain_text.len() as u64)
        .ok_or_else(|| EngineError::CodecError("total PPTX text length overflow".to_string()))?;
    limits.check_total_text(*total_text_bytes)?;

    let next_shapes = slide.objects.len().checked_add(1).ok_or_else(|| {
        EngineError::CodecError("PPTX shapes-per-slide count overflow".to_string())
    })?;
    limits.check_shapes_per_slide(u32::try_from(next_shapes).map_err(|_| {
        EngineError::LimitExceeded {
            limit: u64::from(limits.max_shapes_per_slide),
            actual: next_shapes as u64,
            context: "shapes per slide".to_string(),
        }
    })?)?;
    *total_shapes = total_shapes
        .checked_add(1)
        .ok_or_else(|| EngineError::CodecError("total PPTX object count overflow".to_string()))?;
    limits.check_total_shapes(*total_shapes)?;

    let id = ObjectId::new(*next_object_id);
    *next_object_id = next_object_id
        .checked_add(1)
        .ok_or_else(|| EngineError::CodecError("PPTX object identifier overflow".to_string()))?;
    slide.objects.push(SlideObject::new(
        id,
        ObjectKind::Text(text),
        bounds,
        Transform::IDENTITY,
    ));
    Ok(())
}

fn text_from_elements(
    elements: &[IrElement],
    slide_size: Size,
    canvas: (u64, u64),
) -> Option<Text> {
    let mut paragraphs = Vec::new();
    for element in elements {
        append_element_text(element, &mut paragraphs, slide_size, canvas);
    }
    (!paragraphs.is_empty()).then_some(Text { paragraphs })
}

fn append_element_text(
    element: &IrElement,
    output: &mut Vec<Paragraph>,
    slide_size: Size,
    canvas: (u64, u64),
) {
    match element {
        IrElement::Heading(heading) => {
            let default_size = heading_font_size(heading.level);
            output.push(paragraph_from_inline(
                &heading.content,
                heading.alignment.as_ref(),
                None,
                None,
                default_size,
                slide_size,
                canvas,
            ));
        }
        IrElement::Paragraph(paragraph) => output.push(paragraph_from_inline(
            &paragraph.content,
            paragraph.alignment.as_ref(),
            paragraph.indent_left_twips,
            paragraph.line_spacing.as_ref(),
            12.0,
            slide_size,
            canvas,
        )),
        IrElement::List(list) => append_list_text(list, output, slide_size, canvas, 0.0),
        IrElement::TextBox(text_box) => {
            for child in &text_box.content {
                append_element_text(child, output, slide_size, canvas);
            }
        }
        _ => {
            let fallback = fallback_element_text(element);
            if !fallback.is_empty() {
                output.push(Paragraph::plain(fallback));
            }
        }
    }
}

fn append_list_text(
    list: &IrList,
    output: &mut Vec<Paragraph>,
    slide_size: Size,
    canvas: (u64, u64),
    inherited_indent: f32,
) {
    let base_indent = inherited_indent + f32::from(list.level) * LIST_INDENT_UNITS;
    for (index, item) in list.items.iter().enumerate() {
        let mut item_text = text_from_elements(&item.content, slide_size, canvas)
            .map(|text| text.paragraphs)
            .unwrap_or_default();
        if item_text.is_empty() {
            item_text.push(Paragraph::plain(""));
        }
        for (paragraph_index, paragraph) in item_text.iter_mut().enumerate() {
            paragraph.formatting.indent += base_indent;
            if paragraph_index == 0 {
                paragraph.formatting.bullet = Some(list_marker(list, index));
            }
        }
        output.extend(item_text);
        if let Some(nested) = &item.nested {
            append_list_text(
                nested,
                output,
                slide_size,
                canvas,
                base_indent + LIST_INDENT_UNITS,
            );
        }
    }
}

fn list_marker(list: &IrList, index: usize) -> String {
    if list.ordered {
        let start = list.start_number.unwrap_or(1);
        format!("{}.", start.saturating_add(index as u32))
    } else {
        "•".to_string()
    }
}

fn paragraph_from_inline(
    content: &[IrInlineContent],
    alignment: Option<&IrParagraphAlignment>,
    indent_twips: Option<i32>,
    line_spacing: Option<&IrLineSpacing>,
    default_font_size: f32,
    slide_size: Size,
    canvas: (u64, u64),
) -> Paragraph {
    Paragraph {
        runs: inline_to_text_runs(content, default_font_size),
        formatting: ParagraphFormatting {
            alignment: alignment.map(text_alignment).unwrap_or_default(),
            line_spacing: line_spacing.map(line_spacing_multiplier).unwrap_or(1.0),
            indent: indent_twips
                .map(|value| twips_to_slide_units(value, slide_size.width, canvas.0))
                .unwrap_or(0.0),
            bullet: None,
        },
    }
}

fn inline_to_text_runs(content: &[IrInlineContent], default_font_size: f32) -> Vec<TextRun> {
    let mut runs = Vec::new();
    for item in content {
        match item {
            IrInlineContent::Text(span) if !span.text.is_empty() => runs.push(TextRun {
                text: span.text.clone(),
                formatting: text_formatting(span, default_font_size),
            }),
            IrInlineContent::LineBreak => runs.push(TextRun {
                text: "\n".to_string(),
                formatting: TextFormatting {
                    font_size: default_font_size,
                    ..TextFormatting::default()
                },
            }),
            IrInlineContent::Text(_) => {}
            _ => runs.push(TextRun::new("[Note reference]")),
        }
    }
    runs
}

fn text_formatting(span: &IrTextSpan, default_font_size: f32) -> TextFormatting {
    let font_size = span
        .font_size_half_pt
        .map(|value| value as f32 / 2.0)
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(default_font_size);
    TextFormatting {
        font_family: span.font_name.clone(),
        font_size,
        bold: span.bold,
        italic: span.italic,
        underline: !matches!(span.underline.as_ref(), Some(IrUnderlineStyle::None) | None),
        color: span.color.map(rgb).unwrap_or(Color::BLACK),
    }
}

fn text_alignment(value: &IrParagraphAlignment) -> TextAlignment {
    match value {
        IrParagraphAlignment::Left => TextAlignment::Left,
        IrParagraphAlignment::Center => TextAlignment::Center,
        IrParagraphAlignment::Right => TextAlignment::Right,
        IrParagraphAlignment::Justify | IrParagraphAlignment::Distribute => TextAlignment::Justify,
    }
}

fn line_spacing_multiplier(value: &IrLineSpacing) -> f32 {
    let raw = match value {
        IrLineSpacing::Auto(value)
        | IrLineSpacing::Multiple(value)
        | IrLineSpacing::Exact(value)
        | IrLineSpacing::AtLeast(value) => *value,
    };
    (raw as f32 / 240.0).clamp(0.1, 100.0)
}

fn heading_font_size(level: u8) -> f32 {
    match level {
        1 => 36.0,
        2 => 32.0,
        3 => 28.0,
        4 => 24.0,
        5 => 20.0,
        _ => 18.0,
    }
}

fn fallback_element_text(element: &IrElement) -> String {
    let mut document = DocumentIR::default();
    document.sections.push(Section {
        elements: vec![element.clone()],
        ..Default::default()
    });
    let text = document.plain_text();
    if !text.trim().is_empty() {
        return text;
    }
    match element {
        IrElement::Image(image) => image
            .alt_text
            .as_deref()
            .map(|alt| format!("[Image: {alt}]"))
            .unwrap_or_else(|| "[Image]".to_string()),
        IrElement::Table(_) => "[Table]".to_string(),
        IrElement::ThematicBreak => "[Horizontal rule]".to_string(),
        IrElement::PageBreak => "[Page break]".to_string(),
        IrElement::ColumnBreak => "[Column break]".to_string(),
        _ => "[Unsupported PPTX element]".to_string(),
    }
}

fn section_canvas_emu(section: &Section) -> (u64, u64) {
    let Some(page_setup) = &section.page_setup else {
        return (PPTX_WIDTH_EMU, PPTX_HEIGHT_EMU);
    };
    let width = u64::from(page_setup.width_twips).saturating_mul(EMU_PER_TWIP);
    let height = u64::from(page_setup.height_twips).saturating_mul(EMU_PER_TWIP);
    if width == 0 || height == 0 {
        (PPTX_WIDTH_EMU, PPTX_HEIGHT_EMU)
    } else {
        (width, height)
    }
}

fn slide_size_from_canvas(canvas: (u64, u64)) -> Result<Size, EngineError> {
    let width = (canvas.0 as f64 / PPTX_WIDTH_EMU as f64) * f64::from(DEFAULT_SLIDE_WIDTH);
    let height = (canvas.1 as f64 / PPTX_HEIGHT_EMU as f64) * f64::from(DEFAULT_SLIDE_HEIGHT);
    if !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
        || width > f32::MAX as f64
        || height > f32::MAX as f64
    {
        return Err(EngineError::InvalidInput(
            "PPTX slide size cannot be represented".to_string(),
        ));
    }
    Ok(Size::new(width as f32, height as f32))
}

fn default_text_bounds(size: Size) -> Rect {
    Rect::new(
        Point::new(size.width * 0.0625, size.height * (80.0 / 720.0)),
        size.width * 0.875,
        size.height * (560.0 / 720.0),
    )
}

fn text_box_bounds(
    text_box: &IrTextBox,
    slide_size: Size,
    canvas: (u64, u64),
) -> Result<Option<Rect>, EngineError> {
    let (Some(width), Some(height)) = (text_box.width_emu, text_box.height_emu) else {
        return Ok(None);
    };
    let x = emu_to_slide_signed(text_box.x_emu.unwrap_or(0), canvas.0, slide_size.width)?;
    let y = emu_to_slide_signed(text_box.y_emu.unwrap_or(0), canvas.1, slide_size.height)?;
    let width = emu_to_slide_unsigned(width, canvas.0, slide_size.width)?;
    let height = emu_to_slide_unsigned(height, canvas.1, slide_size.height)?;
    Ok(Some(Rect::new(Point::new(x, y), width, height)))
}

fn emu_to_slide_signed(value: i64, canvas: u64, extent: f32) -> Result<f32, EngineError> {
    let scaled = (value as f64 / canvas as f64) * extent as f64;
    if !scaled.is_finite() || scaled < f32::MIN as f64 || scaled > f32::MAX as f64 {
        return Err(EngineError::InvalidInput(
            "PPTX object position cannot be represented".to_string(),
        ));
    }
    Ok(scaled as f32)
}

fn emu_to_slide_unsigned(value: u64, canvas: u64, extent: f32) -> Result<f32, EngineError> {
    let scaled = (value as f64 / canvas as f64) * extent as f64;
    if !scaled.is_finite() || scaled < 0.0 || scaled > f32::MAX as f64 {
        return Err(EngineError::InvalidInput(
            "PPTX object extent cannot be represented".to_string(),
        ));
    }
    Ok(scaled as f32)
}

fn twips_to_slide_units(value: i32, slide_extent: f32, canvas_extent_emu: u64) -> f32 {
    let scaled =
        (value as f64 * EMU_PER_TWIP as f64 / canvas_extent_emu as f64) * slide_extent as f64;
    if scaled.is_finite() {
        scaled.clamp(f32::MIN as f64, f32::MAX as f64) as f32
    } else if scaled.is_sign_negative() {
        f32::MIN
    } else {
        f32::MAX
    }
}

fn pptx_image_format(image: &SlideImage) -> Result<IrImageFormat, EngineError> {
    if image.bytes.is_empty() {
        return Err(EngineError::InvalidInput(
            "PPTX image bytes must not be empty".to_string(),
        ));
    }
    if image.alt_text.is_some() {
        return Err(EngineError::UnsupportedCapability(
            "PPTX image alternative text export".to_string(),
        ));
    }
    if !image_bytes_match_format(&image.bytes, image.format) {
        return Err(EngineError::InvalidInput(
            "PPTX image bytes do not match their declared format".to_string(),
        ));
    }
    match image.format {
        SlideImageFormat::Png => Ok(IrImageFormat::Png),
        SlideImageFormat::Jpeg => Ok(IrImageFormat::Jpeg),
        SlideImageFormat::Bmp => Ok(IrImageFormat::Bmp),
        SlideImageFormat::Svg | SlideImageFormat::Unknown => Err(
            EngineError::UnsupportedCapability(format!("PPTX {:?} image export", image.format)),
        ),
    }
}

fn image_bytes_match_format(bytes: &[u8], format: SlideImageFormat) -> bool {
    match format {
        SlideImageFormat::Png => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        SlideImageFormat::Jpeg => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        SlideImageFormat::Bmp => bytes.starts_with(b"BM"),
        SlideImageFormat::Svg | SlideImageFormat::Unknown => false,
    }
}

fn add_embedded_image_bytes(
    total: &mut u64,
    bytes: usize,
    limit: u64,
    context: &'static str,
) -> Result<(), EngineError> {
    let bytes = u64::try_from(bytes)
        .map_err(|_| EngineError::CodecError("PPTX image size overflow".to_string()))?;
    let next = total
        .checked_add(bytes)
        .ok_or_else(|| EngineError::CodecError("PPTX image byte count overflow".to_string()))?;
    if next > limit {
        return Err(EngineError::LimitExceeded {
            limit,
            actual: next,
            context: context.to_string(),
        });
    }
    *total = next;
    Ok(())
}
fn text_to_pptx_runs(text: &Text) -> Vec<PptxRun> {
    let mut output = Vec::new();
    for (paragraph_index, paragraph) in text.paragraphs.iter().enumerate() {
        if paragraph_index > 0 {
            output.push(PptxRun::new("\n"));
        }
        if let Some(bullet) = &paragraph.formatting.bullet {
            output.push(PptxRun::new(format!("{bullet} ")));
        }
        output.extend(paragraph.runs.iter().map(pptx_run));
    }
    output
}

fn pptx_run(run: &TextRun) -> PptxRun {
    let mut result = PptxRun::new(run.text.clone());
    result.bold = run.formatting.bold;
    result.italic = run.formatting.italic;
    result.underline = run.formatting.underline;
    result.font_name = run.formatting.font_family.clone();
    if run.formatting.font_size.is_finite() && run.formatting.font_size > 0.0 {
        result.font_size_pt = Some(f64::from(run.formatting.font_size));
    }
    result.color = Some(format!(
        "{:02X}{:02X}{:02X}",
        run.formatting.color.r, run.formatting.color.g, run.formatting.color.b
    ));
    result
}

fn flatten_text(text: &Text) -> String {
    let mut value = String::new();
    for (paragraph_index, paragraph) in text.paragraphs.iter().enumerate() {
        if paragraph_index > 0 {
            value.push('\n');
        }
        for run in &paragraph.runs {
            value.push_str(&run.text);
        }
    }
    value
}

fn rgb(value: [u8; 3]) -> Color {
    Color::rgb(value[0], value[1], value[2])
}

fn validate_slide_size(width: f32, height: f32) -> Result<(u64, u64), EngineError> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(EngineError::InvalidInput(
            "slide size must be finite and greater than zero".to_string(),
        ));
    }
    let width_emu =
        ((width as f64 / f64::from(DEFAULT_SLIDE_WIDTH)) * PPTX_WIDTH_EMU as f64).round();
    let height_emu =
        ((height as f64 / f64::from(DEFAULT_SLIDE_HEIGHT)) * PPTX_HEIGHT_EMU as f64).round();
    if !(1.0..=u64::MAX as f64).contains(&width_emu)
        || !(1.0..=u64::MAX as f64).contains(&height_emu)
    {
        return Err(EngineError::InvalidInput(
            "slide size cannot be represented as EMU".to_string(),
        ));
    }
    Ok((width_emu as u64, height_emu as u64))
}

#[allow(clippy::too_many_arguments)]
fn object_bounds_to_emu(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    slide_width: f32,
    slide_height: f32,
    slide_width_emu: u64,
    slide_height_emu: u64,
) -> Result<(i64, i64, i64, i64), EngineError> {
    if !x.is_finite()
        || !y.is_finite()
        || !width.is_finite()
        || !height.is_finite()
        || width < 0.0
        || height < 0.0
    {
        return Err(EngineError::InvalidInput(
            "text bounds must be finite and non-negative in size".to_string(),
        ));
    }
    let x = to_emu(x, slide_width, slide_width_emu)?;
    let y = to_emu(y, slide_height, slide_height_emu)?;
    let width = to_emu(width, slide_width, slide_width_emu)?.max(1);
    let height = to_emu(height, slide_height, slide_height_emu)?.max(1);
    Ok((x, y, width, height))
}

fn to_emu(value: f32, source_extent: f32, target_extent: u64) -> Result<i64, EngineError> {
    let scaled = (value as f64 / source_extent as f64) * target_extent as f64;
    if !scaled.is_finite() || scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
        return Err(EngineError::InvalidInput(
            "object bounds cannot be represented as EMU".to_string(),
        ));
    }
    Ok(scaled.round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, // signature
            0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, // IHDR
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
            0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63,
            0xf8, 0xcf, 0xc0, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xe2, 0x21, 0xbc, 0x33, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ]
    }

    #[test]
    fn projected_auto_shape_keeps_chevron_geometry_and_explicit_fill() {
        let shape = PptxAutoShape {
            id: 1,
            name: "Chevron".to_owned(),
            alt_text: None,
            position: None,
            preset_geometry: Some("chevron".to_owned()),
            solid_fill_rgb: Some([0x00, 0x46, 0x7f]),
            text_body: None,
            placeholder: None,
        };
        let visual = projected_auto_shape_visual(&shape);
        assert_eq!(visual.kind, ShapeKind::Chevron);
        assert_eq!(visual.fill, Some(Color::rgb(0, 70, 127)));
        assert_eq!(visual.stroke, None);
        assert_eq!(visual.stroke_width, 0.0);
    }

    #[test]
    fn codec_roundtrips_embedded_png_images() {
        let image = tiny_png();
        let mut writer = PptxWriter::new();
        writer.add_slide().add_image(
            image.clone(),
            IrImageFormat::Png,
            1_219_200,
            685_800,
            3_048_000,
            1_371_600,
        );
        let mut output = Cursor::new(Vec::new());
        writer.write_to(&mut output).unwrap();

        let codec = OfficeOxideCodec;
        assert!(codec.capabilities().contains(&Capability::Images));
        let loaded = codec
            .decode(&output.into_inner(), &Limits::default())
            .unwrap();
        assert_eq!(loaded.slides[0].objects.len(), 1);
        let object = &loaded.slides[0].objects[0];
        assert!((object.bounds.origin.x - 128.0).abs() < 0.01);
        assert!((object.bounds.origin.y - 72.0).abs() < 0.01);
        assert!((object.bounds.width - 320.0).abs() < 0.01);
        assert!((object.bounds.height - 144.0).abs() < 0.01);
        let ObjectKind::Image(payload) = &object.kind else {
            panic!("expected an image object");
        };
        assert_eq!(payload.format, SlideImageFormat::Png);
        assert_eq!(payload.bytes, image);

        let saved = codec.encode(&loaded, &Limits::default()).unwrap();
        let reloaded = codec.decode(&saved, &Limits::default()).unwrap();
        let ObjectKind::Image(payload) = &reloaded.slides[0].objects[0].kind else {
            panic!("expected an image object after round trip");
        };
        assert_eq!(payload.format, SlideImageFormat::Png);
        assert_eq!(payload.bytes, image);
    }

    #[test]
    fn image_alt_text_is_not_silently_dropped_on_export() {
        let mut presentation = Presentation::new(PresentationId::new(1));
        let mut slide = Slide::new(SlideId::new(1));
        slide.objects.push(SlideObject::new(
            ObjectId::new(1),
            ObjectKind::Image(SlideImage {
                bytes: tiny_png(),
                format: SlideImageFormat::Png,
                alt_text: Some("logo".to_string()),
            }),
            Rect::new(Point::new(0.0, 0.0), 100.0, 100.0),
            Transform::IDENTITY,
        ));
        presentation.slides.push(slide);

        let codec = OfficeOxideCodec;
        assert!(matches!(
            codec.encode(&presentation, &Limits::default()),
            Err(EngineError::UnsupportedCapability(message))
                if message == "PPTX image alternative text export"
        ));
    }
    #[test]
    fn codec_writes_and_reads_a_real_pptx_package() {
        let mut presentation = Presentation::new(PresentationId::new(1));
        let mut slide = Slide::new(SlideId::new(1));
        slide.objects.push(SlideObject::new(
            ObjectId::new(1),
            ObjectKind::Text(Text::plain("RudraOffice PPTX")),
            Rect::new(Point::new(80.0, 80.0), 1120.0, 120.0),
            Transform::IDENTITY,
        ));
        presentation.slides.push(slide);

        let codec = OfficeOxideCodec;
        let bytes = codec.encode(&presentation, &Limits::default()).unwrap();
        assert!(bytes.starts_with(b"PK\x03\x04"));

        let loaded = codec.decode(&bytes, &Limits::default()).unwrap();
        assert_eq!(loaded.slides.len(), 1);
        let ObjectKind::Text(text) = &loaded.slides[0].objects[0].kind else {
            panic!("expected a text object");
        };
        assert!(flatten_text(text).contains("RudraOffice PPTX"));
    }

    #[test]
    fn unsupported_gif_image_decodes_as_visual_placeholder() {
        // A minimal 1x1 GIF. The adapter deliberately drops GIF (and other
        // non-png/jpeg/bmp) formats from the Makepad image decoder. Decoding
        // must retain the image object so the renderer can draw its standard
        // visible placeholder instead of dropping the element or panicking.
        let gif: Vec<u8> = [
            0x47, 0x49, 0x46, 0x38, 0x39, 0x61, // "GIF89a"
            0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, // 1x1, global color table
            0x00, 0x00, 0x00, 0xff, 0xff, 0xff, // color table: black, white
            0x21, 0xf9, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, // graphic control ext
            0x2c, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, // image descriptor
            0x02, 0x02, 0x44, 0x01, 0x00, // LZW min-code-size + data
            0x3b, // trailer
        ]
        .to_vec();

        let mut writer = PptxWriter::new();
        writer.add_slide().add_image(
            gif,
            IrImageFormat::Gif,
            1_219_200,
            685_800,
            3_048_000,
            1_371_600,
        );
        let mut output = Cursor::new(Vec::new());
        writer.write_to(&mut output).unwrap();

        let loaded = OfficeOxideCodec
            .decode(&output.into_inner(), &Limits::default())
            .unwrap();
        assert_eq!(loaded.slides[0].objects.len(), 1);
        let ObjectKind::Image(image) = &loaded.slides[0].objects[0].kind else {
            panic!("expected GIF to be retained as an image placeholder");
        };
        assert_eq!(image.format, SlideImageFormat::Unknown);
        assert!(!image.bytes.is_empty());
    }

    #[test]
    fn codec_preserves_textbox_bounds_and_basic_run_formatting() {
        let mut writer = PptxWriter::new();
        let target = writer.add_slide();
        let mut first = PptxRun::new("Bold red");
        first.bold = true;
        first.color = Some("102030".to_string());
        first.font_size_pt = Some(24.0);
        let mut second = PptxRun::new(" italic");
        second.italic = true;
        second.underline = true;
        target.add_rich_text_box(&[first, second], 1_219_200, 685_800, 3_048_000, 1_371_600);
        let mut output = Cursor::new(Vec::new());
        writer.write_to(&mut output).unwrap();

        let loaded = OfficeOxideCodec
            .decode(&output.into_inner(), &Limits::default())
            .unwrap();
        assert_eq!(loaded.slides[0].objects.len(), 1);
        let object = &loaded.slides[0].objects[0];
        assert!((object.bounds.origin.x - 128.0).abs() < 0.01);
        assert!((object.bounds.origin.y - 72.0).abs() < 0.01);
        assert!((object.bounds.width - 320.0).abs() < 0.01);
        assert!((object.bounds.height - 144.0).abs() < 0.01);
        let ObjectKind::Text(text) = &object.kind else {
            panic!("expected a text object");
        };
        assert_eq!(text.paragraphs.len(), 1);
        assert_eq!(text.paragraphs[0].runs.len(), 2);
        assert!(text.paragraphs[0].runs[0].formatting.bold);
        assert_eq!(
            text.paragraphs[0].runs[0].formatting.color,
            Color::rgb(16, 32, 48)
        );
        assert_eq!(text.paragraphs[0].runs[0].formatting.font_size, 24.0);
        assert!(text.paragraphs[0].runs[1].formatting.italic);
        assert!(text.paragraphs[0].runs[1].formatting.underline);
    }

    #[test]
    fn codec_roundtrips_editable_run_formatting() {
        let mut presentation = Presentation::new(PresentationId::new(1));
        let mut slide = Slide::new(SlideId::new(1));
        slide.objects.push(SlideObject::new(
            ObjectId::new(1),
            ObjectKind::Text(Text {
                paragraphs: vec![Paragraph {
                    runs: vec![TextRun {
                        text: "Styled".to_string(),
                        formatting: TextFormatting {
                            font_family: Some("Aptos".to_string()),
                            font_size: 20.0,
                            bold: true,
                            italic: true,
                            underline: true,
                            color: Color::rgb(1, 2, 3),
                        },
                    }],
                    formatting: ParagraphFormatting::default(),
                }],
            }),
            Rect::new(Point::new(128.0, 72.0), 320.0, 144.0),
            Transform::IDENTITY,
        ));
        presentation.slides.push(slide);

        let codec = OfficeOxideCodec;
        let bytes = codec.encode(&presentation, &Limits::default()).unwrap();
        let loaded = codec.decode(&bytes, &Limits::default()).unwrap();
        let ObjectKind::Text(text) = &loaded.slides[0].objects[0].kind else {
            panic!("expected a text object");
        };
        let run = &text.paragraphs[0].runs[0];
        assert_eq!(run.text, "Styled");
        assert_eq!(run.formatting.font_family.as_deref(), Some("Aptos"));
        assert_eq!(run.formatting.font_size, 20.0);
        assert!(run.formatting.bold);
        assert!(run.formatting.italic);
        assert!(run.formatting.underline);
        assert_eq!(run.formatting.color, Color::rgb(1, 2, 3));
    }

    #[test]
    fn controlled_pptx_projects_shapes_tables_and_unknown_graphics() {
        const CONTROLLED: &[u8] =
            include_bytes!("../../../../fixtures/public/slides/controlled-projection.pptx");

        let loaded = OfficeOxideCodec
            .decode(CONTROLLED, &Limits::default())
            .unwrap();
        assert_eq!(loaded.slides.len(), 3);
        assert!(
            loaded.slides[0]
                .objects
                .iter()
                .any(|object| matches!(object.kind, ObjectKind::Shape(_)))
        );

        let table = loaded.slides[1]
            .objects
            .iter()
            .find_map(|object| match &object.kind {
                ObjectKind::Table(table) => Some(table),
                _ => None,
            })
            .expect("controlled PPTX table must remain a table object");
        assert!(table.rows >= 2);
        assert!(table.columns >= 2);
        assert!(
            table
                .cells
                .iter()
                .flatten()
                .map(flatten_text)
                .any(|value| value.contains("PPTX-01"))
        );

        assert!(loaded.slides[2].objects.iter().any(|object| {
            matches!(&object.kind, ObjectKind::Text(text)
                if flatten_text(text).contains("Неподдерживаемый графический объект PPTX"))
        }));
    }

    #[test]
    fn projected_table_preserves_merged_cell_geometry() {
        use office_oxide::pptx::{TableCell as PptxTableCell, TableRow as PptxTableRow};

        let source = PptxTable {
            grid_column_widths: vec![10, 20],
            rows: vec![
                PptxTableRow {
                    height: Some(30),
                    cells: vec![
                        PptxTableCell {
                            text_body: None,
                            grid_span: 2,
                            row_span: 2,
                            h_merge: false,
                            v_merge: false,
                        },
                        PptxTableCell {
                            text_body: None,
                            grid_span: 1,
                            row_span: 1,
                            h_merge: true,
                            v_merge: false,
                        },
                    ],
                },
                PptxTableRow {
                    height: Some(40),
                    cells: vec![
                        PptxTableCell {
                            text_body: None,
                            grid_span: 1,
                            row_span: 1,
                            h_merge: false,
                            v_merge: true,
                        },
                        PptxTableCell {
                            text_body: None,
                            grid_span: 1,
                            row_span: 1,
                            h_merge: true,
                            v_merge: true,
                        },
                    ],
                },
            ],
        };
        let mut slide = Slide::new(SlideId::new(1));
        let mut next_object_id = 1;
        let mut total_shapes = 0;
        let mut total_text_bytes = 0;
        let mut total_image_bytes = 0;

        project_pptx_table(
            &source,
            Rect::new(Point::ZERO, 100.0, 100.0),
            &mut slide,
            &mut next_object_id,
            &Limits::default(),
            &mut total_shapes,
            &mut total_text_bytes,
            &mut total_image_bytes,
        )
        .expect("merged source table must project");

        let ObjectKind::Table(table) = &slide.objects[0].kind else {
            panic!("expected a projected table");
        };
        assert!(table.is_valid());
        assert_eq!(table.column_widths, vec![10.0, 20.0]);
        assert_eq!(table.row_heights, vec![30.0, 40.0]);
        assert_eq!(
            table.cell_layout[0],
            TableCellLayout {
                row_span: 2,
                column_span: 2,
                is_merge_continuation: false,
            }
        );
        assert!(table.cell_layout[1].is_merge_continuation);
        assert!(table.cell_layout[2].is_merge_continuation);
        assert!(table.cell_layout[3].is_merge_continuation);
    }
}
