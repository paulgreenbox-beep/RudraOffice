#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;
use std::fmt::Write as _;
use std::io::Cursor as IoCursor;
use std::panic::{self, AssertUnwindSafe};

use office_oxide::{
    Document as OfficeOxideDocument, DocumentFormat as OfficeOxideFormat,
    create::create_from_ir_to_writer,
    ir::{
        BorderLine as IrBorderLine, BorderStyle as IrBorderStyle,
        CellVerticalAlign as IrCellVerticalAlign, DocumentIR, Element as IrElement,
        Heading as IrHeading, Image as IrImage, ImageFormat as IrImageFormat,
        InlineContent as IrInlineContent, LineSpacing as IrLineSpacing, List as IrList,
        ListItem as IrListItem, Paragraph as IrParagraph,
        ParagraphAlignment as IrParagraphAlignment, Table as IrTable, TableBorder as IrTableBorder,
        TableCell as IrTableCell, TableRow as IrTableRow, TextSpan as IrTextSpan,
        UnderlineStyle as IrUnderlineStyle,
    },
};

use writer_core::*;
use zip::ZipArchive;

mod opc;

const CONTENT_TYPES: &str = "application/vnd.openxmlformats-package.content-types+xml";
const RELATIONSHIPS: &str = "application/vnd.openxmlformats-package.relationships+xml";
const DOCUMENT: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const XML: &str = "application/xml";
const PNG: &str = "image/png";
const JPEG: &str = "image/jpeg";
const WEBP: &str = "image/webp";
const PART_DOCUMENT: &str = "word/document.xml";
const PART_CONTENT_TYPES: &str = "[Content_Types].xml";
const PART_RELS: &str = "_rels/.rels";
const PART_DOCUMENT_RELS: &str = "word/_rels/document.xml.rels";
const PART_METADATA: &str = "docProps/core.xml";
const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const REL_OFFICE_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const RUDRA_NS: &str = "urn:rudra:writer";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocxExportOptions {
    pub author: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DocxImportOptions {
    pub strict: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocxErrorKind {
    InvalidPackage,
    MissingPart,
    UnsupportedPart,
    XmlDecode,
    SchemaViolation,
    DuplicateBlock,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocxError {
    pub kind: DocxErrorKind,
    pub message: String,
}

impl DocxError {
    fn new(kind: DocxErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for DocxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DOCX {}: {}", self.kind, self.message)
    }
}

impl fmt::Display for DocxErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidPackage => "invalid package",
            Self::MissingPart => "missing part",
            Self::UnsupportedPart => "unsupported part",
            Self::XmlDecode => "XML decode error",
            Self::SchemaViolation => "schema violation",
            Self::DuplicateBlock => "duplicate block",
        })
    }
}

impl Error for DocxError {}

impl From<ModelError> for DocxError {
    fn from(value: ModelError) -> Self {
        Self::new(DocxErrorKind::SchemaViolation, value.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatOpcPart {
    pub name: String,
    pub content_type: String,
    pub data: FlatOpcData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlatOpcData {
    Xml(String),
    Binary(Vec<u8>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FlatOpcPackage {
    pub parts: Vec<FlatOpcPart>,
}

/// A real standard .docx ZIP package.
pub type DocxPackage = opc::DocxPackage;

pub struct DocxExporter;

impl Default for DocxExporter {
    fn default() -> Self {
        Self
    }
}

impl DocxExporter {
    pub fn export(&self, document: &DocumentSnapshot) -> Result<Vec<u8>, DocxError> {
        self.export_with_options(document, DocxExportOptions::default())
    }

    pub fn export_with_options(
        &self,
        document: &DocumentSnapshot,
        options: DocxExportOptions,
    ) -> Result<Vec<u8>, DocxError> {
        document.validate()?;
        let ir = snapshot_to_office_oxide_ir(document, &options)?;
        let mut output = IoCursor::new(Vec::new());
        create_from_ir_to_writer(&ir, OfficeOxideFormat::Docx, &mut output).map_err(|error| {
            DocxError::new(
                DocxErrorKind::SchemaViolation,
                format!("Office Oxide could not write DOCX: {error}"),
            )
        })?;
        Ok(output.into_inner())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocxSectionLayout {
    pub width_twips: u32,
    pub height_twips: u32,
    pub margin_top_twips: u32,
    pub margin_right_twips: u32,
    pub margin_bottom_twips: u32,
    pub margin_left_twips: u32,
    pub header_distance_twips: u32,
    pub footer_distance_twips: u32,
    pub landscape: bool,
    pub header_text: String,
    pub footer_text: String,
    pub first_page_header_text: String,
    pub first_page_footer_text: String,
    pub even_page_header_text: String,
    pub even_page_footer_text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocxLayoutMetadata {
    pub sections: Vec<DocxSectionLayout>,
}

/// Counts extracted directly from Office Oxide's external DOCX model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocxSourceContentCounts {
    pub paragraphs: usize,
    pub headings: usize,
    pub lists: usize,
    pub tables: usize,
    pub table_cells: usize,
    pub images: usize,
    pub thematic_breaks: usize,
    pub page_breaks: usize,
    pub column_breaks: usize,
    pub text_boxes: usize,
    pub footnotes: usize,
    pub endnotes: usize,
    pub code_blocks: usize,
    pub shapes: usize,
    pub other_elements: usize,
}

/// Counts retained by the Writer snapshot after DOCX projection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocxProjectedContentCounts {
    pub text_blocks: usize,
    pub list_items: usize,
    pub table_blocks: usize,
    pub table_cells: usize,
    pub image_blocks: usize,
    pub page_break_blocks: usize,
    pub fallback_blocks: usize,
}

/// Content represented through an explicit textual fallback rather than silently lost.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocxFallbackCounts {
    pub thematic_breaks: usize,
    pub column_breaks: usize,
    pub text_boxes: usize,
    pub footnotes: usize,
    pub endnotes: usize,
    pub code_blocks: usize,
    pub shapes: usize,
    pub unsupported_images: usize,
    pub irregular_tables: usize,
    pub other_elements: usize,
}

impl DocxFallbackCounts {
    pub fn total(&self) -> usize {
        self.thematic_breaks
            .saturating_add(self.column_breaks)
            .saturating_add(self.text_boxes)
            .saturating_add(self.footnotes)
            .saturating_add(self.endnotes)
            .saturating_add(self.code_blocks)
            .saturating_add(self.shapes)
            .saturating_add(self.unsupported_images)
            .saturating_add(self.irregular_tables)
            .saturating_add(self.other_elements)
    }
}

/// Lightweight evidence describing how an external DOCX reached the Writer viewer model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocxProjectionDiagnostics {
    pub source: DocxSourceContentCounts,
    pub projected: DocxProjectedContentCounts,
    pub fallback: DocxFallbackCounts,
}

impl DocxProjectionDiagnostics {
    pub fn has_fallback_content(&self) -> bool {
        self.fallback.total() > 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocxImportResult {
    pub snapshot: DocumentSnapshot,
    pub layout: DocxLayoutMetadata,
    pub projection: DocxProjectionDiagnostics,
}
pub struct DocxImporter;

impl Default for DocxImporter {
    fn default() -> Self {
        Self
    }
}

impl DocxImporter {
    pub fn import_with_layout(&self, bytes: &[u8]) -> Result<DocxImportResult, DocxError> {
        if is_bounded_standard_docx(bytes) {
            let is_legacy = matches!(
                opc::parse_zip(bytes),
                Ok(package) if is_rudra_legacy_package(&package)
            );
            if !is_legacy {
                return import_with_office_oxide_layout(bytes);
            }
        }
        let snapshot = self.import(bytes)?;
        let projection = DocxProjectionDiagnostics {
            projected: projected_content_counts(&snapshot),
            ..DocxProjectionDiagnostics::default()
        };
        Ok(DocxImportResult {
            snapshot,
            layout: DocxLayoutMetadata::default(),
            projection,
        })
    }

    pub fn import(&self, bytes: &[u8]) -> Result<DocumentSnapshot, DocxError> {
        self.import_with_options(bytes, DocxImportOptions::default())
    }

    pub fn import_with_options(
        &self,
        bytes: &[u8],
        options: DocxImportOptions,
    ) -> Result<DocumentSnapshot, DocxError> {
        match opc::parse_zip(bytes) {
            Ok(package) => {
                if is_rudra_legacy_package(&package) {
                    read_document(&package, options)
                } else if is_bounded_standard_docx(bytes) {
                    import_with_office_oxide(bytes)
                } else {
                    read_document(&package, options)
                }
            }
            Err(legacy_error) => {
                if is_bounded_standard_docx(bytes) {
                    import_with_office_oxide(bytes)
                } else {
                    Err(legacy_error)
                }
            }
        }
    }
}

// Standard OOXML packages may use data descriptors and can be substantially
// larger than the small fixtures used by the legacy parser. Keep this bound as
// a resource guard, not a compatibility cutoff; ZipArchive performs the
// structural validation before Office Oxide reads the package parts.
const MAX_EXTERNAL_DOCX_ARCHIVE_BYTES: usize = 512 * 1024 * 1024;
const MAX_EXTERNAL_DOCX_UNCOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_EXTERNAL_DOCX_PARTS: usize = 4_096;

fn is_rudra_legacy_package(package: &DocxPackage) -> bool {
    package.entries.iter().any(|entry| {
        entry.name == PART_DOCUMENT
            && matches!(&entry.data, FlatOpcData::Xml(xml) if xml.contains(RUDRA_NS))
    })
}

fn is_bounded_standard_docx(bytes: &[u8]) -> bool {
    if bytes.len() > MAX_EXTERNAL_DOCX_ARCHIVE_BYTES || !bytes.starts_with(b"PK\x03\x04") {
        return false;
    }

    match panic::catch_unwind(AssertUnwindSafe(|| preflight_standard_docx(bytes))) {
        Ok(result) => result.unwrap_or(false),
        Err(_) => false,
    }
}

fn preflight_standard_docx(bytes: &[u8]) -> Result<bool, zip::result::ZipError> {
    let cursor = IoCursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)?;
    if archive.len() > MAX_EXTERNAL_DOCX_PARTS {
        return Ok(false);
    }

    let mut total_uncompressed = 0_u64;
    let mut has_content_types = false;
    let mut has_document = false;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let name = entry.name();
        if !opc::zip_name_is_safe(name) {
            return Ok(false);
        }
        total_uncompressed = match total_uncompressed.checked_add(entry.size()) {
            Some(total) if total <= MAX_EXTERNAL_DOCX_UNCOMPRESSED_BYTES => total,
            _ => return Ok(false),
        };
        has_content_types |= name == PART_CONTENT_TYPES;
        has_document |= name == PART_DOCUMENT;
    }
    Ok(has_content_types && has_document)
}

/// Convert Writer's editable subset into Office Oxide IR for standard OOXML output.
///
/// This is deliberately separate from the legacy `rw:*` serializer below. The public
/// exporter always takes this path, while the legacy format remains readable so files
/// created by earlier RudraOffice builds still open.
fn snapshot_to_office_oxide_ir(
    document: &DocumentSnapshot,
    options: &DocxExportOptions,
) -> Result<DocumentIR, DocxError> {
    let mut ir = DocumentIR::default();
    ir.metadata.format = OfficeOxideFormat::Docx;
    ir.metadata.author = options.author.clone();
    ir.metadata.title = options.title.clone();
    ir.metadata.description = options.description.clone();
    ir.sections.push(office_oxide::ir::Section {
        elements: writer_blocks_to_ir(&document.blocks)?,
        ..Default::default()
    });
    Ok(ir)
}

fn writer_blocks_to_ir(blocks: &[BlockSnapshot]) -> Result<Vec<IrElement>, DocxError> {
    let mut elements = Vec::with_capacity(blocks.len());
    let mut index = 0usize;
    while index < blocks.len() {
        if let BlockSnapshot::Text(block) = &blocks[index]
            && let BlockKind::ListItem { list_id, kind } = &block.kind
        {
            let ordered = matches!(kind, ListKind::Numbered);
            let mut items = Vec::new();
            while index < blocks.len() {
                let BlockSnapshot::Text(candidate) = &blocks[index] else {
                    break;
                };
                let BlockKind::ListItem {
                    list_id: candidate_list_id,
                    kind: candidate_kind,
                } = &candidate.kind
                else {
                    break;
                };
                if candidate_list_id != list_id || candidate_kind != kind {
                    break;
                }
                items.push(IrListItem {
                    content: vec![IrElement::Paragraph(writer_text_block_to_paragraph(
                        candidate,
                    ))],
                    nested: None,
                });
                index += 1;
            }
            elements.push(IrElement::List(IrList {
                ordered,
                items,
                ..Default::default()
            }));
            continue;
        }

        elements.push(writer_block_to_ir(&blocks[index])?);
        index += 1;
    }
    Ok(elements)
}

fn writer_block_to_ir(block: &BlockSnapshot) -> Result<IrElement, DocxError> {
    match block {
        BlockSnapshot::Text(value) => match &value.kind {
            BlockKind::Heading(level) => Ok(IrElement::Heading(IrHeading {
                level: level.get(),
                content: writer_runs_to_ir_or_text(&value.runs, &value.text),
                alignment: Some(writer_alignment(value.paragraph_style.alignment())),
                ..Default::default()
            })),
            BlockKind::Paragraph | BlockKind::ListItem { .. } => {
                Ok(IrElement::Paragraph(writer_text_block_to_paragraph(value)))
            }
            BlockKind::PageBreak => Ok(IrElement::PageBreak),
        },
        BlockSnapshot::Table(value) => Ok(IrElement::Table(writer_table_to_ir(value))),
        BlockSnapshot::Image(value) => Ok(IrElement::Image(writer_image_to_ir(value)?)),
    }
}

fn writer_text_block_to_paragraph(block: &TextBlockSnapshot) -> IrParagraph {
    IrParagraph {
        content: writer_runs_to_ir_or_text(&block.runs, &block.text),
        alignment: Some(writer_alignment(block.paragraph_style.alignment())),
        indent_left_twips: Some(half_points_to_twips(
            block.paragraph_style.indent_left_half_points(),
        )),
        indent_right_twips: Some(half_points_to_twips(
            block.paragraph_style.indent_right_half_points(),
        )),
        first_line_indent_twips: Some(half_points_to_twips(
            block.paragraph_style.first_line_indent_half_points(),
        )),
        space_before_twips: Some(
            block
                .paragraph_style
                .space_before_half_points()
                .saturating_mul(10),
        ),
        space_after_twips: Some(
            block
                .paragraph_style
                .space_after_half_points()
                .saturating_mul(10),
        ),
        line_spacing: Some(writer_line_spacing(block.paragraph_style.line_spacing())),
        ..Default::default()
    }
}

fn writer_runs_to_ir(runs: &[InlineRun]) -> Vec<IrInlineContent> {
    let mut content = Vec::new();
    for run in runs {
        let mut lines = run.text.split('\n').peekable();
        while let Some(line) = lines.next() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if !line.is_empty() {
                content.push(IrInlineContent::Text(writer_text_span(
                    run,
                    line.to_string(),
                )));
            }
            if lines.peek().is_some() {
                content.push(IrInlineContent::LineBreak);
            }
        }
    }
    content
}

fn writer_runs_to_ir_or_text(runs: &[InlineRun], text: &str) -> Vec<IrInlineContent> {
    if runs.is_empty() && !text.is_empty() {
        return vec![IrInlineContent::Text(IrTextSpan::plain(text))];
    }
    writer_runs_to_ir(runs)
}

fn writer_text_span(run: &InlineRun, text: String) -> IrTextSpan {
    let style = &run.style;
    IrTextSpan {
        text,
        bold: style.bold(),
        italic: style.italic(),
        strikethrough: style.strikethrough(),
        font_size_half_pt: Some(u32::from(style.size_half_points())),
        color: style.foreground().map(rgba_rgb),
        underline: style.underline().then_some(IrUnderlineStyle::Single),
        font_name: style.font_family().map(ToOwned::to_owned),
        highlight: style.background().map(rgba_rgb),
        ..Default::default()
    }
}

fn writer_alignment(value: Alignment) -> IrParagraphAlignment {
    match value {
        Alignment::Left => IrParagraphAlignment::Left,
        Alignment::Center => IrParagraphAlignment::Center,
        Alignment::Right => IrParagraphAlignment::Right,
        Alignment::Justified => IrParagraphAlignment::Justify,
    }
}

fn writer_line_spacing(value: LineSpacing) -> IrLineSpacing {
    match value {
        LineSpacing::Single => IrLineSpacing::Auto(240),
        LineSpacing::OneAndHalf => IrLineSpacing::Auto(360),
        LineSpacing::Double => IrLineSpacing::Auto(480),
        LineSpacing::Multiple(percentage) => {
            IrLineSpacing::Multiple((u32::from(percentage).saturating_mul(240) / 100).max(1))
        }
    }
}

fn half_points_to_twips(value: i32) -> i32 {
    value.saturating_mul(10)
}

fn writer_table_to_ir(table: &TableSnapshot) -> IrTable {
    let rows = table
        .rows
        .iter()
        .map(|row| IrTableRow {
            cells: row
                .cells
                .iter()
                .map(|cell| {
                    let (vertical_align, text_align) = writer_cell_alignment(cell.alignment);
                    IrTableCell {
                        content: vec![IrElement::Paragraph(IrParagraph {
                            content: writer_runs_to_ir_or_text(&cell.runs, &cell.text),
                            ..Default::default()
                        })],
                        col_span: 1,
                        row_span: 1,
                        background_color: cell.style.background.map(rgba_rgb),
                        border: writer_borders_to_ir(cell.style.borders),
                        vertical_align,
                        text_align,
                        ..Default::default()
                    }
                })
                .collect(),
            ..Default::default()
        })
        .collect();
    IrTable {
        rows,
        column_widths_twips: table
            .columns
            .iter()
            .map(|column| column.width_half_points.saturating_mul(10))
            .collect(),
        ..Default::default()
    }
}

fn writer_cell_alignment(
    value: CellAlignment,
) -> (Option<IrCellVerticalAlign>, Option<IrParagraphAlignment>) {
    match value {
        CellAlignment::Left => (None, Some(IrParagraphAlignment::Left)),
        CellAlignment::Center => (None, Some(IrParagraphAlignment::Center)),
        CellAlignment::Right => (None, Some(IrParagraphAlignment::Right)),
        CellAlignment::Top => (Some(IrCellVerticalAlign::Top), None),
        CellAlignment::Middle => (Some(IrCellVerticalAlign::Center), None),
        CellAlignment::Bottom => (Some(IrCellVerticalAlign::Bottom), None),
    }
}

fn writer_borders_to_ir(borders: Borders) -> Option<IrTableBorder> {
    let border = IrTableBorder {
        top: writer_border_to_ir(borders.top),
        right: writer_border_to_ir(borders.right),
        bottom: writer_border_to_ir(borders.bottom),
        left: writer_border_to_ir(borders.left),
        inside_h: None,
        inside_v: None,
    };
    if border.top.is_none()
        && border.right.is_none()
        && border.bottom.is_none()
        && border.left.is_none()
    {
        None
    } else {
        Some(border)
    }
}

fn writer_border_to_ir(border: Border) -> Option<IrBorderLine> {
    let style = match border.style {
        BorderStyle::None => return None,
        BorderStyle::Solid => IrBorderStyle::Single,
        BorderStyle::Dashed => IrBorderStyle::Dashed,
        BorderStyle::Dotted => IrBorderStyle::Dotted,
    };
    Some(IrBorderLine {
        style,
        color: Some(rgba_rgb(border.color)),
        size: Some(u32::from(border.width_half_points.max(1)).saturating_mul(4)),
        space: None,
    })
}

fn writer_image_to_ir(image: &ImageSnapshot) -> Result<IrImage, DocxError> {
    let format = match image.payload.format {
        ImageFormat::Png => IrImageFormat::Png,
        ImageFormat::Jpeg => IrImageFormat::Jpeg,
        ImageFormat::WebP => {
            return Err(DocxError::new(
                DocxErrorKind::UnsupportedPart,
                "standard DOCX export does not support WebP images; convert the image to PNG or JPEG",
            ));
        }
    };
    let width_emu = u64::from(image.layout.size.width)
        .checked_mul(EXTERNAL_IMAGE_EMU_PER_PIXEL)
        .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "image width overflow"))?;
    let height_emu = u64::from(image.layout.size.height)
        .checked_mul(EXTERNAL_IMAGE_EMU_PER_PIXEL)
        .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "image height overflow"))?;
    Ok(IrImage {
        data: Some(image.payload.bytes.clone()),
        format: Some(format),
        display_width_emu: Some(width_emu),
        display_height_emu: Some(height_emu),
        pixel_width: Some(image.payload.dimensions.width),
        pixel_height: Some(image.payload.dimensions.height),
        ..Default::default()
    })
}

fn rgba_rgb(value: Rgba8) -> [u8; 3] {
    [value.red(), value.green(), value.blue()]
}

