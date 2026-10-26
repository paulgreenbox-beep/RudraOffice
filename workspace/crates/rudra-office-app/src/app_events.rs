//! Application-owned module extracted without changing behavior.
use super::*;

impl MatchEvent for App {
    fn handle_startup(&mut self, cx: &mut Cx) {
        self.read_launcher_startup_request();
        self.refresh_ui(cx);
    }
    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions) {
        #[cfg(windows)]
        {
            let requested = actions
                .iter()
                .filter_map(|action| action.downcast_ref::<AccessibilityUiAction>())
                .map(|action| action.0.clone())
                .collect::<Vec<_>>();
            for request in requested {
                self.handle_accessibility_action(cx, request);
            }
        }

        let tab_strip = self.ui.widget(cx, ids!(document_tabs));
        for action in actions.filter_widget_actions(tab_strip.widget_uid()) {
            match action.cast() {
                DocumentTabsAction::Activate(id) => {
                    if let Err(error) = self.activate_document(cx, id) {
                        self.state.status = "Document tab activation failed.".to_owned();
                        self.state.detail = error.to_string();
                        self.refresh_ui(cx);
                    }
                }
                DocumentTabsAction::Close(id) => {
                    if let Err(error) = self.close_document(cx, id) {
                        self.state.status = "Document tab close failed.".to_owned();
                        self.state.detail = error.to_string();
                        self.refresh_ui(cx);
                    }
                }
                DocumentTabsAction::None => {}
            }
        }

        if self.ui.button(cx, ids!(chrome_minimize)).clicked(actions) {
            minimize_office_window(cx, &self.ui);
        }
        if self.ui.button(cx, ids!(chrome_maximize)).clicked(actions) {
            toggle_office_window_maximized(cx, &self.ui);
        }
        if self.ui.button(cx, ids!(chrome_close)).clicked(actions) {
            cx.quit();
        }

        if self.ui.button(cx, ids!(home_button)).clicked(actions) {
            self.select_module(cx, Module::Home);
        }
        if self.ui.button(cx, ids!(writer_button)).clicked(actions) {
            self.select_module(cx, Module::Writer);
        }
        if self.ui.button(cx, ids!(sheets_button)).clicked(actions) {
            self.select_module(cx, Module::Sheets);
        }
        if self.ui.button(cx, ids!(pdf_button)).clicked(actions) {
            self.select_module(cx, Module::Pdf);
        }
        if self.ui.button(cx, ids!(slides_button)).clicked(actions) {
            self.select_module(cx, Module::Slides);
        }
        if self.ui.button(cx, ids!(probe_button)).clicked(actions) {
            self.probe_engines(cx);
        }
        if self.ui.button(cx, ids!(menu_file_button)).clicked(actions) {
            self.toggle_file_menu(cx);
        }
        if self.ui.button(cx, ids!(menu_home_button)).clicked(actions) {
            self.show_home_ribbon(cx);
        }
        for (tab, widget_id) in ribbon_tab_widgets() {
            if tab != RibbonTab::Home && self.ui.button(cx, widget_id).clicked(actions) {
                self.show_ribbon_tab(cx, tab);
            }
        }
        if self.state.home_ribbon_open
            && self.state.ribbon_tab == RibbonTab::Data
            && self.state.active_module() == Module::Sheets
        {
            let address = self.ui.text_input(cx, ids!(sheets_address_input));
            let search = self.ui.text_input(cx, ids!(sheets_search_input));
            let key = self.ui.text_input(cx, ids!(sheets_key_column_input));
            let filter = self.ui.text_input(cx, ids!(sheets_filter_value_input));
            if let Some(value) = address.changed(actions) {
                self.state.update_sheets_tools_draft(Some(value), None);
            }
            if let Some(value) = search.changed(actions) {
                self.state.update_sheets_tools_draft(None, Some(value));
            }
            if let Some(value) = key.changed(actions) {
                self.state
                    .update_sheets_data_draft(Some(value), None, None, None);
            }
            if let Some(value) = filter.changed(actions) {
                self.state
                    .update_sheets_data_draft(None, None, None, Some(value));
            }
            if let Some(index) = self
                .ui
                .drop_down(cx, ids!(sheets_header_dropdown))
                .selected(actions)
            {
                self.state
                    .update_sheets_data_draft(None, Some(index == 1), None, None);
            }
            if let Some(index) = self
                .ui
                .drop_down(cx, ids!(sheets_filter_kind_dropdown))
                .selected(actions)
            {
                self.state
                    .update_sheets_data_draft(None, None, Some(index), None);
            }
            if address.returned(actions).is_some() {
                self.dispatch_ribbon_command(cx, RibbonCommand::GoToRange);
            }
            if search.returned(actions).is_some() {
                self.dispatch_ribbon_command(cx, RibbonCommand::FindNext);
            }
        }
        if self.state.home_ribbon_open && self.state.ribbon_tab != RibbonTab::Home {
            for (index, widget_id) in ribbon_command_widgets().into_iter().enumerate() {
                if self.ui.button(cx, widget_id).clicked(actions)
                    && let Some(&(command, _)) =
                        commands_for(self.state.active_module(), self.state.ribbon_tab).get(index)
                {
                    self.dispatch_ribbon_command(cx, command);
                }
            }
        }
        for request in rudra_office_launcher::view::requests(cx, &self.ui, actions) {
            self.dispatch_launcher_request(cx, request);
        }
        if self.ui.button(cx, ids!(open_button)).clicked(actions) {
            self.set_file_menu_open(cx, false);
            #[cfg(windows)]
            self.open_document_picker(cx);
            #[cfg(not(windows))]
            self.open_document(cx);
        }
        if self.ui.button(cx, ids!(save_button)).clicked(actions) {
            self.save_document(cx);
        }

        if self.state.should_show_writer_view() && self.state.home_ribbon_open {
            let writer_text_input = self.ui.text_input(cx, ids!(writer_text_input));
            if let Some(text) = writer_text_input.changed(actions) {
                self.state.update_writer_text_editor_draft(text);
            }
            let paragraph_dropdown = self.ui.drop_down(cx, ids!(writer_text_block_dropdown));
            if let Some(index) = paragraph_dropdown.selected(actions) {
                let result = self.state.select_writer_text_editor_block(index);
                self.finish_home_command(cx, result);
            }
            if self
                .ui
                .button(cx, ids!(writer_text_apply_button))
                .clicked(actions)
            {
                let raw = self.ui.text_input(cx, ids!(writer_text_input)).text();
                let result = self.state.apply_writer_text_editor(raw);
                self.finish_home_command(cx, result);
            }
            if self
                .ui
                .button(cx, ids!(writer_text_revert_button))
                .clicked(actions)
            {
                let result = match self.state.reset_writer_text_editor_draft() {
                    Ok(text) => {
                        self.ui
                            .text_input(cx, ids!(writer_text_input))
                            .set_text(cx, &text);
                        Ok(())
                    }
                    Err(error) => Err(error),
                };
                self.finish_home_command(cx, result);
            }
            if self
                .ui
                .button(cx, ids!(writer_undo_button))
                .clicked(actions)
            {
                self.dispatch_active_history_command(cx, OfficeHistoryCommand::Undo);
            }
            if self
                .ui
                .button(cx, ids!(writer_redo_button))
                .clicked(actions)
            {
                self.dispatch_active_history_command(cx, OfficeHistoryCommand::Redo);
            }
            if self
                .ui
                .button(cx, ids!(writer_font_apply_button))
                .clicked(actions)
            {
                let result = self.apply_writer_font_controls(cx);
                self.finish_home_command(cx, result);
            }
            for (widget_id, command) in [
                (ids!(writer_bold_button), WriterHomeCommand::ToggleBold),
                (ids!(writer_italic_button), WriterHomeCommand::ToggleItalic),
                (
                    ids!(writer_underline_button),
                    WriterHomeCommand::ToggleUnderline,
                ),
                (
                    ids!(writer_strike_button),
                    WriterHomeCommand::ToggleStrikethrough,
                ),
                (
                    ids!(writer_align_left_button),
                    WriterHomeCommand::SetAlignment(WriterAlignment::Left),
                ),
                (
                    ids!(writer_align_center_button),
                    WriterHomeCommand::SetAlignment(WriterAlignment::Center),
                ),
                (
                    ids!(writer_align_right_button),
                    WriterHomeCommand::SetAlignment(WriterAlignment::Right),
                ),
                (
                    ids!(writer_align_justify_button),
                    WriterHomeCommand::SetAlignment(WriterAlignment::Justified),
                ),
                (
                    ids!(writer_normal_button),
                    WriterHomeCommand::SetHeading(None),
                ),
                (
                    ids!(writer_h1_button),
                    WriterHomeCommand::SetHeading(Some(1)),
                ),
                (
                    ids!(writer_h2_button),
                    WriterHomeCommand::SetHeading(Some(2)),
                ),
            ] {
                if self.ui.button(cx, widget_id).clicked(actions) {
                    let result = self.apply_writer_command_from_ui(cx, command);
                    self.finish_home_command(cx, result);
                }
            }
            let color_dropdown = self.ui.drop_down(cx, ids!(writer_color_dropdown));
            if let Some(index) = color_dropdown.selected(actions) {
                let result = writer_color_command_from_dropdown(index)
                    .ok_or_else(|| "Выберите цвет Writer из списка.".to_owned())
                    .and_then(|command| self.apply_writer_command_from_ui(cx, command));
                color_dropdown.set_selected_item(cx, COLOR_DROPDOWN_PROMPT_INDEX);
                self.finish_home_command(cx, result);
            }
        }

        if self.state.should_show_sheets_view() && self.state.home_ribbon_open {
            if self
                .ui
                .button(cx, ids!(sheets_cell_apply_button))
                .clicked(actions)
            {
                let raw = self.ui.text_input(cx, ids!(sheets_cell_input)).text();
                let result = self.state.apply_sheets_cell_input(raw);
                self.finish_home_command(cx, result);
            }
            if self
                .ui
                .button(cx, ids!(sheets_cell_clear_button))
                .clicked(actions)
            {
                let result = self.state.apply_ribbon_command(RibbonCommand::ClearCell);
                self.finish_home_command(cx, result);
            }
            for (widget_id, command) in [
                (ids!(sheets_bold_button), SheetsHomeFormat::ToggleBold),
                (ids!(sheets_italic_button), SheetsHomeFormat::ToggleItalic),
                (
                    ids!(sheets_underline_button),
                    SheetsHomeFormat::ToggleUnderline,
                ),
                (ids!(sheets_align_left_button), SheetsHomeFormat::AlignLeft),
                (
                    ids!(sheets_align_center_button),
                    SheetsHomeFormat::AlignCenter,
                ),
                (
                    ids!(sheets_align_right_button),
                    SheetsHomeFormat::AlignRight,
                ),
                (ids!(sheets_wrap_button), SheetsHomeFormat::ToggleWrap),
            ] {
                if self.ui.button(cx, widget_id).clicked(actions) {
                    let result = self.state.apply_sheets_home_format(command);
                    self.finish_home_command(cx, result);
                }
            }
            let color_dropdown = self.ui.drop_down(cx, ids!(sheets_color_dropdown));
            if let Some(index) = color_dropdown.selected(actions) {
                let result = sheets_color_command_from_dropdown(index)
                    .ok_or_else(|| "Выберите цвет Sheets из списка.".to_owned())
                    .and_then(|command| self.state.apply_sheets_home_format(command));
                color_dropdown.set_selected_item(cx, COLOR_DROPDOWN_PROMPT_INDEX);
                self.finish_home_command(cx, result);
            }
        }

        if self.state.should_show_slides_viewer() && self.state.home_ribbon_open {
            for (widget_id, command) in [
                (ids!(slides_new_button), SlidesHomeCommand::Add),
                (ids!(slides_duplicate_button), SlidesHomeCommand::Duplicate),
                (ids!(slides_delete_button), SlidesHomeCommand::Delete),
            ] {
                if self.ui.button(cx, widget_id).clicked(actions) {
                    let result = self.state.apply_slides_home_command(command);
                    self.finish_home_command(cx, result);
                }
            }
            let color_dropdown = self.ui.drop_down(cx, ids!(slides_text_color_dropdown));
            if let Some(index) = color_dropdown.selected(actions) {
                let result = slides_color_command_from_dropdown(index)
                    .ok_or_else(|| "Выберите цвет текста Slides из списка.".to_owned())
                    .and_then(|command| self.state.apply_slides_home_command(command));
                color_dropdown.set_selected_item(cx, COLOR_DROPDOWN_PROMPT_INDEX);
                self.finish_home_command(cx, result);
            }
        }
        if self.state.active_module() == Module::Pdf {
            if self.ui.button(cx, ids!(pdf_first_button)).clicked(actions) {
                self.state.pdf_first();
                self.refresh_ui(cx);
            }
            if self.ui.button(cx, ids!(pdf_prev_button)).clicked(actions) {
                self.state.pdf_previous();
                self.refresh_ui(cx);
            }
            if self.ui.button(cx, ids!(pdf_next_button)).clicked(actions) {
                self.state.pdf_next();
                self.refresh_ui(cx);
            }
            if self.ui.button(cx, ids!(pdf_last_button)).clicked(actions) {
                self.state.pdf_last();
                self.refresh_ui(cx);
            }
            if self
                .ui
                .button(cx, ids!(pdf_zoom_in_button))
                .clicked(actions)
            {
                self.state.pdf_zoom_in();
                self.refresh_ui(cx);
            }
            if self
                .ui
                .button(cx, ids!(pdf_zoom_out_button))
                .clicked(actions)
            {
                self.state.pdf_zoom_out();
                self.refresh_ui(cx);
            }
            if self
                .ui
                .button(cx, ids!(pdf_fit_width_button))
                .clicked(actions)
            {
                self.state.pdf_fit_width();
                self.refresh_ui(cx);
            }
            if self
                .ui
                .button(cx, ids!(pdf_fit_page_button))
                .clicked(actions)
            {
                self.state.pdf_fit_page();
                self.refresh_ui(cx);
            }
            if self
                .ui
                .button(cx, ids!(pdf_zoom_reset_button))
                .clicked(actions)
            {
                self.state.pdf_reset_zoom();
                self.refresh_ui(cx);
            }
            if self.ui.button(cx, ids!(pdf_go_button)).clicked(actions) {
                let page_number = self
                    .ui
                    .text_input(cx, ids!(pdf_page_input))
                    .text()
                    .trim()
                    .parse::<u32>()
                    .unwrap_or(0);
                self.state.pdf_go_to_page(page_number);
                self.refresh_ui(cx);
            }

            let canvas = self.ui.widget(cx, ids!(pdf_page_canvas));
            for action in actions.filter_widget_actions(canvas.widget_uid()) {
                match action.cast() {
                    PdfPageCanvasAction::PageClicked(page_id) => {
                        let page_number = self
                            .state
                            .pdf_controller
                            .state()
                            .page_index_position(page_id)
                            .map(|position| position as u32 + 1)
                            .unwrap_or(0);
                        self.state.pdf_go_to_page(page_number);
                        self.refresh_ui(cx);
                    }
                    PdfPageCanvasAction::Wheel {
                        delta_x,
                        delta_y,
                        control,
                    } => {
                        if control {
                            if delta_y < 0 {
                                self.state.pdf_zoom_in();
                            } else if delta_y > 0 {
                                self.state.pdf_zoom_out();
                            }
                        } else {
                            self.state.pdf_scroll(delta_x as f32, delta_y as f32);
                        }
                        self.refresh_ui(cx);
                    }
                    PdfPageCanvasAction::KeyDown(key) => {
                        self.state.pdf_handle_key(key);
                        self.refresh_ui(cx);
                    }
                    PdfPageCanvasAction::None => {}
                }
            }
        }
        if self.state.should_show_sheets_view() {
            for (widget_id, command) in [
                (ids!(sheets_copy_button), SheetsClipboardCommand::Copy),
                (ids!(sheets_cut_button), SheetsClipboardCommand::Cut),
                (ids!(sheets_paste_button), SheetsClipboardCommand::Paste),
            ] {
                if self.ui.button(cx, widget_id).clicked(actions) {
                    self.dispatch_sheets_clipboard(cx, command);
                }
            }
            if self
                .ui
                .button(cx, ids!(sheets_undo_button))
                .clicked(actions)
            {
                self.dispatch_active_history_command(cx, OfficeHistoryCommand::Undo);
            }
            if self
                .ui
                .button(cx, ids!(sheets_redo_button))
                .clicked(actions)
            {
                self.dispatch_active_history_command(cx, OfficeHistoryCommand::Redo);
            }
            let grid = self.ui.widget(cx, ids!(sheets_grid_canvas));
            for action in actions.filter_widget_actions(grid.widget_uid()) {
                match action.cast() {
                    SheetsGridAction::SheetClicked(sheet) => {
                        self.state.select_sheet_id(sheet);
                        self.refresh_ui(cx);
                    }
                    SheetsGridAction::Wheel {
                        delta_x,
                        delta_y,
                        control,
                    } => {
                        self.state.sheets_wheel(delta_x, delta_y, control);
                        self.refresh_ui(cx);
                    }
                    SheetsGridAction::SelectionInput(input) => {
                        self.state.sheets_handle_selection_input(input);
                        self.refresh_ui(cx);
                    }
                    SheetsGridAction::KeyDown(key, modifiers) => {
                        self.state.sheets_handle_key(key, modifiers);
                        self.refresh_ui(cx);
                    }
                    SheetsGridAction::ZoomIn => {
                        self.state.sheets_zoom_in();
                        self.refresh_ui(cx);
                    }
                    SheetsGridAction::ZoomOut => {
                        self.state.sheets_zoom_out();
                        self.refresh_ui(cx);
                    }
                    SheetsGridAction::None => {}
                }
            }
            if self
                .ui
                .button(cx, ids!(sheets_prev_button))
                .clicked(actions)
            {
                self.state.sheets_previous();
                self.refresh_ui(cx);
            }
            if self
                .ui
                .button(cx, ids!(sheets_next_button))
                .clicked(actions)
            {
                self.state.sheets_next();
                self.refresh_ui(cx);
            }
        }

        if self.state.should_show_slides_viewer() {
            if self
                .ui
                .button(cx, ids!(slides_prev_button))
                .clicked(actions)
            {
                self.state.slides_previous();
                self.refresh_ui(cx);
            }
            if self
                .ui
                .button(cx, ids!(slides_next_button))
                .clicked(actions)
            {
                self.state.slides_next();
                self.refresh_ui(cx);
            }
            if self
                .ui
                .button(cx, ids!(slides_zoom_in_button))
                .clicked(actions)
            {
                self.state.slides_zoom_in();
                self.refresh_ui(cx);
            }
            if self
                .ui
                .button(cx, ids!(slides_zoom_out_button))
                .clicked(actions)
            {
                self.state.slides_zoom_out();
                self.refresh_ui(cx);
            }
            if self
                .ui
                .button(cx, ids!(slides_zoom_reset_button))
                .clicked(actions)
            {
                self.state.slides_reset();
                self.refresh_ui(cx);
            }
        }

        if self.state.should_show_slides_viewer() {
            if self.ui.button(cx, ids!(slides_fit_button)).clicked(actions) {
                self.state.slides_fit();
                self.refresh_ui(cx);
            }

            if self
                .ui
                .button(cx, ids!(slides_present_button))
                .clicked(actions)
            {
                self.state.slides_toggle_presentation();
                self.refresh_ui(cx);
            }
            let canvas = self.ui.widget(cx, ids!(slides_visual_canvas));
            for action in actions.filter_widget_actions(canvas.widget_uid()) {
                match action.cast() {
                    SlidesVisualAction::ThumbnailClicked(index) => {
                        self.state.slides_select(index);
                        self.refresh_ui(cx);
                    }
                    SlidesVisualAction::TextObjectClicked(object_id) => {
                        let result = self.state.slides_select_text_object(object_id);
                        self.finish_home_command(cx, result);
                    }
                    SlidesVisualAction::Wheel {
                        control,
                        delta_y,
                        delta_x,
                    } if control => {
                        let _ = delta_x;
                        if delta_y < 0 {
                            self.state.slides_zoom_in();
                        } else if delta_y > 0 {
                            self.state.slides_zoom_out();
                        }
                        self.refresh_ui(cx);
                    }
                    SlidesVisualAction::KeyDown(key) => {
                        self.state.slides_handle_key(key);
                        self.refresh_ui(cx);
                    }
                    SlidesVisualAction::Wheel { .. } | SlidesVisualAction::None => {}
                }
            }
        }
        let writer_active = self.state.should_show_writer_view();
        if writer_active {
            let inline_text_input = self.ui.widget(cx, ids!(writer_inline_text_input));
            let mut inline_commit = false;
            let mut inline_cancel = false;
            for action in actions.filter_widget_actions(inline_text_input.widget_uid()) {
                match action.cast() {
                    TextInputAction::Changed(text) if self.writer_inline_editor.is_some() => {
                        self.state.update_writer_text_editor_draft(text);
                    }
                    TextInputAction::Returned(_, _) if self.writer_inline_editor.is_some() => {
                        inline_commit = true;
                    }
                    TextInputAction::Escaped if self.writer_inline_editor.is_some() => {
                        inline_cancel = true;
                    }
                    _ => {}
                }
            }
            if inline_cancel {
                self.cancel_writer_inline_editor(cx);
                self.refresh_ui(cx);
            } else if inline_commit {
                let result = self.commit_writer_inline_editor(cx);
                self.finish_home_command(cx, result);
            }
            if self
                .ui
                .button(cx, ids!(writer_edit_text_button))
                .clicked(actions)
            {
                self.show_home_ribbon(cx);
                cx.set_key_focus(self.ui.text_input(cx, ids!(writer_text_input)).area());
            }
            if self
                .ui
                .button(cx, ids!(writer_pages_button))
                .clicked(actions)
            {
                self.state.apply_writer_view_mode(ViewMode::Pages);
                self.refresh_ui(cx);
            }
            if self
                .ui
                .button(cx, ids!(writer_continuous_button))
                .clicked(actions)
            {
                self.state.apply_writer_view_mode(ViewMode::Continuous);
                self.refresh_ui(cx);
            }
            if self
                .ui
                .button(cx, ids!(writer_visual_button))
                .clicked(actions)
            {
                self.state
                    .set_writer_display_mode(WriterDisplayMode::Visual);
                self.refresh_ui(cx);
            }
            if self
                .ui
                .button(cx, ids!(writer_plain_text_button))
                .clicked(actions)
            {
                self.state
                    .set_writer_display_mode(WriterDisplayMode::PlainText);
                self.refresh_ui(cx);
            }
            let zoom_dropdown = self.ui.drop_down(cx, ids!(writer_zoom_dropdown));
            if let Some(index) = zoom_dropdown.selected(actions) {
                if let Some(percent) = writer_zoom_percent_from_dropdown(index) {
                    let result = self.set_writer_canvas_zoom_percent(cx, percent);
                    self.finish_home_command(cx, result);
                } else if let Some(percent) = self
                    .ui
                    .widget(cx, ids!(writer_page_canvas))
                    .borrow::<VirtualCanvas>()
                    .map(|canvas| canvas.zoom_percent())
                {
                    zoom_dropdown
                        .set_selected_item(cx, writer_zoom_dropdown_index_for_percent(percent));
                }
            }
            if self
                .ui
                .button(cx, ids!(writer_zoom_in_button))
                .clicked(actions)
            {
                if let Some(mut canvas) = self
                    .ui
                    .widget(cx, ids!(writer_page_canvas))
                    .borrow_mut::<VirtualCanvas>()
                {
                    canvas.zoom_in(cx);
                }
                self.sync_writer_runtime_view(cx);
                self.refresh_ui(cx);
            }
            if self
                .ui
                .button(cx, ids!(writer_zoom_out_button))
                .clicked(actions)
            {
                if let Some(mut canvas) = self
                    .ui
                    .widget(cx, ids!(writer_page_canvas))
                    .borrow_mut::<VirtualCanvas>()
                {
                    canvas.zoom_out(cx);
                }
                self.sync_writer_runtime_view(cx);
                self.refresh_ui(cx);
            }

            let canvas = self.ui.widget(cx, ids!(writer_page_canvas));
            let mut viewport_changed = false;
            for action in actions.filter_widget_actions(canvas.widget_uid()) {
                match action.cast() {
                    VirtualCanvasAction::ViewportChanged(_) => viewport_changed = true,
                    VirtualCanvasAction::ParagraphActivated {
                        interaction_id,
                        page_index: _,
                        bounds,
                        cursor_byte_offset,
                    } => {
                        let result = self.begin_writer_inline_editor(
                            cx,
                            &interaction_id,
                            bounds,
                            cursor_byte_offset,
                        );
                        self.finish_home_command(cx, result);
                    }
                    VirtualCanvasAction::RulerMarkerChanged {
                        marker_id,
                        page_index: _,
                        logical_position,
                    } => {
                        let result = self
                            .state
                            .apply_writer_ruler_change(&marker_id, logical_position);
                        self.finish_home_command(cx, result);
                    }
                    VirtualCanvasAction::None => {}
                }
            }
            if viewport_changed {
                self.sync_writer_runtime_view(cx);
                self.refresh_ui(cx);
            }
        }
    }
}

