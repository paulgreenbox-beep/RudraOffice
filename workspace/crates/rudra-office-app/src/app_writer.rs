//! Application-owned module extracted without changing behavior.
use super::*;

impl AppState {
    pub(crate) fn writer_inline_draft_is_active(&self) -> bool {
        self.writer_inline_session
            .as_ref()
            .is_some_and(|session| self.active_document_id() == Some(session.document_id))
    }

    pub(crate) fn begin_writer_inline_session(
        &mut self,
        document_id: RuntimeDocumentId,
        block_id: u64,
    ) -> Result<WriterInlineEditorSession, String> {
        if self.writer_inline_session.is_some() {
            return Err("Сначала примените или отмените открытый черновик Writer.".to_owned());
        }
        if self.active_document_id() != Some(document_id) {
            return Err("Документ Writer больше не активен.".to_owned());
        }
        let Some(OpenDocument::Writer(writer)) = self.document() else {
            return Err("Активный документ Writer не найден.".to_owned());
        };
        let snapshot = writer.snapshot();
        let block = snapshot
            .blocks
            .iter()
            .find_map(|block| match block {
                BlockSnapshot::Text(value) if value.id.get() == block_id => Some(value),
                _ => None,
            })
            .ok_or_else(|| "Редактируемый абзац Writer больше не существует.".to_owned())?;
        let text = block.text.clone();
        let epoch = self
            .writer_inline_epoch
            .checked_add(1)
            .ok_or_else(|| "Исчерпан счётчик сессий редактирования Writer.".to_owned())?;
        let session = WriterInlineEditorSession {
            document_id,
            block_id,
            revision: snapshot.revision,
            epoch,
        };
        self.writer_inline_epoch = epoch;
        self.writer_inline_session = Some(session.clone());
        self.writer_text_drafts.insert(
            document_id,
            WriterTextEditorDraft {
                selected_block_id: Some(block_id),
                text,
            },
        );
        Ok(session)
    }

    pub(crate) fn update_writer_inline_draft(
        &mut self,
        session: &WriterInlineEditorSession,
        text: String,
    ) -> Result<(), String> {
        if self.writer_inline_session.as_ref() != Some(session)
            || self.documents.get(session.document_id).is_none()
        {
            return Err("Сессия редактирования Writer устарела.".to_owned());
        }
        self.writer_text_drafts.insert(
            session.document_id,
            WriterTextEditorDraft {
                selected_block_id: Some(session.block_id),
                text,
            },
        );
        Ok(())
    }

    pub(crate) fn commit_writer_inline_draft(
        &mut self,
        session: &WriterInlineEditorSession,
        text: String,
    ) -> Result<(), String> {
        if self.writer_inline_session.as_ref() != Some(session)
            || self.documents.get(session.document_id).is_none()
        {
            return Err("Сессия редактирования Writer устарела.".to_owned());
        }
        if self.active_document_id() != Some(session.document_id) {
            return Err("Вернитесь к документу с черновиком Writer.".to_owned());
        }
        let Some(OpenDocument::Writer(writer)) = self.document_mut() else {
            return Err("Активный документ Writer не найден.".to_owned());
        };
        let snapshot = writer.snapshot();
        let block = snapshot
            .blocks
            .iter()
            .find_map(|block| match block {
                BlockSnapshot::Text(value) if value.id.get() == session.block_id => Some(value),
                _ => None,
            })
            .ok_or_else(|| "Редактируемый абзац Writer больше не существует.".to_owned())?;
        if snapshot.revision != session.revision {
            return Err(
                "Документ Writer изменился; черновик сохранён для разрешения конфликта.".to_owned(),
            );
        }
        if text != block.text {
            let (start, end, replacement) = writer_minimal_text_change(&block.text, &text);
            let container = TextContainerId::Block(BlockId::new(session.block_id));
            let selection = Selection::new(
                Cursor::new(container, TextOffset::new(start)),
                Cursor::new(container, TextOffset::new(end)),
            );
            writer
                .replace_range_atomically(
                    selection,
                    TextRange::new(TextOffset::new(start), TextOffset::new(end)),
                    replacement,
                )
                .map_err(|error| error.to_string())?;
        }
        self.writer_text_drafts.insert(
            session.document_id,
            WriterTextEditorDraft {
                selected_block_id: Some(session.block_id),
                text,
            },
        );
        self.writer_inline_session = None;
        self.status = "Writer: текст абзаца обновлён.".to_owned();
        self.detail = "Изменение применено к сессии Writer; Ctrl+Z отменяет его.".to_owned();
        self.sync_active_session_view();
        Ok(())
    }

    pub(crate) fn cancel_writer_inline_session(
        &mut self,
        session: &WriterInlineEditorSession,
    ) -> Result<(), String> {
        if self.writer_inline_session.as_ref() != Some(session)
            || self.active_document_id() != Some(session.document_id)
        {
            return Err(
                "Сессия редактирования Writer устарела или документ не активен.".to_owned(),
            );
        }
        self.writer_text_drafts.remove(&session.document_id);
        self.writer_inline_session = None;
        Ok(())
    }

