//! Application-owned module extracted without changing behavior.
use super::*;

impl AppState {
    pub(crate) fn active_document_id(&self) -> Option<RuntimeDocumentId> {
        self.active_document
    }

    pub(crate) fn active_module(&self) -> Module {
        self.active_session()
            .map(DocumentSession::kind)
            .unwrap_or(self.idle_module)
    }

    pub(crate) fn document_id_for_module(&self, module: Module) -> Option<RuntimeDocumentId> {
        self.documents
            .iter()
            .find_map(|(id, session)| (session.kind() == module).then_some(id))
    }

    pub(crate) fn ordered_document_ids(&self) -> Vec<RuntimeDocumentId> {
        self.documents.iter().map(|(id, _)| id).collect()
    }

    pub(crate) fn document_tab_frames(&self) -> Vec<DocumentTabFrame> {
        let mut frames = project_document_tabs(self.documents.iter(), self.active_document);
        for frame in &mut frames {
            if !self.new_document_temp_paths.contains_key(&frame.id) {
                continue;
            }
            let title = match self.new_document_formats.get(&frame.id) {
                Some(BlankDocumentFormat::Docx) => "Новый документ.docx",
                Some(BlankDocumentFormat::Xlsx) => "Новая таблица.xlsx",
                Some(BlankDocumentFormat::Pptx) => "Новая презентация.pptx",
                Some(BlankDocumentFormat::Pdf) => "Новый PDF.pdf",
                None => continue,
            };
            frame.label = title.to_owned();
            frame.display_label = title.to_owned();
        }
        frames
    }

    pub(crate) fn active_session(&self) -> Option<&DocumentSession> {
        self.active_document.and_then(|id| self.documents.get(id))
    }

    pub(crate) fn active_session_mut(&mut self) -> Option<&mut DocumentSession> {
        let id = self.active_document?;
        self.documents.get_mut(id)
    }

    pub(crate) fn document(&self) -> Option<&OpenDocument> {
        self.active_session().map(DocumentSession::backend)
    }

    pub(crate) fn document_mut(&mut self) -> Option<&mut OpenDocument> {
        self.active_session_mut().map(DocumentSession::backend_mut)
    }

    pub(crate) fn sync_active_document_summary(&mut self) {
        self.ribbon_command_feedback = None;
        let Some(id) = self.active_document else {
            self.status = Module::Home.hint().to_owned();
            self.detail = "Все вкладки документов закрыты.".to_owned();
            return;
        };
        let Some(session) = self.documents.get(id) else {
            self.active_document = None;
            self.idle_module = Module::Home;
            self.status = Module::Home.hint().to_owned();
            self.detail = "Активная сессия документа недоступна.".to_owned();
            return;
        };
        let (status, detail) = session.backend().opening_summary();
        let path = session.source().path().map(Path::to_path_buf);
        self.status = status;
        self.detail = match path {
            Some(path) => format!(
                "{detail} Файл: {}. Runtime ID: document:{id}",
                path.display()
            ),
            None => format!("{detail} Runtime ID: document:{id}"),
        };
    }

