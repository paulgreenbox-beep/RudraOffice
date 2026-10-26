use office_ui_runtime::{
    LogicalPoint, LogicalRect, LogicalSize, OfficeFrame, PageContentItem, PageImage,
    PageMargins as RuntimePageMargins, PageParagraph, PageStack, PageSurface, PageTable,
    PageTableCell, PageTableRow, ParagraphFlow, RulerAxis, RulerMarker, RulerMarkerVisual,
    TextAlignment, TextRun, TextRunStyle, UiColor,
};
use writer_api::core::{Alignment, CellAlignment};

#[cfg(any(test, feature = "legacy-writer-canvas"))]
use crate::writer_page_view::WriterTextLine;
use crate::writer_page_view::{
    PAGE_GAP_HALF_POINTS, PIXELS_PER_HALF_POINT_AT_100, WORKSPACE_PADDING_HALF_POINTS,
    WriterHeaderFooter, WriterImage, WriterPageFrame, WriterParagraph, WriterTable, WriterTextRun,
    WriterTextStyle, WriterVisualBlock,
};

pub const WRITER_PARAGRAPH_FIRST_LINE_MARKER_PREFIX: &str = "writer.paragraph.first-line:";
pub const WRITER_PARAGRAPH_LEFT_MARKER_PREFIX: &str = "writer.paragraph.left:";
pub const WRITER_PARAGRAPH_RIGHT_MARKER_PREFIX: &str = "writer.paragraph.right:";
pub const WRITER_PARAGRAPH_INTERACTION_PREFIX: &str = "writer.paragraph:";
pub const WRITER_HEADER_DISTANCE_MARKER_ID: &str = "writer.header.distance";
pub const WRITER_FOOTER_DISTANCE_MARKER_ID: &str = "writer.footer.distance";

pub fn writer_paragraph_interaction_id(block_id: u64) -> String {
    format!("{WRITER_PARAGRAPH_INTERACTION_PREFIX}{block_id}")
}

pub fn writer_block_id_from_paragraph_interaction_id(value: &str) -> Option<u64> {
    let block_id = value
        .strip_prefix(WRITER_PARAGRAPH_INTERACTION_PREFIX)?
        .parse()
        .ok()?;
    (block_id != 0).then_some(block_id)
}

/// Adapts the Writer projection into the one shared office runtime frame.
///
/// The adapter is intentionally semantic: it carries runs, paragraph bounds,
/// tables and images to `VirtualCanvas`. Measurement, wrapping, baseline,
/// viewport, clip, zoom and scroll remain owned by the runtime draw path.
pub fn writer_runtime_frame(source: &WriterPageFrame) -> OfficeFrame {
    let pages = source
        .pages
        .iter()
        .map(|page| {
            PageSurface::new(
                format!("writer-page-{}", page.index + 1),
                format!("Page {}", page.index + 1),
                LogicalSize::new(
                    half_points(page.width_half_points),
                    half_points(page.height_half_points),
                ),
            )
            .with_margins(RuntimePageMargins::new(
                half_points(page.margins.left_half_points),
                half_points(page.margins.top_half_points),
                half_points(page.margins.right_half_points),
                half_points(page.margins.bottom_half_points),
            ))
        })
        .collect();

    let mut frame = OfficeFrame::new(PageStack::new(pages).with_spacing(
        half_points(PAGE_GAP_HALF_POINTS),
        half_points(WORKSPACE_PADDING_HALF_POINTS),
    ))
    .with_rulers();

    for (page_index, page) in source.pages.iter().enumerate() {
        if let Some(markers) = page.paragraph_ruler_markers {
            frame.ruler_markers.extend([
                RulerMarker::new(
                    format!(
                        "{WRITER_PARAGRAPH_FIRST_LINE_MARKER_PREFIX}{}",
                        markers.block_id
                    ),
                    "Отступ первой строки",
                    page_index,
                    RulerAxis::Horizontal,
                    signed_half_points(markers.first_line_half_points),
                )
                .with_visual(RulerMarkerVisual::TriangleDown),
                RulerMarker::new(
                    format!("{WRITER_PARAGRAPH_LEFT_MARKER_PREFIX}{}", markers.block_id),
                    "Левый отступ абзаца",
                    page_index,
                    RulerAxis::Horizontal,
                    signed_half_points(markers.left_half_points),
                )
                .with_visual(RulerMarkerVisual::TriangleUp),
                RulerMarker::new(
                    format!("{WRITER_PARAGRAPH_RIGHT_MARKER_PREFIX}{}", markers.block_id),
                    "Правый отступ абзаца",
                    page_index,
                    RulerAxis::Horizontal,
                    signed_half_points(markers.right_half_points),
                )
                .with_visual(RulerMarkerVisual::TriangleUp),
            ]);
        }
        if let Some(header) = &page.header {
            frame.ruler_markers.push(
                RulerMarker::new(
                    WRITER_HEADER_DISTANCE_MARKER_ID,
                    "Положение верхнего колонтитула",
                    page_index,
                    RulerAxis::Vertical,
                    half_points(header.baseline_half_points),
                )
                .with_visual(RulerMarkerVisual::TriangleRight),
            );
            frame
                .content
                .push(PageContentItem::Paragraph(adapt_header_footer(
                    page_index, header, true,
                )));
        }
        for block in &page.blocks {
            match block {
                #[cfg(any(test, feature = "legacy-writer-canvas"))]
                WriterVisualBlock::Text(line) => {
                    frame
                        .content
                        .push(PageContentItem::Paragraph(adapt_text_line(
                            page_index, line,
                        )))
                }
                WriterVisualBlock::Paragraph(paragraph) => frame
                    .content
                    .push(PageContentItem::Paragraph(adapt_paragraph(paragraph))),
                WriterVisualBlock::Table(table) => frame
                    .content
                    .push(PageContentItem::Table(adapt_table(page_index, table))),
                WriterVisualBlock::Image(image) => frame
                    .content
                    .push(PageContentItem::Image(adapt_image(page_index, image))),
            }
        }
        if let Some(footer) = &page.footer {
            frame.ruler_markers.push(
                RulerMarker::new(
                    WRITER_FOOTER_DISTANCE_MARKER_ID,
                    "Положение нижнего колонтитула",
                    page_index,
                    RulerAxis::Vertical,
                    half_points(footer.baseline_half_points),
                )
                .with_visual(RulerMarkerVisual::TriangleLeft),
            );
            frame
                .content
                .push(PageContentItem::Paragraph(adapt_header_footer(
                    page_index, footer, false,
                )));
        }
    }

    frame
}

