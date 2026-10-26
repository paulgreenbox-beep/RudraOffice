# Rudra Slides — Progress Report

**Date:** 2026-08-02  
**Status:** **VERIFIED** — Current continuation complete; no commits yet on master.  
**Branch:** `master` (no commits yet)

---
## Update — 2026-08-09 (T9: public limits adapter + deterministic panic fence + regression coverage)

This update adds the security-hardening layer for the Office Oxide PPTX adapter.

- **Public limits-carrying adapter:** `office_oxide_adapter_with_limits(limits: Limits)` is now `pub` in `src/lib.rs`; `office_oxide_adapter()` delegates with `Limits::default()`. All `limits()` accessors (PresentationBackend / PptxAdapter / CodecBackend) are wired through `limits.rs`.
- **Deterministic panic fence at the real-codec boundary:** `OfficeOxideCodec::decode` runs the entire real OOXML path — `office_oxide::Document::from_reader`, `to_ir`, slide-count enforcement, section/shape conversion (`push_positioned_elements`, text/image push, `presentation.validate()`) — inside one `std::panic::catch_unwind`. Any unwinding panic is converted to a generic `EngineError::CodecError("office-oxide codec panicked while decoding PPTX")`; the panic payload is never exposed. `encode` is unaffected.
- **Regression coverage (3 groups):**
  1. Engine behind trait object: `office_oxide_engine_works_behind_trait_object` — load/save via `&dyn PresentationEngine`.
  2. Unsupported-image fallback: `unsupported_gif_image_decodes_as_fallback_text` — a 1×1 GIF89a written through `PptxWriter` (`IrImageFormat::Gif`) decodes to exactly one `ObjectKind::Text` whose flattened text is `"[Image]"`.
  3. Hostile/truncation determinism: 256-seed hostile byte corpus + every truncation `0..len` of a valid output through public `load` — asserts no unwind (`catch_unwind` is `Ok`), deterministic repeated result, and (for truncations) rejection with `Err`.
- **Verification gates:** `cargo fmt --all` clean; `cargo test -p slides-pptx-adapter` — **58 passed, 0 failed** (48 unit + 10 integration); `cargo clippy -p slides-pptx-adapter --all-targets -- -D warnings` clean (clippy required collapsing one pre-existing nested `if let` in `push_positioned_elements`).

Changed files (T9): `src/lib.rs`, `src/backend.rs`, `src/limits.rs`, `src/office_oxide_codec.rs`, `tests/adapter_contract.rs`.

**Security-audit answers (architect inbox 2026-08-09T07:22Z):**
- **(a) Deterministic `catch_unwind` conversion at the codec boundary — IMPLEMENTED.** Panics from the vendored Office Oxide engine on hostile bytes map to a generic `EngineError::CodecError`; deterministic and payload-free.
- **(b) Central-directory / decompressed-size preflight — NOT IMPLEMENTED.** `Limits::max_input_bytes` bounds only the compressed input slice; `Document::from_reader` decompresses internally and `to_ir` materializes the package in memory, so a crafted ZIP with small compressed size can still inflate to a much larger working set (zip-bomb). This residual risk is the assigned follow-up guard (tracked in T1.5).

## Update — 2026-08-05

This update supersedes the 2026-08-04 adapter summary where it conflicts.

- The real Office Oxide `.pptx` adapter now preserves positioned **PNG, JPEG and BMP** images, including their binary payload and geometry, in addition to the already supported editable text subset.
- Image alternative text is never silently lost: export returns an explicit unsupported-capability error until the underlying writer can serialize it.
- GIF/TIFF/EMF/WMF use a readable fallback. Shapes, complex tables, notes, transitions, transforms, themes/masters and chart fidelity remain outside the supported editable subset.
- Verification: `cargo test --workspace --locked` — **108 passed, 0 failed**.

## Update — 2026-08-04

