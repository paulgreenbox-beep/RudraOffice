//! Application-owned module extracted without changing behavior.
use super::*;

impl AppState {
    pub(crate) fn apply_slides_home_command(
        &mut self,
        command: SlidesHomeCommand,
    ) -> Result<(), String> {
        if let SlidesHomeCommand::SetTextColor(color) = command {
            let result = self.apply_slides_text_color(color);
            if result.is_ok() {
                self.slides_color_value = slides_color_accessibility_value(&command);
            }
            return result;
        }
        let current = self.slides_snapshot();
        let presentation = match self.document() {
            Some(OpenDocument::Slides(presentation)) => presentation.clone(),
            _ => return Err("Активная презентация не найдена.".to_owned()),
        };
        let (editor_command, target_index, label) = match command {
            SlidesHomeCommand::Add => {
                let index = if current.slide_count == 0 {
                    0
                } else {
                    (current.current_index + 1).min(current.slide_count)
                };
                (
                    SlidesEditorCommand::AddSlide {
                        index,
                        layout: SlideLayout::Blank,
                    },
                    index,
                    "добавлен новый слайд",
                )
            }
            SlidesHomeCommand::Duplicate => {
                let slide_id = current
                    .current_slide_id
                    .ok_or_else(|| "Нет слайда для дублирования.".to_owned())?;
                let index = (current.current_index + 1).min(current.slide_count);
                (
                    SlidesEditorCommand::DuplicateSlide { slide_id, index },
                    index,
                    "слайд дублирован",
                )
            }
            SlidesHomeCommand::Delete => {
                if current.slide_count <= 1 {
                    return Err("Последний слайд удалять нельзя.".to_owned());
                }
                let slide_id = current
                    .current_slide_id
                    .ok_or_else(|| "Нет слайда для удаления.".to_owned())?;
                (
                    SlidesEditorCommand::DeleteSlide { slide_id },
                    current.current_index.min(current.slide_count - 2),
                    "слайд удалён",
                )
            }
            SlidesHomeCommand::SetTextColor(_) => unreachable!("handled before slide commands"),
        };
        let mut editor =
            PresentationEditor::new(presentation).map_err(|error| error.to_string())?;
        editor
            .apply(editor_command)
            .map_err(|error| error.to_string())?;
        let updated = editor.presentation().clone();
        let new_count = updated.slides.len();
        let target_index = target_index.min(new_count.saturating_sub(1));
        let Some(OpenDocument::Slides(document)) = self.document_mut() else {
            return Err("Активная презентация больше недоступна.".to_owned());
        };
        *document = updated.clone();
        let mut viewer =
            PresentationViewer::from_document(updated).map_err(|error| error.to_string())?;
        if new_count > 0 {
            viewer
                .select(target_index)
                .map_err(|error| error.to_string())?;
            self.slides_scale = viewer
                .set_zoom(self.slides_scale)
                .map_err(|error| error.to_string())?;
        }
        self.slides_viewer = Some(viewer);
        self.slides_current_index = target_index;
        self.sync_active_session_view();
        self.status = format!("Презентация: {label}.");
        self.detail = "Изменение выполнено в текущей сессии; запись исходного PPTX пока отключена."
            .to_owned();
        Ok(())
    }

    pub(crate) fn slides_selected_text_object(&self) -> Option<(SlideId, ObjectId)> {
        let viewer = self.slides_viewer.as_ref()?;
        let slide_id = viewer.current_slide_id()?;
        let object_id = viewer.selected_object()?;
        let OpenDocument::Slides(presentation) = self.document()? else {
            return None;
        };
        presentation
            .slide(slide_id)?
            .object(object_id)
            .filter(|object| matches!(&object.kind, ObjectKind::Text(_)))
            .map(|_| (slide_id, object_id))
    }

    pub(crate) fn slides_text_color_available(&self) -> bool {
        self.should_show_slides_viewer() && self.slides_selected_text_object().is_some()
    }

