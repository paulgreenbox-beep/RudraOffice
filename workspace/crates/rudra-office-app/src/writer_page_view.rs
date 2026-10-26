#[cfg(feature = "legacy-writer-canvas")]
use std::collections::hash_map::DefaultHasher;
#[cfg(feature = "legacy-writer-canvas")]
use std::hash::{Hash, Hasher};

#[cfg(feature = "legacy-writer-canvas")]
use makepad_widgets::widget::WidgetActionData;
#[cfg(feature = "legacy-writer-canvas")]
use makepad_widgets::*;
use writer_api::DocxLayoutMetadata;
use writer_api::core::{
    Alignment, BlockKind, BlockSnapshot, CellAlignment, DocumentSnapshot, ImageFormat, InlineRun,
    LineSpacing, ListKind, Rgba8, TableSnapshot, TextStyle,
};
use writer_api::layout::{LayoutSettings, LayoutSnapshot, PageMargins};

pub const PIXELS_PER_HALF_POINT_AT_100: f64 = 96.0 / 144.0;
pub const PAGE_GAP_HALF_POINTS: u32 = 36;
pub const WORKSPACE_PADDING_HALF_POINTS: u32 = 72;
#[cfg(feature = "legacy-writer-canvas")]
const MM_PER_HALF_POINT: f64 = 25.4 / 144.0;
const MAX_PAGES: usize = 200;
const MAX_BLOCKS: usize = 400;
const MAX_ROWS: usize = 1200;
const MAX_TEXT: usize = 2048;
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_IMAGE_PIXELS: usize = 4_000_000;
#[cfg(feature = "legacy-writer-canvas")]
const MAX_TEXTURES: usize = 12;

