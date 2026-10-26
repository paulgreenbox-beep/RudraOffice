//! Shared existing Writer root; extracted verbatim from app_ui.rs.
//! Native and browser hosts retain identical widget IDs and draw/input widgets.
use makepad_widgets::*;
script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.VirtualCanvas
    let OnSelected = #(makepad_widgets::drop_down::PopupMenuPosition::OnSelected)
    mod.widgets.RudraWriterView = View {
    width: Fill
    height: Fill
    flow: Down
    spacing: 0
    visible: false

    writer_toolbar := View {
        width: Fill
        height: 34
        flow: Right
        spacing: 4
        padding: Inset{left: 12 right: 12}
        align: Align{y: 0.5}
        show_bg: true
        draw_bg.color: #xffffffff

        writer_pages_button := ButtonFlat {
            width: 64
            height: 28
            text: "Стр."
        }

        writer_continuous_button := ButtonFlat {
            width: 92
            height: 28
            text: "Непрерывно"
        }
        writer_visual_button := ButtonFlat {
            width: 78
            height: 28
            text: "Макет"
        }

        writer_plain_text_button := ButtonFlat {
            width: 62
            height: 28
            text: "Текст"
        }

        writer_edit_text_button := ButtonFlat {
            width: 118
            height: 28
            text: "Редактировать"
        }

    }

    writer_projection_notice_label := Label {
        width: Fill
        height: Fit
        text: ""
        draw_text.text_style.font_size: 11
        draw_text.color: #ffc86b
        visible: false
    }

    writer_content_view := View {
        width: Fill
        height: Fill
        flow: Overlay
        new_batch: true
        clip_x: true
        clip_y: true

    writer_page_canvas := VirtualCanvas {
        width: Fill
        height: Fill
    }

    writer_inline_editor_host := View {
        width: 1
        height: 1
        flow: Overlay
        clip_x: true
        clip_y: true
        visible: false

       writer_inline_text_input := TextInput {
           width: Fill
           height: Fill
           margin: 0.
            padding: Inset{left: 8 top: 6 right: 8 bottom: 6}
           is_multiline: true
           submit_on_enter: false
           empty_text: "Введите текст"
            draw_text +: {
                color: #x202124
                color_hover: #x202124
                color_focus: #x202124
                color_down: #x202124
                color_disabled: #x6b7280
                color_empty: #x6b7280
                color_empty_hover: #x6b7280
                color_empty_focus: #x6b7280
                text_style.font_size: 14
            }
            draw_selection +: {
                color: #x93c5fd66
                color_hover: #x93c5fd66
                color_focus: #x93c5fd66
                color_down: #x93c5fd66
                color_empty: #x93c5fd66
                color_disabled: #xdbeafe
            }
            draw_cursor +: {
                color: #x2563eb
            }
           draw_bg +: {
               color: #xffffffff
               color_hover: #xffffffff
               color_focus: #xffffffff
                border_color: #x2563eb
                border_color_focus: #x2563eb
                border_size: 1.0
            }
        }
    }

    writer_text_fallback_label := Label {
            width: Fill
            height: Fit
            text: "Plain-text fallback is available."
            draw_text.text_style.font_size: 14
            visible: false
        }

        writer_empty_label := Label {
            width: Fill
            height: Fill
            align: Align{x: 0.5 y: 0.5}
            text: "Writer document is empty."
            draw_text.text_style.font_size: 16
            visible: false
        }

        writer_error_label := Label {
            width: Fill
            height: Fill
            align: Align{x: 0.5 y: 0.5}
            text: "Writer display error."
            draw_text.text_style.font_size: 16
            draw_text.color: #xff9b9b
            visible: false
        }
    }
    writer_status_bar := View {
        width: Fill
        height: 30
        flow: Right
        spacing: 6
        padding: Inset{left: 8 right: 8}
        align: Align{y: 0.5}
        show_bg: true
        draw_bg.color: #xffffffff
        new_batch: true

        writer_page_label := Label {
            width: 174
            height: Fit
            text: "Область: — из 0"
            draw_text.text_style.font_size: 11
            draw_text.color: #x344256
        }

        writer_page_setup_label := Label {
            width: Fill
            height: Fit
            text: ""
            draw_text.text_style.font_size: 11
            draw_text.color: #x667588
        }

        writer_zoom_out_button := Button {
            width: 28
            height: 28
            text: "−"
        }

        writer_zoom_label := Label {
            width: 48
            height: Fit
            text: "100%"
            draw_text.text_style.font_size: 12
        }

        writer_zoom_in_button := Button {
            width: 28
            height: 28
            text: "+"
        }

        writer_zoom_dropdown := DropDown {
            width: 80
            height: 28
            selected_item: 6
            popup_menu_position: OnSelected
            labels: [
                "50%"
                "75%"
                "100%"
                "125%"
                "150%"
                "200%"
                "Выбрать…"
            ]
            draw_text.text_style.font_size: 10
        }
    }

}
}
