# Separate Makepad launcher

The entry process is `rudra-office-launcher.exe`. Place it beside
`rudra-office-app.exe`. The launcher does not load document engines.
Its four tiles start the editor with `--new docx|xlsx|pptx|pdf`.
Open starts the editor with `--open-dialog`; the editor owns the Windows picker.
The typed contract also supports `--open <path>` without shell interpolation.

## Boundaries

- One shared Home view: `rudra-office-launcher/src/view.rs`.
- No document/session/temporary file logic in the launcher.
- Editor requests are consumed exactly once after the first draw (HWND ready).
- Closing the launcher does not discard or kill editor documents.
- Each tile starts a separate editor process; existing document tabs live in
  that process. Cross-process tab reuse/IPC is not part of this extraction.
- Existing editor direct launch and embedded Home remain supported.
- The shared Windows UIA adapter reuses Toolkit's existing generic provider.
  It resolves only this process's exact-title window, including multiple editors.
- No Makepad SDK migration, vendor change or global GUI routing change.

## Build / cheapest verification

From `workspace`: `cargo test --offline -p rudra-office-launcher --no-default-features`
then `cargo test --offline -p rudra-office-launcher`, then editor tests.
Build both: `cargo build --offline -p rudra-office-launcher -p rudra-office-app`.
The launcher protocol is std-only; Cargo metadata must show no engine in the
launcher's normal dependency graph. GUI acceptance is separately recorded.

Writer/Sheets/Slides/PDF remain modular sources/backends within one editor,
not four independent executables or dynamically loaded modules.
