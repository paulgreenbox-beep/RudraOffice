//! Application-owned module extracted without changing behavior.
use super::*;

impl AppState {
    pub(crate) fn sheets_active(&self) -> bool {
        matches!(self.document(), Some(OpenDocument::Sheets(_)))
    }

    pub(crate) fn should_show_sheets_view(&self) -> bool {
        self.active_module() == Module::Sheets && self.sheets_active()
    }

    /// Borrows the workbook snapshot; the snapshot stays the single source of truth.
    pub(crate) fn sheets_snapshot(&self) -> Option<&WorkbookSnapshot> {
        match self.document() {
            Some(OpenDocument::Sheets(snapshot)) => Some(snapshot),
            _ => None,
        }
    }

    /// Returns the immutable original package only for a source-backed large
    /// workbook. Small workbooks intentionally continue using IronCalc's live
    /// document model and never retain duplicate package bytes.
    pub(crate) fn sheets_bounded_source_bytes(&self) -> Option<Arc<[u8]>> {
        let session = self.active_session()?;
        if !matches!(session.backend(), OpenDocument::Sheets(_)) {
            return None;
        }
        session.source().retained_bytes().cloned()
    }

    /// Replaces the current live Sheets snapshot with one bounded source
    /// rectangle, then rebinds the existing renderer-neutral UI controller.
    /// The original ZIP remains immutable on the owning DocumentSession.
    pub(crate) fn load_bounded_sheets_window(
        &mut self,
        window: BoundedSheetWindow,
    ) -> Result<(), DocumentRuntimeError> {
        let id = self.active_target()?;
        let (source_bytes, view_state) = {
            let session = self.documents.require(id)?;
            let source_bytes = session.source().retained_bytes().cloned().ok_or(
                DocumentRuntimeError::InvalidState("bounded Sheets source bytes are unavailable"),
            )?;
            let DocumentViewState::Sheets(view_state) = session.view_state() else {
                return Err(DocumentRuntimeError::InvalidState(
                    "bounded Sheets session/view mismatch",
                ));
            };
            (source_bytes, *view_state)
        };

        let snapshot = import_bounded_sheet_window(&source_bytes, window)
            .map_err(|error| DocumentRuntimeError::BackendError(error.to_string()))?;
        let visual_package = XlsxPartsCodec::export(&IroncalcXlsxParts, &snapshot)
            .map_err(|error| DocumentRuntimeError::BackendError(error.to_string()))?;

        let session = self.documents.require_mut(id)?;
        let OpenDocument::Sheets(current) = session.backend_mut() else {
            return Err(DocumentRuntimeError::InvalidState(
                "bounded Sheets session/backend mismatch",
            ));
        };
        *current = snapshot;

        let mut api = SpreadsheetApi::new(IroncalcAdapter::new());
        api.load_xlsx(&visual_package)
            .map_err(|error| DocumentRuntimeError::BackendError(error.to_string()))?;
        let retain_view = self
            .sheets_view
            .as_ref()
            .is_some_and(|view| view.active_sheet() == api.document().active_sheet());
        if retain_view {
            // Keep the same pure controller on an in-sheet window refresh:
            // selection, zoom, scroll and an in-flight drag belong to it.
            self.sheets_api = Some(api);
            self.sync_sheets_visual()
                .map_err(DocumentRuntimeError::BackendError)?;
        } else {
            self.bind_sheets_api(api)
                .map_err(DocumentRuntimeError::BackendError)?;
        }
        self.sheets_selected = window.sheet_index;
        self.sheets_bounded_window = Some(window);
        if !retain_view && let Some(sheet_view) = self.sheets_view.as_mut() {
            sheet_view.set_zoom(view_state.zoom);
            let _ = sheet_view.handle_event(UiEvent::Wheel {
                delta_x: i64::from(view_state.scroll_x),
                delta_y: i64::from(view_state.scroll_y),
            });
        }
        if !retain_view {
            self.restore_sheets_selection()
                .map_err(DocumentRuntimeError::BackendError)?;
        }
        Ok(())
    }

    /// Loads a later XLSX rectangle only after the currently visible grid
    /// escapes the resident range. This makes scroll/keyboard navigation
    /// viewport-driven instead of permanently truncating a large workbook.
    pub(crate) fn refresh_bounded_sheets_window_if_needed(
        &mut self,
    ) -> Result<bool, DocumentRuntimeError> {
        if self.sheets_bounded_source_bytes().is_none() {
            return Ok(false);
        }
        let Some(sheet_index) = self.sheets_selected_index() else {
            return Ok(false);
        };
        let Some(frame) = self.sheets_frame() else {
            return Ok(false);
        };
        let Some(min_row) = frame.row_headers.iter().map(|header| header.index).min() else {
            return Ok(false);
        };
        let Some(max_row) = frame.row_headers.iter().map(|header| header.index).max() else {
            return Ok(false);
        };
        let Some(min_column) = frame.column_headers.iter().map(|header| header.index).min() else {
            return Ok(false);
        };
        let Some(max_column) = frame.column_headers.iter().map(|header| header.index).max() else {
            return Ok(false);
        };
        if self.sheets_bounded_window.is_some_and(|window| {
            window.sheet_index == sheet_index
                && window.contains(min_row, min_column)
                && window.contains(max_row, max_column)
        }) {
            return Ok(false);
        }
        let scroll = self
            .sheets_view
            .as_ref()
            .map(SheetsUi::scroll_offset)
            .unwrap_or_default();
        self.sync_active_session_view();
        self.load_bounded_sheets_window(bounded_sheet_window_from_scroll(
            sheet_index,
            scroll.x,
            scroll.y,
        ))?;
        self.sync_sheets_status();
        Ok(true)
    }

