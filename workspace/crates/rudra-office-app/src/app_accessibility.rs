//! Application-owned module extracted without changing behavior.
use super::*;

impl App {
    pub(crate) fn document_tab_accessibility_frames(
        &self,
        cx: &Cx,
    ) -> Vec<document_tabs::DocumentTabAccessibilityFrame> {
        self.ui
            .widget(cx, ids!(document_tabs))
            .borrow::<DocumentTabStrip>()
            .map(|tabs| tabs.accessibility_tabs(cx))
            .unwrap_or_default()
    }

    #[cfg(windows)]
    pub(crate) fn sheets_accessibility_snapshot(
        &self,
        cx: &Cx,
    ) -> Option<SheetsGridAccessibilitySnapshot> {
        self.ui
            .widget(cx, ids!(sheets_grid_canvas))
            .borrow::<SheetsGridCanvas>()
            .and_then(|grid| grid.accessibility_snapshot(cx))
    }

    #[cfg(windows)]
    pub(crate) fn slides_accessibility_snapshot(
        &self,
        cx: &Cx,
    ) -> Option<SlidesVisualAccessibilitySnapshot> {
        self.ui
            .widget(cx, ids!(slides_visual_canvas))
            .borrow::<SlidesVisualCanvas>()
            .and_then(|canvas| canvas.accessibility_snapshot())
    }

    #[cfg(windows)]
    pub(crate) fn refresh_accessibility(&self, cx: &mut Cx) {
        // Keyboard ownership follows actual Cx focus, not successful UIA export.
        self.refresh_native_paste_target(cx);
        if let Ok(window) = accessibility_bridge::current_window() {
            match self.accessibility_tree(cx, &window) {
                Ok(tree) => accessibility_bridge::publish(tree, &window),
                Err(error) => eprintln!("Rudra accessibility tree refresh failed: {error}"),
            }
        }
        // Install AFTER the existing UIA provider's WNDPROC wrapper, so common
        // controls preserve its chain. Both owners remain independent.
        let mut slot = self.native_paste_hook.borrow_mut();
        if slot.is_none() {
            match rudra_office_hwnd()
                .and_then(windows_grid_paste_hook::NativeGridPasteHook::install)
            {
                Ok(hook) => *slot = Some(hook),
                Err(_)
                    if !self.state.should_show_sheets_view()
                        && accessibility_bridge::current_window().is_err() =>
                {
                    // Startup may precede native window creation. No grid exists yet.
                }
                Err(error) => {
                    // This guard is established during window bootstrap, before
                    // document editing. Do not fall back to an unbounded grid path.
                    eprintln!(
                        "Rudra window bootstrap stopped: bounded clipboard owner unavailable: {error}"
                    );
                    cx.quit();
                    return;
                }
            }
        }
        drop(slot);
        self.refresh_native_paste_target(cx);
    }

