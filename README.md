# Rudra Office

Rudra Office is an actively developed Rust desktop office workspace supporting viewing and evolving editing workflows for DOCX, XLSX, PDF and PPTX. The interface uses Makepad and local document engines.

**Initial Public Source Baseline.** This repository contains the real application, four document components, adapters, tests and verification tooling. It is not a finished commercial office suite or an accepted production release. No Windows executable or release tag is published by this baseline.

Русский: Rudra Office — разрабатываемое офисное приложение на Rust/Makepad. Опубликованы исходники Writer, Sheets, Slides и PDF; это не утверждение полной совместимости с Microsoft Office или ONLYOFFICE. Пользовательские документы и закрытые доказательства приёмки не включены.

## Features

- Multi-document workspace, tabs, typed document identity and document-scoped state.
- DOCX / Writer: document import, page projection, text and evolving editing/history workflows.
- XLSX / Sheets: IronCalc-backed workbooks, grid, selection, formulas, sort/filter, structural and clipboard foundations.
- PDF: local parsing, page rendering and navigation with bounded rendering safeguards.
- PPTX / Slides: local presentation parsing, visual projection, object selection and navigation.
- Shared Rust application shell and a separate Makepad launcher executable.
- Local engines; no cloud account is required by the document engine layer.

Capabilities vary by format and document contents. The list describes source capabilities, not universal format fidelity or fresh GUI acceptance of this publication.

## Project status

**Active development / experimental builds. PARALLEL CYCLE 2 — PARTIAL.**

Known limitations remain visible:

- Editing and persistence coverage is incomplete; complete DOCX round-trip is not promised.
- PDF dense-vector rendering closure is incomplete; complex pages may hit controlled budgets.
- Writer focus and end-to-end editing acceptance is ongoing.
- Sheets Copy/Cut/Paste GUI acceptance is incomplete.
- Full PowerPoint editing and complete Excel compatibility are not claimed.
- Production release acceptance has not been completed.
- The recent browser experiment is separate and partial; this native source publication does not confer browser render/IME acceptance.

## Architecture

```text
Rudra Office
├── Writer   (writer/crates)
├── Sheets   (sheets/crates)
├── Slides   (slides/crates)
├── PDF      (pdf/crates)
└── Shared Application / Makepad Toolkit
    ├── workspace/crates/rudra-office-app
    ├── workspace/crates/rudra-office-launcher
    └── workspace/crates/office-ui-runtime (compatibility facade)
```

[Module architecture](docs/MODULAR_ARCHITECTURE.md) · [Launcher](docs/LAUNCHER.md)

First-party runtime/session code remains in the app; generic EntityId/Registry and reusable canvas primitives remain in the separate [Rust Makepad Toolkit](https://github.com/paulgreenbox-beep/rust-makepad-toolkit). Toolkit and Makepad are not duplicated in this repository.

## Build

See [BUILDING.md](docs/BUILDING.md) for the Windows toolchain, exact dependency boundary and limitations. Expected layout:

```text
Dev/
├── RudraOffice/
└── Rust_Makepad_Toolkit/
    └── .deps/makepad/
```

After an explicitly prepared compatible dependency checkout:

```powershell
cd RudraOffice/workspace
cargo build --locked -j 1 -p rudra-office-app -p rudra-office-launcher
```

**A clean public checkout is not yet build-qualified.** The development Toolkit checkout has unpublished changes; its verification-economy core is absent from the pinned public tree. The local Makepad junction has no independent upstream Git identity. Do not treat the command above as a verified portable bootstrap or omit these blockers.

## Dependencies

- Rust Makepad Toolkit: public base `c3b168ba10255ba644bd3a0d91524f3cd23cbf6b`, with local-development divergence documented in BUILDING.
- Makepad: upstream dependency bootstrapped by Toolkit; public bootstrap default `105158e08c89d47f57af4621420ed68eb3a147ce` is not asserted identical to the operator's local SDK.
- Vendored Rust library snapshots: IronCalc, Office Oxide and PDFOxide, including existing integration modifications and original licensing.

## Testing and verification

Cheapest available component checks (from this root, after dependency resolution):

```powershell
cargo test --locked --offline --manifest-path writer/Cargo.toml -p writer-core -j 1
cargo test --locked --offline --manifest-path sheets/Cargo.toml -p sheets-api -j 1
cargo test --locked --offline --manifest-path slides/Cargo.toml -p slides-api -j 1
```

Offline requires a previously populated Cargo cache. The full tests include private or missing acceptance fixtures referenced by `include_bytes!`; those test targets are intentionally not reported green and can fail compilation without the operator-owned corpus. Test code/assertions were retained, not silently removed or weakened. See [private fixture boundary](docs/PRIVATE_FIXTURES.md).

Verification policy and wrappers are included under `verification/` and `tools/`. Public GUI recipes contain sanitized example paths and historical artifact identities, not an accepted runnable public GUI baseline. The exact verification core/hash pins are retained; no substitute validator was added. There is no knowingly broken mandatory GitHub Actions workflow.

## License

First-party Rudra Office code is available under the [MIT License](LICENSE). Third-party sources/assets retain their own licenses and copyrights; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). This technical inventory is not legal approval of all transitive dependencies.

## Source-control baseline

See [SOURCE_CONTROL_BASELINE.md](docs/SOURCE_CONTROL_BASELINE.md) and the [publication report](reports/github-publication-2026-10-27.md). This publication uses a fresh source snapshot, not the private nested development histories.