    pub(crate) fn set_writer_viewport(
        &mut self,
        id: RuntimeDocumentId,
        zoom: f64,
        scroll_x: f64,
        scroll_y: f64,
    ) -> Result<(), DocumentRuntimeError> {
        let kind = self.documents.require(id)?.kind();
        if kind != Module::Writer {
            return Err(DocumentRuntimeError::UnsupportedOperation {
                id,
                operation: DocumentOperation::Scroll,
                kind,
            });
        }
        if !scroll_x.is_finite() || !scroll_y.is_finite() {
            return Err(DocumentRuntimeError::InvalidState(
                "Writer scroll offsets must be finite",
            ));
        }
        self.set_zoom(id, zoom)?;
        let session = self.documents.require_mut(id)?;
        let DocumentViewState::Writer(view) = session.view_state_mut() else {
            return Err(DocumentRuntimeError::InvalidState(
                "Writer session/view mismatch",
            ));
        };
        view.scroll_x = scroll_x.max(0.0);
        view.scroll_y = scroll_y.max(0.0);
        Ok(())
    }

    pub(crate) fn writer_active(&self) -> bool {
        matches!(self.document(), Some(OpenDocument::Writer(_)))
    }

    pub(crate) fn should_show_writer_view(&self) -> bool {
        self.active_module() == Module::Writer && self.writer_active()
    }

    pub(crate) fn set_writer_display_mode(&mut self, mode: WriterDisplayMode) {
        let _ = self.set_active_view_mode(DocumentViewMode::WriterDisplay(mode));
        self.status.clear();
        self.detail.clear();
    }

    pub(crate) fn apply_writer_view_mode(&mut self, mode: ViewMode) {
        if self
            .set_active_view_mode(DocumentViewMode::WriterLayout(mode))
            .is_ok()
        {
            self.status = "Режим просмотра обновлён.".to_string();
            self.detail.clear();
        }
    }

    pub(crate) fn writer_character_toolbar_available(&self) -> bool {
        let Some(OpenDocument::Writer(writer)) = self.document() else {
            return false;
        };
        let selection = writer.api().selection();
        if !selection.is_collapsed() {
            return writer.toolbar_state().bold.enabled;
        }
        writer_container_len(&writer.snapshot(), selection.active().container())
            .is_some_and(|length| length > 0)
    }

    pub(crate) fn writer_paragraph_toolbar_available(&self) -> bool {
        let Some(OpenDocument::Writer(writer)) = self.document() else {
            return false;
        };
        writer.toolbar_state().alignment.enabled
    }

    pub(crate) fn writer_can_undo(&self) -> bool {
        matches!(self.document(), Some(OpenDocument::Writer(writer)) if writer.api().can_undo())
    }

    pub(crate) fn writer_can_redo(&self) -> bool {
        matches!(self.document(), Some(OpenDocument::Writer(writer)) if writer.api().can_redo())
    }

    pub(crate) fn writer_text_editor_blocks(&self) -> Vec<WriterTextEditorBlock> {
        match self.document() {
            Some(OpenDocument::Writer(writer)) => writer_text_editor_blocks(writer),
            _ => Vec::new(),
        }
    }

    pub(crate) fn writer_text_editor_selected_block_id(
        &self,
        document_id: RuntimeDocumentId,
        blocks: &[WriterTextEditorBlock],
    ) -> Option<u64> {
        if let Some(selected_block_id) = self
            .writer_text_drafts
            .get(&document_id)
            .and_then(|draft| draft.selected_block_id)
            && blocks
                .iter()
                .any(|block| block.block_id == selected_block_id)
        {
            return Some(selected_block_id);
        }
        if let Some(selected_block_id) = match self.document() {
            Some(OpenDocument::Writer(writer)) => writer_selected_block_id(writer),
            _ => None,
        } && blocks
            .iter()
            .any(|block| block.block_id == selected_block_id)
        {
            return Some(selected_block_id);
        }
        blocks.first().map(|block| block.block_id)
    }

    pub(crate) fn writer_text_editor_model(&self) -> Option<WriterTextEditorModel> {
        if !self.writer_active() {
            return None;
        }
        let document_id = self.active_document_id()?;
        let blocks = self.writer_text_editor_blocks();
        let labels = writer_text_editor_labels(&blocks);
        let Some(selected_block_id) =
            self.writer_text_editor_selected_block_id(document_id, &blocks)
        else {
            return Some(WriterTextEditorModel {
                labels,
                selected_index: WRITER_TEXT_EDITOR_PROMPT_INDEX,
                text: String::new(),
                enabled: false,
            });
        };
        let selected_index = blocks
            .iter()
            .position(|block| block.block_id == selected_block_id)
            .map(|index| index + 1)
            .unwrap_or(WRITER_TEXT_EDITOR_PROMPT_INDEX);
        let selected_block = blocks
            .iter()
            .find(|block| block.block_id == selected_block_id)?;
        let text = self
            .writer_text_drafts
            .get(&document_id)
            .filter(|draft| draft.selected_block_id == Some(selected_block_id))
            .map(|draft| draft.text.clone())
            .unwrap_or_else(|| selected_block.text.clone());
        Some(WriterTextEditorModel {
            labels,
            selected_index,
            text,
            enabled: true,
        })
    }