    #[cfg(windows)]
    pub(crate) fn accessibility_tree(
        &self,
        cx: &Cx,
        window: &AccessibilityWindow,
    ) -> Result<AccessibilityTree, String> {
        let mut root = AccessibilityNode::new(
            "rudra-office/window",
            AccessibilityRole::Window,
            "Rudra Office",
            window.bounds(),
        );

        let mut window_controls = accessibility_node(
            window,
            "rudra-office/window-controls",
            AccessibilityRole::ToolBar,
            "Управление окном",
            self.ui.view(cx, ids!(header_tabs_bar)).area().rect(cx),
        );
        window_controls.children.push(accessibility_button(
            window,
            "rudra-office/window/minimize",
            "Свернуть окно",
            self.ui.button(cx, ids!(chrome_minimize)).area().rect(cx),
            true,
        ));
        window_controls.children.push(accessibility_button(
            window,
            "rudra-office/window/maximize",
            "Развернуть или восстановить окно",
            self.ui.button(cx, ids!(chrome_maximize)).area().rect(cx),
            true,
        ));
        window_controls.children.push(accessibility_button(
            window,
            "rudra-office/window/close",
            "Закрыть Rudra Office",
            self.ui.button(cx, ids!(chrome_close)).area().rect(cx),
            true,
        ));

        let mut menu_bar = accessibility_node(
            window,
            "rudra-office/menu-bar",
            AccessibilityRole::MenuBar,
            "Главное меню",
            self.ui.view(cx, ids!(menu_bar)).area().rect(cx),
        );
        let mut file_menu = accessibility_node(
            window,
            "rudra-office/menu/file-trigger",
            AccessibilityRole::MenuItem,
            "Файл",
            self.ui.button(cx, ids!(menu_file_button)).area().rect(cx),
        );
        file_menu.actions.invoke = true;
        menu_bar.children.push(file_menu);
        let mut home_menu = accessibility_node(
            window,
            "rudra-office/menu/home-trigger",
            AccessibilityRole::MenuItem,
            "Главная",
            self.ui.button(cx, ids!(menu_home_button)).area().rect(cx),
        );
        home_menu.enabled = self.state.active_document_id().is_some();
        home_menu.actions.invoke = true;
        home_menu.selected = Some(self.state.ribbon_tab == RibbonTab::Home);
        menu_bar.children.push(home_menu);
        for (tab, widget_id) in ribbon_tab_widgets() {
            if tab == RibbonTab::Home || !tabs_for_format(self.state.active_module()).contains(&tab)
            {
                continue;
            }
            let mut node = accessibility_node(
                window,
                format!("rudra-office/menu/{}-trigger", tab.id()),
                AccessibilityRole::MenuItem,
                tab.label(),
                self.ui.button(cx, widget_id).area().rect(cx),
            );
            node.actions.invoke = true;
            node.enabled = self.state.active_document_id().is_some();
            node.selected = Some(self.state.ribbon_tab == tab);
            menu_bar.children.push(node);
        }
        root.children.push(menu_bar);

        if self.state.home_ribbon_open
            && self.state.active_document_id().is_some()
            && self.state.ribbon_tab != RibbonTab::Home
        {
            let mut ribbon = accessibility_node(
                window,
                format!("rudra-office/ribbon/{}", self.state.ribbon_tab.id()),
                AccessibilityRole::ToolBar,
                self.state.ribbon_tab.label(),
                self.ui.view(cx, ids!(extra_ribbon)).area().rect(cx),
            );
            for ((command, label), widget_id) in
                commands_for(self.state.active_module(), self.state.ribbon_tab)
                    .iter()
                    .zip(ribbon_command_widgets())
            {
                ribbon.children.push(accessibility_button(
                    window,
                    format!("rudra-office/ribbon/{}", command.id()),
                    *label,
                    self.ui.button(cx, widget_id).area().rect(cx),
                    self.state.ribbon_command_available(*command),
                ));
            }
            if self.state.active_module() == Module::Sheets
                && self.state.ribbon_tab == RibbonTab::Data
            {
                for (node_id, name, widget_id) in [
                    (
                        "rudra-office/sheets/address-input",
                        "Адрес ячейки или диапазона",
                        ids!(sheets_address_input),
                    ),
                    (
                        "rudra-office/sheets/search-input",
                        "Поиск по текущему листу",
                        ids!(sheets_search_input),
                    ),
                    (
                        "rudra-office/sheets/key-column",
                        "Столбец сортировки и фильтра",
                        ids!(sheets_key_column_input),
                    ),
                    (
                        "rudra-office/sheets/filter-value",
                        "Критерий фильтра; значения разделены переводом строки",
                        ids!(sheets_filter_value_input),
                    ),
                ] {
                    let widget = self.ui.text_input(cx, widget_id);
                    let mut input = accessibility_node(
                        window,
                        node_id,
                        AccessibilityRole::Edit,
                        name,
                        widget.area().rect(cx),
                    );
                    input.value = Some(widget.text());
                    input.enabled = self.state.sheets_tools_available();
                    input.focused = cx.has_key_focus(widget.area());
                    input.actions = AccessibilityActions {
                        focus: input.enabled,
                        set_value: input.enabled,
                        ..Default::default()
                    };
                    ribbon.children.push(input);
                }
                for (node_id, name, widget_id) in [
                    (
                        "rudra-office/sheets/has-header",
                        "Первая строка — заголовок",
                        ids!(sheets_header_dropdown),
                    ),
                    (
                        "rudra-office/sheets/filter-kind",
                        "Условие фильтра",
                        ids!(sheets_filter_kind_dropdown),
                    ),
                ] {
                    let widget = self.ui.drop_down(cx, widget_id);
                    let mut node = accessibility_node(
                        window,
                        node_id,
                        AccessibilityRole::ComboBox,
                        name,
                        widget.area().rect(cx),
                    );
                    node.value = Some(widget.selected_label());
                    node.enabled = self.state.sheets_tools_available();
                    node.actions = AccessibilityActions {
                        focus: node.enabled,
                        set_value: node.enabled,
                        ..Default::default()
                    };
                    ribbon.children.push(node);
                }
            }
            let mut feedback = accessibility_node(
                window,
                "rudra-office/ribbon/feedback",
                AccessibilityRole::Text,
                "Результат команды",
                self.ui.label(cx, ids!(extra_ribbon_title)).area().rect(cx),
            );
            feedback.value = self.state.ribbon_command_feedback.clone();
            ribbon.children.push(feedback);
            root.children.push(ribbon);
        }
        if self.state.home_ribbon_open
            && self.state.active_document_id().is_some()
            && self.state.ribbon_tab == RibbonTab::Home
        {
            let mut ribbon = accessibility_node(
                window,
                "rudra-office/home",
                AccessibilityRole::ToolBar,
                "Главная",
                self.ui.view(cx, ids!(home_ribbon)).area().rect(cx),
            );
            match self.state.active_module() {
                Module::Writer => {
                    let writer_text_editor = self.state.writer_text_editor_model();
                    let writer_text_editor_enabled = writer_text_editor
                        .as_ref()
                        .is_some_and(|model| model.enabled);
                    for (node_id, name, widget_id, enabled) in [
                        (
                            "rudra-office/home/writer/text-apply",
                            "Применить текст абзаца",
                            ids!(writer_text_apply_button),
                            writer_text_editor_enabled,
                        ),
                        (
                            "rudra-office/home/writer/text-revert",
                            "Вернуть черновик текста абзаца",
                            ids!(writer_text_revert_button),
                            writer_text_editor_enabled,
                        ),
                        (
                            "rudra-office/home/writer/undo",
                            "Отменить изменение",
                            ids!(writer_undo_button),
                            self.state.writer_can_undo(),
                        ),
                        (
                            "rudra-office/home/writer/redo",
                            "Повторить изменение",
                            ids!(writer_redo_button),
                            self.state.writer_can_redo(),
                        ),
                        (
                            "rudra-office/home/writer/font-apply",
                            "Применить шрифт и размер",
                            ids!(writer_font_apply_button),
                            self.state.writer_character_toolbar_available(),
                        ),
                        (
                            "rudra-office/home/writer/bold",
                            "Полужирный",
                            ids!(writer_bold_button),
                            self.state.writer_character_toolbar_available(),
                        ),
                        (
                            "rudra-office/home/writer/italic",
                            "Курсив",
                            ids!(writer_italic_button),
                            self.state.writer_character_toolbar_available(),
                        ),
                        (
                            "rudra-office/home/writer/underline",
                            "Подчёркивание",
                            ids!(writer_underline_button),
                            self.state.writer_character_toolbar_available(),
                        ),
                        (
                            "rudra-office/home/writer/strike",
                            "Зачёркивание",
                            ids!(writer_strike_button),
                            self.state.writer_character_toolbar_available(),
                        ),
                        (
                            "rudra-office/home/writer/align-left",
                            "Выровнять по левому краю",
                            ids!(writer_align_left_button),
                            self.state.writer_paragraph_toolbar_available(),
                        ),
                        (
                            "rudra-office/home/writer/align-center",
                            "Выровнять по центру",
                            ids!(writer_align_center_button),
                            self.state.writer_paragraph_toolbar_available(),
                        ),
                        (
                            "rudra-office/home/writer/align-right",
                            "Выровнять по правому краю",
                            ids!(writer_align_right_button),
                            self.state.writer_paragraph_toolbar_available(),
                        ),
                        (
                            "rudra-office/home/writer/align-justify",
                            "Выровнять по ширине",
                            ids!(writer_align_justify_button),
                            self.state.writer_paragraph_toolbar_available(),
                        ),
                        (
                            "rudra-office/home/writer/style-normal",
                            "Обычный стиль",
                            ids!(writer_normal_button),
                            self.state.writer_paragraph_toolbar_available(),
                        ),
                        (
                            "rudra-office/home/writer/style-h1",
                            "Заголовок 1",
                            ids!(writer_h1_button),
                            self.state.writer_paragraph_toolbar_available(),
                        ),
                        (
                            "rudra-office/home/writer/style-h2",
                            "Заголовок 2",
                            ids!(writer_h2_button),
                            self.state.writer_paragraph_toolbar_available(),
                        ),
                    ] {
                        ribbon.children.push(accessibility_button(
                            window,
                            node_id,
                            name,
                            self.ui.button(cx, widget_id).area().rect(cx),
                            enabled,
                        ));
                    }
                    let color_enabled = self.state.writer_character_toolbar_available();
                    let color_widget = self.ui.widget(cx, ids!(writer_color_dropdown));
                    let color_bounds = color_widget.area().rect(cx);
                    let mut color = accessibility_node(
                        window,
                        "rudra-office/home/writer/color",
                        AccessibilityRole::ComboBox,
                        "Цвет текста и фона",
                        color_bounds,
                    );
                    color.value = Some(self.state.writer_color_value.unwrap_or("unset").to_owned());
                    color.enabled = color_enabled;
                    color.focused = cx.has_key_focus(color_widget.area());
                    color.actions = AccessibilityActions {
                        focus: color_enabled,
                        set_value: color_enabled,
                        ..Default::default()
                    };
                    for (node_id, name) in [
                        (
                            "rudra-office/home/writer/text-auto",
                            "Автоматический цвет текста",
                        ),
                        ("rudra-office/home/writer/text-black", "Чёрный цвет текста"),
                        ("rudra-office/home/writer/text-red", "Красный цвет текста"),
                        ("rudra-office/home/writer/text-blue", "Синий цвет текста"),
                        ("rudra-office/home/writer/text-green", "Зелёный цвет текста"),
                        (
                            "rudra-office/home/writer/text-purple",
                            "Фиолетовый цвет текста",
                        ),
                        (
                            "rudra-office/home/writer/highlight-none",
                            "Убрать заливку текста",
                        ),
                        (
                            "rudra-office/home/writer/highlight-yellow",
                            "Жёлтая заливка текста",
                        ),
                        (
                            "rudra-office/home/writer/highlight-green",
                            "Зелёная заливка текста",
                        ),
                        (
                            "rudra-office/home/writer/highlight-blue",
                            "Голубая заливка текста",
                        ),
                        (
                            "rudra-office/home/writer/highlight-pink",
                            "Розовая заливка текста",
                        ),
                        (
                            "rudra-office/home/writer/highlight-gray",
                            "Серая заливка текста",
                        ),
                    ] {
                        let mut option = accessibility_node(
                            window,
                            node_id,
                            AccessibilityRole::MenuItem,
                            name,
                            color_bounds,
                        );
                        option.visible = false;
                        option.enabled = color_enabled;
                        option.actions.invoke = color_enabled;
                        color.children.push(option);
                    }
                    ribbon.children.push(color);
                    let paragraph_widget = self.ui.widget(cx, ids!(writer_text_block_dropdown));
                    let mut paragraph = accessibility_node(
                        window,
                        "rudra-office/home/writer/paragraph",
                        AccessibilityRole::ComboBox,
                        "Выбранный абзац для редактирования",
                        paragraph_widget.area().rect(cx),
                    );
                    paragraph.value = writer_text_editor
                        .as_ref()
                        .map(|model| model.selected_index.to_string());
                    paragraph.enabled = writer_text_editor_enabled;
                    paragraph.focused = cx.has_key_focus(paragraph_widget.area());
                    paragraph.actions = AccessibilityActions {
                        focus: writer_text_editor_enabled,
                        set_value: writer_text_editor_enabled,
                        ..Default::default()
                    };
                    ribbon.children.push(paragraph);
                    let text_widget = self.ui.text_input(cx, ids!(writer_text_input));
                    let mut text_input = accessibility_node(
                        window,
                        "rudra-office/home/writer/text",
                        AccessibilityRole::Edit,
                        "Текст выбранного абзаца",
                        text_widget.area().rect(cx),
                    );
                    text_input.value = Some(text_widget.text());
                    text_input.enabled = writer_text_editor_enabled;
                    text_input.focused = cx.has_key_focus(text_widget.area());
                    text_input.actions = AccessibilityActions {
                        focus: writer_text_editor_enabled,
                        set_value: writer_text_editor_enabled,
                        ..Default::default()
                    };
                    ribbon.children.push(text_input);
                    for (node_id, name, widget_id) in [
                        (
                            "rudra-office/home/writer/font-family",
                            "Название шрифта",
                            ids!(writer_font_input),
                        ),
                        (
                            "rudra-office/home/writer/font-size",
                            "Размер шрифта",
                            ids!(writer_size_input),
                        ),
                    ] {
                        let widget = self.ui.text_input(cx, widget_id);
                        let mut input = accessibility_node(
                            window,
                            node_id,
                            AccessibilityRole::Edit,
                            name,
                            widget.area().rect(cx),
                        );
                        input.value = Some(widget.text());
                        input.actions = AccessibilityActions {
                            focus: true,
                            set_value: true,
                            ..Default::default()
                        };
                        input.focused = cx.has_key_focus(widget.area());
                        ribbon.children.push(input);
                    }
                }
                Module::Sheets => {
                    let enabled = self.state.sheets_editing_available();
                    let widget = self.ui.text_input(cx, ids!(sheets_cell_input));
                    let mut input = accessibility_node(
                        window,
                        "rudra-office/home/sheets/cell-value",
                        AccessibilityRole::Edit,
                        "Значение или формула ячейки",
                        widget.area().rect(cx),
                    );
                    input.value = Some(widget.text());
                    input.enabled = enabled;
                    input.actions = AccessibilityActions {
                        focus: enabled,
                        set_value: enabled,
                        ..Default::default()
                    };
                    input.focused = cx.has_key_focus(widget.area());
                    ribbon.children.push(input);
                    for (node_id, name, widget_id, enabled) in [
                        (
                            "rudra-office/home/sheets/copy",
                            "Копировать диапазон · Ctrl+C",
                            ids!(sheets_copy_button),
                            self.state.sheets_copy_available(),
                        ),
                        (
                            "rudra-office/home/sheets/cut",
                            "Вырезать диапазон · Ctrl+X",
                            ids!(sheets_cut_button),
                            self.state.sheets_editing_available()
                                && self.state.sheets_copy_available(),
                        ),
                        (
                            "rudra-office/home/sheets/paste",
                            "Вставить диапазон · Ctrl+V",
                            ids!(sheets_paste_button),
                            self.state.sheets_editing_available()
                                && self.state.sheets_clipboard_range().is_ok()
                                && windows_clipboard::has_text(),
                        ),
                        (
                            "rudra-office/home/sheets/undo",
                            "Отменить изменение таблицы",
                            ids!(sheets_undo_button),
                            self.state.sheets_can_undo(),
                        ),
                        (
                            "rudra-office/home/sheets/redo",
                            "Повторить изменение таблицы",
                            ids!(sheets_redo_button),
                            self.state.sheets_can_redo(),
                        ),
                    ] {
                        ribbon.children.push(accessibility_button(
                            window,
                            node_id,
                            name,
                            self.ui.button(cx, widget_id).area().rect(cx),
                            enabled,
                        ));
                    }
                    let mut clipboard_status = accessibility_node(
                        window,
                        "rudra-office/home/sheets/clipboard-status",
                        AccessibilityRole::Text,
                        "Pending Cut",
                        self.ui.label(cx, ids!(sheets_cut_status)).area().rect(cx),
                    );
                    clipboard_status.value = Some(
                        self.state
                            .sheets_clipboard
                            .pending()
                            .map(|claim| {
                                format!(
                                    "document:{} {}{}:{}{}",
                                    claim.source,
                                    column_label(claim.range.start.column),
                                    claim.range.start.row.0 + 1,
                                    column_label(claim.range.end.column),
                                    claim.range.end.row.0 + 1
                                )
                            })
                            .unwrap_or_else(|| "none".into()),
                    );
                    ribbon.children.push(clipboard_status);
                    let mut native_owner = accessibility_node(
                        window,
                        "rudra-office/home/sheets/native-paste-owner",
                        AccessibilityRole::Text,
                        "Владелец ограниченной вставки",
                        self.ui.label(cx, ids!(sheets_cut_status)).area().rect(cx),
                    );
                    native_owner.value = Some(
                        self.native_paste_hook
                            .borrow()
                            .as_ref()
                            .map(|hook| format!("installed;intercepted={}", hook.intercepted()))
                            .unwrap_or_else(|| "unavailable".into()),
                    );
                    ribbon.children.push(native_owner);
                    for (node_id, name, widget_id) in [
                        (
                            "rudra-office/home/sheets/cell-apply",
                            "Применить значение ячейки",
                            ids!(sheets_cell_apply_button),
                        ),
                        (
                            "rudra-office/home/sheets/cell-clear",
                            "Очистить ячейку",
                            ids!(sheets_cell_clear_button),
                        ),
                        (
                            "rudra-office/home/sheets/bold",
                            "Полужирный",
                            ids!(sheets_bold_button),
                        ),
                        (
                            "rudra-office/home/sheets/italic",
                            "Курсив",
                            ids!(sheets_italic_button),
                        ),
                        (
                            "rudra-office/home/sheets/underline",
                            "Подчёркивание",
                            ids!(sheets_underline_button),
                        ),
                        (
                            "rudra-office/home/sheets/align-left",
                            "По левому краю",
                            ids!(sheets_align_left_button),
                        ),
                        (
                            "rudra-office/home/sheets/align-center",
                            "По центру",
                            ids!(sheets_align_center_button),
                        ),
                        (
                            "rudra-office/home/sheets/align-right",
                            "По правому краю",
                            ids!(sheets_align_right_button),
                        ),
                        (
                            "rudra-office/home/sheets/wrap",
                            "Перенос текста",
                            ids!(sheets_wrap_button),
                        ),
                    ] {
                        ribbon.children.push(accessibility_button(
                            window,
                            node_id,
                            name,
                            self.ui.button(cx, widget_id).area().rect(cx),
                            enabled,
                        ));
                    }
                    let color_widget = self.ui.widget(cx, ids!(sheets_color_dropdown));
                    let color_bounds = color_widget.area().rect(cx);
                    let mut color = accessibility_node(
                        window,
                        "rudra-office/home/sheets/color",
                        AccessibilityRole::ComboBox,
                        "Цвет текста и заливка всей ячейки",
                        color_bounds,
                    );
                    color.value = Some(self.state.sheets_color_value.unwrap_or("unset").to_owned());
                    color.enabled = enabled;
                    color.focused = cx.has_key_focus(color_widget.area());
                    color.actions = AccessibilityActions {
                        focus: enabled,
                        set_value: enabled,
                        ..Default::default()
                    };
                    for (node_id, name) in [
                        ("rudra-office/home/sheets/text-black", "Чёрный цвет текста"),
                        ("rudra-office/home/sheets/text-red", "Красный цвет текста"),
                        ("rudra-office/home/sheets/text-blue", "Синий цвет текста"),
                        ("rudra-office/home/sheets/text-green", "Зелёный цвет текста"),
                        (
                            "rudra-office/home/sheets/text-purple",
                            "Фиолетовый цвет текста",
                        ),
                        (
                            "rudra-office/home/sheets/cell-white",
                            "Белая заливка всей ячейки",
                        ),
                        (
                            "rudra-office/home/sheets/cell-yellow",
                            "Жёлтая заливка всей ячейки",
                        ),
                        (
                            "rudra-office/home/sheets/cell-green",
                            "Зелёная заливка всей ячейки",
                        ),
                        (
                            "rudra-office/home/sheets/cell-blue",
                            "Голубая заливка всей ячейки",
                        ),
                        (
                            "rudra-office/home/sheets/cell-pink",
                            "Розовая заливка всей ячейки",
                        ),
                        (
                            "rudra-office/home/sheets/cell-gray",
                            "Серая заливка всей ячейки",
                        ),
                    ] {
                        let mut option = accessibility_node(
                            window,
                            node_id,
                            AccessibilityRole::MenuItem,
                            name,
                            color_bounds,
                        );
                        option.visible = false;
                        option.enabled = enabled;
                        option.actions.invoke = enabled;
                        color.children.push(option);
                    }
                    ribbon.children.push(color);
                }
                Module::Slides => {
                    let count = self.state.slides_snapshot().slide_count;
                    for (node_id, name, widget_id, enabled) in [
                        (
                            "rudra-office/home/slides/new",
                            "Новый слайд",
                            ids!(slides_new_button),
                            true,
                        ),
                        (
                            "rudra-office/home/slides/duplicate",
                            "Дублировать слайд",
                            ids!(slides_duplicate_button),
                            count > 0,
                        ),
                        (
                            "rudra-office/home/slides/delete",
                            "Удалить слайд",
                            ids!(slides_delete_button),
                            count > 1,
                        ),
                    ] {
                        ribbon.children.push(accessibility_button(
                            window,
                            node_id,
                            name,
                            self.ui.button(cx, widget_id).area().rect(cx),
                            enabled,
                        ));
                    }
                    let color_enabled = self.state.slides_text_color_available();
                    let color_widget = self.ui.widget(cx, ids!(slides_text_color_dropdown));
                    let color_bounds = color_widget.area().rect(cx);
                    let mut color = accessibility_node(
                        window,
                        "rudra-office/home/slides/text-color",
                        AccessibilityRole::ComboBox,
                        "Цвет текста выбранного объекта",
                        color_bounds,
                    );
                    color.value = Some(self.state.slides_color_value.unwrap_or("unset").to_owned());
                    color.enabled = color_enabled;
                    color.focused = cx.has_key_focus(color_widget.area());
                    color.actions = AccessibilityActions {
                        focus: color_enabled,
                        set_value: color_enabled,
                        ..Default::default()
                    };
                    for (node_id, name) in [
                        ("rudra-office/home/slides/text-black", "Чёрный цвет текста"),
                        ("rudra-office/home/slides/text-red", "Красный цвет текста"),
                        ("rudra-office/home/slides/text-blue", "Синий цвет текста"),
                        ("rudra-office/home/slides/text-green", "Зелёный цвет текста"),
                        (
                            "rudra-office/home/slides/text-purple",
                            "Фиолетовый цвет текста",
                        ),
                    ] {
                        let mut option = accessibility_node(
                            window,
                            node_id,
                            AccessibilityRole::MenuItem,
                            name,
                            color_bounds,
                        );
                        option.visible = false;
                        option.enabled = color_enabled;
                        option.actions.invoke = color_enabled;
                        color.children.push(option);
                    }
                    ribbon.children.push(color);
                }
                Module::Pdf => ribbon.children.push(accessibility_node(
                    window,
                    "rudra-office/home/pdf/read-only",
                    AccessibilityRole::Text,
                    "PDF: защищённый режим просмотра",
                    self.ui.view(cx, ids!(pdf_home_ribbon)).area().rect(cx),
                )),
                Module::Home => {}
            }
            root.children.push(ribbon);
        }

        if self.state.file_menu_open {
            let mut file_popup = accessibility_node(
                window,
                "rudra-office/menu/file",
                AccessibilityRole::Menu,
                "Файл",
                self.ui.view(cx, ids!(file_menu_popup)).area().rect(cx),
            );
            let mut open_item = accessibility_node(
                window,
                "rudra-office/action/open",
                AccessibilityRole::MenuItem,
                "Открыть…",
                self.ui.button(cx, ids!(open_button)).area().rect(cx),
            );
            open_item.value = self
                .state
                .pending_open_path
                .as_ref()
                .map(|path| path.display().to_string());
            open_item.actions.invoke = true;
            open_item.actions.set_value = true;
            file_popup.children.push(open_item);
            root.children.push(file_popup);
        }

        let mut command_bar = accessibility_node(
            window,
            "rudra-office/commands",
            AccessibilityRole::ToolBar,
            "Команды документа",
            self.ui.view(cx, ids!(menu_bar)).area().rect(cx),
        );
        command_bar.children.push(accessibility_button(
            window,
            "rudra-office/action/save",
            "Сохранить новый документ как",
            self.ui.button(cx, ids!(save_button)).area().rect(cx),
            self.state.can_save_new_document(),
        ));
        command_bar.children.push(accessibility_button(
            window,
            "rudra-office/action/probe-engines",
            "Проверить движки",
            self.ui.button(cx, ids!(probe_button)).area().rect(cx),
            true,
        ));
        root.children.push(command_bar);

        let active_module = self.state.active_module();
        if self.state.ordered_document_ids().is_empty() {
            let mut navigation = accessibility_node(
                window,
                "rudra-office/navigation",
                AccessibilityRole::Tab,
                "Разделы",
                self.ui.view(cx, ids!(sidebar)).area().rect(cx),
            );
            navigation.selection = Some(AccessibilitySelection {
                can_select_multiple: false,
                is_selection_required: true,
            });
            for (module, widget_id) in [
                (Module::Home, ids!(home_button)),
                (Module::Writer, ids!(writer_button)),
                (Module::Sheets, ids!(sheets_button)),
                (Module::Pdf, ids!(pdf_button)),
                (Module::Slides, ids!(slides_button)),
            ] {
                navigation.children.push(accessibility_selectable(
                    window,
                    accessibility_module_node_id(module),
                    AccessibilityRole::TabItem,
                    module.title(),
                    self.ui.button(cx, widget_id).area().rect(cx),
                    module == active_module,
                ));
            }
            root.children.push(navigation);
        }

        let mut workspace = accessibility_node(
            window,
            "rudra-office/workspace",
            AccessibilityRole::Pane,
            "Рабочая область",
            self.ui.view(cx, ids!(workspace_panel)).area().rect(cx),
        );
        workspace.children.push(accessibility_node(
            window,
            "rudra-office/session/status",
            AccessibilityRole::StatusBar,
            format!("{} {}", self.state.status, self.state.detail),
            self.ui.label(cx, ids!(status_label)).area().rect(cx),
        ));
        if active_module == Module::Home {
            workspace.children.push(accessibility_button(
                window,
                "rudra-office/home/open-local",
                "Открыть локальный файл",
                self.ui.button(cx, ids!(home_open_button)).area().rect(cx),
                true,
            ));
            for (node_id, name, widget_id) in [
                (
                    "rudra-office/home/new-docx",
                    "Создать документ DOCX",
                    ids!(home_new_docx_button),
                ),
                (
                    "rudra-office/home/new-xlsx",
                    "Создать таблицу XLSX",
                    ids!(home_new_xlsx_button),
                ),
                (
                    "rudra-office/home/new-pptx",
                    "Создать презентацию PPTX",
                    ids!(home_new_pptx_button),
                ),
                (
                    "rudra-office/home/new-pdf",
                    "Создать пустой PDF",
                    ids!(home_new_pdf_button),
                ),
            ] {
                workspace.children.push(accessibility_button(
                    window,
                    node_id,
                    name,
                    self.ui.button(cx, widget_id).area().rect(cx),
                    self.state.open_job.is_none(),
                ));
            }
        }

        let tab_frames = self.document_tab_accessibility_frames(cx);
        if !tab_frames.is_empty() {
            let mut document_tabs = accessibility_node(
                window,
                "rudra-office/document-tabs",
                AccessibilityRole::Tab,
                "Открытые документы",
                self.ui.widget(cx, ids!(document_tabs)).area().rect(cx),
            );
            document_tabs.selection = Some(AccessibilitySelection {
                can_select_multiple: false,
                is_selection_required: true,
            });
            for tab in tab_frames {
                let mut tab_node = accessibility_selectable(
                    window,
                    accessibility_document_tab_node_id(tab.id),
                    AccessibilityRole::TabItem,
                    tab.label.clone(),
                    tab.bounds,
                    tab.active,
                );
                tab_node.children.push(accessibility_button(
                    window,
                    accessibility_document_close_node_id(tab.id),
                    format!("Закрыть {}", tab.label),
                    tab.close_bounds,
                    true,
                ));
                document_tabs.children.push(tab_node);
            }
            window_controls.children.push(document_tabs);
        }
        root.children.insert(0, window_controls);

        match active_module {
            Module::Sheets if self.state.should_show_sheets_view() => {
                let mut sheets = accessibility_node(
                    window,
                    "rudra-office/sheets",
                    AccessibilityRole::Document,
                    "Таблицы",
                    self.ui.view(cx, ids!(sheets_document_view)).area().rect(cx),
                );
                sheets.children.push(accessibility_button(
                    window,
                    "rudra-office/sheets/previous-sheet",
                    "Предыдущий лист",
                    self.ui.button(cx, ids!(sheets_prev_button)).area().rect(cx),
                    self.state.sheets_can_previous(),
                ));
                sheets.children.push(accessibility_button(
                    window,
                    "rudra-office/sheets/next-sheet",
                    "Следующий лист",
                    self.ui.button(cx, ids!(sheets_next_button)).area().rect(cx),
                    self.state.sheets_can_next(),
                ));
                if let (Some(document_id), Some(snapshot)) = (
                    self.state.active_document_id(),
                    self.sheets_accessibility_snapshot(cx),
                ) {
                    let scope = accessibility_document_scope(document_id);
                    let mut grid = accessibility_node(
                        window,
                        format!("{scope}/sheets/grid"),
                        AccessibilityRole::DataGrid,
                        "Таблица",
                        snapshot.bounds,
                    );
                    grid.actions = AccessibilityActions {
                        focus: true,
                        ..Default::default()
                    };
                    grid.focused =
                        cx.has_key_focus(self.ui.widget(cx, ids!(sheets_grid_canvas)).area());
                    grid.selection = Some(AccessibilitySelection {
                        can_select_multiple: true,
                        is_selection_required: true,
                    });
                    if let Some(view) = self.state.sheets_view.as_ref() {
                        let selection = view.selection();
                        let label = |address: CellAddress| {
                            format!("{}{}", column_label(address.column), address.row.0 + 1)
                        };
                        let mut range = accessibility_node(
                            window,
                            format!("{scope}/sheets/selection"),
                            AccessibilityRole::Text,
                            "Выделенный диапазон",
                            snapshot.bounds,
                        );
                        range.value = selection.range().map(|selected| {
                            format!("{}:{}", label(selected.start), label(selected.end))
                        });
                        grid.children.push(range);
                        for (name, address) in
                            [("anchor", selection.anchor), ("active", selection.focus)]
                        {
                            let mut node = accessibility_node(
                                window,
                                format!("{scope}/sheets/selection/{name}"),
                                AccessibilityRole::Text,
                                name,
                                snapshot.bounds,
                            );
                            node.value = address.map(label);
                            grid.children.push(node);
                        }
                        let mut merges = accessibility_node(
                            window,
                            format!("{scope}/sheets/merges"),
                            AccessibilityRole::Text,
                            "Объединённые диапазоны",
                            snapshot.bounds,
                        );
                        merges.value = self
                            .state
                            .sheets_snapshot()
                            .and_then(|book| book.sheets.get(self.state.sheets_selected))
                            .map(|sheet| {
                                sheet
                                    .merges
                                    .iter()
                                    .map(|range| {
                                        format!("{}:{}", label(range.start), label(range.end))
                                    })
                                    .collect::<Vec<_>>()
                                    .join(",")
                            });
                        grid.children.push(merges);
                        if let Some(api) = self.state.sheets_api.as_ref() {
                            let mut filter = accessibility_node(
                                window,
                                format!("{scope}/sheets/filter-hidden-rows"),
                                AccessibilityRole::Text,
                                "Строки, скрытые фильтром",
                                snapshot.bounds,
                            );
                            filter.value = Some(
                                api.filter_hidden_rows(api.document().active_sheet())
                                    .iter()
                                    .map(|row| (row.0 + 1).to_string())
                                    .collect::<Vec<_>>()
                                    .join(","),
                            );
                            grid.children.push(filter);
                        }
                    }
                    for cell in &snapshot.cells {
                        let mut node = accessibility_selectable(
                            window,
                            format!("{}/{}", scope, cell.id_suffix),
                            AccessibilityRole::DataItem,
                            cell.name.clone(),
                            cell.bounds,
                            cell.selected,
                        );
                        node.value = Some(cell.value.clone());
                        grid.children.push(node);
                    }
                    let mut sheet_tabs = accessibility_node(
                        window,
                        format!("{scope}/sheets/tabs"),
                        AccessibilityRole::Tab,
                        "Листы книги",
                        snapshot.bounds,
                    );
                    sheet_tabs.selection = Some(AccessibilitySelection {
                        can_select_multiple: false,
                        is_selection_required: true,
                    });
                    for tab in &snapshot.tabs {
                        sheet_tabs.children.push(accessibility_selectable(
                            window,
                            format!("{}/{}", scope, tab.id_suffix),
                            AccessibilityRole::TabItem,
                            tab.name.clone(),
                            tab.bounds,
                            tab.active,
                        ));
                    }
                    grid.children.push(sheet_tabs);
                    grid.children.push(accessibility_button(
                        window,
                        format!("{scope}/sheets/zoom-out"),
                        "Уменьшить масштаб таблицы",
                        snapshot.zoom_out_bounds,
                        true,
                    ));
                    grid.children.push(accessibility_button(
                        window,
                        format!("{scope}/sheets/zoom-in"),
                        "Увеличить масштаб таблицы",
                        snapshot.zoom_in_bounds,
                        true,
                    ));
                    sheets.children.push(grid);
                }
                workspace.children.push(sheets);
            }
            Module::Writer if self.state.should_show_writer_view() => {
                let mut writer = accessibility_node(
                    window,
                    "rudra-office/writer",
                    AccessibilityRole::Document,
                    "Документ Writer",
                    self.ui.view(cx, ids!(writer_document_view)).area().rect(cx),
                );
                for (node_id, name, widget_id) in [
                    (
                        "rudra-office/writer/pages",
                        "Постраничный режим",
                        ids!(writer_pages_button),
                    ),
                    (
                        "rudra-office/writer/continuous",
                        "Непрерывный режим",
                        ids!(writer_continuous_button),
                    ),
                    (
                        "rudra-office/writer/print-layout",
                        "Print layout",
                        ids!(writer_visual_button),
                    ),
                    (
                        "rudra-office/writer/plain-text",
                        "Plain text",
                        ids!(writer_plain_text_button),
                    ),
                    (
                        "rudra-office/writer/edit-text",
                        "Открыть редактор текста",
                        ids!(writer_edit_text_button),
                    ),
                    (
                        "rudra-office/writer/zoom-out",
                        "Уменьшить масштаб",
                        ids!(writer_zoom_out_button),
                    ),
                    (
                        "rudra-office/writer/zoom-in",
                        "Увеличить масштаб",
                        ids!(writer_zoom_in_button),
                    ),
                ] {
                    writer.children.push(accessibility_button(
                        window,
                        node_id,
                        name,
                        self.ui.button(cx, widget_id).area().rect(cx),
                        true,
                    ));
                }
                let zoom_widget = self.ui.widget(cx, ids!(writer_zoom_dropdown));
                let mut zoom = accessibility_node(
                    window,
                    "rudra-office/writer/zoom-preset",
                    AccessibilityRole::ComboBox,
                    "Масштаб Writer",
                    zoom_widget.area().rect(cx),
                );
                zoom.value = self
                    .ui
                    .widget(cx, ids!(writer_page_canvas))
                    .borrow::<VirtualCanvas>()
                    .map(|canvas| format!("{}%", canvas.zoom_percent()));
                zoom.focused = cx.has_key_focus(zoom_widget.area());
                zoom.actions = AccessibilityActions {
                    focus: true,
                    set_value: true,
                    ..Default::default()
                };
                writer.children.push(zoom);
                writer.children.push(accessibility_node(
                    window,
                    "rudra-office/writer/status",
                    AccessibilityRole::StatusBar,
                    format!(
                        "{}; масштаб {}",
                        self.ui.label(cx, ids!(writer_page_label)).text(),
                        self.ui.label(cx, ids!(writer_zoom_label)).text()
                    ),
                    self.ui.view(cx, ids!(writer_status_bar)).area().rect(cx),
                ));
                writer.children.push(accessibility_node(
                    window,
                    "rudra-office/writer/canvas",
                    AccessibilityRole::Document,
                    "Область документа Writer",
                    self.ui.widget(cx, ids!(writer_page_canvas)).area().rect(cx),
                ));
                if let (Some(document_id), Some(canvas)) = (
                    self.state.active_document_id(),
                    self.ui
                        .widget(cx, ids!(writer_page_canvas))
                        .borrow::<VirtualCanvas>(),
                ) {
                    let scope = accessibility_document_scope(document_id);
                    for paragraph in canvas
                        .paragraph_interaction_snapshots()
                        .into_iter()
                        .take(64)
                    {
                        let mut node = accessibility_node(
                            window,
                            format!("{scope}/writer/paragraph/{}", paragraph.interaction_id),
                            AccessibilityRole::Button,
                            format!(
                                "Редактировать абзац Writer на странице {}",
                                paragraph.page_index + 1
                            ),
                            Rect {
                                pos: dvec2(paragraph.bounds.origin.x, paragraph.bounds.origin.y),
                                size: dvec2(
                                    paragraph.bounds.size.width,
                                    paragraph.bounds.size.height,
                                ),
                            },
                        );
                        node.actions.invoke = true;
                        writer.children.push(node);
                    }
                    if self.writer_inline_editor_matches_active_document() {
                        let inline_widget = self.ui.text_input(cx, ids!(writer_inline_text_input));
                        let mut inline_input = accessibility_node(
                            window,
                            "rudra-office/writer/inline-text",
                            AccessibilityRole::Edit,
                            "Текст абзаца Writer на странице",
                            inline_widget.area().rect(cx),
                        );
                        inline_input.value = Some(inline_widget.text());
                        inline_input.focused = cx.has_key_focus(inline_widget.area());
                        inline_input.actions = AccessibilityActions {
                            focus: true,
                            set_value: true,
                            ..Default::default()
                        };
                        writer.children.push(inline_input);
                    }
                    for marker in canvas.ruler_marker_snapshots() {
                        let mut node = accessibility_node(
                            window,
                            format!("{scope}/writer/ruler/{}", marker.marker_id),
                            AccessibilityRole::Slider,
                            marker.label,
                            Rect {
                                pos: dvec2(marker.bounds.origin.x, marker.bounds.origin.y),
                                size: dvec2(marker.bounds.size.width, marker.bounds.size.height),
                            },
                        );
                        node.value = Some(format!("{:.3}", marker.logical_position));
                        node.enabled = marker.editable;
                        node.actions.set_value = marker.editable;
                        writer.children.push(node);
                    }
                }
                if let Some(OpenDocument::Writer(vm)) = self.state.document()
                    && let Some(notice) = writer_projection_notice(vm)
                {
                    writer.children.push(accessibility_node(
                        window,
                        "rudra-office/writer/projection-notice",
                        AccessibilityRole::Text,
                        notice,
                        self.ui
                            .label(cx, ids!(writer_projection_notice_label))
                            .area()
                            .rect(cx),
                    ));
                }
                workspace.children.push(writer);
            }
            Module::Pdf if self.state.should_show_pdf_viewer() => {
                let mut pdf = accessibility_node(
                    window,
                    "rudra-office/pdf",
                    AccessibilityRole::Document,
                    "PDF",
                    self.ui.view(cx, ids!(pdf_document_view)).area().rect(cx),
                );
                let controller = self.state.pdf_controller.state();
                for (node_id, name, widget_id, enabled) in [
                    (
                        "rudra-office/pdf/first",
                        "Первая страница",
                        ids!(pdf_first_button),
                        controller.can_previous(),
                    ),
                    (
                        "rudra-office/pdf/previous",
                        "Предыдущая страница",
                        ids!(pdf_prev_button),
                        controller.can_previous(),
                    ),
                    (
                        "rudra-office/pdf/next",
                        "Следующая страница",
                        ids!(pdf_next_button),
                        controller.can_next(),
                    ),
                    (
                        "rudra-office/pdf/last",
                        "Последняя страница",
                        ids!(pdf_last_button),
                        controller.can_next(),
                    ),
                    (
                        "rudra-office/pdf/zoom-out",
                        "Уменьшить масштаб",
                        ids!(pdf_zoom_out_button),
                        true,
                    ),
                    (
                        "rudra-office/pdf/zoom-in",
                        "Увеличить масштаб",
                        ids!(pdf_zoom_in_button),
                        true,
                    ),
                    (
                        "rudra-office/pdf/reset-zoom",
                        "Масштаб 100 процентов",
                        ids!(pdf_zoom_reset_button),
                        true,
                    ),
                    (
                        "rudra-office/pdf/fit-width",
                        "По ширине",
                        ids!(pdf_fit_width_button),
                        true,
                    ),
                    (
                        "rudra-office/pdf/fit-page",
                        "По странице",
                        ids!(pdf_fit_page_button),
                        true,
                    ),
                    (
                        "rudra-office/pdf/go-to-page",
                        "Перейти к странице",
                        ids!(pdf_go_button),
                        true,
                    ),
                ] {
                    pdf.children.push(accessibility_button(
                        window,
                        node_id,
                        name,
                        self.ui.button(cx, widget_id).area().rect(cx),
                        enabled,
                    ));
                }
                let page_input_widget = self.ui.text_input(cx, ids!(pdf_page_input));
                let mut page_input = accessibility_node(
                    window,
                    "rudra-office/pdf/page-input",
                    AccessibilityRole::Edit,
                    "Номер страницы PDF",
                    page_input_widget.area().rect(cx),
                );
                page_input.value = Some(page_input_widget.text());
                page_input.actions = AccessibilityActions {
                    focus: true,
                    set_value: true,
                    ..Default::default()
                };
                page_input.focused = cx.has_key_focus(page_input_widget.area());
                pdf.children.push(page_input);
                pdf.children.push(accessibility_node(
                    window,
                    "rudra-office/pdf/canvas",
                    AccessibilityRole::Document,
                    "Область PDF",
                    self.ui.widget(cx, ids!(pdf_page_canvas)).area().rect(cx),
                ));
                workspace.children.push(pdf);
            }
            Module::Slides if self.state.should_show_slides_viewer() => {
                let mut slides = accessibility_node(
                    window,
                    "rudra-office/slides",
                    AccessibilityRole::Document,
                    "Презентации",
                    self.ui.view(cx, ids!(slides_document_view)).area().rect(cx),
                );
                slides.children.push(accessibility_node(
                    window,
                    "rudra-office/slides/toolbar",
                    AccessibilityRole::Document,
                    "Панель презентации",
                    self.ui.view(cx, ids!(slides_toolbar)).area().rect(cx),
                ));
                slides.children.push(accessibility_node(
                    window,
                    "rudra-office/slides/content",
                    AccessibilityRole::Document,
                    "Контейнер презентации",
                    self.ui.view(cx, ids!(slides_content_view)).area().rect(cx),
                ));
                let slides_snapshot = self.state.slides_snapshot();
                let (slides_can_previous, slides_can_next) =
                    slides_navigation_enabled(slides_snapshot);

                for (node_id, name, widget_id) in [
                    (
                        "rudra-office/slides/previous",
                        "Предыдущий слайд",
                        ids!(slides_prev_button),
                    ),
                    (
                        "rudra-office/slides/next",
                        "Следующий слайд",
                        ids!(slides_next_button),
                    ),
                    (
                        "rudra-office/slides/zoom-out",
                        "Уменьшить масштаб",
                        ids!(slides_zoom_out_button),
                    ),
                    (
                        "rudra-office/slides/zoom-in",
                        "Увеличить масштаб",
                        ids!(slides_zoom_in_button),
                    ),
                    (
                        "rudra-office/slides/reset-zoom",
                        "Масштаб 100 процентов",
                        ids!(slides_zoom_reset_button),
                    ),
                    (
                        "rudra-office/slides/fit",
                        "Вписать слайд",
                        ids!(slides_fit_button),
                    ),
                    (
                        "rudra-office/slides/presentation",
                        "Режим показа",
                        ids!(slides_present_button),
                    ),
                ] {
                    let enabled = match node_id {
                        "rudra-office/slides/previous" => slides_can_previous,
                        "rudra-office/slides/next" => slides_can_next,
                        _ => true,
                    };
                    slides.children.push(accessibility_button(
                        window,
                        node_id,
                        name,
                        self.ui.button(cx, widget_id).area().rect(cx),
                        enabled,
                    ));
                }
                let canvas_bounds = self
                    .ui
                    .widget(cx, ids!(slides_visual_canvas))
                    .area()
                    .rect(cx);
                slides.children.push(accessibility_node(
                    window,
                    "rudra-office/slides/canvas",
                    AccessibilityRole::Document,
                    "Область презентации",
                    canvas_bounds,
                ));
                if let (Some(document_id), Some(snapshot)) = (
                    self.state.active_document_id(),
                    self.slides_accessibility_snapshot(cx),
                ) {
                    let scope = accessibility_document_scope(document_id);
                    let draw_state = if snapshot.draw_called {
                        "фактический draw выполнен"
                    } else {
                        "ожидается первый draw"
                    };
                    slides.children.push(accessibility_node(
                        window,
                        format!("{scope}/slides/render-status"),
                        AccessibilityRole::StatusBar,
                        format!("{}; {draw_state}", snapshot.summary),
                        canvas_bounds,
                    ));
                    for item in snapshot.items {
                        let role = match item.kind {
                            SlidesAccessibilityKind::Text => AccessibilityRole::Text,
                            SlidesAccessibilityKind::Image => AccessibilityRole::Image,
                            SlidesAccessibilityKind::Graphic => AccessibilityRole::Custom,
                        };
                        slides.children.push(accessibility_node(
                            window,
                            format!("{scope}/slides/render/{}", item.command_index),
                            role,
                            item.name,
                            item.bounds,
                        ));
                    }
                }
                workspace.children.push(slides);
            }
            _ => {
                workspace.children.push(accessibility_node(
                    window,
                    "rudra-office/overview",
                    AccessibilityRole::StatusBar,
                    self.state.status.clone(),
                    self.ui.label(cx, ids!(status_label)).area().rect(cx),
                ));
            }
        }

        root.children.push(workspace);
        AccessibilityTree::new(root).map_err(|error| error.to_string())
    }