This report predates the production OOXML adapter. The current code now provides `office_oxide_adapter()` in `slides-pptx-adapter`: it reads and writes real ZIP/XML `.pptx` packages through the vendored Office Oxide engine. The supported editable subset is slide structure and text. Unsupported objects (images, tables, shapes, rich text, notes, transitions, transforms) are rejected explicitly rather than silently discarded. `BuiltinCodec`/`RSLD001` remains for deterministic compatibility tests.

Verified after the change: `cargo check -p slides-pptx-adapter` and `cargo test -p slides-pptx-adapter` pass (49 tests). The remaining sections describe the older architecture; the real-PPTX gap is therefore **partially closed**, not fully solved.
## Summary

The workspace implements the core architectural layer for a PowerPoint-compatible presentation engine, structured as five Rust 2024 edition crates (`slides-api`, `slides-core`, `slides-canvas`, `slides-pptx-adapter`, `slides-ui`), std-only, no external dependencies beyond the standard library. All crates compile, all 103 tests pass, clippy is clean with `-D warnings`, and `cargo fmt --check` passes. The implementation is **architecture-validated**: the document model, deterministic command/event machine, canvas interaction logic (including rotation in hit-testing, bounds, and selection handles), headless UI composition with core delegation, and a bounded internal codec (`RSLD001`) are complete and tested. **No real GPU rendering (Makepad)** and **no real OOXML/PPTX I/O** exist.

---

## Completed Per Crate

### `slides-api` (frozen public contract)
- `DEFAULT_SLIDE_SIZE` constant: 1280×720.
- `Slide.size: Size` field present; `Slide.notes: Option<String>` field present.
- `Transform.rotation_degrees: f32` persisted in the frozen model (no longer core-only map).
- Zero IDs rejected at validation layer.
- Geometry primitives, color, text model, object payloads, slide metadata, document, errors, engine boundary traits all as before.
- **Tests:** 10 unit tests pass.

### `slides-core` (deterministic editing state machine)
- `SetSlideSize` and `UpdateNotes` commands and corresponding events added.
- Persistent rotation stored in `SlideObject.transform.rotation_degrees` (not a separate map).
- Atomic editor via clone→mutate→validate→commit; clipboard with editor rotation preserved.
- Deletion emits `SelectionChanged` when selection is affected.
- Deterministic ID allocation, typed errors, selection invariants.
- **Tests:** 12 unit tests pass.

### `slides-canvas` (headless GPU canvas interaction logic)
- Transform rotation now affects: transformed bounds, corner computation, selection handle positions, and hit-testing (body, resize handles, rotate handle).
- Viewport, CanvasHost, CanvasLayer, interaction state machine, hit-testing, snapping/guides, render command traits all functional headless.
- **Tests:** 19 integration tests pass. No Makepad/GPU dependency.

### `slides-pptx-adapter` (bounded engine façade)
- Still `RSLD001` internal subset only: plain one-run text, rectangle, ellipse.
- Explicitly rejects at encode: custom slide size, notes, any non-identity transform/rotation, and all other unsupported model states (rich text, images, lines, tables, rounded rectangles, styling, transitions, non-blank layouts, etc.).
- `Limits` validates 31-byte minimum record and 7..=12 record remainder.
- Decoder avoids count-derived `Vec` preallocation; all malformed input deterministically rejected without panic.
- **Tests:** 42 unit + 6 integration tests pass. No real OOXML/PPTX.

### `slides-ui` (headless UI composition)
- Direct `dispatch_to_core` using `slides-core` types (no parallel command DTO).
- Persisted notes draft sync between UI and core.
- Selected slide preserved on `sync_from_document`.
- UI-atomic failure semantics (local actions either fully apply or rollback).
- Thumbnails, ribbon, properties, notes, canvas host state, zoom, slideshow controller.
- **Tests:** 14 unit tests pass.

---

## Verification Snapshot (fresh evidence, executed 2026-08-02)

| Check | Result |
|-------|--------|
| `cargo fmt --all -- --check` | **PASS** |
| `cargo check --workspace --all-targets` | **PASS** |
| `cargo test --workspace` | **103 passed, 0 failed** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **PASS** (no warnings) |

