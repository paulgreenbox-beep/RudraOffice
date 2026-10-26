#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;

use writer_core::{
    BlockKind, BlockSnapshot, CoreError, DocumentSnapshot, LayoutEngine, LineSpacing,
    TableCellSnapshot, TableSnapshot, TextBlockSnapshot,
};

const TABLE_CELL_PADDING_HALF_POINTS: u32 = 12;
const TABLE_LINE_HEIGHT_NUMERATOR: u32 = 6;
const TABLE_LINE_HEIGHT_DENOMINATOR: u32 = 5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayoutMode {
    #[default]
    Pages,
    Continuous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageSize {
    pub width_half_points: u32,
    pub height_half_points: u32,
}

impl PageSize {
    pub const A3: Self = Self {
        width_half_points: 1684,
        height_half_points: 2381,
    };
    pub const A4: Self = Self {
        width_half_points: 1191,
        height_half_points: 1684,
    };
    pub const A5: Self = Self {
        width_half_points: 839,
        height_half_points: 1191,
    };
    pub const B4: Self = Self {
        width_half_points: 1417,
        height_half_points: 2004,
    };
    pub const B5: Self = Self {
        width_half_points: 998,
        height_half_points: 1417,
    };
    pub const LETTER: Self = Self {
        width_half_points: 1224,
        height_half_points: 1584,
    };
    pub const LEGAL: Self = Self {
        width_half_points: 1224,
        height_half_points: 2016,
    };

    /// Convert Word twips (1/1440 inch) to the adapter's half-point unit.
    pub const fn from_twips(width: u32, height: u32) -> Self {
        Self {
            width_half_points: (width + 5) / 10,
            height_half_points: (height + 5) / 10,
        }
    }

    pub const fn rotated(self) -> Self {
        Self {
            width_half_points: self.height_half_points,
            height_half_points: self.width_half_points,
        }
    }
}

impl Default for PageSize {
    fn default() -> Self {
        Self::A4
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageMargins {
    pub top_half_points: u32,
    pub right_half_points: u32,
    pub bottom_half_points: u32,
    pub left_half_points: u32,
}

impl PageMargins {
    pub const fn from_twips(top: u32, right: u32, bottom: u32, left: u32) -> Self {
        Self {
            top_half_points: (top + 5) / 10,
            right_half_points: (right + 5) / 10,
            bottom_half_points: (bottom + 5) / 10,
            left_half_points: (left + 5) / 10,
        }
    }
}

impl Default for PageMargins {
    fn default() -> Self {
        Self {
            top_half_points: 113,
            right_half_points: 113,
            bottom_half_points: 113,
            left_half_points: 113,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutSettings {
    pub mode: LayoutMode,
    pub page_size: PageSize,
    pub margins: PageMargins,
    pub default_line_height_half_points: u32,
    pub characters_per_line: u32,
    pub zoom_percent: u16,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            mode: LayoutMode::Pages,
            page_size: PageSize::default(),
            margins: PageMargins::default(),
            default_line_height_half_points: 24,
            characters_per_line: 80,
            zoom_percent: 100,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentRect {
    pub left_half_points: u32,
    pub top_half_points: u32,
    pub width_half_points: u32,
    pub height_half_points: u32,
}

impl LayoutSettings {
    pub fn with_docx_page_setup(
        mut self,
        width_twips: u32,
        height_twips: u32,
        landscape: bool,
        margins: PageMargins,
    ) -> Self {
        let size = PageSize::from_twips(width_twips, height_twips);
        self.page_size = if landscape { size.rotated() } else { size };
        self.margins = margins;
        self
    }

    pub fn content_rect(&self) -> ContentRect {
        ContentRect {
            left_half_points: self.margins.left_half_points,
            top_half_points: self.margins.top_half_points,
            width_half_points: self.usable_width(),
            height_half_points: self.usable_height(),
        }
    }

    fn usable_height(&self) -> u32 {
        self.page_size
            .height_half_points
            .saturating_sub(self.margins.top_half_points)
            .saturating_sub(self.margins.bottom_half_points)
    }

    fn usable_width(&self) -> u32 {
        self.page_size
            .width_half_points
            .saturating_sub(self.margins.left_half_points)
            .saturating_sub(self.margins.right_half_points)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutError {
    pub message: String,
}

impl LayoutError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for LayoutError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFragmentLayout {
    pub block_id: u64,
    pub text: String,
    pub line_index: usize,
    pub x_half_points: u32,
    pub y_half_points: u32,
    pub width_half_points: u32,
    pub height_half_points: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRowLayout {
    pub row_index: usize,
    pub y_half_points: u32,
    pub height_half_points: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockLayout {
    pub block_id: u64,
    /// Position in the source `DocumentSnapshot::blocks` sequence.
    /// IDs are type-local in Writer, so this is the unambiguous visual link.
    pub source_block_index: usize,
    pub page_index: usize,
    pub start_y_half_points: u32,
    pub height_half_points: u32,
    pub fragments: Vec<TextFragmentLayout>,
    pub table_rows: Vec<TableRowLayout>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageLayout {
    pub index: usize,
    pub width_half_points: u32,
    pub height_half_points: u32,
    pub blocks: Vec<BlockLayout>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutSnapshot {
    pub mode: LayoutMode,
    pub pages: Vec<PageLayout>,
    pub total_height_half_points: u32,
}

impl LayoutSnapshot {
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn block_count(&self) -> usize {
        self.pages.iter().map(|page| page.blocks.len()).sum()
    }

    pub fn block(&self, block_id: u64) -> Option<&BlockLayout> {
        self.pages
            .iter()
            .flat_map(|page| page.blocks.iter())
            .find(|block| block.block_id == block_id)
    }
}

#[derive(Clone, Debug)]
pub struct RudraLayoutEngine {
    settings: LayoutSettings,
}

impl Default for RudraLayoutEngine {
    fn default() -> Self {
        Self::new(LayoutSettings::default())
    }
}

impl RudraLayoutEngine {
    pub fn new(settings: LayoutSettings) -> Self {
        Self { settings }
    }

    pub fn settings(&self) -> LayoutSettings {
        self.settings
    }

    pub fn layout_document(
        &self,
        document: &DocumentSnapshot,
    ) -> Result<LayoutSnapshot, LayoutError> {
        document
            .validate()
            .map_err(|error| LayoutError::invalid(error.to_string()))?;
        if self.settings.characters_per_line == 0 {
            return Err(LayoutError::invalid("characters per line must be non-zero"));
        }
        if self.settings.default_line_height_half_points == 0 {
            return Err(LayoutError::invalid("line height must be non-zero"));
        }
        if self.settings.usable_width() == 0 || self.settings.usable_height() == 0 {
            return Err(LayoutError::invalid("page content area must be non-zero"));
        }

        let top = self.settings.margins.top_half_points;
        let bottom = self
            .settings
            .page_size
            .height_half_points
            .saturating_sub(self.settings.margins.bottom_half_points);
        let mut pages = vec![self.empty_page(0)];
        let mut current_y = top;
        let mut total_height = current_y;

        for (source_block_index, block) in document.blocks.iter().enumerate() {
            if matches!(block, BlockSnapshot::Text(text) if matches!(&text.kind, BlockKind::PageBreak))
            {
                if self.settings.mode == LayoutMode::Pages {
                    let index = pages.len();
                    pages.push(self.empty_page(index));
                    current_y = top;
                }
                continue;
            }

            if let BlockSnapshot::Table(table) = block {
                self.layout_table_across_pages(
                    table,
                    source_block_index,
                    &mut pages,
                    &mut current_y,
                    &mut total_height,
                    top,
                    bottom,
                );
                continue;
            }

            let page_index = pages.len() - 1;
            let layout = self.layout_block(block, source_block_index, page_index, current_y)?;
            if self.settings.mode == LayoutMode::Pages
                && current_y > top
                && current_y.saturating_add(layout.height_half_points) > bottom
            {
                let index = pages.len();
                pages.push(self.empty_page(index));
                current_y = top;
                let moved = self.layout_block(block, source_block_index, index, current_y)?;
                current_y = current_y.saturating_add(moved.height_half_points);
                total_height = total_height.saturating_add(moved.height_half_points);
                pages[index].blocks.push(moved);
            } else {
                current_y = current_y.saturating_add(layout.height_half_points);
                total_height = total_height.saturating_add(layout.height_half_points);
                pages[page_index].blocks.push(layout);
            }
        }

        if self.settings.mode == LayoutMode::Continuous {
            let mut only_page = pages.remove(0);
            only_page.height_half_points = total_height.max(top);
            pages.push(only_page);
        }
        Ok(LayoutSnapshot {
            mode: self.settings.mode,
            pages,
            total_height_half_points: total_height,
        })
    }

    fn empty_page(&self, index: usize) -> PageLayout {
        PageLayout {
            index,
            width_half_points: self.settings.page_size.width_half_points,
            height_half_points: self.settings.page_size.height_half_points,
            blocks: Vec::new(),
        }
    }

    // The pagination helper receives one cohesive mutable layout transaction.
    #[allow(clippy::too_many_arguments)]
    fn layout_table_across_pages(
        &self,
        table: &TableSnapshot,
        source_block_index: usize,
        pages: &mut Vec<PageLayout>,
        current_y: &mut u32,
        total_height: &mut u32,
        top: u32,
        bottom: u32,
    ) {
        let row_heights = self.table_row_heights(table);
        let mut segment_start = *current_y;
        let mut segment_rows = Vec::new();

        for (row_index, height_half_points) in row_heights.into_iter().enumerate() {
            let overflows_page = self.settings.mode == LayoutMode::Pages
                && current_y.saturating_add(height_half_points) > bottom;
            if overflows_page && !segment_rows.is_empty() {
                let page_index = pages.len() - 1;
                pages[page_index].blocks.push(Self::table_block(
                    table,
                    source_block_index,
                    page_index,
                    segment_start,
                    std::mem::take(&mut segment_rows),
                ));
                let index = pages.len();
                pages.push(self.empty_page(index));
                *current_y = top;
                segment_start = *current_y;
            } else if overflows_page && *current_y > top {
                let index = pages.len();
                pages.push(self.empty_page(index));
                *current_y = top;
                segment_start = *current_y;
            }

            segment_rows.push(TableRowLayout {
                row_index,
                y_half_points: *current_y,
                height_half_points,
            });
            *current_y = current_y.saturating_add(height_half_points);
            *total_height = total_height.saturating_add(height_half_points);
        }

        if segment_rows.is_empty() {
            segment_rows.push(TableRowLayout {
                row_index: 0,
                y_half_points: *current_y,
                height_half_points: self.settings.default_line_height_half_points,
            });
            *current_y = current_y.saturating_add(self.settings.default_line_height_half_points);
            *total_height =
                total_height.saturating_add(self.settings.default_line_height_half_points);
        }
        let page_index = pages.len() - 1;
        pages[page_index].blocks.push(Self::table_block(
            table,
            source_block_index,
            page_index,
            segment_start,
            segment_rows,
        ));
    }

    fn table_block(
        table: &TableSnapshot,
        source_block_index: usize,
        page_index: usize,
        start_y_half_points: u32,
        table_rows: Vec<TableRowLayout>,
    ) -> BlockLayout {
        let height_half_points = table_rows
            .last()
            .map(|row| {
                row.y_half_points
                    .saturating_add(row.height_half_points)
                    .saturating_sub(start_y_half_points)
            })
            .unwrap_or(0);
        BlockLayout {
            block_id: table.id.get(),
            source_block_index,
            page_index,
            start_y_half_points,
            height_half_points,
            fragments: Vec::new(),
            table_rows,
        }
    }

    fn table_cell_metrics(&self, cell: &TableCellSnapshot) -> (u32, u32) {
        let font_size = cell
            .runs
            .iter()
            .filter(|run| !run.text.is_empty())
            .map(|run| u32::from(run.style.size_half_points()))
            .max()
            .unwrap_or(self.settings.default_line_height_half_points)
            .max(1);
        let scaled_line_height = font_size
            .saturating_mul(TABLE_LINE_HEIGHT_NUMERATOR)
            .saturating_add(TABLE_LINE_HEIGHT_DENOMINATOR - 1)
            / TABLE_LINE_HEIGHT_DENOMINATOR;
        let line_height = self
            .settings
            .default_line_height_half_points
            .max(scaled_line_height)
            .max(1);
        let char_width = font_size.saturating_mul(52).saturating_add(99) / 100;
        (line_height, char_width.max(1))
    }

    fn table_row_heights(&self, table: &TableSnapshot) -> Vec<u32> {
        let columns = self.table_column_widths(table);
        let mut heights = Vec::with_capacity(table.rows.len().max(1));
        for row in &table.rows {
            let height = row
                .cells
                .iter()
                .enumerate()
                .map(|(index, cell)| {
                    let (line_height, char_width) = self.table_cell_metrics(cell);
                    let width = columns
                        .get(index)
                        .copied()
                        .unwrap_or_else(|| self.settings.usable_width());
                    let capacity = width
                        .saturating_sub(TABLE_CELL_PADDING_HALF_POINTS.saturating_mul(2))
                        .checked_div(char_width)
                        .unwrap_or(1)
                        .max(1) as usize;
                    (wrapped_line_count(&cell.text, capacity) as u32)
                        .saturating_mul(line_height)
                        .saturating_add(TABLE_CELL_PADDING_HALF_POINTS.saturating_mul(2))
                })
                .max()
                .unwrap_or_else(|| {
                    self.settings
                        .default_line_height_half_points
                        .max(1)
                        .saturating_add(TABLE_CELL_PADDING_HALF_POINTS.saturating_mul(2))
                });
            heights.push(height);
        }
        if heights.is_empty() {
            heights.push(
                self.settings
                    .default_line_height_half_points
                    .max(1)
                    .saturating_add(TABLE_CELL_PADDING_HALF_POINTS.saturating_mul(2)),
            );
        }
        heights
    }

    fn table_column_widths(&self, table: &TableSnapshot) -> Vec<u32> {
        let content_width = self.settings.usable_width().max(1);
        let mut source = table
            .columns
            .iter()
            .map(|column| column.width_half_points.max(1))
            .collect::<Vec<_>>();
        if source.is_empty() {
            source.push(content_width);
            return source;
        }
        let total = source
            .iter()
            .fold(0_u64, |sum, width| sum + u64::from(*width));
        if total == 0 {
            return vec![content_width / source.len() as u32; source.len()];
        }
        source
            .into_iter()
            .map(|width| {
                ((u64::from(width) * u64::from(content_width)) / total)
                    .max(1)
                    .min(u64::from(u32::MAX)) as u32
            })
            .collect()
    }

    fn layout_block(
        &self,
        block: &BlockSnapshot,
        source_block_index: usize,
        page_index: usize,
        y: u32,
    ) -> Result<BlockLayout, LayoutError> {
        match block {
            BlockSnapshot::Text(text) => self.layout_text(text, source_block_index, page_index, y),
            BlockSnapshot::Table(table) => {
                let rows = self
                    .table_row_heights(table)
                    .into_iter()
                    .enumerate()
                    .scan(y, |row_y, (row_index, height_half_points)| {
                        let row = TableRowLayout {
                            row_index,
                            y_half_points: *row_y,
                            height_half_points,
                        };
                        *row_y = row.y_half_points.saturating_add(row.height_half_points);
                        Some(row)
                    })
                    .collect();
                Ok(Self::table_block(
                    table,
                    source_block_index,
                    page_index,
                    y,
                    rows,
                ))
            }
            BlockSnapshot::Image(image) => Ok(BlockLayout {
                block_id: image.id.get(),
                source_block_index,
                page_index,
                start_y_half_points: y,
                height_half_points: image.layout.size.height.max(1),
                fragments: Vec::new(),
                table_rows: Vec::new(),
            }),
        }
    }

    fn layout_text(
        &self,
        block: &TextBlockSnapshot,
        source_block_index: usize,
        page_index: usize,
        y: u32,
    ) -> Result<BlockLayout, LayoutError> {
        let mut fragments = Vec::new();
        let mut line = String::new();
        let mut line_index = 0usize;
        let left = i64::from(self.settings.margins.left_half_points)
            + i64::from(block.paragraph_style.indent_left_half_points());
        let right_edge = i64::from(self.settings.page_size.width_half_points)
            - i64::from(self.settings.margins.right_half_points)
            - i64::from(block.paragraph_style.indent_right_half_points());
        let clamp_x = |value: i64| value.clamp(0, i64::from(u32::MAX)) as u32;
        let width_at = |x: u32| {
            right_edge
                .saturating_sub(i64::from(x))
                .clamp(1, i64::from(u32::MAX)) as u32
        };
        let normal_x = clamp_x(left);
        let first_x = clamp_x(left.saturating_add(i64::from(
            block.paragraph_style.first_line_indent_half_points(),
        )));
        let normal_width = width_at(normal_x);
        let first_width = width_at(first_x);
        let capacity_for = |width: u32| {
            ((u64::from(self.settings.characters_per_line).saturating_mul(u64::from(width))
                / u64::from(normal_width.max(1)))
            .max(1)) as usize
        };
        let mut x = first_x;
        let mut line_width = first_width;
        let mut line_capacity = capacity_for(first_width);
        let before = block.paragraph_style.space_before_half_points();
        let after = block.paragraph_style.space_after_half_points();
        let content_y = y.saturating_add(before);
        let line_height = line_height(
            block.paragraph_style.line_spacing(),
            self.settings.default_line_height_half_points,
        )
        .max(1);
        for ch in block.text.chars() {
            if ch == '\n' || line.chars().count() >= line_capacity {
                if !line.is_empty() {
                    fragments.push(TextFragmentLayout {
                        block_id: block.id.get(),
                        text: std::mem::take(&mut line),
                        line_index,
                        x_half_points: x,
                        y_half_points: content_y.saturating_add(line_index as u32 * line_height),
                        width_half_points: line_width,
                        height_half_points: line_height,
                    });
                }
                line_index += 1;
                x = normal_x;
                line_width = normal_width;
                line_capacity = capacity_for(normal_width);
                if ch == '\n' {
                    continue;
                }
            }
            line.push(ch);
        }
        if !line.is_empty() || fragments.is_empty() {
            fragments.push(TextFragmentLayout {
                block_id: block.id.get(),
                text: line,
                line_index,
                x_half_points: x,
                y_half_points: content_y.saturating_add(line_index as u32 * line_height),
                width_half_points: line_width,
                height_half_points: line_height,
            });
        }
        Ok(BlockLayout {
            block_id: block.id.get(),
            source_block_index,
            page_index,
            start_y_half_points: y,
            height_half_points: before
                .saturating_add((line_index as u32 + 1).saturating_mul(line_height))
                .saturating_add(after),
            fragments,
            table_rows: Vec::new(),
        })
    }
}

fn wrapped_line_count(text: &str, capacity: usize) -> usize {
    if text.is_empty() {
        return 1;
    }
    let capacity = capacity.max(1);
    let mut lines = 1usize;
    let mut used = 0usize;
    for character in text.chars() {
        if character == '\n' {
            lines += 1;
            used = 0;
            continue;
        }
        used += 1;
        if used > capacity {
            lines += 1;
            used = 1;
        }
    }
    lines
}

impl LayoutEngine for RudraLayoutEngine {
    fn layout(&self, document: &DocumentSnapshot) -> Result<(), CoreError> {
        self.layout_document(document)
            .map(|_| ())
            .map_err(|_| CoreError::UnsupportedFormat)
    }
}

fn line_height(spacing: LineSpacing, base: u32) -> u32 {
    match spacing {
        LineSpacing::Single => base,
        LineSpacing::OneAndHalf => base.saturating_mul(3) / 2,
        LineSpacing::Double => base.saturating_mul(2),
        LineSpacing::Multiple(value) => base.saturating_mul(value as u32) / 100,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use writer_core::{
        BlockId, CellAlignment, CellId, CellStyle, ColumnId, Command, Editor, InlineRun,
        ParagraphStylePatch, RowId, TableColumnSnapshot, TableId, TableRowSnapshot, TextStyle,
    };

    fn editor_with_text(text: &str) -> DocumentSnapshot {
        let mut editor = Editor::new();
        editor
            .execute(Command::InsertText {
                text: text.to_string(),
            })
            .unwrap();
        editor.snapshot()
    }

    #[test]
    fn page_layout_is_deterministic_and_wraps_text() {
        let document = editor_with_text("abcdefghijklmnop");
        let engine = RudraLayoutEngine::new(LayoutSettings {
            characters_per_line: 4,
            ..LayoutSettings::default()
        });
        let layout = engine.layout_document(&document).unwrap();
        assert_eq!(layout.page_count(), 1);
        assert_eq!(layout.block_count(), 1);
        assert_eq!(
            layout.block(BlockId::new(1).get()).unwrap().fragments.len(),
            4
        );
    }

    #[test]
    fn continuous_layout_has_one_growing_page() {
        let document = editor_with_text("a\nb\nc");
        let engine = RudraLayoutEngine::new(LayoutSettings {
            mode: LayoutMode::Continuous,
            ..LayoutSettings::default()
        });
        let layout = engine.layout_document(&document).unwrap();
        assert_eq!(layout.mode, LayoutMode::Continuous);
        assert_eq!(layout.page_count(), 1);
        assert_eq!(
            layout.pages[0].height_half_points,
            layout.total_height_half_points
        );
        assert!(
            layout.pages[0].height_half_points
                < LayoutSettings::default().page_size.height_half_points
        );
    }

    #[test]
    fn layout_trait_accepts_valid_document() {
        let document = editor_with_text("trait");
        assert!(RudraLayoutEngine::default().layout(&document).is_ok());
    }
    #[test]
    fn physical_page_sizes_and_content_rect_are_stable() {
        let sizes = [
            PageSize::A3,
            PageSize::A4,
            PageSize::A5,
            PageSize::B4,
            PageSize::B5,
            PageSize::LETTER,
            PageSize::LEGAL,
        ];
        assert!(
            sizes
                .iter()
                .all(|size| size.width_half_points > 0 && size.height_half_points > 0)
        );
        assert_eq!(PageSize::from_twips(11_906, 16_838), PageSize::A4);
        assert_eq!(
            PageSize::A4.rotated().width_half_points,
            PageSize::A4.height_half_points
        );
        let settings = LayoutSettings {
            page_size: PageSize::A4,
            margins: PageMargins {
                top_half_points: 100,
                right_half_points: 200,
                bottom_half_points: 300,
                left_half_points: 400,
            },
            ..LayoutSettings::default()
        };
        assert_eq!(
            settings.content_rect(),
            ContentRect {
                left_half_points: 400,
                top_half_points: 100,
                width_half_points: 591,
                height_half_points: 1284
            }
        );
    }

    #[test]
    fn docx_page_setup_applies_orientation_and_margins() {
        let settings = LayoutSettings::default().with_docx_page_setup(
            11_906,
            16_838,
            true,
            PageMargins::from_twips(1440, 2160, 2880, 3600),
        );
        assert_eq!(settings.page_size, PageSize::A4.rotated());
        assert_eq!(
            settings.margins,
            PageMargins {
                top_half_points: 144,
                right_half_points: 216,
                bottom_half_points: 288,
                left_half_points: 360
            }
        );
    }

    #[test]
    fn table_rows_split_on_pages_and_ignore_view_zoom() {
        let mut editor = Editor::new();
        editor
            .execute(Command::InsertTable {
                rows: 8,
                columns: 1,
            })
            .unwrap();
        let document = editor.snapshot();
        let base = LayoutSettings {
            page_size: PageSize {
                width_half_points: 400,
                height_half_points: 170,
            },
            margins: PageMargins {
                top_half_points: 20,
                right_half_points: 20,
                bottom_half_points: 20,
                left_half_points: 20,
            },
            default_line_height_half_points: 24,
            ..LayoutSettings::default()
        };
        let at_50 = RudraLayoutEngine::new(LayoutSettings {
            zoom_percent: 50,
            ..base
        })
        .layout_document(&document)
        .unwrap();
        let at_200 = RudraLayoutEngine::new(LayoutSettings {
            zoom_percent: 200,
            ..base
        })
        .layout_document(&document)
        .unwrap();
        assert_eq!(at_50, at_200);
        assert!(at_50.page_count() > 1);
        let rows = at_50
            .pages
            .iter()
            .flat_map(|page| page.blocks.iter())
            .flat_map(|block| block.table_rows.iter().map(|row| row.row_index))
            .collect::<Vec<_>>();
        assert_eq!(rows, (0..8).collect::<Vec<_>>());
    }

    #[test]
    fn table_rows_reserve_height_for_the_largest_cell_font() {
        let style = TextStyle::new(40).unwrap();
        let table = TableSnapshot {
            id: TableId::new(1),
            columns: vec![TableColumnSnapshot {
                id: ColumnId::new(1),
                width_half_points: 200,
            }],
            rows: vec![TableRowSnapshot {
                id: RowId::new(1),
                cells: vec![TableCellSnapshot {
                    id: CellId::new(1),
                    text: "abcdefgh".to_owned(),
                    runs: vec![InlineRun::new("abcdefgh", style)],
                    style: CellStyle::default(),
                    alignment: CellAlignment::Left,
                }],
            }],
        };
        let engine = RudraLayoutEngine::new(LayoutSettings {
            default_line_height_half_points: 24,
            ..LayoutSettings::default()
        });

        let heights = engine.table_row_heights(&table);

        assert_eq!(heights, vec![72]);
    }

    #[test]
    fn empty_table_row_height_keeps_default_line_and_padding() {
        let table = TableSnapshot {
            id: TableId::new(1),
            columns: Vec::new(),
            rows: Vec::new(),
        };
        let engine = RudraLayoutEngine::new(LayoutSettings::default());

        assert_eq!(engine.table_row_heights(&table), vec![48]);
    }

    #[test]
    fn paragraph_spacing_and_hanging_indent_follow_physical_geometry() {
        let mut editor = Editor::new();
        editor
            .execute(Command::InsertText {
                text: "abcdefgh".to_string(),
            })
            .unwrap();
        editor
            .execute(Command::SetParagraphStyle {
                block_id: BlockId::new(1),
                patch: ParagraphStylePatch::default()
                    .indents(40, 50, -20)
                    .spacing(30, 40),
            })
            .unwrap();
        let settings = LayoutSettings {
            page_size: PageSize {
                width_half_points: 600,
                height_half_points: 500,
            },
            margins: PageMargins {
                top_half_points: 100,
                right_half_points: 100,
                bottom_half_points: 100,
                left_half_points: 100,
            },
            default_line_height_half_points: 10,
            characters_per_line: 4,
            ..LayoutSettings::default()
        };
        let layout = RudraLayoutEngine::new(settings)
            .layout_document(&editor.snapshot())
            .unwrap();
        let block = &layout.pages[0].blocks[0];
        assert_eq!(block.fragments.len(), 2);
        assert_eq!(block.fragments[0].x_half_points, 120);
        assert_eq!(block.fragments[1].x_half_points, 140);
        assert_eq!(block.fragments[0].width_half_points, 330);
        assert_eq!(block.fragments[1].width_half_points, 310);
        assert_eq!(block.fragments[0].y_half_points, 130);
        assert_eq!(block.fragments[1].y_half_points, 140);
        assert_eq!(block.height_half_points, 90);
    }
}