impl AppMain for App {
    fn script_mod(vm: &mut ScriptVm) -> ScriptValue {
        crate::makepad_widgets::theme_mod(vm);
        script_eval!(vm, {
            mod.theme = mod.themes.light
        });
        crate::makepad_widgets::widgets_mod(vm);
        office_ui_runtime::script_mod(vm);
        rudra_office_launcher::view::script_mod(vm);
        crate::app_writer_view::script_mod(vm);
        crate::app_ui::script_mod(vm)
    }

    fn handle_event(&mut self, cx: &mut Cx, event: &Event) {
        #[cfg(windows)]
        {
            // Drain BEFORE chrome, open polling, focus changes, Undo/Copy/Cut,
            // or any later input. Signal is a wake-up, not an ordering boundary.
            let pending = self
                .native_paste_hook
                .borrow()
                .as_ref()
                .and_then(|hook| hook.take_pending());
            sheets_clipboard_state::flush_clipboard_intent_before_event(pending, |target| {
                if self.sheets_grid_has_key_focus(cx)
                    && self.state.active_document_id() == Some(target.document)
                    && self.state.sheets_clipboard_range().ok() == Some(target.range)
                {
                    self.dispatch_sheets_clipboard(cx, SheetsClipboardCommand::Paste);
                } else {
                    self.finish_home_command(cx, Err("Назначение Paste изменилось до выполнения; операция отменена без изменений.".into()));
                }
            });
            self.refresh_native_paste_target(cx);
        }
        self.dismiss_file_menu_on_outside_pointer(cx, event);
        if self.handle_custom_chrome_pointer(cx, event) {
            return;
        }
        if matches!(event, Event::Draw(_) | Event::Signal) {
            self.poll_document_open(cx);
        }
        if self.sheets_grid_has_key_focus(cx) {
            // Windows sends native clipboard events BEFORE the corresponding
            // KeyDown. Consume them here and dispatch exactly once on KeyDown,
            // including unsupported/empty clipboard, without a second native write.
            // Focused TextInputs bypass this block and retain their own draft actions.
            match event {
                Event::TextCopy(_) | Event::TextCut(_) => return,
                Event::TextInput(input) if input.was_paste => return,
                Event::KeyDown(key)
                    if (key.modifiers.control || key.modifiers.logo) && !key.modifiers.alt =>
                {
                    let command = match key.key_code {
                        KeyCode::KeyC => Some(SheetsClipboardCommand::Copy),
                        KeyCode::KeyX => Some(SheetsClipboardCommand::Cut),
                        KeyCode::KeyV => Some(SheetsClipboardCommand::Paste),
                        _ => None,
                    };
                    if let Some(command) = command {
                        self.dispatch_sheets_clipboard(cx, command);
                        return;
                    }
                }
                _ => {}
            }
        }
        if let Event::KeyDown(key) = event
            && key.key_code == KeyCode::Escape
            && self.state.sheets_clipboard.cancel_cut()
        {
            self.state.status = "Pending Cut отменён; исходные ячейки не изменены.".into();
            self.refresh_ui(cx);
        }
        if let Event::KeyDown(key) = event
            && (key.modifiers.control || key.modifiers.logo)
        {
            // A focused TextInput owns its own draft history. Only route
            // document history when the key belongs to the page/workspace.
            let history_command = match key.key_code {
                KeyCode::KeyZ if key.modifiers.shift => Some(OfficeHistoryCommand::Redo),
                KeyCode::KeyZ => Some(OfficeHistoryCommand::Undo),
                KeyCode::KeyY => Some(OfficeHistoryCommand::Redo),
                _ => None,
            };
            if let Some(command) = history_command
                && (self.state.writer_active() || self.state.sheets_editing_available())
                && !key.modifiers.alt
                && !self.text_input_has_key_focus(cx)
            {
                self.dispatch_active_history_command(cx, command);
                return;
            }
            match key.key_code {
                KeyCode::KeyO => self.open_document(cx),
                KeyCode::Tab => self.cycle_document_tab(cx, key.modifiers.shift),
                KeyCode::KeyW => {
                    if let Some(id) = self.state.active_document_id()
                        && let Err(error) = self.close_document(cx, id)
                    {
                        self.state.status = "Document tab close failed.".to_owned();
                        self.state.detail = error.to_string();
                        self.refresh_ui(cx);
                    }
                }
                KeyCode::Escape => self.set_file_menu_open(cx, false),
                _ => {}
            }
        }
        if let Event::KeyDown(key) = event
            && key.key_code == KeyCode::Escape
            && self.state.file_menu_open
        {
            self.set_file_menu_open(cx, false);
        }
        self.prepare_writer_zoom_popup(cx, event);
        self.match_event(cx, event);
        self.ui.handle_event(cx, event, &mut Scope::empty());
        if matches!(event, Event::Draw(_)) {
            self.apply_launcher_startup_request(cx);
        }
        self.restore_writer_zoom_selection_after_dismissal(cx, event);
        if matches!(event, Event::Draw(_)) {
            self.refresh_writer_status_after_draw(cx);
        }
        if matches!(event, Event::Draw(_)) && self.writer_inline_editor_pending_focus {
            let inline_input = self.ui.text_input(cx, ids!(writer_inline_text_input));
            let area = inline_input.area();
            if !area.is_empty() {
                cx.set_key_focus(area);
                self.writer_inline_editor_pending_focus = false;
            }
        }
        let slides_needs_reframe = matches!(event, Event::Draw(_))
            && self.state.should_show_slides_viewer()
            && self
                .ui
                .widget(cx, ids!(slides_visual_canvas))
                .borrow::<SlidesVisualCanvas>()
                .is_some_and(|canvas| canvas.needs_viewport_reframe(cx));
        let sheets_needs_reframe = matches!(event, Event::Draw(_))
            && self.state.should_show_sheets_view()
            && self.sync_sheets_canvas_viewport(cx);
        if slides_needs_reframe || sheets_needs_reframe {
            self.refresh_ui(cx);
            // The viewport-aware frame is rebuilt after the current Draw pass.
            // Request one follow-up pass explicitly; a redraw attached to the
            // just-finished draw area can otherwise remain pending until the
            // next unrelated window event (for example, a resize).
            cx.redraw_all();
        }
        if matches!(event, Event::WindowGeomChange(_)) {
            if self.state.file_menu_open {
                self.position_file_menu(cx);
            }
            self.reveal_active_document_tab_after_resize(cx);
        }
        #[cfg(windows)]
        if matches!(event, Event::Draw(_) | Event::KeyFocus(_)) {
            self.refresh_accessibility(cx);
        }
    }
}
