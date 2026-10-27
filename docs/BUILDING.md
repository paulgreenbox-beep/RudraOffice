# Building Rudra Office

## Scope and current reproducibility

This is a source baseline, not an accepted executable release. No clean-machine/public-only Windows build was verified during publication. Full app and verification bootstrap has known blockers listed below. Original development files were not changed to create this repository.

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

**The contract is currently `pending_owner_review`.** The observed public
Toolkit main is `5ae780a75060f00e583500490782a6ebec35b595`; it does not contain
Verification Core. Owner-proposed Toolkit PR #1 provides those exact Core
modules at `a3452ac231a2b61d532739203fd1d2f2e3de4e8c`. This is a review candidate,
not an automatically approved or merged Office dependency. The Makepad pin
below is likewise only a compatibility candidate. Matching package version
`2.0.0` is not SDK qualification.

Once owner/consumer review records both exact compatible revisions in the
contract, an explicit initial network bootstrap can be requested:

```powershell
node tools/public-dependency-verification.mjs --bootstrap
```

Until then it refuses **before network or filesystem changes**. Afterwards it
can create only missing dependency checkouts and validates them; an existing
dirty, mismatched, externally linked or incomplete checkout is rejected and
left intact. Failed newly created directories are retained for inspection,
not silently deleted or overwritten. There is no download in a Cargo build
script and no source from the local development Toolkit is copied by this tool.
Git operations use command-local `core.longpaths=true` for Windows paths; no
global Git/OS setting is changed. Prefer a short checkout root such as `C:/Dev`.

The `Public dependency contract tests` Actions workflow runs only these cheap
Node/source-admission regressions on Windows/Linux. It is deliberately **not**
the full Windows app/launcher CI or a GUI/release gate. Full `windows-ci.yml`
remains blocked until actual public SDK/build qualification succeeds.
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
remain unset until consumer review and actual qualification establish them.

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