fn adapt_paragraph(value: &WriterParagraph) -> PageParagraph {
    let mut paragraph = PageParagraph::new(
        value.page_index,
        LogicalPoint::new(
            half_points(value.x_half_points),
            half_points(value.y_half_points),
        ),
        half_points(value.width_half_points),
        value.runs.iter().map(adapt_run).collect(),
    );
    paragraph.alignment = alignment(value.alignment);
    paragraph.indent_left = signed_half_points(value.indent_left_half_points);
    paragraph.indent_right = signed_half_points(value.indent_right_half_points);
    paragraph.first_line_indent = signed_half_points(value.first_line_indent_half_points);
    paragraph.space_before = half_points(value.space_before_half_points);
    paragraph.space_after = half_points(value.space_after_half_points);
    paragraph.line_spacing = value.line_spacing.max(0.1);
    paragraph.marker = value.marker.as_ref().map(adapt_run);
    paragraph = paragraph.with_interaction(writer_paragraph_interaction_id(value.block_id));
    paragraph
}

#[cfg(any(test, feature = "legacy-writer-canvas"))]
fn adapt_text_line(page_index: usize, line: &WriterTextLine) -> PageParagraph {
    let runs = if line.runs.is_empty() {
        vec![TextRun::new(line.text.clone(), adapt_style(&line.style))]
    } else {
        line.runs.iter().map(adapt_run).collect()
    };
    let mut paragraph = PageParagraph::new(
        page_index,
        LogicalPoint::new(
            half_points(line.x_half_points),
            half_points(line.y_half_points),
        ),
        half_points(line.width_half_points),
        runs,
    );
    paragraph.alignment = alignment(line.alignment);
    if let Some(marker) = &line.marker {
        paragraph.marker = Some(TextRun::new(marker.clone(), adapt_style(&line.style)));
    }
    paragraph
}