    pub(crate) fn sync_active_session_view(&mut self) {
        let Some(id) = self.active_document else {
            return;
        };
        let Some(session) = self.documents.get(id) else {
            self.active_document = None;
            return;
        };
        let previous = *session.view_state();
        let view_state = match previous {
            DocumentViewState::Writer(mut view) => {
                view.display_mode = self.writer_display_mode;
                if let Some(OpenDocument::Writer(writer)) = self.document() {
                    view.layout_mode = writer.state().mode;
                    view.zoom = f64::from(writer.state().zoom_percent) / 100.0;
                }
                DocumentViewState::Writer(view)
            }
            DocumentViewState::Sheets(mut view) => {
                view.selected_sheet = self.sheets_selected;
                if let Some(sheet_view) = self.sheets_view.as_ref() {
                    let scroll = sheet_view.scroll_offset();
                    view.zoom = sheet_view.zoom();
                    view.scroll_x = scroll.x;
                    view.scroll_y = scroll.y;
                }
                DocumentViewState::Sheets(view)
            }
            DocumentViewState::Pdf(mut view) => {
                let state = self.pdf_controller.state();
                view.page_number = state.current_page_number().max(1);
                view.zoom = state.scale;
                view.zoom_mode = state.zoom_mode;
                view.scroll_x = state.scroll_x_points;
                view.scroll_y = state.scroll_y_points;
                view.initialized = true;
                DocumentViewState::Pdf(view)
            }
            DocumentViewState::Slides(mut view) => {
                view.current_index = self.slides_current_index;
                view.zoom = self.slides_scale;
                view.presentation_mode = self.slides_presentation_mode;
                DocumentViewState::Slides(view)
            }
        };
        if let Some(session) = self.documents.get_mut(id) {
            *session.view_state_mut() = view_state;
            if let Some(sheet_view) = self.sheets_view.as_ref() {
                let scroll = sheet_view.scroll_offset();
                session.sheets_selections.remember(
                    sheet_view.selection().clone(),
                    scroll.x,
                    scroll.y,
                );
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn register_document(
        &mut self,
        path: PathBuf,
        document: OpenDocument,
    ) -> Result<RuntimeDocumentId, DocumentRuntimeError> {
        self.register_document_with_source(DocumentSource::from_path(path), document)
    }

    pub(crate) fn register_document_with_source(
        &mut self,
        source: DocumentSource,
        document: OpenDocument,
    ) -> Result<RuntimeDocumentId, DocumentRuntimeError> {
        let session = DocumentSession::new(source, document)?;
        self.documents.insert(session)
    }

    pub(crate) fn activate_document(
        &mut self,
        id: RuntimeDocumentId,
    ) -> Result<(), DocumentRuntimeError> {
        self.documents.require(id)?;
        self.sync_active_session_view();
        if let Some(view) = self.sheets_view.as_mut() {
            view.cancel_pointer_selection();
        }
        // Return the active lease before clearing the visible adapters. A
        // bounded read-only projection is disposable, not a canonical engine.
        if self.sheets_bounded_source_bytes().is_none()
            && let Some(previous) = self.active_document
            && let Some(session) = self.documents.get_mut(previous)
            && let Some(api) = self.sheets_api.take()
        {
            session.sheets_api = Some(api);
        }

        let (view_state, sheets_bytes, has_bounded_sheets_source, pdf_bindings) = {
            let session = self.documents.require(id)?;
            let has_bounded_sheets_source = matches!(session.backend(), OpenDocument::Sheets(_))
                && session.source().retained_bytes().is_some();
            let sheets_bytes = match session.backend() {
                OpenDocument::Sheets(snapshot)
                    if !has_bounded_sheets_source && session.sheets_api.is_none() =>
                {
                    Some(
                        XlsxPartsCodec::export(&IroncalcXlsxParts, snapshot).map_err(|error| {
                            DocumentRuntimeError::BackendError(error.to_string())
                        })?,
                    )
                }
                _ => None,
            };
            let pdf_bindings =
                session
                    .backend()
                    .pdf_bindings()
                    .map(|(backend_id, page_count, page_ids)| {
                        (backend_id, page_count, page_ids.to_vec())
                    });
            (
                *session.view_state(),
                sheets_bytes,
                has_bounded_sheets_source,
                pdf_bindings,
            )
        };

        self.active_document = None;
        self.clear_sheets_document();
        self.clear_pdf_document();
        self.clear_slides_document();
        self.active_document = Some(id);
        self.home_ribbon_open = true;

        match view_state {
            DocumentViewState::Writer(view) => {
                self.writer_display_mode = view.display_mode;
                if let Some(OpenDocument::Writer(writer)) = self.document_mut() {
                    writer.set_view_mode(view.layout_mode);
                    writer
                        .set_zoom_percent((view.zoom * 100.0).round() as u16)
                        .map_err(|error| DocumentRuntimeError::BackendError(error.to_string()))?;
                }
            }
            DocumentViewState::Sheets(view) => {
                let selected_sheet = self
                    .sheets_snapshot()
                    .and_then(|snapshot| clamp_sheet_index(snapshot, view.selected_sheet))
                    .unwrap_or(0);
                self.sheets_selected = selected_sheet;
                if has_bounded_sheets_source {
                    self.load_bounded_sheets_window(bounded_sheet_window_from_scroll(
                        selected_sheet,
                        view.scroll_x,
                        view.scroll_y,
                    ))?;
                } else {
                    if let Some(api) = self.documents.require_mut(id)?.sheets_api.take() {
                        self.bind_sheets_api(api)
                            .map_err(DocumentRuntimeError::BackendError)?;
                    } else if let Some(bytes) = sheets_bytes {
                        self.bind_sheets_visual(&bytes)
                            .map_err(DocumentRuntimeError::BackendError)?;
                    }
                    // Resolve the persisted tab index in the live API. Cached
                    // documents keep the exact SheetId space and their history.
                    let visual_sheet = self.sheets_api.as_ref().and_then(|api| {
                        api.document()
                            .snapshot()
                            .sheets
                            .get(selected_sheet)
                            .map(|sheet| sheet.id)
                    });
                    if let Some(sheet) = visual_sheet {
                        if let Some(api) = self.sheets_api.as_mut() {
                            api.set_active_sheet(sheet).map_err(|error| {
                                DocumentRuntimeError::BackendError(error.to_string())
                            })?;
                        }
                        self.sync_sheets_visual()
                            .map_err(DocumentRuntimeError::BackendError)?;
                    }
                    if let Some(sheet_view) = self.sheets_view.as_mut() {
                        sheet_view.set_zoom(view.zoom);
                        let _ = sheet_view.handle_event(UiEvent::Wheel {
                            delta_x: i64::from(view.scroll_x),
                            delta_y: i64::from(view.scroll_y),
                        });
                    }
                    self.restore_sheets_selection()
                        .map_err(DocumentRuntimeError::BackendError)?;
                }
            }
            DocumentViewState::Pdf(view) => {
                if let Some((backend_id, page_count, page_ids)) = pdf_bindings {
                    self.bind_pdf_document(backend_id, page_count, &page_ids);
                    self.pdf_effect = self.pdf_controller.go_to_page_number(view.page_number);
                    restore_pdf_session_view(self.pdf_controller.state_mut(), view);
                    self.pdf_effect = self.pdf_controller.request_current_page();
                    self.apply_pdf_effect();
                }
            }
            DocumentViewState::Slides(view) => {
                self.bind_slides_document();
                self.slides_select(view.current_index);
                self.slides_scale = view.zoom;
                if let Some(viewer) = self.slides_viewer.as_mut() {
                    self.slides_scale = viewer
                        .set_zoom(view.zoom)
                        .map_err(|error| DocumentRuntimeError::BackendError(error.to_string()))?;
                }
                self.slides_presentation_mode = view.presentation_mode;
            }
        }
        self.sync_active_session_view();
        self.sync_active_document_summary();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn inspect(
        &self,
        id: RuntimeDocumentId,
    ) -> Result<document_runtime::DocumentInspection, DocumentRuntimeError> {
        let session = self.documents.require(id)?;
        Ok(document_runtime::DocumentInspection::from_session(
            id, session,
        ))
    }

    pub(crate) fn active_target(&self) -> Result<RuntimeDocumentId, DocumentRuntimeError> {
        self.active_document
            .ok_or(DocumentRuntimeError::InvalidState("no active document"))
    }

    pub(crate) fn set_zoom(
        &mut self,
        id: RuntimeDocumentId,
        requested: f64,
    ) -> Result<f64, DocumentRuntimeError> {
        if !requested.is_finite() || requested <= 0.0 {
            return Err(DocumentRuntimeError::InvalidState(
                "zoom must be finite and greater than zero",
            ));
        }
        let (kind, applied) = {
            let session = self.documents.require_mut(id)?;
            let kind = session.kind();
            let applied = match session.view_state_mut() {
                DocumentViewState::Writer(view) => {
                    view.zoom = requested.clamp(0.25, 4.0);
                    view.zoom
                }
                DocumentViewState::Sheets(view) => {
                    view.zoom = (requested as f32).clamp(0.5, 2.0);
                    f64::from(view.zoom)
                }
                DocumentViewState::Pdf(view) => {
                    view.zoom = (requested as f32).clamp(PDF_MIN_SCALE, PDF_MAX_SCALE);
                    view.zoom_mode = PdfZoomMode::Custom;
                    f64::from(view.zoom)
                }
                DocumentViewState::Slides(view) => {
                    view.zoom = (requested as f32).clamp(0.25, 4.0);
                    f64::from(view.zoom)
                }
            };
            (kind, applied)
        };

        if kind == Module::Writer {
            let session = self.documents.require_mut(id)?;
            let OpenDocument::Writer(writer) = session.backend_mut() else {
                return Err(DocumentRuntimeError::InvalidState(
                    "Writer session/backend mismatch",
                ));
            };
            writer
                .set_zoom_percent((applied * 100.0).round() as u16)
                .map_err(|error| DocumentRuntimeError::BackendError(error.to_string()))?;
        }

        if self.active_document == Some(id) {
            match kind {
                Module::Writer => {}
                Module::Sheets => {
                    if let Some(view) = self.sheets_view.as_mut() {
                        view.set_zoom(applied as f32);
                    }
                }
                Module::Pdf => {
                    self.pdf_controller.state_mut().set_scale(applied as f32);
                    self.pdf_effect = self.pdf_controller.request_current_page();
                    self.apply_pdf_effect();
                }
                Module::Slides => {
                    self.slides_scale = applied as f32;
                    if let Some(viewer) = self.slides_viewer.as_mut() {
                        self.slides_scale = viewer.set_zoom(applied as f32).map_err(|error| {
                            DocumentRuntimeError::BackendError(error.to_string())
                        })?;
                    }
                }
                Module::Home => {
                    return Err(DocumentRuntimeError::InvalidState(
                        "Home cannot own a document session",
                    ));
                }
            }
            self.sync_active_session_view();
        }
        Ok(applied)
    }

    pub(crate) fn zoom_active_document_to(
        &mut self,
        requested: f64,
    ) -> Result<f64, DocumentRuntimeError> {
        let id = self.active_target()?;
        self.set_zoom(id, requested)
    }

    pub(crate) fn scroll(
        &mut self,
        id: RuntimeDocumentId,
        delta_x: f64,
        delta_y: f64,
    ) -> Result<(f64, f64), DocumentRuntimeError> {
        if !delta_x.is_finite() || !delta_y.is_finite() {
            return Err(DocumentRuntimeError::InvalidState(
                "scroll deltas must be finite",
            ));
        }
        let (kind, position) = {
            let session = self.documents.require_mut(id)?;
            let kind = session.kind();
            let position = match session.view_state_mut() {
                DocumentViewState::Writer(view) => {
                    view.scroll_x = (view.scroll_x + delta_x).max(0.0);
                    view.scroll_y = (view.scroll_y + delta_y).max(0.0);
                    (view.scroll_x, view.scroll_y)
                }
                DocumentViewState::Sheets(view) => {
                    let dx = delta_x.round() as i64;
                    let dy = delta_y.round() as i64;
                    view.scroll_x = if dx.is_negative() {
                        view.scroll_x.saturating_sub(dx.unsigned_abs() as u32)
                    } else {
                        view.scroll_x.saturating_add(dx as u32)
                    };
                    view.scroll_y = if dy.is_negative() {
                        view.scroll_y.saturating_sub(dy.unsigned_abs() as u32)
                    } else {
                        view.scroll_y.saturating_add(dy as u32)
                    };
                    (f64::from(view.scroll_x), f64::from(view.scroll_y))
                }
                DocumentViewState::Pdf(view) => {
                    view.scroll_x = (view.scroll_x + delta_x as f32).max(0.0);
                    view.scroll_y = (view.scroll_y + delta_y as f32).max(0.0);
                    (f64::from(view.scroll_x), f64::from(view.scroll_y))
                }
                DocumentViewState::Slides(_) => {
                    return Err(DocumentRuntimeError::UnsupportedOperation {
                        id,
                        operation: DocumentOperation::Scroll,
                        kind,
                    });
                }
            };
            (kind, position)
        };

        if self.active_document == Some(id) {
            match kind {
                Module::Writer => {}
                Module::Sheets => {
                    if let Some(view) = self.sheets_view.as_mut() {
                        let _ = view.handle_event(UiEvent::Wheel {
                            delta_x: delta_x.round() as i64,
                            delta_y: delta_y.round() as i64,
                        });
                    }
                    if self.refresh_bounded_sheets_window_if_needed()? {
                        self.sync_sheets_status();
                    }
                }
                Module::Pdf => {
                    self.pdf_effect = self
                        .pdf_controller
                        .scroll_by(delta_x as f32, delta_y as f32);
                    self.apply_pdf_effect();
                }
                Module::Slides | Module::Home => {}
            }
            self.sync_active_session_view();
        }
        Ok(position)
    }

    pub(crate) fn scroll_active_document(
        &mut self,
        delta_x: f64,
        delta_y: f64,
    ) -> Result<(f64, f64), DocumentRuntimeError> {
        let id = self.active_target()?;
        self.scroll(id, delta_x, delta_y)
    }

    pub(crate) fn go_to_page(
        &mut self,
        id: RuntimeDocumentId,
        page_number: u32,
    ) -> Result<(), DocumentRuntimeError> {
        if page_number == 0 {
            return Err(DocumentRuntimeError::InvalidState(
                "page numbers are one-based",
            ));
        }
        let kind = self.documents.require(id)?.kind();
        match kind {
            Module::Pdf => {
                let page_count = match self.documents.require(id)?.backend() {
                    OpenDocument::Pdf { page_count, .. } => *page_count,
                    _ => {
                        return Err(DocumentRuntimeError::InvalidState(
                            "PDF session/backend mismatch",
                        ));
                    }
                };
                if page_number > page_count {
                    return Err(DocumentRuntimeError::InvalidState(
                        "page number exceeds document page count",
                    ));
                }
                let session = self.documents.require_mut(id)?;
                let DocumentViewState::Pdf(view) = session.view_state_mut() else {
                    return Err(DocumentRuntimeError::InvalidState(
                        "PDF session/view mismatch",
                    ));
                };
                view.page_number = page_number;
                if self.active_document == Some(id) {
                    self.pdf_effect = self.pdf_controller.go_to_page_number(page_number);
                    self.apply_pdf_effect();
                    self.sync_active_session_view();
                }
                Ok(())
            }
            Module::Slides => {
                let slide_count = match self.documents.require(id)?.backend() {
                    OpenDocument::Slides(presentation) => presentation.slides.len(),
                    _ => {
                        return Err(DocumentRuntimeError::InvalidState(
                            "Slides session/backend mismatch",
                        ));
                    }
                };
                if page_number as usize > slide_count {
                    return Err(DocumentRuntimeError::InvalidState(
                        "page number exceeds slide count",
                    ));
                }
                let index = page_number as usize - 1;
                let session = self.documents.require_mut(id)?;
                let DocumentViewState::Slides(view) = session.view_state_mut() else {
                    return Err(DocumentRuntimeError::InvalidState(
                        "Slides session/view mismatch",
                    ));
                };
                view.current_index = index;
                if self.active_document == Some(id) {
                    if let Some(viewer) = self.slides_viewer.as_mut() {
                        viewer.select(index).map_err(|error| {
                            DocumentRuntimeError::BackendError(error.to_string())
                        })?;
                        self.slides_current_index = viewer.current_index().unwrap_or(index);
                    } else {
                        self.slides_current_index = index;
                    }
                    self.slides_status_update();
                    self.sync_active_session_view();
                }
                Ok(())
            }
            _ => Err(DocumentRuntimeError::UnsupportedOperation {
                id,
                operation: DocumentOperation::GoToPage,
                kind,
            }),
        }
    }

    pub(crate) fn go_to_page_active(
        &mut self,
        page_number: u32,
    ) -> Result<(), DocumentRuntimeError> {
        let id = self.active_target()?;
        self.go_to_page(id, page_number)
    }

    pub(crate) fn set_view_mode(
        &mut self,
        id: RuntimeDocumentId,
        mode: DocumentViewMode,
    ) -> Result<(), DocumentRuntimeError> {
        let kind = self.documents.require(id)?.kind();
        match (kind, mode) {
            (Module::Writer, DocumentViewMode::WriterLayout(layout_mode)) => {
                let session = self.documents.require_mut(id)?;
                let DocumentViewState::Writer(view) = session.view_state_mut() else {
                    return Err(DocumentRuntimeError::InvalidState(
                        "Writer session/view mismatch",
                    ));
                };
                view.layout_mode = layout_mode;
                let OpenDocument::Writer(writer) = session.backend_mut() else {
                    return Err(DocumentRuntimeError::InvalidState(
                        "Writer session/backend mismatch",
                    ));
                };
                writer.set_view_mode(layout_mode);
            }
            (Module::Writer, DocumentViewMode::WriterDisplay(display_mode)) => {
                let session = self.documents.require_mut(id)?;
                let DocumentViewState::Writer(view) = session.view_state_mut() else {
                    return Err(DocumentRuntimeError::InvalidState(
                        "Writer session/view mismatch",
                    ));
                };
                view.display_mode = display_mode;
                if self.active_document == Some(id) {
                    self.writer_display_mode = display_mode;
                }
            }
            (Module::Pdf, DocumentViewMode::PdfZoom(zoom_mode)) => {
                let session = self.documents.require_mut(id)?;
                let DocumentViewState::Pdf(view) = session.view_state_mut() else {
                    return Err(DocumentRuntimeError::InvalidState(
                        "PDF session/view mismatch",
                    ));
                };
                view.zoom_mode = zoom_mode;
                if self.active_document == Some(id) {
                    self.pdf_effect = match zoom_mode {
                        PdfZoomMode::Custom => self.pdf_controller.request_current_page(),
                        PdfZoomMode::FitWidth => self.pdf_controller.fit_width(),
                        PdfZoomMode::FitPage => self.pdf_controller.fit_page(),
                    };
                    self.apply_pdf_effect();
                    self.sync_active_session_view();
                }
            }
            (Module::Slides, DocumentViewMode::SlidesPresentation(enabled)) => {
                let session = self.documents.require_mut(id)?;
                let DocumentViewState::Slides(view) = session.view_state_mut() else {
                    return Err(DocumentRuntimeError::InvalidState(
                        "Slides session/view mismatch",
                    ));
                };
                view.presentation_mode = enabled;
                if self.active_document == Some(id) {
                    self.slides_presentation_mode = enabled;
                }
            }
            _ => {
                return Err(DocumentRuntimeError::UnsupportedOperation {
                    id,
                    operation: DocumentOperation::SetViewMode,
                    kind,
                });
            }
        }
        Ok(())
    }

    pub(crate) fn set_active_view_mode(
        &mut self,
        mode: DocumentViewMode,
    ) -> Result<(), DocumentRuntimeError> {
        let id = self.active_target()?;
        self.set_view_mode(id, mode)
    }

    pub(crate) fn close_document(
        &mut self,
        id: RuntimeDocumentId,
    ) -> Result<(), DocumentRuntimeError> {
        if self
            .writer_inline_session
            .as_ref()
            .is_some_and(|session| session.document_id == id)
        {
            return Err(DocumentRuntimeError::InvalidState(
                "Apply or cancel the open Writer draft before closing the document",
            ));
        }
        let pdf_backend_id = self
            .documents
            .require(id)?
            .backend()
            .pdf_bindings()
            .map(|(backend_id, _, _)| backend_id);
        let was_active = self.active_document == Some(id);

        if was_active {
            self.clear_sheets_document();
            self.clear_slides_document();
        }
        if let Some(backend_id) = pdf_backend_id {
            self.cancel_pdf_document(backend_id);
            self.pdf_controller.release_document(backend_id);
            self.sync_pdf_render_status();
        } else if was_active {
            self.clear_pdf_document();
        }

        if let Some(backend_id) = pdf_backend_id {
            self.pdf_engine
                .close_document(backend_id)
                .map_err(|error| DocumentRuntimeError::BackendError(error.to_string()))?;
        }

        self.documents
            .remove(id)
            .ok_or(DocumentRuntimeError::EntityNotFound(id))?;
        self.sheets_clipboard.source_closed(id);
        self.writer_text_drafts.remove(&id);
        self.sheets_tools.retain(|(document, _), _| *document != id);
        self.new_document_formats.remove(&id);
        if let Some(path) = self.new_document_temp_paths.remove(&id) {
            let _ = std::fs::remove_file(path);
        }
        if was_active {
            self.active_document = None;
            self.idle_module = Module::Home;
            self.writer_display_mode = WriterDisplayMode::Visual;
            self.home_ribbon_open = false;
            self.sync_active_document_summary();
        }
        Ok(())
    }

    pub(crate) fn read_only_save_active(&self) -> bool {
        matches!(
            (self.active_module(), self.document()),
            (Module::Sheets, Some(OpenDocument::Sheets(_)))
                | (Module::Writer, Some(OpenDocument::Writer(_)))
                | (Module::Pdf, Some(OpenDocument::Pdf { .. }))
                | (Module::Slides, Some(OpenDocument::Slides(_)))
        )
    }

    pub(crate) fn can_save_new_document(&self) -> bool {
        self.active_document_id()
            .is_some_and(|id| self.new_document_formats.contains_key(&id))
    }

    pub(crate) fn apply_active_sheet_metadata(
        &self,
        api: &SpreadsheetApi<IroncalcAdapter>,
        view: &mut SheetsUi,
    ) -> Result<(), String> {
        let active = api.document().active_sheet();
        let snapshot = api.document().snapshot();
        let Some(sheet) = snapshot.sheets.iter().find(|sheet| sheet.id == active) else {
            return Ok(());
        };
        view.apply_sheet_metadata(
            &sheet.row_dimensions,
            &sheet.column_dimensions,
            &sheet.merges,
        )
        .map_err(|error| error.to_string())?;
        let mut hidden = sheet.manual_hidden_rows.clone();
        hidden.extend(api.filter_hidden_rows(active));
        hidden.sort_unstable();
        hidden.dedup();
        view.apply_hidden_rows(&hidden)
            .map_err(|error| error.to_string())?;
        view.apply_hidden_columns(&sheet.manual_hidden_columns)
            .map_err(|error| error.to_string())
    }

    #[allow(clippy::collapsible_if)]
    pub(crate) fn select_sheet_index(&mut self, index: usize) {
        let Some(sheet) = self
            .sheets_snapshot()
            .and_then(|snapshot| snapshot.sheets.get(index))
            .map(|sheet| sheet.id)
        else {
            return;
        };
        self.sync_active_session_view();
        if let Some(view) = self.sheets_view.as_mut() {
            view.cancel_pointer_selection();
        }
        self.sheets_selected = index;
        self.ribbon_command_feedback = None;

        if self.sheets_bounded_source_bytes().is_some() {
            let stored = self
                .active_session()
                .and_then(|session| session.sheets_selections.for_sheet(sheet))
                .cloned();
            let (scroll_x, scroll_y) = stored
                .map(|state| (state.scroll_x, state.scroll_y))
                .unwrap_or((0, 0));
            if let Some(session) = self.active_session_mut() {
                if let DocumentViewState::Sheets(view) = session.view_state_mut() {
                    view.selected_sheet = index;
                    view.scroll_x = scroll_x;
                    view.scroll_y = scroll_y;
                }
            }
            if let Err(error) = self.load_bounded_sheets_window(bounded_sheet_window_from_scroll(
                index, scroll_x, scroll_y,
            )) {
                self.status = "Не удалось загрузить область листа большой книги.".to_owned();
                self.detail = error.to_string();
                return;
            }
            self.sync_sheets_status();
            if let Err(error) = self.restore_sheets_selection() {
                self.detail = error;
            }
            self.sync_active_session_view();
            return;
        }

        if let Some(api) = self.sheets_api.as_mut() {
            if let Err(error) = api.set_active_sheet(sheet) {
                self.status = "Не удалось переключить лист.".to_owned();
                self.detail = error.to_string();
                return;
            }
        }
        if let Err(error) = self.sync_sheets_visual() {
            self.status = "Визуальный grid не обновлён.".to_owned();
            self.detail = error;
        }
        if let Err(error) = self.restore_sheets_selection() {
            self.detail = error;
        }
        self.sync_sheets_status();
        self.sync_active_session_view();
    }

    pub(crate) fn select_sheet_id(&mut self, sheet: SheetId) {
        if let Some(index) = self
            .sheets_snapshot()
            .and_then(|snapshot| snapshot.sheets.iter().position(|item| item.id == sheet))
        {
            self.select_sheet_index(index);
        }
    }

    pub(crate) fn dispatch_history_command(
        &mut self,
        target: RuntimeDocumentId,
        command: OfficeHistoryCommand,
    ) -> Result<(), String> {
        let kind = self
            .documents
            .require(target)
            .map_err(|error| error.to_string())?
            .kind();
        if self.active_document_id() != Some(target) {
            return Err("Команда истории адресована не активному документу.".to_owned());
        }
        if kind == Module::Sheets {
            if !self.sheets_editing_available() {
                return Err(
                    "История недоступна для ограниченной книги в режиме просмотра.".to_owned(),
                );
            }
            let api = self
                .sheets_api
                .as_mut()
                .ok_or("Движок таблицы недоступен.")?;
            match command {
                OfficeHistoryCommand::Undo => {
                    api.undo_sheet(api.document().active_sheet())
                        .map_err(|error| error.to_string())?;
                }
                OfficeHistoryCommand::Redo => {
                    api.redo_sheet(api.document().active_sheet())
                        .map_err(|error| error.to_string())?;
                }
            }
            self.commit_sheets_api_snapshot()?;
            self.status = match command {
                OfficeHistoryCommand::Undo => "Изменение таблицы отменено.",
                OfficeHistoryCommand::Redo => "Изменение таблицы повторено.",
            }
            .to_owned();
            return Ok(());
        } else if kind != Module::Writer {
            return Err("История изменений для этого формата пока недоступна.".to_owned());
        }
        self.apply_writer_home_command(match command {
            OfficeHistoryCommand::Undo => WriterHomeCommand::Undo,
            OfficeHistoryCommand::Redo => WriterHomeCommand::Redo,
        })
    }

    pub(crate) fn ribbon_command_available(&self, command: RibbonCommand) -> bool {
        if self.active_document_id().is_none() {
            return false;
        }
        let supported = tabs_for_format(self.active_module()).iter().any(|&tab| {
            commands_for(self.active_module(), tab)
                .iter()
                .any(|&(candidate, _)| candidate == command)
        });
        // Home's single-cell clear remains a compatibility command; Data now
        // uses the explicitly range-scoped operation instead.
        let supported = supported
            || (command == RibbonCommand::ClearCell && self.active_module() == Module::Sheets);
        if !supported {
            return false;
        }
        use RibbonCommand::*;
        match command {
            InsertRows | DeleteRows | InsertColumns | DeleteColumns => self.sheets_editing_available()
                && self.sheets_view.as_ref().is_some_and(|view| {
                    let frozen = view.frozen_panes();
                    frozen.rows == 0 && frozen.cols == 0 && view.selection().range().is_some()
                }),
            SortAscending | SortDescending | ApplyAutoFilter => self.sheets_editing_available() && self.sheets_tools_available(),
            ClearAutoFilter => self.sheets_editing_available() && self.sheets_tools_available()
                && self.sheets_api.as_ref().is_some_and(|api| api.document().snapshot().sheets.iter()
                    .find(|s| s.id == api.document().active_sheet()).is_some_and(|s| s.auto_filter.is_some())),
            AddSheet | DuplicateSheet | RowHeight32 | RowHeight48 | ColumnWidth120
            | ColumnWidth180 | ToggleWrap | ClearCell | ClearRange => self.sheets_editing_available(),
            MergeSelection => self.sheets_editing_available() && self.sheets_view.as_ref()
                .and_then(|view| view.selection().range()).is_some_and(|range| range.rows() > 1 || range.cols() > 1),
            UnmergeSelection => self.sheets_editing_available() && self.sheets_view.as_ref()
                .and_then(|view| view.selection().range()).is_some_and(|range| self.sheets_snapshot()
                    .and_then(|book| book.sheets.iter().find(|sheet| sheet.id == range.sheet))
                    .is_some_and(|sheet| sheet.merges.iter().any(|merge| merge.intersects(&range)))),
            GoToRange | FindNext => self.sheets_tools_available(),
            AutoSum => self.sheets_editing_available()
                && self.sheets_view.as_ref().and_then(|view| view.selection().focus).is_some_and(|address| address.row.0 > 0),
            PreviousSheet => self.sheets_selected > 0,
            NextSheet => self.sheets_snapshot().is_some_and(|book| self.sheets_selected + 1 < book.sheets.len()),
            InsertParagraph | InsertTable => self.document().is_some_and(|document| {
                matches!(document, OpenDocument::Writer(writer) if matches!(writer.api().selection().active().container(), TextContainerId::Block(_)))
            }),
            _ => true,
        }
    }

    pub(crate) fn apply_ribbon_command(&mut self, command: RibbonCommand) -> Result<(), String> {
        if matches!(
            command,
            RibbonCommand::InsertParagraph | RibbonCommand::InsertTable
        ) && self.writer_inline_draft_is_active()
        {
            return Err("Сначала примените или отмените открытый черновик Writer.".to_owned());
        }
        if matches!(
            command,
            RibbonCommand::InsertRows
                | RibbonCommand::DeleteRows
                | RibbonCommand::InsertColumns
                | RibbonCommand::DeleteColumns
        ) && self.active_module() == Module::Sheets
            && !self.sheets_editing_available()
        {
            return Err("Вставка и удаление строк/столбцов требуют полностью загруженной редактируемой книги. Source-backed просмотр не изменяет исходные данные.".into());
        }
        if matches!(
            command,
            RibbonCommand::InsertRows
                | RibbonCommand::DeleteRows
                | RibbonCommand::InsertColumns
                | RibbonCommand::DeleteColumns
        ) && self.active_module() == Module::Sheets
            && self.sheets_view.as_ref().is_some_and(|view| {
                let frozen = view.frozen_panes();
                frozen.rows > 0 || frozen.cols > 0
            })
        {
            return Err("Структурная операция отключена при закреплённых областях: их безопасный перенос ещё не поддержан. Снимите закрепление; данные и история не изменены.".into());
        }
        if !self.ribbon_command_available(command) {
            return Err("Команда недоступна в текущем документе или выделении.".to_owned());
        }
        use RibbonCommand::*;
        match command {
            ZoomIn | ZoomOut | ZoomReset => {
                let session = self
                    .active_session()
                    .ok_or_else(|| "Документ не открыт.".to_owned())?;
                let zoom = match *session.view_state() {
                    DocumentViewState::Writer(view) => view.zoom,
                    DocumentViewState::Sheets(view) => f64::from(view.zoom),
                    DocumentViewState::Pdf(view) => f64::from(view.zoom),
                    DocumentViewState::Slides(view) => f64::from(view.zoom),
                };
                let requested = match command {
                    ZoomIn => zoom + 0.1,
                    ZoomOut => zoom - 0.1,
                    _ => 1.0,
                };
                let applied = self
                    .zoom_active_document_to(requested)
                    .map_err(|error| error.to_string())?;
                self.status = format!("Масштаб: {}%.", (applied * 100.0).round());
            }
            InsertParagraph | InsertTable => {
                let Some(OpenDocument::Writer(writer)) = self.document_mut() else {
                    return Err("Нет Writer.".into());
                };
                let cursor = writer.api().selection().active();
                writer
                    .set_selection(Selection::collapsed(cursor))
                    .map_err(|error| error.to_string())?;
                writer
                    .execute(if command == InsertParagraph {
                        WriterCommand::SplitBlock
                    } else {
                        WriterCommand::InsertTable {
                            rows: 2,
                            columns: 2,
                        }
                    })
                    .map_err(|error| error.to_string())?;
                if let Some(id) = self.active_document_id() {
                    self.writer_text_drafts.remove(&id);
                }
                self.status = if command == InsertParagraph {
                    "Добавлен новый абзац."
                } else {
                    "Добавлена таблица 2 × 2."
                }
                .into();
            }
            Portrait | Landscape | PageA4 | NormalMargins | NarrowMargins => {
                let Some(OpenDocument::Writer(writer)) = self.document_mut() else {
                    return Err("Нет Writer.".into());
                };
                let mut settings = writer.api().layout_settings();
                match command {
                    Portrait | Landscape => {
                        let landscape = settings.page_size.width_half_points
                            > settings.page_size.height_half_points;
                        if landscape != (command == Landscape) {
                            settings.page_size = settings.page_size.rotated();
                        }
                    }
                    PageA4 => {
                        settings.page_size = writer_api::layout::PageSize::A4;
                    }
                    NormalMargins => {
                        settings.margins = writer_api::layout::PageMargins::default();
                    }
                    NarrowMargins => {
                        settings.margins = writer_api::layout::PageMargins {
                            top_half_points: 57,
                            right_half_points: 57,
                            bottom_half_points: 57,
                            left_half_points: 57,
                        };
                    }
                    _ => unreachable!(),
                }
                writer.set_layout_settings(settings);
                self.status = "Параметры страницы изменены.".into();
            }
            AddSheet | DuplicateSheet => {
                let api = self
                    .sheets_api
                    .as_mut()
                    .ok_or_else(|| "Нет Sheets API.".to_owned())?;
                if command == AddSheet {
                    api.add_sheet(None).map_err(|error| error.to_string())?;
                } else {
                    let sheet = api.document().active_sheet();
                    api.duplicate_sheet(sheet, None)
                        .map_err(|error| error.to_string())?;
                }
                self.commit_sheets_api_snapshot()?;
                let index = self
                    .sheets_snapshot()
                    .map(|book| book.sheets.len().saturating_sub(1))
                    .unwrap_or(0);
                self.select_sheet_index(index);
                self.status = "Добавлен лист книги.".into();
            }
            RowHeight32 | RowHeight48 | ColumnWidth120 | ColumnWidth180 => {
                let address = self.ensure_sheets_selection()?;
                let api = self
                    .sheets_api
                    .as_mut()
                    .ok_or_else(|| "Нет Sheets API.".to_owned())?;
                match command {
                    RowHeight32 | RowHeight48 => {
                        api.set_row_dimension(
                            address.sheet,
                            address.row,
                            if command == RowHeight32 { 32.0 } else { 48.0 },
                        )
                        .map_err(|error| error.to_string())?;
                    }
                    _ => {
                        api.set_column_dimension(
                            address.sheet,
                            address.column,
                            if command == ColumnWidth120 {
                                120.0
                            } else {
                                180.0
                            },
                        )
                        .map_err(|error| error.to_string())?;
                    }
                }
                self.commit_sheets_api_snapshot()?;
                self.status = "Размер выбранной строки или столбца изменён.".into();
            }
            ToggleWrap => return self.apply_sheets_home_format(SheetsHomeFormat::ToggleWrap),
            AutoSum => {
                let address = self.ensure_sheets_selection()?;
                let column = column_label(address.column);
                // Sum only rows above the target; never a self-reference.
                return self
                    .apply_sheets_cell_input(format!("=SUM({column}1:{column}{})", address.row.0));
            }
            ClearCell => return self.apply_sheets_cell_input(String::new()),
            GoToRange => return self.go_to_sheets_address(),
            FindNext => return self.find_next_sheets_cell(),
            SortAscending => return self.sort_sheets_selection(true),
            SortDescending => return self.sort_sheets_selection(false),
            ApplyAutoFilter => return self.filter_sheets_selection(),
            ClearAutoFilter => return self.clear_sheets_filter(),
            InsertRows | DeleteRows | InsertColumns | DeleteColumns => {
                return self.change_sheets_structure(command);
            }
            ClearRange | MergeSelection | UnmergeSelection => {
                self.ensure_sheets_selection()?;
                let range = self
                    .sheets_view
                    .as_ref()
                    .and_then(|view| view.selection().range())
                    .ok_or_else(|| "Выделение Sheets недоступно.".to_owned())?;
                if range
                    .rows()
                    .checked_mul(range.cols())
                    .is_none_or(|count| count > 10_000)
                {
                    return Err("Выделите не более 10 000 ячеек для этой операции.".into());
                }
                let api = self
                    .sheets_api
                    .as_mut()
                    .ok_or_else(|| "Нет Sheets API.".to_owned())?;
                match command {
                    ClearRange => {
                        api.clear_range(
                            range.sheet,
                            range,
                            FormatPatchBounds {
                                max_cells: 10_000,
                                max_sheets: 1,
                            },
                        )
                        .map_err(|error| error.to_string())?;
                    }
                    MergeSelection => {
                        api.safe_merge_cells(range.sheet, range)
                            .map_err(sheets_tools::mutation_error)?;
                    }
                    UnmergeSelection => {
                        api.unmerge_intersecting_selection(range.sheet, range)
                            .map_err(|error| error.to_string())?;
                    }
                    _ => unreachable!(),
                }
                self.commit_sheets_api_snapshot()?;
                self.status = match command {
                    ClearRange => "Содержимое выделения очищено; оформление сохранено.",
                    MergeSelection => "Ячейки объединены без потери данных.",
                    _ => "Объединение ячеек снято; содержимое сохранено.",
                }
                .into();
                self.detail =
                    "Изменение выполнено в текущей сессии; исходный XLSX не перезаписывается."
                        .into();
            }
            GoToA1 => {
                let sheet = self
                    .sheets_view
                    .as_ref()
                    .map(SheetsUi::active_sheet)
                    .ok_or_else(|| "Нет Sheets view.".to_owned())?;
                self.sheets_view
                    .as_mut()
                    .ok_or("Нет Sheets view.")?
                    .select_cell(CellAddress::new(sheet, RowIndex(0), ColumnIndex(0)))
                    .map_err(|error| error.to_string())?;
                self.status = "Выбрана ячейка A1.".into();
            }
            PreviousSheet => {
                self.select_sheet_index(self.sheets_selected.saturating_sub(1));
            }
            NextSheet => {
                self.select_sheet_index(self.sheets_selected.saturating_add(1));
            }
            AddSlide => return self.apply_slides_home_command(SlidesHomeCommand::Add),
            DuplicateSlide => return self.apply_slides_home_command(SlidesHomeCommand::Duplicate),
            PrintLayout => {
                self.set_writer_display_mode(WriterDisplayMode::Visual);
                self.set_active_view_mode(DocumentViewMode::WriterLayout(ViewMode::Pages))
                    .map_err(|error| error.to_string())?;
            }
            PlainText => {
                self.set_writer_display_mode(WriterDisplayMode::PlainText);
            }
            FitWidth => self.pdf_fit_width(),
            FitPage => self.pdf_fit_page(),
            PresentSlides => self.slides_toggle_presentation(),
        }
        self.sync_active_session_view();
        Ok(())
    }
}

impl OpenDocument {
    pub(crate) fn open(
        path: &Path,
        bytes: &[u8],
        pdf_engine: &mut PdfOxideAdapter,
    ) -> Result<(Self, String, String), String> {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| "Не удалось определить расширение файла.".to_owned())?;

        document_open_trace(
            "routing",
            "PASS",
            format!("format={extension} bytes={}", bytes.len()),
        );
        match extension.as_str() {
            "xlsx" => {
                let mut sheets = SpreadsheetApi::new(IroncalcAdapter::new());
                let snapshot = sheets.load_xlsx(bytes).map_err(|error| error.to_string())?;
                let document = Self::Sheets(snapshot);
                let (status, detail) = document.opening_summary();
                Ok((document, status, detail))
            }
            "docx" => {
                let mut writer = WriterViewModel::new();
                writer.load_docx(bytes).map_err(|error| error.to_string())?;
                let document = Self::Writer(Box::new(writer));
                let (status, detail) = document.opening_summary();
                Ok((document, status, detail))
            }
            "pdf" => {
                document_open_trace("backend_parse", "START", "format=pdf backend=PDFOxide");
                let info = PdfEngine::parse(pdf_engine, bytes).map_err(|error| {
                    document_open_trace(
                        "backend_parse",
                        "FAIL",
                        format!("format=pdf error={error}"),
                    );
                    format!("Этап PDFOxide/Rudra adapter parse: {error}")
                })?;
                document_open_trace(
                    "backend_parse",
                    "PASS",
                    format!("format=pdf pages={}", info.page_count),
                );
                document_open_trace(
                    "package_object_enumeration",
                    "PASS",
                    format!("format=pdf pages={}", info.page_count),
                );
                let document_id = info.document_id;
                let page_count = info.page_count;
                let page_ids = if info.page_ids.is_empty() {
                    (1..=page_count).map(|i| PageId::new(i as u64)).collect()
                } else {
                    info.page_ids
                };
                let safety_hints = pdf_engine.last_open_safety_hints();
                let document = Self::Pdf {
                    document_id,
                    page_count,
                    page_ids,
                };
                document_open_trace(
                    "rudra_adapter_projection",
                    "PASS",
                    format!(
                        "format=pdf pages={page_count} notices={}",
                        safety_hints.len()
                    ),
                );
                let (status, detail) = document.opening_summary();
                let detail = pdf_viewer_safety_detail(&safety_hints).unwrap_or(detail);
                Ok((document, status, detail))
            }
            "pptx" => {
                document_open_trace("backend_parse", "START", "format=pptx backend=OfficeOxide");
                let presentation = office_oxide_adapter().load_bytes(bytes).map_err(|error| {
                    document_open_trace(
                        "backend_parse",
                        "FAIL",
                        format!("format=pptx error={error}"),
                    );
                    format!("Этап Office Oxide parse: {error}")
                })?;
                document_open_trace(
                    "backend_parse",
                    "PASS",
                    format!("format=pptx slides={}", presentation.slides.len()),
                );
                let source = presentation_projection_counts(&presentation);
                document_open_trace(
                    "package_object_enumeration",
                    "PASS",
                    format!(
                        "format=pptx slides={} source_objects={} source_text={} source_images={} source_shapes={} source_lines={} source_tables={}",
                        presentation.slides.len(),
                        source.objects,
                        source.text,
                        source.images,
                        source.shapes,
                        source.lines,
                        source.tables,
                    ),
                );
                let document = Self::Slides(presentation);
                document_open_trace(
                    "rudra_adapter_projection",
                    "PASS",
                    format!(
                        "format=pptx viewer_model_objects={} viewer_model_text={} viewer_model_images={} viewer_model_shapes={} viewer_model_lines={} viewer_model_tables={}; render warnings are reported per visible slide",
                        source.objects,
                        source.text,
                        source.images,
                        source.shapes,
                        source.lines,
                        source.tables,
                    ),
                );
                let (status, detail) = document.opening_summary();
                Ok((document, status, detail))
            }
            _ => {
                Err("Неподдерживаемый формат. Используйте .xlsx, .docx, .pdf или .pptx.".to_owned())
            }
        }
    }

    pub(crate) fn pdf_from_info(info: DocumentInfo) -> Self {
        let page_count = info.page_count;
        let page_ids = if info.page_ids.is_empty() {
            (1..=page_count)
                .map(|index| PageId::new(index as u64))
                .collect()
        } else {
            info.page_ids
        };
        Self::Pdf {
            document_id: info.document_id,
            page_count,
            page_ids,
        }
    }

    pub(crate) fn rebind_pdf_info(self, info: DocumentInfo) -> Self {
        match self {
            Self::Pdf { .. } => Self::pdf_from_info(info),
            other => other,
        }
    }

    /// Parse PDF on a worker and return a detached artifact for UI adoption.
    /// Other formats retain the existing worker path and are returned without
    /// an artifact because their models are already owned by the result.
    pub(crate) fn open_background(
        path: &Path,
        bytes: &[u8],
    ) -> Result<(Self, Option<PdfOpenArtifact>, String, String), String> {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| "Не удалось определить расширение файла.".to_owned())?;

        if extension == "pdf" {
            document_open_trace(
                "routing",
                "PASS",
                format!("format=pdf bytes={}", bytes.len()),
            );
            document_open_trace(
                "backend_parse",
                "START",
                "format=pdf backend=PDFOxide worker",
            );
            let parser = PdfOxideAdapter::default();
            let artifact = parser.parse_detached(bytes).map_err(|error| {
                document_open_trace("backend_parse", "FAIL", format!("format=pdf error={error}"));
                format!("Этап PDFOxide/Rudra adapter parse: {error}")
            })?;
            let info = artifact.document_info();
            document_open_trace(
                "backend_parse",
                "PASS",
                format!("format=pdf pages={}", info.page_count),
            );
            document_open_trace(
                "package_object_enumeration",
                "PASS",
                format!("format=pdf pages={}", info.page_count),
            );
            let safety_hints = artifact.safety_hints();
            let document = Self::pdf_from_info(info);
            document_open_trace(
                "rudra_adapter_projection",
                "PASS",
                format!(
                    "format=pdf pages={} notices={}",
                    document
                        .pdf_bindings()
                        .map(|bindings| bindings.1)
                        .unwrap_or(0),
                    safety_hints.len()
                ),
            );
            let (status, detail) = document.opening_summary();
            let detail = pdf_viewer_safety_detail(&safety_hints).unwrap_or(detail);
            return Ok((document, Some(artifact), status, detail));
        }

        let mut pdf_engine = PdfOxideAdapter::default();
        let (document, status, detail) = Self::open(path, bytes, &mut pdf_engine)?;
        Ok((document, None, status, detail))
    }

    pub(crate) const fn module(&self) -> Module {
        match self {
            Self::Sheets(_) => Module::Sheets,
            Self::Writer(_) => Module::Writer,
            Self::Pdf { .. } => Module::Pdf,
            Self::Slides(_) => Module::Slides,
            Self::None => Module::Home,
        }
    }

    pub(crate) fn opening_summary(&self) -> (String, String) {
        match self {
            Self::Sheets(snapshot) => {
                let sheet_count = snapshot.sheets.len();
                let cells = snapshot
                    .sheets
                    .iter()
                    .map(|sheet| sheet.cells.len())
                    .sum::<usize>();
                (
                    format!("XLSX открыт: {sheet_count} лист(а/ов), {cells} заполненных ячеек."),
                    "Книга разобрана IronCalc; значения и формат ячеек можно менять в текущей сессии. Запись XLSX пока отключена."
                        .to_owned(),
                )
            }
            Self::Writer(writer) => {
                let block_count = writer
                    .state()
                    .pages
                    .first()
                    .map(|page| page.block_count)
                    .unwrap_or(0);
                (
                    format!("DOCX открыт: {block_count} блок(а/ов) в документе."),
                    "Документ передан в локальную модель Writer; форматирование доступно в текущей сессии. Запись DOCX пока отключена."
                        .to_owned(),
                )
            }
            Self::Pdf { page_count, .. } => (
                format!("PDF открыт: {page_count} страниц(а/ов)."),
                "PDF разобран PDFOxide и доступен только для просмотра без записи.".to_owned(),
            ),
            Self::Slides(presentation) => (
                format!("PPTX открыт: {} слайд(а/ов).", presentation.slides.len()),
                "Презентация разобрана Office Oxide; операции со слайдами доступны в текущей сессии. Запись PPTX пока отключена."
                    .to_owned(),
            ),
            Self::None => (
                "Документ не открыт.".to_owned(),
                "Откройте поддерживаемый файл.".to_owned(),
            ),
        }
    }

    pub(crate) fn pdf_bindings(&self) -> Option<(DocumentId, u32, &[PageId])> {
        match self {
            Self::Pdf {
                document_id,
                page_count,
                page_ids,
            } => Some((*document_id, *page_count, page_ids)),
            _ => None,
        }
    }

    pub(crate) fn export_bytes(&self) -> Result<Vec<u8>, String> {
        match self {
            Self::Sheets(snapshot) => XlsxPartsCodec::export(&IroncalcXlsxParts, snapshot)
                .map_err(|error| error.to_string()),
            Self::Writer(writer) => writer.export_docx().map_err(|error| error.to_string()),
            Self::Pdf { .. } => {
                Err("PDF save/export is disabled by ARCH-PDF-003 STRICT LOCK".to_owned())
            }
            Self::Slides(presentation) => office_oxide_adapter()
                .save_bytes(presentation)
                .map_err(|error| error.to_string()),
            Self::None => Err("Документ не открыт.".to_owned()),
        }
    }
}