/// Import a standard external DOCX through the real Office Oxide parser.
///
/// The writer model currently projects unknown rich OOXML content to its safe
/// editable plain-text representation. Rudra-generated documents retain their
/// richer in-house round-trip path above, identified by the `urn:rudra:writer`
/// marker in `word/document.xml`.
fn import_with_office_oxide_layout(bytes: &[u8]) -> Result<DocxImportResult, DocxError> {
    with_office_oxide_panic_boundary(|| {
        let document = OfficeOxideDocument::from_reader(
            IoCursor::new(bytes.to_vec()),
            OfficeOxideFormat::Docx,
        )
        .map_err(|error| {
            DocxError::new(
                DocxErrorKind::InvalidPackage,
                format!("office-oxide could not read DOCX: {error}"),
            )
        })?;
        let ir = document.to_ir();
        let mut projected = OfficeOxideProjector::new().project_with_diagnostics(&ir)?;
        apply_office_oxide_table_grid_widths(&document, &mut projected.snapshot);
        apply_office_oxide_paragraph_properties(&document, &mut projected.snapshot);
        let sections = ir
            .sections
            .iter()
            .map(|section| {
                let setup = section.page_setup.clone().unwrap_or_default();
                let text = |content: &Option<office_oxide::ir::HeaderFooter>| -> String {
                    content
                        .as_ref()
                        .and_then(|value| elements_to_runs(&value.content).ok())
                        .map(|runs| {
                            runs.into_iter()
                                .map(|run| run.text)
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default()
                };
                DocxSectionLayout {
                    width_twips: setup.width_twips,
                    height_twips: setup.height_twips,
                    margin_top_twips: setup.margin_top_twips,
                    margin_right_twips: setup.margin_right_twips,
                    margin_bottom_twips: setup.margin_bottom_twips,
                    margin_left_twips: setup.margin_left_twips,
                    header_distance_twips: setup.header_distance_twips,
                    footer_distance_twips: setup.footer_distance_twips,
                    landscape: setup.landscape,
                    header_text: text(&section.header),
                    footer_text: text(&section.footer),
                    first_page_header_text: text(&section.first_page_header),
                    first_page_footer_text: text(&section.first_page_footer),
                    even_page_header_text: text(&section.even_page_header),
                    even_page_footer_text: text(&section.even_page_footer),
                }
            })
            .collect();
        Ok(DocxImportResult {
            snapshot: projected.snapshot,
            layout: DocxLayoutMetadata { sections },
            projection: projected.diagnostics,
        })
    })
}
fn import_with_office_oxide(bytes: &[u8]) -> Result<DocumentSnapshot, DocxError> {
    with_office_oxide_panic_boundary(|| {
        let document = OfficeOxideDocument::from_reader(
            IoCursor::new(bytes.to_vec()),
            OfficeOxideFormat::Docx,
        )
        .map_err(|error| {
            DocxError::new(
                DocxErrorKind::InvalidPackage,
                format!("office-oxide could not read DOCX: {error}"),
            )
        })?;

        let ir = document.to_ir();
        let mut projected = OfficeOxideProjector::new().project_with_diagnostics(&ir)?;
        apply_office_oxide_table_grid_widths(&document, &mut projected.snapshot);
        apply_office_oxide_paragraph_properties(&document, &mut projected.snapshot);
        Ok(projected.snapshot)
    })
}

/// Copy the Office Oxide table grid into Writer's existing column model.
/// The source is already parsed by Office Oxide; this only preserves its
/// physical column proportions for Writer layout and drawing.
fn apply_office_oxide_table_grid_widths(
    document: &OfficeOxideDocument,
    snapshot: &mut DocumentSnapshot,
) {
    let Some(docx) = document.as_docx() else {
        return;
    };
    let mut grids = docx
        .body
        .elements
        .iter()
        .filter_map(|element| match element {
            office_oxide::docx::BlockElement::Table(table) => Some(&table.grid),
            office_oxide::docx::BlockElement::Paragraph(_) => None,
        });
    for block in &mut snapshot.blocks {
        let BlockSnapshot::Table(table) = block else {
            continue;
        };
        let Some(grid) = grids.next() else {
            break;
        };
        if grid.len() != table.columns.len() || grid.iter().any(|width| width.0 <= 0) {
            continue;
        }
        for (column, width) in table.columns.iter_mut().zip(grid) {
            column.width_half_points = twips_to_half_points(width.0 as u32);
        }
    }
}

/// Copy paragraph geometry from Office Oxide's public DOCX model into the
/// existing Writer paragraph style. `DocumentIR` intentionally normalizes
/// some pPr data, while this model retains the parsed values; no OOXML parser
/// is introduced here and vendor code remains untouched.
fn apply_office_oxide_paragraph_properties(
    document: &OfficeOxideDocument,
    snapshot: &mut DocumentSnapshot,
) {
    let Some(docx) = document.as_docx() else {
        return;
    };
    let mut paragraphs = docx
        .body
        .elements
        .iter()
        .filter_map(|element| match element {
            office_oxide::docx::BlockElement::Paragraph(paragraph) => Some(paragraph),
            office_oxide::docx::BlockElement::Table(_) => None,
        });
    for block in &mut snapshot.blocks {
        let BlockSnapshot::Text(text) = block else {
            continue;
        };
        let Some(source) = paragraphs.next() else {
            break;
        };
        let Some(properties) = source.properties.as_ref() else {
            continue;
        };
        let style = &text.paragraph_style;
        let indent = properties.indent.as_ref();
        let left = indent
            .and_then(|value| value.left.as_ref())
            .map(|value| signed_twips_to_half_points(value.0))
            .unwrap_or_else(|| style.indent_left_half_points());
        let right = indent
            .and_then(|value| value.right.as_ref())
            .map(|value| signed_twips_to_half_points(value.0))
            .unwrap_or_else(|| style.indent_right_half_points());
        let first_line = indent
            .and_then(|value| value.first_line.as_ref())
            .map(|value| signed_twips_to_half_points(value.0))
            .or_else(|| {
                indent
                    .and_then(|value| value.hanging.as_ref())
                    .map(|value| signed_twips_to_half_points(value.0).saturating_neg())
            })
            .unwrap_or_else(|| style.first_line_indent_half_points());
        let spacing = properties.spacing.as_ref();
        let before = spacing
            .and_then(|value| value.before.as_ref())
            .map(|value| twips_to_half_points(value.0.max(0) as u32))
            .unwrap_or_else(|| style.space_before_half_points());
        let after = spacing
            .and_then(|value| value.after.as_ref())
            .map(|value| twips_to_half_points(value.0.max(0) as u32))
            .unwrap_or_else(|| style.space_after_half_points());
        text.paragraph_style = style
            .clone()
            .with_indents(left, right, first_line)
            .with_spacing(before, after);
    }
}

fn signed_twips_to_half_points(value: i32) -> i32 {
    value / 10
}
fn with_office_oxide_panic_boundary<R, F>(f: F) -> Result<R, DocxError>
where
    F: FnOnce() -> Result<R, DocxError>,
{
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(_) => Err(DocxError::new(
            DocxErrorKind::InvalidPackage,
            "office-oxide panicked while importing DOCX",
        )),
    }
}

#[cfg(test)]
mod office_oxide_guard_tests {
    use super::*;

    #[test]
    fn office_oxide_panic_is_mapped_to_invalid_package() {
        let result: Result<DocumentSnapshot, DocxError> =
            with_office_oxide_panic_boundary(|| panic!("external office-oxide panic"));
        assert_eq!(
            result,
            Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                "office-oxide panicked while importing DOCX",
            ))
        );
    }
}

const EXTERNAL_IMAGE_EMU_PER_PIXEL: u64 = 9_525;
const EXTERNAL_LIST_INDENT_HALF_POINTS: i32 = 72;

/// Converts the real Office Oxide IR into the editable Writer snapshot.
///
/// Writer deliberately has a smaller model than WordprocessingML. The common,
/// editable subset (paragraphs, headings, lists, simple tables, run styles and
/// PNG/JPEG images) is mapped structurally. Features that Writer cannot model
/// are retained as readable fallback text instead of being silently discarded.
struct OfficeOxideProjector {
    limits: ImportLimits,
    next_block: u64,
    next_table: u64,
    next_row: u64,
    next_column: u64,
    next_cell: u64,
    next_image: u64,
    next_list: u64,
    block_count: usize,
    table_count: usize,
    fallback: DocxFallbackCounts,
}

struct OfficeOxideProjection {
    snapshot: DocumentSnapshot,
    diagnostics: DocxProjectionDiagnostics,
}

#[derive(Clone, Copy)]
enum DocxFallbackKind {
    ThematicBreak,
    ColumnBreak,
    TextBox,
    Footnote,
    Endnote,
    CodeBlock,
    Shape,
    UnsupportedImage,
    IrregularTable,
    OtherElement,
}

impl OfficeOxideProjector {
    fn new() -> Self {
        Self {
            limits: ImportLimits::default(),
            next_block: 1,
            next_table: 1,
            next_row: 1,
            next_column: 1,
            next_cell: 1,
            next_image: 1,
            next_list: 1,
            block_count: 0,
            table_count: 0,
            fallback: DocxFallbackCounts::default(),
        }
    }

    #[cfg(test)]
    fn project(self, document: &DocumentIR) -> Result<DocumentSnapshot, DocxError> {
        Ok(self.project_with_diagnostics(document)?.snapshot)
    }

    fn project_with_diagnostics(
        mut self,
        document: &DocumentIR,
    ) -> Result<OfficeOxideProjection, DocxError> {
        let source = source_content_counts(document);
        let mut blocks = Vec::new();
        for section in &document.sections {
            self.project_elements(&section.elements, &mut blocks)?;
        }
        if blocks.is_empty() {
            self.push_text(
                &mut blocks,
                BlockKind::Paragraph,
                ParagraphStyle::default(),
                Vec::new(),
            )?;
        }
        let snapshot = DocumentSnapshot {
            blocks,
            revision: Revision::new(0),
        };
        snapshot.validate().map_err(|error| {
            DocxError::new(
                DocxErrorKind::SchemaViolation,
                format!("could not project DOCX into Writer: {error}"),
            )
        })?;
        let mut projected = projected_content_counts(&snapshot);
        projected.fallback_blocks = self.fallback.total();
        Ok(OfficeOxideProjection {
            snapshot,
            diagnostics: DocxProjectionDiagnostics {
                source,
                projected,
                fallback: self.fallback,
            },
        })
    }

    fn project_elements(
        &mut self,
        elements: &[IrElement],
        blocks: &mut Vec<BlockSnapshot>,
    ) -> Result<(), DocxError> {
        for element in elements {
            match element {
                IrElement::Heading(heading) => {
                    let level = HeadingLevel::new(heading.level.clamp(1, 6)).map_err(|error| {
                        DocxError::new(
                            DocxErrorKind::SchemaViolation,
                            format!("invalid DOCX heading level: {error}"),
                        )
                    })?;
                    let paragraph_style = heading
                        .alignment
                        .as_ref()
                        .map(paragraph_alignment)
                        .map(|alignment| ParagraphStyle::default().with_alignment(alignment))
                        .unwrap_or_default();
                    self.push_text(
                        blocks,
                        BlockKind::Heading(level),
                        paragraph_style,
                        inline_runs(&heading.content)?,
                    )?;
                }
                IrElement::Paragraph(paragraph) => {
                    self.push_text(
                        blocks,
                        BlockKind::Paragraph,
                        paragraph_style(paragraph),
                        inline_runs(&paragraph.content)?,
                    )?;
                }
                IrElement::Table(table) => self.project_table(table, blocks)?,
                IrElement::List(list) => self.project_list(list, 0, blocks)?,
                IrElement::Image(image) => self.project_image(image, blocks)?,
                IrElement::ThematicBreak => self.push_fallback_text(
                    blocks,
                    "[Horizontal rule]".to_string(),
                    DocxFallbackKind::ThematicBreak,
                )?,
                IrElement::PageBreak => self.push_text(
                    blocks,
                    BlockKind::PageBreak,
                    ParagraphStyle::default(),
                    Vec::new(),
                )?,
                IrElement::ColumnBreak => self.push_fallback_text(
                    blocks,
                    "[Column break]".to_string(),
                    DocxFallbackKind::ColumnBreak,
                )?,
                IrElement::TextBox(_) => self.push_fallback_text(
                    blocks,
                    fallback_text(element),
                    DocxFallbackKind::TextBox,
                )?,
                IrElement::Footnote(_) => self.push_fallback_text(
                    blocks,
                    fallback_text(element),
                    DocxFallbackKind::Footnote,
                )?,
                IrElement::Endnote(_) => self.push_fallback_text(
                    blocks,
                    fallback_text(element),
                    DocxFallbackKind::Endnote,
                )?,
                IrElement::CodeBlock(_) => self.push_fallback_text(
                    blocks,
                    fallback_text(element),
                    DocxFallbackKind::CodeBlock,
                )?,
                IrElement::Shape(_) => self.push_fallback_text(
                    blocks,
                    fallback_text(element),
                    DocxFallbackKind::Shape,
                )?,
                _ => self.push_fallback_text(
                    blocks,
                    fallback_text(element),
                    DocxFallbackKind::OtherElement,
                )?,
            }
        }
        Ok(())
    }

    fn project_list(
        &mut self,
        list: &IrList,
        additional_depth: u8,
        blocks: &mut Vec<BlockSnapshot>,
    ) -> Result<(), DocxError> {
        let list_id = ListId::new(take_identifier(&mut self.next_list, "list")?);
        let kind = if list.ordered {
            ListKind::Numbered
        } else {
            ListKind::Bullet
        };
        let depth = list.level.saturating_add(additional_depth);
        let indent = i32::from(depth)
            .saturating_mul(EXTERNAL_LIST_INDENT_HALF_POINTS)
            .clamp(0, i32::MAX);

        for item in &list.items {
            let mut runs = self.elements_to_runs(&item.content)?;
            if runs.is_empty() {
                runs.push(InlineRun::new("", TextStyle::default()));
            }
            self.push_text(
                blocks,
                BlockKind::ListItem { list_id, kind },
                ParagraphStyle::default().with_indents(indent, 0, 0),
                runs,
            )?;
            if let Some(nested) = &item.nested {
                self.project_list(nested, additional_depth.saturating_add(1), blocks)?;
            }
        }
        Ok(())
    }

    fn project_table(
        &mut self,
        table: &IrTable,
        blocks: &mut Vec<BlockSnapshot>,
    ) -> Result<(), DocxError> {
        let Some((column_count, grid_rows)) = simple_table_grid(table) else {
            return self.push_fallback_text(
                blocks,
                format!(
                    "[Table with merged or irregular cells was imported as text]\n{}",
                    table_plain_text(table)
                ),
                DocxFallbackKind::IrregularTable,
            );
        };
        if column_count > self.limits.max_columns_per_table {
            return Err(DocxError::new(
                DocxErrorKind::SchemaViolation,
                format!(
                    "table column count exceeds limit: {column_count} > {}",
                    self.limits.max_columns_per_table
                ),
            ));
        }
        if table.rows.len() > self.limits.max_rows_per_table {
            return Err(DocxError::new(
                DocxErrorKind::SchemaViolation,
                format!(
                    "table row count exceeds limit: {} > {}",
                    table.rows.len(),
                    self.limits.max_rows_per_table
                ),
            ));
        }
        let cell_count = table.rows.len().checked_mul(column_count).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "table cell count overflow")
        })?;
        if cell_count > self.limits.max_cells_per_table {
            return Err(DocxError::new(
                DocxErrorKind::SchemaViolation,
                format!(
                    "table cell count exceeds limit: {cell_count} > {}",
                    self.limits.max_cells_per_table
                ),
            ));
        }
        self.table_count = self.table_count.checked_add(1).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "table count overflow")
        })?;
        if self.table_count > self.limits.max_tables {
            return Err(DocxError::new(
                DocxErrorKind::SchemaViolation,
                format!(
                    "table count exceeds limit: {} > {}",
                    self.table_count, self.limits.max_tables
                ),
            ));
        }

        let table_id = TableId::new(take_identifier(&mut self.next_table, "table")?);
        let mut columns = Vec::with_capacity(column_count);
        for index in 0..column_count {
            columns.push(TableColumnSnapshot {
                id: ColumnId::new(take_identifier(&mut self.next_column, "table column")?),
                width_half_points: table
                    .column_widths_twips
                    .get(index)
                    .copied()
                    .filter(|width| *width > 0)
                    .map(twips_to_half_points)
                    .unwrap_or(1_440),
            });
        }

        let mut rows = Vec::with_capacity(grid_rows.len());
        for grid_row in grid_rows {
            let mut cells = Vec::with_capacity(column_count);
            for cell in grid_row {
                let (text, runs, style, alignment) = if let Some(cell) = cell {
                    let runs = self.elements_to_runs(&cell.content)?;
                    self.validate_runs(&runs, "table cell")?;
                    let text = runs.iter().map(|run| run.text.as_str()).collect();
                    let border = cell.border.as_ref().or(table.border.as_ref());
                    (
                        text,
                        runs,
                        CellStyle {
                            background: cell.background_color.map(rgb),
                            borders: table_borders(border),
                        },
                        cell_alignment(cell),
                    )
                } else {
                    (
                        String::new(),
                        Vec::new(),
                        CellStyle::default(),
                        CellAlignment::default(),
                    )
                };
                cells.push(TableCellSnapshot {
                    id: CellId::new(take_identifier(&mut self.next_cell, "table cell")?),
                    text,
                    runs,
                    style,
                    alignment,
                });
            }
            rows.push(TableRowSnapshot {
                id: RowId::new(take_identifier(&mut self.next_row, "table row")?),
                cells,
            });
        }
        self.push_block(
            blocks,
            BlockSnapshot::Table(TableSnapshot {
                id: table_id,
                columns,
                rows,
            }),
        )
    }

    fn project_image(
        &mut self,
        image: &IrImage,
        blocks: &mut Vec<BlockSnapshot>,
    ) -> Result<(), DocxError> {
        let Some(data) = image.data.as_ref() else {
            return self.push_fallback_text(
                blocks,
                image_fallback_text(image),
                DocxFallbackKind::UnsupportedImage,
            );
        };
        let format = match image.format.as_ref() {
            Some(IrImageFormat::Png) => ImageFormat::Png,
            Some(IrImageFormat::Jpeg) => ImageFormat::Jpeg,
            _ => {
                return self.push_fallback_text(
                    blocks,
                    image_fallback_text(image),
                    DocxFallbackKind::UnsupportedImage,
                );
            }
        };
        let dimensions = PixelSize::new(
            image_pixel_size(image, true),
            image_pixel_size(image, false),
        )
        .map_err(|error| {
            DocxError::new(
                DocxErrorKind::SchemaViolation,
                format!("invalid imported image dimensions: {error}"),
            )
        })?;
        let payload = ImagePayload::new(format, dimensions, data.clone()).map_err(|error| {
            DocxError::new(
                DocxErrorKind::SchemaViolation,
                format!("invalid imported image payload: {error}"),
            )
        })?;
        let image_id = ImageId::new(take_identifier(&mut self.next_image, "image")?);
        self.push_block(
            blocks,
            BlockSnapshot::Image(ImageSnapshot {
                id: image_id,
                payload,
                layout: ImageLayout {
                    size: dimensions,
                    ..ImageLayout::default()
                },
            }),
        )
    }

    fn push_fallback_text(
        &mut self,
        blocks: &mut Vec<BlockSnapshot>,
        text: String,
        kind: DocxFallbackKind,
    ) -> Result<(), DocxError> {
        self.record_fallback(kind);
        self.push_text(
            blocks,
            BlockKind::Paragraph,
            ParagraphStyle::default(),
            vec![InlineRun::new(text, TextStyle::default())],
        )
    }

    fn elements_to_runs(&mut self, elements: &[IrElement]) -> Result<Vec<InlineRun>, DocxError> {
        let mut result = Vec::new();
        for element in elements {
            let runs = match element {
                IrElement::Heading(heading) => inline_runs(&heading.content)?,
                IrElement::Paragraph(paragraph) => inline_runs(&paragraph.content)?,
                _ => {
                    self.record_fallback(DocxFallbackKind::OtherElement);
                    vec![InlineRun::new(fallback_text(element), TextStyle::default())]
                }
            };
            if !runs.is_empty() && !result.is_empty() {
                result.push(InlineRun::new("\n", TextStyle::default()));
            }
            result.extend(runs);
        }
        Ok(result)
    }

    fn record_fallback(&mut self, kind: DocxFallbackKind) {
        let counter = match kind {
            DocxFallbackKind::ThematicBreak => &mut self.fallback.thematic_breaks,
            DocxFallbackKind::ColumnBreak => &mut self.fallback.column_breaks,
            DocxFallbackKind::TextBox => &mut self.fallback.text_boxes,
            DocxFallbackKind::Footnote => &mut self.fallback.footnotes,
            DocxFallbackKind::Endnote => &mut self.fallback.endnotes,
            DocxFallbackKind::CodeBlock => &mut self.fallback.code_blocks,
            DocxFallbackKind::Shape => &mut self.fallback.shapes,
            DocxFallbackKind::UnsupportedImage => &mut self.fallback.unsupported_images,
            DocxFallbackKind::IrregularTable => &mut self.fallback.irregular_tables,
            DocxFallbackKind::OtherElement => &mut self.fallback.other_elements,
        };
        *counter = counter.saturating_add(1);
    }

    fn push_text(
        &mut self,
        blocks: &mut Vec<BlockSnapshot>,
        kind: BlockKind,
        paragraph_style: ParagraphStyle,
        runs: Vec<InlineRun>,
    ) -> Result<(), DocxError> {
        self.validate_runs(&runs, "text block")?;
        let text = runs.iter().map(|run| run.text.as_str()).collect();
        let block_id = BlockId::new(take_identifier(&mut self.next_block, "text block")?);
        self.push_block(
            blocks,
            BlockSnapshot::Text(TextBlockSnapshot {
                id: block_id,
                kind,
                paragraph_style,
                text,
                runs,
            }),
        )
    }

    fn push_block(
        &mut self,
        blocks: &mut Vec<BlockSnapshot>,
        block: BlockSnapshot,
    ) -> Result<(), DocxError> {
        self.block_count = self.block_count.checked_add(1).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "block count overflow")
        })?;
        if self.block_count > self.limits.max_blocks {
            return Err(DocxError::new(
                DocxErrorKind::SchemaViolation,
                format!(
                    "block count exceeds limit: {} > {}",
                    self.block_count, self.limits.max_blocks
                ),
            ));
        }
        blocks.push(block);
        Ok(())
    }

    fn validate_runs(&self, runs: &[InlineRun], context: &str) -> Result<(), DocxError> {
        if runs.len() > self.limits.max_runs_per_block {
            return Err(DocxError::new(
                DocxErrorKind::SchemaViolation,
                format!(
                    "{context} run count exceeds limit: {} > {}",
                    runs.len(),
                    self.limits.max_runs_per_block
                ),
            ));
        }
        for run in runs {
            if run.text.len() > self.limits.max_run_text_len {
                return Err(DocxError::new(
                    DocxErrorKind::SchemaViolation,
                    format!(
                        "{context} run text exceeds limit: {} > {}",
                        run.text.len(),
                        self.limits.max_run_text_len
                    ),
                ));
            }
        }
        Ok(())
    }
}

fn source_content_counts(document: &DocumentIR) -> DocxSourceContentCounts {
    let mut counts = DocxSourceContentCounts::default();
    for section in &document.sections {
        count_source_elements(&section.elements, &mut counts);
    }
    counts
}

fn count_source_elements(elements: &[IrElement], counts: &mut DocxSourceContentCounts) {
    for element in elements {
        match element {
            IrElement::Paragraph(_) => counts.paragraphs = counts.paragraphs.saturating_add(1),
            IrElement::Heading(_) => counts.headings = counts.headings.saturating_add(1),
            IrElement::List(list) => {
                counts.lists = counts.lists.saturating_add(1);
                for item in &list.items {
                    count_source_elements(&item.content, counts);
                    if let Some(nested) = &item.nested {
                        count_source_list(nested, counts);
                    }
                }
            }
            IrElement::Table(table) => {
                counts.tables = counts.tables.saturating_add(1);
                for row in &table.rows {
                    counts.table_cells = counts.table_cells.saturating_add(row.cells.len());
                    for cell in &row.cells {
                        count_source_elements(&cell.content, counts);
                    }
                }
            }
            IrElement::Image(_) => counts.images = counts.images.saturating_add(1),
            IrElement::ThematicBreak => {
                counts.thematic_breaks = counts.thematic_breaks.saturating_add(1)
            }
            IrElement::PageBreak => counts.page_breaks = counts.page_breaks.saturating_add(1),
            IrElement::ColumnBreak => counts.column_breaks = counts.column_breaks.saturating_add(1),
            IrElement::TextBox(_) => counts.text_boxes = counts.text_boxes.saturating_add(1),
            IrElement::Footnote(_) => counts.footnotes = counts.footnotes.saturating_add(1),
            IrElement::Endnote(_) => counts.endnotes = counts.endnotes.saturating_add(1),
            IrElement::CodeBlock(_) => counts.code_blocks = counts.code_blocks.saturating_add(1),
            IrElement::Shape(_) => counts.shapes = counts.shapes.saturating_add(1),
            _ => counts.other_elements = counts.other_elements.saturating_add(1),
        }
    }
}

fn count_source_list(list: &IrList, counts: &mut DocxSourceContentCounts) {
    counts.lists = counts.lists.saturating_add(1);
    for item in &list.items {
        count_source_elements(&item.content, counts);
        if let Some(nested) = &item.nested {
            count_source_list(nested, counts);
        }
    }
}

fn projected_content_counts(snapshot: &DocumentSnapshot) -> DocxProjectedContentCounts {
    let mut counts = DocxProjectedContentCounts::default();
    for block in &snapshot.blocks {
        match block {
            BlockSnapshot::Text(text) => {
                counts.text_blocks = counts.text_blocks.saturating_add(1);
                if matches!(&text.kind, BlockKind::ListItem { .. }) {
                    counts.list_items = counts.list_items.saturating_add(1);
                }
                if matches!(&text.kind, BlockKind::PageBreak) {
                    counts.page_break_blocks = counts.page_break_blocks.saturating_add(1);
                }
            }
            BlockSnapshot::Table(table) => {
                counts.table_blocks = counts.table_blocks.saturating_add(1);
                for row in &table.rows {
                    counts.table_cells = counts.table_cells.saturating_add(row.cells.len());
                }
            }
            BlockSnapshot::Image(_) => counts.image_blocks = counts.image_blocks.saturating_add(1),
        }
    }
    counts
}