fn adapt_header_footer(
    page_index: usize,
    value: &WriterHeaderFooter,
    header: bool,
) -> PageParagraph {
    let font_size = half_points(22);
    let y = (half_points(value.baseline_half_points) - font_size * 0.82).max(0.0);
    let style = TextRunStyle {
        font_family: None,
        font_size,
        color: UiColor(0x475569ff),
        background: None,
        bold: false,
        italic: false,
        underline: false,
        strikethrough: false,
    };
    let mut paragraph = PageParagraph::new(
        page_index,
        LogicalPoint::new(0.0, y),
        10_000.0,
        vec![TextRun::new(value.text.clone(), style)],
    );
    paragraph.flow = ParagraphFlow::Fixed;
    if !header {
        paragraph.space_before = 0.0;
    }
    paragraph
}
fn adapt_table(page_index: usize, table: &WriterTable) -> PageTable {
    let columns = table
        .columns
        .iter()
        .map(|value| half_points(*value))
        .collect::<Vec<_>>();
    let width = columns.iter().sum::<f64>().max(1.0);
    let mut rows = Vec::new();
    for row in &table.rows {
        let mut x = half_points(table.x_half_points);
        let mut cells = Vec::new();
        for (index, cell) in row.cells.iter().enumerate() {
            let cell_width = columns.get(index).copied().unwrap_or(width);
            let rect = LogicalRect::new(
                x,
                half_points(row.y_half_points),
                cell_width,
                half_points(row.height_half_points),
            );
            let runs = if cell.runs.is_empty() {
                vec![TextRun::new(cell.text.clone(), adapt_style(&cell.style))]
            } else {
                cell.runs.iter().map(adapt_run).collect()
            };
            cells.push(PageTableCell {
                rect,
                text: cell.text.clone(),
                runs,
                alignment: cell_alignment(cell.alignment),
                background: cell.background.map(rgba_to_ui_color),
                border: cell.border.map(rgba_to_ui_color),
            });
            x += cell_width;
        }
        rows.push(PageTableRow {
            rect: LogicalRect::new(
                half_points(table.x_half_points),
                half_points(row.y_half_points),
                width,
                half_points(row.height_half_points),
            ),
            cells,
        });
    }
    let y = rows.first().map(|row| row.rect.origin.y).unwrap_or(0.0);
    let height = rows.iter().map(|row| row.rect.bottom()).fold(y, f64::max) - y;
    PageTable {
        page_index,
        rect: LogicalRect::new(half_points(table.x_half_points), y, width, height.max(1.0)),
        columns,
        rows,
    }
}

fn adapt_image(page_index: usize, image: &WriterImage) -> PageImage {
    PageImage {
        page_index,
        rect: LogicalRect::new(
            half_points(image.x_half_points),
            half_points(image.y_half_points),
            half_points(image.width_half_points),
            half_points(image.height_half_points),
        ),
        pixel_width: image.pixel_width.max(1) as usize,
        pixel_height: image.pixel_height.max(1) as usize,
        bytes: image.bytes.clone(),
    }
}

fn adapt_run(run: &WriterTextRun) -> TextRun {
    TextRun::new(run.text.clone(), adapt_style(&run.style))
}

fn adapt_style(style: &WriterTextStyle) -> TextRunStyle {
    TextRunStyle {
        font_family: style.font_family.clone(),
        font_size: half_points(u32::from(style.size_half_points)).clamp(6.0, 72.0),
        color: style
            .foreground
            .map(rgba_to_ui_color)
            .unwrap_or(UiColor(0x202124ff)),
        background: style.background.map(rgba_to_ui_color),
        bold: style.bold,
        italic: style.italic,
        underline: style.underline,
        strikethrough: style.strikethrough,
    }
}

fn alignment(value: Alignment) -> TextAlignment {
    match value {
        Alignment::Center => TextAlignment::Center,
        Alignment::Right => TextAlignment::Right,
        Alignment::Justified => TextAlignment::Justified,
        Alignment::Left => TextAlignment::Left,
    }
}

fn cell_alignment(value: CellAlignment) -> TextAlignment {
    match value {
        CellAlignment::Center | CellAlignment::Middle => TextAlignment::Center,
        CellAlignment::Right => TextAlignment::Right,
        CellAlignment::Left | CellAlignment::Top | CellAlignment::Bottom => TextAlignment::Left,
    }
}

fn rgba_to_ui_color([red, green, blue, alpha]: [u8; 4]) -> UiColor {
    UiColor(
        (u32::from(red) << 24)
            | (u32::from(green) << 16)
            | (u32::from(blue) << 8)
            | u32::from(alpha),
    )
}

fn half_points(value: u32) -> f64 {
    f64::from(value) * PIXELS_PER_HALF_POINT_AT_100
}