    #[cfg(windows)]
    pub(crate) fn handle_accessibility_action(
        &mut self,
        cx: &mut Cx,
        request: AccessibilityActionRequest,
    ) {
        let mut handled = false;
        match request.action {
            AccessibilityAction::Focus => {
                if request.node_id == "rudra-office/writer/inline-text"
                    && self.writer_inline_editor_matches_active_document()
                {
                    cx.set_key_focus(
                        self.ui
                            .text_input(cx, ids!(writer_inline_text_input))
                            .area(),
                    );
                    handled = true;
                } else if request.node_id == "rudra-office/pdf/page-input" {
                    cx.set_key_focus(self.ui.text_input(cx, ids!(pdf_page_input)).area());
                    handled = true;
                } else if request.node_id == "rudra-office/home/writer/font-family" {
                    cx.set_key_focus(self.ui.text_input(cx, ids!(writer_font_input)).area());
                    handled = true;
                } else if request.node_id == "rudra-office/home/writer/font-size" {
                    cx.set_key_focus(self.ui.text_input(cx, ids!(writer_size_input)).area());
                    handled = true;
                } else if request.node_id == "rudra-office/home/writer/text"
                    && self.state.writer_text_editor_available()
                {
                    cx.set_key_focus(self.ui.text_input(cx, ids!(writer_text_input)).area());
                    handled = true;
                } else if request.node_id == "rudra-office/home/writer/paragraph"
                    && self.state.writer_text_editor_available()
                {
                    cx.set_key_focus(self.ui.widget(cx, ids!(writer_text_block_dropdown)).area());
                    handled = true;
                } else if request.node_id == "rudra-office/writer/zoom-preset"
                    && self.state.should_show_writer_view()
                {
                    cx.set_key_focus(self.ui.widget(cx, ids!(writer_zoom_dropdown)).area());
                    handled = true;
                } else if request.node_id == "rudra-office/home/writer/color"
                    && self.state.writer_character_toolbar_available()
                {
                    cx.set_key_focus(self.ui.widget(cx, ids!(writer_color_dropdown)).area());
                    handled = true;
                } else if request.node_id == "rudra-office/home/sheets/cell-value"
                    && self.state.sheets_editing_available()
                {
                    cx.set_key_focus(self.ui.text_input(cx, ids!(sheets_cell_input)).area());
                    handled = true;
                } else if self.state.sheets_tools_available()
                    && [
                        "rudra-office/sheets/address-input",
                        "rudra-office/sheets/search-input",
                        "rudra-office/sheets/key-column",
                        "rudra-office/sheets/filter-value",
                    ]
                    .contains(&request.node_id.as_str())
                {
                    let widget_id = match request.node_id.as_str() {
                        "rudra-office/sheets/address-input" => ids!(sheets_address_input),
                        "rudra-office/sheets/key-column" => ids!(sheets_key_column_input),
                        "rudra-office/sheets/filter-value" => ids!(sheets_filter_value_input),
                        _ => ids!(sheets_search_input),
                    };
                    cx.set_key_focus(self.ui.text_input(cx, widget_id).area());
                    handled = true;
                } else if self.state.sheets_tools_available()
                    && [
                        "rudra-office/sheets/has-header",
                        "rudra-office/sheets/filter-kind",
                    ]
                    .contains(&request.node_id.as_str())
                {
                    let widget_id = if request.node_id.ends_with("has-header") {
                        ids!(sheets_header_dropdown)
                    } else {
                        ids!(sheets_filter_kind_dropdown)
                    };
                    cx.set_key_focus(self.ui.widget(cx, widget_id).area());
                    handled = true;
                } else if request.node_id == "rudra-office/home/sheets/color"
                    && self.state.sheets_editing_available()
                {
                    cx.set_key_focus(self.ui.widget(cx, ids!(sheets_color_dropdown)).area());
                    handled = true;
                } else if request.node_id == "rudra-office/home/slides/text-color"
                    && self.state.slides_text_color_available()
                {
                    cx.set_key_focus(self.ui.widget(cx, ids!(slides_text_color_dropdown)).area());
                    handled = true;
                } else if self.state.active_document_id().is_some_and(|id| {
                    request.node_id == format!("{}/sheets/grid", accessibility_document_scope(id))
                }) {
                    cx.set_key_focus(self.ui.widget(cx, ids!(sheets_grid_canvas)).area());
                    handled = true;
                }
            }
            AccessibilityAction::SetValue(value) => {
                if request.node_id == "rudra-office/action/open" {
                    let value = value.trim();
                    self.state.pending_open_path =
                        (!value.is_empty()).then(|| PathBuf::from(value));
                    handled = true;
                } else if request.node_id == "rudra-office/writer/inline-text"
                    && self.writer_inline_editor_matches_active_document()
                {
                    self.ui
                        .text_input(cx, ids!(writer_inline_text_input))
                        .set_text(cx, &value);
                    self.state.update_writer_text_editor_draft(value);
                    handled = true;
                } else if request.node_id == "rudra-office/pdf/page-input"
                    && self.state.should_show_pdf_viewer()
                {
                    self.ui
                        .text_input(cx, ids!(pdf_page_input))
                        .set_text(cx, &value);
                    handled = true;
                } else if request.node_id == "rudra-office/home/writer/font-family"
                    && self.state.should_show_writer_view()
                {
                    self.ui
                        .text_input(cx, ids!(writer_font_input))
                        .set_text(cx, &value);
                    handled = true;
                } else if request.node_id == "rudra-office/home/writer/font-size"
                    && self.state.should_show_writer_view()
                {
                    self.ui
                        .text_input(cx, ids!(writer_size_input))
                        .set_text(cx, &value);
                    handled = true;
                } else if request.node_id == "rudra-office/home/writer/text"
                    && self.state.writer_text_editor_available()
                {
                    self.ui
                        .text_input(cx, ids!(writer_text_input))
                        .set_text(cx, &value);
                    self.state.update_writer_text_editor_draft(value);
                    handled = true;
                } else if request.node_id == "rudra-office/home/writer/paragraph"
                    && self.state.writer_text_editor_available()
                {
                    let result = value
                        .trim()
                        .parse::<usize>()
                        .map_err(|_| "Номер абзаца должен быть целым числом.".to_owned())
                        .and_then(|index| self.state.select_writer_text_editor_block(index));
                    self.finish_home_command(cx, result);
                    handled = true;
                } else if request.node_id == "rudra-office/writer/zoom-preset"
                    && self.state.should_show_writer_view()
                {
                    let result = writer_zoom_percent_from_value(&value)
                        .and_then(|percent| self.set_writer_canvas_zoom_percent(cx, percent));
                    self.finish_home_command(cx, result);
                    handled = true;
                } else if request.node_id == "rudra-office/home/sheets/cell-value"
                    && self.state.sheets_editing_available()
                {
                    self.ui
                        .text_input(cx, ids!(sheets_cell_input))
                        .set_text(cx, &value);
                    handled = true;
                } else if self.state.sheets_tools_available()
                    && [
                        "rudra-office/sheets/address-input",
                        "rudra-office/sheets/search-input",
                        "rudra-office/sheets/key-column",
                        "rudra-office/sheets/filter-value",
                    ]
                    .contains(&request.node_id.as_str())
                {
                    if request.node_id.ends_with("address-input") {
                        self.ui
                            .text_input(cx, ids!(sheets_address_input))
                            .set_text(cx, &value);
                        self.state.update_sheets_tools_draft(Some(value), None);
                    } else if request.node_id.ends_with("key-column") {
                        self.ui
                            .text_input(cx, ids!(sheets_key_column_input))
                            .set_text(cx, &value);
                        self.state
                            .update_sheets_data_draft(Some(value), None, None, None);
                    } else if request.node_id.ends_with("filter-value") {
                        self.ui
                            .text_input(cx, ids!(sheets_filter_value_input))
                            .set_text(cx, &value);
                        self.state
                            .update_sheets_data_draft(None, None, None, Some(value));
                    } else {
                        self.ui
                            .text_input(cx, ids!(sheets_search_input))
                            .set_text(cx, &value);
                        self.state.update_sheets_tools_draft(None, Some(value));
                    }
                    handled = true;
                } else if self.state.sheets_tools_available()
                    && request.node_id == "rudra-office/sheets/has-header"
                {
                    let result = match value.trim() {
                        "true" | "1" | "Есть заголовок" => Ok(true),
                        "false" | "0" | "Без заголовка" => Ok(false),
                        _ => Err("Заголовок: укажите true или false.".to_owned()),
                    }
                    .map(|has_header| {
                        self.state
                            .update_sheets_data_draft(None, Some(has_header), None, None);
                        self.ui
                            .drop_down(cx, ids!(sheets_header_dropdown))
                            .set_selected_item(cx, usize::from(has_header));
                    });
                    if let Err(error) = result {
                        self.state.status = error;
                    }
                    handled = true;
                } else if self.state.sheets_tools_available()
                    && request.node_id == "rudra-office/sheets/filter-kind"
                {
                    let kind = value
                        .trim()
                        .parse::<usize>()
                        .ok()
                        .filter(|kind| *kind < sheets_tools::FILTER_LABELS.len())
                        .or_else(|| {
                            sheets_tools::FILTER_LABELS
                                .iter()
                                .position(|label| *label == value.trim())
                        });
                    if let Some(kind) = kind {
                        self.state
                            .update_sheets_data_draft(None, None, Some(kind), None);
                        self.ui
                            .drop_down(cx, ids!(sheets_filter_kind_dropdown))
                            .set_selected_item(cx, kind);
                    } else {
                        self.state.status = "Неизвестное условие фильтра.".into();
                    }
                    handled = true;
                } else if request.node_id == "rudra-office/home/writer/color"
                    && self.state.writer_character_toolbar_available()
                {
                    let value = value.trim();
                    let node_id = if value.starts_with("rudra-office/") {
                        value.to_owned()
                    } else {
                        format!("rudra-office/home/writer/{}", value.trim_start_matches('/'))
                    };
                    let result = writer_color_command_from_accessibility_node(&node_id)
                        .ok_or_else(|| format!("Неизвестный цвет Writer: {value}."))
                        .and_then(|command| self.apply_writer_command_from_ui(cx, command));
                    self.finish_home_command(cx, result);
                    handled = true;
                } else if request.node_id == "rudra-office/home/sheets/color"
                    && self.state.sheets_editing_available()
                {
                    let value = value.trim();
                    let node_id = if value.starts_with("rudra-office/") {
                        value.to_owned()
                    } else {
                        format!("rudra-office/home/sheets/{}", value.trim_start_matches('/'))
                    };
                    let result = sheets_color_command_from_accessibility_node(&node_id)
                        .ok_or_else(|| format!("Неизвестный цвет Sheets: {value}."))
                        .and_then(|command| self.state.apply_sheets_home_format(command));
                    self.finish_home_command(cx, result);
                    handled = true;
                } else if request.node_id == "rudra-office/home/slides/text-color"
                    && self.state.slides_text_color_available()
                {
                    let value = value.trim();
                    let node_id = if value.starts_with("rudra-office/") {
                        value.to_owned()
                    } else {
                        format!("rudra-office/home/slides/{}", value.trim_start_matches('/'))
                    };
                    let result = slides_color_command_from_accessibility_node(&node_id)
                        .ok_or_else(|| format!("Неизвестный цвет Slides: {value}."))
                        .and_then(|command| self.state.apply_slides_home_command(command));
                    self.finish_home_command(cx, result);
                    handled = true;
                } else if self.state.should_show_writer_view()
                    && let Some(document_id) = self.state.active_document_id()
                    && let Some(marker_id) = request.node_id.strip_prefix(&format!(
                        "{}/writer/ruler/",
                        accessibility_document_scope(document_id)
                    ))
                {
                    let result = value
                        .trim()
                        .parse::<f64>()
                        .map_err(|_| "Координата маркера должна быть числом.".to_owned())
                        .and_then(|position| {
                            self.state.apply_writer_ruler_change(marker_id, position)
                        });
                    self.finish_home_command(cx, result);
                    handled = true;
                }
            }
            AccessibilityAction::Select => {
                if let Some(module) = accessibility_module_from_node_id(&request.node_id) {
                    self.select_module(cx, module);
                    handled = true;
                } else if let Some(tab) = self
                    .document_tab_accessibility_frames(cx)
                    .into_iter()
                    .find(|tab| accessibility_document_tab_node_id(tab.id) == request.node_id)
                {
                    if let Err(error) = self.activate_document(cx, tab.id) {
                        self.state.status = "Document tab activation failed.".to_owned();
                        self.state.detail = error.to_string();
                        self.refresh_ui(cx);
                    }
                    handled = true;
                } else if self.state.should_show_sheets_view()
                    && let (Some(document_id), Some(snapshot)) = (
                        self.state.active_document_id(),
                        self.sheets_accessibility_snapshot(cx),
                    )
                {
                    let scope = accessibility_document_scope(document_id);
                    if let Some(cell) = snapshot
                        .cells
                        .iter()
                        .find(|cell| request.node_id == format!("{}/{}", scope, cell.id_suffix))
                    {
                        self.state.sheets_select_cell(cell.address);
                        self.refresh_ui(cx);
                        handled = true;
                    } else if let Some(tab) = snapshot
                        .tabs
                        .iter()
                        .find(|tab| request.node_id == format!("{}/{}", scope, tab.id_suffix))
                    {
                        self.state.select_sheet_id(tab.sheet);
                        self.refresh_ui(cx);
                        handled = true;
                    }
                }
            }
            AccessibilityAction::Invoke => {
                handled = match request.node_id.as_str() {
                    "rudra-office/home/sheets/copy" => {
                        self.dispatch_sheets_clipboard(cx, SheetsClipboardCommand::Copy);
                        true
                    }
                    "rudra-office/home/sheets/cut" => {
                        self.dispatch_sheets_clipboard(cx, SheetsClipboardCommand::Cut);
                        true
                    }
                    "rudra-office/home/sheets/paste" => {
                        self.dispatch_sheets_clipboard(cx, SheetsClipboardCommand::Paste);
                        true
                    }
                    "rudra-office/window/minimize" => {
                        minimize_office_window(cx, &self.ui);
                        true
                    }
                    "rudra-office/window/maximize" => {
                        toggle_office_window_maximized(cx, &self.ui);
                        true
                    }
                    "rudra-office/window/close" => {
                        cx.quit();
                        true
                    }
                    "rudra-office/menu/file-trigger" => {
                        self.toggle_file_menu(cx);
                        true
                    }
                    "rudra-office/menu/home-trigger" => {
                        self.show_home_ribbon(cx);
                        true
                    }
                    node_id
                        if node_id.starts_with("rudra-office/menu/")
                            && ribbon_tab_widgets().iter().any(|(tab, _)| {
                                node_id == format!("rudra-office/menu/{}-trigger", tab.id())
                            }) =>
                    {
                        let tab = ribbon_tab_widgets()
                            .into_iter()
                            .find(|(tab, _)| {
                                node_id == format!("rudra-office/menu/{}-trigger", tab.id())
                            })
                            .unwrap()
                            .0;
                        self.show_ribbon_tab(cx, tab);
                        true
                    }
                    node_id if node_id.starts_with("rudra-office/ribbon/") => {
                        let command =
                            commands_for(self.state.active_module(), self.state.ribbon_tab)
                                .iter()
                                .find(|(command, _)| {
                                    node_id == format!("rudra-office/ribbon/{}", command.id())
                                })
                                .map(|&(command, _)| command);
                        if let Some(command) = command {
                            self.dispatch_ribbon_command(cx, command);
                            true
                        } else {
                            false
                        }
                    }
                    "rudra-office/action/open" => {
                        self.open_document(cx);
                        true
                    }
                    "rudra-office/home/open-local" => {
                        self.open_document(cx);
                        true
                    }
                    "rudra-office/home/new-docx" => {
                        self.create_new_document(cx, BlankDocumentFormat::Docx);
                        true
                    }
                    "rudra-office/home/new-xlsx" => {
                        self.create_new_document(cx, BlankDocumentFormat::Xlsx);
                        true
                    }
                    "rudra-office/home/new-pptx" => {
                        self.create_new_document(cx, BlankDocumentFormat::Pptx);
                        true
                    }
                    "rudra-office/home/new-pdf" => {
                        self.create_new_document(cx, BlankDocumentFormat::Pdf);
                        true
                    }
                    "rudra-office/action/save" => {
                        self.save_document(cx);
                        true
                    }
                    "rudra-office/action/probe-engines" => {
                        self.probe_engines(cx);
                        true
                    }
                    node_id
                        if self.state.should_show_writer_view()
                            && self.state.active_document_id().is_some_and(|document_id| {
                                node_id.starts_with(&format!(
                                    "{}/writer/paragraph/",
                                    accessibility_document_scope(document_id)
                                ))
                            }) =>
                    {
                        let document_id = self
                            .state
                            .active_document_id()
                            .expect("Writer paragraph action has an active document");
                        let prefix = format!(
                            "{}/writer/paragraph/",
                            accessibility_document_scope(document_id)
                        );
                        let interaction_id = node_id
                            .strip_prefix(&prefix)
                            .expect("guard resolved Writer paragraph node");
                        let paragraph =
                            self.ui
                                .widget(cx, ids!(writer_page_canvas))
                                .borrow::<VirtualCanvas>()
                                .and_then(|canvas| {
                                    canvas.paragraph_interaction_snapshots().into_iter().find(
                                        |paragraph| paragraph.interaction_id == interaction_id,
                                    )
                                });
                        let result = paragraph
                            .ok_or_else(|| {
                                "Выбранный абзац Writer больше не виден на странице.".to_owned()
                            })
                            .and_then(|paragraph| {
                                self.begin_writer_inline_editor(
                                    cx,
                                    &paragraph.interaction_id,
                                    paragraph.bounds,
                                    0,
                                )
                            });
                        self.finish_home_command(cx, result);
                        true
                    }
                    node_id
                        if self.state.should_show_writer_view()
                            && writer_color_command_from_accessibility_node(node_id).is_some() =>
                    {
                        let command = writer_color_command_from_accessibility_node(node_id)
                            .expect("guard resolved Writer color command");
                        let result = self.apply_writer_command_from_ui(cx, command);
                        self.finish_home_command(cx, result);
                        true
                    }
                    node_id
                        if self.state.sheets_editing_available()
                            && sheets_color_command_from_accessibility_node(node_id).is_some() =>
                    {
                        let command = sheets_color_command_from_accessibility_node(node_id)
                            .expect("guard resolved Sheets color command");
                        let result = self.state.apply_sheets_home_format(command);
                        self.finish_home_command(cx, result);
                        true
                    }
                    node_id
                        if self.state.slides_text_color_available()
                            && slides_color_command_from_accessibility_node(node_id).is_some() =>
                    {
                        let command = slides_color_command_from_accessibility_node(node_id)
                            .expect("guard resolved Slides color command");
                        let result = self.state.apply_slides_home_command(command);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/writer/undo" if self.state.should_show_writer_view() => {
                        self.dispatch_active_history_command(cx, OfficeHistoryCommand::Undo);
                        true
                    }
                    "rudra-office/home/writer/redo" if self.state.should_show_writer_view() => {
                        self.dispatch_active_history_command(cx, OfficeHistoryCommand::Redo);
                        true
                    }
                    "rudra-office/home/writer/font-apply"
                        if self.state.should_show_writer_view() =>
                    {
                        let result = self.apply_writer_font_controls(cx);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/writer/text-apply"
                        if self.state.writer_text_editor_available() =>
                    {
                        let raw = self.ui.text_input(cx, ids!(writer_text_input)).text();
                        let result = self.state.apply_writer_text_editor(raw);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/writer/text-revert"
                        if self.state.writer_text_editor_available() =>
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
                        true
                    }
                    "rudra-office/home/writer/bold" if self.state.should_show_writer_view() => {
                        let result =
                            self.apply_writer_command_from_ui(cx, WriterHomeCommand::ToggleBold);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/writer/italic" if self.state.should_show_writer_view() => {
                        let result =
                            self.apply_writer_command_from_ui(cx, WriterHomeCommand::ToggleItalic);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/writer/underline"
                        if self.state.should_show_writer_view() =>
                    {
                        let result = self
                            .apply_writer_command_from_ui(cx, WriterHomeCommand::ToggleUnderline);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/writer/strike" if self.state.should_show_writer_view() => {
                        let result = self.apply_writer_command_from_ui(
                            cx,
                            WriterHomeCommand::ToggleStrikethrough,
                        );
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/writer/align-left"
                        if self.state.should_show_writer_view() =>
                    {
                        let result = self.apply_writer_command_from_ui(
                            cx,
                            WriterHomeCommand::SetAlignment(WriterAlignment::Left),
                        );
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/writer/align-center"
                        if self.state.should_show_writer_view() =>
                    {
                        let result = self.apply_writer_command_from_ui(
                            cx,
                            WriterHomeCommand::SetAlignment(WriterAlignment::Center),
                        );
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/writer/align-right"
                        if self.state.should_show_writer_view() =>
                    {
                        let result = self.apply_writer_command_from_ui(
                            cx,
                            WriterHomeCommand::SetAlignment(WriterAlignment::Right),
                        );
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/writer/align-justify"
                        if self.state.should_show_writer_view() =>
                    {
                        let result = self.apply_writer_command_from_ui(
                            cx,
                            WriterHomeCommand::SetAlignment(WriterAlignment::Justified),
                        );
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/writer/style-normal"
                        if self.state.should_show_writer_view() =>
                    {
                        let result = self
                            .apply_writer_command_from_ui(cx, WriterHomeCommand::SetHeading(None));
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/writer/style-h1" if self.state.should_show_writer_view() => {
                        let result = self.apply_writer_command_from_ui(
                            cx,
                            WriterHomeCommand::SetHeading(Some(1)),
                        );
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/writer/style-h2" if self.state.should_show_writer_view() => {
                        let result = self.apply_writer_command_from_ui(
                            cx,
                            WriterHomeCommand::SetHeading(Some(2)),
                        );
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/sheets/cell-apply"
                        if self.state.sheets_editing_available() =>
                    {
                        let raw = self.ui.text_input(cx, ids!(sheets_cell_input)).text();
                        let result = self.state.apply_sheets_cell_input(raw);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/sheets/cell-clear"
                        if self.state.sheets_editing_available() =>
                    {
                        let result = self.state.apply_ribbon_command(RibbonCommand::ClearCell);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/sheets/bold" if self.state.sheets_editing_available() => {
                        let result = self
                            .state
                            .apply_sheets_home_format(SheetsHomeFormat::ToggleBold);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/sheets/italic" if self.state.sheets_editing_available() => {
                        let result = self
                            .state
                            .apply_sheets_home_format(SheetsHomeFormat::ToggleItalic);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/sheets/underline"
                        if self.state.sheets_editing_available() =>
                    {
                        let result = self
                            .state
                            .apply_sheets_home_format(SheetsHomeFormat::ToggleUnderline);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/sheets/align-left"
                        if self.state.sheets_editing_available() =>
                    {
                        let result = self
                            .state
                            .apply_sheets_home_format(SheetsHomeFormat::AlignLeft);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/sheets/align-center"
                        if self.state.sheets_editing_available() =>
                    {
                        let result = self
                            .state
                            .apply_sheets_home_format(SheetsHomeFormat::AlignCenter);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/sheets/align-right"
                        if self.state.sheets_editing_available() =>
                    {
                        let result = self
                            .state
                            .apply_sheets_home_format(SheetsHomeFormat::AlignRight);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/sheets/wrap" if self.state.sheets_editing_available() => {
                        let result = self
                            .state
                            .apply_sheets_home_format(SheetsHomeFormat::ToggleWrap);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/sheets/undo" if self.state.sheets_can_undo() => {
                        self.dispatch_active_history_command(cx, OfficeHistoryCommand::Undo);
                        true
                    }
                    "rudra-office/home/sheets/redo" if self.state.sheets_can_redo() => {
                        self.dispatch_active_history_command(cx, OfficeHistoryCommand::Redo);
                        true
                    }
                    "rudra-office/home/slides/new" if self.state.should_show_slides_viewer() => {
                        let result = self.state.apply_slides_home_command(SlidesHomeCommand::Add);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/slides/duplicate"
                        if self.state.should_show_slides_viewer() =>
                    {
                        let result = self
                            .state
                            .apply_slides_home_command(SlidesHomeCommand::Duplicate);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/home/slides/delete" if self.state.should_show_slides_viewer() => {
                        let result = self
                            .state
                            .apply_slides_home_command(SlidesHomeCommand::Delete);
                        self.finish_home_command(cx, result);
                        true
                    }
                    "rudra-office/sheets/previous-sheet"
                        if self.state.should_show_sheets_view() =>
                    {
                        self.state.sheets_previous();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/sheets/next-sheet" if self.state.should_show_sheets_view() => {
                        self.state.sheets_next();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/writer/pages" if self.state.should_show_writer_view() => {
                        self.state.apply_writer_view_mode(ViewMode::Pages);
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/writer/continuous" if self.state.should_show_writer_view() => {
                        self.state.apply_writer_view_mode(ViewMode::Continuous);
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/writer/print-layout" if self.state.should_show_writer_view() => {
                        self.state
                            .set_writer_display_mode(WriterDisplayMode::Visual);
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/writer/plain-text" if self.state.should_show_writer_view() => {
                        self.state
                            .set_writer_display_mode(WriterDisplayMode::PlainText);
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/writer/edit-text" if self.state.should_show_writer_view() => {
                        self.show_home_ribbon(cx);
                        cx.set_key_focus(self.ui.text_input(cx, ids!(writer_text_input)).area());
                        true
                    }
                    "rudra-office/writer/zoom-in" if self.state.should_show_writer_view() => {
                        if let Some(mut canvas) = self
                            .ui
                            .widget(cx, ids!(writer_page_canvas))
                            .borrow_mut::<VirtualCanvas>()
                        {
                            canvas.zoom_in(cx);
                        }
                        self.sync_writer_runtime_view(cx);
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/writer/zoom-out" if self.state.should_show_writer_view() => {
                        if let Some(mut canvas) = self
                            .ui
                            .widget(cx, ids!(writer_page_canvas))
                            .borrow_mut::<VirtualCanvas>()
                        {
                            canvas.zoom_out(cx);
                        }
                        self.sync_writer_runtime_view(cx);
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/pdf/first" if self.state.should_show_pdf_viewer() => {
                        self.state.pdf_first();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/pdf/previous" if self.state.should_show_pdf_viewer() => {
                        self.state.pdf_previous();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/pdf/next" if self.state.should_show_pdf_viewer() => {
                        self.state.pdf_next();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/pdf/last" if self.state.should_show_pdf_viewer() => {
                        self.state.pdf_last();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/pdf/zoom-in" if self.state.should_show_pdf_viewer() => {
                        self.state.pdf_zoom_in();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/pdf/zoom-out" if self.state.should_show_pdf_viewer() => {
                        self.state.pdf_zoom_out();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/pdf/reset-zoom" if self.state.should_show_pdf_viewer() => {
                        self.state.pdf_reset_zoom();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/pdf/fit-width" if self.state.should_show_pdf_viewer() => {
                        self.state.pdf_fit_width();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/pdf/fit-page" if self.state.should_show_pdf_viewer() => {
                        self.state.pdf_fit_page();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/pdf/go-to-page" if self.state.should_show_pdf_viewer() => {
                        let page = self
                            .ui
                            .text_input(cx, ids!(pdf_page_input))
                            .text()
                            .trim()
                            .parse::<u32>()
                            .unwrap_or(0);
                        self.state.pdf_go_to_page(page);
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/slides/previous" if self.state.should_show_slides_viewer() => {
                        self.state.slides_previous();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/slides/next" if self.state.should_show_slides_viewer() => {
                        self.state.slides_next();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/slides/zoom-in" if self.state.should_show_slides_viewer() => {
                        self.state.slides_zoom_in();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/slides/zoom-out" if self.state.should_show_slides_viewer() => {
                        self.state.slides_zoom_out();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/slides/reset-zoom" if self.state.should_show_slides_viewer() => {
                        self.state.slides_reset();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/slides/fit" if self.state.should_show_slides_viewer() => {
                        self.state.slides_fit();
                        self.refresh_ui(cx);
                        true
                    }
                    "rudra-office/slides/presentation"
                        if self.state.should_show_slides_viewer() =>
                    {
                        self.state.slides_toggle_presentation();
                        self.refresh_ui(cx);
                        true
                    }
                    _ => false,
                };

                if !handled {
                    if let Some(tab) = self
                        .document_tab_accessibility_frames(cx)
                        .into_iter()
                        .find(|tab| accessibility_document_close_node_id(tab.id) == request.node_id)
                    {
                        if let Err(error) = self.close_document(cx, tab.id) {
                            self.state.status = "Document tab close failed.".to_owned();
                            self.state.detail = error.to_string();
                            self.refresh_ui(cx);
                        }
                        handled = true;
                    } else if self.state.should_show_sheets_view()
                        && let (Some(document_id), Some(_snapshot)) = (
                            self.state.active_document_id(),
                            self.sheets_accessibility_snapshot(cx),
                        )
                    {
                        let scope = accessibility_document_scope(document_id);
                        if request.node_id == format!("{scope}/sheets/zoom-in") {
                            self.state.sheets_zoom_in();
                            self.refresh_ui(cx);
                            handled = true;
                        } else if request.node_id == format!("{scope}/sheets/zoom-out") {
                            self.state.sheets_zoom_out();
                            self.refresh_ui(cx);
                            handled = true;
                        }
                    }
                }
            }
            AccessibilityAction::AddToSelection | AccessibilityAction::RemoveFromSelection => {}
        }
        if handled {
            self.refresh_accessibility(cx);
        }
    }
}