fn take_identifier(next: &mut u64, label: &str) -> Result<u64, DocxError> {
    let identifier = *next;
    *next = next.checked_add(1).ok_or_else(|| {
        DocxError::new(
            DocxErrorKind::SchemaViolation,
            format!("{label} identifier overflow"),
        )
    })?;
    Ok(identifier)
}

fn inline_runs(content: &[IrInlineContent]) -> Result<Vec<InlineRun>, DocxError> {
    let mut runs = Vec::new();
    for item in content {
        match item {
            IrInlineContent::Text(span) => {
                if !span.text.is_empty() {
                    runs.push(InlineRun::new(span.text.clone(), text_style(span)?));
                }
            }
            IrInlineContent::LineBreak => {
                runs.push(InlineRun::new("\n", TextStyle::default()));
            }
            _ => runs.push(InlineRun::new("[Note reference]", TextStyle::default())),
        }
    }
    Ok(runs)
}

fn text_style(span: &office_oxide::ir::TextSpan) -> Result<TextStyle, DocxError> {
    let underline = !matches!(span.underline.as_ref(), Some(IrUnderlineStyle::None) | None);
    let mut style = TextStyle::default()
        .with_font_family(span.font_name.clone())
        .with_bold(span.bold)
        .with_italic(span.italic)
        .with_underline(underline)
        .with_strikethrough(span.strikethrough)
        .with_foreground(span.color.map(rgb))
        .with_background(span.highlight.map(rgb));
    if let Some(size) = span.font_size_half_pt {
        let safe_size = size.clamp(1, 16_384) as u16;
        style = style.with_size_half_points(safe_size).map_err(|error| {
            DocxError::new(
                DocxErrorKind::SchemaViolation,
                format!("invalid imported text style: {error}"),
            )
        })?;
    }
    Ok(style)
}

fn paragraph_style(paragraph: &IrParagraph) -> ParagraphStyle {
    let alignment = paragraph
        .alignment
        .as_ref()
        .map(paragraph_alignment)
        .unwrap_or_default();
    let line_spacing = paragraph
        .line_spacing
        .as_ref()
        .map(line_spacing)
        .unwrap_or_default();
    ParagraphStyle::default()
        .with_alignment(alignment)
        .with_line_spacing(line_spacing)
        .with_indents(
            paragraph.indent_left_twips.unwrap_or_default() / 10,
            paragraph.indent_right_twips.unwrap_or_default() / 10,
            paragraph.first_line_indent_twips.unwrap_or_default() / 10,
        )
        .with_spacing(
            paragraph.space_before_twips.unwrap_or_default() / 10,
            paragraph.space_after_twips.unwrap_or_default() / 10,
        )
}

fn paragraph_alignment(value: &IrParagraphAlignment) -> Alignment {
    match value {
        IrParagraphAlignment::Left => Alignment::Left,
        IrParagraphAlignment::Center => Alignment::Center,
        IrParagraphAlignment::Right => Alignment::Right,
        IrParagraphAlignment::Justify | IrParagraphAlignment::Distribute => Alignment::Justified,
    }
}

fn line_spacing(value: &IrLineSpacing) -> LineSpacing {
    let raw = match value {
        IrLineSpacing::Auto(value)
        | IrLineSpacing::Multiple(value)
        | IrLineSpacing::Exact(value)
        | IrLineSpacing::AtLeast(value) => *value,
    };
    let percentage = ((u64::from(raw).saturating_mul(100)) / 240).clamp(1, u64::from(u16::MAX));
    match percentage {
        100 => LineSpacing::Single,
        150 => LineSpacing::OneAndHalf,
        200 => LineSpacing::Double,
        value => LineSpacing::Multiple(value as u16),
    }
}

fn elements_to_runs(elements: &[IrElement]) -> Result<Vec<InlineRun>, DocxError> {
    let mut result = Vec::new();
    for element in elements {
        let runs = match element {
            IrElement::Heading(heading) => inline_runs(&heading.content)?,
            IrElement::Paragraph(paragraph) => inline_runs(&paragraph.content)?,
            _ => vec![InlineRun::new(fallback_text(element), TextStyle::default())],
        };
        if !runs.is_empty() && !result.is_empty() {
            result.push(InlineRun::new("\n", TextStyle::default()));
        }
        result.extend(runs);
    }
    Ok(result)
}

/// Normalize a rectangular Office Oxide table into Writer's existing table grid.
///
/// Writer cells do not currently carry span metadata. A simple vertical merge is
/// therefore represented by empty continuation slots while keeping every row and
/// column in the document grid. Tables with horizontal merges or other irregular
/// geometry remain on the established readable-text fallback path.
fn simple_table_grid(table: &IrTable) -> Option<(usize, Vec<Vec<Option<&IrTableCell>>>)> {
    let mut column_count = 0_usize;
    for row in &table.rows {
        let mut row_width = 0_usize;
        for cell in &row.cells {
            let span = usize::try_from(cell.col_span.max(1)).ok()?;
            row_width = row_width.checked_add(span)?;
        }
        column_count = column_count.max(row_width);
    }
    if column_count == 0 {
        return None;
    }

    let mut remaining_rows = vec![0_u32; column_count];
    let mut grid_rows = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        let continued = remaining_rows
            .iter()
            .map(|remaining| *remaining > 0)
            .collect::<Vec<_>>();
        for remaining in &mut remaining_rows {
            *remaining = remaining.saturating_sub(1);
        }

        let mut grid = vec![None; column_count];
        let mut column = 0_usize;
        for cell in &row.cells {
            while column < column_count && continued[column] {
                column += 1;
            }
            let span = usize::try_from(cell.col_span.max(1)).ok()?;
            if span != 1 || column >= column_count || continued[column] {
                return None;
            }
            grid[column] = Some(cell);
            remaining_rows[column] = cell.row_span.max(1).saturating_sub(1);
            column += 1;
        }
        if grid
            .iter()
            .enumerate()
            .any(|(index, cell)| cell.is_none() && !continued[index])
        {
            return None;
        }
        grid_rows.push(grid);
    }
    Some((column_count, grid_rows))
}

fn table_plain_text(table: &IrTable) -> String {
    table
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| {
                    elements_to_runs(&cell.content)
                        .map(|runs| runs.into_iter().map(|run| run.text).collect())
                        .unwrap_or_else(|_| "[Unreadable table cell]".to_string())
                })
                .collect::<Vec<String>>()
                .join("\t")
        })
        .collect::<Vec<String>>()
        .join("\n")
}

fn table_borders(border: Option<&IrTableBorder>) -> Borders {
    let Some(border) = border else {
        return Borders::default();
    };
    Borders {
        top: border_line(border.top.as_ref()),
        right: border_line(border.right.as_ref()),
        bottom: border_line(border.bottom.as_ref()),
        left: border_line(border.left.as_ref()),
    }
}

fn border_line(line: Option<&IrBorderLine>) -> Border {
    let Some(line) = line else {
        return Border::default();
    };
    let style = match &line.style {
        IrBorderStyle::None => BorderStyle::None,
        IrBorderStyle::Dotted => BorderStyle::Dotted,
        IrBorderStyle::Dashed | IrBorderStyle::DashSmallGap => BorderStyle::Dashed,
        _ => BorderStyle::Solid,
    };
    if style == BorderStyle::None {
        return Border::default();
    }
    Border {
        style,
        width_half_points: line
            .size
            .map(|size| ((size.saturating_add(3)) / 4).clamp(1, u32::from(u16::MAX)) as u16)
            .unwrap_or(1),
        color: line.color.map(rgb).unwrap_or_else(|| Rgba8::rgb(0, 0, 0)),
    }
}

fn cell_alignment(cell: &IrTableCell) -> CellAlignment {
    if let Some(vertical) = cell.vertical_align.as_ref() {
        return match vertical {
            IrCellVerticalAlign::Top => CellAlignment::Top,
            IrCellVerticalAlign::Center => CellAlignment::Middle,
            IrCellVerticalAlign::Bottom => CellAlignment::Bottom,
        };
    }
    match cell.text_align.as_ref() {
        Some(IrParagraphAlignment::Center) => CellAlignment::Center,
        Some(IrParagraphAlignment::Right) => CellAlignment::Right,
        _ => CellAlignment::Left,
    }
}

fn image_pixel_size(image: &IrImage, width: bool) -> u32 {
    let pixels = if width {
        image.pixel_width.or_else(|| {
            image
                .display_width_emu
                .map(|value| value / EXTERNAL_IMAGE_EMU_PER_PIXEL)
                .map(|value| value.max(1))
                .and_then(|value| u32::try_from(value).ok())
        })
    } else {
        image.pixel_height.or_else(|| {
            image
                .display_height_emu
                .map(|value| value / EXTERNAL_IMAGE_EMU_PER_PIXEL)
                .map(|value| value.max(1))
                .and_then(|value| u32::try_from(value).ok())
        })
    };
    pixels.unwrap_or(1)
}

fn image_fallback_text(image: &IrImage) -> String {
    image
        .alt_text
        .as_deref()
        .map(|text| format!("[Unsupported image: {text}]"))
        .unwrap_or_else(|| "[Unsupported image]".to_string())
}

fn fallback_text(element: &IrElement) -> String {
    let mut document = DocumentIR::default();
    document.sections.push(office_oxide::ir::Section {
        elements: vec![element.clone()],
        ..Default::default()
    });
    let text = document.plain_text();
    if text.trim().is_empty() {
        "[Unsupported DOCX element]".to_string()
    } else {
        text
    }
}

fn rgb(value: [u8; 3]) -> Rgba8 {
    Rgba8::rgb(value[0], value[1], value[2])
}

fn twips_to_half_points(value: u32) -> u32 {
    ((value.saturating_add(5)) / 10).max(1)
}
/// Export the historical Rudra ZIP representation for compatibility with files
/// produced before the standard OOXML writer was introduced. New callers should use
/// `DocxExporter`, which emits standard DOCX packages through Office Oxide.
pub fn export_flat_opc(document: &DocumentSnapshot) -> Result<Vec<u8>, DocxError> {
    document.validate()?;
    let package = make_zip_package(document, &DocxExportOptions::default())?;
    opc::serialize_zip(&package)
}

pub fn import_flat_opc(bytes: &[u8]) -> Result<DocumentSnapshot, DocxError> {
    DocxImporter.import(bytes)
}

#[derive(Clone, Copy)]
struct ImportLimits {
    max_document_xml_bytes: usize,
    max_rels_xml_bytes: usize,
    max_rels_count: usize,
    max_media_aggregate_bytes: usize,
    max_blocks: usize,
    max_tables: usize,
    max_runs_per_block: usize,
    max_run_text_len: usize,
    max_attr_len: usize,
    max_columns_per_table: usize,
    max_rows_per_table: usize,
    max_cells_per_table: usize,
    max_aggregate_cells: usize,
}

impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_document_xml_bytes: 32 * 1024 * 1024,
            max_rels_xml_bytes: 2 * 1024 * 1024,
            max_rels_count: 4096,
            max_media_aggregate_bytes: 128 * 1024 * 1024,
            max_blocks: 65_536,
            max_tables: 4096,
            max_runs_per_block: 4096,
            max_run_text_len: 65_536,
            max_attr_len: 4096,
            max_columns_per_table: 256,
            max_rows_per_table: 65_536,
            max_cells_per_table: 16 * 1024 * 1024,
            max_aggregate_cells: 64 * 1024 * 1024,
        }
    }
}

fn enforce_package_limits(package: &DocxPackage, limits: ImportLimits) -> Result<(), DocxError> {
    let mut rels_xml_len = 0usize;
    let mut rels_count = 0usize;
    let mut media_len = 0usize;

    for entry in &package.entries {
        match &entry.data {
            FlatOpcData::Xml(s) => {
                if entry.name == PART_DOCUMENT && s.len() > limits.max_document_xml_bytes {
                    return Err(DocxError::new(
                        DocxErrorKind::InvalidPackage,
                        format!(
                            "document xml exceeds limit: {} > {}",
                            s.len(),
                            limits.max_document_xml_bytes
                        ),
                    ));
                }
                if entry.name.ends_with(".rels") || entry.content_type == RELATIONSHIPS {
                    enforce_relationship_limits(s, &mut rels_xml_len, &mut rels_count, limits)?;
                }
            }
            FlatOpcData::Binary(b) => {
                if entry.name.starts_with("word/media/") {
                    let len = b.len();
                    media_len = media_len.checked_add(len).ok_or_else(|| {
                        DocxError::new(DocxErrorKind::InvalidPackage, "media overflow")
                    })?;
                    if media_len > limits.max_media_aggregate_bytes {
                        return Err(DocxError::new(
                            DocxErrorKind::InvalidPackage,
                            format!(
                                "media aggregate exceeds limit: {} > {}",
                                media_len, limits.max_media_aggregate_bytes
                            ),
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn enforce_relationship_limits(
    xml: &str,
    rels_xml_len: &mut usize,
    rels_count: &mut usize,
    limits: ImportLimits,
) -> Result<(), DocxError> {
    *rels_xml_len = rels_xml_len
        .checked_add(xml.len())
        .ok_or_else(|| DocxError::new(DocxErrorKind::InvalidPackage, "rels xml overflow"))?;
    if *rels_xml_len > limits.max_rels_xml_bytes {
        return Err(DocxError::new(
            DocxErrorKind::InvalidPackage,
            format!(
                "relationship xml exceeds limit: {} > {}",
                *rels_xml_len, limits.max_rels_xml_bytes
            ),
        ));
    }

    let mut cursor = 0usize;
    while let Some(relative) = xml[cursor..].find("<Relationship") {
        let start = cursor
            .checked_add(relative)
            .ok_or_else(|| DocxError::new(DocxErrorKind::InvalidPackage, "rels offset overflow"))?;
        let after_name = start
            .checked_add("<Relationship".len())
            .ok_or_else(|| DocxError::new(DocxErrorKind::InvalidPackage, "rels offset overflow"))?;
        if !xml
            .as_bytes()
            .get(after_name)
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'>' || *byte == b'/')
        {
            cursor = after_name;
            continue;
        }
        *rels_count = rels_count
            .checked_add(1)
            .ok_or_else(|| DocxError::new(DocxErrorKind::InvalidPackage, "rels count overflow"))?;
        if *rels_count > limits.max_rels_count {
            return Err(DocxError::new(
                DocxErrorKind::InvalidPackage,
                format!(
                    "relationship count exceeds limit: {} > {}",
                    *rels_count, limits.max_rels_count
                ),
            ));
        }
        cursor = after_name;
    }
    Ok(())
}

fn make_zip_package(
    document: &DocumentSnapshot,
    options: &DocxExportOptions,
) -> Result<DocxPackage, DocxError> {
    let mut entries = Vec::new();
    let mut document_xml = String::new();
    writeln!(
        document_xml,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
    )
    .unwrap();
    writeln!(
        document_xml,
        r#"<w:document xmlns:w="{WORD_NS}" xmlns:rw="{RUDRA_NS}"><w:body>"#
    )
    .unwrap();

    let mut image_entries = Vec::new();
    let mut image_relationships = Vec::new();
    let mut image_index = 0usize;
    for block in &document.blocks {
        match block {
            BlockSnapshot::Text(value) => write_text_block(&mut document_xml, value),
            BlockSnapshot::Table(value) => write_table(&mut document_xml, value),
            BlockSnapshot::Image(value) => {
                let (element, entry, relationship) = write_image(value, image_index)?;
                document_xml.push_str(&element);
                image_entries.push(entry);
                image_relationships.push(relationship);
                image_index += 1;
            }
        }
    }
    writeln!(document_xml, "</w:body></w:document>").unwrap();

    let mut root_rels = String::new();
    writeln!(
        root_rels,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
    )
    .unwrap();
    writeln!(root_rels, r#"<Relationships xmlns="{REL_NS}"><Relationship Id="rIdOfficeDocument" Type="{REL_OFFICE_DOCUMENT}" Target="word/document.xml"/>"#).unwrap();
    if options.author.is_some() || options.title.is_some() || options.description.is_some() {
        writeln!(root_rels, r#"<Relationship Id="rIdCoreProperties" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>"#).unwrap();
    }
    writeln!(root_rels, "</Relationships>").unwrap();

    let mut document_rels = String::new();
    writeln!(
        document_rels,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
    )
    .unwrap();
    writeln!(document_rels, r#"<Relationships xmlns="{REL_NS}">"#).unwrap();
    for relationship in &image_relationships {
        writeln!(
            document_rels,
            r#"<Relationship Id="{}" Type="{}" Target="{}"/>"#,
            relationship.0, REL_IMAGE, relationship.1
        )
        .unwrap();
    }
    writeln!(document_rels, "</Relationships>").unwrap();

    let mut content_types = String::new();
    writeln!(
        content_types,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
    )
    .unwrap();
    writeln!(content_types, r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="{RELATIONSHIPS}"/><Default Extension="xml" ContentType="{XML}"/><Override PartName="/word/document.xml" ContentType="{DOCUMENT}"/><Override PartName="/word/_rels/document.xml.rels" ContentType="{RELATIONSHIPS}"/>"#).unwrap();
    for part in &image_entries {
        writeln!(
            content_types,
            r#"<Override PartName="/{}" ContentType="{}"/>"#,
            part.name, part.content_type
        )
        .unwrap();
    }
    if options.author.is_some() || options.title.is_some() || options.description.is_some() {
        writeln!(content_types, r#"<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>"#).unwrap();
    }
    writeln!(content_types, "</Types>").unwrap();

    entries.push(opc::ZipEntry {
        name: PART_CONTENT_TYPES.to_string(),
        content_type: CONTENT_TYPES.to_string(),
        data: FlatOpcData::Xml(content_types),
        compression: opc::ZipCompression::Stored,
    });
    entries.push(opc::ZipEntry {
        name: PART_RELS.to_string(),
        content_type: RELATIONSHIPS.to_string(),
        data: FlatOpcData::Xml(root_rels.clone()),
        compression: opc::ZipCompression::Stored,
    });
    entries.push(opc::ZipEntry {
        name: PART_DOCUMENT_RELS.to_string(),
        content_type: RELATIONSHIPS.to_string(),
        data: FlatOpcData::Xml(document_rels),
        compression: opc::ZipCompression::Stored,
    });
    entries.push(opc::ZipEntry {
        name: PART_DOCUMENT.to_string(),
        content_type: DOCUMENT.to_string(),
        data: FlatOpcData::Xml(document_xml),
        compression: opc::ZipCompression::Stored,
    });
    for entry in image_entries {
        entries.push(opc::ZipEntry {
            name: entry.name,
            content_type: entry.content_type,
            data: FlatOpcData::Binary(entry.data),
            compression: opc::ZipCompression::Stored,
        });
    }

    if options.author.is_some() || options.title.is_some() || options.description.is_some() {
        let mut core = String::new();
        writeln!(
            core,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(core, r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:creator>{}</dc:creator><dc:title>{}</dc:title><dc:description>{}</dc:description></cp:coreProperties>"#,
            xml_escape(options.author.as_deref().unwrap_or("")),
            xml_escape(options.title.as_deref().unwrap_or("")),
            xml_escape(options.description.as_deref().unwrap_or(""))).unwrap();
        entries.push(opc::ZipEntry {
            name: PART_METADATA.to_string(),
            content_type: "application/vnd.openxmlformats-package.core-properties+xml".to_string(),
            data: FlatOpcData::Xml(core),
            compression: opc::ZipCompression::Stored,
        });
    }

    Ok(DocxPackage { entries })
}

struct ImagePart {
    name: String,
    content_type: String,
    data: Vec<u8>,
}

fn write_image(
    image: &ImageSnapshot,
    index: usize,
) -> Result<(String, ImagePart, (String, String)), DocxError> {
    let (extension, content_type) = match image.payload.format {
        ImageFormat::Png => ("png", PNG),
        ImageFormat::Jpeg => ("jpeg", JPEG),
        ImageFormat::WebP => ("webp", WEBP),
    };
    let name = format!("word/media/image_{index}.{extension}");
    let relationship_id = format!("rIdImage{index}");
    let target = format!("media/image_{index}.{extension}");
    let element = format!(
        r#"<w:p rw:id="{}"><w:r><w:drawing><rw:image rw:id="{}" rw:relationship="{}" rw:width="{}" rw:height="{}"/></w:drawing></w:r></w:p>"#,
        image.id.get(),
        image.id.get(),
        relationship_id,
        image.layout.size.width,
        image.layout.size.height
    );
    Ok((
        element,
        ImagePart {
            name: name.clone(),
            content_type: content_type.to_string(),
            data: image.payload.bytes.clone(),
        },
        (relationship_id, target),
    ))
}

fn write_text_block(output: &mut String, block: &TextBlockSnapshot) {
    write!(
        output,
        r#"<w:p rw:id="{}" rw:kind="{}""#,
        block.id.get(),
        block_kind_name(&block.kind)
    )
    .unwrap();
    if let BlockKind::Heading(level) = block.kind {
        write!(output, r#" rw:heading="{}""#, level.get()).unwrap();
    }
    writeln!(output, ">").unwrap();
    for run in &block.runs {
        write!(output, r#"<w:r"#).unwrap();
        let style = &run.style;
        if let Some(font) = style.font_family() {
            write!(output, r#" rw:font="{}""#, xml_escape(font)).unwrap();
        }
        write!(
            output,
            r#" rw:sz="{}" rw:bold="{}" rw:italic="{}" rw:underline="{}" rw:strike="{}""#,
            style.size_half_points(),
            style.bold(),
            style.italic(),
            style.underline(),
            style.strikethrough()
        )
        .unwrap();
        if let Some(color) = style.foreground() {
            write!(
                output,
                r#" rw:fg="{:02X}{:02X}{:02X}{:02X}""#,
                color.r, color.g, color.b, color.a
            )
            .unwrap();
        }
        if let Some(color) = style.background() {
            write!(
                output,
                r#" rw:bg="{:02X}{:02X}{:02X}{:02X}""#,
                color.r, color.g, color.b, color.a
            )
            .unwrap();
        }
        writeln!(
            output,
            r#"><w:t xml:space="preserve">{}</w:t></w:r>"#,
            xml_escape(&run.text)
        )
        .unwrap();
    }
    if block.runs.is_empty() {
        output.push_str("<w:r><w:t/></w:r>");
    }
    output.push_str("</w:p>");
}

fn block_kind_name(kind: &BlockKind) -> &'static str {
    match kind {
        BlockKind::Paragraph => "paragraph",
        BlockKind::PageBreak => "page-break",
        BlockKind::Heading(_) => "heading",
        BlockKind::ListItem {
            kind: ListKind::Bullet,
            ..
        } => "bullet",
        BlockKind::ListItem {
            kind: ListKind::Numbered,
            ..
        } => "numbered",
    }
}

fn write_table(output: &mut String, table: &TableSnapshot) {
    write!(output, r#"<w:tbl rw:id="{}"><w:tblGrid>"#, table.id.get()).unwrap();
    for column in &table.columns {
        write!(
            output,
            r#"<w:gridCol rw:id="{}" w:w="{}"/>"#,
            column.id.get(),
            column.width_half_points
        )
        .unwrap();
    }
    output.push_str("</w:tblGrid>");
    for row in &table.rows {
        write!(output, r#"<w:tr rw:id="{}">"#, row.id.get()).unwrap();
        for cell in &row.cells {
            write!(output, r#"<w:tc rw:id="{}""#, cell.id.get()).unwrap();
            if let Some(color) = cell.style.background {
                write!(
                    output,
                    r#" rw:bg="{:02X}{:02X}{:02X}{:02X}""#,
                    color.r, color.g, color.b, color.a
                )
                .unwrap();
            }
            writeln!(output, ">").unwrap();
            if cell.runs.is_empty() {
                writeln!(
                    output,
                    r#"<w:p><w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#,
                    xml_escape(&cell.text)
                )
                .unwrap();
            } else {
                writeln!(
                    output,
                    r#"<w:tbl>{}</w:tbl>"#,
                    cell.runs
                        .iter()
                        .map(|run| format!(
                            r#"<w:r><w:t xml:space="preserve">{}</w:t></w:r>"#,
                            xml_escape(&run.text)
                        ))
                        .collect::<String>()
                )
                .unwrap();
            }
            output.push_str("</w:tc>");
        }
        output.push_str("</w:tr>");
    }
    output.push_str("</w:tbl>");
}

/// Parse image relationships from word/_rels/document.xml.rels.
/// Returns a mapping from relationship ID to media file path.
fn parse_image_rels(
    rels_xml: &str,
    limits: ImportLimits,
) -> Result<std::collections::HashMap<String, String>, DocxError> {
    validate_attribute_lengths(rels_xml, limits.max_attr_len)?;
    let mut map = std::collections::HashMap::new();
    let mut cursor = 0usize;
    while let Some(rel_start) = rels_xml[cursor..].find("<Relationship") {
        let abs = cursor.checked_add(rel_start).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "rels offset overflow")
        })?;
        let Some(rel_end) = rels_xml[abs..].find("/>") else {
            break;
        };
        let rel_end_abs = abs.checked_add(rel_end).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "rels offset overflow")
        })?;
        let rel_xml = rels_xml
            .get(abs..rel_end_abs + 2)
            .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "rels slice overflow"))?;
        cursor = rel_end_abs.checked_add(2).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "rels offset overflow")
        })?;

        let id = match attr_limited(rel_xml, "Id", limits.max_attr_len)? {
            Some(v) => v,
            None => continue,
        };
        let target = match attr_limited(rel_xml, "Target", limits.max_attr_len)? {
            Some(v) => v,
            None => continue,
        };
        let rel_type = match attr_limited(rel_xml, "Type", limits.max_attr_len)? {
            Some(v) => v,
            None => continue,
        };

        if rel_type == REL_IMAGE {
            // Resolve target relative to word/ directory
            let resolved = if target.starts_with('/') {
                target.trim_start_matches('/').to_string()
            } else if target.starts_with("word/") {
                target.to_string()
            } else {
                format!("word/{}", target)
            };
            map.insert(id, resolved);
        }
    }
    Ok(map)
}

fn read_document(
    package: &DocxPackage,
    options: DocxImportOptions,
) -> Result<DocumentSnapshot, DocxError> {
    read_document_with_limits(package, options, ImportLimits::default())
}

fn read_document_with_limits(
    package: &DocxPackage,
    options: DocxImportOptions,
    limits: ImportLimits,
) -> Result<DocumentSnapshot, DocxError> {
    enforce_package_limits(package, limits)?;

    let entry = package
        .entries
        .iter()
        .find(|entry| entry.name == PART_DOCUMENT)
        .ok_or_else(|| {
            DocxError::new(DocxErrorKind::MissingPart, "word/document.xml is required")
        })?;
    let xml = match &entry.data {
        FlatOpcData::Xml(s) => s,
        FlatOpcData::Binary(_) => {
            return Err(DocxError::new(
                DocxErrorKind::XmlDecode,
                "document part is not XML",
            ));
        }
    };

    validate_attribute_lengths(xml, limits.max_attr_len)?;

    // Parse image relationships from word/_rels/document.xml.rels
    let image_rels = package
        .entries
        .iter()
        .find(|entry| entry.name == PART_DOCUMENT_RELS)
        .and_then(|entry| match &entry.data {
            FlatOpcData::Xml(s) => Some(parse_image_rels(s, limits)),
            FlatOpcData::Binary(_) => None,
        })
        .transpose()?
        .unwrap_or_default();

    // Build a borrowed index from media file path to package bytes.
    let media_map: std::collections::HashMap<String, &[u8]> = package
        .entries
        .iter()
        .filter(|entry| entry.name.starts_with("word/media/"))
        .filter_map(|entry| match &entry.data {
            FlatOpcData::Binary(bytes) => Some((entry.name.clone(), bytes.as_slice())),
            FlatOpcData::Xml(_) => None,
        })
        .collect();

    let mut aggregate_cells = 0usize;
    let mut blocks = parse_blocks_with_limits(
        xml,
        options,
        &image_rels,
        &media_map,
        limits,
        &mut aggregate_cells,
    )?;

    // Assign deterministic unique block IDs to paragraphs lacking rw:id.
    // In strict mode, duplicate explicit IDs are rejected.
    // In non-strict mode, duplicate IDs are remapped to unique values in document order.
    let mut next_id: u64 = 1;
    let mut seen_ids = std::collections::BTreeSet::new();
    for block in &mut blocks {
        let current_id = block.id();
        if current_id == 0 {
            // Missing rw:id: assign a unique deterministic ID
            while seen_ids.contains(&next_id) {
                next_id += 1;
            }
            let new_id = BlockId::new(next_id);
            next_id += 1;
            *block = replace_block_id(block, new_id);
            seen_ids.insert(new_id.get());
        } else if !seen_ids.insert(current_id) {
            // Duplicate explicit ID
            if options.strict {
                return Err(DocxError::new(
                    DocxErrorKind::DuplicateBlock,
                    format!("duplicate block id {}", current_id),
                ));
            }
            // Non-strict: remap to a unique deterministic ID
            while seen_ids.contains(&next_id) {
                next_id += 1;
            }
            let new_id = BlockId::new(next_id);
            next_id += 1;
            *block = replace_block_id(block, new_id);
            seen_ids.insert(new_id.get());
        }
    }

    if blocks.is_empty() {
        return Err(DocxError::new(
            DocxErrorKind::SchemaViolation,
            "document body has no supported blocks",
        ));
    }

    // Validate final snapshot
    let snapshot = DocumentSnapshot {
        blocks,
        revision: Revision::new(0),
    };
    snapshot.validate().map_err(DocxError::from)?;

    Ok(snapshot)
}

#[cfg(test)]
fn parse_blocks(
    xml: &str,
    options: DocxImportOptions,
    image_rels: &std::collections::HashMap<String, String>,
    media_map: &std::collections::HashMap<String, Vec<u8>>,
) -> Result<Vec<BlockSnapshot>, DocxError> {
    let borrowed: std::collections::HashMap<String, &[u8]> = media_map
        .iter()
        .map(|(k, v)| (k.clone(), v.as_slice()))
        .collect();
    let mut aggregate = 0usize;
    parse_blocks_with_limits(
        xml,
        options,
        image_rels,
        &borrowed,
        ImportLimits::default(),
        &mut aggregate,
    )
}

fn parse_blocks_with_limits(
    xml: &str,
    _options: DocxImportOptions,
    image_rels: &std::collections::HashMap<String, String>,
    media_map: &std::collections::HashMap<String, &[u8]>,
    limits: ImportLimits,
    aggregate_cells: &mut usize,
) -> Result<Vec<BlockSnapshot>, DocxError> {
    // Parse top-level body children in document order.
    // Table cell paragraphs (inside <w:tc>) are NOT promoted to top-level blocks.
    let body_start = xml
        .find("<w:body>")
        .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "missing <w:body>"))?;
    let body_content_start = body_start
        .checked_add("<w:body>".len())
        .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "body offset overflow"))?;
    let body_end = xml[body_content_start..]
        .find("</w:body>")
        .and_then(|relative| body_content_start.checked_add(relative))
        .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "missing </w:body>"))?;
    let body = xml
        .get(body_content_start..body_end)
        .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "invalid <w:body> bounds"))?;

    let mut blocks = Vec::new();
    let mut cursor = 0usize;
    let mut tc_depth = 0usize;
    let mut table_count = 0usize;

    while cursor < body.len() {
        let chunk = &body[cursor..];

        // Track <w:tc> nesting
        if let Some(tc_open) = chunk.find("<w:tc") {
            // Check if there's a <w:p> or <w:tbl> before this <w:tc>
            let before_tc = &chunk[..tc_open];
            if let Some(p_pos) = before_tc.find("<w:p") {
                let p_end = find_tag_end(body, cursor + p_pos, "<w:p>", "</w:p>")?;
                let para = &body[cursor + p_pos..p_end];
                if tc_depth == 0 {
                    if blocks.len() >= limits.max_blocks {
                        return Err(DocxError::new(
                            DocxErrorKind::SchemaViolation,
                            format!(
                                "block count exceeds limit: {} > {}",
                                blocks.len(),
                                limits.max_blocks
                            ),
                        ));
                    }
                    if let Some(block) =
                        parse_paragraph_with_limits(para, image_rels, media_map, limits)?
                    {
                        blocks.push(block);
                    }
                }
                cursor = p_end;
                continue;
            }
            if let Some(tbl_pos) = before_tc.find("<w:tbl") {
                let tbl_end = find_tag_end(body, cursor + tbl_pos, "<w:tbl>", "</w:tbl>")?;
                let tbl_xml = &body[cursor + tbl_pos..tbl_end];
                if tc_depth == 0 {
                    if blocks.len() >= limits.max_blocks {
                        return Err(DocxError::new(
                            DocxErrorKind::SchemaViolation,
                            format!(
                                "block count exceeds limit: {} > {}",
                                blocks.len(),
                                limits.max_blocks
                            ),
                        ));
                    }
                    if table_count >= limits.max_tables {
                        return Err(DocxError::new(
                            DocxErrorKind::SchemaViolation,
                            format!(
                                "table count exceeds limit: {} > {}",
                                table_count, limits.max_tables
                            ),
                        ));
                    }
                    let table = parse_table_with_limits(tbl_xml, limits, aggregate_cells)?;
                    table_count += 1;
                    blocks.push(BlockSnapshot::Table(table));
                }
                cursor = tbl_end;
                continue;
            }
            // Found <w:tc> before any <w:p> or <w:tbl> in this chunk
            tc_depth += 1;
            cursor += tc_open + "<w:tc".len();
            continue;
        }

        // Track </w:tc> closing
        if let Some(tc_close) = chunk.find("</w:tc>") {
            tc_depth = tc_depth.saturating_sub(1);
            cursor += tc_close + "</w:tc>".len();
            continue;
        }

        // No more tags found — process remaining as paragraph if it starts with <w:p
        if let Some(p_pos) = chunk.find("<w:p") {
            let p_end = find_tag_end(body, cursor + p_pos, "<w:p>", "</w:p>")?;
            let para = &body[cursor + p_pos..p_end];
            if tc_depth == 0 {
                if blocks.len() >= limits.max_blocks {
                    return Err(DocxError::new(
                        DocxErrorKind::SchemaViolation,
                        format!(
                            "block count exceeds limit: {} > {}",
                            blocks.len(),
                            limits.max_blocks
                        ),
                    ));
                }
                if let Some(block) =
                    parse_paragraph_with_limits(para, image_rels, media_map, limits)?
                {
                    blocks.push(block);
                }
            }
            cursor = p_end;
            continue;
        }

        // No more tags found
        break;
    }

    // Preserve original order — no sorting by id
    Ok(blocks)
}

