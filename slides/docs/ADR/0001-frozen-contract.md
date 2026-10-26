# ADR 0001: Frozen Contract — slides-ui Viewer, Engine Loading, Render Plans, and OfficeOxide Decode

**Status:** Frozen (no public API alterations)  
**Date:** 2026-08-09  
**Supersedes:** None  
**Related:** PROGRESS.md T9 (adapter hardening), T1.5 (ZIP-bomb follow-up)

---

## Summary

This ADR documents the frozen architectural contract between `slides-ui`, `slides-api`, `slides-canvas`, and `slides-pptx-adapter` as of the current continuation (T9). It captures four interlocked invariants that must not be altered without a new ADR:

1. **slides-ui viewer is renderer-neutral and read-only** — it owns a cloned immutable `Presentation` snapshot and transient navigation/view state; core/host retains mutation.
2. **Loading is engine-hosted through `PresentationEngine`** with exact typed `EngineError` propagation; the adapter is forbidden as a normal `slides-ui` dependency (only prospective dev-dependency for real integration tests).
3. **Render plans use `slides-canvas` `RenderCommand`/`Viewport` authority** — backend/GPU is external.
4. **OfficeOxide decode invokes `Document::from_reader`/`to_ir` under `catch_unwind`** and error maps deterministically to `EngineError::CodecError` without payload; limits cap **compressed input** and **post-decode model size**, NOT decompressed package/ZIP resource usage — **no zip-bomb mitigation is claimed**; residual risk is explicitly stated with follow-up tracked in T1.5.

---

## 1. slides-ui Viewer: Renderer-Neutral, Read-Only, Snapshot-Owning

### Contract

`slides-ui` (`crates/slides-ui/src/lib.rs`) is the headless UI composition layer. It:

- **Owns a cloned immutable `Presentation` snapshot** (`EditorUi.document: Presentation` — line 691). The snapshot is replaced wholesale by `EditorUi::from_document` (line 720) and `EditorUi::sync_from_document` (line 800). The UI never keeps a mutable handle on `slides-core`.
- **Owns transient navigation/view state only**: `ThumbnailList`, `CanvasHostState`, `PropertiesPanelState`, `NotesPanelState`, `RibbonState`, `StatusBarState`, `ZoomControl`, `SlideshowController` — all derived from the snapshot.
- **Is read-only by default for loaded documents**: `EditorUi::from_pptx` (line 763) and `replace_from_pptx` (line 778) set `read_only = true` (line 769). The `read_only` flag blocks any `UiAction.is_core_command()` via `DispatchError::Ui(UiError::ReadOnly)` (lines 829–830, 857–858).
- **Routes mutations exclusively through `EditorCommandRouter`** (trait at line 232) or `dispatch_to_core` (line 852) which maps `UiAction` → exact `EditorCommand` (line 887) and applies through `PresentationEditor::apply` (atomic clone-and-commit). On failure, both editor and UI are restored byte-for-byte from pre-mutation clones (lines 864–875).
- **Has no Makepad dependency** (crate doc line 9). A Makepad host implements `CanvasHostExtension` (trait at line 244) at the application boundary — methods only synchronize view state (`set_slide`, `set_zoom`, `request_repaint`); the renderer must not own presentation mutation or invent a second editor command model.

### Key Source Paths

| Concept | Source |
|---------|--------|
| `EditorUi` document snapshot | `crates/slides-ui/src/lib.rs:691` |
| `from_document` / `sync_from_document` | `crates/slides-ui/src/lib.rs:720, 800` |
| `read_only` flag + enforcement | `crates/slides-ui/src/lib.rs:769, 829–830, 857–858` |
| `EditorCommandRouter` trait | `crates/slides-ui/src/lib.rs:232–238` |
| `dispatch_to_core` atomic semantics | `crates/slides-ui/src/lib.rs:852–882` |
| `map_to_core_command` exact mapping | `crates/slides-ui/src/lib.rs:887–938` |
| `CanvasHostExtension` boundary | `crates/slides-ui/src/lib.rs:244–251` |

---

## 2. Loading: Engine-Hosted via `PresentationEngine` with Typed `EngineError`

### Contract

- **All loading goes through `PresentationEngine`** (trait at `crates/slides-api/src/lib.rs:1051–1067`):
  - `load(&self, bytes: &[u8]) -> Result<Presentation, EngineError>`
  - `save(&self, presentation: &Presentation) -> Result<Vec<u8>, EngineError>`
  - `capabilities() -> &[Capability]`
