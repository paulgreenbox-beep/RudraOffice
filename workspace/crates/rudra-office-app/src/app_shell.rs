//! Application-owned module extracted without changing behavior.
use super::*;

impl App {
    pub(crate) fn position_file_menu(&mut self, cx: &mut Cx) {
        let trigger = self.ui.button(cx, ids!(menu_file_button)).area().rect(cx);
        if trigger.size.x <= 0.0 || trigger.size.y <= 0.0 {
            return;
        }
        if let Some(mut popup) = self.ui.view(cx, ids!(file_menu_popup)).borrow_mut() {
            popup.walk.abs_pos = Some(dvec2(trigger.pos.x, trigger.pos.y + trigger.size.y + 2.0));
        }
    }

    pub(crate) fn set_file_menu_open(&mut self, cx: &mut Cx, open: bool) {
        self.state.file_menu_open = open;
        self.ui
            .view(cx, ids!(file_menu_popup))
            .set_visible(cx, open);
        if open {
            self.position_file_menu(cx);
        }
        cx.redraw_all();
    }

    pub(crate) fn toggle_file_menu(&mut self, cx: &mut Cx) {
        let open = !self.state.file_menu_open;
        if open {
            self.state.home_ribbon_open = false;
        }
        self.set_file_menu_open(cx, open);
        self.refresh_ui(cx);
    }

    pub(crate) fn show_home_ribbon(&mut self, cx: &mut Cx) {
        self.show_ribbon_tab(cx, RibbonTab::Home);
    }

    pub(crate) fn show_ribbon_tab(&mut self, cx: &mut Cx, tab: RibbonTab) {
        if !tabs_for_format(self.state.active_module()).contains(&tab) {
            return;
        }
        self.set_file_menu_open(cx, false);
        self.state.ribbon_tab = tab;
        self.state.ribbon_command_feedback = None;
        self.state.home_ribbon_open = self.state.active_document_id().is_some();
        self.refresh_ui(cx);
    }

    pub(crate) fn finish_home_command(&mut self, cx: &mut Cx, result: Result<(), String>) {
        let succeeded = result.is_ok();
        if let Err(error) = result {
            self.state.status = "Команда не выполнена.".to_owned();
            self.state.detail = error;
        }
        self.refresh_ui(cx);
        // A toolbar click must hand keyboard ownership back to the document.
        // Otherwise Makepad keeps the last TextInput focused and Ctrl+Z undoes
        // its old draft instead of the formatting command just applied.
        if succeeded && self.state.should_show_writer_view() && self.writer_inline_editor.is_none()
        {
            let page = self.ui.widget(cx, ids!(writer_page_canvas)).area();
            if !page.is_empty() {
                cx.set_key_focus(page);
            }
        }
        if succeeded && self.state.should_show_sheets_view() {
            let grid = self.ui.widget(cx, ids!(sheets_grid_canvas)).area();
            if grid.is_valid(cx) {
                cx.set_key_focus(grid);
            }
        }
    }

    pub(crate) fn dispatch_ribbon_command(&mut self, cx: &mut Cx, command: RibbonCommand) {
        let result = if self.state.should_show_writer_view() {
            self.commit_writer_inline_editor(cx)
                .and_then(|_| self.state.apply_ribbon_command(command))
        } else {
            self.state.apply_ribbon_command(command)
        };
        if result.is_ok()
            && matches!(
                command,
                RibbonCommand::InsertRows
                    | RibbonCommand::DeleteRows
                    | RibbonCommand::InsertColumns
                    | RibbonCommand::DeleteColumns
            )
            && let Some(mut grid) = self
                .ui
                .widget(cx, ids!(sheets_grid_canvas))
                .borrow_mut::<SheetsGridCanvas>()
        {
            // State already ended the pure selection gesture. End the native
            // canvas lifecycle too before its shifted frame becomes visible.
            grid.cancel_selection(cx);
        }
        self.state.ribbon_command_feedback = Some(match &result {
            Ok(()) => self.state.status.clone(),
            Err(error) => format!(
                "Не выполнено: {}",
                error.chars().take(220).collect::<String>()
            ),
        });
        self.finish_home_command(cx, result);
        // The reusable canvas remains the sole runtime zoom/scroll owner.
        if self.state.should_show_writer_view() {
            self.restore_writer_runtime_view(cx);
        }
    }

    pub(crate) fn dispatch_active_history_command(
        &mut self,
        cx: &mut Cx,
        command: OfficeHistoryCommand,
    ) {
        let result = self
            .state
            .active_document_id()
            .ok_or_else(|| "Активный документ не найден.".to_owned())
            .and_then(|id| self.state.dispatch_history_command(id, command));
        self.finish_home_command(cx, result);
    }

    pub(crate) fn text_input_has_key_focus(&self, cx: &mut Cx) -> bool {
        [
            ids!(writer_text_input),
            ids!(writer_inline_text_input),
            ids!(writer_font_input),
            ids!(writer_size_input),
            ids!(sheets_cell_input),
            ids!(sheets_address_input),
            ids!(sheets_search_input),
            ids!(sheets_key_column_input),
            ids!(sheets_filter_value_input),
            ids!(pdf_page_input),
        ]
        .into_iter()
        .any(|id| {
            let area = self.ui.text_input(cx, id).area();
            !area.is_empty() && cx.has_key_focus(area)
        })
    }

    pub(crate) fn dismiss_file_menu_on_outside_pointer(&mut self, cx: &mut Cx, event: &Event) {
        if !self.state.file_menu_open {
            return;
        }
        let Event::MouseDown(mouse) = event else {
            return;
        };
        if mouse.button != MouseButton::PRIMARY {
            return;
        }
        let trigger = self.ui.button(cx, ids!(menu_file_button)).area().rect(cx);
        let popup = self.ui.view(cx, ids!(file_menu_popup)).area().rect(cx);
        if !trigger.contains(mouse.abs) && !popup.contains(mouse.abs) {
            self.set_file_menu_open(cx, false);
        }
    }

    pub(crate) fn activate_document(
        &mut self,
        cx: &mut Cx,
        id: RuntimeDocumentId,
    ) -> Result<(), DocumentRuntimeError> {
        let previous = self.state.active_document_id();
        if let Some(mut grid) = self
            .ui
            .widget(cx, ids!(sheets_grid_canvas))
            .borrow_mut::<SheetsGridCanvas>()
        {
            grid.cancel_selection(cx);
        }
        self.sync_writer_runtime_view(cx);
        if let Err(error) = self.state.activate_document(id) {
            let restored = previous
                .map(|previous_id| self.state.activate_document(previous_id).is_ok())
                .unwrap_or(false);
            if !restored {
                self.state.active_document = None;
                self.state.idle_module = Module::Home;
                self.state.clear_sheets_document();
                self.state.clear_pdf_document();
                self.state.clear_slides_document();
            }
            self.reset_writer_runtime_view(cx);
            self.refresh_ui(cx);
            self.restore_writer_runtime_view(cx);
            return Err(error);
        }
        self.reset_writer_runtime_view(cx);
        self.refresh_ui(cx);
        self.restore_writer_runtime_view(cx);
        Ok(())
    }