fn is_exact_open_tag_at(xml: &str, start: usize, tag_prefix: &str) -> bool {
    let Some(remaining) = xml.get(start..) else {
        return false;
    };
    let Some(after_name) = remaining.strip_prefix(tag_prefix) else {
        return false;
    };
    matches!(
        after_name.as_bytes().first(),
        Some(b'>') | Some(b'/') | Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')
    )
}

fn find_exact_open_tag(xml: &str, search_from: usize, tag_prefix: &str) -> Option<usize> {
    let mut cursor = search_from;
    while let Some(relative) = xml.get(cursor..)?.find(tag_prefix) {
        let start = cursor.checked_add(relative)?;
        if is_exact_open_tag_at(xml, start, tag_prefix) {
            return Some(start);
        }
        cursor = start.checked_add(tag_prefix.len())?;
    }
    None
}

fn open_tag_end(xml: &str, start: usize, tag_prefix: &str) -> Result<usize, DocxError> {
    if !is_exact_open_tag_at(xml, start, tag_prefix) {
        return Err(DocxError::new(
            DocxErrorKind::SchemaViolation,
            "expected opening tag at requested offset",
        ));
    }
    xml.get(start..)
        .and_then(|tail| tail.find('>'))
        .and_then(|relative| start.checked_add(relative + 1))
        .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "unterminated opening tag"))
}

/// Find the end of a matching open/close tag pair while respecting nested tags
/// with the same name. Exact tag matching prevents `<w:tblGrid>` and `<w:tcPr>`
/// from being confused with `<w:tbl>` and `<w:tc>`.
fn find_tag_end(
    xml: &str,
    start: usize,
    open_tag: &str,
    close_tag: &str,
) -> Result<usize, DocxError> {
    let open_prefix = open_tag.strip_suffix('>').unwrap_or(open_tag);
    let mut depth = 1usize;
    let mut cursor = open_tag_end(xml, start, open_prefix)?;

    loop {
        let next_open = find_exact_open_tag(xml, cursor, open_prefix);
        let next_close = xml
            .get(cursor..)
            .and_then(|chunk| chunk.find(close_tag))
            .and_then(|relative| cursor.checked_add(relative));

        match (next_open, next_close) {
            (Some(open), Some(close)) if open < close => {
                depth = depth.checked_add(1).ok_or_else(|| {
                    DocxError::new(DocxErrorKind::SchemaViolation, "tag nesting overflow")
                })?;
                cursor = open_tag_end(xml, open, open_prefix)?;
            }
            (_, Some(close)) => {
                depth -= 1;
                let close_end = close.checked_add(close_tag.len()).ok_or_else(|| {
                    DocxError::new(DocxErrorKind::SchemaViolation, "tag offset overflow")
                })?;
                if depth == 0 {
                    return Ok(close_end);
                }
                cursor = close_end;
            }
            _ => {
                return Err(DocxError::new(
                    DocxErrorKind::SchemaViolation,
                    "unterminated tag",
                ));
            }
        }
    }
}
fn parse_paragraph_with_limits(
    xml: &str,
    image_rels: &std::collections::HashMap<String, String>,
    media_map: &std::collections::HashMap<String, &[u8]>,
    limits: ImportLimits,
) -> Result<Option<BlockSnapshot>, DocxError> {
    validate_attribute_lengths(xml, limits.max_attr_len)?;
    if xml.contains("<w:tbl") {
        return Ok(None);
    }

    // Check for image: <w:drawing><rw:image .../>
    if let Some(image_block) = parse_image_block(xml, image_rels, media_map, limits)? {
        return Ok(Some(image_block));
    }

    let id = attr_u64_limited(xml, "rw:id", limits.max_attr_len)?.unwrap_or(0);
    let kind = match attr_limited(xml, "rw:kind", limits.max_attr_len)?.as_deref() {
        Some("page-break") => BlockKind::PageBreak,
        Some("heading") => BlockKind::Heading(
            HeadingLevel::new(
                attr_u64_limited(xml, "rw:heading", limits.max_attr_len)?.unwrap_or(1) as u8,
            )
            .unwrap_or(HeadingLevel::new(1).unwrap()),
        ),
        Some("bullet") => BlockKind::ListItem {
            list_id: ListId::new(1),
            kind: ListKind::Bullet,
        },
        Some("numbered") => BlockKind::ListItem {
            list_id: ListId::new(1),
            kind: ListKind::Numbered,
        },
        _ => BlockKind::Paragraph,
    };
    let mut runs = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative) = xml[cursor..].find("<w:r") {
        let start = cursor
            .checked_add(relative)
            .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "run offset overflow"))?;
        let Some(end_relative) = xml[start..].find("</w:r>") else {
            break;
        };
        let end = start
            .checked_add(end_relative)
            .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "run offset overflow"))?;
        let end = end
            .checked_add("</w:r>".len())
            .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "run offset overflow"))?;
        let run_xml = xml
            .get(start..end)
            .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "run slice overflow"))?;
        if runs.len() >= limits.max_runs_per_block {
            return Err(DocxError::new(
                DocxErrorKind::SchemaViolation,
                format!(
                    "runs per block exceeds limit: {} > {}",
                    runs.len(),
                    limits.max_runs_per_block
                ),
            ));
        }
        let text =
            element_text_limited(run_xml, "w:t", limits.max_run_text_len)?.unwrap_or_default();
        if text.len() > limits.max_run_text_len {
            return Err(DocxError::new(
                DocxErrorKind::SchemaViolation,
                format!(
                    "run text exceeds limit: {} > {}",
                    text.len(),
                    limits.max_run_text_len
                ),
            ));
        }
        if !text.is_empty() || run_xml.contains("<w:t") {
            runs.push(InlineRun::new(
                text,
                parse_style_with_limits(run_xml, limits)?,
            ));
        }
        cursor = end;
    }
    let text = runs.iter().map(|run| run.text.as_str()).collect();
    Ok(Some(BlockSnapshot::Text(TextBlockSnapshot {
        id: BlockId::new(id),
        kind,
        paragraph_style: ParagraphStyle::default(),
        text,
        runs,
    })))
}

/// Try to parse a paragraph as an image block.
/// Looks for <w:drawing><rw:image rw:id="..." rw:relationship="..." rw:width="..." rw:height="..."/>
fn parse_image_block(
    xml: &str,
    image_rels: &std::collections::HashMap<String, String>,
    media_map: &std::collections::HashMap<String, &[u8]>,
    limits: ImportLimits,
) -> Result<Option<BlockSnapshot>, DocxError> {
    // Find <w:drawing> that contains <rw:image>
    let drawing_start = match xml.find("<w:drawing>") {
        Some(pos) => pos,
        None => return Ok(None),
    };

    let drawing_end = match xml[drawing_start..].find("</w:drawing>") {
        Some(pos) => drawing_start
            .checked_add(pos)
            .ok_or_else(|| {
                DocxError::new(DocxErrorKind::SchemaViolation, "drawing offset overflow")
            })?
            .checked_add("</w:drawing>".len())
            .ok_or_else(|| {
                DocxError::new(DocxErrorKind::SchemaViolation, "drawing offset overflow")
            })?,
        None => return Ok(None),
    };

    let drawing_xml = xml
        .get(drawing_start..drawing_end)
        .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "drawing slice overflow"))?;

    validate_attribute_lengths(drawing_xml, limits.max_attr_len)?;

    // Find <rw:image> within the drawing
    let image_start = match drawing_xml.find("<rw:image") {
        Some(pos) => drawing_start.checked_add(pos).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "image offset overflow")
        })?,
        None => return Ok(None),
    };

    let image_end = match xml[image_start..].find("/>") {
        Some(pos) => image_start
            .checked_add(pos)
            .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "image offset overflow"))?
            .checked_add(2)
            .ok_or_else(|| {
                DocxError::new(DocxErrorKind::SchemaViolation, "image offset overflow")
            })?,
        None => return Ok(None),
    };

    let image_xml = xml
        .get(image_start..image_end)
        .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "image slice overflow"))?;

    let rw_id = match attr_u64_limited(image_xml, "rw:id", limits.max_attr_len)? {
        Some(v) if v > 0 => v,
        _ => return Ok(None),
    };

    let relationship_id = match attr_limited(image_xml, "rw:relationship", limits.max_attr_len)? {
        Some(v) => v,
        None => return Ok(None),
    };

    let width = attr_u64_limited(image_xml, "rw:width", limits.max_attr_len)?.unwrap_or(0);
    let height = attr_u64_limited(image_xml, "rw:height", limits.max_attr_len)?.unwrap_or(0);

    // Resolve the media path from the relationship before using it.
    let media_path = match image_rels.get(&relationship_id) {
        Some(path) => path.clone(),
        None => return Ok(None),
    };

    // Determine image format from file extension before cloning bytes.
    let format = if media_path.ends_with(".png") {
        ImageFormat::Png
    } else if media_path.ends_with(".jpeg") || media_path.ends_with(".jpg") {
        ImageFormat::Jpeg
    } else if media_path.ends_with(".webp") {
        ImageFormat::WebP
    } else {
        return Ok(None);
    };

    // Get the media bytes via borrowed index; only clone bytes for the selected image.
    let media_bytes = match media_map.get(&media_path) {
        Some(bytes) => bytes.to_vec(),
        None => return Ok(None),
    };

    let image_id = ImageId::new(rw_id);
    let pixel_size = PixelSize::new(
        width.try_into().map_err(|_| {
            DocxError::new(
                DocxErrorKind::SchemaViolation,
                "image width exceeds u32 range",
            )
        })?,
        height.try_into().map_err(|_| {
            DocxError::new(
                DocxErrorKind::SchemaViolation,
                "image height exceeds u32 range",
            )
        })?,
    )
    .map_err(DocxError::from)?;
    let payload = ImagePayload::new(format, pixel_size, media_bytes).map_err(DocxError::from)?;

    let layout = ImageLayout {
        size: pixel_size,
        alignment: Alignment::Left,
        aspect_locked: true,
    };

    Ok(Some(BlockSnapshot::Image(ImageSnapshot {
        id: image_id,
        payload,
        layout,
    })))
}

