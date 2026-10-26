//! Read-only PDFOxide-backed document admission, geometry, and PNG rendering.
//!
//! PDF actions, attachments, JavaScript, and external resources are never
//! dispatched or exposed here. Their names produce bounded, advisory notices;
//! a name may also occur harmlessly in a stream or comment.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub mod raster_worker;
pub use raster_worker::{PdfRenderCompletion, PdfRenderJob, PdfRenderRequest, PdfRenderWorker};

use pdf_core::{DocumentId, DocumentInfo, PageGeometry, PageId, PdfEngine, PdfError, PdfResult};
use pdf_oxide::document::{PageInfo, PdfDocument};
use pdf_oxide::geometry::Rect;
use pdf_oxide::rendering::{self, ImageFormat, RenderOptions, RenderedImage};

const MAX_SOURCE_BYTES: usize = 64 * 1024 * 1024;
const MAX_PAGES: usize = 20_000;
const MIN_RENDER_SCALE: f32 = 0.01;
const MAX_RENDER_PIXELS: u64 = 16_000_000;
const MAX_RENDER_RASTER_BYTES: u64 = 48_000_000;
const MAX_CROP_WORKING_BYTES: u64 = 128_000_000;
const MAX_RENDER_PNG_BYTES: usize = 64 * 1024 * 1024;
// A render-input budget, not a rejection of the whole PDF or its other pages.
const MAX_DECODED_PAGE_CONTENT_BYTES: usize = 8 * 1024 * 1024;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// Conservative, non-blocking indication of a PDF action/resource name.
/// This is not proof that an action is reachable or executable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PdfViewerSafetyHint {
    JavaScript,
    Launch,
    ExternalUrl,
    EmbeddedFile,
    FileSpecification,
    GoToRemote,
    OpenAction,
}

/// A pre-rendered PNG accepted by the app's open path.
///
/// Detached parsing intentionally supplies no previews: the app renders the
/// first page through [`PdfEngine::render_page`] immediately after adoption.
#[derive(Clone, Debug, PartialEq)]
pub struct PdfInitialPreview {
    pub page_id: PageId,
    pub scale: f32,
    pub png: Vec<u8>,
}

/// Parsed PDF that can cross from the file worker to the UI thread without
/// entering an adapter's registry or acquiring an application document ID.
pub struct PdfOpenArtifact {
    document: PdfDocument,
    page_count: u32,
    safety_hints: Vec<PdfViewerSafetyHint>,
}

impl PdfOpenArtifact {
    /// Metadata before adoption. Document ID zero is a detached sentinel;
    /// [`PdfOxideAdapter::adopt_detached`] assigns a fresh registry ID.
    pub fn document_info(&self) -> DocumentInfo {
        document_info(DocumentId::new(0), self.page_count)
    }

    /// Advisory notices for this document, with at most seven entries.
    pub fn safety_hints(&self) -> Vec<PdfViewerSafetyHint> {
        self.safety_hints.clone()
    }

    /// No speculative render is performed on the file worker.
    pub fn initial_previews(&self) -> Vec<PdfInitialPreview> {
        Vec::new()
    }
}

struct StoredDocument {
    document: Arc<PdfDocument>,
    page_count: u32,
    live: Arc<AtomicBool>,
}

/// Registry of independent read-only PDF documents.
pub struct PdfOxideAdapter {
    next_document_id: u64,
    documents: HashMap<DocumentId, StoredDocument>,
    last_open_safety_hints: Vec<PdfViewerSafetyHint>,
}

impl Default for PdfOxideAdapter {
    fn default() -> Self {
        Self {
            next_document_id: 1,
            documents: HashMap::new(),
            last_open_safety_hints: Vec::new(),
        }
    }
}

impl Drop for PdfOxideAdapter {
    fn drop(&mut self) {
        for stored in self.documents.values() {
            stored.live.store(false, Ordering::Release);
        }
    }
}

impl PdfOxideAdapter {
    /// Parse with PDFOxide without changing this adapter's registry or notices.
    pub fn parse_detached(&self, bytes: &[u8]) -> PdfResult<PdfOpenArtifact> {
        if bytes.len() > MAX_SOURCE_BYTES {
            return Err(error(format!(
                "PDF source exceeds byte limit: {} > {MAX_SOURCE_BYTES}",
                bytes.len()
            )));
        }
        validate_final_eof(bytes)?;
        let safety_hints = viewer_safety_hints(bytes);
        let document = PdfDocument::from_bytes(bytes.to_vec())
            .map_err(|err| error(format!("PDFOxide parse failed: {err}")))?;
        let count = document
            .page_count()
            .map_err(|err| error(format!("PDFOxide page count failed: {err}")))?;
        if count == 0 || count > MAX_PAGES {
            return Err(error(format!(
                "PDF page count must be within 1..={MAX_PAGES} (got {count})"
            )));
        }
        let page_count = u32::try_from(count).map_err(|_| error("PDF page count overflows u32"))?;
        document
            .get_page(0)
            .map_err(|err| error(format!("PDFOxide first page failed: {err}")))?;
        Ok(PdfOpenArtifact {
            document,
            page_count,
            safety_hints,
        })
    }