    pub(crate) fn writer_text_editor_available(&self) -> bool {
        self.writer_text_editor_model()
            .is_some_and(|model| model.enabled)
    }

    pub(crate) fn writer_text_editor_selected_block(
        &self,
    ) -> Result<(RuntimeDocumentId, WriterTextEditorBlock), String> {
        let document_id = self
            .active_document_id()
            .ok_or_else(|| "Активный документ Writer не найден.".to_owned())?;
        let blocks = self.writer_text_editor_blocks();
        let selected_block_id = self
            .writer_text_editor_selected_block_id(document_id, &blocks)
            .ok_or_else(|| "В документе нет текстового абзаца для редактирования.".to_owned())?;
        let block = blocks
            .into_iter()
            .find(|block| block.block_id == selected_block_id)
            .ok_or_else(|| "Выбранный абзац Writer больше не существует.".to_owned())?;
        Ok((document_id, block))
    }

    pub(crate) fn select_writer_block_text_range(
        &mut self,
        block_id: u64,
        text: &str,
        start_byte: usize,
        end_byte: usize,
    ) -> Result<(), String> {
        let (start, end) = writer_char_range_from_byte_range(text, start_byte, end_byte)?;
        let Some(OpenDocument::Writer(writer)) = self.document_mut() else {
            return Err("Активный документ Writer не найден.".to_owned());
        };
        let snapshot = writer.snapshot();
        let actual = snapshot.blocks.iter().find_map(|block| match block {
            BlockSnapshot::Text(value) if value.id.get() == block_id => Some(value.text.as_str()),
            _ => None,
        });
        if actual != Some(text) {
            return Err("Текст абзаца изменился; выделите фрагмент повторно.".to_owned());
        }
        let container = TextContainerId::Block(BlockId::new(block_id));
        writer
            .set_selection(Selection::new(
                Cursor::new(container, TextOffset::new(start)),
                Cursor::new(container, TextOffset::new(end)),
            ))
            .map_err(|error| error.to_string())
    }

    pub(crate) fn select_writer_text_editor_block(
        &mut self,
        dropdown_index: usize,
    ) -> Result<(), String> {
        if self.writer_inline_draft_is_active() {
            return Err("Сначала примените или отмените открытый черновик Writer.".to_owned());
        }
        if dropdown_index == WRITER_TEXT_EDITOR_PROMPT_INDEX {
            return Ok(());
        }
        let document_id = self
            .active_document_id()
            .ok_or_else(|| "Активный документ Writer не найден.".to_owned())?;
        let block = self
            .writer_text_editor_blocks()
            .into_iter()
            .nth(dropdown_index.saturating_sub(1))
            .ok_or_else(|| "Выбранный абзац Writer недоступен.".to_owned())?;
        let Some(OpenDocument::Writer(writer)) = self.document_mut() else {
            return Err("Активный документ Writer не найден.".to_owned());
        };
        writer
            .set_selection(Selection::collapsed(Cursor::new(
                TextContainerId::Block(BlockId::new(block.block_id)),
                TextOffset::new(0),
            )))
            .map_err(|error| error.to_string())?;
        self.writer_text_drafts.insert(
            document_id,
            WriterTextEditorDraft {
                selected_block_id: Some(block.block_id),
                text: block.text,
            },
        );
        self.status = "Writer: выбран абзац для редактирования.".to_owned();
        self.detail = "Введите новый текст в поле «Текст абзаца» и нажмите «Применить».".to_owned();
        self.sync_active_session_view();
        Ok(())
    }

    pub(crate) fn update_writer_text_editor_draft(&mut self, text: String) {
        if self.writer_inline_draft_is_active() {
            return;
        }
        let Ok((document_id, block)) = self.writer_text_editor_selected_block() else {
            return;
        };
        self.writer_text_drafts.insert(
            document_id,
            WriterTextEditorDraft {
                selected_block_id: Some(block.block_id),
                text,
            },
        );
    }