    /// Selected sheet index clamped to the snapshot's current range.
    pub(crate) fn sheets_selected_index(&self) -> Option<usize> {
        let snapshot = self.sheets_snapshot()?;
        clamp_sheet_index(snapshot, self.sheets_selected)
    }

    /// Projects the currently selected sheet without mutating the snapshot.
    pub(crate) fn sheets_grid_view(&self) -> Option<SheetGridView> {
        let snapshot = self.sheets_snapshot()?;
        let index = clamp_sheet_index(snapshot, self.sheets_selected)?;
        snapshot.sheets.get(index).map(project_sheet_grid)
    }

    /// Resets the sheet selection to the snapshot's active sheet on open.
    #[cfg(test)]
    pub(crate) fn bind_sheets_document(&mut self) {
        self.sheets_selected = self
            .sheets_snapshot()
            .and_then(active_sheet_index)
            .unwrap_or(0);
    }

    pub(crate) fn bind_sheets_visual(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut api = SpreadsheetApi::new(IroncalcAdapter::new());
        api.load_xlsx(bytes).map_err(|error| error.to_string())?;
        self.bind_sheets_api(api)
    }

    pub(crate) fn bind_sheets_api(
        &mut self,
        api: SpreadsheetApi<IroncalcAdapter>,
    ) -> Result<(), String> {
        let view = self.create_sheets_view(&api);
        // Preserve the canonical lease even when projection/layout fails: the
        // activation rollback can return it to its owning DocumentSession.
        self.sheets_api = Some(api);
        self.sheets_view = None;
        self.sheets_view = Some(view?);
        Ok(())
    }

    pub(crate) fn create_sheets_view(
        &self,
        api: &SpreadsheetApi<IroncalcAdapter>,
    ) -> Result<SheetsUi, String> {
        let snapshot = UiSnapshot::from_api_document(api).map_err(|error| error.to_string())?;
        let (outer_width, outer_height) = self.sheets_viewport.unwrap_or((1180, 590));
        let mut view = SheetsUi::new(UiConfig {
            initial_sheet: snapshot.active_sheet,
            dimensions: snapshot.dimensions,
            outer_width,
            outer_height,
            // This host paints sheet tabs but no second status bar.
            metrics: sheets_ui::UiMetrics {
                status_bar_height: 0,
                ..Default::default()
            },
            ..UiConfig::default()
        })
        .map_err(|error| error.to_string())?;
        view.sync(snapshot).map_err(|error| error.to_string())?;
        self.apply_active_sheet_metadata(api, &mut view)?;
        Ok(view)
    }