    /// Adopt an already parsed worker result with a fresh, non-reused ID.
    pub fn adopt_detached(&mut self, artifact: PdfOpenArtifact) -> PdfResult<DocumentInfo> {
        let next = self
            .next_document_id
            .checked_add(1)
            .ok_or_else(|| error("PDF document ID space exhausted"))?;
        let id = DocumentId::new(self.next_document_id);
        self.next_document_id = next;
        let info = document_info(id, artifact.page_count);
        self.last_open_safety_hints = artifact.safety_hints;
        self.documents.insert(
            id,
            StoredDocument {
                document: Arc::new(artifact.document),
                page_count: artifact.page_count,
                live: Arc::new(AtomicBool::new(true)),
            },
        );
        Ok(info)
    }

    /// Remove exactly one adopted document. Unknown/stale IDs are errors.
    pub fn close_document(&mut self, document_id: DocumentId) -> PdfResult<()> {
        self.documents
            .remove(&document_id)
            .map(|stored| stored.live.store(false, Ordering::Release))
            .ok_or_else(|| error(format!("unknown PDF document ID {document_id}")))
    }

    /// Capture an owned CPU-only render job without reparsing the PDF.
    ///
    /// Geometry, content decoding and rasterization are deferred until the job
    /// runs. Closing this document or dropping the adapter invalidates its jobs;
    /// an already running vendor call cannot be preempted.
    pub fn prepare_render(&self, request: PdfRenderRequest) -> PdfResult<PdfRenderJob> {
        let (stored, page_index) =
            self.resolve_stored_page(request.document_id(), request.page_id())?;
        Ok(PdfRenderJob::new(
            request,
            Arc::clone(&stored.document),
            page_index,
            Arc::clone(&stored.live),
        ))
    }

    pub fn document_count(&self) -> usize {
        self.documents.len()
    }

    /// Notices for the last successful adoption or direct parse.
    pub fn last_open_safety_hints(&self) -> Vec<PdfViewerSafetyHint> {
        self.last_open_safety_hints.clone()
    }

    fn resolve_page(
        &self,
        document_id: DocumentId,
        page_id: PageId,
    ) -> PdfResult<(&PdfDocument, usize)> {
        let (stored, page_index) = self.resolve_stored_page(document_id, page_id)?;
        Ok((&stored.document, page_index))
    }

    fn resolve_stored_page(
        &self,
        document_id: DocumentId,
        page_id: PageId,
    ) -> PdfResult<(&StoredDocument, usize)> {
        let stored = self
            .documents
            .get(&document_id)
            .ok_or_else(|| error(format!("unknown PDF document ID {document_id}")))?;
        let page = page_id.value();
        if page == 0 || page > u64::from(stored.page_count) {
            return Err(error(format!(
                "PDF page ID {page} is outside 1..={}",
                stored.page_count
            )));
        }
        Ok((stored, (page - 1) as usize))
    }
}

impl PdfEngine for PdfOxideAdapter {
    fn parse(&mut self, bytes: &[u8]) -> PdfResult<DocumentInfo> {
        self.last_open_safety_hints.clear();
        let artifact = self.parse_detached(bytes)?;
        self.adopt_detached(artifact)
    }

    fn page_geometry(
        &mut self,
        document_id: DocumentId,
        page_id: PageId,
    ) -> PdfResult<PageGeometry> {
        let (document, page_index) = self.resolve_page(document_id, page_id)?;
        let info = document.get_page_info(page_index).map_err(|err| {
            error(format!(
                "PDFOxide page {} geometry failed: {err}",
                page_id.value()
            ))
        })?;
        project_geometry(info, page_id)
    }

    fn render_page(
        &mut self,
        document_id: DocumentId,
        page_id: PageId,
        scale: f32,
    ) -> PdfResult<Vec<u8>> {
        validate_render_scale(scale)?;
        let (document, page_index) = self.resolve_page(document_id, page_id)?;
        render_document_page(document, page_index, page_id, scale)
    }
}