    pub(crate) fn close_document(
        &mut self,
        cx: &mut Cx,
        id: RuntimeDocumentId,
    ) -> Result<(), DocumentRuntimeError> {
        let was_active = self.state.active_document_id() == Some(id);
        let next_active = if was_active {
            let ordered_ids = self.state.ordered_document_ids();
            active_after_close(&ordered_ids, self.state.active_document_id(), id)
        } else {
            None
        };
        if was_active {
            self.sync_writer_runtime_view(cx);
        }
        self.state.close_document(id)?;
        if was_active {
            self.reset_writer_runtime_view(cx);
            if let Some(next_id) = next_active
                && let Err(error) = self.activate_document(cx, next_id)
            {
                self.state.status = "Document tab activation failed.".to_owned();
                self.state.detail = error.to_string();
            }
            self.refresh_ui(cx);
        }
        if !was_active {
            self.refresh_ui(cx);
        }
        Ok(())
    }

    pub(crate) fn handle_custom_chrome_pointer(&mut self, cx: &mut Cx, event: &Event) -> bool {
        let Event::WindowDragQuery(query) = event else {
            return false;
        };
        let Some(window_id) = self.ui.window(cx, ids!(main_window)).window_id() else {
            return false;
        };
        if query.window_id != window_id {
            return false;
        }

        let header = self.ui.view(cx, ids!(header_tabs_bar)).area().rect(cx);
        if header.size.x <= 0.0 || header.size.y <= 0.0 || !header.contains(query.abs) {
            return false;
        }
        let controls = [
            self.ui.button(cx, ids!(chrome_minimize)).area().rect(cx),
            self.ui.button(cx, ids!(chrome_maximize)).area().rect(cx),
            self.ui.button(cx, ids!(chrome_close)).area().rect(cx),
        ];
        // Only the actual tab rectangles are client UI. The tab host fills
        // the rest of the header, which must remain draggable as a caption.
        let mut tab_bounds = self
            .document_tab_accessibility_frames(cx)
            .into_iter()
            .map(|tab| tab.bounds)
            .collect::<Vec<_>>();
        if !self.state.ordered_document_ids().is_empty() && tab_bounds.is_empty() {
            // Before the first tab draw, avoid making an undrawn tab clickable
            // as a caption by falling back to the complete host rectangle.
            tab_bounds.push(self.ui.view(cx, ids!(document_tabs_host)).area().rect(cx));
        }
        query.response.set(
            if chrome_drag_target(header, &controls, &tab_bounds, query.abs) {
                WindowDragQueryResponse::Caption
            } else {
                WindowDragQueryResponse::Client
            },
        );
        cx.set_cursor(MouseCursor::Default);
        true
    }

    pub(crate) fn cycle_document_tab(&mut self, cx: &mut Cx, reverse: bool) {
        let ordered_ids = self.state.ordered_document_ids();
        let Some(next_id) = cycle_id(&ordered_ids, self.state.active_document_id(), reverse) else {
            return;
        };
        if Some(next_id) == self.state.active_document_id() {
            return;
        }
        if let Err(error) = self.activate_document(cx, next_id) {
            self.state.status = "Document tab activation failed.".to_owned();
            self.state.detail = error.to_string();
            self.refresh_ui(cx);
        }
    }

    pub(crate) fn reveal_active_document_tab_after_resize(&mut self, cx: &mut Cx) {
        if let Some(mut tab_strip) = self
            .ui
            .widget(cx, ids!(document_tabs))
            .borrow_mut::<DocumentTabStrip>()
        {
            tab_strip.reveal_active_after_resize(cx);
        }
    }