    pub(crate) fn slides_select_text_object(
        &mut self,
        object_id: Option<ObjectId>,
    ) -> Result<(), String> {
        let viewer = self
            .slides_viewer
            .as_mut()
            .ok_or_else(|| "Представление презентации не инициализировано.".to_owned())?;
        viewer
            .dispatch(SlidesViewerAction::SelectObject(object_id))
            .map_err(|error| error.to_string())?;
        self.status = match viewer.selected_object() {
            Some(_) => "Презентация: текстовый объект выбран.".to_owned(),
            None => "Презентация: текстовый объект не выбран.".to_owned(),
        };
        self.detail = "Выберите цвет текста во вкладке «Главная».".to_owned();
        Ok(())
    }

    pub(crate) fn apply_slides_text_color(&mut self, color: SlidesColor) -> Result<(), String> {
        let (slide_id, object_id) = self.slides_selected_text_object().ok_or_else(|| {
            "Сначала выберите текстовый объект щелчком на текущем слайде.".to_owned()
        })?;
        let presentation = match self.document() {
            Some(OpenDocument::Slides(presentation)) => presentation.clone(),
            _ => return Err("Активная презентация не найдена.".to_owned()),
        };
        let mut editor =
            PresentationEditor::new(presentation).map_err(|error| error.to_string())?;
        editor
            .apply(SlidesEditorCommand::SetTextColor {
                slide_id,
                object_id,
                color,
            })
            .map_err(|error| error.to_string())?;
        let updated = editor.presentation().clone();
        let current_index = self.slides_snapshot().current_index;
        let Some(OpenDocument::Slides(document)) = self.document_mut() else {
            return Err("Активная презентация больше недоступна.".to_owned());
        };
        *document = updated.clone();
        let viewer = self
            .slides_viewer
            .as_mut()
            .ok_or_else(|| "Представление презентации не инициализировано.".to_owned())?;
        viewer
            .sync_from_document(&updated)
            .map_err(|error| error.to_string())?;
        viewer
            .select(current_index)
            .map_err(|error| error.to_string())?;
        viewer
            .dispatch(SlidesViewerAction::SelectObject(Some(object_id)))
            .map_err(|error| error.to_string())?;
        self.sync_active_session_view();
        self.status = "Презентация: цвет текста применён.".to_owned();
        self.detail = "Изменение выполнено в текущей сессии; запись исходного PPTX пока отключена."
            .to_owned();
        Ok(())
    }

    pub(crate) fn slides_active(&self) -> bool {
        matches!(self.document(), Some(OpenDocument::Slides(_)))
    }

    pub(crate) fn should_show_slides_viewer(&self) -> bool {
        self.active_module() == Module::Slides && self.slides_active()
    }

    pub(crate) fn slides_current_index(&self) -> Option<usize> {
        let Some(OpenDocument::Slides(presentation)) = self.document() else {
            return None;
        };
        if presentation.slides.is_empty() {
            return None;
        }
        Some(self.slides_snapshot().current_index)
    }

    pub(crate) fn slides_snapshot(&self) -> SlidesViewSnapshot {
        let mut snapshot = SlidesViewSnapshot::default();
        let Some(OpenDocument::Slides(presentation)) = self.document() else {
            return snapshot;
        };
        snapshot.slide_count = presentation.slides.len();
        if snapshot.slide_count == 0 {
            return snapshot;
        }
        snapshot.current_index = self.slides_current_index.min(snapshot.slide_count - 1);
        snapshot.current_slide_id = presentation
            .slides
            .get(snapshot.current_index)
            .map(|slide| slide.id);
        snapshot.scale = self.slides_scale;
        snapshot
    }

    pub(crate) fn bind_slides_document(&mut self) {
        self.slides_current_index = 0;
        self.slides_scale = 1.0;
        self.slides_presentation_mode = false;
        self.slides_viewer = match self.document() {
            Some(OpenDocument::Slides(presentation)) => {
                PresentationViewer::from_document(presentation.clone()).ok()
            }
            _ => None,
        };
    }

    pub(crate) fn clear_slides_document(&mut self) {
        self.slides_current_index = 0;
        self.slides_scale = 1.0;
        self.slides_viewer = None;
        self.slides_presentation_mode = false;
    }

