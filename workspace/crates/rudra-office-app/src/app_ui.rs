//! The original unified Splash UI, with the same widget IDs and registration order.
use super::*;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.VirtualCanvas
    mod.widgets.SheetsGridCanvasBase = #(SheetsGridCanvas::register_widget(vm))
    mod.widgets.SheetsGridCanvas = set_type_default() do mod.widgets.SheetsGridCanvasBase {
        draw_text +: {
            text_style: theme.font_regular
        }
        draw_text_bold +: {
            text_style: theme.font_bold
        }
        draw_text_italic +: {
            text_style: theme.font_italic
        }
        draw_text_bold_italic +: {
            text_style: theme.font_bold_italic
        }
    }
    mod.widgets.PdfPageCanvas = #(PdfPageCanvas::register_widget(vm))
    mod.widgets.SlidesVisualCanvasBase = #(SlidesVisualCanvas::register_widget(vm))
    // The PPTX canvas owns four DrawText instances. They must receive the
    // same initialized font resources as the other document canvases; without
    // this default, render commands are issued but glyphs have no visible font.
    mod.widgets.SlidesVisualCanvas = set_type_default() do mod.widgets.SlidesVisualCanvasBase {
        draw_text +: {
            text_style: theme.font_regular
        }
        draw_text_bold +: {
            text_style: theme.font_bold
        }
        draw_text_italic +: {
            text_style: theme.font_italic
        }
        draw_text_bold_italic +: {
            text_style: theme.font_bold_italic
        }
    }
    mod.widgets.DocumentTabStrip = #(DocumentTabStrip::register_widget(vm))
    mod.widgets.RudraDocumentTabBar = mod.widgets.TabBarFlat {
        height: 34
        draw_bg +: {
            color: #xf6f7f9
        }
        CloseableTab := mod.widgets.TabFlat {
            height: 34
            padding: Inset{left: 9 top: 4 right: 7 bottom: 4}
            draw_text +: {
                text_style: theme.font_regular{
                    font_size: 10.5
                }
            }
            close_button +: {
                width: 12
                height: 12
                margin: Inset{left: 4 right: 1}
            }
        }
    }
    use mod.widgets.SheetsGridCanvas
    use mod.widgets.PdfPageCanvas
    use mod.widgets.SlidesVisualCanvas
    use mod.widgets.DocumentTabStrip

    startup() do #(App::script_component(vm)) {
        ui: Root {
            main_window := Window {
                window.inner_size: vec2(1240, 780)
                window.title: "Rudra Office"
                show_caption_bar: false
                body +: {
                    width: Fill
                    height: Fill
                    flow: Overlay
                    show_bg: true
                    draw_bg.color: #xf1f3f6

                    content := View {
                        width: Fill
                        height: Fill
                        flow: Down
                        padding: 0
                        spacing: 0

                        header_tabs_bar := View {
                            width: Fill
                            height: 38
                            flow: Right
                            spacing: 4
                            padding: Inset{left: 8 top: 2 right: 0 bottom: 2}
                            align: Align{y: 0.5}
                            show_bg: true
                            draw_bg.color: #xf6f7f9
                            new_batch: true

                            chrome_drag := View {
                                width: 152
                                height: Fill
                                flow: Right
                                align: Align{y: 0.5}

                                Label {
                                    text: "Rudra Office"
                                    width: Fit
                                    draw_text.text_style.font_size: 12
                                    draw_text.color: #x273244
                                }
                            }

                            document_tabs_host := View {
                                width: Fill
                                height: 34
                                flow: Down

                                document_tabs := DocumentTabStrip {
                                    width: Fill
                                    height: 34
                                    tab_bar: RudraDocumentTabBar {}
                                }
                            }

                            empty_chrome_drag := View {
                                width: Fill
                                height: Fill
                            }

                            chrome_minimize := ButtonFlatter { width: 46 height: 34 text: "—" }
                            chrome_maximize := ButtonFlatter { width: 46 height: 34 text: "□" }
                            chrome_close := ButtonFlatter { width: 46 height: 34 text: "×" }
                        }

                        menu_bar := View {
                            width: Fill
                            height: 38
                            flow: Right
                            spacing: 6
                            padding: Inset{left: 12 right: 12}
                            align: Align{y: 0.5}
                            show_bg: true
                            draw_bg.color: #xffffffff

                            ScrollXView {
                                width: Fill height: Fill flow: Right spacing: 6
                                align: Align{y: 0.5}
                            menu_file_button := ButtonFlat {
                                width: 64
                                height: 30
                                text: "Файл"
                            }

                            menu_home_button := ButtonFlat {
                                width: 82
                                height: 30
                                text: "Главная"
                            }

                            menu_insert_button := ButtonFlat { width: 82 height: 30 text: "Вставка" visible: false }
                            menu_layout_button := ButtonFlat { width: 76 height: 30 text: "Макет" visible: false }
                            menu_formulas_button := ButtonFlat { width: 88 height: 30 text: "Формулы" visible: false }
                            menu_data_button := ButtonFlat { width: 82 height: 30 text: "Данные" visible: false }
                            menu_view_button := ButtonFlat { width: 58 height: 30 text: "Вид" visible: false }
                            }

                            save_button := ButtonFlat {
                                width: 114
                                height: 30
                                text: "Сохранить как"
                            }

                            probe_button := ButtonFlat {
                                width: 118
                                height: 30
                                text: "Движки"
                            }
                        }

                        extra_ribbon := RoundedView {
                            width: Fill height: 96 flow: Down
                            margin: Inset{left: 8 top: 4 right: 8 bottom: 4}
                            padding: Inset{left: 10 top: 6 right: 10 bottom: 6}
                            spacing: 5 visible: false
                            show_bg: true new_batch: true clip_x: true clip_y: true
                            draw_bg +: { color: #xffffffff border_color: #xd7dde5 border_size: 1.0 border_radius: 3.0 }
                            sheets_data_inputs := ScrollXView {
                                width: Fill height: 32 flow: Right spacing: 8
                                align: Align{y: 0.5} visible: false
                                Label { text: "Адрес" draw_text.color: #x3f4752 }
                                sheets_address_input := TextInput { width: 130 height: 30 empty_text: "A1 или A1:C5" }
                                Label { text: "Найти" draw_text.color: #x3f4752 }
                                sheets_search_input := TextInput { width: 210 height: 30 empty_text: "Текст или формула на листе" }
                                Label { text: "Столбец" draw_text.color: #x3f4752 }
                                sheets_key_column_input := TextInput { width: 60 height: 30 text: "A" empty_text: "A" }
                                sheets_header_dropdown := DropDown { width: 170 height: 30 labels: ["Без заголовка", "Есть заголовок"] popup_menu_position: BelowInput }
                            }
                            sheets_filter_inputs := ScrollXView {
                                width: Fill height: 32 flow: Right spacing: 8 align: Align{y: 0.5} visible: false
                                Label { text: "Условие" draw_text.color: #x3f4752 }
                                sheets_filter_kind_dropdown := DropDown { width: 195 height: 30 labels: ["Равно", "Не равно", "Содержит", "Пустые", "Непустые", "Число равно", "Больше", "Меньше", "Значения (по строкам)"] popup_menu_position: BelowInput }
                                sheets_filter_value_input := TextInput { width: 280 height: 30 empty_text: "Значение; список — по одному в строке" is_multiline: true }
                                Label { text: "Фильтр не удаляет данные" draw_text.color: #x5c6370 draw_text.text_style.font_size: 11 }
                            }
                            ScrollXView {
                                width: Fill height: 44 flow: Right spacing: 8
                                align: Align{y: 0.5}
                                extra_command_0 := ButtonFlat { width: 168 height: 36 text: "" }
                                extra_command_1 := ButtonFlat { width: 168 height: 36 text: "" }
                                extra_command_2 := ButtonFlat { width: 168 height: 36 text: "" }
                                extra_command_3 := ButtonFlat { width: 168 height: 36 text: "" }
                                extra_command_4 := ButtonFlat { width: 168 height: 36 text: "" }
                                extra_command_5 := ButtonFlat { width: 168 height: 36 text: "" }
                                extra_command_6 := ButtonFlat { width: 168 height: 36 text: "" }
                                extra_command_7 := ButtonFlat { width: 168 height: 36 text: "" }
                                extra_command_8 := ButtonFlat { width: 168 height: 36 text: "" }
                                extra_command_9 := ButtonFlat { width: 168 height: 36 text: "" }
                            }
                            extra_ribbon_title := Label {
                                width: Fill height: Fit text: ""
                                draw_text.text_style.font_size: 11
                                draw_text.color: #x5c6370
                            }
                        }

                        home_ribbon := RoundedView {
                            width: Fill
                            height: 96
                            flow: Overlay
                            margin: Inset{left: 8 top: 4 right: 8 bottom: 4}
                            visible: false
                            show_bg: true
                            new_batch: true
                            clip_x: true
                            clip_y: true
                            draw_bg +: {
                                color: #xffffffff
                                border_color: #xd7dde5
                                border_size: 1.0
                                border_radius: 3.0
                            }

                            writer_home_ribbon := ScrollXView {
                                width: Fill
                                height: Fill
                                flow: Right
                                spacing: 10
                                padding: Inset{left: 8 top: 5 right: 8 bottom: 4}
                                visible: false

                                View {
                                    width: 280
                                    height: Fill
                                    flow: Down
                                    spacing: 3
                                    View {
                                        width: Fill height: 30 flow: Right spacing: 4
                                        writer_text_block_dropdown := DropDown {
                                            width: 130
                                            height: 30
                                            popup_menu_position: BelowInput
                                            labels: ["Выберите абзац…"]
                                            draw_text.text_style.font_size: 10
                                        }
                                        writer_text_apply_button := Button { width: 72 height: 30 text: "Применить" }
                                        writer_text_revert_button := Button { width: 68 height: 30 text: "Вернуть" }
                                    }
                                    writer_text_input := TextInput {
                                        width: Fill
                                        height: 42
                                        is_multiline: true
                                        submit_on_enter: false
                                        empty_text: "Текст выбранного абзаца"
                                        draw_selection +: {
                                            color: #x93c5fd66
                                            color_hover: #x93c5fd66
                                            color_focus: #x93c5fd66
                                            color_down: #x93c5fd66
                                            color_empty: #x93c5fd66
                                            color_disabled: #xdbeafe
                                        }
                                    }
                                    Label {
                                        width: Fill
                                        height: Fit
                                        text: "Фрагмент: выделите текст; иначе весь абзац"
                                        draw_text.text_style.font_size: 9
                                        draw_text.color: #x667588
                                    }
                                }

                                View { width: 1 height: Fill show_bg: true draw_bg.color: #xd7dde5 }

                                View {
                                    width: Fit
                                    height: Fill
                                    flow: Down
                                    spacing: 4
                                    View {
                                        width: Fit height: 30 flow: Right spacing: 4
                                        writer_undo_button := Button { width: 70 height: 30 text: "↶ Отмена" }
                                        writer_redo_button := Button { width: 70 height: 30 text: "↷ Повтор" }
                                    }
                                    Label { width: Fill height: Fit text: "Правка" draw_text.text_style.font_size: 9 draw_text.color: #x667588 }
                                }

                                View { width: 1 height: Fill show_bg: true draw_bg.color: #xd7dde5 }

                                View {
                                    width: Fit
                                    height: Fill
                                    flow: Down
                                    spacing: 4
                                    writer_color_dropdown := DropDown {
                                        width: 206
                                        height: 30
                                        margin: 0.
                                        popup_menu_position: BelowInput
                                        labels: [
                                            "Цвет текста и фона…"
                                            "Текст · Авто"
                                            "Текст · Чёрный"
                                            "Текст · Красный"
                                            "Текст · Синий"
                                            "Текст · Зелёный"
                                            "Текст · Фиолетовый"
                                            "Фон текста · Нет"
                                            "Фон текста · Жёлтый"
                                            "Фон текста · Зелёный"
                                            "Фон текста · Голубой"
                                            "Фон текста · Розовый"
                                            "Фон текста · Серый"
                                        ]
                                        draw_text.text_style.font_size: 10
                                    }
                                    Label { width: Fill height: Fit text: "Цвет и заливка текста" draw_text.text_style.font_size: 9 draw_text.color: #x667588 }
                                }

                                View { width: 1 height: Fill show_bg: true draw_bg.color: #xd7dde5 }

                                View {
                                    width: Fit
                                    height: Fill
                                    flow: Down
                                    spacing: 4
                                    View {
                                        width: Fit height: 30 flow: Right spacing: 4
                                        writer_font_input := TextInput { width: 150 height: 30 empty_text: "Шрифт" text: "Arial" }
                                        writer_size_input := TextInput { width: 52 height: 30 empty_text: "pt" text: "11" }
                                        writer_font_apply_button := Button { width: 78 height: 30 text: "Применить" }
                                    }
                                    View {
                                        width: Fit height: 28 flow: Right spacing: 4
                                        writer_bold_button := Button { width: 34 height: 28 text: "Ж" }
                                        writer_italic_button := Button { width: 34 height: 28 text: "К" }
                                        writer_underline_button := Button { width: 34 height: 28 text: "Ч" }
                                        writer_strike_button := Button { width: 34 height: 28 text: "abc" }
                                    }
                                    Label { width: Fill height: Fit text: "Шрифт" draw_text.text_style.font_size: 9 draw_text.color: #x667588 }
                                }

                                View { width: 1 height: Fill show_bg: true draw_bg.color: #xd7dde5 }

                                View {
                                    width: Fit
                                    height: Fill
                                    flow: Down
                                    spacing: 4
                                    View {
                                        width: Fit height: 30 flow: Right spacing: 4
                                        writer_align_left_button := Button { width: 46 height: 30 text: "Лево" }
                                        writer_align_center_button := Button { width: 54 height: 30 text: "Центр" }
                                        writer_align_right_button := Button { width: 52 height: 30 text: "Право" }
                                        writer_align_justify_button := Button { width: 66 height: 30 text: "По шир." }
                                    }
                                    Label { width: Fill height: Fit text: "Весь абзац" draw_text.text_style.font_size: 9 draw_text.color: #x667588 }
                                }

                                View { width: 1 height: Fill show_bg: true draw_bg.color: #xd7dde5 }

                                View {
                                    width: Fit
                                    height: Fill
                                    flow: Down
                                    spacing: 4
                                    View {
                                        width: Fit height: 30 flow: Right spacing: 4
                                        writer_normal_button := Button { width: 82 height: 30 text: "Обычный" }
                                        writer_h1_button := Button { width: 92 height: 30 text: "Заголовок 1" }
                                        writer_h2_button := Button { width: 92 height: 30 text: "Заголовок 2" }
                                    }
                                    Label { width: Fill height: Fit text: "Стиль абзаца" draw_text.text_style.font_size: 9 draw_text.color: #x667588 }
                                }
                            }

                            sheets_home_ribbon := ScrollXView {
                                width: Fill
                                height: Fill
                                flow: Right
                                spacing: 10
                                padding: Inset{left: 8 top: 5 right: 8 bottom: 4}
                                visible: false

                                View {
                                    width: Fit height: Fill flow: Down spacing: 4
                                    View {
                                        width: Fit height: 30 flow: Right spacing: 4
                                        sheets_paste_button := Button { width: 92 height: 30 text: "Вставить" }
                                        sheets_copy_button := Button { width: 100 height: 30 text: "Копировать" }
                                    }
                                    View {
                                        width: Fit height: 30 flow: Right spacing: 4
                                        sheets_cut_button := Button { width: 100 height: 30 text: "Вырезать" }
                                        sheets_cut_status := Label { width: 92 height: Fit text: "—" draw_text.text_style.font_size: 9 draw_text.color: #x315f92 }
                                    }
                                    Label { width: Fill height: Fit text: "Буфер обмена" draw_text.text_style.font_size: 9 draw_text.color: #x667588 }
                                }

                                View { width: 1 height: Fill show_bg: true draw_bg.color: #xd7dde5 }

                                View {
                                    width: Fit height: Fill flow: Down spacing: 4
                                    View {
                                        width: Fit height: 30 flow: Right spacing: 4
                                        sheets_cell_input := TextInput { width: 300 height: 30 empty_text: "Значение или =ФОРМУЛА" }
                                        sheets_cell_apply_button := Button { width: 82 height: 30 text: "Применить" }
                                        sheets_cell_clear_button := Button { width: 72 height: 30 text: "Очистить" }
                                    }
                                    View {
                                        width: Fit height: 30 flow: Right spacing: 4
                                        sheets_undo_button := Button { width: 94 height: 30 text: "↶ Отмена" }
                                        sheets_redo_button := Button { width: 94 height: 30 text: "↷ Повтор" }
                                    }
                                    Label { width: Fill height: Fit text: "Ячейка" draw_text.text_style.font_size: 9 draw_text.color: #x667588 }
                                }

                                View { width: 1 height: Fill show_bg: true draw_bg.color: #xd7dde5 }

                                View {
                                    width: Fit height: Fill flow: Down spacing: 4
                                    View {
                                        width: Fit height: 30 flow: Right spacing: 4
                                        sheets_bold_button := Button { width: 38 height: 30 text: "Ж" }
                                        sheets_italic_button := Button { width: 38 height: 30 text: "К" }
                                        sheets_underline_button := Button { width: 38 height: 30 text: "Ч" }
                                    }
                                    Label { width: Fill height: Fit text: "Шрифт" draw_text.text_style.font_size: 9 draw_text.color: #x667588 }
                                }

                                View { width: 1 height: Fill show_bg: true draw_bg.color: #xd7dde5 }

                                View {
                                    width: Fit height: Fill flow: Down spacing: 4
                                    sheets_color_dropdown := DropDown {
                                        width: 202
                                        height: 30
                                        margin: 0.
                                        popup_menu_position: BelowInput
                                        labels: [
                                            "Цвет текста и ячейки…"
                                            "Текст · Чёрный"
                                            "Текст · Красный"
                                            "Текст · Синий"
                                            "Текст · Зелёный"
                                            "Текст · Фиолетовый"
                                            "Ячейка · Белая"
                                            "Ячейка · Жёлтая"
                                            "Ячейка · Зелёная"
                                            "Ячейка · Голубая"
                                            "Ячейка · Розовая"
                                            "Ячейка · Серая"
                                        ]
                                        draw_text.text_style.font_size: 10
                                    }
                                    Label { width: Fill height: Fit text: "Цвет и заливка всей ячейки" draw_text.text_style.font_size: 9 draw_text.color: #x667588 }
                                }

                                View { width: 1 height: Fill show_bg: true draw_bg.color: #xd7dde5 }

                                View {
                                    width: Fit height: Fill flow: Down spacing: 4
                                    View {
                                        width: Fit height: 30 flow: Right spacing: 4
                                        sheets_align_left_button := Button { width: 46 height: 30 text: "Лево" }
                                        sheets_align_center_button := Button { width: 54 height: 30 text: "Центр" }
                                        sheets_align_right_button := Button { width: 52 height: 30 text: "Право" }
                                        sheets_wrap_button := Button { width: 104 height: 30 text: "Перенос текста" }
                                    }
                                    Label { width: Fill height: Fit text: "Выравнивание" draw_text.text_style.font_size: 9 draw_text.color: #x667588 }
                                }
                            }

                            slides_home_ribbon := ScrollXView {
                                width: Fill
                                height: Fill
                                flow: Right
                                spacing: 10
                                padding: Inset{left: 8 top: 5 right: 8 bottom: 4}
                                visible: false

                                View {
                                    width: Fit height: Fill flow: Down spacing: 4
                                    View {
                                        width: Fit height: 38 flow: Right spacing: 6
                                        slides_new_button := Button { width: 116 height: 38 text: "+ Новый слайд" }
                                        slides_duplicate_button := Button { width: 116 height: 38 text: "Дублировать" }
                                        slides_delete_button := Button { width: 104 height: 38 text: "Удалить" }
                                    }
                                    Label { width: Fill height: Fit text: "Слайды" draw_text.text_style.font_size: 9 draw_text.color: #x667588 }
                                }

                                View { width: 1 height: Fill show_bg: true draw_bg.color: #xd7dde5 }

                                View {
                                    width: Fit height: Fill flow: Down spacing: 4
                                    slides_text_color_dropdown := DropDown {
                                        width: 174
                                        height: 30
                                        margin: 0.
                                        popup_menu_position: BelowInput
                                        labels: [
                                            "Цвет текста…"
                                            "Чёрный"
                                            "Красный"
                                            "Синий"
                                            "Зелёный"
                                            "Фиолетовый"
                                        ]
                                        draw_text.text_style.font_size: 10
                                    }
                                    Label { width: Fill height: Fit text: "Выбранный текстовый объект" draw_text.text_style.font_size: 9 draw_text.color: #x667588 }
                                }

                                View { width: 1 height: Fill show_bg: true draw_bg.color: #xd7dde5 }

                                Label {
                                    width: Fit
                                    height: Fit
                                    margin: Inset{left: 6 top: 10}
                                    text: "Щёлкните текст на слайде, затем выберите его цвет."
                                    draw_text.text_style.font_size: 11
                                    draw_text.color: #x667588
                                }
                            }

                            pdf_home_ribbon := View {
                                width: Fill
                                height: Fill
                                flow: Right
                                padding: Inset{left: 14 top: 8 right: 14 bottom: 8}
                                align: Align{y: 0.5}
                                visible: false
                                Label {
                                    width: Fill
                                    height: Fit
                                    text: "PDF открыт в защищённом режиме просмотра. Исходный текст страницы является частью PDF-рендера: изменение его цвета в этом выпуске недоступно и не имитируется."
                                    draw_text.text_style.font_size: 12
                                    draw_text.color: #x92400e
                                }
                            }
                        }

                        View {
                            width: Fill
                            height: Fill
                            flow: Right
                            spacing: 0
                            margin: 0.

                            sidebar := View {
                                width: 156
                                height: Fill
                                flow: Down
                                spacing: 6
                                padding: Inset{left: 8 top: 8 right: 8 bottom: 8}
                                show_bg: true
                                draw_bg.color: #xf8f9fb
                                new_batch: true

                                Label {
                                    text: "РАЗДЕЛЫ"
                                    draw_text.text_style.font_size: 10
                                    draw_text.color: #x667588
                                }

                                home_button := Button { width: Fill height: 30 text: "Обзор" }
                                writer_button := Button { width: Fill height: 30 text: "Документы" }
                                sheets_button := Button { width: Fill height: 30 text: "Таблицы" }
                                pdf_button := Button { width: Fill height: 30 text: "PDF" }
                                slides_button := Button { width: Fill height: 30 text: "Презентации" }

                                View { width: Fill height: Fill }

                                Label {
                                    text: "DOCX · XLSX\nPDF · PPTX"
                                    draw_text.text_style.font_size: 9
                                    draw_text.color: #x667588
                                }
                            }

                            workspace_panel := View {
                                width: Fill
                                height: Fill
                                flow: Down
                                spacing: 4
                                padding: 0.
                                show_bg: true
                                draw_bg.color: #xf1f3f6
                                new_batch: true

                                module_label := Label {
                                    width: Fill
                                    height: Fit
                                    text: "Обзор"
                                    draw_text.text_style.font_size: 20
                                }

                                status_label := Label {
                                    width: Fill
                                    height: Fit
                                    text: "Откройте документ через меню «Файл» или проверьте доступность движков."
                                    draw_text.text_style.font_size: 13
                                }

                                detail_label := Label {
                                    width: Fill
                                    height: Fit
                                    visible: false
                                    text: "Приложение работает локально и не отправляет документы во внешние сервисы."
                                    draw_text.text_style.font_size: 12
                                    draw_text.color: #x667588
                                }

                                module_viewport := View {
                                    width: Fill
                                    height: Fill
                                    flow: Overlay

                                home_start_view := mod.widgets.RudraOfficeLauncher {}

                                sheets_document_view := View {
                                    width: Fill
                                    height: Fill
                                    flow: Down
                                    spacing: 4
                                    visible: false

                                    View {
                                        width: Fill
                                        height: 30
                                        flow: Right
                                        spacing: 4
                                        align: Align{y: 0.5}

                                        sheets_tabs_label := Label {
                                            width: 160
                                            height: Fit
                                            text: "Листы: нет"
                                            draw_text.text_style.font_size: 12
                                        }

                                        sheets_summary_label := Label {
                                            width: Fill
                                            height: Fit
                                            text: "Лист не выбран."
                                            draw_text.text_style.font_size: 11
                                            draw_text.color: #x9fb0c7
                                        }

                                        sheets_prev_button := Button {
                                            width: 64
                                            height: 30
                                            text: "← Лист"
                                        }

                                        sheets_next_button := Button {
                                            width: 64
                                            height: 30
                                            text: "Лист →"
                                        }
                                    }

                                    sheets_grid_canvas := SheetsGridCanvas {
                                        width: Fill
                                        height: Fill
                                    }

                                    sheets_empty_label := Label {
                                        width: Fill
                                        height: Fill
                                        text: "Книга не содержит заполненных ячеек."
                                        draw_text.text_style.font_size: 16
                                        visible: false
                                    }
                                }

                                writer_document_view := mod.widgets.RudraWriterView {}

                                pdf_document_view := View {
                                    width: Fill
                                    height: Fill
                                    flow: Down
                                    spacing: 4
                                    visible: false

                                    pdf_toolbar := View {
                                        width: Fill
                                        height: 30
                                        flow: Right
                                        spacing: 4
                                        align: Align{y: 0.5}

                                        pdf_first_button := Button {
                                            width: 28
                                            height: 30
                                            text: "|<"
                                        }
                                        pdf_prev_button := Button {
                                            width: 28
                                            height: 30
                                            text: "<"
                                        }
                                        pdf_page_input := TextInput {
                                            width: 42
                                            height: 30
                                            empty_text: "№"
                                        }
                                        pdf_go_button := Button {
                                            width: 30
                                            height: 30
                                            text: "Go"
                                        }
                                        pdf_next_button := Button {
                                            width: 28
                                            height: 30
                                            text: ">"
                                        }
                                        pdf_last_button := Button {
                                            width: 28
                                            height: 30
                                            text: ">|"
                                        }
                                        pdf_page_label := Label {
                                            width: Fill
                                            height: Fit
                                            text: "0 / 0"
                                            draw_text.text_style.font_size: 10.5
                                            draw_text.color: #x9fb0c7
                                        }

                                        pdf_zoom_out_button := Button {
                                            width: 28
                                            height: 30
                                            text: "−"
                                        }
                                        pdf_zoom_label := Label {
                                            width: 76
                                            height: Fit
                                            text: "100%"
                                            draw_text.text_style.font_size: 10.5
                                        }
                                        pdf_zoom_in_button := Button {
                                            width: 28
                                            height: 30
                                            text: "+"
                                        }
                                        pdf_zoom_reset_button := Button {
                                            width: 44
                                            height: 30
                                            text: "100%"
                                        }
                                        pdf_fit_width_button := Button {
                                            width: 58
                                            height: 30
                                            text: "Width"
                                        }
                                        pdf_fit_page_button := Button {
                                            width: 58
                                            height: 30
                                            text: "Page"
                                        }
                                    }

                                    View {
                                        width: Fill
                                        height: Fill
                                        flow: Overlay
                                        clip_x: true
                                        clip_y: true

                                        pdf_page_canvas := PdfPageCanvas {
                                            width: Fill
                                            height: Fill
                                        }

                                        pdf_loading_label := Label {
                                            width: Fill
                                            height: Fill
                                            align: Align{ x: 0.5 y: 0.5 }
                                            text: "Rendering PDF pages..."
                                            draw_text.text_style.font_size: 16
                                            visible: false
                                        }

                                        pdf_empty_label := Label {
                                            width: Fill
                                            height: Fill
                                            align: Align{ x: 0.5 y: 0.5 }
                                            text: "PDF has no pages."
                                            draw_text.text_style.font_size: 16
                                            visible: false
                                        }

                                        pdf_error_label := Label {
                                            width: Fill
                                            height: Fill
                                            align: Align{ x: 0.5 y: 0.5 }
                                            text: "PDF page render error."
                                            draw_text.text_style.font_size: 16
                                            draw_text.color: #xff9b9b
                                            visible: false
                                        }
                                    }
                                }
                                slides_document_view := View {
                                    width: Fill
                                    height: Fill
                                    flow: Down
                                    spacing: 4
                                    visible: false

                                    slides_toolbar := View {
                                        width: Fill
                                        height: 30
                                            flow: Right
                                            spacing: 4
                                            align: Align{y: 0.5}

                                        slides_page_label := Label {
                                            width: 96
                                            height: Fit
                                            text: "Слайды: нет данных"
                                            draw_text.text_style.font_size: 11
                                        }

                                        slides_status_label := Label {
                                            width: Fill
                                            height: Fit
                                            text: "Режим просмотра"
                                            draw_text.text_style.font_size: 10.5
                                            draw_text.color: #x9fb0c7
                                        }

                                        slides_prev_button := Button {
                                            width: 64
                                            height: 30
                                            text: "← Назад"
                                        }

                                        slides_next_button := Button {
                                            width: 64
                                            height: 30
                                            text: "Далее →"
                                        }

                                        slides_zoom_out_button := Button {
                                            width: 30
                                            height: 30
                                            text: "−"
                                        }

                                        slides_zoom_in_button := Button {
                                            width: 30
                                            height: 30
                                            text: "+"
                                        }

                                        slides_zoom_reset_button := Button {
                                            width: 44
                                            height: 30
                                            text: "100%"
                                        }
                                        slides_fit_button := Button {
                                            width: 54
                                            height: 30
                                            text: "Fit"
                                        }

                                        slides_present_button := Button {
                                            width: 58
                                            height: 30
                                            text: "Показ"
                                        }
                                    }

                                    slides_content_view := View {
                                        width: Fill
                                        height: Fill
                                        flow: Overlay

                                        slides_visual_canvas := SlidesVisualCanvas {
                                            width: Fill
                                            height: Fill
                                        }

                                        slides_empty_label := Label {
                                            width: Fill
                                            height: Fill
                                            align: Align{x: 0.5 y: 0.5}
                                            text: "Слайд недоступен для отображения."
                                            draw_text.text_style.font_size: 16
                                            visible: false
                                        }

                                        slides_error_label := Label {
                                            width: Fill
                                            height: Fill
                                            align: Align{x: 0.5 y: 0.5}
                                            text: "Ошибка отображения презентации."
                                            draw_text.text_style.font_size: 16
                                            draw_text.color: #xff9b9b
                                            visible: false
                                        }
                                    }
                                }
                                }
                            }
                        }
                    }

                    file_menu_popup := RoundedView {
                        width: 232
                        height: Fit
                        flow: Down
                        padding: Inset{left: 4 top: 4 right: 4 bottom: 4}
                        visible: false
                        show_bg: true
                        new_batch: true
                        clip_x: true
                        clip_y: true
                        draw_bg +: {
                            color: #x0f1724
                            border_color: #x5f728c
                            border_size: 1.0
                            border_radius: 4.0
                        }

                        open_button := Button {
                            width: Fill
                            height: 32
                            text: "Открыть…     Ctrl+O"
                        }
                    }
                }
            }
        }
    }
}
