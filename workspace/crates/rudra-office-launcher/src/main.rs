#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
//! Engine-free Makepad entry process. Editor children own their own documents.
use makepad_widgets::*;
use rudra_office_launcher::{LaunchRequest, sibling_editor_path};
use std::process::{Child, Command};

app_main!(LauncherApp);

script_mod! {
    use mod.prelude.widgets.*
    startup() do #(LauncherApp::script_component(vm)) {
        ui: Root {
            main_window := Window {
                window.inner_size: vec2(1000, 640)
                window.title: "Rudra Office Launcher"
                body +: {
                    width: Fill height: Fill flow: Down spacing: 0
                    show_bg: true draw_bg.color: #xffffffff
                    ScrollXYView {
                        width: Fill height: Fill
                        home_start_view := mod.widgets.RudraOfficeLauncher { width: 940 height: 510 }
                    }
                    View {
                        width: Fill height: 44 flow: Right spacing: 12 padding: 8
                        launcher_status := Label { width: Fill text: "Выберите документ или откройте файл." }
                        launcher_close := ButtonFlat { width: 86 text: "Закрыть" }
                    }
                }
            }
        }
    }
}

#[derive(Script, ScriptHook)]
struct LauncherApp {
    #[live]
    ui: WidgetRef,
    #[rust]
    children: Vec<Child>,
    #[rust]
    poll_timer: Timer,
    #[rust]
    status: String,
    #[rust]
    drawn: bool,
}

impl LauncherApp {
    fn set_status(&mut self, cx: &mut Cx, status: String) {
        self.ui
            .label(cx, ids!(launcher_status))
            .set_text(cx, &status);
        self.status = status;
        self.ui.redraw(cx);
    }

    fn launch(&mut self, cx: &mut Cx, request: LaunchRequest) {
        // No PATH search, environment override, shell expansion or document IO.
        let result = std::env::current_exe().and_then(|own_exe| {
            let editor = sibling_editor_path(&own_exe);
            if !editor.is_file() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "Редактор не найден: {}. EXE должны лежать рядом.",
                        editor.display()
                    ),
                ));
            }
            let mut command = Command::new(editor);
            command.args(request.editor_args());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x0800_0000); // no console; editor GUI remains visible
            }
            command.spawn()
        });
        match result {
            Ok(child) => {
                let pid = child.id();
                self.children.push(child);
                self.set_status(cx, format!("Редактор запущен · PID {pid}"));
            }
            Err(error) => self.set_status(cx, format!("Не удалось запустить редактор: {error}")),
        }
    }

    fn reap_children(&mut self, cx: &mut Cx) {
        let mut failure = None;
        self.children.retain_mut(|child| match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    failure = Some(format!("Редактор завершился: {status}"));
                }
                false
            }
            Ok(None) => true,
            Err(error) => {
                failure = Some(format!("Не удалось проверить редактор: {error}"));
                false
            }
        });
        if let Some(error) = failure {
            self.set_status(cx, error);
        }
    }

    #[cfg(windows)]
    fn publish_accessibility(&self, cx: &mut Cx) {
        use rudra_office_launcher::accessibility::{current_window, publish};
        use toolkit_windows_automation::{AccessibilityNode, AccessibilityRole, AccessibilityTree};
        if !self.drawn {
            return;
        }
        let Ok(window) = current_window("Rudra Office Launcher") else {
            return;
        };
        let mut root = AccessibilityNode::new(
            "rudra-launcher/window",
            AccessibilityRole::Window,
            "Rudra Office Launcher",
            window.bounds(),
        );
        for (widget, id, name) in [
            (
                ids!(home_open_button),
                "open-local",
                "Открыть локальный файл",
            ),
            (
                ids!(home_new_docx_button),
                "new-docx",
                "Создать документ DOCX",
            ),
            (
                ids!(home_new_xlsx_button),
                "new-xlsx",
                "Создать таблицу XLSX",
            ),
            (
                ids!(home_new_pptx_button),
                "new-pptx",
                "Создать презентацию PPTX",
            ),
            (ids!(home_new_pdf_button), "new-pdf", "Создать PDF"),
            (ids!(launcher_close), "close", "Закрыть лаунчер"),
        ] {
            let area = self.ui.widget(cx, widget).area();
            if !area.is_valid(cx) {
                continue;
            }
            let rect = area.rect(cx);
            let mut node = AccessibilityNode::new(
                format!("rudra-launcher/{id}"),
                AccessibilityRole::Button,
                name,
                window.screen_rect(rect),
            );
            node.visible = rect.size.x > 0.0 && rect.size.y > 0.0;
            node.actions.invoke = true;
            root.children.push(node);
        }
        let area = self.ui.widget(cx, ids!(launcher_status)).area();
        if area.is_valid(cx) {
            root.children.push(AccessibilityNode::new(
                "rudra-launcher/status",
                AccessibilityRole::StatusBar,
                &self.status,
                window.screen_rect(area.rect(cx)),
            ));
        }
        if let Ok(tree) = AccessibilityTree::new(root) {
            publish(tree, &window);
        }
    }
}

impl MatchEvent for LauncherApp {
    fn handle_startup(&mut self, cx: &mut Cx) {
        self.poll_timer = cx.start_interval(1.0);
        self.set_status(cx, "Выберите документ или откройте файл.".into());
    }
    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions) {
        for request in rudra_office_launcher::view::requests(cx, &self.ui, actions) {
            self.launch(cx, request);
        }
        if self.ui.button(cx, ids!(launcher_close)).clicked(actions) {
            cx.quit();
        }
        #[cfg(windows)]
        {
            use rudra_office_launcher::DocumentKind;
            use rudra_office_launcher::accessibility::AccessibilityUiAction;
            use toolkit_windows_automation::AccessibilityAction;
            for action in actions
                .iter()
                .filter_map(|a| a.downcast_ref::<AccessibilityUiAction>())
            {
                if action.0.action != AccessibilityAction::Invoke {
                    continue;
                }
                let request = match action.0.node_id.as_str() {
                    "rudra-launcher/open-local" => Some(LaunchRequest::OpenDialog),
                    "rudra-launcher/new-docx" => Some(LaunchRequest::New(DocumentKind::Docx)),
                    "rudra-launcher/new-xlsx" => Some(LaunchRequest::New(DocumentKind::Xlsx)),
                    "rudra-launcher/new-pptx" => Some(LaunchRequest::New(DocumentKind::Pptx)),
                    "rudra-launcher/new-pdf" => Some(LaunchRequest::New(DocumentKind::Pdf)),
                    "rudra-launcher/close" => {
                        cx.quit();
                        None
                    }
                    _ => None,
                };
                if let Some(request) = request {
                    self.launch(cx, request);
                }
            }
        }
    }
}

impl AppMain for LauncherApp {
    fn script_mod(vm: &mut ScriptVm) -> ScriptValue {
        makepad_widgets::theme_mod(vm);
        script_eval!(vm, { mod.theme = mod.themes.light });
        makepad_widgets::widgets_mod(vm);
        rudra_office_launcher::view::script_mod(vm);
        self::script_mod(vm)
    }
    fn handle_event(&mut self, cx: &mut Cx, event: &Event) {
        self.match_event(cx, event);
        self.ui.handle_event(cx, event, &mut Scope::empty());
        if self.poll_timer.is_event(event).is_some() {
            self.reap_children(cx);
        }
        if matches!(event, Event::Draw(_)) {
            self.drawn = true;
        }
        #[cfg(windows)]
        self.publish_accessibility(cx);
    }
}