    pub(crate) fn restore_sheets_selection(&mut self) -> Result<(), String> {
        let Some(sheet) = self
            .sheets_api
            .as_ref()
            .map(|api| api.document().active_sheet())
        else {
            return Ok(());
        };
        let stored = self
            .active_session()
            .and_then(|session| session.sheets_selections.for_sheet(sheet))
            .cloned();
        if let Some(stored) = stored
            && let Some(view) = self.sheets_view.as_mut()
        {
            // Restoration is not navigation: keep a manually scrolled viewport
            // even if its active cell is offscreen. Keyboard extension still
            // reveals its active endpoint inside SheetsUi.
            view.restore_selection(stored.selection)
                .map_err(|error| error.to_string())?;
            let current = view.scroll_offset();
            view.handle_event(UiEvent::Wheel {
                delta_x: i64::from(stored.scroll_x) - i64::from(current.x),
                delta_y: i64::from(stored.scroll_y) - i64::from(current.y),
            })
            .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    /// Keeps pure Sheets layout in the same coordinate space as the actual
    /// Makepad canvas. The selection is re-clamped by SheetsUi on resize.
    pub(crate) fn sync_sheets_viewport(&mut self, width: u32, height: u32) -> bool {
        if width == 0 || height == 0 {
            return false;
        }
        let viewport = (width, height);
        if self.sheets_viewport == Some(viewport) {
            return false;
        }
        self.sheets_viewport = Some(viewport);
        let resized = self
            .sheets_view
            .as_mut()
            .is_some_and(|view| view.handle_event(UiEvent::Resize { width, height }).is_ok());
        if resized {
            self.sync_active_session_view();
        }
        resized
    }

    pub(crate) fn sync_sheets_visual(&mut self) -> Result<(), String> {
        let Some(api) = self.sheets_api.as_ref() else {
            return Ok(());
        };
        let snapshot = UiSnapshot::from_api_document(api).map_err(|error| error.to_string())?;
        let Some(mut view) = self.sheets_view.take() else {
            return Ok(());
        };
        let result = view
            .sync(snapshot)
            .map_err(|error| error.to_string())
            .and_then(|_| self.apply_active_sheet_metadata(api, &mut view));
        self.sheets_view = Some(view);
        result
    }

    #[allow(clippy::collapsible_if)]
    pub(crate) fn sheets_select_cell(&mut self, address: CellAddress) {
        if let Some(view) = self.sheets_view.as_mut() {
            if let Err(error) = view.select_cell(address) {
                self.status = "Ячейка недоступна.".to_owned();
                self.detail = error.to_string();
            }
        }
        self.sync_active_session_view();
    }

    pub(crate) fn sheets_handle_selection_input(&mut self, input: UiEvent) {
        if let Some(view) = self.sheets_view.as_mut()
            && let Err(error) = view.handle_event(input)
        {
            self.status = "Выделение недоступно.".to_owned();
            self.detail = error.to_string();
        }
        self.sync_active_session_view();
        if let Err(error) = self.refresh_bounded_sheets_window_if_needed() {
            self.status = "Не удалось загрузить область листа большой книги.".to_owned();
            self.detail = error.to_string();
        }
    }

    pub(crate) fn sheets_handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        let mapped = match key {
            KeyCode::ArrowUp => sheets_ui::Key::ArrowUp,
            KeyCode::ArrowDown => sheets_ui::Key::ArrowDown,
            KeyCode::ArrowLeft => sheets_ui::Key::ArrowLeft,
            KeyCode::ArrowRight => sheets_ui::Key::ArrowRight,
            KeyCode::Tab => sheets_ui::Key::Tab,
            KeyCode::ReturnKey | KeyCode::NumpadEnter => sheets_ui::Key::Enter,
            KeyCode::Escape => sheets_ui::Key::Escape,
            KeyCode::KeyA if modifiers.control || modifiers.logo => sheets_ui::Key::Character('a'),
            _ => return,
        };
        self.sheets_handle_selection_input(UiEvent::KeyDown {
            key: mapped,
            modifiers: sheets_grid_widget::sheets_modifiers(modifiers),
        });
    }

    pub(crate) fn sheets_wheel(&mut self, delta_x: i64, delta_y: i64, control: bool) {
        if control {
            if delta_y < 0 {
                self.sheets_zoom_in();
            } else if delta_y > 0 {
                self.sheets_zoom_out();
            }
        } else {
            let _ = self.scroll_active_document(delta_x as f64, delta_y as f64);
        }
    }

    pub(crate) fn sheets_zoom_in(&mut self) {
        let current = self.sheets_view.as_ref().map(SheetsUi::zoom).unwrap_or(1.0);
        let _ = self.zoom_active_document_to(f64::from(current + 0.1));
    }

    pub(crate) fn sheets_zoom_out(&mut self) {
        let current = self.sheets_view.as_ref().map(SheetsUi::zoom).unwrap_or(1.0);
        let _ = self.zoom_active_document_to(f64::from(current - 0.1));
    }

    pub(crate) fn sheets_frame(&self) -> Option<sheets_ui::UiFrame> {
        self.sheets_view.as_ref()?.frame().ok()
    }

    pub(crate) fn clear_sheets_document(&mut self) {
        self.sheets_selected = 0;
        self.sheets_bounded_window = None;
        self.sheets_api = None;
        self.sheets_view = None;
    }

    pub(crate) fn sheets_previous(&mut self) {
        if self.sheets_selected_index().is_some() {
            self.select_sheet_index(self.sheets_selected.saturating_sub(1));
        }
    }

    pub(crate) fn sheets_next(&mut self) {
        let Some(snapshot) = self.sheets_snapshot() else {
            return;
        };
        let last = snapshot.sheets.len().saturating_sub(1);
        if self.sheets_selected < last {
            self.select_sheet_index(self.sheets_selected + 1);
        }
    }

    pub(crate) fn sheets_can_previous(&self) -> bool {
        self.sheets_selected_index().is_some_and(|index| index > 0)
    }

    pub(crate) fn sheets_can_next(&self) -> bool {
        match self.sheets_snapshot() {
            Some(snapshot) => self
                .sheets_selected_index()
                .is_some_and(|index| index + 1 < snapshot.sheets.len()),
            None => false,
        }
    }

    /// Refreshes the status/detail lines after a sheet selection change.
    pub(crate) fn sync_sheets_status(&mut self) {
        let Some(view) = self.sheets_grid_view() else {
            return;
        };
        let selected = self.sheets_selected_index();
        let tabs = self
            .sheets_snapshot()
            .map(|snapshot| sheet_tabs_label(snapshot, selected))
            .unwrap_or_default();
        self.status = sheet_grid_status(&view);
        self.detail = tabs;
        if let Some(window) = self.sheets_bounded_window {
            let end_row = window.start_row.saturating_add(window.row_count);
            let end_column = window.start_column.saturating_add(window.column_count);
            self.status.push_str(" Большая книга: показана область.");
            self.detail = format!(
                "{} · Большая книга: данные загружаются по области (строки {}–{}, столбцы {}–{}).",
                self.detail,
                window.start_row.saturating_add(1),
                end_row,
                column_label(ColumnIndex(window.start_column)),
                column_label(ColumnIndex(end_column.saturating_sub(1))),
            );
        }
    }

    pub(crate) fn sheets_can_undo(&self) -> bool {
        self.sheets_editing_available()
            && self
                .sheets_api
                .as_ref()
                .is_some_and(|api| api.can_undo_sheet(api.document().active_sheet()))
    }

    pub(crate) fn sheets_can_redo(&self) -> bool {
        self.sheets_editing_available()
            && self
                .sheets_api
                .as_ref()
                .is_some_and(|api| api.can_redo_sheet(api.document().active_sheet()))
    }

    pub(crate) fn sheets_editing_available(&self) -> bool {
        self.should_show_sheets_view()
            && self.sheets_bounded_source_bytes().is_none()
            && self.sheets_api.is_some()
            && self.sheets_view.is_some()
    }

    pub(crate) fn sheets_clipboard_range(&self) -> Result<CellRange, String> {
        self.sheets_view
            .as_ref()
            .and_then(|view| view.selection().range())
            .ok_or_else(|| "Выберите прямоугольный диапазон ячеек.".into())
    }

    pub(crate) fn sheets_copy_available(&self) -> bool {
        self.should_show_sheets_view()
            // Window geometry does not prove source formula values or merges.
            // Fail closed until this projection has complete provenance.
            && self.sheets_bounded_source_bytes().is_none()
            && self.sheets_api.is_some()
            && self.sheets_clipboard_range().is_ok_and(|range| {
                range
                    .rows()
                    .checked_mul(range.cols())
                    .is_some_and(|area| area <= 10_000)
            })
    }

    pub(crate) fn capture_sheets_clipboard(&self, cut: bool) -> Result<ClipboardPayload, String> {
        if !self.sheets_copy_available() {
            return Err("Copy требует полностью загруженного диапазона не более 10 000 ячеек. В source-backed книге значения формул и merge topology пока не подтверждены; Copy отключён, чтобы не выдать неполные данные за полный результат.".into());
        }
        if cut && !self.sheets_editing_available() {
            return Err("Cut требует полностью загруженной редактируемой книги. Используйте Copy для source-backed просмотра.".into());
        }
        self.sheets_api
            .as_ref()
            .ok_or("Нет Sheets API.")?
            .copy_range(self.sheets_clipboard_range()?)
            .map_err(sheets_tools::mutation_error)
    }

    pub(crate) fn claim_sheets_clipboard(
        &mut self,
        payload: ClipboardPayload,
        cut: bool,
        observation: windows_clipboard::ClipboardObservation,
    ) -> Result<(), String> {
        if observation.text != payload.tsv() {
            return Err("Буфер Windows изменился до подтверждения Copy; предыдущий внутренний буфер сохранён.".into());
        }
        let source = self.active_target().map_err(|error| error.to_string())?;
        self.sheets_clipboard.replace(ClipboardClaim {
            source,
            revision: payload.source_revision(),
            range: payload.source_range().clone(),
            pending_cut: cut,
            payload,
            text: observation.text,
            sequence: observation.sequence,
        });
        self.status = if cut {
            "Диапазон отмечен для Cut; исходные ячейки сохраняются до успешной вставки.".into()
        } else {
            "Диапазон скопирован: формулы и оформление внутри Rudra, отображаемые значения — в буфере Windows.".into()
        };
        self.detail = "Copy/Paste работает с логическим прямоугольником, включая скрытые строки; исходный XLSX не перезаписывается.".into();
        Ok(())
    }

    pub(crate) fn paste_sheets_clipboard(
        &mut self,
        observation: windows_clipboard::ClipboardObservation,
    ) -> Result<(), String> {
        if !self.sheets_editing_available() {
            return Err("Paste требует полностью загруженной редактируемой книги; source-backed просмотр не изменяется.".into());
        }
        let destination = self.sheets_clipboard_range()?;
        let active = self.active_target().map_err(|error| error.to_string())?;
        // An external overwrite invalidates pending Cut as well as rich matching.
        if self
            .sheets_clipboard
            .matching(observation.sequence, &observation.text)
            .is_none()
        {
            self.sheets_clipboard.cancel_cut();
        }
        let claim = self
            .sheets_clipboard
            .matching(observation.sequence, &observation.text);
        let api = self.sheets_api.as_mut().ok_or("Нет Sheets API.")?;
        let was_cut = claim.is_some_and(|claim| claim.pending_cut);
        let target = match claim {
            Some(claim) if claim.pending_cut => {
                if claim.source != active {
                    return Err(
                        "Cut между разными книгами пока не поддерживается. Используйте Copy."
                            .into(),
                    );
                }
                if !api.cut_payload_valid(&claim.payload) {
                    self.sheets_clipboard.cancel_cut();
                    return Err(
                        "Исходная книга изменилась; pending Cut отменён. Повторите Cut.".into(),
                    );
                }
                api.cut_paste(&claim.payload, destination)
            }
            Some(claim) => api.paste_range(&claim.payload, destination),
            None => api.paste_tsv(&observation.text, destination),
        }
        .map_err(sheets_tools::mutation_error)?;
        if was_cut {
            self.sheets_clipboard.cancel_cut();
        }
        self.commit_sheets_api_snapshot()?;
        if let Some(view) = self.sheets_view.as_mut() {
            // Keep logical rectangle cardinality even when its endpoint is hidden.
            // SheetsUi normalizes the active focus, not the canonical mutation.
            view.restore_selection(sheets_ui::SelectionState {
                kind: if target.start == target.end {
                    sheets_ui::SelectionKind::Cell
                } else {
                    sheets_ui::SelectionKind::Range
                },
                anchor: Some(target.start),
                focus: Some(target.start),
                extent: Some(target.clone()),
            })
            .map_err(|error| error.to_string())?;
            if let Some(focus) = view.selection().focus
                && !target.contains(&focus)
            {
                // A completely filtered-out destination keeps a deterministic
                // visible survivor, not an enlarged range for the next command.
                view.select_cell(focus).map_err(|error| error.to_string())?;
            }
        }
        self.sync_active_session_view();
        self.status = if was_cut {
            "Cut/Paste выполнен одной отменяемой операцией.".into()
        } else {
            "Диапазон вставлен одной отменяемой операцией.".into()
        };
        self.detail = "Размер назначения: одна ячейка или такой же прямоугольник. Undo/Redo хранит изменения в существующей истории Sheets; исходный XLSX не перезаписывается.".into();
        Ok(())
    }

    pub(crate) fn sheets_selected_raw_text(&self) -> Option<String> {
        let address = self.sheets_view.as_ref()?.active_input_address()?;
        self.sheets_api
            .as_ref()?
            .cell_view(address.sheet, address)
            .ok()
            .map(|cell| cell.raw_text())
    }

    pub(crate) fn ensure_sheets_selection(&mut self) -> Result<CellAddress, String> {
        if self.sheets_bounded_source_bytes().is_some() {
            return Err(
                "Редактирование большой source-backed книги пока отключено; просмотр остаётся доступен."
                    .to_owned(),
            );
        }
        if let Some(address) = self
            .sheets_view
            .as_ref()
            .and_then(SheetsUi::active_input_address)
        {
            return Ok(address);
        }
        let sheet = self
            .sheets_api
            .as_ref()
            .ok_or_else(|| "Модель Sheets не инициализирована.".to_owned())?
            .document()
            .active_sheet();
        let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        self.sheets_view
            .as_mut()
            .ok_or_else(|| "Представление Sheets не инициализировано.".to_owned())?
            .select_cell(address)
            .map_err(|error| error.to_string())?;
        Ok(address)
    }

    pub(crate) fn commit_sheets_api_snapshot(&mut self) -> Result<(), String> {
        let snapshot = self
            .sheets_api
            .as_ref()
            .ok_or_else(|| "Модель Sheets не инициализирована.".to_owned())?
            .document()
            .snapshot();
        let id = self.active_target().map_err(|error| error.to_string())?;
        self.sheets_clipboard.source_changed(id, snapshot.revision);
        let session = self
            .documents
            .require_mut(id)
            .map_err(|error| error.to_string())?;
        let OpenDocument::Sheets(current) = session.backend_mut() else {
            return Err("Активная сессия не содержит Sheets workbook.".to_owned());
        };
        *current = snapshot;
        self.sync_sheets_visual()?;
        self.sync_active_session_view();
        Ok(())
    }

    pub(crate) fn apply_sheets_cell_input(&mut self, raw: String) -> Result<(), String> {
        let address = self.ensure_sheets_selection()?;
        let input = EntryParser::parse(&raw)
            .and_then(|entry| entry.into_input())
            .map_err(|error| error.to_string())?;
        self.sheets_api
            .as_mut()
            .ok_or_else(|| "Модель Sheets не инициализирована.".to_owned())?
            .set_cell_input(address.sheet, address, input)
            .map_err(|error| error.to_string())?;
        self.commit_sheets_api_snapshot()?;
        self.status = format!(
            "Sheets: ячейка {}{} обновлена.",
            column_label(address.column),
            address.row.0 + 1
        );
        self.detail = "Изменение выполнено в текущей сессии; запись исходного XLSX пока отключена."
            .to_owned();
        Ok(())
    }

    pub(crate) fn apply_sheets_home_format(
        &mut self,
        command: SheetsHomeFormat,
    ) -> Result<(), String> {
        let color_value = sheets_color_accessibility_value(&command);
        let address = self.ensure_sheets_selection()?;
        let range: CellRange = self
            .sheets_view
            .as_ref()
            .and_then(|view| view.selection().range())
            .ok_or_else(|| "Выделение Sheets недоступно.".to_owned())?;
        let current = self
            .sheets_api
            .as_ref()
            .ok_or_else(|| "Модель Sheets не инициализирована.".to_owned())?
            .cell_view(address.sheet, address)
            .map_err(|error| error.to_string())?
            .format
            .unwrap_or_default();
        let (patch, label) = match command {
            SheetsHomeFormat::ToggleBold => (
                SheetsFormatPatch {
                    bold: Some(!current.font.bold),
                    ..Default::default()
                },
                "полужирный",
            ),
            SheetsHomeFormat::ToggleItalic => (
                SheetsFormatPatch {
                    italic: Some(!current.font.italic),
                    ..Default::default()
                },
                "курсив",
            ),
            SheetsHomeFormat::ToggleUnderline => (
                SheetsFormatPatch {
                    underline: Some(!current.font.underline),
                    ..Default::default()
                },
                "подчёркивание",
            ),
            SheetsHomeFormat::AlignLeft => (
                SheetsFormatPatch {
                    horizontal_alignment: Some("left".to_owned()),
                    ..Default::default()
                },
                "по левому краю",
            ),
            SheetsHomeFormat::AlignCenter => (
                SheetsFormatPatch {
                    horizontal_alignment: Some("center".to_owned()),
                    ..Default::default()
                },
                "по центру",
            ),
            SheetsHomeFormat::AlignRight => (
                SheetsFormatPatch {
                    horizontal_alignment: Some("right".to_owned()),
                    ..Default::default()
                },
                "по правому краю",
            ),
            SheetsHomeFormat::ToggleWrap => (
                SheetsFormatPatch {
                    wrap: Some(!current.alignment.wrap),
                    ..Default::default()
                },
                "перенос текста",
            ),
            SheetsHomeFormat::SetTextColor(color) => (
                SheetsFormatPatch {
                    text_color: Some(color),
                    ..Default::default()
                },
                "цвет текста",
            ),
            SheetsHomeFormat::SetCellBackground(color) => (
                SheetsFormatPatch {
                    background: Some(color),
                    ..Default::default()
                },
                "заливка всей ячейки",
            ),
        };
        self.sheets_api
            .as_mut()
            .ok_or_else(|| "Модель Sheets не инициализирована.".to_owned())?
            .apply_format_range(
                address.sheet,
                range,
                patch,
                FormatPatchBounds {
                    max_cells: 10_000,
                    max_sheets: 1,
                },
            )
            .map_err(|error| error.to_string())?;
        self.commit_sheets_api_snapshot()?;
        self.status = format!("Sheets: формат «{label}» применён.");
        self.detail = "Изменение выполнено в текущей сессии; запись исходного XLSX пока отключена."
            .to_owned();
        if let Some(value) = color_value {
            self.sheets_color_value = Some(value);
        }
        Ok(())
    }

    pub(crate) fn sheets_tools_available(&self) -> bool {
        self.should_show_sheets_view()
            && self.sheets_bounded_source_bytes().is_none()
            && self.sheets_api.is_some()
            && self.sheets_view.is_some()
    }

    pub(crate) fn sheets_tools_key(&self) -> Option<(RuntimeDocumentId, SheetId)> {
        Some((
            self.active_document_id()?,
            self.sheets_view.as_ref()?.active_sheet(),
        ))
    }

    pub(crate) fn sheets_tools_draft(&self) -> SheetsToolsDraft {
        self.sheets_tools_key()
            .and_then(|key| self.sheets_tools.get(&key))
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn update_sheets_tools_draft(
        &mut self,
        address: Option<String>,
        query: Option<String>,
    ) {
        if let Some(key) = self.sheets_tools_key() {
            let draft = self.sheets_tools.entry(key).or_default();
            if let Some(address) = address {
                draft.address = address;
            }
            if let Some(query) = query
                && query != draft.query
            {
                draft.query = query;
                draft.last_match = None;
            }
        }
    }

    pub(crate) fn go_to_sheets_address(&mut self) -> Result<(), String> {
        if !self.sheets_tools_available() {
            return Err(
                "Переход к адресу пока доступен только в полностью загруженной книге.".into(),
            );
        }
        let draft = self.sheets_tools_draft();
        let view = self
            .sheets_view
            .as_ref()
            .ok_or_else(|| "Нет Sheets view.".to_owned())?;
        let config = self
            .sheets_api
            .as_ref()
            .ok_or_else(|| "Нет Sheets API.".to_owned())?
            .config();
        let mut dimensions = UiConfig::default().dimensions;
        dimensions.rows = config.max_document_rows;
        dimensions.columns = config.max_document_columns;
        let (start, end) =
            sheets_ui::parse_a1_range(draft.address.trim(), view.active_sheet(), dimensions)
                .map_err(|error| format!("Некорректный адрес. Укажите A1 или A1:C5: {error}"))?;
        let hidden = self
            .sheets_api
            .as_ref()
            .unwrap()
            .hidden_rows(view.active_sheet());
        if let Some(address) = [start, end]
            .into_iter()
            .find(|address| hidden.contains(&address.row))
        {
            return Err(format!(
                "Строка {} скрыта фильтром или вручную. Переход отклонён; выделение и фильтр не изменены.",
                address.row.0 + 1
            ));
        }
        if let Some(address) = [start, end].into_iter().find(|address| {
            self.sheets_view
                .as_ref()
                .is_some_and(|view| view.is_column_hidden(address.column))
        }) {
            return Err(format!(
                "Столбец {} скрыт вручную. Переход отклонён; выделение не изменено.",
                column_label(address.column)
            ));
        }
        self.sheets_view
            .as_mut()
            .unwrap()
            .select_range(start, end)
            .map_err(|error| error.to_string())?;
        self.status = format!(
            "Выбран диапазон {}{}:{}{}.",
            column_label(start.column),
            start.row.0 + 1,
            column_label(end.column),
            end.row.0 + 1
        );
        self.detail = "Переход меняет только выделение и прокрутку текущего листа.".into();
        self.sync_active_session_view();
        Ok(())
    }

    pub(crate) fn update_sheets_data_draft(
        &mut self,
        key_column: Option<String>,
        has_header: Option<bool>,
        filter_kind: Option<usize>,
        filter_value: Option<String>,
    ) {
        if let Some(key) = self.sheets_tools_key() {
            let draft = self.sheets_tools.entry(key).or_default();
            if let Some(value) = key_column {
                draft.key_column = value;
            }
            if let Some(value) = has_header {
                draft.has_header = value;
            }
            if let Some(value) = filter_kind {
                draft.filter_kind = value;
            }
            if let Some(value) = filter_value {
                draft.filter_value = value;
            }
        }
    }

    pub(crate) fn sort_sheets_selection(&mut self, ascending: bool) -> Result<(), String> {
        if !self.sheets_editing_available() || !self.sheets_tools_available() {
            return Err("Сортировка требует полностью загруженной редактируемой книги.".into());
        }
        let range = self
            .sheets_view
            .as_ref()
            .and_then(|view| view.selection().range())
            .ok_or_else(|| "Выберите прямоугольный диапазон данных.".to_owned())?;
        let draft = self.sheets_tools_draft();
        if range.rows() <= 1 + u64::from(draft.has_header) {
            return Err("Для сортировки выберите не менее двух строк данных; диапазон автоматически не расширяется.".into());
        }
        let key_column = sheets_tools::parse_key_column(
            &draft.key_column,
            range.start.column,
            range.end.column,
        )?;
        self.sheets_api
            .as_mut()
            .ok_or("Нет Sheets API.")?
            .sort_range(
                range.sheet,
                sheets_api::SortOptions {
                    range,
                    key_column,
                    ascending,
                    has_header: draft.has_header,
                },
            )
            .map_err(sheets_tools::mutation_error)?;
        self.commit_sheets_api_snapshot()?;
        self.status = format!(
            "Диапазон отсортирован {} по столбцу {}.",
            if ascending {
                "по возрастанию"
            } else {
                "по убыванию"
            },
            column_label(key_column)
        );
        self.detail = "Строки диапазона перенесены вместе с оформлением. Относительные формулы пересчитаны IronCalc; исходный XLSX не записывается.".into();
        Ok(())
    }

    pub(crate) fn filter_sheets_selection(&mut self) -> Result<(), String> {
        if !self.sheets_editing_available() || !self.sheets_tools_available() {
            return Err("Фильтр требует полностью загруженной редактируемой книги.".into());
        }
        let range = self
            .sheets_view
            .as_ref()
            .and_then(|view| view.selection().range())
            .ok_or_else(|| "Выберите прямоугольный диапазон для фильтра.".to_owned())?;
        let draft = self.sheets_tools_draft();
        let column = sheets_tools::parse_key_column(
            &draft.key_column,
            range.start.column,
            range.end.column,
        )?;
        let criterion =
            sheets_tools::parse_filter_criterion(draft.filter_kind, &draft.filter_value)?;
        let snapshot = self
            .sheets_api
            .as_ref()
            .ok_or("Нет Sheets API.")?
            .document()
            .snapshot();
        let mut criteria = snapshot
            .sheets
            .iter()
            .find(|s| s.id == range.sheet)
            .and_then(|s| s.auto_filter.as_ref())
            .filter(|filter| filter.range == range && filter.has_header == draft.has_header)
            .map(|filter| filter.criteria.clone())
            .unwrap_or_default();
        criteria.insert(column, criterion);
        self.sheets_api
            .as_mut()
            .unwrap()
            .set_auto_filter(
                range.sheet,
                AutoFilter {
                    range,
                    has_header: draft.has_header,
                    criteria,
                },
            )
            .map_err(sheets_tools::mutation_error)?;
        self.commit_sheets_api_snapshot()?;
        self.status = "Фильтр применён: неподходящие строки скрыты, данные сохранены.".into();
        self.detail = "Фильтр принадлежит текущему листу и отменяется одной командой. Поиск пропускает скрытые строки; переход к скрытой строке отклоняется без изменения фильтра.".into();
        Ok(())
    }

    pub(crate) fn clear_sheets_filter(&mut self) -> Result<(), String> {
        if !self.sheets_editing_available() || !self.sheets_tools_available() {
            return Err("Снятие фильтра требует полностью загруженной редактируемой книги.".into());
        }
        let api = self.sheets_api.as_mut().ok_or("Нет Sheets API.")?;
        api.clear_auto_filter(api.document().active_sheet())
            .map_err(sheets_tools::mutation_error)?;
        self.commit_sheets_api_snapshot()?;
        self.status = "Фильтр снят; вручную скрытые строки остаются скрытыми.".into();
        Ok(())
    }

    pub(crate) fn change_sheets_structure(&mut self, command: RibbonCommand) -> Result<(), String> {
        use sheets_structural_tools::{StructuralAxis, plan_structural_selection};
        let range = self
            .sheets_view
            .as_ref()
            .and_then(|view| view.selection().range())
            .ok_or_else(|| "Выделите строки или столбцы для структурной операции.".to_owned())?;
        let plan = plan_structural_selection(&range, command).map_err(|error| error.to_string())?;
        let deleting = matches!(
            command,
            RibbonCommand::DeleteRows | RibbonCommand::DeleteColumns
        );
        let api = self.sheets_api.as_mut().ok_or("Нет Sheets API.")?;
        if range.sheet != api.document().active_sheet() {
            return Err("Выделение принадлежит другому листу; книга не изменена.".into());
        }
        let result = match (plan.axis, deleting) {
            (StructuralAxis::Rows, false) => {
                api.insert_rows(range.sheet, RowIndex(plan.at), plan.count)
            }
            (StructuralAxis::Rows, true) => {
                api.delete_rows(range.sheet, RowIndex(plan.at), plan.count)
            }
            (StructuralAxis::Columns, false) => {
                api.insert_columns(range.sheet, ColumnIndex(plan.at), plan.count)
            }
            (StructuralAxis::Columns, true) => {
                api.delete_columns(range.sheet, ColumnIndex(plan.at), plan.count)
            }
        };
        result.map_err(|error| {
            format!(
                "Структурная операция отклонена: {}",
                sheets_tools::mutation_error(error)
                    .chars()
                    .take(320)
                    .collect::<String>()
            )
        })?;
        self.commit_sheets_api_snapshot()?;
        // Restoration accepts a hidden target, then normalizes its focus to a
        // visible survivor. Filtered-out inserted rows are not falsely exposed.
        let target = plan.target_range;
        if let Some(view) = self.sheets_view.as_mut() {
            view.restore_selection(sheets_ui::SelectionState {
                kind: if target.start == target.end {
                    sheets_ui::SelectionKind::Cell
                } else {
                    sheets_ui::SelectionKind::Range
                },
                anchor: Some(target.start),
                focus: Some(target.start),
                extent: Some(target.clone()),
            })
            .map_err(|error| error.to_string())?;
            if let Some(focus) = view.selection().focus
                && !target.contains(&focus)
            {
                // If the entire affected band is hidden, choose its visible
                // survivor rather than silently enlarging the next command's
                // count to include both the hidden band and that survivor.
                let mut visible_target = target;
                if focus.row < visible_target.start.row || focus.row > visible_target.end.row {
                    visible_target.start.row = focus.row;
                    visible_target.end.row = focus.row;
                }
                if focus.column < visible_target.start.column
                    || focus.column > visible_target.end.column
                {
                    visible_target.start.column = focus.column;
                    visible_target.end.column = focus.column;
                }
                view.restore_selection(sheets_ui::SelectionState {
                    kind: sheets_ui::SelectionKind::Range,
                    anchor: Some(focus),
                    focus: Some(focus),
                    extent: Some(visible_target),
                })
                .map_err(|error| error.to_string())?;
            }
        }
        if let Some(key) = self.sheets_tools_key() {
            let focus = self
                .sheets_view
                .as_ref()
                .and_then(|view| view.selection().focus);
            let draft = self.sheets_tools.entry(key).or_default();
            draft.last_match = None;
            if let Some(address) = focus {
                draft.address = format!("{}{}", column_label(address.column), address.row.0 + 1);
            }
        }
        self.status = format!(
            "{} {}: {} (начало {}).",
            if deleting {
                "Удалены"
            } else {
                "Вставлены"
            },
            if plan.axis == StructuralAxis::Rows {
                "строки"
            } else {
                "столбцы"
            },
            plan.count,
            plan.at + 1
        );
        self.detail = "Каноническая книга и ссылки изменены IronCalc. Операция отменяется целиком; исходный XLSX не записывается.".into();
        self.sync_active_session_view();
        Ok(())
    }

    pub(crate) fn find_next_sheets_cell(&mut self) -> Result<(), String> {
        if !self.sheets_tools_available() {
            return Err("Поиск пока доступен только в полностью загруженной книге.".into());
        }
        let key = self
            .sheets_tools_key()
            .ok_or_else(|| "Нет активного листа.".to_owned())?;
        let draft = self.sheets_tools_draft();
        let query = draft.query.trim();
        if query.is_empty() || query.chars().count() > 256 {
            return Err("Введите текст для поиска: от 1 до 256 символов.".into());
        }
        let mut cells = self
            .sheets_api
            .as_ref()
            .unwrap()
            .cell_views(key.1)
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(sheets_ui::UiCell::from)
            .collect::<Vec<_>>();
        let api = self.sheets_api.as_ref().unwrap();
        let mut hidden = api.filter_hidden_rows(key.1);
        if let Some(sheet) = api
            .document()
            .snapshot()
            .sheets
            .iter()
            .find(|s| s.id == key.1)
        {
            hidden.extend_from_slice(&sheet.manual_hidden_rows);
        }
        let hidden: BTreeSet<_> = hidden.into_iter().collect();
        let hidden_columns: BTreeSet<_> = api
            .document()
            .snapshot()
            .sheets
            .iter()
            .find(|sheet| sheet.id == key.1)
            .map(|sheet| sheet.manual_hidden_columns.iter().copied().collect())
            .unwrap_or_default();
        cells.retain(|cell| {
            !hidden.contains(&cell.address.row) && !hidden_columns.contains(&cell.address.column)
        });
        let selected = self
            .sheets_view
            .as_ref()
            .and_then(|view| view.selection().focus);
        let after = draft.last_match.filter(|last| Some(*last) == selected);
        let address = next_sheets_match(&cells, query, after)
            .ok_or_else(|| "На текущем листе совпадений не найдено.".to_owned())?;
        self.sheets_view
            .as_mut()
            .unwrap()
            .select_cell(address)
            .map_err(|error| error.to_string())?;
        let draft = self.sheets_tools.entry(key).or_default();
        draft.last_match = Some(address);
        draft.address = format!("{}{}", column_label(address.column), address.row.0 + 1);
        self.status = format!(
            "Найдено в {}{}. Следующее нажатие продолжит поиск.",
            column_label(address.column),
            address.row.0 + 1
        );
        self.detail = "Поиск по видимым строкам текущего листа: значения и формулы, без учёта регистра, с переходом в начало.".into();
        self.sync_active_session_view();
        Ok(())
    }
}

impl App {
    pub(crate) fn dispatch_sheets_clipboard(
        &mut self,
        cx: &mut Cx,
        command: SheetsClipboardCommand,
    ) {
        self.state
            .sheets_clipboard
            .observe_sequence(windows_clipboard::current_sequence());
        let result = match command {
            SheetsClipboardCommand::Copy | SheetsClipboardCommand::Cut => {
                let cut = command == SheetsClipboardCommand::Cut;
                self.state
                    .capture_sheets_clipboard(cut)
                    .and_then(|payload| {
                        // The bounded Windows adapter records its own sequence under
                        // the same write lock. Commit rich ownership only after verification.
                        let observation = windows_clipboard::write_text_verified(
                            payload.tsv(),
                            MAX_CLIPBOARD_TEXT_BYTES,
                        )?;
                        self.state.claim_sheets_clipboard(payload, cut, observation)
                    })
            }
            SheetsClipboardCommand::Paste => windows_clipboard::read_text(MAX_CLIPBOARD_TEXT_BYTES)
                .and_then(|observation| {
                    observation.ok_or_else(|| {
                        "В буфере Windows нет поддерживаемого текста/TSV.".to_owned()
                    })
                })
                .and_then(|observation| self.state.paste_sheets_clipboard(observation)),
        };
        self.state.ribbon_command_feedback = Some(match &result {
            Ok(()) => self.state.status.clone(),
            Err(error) => format!(
                "Буфер обмена: {}",
                error.chars().take(220).collect::<String>()
            ),
        });
        self.finish_home_command(cx, result);
    }

    pub(crate) fn sheets_grid_has_key_focus(&self, cx: &Cx) -> bool {
        let area = self.ui.widget(cx, ids!(sheets_grid_canvas)).area();
        self.state.should_show_sheets_view() && !area.is_empty() && cx.has_key_focus(area)
    }

    #[cfg(windows)]
    pub(crate) fn refresh_native_paste_target(&self, cx: &Cx) {
        let slot = self.native_paste_hook.borrow();
        if let Some(hook) = slot.as_ref() {
            let target = self
                .sheets_grid_has_key_focus(cx)
                .then(|| {
                    self.state
                        .active_document_id()
                        .zip(self.state.sheets_clipboard_range().ok())
                })
                .flatten()
                .map(|(document, range)| windows_grid_paste_hook::PasteTarget { document, range });
            hook.update_target(target);
        }
    }

    /// Reads the initialized Makepad canvas after Draw and reflows the pure
    /// Sheets UI only when its physical viewport changed.
    pub(crate) fn sync_sheets_canvas_viewport(&mut self, cx: &mut Cx) -> bool {
        let viewport = self
            .ui
            .widget(cx, ids!(sheets_grid_canvas))
            .borrow_mut::<SheetsGridCanvas>()
            .and_then(|mut canvas| canvas.take_viewport_change(cx));
        viewport.is_some_and(|(width, height)| self.state.sync_sheets_viewport(width, height))
    }
}
