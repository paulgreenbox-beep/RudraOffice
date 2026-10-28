#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;

use writer_core::{
    Command, CoreError, DocumentFormat, DocumentSnapshot, EditError, Editor, SearchError,
    SearchMatch, Selection, StyledTextFragment, TextStylePatch,
};
use writer_docx::{DocxError, DocxExporter, DocxImporter};
use writer_rudra_adapter::{
    LayoutError, LayoutMode, LayoutSettings, LayoutSnapshot, RudraLayoutEngine,
};

pub mod core {
    pub use writer_core::*;
}
pub mod layout {
    pub use writer_rudra_adapter::*;
}
pub use writer_docx::{DocxLayoutMetadata, DocxProjectionDiagnostics};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportedDocument {
    Docx(Vec<u8>),
    Pdf(Vec<u8>),
    PlainText(String),
    Markdown(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiError {
    Edit(EditError),
    Search(SearchError),
    Core(CoreError),
    Layout(LayoutError),
    Docx(DocxError),
    Pdf(String),
    UnsupportedFormat,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Edit(error) => write!(f, "edit error: {error}"),
            Self::Search(error) => write!(f, "search error: {error}"),
            Self::Core(error) => write!(f, "core error: {error}"),
            Self::Layout(error) => write!(f, "layout error: {error}"),
            Self::Docx(error) => write!(f, "DOCX error: {error}"),
            Self::Pdf(error) => write!(f, "PDF error: {error}"),
            Self::UnsupportedFormat => f.write_str("unsupported export format"),
        }
    }
}

impl Error for ApiError {}
impl From<EditError> for ApiError {
    fn from(error: EditError) -> Self {
        Self::Edit(error)
    }
}
impl From<SearchError> for ApiError {
    fn from(error: SearchError) -> Self {
        Self::Search(error)
    }
}
impl From<LayoutError> for ApiError {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}
impl From<DocxError> for ApiError {
    fn from(error: DocxError) -> Self {
        Self::Docx(error)
    }
}
impl From<std::string::FromUtf8Error> for ApiError {
    fn from(error: std::string::FromUtf8Error) -> Self {
        Self::Pdf(error.to_string())
    }
}

pub struct WriterApi {
    editor: Editor,
    layout_settings: LayoutSettings,
    docx_layout: DocxLayoutMetadata,
    docx_projection: DocxProjectionDiagnostics,
}

impl Default for WriterApi {
    fn default() -> Self {
        Self::new()
    }
}

impl WriterApi {
    pub fn new() -> Self {
        Self {
            editor: Editor::new(),
            layout_settings: LayoutSettings::default(),
            docx_layout: DocxLayoutMetadata::default(),
            docx_projection: DocxProjectionDiagnostics::default(),
        }
    }

    pub fn from_snapshot(snapshot: DocumentSnapshot) -> Result<Self, ApiError> {
        let mut editor = Editor::new();
        editor.replace_document(snapshot)?;
        editor.clear_history();
        Ok(Self {
            editor,
            layout_settings: LayoutSettings::default(),
            docx_layout: DocxLayoutMetadata::default(),
            docx_projection: DocxProjectionDiagnostics::default(),
        })
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }
    pub fn editor_mut(&mut self) -> &mut Editor {
        &mut self.editor
    }
    pub fn document(&self) -> DocumentSnapshot {
        self.editor.snapshot()
    }
    pub fn selection(&self) -> Selection {
        self.editor.selection()
    }
    pub fn set_selection(&mut self, selection: Selection) -> Result<(), ApiError> {
        self.editor.set_selection(selection)?;
        Ok(())
    }

    pub fn execute(&mut self, command: Command) -> Result<(), ApiError> {
        self.editor.execute(command)?;
        Ok(())
    }

    pub fn insert_text(&mut self, text: impl Into<String>) -> Result<(), ApiError> {
        self.execute(Command::InsertText { text: text.into() })
    }

    pub fn delete_backward(&mut self) -> Result<(), ApiError> {
        self.execute(Command::DeleteBackward)
    }

    pub fn delete_forward(&mut self) -> Result<(), ApiError> {
        self.execute(Command::DeleteForward)
    }