fn validate_render_scale(scale: f32) -> PdfResult<()> {
    if !scale.is_finite() || scale < MIN_RENDER_SCALE {
        return Err(error(format!(
            "render scale must be finite and at least {MIN_RENDER_SCALE} (got {scale})"
        )));
    }
    Ok(())
}

/// Shared production render path; synchronous calls and worker jobs use the
/// same geometry, decoded-content, raster and encoded-image budgets.
fn render_document_page(
    document: &PdfDocument,
    page_index: usize,
    page_id: PageId,
    scale: f32,
) -> PdfResult<Vec<u8>> {
    let info = document.get_page_info(page_index).map_err(|err| {
        error(format!(
            "PDFOxide page {} geometry failed: {err}",
            page_id.value()
        ))
    })?;
    let geometry = project_geometry(info, page_id)?;
    // PDFOxide currently allocates a full MediaBox raster, even when a
    // CropBox exists. Budget the full raster before entering its renderer.
    let (fit_width, fit_height) = preflight_raster(geometry, scale)?;
    let content = document.get_page_content_data(page_index).map_err(|err| {
        error(format!(
            "PDFOxide page {} content decode failed: {err}",
            page_id.value()
        ))
    })?;
    preflight_content_budget(page_id, content.len())?;
    drop(content);
    let rendered = rendering::render_page_fit(
        document,
        page_index,
        fit_width,
        fit_height,
        &RenderOptions::default(),
    )
    .map_err(|err| {
        error(format!(
            "PDFOxide page {} render failed: {err}",
            page_id.value()
        ))
    })?;
    if rendered.format != ImageFormat::Png || !rendered.data.starts_with(PNG_SIGNATURE) {
        return Err(error("PDFOxide renderer did not return a PNG"));
    }
    if rendered.data.len() > MAX_RENDER_PNG_BYTES {
        return Err(error(format!(
            "rendered PNG exceeds byte limit: {} > {MAX_RENDER_PNG_BYTES}",
            rendered.data.len()
        )));
    }
    crop_rendered_png(rendered, geometry)
}

fn preflight_content_budget(page_id: PageId, decoded_bytes: usize) -> PdfResult<()> {
    if decoded_bytes > MAX_DECODED_PAGE_CONTENT_BYTES {
        return Err(error(format!(
            "PDF, страница {}: этап рендера остановлен по лимиту сложности (content budget: {decoded_bytes} > {MAX_DECODED_PAGE_CONTENT_BYTES} байт). Страница не отрисована; другие страницы доступны.",
            page_id.value()
        )));
    }
    Ok(())
}

fn document_info(document_id: DocumentId, page_count: u32) -> DocumentInfo {
    DocumentInfo::new(
        document_id,
        (1..=page_count)
            .map(|page| PageId::new(u64::from(page)))
            .collect(),
    )
}

fn project_geometry(info: PageInfo, page_id: PageId) -> PdfResult<PageGeometry> {
    let media = box_coordinates(info.media_box);
    let rotation = info.rotation.rem_euclid(360);
    PageGeometry::new(media, None, rotation).map_err(|err| {
        error(format!(
            "invalid geometry for PDF page {}: {err}",
            page_id.value()
        ))
    })?;
    let crop = info
        .crop_box
        .map(|rect| {
            let raw = box_coordinates(rect);
            if raw.iter().any(|value| !value.is_finite()) || raw[2] <= raw[0] || raw[3] <= raw[1] {
                return Err(error(format!(
                    "invalid crop box for PDF page {}",
                    page_id.value()
                )));
            }
            // A crop extending beyond the page is clipped to the real media box.
            // An entirely disjoint crop becomes a controlled geometry error.
            Ok([
                raw[0].max(media[0]),
                raw[1].max(media[1]),
                raw[2].min(media[2]),
                raw[3].min(media[3]),
            ])
        })
        .transpose()?;
    PageGeometry::new(media, crop, rotation).map_err(|err| {
        error(format!(
            "invalid geometry for PDF page {}: {err}",
            page_id.value()
        ))
    })
}

fn box_coordinates(rect: Rect) -> [f32; 4] {
    [rect.x, rect.y, rect.x + rect.width, rect.y + rect.height]
}

