# Rudra Office application modules

The canonical application is `workspace/crates/rudra-office-app`. The frozen
Preview8/9/10 workspaces and reports are historical candidates, not an implicit
replacement for canonical sources. This refactor does not promote their fixes.

## Ownership

- `main.rs`: entry point, App/AppState shared records, compatibility names and
  remaining small shared projections/widgets.
- `app_documents.rs`: open/export adapters, typed document sessions, lifecycle,
  shared document commands and cross-format history ownership.
- `app_shell.rs`: menus, chrome, document tabs, file dispatch and unified UI
  refresh orchestration.
- `app_writer.rs`: Writer editing/view adapters and per-document text state.
- `app_sheets.rs`: spreadsheet commands, clipboard and grid integration.
- `app_pdf.rs`: page geometry, bounded cache, controller and PDF state commands.
- `app_slides.rs`: presentation state, navigation, selection and frame adapter.
- `app_accessibility.rs`: the unified accessibility projection/action boundary.
- `app_events.rs`: Makepad lifecycle and event dispatch in the original order.
- `app_ui.rs`: editor Splash module and editor-local templates.
- `app_launcher.rs`: typed CLI startup and shared Home request dispatch.
- `rudra-office-launcher`: independent engine-free Makepad EXE plus shared Home
  view/protocol/consumer UIA adapter. See `LAUNCHER.md`.
- `app_tests.rs`: the existing `tests` namespace, not a reduced test suite.

Existing backend crates and rendering widgets remain the lower-level owners.
This is Rust source modularization, not dynamic DLL unloading, independent VMs,
a new generic runtime or an Office-specific fork of Toolkit components.

## Migration invariants

Keep App/AppState ownership and DocumentId lifetimes unchanged. Internal methods
use crate-scoped visibility so siblings can share the existing integration
boundary; this is an explicit transitional coupling, not a claim that each
module is already an independently compilable service/crate.

The extraction preserves method bodies, widget IDs, local Splash templates,
theme/widget registration order and event order. Only the final UI registration
call changes from `self::script_mod(vm)` to `crate::app_ui::script_mod(vm)`.
Public PDF names remain available at the crate root. Existing tests/assertions
are retained. Do not use include! to hide a second monolith or make copies of
the domain engines.

CHEAPEST_VERIFICATION_PATH: extraction-helper tests and source-body comparison;
profile/policy tests; fmt of changed files; owning application tests/check;
strict application Clippy. GUI/risk compositions remain separate requirements,
currently constrained by the canary's read-only capability boundary. A clean
build is not visual, editing, clipboard or release acceptance.