    pub(crate) fn slides_previous(&mut self) {
        if let Some(viewer) = self.slides_viewer.as_mut() {
            let _ = viewer.previous();
            self.slides_current_index = viewer.current_index().unwrap_or(0);
        } else if let Some(index) = self.slides_current_index()
            && index > 0
        {
            self.slides_current_index = index - 1;
        }
        self.slides_status_update();
    }

    pub(crate) fn slides_next(&mut self) {
        if let Some(viewer) = self.slides_viewer.as_mut() {
            let _ = viewer.next();
            self.slides_current_index = viewer.current_index().unwrap_or(0);
        } else if let Some(index) = self.slides_current_index() {
            let count = self.slides_snapshot().slide_count;
            if index + 1 < count {
                self.slides_current_index = index + 1;
            }
        }
        self.slides_status_update();
    }

    pub(crate) fn slides_status_update(&mut self) {
        let snapshot = self.slides_snapshot();
        if snapshot.slide_count > 0 {
            self.status = format!(
                "Презентация: слайд {}/{}",
                snapshot.current_index + 1,
                snapshot.slide_count
            );
        }
        self.detail.clear();
        self.sync_active_session_view();
    }

    pub(crate) fn slides_select(&mut self, index: usize) {
        let page_number = index.saturating_add(1).min(u32::MAX as usize) as u32;
        let _ = self.go_to_page_active(page_number);
    }

    pub(crate) fn slides_handle_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::ArrowLeft | KeyCode::ArrowUp | KeyCode::PageUp => self.slides_previous(),
            KeyCode::ArrowRight | KeyCode::ArrowDown | KeyCode::PageDown | KeyCode::Space => {
                self.slides_next()
            }
            KeyCode::Home => self.slides_select(0),
            KeyCode::End => {
                let count = self.slides_snapshot().slide_count;
                if count > 0 {
                    self.slides_select(count - 1);
                }
            }
            KeyCode::Escape => self.slides_presentation_mode = false,
            KeyCode::F11 => self.slides_presentation_mode = !self.slides_presentation_mode,
            _ => {}
        }
    }

    pub(crate) fn slides_toggle_presentation(&mut self) {
        let enabled = !self.slides_presentation_mode;
        let _ = self.set_active_view_mode(DocumentViewMode::SlidesPresentation(enabled));
        self.slides_status_update();
    }

    pub(crate) fn slides_zoom_in(&mut self) {
        let requested = f64::from(self.slides_scale + 0.25);
        if let Ok(scale) = self.zoom_active_document_to(requested) {
            self.status = format!("Масштаб слайда: {:.0}%", scale * 100.0);
            self.detail.clear();
        }
    }

    pub(crate) fn slides_zoom_out(&mut self) {
        let requested = f64::from(self.slides_scale - 0.25);
        if let Ok(scale) = self.zoom_active_document_to(requested) {
            self.status = format!("Масштаб слайда: {:.0}%", scale * 100.0);
            self.detail.clear();
        }
    }

    pub(crate) fn slides_fit(&mut self) {
        let _ = self.zoom_active_document_to(1.0);
        self.status = "Масштаб: Fit Slide".to_owned();
        self.detail = "Слайд вписан в доступную область с сохранением пропорций.".to_owned();
    }

    pub(crate) fn slides_reset(&mut self) {
        let _ = self.go_to_page_active(1);
        let _ = self.zoom_active_document_to(1.0);
        self.slides_status_update();
        if self.slides_active() {
            self.detail = "Масштаб и позиция сброшены.".to_owned();
        }
    }

    pub(crate) fn slides_frame(
        &self,
        viewport_width: f32,
        viewport_height: f32,
    ) -> Option<SlidesViewerFrame> {
        let viewer = self.slides_viewer.as_ref()?;
        Some(SlidesViewerFrame::from_viewer(
            viewer,
            viewport_width,
            viewport_height,
            self.slides_scale,
            self.slides_presentation_mode,
        ))
    }
}