fn preflight_raster(geometry: PageGeometry, scale: f32) -> PdfResult<(u32, u32)> {
    let mut width = geometry.media_box[2] - geometry.media_box[0];
    let mut height = geometry.media_box[3] - geometry.media_box[1];
    if matches!(geometry.rotation_degrees, 90 | 270) {
        std::mem::swap(&mut width, &mut height);
    }
    // Match render_page_fit's 1-point lower bound on dimensions. Ceil and
    // reserve a one-pixel rounding margin so PDFOxide cannot allocate a
    // larger raster than the preflighted bound.
    let pixel_width = ((f64::from(width.max(1.0)) * f64::from(scale)).ceil()).max(1.0);
    let pixel_height = ((f64::from(height.max(1.0)) * f64::from(scale)).ceil()).max(1.0);
    if pixel_width >= f64::from(u32::MAX) || pixel_height >= f64::from(u32::MAX) {
        return Err(error("render dimensions exceed u32 limit"));
    }
    let fit_width = pixel_width as u32;
    let fit_height = pixel_height as u32;
    let bound_width = u64::from(fit_width) + 1;
    let bound_height = u64::from(fit_height) + 1;
    let pixels = bound_width
        .checked_mul(bound_height)
        .ok_or_else(|| error("render pixel count overflow"))?;
    if pixels > MAX_RENDER_PIXELS {
        return Err(error(format!(
            "render pixel budget exceeded: {pixels} > {MAX_RENDER_PIXELS}"
        )));
    }
    let bytes = pixels
        .checked_mul(4)
        .ok_or_else(|| error("render raster byte count overflow"))?;
    if bytes > MAX_RENDER_RASTER_BYTES {
        return Err(error(format!(
            "render raster byte budget exceeded: {bytes} > {MAX_RENDER_RASTER_BYTES}"
        )));
    }
    if geometry
        .crop_box
        .is_some_and(|crop| crop != geometry.media_box)
    {
        // The vendor pixmap, encoded full PNG, decoded PNG, and cropped
        // image can coexist briefly. This conservative bound is checked
        // before entering the vendor renderer.
        let working_bytes = bytes
            .checked_mul(4)
            .ok_or_else(|| error("crop working byte count overflow"))?;
        if working_bytes > MAX_CROP_WORKING_BYTES {
            return Err(error(format!(
                "crop working byte budget exceeded: {working_bytes} > {MAX_CROP_WORKING_BYTES}"
            )));
        }
    }
    Ok((fit_width, fit_height))
}

fn crop_rendered_png(rendered: RenderedImage, geometry: PageGeometry) -> PdfResult<Vec<u8>> {
    let Some(crop) = geometry.crop_box else {
        return Ok(rendered.data);
    };
    let media = geometry.media_box;
    if crop == media {
        return Ok(rendered.data);
    }
    let media_width = f64::from(media[2] - media[0]);
    let media_height = f64::from(media[3] - media[1]);
    // Normalized raster coordinates of the visible PDF rectangle. These
    // correspond to PDFOxide's actual 0/90/180/270 render transforms and
    // use the returned raster size, avoiding its DPI/scale_override mismatch.
    let (left, top, right, bottom) = match geometry.rotation_degrees {
        0 => (
            f64::from(crop[0] - media[0]) / media_width,
            f64::from(media[3] - crop[3]) / media_height,
            f64::from(crop[2] - media[0]) / media_width,
            f64::from(media[3] - crop[1]) / media_height,
        ),
        90 => (
            f64::from(crop[1] - media[1]) / media_height,
            f64::from(crop[0] - media[0]) / media_width,
            f64::from(crop[3] - media[1]) / media_height,
            f64::from(crop[2] - media[0]) / media_width,
        ),
        180 => (
            f64::from(media[2] - crop[2]) / media_width,
            f64::from(crop[1] - media[1]) / media_height,
            f64::from(media[2] - crop[0]) / media_width,
            f64::from(crop[3] - media[1]) / media_height,
        ),
        270 => (
            f64::from(media[3] - crop[3]) / media_height,
            f64::from(media[2] - crop[2]) / media_width,
            f64::from(media[3] - crop[1]) / media_height,
            f64::from(media[2] - crop[0]) / media_width,
        ),
        _ => return Err(error("invalid page rotation for PNG crop")),
    };
    let raster_width = f64::from(rendered.width);
    let raster_height = f64::from(rendered.height);
    let x0 = (left * raster_width).floor().clamp(0.0, raster_width) as u32;
    let y0 = (top * raster_height).floor().clamp(0.0, raster_height) as u32;
    let x1 = (right * raster_width).ceil().clamp(0.0, raster_width) as u32;
    let y1 = (bottom * raster_height).ceil().clamp(0.0, raster_height) as u32;
    if x1 <= x0 || y1 <= y0 {
        return Err(error("PDF crop produced an empty raster"));
    }
    let image = image::load_from_memory_with_format(&rendered.data, image::ImageFormat::Png)
        .map_err(|err| error(format!("PDFOxide PNG decode failed: {err}")))?;
    if image.width() != rendered.width || image.height() != rendered.height {
        return Err(error(
            "PDFOxide PNG dimensions do not match raster metadata",
        ));
    }
    let cropped = image.crop_imm(x0, y0, x1 - x0, y1 - y0);
    let mut output = Cursor::new(Vec::new());
    cropped
        .write_to(&mut output, image::ImageFormat::Png)
        .map_err(|err| error(format!("PDF crop PNG encode failed: {err}")))?;
    let png = output.into_inner();
    if png.len() > MAX_RENDER_PNG_BYTES {
        return Err(error(format!(
            "cropped PNG exceeds byte limit: {} > {MAX_RENDER_PNG_BYTES}",
            png.len()
        )));
    }
    Ok(png)
}

