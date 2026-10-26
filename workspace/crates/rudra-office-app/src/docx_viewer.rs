use writer_api::core::{
    BlockKind, BlockSnapshot, DocumentSnapshot, ImageFormat, ImageSnapshot, InlineRun, ListKind,
};
use writer_rudra_adapter::LayoutSnapshot;

pub const MAX_PAGES: usize = 200;
pub const MAX_BLOCKS_PER_PAGE: usize = 400;
pub const MAX_TABLE_ROWS: usize = 100;
pub const MAX_TABLE_COLUMNS: usize = 40;
pub const MAX_TEXT_BYTES: usize = 24 * 1024;
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualDocument {
    pub pages: Vec<VisualPage>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualPage {
    pub index: usize,
    pub width_half_points: u32,
    pub height_half_points: u32,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionError {
    MissingLayout,
    ResourceLimit,
}

pub fn embedded_images(document: &DocumentSnapshot) -> impl Iterator<Item = &ImageSnapshot> {
    document.blocks.iter().filter_map(BlockSnapshot::image)
}

pub fn project_document(
    document: &DocumentSnapshot,
    layout: Option<&LayoutSnapshot>,
) -> Result<VisualDocument, ProjectionError> {
    let layout = layout.ok_or(ProjectionError::MissingLayout)?;
    if layout.pages.len() > MAX_PAGES {
        return Err(ProjectionError::ResourceLimit);
    }

    let mut truncated = false;
    let mut pages = Vec::with_capacity(layout.pages.len());
    for page in &layout.pages {
        let mut text = format!("ЛИСТ {}\n\n", page.index.saturating_add(1));
        if page.blocks.len() > MAX_BLOCKS_PER_PAGE {
            truncated = true;
        }
        for block_layout in page.blocks.iter().take(MAX_BLOCKS_PER_PAGE) {
            let Some(block) = document
                .blocks
                .iter()
                .find(|block| block.id() == block_layout.block_id)
            else {
                continue;
            };
            append_block(&mut text, block, &mut truncated);
            if text.len() >= MAX_TEXT_BYTES {
                text.truncate(MAX_TEXT_BYTES);
                text.push_str("\n\n[… содержимое страницы ограничено …]");
                truncated = true;
                break;
            }
        }
        if truncated && text.len() < MAX_TEXT_BYTES {
            text.push_str("\n\n[Просмотр ограничен безопасным лимитом]");
        }
        pages.push(VisualPage {
            index: page.index,
            width_half_points: page.width_half_points,
            height_half_points: page.height_half_points,
            text,
        });
    }

    Ok(VisualDocument { pages, truncated })
}

fn append_block(output: &mut String, block: &BlockSnapshot, truncated: &mut bool) {
    match block {
        BlockSnapshot::Text(block) => {
            let prefix = match block.kind {
                BlockKind::Heading(level) => format!("{} ", "#".repeat(level.get() as usize)),
                BlockKind::ListItem {
                    kind: ListKind::Bullet,
                    ..
                } => "• ".to_string(),
                BlockKind::ListItem {
                    kind: ListKind::Numbered,
                    ..
                } => "1. ".to_string(),
                BlockKind::Paragraph | BlockKind::PageBreak => String::new(),
            };
            output.push_str(&prefix);
            append_runs(output, &block.runs);
            output.push_str("\n\n");
        }
        BlockSnapshot::Table(table) => {
            output.push_str("┌ Таблица ─────────────────────────────┐\n");
            let rows = table.rows.len().min(MAX_TABLE_ROWS);
            let columns = table.columns.len().min(MAX_TABLE_COLUMNS);
            for row in table.rows.iter().take(rows) {
                output.push('│');
                for cell in row.cells.iter().take(columns) {
                    output.push(' ');
                    append_runs(output, &cell.runs);
                    output.push_str(" │");
                }
                output.push('\n');
            }
            if table.rows.len() > rows || table.columns.len() > columns {
                *truncated = true;
                output.push_str("│ … строки/столбцы ограничены … │\n");
            }
            output.push_str("└──────────────────────────────────────┘\n\n");
        }
        BlockSnapshot::Image(image) => {
            let format = match image.payload.format {
                ImageFormat::Png => "PNG",
                ImageFormat::Jpeg => "JPEG",
                ImageFormat::WebP => "WebP",
            };
            if image.payload.bytes.len() > MAX_IMAGE_BYTES {
                *truncated = true;
            }
            output.push_str(&format!(
                "┌─ Встроенное изображение {} {}×{} ({} байт) ─┐\n│  Предпросмотр изображения доступен в документе. │\n└───────────────────────────────────────────────┘\n\n",
                format,
                image.payload.dimensions.width,
                image.payload.dimensions.height,
                image.payload.bytes.len().min(MAX_IMAGE_BYTES),
            ));
        }
    }
}

fn append_runs(output: &mut String, runs: &[InlineRun]) {
    for run in runs {
        let mut text = run.text.clone();
        if run.style.strikethrough() {
            text = format!("~~{text}~~");
        }
        if run.style.underline() {
            text = format!("__{text}__");
        }
        if run.style.italic() {
            text = format!("_{text}_");
        }
        if run.style.bold() {
            text = format!("**{text}**");
        }
        output.push_str(&text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use writer_api::core::{
        BlockId, DocumentSnapshot, InlineRun, ParagraphStyle, Revision, TextBlockSnapshot,
        TextStyle,
    };
    use writer_rudra_adapter::{BlockLayout, LayoutMode, PageLayout};

    fn document(blocks: Vec<BlockSnapshot>) -> DocumentSnapshot {
        DocumentSnapshot {
            blocks,
            revision: Revision::new(1),
        }
    }

    #[test]
    fn projection_preserves_headings_lists_and_run_markers() {
        let heading = BlockSnapshot::Text(TextBlockSnapshot {
            id: BlockId::new(1),
            kind: BlockKind::Heading(writer_api::core::HeadingLevel::new(1).unwrap()),
            paragraph_style: ParagraphStyle::default(),
            text: "Title".into(),
            runs: vec![InlineRun::new(
                "Title",
                TextStyle::default().with_bold(true),
            )],
        });
        let layout = LayoutSnapshot {
            mode: LayoutMode::Pages,
            pages: vec![PageLayout {
                index: 0,
                width_half_points: 100,
                height_half_points: 200,
                blocks: vec![BlockLayout {
                    block_id: 1,
                    source_block_index: 0,
                    page_index: 0,
                    start_y_half_points: 10,
                    height_half_points: 20,
                    fragments: vec![],
                    table_rows: vec![],
                }],
            }],
            total_height_half_points: 200,
        };
        let rendered = project_document(&document(vec![heading]), Some(&layout)).unwrap();
        assert!(rendered.pages[0].text.contains("# **Title**"));
    }

    #[test]
    fn embedded_image_projection_is_bounded_and_discoverable() {
        let image = BlockSnapshot::Image(ImageSnapshot {
            id: writer_api::core::ImageId::new(7),
            payload: writer_api::core::ImagePayload::new(
                ImageFormat::Png,
                writer_api::core::PixelSize::new(1, 1).unwrap(),
                vec![0x89, b'P', b'N', b'G'],
            )
            .unwrap(),
            layout: writer_api::core::ImageLayout::default(),
        });
        let document = document(vec![image]);
        assert_eq!(embedded_images(&document).count(), 1);
        let layout = LayoutSnapshot {
            mode: LayoutMode::Pages,
            pages: vec![PageLayout {
                index: 0,
                width_half_points: 100,
                height_half_points: 200,
                blocks: vec![BlockLayout {
                    block_id: 7,
                    source_block_index: 0,
                    page_index: 0,
                    start_y_half_points: 10,
                    height_half_points: 20,
                    fragments: vec![],
                    table_rows: vec![],
                }],
            }],
            total_height_half_points: 200,
        };
        let rendered = project_document(&document, Some(&layout)).unwrap();
        assert!(rendered.pages[0].text.contains("PNG"));
    }
    #[test]
    fn missing_layout_is_controlled_error() {
        let document = document(vec![]);
        assert_eq!(
            project_document(&document, None),
            Err(ProjectionError::MissingLayout)
        );
    }
}