    pub(crate) fn apply_writer_text_editor(&mut self, text: String) -> Result<(), String> {
        if self.writer_inline_draft_is_active() {
            return Err("Сначала примените или отмените открытый черновик Writer.".to_owned());
        }
        let (document_id, block) = self.writer_text_editor_selected_block()?;
        if text == block.text {
            self.writer_text_drafts.insert(
                document_id,
                WriterTextEditorDraft {
                    selected_block_id: Some(block.block_id),
                    text,
                },
            );
            self.status = "Writer: текст абзаца не изменился.".to_owned();
            self.detail = "Черновик синхронизирован с текущей сессией.".to_owned();
            return Ok(());
        }

        let container = TextContainerId::Block(BlockId::new(block.block_id));
        let (start, end, replacement) = writer_minimal_text_change(&block.text, &text);
        let Some(OpenDocument::Writer(writer)) = self.document_mut() else {
            return Err("Активный документ Writer не найден.".to_owned());
        };
        writer
            .set_selection(Selection::new(
                Cursor::new(container, TextOffset::new(start)),
                Cursor::new(container, TextOffset::new(end)),
            ))
            .map_err(|error| error.to_string())?;
        writer
            .execute(WriterCommand::ReplaceRange {
                range: TextRange::new(TextOffset::new(start), TextOffset::new(end)),
                text: replacement,
            })
            .map_err(|error| error.to_string())?;

        self.writer_text_drafts.insert(
            document_id,
            WriterTextEditorDraft {
                selected_block_id: Some(block.block_id),
                text,
            },
        );
        self.status = "Writer: текст абзаца обновлён.".to_owned();
        self.detail = "Изменение применено к реальной сессии Writer; Ctrl+Z отменяет его. Запись исходного DOCX пока отключена."
            .to_owned();
        self.sync_active_session_view();
        Ok(())
    }

    pub(crate) fn reset_writer_text_editor_draft(&mut self) -> Result<String, String> {
        if self.writer_inline_draft_is_active() {
            return Err("Сначала примените или отмените открытый черновик Writer.".to_owned());
        }
        let (document_id, block) = self.writer_text_editor_selected_block()?;
        let text = block.text.clone();
        self.writer_text_drafts.insert(
            document_id,
            WriterTextEditorDraft {
                selected_block_id: Some(block.block_id),
                text: text.clone(),
            },
        );
        self.status = "Черновик Writer возвращён к тексту абзаца.".to_owned();
        self.detail = "Применённые изменения отменяются отдельной командой Ctrl+Z.".to_owned();
        Ok(text)
    }

    pub(crate) fn apply_writer_home_command(
        &mut self,
        command: WriterHomeCommand,
    ) -> Result<(), String> {
        if self.writer_inline_draft_is_active() {
            return Err("Сначала примените или отмените открытый черновик Writer.".to_owned());
        }
        let clears_text_draft =
            matches!(&command, WriterHomeCommand::Undo | WriterHomeCommand::Redo);
        let color_value = writer_color_accessibility_value(&command);
        let outcome = {
            let Some(OpenDocument::Writer(writer)) = self.document_mut() else {
                return Err("Активный документ Writer не найден.".to_owned());
            };
            match command {
                WriterHomeCommand::Undo => {
                    let changed = writer.undo().map_err(|error| error.to_string())?;
                    if changed {
                        "Writer: последнее изменение отменено.".to_owned()
                    } else {
                        "Writer: отменять нечего.".to_owned()
                    }
                }
                WriterHomeCommand::Redo => {
                    let changed = writer.redo().map_err(|error| error.to_string())?;
                    if changed {
                        "Writer: изменение повторено.".to_owned()
                    } else {
                        "Writer: повторять нечего.".to_owned()
                    }
                }
                WriterHomeCommand::SetFontFamily(value) => {
                    let value = value.trim();
                    if value.is_empty() {
                        return Err("Введите название шрифта.".to_owned());
                    }
                    writer_prepare_character_selection(writer)?;
                    let result = writer
                        .dispatch_toolbar(ToolbarAction::SetFontFamily(Some(value.to_owned())))
                        .map_err(|error| error.to_string())?;
                    writer_toolbar_result("шрифт", result)?
                }
                WriterHomeCommand::SetFontSizePoints(points) => {
                    if !(1..=400).contains(&points) {
                        return Err("Размер шрифта должен быть от 1 до 400 pt.".to_owned());
                    }
                    writer_prepare_character_selection(writer)?;
                    let result = writer
                        .dispatch_toolbar(ToolbarAction::SetSizeHalfPoints(points * 2))
                        .map_err(|error| error.to_string())?;
                    writer_toolbar_result("размер шрифта", result)?
                }
                WriterHomeCommand::ToggleBold => {
                    writer_prepare_character_selection(writer)?;
                    let active =
                        matches!(writer.toolbar_state().bold.value, ToolbarValue::Known(true));
                    let result = writer
                        .dispatch_toolbar(ToolbarAction::SetBold(!active))
                        .map_err(|error| error.to_string())?;
                    writer_toolbar_result("полужирный", result)?
                }
                WriterHomeCommand::ToggleItalic => {
                    writer_prepare_character_selection(writer)?;
                    let active = matches!(
                        writer.toolbar_state().italic.value,
                        ToolbarValue::Known(true)
                    );
                    let result = writer
                        .dispatch_toolbar(ToolbarAction::SetItalic(!active))
                        .map_err(|error| error.to_string())?;
                    writer_toolbar_result("курсив", result)?
                }
                WriterHomeCommand::ToggleUnderline => {
                    writer_prepare_character_selection(writer)?;
                    let active = matches!(
                        writer.toolbar_state().underline.value,
                        ToolbarValue::Known(true)
                    );
                    let result = writer
                        .dispatch_toolbar(ToolbarAction::SetUnderline(!active))
                        .map_err(|error| error.to_string())?;
                    writer_toolbar_result("подчёркивание", result)?
                }
                WriterHomeCommand::ToggleStrikethrough => {
                    writer_prepare_character_selection(writer)?;
                    let active = matches!(
                        writer.toolbar_state().strikethrough.value,
                        ToolbarValue::Known(true)
                    );
                    let result = writer
                        .dispatch_toolbar(ToolbarAction::SetStrikethrough(!active))
                        .map_err(|error| error.to_string())?;
                    writer_toolbar_result("зачёркивание", result)?
                }
                WriterHomeCommand::SetForeground(value) => {
                    writer_prepare_character_selection(writer)?;
                    let result = writer
                        .dispatch_toolbar(ToolbarAction::SetForeground(value))
                        .map_err(|error| error.to_string())?;
                    writer_toolbar_result("цвет текста", result)?
                }
                WriterHomeCommand::SetBackground(value) => {
                    writer_prepare_character_selection(writer)?;
                    let result = writer
                        .dispatch_toolbar(ToolbarAction::SetBackground(value))
                        .map_err(|error| error.to_string())?;
                    writer_toolbar_result("заливка текста", result)?
                }
                WriterHomeCommand::SetAlignment(alignment) => {
                    let result = writer
                        .dispatch_toolbar(ToolbarAction::SetAlignment(alignment))
                        .map_err(|error| error.to_string())?;
                    writer_toolbar_result("выравнивание", result)?
                }
                WriterHomeCommand::SetHeading(level) => {
                    let label = level.map_or("обычный стиль".to_owned(), |level| {
                        format!("заголовок {level}")
                    });
                    let result = writer
                        .dispatch_toolbar(ToolbarAction::SetHeading(level))
                        .map_err(|error| error.to_string())?;
                    writer_toolbar_result(&label, result)?
                }
            }
        };
        self.status = outcome;
        self.detail = "Изменение выполнено в текущей сессии; запись исходного DOCX пока отключена."
            .to_owned();
        if let Some(value) = color_value {
            self.writer_color_value = Some(value);
        }
        if clears_text_draft && let Some(document_id) = self.active_document_id() {
            self.writer_text_drafts.remove(&document_id);
        }
        self.sync_active_session_view();
        Ok(())
    }