fn validate_final_eof(bytes: &[u8]) -> PdfResult<()> {
    let final_eof = bytes
        .windows(b"%%EOF".len())
        .enumerate()
        .filter(|(index, marker)| {
            *marker == b"%%EOF" && (*index == 0 || matches!(bytes[*index - 1], b'\n' | b'\r'))
        })
        .map(|(index, _)| index)
        .next_back()
        .ok_or_else(|| error("missing final %%EOF terminator"))?;
    if bytes[final_eof + b"%%EOF".len()..]
        .iter()
        .any(|byte| !is_pdf_whitespace(*byte))
    {
        return Err(error("non-whitespace data follows final %%EOF"));
    }
    Ok(())
}

fn is_pdf_whitespace(byte: u8) -> bool {
    matches!(byte, 0 | b'\t' | b'\n' | 12 | b'\r' | b' ')
}

fn viewer_safety_hints(bytes: &[u8]) -> Vec<PdfViewerSafetyHint> {
    let mut hints = Vec::new();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'/' {
            continue;
        }
        let mut decoded = [0u8; 64];
        let mut decoded_len = 0usize;
        let mut cursor = index + 1;
        while cursor < bytes.len() && !is_name_delimiter(bytes[cursor]) {
            let value = if bytes[cursor] == b'#' && cursor + 2 < bytes.len() {
                match (hex_digit(bytes[cursor + 1]), hex_digit(bytes[cursor + 2])) {
                    (Some(high), Some(low)) => {
                        cursor += 3;
                        (high << 4) | low
                    }
                    _ => {
                        cursor += 1;
                        b'#'
                    }
                }
            } else {
                let value = bytes[cursor];
                cursor += 1;
                value
            };
            if decoded_len < decoded.len() {
                decoded[decoded_len] = value;
            }
            decoded_len += 1;
        }
        if decoded_len > decoded.len() {
            continue;
        }
        let hint = match &decoded[..decoded_len] {
            b"JavaScript" | b"JS" => PdfViewerSafetyHint::JavaScript,
            b"Launch" => PdfViewerSafetyHint::Launch,
            b"URI" | b"URL" => PdfViewerSafetyHint::ExternalUrl,
            b"EmbeddedFile" | b"EF" => PdfViewerSafetyHint::EmbeddedFile,
            b"Filespec" => PdfViewerSafetyHint::FileSpecification,
            b"GoToR" => PdfViewerSafetyHint::GoToRemote,
            b"OpenAction" => PdfViewerSafetyHint::OpenAction,
            _ => continue,
        };
        if !hints.contains(&hint) {
            hints.push(hint);
        }
    }
    hints
}

fn is_name_delimiter(byte: u8) -> bool {
    is_pdf_whitespace(byte) || b"()<>[]{}/%".contains(&byte)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn error(message: impl Into<String>) -> PdfError {
    PdfError::new(message)
}

#[cfg(test)]
mod content_budget_tests {
    use super::*;

    #[test]
    fn ordinary_page_and_exact_decoded_content_limit_are_allowed() {
        assert!(preflight_content_budget(PageId::new(1), 1024).is_ok());
        assert!(preflight_content_budget(PageId::new(1), MAX_DECODED_PAGE_CONTENT_BYTES).is_ok());
    }

    #[test]
    fn excessive_page_content_is_a_precise_page_local_error() {
        let err = preflight_content_budget(PageId::new(30), 9_320_480).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("30"));
        assert!(message.contains("9320480"));
        assert!(message.contains("8388608"));
        assert!(message.contains("content budget"));
    }
}