fn parse_table_with_limits(
    xml: &str,
    limits: ImportLimits,
    aggregate_cells: &mut usize,
) -> Result<TableSnapshot, DocxError> {
    validate_attribute_lengths(xml, limits.max_attr_len)?;
    let id = attr_u64_limited(xml, "rw:id", limits.max_attr_len)?.unwrap_or(1);

    if !is_exact_open_tag_at(xml, 0, "<w:tbl") {
        return Err(DocxError::new(
            DocxErrorKind::SchemaViolation,
            "table XML must start at <w:tbl>",
        ));
    }
    let tbl_open_end = open_tag_end(xml, 0, "<w:tbl")?;
    let table_end = find_tag_end(xml, 0, "<w:tbl>", "</w:tbl>")?;
    let tbl_close_pos = table_end
        .checked_sub("</w:tbl>".len())
        .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "table offset overflow"))?;
    let content = xml.get(tbl_open_end..tbl_close_pos).ok_or_else(|| {
        DocxError::new(
            DocxErrorKind::SchemaViolation,
            "table content slice overflow",
        )
    })?;

    // Parse direct outer <w:gridCol> columns only; nested tables are skipped.
    let mut columns = Vec::new();
    let mut cursor = 0usize;
    let mut nested_tbl_depth = 0usize;
    while cursor < content.len() {
        let Some(relative) = content.get(cursor..).and_then(|chunk| chunk.find('<')) else {
            break;
        };
        let tag_start = cursor.checked_add(relative).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "table offset overflow")
        })?;
        let remaining = content.get(tag_start..).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "table offset overflow")
        })?;

        if is_exact_open_tag_at(content, tag_start, "<w:tbl") {
            nested_tbl_depth = nested_tbl_depth.checked_add(1).ok_or_else(|| {
                DocxError::new(DocxErrorKind::SchemaViolation, "table nesting overflow")
            })?;
            cursor = open_tag_end(content, tag_start, "<w:tbl")?;
            continue;
        }
        if remaining.starts_with("</w:tbl>") {
            nested_tbl_depth = nested_tbl_depth.saturating_sub(1);
            cursor = tag_start.checked_add("</w:tbl>".len()).ok_or_else(|| {
                DocxError::new(DocxErrorKind::SchemaViolation, "table offset overflow")
            })?;
            continue;
        }
        if nested_tbl_depth == 0 && is_exact_open_tag_at(content, tag_start, "<w:gridCol") {
            if columns.len() >= limits.max_columns_per_table {
                return Err(DocxError::new(
                    DocxErrorKind::SchemaViolation,
                    format!(
                        "columns per table exceeds limit: {} > {}",
                        columns.len(),
                        limits.max_columns_per_table
                    ),
                ));
            }
            let end = open_tag_end(content, tag_start, "<w:gridCol")?;
            let col_xml = content.get(tag_start..end).ok_or_else(|| {
                DocxError::new(DocxErrorKind::SchemaViolation, "column slice overflow")
            })?;
            columns.push(TableColumnSnapshot {
                id: ColumnId::new(
                    attr_u64_limited(col_xml, "rw:id", limits.max_attr_len)?
                        .unwrap_or(columns.len() as u64 + 1),
                ),
                width_half_points: attr_u64_limited(col_xml, "w:w", limits.max_attr_len)?
                    .unwrap_or(1440) as u32,
            });
            cursor = end;
            continue;
        }
        cursor = tag_start.checked_add(1).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "table offset overflow")
        })?;
    }

    // Parse direct outer rows only. A full matched row is skipped after parsing,
    // so nested tables cannot be mistaken for sibling rows.
    let mut rows = Vec::new();
    let mut table_cells = 0usize;
    cursor = 0;
    nested_tbl_depth = 0;
    while cursor < content.len() {
        let Some(relative) = content.get(cursor..).and_then(|chunk| chunk.find('<')) else {
            break;
        };
        let tag_start = cursor.checked_add(relative).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "table offset overflow")
        })?;
        let remaining = content.get(tag_start..).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "table offset overflow")
        })?;

        if is_exact_open_tag_at(content, tag_start, "<w:tbl") {
            nested_tbl_depth = nested_tbl_depth.checked_add(1).ok_or_else(|| {
                DocxError::new(DocxErrorKind::SchemaViolation, "table nesting overflow")
            })?;
            cursor = open_tag_end(content, tag_start, "<w:tbl")?;
            continue;
        }
        if remaining.starts_with("</w:tbl>") {
            nested_tbl_depth = nested_tbl_depth.saturating_sub(1);
            cursor = tag_start.checked_add("</w:tbl>".len()).ok_or_else(|| {
                DocxError::new(DocxErrorKind::SchemaViolation, "table offset overflow")
            })?;
            continue;
        }
        if nested_tbl_depth == 0 && is_exact_open_tag_at(content, tag_start, "<w:tr") {
            if rows.len() >= limits.max_rows_per_table {
                return Err(DocxError::new(
                    DocxErrorKind::SchemaViolation,
                    format!(
                        "rows per table exceeds limit: {} > {}",
                        rows.len(),
                        limits.max_rows_per_table
                    ),
                ));
            }
            let tr_end = find_tag_end(content, tag_start, "<w:tr>", "</w:tr>")?;
            rows.push(parse_row_with_counters(
                &content[tag_start..tr_end],
                rows.len() as u64 + 1,
                limits,
                &mut table_cells,
                aggregate_cells,
            )?);
            cursor = tr_end;
            continue;
        }
        cursor = tag_start.checked_add(1).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "table offset overflow")
        })?;
    }

    if columns.is_empty()
        || rows.is_empty()
        || rows.iter().any(|row| row.cells.len() != columns.len())
    {
        return Err(DocxError::new(
            DocxErrorKind::SchemaViolation,
            "table is not rectangular",
        ));
    }

    Ok(TableSnapshot {
        id: TableId::new(id),
        columns,
        rows,
    })
}
#[cfg(test)]
fn parse_row_with_limits(
    xml: &str,
    fallback_id: u64,
    limits: ImportLimits,
    aggregate_cells: &mut usize,
) -> Result<TableRowSnapshot, DocxError> {
    let mut table_cells = 0usize;
    parse_row_with_counters(xml, fallback_id, limits, &mut table_cells, aggregate_cells)
}

fn parse_row_with_counters(
    xml: &str,
    fallback_id: u64,
    limits: ImportLimits,
    table_cells: &mut usize,
    aggregate_cells: &mut usize,
) -> Result<TableRowSnapshot, DocxError> {
    validate_attribute_lengths(xml, limits.max_attr_len)?;
    let id = attr_u64_limited(xml, "rw:id", limits.max_attr_len)?.unwrap_or(fallback_id);
    let mut cells = Vec::new();
    let mut cursor = 0usize;
    let mut nested_tbl_depth = 0usize;

    while cursor < xml.len() {
        let Some(relative) = xml.get(cursor..).and_then(|chunk| chunk.find('<')) else {
            break;
        };
        let tag_start = cursor.checked_add(relative).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "table offset overflow")
        })?;
        let remaining = xml.get(tag_start..).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "table offset overflow")
        })?;

        if is_exact_open_tag_at(xml, tag_start, "<w:tbl") {
            nested_tbl_depth = nested_tbl_depth.checked_add(1).ok_or_else(|| {
                DocxError::new(DocxErrorKind::SchemaViolation, "table nesting overflow")
            })?;
            cursor = open_tag_end(xml, tag_start, "<w:tbl")?;
            continue;
        }
        if remaining.starts_with("</w:tbl>") {
            nested_tbl_depth = nested_tbl_depth.saturating_sub(1);
            cursor = tag_start.checked_add("</w:tbl>".len()).ok_or_else(|| {
                DocxError::new(DocxErrorKind::SchemaViolation, "table offset overflow")
            })?;
            continue;
        }
        if nested_tbl_depth == 0 && is_exact_open_tag_at(xml, tag_start, "<w:tc") {
            let tc_end = find_tag_end(xml, tag_start, "<w:tc>", "</w:tc>")?;
            let cell_xml = xml.get(tag_start..tc_end).ok_or_else(|| {
                DocxError::new(DocxErrorKind::SchemaViolation, "cell slice overflow")
            })?;
            let cell_open_end = open_tag_end(cell_xml, 0, "<w:tc")?;
            if find_exact_open_tag(cell_xml, cell_open_end, "<w:tbl").is_some() {
                return Err(DocxError::new(
                    DocxErrorKind::SchemaViolation,
                    "nested tables inside table cells are not supported",
                ));
            }
            let next_table_cells = table_cells.checked_add(1).ok_or_else(|| {
                DocxError::new(DocxErrorKind::SchemaViolation, "table cell count overflow")
            })?;
            if next_table_cells > limits.max_cells_per_table {
                return Err(DocxError::new(
                    DocxErrorKind::SchemaViolation,
                    format!(
                        "cells per table exceeds limit: {} > {}",
                        next_table_cells, limits.max_cells_per_table
                    ),
                ));
            }
            let next_aggregate = aggregate_cells.checked_add(1).ok_or_else(|| {
                DocxError::new(
                    DocxErrorKind::SchemaViolation,
                    "aggregate cell count overflow",
                )
            })?;
            if next_aggregate > limits.max_aggregate_cells {
                return Err(DocxError::new(
                    DocxErrorKind::SchemaViolation,
                    format!(
                        "aggregate cells exceeds limit: {} > {}",
                        next_aggregate, limits.max_aggregate_cells
                    ),
                ));
            }
            if cells.len() >= limits.max_columns_per_table {
                return Err(DocxError::new(
                    DocxErrorKind::SchemaViolation,
                    format!(
                        "cells per row exceeds limit: {} > {}",
                        cells.len(),
                        limits.max_columns_per_table
                    ),
                ));
            }
            let cell_id = attr_u64_limited(cell_xml, "rw:id", limits.max_attr_len)?
                .unwrap_or(cells.len() as u64 + 1);
            let text =
                element_text_limited(cell_xml, "w:t", limits.max_run_text_len)?.unwrap_or_default();
            let runs = if text.is_empty() {
                Vec::new()
            } else {
                vec![InlineRun::new(text.clone(), TextStyle::default())]
            };
            cells.push(TableCellSnapshot {
                id: CellId::new(cell_id),
                text,
                runs,
                style: CellStyle::default(),
                alignment: CellAlignment::default(),
            });
            *table_cells = next_table_cells;
            *aggregate_cells = next_aggregate;
            cursor = tc_end;
            continue;
        }
        cursor = tag_start.checked_add(1).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "table offset overflow")
        })?;
    }

    Ok(TableRowSnapshot {
        id: RowId::new(id),
        cells,
    })
}
fn parse_bool_limited(xml: &str, name: &str, max_len: usize) -> Result<bool, DocxError> {
    Ok(attr_limited(xml, name, max_len)?
        .and_then(|value| value.parse().ok())
        .unwrap_or(false))
}

fn parse_style_with_limits(xml: &str, limits: ImportLimits) -> Result<TextStyle, DocxError> {
    let mut style = TextStyle::default();
    style = style.with_bold(parse_bool_limited(xml, "rw:bold", limits.max_attr_len)?);
    style = style.with_italic(parse_bool_limited(xml, "rw:italic", limits.max_attr_len)?);
    style = style.with_underline(parse_bool_limited(
        xml,
        "rw:underline",
        limits.max_attr_len,
    )?);
    style = style.with_strikethrough(parse_bool_limited(xml, "rw:strike", limits.max_attr_len)?);
    if let Some(font) = attr_limited(xml, "rw:font", limits.max_attr_len)? {
        style = style.with_font_family(Some(font));
    }
    if let Some(value) = attr_limited(xml, "rw:sz", limits.max_attr_len)? {
        let value = value.parse::<u64>().map_err(|_| {
            DocxError::new(DocxErrorKind::SchemaViolation, "invalid size attribute")
        })?;
        style = style
            .with_size_half_points(value as u16)
            .map_err(DocxError::from)?;
    }
    if let Some(color) = attr_limited(xml, "rw:fg", limits.max_attr_len)? {
        style = style.with_foreground(parse_color(&color));
    }
    if let Some(color) = attr_limited(xml, "rw:bg", limits.max_attr_len)? {
        style = style.with_background(parse_color(&color));
    }
    Ok(style)
}

fn parse_color(value: &str) -> Option<Rgba8> {
    if !value.is_ascii() || value.len() != 8 {
        return None;
    }
    let parse = |part: &str| u8::from_str_radix(part, 16).ok();
    Some(Rgba8::new(
        parse(&value[0..2])?,
        parse(&value[2..4])?,
        parse(&value[4..6])?,
        parse(&value[6..8])?,
    ))
}

fn attr_limited(xml: &str, name: &str, max_len: usize) -> Result<Option<String>, DocxError> {
    let needle = format!(r#"{name}="#);
    let Some(start) = xml
        .find(&needle)
        .and_then(|value| value.checked_add(needle.len()))
    else {
        return Ok(None);
    };
    let rest = xml
        .get(start..)
        .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "attribute offset overflow"))?
        .strip_prefix('"')
        .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "attribute is not quoted"))?;
    let end = rest
        .find('"')
        .ok_or_else(|| DocxError::new(DocxErrorKind::SchemaViolation, "unterminated attribute"))?;
    let raw = &rest[..end];
    if raw.len() > max_len {
        return Err(DocxError::new(
            DocxErrorKind::SchemaViolation,
            format!(
                "attribute length exceeds limit: {} > {}",
                raw.len(),
                max_len
            ),
        ));
    }
    let value = xml_unescape(raw);
    Ok(Some(value))
}

fn validate_attribute_lengths(xml: &str, max_len: usize) -> Result<(), DocxError> {
    let mut cursor = 0usize;
    while let Some(relative) = xml[cursor..].find('=') {
        let equals = cursor.checked_add(relative).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "attribute offset overflow")
        })?;
        let mut value_start = equals.checked_add(1).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "attribute offset overflow")
        })?;
        while xml
            .as_bytes()
            .get(value_start)
            .is_some_and(u8::is_ascii_whitespace)
        {
            value_start = value_start.checked_add(1).ok_or_else(|| {
                DocxError::new(DocxErrorKind::SchemaViolation, "attribute offset overflow")
            })?;
        }
        let Some(&quote) = xml.as_bytes().get(value_start) else {
            break;
        };
        if quote != b'"' && quote != b'\'' {
            cursor = value_start;
            continue;
        }
        let value_end = xml[value_start + 1..]
            .find(quote as char)
            .and_then(|relative| value_start.checked_add(relative + 1))
            .ok_or_else(|| {
                DocxError::new(DocxErrorKind::SchemaViolation, "unterminated attribute")
            })?;
        let raw = &xml[value_start + 1..value_end];
        if raw.len() > max_len {
            return Err(DocxError::new(
                DocxErrorKind::SchemaViolation,
                format!(
                    "attribute length exceeds limit: {} > {}",
                    raw.len(),
                    max_len
                ),
            ));
        }
        cursor = value_end.checked_add(1).ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "attribute offset overflow")
        })?;
    }
    Ok(())
}

fn element_text_limited(
    xml: &str,
    name: &str,
    max_len: usize,
) -> Result<Option<String>, DocxError> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let Some(start) = xml.find(&open) else {
        return Ok(None);
    };
    let open_end = xml[start..]
        .find('>')
        .and_then(|value| {
            start
                .checked_add(value)
                .and_then(|value| value.checked_add(1))
        })
        .ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "unterminated text element")
        })?;
    if xml[start..open_end].trim_end().ends_with("/>") {
        return Ok(Some(String::new()));
    }
    let end = xml[open_end..]
        .find(&close)
        .and_then(|value| open_end.checked_add(value))
        .ok_or_else(|| {
            DocxError::new(DocxErrorKind::SchemaViolation, "unterminated text element")
        })?;
    let raw = &xml[open_end..end];
    if raw.len() > max_len {
        return Err(DocxError::new(
            DocxErrorKind::SchemaViolation,
            format!("text length exceeds limit: {} > {}", raw.len(), max_len),
        ));
    }
    let text = xml_unescape(raw);
    if text.len() > max_len {
        return Err(DocxError::new(
            DocxErrorKind::SchemaViolation,
            format!("text length exceeds limit: {} > {}", text.len(), max_len),
        ));
    }
    Ok(Some(text))
}

fn attr_u64_limited(xml: &str, name: &str, max_len: usize) -> Result<Option<u64>, DocxError> {
    Ok(attr_limited(xml, name, max_len)?.and_then(|value| value.parse().ok()))
}

fn xml_escape(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&apos;".to_string(),
            ch => ch.to_string(),
        })
        .collect()
}

fn xml_unescape(text: &str) -> String {
    let mut value = text.to_string();
    for (from, to) in [
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&apos;", "'"),
        ("&amp;", "&"),
    ] {
        value = value.replace(from, to);
    }
    value
}