fn signed_half_points(value: i32) -> f64 {
    f64::from(value) * PIXELS_PER_HALF_POINT_AT_100
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer_page_view::{WriterPageFramePage, WriterTextStyle};
    use writer_api::layout::PageMargins as WriterPageMargins;

    fn source_frame() -> WriterPageFrame {
        WriterPageFrame {
            pages: vec![WriterPageFramePage {
                index: 0,
                width_half_points: 1191,
                height_half_points: 1684,
                margins: WriterPageMargins {
                    top_half_points: 57,
                    right_half_points: 57,
                    bottom_half_points: 85,
                    left_half_points: 113,
                },
                header: None,
                footer: None,
                paragraph_ruler_markers: Some(
                    crate::writer_page_view::WriterParagraphRulerMarkers {
                        block_id: 1,
                        first_line_half_points: 113,
                        left_half_points: 113,
                        right_half_points: 1_078,
                    },
                ),
                blocks: vec![WriterVisualBlock::Text(WriterTextLine {
                    text: "Pilot paragraph".to_owned(),
                    x_half_points: 113,
                    y_half_points: 90,
                    width_half_points: 1021,
                    alignment: Alignment::Left,
                    style: WriterTextStyle::default(),
                    runs: Vec::new(),
                    marker: None,
                })],
            }],
            zoom_percent: 175,
            scroll_x_points: 420.0,
            scroll_y_points: 840.0,
            truncated: false,
        }
    }

    #[test]
    fn adapter_preserves_page_geometry_and_margins_at_logical_scale() {
        let frame = writer_runtime_frame(&source_frame());
        let page = &frame.pages.pages[0];
        assert!((page.size.width - half_points(1191)).abs() < 1e-6);
        assert!((page.size.height - half_points(1684)).abs() < 1e-6);
        assert!((page.margins.left - half_points(113)).abs() < 1e-6);
        assert!(frame.horizontal_ruler.is_some());
        assert!(frame.vertical_ruler.is_some());
        assert_eq!(frame.ruler_markers.len(), 3);
        assert_eq!(frame.ruler_markers[0].axis, RulerAxis::Horizontal);
    }

    #[test]
    fn paragraph_interaction_identity_round_trips_a_writer_block_id() {
        let interaction = writer_paragraph_interaction_id(42);
        assert_eq!(interaction, "writer.paragraph:42");
        assert_eq!(
            writer_block_id_from_paragraph_interaction_id(&interaction),
            Some(42)
        );
        assert_eq!(
            writer_block_id_from_paragraph_interaction_id("writer.paragraph:0"),
            None
        );
        assert_eq!(
            writer_block_id_from_paragraph_interaction_id("other:42"),
            None
        );
    }

    #[test]
    fn adapter_uses_page_local_coordinates_for_one_domain_paragraph() {
        let frame = writer_runtime_frame(&source_frame());
        let PageContentItem::Paragraph(paragraph) = &frame.content[0] else {
            panic!("paragraph must use the shared paragraph primitive");
        };
        assert_eq!(paragraph.page_index, 0);
        assert!((paragraph.origin.x - half_points(113)).abs() < 1e-6);
        assert_eq!(paragraph.runs[0].text, "Pilot paragraph");
    }

    #[test]
    fn adapter_preserves_cyrillic_run_text_without_reencoding() {
        let mut source = source_frame();
        let WriterVisualBlock::Text(line) = &mut source.pages[0].blocks[0] else {
            panic!("test source must contain a text block");
        };
        line.runs = vec![WriterTextRun {
            text: "Жёлтый текст".to_owned(),
            style: WriterTextStyle::default(),
        }];

        let frame = writer_runtime_frame(&source);
        let PageContentItem::Paragraph(paragraph) = &frame.content[0] else {
            panic!("paragraph must use the shared paragraph primitive");
        };
        assert_eq!(paragraph.runs[0].text, "Жёлтый текст");
    }

    #[test]
    fn adapter_emits_table_and_image_content_items() {
        let mut source = source_frame();
        source.pages[0]
            .blocks
            .push(WriterVisualBlock::Table(WriterTable {
                x_half_points: 113,
                columns: vec![300, 500],
                rows: vec![crate::writer_page_view::WriterTableRow {
                    y_half_points: 160,
                    height_half_points: 80,
                    cells: vec![
                        crate::writer_page_view::WriterTableCell {
                            text: "A".to_owned(),
                            runs: Vec::new(),
                            alignment: CellAlignment::Left,
                            background: Some([240, 240, 240, 255]),
                            border: Some([0, 0, 0, 255]),
                            style: WriterTextStyle::default(),
                        },
                        crate::writer_page_view::WriterTableCell {
                            text: "B".to_owned(),
                            runs: Vec::new(),
                            alignment: CellAlignment::Right,
                            background: None,
                            border: Some([0, 0, 0, 255]),
                            style: WriterTextStyle::default(),
                        },
                    ],
                }],
            }));
        source.pages[0]
            .blocks
            .push(WriterVisualBlock::Image(WriterImage {
                x_half_points: 113,
                y_half_points: 300,
                width_half_points: 240,
                height_half_points: 160,
                pixel_width: 2,
                pixel_height: 2,
                format: writer_api::core::ImageFormat::Png,
                bytes: vec![137, 80, 78, 71],
            }));

        let frame = writer_runtime_frame(&source);
        assert!(
            frame
                .content
                .iter()
                .any(|item| matches!(item, PageContentItem::Table(_)))
        );
        assert!(
            frame
                .content
                .iter()
                .any(|item| matches!(item, PageContentItem::Image(_)))
        );
    }
    #[test]
    fn source_zoom_and_scroll_are_not_copied_into_runtime_frame() {
        let frame = writer_runtime_frame(&source_frame());
        let layout = frame.pages.layout(LogicalSize::new(1200.0, 700.0));
        assert_eq!(layout.placements.len(), 1);
        assert_eq!(
            layout.placements[0].rect.origin.y,
            half_points(WORKSPACE_PADDING_HALF_POINTS)
        );
    }

    #[test]
    fn adapter_preserves_text_color_background_and_editable_ruler_markers() {
        let mut source = source_frame();
        let WriterVisualBlock::Text(line) = &mut source.pages[0].blocks[0] else {
            panic!("test source must contain text");
        };
        line.style.foreground = Some([220, 38, 38, 255]);
        line.style.background = Some([254, 240, 138, 255]);
        source.pages[0].header = Some(WriterHeaderFooter {
            text: "Header".to_owned(),
            baseline_half_points: 48,
        });
        source.pages[0].footer = Some(WriterHeaderFooter {
            text: "Footer".to_owned(),
            baseline_half_points: 1_620,
        });

        let frame = writer_runtime_frame(&source);
        let paragraph = frame.content.iter().find_map(|item| match item {
            PageContentItem::Paragraph(paragraph)
                if paragraph
                    .runs
                    .iter()
                    .any(|run| run.text == "Pilot paragraph") =>
            {
                Some(paragraph)
            }
            _ => None,
        });
        let style = &paragraph.expect("text paragraph").runs[0].style;
        assert_eq!(style.color, UiColor(0xdc2626ff));
        assert_eq!(style.background, Some(UiColor(0xfef08aff)));
        assert_eq!(
            frame
                .ruler_markers
                .iter()
                .filter(|marker| marker.axis == RulerAxis::Horizontal)
                .count(),
            3
        );
        assert_eq!(
            frame
                .ruler_markers
                .iter()
                .filter(|marker| marker.axis == RulerAxis::Vertical)
                .count(),
            2
        );
        assert!(frame.ruler_markers.iter().all(|marker| marker.editable));
    }

    #[test]
    fn table_adapter_preserves_mixed_runs_for_real_glyph_measurement() {
        let mut source = source_frame();
        let regular = WriterTextStyle::default();
        let large = WriterTextStyle {
            size_half_points: 40,
            bold: true,
            ..WriterTextStyle::default()
        };
        source.pages[0]
            .blocks
            .push(WriterVisualBlock::Table(WriterTable {
                x_half_points: 113,
                columns: vec![500],
                rows: vec![crate::writer_page_view::WriterTableRow {
                    y_half_points: 160,
                    height_half_points: 60,
                    cells: vec![crate::writer_page_view::WriterTableCell {
                        text: "Small LARGE".to_owned(),
                        runs: vec![
                            crate::writer_page_view::WriterTextRun {
                                text: "Small ".to_owned(),
                                style: regular,
                            },
                            crate::writer_page_view::WriterTextRun {
                                text: "LARGE".to_owned(),
                                style: large,
                            },
                        ],
                        alignment: CellAlignment::Left,
                        background: None,
                        border: None,
                        style: WriterTextStyle::default(),
                    }],
                }],
            }));

        let frame = writer_runtime_frame(&source);
        let table = frame.content.iter().find_map(|item| match item {
            PageContentItem::Table(table) => Some(table),
            _ => None,
        });
        let runs = &table.unwrap().rows[0].cells[0].runs;

        assert_eq!(runs.len(), 2);
        assert!(runs[1].style.bold);
        assert!((runs[1].style.font_size - half_points(40)).abs() < 1e-6);
    }
}