    pub fn apply_text_style(&mut self, patch: TextStylePatch) -> Result<(), ApiError> {
        let range = self
            .selection()
            .range()
            .map(|(_, range)| range)
            .ok_or(EditError::InvalidSelection)?;
        self.execute(Command::SetTextStyle { range, patch })
    }

    pub fn undo(&mut self) -> Result<bool, ApiError> {
        Ok(self.editor.undo()?)
    }
    pub fn redo(&mut self) -> Result<bool, ApiError> {
        Ok(self.editor.redo()?)
    }
    pub fn can_undo(&self) -> bool {
        self.editor.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.editor.can_redo()
    }

    pub fn copy(&self) -> Option<StyledTextFragment> {
        self.editor.copy()
    }

    pub fn paste(&mut self, content: core::ClipboardContent) -> Result<(), ApiError> {
        self.execute(Command::Paste { content })
    }

    pub fn find(&self, query: impl Into<core::SearchQuery>) -> Result<Vec<SearchMatch>, ApiError> {
        Ok(self.editor.find(query)?)
    }

    pub fn replace_all(
        &mut self,
        query: impl Into<core::SearchQuery>,
        replacement: &str,
    ) -> Result<usize, ApiError> {
        Ok(self.editor.replace_all(query, replacement)?)
    }

    pub fn set_layout_settings(&mut self, settings: LayoutSettings) {
        self.layout_settings = settings;
    }

    pub fn layout_settings(&self) -> LayoutSettings {
        self.layout_settings
    }

    pub fn docx_layout(&self) -> &DocxLayoutMetadata {
        &self.docx_layout
    }

    /// Updates the imported header position for the current in-memory session.
    /// DOCX persistence remains owned by the exporter contract.
    pub fn set_header_distance_half_points(&mut self, value: u32) -> bool {
        let Some(section) = self.docx_layout.sections.first_mut() else {
            return false;
        };
        let value_twips = value.saturating_mul(10);
        if section.header_distance_twips == value_twips {
            return false;
        }
        section.header_distance_twips = value_twips;
        true
    }

    /// Updates the imported footer distance from the page bottom for this session.
    pub fn set_footer_distance_half_points(&mut self, value: u32) -> bool {
        let Some(section) = self.docx_layout.sections.first_mut() else {
            return false;
        };
        let value_twips = value.saturating_mul(10);
        if section.footer_distance_twips == value_twips {
            return false;
        }
        section.footer_distance_twips = value_twips;
        true
    }

    pub fn docx_projection(&self) -> &DocxProjectionDiagnostics {
        &self.docx_projection
    }

    pub fn layout(&self) -> Result<LayoutSnapshot, ApiError> {
        Ok(RudraLayoutEngine::new(self.layout_settings).layout_document(&self.document())?)
    }

    pub fn set_layout_mode(&mut self, mode: LayoutMode) {
        self.layout_settings.mode = mode;
    }

    pub fn set_zoom_percent(&mut self, percent: u16) {
        self.layout_settings.zoom_percent = percent;
    }

    pub fn export(&self, format: DocumentFormat) -> Result<ExportedDocument, ApiError> {
        let document = self.document();
        match format {
            DocumentFormat::Docx => Ok(ExportedDocument::Docx(DocxExporter.export(&document)?)),
            DocumentFormat::PlainText => Ok(ExportedDocument::PlainText(
                core::document_to_plain_text(&document),
            )),
            DocumentFormat::Markdown => Ok(ExportedDocument::Markdown(core::document_to_markdown(
                &document,
            ))),
            DocumentFormat::Pdf => {
                let pdf = Self::export_pdf(&document)?;
                Ok(ExportedDocument::Pdf(pdf))
            }
        }
    }

    pub fn export_docx(&self) -> Result<Vec<u8>, ApiError> {
        match self.export(DocumentFormat::Docx)? {
            ExportedDocument::Docx(bytes) => Ok(bytes),
            _ => Err(ApiError::UnsupportedFormat),
        }
    }