    pub(crate) fn apply_writer_ruler_change(
        &mut self,
        marker_id: &str,
        logical_position: f64,
    ) -> Result<(), String> {
        if self.writer_inline_draft_is_active() {
            return Err("Сначала примените или отмените открытый черновик Writer.".to_owned());
        }
        if !logical_position.is_finite() {
            return Err("Линейка передала некорректную координату.".to_owned());
        }
        let target = writer_ruler_target(marker_id)
            .ok_or_else(|| format!("Неизвестный маркер линейки: {marker_id}"))?;
        let absolute_half_points = (logical_position / PIXELS_PER_HALF_POINT_AT_100).round() as i32;
        let outcome = {
            let Some(OpenDocument::Writer(writer)) = self.document_mut() else {
                return Err("Активный документ Writer не найден.".to_owned());
            };
            let settings = writer.api().layout_settings();
            match target {
                WriterRulerTarget::ParagraphFirstLine(block_id)
                | WriterRulerTarget::ParagraphLeft(block_id)
                | WriterRulerTarget::ParagraphRight(block_id) => {
                    let snapshot = writer.snapshot();
                    let text = snapshot
                        .blocks
                        .iter()
                        .find_map(|block| match block {
                            BlockSnapshot::Text(text) if text.id.get() == block_id => Some(text),
                            _ => None,
                        })
                        .ok_or_else(|| "Абзац маркера линейки больше не существует.".to_owned())?;
                    let style = &text.paragraph_style;
                    let (left, right, first_line) = writer_indents_from_ruler(
                        target,
                        absolute_half_points,
                        settings.page_size.width_half_points,
                        settings.margins,
                        style.indent_left_half_points(),
                        style.indent_right_half_points(),
                        style.first_line_indent_half_points(),
                    );
                    writer
                        .execute(WriterCommand::SetParagraphStyle {
                            block_id: writer_api::core::BlockId::new(block_id),
                            patch: ParagraphStylePatch::default().indents(left, right, first_line),
                        })
                        .map_err(|error| error.to_string())?;
                    format!(
                        "Writer: отступы абзаца обновлены (левый {left}, правый {right}, первая строка {first_line} half-pt)."
                    )
                }
                WriterRulerTarget::HeaderDistance => {
                    if writer.docx_layout().sections.is_empty() {
                        return Err(
                            "В документе нет секции DOCX для верхнего колонтитула.".to_owned()
                        );
                    }
                    let max_distance = settings
                        .page_size
                        .height_half_points
                        .saturating_sub(22)
                        .max(22);
                    let distance = u32::try_from(absolute_half_points.max(22))
                        .unwrap_or(max_distance)
                        .min(max_distance);
                    writer.set_header_distance_half_points(distance);
                    format!("Writer: положение верхнего колонтитула — {distance} half-pt.")
                }
                WriterRulerTarget::FooterDistance => {
                    if writer.docx_layout().sections.is_empty() {
                        return Err(
                            "В документе нет секции DOCX для нижнего колонтитула.".to_owned()
                        );
                    }
                    let page_height = settings.page_size.height_half_points.max(44);
                    let baseline = u32::try_from(absolute_half_points.max(22))
                        .unwrap_or(page_height.saturating_sub(22))
                        .clamp(22, page_height.saturating_sub(22));
                    let distance = page_height.saturating_sub(baseline).max(22);
                    writer.set_footer_distance_half_points(distance);
                    format!("Writer: положение нижнего колонтитула — {distance} half-pt от низа.")
                }
            }
        };
        self.status = outcome;
        self.detail =
            "Изменение линейки выполнено в текущей сессии; запись исходного DOCX пока отключена."
                .to_owned();
        self.sync_active_session_view();
        Ok(())
    }
}