fn replace_block_id(block: &mut BlockSnapshot, new_id: BlockId) -> BlockSnapshot {
    match block {
        BlockSnapshot::Text(value) => {
            value.id = new_id;
            BlockSnapshot::Text(value.clone())
        }
        BlockSnapshot::Table(value) => {
            value.id = TableId::new(new_id.get());
            BlockSnapshot::Table(value.clone())
        }
        BlockSnapshot::Image(value) => {
            value.id = ImageId::new(new_id.get());
            BlockSnapshot::Image(value.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn office_oxide_docx_roundtrip_keeps_a_hard_page_break_in_ir() {
        let mut source = DocumentIR::default();
        source.metadata.format = OfficeOxideFormat::Docx;
        source.sections.push(office_oxide::ir::Section {
            elements: vec![
                IrElement::Paragraph(IrParagraph {
                    content: vec![IrInlineContent::Text(IrTextSpan::plain("before"))],
                    ..Default::default()
                }),
                IrElement::PageBreak,
                IrElement::Paragraph(IrParagraph {
                    content: vec![IrInlineContent::Text(IrTextSpan::plain("after"))],
                    ..Default::default()
                }),
            ],
            ..Default::default()
        });
        let mut bytes = IoCursor::new(Vec::new());
        create_from_ir_to_writer(&source, OfficeOxideFormat::Docx, &mut bytes)
            .expect("write minimal DOCX with page break");
        let parsed = OfficeOxideDocument::from_reader(
            IoCursor::new(bytes.into_inner()),
            OfficeOxideFormat::Docx,
        )
        .expect("parse minimal DOCX with page break");
        assert!(
            parsed.to_ir().sections[0]
                .elements
                .iter()
                .any(|element| matches!(element, IrElement::PageBreak))
        );
    }

    fn sample() -> DocumentSnapshot {
        let mut editor = Editor::new();
        editor
            .execute(Command::InsertText {
                text: "Hello <Rudra> & world".into(),
            })
            .unwrap();
        editor.snapshot()
    }

    fn valid_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x08,
            0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99,
            0x3D, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ]
    }

    fn standard_docx_part_text(bytes: &[u8], name: &str) -> String {
        use std::io::Read as _;

        let mut archive = ZipArchive::new(IoCursor::new(bytes)).unwrap();
        let mut entry = archive.by_name(name).unwrap();
        let mut text = String::new();
        entry.read_to_string(&mut text).unwrap();
        text
    }

    fn standard_docx_has_part(bytes: &[u8], name: &str) -> bool {
        let mut archive = ZipArchive::new(IoCursor::new(bytes)).unwrap();
        archive.by_name(name).is_ok()
    }

    #[test]
    fn export_is_real_zip_docx() {
        let exporter = DocxExporter;
        let bytes = exporter.export(&sample()).unwrap();
        assert!(bytes.starts_with(&opc::ZIP_LOCAL_FILE_SIGNATURE.to_le_bytes()));
        // Verify it contains [Content_Types].xml
        assert!(
            bytes
                .windows(b"[Content_Types].xml".len())
                .any(|w| w == b"[Content_Types].xml")
        );
        // Verify it contains _rels/.rels
        assert!(
            bytes
                .windows(b"_rels/.rels".len())
                .any(|w| w == b"_rels/.rels")
        );
        // Verify it contains word/document.xml
        assert!(
            bytes
                .windows(b"word/document.xml".len())
                .any(|w| w == b"word/document.xml")
        );
        // Verify it starts with PK (ZIP magic bytes)
        assert_eq!(&bytes[..2], b"PK");
    }

    #[test]
    fn export_import_roundtrip_preserves_text_and_style() {
        let mut editor = Editor::new();
        editor
            .execute(Command::InsertText {
                text: "styled".into(),
            })
            .unwrap();
        let block_id = BlockId::new(1);
        editor
            .set_selection(Selection::new(
                Cursor::new(TextContainerId::Block(block_id), TextOffset::new(0)),
                Cursor::new(TextContainerId::Block(block_id), TextOffset::new(6)),
            ))
            .unwrap();
        editor
            .execute(Command::SetTextStyle {
                range: TextRange::new(TextOffset::new(0), TextOffset::new(6)),
                patch: TextStylePatch::new()
                    .bold(true)
                    .foreground(Some(Rgba8::rgb(1, 2, 3))),
            })
            .unwrap();
        let source = editor.snapshot();
        let bytes = DocxExporter.export(&source).unwrap();
        let restored = DocxImporter.import(&bytes).unwrap();
        assert_eq!(restored.blocks[0].text(), source.blocks[0].text());
        let restored_run = &restored.blocks[0].text_block().unwrap().runs[0];
        assert!(restored_run.style.bold());
        assert_eq!(restored_run.style.foreground(), Some(Rgba8::rgb(1, 2, 3)));
    }

    #[test]
    fn export_import_roundtrip_preserves_paragraph_spacing() {
        let mut editor = Editor::new();
        editor
            .execute(Command::InsertText {
                text: "spaced".into(),
            })
            .unwrap();
        editor
            .execute(Command::SetParagraphStyle {
                block_id: BlockId::new(1),
                patch: ParagraphStylePatch::default()
                    .indents(24, 36, -12)
                    .spacing(30, 42),
            })
            .unwrap();
        let bytes = DocxExporter.export(&editor.snapshot()).unwrap();
        let restored = DocxImporter.import_with_layout(&bytes).unwrap().snapshot;
        let style = &restored.blocks[0].text_block().unwrap().paragraph_style;
        assert_eq!(style.indent_left_half_points(), 24);
        assert_eq!(style.indent_right_half_points(), 36);
        assert_eq!(style.first_line_indent_half_points(), -12);
        assert_eq!(style.space_before_half_points(), 30);
        assert_eq!(style.space_after_half_points(), 42);
    }

    #[test]
    fn standard_export_roundtrips_headings_lists_and_tables() {
        let list_id = ListId::new(77);
        let document = DocumentSnapshot {
            blocks: vec![
                BlockSnapshot::Text(TextBlockSnapshot {
                    id: BlockId::new(1),
                    kind: BlockKind::Heading(HeadingLevel::new(2).unwrap()),
                    paragraph_style: ParagraphStyle::default().with_alignment(Alignment::Center),
                    text: "Release notes".to_string(),
                    runs: vec![InlineRun::new(
                        "Release notes",
                        TextStyle::default().with_bold(true),
                    )],
                }),
                BlockSnapshot::Text(TextBlockSnapshot {
                    id: BlockId::new(2),
                    kind: BlockKind::ListItem {
                        list_id,
                        kind: ListKind::Numbered,
                    },
                    paragraph_style: ParagraphStyle::default(),
                    text: "First item".to_string(),
                    runs: vec![InlineRun::new("First item", TextStyle::default())],
                }),
                BlockSnapshot::Text(TextBlockSnapshot {
                    id: BlockId::new(3),
                    kind: BlockKind::ListItem {
                        list_id,
                        kind: ListKind::Numbered,
                    },
                    paragraph_style: ParagraphStyle::default(),
                    text: "Second item".to_string(),
                    runs: vec![InlineRun::new("Second item", TextStyle::default())],
                }),
                BlockSnapshot::Table(TableSnapshot {
                    id: TableId::new(1),
                    columns: vec![
                        TableColumnSnapshot {
                            id: ColumnId::new(1),
                            width_half_points: 1_440,
                        },
                        TableColumnSnapshot {
                            id: ColumnId::new(2),
                            width_half_points: 1_440,
                        },
                    ],
                    rows: vec![
                        TableRowSnapshot {
                            id: RowId::new(1),
                            cells: vec![
                                TableCellSnapshot {
                                    id: CellId::new(1),
                                    text: "Name".to_string(),
                                    runs: Vec::new(),
                                    style: CellStyle::default(),
                                    alignment: CellAlignment::Left,
                                },
                                TableCellSnapshot {
                                    id: CellId::new(2),
                                    text: "Value".to_string(),
                                    runs: Vec::new(),
                                    style: CellStyle::default(),
                                    alignment: CellAlignment::Center,
                                },
                            ],
                        },
                        TableRowSnapshot {
                            id: RowId::new(2),
                            cells: vec![
                                TableCellSnapshot {
                                    id: CellId::new(3),
                                    text: "Rudra".to_string(),
                                    runs: Vec::new(),
                                    style: CellStyle::default(),
                                    alignment: CellAlignment::Left,
                                },
                                TableCellSnapshot {
                                    id: CellId::new(4),
                                    text: "42".to_string(),
                                    runs: Vec::new(),
                                    style: CellStyle::default(),
                                    alignment: CellAlignment::Right,
                                },
                            ],
                        },
                    ],
                }),
            ],
            revision: Revision::new(0),
        };

        let bytes = DocxExporter.export(&document).unwrap();
        let document_xml = standard_docx_part_text(&bytes, PART_DOCUMENT);
        assert!(!document_xml.contains(RUDRA_NS));
        let restored = DocxImporter.import(&bytes).unwrap();

        let heading = restored
            .blocks
            .iter()
            .filter_map(BlockSnapshot::text_block)
            .find(|block| block.text() == "Release notes")
            .unwrap();
        assert!(matches!(
            heading.kind,
            BlockKind::Heading(level) if level.get() == 2
        ));
        assert_eq!(heading.paragraph_style.alignment(), Alignment::Center);

        let list = restored
            .blocks
            .iter()
            .filter_map(BlockSnapshot::text_block)
            .filter(|block| matches!(block.kind, BlockKind::ListItem { .. }))
            .collect::<Vec<_>>();
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|block| matches!(
            block.kind,
            BlockKind::ListItem {
                kind: ListKind::Numbered,
                ..
            }
        )));

        let table = restored
            .blocks
            .iter()
            .find_map(BlockSnapshot::table)
            .unwrap();
        assert_eq!(table.row_count(), 2);
        assert_eq!(table.column_count(), 2);
        assert_eq!(table.rows[0].cells[0].text, "Name");
        assert_eq!(table.rows[1].cells[1].text, "42");
    }

    #[test]
    fn standard_export_rejects_webp_without_silent_loss() {
        let document = DocumentSnapshot {
            blocks: vec![BlockSnapshot::Image(ImageSnapshot {
                id: ImageId::new(1),
                payload: ImagePayload::new(
                    ImageFormat::WebP,
                    PixelSize::new(1, 1).unwrap(),
                    vec![1],
                )
                .unwrap(),
                layout: ImageLayout::default(),
            })],
            revision: Revision::new(0),
        };

        let error = DocxExporter.export(&document).unwrap_err();
        assert_eq!(error.kind, DocxErrorKind::UnsupportedPart);
        assert!(error.message.contains("WebP"));
    }

    #[test]
    fn malformed_input_is_rejected_honestly() {
        let error = DocxImporter.import(b"not xml").unwrap_err();
        assert_eq!(error.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn import_rejects_deflate_compression() {
        // Build a valid stored ZIP, then mutate the method to deflate
        let mut fake = Vec::new();
        // Local file header (stored)
        opc::write_u32_le(&mut fake, opc::ZIP_LOCAL_FILE_SIGNATURE);
        opc::write_u16_le(&mut fake, 20);
        opc::write_u16_le(&mut fake, 0);
        opc::write_u16_le(&mut fake, opc::ZIP_METHOD_STORED);
        opc::write_u16_le(&mut fake, 0);
        opc::write_u16_le(&mut fake, 0);
        opc::write_crc32(&mut fake, 0);
        opc::write_u32_le(&mut fake, 12);
        opc::write_u32_le(&mut fake, 12);
        opc::write_u16_le(&mut fake, 12);
        opc::write_u16_le(&mut fake, 0);
        fake.extend_from_slice(b"[Content_Types]");
        fake.extend_from_slice(b"<?xml version=\"1.0\"?><Types/>");
        // EOCD (minimal, no central dir)
        let eocd_offset = fake.len() as u32;
        opc::write_u32_le(&mut fake, opc::ZIP_END_OF_CENTRAL_DIRECTORY_SIGNATURE);
        opc::write_u16_le(&mut fake, 0);
        opc::write_u16_le(&mut fake, 0);
        opc::write_u16_le(&mut fake, 1);
        opc::write_u16_le(&mut fake, 1);
        opc::write_u32_le(&mut fake, 0); // central dir size = 0
        opc::write_u32_le(&mut fake, eocd_offset);
        opc::write_u16_le(&mut fake, 0);

        let error = DocxImporter.import(&fake).unwrap_err();
        assert_eq!(error.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn import_flat_opc_auto_detection() {
        // Flat OPC XML should be rejected by the ZIP importer
        let flat_xml = b"<?xml version=\"1.0\"?><pkg:package xmlns:pkg=\"http://schemas.microsoft.com/office/2006/xmlPackage\"></pkg:package>";
        let error = DocxImporter.import(flat_xml).unwrap_err();
        assert_eq!(error.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn export_preserves_flat_opc_api() {
        // export_flat_opc is an explicitly named XML API
        let doc = sample();
        let bytes = export_flat_opc(&doc).unwrap();
        assert!(!bytes.is_empty());
        assert!(bytes.starts_with(b"PK"));
    }

    // --- Regression tests for T1.4.1 hardening ---

    #[test]
    fn import_assigns_unique_ids_to_missing_rw_id() {
        // Build a DOCX with two paragraphs that lack rw:id (both default to 1)
        let mut doc_xml = String::new();
        writeln!(
            doc_xml,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(
            doc_xml,
            r#"<w:document xmlns:w="{WORD_NS}" xmlns:rw="{RUDRA_NS}"><w:body>"#
        )
        .unwrap();
        writeln!(doc_xml, r#"<w:p><w:r><w:t>First</w:t></w:r></w:p>"#).unwrap();
        writeln!(doc_xml, r#"<w:p><w:r><w:t>Second</w:t></w:r></w:p>"#).unwrap();
        writeln!(doc_xml, "</w:body></w:document>").unwrap();

        let mut rels_xml = String::new();
        writeln!(
            rels_xml,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(
            rels_xml,
            r#"<Relationships xmlns="{REL_NS}"><Relationship Id="rIdDocument" Type="{REL_OFFICE_DOCUMENT}" Target="word/document.xml"/></Relationships>"#
        )
        .unwrap();

        let mut content_types = String::new();
        writeln!(
            content_types,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(content_types, r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="{RELATIONSHIPS}"/><Default Extension="xml" ContentType="{XML}"/><Override PartName="/word/document.xml" ContentType="{DOCUMENT}"/></Types>"#).unwrap();

        let package = opc::DocxPackage {
            entries: vec![
                opc::ZipEntry {
                    name: PART_CONTENT_TYPES.to_string(),
                    content_type: CONTENT_TYPES.to_string(),
                    data: FlatOpcData::Xml(content_types),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_RELS.to_string(),
                    content_type: RELATIONSHIPS.to_string(),
                    data: FlatOpcData::Xml(rels_xml),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_DOCUMENT.to_string(),
                    content_type: DOCUMENT.to_string(),
                    data: FlatOpcData::Xml(doc_xml),
                    compression: opc::ZipCompression::Stored,
                },
            ],
        };

        let bytes = opc::serialize_zip(&package).unwrap();
        let restored = DocxImporter.import(&bytes).unwrap();
        assert_eq!(restored.blocks.len(), 2);
        // The two blocks must have different IDs
        let id0 = restored.blocks[0].id();
        let id1 = restored.blocks[1].id();
        assert_ne!(id0, id1, "paragraphs without rw:id must get unique IDs");
    }

    #[test]
    fn import_strict_rejects_duplicate_ids() {
        // Build a DOCX with two paragraphs that have the same explicit rw:id
        let mut doc_xml = String::new();
        writeln!(
            doc_xml,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(
            doc_xml,
            r#"<w:document xmlns:w="{WORD_NS}" xmlns:rw="{RUDRA_NS}"><w:body>"#
        )
        .unwrap();
        writeln!(
            doc_xml,
            r#"<w:p rw:id="42"><w:r><w:t>First</w:t></w:r></w:p>"#
        )
        .unwrap();
        writeln!(
            doc_xml,
            r#"<w:p rw:id="42"><w:r><w:t>Second</w:t></w:r></w:p>"#
        )
        .unwrap();
        writeln!(doc_xml, "</w:body></w:document>").unwrap();

        let mut rels_xml = String::new();
        writeln!(
            rels_xml,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(
            rels_xml,
            r#"<Relationships xmlns="{REL_NS}"><Relationship Id="rIdDocument" Type="{REL_OFFICE_DOCUMENT}" Target="word/document.xml"/></Relationships>"#
        )
        .unwrap();

        let mut content_types = String::new();
        writeln!(
            content_types,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(content_types, r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="{RELATIONSHIPS}"/><Default Extension="xml" ContentType="{XML}"/><Override PartName="/word/document.xml" ContentType="{DOCUMENT}"/></Types>"#).unwrap();

        let package = opc::DocxPackage {
            entries: vec![
                opc::ZipEntry {
                    name: PART_CONTENT_TYPES.to_string(),
                    content_type: CONTENT_TYPES.to_string(),
                    data: FlatOpcData::Xml(content_types),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_RELS.to_string(),
                    content_type: RELATIONSHIPS.to_string(),
                    data: FlatOpcData::Xml(rels_xml),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_DOCUMENT.to_string(),
                    content_type: DOCUMENT.to_string(),
                    data: FlatOpcData::Xml(doc_xml),
                    compression: opc::ZipCompression::Stored,
                },
            ],
        };

        let bytes = opc::serialize_zip(&package).unwrap();
        let result = DocxImporter.import_with_options(&bytes, DocxImportOptions { strict: true });
        assert!(result.is_err());
    }

    #[test]
    fn import_nondedup_remaps_duplicate_ids() {
        // Build a DOCX with two paragraphs that have the same explicit rw:id
        let mut doc_xml = String::new();
        writeln!(
            doc_xml,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(
            doc_xml,
            r#"<w:document xmlns:w="{WORD_NS}" xmlns:rw="{RUDRA_NS}"><w:body>"#
        )
        .unwrap();
        writeln!(
            doc_xml,
            r#"<w:p rw:id="42"><w:r><w:t>First</w:t></w:r></w:p>"#
        )
        .unwrap();
        writeln!(
            doc_xml,
            r#"<w:p rw:id="42"><w:r><w:t>Second</w:t></w:r></w:p>"#
        )
        .unwrap();
        writeln!(doc_xml, "</w:body></w:document>").unwrap();

        let mut rels_xml = String::new();
        writeln!(
            rels_xml,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(
            rels_xml,
            r#"<Relationships xmlns="{REL_NS}"><Relationship Id="rIdDocument" Type="{REL_OFFICE_DOCUMENT}" Target="word/document.xml"/></Relationships>"#
        )
        .unwrap();

        let mut content_types = String::new();
        writeln!(
            content_types,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(content_types, r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="{RELATIONSHIPS}"/><Default Extension="xml" ContentType="{XML}"/><Override PartName="/word/document.xml" ContentType="{DOCUMENT}"/></Types>"#).unwrap();

        let package = opc::DocxPackage {
            entries: vec![
                opc::ZipEntry {
                    name: PART_CONTENT_TYPES.to_string(),
                    content_type: CONTENT_TYPES.to_string(),
                    data: FlatOpcData::Xml(content_types),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_RELS.to_string(),
                    content_type: RELATIONSHIPS.to_string(),
                    data: FlatOpcData::Xml(rels_xml),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_DOCUMENT.to_string(),
                    content_type: DOCUMENT.to_string(),
                    data: FlatOpcData::Xml(doc_xml),
                    compression: opc::ZipCompression::Stored,
                },
            ],
        };

        let bytes = opc::serialize_zip(&package).unwrap();
        let restored = DocxImporter
            .import_with_options(&bytes, DocxImportOptions { strict: false })
            .unwrap();
        assert_eq!(restored.blocks.len(), 2);
        // Both blocks should be retained with different IDs
        let id0 = restored.blocks[0].id();
        let id1 = restored.blocks[1].id();
        assert_ne!(id0, id1, "duplicate IDs must be remapped to unique values");
        assert_eq!(restored.blocks[0].text(), Some("First"));
        assert_eq!(restored.blocks[1].text(), Some("Second"));
    }

    #[test]
    fn import_image_roundtrip() {
        let mut editor = Editor::new();
        let image_payload = ImagePayload::new(
            ImageFormat::Png,
            PixelSize::new(100, 100).unwrap(),
            valid_png(),
        )
        .unwrap();
        editor
            .execute(Command::InsertImage {
                payload: image_payload,
            })
            .unwrap();
        let source = editor.snapshot();
        let bytes = DocxExporter.export(&source).unwrap();
        let restored = DocxImporter.import(&bytes).unwrap();

        assert_eq!(restored.blocks.len(), 2);
        let restored_image = restored
            .blocks
            .iter()
            .find_map(BlockSnapshot::image)
            .unwrap_or_else(|| {
                panic!("expected image after standard DOCX round-trip: {restored:#?}")
            });
        assert_eq!(restored_image.payload.bytes, valid_png());
        assert_eq!(restored_image.layout.size.width, 100);
        assert_eq!(restored_image.layout.size.height, 100);
    }

    #[test]
    fn import_image_roundtrip_preserves_data() {
        let mut editor = Editor::new();
        let image_payload = ImagePayload::new(
            ImageFormat::Png,
            PixelSize::new(64, 64).unwrap(),
            valid_png(),
        )
        .unwrap();
        editor
            .execute(Command::InsertImage {
                payload: image_payload,
            })
            .unwrap();
        let source = editor.snapshot();
        let bytes = DocxExporter.export(&source).unwrap();
        let restored = DocxImporter.import(&bytes).unwrap();

        assert_eq!(restored.blocks.len(), 2);
        let restored_image = restored
            .blocks
            .iter()
            .find_map(BlockSnapshot::image)
            .unwrap_or_else(|| {
                panic!("expected image after standard DOCX round-trip: {restored:#?}")
            });
        assert_eq!(restored_image.payload.bytes, valid_png());
    }

    #[test]
    fn import_image_without_rels_falls_back_to_paragraph() {
        // Build a DOCX where the document references an image relationship
        // but the rels file is missing — image block is skipped, paragraph remains
        let mut doc_xml = String::new();
        writeln!(
            doc_xml,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(
            doc_xml,
            r#"<w:document xmlns:w="{WORD_NS}" xmlns:rw="{RUDRA_NS}"><w:body>"#
        )
        .unwrap();
        writeln!(
            doc_xml,
            r#"<w:p><w:r><w:drawing><rw:image rw:id="1" rw:relationship="rIdMissing" rw:width="100" rw:height="100"/></w:drawing></w:r></w:p>"#
        )
        .unwrap();
        writeln!(doc_xml, "</w:body></w:document>").unwrap();

        let mut rels_xml = String::new();
        writeln!(
            rels_xml,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(
            rels_xml,
            r#"<Relationships xmlns="{REL_NS}"><Relationship Id="rIdDocument" Type="{REL_OFFICE_DOCUMENT}" Target="word/document.xml"/></Relationships>"#
        )
        .unwrap();

        let mut content_types = String::new();
        writeln!(
            content_types,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(content_types, r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="{RELATIONSHIPS}"/><Default Extension="xml" ContentType="{XML}"/><Override PartName="/word/document.xml" ContentType="{DOCUMENT}"/></Types>"#).unwrap();

        let package = opc::DocxPackage {
            entries: vec![
                opc::ZipEntry {
                    name: PART_CONTENT_TYPES.to_string(),
                    content_type: CONTENT_TYPES.to_string(),
                    data: FlatOpcData::Xml(content_types),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_RELS.to_string(),
                    content_type: RELATIONSHIPS.to_string(),
                    data: FlatOpcData::Xml(rels_xml),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_DOCUMENT.to_string(),
                    content_type: DOCUMENT.to_string(),
                    data: FlatOpcData::Xml(doc_xml),
                    compression: opc::ZipCompression::Stored,
                },
            ],
        };

        let bytes = opc::serialize_zip(&package).unwrap();
        // Image with missing relationship should be skipped (no crash), paragraph retained
        let restored = DocxImporter.import(&bytes).unwrap();
        assert_eq!(restored.blocks.len(), 1);
        assert!(restored.blocks[0].text_block().is_some());
    }

    // --- T1.4.4.2: OPC relationship/metadata tests ---

    #[test]
    fn export_root_rels_has_office_document_and_no_image_relationship() {
        let bytes = DocxExporter.export(&sample()).unwrap();
        let rels_xml = standard_docx_part_text(&bytes, PART_RELS);
        assert!(rels_xml.contains(&format!(
            r#"Type="{REL_OFFICE_DOCUMENT}" Target="word/document.xml""#
        )));
        assert!(!rels_xml.contains(REL_IMAGE));
    }

    #[test]
    fn export_document_rels_has_standard_image_relationship() {
        let mut editor = Editor::new();
        let image_payload = ImagePayload::new(
            ImageFormat::Png,
            PixelSize::new(100, 100).unwrap(),
            valid_png(),
        )
        .unwrap();
        editor
            .execute(Command::InsertImage {
                payload: image_payload,
            })
            .unwrap();
        let bytes = DocxExporter.export(&editor.snapshot()).unwrap();
        let rels_xml = standard_docx_part_text(&bytes, PART_DOCUMENT_RELS);
        let document_xml = standard_docx_part_text(&bytes, PART_DOCUMENT);
        assert!(rels_xml.contains(REL_IMAGE));
        assert!(!rels_xml.contains(REL_OFFICE_DOCUMENT));
        assert!(rels_xml.contains(r#"Target="media/image1.png""#));
        assert!(document_xml.contains("a:blip"));
        assert!(!document_xml.contains(RUDRA_NS));
    }

    #[test]
    fn export_metadata_entry_has_no_leading_slash() {
        let options = DocxExportOptions {
            author: Some("Test Author".to_string()),
            ..DocxExportOptions::default()
        };
        let bytes = DocxExporter
            .export_with_options(&sample(), options)
            .unwrap();
        assert!(standard_docx_has_part(&bytes, PART_METADATA));
        let core_xml = standard_docx_part_text(&bytes, PART_METADATA);
        assert!(core_xml.contains("Test Author"));
    }

    #[test]
    fn export_metadata_has_content_type_override_in_types() {
        let options = DocxExportOptions {
            author: Some("Test Author".to_string()),
            ..DocxExportOptions::default()
        };
        let bytes = DocxExporter
            .export_with_options(&sample(), options)
            .unwrap();
        let ct_xml = standard_docx_part_text(&bytes, PART_CONTENT_TYPES);
        assert!(ct_xml.contains(r#"Override PartName="/docProps/core.xml""#));
    }

    #[test]
    fn export_root_rels_has_core_properties_relationship_when_metadata_exists() {
        let options = DocxExportOptions {
            author: Some("Test Author".to_string()),
            ..DocxExportOptions::default()
        };
        let bytes = DocxExporter
            .export_with_options(&sample(), options)
            .unwrap();
        let rels_xml = standard_docx_part_text(&bytes, PART_RELS);
        assert!(rels_xml.contains(r#"Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml""#));
    }

    #[test]
    fn export_standard_docx_has_core_properties_part() {
        let bytes = DocxExporter.export(&sample()).unwrap();
        let rels_xml = standard_docx_part_text(&bytes, PART_RELS);
        assert!(rels_xml.contains("core-properties"));
        assert!(standard_docx_has_part(&bytes, PART_METADATA));
    }

    #[test]
    fn import_resolves_relative_image_targets() {
        // Build a DOCX where document.xml.rels uses relative media/ targets
        let mut doc_xml = String::new();
        writeln!(
            doc_xml,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(
            doc_xml,
            r#"<w:document xmlns:w="{WORD_NS}" xmlns:rw="{RUDRA_NS}"><w:body>"#
        )
        .unwrap();
        writeln!(
            doc_xml,
            r#"<w:p><w:r><w:drawing><rw:image rw:id="1" rw:relationship="rIdImage0" rw:width="100" rw:height="100"/></w:drawing></w:r></w:p>"#
        )
        .unwrap();
        writeln!(doc_xml, "</w:body></w:document>").unwrap();

        let mut root_rels = String::new();
        writeln!(
            root_rels,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(
            root_rels,
            r#"<Relationships xmlns="{REL_NS}"><Relationship Id="rIdOfficeDocument" Type="{REL_OFFICE_DOCUMENT}" Target="word/document.xml"/></Relationships>"#
        )
        .unwrap();

        let mut doc_rels = String::new();
        writeln!(
            doc_rels,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(
            doc_rels,
            r#"<Relationships xmlns="{REL_NS}"><Relationship Id="rIdImage0" Type="{REL_IMAGE}" Target="media/image_0.png"/></Relationships>"#
        )
        .unwrap();

        let mut content_types = String::new();
        writeln!(
            content_types,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(content_types, r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="{RELATIONSHIPS}"/><Default Extension="xml" ContentType="{XML}"/><Override PartName="/word/document.xml" ContentType="{DOCUMENT}"/><Override PartName="/word/_rels/document.xml.rels" ContentType="{RELATIONSHIPS}"/><Override PartName="/word/media/image_0.png" ContentType="{PNG}"/></Types>"#).unwrap();

        let package = opc::DocxPackage {
            entries: vec![
                opc::ZipEntry {
                    name: PART_CONTENT_TYPES.to_string(),
                    content_type: CONTENT_TYPES.to_string(),
                    data: FlatOpcData::Xml(content_types),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_RELS.to_string(),
                    content_type: RELATIONSHIPS.to_string(),
                    data: FlatOpcData::Xml(root_rels),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_DOCUMENT_RELS.to_string(),
                    content_type: RELATIONSHIPS.to_string(),
                    data: FlatOpcData::Xml(doc_rels),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_DOCUMENT.to_string(),
                    content_type: DOCUMENT.to_string(),
                    data: FlatOpcData::Xml(doc_xml),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: "word/media/image_0.png".to_string(),
                    content_type: PNG.to_string(),
                    data: FlatOpcData::Binary(vec![0x89, 0x50, 0x4E, 0x47]),
                    compression: opc::ZipCompression::Stored,
                },
            ],
        };

        let bytes = opc::serialize_zip(&package).unwrap();
        let restored = DocxImporter.import(&bytes).unwrap();
        assert_eq!(restored.blocks.len(), 1);
        let image = restored.blocks[0].image().expect("should be image block");
        assert_eq!(image.payload.bytes, vec![0x89, 0x50, 0x4E, 0x47]);
    }

    #[test]
    fn parse_blocks_rejects_inverted_body_markers_without_panicking() {
        let image_rels = std::collections::HashMap::new();
        let media_map = std::collections::HashMap::new();
        let result = std::panic::catch_unwind(|| {
            parse_blocks(
                "</w:body><w:body><w:p><w:r><w:t>x</w:t></w:r></w:p>",
                DocxImportOptions::default(),
                &image_rels,
                &media_map,
            )
        });

        assert!(result.is_ok(), "malformed body markers must not panic");
        assert!(result.unwrap().is_err());
    }

    #[test]
    fn parse_blocks_rejects_missing_body_markers_without_panicking() {
        let image_rels = std::collections::HashMap::new();
        let media_map = std::collections::HashMap::new();
        for xml in ["<w:body>", "</w:body>", "<w:document/>"] {
            let result = std::panic::catch_unwind(|| {
                parse_blocks(xml, DocxImportOptions::default(), &image_rels, &media_map)
            });
            assert!(result.is_ok(), "malformed XML must not panic: {xml}");
            assert!(result.unwrap().is_err());
        }
    }

    #[test]
    fn find_tag_end_returns_the_real_close_after_an_exact_open_tag() {
        let xml = "<w:p rw:id=\"1\"><w:r><w:t>x</w:t></w:r></w:p>";
        assert_eq!(find_tag_end(xml, 0, "<w:p>", "</w:p>").unwrap(), xml.len());
    }

    #[test]
    fn find_tag_end_rejects_an_unclosed_exact_open_tag() {
        assert!(find_tag_end("<w:p>", 0, "<w:p>", "</w:p>").is_err());
    }

    #[test]
    fn parse_color_rejects_non_ascii_without_panicking() {
        let result = std::panic::catch_unwind(|| parse_color("0é34567"));
        assert!(result.is_ok(), "non-ASCII color must not panic");
        assert_eq!(result.unwrap(), None);
    }

    // --- T1.4.4.3 importer cap hardening tests ---

    #[test]
    fn import_rejects_oversized_document_xml() {
        let limits = ImportLimits {
            max_document_xml_bytes: 64,
            ..ImportLimits::default()
        };
        let package = opc::DocxPackage {
            entries: vec![opc::ZipEntry {
                name: PART_DOCUMENT.to_string(),
                content_type: DOCUMENT.to_string(),
                data: FlatOpcData::Xml("x".repeat(65)),
                compression: opc::ZipCompression::Stored,
            }],
        };
        let err =
            read_document_with_limits(&package, DocxImportOptions::default(), limits).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn import_rejects_oversized_rels_xml() {
        let package = opc::DocxPackage {
            entries: vec![opc::ZipEntry {
                name: PART_RELS.to_string(),
                content_type: String::new(),
                data: FlatOpcData::Xml("x".repeat(65)),
                compression: opc::ZipCompression::Stored,
            }],
        };
        let limits = ImportLimits {
            max_rels_xml_bytes: 64,
            ..ImportLimits::default()
        };
        let err = enforce_package_limits(&package, limits).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn import_rejects_too_many_relationships_after_real_zip_parse() {
        let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="t" Target="t"/><Relationship Id="r2" Type="t" Target="t"/></Relationships>"#;
        let package = opc::DocxPackage {
            entries: vec![opc::ZipEntry {
                name: PART_RELS.to_string(),
                content_type: RELATIONSHIPS.to_string(),
                data: FlatOpcData::Xml(rels.to_string()),
                compression: opc::ZipCompression::Stored,
            }],
        };
        let bytes = opc::serialize_zip(&package).unwrap();
        let parsed = opc::parse_zip(&bytes).unwrap();
        let limits = ImportLimits {
            max_rels_count: 1,
            ..ImportLimits::default()
        };
        let err = enforce_package_limits(&parsed, limits).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn import_rejects_oversized_media_aggregate() {
        let package = opc::DocxPackage {
            entries: vec![opc::ZipEntry {
                name: "word/media/image.png".to_string(),
                content_type: PNG.to_string(),
                data: FlatOpcData::Binary(vec![0; 9]),
                compression: opc::ZipCompression::Stored,
            }],
        };
        let limits = ImportLimits {
            max_media_aggregate_bytes: 8,
            ..ImportLimits::default()
        };
        let err = enforce_package_limits(&package, limits).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn import_rejects_too_many_top_level_blocks() {
        let para = r#"<w:p><w:r><w:t>x</w:t></w:r></w:p>"#;
        let body = para.repeat(3);
        let doc_xml = format!(
            r#"<?xml version="1.0"?><w:document xmlns:w="{WORD_NS}" xmlns:rw="{RUDRA_NS}"><w:body>{}</w:body></w:document>"#,
            body
        );
        let package = opc::DocxPackage {
            entries: vec![opc::ZipEntry {
                name: PART_DOCUMENT.to_string(),
                content_type: DOCUMENT.to_string(),
                data: FlatOpcData::Xml(doc_xml),
                compression: opc::ZipCompression::Stored,
            }],
        };
        let limits = ImportLimits {
            max_blocks: 2,
            ..ImportLimits::default()
        };
        let err =
            read_document_with_limits(&package, DocxImportOptions::default(), limits).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::SchemaViolation);
    }

    #[test]
    fn import_rejects_too_many_runs_per_block() {
        let doc_xml = format!(
            r#"<?xml version="1.0"?><w:document xmlns:w="{WORD_NS}" xmlns:rw="{RUDRA_NS}"><w:body><w:p><w:r><w:t>x</w:t></w:r><w:r><w:t>y</w:t></w:r></w:p></w:body></w:document>"#
        );
        let package = opc::DocxPackage {
            entries: vec![opc::ZipEntry {
                name: PART_DOCUMENT.to_string(),
                content_type: DOCUMENT.to_string(),
                data: FlatOpcData::Xml(doc_xml),
                compression: opc::ZipCompression::Stored,
            }],
        };
        let limits = ImportLimits {
            max_runs_per_block: 1,
            ..ImportLimits::default()
        };
        let err =
            read_document_with_limits(&package, DocxImportOptions::default(), limits).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::SchemaViolation);
    }

    #[test]
    fn import_rejects_too_long_run_text() {
        let doc_xml = format!(
            r#"<?xml version="1.0"?><w:document xmlns:w="{WORD_NS}" xmlns:rw="{RUDRA_NS}"><w:body><w:p><w:r><w:t xml:space="preserve">12345</w:t></w:r></w:p></w:body></w:document>"#
        );
        let package = opc::DocxPackage {
            entries: vec![opc::ZipEntry {
                name: PART_DOCUMENT.to_string(),
                content_type: DOCUMENT.to_string(),
                data: FlatOpcData::Xml(doc_xml),
                compression: opc::ZipCompression::Stored,
            }],
        };
        let limits = ImportLimits {
            max_run_text_len: 4,
            ..ImportLimits::default()
        };
        let err =
            read_document_with_limits(&package, DocxImportOptions::default(), limits).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::SchemaViolation);
    }

    #[test]
    fn import_rejects_oversized_inbound_attribute() {
        let doc_xml = format!(
            r#"<?xml version="1.0"?><w:document xmlns:w="{WORD_NS}" xmlns:rw="{RUDRA_NS}"><w:body><w:p rw:id="12345"><w:r><w:t>x</w:t></w:r></w:p></w:body></w:document>"#
        );
        let package = opc::DocxPackage {
            entries: vec![opc::ZipEntry {
                name: PART_DOCUMENT.to_string(),
                content_type: DOCUMENT.to_string(),
                data: FlatOpcData::Xml(doc_xml),
                compression: opc::ZipCompression::Stored,
            }],
        };
        let limits = ImportLimits {
            max_attr_len: 4,
            ..ImportLimits::default()
        };
        let err =
            read_document_with_limits(&package, DocxImportOptions::default(), limits).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::SchemaViolation);
    }

    #[test]
    fn import_rejects_too_many_table_cells_per_table() {
        let tbl = r#"<w:tbl rw:id="1"><w:tblGrid><w:gridCol w:w="1440"/><w:gridCol w:w="1440"/></w:tblGrid><w:tr><w:tc><w:t>a</w:t></w:tc><w:tc><w:t>b</w:t></w:tc></w:tr><w:tr><w:tc><w:t>c</w:t></w:tc><w:tc><w:t>d</w:t></w:tc></w:tr></w:tbl>"#;
        let limits = ImportLimits {
            max_cells_per_table: 3,
            ..ImportLimits::default()
        };
        let err = parse_table_with_limits(tbl, limits, &mut 0).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::SchemaViolation);
    }

    #[test]
    fn import_rejects_too_many_table_rows() {
        let tbl = r#"<w:tbl rw:id="1"><w:tblGrid><w:gridCol w:w="1440"/></w:tblGrid><w:tr><w:tc><w:t>a</w:t></w:tc></w:tr><w:tr><w:tc><w:t>b</w:t></w:tc></w:tr></w:tbl>"#;
        let limits = ImportLimits {
            max_rows_per_table: 1,
            ..ImportLimits::default()
        };
        let err = parse_table_with_limits(tbl, limits, &mut 0).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::SchemaViolation);
    }

    #[test]
    fn parse_table_with_limits_rejects_aggregate_cells() {
        let tbl = r#"<w:tbl rw:id="1"><w:tblGrid><w:gridCol w:w="1440"/></w:tblGrid><w:tr><w:tc><w:t>a</w:t></w:tc></w:tr><w:tr><w:tc><w:t>b</w:t></w:tc></w:tr></w:tbl>"#;
        let limits = ImportLimits {
            max_aggregate_cells: 1,
            ..ImportLimits::default()
        };
        let err = parse_table_with_limits(tbl, limits, &mut 0).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::SchemaViolation);
    }

    #[test]
    fn import_media_borrowed_index_avoids_full_media_clone() {
        let media_bytes = vec![0x89, 0x50, 0x4E, 0x47];
        let doc_xml = format!(
            r#"<?xml version="1.0"?><w:document xmlns:w="{WORD_NS}" xmlns:rw="{RUDRA_NS}"><w:body><w:p><w:r><w:drawing><rw:image rw:id="1" rw:relationship="rIdImage0" rw:width="100" rw:height="100"/></w:drawing></w:r></w:p></w:body></w:document>"#
        );
        let root_rels = format!(
            r#"<?xml version="1.0"?><Relationships xmlns="{REL_NS}"><Relationship Id="rIdOfficeDocument" Type="{REL_OFFICE_DOCUMENT}" Target="word/document.xml"/></Relationships>"#
        );
        let doc_rels = format!(
            r#"<?xml version="1.0"?><Relationships xmlns="{REL_NS}"><Relationship Id="rIdImage0" Type="{REL_IMAGE}" Target="media/image_0.png"/></Relationships>"#
        );
        let content_types = format!(
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="{RELATIONSHIPS}"/><Default Extension="xml" ContentType="{XML}"/><Override PartName="/word/document.xml" ContentType="{DOCUMENT}"/><Override PartName="/word/_rels/document.xml.rels" ContentType="{RELATIONSHIPS}"/><Override PartName="/word/media/image_0.png" ContentType="{PNG}"/></Types>"#
        );
        let package = opc::DocxPackage {
            entries: vec![
                opc::ZipEntry {
                    name: PART_CONTENT_TYPES.to_string(),
                    content_type: CONTENT_TYPES.to_string(),
                    data: FlatOpcData::Xml(content_types),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_RELS.to_string(),
                    content_type: RELATIONSHIPS.to_string(),
                    data: FlatOpcData::Xml(root_rels),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_DOCUMENT_RELS.to_string(),
                    content_type: RELATIONSHIPS.to_string(),
                    data: FlatOpcData::Xml(doc_rels),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_DOCUMENT.to_string(),
                    content_type: DOCUMENT.to_string(),
                    data: FlatOpcData::Xml(doc_xml),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: "word/media/image_0.png".to_string(),
                    content_type: PNG.to_string(),
                    data: FlatOpcData::Binary(media_bytes.clone()),
                    compression: opc::ZipCompression::Stored,
                },
            ],
        };
        let bytes = opc::serialize_zip(&package).unwrap();
        let restored = DocxImporter.import(&bytes).unwrap();
        assert_eq!(restored.blocks.len(), 1);
        let image = restored.blocks[0].image().expect("should be image block");
        assert_eq!(image.payload.bytes, media_bytes);
    }

    #[test]
    fn parse_table_with_limits_rejects_wide_table() {
        let limits = ImportLimits {
            max_columns_per_table: 2,
            ..ImportLimits::default()
        };
        let xml = r#"<w:tbl rw:id="1"><w:tblGrid><w:gridCol w:w="1440"/><w:gridCol w:w="1440"/><w:gridCol w:w="1440"/></w:tblGrid></w:tbl>"#;
        let err = parse_table_with_limits(xml, limits, &mut 0).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::SchemaViolation);
    }

    #[test]
    fn parse_table_with_limits_rejects_many_cells_per_table() {
        let limits = ImportLimits {
            max_cells_per_table: 3,
            ..ImportLimits::default()
        };
        let xml = r#"<w:tbl rw:id="1"><w:tblGrid><w:gridCol w:w="1440"/><w:gridCol w:w="1440"/></w:tblGrid><w:tr><w:tc><w:t>a</w:t></w:tc><w:tc><w:t>b</w:t></w:tc></w:tr><w:tr><w:tc><w:t>c</w:t></w:tc><w:tc><w:t>d</w:t></w:tc></w:tr></w:tbl>"#;
        let err = parse_table_with_limits(xml, limits, &mut 0).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::SchemaViolation);
    }

    #[test]
    fn parse_row_with_limits_rejects_many_cells_per_row() {
        let limits = ImportLimits {
            max_columns_per_table: 2,
            ..ImportLimits::default()
        };
        let xml = r#"<w:tr><w:tc><w:t>a</w:t></w:tc><w:tc><w:t>b</w:t></w:tc><w:tc><w:t>c</w:t></w:tc></w:tr>"#;
        let err = parse_row_with_limits(xml, 1, limits, &mut 0).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::SchemaViolation);
    }

    #[test]
    fn import_rejects_too_many_tables() {
        let xml = format!(
            r#"<w:document xmlns:w="{WORD_NS}" xmlns:rw="{RUDRA_NS}"><w:body><w:tbl><w:tblGrid><w:gridCol/></w:tblGrid><w:tr><w:tc><w:t>a</w:t></w:tc></w:tr></w:tbl><w:tbl><w:tblGrid><w:gridCol/></w:tblGrid><w:tr><w:tc><w:t>b</w:t></w:tc></w:tr></w:tbl></w:body></w:document>"#
        );
        let limits = ImportLimits {
            max_tables: 1,
            ..ImportLimits::default()
        };
        let err = parse_blocks_with_limits(
            &xml,
            DocxImportOptions::default(),
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            limits,
            &mut 0,
        )
        .unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::SchemaViolation);
    }

    #[test]
    fn import_rejects_aggregate_cells_across_tables() {
        let xml = format!(
            r#"<w:document xmlns:w="{WORD_NS}" xmlns:rw="{RUDRA_NS}"><w:body><w:tbl><w:tblGrid><w:gridCol/></w:tblGrid><w:tr><w:tc><w:t>a</w:t></w:tc></w:tr></w:tbl><w:tbl><w:tblGrid><w:gridCol/></w:tblGrid><w:tr><w:tc><w:t>b</w:t></w:tc></w:tr></w:tbl></w:body></w:document>"#
        );
        let limits = ImportLimits {
            max_aggregate_cells: 1,
            ..ImportLimits::default()
        };
        let err = parse_blocks_with_limits(
            &xml,
            DocxImportOptions::default(),
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            limits,
            &mut 0,
        )
        .unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::SchemaViolation);
    }

    // --- T1.4.4.3 additional tests ---

    #[test]
    fn import_rejects_oversized_image_dimensions() {
        // Build a DOCX with an image whose rw:width exceeds u32::MAX
        let mut doc_xml = String::new();
        writeln!(
            doc_xml,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(
            doc_xml,
            r#"<w:document xmlns:w="{WORD_NS}" xmlns:rw="{RUDRA_NS}"><w:body>"#
        )
        .unwrap();
        writeln!(
            doc_xml,
            r#"<w:p><w:r><w:drawing><rw:image rw:id="1" rw:relationship="rIdImage0" rw:width="4294967297" rw:height="100"/></w:drawing></w:r></w:p>"#
        )
        .unwrap();
        writeln!(doc_xml, "</w:body></w:document>").unwrap();

        let mut root_rels = String::new();
        writeln!(
            root_rels,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(
            root_rels,
            r#"<Relationships xmlns="{REL_NS}"><Relationship Id="rIdOfficeDocument" Type="{REL_OFFICE_DOCUMENT}" Target="word/document.xml"/></Relationships>"#
        )
        .unwrap();

        let mut doc_rels = String::new();
        writeln!(
            doc_rels,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(
            doc_rels,
            r#"<Relationships xmlns="{REL_NS}"><Relationship Id="rIdImage0" Type="{REL_IMAGE}" Target="media/image_0.png"/></Relationships>"#
        )
        .unwrap();

        let mut content_types = String::new();
        writeln!(
            content_types,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#
        )
        .unwrap();
        writeln!(content_types, r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="{RELATIONSHIPS}"/><Default Extension="xml" ContentType="{XML}"/><Override PartName="/word/document.xml" ContentType="{DOCUMENT}"/><Override PartName="/word/_rels/document.xml.rels" ContentType="{RELATIONSHIPS}"/><Override PartName="/word/media/image_0.png" ContentType="{PNG}"/></Types>"#).unwrap();

        let package = opc::DocxPackage {
            entries: vec![
                opc::ZipEntry {
                    name: PART_CONTENT_TYPES.to_string(),
                    content_type: CONTENT_TYPES.to_string(),
                    data: FlatOpcData::Xml(content_types),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_RELS.to_string(),
                    content_type: RELATIONSHIPS.to_string(),
                    data: FlatOpcData::Xml(root_rels),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_DOCUMENT_RELS.to_string(),
                    content_type: RELATIONSHIPS.to_string(),
                    data: FlatOpcData::Xml(doc_rels),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_DOCUMENT.to_string(),
                    content_type: DOCUMENT.to_string(),
                    data: FlatOpcData::Xml(doc_xml),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: "word/media/image_0.png".to_string(),
                    content_type: PNG.to_string(),
                    data: FlatOpcData::Binary(vec![0x89, 0x50, 0x4E, 0x47]),
                    compression: opc::ZipCompression::Stored,
                },
            ],
        };
        let bytes = opc::serialize_zip(&package).unwrap();
        let err = DocxImporter.import(&bytes).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::SchemaViolation);
    }

    #[test]
    fn import_rejects_oversized_rels_xml_after_zip_parse() {
        let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="t" Target="t"/></Relationships>"#;
        let package = opc::DocxPackage {
            entries: vec![opc::ZipEntry {
                name: PART_RELS.to_string(),
                content_type: RELATIONSHIPS.to_string(),
                data: FlatOpcData::Xml(rels.to_string()),
                compression: opc::ZipCompression::Stored,
            }],
        };
        let bytes = opc::serialize_zip(&package).unwrap();
        let parsed = opc::parse_zip(&bytes).unwrap();
        let limits = ImportLimits {
            max_rels_xml_bytes: 10,
            ..ImportLimits::default()
        };
        let err = enforce_package_limits(&parsed, limits).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::InvalidPackage);
    }

    #[test]
    fn import_rejects_nested_table_rows() {
        // Nested tables are intentionally rejected until their full structure can be
        // represented without flattening or silently losing cells.
        let tbl = r#"<w:tbl rw:id="1"><w:tblGrid><w:gridCol w:w="1440"/><w:gridCol w:w="1440"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>outer</w:t></w:r></w:p><w:tbl><w:tblGrid><w:gridCol w:w="1440"/></w:tblGrid><w:tr><w:tc><w:t>inner</w:t></w:tc></w:tr></w:tbl></w:tc><w:tc><w:t>b</w:t></w:tc></w:tr></w:tbl>"#;
        let limits = ImportLimits {
            max_rows_per_table: 1,
            ..ImportLimits::default()
        };
        let err = parse_table_with_limits(tbl, limits, &mut 0).unwrap_err();
        assert_eq!(err.kind, DocxErrorKind::SchemaViolation);
    }
    #[test]
    fn imports_standard_docx_through_office_oxide() {
        let mut writer = office_oxide::docx::write::DocxWriter::new();
        writer.add_heading("External DOCX", 1);
        writer.add_paragraph("This package was produced by Office Oxide.");
        let mut output = std::io::Cursor::new(Vec::new());
        writer.write_to(&mut output).unwrap();

        let imported = DocxImporter.import(&output.into_inner()).unwrap();
        let text = document_to_plain_text(&imported);
        assert!(text.contains("External DOCX"));
        assert!(text.contains("produced by Office Oxide"));
    }

    #[test]
    fn imports_external_docx_into_editable_structure() {
        use office_oxide::docx::write::{Alignment as ExternalAlignment, Run as ExternalRun};

        let mut writer = office_oxide::docx::write::DocxWriter::new();
        writer
            .add_heading("Structured import", 2)
            .add_rich_paragraph(&[
                ExternalRun::new("Bold red").bold().color("102030"),
                ExternalRun::new(" italic").italic(),
            ])
            .add_paragraph_aligned("Centered", ExternalAlignment::Center)
            .add_list(&["First", "Second"], false)
            .add_table(&[vec!["Name", "Value"], vec!["Rudra", "42"]]);
        let mut output = std::io::Cursor::new(Vec::new());
        writer.write_to(&mut output).unwrap();

        let imported = DocxImporter.import(&output.into_inner()).unwrap();
        let heading = imported
            .blocks
            .iter()
            .find_map(BlockSnapshot::text_block)
            .filter(|block| block.text() == "Structured import")
            .unwrap();
        assert!(matches!(
            &heading.kind,
            BlockKind::Heading(level) if level.get() == 2
        ));

        let rich = imported
            .blocks
            .iter()
            .filter_map(BlockSnapshot::text_block)
            .find(|block| block.text() == "Bold red italic")
            .unwrap();
        assert_eq!(rich.runs.len(), 2);
        assert!(rich.runs[0].style.bold());
        assert_eq!(
            rich.runs[0].style.foreground(),
            Some(Rgba8::rgb(16, 32, 48))
        );
        assert!(rich.runs[1].style.italic());

        let centered = imported
            .blocks
            .iter()
            .filter_map(BlockSnapshot::text_block)
            .find(|block| block.text() == "Centered")
            .unwrap();
        assert_eq!(centered.paragraph_style.alignment(), Alignment::Center);

        let list_items = imported
            .blocks
            .iter()
            .filter_map(BlockSnapshot::text_block)
            .filter(|block| matches!(&block.kind, BlockKind::ListItem { .. }))
            .collect::<Vec<_>>();
        assert_eq!(list_items.len(), 2);
        assert!(list_items.iter().all(|block| matches!(
            &block.kind,
            BlockKind::ListItem {
                kind: ListKind::Bullet,
                ..
            }
        )));

        let table = imported
            .blocks
            .iter()
            .find_map(BlockSnapshot::table)
            .unwrap();
        assert_eq!(table.row_count(), 2);
        assert_eq!(table.column_count(), 2);
        assert_eq!(table.rows[0].cells[0].text, "Name");
        assert_eq!(table.rows[1].cells[1].text, "42");
    }

    #[test]
    fn vertical_merge_table_projects_to_writer_grid() {
        let cell = |text: &str, row_span: u32| IrTableCell {
            content: vec![IrElement::Paragraph(IrParagraph {
                content: vec![IrInlineContent::Text(IrTextSpan::plain(text))],
                ..Default::default()
            })],
            row_span,
            ..Default::default()
        };
        let document = DocumentIR {
            sections: vec![office_oxide::ir::Section {
                elements: vec![IrElement::Table(IrTable {
                    rows: vec![
                        IrTableRow {
                            cells: vec![cell("A", 2), cell("B", 2), cell("C", 1)],
                            ..Default::default()
                        },
                        IrTableRow {
                            cells: vec![cell("D", 1)],
                            ..Default::default()
                        },
                    ],
                    column_widths_twips: vec![100, 200, 300],
                    ..Default::default()
                })],
                ..Default::default()
            }],
            ..Default::default()
        };
        let snapshot = OfficeOxideProjector::new().project(&document).unwrap();
        let table = snapshot
            .blocks
            .iter()
            .find_map(BlockSnapshot::table)
            .unwrap();
        assert_eq!(table.columns.len(), 3);
        assert_eq!(
            table
                .columns
                .iter()
                .map(|column| column.width_half_points)
                .collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
        assert_eq!(table.rows.len(), 2);
        assert!(table.rows.iter().all(|row| row.cells.len() == 3));
        assert_eq!(table.rows[1].cells[0].text, "");
        assert_eq!(table.rows[1].cells[1].text, "");
        assert_eq!(table.rows[1].cells[2].text, "D");
    }

    #[cfg(feature = "private-fixtures")]
    fn private_docx_fixture(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/release-acceptance/docx")
            .join(name);
        std::fs::read(&path)
            .unwrap_or_else(|error| panic!("private DOCX fixture {}: {error}", path.display()))
    }

    #[cfg(feature = "private-fixtures")]
    #[test]
    fn projection_diagnostics_report_supported_fixture_content() {
        let bytes = private_docx_fixture("writer-layout-a4-multipage.docx");
        let imported = DocxImporter
            .import_with_layout(&bytes)
            .expect("import controlled DOCX fixture");

        assert_eq!(imported.projection.source.tables, 1);
        assert_eq!(imported.projection.source.images, 1);
        assert_eq!(imported.projection.source.page_breaks, 1);
        assert_eq!(imported.projection.projected.table_blocks, 1);
        assert_eq!(imported.projection.projected.image_blocks, 1);
        assert_eq!(imported.projection.projected.page_break_blocks, 1);
        assert_eq!(imported.projection.fallback.total(), 0);
    }

    #[test]
    fn public_docx_projection_reports_table_image_and_page_break() {
        let imported = DocxImporter
            .import_with_layout(include_bytes!(
                "../../../../fixtures/public/writer/synthetic-rich.docx"
            ))
            .expect("import synthetic DOCX with table, drawing, and page break");

        assert_eq!(imported.projection.source.tables, 1);
        assert_eq!(imported.projection.source.images, 1);
        assert_eq!(imported.projection.source.page_breaks, 1);
        assert_eq!(imported.projection.projected.table_blocks, 1);
        assert_eq!(imported.projection.projected.image_blocks, 1);
        assert_eq!(imported.projection.projected.page_break_blocks, 1);
        assert_eq!(imported.projection.fallback.total(), 0);
    }

    #[cfg(feature = "private-fixtures")]
    #[test]
    fn standard_drawingml_image_uses_display_geometry() {
        let bytes = private_docx_fixture("writer-visual-fidelity-fixture.docx");
        let imported = DocxImporter
            .import_with_layout(&bytes)
            .expect("import standard DrawingML image fixture");

        assert_eq!(imported.projection.source.images, 1);
        assert_eq!(imported.projection.projected.image_blocks, 1);
        let image = imported
            .snapshot
            .blocks
            .iter()
            .find_map(BlockSnapshot::image)
            .expect("projected image block");
        assert_eq!(image.layout.size, PixelSize::new(240, 140).unwrap());
    }

    #[test]
    fn public_drawingml_image_uses_display_geometry() {
        let imported = DocxImporter
            .import_with_layout(include_bytes!(
                "../../../../fixtures/public/writer/synthetic-rich.docx"
            ))
            .expect("import synthetic DrawingML image fixture");

        assert_eq!(imported.projection.source.images, 1);
        assert_eq!(imported.projection.projected.image_blocks, 1);
        let image = imported
            .snapshot
            .blocks
            .iter()
            .find_map(BlockSnapshot::image)
            .expect("projected image block");
        assert_eq!(image.layout.size, PixelSize::new(240, 140).unwrap());
    }

    #[cfg(feature = "private-fixtures")]
    #[test]
    fn standard_docx_fixture_preserves_cyrillic_text_through_projection() {
        let bytes = private_docx_fixture("writer-visual-fidelity-fixture.docx");
        let imported = DocxImporter
            .import_with_layout(&bytes)
            .expect("import standard Unicode DOCX fixture");

        let projected_text = imported
            .snapshot
            .blocks
            .iter()
            .filter_map(BlockSnapshot::text)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            projected_text.contains("Жёлтый текст"),
            "the standard DOCX reader and projection must preserve UTF-8 Cyrillic text"
        );
    }

    #[test]
    fn public_docx_preserves_styled_unicode_paragraphs_and_page_metadata() {
        let imported = DocxImporter
            .import_with_layout(include_bytes!(
                "../../../../fixtures/public/writer/synthetic-basic.docx"
            ))
            .expect("import independently generated public DOCX");
        let text_blocks = imported
            .snapshot
            .blocks
            .iter()
            .filter_map(BlockSnapshot::text_block)
            .collect::<Vec<_>>();
        let first = text_blocks
            .iter()
            .find(|block| block.text == "Рассвет 🙂на орбите")
            .expect("first public paragraph must retain Cyrillic and emoji");
        assert!(
            first
                .runs
                .iter()
                .any(|run| run.text == "Рассвет " && run.style.bold())
        );
        assert!(
            first
                .runs
                .iter()
                .any(|run| run.text == "🙂на орбите" && run.style.italic())
        );
        assert!(
            text_blocks
                .iter()
                .any(|block| block.text == "Учебная страница 😀")
        );
        assert_eq!(imported.projection.source.page_breaks, 1);
        assert_eq!(imported.projection.projected.page_break_blocks, 1);
        let section = imported
            .layout
            .sections
            .first()
            .expect("public section metadata");
        assert_eq!(section.width_twips, 11_906);
        assert_eq!(section.height_twips, 16_838);
        assert_eq!(section.header_text, "Синтетический верх");
        assert_eq!(section.footer_text, "Синтетический низ");
    }

    #[test]
    fn public_editing_pages_docx_imports_adjacent_paragraphs_and_page_break() {
        let imported = DocxImporter
            .import_with_layout(include_bytes!(
                "../../../../fixtures/public/writer/synthetic-editing-pages.docx"
            ))
            .expect("import public editing-pages DOCX");
        let blocks = &imported.snapshot.blocks;
        assert_eq!(blocks.len(), 4);
        let left = blocks[0].text_block().expect("first paragraph");
        let right = blocks[1].text_block().expect("adjacent paragraph");
        let page_break = blocks[2].text_block().expect("page-break block");
        let page2 = blocks[3].text_block().expect("second-page paragraph");
        assert_eq!(left.kind, BlockKind::Paragraph);
        assert_eq!(right.kind, BlockKind::Paragraph);
        assert_eq!(page_break.kind, BlockKind::PageBreak);
        assert_eq!(page2.kind, BlockKind::Paragraph);
        let ids = [left.id, right.id, page_break.id, page2.id];
        for (index, id) in ids.iter().enumerate() {
            for other in &ids[index + 1..] {
                assert_ne!(
                    id, other,
                    "block IDs are distinct and read from the snapshot"
                );
            }
        }
        assert_eq!(left.text, "Ж🙂");
        assert_eq!(right.text, "Б\u{301}Я");
        assert_eq!(
            left.text.chars().count(),
            2,
            "the end boundary uses scalar offsets"
        );
        assert_eq!(left.paragraph_style.alignment(), Alignment::Center);
        assert_eq!(right.paragraph_style.alignment(), Alignment::Right);
        assert_eq!(left.runs.len(), 2);
        assert_eq!(left.runs[0].text, "Ж");
        assert_eq!(left.runs[0].style, TextStyle::default().with_bold(true));
        assert_eq!(left.runs[1].text, "🙂");
        assert_eq!(left.runs[1].style, TextStyle::default().with_italic(true));
        assert_eq!(right.runs.len(), 2);
        assert_eq!(right.runs[0].text, "Б\u{301}");
        assert_eq!(
            right.runs[0].style,
            TextStyle::default().with_underline(true)
        );
        assert_eq!(right.runs[1].text, "Я");
        assert_eq!(
            right.runs[1].style,
            TextStyle::default().with_foreground(Some(Rgba8::rgb(0, 87, 184)))
        );
        assert_eq!(page_break.text, "");
        assert!(page_break.runs.is_empty());
        assert_eq!(page2.text, "Учебная страница 😀");
        assert_eq!(page2.paragraph_style.alignment(), Alignment::Left);
        assert_eq!(page2.runs.len(), 3);
        assert_eq!(page2.runs[0].text, "Учебная ");
        assert_eq!(page2.runs[0].style, TextStyle::default());
        assert_eq!(page2.runs[1].text, "страница ");
        assert_eq!(
            page2.runs[1].style,
            TextStyle::default().with_underline(true)
        );
        assert_eq!(page2.runs[2].text, "😀");
        assert_eq!(
            page2.runs[2].style,
            TextStyle::default()
                .with_bold(true)
                .with_foreground(Some(Rgba8::rgb(0, 87, 184)))
        );
        assert_eq!(imported.projection.source.page_breaks, 1);
        assert_eq!(imported.projection.projected.page_break_blocks, 1);
    }

    // Model-only contract for the imported public fixture. This does not send
    // a keyboard event and is not evidence of a real GUI editing path.
    #[test]
    fn public_editing_pages_model_join_preserves_page_two_and_history() {
        let imported = DocxImporter
            .import(include_bytes!(
                "../../../../fixtures/public/writer/synthetic-editing-pages.docx"
            ))
            .expect("import public editing-pages DOCX");
        assert_eq!(imported.blocks.len(), 4);
        let before_blocks = imported.blocks.clone();
        let left = before_blocks[0].text_block().expect("left paragraph");
        let right = before_blocks[1].text_block().expect("right paragraph");
        assert_eq!(left.kind, BlockKind::Paragraph);
        assert_eq!(right.kind, BlockKind::Paragraph);
        assert_eq!(
            before_blocks[2].text_block().unwrap().kind,
            BlockKind::PageBreak
        );
        assert_eq!(
            before_blocks[3].text_block().unwrap().kind,
            BlockKind::Paragraph
        );
        let left_id = left.id;
        let right_id = right.id;
        let left_style = left.paragraph_style.clone();
        let expected_runs = left
            .runs
            .iter()
            .chain(&right.runs)
            .cloned()
            .collect::<Vec<_>>();
        let boundary = Selection::collapsed(Cursor::new(
            TextContainerId::Block(left_id),
            TextOffset::new(left.text.chars().count()),
        ));
        assert_eq!(left.text.chars().count(), 2);
        let mut editor = Editor::new();
        editor
            .replace_document(imported)
            .expect("load imported model");
        editor.clear_history();
        editor
            .set_selection(boundary)
            .expect("place model selection");
        let revision = editor.revision().get();
        editor
            .execute(Command::DeleteForward)
            .expect("join adjacent paragraphs");
        let merged = editor.snapshot();
        assert_eq!(merged.revision.get(), revision + 1);
        assert_eq!(merged.blocks.len(), 3);
        let merged_left = merged.blocks[0].text_block().unwrap();
        assert_eq!(merged_left.id, left_id);
        assert_ne!(merged_left.id, right_id);
        assert!(
            merged
                .blocks
                .iter()
                .all(|block| block.text_block().is_none_or(|text| text.id != right_id))
        );
        assert_eq!(merged_left.kind, BlockKind::Paragraph);
        assert_eq!(merged_left.text, "Ж🙂Б\u{301}Я");
        assert_eq!(merged_left.paragraph_style, left_style);
        assert_eq!(merged_left.runs, expected_runs);
        assert_eq!(
            merged.blocks[1], before_blocks[2],
            "page-break ID and content stay unchanged"
        );
        assert_eq!(
            merged.blocks[2], before_blocks[3],
            "page-two ID, runs and style stay unchanged"
        );
        assert_eq!(editor.selection(), boundary);
        assert!(editor.can_undo());
        assert!(!editor.can_redo());
        assert!(editor.undo().unwrap());
        assert_eq!(editor.revision().get(), revision + 2);
        assert_eq!(editor.snapshot().blocks, before_blocks);
        assert_eq!(editor.selection(), boundary);
        assert!(editor.can_redo());
        assert!(editor.redo().unwrap());
        assert_eq!(editor.revision().get(), revision + 3);
        assert_eq!(editor.snapshot().blocks, merged.blocks);
        assert_eq!(editor.selection(), boundary);
        assert!(!editor.can_redo());
    }

    #[test]
    fn projection_diagnostics_make_textual_fallbacks_explicit() {
        let document = DocumentIR {
            sections: vec![office_oxide::ir::Section {
                elements: vec![
                    IrElement::ThematicBreak,
                    IrElement::ColumnBreak,
                    IrElement::Shape(office_oxide::ir::Shape::default()),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };

        let projected = OfficeOxideProjector::new()
            .project_with_diagnostics(&document)
            .expect("project fallback fixture");

        assert_eq!(projected.diagnostics.source.thematic_breaks, 1);
        assert_eq!(projected.diagnostics.source.column_breaks, 1);
        assert_eq!(projected.diagnostics.source.shapes, 1);
        assert_eq!(projected.diagnostics.fallback.thematic_breaks, 1);
        assert_eq!(projected.diagnostics.fallback.column_breaks, 1);
        assert_eq!(projected.diagnostics.fallback.shapes, 1);
        assert_eq!(projected.diagnostics.projected.fallback_blocks, 3);
        assert!(projected.diagnostics.has_fallback_content());
    }

    #[test]
    fn real_docx_preflight_contract_when_path_is_provided() {
        let Ok(path) = std::env::var("RUDRA_WRITER_ACCEPTANCE_DOCX") else {
            return;
        };
        let bytes = std::fs::read(&path).expect("read real DOCX acceptance fixture");
        let preflight = preflight_standard_docx(&bytes);
        assert!(
            matches!(&preflight, Ok(true)),
            "preflight rejected {path:?}: {preflight:?}"
        );
        assert!(
            is_bounded_standard_docx(&bytes),
            "bounded standard DOCX check rejected {path:?}"
        );
        let imported = DocxImporter
            .import_with_layout(&bytes)
            .expect("import real DOCX acceptance fixture with layout");
        assert!(
            !imported.layout.sections.is_empty(),
            "standard DOCX import lost its page layout for {path:?}"
        );
    }

    #[test]
    fn is_bounded_standard_docx_accepts_minimal_valid_zip() {
        let package = opc::DocxPackage {
            entries: vec![
                opc::ZipEntry {
                    name: PART_CONTENT_TYPES.to_string(),
                    content_type: CONTENT_TYPES.to_string(),
                    data: FlatOpcData::Xml(
                        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_string(),
                    ),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_DOCUMENT.to_string(),
                    content_type: DOCUMENT.to_string(),
                    data: FlatOpcData::Xml(
                        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Hello</w:t></w:r></w:p></w:body></w:document>"#.to_string(),
                    ),
                    compression: opc::ZipCompression::Stored,
                },
            ],
        };
        let bytes = opc::serialize_zip(&package).unwrap();
        assert!(is_bounded_standard_docx(&bytes));
    }

    #[test]
    fn is_bounded_standard_docx_rejects_unsafe_optional_entry_names() {
        let package = opc::DocxPackage {
            entries: vec![
                opc::ZipEntry {
                    name: PART_CONTENT_TYPES.to_string(),
                    content_type: CONTENT_TYPES.to_string(),
                    data: FlatOpcData::Xml(
                        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_string(),
                    ),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: PART_DOCUMENT.to_string(),
                    content_type: DOCUMENT.to_string(),
                    data: FlatOpcData::Xml(
                        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Hello</w:t></w:r></w:p></w:body></w:document>"#.to_string(),
                    ),
                    compression: opc::ZipCompression::Stored,
                },
                opc::ZipEntry {
                    name: "word/media/image1.png".to_string(),
                    content_type: PNG.to_string(),
                    data: FlatOpcData::Binary(vec![0x89, 0x50, 0x4E, 0x47]),
                    compression: opc::ZipCompression::Stored,
                },
            ],
        };
        let safe_image_name = "word/media/image1.png";
        let base_bytes = opc::serialize_zip(&package).unwrap();

        let cases = [
            "word/media/ima\0e1.png",
            "/ord/media/image1.png",
            "word\\media/image1.png",
            "word/../media/abc.png",
        ];
        for bad_name in cases {
            assert_eq!(safe_image_name.len(), bad_name.len());
            let mut bytes = base_bytes.clone();
            patch_zip_entry_name(&mut bytes, safe_image_name, bad_name);
            assert!(
                !is_bounded_standard_docx(&bytes),
                "expected rejection for unsafe name: {bad_name:?}"
            );
        }
    }

    fn patch_zip_entry_name(bytes: &mut [u8], old_name: &str, new_name: &str) {
        assert_eq!(old_name.len(), new_name.len());
        let old_bytes = old_name.as_bytes();
        let new_bytes = new_name.as_bytes();
        let mut count = 0usize;
        let mut start = 0usize;
        while let Some(pos) = bytes[start..]
            .windows(old_bytes.len())
            .position(|w| w == old_bytes)
        {
            let idx = start + pos;
            bytes[idx..idx + old_bytes.len()].copy_from_slice(new_bytes);
            start = idx + old_bytes.len();
            count += 1;
        }
        assert_eq!(count, 2, "expected exactly 2 occurrences, found {count}");
    }

    // ------------------------------------------------------------------
    // T14.1 fixture machinery + safe controls
    //
    // Reusable test-only machinery that builds a safe *stored* ZIP body whose
    // package is a valid DOCX plus an unreferenced safe optional part
    // (`extra/part.bin`). The optional part name is deliberately chosen so a
    // future adversarial test can raw-patch an equal-length unsafe name into
    // the exact local-file-header + central-directory occurrences via
    // `patch_zip_entry_name` (which is scan-based, checks equal lengths and
    // asserts exactly two mutations). An unsafe name is never serialized here.
    // ------------------------------------------------------------------

    const SAFE_OPTIONAL_PART: &str = "extra/part.bin";

    /// Re-serialize exporter output as a fully *stored* package and append the
    /// unreferenced safe optional part. Only safe names are ever serialized:
    /// the raw-mutation step that T14.2 needs is deliberately kept out of here.
    fn stored_rewrap_with_optional_part(mut entries: Vec<opc::ZipEntry>) -> Vec<u8> {
        for entry in &mut entries {
            entry.compression = opc::ZipCompression::Stored;
        }
        entries.push(opc::ZipEntry {
            name: SAFE_OPTIONAL_PART.to_string(),
            content_type: String::new(),
            data: FlatOpcData::Binary(b"unreferenced-safe-optional".to_vec()),
            compression: opc::ZipCompression::Stored,
        });
        opc::serialize_zip(&opc::DocxPackage { entries }).unwrap()
    }

    /// The real `DocxExporter` output re-read into OPC entries. The exporter
    /// emits deflated entries, so this goes through the `zip` crate (not
    /// `opc::parse_zip`, which is stored-only) and re-declares everything as
    /// stored for `stored_rewrap_with_optional_part`.
    fn standard_export_entries() -> Vec<opc::ZipEntry> {
        use std::io::Read as _;

        let bytes = DocxExporter.export(&sample()).unwrap();
        let mut archive = ZipArchive::new(IoCursor::new(bytes)).unwrap();
        let mut entries = Vec::with_capacity(archive.len());
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).unwrap();
            let name = file.name().to_string();
            let mut raw = Vec::new();
            file.read_to_end(&mut raw).unwrap();
            let is_text_part = name.ends_with(".xml") || name.ends_with(".rels");
            let data = match String::from_utf8(raw.clone()) {
                Ok(xml) if is_text_part => FlatOpcData::Xml(xml),
                _ => FlatOpcData::Binary(raw),
            };
            entries.push(opc::ZipEntry {
                name,
                content_type: if is_text_part {
                    XML.to_string()
                } else {
                    String::new()
                },
                data,
                compression: opc::ZipCompression::Stored,
            });
        }
        entries
    }

    /// The same valid standard body, except the document part carries the
    /// `urn:rudra:writer` marker so `import_with_options` routes it through the
    /// legacy `read_document` path instead of the Office Oxide path.
    fn legacy_export_entries() -> Vec<opc::ZipEntry> {
        let mut entries = standard_export_entries();
        for entry in &mut entries {
            if entry.name == PART_DOCUMENT {
                let FlatOpcData::Xml(xml) = &mut entry.data else {
                    panic!("standard export document part must be XML");
                };
                *xml = inject_rudra_marker(xml);
            }
        }
        entries
    }

    /// Insert `xmlns:rw="urn:rudra:writer"` into the root `<w:document ...>`
    /// start tag of a standard OOXML document part.
    fn inject_rudra_marker(xml: &str) -> String {
        let marker = format!(" xmlns:rw=\"{RUDRA_NS}\"");
        let start = xml
            .find("<w:document")
            .unwrap_or_else(|| panic!("standard export document part has no <w:document root"));
        let after_prefix = &xml[start + "<w:document".len()..];
        let tag_end = after_prefix.find('>').unwrap_or_else(|| {
            panic!("standard export document part has unterminated root start tag")
        });
        let insert_at = start + "<w:document".len() + tag_end;
        let mut out = String::with_capacity(xml.len() + marker.len());
        out.push_str(&xml[..insert_at]);
        out.push_str(&marker);
        out.push_str(&xml[insert_at..]);
        out
    }

    /// Import a safe control fixture, asserting the import neither panics nor
    /// errors. `catch_unwind` turns a future routing panic into a loud failure
    /// here instead of a silent test abort.
    fn import_safe_control(bytes: &[u8], options: DocxImportOptions) -> DocumentSnapshot {
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
            DocxImporter.import_with_options(bytes, options)
        }));
        outcome
            .unwrap_or_else(|_| panic!("import panicked on a safe control fixture"))
            .unwrap_or_else(|err| panic!("safe control fixture failed to import: {err:?}"))
    }

    #[test]
    fn safe_optional_part_fixture_is_stored_safe_and_patchable() {
        let bytes = stored_rewrap_with_optional_part(standard_export_entries());
        assert!(opc::zip_name_is_safe(SAFE_OPTIONAL_PART));
        // The optional part name must appear exactly twice: the local file
        // header and the central directory entry. This is the precondition the
        // raw patch helper relies on (assert_eq!(count, 2)).
        let occurrences = bytes
            .windows(SAFE_OPTIONAL_PART.len())
            .filter(|window| *window == SAFE_OPTIONAL_PART.as_bytes())
            .count();
        assert_eq!(
            occurrences, 2,
            "expected exactly 2 raw occurrences (local + central), found {occurrences}"
        );
    }

    #[test]
    fn safe_control_legacy_body_imports_via_read_document() {
        let bytes = stored_rewrap_with_optional_part(legacy_export_entries());
        for options in [
            DocxImportOptions::default(),
            DocxImportOptions { strict: true },
        ] {
            let imported = import_safe_control(&bytes, options);
            let text = document_to_plain_text(&imported);
            assert!(
                text.contains("Hello <Rudra> & world"),
                "legacy control must preserve sample text, got: {text:?}"
            );
        }
    }

    #[test]
    fn safe_control_standard_body_imports_via_office_oxide() {
        let bytes = stored_rewrap_with_optional_part(standard_export_entries());
        for options in [
            DocxImportOptions::default(),
            DocxImportOptions { strict: true },
        ] {
            let imported = import_safe_control(&bytes, options);
            let text = document_to_plain_text(&imported);
            assert!(
                text.contains("Hello") && text.contains("Rudra") && text.contains("world"),
                "standard control must preserve sample text tokens, got: {text:?}"
            );
        }
    }

    type BodyBuilder = fn() -> Vec<opc::ZipEntry>;

    #[test]
    fn unsafe_optional_entry_names_reject_import_standard_and_legacy_default_and_strict() {
        let name_classes = [
            ("nul", "extra/part.bin", "extra/part\0bin"),
            ("leading_slash", "Xextra/part.bin", "/extra/part.bin"),
            ("backslash", "extra/part.bin", "extra\\part.bin"),
            ("parent_segment", "extra/XZ/part.bin", "extra/../part.bin"),
        ];
        let bodies: &[(&str, BodyBuilder)] = &[
            ("standard", standard_export_entries),
            ("legacy", legacy_export_entries),
        ];
        let option_sets = [
            ("default", DocxImportOptions::default()),
            ("strict", DocxImportOptions { strict: true }),
        ];

        for (name_class, safe_name, bad_name) in name_classes {
            assert_eq!(
                safe_name.len(),
                bad_name.len(),
                "equal-length precondition failed for {name_class}"
            );
            for &(body_label, body_fn) in bodies {
                let mut entries = body_fn();
                for entry in &mut entries {
                    entry.compression = opc::ZipCompression::Stored;
                }
                entries.push(opc::ZipEntry {
                    name: safe_name.to_string(),
                    content_type: String::new(),
                    data: FlatOpcData::Binary(b"unreferenced-safe-optional".to_vec()),
                    compression: opc::ZipCompression::Stored,
                });
                let safe_bytes = opc::serialize_zip(&opc::DocxPackage { entries }).unwrap();
                for (options_label, options) in option_sets {
                    let mut bytes = safe_bytes.clone();
                    patch_zip_entry_name(&mut bytes, safe_name, bad_name);
                    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
                        DocxImporter.import_with_options(&bytes, options)
                    }));
                    assert!(
                        outcome.is_ok(),
                        "import panicked for {name_class}/{body_label}/{options_label}"
                    );
                    let result = outcome.unwrap();
                    assert!(
                        result.is_err(),
                        "expected error for {name_class}/{body_label}/{options_label}, got Ok"
                    );
                    let err = result.unwrap_err();
                    assert_eq!(
                        err.kind,
                        DocxErrorKind::InvalidPackage,
                        "expected InvalidPackage for {name_class}/{body_label}/{options_label}, got {:?}",
                        err.kind
                    );
                    assert!(
                        err.message.contains("unsafe ZIP entry name"),
                        "expected unsafe ZIP entry name message for {name_class}/{body_label}/{options_label}, got: {}",
                        err.message
                    );
                }
            }
        }
    }
}
