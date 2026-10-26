//! Shared start screen requests; the editor alone owns document sessions/files.
use super::*;
use rudra_office_launcher::{LaunchRequest, parse_editor_args};

impl App {
    pub(crate) fn read_launcher_startup_request(&mut self) {
        match parse_editor_args(std::env::args_os().skip(1)) {
            Ok(request) => self.launcher_startup_request = request,
            Err(error) => {
                self.state.status = "Не удалось выполнить команду лаунчера.".to_owned();
                self.state.detail = error;
            }
        }
    }

    // Wait for the first layout/draw so a native file dialog has its own HWND.
    // take() ensures redraws never create a second document.
    pub(crate) fn apply_launcher_startup_request(&mut self, cx: &mut Cx) {
        if let Some(request) = self.launcher_startup_request.take() {
            self.dispatch_launcher_request(cx, request);
        }
    }

    pub(crate) fn dispatch_launcher_request(&mut self, cx: &mut Cx, request: LaunchRequest) {
        match request {
            LaunchRequest::New(kind) => self.create_new_document(cx, kind),
            LaunchRequest::Open(path) => self.open_document_path(cx, path),
            LaunchRequest::OpenDialog => self.open_document(cx),
        }
    }
}