impl App {
    pub(crate) fn writer_inline_editor_matches_active_document(&self) -> bool {
        self.writer_inline_editor.as_ref().is_some_and(|session| {
            self.state.active_document_id() == Some(session.document_id)
                && self.state.writer_inline_session.as_ref() == Some(session)
        })
    }

    pub(crate) fn position_writer_inline_editor(&mut self, cx: &mut Cx, bounds: ScreenRect) {
        let width = bounds.size.width.max(160.0);
        let height = bounds.size.height.max(30.0);
        if let Some(mut host) = self
            .ui
            .view(cx, ids!(writer_inline_editor_host))
            .borrow_mut()
        {
            host.walk.abs_pos = Some(dvec2(bounds.origin.x, bounds.origin.y));
            host.walk.width = Size::Fixed(width);
            host.walk.height = Size::Fixed(height);
        }
    }

    pub(crate) fn begin_writer_inline_editor(
        &mut self,
        cx: &mut Cx,
        interaction_id: &str,
        bounds: ScreenRect,
        cursor_byte_offset: usize,
    ) -> Result<(), String> {
        let block_id = writer_block_id_from_paragraph_interaction_id(interaction_id)
            .ok_or_else(|| "Canvas Writer передал неизвестный абзац.".to_owned())?;
        let document_id = self
            .state
            .active_document_id()
            .ok_or_else(|| "Активный документ Writer не найден.".to_owned())?;

        if self.writer_inline_editor_matches_active_document()
            && self
                .writer_inline_editor
                .as_ref()
                .is_some_and(|session| session.block_id == block_id)
        {
            self.position_writer_inline_editor(cx, bounds);
            if let Some(mut input) = self
                .ui
                .widget(cx, ids!(writer_inline_text_input))
                .borrow_mut::<TextInput>()
            {
                input.set_cursor(
                    cx,
                    MakepadTextCursor {
                        index: cursor_byte_offset,
                        prefer_next_row: false,
                    },
                    false,
                );
            }
            self.writer_inline_editor_pending_focus = true;
            cx.redraw_all();
            return Ok(());
        }

        if self.writer_inline_editor.is_some() {
            self.commit_writer_inline_editor(cx)?;
        }

        let dropdown_index = self
            .state
            .writer_text_editor_blocks()
            .iter()
            .position(|block| block.block_id == block_id)
            .map(|index| index + 1)
            .ok_or_else(|| "Выбранный абзац Writer больше не существует.".to_owned())?;
        self.state.select_writer_text_editor_block(dropdown_index)?;
        self.ui.text_input(cx, ids!(writer_text_input)).set_cursor(
            cx,
            MakepadTextCursor {
                index: 0,
                prefer_next_row: false,
            },
            false,
        );
        let text = self
            .state
            .writer_text_editor_model()
            .map(|model| model.text)
            .ok_or_else(|| "Текст выбранного абзаца Writer недоступен.".to_owned())?;
        let session = self
            .state
            .begin_writer_inline_session(document_id, block_id)?;

        self.position_writer_inline_editor(cx, bounds);
        self.ui
            .view(cx, ids!(writer_inline_editor_host))
            .set_visible(cx, true);
        if let Some(mut input) = self
            .ui
            .widget(cx, ids!(writer_inline_text_input))
            .borrow_mut::<TextInput>()
        {
            input.set_text(cx, &text);
            input.set_cursor(
                cx,
                MakepadTextCursor {
                    index: cursor_byte_offset.min(text.len()),
                    prefer_next_row: false,
                },
                false,
            );
        }
        self.writer_inline_editor = Some(session);
        self.writer_inline_editor_pending_focus = true;
        self.state.status = "Writer: включено редактирование текста на странице.".to_owned();
        self.state.detail =
            "Поставьте каретку, выделите фрагмент или вводите текст прямо в рамке на листе. Ctrl+Enter применяет изменение; Esc отменяет черновик."
                .to_owned();
        cx.redraw_all();
        Ok(())
    }