#[cfg(feature = "legacy-writer-canvas")]
#[derive(Clone, Debug, Default)]
pub enum WriterPageCanvasAction {
    #[default]
    None,
    Wheel {
        delta_x: i64,
        delta_y: i64,
        control: bool,
    },
    KeyDown(KeyCode),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WriterPageFrame {
    pub pages: Vec<WriterPageFramePage>,
    pub zoom_percent: u16,
    pub scroll_x_points: f64,
    pub scroll_y_points: f64,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WriterPageFrameViewState {
    pub active_block_id: Option<u64>,
    pub zoom_percent: u16,
    pub scroll_x_points: f64,
    pub scroll_y_points: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WriterPageFramePage {
    pub index: usize,
    pub width_half_points: u32,
    pub height_half_points: u32,
    pub margins: PageMargins,
    pub header: Option<WriterHeaderFooter>,
    pub footer: Option<WriterHeaderFooter>,
    /// Paragraph indent markers for the selected text block on the page.
    pub paragraph_ruler_markers: Option<WriterParagraphRulerMarkers>,
    pub blocks: Vec<WriterVisualBlock>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriterParagraphRulerMarkers {
    pub block_id: u64,
    pub first_line_half_points: i32,
    pub left_half_points: i32,
    pub right_half_points: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WriterHeaderFooter {
    pub text: String,
    pub baseline_half_points: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WriterVisualBlock {
    #[cfg(any(test, feature = "legacy-writer-canvas"))]
    Text(WriterTextLine),
    Paragraph(WriterParagraph),
    Table(WriterTable),
    Image(WriterImage),
}

#[cfg(any(test, feature = "legacy-writer-canvas"))]
#[derive(Clone, Debug, PartialEq)]
pub struct WriterTextLine {
    pub text: String,
    pub x_half_points: u32,
    pub y_half_points: u32,
    pub width_half_points: u32,
    pub alignment: Alignment,
    pub style: WriterTextStyle,
    pub runs: Vec<WriterTextRun>,
    pub marker: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WriterParagraph {
    /// Stable Writer text-block identity. It is carried through the visual
    /// projection so page interaction can resolve back to the document model.
    pub block_id: u64,
    pub page_index: usize,
    pub x_half_points: u32,
    pub y_half_points: u32,
    pub width_half_points: u32,
    pub alignment: Alignment,
    pub indent_left_half_points: i32,
    pub indent_right_half_points: i32,
    pub first_line_indent_half_points: i32,
    pub space_before_half_points: u32,
    pub space_after_half_points: u32,
    pub line_spacing: f64,
    pub runs: Vec<WriterTextRun>,
    pub marker: Option<WriterTextRun>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WriterTextRun {
    pub text: String,
    pub style: WriterTextStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WriterTextStyle {
    pub font_family: Option<String>,
    pub size_half_points: u16,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub foreground: Option<[u8; 4]>,
    pub background: Option<[u8; 4]>,
}
impl Default for WriterTextStyle {
    fn default() -> Self {
        Self {
            font_family: None,
            size_half_points: 22,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            foreground: None,
            background: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WriterTable {
    pub x_half_points: u32,
    pub columns: Vec<u32>,
    pub rows: Vec<WriterTableRow>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct WriterTableRow {
    pub y_half_points: u32,
    pub height_half_points: u32,
    pub cells: Vec<WriterTableCell>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct WriterTableCell {
    pub text: String,
    pub runs: Vec<WriterTextRun>,
    pub alignment: CellAlignment,
    pub background: Option<[u8; 4]>,
    pub border: Option<[u8; 4]>,
    pub style: WriterTextStyle,
}
#[derive(Clone, Debug, PartialEq)]
pub struct WriterImage {
    pub x_half_points: u32,
    pub y_half_points: u32,
    pub width_half_points: u32,
    pub height_half_points: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub format: ImageFormat,
    pub bytes: Vec<u8>,
}
#[cfg(any(test, feature = "legacy-writer-canvas"))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WriterCanvasMetrics {
    pub workspace_width_points: f64,
    pub content_height_points: f64,
    pub max_scroll_x_points: f64,
    pub max_scroll_y_points: f64,
}
#[cfg(any(test, feature = "legacy-writer-canvas"))]
#[derive(Clone, Copy, Debug)]
struct Placement {
    index: usize,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Presentation only: all content originates in the existing document snapshot
/// and LayoutSnapshot. No DOCX parsing happens in this module.
#[cfg(test)]
pub fn build_writer_page_frame(
    document: &DocumentSnapshot,
    layout: &LayoutSnapshot,
    settings: LayoutSettings,
    docx_layout: &DocxLayoutMetadata,
    zoom_percent: u16,
    scroll_x_points: f64,
    scroll_y_points: f64,
) -> WriterPageFrame {
    build_writer_page_frame_for_active_block(
        document,
        layout,
        settings,
        docx_layout,
        WriterPageFrameViewState {
            active_block_id: None,
            zoom_percent,
            scroll_x_points,
            scroll_y_points,
        },
    )
}

pub fn build_writer_page_frame_for_active_block(
    document: &DocumentSnapshot,
    layout: &LayoutSnapshot,
    settings: LayoutSettings,
    docx_layout: &DocxLayoutMetadata,
    view_state: WriterPageFrameViewState,
) -> WriterPageFrame {
    let WriterPageFrameViewState {
        active_block_id,
        zoom_percent,
        scroll_x_points,
        scroll_y_points,
    } = view_state;
    let mut truncated = layout.pages.len() > MAX_PAGES;
    let content = settings.content_rect();
    let mut pages = Vec::new();
    for page in layout.pages.iter().take(MAX_PAGES) {
        let mut blocks = Vec::new();
        let mut paragraph_ruler_markers = None;
        if page.blocks.len() > MAX_BLOCKS {
            truncated = true;
        }
        for block_layout in page.blocks.iter().take(MAX_BLOCKS) {
            let Some(block) = document.blocks.get(block_layout.source_block_index) else {
                continue;
            };
            match block {
                BlockSnapshot::Text(text) => {
                    let is_ruler_target = active_block_id
                        .map(|block_id| text.id.get() == block_id)
                        .unwrap_or_else(|| paragraph_ruler_markers.is_none());
                    if is_ruler_target {
                        paragraph_ruler_markers = Some(ruler_markers_for_paragraph(
                            text.id.get(),
                            page.width_half_points,
                            settings.margins,
                            text.paragraph_style.indent_left_half_points(),
                            text.paragraph_style.indent_right_half_points(),
                            text.paragraph_style.first_line_indent_half_points(),
                        ));
                    }
                    let style = text_style(&text.runs, Some(&text.kind));
                    let (value, cut) = bounded(&text.text, MAX_TEXT);
                    truncated |= cut;
                    let fragment = block_layout.fragments.first();
                    let x_half_points = fragment
                        .map(|value| value.x_half_points)
                        .unwrap_or(content.left_half_points);
                    let y_half_points = block_layout.start_y_half_points;
                    let width_half_points = fragment
                        .map(|value| value.width_half_points)
                        .unwrap_or(content.width_half_points)
                        .max(1);
                    let runs = project_runs(&text.runs, 0, &value, &text.kind);
                    let marker =
                        marker_for_kind(&text.kind, block_layout.source_block_index).map(|value| {
                            WriterTextRun {
                                text: value,
                                style: style.clone(),
                            }
                        });
                    blocks.push(WriterVisualBlock::Paragraph(WriterParagraph {
                        block_id: text.id.get(),
                        page_index: page.index,
                        x_half_points,
                        y_half_points,
                        width_half_points,
                        alignment: text.paragraph_style.alignment(),
                        indent_left_half_points: text.paragraph_style.indent_left_half_points(),
                        indent_right_half_points: text.paragraph_style.indent_right_half_points(),
                        first_line_indent_half_points: text
                            .paragraph_style
                            .first_line_indent_half_points(),
                        space_before_half_points: text.paragraph_style.space_before_half_points(),
                        space_after_half_points: text.paragraph_style.space_after_half_points(),
                        line_spacing: writer_line_spacing(text.paragraph_style.line_spacing()),
                        runs,
                        marker,
                    }));
                }
                BlockSnapshot::Table(table) => {
                    let (value, cut) = project_table(
                        table,
                        block_layout,
                        content.left_half_points,
                        content.width_half_points,
                    );
                    truncated |= cut;
                    blocks.push(WriterVisualBlock::Table(value));
                }
                BlockSnapshot::Image(image) => {
                    let (value, cut) = project_image(
                        image,
                        block_layout.start_y_half_points,
                        page.height_half_points,
                        content.left_half_points,
                        content.width_half_points,
                        settings.margins.bottom_half_points,
                    );
                    truncated |= cut;
                    blocks.push(WriterVisualBlock::Image(value));
                }
            }
        }
        pages.push(WriterPageFramePage {
            index: page.index,
            width_half_points: page.width_half_points.max(1),
            height_half_points: page.height_half_points.max(1),
            margins: settings.margins,
            header: header_for(docx_layout, page.index),
            footer: footer_for(docx_layout, page.index, page.height_half_points),
            paragraph_ruler_markers,
            blocks,
        });
    }
    if pages.is_empty() {
        pages.push(WriterPageFramePage {
            index: 0,
            width_half_points: settings.page_size.width_half_points.max(1),
            height_half_points: settings.page_size.height_half_points.max(1),
            margins: settings.margins,
            header: header_for(docx_layout, 0),
            footer: footer_for(docx_layout, 0, settings.page_size.height_half_points),
            paragraph_ruler_markers: None,
            blocks: Vec::new(),
        });
    }
    WriterPageFrame {
        pages,
        zoom_percent: zoom_percent.clamp(50, 200),
        scroll_x_points: scroll_x_points.max(0.0),
        scroll_y_points: scroll_y_points.max(0.0),
        truncated,
    }
}
fn project_table(
    table: &TableSnapshot,
    layout: &writer_api::layout::BlockLayout,
    x_half_points: u32,
    content_width_half_points: u32,
) -> (WriterTable, bool) {
    let columns = fit_columns(table, content_width_half_points.max(1));
    let mut truncated = layout.table_rows.len() > MAX_ROWS;
    let mut rows = Vec::new();
    for row_layout in layout.table_rows.iter().take(MAX_ROWS) {
        let cells = table
            .rows
            .get(row_layout.row_index)
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| {
                        let (text, cut) = bounded(&cell.text, MAX_TEXT);
                        truncated |= cut;
                        WriterTableCell {
                            text: text.clone(),
                            runs: project_runs(&cell.runs, 0, &text, &BlockKind::Paragraph),
                            alignment: cell.alignment,
                            background: cell.style.background.map(rgba),
                            border: border_color(cell),
                            style: text_style(&cell.runs, None),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        rows.push(WriterTableRow {
            y_half_points: row_layout.y_half_points,
            height_half_points: row_layout.height_half_points.max(1),
            cells,
        });
    }
    if rows.is_empty() && !table.rows.is_empty() {
        // Compatibility projection for an obsolete cached layout. Current
        // pages carry row geometry from the layout engine above.
        truncated = true;
        for (index, row) in table.rows.iter().take(MAX_ROWS).enumerate() {
            rows.push(WriterTableRow {
                y_half_points: layout.start_y_half_points.saturating_add(index as u32 * 48),
                height_half_points: 48,
                cells: row
                    .cells
                    .iter()
                    .map(|cell| WriterTableCell {
                        text: bounded(&cell.text, MAX_TEXT).0,
                        runs: project_runs(
                            &cell.runs,
                            0,
                            &bounded(&cell.text, MAX_TEXT).0,
                            &BlockKind::Paragraph,
                        ),
                        alignment: cell.alignment,
                        background: cell.style.background.map(rgba),
                        border: border_color(cell),
                        style: text_style(&cell.runs, None),
                    })
                    .collect(),
            });
        }
    }
    (
        WriterTable {
            x_half_points,
            columns,
            rows,
        },
        truncated,
    )
}

fn project_image(
    image: &writer_api::core::ImageSnapshot,
    y_half_points: u32,
    page_height_half_points: u32,
    content_left_half_points: u32,
    content_width_half_points: u32,
    bottom_margin_half_points: u32,
) -> (WriterImage, bool) {
    let native_width = u64::from(image.payload.dimensions.width)
        .saturating_mul(3)
        .saturating_div(2)
        .max(1) as u32;
    let native_height = u64::from(image.payload.dimensions.height)
        .saturating_mul(3)
        .saturating_div(2)
        .max(1) as u32;
    let width = native_width.min(content_width_half_points.max(1));
    let height = ((u64::from(native_height) * u64::from(width)) / u64::from(native_width.max(1)))
        .max(1)
        .min(u64::from(u32::MAX)) as u32;
    let height = height.min(
        page_height_half_points
            .saturating_sub(y_half_points)
            .saturating_sub(bottom_margin_half_points)
            .max(1),
    );
    let x_half_points = match image.layout.alignment {
        Alignment::Center => content_left_half_points
            .saturating_add(content_width_half_points.saturating_sub(width) / 2),
        Alignment::Right => {
            content_left_half_points.saturating_add(content_width_half_points.saturating_sub(width))
        }
        Alignment::Left | Alignment::Justified => content_left_half_points,
    };
    let cut = image.payload.bytes.len() > MAX_IMAGE_BYTES
        || image
            .payload
            .dimensions
            .width
            .saturating_mul(image.payload.dimensions.height)
            > MAX_IMAGE_PIXELS as u32;
    (
        WriterImage {
            x_half_points,
            y_half_points,
            width_half_points: width,
            height_half_points: height,
            pixel_width: image.payload.dimensions.width,
            pixel_height: image.payload.dimensions.height,
            format: image.payload.format,
            bytes: if cut {
                Vec::new()
            } else {
                image.payload.bytes.clone()
            },
        },
        cut,
    )
}

fn fit_columns(table: &TableSnapshot, content: u32) -> Vec<u32> {
    let count = table
        .columns
        .len()
        .max(table.rows.first().map(|row| row.cells.len()).unwrap_or(0))
        .max(1);
    let source = if table.columns.is_empty() {
        vec![1; count]
    } else {
        table
            .columns
            .iter()
            .map(|column| column.width_half_points.max(1))
            .collect::<Vec<_>>()
    };
    let total = source
        .iter()
        .fold(0_u64, |sum, value| sum + u64::from(*value))
        .max(1);
    let mut result = source
        .into_iter()
        .map(|value| ((u64::from(value) * u64::from(content)) / total).max(1) as u32)
        .collect::<Vec<_>>();
    let used = result.iter().copied().sum::<u32>();
    if let Some(last) = result.last_mut() {
        *last = last.saturating_add(content.saturating_sub(used));
    }
    result
}

fn writer_style(value: &TextStyle, kind: Option<&BlockKind>) -> WriterTextStyle {
    let mut result = WriterTextStyle {
        font_family: value.font_family().map(ToOwned::to_owned),
        size_half_points: value.size_half_points().max(12),
        bold: value.bold(),
        italic: value.italic(),
        underline: value.underline(),
        strikethrough: value.strikethrough(),
        foreground: value.foreground().map(rgba),
        background: value.background().map(rgba),
    };
    if let Some(BlockKind::Heading(level)) = kind {
        result.size_half_points = result.size_half_points.max(match level.get() {
            1 => 36,
            2 => 30,
            3 => 26,
            _ => 24,
        });
        result.bold = true;
    }
    result
}

fn text_style(runs: &[InlineRun], kind: Option<&BlockKind>) -> WriterTextStyle {
    let fallback = TextStyle::default();
    let value = runs
        .iter()
        .find(|run| !run.text.is_empty())
        .map(|run| &run.style)
        .unwrap_or(&fallback);
    writer_style(value, kind)
}

fn project_runs(
    runs: &[InlineRun],
    offset: usize,
    fragment: &str,
    kind: &BlockKind,
) -> Vec<WriterTextRun> {
    let wanted = fragment.chars().count();
    let mut skipped = 0usize;
    let mut remaining = wanted;
    let mut result = Vec::new();
    for run in runs {
        let run_len = run.text.chars().count();
        if skipped.saturating_add(run_len) <= offset {
            skipped = skipped.saturating_add(run_len);
            continue;
        }
        let local_start = offset.saturating_sub(skipped);
        let take = remaining.min(run_len.saturating_sub(local_start));
        if take == 0 {
            skipped = skipped.saturating_add(run_len);
            continue;
        }
        let text = run
            .text
            .chars()
            .skip(local_start)
            .take(take)
            .collect::<String>();
        let style = writer_style(&run.style, Some(kind));
        result.push(WriterTextRun { text, style });
        remaining = remaining.saturating_sub(take);
        skipped = skipped.saturating_add(run_len);
        if remaining == 0 {
            break;
        }
    }
    if result.is_empty() && !fragment.is_empty() {
        result.push(WriterTextRun {
            text: fragment.to_owned(),
            style: text_style(runs, Some(kind)),
        });
    }
    result
}

fn writer_line_spacing(value: LineSpacing) -> f64 {
    match value {
        LineSpacing::Single => 1.0,
        LineSpacing::OneAndHalf => 1.5,
        LineSpacing::Double => 2.0,
        LineSpacing::Multiple(value) => (value as f64 / 100.0).max(0.1),
    }
}
fn marker_for_kind(kind: &BlockKind, block_index: usize) -> Option<String> {
    match kind {
        BlockKind::ListItem {
            kind: ListKind::Bullet,
            ..
        } => Some("\u{2022} ".to_owned()),
        BlockKind::ListItem {
            kind: ListKind::Numbered,
            ..
        } => Some(format!("{}. ", block_index.saturating_add(1))),
        BlockKind::Paragraph | BlockKind::Heading(_) | BlockKind::PageBreak => None,
    }
}
fn rgba(value: Rgba8) -> [u8; 4] {
    [value.red(), value.green(), value.blue(), value.alpha()]
}
fn border_color(cell: &writer_api::core::TableCellSnapshot) -> Option<[u8; 4]> {
    let borders = cell.style.borders;
    [borders.top, borders.right, borders.bottom, borders.left]
        .into_iter()
        .find(|border| border.width_half_points > 0)
        .map(|border| rgba(border.color))
}
fn bounded(value: &str, limit: usize) -> (String, bool) {
    let mut chars = value.chars();
    let text = chars.by_ref().take(limit).collect();
    (text, chars.next().is_some())
}
fn twips_to_half_points(value: u32) -> u32 {
    (value.saturating_add(5) / 10).max(1)
}
fn header_for(metadata: &DocxLayoutMetadata, index: usize) -> Option<WriterHeaderFooter> {
    let section = metadata.sections.first()?;
    let text = if index == 0 && !section.first_page_header_text.is_empty() {
        &section.first_page_header_text
    } else if index % 2 == 1 && !section.even_page_header_text.is_empty() {
        &section.even_page_header_text
    } else {
        &section.header_text
    };
    (!text.is_empty()).then(|| WriterHeaderFooter {
        text: bounded(text, MAX_TEXT).0,
        baseline_half_points: twips_to_half_points(section.header_distance_twips).max(22),
    })
}
fn footer_for(
    metadata: &DocxLayoutMetadata,
    index: usize,
    page_height: u32,
) -> Option<WriterHeaderFooter> {
    let section = metadata.sections.first()?;
    let text = if index == 0 && !section.first_page_footer_text.is_empty() {
        &section.first_page_footer_text
    } else if index % 2 == 1 && !section.even_page_footer_text.is_empty() {
        &section.even_page_footer_text
    } else {
        &section.footer_text
    };
    (!text.is_empty()).then(|| WriterHeaderFooter {
        text: bounded(text, MAX_TEXT).0,
        baseline_half_points: page_height
            .saturating_sub(twips_to_half_points(section.footer_distance_twips).max(22)),
    })
}

#[cfg(any(test, feature = "legacy-writer-canvas"))]
pub fn half_points_to_points(value: u32, zoom_percent: u16) -> f64 {
    f64::from(value) * PIXELS_PER_HALF_POINT_AT_100 * f64::from(zoom_percent.clamp(50, 200)) / 100.0
}
#[cfg(any(test, feature = "legacy-writer-canvas"))]
fn signed_half_points_to_points(value: i32, zoom_percent: u16) -> f64 {
    f64::from(value) * PIXELS_PER_HALF_POINT_AT_100 * f64::from(zoom_percent.clamp(50, 200)) / 100.0
}
fn ruler_markers_for_paragraph(
    block_id: u64,
    page_width_half_points: u32,
    margins: PageMargins,
    indent_left_half_points: i32,
    indent_right_half_points: i32,
    first_line_indent_half_points: i32,
) -> WriterParagraphRulerMarkers {
    let page_width = i64::from(page_width_half_points);
    let clamp_to_page = |value: i64| value.clamp(0, page_width) as i32;
    let left = i64::from(margins.left_half_points) + i64::from(indent_left_half_points);
    WriterParagraphRulerMarkers {
        block_id,
        first_line_half_points: clamp_to_page(
            left.saturating_add(i64::from(first_line_indent_half_points)),
        ),
        left_half_points: clamp_to_page(left),
        right_half_points: clamp_to_page(
            page_width
                .saturating_sub(i64::from(margins.right_half_points))
                .saturating_sub(i64::from(indent_right_half_points)),
        ),
    }
}
#[cfg(any(test, feature = "legacy-writer-canvas"))]
pub fn writer_canvas_metrics(
    frame: &WriterPageFrame,
    viewport_width: f64,
    viewport_height: f64,
) -> WriterCanvasMetrics {
    let viewport_width = viewport_width.max(1.0);
    let viewport_height = viewport_height.max(1.0);
    let padding = half_points_to_points(WORKSPACE_PADDING_HALF_POINTS, frame.zoom_percent);
    let gap = half_points_to_points(PAGE_GAP_HALF_POINTS, frame.zoom_percent);
    let widest = frame
        .pages
        .iter()
        .map(|page| half_points_to_points(page.width_half_points, frame.zoom_percent))
        .fold(1.0, f64::max);
    let pages_height = frame
        .pages
        .iter()
        .map(|page| half_points_to_points(page.height_half_points, frame.zoom_percent))
        .sum::<f64>();
    let workspace_width_points = viewport_width.max(widest + padding * 2.0);
    let content_height_points =
        (pages_height + gap * frame.pages.len().saturating_sub(1) as f64 + padding * 2.0)
            .max(viewport_height);
    WriterCanvasMetrics {
        workspace_width_points,
        content_height_points,
        max_scroll_x_points: (workspace_width_points - viewport_width).max(0.0),
        max_scroll_y_points: (content_height_points - viewport_height).max(0.0),
    }
}
#[cfg(any(test, feature = "legacy-writer-canvas"))]
fn placements(
    frame: &WriterPageFrame,
    viewport_width: f64,
    viewport_height: f64,
) -> (WriterCanvasMetrics, Vec<Placement>) {
    let metrics = writer_canvas_metrics(frame, viewport_width, viewport_height);
    let mut y = half_points_to_points(WORKSPACE_PADDING_HALF_POINTS, frame.zoom_percent);
    let gap = half_points_to_points(PAGE_GAP_HALF_POINTS, frame.zoom_percent);
    let values = frame
        .pages
        .iter()
        .map(|page| {
            let width = half_points_to_points(page.width_half_points, frame.zoom_percent);
            let height = half_points_to_points(page.height_half_points, frame.zoom_percent);
            let result = Placement {
                index: page.index,
                x: (metrics.workspace_width_points - width) * 0.5,
                y,
                width,
                height,
            };
            y += height + gap;
            result
        })
        .collect();
    (metrics, values)
}
#[cfg(feature = "legacy-writer-canvas")]
struct CachedTexture {
    key: u64,
    texture: Texture,
    width: usize,
    height: usize,
}

#[cfg(feature = "legacy-writer-canvas")]
#[derive(Script, ScriptHook, Widget)]
pub struct WriterPageCanvas {
    #[uid]
    uid: WidgetUid,
    #[source]
    source: ScriptObjectRef,
    #[walk]
    walk: Walk,
    #[layout]
    layout: Layout,
    #[redraw]
    #[live]
    draw_bg: DrawColor,
    #[live]
    draw_shadow: DrawColor,
    #[live]
    draw_page: DrawColor,
    #[live]
    draw_shape: DrawColor,
    #[live]
    draw_text: DrawText,
    #[live]
    draw_text_bold: DrawText,
    #[live]
    draw_text_italic: DrawText,
    #[live]
    draw_text_bold_italic: DrawText,
    #[live]
    draw_image: DrawImage,
    #[action_data]
    #[rust]
    action_data: WidgetActionData,
    #[rust]
    frame: Option<WriterPageFrame>,
    #[rust]
    textures: Vec<CachedTexture>,
}

#[cfg(feature = "legacy-writer-canvas")]
impl WriterPageCanvas {
    pub fn set_frame(&mut self, cx: &mut Cx, frame: Option<WriterPageFrame>) {
        self.sync_textures(cx, frame.as_ref());
        self.frame = frame;
        self.draw_bg.redraw(cx);
    }
    pub fn viewport_points(&self, cx: &Cx) -> (f64, f64) {
        let rect = self.draw_bg.area().rect(cx);
        (rect.size.x.max(1.0), rect.size.y.max(1.0))
    }
    pub fn scroll_limits(&self, cx: &Cx) -> (f64, f64) {
        let Some(frame) = self.frame.as_ref() else {
            return (0.0, 0.0);
        };
        let (width, height) = self.viewport_points(cx);
        let metrics = writer_canvas_metrics(frame, width, height);
        (metrics.max_scroll_x_points, metrics.max_scroll_y_points)
    }
    fn sync_textures(&mut self, cx: &mut Cx, frame: Option<&WriterPageFrame>) {
        let Some(frame) = frame else {
            self.textures.clear();
            return;
        };
        let mut next = Vec::new();
        for image in frame
            .pages
            .iter()
            .flat_map(|page| page.blocks.iter())
            .filter_map(|block| match block {
                WriterVisualBlock::Image(image) => Some(image),
                _ => None,
            })
            .take(MAX_TEXTURES)
        {
            if image.bytes.is_empty() {
                continue;
            }
            let key = image_key(image.format, &image.bytes);
            if let Some(cached) = self.textures.iter().find(|cached| cached.key == key) {
                next.push(CachedTexture {
                    key,
                    texture: cached.texture.clone(),
                    width: cached.width,
                    height: cached.height,
                });
                continue;
            }
            let decoded = match image.format {
                ImageFormat::Png => ImageBuffer::from_png(&image.bytes).ok(),
                ImageFormat::Jpeg => ImageBuffer::from_jpg(&image.bytes).ok(),
                ImageFormat::WebP => None,
            };
            let Some(decoded) = decoded else {
                continue;
            };
            if decoded.width.saturating_mul(decoded.height) > MAX_IMAGE_PIXELS {
                continue;
            }
            let width = decoded.width;
            let height = decoded.height;
            next.push(CachedTexture {
                key,
                texture: decoded.into_new_texture(cx),
                width,
                height,
            });
        }
        self.textures = next;
    }
    fn emit(&self, cx: &mut Cx, action: WriterPageCanvasAction) {
        cx.widget_action_with_data(&self.action_data, self.uid, action);
    }
    fn rect(&mut self, cx: &mut Cx2d, rect: Rect, color: Vec4f) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        self.draw_shape.color = color;
        self.draw_shape.draw_abs(cx, rect);
    }
    fn outline(&mut self, cx: &mut Cx2d, rect: Rect, color: Vec4f, width: f64) {
        let width = width.max(0.5).min(rect.size.x.min(rect.size.y).max(0.5));
        self.rect(
            cx,
            Rect {
                pos: rect.pos,
                size: dvec2(rect.size.x, width),
            },
            color,
        );
        self.rect(
            cx,
            Rect {
                pos: dvec2(rect.pos.x, rect.pos.y + rect.size.y - width),
                size: dvec2(rect.size.x, width),
            },
            color,
        );
        self.rect(
            cx,
            Rect {
                pos: rect.pos,
                size: dvec2(width, rect.size.y),
            },
            color,
        );
        self.rect(
            cx,
            Rect {
                pos: dvec2(rect.pos.x + rect.size.x - width, rect.pos.y),
                size: dvec2(width, rect.size.y),
            },
            color,
        );
    }
    fn visible(rect: Rect, canvas: Rect) -> bool {
        rect.pos.x + rect.size.x >= canvas.pos.x
            && rect.pos.x <= canvas.pos.x + canvas.size.x
            && rect.pos.y + rect.size.y >= canvas.pos.y
            && rect.pos.y <= canvas.pos.y + canvas.size.y
    }
    fn page_rect(canvas: Rect, placement: Placement, scroll_x: f64, scroll_y: f64) -> Rect {
        Rect {
            pos: dvec2(
                canvas.pos.x + placement.x - scroll_x,
                canvas.pos.y + placement.y - scroll_y,
            ),
            size: dvec2(placement.width, placement.height),
        }
    }
    fn text_at(
        &mut self,
        cx: &mut Cx2d,
        value: &str,
        x: f64,
        baseline: f64,
        color: Vec4f,
        size: f32,
        bold: bool,
        italic: bool,
    ) {
        if value.is_empty() {
            return;
        }
        let draw = match (bold, italic) {
            (true, true) => &mut self.draw_text_bold_italic,
            (true, false) => &mut self.draw_text_bold,
            (false, true) => &mut self.draw_text_italic,
            (false, false) => &mut self.draw_text,
        };
        draw.color = color;
        draw.text_style.font_size = size;
        draw.draw_abs(cx, dvec2(x, baseline), value);
    }
    fn draw_page(
        &mut self,
        cx: &mut Cx2d,
        rect: Rect,
        page: &WriterPageFramePage,
        frame: &WriterPageFrame,
        active: bool,
    ) {
        self.draw_shadow.color = Vec4f::from_u32(0x1f29374d);
        self.draw_shadow.draw_abs(
            cx,
            Rect {
                pos: dvec2(rect.pos.x + 5.0, rect.pos.y + 6.0),
                size: rect.size,
            },
        );
        self.draw_page.color = Vec4f::from_u32(0xffffffff);
        self.draw_page.draw_abs(cx, rect);
        self.outline(
            cx,
            rect,
            if active {
                Vec4f::from_u32(0xaeb9c7ff)
            } else {
                Vec4f::from_u32(0xd6dbe3ff)
            },
            1.0,
        );
        let left = half_points_to_points(page.margins.left_half_points, frame.zoom_percent);
        let right = half_points_to_points(page.margins.right_half_points, frame.zoom_percent);
        let top = half_points_to_points(page.margins.top_half_points, frame.zoom_percent);
        let bottom = half_points_to_points(page.margins.bottom_half_points, frame.zoom_percent);
        self.outline(
            cx,
            Rect {
                pos: dvec2(rect.pos.x + left, rect.pos.y + top),
                size: dvec2(
                    (rect.size.x - left - right).max(1.0),
                    (rect.size.y - top - bottom).max(1.0),
                ),
            },
            Vec4f::from_u32(0xf0f2f5cc),
            0.5,
        );
    }
    fn draw_content(
        &mut self,
        cx: &mut Cx2d,
        page_rect: Rect,
        page: &WriterPageFramePage,
        frame: &WriterPageFrame,
    ) {
        if let Some(header) = page.header.as_ref() {
            self.draw_header_footer(cx, page_rect, page, frame, header, true);
        }
        if let Some(footer) = page.footer.as_ref() {
            self.draw_header_footer(cx, page_rect, page, frame, footer, false);
        }
        for block in &page.blocks {
            match block {
                WriterVisualBlock::Text(line) => self.draw_text_line(cx, page_rect, frame, line),
                WriterVisualBlock::Paragraph(_) => {}
                WriterVisualBlock::Table(table) => self.draw_table(cx, page_rect, frame, table),
                WriterVisualBlock::Image(image) => {
                    self.draw_image_block(cx, page_rect, frame, image)
                }
            }
        }
    }
    fn draw_header_footer(
        &mut self,
        cx: &mut Cx2d,
        page_rect: Rect,
        page: &WriterPageFramePage,
        frame: &WriterPageFrame,
        item: &WriterHeaderFooter,
        header: bool,
    ) {
        let size = half_points_to_points(18, frame.zoom_percent).clamp(6.0, 24.0) as f32;
        let available = page
            .width_half_points
            .saturating_sub(page.margins.left_half_points)
            .saturating_sub(page.margins.right_half_points);
        let value = clip_line(
            &item.text,
            half_points_to_points(available, frame.zoom_percent),
            f64::from(size),
        );
        let estimated = value.chars().count() as f64 * f64::from(size) * 0.52;
        let x = if header {
            page_rect.pos.x
                + half_points_to_points(page.margins.left_half_points, frame.zoom_percent)
        } else {
            page_rect.pos.x
                + half_points_to_points(
                    page.width_half_points
                        .saturating_sub(page.margins.right_half_points),
                    frame.zoom_percent,
                )
                - estimated
        };
        let y =
            page_rect.pos.y + half_points_to_points(item.baseline_half_points, frame.zoom_percent);
        self.text_at(
            cx,
            &value,
            x,
            y,
            Vec4f::from_u32(0x5f6b7aff),
            size,
            false,
            false,
        );
    }
    fn draw_text_line(
        &mut self,
        cx: &mut Cx2d,
        page_rect: Rect,
        frame: &WriterPageFrame,
        line: &WriterTextLine,
    ) {
        let size = half_points_to_points(line.style.size_half_points as u32, frame.zoom_percent)
            .clamp(6.0, 72.0) as f32;
        let width = half_points_to_points(line.width_half_points, frame.zoom_percent);
        let value = clip_line(&line.text, width, f64::from(size));
        let estimated = value.chars().count() as f64 * f64::from(size) * 0.52;
        let base_x =
            page_rect.pos.x + half_points_to_points(line.x_half_points, frame.zoom_percent);
        let x = match line.alignment {
            Alignment::Center => base_x + (width - estimated).max(0.0) * 0.5,
            Alignment::Right => base_x + (width - estimated).max(0.0),
            Alignment::Left | Alignment::Justified => base_x,
        };
        let baseline = page_rect.pos.y
            + half_points_to_points(line.y_half_points, frame.zoom_percent)
            + f64::from(size) * 0.86;
        let color = line
            .style
            .foreground
            .map(color)
            .unwrap_or_else(|| Vec4f::from_u32(0x202124ff));
        self.text_at(
            cx,
            &value,
            x,
            baseline,
            color,
            size,
            line.style.bold,
            line.style.italic,
        );
        if line.style.underline && !value.is_empty() {
            self.rect(
                cx,
                Rect {
                    pos: dvec2(x, baseline + 1.5),
                    size: dvec2(estimated.min(width), 0.8),
                },
                color,
            );
        }
    }
    fn draw_table(
        &mut self,
        cx: &mut Cx2d,
        page_rect: Rect,
        frame: &WriterPageFrame,
        table: &WriterTable,
    ) {
        if table.columns.is_empty() {
            return;
        }
        let mut column_points = Vec::with_capacity(table.columns.len());
        for width in &table.columns {
            column_points.push(half_points_to_points(*width, frame.zoom_percent));
        }
        let table_x =
            page_rect.pos.x + half_points_to_points(table.x_half_points, frame.zoom_percent);
        for row in &table.rows {
            let y = page_rect.pos.y + half_points_to_points(row.y_half_points, frame.zoom_percent);
            let h = half_points_to_points(row.height_half_points, frame.zoom_percent).max(1.0);
            let mut x = table_x;
            for (column, width) in column_points.iter().copied().enumerate() {
                let rect = Rect {
                    pos: dvec2(x, y),
                    size: dvec2(width.max(1.0), h),
                };
                if let Some(cell) = row.cells.get(column) {
                    self.rect(
                        cx,
                        rect,
                        cell.background
                            .map(color)
                            .unwrap_or_else(|| Vec4f::from_u32(0xffffffff)),
                    );
                    self.outline(
                        cx,
                        rect,
                        cell.border
                            .map(color)
                            .unwrap_or_else(|| Vec4f::from_u32(0x606a75ff)),
                        0.7,
                    );
                    self.draw_cell_text(cx, rect, frame, cell);
                } else {
                    self.rect(cx, rect, Vec4f::from_u32(0xffffffff));
                    self.outline(cx, rect, Vec4f::from_u32(0x606a75ff), 0.7);
                }
                x += width;
            }
        }
    }
    fn draw_cell_text(
        &mut self,
        cx: &mut Cx2d,
        rect: Rect,
        frame: &WriterPageFrame,
        cell: &WriterTableCell,
    ) {
        let size = half_points_to_points(cell.style.size_half_points as u32, frame.zoom_percent)
            .clamp(6.0, 32.0) as f32;
        let padding = half_points_to_points(12, frame.zoom_percent).max(2.0);
        let capacity = ((rect.size.x - padding * 2.0).max(1.0) / (f64::from(size) * 0.52).max(1.0))
            .floor()
            .max(1.0) as usize;
        let line_height = (f64::from(size) * 1.16).max(7.0);
        let max_lines = ((rect.size.y - padding * 2.0) / line_height)
            .floor()
            .max(1.0) as usize;
        let lines = wrap_lines(&cell.text, capacity, max_lines);
        let total = line_height * lines.len() as f64;
        let top = match cell.alignment {
            CellAlignment::Middle => rect.pos.y + (rect.size.y - total).max(0.0) * 0.5,
            CellAlignment::Bottom => rect.pos.y + (rect.size.y - padding - total).max(0.0),
            _ => rect.pos.y + padding,
        };
        let ink = cell
            .style
            .foreground
            .map(color)
            .unwrap_or_else(|| Vec4f::from_u32(0x202124ff));
        for (index, value) in lines.iter().enumerate() {
            let estimated = value.chars().count() as f64 * f64::from(size) * 0.52;
            let x = match cell.alignment {
                CellAlignment::Center => rect.pos.x + (rect.size.x - estimated).max(0.0) * 0.5,
                CellAlignment::Right => {
                    rect.pos.x + (rect.size.x - padding - estimated).max(padding)
                }
                _ => rect.pos.x + padding,
            };
            self.text_at(
                cx,
                value,
                x,
                top + f64::from(size) * 0.86 + index as f64 * line_height,
                ink,
                size,
                cell.style.bold,
                cell.style.italic,
            );
        }
    }
    fn draw_image_block(
        &mut self,
        cx: &mut Cx2d,
        page_rect: Rect,
        frame: &WriterPageFrame,
        image: &WriterImage,
    ) {
        let rect = Rect {
            pos: dvec2(
                page_rect.pos.x + half_points_to_points(image.x_half_points, frame.zoom_percent),
                page_rect.pos.y + half_points_to_points(image.y_half_points, frame.zoom_percent),
            ),
            size: dvec2(
                half_points_to_points(image.width_half_points, frame.zoom_percent).max(1.0),
                half_points_to_points(image.height_half_points, frame.zoom_percent).max(1.0),
            ),
        };
        let key = image_key(image.format, &image.bytes);
        if let Some(texture) = self.textures.iter().find(|texture| texture.key == key) {
            self.draw_image.draw_vars.set_texture(0, &texture.texture);
            self.draw_image.image_dim_w = texture.width as f32;
            self.draw_image.image_dim_h = texture.height as f32;
            self.draw_image.draw_abs(cx, rect);
        } else {
            self.rect(cx, rect, Vec4f::from_u32(0xe9edf2ff));
            self.outline(cx, rect, Vec4f::from_u32(0x9aa7b6ff), 1.0);
            self.text_at(
                cx,
                "Image",
                rect.pos.x + 8.0,
                rect.pos.y + 18.0,
                Vec4f::from_u32(0x536273ff),
                12.0,
                false,
                false,
            );
        }
    }
    fn draw_rulers(
        &mut self,
        cx: &mut Cx2d,
        canvas: Rect,
        page_rect: Rect,
        page: &WriterPageFramePage,
        frame: &WriterPageFrame,
    ) {
        let horizontal = Rect {
            pos: dvec2(page_rect.pos.x, canvas.pos.y),
            size: dvec2(page_rect.size.x, 22.0),
        };
        self.rect(cx, horizontal, Vec4f::from_u32(0xf6f8fbf2));
        self.outline(cx, horizontal, Vec4f::from_u32(0xc8d0dbff), 0.7);
        let left = half_points_to_points(page.margins.left_half_points, frame.zoom_percent);
        let right = half_points_to_points(page.margins.right_half_points, frame.zoom_percent);
        self.rect(
            cx,
            Rect {
                pos: horizontal.pos,
                size: dvec2(left, horizontal.size.y),
            },
            Vec4f::from_u32(0xe4eaf2cc),
        );
        self.rect(
            cx,
            Rect {
                pos: dvec2(
                    horizontal.pos.x + horizontal.size.x - right,
                    horizontal.pos.y,
                ),
                size: dvec2(right, horizontal.size.y),
            },
            Vec4f::from_u32(0xe4eaf2cc),
        );
        if let Some(markers) = page.paragraph_ruler_markers {
            let marker_x = |value: i32| {
                (page_rect.pos.x + signed_half_points_to_points(value, frame.zoom_percent)).clamp(
                    horizontal.pos.x + 1.0,
                    horizontal.pos.x + horizontal.size.x - 1.0,
                )
            };
            let first_line = marker_x(markers.first_line_half_points);
            let left_indent = marker_x(markers.left_half_points);
            let right_indent = marker_x(markers.right_half_points);
            let marker = Vec4f::from_u32(0x3273caff);
            self.rect(
                cx,
                Rect {
                    pos: dvec2(first_line - 2.0, horizontal.pos.y + 1.0),
                    size: dvec2(4.0, 4.0),
                },
                marker,
            );
            for x in [left_indent, right_indent] {
                self.rect(
                    cx,
                    Rect {
                        pos: dvec2(x - 1.5, horizontal.pos.y + horizontal.size.y - 6.0),
                        size: dvec2(3.0, 5.0),
                    },
                    marker,
                );
            }
        }
        let page_mm = f64::from(page.width_half_points) * MM_PER_HALF_POINT;
        for mm in (0..=page_mm.floor() as u32).step_by(5) {
            let x = page_rect.pos.x
                + half_points_to_points(
                    (f64::from(mm) / MM_PER_HALF_POINT).round() as u32,
                    frame.zoom_percent,
                );
            let major = mm % 10 == 0;
            let height = if major { 10.0 } else { 6.0 };
            self.rect(
                cx,
                Rect {
                    pos: dvec2(x, horizontal.pos.y + horizontal.size.y - height),
                    size: dvec2(0.8, height),
                },
                Vec4f::from_u32(0x657386ff),
            );
            if major && mm > 0 {
                self.text_at(
                    cx,
                    &mm.to_string(),
                    x + 2.0,
                    horizontal.pos.y + 10.5,
                    Vec4f::from_u32(0x526274ff),
                    8.0,
                    false,
                    false,
                );
            }
        }
        let x = (page_rect.pos.x - 24.0).max(canvas.pos.x);
        let vertical = Rect {
            pos: dvec2(x, page_rect.pos.y),
            size: dvec2(22.0, page_rect.size.y),
        };
        self.rect(cx, vertical, Vec4f::from_u32(0xf6f8fbf2));
        self.outline(cx, vertical, Vec4f::from_u32(0xc8d0dbff), 0.7);
        let top = half_points_to_points(page.margins.top_half_points, frame.zoom_percent);
        let bottom = half_points_to_points(page.margins.bottom_half_points, frame.zoom_percent);
        self.rect(
            cx,
            Rect {
                pos: vertical.pos,
                size: dvec2(vertical.size.x, top),
            },
            Vec4f::from_u32(0xe4eaf2cc),
        );
        self.rect(
            cx,
            Rect {
                pos: dvec2(vertical.pos.x, vertical.pos.y + vertical.size.y - bottom),
                size: dvec2(vertical.size.x, bottom),
            },
            Vec4f::from_u32(0xe4eaf2cc),
        );
        let page_mm = f64::from(page.height_half_points) * MM_PER_HALF_POINT;
        for mm in (0..=page_mm.floor() as u32).step_by(5) {
            let y = page_rect.pos.y
                + half_points_to_points(
                    (f64::from(mm) / MM_PER_HALF_POINT).round() as u32,
                    frame.zoom_percent,
                );
            let major = mm % 10 == 0;
            let width = if major { 9.0 } else { 5.0 };
            self.rect(
                cx,
                Rect {
                    pos: dvec2(vertical.pos.x + vertical.size.x - width, y),
                    size: dvec2(width, 0.8),
                },
                Vec4f::from_u32(0x657386ff),
            );
            if major && mm > 0 && mm % 20 == 0 {
                self.text_at(
                    cx,
                    &mm.to_string(),
                    vertical.pos.x + 1.0,
                    y + 8.0,
                    Vec4f::from_u32(0x526274ff),
                    8.0,
                    false,
                    false,
                );
            }
        }
    }
    fn draw_scrollbars(
        &mut self,
        cx: &mut Cx2d,
        canvas: Rect,
        metrics: WriterCanvasMetrics,
        scroll_x: f64,
        scroll_y: f64,
    ) {
        if metrics.max_scroll_y_points > 0.5 {
            let height = (canvas.size.y * canvas.size.y / metrics.content_height_points)
                .clamp(30.0, canvas.size.y);
            let y = canvas.pos.y
                + (canvas.size.y - height)
                    * (scroll_y / metrics.max_scroll_y_points).clamp(0.0, 1.0);
            self.rect(
                cx,
                Rect {
                    pos: dvec2(canvas.pos.x + canvas.size.x - 5.0, y),
                    size: dvec2(3.0, height),
                },
                Vec4f::from_u32(0x718096a8),
            );
        }
        if metrics.max_scroll_x_points > 0.5 {
            let width = (canvas.size.x * canvas.size.x / metrics.workspace_width_points)
                .clamp(30.0, canvas.size.x);
            let x = canvas.pos.x
                + (canvas.size.x - width)
                    * (scroll_x / metrics.max_scroll_x_points).clamp(0.0, 1.0);
            self.rect(
                cx,
                Rect {
                    pos: dvec2(x, canvas.pos.y + canvas.size.y - 5.0),
                    size: dvec2(width, 3.0),
                },
                Vec4f::from_u32(0x718096a8),
            );
        }
    }
}

#[cfg(feature = "legacy-writer-canvas")]
impl Widget for WriterPageCanvas {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, _scope: &mut Scope) {
        let area = self.draw_bg.area();
        match event.hits_with_capture_overload(cx, area, true) {
            Hit::FingerDown(fe) if fe.is_primary_hit() => cx.set_key_focus(area),
            Hit::FingerScroll(fe) => self.emit(
                cx,
                WriterPageCanvasAction::Wheel {
                    delta_x: fe.scroll.x.round() as i64,
                    delta_y: fe.scroll.y.round() as i64,
                    control: fe.modifiers.control,
                },
            ),
            Hit::KeyDown(ke) => self.emit(cx, WriterPageCanvasAction::KeyDown(ke.key_code)),
            _ => {}
        }
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        let canvas = cx.walk_turtle(walk);
        self.draw_bg.color = Vec4f::from_u32(0xe8edf3ff);
        self.draw_bg.draw_abs(cx, canvas);
        let Some(frame) = self.frame.clone() else {
            let padding = half_points_to_points(WORKSPACE_PADDING_HALF_POINTS, 100);
            let static_a4 = Rect {
                pos: dvec2(
                    canvas.pos.x
                        + (canvas
                            .size
                            .x
                            .max(half_points_to_points(1191, 100) + padding * 2.0)
                            - half_points_to_points(1191, 100))
                            * 0.5,
                    canvas.pos.y + padding,
                ),
                size: dvec2(
                    half_points_to_points(1191, 100),
                    half_points_to_points(1684, 100),
                ),
            };
            let clip_rect = self.draw_bg.area().clipped_rect(cx);
            self.draw_page.color = Vec4f::from_u32(0xffffffff);
            self.draw_page.draw_abs(cx, static_a4);
            self.outline(cx, canvas, Vec4f::from_u32(0xd946efff), 1.5);
            self.outline(cx, clip_rect, Vec4f::from_u32(0x10b981ff), 1.0);
            self.outline(cx, static_a4, Vec4f::from_u32(0x10b981ff), 1.5);
            self.text_at(
                cx,
                &format!(
                    "draw_called=true frame=false viewport x={:.1} y={:.1} w={:.1} h={:.1}",
                    canvas.pos.x, canvas.pos.y, canvas.size.x, canvas.size.y
                ),
                canvas.pos.x + 8.0,
                canvas.pos.y + 13.0,
                Vec4f::from_u32(0x111827ff),
                10.0,
                true,
                false,
            );
            self.text_at(
                cx,
                &format!(
                    "clip x={:.1} y={:.1} w={:.1} h={:.1}; page_count=0; mode=PrintLayout",
                    clip_rect.pos.x, clip_rect.pos.y, clip_rect.size.x, clip_rect.size.y
                ),
                canvas.pos.x + 8.0,
                canvas.pos.y + 25.0,
                Vec4f::from_u32(0x111827ff),
                10.0,
                false,
                false,
            );
            return DrawStep::done();
        };
        // Temporary blocker diagnostic. It intentionally mirrors the A4 placement
        // transform without changing Writer's layout model or page algorithm.
        let show_debug_overlay = frame.zoom_percent == 100;
        if show_debug_overlay {
            let padding = half_points_to_points(WORKSPACE_PADDING_HALF_POINTS, 100);
            let static_a4 = Rect {
                pos: dvec2(
                    canvas.pos.x
                        + (canvas
                            .size
                            .x
                            .max(half_points_to_points(1191, 100) + padding * 2.0)
                            - half_points_to_points(1191, 100))
                            * 0.5,
                    canvas.pos.y + padding,
                ),
                size: dvec2(
                    half_points_to_points(1191, 100),
                    half_points_to_points(1684, 100),
                ),
            };
            self.draw_page.color = Vec4f::from_u32(0xffffffff);
            self.draw_page.draw_abs(cx, static_a4);
            self.outline(cx, static_a4, Vec4f::from_u32(0x10b981ff), 1.5);
        }
        let (metrics, placements) = placements(&frame, canvas.size.x, canvas.size.y);
        let scroll_x = frame
            .scroll_x_points
            .clamp(0.0, metrics.max_scroll_x_points);
        let scroll_y = frame
            .scroll_y_points
            .clamp(0.0, metrics.max_scroll_y_points);
        let active = placements
            .iter()
            .find(|placement| {
                Self::visible(
                    Self::page_rect(canvas, **placement, scroll_x, scroll_y),
                    canvas,
                )
            })
            .map(|placement| placement.index)
            .or_else(|| placements.first().map(|placement| placement.index));
        let mut active_page = None;
        for (page, placement) in frame.pages.iter().zip(placements.iter()) {
            let rect = Self::page_rect(canvas, *placement, scroll_x, scroll_y);
            if !Self::visible(rect, canvas) {
                continue;
            }
            let is_active = Some(page.index) == active;
            self.draw_page(cx, rect, page, &frame, is_active);
            self.draw_content(cx, rect, page, &frame);
            if is_active {
                active_page = Some((rect, page));
            }
        }
        if let Some((rect, page)) = active_page.as_ref() {
            self.draw_rulers(cx, canvas, *rect, page, &frame);
        }
        self.draw_scrollbars(cx, canvas, metrics, scroll_x, scroll_y);
        if show_debug_overlay {
            let clip_rect = self.draw_bg.area().clipped_rect(cx);
            let page_one_rect = placements
                .first()
                .map(|placement| Self::page_rect(canvas, *placement, scroll_x, scroll_y));
            let diagnostic = Vec4f::from_u32(0x111827ff);
            let viewport_color = Vec4f::from_u32(0xd946efff);
            let page_color = Vec4f::from_u32(0x38bdf8ff);
            let content_color = Vec4f::from_u32(0xfacc15ff);
            let ruler_color = Vec4f::from_u32(0xfb923cff);
            let text_color = Vec4f::from_u32(0xa855f7ff);
            let clip_color = Vec4f::from_u32(0x10b981ff);
            let mut label_y = canvas.pos.y + 13.0;
            self.outline(cx, canvas, viewport_color, 1.5);
            self.text_at(
                cx,
                &format!(
                    "viewport x={:.1} y={:.1} w={:.1} h={:.1}",
                    canvas.pos.x, canvas.pos.y, canvas.size.x, canvas.size.y
                ),
                canvas.pos.x + 8.0,
                label_y,
                diagnostic,
                10.0,
                true,
                false,
            );
            label_y += 12.0;
            self.outline(cx, clip_rect, clip_color, 1.0);
            self.text_at(
                cx,
                &format!(
                    "clip x={:.1} y={:.1} w={:.1} h={:.1}",
                    clip_rect.pos.x, clip_rect.pos.y, clip_rect.size.x, clip_rect.size.y
                ),
                canvas.pos.x + 8.0,
                label_y,
                diagnostic,
                10.0,
                false,
                false,
            );
            label_y += 12.0;
            self.text_at(
                cx,
                &format!(
                    "draw_called=true mode=PrintLayout pages={} blocks={} zoom={} scroll=({:.1},{:.1})",
                    frame.pages.len(),
                    frame.pages.iter().map(|page| page.blocks.len()).sum::<usize>(),
                    frame.zoom_percent,
                    scroll_x,
                    scroll_y
                ),
                canvas.pos.x + 8.0,
                label_y,
                diagnostic,
                10.0,
                false,
                false,
            );
            label_y += 12.0;
            self.text_at(
                cx,
                "static_a4_control=true (green frame)",
                canvas.pos.x + 8.0,
                label_y,
                diagnostic,
                10.0,
                false,
                false,
            );
            if let Some(page_rect) = page_one_rect {
                self.outline(cx, page_rect, page_color, 1.5);
                self.text_at(
                    cx,
                    &format!(
                        "page_1 x={:.1} y={:.1} w={:.1} h={:.1}",
                        page_rect.pos.x, page_rect.pos.y, page_rect.size.x, page_rect.size.y
                    ),
                    page_rect.pos.x + 6.0,
                    page_rect.pos.y + 15.0,
                    page_color,
                    10.0,
                    true,
                    false,
                );
            }
            if let Some((page_rect, page)) = active_page.as_ref() {
                let left = half_points_to_points(page.margins.left_half_points, frame.zoom_percent);
                let right =
                    half_points_to_points(page.margins.right_half_points, frame.zoom_percent);
                let top = half_points_to_points(page.margins.top_half_points, frame.zoom_percent);
                let bottom =
                    half_points_to_points(page.margins.bottom_half_points, frame.zoom_percent);
                let content_rect = Rect {
                    pos: dvec2(page_rect.pos.x + left, page_rect.pos.y + top),
                    size: dvec2(
                        (page_rect.size.x - left - right).max(1.0),
                        (page_rect.size.y - top - bottom).max(1.0),
                    ),
                };
                let horizontal_ruler = Rect {
                    pos: dvec2(page_rect.pos.x, canvas.pos.y),
                    size: dvec2(page_rect.size.x, 22.0),
                };
                let vertical_ruler = Rect {
                    pos: dvec2((page_rect.pos.x - 24.0).max(canvas.pos.x), page_rect.pos.y),
                    size: dvec2(22.0, page_rect.size.y),
                };
                self.outline(cx, content_rect, content_color, 1.25);
                self.text_at(
                    cx,
                    &format!(
                        "content x={:.1} y={:.1} w={:.1} h={:.1}",
                        content_rect.pos.x,
                        content_rect.pos.y,
                        content_rect.size.x,
                        content_rect.size.y
                    ),
                    content_rect.pos.x + 5.0,
                    content_rect.pos.y + 15.0,
                    content_color,
                    10.0,
                    true,
                    false,
                );
                self.outline(cx, horizontal_ruler, ruler_color, 1.25);
                self.outline(cx, vertical_ruler, ruler_color, 1.25);
                self.text_at(
                    cx,
                    &format!(
                        "ruler_h x={:.1} y={:.1} w={:.1} h={:.1}",
                        horizontal_ruler.pos.x,
                        horizontal_ruler.pos.y,
                        horizontal_ruler.size.x,
                        horizontal_ruler.size.y
                    ),
                    horizontal_ruler.pos.x + 5.0,
                    horizontal_ruler.pos.y + 12.0,
                    ruler_color,
                    9.0,
                    true,
                    false,
                );
                self.text_at(
                    cx,
                    &format!(
                        "ruler_v x={:.1} y={:.1} w={:.1} h={:.1}",
                        vertical_ruler.pos.x,
                        vertical_ruler.pos.y,
                        vertical_ruler.size.x,
                        vertical_ruler.size.y
                    ),
                    vertical_ruler.pos.x + 2.0,
                    vertical_ruler.pos.y + 28.0,
                    ruler_color,
                    9.0,
                    false,
                    false,
                );
                if let Some(line) = page.blocks.iter().find_map(|block| match block {
                    WriterVisualBlock::Text(line) => Some(line),
                    _ => None,
                }) {
                    let size = half_points_to_points(
                        line.style.size_half_points as u32,
                        frame.zoom_percent,
                    )
                    .clamp(6.0, 72.0) as f32;
                    let width = half_points_to_points(line.width_half_points, frame.zoom_percent);
                    let value = clip_line(&line.text, width, f64::from(size));
                    let estimated = value.chars().count() as f64 * f64::from(size) * 0.52;
                    let base_x = page_rect.pos.x
                        + half_points_to_points(line.x_half_points, frame.zoom_percent);
                    let x = match line.alignment {
                        Alignment::Center => base_x + (width - estimated).max(0.0) * 0.5,
                        Alignment::Right => base_x + (width - estimated).max(0.0),
                        Alignment::Left | Alignment::Justified => base_x,
                    };
                    let baseline = page_rect.pos.y
                        + half_points_to_points(line.y_half_points, frame.zoom_percent)
                        + f64::from(size) * 0.86;
                    self.rect(
                        cx,
                        Rect {
                            pos: dvec2(x - 3.0, baseline - 3.0),
                            size: dvec2(6.0, 6.0),
                        },
                        text_color,
                    );
                    self.text_at(
                        cx,
                        &format!("text_origin x={:.1} y={:.1}", x, baseline),
                        x + 5.0,
                        baseline - 4.0,
                        text_color,
                        10.0,
                        true,
                        false,
                    );
                } else {
                    self.text_at(
                        cx,
                        "text_origin=none",
                        content_rect.pos.x + 5.0,
                        content_rect.pos.y + 29.0,
                        text_color,
                        10.0,
                        false,
                        false,
                    );
                }
            }
        }
        if frame.truncated {
            self.text_at(
                cx,
                "Preview truncated for safety",
                canvas.pos.x + 12.0,
                canvas.pos.y + 18.0,
                Vec4f::from_u32(0x9c2f2fff),
                12.0,
                true,
                false,
            );
        }
        DrawStep::done()
    }
}

#[cfg(feature = "legacy-writer-canvas")]
fn clip_line(value: &str, width: f64, size: f64) -> String {
    let capacity = (width / (size * 0.52).max(1.0)).floor().max(1.0) as usize;
    let (mut result, cut) = bounded(value, capacity);
    if cut && capacity > 1 {
        result = result.chars().take(capacity - 1).collect();
        result.push('…');
    }
    result
}
#[cfg(feature = "legacy-writer-canvas")]
fn wrap_lines(value: &str, capacity: usize, max_lines: usize) -> Vec<String> {
    let mut result = Vec::new();
    let capacity = capacity.max(1);
    for raw in value.split('\n') {
        let mut line = String::new();
        for word in raw.split_inclusive(char::is_whitespace) {
            if !line.is_empty() && line.chars().count() + word.chars().count() > capacity {
                result.push(line.trim_end().to_owned());
                line.clear();
                if result.len() >= max_lines {
                    return ellipsize(result, max_lines);
                }
            }
            if line.is_empty() && word.chars().count() > capacity {
                let mut chars = word.chars();
                while chars.clone().next().is_some() {
                    result.push(
                        chars
                            .by_ref()
                            .take(capacity)
                            .collect::<String>()
                            .trim_end()
                            .to_owned(),
                    );
                    if result.len() >= max_lines {
                        return ellipsize(result, max_lines);
                    }
                }
            } else {
                line.push_str(word);
            }
        }
        if !line.is_empty() || raw.is_empty() {
            result.push(line.trim_end().to_owned());
        }
        if result.len() >= max_lines {
            return ellipsize(result, max_lines);
        }
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
}
#[cfg(feature = "legacy-writer-canvas")]
fn ellipsize(mut lines: Vec<String>, max_lines: usize) -> Vec<String> {
    lines.truncate(max_lines.max(1));
    if let Some(last) = lines.last_mut() {
        if last.chars().count() > 1 {
            last.pop();
        }
        if !last.ends_with('…') {
            last.push('…');
        }
    }
    lines
}
#[cfg(feature = "legacy-writer-canvas")]
fn image_key(format: ImageFormat, bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    match format {
        ImageFormat::Png => 1_u8,
        ImageFormat::Jpeg => 2_u8,
        ImageFormat::WebP => 3_u8,
    }
    .hash(&mut hasher);
    bytes.hash(&mut hasher);
    hasher.finish()
}
#[cfg(feature = "legacy-writer-canvas")]
fn color(value: [u8; 4]) -> Vec4f {
    Vec4f {
        x: value[0] as f32 / 255.0,
        y: value[1] as f32 / 255.0,
        z: value[2] as f32 / 255.0,
        w: value[3] as f32 / 255.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use makepad_widgets::{Rect, dvec2};
    use writer_api::WriterApi;
    use writer_api::core::{Command, ParagraphStylePatch};
    use writer_api::layout::PageSize;

    fn page_rect_for_test(
        canvas: Rect,
        placement: Placement,
        scroll_x: f64,
        scroll_y: f64,
    ) -> Rect {
        Rect {
            pos: dvec2(
                canvas.pos.x + placement.x - scroll_x,
                canvas.pos.y + placement.y - scroll_y,
            ),
            size: dvec2(placement.width, placement.height),
        }
    }

    fn intersects_viewport(rect: Rect, viewport: Rect) -> bool {
        rect.pos.x + rect.size.x >= viewport.pos.x
            && rect.pos.x <= viewport.pos.x + viewport.size.x
            && rect.pos.y + rect.size.y >= viewport.pos.y
            && rect.pos.y <= viewport.pos.y + viewport.size.y
    }

    fn blank_page(index: usize, size: PageSize, margins: PageMargins) -> WriterPageFramePage {
        WriterPageFramePage {
            index,
            width_half_points: size.width_half_points,
            height_half_points: size.height_half_points,
            margins,
            header: None,
            footer: None,
            paragraph_ruler_markers: None,
            blocks: Vec::new(),
        }
    }

    #[test]
    fn a4_uses_one_physical_zoom_transform() {
        let width = half_points_to_points(1191, 100);
        let height = half_points_to_points(1684, 100);
        assert!((width / height - 210.0 / 297.0).abs() < 0.002);
        assert!((half_points_to_points(1191, 50) * 2.0 - width).abs() < 0.01);
        assert!((half_points_to_points(1684, 200) - height * 2.0).abs() < 0.01);
    }

    #[test]
    fn list_markers_use_unicode_bullets_and_one_based_numbers() {
        let list_id = writer_api::core::ListId::new(7);
        assert_eq!(
            marker_for_kind(
                &BlockKind::ListItem {
                    list_id,
                    kind: ListKind::Bullet,
                },
                0,
            ),
            Some("\u{2022} ".to_owned())
        );
        assert_eq!(
            marker_for_kind(
                &BlockKind::ListItem {
                    list_id,
                    kind: ListKind::Numbered,
                },
                4,
            ),
            Some("5. ".to_owned())
        );
    }
    #[test]
    fn all_supported_page_formats_keep_their_physical_ratios() {
        let formats = [
            (PageSize::A3, 297.0 / 420.0),
            (PageSize::A4, 210.0 / 297.0),
            (PageSize::A5, 148.0 / 210.0),
            (PageSize::B4, 250.0 / 353.0),
            (PageSize::B5, 176.0 / 250.0),
            (PageSize::LETTER, 8.5 / 11.0),
            (PageSize::LEGAL, 8.5 / 14.0),
        ];
        for (size, expected_ratio) in formats {
            let ratio = half_points_to_points(size.width_half_points, 100)
                / half_points_to_points(size.height_half_points, 100);
            assert!((ratio - expected_ratio).abs() < 0.002, "{size:?}");
        }
        assert_eq!(
            PageSize::A4.rotated().width_half_points,
            PageSize::A4.height_half_points
        );
        assert_eq!(
            PageSize::A4.rotated().height_half_points,
            PageSize::A4.width_half_points
        );
    }

    #[test]
    fn pages_center_and_keep_a_physical_gap_at_every_zoom() {
        let margins = PageMargins::default();
        for zoom_percent in [50, 100, 200] {
            let frame = WriterPageFrame {
                pages: vec![
                    blank_page(0, PageSize::A4, margins),
                    blank_page(1, PageSize::A4.rotated(), margins),
                ],
                zoom_percent,
                scroll_x_points: 0.0,
                scroll_y_points: 0.0,
                truncated: false,
            };
            let (metrics, placed) = placements(&frame, 2_000.0, 800.0);
            assert_eq!(placed.len(), 2);
            for page in &placed {
                assert!((page.x * 2.0 + page.width - metrics.workspace_width_points).abs() < 0.01);
            }
            let gap = half_points_to_points(PAGE_GAP_HALF_POINTS, zoom_percent);
            assert!(
                (placed[1].y - placed[0].y - placed[0].height - gap).abs() < 0.01,
                "zoom={zoom_percent}"
            );
        }
    }

    #[test]
    fn a4_page_rect_intersects_a_nonzero_workspace_at_every_supported_zoom() {
        let margins = PageMargins::default();
        let canvas = Rect {
            pos: dvec2(100.0, 200.0),
            size: dvec2(960.0, 600.0),
        };
        for zoom_percent in [50, 100, 200] {
            let frame = WriterPageFrame {
                pages: vec![blank_page(0, PageSize::A4, margins)],
                zoom_percent,
                scroll_x_points: 0.0,
                scroll_y_points: 0.0,
                truncated: false,
            };
            let (_, placed) = placements(&frame, canvas.size.x, canvas.size.y);
            assert_eq!(frame.pages.len(), 1);
            assert_eq!(placed[0].index, 0);
            assert!(canvas.size.x > 0.0 && canvas.size.y > 0.0);
            let page_rect = page_rect_for_test(canvas, placed[0], 0.0, 0.0);
            assert!(page_rect.size.x > 0.0 && page_rect.size.y > 0.0);
            assert!(
                intersects_viewport(page_rect, canvas),
                "A4 must intersect the workspace at {zoom_percent}%"
            );
        }
    }
    #[test]
    fn paragraph_ruler_markers_follow_margins_and_hanging_indent() {
        let margins = PageMargins::default();
        let expected = WriterParagraphRulerMarkers {
            block_id: 1,
            first_line_half_points: 153,
            left_half_points: 173,
            right_half_points: 1_038,
        };
        assert_eq!(
            ruler_markers_for_paragraph(1, PageSize::A4.width_half_points, margins, 60, 40, -20),
            expected
        );
        let mut api = WriterApi::new();
        api.execute(Command::InsertText {
            text: "hanging".to_string(),
        })
        .unwrap();
        api.execute(Command::SetParagraphStyle {
            block_id: writer_api::core::BlockId::new(1),
            patch: ParagraphStylePatch::default().indents(60, 40, -20),
        })
        .unwrap();
        let document = api.document();
        let layout = api.layout().unwrap();
        let frame = build_writer_page_frame(
            &document,
            &layout,
            api.layout_settings(),
            api.docx_layout(),
            200,
            0.0,
            0.0,
        );
        assert_eq!(frame.pages[0].paragraph_ruler_markers, Some(expected));
        assert!(
            (signed_half_points_to_points(
                expected.left_half_points - expected.first_line_half_points,
                200,
            ) - half_points_to_points(20, 200))
            .abs()
                < 0.01
        );
    }

    #[test]
    fn empty_document_gets_a_page_and_scroll_is_geometry_based() {
        let api = WriterApi::new();
        let document = api.document();
        let layout = api.layout().unwrap();
        let frame = build_writer_page_frame(
            &document,
            &layout,
            api.layout_settings(),
            api.docx_layout(),
            100,
            0.0,
            0.0,
        );
        assert_eq!(frame.pages.len(), 1);
        assert_eq!(
            writer_canvas_metrics(&frame, 1400.0, 700.0).max_scroll_x_points,
            0.0
        );
        assert!(writer_canvas_metrics(&frame, 500.0, 700.0).max_scroll_x_points > 0.0);
    }
    #[test]
    fn table_projection_uses_source_index_when_type_local_ids_overlap() {
        let mut api = WriterApi::new();
        api.execute(Command::InsertText {
            text: "Title before table".to_string(),
        })
        .unwrap();
        api.execute(Command::InsertTable {
            rows: 1,
            columns: 2,
        })
        .unwrap();
        let document = api.document();
        let source_index = document
            .blocks
            .iter()
            .position(|block| matches!(block, BlockSnapshot::Table(_)))
            .unwrap();
        let layout = api.layout().unwrap();
        let layout_table = layout
            .pages
            .iter()
            .flat_map(|page| page.blocks.iter())
            .find(|block| !block.table_rows.is_empty())
            .unwrap();
        assert_eq!(layout_table.source_block_index, source_index);

        let frame = build_writer_page_frame(
            &document,
            &layout,
            api.layout_settings(),
            api.docx_layout(),
            100,
            0.0,
            0.0,
        );
        let table = frame
            .pages
            .iter()
            .flat_map(|page| page.blocks.iter())
            .find_map(|block| match block {
                WriterVisualBlock::Table(value) => Some(value),
                _ => None,
            })
            .unwrap();
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0].cells.len(), 2);
    }

    #[test]
    fn table_projection_keeps_grid_rows_and_cells() {
        let mut api = WriterApi::new();
        api.execute(Command::InsertTable {
            rows: 2,
            columns: 3,
        })
        .unwrap();
        let document = api.document();
        let layout = api.layout().unwrap();
        let frame = build_writer_page_frame(
            &document,
            &layout,
            api.layout_settings(),
            api.docx_layout(),
            100,
            0.0,
            0.0,
        );
        let table = frame.pages[0]
            .blocks
            .iter()
            .find_map(|block| match block {
                WriterVisualBlock::Table(value) => Some(value),
                _ => None,
            })
            .unwrap();
        assert_eq!(table.columns.len(), 3);
        assert_eq!(table.rows.len(), 2);
        assert!(table.rows.iter().all(|row| row.cells.len() == 3));
    }
}