- **Exact typed `EngineError` propagation** (`crates/slides-api/src/lib.rs:996–1020`):
  - `InvalidInput(String)` — input bytes or model values invalid
  - `InvalidFormat` — unrecognized by engine
  - `UnsupportedCapability(String)` — capability not implemented
  - `LimitExceeded { limit, actual, context }` — bounded engine limit exceeded
  - `Utf8Error` — invalid UTF-8 where text required
  - `ReferenceIntegrity(String)` — document reference failed integrity
  - `CodecError(String)` — codec-specific failure (used by OfficeOxide panic fence)
  - `ValidationError(ValidationError)` — model validation failed before serialization
- **`slides-pptx-adapter` exposes `PptxAdapter<B: PresentationBackend>`** implementing `PresentationEngine` (`crates/slides-pptx-adapter/src/backend.rs:127–139`).
- **Adapter is forbidden as a normal `slides-ui` dependency**: `slides-ui/Cargo.toml` (lines 7–10) depends on `slides-api`, `slides-canvas`, `slides-core` only. The adapter may only appear as a **prospective dev-dependency** for real integration tests (not yet present in workspace).

### Key Source Paths

| Concept | Source |
|---------|--------|
| `PresentationEngine` trait | `crates/slides-api/src/lib.rs:1051–1067` |
| `EngineError` enum (all variants) | `crates/slides-api/src/lib.rs:996–1020` |
| `PptxAdapter` implements `PresentationEngine` | `crates/slides-pptx-adapter/src/backend.rs:127–139` |
| `slides-ui` Cargo.toml (no adapter) | `crates/slides-ui/Cargo.toml:7–10` |

---

## 3. Render Plans: `slides-canvas` `RenderCommand` / `Viewport` Authority

### Contract

- **Render plans are renderer-neutral** (`RenderPlan` at `crates/slides-ui/src/view.rs:218–231`):
  - `slide_id`, `slide_size`, `viewport: Viewport`, `commands: Vec<RenderCommand>`, `warnings: Vec<RenderWarning>`
  - Validation: `RenderPlan::validate()` checks `slide_size` strictly positive/finite and every `RenderCommand::is_valid()` (lines 238–244)
- **`RenderCommand` is the GPU-neutral deterministic command list** (`crates/slides-canvas/src/render.rs:11–46`):
  - Variants: `Clear`, `FillRect`, `StrokeRect`, `DrawLine`, `DrawText`, `DrawImage`, `PushClip`, `PopClip`
  - `RenderCommand::is_valid()` checks finiteness, non-negative dimensions, valid formats, non-empty image bytes
- **`Viewport` is transient canvas state only** (`crates/slides-canvas/src/types.rs:159–163`): wraps `Transform` with zoom/pan/clamp/fit helpers
- **Backend/GPU is external**: `RenderBackend` trait (`crates/slides-canvas/src/render.rs:111–119`) with `create_encoder` and `present(encoder) -> Result<Vec<RenderCommand>, RenderBackendError>` — the backend is implemented by the host (e.g., future Makepad crate), not by `slides-canvas` or `slides-ui`.

### Key Source Paths

| Concept | Source |
|---------|--------|
| `RenderPlan` | `crates/slides-ui/src/view.rs:218–231` |
| `RenderPlan::validate` | `crates/slides-ui/src/view.rs:238–244` |
| `RenderCommand` enum + `is_valid` | `crates/slides-canvas/src/render.rs:11–100` |
| `Viewport` | `crates/slides-canvas/src/types.rs:159–288` |
| `RenderBackend` trait | `crates/slides-canvas/src/render.rs:111–119` |

---

## 4. OfficeOxide Decode: `catch_unwind` Fence, Deterministic `CodecError`, Limits Scope, Residual ZIP Risk

### Contract

**Panic fence** (`crates/slides-pptx-adapter/src/office_oxide_codec.rs:56–149`):

```rust
std::panic::catch_unwind(|| {
    let document = Document::from_reader(Cursor::new(bytes.to_vec()), DocumentFormat::Pptx)
        .map_err(|error| EngineError::CodecError(format!("office-oxide could not read PPTX: {error}")))?;
    let ir = document.to_ir();
    // ... slide count enforcement, section/shape conversion, validation
    Ok(presentation)
})
.map_err(|_| EngineError::CodecError("office-oxide codec panicked while decoding PPTX".to_string()))?
```

- The **entire real OOXML path** — `Document::from_reader`, `to_ir`, slide-count enforcement, section/shape conversion (`push_positioned_elements`, text/image push), `presentation.validate()` — runs inside **one** `catch_unwind`.
- Any unwinding panic is converted **deterministically** to `EngineError::CodecError("office-oxide codec panicked while decoding PPTX")`.
- **The panic payload is never exposed** (line 147).

