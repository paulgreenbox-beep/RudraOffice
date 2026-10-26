# Building Rudra Office

## Scope and current reproducibility

This is a source baseline, not an accepted executable release. No clean-machine/public-only Windows build was verified during publication. Full app and verification bootstrap has known blockers listed below. Original development files were not changed to create this repository.

## Windows requirements

- Windows 10/11, x64, Git and PowerShell.
- Rust via rustup, stable MSVC host (`x86_64-pc-windows-msvc`), with Cargo.
- Visual Studio 2022 Build Tools: Desktop development with C++, MSVC v143 and Windows SDK.
- The vendored engines declare Rust 1.88 or newer; a compatible stable toolchain supporting edition 2024 is necessary. No lower MSRV is promised for the complete app.
- Node.js for existing verification wrappers. Python generators have additional optional dependencies and are not a compiler prerequisite.

Record `rustc -Vv`, `cargo -V` and the SDK revision with any real build receipt. Do not install a new default toolchain or silently change global settings merely to run these commands.

## Checkout topology

The Cargo path contract uses sibling directories, independent of a user profile:

```text
C:/Dev/
  RudraOffice/
  Rust_Makepad_Toolkit/
    .deps/makepad/widgets/Cargo.toml
```

```powershell
git clone https://github.com/paulgreenbox-beep/RudraOffice.git
git clone https://github.com/paulgreenbox-beep/rust-makepad-toolkit.git Rust_Makepad_Toolkit
git -C Rust_Makepad_Toolkit checkout --detach c3b168ba10255ba644bd3a0d91524f3cd23cbf6b
```

The public Toolkit base contains rudra-runtime, makepad-toolkit-core, makepad-toolkit-widgets and toolkit-windows-automation. The local development copy has unpublished modifications; these are not copied into RudraOffice and the base SHA is not a claim of byte-identical dependencies.

## Makepad bootstrap

Toolkit provides its own explicit bootstrap (initial setup requires network):

```powershell
cd Rust_Makepad_Toolkit
./tools/bootstrap-makepad.ps1 -Revision 105158e08c89d47f57af4621420ed68eb3a147ce
```

This is the public script's recorded default, not a RudraOffice compatibility qualification. The development junction points to an operator-managed SDK directory with no independent `.git`; an enclosing unrelated repository's HEAD must not be called a Makepad revision. The old unused RudraOffice vendor/makepad commit is not the current application's dependency either. No SDK fork or hidden build-script download is introduced here.

The operator can use the Toolkit script's `-MakepadPath` option to an existing compatible checkout. Its source identity must be separately verified. Do not overwrite an existing dependency link, change it to another source without review, or assume that any Makepad 2.0 package is compatible.

## Native application

From an environment with compatible dependencies:

```powershell
cd RudraOffice/workspace
cargo metadata --locked --offline --no-deps --format-version 1
cargo build --locked -j 1 -p rudra-office-app -p rudra-office-launcher
```

When the exact lockfile dependencies are already cached, add `--offline` to build. Offline mode is not a promise that a fresh machine has the cache.

Expected debug outputs:

```text
RudraOffice/workspace/target/debug/rudra-office-app.exe
RudraOffice/workspace/target/debug/rudra-office-launcher.exe
```

Keep them together: the launcher resolves its sibling editor. `cargo run --locked -p rudra-office-app` launches the editor; `cargo run --locked -p rudra-office-launcher` launches Home. Outputs are diagnostic/debug candidates, not production release artifacts.

## Engines and workspaces

- workspace/Cargo.toml: app, launcher and office-ui-runtime facade.
- writer/Cargo.toml: Writer core/API/DOCX/UI adapter packages.
- sheets/Cargo.toml: Sheets API/core/grid/UI/XLSX/IronCalc adapter.
- slides/Cargo.toml: Slides API/core/canvas/UI/PPTX adapter.
- pdf/crates/pdf-core and pdf-oxide-adapter: existing standalone manifests/lockfiles.

All original first-party Cargo.lock files are retained. This publication does not mass-regenerate them. Engine source lives under vendor/ironcalc, vendor/office-oxide and vendor/pdf-oxide; original library licenses remain. The workspace crates.io Office Oxide patch intentionally keeps a single Office Oxide source/FFI identity.

## Verification and fixture limitations

Start with a small pure package rather than a full app rebuild. The README shows the available focused commands. Full Writer DOCX, PDF adapter and Slides adapter tests reference private or missing binary fixtures; these are not included for privacy/provenance reasons. This is a documented test-compilation limitation, not passing/skipped GUI proof. Assertions are unchanged.

The pinned public Toolkit tree does not contain the development `verification-economy/` core expected by tools/toolkit-verification-core.mjs. The retained exact version/module hashes fail closed if missing or mismatched. An explicitly published compatible Toolkit/core revision is needed before these wrappers can work from public dependencies. Do not remove the validator, weaken hashes or forge receipts.

GUI recipes have sanitized sample installation paths and omitted real-user corpus. They need explicit operator binding and fresh source/artifact/runtime proof before execution. Neither old RC hashes nor source publication establishes GUI acceptance. No full Windows CI workflow is added until this bootstrap is reproducible.