    pub fn load_docx(&mut self, bytes: &[u8]) -> Result<(), ApiError> {
        let imported = DocxImporter.import_with_layout(bytes)?;
        self.editor.replace_document(imported.snapshot)?;
        self.editor.clear_history();
        self.docx_layout = imported.layout;
        self.docx_projection = imported.projection;
        if let Some(section) = self.docx_layout.sections.first()
            && section.width_twips > 0
            && section.height_twips > 0
        {
            self.layout_settings = self.layout_settings.with_docx_page_setup(
                section.width_twips,
                section.height_twips,
                section.landscape,
                writer_rudra_adapter::PageMargins::from_twips(
                    section.margin_top_twips,
                    section.margin_right_twips,
                    section.margin_bottom_twips,
                    section.margin_left_twips,
                ),
            );
        }
        Ok(())
    }

    pub fn from_docx(bytes: &[u8]) -> Result<Self, ApiError> {
        let mut api = Self::new();
        api.load_docx(bytes)?;
        Ok(api)
    }

    fn export_pdf(document: &DocumentSnapshot) -> Result<Vec<u8>, ApiError> {
        let mut pdf = Vec::new();
        // Minimal deterministic PDF: header, one page with text content, cross-reference table, trailer.
        pdf.extend_from_slice(b"%PDF-1.4\n");
        let header_len = pdf.len();
        let body = format_docx_pdf_body(document);
        let body_bytes = body.as_bytes();
        pdf.extend_from_slice(body_bytes);
        let xref_offset = pdf.len();
        let xref = format_xref(xref_offset, body_bytes, header_len);
        pdf.extend_from_slice(xref.as_bytes());
        let trailer = format_trailer(xref_offset);
        pdf.extend_from_slice(trailer.as_bytes());
        Ok(pdf)
    }
}

/// Escapes a string for use as a PDF literal string in a single pass.
/// Handles backslash, parentheses, CR, LF, tab, backspace, and form feed.
/// Non-ASCII scalar values are replaced with '?' because Helvetica (a base
/// Type 1 font) has no glyphs for them and we do not embed external fonts.
fn pdf_escape_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0C' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7E => out.push('?'),
            c => out.push(c),
        }
    }
    out
}

fn format_docx_pdf_body(document: &DocumentSnapshot) -> String {
    let mut body = String::new();
    body.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    body.push_str("2 0 obj\n<< /Type /Pages /Kids [");
    body.push_str("3 0 R");
    body.push_str("] /Count 1 >>\nendobj\n");
    let page_content = format_page_content(document);
    body.push_str(
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>\nendobj\n",
    );
    body.push_str("4 0 obj\n");
    let content_bytes = page_content.as_bytes();
    body.push_str(&format!("<< /Length {} >>\nstream\n", content_bytes.len()));
    body.push_str(&page_content);
    body.push_str("\nendstream\nendobj\n");
    body
}

fn format_page_content(document: &DocumentSnapshot) -> String {
    let mut content = String::new();
    content.push_str("BT\n/F1 12 Tf\n72 720 Td\n");
    for block in &document.blocks {
        let text = match block {
            core::BlockSnapshot::Text(t) => t.text.clone(),
            core::BlockSnapshot::Table(t) => {
                let mut s = String::new();
                for row in &t.rows {
                    for cell in &row.cells {
                        s.push_str(&cell.text);
                        s.push(' ');
                    }
                    s.push('\n');
                }
                s
            }
            core::BlockSnapshot::Image(_) => "[Image]\n".to_string(),
        };
        let escaped = pdf_escape_literal(&text);
        content.push_str(&format!("({}) Tj\n", escaped));
        content.push_str("0 -14 Td\n");
    }
    content.push_str("ET\n");
    content
}

fn format_xref(_xref_offset: usize, body_bytes: &[u8], header_len: usize) -> String {
    let mut s = String::new();
    s.push_str("xref\n");
    s.push_str(&format!("0 {}\n", 5));
    s.push_str("0000000000 65535 f \n");
    for i in 1..=4 {
        let marker = format!("{} 0 obj\n", i);
        let marker_bytes = marker.as_bytes();
        let offset = find_bytes(body_bytes, marker_bytes)
            .map(|pos| pos as u64 + header_len as u64)
            .unwrap_or(0);
        s.push_str(&format!("{:010} 00000 n \n", offset));
    }
    s
}

/// Finds the first occurrence of `needle` in `haystack` and returns its byte offset.
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn format_trailer(xref_offset: usize) -> String {
    format!(
        "trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        xref_offset
    )
}

#[cfg(test)]
mod tests {
    use self::core::{BlockId, Cursor, TextContainerId, TextOffset};
    use super::*;