**Limits scope** (`crates/slides-pptx-adapter/src/limits.rs:21–37`, `office_oxide_codec.rs:57–58`):

```rust
pub struct Limits {
    pub max_input_bytes: u64,              // compressed input slice
    pub max_output_bytes: u64,             // serialized output
    pub max_slides: u32,                   // post-decode model
    pub max_shapes_per_slide: u32,         // post-decode model
    pub max_total_shapes: u64,             // post-decode model
    pub max_text_bytes: u32,               // post-decode model
    pub max_total_text_bytes: u64,         // post-decode model
    pub max_input_bytes_ceiling: u64,      // hard ceiling before allocation
    pub max_output_bytes_ceiling: u64,     // hard ceiling before allocation
    pub min_record_remaining_bytes: usize, // 7..=12 (RSLD001 header)
}
```

- `limits.validate()` (lines 75–113) enforces operational maxima < hard ceilings, `max_input_bytes >= MIN_DOCUMENT_BYTES (31)`, `max_slides != 0`, `min_record_remaining_bytes in 7..=12`.
- At decode entry: `limits.validate()?` then `limits.check_input_len(bytes.len())?` (lines 57–58) — **bounds the compressed input slice only**.
- Post-decode model size limits (`max_slides`, `max_total_shapes`, `max_total_text_bytes`, etc.) are checked during IR conversion (lines 70–76, 134–137, etc.).
- **Decompressed package / ZIP central-directory structure / inflated resource usage are NOT bounded** by these limits. `office_oxide::Document::from_reader` decompresses internally and `to_ir()` materializes the package in memory.

**Explicit residual risk statement (from PROGRESS.md T9 security-audit answers):**

> **(b) Central-directory / decompressed-size preflight — NOT IMPLEMENTED.** `Limits::max_input_bytes` bounds only the compressed input slice; `Document::from_reader` decompresses internally and `to_ir` materializes the package in memory, so a crafted ZIP with small compressed size can still inflate to a much larger working set (zip-bomb). This residual risk is the assigned follow-up guard (tracked in T1.5).

**Do not claim zip-bomb mitigation.** The current contract only provides:
- Compressed input byte ceiling (`max_input_bytes` + `max_input_bytes_ceiling`)
- Post-decode model size ceilings (slides, shapes, text bytes)
- Deterministic panic-to-`CodecError` conversion

A separate hardening task (T1.5) is required for ZIP central-directory preflight, decompression bombs, and inflated resource bounds.

### Key Source Paths

| Concept | Source |
|---------|--------|
| `OfficeOxideCodec::decode` with `catch_unwind` | `crates/slides-pptx-adapter/src/office_oxide_codec.rs:56–149` |
| `Limits` struct + validation | `crates/slides-pptx-adapter/src/limits.rs:21–113` |
| Decode entry limit checks | `crates/slides-pptx-adapter/src/office_oxide_codec.rs:57–58` |
| Post-decode model limit checks | `crates/slides-pptx-adapter/src/office_oxide_codec.rs:70–76, 134–137, 199–205, 217–225, 289` |
| PROGRESS.md residual risk acknowledgment | `docs/PROGRESS.md:22–25` |

---

## Non-Goals / Explicitly Out of Scope

- **No public API alterations** — this ADR documents the frozen contract as-is.
- **No Makepad GPU renderer** — `slides-canvas` stays headless logic; a future `slides-canvas-makepad` crate would implement `RenderBackend` + `CanvasHost`.
- **No ZIP-bomb mitigation** — explicitly not claimed; tracked as follow-up T1.5.
- **No adapter as `slides-ui` runtime dependency** — only dev-dependency for integration tests.

---

## Verification Gates (Current)

- `cargo fmt --all` — clean
- `cargo test -p slides-pptx-adapter` — **58 passed, 0 failed** (48 unit + 10 integration)
- `cargo clippy -p slides-pptx-adapter --all-targets -- -D warnings` — clean
- `cargo test --workspace` — all crates pass

---

## Follow-Up

- **T1.5**: ZIP-bomb mitigation — central-directory preflight, decompression bounds, inflated resource ceilings.
- **Architecture docs**: `docs/ARCHITECTURE.md` (crate graph, trait boundaries, data flow), `docs/ADR/0002-codec-strategy.md`.
- **CI**: GitHub Actions for `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`.

---

*This ADR is the single source of truth for the frozen contract described herein. Any modification to the four invariants requires a new ADR superseding this one.*