    pub(crate) fn commit_writer_inline_editor(&mut self, cx: &mut Cx) -> Result<(), String> {
        let Some(session) = self.writer_inline_editor.clone() else {
            return Ok(());
        };
        if self.state.active_document_id() != Some(session.document_id) {
            return Err("Вернитесь к документу с черновиком Writer.".to_owned());
        }
        let text = self
            .ui
            .text_input(cx, ids!(writer_inline_text_input))
            .text();
        self.commit_writer_inline_editor_with_text(cx, &session, text)
    }

    pub(crate) fn commit_writer_inline_editor_with_text(
        &mut self,
        cx: &mut Cx,
        session: &WriterInlineEditorSession,
        text: String,
    ) -> Result<(), String> {
        if self.writer_inline_editor.as_ref() != Some(session) {
            return Err("Сессия редактирования Writer устарела.".to_owned());
        }
        self.state
            .update_writer_inline_draft(session, text.clone())?;
        self.state.commit_writer_inline_draft(session, text)?;
        self.writer_inline_editor = None;
        self.writer_inline_editor_pending_focus = false;
        self.ui
            .view(cx, ids!(writer_inline_editor_host))
            .set_visible(cx, false);
        Ok(())
    }

    pub(crate) fn cancel_writer_inline_editor_for_token(
        &mut self,
        cx: &mut Cx,
        session: &WriterInlineEditorSession,
    ) -> Result<(), String> {
        if self.writer_inline_editor.as_ref() != Some(session) {
            return Err("Сессия редактирования Writer устарела.".to_owned());
        }
        self.state.cancel_writer_inline_session(session)?;
        self.writer_inline_editor = None;
        self.writer_inline_editor_pending_focus = false;
        self.ui
            .view(cx, ids!(writer_inline_editor_host))
            .set_visible(cx, false);
        self.state.status = "Черновик редактирования на странице отменён.".to_owned();
        self.state.detail = "Текст документа не изменён.".to_owned();
        Ok(())
    }

    pub(crate) fn apply_writer_command_from_ui(
        &mut self,
        cx: &mut Cx,
        command: WriterHomeCommand,
    ) -> Result<(), String> {
        let character_command = matches!(
            command,
            WriterHomeCommand::SetFontFamily(_)
                | WriterHomeCommand::SetFontSizePoints(_)
                | WriterHomeCommand::ToggleBold
                | WriterHomeCommand::ToggleItalic
                | WriterHomeCommand::ToggleUnderline
                | WriterHomeCommand::ToggleStrikethrough
                | WriterHomeCommand::SetForeground(_)
                | WriterHomeCommand::SetBackground(_)
        );
        if let Some(session) = self.writer_inline_editor.clone() {
            let input = self.ui.text_input(cx, ids!(writer_inline_text_input));
            let text = input.text();
            let selection = input.selection();
            let start_byte = selection.start().index;
            let end_byte = selection.end().index;
            if character_command && start_byte == end_byte {
                return Err(
                    "Выделите фрагмент текста в рамке на листе перед форматированием.".to_owned(),
                );
            }
            writer_char_range_from_byte_range(&text, start_byte, end_byte)?;
            self.commit_writer_inline_editor(cx)?;
            self.state.select_writer_block_text_range(
                session.block_id,
                &text,
                start_byte,
                end_byte,
            )?;
        } else if character_command {
            let input = self.ui.text_input(cx, ids!(writer_text_input));
            let selection = input.selection();
            let start_byte = selection.start().index;
            let end_byte = selection.end().index;
            if start_byte != end_byte {
                let text = input.text();
                let (_, block) = self.state.writer_text_editor_selected_block()?;
                writer_char_range_from_byte_range(&text, start_byte, end_byte)?;
                if text != block.text {
                    self.state.apply_writer_text_editor(text.clone())?;
                }
                self.state.select_writer_block_text_range(
                    block.block_id,
                    &text,
                    start_byte,
                    end_byte,
                )?;
            }
        }
        self.state.apply_writer_home_command(command)
    }

    pub(crate) fn apply_writer_font_controls(&mut self, cx: &mut Cx) -> Result<(), String> {
        let font = self.ui.text_input(cx, ids!(writer_font_input)).text();
        let size = self
            .ui
            .text_input(cx, ids!(writer_size_input))
            .text()
            .trim()
            .parse::<u16>()
            .map_err(|_| "Введите целый размер шрифта от 1 до 400 pt.".to_owned())?;
        self.apply_writer_command_from_ui(cx, WriterHomeCommand::SetFontFamily(font))?;
        self.apply_writer_command_from_ui(cx, WriterHomeCommand::SetFontSizePoints(size))
    }

    pub(crate) fn reset_writer_runtime_view(&mut self, cx: &mut Cx) {
        if let Some(mut canvas) = self
            .ui
            .widget(cx, ids!(writer_page_canvas))
            .borrow_mut::<VirtualCanvas>()
        {
            canvas.replace_frame_and_reset_view(cx, None);
        }
        let page_count = match self.state.document() {
            Some(OpenDocument::Writer(vm)) => vm.state().pages.len(),
            _ => 0,
        };
        self.ui
            .label(cx, ids!(writer_page_label))
            .set_text(cx, &writer_visible_pages_label(None, page_count));
    }