Breakdown: `slides-api` 10, `slides-core` 12, `slides-canvas` 19, `slides-pptx-adapter` 48 (42 unit + 6 integration), `slides-ui` 14.

---

## Remaining Gaps (honest, not overclaimed)

| Area | Status | Notes |
|------|--------|-------|
| **Makepad GPU canvas** | ❌ Not started | `slides-canvas` is logic-only; no `makepad` crate, no widget, no GPU draw calls, no font atlas, no text shaping. |
| **Real PPTX I/O (OOXML)** | 🟡 Text/structure subset | `office_oxide_adapter()` reads and writes real PPTX ZIP/XML through the vendored Office Oxide engine. Images, tables, complex shapes, rich text, notes, transitions, themes/masters, charts and compatibility behavior remain explicitly unsupported. |
| **Codec coverage for rich objects/metadata** | ❌ Missing | BuiltinCodec rejects images, lines, tables, rounded rectangles, multi-paragraph, multi-run, formatting, custom slide size, notes, transforms/rotation. |
| **Architecture docs / CI** | ❌ Absent | Only `docs/TZ.md` exists. No ADR, no module layout, no trait contract diagrams, no GitHub Actions. |
| **Baseline hygiene** | ❌ Uncommitted/untracked | No commits on `master`; `target/`, `*.log` untracked; missing `.gitignore`; `Cargo.lock` not committed. |

> **Do not say slide size/notes are missing** — they are present in `slides-api` `Slide` and wired through core commands/events and UI sync.

> **No production readiness claimed.** This is a validated architectural skeleton ready for backend/renderer integration behind existing ports.

---

## Recommended Next Steps (prioritized)

1. **Baseline hygiene** — `git add -A && git commit -m "chore: baseline architecture (api, core, canvas, adapter, ui)"`; add `.gitignore` (ignore `target/`, `*.log`, `.mimocode/node_modules/`); **commit `Cargo.lock`** for reproducible builds.
2. **Architecture docs & CI** — Write `docs/ARCHITECTURE.md` (crate graph, trait boundaries, data flow), `docs/ADR/0001-trait-boundaries.md`, `docs/ADR/0002-codec-strategy.md`; add GitHub Actions: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`.
3. **Renderer behind existing port** — New crate `slides-canvas-makepad` (or feature flag) implementing `CanvasHost` + `RenderBackend` using Makepad widgets, GPU text, selection handles rendering; `slides-canvas` stays headless logic.
4. **PPTX backend behind existing port** — Evaluate `ppt-rs` vs `msoffice-pptx-rs`; implement `PresentationBackend`/`PresentationCodec`; keep `RSLD001` for deterministic tests.
5. **Extend codec coverage** — Rich text (multi-paragraph, multi-run, per-run formatting), images, lines, tables, rounded rectangles, styling, custom slide size, notes, transforms/rotation — behind the same `PresentationCodec` trait.

---

## Distinction: Architecture vs. Real Rendering / Real PPTX

| Layer | Implemented | Not Implemented |
|-------|-------------|-----------------|
| Document model & validation (size, notes, rotation in Transform) | ✅ `slides-api` | — |
| Deterministic editing commands/events (SetSlideSize, UpdateNotes, rotation persisted) | ✅ `slides-core` | — |
| Canvas interaction math (hit-test, snap, guides, viewport, **rotation-aware bounds/handles**) | ✅ `slides-canvas` | GPU draw, font shaping, widget tree, Makepad integration |
| Headless UI composition (thumbnails, ribbon, panels, slideshow, **notes draft sync, selection preserve**) | ✅ `slides-ui` | Native window, OS menubar, system clipboard, accessibility |
| Internal interchange codec (`RSLD001` bounded subset) | ✅ `slides-pptx-adapter` | Real OOXML/PPTX read/write, ZIP, themes, masters, images, tables, charts, animations, fonts, rich text |

(End of file)