    pub(crate) fn refresh_ui(&mut self, cx: &mut Cx) {
        self.ui
            .view(cx, ids!(file_menu_popup))
            .set_visible(cx, self.state.file_menu_open);
        if self.state.file_menu_open {
            self.position_file_menu(cx);
        }
        let tabs = self.state.document_tab_frames();
        let has_document_tabs = !tabs.is_empty();
        if !tabs_for_format(self.state.active_module()).contains(&self.state.ribbon_tab) {
            self.state.ribbon_tab = RibbonTab::Home;
        }
        let show_ribbon = has_document_tabs && self.state.home_ribbon_open;
        let show_home_ribbon = show_ribbon && self.state.ribbon_tab == RibbonTab::Home;
        if let Some(mut tab_strip) = self
            .ui
            .widget(cx, ids!(document_tabs))
            .borrow_mut::<DocumentTabStrip>()
        {
            tab_strip.set_tabs(cx, tabs);
        }
        self.ui
            .view(cx, ids!(document_tabs_host))
            .set_visible(cx, has_document_tabs);
        self.ui
            .view(cx, ids!(empty_chrome_drag))
            .set_visible(cx, !has_document_tabs);
        self.ui
            .button(cx, ids!(menu_home_button))
            .set_enabled(cx, has_document_tabs);
        for (tab, widget_id) in ribbon_tab_widgets() {
            let available = tabs_for_format(self.state.active_module()).contains(&tab);
            self.ui.widget(cx, widget_id).set_visible(cx, available);
            self.ui
                .button(cx, widget_id)
                .set_enabled(cx, has_document_tabs && available);
            let active = show_ribbon && self.state.ribbon_tab == tab;
            let mut button = self.ui.widget(cx, widget_id);
            let foreground = Vec4f::from_u32(if active { 0x205facff } else { 0x3f4752ff });
            let background = Vec4f::from_u32(if active { 0xe8f0fbff } else { 0xffffffff });
            script_apply_eval!(cx, button, {
                draw_text.color: #(foreground)
                draw_bg +: { color: #(background) }
            });
        }
        let extra_ribbon = show_ribbon && self.state.ribbon_tab != RibbonTab::Home;
        self.ui
            .view(cx, ids!(extra_ribbon))
            .set_visible(cx, extra_ribbon);
        self.ui
            .label(cx, ids!(extra_ribbon_title))
            .set_text(cx, self.state.ribbon_tab.label());
        let sheets_data = extra_ribbon
            && self.state.active_module() == Module::Sheets
            && self.state.ribbon_tab == RibbonTab::Data;
        self.ui
            .view(cx, ids!(sheets_data_inputs))
            .set_visible(cx, sheets_data);
        self.ui
            .view(cx, ids!(sheets_filter_inputs))
            .set_visible(cx, sheets_data);
        if let Some(mut ribbon) = self.ui.view(cx, ids!(extra_ribbon)).borrow_mut() {
            ribbon.walk.height = Size::Fixed(if sheets_data { 164.0 } else { 96.0 });
        }
        if sheets_data {
            let enabled = self.state.sheets_tools_available();
            let draft = self.state.sheets_tools_draft();
            for (widget_id, value) in [
                (ids!(sheets_address_input), draft.address),
                (ids!(sheets_search_input), draft.query),
                (ids!(sheets_key_column_input), draft.key_column),
                (ids!(sheets_filter_value_input), draft.filter_value),
            ] {
                let widget = self.ui.text_input(cx, widget_id);
                widget.set_disabled(cx, !enabled);
                if widget.text() != value {
                    widget.set_text(cx, &value);
                }
            }
            self.ui
                .drop_down(cx, ids!(sheets_header_dropdown))
                .set_selected_item(cx, usize::from(draft.has_header));
            self.ui
                .drop_down(cx, ids!(sheets_filter_kind_dropdown))
                .set_selected_item(cx, draft.filter_kind);
            self.ui
                .widget(cx, ids!(sheets_header_dropdown))
                .set_disabled(cx, !enabled);
            self.ui
                .widget(cx, ids!(sheets_filter_kind_dropdown))
                .set_disabled(cx, !enabled);
            let selection = self
                .state
                .sheets_view
                .as_ref()
                .and_then(|view| view.selection().range())
                .map(|range| {
                    format!(
                        "{}{}:{}{}",
                        column_label(range.start.column),
                        range.start.row.0 + 1,
                        column_label(range.end.column),
                        range.end.row.0 + 1
                    )
                })
                .unwrap_or_else(|| "нет".into());
            self.ui.label(cx, ids!(extra_ribbon_title)).set_text(
                cx,
                &if enabled {
                    format!("Диапазон: {selection} • Столбец и заголовок задаются явно • Сортировка меняет строки; фильтр скрывает их")
                } else {
                    "Большая книга: поиск и изменения диапазона пока недоступны".into()
                },
            );
        }
        if let Some(feedback) = &self.state.ribbon_command_feedback {
            self.ui
                .label(cx, ids!(extra_ribbon_title))
                .set_text(cx, feedback);
        }
        let commands = commands_for(self.state.active_module(), self.state.ribbon_tab);
        for (index, widget_id) in ribbon_command_widgets().into_iter().enumerate() {
            self.ui
                .widget(cx, widget_id)
                .set_visible(cx, extra_ribbon && index < commands.len());
            if let Some(&(command, label)) = commands.get(index) {
                self.ui.button(cx, widget_id).set_text(cx, label);
                self.ui
                    .button(cx, widget_id)
                    .set_enabled(cx, self.state.ribbon_command_available(command));
            }
        }
        self.ui
            .view(cx, ids!(home_ribbon))
            .set_visible(cx, show_home_ribbon);
        let writer_home_visible = show_home_ribbon && self.state.should_show_writer_view();
        let sheets_home_visible = show_home_ribbon && self.state.should_show_sheets_view();
        let slides_home_visible = show_home_ribbon && self.state.should_show_slides_viewer();
        let pdf_home_visible = show_home_ribbon && self.state.should_show_pdf_viewer();
        self.ui
            .view(cx, ids!(writer_home_ribbon))
            .set_visible(cx, writer_home_visible);
        self.ui
            .view(cx, ids!(sheets_home_ribbon))
            .set_visible(cx, sheets_home_visible);
        self.ui
            .view(cx, ids!(slides_home_ribbon))
            .set_visible(cx, slides_home_visible);
        self.ui
            .view(cx, ids!(pdf_home_ribbon))
            .set_visible(cx, pdf_home_visible);

        let writer_character_enabled =
            writer_home_visible && self.state.writer_character_toolbar_available();
        let writer_paragraph_enabled =
            writer_home_visible && self.state.writer_paragraph_toolbar_available();
        self.ui
            .button(cx, ids!(writer_undo_button))
            .set_enabled(cx, writer_home_visible && self.state.writer_can_undo());
        self.ui
            .button(cx, ids!(writer_redo_button))
            .set_enabled(cx, writer_home_visible && self.state.writer_can_redo());
        for widget_id in [
            ids!(writer_font_apply_button),
            ids!(writer_bold_button),
            ids!(writer_italic_button),
            ids!(writer_underline_button),
            ids!(writer_strike_button),
        ] {
            self.ui
                .button(cx, widget_id)
                .set_enabled(cx, writer_character_enabled);
        }
        self.ui
            .widget(cx, ids!(writer_color_dropdown))
            .set_disabled(cx, !writer_character_enabled);
        for widget_id in [
            ids!(writer_align_left_button),
            ids!(writer_align_center_button),
            ids!(writer_align_right_button),
            ids!(writer_align_justify_button),
            ids!(writer_normal_button),
            ids!(writer_h1_button),
            ids!(writer_h2_button),
        ] {
            self.ui
                .button(cx, widget_id)
                .set_enabled(cx, writer_paragraph_enabled);
        }
        if writer_home_visible && let Some(OpenDocument::Writer(writer)) = self.state.document() {
            let toolbar = writer.toolbar_state();
            if let ToolbarValue::Known(Some(font)) = toolbar.font_family.value {
                self.ui
                    .text_input(cx, ids!(writer_font_input))
                    .set_text(cx, &font);
            }
            if let ToolbarValue::Known(size) = toolbar.size_half_points.value {
                self.ui
                    .text_input(cx, ids!(writer_size_input))
                    .set_text(cx, &(size / 2).max(1).to_string());
            }
        }
        let writer_text_editor = self.state.writer_text_editor_model();
        let writer_text_editor_enabled = writer_home_visible
            && writer_text_editor
                .as_ref()
                .is_some_and(|model| model.enabled);
        self.ui
            .button(cx, ids!(writer_text_apply_button))
            .set_enabled(cx, writer_text_editor_enabled);
        self.ui
            .button(cx, ids!(writer_text_revert_button))
            .set_enabled(cx, writer_text_editor_enabled);
        self.ui
            .widget(cx, ids!(writer_text_block_dropdown))
            .set_disabled(cx, !writer_text_editor_enabled);
        self.ui
            .widget(cx, ids!(writer_text_input))
            .set_disabled(cx, !writer_text_editor_enabled);
        if let Some(model) = writer_text_editor {
            let labels = model.labels;
            let selected_index = model.selected_index;
            let text = model.text;
            let dropdown = self.ui.drop_down(cx, ids!(writer_text_block_dropdown));
            dropdown.set_labels(cx, labels);
            dropdown.set_selected_item(cx, selected_index);
            self.ui
                .text_input(cx, ids!(writer_text_input))
                .set_text(cx, &text);
        } else {
            let dropdown = self.ui.drop_down(cx, ids!(writer_text_block_dropdown));
            dropdown.set_labels(cx, vec!["Нет текстовых абзацев".to_owned()]);
            dropdown.set_selected_item(cx, WRITER_TEXT_EDITOR_PROMPT_INDEX);
            self.ui
                .text_input(cx, ids!(writer_text_input))
                .set_text(cx, "");
        }
        self.ui
            .button(cx, ids!(writer_edit_text_button))
            .set_enabled(cx, self.state.should_show_writer_view());
        self.ui
            .widget(cx, ids!(writer_zoom_dropdown))
            .set_disabled(cx, !self.state.should_show_writer_view());

        let sheets_editing_enabled = sheets_home_visible && self.state.sheets_editing_available();
        self.state
            .sheets_clipboard
            .observe_sequence(windows_clipboard::current_sequence());
        self.ui.button(cx, ids!(sheets_copy_button)).set_enabled(
            cx,
            sheets_home_visible && self.state.sheets_copy_available(),
        );
        self.ui.button(cx, ids!(sheets_cut_button)).set_enabled(
            cx,
            sheets_editing_enabled && self.state.sheets_copy_available(),
        );
        self.ui.button(cx, ids!(sheets_paste_button)).set_enabled(
            cx,
            sheets_editing_enabled
                && self.state.sheets_clipboard_range().is_ok()
                && windows_clipboard::has_text(),
        );
        let pending = self
            .state
            .sheets_clipboard
            .pending()
            .map(|claim| {
                format!(
                    "Cut: {}{}:{}{}",
                    column_label(claim.range.start.column),
                    claim.range.start.row.0 + 1,
                    column_label(claim.range.end.column),
                    claim.range.end.row.0 + 1
                )
            })
            .unwrap_or_else(|| "—".into());
        self.ui
            .label(cx, ids!(sheets_cut_status))
            .set_text(cx, &pending);
        let cut_range = self
            .state
            .sheets_clipboard
            .pending()
            .filter(|claim| Some(claim.source) == self.state.active_document_id())
            .map(|claim| claim.range.clone());
        if let Some(mut grid) = self
            .ui
            .widget(cx, ids!(sheets_grid_canvas))
            .borrow_mut::<SheetsGridCanvas>()
        {
            grid.set_pending_cut(cx, cut_range);
        }
        self.ui
            .button(cx, ids!(sheets_undo_button))
            .set_enabled(cx, sheets_home_visible && self.state.sheets_can_undo());
        self.ui
            .button(cx, ids!(sheets_redo_button))
            .set_enabled(cx, sheets_home_visible && self.state.sheets_can_redo());
        for widget_id in [
            ids!(sheets_cell_apply_button),
            ids!(sheets_cell_clear_button),
            ids!(sheets_bold_button),
            ids!(sheets_italic_button),
            ids!(sheets_underline_button),
            ids!(sheets_align_left_button),
            ids!(sheets_align_center_button),
            ids!(sheets_align_right_button),
            ids!(sheets_wrap_button),
        ] {
            self.ui
                .button(cx, widget_id)
                .set_enabled(cx, sheets_editing_enabled);
        }
        self.ui
            .widget(cx, ids!(sheets_color_dropdown))
            .set_disabled(cx, !sheets_editing_enabled);
        if sheets_home_visible {
            let raw = self.state.sheets_selected_raw_text().unwrap_or_default();
            let binding = self.state.active_document_id().and_then(|id| {
                self.state
                    .sheets_view
                    .as_ref()?
                    .selection()
                    .focus
                    .map(|address| (id, address, raw))
            });
            if let Some(raw) = sheets_input_sync_value(&mut self.sheets_cell_input_binding, binding)
            {
                self.ui
                    .text_input(cx, ids!(sheets_cell_input))
                    .set_text(cx, &raw);
            }
        } else if !self.state.should_show_sheets_view() {
            self.sheets_cell_input_binding = None;
        }

        let slides_snapshot = self.state.slides_snapshot();
        self.ui
            .button(cx, ids!(slides_new_button))
            .set_enabled(cx, slides_home_visible);
        self.ui
            .button(cx, ids!(slides_duplicate_button))
            .set_enabled(cx, slides_home_visible && slides_snapshot.slide_count > 0);
        self.ui
            .button(cx, ids!(slides_delete_button))
            .set_enabled(cx, slides_home_visible && slides_snapshot.slide_count > 1);
        self.ui
            .widget(cx, ids!(slides_text_color_dropdown))
            .set_disabled(cx, !self.state.slides_text_color_available());
        self.ui
            .view(cx, ids!(sidebar))
            .set_visible(cx, !has_document_tabs);
        let workspace_header_hidden = self.state.should_show_writer_view()
            || self.state.should_show_pdf_viewer()
            || self.state.should_show_sheets_view()
            || self.state.should_show_slides_viewer();
        self.ui
            .label(cx, ids!(module_label))
            .set_text(cx, self.state.active_module().title());
        self.ui
            .label(cx, ids!(status_label))
            .set_text(cx, &self.state.status);
        self.ui
            .label(cx, ids!(detail_label))
            .set_text(cx, &self.state.detail);
        self.ui.label(cx, ids!(module_label)).set_visible(
            cx,
            !workspace_header_hidden && self.state.active_module() != Module::Home,
        );
        self.ui.label(cx, ids!(status_label)).set_visible(
            cx,
            !workspace_header_hidden && !self.state.status.is_empty(),
        );
        self.ui.label(cx, ids!(detail_label)).set_visible(
            cx,
            !workspace_header_hidden && !self.state.detail.is_empty(),
        );
        self.ui.view(cx, ids!(home_start_view)).set_visible(
            cx,
            !workspace_header_hidden && self.state.active_module() == Module::Home,
        );

        let should_show_pdf_viewer = self.state.should_show_pdf_viewer();
        self.ui
            .view(cx, ids!(pdf_document_view))
            .set_visible(cx, should_show_pdf_viewer);

        if should_show_pdf_viewer {
            if let Some(canvas) = self
                .ui
                .widget(cx, ids!(pdf_page_canvas))
                .borrow::<PdfPageCanvas>()
            {
                let (width_points, height_points) = canvas.viewport_points(cx);
                if width_points > 1.0 && height_points > 1.0 {
                    self.state.pdf_set_viewport(width_points, height_points);
                }
            }
            let frame = self.state.pdf_frame();
            let controller = self.state.pdf_controller.state();
            let geometry = controller.geometry_for(controller.current_page);
            let page_text = if controller.page_count == 0 {
                "0 / 0".to_owned()
            } else {
                format!(
                    "{} / {} · {:.0}×{:.0} pt · {}°",
                    controller.current_page_number(),
                    controller.page_count,
                    geometry.width_points,
                    geometry.height_points,
                    geometry.rotation_degrees,
                )
            };
            let zoom_text = format!("{:.0}%", controller.scale * 100.0);
            self.ui
                .label(cx, ids!(pdf_page_label))
                .set_text(cx, &page_text);
            self.ui
                .label(cx, ids!(pdf_zoom_label))
                .set_text(cx, &zoom_text);
            self.ui
                .text_input(cx, ids!(pdf_page_input))
                .set_text(cx, &controller.current_page_number().to_string());
            self.ui
                .button(cx, ids!(pdf_first_button))
                .set_enabled(cx, controller.can_previous());
            self.ui
                .button(cx, ids!(pdf_prev_button))
                .set_enabled(cx, controller.can_previous());
            self.ui
                .button(cx, ids!(pdf_next_button))
                .set_enabled(cx, controller.can_next());
            self.ui
                .button(cx, ids!(pdf_last_button))
                .set_enabled(cx, controller.can_next());

            let loading = controller.status == PdfViewStatus::Loading;
            let failed = controller.status == PdfViewStatus::Error;
            let empty = controller.status == PdfViewStatus::Empty || controller.page_count == 0;
            self.ui
                .label(cx, ids!(pdf_loading_label))
                .set_visible(cx, loading);
            self.ui
                .label(cx, ids!(pdf_empty_label))
                .set_visible(cx, empty && !loading && !failed);
            self.ui
                .label(cx, ids!(pdf_error_label))
                .set_visible(cx, failed);
            if failed {
                let error_text = controller
                    .error
                    .clone()
                    .unwrap_or_else(|| "PDF page render error".to_owned());
                self.ui
                    .label(cx, ids!(pdf_error_label))
                    .set_text(cx, &error_text);
            }
            if let Some(mut canvas) = self
                .ui
                .widget(cx, ids!(pdf_page_canvas))
                .borrow_mut::<PdfPageCanvas>()
            {
                document_open_trace(
                    "initial_render_plan",
                    "PASS",
                    format!("format=pdf frame_pages={}", frame.pages.len()),
                );
                canvas.set_frame(cx, Some(frame));
            }
        }
        let should_show_sheets_view = self.state.should_show_sheets_view();
        self.ui
            .view(cx, ids!(sheets_document_view))
            .set_visible(cx, should_show_sheets_view);

        if should_show_sheets_view {
            let selected = self.state.sheets_selected_index();
            let tabs_text = self
                .state
                .sheets_snapshot()
                .and_then(|snapshot| selected.and_then(|index| snapshot.sheets.get(index)))
                .map(|sheet| format!("Лист: {}", sheet.name))
                .unwrap_or_else(|| "Листы: нет".to_owned());
            self.ui
                .label(cx, ids!(sheets_tabs_label))
                .set_text(cx, &tabs_text);
            self.ui
                .button(cx, ids!(sheets_prev_button))
                .set_enabled(cx, self.state.sheets_can_previous());
            self.ui
                .button(cx, ids!(sheets_next_button))
                .set_enabled(cx, self.state.sheets_can_next());
            let frame = self.state.sheets_frame();
            let sheet_summary = frame
                .as_ref()
                .map(|frame| format!("Масштаб: {}%", (frame.zoom * 100.0).round()))
                .unwrap_or_else(|| "Лист не выбран.".to_owned());
            self.ui
                .label(cx, ids!(sheets_summary_label))
                .set_text(cx, &sheet_summary);
            self.ui
                .label(cx, ids!(sheets_empty_label))
                .set_visible(cx, frame.is_none());
            if let Some(mut canvas) = self
                .ui
                .widget(cx, ids!(sheets_grid_canvas))
                .borrow_mut::<SheetsGridCanvas>()
            {
                canvas.set_frame(cx, frame);
            }
        } else if let Some(mut canvas) = self
            .ui
            .widget(cx, ids!(sheets_grid_canvas))
            .borrow_mut::<SheetsGridCanvas>()
        {
            canvas.set_frame(cx, None);
            self.ui
                .label(cx, ids!(sheets_empty_label))
                .set_visible(cx, false);
        }

        let should_show_slides_viewer = self.state.should_show_slides_viewer();
        self.ui
            .view(cx, ids!(slides_document_view))
            .set_visible(cx, should_show_slides_viewer);

        if should_show_slides_viewer {
            let (viewport_width, viewport_height) = self
                .ui
                .widget(cx, ids!(slides_visual_canvas))
                .borrow::<SlidesVisualCanvas>()
                .map(|canvas| canvas.viewport_points(cx))
                .unwrap_or((1.0, 1.0));
            let frame = self.state.slides_frame(viewport_width, viewport_height);
            let page_text = frame
                .as_ref()
                .map(|value| format!("Слайды: {}/{}", value.current_index + 1, value.slide_count))
                .unwrap_or_else(|| "Слайды: нет данных".to_owned());
            let status_text = frame
                .as_ref()
                .map(SlidesViewerFrame::status_text)
                .unwrap_or_else(|| "Презентация не содержит слайдов.".to_owned());
            self.ui
                .label(cx, ids!(slides_page_label))
                .set_text(cx, &page_text);
            self.ui
                .label(cx, ids!(slides_status_label))
                .set_text(cx, &status_text);
            self.ui
                .view(cx, ids!(slides_toolbar))
                .set_visible(cx, !self.state.slides_presentation_mode);
            let count = frame.as_ref().map(|value| value.slide_count).unwrap_or(0);
            let current = frame.as_ref().map(|value| value.current_index).unwrap_or(0);
            self.ui
                .button(cx, ids!(slides_prev_button))
                .set_enabled(cx, count > 0 && current > 0);
            self.ui
                .button(cx, ids!(slides_next_button))
                .set_enabled(cx, count > 0 && current + 1 < count);
            self.ui
                .label(cx, ids!(slides_empty_label))
                .set_visible(cx, count == 0);
            self.ui
                .label(cx, ids!(slides_error_label))
                .set_visible(cx, false);
            if let Some(mut canvas) = self
                .ui
                .widget(cx, ids!(slides_visual_canvas))
                .borrow_mut::<SlidesVisualCanvas>()
            {
                document_open_trace(
                    "initial_render_plan",
                    "PASS",
                    format!(
                        "format=pptx slides={count} frame_present={}",
                        frame.is_some()
                    ),
                );
                canvas.set_frame(cx, frame);
            }
        } else {
            self.ui.view(cx, ids!(slides_toolbar)).set_visible(cx, true);
            self.ui
                .label(cx, ids!(slides_empty_label))
                .set_visible(cx, false);
            self.ui
                .label(cx, ids!(slides_error_label))
                .set_visible(cx, false);
            if let Some(mut canvas) = self
                .ui
                .widget(cx, ids!(slides_visual_canvas))
                .borrow_mut::<SlidesVisualCanvas>()
            {
                canvas.set_frame(cx, None);
            }
        }
        let should_show_writer_view = self.state.should_show_writer_view();
        self.ui
            .view(cx, ids!(writer_document_view))
            .set_visible(cx, should_show_writer_view);

        self.ui
            .button(cx, ids!(save_button))
            .set_enabled(cx, self.state.can_save_new_document());

        if should_show_writer_view {
            let writer_canvas = self.ui.widget(cx, ids!(writer_page_canvas));
            let zoom_percent = writer_canvas
                .borrow::<VirtualCanvas>()
                .map(|canvas| canvas.zoom_percent())
                .unwrap_or(100);
            let Some(OpenDocument::Writer(vm)) = self.state.document() else {
                return;
            };
            let state = vm.state();
            let text_fallback = writer_text_fallback(vm);
            let fallback_kind = writer_fallback_kind(vm);
            let snapshot = vm.snapshot();
            let visual_frame = state.layout.as_ref().map(|layout| {
                let projection = build_writer_page_frame_for_active_block(
                    &snapshot,
                    layout,
                    vm.api().layout_settings(),
                    vm.docx_layout(),
                    WriterPageFrameViewState {
                        active_block_id: writer_selected_block_id(vm),
                        zoom_percent: 100,
                        scroll_x_points: 0.0,
                        scroll_y_points: 0.0,
                    },
                );
                writer_runtime_frame(&projection)
            });
            self.ui
                .label(cx, ids!(writer_page_setup_label))
                .set_text(cx, &writer_page_setup_label(vm));
            let projection_notice = writer_projection_notice(vm);
            self.ui
                .label(cx, ids!(writer_projection_notice_label))
                .set_visible(cx, projection_notice.is_some());
            if let Some(notice) = projection_notice {
                self.ui
                    .label(cx, ids!(writer_projection_notice_label))
                    .set_text(cx, &notice);
            }
            self.ui
                .label(cx, ids!(writer_zoom_label))
                .set_text(cx, &format!("{zoom_percent}%"));
            self.ui
                .drop_down(cx, ids!(writer_zoom_dropdown))
                .set_selected_item(cx, writer_zoom_dropdown_index_for_percent(zoom_percent));

            let show_visual = self.state.writer_display_mode == WriterDisplayMode::Visual
                && visual_frame.is_some();
            writer_canvas.set_visible(cx, show_visual);
            if let Some(mut canvas) = writer_canvas.borrow_mut::<VirtualCanvas>() {
                canvas.set_frame(cx, if show_visual { visual_frame } else { None });
            }
            self.ui
                .view(cx, ids!(writer_inline_editor_host))
                .set_visible(
                    cx,
                    show_visual && self.writer_inline_editor_matches_active_document(),
                );
            let show_text = !show_visual && matches!(fallback_kind, WriterFallback::Text);
            let show_empty = !show_visual && matches!(fallback_kind, WriterFallback::Empty);
            let show_error = !show_visual && matches!(fallback_kind, WriterFallback::Unavailable);

            self.ui
                .label(cx, ids!(writer_text_fallback_label))
                .set_visible(cx, show_text);
            if show_text && let Some(text) = text_fallback {
                self.ui
                    .label(cx, ids!(writer_text_fallback_label))
                    .set_text(cx, &text);
            }
            self.ui
                .label(cx, ids!(writer_empty_label))
                .set_visible(cx, show_empty);
            self.ui
                .label(cx, ids!(writer_error_label))
                .set_visible(cx, show_error);
            if show_error {
                self.ui
                    .label(cx, ids!(writer_error_label))
                    .set_text(cx, &writer_fallback_label(vm));
            }
        } else {
            self.ui
                .label(cx, ids!(writer_empty_label))
                .set_visible(cx, false);
            self.ui
                .label(cx, ids!(writer_error_label))
                .set_visible(cx, false);
            self.ui
                .label(cx, ids!(writer_text_fallback_label))
                .set_visible(cx, false);
            self.ui
                .label(cx, ids!(writer_projection_notice_label))
                .set_visible(cx, false);
            let writer_canvas = self.ui.widget(cx, ids!(writer_page_canvas));
            writer_canvas.set_visible(cx, false);
            self.ui
                .view(cx, ids!(writer_inline_editor_host))
                .set_visible(cx, false);
            if let Some(mut canvas) = writer_canvas.borrow_mut::<VirtualCanvas>() {
                canvas.set_frame(cx, None);
            }
        }
    }
    pub(crate) fn select_module(&mut self, cx: &mut Cx, module: Module) {
        if self.state.active_document_id().is_none() {
            self.state.idle_module = module;
            self.state.status = module.hint().to_owned();
            self.state.detail =
                "Открытие файла выполнит проверку реальным локальным движком.".to_owned();
            self.refresh_ui(cx);
            return;
        }

        if self.state.active_module() == module {
            self.state.status = module.hint().to_owned();
            self.state.detail =
                "Активный раздел определяется текущей вкладкой документа.".to_owned();
            self.refresh_ui(cx);
            return;
        }

        if let Some(id) = self.state.document_id_for_module(module) {
            if let Err(error) = self.activate_document(cx, id) {
                self.state.status = "Не удалось активировать вкладку раздела.".to_owned();
                self.state.detail = error.to_string();
            } else {
                self.state.status = format!("Активирована вкладка раздела «{}».", module.title());
                self.state.detail = format!("Runtime ID: document:{id}");
            }
        } else if module == Module::Home {
            self.state.status = "Активная вкладка документа остаётся открытой.".to_owned();
            self.state.detail = "Закройте вкладки, чтобы вернуться к пустому обзору.".to_owned();
        } else {
            self.state.status = format!("Нет открытой вкладки раздела «{}».", module.title());
            self.state.detail =
                "Откройте документ этого формата или выберите существующую вкладку.".to_owned();
        }
        self.refresh_ui(cx);
    }

    pub(crate) fn probe_engines(&mut self, cx: &mut Cx) {
        let sheets_backend = IroncalcAdapter::new().backend_name();
        let _writer = WriterApi::new();
        let _pdf = PdfOxideAdapter::default();
        let slides_capabilities = office_oxide_adapter().capabilities().len();

        self.state.status = "Все локальные движки инициализированы.".to_owned();
        self.state.detail = format!(
            "Build: {PRODUCT_BUILD_ID}; XLSX: {sheets_backend}; DOCX/PPTX: Office Oxide; PDF: PDFOxide; возможностей PPTX: {slides_capabilities}."
        );
        self.refresh_ui(cx);
    }

    pub(crate) fn finish_open_result(&mut self, cx: &mut Cx, result: DocumentOpenResult) {
        let DocumentOpenResult {
            path,
            retained_source_bytes,
            result,
        } = result;
        self.state.open_job = None;
        let pending_blank_creation = self.state.pending_blank_creation.take();
        let mut blank_registered = false;
        match result {
            Ok((document, detached_pdf, status, detail)) => {
                let initial_previews = detached_pdf
                    .as_ref()
                    .map(PdfOpenArtifact::initial_previews)
                    .unwrap_or_default();
                let preview_count = initial_previews.len();
                let document = match detached_pdf {
                    Some(artifact) => match self.state.pdf_engine.adopt_detached(artifact) {
                        Ok(info) => {
                            for preview in initial_previews {
                                self.state.pdf_controller.cache.insert(
                                    info.document_id,
                                    preview.page_id,
                                    preview.scale,
                                    preview.png,
                                );
                            }
                            document_open_trace(
                                "backend_adoption",
                                "PASS",
                                format!(
                                    "format=pdf document_id={} initial_previews={preview_count}",
                                    info.document_id
                                ),
                            );
                            document.rebind_pdf_info(info)
                        }
                        Err(error) => {
                            document_open_trace(
                                "backend_adoption",
                                "FAIL",
                                format!("format=pdf error={error}"),
                            );
                            self.state.status =
                                "Документ не открыт; текущая сессия не изменена.".to_owned();
                            self.state.detail = format!("Этап PDF session adoption: {error}");
                            if let Some((temp_path, _)) = pending_blank_creation {
                                let _ = std::fs::remove_file(temp_path);
                            }
                            self.refresh_ui(cx);
                            return;
                        }
                    },
                    None => document,
                };
                let pdf_backend_id = document.pdf_bindings().map(|bindings| bindings.0);
                document_open_trace(
                    "document_session_creation",
                    "START",
                    format!("path={}", path.display()),
                );
                let source = if matches!(&document, OpenDocument::Sheets(_)) {
                    retained_source_bytes.map_or_else(
                        || DocumentSource::from_path(path.clone()),
                        |bytes| DocumentSource::from_path_with_retained_bytes(path.clone(), bytes),
                    )
                } else {
                    DocumentSource::from_path(path.clone())
                };
                match self.state.register_document_with_source(source, document) {
                    Ok(id) => {
                        document_open_trace(
                            "document_session_creation",
                            "PASS",
                            format!("document_id={id}"),
                        );
                        document_open_trace(
                            "session_activation",
                            "START",
                            format!("document_id={id}"),
                        );
                        match self.activate_document(cx, id) {
                            Ok(()) => {
                                if let Some((temp_path, format)) = &pending_blank_creation
                                    && temp_path == &path
                                {
                                    self.state.new_document_formats.insert(id, *format);
                                    self.state
                                        .new_document_temp_paths
                                        .insert(id, temp_path.clone());
                                    blank_registered = true;
                                }
                                document_open_trace(
                                    "session_activation",
                                    "PASS",
                                    format!("document_id={id}"),
                                );
                                self.state.status = status;
                                self.state.detail = format!(
                                    "{detail} Файл: {}. Runtime ID: document:{id}",
                                    path.display()
                                );
                                if blank_registered {
                                    self.state.status =
                                        "Создан новый временный документ.".to_owned();
                                    self.state.detail = "Документ открыт во вкладке. Сохраните его через «Сохранить как»; исходный файл не затрагивается.".to_owned();
                                }
                            }
                            Err(error) => {
                                document_open_trace(
                                    "session_activation",
                                    "FAIL",
                                    format!("document_id={id} error={error}"),
                                );
                                let _ = self.close_document(cx, id);
                                self.state.status =
                                    "Документ загружен, но сессия просмотра не активирована."
                                        .to_owned();
                                self.state.detail = error.to_string();
                            }
                        }
                    }
                    Err(error) => {
                        document_open_trace(
                            "document_session_creation",
                            "FAIL",
                            format!("error={error}"),
                        );
                        if let Some(backend_id) = pdf_backend_id {
                            let _ = self.state.pdf_engine.close_document(backend_id);
                        }
                        self.state.status =
                            "Документ не открыт; текущая сессия не изменена.".to_owned();
                        self.state.detail = error.to_string();
                    }
                }
            }
            Err(error) => {
                document_open_trace("document_open_model", "FAIL", format!("error={error}"));
                self.state.status = "Документ не открыт; текущая сессия не изменена.".to_owned();
                self.state.detail = error;
            }
        }
        if !blank_registered && let Some((temp_path, _)) = pending_blank_creation {
            let _ = std::fs::remove_file(temp_path);
        }
        self.refresh_ui(cx);
    }

    pub(crate) fn poll_document_open(&mut self, cx: &mut Cx) {
        let poll = self
            .state
            .open_job
            .as_ref()
            .map(|job| job.receiver.try_recv());
        match poll {
            None => {}
            Some(Ok(result)) => self.finish_open_result(cx, result),
            // The worker posts `Event::Signal` when it has a result. Do not
            // redraw continuously while parsing a large document: that would
            // create a CPU busy-loop and prevent the UI from becoming idle.
            Some(Err(TryRecvError::Empty)) => {}
            Some(Err(TryRecvError::Disconnected)) => {
                self.state.open_job = None;
                if let Some((temp_path, _)) = self.state.pending_blank_creation.take() {
                    let _ = std::fs::remove_file(temp_path);
                }
                self.state.status = "Документ не открыт; текущая сессия не изменена.".to_owned();
                self.state.detail =
                    "Фоновая операция открытия завершилась без результата.".to_owned();
                self.refresh_ui(cx);
            }
        }
    }

    #[cfg(windows)]
    pub(crate) fn open_document_picker(&mut self, cx: &mut Cx) {
        self.set_file_menu_open(cx, false);
        match windows_file_picker::select_office_document() {
            Ok(Some(path)) => self.open_document_path(cx, path),
            Ok(None) => {}
            Err(error) => {
                self.state.status = "Не удалось открыть Проводник Windows.".to_owned();
                self.state.detail = error;
                self.refresh_ui(cx);
            }
        }
    }

    pub(crate) fn open_document(&mut self, cx: &mut Cx) {
        self.set_file_menu_open(cx, false);
        if let Some(path) = self.state.pending_open_path.take() {
            self.open_document_path(cx, path);
            return;
        }
        #[cfg(windows)]
        self.open_document_picker(cx);
        #[cfg(not(windows))]
        {
            self.state.status = "Системный выбор файла недоступен на этой платформе.".to_owned();
            self.state.detail =
                "Поддержка диалога выбора файла пока реализована для Windows.".to_owned();
            self.refresh_ui(cx);
        }
    }

    pub(crate) fn create_new_document(&mut self, cx: &mut Cx, format: BlankDocumentFormat) {
        if self.state.open_job.is_some() {
            self.state.status = "Открытие документа уже выполняется.".to_owned();
            self.state.detail = "Дождитесь завершения текущей операции.".to_owned();
            self.refresh_ui(cx);
            return;
        }
        match blank_documents::create_temporary(format) {
            Ok(path) => {
                self.state.pending_blank_creation = Some((path.clone(), format));
                self.open_document_path(cx, path);
                if self.state.open_job.is_none()
                    && let Some((temp_path, _)) = self.state.pending_blank_creation.take()
                {
                    let _ = std::fs::remove_file(temp_path);
                }
            }
            Err(error) => {
                self.state.status = "Не удалось создать новый документ.".to_owned();
                self.state.detail = error;
                self.refresh_ui(cx);
            }
        }
    }

    pub(crate) fn open_document_path(&mut self, cx: &mut Cx, path: PathBuf) {
        if self.state.open_job.is_some() {
            self.state.status = "Открытие документа уже выполняется.".to_owned();
            self.state.detail = "Дождитесь завершения текущей операции.".to_owned();
            self.refresh_ui(cx);
            return;
        }
        let admission = match admit(&path) {
            Ok(admission) => {
                document_open_trace(
                    "file_admission",
                    "PASS",
                    format!("declared_bytes={}", admission.declared_size()),
                );
                admission
            }
            Err(error) => {
                document_open_trace("file_admission", "FAIL", format!("error={error}"));
                self.state.status = "Документ не открыт; текущая сессия не изменена.".to_owned();
                self.state.detail = format!("Этап file admission: {error}");
                self.refresh_ui(cx);
                return;
            }
        };

        let (sender, receiver) = mpsc::channel();
        self.state.open_job = Some(DocumentOpenJob { receiver });
        self.state.status = "Открытие документа…".to_owned();
        self.state.detail = format!(
            "Чтение и разбор выполняются в фоне. Файл: {}",
            path.display()
        );
        self.refresh_ui(cx);
        cx.redraw_all();

        let worker_path = path.clone();
        thread::spawn(move || {
            let (result, retained_source_bytes) =
                match read_after_admission(&worker_path, admission) {
                    Ok(bytes) => {
                        document_open_trace(
                            "byte_read",
                            "PASS",
                            format!("actual_bytes={}", bytes.len()),
                        );
                        let retain_for_bounded_sheets = worker_path
                            .extension()
                            .and_then(|extension| extension.to_str())
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("xlsx"))
                            && uses_bounded_import(&bytes);
                        let result = OpenDocument::open_background(&worker_path, &bytes);
                        let retained_source_bytes =
                            retain_for_bounded_sheets.then(|| Arc::<[u8]>::from(bytes));
                        (result, retained_source_bytes)
                    }
                    Err(error) => {
                        document_open_trace("byte_read", "FAIL", format!("error={error}"));
                        (Err(format!("Этап byte read: {error}")), None)
                    }
                };
            if sender
                .send(DocumentOpenResult {
                    path: worker_path,
                    retained_source_bytes,
                    result,
                })
                .is_ok()
            {
                // Wake the Makepad event loop exactly once so `poll_document_open`
                // can adopt the result without a redraw-based polling loop.
                SignalToUI::set_ui_signal();
            }
        });
    }
    pub(crate) fn save_document(&mut self, cx: &mut Cx) {
        let Some(id) = self.state.active_document_id() else {
            self.state.status = "Нечего сохранять: сначала создайте документ.".to_owned();
            self.refresh_ui(cx);
            return;
        };
        let Some(format) = self.state.new_document_formats.get(&id).copied() else {
            self.state.status = if self.state.read_only_save_active() {
                "Сохранение недоступно для открытого файла.".to_owned()
            } else {
                "Нечего сохранять в этой вкладке.".to_owned()
            };
            self.state.detail = "Исходные файлы пока открываются без права перезаписи. Создайте новый документ на главной странице, чтобы использовать «Сохранить как».".to_owned();
            self.refresh_ui(cx);
            return;
        };
        if format == BlankDocumentFormat::Xlsx
            && let Err(error) = self.state.commit_sheets_api_snapshot()
        {
            self.state.status = "Не удалось подготовить таблицу к сохранению.".to_owned();
            self.state.detail = error;
            self.refresh_ui(cx);
            return;
        }
        let bytes = match self.state.documents.require(id) {
            Ok(session) => match format {
                BlankDocumentFormat::Pdf => session
                    .source()
                    .path()
                    .ok_or_else(|| "Не найден источник нового PDF.".to_owned())
                    .and_then(|path| std::fs::read(path).map_err(|error| error.to_string())),
                _ => session.backend().export_bytes(),
            },
            Err(error) => Err(error.to_string()),
        };
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(error) => {
                self.state.status = "Не удалось подготовить документ к сохранению.".to_owned();
                self.state.detail = error;
                self.refresh_ui(cx);
                return;
            }
        };
        let validation_name = format!("saved.{}", format.extension());
        if let Err(error) = OpenDocument::open_background(Path::new(&validation_name), &bytes) {
            self.state.status = "Документ не сохранён: проверка экспорта не прошла.".to_owned();
            self.state.detail = error;
            self.refresh_ui(cx);
            return;
        }
        #[cfg(windows)]
        let destination = match windows_file_picker::select_save_document(format.extension()) {
            Ok(Some(path)) => path,
            Ok(None) => return,
            Err(error) => {
                self.state.status = "Не удалось выбрать путь сохранения.".to_owned();
                self.state.detail = error;
                self.refresh_ui(cx);
                return;
            }
        };
        #[cfg(not(windows))]
        let destination = {
            self.state.status = "Сохранение пока доступно только в Windows.".to_owned();
            self.refresh_ui(cx);
            return;
        };
        match new_document_save::write_new_document(&destination, format.extension(), &bytes) {
            Ok(()) => {
                if let Ok(session) = self.state.documents.require_mut(id) {
                    session.replace_source_path_after_save(destination.clone());
                }
                if let Some(temp_path) = self.state.new_document_temp_paths.remove(&id) {
                    let _ = std::fs::remove_file(temp_path);
                }
                self.state.status = "Новый документ сохранён.".to_owned();
                self.state.detail = format!(
                    "Файл: {}. Вкладка осталась открытой.",
                    destination.display()
                );
            }
            Err(error) => {
                self.state.status = "Документ не сохранён.".to_owned();
                self.state.detail = error;
            }
        }
        self.refresh_ui(cx);
    }

    #[cfg(test)]
    pub(crate) fn inspect_document(
        path: &Path,
        bytes: &[u8],
    ) -> Result<(Module, String, String), String> {
        let mut pdf_engine = PdfOxideAdapter::default();
        let (document, status, detail) = OpenDocument::open(path, bytes, &mut pdf_engine)?;
        Ok((document.module(), status, detail))
    }
}
