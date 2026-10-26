//! Application-level document identity and session ownership.
//!
//! This layer sits above backend-specific handles and below Makepad widgets.
//! Its IDs are runtime-local and are never paths, tab positions, widget IDs, or
//! engine IDs.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::sheets_selection_view_state::SheetsSelectionViewStates;
use rudra_runtime::{EntityId, Registry};
use sheets_api::SpreadsheetApi;
use sheets_ironcalc_adapter::IroncalcAdapter;
use writer_ui::ViewMode;

use super::{Module, OpenDocument, PdfZoomMode, WriterDisplayMode};

/// Stable application identity for one live document session.
pub(crate) type DocumentId = EntityId<DocumentSession>;

/// Metadata describing where a document was opened from.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DocumentSource {
    path: Option<PathBuf>,
    /// Immutable source bytes retained only by viewers that need bounded,
    /// on-demand projection after the initial open completes.
    retained_bytes: Option<Arc<[u8]>>,
}

impl DocumentSource {
    pub(crate) fn from_path(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            retained_bytes: None,
        }
    }

    pub(crate) fn from_path_with_retained_bytes(path: PathBuf, bytes: Arc<[u8]>) -> Self {
        Self {
            path: Some(path),
            retained_bytes: Some(bytes),
        }
    }

    pub(crate) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub(crate) fn retained_bytes(&self) -> Option<&Arc<[u8]>> {
        self.retained_bytes.as_ref()
    }

    /// A successful Save As changes the current location, not the runtime ID.
    /// Retained import bytes are invalid after exporting an edited document.
    pub(crate) fn replace_path_after_save(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.retained_bytes = None;
    }
}
/// Per-document Writer viewport state independent from a concrete canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WriterSessionView {
    pub(crate) display_mode: WriterDisplayMode,
    pub(crate) layout_mode: ViewMode,
    pub(crate) zoom: f64,
    pub(crate) scroll_x: f64,
    pub(crate) scroll_y: f64,
}

impl Default for WriterSessionView {
    fn default() -> Self {
        Self {
            display_mode: WriterDisplayMode::Visual,
            layout_mode: ViewMode::Pages,
            zoom: 1.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
        }
    }
}

/// Per-document Sheets viewport state without backend identity details.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SheetsSessionView {
    pub(crate) selected_sheet: usize,
    pub(crate) zoom: f32,
    pub(crate) scroll_x: u32,
    pub(crate) scroll_y: u32,
}

impl Default for SheetsSessionView {
    fn default() -> Self {
        Self {
            selected_sheet: 0,
            zoom: 1.0,
            scroll_x: 0,
            scroll_y: 0,
        }
    }
}

/// Per-document PDF viewport state; the PDF engine handle stays in the backend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PdfSessionView {
    pub(crate) page_number: u32,
    pub(crate) zoom: f32,
    pub(crate) zoom_mode: PdfZoomMode,
    pub(crate) scroll_x: f32,
    pub(crate) scroll_y: f32,
    /// False until the first real controller geometry has been synchronized.
    /// A fresh session must not overwrite the controller's initial page reveal
    /// with synthetic (0, 0) scroll coordinates.
    pub(crate) initialized: bool,
}

impl Default for PdfSessionView {
    fn default() -> Self {
        Self {
            page_number: 1,
            zoom: 1.0,
            zoom_mode: PdfZoomMode::Custom,
            scroll_x: 0.0,
            scroll_y: 0.0,
            initialized: false,
        }
    }
}

/// Per-document Slides viewport state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SlidesSessionView {
    pub(crate) current_index: usize,
    pub(crate) zoom: f32,
    pub(crate) presentation_mode: bool,
}

impl Default for SlidesSessionView {
    fn default() -> Self {
        Self {
            current_index: 0,
            zoom: 1.0,
            presentation_mode: false,
        }
    }
}

/// Canonical view state owned by the document rather than by a Makepad widget.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DocumentViewState {
    Writer(WriterSessionView),
    Sheets(SheetsSessionView),
    Pdf(PdfSessionView),
    Slides(SlidesSessionView),
}

impl DocumentViewState {
    fn for_backend(backend: &OpenDocument) -> Result<Self, DocumentRuntimeError> {
        match backend {
            OpenDocument::Writer(_) => Ok(Self::Writer(WriterSessionView::default())),
            OpenDocument::Sheets(_) => Ok(Self::Sheets(SheetsSessionView::default())),
            OpenDocument::Pdf { .. } => Ok(Self::Pdf(PdfSessionView::default())),
            OpenDocument::Slides(_) => Ok(Self::Slides(SlidesSessionView::default())),
            OpenDocument::None => Err(DocumentRuntimeError::InvalidState(
                "an empty OpenDocument cannot become a session",
            )),
        }
    }
}

/// One live Office document and all application-owned metadata around it.
pub(crate) struct DocumentSession {
    source: DocumentSource,
    backend: OpenDocument,
    view_state: DocumentViewState,
    /// The active app temporarily leases this exact API. Inactive sessions own
    /// the engine and bounded transaction history; tab switches never reimport it.
    pub(crate) sheets_api: Option<SpreadsheetApi<IroncalcAdapter>>,
    pub(crate) sheets_selections: SheetsSelectionViewStates,
}

impl DocumentSession {
    pub(crate) fn new(
        source: DocumentSource,
        backend: OpenDocument,
    ) -> Result<Self, DocumentRuntimeError> {
        let view_state = DocumentViewState::for_backend(&backend)?;
        Ok(Self {
            source,
            backend,
            view_state,
            sheets_api: None,
            sheets_selections: SheetsSelectionViewStates::default(),
        })
    }