    const SYNTHETIC_BASIC_DOCX: &[u8] =
        include_bytes!("../../../../fixtures/public/writer/synthetic-basic.docx");

    #[cfg(feature = "private-fixtures")]
    fn private_layout_docx_bytes() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/release-acceptance/docx/writer-layout-a4-multipage.docx");
        std::fs::read(&path).unwrap_or_else(|error| {
            panic!("failed to read private DOCX fixture at {path:?}: {error}")
        })
    }

    fn assert_imported_docx_is_clean_undo_baseline_and_first_edit_redoes(mut api: WriterApi) {
        let imported = api.document();

        assert!(!api.can_undo(), "DOCX import must start a clean history");
        assert!(!api.can_redo());

        api.insert_text(" regression edit").unwrap();
        let edited = api.document();
        assert_ne!(edited.blocks, imported.blocks);
        assert!(api.can_undo(), "the first user edit must be undoable");
        assert!(!api.can_redo());

        assert!(api.undo().unwrap());
        assert_eq!(api.document().blocks, imported.blocks);
        assert!(!api.can_undo(), "undo must stop at the imported baseline");
        assert!(api.can_redo());

        assert!(api.redo().unwrap());
        assert_eq!(api.document().blocks, edited.blocks);
        assert!(api.can_undo());
        assert!(!api.can_redo());
    }

    fn assert_page_break_starts_following_content_on_new_page(api: &WriterApi, marker: &str) {
        let document = api.document();
        let layout = api.layout().expect("layout controlled DOCX fixture");

        assert_eq!(
            layout.page_count(),
            2,
            "explicit DOCX page break must create page 2"
        );
        assert!(
            layout.pages[1]
                .blocks
                .iter()
                .filter_map(|layout_block| document.blocks.get(layout_block.source_block_index))
                .filter_map(core::BlockSnapshot::text_block)
                .any(|block| block.text.contains(marker))
        );
    }

    fn assert_docx_header_footer_distances_are_editable_in_session(mut api: WriterApi) {
        assert!(!api.docx_layout().sections.is_empty());
        assert!(api.set_header_distance_half_points(73));
        assert!(api.set_footer_distance_half_points(85));
        let section = &api.docx_layout().sections[0];
        assert_eq!(section.header_distance_twips, 730);
        assert_eq!(section.footer_distance_twips, 850);
        assert!(!api.set_header_distance_half_points(73));
        assert!(!api.set_footer_distance_half_points(85));
    }

    fn nonempty_text_blocks(document: &core::DocumentSnapshot) -> Vec<&core::TextBlockSnapshot> {
        document
            .blocks
            .iter()
            .filter_map(core::BlockSnapshot::text_block)
            .filter(|block| !block.text.is_empty())
            .collect()
    }

    #[test]
    fn facade_covers_edit_undo_redo_and_export() {
        let mut api = WriterApi::new();
        api.insert_text("Hello").unwrap();
        assert_eq!(api.document().blocks[0].text(), Some("Hello"));
        assert!(api.can_undo());
        api.undo().unwrap();
        assert_eq!(api.document().blocks[0].text(), Some(""));
        api.redo().unwrap();
        let text = match api.export(DocumentFormat::PlainText).unwrap() {
            ExportedDocument::PlainText(text) => text,
            _ => unreachable!(),
        };
        assert_eq!(text, "Hello");
        assert!(!api.export_docx().unwrap().is_empty());
    }

    #[test]
    fn facade_exposes_layout_search_replace_and_style() {
        let mut api = WriterApi::new();
        api.insert_text("hello hello").unwrap();
        assert_eq!(api.find("hello").unwrap().len(), 2);
        assert_eq!(api.replace_all("hello", "hi").unwrap(), 2);
        api.set_selection(Selection::new(
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(0)),
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(2)),
        ))
        .unwrap();
        api.apply_text_style(TextStylePatch::new().bold(true))
            .unwrap();
        assert!(
            api.document().blocks[0].text_block().unwrap().runs[0]
                .style
                .bold()
        );
        assert_eq!(api.layout().unwrap().block_count(), 1);
    }

    #[test]
    fn pdf_export_is_deterministic() {
        let mut api = WriterApi::new();
        api.insert_text("Hello PDF").unwrap();
        let pdf1 = match api.export(DocumentFormat::Pdf).unwrap() {
            ExportedDocument::Pdf(bytes) => bytes,
            _ => unreachable!(),
        };
        let pdf2 = match api.export(DocumentFormat::Pdf).unwrap() {
            ExportedDocument::Pdf(bytes) => bytes,
            _ => unreachable!(),
        };
        assert_eq!(pdf1, pdf2);
        assert!(pdf1.starts_with(b"%PDF-1.4"));
    }

    #[test]
    fn pdf_xref_offsets_point_to_valid_obj_markers() {
        let mut api = WriterApi::new();
        api.insert_text("Hello PDF").unwrap();
        let pdf = match api.export(DocumentFormat::Pdf).unwrap() {
            ExportedDocument::Pdf(bytes) => bytes,
            _ => unreachable!(),
        };
        let pdf_str = String::from_utf8_lossy(&pdf);
        // Parse startxref
        let startxref_pos = pdf_str
            .rfind("startxref\n")
            .expect("startxref marker present");
        let after_startxref = &pdf_str[startxref_pos + "startxref\n".len()..];
        let xref_offset: usize = after_startxref
            .lines()
            .next()
            .expect("xref offset line")
            .trim()
            .parse()
            .expect("xref offset is a number");

        // Verify startxref points exactly to the xref section
        assert_eq!(
            &pdf[xref_offset..xref_offset + 4],
            b"xref",
            "startxref must point to xref keyword"
        );

        // Parse xref entries and verify each offset points to "N 0 obj"
        let xref_section = &pdf_str[xref_offset..];
        let mut lines = xref_section.lines();
        let header = lines.next().expect("xref header line");
        assert!(header.starts_with("xref"));
        let count_line = lines.next().expect("xref count line");
        let parts: Vec<&str> = count_line.split_whitespace().collect();
        assert_eq!(parts[0], "0");
        let num_objects: usize = parts[1].parse().expect("object count");

        for _ in 0..num_objects {
            let entry_line = lines.next().expect("xref entry line");
            let entry_parts: Vec<&str> = entry_line.split_whitespace().collect();
            let offset: usize = entry_parts[0].parse().expect("xref offset is a number");
            let generation: &str = entry_parts[1];
            let use_flag: &str = entry_parts[2];

            // Skip the free entry (object 0); only validate in-use objects.
            if use_flag == "f" {
                assert_eq!(generation, "65535");
                continue;
            }

            // In-use entries must use generation 00000.
            assert_eq!(generation, "00000");

            // Verify the byte at offset points to "N 0 obj".
            let obj_marker = &pdf[offset..];
            let obj_str = String::from_utf8_lossy(obj_marker);
            assert!(
                obj_str.starts_with("1 0 obj")
                    || obj_str.starts_with("2 0 obj")
                    || obj_str.starts_with("3 0 obj")
                    || obj_str.starts_with("4 0 obj"),
                "xref entry at offset {} should point to valid obj marker, got: {}",
                offset,
                obj_str.trim()
            );
        }
    }

    #[test]
    fn pdf_escape_literal_handles_all_required_characters() {
        // Backslash
        assert_eq!(pdf_escape_literal(r"a\b"), "a\\\\b");
        // Open paren
        assert_eq!(pdf_escape_literal("a(b"), "a\\(b");
        // Close paren
        assert_eq!(pdf_escape_literal("a)b"), "a\\)b");
        // CR
        assert_eq!(pdf_escape_literal("a\rb"), "a\\rb");
        // LF
        assert_eq!(pdf_escape_literal("a\nb"), "a\\nb");
        // Tab
        assert_eq!(pdf_escape_literal("a\tb"), "a\\tb");
        // Backspace
        assert_eq!(pdf_escape_literal("a\x08b"), "a\\bb");
        // Form feed
        assert_eq!(pdf_escape_literal("a\x0Cb"), "a\\fb");
        // Non-ASCII replaced with ?
        assert_eq!(pdf_escape_literal("héllo"), "h?llo");
        // Mixed
        assert_eq!(
            pdf_escape_literal("line1\nline2\ttab"),
            "line1\\nline2\\ttab"
        );
    }

    #[test]
    fn pdf_escape_literal_is_single_pass() {
        // A single backslash should become two backslashes, not four
        assert_eq!(pdf_escape_literal("\\"), "\\\\");
        // A backslash before a paren should escape both correctly
        assert_eq!(pdf_escape_literal(r"\("), "\\\\\\(");
        // No double-escaping of already-escaped sequences
        assert_eq!(pdf_escape_literal("hello"), "hello");
    }

    #[test]
    fn pdf_non_ascii_replaced_with_question_mark() {
        let mut api = WriterApi::new();
        api.insert_text("héllo wörld 日本語").unwrap();
        let pdf = match api.export(DocumentFormat::Pdf).unwrap() {
            ExportedDocument::Pdf(bytes) => bytes,
            _ => unreachable!(),
        };
        let pdf_str = String::from_utf8_lossy(&pdf);
        // Non-ASCII characters should be replaced with ? in the PDF stream
        // The PDF should not contain raw UTF-8 bytes for non-ASCII chars in the content stream
        assert!(pdf_str.contains("h?llo w?rld ???"));
    }

    #[test]
    fn load_docx_roundtrip() {
        let mut api = WriterApi::new();
        api.insert_text("roundtrip").unwrap();
        let docx = api.export_docx().unwrap();
        api.load_docx(&docx).unwrap();
        assert_eq!(api.document().blocks[0].text(), Some("roundtrip"));
    }

    #[cfg(feature = "private-fixtures")]
    #[test]
    fn imported_real_docx_is_clean_undo_baseline_and_first_edit_redoes() {
        let bytes = private_layout_docx_bytes();
        let api = WriterApi::from_docx(&bytes).expect("import controlled DOCX fixture");
        assert_imported_docx_is_clean_undo_baseline_and_first_edit_redoes(api);
    }

    #[test]
    fn imported_public_synthetic_docx_is_clean_undo_baseline_and_first_edit_redoes() {
        let api = WriterApi::from_docx(SYNTHETIC_BASIC_DOCX)
            .expect("import public synthetic DOCX fixture");
        assert_imported_docx_is_clean_undo_baseline_and_first_edit_redoes(api);
    }

    #[test]
    fn from_snapshot_starts_with_clean_history() {
        let mut source = WriterApi::new();
        source.insert_text("snapshot baseline").unwrap();

        let api = WriterApi::from_snapshot(source.document()).unwrap();

        assert_eq!(api.document().blocks[0].text(), Some("snapshot baseline"));
        assert!(!api.can_undo());
        assert!(!api.can_redo());
    }

    #[cfg(feature = "private-fixtures")]
    #[test]
    fn external_docx_page_break_starts_following_content_on_new_page() {
        let bytes = private_layout_docx_bytes();
        let api = WriterApi::from_docx(&bytes).expect("import controlled DOCX fixture");
        assert_page_break_starts_following_content_on_new_page(&api, "Вторая страница");
    }

    #[test]
    fn public_synthetic_docx_page_break_starts_following_content_on_new_page() {
        let api = WriterApi::from_docx(SYNTHETIC_BASIC_DOCX)
            .expect("import public synthetic DOCX fixture");
        assert_page_break_starts_following_content_on_new_page(&api, "Учебная страница 😀");
    }

    #[cfg(feature = "private-fixtures")]
    #[test]
    fn loaded_docx_header_footer_distances_are_editable_in_session() {
        let bytes = private_layout_docx_bytes();
        let api = WriterApi::from_docx(&bytes).expect("import controlled DOCX fixture");
        assert_docx_header_footer_distances_are_editable_in_session(api);
    }

    #[test]
    fn public_synthetic_docx_header_footer_distances_are_editable_in_session() {
        let api = WriterApi::from_docx(SYNTHETIC_BASIC_DOCX)
            .expect("import public synthetic DOCX fixture");
        assert_docx_header_footer_distances_are_editable_in_session(api);
    }

    #[test]
    fn public_synthetic_docx_preserves_styled_unicode_through_edit_and_body_roundtrip() {
        let mut api = WriterApi::from_docx(SYNTHETIC_BASIC_DOCX)
            .expect("import public synthetic DOCX fixture");
        let imported_document = api.document();
        let imported_paragraphs = nonempty_text_blocks(&imported_document);

        assert_eq!(imported_paragraphs.len(), 2);
        assert_eq!(imported_paragraphs[0].text, "Рассвет 🙂на орбите");
        assert_eq!(imported_paragraphs[1].text, "Учебная страница 😀");
        assert_eq!(
            imported_paragraphs[0]
                .runs
                .iter()
                .map(|run| (run.text.as_str(), run.style.bold(), run.style.italic()))
                .collect::<Vec<_>>(),
            vec![("Рассвет ", true, false), ("🙂на орбите", false, true)]
        );

        let layout = api.docx_layout();
        assert_eq!(layout.sections.len(), 1);
        let section = &layout.sections[0];
        assert_eq!(section.width_twips, 11_906);
        assert_eq!(section.height_twips, 16_838);
        assert_eq!(section.header_text, "Синтетический верх");
        assert_eq!(section.footer_text, "Синтетический низ");
        assert!(!api.can_undo(), "DOCX import must start a clean history");
        assert!(!api.can_redo());

        let first_paragraph = imported_paragraphs[0];
        api.set_selection(Selection::collapsed(Cursor::new(
            TextContainerId::Block(first_paragraph.id),
            TextOffset::new(first_paragraph.text.chars().count()),
        )))
        .expect("the first paragraph end is a valid cursor");
        let imported_blocks = api.document().blocks;
        api.insert_text(" ✓").expect("append a Unicode edit");
        let edited_blocks = api.document().blocks;
        let edited_document = api.document();
        let edited_paragraphs = nonempty_text_blocks(&edited_document);
        assert_eq!(edited_paragraphs[0].text, "Рассвет 🙂на орбите ✓");
        let imported_styles = imported_paragraphs[0]
            .runs
            .iter()
            .flat_map(|run| {
                run.text
                    .chars()
                    .map(move |ch| (ch, run.style.bold(), run.style.italic()))
            })
            .collect::<Vec<_>>();
        let edited_styles = edited_paragraphs[0]
            .runs
            .iter()
            .flat_map(|run| {
                run.text
                    .chars()
                    .map(move |ch| (ch, run.style.bold(), run.style.italic()))
            })
            .collect::<Vec<_>>();
        assert!(
            edited_styles.starts_with(&imported_styles),
            "editing must retain the existing characters' styles"
        );
        assert_ne!(edited_blocks, imported_blocks);
        assert!(api.can_undo());
        assert!(!api.can_redo());

        assert!(api.undo().expect("undo the public fixture edit"));
        assert_eq!(api.document().blocks, imported_blocks);
        assert!(api.can_redo());
        assert!(api.redo().expect("redo the public fixture edit"));
        assert_eq!(api.document().blocks, edited_blocks);

        let exported = api.export_docx().expect("export edited body as DOCX");
        let reimported =
            WriterApi::from_docx(&exported).expect("reimport edited public fixture body");
        let reimported_document = reimported.document();
        let reimported_paragraphs = nonempty_text_blocks(&reimported_document);
        assert_eq!(reimported_paragraphs.len(), 2);
        assert_eq!(reimported_paragraphs[0].text, "Рассвет 🙂на орбите ✓");
        assert_eq!(reimported_paragraphs[1].text, "Учебная страница 😀");
        for (actual, expected) in reimported_paragraphs.iter().zip(&edited_paragraphs) {
            let actual_styles = actual
                .runs
                .iter()
                .flat_map(|run| {
                    run.text
                        .chars()
                        .map(move |ch| (ch, run.style.bold(), run.style.italic()))
                })
                .collect::<Vec<_>>();
            let expected_styles = expected
                .runs
                .iter()
                .flat_map(|run| {
                    run.text
                        .chars()
                        .map(move |ch| (ch, run.style.bold(), run.style.italic()))
                })
                .collect::<Vec<_>>();
            assert_eq!(
                actual_styles, expected_styles,
                "export/reimport must preserve supported body run styles"
            );
        }
    }

    #[test]
    fn from_docx_loads_document() {
        let mut api = WriterApi::new();
        api.insert_text("from_docx").unwrap();
        let docx = api.export_docx().unwrap();
        let api2 = WriterApi::from_docx(&docx).unwrap();
        assert_eq!(api2.document().blocks[0].text(), Some("from_docx"));
    }
}
