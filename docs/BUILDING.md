# Building Rudra Office

## Scope and current reproducibility

This is a source baseline, not an accepted executable release. The original publication had no clean public Windows build. A subsequent candidate qualification now passes as recorded below; source review, native GUI and release acceptance remain separate. Original development files were not changed to create this repository.

### Qualified public build candidate (2026-10-27)

[Windows run 40567876295](https://github.com/paulgreenbox-beep/RudraOffice/actions/runs/40567876295) passed full locked dependency resolution and `cargo build --locked -j 1 -p rudra-office-app -p rudra-office-launcher` on a fresh Windows runner:

- Office: `a20df27649a8faa770ac47c4dbaf4432c21d221a` (PR #1, not main).
- Toolkit: `a3452ac231a2b61d532739203fd1d2f2e3de4e8c` (Toolkit PR #1, pending owner review/merge).
- Official Makepad: `105158e08c89d47f57af4621420ed68eb3a147ce`.
- Rust/Cargo: `1.97.1`, MSVC x64; Node 22 for source verification.
- Workspace Cargo.lock SHA-256: `1427F7CC9E1BAF6989A43C3B9D4752D0458C2C539B5E1288BDE441D6D11FDCA0`.
- Actual compilation time: 9m45s.
- App EXE SHA-256: `6BD5C4542353393FE284FE94489C9223A12274319B1FEE6D136BD04B0F4CD809`.
- Launcher EXE SHA-256: `841310E95C389E07611D726D905128F912FE98FFD0B1EEEBE2D87C7F5F9E8266`.

These hashes describe diagnostic runner outputs, not uploaded binaries or a release. That run wrote generated Cargo metadata inside its source tree; a clean artifact source-state label is not asserted. Subsequent qualification writes metadata outside the tree. No GUI operation, app test, strict Clippy or complete corpus test was proved by this build alone. Main/public pins are not automatically advanced by a successful candidate build.

### Foundation merge qualification (2026-10-28)

Toolkit PR #1 was merged at `0b01942f1a7d1c276f130c9db03146cb96c1718a`, Office PR #1 at `42de15966bbee1eb93b6faf6598b00a79f45e7ef`, and Office PR #2 at `91c8ace8b91bea1c8b7ebd99d3238e4201c73015`. The Toolkit merge tree equals the reviewed `a3452ac231a2b61d532739203fd1d2f2e3de4e8c` tree. The exact reviewed Toolkit/Makepad pins are retained; a merge commit is not an automatic dependency upgrade. These merges do not retroactively qualify their main artifacts using historical PR-head runs.

PR #3 now checks out `${{ github.sha }}`: the actual GitHub synthetic merge ref for pull requests, and the actual main commit for pushes. Both the reviewed merge-compatible run and a separate fresh final-main run are required before claiming the integrated source/build baseline. This document does not predeclare either run successful. Expected source checks are now 86 (three additive impact-policy regressions, including isolated Writer API source at a known baseline without caller tags); the 812 Rust passes are preserved, with five explicit private-fixture ignores, including the restored private PPTX opt-in. Native input/rendering, expanded L4b compositions and release gates remain separate.

## RDO-001 public dependency admission

The source-only bootstrap contract is `verification/public-dependencies.json`.
Run the read-only preflight before a public Windows build:

```powershell
node tools/public-dependency-verification.mjs
node --test tools/public-dependency-verification.test.mjs
```

A blocked result is exit 2 with concrete issues; it is not a passing build.
The preflight checks independent Git roots (never an enclosing repository's
HEAD), reviewed exact revisions, repository origins, clean trees including
hidden index flags, physical path containment, required files and all seven
existing Verification Core integrity pins. It neither repairs dependencies nor
changes the authoritative `verification/toolkit-core.json` binding.

**The exact source/build profile is now owner-reviewed.** The observed public
Toolkit main is now `0b01942f1a7d1c276f130c9db03146cb96c1718a`; it contains
the exact Core modules reviewed at `a3452ac231a2b61d532739203fd1d2f2e3de4e8c`.
The historical candidate description above remains a record of that earlier run, not the present PR state. Owner approval remains restricted to source/bootstrap and Windows build. Matching package version `2.0.0` alone is not SDK qualification. The owner separately reported D3D11 shader failures in other applications using this SDK; no Office runtime result is inferred, and native render/input acceptance remains required.

The reviewed contract permits explicit initial network bootstrap:

```powershell
node tools/public-dependency-verification.mjs --bootstrap
```

Unreviewed contracts still refuse **before network or filesystem changes**. A reviewed bootstrap can create only missing dependency checkouts and validates them; an existing
dirty, mismatched, externally linked or incomplete checkout is rejected and
left intact. Failed newly created directories are retained for inspection,
not silently deleted or overwritten. There is no download in a Cargo build
script and no source from the local development Toolkit is copied by this tool.
Git operations use command-local `core.longpaths=true` for Windows paths; no
global Git/OS setting is changed. Prefer a short checkout root such as `C:/Dev`.

The `Public dependency contract tests` Actions workflow runs only these cheap
Node/source-admission regressions on Windows/Linux. It is deliberately **not**
the full Windows app/launcher CI or a GUI/release gate. A full `windows-ci.yml`
also needs compilable public test fixtures and owning-component qualification.
The job also checks existing consumer policy tests against the exact public
Core candidate above. The missing `verification/inputs/RC28-closure.json` is
restored as first-party planning data: it contains historical PARTIAL hashes
and relative report paths, not private documents or a new accepted run.

A separate **manual candidate qualification**, not a required product CI check,
can be requested by a maintainer applying `public-windows-qualification` to a
same-repository PR. It uses the exact public Toolkit/Makepad candidates and
isolated Rust 1.97.1 on a fresh Windows runner. Identity and resource checks and
locked metadata precede the locked app/launcher build. A failed run records the
first unresolved compatibility boundary; it does not qualify the proposed SDK.
No desktop GUI or EXE upload/release is performed. Reviewed production pins
are recorded only for the approved source/build scope, not native GUI or release.

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
git -C Rust_Makepad_Toolkit checkout --detach a3452ac231a2b61d532739203fd1d2f2e3de4e8c
```

The public Toolkit base contains rudra-runtime, makepad-toolkit-core, makepad-toolkit-widgets and toolkit-windows-automation. The local development copy has unpublished modifications; these are not copied into RudraOffice and the base SHA is not a claim of byte-identical dependencies.

## Makepad bootstrap

Toolkit provides its own explicit bootstrap (initial setup requires network):

```powershell
cd Rust_Makepad_Toolkit
./tools/bootstrap-makepad.ps1 -Revision 105158e08c89d47f57af4621420ed68eb3a147ce
```

This exact revision passed the public source/build qualification above, not native GUI acceptance. The development junction points to an operator-managed SDK directory with no independent `.git`; an enclosing unrelated repository's HEAD must not be called a Makepad revision. The old unused RudraOffice vendor/makepad commit is not the current application's dependency either. No SDK fork or hidden build-script download is introduced here.

The operator can use the Toolkit script's `-MakepadPath` option to an existing compatible checkout. Its source identity must be separately verified. Do not overwrite an existing dependency link, change it to another source without review, or assume that any Makepad 2.0 package is compatible.

## Native application

From an environment with compatible dependencies:

```powershell
cd RudraOffice/workspace
cargo metadata --locked --offline --format-version 1
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

Start with a small pure package rather than a full app rebuild. PR #2 provides eight deterministic public fixtures and preserves private fidelity assertions under explicit opt-in modes; see [PRIVATE_FIXTURES.md](PRIVATE_FIXTURES.md) and [fixture provenance](../fixtures/public/README.md). Default Writer/DOCX, PDF and Slides test targets no longer include missing private document bytes. App layout/font/header tests use the synthetic 4-row/3-column Writer document; operator DOCX/JPEG acceptance is explicitly ignored, not silently passed. Cargo and rendering results still require actual run receipts.

The reviewed Toolkit PR revision contains all seven exact Core modules expected by `tools/toolkit-verification-core.mjs`; Toolkit main did not contain them at the audited baseline. Version/module hashes remain unchanged and fail closed. This is an explicit public dependency selection, not permission to use arbitrary dirty Toolkit source, weaken the validator or forge receipts.

`Windows public candidate CI` (PR #3) checks the exact GitHub merge-ref bytes for pull requests and main bytes for pushes: source/Core/provenance tests, first-party formatting, owning-workspace locked tests, app/launcher integration, strict Clippy and locked diagnostic build. Historical head-only runs are not reused as current merge/main execution. No desktop GUI or release qualification is implied. CI failures remain real failures; branch protection is withheld until a stable green baseline and merge checks are reviewed.

GUI recipes have sanitized sample installation paths and omitted real-user corpus. They need explicit operator binding and fresh source/artifact/runtime proof before execution. Neither old RC hashes nor source publication establishes GUI acceptance. Native input remains blocked by the read-only canary. Both expanded L4b compositions and frozen-artifact L5 remain unaccepted.