    pub(crate) fn kind(&self) -> Module {
        self.backend.module()
    }

    pub(crate) fn source(&self) -> &DocumentSource {
        &self.source
    }

    pub(crate) fn replace_source_path_after_save(&mut self, path: PathBuf) {
        self.source.replace_path_after_save(path);
    }

    pub(crate) fn backend(&self) -> &OpenDocument {
        &self.backend
    }

    pub(crate) fn backend_mut(&mut self) -> &mut OpenDocument {
        &mut self.backend
    }

    pub(crate) fn view_state(&self) -> &DocumentViewState {
        &self.view_state
    }

    pub(crate) fn view_state_mut(&mut self) -> &mut DocumentViewState {
        &mut self.view_state
    }

    #[cfg(test)]
    pub(crate) fn backend_kind(&self) -> BackendHandleKind {
        match self.backend {
            OpenDocument::Writer(_) => BackendHandleKind::Writer,
            OpenDocument::Sheets(_) => BackendHandleKind::Sheets,
            OpenDocument::Pdf { .. } => BackendHandleKind::Pdf,
            OpenDocument::Slides(_) => BackendHandleKind::Slides,
            OpenDocument::None => BackendHandleKind::None,
        }
    }
}

/// Non-identifying backend classification exposed by inspection.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BackendHandleKind {
    None,
    Writer,
    Sheets,
    Pdf,
    Slides,
}

/// Owner of all live document sessions for one application runtime.
#[derive(Default)]
pub(crate) struct DocumentRegistry {
    sessions: Registry<DocumentSession>,
}

impl DocumentRegistry {
    pub(crate) fn insert(
        &mut self,
        session: DocumentSession,
    ) -> Result<DocumentId, DocumentRuntimeError> {
        self.sessions
            .insert(session)
            .map_err(|_| DocumentRuntimeError::IdExhausted)
    }

    pub(crate) fn get(&self, id: DocumentId) -> Option<&DocumentSession> {
        self.sessions.get(id)
    }

    pub(crate) fn get_mut(&mut self, id: DocumentId) -> Option<&mut DocumentSession> {
        self.sessions.get_mut(id)
    }

    pub(crate) fn require(&self, id: DocumentId) -> Result<&DocumentSession, DocumentRuntimeError> {
        self.get(id).ok_or(DocumentRuntimeError::EntityNotFound(id))
    }

    pub(crate) fn require_mut(
        &mut self,
        id: DocumentId,
    ) -> Result<&mut DocumentSession, DocumentRuntimeError> {
        self.get_mut(id)
            .ok_or(DocumentRuntimeError::EntityNotFound(id))
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, id: DocumentId) -> bool {
        self.sessions.contains(id)
    }

    pub(crate) fn remove(&mut self, id: DocumentId) -> Option<DocumentSession> {
        self.sessions.remove(id)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.sessions.len()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    pub(crate) fn iter(
        &self,
    ) -> impl ExactSizeIterator<Item = (DocumentId, &DocumentSession)> + '_ {
        self.sessions.iter()
    }
}

/// Operation names carried by typed command errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DocumentOperation {
    Scroll,
    GoToPage,
    SetViewMode,
}

impl fmt::Display for DocumentOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Scroll => "scroll",
            Self::GoToPage => "go_to_page",
            Self::SetViewMode => "set_view_mode",
        };
        formatter.write_str(value)
    }
}

/// Target-aware view modes supported by the first Office integration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DocumentViewMode {
    WriterLayout(ViewMode),
    WriterDisplay(WriterDisplayMode),
    PdfZoom(PdfZoomMode),
    SlidesPresentation(bool),
}

/// Typed failures from document identity, lifecycle, and target commands.
#[derive(Debug, PartialEq)]
pub(crate) enum DocumentRuntimeError {
    EntityNotFound(DocumentId),
    UnsupportedOperation {
        id: DocumentId,
        operation: DocumentOperation,
        kind: Module,
    },
    BackendError(String),
    InvalidState(&'static str),
    IdExhausted,
}

impl fmt::Display for DocumentRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EntityNotFound(id) => write!(formatter, "document:{id} was not found"),
            Self::UnsupportedOperation {
                id,
                operation,
                kind,
            } => write!(
                formatter,
                "{operation} is unsupported for document:{id} ({kind:?})"
            ),
            Self::BackendError(message) => write!(formatter, "backend error: {message}"),
            Self::InvalidState(message) => write!(formatter, "invalid document state: {message}"),
            Self::IdExhausted => formatter.write_str("document id space exhausted"),
        }
    }
}

impl Error for DocumentRuntimeError {}

/// Stable, backend-handle-free diagnostic projection of a session.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DocumentInspection {
    pub(crate) id: DocumentId,
    pub(crate) kind: Module,
    pub(crate) source_path: Option<PathBuf>,
    pub(crate) backend_kind: BackendHandleKind,
    pub(crate) view_state: DocumentViewState,
}

#[cfg(test)]
impl DocumentInspection {
    pub(crate) fn from_session(id: DocumentId, session: &DocumentSession) -> Self {
        Self {
            id,
            kind: session.kind(),
            source_path: session.source().path().map(Path::to_path_buf),
            backend_kind: session.backend_kind(),
            view_state: *session.view_state(),
        }
    }
}
