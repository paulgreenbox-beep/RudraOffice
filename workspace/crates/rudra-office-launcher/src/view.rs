//! Single source of truth for both standalone and embedded start screens.
use crate::{DocumentKind, LaunchRequest};
use makepad_widgets::*;

script_mod! {
    use mod.prelude.widgets.*
    mod.widgets.RudraOfficeLauncher = RoundedView {
    width: Fill
    height: Fill
    flow: Down
    spacing: 18
    padding: Inset{left: 32 top: 28 right: 32 bottom: 24}
    show_bg: true
    draw_bg +: { color: #xffffffff border_size: 0.0 border_radius: 0.0 }

    View {
        width: Fill height: Fit flow: Down spacing: 7
        Label {
            width: Fill height: Fit
            text: "Главная"
            draw_text.text_style.font_size: 24
            draw_text.color: #x243246
        }
        Label {
            width: Fill height: Fit
            text: "Работайте с локальными документами в одном окне."
            draw_text.text_style.font_size: 12
            draw_text.color: #x667588
        }
    }

    home_open_button := Button {
        width: 216 height: 38
        text: "Открыть локальный файл…"
    }

    View { width: Fill height: 1 show_bg: true draw_bg.color: #xe4e8ee }

    Label {
        width: Fill height: Fit
        text: "Поддерживаемые форматы"
        draw_text.text_style.font_size: 15
        draw_text.color: #x344256
    }

    View {
        width: Fill height: 150 flow: Right spacing: 14
        align: Align{x: 0.5 y: 0.5}

        home_new_docx_button := Button {
            width: 150 height: 136 margin: 0.
            spacing: 0.
            align: Center
            icon_walk: Walk{width: 0. height: 0.}
            text: "DOCX\nДокументы"
            draw_text.text_style.font_size: 16
            draw_text.color: #x246b99
            draw_bg +: {
                color: #xf4f8fc
                color_hover: #xe9f3fb
                color_down: #xdfeef9
                color_focus: #xe9f3fb
                border_color: #x9fc7e3
                border_color_hover: #x2e81b8
                border_color_down: #x2e81b8
                border_color_focus: #x2e81b8
                border_color_2: vec4(-1.0, -1.0, -1.0, -1.0)
                border_size: 1.0
                border_radius: 5.0
            }
        }
        home_new_xlsx_button := Button {
            width: 150 height: 136 margin: 0.
            spacing: 0.
            align: Center
            icon_walk: Walk{width: 0. height: 0.}
            text: "XLSX\nТаблицы"
            draw_text.text_style.font_size: 16
            draw_text.color: #x20784a
            draw_bg +: {
                color: #xf4faf6
                color_hover: #xe9f5ed
                color_down: #xdceee3
                color_focus: #xe9f5ed
                border_color: #xa8d7bc
                border_color_hover: #x24945b
                border_color_down: #x24945b
                border_color_focus: #x24945b
                border_color_2: vec4(-1.0, -1.0, -1.0, -1.0)
                border_size: 1.0
                border_radius: 5.0
            }
        }
        home_new_pptx_button := Button {
            width: 150 height: 136 margin: 0.
            spacing: 0.
            align: Center
            icon_walk: Walk{width: 0. height: 0.}
            text: "PPTX\nПрезентации"
            draw_text.text_style.font_size: 16
            draw_text.color: #xa75c25
            draw_bg +: {
                color: #xfff9f2
                color_hover: #xfdf0e1
                color_down: #xf8e3ca
                color_focus: #xfdf0e1
                border_color: #xe8c6a3
                border_color_hover: #xd27831
                border_color_down: #xd27831
                border_color_focus: #xd27831
                border_color_2: vec4(-1.0, -1.0, -1.0, -1.0)
                border_size: 1.0
                border_radius: 5.0
            }
        }
        home_new_pdf_button := Button {
            width: 150 height: 136 margin: 0.
            spacing: 0.
            align: Center
            icon_walk: Walk{width: 0. height: 0.}
            text: "PDF\nНовый PDF"
            draw_text.text_style.font_size: 16
            draw_text.color: #xa34744
            draw_bg +: {
                color: #xfff7f6
                color_hover: #xfcecea
                color_down: #xf5d9d6
                color_focus: #xfcecea
                border_color: #xe7b8b6
                border_color_hover: #xc75b58
                border_color_down: #xc75b58
                border_color_focus: #xc75b58
                border_color_2: vec4(-1.0, -1.0, -1.0, -1.0)
                border_size: 1.0
                border_radius: 5.0
            }
        }
    }

    View { width: Fill height: 1 show_bg: true draw_bg.color: #xe4e8ee }

    Label {
        width: Fill height: Fit
        text: "Документы открываются в редакторе Rudra Office"
        draw_text.text_style.font_size: 12
        draw_text.color: #x667588
    }
}
}

/// Translate existing widget actions; the host owns document/process lifetime.
pub fn requests(cx: &mut Cx, ui: &WidgetRef, actions: &Actions) -> Vec<LaunchRequest> {
    let mut result = Vec::new();
    if ui.button(cx, ids!(home_open_button)).clicked(actions) {
        result.push(LaunchRequest::OpenDialog);
    }
    for (id, kind) in [
        (ids!(home_new_docx_button), DocumentKind::Docx),
        (ids!(home_new_xlsx_button), DocumentKind::Xlsx),
        (ids!(home_new_pptx_button), DocumentKind::Pptx),
        (ids!(home_new_pdf_button), DocumentKind::Pdf),
    ] {
        if ui.button(cx, id).clicked(actions) {
            result.push(LaunchRequest::New(kind));
        }
    }
    result
}