    pub(crate) fn sync_writer_runtime_view(&mut self, cx: &mut Cx) {
        let Some(id) = self.state.active_document_id() else {
            return;
        };
        let canvas = self.ui.widget(cx, ids!(writer_page_canvas));
        let Some(snapshot) = canvas
            .borrow::<VirtualCanvas>()
            .map(|canvas| canvas.snapshot())
        else {
            return;
        };
        let _ = self.state.set_writer_viewport(
            id,
            f64::from(snapshot.zoom_percent) / 100.0,
            snapshot.scroll_x,
            snapshot.scroll_y,
        );
    }

    pub(crate) fn prepare_writer_zoom_popup(&mut self, cx: &mut Cx, event: &Event) {
        if !self.state.should_show_writer_view() {
            return;
        }
        let Event::MouseDown(mouse) = event else {
            return;
        };
        if mouse.button != MouseButton::PRIMARY {
            return;
        }
        let dropdown = self.ui.widget(cx, ids!(writer_zoom_dropdown));
        if dropdown.area().rect(cx).contains(mouse.abs) {
            // The footer needs the last item as the popup anchor so all presets open upward.
            // Keep the selected preset between keyboard events so arrows can traverse the list.
            self.ui
                .drop_down(cx, ids!(writer_zoom_dropdown))
                .set_selected_item(cx, WRITER_ZOOM_PRESET_PROMPT_INDEX);
        }
    }

    pub(crate) fn restore_writer_zoom_selection_after_dismissal(
        &mut self,
        cx: &mut Cx,
        event: &Event,
    ) {
        if !self.state.should_show_writer_view() {
            return;
        }
        let dropdown = self.ui.widget(cx, ids!(writer_zoom_dropdown));
        let should_restore = match event {
            Event::MouseUp(mouse) => !dropdown.area().rect(cx).contains(mouse.abs),
            Event::KeyFocus(_) => !cx.has_key_focus(dropdown.area()),
            _ => false,
        };
        if !should_restore {
            return;
        }
        let canvas = self.ui.widget(cx, ids!(writer_page_canvas));
        if let Some(percent) = canvas
            .borrow::<VirtualCanvas>()
            .map(|canvas| canvas.zoom_percent())
        {
            self.ui
                .drop_down(cx, ids!(writer_zoom_dropdown))
                .set_selected_item(cx, writer_zoom_dropdown_index_for_percent(percent));
        }
    }

    pub(crate) fn refresh_writer_status_after_draw(&mut self, cx: &mut Cx) {
        if !self.state.should_show_writer_view() {
            return;
        }
        let Some(OpenDocument::Writer(vm)) = self.state.document() else {
            return;
        };
        let page_count = vm.state().pages.len();
        let canvas = self.ui.widget(cx, ids!(writer_page_canvas));
        let Some(snapshot) = canvas
            .borrow::<VirtualCanvas>()
            .map(|canvas| canvas.snapshot())
        else {
            return;
        };
        let range = if snapshot.visible && snapshot.frame.draw_called {
            snapshot.frame.visible_range.as_ref()
        } else {
            None
        };
        self.ui
            .label(cx, ids!(writer_page_label))
            .set_text(cx, &writer_visible_pages_label(range, page_count));
        self.ui
            .label(cx, ids!(writer_zoom_label))
            .set_text(cx, &format!("{}%", snapshot.zoom_percent));
    }

    pub(crate) fn set_writer_canvas_zoom_percent(
        &mut self,
        cx: &mut Cx,
        percent: u16,
    ) -> Result<(), String> {
        if !self.state.should_show_writer_view() {
            return Err("Активный документ Writer не найден.".to_owned());
        }
        let canvas_widget = self.ui.widget(cx, ids!(writer_page_canvas));
        let Some(mut canvas) = canvas_widget.borrow_mut::<VirtualCanvas>() else {
            return Err("Canvas Writer недоступен.".to_owned());
        };
        canvas.set_zoom(cx, f64::from(percent) / 100.0);
        drop(canvas);
        self.sync_writer_runtime_view(cx);
        self.state.status = format!("Writer: масштаб {percent}%.");
        self.state.detail = "Масштаб хранится в состоянии открытой вкладки Writer.".to_owned();
        Ok(())
    }

    pub(crate) fn restore_writer_runtime_view(&mut self, cx: &mut Cx) {
        let Some(session) = self.state.active_session() else {
            return;
        };
        let DocumentViewState::Writer(view) = *session.view_state() else {
            return;
        };
        if let Some(mut canvas) = self
            .ui
            .widget(cx, ids!(writer_page_canvas))
            .borrow_mut::<VirtualCanvas>()
        {
            canvas.set_zoom(cx, view.zoom);
            canvas.scroll_to(cx, view.scroll_x, view.scroll_y);
        }
    }
}
